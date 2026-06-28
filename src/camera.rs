use bevy::input::mouse::{MouseScrollUnit, MouseWheel};
use bevy::prelude::*;

use crate::{
    build::BuildMode,
    player::{Player, Seated},
    ship::PlayerShip,
};

/// Orthographic zoom (world units per screen unit; >1 = more of the world on screen,
/// so things look smaller). Closer on foot; pulled back when flying or building so the
/// ship/structure fits. The scroll wheel scales these by [`CameraZoom`].
const WALK_ZOOM: f32 = 1.0;
const PILOT_ZOOM: f32 = 2.0;

/// Player scroll-wheel zoom, a multiplier on the mode's base zoom (1.0 = the default
/// for the current mode; smaller = zoomed in). `move_camera` eases toward it.
#[derive(Resource)]
pub struct CameraZoom(pub f32);

impl Default for CameraZoom {
    fn default() -> Self {
        Self(1.0)
    }
}

/// How tight / wide the scroll wheel can push the zoom multiplier.
const ZOOM_MIN: f32 = 0.5;
const ZOOM_MAX: f32 = 2.5;
/// Multiplier applied per scroll line — <1 so scrolling up (positive) zooms in.
const ZOOM_PER_LINE: f32 = 0.88;

/// Adjust the [`CameraZoom`] multiplier from the scroll wheel. The actual camera scale
/// eases toward it in `move_camera`, so the zoom is smooth.
pub fn scroll_zoom(mut wheel: MessageReader<MouseWheel>, mut zoom: ResMut<CameraZoom>) {
    let mut lines = 0.0;
    for event in wheel.read() {
        lines += match event.unit {
            MouseScrollUnit::Line => event.y,
            MouseScrollUnit::Pixel => event.y / 16.0,
        };
    }
    if lines != 0.0 {
        zoom.0 = (zoom.0 * ZOOM_PER_LINE.powf(lines)).clamp(ZOOM_MIN, ZOOM_MAX);
    }
}

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
    zoom: Res<CameraZoom>,
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

    let (target_translation, target_rotation, base_scale) = if let Some(t) = build_target {
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
    // The scroll wheel scales the mode's base zoom.
    let target_scale = base_scale * zoom.0;

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
