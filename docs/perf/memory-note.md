# Why the gate's memory rows are +16–23% over the baseline

The gate compares each row's resident memory (`ps` RSS at the end of a 60 s flight) with `baseline.json`, recorded at v1.4.0 (commit f4be709, 2026-11-02, on this Mac mini M1).

**It is not the packs.** Texture memory by style (`look::memory_report::texture_memory_by_style`, run with `--ignored --nocapture`):

| Style | Atlas | Mob textures | Total |
|---|---|---|---|
| Vanilla | 1024×432 = 1.7 MiB | 2.5 MiB | 4.2 MiB |
| Barebones (Bare Bones, Better Leaves, Fresh Animations, Bare Bones x FA; 55 custom models) | 1024×432 = 1.7 MiB | 2.5 MiB | 4.2 MiB |
| Realistic (Patrix 32x; 54 custom models) | 2048×896 = 7.0 MiB | 11.0 MiB | 18 MiB |

The 16× packs add nothing to the atlas and Patrix adds about 14 MiB. The rows moved by 100+ MiB.

**The baseline is a low draw of a noisy number.** Same code, same row, different runs:
- v1.4.0 itself: baseline `trailer-medium` 580 MiB, but M58's own run of the same row (the commit v1.4.0 was cut from) measured 698 MiB.
- `vanilla` row, no packs, no shaders: 569 (M54) to 685 (M62) MiB.
- `trailer-medium`: 520 (M54) to 701 (M57) MiB.

The jump from about 580 to about 700 happened with M55–M57 (more render targets and shadow maps in the shader styles) and the figure has stayed in 690–745 since. The Vanilla style with Shaders Medium (no packs) is 695–711 MiB now, the same as Barebones with its packs (697–715). So the baseline rows `barebones-low/medium` (605/580) are the low end of v1.4's range, not a measurement from before the packs: v1.4's Trailer rows had no packs and the same code paths.

**What would make the gate meaningful:** measure `phys_footprint` (Mach `task_info`) instead of RSS, or take the median of several launches per row. Not changed here: the baseline stays as it is until you decide.
