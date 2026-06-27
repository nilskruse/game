pub mod action;
pub mod animation;
pub mod camera;
pub mod character;
pub mod enemy;
pub mod faction;
pub mod health;
pub mod movement;
pub mod player;
pub mod ship;
pub mod world;

use action::{finish_actions, process_damage_area, start_actions};
use animation::{animate_sprite, set_animation_direction, set_animation_key, set_animation_type};
use avian2d::{
    dynamics::{
        ccd::SweptCcdSystems,
        integrator::IntegrationSystems,
        solver::{schedule::SubstepSolverSystems, xpbd::XpbdSolverSystems, SolverConfig},
    },
    physics_transform::PhysicsTransformSystems,
    prelude::*,
};
use bevy::{app::HierarchyPropagatePlugin, prelude::*, render::RenderSystems};
use character::Character;
use movement::handle_input_ship;
// use player::spawn_player;

use crate::{
    camera::{move_camera, spawn_camera},
    enemy::{spawn_enemy, spawn_enemy_ship},
    faction::InFaction,
    movement::apply_movement_damping,
    player::{sync_with_ship_via_position, sync_with_ship_via_transform},
    ship::turret::{fire_turret, rotate_turret, select_target},
};

fn main() {
    App::new()
        .add_plugins(DefaultPlugins)
        .add_plugins(HierarchyPropagatePlugin::<InFaction>::new(PostUpdate))
        .add_plugins(PhysicsPlugins::default())
        .add_plugins(PhysicsDebugPlugin)
        .insert_resource(Gravity(Vec2::ZERO))
        .insert_resource(SolverConfig {
            max_overlap_solve_speed: 1000000000000000.,
            contact_damping_ratio: 0.,
            ..Default::default()
        })
        // .insert_resource(NarrowPhaseConfig {
        //     match_contacts: false,
        //     ..Default::default()
        // })
        // .insert_resource(SubstepCount(12))
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
        // app.add_systems(FixedUpdate, player::handle_input_transform);
        // app.add_systems(
        //     FixedPostUpdate,
        //     (player::handle_input, player::sync_with_ship)
        //         .chain()
        //         .before(PhysicsTransformSystems::Propagate),
        // );
        // app.add_systems(
        //     FixedPostUpdate,
        //     player::sync_after.after(PhysicsTransformSystems::PositionToTransform),
        // );
        // app.add_systems(
        //     FixedPostUpdate,
        //     player::sync_with_ship.before(SolverSystems::Substep),
        // );
        app.add_systems(
            FixedPostUpdate,
            (sync_with_ship_via_position)
                .before(PhysicsSystems::Writeback)
                .after(PhysicsSystems::StepSimulation),
        );
        app.add_systems(PreUpdate, (player::handle_input_transform).chain());
        // app.add_systems(FixedPostUpdate, ( sync_with_ship_via_transform).chain().in_set(PhysicsTransformSystems::PositionToTransform));
        // app.add_systems(FixedPostUpdate, (player::handle_input_transform, sync_with_ship_via_transform).chain());
        // app.add_systems(
        //     PostUpdate,
        //     sync_with_ship_via_transform
        //         // .after(TransformSystems::Propagate)
        //         // .before(PhysicsTransformSystems::PositionToTransform), // don't fight Avian's writeback
        // );

        // OLD METHOD
        // app.add_systems(
        //     SubstepSchedule,
        //     (player::sync_with_ship_in_substep)
        //         .chain()
        //         .after(SubstepSolverSystems::SolveConstraints)
        //         .before(IntegrationSystems::Position), // .after(XpbdSolverSystems::VelocityProjection),
        // );
        // app.add_systems(
        //     SubstepSchedule,
        //     player::handle_input_2
        //         // .before(SubstepSolverSystems::WarmStart)
        //         // .in_set(SubstepSolverSystems::WarmStart)
        //         // .before(SweptCcdSystems)
        //         // .before(SolverSystems::Restitution)
        //         // .after(SolverSystems::PrepareSolverBodies)
        //         .before(SubstepSolverSystems::WarmStart)
        //         .after(IntegrationSystems::Velocity)
        //         .before(SubstepSolverSystems::SolveConstraints)
        //         .before(IntegrationSystems::Position),
        // );
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
