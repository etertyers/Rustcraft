//! The structure capture tool (creative only): F8 and F9 mark two
//! corners on the blocks under the crosshair, F10 saves everything between
//! them (blocks, air included, and the mobs inside) as a template in
//! `assets/structures/captured/`. Copying that file over one of the
//! shipped templates (keeping its `jigsaws`, which say where streets join)
//! replaces that piece in new chunks.

use std::path::PathBuf;

use anyhow::{Result, bail};
use glam::IVec3;

use super::Game;
use crate::world::structure::template::{Sketch, write};

/// The biggest region one capture may hold, each way.
const MAX_SIZE: i32 = 64;
/// Ticks a message stays on screen.
const MESSAGE_TICKS: u32 = 100;

impl Game {
    /// Marks corner `which` (0 or 1) at the block under the crosshair.
    pub(super) fn mark_corner(&mut self, which: usize) {
        if !self.player.mode.has_every_item() {
            return;
        }
        let Some(hit) = self.player.target.as_ref().map(|hit| hit.block) else {
            self.player.message = Some(("Look at a block to mark a corner".into(), MESSAGE_TICKS));
            return;
        };
        self.player.capture[which] = Some(hit);
        self.player.message = Some((
            format!("Corner {} at {} {} {}", which + 1, hit.x, hit.y, hit.z),
            MESSAGE_TICKS,
        ));
    }

    /// Saves the marked region, telling the player where.
    pub(super) fn save_capture(&mut self) {
        if !self.player.mode.has_every_item() {
            return;
        }
        let text = match self.capture_to(&crate::assets::dir().join("structures/captured")) {
            Ok(path) => format!("Saved {}", path.display()),
            Err(err) => format!("Couldn't save: {err}"),
        };
        log::info!("{text}");
        self.player.message = Some((text, MESSAGE_TICKS));
    }

    /// The marked region as a template, its corner at the origin.
    pub(super) fn capture_sketch(&self) -> Result<Sketch> {
        let [Some(a), Some(b)] = self.player.capture else {
            bail!("mark both corners first (F8, F9)");
        };
        let (lo, hi) = (a.min(b), a.max(b));
        let size = hi - lo + IVec3::ONE;
        if size.max_element() > MAX_SIZE {
            bail!(
                "{} × {} × {} is bigger than {MAX_SIZE} a side",
                size.x,
                size.y,
                size.z
            );
        }
        let world = self.chunks.world();
        let mut sketch = Sketch::new(size.to_array());
        for y in lo.y..=hi.y {
            for z in lo.z..=hi.z {
                for x in lo.x..=hi.x {
                    let Some(block) = world.block(x, y, z) else {
                        bail!("part of the region isn't loaded");
                    };
                    sketch.set(x - lo.x, y - lo.y, z - lo.z, block);
                }
            }
        }
        for mob in &self.mobs {
            let feet = mob.feet();
            let cell = feet.floor().as_ivec3();
            if mob.is_dead() || cell.cmplt(lo).any() || cell.cmpgt(hi).any() {
                continue;
            }
            let at = feet - lo.as_dvec3();
            let name = &self.mob_defs.get(mob.kind).name;
            let job = mob
                .data
                .villager
                .as_deref()
                .and_then(|v| v.profession)
                .map(|p| p.name());
            sketch.mob(at.x, at.y, at.z, name, job);
        }
        Ok(sketch)
    }

    fn capture_to(&self, dir: &std::path::Path) -> Result<PathBuf> {
        let template = self.capture_sketch()?.template();
        let path = (1..)
            .map(|n| dir.join(format!("capture_{n}.ron")))
            .find(|path| !path.exists())
            .expect("a free name");
        write(&template, &path)?;
        Ok(path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        game::tests::flat_game,
        player::{mode::GameMode, raycast::Hit},
        world::{block::BlockState, blocks, structure::template::Template},
    };

    fn aim(game: &mut Game, at: IVec3) {
        game.player.target = Some(Hit {
            block: at,
            face: None,
            point: at.as_dvec3(),
        });
    }

    #[test]
    fn a_captured_region_saves_and_loads_as_a_template() {
        let mut game = flat_game(GameMode::Creative);
        game.chunks.set_blocks(&[
            (IVec3::new(3, 64, 3), BlockState::of(blocks::BOOKSHELF)),
            (IVec3::new(4, 65, 5), BlockState::of(blocks::GLASS)),
        ]);
        aim(&mut game, IVec3::new(2, 63, 2));
        game.mark_corner(0);
        aim(&mut game, IVec3::new(5, 66, 5));
        game.mark_corner(1);
        let dir = crate::world::save::TempDir::new("capture");
        let path = game.capture_to(dir.path()).unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        let template: Template = ron::from_str(&text).unwrap();
        template.validate().unwrap();
        assert_eq!(template.size, [4, 4, 4]);
        let piece = template.place(IVec3::ZERO, 0);
        let at = |p: IVec3| piece.blocks.iter().find(|(q, _)| *q == p).map(|(_, b)| *b);
        assert_eq!(
            at(IVec3::new(1, 1, 1)),
            Some(BlockState::of(blocks::BOOKSHELF))
        );
        assert_eq!(at(IVec3::new(2, 2, 3)), Some(BlockState::of(blocks::GLASS)));
        assert_eq!(
            at(IVec3::new(0, 0, 0)),
            Some(BlockState::of(blocks::GRASS_BLOCK))
        );
        // A second capture doesn't overwrite the first.
        let again = game.capture_to(dir.path()).unwrap();
        assert_ne!(again, path);
    }

    #[test]
    fn only_creative_players_capture() {
        let mut game = flat_game(GameMode::Survival);
        aim(&mut game, IVec3::new(2, 63, 2));
        game.mark_corner(0);
        assert_eq!(game.player.capture, [None, None]);
    }
}
