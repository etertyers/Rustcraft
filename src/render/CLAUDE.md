# Rendering rules

Nested file: only rules that also appear in the root CLAUDE.md or in a milestone entry (`docs/milestones/`).

## Pipeline
- `wgpu` (Metal on macOS) with WGSL shaders in this folder (`*.wgsl`). No game engine: the renderer and game loop are ours.
- Chunk vertices are built by `world/mesh.rs` on worker threads; the main thread only uploads finished meshes. It never blocks on them.
- Rendering runs at the display refresh rate with interpolation; game logic is fixed at 20 ticks/second.
- `lightmap.rs` is a tested copy of the shader's lightmap formula.
- Underwater fog follows vanilla's 96-block fog with water vision, and water colors stay vanilla (M47, M51).

## The two looks (v1.4)
- `visuals/`: Vanilla is M53's passes and shaders into the surface and must stay pixel-identical (`tests/vanilla_reference.rs`); Trailer draws into an HDR target with `trailer.wgsl`, shadow maps (`visuals/shadow.rs`, `sun.rs`) and a tone-mapping post pass. `Vertex` stays as M53 had it: the Trailer look's per-face data is a second vertex buffer (`Mesh::faces`).
- Never change `shader.wgsl`, `sky.wgsl`, `clouds.wgsl` or the Vanilla pipelines for a Trailer effect. Budgets and shares: `docs/milestones/v1.4.md`.

## Textures
- Textures are looked up by modern resource location (`block/stone`) through `pack::Resources`: selected packs from `resourcepacks/` over Kenzo's `assets/textures/` over the vanilla fallback in `assets/vanilla/`. Older names are in `assets/data/texture_names.ron`.
- `look::Look` builds everything texture-derived (atlas, models, icons) and is rebuilt when the packs change.
- Missing textures render as a magenta/black checkerboard. Don't generate or download other art. Never commit `assets/vanilla/`.

## Checking a change
- Test the logic, not the GPU: unit-test pure code and verify rendering by running the game.
- `cargo run --release -- --shot <name>` captures a scripted scene to `docs/screenshots/<name>.png` (`--shot list` shows them; works with the screen locked). At most one screenshot per milestone, at the end, opened once. Never take screenshots while debugging. Kenzo does the visual checks.
- Never lower visual quality to pass a performance target; stop and report instead.
