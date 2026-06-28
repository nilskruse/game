use avian2d::prelude::*;
use bevy::prelude::*;

use crate::build::BuiltModule;
use crate::effects::{spawn_hit_spark, Hit};
use crate::faction::{Faction, InFaction};
use crate::health::{
    apply_armor, DamageReceived, Health, Invincible, ModuleDisabled, ModuleHealth, ShipHealth,
};
use crate::ship::{GameLayer, PlayerShip};

/// How much punishment a projectile can soak from point-defense before it's
/// destroyed. >1 PD slug damage means a single slug only chips it, not deletes it.
pub(crate) const BULLET_HEALTH: f32 = 3.0;

#[derive(Component)]
pub(crate) struct Bullet {
    pub(crate) damage: f32,
    /// The faction that fired this shot, so it passes harmlessly through its own
    /// side instead of damaging it (and so point-defense only targets enemy shots).
    pub(crate) faction: Faction,
    /// Durability against point-defense fire (see [`BULLET_HEALTH`]); not its ship
    /// damage. Each PD slug chips this; the projectile dies at 0.
    pub(crate) health: f32,
}

pub fn spawn(
    spawn_location: Transform,
    spawn_velocity: Vec2,
    damage: f32,
    faction: Faction,
    mut commands: Commands,
    meshes: &mut ResMut<Assets<Mesh>>,
    materials: &mut ResMut<Assets<ColorMaterial>>,
) {
    let shape = Rectangle::new(5., 10.);
    commands
        .spawn((
            Bullet {
                damage,
                faction,
                health: BULLET_HEALTH,
            },
            spawn_location,
            Collider::from(shape),
            Mesh2d(meshes.add(shape)),
            MeshMaterial2d(materials.add(Color::srgb(1., 1., 1.))),
            LinearVelocity(spawn_velocity),
            RigidBody::Kinematic,
            Sensor,
            // Projectiles strike structural bodies (hulls / module structural
            // colliders, the `Default` layer) only — not interior `Walls` or the
            // walking `Player` — so each shot registers once per module, not once per
            // wall segment too.
            CollisionLayers::new(GameLayer::Default, [GameLayer::Default]),
            CollisionEventsEnabled,
        ))
        .observe(on_bullet_hit);
}

fn on_bullet_hit(
    event: On<CollisionStart>,
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<ColorMaterial>>,
    invincible: Res<Invincible>,
    collisions: Collisions,
    bullets: Query<&Bullet>,
    positions: Query<&Position>,
    parents: Query<&ChildOf>,
    factions: Query<&InFaction>,
    built: Query<&BuiltModule>,
    disabled: Query<(), With<ModuleDisabled>>,
    health_targets: Query<(), With<Health>>,
    player_ships: Query<(), With<PlayerShip>>,
    mut module_health: Query<&mut ModuleHealth>,
    mut ship_health: Query<&mut ShipHealth>,
) {
    // One of the two colliders is this bullet; the other is what it struck.
    let (bullet_entity, other) = if bullets.contains(event.collider1) {
        (event.collider1, event.collider2)
    } else if bullets.contains(event.collider2) {
        (event.collider2, event.collider1)
    } else {
        return;
    };
    let Ok(bullet) = bullets.get(bullet_entity) else {
        return;
    };
    // Place the impact on the struck surface: avian's world-space contact point if
    // there is one, else the bullet's physics position (`GlobalTransform` lags a frame
    // and would place the spark short of the hit).
    let hit_pos = collisions
        .get(bullet_entity, other)
        .and_then(|pair| pair.manifolds.first())
        .and_then(|manifold| manifold.points.first())
        .map(|contact| contact.point)
        .or_else(|| positions.get(bullet_entity).map(|p| p.0).ok())
        .unwrap_or_default();

    // Walk from the struck collider up to its module (first `ModuleHealth` ancestor),
    // its faction (nearest ancestor that has one), and its structure root (top of the
    // `ChildOf` chain).
    let mut cur = other;
    let mut module = None;
    let mut faction = None;
    loop {
        if module.is_none() && module_health.contains(cur) {
            module = Some(cur);
        }
        if faction.is_none() {
            if let Ok(f) = factions.get(cur) {
                faction = Some(f.0.clone());
            }
        }
        match parents.get(cur) {
            Ok(child_of) => cur = child_of.parent(),
            Err(_) => break,
        }
    }
    let root = cur;

    // A ship hit: damage the module and the ship's total pool.
    if ship_health.contains(root) {
        // Friendly fire passes straight through (don't even consume the bullet).
        if faction.as_ref() == Some(&bullet.faction) {
            return;
        }
        // Debug invincibility: the player ship soaks the hit with no damage.
        if invincible.0 && player_ships.contains(root) {
            commands.entity(bullet_entity).try_despawn();
            return;
        }
        let Some(module_entity) = module else {
            return;
        };

        let armor = module_health.get(module_entity).map_or(0., |m| m.armor);
        let dmg = apply_armor(bullet.damage, armor);

        if let Ok(mut m) = module_health.get_mut(module_entity) {
            m.current = (m.current - dmg).max(0.);
            if m.current == 0. && !disabled.contains(module_entity) {
                disable_module(
                    &mut commands,
                    &mut meshes,
                    &mut materials,
                    module_entity,
                    &built,
                );
            }
        }
        if let Ok(mut ship) = ship_health.get_mut(root) {
            // Deplete the pool; `destroy_dead_ships` despawns the ship once it hits 0.
            // (Done there, not here, so several hits the same frame don't each try to
            // despawn the same — already-gone — ship and its parts.)
            ship.current -= dmg;
        }
        spawn_hit_spark(&mut commands, hit_pos, Hit::Ship);
        commands.entity(bullet_entity).try_despawn();
        return;
    }

    // Otherwise a plain health target (e.g. a character): the original damage path.
    if health_targets.contains(other) {
        commands.trigger(DamageReceived {
            target: other,
            damage: bullet.damage,
        });
        spawn_hit_spark(&mut commands, hit_pos, Hit::Ship);
        commands.entity(bullet_entity).try_despawn();
    }
}

/// Mark a module disabled and lay a dark overlay over its footprint so the wreck
/// reads at a glance. (Repair will clear both later.)
fn disable_module(
    commands: &mut Commands,
    meshes: &mut ResMut<Assets<Mesh>>,
    materials: &mut ResMut<Assets<ColorMaterial>>,
    module: Entity,
    built: &Query<&BuiltModule>,
) {
    commands.entity(module).try_insert(ModuleDisabled);
    if let Ok(b) = built.get(module) {
        let overlay = Rectangle::new(b.size.x, b.size.y);
        commands.spawn((
            ChildOf(module),
            Transform::from_xyz(0., 0., 0.8),
            Mesh2d(meshes.add(overlay)),
            MeshMaterial2d(materials.add(Color::srgba(0.0, 0.0, 0.0, 0.55))),
        ));
    }
}
