use avian2d::prelude::*;
use bevy::{
    asset::RenderAssetUsages,
    math::EulerRot,
    mesh::{Indices, PrimitiveTopology},
    prelude::*,
};

use crate::{
    build::BuiltModule,
    effects::{spawn_hit_spark, Hit, Lifetime},
    faction::{Faction, InFaction},
    health::ModuleDisabled,
    ship::{bullet, bullet::Bullet, ShipBase, StructureRoot},
};

/// How far a point-defense turret can reach incoming projectiles (world units).
const PD_RANGE: f32 = 320.;
/// Seconds between point-defense shots (one per barrel, alternating) — a fast stream.
const PD_FIRE_INTERVAL: f32 = 0.04;
/// Speed of a point-defense slug (world units / second) — faster than enemy fire.
const PD_SLUG_SPEED: f32 = 2600.;
/// A PD slug knocks out an enemy projectile within this distance.
const PD_HIT_RADIUS: f32 = 26.;
/// Durability a single PD slug strips from a projectile (cf. `bullet::BULLET_HEALTH`,
/// so it takes several hits — not one — to kill an incoming round).
const PD_SLUG_DAMAGE: f32 = 1.0;
/// Half the gap between the two PD barrels (local units); slugs alternate sides.
const PD_BARREL_OFFSET: f32 = 7.0;
/// How fast turrets slew toward their target (radians / second). Point-defense
/// tracks faster than a main cannon.
const CANNON_TURN_SPEED: f32 = 6.0;
const PD_TURN_SPEED: f32 = 13.0;

/// A turret's role. Orthogonal to its [`FireArc`].
#[derive(Clone, Copy, PartialEq)]
pub enum TurretKind {
    /// A regular gun: tracks and shoots enemy ships (see `select_target` /
    /// `fire_turret`).
    Cannon,
    /// Point-defense: twin short barrels firing a fast alternating stream of slugs at
    /// incoming enemy *projectiles* (not ships) in range, wearing them down over
    /// several hits (see `point_defense` / `update_pd_slugs`); deals no ship damage.
    PointDefense,
}

/// Whether a turret can fire over its own ship. Orthogonal to its [`TurretKind`] —
/// both cannons and point-defense turrets have an arc.
#[derive(Clone, Copy, PartialEq)]
pub enum FireArc {
    /// Mounted high / on a free arc: fires from any angle, even across its own hull.
    OverShip,
    /// Mounted on the hull: a shot is suppressed when the line to its target would
    /// pass through one of its own ship's modules (or hull).
    Hull,
}

impl TurretKind {
    pub fn name(self) -> &'static str {
        match self {
            TurretKind::Cannon => "Cannon",
            TurretKind::PointDefense => "Point-defense",
        }
    }

    /// Toggle between the kinds, for the build-mode `T` key.
    pub fn next(self) -> Self {
        match self {
            TurretKind::Cannon => TurretKind::PointDefense,
            TurretKind::PointDefense => TurretKind::Cannon,
        }
    }
}

impl FireArc {
    pub fn name(self) -> &'static str {
        match self {
            FireArc::OverShip => "Over-ship",
            FireArc::Hull => "Hull",
        }
    }

    /// Toggle between the arcs, for the build-mode `Y` key.
    pub fn next(self) -> Self {
        match self {
            FireArc::OverShip => FireArc::Hull,
            FireArc::Hull => FireArc::OverShip,
        }
    }
}

#[derive(Component)]
#[require(Transform)]
pub struct Turret {
    timer: Timer,
    _fire_rate: f32,
    velocity: f32,
    damage: f32,
    kind: TurretKind,
    arc: FireArc,
    /// Point-defense: which barrel fires next, toggled each shot so the two barrels
    /// alternate.
    next_barrel: bool,
}

impl Turret {
    pub fn new(fire_rate: f32, velocity: f32, damage: f32, kind: TurretKind, arc: FireArc) -> Self {
        let timer = Timer::from_seconds(fire_rate, TimerMode::Repeating);
        Self {
            timer,
            _fire_rate: fire_rate,
            velocity,
            damage,
            kind,
            arc,
            next_barrel: false,
        }
    }
}

#[derive(Component)]
#[relationship(relationship_target = TargettedBy)]
pub struct Target(pub Entity);
#[derive(Component)]
#[relationship_target(relationship = Target)]
pub struct TargettedBy(Vec<Entity>);

/// Install a turret of `kind`/`arc` into a turret module (`parent`). The module is a
/// bare mount; this is what puts an actual weapon on it. Returns the turret entity.
/// Cannons are tinted by arc (over-ship blue, hull white); point-defense is amber.
pub fn spawn_turret(
    parent: Entity,
    kind: TurretKind,
    arc: FireArc,
    mut commands: Commands,
    meshes: &mut ResMut<Assets<Mesh>>,
    materials: &mut ResMut<Assets<ColorMaterial>>,
) -> Entity {
    // Mesh + weapon stats by kind; point-defense has twin short barrels, a very high
    // fire rate and no ship damage.
    let (shape, turret) = match kind {
        TurretKind::Cannon => (
            create_combined_mesh(),
            Turret::new(1.0, 2000., 100., kind, arc),
        ),
        TurretKind::PointDefense => (
            create_pd_mesh(),
            Turret::new(PD_FIRE_INTERVAL, 0., 0., kind, arc),
        ),
    };
    // Tint: point-defense is amber; cannons read their arc (over-ship blue, hull white).
    let tint = match kind {
        TurretKind::PointDefense => Color::srgb(1.0, 0.8, 0.2),
        TurretKind::Cannon => match arc {
            FireArc::OverShip => Color::srgb(0.4, 0.8, 1.0),
            FireArc::Hull => Color::WHITE,
        },
    };
    commands
        .spawn((
            turret,
            // Sits on top of the module block.
            Transform::from_xyz(0., 0., 0.6),
            Mesh2d(meshes.add(shape)),
            MeshMaterial2d(materials.add(tint)),
            ChildOf(parent),
        ))
        .id()
}

pub fn select_target(
    mut commands: Commands,
    turret_query: Query<(Entity, &InFaction, &Turret), Without<Target>>,
    target_query: Query<(Entity, &InFaction)>,
) {
    for (turret_entity, turret_faction, turret) in turret_query.iter() {
        // Point-defense turrets target projectiles, not ships (see `point_defense`).
        if turret.kind == TurretKind::PointDefense {
            continue;
        }
        for (target_entity, target_faction) in target_query.iter() {
            if turret_faction != target_faction {
                commands.entity(turret_entity).insert(Target(target_entity));
                break;
            }
        }
    }
}

pub fn rotate_turret(
    time: Res<Time>,
    mut turret_query: Query<(&Target, &Turret, &mut Transform, &GlobalTransform)>,
    enemy_query: Query<&GlobalTransform>,
) {
    let dt = time.delta_secs();
    for (target, _turret, mut turret_transform, turret_global_transform) in turret_query.iter_mut()
    {
        // The target may have been destroyed; just skip this turret until it
        // re-acquires (the `Target` relationship is cleared when its entity despawns).
        let Ok(enemy_global_transform) = enemy_query.get(target.0) else {
            continue;
        };
        rotate_toward(
            &mut turret_transform,
            turret_global_transform,
            enemy_global_transform.translation().xy(),
            CANNON_TURN_SPEED * dt,
        );
    }
}

/// Slew the turret's barrels (local `+Y`) toward world `target` by at most
/// `max_step` radians, given its current local `transform` and `global` pose — so it
/// tracks naturally instead of snapping. Sets `transform.rotation` so the result
/// holds regardless of how the ship (parent) is oriented.
///
/// Works purely in 2D z-angles (everything here rotates only about z). The earlier
/// `Quat::from_rotation_arc(Y, dir)` form hit a degenerate flip when aiming roughly
/// backwards (`dir ≈ -Y`), which left the turret snapped to an arbitrary pose.
fn rotate_toward(transform: &mut Transform, global: &GlobalTransform, target: Vec2, max_step: f32) {
    let offset = target - global.translation().xy();
    if offset.length_squared() < 1e-6 {
        return;
    }
    let global_z = global.rotation().to_euler(EulerRot::ZYX).0;
    let local_z = transform.rotation.to_euler(EulerRot::ZYX).0;
    let parent_z = global_z - local_z; // the mount's world z-rotation
                                       // The barrel points along local +Y, so its world angle is `parent_z + local_z +
                                       // 90°`; solve for the local angle that aims it at the target.
    let desired_local = offset.to_angle() - std::f32::consts::FRAC_PI_2 - parent_z;
    let step = wrap_pi(desired_local - local_z).clamp(-max_step, max_step);
    transform.rotation = Quat::from_rotation_z(local_z + step);
}

/// Wrap an angle to `(-π, π]`.
fn wrap_pi(a: f32) -> f32 {
    use std::f32::consts::{PI, TAU};
    (a + PI).rem_euclid(TAU) - PI
}

/// Point-defense: each PD turret tracks the nearest incoming enemy projectile within
/// range, and on its (fast) timer shoots it down (hitscan) with a brief tracer. It
/// never damages ships.
pub(crate) fn point_defense(
    time: Res<Time>,
    mut commands: Commands,
    mut turrets: Query<(
        &mut Turret,
        &GlobalTransform,
        &mut Transform,
        &InFaction,
        &ChildOf,
        &StructureRoot,
    )>,
    bullets: Query<(&Position, &Bullet)>,
    disabled: Query<(), With<ModuleDisabled>>,
    modules: Query<(Entity, &GlobalTransform, &BuiltModule, &StructureRoot)>,
    hulls: Query<(&GlobalTransform, &Collider), With<ShipBase>>,
) {
    let dt = time.delta_secs();
    for (mut turret, global, mut transform, faction, child_of, root) in &mut turrets {
        if turret.kind != TurretKind::PointDefense {
            continue;
        }
        turret.timer.tick(time.delta());
        // Shot out with its module? Can't defend.
        if disabled.contains(child_of.parent()) {
            continue;
        }

        let pos = global.translation().xy();
        // Nearest incoming enemy projectile in range (with a clear line, if hull-arc).
        // Read the bullet's physics `Position` (current) rather than its lagged
        // `GlobalTransform`, so aim and intercepts line up with where it really is.
        let mut nearest: Option<(Vec2, f32)> = None;
        for (bullet_pos, bullet) in &bullets {
            if bullet.faction == faction.0 {
                continue; // our own side's shot
            }
            let bullet_pos = bullet_pos.0;
            let dist = pos.distance(bullet_pos);
            if dist > PD_RANGE {
                continue;
            }
            if turret.arc == FireArc::Hull
                && shot_blocked(pos, bullet_pos, root.0, child_of.parent(), &modules, &hulls)
            {
                continue;
            }
            if nearest.is_none_or(|(_, best)| dist < best) {
                nearest = Some((bullet_pos, dist));
            }
        }
        let Some((bullet_pos, _)) = nearest else {
            continue;
        };

        // Track the threat naturally, and fire a slug along the current barrel line,
        // alternating between the two barrels.
        rotate_toward(&mut transform, global, bullet_pos, PD_TURN_SPEED * dt);
        if turret.timer.just_finished() {
            let dir = (global.rotation() * Vec3::Y).truncate().normalize_or_zero();
            if dir != Vec2::ZERO {
                // Perpendicular to the aim, to offset to whichever barrel is up next.
                let side = if turret.next_barrel { 1.0 } else { -1.0 };
                turret.next_barrel = !turret.next_barrel;
                let perp = Vec2::new(dir.y, -dir.x);
                let muzzle = pos + dir * 24.0 + perp * (side * PD_BARREL_OFFSET);
                spawn_pd_slug(
                    &mut commands,
                    muzzle,
                    dir * PD_SLUG_SPEED,
                    faction.0.clone(),
                );
            }
        }
    }
}

/// A point-defense slug: a small projectile that knocks out enemy projectiles it
/// reaches and harms nothing else. Moved and tested for hits in [`update_pd_slugs`].
#[derive(Component)]
pub(crate) struct PdSlug {
    velocity: Vec2,
    /// The firing side; the slug only kills projectiles of the *other* faction.
    faction: Faction,
}

/// Spawn a point-defense slug at `pos` moving at `velocity`, fired by `faction`.
fn spawn_pd_slug(commands: &mut Commands, pos: Vec2, velocity: Vec2, faction: Faction) {
    commands.spawn((
        PdSlug { velocity, faction },
        Sprite::from_color(Color::srgb(1.0, 0.85, 0.3), Vec2::splat(5.0)),
        Transform::from_translation(pos.extend(1.0)),
        Lifetime(Timer::from_seconds(0.5, TimerMode::Once)),
    ));
}

/// Fly point-defense slugs along their velocity and knock out any enemy projectile
/// they reach (both are destroyed). Slugs that hit nothing expire via [`Lifetime`].
pub(crate) fn update_pd_slugs(
    time: Res<Time>,
    mut commands: Commands,
    mut slugs: Query<(Entity, &mut Transform, &PdSlug)>,
    mut bullets: Query<(Entity, &Position, &mut Bullet)>,
) {
    let dt = time.delta_secs();
    for (slug_entity, mut transform, slug) in &mut slugs {
        // Test against the segment swept this frame, so a fast slug can't tunnel past
        // an (also fast) incoming projectile between frames.
        let from = transform.translation.xy();
        let to = from + slug.velocity * dt;
        transform.translation = to.extend(transform.translation.z);
        for (bullet_entity, bullet_position, mut bullet) in &mut bullets {
            if bullet.faction == slug.faction {
                continue; // our own side's projectile
            }
            let bullet_pos = bullet_position.0;
            if point_segment_distance(bullet_pos, from, to) <= PD_HIT_RADIUS {
                // A slug chips the projectile (and is spent); it only dies once worn
                // down — a single hit no longer deletes it.
                bullet.health -= PD_SLUG_DAMAGE;
                if bullet.health <= 0. {
                    commands.entity(bullet_entity).try_despawn();
                }
                commands.entity(slug_entity).try_despawn();
                spawn_hit_spark(&mut commands, bullet_pos, Hit::Intercept);
                break;
            }
        }
    }
}

/// Distance from point `p` to the segment `[a, b]`.
fn point_segment_distance(p: Vec2, a: Vec2, b: Vec2) -> f32 {
    let ab = b - a;
    let len_sq = ab.length_squared();
    let t = if len_sq < 1e-6 {
        0.0
    } else {
        ((p - a).dot(ab) / len_sq).clamp(0.0, 1.0)
    };
    p.distance(a + ab * t)
}

pub(crate) fn fire_turret(
    mut commands: Commands,
    mut turret_query: Query<(
        &mut Turret,
        &GlobalTransform,
        &InFaction,
        &ChildOf,
        &Target,
        &StructureRoot,
    )>,
    disabled: Query<(), With<ModuleDisabled>>,
    transforms: Query<&GlobalTransform>,
    modules: Query<(Entity, &GlobalTransform, &BuiltModule, &StructureRoot)>,
    hulls: Query<(&GlobalTransform, &Collider), With<ShipBase>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<ColorMaterial>>,
    time: Res<Time>,
) {
    for (mut turret, turret_global_transform, faction, child_of, target, root) in
        turret_query.iter_mut()
    {
        turret.timer.tick(time.delta());
        if !turret.timer.just_finished() {
            continue;
        }
        // A turret sits on a module block; if that block is shot out, it can't fire.
        if disabled.contains(child_of.parent()) {
            continue;
        }
        // A hull-arc turret holds fire when its shot would pass through its own ship.
        if turret.arc == FireArc::Hull {
            if let Ok(target_gt) = transforms.get(target.0) {
                let from = turret_global_transform.translation().xy();
                let to = target_gt.translation().xy();
                if shot_blocked(from, to, root.0, child_of.parent(), &modules, &hulls) {
                    continue;
                }
            }
        }
        let global_translation = turret_global_transform.translation();
        let global_rotation = turret_global_transform.rotation();

        let forward_direction = global_rotation * Vec3::Y;

        let muzzle_offset = Vec3::new(0., 100., 0.);
        let muzzle_location = global_translation + (global_rotation * muzzle_offset);

        let mut spawn_location = Transform::from_translation(muzzle_location);
        spawn_location.rotation = global_rotation;

        // Velocity in the direction the turret is facing
        let spawn_velocity = forward_direction.xy() * turret.velocity;

        bullet::spawn(
            spawn_location,
            spawn_velocity,
            turret.damage,
            faction.0.clone(),
            commands.reborrow(),
            &mut meshes,
            &mut materials,
        );
    }
}

/// Whether the segment from `from` to `to` (world space) passes through any of
/// ship `root`'s own structure — its modules (except the turret's own `mount`) or
/// its hull. Used to gate a hull turret's fire so it can't shoot through its ship.
fn shot_blocked(
    from: Vec2,
    to: Vec2,
    root: Entity,
    mount: Entity,
    modules: &Query<(Entity, &GlobalTransform, &BuiltModule, &StructureRoot)>,
    hulls: &Query<(&GlobalTransform, &Collider), With<ShipBase>>,
) -> bool {
    for (entity, gt, built, sr) in modules.iter() {
        if sr.0 != root || entity == mount {
            continue;
        }
        if segment_hits_local_box(from, to, gt, built.size) {
            return true;
        }
    }
    // The central hull (the root body) blocks too; take its size from its collider.
    if let Ok((gt, collider)) = hulls.get(root) {
        if let Some(cuboid) = collider.shape().as_cuboid() {
            let size = Vec2::new(cuboid.half_extents.x, cuboid.half_extents.y) * 2.0;
            if segment_hits_local_box(from, to, gt, size) {
                return true;
            }
        }
    }
    false
}

/// Transform the world segment into `gt`'s local frame and test it against the
/// axis-aligned box of the given `size` centered at the origin.
fn segment_hits_local_box(from: Vec2, to: Vec2, gt: &GlobalTransform, size: Vec2) -> bool {
    let inv = gt.affine().inverse();
    let p0 = inv.transform_point3(from.extend(0.)).truncate();
    let p1 = inv.transform_point3(to.extend(0.)).truncate();
    segment_hits_box(p0, p1, size / 2.0)
}

/// Slab-method intersection of segment `[p0, p1]` with the axis-aligned box
/// `[-half, half]`.
fn segment_hits_box(p0: Vec2, p1: Vec2, half: Vec2) -> bool {
    let d = (p1 - p0).to_array();
    let o = p0.to_array();
    let h = half.to_array();
    let (mut tmin, mut tmax) = (0.0_f32, 1.0_f32);
    for axis in 0..2 {
        if d[axis].abs() < 1e-6 {
            // Parallel to this slab: the origin must already lie within it.
            if o[axis] < -h[axis] || o[axis] > h[axis] {
                return false;
            }
        } else {
            let inv = 1.0 / d[axis];
            let mut t1 = (-h[axis] - o[axis]) * inv;
            let mut t2 = (h[axis] - o[axis]) * inv;
            if t1 > t2 {
                std::mem::swap(&mut t1, &mut t2);
            }
            tmin = tmin.max(t1);
            tmax = tmax.min(t2);
            if tmin > tmax {
                return false;
            }
        }
    }
    true
}

/// A point-defense turret mesh: a small round base with two short barrels.
fn create_pd_mesh() -> Mesh {
    let mut vertices: Vec<[f32; 3]> = vec![];
    let mut colors: Vec<Vec4> = vec![];
    let mut indices: Vec<u32> = vec![];

    // Round base (smaller than a cannon's).
    let radius = 16.0;
    let segments = 24u32;
    let base_color = Color::srgb(0.45, 0.45, 0.5).to_srgba().to_vec4();
    let center = vertices.len() as u32;
    vertices.push([0., 0., 0.]);
    colors.push(base_color);
    for i in 0..segments {
        let angle = (i as f32 / segments as f32) * std::f32::consts::TAU;
        vertices.push([radius * angle.cos(), radius * angle.sin(), 0.]);
        colors.push(base_color);
    }
    for i in 0..segments {
        indices.push(center);
        indices.push(center + 1 + i);
        indices.push(center + 1 + ((i + 1) % segments));
    }

    // Two short barrels side by side, pointing +Y (the turret's aim direction).
    let barrel_color = Color::srgb(0.85, 0.85, 0.9).to_srgba().to_vec4();
    let half_w = 4.0;
    let length = 26.0;
    for cx in [-7.0_f32, 7.0] {
        let start = vertices.len() as u32;
        vertices.extend([
            [cx - half_w, 0., 0.],
            [cx + half_w, 0., 0.],
            [cx + half_w, length, 0.],
            [cx - half_w, length, 0.],
        ]);
        colors.extend(std::iter::repeat_n(barrel_color, 4));
        for idx in [0, 1, 2, 0, 2, 3] {
            indices.push(start + idx);
        }
    }

    let mut mesh = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::RENDER_WORLD,
    );
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, vertices);
    mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, colors);
    mesh.insert_indices(Indices::U32(indices));
    mesh
}

fn create_combined_mesh() -> Mesh {
    let mut vertices = vec![];
    let mut colors = vec![];
    let mut indices = vec![];

    // Circle vertices (offset to the right)
    let circle_radius = 20.0;
    let circle_segments = 32;
    let offset_x = 0.0;
    let mut circle_vertices = vec![];

    for i in 0..circle_segments {
        let angle = (i as f32 / circle_segments as f32) * std::f32::consts::TAU;
        circle_vertices.push([
            offset_x + circle_radius * angle.cos(),
            circle_radius * angle.sin(),
            0.0,
        ]);
    }

    let circle_center_index = circle_vertices.len() as u32;
    circle_vertices.push([offset_x, 0.0, 0.0]); // circle center

    let circle_color = Color::srgb(0., 1., 0.).to_srgba().to_vec4();
    let circle_colors: Vec<Vec4> =
        std::iter::repeat_n(circle_color, circle_vertices.len()).collect();

    let vertex_count = vertices.len() as u32;
    let mut circle_indices = vec![];
    for i in 0..circle_segments as u32 {
        circle_indices.push(circle_center_index);
        circle_indices.push(vertex_count + i);
        circle_indices.push(vertex_count + ((i + 1) % circle_segments as u32));
    }

    vertices.extend(circle_vertices);
    colors.extend(circle_colors);
    indices.extend(circle_indices);

    // Rectangle vertices (centered at origin)
    let offset_y = -25.;
    let rect_width = 5.0;
    let rect_height = 50.0;
    let rect_vertices = vec![
        [-rect_width / 2.0, -rect_height / 2.0 - offset_y, 0.0],
        [rect_width / 2.0, -rect_height / 2.0 - offset_y, 0.0],
        [rect_width / 2.0, rect_height / 2.0 - offset_y, 0.0],
        [-rect_width / 2.0, rect_height / 2.0 - offset_y, 0.0],
    ];
    let rect_color = Color::srgb(1., 0., 0.).to_srgba().to_vec4();
    let rect_colors: Vec<Vec4> = std::iter::repeat_n(rect_color, rect_vertices.len()).collect();

    let vertex_count = vertices.len() as u32;
    // Create indices for rectangle (2 triangles)
    let rect_indices = [0, 1, 2, 0, 2, 3].iter().map(|v| v + vertex_count);

    vertices.extend(rect_vertices);
    colors.extend(rect_colors);
    indices.extend(rect_indices);

    // Create mesh
    let mut mesh = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::RENDER_WORLD,
    );
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, vertices);
    mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, colors);
    mesh.insert_indices(Indices::U32(indices));

    mesh
}
