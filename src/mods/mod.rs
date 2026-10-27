//! Mods (M39): folders or `.zip`s in `mods/` with a `mod.ron`, data files
//! in the same RON formats as `assets/data/`, a resource-pack layer in
//! `assets/minecraft/` and a `main.lua`. The set of enabled mods is chosen
//! once, when the game starts (`init`), because blocks and items get their
//! runtime IDs from it: the built-in ones first, then each mod's in load
//! order. Changes in the Mods screen apply the next time the game starts.

pub mod client;
pub mod lua;

use std::{
    collections::HashMap,
    fs::File,
    io::{BufReader, Read},
    path::{Path, PathBuf},
    sync::{Mutex, OnceLock},
};

use anyhow::{Context, Result, bail};
use log::{info, warn};
use serde::{Deserialize, Serialize};

/// The game's version, compared with a mod's `rustcraft` field.
pub const GAME_VERSION: &str = env!("CARGO_PKG_VERSION");

/// The blocks and items of one mod, in the order IDs are given out.
const MOD_ID_CHARS: &str = "abcdefghijklmnopqrstuvwxyz0123456789_";

/// What a mod's `mod.ron` says.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    pub id: String,
    pub name: String,
    pub version: String,
    #[serde(default)]
    pub authors: Vec<String>,
    #[serde(default)]
    pub description: String,
    /// The oldest game version it works with (`"1.2"`).
    #[serde(default)]
    pub rustcraft: String,
    /// Ids of the mods it needs, loaded before it.
    #[serde(default)]
    pub dependencies: Vec<String>,
    /// Only draws and listens on the player's own computer (Fullbright):
    /// a LAN host and its guests needn't have the same copy.
    #[serde(default)]
    pub client_only: bool,
}

/// Where a mod's files are.
#[derive(Debug, Clone, PartialEq)]
pub enum Source {
    Dir(PathBuf),
    /// A `.zip`, with the folder inside it holding `mod.ron` (or empty).
    Zip {
        path: PathBuf,
        prefix: String,
    },
}

impl Source {
    /// A file inside the mod, `None` if it isn't there.
    pub fn read(&self, rel: &str) -> Option<Vec<u8>> {
        match self {
            Source::Dir(dir) => std::fs::read(dir.join(rel)).ok(),
            Source::Zip { path, prefix } => {
                let file = File::open(path).ok()?;
                let mut archive = zip::ZipArchive::new(BufReader::new(file)).ok()?;
                let mut entry = archive.by_name(&format!("{prefix}{rel}")).ok()?;
                let mut bytes = Vec::new();
                entry.read_to_end(&mut bytes).ok()?;
                Some(bytes)
            }
        }
    }

    /// Whether the mod has a resource layer (`assets/minecraft/`).
    pub fn has_assets(&self) -> bool {
        match self {
            Source::Dir(dir) => dir.join("assets/minecraft").is_dir(),
            Source::Zip { path, prefix } => File::open(path)
                .ok()
                .and_then(|f| zip::ZipArchive::new(BufReader::new(f)).ok())
                .is_some_and(|archive| {
                    let wanted = format!("{prefix}assets/minecraft/");
                    archive
                        .file_names()
                        .any(|n| n.is_ok_and(|n| n.starts_with(&wanted)))
                }),
        }
    }

    pub fn text(&self, rel: &str) -> Option<String> {
        self.read(rel)
            .map(|b| String::from_utf8_lossy(&b).into_owned())
    }

    /// The folder or `.zip` itself.
    pub fn path(&self) -> &Path {
        match self {
            Source::Dir(path) | Source::Zip { path, .. } => path,
        }
    }
}

/// A mod found on disk, usable or not.
#[derive(Debug, Clone)]
pub struct Found {
    /// The manifest's id, or the file name if the manifest is unreadable.
    pub id: String,
    pub manifest: Option<Manifest>,
    pub source: Source,
    /// Shipped with the game (Fullbright).
    pub bundled: bool,
    /// Why it can't be used.
    pub problem: Option<String>,
}

/// Enabled mods and their order, kept in `mods.ron` beside the saves.
#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
pub struct Config {
    /// Ids turned off in the Mods screen.
    #[serde(default)]
    pub disabled: Vec<String>,
    /// Load order; mods not listed come after, by name.
    #[serde(default)]
    pub order: Vec<String>,
}

impl Config {
    pub fn path() -> PathBuf {
        crate::level::saves_dir().with_file_name("mods.ron")
    }

    pub fn load() -> Self {
        std::fs::read_to_string(Self::path())
            .ok()
            .and_then(|text| ron::from_str(&text).ok())
            .unwrap_or_default()
    }

    pub fn save(&self) -> Result<()> {
        let text = ron::ser::to_string_pretty(self, ron::ser::PrettyConfig::default())?;
        std::fs::write(Self::path(), text).context("writing mods.ron")
    }
}

/// The folders mods are looked for in: `mods/` beside the saves, and the
/// one shipped inside the app bundle (the same folder when run from the
/// source tree).
pub fn mod_dirs() -> Vec<PathBuf> {
    let mut dirs = vec![crate::level::saves_dir().with_file_name("mods")];
    let shipped = crate::assets::dir()
        .parent()
        .map_or_else(|| PathBuf::from("mods"), |p| p.join("mods"));
    let same = |a: &Path, b: &Path| {
        a.canonicalize()
            .ok()
            .zip(b.canonicalize().ok())
            .is_some_and(|(a, b)| a == b)
    };
    if !same(&dirs[0], &shipped) {
        dirs.push(shipped);
    }
    dirs
}

fn open_zip_source(path: &Path) -> Result<Source> {
    let file = File::open(path)?;
    let archive = zip::ZipArchive::new(BufReader::new(file))?;
    let prefix = archive
        .file_names()
        .filter_map(|name| name.ok())
        .filter_map(|n| n.strip_suffix("mod.ron").map(str::to_owned))
        .filter(|p| p.is_empty() || (p.ends_with('/') && !p[..p.len() - 1].contains('/')))
        .min_by_key(|p| p.len())
        .context("no mod.ron")?;
    Ok(Source::Zip {
        path: path.to_owned(),
        prefix,
    })
}

/// Reads and checks a manifest.
pub fn parse_manifest(text: &str) -> Result<Manifest> {
    let manifest: Manifest = ron::from_str(text).context("mod.ron")?;
    if manifest.id.is_empty() || !manifest.id.chars().all(|c| MOD_ID_CHARS.contains(c)) {
        bail!(
            "the id {:?} may only have a-z, 0-9 and _ (it is the namespace of the mod's blocks and items)",
            manifest.id
        );
    }
    if manifest.id == "minecraft" {
        bail!("the id minecraft is taken");
    }
    Ok(manifest)
}

/// Every mod in `dirs`, sorted by name; the first of two with one id wins.
pub fn discover(dirs: &[PathBuf], bundled_dir: Option<&Path>) -> Vec<Found> {
    let mut found: Vec<Found> = Vec::new();
    for dir in dirs {
        let Ok(entries) = std::fs::read_dir(dir) else {
            continue;
        };
        let mut paths: Vec<PathBuf> = entries.filter_map(|e| e.ok().map(|e| e.path())).collect();
        paths.sort();
        for path in paths {
            let is_zip = path
                .extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("zip"));
            let file_name = path
                .file_stem()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            if !(path.is_dir() || is_zip) || file_name.starts_with('.') {
                continue;
            }
            let source = if is_zip {
                match open_zip_source(&path) {
                    Ok(source) => source,
                    Err(err) => {
                        found.push(Found {
                            id: file_name,
                            manifest: None,
                            source: Source::Dir(path),
                            bundled: false,
                            problem: Some(format!("{err:#}")),
                        });
                        continue;
                    }
                }
            } else if path.join("mod.ron").is_file() {
                Source::Dir(path.clone())
            } else {
                continue;
            };
            let bundled = bundled_dir.is_some_and(|b| b == dir);
            let parsed = source
                .text("mod.ron")
                .context("cannot read mod.ron")
                .and_then(|text| parse_manifest(&text));
            let (id, manifest, problem) = match parsed {
                Ok(m) => (m.id.clone(), Some(m), None),
                Err(err) => (file_name, None, Some(format!("{err:#}"))),
            };
            if found.iter().any(|f| f.id == id) {
                continue;
            }
            found.push(Found {
                id,
                manifest,
                source,
                bundled,
                problem,
            });
        }
    }
    found.sort_by_key(|f| {
        f.manifest
            .as_ref()
            .map_or(f.id.to_lowercase(), |m| m.name.to_lowercase())
    });
    found
}

/// `1.2.3` → numbers, for comparing versions.
fn version_numbers(text: &str) -> Vec<u32> {
    text.split('.')
        .map(|p| p.trim().parse().unwrap_or(0))
        .collect()
}

/// Whether `have` is at least `need` (missing parts count as 0).
pub fn version_at_least(have: &str, need: &str) -> bool {
    let (have, need) = (version_numbers(have), version_numbers(need));
    for i in 0..have.len().max(need.len()) {
        let (a, b) = (
            have.get(i).copied().unwrap_or(0),
            need.get(i).copied().unwrap_or(0),
        );
        if a != b {
            return a > b;
        }
    }
    true
}

/// What happened to each found mod when the set was chosen.
#[derive(Debug, Clone, PartialEq)]
pub enum Status {
    Active,
    Disabled,
    /// Turned off by the game, with the reason (missing dependency...).
    Failed(String),
}

/// One row of the Mods screen.
#[derive(Debug, Clone)]
pub struct Entry {
    pub found: Found,
    pub status: Status,
}

/// A mod's data files, read as text.
#[derive(Debug, Clone, Default)]
pub struct Data {
    pub blocks: Option<String>,
    pub items: Option<String>,
    pub recipes: Option<String>,
    pub creative_tabs: Option<String>,
    pub mob_drops: Option<String>,
    pub loot: Option<String>,
}

/// A mod that is running.
#[derive(Debug, Clone)]
pub struct Active {
    pub manifest: Manifest,
    pub source: Source,
    pub data: Data,
    pub lua: Option<String>,
}

/// A block or item defined in `main.lua`, as JSON (the `BlockDef` or
/// `ItemDef` fields).
#[derive(Debug, Clone)]
pub struct LuaDef {
    pub mod_id: String,
    pub def: serde_json::Value,
}

/// A command from `rc.register_command`.
#[derive(Debug, Clone, PartialEq)]
pub struct CommandSpec {
    pub mod_id: String,
    pub name: String,
    pub usage: String,
    pub cheat: bool,
    pub host: bool,
}

/// A key binding from `rc.register_key`.
#[derive(Debug, Clone, PartialEq)]
pub struct KeySpec {
    pub mod_id: String,
    /// `mod:key`.
    pub id: String,
    pub name: String,
    /// A key name as in `bindings.rs` ("G").
    pub default: String,
}

/// Everything the running mods add. Chosen once per run.
#[derive(Default)]
pub struct ModSet {
    pub active: Vec<Active>,
    pub catalog: Vec<Entry>,
    pub lua_blocks: Vec<LuaDef>,
    pub lua_items: Vec<LuaDef>,
    pub commands: Vec<CommandSpec>,
    pub keys: Vec<KeySpec>,
    /// `mod:key`, the `options.txt` id (`key.mod.mod:key`) and the label of
    /// each key, as static strings for the bindings.
    pub key_ids: Vec<&'static str>,
    pub key_option_ids: Vec<&'static str>,
    pub key_names: Vec<&'static str>,
    block_names: Vec<&'static str>,
    item_names: Vec<&'static str>,
    block_index: HashMap<&'static str, u16>,
    item_index: HashMap<&'static str, u16>,
}

static ACTIVE: OnceLock<ModSet> = OnceLock::new();
static ERRORS: Mutex<Vec<(String, String)>> = Mutex::new(Vec::new());

/// The mods this run has; none until `init` ran. Tests run with the
/// bundled mods and the Ruby example, so they cover the game with mods.
pub fn active() -> &'static ModSet {
    ACTIVE.get_or_init(|| {
        if cfg!(test) {
            let root = Path::new(env!("CARGO_MANIFEST_DIR"));
            let dirs = [root.join("mods"), root.join("examples/mods")];
            let found = discover(&dirs, Some(&dirs[0]));
            ModSet::choose(found, &Config::default())
        } else {
            ModSet::default()
        }
    })
}

/// Remembers a Lua error for the Mods screen (the last 50 are kept).
pub fn report_error(mod_id: &str, text: String) {
    warn!("mod {mod_id}: {text}");
    let mut errors = ERRORS.lock().unwrap_or_else(|e| e.into_inner());
    errors.push((mod_id.to_owned(), text));
    if errors.len() > 50 {
        errors.remove(0);
    }
}

/// The errors mod `id` has caused so far.
pub fn errors_of(id: &str) -> Vec<String> {
    let errors = ERRORS.lock().unwrap_or_else(|e| e.into_inner());
    errors
        .iter()
        .filter(|(mod_id, _)| mod_id == id)
        .map(|(_, text)| text.clone())
        .collect()
}

/// Chooses the mods from the disk and the config, runs their `main.lua`
/// registrations and fixes the block and item names. Call once, before
/// any registry is used; later calls do nothing.
pub fn init() {
    let dirs = mod_dirs();
    let bundled = dirs.last().cloned();
    let found = discover(&dirs, bundled.as_deref());
    init_with(found, &Config::load());
}

/// `init` with the mods and configuration given (tests).
pub fn init_with(found: Vec<Found>, config: &Config) {
    let set = ModSet::choose(found, config);
    if ACTIVE.set(set).is_err() {
        warn!("mods were already chosen for this run");
    }
}

impl ModSet {
    /// Picks the enabled mods in load order, reads their data and runs
    /// their registrations.
    pub fn choose(found: Vec<Found>, config: &Config) -> Self {
        let mut set = ModSet::default();
        let rank = |id: &str| {
            config
                .order
                .iter()
                .position(|o| o == id)
                .unwrap_or(usize::MAX)
        };
        let mut order: Vec<usize> = (0..found.len()).collect();
        order.sort_by_key(|&i| rank(&found[i].id));
        let mut statuses: Vec<Status> = found
            .iter()
            .map(|f| match (&f.problem, config.disabled.contains(&f.id)) {
                (Some(problem), _) => Status::Failed(problem.clone()),
                (None, true) => Status::Disabled,
                (None, false) => Status::Active,
            })
            .collect();
        // Dependencies and game version; repeated, since a failure can
        // starve a mod that needed the one that failed.
        loop {
            let mut changed = false;
            for &i in &order {
                let Some(manifest) = &found[i].manifest else {
                    continue;
                };
                if statuses[i] != Status::Active {
                    continue;
                }
                let why = if !manifest.rustcraft.is_empty()
                    && !version_at_least(GAME_VERSION, &manifest.rustcraft)
                {
                    Some(format!(
                        "needs Rustcraft {} or newer (this is {GAME_VERSION})",
                        manifest.rustcraft
                    ))
                } else {
                    manifest.dependencies.iter().find_map(|dep| {
                        let ok = found
                            .iter()
                            .zip(&statuses)
                            .any(|(f, s)| f.id == *dep && *s == Status::Active);
                        (!ok).then(|| format!("needs the mod {dep}, which is missing or off"))
                    })
                };
                if let Some(why) = why {
                    statuses[i] = Status::Failed(why);
                    changed = true;
                }
            }
            if !changed {
                break;
            }
        }
        // Dependencies first: a stable pass that moves a mod after the
        // last of its dependencies.
        let mut sorted: Vec<usize> = Vec::new();
        let mut pending: Vec<usize> = order
            .iter()
            .copied()
            .filter(|&i| statuses[i] == Status::Active)
            .collect();
        while !pending.is_empty() {
            let ready = pending.iter().position(|&i| {
                found[i].manifest.as_ref().is_some_and(|m| {
                    m.dependencies
                        .iter()
                        .all(|d| sorted.iter().any(|&s| found[s].id == *d))
                })
            });
            match ready {
                Some(at) => sorted.push(pending.remove(at)),
                None => {
                    for &i in &pending {
                        statuses[i] =
                            Status::Failed("its dependencies depend on each other".into());
                    }
                    break;
                }
            }
        }
        for &i in &sorted {
            let f = &found[i];
            let manifest = f.manifest.clone().expect("active mods have manifests");
            let read = |name: &str| f.source.text(&format!("data/{name}"));
            set.active.push(Active {
                data: Data {
                    blocks: read("blocks.ron"),
                    items: read("items.ron"),
                    recipes: read("recipes.ron"),
                    creative_tabs: read("creative_tabs.ron"),
                    mob_drops: read("mob_drops.ron"),
                    loot: read("loot.ron"),
                },
                lua: f.source.text("main.lua"),
                source: f.source.clone(),
                manifest,
            });
        }
        // Names from the data files first, then the Lua registrations.
        for index in 0..set.active.len() {
            if let Err(err) = set.register_data_names(index) {
                let id = set.active[index].manifest.id.clone();
                report_error(&id, format!("{err:#}"));
                // A broken data file keeps its other content out.
                set.active[index].data = Data::default();
            }
        }
        lua::register(&mut set);
        set.keys.truncate(crate::bindings::MOD_KEYS);
        for key in &set.keys {
            set.key_ids.push(Box::leak(key.id.clone().into_boxed_str()));
            set.key_option_ids
                .push(Box::leak(format!("key.mod.{}", key.id).into_boxed_str()));
            set.key_names
                .push(Box::leak(key.name.clone().into_boxed_str()));
        }
        for (found, status) in found.into_iter().zip(statuses) {
            set.catalog.push(Entry { found, status });
        }
        info!(
            "mods: {} active ({})",
            set.active.len(),
            set.active
                .iter()
                .map(|m| m.manifest.id.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        );
        set
    }

    fn register_data_names(&mut self, index: usize) -> Result<()> {
        #[derive(Deserialize)]
        struct Named {
            name: String,
            #[serde(default = "yes")]
            item: bool,
        }
        fn yes() -> bool {
            true
        }
        let id = self.active[index].manifest.id.clone();
        let own = |name: &str| {
            name.strip_prefix(&format!("{id}:"))
                .is_some_and(|rest| !rest.is_empty())
        };
        let mut blocks = Vec::new();
        let mut block_items = Vec::new();
        if let Some(text) = &self.active[index].data.blocks {
            let list: Vec<Named> = ron::from_str(text).context("data/blocks.ron")?;
            for b in list {
                if !own(&b.name) {
                    bail!("block {:?} must be named {id}:<name>", b.name);
                }
                if b.item {
                    block_items.push(b.name.clone());
                }
                blocks.push(b.name);
            }
        }
        let mut items = Vec::new();
        if let Some(text) = &self.active[index].data.items {
            let list: Vec<Named> = ron::from_str(text).context("data/items.ron")?;
            for i in list {
                if !own(&i.name) {
                    bail!("item {:?} must be named {id}:<name>", i.name);
                }
                items.push(i.name);
            }
        }
        for name in &blocks {
            self.add_block_name(name)?;
        }
        for name in block_items.iter().chain(&items) {
            self.add_item_name(name)?;
        }
        Ok(())
    }

    pub(crate) fn add_block_name(&mut self, name: &str) -> Result<()> {
        let name: &'static str = Box::leak(name.to_owned().into_boxed_str());
        if crate::world::blocks::by_builtin(name).is_some() || self.block_index.contains_key(name) {
            bail!("two blocks are named {name}");
        }
        self.block_index.insert(
            name,
            (crate::world::blocks::COUNT + self.block_names.len()) as u16,
        );
        self.block_names.push(name);
        Ok(())
    }

    /// Items are registered in the order the registry will give them
    /// out: the block items of every mod's data, then its items; a
    /// block from `main.lua` has its item registered with it.
    pub(crate) fn add_item_name(&mut self, name: &str) -> Result<()> {
        let name: &'static str = Box::leak(name.to_owned().into_boxed_str());
        if crate::inventory::items::by_builtin(name).is_some() || self.item_index.contains_key(name)
        {
            bail!("two items are named {name}");
        }
        self.item_index.insert(
            name,
            (crate::inventory::items::COUNT + self.item_names.len()) as u16,
        );
        self.item_names.push(name);
        Ok(())
    }

    /// The (id, version) of each mod a guest must also have.
    pub fn required(&self) -> Vec<(String, String)> {
        self.active
            .iter()
            .filter(|m| !m.manifest.client_only)
            .map(|m| (m.manifest.id.clone(), m.manifest.version.clone()))
            .collect()
    }

    /// The (id, version) of every active mod, for a world's record.
    pub fn all_versions(&self) -> Vec<(String, String)> {
        self.active
            .iter()
            .map(|m| (m.manifest.id.clone(), m.manifest.version.clone()))
            .collect()
    }

    /// The block `index` (counting from the first mod block) is named.
    pub fn block_name(&self, index: usize) -> Option<&'static str> {
        self.block_names
            .get(index.checked_sub(crate::world::blocks::COUNT)?)
            .copied()
    }

    pub fn item_name(&self, index: usize) -> Option<&'static str> {
        self.item_names
            .get(index.checked_sub(crate::inventory::items::COUNT)?)
            .copied()
    }

    /// Every mod block's name, in ID order.
    pub fn block_names_all(&self) -> &[&'static str] {
        &self.block_names
    }

    pub fn item_names_all(&self) -> &[&'static str] {
        &self.item_names
    }

    pub fn block_by_name(&self, name: &str) -> Option<u16> {
        self.block_index.get(name).copied()
    }

    pub fn item_by_name(&self, name: &str) -> Option<u16> {
        self.item_index.get(name).copied()
    }

    pub fn is_empty(&self) -> bool {
        self.active.is_empty()
    }

    /// The resource layers of the mods, lowest-priority last... they all
    /// sit over the base textures and under the player's packs.
    pub fn packs(&self) -> Vec<crate::pack::Pack> {
        self.active
            .iter()
            .rev()
            .filter(|m| m.source.has_assets())
            .filter_map(|m| match crate::pack::Pack::open_mod(m.source.path()) {
                Ok(pack) => Some(pack),
                Err(err) => {
                    warn!("mod {} has no resources: {err:#}", m.manifest.id);
                    None
                }
            })
            .collect()
    }
}

/// A mod name for the Mods screen: `Name 1.0`.
pub fn title(manifest: &Manifest) -> String {
    format!("{} {}", manifest.name, manifest.version)
}

/// Compares a world's or a host's mods with ours: the lines saying what
/// differs (empty when they match). `theirs` and `ours` are (id, version).
pub fn differences(theirs: &[(String, String)], ours: &[(String, String)]) -> Vec<String> {
    let mut out = Vec::new();
    for (id, version) in theirs {
        match ours.iter().find(|(o, _)| o == id) {
            None => out.push(format!("missing: {id} {version}")),
            Some((_, mine)) if mine != version => {
                out.push(format!("{id}: host has {version}, you have {mine}"))
            }
            _ => {}
        }
    }
    for (id, version) in ours {
        if !theirs.iter().any(|(t, _)| t == id) {
            out.push(format!("not on the host: {id} {version}"));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_compare_by_number() {
        assert!(version_at_least("1.2.0", "1.2"));
        assert!(version_at_least("1.10", "1.9"));
        assert!(!version_at_least("1.1.9", "1.2"));
        assert!(version_at_least("1.2", ""));
    }

    #[test]
    fn manifests_need_a_safe_id() {
        let ok = r#"(id: "ruby", name: "Ruby", version: "1.0")"#;
        assert_eq!(parse_manifest(ok).unwrap().id, "ruby");
        for bad in ["Ruby", "a:b", "", "minecraft", "../x"] {
            let text = format!(r#"(id: "{bad}", name: "x", version: "1")"#);
            assert!(parse_manifest(&text).is_err(), "{bad}");
        }
        assert!(parse_manifest(r#"(id: "a", name: "x", version: "1", nope: 1)"#).is_err());
    }

    #[test]
    fn differences_name_what_does_not_match() {
        let host = vec![
            ("a".to_owned(), "1".to_owned()),
            ("b".to_owned(), "2".to_owned()),
        ];
        let mine = vec![
            ("a".to_owned(), "2".to_owned()),
            ("c".to_owned(), "1".to_owned()),
        ];
        let lines = differences(&host, &mine);
        assert_eq!(lines.len(), 3);
        assert!(differences(&host, &host).is_empty());
    }

    fn found(id: &str, deps: &[&str], needs: &str) -> Found {
        Found {
            id: id.to_owned(),
            manifest: Some(Manifest {
                id: id.to_owned(),
                name: id.to_owned(),
                version: "1".into(),
                authors: vec![],
                description: String::new(),
                rustcraft: needs.to_owned(),
                dependencies: deps.iter().map(|d| (*d).to_owned()).collect(),
                client_only: false,
            }),
            source: Source::Dir(PathBuf::from("/nonexistent")),
            bundled: false,
            problem: None,
        }
    }

    #[test]
    fn dependencies_load_first_and_missing_ones_fail() {
        let set = ModSet::choose(
            vec![
                found("b", &["a"], ""),
                found("a", &[], ""),
                found("c", &["zzz"], ""),
                found("d", &[], "99.0"),
            ],
            &Config::default(),
        );
        let ids: Vec<_> = set.active.iter().map(|m| m.manifest.id.as_str()).collect();
        assert_eq!(ids, ["a", "b"]);
        let failed = |id: &str| {
            matches!(
                &set.catalog
                    .iter()
                    .find(|e| e.found.id == id)
                    .unwrap()
                    .status,
                Status::Failed(_)
            )
        };
        assert!(failed("c") && failed("d"));
    }

    #[test]
    fn the_config_turns_mods_off_and_orders_them() {
        let config = Config {
            disabled: vec!["a".into()],
            order: vec!["c".into(), "b".into()],
        };
        let set = ModSet::choose(
            vec![
                found("a", &[], ""),
                found("b", &[], ""),
                found("c", &[], ""),
            ],
            &config,
        );
        let ids: Vec<_> = set.active.iter().map(|m| m.manifest.id.as_str()).collect();
        assert_eq!(ids, ["c", "b"]);
    }
}
