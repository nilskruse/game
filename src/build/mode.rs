use avian2d::prelude::*;
use bevy::prelude::*;

use super::attach::AttachPoint;
use super::kinds::ModuleKind;
use super::same_dir;
use super::spawn::{spawn_module_at, BuiltModule};

/// How close (world units) the cursor must be to an attachment point to snap.
const SNAP: f32 = 35.;

#[derive(Resource, Default)]
pub struct BuildMode {
    pub active: bool,
    selected: Option<ModuleKind>,
    /// The ghost preview entity following the cursor, if a module is selected.
    ghost: Option<Entity>,
}

/// The translucent preview that follows the cursor while placing.
#[derive(Component)]
pub(crate) struct Ghost;

/// On-screen build-mode hint text.
#[derive(Component)]
pub(crate) struct BuildText;

pub(crate) fn toggle_build_mode(
    keyboard: Res<ButtonInput<KeyCode>>,
    mut build: ResMut<BuildMode>,
    mut commands: Commands,
) {
    if !keyboard.just_pressed(KeyCode::KeyB) {
        return;
    }
    build.active = !build.active;
    if !build.active {
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
    } else {
        return;
    };

    clear_selection(&mut build, &mut commands);

    let extent = kind.extent();
    let ghost = commands
        .spawn((
            Ghost,
            Transform::from_xyz(0., 0., 5.),
            Mesh2d(meshes.add(Rectangle::new(extent, extent))),
            MeshMaterial2d(materials.add(kind.color().with_alpha(0.45))),
        ))
        .id();
    build.selected = Some(kind);
    build.ghost = Some(ghost);
}

pub(crate) fn update_ghost(
    build: Res<BuildMode>,
    windows: Query<&Window>,
    cameras: Query<(&Camera, &GlobalTransform), With<Camera2d>>,
    mut ghosts: Query<&mut Transform, With<Ghost>>,
) {
    if !build.active {
        return;
    }
    let Some(world) = cursor_world(&windows, &cameras) else {
        return;
    };
    for mut transform in &mut ghosts {
        transform.translation.x = world.x;
        transform.translation.y = world.y;
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
    panel: Entity,
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

/// Resolve where a size-`size` module would attach for the given cursor: pick the
/// side under the cursor, then the run of `size` consecutive free points on it
/// nearest the cursor.
fn plan(infos: &[PointInfo], cursor: Vec2, size: u32) -> Option<Plan> {
    // Nearest free point under the cursor selects the target side.
    let target = infos
        .iter()
        .filter(|p| !p.occupied && p.world.distance(cursor) <= SNAP)
        .min_by(|a, b| {
            a.world
                .distance(cursor)
                .partial_cmp(&b.world.distance(cursor))
                .unwrap()
        })?;
    let dir = target.direction;
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
        if side[i..i + n].iter().any(|p| p.occupied) {
            continue;
        }
        let sum = side[i..i + n].iter().fold(Vec2::ZERO, |acc, p| acc + p.world);
        let d = (sum / n as f32).distance(cursor);
        if best.map_or(true, |(bd, _)| d < bd) {
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
    mut materials: ResMut<Assets<ColorMaterial>>,
) {
    if !build.active {
        for (_, _, _, _, _, mut vis) in &mut points {
            *vis = Visibility::Hidden;
        }
        return;
    }

    let infos: Vec<PointInfo> = points
        .iter()
        .map(|(entity, point, gt, _, _, _)| PointInfo {
            entity,
            body: point.body,
            local: point.local,
            direction: point.direction,
            world: gt.translation().xy(),
            occupied: point.occupied,
            panel: point.door_panel,
        })
        .collect();

    let size = build.selected.map_or(1, |k| k.size_units());
    let covered: Vec<Entity> = cursor_world(&windows, &cameras)
        .and_then(|cursor| plan(&infos, cursor, size))
        .map(|p| p.covered)
        .unwrap_or_default();

    for (entity, point, _, material, mut transform, mut vis) in &mut points {
        *vis = Visibility::Visible;
        let hovered = covered.contains(&entity);
        let color = if point.occupied {
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
    let Some(cursor) = cursor_world(&windows, &cameras) else {
        return;
    };

    let infos: Vec<PointInfo> = points
        .iter()
        .map(|(entity, point, gt)| PointInfo {
            entity,
            body: point.body,
            local: point.local,
            direction: point.direction,
            world: gt.translation().xy(),
            occupied: point.occupied,
            panel: point.door_panel,
        })
        .collect();

    let Some(resolved) = plan(&infos, cursor, kind.size_units()) else {
        return;
    };

    let module = spawn_module_at(
        &mut commands,
        resolved.body,
        resolved.local_center,
        resolved.direction,
        kind,
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
        extent: kind.extent(),
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
    mut points: Query<&mut AttachPoint>,
) {
    if !build.active || build.selected.is_some() || !mouse.just_pressed(MouseButton::Left) {
        return;
    }
    let Some(cursor) = cursor_world(&windows, &cameras) else {
        return;
    };

    // The built module whose footprint is under the cursor (nearest center wins).
    let mut hit: Option<(Entity, f32)> = None;
    for (entity, module, gt) in &modules {
        let local = gt.affine().inverse().transform_point3(cursor.extend(0.));
        let h = module.extent / 2.;
        if local.x.abs() <= h && local.y.abs() <= h {
            let d = local.truncate().length();
            if hit.map_or(true, |(_, best)| d < best) {
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

pub(crate) fn update_build_text(build: Res<BuildMode>, mut text: Query<&mut Text, With<BuildText>>) {
    let Ok(mut text) = text.single_mut() else {
        return;
    };
    let content = if !build.active {
        "[B] Build mode".to_string()
    } else {
        match build.selected {
            None => "BUILD MODE — select: [1] Cargo  [2] Engine  [3] Sensor  [4] Turret  [5] Dock   |  click a module to remove   ([B] exit)".to_string(),
            Some(kind) => format!(
                "BUILD MODE — placing {} — click a highlighted attach point    ([B] exit)",
                kind.name()
            ),
        }
    };
    *text = Text::new(content);
}

pub(crate) fn spawn_build_ui(mut commands: Commands) {
    commands.spawn((
        BuildText,
        Text::new("[B] Build mode"),
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
}
