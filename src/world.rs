use avian2d::prelude::*;
use bevy::prelude::*;

use crate::station::spawn_space_station;

/// Barebones world foundation.
///
/// The world is described *declaratively*: you spawn a piece of world content
/// with just its data (e.g. a [`WorldBlock`] with a size and colour) and a
/// position, and a lifecycle observer fills in the rendering + physics for you.
/// This keeps "adding stuff to the world" to a single `commands.spawn(...)`.
pub struct WorldPlugin;

impl Plugin for WorldPlugin {
    fn build(&self, app: &mut App) {
        app
            // Reactively flesh out world content the moment it's spawned.
            .add_observer(build_world_block)
            .add_systems(Startup, spawn_world);
    }
}

/// Marker for anything that belongs to the game world (terrain, obstacles,
/// pickups, ...). Lets us query or tear down the whole world at once later.
#[derive(Component, Default)]
pub struct WorldElement;

/// A solid, static rectangular piece of the world: a wall, crate, island edge,
/// and so on. Spawn it with just a [`Transform`] (its position) plus its size
/// and colour; the [`build_world_block`] observer attaches the mesh, material
/// and collider automatically.
///
/// ```ignore
/// commands.spawn((
///     WorldBlock::new(Vec2::new(200., 40.), Color::srgb(0.4, 0.3, 0.2)),
///     Transform::from_xyz(0., -300., 0.),
/// ));
/// ```
#[derive(Component, Clone)]
#[require(WorldElement, Transform)]
pub struct WorldBlock {
    pub size: Vec2,
    pub color: Color,
}

impl WorldBlock {
    pub fn new(size: Vec2, color: Color) -> Self {
        Self { size, color }
    }
}

/// When a [`WorldBlock`] is added, give it its visual + physical body. Runs as a
/// component-lifecycle observer, so it covers blocks spawned at startup and any
/// added later at runtime, from anywhere, with no extra wiring.
fn build_world_block(
    add: On<Add, WorldBlock>,
    blocks: Query<&WorldBlock>,
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<ColorMaterial>>,
) {
    let Ok(block) = blocks.get(add.entity) else {
        return;
    };
    let rect = Rectangle::new(block.size.x, block.size.y);
    commands.entity(add.entity).insert((
        RigidBody::Static,
        Collider::from(rect),
        Mesh2d(meshes.add(rect)),
        MeshMaterial2d(materials.add(block.color)),
    ));
}

/// Starter content. Intentionally tiny — just enough to see the foundation
/// working. We'll grow this as we add features.
fn spawn_world(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<ColorMaterial>>,
) {
    let wall = Color::srgb(0.35, 0.30, 0.25);

    // A single ground strip below the play area to prove the pipeline works.
    commands.spawn((
        WorldBlock::new(Vec2::new(800., 40.), wall),
        Transform::from_xyz(0., -300., -1.),
    ));

    // The first real world element: a space station off to one side, built as a
    // hierarchy (see `spawn_space_station`).
    spawn_space_station(
        Vec2::new(1200., 0.),
        commands.reborrow(),
        &mut meshes,
        &mut materials,
    );
}
