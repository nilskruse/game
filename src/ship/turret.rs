use bevy::{
    asset::RenderAssetUsages,
    mesh::{Indices, PrimitiveTopology},
    prelude::*,
};

use crate::{faction::InFaction, health::ModuleDisabled, ship::bullet};

#[derive(Component)]
#[require(Transform)]
pub struct Turret {
    timer: Timer,
    _fire_rate: f32,
    velocity: f32,
    damage: f32,
}

impl Turret {
    pub fn new(fire_rate: f32, velocity: f32, damage: f32) -> Self {
        let timer = Timer::from_seconds(fire_rate, TimerMode::Repeating);
        Self {
            timer,
            _fire_rate: fire_rate,
            velocity,
            damage,
        }
    }
}

#[derive(Component)]
#[relationship(relationship_target = TargettedBy)]
pub struct Target(pub Entity);
#[derive(Component)]
#[relationship_target(relationship = Target)]
pub struct TargettedBy(Vec<Entity>);

pub fn spawn_turret(
    parent: Entity,
    mut commands: Commands,
    meshes: &mut ResMut<Assets<Mesh>>,
    materials: &mut ResMut<Assets<ColorMaterial>>,
) -> Entity {
    let shape = create_combined_mesh();
    commands
        .spawn((
            Turret::new(1.0, 2000., 100.),
            Mesh2d(meshes.add(shape)),
            MeshMaterial2d(materials.add(ColorMaterial::default())),
            ChildOf(parent),
        ))
        .id()
}

pub fn select_target(
    mut commands: Commands,
    turret_query: Query<(Entity, &InFaction), (With<Turret>, Without<Target>)>,
    target_query: Query<(Entity, &InFaction)>,
) {
    for (turret_entity, turret_faction) in turret_query.iter() {
        for (target_entity, target_faction) in target_query.iter() {
            if turret_faction != target_faction {
                commands.entity(turret_entity).insert(Target(target_entity));
                break;
            }
        }
    }
}

pub fn rotate_turret(
    mut turret_query: Query<(&Target, &Turret, &mut Transform, &GlobalTransform)>,
    enemy_query: Query<&GlobalTransform>,
) {
    for (target, _turret, mut turret_transform, turret_global_transform) in turret_query.iter_mut()
    {
        // The target may have been destroyed; just skip this turret until it
        // re-acquires (the `Target` relationship is cleared when its entity despawns).
        let Ok(enemy_global_transform) = enemy_query.get(target.0) else {
            continue;
        };

        let enemy_translation = enemy_global_transform.translation().xy();
        let turret_translation = turret_global_transform.translation().xy();
        let to_enemy = (enemy_translation - turret_translation).normalize();
        let desired_global_rotation =
            Quat::from_rotation_arc(Vec3::Y, to_enemy.extend(0.)).normalize();

        // Derive the parent rotation from: global = parent * local
        // So: parent = global * local.inverse()
        let turret_global_rotation = turret_global_transform.rotation();
        let parent_rotation = turret_global_rotation * turret_transform.rotation.conjugate();

        // Convert desired global to local: local = parent.inverse() * global
        let local_rotation = parent_rotation.conjugate() * desired_global_rotation;

        turret_transform.rotation = local_rotation;
    }
}

pub fn fire_turret(
    mut commands: Commands,
    mut turret_query: Query<(&mut Turret, &GlobalTransform, &InFaction, &ChildOf), With<Target>>,
    disabled: Query<(), With<ModuleDisabled>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<ColorMaterial>>,
    time: Res<Time>,
) {
    for (mut turret, turret_global_transform, faction, child_of) in turret_query.iter_mut() {
        turret.timer.tick(time.delta());
        if !turret.timer.just_finished() {
            continue;
        }
        // A turret sits on a module block; if that block is shot out, it can't fire.
        if disabled.contains(child_of.parent()) {
            continue;
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
