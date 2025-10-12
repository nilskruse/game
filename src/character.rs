use bevy::prelude::*;

use crate::action::{ActionContainer, ActionDirection};

#[derive(Component, Default)]
pub struct Character {
    pub requested_action: ActionContainer,
    pub current_action: ActionContainer,
    pub prev_action: ActionContainer,
    pub current_direction: ActionDirection,
}
