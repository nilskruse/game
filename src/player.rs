use avian2d::{dynamics::solver::solver_body::SolverBody, prelude::*};
use bevy::prelude::*;

use crate::{
    animation::{Animated, Animations},
    character::Character,
    ship::{GameLayer, PlayerShip, ShipBase},
};

#[derive(Component)]
#[require(Character)]
pub struct Player;

#[derive(Component)]
pub struct OnShip {
    ship_entity: Entity,
    relative_transform: Transform,
}

pub fn spawn_player(
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    mut texture_atlas_layouts: ResMut<Assets<TextureAtlasLayout>>,
    player_ship: Single<(Entity, &GlobalTransform), With<PlayerShip>>,
) {
    // let texture = asset_server.load("Factions/Knights/Troops/Warrior/Blue/Warrior_Blue.png");
    let layout = TextureAtlasLayout::from_grid(UVec2::splat(192), 6, 8, None, None);
    let texture_atlas_layout = texture_atlas_layouts.add(layout);
    let animations = Animations::from([
        ("idle-left", (0, 5, true)),
        ("idle-right", (0, 5, false)),
        ("walk-left", (6, 11, true)),
        ("walk-right", (6, 11, false)),
        ("attack-right", (12, 17, false)),
        ("attack-right-2", (18, 23, false)),
        ("attack-left", (12, 17, true)),
        ("attack-left-2", (18, 23, true)),
        ("attack-down", (24, 29, false)),
        ("attack-down-2", (30, 35, false)),
        ("attack-up", (36, 41, false)),
        ("attack-up-2", (42, 47, false)),
    ]);

    let (ship_entity, ship_transform) = *player_ship;
    let player_entity = commands
        .spawn((
            Player,
            Animated {
                animations,
                ..Default::default()
            },
            RigidBody::Kinematic,
            LockedAxes::ROTATION_LOCKED,
            // LockedAxes::ALL_LOCKED,
            Collider::rectangle(25., 25.),
            // Dominance(-1),
            // Friction {
            //     dynamic_coefficient: 0.,
            //     static_coefficient: 0.,
            //     combine_rule: CoefficientCombine::Min,
            // },
            // Restitution {
            //     coefficient: 0.,
            //     combine_rule: CoefficientCombine::Min,
            // },
            // CollisionEventsEnabled,
            // Transform::default(),
            // TransformInterpolation,
            ship_transform.compute_transform(),
            // Sprite::from_atlas_image(
            //     texture,
            //     TextureAtlas {
            //         layout: texture_atlas_layout,
            //         index: 0,
            //     },
            // ),
            OnShip {
                ship_entity: ship_entity,
                relative_transform: Transform::from_xyz(25., 0., 0.),
            },
            CollisionLayers::new(GameLayer::Walls, [GameLayer::Walls]),
            // SweptCcd::default(),
            // CustomPositionIntegration,
            // CustomVelocityIntegration,
        ))
        .id();

    // let joint = commands.spawn((FixedJoint::new(ship_entity, player_entity)));
}

//
pub fn sync_with_ship_via_transform(
    time: Res<Time>,
    mut query: Query<
        (
            &mut Transform,
            &mut LinearVelocity,
            &mut AngularVelocity,
            &OnShip,
        ),
        Without<ShipBase>,
    >,
    ship: Query<(&GlobalTransform, &LinearVelocity, &AngularVelocity), With<ShipBase>>,
) {
    for (mut player_transform, mut player_linear_velocity, mut player_angular_velocity, on_ship) in
        query.iter_mut()
    {
        let (ship_global_transform, ship_linear_velocity, ship_angular_velocity) =
            ship.get(on_ship.ship_entity).expect("on_ship");

        *player_transform = ship_global_transform
            .mul_transform(on_ship.relative_transform)
            .into();

        // let rotated_velocity =
        //     ship_global_transform.rotation() * player_linear_velocity.extend(0.0);
        // player_linear_velocity.0 = rotated_velocity.xy();
    }
}

pub fn sync_with_ship_via_position(
    mut query: Query<(&mut Position, &mut Rotation, &mut LinearVelocity, &OnShip), Without<ShipBase>>,
    ship: Query<(&Position, &Rotation, &LinearVelocity), With<ShipBase>>,
) {
    for (mut player_pos, mut player_rot, mut player_vel, on_ship) in query.iter_mut() {
        let Ok((ship_pos, ship_rot, ship_vel)) = ship.get(on_ship.ship_entity) else { continue };

        // Rotate the offset by the ship's current rotation
        let offset = ship_rot * on_ship.relative_transform.translation.xy();
        player_pos.0 = ship_pos.0 + offset;

        // Inherit ship rotation
        *player_rot = *ship_rot;

        // player_vel.0 = ship_vel.0;
    }
}
// pub fn sync_after(
//     mut query: Query<(&mut Transform, &mut LinearVelocity, &mut OnShip), Without<ShipBase>>,
//     ship: Query<(&GlobalTransform, &LinearVelocity, &AngularVelocity), With<ShipBase>>,
// ) {
//     for (mut player_transform, mut player_linear_velocity, mut on_ship) in query.iter_mut() {
//         let (ship_global_transform, ship_linear_velocity, ship_angular_velocity) =
//             ship.get(on_ship.ship_entity).expect("on_ship");
//         on_ship.relative_transform.translation = ship_global_transform.rotation().inverse()
//             * (player_transform.translation - ship_global_transform.translation());
//     }
// }

pub fn handle_input_transform(
    keyboard_input: Res<ButtonInput<KeyCode>>,
    mut query: Query<&mut OnShip, With<Player>>,
    time: Res<Time>,
) {
    const SPEED: f32 = 210.0;
    for mut oh_ship in query.iter_mut() {
        let mut x = 0.;
        let mut y = 0.;
        if keyboard_input.pressed(KeyCode::ArrowUp) {
            y += 1.0;
        }
        if keyboard_input.pressed(KeyCode::ArrowDown) {
            y -= 1.0;
        }
        if keyboard_input.pressed(KeyCode::ArrowLeft) {
            x -= 1.0;
        }
        if keyboard_input.pressed(KeyCode::ArrowRight) {
            x += 1.0;
        }

        oh_ship.relative_transform.translation +=
            Vec2::new(x, y).extend(0.).normalize_or_zero() * SPEED * time.delta_secs();
    }
}

pub fn handle_input(
    keyboard_input: Res<ButtonInput<KeyCode>>,
    mut query: Query<&mut LinearVelocity, With<Player>>,
) {
    const SPEED: f32 = 210.0;
    for mut linear_velocity in query.iter_mut() {
        let mut x = 0.;
        let mut y = 0.;
        if keyboard_input.pressed(KeyCode::ArrowUp) {
            y += 1.0;
        }
        if keyboard_input.pressed(KeyCode::ArrowDown) {
            y -= 1.0;
        }
        if keyboard_input.pressed(KeyCode::ArrowLeft) {
            x -= 1.0;
        }
        if keyboard_input.pressed(KeyCode::ArrowRight) {
            x += 1.0;
        }

        linear_velocity.0 = Vec2::new(x, y).normalize_or_zero() * SPEED;
    }
}

pub fn handle_input_2(
    keyboard_input: Res<ButtonInput<KeyCode>>,
    mut query: Query<&mut SolverBody, With<Player>>,
) {
    const SPEED: f32 = 210.0;
    for mut solver_body in query.iter_mut() {
        let mut x = 0.;
        let mut y = 0.;
        if keyboard_input.pressed(KeyCode::ArrowUp) {
            y += 1.0;
        }
        if keyboard_input.pressed(KeyCode::ArrowDown) {
            y -= 1.0;
        }
        if keyboard_input.pressed(KeyCode::ArrowLeft) {
            x -= 1.0;
        }
        if keyboard_input.pressed(KeyCode::ArrowRight) {
            x += 1.0;
        }

        solver_body.linear_velocity = Vec2::new(x, y).normalize_or_zero() * SPEED;
    }
}

pub fn sync_with_ship_in_substep(
    mut query: Query<(&mut SolverBody, &OnShip), Without<ShipBase>>,
    ship: Query<(&SolverBody), With<ShipBase>>,
) {
    for (mut player_solver_body, on_ship) in query.iter_mut() {
        let Ok(ship_solver_body) = ship.get(on_ship.ship_entity) else {
            error!("no ship solver body");
            continue;
        };
        player_solver_body.linear_velocity += ship_solver_body.linear_velocity;
        // player_solver_body.delta_position += ship_solver_body.delta_position;
    }
}

pub fn sync_after(
    mut query: Query<(&mut Transform, &mut LinearVelocity, &mut OnShip), Without<ShipBase>>,
    ship: Query<(&GlobalTransform, &LinearVelocity, &AngularVelocity), With<ShipBase>>,
) {
    // for (mut player_transform, mut player_linear_velocity, mut on_ship) in query.iter_mut() {
    //     let (ship_global_transform, ship_linear_velocity, ship_angular_velocity) =
    //         ship.get(on_ship.ship_entity).expect("on_ship");
    //     on_ship.relative_transform.translation = ship_global_transform.rotation().inverse()
    //         * (player_transform.translation - ship_global_transform.translation());
    // }
}
