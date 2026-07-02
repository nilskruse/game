use bevy::prelude::*;

mod attach;
mod blueprint;
mod kinds;
mod mode;
mod registry;
mod spawn;

pub(crate) use attach::AttachPoint;
pub use attach::{build_buildable_side, AttachSlot};
pub(crate) use blueprint::{
    build_structure, dump_blueprints, extract_blueprint, populate_structure, Blueprint,
};
pub(crate) use kinds::ModuleKind;
pub(crate) use mode::{begin_module_drag, drop_module, install_turret, ModuleDeconstructed};
pub use mode::{spawn_build_console, BuildMode, BuildState};
pub(crate) use registry::ModuleDef;
pub(crate) use registry::ModuleRegistry;
pub(crate) use spawn::{mount, BuiltModule, Mounted};
pub(crate) use spawn::{mount_preplaced_cockpit, mount_preplaced_dock, mount_preplaced_turret};

/// One size step in world units. A body of "size N" is `N * UNIT` on each side
/// and exposes N attachment points per side.
pub const UNIT: f32 = 50.;
/// Thickness of hull / module walls.
pub(crate) const WALL: f32 = 5.;
/// Width of the doorway gap left in a wall for each attachment slot.
pub(crate) const DOOR: f32 = 40.;
/// Metallic wall color (matches the station/ship hull look).
pub(crate) const HULL: Color = Color::srgb(0.46, 0.49, 0.55);
/// Removable door-panel color (bronze).
pub(crate) const PANEL: Color = Color::srgb(0.80, 0.45, 0.20);

/// Two body-local directions point the same way (axis-aligned units).
pub(crate) fn same_dir(a: Vec2, b: Vec2) -> bool {
    a.distance(b) < 0.01
}

/// Click-to-build: toggle build mode, pick a module (it follows the cursor as a
/// ghost), and click a highlighted attachment point to attach it to the ship.
pub struct BuildPlugin;

impl Plugin for BuildPlugin {
    fn build(&self, app: &mut App) {
        // The module content registry, available to `Startup` spawners (ship/station/
        // enemy) and the load path.
        app.init_resource::<ModuleRegistry>();
        app.init_state::<BuildState>();
        app.init_resource::<BuildMode>()
            .add_systems(Startup, (mode::spawn_build_ui, mode::spawn_com_marker))
            // Transition work lives on the state edges; the per-frame build systems
            // only run while building.
            .add_systems(OnEnter(BuildState::Building), mode::on_enter_build)
            .add_systems(OnExit(BuildState::Building), mode::on_exit_build)
            .add_systems(
                Update,
                (
                    mode::exit_build_mode,
                    mode::select_module,
                    mode::rotate_module,
                    mode::update_ghost,
                    mode::highlight_attach_points,
                    mode::place_module,
                    mode::deconstruct_module,
                    mode::update_build_text,
                )
                    .run_if(in_state(BuildState::Building)),
            )
            // These self-gate (the marker keys off `BuildMode::structure`; airlocks
            // are a docking concern), so they run in every state.
            .add_systems(
                Update,
                (mode::update_com_marker, spawn::update_airlock_doors),
            );
    }
}
