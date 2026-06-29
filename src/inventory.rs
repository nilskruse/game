//! The player inventory: a data model ([`Inventory`]) plus a window that *views* it,
//! built on the `ui` toolkit. Toggle it with the **I** key or the on-screen button; it
//! opens as a panel down the left side.
//!
//! Deliberately a view-over-model: the window holds no item state, only renders
//! [`Inventory::items`], and rebuilds when the model changes ([`rebuild_inventory_ui`]).
//! Each slot entity carries an [`InventorySlot`] (its index into `items`). That is the
//! hook for the planned **drag-and-drop into build mode**: a `Pointer<DragStart>` observer
//! on a slot reads its index → `items[index]`; for an [`ItemKind::Module`] the drop in
//! build mode will place that `ModuleKind` onto the ship (picking already emits the drag
//! events — see `ui`). Mutating the model then rebuilds the view, so save/load stays a
//! matter of persisting `Inventory`, never the UI.

use bevy::picking::events::{Click, Pointer};
use bevy::prelude::*;

use crate::build::{ModuleKind, ModuleRegistry};
use crate::ship::{PlayerShip, ShipBase};
use crate::station::SpaceStation;
use crate::ui::{self, Theme};

pub struct InventoryPlugin;

impl Plugin for InventoryPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, spawn_inventory_ui)
            // Attach an inventory the moment a structure root appears, rather than polling
            // every frame.
            .add_observer(attach_inventory::<ShipBase>)
            .add_observer(attach_inventory::<SpaceStation>)
            .add_systems(
                Update,
                (
                    seed_player_inventory,
                    toggle_inventory_hotkey,
                    rebuild_inventory_ui,
                ),
            );
    }
}

// ---------------------------------------------------------------------------
// Data model
// ---------------------------------------------------------------------------

/// What an inventory item *is*. Modules can be placed onto the ship in build mode (the
/// planned drag-and-drop); components are generic build materials for now. Later this is
/// the place an `ItemDef`/item-registry id would live (same pattern as `ModuleRegistry`).
#[derive(Clone)]
pub(crate) enum ItemKind {
    /// A buildable ship module, identified by its [`ModuleKind`].
    Module(ModuleKind),
    /// A generic component / material (placeholder until items have definitions).
    Component,
}

/// One stack of identical items in the inventory.
#[derive(Clone)]
pub(crate) struct ItemStack {
    pub kind: ItemKind,
    pub name: String,
    pub count: u32,
}

/// An inventory, owned **per structure** — it lives on a ship or station root, not on the
/// player. The window shows the player's current ship's inventory. Cargo modules will
/// later set its capacity; for now it's an unbounded list.
#[derive(Component, Default)]
pub(crate) struct Inventory {
    pub items: Vec<ItemStack>,
}

/// Give a structure root its own (empty) [`Inventory`] the moment it appears. An `Add`
/// observer (registered for `ShipBase` and `SpaceStation`), so it fires exactly once per
/// structure — no per-frame polling — and covers initial, runtime-built (enemy) and
/// save-loaded roots alike.
fn attach_inventory<C: Component>(add: On<Add, C>, mut commands: Commands) {
    commands.entity(add.entity).insert(Inventory::default());
}

/// Fill the player ship's inventory with a starter set the moment it first gets one
/// (initial spawn, and again after a load / new game rebuilds the ship). Demo content
/// until cargo modules and real item acquisition exist. Module names come from the
/// registry so they track `module_defs()`.
fn seed_player_inventory(
    mut ships: Query<&mut Inventory, (Added<Inventory>, With<PlayerShip>)>,
    registry: Res<ModuleRegistry>,
) {
    let Ok(mut inventory) = ships.single_mut() else {
        return;
    };
    let modules = [
        (ModuleKind::Cargo, 3),
        (ModuleKind::Hallway, 4),
        (ModuleKind::Thruster, 2),
        (ModuleKind::Turret, 1),
        (ModuleKind::Engine, 2),
    ];
    for (kind, count) in modules {
        inventory.items.push(ItemStack {
            kind: ItemKind::Module(kind),
            name: registry.module(kind).name.to_string(),
            count,
        });
    }
    for (name, count) in [("Steel Plate", 12), ("Power Cell", 5), ("Circuitry", 8)] {
        inventory.items.push(ItemStack {
            kind: ItemKind::Component,
            name: name.to_string(),
            count,
        });
    }
}

// ---------------------------------------------------------------------------
// View
// ---------------------------------------------------------------------------

/// Root of the inventory window (the left-side panel). Toggled via its [`Visibility`].
#[derive(Component)]
struct InventoryWindow;

/// The container the item slots are (re)built into — kept separate from the heading so a
/// rebuild only replaces the slot list.
#[derive(Component)]
struct SlotContainer;

/// A single item slot, tagging the model index it shows. The future drag-into-build hook
/// reads this to know which [`ItemStack`] is being dragged.
#[derive(Component)]
pub(crate) struct InventorySlot {
    // Read by the future drag-into-build handler (see module docs); unused until then.
    #[allow(dead_code)]
    pub index: usize,
}

/// Spawn the (initially hidden) inventory window down the left edge, plus the bottom-right
/// toggle button that opens/closes it.
fn spawn_inventory_ui(mut commands: Commands, theme: Res<Theme>) {
    // The window: an absolute, full-height container (so it can't use `ui::panel`'s baked
    // Node) styled with `ui::panel_style`.
    commands
        .spawn((
            InventoryWindow,
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(theme.space.md),
                top: Val::Px(theme.space.md),
                bottom: Val::Px(theme.space.md),
                width: Val::Px(280.),
                padding: UiRect::all(Val::Px(theme.space.md)),
                border: UiRect::all(Val::Px(1.)),
                border_radius: BorderRadius::all(Val::Px(theme.radius)),
                flex_direction: FlexDirection::Column,
                row_gap: Val::Px(theme.space.sm),
                ..default()
            },
            ui::panel_style(&theme),
            GlobalZIndex(ui::Z_PANEL),
            Visibility::Hidden,
        ))
        .with_children(|window| {
            window.spawn(ui::heading(&theme, "Inventory"));
            window.spawn((
                SlotContainer,
                Node {
                    flex_direction: FlexDirection::Column,
                    row_gap: Val::Px(theme.space.xs),
                    // Fill the remaining height below the heading.
                    flex_grow: 1.,
                    ..default()
                },
            ));
        });

    // The toggle button, bottom-right (clear of the New Game button top-right and the
    // left-side window). Clicking it toggles the window, same as the I key.
    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                right: Val::Px(theme.space.md),
                bottom: Val::Px(theme.space.md),
                ..default()
            },
            GlobalZIndex(ui::Z_HUD),
        ))
        .with_children(|bar| {
            bar.spawn(ui::button(&theme, "Inventory (I)")).observe(
                |_: On<Pointer<Click>>,
                 mut windows: Query<&mut Visibility, With<InventoryWindow>>| {
                    for mut visibility in &mut windows {
                        toggle(&mut visibility);
                    }
                },
            );
        });
}

/// Toggle the inventory window with the **I** key.
fn toggle_inventory_hotkey(
    keyboard: Res<ButtonInput<KeyCode>>,
    mut windows: Query<&mut Visibility, With<InventoryWindow>>,
) {
    if !keyboard.just_pressed(KeyCode::KeyI) {
        return;
    }
    for mut visibility in &mut windows {
        toggle(&mut visibility);
    }
}

/// Flip a window between shown and hidden.
fn toggle(visibility: &mut Visibility) {
    *visibility = if *visibility == Visibility::Hidden {
        Visibility::Visible
    } else {
        Visibility::Hidden
    };
}

/// Rebuild the slot list whenever the player ship's [`Inventory`] changes (the initial
/// seed, any later mutation, or a load/new-game swapping in a fresh ship — `Added` counts
/// as `Changed`). Clears the slot container and respawns one slot per [`ItemStack`] — the
/// view follows the model.
fn rebuild_inventory_ui(
    inventories: Query<&Inventory, (With<PlayerShip>, Changed<Inventory>)>,
    registry: Res<ModuleRegistry>,
    theme: Res<Theme>,
    mut commands: Commands,
    container: Query<Entity, With<SlotContainer>>,
    children: Query<&Children>,
) {
    let Ok(inventory) = inventories.single() else {
        return;
    };
    let Ok(container) = container.single() else {
        return;
    };
    // Drop the old slots (despawn is recursive, so their swatch/text go too).
    if let Ok(existing) = children.get(container) {
        for &slot in existing {
            commands.entity(slot).despawn();
        }
    }
    commands.entity(container).with_children(|list| {
        for (index, stack) in inventory.items.iter().enumerate() {
            // A module slot previews the module's build color; a component is neutral.
            let swatch = match stack.kind {
                ItemKind::Module(kind) => registry.module(kind).color,
                ItemKind::Component => theme.palette.text_dim,
            };
            list.spawn((
                InventorySlot { index },
                Node {
                    flex_direction: FlexDirection::Row,
                    align_items: AlignItems::Center,
                    column_gap: Val::Px(theme.space.sm),
                    padding: UiRect::all(Val::Px(theme.space.xs)),
                    border_radius: BorderRadius::all(Val::Px(theme.radius - 2.)),
                    width: Val::Percent(100.),
                    ..default()
                },
                BackgroundColor(theme.palette.surface_alt),
            ))
            .with_children(|slot| {
                // Color swatch (a stand-in for the item icon to come).
                slot.spawn((
                    Node {
                        width: Val::Px(26.),
                        height: Val::Px(26.),
                        border_radius: BorderRadius::all(Val::Px(4.)),
                        ..default()
                    },
                    BackgroundColor(swatch),
                ));
                slot.spawn(ui::column(theme.space.xs))
                    .with_children(|info| {
                        info.spawn(ui::label(&theme, stack.name.clone()));
                        info.spawn(ui::small(&theme, format!("×{}", stack.count)));
                    });
            });
        }
    });
}
