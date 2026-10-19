# CLAUDE.md — Rustcraft (a simplified Minecraft clone)

> v1.0 (M0–M13) is complete and shipped as a macOS app. **v1.1 (M14–M30)** moves the game toward modern Minecraft; see §1.1, §7.2 and the decisions in §9.

## 1. What we're building

Rustcraft (working name) is a single-player Minecraft clone with survival and creative modes (no cheats or commands) for macOS (Apple Silicon), written in Rust. It follows **Minecraft Java Edition 1.0.0**: the same blocks, items, recipes and, where practical, the same mechanics and physics constants. The core loop is an infinite procedurally generated world, mining and placing blocks, inventory, survival (health, hunger, mobs, day/night), crafting, and saving.

**Reference list:** `docs/blocks-and-items.md` (copied from the planning folder) lists every 1.0.0 block and item with its original numeric ID. It is the source of truth for what exists in the game.

**Faithfulness:** mechanics should match Minecraft 1.0.0 as closely as practical. When unsure of an exact value, use the best-known Minecraft value, write it in `docs/DECISIONS.md`, and keep it as a named constant in `src/config.rs`.

**Out of scope (v1.0):** multiplayer, cheats/commands, switching game mode inside an existing world, mod support, Windows/Linux builds (wgpu may run there, but don't spend effort on it). Blocks for redstone, the Nether, the End, enchanting and brewing are registered and placeable, but their special behavior was not built in v1.0.

### 1.1 Version 1.1: toward modern Minecraft

v1.1 keeps everything from v1.0 and moves the game toward **modern Minecraft Java Edition (use 1.21.x as the reference)**: modern movement and combat, many more mobs and biomes, structures and villages, Minecraft resource-pack support, a first-person hand, the Nether, the End with the ender dragon, and advancements.

- **Faithfulness from M14 on:** match modern Java Edition behavior and values, not 1.0.0. Where 1.0.0 and modern differ (sprinting, hitboxes, combat, recipes), modern wins. Record each switch in `docs/DECISIONS.md`.
- **New content gets added with its recipes.** Each milestone adds crafting/smelting recipes for the blocks and items it introduces. M29 does a final sweep for anything still missing.
- **Mojang's files:** don't download or bundle Minecraft's own assets. Textures come from Kenzo's assets or resource packs Kenzo provides (M15). Mob models and structure layouts are hand-built in code/data to match vanilla shapes.
- **Still out of scope in v1.1:** multiplayer, commands/cheats, mods, Windows/Linux, comparators and observers, the Wither, and ocean monuments.

## 2. Tech stack

| Concern | Choice | Why |
|---|---|---|
| Language | Rust (stable, edition 2021+) | Performance, safety, good voxel ecosystem |
| Windowing/input | `winit` | Standard, native macOS support |
| Rendering | `wgpu` (Metal backend on macOS) | Native Metal underneath, WGSL shaders, stable API |
| Math | `glam` | Fast, simple, widely used with wgpu |
| GPU data | `bytemuck` | Safe casting of vertex/uniform structs |
| Noise | `noise` | Terrain generation (switch to `fastnoise-lite` only if profiling demands it) |
| Debug/menu UI | `egui` + `egui-wgpu` + `egui-winit` | Debug overlay, menus |
| Image loading | `image` | Texture atlas |
| Audio | `kira` | Sound effects, music |
| Threading | `std::thread` + `crossbeam-channel` | Background chunk gen/meshing |
| Serialization | `serde` + `bincode` + `lz4_flex` | Compact compressed chunk saves |
| Data files | `ron` | blocks/items/recipes definitions |
| Logging | `log` + `env_logger` | `RUST_LOG=debug cargo run` |
| Errors | `anyhow` (app), `thiserror` (modules) | |
| Benchmarks (v1.1) | `criterion` | Performance gate from M14 on |

**Version policy:** At M0, pin the latest stable versions of these crates in `Cargo.toml` and commit `Cargo.lock`. Do not upgrade crates mid-milestone. If an API doesn't match your memory, check the docs for the pinned version (`cargo doc --open` or docs.rs) rather than guessing.

**No game engine.** Do not add Bevy, Fyrox, or similar. The renderer and game loop are ours.

## 3. Commands

```bash
cargo run --release            # play (debug builds are too slow for real chunk loads)
cargo run                      # debug build
cargo test                     # all tests
cargo clippy -- -D warnings    # must pass before every commit
cargo fmt                      # must be applied before every commit
RUST_LOG=rustcraft=debug cargo run --release
cargo run --release -- --bench [seconds] [--distance <chunks>]   # timed flight, prints fps
cargo run --release -- --perf-test   # the performance gate: writes docs/perf/m<N>.md (§7.2)
cargo bench                    # chunk generation, lighting, meshing and fluid benchmarks
cargo run --release -- --shot <name>   # capture a scripted scene to docs/screenshots/<name>.png (--shot list)
scripts/bundle-macos.sh        # build target/release/bundle/Rustcraft.app (icon from assets/*.svg)
screencapture -x docs/screenshots/m<N>-<what>.png   # milestone screenshots (or --shot, which works with the screen locked)
```

Add to `Cargo.toml` so debug builds stay playable:

```toml
[profile.dev.package."*"]
opt-level = 3
[profile.dev]
opt-level = 1
```

## 4. Architecture

```
src/
  main.rs            # entry: create window, run App
  app.rs             # winit event loop, owns State, fixed-timestep loop
  config.rs          # constants (chunk size, world height, render distance, physics values)
  render/            # wgpu device, pipelines, camera uniforms, texture atlas, shaders (*.wgsl)
  world/
    block.rs         # BlockId (u16) + BlockRegistry (data-driven properties)
    chunk.rs         # Chunk storage (16x16x16 sections), get/set
    mesh.rs          # chunk → vertex buffer (face culling, then greedy meshing)
    worldgen/        # terrain, biomes, caves, ores, trees (`gen` is reserved in Rust 2024)
    manager.rs       # load/unload around player, worker threads, dirty tracking
    light.rs         # sky light + block light propagation
    fluid.rs         # water/lava flow (M12)
    save.rs          # region files
  player/            # controller, physics, collision (AABB), raycast, interaction, stats
  entity/            # mobs, item drops
  inventory/         # items, stacks, inventory, crafting recipes
  ui/                # HUD, hotbar, inventory screen, menus (egui)
  pack/              # resource packs: layers, animations, block and item models
  look.rs            # everything texture-derived, rebuilt when the packs change
  figure.rs          # the player as drawn: the hand, or the model and what it holds
  game/view.rs       # swing, hand dip, bobbing, body turn, F5 views
  game/combat.rs     # cooldowns, damage, crits, sweeps, the shield (constants in config::combat)
  render/{player,hand,held}.rs   # player model and poses, first-person hand, held item geometry
  world/{redstone,piston,rails,explosion}.rs   # signals and ticks, what pistons move, rail joining, blast rays
  game/{redstone,logistics,minecarts,explosion}.rs   # carrying out redstone events, hoppers and droppers, carts, TNT
  entity/{minecart,tnt}.rs   # rail physics, primed TNT
  entity/{mobs,mob,ai}.rs   # mob definitions (mobs.ron), a living mob, what each kind decides
  game/{mobs,mob_acts,animals}.rs   # mob ticks and spawning, what mobs do to the world, the player and animals
  audio/
assets/
  textures/          # Kenzo's PNGs (the base resource layer, 1.12 layout) → packed into an atlas at startup
  sounds/            # Kenzo's sound files, named per docs/sounds.md
  data/              # blocks.ron, items.ron, recipes.ron
resourcepacks/       # beside saves/: Minecraft resource packs, picked in the Resource Packs screen
docs/
  blocks-and-items.md
  sounds.md
  DECISIONS.md
  screenshots/
```

**Key conventions**
- **World size:** Y is up. Chunks are 16×16 columns made of vertical 16³ sections. World height is **256** (Y 0..255, 16 sections) since M14 (128 in v1.0), with sea level at Y 63. Default render distance is **9 chunks**. Both are constants in `config.rs`; if 60 FPS isn't reachable, lower them, note it in `DECISIONS.md`, and tell Kenzo.
- **IDs:** Since M14, blocks and items have modern Java Edition names (`minecraft:oak_log`), defined in `assets/data/blocks.ron` and `items.ron`; `build.rs` turns the names into constants (`world::blocks::OAK_LOG`, `inventory::items::STICK`). Runtime IDs are file positions and saves store names. A `BlockState` is a block plus modern block state properties (`world::props`). `assets/data/legacy.ron` maps v1.0's numeric 1.0.0 IDs to the new names. Properties (solid, transparent, hardness, drops, texture per face, light emission, tool type, placement, shape) live in `blocks.ron`. Never scatter block behavior across `match` arms.
- **Textures (M15):** textures are looked up by modern resource location (`block/stone`) through `pack::Resources`: selected packs from `resourcepacks/` over Kenzo's `assets/textures/`; older names are in `assets/data/texture_names.ron`. `look::Look` builds everything texture-derived and is rebuilt when the packs change. At M1, inspect the folder, map filenames to blocks/items in the RON files, and list any block with no matching texture in `DECISIONS.md`. Missing textures render as a magenta/black checkerboard so they're easy to spot. Don't generate or download other art.
- **Timing:** Game logic runs at a fixed 20 ticks/second, like Minecraft. Rendering runs at the display refresh rate with interpolation.
- **Threading:** Chunk generation and meshing run on worker threads. The main thread never blocks on them; it uploads finished meshes to the GPU.
- **Physics constants (Minecraft values, per tick):** gravity 0.08 blocks/tick², vertical drag ×0.98 per tick, jump velocity 0.42, walk ≈4.317 blocks/s, sprint ≈5.612, sneak ≈1.31, player hitbox 0.6×1.8 (sneaking 1.5, swimming and crawling 0.6), eye height 1.62 (1.27, 0.4), step height 0.6, block reach 5 (survival), 20 health points (10 hearts), 20 hunger points.

## 5. Working rules for Claude Code

1. **Work one milestone at a time, in order.** Read the milestone's goals and acceptance criteria before starting. Don't start features from later milestones.
2. **Small, compilable steps.** Every commit must build, pass `cargo clippy -- -D warnings`, pass `cargo fmt --check`, and pass `cargo test`.
3. **Test the logic, not the GPU.** Unit-test pure code: chunk indexing, meshing face counts, raycasts, collision, physics constants, inventory stacking, crafting matching, save round-trips, light propagation. Rendering is verified by running the game.
4. **Verify every milestone with screenshots, then stop for Kenzo.** Run `cargo run --release`, capture screenshots that show each acceptance criterion into `docs/screenshots/`, commit them, and open the milestone PR (see §6). Then stop and ask Kenzo to check it by hand. Don't start the next milestone until Kenzo approves.
5. **Performance budget:** 60+ FPS at render distance 9 on the M-series Mac mini. Show FPS, position, biome, chunk count, and facing direction in an F3 debug overlay from M1 onward.
6. **When stuck or when a design decision isn't covered here,** pick the option closest to Minecraft 1.0.0, write it in `docs/DECISIONS.md` (one line: date, decision, reason), and continue.
7. **Update this file** when a convention changes. Mark the milestone checkbox when its acceptance criteria are met.
8. Never commit build output (`target/`) or saves (`saves/`). Screenshots go only in `docs/screenshots/`.

## 6. Git workflow

**Remote:** the repo lives on GitHub (`origin`). `main` is always playable.

**Branching:** one branch per milestone, `m<N>-<slug>` (e.g. `m3-worldgen`). Push it to `origin` as you go.

**Milestone finish:**
1. Last commit on the branch: `docs(m<N>): mark milestone <N> complete`.
2. Open a PR into `main` titled `M<N>: <milestone name>`, with a checklist of acceptance criteria and the screenshots embedded.
3. Stop and wait for Kenzo's manual check.
4. After approval, merge with a merge commit (no squash), then tag `m<N>` and `git push origin m<N>`.

**Commit format** (Conventional Commits, milestone as scope):

```
<type>(m<N>): <imperative summary, ≤72 chars>

<optional body: what and why, wrapped at 72>
```

Types: `feat`, `fix`, `perf`, `refactor`, `test`, `docs`, `chore`, `build`.

Examples:
```
feat(m2): add chunk face-culling mesher
test(m2): cover mesher face counts for single and adjacent blocks
perf(m3): move chunk generation onto worker threads
fix(m4): stop player clipping through blocks at chunk borders
docs(m4): mark milestone 4 complete in CLAUDE.md
```

**Cadence:** commit after each working sub-step (roughly every 30–150 lines of meaningful change) and push regularly. Never bundle unrelated changes.

## 7. Milestones

Each milestone ends with a playable build, screenshots, a PR, and Kenzo's sign-off.

### 7.1 Version 1.0 (complete)

### - [x] M0 — Project scaffold
- `cargo init --name rustcraft` in the current folder (the folder Claude Code was started in; don't create a nested project folder), crate list from §2 pinned, `.gitignore`, dev profile tweaks, `docs/DECISIONS.md`, `docs/blocks-and-items.md`, `docs/sounds.md`, `assets/textures/` with Kenzo's PNGs.
- GitHub remote set up; `main` pushed.
- Empty winit window that opens, clears to sky blue via wgpu, and closes cleanly.
- **Done when:** `cargo run` shows a blue window; clippy/fmt/test pass; tagged `m0`.

### - [x] M1 — Render a cube, fly around
- wgpu pipeline, WGSL shader, depth buffer, window resize handling.
- Free-fly camera: WASD + mouse look, cursor grab (Esc releases), Space/Shift up/down.
- Texture atlas built from `assets/textures/`; texture-to-block mapping table; a single textured cube.
- F3 debug overlay (egui): FPS, position, direction.
- **Done when:** a textured cube renders correctly from all angles at 60+ FPS.

### - [x] M2 — Chunks, meshing, and the full block registry
- `blocks.ron` with **every block from `docs/blocks-and-items.md`** (IDs 0–122): properties, hardness, per-face textures, transparency, light emission.
- `Chunk` storage and a flat test world of several chunks.
- Mesher with face culling (only faces bordering transparent blocks) and per-face textures (grass top/side/bottom). Non-cube shapes (torches, flowers, slabs, stairs, fences, doors, cross-shaped plants) can render as simple placeholders at first, but each must be listed in `DECISIONS.md` as pending.
- Cross-chunk neighbor lookup so borders don't render hidden faces. Frustum culling of chunks.
- **Done when:** a flat world renders at 60+ FPS; a debug "block gallery" world shows every block; unit tests cover face counts.

### - [x] M3 — Infinite world generation (4 biomes)
- Seeded noise terrain with world height 128 and sea level 63.
- Biomes: **ocean, plains, forest, desert**, with blended borders. Oceans are filled with still water (ID 9) that doesn't flow yet.
- Caves (3D noise), ores at 1.0.0 depths (coal, iron, gold, lapis, redstone, diamond), oak and birch trees, flowers and tall grass, sand and sandstone in deserts, cactus and sugar cane, bedrock floor.
- Chunk manager: load/unload around the player within render distance 9, worker threads for gen + meshing, priority by distance.
- Upgrade to greedy meshing if vertex counts or FPS demand it.
- **Done when:** flying in one direction for 2 minutes shows all four biomes with continuous terrain and no stalls; the same seed gives the same world.

### - [x] M4 — Player physics and block interaction
- Walking with Minecraft constants from §4: gravity, jump, AABB collision, step-up, sprint, sneak (no falling off edges). The M1 free-fly camera stays as a dev tool in debug builds only (`cfg(debug_assertions)`). Player flying in release builds comes with creative mode in M7.
- DDA voxel raycast (reach 5) for the target block, wireframe highlight outline.
- Left-click breaks (instant for now), right-click places the selected block; can't place inside the player.
- Remesh only affected chunks (and neighbors on borders).
- **Done when:** you can walk, jump, dig a staircase down, and build a house without clipping; movement speeds match §4 in unit tests.

### - [x] M5 — Lighting
- Sky light (sunlight propagation from top) and block light (torches, glowstone, lava, jack o'lanterns, etc. at their 1.0.0 levels) using BFS flood fill, including removal.
- Smooth lighting / ambient occlusion per vertex.
- **Done when:** caves are dark, torches light them, and breaking/placing blocks updates light correctly across chunk borders.

### - [x] M6 — Saving and loading
- Region files (32×32 chunks per file), only modified chunks saved, compressed.
- Save player position, stats, inventory, world seed, game mode, time of day. Autosave every few minutes and on quit.
- Main menu: create world (name + seed + **game mode: Survival or Creative**, default Survival), load world (list shows each world's mode), quit. The mode is fixed for the life of the world, as in 1.0.0.
- **Done when:** edits survive quit-and-relaunch; a creative world reloads as creative; round-trip tests pass.

### - [x] M7 — Items, inventory, survival mining, and creative mode
- `items.ron` with every item from `docs/blocks-and-items.md`; stacks (64, or 1/16 where 1.0.0 differs); dropped item entities with pickup.
- Hotbar (1–9, scroll wheel), full inventory screen (E) with click and drag stack moving.
- Mining time from 1.0.0 hardness × tool type/tier (wood/stone/iron/gold/diamond), tool durability, correct-tool requirements for drops, 1.0.0 drop rules.
- **Creative mode** (1.0.0 behavior): blocks break instantly with no drops; placing doesn't use up items; tools take no durability; double-tap Space toggles flying (Space up, Shift down, faster horizontal speed); the E key opens a scrollable creative inventory containing every registered block and item, and clicking one puts a stack in the hotbar. Keep game-mode checks in one place (e.g. a `GameMode` on the player) rather than scattered `if creative` checks.
- **Done when:** in survival you can mine stone with a pickaxe, get cobblestone, and see tool durability drop; in a creative world you can fly, break blocks instantly, and pull any block from the creative inventory.

### - [x] M8 — Environment
- Day/night cycle (20-minute day), sun and moon, sky color gradient, sky light level tied to time.
- Transparent render pass for water, ice and glass; swimming and buoyancy in still water; underwater fog tint.
- Fog at render distance edge; clouds.
- **Done when:** a full day cycle looks right and you can swim in the ocean.

### - [x] M9 — Health, hunger, and mobs
- Health, hunger and saturation, fall damage, drowning, lava damage, respawn at spawn point; hearts and hunger HUD.
- Entity system for mobs: AABB physics shared with the player, box-model rendering.
- Mobs: **pig** (passive, wanders, drops porkchop) and **zombie** (hostile, spawns in darkness, chases and attacks, burns in daylight, drops rotten flesh), with 1.0.0 behaviors. Build the mob code so more 1.0.0 mobs can be added later from data plus a small AI module; adding them isn't planned yet.
- Melee combat with knockback and critical hits; bow and arrows; eating restores hunger.
- In creative mode: no damage of any kind, no hunger, no health/hunger HUD, and hostile mobs ignore the player.
- **Done when:** in survival you can survive a night, kill a zombie, and eat a porkchop from a pig; in creative a zombie at night doesn't attack you and falling or lava does no damage.

### - [x] M10 — Crafting and smelting
- 2×2 inventory grid and 3×3 crafting table; shaped and shapeless recipes from `recipes.ron`.
- **Most 1.0.0 recipes** for everything in scope: building blocks, tools, weapons, armor, food, torches, chests, doors, beds, buckets, etc. Skip recipes for deferred systems (redstone, enchanting, brewing) and list them in `DECISIONS.md`.
- Furnace with 1.0.0 fuels and smelting recipes.
- Chest block with persistent storage. Beds set the spawn point and skip the night.
- **Done when:** the wood → stone → iron → diamond progression works in a fresh survival world.

### - [x] M11 — Audio and polish
- **[PLACEHOLDER: sound effects and music, waiting on Kenzo]** Don't implement audio yet. Kenzo is still downloading the files listed in `docs/sounds.md`. Until Kenzo confirms the files are in `assets/sounds/` and replaces this line with real instructions, skip all audio work, add no audio crate code, and don't treat missing sound as a failure. If you reach M11 before then, do the rest of M11 and note in the PR that audio is pending.
- Settings menu: render distance, FOV, mouse sensitivity, volume, vsync. Pause menu.
- Block-breaking particles and crack overlay.
- **Done when:** a fresh player can go from a new world to iron tools with no debug UI (sound is excluded while the placeholder above stands).

### - [x] M12 — Fluids
- Flowing water and lava with 1.0.0 spread rules (water 7 blocks, lava 3 in the Overworld), source blocks, infinite water sources, lava + water → obsidian/cobblestone.
- Buckets pick up and place fluids. Current pushes entities.
- **Done when:** pouring water into a hole spreads and fills like Minecraft, and lava + water makes cobblestone and obsidian.

### - [x] M13 — Performance and release
- Profile (Instruments / `cargo flamegraph`), fix hot spots, reduce allocations in meshing.
- Release build bundled as a macOS `.app` (e.g. `cargo-bundle`), app icon.
- **Done when:** 60+ FPS at render distance 9 on the Mac mini (stretch goal 12); `.app` launches by double-click.

### 7.2 Version 1.1 — toward modern Minecraft

Same build rules as v1.0 (small commits, screenshots, one PR per milestone), but reviews happen at **checkpoints** instead of after every milestone:

| Checkpoint | Milestones | Theme |
|---|---|---|
| A | M14–M17 | Foundations, resource packs, player model, movement |
| B | M18–M23 | Overworld: combat, redstone, mobs, biomes, enchanting, villages |
| C | M24–M26 | The Nether and brewing |
| D | M27–M30 | The End, advancements, release |

**Checkpoint rules (override §5 rule 4 for v1.1):**
- Within a checkpoint, finish each milestone fully (tests, screenshots, PR), then start the next one on a branch made from the previous milestone's branch. Don't merge anything yourself.
- **Check your own screenshots.** After capturing each one, open it and confirm it shows the acceptance criterion. Write what you see in the PR. A blank, black or wrong screenshot means the milestone isn't done.
- At the end of each checkpoint, **stop**. Post a summary table (milestone, pass/fail, PR link, known issues) and wait for Kenzo. After Kenzo approves, merge the checkpoint's PRs into `main` in order with merge commits, tag each `m<N>`, and push the tags.
- If something blocks a milestone (a design question, a failing test you can't fix), stop early and ask rather than pushing on.
- **Performance gate (from M15 on):** at the end of every milestone, run `cargo bench` and the `--perf-test` from M14 and save the results to `docs/perf/m<N>.md`. If average FPS drops below 60, the 1% low drops below 40, or any benchmark is more than 15% slower than the previous milestone, fix it before moving on (or stop and ask if you can't). Include the FPS numbers in each PR and in the checkpoint summary table.
- **Model recommendation:** At the end of each milestone, recommend Sonnet or Opus for the next one. Pick Opus only for world generation, structures, or complex systems like the Ender Dragon fight; otherwise Sonnet.
- After M30 merges, tag `v1.1.0` and rebuild the `.app`.

#### Checkpoint A — Foundations

### - [x] M14 — Foundations for modern content
- Switch blocks and items to namespaced string IDs (`minecraft:stone`, `minecraft:oak_log`) matching modern Java names, mapped to compact numeric runtime IDs. Keep a table from v1.0 numeric IDs to the new names.
- Block states (facing, axis, half, waterlogged, age, powered, etc.) so stairs, logs, slabs, doors, crops and redstone parts are described the modern way.
- **World height 256** (Y 0–255, 16 sections). Sea level stays 63. Existing v1.0 chunks keep their terrain, with air above Y 127.
- Save format version number plus a one-time migration, so v1.0 worlds open in v1.1 without losing anything.
- Dimension-aware world storage (Overworld now; the Nether and the End plug in later).
- **Accurate block shapes:** every block state gets its own collision box and selection outline matching modern Minecraft, so torches, flowers, saplings, tall grass, rails, buttons, levers, carpets, slabs, stairs, fences, walls, panes, doors, trapdoors, ladders, beds, cakes, chests, snow layers and farmland are only as big as they look. You can walk through torches and flowers, stand on half slabs, and the raycast in §4 targets the real shape.
- **Gravity blocks:** sand, red sand, gravel, concrete powder, anvils and the dragon egg become falling-block entities when there's air below and they get a block update (placement, a neighbor changing, or a fluid flowing past). Naturally generated blocks stay where worldgen put them until updated, as in Minecraft. Falling blocks land as blocks, or drop as items if they land on a torch, slab or similar.
- **Performance test harness (used by every later milestone):**
  - `cargo bench` benchmarks (with `criterion`) for chunk generation, meshing, lighting and fluid/redstone ticks.
  - `cargo run --release -- --perf-test`: loads a fixed seed, flies a scripted path for 60 seconds at render distance 9, and writes average FPS, 1% low FPS, frame-time spikes, chunk gen/mesh times and memory to `docs/perf/m<N>.md`.
  - `docs/perf/baseline.md` records the numbers from the end of M14.
- **Done when:** a v1.0 world opens unchanged in v1.1, you can build up to Y 255, blocks with states place and save correctly, you can walk through a torch and stand on a slab, sand above a dug-out hole falls, migration round-trip tests pass, and the perf test shows 60+ FPS at render distance 9.

### - [x] M15 — Resource pack support
- Load Minecraft Java resource packs from a `resourcepacks/` folder, as a `.zip` or a folder with `pack.mcmeta`. Read `assets/minecraft/textures/{block,item,entity,environment,gui}/...` by their modern names.
- Animated textures (`.png.mcmeta`), and block/item model JSON where practical (at least `cube_all`, `cube_column`, `cross`, `orientable`, and the item model `display` transforms used in M16).
- Packs stack in priority order, with Kenzo's v1.0 assets as the base layer. Any texture a pack doesn't cover falls back to the layer below.
- Resource Packs screen in the main menu and settings: pick, reorder and apply without restarting.
- **Done when:** loading a vanilla-style 16× pack and a 32× pack each re-textures the world, items, mobs and GUI, and missing textures fall back cleanly.

### - [x] M16 — Player model, first-person hand, and third-person view
- Player skin: a 64×64 Minecraft skin PNG in `assets/skins/` (Steve-style by default). A skin in the active resource pack's `entity/player/` overrides it.
- **First person:** right arm from the skin; held blocks as small 3D cubes and flat items as extruded sprites, positioned with modern `firstperson_righthand` display transforms (from the pack's model JSON if present, otherwise built-in defaults matching vanilla). Off-hand rendering ready for M18.
- **Full player model** with modern animations: walking, sprinting, sneaking, swimming, crawling, swinging, eating, bow draw, holding items in both hands, and elytra flight (used in M28).
- **F5 cycles** first person → third person behind → third person in front, with the camera pulled in when blocked by walls.
- Animations: swing on mine/attack/place, equip dip on hotbar change, view bobbing (toggle in settings).
- **Done when:** the hand and held items look and animate like modern Minecraft, and F5 shows the skinned player animating correctly while walking, sneaking and swimming.

### - [x] M17 — Modern movement
- Sprint with double-tap W or a sprint key (Ctrl by default), sprint-jumping, FOV widening while sprinting, and sprint stopping on collision, sneaking, hunger ≤ 6 or attacking. Sprinting drains hunger at the modern rate.
- Modern sneaking: 1.5-block-tall hitbox, slower speed, edge protection.
- Swimming: sprint underwater to swim in a 0.6-block-tall pose; crawling into 1-block gaps.
- Ladders and vines climb at modern speeds; soul sand, slime/honey-style slow blocks and ice slipperiness use modern values.
- **Done when:** sprinting, sprint-jumping, sneaking, swimming and crawling feel like modern Minecraft, with speeds checked by unit tests. **Stop: Checkpoint A.**

#### Checkpoint B — The Overworld

### - [x] M18 — Modern combat and essential items
- Modern (1.9+) combat with Java Edition values, kept as constants in `config.rs`:
  - **Attack cooldowns** from each item's attack speed: swords 1.6/s (0.625 s), axes by tier (wood and stone 0.8/s, iron 0.9/s, gold, diamond and netherite 1.0/s), pickaxes 1.2/s, shovels 1.0/s, hoes by tier (1.0–4.0/s), empty hand and other items 4.0/s. Switching items resets the cooldown. Show the cooldown indicator under the crosshair.
  - **Damage** follows the modern charge curve: damage × (0.2 + 0.8 × charge²). Axes hit harder per swing than swords but recover more slowly, using modern per-tier damage values.
  - **Critical hits:** ×1.5 damage when the attacker is falling, not on the ground, not in water or lava, not on a ladder or vine, not riding, not blind, not sprinting, and the cooldown is at least 90% charged. Show crit particles and play the crit sound.
  - **Sweeping:** swords only, on the ground, not sprinting, fully charged; hits nearby mobs for reduced damage, with sweep particles.
  - **Sprint knockback:** a sprinting, charged hit deals extra knockback and stops the sprint.
  - **Axes disable shields** for 5 seconds.
  - Unit tests cover the cooldown timing, the charge curve, the crit conditions and sweep eligibility.
- Off-hand slot (F to swap).
- **Shield:** craftable, blocks frontal damage while raised, disabled by axe hits, shown in the off-hand.
- **Bow and arrows:** charge time, modern arrow physics, arrows stick in blocks and can be picked up.
- **Flint and steel:** lights fire; fire spreads and burns flammable blocks with modern rules.
- **Done when:** sword and axe cooldowns match modern timing, jump-attacks land crits with particles, you can block a zombie with a shield, kill a mob with a charged bow, and light a fire that spreads.

### - [x] M19 — Core redstone and rails
- Redstone dust (signal strength 15 with falloff), redstone torch, lever, buttons, pressure plates, redstone block, repeater (delay and locking), piston and sticky piston, iron and wooden doors and trapdoors, fence gates, note block, hopper, dropper, dispenser, TNT, redstone lamp.
- Comparators and observers are out of v1.1.
- **Rails and minecarts:** rail, powered rail, detector rail, activator rail; rideable minecart, chest minecart, hopper minecart.
- Updates run on the tick system in a deterministic order, with unit tests for wire power, repeaters and pistons.
- **Done when:** a piston door, a repeater clock and a powered rail line with a detector all work, and the redstone tests pass.

### - [x] M20 — More Overworld mobs
- Hostile: skeleton, creeper (explosions that break blocks), spider, cave spider, enderman (picks up blocks, teleports, drops ender pearls), slime, witch, drowned, husk, stray, phantom.
- Passive/neutral: cow, sheep (shearing, dyeable wool), chicken (eggs), wolf (tameable), cat, horse (rideable with saddle), rabbit, squid, cod/salmon, bee, fox, villager and iron golem (used in M23).
- Hand-built box models that match vanilla shapes, textured from `entity/` in the active pack. Modern spawn rules (light level 0 for hostiles, biome-specific spawns).
- **Line-of-sight attacks:** melee mobs (zombies, spiders, etc.) can only hit you if a raycast from their eye position to your hitbox is clear and within their modern attack reach. No hitting through walls, floors or corners. Ranged mobs (skeletons, witches) also need line of sight to shoot. Apply this to the v1.0 zombie too, and cover it with unit tests (a mob behind a wall or a 1-block ceiling can't attack).
- **Done when:** a full day/night cycle shows a natural mix of passive and hostile mobs, each behaving and dropping like modern Minecraft, and a zombie behind a wall or under a slab ceiling can't hit you.

### - [ ] M21 — More biomes
- Modern climate-based biome placement (temperature, humidity, continentalness, erosion, weirdness noise), using the full 256-block height for mountains and deep caves.
- Add: beach, stony shore, river, taiga, snowy plains, snowy taiga, ice spikes, jungle, bamboo jungle, swamp, mangrove swamp, savanna, badlands, birch forest, dark forest, windswept hills, meadow, grove, snowy slopes, jagged/frozen peaks, cherry grove, mushroom fields, frozen/cold/lukewarm/warm/deep oceans, and lush and dripstone caves.
- All the blocks these need (spruce, jungle, acacia, dark oak, mangrove, cherry and bamboo wood sets, terracotta, ice variants, mud, moss, dripstone, amethyst, etc.), with recipes.
- **Accurate ore generation by height**, following modern Minecraft's distributions adapted to our Y 0–255 world. Modern Y −64 to 63 is compressed into our Y 0 to 63, and everything at or above sea level keeps its modern Y. Ores use the modern triangular and uniform spreads, vein sizes and air-exposure reduction: coal (common high up, including in mountains), copper (mid-depth, more in dripstone caves), iron (deep, plus a band in mountains), gold (deep, extra in badlands), redstone and diamond (deepest, rarer when exposed to air), lapis (deep, peaking mid-low), emerald (mountain biomes only). Add deepslate and deepslate ore variants in the lowest layers. Write the final table in `DECISIONS.md` and unit-test that a sample of chunks matches the expected per-height counts.
- **Done when:** a seed shows every new biome with correct terrain, trees, blocks and mobs; ores appear at the right heights (diamonds near the bottom, emeralds only in mountains); and the perf test still shows 60+ FPS at render distance 9.

### - [ ] M22 — Experience, enchanting, and anvils
- XP orbs from mobs, mining, smelting and breeding; XP bar and levels with modern level costs; XP lost on death.
- Enchanting table with bookshelf power (up to 15), three offers, lapis cost, and the modern enchantment list for tools, weapons, armor, bows and fishing rods (where those exist).
- Enchanted books, anvils (combine, repair, rename, "Too Expensive!" cap), grindstone.
- Enchanting glint on enchanted items.
- **Done when:** you can enchant a diamond pickaxe with Efficiency, combine two enchanted books on an anvil, and the effects match modern behavior.

### - [ ] M23 — Structures and villages
- Structure system: templates stored as data files (e.g. `assets/structures/*.ron`), placement rules per biome, and randomized pieces joined jigsaw-style.
- **Claude Code builds the village pieces** (houses, farms, paths, wells, job-site buildings) to look like vanilla villages, for plains, desert, savanna, taiga and snowy variants.
- **Structure capture tool (creative only):** select two corners and save the region as a template, so Kenzo can replace any piece later.
- Villagers with professions and job sites, beds, daily schedule, breeding, and iron golems.
- **Trading:** farmer, librarian (enchanted books), armorer, toolsmith, weaponsmith, cleric and butcher at all 5 levels with restocking. No reputation-based pricing.
- Other Overworld structures: dungeons with spawners, mineshafts, desert and jungle temples, shipwrecks, ruined portals, and **strongholds** with an End portal room (needed in M27).
- **Done when:** a new world has villages you can walk into and trade in up to master level, and each listed structure generates with loot. **Stop: Checkpoint B.**

#### Checkpoint C — The Nether

### - [ ] M24 — The Nether: dimension and biomes
- Nether portal: obsidian frame of any valid size, lit with flint and steel or fire, linked portal travel at 8:1 coordinates, portal animation and transition.
- The Nether (height 128, bedrock floor and ceiling, lava ocean) with nether wastes, crimson forest, warped forest, soul sand valley and basalt deltas, and all their blocks.
- Nether mobs: ghast, zombified piglin, piglin (bartering with gold), hoglin, magma cube, strider (rideable).
- Beds explode in the Nether, and water evaporates.
- **Done when:** you can build a portal, visit each Nether biome, and return to the Overworld through the linked portal.

### - [ ] M25 — Nether structures
- Nether fortress (blazes, wither skeletons, nether wart), bastion remnants (piglins, piglin brutes, loot), ruined portals in the Nether.
- Items: blaze rods and powder, nether wart, wither skeleton skulls, gold, and netherite (ancient debris, smithing table upgrade with templates).
- **Done when:** you can find a fortress, kill blazes for blaze rods, and upgrade a diamond tool to netherite.

### - [ ] M26 — Brewing
- Brewing stand with blaze powder fuel, water bottles, cauldrons, and the modern potion tree: awkward base, then healing, regeneration, strength, swiftness, fire resistance, night vision, invisibility, water breathing, leaping, slow falling, poison, weakness, slowness, harming, and turtle master if turtles exist.
- Redstone extends duration, glowstone raises potency, and gunpowder makes splash potions (lingering optional).
- Status effect system with HUD icons, timers and particles, also used by golden apples and food.
- **Done when:** you can brew and drink a fire resistance potion and see its effect and timer. **Stop: Checkpoint C.**

#### Checkpoint D — The End and release

### - [ ] M27 — The End and beating the game
- **Eyes of ender:** crafted from ender pearl + blaze powder, thrown to point toward the nearest stronghold, sometimes breaking.
- End portal activation by filling all 12 frames.
- The End: main island of end stone, obsidian pillars with **end crystals** (some caged), the **ender dragon** (circling, strafing, perching on the fountain, dragon breath, healing from crystals, modern health and phases).
- On death: the **exit portal / end fountain** activates, the dragon egg appears, an End gateway opens, and XP drops.
- **Beating the game:** entering the exit portal plays the end poem and credits, then returns you to your Overworld spawn.
- **Done when:** you can go from a fresh survival world to killing the dragon and seeing the credits, without creative mode.

### - [ ] M28 — Outer End: End cities and elytra
- End gateways teleport to the outer islands; outer island generation with chorus plants and fruit.
- End cities and End ships with loot, shulkers (levitation bullets, shulker shells, shulker boxes that keep their contents).
- **Elytra:** found in End ships; gliding physics, firework rocket boosting, repair with phantom membranes, shown on the player model.
- **Done when:** you can reach an End city, take an elytra from an End ship, and glide and rocket-boost back.

### - [ ] M29 — Advancements and the recipe sweep
- Modern advancements: the Minecraft, Nether, The End, Adventure and Husbandry tabs, covering everything implemented. Toast pop-ups, an advancements screen (L), saved per world.
- Recipe sweep: every block and item has its modern crafting, smelting, stonecutter and smithing recipes, with any missing workstation blocks added. List anything intentionally skipped in `DECISIONS.md`.
- **Done when:** a survival playthrough unlocks advancements as expected, and an automated test confirms every craftable item has a recipe.

### - [ ] M30 — v1.1 performance and release
- Profile with all content loaded (Overworld, Nether, End), and fix hot spots.
- Rebuild the `.app`, set its version to 1.1.0, tag `v1.1.0`.
- **Done when:** 60+ FPS at render distance 9 in each dimension on the Mac mini, and the new `.app` launches by double-click and opens v1.0 worlds. **Stop: Checkpoint D.**

## 8. Decisions log (from Kenzo)

- Mobs: pig and zombie in v1.0; many more mobs in v1.1 (M20, M24, M25, M28).
- Survival and creative modes, chosen per world at creation (changed from survival-only after M5). No stats editing, cheats, or commands.
- Sounds come from Kenzo's Minecraft sample pack, downloaded per `docs/sounds.md`. Audio stays a placeholder in M11 until the files are downloaded.
- v1.0 ended at M13. v1.1 (M14–M30) was added after v1.0 shipped, targeting modern Java Edition.

## 9. v1.1 decisions (from Kenzo's grill-me answers)

- World height raised to 256 (Y 0–255) in M14.
- Enchanting (with XP and anvils) and brewing are both in v1.1 (M22, M26).
- Villager trading: main professions (farmer, librarian, armorer, toolsmith, weaponsmith, cleric, butcher) at all 5 levels with restocking; no reputation pricing.
- End content: dragon plus outer End, End cities, shulkers and elytra. The Wither and ocean monuments wait for a later version.
- Third-person view (F5) and the full player model are in M16.
- Redstone: core set in M19 (no comparators or observers).
- Village buildings: Claude Code builds vanilla-like pieces first; Kenzo can replace any of them with the creative capture tool.
- Reviews: checkpoints A–D instead of every milestone (see §7.2).
- Added after grilling: accurate block shapes and gravity blocks (M14), a performance test harness (M14) with a gate on every later milestone, line-of-sight mob attacks (M20), height-accurate ore generation (M21), and exact modern sword/axe cooldowns, damage and critical hits (M18).
- Player skin: a Steve-style 64×64 skin PNG that Kenzo drops into `assets/skins/`.
