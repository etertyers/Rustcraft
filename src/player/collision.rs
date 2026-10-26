//! Axis-aligned boxes and Minecraft-style collision against the world's blocks.

use glam::{DVec3, IVec3, Vec3};

use crate::world::{
    World,
    block::{BlockRegistry, BlockState, Fluid},
    blocks,
    chunk::ChunkPos,
    fluid, shape,
};

#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
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

    /// Whether `point` is inside (or on) the box.
    pub fn contains(&self, point: DVec3) -> bool {
        point.cmpge(self.min).all() && point.cmple(self.max).all()
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

    /// How the cell at (x, y, z) scales walking (soul sand 0.4), 1 for
    /// most blocks.
    fn speed_factor(&self, _x: i32, _y: i32, _z: i32) -> f32 {
        1.0
    }

    /// How the cell at (x, y, z) scales jumping (honey 0.5), 1 for most.
    fn jump_factor(&self, _x: i32, _y: i32, _z: i32) -> f32 {
        1.0
    }

    /// Whether the cell at (x, y, z) is a ladder or vine.
    fn climbable(&self, _x: i32, _y: i32, _z: i32) -> bool {
        false
    }

    /// Whether landing on the block at (x, y, z) bounces (slime).
    fn bouncy(&self, _x: i32, _y: i32, _z: i32) -> bool {
        false
    }

    /// The liquid `region` touches, if any (water first): one whose
    /// surface is above the bottom of `region` (modern
    /// `updateFluidHeightAndDoFluidPushing`, which the player's hitbox
    /// feels through, shrunk a hair).
    fn liquid(&self, _region: &Aabb) -> Option<Fluid> {
        None
    }

    /// How far above the bottom of `region` the highest surface of
    /// `fluid` it touches reaches, or zero.
    fn fluid_depth(&self, _region: &Aabb, _fluid: Fluid) -> f64 {
        0.0
    }

    /// The fluid at `point` if it is below the fluid's surface there.
    fn fluid_at(&self, _point: DVec3) -> Option<Fluid> {
        None
    }

    /// Which way the water `region` touches runs, normalized, or zero
    /// (the flow of every water cell it touches, added up).
    fn current(&self, _region: &Aabb) -> DVec3 {
        DVec3::ZERO
    }

    /// Whether `region` touches a fire block (1.0.0's
    /// `isBoundingBoxBurning`, without lava, which `liquid` covers).
    fn in_fire(&self, _region: &Aabb) -> bool {
        false
    }

    /// Whether `region` overlaps any lava, however shallow (1.0.0's
    /// `isMaterialInBB`, which burns items).
    fn in_lava(&self, _region: &Aabb) -> bool {
        false
    }
}

/// A world in which lava holds a body up like ground and doesn't count as
/// a liquid around it: what a strider walks on.
pub struct LavaFloor<'a, C: Collider>(pub &'a C);

impl<C: Collider> Collider for LavaFloor<'_, C> {
    fn boxes(&self, region: &Aabb, out: &mut Vec<Aabb>) {
        self.0.boxes(region, out);
        let min = region.min.floor().as_ivec3();
        let max = region.max.floor().as_ivec3();
        for y in min.y - 1..=max.y {
            for z in min.z..=max.z {
                for x in min.x..=max.x {
                    let center = DVec3::new(x as f64 + 0.5, y as f64 + 0.5, z as f64 + 0.5);
                    if self.0.fluid_at(center) == Some(Fluid::Lava) {
                        let cell = DVec3::new(x as f64, y as f64, z as f64);
                        out.push(Aabb::new(cell, cell + DVec3::ONE));
                    }
                }
            }
        }
    }

    fn slipperiness(&self, x: i32, y: i32, z: i32) -> f32 {
        self.0.slipperiness(x, y, z)
    }

    fn speed_factor(&self, x: i32, y: i32, z: i32) -> f32 {
        self.0.speed_factor(x, y, z)
    }

    fn liquid(&self, region: &Aabb) -> Option<Fluid> {
        self.0.liquid(region).filter(|&f| f != Fluid::Lava)
    }

    fn fluid_depth(&self, region: &Aabb, fluid: Fluid) -> f64 {
        if fluid == Fluid::Lava {
            0.0
        } else {
            self.0.fluid_depth(region, fluid)
        }
    }

    fn fluid_at(&self, point: DVec3) -> Option<Fluid> {
        self.0.fluid_at(point).filter(|&f| f != Fluid::Lava)
    }

    fn current(&self, region: &Aabb) -> DVec3 {
        self.0.current(region)
    }

    fn in_fire(&self, region: &Aabb) -> bool {
        self.0.in_fire(region)
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

impl WorldCollider<'_> {
    /// The definition of the block at (x, y, z), if it is loaded.
    fn def_at(&self, x: i32, y: i32, z: i32) -> Option<&crate::world::block::BlockDef> {
        self.blocks.get(self.world.block(x, y, z)?.id())
    }

    /// Whether any block in a cell `region` overlaps passes `test`.
    fn any_cell(&self, region: &Aabb, test: impl Fn(BlockState) -> bool) -> bool {
        let min = (region.min + 0.001).floor().as_ivec3();
        let max = (region.max - 0.001).floor().as_ivec3();
        (min.y..=max.y).any(|y| {
            (min.z..=max.z)
                .any(|z| (min.x..=max.x).any(|x| self.world.block(x, y, z).is_some_and(&test)))
        })
    }

    /// The liquid cells `region` touches whose surface is above its
    /// bottom, with the surface height.
    fn liquid_cells<'b>(
        &'b self,
        region: &'b Aabb,
    ) -> impl Iterator<Item = (IVec3, Fluid, f64)> + 'b {
        let min = region.min.floor().as_ivec3();
        let max = region.max.floor().as_ivec3();
        (min.y..=max.y).flat_map(move |y| {
            (min.z..=max.z).flat_map(move |z| {
                (min.x..=max.x).filter_map(move |x| {
                    let block = self.world.block(x, y, z)?;
                    let fluid = self.blocks.get(block.id())?.fluid?;
                    let surface =
                        (y + 1) as f64 - fluid::height_percent(fluid::level(block)) as f64;
                    (surface >= region.min.y).then_some((IVec3::new(x, y, z), fluid, surface))
                })
            })
        })
    }
}

impl Collider for WorldCollider<'_> {
    fn boxes(&self, region: &Aabb, out: &mut Vec<Aabb>) {
        let min = region.min.floor().as_ivec3();
        let max = region.max.floor().as_ivec3();
        let shapes = shape::shapes();
        // One cell lower too: fences and walls reach half a block up.
        for y in min.y - 1..=max.y {
            for z in min.z..=max.z {
                for x in min.x..=max.x {
                    let cell = DVec3::new(x as f64, y as f64, z as f64);
                    let Some(block) = self.world.block(x, y, z) else {
                        // Unloaded chunks are walls.
                        if y >= min.y {
                            out.push(Aabb::new(cell, cell + DVec3::ONE));
                        }
                        continue;
                    };
                    for b in shapes.collision(block) {
                        let found = Aabb::new(
                            cell + Vec3::from(b.min).as_dvec3(),
                            cell + Vec3::from(b.max).as_dvec3(),
                        );
                        if found.intersects(region) || y >= min.y {
                            out.push(found);
                        }
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

    fn speed_factor(&self, x: i32, y: i32, z: i32) -> f32 {
        self.def_at(x, y, z).map_or(1.0, |def| def.speed_factor)
    }

    fn jump_factor(&self, x: i32, y: i32, z: i32) -> f32 {
        self.def_at(x, y, z).map_or(1.0, |def| def.jump_factor)
    }

    fn climbable(&self, x: i32, y: i32, z: i32) -> bool {
        self.def_at(x, y, z).is_some_and(|def| def.climbable)
    }

    fn bouncy(&self, x: i32, y: i32, z: i32) -> bool {
        self.def_at(x, y, z).is_some_and(|def| def.bouncy)
    }

    fn liquid(&self, region: &Aabb) -> Option<Fluid> {
        let mut found = None;
        for (_, fluid, _) in self.liquid_cells(region) {
            if fluid == Fluid::Water {
                return Some(fluid);
            }
            found = Some(fluid);
        }
        found
    }

    fn fluid_depth(&self, region: &Aabb, fluid: Fluid) -> f64 {
        self.liquid_cells(region)
            .filter(|&(_, found, _)| found == fluid)
            .map(|(_, _, surface)| surface - region.min.y)
            .fold(0.0, f64::max)
    }

    fn fluid_at(&self, point: DVec3) -> Option<Fluid> {
        let cell = point.floor().as_ivec3();
        let fluid_of = |at: IVec3| {
            let block = self.world.block(at.x, at.y, at.z)?;
            Some((self.blocks.get(block.id())?.fluid?, block))
        };
        let (fluid, block) = fluid_of(cell)?;
        // A cell with the same fluid above it is full to the top.
        let full = fluid_of(cell + IVec3::Y).is_some_and(|(above, _)| above == fluid);
        let surface = if full {
            (cell.y + 1) as f64
        } else {
            (cell.y + 1) as f64 - fluid::height_percent(fluid::level(block)) as f64
        };
        (point.y < surface).then_some(fluid)
    }

    fn in_fire(&self, region: &Aabb) -> bool {
        self.any_cell(region, |block| {
            block.is(blocks::FIRE) || block.is(blocks::SOUL_FIRE)
        })
    }

    fn in_lava(&self, region: &Aabb) -> bool {
        self.any_cell(region, |block| {
            self.blocks
                .get(block.id())
                .is_some_and(|def| def.fluid == Some(Fluid::Lava))
        })
    }

    fn current(&self, region: &Aabb) -> DVec3 {
        let cell = |at: IVec3| {
            let block = self.world.block(at.x, at.y, at.z)?;
            Some(fluid::Cell::new(block, self.blocks))
        };
        let flow: DVec3 = self
            .liquid_cells(region)
            .filter(|&(_, fluid, _)| fluid == Fluid::Water)
            .map(|(at, fluid, _)| fluid::flow_vector(fluid, at, &cell))
            .sum();
        flow.normalize_or_zero()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::blocks;
    use crate::world::chunk::Chunk;

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
        let registry = crate::world::block::registry();
        let mut world = World::default();
        let mut chunk = Chunk::new(ChunkPos::new(0, 0));
        chunk.set(1, 63, 1, BlockState::of(blocks::STONE)); // stone
        chunk.set(2, 63, 1, BlockState::of(blocks::DANDELION)); // dandelion, not solid
        chunk.set(3, 63, 1, BlockState::of(blocks::ICE)); // ice
        world.insert(chunk);
        let collider = WorldCollider {
            world: &world,
            blocks: registry,
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
        // Block movement data: soul sand slows, ladders and vines climb.
        assert_eq!(collider.speed_factor(1, 63, 1), 1.0);
        assert_eq!(collider.jump_factor(1, 63, 1), 1.0);
        assert!(!collider.climbable(1, 63, 1) && !collider.bouncy(1, 63, 1));
        let defs = |id| registry.get(id).unwrap();
        assert_eq!(defs(blocks::SOUL_SAND).speed_factor, 0.4);
        assert!(defs(blocks::LADDER).climbable && defs(blocks::VINE).climbable);
        assert!(collider.is_loaded(&Aabb::block(5, 64, 5)));
        assert!(!collider.is_loaded(&Aabb::block(15, 64, 5)));
        assert_eq!(collider.slipperiness(1, 63, 1), 0.6);
    }
}
