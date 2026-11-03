//! Modern (1.18+) caves, built from vanilla's own formulas:
//!
//! - the **final density** (`NoiseRouterData.overworld`): the land's
//!   sloped density below 1.5625 is cut only by cave **entrances** (a
//!   spaghetti-3D tunnel or an entrance blob); deeper it is the
//!   **underground** (cheese caverns with cave layers, thin 3D and 2D
//!   spaghetti, and pillars), and **noodles** cut through all of it;
//! - the **aquifers** (`Aquifer.NoiseBasedAquifer`): centers on a jittered
//!   grid each give their blocks a fluid level, so water and lava stand in
//!   caves at levels of their own, kept apart by irregular rock walls,
//!   joining the sea where the surface is under it, and lava below Y −54;
//! - the older **worm** carvers (caves and canyons) a chunk replays from
//!   the chunks around it.
//!
//! Everything is a pure function of the seed and coordinates.

use std::collections::HashMap;

use crate::world::fast_hash::FastMap;

use super::{
    normal_noise::NormalNoise,
    rng::{Rng, mix},
};
use crate::{
    config::{MIN_Y, SEA_LEVEL},
    world::{block::BlockState, blocks},
};

/// Open blocks at or below this height hold lava when no aquifer says
/// otherwise (modern's global fluid picker).
pub const LAVA_LEVEL: i32 = -55;
/// The level of an aquifer with no fluid: far below any block (modern's
/// `WAY_BELOW_MIN_Y`), so pressure against a wet one is large.
const DRY_LEVEL: i32 = -4096;
/// Local water in caves never stands above this unless it joins the sea.
const INLAND_WATER_TOP: i32 = 30;
/// Above this height `spaghetti_2d`'s thick term (`(|elevation + gradient| +
/// thickness)³`) is positive for every elevation and thickness.
const SPAGHETTI_2D_TOP: f64 = 75.0;
/// The sloped density at which the land stops being "near the surface".
const NEAR_SURFACE: f64 = 1.5625;
/// The bottom slide: density blends to solid between the floor and this
/// many blocks above it (modern `slideOverworld`).
const SLIDE_HEIGHT: f64 = 24.0;
const SLIDE_SOLID: f64 = 0.117_187_5;

/// What fills a block that isn't solid.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fill {
    Air,
    Water,
    Lava,
    /// Stays rock: a barrier between two aquifers' fluids.
    Rock,
}

impl Fill {
    pub fn block(self) -> Option<BlockState> {
        match self {
            Fill::Air => Some(BlockState::AIR),
            Fill::Water => Some(BlockState::of(blocks::WATER)),
            Fill::Lava => Some(BlockState::of(blocks::LAVA)),
            Fill::Rock => None,
        }
    }
}

/// Every cave noise, with vanilla's first octave and amplitudes.
pub struct Caves {
    seed: u64,
    cave_layer: NormalNoise,
    cave_cheese: NormalNoise,
    cave_entrance: NormalNoise,
    spaghetti_3d_1: NormalNoise,
    spaghetti_3d_2: NormalNoise,
    spaghetti_3d_rarity: NormalNoise,
    spaghetti_3d_thickness: NormalNoise,
    spaghetti_2d: NormalNoise,
    spaghetti_2d_elevation: NormalNoise,
    spaghetti_2d_modulator: NormalNoise,
    spaghetti_2d_thickness: NormalNoise,
    spaghetti_roughness: NormalNoise,
    spaghetti_roughness_modulator: NormalNoise,
    noodle: NormalNoise,
    noodle_thickness: NormalNoise,
    noodle_ridge_a: NormalNoise,
    noodle_ridge_b: NormalNoise,
    pillar: NormalNoise,
    pillar_rareness: NormalNoise,
    pillar_thickness: NormalNoise,
    aquifer_barrier: NormalNoise,
    aquifer_floodedness: NormalNoise,
    aquifer_spread: NormalNoise,
    aquifer_lava: NormalNoise,
}

/// A value of `noise` in -1..1 mapped to `from`..`to`.
fn mapped(noise: f64, from: f64, to: f64) -> f64 {
    from + (to - from) * (noise.clamp(-1.0, 1.0) + 1.0) * 0.5
}

/// Vanilla's rarity mappers for the spaghetti noises.
fn rarity_type1(v: f64) -> f64 {
    if v < -0.5 {
        0.5
    } else if v < 0.0 {
        0.75
    } else if v < 0.5 {
        1.0
    } else if v < 0.75 {
        2.0
    } else {
        3.0
    }
}

fn rarity_type2(v: f64) -> f64 {
    if v < -0.75 {
        0.5
    } else if v < -0.5 {
        0.75
    } else if v < 0.5 {
        1.0
    } else if v < 0.75 {
        2.0
    } else {
        3.0
    }
}

/// Vanilla's `weirdScaledSampler`: the noise sampled at a scale the
/// `rarity` value picks, taken absolute and scaled back up.
fn weird(rarity: f64, mapper: fn(f64) -> f64, noise: &NormalNoise, x: f64, y: f64, z: f64) -> f64 {
    let r = mapper(rarity);
    r * noise.get(x / r, y / r, z / r).abs()
}

/// A value of `from` at `y0` and `to` at `y1`, held at both ends.
fn y_gradient(y: f64, y0: f64, y1: f64, from: f64, to: f64) -> f64 {
    let t = ((y - y0) / (y1 - y0)).clamp(0.0, 1.0);
    from + (to - from) * t
}

/// Vanilla's `squeeze` after clamping: the density the aquifer sees.
pub fn squeeze(density: f64) -> f64 {
    let c = density.clamp(-1.0, 1.0);
    c / 2.0 - c * c * c / 24.0
}

impl Caves {
    pub fn new(seed: u64) -> Self {
        let n = |index: u64, octave: i32, amplitudes: &[f64]| {
            NormalNoise::new(seed, 100 + index, octave, amplitudes)
        };
        Self {
            seed,
            cave_layer: n(0, -8, &[1.0]),
            cave_cheese: n(1, -8, &[0.5, 1.0, 2.0, 1.0, 2.0, 1.0, 0.0, 2.0, 0.0]),
            cave_entrance: n(2, -7, &[0.4, 0.5, 1.0]),
            spaghetti_3d_1: n(3, -7, &[1.0]),
            spaghetti_3d_2: n(4, -7, &[1.0]),
            spaghetti_3d_rarity: n(5, -11, &[1.0]),
            spaghetti_3d_thickness: n(6, -8, &[1.0]),
            spaghetti_2d: n(7, -7, &[1.0]),
            spaghetti_2d_elevation: n(8, -8, &[1.0]),
            spaghetti_2d_modulator: n(9, -11, &[1.0]),
            spaghetti_2d_thickness: n(10, -11, &[1.0]),
            spaghetti_roughness: n(11, -5, &[1.0]),
            spaghetti_roughness_modulator: n(12, -8, &[1.0]),
            noodle: n(13, -8, &[1.0]),
            noodle_thickness: n(14, -8, &[1.0]),
            noodle_ridge_a: n(15, -7, &[1.0]),
            noodle_ridge_b: n(16, -7, &[1.0]),
            pillar: n(17, -7, &[1.0, 1.0]),
            pillar_rareness: n(18, -8, &[1.0]),
            pillar_thickness: n(19, -8, &[1.0]),
            aquifer_barrier: n(20, -3, &[1.0]),
            aquifer_floodedness: n(21, -7, &[1.0]),
            aquifer_spread: n(22, -5, &[1.0]),
            aquifer_lava: n(23, -1, &[1.0]),
        }
    }

    /// Cave entrances: negative where a 3D spaghetti tunnel or an
    /// entrance blob opens (`NoiseRouterData.entrances`).
    pub fn entrances(&self, x: f64, y: f64, z: f64) -> f64 {
        let rarity = self.spaghetti_3d_rarity.get(x * 2.0, y, z * 2.0);
        let thickness = mapped(self.spaghetti_3d_thickness.get(x, y, z), -0.065, -0.088);
        let a = weird(rarity, rarity_type1, &self.spaghetti_3d_1, x, y, z);
        let b = weird(rarity, rarity_type1, &self.spaghetti_3d_2, x, y, z);
        let tunnel = (a.max(b) + thickness).clamp(-1.0, 1.0);
        let blob = self.cave_entrance.get(x * 0.75, y * 0.5, z * 0.75)
            + 0.37
            + y_gradient(y, -10.0, 30.0, 0.3, 0.0);
        blob.min(tunnel)
    }

    /// Flat spaghetti caves at a wavering height (`spaghetti2D`).
    fn spaghetti_2d(&self, x: f64, y: f64, z: f64) -> f64 {
        let modulator = self.spaghetti_2d_modulator.get(x * 2.0, y, z * 2.0);
        let sampler = weird(modulator, rarity_type2, &self.spaghetti_2d, x, y, z);
        let elevation = mapped(self.spaghetti_2d_elevation.get(x, 0.0, z), -8.0, 8.0);
        let thickness = mapped(
            self.spaghetti_2d_thickness.get(x * 2.0, y, z * 2.0),
            -0.6,
            -1.3,
        );
        let offset = (elevation + y_gradient(y, -64.0, 320.0, 8.0, -40.0)).abs();
        let thick = (offset + thickness).powi(3);
        let sampler = sampler + 0.083 * thickness;
        sampler.max(thick).clamp(-1.0, 1.0)
    }

    /// Roughens the spaghetti walls (`spaghettiRoughnessFunction`).
    fn roughness(&self, x: f64, y: f64, z: f64) -> f64 {
        let modulator = mapped(self.spaghetti_roughness_modulator.get(x, y, z), 0.0, -0.1);
        modulator * (self.spaghetti_roughness.get(x, y, z).abs() - 0.4)
    }

    /// Stone pillars in the caverns: solid where this reaches 0.03.
    fn pillars(&self, x: f64, y: f64, z: f64) -> f64 {
        let noise = self.pillar.get(x * 25.0, y * 0.3, z * 25.0);
        let rareness = mapped(self.pillar_rareness.get(x, y, z), 0.0, -2.0);
        let thickness = mapped(self.pillar_thickness.get(x, y, z), 0.0, 1.1);
        (noise * 2.0 + rareness) * thickness.powi(3)
    }

    /// The caves under the near-surface band (`underground`).
    fn underground(&self, sloped: f64, x: f64, y: f64, z: f64) -> f64 {
        let layer = self.cave_layer.get(x, y * 8.0, z);
        let layer = 4.0 * layer * layer;
        let cheese = (0.27 + self.cave_cheese.get(x, y * 0.666_666_666_666_666_6, z))
            .clamp(-1.0, 1.0)
            + (1.5 - 0.64 * sloped).clamp(0.0, 0.5);
        // The most open of the cheese, the entrances and the thin tunnels;
        // once it is well under zero nothing can make it solid again, so
        // the rest isn't worked out (its exact depth only shades where a
        // wall falls between two cells).
        let mut caves = layer + cheese;
        if caves > -1.0 {
            caves = caves.min(self.entrances(x, y, z));
        }
        // The 2D tunnels sit within a few blocks of a sloping plane that
        // leaves the world's upper half: higher up they can't open.
        if caves > -1.0 && y < SPAGHETTI_2D_TOP {
            caves = caves.min(self.spaghetti_2d(x, y, z) + self.roughness(x, y, z));
        }
        if caves >= 2.0 {
            return caves;
        }
        let pillars = self.pillars(x, y, z);
        caves.max(if pillars >= 0.03 { pillars } else { -1.0e6 })
    }

    /// Noodle caves: very thin and winding, from Y −60 up to 320. Positive
    /// (solid) wherever there is none.
    fn noodle(&self, x: f64, y: f64, z: f64) -> f64 {
        const SOLID: f64 = 64.0;
        if !(-60.0..320.0).contains(&y) || self.noodle.get(x, y, z) < 0.0 {
            return SOLID;
        }
        let thickness = mapped(self.noodle_thickness.get(x, y, z), -0.05, -0.1);
        let f = 8.0 / 3.0;
        let a = self.noodle_ridge_a.get(x * f, y * f, z * f).abs();
        let b = self.noodle_ridge_b.get(x * f, y * f, z * f).abs();
        thickness + 1.5 * a.max(b)
    }

    /// The final density at a point whose land (sloped) density is
    /// `sloped`: positive is solid. Air above the ground is left as it is;
    /// near the surface only entrances cut; deeper the underground does;
    /// noodles cut everywhere; the bottom slide keeps the floor solid.
    pub fn final_density(&self, sloped: f64, x: i32, y: i32, z: i32) -> f64 {
        if sloped <= 0.0 {
            return sloped;
        }
        let (fx, fy, fz) = (f64::from(x), f64::from(y), f64::from(z));
        let combined = if sloped < NEAR_SURFACE {
            sloped.min(5.0 * self.entrances(fx, fy, fz))
        } else {
            self.underground(sloped, fx, fy, fz)
        };
        let t = ((fy - f64::from(MIN_Y)) / SLIDE_HEIGHT).clamp(0.0, 1.0);
        let slid = SLIDE_SOLID + (combined - SLIDE_SOLID) * t;
        if slid <= 0.0 {
            return slid;
        }
        slid.min(self.noodle(fx, fy, fz))
    }
}

/// What an aquifer center holds: the level its fluid stands at and what it
/// is (`None`: dry).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Status {
    pub level: i32,
    pub fluid: Fill,
}

impl Status {
    /// The fluid at height `y` (air above the level).
    pub fn at(self, y: i32) -> Fill {
        if y < self.level {
            self.fluid
        } else {
            Fill::Air
        }
    }
}

/// How much two aquifer centers' fluids press on a block at height `y`
/// between them (`Aquifer.calculatePressure`, without its barrier noise):
/// the part of the pressure that the barrier noise is added to, and the
/// distance of the block from the boundary of the two levels.
pub fn pressure_terms(s1: Status, s2: Status, y: i32) -> Option<(f64, bool)> {
    let (b1, b2) = (s1.at(y), s2.at(y));
    if matches!(
        (b1, b2),
        (Fill::Lava, Fill::Water) | (Fill::Water, Fill::Lava)
    ) {
        return Some((1.0, false));
    }
    let diff = (s1.level - s2.level).abs();
    if diff == 0 {
        return None;
    }
    let middle = 0.5 * f64::from(s1.level + s2.level);
    let e = f64::from(y) + 0.5 - middle;
    let f = f64::from(diff) / 2.0;
    let q = f - e.abs();
    let r = if e > 0.0 {
        let s = q;
        if s > 0.0 { s / 1.5 } else { s / 2.5 }
    } else {
        let s = 3.0 + q;
        if s > 0.0 { s / 3.0 } else { s / 10.0 }
    };
    Some((r, !(-2.0..=2.0).contains(&r)))
}

/// Blocks whose two nearest centers are this similar (or more) but not
/// within a barrier's reach get a fluid update when their chunk loads
/// (`similarity(10², 12²)`).
const FLOWING_SIMILARITY: f64 = -0.76;

/// Vanilla's closeness of two squared distances: 1 when equal, 0 at 25
/// apart.
pub fn similarity(a: f64, b: f64) -> f64 {
    1.0 - (b - a).abs() / 25.0
}

/// The aquifers of one chunk: fluid levels at jittered centers, and what
/// each open block holds.
pub struct Aquifer<'a, S: FnMut(i32, i32) -> i32> {
    caves: &'a Caves,
    surface: S,
    statuses: FastMap<(i32, i32, i32), (IVec3Like, Status)>,
    /// The twelve centers round a block's search cell, by that cell.
    neighborhoods: FastMap<(i32, i32, i32), Hood>,
    /// The search cell asked about last, since a column's blocks mostly
    /// stay in one.
    last_hood: Option<((i32, i32, i32), Hood)>,
    surfaces: FastMap<(i32, i32), i32>,
    /// Whether the last `substance` call asked for a fluid update
    /// (modern `shouldScheduleFluidUpdate`).
    pub schedule: bool,
}

/// A center's position.
type IVec3Like = [i32; 3];

/// The twelve centers round a search cell and their statuses, and the one
/// status they all share, if they do.
#[derive(Clone, Copy)]
struct Hood {
    centers: [(IVec3Like, Status); 12],
    uniform: Option<Status>,
}

impl<'a, S: FnMut(i32, i32) -> i32> Aquifer<'a, S> {
    /// `surface(x, z)` gives the land's height at a column (its
    /// preliminary surface).
    pub fn new(caves: &'a Caves, surface: S) -> Self {
        Self {
            caves,
            surface,
            statuses: FastMap::default(),
            neighborhoods: FastMap::default(),
            last_hood: None,
            surfaces: FastMap::default(),
            schedule: false,
        }
    }

    fn surface_at(&mut self, x: i32, z: i32) -> i32 {
        if let Some(&y) = self.surfaces.get(&(x, z)) {
            return y;
        }
        let y = (self.surface)(x, z);
        self.surfaces.insert((x, z), y);
        y
    }

    /// The center of grid cell (gx, gy, gz): 16 across, 12 high, jittered
    /// by 0–9 blocks across and 0–8 up.
    fn center(&self, gx: i32, gy: i32, gz: i32) -> IVec3Like {
        let mut rng = Rng::for_chunk(mix(self.caves.seed ^ 0xA9), gx, gz, gy as u64);
        [
            gx * 16 + rng.below(10) as i32,
            gy * 12 + rng.below(9) as i32,
            gz * 16 + rng.below(10) as i32,
        ]
    }

    fn status_of(&mut self, gx: i32, gy: i32, gz: i32) -> ([i32; 3], Status) {
        if let Some(&(at, status)) = self.statuses.get(&(gx, gy, gz)) {
            return (at, status);
        }
        let at = self.center(gx, gy, gz);
        let status = self.compute_fluid(at);
        self.statuses.insert((gx, gy, gz), (at, status));
        (at, status)
    }

    /// Modern's global fluid picker: sea level water, lava below Y −54.
    fn global(y: i32) -> Status {
        if y < LAVA_LEVEL + 1 {
            Status {
                level: LAVA_LEVEL + 1,
                fluid: Fill::Lava,
            }
        } else {
            Status {
                level: SEA_LEVEL,
                fluid: Fill::Water,
            }
        }
    }

    /// The fluid level and kind at a center (`computeFluid`).
    fn compute_fluid(&mut self, [x, y, z]: [i32; 3]) -> Status {
        let global = Self::global(y);
        let mut min_surface = i32::MAX;
        let top = y + 12;
        let bottom = y - 12;
        let mut near_surface = false;
        for (ox, oz) in [(0, 0), (-2, -1), (-1, -1), (-3, 0), (-2, 0)] {
            let (sx, sz) = (x + ox * 16, z + oz * 16);
            let surface = self.surface_at(sx, sz);
            let limit = surface + 8;
            let center = ox == 0 && oz == 0;
            if center && bottom > limit {
                return global;
            }
            let top_near = top > limit;
            if top_near || center {
                // Water at the surface there: the sea, a river or a lake.
                let wet = Self::global(surface).at(surface) != Fill::Air;
                if wet {
                    if center {
                        near_surface = true;
                    }
                    if top_near {
                        return Self::global(surface);
                    }
                }
            }
            min_surface = min_surface.min(surface);
        }
        let level = self.surface_level(x, y, z, global, min_surface, near_surface);
        let fluid = match level {
            None => Fill::Air,
            Some(level) if level <= -10 && global.fluid != Fill::Lava => {
                let d = self.caves.aquifer_lava.get(
                    f64::from(x.div_euclid(64)),
                    f64::from(y.div_euclid(40)),
                    f64::from(z.div_euclid(64)),
                );
                if d.abs() > 0.3 {
                    Fill::Lava
                } else {
                    global.fluid
                }
            }
            Some(_) => global.fluid,
        };
        match level {
            Some(level) if fluid != Fill::Air => Status { level, fluid },
            _ => Status {
                level: DRY_LEVEL,
                fluid: Fill::Air,
            },
        }
    }

    /// How high the fluid stands at a center, if it holds any
    /// (`computeSurfaceLevel`).
    fn surface_level(
        &self,
        x: i32,
        y: i32,
        z: i32,
        global: Status,
        min_surface: i32,
        near_surface: bool,
    ) -> Option<i32> {
        let depth = f64::from(min_surface + 8 - y);
        let f = if near_surface {
            (1.0 - depth / 64.0).clamp(0.0, 1.0)
        } else {
            0.0
        };
        let g = self
            .caves
            .aquifer_floodedness
            .get(f64::from(x), f64::from(y) * 0.67, f64::from(z))
            .clamp(-1.0, 1.0);
        // Mth.map(f, 1, 0, -0.3, 0.8) and Mth.map(f, 1, 0, -0.8, 0.4).
        let flooded_threshold = 0.8 + (-0.3 - 0.8) * f;
        let spread_threshold = 0.4 + (-0.8 - 0.4) * f;
        if g - flooded_threshold > 0.0 {
            // Flooded to the sea; inland water never stands that high.
            return Some(if near_surface {
                global.level
            } else {
                global.level.min(INLAND_WATER_TOP)
            });
        }
        if g - spread_threshold > 0.0 {
            let (i, j, k) = (x.div_euclid(16), y.div_euclid(40), z.div_euclid(16));
            let spread =
                self.caves
                    .aquifer_spread
                    .get(f64::from(i), f64::from(j) / 1.4, f64::from(k))
                    * 10.0;
            let stepped = (spread / 3.0).floor() as i32 * 3;
            let level = j * 40 + 20 + stepped;
            return Some(level.min(min_surface).min(INLAND_WATER_TOP));
        }
        None
    }

    /// The twelve aquifer centers (2 × 3 × 2) round search cell
    /// (x0, y0, z0), with their statuses.
    fn neighborhood(&mut self, x0: i32, y0: i32, z0: i32) -> Hood {
        if let Some((key, hood)) = self.last_hood
            && key == (x0, y0, z0)
        {
            return hood;
        }
        if let Some(found) = self.neighborhoods.get(&(x0, y0, z0)).copied() {
            self.last_hood = Some(((x0, y0, z0), found));
            return found;
        }
        let dry = (
            [0; 3],
            Status {
                level: DRY_LEVEL,
                fluid: Fill::Air,
            },
        );
        let mut found = [dry; 12];
        let mut i = 0;
        for dx in 0..2 {
            for dy in -1..=1 {
                for dz in 0..2 {
                    found[i] = self.status_of(x0 + dx, y0 + dy, z0 + dz);
                    i += 1;
                }
            }
        }
        let uniform = found
            .iter()
            .all(|(_, status)| *status == found[0].1)
            .then_some(found[0].1);
        let hood = Hood {
            centers: found,
            uniform,
        };
        self.neighborhoods.insert((x0, y0, z0), hood);
        self.last_hood = Some(((x0, y0, z0), hood));
        hood
    }

    /// What the open block at (x, y, z) holds, where the final density
    /// (squeezed) is `density`: its aquifer's fluid, or `Rock` where a
    /// barrier between two aquifers' fluids keeps it solid
    /// (`computeSubstance`).
    pub fn substance(&mut self, x: i32, y: i32, z: i32, density: f64) -> Fill {
        self.schedule = false;
        if density > 0.0 {
            return Fill::Rock;
        }
        if y <= LAVA_LEVEL {
            return Fill::Lava;
        }
        let (x0, y0, z0) = (
            (x - 5).div_euclid(16),
            (y + 1).div_euclid(12),
            (z - 5).div_euclid(16),
        );
        // The three nearest of the 2 × 3 × 2 centers around.
        let hood = self.neighborhood(x0, y0, z0);
        if let Some(status) = hood.uniform {
            // Every center agrees: no barrier, no edge.
            return status.at(y);
        }
        let dry = Status {
            level: DRY_LEVEL,
            fluid: Fill::Air,
        };
        let mut best = [(f64::MAX, dry); 3];
        for (at, status) in hood.centers {
            let d = f64::from((x - at[0]).pow(2) + (y - at[1]).pow(2) + (z - at[2]).pow(2));
            if d < best[0].0 {
                best = [(d, status), best[0], best[1]];
            } else if d < best[1].0 {
                best = [best[0], (d, status), best[1]];
            } else if d < best[2].0 {
                best[2] = (d, status);
            }
        }
        let [(d1, s1), (d2, s2), (d3, s3)] = best;
        let own = s1.at(y);
        let sim12 = similarity(d1, d2);
        if sim12 <= 0.0 {
            // Close to the edge of its aquifer: its fluid may flow.
            self.schedule = sim12 >= FLOWING_SIMILARITY;
            return own;
        }
        if own == Fill::Water && Self::global(y - 1).at(y - 1) == Fill::Lava {
            self.schedule = true;
            return own;
        }
        let mut barrier = f64::NAN;
        let mut pressed = |s: Status, t: Status, caves: &Caves| -> f64 {
            let Some((r, outside)) = pressure_terms(s, t, y) else {
                return 0.0;
            };
            let noise = if outside {
                0.0
            } else {
                if barrier.is_nan() {
                    barrier =
                        caves
                            .aquifer_barrier
                            .get(f64::from(x), f64::from(y) * 0.5, f64::from(z));
                }
                barrier
            };
            2.0 * (noise + r)
        };
        let caves = self.caves;
        let p12 = pressed(s1, s2, caves);
        if density + sim12.max(0.0) * p12 > 0.0 {
            return Fill::Rock;
        }
        let sim13 = similarity(d1, d3);
        if sim13 > 0.0 && s1.at(y) != s3.at(y) {
            let p13 = pressed(s1, s3, caves);
            if density + sim13 * p13 > 0.0 {
                return Fill::Rock;
            }
        }
        let sim23 = similarity(d2, d3);
        if sim23 > 0.0 && s2.at(y) != s3.at(y) {
            let p23 = pressed(s2, s3, caves);
            if density + sim23 * p23 > 0.0 {
                return Fill::Rock;
            }
        }
        own
    }

    /// The level and kind of the aquifer a block belongs to (its nearest
    /// center's), for tests.
    pub fn status_at(&mut self, x: i32, y: i32, z: i32) -> Status {
        let (x0, y0, z0) = (
            (x - 5).div_euclid(16),
            (y + 1).div_euclid(12),
            (z - 5).div_euclid(16),
        );
        let mut best = (
            f64::MAX,
            Status {
                level: DRY_LEVEL,
                fluid: Fill::Air,
            },
        );
        for (at, status) in self.neighborhood(x0, y0, z0).centers {
            let d = f64::from((x - at[0]).pow(2) + (y - at[1]).pow(2) + (z - at[2]).pow(2));
            if d < best.0 {
                best = (d, status);
            }
        }
        best.1
    }
}

/// One carved tunnel's path, step by step: the middle and the radii
/// across and up.
pub struct Worm {
    pub steps: Vec<([f64; 3], f64, f64)>,
}

/// Chance a chunk starts worm caves, and starts a ravine (modern
/// `cave` 0.15 plus `cave_extra_underground` 0.07, and `canyon` 0.01).
const WORM_CHANCE: f64 = 0.15;
const DEEP_WORM_CHANCE: f64 = 0.07;
const RAVINE_CHANCE: f64 = 0.01;
/// How far (in chunks) a tunnel started elsewhere can reach.
pub const CARVER_REACH: i32 = 8;

/// The worms chunk (cx, cz) starts, remembered per thread: each chunk's
/// worms are replayed by every chunk within `CARVER_REACH` of it.
pub fn worms_of(seed: u64, cx: i32, cz: i32) -> std::rc::Rc<Vec<Worm>> {
    type Memo = (u64, HashMap<(i32, i32), std::rc::Rc<Vec<Worm>>>);
    thread_local! {
        static MEMO: std::cell::RefCell<Memo> = std::cell::RefCell::new((0, HashMap::new()));
    }
    MEMO.with(|memo| {
        let mut memo = memo.borrow_mut();
        if memo.0 != seed {
            *memo = (seed, HashMap::new());
        }
        if memo.1.len() > 4096 {
            memo.1.clear();
        }
        memo.1
            .entry((cx, cz))
            .or_insert_with(|| std::rc::Rc::new(worms(seed, cx, cz)))
            .clone()
    })
}

/// The worm tunnels and ravines chunk (cx, cz) starts.
pub fn worms(seed: u64, cx: i32, cz: i32) -> Vec<Worm> {
    let mut rng = Rng::for_chunk(seed, cx, cz, 0xCA5E);
    let mut out = Vec::new();
    let (x0, z0) = (cx * 16, cz * 16);
    let start = |rng: &mut Rng, low: i32, high: i32| {
        [
            (x0 + rng.below(16) as i32) as f64,
            (low + rng.below((high - low) as u32) as i32) as f64,
            (z0 + rng.below(16) as i32) as f64,
        ]
    };
    if rng.next_f64() < WORM_CHANCE {
        for _ in 0..1 + rng.below(3) {
            let at = start(&mut rng, MIN_Y + 8, 180);
            out.push(worm(&mut rng, at, false));
        }
    }
    if rng.next_f64() < DEEP_WORM_CHANCE {
        let at = start(&mut rng, MIN_Y + 8, 47);
        out.push(worm(&mut rng, at, false));
    }
    if rng.next_f64() < RAVINE_CHANCE {
        let at = start(&mut rng, 10, 68);
        out.push(worm(&mut rng, at, true));
    }
    out
}

/// A tunnel wandering from `at` (modern `CaveWorldCarver`): its heading
/// drifts a little each step, it swells in the middle, and a ravine is
/// tall and narrow and keeps nearly level.
fn worm(rng: &mut Rng, at: [f64; 3], ravine: bool) -> Worm {
    let mut p = at;
    let mut yaw = rng.next_f64() * std::f64::consts::TAU;
    let mut pitch = (rng.next_f64() - 0.5) * if ravine { 0.25 } else { 0.5 };
    let (mut yaw_drift, mut pitch_drift) = (0.0, 0.0);
    let width = if ravine {
        1.5 + rng.next_f64() * 2.5
    } else {
        0.8 + rng.next_f64() * 2.2
    };
    let length = if ravine {
        90 + rng.below(70)
    } else {
        60 + rng.below(60)
    } as usize;
    let tall = if ravine {
        3.0
    } else {
        0.9 + rng.next_f64() * 0.4
    };
    let mut steps = Vec::with_capacity(length);
    for step in 0..length {
        let swell = (step as f64 * std::f64::consts::PI / length as f64).sin();
        let across = 1.5 + swell * width;
        steps.push((p, across, across * tall));
        let (cp, sp) = (pitch.cos(), pitch.sin());
        p[0] += yaw.cos() * cp;
        p[1] += sp;
        p[2] += yaw.sin() * cp;
        pitch *= if ravine { 0.7 } else { 0.92 };
        pitch += pitch_drift * 0.1;
        yaw += yaw_drift * 0.1;
        pitch_drift *= if ravine { 0.8 } else { 0.9 };
        yaw_drift *= 0.75;
        pitch_drift += (rng.next_f64() - rng.next_f64()) * rng.next_f64() * 2.0;
        yaw_drift += (rng.next_f64() - rng.next_f64()) * rng.next_f64() * 4.0;
    }
    Worm { steps }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn open_aquifer(caves: &Caves) -> Aquifer<'_, impl FnMut(i32, i32) -> i32> {
        // Land at Y 90 everywhere: nothing near the sea.
        Aquifer::new(caves, |_, _| 90)
    }

    #[test]
    fn the_final_density_is_the_same_at_fixed_points_for_a_seed() {
        // A regression anchor: the router's numbers at fixed points (the
        // sloped density `s` standing for how deep the land is there).
        let caves = Caves::new(12345);
        let points = [
            (10, -30, 20, 6.0),
            (-200, -10, 90, 4.0),
            (333, 40, -75, 3.0),
            (0, 62, 0, 1.0),
            (50, -62, 50, 9.0),
        ];
        let again = Caves::new(12345);
        for (x, y, z, s) in points {
            let a = caves.final_density(s, x, y, z);
            assert_eq!(a, again.final_density(s, x, y, z));
            assert!(a.is_finite() && a.abs() < 1.0e7, "{a}");
        }
        // Air above the land stays as it is.
        assert_eq!(caves.final_density(-0.5, 3, 90, 3), -0.5);
        // The same point under another seed differs somewhere.
        let other = Caves::new(54321);
        assert!(points.iter().any(|&(x, y, z, s)| {
            (caves.final_density(s, x, y, z) - other.final_density(s, x, y, z)).abs() > 1e-9
        }));
    }

    #[test]
    fn noodles_never_cut_below_minus_60_and_the_floor_stays_solid() {
        let caves = Caves::new(7);
        for i in 0..4000 {
            let (x, z) = (f64::from(i) * 5.3, f64::from(i) * 3.1 - 700.0);
            for y in [-64.0, -63.0, -62.0, -61.0] {
                assert!(caves.noodle(x, y, z) >= 64.0, "noodle at {y}");
            }
            // The bottom slide: Y −64 to −61 stay solid whatever the caves.
            for y in -64..=-61 {
                let d = caves.final_density(8.0, (x as i32) * 2, y, z as i32);
                assert!(d > 0.0, "open at {y}: {d}");
            }
        }
        // Noodles do exist above −60.
        let found =
            (0..20_000).any(|i| caves.noodle(f64::from(i) * 7.1, 20.0, f64::from(i) * 3.3) < 0.0);
        assert!(found, "no noodle cave found");
    }

    #[test]
    fn two_aquifers_at_different_levels_get_a_wall_between_them() {
        let wet = Status {
            level: 10,
            fluid: Fill::Water,
        };
        let low = Status {
            level: -20,
            fluid: Fill::Water,
        };
        // Halfway between the two levels the pressure is high: an open
        // block (density −5, squeezed) next to the boundary stays rock.
        let (r, outside) = pressure_terms(wet, low, -5).expect("levels differ");
        assert!(outside && r > 2.0, "{r}");
        let pressure = 2.0 * r;
        assert!(squeeze(-5.0) + similarity(100.0, 101.0) * pressure > 0.0);
        // Far from the boundary (similarity 0) nothing walls it.
        assert!(squeeze(-5.0) + similarity(0.0, 40.0).max(0.0) * pressure <= 0.0);
        // Equal levels never wall.
        assert_eq!(pressure_terms(wet, wet, -5), None);
        // Water against lava always presses.
        let lava = Status {
            level: -40,
            fluid: Fill::Lava,
        };
        assert_eq!(pressure_terms(wet, lava, -50), Some((1.0, false)));
    }

    #[test]
    fn open_blocks_hold_water_only_below_their_aquifers_level_and_lava_below_minus_55() {
        let caves = Caves::new(99);
        let mut aquifer = open_aquifer(&caves);
        let (mut water, mut lava, mut rock) = (0, 0, 0);
        for x in (0..400).step_by(3) {
            for z in (0..400).step_by(5) {
                for y in -64..60 {
                    let fill = aquifer.substance(x, y, z, -0.2);
                    if y <= LAVA_LEVEL {
                        assert_eq!(fill, Fill::Lava, "{x} {y} {z}");
                        lava += 1;
                        continue;
                    }
                    match fill {
                        Fill::Water => {
                            let level = aquifer.status_at(x, y, z).level;
                            assert!(y < level, "water at {y} in an aquifer of {level}");
                            // Inland, water never stands above Y 30.
                            assert!(y < INLAND_WATER_TOP, "inland water at {y}");
                            water += 1;
                        }
                        Fill::Rock => rock += 1,
                        Fill::Lava => {
                            assert!(aquifer.status_at(x, y, z).level > y);
                            lava += 1;
                        }
                        Fill::Air => {}
                    }
                }
            }
        }
        assert!(water > 1000 && lava > 1000, "water {water}, lava {lava}");
        assert!(rock > 0, "barriers exist");
    }

    #[test]
    fn some_chunks_start_worms_and_a_few_ravines() {
        let (mut worms_seen, mut ravines) = (0, 0);
        for cx in 0..40 {
            for cz in 0..40 {
                for w in worms(1, cx, cz) {
                    if w.steps.iter().any(|s| s.2 > s.1 * 2.0) {
                        ravines += 1;
                    } else {
                        worms_seen += 1;
                    }
                }
            }
        }
        assert!((250..900).contains(&worms_seen), "{worms_seen} worms");
        assert!((5..50).contains(&ravines), "{ravines} ravines");
    }
}
