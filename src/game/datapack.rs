//! Data packs (M45): `saves/<world>/datapacks/<pack>/` with a `pack.mcmeta`
//! and `data/<namespace>/...`. The game reads functions
//! (`function/**/*.mcfunction`), function tags (`tags/function/*.json`),
//! predicates (`predicate/*.json`) and loot tables (`loot_table/*.json`).

use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
};

use super::predicate::Predicate;
use crate::world::structure::loot::{Entry, Pool};

/// A pack folder with its `pack.mcmeta`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pack {
    /// `file/<folder>`, as `/datapack` names it.
    pub name: String,
    pub dir: PathBuf,
    pub description: String,
}

/// Everything the enabled packs add.
#[derive(Default, Clone)]
pub struct Loaded {
    /// Function id (`ns:path`) → its lines, continuations joined.
    pub functions: BTreeMap<String, Vec<String>>,
    /// Function tag id → its entries (ids, or `#tags`), in order.
    pub tags: BTreeMap<String, Vec<String>>,
    pub predicates: BTreeMap<String, Predicate>,
    pub loot: BTreeMap<String, Vec<Pool>>,
}

/// The packs in `world/datapacks`, those with a valid `pack.mcmeta`.
pub fn discover(world: &Path) -> Vec<Pack> {
    let Ok(entries) = fs::read_dir(world.join("datapacks")) else {
        return Vec::new();
    };
    let mut packs: Vec<Pack> = entries
        .flatten()
        .filter(|e| e.path().is_dir())
        .filter_map(|e| {
            let dir = e.path();
            let meta: serde_json::Value =
                serde_json::from_str(&fs::read_to_string(dir.join("pack.mcmeta")).ok()?).ok()?;
            let pack = meta.get("pack")?;
            pack.get("pack_format")?;
            let description = match pack.get("description") {
                Some(serde_json::Value::String(s)) => s.clone(),
                Some(other) => other
                    .get("text")
                    .and_then(|t| t.as_str())
                    .unwrap_or("")
                    .to_owned(),
                None => String::new(),
            };
            Some(Pack {
                name: format!("file/{}", e.file_name().to_string_lossy()),
                dir,
                description,
            })
        })
        .collect();
    packs.sort_by(|a, b| a.name.cmp(&b.name));
    packs
}

/// The order and choice of packs: those enabled (first has the lowest
/// priority, later ones override), saved in `datapacks.txt`; a pack not
/// mentioned there is enabled the first time the world sees it, as vanilla.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Selection {
    pub enabled: Vec<String>,
    pub disabled: BTreeSet<String>,
}

const FILE: &str = "datapacks.txt";

impl Selection {
    pub fn load(world: &Path) -> Self {
        let mut sel = Selection::default();
        for line in fs::read_to_string(world.join(FILE))
            .unwrap_or_default()
            .lines()
        {
            match line.split_once(' ') {
                Some(("+", name)) => sel.enabled.push(name.to_owned()),
                Some(("-", name)) => {
                    sel.disabled.insert(name.to_owned());
                }
                _ => {}
            }
        }
        sel
    }

    pub fn save(&self, world: &Path) {
        let mut text = String::new();
        for name in &self.enabled {
            text.push_str(&format!("+ {name}\n"));
        }
        for name in &self.disabled {
            text.push_str(&format!("- {name}\n"));
        }
        let _ = fs::create_dir_all(world);
        if let Err(err) = fs::write(world.join(FILE), text) {
            log::warn!("couldn't write {FILE}: {err}");
        }
    }

    /// Enables packs never seen before, and drops those that are gone.
    pub fn reconcile(&mut self, found: &[Pack]) {
        let names: BTreeSet<&str> = found.iter().map(|p| p.name.as_str()).collect();
        self.enabled.retain(|n| names.contains(n.as_str()));
        self.disabled.retain(|n| names.contains(n.as_str()));
        for pack in found {
            if !self.enabled.contains(&pack.name) && !self.disabled.contains(&pack.name) {
                self.enabled.push(pack.name.clone());
            }
        }
    }
}

fn id_of(namespace: &str, path: &Path, root: &Path, ext: &str) -> Option<String> {
    let rel = path.strip_prefix(root).ok()?.with_extension("");
    let rel = rel.to_string_lossy().replace('\\', "/");
    let _ = ext;
    Some(format!("{namespace}:{rel}"))
}

fn files_under(dir: &Path, ext: &str, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    let mut entries: Vec<_> = entries.flatten().map(|e| e.path()).collect();
    entries.sort();
    for path in entries {
        if path.is_dir() {
            files_under(&path, ext, out);
        } else if path.extension().is_some_and(|e| e == ext) {
            out.push(path);
        }
    }
}

/// A function file's commands: comments and blank lines dropped, a line
/// ending in `\` joined with the next.
pub fn function_lines(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut joined = String::new();
    for raw in text.lines() {
        let line = raw.trim_end();
        if let Some(head) = line.strip_suffix('\\') {
            joined.push_str(head.trim_start());
            continue;
        }
        joined.push_str(line.trim_start());
        let full = std::mem::take(&mut joined);
        let trimmed = full.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        out.push(trimmed.strip_prefix('/').unwrap_or(trimmed).to_owned());
    }
    out
}

/// A vanilla loot table's pools, as far as the game's rolls go: item
/// entries with weights and `set_count`, and `empty` entries.
pub fn loot_pools(json: &serde_json::Value) -> Result<Vec<Pool>, String> {
    let range = |v: Option<&serde_json::Value>| -> (u32, u32) {
        match v {
            Some(serde_json::Value::Number(n)) => {
                let n = n.as_u64().unwrap_or(1) as u32;
                (n, n)
            }
            Some(obj) => (
                obj.get("min").and_then(|m| m.as_u64()).unwrap_or(1) as u32,
                obj.get("max").and_then(|m| m.as_u64()).unwrap_or(1) as u32,
            ),
            None => (1, 1),
        }
    };
    let mut pools = Vec::new();
    for pool in json
        .get("pools")
        .and_then(|p| p.as_array())
        .ok_or("A loot table needs pools")?
    {
        let (lo, hi) = range(pool.get("rolls"));
        let mut entries = Vec::new();
        for entry in pool
            .get("entries")
            .and_then(|e| e.as_array())
            .ok_or("A pool needs entries")?
        {
            let weight = entry.get("weight").and_then(|w| w.as_u64()).unwrap_or(1) as u32;
            let kind = entry
                .get("type")
                .and_then(|t| t.as_str())
                .unwrap_or("minecraft:item");
            let item = match kind.strip_prefix("minecraft:").unwrap_or(kind) {
                "empty" => "",
                "item" => entry
                    .get("name")
                    .and_then(|n| n.as_str())
                    .map(|n| n.strip_prefix("minecraft:").unwrap_or(n))
                    .ok_or("An item entry needs a name")?,
                other => return Err(format!("Unknown loot entry type '{other}'")),
            };
            let (mut min, mut max) = (1u32, 1u32);
            for f in entry
                .get("functions")
                .and_then(|f| f.as_array())
                .into_iter()
                .flatten()
            {
                let name = f.get("function").and_then(|n| n.as_str()).unwrap_or("");
                if name.strip_prefix("minecraft:").unwrap_or(name) == "set_count" {
                    (min, max) = range(f.get("count"));
                }
            }
            entries.push(Entry {
                item: Box::leak(item.to_owned().into_boxed_str()),
                weight: weight.max(1),
                min: min.min(64) as u8,
                max: max.clamp(min, 64) as u8,
                enchant: None,
            });
        }
        pools.push(Pool {
            rolls: (lo, hi.max(lo)),
            entries,
        });
    }
    Ok(pools)
}

/// Reads the packs, earlier ones first (later ones win). Problems come
/// back as lines for the log, never stopping the rest.
pub fn load(packs: &[Pack]) -> (Loaded, Vec<String>) {
    let mut loaded = Loaded::default();
    let mut errors = Vec::new();
    for pack in packs {
        let data = pack.dir.join("data");
        let Ok(spaces) = fs::read_dir(&data) else {
            continue;
        };
        let mut spaces: Vec<_> = spaces.flatten().filter(|e| e.path().is_dir()).collect();
        spaces.sort_by_key(|e| e.file_name());
        for space in spaces {
            let ns = space.file_name().to_string_lossy().into_owned();
            let root = space.path();
            // Functions (`functions/` is the older folder name).
            for folder in ["function", "functions"] {
                let mut files = Vec::new();
                files_under(&root.join(folder), "mcfunction", &mut files);
                for file in files {
                    let Some(id) = id_of(&ns, &file, &root.join(folder), "mcfunction") else {
                        continue;
                    };
                    match fs::read_to_string(&file) {
                        Ok(text) => {
                            loaded.functions.insert(id, function_lines(&text));
                        }
                        Err(err) => errors.push(format!("{}: {err}", file.display())),
                    }
                }
            }
            let json_files = |sub: &str| {
                let mut files = Vec::new();
                files_under(&root.join(sub), "json", &mut files);
                files
            };
            for folder in ["tags/function", "tags/functions"] {
                for file in json_files(folder) {
                    let Some(id) = id_of(&ns, &file, &root.join(folder), "json") else {
                        continue;
                    };
                    let parsed = fs::read_to_string(&file)
                        .map_err(|e| e.to_string())
                        .and_then(|t| {
                            serde_json::from_str::<serde_json::Value>(&t).map_err(|e| e.to_string())
                        });
                    match parsed {
                        Ok(v) => {
                            let values: Vec<String> = v
                                .get("values")
                                .and_then(|a| a.as_array())
                                .into_iter()
                                .flatten()
                                .filter_map(|e| match e {
                                    serde_json::Value::String(s) => Some(s.clone()),
                                    other => {
                                        other.get("id").and_then(|i| i.as_str()).map(str::to_owned)
                                    }
                                })
                                .collect();
                            let replace =
                                v.get("replace").and_then(|r| r.as_bool()).unwrap_or(false);
                            let entry = loaded.tags.entry(id).or_default();
                            if replace {
                                entry.clear();
                            }
                            entry.extend(values);
                        }
                        Err(err) => errors.push(format!("{}: {err}", file.display())),
                    }
                }
            }
            for folder in ["predicate", "predicates"] {
                for file in json_files(folder) {
                    let Some(id) = id_of(&ns, &file, &root.join(folder), "json") else {
                        continue;
                    };
                    let parsed = fs::read_to_string(&file)
                        .map_err(|e| e.to_string())
                        .and_then(|t| Predicate::parse(&t));
                    match parsed {
                        Ok(p) => {
                            loaded.predicates.insert(id, p);
                        }
                        Err(err) => errors.push(format!("{}: {err}", file.display())),
                    }
                }
            }
            for folder in ["loot_table", "loot_tables"] {
                for file in json_files(folder) {
                    let Some(id) = id_of(&ns, &file, &root.join(folder), "json") else {
                        continue;
                    };
                    let parsed = fs::read_to_string(&file)
                        .map_err(|e| e.to_string())
                        .and_then(|t| {
                            serde_json::from_str::<serde_json::Value>(&t).map_err(|e| e.to_string())
                        })
                        .and_then(|v| loot_pools(&v));
                    match parsed {
                        Ok(pools) => {
                            loaded.loot.insert(id, pools);
                        }
                        Err(err) => errors.push(format!("{}: {err}", file.display())),
                    }
                }
            }
        }
    }
    (loaded, errors)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn function_files_drop_comments_and_join_continuations() {
        let lines = function_lines(
            "# hello\n\nsay hi\n/say slash\nsummon pig ~ ~ ~ \\\n  {Tags:[\"a\"]}\n$say $(who)\n",
        );
        assert_eq!(
            lines,
            [
                "say hi",
                "say slash",
                "summon pig ~ ~ ~ {Tags:[\"a\"]}",
                "$say $(who)"
            ]
        );
    }

    #[test]
    fn a_pack_folder_loads_functions_tags_predicates_and_loot() {
        let root = std::env::temp_dir().join(format!("rc-pack-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let pack = root.join("datapacks/demo");
        let write = |path: &str, text: &str| {
            let p = pack.join(path);
            fs::create_dir_all(p.parent().unwrap()).unwrap();
            fs::write(p, text).unwrap();
        };
        write(
            "pack.mcmeta",
            r#"{"pack":{"pack_format":48,"description":"Demo pack"}}"#,
        );
        write(
            "data/demo/function/hello.mcfunction",
            "say hello\nsay again\n",
        );
        write("data/demo/function/deep/inner.mcfunction", "say inner\n");
        write(
            "data/minecraft/tags/function/tick.json",
            r#"{"values":["demo:hello"]}"#,
        );
        write(
            "data/demo/predicate/always.json",
            r#"{"condition":"random_chance","chance":1}"#,
        );
        write(
            "data/demo/loot_table/prize.json",
            r#"{"pools":[{"rolls":2,"entries":[{"type":"minecraft:item","name":"minecraft:diamond","weight":3,"functions":[{"function":"minecraft:set_count","count":{"min":1,"max":2}}]},{"type":"minecraft:empty","weight":1}]}]}"#,
        );
        write("data/demo/predicate/broken.json", "{");
        let found = discover(&root);
        assert_eq!(found.len(), 1);
        assert_eq!(
            (found[0].name.as_str(), found[0].description.as_str()),
            ("file/demo", "Demo pack")
        );
        let (loaded, errors) = load(&found);
        assert_eq!(loaded.functions["demo:hello"], ["say hello", "say again"]);
        assert_eq!(loaded.functions["demo:deep/inner"], ["say inner"]);
        assert_eq!(loaded.tags["minecraft:tick"], ["demo:hello"]);
        assert!(loaded.predicates.contains_key("demo:always"));
        assert_eq!(errors.len(), 1, "{errors:?}");
        let pools = &loaded.loot["demo:prize"];
        assert_eq!(pools[0].rolls, (2, 2));
        assert_eq!(
            (
                pools[0].entries[0].item,
                pools[0].entries[0].weight,
                pools[0].entries[0].min,
                pools[0].entries[0].max
            ),
            ("diamond", 3, 1, 2)
        );
        assert_eq!(pools[0].entries[1].item, "");
        // A new pack is enabled; a disabled one stays so; the selection saves.
        let mut sel = Selection::default();
        sel.reconcile(&found);
        assert_eq!(sel.enabled, ["file/demo"]);
        sel.enabled.clear();
        sel.disabled.insert("file/demo".into());
        sel.save(&root);
        let mut back = Selection::load(&root);
        back.reconcile(&found);
        assert!(back.enabled.is_empty() && back.disabled.contains("file/demo"));
        let _ = fs::remove_dir_all(&root);
    }
}
