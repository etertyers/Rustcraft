//! Picking a biome from the climate, as modern Minecraft's
//! `OverworldBiomeBuilder` does: each parameter falls in a band, and the
//! bands index vanilla's tables (see `docs/worldgen.md`). The tables are
//! written with vanilla's biomes; those Rustcraft doesn't have yet map to
//! the nearest one it does (`fallback`). Biomes are chosen once per 4×4
//! column ("quart") from the climate at its corner, with no blending and
//! no randomness.

use super::climate::{COAST, Climate, DEEP_OCEAN, MUSHROOM, OCEAN};
use crate::world::biome::Biome;

/// Vanilla's overworld biomes, as the tables name them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Vanilla {
    MushroomFields,
    DeepFrozenOcean,
    DeepColdOcean,
    DeepOcean,
    DeepLukewarmOcean,
    WarmOcean,
    FrozenOcean,
    ColdOcean,
    Ocean,
    LukewarmOcean,
    SnowyPlains,
    SnowyTaiga,
    Taiga,
    Plains,
    Forest,
    OldGrowthSpruceTaiga,
    OldGrowthPineTaiga,
    FlowerForest,
    BirchForest,
    OldGrowthBirchForest,
    DarkForest,
    Savanna,
    SavannaPlateau,
    WindsweptSavanna,
    Jungle,
    SparseJungle,
    BambooJungle,
    Desert,
    IceSpikes,
    SunflowerPlains,
    Meadow,
    CherryGrove,
    Badlands,
    WoodedBadlands,
    ErodedBadlands,
    WindsweptGravellyHills,
    WindsweptHills,
    WindsweptForest,
    JaggedPeaks,
    FrozenPeaks,
    StonyPeaks,
    SnowySlopes,
    Grove,
    StonyShore,
    Beach,
    SnowyBeach,
    Swamp,
    MangroveSwamp,
    River,
    FrozenRiver,
}

/// The Rustcraft biome standing in for a vanilla one.
pub fn fallback(v: Vanilla) -> Biome {
    use Vanilla as V;
    match v {
        V::MushroomFields => Biome::MushroomFields,
        V::DeepFrozenOcean => Biome::DeepFrozenOcean,
        V::DeepColdOcean => Biome::DeepColdOcean,
        V::DeepOcean => Biome::DeepOcean,
        V::DeepLukewarmOcean => Biome::DeepLukewarmOcean,
        V::WarmOcean => Biome::WarmOcean,
        V::FrozenOcean => Biome::FrozenOcean,
        V::ColdOcean => Biome::ColdOcean,
        V::Ocean => Biome::Ocean,
        V::LukewarmOcean => Biome::LukewarmOcean,
        V::SnowyPlains => Biome::SnowyPlains,
        V::SnowyTaiga => Biome::SnowyTaiga,
        V::Taiga | V::OldGrowthSpruceTaiga | V::OldGrowthPineTaiga => Biome::Taiga,
        V::Plains | V::SunflowerPlains => Biome::Plains,
        V::Forest | V::FlowerForest => Biome::Forest,
        V::BirchForest | V::OldGrowthBirchForest => Biome::BirchForest,
        V::DarkForest => Biome::DarkForest,
        V::Savanna | V::SavannaPlateau | V::WindsweptSavanna => Biome::Savanna,
        V::Jungle | V::SparseJungle => Biome::Jungle,
        V::BambooJungle => Biome::BambooJungle,
        V::Desert => Biome::Desert,
        V::IceSpikes => Biome::IceSpikes,
        V::Meadow => Biome::Meadow,
        V::CherryGrove => Biome::CherryGrove,
        V::Badlands | V::WoodedBadlands | V::ErodedBadlands => Biome::Badlands,
        V::WindsweptGravellyHills | V::WindsweptHills | V::WindsweptForest | V::StonyPeaks => {
            Biome::WindsweptHills
        }
        V::JaggedPeaks => Biome::JaggedPeaks,
        V::FrozenPeaks => Biome::FrozenPeaks,
        V::SnowySlopes => Biome::SnowySlopes,
        V::Grove => Biome::Grove,
        V::StonyShore => Biome::StonyShore,
        V::Beach | V::SnowyBeach => Biome::Beach,
        V::Swamp => Biome::Swamp,
        V::MangroveSwamp => Biome::MangroveSwamp,
        V::River | V::FrozenRiver => Biome::River,
    }
}

/// Which band a value is in, given the inner edges.
fn band(value: f64, edges: &[f64]) -> usize {
    edges.iter().take_while(|&&edge| value >= edge).count()
}

/// T0..T4, H0..H4, E0..E6 (modern's edges).
pub fn temperature_band(t: f64) -> usize {
    band(t, &[-0.45, -0.15, 0.2, 0.55])
}
pub fn humidity_band(h: f64) -> usize {
    band(h, &[-0.35, -0.1, 0.1, 0.3])
}
pub fn erosion_band(e: f64) -> usize {
    band(e, &[-0.78, -0.375, -0.2225, 0.05, 0.45, 0.55])
}

/// Continentalness bands inland of the ocean.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Inland {
    Coast,
    Near,
    Mid,
    Far,
}

/// Peaks-and-valleys bands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pv {
    Valleys,
    Low,
    Mid,
    High,
    Peaks,
}

pub fn pv_band(pv: f64) -> Pv {
    match pv {
        v if v < -0.85 => Pv::Valleys,
        v if v < -0.2 => Pv::Low,
        v if v < 0.2 => Pv::Mid,
        v if v < 0.7 => Pv::High,
        _ => Pv::Peaks,
    }
}

use Vanilla as V;

const MIDDLE: [[V; 5]; 5] = [
    [
        V::SnowyPlains,
        V::SnowyPlains,
        V::SnowyPlains,
        V::SnowyTaiga,
        V::Taiga,
    ],
    [
        V::Plains,
        V::Plains,
        V::Forest,
        V::Taiga,
        V::OldGrowthSpruceTaiga,
    ],
    [
        V::FlowerForest,
        V::Plains,
        V::Forest,
        V::BirchForest,
        V::DarkForest,
    ],
    [V::Savanna, V::Savanna, V::Forest, V::Jungle, V::Jungle],
    [V::Desert, V::Desert, V::Desert, V::Desert, V::Desert],
];

const MIDDLE_VARIANT: [[Option<V>; 5]; 5] = [
    [Some(V::IceSpikes), None, Some(V::SnowyTaiga), None, None],
    [None, None, None, None, Some(V::OldGrowthPineTaiga)],
    [
        Some(V::SunflowerPlains),
        None,
        None,
        Some(V::OldGrowthBirchForest),
        None,
    ],
    [
        None,
        None,
        Some(V::Plains),
        Some(V::SparseJungle),
        Some(V::BambooJungle),
    ],
    [None, None, None, None, None],
];

const PLATEAU: [[V; 5]; 5] = [
    [
        V::SnowyPlains,
        V::SnowyPlains,
        V::SnowyPlains,
        V::SnowyTaiga,
        V::SnowyTaiga,
    ],
    [
        V::Meadow,
        V::Meadow,
        V::Forest,
        V::Taiga,
        V::OldGrowthSpruceTaiga,
    ],
    [V::Meadow, V::Meadow, V::Meadow, V::Meadow, V::DarkForest],
    [
        V::SavannaPlateau,
        V::SavannaPlateau,
        V::Forest,
        V::Forest,
        V::Jungle,
    ],
    [
        V::Badlands,
        V::Badlands,
        V::Badlands,
        V::WoodedBadlands,
        V::WoodedBadlands,
    ],
];

const PLATEAU_VARIANT: [[Option<V>; 5]; 5] = [
    [Some(V::IceSpikes), None, None, None, None],
    [
        Some(V::CherryGrove),
        None,
        Some(V::Meadow),
        Some(V::Meadow),
        Some(V::OldGrowthPineTaiga),
    ],
    [
        Some(V::CherryGrove),
        Some(V::CherryGrove),
        Some(V::Forest),
        Some(V::BirchForest),
        None,
    ],
    [None, None, None, None, None],
    [
        Some(V::ErodedBadlands),
        Some(V::ErodedBadlands),
        None,
        None,
        None,
    ],
];

const SHATTERED: [[Option<V>; 5]; 5] = [
    [
        Some(V::WindsweptGravellyHills),
        Some(V::WindsweptGravellyHills),
        Some(V::WindsweptHills),
        Some(V::WindsweptForest),
        Some(V::WindsweptForest),
    ],
    [
        Some(V::WindsweptGravellyHills),
        Some(V::WindsweptGravellyHills),
        Some(V::WindsweptHills),
        Some(V::WindsweptForest),
        Some(V::WindsweptForest),
    ],
    [
        Some(V::WindsweptHills),
        Some(V::WindsweptHills),
        Some(V::WindsweptHills),
        Some(V::WindsweptForest),
        Some(V::WindsweptForest),
    ],
    [None, None, None, None, None],
    [None, None, None, None, None],
];

/// The bands of one quart, and the picks the tables are written in.
struct Pick {
    t: usize,
    h: usize,
    /// The sign of weirdness picks variants.
    w: f64,
}

impl Pick {
    fn variant(&self) -> bool {
        self.w > 0.0
    }

    /// M: the middle biome (its variant where weirdness is positive).
    fn m(&self) -> V {
        let variant = MIDDLE_VARIANT[self.t][self.h].filter(|_| self.variant());
        variant.unwrap_or(MIDDLE[self.t][self.h])
    }

    fn badlands(&self) -> V {
        match self.h {
            0 | 1 if self.w < 0.0 => V::Badlands,
            0 | 1 => V::ErodedBadlands,
            2 => V::Badlands,
            _ => V::WoodedBadlands,
        }
    }

    /// MB: M, but badlands where hot.
    fn mb(&self) -> V {
        if self.t == 4 {
            self.badlands()
        } else {
            self.m()
        }
    }

    /// MBS: MB, but a slope where frozen.
    fn mbs(&self) -> V {
        if self.t == 0 { self.slope() } else { self.mb() }
    }

    /// P: the plateau biome (its variant where weirdness is positive).
    fn p(&self) -> V {
        let variant = PLATEAU_VARIANT[self.t][self.h].filter(|_| self.variant());
        variant.unwrap_or(PLATEAU[self.t][self.h])
    }

    /// S: shattered, or M where too warm.
    fn s(&self) -> V {
        SHATTERED[self.t][self.h].unwrap_or_else(|| self.m())
    }

    /// WS(x): windswept savanna where warm, not humid and weird.
    fn ws(&self, x: V) -> V {
        if self.t >= 2 && self.h <= 3 && self.w >= 0.0 {
            V::WindsweptSavanna
        } else {
            x
        }
    }

    fn peak(&self) -> V {
        match self.t {
            0..=2 if self.w < 0.0 => V::JaggedPeaks,
            0..=2 => V::FrozenPeaks,
            3 => V::StonyPeaks,
            _ => self.badlands(),
        }
    }

    fn slope(&self) -> V {
        match (self.t, self.h) {
            (3.., _) => self.p(),
            (_, 0 | 1) => V::SnowySlopes,
            _ => V::Grove,
        }
    }

    fn beach(&self) -> V {
        match self.t {
            0 => V::SnowyBeach,
            4 => V::Desert,
            _ => V::Beach,
        }
    }

    /// SC: WS of M (or the beach where weirdness is negative).
    fn sc(&self) -> V {
        self.ws(if self.w >= 0.0 {
            self.m()
        } else {
            self.beach()
        })
    }

    fn swamp(&self) -> V {
        match self.t {
            0 => self.m(),
            1 | 2 => V::Swamp,
            _ => V::MangroveSwamp,
        }
    }

    fn river(&self) -> V {
        if self.t == 0 {
            V::FrozenRiver
        } else {
            V::River
        }
    }

    fn beach_or_m(&self) -> V {
        if self.w < 0.0 { self.beach() } else { self.m() }
    }

    /// The inland table: rows by peaks-and-valleys and erosion, columns
    /// by continentalness.
    fn inland(&self, pv: Pv, e: usize, c: Inland) -> V {
        use Inland::*;
        let col = |coast: V, near: V, mid: V, far: V| match c {
            Coast => coast,
            Near => near,
            Mid => mid,
            Far => far,
        };
        let (m, mb, mbs, p, s, pk, sl) = (
            self.m(),
            self.mb(),
            self.mbs(),
            self.p(),
            self.s(),
            self.peak(),
            self.slope(),
        );
        let wsm = self.ws(m);
        match pv {
            Pv::Peaks => match e {
                0 => pk,
                1 => col(mbs, mbs, pk, pk),
                2 => col(m, m, p, p),
                3 => col(m, m, mb, p),
                5 => col(wsm, wsm, s, s),
                _ => m,
            },
            Pv::High => match e {
                0 => col(m, sl, pk, pk),
                1 => col(m, mbs, sl, sl),
                2 => col(m, m, p, p),
                3 => col(m, m, mb, p),
                5 => col(wsm, wsm, s, s),
                _ => m,
            },
            Pv::Mid => match e {
                0 => col(V::StonyShore, sl, sl, sl),
                1 => col(V::StonyShore, mbs, mbs, if self.t == 0 { sl } else { p }),
                2 => col(V::StonyShore, m, mb, p),
                3 => col(m, m, mb, mb),
                4 => col(self.beach_or_m(), m, m, m),
                5 => col(self.sc(), wsm, s, s),
                _ => col(self.beach_or_m(), self.swamp(), self.swamp(), self.swamp()),
            },
            Pv::Low => match e {
                0 | 1 => col(V::StonyShore, mbs, mbs, mbs),
                2 => col(V::StonyShore, m, mb, mb),
                3 => col(self.beach(), m, mb, mb),
                4 => col(self.beach(), m, m, m),
                5 => col(self.sc(), wsm, s, s),
                _ => col(self.beach(), self.swamp(), self.swamp(), self.swamp()),
            },
            Pv::Valleys => match e {
                0 | 1 => col(
                    if self.w < 0.0 {
                        V::StonyShore
                    } else {
                        self.river()
                    },
                    self.river(),
                    mb,
                    mb,
                ),
                6 => col(self.river(), self.swamp(), self.swamp(), self.swamp()),
                _ => self.river(),
            },
        }
    }
}

/// The vanilla surface biome for a climate.
pub fn vanilla_surface(c: &Climate) -> V {
    let pick = Pick {
        t: temperature_band(c.temperature),
        h: humidity_band(c.humidity),
        w: c.weirdness,
    };
    let continent = c.continent;
    if continent < MUSHROOM {
        return V::MushroomFields;
    }
    if continent < OCEAN {
        let deep = continent < DEEP_OCEAN;
        return match (pick.t, deep) {
            (0, true) => V::DeepFrozenOcean,
            (1, true) => V::DeepColdOcean,
            (2, true) => V::DeepOcean,
            (3, true) => V::DeepLukewarmOcean,
            (0, false) => V::FrozenOcean,
            (1, false) => V::ColdOcean,
            (2, false) => V::Ocean,
            (3, false) => V::LukewarmOcean,
            _ => V::WarmOcean,
        };
    }
    let inland = match continent {
        c if c < COAST => Inland::Coast,
        c if c < 0.03 => Inland::Near,
        c if c < 0.3 => Inland::Mid,
        _ => Inland::Far,
    };
    pick.inland(pv_band(c.peaks()), erosion_band(c.erosion), inland)
}

/// The surface biome Rustcraft uses for a climate.
pub fn surface(c: &Climate) -> Biome {
    fallback(vanilla_surface(c))
}

/// Cave biomes: from depth 0.2 to 0.9 under the surface (0 at the surface,
/// 1/128 more per block down), lush caves where humidity is 0.7 or more,
/// dripstone caves where continentalness is 0.8 or more.
pub const CAVE_DEPTH: (f64, f64) = (0.2, 0.9);

pub fn cave(c: &Climate, depth: f64) -> Option<Biome> {
    if !(CAVE_DEPTH.0..=CAVE_DEPTH.1).contains(&depth) {
        return None;
    }
    if c.humidity >= 0.7 {
        Some(Biome::LushCaves)
    } else if c.continent >= 0.8 {
        Some(Biome::DripstoneCaves)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A value inside each band.
    const T: [f64; 5] = [-0.7, -0.3, 0.0, 0.4, 0.8];
    const H: [f64; 5] = [-0.6, -0.2, 0.0, 0.2, 0.6];
    const E: [f64; 7] = [-0.9, -0.5, -0.3, -0.1, 0.2, 0.5, 0.8];
    const C: [f64; 8] = [-1.2, -0.7, -0.3, -0.15, -0.05, 0.1, 0.5, 0.9];
    /// Weirdness for each PV band, on both signs.
    const W: [f64; 10] = [0.02, -0.02, 0.3, -0.3, 0.5, -0.5, 0.6, -0.6, 0.66, -0.66];

    #[test]
    fn every_band_combination_has_a_biome() {
        let mut seen = std::collections::HashSet::new();
        for t in T {
            for h in H {
                for e in E {
                    for c in C {
                        for w in W {
                            let climate = Climate {
                                temperature: t,
                                humidity: h,
                                continent: c,
                                erosion: e,
                                weirdness: w,
                            };
                            seen.insert(surface(&climate));
                        }
                    }
                }
            }
        }
        // Every surface biome Rustcraft has comes up somewhere.
        for biome in Biome::ALL {
            if !matches!(biome, Biome::LushCaves | Biome::DripstoneCaves) {
                assert!(seen.contains(&biome), "{biome:?} never picked");
            }
        }
    }

    #[test]
    fn the_bands_follow_modern_edges() {
        assert_eq!(temperature_band(-0.45), 1);
        assert_eq!(temperature_band(0.55), 4);
        assert_eq!(erosion_band(-0.2225), 3);
        assert_eq!(pv_band(-0.9), Pv::Valleys);
        assert_eq!(pv_band(0.75), Pv::Peaks);
    }

    #[test]
    fn some_known_spots() {
        let c = |t, h, cont, e, w| Climate {
            temperature: t,
            humidity: h,
            continent: cont,
            erosion: e,
            weirdness: w,
        };
        // Weirdness near zero is a valley: a river, frozen where cold.
        assert_eq!(vanilla_surface(&c(0.0, 0.0, 0.1, 0.2, 0.01)), V::River);
        assert_eq!(
            vanilla_surface(&c(-0.8, 0.0, 0.1, 0.2, 0.01)),
            V::FrozenRiver
        );
        // Hot and dry in the middle: desert; deep water: deep ocean.
        assert_eq!(vanilla_surface(&c(0.8, -0.6, 0.1, 0.2, -0.3)), V::Desert);
        assert_eq!(vanilla_surface(&c(0.0, 0.0, -0.6, 0.0, 0.3)), V::DeepOcean);
        // Low erosion far inland on a peak: jagged or frozen peaks.
        assert_eq!(
            vanilla_surface(&c(-0.3, 0.0, 0.5, -0.9, -0.66)),
            V::JaggedPeaks
        );
        assert_eq!(
            vanilla_surface(&c(-0.3, 0.0, 0.5, -0.9, 0.66)),
            V::FrozenPeaks
        );
        // Flat, wet ground: swamp, or mangroves where hot.
        assert_eq!(vanilla_surface(&c(0.0, 0.0, 0.1, 0.8, 0.3)), V::Swamp);
        assert_eq!(
            vanilla_surface(&c(0.4, 0.0, 0.1, 0.8, 0.3)),
            V::MangroveSwamp
        );
    }

    #[test]
    fn caves_only_between_their_depths() {
        let humid = Climate {
            humidity: 0.8,
            ..Default::default()
        };
        assert_eq!(cave(&humid, 0.5), Some(Biome::LushCaves));
        assert_eq!(cave(&humid, 0.1), None);
        let inland = Climate {
            continent: 0.9,
            ..Default::default()
        };
        assert_eq!(cave(&inland, 0.3), Some(Biome::DripstoneCaves));
        assert_eq!(cave(&Climate::default(), 0.5), None);
    }
}
