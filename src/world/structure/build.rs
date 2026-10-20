//! Drawing a code-made piece (dungeons, mineshafts, strongholds) straight
//! in world space.

use glam::{DVec3, IVec3};

use super::{MobSpawn, Piece};
use crate::world::{block::BlockState, blocks};

#[derive(Debug, Default)]
pub struct Build {
    piece: Piece,
}

impl Build {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn set(&mut self, at: IVec3, block: BlockState) {
        self.piece.blocks.push((at, block));
    }

    /// Fills the box between two corners.
    pub fn fill(&mut self, a: IVec3, b: IVec3, block: BlockState) {
        let (lo, hi) = (a.min(b), a.max(b));
        for y in lo.y..=hi.y {
            for z in lo.z..=hi.z {
                for x in lo.x..=hi.x {
                    self.set(IVec3::new(x, y, z), block);
                }
            }
        }
    }

    /// Fills the box with `pick`'s choice at each spot (mossy and cracked
    /// stone bricks mixed in).
    pub fn fill_with(&mut self, a: IVec3, b: IVec3, mut pick: impl FnMut(IVec3) -> BlockState) {
        let (lo, hi) = (a.min(b), a.max(b));
        for y in lo.y..=hi.y {
            for z in lo.z..=hi.z {
                for x in lo.x..=hi.x {
                    let at = IVec3::new(x, y, z);
                    let block = pick(at);
                    self.set(at, block);
                }
            }
        }
    }

    /// The box's shell of `pick`'s blocks with air inside.
    pub fn hollow(&mut self, a: IVec3, b: IVec3, pick: impl FnMut(IVec3) -> BlockState) {
        let (lo, hi) = (a.min(b), a.max(b));
        self.fill_with(lo, hi, pick);
        if hi.cmpgt(lo + IVec3::ONE).all() {
            self.fill(lo + IVec3::ONE, hi - IVec3::ONE, BlockState::AIR);
        }
    }

    pub fn chest(&mut self, at: IVec3, block: BlockState, table: &str) {
        self.set(at, block);
        self.piece.loot.push((at, table.to_owned()));
    }

    pub fn spawner(&mut self, at: IVec3, mob: &str) {
        self.set(at, BlockState::of(blocks::SPAWNER));
        self.piece.spawners.push((at, mob.to_owned()));
    }

    pub fn mob(&mut self, at: DVec3, mob: &str, tag: Option<&str>) {
        self.piece.mobs.push(MobSpawn {
            mob: mob.to_owned(),
            at,
            tag: tag.map(str::to_owned),
        });
    }

    pub fn finish(mut self) -> Piece {
        self.piece.compute_bounds();
        self.piece
    }
}
