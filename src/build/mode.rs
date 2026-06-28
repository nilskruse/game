use avian2d::prelude::*;
use bevy::math::Affine3A;
use bevy::prelude::*;

use crate::interaction::{Interactable, Interacted};

use super::attach::AttachPoint;
use super::kinds::{Footprint, ModuleKind};
use super::spawn::{spawn_module_at, BuiltModule};
use super::{same_dir, UNIT};

/// How close (world units) the snap-test point must be to an attachment point.
const SNAP: f32 = 35.;

/// Camera rotation (the build-mode camera is aligned to the ship), or identity.
fn cam_rotation(cameras: &Query<(&Camera, &GlobalTransform), With<Camera2d>>) -> Quat {
    cameras
        .iter()
        .next()
        .map(|(_, gt)| gt.compute_transform().rotation)
        .unwrap_or_default()
}

/// The point to test for snapping. The cursor marks the ghost's center, but a
/// module connects along its inward edge — half its depth toward the body — so we
/// offset the query there. That way a module snaps as its connecting edge meets
/// the hull, not only once the cursor is pushed deep inside.
fn snap_query(cursor: Vec2, facing: Vec2, depth_units: u32, cam_rot: Quat) -> Vec2 {
    let world_facing = (cam_rot * facing.extend(0.)).truncate().normalize_or_zero();
    cursor - world_facing * (depth_units as f32 * UNIT / 2.)
}

#[derive(Resource)]
pub struct BuildMode {
    pub active: bool,
    /// The structure (ship/station root) currently being edited. Set when entering
    /// build mode via an engineering console; only this structure's attach points
    /// are active. `None` when not building.
    structure: Option<Entity>,
    selected: Option<ModuleKind>,
    /// Body-local outward direction the selected module extends in, set manually
    /// with `R`. Always one of +Y / +X / -Y / -X. The module attaches only to a
    /// side it faces.
    facing: Vec2,
    /// The ghost preview entity following the cursor, if a module is selected.
    ghost: Option<Entity>,
}

impl Default for BuildMode {
    fn default() -> Self {
        Self {
            active: false,
            structure: None,
            selected: None,
            facing: Vec2::Y,
            ghost: None,
        }
    }
}

impl BuildMode {
    /// The structure (ship/station root) currently being edited, if in build mode.
    pub fn structure(&self) -> Option<Entity> {
        self.structure
    }

    /// The footprint of the selected module, if any.
    fn footprint(&self) -> Option<Footprint> {
        self.selected.map(ModuleKind::footprint)
    }
}

/// A console on an engineering module. Interacting with it (E) enters build mode
/// scoped to the `structure` (ship/station root) it belongs to.
#[derive(Component)]
pub(crate) struct BuildConsole {
    structure: Entity,
}

/// Spawn an engineering build console as a child of structure root `root`, at
/// body-local `position`. The engineering module is every structure's initial
/// module; this console is how the crew opens build mode for that structure.
pub fn spawn_build_console(
    root: Entity,
    position: Vec2,
    commands: &mut Commands,
    meshes: &mut ResMut<Assets<Mesh>>,
    materials: &mut ResMut<Assets<ColorMaterial>>,
) -> Entity {
    let panel = Rectangle::new(26., 14.);
    commands
        .spawn((
            Interactable {
                label: "Engineering".to_string(),
                range: 45.,
            },
            BuildConsole { structure: root },
            ChildOf(root),
            Transform::from_xyz(position.x, position.y, 0.6),
            Mesh2d(meshes.add(panel)),
            MeshMaterial2d(materials.add(Color::srgb(0.95, 0.65, 0.20))),
        ))
        .observe(enter_build_mode)
        .id()
}

/// Observer: toggle build mode for the console's structure when interacted with —
/// E opens it, E again leaves (as do B and Esc, see [`exit_build_mode`]).
fn enter_build_mode(
    event: On<Interacted>,
    consoles: Query<&BuildConsole>,
    mut build: ResMut<BuildMode>,
    mut commands: Commands,
) {
    let Ok(console) = consoles.get(event.0) else {
        return;
    };
    if build.active && build.structure == Some(console.structure) {
        // Already building this structure — leave.
        build.active = false;
        build.structure = None;
        clear_selection(&mut build, &mut commands);
    } else {
        build.active = true;
        build.structure = Some(console.structure);
    }
}

/// Walk up the `ChildOf` chain to the structure root (ship hull / station root).
fn structure_root(entity: Entity, parents: &Query<&ChildOf>) -> Entity {
    let mut current = entity;
    while let Ok(child_of) = parents.get(current) {
        current = child_of.parent();
    }
    current
}

/// Footprints of `structure`'s existing modules, in the structure's local frame —
/// axis-aligned, since modules aren't rotated relative to their structure. Each is
/// `(center, half_size)`. `inv` maps world space into that local frame. Used to
/// block attach points whose neighbouring cell is already occupied.
fn structure_footprints(
    structure: Entity,
    inv: Affine3A,
    modules: &Query<(Entity, &BuiltModule, &GlobalTransform)>,
    parents: &Query<&ChildOf>,
) -> Vec<(Vec2, Vec2)> {
    modules
        .iter()
        .filter(|(e, ..)| structure_root(*e, parents) == structure)
        .map(|(_, m, gt)| (inv.transform_point3(gt.translation()).xy(), m.size / 2.))
        .collect()
}

/// The world->structure-local transform and the structure's module footprints,
/// for blocking attach points. Returns `(None, empty)` when not building.
fn structure_blocking(
    structure: Option<Entity>,
    bodies: &Query<&GlobalTransform>,
    modules: &Query<(Entity, &BuiltModule, &GlobalTransform)>,
    parents: &Query<&ChildOf>,
) -> (Option<Affine3A>, Vec<(Vec2, Vec2)>) {
    let Some(structure) = structure else {
        return (None, Vec::new());
    };
    let Ok(gt) = bodies.get(structure) else {
        return (None, Vec::new());
    };
    let inv = gt.affine().inverse();
    (
        Some(inv),
        structure_footprints(structure, inv, modules, parents),
    )
}

/// Whether the cell just outside an attach point (world position `world`, facing
/// body-local `dir`) is already occupied by one of `footprints` — i.e. a module
/// already sits where one attached here would extend. `inv` maps world space into
/// the structure's local frame (where both the point and the footprints live).
fn cell_blocked(world: Vec2, dir: Vec2, inv: Affine3A, footprints: &[(Vec2, Vec2)]) -> bool {
    let cell = inv.transform_point3(world.extend(0.)).xy() + dir * (UNIT / 2.);
    footprints
        .iter()
        .any(|(c, h)| (cell.x - c.x).abs() < h.x - 0.5 && (cell.y - c.y).abs() < h.y - 0.5)
}

/// The translucent preview that follows the cursor while placing.
#[derive(Component)]
pub(crate) struct Ghost;

/// On-screen build-mode hint text.
#[derive(Component)]
pub(crate) struct BuildText;

/// Exit build mode with `B` or `Escape`. Entering build mode is done by interacting
/// (E) with an engineering console — see [`spawn_build_console`].
pub(crate) fn exit_build_mode(
    keyboard: Res<ButtonInput<KeyCode>>,
    mut build: ResMut<BuildMode>,
    mut commands: Commands,
) {
    if !build.active {
        return;
    }
    if keyboard.just_pressed(KeyCode::KeyB) || keyboard.just_pressed(KeyCode::Escape) {
        build.active = false;
        build.structure = None;
        clear_selection(&mut build, &mut commands);
    }
}

pub(crate) fn select_module(
    keyboard: Res<ButtonInput<KeyCode>>,
    mut build: ResMut<BuildMode>,
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<ColorMaterial>>,
) {
    if !build.active {
        return;
    }
    let kind = if keyboard.just_pressed(KeyCode::Digit1) {
        ModuleKind::Cargo
    } else if keyboard.just_pressed(KeyCode::Digit2) {
        ModuleKind::Engine
    } else if keyboard.just_pressed(KeyCode::Digit3) {
        ModuleKind::Sensor
    } else if keyboard.just_pressed(KeyCode::Digit4) {
        ModuleKind::Turret
    } else if keyboard.just_pressed(KeyCode::Digit5) {
        ModuleKind::Dock
    } else if keyboard.just_pressed(KeyCode::Digit6) {
        ModuleKind::Hallway
    } else if keyboard.just_pressed(KeyCode::Digit7) {
        ModuleKind::Cockpit
    } else {
        return;
    };

    clear_selection(&mut build, &mut commands);

    let f = kind.footprint();
    let ghost = spawn_ghost(&mut commands, kind, f, &mut meshes, &mut materials);
    build.selected = Some(kind);
    build.facing = Vec2::Y;
    build.ghost = Some(ghost);
}

/// Rotate the selected module's facing a quarter turn clockwise with `R`. The
/// ghost's shape doesn't change — `update_ghost` re-points it — and the module
/// attaches only to a side it faces, so this chooses which way it extends.
pub(crate) fn rotate_module(keyboard: Res<ButtonInput<KeyCode>>, mut build: ResMut<BuildMode>) {
    if !build.active || build.selected.is_none() || !keyboard.just_pressed(KeyCode::KeyR) {
        return;
    }
    // Rotate the facing 90° clockwise: (x, y) -> (y, -x).
    build.facing = Vec2::new(build.facing.y, -build.facing.x);
}

/// Spawn the translucent ghost preview for `kind` with footprint `f`: a rectangle
/// matching the module's footprint, with dots marking the attach points it would
/// expose once placed (in the ghost's local frame; `+Y` is the outward edge).
fn spawn_ghost(
    commands: &mut Commands,
    kind: ModuleKind,
    f: Footprint,
    meshes: &mut ResMut<Assets<Mesh>>,
    materials: &mut ResMut<Assets<ColorMaterial>>,
) -> Entity {
    let body = Rectangle::new(f.width as f32 * UNIT, f.depth as f32 * UNIT);
    let ghost = commands
        .spawn((
            Ghost,
            Transform::from_xyz(0., 0., 5.),
            Mesh2d(meshes.add(body)),
            MeshMaterial2d(materials.add(kind.color().with_alpha(0.45))),
        ))
        .id();

    let dot = meshes.add(Circle::new(8.));
    let dot_mat = materials.add(Color::srgba(0.3, 1.0, 0.4, 0.9));
    for local in ghost_attach_points(kind, f) {
        commands.spawn((
            ChildOf(ghost),
            Transform::from_xyz(local.x, local.y, 0.1),
            Mesh2d(dot.clone()),
            MeshMaterial2d(dot_mat.clone()),
        ));
    }
    ghost
}

/// Local positions of a module's attach points to mark on the ghost, in its local
/// frame (`+Y` outward, `-Y` toward the body, `+X` across the width). Shows the
/// connecting side (`-Y`, every module) and, for walkable modules, the outward end
/// (`+Y`). Side faces are left unmarked even though square modules stay buildable
/// there.
fn ghost_attach_points(kind: ModuleKind, f: Footprint) -> Vec<Vec2> {
    let hy = f.depth as f32 * UNIT / 2.;
    let mut pts = Vec::new();

    // Connecting side (faces the body).
    for i in 0..f.width {
        let x = ((i as f32) + 0.5 - f.width as f32 / 2.) * UNIT;
        pts.push(Vec2::new(x, -hy));
    }

    // Outward end (only walkable modules expose it).
    if kind.walkable() {
        for i in 0..f.width {
            let x = ((i as f32) + 0.5 - f.width as f32 / 2.) * UNIT;
            pts.push(Vec2::new(x, hy));
        }
    }

    pts
}

pub(crate) fn update_ghost(
    build: Res<BuildMode>,
    windows: Query<&Window>,
    cameras: Query<(&Camera, &GlobalTransform), With<Camera2d>>,
    points: Query<(Entity, &AttachPoint, &GlobalTransform)>,
    bodies: Query<&GlobalTransform>,
    modules: Query<(Entity, &BuiltModule, &GlobalTransform)>,
    parents: Query<&ChildOf>,
    mut ghosts: Query<&mut Transform, With<Ghost>>,
) {
    if !build.active {
        return;
    }
    let Some(footprint) = build.footprint() else {
        return;
    };
    let Some(cursor) = cursor_world(&windows, &cameras) else {
        return;
    };
    let facing = build.facing;
    // The camera is aligned to the ship in build mode, so its rotation gives the
    // facing in world space (for the snap query and the free-drifting ghost).
    let cam_rot = cam_rotation(&cameras);
    let query = snap_query(cursor, facing, footprint.depth, cam_rot);

    // Resolve where the module would attach (only to a side it faces, and only on
    // the structure being edited) and snap the ghost there: at the module's center,
    // rotated so its outward edge points out.
    let (inv, footprints) = structure_blocking(build.structure, &bodies, &modules, &parents);
    let infos: Vec<PointInfo> = points
        .iter()
        .filter(|(entity, ..)| Some(structure_root(*entity, &parents)) == build.structure)
        .map(|(entity, point, gt)| {
            let world = gt.translation().xy();
            PointInfo {
                entity,
                body: point.body,
                local: point.local,
                direction: point.direction,
                world,
                occupied: point.occupied,
                blocked: inv.is_some_and(|i| cell_blocked(world, point.direction, i, &footprints)),
                panel: point.door_panel,
            }
        })
        .collect();

    let snapped = plan(&infos, query, footprint.width, facing).and_then(|p| {
        let body_gt = bodies.get(p.body).ok()?;
        let depth = footprint.depth as f32 * UNIT;
        let center_local = p.local_center + p.direction * (depth / 2.);
        let world = body_gt
            .affine()
            .transform_point3(center_local.extend(0.))
            .xy();
        let world_dir = body_gt
            .affine()
            .transform_vector3(p.direction.extend(0.))
            .truncate()
            .normalize_or_zero();
        Some((world, world_dir))
    });

    for mut transform in &mut ghosts {
        let (pos, world_dir) = match snapped {
            // Snapped to a valid attach run on a side the module faces.
            Some((world, world_dir)) => (world, world_dir),
            // No valid target — drift with the cursor, still showing the facing.
            None => {
                let world_facing = (cam_rot * facing.extend(0.)).truncate().normalize_or_zero();
                (cursor, world_facing)
            }
        };
        let angle = (-world_dir.x).atan2(world_dir.y);
        transform.translation = pos.extend(5.);
        transform.rotation = Quat::from_rotation_z(angle);
    }
}

/// Read-only snapshot of an attachment point, used for planning placement.
struct PointInfo {
    entity: Entity,
    body: Entity,
    local: Vec2,
    direction: Vec2,
    world: Vec2,
    occupied: bool,
    /// Outward cell already filled by a neighbouring module — can't build here.
    blocked: bool,
    panel: Entity,
}

impl PointInfo {
    /// A point you can attach a new module to: free and not blocked by a neighbour.
    fn available(&self) -> bool {
        !self.occupied && !self.blocked
    }
}

/// A resolved attachment: which points a module would cover and where it sits.
struct Plan {
    body: Entity,
    direction: Vec2,
    /// Body-local midpoint of the covered points (on the body edge).
    local_center: Vec2,
    covered: Vec<Entity>,
    panels: Vec<Entity>,
}

/// Resolve where a size-`size` module facing `facing` would attach for the given
/// cursor: among free points on sides the module faces, pick the nearest under the
/// cursor, then the run of `size` consecutive free points on that side.
fn plan(infos: &[PointInfo], cursor: Vec2, size: u32, facing: Vec2) -> Option<Plan> {
    // Nearest free point on a faced side selects the target side.
    let target = infos
        .iter()
        .filter(|p| {
            p.available() && same_dir(p.direction, facing) && p.world.distance(cursor) <= SNAP
        })
        .min_by(|a, b| {
            a.world
                .distance(cursor)
                .partial_cmp(&b.world.distance(cursor))
                .unwrap()
        })?;
    let dir = facing;
    let body = target.body;

    // All points on that side, ordered along the side axis.
    let tangent = Vec2::new(-dir.y, dir.x);
    let mut side: Vec<&PointInfo> = infos
        .iter()
        .filter(|p| p.body == body && same_dir(p.direction, dir))
        .collect();
    side.sort_by(|a, b| {
        a.local
            .dot(tangent)
            .partial_cmp(&b.local.dot(tangent))
            .unwrap()
    });

    let n = size as usize;
    if side.len() < n {
        return None;
    }

    // Best run of `n` consecutive free points (nearest the cursor).
    let mut best: Option<(f32, usize)> = None;
    for i in 0..=side.len() - n {
        if side[i..i + n].iter().any(|p| !p.available()) {
            continue;
        }
        let sum = side[i..i + n]
            .iter()
            .fold(Vec2::ZERO, |acc, p| acc + p.world);
        let d = (sum / n as f32).distance(cursor);
        if best.is_none_or(|(bd, _)| d < bd) {
            best = Some((d, i));
        }
    }
    let (_, i) = best?;
    let chosen = &side[i..i + n];
    let local_center = chosen.iter().fold(Vec2::ZERO, |acc, p| acc + p.local) / n as f32;

    Some(Plan {
        body,
        direction: dir,
        local_center,
        covered: chosen.iter().map(|p| p.entity).collect(),
        panels: chosen.iter().map(|p| p.panel).collect(),
    })
}

pub(crate) fn highlight_attach_points(
    build: Res<BuildMode>,
    windows: Query<&Window>,
    cameras: Query<(&Camera, &GlobalTransform), With<Camera2d>>,
    mut points: Query<(
        Entity,
        &AttachPoint,
        &GlobalTransform,
        &MeshMaterial2d<ColorMaterial>,
        &mut Transform,
        &mut Visibility,
    )>,
    bodies: Query<&GlobalTransform>,
    modules: Query<(Entity, &BuiltModule, &GlobalTransform)>,
    parents: Query<&ChildOf>,
    mut materials: ResMut<Assets<ColorMaterial>>,
) {
    if !build.active {
        for (_, _, _, _, _, mut vis) in &mut points {
            *vis = Visibility::Hidden;
        }
        return;
    }

    // Only the structure being edited contributes attach points.
    let (inv, footprints) = structure_blocking(build.structure, &bodies, &modules, &parents);
    let blocked =
        |world: Vec2, dir: Vec2| inv.is_some_and(|i| cell_blocked(world, dir, i, &footprints));
    let infos: Vec<PointInfo> = points
        .iter()
        .filter(|(entity, ..)| Some(structure_root(*entity, &parents)) == build.structure)
        .map(|(entity, point, gt, _, _, _)| {
            let world = gt.translation().xy();
            PointInfo {
                entity,
                body: point.body,
                local: point.local,
                direction: point.direction,
                world,
                occupied: point.occupied,
                blocked: blocked(world, point.direction),
                panel: point.door_panel,
            }
        })
        .collect();

    let cam_rot = cam_rotation(&cameras);
    let covered: Vec<Entity> = build
        .footprint()
        .zip(cursor_world(&windows, &cameras))
        .and_then(|(f, cursor)| {
            let query = snap_query(cursor, build.facing, f.depth, cam_rot);
            plan(&infos, query, f.width, build.facing)
        })
        .map(|p| p.covered)
        .unwrap_or_default();

    for (entity, point, gt, material, mut transform, mut vis) in &mut points {
        // Points on other structures stay hidden — you build one structure at a time.
        if Some(structure_root(entity, &parents)) != build.structure {
            *vis = Visibility::Hidden;
            continue;
        }
        *vis = Visibility::Visible;
        let hovered = covered.contains(&entity);
        // Occupied or blocked by a neighbour -> greyed out (can't build here).
        let unavailable = point.occupied || blocked(gt.translation().xy(), point.direction);
        let color = if unavailable {
            Color::srgba(0.5, 0.5, 0.5, 0.6)
        } else if hovered {
            Color::srgba(1.0, 0.95, 0.3, 0.95)
        } else {
            Color::srgba(0.3, 1.0, 0.4, 0.8)
        };
        if let Some(mut mat) = materials.get_mut(&material.0) {
            mat.color = color;
        }
        transform.scale = Vec3::splat(if hovered { 1.5 } else { 1.0 });
    }
}

pub(crate) fn place_module(
    mouse: Res<ButtonInput<MouseButton>>,
    windows: Query<&Window>,
    cameras: Query<(&Camera, &GlobalTransform), With<Camera2d>>,
    mut build: ResMut<BuildMode>,
    mut points: Query<(Entity, &mut AttachPoint, &GlobalTransform)>,
    bodies: Query<&GlobalTransform>,
    modules: Query<(Entity, &BuiltModule, &GlobalTransform)>,
    parents: Query<&ChildOf>,
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<ColorMaterial>>,
) {
    if !build.active || !mouse.just_pressed(MouseButton::Left) {
        return;
    }
    let Some(kind) = build.selected else {
        return;
    };
    let Some(footprint) = build.footprint() else {
        return;
    };
    let Some(cursor) = cursor_world(&windows, &cameras) else {
        return;
    };

    let (inv, footprints) = structure_blocking(build.structure, &bodies, &modules, &parents);
    let infos: Vec<PointInfo> = points
        .iter()
        .filter(|(entity, ..)| Some(structure_root(*entity, &parents)) == build.structure)
        .map(|(entity, point, gt)| {
            let world = gt.translation().xy();
            PointInfo {
                entity,
                body: point.body,
                local: point.local,
                direction: point.direction,
                world,
                occupied: point.occupied,
                blocked: inv.is_some_and(|i| cell_blocked(world, point.direction, i, &footprints)),
                panel: point.door_panel,
            }
        })
        .collect();

    let cam_rot = cam_rotation(&cameras);
    let query = snap_query(cursor, build.facing, footprint.depth, cam_rot);
    let Some(resolved) = plan(&infos, query, footprint.width, build.facing) else {
        return;
    };

    let module = spawn_module_at(
        &mut commands,
        resolved.body,
        resolved.local_center,
        resolved.direction,
        kind,
        footprint,
        &mut meshes,
        &mut materials,
    );

    // Rooms and docks open the covered hull doorways (disable the panels so they
    // can be re-sealed on removal); solid modules stay sealed.
    let opened = if kind.opens_doorway() {
        for panel in &resolved.panels {
            commands
                .entity(*panel)
                .insert((ColliderDisabled, Visibility::Hidden));
        }
        resolved.panels.clone()
    } else {
        Vec::new()
    };

    commands.entity(module).insert(BuiltModule {
        points: resolved.covered.clone(),
        panels: opened,
        size: footprint.world_size(resolved.direction),
    });

    for entity in resolved.covered {
        if let Ok((_, mut point, _)) = points.get_mut(entity) {
            point.occupied = true;
        }
    }
    clear_selection(&mut build, &mut commands);
}

/// In build mode with no module selected, left-click a built module to remove it:
/// free the attach points it occupied, re-seal any doorways it opened, and despawn
/// it (and anything built onto it).
pub(crate) fn deconstruct_module(
    mouse: Res<ButtonInput<MouseButton>>,
    build: Res<BuildMode>,
    windows: Query<&Window>,
    cameras: Query<(&Camera, &GlobalTransform), With<Camera2d>>,
    mut commands: Commands,
    modules: Query<(Entity, &BuiltModule, &GlobalTransform)>,
    parents: Query<&ChildOf>,
    mut points: Query<&mut AttachPoint>,
) {
    if !build.active || build.selected.is_some() || !mouse.just_pressed(MouseButton::Left) {
        return;
    }
    let Some(cursor) = cursor_world(&windows, &cameras) else {
        return;
    };

    // The built module whose footprint is under the cursor (nearest center wins),
    // restricted to the structure being edited.
    let mut hit: Option<(Entity, f32)> = None;
    for (entity, module, gt) in &modules {
        if Some(structure_root(entity, &parents)) != build.structure {
            continue;
        }
        let local = gt.affine().inverse().transform_point3(cursor.extend(0.));
        let h = module.size / 2.;
        if local.x.abs() <= h.x && local.y.abs() <= h.y {
            let d = local.truncate().length();
            if hit.is_none_or(|(_, best)| d < best) {
                hit = Some((entity, d));
            }
        }
    }
    let Some((entity, _)) = hit else {
        return;
    };

    let (occupied, panels) = {
        let module = modules.get(entity).unwrap().1;
        (module.points.clone(), module.panels.clone())
    };
    for point in occupied {
        if let Ok(mut point) = points.get_mut(point) {
            point.occupied = false;
        }
    }
    for panel in panels {
        commands
            .entity(panel)
            .remove::<ColliderDisabled>()
            .insert(Visibility::Visible);
    }
    commands.entity(entity).despawn();
}

pub(crate) fn update_build_text(
    build: Res<BuildMode>,
    mut text: Query<&mut Text, With<BuildText>>,
) {
    let Ok(mut text) = text.single_mut() else {
        return;
    };
    let content = if !build.active {
        String::new()
    } else {
        match build.selected {
            None => "BUILD MODE — select: [1] Cargo  [2] Engine  [3] Sensor  [4] Turret  [5] Dock  [6] Hallway  [7] Cockpit   |  click a module to remove   ([B]/[Esc] exit)".to_string(),
            Some(kind) => format!(
                "BUILD MODE — placing {} — click a highlighted attach point   ([R] rotate, [B]/[Esc] exit)",
                kind.name()
            ),
        }
    };
    *text = Text::new(content);
}

pub(crate) fn spawn_build_ui(mut commands: Commands) {
    commands.spawn((
        BuildText,
        Text::new(""),
        TextColor(Color::srgb(0.8, 0.9, 1.0)),
        Node {
            position_type: PositionType::Absolute,
            left: Val::Px(10.),
            top: Val::Px(10.),
            ..default()
        },
    ));
}

/// Cursor position in world space, or `None` if the cursor is off-window.
fn cursor_world(
    windows: &Query<&Window>,
    cameras: &Query<(&Camera, &GlobalTransform), With<Camera2d>>,
) -> Option<Vec2> {
    let window = windows.iter().next()?;
    let cursor = window.cursor_position()?;
    let (camera, cam_tf) = cameras.iter().next()?;
    camera.viewport_to_world_2d(cam_tf, cursor).ok()
}

fn clear_selection(build: &mut BuildMode, commands: &mut Commands) {
    if let Some(ghost) = build.ghost.take() {
        commands.entity(ghost).despawn();
    }
    build.selected = None;
    build.facing = Vec2::Y;
}
