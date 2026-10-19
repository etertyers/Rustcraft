//! Modern Minecraft's climate parameters and the biome they pick.
//!
//! Five noise fields describe every column, as in modern Java Edition:
//! continentalness (ocean to far inland), erosion (mountainous to flat),
//! weirdness (which, folded, gives peaks and valleys), temperature and
//! humidity. Each is spread to an even -1..1, so a threshold reads as a
//! share of the world: `continent < -0.3` is the lowest 35%.
//!
//! The fields are sampled every `GRID` blocks and blended in between, like
//! modern Minecraft's 4×4 noise cells, which keeps generation cheap and
//! the climate smooth.

use noise::{Fbm, MultiFractal, NoiseFn, Perlin};

use super::rng::mix;
use crate::world::biome::Biome;

/// Blocks between climate samples.
pub const GRID: i32 = 4;

/// The five parameters at a column, each spread evenly over -1..1.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Climate {
    pub continent: f64,
    pub erosion: f64,
    pub weirdness: f64,
    pub temperature: f64,
    pub humidity: f64,
}

impl Climate {
    fn lerp(self, other: Self, t: f64) -> Self {
        let l = |a: f64, b: f64| a + (b - a) * t;
        Self {
            continent: l(self.continent, other.continent),
            erosion: l(self.erosion, other.erosion),
            weirdness: l(self.weirdness, other.weirdness),
            temperature: l(self.temperature, other.temperature),
            humidity: l(self.humidity, other.humidity),
        }
    }

    /// Modern "peaks and valleys": weirdness folded so that both its middle
    /// and its ends are valleys (-1) and the bands between are peaks (1).
    pub fn peaks(&self) -> f64 {
        1.0 - (3.0 * self.weirdness.abs() - 2.0).abs()
    }

    /// 0 (frozen) to 4 (hot), modern's five temperature bands.
    pub fn temperature_band(&self) -> usize {
        band(self.temperature, TEMPERATURE_BANDS)
    }

    /// 0 (arid) to 4 (humid), modern's five humidity bands.
    pub fn humidity_band(&self) -> usize {
        band(self.humidity, HUMIDITY_BANDS)
    }
}

const TEMPERATURE_BANDS: [f64; 4] = [-0.6, -0.2, 0.2, 0.55];
const HUMIDITY_BANDS: [f64; 4] = [-0.6, -0.2, 0.2, 0.6];

fn band(value: f64, edges: [f64; 4]) -> usize {
    edges.iter().take_while(|&&edge| value >= edge).count()
}

/// Continentalness below this is ocean, below `DEEP_OCEAN` deep ocean.
pub const OCEAN: f64 = -0.3;
pub const DEEP_OCEAN: f64 = -0.6;
/// The shore runs from `OCEAN` up to this.
pub const COAST: f64 = -0.18;
/// Mushroom islands rise where continentalness is below this and the
/// mushroom noise is high.
const MUSHROOM_CONTINENT: f64 = -0.85;
const MUSHROOM_NOISE: f64 = 0.87;

fn smoothstep(edge0: f64, edge1: f64, x: f64) -> f64 {
    let t = ((x - edge0) / (edge1 - edge0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Standard deviation of `Fbm<Perlin>` output by octave count (measured
/// over 200,000 samples), to spread each field evenly.
fn spread(octaves: usize) -> f64 {
    match octaves {
        1 | 2 => 0.324,
        3 => 0.285,
        4 => 0.267,
        _ => 0.259,
    }
}

/// The error function (Abramowitz and Stegun 7.1.26, within 1.5e-7).
fn erf(x: f64) -> f64 {
    let t = 1.0 / (1.0 + 0.327_591_1 * x.abs());
    let poly = t
        * (0.254_829_592
            + t * (-0.284_496_736
                + t * (1.421_413_741 + t * (-1.453_152_027 + t * 1.061_405_429))));
    let y = 1.0 - poly * (-x * x).exp();
    if x < 0.0 { -y } else { y }
}

/// A noise field spread evenly over -1..1.
struct Field {
    noise: Fbm<Perlin>,
    scale: f64,
    offset: [f64; 2],
}

impl Field {
    fn new(seed: u64, index: u64, octaves: usize, wavelength: f64) -> Self {
        Self {
            noise: Fbm::<Perlin>::new(noise_seed(seed, index))
                .set_octaves(octaves)
                .set_frequency(1.0 / wavelength),
            scale: 1.0 / (spread(octaves) * std::f64::consts::SQRT_2),
            offset: {
                let [x, _, z] = noise_offset(seed, index);
                [x, z]
            },
        }
    }

    fn get(&self, x: f64, z: f64) -> f64 {
        erf(self.noise.get([x + self.offset[0], z + self.offset[1]]) * self.scale)
    }
}

/// A 32-bit noise seed derived from the world seed, different for each `index`.
pub fn noise_seed(seed: u64, index: u64) -> u32 {
    mix(seed ^ mix(index)) as u32
}

/// A shift for a noise field's coordinates, different for each `index`.
/// Perlin noise is zero on its lattice, which would otherwise pin every
/// field to the same value at the origin (a river at 0, 0 in every world).
pub fn noise_offset(seed: u64, index: u64) -> [f64; 3] {
    let h = mix(seed ^ mix(index ^ 0x0FF5E7));
    [h, h >> 21, h >> 42].map(|bits| (bits & 0xFFFFF) as f64 / 64.0 + 0.37)
}

pub struct ClimateNoise {
    continent: Field,
    erosion: Field,
    weirdness: Field,
    temperature: Field,
    humidity: Field,
    mushroom: Field,
}

impl ClimateNoise {
    pub fn new(seed: u64) -> Self {
        Self {
            continent: Field::new(seed, 1, 5, 1600.0),
            erosion: Field::new(seed, 11, 4, 1000.0),
            weirdness: Field::new(seed, 12, 3, 700.0),
            temperature: Field::new(seed, 2, 3, 1300.0),
            humidity: Field::new(seed, 3, 3, 1000.0),
            mushroom: Field::new(seed, 13, 2, 300.0),
        }
    }

    /// The climate exactly at (x, z), from the noise.
    pub fn sample(&self, x: i32, z: i32) -> Climate {
        let (x, z) = (x as f64, z as f64);
        Climate {
            continent: self.continent.get(x, z),
            erosion: self.erosion.get(x, z),
            weirdness: self.weirdness.get(x, z),
            temperature: self.temperature.get(x, z),
            humidity: self.humidity.get(x, z),
        }
    }

    /// The climate at (x, z), blended from the `GRID` samples around it.
    pub fn at(&self, x: i32, z: i32) -> Climate {
        let (gx, gz) = (x.div_euclid(GRID) * GRID, z.div_euclid(GRID) * GRID);
        let (tx, tz) = ((x - gx) as f64 / GRID as f64, (z - gz) as f64 / GRID as f64);
        blend(
            [
                self.sample(gx, gz),
                self.sample(gx + GRID, gz),
                self.sample(gx, gz + GRID),
                self.sample(gx + GRID, gz + GRID),
            ],
            tx,
            tz,
        )
    }

    /// How much a mushroom island rises here, far out to sea: 0 none, 1
    /// fully, between them its shore.
    pub fn mushroom(&self, x: i32, z: i32, climate: &Climate) -> f64 {
        if climate.continent > MUSHROOM_CONTINENT + 0.05 {
            return 0.0;
        }
        let noise = self.mushroom.get(x as f64, z as f64);
        smoothstep(MUSHROOM_NOISE - 0.05, MUSHROOM_NOISE + 0.05, noise)
            * smoothstep(
                MUSHROOM_CONTINENT + 0.05,
                MUSHROOM_CONTINENT,
                climate.continent,
            )
    }
}

/// Bilinear blend of the corner climates (x0z0, x1z0, x0z1, x1z1).
pub fn blend(corners: [Climate; 4], tx: f64, tz: f64) -> Climate {
    let near = corners[0].lerp(corners[1], tx);
    let far = corners[2].lerp(corners[3], tx);
    near.lerp(far, tz)
}

/// What the terrain is like at a column, besides its climate.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Lay {
    /// Y of the topmost solid block.
    pub height: i32,
    /// 0 (lowland) to 1 (mountain range), from `Shape::mountain`.
    pub mountain: f64,
    /// 1 in a river's bed, falling to 0 at its banks.
    pub river: f64,
    /// 1 where erosion has worn the land flat at sea level (swamps).
    pub flat: f64,
    /// Whether this is a mushroom island.
    pub mushroom: bool,
}

/// Height at which the slope biomes (grove, snowy slopes) begin.
const SLOPES: i32 = 140;
/// Height at which peaks begin.
const PEAKS: i32 = 175;
/// Height at which the highland biomes (meadow, cherry grove) begin.
const HIGHLAND: i32 = 108;
/// Height at which windswept hills begin.
const WINDSWEPT: i32 = 92;

/// The surface biome of a column (modern `MultiNoiseBiomeSource`, with
/// the overworld's parameter table cut down to the biomes Rustcraft has).
pub fn pick(climate: &Climate, lay: &Lay, sea_level: i32) -> Biome {
    let t = climate.temperature_band();
    let h = climate.humidity_band();
    let c = climate.continent;
    if lay.mushroom {
        return Biome::MushroomFields;
    }
    if c < OCEAN && lay.height < sea_level {
        let deep = c < DEEP_OCEAN;
        return match (t, deep) {
            (0, false) => Biome::FrozenOcean,
            (0, true) => Biome::DeepFrozenOcean,
            (1, false) => Biome::ColdOcean,
            (1, true) => Biome::DeepColdOcean,
            (2, false) => Biome::Ocean,
            (2, true) => Biome::DeepOcean,
            (3, false) => Biome::LukewarmOcean,
            (3, true) | (4, true) => Biome::DeepLukewarmOcean,
            _ => Biome::WarmOcean,
        };
    }
    if lay.river > 0.45 && lay.height < sea_level + 2 {
        return Biome::River;
    }
    let mountain = lay.mountain;
    if lay.height >= PEAKS && mountain > 0.35 {
        return if climate.weirdness >= 0.0 && t >= 1 {
            Biome::JaggedPeaks
        } else {
            Biome::FrozenPeaks
        };
    }
    if lay.height >= SLOPES && mountain > 0.2 {
        return match t {
            0..=2 if h >= 2 => Biome::Grove,
            0..=2 => Biome::SnowySlopes,
            3 => highland(climate),
            _ => Biome::Savanna,
        };
    }
    if c < COAST && lay.height <= sea_level + 3 {
        return if climate.erosion < -0.45 {
            Biome::StonyShore
        } else if t == 4 {
            Biome::Desert
        } else if t == 0 {
            Biome::SnowyPlains
        } else {
            Biome::Beach
        };
    }
    if lay.height >= HIGHLAND && mountain > 0.12 {
        return match t {
            0 | 1 if h >= 2 => Biome::Grove,
            0 | 1 => Biome::SnowySlopes,
            2 | 3 => highland(climate),
            _ => badlands_or(climate, Biome::Savanna),
        };
    }
    if lay.height >= WINDSWEPT && mountain > 0.12 && t <= 2 {
        return Biome::WindsweptHills;
    }
    if lay.flat > 0.5 && lay.height <= sea_level + 2 {
        match t {
            1 | 2 if h >= 1 => return Biome::Swamp,
            3 | 4 if h >= 2 => return Biome::MangroveSwamp,
            _ => {}
        }
    }
    middle(climate, t, h)
}

/// Meadows, or cherry groves where it's weird and humid enough.
fn highland(climate: &Climate) -> Biome {
    if climate.weirdness > 0.2 && climate.humidity > 0.0 {
        Biome::CherryGrove
    } else {
        Biome::Meadow
    }
}

/// Hot inland ground that is rough enough is badlands.
fn badlands_or(climate: &Climate, otherwise: Biome) -> Biome {
    if climate.erosion < 0.05 && climate.continent > -0.05 {
        Biome::Badlands
    } else {
        otherwise
    }
}

/// Modern's "middle biomes" table, by temperature and humidity band.
fn middle(climate: &Climate, t: usize, h: usize) -> Biome {
    use Biome::*;
    match t {
        0 if h <= 1 && climate.weirdness > 0.45 => IceSpikes,
        0 => [
            SnowyPlains,
            SnowyPlains,
            SnowyPlains,
            SnowyTaiga,
            SnowyTaiga,
        ][h],
        1 => [Plains, Plains, Forest, Taiga, Taiga][h],
        2 => [Plains, Plains, Forest, BirchForest, DarkForest][h],
        3 if h >= 3 && climate.weirdness > 0.25 => BambooJungle,
        3 => [Savanna, Savanna, Forest, Jungle, Jungle][h],
        _ => badlands_or(climate, Desert),
    }
}

/// The cave biome deep under a column, if any: lush caves under humid
/// patches, dripstone caves under others (modern places them by humidity
/// and continentalness underground; these patch fields stand in for that).
pub struct CaveBiomes {
    lush: Field,
    dripstone: Field,
}

const CAVE_BIOME_EDGE: f64 = 0.55;

impl CaveBiomes {
    pub fn new(seed: u64) -> Self {
        Self {
            lush: Field::new(seed, 21, 2, 260.0),
            dripstone: Field::new(seed, 22, 2, 260.0),
        }
    }

    pub fn at(&self, x: i32, z: i32) -> Option<Biome> {
        let (x, z) = (x as f64, z as f64);
        if self.lush.get(x, z) > CAVE_BIOME_EDGE {
            Some(Biome::LushCaves)
        } else if self.dripstone.get(x, z) > CAVE_BIOME_EDGE {
            Some(Biome::DripstoneCaves)
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn erf_matches_known_values() {
        for (x, want) in [
            (0.0, 0.0),
            (0.5, 0.520_499_9),
            (1.0, 0.842_700_8),
            (2.0, 0.995_322_3),
        ] {
            assert!((erf(x) - want).abs() < 1e-6, "erf({x})");
            assert!((erf(-x) + want).abs() < 1e-6);
        }
    }

    #[test]
    fn fields_are_spread_evenly() {
        let noise = ClimateNoise::new(3);
        let mut values: Vec<f64> = (0..400)
            .flat_map(|i| (0..400).map(move |j| (i * 211 - 40_000, j * 197 - 40_000)))
            .map(|(x, z)| noise.sample(x, z).temperature)
            .collect();
        values.sort_by(f64::total_cmp);
        let quantile = |p: f64| values[(p * (values.len() - 1) as f64) as usize];
        // Even over -1..1: the quartiles fall near -0.5, 0 and 0.5.
        for (p, want) in [(0.25, -0.5), (0.5, 0.0), (0.75, 0.5)] {
            assert!(
                (quantile(p) - want).abs() < 0.08,
                "quantile {p} is {}",
                quantile(p)
            );
        }
    }

    #[test]
    fn blended_climate_meets_the_samples_on_the_grid() {
        let noise = ClimateNoise::new(9);
        assert_eq!(noise.at(8, -12), noise.sample(8, -12));
        let mid = noise.at(10, -12);
        let (a, b) = (noise.sample(8, -12), noise.sample(12, -12));
        assert!((mid.erosion - (a.erosion + b.erosion) / 2.0).abs() < 1e-12);
    }

    #[test]
    fn peaks_fold_weirdness() {
        let at = |w| {
            Climate {
                weirdness: w,
                ..Default::default()
            }
            .peaks()
        };
        assert_eq!(at(0.0), -1.0);
        assert_eq!(at(1.0), 0.0);
        assert!((at(2.0 / 3.0) - 1.0).abs() < 1e-12);
    }
}
