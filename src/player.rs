use avian2d::{dynamics::solver::solver_body::SolverBody, prelude::*};
use bevy::prelude::*;

use crate::{
    animation::{Animated, Animations},
    character::Character,
    ship::{GameLayer, PlayerShip, ShipBase},
};

/// Rotate a 2D vector by `angle` radians.
fn rotate_vec(v: Vec2, angle: f32) -> Vec2 {
    let (s, c) = angle.sin_cos();
    Vec2::new(c * v.x - s * v.y, s * v.x + c * v.y)
}

/// Wrap an angle into [-pi, pi].
fn wrap_pi(a: f32) -> f32 {
    (a + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU) - std::f32::consts::PI
}

/// Snap a vector to the nearest cardinal axis as a unit vector. Used to recover
/// an axis-aligned wall's exact local normal from a slightly-stale contact
/// normal.
fn snap_to_axis(v: Vec2) -> Vec2 {
    if v.x.abs() >= v.y.abs() {
        Vec2::new(v.x.signum(), 0.0)
    } else {
        Vec2::new(0.0, v.y.signum())
    }
}

#[derive(Component)]
#[require(Character)]
pub struct Player;

#[derive(Component)]
pub struct OnShip {
    ship_entity: Entity,
    relative_transform: Transform,
}

/// Desired walking direction in the ship's local frame, set from input.
#[derive(Component, Default)]
pub struct MoveInput(pub Vec2);

/// Snapshot taken before the physics step so the carry can be corrected against
/// the ship's *actual* motion afterwards. See `drive_player_on_ship` /
/// `correct_player_carry`.
#[derive(Component, Default)]
pub struct CarryState {
    /// Player position relative to the ship's center of mass, in world space.
    rel_pre: Vec2,
    ship_com_pre: Vec2,
    ship_rot_pre: f32,
    /// The commanded ship velocity we fed the carry (may differ from reality).
    ship_lin_cmd: Vec2,
    ship_ang_cmd: f32,
    valid: bool,
}

pub fn spawn_player(
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    mut texture_atlas_layouts: ResMut<Assets<TextureAtlasLayout>>,
    player_ship: Single<(Entity, &GlobalTransform), With<PlayerShip>>,
) {
    // let texture = asset_server.load("Factions/Knights/Troops/Warrior/Blue/Warrior_Blue.png");
    let layout = TextureAtlasLayout::from_grid(UVec2::splat(192), 6, 8, None, None);
    let texture_atlas_layout = texture_atlas_layouts.add(layout);
    let animations = Animations::from([
        ("idle-left", (0, 5, true)),
        ("idle-right", (0, 5, false)),
        ("walk-left", (6, 11, true)),
        ("walk-right", (6, 11, false)),
        ("attack-right", (12, 17, false)),
        ("attack-right-2", (18, 23, false)),
        ("attack-left", (12, 17, true)),
        ("attack-left-2", (18, 23, true)),
        ("attack-down", (24, 29, false)),
        ("attack-down-2", (30, 35, false)),
        ("attack-up", (36, 41, false)),
        ("attack-up-2", (42, 47, false)),
    ]);

    let (ship_entity, ship_transform) = *player_ship;
    let player_entity = commands
        .spawn((
            Player,
            Animated {
                animations,
                ..Default::default()
            },
            RigidBody::Dynamic,
            // The player's facing is slaved to the ship (see drive_player_on_ship),
            // so lock rotation to stop wall-contact torque from tilting it.
            LockedAxes::ROTATION_LOCKED,
            // Player is far lighter than the ship: let the ship treat it as
            // negligible so walking into a wall never shoves the ship.
            Dominance(-1),
            MoveInput::default(),
            CarryState::default(),
            Collider::rectangle(25., 25.),
            // Frictionless: the carry already moves the player tangentially with
            // the ship, so contact friction would only drag it along walls when
            // the ship turns (the chord-vs-arc velocity mismatch). Min combine
            // makes the contact frictionless regardless of the wall's friction.
            Friction {
                dynamic_coefficient: 0.,
                static_coefficient: 0.,
                combine_rule: CoefficientCombine::Min,
            },
            // Restitution {
            //     coefficient: 0.,
            //     combine_rule: CoefficientCombine::Min,
            // },
            // CollisionEventsEnabled,
            // Transform::default(),
            // TransformInterpolation,
            ship_transform.compute_transform(),
            // Sprite::from_atlas_image(
            //     texture,
            //     TextureAtlas {
            //         layout: texture_atlas_layout,
            //         index: 0,
            //     },
            // ),
            OnShip {
                ship_entity: ship_entity,
                relative_transform: Transform::from_xyz(25., 0., 0.),
            },
            CollisionLayers::new(GameLayer::Walls, [GameLayer::Walls]),
            // SweptCcd::default(),
            // CustomPositionIntegration,
            // CustomVelocityIntegration,
        ))
        .id();

    // let joint = commands.spawn((FixedJoint::new(ship_entity, player_entity)));
}

/// Read keyboard into the player's local-frame walk direction.
pub fn read_player_input(
    keyboard_input: Res<ButtonInput<KeyCode>>,
    mut query: Query<&mut MoveInput, With<Player>>,
) {
    for mut input in query.iter_mut() {
        let mut v = Vec2::ZERO;
        if keyboard_input.pressed(KeyCode::ArrowUp) {
            v.y += 1.0;
        }
        if keyboard_input.pressed(KeyCode::ArrowDown) {
            v.y -= 1.0;
        }
        if keyboard_input.pressed(KeyCode::ArrowLeft) {
            v.x -= 1.0;
        }
        if keyboard_input.pressed(KeyCode::ArrowRight) {
            v.x += 1.0;
        }
        input.0 = v;
    }
}

/// Drive the player as a dynamic body riding the ship: its velocity is the
/// velocity of the ship point beneath it (so it's carried + spun along) plus
/// its own walk input rotated into the ship's frame. Runs before the physics
/// step so the solver resolves wall collisions instead of us teleporting
/// through them.
pub fn drive_player_on_ship(
    time: Res<Time>,
    collisions: Collisions,
    mut query: Query<
        (
            Entity,
            &Position,
            &MoveInput,
            &mut LinearVelocity,
            &mut CarryState,
            &OnShip,
        ),
        Without<ShipBase>,
    >,
    ship: Query<
        (
            &Position,
            &Rotation,
            &LinearVelocity,
            &AngularVelocity,
            &ComputedCenterOfMass,
        ),
        With<ShipBase>,
    >,
) {
    const SPEED: f32 = 210.0;
    let dt = time.delta_secs();
    for (player_entity, player_pos, input, mut player_vel, mut carry_state, on_ship) in
        query.iter_mut()
    {
        let Ok((ship_pos, ship_rot, ship_lin, ship_ang_vel, ship_com_local)) =
            ship.get(on_ship.ship_entity)
        else {
            continue;
        };

        // Carry with the ship's *commanded* velocity (set this tick) so there's
        // zero lag. This is only a prediction: if a collision blocks or
        // redirects the ship, `correct_player_carry` fixes the player up after
        // the step using the ship's real motion.
        let ship_vel = ship_lin.0;
        let ship_ang = ship_ang_vel.0;

        // The ship rotates about its center of mass, not its `Position` origin
        // (its walls are uneven, so the COM is offset). Pivot around the global
        // COM and treat `LinearVelocity` as that COM's velocity.
        let ship_com = ship_pos.0 + ship_rot * ship_com_local.0;

        // Velocity that keeps the player locked to the ship point under it.
        // Using the *finite* rotation over this tick (not the linear tangent
        // `omega x r`) lands the player exactly on the swept arc each step, so
        // there's no rotational drift across the deck.
        let r = player_pos.0 - ship_com;
        let carry = if dt > 0.0 {
            let rotated_r = rotate_vec(r, ship_ang * dt);
            ship_vel + (rotated_r - r) / dt
        } else {
            ship_vel
        };

        // Snapshot for the post-step correction.
        *carry_state = CarryState {
            rel_pre: r,
            ship_com_pre: ship_com,
            ship_rot_pre: ship_rot.as_radians(),
            ship_lin_cmd: ship_vel,
            ship_ang_cmd: ship_ang,
            valid: true,
        };

        // Player's own walking, in the ship's local frame (input is already
        // local). We do the wall projection here, in local space, because the
        // ship's walls are axis-aligned in its frame.
        let mut walk_local = input.0.normalize_or_zero() * SPEED;

        // Strip out any part of the walk that points into a wall we're already
        // touching, so we don't command velocity into it (which the solver then
        // pushes back out along a rotated normal, sliding us along the wall).
        // The contact normal from `Collisions` is one step stale, so converting
        // it to the ship frame and snapping it to the nearest local axis
        // recovers the true wall normal regardless of how far the ship has
        // turned since.
        for contact in collisions.collisions_with(player_entity) {
            for manifold in &contact.manifolds {
                // `normal` points from collider1 to collider2 in world space;
                // orient it to point out of the wall, toward the player.
                let out_of_wall = if contact.collider1 == player_entity {
                    -manifold.normal
                } else {
                    manifold.normal
                };
                let local_n = snap_to_axis(ship_rot.inverse() * out_of_wall);
                let into_wall = walk_local.dot(local_n);
                if into_wall < 0.0 {
                    walk_local -= into_wall * local_n;
                }
            }
        }

        let walk = ship_rot * walk_local;
        player_vel.0 = carry + walk;
    }
}

/// After the physics step, snap the player onto the spot it should occupy given
/// the ship's *actual* motion this step. When the ship moved exactly as
/// commanded (free flight) the shift is zero; when a collision stopped,
/// redirected, or rotated the ship, this removes the carry we over-predicted —
/// so the player neither glides through obstacles nor slides along walls, and
/// it follows collision-induced rotation. Runs after the solve but before
/// transform writeback, so the corrected pose renders this frame (no lag).
pub fn correct_player_carry(
    time: Res<Time>,
    mut players: Query<(&mut Position, &mut Rotation, &CarryState, &OnShip), Without<ShipBase>>,
    ships: Query<(&Position, &Rotation, &ComputedCenterOfMass), With<ShipBase>>,
) {
    let dt = time.delta_secs();
    for (mut player_pos, mut player_rot, state, on_ship) in players.iter_mut() {
        if !state.valid {
            continue;
        }
        let Ok((ship_pos, ship_rot, ship_com_local)) = ships.get(on_ship.ship_entity) else {
            continue;
        };
        let ship_com_post = ship_pos.0 + ship_rot * ship_com_local.0;

        // Where the player should be if rigidly attached to the ship's *actual*
        // post-step frame.
        let d_ang_actual = wrap_pi(ship_rot.as_radians() - state.ship_rot_pre);
        let actual_target = ship_com_post + rotate_vec(state.rel_pre, d_ang_actual);

        // Where the commanded carry actually placed the player's carry component.
        let commanded_endpoint = state.ship_com_pre
            + state.ship_lin_cmd * dt
            + rotate_vec(state.rel_pre, state.ship_ang_cmd * dt);

        // Shift by the difference; walk and wall-collision adjustments are
        // preserved because they sit on top of this carry component.
        player_pos.0 += actual_target - commanded_endpoint;

        // Slave the facing to the ship's true orientation (follows
        // collision-induced rotation, no lag).
        *player_rot = *ship_rot;
    }
}

//
pub fn sync_with_ship_via_transform(
    time: Res<Time>,
    mut query: Query<
        (
            &mut Transform,
            &mut LinearVelocity,
            &mut AngularVelocity,
            &OnShip,
        ),
        Without<ShipBase>,
    >,
    ship: Query<(&GlobalTransform, &LinearVelocity, &AngularVelocity), With<ShipBase>>,
) {
    for (mut player_transform, mut player_linear_velocity, mut player_angular_velocity, on_ship) in
        query.iter_mut()
    {
        let (ship_global_transform, ship_linear_velocity, ship_angular_velocity) =
            ship.get(on_ship.ship_entity).expect("on_ship");

        *player_transform = ship_global_transform
            .mul_transform(on_ship.relative_transform)
            .into();

        // let rotated_velocity =
        //     ship_global_transform.rotation() * player_linear_velocity.extend(0.0);
        // player_linear_velocity.0 = rotated_velocity.xy();
    }
}

pub fn sync_with_ship_via_position(
    mut query: Query<(&mut Position, &mut Rotation, &mut LinearVelocity, &OnShip), Without<ShipBase>>,
    ship: Query<(&Position, &Rotation, &LinearVelocity), With<ShipBase>>,
) {
    for (mut player_pos, mut player_rot, mut player_vel, on_ship) in query.iter_mut() {
        let Ok((ship_pos, ship_rot, ship_vel)) = ship.get(on_ship.ship_entity) else { continue };

        // Rotate the offset by the ship's current rotation
        let offset = ship_rot * on_ship.relative_transform.translation.xy();
        player_pos.0 = ship_pos.0 + offset;

        // Inherit ship rotation
        *player_rot = *ship_rot;

        // player_vel.0 = ship_vel.0;
    }
}
// pub fn sync_after(
//     mut query: Query<(&mut Transform, &mut LinearVelocity, &mut OnShip), Without<ShipBase>>,
//     ship: Query<(&GlobalTransform, &LinearVelocity, &AngularVelocity), With<ShipBase>>,
// ) {
//     for (mut player_transform, mut player_linear_velocity, mut on_ship) in query.iter_mut() {
//         let (ship_global_transform, ship_linear_velocity, ship_angular_velocity) =
//             ship.get(on_ship.ship_entity).expect("on_ship");
//         on_ship.relative_transform.translation = ship_global_transform.rotation().inverse()
//             * (player_transform.translation - ship_global_transform.translation());
//     }
// }

pub fn handle_input_transform(
    keyboard_input: Res<ButtonInput<KeyCode>>,
    mut query: Query<&mut OnShip, With<Player>>,
    time: Res<Time>,
) {
    const SPEED: f32 = 210.0;
    for mut oh_ship in query.iter_mut() {
        let mut x = 0.;
        let mut y = 0.;
        if keyboard_input.pressed(KeyCode::ArrowUp) {
            y += 1.0;
        }
        if keyboard_input.pressed(KeyCode::ArrowDown) {
            y -= 1.0;
        }
        if keyboard_input.pressed(KeyCode::ArrowLeft) {
            x -= 1.0;
        }
        if keyboard_input.pressed(KeyCode::ArrowRight) {
            x += 1.0;
        }

        oh_ship.relative_transform.translation +=
            Vec2::new(x, y).extend(0.).normalize_or_zero() * SPEED * time.delta_secs();
    }
}

pub fn handle_input(
    keyboard_input: Res<ButtonInput<KeyCode>>,
    mut query: Query<&mut LinearVelocity, With<Player>>,
) {
    const SPEED: f32 = 210.0;
    for mut linear_velocity in query.iter_mut() {
        let mut x = 0.;
        let mut y = 0.;
        if keyboard_input.pressed(KeyCode::ArrowUp) {
            y += 1.0;
        }
        if keyboard_input.pressed(KeyCode::ArrowDown) {
            y -= 1.0;
        }
        if keyboard_input.pressed(KeyCode::ArrowLeft) {
            x -= 1.0;
        }
        if keyboard_input.pressed(KeyCode::ArrowRight) {
            x += 1.0;
        }

        linear_velocity.0 = Vec2::new(x, y).normalize_or_zero() * SPEED;
    }
}

pub fn handle_input_2(
    keyboard_input: Res<ButtonInput<KeyCode>>,
    mut query: Query<&mut SolverBody, With<Player>>,
) {
    const SPEED: f32 = 210.0;
    for mut solver_body in query.iter_mut() {
        let mut x = 0.;
        let mut y = 0.;
        if keyboard_input.pressed(KeyCode::ArrowUp) {
            y += 1.0;
        }
        if keyboard_input.pressed(KeyCode::ArrowDown) {
            y -= 1.0;
        }
        if keyboard_input.pressed(KeyCode::ArrowLeft) {
            x -= 1.0;
        }
        if keyboard_input.pressed(KeyCode::ArrowRight) {
            x += 1.0;
        }

        solver_body.linear_velocity = Vec2::new(x, y).normalize_or_zero() * SPEED;
    }
}

pub fn sync_with_ship_in_substep(
    mut query: Query<(&mut SolverBody, &OnShip), Without<ShipBase>>,
    ship: Query<(&SolverBody), With<ShipBase>>,
) {
    for (mut player_solver_body, on_ship) in query.iter_mut() {
        let Ok(ship_solver_body) = ship.get(on_ship.ship_entity) else {
            error!("no ship solver body");
            continue;
        };
        player_solver_body.linear_velocity += ship_solver_body.linear_velocity;
        // player_solver_body.delta_position += ship_solver_body.delta_position;
    }
}

pub fn sync_after(
    mut query: Query<(&mut Transform, &mut LinearVelocity, &mut OnShip), Without<ShipBase>>,
    ship: Query<(&GlobalTransform, &LinearVelocity, &AngularVelocity), With<ShipBase>>,
) {
    // for (mut player_transform, mut player_linear_velocity, mut on_ship) in query.iter_mut() {
    //     let (ship_global_transform, ship_linear_velocity, ship_angular_velocity) =
    //         ship.get(on_ship.ship_entity).expect("on_ship");
    //     on_ship.relative_transform.translation = ship_global_transform.rotation().inverse()
    //         * (player_transform.translation - ship_global_transform.translation());
    // }
}
