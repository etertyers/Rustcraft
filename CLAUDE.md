# CLAUDE.md — Rustcraft (a simplified Minecraft clone)

> Draft v0.3. All open questions are settled (questions.md plus Kenzo's follow-up answers).

## 0. Locations (read first)

- **Project root:** `~/code/rustcraft` (the folder containing this file). All code, `Cargo.toml`, `docs/`, `assets/` and the git repo live here. Run every command from this folder, even if Claude Code was started in a parent folder like `~/code`.
- **Planning folder:** `~/Documents/rustcraft`. It is source material only: copy files *from* it into the project root, but never create, build or commit anything there. Any instruction that seems to target the planning folder (or `documents/rustcraft`) applies to `~/code/rustcraft` instead.
- `~/Documents` is privacy-protected on macOS. If reading it fails with "Operation not permitted", stop and ask Kenzo to copy the files over or grant access; don't work around it.

## 1. What we're building

Rustcraft (working name) is a single-player, survival-only Minecraft clone (no creative mode, no cheats) for macOS (Apple Silicon), written in Rust. It follows **Minecraft Java Edition 1.0.0**: the same blocks, items, recipes and, where practical, the same mechanics and physics constants. The core loop is an infinite procedurally generated world, mining and placing blocks, inventory, survival (health, hunger, mobs, day/night), crafting, and saving.

**Reference list:** `docs/blocks-and-items.md` (copied from the planning folder) lists every 1.0.0 block and item with its original numeric ID. It is the source of truth for what exists in the game.

**Faithfulness:** mechanics should match Minecraft 1.0.0 as closely as practical. When unsure of an exact value, use the best-known Minecraft value, write it in `docs/DECISIONS.md`, and keep it as a named constant in `src/config.rs`.

**Out of scope:** multiplayer, creative mode, cheats/commands, mod support, Windows/Linux builds (wgpu may run there, but don't spend effort on it). Blocks for redstone, the Nether, the End, enchanting and brewing are registered and placeable, but their special behavior (circuits, portals, dimensions, enchanting, potions) is not planned. Nothing after M13 is planned.

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
screencapture -x docs/screenshots/m<N>-<what>.png   # milestone screenshots
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
    gen/             # terrain, biomes, caves, ores, trees
    manager.rs       # load/unload around player, worker threads, dirty tracking
    light.rs         # sky light + block light propagation
    fluid.rs         # water/lava flow (M12)
    save.rs          # region files
  player/            # controller, physics, collision (AABB), raycast, interaction, stats
  entity/            # mobs, item drops
  inventory/         # items, stacks, inventory, crafting recipes
  ui/                # HUD, hotbar, inventory screen, menus (egui)
  audio/
assets/
  textures/          # Kenzo's PNGs from the planning folder's assets/ → packed into an atlas at startup
  sounds/            # Kenzo's sound files, named per docs/sounds.md
  data/              # blocks.ron, items.ron, recipes.ron
docs/
  blocks-and-items.md
  sounds.md
  DECISIONS.md
  screenshots/
```

**Key conventions**
- **World size:** Y is up. Chunks are 16×16 columns made of vertical 16³ sections. World height is **128** (Y 0..127, 8 sections), with sea level at Y 63 (matches 1.0.0). Default render distance is **9 chunks**. Both are constants in `config.rs`; if 60 FPS isn't reachable, lower them, note it in `DECISIONS.md`, and tell Kenzo.
- **IDs:** Blocks and items use their original 1.0.0 numeric IDs from `docs/blocks-and-items.md` (`u16`). Properties (solid, transparent, hardness, drops, texture per face, light emission, tool type) live in `assets/data/blocks.ron`. Never scatter block behavior across `match` arms.
- **Textures:** Use the PNGs Kenzo provided in `assets/textures/`. At M1, inspect the folder, map filenames to blocks/items in the RON files, and list any block with no matching texture in `DECISIONS.md`. Missing textures render as a magenta/black checkerboard so they're easy to spot. Don't generate or download other art.
- **Timing:** Game logic runs at a fixed 20 ticks/second, like Minecraft. Rendering runs at the display refresh rate with interpolation.
- **Threading:** Chunk generation and meshing run on worker threads. The main thread never blocks on them; it uploads finished meshes to the GPU.
- **Physics constants (Minecraft values, per tick):** gravity 0.08 blocks/tick², vertical drag ×0.98 per tick, jump velocity 0.42, walk ≈4.317 blocks/s, sprint ≈5.612, sneak ≈1.31, player hitbox 0.6×1.8, eye height 1.62, step height 0.5, block reach 5 (survival), 20 health points (10 hearts), 20 hunger points.

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

### - [ ] M0 — Project scaffold
- `cargo init --name rustcraft` in the project root `~/code/rustcraft` (see §0; don't create a nested project folder), crate list from §2 pinned, `.gitignore`, dev profile tweaks, `docs/DECISIONS.md`, `docs/blocks-and-items.md` and `docs/sounds.md` (copied from the planning folder), `assets/textures/` with Kenzo's PNGs (copied from the planning folder's `assets/`).
- GitHub remote set up; `main` pushed.
- Empty winit window that opens, clears to sky blue via wgpu, and closes cleanly.
- **Done when:** `cargo run` shows a blue window; clippy/fmt/test pass; tagged `m0`.

### - [ ] M1 — Render a cube, fly around
- wgpu pipeline, WGSL shader, depth buffer, window resize handling.
- Free-fly camera: WASD + mouse look, cursor grab (Esc releases), Space/Shift up/down.
- Texture atlas built from `assets/textures/`; texture-to-block mapping table; a single textured cube.
- F3 debug overlay (egui): FPS, position, direction.
- **Done when:** a textured cube renders correctly from all angles at 60+ FPS.

### - [ ] M2 — Chunks, meshing, and the full block registry
- `blocks.ron` with **every block from `docs/blocks-and-items.md`** (IDs 0–122): properties, hardness, per-face textures, transparency, light emission.
- `Chunk` storage and a flat test world of several chunks.
- Mesher with face culling (only faces bordering transparent blocks) and per-face textures (grass top/side/bottom). Non-cube shapes (torches, flowers, slabs, stairs, fences, doors, cross-shaped plants) can render as simple placeholders at first, but each must be listed in `DECISIONS.md` as pending.
- Cross-chunk neighbor lookup so borders don't render hidden faces. Frustum culling of chunks.
- **Done when:** a flat world renders at 60+ FPS; a debug "block gallery" world shows every block; unit tests cover face counts.

### - [ ] M3 — Infinite world generation (4 biomes)
- Seeded noise terrain with world height 128 and sea level 63.
- Biomes: **ocean, plains, forest, desert**, with blended borders. Oceans are filled with still water (ID 9) that doesn't flow yet.
- Caves (3D noise), ores at 1.0.0 depths (coal, iron, gold, lapis, redstone, diamond), oak and birch trees, flowers and tall grass, sand and sandstone in deserts, cactus and sugar cane, bedrock floor.
- Chunk manager: load/unload around the player within render distance 9, worker threads for gen + meshing, priority by distance.
- Upgrade to greedy meshing if vertex counts or FPS demand it.
- **Done when:** flying in one direction for 2 minutes shows all four biomes with continuous terrain and no stalls; the same seed gives the same world.

### - [ ] M4 — Player physics and block interaction
- Walking with Minecraft constants from §4: gravity, jump, AABB collision, step-up, sprint, sneak (no falling off edges). The M1 free-fly camera stays as a dev tool in debug builds only (`cfg(debug_assertions)`); release builds have no flying.
- DDA voxel raycast (reach 5) for the target block, wireframe highlight outline.
- Left-click breaks (instant for now), right-click places the selected block; can't place inside the player.
- Remesh only affected chunks (and neighbors on borders).
- **Done when:** you can walk, jump, dig a staircase down, and build a house without clipping; movement speeds match §4 in unit tests.

### - [ ] M5 — Lighting
- Sky light (sunlight propagation from top) and block light (torches, glowstone, lava, jack o'lanterns, etc. at their 1.0.0 levels) using BFS flood fill, including removal.
- Smooth lighting / ambient occlusion per vertex.
- **Done when:** caves are dark, torches light them, and breaking/placing blocks updates light correctly across chunk borders.

### - [ ] M6 — Saving and loading
- Region files (32×32 chunks per file), only modified chunks saved, compressed.
- Save player position, stats, inventory, world seed, time of day. Autosave every few minutes and on quit.
- Main menu: create world (name + seed), load world, quit.
- **Done when:** edits survive quit-and-relaunch; round-trip tests pass.

### - [ ] M7 — Items, inventory, and survival mining
- `items.ron` with every item from `docs/blocks-and-items.md`; stacks (64, or 1/16 where 1.0.0 differs); dropped item entities with pickup.
- Hotbar (1–9, scroll wheel), full inventory screen (E) with click and drag stack moving.
- Mining time from 1.0.0 hardness × tool type/tier (wood/stone/iron/gold/diamond), tool durability, correct-tool requirements for drops, 1.0.0 drop rules.
- **Done when:** you can mine stone with a pickaxe, get cobblestone, and see tool durability drop.

### - [ ] M8 — Environment
- Day/night cycle (20-minute day), sun and moon, sky color gradient, sky light level tied to time.
- Transparent render pass for water, ice and glass; swimming and buoyancy in still water; underwater fog tint.
- Fog at render distance edge; clouds.
- **Done when:** a full day cycle looks right and you can swim in the ocean.

### - [ ] M9 — Health, hunger, and mobs
- Health, hunger and saturation, fall damage, drowning, lava damage, respawn at spawn point; hearts and hunger HUD.
- Entity system for mobs: AABB physics shared with the player, box-model rendering.
- Mobs: **pig** (passive, wanders, drops porkchop) and **zombie** (hostile, spawns in darkness, chases and attacks, burns in daylight, drops rotten flesh), with 1.0.0 behaviors. Build the mob code so more 1.0.0 mobs can be added later from data plus a small AI module; adding them isn't planned yet.
- Melee combat with knockback and critical hits; bow and arrows; eating restores hunger.
- **Done when:** you can survive a night, kill a zombie, and eat a porkchop from a pig.

### - [ ] M10 — Crafting and smelting
- 2×2 inventory grid and 3×3 crafting table; shaped and shapeless recipes from `recipes.ron`.
- **Most 1.0.0 recipes** for everything in scope: building blocks, tools, weapons, armor, food, torches, chests, doors, beds, buckets, etc. Skip recipes for deferred systems (redstone, enchanting, brewing) and list them in `DECISIONS.md`.
- Furnace with 1.0.0 fuels and smelting recipes.
- Chest block with persistent storage. Beds set the spawn point and skip the night.
- **Done when:** the wood → stone → iron → diamond progression works in a fresh survival world.

### - [ ] M11 — Audio and polish
- **[PLACEHOLDER: sound effects and music, waiting on Kenzo]** Don't implement audio yet. Kenzo is still downloading the files listed in `docs/sounds.md`. Until Kenzo confirms the files are in `assets/sounds/` and replaces this line with real instructions, skip all audio work, add no audio crate code, and don't treat missing sound as a failure. If you reach M11 before then, do the rest of M11 and note in the PR that audio is pending.
- Settings menu: render distance, FOV, mouse sensitivity, volume, vsync. Pause menu.
- Block-breaking particles and crack overlay.
- **Done when:** a fresh player can go from a new world to iron tools with no debug UI (sound is excluded while the placeholder above stands).

### - [ ] M12 — Fluids
- Flowing water and lava with 1.0.0 spread rules (water 7 blocks, lava 3 in the Overworld), source blocks, infinite water sources, lava + water → obsidian/cobblestone.
- Buckets pick up and place fluids. Current pushes entities.
- **Done when:** pouring water into a hole spreads and fills like Minecraft, and lava + water makes cobblestone and obsidian.

### - [ ] M13 — Performance and release
- Profile (Instruments / `cargo flamegraph`), fix hot spots, reduce allocations in meshing.
- Release build bundled as a macOS `.app` (e.g. `cargo-bundle`), app icon.
- **Done when:** 60+ FPS at render distance 9 on the Mac mini (stretch goal 12); `.app` launches by double-click.

## 8. Decisions log (from Kenzo)

- Mobs: pig and zombie only for now; more default mobs later (unscheduled).
- Survival only. No stats editing, cheats, or creative mode.
- Sounds come from Kenzo's Minecraft sample pack, downloaded per `docs/sounds.md`. Audio stays a placeholder in M11 until the files are downloaded.
- Nothing after M13 is planned.
