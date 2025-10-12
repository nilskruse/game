pub mod action;
pub mod animation;
pub mod character;
pub mod enemy;
pub mod health;
pub mod movement;
pub mod player;
pub mod ship;
pub mod world;

use action::{finish_actions, process_damage_area, start_actions};
use animation::{animate_sprite, set_animation_direction, set_animation_key, set_animation_type};
use avian2d::prelude::*;
use bevy::prelude::*;
use character::Character;
use movement::handle_input;
// use player::spawn_player;

use crate::{
    enemy::spawn_enemy,
    movement::apply_movement_damping,
    ship::turret::{fire_turret, rotate_turret, select_target},
};

fn main() {
    App::new()
        .add_plugins(DefaultPlugins)
        .add_plugins(PhysicsPlugins::default())
        .add_plugins(PhysicsDebugPlugin::default())
        .insert_resource(Gravity(Vec2::ZERO))
        .add_plugins(Game)
        .run();
}

fn spawn_obstacle(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<ColorMaterial>>,
) {
    commands.spawn((
        Collider::rectangle(25., 25.),
        Mesh2d(meshes.add(Rectangle::new(25., 25.))),
        MeshMaterial2d(materials.add(Color::srgb(0., 1., 0.))),
        Transform::from_xyz(-50., 0., 0.),
        RigidBody::Static,
        CollisionEventsEnabled,
    ));
}

pub struct Game;

impl Plugin for Game {
    fn build(&self, app: &mut App) {
        // app.add_systems(Startup, setup_world);
        // app.add_systems(Startup, spawn_player);
        app.add_systems(Startup, spawn_obstacle);
        app.add_systems(Startup, spawn_enemy);
        app.add_systems(Startup, ship::spawn_player_ship);
        app.add_systems(FixedUpdate, (animate_sprite, finish_actions).chain());
        app.add_systems(FixedUpdate, process_damage_area);
        app.add_systems(FixedUpdate, select_target);
        app.add_systems(FixedUpdate, rotate_turret);
        app.add_systems(Update, fire_turret);
        app.add_systems(
            RunFixedMainLoop,
            ((
                handle_input,
                start_actions,
                set_animation_direction,
                set_animation_type,
                set_animation_key,
                apply_movement_damping,
            )
                .chain()
                .in_set(RunFixedMainLoopSystems::BeforeFixedMainLoop),),
        );
        app.add_observer(health::on_damage_received);
        app.add_systems(Update, display_events);
    }
}

fn display_events(mut collision_events: MessageReader<CollisionStart>) {
    for collision_event in collision_events.read() {
        trace!("Received collision event: {:?}", collision_event);
    }
}
