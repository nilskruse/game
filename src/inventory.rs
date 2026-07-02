//! The player inventory: a data model ([`Inventory`]) plus a window that *views* it,
//! built on the `ui` toolkit. Toggle it with the **I** key or the on-screen button; it
//! opens as a panel down the left side.
//!
//! Deliberately a view-over-model: the window holds no item state, only renders
//! [`Inventory::items`], and rebuilds when the model changes ([`rebuild_inventory_ui`]).
//! Each slot entity carries an [`InventorySlot`] (its container — the inventory-owning
//! structure root — plus its index into that container's `items`), which the
//! pointer observers use: **drag a module slot onto the ship in build mode to place it**
//! (`on_slot_drag_*` → `build::drop_module`, consuming one from the stack), and **hover a
//! slot to show its stats** in a side panel (`on_slot_hover_*` → [`StatFocus`] →
//! [`update_stat_window`], stats from [`item_stats`]). Mutating the model rebuilds the
//! view, so save/load stays a matter of persisting `Inventory`, never the UI.

use bevy::ecs::system::SystemParam;
use bevy::picking::events::{Click, Drag, DragEnd, DragStart, Out, Over, Pointer};
use bevy::picking::Pickable;
use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use crate::build::{
    begin_module_drag, drop_module, install_turret, AttachPoint, BuildMode, BuiltModule,
    ModuleDeconstructed, ModuleDef, ModuleKind, ModuleRegistry,
};
use crate::health::{ModuleDisabled, ModuleHealth};
use crate::save::{InstanceId, PersistSet, SaveFile};
use crate::ship::turret::{FireArc, Turret, TurretKind};
use crate::ship::{PlayerShip, ShipBase, StructureRoot};
use crate::station::SpaceStation;
use crate::ui::{self, PointerOverUi, Theme};

pub struct InventoryPlugin;

impl Plugin for InventoryPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<StatFocus>()
            .init_resource::<PendingInventories>()
            .add_systems(Startup, spawn_inventory_ui)
            // Attach an inventory the moment a structure root appears, rather than polling
            // every frame.
            .add_observer(attach_inventory::<ShipBase>)
            .add_observer(attach_inventory::<SpaceStation>)
            // Drag a module out of the inventory onto the ship while in build mode.
            .add_observer(on_slot_drag_start)
            .add_observer(on_slot_drag)
            .add_observer(on_slot_drag_end)
            // Hover a slot to show its stats.
            .add_observer(on_slot_hover_start)
            .add_observer(on_slot_hover_end)
            // Refund a deconstructed module (+ its turret) back into inventory.
            .add_observer(refund_deconstructed)
            .add_systems(
                Update,
                (
                    // Seed before applying saved items, so a loaded inventory always
                    // overwrites the starter set (see `apply_pending_inventories`).
                    (seed_player_inventory, apply_pending_inventories).chain(),
                    toggle_inventory_hotkey,
                    sync_inventory_to_build_mode,
                    rebuild_inventory_ui,
                    hover_built_module,
                    update_stat_window,
                    highlight_turret_mounts,
                ),
            )
            // Persistence: inventories own their save chunk (see `save.rs`).
            .add_systems(
                Update,
                (
                    capture_inventories.in_set(PersistSet::Capture),
                    apply_inventories.in_set(PersistSet::Apply),
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
#[derive(Clone, Copy, Serialize, Deserialize)]
pub(crate) enum ItemKind {
    /// A buildable ship module, identified by its [`ModuleKind`]. Build-relevant.
    Module(ModuleKind),
    /// A weapon turret, installed by dragging it onto a placed turret mount (a
    /// [`ModuleKind::Turret`] module). Build-relevant.
    Turret(TurretKind, FireArc),
    /// A build material / part (placeholder until items have definitions). Build-relevant.
    Component,
    /// A general trade good / cargo (ore, supplies, loot). Not used in build mode.
    Trade,
}

impl ItemKind {
    /// Whether this item belongs in build mode — a module/turret to install or a build
    /// material. General trade goods are filtered out of the inventory while building.
    fn build_relevant(&self) -> bool {
        matches!(
            self,
            ItemKind::Module(_) | ItemKind::Turret(..) | ItemKind::Component
        )
    }
}

/// One displayed stat line (a label and its value). The stat window renders a list of
/// these.
struct Stat {
    label: &'static str,
    value: String,
}

/// The stats shown for an item. **This is the single, extensible place to add stats** —
/// add a `Stat` line here (for modules, reading from its `ModuleDef`). Kept data-driven so
/// new module data (or item definitions, once they exist) surface here with one line each.
fn item_stats(stack: &ItemStack, registry: &ModuleRegistry) -> Vec<Stat> {
    let mut stats = Vec::new();
    match stack.kind {
        ItemKind::Module(kind) => {
            let def = registry.module(kind);
            stats.push(Stat {
                label: "Type",
                value: format!("Module - {}", module_role(def)),
            });
            stats.push(Stat {
                label: "Size",
                value: format!("{}x{}", def.footprint.width, def.footprint.depth),
            });
            stats.push(Stat {
                label: "Hull",
                value: format!("{:.0}", def.durability.0),
            });
            stats.push(Stat {
                label: "Armor",
                value: format!("{:.0}", def.durability.1),
            });
            if let Some(thrust) = def.thrust {
                stats.push(Stat {
                    label: "Thrust",
                    value: format!("{:.0}", thrust.strength),
                });
            }
            if def.mounts_turret {
                stats.push(Stat {
                    label: "Mount",
                    value: "Turret".to_string(),
                });
            }
        }
        ItemKind::Turret(kind, arc) => {
            stats.push(Stat {
                label: "Type",
                value: format!("Turret - {}", kind.name()),
            });
            stats.push(Stat {
                label: "Arc",
                value: arc.name().to_string(),
            });
        }
        ItemKind::Component => stats.push(Stat {
            label: "Type",
            value: "Component".to_string(),
        }),
        ItemKind::Trade => stats.push(Stat {
            label: "Type",
            value: "Trade good".to_string(),
        }),
    }
    stats.push(Stat {
        label: "Count",
        value: stack.count.to_string(),
    });
    stats
}

/// Short role label for a module definition, shared by the inventory item stats and the
/// build-mode hover stats.
fn module_role(def: &ModuleDef) -> &'static str {
    if def.is_dock() {
        "Docking port"
    } else if def.has_seat() {
        "Cockpit"
    } else if def.walkable() {
        "Room"
    } else {
        "Solid"
    }
}

/// Stats for a *placed* module (hovered in build mode): like the item stats, but the hull
/// shows the live `current / max` from its [`ModuleHealth`], the mount notes whether it's
/// armed, and a status line reflects [`ModuleDisabled`]. The same extensible-list idea as
/// [`item_stats`].
fn module_stats(
    kind: ModuleKind,
    health: Option<&ModuleHealth>,
    disabled: bool,
    has_turret: bool,
    registry: &ModuleRegistry,
) -> Vec<Stat> {
    let def = registry.module(kind);
    let mut stats = vec![
        Stat {
            label: "Type",
            value: format!("Module - {}", module_role(def)),
        },
        Stat {
            label: "Size",
            value: format!("{}x{}", def.footprint.width, def.footprint.depth),
        },
    ];
    if let Some(h) = health {
        stats.push(Stat {
            label: "Hull",
            value: format!("{:.0} / {:.0}", h.current.max(0.), h.max),
        });
        stats.push(Stat {
            label: "Armor",
            value: format!("{:.0}", h.armor),
        });
    } else {
        stats.push(Stat {
            label: "Hull",
            value: format!("{:.0}", def.durability.0),
        });
        stats.push(Stat {
            label: "Armor",
            value: format!("{:.0}", def.durability.1),
        });
    }
    if let Some(thrust) = def.thrust {
        stats.push(Stat {
            label: "Thrust",
            value: format!("{:.0}", thrust.strength),
        });
    }
    if def.mounts_turret {
        stats.push(Stat {
            label: "Mount",
            value: if has_turret {
                "Turret (armed)".to_string()
            } else {
                "Turret (empty)".to_string()
            },
        });
    }
    stats.push(Stat {
        label: "Status",
        value: if disabled {
            "Disabled".to_string()
        } else {
            "Operational".to_string()
        },
    });
    stats
}

/// Display name for a turret item of `kind` — derived in exactly one place so every
/// site that creates a turret stack (seeding, refunds) produces the same name, and
/// stacking (which compares kind + name, see [`add_item`]) always merges identical
/// turrets. Module item names are single-sourced the same way, through the registry.
fn turret_item_name(kind: TurretKind) -> String {
    format!("{} Turret", kind.name())
}

/// The slot swatch / drag-chip color for an item (module uses its build color).
fn item_swatch(stack: &ItemStack, registry: &ModuleRegistry, theme: &Theme) -> Color {
    match stack.kind {
        ItemKind::Module(kind) => registry.module(kind).color,
        ItemKind::Turret(..) => Color::srgb(0.75, 0.7, 0.5),
        ItemKind::Component => theme.palette.accent,
        ItemKind::Trade => theme.palette.text_dim,
    }
}

/// What the stat window shows. An inventory slot wins (the one being **dragged** if any,
/// else the one **hovered**, via [`Self::target`]); failing that, a built **module** hovered
/// in the world while in build mode (inspect/deconstruct). Slots are addressed as
/// `(container, index)` — the inventory-owning structure root plus the stack index —
/// so the focus works for any container a window might show, not just the player ship.
#[derive(Resource, Default)]
struct StatFocus {
    hovered: Option<(Entity, usize)>,
    dragged: Option<(Entity, usize)>,
    module: Option<Entity>,
}

impl StatFocus {
    fn target(&self) -> Option<(Entity, usize)> {
        self.dragged.or(self.hovered)
    }
}

/// One stack of identical items in the inventory. For modules and turrets the `name`
/// is *derived* from the kind (registry name / [`turret_item_name`]) — never hand-write
/// it, or otherwise-identical stacks stop merging. Component/Trade placeholders carry
/// bespoke names until a real item registry exists.
#[derive(Clone, Serialize, Deserialize)]
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
/// registry so they track `module_defs()`. On a load this runs *before*
/// [`apply_pending_inventories`] (chained in the plugin), which overwrites the seed
/// with the saved items — so the seed only survives for genuinely new ships.
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
    // Turrets — installed by dragging onto a placed turret mount.
    for (kind, arc, count) in [
        (TurretKind::Cannon, FireArc::OverShip, 2),
        (TurretKind::PointDefense, FireArc::OverShip, 1),
    ] {
        inventory.items.push(ItemStack {
            kind: ItemKind::Turret(kind, arc),
            name: turret_item_name(kind),
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

/// Refund a deconstructed module — and its turret weapon, if it was an armed mount — to the
/// ship's [`Inventory`] (the [`ModuleDeconstructed::ship`], i.e. the player ship). The model
/// mutation triggers `rebuild_inventory_ui`, so the item reappears in the window.
fn refund_deconstructed(
    event: On<ModuleDeconstructed>,
    registry: Res<ModuleRegistry>,
    mut inventories: Query<&mut Inventory>,
) {
    let Ok(mut inventory) = inventories.get_mut(event.ship) else {
        return;
    };
    add_item(
        &mut inventory,
        ItemKind::Module(event.kind),
        registry.module(event.kind).name.to_string(),
    );
    if let Some((kind, arc)) = event.turret {
        add_item(
            &mut inventory,
            ItemKind::Turret(kind, arc),
            turret_item_name(kind),
        );
    }
}

/// Add one of an item to `inventory`, stacking onto an identical existing stack (same kind
/// and name) or pushing a new one. The model the inventory drag/refund flows mutate.
fn add_item(inventory: &mut Inventory, kind: ItemKind, name: String) {
    if let Some(stack) = inventory
        .items
        .iter_mut()
        .find(|s| same_item(s.kind, kind) && s.name == name)
    {
        stack.count += 1;
    } else {
        inventory.items.push(ItemStack {
            kind,
            name,
            count: 1,
        });
    }
}

/// Whether two item kinds are the same for stacking (`ItemKind` isn't `PartialEq` since it
/// wraps several enums). Component/Trade match on variant alone — callers also compare the
/// name (see [`add_item`]) to keep distinct goods apart.
fn same_item(a: ItemKind, b: ItemKind) -> bool {
    match (a, b) {
        (ItemKind::Module(x), ItemKind::Module(y)) => x == y,
        (ItemKind::Turret(k1, a1), ItemKind::Turret(k2, a2)) => k1 == k2 && a1 == a2,
        (ItemKind::Component, ItemKind::Component) => true,
        (ItemKind::Trade, ItemKind::Trade) => true,
        _ => false,
    }
}

// ---------------------------------------------------------------------------
// Persistence (the "inventories" save chunk)
// ---------------------------------------------------------------------------

/// The inventories chunk: every structure's items, keyed by its `InstanceId` (the
/// stable cross-save id — a live `Entity` wouldn't survive the load rebuild).
#[derive(Serialize, Deserialize, Default)]
struct InventoriesSave {
    inventories: Vec<(u64, Vec<ItemStack>)>,
}

/// Saved inventories waiting to be applied. Load rebuilds structures via deferred
/// commands, so the chunk can't be applied to them directly in `PersistSet::Apply`;
/// [`apply_pending_inventories`] retries matching by `InstanceId` each frame until
/// every entry lands (the `PendingPilot` pattern from `player.rs`).
#[derive(Resource, Default)]
pub(crate) struct PendingInventories(pub(crate) Vec<(u64, Vec<ItemStack>)>);

/// Capture every structure's inventory into the chunk (runs in `PersistSet::Capture`).
fn capture_inventories(
    structures: Query<(&InstanceId, &Inventory)>,
    mut file: ResMut<SaveFile>,
) {
    let inventories = structures
        .iter()
        .map(|(id, inventory)| (id.0, inventory.items.clone()))
        .collect();
    file.write("inventories", &InventoriesSave { inventories });
}

/// Read the inventories chunk into [`PendingInventories`] (runs in `PersistSet::Apply`).
/// A save without the chunk simply leaves nothing pending (the seed stands).
fn apply_inventories(file: Res<SaveFile>, mut pending: ResMut<PendingInventories>) {
    pending.0 = file
        .read::<InventoriesSave>("inventories")
        .map(|chunk| chunk.inventories)
        .unwrap_or_default();
}

/// Apply pending saved inventories to structures as they (re)appear, matched by
/// `InstanceId`. Overwrites whatever the structure spawned with — including the player
/// ship's starter seed, which is why this is chained after `seed_player_inventory`.
fn apply_pending_inventories(
    mut pending: ResMut<PendingInventories>,
    mut structures: Query<(&InstanceId, &mut Inventory)>,
) {
    if pending.0.is_empty() {
        return;
    }
    pending.0.retain(|(want, items)| {
        for (id, mut inventory) in structures.iter_mut() {
            if id.0 == *want {
                inventory.items = items.clone();
                return false; // applied — drop the entry
            }
        }
        true // structure not rebuilt yet — retry next frame
    });
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

/// A single item slot: the **container** (the structure root whose [`Inventory`] the
/// window shows) plus the model index of the stack it displays. The pointer observers
/// resolve the inventory through `container` rather than assuming the player ship, so
/// a second window over another structure's inventory (trading, station cargo) reuses
/// the same slots and handlers.
#[derive(Component)]
pub(crate) struct InventorySlot {
    pub container: Entity,
    pub index: usize,
}

/// A small chip that follows the cursor while dragging an item out of the inventory, drawn
/// on the `Z_DRAG` layer so it sits *above* the inventory panel (the in-world build ghost
/// is behind the UI). It shows what's being dragged; the footprint ghost still previews
/// placement on the ship.
#[derive(Component)]
struct DragChip;

/// Root of the stat window (a panel beside the inventory, on `Z_TOOLTIP`). Shown while a
/// slot is hovered or a module is dragged; its content is `StatContent`.
#[derive(Component)]
struct StatWindow;

/// The container the stat lines are (re)built into when the focused item changes.
#[derive(Component)]
struct StatContent;

/// A translucent overlay shown on an empty turret mount while a turret is being dragged,
/// marking it as a valid drop target.
#[derive(Component)]
struct TurretMountHighlight;

/// A translucent overlay on the built module currently hovered in build mode — the visual
/// indication of what the stat window is describing (and what a no-selection click would
/// deconstruct).
#[derive(Component)]
struct HoverHighlight;

/// Width (px) of the inventory window; the stat window sits just to its right.
const INVENTORY_WIDTH: f32 = 280.;

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
                width: Val::Px(INVENTORY_WIDTH),
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

    // The stat window: a panel just right of the inventory (translucent so it doesn't hide
    // the world), shown while a slot is hovered or a module dragged. `update_stat_window`
    // fills `StatContent`.
    commands
        .spawn((
            StatWindow,
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(theme.space.md * 2. + INVENTORY_WIDTH),
                top: Val::Px(theme.space.md),
                width: Val::Px(240.),
                padding: UiRect::all(Val::Px(theme.space.md)),
                border: UiRect::all(Val::Px(1.)),
                border_radius: BorderRadius::all(Val::Px(theme.radius)),
                flex_direction: FlexDirection::Column,
                row_gap: Val::Px(theme.space.xs),
                ..default()
            },
            BackgroundColor(theme.palette.surface.with_alpha(0.9)),
            BorderColor::all(theme.palette.border),
            GlobalZIndex(ui::Z_TOOLTIP),
            Visibility::Hidden,
        ))
        .with_children(|window| {
            window.spawn((StatContent, ui::column(theme.space.xs)));
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
    inventories: Query<(Entity, &Inventory), With<PlayerShip>>,
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
    let Ok((ship, inventory)) = inventories.single() else {
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
            let swatch = item_swatch(stack, &registry, &theme);
            list.spawn((
                InventorySlot {
                    container: ship,
                    index,
                },
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
                            ui::small(&theme, format!("x{}", stack.count)),
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
    inventories: Query<&Inventory>,
    registry: Res<ModuleRegistry>,
    theme: Res<Theme>,
    mut build: ResMut<BuildMode>,
    mut focus: ResMut<StatFocus>,
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
    let Ok(inventory) = inventories.get(slot.container) else {
        return;
    };
    let Some(stack) = inventory.items.get(slot.index) else {
        return;
    };
    if stack.count == 0 {
        return;
    }
    match stack.kind {
        // A module gets a placement ghost (snaps to attach points). A turret has no ghost —
        // it's dropped onto an existing mount. Other items aren't placeable.
        ItemKind::Module(kind) => begin_module_drag(
            kind,
            &mut build,
            &registry,
            &mut commands,
            &mut meshes,
            &mut materials,
        ),
        ItemKind::Turret(..) => {}
        ItemKind::Component | ItemKind::Trade => return,
    }
    // Show this item's stats for the duration of the drag.
    focus.dragged = Some((slot.container, slot.index));
    // A cursor-following chip (on Z_DRAG, above the panel) so the dragged item is visible
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
        BackgroundColor(item_swatch(stack, &registry, &theme)),
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

/// Commands + mesh/material assets bundled so the drag-end observer stays under Bevy's
/// system-param count limit.
#[derive(SystemParam)]
struct SpawnCtx<'w, 's> {
    commands: Commands<'w, 's>,
    meshes: ResMut<'w, Assets<Mesh>>,
    materials: ResMut<'w, Assets<ColorMaterial>>,
}

/// Finish dragging an item: a **module** is placed at the cursor (`build::drop_module`,
/// same snapping/blocking as click-to-build); a **turret** is installed into the empty
/// turret mount under the cursor (`build::install_turret`). On success one is consumed from
/// the stack; otherwise the drag is cancelled (nothing consumed).
fn on_slot_drag_end(
    mut event: On<Pointer<DragEnd>>,
    over_ui: Res<PointerOverUi>,
    slots: Query<&InventorySlot>,
    chips: Query<Entity, With<DragChip>>,
    mut inventories: Query<&mut Inventory>,
    registry: Res<ModuleRegistry>,
    mut build: ResMut<BuildMode>,
    mut focus: ResMut<StatFocus>,
    windows: Query<&Window>,
    cameras: Query<(&Camera, &GlobalTransform), With<Camera2d>>,
    mut points: Query<(Entity, &mut AttachPoint, &GlobalTransform, &StructureRoot)>,
    bodies: Query<&GlobalTransform>,
    modules: Query<(Entity, &BuiltModule, &GlobalTransform, &StructureRoot)>,
    children: Query<&Children>,
    turrets: Query<(), With<Turret>>,
    mut ctx: SpawnCtx,
) {
    // The drag is over: drop the cursor chip and the dragged-stat focus first, so they're
    // cleaned up even if the slot entity was despawned mid-drag (otherwise the early-return
    // below would leak a stuck chip / stat window). Guarded so a non-slot `DragEnd` doesn't
    // spuriously mark `StatFocus` changed.
    if focus.dragged.is_some() {
        focus.dragged = None;
    }
    for chip in &chips {
        ctx.commands.entity(chip).despawn();
    }
    let Ok(slot) = slots.get(event.entity) else {
        return;
    };
    // Handle the slot once — see `on_slot_drag_start`. (The bubbling re-fires were what
    // double-despawned the chip above.)
    event.propagate(false);
    let (container, index) = (slot.container, slot.index);
    if !build.active {
        return;
    }
    let item_kind = inventories
        .get(container)
        .ok()
        .and_then(|inv| inv.items.get(index).map(|stack| stack.kind));
    let Some(item_kind) = item_kind else {
        return;
    };
    let placed = match item_kind {
        ItemKind::Module(_) => drop_module(
            over_ui.0,
            &mut build,
            &registry,
            &windows,
            &cameras,
            &mut points,
            &bodies,
            &modules,
            &mut ctx.commands,
            &mut ctx.meshes,
            &mut ctx.materials,
        ),
        ItemKind::Turret(kind, arc) => install_turret(
            over_ui.0,
            kind,
            arc,
            &build,
            &windows,
            &cameras,
            &modules,
            &children,
            &turrets,
            &mut ctx.commands,
            &mut ctx.meshes,
            &mut ctx.materials,
        ),
        ItemKind::Component | ItemKind::Trade => false,
    };
    if !placed {
        return;
    }
    // Consume one of the installed item from the slot's container (the model mutation
    // triggers `rebuild_inventory_ui`).
    let Ok(mut inventory) = inventories.get_mut(container) else {
        return;
    };
    if let Some(stack) = inventory.items.get_mut(index) {
        stack.count = stack.count.saturating_sub(1);
        if stack.count == 0 {
            inventory.items.remove(index);
        }
    }
}

/// While a turret item is being dragged in build mode, show a green overlay on every empty
/// turret mount of the edited structure (the valid drop targets); clear them when the drag
/// ends. Acts on the drag edge (a `Local<bool>`) so it spawns/despawns once per drag, not
/// every frame.
fn highlight_turret_mounts(
    build: Res<BuildMode>,
    focus: Res<StatFocus>,
    inventories: Query<&Inventory>,
    mut was_dragging: Local<bool>,
    mounts: Query<(Entity, &BuiltModule, &StructureRoot)>,
    children: Query<&Children>,
    turrets: Query<(), With<Turret>>,
    highlights: Query<Entity, With<TurretMountHighlight>>,
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<ColorMaterial>>,
) {
    let dragging_turret = build.active
        && focus
            .dragged
            .and_then(|(container, index)| {
                inventories
                    .get(container)
                    .ok()
                    .and_then(|inv| inv.items.get(index))
            })
            .is_some_and(|stack| matches!(stack.kind, ItemKind::Turret(..)));
    if dragging_turret == *was_dragging {
        return;
    }
    *was_dragging = dragging_turret;
    if !dragging_turret {
        for highlight in &highlights {
            commands.entity(highlight).despawn();
        }
        return;
    }
    let Some(structure) = build.structure() else {
        return;
    };
    for (entity, module, root) in &mounts {
        if module.kind != ModuleKind::Turret || root.0 != structure {
            continue;
        }
        let armed = children
            .get(entity)
            .is_ok_and(|kids| kids.iter().any(|child| turrets.contains(child)));
        if armed {
            continue;
        }
        commands.entity(entity).with_children(|mount| {
            mount.spawn((
                TurretMountHighlight,
                Mesh2d(meshes.add(Rectangle::new(module.size.x, module.size.y))),
                MeshMaterial2d(materials.add(Color::srgba(0.3, 1.0, 0.4, 0.35))),
                Transform::from_xyz(0., 0., 0.5),
            ));
        });
    }
}

// ---------------------------------------------------------------------------
// Build-mode hover: inspect a built module
// ---------------------------------------------------------------------------

/// While in build mode (and not placing/dragging anything), track the built module under
/// the cursor on the edited structure: focus its stats ([`StatFocus::module`] → the stat
/// window) and overlay a translucent [`HoverHighlight`] on it so it reads as selected. The
/// overlay is re-placed only when the hovered module changes, not every frame.
fn hover_built_module(
    build: Res<BuildMode>,
    over_ui: Res<PointerOverUi>,
    mut focus: ResMut<StatFocus>,
    windows: Query<&Window>,
    cameras: Query<(&Camera, &GlobalTransform), With<Camera2d>>,
    modules: Query<(Entity, &BuiltModule, &GlobalTransform, &StructureRoot)>,
    highlights: Query<Entity, With<HoverHighlight>>,
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<ColorMaterial>>,
) {
    // Inspect only while building, with nothing being placed/dragged and the cursor over the
    // world (not the inventory UI).
    let inspecting = build.active && !build.is_placing() && focus.dragged.is_none() && !over_ui.0;
    let hit = if inspecting {
        build.structure().and_then(|structure| {
            cursor_world(&windows, &cameras)
                .and_then(|cursor| module_under_cursor(cursor, structure, &modules))
        })
    } else {
        None
    };
    if hit == focus.module {
        return;
    }
    focus.module = hit;
    // The hovered module changed — drop the old overlay and place one on the new module.
    for highlight in &highlights {
        commands.entity(highlight).despawn();
    }
    let Some(entity) = hit else {
        return;
    };
    let Ok((_, module, _, _)) = modules.get(entity) else {
        return;
    };
    let size = module.size;
    commands.entity(entity).with_children(|m| {
        m.spawn((
            HoverHighlight,
            Mesh2d(meshes.add(Rectangle::new(size.x, size.y))),
            MeshMaterial2d(materials.add(Color::srgba(0.6, 0.9, 1.0, 0.22))),
            Transform::from_xyz(0., 0., 0.6),
        ));
    });
}

/// The built module on `structure` whose footprint is under `cursor` (nearest center wins),
/// mirroring `build`'s deconstruct pick. Modules sit axis-aligned in the structure frame, so
/// `BuiltModule.size` is the local hit box.
fn module_under_cursor(
    cursor: Vec2,
    structure: Entity,
    modules: &Query<(Entity, &BuiltModule, &GlobalTransform, &StructureRoot)>,
) -> Option<Entity> {
    let mut hit: Option<(Entity, f32)> = None;
    for (entity, module, gt, root) in modules {
        if root.0 != structure {
            continue;
        }
        let local = gt.affine().inverse().transform_point3(cursor.extend(0.));
        let h = module.size / 2.;
        if local.x.abs() <= h.x && local.y.abs() <= h.y {
            let d = local.truncate().length();
            if hit.is_none_or(|(_, best)| d < best) {
                hit = Some((entity, d));
            }
        }
    }
    hit.map(|(entity, _)| entity)
}

/// Cursor position in world space, or `None` if it's off-window.
fn cursor_world(
    windows: &Query<&Window>,
    cameras: &Query<(&Camera, &GlobalTransform), With<Camera2d>>,
) -> Option<Vec2> {
    let window = windows.iter().next()?;
    let cursor = window.cursor_position()?;
    let (camera, cam_tf) = cameras.iter().next()?;
    camera.viewport_to_world_2d(cam_tf, cursor).ok()
}

// ---------------------------------------------------------------------------
// Hover → stat window
// ---------------------------------------------------------------------------

/// Hovering a slot focuses its stats. (Propagation stopped — see `on_slot_drag_start`.)
fn on_slot_hover_start(
    mut event: On<Pointer<Over>>,
    slots: Query<&InventorySlot>,
    mut focus: ResMut<StatFocus>,
) {
    let Ok(slot) = slots.get(event.entity) else {
        return;
    };
    event.propagate(false);
    if focus.hovered != Some((slot.container, slot.index)) {
        focus.hovered = Some((slot.container, slot.index));
    }
}

/// Leaving a slot clears its hover focus (a drag still keeps its own focus).
fn on_slot_hover_end(
    mut event: On<Pointer<Out>>,
    slots: Query<&InventorySlot>,
    mut focus: ResMut<StatFocus>,
) {
    let Ok(slot) = slots.get(event.entity) else {
        return;
    };
    event.propagate(false);
    if focus.hovered == Some((slot.container, slot.index)) {
        focus.hovered = None;
    }
}

/// Show/hide and (re)fill the stat window for the focused thing: an inventory item
/// (`StatFocus::target`) if any, else a built module hovered in the world
/// (`StatFocus::module`). Runs when the focus changes — or when the focused module's health
/// changes, so a damaged module's hull stays live — then clears `StatContent` and respawns a
/// heading + one row per `Stat`.
fn update_stat_window(
    focus: Res<StatFocus>,
    inventories: Query<&Inventory>,
    registry: Res<ModuleRegistry>,
    theme: Res<Theme>,
    built: Query<(
        &BuiltModule,
        Option<&ModuleHealth>,
        Has<ModuleDisabled>,
        Option<&Children>,
    )>,
    turrets: Query<(), With<Turret>>,
    health_changed: Query<(), Changed<ModuleHealth>>,
    mut windows: Query<&mut Visibility, With<StatWindow>>,
    content: Query<Entity, With<StatContent>>,
    children: Query<&Children>,
    mut commands: Commands,
) {
    // Refresh on a focus change, or when the focused module takes damage (live hull).
    let module_dirty = focus.module.is_some_and(|m| health_changed.contains(m));
    if !focus.is_changed() && !module_dirty {
        return;
    }
    let Ok(mut visibility) = windows.single_mut() else {
        return;
    };
    let Ok(content) = content.single() else {
        return;
    };
    // Clear the previous content.
    if let Ok(existing) = children.get(content) {
        for &child in existing {
            commands.entity(child).despawn();
        }
    }

    // An inventory item wins; otherwise show the hovered built module's stats + live health.
    let item = focus.target().and_then(|(container, index)| {
        inventories
            .get(container)
            .ok()
            .and_then(|inv| inv.items.get(index))
    });
    let display: Option<(String, Vec<Stat>)> = if let Some(stack) = item {
        Some((stack.name.clone(), item_stats(stack, &registry)))
    } else if let Some((module, health, disabled, kids)) =
        focus.module.and_then(|m| built.get(m).ok())
    {
        let has_turret = kids.is_some_and(|kids| kids.iter().any(|c| turrets.contains(c)));
        let name = registry.module(module.kind).name.to_string();
        Some((
            name,
            module_stats(module.kind, health, disabled, has_turret, &registry),
        ))
    } else {
        None
    };

    let Some((name, stats)) = display else {
        *visibility = Visibility::Hidden;
        return;
    };
    *visibility = Visibility::Visible;

    commands.entity(content).with_children(|panel| {
        panel.spawn(ui::heading(&theme, name));
        for stat in stats {
            panel
                .spawn(Node {
                    flex_direction: FlexDirection::Row,
                    // Vertically center the (smaller) label against the (larger) value, and
                    // give the label a fixed width so all values line up in a column.
                    align_items: AlignItems::Center,
                    column_gap: Val::Px(theme.space.sm),
                    width: Val::Percent(100.),
                    ..default()
                })
                .with_children(|line| {
                    line.spawn((
                        ui::small(&theme, stat.label),
                        Node {
                            width: Val::Px(76.),
                            ..default()
                        },
                    ));
                    line.spawn(ui::label(&theme, stat.value));
                });
        }
    });
}
