use avian2d::prelude::*;
use bevy::prelude::*;

use crate::{
    docking::spawn_docking_port, interaction::spawn_console, ship::GameLayer, world::WorldElement,
};

/// Marker for a space station's root entity.
#[derive(Component)]
pub struct SpaceStation;

/// Marker for a single module of a station (a room or a solid block).
#[derive(Component)]
pub struct StationModule;

/// Which sides of a [walkable module](spawn_room) have a doorway. Put a doorway
/// on each side that connects to another walkable module so the player can pass
/// between them.
#[derive(Clone, Copy, Default)]
pub struct Doors {
    pub up: bool,
    pub down: bool,
    pub left: bool,
    pub right: bool,
}

const WALL: f32 = 6.;
const DOOR: f32 = 50.;

/// Build the player-accessible station: a grid of modules. Walkable rooms are
/// connected by doorways; solid modules have no interior and are fronted by a
/// console on the adjacent room's wall (the placeholder for accessing them).
///
/// Built hierarchically (station -> module -> walls), mirroring the ship.
pub fn spawn_space_station(
    position: Vec2,
    mut commands: Commands,
    meshes: &mut ResMut<Assets<Mesh>>,
    materials: &mut ResMut<Assets<ColorMaterial>>,
) -> Entity {
    let station = commands
        .spawn((
            SpaceStation,
            WorldElement,
            RigidBody::Static,
            Transform::from_xyz(position.x, position.y, 0.),
            Visibility::default(),
        ))
        .id();

    let size = Vec2::splat(150.);
    let cell = 150.; // modules sit flush so shared edges (and doorways) line up

    // --- Walkable rooms ---
    // Hub: open to the bay (up) and crew quarters (left). Its right and bottom
    // walls front solid modules (consoles below).
    spawn_room(
        station,
        Vec2::ZERO,
        size,
        Doors {
            up: true,
            left: true,
            ..default()
        },
        Color::srgb(0.35, 0.40, 0.50),
        commands.reborrow(),
        meshes,
        materials,
    );
    // Docking bay: open down (to the hub) and up (the dock doorway).
    spawn_room(
        station,
        Vec2::new(0., cell),
        size,
        Doors {
            down: true,
            up: true,
            ..default()
        },
        Color::srgb(0.30, 0.55, 0.45),
        commands.reborrow(),
        meshes,
        materials,
    );
    // Crew quarters: open right (to the hub).
    spawn_room(
        station,
        Vec2::new(-cell, 0.),
        size,
        Doors {
            right: true,
            ..default()
        },
        Color::srgb(0.45, 0.40, 0.55),
        commands.reborrow(),
        meshes,
        materials,
    );

    // --- Solid (non-walkable) modules + their access consoles ---
    // Engineering, to the right of the hub.
    spawn_solid_module(
        station,
        Vec2::new(cell, 0.),
        size,
        Color::srgb(0.50, 0.35, 0.20),
        commands.reborrow(),
        meshes,
        materials,
    );
    spawn_console(
        station,
        Vec2::new(size.x / 2. - 8., 0.),
        "Engineering",
        commands.reborrow(),
        meshes,
        materials,
    );

    // Reactor, below the hub.
    spawn_solid_module(
        station,
        Vec2::new(0., -cell),
        size,
        Color::srgb(0.50, 0.30, 0.30),
        commands.reborrow(),
        meshes,
        materials,
    );
    spawn_console(
        station,
        Vec2::new(0., -size.y / 2. + 8.),
        "Reactor",
        commands.reborrow(),
        meshes,
        materials,
    );

    // Docking port at the top of the bay's open doorway.
    let port = Vec2::new(0., cell + size.y / 2. + 10.);
    spawn_docking_port(station, port, 0., commands.reborrow(), meshes, materials);

    station
}

/// A walkable room module: four walls (on the `Walls` layer so the player
/// collides) with a doorway gap on each side flagged in `doors`.
fn spawn_room(
    parent: Entity,
    center: Vec2,
    size: Vec2,
    doors: Doors,
    color: Color,
    mut commands: Commands,
    meshes: &mut ResMut<Assets<Mesh>>,
    materials: &mut ResMut<Assets<ColorMaterial>>,
) -> Entity {
    let module = commands
        .spawn((
            StationModule,
            ChildOf(parent),
            Transform::from_xyz(center.x, center.y, 0.),
            Visibility::default(),
        ))
        .id();

    let (hw, hh) = (size.x / 2., size.y / 2.);
    // Walls are placed relative to the module's center.
    spawn_wall_or_door(module, Vec2::new(0., hh), true, size.x, doors.up, color, &mut commands, meshes, materials);
    spawn_wall_or_door(module, Vec2::new(0., -hh), true, size.x, doors.down, color, &mut commands, meshes, materials);
    spawn_wall_or_door(module, Vec2::new(-hw, 0.), false, size.y, doors.left, color, &mut commands, meshes, materials);
    spawn_wall_or_door(module, Vec2::new(hw, 0.), false, size.y, doors.right, color, &mut commands, meshes, materials);

    module
}

/// A solid module: a single filled collider with no interior. Fronted by a
/// console on an adjacent room so it can still be "accessed".
fn spawn_solid_module(
    parent: Entity,
    center: Vec2,
    size: Vec2,
    color: Color,
    mut commands: Commands,
    meshes: &mut ResMut<Assets<Mesh>>,
    materials: &mut ResMut<Assets<ColorMaterial>>,
) -> Entity {
    let rect = Rectangle::new(size.x, size.y);
    commands
        .spawn((
            StationModule,
            ChildOf(parent),
            Transform::from_xyz(center.x, center.y, -0.1),
            Collider::from(rect),
            Mesh2d(meshes.add(rect)),
            MeshMaterial2d(materials.add(color)),
            CollisionLayers::new(GameLayer::Walls, [GameLayer::Walls]),
        ))
        .id()
}

/// Spawn one side of a room: either a single solid wall, or two segments leaving
/// a centered doorway gap. `horizontal` means the wall runs along the x axis.
fn spawn_wall_or_door(
    parent: Entity,
    center: Vec2,
    horizontal: bool,
    length: f32,
    door: bool,
    color: Color,
    commands: &mut Commands,
    meshes: &mut ResMut<Assets<Mesh>>,
    materials: &mut ResMut<Assets<ColorMaterial>>,
) {
    let seg_size = |len: f32| {
        if horizontal {
            Vec2::new(len, WALL)
        } else {
            Vec2::new(WALL, len)
        }
    };

    if !door {
        spawn_wall_seg(parent, center, seg_size(length), color, commands, meshes, materials);
        return;
    }

    let seg_len = (length - DOOR) / 2.;
    let off = DOOR / 2. + seg_len / 2.;
    let axis = if horizontal { Vec2::X } else { Vec2::Y };
    spawn_wall_seg(parent, center - axis * off, seg_size(seg_len), color, commands, meshes, materials);
    spawn_wall_seg(parent, center + axis * off, seg_size(seg_len), color, commands, meshes, materials);
}

fn spawn_wall_seg(
    parent: Entity,
    center: Vec2,
    size: Vec2,
    color: Color,
    commands: &mut Commands,
    meshes: &mut ResMut<Assets<Mesh>>,
    materials: &mut ResMut<Assets<ColorMaterial>>,
) {
    let rect = Rectangle::new(size.x, size.y);
    commands.spawn((
        ChildOf(parent),
        Transform::from_xyz(center.x, center.y, 0.),
        Collider::from(rect),
        Mesh2d(meshes.add(rect)),
        MeshMaterial2d(materials.add(color)),
        CollisionLayers::new(GameLayer::Walls, [GameLayer::Walls]),
    ));
}
