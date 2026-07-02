use avian2d::prelude::*;
use bevy::{
    asset::RenderAssetUsages,
    math::EulerRot,
    mesh::{Indices, PrimitiveTopology},
    prelude::*,
};

use crate::{
    build::{BuildState, BuiltModule},
    effects::{spawn_hit_spark, Hit, Lifetime},
    faction::{Faction, InFaction},
    health::ModuleDisabled,
    player::Seated,
    ship::{bullet, bullet::Bullet, ShipBase, StructureRoot},
};

/// A PD slug knocks out an enemy projectile within this distance.
const PD_HIT_RADIUS: f32 = 26.;
/// How far to project an aim ray when testing whether a direction clears the ship —
/// well past any of the firing ship's own structure.
const AIM_REACH: f32 = 1000.;
/// Angular step (radians) when searching for the nearest clear aim direction.
const AIM_SWEEP_STEP: f32 = 0.05;

/// A turret's role. A stable id only — all per-kind data (including its [`FireArc`])
/// lives in its [`TurretDef`], looked up through the [`TurretRegistry`]; the
/// kind still selects *behavior* (which system drives the turret), like
/// `ModuleArchetype` does for modules.
#[derive(Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum TurretKind {
    /// A regular gun: auto-tracks and shoots enemy ships (see `select_target` /
    /// `fire_turret`).
    Cannon,
    /// Point-defense: twin short barrels firing a fast alternating stream of slugs at
    /// incoming enemy *projectiles* (not ships) in range, wearing them down over
    /// several hits (see `point_defense` / `update_pd_slugs`); deals no ship damage.
    PointDefense,
    /// A player-aimed gun (see `player_weapons`): while piloting, it follows the
    /// cursor and fires on held left-mouse. Can't shoot over its own ship (always
    /// line-of-sight gated). Not auto-targeted.
    PlayerCannon,
}

/// Whether a turret can fire over its own ship. Part of the [`TurretDef`] — a weapon
/// model with a different arc is a different def row (never persisted per-instance).
#[derive(Clone, Copy, PartialEq)]
pub enum FireArc {
    /// Mounted high / on a free arc: fires from any angle, even across its own hull.
    OverShip,
    /// Mounted on the hull: a shot is suppressed when the line to its target would
    /// pass through one of its own ship's modules (or hull).
    Hull,
}

impl FireArc {
    pub fn name(self) -> &'static str {
        match self {
            FireArc::OverShip => "Over-ship",
            FireArc::Hull => "Hull",
        }
    }
}

/// Which barrel mesh a turret uses (mesh construction stays code; the def picks one).
#[derive(Clone, Copy)]
pub(crate) enum TurretMesh {
    /// Round base with one long barrel ([`create_combined_mesh`]).
    Cannon,
    /// Small round base with twin short barrels ([`create_pd_mesh`]).
    TwinBarrel,
}

/// The definition of a turret weapon: all its static per-kind data in one place, the
/// same def-vs-instance split as [`ModuleDef`](crate::build::ModuleDef). An installed
/// [`Turret`] is the instance (timer + kind/arc); systems and `spawn_turret` read the
/// numbers from here via the [`TurretRegistry`].
pub(crate) struct TurretDef {
    pub kind: TurretKind,
    pub name: &'static str,
    /// Seconds between shots.
    pub fire_interval: f32,
    /// Round speed (world units/s): bullets for cannons, interception slugs for PD.
    pub projectile_speed: f32,
    /// Damage per round: armored ship damage for cannons; for PD, the durability one
    /// slug chips off an incoming projectile (cf. `bullet::BULLET_HEALTH`, so it takes
    /// several hits — not one — to kill a round). PD never damages ships.
    pub damage: f32,
    /// How fast the turret slews toward its aim (radians/s).
    pub turn_speed: f32,
    /// Engagement reach (world units). PD: how far it tracks incoming projectiles.
    /// Cannons: unlimited for now — `select_target` has no range yet (see Code check).
    pub range: f32,
    /// Distance from the pivot to the muzzle, where rounds spawn.
    pub muzzle: f32,
    /// Half the gap between twin barrels (0 = single barrel); PD slugs alternate sides.
    pub barrel_offset: f32,
    /// Whether this weapon can fire over its own ship (see [`FireArc`]).
    pub arc: FireArc,
    /// Barrel tint.
    pub tint: Color,
    pub mesh: TurretMesh,
}

/// The turret content registry: every [`TurretDef`] keyed by [`TurretKind`]. Inserted
/// at app build (see `main.rs`); query with `get(kind)`.
pub(crate) type TurretRegistry = crate::registry::Registry<TurretKind, TurretDef>;

impl Default for TurretRegistry {
    fn default() -> Self {
        Self::new(turret_defs().into_iter().map(|d| (d.kind, d)))
    }
}

/// The authored turret definitions — the single source of truth for weapon stats.
fn turret_defs() -> Vec<TurretDef> {
    vec![
        TurretDef {
            kind: TurretKind::Cannon,
            name: "Cannon",
            fire_interval: 1.0,
            projectile_speed: 2000.,
            damage: 100.,
            turn_speed: 6.0,
            range: f32::INFINITY,
            muzzle: 100.,
            barrel_offset: 0.,
            arc: FireArc::OverShip,
            tint: Color::srgb(0.4, 0.8, 1.0),
            mesh: TurretMesh::Cannon,
        },
        TurretDef {
            kind: TurretKind::PointDefense,
            name: "Point-defense",
            fire_interval: 0.04,
            projectile_speed: 2600.,
            damage: 1.0,
            turn_speed: 13.0,
            range: 320.,
            muzzle: 24.,
            barrel_offset: 7.0,
            arc: FireArc::OverShip,
            tint: Color::srgb(1.0, 0.8, 0.2),
            mesh: TurretMesh::TwinBarrel,
        },
        TurretDef {
            kind: TurretKind::PlayerCannon,
            name: "Player cannon",
            fire_interval: 0.25,
            projectile_speed: 2000.,
            damage: 100.,
            turn_speed: 16.0,
            range: f32::INFINITY,
            muzzle: 100.,
            barrel_offset: 0.,
            // Hull arc: the player gun is always LOS-gated against its own ship.
            arc: FireArc::Hull,
            tint: Color::srgb(0.4, 1.0, 0.5),
            mesh: TurretMesh::Cannon,
        },
    ]
}

/// An installed turret weapon — the *instance*: which def it is (`kind`) and its live
/// firing state. All static stats (including the fire arc) stay in the [`TurretDef`];
/// systems look them up by `kind`.
#[derive(Component)]
#[require(Transform)]
pub struct Turret {
    timer: Timer,
    kind: TurretKind,
    /// Point-defense: which barrel fires next, toggled each shot so the two barrels
    /// alternate.
    next_barrel: bool,
}

impl Turret {
    /// The installed weapon's role (for blueprint extraction).
    pub fn kind(&self) -> TurretKind {
        self.kind
    }
}

#[derive(Component)]
#[relationship(relationship_target = TargettedBy)]
pub struct Target(pub Entity);
#[derive(Component)]
#[relationship_target(relationship = Target)]
pub struct TargettedBy(Vec<Entity>);

/// Install a turret of `kind` into a turret module (`parent`). The module is a
/// bare mount; this is what puts an actual weapon on it. Returns the turret entity.
/// Everything about the weapon (mesh, fire rate, arc, tint) comes from its [`TurretDef`].
pub(crate) fn spawn_turret(
    parent: Entity,
    kind: TurretKind,
    defs: &TurretRegistry,
    mut commands: Commands,
    meshes: &mut ResMut<Assets<Mesh>>,
    materials: &mut ResMut<Assets<ColorMaterial>>,
) -> Entity {
    let def = defs.get(kind);
    let turret = Turret {
        timer: Timer::from_seconds(def.fire_interval, TimerMode::Repeating),
        kind,
        next_barrel: false,
    };
    let shape = match def.mesh {
        TurretMesh::Cannon => create_combined_mesh(),
        TurretMesh::TwinBarrel => create_pd_mesh(),
    };
    commands
        .spawn((
            turret,
            // Sits on top of the module block.
            Transform::from_xyz(0., 0., 0.6),
            Mesh2d(meshes.add(shape)),
            MeshMaterial2d(materials.add(def.tint)),
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
        // Only auto cannons lock ship targets; point-defense tracks projectiles and
        // player cannons are aimed by the pilot (see `point_defense`/`player_weapons`).
        if turret.kind != TurretKind::Cannon {
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

pub(crate) fn rotate_turret(
    time: Res<Time>,
    defs: Res<TurretRegistry>,
    mut turret_query: Query<(
        &Target,
        &Turret,
        &mut Transform,
        &GlobalTransform,
        &ChildOf,
        &StructureRoot,
    )>,
    targets: Query<&GlobalTransform>,
    modules: Query<(Entity, &GlobalTransform, &BuiltModule, &StructureRoot)>,
    hulls: Query<(&GlobalTransform, &Collider), With<ShipBase>>,
) {
    let dt = time.delta_secs();
    for (target, turret, mut transform, global, child_of, root) in turret_query.iter_mut() {
        // The target may have been destroyed; just skip this turret until it
        // re-acquires (the `Target` relationship is cleared when its entity despawns).
        let Ok(target_gt) = targets.get(target.0) else {
            continue;
        };
        let def = defs.get(turret.kind);
        let pos = global.translation().xy();
        // A hull turret clamps to the edge of its arc rather than aiming into the ship.
        let aim = aim_point(
            pos,
            target_gt.translation().xy(),
            def.arc,
            root.0,
            child_of.parent(),
            &modules,
            &hulls,
        );
        rotate_toward(&mut transform, global, aim, def.turn_speed * dt);
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
    defs: Res<TurretRegistry>,
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
        let def = defs.get(turret.kind);
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
            if dist > def.range {
                continue;
            }
            if def.arc == FireArc::Hull
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
        rotate_toward(&mut transform, global, bullet_pos, def.turn_speed * dt);
        if turret.timer.just_finished() {
            let dir = (global.rotation() * Vec3::Y).truncate().normalize_or_zero();
            if dir != Vec2::ZERO {
                // Perpendicular to the aim, to offset to whichever barrel is up next.
                let side = if turret.next_barrel { 1.0 } else { -1.0 };
                turret.next_barrel = !turret.next_barrel;
                let perp = Vec2::new(dir.y, -dir.x);
                let muzzle = pos + dir * def.muzzle + perp * (side * def.barrel_offset);
                spawn_pd_slug(
                    &mut commands,
                    muzzle,
                    dir * def.projectile_speed,
                    def.damage,
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
    /// Durability chipped off a projectile it reaches (from the firing turret's def).
    damage: f32,
    /// The firing side; the slug only kills projectiles of the *other* faction.
    faction: Faction,
}

/// Spawn a point-defense slug at `pos` moving at `velocity`, fired by `faction`.
fn spawn_pd_slug(
    commands: &mut Commands,
    pos: Vec2,
    velocity: Vec2,
    damage: f32,
    faction: Faction,
) {
    commands.spawn((
        PdSlug {
            velocity,
            damage,
            faction,
        },
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
                bullet.health -= slug.damage;
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

/// Player-aimed cannons: while piloting (and not in build mode), each [`PlayerCannon`]
/// on the piloted ship follows the cursor and fires along its barrel on held left
/// mouse, gated by its fire rate and by line-of-sight (it can't shoot over its own
/// ship). The pilot drives the ship with the keyboard and aims/fires with the mouse.
///
/// [`PlayerCannon`]: TurretKind::PlayerCannon
pub(crate) fn player_weapons(
    time: Res<Time>,
    defs: Res<TurretRegistry>,
    mouse: Res<ButtonInput<MouseButton>>,
    build_state: Res<State<BuildState>>,
    over_ui: Res<crate::ui::PointerOverUi>,
    pilots: Query<&Seated>,
    windows: Query<&Window>,
    cameras: Query<(&Camera, &GlobalTransform), With<Camera2d>>,
    mut commands: Commands,
    mut turrets: Query<(
        &mut Turret,
        &GlobalTransform,
        &mut Transform,
        &InFaction,
        &ChildOf,
        &StructureRoot,
    )>,
    disabled: Query<(), With<ModuleDisabled>>,
    modules: Query<(Entity, &GlobalTransform, &BuiltModule, &StructureRoot)>,
    hulls: Query<(&GlobalTransform, &Collider), With<ShipBase>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<ColorMaterial>>,
) {
    // Only the piloted ship's guns, and not while building (left-click builds then),
    // and not when the click landed on the UI.
    if *build_state.get() == BuildState::Building || over_ui.0 {
        return;
    }
    let Some(seated) = pilots.iter().next() else {
        return;
    };
    let Some(cursor) = cursor_world(&windows, &cameras) else {
        return;
    };
    let firing = mouse.pressed(MouseButton::Left);
    let dt = time.delta_secs();

    for (mut turret, global, mut transform, faction, child_of, root) in &mut turrets {
        if turret.kind != TurretKind::PlayerCannon || root.0 != seated.ship {
            continue;
        }
        let def = defs.get(turret.kind);
        turret.timer.tick(time.delta());
        // Shot out with its module? Can't aim or fire.
        if disabled.contains(child_of.parent()) {
            continue;
        }

        // Follow the cursor, but a player cannon can't shoot over its own ship (its
        // def's arc is `Hull`): lock to the nearest direction that clears it instead
        // of clipping into the hull.
        let pos = global.translation().xy();
        let aim = aim_point(
            pos,
            cursor,
            def.arc,
            root.0,
            child_of.parent(),
            &modules,
            &hulls,
        );
        rotate_toward(&mut transform, global, aim, def.turn_speed * dt);

        // Fire on held trigger, gated by rate and by the barrel's *current* line being
        // clear — so it fires wherever it's locked, never through the ship.
        if !firing || !turret.timer.just_finished() {
            continue;
        }
        let facing = (global.rotation() * Vec3::Y).xy().to_angle();
        if aim_blocked(pos, facing, root.0, child_of.parent(), &modules, &hulls) {
            continue;
        }
        let rotation = global.rotation();
        let muzzle = global.translation() + rotation * Vec3::new(0., def.muzzle, 0.);
        let mut spawn_location = Transform::from_translation(muzzle);
        spawn_location.rotation = rotation;
        let velocity = (rotation * Vec3::Y).xy() * def.projectile_speed;
        bullet::spawn(
            spawn_location,
            velocity,
            def.damage,
            faction.0.clone(),
            commands.reborrow(),
            &mut meshes,
            &mut materials,
        );
    }
}

/// Cursor position in world space, or `None` if it's off-window.
fn cursor_world(
    windows: &Query<&Window>,
    cameras: &Query<(&Camera, &GlobalTransform), With<Camera2d>>,
) -> Option<Vec2> {
    let window = windows.iter().next()?;
    let cursor = window.cursor_position()?;
    let (camera, cam_transform) = cameras.iter().next()?;
    camera.viewport_to_world_2d(cam_transform, cursor).ok()
}

pub(crate) fn fire_turret(
    mut commands: Commands,
    defs: Res<TurretRegistry>,
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
        let def = defs.get(turret.kind);
        turret.timer.tick(time.delta());
        if !turret.timer.just_finished() {
            continue;
        }
        // A turret sits on a module block; if that block is shot out, it can't fire.
        if disabled.contains(child_of.parent()) {
            continue;
        }
        // A hull-arc turret holds fire when its shot would pass through its own ship.
        if def.arc == FireArc::Hull {
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

        let muzzle_offset = Vec3::new(0., def.muzzle, 0.);
        let muzzle_location = global_translation + (global_rotation * muzzle_offset);

        let mut spawn_location = Transform::from_translation(muzzle_location);
        spawn_location.rotation = global_rotation;

        // Velocity in the direction the turret is facing
        let spawn_velocity = forward_direction.xy() * def.projectile_speed;

        bullet::spawn(
            spawn_location,
            spawn_velocity,
            def.damage,
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

/// Whether firing from `turret_pos` along `angle` would pass through ship `root`'s own
/// structure (a long ray cast against [`shot_blocked`]).
fn aim_blocked(
    turret_pos: Vec2,
    angle: f32,
    root: Entity,
    mount: Entity,
    modules: &Query<(Entity, &GlobalTransform, &BuiltModule, &StructureRoot)>,
    hulls: &Query<(&GlobalTransform, &Collider), With<ShipBase>>,
) -> bool {
    let to = turret_pos + Vec2::from_angle(angle) * AIM_REACH;
    shot_blocked(turret_pos, to, root, mount, modules, hulls)
}

/// The aim angle nearest `desired` (radians) from which the turret has a clear shot
/// past its own ship — `desired` itself if already clear, otherwise swept outward to
/// the near edge of the blocked arc. `None` only if every direction is blocked.
fn clear_aim_angle(
    turret_pos: Vec2,
    desired: f32,
    root: Entity,
    mount: Entity,
    modules: &Query<(Entity, &GlobalTransform, &BuiltModule, &StructureRoot)>,
    hulls: &Query<(&GlobalTransform, &Collider), With<ShipBase>>,
) -> Option<f32> {
    if !aim_blocked(turret_pos, desired, root, mount, modules, hulls) {
        return Some(desired);
    }
    let mut offset = AIM_SWEEP_STEP;
    while offset <= std::f32::consts::PI {
        if !aim_blocked(turret_pos, desired + offset, root, mount, modules, hulls) {
            return Some(desired + offset);
        }
        if !aim_blocked(turret_pos, desired - offset, root, mount, modules, hulls) {
            return Some(desired - offset);
        }
        offset += AIM_SWEEP_STEP;
    }
    None
}

/// The world point a turret should aim at: the `target` itself for an over-ship
/// turret, or — for a hull turret — a point in the nearest direction that clears its
/// own ship, so the barrel locks at the edge of its arc instead of clipping the hull.
fn aim_point(
    turret_pos: Vec2,
    target: Vec2,
    arc: FireArc,
    root: Entity,
    mount: Entity,
    modules: &Query<(Entity, &GlobalTransform, &BuiltModule, &StructureRoot)>,
    hulls: &Query<(&GlobalTransform, &Collider), With<ShipBase>>,
) -> Vec2 {
    let offset = target - turret_pos;
    if arc == FireArc::OverShip || offset.length_squared() < 1e-6 {
        return target;
    }
    let desired = offset.to_angle();
    let angle =
        clear_aim_angle(turret_pos, desired, root, mount, modules, hulls).unwrap_or(desired);
    turret_pos + Vec2::from_angle(angle) * offset.length()
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
