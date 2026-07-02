//! The simulation bubble: structures stay in the ECS forever, but only those near
//! the player are **Active** — full child hierarchy, physics, rendering. Beyond the
//! bubble a structure goes **Simulated**: its root entity survives with all its
//! state (identity, `Inventory`, `ShipHealth`, faction, frozen body state) plus a
//! captured [`StoredBlueprint`] and its f64 [`WorldPos`], while the child hierarchy
//! is despawned and physics/rendering are disabled. Coming back into range rebuilds
//! the children from the blueprint onto the *same* root entity — so `Entity`
//! references (dock partners, targets, future missions) stay valid across the
//! round-trip.
//!
//! [`Simulated`] is registered as a **disabling component** (Bevy entity disabling):
//! every query that doesn't mention it skips dormant roots automatically, so
//! gameplay systems need no changes. Systems that *want* dormant structures — the
//! future off-screen economy/AI tick, save capture, world teardown — opt in by
//! naming it (`Allow<Simulated>` / `Has<Simulated>` / `With<Simulated>`).
//! Avian's collider tree only special-cases Bevy's built-in `Disabled` in its
//! removal observers, so dormancy also inserts avian's own `RigidBodyDisabled` +
//! `ColliderDisabled` on the root (its explicitly supported path) rather than
//! relying on query filtering alone.
//!
//! Authority rule: while Simulated, [`WorldPos`] is the structure's position (f64,
//! origin-independent); while Active, the avian `Position`/`Transform` is (and
//! `WorldPos` is stale — recompute `origin + Position` when a world coordinate is
//! needed, as the save capture does).

use avian2d::prelude::{ColliderDisabled, Position, RigidBodyDisabled};
use bevy::math::DVec2;
use bevy::prelude::*;

use crate::build::{
    extract_blueprint, populate_structure, AttachPoint, Blueprint, BuiltModule, ModuleRegistry,
};
use crate::health::ModuleHealth;
use crate::origin::WorldOrigin;
use crate::player::Player;
use crate::save::Origin;
use crate::ship::turret::{Turret, TurretRegistry};
use crate::ship::{PlayerShip, StructureRoot};
use crate::station::SpaceStation;

/// A structure farther than this from the player goes dormant.
const DEACTIVATE_RADIUS: f32 = 5500.;
/// A dormant structure nearer than this wakes. Smaller than [`DEACTIVATE_RADIUS`]
/// so a structure on the boundary doesn't thrash between the states.
const ACTIVATE_RADIUS: f32 = 4500.;

/// Marker: this structure root is fully present — child hierarchy, physics,
/// rendering. The complement of [`Simulated`]; maintained by the transition systems
/// (and [`tag_new_structure`] for fresh spawns).
#[derive(Component)]
pub struct Active;

/// Marker: this structure root is dormant — background-simulation only. Registered
/// as a *disabling component*, so queries that don't mention it skip these entities.
#[derive(Component, Clone)]
pub struct Simulated;

/// The structure's world-space position (f64, floating-origin-independent).
/// Authoritative while [`Simulated`]; stale while [`Active`] (derive
/// `WorldOrigin + Position` instead — see the module docs).
#[derive(Component, Default)]
pub struct WorldPos(pub DVec2);

/// The dormant structure's captured layout, used to rebuild its children on waking.
/// Present only while [`Simulated`].
#[derive(Component)]
pub(crate) struct StoredBlueprint(pub(crate) Blueprint);

pub struct BubblePlugin;

impl Plugin for BubblePlugin {
    fn build(&self, app: &mut App) {
        app.world_mut().register_disabling_component::<Simulated>();
        app.add_observer(tag_new_structure);
        app.add_systems(Update, (deactivate_structures, activate_structures));
    }
}

/// Every freshly created structure root (anything gaining an [`Origin`]) starts
/// Active with a (stale, zero) [`WorldPos`] — uniform across authored startup
/// spawns, save-loaded rebuilds and future player-built structures.
fn tag_new_structure(add: On<Add, Origin>, mut commands: Commands) {
    commands
        .entity(add.entity)
        .insert((Active, WorldPos::default()));
}

/// Put structures beyond [`DEACTIVATE_RADIUS`] of the player to sleep: capture the
/// blueprint + world position onto the root, despawn the children, disable physics
/// and rendering, and swap [`Active`] for [`Simulated`]. The player's own ship is
/// never dormed (the player is the bubble's anchor).
pub(crate) fn deactivate_structures(
    mut commands: Commands,
    origin: Res<WorldOrigin>,
    player: Query<&Position, With<Player>>,
    roots: Query<(Entity, &Position), (With<Origin>, With<Active>, Without<PlayerShip>)>,
    children: Query<&Children>,
    // Queries `extract_blueprint` needs:
    player_ships: Query<(), With<PlayerShip>>,
    stations: Query<(), With<SpaceStation>>,
    modules: Query<(Entity, &BuiltModule, &StructureRoot, &ChildOf)>,
    attach: Query<&AttachPoint>,
    turrets: Query<(&ChildOf, &Turret)>,
    healths: Query<&ModuleHealth>,
) {
    let Ok(player_pos) = player.single() else {
        return;
    };
    for (root, pos) in &roots {
        if pos.0.distance(player_pos.0) < DEACTIVATE_RADIUS {
            continue;
        }
        let blueprint = extract_blueprint(
            root,
            &player_ships,
            &stations,
            &modules,
            &attach,
            &turrets,
            &healths,
        );
        if let Ok(kids) = children.get(root) {
            for child in kids.iter() {
                commands.entity(child).despawn();
            }
        }
        commands.entity(root).remove::<Active>().insert((
            Simulated,
            StoredBlueprint(blueprint),
            WorldPos(origin.0 + pos.0.as_dvec2()),
            RigidBodyDisabled,
            ColliderDisabled,
            Visibility::Hidden,
        ));
        info!("structure {root} left the simulation bubble (dormant)");
    }
}

/// Wake dormant structures within [`ACTIVATE_RADIUS`] of the player: place the root
/// at its world position in the current origin frame, rebuild its children from the
/// stored blueprint, re-enable physics/rendering and swap back to [`Active`].
pub(crate) fn activate_structures(
    mut commands: Commands,
    origin: Res<WorldOrigin>,
    player: Query<&Position, (With<Player>, Without<Simulated>)>,
    mut dormant: Query<
        (
            Entity,
            &WorldPos,
            &StoredBlueprint,
            &mut Transform,
            &mut Position,
        ),
        With<Simulated>,
    >,
    registry: Res<ModuleRegistry>,
    turret_defs: Res<TurretRegistry>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<ColorMaterial>>,
) {
    let Ok(player_pos) = player.single() else {
        return;
    };
    for (root, world_pos, blueprint, mut transform, mut position) in &mut dormant {
        let local = (world_pos.0 - origin.0).as_vec2();
        if local.distance(player_pos.0) > ACTIVATE_RADIUS {
            continue;
        }
        // The origin may have rebased any number of times while dormant; re-derive
        // the local pose from the authoritative world position.
        position.0 = local;
        transform.translation.x = local.x;
        transform.translation.y = local.y;
        populate_structure(
            &mut commands,
            root,
            &blueprint.0,
            &registry,
            &turret_defs,
            &mut meshes,
            &mut materials,
        );
        commands
            .entity(root)
            .remove::<(
                Simulated,
                StoredBlueprint,
                RigidBodyDisabled,
                ColliderDisabled,
            )>()
            .insert((Active, Visibility::Visible));
        info!("structure {root} entered the simulation bubble (active)");
    }
}
