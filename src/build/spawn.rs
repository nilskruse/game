use avian2d::prelude::*;
use bevy::prelude::*;

use crate::ship::GameLayer;

use super::attach::{build_buildable_side, AttachPoint, AttachSlot};
use super::kinds::ModuleKind;
use super::{same_dir, HULL, UNIT, WALL};

/// Airlock door color (sealed bulkhead).
const DOOR_COLOR: Color = Color::srgb(0.80, 0.30, 0.25);

/// A module that was built onto a body, with what it needs to be deconstructed.
#[derive(Component)]
pub(crate) struct BuiltModule {
    /// Attach points on the parent body this module occupies (freed on removal).
    pub(crate) points: Vec<Entity>,
    /// Doorway panels this module opened (disabled), to re-seal on removal.
    pub(crate) panels: Vec<Entity>,
    /// Square footprint side length, for cursor hit-testing during removal.
    pub(crate) extent: f32,
}

/// A barrier on a docking airlock that opens when the airlock's `port` docks: the
/// visible door (blocks the player) and the structural collider (blocks hulls).
#[derive(Component)]
pub(crate) struct AirlockDoor {
    port: Entity,
}

/// Open each airlock barrier whose port is docked, close it otherwise. This covers
/// both the visible door (which blocks the player) and the structural collider
/// (which blocks other hulls) — disabling them lets two docked airlocks meet and
/// the crew cross; re-enabling them re-seals the airlock in flight.
pub(crate) fn update_airlock_doors(
    mut commands: Commands,
    ports: Query<&crate::docking::DockingPort>,
    mut doors: Query<(Entity, &AirlockDoor, &mut Visibility, Has<ColliderDisabled>)>,
) {
    for (entity, door, mut visibility, disabled) in &mut doors {
        let docked = ports.get(door.port).map_or(false, |p| p.docked_to.is_some());
        if docked && !disabled {
            commands.entity(entity).insert(ColliderDisabled);
            *visibility = Visibility::Hidden;
        } else if !docked && disabled {
            commands.entity(entity).remove::<ColliderDisabled>();
            *visibility = Visibility::Visible;
        }
    }
}

/// Spawn a module of `kind` as a child of `body`. `edge` is the body-local
/// midpoint of the covered attach points (on the hull edge); `direction` points
/// outward. Dispatches on kind: docking port (thin sensor collar at the edge),
/// walkable room, plain solid block, or a solid block with a turret on top.
pub(crate) fn spawn_module_at(
    commands: &mut Commands,
    body: Entity,
    edge: Vec2,
    direction: Vec2,
    kind: ModuleKind,
    meshes: &mut ResMut<Assets<Mesh>>,
    materials: &mut ResMut<Assets<ColorMaterial>>,
) -> Entity {
    if kind.is_dock() {
        return spawn_dock_module(commands, body, edge, direction, meshes, materials);
    }

    // Square modules center half their depth outside the edge.
    let center = edge + direction * (kind.extent() / 2.);
    if kind.walkable() {
        return spawn_module_room(commands, body, center, direction, kind, meshes, materials);
    }

    let module = spawn_solid_module(commands, body, center, kind, meshes, materials);
    if kind.mounts_turret() {
        // Faction is inherited from the hull via hierarchy propagation, so the
        // turret picks up the player faction just like the old hardcoded one.
        let turret = crate::ship::turret::spawn_turret(module, commands.reborrow(), meshes, materials);
        commands
            .entity(turret)
            .insert(Transform::from_xyz(0., 0., 0.6));
    }
    module
}

/// Occupy `slots` and mount a module of `kind` on them during construction. Lets
/// ship setup reuse the same module path as click-to-build placement.
fn mount_preplaced(
    commands: &mut Commands,
    body: Entity,
    slots: &[&AttachSlot],
    direction: Vec2,
    kind: ModuleKind,
    meshes: &mut ResMut<Assets<Mesh>>,
    materials: &mut ResMut<Assets<ColorMaterial>>,
) {
    let mut sum = Vec2::ZERO;
    let mut opened = Vec::new();
    let mut occupied = Vec::new();
    for slot in slots {
        commands.entity(slot.entity).insert(AttachPoint {
            occupied: true,
            body,
            local: slot.local,
            direction,
            door_panel: slot.panel,
        });
        if kind.opens_doorway() {
            commands
                .entity(slot.panel)
                .insert((ColliderDisabled, Visibility::Hidden));
            opened.push(slot.panel);
        }
        sum += slot.local;
        occupied.push(slot.entity);
    }
    let edge = sum / slots.len() as f32;
    let module = spawn_module_at(commands, body, edge, direction, kind, meshes, materials);
    commands.entity(module).insert(BuiltModule {
        points: occupied,
        panels: opened,
        extent: kind.extent(),
    });
}

/// Pre-mount a turret module on `slot` during ship construction.
pub fn mount_preplaced_turret(
    commands: &mut Commands,
    body: Entity,
    slot: &AttachSlot,
    direction: Vec2,
    meshes: &mut ResMut<Assets<Mesh>>,
    materials: &mut ResMut<Assets<ColorMaterial>>,
) {
    mount_preplaced(commands, body, &[slot], direction, ModuleKind::Turret, meshes, materials);
}

/// Pre-mount a docking-port module spanning `slots` during construction.
pub fn mount_preplaced_dock(
    commands: &mut Commands,
    body: Entity,
    slots: &[&AttachSlot],
    direction: Vec2,
    meshes: &mut ResMut<Assets<Mesh>>,
    materials: &mut ResMut<Assets<ColorMaterial>>,
) {
    mount_preplaced(commands, body, slots, direction, ModuleKind::Dock, meshes, materials);
}

/// Attach a solid (non-walkable) module block. The hull doorway stays sealed and
/// the block exposes no further attachment points.
fn spawn_solid_module(
    commands: &mut Commands,
    body: Entity,
    center: Vec2,
    kind: ModuleKind,
    meshes: &mut ResMut<Assets<Mesh>>,
    materials: &mut ResMut<Assets<ColorMaterial>>,
) -> Entity {
    let extent = kind.extent();
    let rect = Rectangle::new(extent, extent);
    commands
        .spawn((
            ChildOf(body),
            Transform::from_xyz(center.x, center.y, 0.4),
            Collider::from(rect),
            Mesh2d(meshes.add(rect)),
            MeshMaterial2d(materials.add(kind.color())),
        ))
        .id()
}

/// Spawn a size-1 docking airlock as a child of `body`. `edge` is the hull-edge
/// midpoint and `direction` points outward. The airlock is a small walkable room,
/// open on both the ship-facing side and the outward side (where a docking-port
/// collar sits), with solid walls on the two perpendicular sides — so the crew can
/// board straight through it once docked.
///
/// Shared by ship build/placement and station construction so both ends of a dock
/// are the same component.
pub fn spawn_dock_module(
    commands: &mut Commands,
    body: Entity,
    edge: Vec2,
    direction: Vec2,
    meshes: &mut ResMut<Assets<Mesh>>,
    materials: &mut ResMut<Assets<ColorMaterial>>,
) -> Entity {
    let extent = UNIT;
    let center = edge + direction * (extent / 2.);
    let module = commands
        .spawn((
            ChildOf(body),
            Transform::from_xyz(center.x, center.y, 0.),
            Visibility::default(),
        ))
        .id();

    // Floor.
    let floor = Rectangle::new(extent - WALL, extent - WALL);
    commands.spawn((
        ChildOf(module),
        Transform::from_xyz(0., 0., -0.5),
        Mesh2d(meshes.add(floor)),
        MeshMaterial2d(materials.add(HULL)),
    ));

    // Solid walls on the two sides perpendicular to the entry/exit axis; both the
    // ship-facing and outward sides are left open.
    let half = extent / 2.;
    let layers = CollisionLayers::new(GameLayer::Walls, [GameLayer::Player, GameLayer::Default]);
    for normal in [Vec2::Y, Vec2::NEG_Y, Vec2::X, Vec2::NEG_X] {
        if same_dir(normal, direction) || same_dir(normal, -direction) {
            continue;
        }
        let horizontal = normal.x == 0.0;
        let pos = normal * (half - WALL / 2.);
        let size = if horizontal {
            Vec2::new(extent, WALL)
        } else {
            Vec2::new(WALL, extent)
        };
        let rect = Rectangle::new(size.x, size.y);
        commands.spawn((
            ChildOf(module),
            Transform::from_xyz(pos.x, pos.y, 0.),
            Collider::from(rect),
            Mesh2d(meshes.add(rect)),
            MeshMaterial2d(materials.add(HULL)),
            layers,
        ));
    }

    // Docking-port collar at the outward face. A port faces along its local +Y,
    // so rotate +Y onto `direction`.
    let angle = (-direction.x).atan2(direction.y);
    let port = crate::docking::spawn_docking_port(
        module,
        direction * (half - 1.),
        angle,
        commands.reborrow(),
        meshes,
        materials,
    );

    // Outer door across the opening: sealed (blocks the player) until the port
    // docks, then `update_airlock_doors` opens it.
    let horizontal = direction.x == 0.0;
    let door_pos = direction * (half - WALL / 2.);
    let door_size = if horizontal {
        Vec2::new(extent, WALL)
    } else {
        Vec2::new(WALL, extent)
    };
    let door_rect = Rectangle::new(door_size.x, door_size.y);
    commands.spawn((
        AirlockDoor { port },
        ChildOf(module),
        Transform::from_xyz(door_pos.x, door_pos.y, 0.1),
        Collider::from(door_rect),
        Mesh2d(meshes.add(door_rect)),
        MeshMaterial2d(materials.add(DOOR_COLOR)),
        layers,
    ));

    // Structural collider (default layer, like the hull) so the airlock is solid
    // against other structures while flying — it blocks hulls but not the player.
    // Tagged as an airlock barrier so it's disabled when docked, letting two
    // airlocks meet at the interface without the solver shoving them apart.
    commands.spawn((
        AirlockDoor { port },
        ChildOf(module),
        Transform::from_xyz(0., 0., 0.),
        Collider::from(Rectangle::new(extent, extent)),
        Visibility::Visible,
    ));

    module
}

/// Spawn a walkable module room as a child of `body` at body-local `center`. Its
/// side facing back toward the body (normal `-direction`) is left fully open so it
/// connects through the body's doorways; its other three sides are buildable, so
/// the room can be extended further.
fn spawn_module_room(
    commands: &mut Commands,
    body: Entity,
    center: Vec2,
    direction: Vec2,
    kind: ModuleKind,
    meshes: &mut ResMut<Assets<Mesh>>,
    materials: &mut ResMut<Assets<ColorMaterial>>,
) -> Entity {
    let size = kind.size_units();
    let extent = kind.extent();
    let module = commands
        .spawn((
            ChildOf(body),
            Transform::from_xyz(center.x, center.y, 0.),
            Visibility::default(),
        ))
        .id();

    // Floor.
    let floor = Rectangle::new(extent - WALL, extent - WALL);
    commands.spawn((
        ChildOf(module),
        Transform::from_xyz(0., 0., -0.5),
        Mesh2d(meshes.add(floor)),
        MeshMaterial2d(materials.add(kind.color())),
    ));

    // Buildable walls on every side except the open one facing the parent body.
    let half = Vec2::splat(extent / 2.);
    for normal in [Vec2::Y, Vec2::NEG_Y, Vec2::X, Vec2::NEG_X] {
        if same_dir(normal, -direction) {
            continue;
        }
        build_buildable_side(commands, module, half, size, normal, meshes, materials);
    }

    // Structural collider (default layer, like the hull) so the room is solid
    // against other structures. It blocks hulls but not the player.
    commands.spawn((
        ChildOf(module),
        Transform::from_xyz(0., 0., 0.),
        Collider::from(Rectangle::new(extent, extent)),
    ));

    module
}
