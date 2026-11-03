# World generation rules

Nested file: only rules that also appear in the root CLAUDE.md or in a milestone entry (`docs/milestones/`). Details: `docs/worldgen.md`.

## Vanilla data version
- Caves follow **vanilla 1.21.11's world-gen data** term by term (`misode/mcmeta`, branch `1.21.11-data`: `density_function/`, `noise/`, `noise_settings/overworld.json`, `configured_carver/`). `scripts/fetch-vanilla-assets.sh` downloads it into the git-ignored `assets/vanilla/data`. Never commit it. `docs/worldgen.md` says which data version the caves match (M53).
- Biomes use vanilla's multi-noise placement (`OverworldBiomeBuilder`); `config::BIOME_SCALE` (1.0 = vanilla) is the only stretch.
- The Nether and End keep 0..127 and 0..255 inside the Y −64..319 chunk (`Dimension::min_y`/`max_y`).

## Caves, ores, structures
- **Port exactly, no tuning (M53):** don't add rules, multipliers, offsets or "boosts" that force or suppress cave openings. If the port matches the JSON and the noise matches the formulas, the frequency is vanilla's by construction. Report numbers; don't invent targets (root §8).
- Normal noise = two improved-Perlin octave sets, the second at coordinates × 1.0181268882175227, with vanilla's value factor (M53 entry has the formula and test values).
- Carvers follow vanilla's `configured_carver` JSON and `CaveWorldCarver` logic, and ask the aquifer what to place.
- Ores are height-accurate to vanilla's tables (M21). Cave air by band, no water boxes, exposed ores and deepslate keep passing M48's `--cave-test` targets.
- Structures: templates are RON under `assets/structures/` (`--write-structures` draws the shipped ones); placement and jigsaw joining are in `world/structure/`. `--structure-test` checks M33's rules.

## Tests and tools
- Unit-test worldgen logic without a GPU: the reference-evaluator comparison against the JSON, noise value factors and standard deviations, the carver tunnel-count test, structure placement rules.
- Tools: `--cave-test`, `--cave-openings`, `--cave-slice`, `--nether-map`, `--structure-test`, `--spawn-test`, `--flower-test` (usage in `--help`).
- Chunk generation must not get slower (perf gate, root §7). Never lower render distance or world height to pass it.
- Golden hashes (`tests/golden.rs`) cover generated chunks; update them only as root §7 says.
