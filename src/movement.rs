use core::f32;

use avian2d::prelude::*;
use bevy::prelude::*;

use crate::{player::Seated, ship::PlayerShip};

#[derive(Hash, Eq, PartialEq, Default, Copy, Clone, Debug)]
pub enum Movement {
    #[default]
    Idle,
    Moving,
}

pub fn handle_input_ship(
    keyboard_input: Res<ButtonInput<KeyCode>>,
    mut query: Query<
        (Entity, &mut LinearVelocity, &mut AngularVelocity, &Transform),
        With<PlayerShip>,
    >,
    pilots: Query<&Seated>,
) {
    const SPEED: f32 = 210.0;
    const ROTATION_SPEED: f32 = 5.;
    for (ship_entity, mut linear_velocity, mut angular_velocity, transform) in query.iter_mut() {
        // The ship only responds to steering input while a player is seated at
        // one of its pilot seats. Otherwise it just coasts (damping slows it).
        let piloted = pilots.iter().any(|seated| seated.ship == ship_entity);
        if !piloted {
            angular_velocity.0 = 0.0;
            continue;
        }

        let mut rotation_factor = 0.0;
        let mut movement_factor = 0.0;

        if keyboard_input.pressed(KeyCode::KeyA) {
            rotation_factor += 1.0;
        }

        if keyboard_input.pressed(KeyCode::KeyD) {
            rotation_factor -= 1.0;
        }

        if keyboard_input.pressed(KeyCode::KeyW) {
            movement_factor += 1.0;
        }

        // Update the ship rotation around the Z axis (perpendicular to the 2D plane of the screen)
        angular_velocity.0 = rotation_factor * ROTATION_SPEED;

        // Get the ship's forward vector by applying the current rotation to the ships initial facing
        // vector
        let movement_direction = transform.rotation * Vec3::Y;
        // Get the distance the ship will move based on direction, the ship's movement speed and delta
        // time
        let movement_distance = movement_factor * SPEED;
        // Create the change in translation using the new movement direction and distance
        let translation_delta = movement_direction * movement_distance;

        linear_velocity.0 = translation_delta.xy();
    }
}

// pub fn handle_input(
//     keyboard_input: Res<ButtonInput<KeyCode>>,
//     mut query: Query<(&mut Character, &mut LinearVelocity, &mut AngularVelocity), With<Player>>,
// ) {
//     const SPEED: f32 = 210.0;
//     for (mut character, mut linear_velocity, mut angular_velocity) in query.iter_mut() {
//         let mut x = 0.;
//         let mut y = 0.;
//         if keyboard_input.pressed(KeyCode::KeyW) {
//             y += 1.0;
//             character.current_direction = ActionDirection::Up;
//         }
//         if keyboard_input.pressed(KeyCode::KeyS) {
//             y -= 1.0;
//             character.current_direction = ActionDirection::Down;
//         }
//         if keyboard_input.pressed(KeyCode::KeyA) {
//             x -= 1.0;
//             character.current_direction = ActionDirection::Left;
//         }
//         if keyboard_input.pressed(KeyCode::KeyD) {
//             x += 1.0;
//             character.current_direction = ActionDirection::Right;
//         }

//         if keyboard_input.pressed(KeyCode::Space) {
//             // println!("action");
//             character.requested_action = ActionContainer {
//                 action_type: ActionType::Attack,
//                 ..Default::default()
//             };
//         }

//         linear_velocity.0 = Vec2::new(x, y).normalize_or_zero() * SPEED;
//     }
// }

pub fn apply_movement_damping(time: Res<Time>, mut query: Query<&mut LinearVelocity>) {
    // Precision is adjusted so that the example works with
    // both the `f32` and `f64` features. Otherwise you don't need this.
    let delta_time = time.delta_secs();

    for mut linear_velocity in &mut query {
        // We could use `LinearDamping`, but we don't want to dampen movement along the Y axis
        linear_velocity.x *= 1.0 / (1.0 + 0.9 * delta_time);
        linear_velocity.y *= 1.0 / (1.0 + 0.9 * delta_time);
    }
}

// pub fn advance_physics(
//     fixed_time: Res<Time<Fixed>>,
//     mut query: Query<(
//         &mut AccumulatedInput,
//         &Velocity,
//     )>,
// ) {
//     for (mut input, velocity) in query.iter_mut() {
//         let step = velocity.0 * fixed_time.delta_secs();
//         controller.translation = Some(step);
//         input.0 = Vec2::ZERO;
//     }
// }
