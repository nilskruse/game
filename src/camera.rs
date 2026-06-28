use bevy::prelude::*;

use crate::{
    build::BuildMode,
    player::{Player, Seated},
    ship::PlayerShip,
};

/// Orthographic zoom (world units per screen unit; >1 = more of the world on screen,
/// so things look smaller). Closer on foot; pulled back when flying or building so the
/// ship/structure fits.
const WALK_ZOOM: f32 = 1.0;
const PILOT_ZOOM: f32 = 2.0;

pub fn spawn_camera(mut commands: Commands) {
    // Start on foot, so start zoomed in.
    commands.spawn((
        Camera2d,
        Projection::from(OrthographicProjection {
            scale: WALK_ZOOM,
            ..OrthographicProjection::default_2d()
        }),
    ));
}

pub fn move_camera(
    time: Res<Time>,
    build: Res<BuildMode>,
    camera: Single<(&mut Transform, &mut Projection), With<Camera2d>>,
    player: Single<(&Transform, Option<&Seated>), (With<Player>, Without<Camera2d>)>,
    ship_transform: Single<&Transform, (With<PlayerShip>, Without<Camera2d>, Without<Player>)>,
    structures: Query<&GlobalTransform, Without<Camera2d>>,
) {
    let (mut camera_transform, mut projection) = camera.into_inner();
    let (player_transform, seated) = *player;

    let build_target = build
        .structure()
        .and_then(|s| structures.get(s).ok())
        .map(|gt| gt.compute_transform());

    let (target_translation, target_rotation, target_scale) = if let Some(t) = build_target {
        // Build mode: frame the structure being edited, rotated so it sits upright.
        (t.translation, t.rotation, PILOT_ZOOM)
    } else if seated.is_some() {
        // Piloting: follow the ship, upright, pulled back.
        (ship_transform.translation, Quat::IDENTITY, PILOT_ZOOM)
    } else {
        // Walking the deck: follow the player (rotated with their facing so the
        // ship's forward stays "up") and zoom in close.
        (
            player_transform.translation,
            player_transform.rotation,
            WALK_ZOOM,
        )
    };

    // Translation snaps so following stays tight; rotation and zoom ease toward the
    // target so switching modes is smooth. Frame-rate-independent exponential smoothing.
    const SMOOTH_DECAY: f32 = 8.0;
    let t = 1.0 - (-SMOOTH_DECAY * time.delta_secs()).exp();
    camera_transform.translation = target_translation;
    camera_transform.rotation = camera_transform.rotation.slerp(target_rotation, t);
    if let Projection::Orthographic(ortho) = &mut *projection {
        ortho.scale += (target_scale - ortho.scale) * t;
    }
}
