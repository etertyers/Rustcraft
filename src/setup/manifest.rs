//! What the game downloads and from where. The packs and mods are pinned in
//! `assets/data/downloads.ron` (also read by `scripts/check-downloads.sh`);
//! Mojang's side is a version name.

use serde::Deserialize;
use std::sync::OnceLock;

/// The Mojang version whose files Rustcraft uses (branch 26.3 of the
/// minecraft-assets mirror the fetch script reads).
pub const VANILLA_VERSION: &str = "26.3";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
pub enum Kind {
    Pack,
    Mod,
}

/// The styles that need an entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
pub enum StyleTag {
    Barebones,
    Realistic,
}

/// One pinned file on Modrinth.
#[derive(Debug, Clone, Deserialize)]
pub struct Extra {
    pub name: String,
    pub kind: Kind,
    pub project: String,
    pub version: String,
    pub file: String,
    pub size: u64,
    pub sha512: String,
    pub license: String,
    pub author: String,
    pub page: String,
    pub styles: Vec<StyleTag>,
}

impl Extra {
    /// The folder under `downloads/` this file goes in.
    pub fn folder(&self) -> &'static str {
        match self.kind {
            Kind::Pack => "resourcepacks",
            Kind::Mod => "mods",
        }
    }

    /// `resourcepacks/<file>` or `mods/<file>`, under `downloads/`.
    pub fn rel_path(&self) -> String {
        format!("{}/{}", self.folder(), self.file)
    }
}

#[derive(Deserialize)]
struct List {
    entries: Vec<Extra>,
}

/// Parses the text of a `downloads.ron`.
pub fn parse(text: &str) -> Result<Vec<Extra>, String> {
    ron::from_str::<List>(text)
        .map(|list| list.entries)
        .map_err(|err| err.to_string())
}

/// The 5 packs and 9 jars, in the order of `downloads.ron`.
pub fn extras() -> &'static [Extra] {
    static EXTRAS: OnceLock<Vec<Extra>> = OnceLock::new();
    EXTRAS.get_or_init(|| {
        parse(include_str!("../../assets/data/downloads.ron"))
            .expect("assets/data/downloads.ron is valid (a test checks it)")
    })
}

/// The servers to ask. Tests point these at a local server.
#[derive(Debug, Clone)]
pub struct Endpoints {
    /// Mojang's `version_manifest_v2.json`.
    pub version_manifest: String,
    /// Where asset objects live: `<base>/<first two hash chars>/<hash>`.
    pub objects_base: String,
    /// Modrinth's API root, `https://api.modrinth.com`.
    pub modrinth_api: String,
    /// Modrinth's file CDN, used only if the API can't be asked.
    pub modrinth_cdn: String,
    pub user_agent: String,
}

impl Endpoints {
    pub fn official() -> Self {
        Self {
            version_manifest: "https://piston-meta.mojang.com/mc/game/version_manifest_v2.json"
                .into(),
            objects_base: "https://resources.download.minecraft.net".into(),
            modrinth_api: "https://api.modrinth.com".into(),
            modrinth_cdn: "https://cdn.modrinth.com".into(),
            user_agent: format!(
                "Rustcraft/{} (github.com/etertyers/Rustcraft)",
                env!("CARGO_PKG_VERSION")
            ),
        }
    }

    /// Everything on one local server (tests).
    pub fn local(base: &str) -> Self {
        Self {
            version_manifest: format!("{base}/mc/game/version_manifest_v2.json"),
            objects_base: format!("{base}/objects"),
            modrinth_api: format!("{base}/modrinth"),
            modrinth_cdn: format!("{base}/cdn"),
            user_agent: "Rustcraft-test".into(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_pinned_list_has_five_packs_and_nine_jars() {
        let all = extras();
        assert_eq!(all.iter().filter(|e| e.kind == Kind::Pack).count(), 5);
        assert_eq!(all.iter().filter(|e| e.kind == Kind::Mod).count(), 9);
        for e in all {
            assert_eq!(e.sha512.len(), 128, "{}", e.name);
            assert!(
                e.size > 0 && !e.license.is_empty() && e.page.starts_with("https://modrinth.com/")
            );
        }
        // Every mod serves both styles; Patrix only Realistic.
        assert!(
            all.iter()
                .filter(|e| e.kind == Kind::Mod)
                .all(|e| e.styles.len() == 2)
        );
        let patrix = all.iter().find(|e| e.name == "Patrix 32x").unwrap();
        assert_eq!(patrix.styles, [StyleTag::Realistic]);
    }

    #[test]
    fn the_user_agent_names_the_game() {
        assert!(Endpoints::official().user_agent.starts_with("Rustcraft/1."));
    }
}
