use std::f32::consts::PI;

use avian2d::prelude::*;
use bevy::{
    math::{Affine3A, EulerRot},
    prelude::*,
};

use crate::{
    player::Seated,
    ship::{ShipBase, StructureRoot},
};

/// A docking port module. It marks a point on a structure (a ship or a space
/// station) where two structures can latch together. Shared by both so the
/// docking logic can be written once against this component.
///
/// By convention a port "faces" along its local **+Y** axis (its outward
/// normal); two ports dock when they meet facing each other. Built as a child
/// entity, in the same hierarchical style as ship/station parts — see
/// [`spawn_docking_port`].
#[derive(Component, Default)]
pub struct DockingPort {
    /// The port we're currently docked to, if any. `None` when free.
    pub docked_to: Option<Entity>,
}

/// Marker on a structure's root that is currently docked (latched and locked in
/// place). Used to suppress steering input while docked.
#[derive(Component)]
pub struct Docked;

/// A ship in the middle of easing into its docked pose. While present, the ship is
/// kinematic and `advance_docking` lerps it toward `target_*`; on arrival it becomes
/// [`Docked`]. Steering is suppressed during the slide (see `movement`).
#[derive(Component)]
pub struct Docking {
    pub target_pos: Vec2,
    /// Target heading (radians).
    pub target_rot: f32,
}

/// How close two free ports must be (world units, port-to-port) to dock.
const DOCK_RANGE: f32 = 130.0;
/// Exponential approach rate for the docking slide (per second); higher snaps faster.
const DOCK_RATE: f32 = 2.0;
/// Stop the slide once within this distance / heading error of the target.
const DOCK_POS_EPS: f32 = 0.5;
const DOCK_ANG_EPS: f32 = 0.01;
/// Gentle nudge away from the partner on undock (world units / second).
const PUSHOFF_SPEED: f32 = 30.0;

/// Docking-port collar color: idle (no dock available) vs. ready/engaged (a valid
/// partner is lined up, or it's already latched).
const PORT_IDLE: Color = Color::srgb(1.0, 0.7, 0.1);
const PORT_READY: Color = Color::srgb(0.2, 1.0, 0.3);

/// Attach a docking port to `parent` at a local `offset`, rotated by `angle`
/// (radians) so its +Y points outward from the structure. Returns the port
/// entity.
///
/// The port is a **sensor**, so it detects an approaching port without
/// physically blocking it, and collision events are enabled for the docking
/// logic we'll add next.
pub fn spawn_docking_port(
    parent: Entity,
    offset: Vec2,
    angle: f32,
    mut commands: Commands,
    meshes: &mut ResMut<Assets<Mesh>>,
    materials: &mut ResMut<Assets<ColorMaterial>>,
) -> Entity {
    // A short collar bar, perpendicular to the facing (+Y) direction.
    let collar = Rectangle::new(40., 14.);
    commands
        .spawn((
            DockingPort::default(),
            ChildOf(parent),
            // Slightly in front in z so it reads on top of the hull.
            Transform::from_xyz(offset.x, offset.y, 0.2)
                .with_rotation(Quat::from_rotation_z(angle)),
            Collider::from(collar),
            Sensor,
            CollisionEventsEnabled,
            Mesh2d(meshes.add(collar)),
            MeshMaterial2d(materials.add(PORT_IDLE)),
        ))
        .id()
}

/// Snapshot of a docking port's world pose, gathered up-front so we can search
/// freely before mutating anything.
struct PortSnapshot {
    entity: Entity,
    /// The structure root this port is attached to.
    structure: Entity,
    position: Vec2,
    /// Outward facing normal (world space).
    normal: Vec2,
    affine: Affine3A,
    docked_to: Option<Entity>,
}

/// Dock / undock the piloted ship with the press of `G`.
///
/// You must be seated at the helm. If the ship's port is free and lined up with
/// another free port (in range, both facing each other), the ship begins easing into
/// the docked pose — the two ports meeting coincident and facing opposite (see
/// [`Docking`] / `advance_docking`). Press `G` again to release, which unlatches and
/// gives the ship a gentle pushoff away from the partner.
///
/// Runs in `Update` so the `just_pressed` edge is never missed.
pub fn toggle_dock(
    keyboard: Res<ButtonInput<KeyCode>>,
    pilots: Query<&Seated>,
    mut commands: Commands,
    mut ports: Query<(Entity, &mut DockingPort, &GlobalTransform, &StructureRoot)>,
    mut ships: Query<
        (
            &GlobalTransform,
            &mut Position,
            &mut Rotation,
            &mut LinearVelocity,
            &mut AngularVelocity,
        ),
        With<ShipBase>,
    >,
) {
    if !keyboard.just_pressed(KeyCode::KeyG) {
        return;
    }

    // Must be piloting to dock.
    let Some(seated) = pilots.iter().next() else {
        return;
    };
    let ship_entity = seated.ship;

    // Snapshot every port's world pose so we can search without holding borrows.
    // A port's structure is its propagated [`StructureRoot`], so ports mounted on
    // nested modules still resolve to the ship/station they belong to.
    let snapshots: Vec<PortSnapshot> = ports
        .iter()
        .map(|(entity, port, gt, root)| {
            let t = gt.compute_transform();
            PortSnapshot {
                entity,
                structure: root.0,
                position: t.translation.xy(),
                normal: (t.rotation * Vec3::Y).xy().normalize_or_zero(),
                affine: gt.affine(),
                docked_to: port.docked_to,
            }
        })
        .collect();

    // All of the ship's own ports (a ship can carry several docks).
    let ship_ports: Vec<&PortSnapshot> = snapshots
        .iter()
        .filter(|p| p.structure == ship_entity)
        .collect();
    if ship_ports.is_empty() {
        return;
    }

    // Already docked on any port -> release every latched port, push gently off, bail.
    let latched: Vec<(Entity, Entity)> = ship_ports
        .iter()
        .filter_map(|p| p.docked_to.map(|other| (p.entity, other)))
        .collect();
    if !latched.is_empty() {
        // Pushoff is straight back out of our docked port (opposite its outward
        // normal, which points into the partner). Use the first latched port.
        let pushoff = ship_ports
            .iter()
            .find(|p| p.docked_to.is_some())
            .map(|p| -p.normal * PUSHOFF_SPEED)
            .unwrap_or(Vec2::ZERO);
        for (ours, other) in latched {
            if let Ok((_, mut port, _, _)) = ports.get_mut(ours) {
                port.docked_to = None;
            }
            if let Ok((_, mut port, _, _)) = ports.get_mut(other) {
                port.docked_to = None;
            }
        }
        commands
            .entity(ship_entity)
            .remove::<Docked>()
            .remove::<Docking>()
            .insert(RigidBody::Dynamic);
        if let Ok((_, _, _, mut lin, _)) = ships.get_mut(ship_entity) {
            lin.0 = pushoff;
        }
        return;
    }

    // Over every pairing of one of our free ports with a free port on another
    // structure, find the nearest that's lined up: in range and facing each other.
    let mut best: Option<(&PortSnapshot, &PortSnapshot, f32)> = None;
    for &ship_port in &ship_ports {
        for cand in &snapshots {
            if cand.structure == ship_entity || cand.docked_to.is_some() {
                continue;
            }
            if !ports_dockable(
                ship_port.position,
                ship_port.normal,
                cand.position,
                cand.normal,
            ) {
                continue;
            }
            let dist = ship_port.position.distance(cand.position);
            if best.is_none_or(|(_, _, b)| dist < b) {
                best = Some((ship_port, cand, dist));
            }
        }
    }
    let Some((ship_port, cand, _)) = best else {
        return;
    };

    // Compute the docked pose: the world delta that moves the ship port onto the
    // (180°-flipped) candidate port, applied to the ship root. We don't snap there —
    // `advance_docking` eases the ship in from where it is now.
    let Ok((ship_gt, _, _, mut lin, mut ang)) = ships.get_mut(ship_entity) else {
        return;
    };
    let target_port = cand.affine * Affine3A::from_rotation_z(PI);
    let delta = target_port * ship_port.affine.inverse();
    let ship_new = GlobalTransform::from(delta * ship_gt.affine()).compute_transform();
    lin.0 = Vec2::ZERO;
    ang.0 = 0.0;

    // Latch both ports now (so the airlock doors open and these ports stop being
    // candidates during the slide), and begin easing the (now kinematic) ship in.
    let (ship_port_entity, cand_entity) = (ship_port.entity, cand.entity);
    if let Ok((_, mut port, _, _)) = ports.get_mut(ship_port_entity) {
        port.docked_to = Some(cand_entity);
    }
    if let Ok((_, mut port, _, _)) = ports.get_mut(cand_entity) {
        port.docked_to = Some(ship_port_entity);
    }
    commands.entity(ship_entity).insert((
        Docking {
            target_pos: ship_new.translation.xy(),
            target_rot: ship_new.rotation.to_euler(EulerRot::ZYX).0,
        },
        RigidBody::Kinematic,
    ));
}

/// Whether a port at `a_pos` facing `a_normal` could dock with one at `b_pos` facing
/// `b_normal`: within range and facing each other. Shared by the dock search and the
/// readiness indicator so they agree.
fn ports_dockable(a_pos: Vec2, a_normal: Vec2, b_pos: Vec2, b_normal: Vec2) -> bool {
    let to = b_pos - a_pos;
    let dist = to.length();
    if dist > DOCK_RANGE || dist < 1e-3 {
        return false;
    }
    let dir = to / dist;
    a_normal.dot(dir) > 0.0 && b_normal.dot(-dir) > 0.0
}

/// Ease a docking ship into its target pose with an exponential (ease-out) slide,
/// then latch it as [`Docked`]. The ship is kinematic during the slide, so it glides
/// in cleanly without the physics solver fighting the approaching hull.
pub fn advance_docking(
    time: Res<Time>,
    mut commands: Commands,
    mut ships: Query<(
        Entity,
        &Docking,
        &mut Position,
        &mut Rotation,
        &mut LinearVelocity,
        &mut AngularVelocity,
    )>,
) {
    let t = 1.0 - (-DOCK_RATE * time.delta_secs()).exp();
    for (entity, docking, mut pos, mut rot, mut lin, mut ang) in &mut ships {
        // Kinematic: drive the pose directly, keep velocities zeroed.
        lin.0 = Vec2::ZERO;
        ang.0 = 0.0;

        let d_ang = wrap_pi(docking.target_rot - rot.as_radians());
        if pos.0.distance(docking.target_pos) <= DOCK_POS_EPS && d_ang.abs() <= DOCK_ANG_EPS {
            pos.0 = docking.target_pos;
            *rot = Rotation::radians(docking.target_rot);
            commands.entity(entity).remove::<Docking>().insert(Docked);
            continue;
        }
        pos.0 = pos.0.lerp(docking.target_pos, t);
        *rot = Rotation::radians(rot.as_radians() + d_ang * t);
    }
}

/// Light each docking-port collar by readiness: green when it's lined up with a free
/// partner (so the pilot sees `G` will dock) or already engaged, idle orange
/// otherwise. Runs every frame in `Update`.
pub fn update_dock_indicators(
    ports: Query<(
        &DockingPort,
        &GlobalTransform,
        &StructureRoot,
        &MeshMaterial2d<ColorMaterial>,
    )>,
    mut materials: ResMut<Assets<ColorMaterial>>,
) {
    // Snapshot pose / membership / freedom / material up front (pos, normal,
    // structure, free, material).
    let snaps: Vec<(Vec2, Vec2, Entity, bool, Handle<ColorMaterial>)> = ports
        .iter()
        .map(|(port, gt, root, mat)| {
            let tf = gt.compute_transform();
            (
                tf.translation.xy(),
                (tf.rotation * Vec3::Y).xy().normalize_or_zero(),
                root.0,
                port.docked_to.is_none(),
                mat.0.clone(),
            )
        })
        .collect();

    for (idx, (pos, normal, structure, free, mat)) in snaps.iter().enumerate() {
        let ready = *free
            && snaps.iter().enumerate().any(|(j, other)| {
                j != idx
                    && other.3 // partner free
                    && other.2 != *structure // different structure
                    && ports_dockable(*pos, *normal, other.0, other.1)
            });
        let color = if !*free || ready {
            PORT_READY
        } else {
            PORT_IDLE
        };
        if let Some(mut material) = materials.get_mut(mat) {
            material.color = color;
        }
    }
}

/// Wrap an angle to `(-π, π]`.
fn wrap_pi(a: f32) -> f32 {
    (a + PI).rem_euclid(std::f32::consts::TAU) - PI
}
