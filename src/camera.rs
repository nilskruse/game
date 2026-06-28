use bevy::input::mouse::{MouseScrollUnit, MouseWheel};
use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use crate::{
    build::BuildMode,
    player::{PendingPilot, Player, Seated},
    save::SaveFile,
    ship::PlayerShip,
};

/// Set after a load/new-game so the camera jumps to the correct pose+zoom instead of
/// easing from the pre-load state (which, combined with the deferred re-seat, made the
/// loaded zoom look wrong). Held until the piloting state settles (no `PendingPilot`).
#[derive(Resource, Default)]
pub struct CameraSnap(pub bool);

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
    mut snap: ResMut<CameraSnap>,
    pending_pilot: Res<PendingPilot>,
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
    // Right after a load/new-game, snap instead (t = 1) so the camera lands on the
    // restored pose+zoom immediately rather than easing from the pre-load state.
    const SMOOTH_DECAY: f32 = 8.0;
    let t = if snap.0 {
        1.0
    } else {
        1.0 - (-SMOOTH_DECAY * time.delta_secs()).exp()
    };
    camera_transform.translation = target_translation;
    camera_transform.rotation = camera_transform.rotation.slerp(target_rotation, t);
    if let Projection::Orthographic(ortho) = &mut *projection {
        ortho.scale += (target_scale - ortho.scale) * t;
    }
    // Keep snapping until the piloting state has settled (the re-seat is deferred), so
    // the final snap uses the correct walking/piloting base zoom.
    if snap.0 && pending_pilot.0.is_none() {
        snap.0 = false;
    }
}

/// The camera's persisted state — just the scroll-wheel zoom multiplier.
#[derive(Serialize, Deserialize)]
struct CameraSave {
    zoom: f32,
}

/// Save the camera chunk (runs in `PersistSet::Capture`, i.e. only while saving).
pub(crate) fn capture_camera(zoom: Res<CameraZoom>, mut file: ResMut<SaveFile>) {
    file.write("camera", &CameraSave { zoom: zoom.0 });
}

/// Restore the camera chunk (runs in `PersistSet::Apply`, i.e. only while loading), and
/// snap the camera to the loaded pose/zoom rather than easing from the pre-load state.
pub(crate) fn apply_camera(
    file: Res<SaveFile>,
    mut zoom: ResMut<CameraZoom>,
    mut snap: ResMut<CameraSnap>,
) {
    if let Some(saved) = file.read::<CameraSave>("camera") {
        zoom.0 = saved.zoom;
    }
    snap.0 = true;
}
