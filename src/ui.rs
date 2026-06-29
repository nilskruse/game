//! A small, reusable UI toolkit. The whole UI is styled from one place — the [`Theme`]
//! resource — and assembled from bundle-constructor helpers (`panel`, `window`,
//! `button`, the text and layout helpers) that read it. Restyle everything by editing
//! [`Theme::default`].
//!
//! Interaction is observer + `bevy_picking` based: a [`button`] carries [`ButtonColors`]
//! and gets automatic hover/press feedback from global observers (registered by
//! [`UiPlugin`]); attach behavior per button with
//! `.observe(|_: On<Pointer<Click>>, ...| { ... })`. [`PointerOverUi`] lets world systems
//! ignore clicks that landed on the UI.
//!
//! `bevy_picking` also emits drag events for free (`Pointer<DragStart>` / `Drag` /
//! `DragOver` / `DragDrop` / `DragEnd`), so the planned drag-and-drop inventory builds on
//! this without rework — see the [`Z_DRAG`] et al. layer constants, reserved so a dragged
//! item / tooltip / modal renders above ordinary panels.

use bevy::picking::events::{Out, Over, Pointer, Press, Release};
use bevy::picking::hover::HoverMap;
use bevy::prelude::*;

/// Sets up the UI toolkit: the [`Theme`], the [`PointerOverUi`] guard, and the global
/// button hover/press feedback observers.
pub struct UiPlugin;

impl Plugin for UiPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Theme>()
            .init_resource::<PointerOverUi>()
            .add_systems(Update, update_pointer_over_ui)
            .add_observer(on_button_over)
            .add_observer(on_button_out)
            .add_observer(on_button_press)
            .add_observer(on_button_release);
    }
}

// ---------------------------------------------------------------------------
// Z-layers
// ---------------------------------------------------------------------------

/// Stacking layers for UI, applied with `GlobalZIndex`. Higher renders on top. The high
/// layers are reserved so transient overlays (a dragged inventory item, a tooltip, a
/// modal dialog) always sit above ordinary panels.
pub const Z_HUD: i32 = 0;
pub const Z_PANEL: i32 = 10;
pub const Z_MODAL: i32 = 20;
pub const Z_TOOLTIP: i32 = 30;
pub const Z_DRAG: i32 = 40;

// ---------------------------------------------------------------------------
// Theme
// ---------------------------------------------------------------------------

/// The single styling source of truth: colors, spacing, text sizes, corner rounding and
/// the UI font. Tweak [`Theme::default`] to restyle the whole UI at once.
#[derive(Resource, Clone)]
pub struct Theme {
    pub palette: Palette,
    pub space: Spacing,
    pub text: TextScale,
    /// Corner rounding (px) for panels and buttons.
    pub radius: f32,
    /// UI font; `None` uses Bevy's built-in default font.
    pub font: Option<Handle<Font>>,
}

/// The UI color palette.
#[derive(Clone, Copy)]
pub struct Palette {
    /// Panel background.
    pub surface: Color,
    /// Raised elements inside a panel (slots, headers).
    pub surface_alt: Color,
    /// Panel / button borders.
    pub border: Color,
    /// Primary text.
    pub text: Color,
    /// Secondary / muted text.
    pub text_dim: Color,
    /// Highlight color for primary actions and selection.
    pub accent: Color,
    /// Text drawn on top of `accent`.
    pub accent_text: Color,
    pub button: Color,
    pub button_hover: Color,
    pub button_press: Color,
}

/// A 4-step spacing scale used for padding and gaps, so spacing stays consistent.
#[derive(Clone, Copy)]
pub struct Spacing {
    pub xs: f32,
    pub sm: f32,
    pub md: f32,
    pub lg: f32,
}

/// Font sizes for the three text roles.
#[derive(Clone, Copy)]
pub struct TextScale {
    pub small: f32,
    pub body: f32,
    pub heading: f32,
}

impl Default for Theme {
    fn default() -> Self {
        // Dark sci-fi palette, harmonizing with the hull/bronze look of the ships.
        Self {
            palette: Palette {
                surface: Color::srgb(0.106, 0.118, 0.149),
                surface_alt: Color::srgb(0.145, 0.165, 0.208),
                border: Color::srgb(0.227, 0.255, 0.314),
                text: Color::srgb(0.863, 0.902, 1.0),
                text_dim: Color::srgb(0.545, 0.576, 0.655),
                accent: Color::srgb(0.878, 0.451, 0.180),
                accent_text: Color::srgb(0.063, 0.075, 0.102),
                button: Color::srgb(0.165, 0.188, 0.251),
                button_hover: Color::srgb(0.227, 0.271, 0.333),
                button_press: Color::srgb(0.114, 0.133, 0.188),
            },
            space: Spacing {
                xs: 4.,
                sm: 8.,
                md: 12.,
                lg: 20.,
            },
            text: TextScale {
                small: 12.,
                body: 16.,
                heading: 22.,
            },
            radius: 6.,
            font: None,
        }
    }
}

impl Theme {
    /// The themed font handle (the built-in default font when none is set).
    fn font(&self) -> Handle<Font> {
        self.font.clone().unwrap_or_default()
    }
}

// ---------------------------------------------------------------------------
// Helper constructors (return `impl Bundle`)
// ---------------------------------------------------------------------------

/// A padded, rounded, bordered column container — the base of every panel and window.
/// Spawn it then `.with_children(...)` to fill it.
pub fn panel(theme: &Theme) -> impl Bundle {
    (
        Node {
            padding: UiRect::all(Val::Px(theme.space.md)),
            border: UiRect::all(Val::Px(1.)),
            border_radius: BorderRadius::all(Val::Px(theme.radius)),
            flex_direction: FlexDirection::Column,
            row_gap: Val::Px(theme.space.sm),
            ..default()
        },
        BackgroundColor(theme.palette.surface),
        BorderColor::all(theme.palette.border),
    )
}

/// A [`panel`] with a heading at the top — the base for inventory / trading / dialog
/// windows. Further children added with `.with_children(...)` appear below the title.
pub fn window(theme: &Theme, title: impl Into<String>) -> impl Bundle {
    (panel(theme), children![heading(theme, title)])
}

/// Just the *visual* styling of a [`panel`] (surface + border color), for callers that
/// build their own `Node` — e.g. an absolutely-positioned, full-height container that
/// [`panel`]'s baked-in `Node` can't express. Set the `Node`'s `border` / `border_radius`
/// yourself; [`panel`] is the ready-made version for ordinary content boxes.
pub fn panel_style(theme: &Theme) -> impl Bundle {
    (
        BackgroundColor(theme.palette.surface),
        BorderColor::all(theme.palette.border),
    )
}

/// A clickable, themed button carrying [`ButtonColors`] so it gets automatic hover/press
/// feedback. Attach behavior with `.observe(|_: On<Pointer<Click>>, ...| { ... })`.
pub fn button(theme: &Theme, label: impl Into<String>) -> impl Bundle {
    let colors = ButtonColors {
        normal: theme.palette.button,
        hover: theme.palette.button_hover,
        press: theme.palette.button_press,
    };
    (
        Button,
        Node {
            padding: UiRect::axes(Val::Px(theme.space.md), Val::Px(theme.space.sm)),
            border: UiRect::all(Val::Px(1.)),
            border_radius: BorderRadius::all(Val::Px(theme.radius)),
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
            ..default()
        },
        BackgroundColor(colors.normal),
        BorderColor::all(theme.palette.border),
        colors,
        children![text(theme, label, theme.text.body, theme.palette.text)],
    )
}

/// A heading-sized text bundle in the primary text color.
pub fn heading(theme: &Theme, content: impl Into<String>) -> impl Bundle {
    text(theme, content, theme.text.heading, theme.palette.text)
}

/// A body-sized text bundle in the primary text color.
pub fn label(theme: &Theme, content: impl Into<String>) -> impl Bundle {
    text(theme, content, theme.text.body, theme.palette.text)
}

/// A small text bundle in the muted (`text_dim`) color, for captions and hints.
pub fn small(theme: &Theme, content: impl Into<String>) -> impl Bundle {
    text(theme, content, theme.text.small, theme.palette.text_dim)
}

/// A themed text bundle at an explicit `size` and `color`. The role helpers
/// ([`heading`] / [`label`] / [`small`]) cover the common cases.
pub fn text(theme: &Theme, content: impl Into<String>, size: f32, color: Color) -> impl Bundle {
    (
        Text::new(content),
        TextFont {
            font: theme.font().into(),
            font_size: size.into(),
            ..default()
        },
        TextColor(color),
    )
}

/// A vertical layout node with `gap` (px) between children.
pub fn column(gap: f32) -> impl Bundle {
    Node {
        flex_direction: FlexDirection::Column,
        row_gap: Val::Px(gap),
        ..default()
    }
}

/// A horizontal layout node with `gap` (px) between children.
pub fn row(gap: f32) -> impl Bundle {
    Node {
        flex_direction: FlexDirection::Row,
        column_gap: Val::Px(gap),
        ..default()
    }
}

// ---------------------------------------------------------------------------
// Button hover/press feedback (global observers)
// ---------------------------------------------------------------------------

/// The three background colors a [`button`] swaps between as it is hovered and pressed.
/// Set automatically by [`button`]; the global observers in [`UiPlugin`] apply it.
#[derive(Component, Clone, Copy)]
pub struct ButtonColors {
    pub normal: Color,
    pub hover: Color,
    pub press: Color,
}

fn on_button_over(
    event: On<Pointer<Over>>,
    mut buttons: Query<(&ButtonColors, &mut BackgroundColor)>,
) {
    if let Ok((colors, mut bg)) = buttons.get_mut(event.entity) {
        bg.0 = colors.hover;
    }
}

fn on_button_out(
    event: On<Pointer<Out>>,
    mut buttons: Query<(&ButtonColors, &mut BackgroundColor)>,
) {
    if let Ok((colors, mut bg)) = buttons.get_mut(event.entity) {
        bg.0 = colors.normal;
    }
}

fn on_button_press(
    event: On<Pointer<Press>>,
    mut buttons: Query<(&ButtonColors, &mut BackgroundColor)>,
) {
    if let Ok((colors, mut bg)) = buttons.get_mut(event.entity) {
        bg.0 = colors.press;
    }
}

fn on_button_release(
    event: On<Pointer<Release>>,
    mut buttons: Query<(&ButtonColors, &mut BackgroundColor)>,
) {
    // Pointer is still over the button right after a click, so return to hover.
    if let Ok((colors, mut bg)) = buttons.get_mut(event.entity) {
        bg.0 = colors.hover;
    }
}

// ---------------------------------------------------------------------------
// Pointer-over-UI guard
// ---------------------------------------------------------------------------

/// `true` while the cursor is over any UI node. World-space click systems (weapon fire,
/// build placement) check this so a click on the UI doesn't also act on the world.
#[derive(Resource, Default)]
pub struct PointerOverUi(pub bool);

/// Recompute [`PointerOverUi`] from `bevy_picking`'s hover map: true if any hovered
/// entity is a UI node.
fn update_pointer_over_ui(
    hover_map: Res<HoverMap>,
    ui_nodes: Query<(), With<Node>>,
    mut over: ResMut<PointerOverUi>,
) {
    over.0 = hover_map
        .values()
        .flat_map(|hits| hits.keys())
        .any(|entity| ui_nodes.contains(*entity));
}
