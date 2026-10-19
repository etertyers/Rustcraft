//! Fire, following modern Minecraft's `FireBlock` in the Overworld on
//! normal difficulty, without rain: fire ages on scheduled ticks, spreads to
//! air beside flammable blocks, burns those blocks away and goes out once
//! nothing around it burns. Lava starts fires on random ticks (modern
//! `LavaFluid.randomTick`). A fire's `age` is 0–15.

use std::collections::BTreeMap;

use glam::IVec3;

use super::{
    World,
    block::{BlockDef, BlockRegistry, BlockState, Shape},
    blocks::{FIRE, LAVA, NETHERRACK},
    chunk::{Chunk, ChunkPos, SECTION_COUNT, SECTION_SIZE},
    fast_hash::{FastMap, FastSet},
    props::Prop,
};
use crate::config::{CHUNK_SIZE, WORLD_HEIGHT};

// Netherrack burns forever (modern `#infiniburn_overworld`).
const MAX_AGE: u8 = 15;
/// Normal difficulty, which makes fire spread a little faster.
const DIFFICULTY: i32 = 2;
/// Random ticks per 16-block section each game tick (modern
/// `randomTickSpeed`).
const RANDOM_TICKS: u32 = 3;
const NEIGHBORS: [IVec3; 6] = [
    IVec3::NEG_X,
    IVec3::X,
    IVec3::NEG_Y,
    IVec3::Y,
    IVec3::NEG_Z,
    IVec3::Z,
];

/// Fire of age `age`.
fn fire(age: u8) -> BlockState {
    BlockState::of(FIRE).with_number(Prop::Age15, age)
}

/// Scheduled fire updates, and blocks changed since the last tick. Not
/// saved: fire in a chunk is scheduled again when it loads.
#[derive(Debug, Default)]
pub struct Fires {
    /// By (due tick, order scheduled).
    queue: BTreeMap<(u64, u64), IVec3>,
    scheduled: FastSet<IVec3>,
    next: u64,
    changed: Vec<IVec3>,
}

impl Fires {
    /// Notes that the block at `at` changed, so fire next to it that lost
    /// its footing goes out.
    pub fn changed(&mut self, at: IVec3) {
        self.changed.push(at);
    }

    /// How many updates are waiting.
    #[cfg(test)]
    pub fn pending(&self) -> usize {
        self.queue.len()
    }

    /// Schedules the fire in a chunk that just loaded.
    pub fn chunk_loaded(&mut self, chunk: &Chunk, time: u64) {
        let (min_x, min_z) = chunk.pos.min_block();
        for section in (0..SECTION_COUNT).filter(|&s| !chunk.section_is_empty(s)) {
            for y in section * SECTION_SIZE..(section + 1) * SECTION_SIZE {
                for z in 0..CHUNK_SIZE {
                    for x in 0..CHUNK_SIZE {
                        if chunk.get(x, y, z).id() == FIRE {
                            let at = IVec3::new(min_x + x as i32, y as i32, min_z + z as i32);
                            self.schedule(at, time + 30);
                        }
                    }
                }
            }
        }
    }

    fn schedule(&mut self, at: IVec3, due: u64) {
        if self.scheduled.insert(at) {
            self.queue.insert((due, self.next), at);
            self.next += 1;
        }
    }

    /// Lights a fire at `at` as flint and steel does (modern
    /// `BaseFireBlock.canBePlacedAt`): in air where fire could stay, on a
    /// sturdy block or beside something that burns. Returns the new block.
    pub fn ignite(
        &mut self,
        world: &World,
        blocks: &BlockRegistry,
        at: IVec3,
        time: u64,
        random: &mut dyn FnMut(u32) -> u32,
    ) -> Option<(IVec3, BlockState)> {
        let mut step = Step {
            world,
            blocks,
            fires: self,
            time,
            random,
            edits: FastMap::default(),
            order: Vec::new(),
        };
        if !step.is_air(at) || !step.can_survive(at) {
            return None;
        }
        step.place_fire(at, 0);
        Some((at, fire(0)))
    }

    /// Puts out fire that lost its footing, runs the fire updates due by
    /// `time`, then random-ticks the lava in the chunks `near` the player.
    /// `random(n)` is a random number below `n`. Returns every changed
    /// block's new state, once each.
    pub fn tick(
        &mut self,
        world: &World,
        blocks: &BlockRegistry,
        time: u64,
        near: &[ChunkPos],
        random: &mut dyn FnMut(u32) -> u32,
    ) -> Vec<(IVec3, BlockState)> {
        let changed = std::mem::take(&mut self.changed);
        let mut due = Vec::new();
        while let Some(entry) = self.queue.first_entry() {
            if entry.key().0 > time {
                break;
            }
            let at = entry.remove();
            self.scheduled.remove(&at);
            due.push(at);
        }
        let mut step = Step {
            world,
            blocks,
            fires: self,
            time,
            random,
            edits: FastMap::default(),
            order: Vec::new(),
        };
        for at in changed {
            for offset in NEIGHBORS {
                step.check_footing(at + offset);
            }
        }
        for at in due {
            step.update(at);
        }
        for &pos in near {
            step.random_ticks(pos);
        }
        let Step { edits, order, .. } = step;
        order.into_iter().map(|at| (at, edits[&at])).collect()
    }
}

/// One tick's work, with its edits laid over the world.
struct Step<'a> {
    world: &'a World,
    blocks: &'a BlockRegistry,
    fires: &'a mut Fires,
    time: u64,
    random: &'a mut dyn FnMut(u32) -> u32,
    edits: FastMap<IVec3, BlockState>,
    order: Vec<IVec3>,
}

impl Step<'_> {
    fn get(&self, at: IVec3) -> Option<BlockState> {
        if !(0..WORLD_HEIGHT as i32).contains(&at.y) {
            return None;
        }
        match self.edits.get(&at) {
            Some(&block) => Some(block),
            None => self.world.block(at.x, at.y, at.z),
        }
    }

    fn def(&self, at: IVec3) -> Option<&BlockDef> {
        self.blocks.get(self.get(at)?.id())
    }

    fn set(&mut self, at: IVec3, block: BlockState) {
        if self.edits.insert(at, block).is_none() {
            self.order.push(at);
        }
    }

    fn is_air(&self, at: IVec3) -> bool {
        self.get(at).is_some_and(BlockState::is_air)
    }

    fn flammable(&self, at: IVec3) -> bool {
        self.def(at).is_some_and(|def| def.burns.is_some())
    }

    fn has_flammable_neighbor(&self, at: IVec3) -> bool {
        NEIGHBORS.iter().any(|&offset| self.flammable(at + offset))
    }

    /// A full solid block to stand on (modern `isFaceSturdy` for the top).
    fn sturdy(&self, at: IVec3) -> bool {
        self.def(at)
            .is_some_and(|def| def.solid && def.shape == Shape::Cube)
    }

    /// Fire stays on a sturdy block or beside something that burns.
    fn can_survive(&self, at: IVec3) -> bool {
        self.sturdy(at - IVec3::Y) || self.has_flammable_neighbor(at)
    }

    fn place_fire(&mut self, at: IVec3, age: u8) {
        self.set(at, fire(age.min(MAX_AGE)));
        let delay = self.delay();
        self.fires.schedule(at, self.time + delay);
    }

    /// Ticks until a fire's next update: 30 to 39.
    fn delay(&mut self) -> u64 {
        30 + (self.random)(10) as u64
    }

    /// Puts out the fire at `at`, if there is one, when it can't stay.
    fn check_footing(&mut self, at: IVec3) {
        if self.get(at).is_some_and(|block| block.id() == FIRE) && !self.can_survive(at) {
            self.set(at, BlockState::AIR);
        }
    }

    /// A scheduled update of the fire at `at` (modern `FireBlock.tick`).
    fn update(&mut self, at: IVec3) {
        let Some(block) = self.get(at).filter(|block| block.id() == FIRE) else {
            return;
        };
        if !self.can_survive(at) {
            self.set(at, BlockState::AIR);
            return;
        }
        let delay = self.delay();
        self.fires.schedule(at, self.time + delay);
        let below = at - IVec3::Y;
        let forever = self.get(below).is_some_and(|b| b.id() == NETHERRACK);
        // It ages, but this update goes by the age it had.
        let age = block.number(Prop::Age15);
        let older = (age + (self.random)(3) as u8 / 2).min(MAX_AGE);
        if older != age {
            self.set(at, fire(older));
        }
        if !forever {
            if !self.has_flammable_neighbor(at) {
                if !self.sturdy(below) || age > 3 {
                    self.set(at, BlockState::AIR);
                }
                return;
            }
            if age == MAX_AGE && (self.random)(4) == 0 && !self.flammable(below) {
                self.set(at, BlockState::AIR);
                return;
            }
        }
        for (offset, chance) in [
            (IVec3::X, 300),
            (IVec3::NEG_X, 300),
            (IVec3::NEG_Y, 250),
            (IVec3::Y, 250),
            (IVec3::NEG_Z, 300),
            (IVec3::Z, 300),
        ] {
            self.burn_out(at + offset, chance, age);
        }
        // Spreads to air up to 4 above and 1 around or below, less likely
        // the higher it goes.
        for dy in -1..=4 {
            for dz in -1..=1 {
                for dx in -1..=1 {
                    if (dx, dy, dz) == (0, 0, 0) {
                        continue;
                    }
                    let to = at + IVec3::new(dx, dy, dz);
                    let odds = self.ignite_odds(to);
                    if odds == 0 {
                        continue;
                    }
                    let out_of = 100 + if dy > 1 { (dy - 1) * 100 } else { 0 };
                    let spread = (odds + 40 + DIFFICULTY * 7) / (age as i32 + 30);
                    if spread > 0 && (self.random)(out_of as u32) as i32 <= spread {
                        let new_age = age + (self.random)(5) as u8 / 4;
                        self.place_fire(to, new_age);
                    }
                }
            }
        }
    }

    /// The block at `at` may catch: burn away, or turn to fire itself
    /// (modern `checkBurnOut`). `chance` is out of how many it's rolled.
    fn burn_out(&mut self, at: IVec3, chance: u32, age: u8) {
        let Some(burns) = self.def(at).and_then(|def| def.burns) else {
            return;
        };
        if (self.random)(chance) >= burns.catch as u32 {
            return;
        }
        if (self.random)(age as u32 + 10) < 5 {
            let new_age = age + (self.random)(5) as u8 / 4;
            self.place_fire(at, new_age);
        } else {
            self.set(at, BlockState::AIR);
        }
    }

    /// How readily fire spreads into the air at `at`: the most encouraging
    /// block next to it (modern `getIgniteOdds`), or 0 if it isn't air.
    fn ignite_odds(&self, at: IVec3) -> i32 {
        if !self.is_air(at) {
            return 0;
        }
        NEIGHBORS
            .iter()
            .filter_map(|&offset| self.def(at + offset)?.burns)
            .map(|burns| burns.encourage as i32)
            .max()
            .unwrap_or(0)
    }

    /// Random ticks in one chunk: a few random cells per section, and the
    /// lava among them may start fires.
    fn random_ticks(&mut self, pos: ChunkPos) {
        let world = self.world;
        let Some(chunk) = world.get(pos) else {
            return;
        };
        let (min_x, min_z) = pos.min_block();
        for section in (0..SECTION_COUNT).filter(|&s| !chunk.section_is_empty(s)) {
            for _ in 0..RANDOM_TICKS {
                let x = (self.random)(CHUNK_SIZE as u32) as i32;
                let y = (section * SECTION_SIZE) as i32 + (self.random)(SECTION_SIZE as u32) as i32;
                let z = (self.random)(CHUNK_SIZE as u32) as i32;
                let at = IVec3::new(min_x + x, y, min_z + z);
                if self.get(at).is_some_and(|block| block.is(LAVA)) {
                    self.lava_tick(at);
                }
            }
        }
    }

    /// Lava's random tick (modern `LavaFluid.randomTick`): two times in
    /// three it looks up to 2 cells up and around for air beside something
    /// flammable; otherwise it tries 3 cells around it for a flammable
    /// block with air on top.
    fn lava_tick(&mut self, at: IVec3) {
        let rises = (self.random)(3);
        if rises > 0 {
            let mut to = at;
            for _ in 0..rises {
                let dx = (self.random)(3) as i32 - 1;
                let dz = (self.random)(3) as i32 - 1;
                to += IVec3::new(dx, 1, dz);
                let Some(def) = self.def(to) else {
                    return;
                };
                if self.is_air(to) {
                    if self.has_flammable_neighbor(to) {
                        self.place_fire(to, 0);
                        return;
                    }
                } else if def.solid {
                    return;
                }
            }
        } else {
            for _ in 0..3 {
                let dx = (self.random)(3) as i32 - 1;
                let dz = (self.random)(3) as i32 - 1;
                let to = at + IVec3::new(dx, 0, dz);
                if self.get(to).is_none() {
                    return;
                }
                if self.is_air(to + IVec3::Y) && self.flammable(to) {
                    self.place_fire(to + IVec3::Y, 0);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::blocks;

    const STONE: BlockState = BlockState::of(blocks::STONE);
    const PLANKS: BlockState = BlockState::of(blocks::OAK_PLANKS);
    const FLOOR: i32 = 63;

    struct Yard {
        world: World,
        blocks: &'static BlockRegistry,
        fires: Fires,
        time: u64,
        seed: u32,
    }

    impl Yard {
        /// 3×3 chunks of air over a stone floor at y 63.
        fn new() -> Self {
            let mut world = World::default();
            for cz in -1..=1 {
                for cx in -1..=1 {
                    let mut chunk = Chunk::new(ChunkPos::new(cx, cz));
                    for z in 0..CHUNK_SIZE {
                        for x in 0..CHUNK_SIZE {
                            chunk.set(x, FLOOR as usize, z, STONE);
                        }
                    }
                    world.insert(chunk);
                }
            }
            let blocks = crate::world::block::registry();
            Self {
                world,
                blocks,
                fires: Fires::default(),
                time: 0,
                seed: 3,
            }
        }

        fn get(&self, at: IVec3) -> BlockState {
            self.world.block(at.x, at.y, at.z).unwrap()
        }

        fn set(&mut self, at: IVec3, block: BlockState) {
            let pos = ChunkPos::of_block(at.x, at.z);
            let size = CHUNK_SIZE as i32;
            let chunk = self.world.get_mut(pos).unwrap();
            let (x, z) = (at.x.rem_euclid(size), at.z.rem_euclid(size));
            chunk.set(x as usize, at.y as usize, z as usize, block);
            self.fires.changed(at);
        }

        fn light(&mut self, at: IVec3) {
            self.set(at, BlockState::of(FIRE));
            self.fires.schedule(at, self.time + 1);
        }

        fn run(&mut self, ticks: u32, near: &[ChunkPos]) {
            for _ in 0..ticks {
                let seed = &mut self.seed;
                let mut random = |n: u32| {
                    *seed = seed.wrapping_mul(1_103_515_245).wrapping_add(12345);
                    (*seed >> 16) % n
                };
                let edits = self
                    .fires
                    .tick(&self.world, self.blocks, self.time, near, &mut random);
                for (at, block) in edits {
                    self.set(at, block);
                }
                self.time += 1;
            }
        }

        fn count(&self, block: crate::world::block::BlockId) -> usize {
            let mut count = 0;
            for x in -16..32 {
                for z in -16..32 {
                    for y in FLOOR..FLOOR + 8 {
                        if self.get(IVec3::new(x, y, z)).id() == block {
                            count += 1;
                        }
                    }
                }
            }
            count
        }
    }

    #[test]
    fn fire_spreads_over_a_wooden_floor_and_goes_out() {
        let mut yard = Yard::new();
        for x in 0..6 {
            for z in 0..6 {
                yard.set(IVec3::new(x, 64, z), PLANKS);
            }
        }
        yard.light(IVec3::new(0, 65, 0));
        yard.run(400, &[]);
        assert!(yard.count(FIRE) > 1, "it spread");
        yard.run(20_000, &[]);
        // Fire creeps slowly over flat ground, so a floor burns in patches.
        assert!(
            yard.count(blocks::OAK_PLANKS) < 36,
            "some planks burned away"
        );
        assert_eq!(yard.count(FIRE), 0, "and the fire went out");
        assert_eq!(yard.fires.pending(), 0);
        assert_eq!(
            yard.get(IVec3::new(3, FLOOR, 3)),
            STONE,
            "stone doesn't burn"
        );
    }

    #[test]
    fn fire_on_bare_stone_dies_down_and_loses_its_footing() {
        let mut yard = Yard::new();
        let at = IVec3::new(0, 64, 0);
        yard.light(at);
        yard.run(2000, &[]);
        assert_eq!(yard.get(at), BlockState::AIR, "nothing to burn");

        // Fire on a plank goes out when the plank is gone.
        let mut yard = Yard::new();
        let plank = IVec3::new(0, 65, 0);
        yard.set(plank, PLANKS);
        yard.light(plank + IVec3::Y);
        yard.set(plank, BlockState::AIR);
        yard.run(1, &[]);
        assert_eq!(yard.get(plank + IVec3::Y), BlockState::AIR);

        // On netherrack it burns forever.
        let mut yard = Yard::new();
        yard.set(at, BlockState::of(NETHERRACK));
        yard.light(at + IVec3::Y);
        yard.run(5000, &[]);
        assert_eq!(yard.get(at + IVec3::Y).id(), FIRE);
    }

    #[test]
    fn lava_sets_fire_to_wood_near_it() {
        let mut yard = Yard::new();
        let lava = IVec3::new(4, 64, 4);
        yard.set(lava, BlockState::of(LAVA));
        for x in 2..=6 {
            for z in 2..=6 {
                if (x, z) != (4, 4) {
                    yard.set(IVec3::new(x, 64, z), PLANKS);
                }
            }
        }
        let planks = yard.count(blocks::OAK_PLANKS);
        // Random ticks are rare: 3 a section a tick, and the lava is one
        // cell in 4096.
        yard.run(30_000, &[ChunkPos::new(0, 0)]);
        assert!(
            yard.count(blocks::OAK_PLANKS) < planks,
            "some planks burned"
        );

        // Without anything flammable, lava starts nothing.
        let mut yard = Yard::new();
        yard.set(lava, BlockState::of(LAVA));
        yard.run(30_000, &[ChunkPos::new(0, 0)]);
        assert_eq!(yard.count(FIRE), 0);
    }
}
