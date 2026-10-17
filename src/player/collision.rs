//! Axis-aligned boxes and Minecraft-style collision against the world's blocks.

use glam::DVec3;

use crate::world::{
    World,
    block::{BlockRegistry, Fluid},
    chunk::ChunkPos,
};

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Aabb {
    pub min: DVec3,
    pub max: DVec3,
}

impl Aabb {
    pub const fn new(min: DVec3, max: DVec3) -> Self {
        Self { min, max }
    }

    /// The unit cube of block (x, y, z).
    pub fn block(x: i32, y: i32, z: i32) -> Self {
        let min = DVec3::new(x as f64, y as f64, z as f64);
        Self::new(min, min + DVec3::ONE)
    }

    pub fn offset(self, d: DVec3) -> Self {
        Self::new(self.min + d, self.max + d)
    }

    /// Grows the box toward `d` so it covers a move by `d` (Minecraft's `addCoord`).
    pub fn stretch(self, d: DVec3) -> Self {
        Self::new(self.min + d.min(DVec3::ZERO), self.max + d.max(DVec3::ZERO))
    }

    /// Grows the box by `by` on every side.
    pub fn grow(self, by: f64) -> Self {
        Self::new(self.min - DVec3::splat(by), self.max + DVec3::splat(by))
    }

    /// Distance along the ray from `origin` in direction `dir` (a unit
    /// vector) where it enters the box, if within `reach`.
    pub fn ray_hit(&self, origin: DVec3, dir: DVec3, reach: f64) -> Option<f64> {
        let (mut near, mut far) = (0.0f64, reach);
        for axis in 0..3 {
            if dir[axis].abs() < 1e-12 {
                if origin[axis] < self.min[axis] || origin[axis] > self.max[axis] {
                    return None;
                }
                continue;
            }
            let t1 = (self.min[axis] - origin[axis]) / dir[axis];
            let t2 = (self.max[axis] - origin[axis]) / dir[axis];
            near = near.max(t1.min(t2));
            far = far.min(t1.max(t2));
            if near > far {
                return None;
            }
        }
        Some(near)
    }

    /// Overlap with positive volume; touching faces don't count.
    pub fn intersects(&self, other: &Aabb) -> bool {
        self.min.cmplt(other.max).all() && self.max.cmpgt(other.min).all()
    }

    /// Clips a move of `other` by `d` along `axis` (0 = X, 1 = Y, 2 = Z) so it
    /// stops at this box. Only boxes overlapping on the other two axes block
    /// it, and a box `other` already overlaps never does.
    pub fn clip(&self, other: &Aabb, axis: usize, d: f64) -> f64 {
        let overlaps_across = (0..3)
            .filter(|&a| a != axis)
            .all(|a| other.max[a] > self.min[a] && other.min[a] < self.max[a]);
        if !overlaps_across {
            return d;
        }
        if d > 0.0 && other.max[axis] <= self.min[axis] {
            d.min(self.min[axis] - other.max[axis])
        } else if d < 0.0 && other.min[axis] >= self.max[axis] {
            d.max(self.max[axis] - other.min[axis])
        } else {
            d
        }
    }
}

/// What the player collides with.
pub trait Collider {
    /// Appends the collision boxes that may touch `region`.
    fn boxes(&self, region: &Aabb, out: &mut Vec<Aabb>);

    /// Slipperiness of the block at (x, y, z), 0.6 for most blocks.
    fn slipperiness(&self, x: i32, y: i32, z: i32) -> f32;

    /// The liquid `region` reaches into, if any (water first). A liquid
    /// counts once the region's top reaches its surface, as in 1.0.0's
    /// `handleMaterialAcceleration`.
    fn liquid(&self, _region: &Aabb) -> Option<Fluid> {
        None
    }
}

/// The loaded world's solid blocks. Unloaded chunks are walls, so nothing
/// walks or falls into them.
pub struct WorldCollider<'a> {
    pub world: &'a World,
    pub blocks: &'a BlockRegistry,
}

impl WorldCollider<'_> {
    /// Whether every chunk within a block of `region` is loaded.
    pub fn is_loaded(&self, region: &Aabb) -> bool {
        let min = region.min.floor().as_ivec3() - 1;
        let max = region.max.floor().as_ivec3() + 1;
        let (low, high) = (
            ChunkPos::of_block(min.x, min.z),
            ChunkPos::of_block(max.x, max.z),
        );
        (low.z..=high.z)
            .all(|z| (low.x..=high.x).all(|x| self.world.get(ChunkPos::new(x, z)).is_some()))
    }
}

impl Collider for WorldCollider<'_> {
    fn boxes(&self, region: &Aabb, out: &mut Vec<Aabb>) {
        let min = region.min.floor().as_ivec3();
        let max = region.max.floor().as_ivec3();
        for y in min.y..=max.y {
            for z in min.z..=max.z {
                for x in min.x..=max.x {
                    let solid = match self.world.block(x, y, z) {
                        None => true,
                        Some(block) => self.blocks.get(block.id()).is_some_and(|def| def.solid),
                    };
                    if solid {
                        out.push(Aabb::block(x, y, z));
                    }
                }
            }
        }
    }

    fn slipperiness(&self, x: i32, y: i32, z: i32) -> f32 {
        self.world
            .block(x, y, z)
            .and_then(|block| self.blocks.get(block.id()))
            .map_or(0.6, |def| def.slipperiness)
    }

    fn liquid(&self, region: &Aabb) -> Option<Fluid> {
        let min = region.min.floor().as_ivec3();
        let max = region.max.floor().as_ivec3();
        let mut found = None;
        for y in min.y..=max.y {
            for z in min.z..=max.z {
                for x in min.x..=max.x {
                    let Some(block) = self.world.block(x, y, z) else {
                        continue;
                    };
                    let Some(fluid) = self.blocks.get(block.id()).and_then(|def| def.fluid) else {
                        continue;
                    };
                    // 1.0.0's getFluidHeightPercent: level 0 (a source) fills 8/9.
                    let level = (block.data() & 7) as f64;
                    let surface = (y + 1) as f64 - (level + 1.0) / 9.0;
                    if region.max.y >= surface {
                        if fluid == Fluid::Water {
                            return Some(fluid);
                        }
                        found = Some(fluid);
                    }
                }
            }
        }
        found
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::{block::BlockState, chunk::Chunk};

    fn player_at(x: f64, y: f64, z: f64) -> Aabb {
        Aabb::new(
            DVec3::new(x - 0.3, y, z - 0.3),
            DVec3::new(x + 0.3, y + 1.8, z + 0.3),
        )
    }

    #[test]
    fn falling_stops_on_top_of_a_block() {
        let ground = Aabb::block(0, 63, 0);
        let player = player_at(0.5, 64.5, 0.5);
        assert_eq!(ground.clip(&player, 1, -2.0), -0.5);
        // Not above it: no effect.
        assert_eq!(ground.clip(&player_at(5.0, 64.5, 0.5), 1, -2.0), -2.0);
        // Moving away: no effect.
        assert_eq!(ground.clip(&player, 1, 1.0), 1.0);
    }

    #[test]
    fn walking_stops_at_a_wall_and_overlap_never_blocks() {
        let wall = Aabb::block(2, 64, 0);
        let player = player_at(1.0, 64.0, 0.5);
        assert!((wall.clip(&player, 0, 1.0) - 0.7).abs() < 1e-12);
        assert_eq!(wall.clip(&player, 0, -1.0), -1.0);
        let inside = player_at(2.5, 64.0, 0.5);
        assert_eq!(wall.clip(&inside, 0, 1.0), 1.0);
    }

    #[test]
    fn touching_boxes_do_not_intersect() {
        let a = Aabb::block(0, 0, 0);
        assert!(!a.intersects(&Aabb::block(1, 0, 0)));
        assert!(a.intersects(&a.offset(DVec3::splat(0.5))));
    }

    #[test]
    fn stretch_covers_the_move() {
        let a = Aabb::block(0, 0, 0).stretch(DVec3::new(-1.0, 2.0, 0.0));
        assert_eq!(a.min, DVec3::new(-1.0, 0.0, 0.0));
        assert_eq!(a.max, DVec3::new(1.0, 3.0, 1.0));
    }

    #[test]
    fn world_collider_returns_solid_blocks_and_walls_off_unloaded_chunks() {
        let blocks = BlockRegistry::load(&crate::assets::dir().join("data/blocks.ron")).unwrap();
        let mut world = World::default();
        let mut chunk = Chunk::new(ChunkPos::new(0, 0));
        chunk.set(1, 63, 1, BlockState::new(1, 0)); // stone
        chunk.set(2, 63, 1, BlockState::new(37, 0)); // dandelion, not solid
        chunk.set(3, 63, 1, BlockState::new(79, 0)); // ice
        world.insert(chunk);
        let collider = WorldCollider {
            world: &world,
            blocks: &blocks,
        };
        let mut boxes = Vec::new();
        let region = Aabb::new(DVec3::new(1.0, 63.0, 1.0), DVec3::new(2.9, 63.9, 1.9));
        collider.boxes(&region, &mut boxes);
        assert_eq!(boxes, vec![Aabb::block(1, 63, 1)]);

        boxes.clear();
        let unloaded = Aabb::new(DVec3::new(16.2, 70.2, 0.2), DVec3::new(16.8, 70.8, 0.8));
        collider.boxes(&unloaded, &mut boxes);
        assert_eq!(boxes, vec![Aabb::block(16, 70, 0)]);

        assert_eq!(collider.slipperiness(3, 63, 1), 0.98);
        assert!(collider.is_loaded(&Aabb::block(5, 64, 5)));
        assert!(!collider.is_loaded(&Aabb::block(15, 64, 5)));
        assert_eq!(collider.slipperiness(1, 63, 1), 0.6);
    }
}
