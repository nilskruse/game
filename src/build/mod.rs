use bevy::prelude::*;

mod attach;
mod kinds;
mod mode;
mod spawn;

pub use attach::{build_buildable_side, AttachSlot};
pub(crate) use kinds::ModuleKind;
pub use mode::BuildMode;
pub(crate) use spawn::{mount, Mounted};
pub use spawn::{mount_preplaced_dock, mount_preplaced_turret, spawn_dock_module};

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
        app.init_resource::<BuildMode>()
            .add_systems(Startup, mode::spawn_build_ui)
            .add_systems(
                Update,
                (
                    mode::toggle_build_mode,
                    mode::select_module,
                    mode::rotate_module,
                    mode::update_ghost,
                    mode::highlight_attach_points,
                    mode::place_module,
                    mode::deconstruct_module,
                    mode::update_build_text,
                    spawn::update_airlock_doors,
                ),
            );
    }
}
