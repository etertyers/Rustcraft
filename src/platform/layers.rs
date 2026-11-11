//! The vanilla layer check (M71), the same rules as `scripts/check-vanilla.sh`:
//! textures, models, blockstates and items present, `lang/en_us.json`,
//! `sounds.json` and every `.ogg` it names, `font/default.json` and every
//! font page it names. The browser's setup screen runs it on the files the
//! player picks; it works on any list of paths and a way to read one file.

use std::collections::HashSet;

/// What the check found.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct VanillaReport {
    pub textures: usize,
    pub models: usize,
    pub blockstates: usize,
    pub items: usize,
    pub sound_files: usize,
    pub font_pages: usize,
    /// One line per thing missing; empty means complete.
    pub problems: Vec<String>,
}

impl VanillaReport {
    pub fn complete(&self) -> bool {
        self.problems.is_empty()
    }

    pub fn summary(&self) -> String {
        if self.complete() {
            format!(
                "vanilla layer complete: {} textures, {} models, {} blockstates, {} items, {} sound files, {} font pages",
                self.textures,
                self.models,
                self.blockstates,
                self.items,
                self.sound_files,
                self.font_pages
            )
        } else {
            self.problems.join("\n")
        }
    }
}

/// `paths` are relative to `assets/minecraft/` (`textures/block/stone.png`);
/// `read` returns a file's bytes.
pub fn check_vanilla(
    paths: &HashSet<String>,
    read: &dyn Fn(&str) -> Option<Vec<u8>>,
) -> VanillaReport {
    let mut report = VanillaReport::default();
    let count = |dir: &str| {
        let prefix = format!("{dir}/");
        paths.iter().filter(|p| p.starts_with(&prefix)).count()
    };
    report.textures = count("textures");
    report.models = count("models");
    report.blockstates = count("blockstates");
    report.items = count("items");
    for (name, n) in [
        ("textures", report.textures),
        ("models", report.models),
        ("blockstates", report.blockstates),
        ("items", report.items),
    ] {
        if n == 0 {
            report
                .problems
                .push(format!("the vanilla layer has no {name}/ files"));
        }
    }
    if !paths.contains("lang/en_us.json") {
        report
            .problems
            .push("the vanilla layer is missing lang/en_us.json".into());
    }

    let json =
        |path: &str| -> Option<serde_json::Value> { serde_json::from_slice(&read(path)?).ok() };
    // Every sound file sounds.json names.
    let mut named = HashSet::new();
    match json("sounds.json") {
        Some(serde_json::Value::Object(events)) => {
            for event in events.values() {
                let Some(sounds) = event.get("sounds").and_then(|s| s.as_array()) else {
                    continue;
                };
                for entry in sounds {
                    let (name, kind) = match entry {
                        serde_json::Value::String(s) => (s.as_str(), "sound"),
                        other => (
                            other.get("name").and_then(|n| n.as_str()).unwrap_or(""),
                            other
                                .get("type")
                                .and_then(|t| t.as_str())
                                .unwrap_or("sound"),
                        ),
                    };
                    if kind == "event" || (name.contains(':') && !name.starts_with("minecraft:")) {
                        continue;
                    }
                    named.insert(name.trim_start_matches("minecraft:").to_owned());
                }
            }
        }
        _ => report
            .problems
            .push("the vanilla layer is missing sounds.json".into()),
    }
    report.sound_files = named.len();
    let mut missing: Vec<&String> = named
        .iter()
        .filter(|n| !paths.contains(&format!("sounds/{n}.ogg")))
        .collect();
    missing.sort();
    if let Some(first) = missing.first() {
        report.problems.push(format!(
            "the vanilla layer is missing {} sound files (of {} named, e.g. {first}.ogg)",
            missing.len(),
            named.len()
        ));
    }

    // The font: default.json, the providers it references, and every page.
    let mut pages = HashSet::new();
    let mut todo = vec![("font/default.json".to_owned(), 0)];
    while let Some((path, depth)) = todo.pop() {
        let Some(data) = json(&path) else {
            report
                .problems
                .push(format!("the vanilla layer is missing {path}"));
            continue;
        };
        for provider in data
            .get("providers")
            .and_then(|p| p.as_array())
            .into_iter()
            .flatten()
        {
            match provider.get("type").and_then(|t| t.as_str()) {
                Some("reference") if depth < 4 => {
                    if let Some(id) = provider.get("id").and_then(|i| i.as_str()) {
                        todo.push((
                            format!("font/{}.json", id.trim_start_matches("minecraft:")),
                            depth + 1,
                        ));
                    }
                }
                Some("bitmap") => {
                    if let Some(file) = provider.get("file").and_then(|f| f.as_str()) {
                        pages.insert(format!(
                            "textures/{}",
                            file.trim_start_matches("minecraft:")
                        ));
                    }
                }
                _ => {}
            }
        }
    }
    report.font_pages = pages.len();
    let missing_pages = pages.iter().filter(|p| !paths.contains(*p)).count();
    if missing_pages > 0 {
        report.problems.push(format!(
            "the vanilla layer is missing {missing_pages} font pages"
        ));
    }
    report
}

/// Where a picked file lands in the vanilla layer: everything from
/// `assets/minecraft/` on goes under `assets/vanilla/`, whatever folder the
/// player picked or whatever a zip's top folder is called. Anything else
/// (the `data/` folder, a jar's classes) is skipped.
pub fn vanilla_destination(picked: &str) -> Option<String> {
    let picked = picked.replace('\\', "/");
    let i = picked.find("assets/minecraft/")?;
    let rel = &picked[i..];
    if rel.ends_with('/') {
        return None;
    }
    Some(format!("assets/vanilla/{rel}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn layer(skip: &[&str]) -> (HashSet<String>, std::collections::HashMap<String, Vec<u8>>) {
        let mut files = std::collections::HashMap::new();
        let mut add = |p: &str, data: &str| {
            if !skip.contains(&p) {
                files.insert(p.to_owned(), data.as_bytes().to_vec());
            }
        };
        add("textures/block/stone.png", "x");
        add("textures/font/ascii.png", "x");
        add("models/block/stone.json", "{}");
        add("blockstates/stone.json", "{}");
        add("items/stick.json", "{}");
        add("lang/en_us.json", "{}");
        add(
            "sounds.json",
            r#"{"a":{"sounds":["block/stone/step1",{"name":"block/stone/step2","type":"sound"},{"name":"x","type":"event"}]}}"#,
        );
        add("sounds/block/stone/step1.ogg", "o");
        add("sounds/block/stone/step2.ogg", "o");
        add(
            "font/default.json",
            r#"{"providers":[{"type":"bitmap","file":"minecraft:font/ascii.png"},{"type":"reference","id":"minecraft:include/x"}]}"#,
        );
        add("font/include/x.json", r#"{"providers":[]}"#);
        (files.keys().cloned().collect(), files)
    }

    fn run(skip: &[&str]) -> VanillaReport {
        let (paths, files) = layer(skip);
        check_vanilla(&paths, &|p| files.get(p).cloned())
    }

    #[test]
    fn a_complete_layer_passes_and_counts() {
        let r = run(&[]);
        assert!(r.complete(), "{:?}", r.problems);
        assert_eq!((r.sound_files, r.font_pages), (2, 1));
        assert!(r.summary().starts_with("vanilla layer complete"));
    }

    #[test]
    fn missing_sounds_font_and_directories_are_named() {
        let r = run(&["sounds/block/stone/step2.ogg"]);
        assert!(r.problems[0].contains("1 sound files") && r.problems[0].contains("step2.ogg"));
        assert!(run(&["font/default.json"]).problems[0].contains("font/default.json"));
        assert!(run(&["textures/font/ascii.png"]).problems[0].contains("font pages"));
        assert!(run(&["models/block/stone.json"]).problems[0].contains("no models/"));
        assert!(run(&["sounds.json"]).problems[0].contains("sounds.json"));
        assert!(run(&["lang/en_us.json"]).problems[0].contains("en_us"));
    }

    #[test]
    fn picked_paths_land_under_the_vanilla_layer() {
        let dest = |p| vanilla_destination(p);
        assert_eq!(
            dest("vanilla/assets/minecraft/sounds.json").as_deref(),
            Some("assets/vanilla/assets/minecraft/sounds.json")
        );
        assert_eq!(
            dest("minecraft-assets-26.3/assets/minecraft/lang/en_us.json").as_deref(),
            Some("assets/vanilla/assets/minecraft/lang/en_us.json")
        );
        assert_eq!(dest("assets/minecraft/").as_deref(), None);
        assert_eq!(dest("data/minecraft/worldgen/x.json"), None);
        assert_eq!(
            dest("pick\\assets\\minecraft\\sounds.json").as_deref(),
            Some("assets/vanilla/assets/minecraft/sounds.json")
        );
    }
}
