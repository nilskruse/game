use avian2d::prelude::*;
use bevy::{app::Propagate, prelude::*};

use crate::build::{
    build_buildable_side, mount, AttachSlot, ModuleKind, ModuleRegistry, Mounted, UNIT,
};
use crate::ship::StructureRoot;
use crate::world::WorldElement;

/// Marker for a space station's root entity.
#[derive(Component)]
pub struct SpaceStation;

/// Floor color of the central hub (the station's root room).
const HUB_FLOOR: Color = Color::srgb(0.30, 0.34, 0.42);

/// The station's central hub is this many cells on a side.
const HUB_SIZE: u32 = 10;
/// Length (in hallway segments) of each docking arm reaching out from the hub.
const ARM_SEGMENTS: u32 = 5;

/// Build the player-accessible station entirely out of standard modules — the same
/// kinds you can build onto a ship. A large central hub (a buildable body, like the
/// ship base) has rooms, corridors, docking ports and equipment mounted onto its
/// sides through the shared `mount` path, and modules chained onto those.
///
/// It dwarfs the ship: a wide hub (`HUB_SIZE`) with rows of cargo holds, long docking
/// arms (`ARM_SEGMENTS`) and a bank of equipment — but built at the same `UNIT` scale,
/// so a room is still a room you walk through. The per-side layout scales with the hub
/// size rather than hard-coding slots.
///
/// Built hierarchically (station hub -> module -> walls), mirroring the ship.
pub(crate) fn spawn_space_station(
    position: Vec2,
    mut commands: Commands,
    registry: &ModuleRegistry,
    turrets: &crate::ship::turret::TurretRegistry,
    meshes: &mut ResMut<Assets<Mesh>>,
    materials: &mut ResMut<Assets<ColorMaterial>>,
) -> Entity {
    // Central hub: a large square body, the backbone everything mounts onto. Like the
    // ship base, its full-square collider blocks other hulls (not the player), and its
    // buildable sides enclose a walkable room with doorways.
    let hub_size = HUB_SIZE;
    let extent = hub_size as f32 * UNIT;
    let half = Vec2::splat(extent / 2.);
    let rect = Rectangle::new(extent, extent);
    let station = commands
        .spawn((
            SpaceStation,
            crate::save::Origin::Authored("station".to_string()),
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

    // Three sides berth ships — a long docking arm at each corner with cargo holds
    // filling the span between — and the east side is an equipment bank. This scales
    // with `HUB_SIZE` rather than hard-coding slot indices.
    holds_and_docks(
        &mut commands,
        station,
        &up,
        Vec2::Y,
        registry,
        meshes,
        materials,
    );
    holds_and_docks(
        &mut commands,
        station,
        &down,
        Vec2::NEG_Y,
        registry,
        meshes,
        materials,
    );
    holds_and_docks(
        &mut commands,
        station,
        &left,
        Vec2::NEG_X,
        registry,
        meshes,
        materials,
    );
    equipment_bank(
        &mut commands,
        station,
        &right,
        Vec2::X,
        registry,
        turrets,
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
    registry: &ModuleRegistry,
    meshes: &mut ResMut<Assets<Mesh>>,
    materials: &mut ResMut<Assets<ColorMaterial>>,
) -> Mounted {
    mount(
        commands,
        body,
        &[a, b],
        dir,
        ModuleKind::Cargo,
        registry,
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
    registry: &ModuleRegistry,
    meshes: &mut ResMut<Assets<Mesh>>,
    materials: &mut ResMut<Assets<ColorMaterial>>,
) {
    let first = mount(
        commands,
        body,
        &[slot],
        dir,
        ModuleKind::Hallway,
        registry,
        meshes,
        materials,
    );
    chain_corridor(
        commands,
        first,
        dir,
        segments - 1,
        registry,
        meshes,
        materials,
    );
}

/// Populate one hub side for berthing: a docking arm at each end, with 2-wide cargo
/// holds filling the slots between. Scales to any side width.
fn holds_and_docks(
    commands: &mut Commands,
    station: Entity,
    slots: &[AttachSlot],
    dir: Vec2,
    registry: &ModuleRegistry,
    meshes: &mut ResMut<Assets<Mesh>>,
    materials: &mut ResMut<Assets<ColorMaterial>>,
) {
    let n = slots.len();
    if n == 0 {
        return;
    }
    corridor_dock(
        commands,
        station,
        &slots[0],
        dir,
        ARM_SEGMENTS,
        registry,
        meshes,
        materials,
    );
    if n > 1 {
        corridor_dock(
            commands,
            station,
            &slots[n - 1],
            dir,
            ARM_SEGMENTS,
            registry,
            meshes,
            materials,
        );
    }
    // Cargo holds (each 2 wide) across the interior slots.
    let mut i = 1;
    while i < n.saturating_sub(2) {
        cargo(
            commands,
            station,
            &slots[i],
            &slots[i + 1],
            dir,
            registry,
            meshes,
            materials,
        );
        i += 2;
    }
}

/// Populate one hub side as an equipment bank: a repeating engine / turret / sensor
/// pattern of solid modules across its slots (each turret gets an over-ship cannon).
fn equipment_bank(
    commands: &mut Commands,
    station: Entity,
    slots: &[AttachSlot],
    dir: Vec2,
    registry: &ModuleRegistry,
    turrets: &crate::ship::turret::TurretRegistry,
    meshes: &mut ResMut<Assets<Mesh>>,
    materials: &mut ResMut<Assets<ColorMaterial>>,
) {
    for (i, slot) in slots.iter().enumerate() {
        let kind = match i % 3 {
            0 => ModuleKind::Engine,
            1 => ModuleKind::Turret,
            _ => ModuleKind::Sensor,
        };
        let mounted = mount(
            commands,
            station,
            &[slot],
            dir,
            kind,
            registry,
            meshes,
            materials,
        );
        if kind == ModuleKind::Turret {
            crate::ship::turret::spawn_turret(
                mounted.module,
                crate::ship::turret::TurretKind::Cannon,
                turrets,
                commands.reborrow(),
                meshes,
                materials,
            );
        }
    }
}

/// Extend `from` with `more` further hallways, then cap the end with a dock.
fn chain_corridor(
    commands: &mut Commands,
    from: Mounted,
    dir: Vec2,
    more: u32,
    registry: &ModuleRegistry,
    meshes: &mut ResMut<Assets<Mesh>>,
    materials: &mut ResMut<Assets<ColorMaterial>>,
) {
    let mut tail = from;
    for _ in 0..more {
        tail = mount_far(
            commands,
            &tail,
            dir,
            ModuleKind::Hallway,
            registry,
            meshes,
            materials,
        );
    }
    mount_far(
        commands,
        &tail,
        dir,
        ModuleKind::Dock,
        registry,
        meshes,
        materials,
    );
}

/// Mount a module of `kind` onto the far (`direction`) side of an already-mounted
/// module, taking as many leading slots as the kind is wide. Returns the new
/// module so further modules can be chained beyond it.
fn mount_far(
    commands: &mut Commands,
    parent: &Mounted,
    direction: Vec2,
    kind: ModuleKind,
    registry: &ModuleRegistry,
    meshes: &mut ResMut<Assets<Mesh>>,
    materials: &mut ResMut<Assets<ColorMaterial>>,
) -> Mounted {
    let width = registry.get(kind).footprint.width as usize;
    let slots: Vec<&AttachSlot> = parent.side(direction).iter().take(width).collect();
    mount(
        commands,
        parent.module,
        &slots,
        direction,
        kind,
        registry,
        meshes,
        materials,
    )
}
