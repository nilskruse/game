use crate::{
    animation::{Animated, Animations},
    build::{build_buildable_side, mount_preplaced_turret},
    character::Character,
    faction::{Faction, InFaction},
    health::{self, Health},
    ship::{ShipBase, StructureRoot, ThrustCommand, ThrustControl},
};
use avian2d::prelude::*;
use bevy::{app::Propagate, prelude::*};

pub fn spawn_enemy(
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    mut texture_atlas_layouts: ResMut<Assets<TextureAtlasLayout>>,
) {
    let texture = asset_server.load("Factions/Goblins/Troops/Torch/Purple/Torch_Purple.png");

    let layout = TextureAtlasLayout::from_grid(UVec2::splat(192), 7, 6, None, None);

    let texture_atlas_layout = texture_atlas_layouts.add(layout);

    let animations = Animations::from([
        ("idle-left", (0, 6, true)),
        ("idle-right", (0, 6, false)),
        ("walk-left", (7, 12, true)),
        ("walk-right", (7, 12, false)),
        ("attack-right", (14, 19, false)),
        ("attack-left", (20, 25, true)),
        ("attack-down", (26, 31, false)),
        ("attack-up", (32, 37, false)),
    ]);

    commands
        .spawn((
            Character::default(),
            Propagate(InFaction(Faction::Enemy)),
            Health { current: 1000. },
            Animated {
                animations,

                ..Default::default()
            },
            RigidBody::Dynamic,
            Collider::rectangle(25., 25.),
            Transform::from_xyz(-100., 0., 1.),
            Sprite::from_atlas_image(
                texture,
                TextureAtlas {
                    layout: texture_atlas_layout,

                    index: 0,
                },
            ),
            LockedAxes::ROTATION_LOCKED,
        ))
        .observe(health::on_health_expired);
}

pub fn spawn_enemy_ship(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<ColorMaterial>>,
) {
    let ship_rectangle = Rectangle::new(50., 50.);
    let ship_base = spawn_enemy_ship_base(
        ship_rectangle,
        commands.reborrow(),
        &mut meshes,
        &mut materials,
    );

    // Mount a turret module on the right side through the shared module path (the
    // same one the player ship and station use). The turret inherits the Enemy
    // faction via hierarchy propagation from the ship base.
    let half = ship_rectangle.half_size;
    let right = build_buildable_side(
        &mut commands,
        ship_base,
        half,
        1,
        Vec2::X,
        &mut meshes,
        &mut materials,
    );
    mount_preplaced_turret(
        &mut commands,
        ship_base,
        &right[0],
        Vec2::X,
        &mut meshes,
        &mut materials,
    );
}

pub fn spawn_enemy_ship_base(
    rectangle: Rectangle,
    mut commands: Commands,
    meshes: &mut ResMut<Assets<Mesh>>,
    materials: &mut ResMut<Assets<ColorMaterial>>,
) -> Entity {
    let ship_base = commands
        .spawn((
            ShipBase,
            Propagate(InFaction(Faction::Enemy)),
            // Drivable by the same shared solver as the player ship; an AI sets the
            // `ThrustControl` intent (and adds `Piloted`) — no flight code is
            // player-specific.
            ThrustControl::default(),
            ThrustCommand::default(),
            RigidBody::Dynamic,
            Transform::from_xyz(-100., 0., 1.),
            Collider::from(rectangle),
            Mesh2d(meshes.add(rectangle)),
            MeshMaterial2d(materials.add(Color::srgb(1., 1., 0.))),
        ))
        .id();
    commands
        .entity(ship_base)
        .insert(Propagate(StructureRoot(ship_base)));
    ship_base
}
