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
- **Piloting** (seated at a cockpit's pilot seat): **W/S** forward/reverse, **A/D** rotate, **Q/E** strafe. Thrust is gated by available thrusters (see thruster system).
- **F** — interact: sit/stand at a pilot seat, use a console, or open build mode at an engineering console.
- **G** — dock / undock (must be seated at the helm, lined up with another port).
- **Build mode** — entered with **F** at an engineering console; **1–8** select module (Cargo/Engine/Sensor/Turret/Dock/Hallway/Cockpit/Thruster); **R** rotates the ghost's facing; left-click places, or (with nothing selected) deconstructs the module under the cursor; **B**/**Esc** exits.

## Architecture

Bevy ECS: behavior lives in **systems** registered in `main.rs` (the `Game` plugin) and in per-feature plugins (`BuildPlugin`, `WorldPlugin`, `BackgroundPlugin`). Structures are built as **entity hierarchies** via `ChildOf`; only the root (ship hull / station root) is a `RigidBody`, and child colliders form one compound body.

### Collision layers (`GameLayer` in `src/ship/mod.rs`) — read this first
A 3-layer model whose subtleties cause most physics bugs. **Critical trap:** any collider spawned *without* an explicit `CollisionLayers` lands on `Default` (the first variant) with a filter of *all* layers — so it collides with everything. This is load-bearing (hulls/structural colliders rely on it) but means a forgotten tag silently turns decoration into a wall.

- `Default` — structural bodies (hulls, module/room structural colliders). Block other structures; also the implicit membership of untagged colliders.
- `Walls` — interior walls. Block the **player** but **not each other**, so docked structures' walls can overlap without the solver fighting.
- `Player` — the walking player; filtered to collide only with `Walls`.

### Module system (`src/build/`) — the core mechanic
One unified path builds **all** modules — player ship, station, and enemy ship all go through it. Do not add a parallel module system.

- `kinds.rs` — `ModuleKind` enum + `Footprint { width, depth }` (in `UNIT`=50 cells). `width` runs along the attaching edge; `depth` extends outward. Per-kind flags: `walkable`, `opens_doorway`, `mounts_turret`, `is_dock`.
- `attach.rs` — `build_buildable_side(...)` creates a row of doorway slots (removable panels + `AttachPoint`s) and wall segments for one side of a body. Returns `AttachSlot`s for pre-mounting.
- `spawn.rs` — `mount(...)` occupies slots and spawns a module, returning `Mounted { module, sides }` so further modules chain onto exposed sides (`mount_far` pattern in `station.rs`). `mount_preplaced_turret`/`mount_preplaced_dock` are construction-time shortcuts. Walkable rooms expose buildable sides; solid/dock modules expose none.
- `mode.rs` — interactive build mode: ghost preview, manual facing via `R`, snapping. Snapping measures from the module's **connecting edge** (offset by half its depth), not the cursor center. `BuiltModule` records occupied points + opened panels so deconstruction can re-seal.

To assemble a structure: create a root body with a `Collider`, call `build_buildable_side` per side, then `mount`/`mount_far`. See `ship/mod.rs::spawn_player_ship_base` and `station.rs::spawn_space_station`.

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
Flight is split so any ship (player or AI) flies the same way: a *controller* sets the ship's `ThrustControl` intent (-1/0/1 per axis) and adds the `Piloted` marker; the shared `drive_ships` solver turns intent + thrusters into motion for every `ShipBase`. `control_player_ship` is the only player-specific piece (keyboard → the player ship's `ThrustControl`); an enemy AI would set `ThrustControl` + `Piloted` on its ship instead. A `Piloted` ship auto-brakes toward rest (gated by opposing thrust); a non-`Piloted` ship coasts. `ThrustCommand` is the *effective* per-frame thrust (intent + auto-brake) the solver writes for nozzle visuals. All ships are excluded from `apply_movement_damping` — braking is thruster-gated, not free drag.

### Other modules
`world.rs` spawns the world (station, ground) via `WorldPlugin`. `interaction.rs` — `Interactable` + consoles. `camera.rs` — follows player/ship; aligns to the ship in build mode. `action.rs`/`animation.rs`/`health.rs`/`character.rs` — combat, sprite animation, damage. `enemy.rs` builds an enemy ship through the shared module path.

## Conventions

- Physics-frame work goes in `FixedUpdate`/`FixedPostUpdate`; **`just_pressed` input must be polled in `Update`** (FixedUpdate can run zero or many times per frame and miss the edge).
- Cross-submodule items use `pub(crate)`; a private type in a `pub(crate)` fn signature triggers `private_interfaces` — bump the type's visibility to match.
- `commands.entity(e).despawn()` is recursive in Bevy 0.19 (despawns children too).
