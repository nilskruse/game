pub mod action;
pub mod animation;
pub mod character;
pub mod movement;
pub mod player;
pub mod world;

use action::{finish_actions, process_damage_area, start_actions};
use animation::{animate_sprite, set_animation_direction, set_animation_key, set_animation_type};
use bevy::prelude::*;
use bevy_ecs_tilemap::prelude::*;
use bevy_rapier2d::prelude::*;
use character::Character;
use movement::{advance_physics, handle_input};
use player::spawn_player;
use world::setup_world;

fn main() {
    App::new()
        .add_plugins(DefaultPlugins)
        .add_plugins(RapierPhysicsPlugin::<NoUserData>::pixels_per_meter(100.0))
        .add_plugins(RapierDebugRenderPlugin::default())
        .add_plugins(TilemapPlugin)
        .add_plugins(Game)
        .run();
}

fn spawn_obstacle(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<ColorMaterial>>,
) {
    commands.spawn((
        Collider::cuboid(12.5, 12.5),
        Mesh2d(meshes.add(Rectangle::new(25., 25.))),
        MeshMaterial2d(materials.add(Color::srgb(0., 1., 0.))),
        Transform::from_xyz(-50., 0., 0.),
        RigidBody::Fixed,
        ActiveEvents::COLLISION_EVENTS,
    ));
}

pub struct Game;

impl Plugin for Game {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, setup_world);
        app.add_systems(Startup, spawn_player);
        app.add_systems(Startup, spawn_obstacle);
        app.add_systems(Update, (animate_sprite, finish_actions).chain());
        app.add_systems(Update, process_damage_area);
        app.add_systems(FixedUpdate, advance_physics);
        app.add_systems(
            RunFixedMainLoop,
            ((
                handle_input,
                start_actions,
                set_animation_direction,
                set_animation_type,
                set_animation_key,
            )
                .chain()
                .in_set(RunFixedMainLoopSystem::BeforeFixedMainLoop),),
        );
        app.add_systems(Update, display_events);
    }
}

fn display_events(
    mut collision_events: EventReader<CollisionEvent>,
) {
    for collision_event in collision_events.read() {
        println!("Received collision event: {:?}", collision_event);
    }
}
