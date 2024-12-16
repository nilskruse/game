use bevy::prelude::*;
use bevy_rapier2d::prelude::*;

use crate::{
    animation::{AnimatedCharacter, Animations},
    character::Character,
    movement::{self, AccumulatedInput},
};

#[derive(Component)]
#[require(Character)]
pub struct Player;

pub fn spawn_player(
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    mut texture_atlas_layouts: ResMut<Assets<TextureAtlasLayout>>,
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

    commands.spawn(Camera2d);
    commands
        .spawn((
            Player,
            AnimatedCharacter {
                animations,
                ..Default::default()
            },
            RigidBody::KinematicPositionBased,
            Collider::cuboid(25., 25.),
            Transform::from_xyz(100., 0., 1.),
            AccumulatedInput::default(),
            movement::Velocity::default(),
            Sprite::from_atlas_image(
                texture,
                TextureAtlas {
                    layout: texture_atlas_layout,
                    index: 0,
                },
            ),
            ActiveEvents::COLLISION_EVENTS,
        ))
        .insert(KinematicCharacterController::default());
}
