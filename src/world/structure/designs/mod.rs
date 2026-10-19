//! The structure templates Rustcraft ships with, drawn in code and
//! written to `assets/structures` by `cargo run -- --write-structures`.
//! The game only reads the files: any of them can be edited by hand or
//! replaced with one captured in the game.

mod ruins;
mod village;

use std::path::Path;

use anyhow::Result;

use super::template::{Sketch, Template, write};
use crate::world::{Face, block::BlockState};

/// A block state by its text, e.g. `oak_stairs[facing=east]`.
pub(crate) fn b(text: &str) -> BlockState {
    BlockState::parse(text).unwrap_or_else(|err| panic!("{text}: {err}"))
}

pub(crate) fn facing_name(face: Face) -> &'static str {
    super::template::face_name(face)
}

/// `name` stairs facing `facing`, upside down if `top`.
pub(crate) fn stairs(name: &str, facing: Face, top: bool) -> BlockState {
    b(&format!(
        "{name}[facing={},half={}]",
        facing_name(facing),
        if top { "top" } else { "bottom" }
    ))
}

impl Sketch {
    /// A door in the wall at (x, z), its bottom at `y`.
    pub(crate) fn door(&mut self, x: i32, y: i32, z: i32, name: &str, facing: Face) {
        let f = facing_name(facing);
        self.set(x, y, z, b(&format!("{name}[facing={f},half=lower]")));
        self.set(x, y + 1, z, b(&format!("{name}[facing={f},half=upper]")));
    }

    /// A bed whose foot is at (x, y, z), its head toward `facing`.
    pub(crate) fn bed(&mut self, x: i32, y: i32, z: i32, facing: Face) {
        let f = facing_name(facing);
        let [dx, _, dz] = facing.normal();
        self.set(x, y, z, b(&format!("red_bed[facing={f},part=foot]")));
        self.set(
            x + dx,
            y,
            z + dz,
            b(&format!("red_bed[facing={f},part=head]")),
        );
    }

    /// A torch on the wall behind it, pointing `facing`.
    pub(crate) fn wall_torch(&mut self, x: i32, y: i32, z: i32, facing: Face) {
        self.set(
            x,
            y,
            z,
            b(&format!("wall_torch[facing={}]", facing_name(facing))),
        );
    }
}

/// Every template, by its path under `assets/structures` (without `.ron`).
pub fn all() -> Vec<(String, Template)> {
    let mut all = village::all();
    all.extend(ruins::all());
    all
}

/// Writes every template under `dir`, returning how many.
pub fn write_all(dir: &Path) -> Result<usize> {
    let all = all();
    for (name, template) in &all {
        write(template, &dir.join(format!("{name}.ron")))?;
    }
    Ok(all.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_design_is_a_valid_template() {
        for (name, template) in all() {
            template
                .validate()
                .unwrap_or_else(|err| panic!("{name}: {err}"));
            // What the ground layer joins to has to be on the ground layer.
            for jigsaw in &template.jigsaws {
                assert_eq!(jigsaw.at[1], 0, "{name}");
            }
        }
    }

    #[test]
    fn the_shipped_files_load_and_every_pool_a_connector_names_exists() {
        let registry =
            super::super::template::Registry::load(&crate::assets::dir().join("structures"))
                .unwrap();
        assert!(
            registry.templates.len() >= 70,
            "{}",
            registry.templates.len()
        );
        for (name, template) in &registry.templates {
            for jigsaw in &template.jigsaws {
                assert!(
                    !registry.pool(&jigsaw.pool).is_empty(),
                    "{name} joins the empty pool {}",
                    jigsaw.pool
                );
            }
        }
        for style in super::super::village::STYLES {
            for pool in ["centers", "streets", "houses"] {
                assert!(!registry.pool(&format!("village/{style}/{pool}")).is_empty());
            }
        }
        for pool in [
            "desert_temple",
            "jungle_temple",
            "shipwreck",
            "shipwreck_beached",
            "ruined_portal",
        ] {
            assert!(!registry.pool(pool).is_empty(), "{pool}");
        }
    }
}
