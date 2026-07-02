//! On-screen debug overlay, toggled with **F3** (starts hidden). Shows fps, the
//! floating-origin offset, and the player's / player ship's local (scene) and world
//! (`WorldOrigin` + local) positions — the main tool for eyeballing origin rebases.

use avian2d::prelude::{AngularVelocity, LinearVelocity, Position, Rotation};
use bevy::diagnostic::{DiagnosticsStore, FrameTimeDiagnosticsPlugin};
use bevy::prelude::*;

use crate::origin::WorldOrigin;
use crate::player::Player;
use crate::ship::PlayerShip;

pub struct DebugOverlayPlugin;

impl Plugin for DebugOverlayPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(FrameTimeDiagnosticsPlugin::default())
            .add_systems(Startup, spawn_debug_overlay)
            .add_systems(Update, (toggle_debug_overlay, update_debug_text).chain());
    }
}

/// The overlay's absolute container (visibility is toggled on this).
#[derive(Component)]
struct DebugPanel;

/// The text block inside the overlay, rebuilt each frame while visible.
#[derive(Component)]
struct DebugText;

/// Top-right, below the New Game button (top-left belongs to the build hint panel).
fn spawn_debug_overlay(mut commands: Commands, theme: Res<crate::ui::Theme>) {
    commands
        .spawn((
            DebugPanel,
            Node {
                position_type: PositionType::Absolute,
                right: Val::Px(10.),
                top: Val::Px(56.),
                ..default()
            },
            GlobalZIndex(crate::ui::Z_HUD),
            Visibility::Hidden,
        ))
        .with_children(|parent| {
            parent
                .spawn(crate::ui::panel(&theme))
                .with_children(|panel| {
                    panel.spawn((DebugText, crate::ui::small(&theme, "")));
                });
        });
}

fn toggle_debug_overlay(
    keyboard: Res<ButtonInput<KeyCode>>,
    mut panels: Query<&mut Visibility, With<DebugPanel>>,
) {
    if !keyboard.just_pressed(KeyCode::F3) {
        return;
    }
    for mut visibility in &mut panels {
        *visibility = match *visibility {
            Visibility::Hidden => Visibility::Visible,
            _ => Visibility::Hidden,
        };
    }
}

/// Rebuild the overlay text (only while visible). Positions are read from the avian
/// `Position` (current, not the frame-lagged `GlobalTransform`); world = origin + local.
fn update_debug_text(
    panels: Query<&Visibility, With<DebugPanel>>,
    mut texts: Query<&mut Text, With<DebugText>>,
    diagnostics: Res<DiagnosticsStore>,
    origin: Res<WorldOrigin>,
    player: Query<(&Position, &LinearVelocity), With<Player>>,
    ships: Query<(&Position, &Rotation, &LinearVelocity, &AngularVelocity), With<PlayerShip>>,
    structures: Query<Has<crate::bubble::Simulated>, With<crate::save::Origin>>,
) {
    if panels.iter().all(|v| *v != Visibility::Visible) {
        return;
    }
    let Ok(mut text) = texts.single_mut() else {
        return;
    };

    let fps = diagnostics
        .get(&FrameTimeDiagnosticsPlugin::FPS)
        .and_then(|d| d.smoothed());
    let mut s = match fps {
        Some(fps) => format!("fps {fps:.0}"),
        None => "fps --".to_string(),
    };
    s.push_str(&format!("\norigin {:.1}, {:.1}", origin.0.x, origin.0.y));
    let dormant = structures.iter().filter(|&d| d).count();
    s.push_str(&format!(
        "\nstructures {} active / {dormant} dormant",
        structures.iter().count() - dormant,
    ));
    if let Ok((pos, vel)) = player.single() {
        let world = origin.0 + pos.0.as_dvec2();
        s.push_str(&format!(
            "\nplayer local {:.1}, {:.1}\nplayer world {:.1}, {:.1}\nplayer speed {:.1}",
            pos.0.x,
            pos.0.y,
            world.x,
            world.y,
            vel.0.length(),
        ));
    }
    if let Ok((pos, rot, lin, ang)) = ships.single() {
        let world = origin.0 + pos.0.as_dvec2();
        s.push_str(&format!(
            "\nship local {:.1}, {:.1}\nship world {:.1}, {:.1}\nship speed {:.1}  heading {:.0}\u{b0}  spin {:.2}",
            pos.0.x,
            pos.0.y,
            world.x,
            world.y,
            lin.0.length(),
            rot.as_radians().to_degrees(),
            ang.0,
        ));
    }
    *text = Text::new(s);
}
