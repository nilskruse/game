use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use super::UNIT;

/// A module's footprint in size units: `width` runs along the edge it attaches to
/// (and equals the number of attach points it covers / exposes per width-face),
/// `depth` is how far it extends outward. A square module has `width == depth`.
#[derive(Clone, Copy)]
pub(crate) struct Footprint {
    pub width: u32,
    pub depth: u32,
}

impl Footprint {
    /// Axis-aligned world size of this footprint when it extends along `direction`
    /// (an axis-aligned unit): depth runs along `direction`, width across it.
    pub(crate) fn world_size(self, direction: Vec2) -> Vec2 {
        let w = self.width as f32 * UNIT;
        let d = self.depth as f32 * UNIT;
        if direction.x != 0.0 {
            Vec2::new(d, w)
        } else {
            Vec2::new(w, d)
        }
    }
}

/// The kinds of module that can be built onto a structure. This is just the stable
/// **id** of a module type; all its data lives in its [`ModuleDef`](super::ModuleDef),
/// looked up through the [`ModuleRegistry`](super::ModuleRegistry).
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Serialize, Deserialize)]
pub(crate) enum ModuleKind {
    Cargo,
    Engine,
    Sensor,
    Turret,
    Dock,
    Hallway,
    Cockpit,
    /// Maneuvering thruster: pushes in three directions (inward + both laterals),
    /// weaker than the main engine. For steering and backing up.
    Thruster,
}

/// Thrust capacity, per push direction, of the main engine (the repurposed
/// [`ModuleKind::Engine`]) and the [`ModuleKind::Thruster`] maneuvering cluster.
/// The engine is stronger but only pushes one way; the cluster is weaker but
/// pushes three ways. Tunable.
pub(crate) const MAIN_THRUST: f32 = 40_000.;
pub(crate) const MANEUVER_THRUST: f32 = 14_000.;

/// A thruster module's ship-local thrust capability (see [`ModuleKind::thruster`]).
pub(crate) struct ThrusterSpec {
    /// Push directions for motion. A *lateral* (±X) push also steers the ship when the
    /// thruster sits off the center of mass (decided geometrically in `movement`), so a
    /// sideways thruster mounted fore/aft turns the ship; forward/back (±Y) pushes never
    /// steer, even mounted off-center.
    pub push: Vec<Vec2>,
    /// Thrust per direction.
    pub strength: f32,
}
