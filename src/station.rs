use avian2d::prelude::*;
use bevy::prelude::*;

use crate::{interaction::spawn_console, ship::GameLayer, world::WorldElement};

/// Marker for a space station's root entity.
#[derive(Component)]
pub struct SpaceStation;

/// Marker for a single module of a station (a room or a solid block).
#[derive(Component)]
pub struct StationModule;

const WALL: f32 = 6.;
const DOOR: f32 = 50.;

/// Shared metallic color for all structural walls.
const HULL: Color = Color::srgb(0.46, 0.49, 0.55);
/// Dark panel drawn behind each module; overlapping panels of adjacent modules
/// merge, visually joining them into one hull.
const BACKING: Color = Color::srgb(0.10, 0.11, 0.14);

/// Which wall of a room a doorway sits on.
#[derive(Clone, Copy, PartialEq)]
enum Side {
    Up,
    Down,
    Left,
    Right,
}

/// A doorway on a room: which `side`, and `at` (the station-local coordinate
/// along that wall where the gap is centered — an x for up/down walls, a y for
/// left/right walls). Two adjacent rooms get connected by giving each a doorway
/// on the shared edge at the same `at`.
#[derive(Clone, Copy)]
struct Doorway {
    side: Side,
    at: f32,
}

fn door(side: Side, at: f32) -> Doorway {
    Doorway { side, at }
}

/// Build the player-accessible station: a central spine corridor with assorted
/// modules branching off it. Walkable rooms connect through doorways; solid
/// modules have no interior and are fronted by a console.
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

    // Central spine corridor (tall and narrow), the backbone everything hangs off.
    spawn_room(
        station,
        Vec2::ZERO,
        Vec2::new(70., 320.),
        &[
            door(Side::Up, 0.),       // -> docking bay
            door(Side::Down, 0.),     // -> cargo hold
            door(Side::Left, 90.),    // -> crew lounge
            door(Side::Right, 105.),  // -> quarters (upper)
            door(Side::Right, -105.), // -> quarters (lower)
        ],
        Color::srgb(0.30, 0.34, 0.42),
        &mut commands,
        meshes,
        materials,
    );

    // Docking bay at the top: open down (spine) and up (the dock).
    spawn_room(
        station,
        Vec2::new(0., 215.),
        Vec2::new(130., 110.),
        &[door(Side::Down, 0.), door(Side::Up, 0.)],
        Color::srgb(0.30, 0.55, 0.45),
        &mut commands,
        meshes,
        materials,
    );

    // Large crew lounge off the left of the spine.
    spawn_room(
        station,
        Vec2::new(-125., 90.),
        Vec2::new(180., 140.),
        &[door(Side::Right, 90.)],
        Color::srgb(0.45, 0.40, 0.55),
        &mut commands,
        meshes,
        materials,
    );

    // Two small crew quarters off the right of the spine.
    spawn_room(
        station,
        Vec2::new(92.5, 105.),
        Vec2::new(115., 90.),
        &[door(Side::Left, 105.)],
        Color::srgb(0.38, 0.46, 0.55),
        &mut commands,
        meshes,
        materials,
    );
    spawn_room(
        station,
        Vec2::new(92.5, -105.),
        Vec2::new(115., 90.),
        &[door(Side::Left, -105.)],
        Color::srgb(0.38, 0.46, 0.55),
        &mut commands,
        meshes,
        materials,
    );

    // Wide cargo hold across the bottom.
    spawn_room(
        station,
        Vec2::new(0., -230.),
        Vec2::new(220., 140.),
        &[door(Side::Up, 0.)],
        Color::srgb(0.50, 0.45, 0.30),
        &mut commands,
        meshes,
        materials,
    );

    // --- Solid (non-walkable) modules + their access consoles ---
    // Engineering: slots between the two quarters, fronted from the spine.
    spawn_solid_module(
        station,
        Vec2::new(92.5, 0.),
        Vec2::new(115., 120.),
        Color::srgb(0.50, 0.35, 0.20),
        commands.reborrow(),
        meshes,
        materials,
    );
    spawn_console(station, Vec2::new(30., 0.), "Engineering", commands.reborrow(), meshes, materials);

    // Reactor: a big block beneath the crew lounge, fronted from the lounge.
    spawn_solid_module(
        station,
        Vec2::new(-125., -55.),
        Vec2::new(180., 150.),
        Color::srgb(0.50, 0.30, 0.30),
        commands.reborrow(),
        meshes,
        materials,
    );
    spawn_console(station, Vec2::new(-125., 28.), "Reactor", commands.reborrow(), meshes, materials);

    // Docking airlock on the bay's top edge (the same module ships use), opening
    // down into the bay and outward (+Y) for an arriving ship to mate with.
    crate::build::spawn_dock_module(
        &mut commands,
        station,
        Vec2::new(0., 270.),
        Vec2::Y,
        meshes,
        materials,
    );

    station
}

/// A walkable room module: a colored floor framed by a dark backing panel, with
/// four metallic walls (on the `Walls` layer so the player collides) and doorway
/// gaps punched where `doorways` say.
fn spawn_room(
    parent: Entity,
    center: Vec2,
    size: Vec2,
    doorways: &[Doorway],
    floor: Color,
    commands: &mut Commands,
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

    // Backing panel (behind), then floor (in front of backing, behind walls).
    spawn_quad(module, Vec2::ZERO, size + Vec2::splat(16.), -0.7, BACKING, commands, meshes, materials);
    spawn_quad(module, Vec2::ZERO, size - Vec2::splat(WALL), -0.5, floor, commands, meshes, materials);

    let (hw, hh) = (size.x / 2., size.y / 2.);
    spawn_side(module, true, hh, size.x, center.x, doorways, Side::Up, HULL, commands, meshes, materials);
    spawn_side(module, true, -hh, size.x, center.x, doorways, Side::Down, HULL, commands, meshes, materials);
    spawn_side(module, false, -hw, size.y, center.y, doorways, Side::Left, HULL, commands, meshes, materials);
    spawn_side(module, false, hw, size.y, center.y, doorways, Side::Right, HULL, commands, meshes, materials);

    module
}

/// Build one wall of a room as solid segments, leaving a `DOOR`-wide gap at each
/// doorway on this `side`. `perp` is the wall's fixed (perpendicular) local
/// coordinate; `center_axis` is the room center's coordinate along the wall.
#[allow(clippy::too_many_arguments)]
fn spawn_side(
    module: Entity,
    horizontal: bool,
    perp: f32,
    length: f32,
    center_axis: f32,
    doorways: &[Doorway],
    side: Side,
    color: Color,
    commands: &mut Commands,
    meshes: &mut ResMut<Assets<Mesh>>,
    materials: &mut ResMut<Assets<ColorMaterial>>,
) {
    let gaps: Vec<f32> = doorways
        .iter()
        .filter(|d| d.side == side)
        .map(|d| d.at - center_axis)
        .collect();

    for (seg_center, seg_len) in wall_segments(length, &gaps) {
        let (pos, size) = if horizontal {
            (Vec2::new(seg_center, perp), Vec2::new(seg_len, WALL))
        } else {
            (Vec2::new(perp, seg_center), Vec2::new(WALL, seg_len))
        };
        spawn_wall_seg(module, pos, size, color, commands, meshes, materials);
    }
}

/// Split a wall of `length` (centered at 0) into the solid segments left over
/// after removing a `DOOR`-wide gap at each center in `gaps`. Returns
/// `(segment_center, segment_length)` pairs.
fn wall_segments(length: f32, gaps: &[f32]) -> Vec<(f32, f32)> {
    let half = length / 2.;
    let mut cuts: Vec<(f32, f32)> = gaps
        .iter()
        .map(|g| ((g - DOOR / 2.).max(-half), (g + DOOR / 2.).min(half)))
        .filter(|(lo, hi)| hi > lo)
        .collect();
    cuts.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());

    let mut segments = Vec::new();
    let mut cursor = -half;
    for (lo, hi) in cuts {
        if lo > cursor {
            segments.push(((cursor + lo) / 2., lo - cursor));
        }
        cursor = cursor.max(hi);
    }
    if cursor < half {
        segments.push(((cursor + half) / 2., half - cursor));
    }
    segments
}

/// A solid module: a filled collider with no interior, dressed with a backing
/// panel and a grid of window lights. Fronted by a console on an adjacent room
/// so it can still be "accessed".
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
    let module = commands
        .spawn((
            StationModule,
            ChildOf(parent),
            Transform::from_xyz(center.x, center.y, 0.),
            Collider::from(rect),
            Mesh2d(meshes.add(rect)),
            MeshMaterial2d(materials.add(color)),
            CollisionLayers::new(GameLayer::Walls, [GameLayer::Player, GameLayer::Default]),
        ))
        .id();

    spawn_quad(module, Vec2::ZERO, size + Vec2::splat(16.), -0.7, BACKING, &mut commands, meshes, materials);
    spawn_lights(module, size, &mut commands, meshes, materials);
    module
}

/// Scatter a grid of small lit "windows" across a module for detail.
fn spawn_lights(
    parent: Entity,
    size: Vec2,
    commands: &mut Commands,
    meshes: &mut ResMut<Assets<Mesh>>,
    materials: &mut ResMut<Assets<ColorMaterial>>,
) {
    let mesh = meshes.add(Rectangle::new(5., 7.));
    let mat = materials.add(Color::srgb(0.95, 0.82, 0.45));
    let spacing = 36.;
    let cols = (((size.x - 30.) / spacing).floor() as i32).max(1);
    let rows = (((size.y - 30.) / spacing).floor() as i32).max(1);
    let x0 = -(cols - 1) as f32 * spacing / 2.;
    let y0 = -(rows - 1) as f32 * spacing / 2.;
    for i in 0..cols {
        for j in 0..rows {
            let pos = Vec2::new(x0 + i as f32 * spacing, y0 + j as f32 * spacing);
            commands.spawn((
                ChildOf(parent),
                Transform::from_xyz(pos.x, pos.y, 0.3),
                Mesh2d(mesh.clone()),
                MeshMaterial2d(mat.clone()),
            ));
        }
    }
}

/// A plain (no-collider) colored quad child, for floors, panels, and trim.
fn spawn_quad(
    parent: Entity,
    center: Vec2,
    size: Vec2,
    z: f32,
    color: Color,
    commands: &mut Commands,
    meshes: &mut ResMut<Assets<Mesh>>,
    materials: &mut ResMut<Assets<ColorMaterial>>,
) -> Entity {
    let rect = Rectangle::new(size.x, size.y);
    commands
        .spawn((
            ChildOf(parent),
            Transform::from_xyz(center.x, center.y, z),
            Mesh2d(meshes.add(rect)),
            MeshMaterial2d(materials.add(color)),
        ))
        .id()
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
        CollisionLayers::new(GameLayer::Walls, [GameLayer::Player, GameLayer::Default]),
    ));
}
