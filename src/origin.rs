//! Floating origin: one continuous world without f32 precision decay.
//!
//! The live scene (entities, physics, rendering) always stays near `(0, 0)`, where
//! f32 is precise; an entity's *true* position in the world is
//! `WorldOrigin + local`. When the player strays past [`REBASE_DISTANCE`],
//! [`rebase_origin`] shifts every top-level entity back by the player's offset in
//! one frame and accumulates that offset into [`WorldOrigin`] (an f64 `DVec2`, so
//! the world coordinate space is precise at any distance). Both the camera and the
//! world shift by the same amount in the same frame, so a rebase is invisible.
//!
//! Rules this imposes on the rest of the game:
//! - **Never store an absolute world position in a component across frames**
//!   unless `rebase_origin` shifts it (as it does for `Docking::target_pos` and
//!   `CarryState`) — positions derived fresh each frame are always safe.
//! - Anything expressing a position in *world* coordinates (the future off-screen
//!   sim/data model, far-away mission markers, ...) should store f64
//!   (`WorldOrigin + local`) and convert to local f32 only when spawning entities.
//! - Saves store the live (origin-relative) coordinates plus the origin itself,
//!   in this module's `"origin"` chunk; a missing chunk means origin zero.

use avian2d::prelude::Position;
use bevy::math::DVec2;
use bevy::prelude::*;
use bevy::transform::TransformSystems;
use serde::{Deserialize, Serialize};

use crate::docking::Docking;
use crate::player::{CarryState, Player};
use crate::save::{PersistSet, SaveFile};

/// How far (world units) the player may get from the origin before the world is
/// rebased around them. Far enough that it triggers rarely, close enough that f32
/// stays precise (~0.001 units of resolution at this range).
const REBASE_DISTANCE: f32 = 10_000.;

/// The world-space position of the live scene's `(0, 0)`, accumulated over rebases.
/// An entity's true world position is `origin + its local translation`.
#[derive(Resource, Default)]
pub struct WorldOrigin(pub DVec2);

pub struct FloatingOriginPlugin;

impl Plugin for FloatingOriginPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<WorldOrigin>()
            // Before transform propagation, so this frame's `GlobalTransform`s (and
            // the render) already reflect the shift — camera and world move
            // together and nothing visibly jumps.
            .add_systems(
                PostUpdate,
                rebase_origin.before(TransformSystems::Propagate),
            )
            // The origin persists in its own save chunk. Apply is *not* in
            // `PersistSet::Apply`: saved structure positions are world-space, so the
            // origin must be restored before `load_structures` maps them into the
            // origin frame (the Apply set runs after it).
            .add_systems(
                Update,
                (
                    capture_origin.in_set(PersistSet::Capture),
                    apply_origin
                        .run_if(crate::save::loading)
                        .before(crate::save::load_structures),
                ),
            );
    }
}

/// When the player is beyond [`REBASE_DISTANCE`] from the origin, shift the whole
/// live scene so they're back at `(0, 0)`, and add the shift to [`WorldOrigin`].
///
/// What shifts: every top-level entity's `Transform` (children follow through
/// propagation) *and* its avian `Position` (shifted together so the physics never
/// sees a transform/position disagreement; child collider positions re-sync from
/// their body on the next physics step) — plus the cached absolute positions that
/// live in components: a docking slide's target pose and the player-carry snapshot.
/// UI nodes are excluded; parallax backdrop elements are shifted like everything
/// else and re-derive from the camera + origin next frame (see `background`).
pub(crate) fn rebase_origin(
    mut origin: ResMut<WorldOrigin>,
    player: Query<Entity, With<Player>>,
    mut movers: Query<(&mut Transform, Option<&mut Position>), (Without<ChildOf>, Without<Node>)>,
    mut docking: Query<&mut Docking>,
    mut carries: Query<&mut CarryState>,
) {
    let Ok(player) = player.single() else {
        return;
    };
    let Ok((anchor, _)) = movers.get(player) else {
        return;
    };
    let delta = anchor.translation.xy();
    if delta.length_squared() < REBASE_DISTANCE * REBASE_DISTANCE {
        return;
    }

    origin.0 += delta.as_dvec2();
    for (mut transform, position) in &mut movers {
        transform.translation.x -= delta.x;
        transform.translation.y -= delta.y;
        if let Some(mut position) = position {
            position.0 -= delta;
        }
    }
    for mut docking in &mut docking {
        docking.target_pos -= delta;
    }
    for mut carry in &mut carries {
        carry.shift(-delta);
    }
    info!(
        "rebased world origin by {delta} (origin now {}, {})",
        origin.0.x, origin.0.y
    );
}

/// The persisted origin. Everything else in the save is origin-relative (the live
/// coordinates); this chunk is what anchors them in the continuous world.
#[derive(Serialize, Deserialize)]
struct OriginSave {
    origin: [f64; 2],
}

/// Save the origin chunk (runs in `PersistSet::Capture`, i.e. only while saving).
fn capture_origin(origin: Res<WorldOrigin>, mut file: ResMut<SaveFile>) {
    file.write(
        "origin",
        &OriginSave {
            origin: origin.0.to_array(),
        },
    );
}

/// Restore the origin chunk (runs in `PersistSet::Apply`, i.e. only while loading).
/// A save without one (pre-floating-origin) was implicitly at origin zero — reset
/// rather than keep the current session's origin, since the loaded coordinates are
/// relative to the *saved* origin.
fn apply_origin(file: Res<SaveFile>, mut origin: ResMut<WorldOrigin>) {
    origin.0 = file
        .read::<OriginSave>("origin")
        .map(|s| DVec2::from_array(s.origin))
        .unwrap_or(DVec2::ZERO);
}
