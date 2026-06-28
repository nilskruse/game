use avian2d::prelude::*;
use bevy::prelude::*;

use crate::ship::GameLayer;

use super::attach::{build_buildable_side, AttachPoint, AttachSlot};
use super::kinds::{Footprint, ModuleKind};
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
    /// Axis-aligned footprint size (world units), for cursor hit-testing on removal.
    pub(crate) size: Vec2,
}

/// A barrier on a docking airlock that opens when the airlock's `port` docks: the
/// visible door (blocks the player) and the structural collider (blocks hulls).
#[derive(Component)]
pub(crate) struct AirlockDoor {
    port: Entity,
}

/// One free side of a freshly mounted module: its outward `direction` and the
/// attach slots exposed along it, for chaining further modules.
pub(crate) struct MountedSide {
    pub(crate) direction: Vec2,
    pub(crate) slots: Vec<AttachSlot>,
}

/// A module mounted onto a structure during construction, plus the sides it
/// exposes. Solid and dock modules expose nothing. Returned by [`mount`] so code
/// that assembles a structure (the station) can attach modules onto modules.
pub(crate) struct Mounted {
    pub(crate) module: Entity,
    pub(crate) sides: Vec<MountedSide>,
}

impl Mounted {
    /// The attach slots exposed on the side pointing `direction` (empty if none).
    pub(crate) fn side(&self, direction: Vec2) -> &[AttachSlot] {
        self.sides
            .iter()
            .find(|s| same_dir(s.direction, direction))
            .map_or(&[], |s| &s.slots)
    }
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
        let docked = ports.get(door.port).is_ok_and(|p| p.docked_to.is_some());
        if docked && !disabled {
            commands.entity(entity).insert(ColliderDisabled);
            *visibility = Visibility::Hidden;
        } else if !docked && disabled {
            commands.entity(entity).remove::<ColliderDisabled>();
            *visibility = Visibility::Visible;
        }
    }
}

/// Spawn a module of `kind` with the given `footprint` as a child of `body`.
/// `edge` is the body-local midpoint of the covered attach points (on the hull
/// edge); `direction` points outward. Dispatches on kind: docking port (thin
/// sensor collar at the edge), walkable room, plain solid block, or a solid block
/// with a turret on top.
pub(crate) fn spawn_module_at(
    commands: &mut Commands,
    body: Entity,
    edge: Vec2,
    direction: Vec2,
    kind: ModuleKind,
    footprint: Footprint,
    meshes: &mut ResMut<Assets<Mesh>>,
    materials: &mut ResMut<Assets<ColorMaterial>>,
) -> Entity {
    spawn_module_sided(
        commands, body, edge, direction, kind, footprint, meshes, materials,
    )
    .module
}

/// Spawn a module and report the sides it exposes (see [`Mounted`]). Dispatches on
/// kind: docking port (sensor collar, no exposed sides), walkable room, plain solid
/// block, or a solid block with a turret on top (neither exposes sides).
fn spawn_module_sided(
    commands: &mut Commands,
    body: Entity,
    edge: Vec2,
    direction: Vec2,
    kind: ModuleKind,
    footprint: Footprint,
    meshes: &mut ResMut<Assets<Mesh>>,
    materials: &mut ResMut<Assets<ColorMaterial>>,
) -> Mounted {
    if kind.is_dock() {
        let module = spawn_dock_module(commands, body, edge, direction, meshes, materials);
        return Mounted {
            module,
            sides: Vec::new(),
        };
    }

    // A module centers half its depth outside the edge.
    let center = edge + direction * (footprint.depth as f32 * UNIT / 2.);
    if kind.walkable() {
        let mounted = spawn_module_room(
            commands, body, center, direction, kind, footprint, meshes, materials,
        );
        if kind.has_seat() {
            spawn_pilot_seat(commands, mounted.module, meshes, materials);
        }
        return mounted;
    }

    let module = spawn_solid_module(
        commands, body, center, direction, kind, footprint, meshes, materials,
    );
    if kind.mounts_turret() {
        // Faction is inherited from the hull via hierarchy propagation, so the
        // turret picks up the player faction just like the old hardcoded one.
        let turret =
            crate::ship::turret::spawn_turret(module, commands.reborrow(), meshes, materials);
        commands
            .entity(turret)
            .insert(Transform::from_xyz(0., 0., 0.6));
    }
    Mounted {
        module,
        sides: Vec::new(),
    }
}

/// Occupy `slots` on `body` and mount a module of `kind` across them, opening the
/// covered doorways if the kind passes through. Returns the module and the sides it
/// exposes (see [`Mounted`]). This is the construction-time counterpart to
/// click-to-build placement, used to assemble the ship and station from modules.
pub(crate) fn mount(
    commands: &mut Commands,
    body: Entity,
    slots: &[&AttachSlot],
    direction: Vec2,
    kind: ModuleKind,
    meshes: &mut ResMut<Assets<Mesh>>,
    materials: &mut ResMut<Assets<ColorMaterial>>,
) -> Mounted {
    let footprint = kind.footprint();
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
    let mounted = spawn_module_sided(
        commands, body, edge, direction, kind, footprint, meshes, materials,
    );
    commands.entity(mounted.module).insert(BuiltModule {
        points: occupied,
        panels: opened,
        size: footprint.world_size(direction),
    });
    mounted
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
    mount(
        commands,
        body,
        &[slot],
        direction,
        ModuleKind::Turret,
        meshes,
        materials,
    );
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
    mount(
        commands,
        body,
        slots,
        direction,
        ModuleKind::Dock,
        meshes,
        materials,
    );
}

/// Pre-mount a cockpit module on `slot` during ship construction.
pub fn mount_preplaced_cockpit(
    commands: &mut Commands,
    body: Entity,
    slot: &AttachSlot,
    direction: Vec2,
    meshes: &mut ResMut<Assets<Mesh>>,
    materials: &mut ResMut<Assets<ColorMaterial>>,
) {
    mount(
        commands,
        body,
        &[slot],
        direction,
        ModuleKind::Cockpit,
        meshes,
        materials,
    );
}

/// Spawn the pilot seat at the center of a cockpit `module`. No collider, so the
/// player walks onto it and sits with E.
fn spawn_pilot_seat(
    commands: &mut Commands,
    module: Entity,
    meshes: &mut ResMut<Assets<Mesh>>,
    materials: &mut ResMut<Assets<ColorMaterial>>,
) {
    let seat = Circle::new(8.);
    commands.spawn((
        crate::ship::PilotSeat,
        ChildOf(module),
        Transform::from_xyz(0., 0., 0.5),
        Mesh2d(meshes.add(seat)),
        MeshMaterial2d(materials.add(Color::srgb(0., 0.6, 1.))),
    ));
}

/// Attach a solid (non-walkable) module block. The hull doorway stays sealed and
/// the block exposes no further attachment points.
fn spawn_solid_module(
    commands: &mut Commands,
    body: Entity,
    center: Vec2,
    direction: Vec2,
    kind: ModuleKind,
    footprint: Footprint,
    meshes: &mut ResMut<Assets<Mesh>>,
    materials: &mut ResMut<Assets<ColorMaterial>>,
) -> Entity {
    let size = footprint.world_size(direction);
    let rect = Rectangle::new(size.x, size.y);
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
/// connects through the body's doorways. The opposite (outward) end is always
/// buildable so the room can be extended further. The two long sides are buildable
/// for a square room, but solid walls for an elongated one (e.g. a hallway).
fn spawn_module_room(
    commands: &mut Commands,
    body: Entity,
    center: Vec2,
    direction: Vec2,
    kind: ModuleKind,
    footprint: Footprint,
    meshes: &mut ResMut<Assets<Mesh>>,
    materials: &mut ResMut<Assets<ColorMaterial>>,
) -> Mounted {
    let size = footprint.world_size(direction);
    let half = size / 2.;
    let module = commands
        .spawn((
            ChildOf(body),
            Transform::from_xyz(center.x, center.y, 0.),
            Visibility::default(),
        ))
        .id();

    // Floor.
    let floor = Rectangle::new(size.x - WALL, size.y - WALL);
    commands.spawn((
        ChildOf(module),
        Transform::from_xyz(0., 0., -0.5),
        Mesh2d(meshes.add(floor)),
        MeshMaterial2d(materials.add(kind.color())),
    ));

    // Record the buildable sides this room exposes (in module-local directions),
    // so a structure assembler can chain further modules onto them.
    let mut sides = Vec::new();
    for normal in [Vec2::Y, Vec2::NEG_Y, Vec2::X, Vec2::NEG_X] {
        if same_dir(normal, -direction) {
            // Open side facing the parent body.
            continue;
        }
        if same_dir(normal, direction) {
            // Outward end: always buildable, exposing `width` attach points.
            let slots = build_buildable_side(
                commands,
                module,
                half,
                footprint.width,
                normal,
                meshes,
                materials,
            );
            sides.push(MountedSide {
                direction: normal,
                slots,
            });
        } else if footprint.is_square() {
            // Long sides of a square room are buildable too (exposing `depth`).
            let slots = build_buildable_side(
                commands,
                module,
                half,
                footprint.depth,
                normal,
                meshes,
                materials,
            );
            sides.push(MountedSide {
                direction: normal,
                slots,
            });
        } else {
            // Long sides of an elongated room are solid walls.
            spawn_solid_wall(commands, module, half, normal, meshes, materials);
        }
    }

    // Structural collider (default layer, like the hull) so the room is solid
    // against other structures. It blocks hulls but not the player.
    commands.spawn((
        ChildOf(module),
        Transform::from_xyz(0., 0., 0.),
        Collider::from(Rectangle::new(size.x, size.y)),
    ));

    Mounted { module, sides }
}

/// Spawn a single solid wall covering one full side of a module (half-extents
/// `half`), with outward `normal`. Used for the long sides of elongated rooms.
fn spawn_solid_wall(
    commands: &mut Commands,
    module: Entity,
    half: Vec2,
    normal: Vec2,
    meshes: &mut ResMut<Assets<Mesh>>,
    materials: &mut ResMut<Assets<ColorMaterial>>,
) {
    // The side runs along x when its outward normal is vertical.
    let horizontal = normal.x == 0.0;
    let l = if horizontal { half.x } else { half.y };
    let perp = (if horizontal { half.y } else { half.x }) - WALL / 2.;
    let sign = if horizontal {
        normal.y.signum()
    } else {
        normal.x.signum()
    };
    let base_perp = sign * perp;
    let (wsize, wpos) = if horizontal {
        (Vec2::new(2. * l, WALL), Vec2::new(0., base_perp))
    } else {
        (Vec2::new(WALL, 2. * l), Vec2::new(base_perp, 0.))
    };
    let rect = Rectangle::new(wsize.x, wsize.y);
    commands.spawn((
        ChildOf(module),
        Collider::from(rect),
        Transform::from_xyz(wpos.x, wpos.y, 0.),
        Mesh2d(meshes.add(rect)),
        MeshMaterial2d(materials.add(HULL)),
        CollisionLayers::new(GameLayer::Walls, [GameLayer::Player, GameLayer::Default]),
    ));
}
