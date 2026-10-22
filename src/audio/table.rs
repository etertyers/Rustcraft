//! The sound table: every sound event (`block.stone.break`) and the files
//! it can play, merged from `sounds.json` in each resource layer.
//!
//! Layers are merged from the bottom (the vanilla fallback) up through
//! Kenzo's assets and the selected packs. An event a higher layer defines
//! replaces the lower layers' version, unless that layer says
//! `"replace": false`, which adds its sounds to theirs (Minecraft's own
//! rule for packs). So Kenzo's sounds win wherever he has them and vanilla
//! fills every event he lacks.

use std::collections::BTreeMap;

use serde_json::Value;

/// How far a sound carries, in blocks, when it doesn't say.
pub const DEFAULT_RANGE: f32 = 16.0;

/// One thing an event may play.
#[derive(Debug, Clone, PartialEq)]
pub struct Entry {
    /// A file under `sounds/` without its extension (`block/stone/break1`),
    /// or another event's name if `is_event`.
    pub name: String,
    pub volume: f32,
    pub pitch: f32,
    /// How likely it is to be picked, against the event's other entries.
    pub weight: u32,
    /// Played from the file as it decodes (music, long loops).
    pub stream: bool,
    /// `name` is another event, whose entries are picked instead.
    pub is_event: bool,
    /// Blocks it can be heard from.
    pub range: f32,
}

/// Every event's entries.
#[derive(Debug, Default, Clone)]
pub struct SoundTable {
    events: BTreeMap<String, Vec<Entry>>,
}

/// `minecraft:block/stone/break1` → `block/stone/break1`.
fn plain(name: &str) -> &str {
    name.strip_prefix("minecraft:").unwrap_or(name)
}

fn entry(value: &Value) -> Option<Entry> {
    let object = match value {
        Value::String(name) => {
            return Some(Entry {
                name: plain(name).to_owned(),
                volume: 1.0,
                pitch: 1.0,
                weight: 1,
                stream: false,
                is_event: false,
                range: DEFAULT_RANGE,
            });
        }
        Value::Object(object) => object,
        _ => return None,
    };
    let number =
        |key: &str, default: f64| object.get(key).and_then(Value::as_f64).unwrap_or(default);
    Some(Entry {
        name: plain(object.get("name")?.as_str()?).to_owned(),
        volume: number("volume", 1.0) as f32,
        pitch: number("pitch", 1.0) as f32,
        weight: number("weight", 1.0).max(1.0) as u32,
        stream: object
            .get("stream")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        is_event: object.get("type").and_then(Value::as_str) == Some("event"),
        range: number("attenuation_distance", f64::from(DEFAULT_RANGE)) as f32,
    })
}

impl SoundTable {
    /// The table for `layers` of `sounds.json` text, lowest layer first.
    /// Layers that don't parse are skipped.
    pub fn from_layers<'a>(layers: impl IntoIterator<Item = &'a str>) -> Self {
        let mut table = Self::default();
        for text in layers {
            table.add_layer(text);
        }
        table
    }

    /// Merges one layer on top of what's there. Returns whether it parsed.
    pub fn add_layer(&mut self, text: &str) -> bool {
        let Ok(Value::Object(events)) = crate::pack::parse_json(text) else {
            return false;
        };
        for (name, event) in events {
            let sounds: Vec<Entry> = event
                .get("sounds")
                .and_then(Value::as_array)
                .map(|list| list.iter().filter_map(entry).collect())
                .unwrap_or_default();
            let adds = event.get("replace").and_then(Value::as_bool) == Some(false);
            let name = plain(&name).to_owned();
            match self.events.get_mut(&name) {
                Some(have) if adds => have.extend(sounds),
                _ => {
                    self.events.insert(name, sounds);
                }
            }
        }
        true
    }

    pub fn contains(&self, event: &str) -> bool {
        self.events.get(plain(event)).is_some_and(|e| !e.is_empty())
    }

    pub fn entries(&self, event: &str) -> &[Entry] {
        self.events.get(plain(event)).map_or(&[], Vec::as_slice)
    }

    pub fn len(&self) -> usize {
        self.events.len()
    }

    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }

    /// Every event name.
    pub fn events(&self) -> impl Iterator<Item = &str> {
        self.events.keys().map(String::as_str)
    }

    /// One entry of `event` picked by weight, `r` in 0..1 choosing among
    /// them; entries that are other events are followed. `None` if the
    /// event is unknown or empty.
    pub fn pick(&self, event: &str, r: f32) -> Option<Entry> {
        let mut event = event;
        let mut r = r;
        for _ in 0..4 {
            let entries = self.events.get(plain(event))?;
            let total: u32 = entries.iter().map(|e| e.weight).sum();
            if total == 0 {
                return None;
            }
            let mut left = ((r.clamp(0.0, 0.999_999) * total as f32) as u32).min(total - 1);
            let chosen = entries.iter().find(|e| {
                if left < e.weight {
                    true
                } else {
                    left -= e.weight;
                    false
                }
            })?;
            if !chosen.is_event {
                return Some(chosen.clone());
            }
            // Another event: pick from it with the rest of the chance.
            event = &chosen.name;
            r = (r * 7.31).fract();
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const VANILLA: &str = r#"{
        "block.stone.break": {"sounds": [
            {"name": "dig/stone1", "volume": 0.8},
            {"name": "minecraft:dig/stone2", "weight": 3, "pitch": 0.5}]},
        "entity.zombie.ambient": {"sounds": ["mob/zombie/say1"]},
        "block.stone.step": {"sounds": ["step/stone1"]},
        "entity.cow.say": {"sounds": [{"name": "block.stone.step", "type": "event"}]}
    }"#;

    #[test]
    fn reads_strings_and_objects_with_their_defaults() {
        let table = SoundTable::from_layers([VANILLA]);
        assert_eq!(table.len(), 4);
        let stone = table.entries("block.stone.break");
        assert_eq!(stone.len(), 2);
        assert_eq!((stone[0].volume, stone[0].weight), (0.8, 1));
        assert_eq!(stone[1].name, "dig/stone2");
        assert_eq!((stone[1].pitch, stone[1].weight), (0.5, 3));
        let zombie = &table.entries("minecraft:entity.zombie.ambient")[0];
        assert_eq!(
            (zombie.volume, zombie.pitch, zombie.range),
            (1.0, 1.0, 16.0)
        );
        assert!(table.contains("block.stone.step"));
        assert!(!table.contains("block.glass.break"));
    }

    #[test]
    fn a_higher_layer_replaces_an_event_unless_it_says_replace_false() {
        let kenzo = r#"{
            "block.stone.break": {"sounds": ["kenzo/stone"]},
            "block.stone.step": {"replace": false, "sounds": ["kenzo/step"]},
            "entity.pig.say": {"sounds": ["kenzo/oink"]}
        }"#;
        let table = SoundTable::from_layers([VANILLA, kenzo]);
        // Replaced: only Kenzo's.
        let names: Vec<_> = table
            .entries("block.stone.break")
            .iter()
            .map(|e| e.name.as_str())
            .collect();
        assert_eq!(names, ["kenzo/stone"]);
        // Added to.
        let names: Vec<_> = table
            .entries("block.stone.step")
            .iter()
            .map(|e| e.name.as_str())
            .collect();
        assert_eq!(names, ["step/stone1", "kenzo/step"]);
        // New in the higher layer, and untouched in the lower.
        assert!(table.contains("entity.pig.say"));
        assert_eq!(
            table.entries("entity.zombie.ambient")[0].name,
            "mob/zombie/say1"
        );
    }

    #[test]
    fn picks_by_weight_and_follows_event_references() {
        let table = SoundTable::from_layers([VANILLA]);
        // Weights 1 and 3: the first quarter of the range is stone1.
        assert_eq!(
            table.pick("block.stone.break", 0.0).unwrap().name,
            "dig/stone1"
        );
        assert_eq!(
            table.pick("block.stone.break", 0.24).unwrap().name,
            "dig/stone1"
        );
        assert_eq!(
            table.pick("block.stone.break", 0.26).unwrap().name,
            "dig/stone2"
        );
        assert_eq!(
            table.pick("block.stone.break", 1.0).unwrap().name,
            "dig/stone2"
        );
        // A reference plays the other event's file.
        assert_eq!(
            table.pick("entity.cow.say", 0.5).unwrap().name,
            "step/stone1"
        );
        assert!(table.pick("nothing.here", 0.5).is_none());
    }

    #[test]
    fn broken_layers_are_skipped() {
        let mut table = SoundTable::from_layers([VANILLA]);
        assert!(!table.add_layer("not json"));
        assert_eq!(table.len(), 4);
    }
}
