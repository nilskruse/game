use crate::Character;
use bevy::prelude::*;
use bevy_rapier2d::prelude::*;

#[derive(Hash, Eq, PartialEq, Default, Copy, Clone, Debug)]
pub enum ActionType {
    #[default]
    None,
    Attack,
}

#[derive(Hash, Eq, PartialEq, Default, Copy, Clone, Debug)]
pub enum ActionState {
    #[default]
    Init,
    Running,
    Finished,
}

#[derive(Default, Hash, Eq, PartialEq, Copy, Clone, Debug)]
pub enum ActionDirection {
    #[default]
    Right,
    Left,
    Up,
    Down,
}

#[derive(Hash, Eq, PartialEq, Default, Copy, Clone, Debug)]
pub struct ActionContainer {
    pub action_type: ActionType,
    pub state: ActionState,
    pub direction: ActionDirection,
}

pub fn start_actions(mut query: Query<&mut Character>) {
    for mut character in &mut query {
        if character.requested_action.action_type != ActionType::None
            && character.current_action.action_type == ActionType::None
        {
            character.current_action = character.requested_action;
            character.current_action.direction = character.current_direction;
        }
        character.requested_action = ActionContainer::default();
    }
}

pub fn finish_actions(
    mut commands: Commands,
    mut query: Query<(&mut Character, &Transform, &Collider)>,
) {
    for (mut character, char_transform, char_collider) in &mut query {
        if character.current_action.action_type != ActionType::None
            && character.current_action.state == ActionState::Finished
        {
            println!("action finished");
            match character.current_action.action_type {
                ActionType::None => (),
                ActionType::Attack => handle_attack(
                    &character,
                    character.current_action.direction,
                    char_transform,
                    char_collider,
                    &mut commands,
                ),
            }

            character.current_action = ActionContainer::default();
        }
    }
}

pub fn handle_attack(
    character: &Character,
    direction: ActionDirection,
    char_transform: &Transform,
    char_collider: &Collider,
    commands: &mut Commands,
) {
    let height = 100.;
    let width = 50.;
    let collider = match direction {
        ActionDirection::Right | ActionDirection::Left => Collider::cuboid(width, height),
        ActionDirection::Up | ActionDirection::Down => Collider::cuboid(height, width),
    };

    let collider_cuboid = collider.as_cuboid().expect("this should be an cuboid");
    let char_cuboid = char_collider.as_cuboid().expect("this should be an cuboid");
    let (x, y) = match direction {
        ActionDirection::Right => (
            char_transform.translation.x
                + char_cuboid.half_extents().x
                + collider_cuboid.half_extents().x,
            char_transform.translation.y,
        ),
        ActionDirection::Left => (
            char_transform.translation.x
                - char_cuboid.half_extents().x
                - collider_cuboid.half_extents().x,
            char_transform.translation.y,
        ),
        ActionDirection::Up => (
            char_transform.translation.x,
            char_transform.translation.y
                + char_cuboid.half_extents().y
                + collider_cuboid.half_extents().y,
        ),
        ActionDirection::Down => (
            char_transform.translation.x,
            char_transform.translation.y
                - char_cuboid.half_extents().y
                - collider_cuboid.half_extents().y,
        ),
    };

    commands
        .spawn((
            DamageArea {},
            collider,
            Transform::from_xyz(x, y, 0.),
            RigidBody::Dynamic,
            GravityScale(0.0)
        ))
        .insert(ActiveEvents::COLLISION_EVENTS);
}

#[derive(Component)]
// #[require(Sensor)]
pub struct DamageArea {}

pub fn process_damage_area(query: Query<&CollidingEntities>) {
    // pub fn process_damage_area(query: Query<(&Collider, &Transform)>) {
    // for (collider, colliding_entities) in query.iter() {
    for colliding_entities in query.iter() {
        for e in colliding_entities.iter() {
            println!("e: {e:?}");
        }
    }
}
