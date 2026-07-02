//! In-code content registry: the data definitions for buildable modules, looked up by
//! a stable id ([`ModuleKind`]). This is the single source of truth that replaces the
//! per-kind `match` tables that used to live as methods on `ModuleKind`.
//!
//! The **definition vs instance** split: a placed module is the *instance* (an entity
//! carrying `BuiltModule` + `ModuleHealth`), while a [`ModuleDef`] is its *definition*
//! (stats + behavior category). Definitions live in the [`ModuleRegistry`] resource —
//! an alias of the generic [`Registry`](crate::registry::Registry) container — built
//! once at startup from [`module_defs`]. Later the registry can be populated from
//! asset files instead, without touching any consumer — they all go through `get()`.

use bevy::prelude::*;

use super::kinds::{Footprint, ModuleKind, ThrusterSpec, MAIN_THRUST, MANEUVER_THRUST};

/// Behavioral category of a module — drives which spawn routine builds it and how it
/// connects to the hull.
#[derive(Clone, Copy)]
pub(crate) enum ModuleArchetype {
    /// A walkable room: opens the covered hull doorways, exposes buildable sides, and
    /// (if `seat`) holds a pilot seat.
    Room { seat: bool },
    /// A docking airlock: opens the doorway and carries a docking-port collar (no block).
    Dock,
    /// A sealed solid block — the hull stays closed (e.g. sensor, engine, turret mount).
    Solid,
}

/// How a thruster module pushes: per-direction `strength`, and whether it also pushes
/// along both laterals (a maneuvering cluster) rather than only inward. The concrete
/// push vectors depend on the mounted outward normal — see [`ModuleDef::thruster_spec`].
#[derive(Clone, Copy)]
pub(crate) struct ThrustDef {
    pub strength: f32,
    pub lateral: bool,
}

/// The definition of a buildable module: all its static data in one place.
pub(crate) struct ModuleDef {
    pub kind: ModuleKind,
    pub name: &'static str,
    pub footprint: Footprint,
    pub color: Color,
    /// `(max health, armor)`. Armor is flat damage reduction (see `health::apply_armor`).
    pub durability: (f32, f32),
    pub archetype: ModuleArchetype,
    /// Thrust capability, if this is an engine / maneuvering thruster.
    pub thrust: Option<ThrustDef>,
    /// Whether a weapon turret is installed on top (otherwise it's a bare mount).
    pub mounts_turret: bool,
}

impl ModuleDef {
    /// Walkable rooms are enterable bodies that open the hull doorways; everything else
    /// leaves the hull sealed.
    pub(crate) fn walkable(&self) -> bool {
        matches!(self.archetype, ModuleArchetype::Room { .. })
    }

    /// Whether a pilot seat is placed in the module (the cockpit).
    pub(crate) fn has_seat(&self) -> bool {
        matches!(self.archetype, ModuleArchetype::Room { seat: true })
    }

    /// Whether this is a docking port (a sensor collar at the hull edge, no block).
    pub(crate) fn is_dock(&self) -> bool {
        matches!(self.archetype, ModuleArchetype::Dock)
    }

    /// Whether placing this opens the covered hull doorways (walkable rooms and docks).
    pub(crate) fn opens_doorway(&self) -> bool {
        matches!(
            self.archetype,
            ModuleArchetype::Room { .. } | ModuleArchetype::Dock
        )
    }

    /// Push directions + strength for a thruster mounted with outward normal `outward`,
    /// or `None` if this module isn't a thruster. A thruster pushes the ship *inward*
    /// (`-outward`); a maneuvering cluster also pushes along both laterals.
    pub(crate) fn thruster_spec(&self, outward: Vec2) -> Option<ThrusterSpec> {
        let t = self.thrust?;
        let inward = -outward;
        let lateral = Vec2::new(outward.y, -outward.x);
        let push = if t.lateral {
            vec![inward, lateral, -lateral]
        } else {
            vec![inward]
        };
        Some(ThrusterSpec {
            push,
            strength: t.strength,
        })
    }
}

/// The module content registry: every [`ModuleDef`] keyed by [`ModuleKind`]. Inserted
/// at app build (so it's available to `Startup` spawners); query with `get(kind)`.
pub(crate) type ModuleRegistry = crate::registry::Registry<ModuleKind, ModuleDef>;

impl Default for ModuleRegistry {
    fn default() -> Self {
        Self::new(module_defs().into_iter().map(|d| (d.kind, d)))
    }
}

/// The authored module definitions — the single source of truth for module data.
fn module_defs() -> Vec<ModuleDef> {
    use ModuleArchetype::{Dock, Room, Solid};
    use ModuleKind as K;
    vec![
        ModuleDef {
            kind: K::Cargo,
            name: "Cargo",
            footprint: Footprint { width: 2, depth: 2 },
            color: Color::srgb(0.55, 0.42, 0.25),
            durability: (200., 5.),
            archetype: Room { seat: false },
            thrust: None,
            mounts_turret: false,
        },
        ModuleDef {
            kind: K::Engine,
            name: "Engine",
            footprint: Footprint { width: 1, depth: 1 },
            // The main engine: a hotter, more aggressive metal.
            color: Color::srgb(0.55, 0.30, 0.30),
            durability: (150., 8.),
            archetype: Solid,
            thrust: Some(ThrustDef {
                strength: MAIN_THRUST,
                lateral: false,
            }),
            mounts_turret: false,
        },
        ModuleDef {
            kind: K::Sensor,
            name: "Sensor",
            footprint: Footprint { width: 1, depth: 1 },
            color: Color::srgb(0.35, 0.60, 0.40),
            durability: (80., 3.),
            archetype: Solid,
            thrust: None,
            mounts_turret: false,
        },
        ModuleDef {
            kind: K::Turret,
            name: "Turret",
            footprint: Footprint { width: 1, depth: 1 },
            color: Color::srgb(0.40, 0.42, 0.45),
            durability: (120., 6.),
            archetype: Solid,
            thrust: None,
            mounts_turret: true,
        },
        ModuleDef {
            kind: K::Dock,
            name: "Dock",
            footprint: Footprint { width: 1, depth: 1 },
            color: Color::srgb(1.0, 0.7, 0.1),
            durability: (100., 4.),
            archetype: Dock,
            thrust: None,
            mounts_turret: false,
        },
        ModuleDef {
            kind: K::Hallway,
            name: "Hallway",
            footprint: Footprint { width: 1, depth: 2 },
            color: Color::srgb(0.45, 0.48, 0.52),
            durability: (80., 2.),
            archetype: Room { seat: false },
            thrust: None,
            mounts_turret: false,
        },
        ModuleDef {
            kind: K::Cockpit,
            name: "Cockpit",
            footprint: Footprint { width: 1, depth: 1 },
            color: Color::srgb(0.25, 0.45, 0.65),
            durability: (120., 5.),
            archetype: Room { seat: true },
            thrust: None,
            mounts_turret: false,
        },
        ModuleDef {
            kind: K::Thruster,
            name: "Thruster",
            footprint: Footprint { width: 1, depth: 1 },
            color: Color::srgb(0.45, 0.40, 0.50),
            durability: (100., 5.),
            archetype: Solid,
            thrust: Some(ThrustDef {
                strength: MANEUVER_THRUST,
                lateral: true,
            }),
            mounts_turret: false,
        },
    ]
}
