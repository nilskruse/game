use bevy::prelude::*;

#[derive(Component, Deref, DerefMut)]
// #[require(Observer::new(on_damage_received))]
pub struct Health {
    pub current: f32,
}

/// Per-module durability. Every ship module (including the hull / engineering
/// module) carries one. `armor` is a flat damage reduction (see [`apply_armor`]);
/// `current` falls as the module is hit and the module is disabled at 0. The
/// module's `max` also feeds its ship's total-health capacity (see [`ShipHealth`]).
#[derive(Component)]
pub struct ModuleHealth {
    pub current: f32,
    pub max: f32,
    pub armor: f32,
}

impl ModuleHealth {
    pub fn new(max: f32, armor: f32) -> Self {
        Self {
            current: max,
            max,
            armor,
        }
    }
}

/// A ship's overall integrity pool, on the ship root. `max` (capacity) tracks the
/// sum of its modules' max health (so building/removing modules grows/shrinks it,
/// kept current by [`sync_ship_health`]); `current` is damaged independently as the
/// ship is hit, and the ship is destroyed when it reaches 0. So total health is tied
/// to the modules' capacity but takes damage as its own pool, not a live re-sum.
#[derive(Component, Default)]
pub struct ShipHealth {
    pub current: f32,
    pub max: f32,
}

/// Marker on a module whose health has hit 0. For now it just records the state (and
/// is shown by a dark overlay); a repair mechanic will clear it later.
#[derive(Component)]
pub struct ModuleDisabled;

/// Debug toggle: when set, the player ship takes no damage. Flipped with `F1` (see
/// [`toggle_invincible`]); checked in the bullet hit path.
#[derive(Resource, Default)]
pub struct Invincible(pub bool);

/// Toggle player-ship [`Invincible`] with `F1`. Runs in `Update` so the press edge
/// isn't missed.
pub fn toggle_invincible(keyboard: Res<ButtonInput<KeyCode>>, mut invincible: ResMut<Invincible>) {
    if keyboard.just_pressed(KeyCode::F1) {
        invincible.0 = !invincible.0;
        info!("debug: player ship invincible = {}", invincible.0);
    }
}

/// Smallest damage any hit deals after armor, so heavy armor never makes a module
/// fully immune to a small shot.
pub const ARMOR_CHIP: f32 = 1.0;

/// Flat armor reduction: a hit of `raw` against `armor` deals `raw - armor`, but
/// never less than [`ARMOR_CHIP`] (nor more than `raw` for tiny hits).
pub fn apply_armor(raw: f32, armor: f32) -> f32 {
    (raw - armor).max(ARMOR_CHIP).min(raw)
}

/// Keep each ship's [`ShipHealth`] capacity in step with the sum of its modules'
/// max health. When the capacity changes (a module was built or removed) the current
/// pool shifts by the same delta; combat damage (which doesn't change `max`) is left
/// alone. Removing a *damaged* module subtracts its full `max` — close enough until
/// repair/teardown accounting exists.
pub fn sync_ship_health(
    mut ships: Query<(Entity, &mut ShipHealth)>,
    modules: Query<(Entity, &ModuleHealth)>,
    parents: Query<&ChildOf>,
) {
    for (root, mut ship) in &mut ships {
        let new_max: f32 = modules
            .iter()
            .filter(|(e, _)| root_of(*e, &parents) == root)
            .map(|(_, m)| m.max)
            .sum();
        let delta = new_max - ship.max;
        if delta.abs() > f32::EPSILON {
            ship.max = new_max;
            ship.current = (ship.current + delta).clamp(0., new_max);
        }
    }
}

/// Destroy any ship whose total integrity pool has run out. Done in its own system
/// (not inline where damage is applied) so that several hits landing the same frame
/// don't each try to despawn the same ship and its now-gone parts.
pub(crate) fn destroy_dead_ships(mut commands: Commands, ships: Query<(Entity, &ShipHealth)>) {
    for (root, health) in &ships {
        if health.current <= 0. {
            commands.entity(root).try_despawn();
        }
    }
}

/// Walk up the `ChildOf` chain to a part's structure root.
fn root_of(mut entity: Entity, parents: &Query<&ChildOf>) -> Entity {
    while let Ok(child_of) = parents.get(entity) {
        entity = child_of.parent();
    }
    entity
}

#[derive(EntityEvent)]
pub struct HealthExpired(Entity);

#[derive(EntityEvent)]
pub struct DamageReceived {
    #[event_target]
    pub target: Entity,
    pub damage: f32,
}

pub fn on_damage_received(
    event: On<DamageReceived>,
    mut commands: Commands,
    mut target_query: Query<&mut Health>,
) {
    if let Ok(mut health) = target_query.get_mut(event.target) {
        health.current -= event.damage;

        if health.current < 0. {
            commands.trigger(HealthExpired(event.target));
        }
    }
}

pub fn on_health_expired(event: On<HealthExpired>, mut commands: Commands) {
    info!("health expired");
    commands.entity(event.0).despawn();
}

/// A floating health bar that tracks a ship's [`ShipHealth`]. A top-level (un-
/// parented) entity so it stays upright and level above the ship regardless of the
/// ship's facing; its colored fill child is [`HealthBarFill`].
#[derive(Component)]
pub(crate) struct HealthBar {
    ship: Entity,
}

/// The colored fill of a [`HealthBar`]; its width (x-scale) and color track the
/// ship's health fraction.
#[derive(Component)]
pub(crate) struct HealthBarFill;

const BAR_WIDTH: f32 = 64.;
const BAR_HEIGHT: f32 = 8.;
/// How far above the ship's center the bar floats (world units; clears the hull and
/// its top modules).
const BAR_Y_OFFSET: f32 = 150.;

/// Spawn a floating health bar for each ship the first frame it has a [`ShipHealth`].
pub(crate) fn spawn_health_bars(
    mut commands: Commands,
    new_ships: Query<Entity, Added<ShipHealth>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<ColorMaterial>>,
) {
    for ship in &new_ships {
        let background = meshes.add(Rectangle::new(BAR_WIDTH, BAR_HEIGHT));
        let fill = meshes.add(Rectangle::new(BAR_WIDTH, BAR_HEIGHT));
        let dark = materials.add(Color::srgba(0.05, 0.05, 0.05, 0.85));
        let green = materials.add(health_color(1.0));

        commands
            .spawn((
                HealthBar { ship },
                Transform::from_xyz(0., 0., 50.),
                Visibility::default(),
            ))
            .with_children(|parent| {
                parent.spawn((
                    Mesh2d(background),
                    MeshMaterial2d(dark),
                    Transform::from_xyz(0., 0., 0.),
                ));
                parent.spawn((
                    HealthBarFill,
                    Mesh2d(fill),
                    MeshMaterial2d(green),
                    Transform::from_xyz(0., 0., 0.1),
                ));
            });
    }
}

/// Move each health bar above its ship and size/recolor its fill by the ship's
/// health fraction. Despawns the bar once its ship is gone.
pub(crate) fn update_health_bars(
    mut commands: Commands,
    ships: Query<(&GlobalTransform, &ShipHealth)>,
    mut bars: Query<(Entity, &HealthBar, &mut Transform, &Children)>,
    mut fills: Query<&mut Transform, (With<HealthBarFill>, Without<HealthBar>)>,
    fill_materials: Query<&MeshMaterial2d<ColorMaterial>, With<HealthBarFill>>,
    mut materials: ResMut<Assets<ColorMaterial>>,
) {
    for (bar_entity, bar, mut bar_transform, children) in &mut bars {
        let Ok((ship_gt, health)) = ships.get(bar.ship) else {
            // Ship destroyed — remove its bar.
            commands.entity(bar_entity).despawn();
            continue;
        };

        let p = ship_gt.translation();
        bar_transform.translation = Vec3::new(p.x, p.y + BAR_Y_OFFSET, 50.);

        let frac = if health.max > 0. {
            (health.current / health.max).clamp(0., 1.)
        } else {
            0.
        };

        for &child in children {
            if let Ok(mut fill_transform) = fills.get_mut(child) {
                // Scale the fill from full width down to `frac`, anchored at the left.
                fill_transform.scale.x = frac;
                fill_transform.translation.x = -BAR_WIDTH / 2. * (1. - frac);
            }
            if let Ok(mat) = fill_materials.get(child) {
                if let Some(mut material) = materials.get_mut(&mat.0) {
                    material.color = health_color(frac);
                }
            }
        }
    }
}

/// Green when full, through yellow, to red when empty.
fn health_color(frac: f32) -> Color {
    Color::srgb((2.0 * (1.0 - frac)).min(1.0), (2.0 * frac).min(1.0), 0.1)
}
