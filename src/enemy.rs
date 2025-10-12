use crate::{
    animation::{Animated, Animations},
    character::Character,
    health::{self, Health},
};
use avian2d::prelude::*;
use bevy::prelude::*;

#[derive(Component)]
#[require(Character)]
pub struct Enemy {}

pub fn spawn_enemy(
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    mut texture_atlas_layouts: ResMut<Assets<TextureAtlasLayout>>,
) {
    let texture = asset_server.load("Factions/Goblins/Troops/Torch/Purple/Torch_Purple.png");

    let layout = TextureAtlasLayout::from_grid(UVec2::splat(192), 7, 6, None, None);

    let texture_atlas_layout = texture_atlas_layouts.add(layout);

    let animations = Animations::from([
        ("idle-left", (0, 6, true)),
        ("idle-right", (0, 6, false)),
        ("walk-left", (7, 12, true)),
        ("walk-right", (7, 12, false)),
        ("attack-right", (14, 19, false)),
        ("attack-left", (20, 25, true)),
        ("attack-down", (26, 31, false)),
        ("attack-up", (32, 37, false)),
    ]);

    commands
        .spawn((
            Enemy {},
            Health { current: 1000. },
            Animated {
                animations,

                ..Default::default()
            },
            RigidBody::Dynamic,
            Collider::rectangle(25., 25.),
            Transform::from_xyz(-100., 0., 1.),
            Sprite::from_atlas_image(
                texture,
                TextureAtlas {
                    layout: texture_atlas_layout,

                    index: 0,
                },
            ),
            LockedAxes::ROTATION_LOCKED,
        ))
        .observe(health::on_health_expired);
}

pub fn spawn_enemy_ship(
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    mut texture_atlas_layouts: ResMut<Assets<TextureAtlasLayout>>,
) {
    let texture = asset_server.load("Factions/Goblins/Troops/Torch/Purple/Torch_Purple.png");

    let layout = TextureAtlasLayout::from_grid(UVec2::splat(192), 7, 6, None, None);

    let texture_atlas_layout = texture_atlas_layouts.add(layout);

    let animations = Animations::from([
        ("idle-left", (0, 6, true)),
        ("idle-right", (0, 6, false)),
        ("walk-left", (7, 12, true)),
        ("walk-right", (7, 12, false)),
        ("attack-right", (14, 19, false)),
        ("attack-left", (20, 25, true)),
        ("attack-down", (26, 31, false)),
        ("attack-up", (32, 37, false)),
    ]);

    commands
        .spawn((
            Enemy {},
            Health { current: 1000. },
            Animated {
                animations,

                ..Default::default()
            },
            RigidBody::Dynamic,
            Collider::rectangle(25., 25.),
            Transform::from_xyz(-100., 0., 1.),
            Sprite::from_atlas_image(
                texture,
                TextureAtlas {
                    layout: texture_atlas_layout,

                    index: 0,
                },
            ),
            LockedAxes::ROTATION_LOCKED,
        ))
        .observe(health::on_health_expired);
}

