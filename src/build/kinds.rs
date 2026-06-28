use bevy::prelude::*;

use super::UNIT;

/// The kinds of module that can be built onto a structure.
#[derive(Clone, Copy, PartialEq)]
pub(crate) enum ModuleKind {
    Cargo,
    Engine,
    Sensor,
    Turret,
    Dock,
}

impl ModuleKind {
    /// Footprint in size units (also the number of attach points it covers on
    /// the side it connects to, and the points it exposes per free side).
    pub(crate) fn size_units(self) -> u32 {
        match self {
            ModuleKind::Cargo => 2,
            ModuleKind::Engine => 1,
            ModuleKind::Sensor => 1,
            ModuleKind::Turret => 1,
            ModuleKind::Dock => 1,
        }
    }

    /// Square side length in world units.
    pub(crate) fn extent(self) -> f32 {
        self.size_units() as f32 * UNIT
    }

    pub(crate) fn color(self) -> Color {
        match self {
            ModuleKind::Cargo => Color::srgb(0.55, 0.42, 0.25),
            ModuleKind::Engine => Color::srgb(0.30, 0.50, 0.70),
            ModuleKind::Sensor => Color::srgb(0.35, 0.60, 0.40),
            ModuleKind::Turret => Color::srgb(0.40, 0.42, 0.45),
            ModuleKind::Dock => Color::srgb(1.0, 0.7, 0.1),
        }
    }

    pub(crate) fn name(self) -> &'static str {
        match self {
            ModuleKind::Cargo => "Cargo",
            ModuleKind::Engine => "Engine",
            ModuleKind::Sensor => "Sensor",
            ModuleKind::Turret => "Turret",
            ModuleKind::Dock => "Dock",
        }
    }

    /// Walkable modules are rooms you can enter (they open the hull doorways and
    /// become buildable bodies themselves); non-walkable ones are solid blocks
    /// that leave the hull sealed.
    pub(crate) fn walkable(self) -> bool {
        matches!(self, ModuleKind::Cargo)
    }

    /// Whether placing this opens the covered hull doorways (so the crew can pass
    /// through): walkable rooms, and docking ports (to board a docked structure).
    pub(crate) fn opens_doorway(self) -> bool {
        matches!(self, ModuleKind::Cargo | ModuleKind::Dock)
    }

    /// Whether a weapon turret is mounted on top of the module's block.
    pub(crate) fn mounts_turret(self) -> bool {
        matches!(self, ModuleKind::Turret)
    }

    /// Whether this is a docking port (a sensor collar at the hull edge, no block).
    pub(crate) fn is_dock(self) -> bool {
        matches!(self, ModuleKind::Dock)
    }
}
