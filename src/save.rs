use std::collections::{BTreeMap, HashSet};
use std::fs;

use avian2d::prelude::*;
use bevy::ecs::system::SystemParam;
use bevy::picking::events::{Click, Pointer};
use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use crate::build::{
    build_structure, extract_blueprint, AttachPoint, Blueprint, BuiltModule, ModuleRegistry,
};
use crate::camera::CameraZoom;
use crate::enemy::build_enemy_ship;
use crate::health::{ModuleHealth, ShipHealth};
use crate::player::{Player, Seated};
use crate::ship::turret::Turret;
use crate::ship::{spawn_player_ship_base, PlayerShip, StructureRoot};
use crate::station::{spawn_space_station, SpaceStation};

/// Where saves are written (relative to the working dir for now).
const SAVE_PATH: &str = "save.ron";
/// Save schema version. Bump only when an existing chunk's format changes
/// incompatibly — adding or removing a whole chunk does not need a bump (a reader
/// tolerates a missing chunk; see [`SaveFile`]).
// v3: `ModuleSpec.turret` / `ItemKind::Turret` store a bare `TurretKind` (the fire
// arc moved into the `TurretDef`).
const SAVE_VERSION: u32 = 3;

/// Every authored content id the game defines, in spawn order. On load, ids here
/// that aren't in a save's `known_content_ids` are injected as new content. Keep in
/// sync with [`spawn_authored`].
const AUTHORED_CONTENT: &[&str] = &["player_ship", "enemy_ship", "station"];

/// Where a persistent structure came from. Distinguishes hand-authored content
/// (re-spawned from the game's content set, keyed by a stable `ContentId`) from
/// things the player built. Used both at runtime (gameplay rules — e.g. you can't
/// deconstruct authored stations) and by save/load reconciliation.
#[derive(Component, Clone, Serialize, Deserialize)]
pub enum Origin {
    /// Hand-authored content with a stable content id (e.g. `"station"`). World-setup
    /// re-creates these each load; the save only records which exist + their state.
    Authored(String),
    /// Created by the player at runtime; the save holds its full blueprint.
    PlayerBuilt,
}

/// A stable, unique id for a persistent instance — used for save cross-references
/// (docking, ownership, ...) instead of runtime `Entity` ids. Assigned by
/// [`assign_instance_ids`] and persisted so it stays valid across sessions.
#[derive(Component, Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct InstanceId(pub u64);

/// The next [`InstanceId`] to hand out (persisted in the save).
#[derive(Resource)]
pub struct NextInstanceId(pub u64);

impl Default for NextInstanceId {
    fn default() -> Self {
        Self(1) // 0 left free to mean "none"
    }
}

/// Give every persistent structure (anything with an [`Origin`]) a unique
/// [`InstanceId`] once it's spawned, so saves can reference it stably.
pub fn assign_instance_ids(
    mut commands: Commands,
    mut next: ResMut<NextInstanceId>,
    new: Query<Entity, (With<Origin>, Without<InstanceId>)>,
) {
    for entity in &new {
        // `try_insert`: on the load frame this and `load_structures` both touch the default
        // structures — they're queued for despawn here, so the insert may land after they're
        // gone. Skip silently rather than panic (the rebuilt structures get their own ids).
        commands.entity(entity).try_insert(InstanceId(next.0));
        next.0 += 1;
    }
}

/// The on-disk save: a schema version plus a set of named **chunks**, each an
/// independently-serialized blob. Every feature owns its own chunk — it writes it in a
/// system in [`PersistSet::Capture`] and restores it in [`PersistSet::Apply`] — so this
/// module never has to know what a feature persists. To add persistence for a new
/// system, give it `capture`/`apply` systems in those sets and have them call
/// [`SaveFile::write`]/[`SaveFile::read`] with their own key (see `camera::capture_camera`).
///
/// Chunk values are plain primitives only (no `glam`/`avian` types) so they stay
/// serde-friendly and stable across refactors.
#[derive(Resource, Default, Serialize, Deserialize)]
pub(crate) struct SaveFile {
    version: u32,
    chunks: BTreeMap<String, String>,
}

impl SaveFile {
    /// Store a feature's chunk under `key` (compact RON, one line per chunk).
    pub(crate) fn write<T: Serialize>(&mut self, key: &str, value: &T) {
        match ron::ser::to_string(value) {
            Ok(s) => {
                self.chunks.insert(key.to_string(), s);
            }
            Err(e) => error!("save: chunk '{key}' serialize failed: {e}"),
        }
    }

    /// Read a feature's chunk by `key`, or `None` if it's absent or unparseable (so a
    /// save written before the chunk existed simply yields the feature's default).
    pub(crate) fn read<T: serde::de::DeserializeOwned>(&self, key: &str) -> Option<T> {
        let raw = self.chunks.get(key)?;
        match ron::from_str(raw) {
            Ok(v) => Some(v),
            Err(e) => {
                error!("save: chunk '{key}' parse failed: {e}");
                None
            }
        }
    }
}

/// Whether a save or load is in flight this frame; gates the capture/apply sets.
#[derive(Resource, Default)]
pub(crate) struct PersistOp {
    save: bool,
    load: bool,
}

/// System sets features hook into: write your chunk in `Capture`, restore it in `Apply`.
/// `Capture` runs only while saving and `Apply` only while loading (see [`saving`] /
/// [`loading`]), so feature capture/apply systems can be plain unconditional systems.
#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum PersistSet {
    Capture,
    Apply,
}

/// Run condition: a save is in progress (gates [`PersistSet::Capture`]).
pub(crate) fn saving(op: Res<PersistOp>) -> bool {
    op.save
}

/// Run condition: a load is in progress (gates [`PersistSet::Apply`]).
pub(crate) fn loading(op: Res<PersistOp>) -> bool {
    op.load
}

/// The structures chunk — the world skeleton, owned by this module. It's special among
/// chunks because restoring it clears and respawns entities, so it's driven by
/// [`load_structures`] *before* the feature `Apply` systems (which may reference the
/// rebuilt structures by `InstanceId`).
///
/// Model: a full blueprint of every live structure (authored or built) plus the set of
/// authored content ids known at save time. On load we rebuild the saved structures and
/// then inject any authored content whose id is new (not in the known set) — so content
/// added in a patch appears in old saves, while destroyed authored content (in the known
/// set but with no saved structure) stays gone.
#[derive(Serialize, Deserialize, Default)]
struct StructuresChunk {
    next_instance_id: u64,
    known_content_ids: Vec<String>,
    structures: Vec<StructureSave>,
}

/// One persisted structure: its identity, full layout blueprint, and dynamic state.
#[derive(Serialize, Deserialize)]
struct StructureSave {
    instance_id: u64,
    origin: Origin,
    blueprint: Blueprint,
    body: BodyState,
}

/// Serializable rigid-body state (world frame). `health`/`health_max` are the ship
/// integrity pool (ships only).
#[derive(Serialize, Deserialize, Default, Clone)]
pub(crate) struct BodyState {
    pub(crate) pos: [f32; 2],
    /// Heading in radians.
    pub(crate) rot: f32,
    pub(crate) lin: [f32; 2],
    pub(crate) ang: f32,
    pub(crate) health: f32,
    pub(crate) health_max: f32,
}

/// Begin a save on `F5`: stamp the version, drop stale chunks, and flag the capture
/// pass. [`PersistSet::Capture`] systems then each write their chunk and [`commit_save`]
/// writes the file.
pub(crate) fn request_save(
    keyboard: Res<ButtonInput<KeyCode>>,
    mut op: ResMut<PersistOp>,
    mut file: ResMut<SaveFile>,
) {
    if keyboard.just_pressed(KeyCode::F5) {
        file.version = SAVE_VERSION;
        file.chunks.clear();
        op.save = true;
    }
}

/// Capture the structures chunk: a full blueprint + dynamic state of every live
/// structure, the instance counter, and the authored-content ids known right now.
pub(crate) fn capture_structures(
    next: Res<NextInstanceId>,
    mut file: ResMut<SaveFile>,
    roots: Query<(
        Entity,
        &Origin,
        &InstanceId,
        &Position,
        &Rotation,
        Option<&LinearVelocity>,
        Option<&AngularVelocity>,
        Option<&ShipHealth>,
    )>,
    // Queries `extract_blueprint` needs:
    player_ships: Query<(), With<PlayerShip>>,
    stations: Query<(), With<SpaceStation>>,
    modules: Query<(Entity, &BuiltModule, &StructureRoot, &ChildOf)>,
    attach: Query<&AttachPoint>,
    turrets: Query<(&ChildOf, &Turret)>,
    healths: Query<&ModuleHealth>,
) {
    let structures = roots
        .iter()
        .map(
            |(entity, origin, instance, pos, rot, lin, ang, health)| StructureSave {
                instance_id: instance.0,
                origin: origin.clone(),
                blueprint: extract_blueprint(
                    entity,
                    &player_ships,
                    &stations,
                    &modules,
                    &attach,
                    &turrets,
                    &healths,
                ),
                body: BodyState {
                    pos: pos.0.to_array(),
                    rot: rot.as_radians(),
                    lin: lin.map(|l| l.0.to_array()).unwrap_or_default(),
                    ang: ang.map(|a| a.0).unwrap_or(0.),
                    health: health.map(|h| h.current).unwrap_or(0.),
                    health_max: health.map(|h| h.max).unwrap_or(0.),
                },
            },
        )
        .collect();

    file.write(
        "structures",
        &StructuresChunk {
            next_instance_id: next.0,
            known_content_ids: AUTHORED_CONTENT.iter().map(|s| s.to_string()).collect(),
            structures,
        },
    );
}

/// Write the captured [`SaveFile`] to disk, ending the save pass.
pub(crate) fn commit_save(mut op: ResMut<PersistOp>, file: Res<SaveFile>) {
    op.save = false;
    match ron::ser::to_string_pretty(&*file, ron::ser::PrettyConfig::default()) {
        Ok(text) => match fs::write(SAVE_PATH, text) {
            Ok(()) => info!("game saved to {SAVE_PATH} ({} chunks)", file.chunks.len()),
            Err(e) => error!("save write failed: {e}"),
        },
        Err(e) => error!("save serialize failed: {e}"),
    }
}

/// Bundle of everything the load/new-game routines mutate, so the F9 system, the
/// startup auto-load, and the new-game button can share one implementation.
#[derive(SystemParam)]
pub(crate) struct WorldEdit<'w, 's> {
    commands: Commands<'w, 's>,
    meshes: ResMut<'w, Assets<Mesh>>,
    materials: ResMut<'w, Assets<ColorMaterial>>,
    next: ResMut<'w, NextInstanceId>,
    registry: Res<'w, ModuleRegistry>,
    turrets: Res<'w, crate::ship::turret::TurretRegistry>,
    origin: ResMut<'w, crate::origin::WorldOrigin>,
    zoom: ResMut<'w, CameraZoom>,
    camera_snap: ResMut<'w, crate::camera::CameraSnap>,
    pending_pilot: ResMut<'w, crate::player::PendingPilot>,
    pending_inventories: ResMut<'w, crate::inventory::PendingInventories>,
    roots: Query<'w, 's, Entity, With<Origin>>,
    player: Query<'w, 's, (Entity, &'static mut Position), With<Player>>,
}

/// Load on `F9`: parse the save file into [`SaveFile`] and flag the apply pass.
pub(crate) fn request_load(
    keyboard: Res<ButtonInput<KeyCode>>,
    mut op: ResMut<PersistOp>,
    mut file: ResMut<SaveFile>,
) {
    if keyboard.just_pressed(KeyCode::F9) {
        read_save_file(&mut file, &mut op);
    }
}

/// Auto-load the save once at startup if one exists (otherwise the freshly-spawned
/// default world stays). Runs in `PostStartup`, after the default world is spawned; the
/// load pipeline (structures + chunk apply) then runs on the first `Update` frame.
pub(crate) fn request_load_on_start(mut op: ResMut<PersistOp>, mut file: ResMut<SaveFile>) {
    if std::path::Path::new(SAVE_PATH).exists() {
        read_save_file(&mut file, &mut op);
    }
}

/// Read + version-check the save file into [`SaveFile`], flagging the load on success.
fn read_save_file(file: &mut SaveFile, op: &mut PersistOp) {
    let text = match fs::read_to_string(SAVE_PATH) {
        Ok(text) => text,
        Err(e) => {
            warn!("no save to load ({e})");
            return;
        }
    };
    let loaded: SaveFile = match ron::from_str(&text) {
        Ok(data) => data,
        Err(e) => {
            error!("load parse failed: {e}");
            return;
        }
    };
    if loaded.version != SAVE_VERSION {
        error!(
            "save version {} unsupported (this build expects {SAVE_VERSION})",
            loaded.version
        );
        return;
    }
    *file = loaded;
    op.load = true;
}

/// Rebuild the world skeleton from the structures chunk: clear the live structures,
/// respawn the saved ones from their blueprints, then inject any authored content new
/// since the save. Runs before [`PersistSet::Apply`], so chunk-apply systems (which may
/// reference structures by `InstanceId`) see the rebuilt world. The structures spawn via
/// deferred commands, so `InstanceId` cross-refs are re-linked by retry systems
/// (`apply_pending_pilot`, `keep_player_on_ship`), not here.
pub(crate) fn load_structures(
    file: Res<SaveFile>,
    mut commands: Commands,
    registry: Res<ModuleRegistry>,
    turrets: Res<crate::ship::turret::TurretRegistry>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<ColorMaterial>>,
    mut next: ResMut<NextInstanceId>,
    roots: Query<Entity, With<Origin>>,
) {
    let Some(chunk) = file.read::<StructuresChunk>("structures") else {
        return;
    };

    // Replace the live world's structures with the saved ones.
    for root in &roots {
        commands.entity(root).try_despawn();
    }
    next.0 = chunk.next_instance_id;
    for s in &chunk.structures {
        build_structure(
            &mut commands,
            &s.blueprint,
            &s.body,
            s.origin.clone(),
            s.instance_id,
            &registry,
            &turrets,
            &mut meshes,
            &mut materials,
        );
    }

    // Inject authored content that's new since the save (id not in its known set).
    let known: HashSet<&str> = chunk.known_content_ids.iter().map(String::as_str).collect();
    for &id in AUTHORED_CONTENT {
        if !known.contains(id) {
            spawn_authored(
                id,
                &registry,
                &turrets,
                &mut commands,
                &mut meshes,
                &mut materials,
            );
        }
    }
    info!(
        "world rebuilt from {SAVE_PATH} ({} structures)",
        chunk.structures.len()
    );
}

/// End the load pass once the chunk-apply systems have run.
pub(crate) fn commit_load(mut op: ResMut<PersistOp>) {
    op.load = false;
}

/// Discard the save and reset to a brand-new default world (the "New Game" button).
fn new_game(edit: &mut WorldEdit) {
    let _ = fs::remove_file(SAVE_PATH);
    for root in &edit.roots {
        edit.commands.entity(root).try_despawn();
    }
    edit.next.0 = NextInstanceId::default().0;
    for &id in AUTHORED_CONTENT {
        spawn_authored(
            id,
            &edit.registry,
            &edit.turrets,
            &mut edit.commands,
            &mut edit.meshes,
            &mut edit.materials,
        );
    }
    if let Ok((entity, mut pos)) = edit.player.single_mut() {
        pos.0 = Vec2::new(100., 0.); // the player ship's default location
        edit.commands.entity(entity).remove::<Seated>();
    }
    edit.pending_pilot.0 = None;
    // Fresh structures reuse instance ids from 1, so a stale pending list from an
    // earlier load must not apply to them.
    edit.pending_inventories.0.clear();
    // The default world spawns at authored (origin-zero) coordinates.
    edit.origin.0 = bevy::math::DVec2::ZERO;
    edit.zoom.0 = 1.0;
    edit.camera_snap.0 = true;
    info!("started a new game");
}

/// Marker for the on-screen "New Game" button.
#[derive(Component)]
pub(crate) struct NewGameButton;

/// Spawn the "New Game" button in the top-right corner, built from the UI toolkit.
/// Its click observer resets to a fresh default world.
pub(crate) fn spawn_new_game_button(mut commands: Commands, theme: Res<crate::ui::Theme>) {
    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                right: Val::Px(10.),
                top: Val::Px(10.),
                ..default()
            },
            GlobalZIndex(crate::ui::Z_HUD),
        ))
        .with_children(|parent| {
            parent
                .spawn((NewGameButton, crate::ui::button(&theme, "New Game")))
                .observe(|_: On<Pointer<Click>>, mut edit: WorldEdit| new_game(&mut edit));
        });
}

/// Spawn an authored structure by content id at its default state/position. Used to
/// inject content added since a save was made. Keep in sync with [`AUTHORED_CONTENT`].
fn spawn_authored(
    id: &str,
    registry: &ModuleRegistry,
    turrets: &crate::ship::turret::TurretRegistry,
    commands: &mut Commands,
    meshes: &mut ResMut<Assets<Mesh>>,
    materials: &mut ResMut<Assets<ColorMaterial>>,
) {
    match id {
        "player_ship" => {
            spawn_player_ship_base(
                Rectangle::new(150., 150.),
                commands.reborrow(),
                registry,
                turrets,
                meshes,
                materials,
            );
        }
        "enemy_ship" => build_enemy_ship(commands, registry, turrets, meshes, materials),
        "station" => {
            spawn_space_station(
                Vec2::new(1200., 0.),
                commands.reborrow(),
                registry,
                turrets,
                meshes,
                materials,
            );
        }
        other => warn!("unknown authored content id '{other}'"),
    }
}
