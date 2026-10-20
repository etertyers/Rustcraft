//! The Overworld's biomes (modern Java Edition's, M21) and their grass and
//! foliage colors.

use anyhow::{Context, Result};
use image::RgbaImage;

/// A biome. The first four are v1.0's; saves store `id`, the position in
/// `ALL`, so new biomes only ever go on the end.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Hash, serde::Deserialize)]
pub enum Biome {
    Ocean,
    #[default]
    Plains,
    Forest,
    Desert,
    Beach,
    StonyShore,
    River,
    Taiga,
    SnowyPlains,
    SnowyTaiga,
    IceSpikes,
    Jungle,
    BambooJungle,
    Swamp,
    MangroveSwamp,
    Savanna,
    Badlands,
    BirchForest,
    DarkForest,
    WindsweptHills,
    Meadow,
    Grove,
    SnowySlopes,
    JaggedPeaks,
    FrozenPeaks,
    CherryGrove,
    MushroomFields,
    FrozenOcean,
    ColdOcean,
    LukewarmOcean,
    WarmOcean,
    DeepOcean,
    DeepFrozenOcean,
    DeepColdOcean,
    DeepLukewarmOcean,
    LushCaves,
    DripstoneCaves,
    NetherWastes,
    CrimsonForest,
    WarpedForest,
    SoulSandValley,
    BasaltDeltas,
}

/// How a biome's grass and foliage colors are worked out.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Tint {
    /// From the colormaps, by temperature and downfall.
    Climate,
    /// Modern dark forest: the climate color averaged with 0x28340A.
    DarkForest,
    /// Fixed (grass, foliage) colors.
    Fixed(u32, u32),
}

/// Modern `Biome` settings.
struct Info {
    name: &'static str,
    temperature: f32,
    downfall: f32,
    tint: Tint,
}

const fn info(name: &'static str, temperature: f32, downfall: f32) -> Info {
    Info {
        name,
        temperature,
        downfall,
        tint: Tint::Climate,
    }
}

const fn fixed(
    name: &'static str,
    temperature: f32,
    downfall: f32,
    grass: u32,
    foliage: u32,
) -> Info {
    Info {
        name,
        temperature,
        downfall,
        tint: Tint::Fixed(grass, foliage),
    }
}

/// Indexed by `Biome::id`; values from modern `OverworldBiomes`.
const INFO: [Info; Biome::COUNT] = [
    info("Ocean", 0.5, 0.5),
    info("Plains", 0.8, 0.4),
    info("Forest", 0.7, 0.8),
    info("Desert", 2.0, 0.0),
    info("Beach", 0.8, 0.4),
    info("Stony Shore", 0.2, 0.3),
    info("River", 0.5, 0.5),
    info("Taiga", 0.25, 0.8),
    info("Snowy Plains", 0.0, 0.5),
    info("Snowy Taiga", -0.5, 0.4),
    info("Ice Spikes", 0.0, 0.5),
    info("Jungle", 0.95, 0.9),
    info("Bamboo Jungle", 0.95, 0.9),
    fixed("Swamp", 0.8, 0.9, 0x6A7039, 0x6A7039),
    fixed("Mangrove Swamp", 0.8, 0.9, 0x6A7039, 0x8DB127),
    info("Savanna", 2.0, 0.0),
    fixed("Badlands", 2.0, 0.0, 0x90814D, 0x9E814D),
    info("Birch Forest", 0.6, 0.6),
    Info {
        name: "Dark Forest",
        temperature: 0.7,
        downfall: 0.8,
        tint: Tint::DarkForest,
    },
    info("Windswept Hills", 0.2, 0.3),
    info("Meadow", 0.5, 0.8),
    info("Grove", -0.2, 0.8),
    info("Snowy Slopes", -0.3, 0.9),
    info("Jagged Peaks", -0.7, 0.9),
    info("Frozen Peaks", -0.7, 0.9),
    fixed("Cherry Grove", 0.5, 0.8, 0xB6DB61, 0xB6DB61),
    info("Mushroom Fields", 0.9, 1.0),
    info("Frozen Ocean", 0.0, 0.5),
    info("Cold Ocean", 0.5, 0.5),
    info("Lukewarm Ocean", 0.5, 0.5),
    info("Warm Ocean", 0.5, 0.5),
    info("Deep Ocean", 0.5, 0.5),
    info("Deep Frozen Ocean", 0.5, 0.5),
    info("Deep Cold Ocean", 0.5, 0.5),
    info("Deep Lukewarm Ocean", 0.5, 0.5),
    info("Lush Caves", 0.5, 0.5),
    info("Dripstone Caves", 0.8, 0.4),
    // Modern `NetherBiomes`: hot and dry, with no rain.
    info("Nether Wastes", 2.0, 0.0),
    info("Crimson Forest", 2.0, 0.0),
    info("Warped Forest", 2.0, 0.0),
    info("Soul Sand Valley", 2.0, 0.0),
    info("Basalt Deltas", 2.0, 0.0),
];

impl Biome {
    pub const COUNT: usize = 42;

    pub const ALL: [Biome; Biome::COUNT] = {
        use Biome::*;
        [
            Ocean,
            Plains,
            Forest,
            Desert,
            Beach,
            StonyShore,
            River,
            Taiga,
            SnowyPlains,
            SnowyTaiga,
            IceSpikes,
            Jungle,
            BambooJungle,
            Swamp,
            MangroveSwamp,
            Savanna,
            Badlands,
            BirchForest,
            DarkForest,
            WindsweptHills,
            Meadow,
            Grove,
            SnowySlopes,
            JaggedPeaks,
            FrozenPeaks,
            CherryGrove,
            MushroomFields,
            FrozenOcean,
            ColdOcean,
            LukewarmOcean,
            WarmOcean,
            DeepOcean,
            DeepFrozenOcean,
            DeepColdOcean,
            DeepLukewarmOcean,
            LushCaves,
            DripstoneCaves,
            NetherWastes,
            CrimsonForest,
            WarpedForest,
            SoulSandValley,
            BasaltDeltas,
        ]
    };

    /// The Nether's biomes (M24).
    pub const NETHER: [Biome; 5] = [
        Biome::NetherWastes,
        Biome::CrimsonForest,
        Biome::WarpedForest,
        Biome::SoulSandValley,
        Biome::BasaltDeltas,
    ];

    fn info(self) -> &'static Info {
        &INFO[self.index()]
    }

    pub fn name(self) -> &'static str {
        self.info().name
    }

    /// Modern (temperature, downfall), which pick the biome's colors.
    pub fn climate(self) -> (f32, f32) {
        let info = self.info();
        (info.temperature, info.downfall)
    }

    /// Modern `Biome.getTemperature`: the base temperature, colder by
    /// 0.00125 a block above y 80 (noise left out).
    pub fn temperature_at(self, y: i32) -> f32 {
        let base = self.info().temperature;
        if y > SNOW_LINE {
            base - (y - SNOW_LINE) as f32 * 0.00125
        } else {
            base
        }
    }

    /// Whether water freezes and snow settles at height `y` (modern
    /// `coldEnoughToSnow`: temperature below 0.15). The frozen oceans count
    /// as cold whatever their temperature, as modern ones do.
    pub fn is_cold_at(self, y: i32) -> bool {
        matches!(self, Biome::FrozenOcean | Biome::DeepFrozenOcean) || self.temperature_at(y) < 0.15
    }

    pub fn is_ocean(self) -> bool {
        matches!(
            self,
            Biome::Ocean
                | Biome::FrozenOcean
                | Biome::ColdOcean
                | Biome::LukewarmOcean
                | Biome::WarmOcean
                | Biome::DeepOcean
                | Biome::DeepFrozenOcean
                | Biome::DeepColdOcean
                | Biome::DeepLukewarmOcean
        )
    }

    pub fn is_nether(self) -> bool {
        Self::NETHER.contains(&self)
    }

    /// The fog color of a Nether biome (sRGB), from modern `NetherBiomes`.
    pub fn nether_fog(self) -> Option<u32> {
        Some(match self {
            Biome::NetherWastes => 0x330808,
            Biome::CrimsonForest => 0x330303,
            Biome::WarpedForest => 0x1A051A,
            Biome::SoulSandValley => 0x1B4745,
            Biome::BasaltDeltas => 0x685F70,
            _ => return None,
        })
    }

    /// Lush and dripstone caves, which lie under other biomes.
    pub fn is_cave(self) -> bool {
        matches!(self, Biome::LushCaves | Biome::DripstoneCaves)
    }

    fn index(self) -> usize {
        self as usize
    }

    /// Stable number for saving: the position in `ALL`.
    pub fn id(self) -> u8 {
        self.index() as u8
    }

    pub fn from_id(id: u8) -> Option<Self> {
        Self::ALL.get(id as usize).copied()
    }
}

/// Height above which biomes get colder (modern: 80).
const SNOW_LINE: i32 = 80;

/// sRGB grass and foliage colors for each biome.
#[derive(Debug, Clone, PartialEq)]
pub struct BiomeColors {
    grass: [[f32; 3]; Biome::COUNT],
    foliage: [[f32; 3]; Biome::COUNT],
}

impl BiomeColors {
    /// Samples `colormap/grass.png` and `colormap/foliage.png` from the
    /// highest layers that have them.
    pub fn load(res: &crate::pack::Resources) -> Result<Self> {
        let open = |name: &str| {
            res.image(name)
                .with_context(|| format!("no texture {name}"))
        };
        Ok(Self::from_colormaps(
            &open("colormap/grass")?,
            &open("colormap/foliage")?,
        ))
    }

    /// Looks each biome's climate up in the colormaps, the way modern
    /// Minecraft does, then applies the biomes with their own colors.
    pub fn from_colormaps(grass: &RgbaImage, foliage: &RgbaImage) -> Self {
        let sample = |map: &RgbaImage, biome: Biome| {
            let (temperature, rainfall) = biome.climate();
            let (x, y) = colormap_coords(temperature, rainfall, map.width(), map.height());
            let [r, g, b, _] = map.get_pixel(x, y).0;
            [r, g, b].map(|c| c as f32 / 255.0)
        };
        let color = |map: &RgbaImage, biome: Biome, grass: bool| match biome.info().tint {
            Tint::Climate => sample(map, biome),
            // Modern `DARK_FOREST`: (color & 0xFEFEFE) + 0x28340A, halved.
            Tint::DarkForest if grass => {
                let base = sample(map, biome);
                let add = rgb(0x28340A);
                [0, 1, 2].map(|i| {
                    let even = ((base[i] * 255.0).round() as u32 & 0xFE) as f32 / 255.0;
                    (even + add[i]) / 2.0
                })
            }
            Tint::DarkForest => sample(map, biome),
            Tint::Fixed(g, f) => rgb(if grass { g } else { f }),
        };
        Self {
            grass: Biome::ALL.map(|b| color(grass, b, true)),
            foliage: Biome::ALL.map(|b| color(foliage, b, false)),
        }
    }

    /// Every biome uses vanilla plains colors (for tests, or if the
    /// colormaps are missing).
    pub fn uniform() -> Self {
        let grass = rgb(0x91BD59);
        let foliage = rgb(0x77AB2F);
        Self {
            grass: [grass; Biome::COUNT],
            foliage: [foliage; Biome::COUNT],
        }
    }

    pub fn grass(&self, biome: Biome) -> [f32; 3] {
        self.grass[biome.index()]
    }

    pub fn foliage(&self, biome: Biome) -> [f32; 3] {
        self.foliage[biome.index()]
    }
}

/// 0xRRGGBB to sRGB floats.
pub fn rgb(hex: u32) -> [f32; 3] {
    [hex >> 16, hex >> 8, hex].map(|c| (c & 0xFF) as f32 / 255.0)
}

/// Pixel in a colormap for a climate: temperature runs right to left and
/// rainfall × temperature runs bottom to top, both clamped to 0..1.
fn colormap_coords(temperature: f32, rainfall: f32, width: u32, height: u32) -> (u32, u32) {
    let t = temperature.clamp(0.0, 1.0);
    let r = rainfall.clamp(0.0, 1.0) * t;
    let x = ((1.0 - t) * (width - 1) as f32) as u32;
    let y = ((1.0 - r) * (height - 1) as f32) as u32;
    (x, y)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colormap_coords_match_1_0_0() {
        // Plains: temperature 0.8, rainfall 0.4 × 0.8. (1 - 0.8) × 255 is just
        // under 51 in floating point, and 1.0.0 truncates it to 50 as well.
        assert_eq!(colormap_coords(0.8, 0.4, 256, 256), (50, 173));
        // Desert is hotter than 1.0 and clamps to the hot, dry corner.
        assert_eq!(colormap_coords(2.0, 0.0, 256, 256), (0, 255));
    }

    #[test]
    fn each_biome_samples_its_own_pixel() {
        let map = RgbaImage::from_fn(256, 256, |x, y| image::Rgba([x as u8, y as u8, 0, 255]));
        let colors = BiomeColors::from_colormaps(&map, &map);
        let plains = colors.grass(Biome::Plains);
        assert_eq!(plains, [50.0 / 255.0, 173.0 / 255.0, 0.0]);
        assert_ne!(colors.grass(Biome::Desert), plains);
    }

    #[test]
    fn swamps_badlands_and_cherry_groves_have_their_own_colors() {
        let map = RgbaImage::from_pixel(256, 256, image::Rgba([0, 255, 0, 255]));
        let colors = BiomeColors::from_colormaps(&map, &map);
        assert_eq!(colors.grass(Biome::Swamp), rgb(0x6A7039));
        assert_eq!(colors.foliage(Biome::MangroveSwamp), rgb(0x8DB127));
        assert_eq!(colors.grass(Biome::Badlands), rgb(0x90814D));
        assert_eq!(colors.foliage(Biome::CherryGrove), rgb(0xB6DB61));
        // Dark forest grass is the colormap's green averaged with 0x28340A.
        let dark = colors.grass(Biome::DarkForest);
        let expected = [0x28, (0xFE + 0x34), 0x0A].map(|c| c as f32 / 2.0 / 255.0);
        for (got, want) in dark.iter().zip(expected) {
            assert!((got - want).abs() < 1e-6, "{dark:?}");
        }
        assert_eq!(colors.foliage(Biome::DarkForest), rgb(0x00FF00));
    }

    #[test]
    fn real_colormaps_load() {
        let base = crate::pack::Resources::load(&crate::assets::dir(), Vec::new()).unwrap();
        let colors = BiomeColors::load(&base).unwrap();
        // Forest grass is a different green from desert grass.
        assert_ne!(colors.grass(Biome::Forest), colors.grass(Biome::Desert));
    }

    #[test]
    fn ids_are_positions_and_v1_biomes_keep_theirs() {
        for (i, biome) in Biome::ALL.iter().enumerate() {
            assert_eq!(biome.id() as usize, i);
            assert_eq!(Biome::from_id(i as u8), Some(*biome));
        }
        assert_eq!(Biome::from_id(Biome::COUNT as u8), None);
        let v1 = [Biome::Ocean, Biome::Plains, Biome::Forest, Biome::Desert];
        assert_eq!(v1.map(Biome::id), [0, 1, 2, 3]);
    }

    #[test]
    fn mountains_are_colder_higher_up() {
        // Plains never freeze low down, but a windswept hill top does.
        assert!(!Biome::Plains.is_cold_at(64));
        assert!(!Biome::WindsweptHills.is_cold_at(80));
        assert!(Biome::WindsweptHills.is_cold_at(130));
        assert!(Biome::SnowyPlains.is_cold_at(64));
        assert!(Biome::FrozenOcean.is_cold_at(63));
        assert!(!Biome::Desert.is_cold_at(250));
    }

    #[test]
    fn rgb_unpacks_hex() {
        assert_eq!(rgb(0xFF8000), [1.0, 128.0 / 255.0, 0.0]);
    }
}
