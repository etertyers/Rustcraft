//! Modern (1.18+) caves, from Y −64 up under the land:
//!
//! - noise caves: **cheese** caverns (big open rooms, with stone pillars
//!   standing in them), **spaghetti** tunnels (long, a few blocks wide)
//!   and **noodles** (very thin and winding);
//! - carved caves: the older **worm** tunnels and **ravines**, which a
//!   chunk replays from the chunks around it;
//! - **aquifers**: cells of the underground that are flooded to a level of
//!   their own, kept apart from dry ones by walls of stone, and lava below
//!   Y −55.
//!
//! Everything is a pure function of the seed and coordinates.

use noise::{Fbm, MultiFractal, NoiseFn, Perlin};

use super::{
    climate::{noise_offset, noise_seed},
    rng::{Rng, mix},
};
use crate::{
    config::{MIN_Y, SEA_LEVEL},
    world::{block::BlockState, blocks},
};

/// Air carved at or below this height is lava instead (modern's lava
/// aquifer below −54).
pub const LAVA_LEVEL: i32 = -55;
/// Aquifer cells: 16 across, 12 high (modern `Aquifer`'s grid).
const CELL_XZ: i32 = 16;
const CELL_Y: i32 = 12;
/// Share of aquifer cells (below the surface's reach) that hold water.
const FLOODED: u32 = 28;
/// Cells this high or higher stay dry, so caves near the surface aren't
/// flooded unless the sea comes in.
const DRY_ABOVE: i32 = SEA_LEVEL - 8;

/// What fills a carved block.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fill {
    Air,
    Water,
    Lava,
    /// Left as rock: a wall between a flooded cell and a dry one.
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

pub struct Caves {
    seed: u64,
    shift: [f64; 3],
    cheese: Fbm<Perlin>,
    pillars: Perlin,
    spaghetti_a: Perlin,
    spaghetti_b: Perlin,
    spaghetti_width: Perlin,
    noodle_a: Perlin,
    noodle_b: Perlin,
    noodle_toggle: Perlin,
}

/// How much each kind of noise cave opens: the threshold over which
/// cheese noise carves (lower deep down), the pillars' threshold, and the
/// spaghetti and noodle widths (squared distance from the noise zero).
const CHEESE_DEEP: f64 = 0.36;
const CHEESE_HIGH: f64 = 0.58;
const PILLAR: f64 = 0.52;
const SPAGHETTI_MIN: f64 = 0.0025;
const SPAGHETTI_MAX: f64 = 0.0095;
const NOODLE_WIDTH: f64 = 0.0016;
/// Noodles stay within these heights (modern −60..320, mostly low).
const NOODLE_TOP: i32 = 130;

fn smoothstep(edge0: f64, edge1: f64, x: f64) -> f64 {
    let t = ((x - edge0) / (edge1 - edge0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

impl Caves {
    pub fn new(seed: u64) -> Self {
        let fbm = |index, octaves, wavelength: f64| {
            Fbm::<Perlin>::new(noise_seed(seed, index))
                .set_octaves(octaves)
                .set_frequency(1.0 / wavelength)
        };
        let perlin = |index| Perlin::new(noise_seed(seed, index));
        Self {
            seed,
            shift: noise_offset(seed, 3),
            cheese: fbm(40, 2, 1.0),
            pillars: perlin(41),
            spaghetti_a: perlin(42),
            spaghetti_b: perlin(43),
            spaghetti_width: perlin(44),
            noodle_a: perlin(45),
            noodle_b: perlin(46),
            noodle_toggle: perlin(47),
        }
    }

    /// Positive where a noise cave opens, the larger the further inside:
    /// the most open of the cheese, spaghetti and noodle fields.
    pub fn density(&self, x: i32, y: i32, z: i32) -> f64 {
        let [sx, sy, sz] = self.shift;
        let (fx, fy, fz) = (x as f64 + sx, y as f64 + sy, z as f64 + sz);
        // Cheese: big rooms, commoner deep down, with pillars left in them.
        let threshold =
            CHEESE_DEEP + (CHEESE_HIGH - CHEESE_DEEP) * smoothstep(-40.0, 50.0, y as f64);
        let cheese = self.cheese.get([fx / 110.0, fy / 55.0, fz / 110.0]) - threshold;
        let pillar = self.pillars.get([fx / 11.0, fy / 40.0, fz / 11.0]) - PILLAR;
        let cheese = if pillar > 0.0 {
            cheese - pillar * 3.0
        } else {
            cheese
        };
        // Spaghetti: where two fields are both near zero, wider here and
        // narrower there.
        let p = [fx / 52.0, fy / 26.0, fz / 52.0];
        let (a, b) = (self.spaghetti_a.get(p), self.spaghetti_b.get(p));
        let width = SPAGHETTI_MIN
            + (SPAGHETTI_MAX - SPAGHETTI_MIN)
                * (self.spaghetti_width.get([fx / 90.0, fy / 90.0, fz / 90.0]) * 0.5 + 0.5);
        let spaghetti = (width - (a * a + b * b)) * 30.0;
        // Noodles: thin, winding, in patches, and low.
        let noodle =
            if y < NOODLE_TOP && self.noodle_toggle.get([fx / 70.0, fy / 70.0, fz / 70.0]) > 0.15 {
                let q = [fx / 18.0, fy / 14.0, fz / 18.0];
                let (a, b) = (self.noodle_a.get(q), self.noodle_b.get(q));
                (NOODLE_WIDTH - (a * a + b * b)) * 60.0
            } else {
                -1.0
            };
        cheese.max(spaghetti).max(noodle)
    }

    /// What a carved block at (x, y, z) holds: its aquifer cell's water up
    /// to that cell's level, lava at the bottom of the world, otherwise
    /// air, and rock where a flooded cell meets a dry one.
    pub fn fill(&self, x: i32, y: i32, z: i32, surface: i32) -> Fill {
        if y <= LAVA_LEVEL {
            return Fill::Lava;
        }
        let own = self.cell_fill(x, y, z, surface);
        // A wall a block thick at the faces of cells that disagree.
        for (dx, dy, dz) in [(1, 0, 0), (-1, 0, 0), (0, 0, 1), (0, 0, -1), (0, -1, 0)] {
            let (nx, ny, nz) = (x + dx, y + dy, z + dz);
            if cell(nx, ny, nz) == cell(x, y, z) {
                continue;
            }
            let theirs = self.cell_fill(nx, ny, nz, surface);
            let wet = |f: Fill| f == Fill::Water;
            // Water beside air, or water over air below it, would pour.
            if wet(own) != wet(theirs) && (dy == 0 || wet(own)) {
                return Fill::Rock;
            }
        }
        own
    }

    /// The fluid a cell holds at height `y`.
    fn cell_fill(&self, x: i32, y: i32, z: i32, surface: i32) -> Fill {
        if y <= LAVA_LEVEL {
            return Fill::Lava;
        }
        let (cx, cy, cz) = cell(x, y, z);
        let bottom = cy * CELL_Y;
        if bottom + CELL_Y >= DRY_ABOVE.min(surface - 8) {
            return Fill::Air;
        }
        let mut rng = Rng::for_chunk(mix(self.seed ^ 0xA9), cx, cz, cy as u64);
        if rng.below(100) >= FLOODED {
            return Fill::Air;
        }
        let level = bottom + 2 + rng.below(CELL_Y as u32 + 4) as i32;
        if y <= level { Fill::Water } else { Fill::Air }
    }
}

/// The aquifer cell holding a block.
fn cell(x: i32, y: i32, z: i32) -> (i32, i32, i32) {
    (
        x.div_euclid(CELL_XZ),
        y.div_euclid(CELL_Y),
        z.div_euclid(CELL_XZ),
    )
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
const RAVINE_CHANCE: f64 = 0.012;
/// How far (in chunks) a tunnel started elsewhere can reach.
pub const CARVER_REACH: i32 = 8;

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

    /// Share of blocks a noise cave opens between `low` and `high`.
    fn open_share(caves: &Caves, low: i32, high: i32) -> f64 {
        let mut open = 0;
        let mut all = 0;
        for x in (0..512).step_by(3) {
            for z in (0..512).step_by(7) {
                for y in (low..high).step_by(2) {
                    all += 1;
                    open += usize::from(caves.density(x, y, z) > 0.0);
                }
            }
        }
        open as f64 / all as f64
    }

    #[test]
    fn caves_open_more_of_the_deep_than_near_the_surface() {
        let caves = Caves::new(20111118);
        let deep = open_share(&caves, -56, 0);
        let middle = open_share(&caves, 0, 40);
        let high = open_share(&caves, 60, 100);
        assert!((0.05..0.3).contains(&deep), "deep {deep}");
        assert!(middle > 0.02 && middle < deep, "middle {middle}");
        assert!(high < middle, "high {high}");
    }

    #[test]
    fn the_bottom_is_lava_and_flooded_cells_are_walled_off() {
        let caves = Caves::new(7);
        assert_eq!(caves.fill(3, -58, 3, 70), Fill::Lava);
        // Above the dry line nothing floods.
        assert_eq!(caves.fill(3, SEA_LEVEL, 3, 120), Fill::Air);
        // Some cells hold water; next to a dry cell at the same height the
        // edge block stays rock.
        let mut water = 0;
        let mut walls = 0;
        for x in 0..256 {
            for y in -40..40 {
                match caves.fill(x, y, 5, 100) {
                    Fill::Water => water += 1,
                    Fill::Rock => walls += 1,
                    _ => {}
                }
            }
        }
        assert!(water > 500, "{water} water");
        assert!(walls > 0);
        for x in 0..256 {
            for y in -40..40 {
                if caves.fill(x, y, 5, 100) == Fill::Water {
                    for (dx, dy) in [(1, 0), (-1, 0), (0, -1)] {
                        assert_ne!(caves.fill(x + dx, y + dy, 5, 100), Fill::Air, "{x} {y}");
                    }
                }
            }
        }
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
