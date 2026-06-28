use avian2d::prelude::*;
use bevy::prelude::*;

use crate::ship::GameLayer;

/// One size step in world units. A body of "size N" is `N * UNIT` on each side
/// and exposes N attachment points per side.
pub const UNIT: f32 = 50.;
/// Thickness of hull / module walls.
const WALL: f32 = 5.;
/// Width of the doorway gap left in a wall for each attachment slot.
const DOOR: f32 = 40.;
/// Metallic wall color (matches the station/ship hull look).
const HULL: Color = Color::srgb(0.46, 0.49, 0.55);
/// Removable door-panel color (bronze).
const PANEL: Color = Color::srgb(0.80, 0.45, 0.20);

/// Click-to-build: toggle build mode, pick a module (it follows the cursor as a
/// ghost), and click a highlighted attachment point to attach it to the ship.
pub struct BuildPlugin;

impl Plugin for BuildPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<BuildMode>()
            .add_systems(Startup, spawn_build_ui)
            .add_systems(
                Update,
                (
                    toggle_build_mode,
                    select_module,
                    update_ghost,
                    highlight_attach_points,
                    place_module,
                    update_build_text,
                    update_airlock_doors,
                ),
            );
    }
}

/// Open each airlock door whose port is docked, close it otherwise. Disabling the
/// collider lets the player cross; re-enabling it seals the airlock in flight.
fn update_airlock_doors(
    mut commands: Commands,
    ports: Query<&crate::docking::DockingPort>,
    mut doors: Query<(Entity, &AirlockDoor, &mut Visibility, Has<ColliderDisabled>)>,
) {
    for (entity, door, mut visibility, disabled) in &mut doors {
        let docked = ports.get(door.port).map_or(false, |p| p.docked_to.is_some());
        if docked && !disabled {
            commands.entity(entity).insert(ColliderDisabled);
            *visibility = Visibility::Hidden;
        } else if !docked && disabled {
            commands.entity(entity).remove::<ColliderDisabled>();
            *visibility = Visibility::Visible;
        }
    }
}

/// How close (world units) the cursor must be to an attachment point to snap.
const SNAP: f32 = 35.;

#[derive(Resource, Default)]
pub struct BuildMode {
    pub active: bool,
    selected: Option<ModuleKind>,
    /// The ghost preview entity following the cursor, if a module is selected.
    ghost: Option<Entity>,
}

/// A point on a body where a module can be attached. Hidden until build mode.
///
/// A side of a size-N body has N of these in a row. A module of size M occupies
/// M consecutive points on the side it attaches to.
#[derive(Component)]
pub struct AttachPoint {
    pub occupied: bool,
    /// The body (ship hull or a built module) this point belongs to. Modules are
    /// spawned as children of their body, so this is also their transform parent.
    pub body: Entity,
    /// Position of this point on the body's edge, in body-local space.
    pub local: Vec2,
    /// Outward direction (body-local, axis-aligned unit) a module extends in.
    pub direction: Vec2,
    /// The hull panel sealing this slot's doorway. Despawned to open the doorway
    /// when a walkable module is attached.
    pub door_panel: Entity,
}

/// The translucent preview that follows the cursor while placing.
#[derive(Component)]
struct Ghost;

/// A module that was built onto the ship.
#[derive(Component)]
pub struct BuiltModule;

/// On-screen build-mode hint text.
#[derive(Component)]
struct BuildText;

/// The outer door of a docking airlock. It's closed (blocking the player) until
/// its `port` latches onto another, then it opens so the crew can cross.
#[derive(Component)]
struct AirlockDoor {
    port: Entity,
}

/// Airlock door color (sealed bulkhead).
const DOOR_COLOR: Color = Color::srgb(0.80, 0.30, 0.25);

#[derive(Clone, Copy, PartialEq)]
enum ModuleKind {
    Cargo,
    Engine,
    Sensor,
    Turret,
    Dock,
}

impl ModuleKind {
    /// Footprint in size units (also the number of attach points it covers on
    /// the side it connects to, and the points it exposes per free side).
    fn size_units(self) -> u32 {
        match self {
            ModuleKind::Cargo => 2,
            ModuleKind::Engine => 1,
            ModuleKind::Sensor => 1,
            ModuleKind::Turret => 1,
            ModuleKind::Dock => 1,
        }
    }

    /// Square side length in world units.
    fn extent(self) -> f32 {
        self.size_units() as f32 * UNIT
    }

    fn color(self) -> Color {
        match self {
            ModuleKind::Cargo => Color::srgb(0.55, 0.42, 0.25),
            ModuleKind::Engine => Color::srgb(0.30, 0.50, 0.70),
            ModuleKind::Sensor => Color::srgb(0.35, 0.60, 0.40),
            ModuleKind::Turret => Color::srgb(0.40, 0.42, 0.45),
            ModuleKind::Dock => Color::srgb(1.0, 0.7, 0.1),
        }
    }

    fn name(self) -> &'static str {
        match self {
            ModuleKind::Cargo => "Cargo",
            ModuleKind::Engine => "Engine",
            ModuleKind::Sensor => "Sensor",
            ModuleKind::Turret => "Turret",
            ModuleKind::Dock => "Dock",
        }
    }

    /// Walkable modules are rooms you can enter (they open the hull doorways and
    /// become buildable bodies themselves); non-walkable ones are solid blocks
    /// that leave the hull sealed.
    fn walkable(self) -> bool {
        matches!(self, ModuleKind::Cargo)
    }

    /// Whether placing this opens the covered hull doorways (so the crew can pass
    /// through): walkable rooms, and docking ports (to board a docked structure).
    fn opens_doorway(self) -> bool {
        matches!(self, ModuleKind::Cargo | ModuleKind::Dock)
    }

    /// Whether a weapon turret is mounted on top of the module's block.
    fn mounts_turret(self) -> bool {
        matches!(self, ModuleKind::Turret)
    }

    /// Whether this is a docking port (a sensor collar at the hull edge, no block).
    fn is_dock(self) -> bool {
        matches!(self, ModuleKind::Dock)
    }
}

/// Two body-local directions point the same way (axis-aligned units).
fn same_dir(a: Vec2, b: Vec2) -> bool {
    a.distance(b) < 0.01
}

/// One attachment slot created by [`build_buildable_side`], in slot order along
/// the side. Lets callers pre-occupy a slot and mount something on it.
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
    let sign = if horizontal { normal.y.signum() } else { normal.x.signum() };
    let base_perp = sign * perp;
    let edge_perp = sign * if horizontal { half.y } else { half.x };
    let layers = CollisionLayers::new(GameLayer::Walls, [GameLayer::Player]);

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
    let mut gaps: Vec<(f32, f32)> = slots.iter().map(|&c| (c - DOOR / 2., c + DOOR / 2.)).collect();
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

fn toggle_build_mode(
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

fn select_module(
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

fn update_ghost(
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

fn highlight_attach_points(
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

fn place_module(
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

    // Rooms and docks open the covered hull doorways; solid modules stay sealed.
    if kind.opens_doorway() {
        for panel in &resolved.panels {
            commands.entity(*panel).despawn();
        }
    }
    spawn_module_at(
        &mut commands,
        resolved.body,
        resolved.local_center,
        resolved.direction,
        kind,
        &mut meshes,
        &mut materials,
    );

    for entity in resolved.covered {
        if let Ok((_, mut point, _)) = points.get_mut(entity) {
            point.occupied = true;
        }
    }
    clear_selection(&mut build, &mut commands);
}

/// Spawn a module of `kind` as a child of `body`. `edge` is the body-local
/// midpoint of the covered attach points (on the hull edge); `direction` points
/// outward. Dispatches on kind: docking port (thin sensor collar at the edge),
/// walkable room, plain solid block, or a solid block with a turret on top.
fn spawn_module_at(
    commands: &mut Commands,
    body: Entity,
    edge: Vec2,
    direction: Vec2,
    kind: ModuleKind,
    meshes: &mut ResMut<Assets<Mesh>>,
    materials: &mut ResMut<Assets<ColorMaterial>>,
) {
    if kind.is_dock() {
        spawn_dock_module(commands, body, edge, direction, meshes, materials);
        return;
    }

    // Square modules center half their depth outside the edge.
    let center = edge + direction * (kind.extent() / 2.);
    if kind.walkable() {
        spawn_module_room(commands, body, center, direction, kind, meshes, materials);
        return;
    }

    let module = spawn_solid_module(commands, body, center, kind, meshes, materials);
    if kind.mounts_turret() {
        // Faction is inherited from the hull via hierarchy propagation, so the
        // turret picks up the player faction just like the old hardcoded one.
        let turret = crate::ship::turret::spawn_turret(module, commands.reborrow(), meshes, materials);
        commands
            .entity(turret)
            .insert(Transform::from_xyz(0., 0., 0.6));
    }
}

/// Occupy `slots` and mount a module of `kind` on them during construction. Lets
/// ship setup reuse the same module path as click-to-build placement.
fn mount_preplaced(
    commands: &mut Commands,
    body: Entity,
    slots: &[&AttachSlot],
    direction: Vec2,
    kind: ModuleKind,
    meshes: &mut ResMut<Assets<Mesh>>,
    materials: &mut ResMut<Assets<ColorMaterial>>,
) {
    let mut sum = Vec2::ZERO;
    for slot in slots {
        commands.entity(slot.entity).insert(AttachPoint {
            occupied: true,
            body,
            local: slot.local,
            direction,
            door_panel: slot.panel,
        });
        if kind.opens_doorway() {
            commands.entity(slot.panel).despawn();
        }
        sum += slot.local;
    }
    let edge = sum / slots.len() as f32;
    spawn_module_at(commands, body, edge, direction, kind, meshes, materials);
}

/// Pre-mount a turret module on `slot` during ship construction.
pub fn mount_preplaced_turret(
    commands: &mut Commands,
    body: Entity,
    slot: &AttachSlot,
    direction: Vec2,
    meshes: &mut ResMut<Assets<Mesh>>,
    materials: &mut ResMut<Assets<ColorMaterial>>,
) {
    mount_preplaced(commands, body, &[slot], direction, ModuleKind::Turret, meshes, materials);
}

/// Pre-mount a docking-port module spanning `slots` during construction.
pub fn mount_preplaced_dock(
    commands: &mut Commands,
    body: Entity,
    slots: &[&AttachSlot],
    direction: Vec2,
    meshes: &mut ResMut<Assets<Mesh>>,
    materials: &mut ResMut<Assets<ColorMaterial>>,
) {
    mount_preplaced(commands, body, slots, direction, ModuleKind::Dock, meshes, materials);
}

/// Attach a solid (non-walkable) module block. The hull doorway stays sealed and
/// the block exposes no further attachment points.
fn spawn_solid_module(
    commands: &mut Commands,
    body: Entity,
    center: Vec2,
    kind: ModuleKind,
    meshes: &mut ResMut<Assets<Mesh>>,
    materials: &mut ResMut<Assets<ColorMaterial>>,
) -> Entity {
    let extent = kind.extent();
    let rect = Rectangle::new(extent, extent);
    commands
        .spawn((
            BuiltModule,
            ChildOf(body),
            Transform::from_xyz(center.x, center.y, 0.4),
            Collider::from(rect),
            Mesh2d(meshes.add(rect)),
            MeshMaterial2d(materials.add(kind.color())),
        ))
        .id()
}

/// Spawn a size-1 docking airlock as a child of `body`. `edge` is the hull-edge
/// midpoint and `direction` points outward. The airlock is a small walkable room,
/// open on both the ship-facing side and the outward side (where a docking-port
/// collar sits), with solid walls on the two perpendicular sides — so the crew can
/// board straight through it once docked.
///
/// Shared by ship build/placement and station construction so both ends of a dock
/// are the same component.
pub fn spawn_dock_module(
    commands: &mut Commands,
    body: Entity,
    edge: Vec2,
    direction: Vec2,
    meshes: &mut ResMut<Assets<Mesh>>,
    materials: &mut ResMut<Assets<ColorMaterial>>,
) {
    let extent = UNIT;
    let center = edge + direction * (extent / 2.);
    let module = commands
        .spawn((
            BuiltModule,
            ChildOf(body),
            Transform::from_xyz(center.x, center.y, 0.),
            Visibility::default(),
        ))
        .id();

    // Floor.
    let floor = Rectangle::new(extent - WALL, extent - WALL);
    commands.spawn((
        ChildOf(module),
        Transform::from_xyz(0., 0., -0.5),
        Mesh2d(meshes.add(floor)),
        MeshMaterial2d(materials.add(HULL)),
    ));

    // Solid walls on the two sides perpendicular to the entry/exit axis; both the
    // ship-facing and outward sides are left open.
    let half = extent / 2.;
    let layers = CollisionLayers::new(GameLayer::Walls, [GameLayer::Player]);
    for normal in [Vec2::Y, Vec2::NEG_Y, Vec2::X, Vec2::NEG_X] {
        if same_dir(normal, direction) || same_dir(normal, -direction) {
            continue;
        }
        let horizontal = normal.x == 0.0;
        let pos = normal * (half - WALL / 2.);
        let size = if horizontal {
            Vec2::new(extent, WALL)
        } else {
            Vec2::new(WALL, extent)
        };
        let rect = Rectangle::new(size.x, size.y);
        commands.spawn((
            ChildOf(module),
            Transform::from_xyz(pos.x, pos.y, 0.),
            Collider::from(rect),
            Mesh2d(meshes.add(rect)),
            MeshMaterial2d(materials.add(HULL)),
            layers,
        ));
    }

    // Docking-port collar at the outward face. A port faces along its local +Y,
    // so rotate +Y onto `direction`.
    let angle = (-direction.x).atan2(direction.y);
    let port = crate::docking::spawn_docking_port(
        module,
        direction * (half - 1.),
        angle,
        commands.reborrow(),
        meshes,
        materials,
    );

    // Outer door across the opening: sealed (blocks the player) until the port
    // docks, then `update_airlock_doors` opens it.
    let horizontal = direction.x == 0.0;
    let door_pos = direction * (half - WALL / 2.);
    let door_size = if horizontal {
        Vec2::new(extent, WALL)
    } else {
        Vec2::new(WALL, extent)
    };
    let door_rect = Rectangle::new(door_size.x, door_size.y);
    commands.spawn((
        AirlockDoor { port },
        ChildOf(module),
        Transform::from_xyz(door_pos.x, door_pos.y, 0.1),
        Collider::from(door_rect),
        Mesh2d(meshes.add(door_rect)),
        MeshMaterial2d(materials.add(DOOR_COLOR)),
        layers,
    ));
}

/// Spawn a walkable module room as a child of `body` at body-local `center`. Its
/// side facing back toward the body (normal `-direction`) is left fully open so it
/// connects through the body's doorways; its other three sides are buildable, so
/// the room can be extended further.
fn spawn_module_room(
    commands: &mut Commands,
    body: Entity,
    center: Vec2,
    direction: Vec2,
    kind: ModuleKind,
    meshes: &mut ResMut<Assets<Mesh>>,
    materials: &mut ResMut<Assets<ColorMaterial>>,
) {
    let size = kind.size_units();
    let extent = kind.extent();
    let module = commands
        .spawn((
            BuiltModule,
            ChildOf(body),
            Transform::from_xyz(center.x, center.y, 0.),
            Visibility::default(),
        ))
        .id();

    // Floor.
    let floor = Rectangle::new(extent - WALL, extent - WALL);
    commands.spawn((
        ChildOf(module),
        Transform::from_xyz(0., 0., -0.5),
        Mesh2d(meshes.add(floor)),
        MeshMaterial2d(materials.add(kind.color())),
    ));

    // Buildable walls on every side except the open one facing the parent body.
    let half = Vec2::splat(extent / 2.);
    for normal in [Vec2::Y, Vec2::NEG_Y, Vec2::X, Vec2::NEG_X] {
        if same_dir(normal, -direction) {
            continue;
        }
        build_buildable_side(commands, module, half, size, normal, meshes, materials);
    }
}

fn update_build_text(build: Res<BuildMode>, mut text: Query<&mut Text, With<BuildText>>) {
    let Ok(mut text) = text.single_mut() else {
        return;
    };
    let content = if !build.active {
        "[B] Build mode".to_string()
    } else {
        match build.selected {
            None => "BUILD MODE — select: [1] Cargo  [2] Engine  [3] Sensor  [4] Turret  [5] Dock    ([B] exit)".to_string(),
            Some(kind) => format!(
                "BUILD MODE — placing {} — click a highlighted attach point    ([B] exit)",
                kind.name()
            ),
        }
    };
    *text = Text::new(content);
}

fn spawn_build_ui(mut commands: Commands) {
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
