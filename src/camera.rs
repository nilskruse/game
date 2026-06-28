use bevy::prelude::*;

use crate::{
    build::BuildMode,
    player::{Player, Seated},
    ship::PlayerShip,
};

pub fn spawn_camera(mut commands: Commands) {
    commands.spawn(Camera2d);
}

pub fn move_camera(
    time: Res<Time>,
    build: Res<BuildMode>,
    mut camera_transform: Single<&mut Transform, With<Camera2d>>,
    player: Single<(&Transform, Option<&Seated>), (With<Player>, Without<Camera2d>)>,
    ship_transform: Single<&Transform, (With<PlayerShip>, Without<Camera2d>, Without<Player>)>,
) {
    let (player_transform, seated) = *player;

    let (target_translation, target_rotation) = if build.active {
        // Build mode: frame the whole ship, rotated so the hull sits upright.
        (ship_transform.translation, ship_transform.rotation)
    } else if seated.is_some() {
        // Piloting: follow the ship, upright.
        (ship_transform.translation, Quat::IDENTITY)
    } else {
        // Walking the deck: follow the player and rotate with their facing so
        // the ship's forward stays "up".
        (player_transform.translation, player_transform.rotation)
    };

    // Translation snaps so following stays tight; rotation eases toward the
    // target so switching modes (and the upright<->facing flip) is smooth.
    // Frame-rate-independent exponential smoothing.
    const ROTATION_DECAY: f32 = 8.0;
    let t = 1.0 - (-ROTATION_DECAY * time.delta_secs()).exp();
    camera_transform.translation = target_translation;
    camera_transform.rotation = camera_transform.rotation.slerp(target_rotation, t);
}
