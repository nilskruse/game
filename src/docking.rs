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
            MeshMaterial2d(materials.add(Color::srgb(1.0, 0.7, 0.1))),
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
/// another free port (in range, both facing each other), the ship snaps so the
/// two ports meet — coincident and facing opposite — and locks in place. Press
/// `G` again to release.
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

    // Already docked on any port -> release every latched port and bail.
    let latched: Vec<(Entity, Entity)> = ship_ports
        .iter()
        .filter_map(|p| p.docked_to.map(|other| (p.entity, other)))
        .collect();
    if !latched.is_empty() {
        for (ours, other) in latched {
            if let Ok((_, mut port, _, _)) = ports.get_mut(ours) {
                port.docked_to = None;
            }
            if let Ok((_, mut port, _, _)) = ports.get_mut(other) {
                port.docked_to = None;
            }
        }
        commands.entity(ship_entity).remove::<Docked>();
        return;
    }

    // Over every pairing of one of our free ports with a free port on another
    // structure, find the nearest that's lined up: in range and facing each other.
    const RANGE: f32 = 130.0;
    let mut best: Option<(&PortSnapshot, &PortSnapshot, f32)> = None;
    for &ship_port in &ship_ports {
        for cand in &snapshots {
            if cand.structure == ship_entity || cand.docked_to.is_some() {
                continue;
            }
            let to_cand = cand.position - ship_port.position;
            let dist = to_cand.length();
            if dist > RANGE || dist < 1e-3 {
                continue;
            }
            let dir = to_cand / dist;
            // Our port must face toward the candidate and vice-versa.
            if ship_port.normal.dot(dir) <= 0.0 || cand.normal.dot(-dir) <= 0.0 {
                continue;
            }
            if best.is_none_or(|(_, _, b)| dist < b) {
                best = Some((ship_port, cand, dist));
            }
        }
    }
    let Some((ship_port, cand, _)) = best else {
        return;
    };

    // Snap the ship so its port coincides with the candidate port, facing the
    // opposite way. We compute the world delta that moves the ship port onto the
    // (180°-flipped) candidate port, then apply that same delta to the ship root.
    let Ok((ship_gt, mut ship_pos, mut ship_rot, mut lin, mut ang)) = ships.get_mut(ship_entity)
    else {
        return;
    };
    let target_port = cand.affine * Affine3A::from_rotation_z(PI);
    let delta = target_port * ship_port.affine.inverse();
    let ship_new = GlobalTransform::from(delta * ship_gt.affine()).compute_transform();

    ship_pos.0 = ship_new.translation.xy();
    *ship_rot = Rotation::radians(ship_new.rotation.to_euler(EulerRot::ZYX).0);
    lin.0 = Vec2::ZERO;
    ang.0 = 0.0;

    // Record the latch on both ports and lock the ship.
    let (ship_port_entity, cand_entity) = (ship_port.entity, cand.entity);
    if let Ok((_, mut port, _, _)) = ports.get_mut(ship_port_entity) {
        port.docked_to = Some(cand_entity);
    }
    if let Ok((_, mut port, _, _)) = ports.get_mut(cand_entity) {
        port.docked_to = Some(ship_port_entity);
    }
    commands.entity(ship_entity).insert(Docked);
}
