# Decisions

One line per decision: date, decision, reason.

- 2026-10-16 — Added `pollster` as a dependency (not in CLAUDE.md §2). — wgpu adapter/device setup is async; pollster blocks on it without pulling in an async runtime.
- 2026-10-16 — Pinned `bincode` to 2.0.1 instead of the newest 3.0.0. — 3.0.0 is a tombstone release whose only content is a `compile_error!`.
- 2026-10-16 — `assets/textures/` is a copy of the provided pack's `assets/minecraft/textures/` (subfolders kept, `.mcmeta` animation files included). The pack's `blockstates/` and `mcpatcher/` folders were not copied. — Only the PNGs are needed; blockstates are a post-1.0 format and mcpatcher is an optional mod format. Revisit at M1 if anything is missing.
- 2026-10-16 — `docs/sounds.md` is not in the repo yet. — It wasn't among the files provided. Audio is a placeholder until M11 anyway; add it when Kenzo provides it.
- 2026-10-16 — The window's clear color is sky blue (0.47, 0.65, 1.0) in sRGB. — Close to the 1.0.0 daytime sky.
