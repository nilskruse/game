use avian2d::prelude::*;
use bevy::prelude::*;

use crate::player::Player;

/// A point in the world the player can interact with by standing near it and
/// pressing the interact key (E). Use this for consoles, switches, terminals,
/// etc. — anything that triggers behavior without being walked into.
#[derive(Component)]
pub struct Interactable {
    /// Shown/logged when activated; identifies what this does.
    pub label: String,
    /// How close (world units) the player must be to activate it.
    pub range: f32,
}

/// Fired at an [`Interactable`] entity when the player activates it. Attach an
/// observer to the entity (see [`spawn_console`]) to give it behavior. This is
/// the extension point: today the placeholder observer just logs.
#[derive(EntityEvent)]
pub struct Interacted(pub Entity);

/// Spawn a wall console: an [`Interactable`] marker with a small visual, parented
/// to `parent` at local `position`. Activating it currently just logs `label`;
/// replace the attached observer to drive real behavior (settings, status, ...).
pub fn spawn_console(
    parent: Entity,
    position: Vec2,
    label: &str,
    mut commands: Commands,
    meshes: &mut ResMut<Assets<Mesh>>,
    materials: &mut ResMut<Assets<ColorMaterial>>,
) -> Entity {
    let panel = Rectangle::new(22., 10.);
    commands
        .spawn((
            Interactable {
                label: label.to_string(),
                range: 45.,
            },
            ChildOf(parent),
            Transform::from_xyz(position.x, position.y, 0.5),
            Mesh2d(meshes.add(panel)),
            MeshMaterial2d(materials.add(Color::srgb(0.1, 0.95, 0.95))),
        ))
        .observe(on_console_interact)
        .id()
}

/// Placeholder behavior for a console. Swap this (or add per-console observers)
/// to open settings for the module it fronts, e.g. an engineering panel.
fn on_console_interact(event: On<Interacted>, interactables: Query<&Interactable>) {
    if let Ok(interactable) = interactables.get(event.0) {
        info!(
            "Console activated: {} (placeholder — hook real behavior here)",
            interactable.label
        );
    }
}

/// On E, activate the nearest in-range [`Interactable`]. Runs in `Update` so the
/// key edge is never missed.
pub fn interact(
    keyboard: Res<ButtonInput<KeyCode>>,
    mut commands: Commands,
    player: Query<&Position, With<Player>>,
    interactables: Query<(Entity, &Interactable, &GlobalTransform)>,
) {
    if !keyboard.just_pressed(KeyCode::KeyE) {
        return;
    }
    let Ok(player_pos) = player.single() else {
        return;
    };

    let mut best: Option<(Entity, f32)> = None;
    for (entity, interactable, gt) in &interactables {
        let dist = player_pos.0.distance(gt.translation().xy());
        if dist <= interactable.range && best.map_or(true, |(_, b)| dist < b) {
            best = Some((entity, dist));
        }
    }
    if let Some((entity, _)) = best {
        commands.trigger(Interacted(entity));
    }
}
