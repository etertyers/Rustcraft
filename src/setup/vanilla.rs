//! Mojang's side: the launcher manifest, the version's JSON, `client.jar`
//! and the asset index, and which of their files the game keeps.
//!
//! The result is the layout `scripts/fetch-vanilla-assets.sh` makes under
//! `vanilla/assets/minecraft/`: the jar's textures, models, blockstates,
//! items, `lang/en_us.json` and font definitions, then the asset index's own
//! files under the same paths (the jar holds 1×1 panorama stubs and an empty
//! `unifont.json`; the index has the real ones) plus `sounds.json` and every
//! sound.

use super::download::{Check, Http};
use super::manifest::{Endpoints, VANILLA_VERSION};
use super::{Result, SetupError};
use serde::Deserialize;
use std::collections::{HashMap, HashSet};
use std::path::Path;

/// A file Mojang lists with its sha1.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Blob {
    pub url: String,
    pub sha1: String,
    pub size: u64,
}

/// A file of the asset index kept in the layer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Object {
    /// Under `assets/minecraft/`, like `sounds/ambient/cave/cave1.ogg`.
    pub rel: String,
    pub hash: String,
    pub size: u64,
}

/// What a vanilla download is made of, measured from Mojang's own lists.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Plan {
    pub version: String,
    pub client: Blob,
    pub objects: Vec<Object>,
}

impl Plan {
    /// Bytes of the sound files.
    pub fn sound_bytes(&self) -> u64 {
        self.objects
            .iter()
            .filter(|o| o.rel.starts_with("sounds/"))
            .map(|o| o.size)
            .sum()
    }

    /// Bytes of every object (sounds and the few other files).
    pub fn object_bytes(&self) -> u64 {
        self.objects.iter().map(|o| o.size).sum()
    }

    /// What the download transfers: `client.jar` and the objects.
    pub fn download_bytes(&self) -> u64 {
        self.client.size + self.object_bytes()
    }
}

/// Whether `rel` (under `assets/minecraft/`) is a path the layer keeps:
/// what the fetch script's sparse checkout keeps.
pub fn keep_path(rel: &str) -> bool {
    if rel.contains("..") || rel.starts_with('/') {
        return false;
    }
    if ["textures/", "models/", "blockstates/", "items/", "sounds/"]
        .iter()
        .any(|p| rel.starts_with(p))
    {
        return true;
    }
    if rel == "lang/en_us.json" || rel == "sounds.json" || rel.starts_with("font/include/") {
        return true;
    }
    rel.strip_prefix("font/")
        .is_some_and(|name| !name.contains('/') && name.ends_with(".json"))
}

#[derive(Deserialize)]
struct VersionManifest {
    versions: Vec<ManifestVersion>,
}

#[derive(Deserialize)]
struct ManifestVersion {
    id: String,
    url: String,
    sha1: String,
}

#[derive(Deserialize)]
struct VersionJson {
    downloads: VersionDownloads,
    #[serde(rename = "assetIndex")]
    asset_index: IndexRef,
}

#[derive(Deserialize)]
struct VersionDownloads {
    client: Ref,
}

#[derive(Deserialize)]
struct Ref {
    sha1: String,
    size: u64,
    url: String,
}

#[derive(Deserialize)]
struct IndexRef {
    sha1: String,
    size: u64,
    url: String,
}

#[derive(Deserialize)]
struct AssetIndex {
    objects: HashMap<String, IndexObject>,
}

#[derive(Deserialize)]
struct IndexObject {
    hash: String,
    size: u64,
}

fn parse<T: for<'de> Deserialize<'de>>(bytes: &[u8], what: &str) -> Result<T> {
    serde_json::from_slice(bytes).map_err(|err| {
        SetupError::Format(format!("Mojang's {what} isn't what was expected: {err}"))
    })
}

/// Asks Mojang for the version's files: the manifest, the version JSON
/// (checked against the manifest's sha1), the asset index (checked against
/// the version JSON's sha1). A manifest without [`VANILLA_VERSION`] is an
/// error, never a nearby version.
pub fn resolve(http: &Http, ep: &Endpoints) -> Result<Plan> {
    const SMALL: u64 = 64 << 20;
    let manifest: VersionManifest = parse(
        &http.bytes(&ep.version_manifest, None, SMALL)?,
        "version list",
    )?;
    let entry = manifest
        .versions
        .into_iter()
        .find(|v| v.id == VANILLA_VERSION)
        .ok_or_else(|| SetupError::NoVersion(VANILLA_VERSION.into()))?;
    let version: VersionJson = parse(
        &http.bytes(&entry.url, Some(Check::sha1(&entry.sha1)), SMALL)?,
        "version file",
    )?;
    let index: AssetIndex = parse(
        &http.bytes(
            &version.asset_index.url,
            Some(Check::sha1(&version.asset_index.sha1)),
            SMALL,
        )?,
        "asset index",
    )?;
    let _ = version.asset_index.size;
    let mut objects: Vec<Object> = index
        .objects
        .into_iter()
        .filter_map(|(name, o)| {
            let rel = name.strip_prefix("minecraft/")?;
            (keep_path(rel) && o.hash.len() >= 2 && o.hash.bytes().all(|b| b.is_ascii_hexdigit()))
                .then(|| Object {
                    rel: rel.to_owned(),
                    hash: o.hash,
                    size: o.size,
                })
        })
        .collect();
    objects.sort_by(|a, b| a.rel.cmp(&b.rel));
    Ok(Plan {
        version: entry.id,
        client: Blob {
            url: version.downloads.client.url,
            sha1: version.downloads.client.sha1,
            size: version.downloads.client.size,
        },
        objects,
    })
}

/// Where an asset object is: `<base>/<first two hash chars>/<hash>`.
pub fn object_url(ep: &Endpoints, hash: &str) -> String {
    format!("{}/{}/{}", ep.objects_base, &hash[..2], hash)
}

/// Unpacks the kept paths of `client.jar` into `mc` (the
/// `assets/minecraft` folder), except those in `skip` (the asset index
/// has its own copy). Returns how many files it wrote.
pub fn extract_client_jar(jar: &Path, mc: &Path, skip: &HashSet<&str>) -> Result<usize> {
    let file = std::fs::File::open(jar)?;
    let mut archive = zip::ZipArchive::new(file)
        .map_err(|err| SetupError::Format(format!("client.jar can't be opened: {err}")))?;
    let mut written = 0;
    for i in 0..archive.len() {
        let mut entry = archive
            .by_index(i)
            .map_err(|err| SetupError::Format(format!("client.jar is damaged: {err}")))?;
        if entry.is_dir() {
            continue;
        }
        let Some(name) = entry.enclosed_name() else {
            continue;
        };
        let Some(rel) = name
            .to_str()
            .and_then(|n| n.strip_prefix("assets/minecraft/"))
            .filter(|rel| keep_path(rel) && !skip.contains(rel))
        else {
            continue;
        };
        let dest = mc.join(rel);
        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut out = std::fs::File::create(&dest)?;
        std::io::copy(&mut entry, &mut out)?;
        written += 1;
    }
    Ok(written)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_what_the_fetch_script_keeps() {
        for kept in [
            "textures/block/stone.png",
            "models/item/stick.json",
            "blockstates/oak_log.json",
            "items/stick.json",
            "sounds/ambient/cave/cave1.ogg",
            "sounds.json",
            "lang/en_us.json",
            "font/default.json",
            "font/include/unifont.json",
        ] {
            assert!(keep_path(kept), "{kept}");
        }
        for dropped in [
            "lang/de_de.json",
            "font/unifont.zip",
            "shaders/core/panorama.fsh",
            "atlases/blocks.json",
            "textures/../../x",
            "/etc/passwd",
        ] {
            assert!(!keep_path(dropped), "{dropped}");
        }
    }

    #[test]
    fn object_urls_use_the_first_two_hash_characters() {
        let ep = Endpoints::official();
        assert_eq!(
            object_url(&ep, "ab12cd"),
            "https://resources.download.minecraft.net/ab/ab12cd"
        );
    }
}
