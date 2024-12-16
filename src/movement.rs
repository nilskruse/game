use core::f32;

use bevy::prelude::*;
use bevy_rapier2d::prelude::*;

use crate::{
    action::{ActionContainer, ActionDirection, ActionType},
    animation::Direction,
    player::Player,
    Character,
};

/// A vector representing the player's input, accumulated over all frames that ran
/// since the last time the physics simulation was advanced.
#[derive(Debug, Component, Clone, Copy, PartialEq, Default, Deref, DerefMut)]
pub struct AccumulatedInput(Vec2);

/// A vector representing the player's velocity in the physics simulation.
#[derive(Debug, Component, Clone, Copy, PartialEq, Default, Deref, DerefMut)]
#[require(Direction)]
pub struct Velocity(pub Vec2);

#[derive(Hash, Eq, PartialEq, Default, Copy, Clone, Debug)]
pub enum Movement {
    #[default]
    Idle,
    Moving,
}

/// Handle keyboard input and accumulate it in the `AccumulatedInput` component.
///
/// There are many strategies for how to handle all the input that happened since the last fixed timestep.
/// This is a very simple one: we just accumulate the input and average it out by normalizing it.
pub fn handle_input(
    keyboard_input: Res<ButtonInput<KeyCode>>,
    mut query: Query<(&mut Character, &mut AccumulatedInput, &mut Velocity), With<Player>>,
) {
    const SPEED: f32 = 210.0;
    for (mut character, mut input, mut velocity) in query.iter_mut() {
        if keyboard_input.pressed(KeyCode::KeyW) {
            input.y += 1.0;
            character.current_direction = ActionDirection::Up;
        }
        if keyboard_input.pressed(KeyCode::KeyS) {
            input.y -= 1.0;
            character.current_direction = ActionDirection::Down;
        }
        if keyboard_input.pressed(KeyCode::KeyA) {
            input.x -= 1.0;
            character.current_direction = ActionDirection::Left;
        }
        if keyboard_input.pressed(KeyCode::KeyD) {
            input.x += 1.0;
            character.current_direction = ActionDirection::Right;
        }

        if keyboard_input.pressed(KeyCode::Space) {
            println!("action");
            character.requested_action = ActionContainer {
                action_type: ActionType::Attack,
                ..Default::default()
            };
        }

        velocity.0 = input.normalize_or_zero() * SPEED;
    }
}

pub fn advance_physics(
    fixed_time: Res<Time<Fixed>>,
    mut query: Query<(
        &mut KinematicCharacterController,
        &mut AccumulatedInput,
        &Velocity,
    )>,
) {
    for (mut controller, mut input, velocity) in query.iter_mut() {
        let step = velocity.0 * fixed_time.delta_secs();
        controller.translation = Some(step);
        input.0 = Vec2::ZERO;
    }
}
