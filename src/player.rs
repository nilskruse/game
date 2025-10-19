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
    let texture = asset_server.load("Factions/Knights/Troops/Warrior/Blue/Warrior_Blue.png");
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
    commands.spawn((
        Player,
        Animated {
            animations,
            ..Default::default()
        },
        RigidBody::Dynamic,
        LockedAxes::ROTATION_LOCKED,
        // LockedAxes::ALL_LOCKED,
        Collider::rectangle(25., 25.),
        Dominance(-1),
        Friction {
            dynamic_coefficient: 0.,
            static_coefficient: 0.,
            combine_rule: CoefficientCombine::Min,
        },
        Restitution {
            coefficient: 0.,
            combine_rule: CoefficientCombine::Min,
        },
        // CollisionEventsEnabled,
        Transform::default(),
        Sprite::from_atlas_image(
            texture,
            TextureAtlas {
                layout: texture_atlas_layout,
                index: 0,
            },
        ),
        OnShip {
            ship_entity: ship_entity,
            relative_transform: Transform::from_xyz(0., 0., 0.),
        },
        CollisionLayers::new(GameLayer::Walls, [GameLayer::Walls]),
    ));
}

// pub fn sync_with_ship(
//     time: Res<Time<Fixed>>,
//     mut query: Query<
//         (
//             &mut Transform,
//             &mut LinearVelocity,
//             &mut AngularVelocity,
//             &OnShip,
//         ),
//         Without<ShipBase>,
//     >,
//     ship: Query<(&GlobalTransform, &LinearVelocity, &AngularVelocity), With<ShipBase>>,
// ) {
//     for (mut player_transform, mut player_linear_velocity, mut player_angular_velocity, on_ship) in
//         query.iter_mut()
//     {
//         let (ship_global_transform, ship_linear_velocity, ship_angular_velocity) =
//             ship.get(on_ship.ship_entity).expect("on_ship");

//         *player_transform = ship_global_transform
//             .mul_transform(on_ship.relative_transform)
//             .into();

//         let rotated_velocity =
//             ship_global_transform.rotation() * player_linear_velocity.extend(0.0);
//         player_linear_velocity.0 = rotated_velocity.xy();
//     }
// }

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

pub fn sync_with_ship(
    mut query: Query<(&mut SolverBody, &OnShip), Without<ShipBase>>,
    ship: Query<(&SolverBody), With<ShipBase>>,
) {
    for (mut player_solver_body, on_ship) in query.iter_mut() {
        let Ok(ship_solver_body) = ship.get(on_ship.ship_entity) else {
            error!("what");
            continue;
        };
        player_solver_body.linear_velocity += ship_solver_body.linear_velocity;
        info!("ship: {:?}", player_solver_body.linear_velocity);
        info!("player: {:?}", ship_solver_body.linear_velocity);
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
