use avian2d::prelude::*;
use bevy::{app::Propagate, prelude::*};

use crate::build::{build_buildable_side, mount, AttachSlot, ModuleKind, Mounted, UNIT};
use crate::ship::StructureRoot;
use crate::{interaction::spawn_console, world::WorldElement};

/// Marker for a space station's root entity.
#[derive(Component)]
pub struct SpaceStation;

/// Floor color of the central hub (the station's root room).
const HUB_FLOOR: Color = Color::srgb(0.30, 0.34, 0.42);

/// Build the player-accessible station entirely out of standard modules — the same
/// kinds you can build onto a ship. A large central hub (a buildable body, like the
/// ship base) has rooms, corridors, docking ports and equipment mounted onto its
/// sides through the shared `mount` path, and modules chained onto those.
///
/// It dwarfs the ship: a 6-wide hub with rows of cargo holds, long docking arms,
/// and a bank of equipment — but built at the same `UNIT` scale, so a room is still
/// a room you walk through.
///
/// Built hierarchically (station hub -> module -> walls), mirroring the ship.
pub fn spawn_space_station(
    position: Vec2,
    mut commands: Commands,
    meshes: &mut ResMut<Assets<Mesh>>,
    materials: &mut ResMut<Assets<ColorMaterial>>,
) -> Entity {
    // Central hub: a size-6 square body, the backbone everything mounts onto. Like
    // the ship base, its full-square collider blocks other hulls (not the player),
    // and its buildable sides enclose a walkable room with doorways.
    let hub_size = 6u32;
    let extent = hub_size as f32 * UNIT;
    let half = Vec2::splat(extent / 2.);
    let rect = Rectangle::new(extent, extent);
    let station = commands
        .spawn((
            SpaceStation,
            WorldElement,
            RigidBody::Static,
            Transform::from_xyz(position.x, position.y, 0.),
            Collider::from(rect),
            Mesh2d(meshes.add(rect)),
            MeshMaterial2d(materials.add(HUB_FLOOR)),
            Visibility::default(),
        ))
        .id();
    // Tag every part of the station with its root, so docking ports (and anything
    // else) resolve their structure with an O(1) read instead of a hierarchy walk.
    commands
        .entity(station)
        .insert(Propagate(StructureRoot(station)));

    let up = build_buildable_side(
        &mut commands,
        station,
        half,
        hub_size,
        Vec2::Y,
        meshes,
        materials,
    );
    let down = build_buildable_side(
        &mut commands,
        station,
        half,
        hub_size,
        Vec2::NEG_Y,
        meshes,
        materials,
    );
    let left = build_buildable_side(
        &mut commands,
        station,
        half,
        hub_size,
        Vec2::NEG_X,
        meshes,
        materials,
    );
    let right = build_buildable_side(
        &mut commands,
        station,
        half,
        hub_size,
        Vec2::X,
        meshes,
        materials,
    );

    // North: a central cargo hold flanked by two long docking arms.
    cargo(
        &mut commands,
        station,
        &up[2],
        &up[3],
        Vec2::Y,
        meshes,
        materials,
    );
    corridor_dock(
        &mut commands,
        station,
        &up[0],
        Vec2::Y,
        3,
        meshes,
        materials,
    );
    corridor_dock(
        &mut commands,
        station,
        &up[5],
        Vec2::Y,
        3,
        meshes,
        materials,
    );

    // South: a bank of three cargo holds, with a docking arm chained off the middle.
    cargo(
        &mut commands,
        station,
        &down[0],
        &down[1],
        Vec2::NEG_Y,
        meshes,
        materials,
    );
    let mid_hold = cargo(
        &mut commands,
        station,
        &down[2],
        &down[3],
        Vec2::NEG_Y,
        meshes,
        materials,
    );
    cargo(
        &mut commands,
        station,
        &down[4],
        &down[5],
        Vec2::NEG_Y,
        meshes,
        materials,
    );
    chain_corridor_dock(&mut commands, &mid_hold, Vec2::NEG_Y, 2, meshes, materials);

    // West: two crew lounges and a docking arm between them.
    cargo(
        &mut commands,
        station,
        &left[0],
        &left[1],
        Vec2::NEG_X,
        meshes,
        materials,
    );
    cargo(
        &mut commands,
        station,
        &left[4],
        &left[5],
        Vec2::NEG_X,
        meshes,
        materials,
    );
    corridor_dock(
        &mut commands,
        station,
        &left[2],
        Vec2::NEG_X,
        2,
        meshes,
        materials,
    );

    // East: an equipment bank — engines, turrets and sensors (solid modules) — with
    // access consoles in the hub fronting the engine and sensor blocks.
    for slot in [&right[0], &right[1]] {
        mount(
            &mut commands,
            station,
            &[slot],
            Vec2::X,
            ModuleKind::Engine,
            meshes,
            materials,
        );
    }
    for slot in [&right[2], &right[3]] {
        let mounted = mount(
            &mut commands,
            station,
            &[slot],
            Vec2::X,
            ModuleKind::Turret,
            meshes,
            materials,
        );
        crate::ship::turret::spawn_turret(
            mounted.module,
            crate::ship::turret::TurretKind::Cannon,
            crate::ship::turret::FireArc::OverShip,
            commands.reborrow(),
            meshes,
            materials,
        );
    }
    for slot in [&right[4], &right[5]] {
        mount(
            &mut commands,
            station,
            &[slot],
            Vec2::X,
            ModuleKind::Sensor,
            meshes,
            materials,
        );
    }
    let edge = half.x - 20.;
    spawn_console(
        station,
        Vec2::new(edge, right[0].local.y),
        "Engine",
        commands.reborrow(),
        meshes,
        materials,
    );
    spawn_console(
        station,
        Vec2::new(edge, right[5].local.y),
        "Sensors",
        commands.reborrow(),
        meshes,
        materials,
    );

    // Engineering console in the hub: the hub is the station's engineering module;
    // interacting with this (E) opens build mode for the station.
    crate::build::spawn_build_console(
        station,
        Vec2::new(0., -40.),
        &mut commands,
        meshes,
        materials,
    );

    station
}

/// Mount a 2x2 cargo room across two adjacent slots on `body`, extending `dir`.
fn cargo(
    commands: &mut Commands,
    body: Entity,
    a: &AttachSlot,
    b: &AttachSlot,
    dir: Vec2,
    meshes: &mut ResMut<Assets<Mesh>>,
    materials: &mut ResMut<Assets<ColorMaterial>>,
) -> Mounted {
    mount(
        commands,
        body,
        &[a, b],
        dir,
        ModuleKind::Cargo,
        meshes,
        materials,
    )
}

/// A docking arm off `body`'s `slot`: `segments` hallways in a row, capped by a
/// docking port — a long corridor reaching out for ships to mate with.
fn corridor_dock(
    commands: &mut Commands,
    body: Entity,
    slot: &AttachSlot,
    dir: Vec2,
    segments: u32,
    meshes: &mut ResMut<Assets<Mesh>>,
    materials: &mut ResMut<Assets<ColorMaterial>>,
) {
    let first = mount(
        commands,
        body,
        &[slot],
        dir,
        ModuleKind::Hallway,
        meshes,
        materials,
    );
    chain_corridor(commands, first, dir, segments - 1, meshes, materials);
}

/// As [`corridor_dock`], but chained onto an already-mounted module's far side.
fn chain_corridor_dock(
    commands: &mut Commands,
    parent: &Mounted,
    dir: Vec2,
    segments: u32,
    meshes: &mut ResMut<Assets<Mesh>>,
    materials: &mut ResMut<Assets<ColorMaterial>>,
) {
    let first = mount_far(
        commands,
        parent,
        dir,
        ModuleKind::Hallway,
        meshes,
        materials,
    );
    chain_corridor(commands, first, dir, segments - 1, meshes, materials);
}

/// Extend `from` with `more` further hallways, then cap the end with a dock.
fn chain_corridor(
    commands: &mut Commands,
    from: Mounted,
    dir: Vec2,
    more: u32,
    meshes: &mut ResMut<Assets<Mesh>>,
    materials: &mut ResMut<Assets<ColorMaterial>>,
) {
    let mut tail = from;
    for _ in 0..more {
        tail = mount_far(commands, &tail, dir, ModuleKind::Hallway, meshes, materials);
    }
    mount_far(commands, &tail, dir, ModuleKind::Dock, meshes, materials);
}

/// Mount a module of `kind` onto the far (`direction`) side of an already-mounted
/// module, taking as many leading slots as the kind is wide. Returns the new
/// module so further modules can be chained beyond it.
fn mount_far(
    commands: &mut Commands,
    parent: &Mounted,
    direction: Vec2,
    kind: ModuleKind,
    meshes: &mut ResMut<Assets<Mesh>>,
    materials: &mut ResMut<Assets<ColorMaterial>>,
) -> Mounted {
    let width = kind.footprint().width as usize;
    let slots: Vec<&AttachSlot> = parent.side(direction).iter().take(width).collect();
    mount(
        commands,
        parent.module,
        &slots,
        direction,
        kind,
        meshes,
        materials,
    )
}
