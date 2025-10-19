use bevy::prelude::*;

use crate::ship::PlayerShip;

pub fn spawn_camera(mut commands: Commands) {
    commands.spawn(Camera2d);
}

pub fn move_camera(
    mut camera_transform: Single<&mut Transform, With<Camera2d>>,
    player_transform: Single<&Transform, (With<PlayerShip>, Without<Camera2d>)>,
) {
    camera_transform.translation = player_transform.translation;
}
