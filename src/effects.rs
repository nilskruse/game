use bevy::prelude::*;

/// A short-lived entity that despawns when its timer ends — projectiles (PD slugs)
/// and transient visual effects (hit sparks).
#[derive(Component)]
pub struct Lifetime(pub Timer);

/// Despawn entities whose [`Lifetime`] has elapsed.
pub(crate) fn expire_lifetimes(
    time: Res<Time>,
    mut commands: Commands,
    mut query: Query<(Entity, &mut Lifetime)>,
) {
    for (entity, mut lifetime) in &mut query {
        lifetime.0.tick(time.delta());
        if lifetime.0.is_finished() {
            commands.entity(entity).try_despawn();
        }
    }
}

/// The sort of impact a spark marks, which sets how it looks — so a ship taking a hit
/// reads differently from a point-defense interception.
#[derive(Clone, Copy)]
pub enum Hit {
    /// A projectile struck a ship (or character): a larger orange burst.
    Ship,
    /// Point-defense struck an incoming projectile: a small cyan spark.
    Intercept,
}

/// A flash that grows from `from_scale` to `to_scale` and fades out over its
/// [`Lifetime`].
#[derive(Component)]
pub(crate) struct HitSpark {
    from_scale: f32,
    to_scale: f32,
}

/// Spawn a hit spark at world `pos`, styled by `hit`.
pub fn spawn_hit_spark(commands: &mut Commands, pos: Vec2, hit: Hit) {
    let (color, size, from, to, secs) = match hit {
        Hit::Ship => (Color::srgb(1.0, 0.55, 0.15), 16.0, 0.6, 2.2, 0.18),
        Hit::Intercept => (Color::srgb(0.6, 0.95, 1.0), 9.0, 0.7, 1.7, 0.12),
    };
    commands.spawn((
        HitSpark {
            from_scale: from,
            to_scale: to,
        },
        Sprite::from_color(color, Vec2::splat(size)),
        Transform::from_translation(pos.extend(2.0)).with_scale(Vec3::splat(from)),
        Lifetime(Timer::from_seconds(secs, TimerMode::Once)),
    ));
}

/// Grow and fade each hit spark across its lifetime (the timer is ticked by
/// [`expire_lifetimes`]).
pub(crate) fn animate_hit_sparks(
    mut sparks: Query<(&mut Sprite, &mut Transform, &Lifetime, &HitSpark)>,
) {
    for (mut sprite, mut transform, lifetime, spark) in &mut sparks {
        let f = lifetime.0.fraction().clamp(0.0, 1.0);
        let scale = spark.from_scale + (spark.to_scale - spark.from_scale) * f;
        transform.scale = Vec3::splat(scale);
        sprite.color = sprite.color.with_alpha(1.0 - f);
    }
}
