//! Sound: every sound the game plays is a named event (`block.stone.break`,
//! `entity.zombie.ambient`) looked up in the merged sound table
//! (`table::SoundTable`): resource packs, then Kenzo's `assets/sounds`, then
//! the vanilla fallback in `assets/vanilla`. Game code calls `play`, which
//! only queues a cue; `Audio::update` (once a frame, in the app) turns the
//! cues into sounds on the speakers with kira.

pub mod events;
pub mod table;

use std::{borrow::Cow, cell::RefCell, collections::HashMap, io::Cursor, time::Duration};

use glam::DVec3;
use kira::{
    AudioManager, AudioManagerSettings, DefaultBackend, Tween,
    sound::{
        PlaybackState,
        static_sound::StaticSoundData,
        streaming::{StreamingSoundData, StreamingSoundHandle},
    },
};
use log::{debug, info, warn};

use crate::pack::Resources;
pub use table::SoundTable;
use table::{Entry, SoundTable as Table};

/// One sound asked for: an event, where and how loud.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Cue {
    pub event: Cow<'static, str>,
    /// Where it is; `None` plays it at the listener (the player's own
    /// sounds and the interface), with no falloff.
    pub at: Option<DVec3>,
    pub volume: f32,
    pub pitch: f32,
}

/// Music & Sounds' categories (vanilla's `SoundSource`), each with its
/// slider under Master Volume.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Category {
    Master,
    Music,
    Records,
    Weather,
    Blocks,
    Hostile,
    Neutral,
    Players,
    Ambient,
    Voice,
}

/// Mobs whose sounds are Hostile Creatures'; every other mob's are
/// Friendly Creatures'.
const HOSTILE: &[&str] = &[
    "blaze",
    "bogged",
    "breeze",
    "cave_spider",
    "creaking",
    "creeper",
    "drowned",
    "elder_guardian",
    "ender_dragon",
    "enderman",
    "endermite",
    "evoker",
    "ghast",
    "guardian",
    "hoglin",
    "husk",
    "illusioner",
    "magma_cube",
    "phantom",
    "piglin",
    "piglin_brute",
    "pillager",
    "ravager",
    "shulker",
    "shulker_bullet",
    "silverfish",
    "skeleton",
    "slime",
    "spider",
    "stray",
    "vex",
    "vindicator",
    "warden",
    "witch",
    "wither",
    "wither_skeleton",
    "zoglin",
    "zombie",
    "zombie_villager",
    "zombified_piglin",
];

/// The category an event's sound plays in, from its name the way vanilla's
/// code files them: blocks, mobs by temper, the player's own and items',
/// music, records and note blocks, weather and the surroundings.
pub fn category(event: &str) -> Category {
    let mut parts = event.split('.');
    match (parts.next(), parts.next()) {
        (Some("music"), _) => Category::Music,
        (Some("music_disc"), _) | (Some("block"), Some("note_block")) => Category::Records,
        (Some("weather"), _) => Category::Weather,
        (Some("ambient"), _) => Category::Ambient,
        (Some("block"), _) => Category::Blocks,
        (Some("item"), _) => Category::Players,
        (Some("entity"), Some("player" | "item" | "experience_orb")) => Category::Players,
        (Some("entity"), Some("generic" | "tnt")) => Category::Blocks,
        (Some("entity"), Some(mob)) if HOSTILE.contains(&mob) => Category::Hostile,
        (Some("entity"), _) => Category::Neutral,
        _ => Category::Master,
    }
}

impl crate::options::Volumes {
    /// The slider for `category` (Master's is applied to everything
    /// separately).
    pub fn of(&self, category: Category) -> f32 {
        match category {
            Category::Master => 1.0,
            Category::Music => self.music,
            Category::Records => self.records,
            Category::Weather => self.weather,
            Category::Blocks => self.blocks,
            Category::Hostile => self.hostile,
            Category::Neutral => self.neutral,
            Category::Players => self.players,
            Category::Ambient => self.ambient,
            Category::Voice => self.voice,
        }
    }
}

/// Cues waiting for the next `Audio::update`; no more than this are kept.
const MAX_QUEUED: usize = 256;

thread_local! {
    static QUEUE: RefCell<Vec<Cue>> = const { RefCell::new(Vec::new()) };
}

fn queue(cue: Cue) {
    QUEUE.with(|queue| {
        let mut queue = queue.borrow_mut();
        if queue.len() >= MAX_QUEUED {
            queue.remove(0);
        }
        queue.push(cue);
    });
}

/// Plays `event` at `at` in the world.
pub fn play(event: impl Into<Cow<'static, str>>, at: DVec3) {
    play_at(event, at, 1.0, 1.0);
}

/// Plays `event` at `at`, `volume` times as loud and `pitch` times as high.
pub fn play_at(event: impl Into<Cow<'static, str>>, at: DVec3, volume: f32, pitch: f32) {
    queue(Cue {
        event: event.into(),
        at: Some(at),
        volume,
        pitch,
    });
}

/// Plays `event` at the listener: the player's own sounds, the interface.
pub fn play_here(event: impl Into<Cow<'static, str>>) {
    play_here_with(event, 1.0, 1.0);
}

pub fn play_here_with(event: impl Into<Cow<'static, str>>, volume: f32, pitch: f32) {
    queue(Cue {
        event: event.into(),
        at: None,
        volume,
        pitch,
    });
}

/// Puts a cue back at the end of the queue.
pub fn requeue(cue: Cue) {
    queue(cue);
}

/// Takes the cues queued so far on this thread.
pub fn drain() -> Vec<Cue> {
    QUEUE.with(|queue| std::mem::take(&mut *queue.borrow_mut()))
}

/// Where the listener is and which way is right.
#[derive(Debug, Clone, Copy)]
pub struct Listener {
    pub position: DVec3,
    /// A unit vector to the listener's right, flat on the ground.
    pub right: DVec3,
}

/// How a cue sounds to a listener.
#[derive(Debug, Clone, PartialEq)]
pub struct Playback {
    pub entry: Entry,
    /// Loudness as an amplitude, 0 to about 2.
    pub gain: f32,
    /// -1 (left) to 1 (right).
    pub pan: f32,
    pub rate: f64,
}

/// Decibels for an amplitude.
fn decibels(gain: f32) -> f32 {
    20.0 * gain.max(1e-4).log10()
}

/// Works out how `cue` sounds from `listener`: the entry picked with `r`
/// (0 to 1), and its loudness (linear falloff to nothing at its range, as in
/// Minecraft), pan and speed. `None` if the event is unknown or out of range.
pub fn plan(
    table: &Table,
    cue: &Cue,
    listener: &Listener,
    r: f32,
    master: f32,
) -> Option<Playback> {
    let entry = table.pick(&cue.event, r)?;
    let (falloff, pan) = match cue.at {
        None => (1.0, 0.0),
        Some(at) => {
            let offset = at - listener.position;
            let range = f64::from(entry.range * cue.volume.max(1.0));
            let distance = offset.length();
            if distance >= range {
                return None;
            }
            let pan = if distance > 0.5 {
                (offset / distance).dot(listener.right) * 0.8
            } else {
                0.0
            };
            (1.0 - distance / range, pan)
        }
    };
    let gain = entry.volume * cue.volume * falloff as f32 * master;
    (gain > 0.001).then(|| Playback {
        rate: f64::from((entry.pitch * cue.pitch).clamp(0.5, 2.0)),
        entry,
        gain,
        pan: pan as f32,
    })
}

/// A tiny xorshift for picking sounds.
struct Random(u64);

impl Random {
    fn next(&mut self) -> f32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 >> 40) as f32 / (1u64 << 24) as f32
    }
}

/// The sound engine: the merged table, decoded sounds, and the speakers.
pub struct Audio {
    manager: Option<AudioManager<DefaultBackend>>,
    table: Table,
    /// Decoded sound effects by file name (`None` if the file is missing).
    cache: HashMap<String, Option<StaticSoundData>>,
    random: Random,
    /// The track playing, its event and its own volume.
    music: Option<(
        String,
        StreamingSoundHandle<kira::sound::FromFileError>,
        f32,
    )>,
    /// Seconds until the next track may start.
    music_wait: f32,
    /// Files already reported missing.
    reported: std::collections::HashSet<String>,
}

impl Audio {
    /// The engine for `res`'s sounds. Without a sound device it still takes
    /// cues, and plays nothing.
    pub fn new(res: &Resources) -> Self {
        let manager = match AudioManager::<DefaultBackend>::new(AudioManagerSettings::default()) {
            Ok(manager) => Some(manager),
            Err(err) => {
                warn!("no sound: {err}");
                None
            }
        };
        let mut audio = Self {
            manager,
            table: Table::default(),
            cache: HashMap::new(),
            random: Random(0x9E37_79B9_7F4A_7C15),
            music: None,
            music_wait: 5.0,
            reported: Default::default(),
        };
        audio.reload(res);
        audio
    }

    /// Rebuilds the table from the resources, after the packs change.
    pub fn reload(&mut self, res: &Resources) {
        self.table = build_table(res);
        self.cache.clear();
        info!("sound: {} events", self.table.len());
    }

    pub fn table(&self) -> &Table {
        &self.table
    }

    /// Plays the queued cues for a listener. `master` is the Master Volume
    /// setting and `volumes` the categories' sliders.
    pub fn update(
        &mut self,
        res: &Resources,
        listener: &Listener,
        master: f32,
        volumes: &crate::options::Volumes,
    ) {
        let cues = drain();
        if let Some(manager) = &mut self.manager {
            manager
                .main_track()
                .set_volume(decibels(master), Tween::default());
        }
        if self.manager.is_none() {
            return;
        }
        // The queue is bounded, but a burst (an explosion) doesn't need all of it.
        for cue in cues.iter().rev().take(24) {
            let r = self.random.next();
            let volume = volumes.of(category(&cue.event));
            let Some(playback) = plan(&self.table, cue, listener, r, volume) else {
                continue;
            };
            self.play(res, playback);
        }
    }

    fn sound(&mut self, res: &Resources, name: &str) -> Option<StaticSoundData> {
        if !self.cache.contains_key(name) {
            let data = res.bytes(&format!("sounds/{name}.ogg")).and_then(|bytes| {
                match StaticSoundData::from_cursor(Cursor::new(bytes)) {
                    Ok(data) => Some(data),
                    Err(err) => {
                        warn!("can't decode sounds/{name}.ogg: {err}");
                        None
                    }
                }
            });
            if data.is_none() && self.reported.insert(name.to_owned()) {
                debug!("no sound file sounds/{name}.ogg");
            }
            self.cache.insert(name.to_owned(), data);
        }
        self.cache[name].clone()
    }

    fn play(&mut self, res: &Resources, playback: Playback) {
        if playback.entry.stream {
            return;
        }
        let Some(data) = self.sound(res, &playback.entry.name) else {
            return;
        };
        let data = data
            .volume(decibels(playback.gain))
            .panning(playback.pan)
            .playback_rate(playback.rate);
        if let Some(manager) = &mut self.manager
            && let Err(err) = manager.play(data)
        {
            debug!("can't play {}: {err}", playback.entry.name);
        }
    }

    /// Keeps the music going: `request` says which event's tracks fit
    /// where the player is (or `None` for silence); a track plays through,
    /// then there is a pause, and a new place fades the old track out.
    /// `volume` is the Music slider (Master is on the main track).
    pub fn update_music(&mut self, res: &Resources, request: Option<&str>, dt: f32, volume: f32) {
        if self.manager.is_none() {
            return;
        }
        let playing = self
            .music
            .as_ref()
            .is_some_and(|(_, handle, _)| handle.state() == PlaybackState::Playing);
        if let Some((event, handle, own)) = &mut self.music {
            handle.set_volume(decibels(*own * volume * 0.7), Tween::default());
            let changed = request != Some(event.as_str());
            if changed && playing {
                handle.stop(Tween {
                    duration: Duration::from_secs(2),
                    ..Tween::default()
                });
            }
            if !playing {
                self.music = None;
            }
        }
        if self.music.is_some() {
            return;
        }
        let Some(event) = request else {
            return;
        };
        self.music_wait -= dt;
        if self.music_wait > 0.0 {
            return;
        }
        let Some(entry) = self.table.pick(event, self.random.next()) else {
            return;
        };
        let Some(bytes) = res.bytes(&format!("sounds/{}.ogg", entry.name)) else {
            self.music_wait = 30.0;
            return;
        };
        let data = match StreamingSoundData::from_cursor(Cursor::new(bytes)) {
            Ok(data) => data.volume(decibels(entry.volume * volume * 0.7)),
            Err(err) => {
                warn!("can't stream {}: {err}", entry.name);
                self.music_wait = 30.0;
                return;
            }
        };
        if let Some(manager) = &mut self.manager {
            match manager.play(data) {
                Ok(handle) => self.music = Some((event.to_owned(), handle, entry.volume)),
                Err(err) => debug!("can't play {}: {err}", entry.name),
            }
        }
        // Between tracks: half a minute to three, as Minecraft's menu and game do.
        self.music_wait = 30.0 + self.random.next() * 150.0;
    }
}

/// The merged table for `res`: vanilla's `sounds.json`, then each layer's
/// above it.
pub fn build_table(res: &Resources) -> Table {
    let layers: Vec<String> = res
        .all_bytes("sounds.json")
        .iter()
        .map(|bytes| String::from_utf8_lossy(bytes).into_owned())
        .collect();
    Table::from_layers(layers.iter().map(String::as_str))
}

/// What the merged table of `res` can actually play.
#[derive(Debug, Default)]
pub struct Report {
    /// Events in the table.
    pub events: usize,
    /// Events none of whose files exist in any layer (silent).
    pub silent: Vec<String>,
    /// Files the table names that no layer has.
    pub missing_files: Vec<String>,
}

/// Checks every event of `res`'s table against the files of its layers.
/// Events that are only references to other events, and ones with no
/// entries, are looked through.
pub fn report(res: &Resources) -> Report {
    let table = build_table(res);
    let mut report = Report {
        events: table.len(),
        ..Report::default()
    };
    let has = |entry: &Entry| {
        res.layer_of(&format!("sounds/{}.ogg", entry.name))
            .is_some()
    };
    for event in table.events() {
        let entries = table.entries(event);
        let playable = entries.iter().any(|entry| {
            if entry.is_event {
                table.contains(&entry.name)
            } else {
                has(entry)
            }
        });
        for entry in entries.iter().filter(|e| !e.is_event && !has(e)) {
            report.missing_files.push(entry.name.clone());
        }
        // Events with no entries at all (a biome without tracks) are empty
        // on purpose.
        if !entries.is_empty() && !playable {
            report.silent.push(event.to_owned());
        }
    }
    report.missing_files.sort();
    report.missing_files.dedup();
    report
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn events_fall_in_vanillas_sound_categories() {
        assert_eq!(category("block.stone.break"), Category::Blocks);
        assert_eq!(category("block.note_block.harp"), Category::Records);
        assert_eq!(category("music.game"), Category::Music);
        assert_eq!(category("entity.zombie.ambient"), Category::Hostile);
        assert_eq!(category("entity.pig.ambient"), Category::Neutral);
        assert_eq!(category("entity.player.hurt"), Category::Players);
        assert_eq!(category("entity.item.pickup"), Category::Players);
        assert_eq!(category("item.armor.equip_iron"), Category::Players);
        assert_eq!(category("entity.generic.explode"), Category::Blocks);
        assert_eq!(category("ambient.cave"), Category::Ambient);
        assert_eq!(category("ui.button.click"), Category::Master);
        let volumes = crate::options::Volumes {
            hostile: 0.25,
            ..Default::default()
        };
        assert_eq!(volumes.of(category("entity.creeper.primed")), 0.25);
        assert_eq!(volumes.of(Category::Master), 1.0);
    }

    const JSON: &str = r#"{"block.stone.break": {"sounds": [{"name": "dig/stone1", "volume": 0.5, "pitch": 0.8}]}}"#;

    fn listener() -> Listener {
        Listener {
            position: DVec3::ZERO,
            right: DVec3::X,
        }
    }

    fn cue(at: Option<DVec3>, volume: f32) -> Cue {
        Cue {
            event: "block.stone.break".into(),
            at,
            volume,
            pitch: 1.0,
        }
    }

    #[test]
    fn cues_queue_and_drain_in_order() {
        drain();
        play("block.stone.break", DVec3::new(1.0, 2.0, 3.0));
        play_here_with("ui.button.click", 0.5, 2.0);
        let cues = drain();
        assert_eq!(cues.len(), 2);
        assert_eq!(cues[0].at, Some(DVec3::new(1.0, 2.0, 3.0)));
        assert_eq!(cues[1].event, "ui.button.click");
        assert_eq!(
            (cues[1].volume, cues[1].pitch, cues[1].at),
            (0.5, 2.0, None)
        );
        assert!(drain().is_empty());
    }

    #[test]
    fn the_queue_keeps_the_newest_cues() {
        drain();
        for i in 0..(MAX_QUEUED + 10) {
            play_at("x", DVec3::new(i as f64, 0.0, 0.0), 1.0, 1.0);
        }
        let cues = drain();
        assert_eq!(cues.len(), MAX_QUEUED);
        assert_eq!(cues.last().unwrap().at.unwrap().x, (MAX_QUEUED + 9) as f64);
    }

    #[test]
    fn sounds_fade_linearly_to_nothing_at_sixteen_blocks_and_pan_to_their_side() {
        let table = Table::from_layers([JSON]);
        let near = plan(
            &table,
            &cue(Some(DVec3::new(0.0, 0.0, 0.0)), 1.0),
            &listener(),
            0.0,
            1.0,
        )
        .unwrap();
        assert_eq!((near.gain, near.pan), (0.5, 0.0));
        assert!((near.rate - 0.8).abs() < 1e-6);
        let half = plan(
            &table,
            &cue(Some(DVec3::new(8.0, 0.0, 0.0)), 1.0),
            &listener(),
            0.0,
            1.0,
        )
        .unwrap();
        assert!((half.gain - 0.25).abs() < 1e-6, "{}", half.gain);
        assert!(half.pan > 0.7, "to the right");
        let left = plan(
            &table,
            &cue(Some(DVec3::new(-8.0, 0.0, 0.0)), 1.0),
            &listener(),
            0.0,
            1.0,
        )
        .unwrap();
        assert!(left.pan < -0.7);
        assert!(
            plan(
                &table,
                &cue(Some(DVec3::new(16.0, 0.0, 0.0)), 1.0),
                &listener(),
                0.0,
                1.0
            )
            .is_none()
        );
        // Louder sounds carry further; the listener's own are never faded.
        assert!(
            plan(
                &table,
                &cue(Some(DVec3::new(20.0, 0.0, 0.0)), 2.0),
                &listener(),
                0.0,
                1.0
            )
            .is_some()
        );
        assert_eq!(
            plan(&table, &cue(None, 1.0), &listener(), 0.0, 1.0)
                .unwrap()
                .gain,
            0.5
        );
        // The volume setting scales it, and zero silences it.
        assert_eq!(
            plan(&table, &cue(None, 1.0), &listener(), 0.0, 0.5)
                .unwrap()
                .gain,
            0.25
        );
        assert!(plan(&table, &cue(None, 1.0), &listener(), 0.0, 0.0).is_none());
        // Unknown events are dropped.
        let unknown = Cue {
            event: "no.such".into(),
            ..cue(None, 1.0)
        };
        assert!(plan(&table, &unknown, &listener(), 0.0, 1.0).is_none());
    }

    #[test]
    fn the_vanilla_sounds_decode_and_every_event_has_a_file() {
        let assets = crate::assets::dir();
        if !assets.join("vanilla/assets/minecraft/sounds").is_dir() {
            return;
        }
        let res = Resources::load(&assets, Vec::new()).unwrap();
        let report = report(&res);
        assert!(report.events > 1500, "{} events", report.events);
        assert!(
            report.silent.is_empty(),
            "silent events: {:?}",
            report.silent
        );
        // One real file decodes with ogg support.
        let table = build_table(&res);
        let entry = table.pick("block.stone.break", 0.0).unwrap();
        let bytes = res.bytes(&format!("sounds/{}.ogg", entry.name)).unwrap();
        let data = StaticSoundData::from_cursor(Cursor::new(bytes)).unwrap();
        assert!(data.num_frames() > 1000);
    }

    #[test]
    fn decibels_follow_amplitude() {
        assert!((decibels(1.0)).abs() < 1e-6);
        assert!((decibels(0.5) + 6.0206).abs() < 1e-3);
        assert!(decibels(0.0) < -60.0);
    }
}
