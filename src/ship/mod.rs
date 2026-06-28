pub mod bullet;
pub mod turret;

use avian2d::prelude::*;
use bevy::{app::Propagate, prelude::*};

use crate::faction::{Faction, InFaction};

#[derive(Component)]
pub struct PlayerShip;

#[derive(Component)]
pub struct ShipBase;

/// A station the player can sit at to steer the ship. `local_offset` is its
/// position relative to the ship base origin, used to anchor the seated player
/// directly from the ship's physics `Position` (no transform-propagation lag).
#[derive(Component)]
pub struct PilotSeat {
    pub local_offset: Vec2,
}

pub fn spawn_player_ship(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<ColorMaterial>>,
) {
    let ship_rectangle = Rectangle::new(100., 100.);
    // The ship's starting turret and docking port are pre-mounted buildable
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

    // The hull is a size-2 body: every side is buildable, with two doorway slots
    // (sealed by removable panels) and two attach points. Top and bottom start
    // free; the right and left come with pre-mounted modules built through the
    // same path the build menu uses.
    for normal in [Vec2::Y, Vec2::NEG_Y] {
        crate::build::build_buildable_side(
            &mut commands,
            ship_base,
            half,
            2,
            normal,
            meshes,
            materials,
        );
    }

    // Right side: a starting turret on its first slot.
    let right = crate::build::build_buildable_side(
        &mut commands,
        ship_base,
        half,
        2,
        Vec2::X,
        meshes,
        materials,
    );
    crate::build::mount_preplaced_turret(
        &mut commands,
        ship_base,
        &right[0],
        Vec2::X,
        meshes,
        materials,
    );

    // Left side: a docking airlock on its first slot (opens the doorway so the
    // crew can board a docked structure).
    let left = crate::build::build_buildable_side(
        &mut commands,
        ship_base,
        half,
        2,
        Vec2::NEG_X,
        meshes,
        materials,
    );
    crate::build::mount_preplaced_dock(
        &mut commands,
        ship_base,
        &[&left[0]],
        Vec2::NEG_X,
        meshes,
        materials,
    );

    // Pilot seat: a small marker near the front of the ship the player can sit
    // at to steer. No collider so the player can walk onto it.
    let seat_offset = Vec2::new(0., 30.);
    let _pilot_seat = {
        let seat = Circle::new(8.);
        commands.spawn((
            PilotSeat {
                local_offset: seat_offset,
            },
            ChildOf(ship_base),
            Transform::from_xyz(seat_offset.x, seat_offset.y, 0.5),
            Mesh2d(meshes.add(seat)),
            MeshMaterial2d(materials.add(Color::srgb(0., 0.6, 1.))),
        ))
    };

    ship_base
}
