use crate::{
    animation::{Animated, Animations},
    build::{build_buildable_side, mount, mount_preplaced_turret, ModuleKind},
    character::Character,
    docking::{Docked, Docking},
    faction::{Faction, InFaction},
    health::{self, Health},
    ship::{Piloted, PlayerShip, ShipBase, StructureRoot, ThrustCommand, ThrustControl},
};
use avian2d::prelude::*;
use bevy::{app::Propagate, prelude::*};

/// Marks an AI-flown ship. `fly_enemy_ships` steers it toward its target and holds
/// it at `engage_range`; the ship's turret aims and fires on its own (by faction).
#[derive(Component)]
pub struct ShipAi {
    /// Preferred distance to hold from the target (world units).
    pub engage_range: f32,
}

pub fn spawn_enemy(
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    mut texture_atlas_layouts: ResMut<Assets<TextureAtlasLayout>>,
) {
    let texture = asset_server.load("Factions/Goblins/Troops/Torch/Purple/Torch_Purple.png");

    let layout = TextureAtlasLayout::from_grid(UVec2::splat(192), 7, 6, None, None);

    let texture_atlas_layout = texture_atlas_layouts.add(layout);

    let animations = Animations::from([
        ("idle-left", (0, 6, true)),
        ("idle-right", (0, 6, false)),
        ("walk-left", (7, 12, true)),
        ("walk-right", (7, 12, false)),
        ("attack-right", (14, 19, false)),
        ("attack-left", (20, 25, true)),
        ("attack-down", (26, 31, false)),
        ("attack-up", (32, 37, false)),
    ]);

    commands
        .spawn((
            Character::default(),
            Propagate(InFaction(Faction::Enemy)),
            Health { current: 1000. },
            Animated {
                animations,

                ..Default::default()
            },
            RigidBody::Dynamic,
            Collider::rectangle(25., 25.),
            Transform::from_xyz(-100., 0., 1.),
            Sprite::from_atlas_image(
                texture,
                TextureAtlas {
                    layout: texture_atlas_layout,

                    index: 0,
                },
            ),
            LockedAxes::ROTATION_LOCKED,
        ))
        .observe(health::on_health_expired);
}

/// Startup system: spawn the authored enemy ship.
pub fn spawn_enemy_ship(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<ColorMaterial>>,
) {
    build_enemy_ship(&mut commands, &mut meshes, &mut materials);
}

/// Build the authored enemy ship (callable from startup and from save-load content
/// injection). A size-3 hull with a flight loadout so the AI can actually maneuver:
/// a main engine for forward thrust and a pair of off-center maneuvering thrusters
/// for reverse, strafing and rotation. Built through the shared module path.
pub(crate) fn build_enemy_ship(
    commands: &mut Commands,
    meshes: &mut ResMut<Assets<Mesh>>,
    materials: &mut ResMut<Assets<ColorMaterial>>,
) {
    let ship_rectangle = Rectangle::new(150., 150.);
    let ship_base = spawn_enemy_ship_base(ship_rectangle, commands.reborrow(), meshes, materials);
    let half = ship_rectangle.half_size;
    const MID: usize = 1;

    // Bottom: main engine (forward thrust, +Y).
    let bottom = build_buildable_side(commands, ship_base, half, 3, Vec2::NEG_Y, meshes, materials);
    mount(
        commands,
        ship_base,
        &[&bottom[MID]],
        Vec2::NEG_Y,
        ModuleKind::Engine,
        meshes,
        materials,
    );

    // Top corners: maneuvering thrusters (reverse / strafe / rotation), off-center so
    // they can spin the ship.
    let top = build_buildable_side(commands, ship_base, half, 3, Vec2::Y, meshes, materials);
    for corner in [&top[0], &top[2]] {
        mount(
            commands,
            ship_base,
            &[corner],
            Vec2::Y,
            ModuleKind::Thruster,
            meshes,
            materials,
        );
    }

    // Right: a turret. It inherits the Enemy faction via hierarchy propagation, so it
    // targets and fires on the player on its own.
    let right = build_buildable_side(commands, ship_base, half, 3, Vec2::X, meshes, materials);
    mount_preplaced_turret(
        commands,
        ship_base,
        &right[MID],
        Vec2::X,
        crate::ship::turret::TurretKind::Cannon,
        crate::ship::turret::FireArc::OverShip,
        meshes,
        materials,
    );
}

pub fn spawn_enemy_ship_base(
    rectangle: Rectangle,
    mut commands: Commands,
    meshes: &mut ResMut<Assets<Mesh>>,
    materials: &mut ResMut<Assets<ColorMaterial>>,
) -> Entity {
    let ship_base = commands
        .spawn((
            ShipBase,
            crate::save::Origin::Authored("enemy_ship".to_string()),
            ShipAi { engage_range: 550. },
            // Hull = engineering module; its health feeds the ship total pool.
            crate::health::ModuleHealth::new(300., 10.),
            crate::health::ShipHealth::default(),
            Propagate(InFaction(Faction::Enemy)),
            // Drivable by the same shared solver as the player ship; the AI sets the
            // `ThrustControl` intent (and adds `Piloted`) — no flight code is
            // player-specific.
            ThrustControl::default(),
            ThrustCommand::default(),
            RigidBody::Dynamic,
            // Start well away from the player so the approach is visible.
            Transform::from_xyz(-700., 350., 1.),
            Collider::from(rectangle),
            Mesh2d(meshes.add(rectangle)),
            MeshMaterial2d(materials.add(Color::srgb(0.8, 0.2, 0.2))),
        ))
        .id();
    commands
        .entity(ship_base)
        .insert(Propagate(StructureRoot(ship_base)));
    ship_base
}

/// Fly each AI ship toward its target: rotate to point its nose (+Y) at the target,
/// and thrust forward to close to `engage_range`, backing off if too near (its turret
/// does the shooting). Sets the same `ThrustControl` intent + `Piloted` marker the
/// player's controller uses, so the shared `drive_ships` solver does the rest.
///
/// Targets the player ship for now; a fuller version would pick the nearest
/// opposing-faction ship.
pub(crate) fn fly_enemy_ships(
    mut commands: Commands,
    target: Query<&Position, With<PlayerShip>>,
    mut ships: Query<
        (
            Entity,
            &Position,
            &Rotation,
            &ShipAi,
            &mut ThrustControl,
            Has<Piloted>,
        ),
        (With<ShipBase>, Without<Docking>, Without<Docked>),
    >,
) {
    let Ok(target_pos) = target.single() else {
        return;
    };

    for (entity, pos, rot, ai, mut control, piloted) in &mut ships {
        let to_target = target_pos.0 - pos.0;
        let dist = to_target.length();
        if dist < 1e-3 {
            continue;
        }

        // Heading that points the ship's forward (+Y) at the target (matches the
        // dock/turret +Y-faces-outward convention).
        let desired = (-to_target.x).atan2(to_target.y);
        let ang_err = wrap_pi(desired - rot.as_radians());
        let rotation = if ang_err > 0.05 {
            1.0
        } else if ang_err < -0.05 {
            -1.0
        } else {
            0.0
        };

        // Only drive forward once roughly facing the target (otherwise turn first),
        // then hold a band around `engage_range`. With no input the solver auto-brakes.
        let forward = if ang_err.abs() > 0.5 {
            0.0
        } else if dist > ai.engage_range * 1.1 {
            1.0
        } else if dist < ai.engage_range * 0.9 {
            -1.0
        } else {
            0.0
        };

        *control = ThrustControl {
            linear: Vec2::new(0., forward),
            rotation,
        };
        if !piloted {
            commands.entity(entity).insert(Piloted);
        }
    }
}

/// Wrap an angle to `(-π, π]`.
fn wrap_pi(a: f32) -> f32 {
    use std::f32::consts::{PI, TAU};
    (a + PI).rem_euclid(TAU) - PI
}
