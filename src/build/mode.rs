use avian2d::prelude::*;
use bevy::math::Affine3A;
use bevy::prelude::*;

use crate::interaction::{Interactable, Interacted};

use super::attach::AttachPoint;
use super::kinds::{Footprint, ModuleKind};
use super::registry::{ModuleDef, ModuleRegistry};
use super::spawn::{spawn_module_sided, BuiltModule};
use super::{same_dir, UNIT};
use crate::ship::turret::{spawn_turret, Turret, TurretKind, TurretRegistry};
use crate::ship::StructureRoot;

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

/// Whether the player is at an engineering console building. A real Bevy `State`:
/// the per-frame build systems are gated on `in_state(Building)`, the transition
/// work (open/close the build UI, clear selection/overlays) lives in `OnEnter`/
/// `OnExit` systems, and other features (inventory window, walk input, player
/// weapons) hook the same state instead of polling a flag.
#[derive(States, Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum BuildState {
    #[default]
    Idle,
    Building,
}

#[derive(Resource)]
pub struct BuildMode {
    /// The structure (ship/station root) currently being edited. Set when entering
    /// build mode via an engineering console; only this structure's attach points
    /// are active. `None` when not building (cleared in [`on_exit_build`]).
    structure: Option<Entity>,
    selected: Option<ModuleKind>,
    /// Body-local outward direction for the *free-floating* ghost (when nothing snaps),
    /// rotated with `R`. Always one of +Y / +X / -Y / -X. When near attach points the module
    /// auto-orients to the nearest one (see [`plan_oriented`]), so this only sets the
    /// preview's facing in open space.
    facing: Vec2,
    /// The ghost preview entity following the cursor, if a module is selected.
    ghost: Option<Entity>,
}

impl Default for BuildMode {
    fn default() -> Self {
        Self {
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

    /// Whether a module is currently selected for placement (its ghost follows the
    /// cursor). Used to suppress the hover-inspect highlight while placing.
    pub fn is_placing(&self) -> bool {
        self.selected.is_some()
    }

    /// The footprint of the selected module, if any.
    fn footprint(&self, registry: &ModuleRegistry) -> Option<Footprint> {
        self.selected.map(|k| registry.get(k).footprint)
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
/// F opens it, F again leaves (as do B and Esc, see [`exit_build_mode`]).
fn enter_build_mode(
    event: On<Interacted>,
    consoles: Query<&BuildConsole>,
    state: Res<State<BuildState>>,
    mut next: ResMut<NextState<BuildState>>,
    mut build: ResMut<BuildMode>,
) {
    let Ok(console) = consoles.get(event.0) else {
        return;
    };
    if *state.get() == BuildState::Building && build.structure == Some(console.structure) {
        // Already building this structure — leave (cleanup runs in `on_exit_build`).
        next.set(BuildState::Idle);
    } else {
        build.structure = Some(console.structure);
        next.set(BuildState::Building);
    }
}

// Structure membership is read from the propagated `StructureRoot` component (an O(1)
// lookup) rather than walking `ChildOf` per part per frame. The propagation lands one
// frame after a part is built, so a just-placed module/attach point is invisible to
// these systems for a single frame — negligible for user-paced building.

/// Footprints of `structure`'s existing modules, in the structure's local frame —
/// axis-aligned, since modules aren't rotated relative to their structure. Each is
/// `(center, half_size)`. `inv` maps world space into that local frame. Used to
/// block attach points whose neighbouring cell is already occupied.
fn structure_footprints(
    structure: Entity,
    inv: Affine3A,
    modules: &Query<(Entity, &BuiltModule, &GlobalTransform, &StructureRoot)>,
) -> Vec<(Vec2, Vec2)> {
    modules
        .iter()
        .filter(|(.., sr)| sr.0 == structure)
        .map(|(_, m, gt, _)| (inv.transform_point3(gt.translation()).xy(), m.size / 2.))
        .collect()
}

/// The world->structure-local transform and the structure's module footprints,
/// for blocking attach points. Returns `(None, empty)` when not building.
fn structure_blocking(
    structure: Option<Entity>,
    bodies: &Query<&GlobalTransform>,
    modules: &Query<(Entity, &BuiltModule, &GlobalTransform, &StructureRoot)>,
) -> (Option<Affine3A>, Vec<(Vec2, Vec2)>) {
    let Some(structure) = structure else {
        return (None, Vec::new());
    };
    let Ok(gt) = bodies.get(structure) else {
        return (None, Vec::new());
    };
    let inv = gt.affine().inverse();
    (Some(inv), structure_footprints(structure, inv, modules))
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

/// The panel wrapping [`BuildText`]; shown only while build mode is active.
#[derive(Component)]
pub(crate) struct BuildPanel;

/// Crosshair marking the edited structure's center of mass while building (handy
/// since thruster rotation pivots around it).
#[derive(Component)]
pub(crate) struct ComMarker;

/// Spawn the (hidden) center-of-mass crosshair: a translucent disc with a yellow
/// cross, shown over the structure being edited by [`update_com_marker`].
pub(crate) fn spawn_com_marker(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<ColorMaterial>>,
) {
    let marker = commands
        .spawn((
            ComMarker,
            Transform::from_xyz(0., 0., 20.),
            Visibility::Hidden,
        ))
        .id();
    let disc = meshes.add(Circle::new(14.));
    let disc_mat = materials.add(Color::srgba(0., 0., 0., 0.55));
    commands.spawn((
        ChildOf(marker),
        Transform::from_xyz(0., 0., 0.),
        Mesh2d(disc),
        MeshMaterial2d(disc_mat),
    ));
    let cross_mat = materials.add(Color::srgb(1.0, 0.85, 0.1));
    for mesh in [Rectangle::new(34., 4.), Rectangle::new(4., 34.)] {
        commands.spawn((
            ChildOf(marker),
            Transform::from_xyz(0., 0., 0.1),
            Mesh2d(meshes.add(mesh)),
            MeshMaterial2d(cross_mat.clone()),
        ));
    }
}

/// Show the center-of-mass crosshair over the structure being edited (aligned to
/// its frame so it reads upright in build mode), hidden otherwise. The CoM shifts
/// as modules are added or removed, so it's tracked every frame.
pub(crate) fn update_com_marker(
    build: Res<BuildMode>,
    structures: Query<(&GlobalTransform, &ComputedCenterOfMass)>,
    mut marker: Query<(&mut Transform, &mut Visibility), With<ComMarker>>,
) {
    let Ok((mut transform, mut visibility)) = marker.single_mut() else {
        return;
    };
    match build.structure().and_then(|s| structures.get(s).ok()) {
        Some((gt, com)) => {
            let world = gt.transform_point(com.0.extend(0.));
            transform.translation = world.truncate().extend(20.);
            transform.rotation = gt.rotation();
            *visibility = Visibility::Visible;
        }
        None => *visibility = Visibility::Hidden,
    }
}

/// Exit build mode with `B` or `Escape` (runs only while building; the teardown
/// itself happens in [`on_exit_build`]). Entering build mode is done by interacting
/// (F) with an engineering console — see [`spawn_build_console`].
pub(crate) fn exit_build_mode(
    keyboard: Res<ButtonInput<KeyCode>>,
    mut next: ResMut<NextState<BuildState>>,
) {
    if keyboard.just_pressed(KeyCode::KeyB) || keyboard.just_pressed(KeyCode::Escape) {
        next.set(BuildState::Idle);
    }
}

/// `OnEnter(Building)`: show the build-hint panel. (The inventory window opens itself
/// on the same transition — see `inventory`.)
pub(crate) fn on_enter_build(mut panels: Query<&mut Visibility, With<BuildPanel>>) {
    for mut visibility in &mut panels {
        *visibility = Visibility::Visible;
    }
}

/// `OnExit(Building)`: tear down build mode — drop the selection and its ghost,
/// forget the edited structure (the camera and CoM marker key off it), and hide the
/// hint panel and every attach-point marker.
pub(crate) fn on_exit_build(
    mut build: ResMut<BuildMode>,
    mut commands: Commands,
    mut panels: Query<&mut Visibility, (With<BuildPanel>, Without<AttachPoint>)>,
    mut points: Query<&mut Visibility, With<AttachPoint>>,
) {
    build.structure = None;
    clear_selection(&mut build, &mut commands);
    for mut visibility in &mut panels {
        *visibility = Visibility::Hidden;
    }
    for mut visibility in &mut points {
        *visibility = Visibility::Hidden;
    }
}

pub(crate) fn select_module(
    keyboard: Res<ButtonInput<KeyCode>>,
    registry: Res<ModuleRegistry>,
    mut build: ResMut<BuildMode>,
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<ColorMaterial>>,
) {
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
    } else if keyboard.just_pressed(KeyCode::Digit8) {
        ModuleKind::Thruster
    } else {
        return;
    };

    begin_module_drag(
        kind,
        &mut build,
        &registry,
        &mut commands,
        &mut meshes,
        &mut materials,
    );
}

/// Select `kind` for placement and show its ghost (which then follows the cursor). Shared
/// by keyboard selection ([`select_module`]) and the inventory drag-into-build start (see
/// `inventory`).
pub(crate) fn begin_module_drag(
    kind: ModuleKind,
    build: &mut BuildMode,
    registry: &ModuleRegistry,
    commands: &mut Commands,
    meshes: &mut ResMut<Assets<Mesh>>,
    materials: &mut ResMut<Assets<ColorMaterial>>,
) {
    clear_selection(build, commands);
    let def = registry.get(kind);
    let ghost = spawn_ghost(commands, def, meshes, materials);
    build.selected = Some(kind);
    build.facing = Vec2::Y;
    build.ghost = Some(ghost);
}

/// Rotate the selected module's facing a quarter turn clockwise with `R`. The
/// ghost's shape doesn't change — `update_ghost` re-points it — and the module
/// attaches only to a side it faces, so this chooses which way it extends.
pub(crate) fn rotate_module(keyboard: Res<ButtonInput<KeyCode>>, mut build: ResMut<BuildMode>) {
    if build.selected.is_none() || !keyboard.just_pressed(KeyCode::KeyR) {
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
    def: &ModuleDef,
    meshes: &mut ResMut<Assets<Mesh>>,
    materials: &mut ResMut<Assets<ColorMaterial>>,
) -> Entity {
    let f = def.footprint;
    let body = Rectangle::new(f.width as f32 * UNIT, f.depth as f32 * UNIT);
    let ghost = commands
        .spawn((
            Ghost,
            Transform::from_xyz(0., 0., 5.),
            Mesh2d(meshes.add(body)),
            MeshMaterial2d(materials.add(def.color.with_alpha(0.45))),
        ))
        .id();

    let dot = meshes.add(Circle::new(8.));
    let dot_mat = materials.add(Color::srgba(0.3, 1.0, 0.4, 0.9));
    for local in ghost_attach_points(def) {
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
fn ghost_attach_points(def: &ModuleDef) -> Vec<Vec2> {
    let f = def.footprint;
    let hy = f.depth as f32 * UNIT / 2.;
    let mut pts = Vec::new();

    // Connecting side (faces the body).
    for i in 0..f.width {
        let x = ((i as f32) + 0.5 - f.width as f32 / 2.) * UNIT;
        pts.push(Vec2::new(x, -hy));
    }

    // Outward end (only walkable modules expose it).
    if def.walkable() {
        for i in 0..f.width {
            let x = ((i as f32) + 0.5 - f.width as f32 / 2.) * UNIT;
            pts.push(Vec2::new(x, hy));
        }
    }

    pts
}

pub(crate) fn update_ghost(
    build: Res<BuildMode>,
    registry: Res<ModuleRegistry>,
    windows: Query<&Window>,
    cameras: Query<(&Camera, &GlobalTransform), With<Camera2d>>,
    points: Query<(Entity, &AttachPoint, &GlobalTransform, &StructureRoot)>,
    bodies: Query<&GlobalTransform>,
    modules: Query<(Entity, &BuiltModule, &GlobalTransform, &StructureRoot)>,
    mut ghosts: Query<&mut Transform, With<Ghost>>,
) {
    let Some(footprint) = build.footprint(&registry) else {
        return;
    };
    let Some(cursor) = cursor_world(&windows, &cameras) else {
        return;
    };
    let facing = build.facing;
    // The camera is aligned to the ship in build mode, so its rotation gives the
    // facing in world space (for the free-drifting ghost when nothing snaps).
    let cam_rot = cam_rotation(&cameras);

    // Resolve where the module would attach (auto-orienting to the nearest side, only on the
    // structure being edited) and snap the ghost there: at the module's center, rotated so its
    // outward edge points out.
    let (inv, footprints) = structure_blocking(build.structure, &bodies, &modules);
    let infos: Vec<PointInfo> = points
        .iter()
        .filter(|(.., sr)| Some(sr.0) == build.structure)
        .map(|(entity, point, gt, _)| {
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

    let snapped = plan_oriented(&infos, cursor, footprint, cam_rot).and_then(|p| {
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

/// Resolve placement letting the module orient to whichever side is nearest the cursor: run
/// [`plan`] for all four facings (each with its own connecting-edge offset) and keep the
/// closest hit. So the player doesn't have to manually rotate a module to match the target
/// side — bringing it near any attach point connects it. Returns the chosen plan (whose
/// `direction` is the resolved facing), or `None` if no side resolves.
fn plan_oriented(
    infos: &[PointInfo],
    cursor: Vec2,
    footprint: Footprint,
    cam_rot: Quat,
) -> Option<Plan> {
    let mut best: Option<(f32, Plan)> = None;
    for dir in [Vec2::Y, Vec2::NEG_Y, Vec2::X, Vec2::NEG_X] {
        let query = snap_query(cursor, dir, footprint.depth, cam_rot);
        let Some(p) = plan(infos, query, footprint.width, dir) else {
            continue;
        };
        // Rank facings by how close the covered run sits to this facing's query.
        let dist = p
            .covered
            .iter()
            .filter_map(|e| infos.iter().find(|pi| pi.entity == *e))
            .map(|pi| pi.world.distance(query))
            .fold(f32::INFINITY, f32::min);
        if best.as_ref().is_none_or(|(b, _)| dist < *b) {
            best = Some((dist, p));
        }
    }
    best.map(|(_, p)| p)
}

pub(crate) fn highlight_attach_points(
    build: Res<BuildMode>,
    registry: Res<ModuleRegistry>,
    windows: Query<&Window>,
    cameras: Query<(&Camera, &GlobalTransform), With<Camera2d>>,
    mut points: Query<(
        Entity,
        &AttachPoint,
        &GlobalTransform,
        &StructureRoot,
        &MeshMaterial2d<ColorMaterial>,
        &mut Transform,
        &mut Visibility,
    )>,
    bodies: Query<&GlobalTransform>,
    modules: Query<(Entity, &BuiltModule, &GlobalTransform, &StructureRoot)>,
    mut materials: ResMut<Assets<ColorMaterial>>,
) {
    // Only the structure being edited contributes attach points. (Gated to build mode;
    // `on_exit_build` hides every point when leaving.)
    let (inv, footprints) = structure_blocking(build.structure, &bodies, &modules);
    let blocked =
        |world: Vec2, dir: Vec2| inv.is_some_and(|i| cell_blocked(world, dir, i, &footprints));
    let infos: Vec<PointInfo> = points
        .iter()
        .filter(|(_, _, _, sr, ..)| Some(sr.0) == build.structure)
        .map(|(entity, point, gt, _, _, _, _)| {
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
    let mut covered: Vec<Entity> = Vec::new();
    if let Some((f, cursor)) = build
        .footprint(&registry)
        .zip(cursor_world(&windows, &cameras))
    {
        if let Some(near) = plan_oriented(&infos, cursor, f, cam_rot) {
            covered.extend(near.covered.iter().copied());
            // A walkable module that would bridge lights up its far end too, so it's clear
            // both ends will connect.
            let walkable = build.selected.is_some_and(|k| registry.get(k).walkable());
            if walkable {
                if let Some(far) = find_bridge(&infos, &near, f, &bodies) {
                    covered.extend(far.covered);
                }
            }
        }
    }

    for (entity, point, gt, sr, material, mut transform, mut vis) in &mut points {
        // Points on other structures stay hidden — you build one structure at a time.
        if Some(sr.0) != build.structure {
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

/// Place the build-selected module at `cursor` (world space) if an attach point on the
/// edited structure resolves there. Returns whether a module was placed. The shared core
/// of the click path ([`place_module`]) and the inventory drag-drop path ([`drop_module`]);
/// neither clears the selection — the caller decides.
fn try_place_module(
    cursor: Vec2,
    build: &BuildMode,
    registry: &ModuleRegistry,
    cameras: &Query<(&Camera, &GlobalTransform), With<Camera2d>>,
    points: &mut Query<(Entity, &mut AttachPoint, &GlobalTransform, &StructureRoot)>,
    bodies: &Query<&GlobalTransform>,
    modules: &Query<(Entity, &BuiltModule, &GlobalTransform, &StructureRoot)>,
    commands: &mut Commands,
    meshes: &mut ResMut<Assets<Mesh>>,
    materials: &mut ResMut<Assets<ColorMaterial>>,
) -> bool {
    let Some(kind) = build.selected else {
        return false;
    };
    let def = registry.get(kind);
    let footprint = def.footprint;

    let (inv, footprints) = structure_blocking(build.structure, bodies, modules);
    let infos: Vec<PointInfo> = points
        .iter()
        .filter(|(.., sr)| Some(sr.0) == build.structure)
        .map(|(entity, point, gt, _)| {
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

    let cam_rot = cam_rotation(cameras);
    let Some(resolved) = plan_oriented(&infos, cursor, footprint, cam_rot) else {
        return false;
    };

    // A walkable module whose far end also reaches free, back-facing attach points connects
    // at *both* ends — a corridor bridging two modules. Detected against the pre-placement
    // point state, before spawning.
    let bridge = if def.walkable() {
        find_bridge(&infos, &resolved, footprint, bodies)
    } else {
        None
    };

    let mounted = spawn_module_sided(
        commands,
        resolved.body,
        resolved.local_center,
        resolved.direction,
        def,
        meshes,
        materials,
    );
    let module = mounted.module;

    // Rooms and docks open the covered hull doorways (disable the panels so they can be
    // re-sealed on removal); solid modules stay sealed. `occupied`/`opened` collect every
    // existing attach point and panel this module claims — on the near body and, when
    // bridging, the far one — so deconstruction restores both ends.
    let mut occupied = resolved.covered.clone();
    let mut opened = Vec::new();
    if def.opens_doorway() {
        for panel in &resolved.panels {
            commands
                .entity(*panel)
                .insert((ColliderDisabled, Visibility::Hidden));
        }
        opened.extend(resolved.panels.iter().copied());
    }
    if let Some(far) = &bridge {
        // Open the far module's doorway and claim its points; open the corridor's own outward
        // doorway and claim its points too, so the two meet walkable and the seam isn't
        // offered as buildable. (The corridor's own points despawn with it, so they aren't
        // recorded for restoration — only the far body's are.)
        for panel in &far.panels {
            commands
                .entity(*panel)
                .insert((ColliderDisabled, Visibility::Hidden));
        }
        opened.extend(far.panels.iter().copied());
        occupied.extend(far.covered.iter().copied());
        for side in &mounted.sides {
            if !same_dir(side.direction, resolved.direction) {
                continue;
            }
            for slot in &side.slots {
                commands
                    .entity(slot.panel)
                    .insert((ColliderDisabled, Visibility::Hidden));
                commands.entity(slot.entity).insert(AttachPoint {
                    occupied: true,
                    body: module,
                    local: slot.local,
                    direction: resolved.direction,
                    door_panel: slot.panel,
                });
            }
        }
    }

    let (hp, armor) = def.durability;
    commands.entity(module).insert((
        BuiltModule {
            kind,
            points: occupied.clone(),
            panels: opened,
            size: footprint.world_size(resolved.direction),
        },
        crate::health::ModuleHealth::new(hp, armor),
    ));

    // A turret module is left as a bare mount — a turret is installed separately by
    // dragging a turret item onto it (see `install_turret` / `inventory`).

    for entity in occupied {
        if let Ok((_, mut point, _, _)) = points.get_mut(entity) {
            point.occupied = true;
        }
    }
    true
}

/// If `near` is the near-side attachment of a walkable module, look for a far-side
/// attachment: free attach points facing back toward the module, one module-depth across the
/// gap, so the module connects at *both* ends (a corridor between two modules). `None` when
/// the far end is open space (ordinary one-sided placement). Reuses [`plan`] with the far
/// edge as the query and the opposite facing.
fn find_bridge(
    infos: &[PointInfo],
    near: &Plan,
    footprint: Footprint,
    bodies: &Query<&GlobalTransform>,
) -> Option<Plan> {
    let body_gt = bodies.get(near.body).ok()?;
    let depth = footprint.depth as f32 * UNIT;
    let near_edge = body_gt
        .affine()
        .transform_point3(near.local_center.extend(0.))
        .xy();
    let world_dir = body_gt
        .affine()
        .transform_vector3(near.direction.extend(0.))
        .truncate()
        .normalize_or_zero();
    let far_query = near_edge + world_dir * depth;
    // A genuine bridge spans to a *different* body.
    plan(infos, far_query, footprint.width, -near.direction).filter(|f| f.body != near.body)
}

pub(crate) fn place_module(
    mouse: Res<ButtonInput<MouseButton>>,
    over_ui: Res<crate::ui::PointerOverUi>,
    registry: Res<ModuleRegistry>,
    windows: Query<&Window>,
    cameras: Query<(&Camera, &GlobalTransform), With<Camera2d>>,
    mut build: ResMut<BuildMode>,
    mut points: Query<(Entity, &mut AttachPoint, &GlobalTransform, &StructureRoot)>,
    bodies: Query<&GlobalTransform>,
    modules: Query<(Entity, &BuiltModule, &GlobalTransform, &StructureRoot)>,
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<ColorMaterial>>,
) {
    if over_ui.0 || !mouse.just_pressed(MouseButton::Left) {
        return;
    }
    let Some(cursor) = cursor_world(&windows, &cameras) else {
        return;
    };
    // Clicking empty space keeps the selection (so you can try again); only a successful
    // placement consumes it.
    if try_place_module(
        cursor,
        &build,
        &registry,
        &cameras,
        &mut points,
        &bodies,
        &modules,
        &mut commands,
        &mut meshes,
        &mut materials,
    ) {
        clear_selection(&mut build, &mut commands);
    }
}

/// Finish an inventory drag-into-build: place the selected module at the cursor (unless the
/// release landed on the UI) and end the drag. Returns whether a module was placed, so the
/// caller can consume the dragged item. See `inventory`.
pub(crate) fn drop_module(
    over_ui: bool,
    build: &mut BuildMode,
    registry: &ModuleRegistry,
    windows: &Query<&Window>,
    cameras: &Query<(&Camera, &GlobalTransform), With<Camera2d>>,
    points: &mut Query<(Entity, &mut AttachPoint, &GlobalTransform, &StructureRoot)>,
    bodies: &Query<&GlobalTransform>,
    modules: &Query<(Entity, &BuiltModule, &GlobalTransform, &StructureRoot)>,
    commands: &mut Commands,
    meshes: &mut ResMut<Assets<Mesh>>,
    materials: &mut ResMut<Assets<ColorMaterial>>,
) -> bool {
    let placed = if over_ui {
        false
    } else if let Some(cursor) = cursor_world(windows, cameras) {
        try_place_module(
            cursor, build, registry, cameras, points, bodies, modules, commands, meshes, materials,
        )
    } else {
        false
    };
    // The drag is over whether or not it landed on a valid spot.
    clear_selection(build, commands);
    placed
}

/// Install a turret of `kind` into the (empty) turret-mount module under the cursor,
/// on the edited structure. Returns whether one was installed — for the inventory
/// drag-a-turret-into-a-mount flow (see `inventory`). A turret mount is a
/// [`ModuleKind::Turret`] module; "empty" means it has no `Turret` child yet.
pub(crate) fn install_turret(
    over_ui: bool,
    kind: TurretKind,
    build: &BuildMode,
    turret_defs: &TurretRegistry,
    windows: &Query<&Window>,
    cameras: &Query<(&Camera, &GlobalTransform), With<Camera2d>>,
    modules: &Query<(Entity, &BuiltModule, &GlobalTransform, &StructureRoot)>,
    children: &Query<&Children>,
    turrets: &Query<(), With<Turret>>,
    commands: &mut Commands,
    meshes: &mut ResMut<Assets<Mesh>>,
    materials: &mut ResMut<Assets<ColorMaterial>>,
) -> bool {
    if over_ui {
        return false;
    }
    let Some(cursor) = cursor_world(windows, cameras) else {
        return false;
    };
    // Nearest empty turret mount whose footprint is under the cursor, on the edited
    // structure (mirrors `deconstruct_module`'s pick, filtered to empty turret mounts).
    let mut hit: Option<(Entity, f32)> = None;
    for (entity, module, gt, sr) in modules {
        if module.kind != ModuleKind::Turret || Some(sr.0) != build.structure {
            continue;
        }
        let has_turret = children
            .get(entity)
            .map(|kids| kids.iter().any(|child| turrets.contains(child)))
            .unwrap_or(false);
        if has_turret {
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
    let Some((module, _)) = hit else {
        return false;
    };
    spawn_turret(
        module,
        kind,
        turret_defs,
        commands.reborrow(),
        meshes,
        materials,
    );
    true
}

/// Emitted when a built module is deconstructed, so its parts can be refunded to an
/// inventory. Targets the ship to credit (the player ship); carries plain data since the
/// module entity is despawned. `turret` is the installed weapon, if the mount was armed.
#[derive(EntityEvent)]
pub(crate) struct ModuleDeconstructed {
    #[event_target]
    pub ship: Entity,
    pub kind: ModuleKind,
    pub turret: Option<TurretKind>,
}

/// In build mode with no module selected, left-click a built module to remove it:
/// free the attach points it occupied, re-seal any doorways it opened, despawn it (and
/// anything built onto it), and refund its parts to the player ship's inventory (via
/// [`ModuleDeconstructed`]).
pub(crate) fn deconstruct_module(
    mouse: Res<ButtonInput<MouseButton>>,
    over_ui: Res<crate::ui::PointerOverUi>,
    build: Res<BuildMode>,
    windows: Query<&Window>,
    cameras: Query<(&Camera, &GlobalTransform), With<Camera2d>>,
    mut commands: Commands,
    modules: Query<(Entity, &BuiltModule, &GlobalTransform, &StructureRoot)>,
    children: Query<&Children>,
    turrets: Query<&Turret>,
    players: Query<Entity, With<crate::ship::PlayerShip>>,
    mut points: Query<&mut AttachPoint>,
) {
    if over_ui.0 || build.selected.is_some() || !mouse.just_pressed(MouseButton::Left) {
        return;
    }
    let Some(cursor) = cursor_world(&windows, &cameras) else {
        return;
    };

    // The built module whose footprint is under the cursor (nearest center wins),
    // restricted to the structure being edited.
    let mut hit: Option<(Entity, f32)> = None;
    for (entity, module, gt, sr) in &modules {
        if Some(sr.0) != build.structure {
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

    let (kind, occupied, panels) = {
        let module = modules.get(entity).unwrap().1;
        (module.kind, module.points.clone(), module.panels.clone())
    };
    // Capture an installed turret weapon (if this is an armed mount) so it's refunded too.
    let turret = children.get(entity).ok().and_then(|kids| {
        kids.iter()
            .find_map(|child| turrets.get(child).ok().map(|t| t.kind()))
    });
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

    // Refund the module (and any turret) to the player ship's inventory — what the build
    // window shows and what placement draws from (see `inventory::refund_deconstructed`).
    if let Ok(ship) = players.single() {
        commands.trigger(ModuleDeconstructed { ship, kind, turret });
    }
}

pub(crate) fn update_build_text(
    build: Res<BuildMode>,
    registry: Res<ModuleRegistry>,
    mut text: Query<&mut Text, With<BuildText>>,
) {
    // The hint only changes when build mode does (entered, module selected/placed). Skip
    // rebuilding the string (and forcing a text re-layout) on every other frame. Entering
    // build mode sets `structure`, so the first gated run refreshes the text; the panel
    // itself is shown/hidden by `on_enter_build`/`on_exit_build`.
    if !build.is_changed() {
        return;
    }
    let Ok(mut text) = text.single_mut() else {
        return;
    };
    let content = match build.selected {
        None => "BUILD MODE - select: [1] Cargo  [2] Engine  [3] Sensor  [4] Turret  [5] Dock  [6] Hallway  [7] Cockpit  [8] Thruster   |  click a module to remove, or drag a turret onto a turret mount   ([B]/[Esc] exit)".to_string(),
        Some(kind) => format!(
            "BUILD MODE - placing {} - click a highlighted attach point   ([R] rotate, [B]/[Esc] exit)",
            registry.get(kind).name
        ),
    };
    *text = Text::new(content);
}

pub(crate) fn spawn_build_ui(mut commands: Commands, theme: Res<crate::ui::Theme>) {
    commands
        .spawn((
            BuildPanel,
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(10.),
                top: Val::Px(10.),
                max_width: Val::Percent(96.),
                ..default()
            },
            GlobalZIndex(crate::ui::Z_HUD),
            // Hidden until build mode opens (see `update_build_text`).
            Visibility::Hidden,
        ))
        .with_children(|parent| {
            parent
                .spawn(crate::ui::panel(&theme))
                .with_children(|panel| {
                    panel.spawn((BuildText, crate::ui::label(&theme, "")));
                });
        });
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
