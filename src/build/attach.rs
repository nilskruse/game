use avian2d::prelude::*;
use bevy::prelude::*;

use crate::ship::GameLayer;

use super::{DOOR, HULL, PANEL, UNIT, WALL};

/// A point on a body where a module can be attached. Hidden until build mode.
///
/// A side of a size-N body has N of these in a row. A module of size M occupies
/// M consecutive points on the side it attaches to.
#[derive(Component)]
pub(crate) struct AttachPoint {
    pub occupied: bool,
    /// The body (ship hull or a built module) this point belongs to. Modules are
    /// spawned as children of their body, so this is also their transform parent.
    pub body: Entity,
    /// Position of this point on the body's edge, in body-local space.
    pub local: Vec2,
    /// Outward direction (body-local, axis-aligned unit) a module extends in.
    pub direction: Vec2,
    /// The hull panel sealing this slot's doorway. Disabled to open the doorway
    /// when a walkable module is attached.
    pub door_panel: Entity,
}

/// One attachment slot created by [`build_buildable_side`], in slot order along
/// the side. Lets callers pre-occupy a slot and mount something on it.
#[derive(Clone, Copy)]
pub struct AttachSlot {
    pub entity: Entity,
    pub local: Vec2,
    pub panel: Entity,
}

/// Build one buildable side of a size-`size` body whose half-extents are `half`:
/// a row of `size` doorway slots, each sealed by a removable panel and fronted by
/// an attachment point, with solid wall segments filling the gaps between slots.
/// Returns the slots in order along the side.
///
/// Used both for the ship hull and for walkable modules (so modules chain).
pub fn build_buildable_side(
    commands: &mut Commands,
    body: Entity,
    half: Vec2,
    size: u32,
    normal: Vec2,
    meshes: &mut ResMut<Assets<Mesh>>,
    materials: &mut ResMut<Assets<ColorMaterial>>,
) -> Vec<AttachSlot> {
    // The side runs along x when its outward normal is vertical.
    let horizontal = normal.x == 0.0;
    let l = if horizontal { half.x } else { half.y };
    // Wall/panel center sits inset by half a wall thickness; the attach point
    // sits out on the actual edge.
    let perp = (if horizontal { half.y } else { half.x }) - WALL / 2.;
    let sign = if horizontal {
        normal.y.signum()
    } else {
        normal.x.signum()
    };
    let base_perp = sign * perp;
    let edge_perp = sign * if horizontal { half.y } else { half.x };
    let layers = CollisionLayers::new(GameLayer::Walls, [GameLayer::Player, GameLayer::Default]);

    // Slot centers along the side axis, evenly spaced and centered.
    let slots: Vec<f32> = (0..size)
        .map(|i| ((i as f32) + 0.5 - size as f32 / 2.) * UNIT)
        .collect();

    // A removable panel + an attachment point per slot.
    let mut created: Vec<AttachSlot> = Vec::new();
    for &t in &slots {
        let (psize, ppos) = if horizontal {
            (Vec2::new(DOOR, WALL), Vec2::new(t, base_perp))
        } else {
            (Vec2::new(WALL, DOOR), Vec2::new(base_perp, t))
        };
        let prect = Rectangle::new(psize.x, psize.y);
        let panel = commands
            .spawn((
                ChildOf(body),
                Collider::from(prect),
                Transform::from_xyz(ppos.x, ppos.y, 0.),
                Mesh2d(meshes.add(prect)),
                MeshMaterial2d(materials.add(PANEL)),
                layers,
            ))
            .id();

        let apos = if horizontal {
            Vec2::new(t, edge_perp)
        } else {
            Vec2::new(edge_perp, t)
        };
        let entity = commands
            .spawn((
                AttachPoint {
                    occupied: false,
                    body,
                    local: apos,
                    direction: normal,
                    door_panel: panel,
                },
                ChildOf(body),
                Transform::from_xyz(apos.x, apos.y, 1.),
                Mesh2d(meshes.add(Circle::new(8.))),
                MeshMaterial2d(materials.add(Color::srgba(0.3, 1.0, 0.4, 0.8))),
                Visibility::Hidden,
            ))
            .id();
        created.push(AttachSlot {
            entity,
            local: apos,
            panel,
        });
    }

    // Solid wall segments filling everything that isn't a doorway gap.
    let mut gaps: Vec<(f32, f32)> = slots
        .iter()
        .map(|&c| (c - DOOR / 2., c + DOOR / 2.))
        .collect();
    gaps.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());
    let mut walls: Vec<(f32, f32)> = Vec::new();
    let mut cur = -l;
    for (gs, ge) in &gaps {
        if gs - cur > 0.01 {
            walls.push((cur, *gs));
        }
        cur = *ge;
    }
    if l - cur > 0.01 {
        walls.push((cur, l));
    }
    for (a, b) in walls {
        let center = (a + b) / 2.;
        let len = b - a;
        let (wsize, wpos) = if horizontal {
            (Vec2::new(len, WALL), Vec2::new(center, base_perp))
        } else {
            (Vec2::new(WALL, len), Vec2::new(base_perp, center))
        };
        let wrect = Rectangle::new(wsize.x, wsize.y);
        commands.spawn((
            ChildOf(body),
            Collider::from(wrect),
            Transform::from_xyz(wpos.x, wpos.y, 0.),
            Mesh2d(meshes.add(wrect)),
            MeshMaterial2d(materials.add(HULL)),
            layers,
        ));
    }

    created
}
