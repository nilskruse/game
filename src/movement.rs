use avian2d::prelude::*;
use bevy::prelude::*;

use crate::{
    build::{BuiltModule, UNIT},
    docking::Docked,
    player::Seated,
    ship::{PlayerShip, ThrustCommand, Thruster, ThrusterNozzle, NOZZLE_BLOCKED, NOZZLE_OPEN},
};

/// Nozzle color while actively firing.
const NOZZLE_FIRING: Color = Color::srgb(1.0, 0.55, 0.1);

/// How far off the push's axis-through-the-CoM a thruster must sit to steer (its
/// lever arm). Only the offset *perpendicular* to the push counts — a sideways
/// engine mounted high steers even though it's also far out to the side. Half a
/// cell, so a near-centered engine doesn't spin the ship; tunable.
const ROTATION_LEVER_MIN: f32 = 0.5 * UNIT;

/// Which way a push at `offset` from the center of mass turns the ship: `+1`
/// counter-clockwise, `-1` clockwise, `0` if it runs through (or near) the CoM and
/// can't steer. Uses only the lever arm's *sign* past the minimum — no scaling by
/// how far out it is (no leverage), just "off-center enough and pushing across".
fn rotation_sense(offset: Vec2, push: Vec2) -> f32 {
    // Perpendicular offset (the lever arm); `push` is a unit vector.
    let lever = offset.x * push.y - offset.y * push.x;
    if lever > ROTATION_LEVER_MIN {
        1.0
    } else if lever < -ROTATION_LEVER_MIN {
        -1.0
    } else {
        0.0
    }
}

#[derive(Hash, Eq, PartialEq, Default, Copy, Clone, Debug)]
pub enum Movement {
    #[default]
    Idle,
    Moving,
}

// Thrust drives motion through these gains. Top speed in a direction is
// `thrust_there / mass * SPEED_GAIN`; you ramp toward it (and brake toward zero)
// at `thrust_there / mass * ACCEL_GAIN` per second — so more thrust or less mass
// means both faster and quicker to respond. All tunable.
const LIN_SPEED_GAIN: f32 = 250.0;
const LIN_ACCEL_GAIN: f32 = 200.0;
const ROT_SPEED_GAIN: f32 = 3.5;
const ROT_ACCEL_GAIN: f32 = 7.0;

/// The total thrust a ship can produce, resolved in its own local frame.
#[derive(Default)]
struct ThrustPools {
    /// Linear thrust pushing the ship +X / -X / +Y / -Y (ship-local).
    pos_x: f32,
    neg_x: f32,
    pos_y: f32,
    neg_y: f32,
    /// Rotational thrust turning the ship counter-clockwise / clockwise.
    ccw: f32,
    cw: f32,
}

/// Steer the piloted ship by its thrusters. A ship can only accelerate (or brake,
/// or turn) in a direction it has a thruster pushing that way — there's no free
/// movement. Releasing the controls auto-brakes toward rest, but only as fast as
/// the *opposing* thrusters allow (no reverse thruster => you coast forever).
///
/// Motion is momentum-based: thrust ramps velocity toward a thrust/mass-scaled top
/// speed rather than snapping to it. Rotation uses mass too (no moment of inertia /
/// lever arm), but a thruster must sit off the center of mass to turn the ship.
pub(crate) fn handle_input_ship(
    time: Res<Time>,
    keyboard_input: Res<ButtonInput<KeyCode>>,
    mut ships: Query<
        (
            Entity,
            &mut LinearVelocity,
            &mut AngularVelocity,
            &Transform,
            &GlobalTransform,
            &ComputedMass,
            &ComputedCenterOfMass,
            &mut ThrustCommand,
            Has<Docked>,
        ),
        With<PlayerShip>,
    >,
    pilots: Query<&Seated>,
    thrusters: Query<(Entity, &Thruster, &GlobalTransform)>,
    modules: Query<(Entity, &BuiltModule, &GlobalTransform)>,
    parents: Query<&ChildOf>,
) {
    let dt = time.delta_secs();
    for (ship, mut lin, mut ang, transform, ship_gt, mass, com, mut command, docked) in
        ships.iter_mut()
    {
        // Docked: locked to a static structure, hold still (and fire nothing).
        if docked {
            lin.0 = Vec2::ZERO;
            ang.0 = 0.0;
            *command = ThrustCommand::default();
            continue;
        }

        // Only a piloted ship is steered (and auto-braked); otherwise it coasts.
        if !pilots.iter().any(|seated| seated.ship == ship) {
            *command = ThrustCommand::default();
            continue;
        }

        let mass = mass.value();
        if mass <= 0.0 {
            continue;
        }
        let pools = collect_thrust(ship, ship_gt, com.0, &thrusters, &modules, &parents);

        // Inputs (ship-local): Q/E strafe (+X right), W/S forward (+Y), A/D rotate
        // (+rot counter-clockwise).
        let in_x = axis(&keyboard_input, KeyCode::KeyE, KeyCode::KeyQ);
        let in_y = axis(&keyboard_input, KeyCode::KeyW, KeyCode::KeyS);
        let in_rot = axis(&keyboard_input, KeyCode::KeyA, KeyCode::KeyD);

        // Linear: work in the ship's local frame so "forward" tracks its facing.
        let rot = transform.rotation;
        let local_v = rot.inverse().mul_vec3(lin.0.extend(0.)).truncate();

        // Record which way thrust is commanded (input, or auto-brake when idle) so
        // the firing nozzles can be lit. Uses pre-step velocity / spin.
        *command = ThrustCommand {
            linear: Vec2::new(
                cmd_axis(in_x, local_v.x, 1.0),
                cmd_axis(in_y, local_v.y, 1.0),
            ),
            rotation: cmd_axis(in_rot, ang.0, 0.05),
        };

        let target = Vec2::new(
            target_speed(in_x, pools.pos_x, pools.neg_x, mass),
            target_speed(in_y, pools.pos_y, pools.neg_y, mass),
        );
        let new_local = Vec2::new(
            step(
                local_v.x,
                target.x,
                pools.pos_x,
                pools.neg_x,
                mass,
                LIN_ACCEL_GAIN,
                dt,
            ),
            step(
                local_v.y,
                target.y,
                pools.pos_y,
                pools.neg_y,
                mass,
                LIN_ACCEL_GAIN,
                dt,
            ),
        );
        lin.0 = rot.mul_vec3(new_local.extend(0.)).truncate();

        // Rotation: ramp toward a target spin, braking via the opposing thrust.
        let target_w = if in_rot > 0.0 {
            pools.ccw / mass * ROT_SPEED_GAIN
        } else if in_rot < 0.0 {
            -(pools.cw / mass * ROT_SPEED_GAIN)
        } else {
            0.0
        };
        ang.0 = step(
            ang.0,
            target_w,
            pools.ccw,
            pools.cw,
            mass,
            ROT_ACCEL_GAIN,
            dt,
        );
    }
}

/// `+1` if `pos` is held and `neg` isn't, `-1` for the reverse, else `0`.
fn axis(keyboard: &ButtonInput<KeyCode>, pos: KeyCode, neg: KeyCode) -> f32 {
    keyboard.pressed(pos) as i32 as f32 - keyboard.pressed(neg) as i32 as f32
}

/// The sign of thrust commanded on an axis: the `input` direction if pressed,
/// otherwise the auto-brake direction (opposing the current `vel`, once it exceeds
/// `eps`), otherwise none. Used to light the firing nozzles.
fn cmd_axis(input: f32, vel: f32, eps: f32) -> f32 {
    if input != 0.0 {
        input.signum()
    } else if vel.abs() > eps {
        // Idle: auto-brake by thrusting opposite the current motion.
        -vel.signum()
    } else {
        0.0
    }
}

/// Target velocity for one axis: zero with no input, otherwise the thrust/mass
/// top speed in the pressed direction (using that direction's thrust pool).
fn target_speed(input: f32, thrust_pos: f32, thrust_neg: f32, mass: f32) -> f32 {
    if input > 0.0 {
        thrust_pos / mass * LIN_SPEED_GAIN
    } else if input < 0.0 {
        -(thrust_neg / mass * LIN_SPEED_GAIN)
    } else {
        0.0
    }
}

/// Move `cur` toward `target`, limited by the thrust available in the direction of
/// change (so braking needs an opposing thruster). `thrust_pos`/`thrust_neg` are
/// the thrust pushing the value up / down.
fn step(
    cur: f32,
    target: f32,
    thrust_pos: f32,
    thrust_neg: f32,
    mass: f32,
    gain: f32,
    dt: f32,
) -> f32 {
    let delta = target - cur;
    let avail = if delta > 0.0 { thrust_pos } else { thrust_neg };
    let max_step = avail / mass * gain * dt;
    cur + delta.clamp(-max_step, max_step)
}

/// Sum a ship's thrusters into its local-frame thrust pools. A thruster pushes the
/// ship along each of its directions; if it sits off the center of mass, that push
/// also spins the ship (sign of the cross product), with no lever-arm scaling.
fn collect_thrust(
    ship: Entity,
    ship_gt: &GlobalTransform,
    com: Vec2,
    thrusters: &Query<(Entity, &Thruster, &GlobalTransform)>,
    modules: &Query<(Entity, &BuiltModule, &GlobalTransform)>,
    parents: &Query<&ChildOf>,
) -> ThrustPools {
    let inv = ship_gt.affine().inverse();
    let mut p = ThrustPools::default();
    for (entity, thruster, gt) in thrusters.iter() {
        if root_of(entity, parents) != ship {
            continue;
        }
        let offset = inv.transform_point3(gt.translation()).truncate() - com;
        // A push whose exhaust runs straight into a neighbouring module is dead.
        let live = |dir: Vec2| !exhaust_blocked(gt, dir, entity, ship, modules, parents);

        for &dir in &thruster.directions {
            if !live(dir) {
                // Exhaust runs straight into a neighbouring module — dead.
                continue;
            }
            // Linear: the push feeds its direction's pool.
            if dir.x > 0.5 {
                p.pos_x += thruster.strength;
            } else if dir.x < -0.5 {
                p.neg_x += thruster.strength;
            }
            if dir.y > 0.5 {
                p.pos_y += thruster.strength;
            } else if dir.y < -0.5 {
                p.neg_y += thruster.strength;
            }
            // Rotation: a sideways push off the center of mass also spins the ship.
            let sense = rotation_sense(offset, dir);
            if sense > 0.0 {
                p.ccw += thruster.strength;
            } else if sense < 0.0 {
                p.cw += thruster.strength;
            }
        }
    }
    p
}

/// Whether the exhaust of a thruster at `thruster_gt` pushing in `push` is blocked:
/// is the cell one unit out along the exhaust (`-push`, in world space) occupied by
/// a module of the same `structure` (other than the thruster's own `module`)?
fn exhaust_blocked(
    thruster_gt: &GlobalTransform,
    push: Vec2,
    module: Entity,
    structure: Entity,
    modules: &Query<(Entity, &BuiltModule, &GlobalTransform)>,
    parents: &Query<&ChildOf>,
) -> bool {
    let exhaust = thruster_gt
        .affine()
        .transform_vector3((-push).extend(0.))
        .truncate()
        .normalize_or_zero();
    let cell = thruster_gt.translation().truncate() + exhaust * UNIT;
    module_at(cell, module, structure, modules, parents)
}

/// Whether a module of `structure` (other than `exclude`) covers world point `cell`.
fn module_at(
    cell: Vec2,
    exclude: Entity,
    structure: Entity,
    modules: &Query<(Entity, &BuiltModule, &GlobalTransform)>,
    parents: &Query<&ChildOf>,
) -> bool {
    for (entity, built, gt) in modules.iter() {
        if entity == exclude || root_of(entity, parents) != structure {
            continue;
        }
        let local = gt
            .affine()
            .inverse()
            .transform_point3(cell.extend(0.))
            .truncate();
        let h = built.size / 2.;
        if local.x.abs() < h.x - 0.5 && local.y.abs() < h.y - 0.5 {
            return true;
        }
    }
    false
}

/// Walk up the `ChildOf` chain to a thruster's structure root.
fn root_of(mut entity: Entity, parents: &Query<&ChildOf>) -> Entity {
    while let Ok(child_of) = parents.get(entity) {
        entity = child_of.parent();
    }
    entity
}

/// Animate each thruster nozzle: a firing exhaust flares bright and pulses, a
/// blocked one shows red, an idle one sits dark. "Firing" means its push direction
/// is currently commanded (input or auto-brake, see [`ThrustCommand`]) and clear.
pub(crate) fn animate_thrusters(
    time: Res<Time>,
    mut nozzles: Query<(
        &ThrusterNozzle,
        &ChildOf,
        &MeshMaterial2d<ColorMaterial>,
        &mut Transform,
    )>,
    transforms: Query<&GlobalTransform>,
    ships: Query<(&GlobalTransform, &ComputedCenterOfMass, &ThrustCommand)>,
    modules: Query<(Entity, &BuiltModule, &GlobalTransform)>,
    parents: Query<&ChildOf>,
    mut materials: ResMut<Assets<ColorMaterial>>,
) {
    // A shared flicker so all firing nozzles pulse together like exhaust.
    let pulse = 1.3 + 0.25 * (time.elapsed_secs() * 20.0).sin();

    for (nozzle, child_of, material, mut transform) in &mut nozzles {
        let module = child_of.parent();
        let Ok(module_gt) = transforms.get(module) else {
            continue;
        };
        let structure = root_of(module, &parents);

        // Blocked? (exhaust runs straight into a neighbouring module)
        let exhaust_world = module_gt
            .affine()
            .transform_vector3(nozzle.exhaust.extend(0.))
            .truncate()
            .normalize_or_zero();
        let cell = module_gt.translation().truncate() + exhaust_world * UNIT;
        let blocked = module_at(cell, module, structure, &modules, &parents);

        // Firing? (its push direction is commanded and not blocked)
        let push = -nozzle.exhaust;
        let firing = !blocked
            && ships.get(structure).is_ok_and(|(ship_gt, com, cmd)| {
                let linear = push.x * cmd.linear.x > 0.5 || push.y * cmd.linear.y > 0.5;
                let steers = cmd.rotation != 0.0 && {
                    let pos = ship_gt
                        .affine()
                        .inverse()
                        .transform_point3(module_gt.translation())
                        .truncate();
                    rotation_sense(pos - com.0, push) * cmd.rotation > 0.0
                };
                linear || steers
            });

        let (color, scale) = if firing {
            (NOZZLE_FIRING, pulse)
        } else if blocked {
            (NOZZLE_BLOCKED, 1.0)
        } else {
            (NOZZLE_OPEN, 1.0)
        };
        transform.scale = Vec3::splat(scale);
        if let Some(mut mat) = materials.get_mut(&material.0) {
            mat.color = color;
        }
    }
}

/// Light damping so coasting craft (and other loose bodies) settle. The piloted
/// ship is excluded — its braking is the thruster-gated auto-brake above, not free
/// drag, so a ship with no reverse thruster genuinely can't slow down.
pub fn apply_movement_damping(
    time: Res<Time>,
    mut query: Query<&mut LinearVelocity, Without<PlayerShip>>,
) {
    let delta_time = time.delta_secs();
    for mut linear_velocity in &mut query {
        linear_velocity.x *= 1.0 / (1.0 + 0.9 * delta_time);
        linear_velocity.y *= 1.0 / (1.0 + 0.9 * delta_time);
    }
}
