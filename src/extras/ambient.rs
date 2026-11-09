//! Ambient Sounds (M65), after the mod of that name: looping ambient beds
//! that fade in and out with where the player is. Its regions, sound
//! categories and sound files are read from its jar at runtime; this reads
//! the conditions as they're written there (height above the ground, sky
//! light, underwater, rain, temperature, biomes, other regions, and each
//! category's time-of-day curve) and says which beds should be sounding and
//! how loud.

use std::collections::HashMap;

use serde_json::Value;

/// Everything the conditions look at.
#[derive(Debug, Clone, Default)]
pub struct Situation {
    pub dimension: Dimension,
    /// The biome's name, lower case without separators (`darkforest`).
    pub biome: String,
    pub temperature: f32,
    /// Hour of the day, 0–24 (6 is sunrise).
    pub hour: f32,
    pub y: f32,
    /// Height above the ground under the player (negative in a cave).
    pub relative_height: f32,
    pub sky_light: f32,
    /// How much sky is in view, 0–1.
    pub sky: f32,
    /// Blocks of water over the head (0 when not underwater).
    pub underwater: f32,
    pub raining: bool,
    pub thunder: bool,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Dimension {
    #[default]
    Overworld,
    Nether,
    End,
}

/// A numeric condition: inside `min..=max` it's 1, and it falls to 0 over
/// `fade` beyond either end.
#[derive(Debug, Clone, Copy, Default)]
struct Span {
    min: Option<f32>,
    max: Option<f32>,
    fade: f32,
}

impl Span {
    fn of(value: Option<&Value>) -> Option<Self> {
        let o = value?.as_object()?;
        let get = |k: &str| o.get(k).and_then(Value::as_f64).map(|v| v as f32);
        Some(Self {
            min: get("min"),
            max: get("max"),
            fade: get("fade").unwrap_or(0.0),
        })
    }

    fn at(self, v: f32) -> f32 {
        let outside = match (self.min, self.max) {
            (Some(lo), _) if v < lo => lo - v,
            (_, Some(hi)) if v > hi => v - hi,
            _ => return 1.0,
        };
        if self.fade <= 0.0 {
            0.0
        } else {
            (1.0 - outside / self.fade).clamp(0.0, 1.0)
        }
    }
}

#[derive(Debug, Clone, Default)]
struct Variant {
    regions: Vec<String>,
    biomes: Vec<String>,
    raining: Option<bool>,
    overall_raining: Option<bool>,
    temperature: Option<Span>,
    volume: f32,
}

/// One bed of sound in a region.
#[derive(Debug, Clone)]
pub struct SoundSpec {
    pub name: String,
    /// Events of the mod's `sounds.json` (`ambientsounds:animals.bird-ambience1`).
    pub files: Vec<String>,
    pub category: String,
    pub volume: f32,
    /// Seconds a file plays before the next, if it's cut short.
    pub length: Option<(f32, f32)>,
    /// Seconds of silence between plays.
    pub pause: Option<(f32, f32)>,
    pub transition: f32,
}

#[derive(Debug, Clone, Default)]
struct Region {
    name: String,
    regions: Vec<String>,
    bad_regions: Vec<String>,
    bad_biomes: Vec<String>,
    biomes: Vec<String>,
    variants: Vec<Variant>,
    relative_height: Option<Span>,
    absolute_height: Option<Span>,
    sky_light: Option<Span>,
    sky: Option<Span>,
    underwater: Option<Span>,
    temperature: Option<Span>,
    raining: Option<bool>,
    mute: f32,
    mute_priority: i32,
    sounds: Vec<SoundSpec>,
}

#[derive(Debug, Clone, Default)]
struct Category {
    parent: Option<String>,
    /// Hour → factor, linear between points.
    time: Vec<(f32, f32)>,
}

/// A bed that should be sounding now.
#[derive(Debug, Clone, PartialEq)]
pub struct Bed {
    /// Region and sound: stable while the bed lasts.
    pub key: String,
    pub files: Vec<String>,
    pub volume: f32,
    pub length: Option<(f32, f32)>,
    pub pause: Option<(f32, f32)>,
    pub transition: f32,
    pub category: String,
}

#[derive(Debug, Default)]
pub struct Ambient {
    regions: HashMap<String, Region>,
    categories: HashMap<String, Category>,
}

fn strings(v: Option<&Value>) -> Vec<String> {
    v.and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(|s| s.as_str().map(str::to_lowercase))
                .collect()
        })
        .unwrap_or_default()
}

fn pair(v: Option<&Value>) -> Option<(f32, f32)> {
    let o = v?.as_object()?;
    let get = |k: &str| o.get(k).and_then(Value::as_f64).map(|v| v as f32 / 1000.0);
    Some((get("min")?, get("max").or_else(|| get("min"))?))
}

fn boolean(v: Option<&Value>) -> Option<bool> {
    v.and_then(Value::as_bool)
}

impl Ambient {
    /// Reads the mod's regions and categories from its jar in `dir`.
    pub fn load(dir: &std::path::Path) -> Option<Self> {
        let jar = super::jar::find_jar_in(dir, "AmbientSounds_")?;
        let mut zip = zip::ZipArchive::new(std::fs::File::open(&jar).ok()?).ok()?;
        let names: Vec<String> = zip
            .file_names()
            .filter_map(|n| n.ok().map(|n| n.into_owned()))
            .collect();
        let mut ambient = Self::default();
        for name in names {
            let Some(rest) = name.strip_prefix("assets/ambientsounds/basic/") else {
                continue;
            };
            let kind = rest.split('/').next().unwrap_or("");
            if !(kind == "regions" || kind == "sound_categories") || !name.ends_with(".json") {
                continue;
            }
            let mut text = String::new();
            {
                use std::io::Read;
                let Ok(mut entry) = zip.by_name(&name) else {
                    continue;
                };
                if entry.read_to_string(&mut text).is_err() {
                    continue;
                }
            }
            let Ok(json) = crate::pack::parse_json(&text) else {
                continue;
            };
            let id = rest
                .rsplit('/')
                .next()
                .unwrap_or("")
                .trim_end_matches(".json")
                .to_owned();
            if kind == "regions" {
                ambient.regions.insert(id, region(&json));
            } else {
                ambient.categories.insert(id, category(&json));
            }
        }
        Some(ambient)
    }

    pub fn is_empty(&self) -> bool {
        self.regions.is_empty()
    }

    fn time_factor(&self, category: &str, hour: f32) -> f32 {
        let mut name = Some(category);
        for _ in 0..4 {
            let Some(n) = name else { break };
            let Some(cat) = self.categories.get(n) else {
                return 1.0;
            };
            if !cat.time.is_empty() {
                return curve(&cat.time, hour);
            }
            name = cat.parent.as_deref();
        }
        1.0
    }

    /// How much a region is in force, 0–1, and the volume of the variant
    /// that put it there.
    fn activity(
        &self,
        name: &str,
        sit: &Situation,
        memo: &mut HashMap<String, (f32, f32)>,
        depth: u32,
    ) -> (f32, f32) {
        if let Some(found) = memo.get(name) {
            return *found;
        }
        let Some(region) = self.regions.get(name) else {
            return (0.0, 1.0);
        };
        if depth > 6 {
            return (0.0, 1.0);
        }
        // A cycle reads as inactive while it's being worked out.
        memo.insert(name.to_owned(), (0.0, 1.0));
        let mut a = 1.0;
        if sit.dimension != Dimension::Overworld {
            // The other dimensions have their own regions only.
            let own = match sit.dimension {
                Dimension::Nether => "underworld",
                _ => "space",
            };
            a = f32::from(name == own);
        } else if name == "underworld" || name == "space" {
            a = 0.0;
        }
        let mut apply = |span: Option<Span>, v: f32| {
            if let Some(s) = span {
                a *= s.at(v);
            }
        };
        apply(region.relative_height, sit.relative_height);
        apply(region.absolute_height, sit.y);
        apply(region.sky_light, sit.sky_light);
        apply(region.sky, sit.sky);
        apply(region.underwater, sit.underwater);
        apply(region.temperature, sit.temperature);
        if let Some(r) = region.raining {
            a *= f32::from(r == (sit.raining && sit.temperature >= 0.15));
        }
        if biome_in(&region.bad_biomes, &sit.biome) {
            a = 0.0;
        }
        if !region.biomes.is_empty() && !biome_in(&region.biomes, &sit.biome) {
            a = 0.0;
        }
        for needed in &region.regions {
            a *= self.activity(needed, sit, memo, depth + 1).0;
        }
        for bad in &region.bad_regions {
            a *= 1.0 - self.activity(bad, sit, memo, depth + 1).0;
        }
        let mut volume = 1.0;
        if !region.variants.is_empty() {
            let mut best = (0.0f32, 1.0f32);
            for v in &region.variants {
                let mut va = 1.0;
                for r in &v.regions {
                    va *= self.activity(r, sit, memo, depth + 1).0;
                }
                if !v.biomes.is_empty() && !biome_in(&v.biomes, &sit.biome) {
                    va = 0.0;
                }
                if let Some(want) = v.raining {
                    va *= f32::from(want == (sit.raining && sit.temperature >= 0.15));
                }
                if let Some(want) = v.overall_raining {
                    va *= f32::from(want == sit.raining);
                }
                if let Some(t) = v.temperature {
                    va *= t.at(sit.temperature);
                }
                let nothing_set = v.regions.is_empty()
                    && v.biomes.is_empty()
                    && v.raining.is_none()
                    && v.overall_raining.is_none();
                if nothing_set {
                    va = 0.0;
                }
                if va * v.volume > best.0 * best.1 {
                    best = (va, v.volume);
                }
            }
            a *= best.0;
            volume = best.1;
        }
        let result = (a.clamp(0.0, 1.0), volume);
        memo.insert(name.to_owned(), result);
        result
    }

    /// The beds that should be sounding in `sit`, with their volumes (0 and
    /// negligible ones left out).
    pub fn beds(&self, sit: &Situation) -> Vec<Bed> {
        let mut memo = HashMap::new();
        let mut active: Vec<(&Region, f32, f32)> = Vec::new();
        let mut names: Vec<&String> = self.regions.keys().collect();
        names.sort();
        for name in names {
            let (a, v) = self.activity(name, sit, &mut memo, 0);
            if a > 0.01 {
                active.push((&self.regions[name.as_str()], a, v));
            }
        }
        let mut beds = Vec::new();
        for &(region, a, v) in &active {
            // Regions with a higher mute priority hush the lower ones.
            let hush: f32 = active
                .iter()
                .filter(|(o, ..)| o.mute_priority > region.mute_priority && o.mute > 0.0)
                .map(|(o, oa, _)| o.mute * oa)
                .fold(0.0f32, f32::max);
            for sound in &region.sounds {
                let time = self.time_factor(&sound.category, sit.hour);
                let volume = a * v * sound.volume * time * (1.0 - hush.min(1.0));
                if volume > 0.01 {
                    beds.push(Bed {
                        key: format!("{}/{}", region.name, sound.name),
                        files: sound.files.clone(),
                        volume,
                        length: sound.length,
                        pause: sound.pause,
                        transition: sound.transition,
                        category: sound.category.clone(),
                    });
                }
            }
        }
        beds
    }
}

fn biome_in(list: &[String], biome: &str) -> bool {
    list.iter().any(|entry| {
        let entry = entry
            .trim_start_matches('#')
            .trim_start_matches("is_")
            .replace([' ', '_'], "");
        !entry.is_empty() && biome.contains(&entry)
    })
}

/// A piecewise-linear curve over the 24-hour day, wrapping.
fn curve(points: &[(f32, f32)], hour: f32) -> f32 {
    let mut sorted = points.to_vec();
    sorted.sort_by(|a, b| a.0.total_cmp(&b.0));
    let hour = hour.rem_euclid(24.0);
    let (mut lo, mut hi) = (*sorted.last().unwrap(), sorted[0]);
    lo.0 -= 24.0;
    hi.0 += 24.0;
    let mut before = lo;
    let mut after = sorted[0];
    for p in &sorted {
        if p.0 <= hour {
            before = *p;
        }
    }
    for p in sorted.iter().rev() {
        if p.0 >= hour {
            after = *p;
        }
    }
    if after.0 < before.0 {
        after = hi;
    }
    if (after.0 - before.0).abs() < 1e-6 {
        return before.1;
    }
    before.1 + (after.1 - before.1) * ((hour - before.0) / (after.0 - before.0)).clamp(0.0, 1.0)
}

fn category(json: &Value) -> Category {
    let mut time: Vec<(f32, f32)> = json
        .get("time")
        .and_then(Value::as_object)
        .map(|o| {
            o.iter()
                .filter_map(|(k, v)| Some((k.parse().ok()?, v.as_f64()? as f32)))
                .collect()
        })
        .unwrap_or_default();
    time.sort_by(|a, b| a.0.total_cmp(&b.0));
    Category {
        parent: json
            .get("parent")
            .and_then(Value::as_str)
            .map(str::to_owned),
        time,
    }
}

fn region(json: &Value) -> Region {
    let variants = json
        .get("variants")
        .and_then(Value::as_array)
        .map(|list| {
            list.iter()
                .map(|v| Variant {
                    regions: strings(v.get("regions")),
                    biomes: strings(v.get("biomes")),
                    raining: boolean(v.get("raining")),
                    overall_raining: boolean(v.get("overall-raining")),
                    temperature: Span::of(v.get("temperature")),
                    volume: v.get("volume").and_then(Value::as_f64).unwrap_or(1.0) as f32,
                })
                .collect()
        })
        .unwrap_or_default();
    let sounds = json
        .get("sounds")
        .and_then(Value::as_array)
        .map(|list| {
            list.iter()
                .map(|s| SoundSpec {
                    name: s
                        .get("name")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .to_owned(),
                    files: s
                        .get("files")
                        .and_then(Value::as_array)
                        .map(|f| {
                            f.iter()
                                .filter_map(|x| x.as_str().map(str::to_owned))
                                .collect()
                        })
                        .unwrap_or_default(),
                    category: s
                        .get("category")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .to_owned(),
                    volume: s.get("volume").and_then(Value::as_f64).unwrap_or(1.0) as f32,
                    length: pair(s.get("length")),
                    pause: pair(s.get("pause")),
                    transition: s.get("transition").and_then(Value::as_f64).unwrap_or(400.0) as f32
                        / 1000.0,
                })
                .collect()
        })
        .unwrap_or_default();
    Region {
        name: json
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_owned(),
        regions: strings(json.get("regions")),
        bad_regions: strings(json.get("bad-regions")),
        bad_biomes: strings(json.get("bad-biomes")),
        biomes: strings(json.get("biomes")),
        variants,
        relative_height: Span::of(json.get("relative-height")),
        absolute_height: Span::of(json.get("absolute-height")),
        sky_light: Span::of(json.get("sky-light")),
        sky: Span::of(json.get("sky")),
        underwater: Span::of(json.get("underwater")),
        temperature: Span::of(json.get("temperature")),
        raining: boolean(json.get("raining")),
        mute: json.get("mute").and_then(Value::as_f64).unwrap_or(0.0) as f32,
        mute_priority: json
            .get("mute-priority")
            .and_then(Value::as_i64)
            .unwrap_or(0) as i32,
        sounds,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn real() -> Option<Ambient> {
        let dir = crate::assets::dir().join("downloads/mods");
        let a = Ambient::load(&dir).filter(|a| !a.is_empty());
        if a.is_none() {
            eprintln!("skipped: no Ambient Sounds jar in {}", dir.display());
        }
        a
    }

    fn forest(hour: f32) -> Situation {
        Situation {
            biome: "forest".into(),
            temperature: 0.7,
            hour,
            y: 70.0,
            relative_height: 1.0,
            sky_light: 15.0,
            sky: 1.0,
            ..Situation::default()
        }
    }

    fn has(beds: &[Bed], needle: &str) -> bool {
        beds.iter()
            .any(|b| b.key.contains(needle) && b.volume > 0.05)
    }

    #[test]
    fn a_forest_at_noon_has_birds_and_at_night_owls_and_crickets() {
        let Some(a) = real() else { return };
        let noon = a.beds(&forest(12.0));
        assert!(
            has(&noon, "bird-ambience"),
            "{:?}",
            noon.iter().map(|b| &b.key).collect::<Vec<_>>()
        );
        assert!(!has(&noon, "owl"));
        let night = a.beds(&forest(23.0));
        assert!(
            has(&night, "owl"),
            "{:?}",
            night.iter().map(|b| (&b.key, b.volume)).collect::<Vec<_>>()
        );
        let bird = |beds: &[Bed]| {
            beds.iter()
                .filter(|b| b.key.contains("bird-ambience"))
                .map(|b| b.volume)
                .fold(0.0, f32::max)
        };
        assert!(bird(&noon) > 0.5 && bird(&night) < 0.05);
    }

    #[test]
    fn a_cave_has_cave_sounds_and_no_birds() {
        let Some(a) = real() else { return };
        let cave = Situation {
            biome: "dripstonecaves".into(),
            temperature: 0.8,
            hour: 12.0,
            y: 20.0,
            relative_height: -50.0,
            sky_light: 0.0,
            sky: 0.0,
            ..Situation::default()
        };
        let beds = a.beds(&cave);
        assert!(
            beds.iter()
                .any(|b| b.key.starts_with("cave") && b.volume > 0.05),
            "{:?}",
            beds.iter().map(|b| &b.key).collect::<Vec<_>>()
        );
        assert!(!has(&beds, "bird"));
    }

    #[test]
    fn underwater_hushes_the_rest_and_rain_brings_rain() {
        let Some(a) = real() else { return };
        let wet = Situation {
            underwater: 2.0,
            ..forest(12.0)
        };
        let beds = a.beds(&wet);
        assert!(has(&beds, "underwater"));
        let birds = beds
            .iter()
            .filter(|b| b.key.contains("bird"))
            .map(|b| b.volume)
            .fold(0.0, f32::max);
        assert!(birds < 0.1, "{birds}");
        let rain = a.beds(&Situation {
            raining: true,
            ..forest(12.0)
        });
        assert!(has(&rain, "rain"));
        assert!(!has(&a.beds(&forest(12.0)), "rain/rain"));
    }

    #[test]
    fn time_curves_wrap_around_midnight() {
        let pts = [(0.0, 0.5), (6.0, 1.0), (7.0, 0.0), (18.0, 1.0), (24.0, 0.5)];
        assert!((curve(&pts, 6.0) - 1.0).abs() < 1e-5);
        assert!((curve(&pts, 12.0) - (1.0 - 5.0 / 11.0 * 1.0)).abs() < 0.2);
        assert!((curve(&pts, 23.0) - 0.5 - (1.0 - 0.5) * 0.0).abs() <= 0.5);
        let single = [(6.0, 0.0), (8.0, 1.0)];
        assert!(curve(&single, 7.0) > 0.4 && curve(&single, 7.0) < 0.6);
    }
}
