# Decisions

One line per decision: date, decision, reason.

- 2026-10-16 — Added `pollster` as a dependency (not in CLAUDE.md §2). — wgpu adapter/device setup is async; pollster blocks on it without pulling in an async runtime.
- 2026-10-16 — Pinned `bincode` to 2.0.1 instead of the newest 3.0.0. — 3.0.0 is a tombstone release whose only content is a `compile_error!`.
- 2026-10-16 — `assets/textures/` is a copy of the provided pack's `assets/minecraft/textures/` (subfolders kept, `.mcmeta` animation files included). The pack's `blockstates/` and `mcpatcher/` folders were not copied. — Only the PNGs are needed; blockstates are a post-1.0 format and mcpatcher is an optional mod format. Revisit at M1 if anything is missing.
- 2026-10-16 — `docs/sounds.md` is not in the repo yet. — It wasn't among the files provided. Audio is a placeholder until M11 anyway; add it when Kenzo provides it.
- 2026-10-16 — The window's clear color is sky blue (0.47, 0.65, 1.0) in sRGB. — Close to the 1.0.0 daytime sky.
- 2026-10-16 — Block textures are mapped in `assets/data/blocks.ron` (IDs 0–122) and item textures in `assets/data/items.ron` (256–382, 2256–2266). The pack uses 1.12-era file names, so most map directly. — CLAUDE.md §4: map filenames at M1.
- 2026-10-16 — **Blocks with no matching texture** (render as the magenta/black checkerboard): chest (54) and locked chest (95) (`chest_top`/`chest_side`/`chest_front`), standing sign (63) and wall sign (68) (`sign`), end portal (119) (`end_portal`). Chests and signs can be cut from `entity/chest/normal.png` and `entity/sign.png` when their models are built (M2); the end portal effect isn't planned. Air (0) and moving piston (36) draw nothing by design. All item textures exist. — `world::block` tests keep this list in sync.
- 2026-10-16 — Grayscale textures get a tint: grass top, tall grass, leaves and vines use the plains colors (grass #91BD59, foliage #77AB2F) until biomes exist (M3); lily pads use a fixed #208030. The pack's `grass_side` and water textures are pre-colored, so they aren't tinted. — Matches 1.0.0's colormap behavior for plains.
- 2026-10-16 — Tint and face shading (top 1.0, bottom 0.5, N/S 0.8, E/W 0.6) are applied in sRGB space, like Minecraft, by linearizing the vertex color in the shader. — Multiplying in linear space would make shaded faces too dark.
- 2026-10-16 — Blocks with a "front" (furnace, dispenser, pumpkin, bed foot end) put it on the south face until placement orientation exists (M4). — Placeholder orientation.
- 2026-10-16 — Animated textures (water, lava, fire, portal, compass, clock) use their first frame for now. — Animation isn't in any milestone yet; revisit at M8 (water) or M11 (polish).
- 2026-10-16 — The free-fly camera moves once per rendered frame, not on the 20 TPS fixed timestep. — The fixed-timestep loop arrives with player physics (M4); a per-frame dev camera is smoother.
