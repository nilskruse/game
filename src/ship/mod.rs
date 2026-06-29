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

/// A thruster module's contribution to a ship's motion. The ship can only thrust
/// (and rotate) in directions some thruster pushes; see `movement`. Each push also
/// steers the ship when the thruster is off the center of mass and pushes across it
/// (decided geometrically in `movement`). All are ship-local axis-aligned units;
/// `strength` is the thrust per direction. A push whose exhaust is blocked by a
/// neighbouring module produces no thrust.
#[derive(Component)]
pub struct Thruster {
    pub directions: Vec<Vec2>,
    pub strength: f32,
}

/// One exhaust-nozzle marker on a thruster module. `exhaust` is the ship-local
/// direction the exhaust leaves (opposite the push it represents); used to recolor
/// the nozzle when that exhaust is blocked.
#[derive(Component)]
pub struct ThrusterNozzle {
    pub exhaust: Vec2,
}

/// What a controller (the player, or an AI) wants the ship to do, in ship-local
/// signs: each of `linear.x` / `linear.y` / `rotation` is -1, 0, or +1. This is the
/// raw *intent* — a controller sets it (see `control_player_ship`), and the shared
/// solver (`drive_ships`) turns it into motion for any ship regardless of faction.
/// Auto-braking is the solver's job, not encoded here.
#[derive(Component, Default)]
pub struct ThrustControl {
    pub linear: Vec2,
    pub rotation: f32,
}

/// Marker on a ship root while a controller (player seated at its helm, or an AI)
/// is actively driving it. The solver auto-brakes a `Piloted` ship toward rest when
/// its [`ThrustControl`] is zero; an un-piloted ship (a drifting hulk) coasts.
#[derive(Component)]
pub struct Piloted;

/// The thrust the ship is currently exerting, in ship-local signs: each of
/// `linear.x` / `linear.y` / `rotation` is -1, 0, or +1. Computed every frame by the
/// solver (`drive_ships`) from the [`ThrustControl`] intent *plus* auto-braking (the
/// opposing direction fires to slow the ship when there's no input); drives which
/// thruster nozzles flare.
#[derive(Component, Default)]
pub struct ThrustCommand {
    pub linear: Vec2,
    pub rotation: f32,
}

/// The root structure entity (ship hull or station root) a part belongs to.
/// Propagated down each structure's hierarchy via `HierarchyPropagatePlugin` (set on
/// the root as `Propagate(StructureRoot(root))`, like [`InFaction`](crate::faction::InFaction)),
/// so systems resolve membership with an O(1) component read instead of walking the
/// `ChildOf` chain and scanning every part in the world each tick. A freshly built
/// part inherits it one frame later, when propagation next runs.
#[derive(Component, Clone, PartialEq, Debug)]
pub struct StructureRoot(pub Entity);

/// Nozzle color when its exhaust is clear (the thrust direction works).
pub const NOZZLE_OPEN: Color = Color::srgb(0.12, 0.12, 0.14);
/// Nozzle color when a neighbouring module blocks its exhaust (direction disabled).
pub const NOZZLE_BLOCKED: Color = Color::srgb(0.65, 0.18, 0.14);

pub fn spawn_player_ship(
    mut commands: Commands,
    registry: Res<crate::build::ModuleRegistry>,
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
        &registry,
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
    registry: &crate::build::ModuleRegistry,
    meshes: &mut ResMut<Assets<Mesh>>,
    materials: &mut ResMut<Assets<ColorMaterial>>,
) -> Entity {
    let ship_base = commands
        .spawn((
            PlayerShip,
            crate::save::Origin::Authored("player_ship".to_string()),
            Propagate(InFaction(Faction::Player)),
            ShipBase,
            // The hull is the engineering module: tough, well-armored. Its health
            // feeds the ship's total pool (see `ShipHealth` / `sync_ship_health`).
            crate::health::ModuleHealth::new(300., 10.),
            crate::health::ShipHealth::default(),
            ThrustControl::default(),
            ThrustCommand::default(),
            RigidBody::Dynamic,
            Transform::from_xyz(100., 0., 0.),
            Collider::from(rectangle),
            Mesh2d(meshes.add(rectangle)),
            MeshMaterial2d(materials.add(Color::srgb(1., 1., 0.))),
        ))
        .id();
    // Tag every part of this ship with its root, propagated down the hierarchy.
    commands
        .entity(ship_base)
        .insert(Propagate(StructureRoot(ship_base)));

    let half = rectangle.half_size;

    // The hull (the engineering module) is a size-3 body: every side is buildable,
    // with three doorway slots (sealed by removable panels) and three attach points.
    // The bottom starts free; the other three sides come with pre-mounted modules,
    // each centered on the middle slot, built through the same path the build menu
    // uses.
    const MID: usize = 1;

    // Bottom (aft) side: the main engine (pushes the ship forward), with an
    // auto-defense cannon (free arc) on a corner to cover the rear.
    let bottom = crate::build::build_buildable_side(
        &mut commands,
        ship_base,
        half,
        3,
        Vec2::NEG_Y,
        meshes,
        materials,
    );
    crate::build::mount(
        &mut commands,
        ship_base,
        &[&bottom[MID]],
        Vec2::NEG_Y,
        crate::build::ModuleKind::Engine,
        registry,
        meshes,
        materials,
    );
    crate::build::mount_preplaced_turret(
        &mut commands,
        ship_base,
        &bottom[0],
        Vec2::NEG_Y,
        crate::ship::turret::TurretKind::Cannon,
        crate::ship::turret::FireArc::OverShip,
        registry,
        meshes,
        materials,
    );

    // Top (front) side: the player-aimed cannon on the center slot, with maneuvering
    // thrusters on the two corners — off-center so they can spin the ship (they also
    // reverse and strafe).
    let top = crate::build::build_buildable_side(
        &mut commands,
        ship_base,
        half,
        3,
        Vec2::Y,
        meshes,
        materials,
    );
    crate::build::mount_preplaced_turret(
        &mut commands,
        ship_base,
        &top[MID],
        Vec2::Y,
        crate::ship::turret::TurretKind::PlayerCannon,
        crate::ship::turret::FireArc::Hull,
        registry,
        meshes,
        materials,
    );
    for corner in [&top[0], &top[2]] {
        crate::build::mount(
            &mut commands,
            ship_base,
            &[corner],
            Vec2::Y,
            crate::build::ModuleKind::Thruster,
            registry,
            meshes,
            materials,
        );
    }

    // Right (starboard) side: the cockpit with the pilot seat.
    let right = crate::build::build_buildable_side(
        &mut commands,
        ship_base,
        half,
        3,
        Vec2::X,
        meshes,
        materials,
    );
    crate::build::mount_preplaced_cockpit(
        &mut commands,
        ship_base,
        &right[MID],
        Vec2::X,
        registry,
        meshes,
        materials,
    );

    // Left side: a docking airlock (opens the doorway so the crew can board a
    // docked structure), with a point-defense turret on the corner to swat incoming
    // fire.
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
        registry,
        meshes,
        materials,
    );
    crate::build::mount_preplaced_turret(
        &mut commands,
        ship_base,
        &left[0],
        Vec2::NEG_X,
        crate::ship::turret::TurretKind::PointDefense,
        crate::ship::turret::FireArc::OverShip,
        registry,
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
