use crate::{
    action::{ActionDirection, ActionState, ActionType},
    character::Character,
    movement::Movement,
};
use avian2d::prelude::LinearVelocity;
use bevy::prelude::*;
use std::{cmp::Ordering, collections::HashMap};

#[derive(Deref, DerefMut, Default)]
pub struct AnimationIndices {
    pub map: HashMap<(Movement, Direction), (usize, usize, bool)>,
}

impl<const N: usize, T: Into<Direction>> From<[((Movement, T), (usize, usize, bool)); N]>
    for AnimationIndices
{
    fn from(arr: [((Movement, T), (usize, usize, bool)); N]) -> Self {
        let mapped = arr.map(|((anim_type, dir), value)| ((anim_type, dir.into()), value));
        Self {
            map: HashMap::from(mapped),
        }
    }
}

#[derive(Deref, DerefMut, Default)]
pub struct Animations {
    pub map: HashMap<&'static str, (usize, usize, bool)>,
}

impl<const N: usize> From<[(&'static str, (usize, usize, bool)); N]> for Animations {
    fn from(arr: [(&'static str, (usize, usize, bool)); N]) -> Self {
        Self {
            map: HashMap::from(arr.map(|(k, v)| (k, v))),
        }
    }
}

#[derive(Deref, DerefMut)]
pub struct AnimationTimer(Timer);

impl Default for AnimationTimer {
    fn default() -> Self {
        AnimationTimer(Timer::from_seconds(0.1, TimerMode::Repeating))
    }
}

#[derive(Hash, Eq, PartialEq, Default, Copy, Clone, Debug)]
pub enum HorizontalDirection {
    #[default]
    Right,
    Left,
}

#[derive(Hash, Eq, PartialEq, Default, Copy, Clone, Debug)]
pub enum VerticalDirection {
    #[default]
    Down,
    Up,
}

impl From<(HorizontalDirection, VerticalDirection)> for Direction {
    fn from((h, v): (HorizontalDirection, VerticalDirection)) -> Self {
        Self { h, v }
    }
}

#[derive(Component, Hash, Eq, PartialEq, Default, Copy, Clone, Debug)]
pub struct Direction {
    pub h: HorizontalDirection,
    pub v: VerticalDirection,
}

#[derive(Component)]
#[require(Sprite, Direction)]
#[derive(Default)]
pub struct Animated {
    pub state: Movement,
    pub prev_state: Movement,
    pub animations: Animations,
    pub timer: AnimationTimer,
    pub animation: &'static str,
    pub prev_animation: &'static str,
}

pub fn set_animation_direction(mut query: Query<(&LinearVelocity, &mut Direction)>) {
    for (velocity, mut direction) in &mut query {
        direction.h = match velocity.0.x.total_cmp(&0.0) {
            Ordering::Less => HorizontalDirection::Left,
            Ordering::Equal => direction.h,
            Ordering::Greater => HorizontalDirection::Right,
        };

        direction.v = match velocity.0.y.total_cmp(&0.0) {
            Ordering::Less => VerticalDirection::Down,
            Ordering::Equal => direction.v,
            Ordering::Greater => VerticalDirection::Up,
        }
    }
}

pub fn set_animation_type(mut query: Query<(&mut Animated, &Character, &LinearVelocity)>) {
    for (mut animated_character, character, velocity) in &mut query {
        if character.current_action.action_type == ActionType::None {
            if velocity.length() > 0. {
                animated_character.state = Movement::Moving;
            } else {
                animated_character.state = Movement::Idle;
            }
        }
    }
}

pub fn set_animation_key(mut query: Query<(&Character, &mut Animated, &Direction)>) {
    for (character, mut animated_character, direction) in &mut query {
        animated_character.animation = match character.current_action.action_type {
            ActionType::None => match (animated_character.state, direction.v, direction.h) {
                (Movement::Idle, _, HorizontalDirection::Left) => "idle-left",
                (Movement::Idle, _, HorizontalDirection::Right) => "idle-right",
                (Movement::Moving, _, HorizontalDirection::Left) => "walk-left",
                (Movement::Moving, _, HorizontalDirection::Right) => "walk-right",
            },
            ActionType::Attack => match character.current_action.direction {
                ActionDirection::Left => "attack-left",
                ActionDirection::Right => "attack-right",
                ActionDirection::Up => "attack-up",
                ActionDirection::Down => "attack-down",
            },
        };
    }
}

pub fn animate_sprite(
    time: Res<Time>,
    mut query: Query<(&mut Character, &mut Animated, &mut Sprite)>,
) {
    for (mut character, mut animated_character, mut sprite) in &mut query {
        animated_character.timer.tick(time.delta());

        if !animated_character.timer.just_finished() {
            continue;
        }

        let Some(atlas) = &mut sprite.texture_atlas else {
            continue;
        };

        let Some((first, last, flip_x)) = animated_character
            .animations
            .get(animated_character.animation)
        else {
            continue;
        };

        atlas.index = if animated_character.prev_animation != animated_character.animation {
            *first
        } else if atlas.index >= *last {
            if !matches!(character.current_action.action_type, ActionType::None) {
                character.current_action.state = ActionState::Finished;
            }
            *first
        } else {
            atlas.index + 1
        };

        sprite.flip_x = *flip_x;
        animated_character.prev_state = animated_character.state;
        animated_character.prev_animation = animated_character.animation;
    }
}
