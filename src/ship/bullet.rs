use avian2d::prelude::*;
use bevy::prelude::*;

use crate::health::{DamageReceived, Health};

#[derive(Component)]
struct Bullet {
    damage: f32,
}

pub fn spawn(
    spawn_location: Transform,
    spawn_velocity: Vec2,
    damage: f32,
    mut commands: Commands,
    meshes: &mut ResMut<Assets<Mesh>>,
    materials: &mut ResMut<Assets<ColorMaterial>>,
) {
    let shape = Rectangle::new(5., 10.);
    commands
        .spawn((
            Bullet { damage },
            spawn_location,
            Collider::from(shape),
            Mesh2d(meshes.add(shape)),
            MeshMaterial2d(materials.add(Color::srgb(1., 1., 1.))),
            LinearVelocity(spawn_velocity),
            RigidBody::Kinematic,
            Sensor,
            CollisionEventsEnabled,
        ))
        .observe(on_bullet_hit);
}

fn on_bullet_hit(
    event: On<CollisionStart>,
    mut commands: Commands,
    target_query: Query<(), With<Health>>,
    bullet_query: Query<&Bullet>,
) {
    info!("bullet hit: {:?}", event);

    if target_query.contains(event.collider2) {
        info!("bullet hit something with health");
        let damage = bullet_query
            .get(event.collider1)
            .expect("there should be bullet")
            .damage;

        commands.trigger(DamageReceived {
            target: event.collider2,
            damage,
        });

        commands.entity(event.collider1).despawn();
    }
}
