// Bevy systems routinely take many `Query`/`Res` parameters and deeply nested
// `Query<(...), (With<..>, Without<..>)>` filter types; these two lints fire on
// nearly every system and spawn helper, so we allow them crate-wide (the common
// practice for Bevy projects) rather than obscuring signatures to appease them.
#![allow(clippy::type_complexity)]
#![allow(clippy::too_many_arguments)]

pub mod action;
pub mod animation;
pub mod background;
pub mod build;
pub mod camera;
pub mod character;
pub mod docking;
pub mod enemy;
pub mod faction;
pub mod health;
pub mod interaction;
pub mod movement;
pub mod player;
pub mod ship;
pub mod station;
pub mod world;

use action::{finish_actions, process_damage_area, start_actions};
use animation::{animate_sprite, set_animation_direction, set_animation_key, set_animation_type};
use avian2d::prelude::*;
use bevy::{app::HierarchyPropagatePlugin, prelude::*};
use character::Character;
use movement::handle_input_ship;

use crate::{
    camera::{move_camera, spawn_camera},
    docking::toggle_dock,
    enemy::{spawn_enemy, spawn_enemy_ship},
    faction::InFaction,
    interaction::interact,
    movement::apply_movement_damping,
    player::{correct_player_carry, drive_player_on_ship, read_player_input, toggle_seat},
    ship::turret::{fire_turret, rotate_turret, select_target},
    world::WorldPlugin,
};

fn main() {
    App::new()
        .add_plugins(DefaultPlugins)
        .add_plugins(HierarchyPropagatePlugin::<InFaction>::new(PostUpdate))
        .add_plugins(PhysicsPlugins::default())
        .add_plugins(PhysicsDebugPlugin)
        .insert_resource(Gravity(Vec2::ZERO))
        // Default SolverConfig. The previous override (contact_damping_ratio: 0,
        // max_overlap_solve_speed: 1e15) made contacts an undamped spring with
        // infinite pushout, which oscillated the player into moving walls.
        // .insert_resource(SolverConfig {
        //     max_overlap_solve_speed: 1000000000000000.,
        //     contact_damping_ratio: 0.,
        //     ..Default::default()
        // })
        // .insert_resource(NarrowPhaseConfig {
        //     match_contacts: false,
        //     ..Default::default()
        // })
        // .insert_resource(SubstepCount(12))
        .add_plugins(Game)
        .add_plugins(WorldPlugin)
        .add_plugins(background::BackgroundPlugin)
        .add_plugins(build::BuildPlugin)
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
        app.add_systems(Startup, spawn_obstacle);
        app.add_systems(Startup, spawn_enemy);
        app.add_systems(Startup, spawn_enemy_ship);
        app.add_systems(Startup, spawn_camera);
        app.add_systems(
            Startup,
            (ship::spawn_player_ship, player::spawn_player).chain(),
        );
        app.add_systems(FixedUpdate, (animate_sprite, finish_actions).chain());
        app.add_systems(FixedUpdate, process_damage_area);
        app.add_systems(FixedUpdate, select_target);
        app.add_systems(FixedUpdate, rotate_turret);
        // Set the player's velocity each tick *before* the physics step, so the
        // solver carries it with the ship and blocks it on the walls.
        // `just_pressed` must be polled at frame rate; in FixedUpdate (which can
        // tick zero or many times per frame) the press edge gets missed.
        app.add_systems(Update, (toggle_seat, toggle_dock, interact));
        app.add_systems(
            FixedUpdate,
            (read_player_input, drive_player_on_ship).chain(),
        );
        // After the solve but before transform writeback, fix the player up
        // against the ship's *actual* motion this step.
        app.add_systems(
            FixedPostUpdate,
            correct_player_carry
                .after(PhysicsSystems::StepSimulation)
                .before(PhysicsSystems::Writeback),
        );
        // (Player-on-ship carry was also prototyped via transform sync, position
        // sync, and substep-schedule velocity injection; all abandoned in favor of
        // the drive + correct_player_carry pair above.)
        app.add_systems(Update, fire_turret);
        app.add_systems(Update, move_camera);
        app.add_systems(
            RunFixedMainLoop,
            ((
                handle_input_ship,
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
