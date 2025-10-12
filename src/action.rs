use crate::{health::Health, Character};
use avian2d::prelude::*;
use bevy::prelude::*;

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
    direction: ActionDirection,
    char_transform: &Transform,
    char_collider: &Collider,
    commands: &mut Commands,
) {
    let height = 100.;
    let width = 50.;
    let collider = match direction {
        ActionDirection::Right | ActionDirection::Left => Collider::rectangle(width, height),
        ActionDirection::Up | ActionDirection::Down => Collider::rectangle(height, width),
    };

    let collider_cuboid = collider
        .shape()
        .as_cuboid()
        .expect("this should be an cuboid");
    let char_cuboid = char_collider
        .shape()
        .as_cuboid()
        .expect("this should be an cuboid");
    let (x, y) = match direction {
        ActionDirection::Right => (
            char_transform.translation.x
                + char_cuboid.half_extents.x
                + collider_cuboid.half_extents.x,
            char_transform.translation.y,
        ),
        ActionDirection::Left => (
            char_transform.translation.x
                - char_cuboid.half_extents.x
                - collider_cuboid.half_extents.x,
            char_transform.translation.y,
        ),
        ActionDirection::Up => (
            char_transform.translation.x,
            char_transform.translation.y
                + char_cuboid.half_extents.y
                + collider_cuboid.half_extents.y,
        ),
        ActionDirection::Down => (
            char_transform.translation.x,
            char_transform.translation.y
                - char_cuboid.half_extents.y
                - collider_cuboid.half_extents.y,
        ),
    };

    commands.spawn((
        DamageArea { damage: 40. },
        collider,
        Transform::from_xyz(x, y, 0.),
        RigidBody::Static,
        CollidingEntities::default(),
    ));
}

#[derive(Component)]
#[require(Sensor)]
pub struct DamageArea {
    damage: f32,
}

pub fn process_damage_area(
    mut commands: Commands,
    query: Query<(Entity, &DamageArea, &CollidingEntities)>,
    mut query2: Query<(Entity, &mut Health)>,
) {
    // for collision_event in collision_events.read() {
    //     println!("Received collision event 2: {:?}", collision_event);
    // }
    for (area_entity, area, colliding_entities) in query.iter() {
        println!(
            "{} is colliding with the following entities: {:?}",
            area_entity, colliding_entities,
        );

        for hit_entity in colliding_entities.iter() {
            if let Ok((entity, mut health)) = query2.get_mut(*hit_entity) {
                println!("damaged");

                health.current -= area.damage;

                if health.current < 0. {
                    commands.entity(entity).despawn();
                }
            }
        }
        commands.entity(area_entity).despawn();
    }
}
