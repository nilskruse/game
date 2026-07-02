//! Structures-as-data: a serializable description of a built structure (a ship or
//! station) that can be extracted from the live world and (Stage 3) rebuilt. This is
//! the backbone of full persistence and of future player station-building.

use std::collections::HashMap;

use avian2d::prelude::*;
use bevy::{app::Propagate, prelude::*};
use serde::{Deserialize, Serialize};

use super::attach::{AttachPoint, AttachSlot};
use super::kinds::ModuleKind;
use super::registry::ModuleRegistry;
use super::spawn::BuiltModule;
use super::{build_buildable_side, mount, same_dir, spawn_build_console};
use crate::enemy::ShipAi;
use crate::faction::{Faction, InFaction};
use crate::health::{ModuleDisabled, ModuleHealth, ShipHealth};
use crate::save::{BodyState, Origin};
use crate::ship::turret::{spawn_turret, Turret, TurretKind, TurretRegistry};
use crate::ship::{PlayerShip, ShipBase, StructureRoot, ThrustCommand, ThrustControl};
use crate::station::SpaceStation;
use crate::world::WorldElement;

/// Which kind of root body a structure has. Determines the bare hull when rebuilding
/// (size, look, physics, faction, markers, console, thrust components).
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Debug)]
pub enum RootKind {
    PlayerShip,
    EnemyShip,
    Station,
}

/// One mounted module, described relative to its parent body.
#[derive(Serialize, Deserialize, Clone)]
pub struct ModuleSpec {
    /// Index into the blueprint's bodies: `0` = root, `1..` = earlier modules. Always
    /// less than this module's own index (parents come first), so rebuild can replay
    /// in order.
    pub parent: usize,
    /// Outward direction it was mounted along (axis-aligned unit, body-local).
    pub dir: [f32; 2],
    /// Body-local positions of the attach points it occupies (matched back to slots
    /// on rebuild).
    pub slots: Vec<[f32; 2]>,
    pub kind: ModuleKind,
    /// Turret installed on it (only for turret modules). The kind alone identifies the
    /// weapon — everything else (arc included) comes from its def on rebuild.
    pub turret: Option<TurretKind>,
    /// Current module durability.
    pub health: f32,
}

/// A full description of a structure: its root kind plus its modules in an order
/// where each module's parent appears before it.
#[derive(Serialize, Deserialize, Clone)]
pub struct Blueprint {
    pub root: RootKind,
    pub modules: Vec<ModuleSpec>,
}

/// Build a [`Blueprint`] from a live structure `root`. Walks the structure's modules,
/// numbering them so parents precede children, and records each module's placement
/// (parent, side, occupied slot positions), kind, turret, and current health.
pub(crate) fn extract_blueprint(
    root: Entity,
    player_ships: &Query<(), With<PlayerShip>>,
    stations: &Query<(), With<SpaceStation>>,
    modules: &Query<(Entity, &BuiltModule, &StructureRoot, &ChildOf)>,
    attach: &Query<&AttachPoint>,
    turrets: &Query<(&ChildOf, &Turret)>,
    healths: &Query<&ModuleHealth>,
) -> Blueprint {
    let root_kind = if player_ships.contains(root) {
        RootKind::PlayerShip
    } else if stations.contains(root) {
        RootKind::Station
    } else {
        RootKind::EnemyShip
    };

    // This structure's modules and each one's parent body (root or another module).
    let mut parent_of: HashMap<Entity, Entity> = HashMap::new();
    let mut module_entities: Vec<Entity> = Vec::new();
    for (entity, _built, structure_root, child_of) in modules.iter() {
        if structure_root.0 == root {
            parent_of.insert(entity, child_of.parent());
            module_entities.push(entity);
        }
    }

    // Order modules so every parent precedes its children (sort by depth from root).
    let mut depth_memo: HashMap<Entity, u32> = HashMap::new();
    module_entities.sort_by_key(|&e| (depth(e, root, &parent_of, &mut depth_memo), e.index()));

    // Body index: root = 0, modules numbered in that order.
    let mut index: HashMap<Entity, usize> = HashMap::new();
    index.insert(root, 0);
    for (i, &entity) in module_entities.iter().enumerate() {
        index.insert(entity, i + 1);
    }

    let mut specs = Vec::new();
    for &entity in &module_entities {
        let Ok((_, built, _, _)) = modules.get(entity) else {
            continue;
        };
        let parent_body = parent_of[&entity];
        let parent = *index.get(&parent_body).unwrap_or(&0);
        // Only points on the *parent* body describe how this module mounts. A bridged corridor
        // also records the far body's points in `BuiltModule.points` (for teardown) — exclude
        // those here, or the blueprint would carry slots from two bodies and fail to round-trip.
        let parent_points: Vec<&AttachPoint> = built
            .points
            .iter()
            .filter_map(|&p| attach.get(p).ok())
            .filter(|ap| ap.body == parent_body)
            .collect();
        let slots: Vec<[f32; 2]> = parent_points.iter().map(|ap| ap.local.to_array()).collect();
        let dir = parent_points
            .first()
            .map(|ap| ap.direction.to_array())
            .unwrap_or([0.0, 1.0]);
        let turret = turrets
            .iter()
            .find(|(child_of, _)| child_of.parent() == entity)
            .map(|(_, t)| t.kind());
        let health = healths.get(entity).map(|h| h.current).unwrap_or(0.0);
        specs.push(ModuleSpec {
            parent,
            dir,
            slots,
            kind: built.kind,
            turret,
            health,
        });
    }

    Blueprint {
        root: root_kind,
        modules: specs,
    }
}

/// Spawn a bare structure root for `kind` at `transform`: the hull entity with its
/// per-kind components + console, and its buildable sides built. Returns the root and
/// its sides (direction + slots) for mounting. No modules and no `Origin`/`InstanceId`
/// — `build_structure` adds those and the modules.
///
/// NB: this mirrors the root creation in `spawn_player_ship_base` / `spawn_enemy_ship`
/// / `spawn_space_station`; keep them in sync (or DRY them later).
fn spawn_root(
    kind: RootKind,
    transform: Transform,
    commands: &mut Commands,
    meshes: &mut ResMut<Assets<Mesh>>,
    materials: &mut ResMut<Assets<ColorMaterial>>,
) -> (Entity, Vec<(Vec2, Vec<AttachSlot>)>) {
    let size: u32 = if kind == RootKind::Station { 10 } else { 3 };
    let extent = size as f32 * super::UNIT;
    let rect = Rectangle::new(extent, extent);

    let root = match kind {
        RootKind::PlayerShip => commands
            .spawn((
                PlayerShip,
                Propagate(InFaction(Faction::Player)),
                ShipBase,
                ModuleHealth::new(300., 10.),
                ShipHealth::default(),
                ThrustControl::default(),
                ThrustCommand::default(),
                RigidBody::Dynamic,
                transform,
                Collider::from(rect),
                Mesh2d(meshes.add(rect)),
                MeshMaterial2d(materials.add(Color::srgb(1., 1., 0.))),
            ))
            .id(),
        RootKind::EnemyShip => commands
            .spawn((
                ShipBase,
                ShipAi { engage_range: 550. },
                Propagate(InFaction(Faction::Enemy)),
                ModuleHealth::new(300., 10.),
                ShipHealth::default(),
                ThrustControl::default(),
                ThrustCommand::default(),
                RigidBody::Dynamic,
                transform,
                Collider::from(rect),
                Mesh2d(meshes.add(rect)),
                MeshMaterial2d(materials.add(Color::srgb(0.8, 0.2, 0.2))),
            ))
            .id(),
        RootKind::Station => commands
            .spawn((
                SpaceStation,
                WorldElement,
                RigidBody::Static,
                transform,
                Collider::from(rect),
                Mesh2d(meshes.add(rect)),
                MeshMaterial2d(materials.add(Color::srgb(0.30, 0.34, 0.42))),
                Visibility::default(),
            ))
            .id(),
    };
    commands.entity(root).insert(Propagate(StructureRoot(root)));

    let half = rect.half_size;
    let mut sides = Vec::new();
    for dir in [Vec2::Y, Vec2::NEG_Y, Vec2::X, Vec2::NEG_X] {
        let slots = build_buildable_side(commands, root, half, size, dir, meshes, materials);
        sides.push((dir, slots));
    }

    // Engineering console (ships at -30, station at -40 local y); enemy has none.
    match kind {
        RootKind::PlayerShip => {
            spawn_build_console(root, Vec2::new(0., -30.), commands, meshes, materials);
        }
        RootKind::Station => {
            spawn_build_console(root, Vec2::new(0., -40.), commands, meshes, materials);
        }
        RootKind::EnemyShip => {}
    }

    (root, sides)
}

/// Rebuild a whole structure from a [`Blueprint`] and dynamic [`BodyState`]: spawn the
/// root, set its identity (`origin`/`instance`) and dynamic state, then replay every
/// module by matching its recorded slot positions to the parent body's slots and
/// `mount`ing it (installing turrets / setting health). Returns the root entity.
pub(crate) fn build_structure(
    commands: &mut Commands,
    blueprint: &Blueprint,
    body: &BodyState,
    origin: Origin,
    instance: u64,
    registry: &ModuleRegistry,
    turrets: &TurretRegistry,
    meshes: &mut ResMut<Assets<Mesh>>,
    materials: &mut ResMut<Assets<ColorMaterial>>,
) -> Entity {
    let transform = Transform::from_xyz(body.pos[0], body.pos[1], 0.)
        .with_rotation(Quat::from_rotation_z(body.rot));
    let (root, root_sides) = spawn_root(blueprint.root, transform, commands, meshes, materials);

    commands
        .entity(root)
        .insert((origin, crate::save::InstanceId(instance)));
    // Dynamic state — ships only (the station is static).
    if blueprint.root != RootKind::Station {
        commands.entity(root).insert((
            LinearVelocity(Vec2::new(body.lin[0], body.lin[1])),
            AngularVelocity(body.ang),
            ShipHealth {
                current: body.health,
                max: body.health_max,
            },
        ));
    }

    // bodies[i] = the entity + sides for blueprint body index i (0 = root); `None` for a
    // module that couldn't be rebuilt. Indexed by body number — so a skipped module pushes
    // `None` to keep later parents pointing at the right entity (a `continue` without a push
    // would shift every subsequent index and mount onto the wrong/stale body).
    let mut bodies: Vec<Option<(Entity, Vec<(Vec2, Vec<AttachSlot>)>)>> =
        vec![Some((root, root_sides))];

    for spec in &blueprint.modules {
        let dir = Vec2::from_array(spec.dir);
        let Some(Some((parent_entity, parent_sides))) = bodies.get(spec.parent).cloned() else {
            warn!(
                "blueprint parent index {} missing; skipping module",
                spec.parent
            );
            bodies.push(None);
            continue;
        };
        // The parent's slots on the matching side.
        let side_slots = parent_sides
            .iter()
            .find(|(d, _)| same_dir(*d, dir))
            .map(|(_, s)| s.clone())
            .unwrap_or_default();
        // Match each recorded slot position back to an actual slot.
        let picked: Vec<&AttachSlot> = spec
            .slots
            .iter()
            .filter_map(|want| {
                let want = Vec2::from_array(*want);
                side_slots.iter().find(|s| s.local.distance(want) < 0.5)
            })
            .collect();
        if picked.len() != spec.slots.len() {
            warn!("blueprint: couldn't match all slots for a module; skipping");
            bodies.push(None);
            continue;
        }

        let mounted = mount(
            commands,
            parent_entity,
            &picked,
            dir,
            spec.kind,
            registry,
            meshes,
            materials,
        );
        if let Some(turret_kind) = spec.turret {
            spawn_turret(
                mounted.module,
                turret_kind,
                turrets,
                commands.reborrow(),
                meshes,
                materials,
            );
        }
        // Restore durability (mount inserted a full one); disable if shot out.
        let (max, armor) = registry.get(spec.kind).durability;
        commands.entity(mounted.module).insert(ModuleHealth {
            current: spec.health,
            max,
            armor,
        });
        if spec.health <= 0. {
            commands.entity(mounted.module).insert(ModuleDisabled);
        }

        let sides: Vec<(Vec2, Vec<AttachSlot>)> = mounted
            .sides
            .iter()
            .map(|s| (s.direction, s.slots.clone()))
            .collect();
        bodies.push(Some((mounted.module, sides)));
    }

    root
}

/// Hops from `e` up the parent chain to `root` (root = 0). A module's parent is the
/// root or another module, so this terminates.
fn depth(
    e: Entity,
    root: Entity,
    parent_of: &HashMap<Entity, Entity>,
    memo: &mut HashMap<Entity, u32>,
) -> u32 {
    if e == root {
        return 0;
    }
    if let Some(&d) = memo.get(&e) {
        return d;
    }
    let d = match parent_of.get(&e) {
        Some(&parent) => depth(parent, root, parent_of, memo) + 1,
        None => 1,
    };
    memo.insert(e, d);
    d
}

/// Debug (F7): log each structure's extracted blueprint summary, to sanity-check
/// extraction before rebuild relies on it.
pub(crate) fn dump_blueprints(
    keyboard: Res<ButtonInput<KeyCode>>,
    roots: Query<(Entity, &crate::save::Origin)>,
    player_ships: Query<(), With<PlayerShip>>,
    stations: Query<(), With<SpaceStation>>,
    modules: Query<(Entity, &BuiltModule, &StructureRoot, &ChildOf)>,
    attach: Query<&AttachPoint>,
    turrets: Query<(&ChildOf, &Turret)>,
    healths: Query<&ModuleHealth>,
) {
    if !keyboard.just_pressed(KeyCode::F7) {
        return;
    }
    for (root, origin) in &roots {
        let bp = extract_blueprint(
            root,
            &player_ships,
            &stations,
            &modules,
            &attach,
            &turrets,
            &healths,
        );
        let name = match origin {
            crate::save::Origin::Authored(id) => id.as_str(),
            crate::save::Origin::PlayerBuilt => "<built>",
        };
        let turrets = bp.modules.iter().filter(|m| m.turret.is_some()).count();
        info!(
            "blueprint '{name}': root={:?}, {} modules ({} turrets)",
            bp.root,
            bp.modules.len(),
            turrets
        );
    }
}
