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

use bevy::picking::events::{Click, Drag, DragEnd, DragStart, Pointer};
use bevy::picking::Pickable;
use bevy::prelude::*;

use crate::build::{
    begin_module_drag, drop_module, AttachPoint, BuildMode, BuiltModule, ModuleKind, ModuleRegistry,
};
use crate::ship::{PlayerShip, ShipBase};
use crate::station::SpaceStation;
use crate::ui::{self, PointerOverUi, Theme};

pub struct InventoryPlugin;

impl Plugin for InventoryPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, spawn_inventory_ui)
            // Attach an inventory the moment a structure root appears, rather than polling
            // every frame.
            .add_observer(attach_inventory::<ShipBase>)
            .add_observer(attach_inventory::<SpaceStation>)
            // Drag a module out of the inventory onto the ship while in build mode.
            .add_observer(on_slot_drag_start)
            .add_observer(on_slot_drag)
            .add_observer(on_slot_drag_end)
            .add_systems(
                Update,
                (
                    seed_player_inventory,
                    toggle_inventory_hotkey,
                    sync_inventory_to_build_mode,
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
    /// A buildable ship module, identified by its [`ModuleKind`]. Build-relevant.
    Module(ModuleKind),
    /// A build material / part (placeholder until items have definitions). Build-relevant.
    Component,
    /// A general trade good / cargo (ore, supplies, loot). Not used in build mode.
    Trade,
}

impl ItemKind {
    /// Whether this item belongs in build mode — a module to install or a build material.
    /// General trade goods are filtered out of the inventory while building.
    fn build_relevant(&self) -> bool {
        matches!(self, ItemKind::Module(_) | ItemKind::Component)
    }
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
    // Non-build cargo, so the build-mode filter has something to hide.
    for (name, count) in [("Iron Ore", 40), ("Med Supplies", 6)] {
        inventory.items.push(ItemStack {
            kind: ItemKind::Trade,
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

/// A single item slot, tagging the model index it shows. The drag-into-build handlers read
/// this to know which [`ItemStack`] is being dragged.
#[derive(Component)]
pub(crate) struct InventorySlot {
    pub index: usize,
}

/// A small chip that follows the cursor while dragging an item out of the inventory, drawn
/// on the `Z_DRAG` layer so it sits *above* the inventory panel (the in-world build ghost
/// is behind the UI). It shows what's being dragged; the footprint ghost still previews
/// placement on the ship.
#[derive(Component)]
struct DragChip;

/// Spawn the (initially hidden) inventory window down the left edge, plus the bottom-right
/// toggle button that opens/closes it.
fn spawn_inventory_ui(mut commands: Commands, theme: Res<Theme>) {
    // The window: an absolute, full-height container (so it can't use `ui::panel`'s baked
    // Node). Its background is translucent so the world build ghost / ship show through it
    // — UI always draws over the 2D world, so an opaque panel would hide the ghost behind
    // it during a drag.
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
            BackgroundColor(theme.palette.surface.with_alpha(0.82)),
            BorderColor::all(theme.palette.border),
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

/// Open the inventory on entering build mode and close it on leaving — acting only on the
/// active-state edge, so manual toggles (the I key / button) still work in between.
fn sync_inventory_to_build_mode(
    build: Res<BuildMode>,
    mut prev_active: Local<bool>,
    mut windows: Query<&mut Visibility, With<InventoryWindow>>,
) {
    if build.active == *prev_active {
        return;
    }
    *prev_active = build.active;
    let target = if build.active {
        Visibility::Visible
    } else {
        Visibility::Hidden
    };
    for mut visibility in &mut windows {
        *visibility = target;
    }
}

/// Rebuild the slot list when the player ship's [`Inventory`] changes (the initial seed,
/// any later mutation, or a load/new-game swapping in a fresh ship — `Added` counts as
/// `Changed`) or when build mode is toggled (the filter applies/lifts). Clears the slot
/// container and respawns one slot per [`ItemStack`] — the view follows the model. While
/// building, only build-relevant items (modules + components) are shown.
fn rebuild_inventory_ui(
    build: Res<BuildMode>,
    mut prev_active: Local<bool>,
    registry: Res<ModuleRegistry>,
    theme: Res<Theme>,
    inventories: Query<&Inventory, With<PlayerShip>>,
    changed: Query<(), (With<PlayerShip>, Changed<Inventory>)>,
    mut commands: Commands,
    container: Query<Entity, With<SlotContainer>>,
    children: Query<&Children>,
) {
    // Rebuild on an inventory change or a build-mode active-state edge (tracked here so
    // selecting/rotating a module — other `BuildMode` mutations — doesn't rebuild).
    let active_changed = build.active != *prev_active;
    *prev_active = build.active;
    if !active_changed && changed.is_empty() {
        return;
    }
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
            // While building, show only the items usable there.
            if build.active && !stack.kind.build_relevant() {
                continue;
            }
            // A module previews its build color; a component is accented; trade is neutral.
            let swatch = match stack.kind {
                ItemKind::Module(kind) => registry.module(kind).color,
                ItemKind::Component => theme.palette.accent,
                ItemKind::Trade => theme.palette.text_dim,
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
                // The slot's children are `Pickable::IGNORE` so the slot itself is the
                // drag target (otherwise a child swatch/label under the cursor would be).
                // Color swatch (a stand-in for the item icon to come).
                slot.spawn((
                    Node {
                        width: Val::Px(26.),
                        height: Val::Px(26.),
                        border_radius: BorderRadius::all(Val::Px(4.)),
                        ..default()
                    },
                    BackgroundColor(swatch),
                    Pickable::IGNORE,
                ));
                slot.spawn((ui::column(theme.space.xs), Pickable::IGNORE))
                    .with_children(|info| {
                        info.spawn((ui::label(&theme, stack.name.clone()), Pickable::IGNORE));
                        info.spawn((
                            ui::small(&theme, format!("×{}", stack.count)),
                            Pickable::IGNORE,
                        ));
                    });
            });
        }
    });
}

// ---------------------------------------------------------------------------
// Drag a module from the inventory onto the ship (build mode)
// ---------------------------------------------------------------------------

/// Start dragging a module item out of the inventory. While in build mode, dragging a
/// *module* slot selects that module and shows the build ghost (which then follows the
/// cursor via build mode's own systems). Component items aren't placeable yet, so
/// dragging them does nothing; outside build mode nothing happens either.
fn on_slot_drag_start(
    mut event: On<Pointer<DragStart>>,
    slots: Query<&InventorySlot>,
    inventories: Query<&Inventory, With<PlayerShip>>,
    registry: Res<ModuleRegistry>,
    theme: Res<Theme>,
    mut build: ResMut<BuildMode>,
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<ColorMaterial>>,
) {
    let Ok(slot) = slots.get(event.entity) else {
        return;
    };
    // Pointer events bubble up the UI tree, so this global observer would otherwise re-fire
    // for each ancestor (spawning duplicate chips); handle the slot once and stop here.
    event.propagate(false);
    if !build.active {
        return;
    }
    let Ok(inventory) = inventories.single() else {
        return;
    };
    let Some(stack) = inventory.items.get(slot.index) else {
        return;
    };
    let kind = match stack.kind {
        ItemKind::Module(kind) => kind,
        ItemKind::Component | ItemKind::Trade => return,
    };
    if stack.count == 0 {
        return;
    }
    begin_module_drag(
        kind,
        &mut build,
        &registry,
        &mut commands,
        &mut meshes,
        &mut materials,
    );
    // A cursor-following chip (on Z_DRAG, above the panel) so the dragged module is visible
    // over the inventory; `on_slot_drag` moves it, `on_slot_drag_end` despawns it.
    let pos = event.pointer_location.position;
    commands.spawn((
        DragChip,
        Node {
            position_type: PositionType::Absolute,
            left: Val::Px(pos.x + 14.),
            top: Val::Px(pos.y + 14.),
            width: Val::Px(30.),
            height: Val::Px(30.),
            border: UiRect::all(Val::Px(2.)),
            border_radius: BorderRadius::all(Val::Px(6.)),
            ..default()
        },
        BackgroundColor(registry.module(kind).color),
        BorderColor::all(theme.palette.text),
        GlobalZIndex(ui::Z_DRAG),
        Pickable::IGNORE,
    ));
}

/// Keep the drag chip under the cursor as the pointer moves.
fn on_slot_drag(
    mut event: On<Pointer<Drag>>,
    slots: Query<&InventorySlot>,
    mut chips: Query<&mut Node, With<DragChip>>,
) {
    if !slots.contains(event.entity) {
        return;
    }
    event.propagate(false);
    let pos = event.pointer_location.position;
    for mut node in &mut chips {
        node.left = Val::Px(pos.x + 14.);
        node.top = Val::Px(pos.y + 14.);
    }
}

/// Finish dragging a module item: drop it onto the ship. If it landed on a valid attach
/// point, build mode places it and we consume one from the stack; otherwise the drag is
/// cancelled (selection cleared, nothing consumed). Reuses the build-mode placement path
/// (`drop_module`), so the dragged module obeys the same snapping/blocking rules as
/// click-to-build.
fn on_slot_drag_end(
    mut event: On<Pointer<DragEnd>>,
    over_ui: Res<PointerOverUi>,
    slots: Query<&InventorySlot>,
    chips: Query<Entity, With<DragChip>>,
    mut inventories: Query<&mut Inventory, With<PlayerShip>>,
    registry: Res<ModuleRegistry>,
    mut build: ResMut<BuildMode>,
    windows: Query<&Window>,
    cameras: Query<(&Camera, &GlobalTransform), With<Camera2d>>,
    mut points: Query<(Entity, &mut AttachPoint, &GlobalTransform)>,
    bodies: Query<&GlobalTransform>,
    modules: Query<(Entity, &BuiltModule, &GlobalTransform)>,
    parents: Query<&ChildOf>,
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<ColorMaterial>>,
) {
    let Ok(slot) = slots.get(event.entity) else {
        return;
    };
    // Handle the slot once — see `on_slot_drag_start`. (The bubbling re-fires were what
    // double-despawned the chip below.)
    event.propagate(false);
    let index = slot.index;
    // The drag is over: drop the cursor chip regardless of what happens next.
    for chip in &chips {
        commands.entity(chip).despawn();
    }
    if !build.active {
        return;
    }
    let placed = drop_module(
        over_ui.0,
        &mut build,
        &registry,
        &windows,
        &cameras,
        &mut points,
        &bodies,
        &modules,
        &parents,
        &mut commands,
        &mut meshes,
        &mut materials,
    );
    if !placed {
        return;
    }
    // Consume one of the placed module from the ship's inventory (the model mutation
    // triggers `rebuild_inventory_ui`).
    let Ok(mut inventory) = inventories.single_mut() else {
        return;
    };
    if let Some(stack) = inventory.items.get_mut(index) {
        stack.count = stack.count.saturating_sub(1);
        if stack.count == 0 {
            inventory.items.remove(index);
        }
    }
}
