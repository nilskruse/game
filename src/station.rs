use avian2d::prelude::*;
use bevy::prelude::*;

use crate::build::{build_buildable_side, mount, AttachSlot, ModuleKind, Mounted, UNIT};
use crate::{interaction::spawn_console, world::WorldElement};

/// Marker for a space station's root entity.
#[derive(Component)]
pub struct SpaceStation;

/// Floor color of the central hub (the station's root room).
const HUB_FLOOR: Color = Color::srgb(0.30, 0.34, 0.42);

/// Build the player-accessible station entirely out of standard modules — the same
/// kinds you can build onto a ship. A square central hub (a buildable body, like
/// the ship base) has rooms, corridors, docking ports and equipment mounted onto
/// its sides through the shared `mount` path, and rooms chained onto those.
///
/// Built hierarchically (station hub -> module -> walls), mirroring the ship.
pub fn spawn_space_station(
    position: Vec2,
    mut commands: Commands,
    meshes: &mut ResMut<Assets<Mesh>>,
    materials: &mut ResMut<Assets<ColorMaterial>>,
) -> Entity {
    // Central hub: a size-3 square body, the backbone everything mounts onto. Like
    // the ship base, its full-square collider blocks other hulls (not the player),
    // and its four buildable sides enclose a walkable room with doorways.
    let hub_size = 3u32;
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

    // North: a corridor up to a docking port, on the centered slot.
    let corridor = mount(
        &mut commands,
        station,
        &[&up[1]],
        Vec2::Y,
        ModuleKind::Hallway,
        meshes,
        materials,
    );
    mount_far(
        &mut commands,
        &corridor,
        Vec2::Y,
        ModuleKind::Dock,
        meshes,
        materials,
    );

    // South: a 2x2 cargo hold, with a corridor + second docking port chained off
    // its far end (so the station, like a ship, can carry several docks).
    let hold = mount(
        &mut commands,
        station,
        &[&down[0], &down[1]],
        Vec2::NEG_Y,
        ModuleKind::Cargo,
        meshes,
        materials,
    );
    let corridor2 = mount_far(
        &mut commands,
        &hold,
        Vec2::NEG_Y,
        ModuleKind::Hallway,
        meshes,
        materials,
    );
    mount_far(
        &mut commands,
        &corridor2,
        Vec2::NEG_Y,
        ModuleKind::Dock,
        meshes,
        materials,
    );

    // West: a 2x2 crew lounge.
    mount(
        &mut commands,
        station,
        &[&left[0], &left[1]],
        Vec2::NEG_X,
        ModuleKind::Cargo,
        meshes,
        materials,
    );

    // East: external equipment — an engine and a sensor block (solid modules
    // fronted by access consoles in the hub), and a defense turret between them.
    mount(
        &mut commands,
        station,
        &[&right[0]],
        Vec2::X,
        ModuleKind::Engine,
        meshes,
        materials,
    );
    mount(
        &mut commands,
        station,
        &[&right[1]],
        Vec2::X,
        ModuleKind::Turret,
        meshes,
        materials,
    );
    mount(
        &mut commands,
        station,
        &[&right[2]],
        Vec2::X,
        ModuleKind::Sensor,
        meshes,
        materials,
    );
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
        Vec2::new(edge, right[2].local.y),
        "Sensors",
        commands.reborrow(),
        meshes,
        materials,
    );

    station
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
