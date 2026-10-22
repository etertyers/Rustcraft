//! The player's settings (1.0.0's `GameSettings`), kept in `options.ron`
//! beside the saves folder: render distance, biome blend, field of view,
//! mouse sensitivity, volume, vsync and the selected resource packs.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use log::warn;
use serde::{Deserialize, Serialize};

use crate::{config, level};

/// Render distance range in chunks: 1.0.0's Tiny (2) to Far (16).
pub const RENDER_DISTANCES: std::ops::RangeInclusive<i32> = 2..=16;
/// Biome blend radius range: off to modern's 15 × 15.
pub const BIOME_BLENDS: std::ops::RangeInclusive<i32> = 0..=7;
/// Field of view range in degrees: 1.0.0's slider ran from 30 to "Quake
/// Pro" at 110, with 70 as "Normal".
pub const FOVS: std::ops::RangeInclusive<f32> = 30.0..=110.0;
/// Mouse sensitivity range, as a fraction of the default (1.0.0's 0–200%).
pub const SENSITIVITIES: std::ops::RangeInclusive<f32> = 0.0..=2.0;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Options {
    /// Chunks drawn in each direction.
    pub render_distance: i32,
    /// Columns each way that grass and foliage colors average over (0 is
    /// off).
    pub biome_blend: i32,
    /// Vertical field of view in degrees.
    pub fov: f32,
    /// Mouse look speed, 1 for the default.
    pub sensitivity: f32,
    /// Sound volume 0–1: the master volume of sounds and music.
    pub volume: f32,
    pub vsync: bool,
    /// Whether the view and hand shake with each step.
    pub view_bobbing: bool,
    /// Selected resource packs (file or folder names in `resourcepacks/`),
    /// highest priority first.
    pub resource_packs: Vec<String>,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            render_distance: config::RENDER_DISTANCE,
            biome_blend: config::BIOME_BLEND,
            fov: config::FOV_Y_DEGREES,
            sensitivity: 1.0,
            volume: 1.0,
            vsync: true,
            view_bobbing: true,
            resource_packs: Vec::new(),
        }
    }
}

impl Options {
    /// `options.ron` next to the saves folder.
    pub fn path() -> PathBuf {
        level::saves_dir().with_file_name("options.ron")
    }

    /// Reads the options, falling back to the defaults if the file is
    /// missing or can't be read.
    pub fn load(path: &Path) -> Self {
        let text = match std::fs::read_to_string(path) {
            Ok(text) => text,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Self::default(),
            Err(err) => {
                warn!("reading {}: {err}; using default options", path.display());
                return Self::default();
            }
        };
        match ron::from_str::<Self>(&text) {
            Ok(options) => options.clamped(),
            Err(err) => {
                warn!("parsing {}: {err}; using default options", path.display());
                Self::default()
            }
        }
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        let text = ron::ser::to_string_pretty(self, ron::ser::PrettyConfig::default())?;
        if let Some(dir) = path.parent().filter(|dir| !dir.as_os_str().is_empty()) {
            std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
        }
        std::fs::write(path, text).with_context(|| format!("writing {}", path.display()))
    }

    /// Every value inside its slider's range.
    pub fn clamped(self) -> Self {
        Self {
            render_distance: self
                .render_distance
                .clamp(*RENDER_DISTANCES.start(), *RENDER_DISTANCES.end()),
            biome_blend: self
                .biome_blend
                .clamp(*BIOME_BLENDS.start(), *BIOME_BLENDS.end()),
            fov: self.fov.clamp(*FOVS.start(), *FOVS.end()),
            sensitivity: self
                .sensitivity
                .clamp(*SENSITIVITIES.start(), *SENSITIVITIES.end()),
            volume: self.volume.clamp(0.0, 1.0),
            vsync: self.vsync,
            view_bobbing: self.view_bobbing,
            resource_packs: self.resource_packs,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::save::TempDir;

    #[test]
    fn options_round_trip_and_bad_files_fall_back_to_defaults() {
        let dir = TempDir::new("options");
        let path = dir.path().join("options.ron");
        assert_eq!(Options::load(&path), Options::default());
        let options = Options {
            render_distance: 12,
            biome_blend: 5,
            fov: 90.0,
            sensitivity: 1.5,
            volume: 0.25,
            vsync: false,
            view_bobbing: false,
            resource_packs: vec!["High.zip".to_owned(), "Low".to_owned()],
        };
        options.save(&path).unwrap();
        assert_eq!(Options::load(&path), options);
        // Older files without a setting keep its default; values out of
        // range are pulled back in.
        std::fs::write(&path, "(fov: 500.0)").unwrap();
        let loaded = Options::load(&path);
        assert_eq!(loaded.fov, 110.0);
        assert_eq!(loaded.biome_blend, config::BIOME_BLEND);
        std::fs::write(&path, "(biome_blend: 20)").unwrap();
        assert_eq!(Options::load(&path).biome_blend, 7);
        assert_eq!(loaded.render_distance, config::RENDER_DISTANCE);
        std::fs::write(&path, "not ron").unwrap();
        assert_eq!(Options::load(&path), Options::default());
    }
}
