# CLAUDE.md — Rustcraft (a Minecraft Java Edition clone)

> Rules every session needs. Milestone entries live in `docs/milestones/` (see §6), the file map in `docs/architecture.md`, decisions in `docs/DECISIONS.md`. Nested CLAUDE.md files hold folder rules: `src/world/worldgen/CLAUDE.md`, `src/render/CLAUDE.md`, `src/game/CLAUDE.md`.

## 1. What we're building

Rustcraft is a Minecraft clone for macOS (Apple Silicon), written in Rust with wgpu. It follows **modern Minecraft Java Edition (use 1.21.x as the reference)**: the same blocks, items, recipes, mobs, biomes and structures, and the same mechanics and physics constants where practical. Where an older value and a modern one differ, modern wins.

**As built today:**
- An infinite procedurally generated world in the Overworld, the Nether and the End (with the ender dragon), at modern height (Y −64 to 319).
- Game modes: survival, creative, adventure and spectator. Mode is chosen per world; `/gamemode` changes it only in worlds with **Allow Cheats** on.
- LAN multiplayer for up to 8 players (Open to LAN, a server and clients), with chat.
- Commands with vanilla Java syntax (selectors, `/execute`, scoreboards, data packs, command blocks). Cheat commands need Allow Cheats, or an operator on LAN.
- Resource packs, an options menu with rebindable keys, advancements, Lua mods (with a bundled Fullbright mod), farming and fishing.

**Reference list:** `docs/blocks-and-items.md` lists every 1.0.0 block and item with its original numeric ID. Modern names are in `assets/data/blocks.ron` and `items.ron`.

**Mojang's files:** vanilla textures, models, sounds, language files and world-gen data are downloaded by `scripts/fetch-vanilla-assets.sh` into the git-ignored `assets/vanilla/` folder and used only as the lowest fallback layer, below Kenzo's assets and any resource pack. Never commit them. They are bundled into the local .app build.

**New content gets added with its recipes.**

**Out of scope:** online (internet) multiplayer, accounts and dedicated servers, running Java (Fabric/Forge) mods, Windows/Linux builds (wgpu may run there, but don't spend effort on it), comparators and observers, the Wither, and ocean monuments.

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
| Lua mods (v1.2) | `mlua` (Lua 5.4, `vendored` feature) | Sandboxed mod scripts from M39 on |
| Networking (v1.2) | `std::net` (TCP + UDP multicast) + `bincode` | LAN play from M36 on; no async runtime needed |

**Version policy:** At M0, pin the latest stable versions of these crates in `Cargo.toml` and commit `Cargo.lock`. Do not upgrade crates mid-milestone. If an API doesn't match your memory, check the docs for the pinned version (`cargo doc --open` or docs.rs) rather than guessing.

**No game engine.** Do not add Bevy, Fyrox, or similar. The renderer and game loop are ours.

## 3. Commands

```bash
cargo run --release            # play (debug builds are too slow for real chunk loads)
cargo run                      # debug build
cargo test                     # all tests
cargo clippy --all-targets -- -D warnings    # must pass before every commit
cargo fmt                      # must be applied before every commit
git config core.hooksPath .githooks   # once per clone: the pre-commit hook runs fmt --check and clippy
RUST_LOG=rustcraft=debug cargo run --release
cargo run --release -- --bench [seconds] [--distance <chunks>]   # timed flight, prints fps
cargo run --release -- --perf-test [--runs <n>] [--label <name>] [--row <name>] [--nether|--end|--underwater]   # writes docs/perf/<label>.md and .json
scripts/add_bench.py docs/perf/m<N>.json   # adds cargo bench's medians to the results
scripts/perf_gate.py docs/perf/m<N>.json    # compares with docs/perf/baseline.json; the exit code is the gate
UPDATE_GOLDEN=1 cargo test --test golden   # rewrite tests/golden/hashes.ron (only per the golden rule in §7)
cargo bench                    # chunk generation, lighting, meshing and fluid benchmarks
cargo run --release -- --end         # an unsaved test world in the End, dragon circling (also --nether)
cargo run --release -- --creative    # a new creative world "Creative test", opened without the menu
cargo run --release -- --world <name> [--creative] [--seed <n>]   # open that saved world, or create it
cargo run --release -- --help        # every flag (note the `--`: without it cargo rejects the flag itself)
cargo run --release -- --shot <name>   # capture a scripted scene to docs/screenshots/<name>.png (--shot list)
# Flags: --creative, --world <name>, --seed <n>, --flat, --gallery, --nether, --end, --underwater,
#   --view first|back|front, --pack <name>, --bench [s], --distance <n>, --perf-test, --shot <name> [--out <file>],
#   --name <player>, --join <ip:port>, --lan [port], --lan-test <n>, --assets <dir>, --help
# Tools (usage text in --help): --spawn-test, --flower-test, --structure-test, --nether-map, --cave-test,
#   --cave-openings, --cave-slice, --write-structures, --asset-report, --gen-placeholders
cargo run --release -- --spawn-test --seed 12345   # one night in plains, prints monster and animal counts every 1,000 ticks
scripts/bundle-macos.sh        # build target/release/bundle/Rustcraft.app (icon from assets/*.svg); refuses an incomplete assets/vanilla, then runs scripts/check-bundle.sh
scripts/check-vanilla.sh       # is assets/vanilla complete (sounds, font, textures)?   scripts/check-downloads.sh   # are Kenzo's 5 packs and 9 mod jars in assets/downloads/?
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

Module map (`src/` unless noted). The full file list is `docs/architecture.md`; `ls` a folder for the truth.

```
main.rs, lib.rs, app.rs   # entry (flags and tools), the library the benches use, the winit loop and State
cli.rs, bench.rs, shots.rs, lantest.rs   # flags and usage text; --bench; --shot scenes; --lan-test bots
config.rs            # every constant: chunk size, world height, render distance, physics, combat
assets.rs, options.rs, level.rs, worldlist.rs   # the asset root (--assets, else the .app's Resources, else the repo; never the current folder), the asset report; where saves live; options.ron; a world folder's level.ron; the Select World list
world/               # blocks, chunks, light, meshing, fluids, saves, redstone, pistons, dimensions, portals
  block.rs, blocks.rs, props.rs   # the registry, generated ID constants, block state properties
  chunk.rs, light.rs, mesh.rs, manager.rs   # 16x16x16 sections, light, chunk to vertices, load/unload and workers
  save.rs            # region files
  worldgen/          # climate and biomes, terrain, caves, ores, features, the Nether and the End (nested CLAUDE.md)
  structure/         # placement, templates (RON), jigsaw joining, loot, villages, fortresses, strongholds, End cities
cem/                 # OptiFine custom entity models (.jem) and their animation expressions (M58)
render/              # wgpu pipelines, atlas, camera, shaders (*.wgsl), models, hand, sky (nested CLAUDE.md)
look.rs, figure.rs   # everything texture-derived (rebuilt on pack change); the player as drawn
pack/                # resource packs: layers, animations, block and item models
player/              # controller, physics, collision, raycast, interaction, stats, effects, flying
entity/              # mobs and their AI, items, projectiles, boats, minecarts, the dragon, villagers
inventory/           # items, stacks, crafting, enchanting, brewing, anvils, creative tabs, recipe book
game/                # what happens each tick, on the server: mobs, redstone, commands, farming, fishing, sync, sounds (nested CLAUDE.md)
server.rs, client.rs, net/   # the server, the client link (prediction), messages, TCP and in-process transports, LAN
mods/, game/modding.rs   # the Lua runtime and API, the server's mod host
ui/                  # HUD, screens, menus, chat, options, creative and recipe book (egui)
audio/               # sound events, the merged sounds.json table, kira playback
advancements.rs, credits.rs, placeholders.rs   # advancement tree, End poem and credits, generated placeholder blocks and items
build.rs             # turns blocks.ron and items.ron names into constants
benches/             # criterion: chunk generation, lighting, meshing, fluids
assets/              # data/*.ron, structures/, textures/ (Kenzo's), skins/, vanilla/ (git-ignored)
mods/, examples/mods/   # bundled Fullbright; the Ruby example (docs/modding.md)
scripts/             # fetch-vanilla-assets.sh, bundle-macos.sh
docs/                # milestones/, architecture.md, DECISIONS.md, perf/, worldgen.md, modding.md, screenshots/
```

**`docs/DECISIONS.md` is a log: search it with grep, never read it whole.**

**Key conventions**
- **World size:** Y is up. Chunks are 16×16 columns made of vertical 16³ sections. Chunks hold **Y −64..319** (384 blocks, 24 sections; `config::MIN_Y` −64, `MAX_Y` 320 exclusive), with sea level at Y 63; the Nether and End keep 0..127 and 0..255 inside that (`Dimension::min_y`/`max_y`). Blocks are addressed by world Y everywhere (`Chunk::get(x, y: i32, z)`). Default render distance is **9 chunks** (`config::RENDER_DISTANCE`). **Never lower render distance, world height or visual quality to pass a performance target; stop and report instead.**
- **IDs:** Since M14, blocks and items have modern Java Edition names (`minecraft:oak_log`), defined in `assets/data/blocks.ron` and `items.ron`; `build.rs` turns the names into constants (`world::blocks::OAK_LOG`, `inventory::items::STICK`). Runtime IDs are file positions and saves store names. A `BlockState` is a block plus modern block state properties (`world::props`). `assets/data/legacy.ron` maps v1.0's numeric 1.0.0 IDs to the new names. Properties (solid, transparent, hardness, drops, texture per face, light emission, tool type, placement, shape) live in `blocks.ron`. Never scatter block behavior across `match` arms; a block that behaves like another names it with `acts_as` (every wooden button acts as `oak_button`, every kind of leaves as `oak_leaves`), and code matches on `BlockState::kind()`.
- **Textures (M15):** textures are looked up by modern resource location (`block/stone`) through `pack::Resources`: selected packs from `resourcepacks/` over Kenzo's `assets/textures/` over the vanilla fallback in `assets/vanilla/`; older names are in `assets/data/texture_names.ron`. `look::Look` builds everything texture-derived and is rebuilt when the packs change. Missing textures render as a magenta/black checkerboard so they're easy to spot. Don't generate or download other art.
- **Timing:** Game logic runs at a fixed 20 ticks/second, like Minecraft. Rendering runs at the display refresh rate with interpolation.
- **Threading:** Chunk generation and meshing run on worker threads. The main thread never blocks on them; it uploads finished meshes to the GPU.
- **Physics constants (Minecraft values, per tick):** gravity 0.08 blocks/tick², vertical drag ×0.98 per tick, jump velocity 0.42, walk ≈4.317 blocks/s, sprint ≈5.612, sneak ≈1.31, player hitbox 0.6×1.8 (sneaking 1.5, swimming and crawling 0.6), eye height 1.62 (1.27, 0.4), step height 0.6, block reach 5 (survival), 20 health points (10 hearts), 20 hunger points.

## 5. Git workflow

**Remote:** the repo lives on GitHub (`origin`). `main` is always playable.

**Branching:** one branch per milestone, `m<N>-<slug>` (e.g. `m3-worldgen`). Push it to `origin` as you go. Work outside a milestone uses a descriptive branch name and the scope of its subject (e.g. `foundation`).

**Milestone finish:**
1. Last commit on the branch: `docs(m<N>): mark milestone <N> complete`.
2. Open a PR titled `M<N>: <milestone name>` into `main`, or into the previous milestone's branch when stacked (see "Milestone rules"), with a checklist of acceptance criteria.
3. Stop and wait for Kenzo's manual check.
4. After approval, merge with a merge commit (no squash), then tag `m<N>` and `git push origin m<N>`.

**Commit format** (Conventional Commits, milestone as scope):

```
<type>(m<N>): <imperative summary, ≤72 chars>

<optional body: what and why, wrapped at 72>
```

Types: `feat`, `fix`, `perf`, `refactor`, `test`, `docs`, `chore`, `build`. Example: `fix(m4): stop player clipping through blocks at chunk borders`.

**Cadence:** commit after each working sub-step (roughly every 30–150 lines of meaningful change) and push regularly. Never bundle unrelated changes. Never commit build output (`target/`) or saves (`saves/`). Screenshots go only in `docs/screenshots/`.

## 6. Milestones

Entries are in `docs/milestones/`, one file per version, each starting with its intro and checkpoint table. Read only the entry you are working on (`grep -n "### - \[.\] M<N> " docs/milestones/*.md`).

| Version | Milestones | Checkpoints | File | Status |
|---|---|---|---|---|
| 1.0 | M0–M13 | none (reviewed after each) | `docs/milestones/v1.0.md` | released |
| 1.1 | M14–M30 | A–D | `docs/milestones/v1.1.md` | released (`v1.1.0`) |
| 1.2 | M31–M42 | E–G | `docs/milestones/v1.2.md` | released (`v1.2.0`) |
| 1.3 | M43–M53 | H–J | `docs/milestones/v1.3.md` | released (`v1.3.0`) |
| 1.4 | M54–M58 | K | `docs/milestones/v1.4.md` | released (`v1.4.0`); Low/High miss their budgets by a few percent |
| 1.5 | M59–M69 | L–N | `docs/milestones/v1.5.md` | in progress (Checkpoint L: M59–M62) |

After M30 merges, tag `v1.1.0` and rebuild the `.app`; after M42 merges, tag `v1.2.0` and rebuild the `.app`; after M53 merges, tag `v1.3.0`; after M58 merges, tag `v1.4.0` and rebuild the `.app`.

## 7. Milestone rules

The rules in force. Where the old rules conflicted, the newer won (v1.2 over v1.1 over v1.0).

1. **One milestone at a time, in order.** Read its goals and acceptance criteria first. Don't start features from later milestones.
2. **Step 0: measure before changing.** Where an entry has a Step 0, run it and record the numbers (one line per item, in the first commit body and the PR) before anything changes.
3. **One PR per milestone**, titled `M<N>: <name>`. Within a checkpoint, branch each milestone from the previous milestone's branch (stacked). **Never merge without Kenzo.**
4. **At the end of a checkpoint, stop.** Post a summary table (milestone, pass/fail, PR link, known issues) and wait. After Kenzo approves, merge the PRs into `main` in order with merge commits, tag each `m<N>`, and push the tags.
5. **If something blocks a milestone** (a design question, a failing test you can't fix), stop early and ask.
6. **Small, compilable steps.** Every commit builds and passes `cargo clippy --all-targets -- -D warnings`, `cargo fmt --check` and `cargo test`.
7. **Test the logic, not the GPU.** Unit-test pure code: chunk indexing, meshing face counts, raycasts, collision, physics, inventory, crafting, save round-trips, light. Rendering is verified by running the game.
8. **At most one screenshot per milestone.** Verify with `cargo test`, logs, printed counts and test flags instead. Take one at the end, of the most visual acceptance criterion, preferably with `--shot`, and open it once to check it. If nothing is visual, take none. Never take screenshots while debugging.
9. **Kenzo does the visual checks.** Each PR lists the in-game checks for Kenzo, one line each.
10. **Multiplayer-ready from M36 on.** Every new feature runs on the server and syncs to clients, and includes at least one test with a server and two in-process clients that checks they agree.
11. **Vanilla values are written into each milestone** where they matter. Use them rather than fetching the wiki. If a value isn't given, use the best-known modern Java value, record it in `docs/DECISIONS.md` (one line: date, decision, reason) and keep it as a named constant in `config.rs`.
12. **Performance:** 60+ FPS at render distance 9 on the M-series Mac mini, with FPS, position, biome, chunk count and facing in the F3 overlay. At the end of every milestone run `cargo bench` and `cargo run --release -- --perf-test --runs 3`, add the benchmarks with `scripts/add_bench.py docs/perf/m<N>.json`, then run `scripts/perf_gate.py docs/perf/m<N>.json`. The script's exit code decides, against the fixed `docs/perf/baseline.json` (not the previous milestone). Paste its table into the PR and `docs/perf/m<N>.md`. If it fails, fix it before moving on, or stop and ask. Only use `--update-baseline --accept "<reason>"` when Kenzo agreed in the milestone entry or in chat.
13. **Update this file** when a convention changes, and mark the milestone checkbox when its acceptance criteria are met.
14. **Golden hashes** (`tests/golden.rs`, `tests/golden/hashes.ron`) cover blocks, light and mesh of fixed chunks in all three dimensions. Only update them when the milestone's entry says that output is meant to change, and say in the PR which hashes changed and why.
15. **Model recommendation:** at the end of each milestone, recommend Sonnet or Opus for the next one. Opus only for world generation, structures, or complex systems like the Ender Dragon fight; otherwise Sonnet.

## 8. Sources for numbers

Every numeric target in a milestone says where it comes from: vanilla code or data, a measurement, or Kenzo's preference. Never tune code toward a number with no source; ask instead. (M48's invented surface-opening targets are the example; M53 removed them.)

## 9. Kenzo's decisions

### v1.0 (later subheadings win where they differ)

- Mobs: pig and zombie in v1.0; many more mobs in v1.1 (M20, M24, M25, M28).
- Survival and creative modes, chosen per world at creation (changed from survival-only after M5). No stats editing, cheats, or commands.
- Sounds come from Kenzo's Minecraft sample pack, downloaded per `docs/sounds.md`. Audio stays a placeholder in M11 until the files are downloaded.
- v1.0 ended at M13. v1.1 (M14–M30) was added after v1.0 shipped, targeting modern Java Edition.

### v1.1 (from Kenzo's grill-me answers)

- World height raised to 256 (Y 0–255) in M14.
- Enchanting (with XP and anvils) and brewing are both in v1.1 (M22, M26).
- Villager trading: main professions (farmer, librarian, armorer, toolsmith, weaponsmith, cleric, butcher) at all 5 levels with restocking; no reputation pricing.
- End content: dragon plus outer End, End cities, shulkers and elytra. The Wither and ocean monuments wait for a later version.
- Third-person view (F5) and the full player model are in M16.
- Redstone: core set in M19 (no comparators or observers).
- Village buildings: Claude Code builds vanilla-like pieces first; Kenzo can replace any of them with the creative capture tool.
- Reviews: checkpoints A–D instead of every milestone (see "Milestone rules").
- Added after grilling: accurate block shapes and gravity blocks (M14), a performance test harness (M14) with a gate on every later milestone, line-of-sight mob attacks (M20), height-accurate ore generation (M21), and exact modern sword/axe cooldowns, damage and critical hits (M18).
- Player skin: a Steve-style 64×64 skin PNG that Kenzo drops into `assets/skins/`.

### v1.2 and v1.3 (from Kenzo's grill-me answers)

- v1.2 is mainly the v1.1 playtest bug fixes (M31–M34), plus options/keybinds, LAN, chat, mods, farming and fishing.
- The protection bug: swords currently CAN take Protection and must not.
- Mods: data mods plus Lua scripting (`mlua`), not Java mods. Fullbright ships as a bundled Lua mod with a G toggle key.
- LAN multiplayer: the full game in every dimension, up to 8 players, via vanilla-style Open to LAN.
- Chat with full vanilla-style commands, but only in worlds with Allow Cheats on; harmless commands always work.
- World height extended to modern Y −64 to 319 (M34) so caves and ores copy modern generation; old worlds get new depth below Y 0.
- Farming: every crop, bone meal and composters, animal breeding, and farmer villagers who harvest and replant.
- Sugar cane follows the vanilla placement rule (no gravel).
- The head-clipping bug is the inventory player picture.
- Order: bug fixes first, then the client/server split and LAN before chat, mods, farming and fishing, so those are built multiplayer-ready.
- At most one screenshot per milestone (usage cost); Kenzo does the visual checks by hand.
- v1.3 (M43–M45) expands commands to vanilla Java's full set, including spectator and adventure mode, /execute, scoreboards, teams, data-pack functions and command blocks.
- Cheat commands still need Allow Cheats (or operator on LAN); command blocks are op-only in creative.
- Checkpoint I (M46–M49) fixes Kenzo's v1.3 playtest bugs: spectator physics and flying sprint, boats, ocean color, jukeboxes, caves rebuilt to vanilla 1.18+, Nether basalt, and /locate in every dimension.
- Flying players (creative flight and spectators) ignore water like vanilla; spectators are never pushed by anything.
- Cave entrances follow vanilla's frequency; the earlier "keep surface caves rare" rule is dropped because it left survival players with no way into the caves.
- Checkpoint J (M50–M52) fixes Kenzo's second v1.3 playtest: the boat camera, stray characters when screens open, timed HUD messages, underwater visibility, and /locate bastion; and adds strongholds with eyes of ender so survival can be beaten.
- Caves match vanilla 1.21.11's world-gen data term by term, checked by a test against the downloaded JSON (assets/vanilla/data, never committed); M48's made-up surface-opening targets are removed (M53).
- "Oceans are too dark" meant underwater visibility, not water color. Water colors stay vanilla (M47); underwater fog follows vanilla's 96-block fog with water vision (M51).

### v1.4 (the trailer look)

- v1.4 adds the Trailer look (Checkpoint K, M54–M58): native wgpu effects in the style of the trailers and Complementary Reimagined, with a quality profile (Low/Medium/High) and per-effect toggles. It's the default; F12 switches to the Vanilla look, which stays identical to M53 (checked against reference images).
- Rustcraft doesn't load OptiFine/Iris shader packs: they target vanilla's OpenGL pipeline and would need a compatibility layer that is slower and months of work. The look is written natively in WGSL instead. Resource packs with OptiFine CEM models and animations (Fresh Animations) are supported (M58).
- Each look has its own resource pack list. The Trailer list starts with Fresh Animations, Barebones Better Leaves and Bare Bones if they're in resourcepacks/; none of the three is ever bundled or committed (their licenses forbid redistribution).
- Frame-rate budgets on Kenzo's Mac mini M1 at 1080p, render distance 9: Vanilla within 3% of M53; Trailer Low ≥ 100 FPS average / 50 at the 1% low; Medium ≥ 75 / 45; High ≥ 60 / 40. Each Trailer effect has a GPU-time share, measured with timestamps every milestone.

### v1.5 (assets, styles, mods, caves and speed)

- v1.5 (M59–M69) fixes Kenzo's v1.4 playtest and builds the Barebones style: assets in the .app, broken textures, three styles with one Shaders switch, native versions of the trailer-look mods, the Mods screen, caves that keep the grass, and speed on Apple silicon.
- Vanilla's files (minecraft-assets 26.3) have every texture, sound and the font, so Rustcraft draws no replacement art. Missing assets are a lookup or bundling bug, or a download to ask Kenzo for. Mojang's files, the packs and the mod jars stay git-ignored and are bundled only into Kenzo's own .app.
- The game finds its assets from the executable's location (the .app's Contents/Resources), never the current directory. scripts/bundle-macos.sh refuses to build without the vanilla layer's sounds and font, and scripts/check-bundle.sh verifies every build (M59).
- Kenzo's downloads live in assets/downloads/ (resourcepacks/ and mods/, git-ignored). When something needed is missing, the session stops and asks Kenzo with links, never downloads packs or mods itself and never works around a missing file.
- Kenzo checks textures with the Texture Viewer (Options → Video Settings → Texture Viewer, or /textures). His marks in texture-marks.ron are the input for texture fixes (M60).
- Three built-in styles, grouped by performance: Barebones (Bare Bones, Barebones Better Leaves, Fresh Animations, Bare Bones x Fresh Animations), Vanilla, and Realistic (Patrix 32x, plus Fresh Animations if its mobs fit), with one Shaders switch (Low/Medium/High) that works with any style. Barebones and Realistic have the same extras. F12 toggles Shaders and Shift+F12 cycles the style. The default is Barebones with Shaders on at Medium. Rustcraft supports the OptiFine features Patrix uses (connected, natural and random textures, emissive) but not PBR maps (M62).
- The Barebones style includes every mod from lowresbones' "Make Minecraft look like the trailer (UPDATED)" video except BSL Shaders, Better Combat and Nyf's Spiders. Each is rebuilt natively (Rustcraft doesn't run Java mods), from its behavior, never its code, with its assets read from its jar. Client-side mods are on by default in the Barebones and Realistic styles. Better Combat and Nyf's Spiders were removed because they change gameplay: every extra is client-side, and gameplay stays vanilla (M63–M65).
- The Mods screen recognizes what a file is. It routes resource packs (like Fresh Animations) to Resource Packs and data packs to worlds; Java mods and shader packs get a plain "not supported" (M66).
- Surface rules run after noise caves and use stone_depth floors with above_preliminary_surface. Carvers turn exposed dirt under removed grass into the top block, as vanilla does. Checked against chunks from Mojang's own server on the same seed (M67).
- Render Scale defaults to Auto (the world at no more than about 4 million pixels, the UI always at full resolution). The game runs at the display's full refresh rate, including 120 Hz ProMotion (M68). v1.5's budgets are measured on Kenzo's M4.
- Distant Horizons is rebuilt natively as Distant Terrain: LOD terrain to 32–512 chunks (default 128 in Barebones and Realistic, Off in Vanilla), built by the server in the background and cached per world (M69).
