//! Footsteps by Material (M65), after Presence Footsteps: every foot makes
//! its own sound, from the material of the block it lands on. The block
//! map, the acoustics and the sound files are read from the mod's jar at
//! runtime (`assets/presencefootsteps/config/…`); vanilla's step sounds are
//! muted while it's on.

use std::collections::HashMap;

use serde_json::Value;

/// How many feet a walker has and how they fall in a cycle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Locomotion {
    None,
    Biped,
    Quadruped,
}

impl Locomotion {
    pub fn feet(self) -> u32 {
        match self {
            Locomotion::None => 0,
            Locomotion::Biped => 2,
            Locomotion::Quadruped => 4,
        }
    }
}

/// What kind of step.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum StepEvent {
    /// Strolling or sneaking.
    Wander,
    Walk,
    Run,
    Land,
    Jump,
}

impl StepEvent {
    fn key(self) -> &'static str {
        match self {
            StepEvent::Wander => "wander",
            StepEvent::Walk => "walk",
            StepEvent::Run => "run",
            StepEvent::Land => "land",
            StepEvent::Jump => "jump",
        }
    }

    /// How a walker's pace names its steps: `amount` is the limb speed
    /// (0–1, vanilla's walk animation speed).
    pub fn for_pace(amount: f32, sneaking: bool) -> Self {
        if sneaking || amount < 0.3 {
            StepEvent::Wander
        } else if amount < 0.8 {
            StepEvent::Walk
        } else {
            StepEvent::Run
        }
    }
}

/// One sound to play: an event of the mod's own `sounds.json`.
#[derive(Debug, Clone, PartialEq)]
pub struct StepCue {
    pub event: String,
    pub volume: f32,
    pub pitch: f32,
    /// Ticks to wait before it plays.
    pub delay_ticks: u32,
}

#[derive(Debug, Clone, PartialEq)]
enum Acoustic {
    Basic {
        name: String,
        volume: (f32, f32),
        pitch: (f32, f32),
    },
    Delayed {
        inner: Box<Acoustic>,
        delay_ms: f32,
    },
    Chance {
        inner: Box<Acoustic>,
        chance: f32,
    },
    All(Vec<Acoustic>),
}

fn range(value: Option<&Value>, default: f32) -> (f32, f32) {
    match value {
        Some(Value::Number(n)) => {
            let v = n.as_f64().unwrap_or(f64::from(default)) as f32;
            (v, v)
        }
        Some(Value::Object(o)) => {
            let get = |k: &str| o.get(k).and_then(Value::as_f64).map(|v| v as f32);
            (get("min").unwrap_or(default), get("max").unwrap_or(default))
        }
        _ => (default, default),
    }
}

fn acoustic(value: &Value) -> Option<Acoustic> {
    match value {
        Value::Array(list) => Some(Acoustic::All(list.iter().filter_map(acoustic).collect())),
        Value::Object(o) => match o.get("type").and_then(Value::as_str).unwrap_or("basic") {
            "delayed" => Some(Acoustic::Delayed {
                inner: Box::new(acoustic(o.get("acoustic")?)?),
                delay_ms: o.get("delay").and_then(Value::as_f64).unwrap_or(0.0) as f32,
            }),
            "chance" | "probability" => Some(Acoustic::Chance {
                inner: Box::new(acoustic(o.get("acoustic")?)?),
                chance: o
                    .get("probability")
                    .and_then(Value::as_f64)
                    .unwrap_or(100.0) as f32
                    / 100.0,
            }),
            _ => Some(Acoustic::Basic {
                name: o.get("name")?.as_str()?.to_owned(),
                volume: range(o.get("volume"), 100.0),
                pitch: range(o.get("pitch"), 100.0),
            }),
        },
        _ => None,
    }
}

impl Acoustic {
    fn cues(&self, delay: u32, random: &mut impl FnMut() -> f32, out: &mut Vec<StepCue>) {
        match self {
            Acoustic::Basic {
                name,
                volume,
                pitch,
            } => {
                let pick = |(lo, hi): (f32, f32), r: f32| lo + (hi - lo) * r;
                out.push(StepCue {
                    event: name.clone(),
                    volume: pick(*volume, random()) / 100.0,
                    pitch: pick(*pitch, random()) / 100.0,
                    delay_ticks: delay,
                });
            }
            Acoustic::Delayed { inner, delay_ms } => {
                inner.cues(delay + (delay_ms / 50.0).round() as u32, random, out);
            }
            Acoustic::Chance { inner, chance } => {
                if random() < *chance {
                    inner.cues(delay, random, out);
                }
            }
            Acoustic::All(list) => {
                for a in list {
                    a.cues(delay, random, out);
                }
            }
        }
    }
}

/// How a key in the block map matches a block's name.
#[derive(Debug, Clone, PartialEq)]
enum Matcher {
    Exact(String),
    /// `#minecraft:planks`: a tag. The jar has no tag data here, so a tag
    /// is a block whose name is the tag's name or ends with it
    /// (`oak_planks`, `red_wool`), singular or plural.
    Tag(String),
}

impl Matcher {
    fn matches(&self, name: &str) -> bool {
        match self {
            Matcher::Exact(n) => n == name,
            Matcher::Tag(tag) => {
                let singular = tag.strip_suffix('s').unwrap_or(tag);
                name == tag
                    || name.ends_with(&format!("_{tag}"))
                    || name == singular
                    || name.ends_with(&format!("_{singular}"))
            }
        }
    }
}

/// The data of the mod: which materials each block has, the acoustics of
/// each material, and how each walking mob moves.
#[derive(Debug, Default)]
pub struct Footsteps {
    blocks: Vec<(Matcher, Vec<String>)>,
    /// Vanilla step events (`block.grass.step`) to materials, for blocks
    /// the block map doesn't list.
    primitives: HashMap<String, Vec<String>>,
    materials: HashMap<String, HashMap<String, Acoustic>>,
    locomotion: HashMap<String, Locomotion>,
}

fn words(text: &str) -> Vec<String> {
    text.split(',')
        .map(|w| w.trim().to_owned())
        .filter(|w| !w.is_empty())
        .collect()
}

impl Footsteps {
    /// Reads the mod's files from its jar in `dir`; `None` if it's missing.
    pub fn load(dir: &std::path::Path) -> Option<Self> {
        let jar = super::jar::find_jar_in(dir, "PresenceFootsteps-")?;
        let text = |path: &str| {
            let bytes = super::jar::read_in(&jar, &format!("assets/presencefootsteps/{path}"))?;
            crate::pack::parse_json(&String::from_utf8_lossy(&bytes)).ok()
        };
        let mut data = Self::default();
        if let Some(Value::Object(map)) = text("config/blockmap.json") {
            for (key, value) in &map {
                let Some(list) = value.as_str() else { continue };
                if key.starts_with('*') || key.contains('.') {
                    continue;
                }
                let matcher = match key.strip_prefix('#') {
                    Some(tag) => {
                        Matcher::Tag(tag.strip_prefix("minecraft:").unwrap_or(tag).to_owned())
                    }
                    None => {
                        Matcher::Exact(key.strip_prefix("minecraft:").unwrap_or(key).to_owned())
                    }
                };
                data.blocks.push((matcher, words(list)));
            }
        }
        if let Some(Value::Object(map)) = text("config/primitivemap.json") {
            for (key, value) in &map {
                if let Some(list) = value.as_str() {
                    let key = key.strip_prefix("minecraft:").unwrap_or(key);
                    data.primitives.insert(key.to_owned(), words(list));
                }
            }
        }
        if let Some(Value::Object(map)) = text("config/locomotionmap.json") {
            for (key, value) in &map {
                let kind = match value.as_str() {
                    Some("BIPED") => Locomotion::Biped,
                    Some("QUADRUPED") => Locomotion::Quadruped,
                    _ => Locomotion::None,
                };
                data.locomotion.insert(
                    key.strip_prefix("minecraft:").unwrap_or(key).to_owned(),
                    kind,
                );
            }
        }
        // Every material the maps name has an acoustics file.
        let mut names: Vec<String> = data
            .blocks
            .iter()
            .flat_map(|(_, m)| m.clone())
            .chain(data.primitives.values().flatten().cloned())
            .collect();
        names.sort();
        names.dedup();
        for material in names {
            if let Some(Value::Object(events)) = text(&format!("config/acoustics/{material}.json"))
            {
                let parsed = events
                    .iter()
                    .filter_map(|(k, v)| Some((k.clone(), acoustic(v)?)))
                    .collect();
                data.materials.insert(material, parsed);
            }
        }
        Some(data)
    }

    pub fn is_empty(&self) -> bool {
        self.blocks.is_empty() && self.materials.is_empty()
    }

    /// The materials of a block (`oak_planks`): its entry in the block map,
    /// else the one for its vanilla step sound (`block.wood.step`). Empty
    /// for blocks that make no step sound (air, flowers).
    pub fn materials(&self, block: &str, vanilla_step: &str) -> Vec<String> {
        let found = self
            .blocks
            .iter()
            .find(|(m, _)| matches!(m, Matcher::Exact(_)) && m.matches(block))
            .or_else(|| self.blocks.iter().find(|(m, _)| m.matches(block)));
        let list = match found {
            Some((_, list)) => list.clone(),
            None => self
                .primitives
                .get(vanilla_step)
                .cloned()
                .unwrap_or_default(),
        };
        if list.iter().any(|m| m == "NOT_EMITTER") {
            return Vec::new();
        }
        list
    }

    /// How `mob` (`cow`) moves.
    pub fn locomotion(&self, mob: &str) -> Locomotion {
        self.locomotion
            .get(mob)
            .copied()
            .unwrap_or(Locomotion::None)
    }

    /// The sounds one foot makes on `materials`, layered: every material
    /// that has the event adds its own.
    pub fn cues(
        &self,
        materials: &[String],
        event: StepEvent,
        random: &mut impl FnMut() -> f32,
    ) -> Vec<StepCue> {
        let mut out = Vec::new();
        for material in materials {
            if let Some(a) = self
                .materials
                .get(material)
                .and_then(|m| m.get(event.key()))
            {
                a.cues(0, random, &mut out);
            }
        }
        out
    }
}

/// Counts a walker's footfalls from its walk animation position: a cycle is
/// 2π / 0.6662 units of `limb_swing`, with one footfall per foot in it.
#[derive(Debug, Clone, Copy, Default)]
pub struct Stride {
    last: Option<f32>,
}

/// One walk cycle in limb-swing units.
pub const CYCLE: f32 = std::f32::consts::TAU / 0.6662;

impl Stride {
    /// Footfalls since the last call.
    pub fn steps(&mut self, swing: f32, feet: u32) -> u32 {
        if feet == 0 {
            return 0;
        }
        let unit = CYCLE / feet as f32;
        let now = (swing / unit).floor();
        let before = self.last.replace(now).unwrap_or(now);
        (now - before).clamp(0.0, 3.0) as u32
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn real() -> Option<Footsteps> {
        let dir = crate::assets::dir().join("downloads/mods");
        let data = Footsteps::load(&dir);
        if data.is_none() {
            eprintln!("skipped: no Presence Footsteps jar in {}", dir.display());
        }
        data
    }

    #[test]
    fn the_material_map_loads_from_the_jar_and_maps_the_common_blocks() {
        let Some(data) = real() else { return };
        assert!(!data.is_empty());
        let has = |block: &str, step: &str, wanted: &str| {
            let m = data.materials(block, step);
            assert!(m.iter().any(|x| x == wanted), "{block}: {m:?}");
        };
        has("grass_block", "block.grass.step", "grass");
        has("stone", "block.stone.step", "stone");
        has("oak_planks", "block.wood.step", "wood");
        has("sand", "block.sand.step", "sand");
        has("white_wool", "block.wool.step", "rug");
        assert!(data.materials("air", "").is_empty());
        // Mobs: a cow has four feet, a zombie two, a bat none.
        assert_eq!(data.locomotion("cow"), Locomotion::Quadruped);
        assert_eq!(data.locomotion("zombie"), Locomotion::Biped);
        assert_eq!(data.locomotion("bat"), Locomotion::None);
    }

    #[test]
    fn a_step_on_grass_layers_its_sounds_and_a_run_is_louder_than_a_wander() {
        let Some(data) = real() else { return };
        let mut r = 0.5f32;
        let mut random = move || {
            r = (r * 7.31 + 0.37).fract();
            r
        };
        let grass = data.materials("grass_block", "block.grass.step");
        let cues = data.cues(&grass, StepEvent::Walk, &mut random);
        assert!(!cues.is_empty());
        assert!(cues.iter().all(|c| c.event.starts_with("pf_presence.")));
        assert!(
            cues.iter()
                .all(|c| (0.0..=1.2).contains(&c.volume) && c.pitch > 0.5)
        );
        // Stone is a layer of its own.
        let stone = data.materials("stone", "block.stone.step");
        assert!(!data.cues(&stone, StepEvent::Land, &mut random).is_empty());
    }

    #[test]
    fn pace_names_the_step() {
        assert_eq!(StepEvent::for_pace(0.1, false), StepEvent::Wander);
        assert_eq!(StepEvent::for_pace(0.9, true), StepEvent::Wander);
        assert_eq!(StepEvent::for_pace(0.5, false), StepEvent::Walk);
        assert_eq!(StepEvent::for_pace(1.0, false), StepEvent::Run);
    }

    #[test]
    fn a_quadruped_makes_four_footfalls_a_walk_cycle_and_a_biped_two() {
        for (feet, expected) in [(4, 4), (2, 2)] {
            let mut stride = Stride::default();
            stride.steps(0.001, feet);
            let mut total = 0;
            // One cycle in small steps of the walk animation.
            let mut swing = 0.001;
            while swing < CYCLE + 0.001 {
                swing += 0.3;
                total += stride.steps(swing, feet);
            }
            assert_eq!(total, expected, "{feet} feet");
        }
        assert_eq!(Stride::default().steps(50.0, 0), 0);
    }

    #[test]
    fn delayed_and_chance_acoustics_parse() {
        let json: Value = serde_json::from_str(
            r#"[{"name":"a","volume":{"min":50,"max":100},"pitch":100,"type":"basic"},
               {"acoustic":{"name":"b","volume":80,"pitch":100,"type":"basic"},"delay":100,"type":"delayed"},
               {"acoustic":{"name":"c","volume":80,"pitch":100,"type":"basic"},"probability":0,"type":"chance"}]"#,
        )
        .unwrap();
        let a = acoustic(&json).unwrap();
        let mut out = Vec::new();
        a.cues(0, &mut || 0.5, &mut out);
        assert_eq!(out.len(), 2, "the zero-chance one never plays");
        assert_eq!((out[0].event.as_str(), out[0].volume), ("a", 0.75));
        assert_eq!((out[1].event.as_str(), out[1].delay_ticks), ("b", 2));
    }
}
