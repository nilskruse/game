pub mod bullet;
pub mod turret;

use avian2d::prelude::*;
use bevy::prelude::*;

#[derive(Component)]
pub struct PlayerShip;

#[derive(Component)]
pub struct ShipBase;

#[derive(Component)]
pub struct ShipModule;

#[derive(Component)]
pub struct ModuleAttachmentPoint;

#[derive(Component)]
#[require(Transform)]
pub struct TurretAttachmentPoint;

pub fn spawn_player_ship(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<ColorMaterial>>,
) {
    commands.spawn(Camera2d);

    let ship_rectangle = Rectangle::new(50., 50.);
    let ship_base = spawn_player_ship_base(
        ship_rectangle,
        commands.reborrow(),
        &mut meshes,
        &mut materials,
    );

    let module_attachment_point = spawn_module_attachment_point(
        ship_base,
        ship_rectangle,
        commands.reborrow(),
        &mut meshes,
        &mut materials,
    );

    let module_rectangle = Rectangle::new(50., 50.);
    let module = spawn_module(
        module_attachment_point,
        module_rectangle,
        commands.reborrow(),
        &mut meshes,
        &mut materials,
    );

    let turret_attachment_point =
        create_turret_attachment_point(module, commands.reborrow(), &mut meshes, &mut materials);
    let _turret = turret::spawn_turret(
        turret_attachment_point,
        commands.reborrow(),
        &mut meshes,
        &mut materials,
    );
}

pub fn spawn_player_ship_base(
    rectangle: Rectangle,
    mut commands: Commands,
    meshes: &mut ResMut<Assets<Mesh>>,
    materials: &mut ResMut<Assets<ColorMaterial>>,
) -> Entity {
    commands
        .spawn((
            PlayerShip,
            ShipBase,
            RigidBody::Dynamic,
            Transform::from_xyz(100., 0., 1.),
            Collider::from(rectangle),
            Mesh2d(meshes.add(rectangle)),
            MeshMaterial2d(materials.add(Color::srgb(1., 1., 0.))),
        ))
        .id()
}

pub fn spawn_module(
    parent: Entity,
    rectangle: Rectangle,
    mut commands: Commands,
    meshes: &mut ResMut<Assets<Mesh>>,
    materials: &mut ResMut<Assets<ColorMaterial>>,
) -> Entity {
    commands
        .spawn((
            ShipModule,
            Transform::from_xyz(rectangle.half_size.x, 0., 1.),
            Collider::from(rectangle),
            Mesh2d(meshes.add(rectangle)),
            MeshMaterial2d(materials.add(Color::srgb(1., 1., 0.))),
            ChildOf(parent),
        ))
        .id()
}

pub fn spawn_module_attachment_point(
    parent: Entity,
    parent_rectangle: Rectangle,
    mut commands: Commands,
    meshes: &mut ResMut<Assets<Mesh>>,
    materials: &mut ResMut<Assets<ColorMaterial>>,
) -> Entity {
    let half_width = parent_rectangle.half_size.x;

    commands
        .spawn((
            ModuleAttachmentPoint,
            Transform::from_xyz(half_width, 0., 0.),
            ChildOf(parent),
            Mesh2d(meshes.add(Circle::new(5.))),
            MeshMaterial2d(materials.add(Color::srgb(0., 0., 1.))),
        ))
        .id()
}

pub fn create_turret_attachment_point(
    parent: Entity,
    mut commands: Commands,
    meshes: &mut ResMut<Assets<Mesh>>,
    materials: &mut ResMut<Assets<ColorMaterial>>,
) -> Entity {
    commands
        .spawn((
            TurretAttachmentPoint,
            ChildOf(parent),
            Mesh2d(meshes.add(Circle::new(5.))),
            MeshMaterial2d(materials.add(Color::srgb(0., 0., 1.))),
        ))
        .id()
}
