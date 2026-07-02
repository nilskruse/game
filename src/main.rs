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
pub mod debug;
pub mod docking;
pub mod effects;
pub mod enemy;
pub mod faction;
pub mod health;
pub mod interaction;
pub mod inventory;
pub mod movement;
pub mod origin;
pub mod player;
pub mod registry;
pub mod save;
pub mod ship;
pub mod station;
pub mod ui;
pub mod world;

use action::{finish_actions, process_damage_area, start_actions};
use animation::{animate_sprite, set_animation_direction, set_animation_key, set_animation_type};
use avian2d::prelude::*;
use bevy::{app::HierarchyPropagatePlugin, prelude::*};
use character::Character;
use movement::{control_player_ship, drive_ships};

use crate::{
    camera::{apply_camera, capture_camera, move_camera, scroll_zoom, spawn_camera, CameraZoom},
    docking::{advance_docking, toggle_dock, update_dock_indicators},
    enemy::{fly_enemy_ships, spawn_enemy, spawn_enemy_ship},
    faction::InFaction,
    interaction::interact,
    movement::apply_movement_damping,
    player::{
        apply_pending_pilot, apply_player, capture_player, correct_player_carry,
        drive_player_on_ship, keep_player_on_ship, read_player_input, toggle_seat, PendingPilot,
    },
    ship::turret::{
        fire_turret, player_weapons, point_defense, rotate_turret, select_target, update_pd_slugs,
    },
    world::WorldPlugin,
};

fn main() {
    App::new()
        .add_plugins(DefaultPlugins)
        .add_plugins(HierarchyPropagatePlugin::<InFaction>::new(PostUpdate))
        .add_plugins(HierarchyPropagatePlugin::<ship::StructureRoot>::new(
            PostUpdate,
        ))
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
        .add_plugins(origin::FloatingOriginPlugin)
        .add_plugins(debug::DebugOverlayPlugin)
        .add_plugins(background::BackgroundPlugin)
        .add_plugins(build::BuildPlugin)
        .add_plugins(ui::UiPlugin)
        .add_plugins(inventory::InventoryPlugin)
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
        // The turret content registry (weapon stats by `TurretKind`), available to the
        // `Startup` ship/station spawners and the load path — the counterpart of
        // `ModuleRegistry` (initialized in `BuildPlugin`).
        app.init_resource::<ship::turret::TurretRegistry>();
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
        app.add_systems(
            Update,
            (toggle_seat, toggle_dock, interact, update_dock_indicators),
        );
        app.add_systems(FixedUpdate, advance_docking);
        app.add_systems(
            FixedUpdate,
            (keep_player_on_ship, read_player_input, drive_player_on_ship).chain(),
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
        app.add_systems(
            Update,
            (fire_turret, player_weapons, point_defense, update_pd_slugs),
        );
        app.add_systems(
            Update,
            (effects::expire_lifetimes, effects::animate_hit_sparks),
        );
        app.init_resource::<CameraZoom>();
        app.init_resource::<camera::CameraSnap>();
        app.add_systems(Update, (scroll_zoom, move_camera).chain());
        // Persistence: a save/load framework where each feature owns its own chunk.
        // Capture systems write their chunk (in `PersistSet::Capture`, only while
        // saving); apply systems restore it (in `PersistSet::Apply`, only while
        // loading). Adding persistence for a new system = register its capture/apply in
        // these sets — no central save struct to edit. See `save.rs`.
        app.init_resource::<save::NextInstanceId>();
        app.init_resource::<save::SaveFile>();
        app.init_resource::<save::PersistOp>();
        app.init_resource::<PendingPilot>();
        app.add_systems(Startup, save::spawn_new_game_button);
        app.add_systems(PostStartup, save::request_load_on_start);
        app.configure_sets(
            Update,
            (
                save::PersistSet::Capture.run_if(save::saving),
                save::PersistSet::Apply.run_if(save::loading),
            ),
        );
        app.add_systems(Update, (save::assign_instance_ids, apply_pending_pilot));
        // Save pipeline: request (F5) -> features capture their chunks -> write file.
        app.add_systems(
            Update,
            (
                save::request_save.before(save::PersistSet::Capture),
                (save::capture_structures, capture_camera, capture_player)
                    .in_set(save::PersistSet::Capture),
                save::commit_save
                    .after(save::PersistSet::Capture)
                    .run_if(save::saving),
            ),
        );
        // Load pipeline: request (F9 / startup) -> rebuild structures -> features apply
        // their chunks -> finish.
        app.add_systems(
            Update,
            (
                save::request_load.before(save::load_structures),
                save::load_structures
                    .run_if(save::loading)
                    .before(save::PersistSet::Apply),
                (apply_camera, apply_player).in_set(save::PersistSet::Apply),
                save::commit_load
                    .after(save::PersistSet::Apply)
                    .run_if(save::loading),
            ),
        );
        app.add_systems(Update, build::dump_blueprints);
        app.add_systems(Update, movement::animate_thrusters);
        app.add_systems(
            RunFixedMainLoop,
            ((
                fly_enemy_ships,
                control_player_ship,
                drive_ships,
                start_actions,
                set_animation_direction,
                set_animation_type,
                set_animation_key,
                apply_movement_damping,
            )
                .chain()
                .in_set(RunFixedMainLoopSystems::BeforeFixedMainLoop),),
        );
        app.init_resource::<health::Invincible>();
        app.add_systems(Update, health::toggle_invincible);
        app.add_systems(
            Update,
            (
                health::sync_ship_health,
                health::destroy_dead_ships,
                health::spawn_health_bars,
                health::update_health_bars,
            )
                .chain(),
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
