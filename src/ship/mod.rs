pub mod bullet;
pub mod turret;

use avian2d::prelude::*;
use bevy::{app::Propagate, prelude::*};

use crate::faction::{Faction, InFaction};

#[derive(Component)]
pub struct PlayerShip;

#[derive(Component)]
pub struct ShipBase;

/// A pilot seat the player can sit at to steer the ship. Lives inside a cockpit
/// module. The seated player's pose is anchored from the ship's physics `Position`
/// plus the seat's offset in the ship-root frame (captured in `Seated`), so there's
/// no transform-propagation lag.
#[derive(Component)]
pub struct PilotSeat;

pub fn spawn_player_ship(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<ColorMaterial>>,
) {
    let ship_rectangle = Rectangle::new(150., 150.);
    // The ship's starting cockpit, turret and docking port are pre-mounted buildable
    // modules (see `spawn_player_ship_base`), the same kinds you can build via the
    // build menu.
    spawn_player_ship_base(
        ship_rectangle,
        commands.reborrow(),
        &mut meshes,
        &mut materials,
    );
}

/// Physics collision layers for the game.
///
/// IMPORTANT: any collider spawned *without* an explicit [`CollisionLayers`]
/// component lands on [`GameLayer::Default`] (the first/`#[default]` variant) with
/// a filter of *all* layers — so it collides with everything. The structural
/// solidity model relies on this on purpose (ship hulls and module structural
/// colliders are deliberately left on `Default` so they block other structures).
/// The flip side: a new decorative/child collider you forget to tag will silently
/// become a solid obstacle for ships and the player. When in doubt, set
/// `CollisionLayers` explicitly.
#[derive(PhysicsLayer, Default)]
pub enum GameLayer {
    /// Structural bodies: ship hulls and module/room structural colliders. Block
    /// other structures (and the player, via the `Walls` filter). Also the
    /// implicit membership of any untagged collider — see the note above.
    #[default]
    Default,
    /// Interior walls (hull, modules, station rooms). They block the player but
    /// not each other, so structures don't physically fight when docking.
    Walls,
    /// The walking player.
    Player,
}

pub fn spawn_player_ship_base(
    rectangle: Rectangle,
    mut commands: Commands,
    meshes: &mut ResMut<Assets<Mesh>>,
    materials: &mut ResMut<Assets<ColorMaterial>>,
) -> Entity {
    let ship_base = commands
        .spawn((
            PlayerShip,
            Propagate(InFaction(Faction::Player)),
            ShipBase,
            RigidBody::Dynamic,
            Transform::from_xyz(100., 0., 0.),
            Collider::from(rectangle),
            Mesh2d(meshes.add(rectangle)),
            MeshMaterial2d(materials.add(Color::srgb(1., 1., 0.))),
        ))
        .id();

    let half = rectangle.half_size;

    // The hull (the engineering module) is a size-3 body: every side is buildable,
    // with three doorway slots (sealed by removable panels) and three attach points.
    // The bottom starts free; the other three sides come with pre-mounted modules,
    // each centered on the middle slot, built through the same path the build menu
    // uses.
    const MID: usize = 1;
    crate::build::build_buildable_side(
        &mut commands,
        ship_base,
        half,
        3,
        Vec2::NEG_Y,
        meshes,
        materials,
    );

    // Top side: a starting cockpit module (holds the pilot seat).
    let top = crate::build::build_buildable_side(
        &mut commands,
        ship_base,
        half,
        3,
        Vec2::Y,
        meshes,
        materials,
    );
    crate::build::mount_preplaced_cockpit(
        &mut commands,
        ship_base,
        &top[MID],
        Vec2::Y,
        meshes,
        materials,
    );

    // Right side: a starting turret.
    let right = crate::build::build_buildable_side(
        &mut commands,
        ship_base,
        half,
        3,
        Vec2::X,
        meshes,
        materials,
    );
    crate::build::mount_preplaced_turret(
        &mut commands,
        ship_base,
        &right[MID],
        Vec2::X,
        meshes,
        materials,
    );

    // Left side: a docking airlock (opens the doorway so the crew can board a
    // docked structure).
    let left = crate::build::build_buildable_side(
        &mut commands,
        ship_base,
        half,
        3,
        Vec2::NEG_X,
        meshes,
        materials,
    );
    crate::build::mount_preplaced_dock(
        &mut commands,
        ship_base,
        &[&left[MID]],
        Vec2::NEG_X,
        meshes,
        materials,
    );

    // Engineering console: the ship hull is its engineering module; interacting
    // with this console (E) opens build mode for this ship.
    crate::build::spawn_build_console(
        ship_base,
        Vec2::new(0., -30.),
        &mut commands,
        meshes,
        materials,
    );

    ship_base
}
