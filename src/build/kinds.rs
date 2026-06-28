use bevy::prelude::*;

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
    pub(crate) fn is_square(self) -> bool {
        self.width == self.depth
    }

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

/// The kinds of module that can be built onto a structure.
#[derive(Clone, Copy, PartialEq)]
pub(crate) enum ModuleKind {
    Cargo,
    Engine,
    Sensor,
    Turret,
    Dock,
    Hallway,
    Cockpit,
}

impl ModuleKind {
    /// Footprint in size units (its un-rotated orientation).
    pub(crate) fn footprint(self) -> Footprint {
        let (width, depth) = match self {
            ModuleKind::Cargo => (2, 2),
            ModuleKind::Engine => (1, 1),
            ModuleKind::Sensor => (1, 1),
            ModuleKind::Turret => (1, 1),
            ModuleKind::Dock => (1, 1),
            ModuleKind::Hallway => (1, 2),
            ModuleKind::Cockpit => (1, 1),
        };
        Footprint { width, depth }
    }

    pub(crate) fn color(self) -> Color {
        match self {
            ModuleKind::Cargo => Color::srgb(0.55, 0.42, 0.25),
            ModuleKind::Engine => Color::srgb(0.30, 0.50, 0.70),
            ModuleKind::Sensor => Color::srgb(0.35, 0.60, 0.40),
            ModuleKind::Turret => Color::srgb(0.40, 0.42, 0.45),
            ModuleKind::Dock => Color::srgb(1.0, 0.7, 0.1),
            ModuleKind::Hallway => Color::srgb(0.45, 0.48, 0.52),
            ModuleKind::Cockpit => Color::srgb(0.25, 0.45, 0.65),
        }
    }

    pub(crate) fn name(self) -> &'static str {
        match self {
            ModuleKind::Cargo => "Cargo",
            ModuleKind::Engine => "Engine",
            ModuleKind::Sensor => "Sensor",
            ModuleKind::Turret => "Turret",
            ModuleKind::Dock => "Dock",
            ModuleKind::Hallway => "Hallway",
            ModuleKind::Cockpit => "Cockpit",
        }
    }

    /// Walkable modules are rooms you can enter (they open the hull doorways and
    /// become buildable bodies themselves); non-walkable ones are solid blocks
    /// that leave the hull sealed.
    pub(crate) fn walkable(self) -> bool {
        matches!(
            self,
            ModuleKind::Cargo | ModuleKind::Hallway | ModuleKind::Cockpit
        )
    }

    /// Whether placing this opens the covered hull doorways (so the crew can pass
    /// through): walkable rooms, and docking ports (to board a docked structure).
    pub(crate) fn opens_doorway(self) -> bool {
        matches!(
            self,
            ModuleKind::Cargo | ModuleKind::Hallway | ModuleKind::Dock | ModuleKind::Cockpit
        )
    }

    /// Whether a weapon turret is mounted on top of the module's block.
    pub(crate) fn mounts_turret(self) -> bool {
        matches!(self, ModuleKind::Turret)
    }

    /// Whether a pilot seat is placed in the module (the cockpit).
    pub(crate) fn has_seat(self) -> bool {
        matches!(self, ModuleKind::Cockpit)
    }

    /// Whether this is a docking port (a sensor collar at the hull edge, no block).
    pub(crate) fn is_dock(self) -> bool {
        matches!(self, ModuleKind::Dock)
    }
}
