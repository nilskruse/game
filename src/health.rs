use bevy::prelude::*;

#[derive(Component, Deref, DerefMut)]
// #[require(Observer::new(on_damage_received))]
pub struct Health {
    pub current: f32,
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
    info!("damage received");

    if target_query.contains(event.target) {
        info!("entity with health received damage");
        let mut health = target_query
            .get_mut(event.target)
            .expect("there should be health");
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
