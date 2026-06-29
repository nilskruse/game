# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project

A 2D space game in Rust built on **Bevy 0.19** (ECS) and **avian2d 0.7** (physics). You fly a modular ship, walk around inside it while it moves, dock with stations, and build/deconstruct ship modules at runtime.

## Commands

```bash
cargo run          # build and launch the game
cargo build        # compile only
cargo check        # fast type-check (preferred for quick iteration)
cargo clippy       # lint — the project is kept warning-clean
cargo fmt          # format
```

There is **no test suite** — this is a real-time game validated by running it. After changes, prefer `cargo check`/`cargo clippy` to confirm it compiles cleanly rather than assuming.

`clippy::type_complexity` and `clippy::too_many_arguments` are allowed crate-wide (in `main.rs`) because they fire on nearly every Bevy system/spawn helper; keep the rest warning-free.

## Controls (for manual testing)

- **On foot**: WASD / arrows to walk.
- **Piloting** (seated at a cockpit's pilot seat): **W/S** forward/reverse, **A/D** rotate, **Q/E** strafe. Thrust is gated by available thrusters (see thruster system). **Mouse aims** the ship's player-controlled cannons and **held left-click** fires them.
- **F** — interact: sit/stand at a pilot seat, use a console, or open build mode at an engineering console.
- **G** — dock / undock (must be seated at the helm, lined up with another port).
- **Build mode** — entered with **F** at an engineering console; **1–8** select module (Cargo/Engine/Sensor/Turret/Dock/Hallway/Cockpit/Thruster); with **Turret** selected, **T** cycles the turret kind (Cannon / Point-defense) and **Y** cycles the firing arc (Over-ship / Hull); **R** rotates the ghost's facing; left-click places, or (with nothing selected) deconstructs the module under the cursor; **B**/**Esc** exits.
- **F1** — debug: toggle player-ship invincibility (the `Invincible` resource; checked in the bullet hit path).
- **F5 / F9** — save / load (see Persistence). Mouse wheel zooms (smooth).

## Architecture

Bevy ECS: behavior lives in **systems** registered in `main.rs` (the `Game` plugin) and in per-feature plugins (`BuildPlugin`, `WorldPlugin`, `BackgroundPlugin`). Structures are built as **entity hierarchies** via `ChildOf`; only the root (ship hull / station root) is a `RigidBody`, and child colliders form one compound body.

### Collision layers (`GameLayer` in `src/ship/mod.rs`) — read this first
A 3-layer model whose subtleties cause most physics bugs. **Critical trap:** any collider spawned *without* an explicit `CollisionLayers` lands on `Default` (the first variant) with a filter of *all* layers — so it collides with everything. This is load-bearing (hulls/structural colliders rely on it) but means a forgotten tag silently turns decoration into a wall.

- `Default` — structural bodies (hulls, module/room structural colliders). Block other structures; also the implicit membership of untagged colliders.
- `Walls` — interior walls. Block the **player** but **not each other**, so docked structures' walls can overlap without the solver fighting.
- `Player` — the walking player; filtered to collide only with `Walls`.

### Module system (`src/build/`) — the core mechanic
One unified path builds **all** modules — player ship, station, and enemy ship all go through it. Do not add a parallel module system.

- `kinds.rs` — `ModuleKind` enum (just the stable **id** of a module type) + `Footprint { width, depth }` (in `UNIT`=50 cells). `width` runs along the attaching edge; `depth` extends outward.
- `registry.rs` — **the content registry (data-driven content).** `ModuleKind` carries *no data*; all per-kind data lives in a `ModuleDef` (name, footprint, color, durability `(hp, armor)`, `archetype`, optional `thrust`, `mounts_turret`), looked up by id through the `ModuleRegistry` resource (built at app start from the `module_defs()` table). **Definition vs instance:** a placed module is the instance (entity with `BuiltModule` + `ModuleHealth`); the `ModuleDef` is its definition. `ModuleArchetype` (`Room { seat } | Dock | Solid`) is the behavior category that drives which spawn routine runs; flags like `walkable`/`opens_doorway`/`has_seat`/`is_dock` are derived from it. To add or retune a module, edit `module_defs()` — nothing else. The registry is threaded as `&ModuleRegistry` through the spawn path (`mount`, the `spawn_*_base`/station/enemy builders, `build_structure`, `spawn_authored`) and as `Res<ModuleRegistry>` into the build-mode systems. (Later it can be populated from asset files without touching consumers — they all go through `registry.module(kind)`.) Turret stats are **not** registry-driven yet (still hardcoded in `ship/turret.rs`); that's the obvious next step.
- `attach.rs` — `build_buildable_side(...)` creates a row of doorway slots (removable panels + `AttachPoint`s) and wall segments for one side of a body. Returns `AttachSlot`s for pre-mounting.
- `spawn.rs` — `mount(...)` (takes `&ModuleRegistry`) occupies slots and spawns a module, returning `Mounted { module, sides }` so further modules chain onto exposed sides (`mount_far` pattern in `station.rs`). `mount_preplaced_turret`/`mount_preplaced_dock` are construction-time shortcuts. Walkable rooms expose buildable sides; solid/dock modules expose none. Internal spawn helpers take a `&ModuleDef` rather than a `ModuleKind`.
- `mode.rs` — interactive build mode: ghost preview, manual facing via `R`, snapping. Snapping measures from the module's **connecting edge** (offset by half its depth), not the cursor center. `BuiltModule` records occupied points + opened panels so deconstruction can re-seal.

To assemble a structure: create a root body with a `Collider`, call `build_buildable_side` per side, then `mount`/`mount_far` (passing the `&ModuleRegistry`). See `ship/mod.rs::spawn_player_ship_base` and `station.rs::spawn_space_station`.

### Docking (`src/docking.rs`)
`DockingPort` faces along its local **+Y**. `toggle_dock` (Update, so the `G` `just_pressed` edge isn't missed) snapshots every port's world pose, resolves each port's structure via its `StructureRoot` (see below), then finds the nearest aligned free pair (`ports_dockable`) and computes the docked pose with affine math. A ship may carry several ports; all are considered. Airlock doors/structural colliders (`AirlockDoor`) are disabled while docked so two airlocks meet without ejection.

Docking is animated, not instant:
- On dock, the ports latch immediately (opening airlocks), the ship is switched to `RigidBody::Kinematic` and tagged `Docking { target_pos, target_rot }`; `advance_docking` (FixedUpdate) eases it in with an exponential slide, then swaps `Docking` → `Docked`. Kinematic-during-slide keeps the physics solver from fighting the approaching hull. Steering is suppressed during the slide via `Without<Docking>` on the `movement` ship queries.
- On undock, the ship returns to `RigidBody::Dynamic` and gets a gentle pushoff velocity (`PUSHOFF_SPEED`) straight out of its port; auto-brake then arrests it.
- `update_dock_indicators` (Update) recolors each port collar green when a free aligned partner is in range (or it's already engaged), idle orange otherwise — purely geometric, independent of who's seated. Each port owns its material (so it recolors individually, like thruster nozzles).

### Player-on-ship carry (`src/player.rs`) — subtle, don't refactor blindly
The player is a separate dynamic body that must move with the ship. The working approach (others were tried and abandoned — see the note in `main.rs`):
- `read_player_input` → `drive_player_on_ship` (FixedUpdate): set the player's carry velocity *before* the physics step so the solver carries it and walls block it.
- `correct_player_carry` (FixedPostUpdate, between `StepSimulation` and `Writeback`): reconcile against the ship's *actual* post-solve motion using `CarryState`. When seated, hard-anchor to the `PilotSeat`.

### Hierarchy propagation (factions & structure roots)
Two components propagate down each structure's hierarchy via `HierarchyPropagatePlugin::<T>` (both registered in `main.rs`); set them on the root as `Propagate(T(..))` and every descendant — including modules built at runtime — inherits them one frame later.
- `InFaction(Faction)` (`faction.rs`): a turret mounted on a ship inherits the root's faction automatically — that's how the same turret module serves both player and enemy ships. Turret targeting (`ship/turret.rs`) uses it to pick enemies.
- `StructureRoot(Entity)` (`ship/mod.rs`): the ship-hull / station-root entity a part belongs to. Read it for O(1) structure membership instead of walking `ChildOf` and scanning every part each tick — used by the thrust solver, exhaust-blocking, and docking. **Because it lands a frame late, a just-built part is invisible to those systems for one frame (negligible).** Don't reintroduce a `root_of` ChildOf-walk in hot loops.

### Ship flight (`src/movement.rs`) — faction-agnostic
Flight is split so any ship (player or AI) flies the same way: a *controller* sets the ship's `ThrustControl` intent (-1/0/1 per axis) and adds the `Piloted` marker; the shared `drive_ships` solver turns intent + thrusters into motion for every `ShipBase`. The two controllers: `control_player_ship` (keyboard → the player ship's `ThrustControl`) and `fly_enemy_ships` (`enemy.rs`, the AI). A `Piloted` ship auto-brakes toward rest (gated by opposing thrust); a non-`Piloted` ship coasts. `ThrustCommand` is the *effective* per-frame thrust (intent + auto-brake) the solver writes for nozzle visuals. All ships are excluded from `apply_movement_damping` — braking is thruster-gated, not free drag.

### Enemy AI (`src/enemy.rs`)
`ShipAi { engage_range }` marks an AI-flown ship. `fly_enemy_ships` (in the fixed pre-loop, before the controllers) rotates the ship to point its nose (+Y) at the target and thrusts forward to hold `engage_range`, setting `ThrustControl` + `Piloted` so `drive_ships` flies it. The turret aims/fires on its own by faction, so the AI only positions. It currently targets the player ship; nearest-opposing-faction targeting is the obvious next step. The enemy ship is built from standard modules (main engine + maneuvering thrusters + turret) through the same `mount` path as the player ship.

### Turrets (`src/ship/turret.rs`)
A **turret module** (`ModuleKind::Turret`) is just a bare solid-block mount; a **turret** is installed into it separately by `spawn_turret(module, kind, arc, …)`, so different turrets can go on the same module. A turret has two **orthogonal** properties:
- `TurretKind` — role. `Cannon` (auto-tracks & shoots enemy *ships*, via `select_target`/`rotate_turret`/`fire_turret`); `PointDefense` (twin short barrels, very high fire rate; tracks incoming enemy *projectiles* in `PD_RANGE` and fires fast **slugs** at them via `point_defense`; deals no ship damage); or `PlayerCannon` (player-aimed — while piloting it follows the cursor and fires along its barrel on **held left-mouse**, gated by fire rate and always LOS-gated so it can't shoot over its own ship, via `player_weapons`). Only auto `Cannon`s get a ship `Target` (`select_target` skips the other two); `PointDefense`/`PlayerCannon` are driven by their own systems.
- `FireArc` — `OverShip` (fires from any angle) or `Hull` (can't shoot over its own ship). A hull turret doesn't aim into the hull: `aim_point`/`clear_aim_angle` clamp its aim to the nearest direction that clears the ship (swept via `aim_blocked`, a long-ray `shot_blocked`), so the barrel **locks at the edge of its arc** instead of clipping. `shot_blocked`/`segment_hits_box` (segment vs each own module/hull box in its local frame, excluding the turret's own mount) is the underlying test; `PlayerCannon` is always treated as `Hull`.

Turrets slew toward their (clamped) aim at a capped turn rate (`rotate_toward`, `CANNON_TURN_SPEED` / `PD_TURN_SPEED` / `PLAYER_TURN_SPEED`) and fire along their **current** barrel facing — they track naturally rather than snapping. A PD turret fires a fast stream (`PD_FIRE_INTERVAL`) alternating between its two barrels (`Turret.next_barrel`, `PD_BARREL_OFFSET`). Each PD slug (`PdSlug`, spawned by `spawn_pd_slug`) is a non-physics projectile moved by `update_pd_slugs`; when it reaches an enemy projectile (within `PD_HIT_RADIUS` of its swept segment) it strips `PD_SLUG_DAMAGE` from that projectile's `Bullet.health` (`bullet::BULLET_HEALTH`) and is spent — so a round takes several hits to kill, not one. Slugs that miss expire via `Lifetime` (`expire_lifetimes`).

Install sites: `mount_preplaced_turret(.., kind, arc, ..)` for ship loadouts, an explicit `spawn_turret` after `mount` for the station, and build-mode placement installs `BuildMode.turret_kind`/`turret_arc` (cycled with `T` / `Y` when a turret is selected). Tint: PD amber; cannon by arc (over-ship blue, hull white).

Current player-ship loadout: `PlayerCannon` front-center (cursor-aimed), an auto `Cannon`/`OverShip` at the rear corner, and a `PointDefense`/`OverShip` on the port corner (cockpit moved to starboard to free the front). Enemy = `Cannon`/`OverShip`; station = `Cannon`/`OverShip` (inert — no faction, so its turrets never acquire a target). Targeting (`select_target`) is still first-opposing-faction, locked, no range.

### Health & damage (`src/health.rs`, `src/ship/bullet.rs`)
Two-tier ship durability:
- Every module carries `ModuleHealth { current, max, armor }` (added in `mount`; per-kind values from `ModuleKind::durability()`; the hull/engineering root gets one directly in each `spawn_*_ship_base`). `armor` is flat reduction via `apply_armor` (`max(raw − armor, ARMOR_CHIP)`).
- The ship root carries `ShipHealth { current, max }`. `max` (capacity) tracks the sum of its modules' max health — kept in step by `sync_ship_health` (which shifts `current` by the same delta when modules are built/removed); it re-sums only on frames where a `ModuleHealth` was added or removed (gated by `Added<ModuleHealth>` / `RemovedComponents<ModuleHealth>`), not every frame. `current` is damaged on its own. A ship at 0 is despawned by `destroy_dead_ships` — a dedicated system, not the hit handler, so several hits landing the same frame don't each recursively despawn the same ship (which caused "entity despawned" command errors). Damage-path despawns use `try_despawn`/`try_insert` for the same reason.

A bullet (`bullet.rs`) carries the firing turret's `Faction`. On hit it walks `ChildOf` up from the struck collider to find the module (first `ModuleHealth` ancestor), the faction (nearest with `InFaction`), and the root; same-faction hits pass through (no friendly fire). Otherwise it applies armored damage to **both** the module and the ship pool. A module at 0 health gets `ModuleDisabled` + a dark overlay; disabled thrusters produce no thrust (`collect_thrust`) and disabled turrets don't fire (`fire_turret`). Bullets are on `GameLayer::Default` filtered to `Default`, so they strike structural bodies once (not interior `Walls`, not the walking `Player`). Characters (with plain `Health`) still take damage through the original `DamageReceived` path.

Each ship gets a floating health bar (`spawn_health_bars` / `update_health_bars`): a top-level (un-parented) entity tracking the ship's `ShipHealth`, kept above the ship and upright (uses the ship's translation, not rotation), with a left-anchored fill scaled/recolored (green→red) by health fraction. It despawns with its ship.

### Effects (`src/effects.rs`)
Small transient visuals. `Lifetime(Timer)` + `expire_lifetimes` despawn short-lived entities (PD slugs, sparks). `spawn_hit_spark(commands, pos, Hit)` spawns a flash sprite that grows and fades (`HitSpark` + `animate_hit_sparks`), with two looks so feedback is distinguishable: `Hit::Ship` (larger orange burst — a projectile struck a ship/character, from `bullet::on_bullet_hit`) vs `Hit::Intercept` (small cyan spark — point-defense struck an incoming projectile, from `update_pd_slugs`).

Hit positions avoid `GlobalTransform` (propagated in `PostUpdate`, so it lags a frame and places the marker short of where a fast bullet actually is). The ship-hit spark uses avian's world-space contact point (`Collisions::get(bullet, other)` → first manifold point's `.point`) so it sits on the struck surface, falling back to the bullet's physics `Position`. PD aim/intercepts read the bullet's `Position`. Don't use `GlobalTransform` for projectile-hit placement.

### Persistence (`src/save.rs`)
Save/load via serde + RON, read/written from `save.ron` (F5 save / F9 load). The key principle: **don't serialize live entities** (meshes/colliders/hierarchies are derived) — persist a plain-data snapshot (primitive fields, no `glam`/`avian` types) and rebuild the world from it. Cross-references must avoid raw `Entity` (runtime-only); use stable ids/markers.

**Chunked, colocated save framework — each feature owns its own chunk.** The on-disk `SaveFile` is `{ version, chunks: BTreeMap<String, String> }`: a map of named chunks, each an independently-serialized RON blob. A feature persists itself by registering a *capture* system (writes its chunk via `SaveFile::write(key, &T)`) in `PersistSet::Capture` and an *apply* system (reads via `SaveFile::read::<T>(key)`) in `PersistSet::Apply` — `save.rs` never has to know what a feature persists. **To add persistence for a new system, add its capture/apply systems to those sets in its own module; do not edit a central save struct.** Adding/removing a whole chunk needs no `SAVE_VERSION` bump (a missing chunk → `read` returns `None` → feature default); bump only when an existing chunk's format changes incompatibly. Examples: `camera::{capture_camera, apply_camera}` (zoom), `player::{capture_player, apply_player}` (pos + piloting `InstanceId`).

The pipeline runs in `Update` via two run-condition-gated sets (`saving`/`loading` read a `PersistOp` flag): **save** = `request_save` (F5 → stamp version, clear chunks, flag) → `PersistSet::Capture` (features write chunks) → `commit_save` (write file); **load** = `request_load` (F9) / `request_load_on_start` (PostStartup) parse the file into `SaveFile` + version-check + flag → `load_structures` (rebuild the world skeleton, see below) → `PersistSet::Apply` (features restore chunks) → `commit_load`. A whole load/save completes in one frame.

**Target model — "snapshot + inject new":** the save stores a full blueprint of *every* live structure (authored or built) plus the set of authored `ContentId`s it knew about; on load, rebuild all saved structures, then world-setup spawns any authored `ContentId` **not** in the save's known set (= content added since the save). So new hand-authored content appears in old saves; destroyed authored content stays gone (it's in the known set with no live instance). A `version` field guards schema migrations.

**Identity (built):** `Origin` (`Authored(ContentId)` vs `PlayerBuilt`) distinguishes hand-authored from player-built — at runtime *and* in saves. `InstanceId(u64)` is a stable per-instance id (from the persisted `NextInstanceId`, assigned by `assign_instance_ids`) used for cross-references instead of raw `Entity`.

**The structures chunk** is owned by `save.rs` (key `"structures"`, a `StructuresChunk` of every structure's blueprint + origin + instance + dynamic state, plus `next_instance_id` and `known_content_ids`). It's special among chunks because restoring it clears and respawns entities, so `load_structures` drives it **before** `PersistSet::Apply` (feature apply systems may reference rebuilt structures by `InstanceId`). Stages: (1) identity & origin; (2) blueprint model + `extract_blueprint` (`build/blueprint.rs`: `Blueprint`/`ModuleSpec`/`RootKind`; modules record their `kind` in `BuiltModule`); (3) generic `build_structure(blueprint, body, origin, instance)` — `spawn_root` makes the bare hull per `RootKind` (mirrors `spawn_*_base`; **keep in sync**), then replays each module by matching `ModuleSpec.slots` to the parent body's attach-point locals (+ `mount`, install turret, set/disable health); (4) `capture_structures` extracts the chunk, `load_structures` clears current structures + rebuilds saved ones via `build_structure`, then `spawn_authored` injects any `AUTHORED_CONTENT` id not in the save's known set. `extract_blueprint` orders modules so parents precede children (depth from root), so chained structures (station corridor arms) round-trip.

Piloting is persisted: the save records the piloted ship's `InstanceId`; load un-seats then `apply_pending_pilot` (`player.rs`, via the `PendingPilot` resource) re-seats the player at the rebuilt ship's pilot seat once it exists (retried each frame, since the rebuild is deferred). This is the pattern for re-linking saved `Entity` cross-refs by `InstanceId` after load.

**Watch for dangling `Entity` refs after replacing structures.** Load/new-game despawn the old structures, so anything holding the old ship's `Entity` breaks. `OnShip` (which `drive_player_on_ship` reads to carry the player — for both walking and seated) is re-linked each frame by `keep_player_on_ship`: if its ship is gone, point it at the current `PlayerShip` (else the player freezes with a stale facing). Any future component that stores a structure `Entity` needs the same treatment.

`request_load_on_start` (PostStartup) auto-loads the save if `save.ron` exists (else the default world stays); the load pipeline then runs on the first `Update` frame. Camera zoom is mode-dependent (`base × CameraZoom`, base = walk/pilot), so `apply_camera` sets a `CameraSnap` flag: `move_camera` snaps (no ease) until `PendingPilot` resolves, so the camera lands on the restored zoom immediately rather than easing from the stale pre-load scale through the wrong (not-yet-re-seated) base. An on-screen **New Game** button (`spawn_new_game_button`, top-right) deletes the save and resets to the default world; it's built from the UI toolkit (`ui::button`) with a `Pointer<Click>` observer that runs `new_game` via the `WorldEdit` `SystemParam` bundle (the only remaining `WorldEdit` user). Clicks no longer leak into the world — `ui::PointerOverUi` gates `player_weapons`/`place_module`/`deconstruct_module` (see UI toolkit).

**Debug keys:** F5 save, F9 load (also auto-loads at startup), F7 logs blueprint summaries. **Known fragility:** the on-disk format changed when the chunk framework landed — older flat `save.ron` files won't parse (they're rejected, default world stays). `spawn_root` duplicates the per-kind root setup from the `spawn_*_base` fns; the round-trip relies on `extract_blueprint`/`build_structure` agreeing with how `mount` placed things. Docking (`docked_to`) is still not persisted/re-linked.

### UI toolkit (`src/ui.rs`)
A small, reusable, themeable UI layer that all game UI (inventory/trading/missions/etc. as they land) builds on — *not* the experimental `bevy_feathers`/`bevy_ui_widgets` crates (their APIs churn; feathers is editor-flavored), though it borrows their patterns (token theming, headless behavior). `UiPlugin` installs it.

- **`Theme` resource — the single styling source of truth.** `Palette` (colors), `Spacing` (xs/sm/md/lg scale), `TextScale` (small/body/heading sizes), `radius`, optional `font`. Restyle the whole UI by editing `Theme::default` — the same "data in one place" idea as `ModuleRegistry`.
- **Bundle-constructor helpers** (return `impl Bundle`, so you `spawn(...).with_children(...)` and add markers): `panel` (padded/rounded/bordered column), `window(title)` (panel + heading), `button(label)`, the text helpers `heading`/`label`/`small`/`text`, and layout `row(gap)`/`column(gap)`. **0.19 note:** `border_radius` is a `Node` field (not a standalone component); `TextFont.font` is a `FontSource` (`Handle<Font>.into()`), `font_size` a `FontSize` (`f32.into()`).
- **Interaction = observers + `bevy_picking`** (the modern path, not `Interaction` polling). A `button` carries `ButtonColors` and gets automatic hover/press feedback from global observers registered by `UiPlugin` (`Pointer<Over>/Out/Press/Release`; read the target via `event.entity`). Attach behavior per button with `.observe(|_: On<Pointer<Click>>, ...| { ... })`.
- **`PointerOverUi(bool)`** — recomputed each frame from picking's `HoverMap` (true if any hovered entity is a `Node`); world-space click systems (`player_weapons`, `place_module`, `deconstruct_module`) check it so a UI click doesn't also act on the world.
- **Z-layer constants** (`Z_HUD < Z_PANEL < Z_MODAL < Z_TOOLTIP < Z_DRAG`, applied via `GlobalZIndex`) reserve high layers for transient overlays. **Drag-and-drop inventory is set up by these choices:** picking emits drag events for free (`Pointer<DragStart>/Drag/DragOver/DragDrop/DragEnd`), so it's just more observers. When building it: drag a lightweight ghost marked `Pickable::IGNORE` (so it doesn't eat the drop target's events); keep the UI a *view* over an inventory data model (slots carry container id + index, the drop handler moves data then the view refreshes — also makes save/load trivial); and let slot/button helpers take an `ImageNode` icon child (for a future `ItemDef` registry, same pattern as `ModuleRegistry`).

Migrated onto the toolkit: the **build-mode hint** (`spawn_build_ui` → a `panel` shown only while building, via a `BuildPanel` visibility toggle in `update_build_text`) and the **New Game button** (see Persistence).

### Inventory (`src/inventory.rs`) — first feature on the UI toolkit
`InventoryPlugin`. **Model — owned per structure, not per player:** `Inventory` is a **component** (`Vec<ItemStack>`) living on a ship/station **root** (cargo modules will set its capacity later). `attach_inventory::<C>` — an `Add` observer registered for `ShipBase` and `SpaceStation` (fires once per root, no per-frame polling; covers initial, runtime-built and save-loaded structures uniformly) — gives every root an empty one; `seed_player_inventory` fills the `PlayerShip`'s with a starter set on `Added<Inventory>` (so it re-seeds after a load/new-game rebuilds the ship — inventory isn't persisted yet). An `ItemStack` is `{ kind: ItemKind, name, count }` where `ItemKind` is `Module(ModuleKind)` (a buildable module — the drag-into-build target) or `Component` (generic material placeholder; the spot a future item-registry id goes). Names come from `ModuleRegistry` so they track `module_defs()`.

**View = view-over-model** (the pattern to copy for trading/missions/etc.): the window shows the **player ship's** inventory and holds no item state. `spawn_inventory_ui` builds the left-side window once (an absolute, full-height container — uses `ui::panel_style` since `ui::panel`'s baked `Node` can't express absolute+fill — `Z_PANEL`, starts `Visibility::Hidden`) with a heading and an empty `SlotContainer`; a bottom-right `ui::button` and the **I** key both toggle the window's `Visibility` (`toggle_inventory_hotkey` / a click observer; `PointerOverUi` already stops the button click leaking). `rebuild_inventory_ui` runs when the player ship's inventory changes (`Query<&Inventory, (With<PlayerShip>, Changed<Inventory>)>` — also fires on the seed and on a ship swap, since `Added` ⇒ `Changed`): it clears the `SlotContainer`'s children and respawns one slot per `ItemStack`. Each slot carries `InventorySlot { index }` (its index into `items`) — **the hook for drag-into-build**: a `Pointer<DragStart>` observer reads the index → `items[index]`, and a module item's drop in build mode will place that `ModuleKind` (picking already emits the drag events; mutating the model rebuilds the view, so persistence later stays "save the `Inventory` component, not the UI"). Slots show a color swatch (module's registry color; a stand-in for the eventual item icon) + name + count.

### Other modules
`world.rs` spawns the world (station, ground) via `WorldPlugin`. `station.rs` builds the station parametrically from `HUB_SIZE`/`ARM_SEGMENTS` (each side filled by `holds_and_docks` or `equipment_bank`, scaling with the hub width — no hard-coded slot indices). `interaction.rs` — `Interactable` + consoles. `camera.rs` — follows player/ship and aligns to the ship in build mode; orthographic zoom eases between `WALK_ZOOM` (on foot, close) and `PILOT_ZOOM` (piloting/build, pulled back), scaled by the scroll-wheel `CameraZoom` multiplier (`scroll_zoom`). `move_camera` eases the actual scale toward the target, so scroll zoom is smooth. `action.rs`/`animation.rs`/`health.rs`/`character.rs` — combat, sprite animation, damage. `enemy.rs` builds an enemy ship through the shared module path.

## Code check (scaling watch-points)

**Planned scope** (what the architecture must eventually carry): trading, mining, a simulated economy, station building, action-packed combat, missions, NPCs that walk around / do tasks on (moving) ships, a skill tree, crafting, and looting with quality + randomization — across a *big* world.

When the user says **"code check"**, review the current code against the risks below (and look for new ones in the same spirit): decisions/implementations that are fine now but will be expensive once the scope above lands. Be honest and specific; cite concrete code.

Tier 1 — foundational (decide before building big systems on top):
1. **World structure & coordinates** — one `f32` space; precision degrades at large coords. Decide contiguous-space-with-floating-origin vs discrete sectors/systems. Dictates streaming, economy graph, travel.
2. **Sim vs presentation + activation/LOD** — everything is a live rendered entity (a station is hundreds of child entities). Need a lightweight off-screen data model and "spawn detailed hierarchy only near the player." No sector/streaming concept.
3. **Persistence / stable IDs** — *largely done* (`src/save.rs` + `build/blueprint.rs`: full structure save/load via blueprints, F5/F9; chunked framework where each feature owns its chunk; see Persistence). Remaining: re-link raw `Entity` cross-refs by `InstanceId` on load (`Target`, `docked_to`, `Seated.ship`) so docking/targeting survive a load; persist non-structure state as it's added (economy, inventory, missions) — now a matter of adding a capture/apply pair in that feature's module; and DRY `spawn_root` against the `spawn_*_base` fns.
4. **Data-driven content** — *modules done* (`build/registry.rs`: `ModuleKind` is an id, data lives in `ModuleDef` in the `ModuleRegistry` resource, def-vs-instance split, archetype-driven behavior — see Module system). Remaining: turret stats are still hardcoded enums + `match` in `ship/turret.rs` (port them to a `TurretDef`/registry next, same pattern); `Faction` is still a hardcoded enum (see #5); and items/weapons/recipes/missions/loot want the same registry pattern + an item-definition vs item-instance split once those features exist. The registry is in-code now but consumers go through `registry.module(kind)`, so it can later be backed by asset files without touching them.
5. **Faction & reputation** — `Faction{Player,Enemy}` + "`!=` ⇒ hostile". Needs a faction registry + relationship/standing matrix (traders, pirates, neutrals, missions).

Tier 2 — bites during big fights / many NPCs:
6. **No spatial partitioning** — `select_target`, point-defense, `shot_blocked`/aim-clamp, damage all do global O(n)–O(n²) scans per frame. Use avian `spatial_query` / range/sector culling.
7. **Projectiles** — regular bullets have **no `Lifetime`** (miss ⇒ fly forever = leak); each is a physics sensor + observer. Add lifetimes, pooling, a cheap non-physics bulk path.
8. **Targeting/AI placeholder** — `select_target` locks the first opposing *part* forever (no nearest/range/LOS/threat); `fly_enemy_ships` is one routine. Needs a real AI architecture.
9. **NPCs on moving ships** — player carry is a delicate single-entity hack; many NPCs walking on many moving ships + interior pathfinding likely needs a ship-local interior space.
10. **Damage pipeline** — computed inline in the bullet observer; `Health` vs `ModuleHealth`/`ShipHealth` are separate. Want structured damage events + mitigation layers (shields/types/resist/crit) and a unified framework.

Tier 3 — compounding cleanups:
11. **Game states** — no Bevy `States`; everything runs always (menus, pause, docked/map/trading screens, sim pause).
12. **Input abstraction** — hardcoded scattered `KeyCode`s, conflicts creeping; want an action layer (rebinding, contextual, UI capture).
13. **UI architecture** — *foundation done* (`src/ui.rs`: themeable toolkit — `Theme` resource + bundle helpers + observer/picking interaction + `PointerOverUi` guard + z-layers; see UI toolkit). Strategy chosen: a custom theme+helpers layer on plain `bevy_ui` rather than the experimental widget crates. Remaining: build the actual feature UIs (inventory/trading/missions/skill tree/crafting) on it — starting with a drag-and-drop inventory as a *view* over a data model; consider a custom font and an `ImageNode`-icon slot helper.
14. **Schedule discipline** — recurring Fixed-vs-Update / `GlobalTransform`-lag / `Position` bugs; define explicit `SystemSet`s + documented ordering.
15. **Tests for pure logic** — extract economy/damage/inventory/loot math into pure, unit-testable functions.

Minor: dev scaffolding still in `main` (`spawn_obstacle`, `display_events`, `PhysicsDebugPlugin`).

## Conventions

- Physics-frame work goes in `FixedUpdate`/`FixedPostUpdate`; **`just_pressed` input must be polled in `Update`** (FixedUpdate can run zero or many times per frame and miss the edge).
- Cross-submodule items use `pub(crate)`; a private type in a `pub(crate)` fn signature triggers `private_interfaces` — bump the type's visibility to match.
- `commands.entity(e).despawn()` is recursive in Bevy 0.19 (despawns children too).
