//! Flowing water and lava, following 1.0.0's `BlockFluid`, `BlockFlowing`
//! and `BlockStationary`, with modern Minecraft's blocks: one `water` and
//! one `lava` block whose `level` is 0 for a source, 1–7 for flowing water
//! further from one (lava steps by 2), and 8 plus the level above for fluid
//! falling down. A fluid updates on scheduled ticks (every 5 game ticks for
//! water, 30 for lava) while it's spreading, and is scheduled again when
//! anything next to it changes (where 1.0.0 swapped still and flowing
//! blocks).
//!
//! Lava meeting water follows modern Minecraft instead, so cobblestone
//! generators work: any lava touching water hardens (a source to obsidian,
//! the rest to cobblestone), lava can't flow sideways into water, and lava
//! looks only 2 blocks ahead for a way down.

use std::collections::BTreeMap;

use glam::{DVec3, IVec3};

use super::{
    World,
    block::{BlockId, BlockRegistry, BlockState, Fluid},
    blocks,
    chunk::{Chunk, SECTION_COUNT},
    fast_hash::{FastMap, FastSet},
    props::Prop,
};
use crate::config::CHUNK_SIZE;

const STONE: BlockState = BlockState::of(blocks::STONE);
const COBBLESTONE: BlockState = BlockState::of(blocks::COBBLESTONE);
const OBSIDIAN: BlockState = BlockState::of(blocks::OBSIDIAN);

/// 1.0.0 runs at most this many scheduled ticks per game tick.
const MAX_UPDATES: usize = 1000;
/// Levels a flow loses per block, and the level past which it stops.
const LAST_LEVEL: i32 = 8;
/// The bit set on fluid falling down.
const FALLING: i32 = 8;
/// The four sideways neighbors, in 1.0.0's order (−x, +x, −z, +z).
const SIDEWAYS: [IVec3; 4] = [
    IVec3::new(-1, 0, 0),
    IVec3::new(1, 0, 0),
    IVec3::new(0, 0, -1),
    IVec3::new(0, 0, 1),
];

/// A fluid's block.
pub fn block(fluid: Fluid) -> BlockId {
    match fluid {
        Fluid::Water => blocks::WATER,
        Fluid::Lava => blocks::LAVA,
    }
}

/// `fluid` at `level`.
pub fn at_level(fluid: Fluid, level: u8) -> BlockState {
    BlockState::of(block(fluid)).with_number(Prop::Level, level)
}

/// A fluid block's level (0 for a source).
pub fn level(block: BlockState) -> u8 {
    block.number(Prop::Level)
}

/// Game ticks between a flowing block's updates; lava is three times
/// quicker in the Nether (`warm`, modern `ultraWarm`).
fn tick_rate(fluid: Fluid, warm: bool) -> u64 {
    match fluid {
        Fluid::Water => 5,
        Fluid::Lava if warm => 10,
        Fluid::Lava => 30,
    }
}

/// How far ahead a flow looks for a way down: 1.0.0's `calculateFlowCost`
/// gives up past 4 blocks, modern lava in the Overworld past 2 (4 in the
/// Nether).
fn search_depth(fluid: Fluid, warm: bool) -> i32 {
    match fluid {
        Fluid::Water => 4,
        Fluid::Lava if warm => 4,
        Fluid::Lava => 2,
    }
}

/// Levels lost per block of spreading: lava reaches 3 blocks (7 in the
/// Nether), water 7.
fn level_step(fluid: Fluid, warm: bool) -> i32 {
    match fluid {
        Fluid::Water => 1,
        Fluid::Lava if warm => 1,
        Fluid::Lava => 2,
    }
}

/// How much of its cell a fluid at `level` doesn't fill (1.0.0's
/// `getFluidHeightPercent`, which is the empty part): a source or a fall
/// leaves 1/9 empty, the last flowing level 8/9.
pub fn height_percent(level: u8) -> f32 {
    let level = if level >= FALLING as u8 { 0 } else { level };
    (level as f32 + 1.0) / 9.0
}

/// What the fluid rules need to know about one cell.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Cell {
    pub block: BlockState,
    pub fluid: Option<Fluid>,
    /// Holds fluids back: 1.0.0's solid materials, plus signs, doors,
    /// ladders and sugar cane.
    pub stops: bool,
}

impl Cell {
    pub fn new(block: BlockState, blocks: &BlockRegistry) -> Self {
        let def = blocks.get(block.id());
        Self {
            block,
            fluid: def.and_then(|def| def.fluid),
            stops: def.is_some_and(|def| def.stops_fluid()),
        }
    }

    /// The fluid's level if it's `fluid`, with falling counted as 0
    /// (1.0.0's `getEffectiveFlowDecay`), or −1 if it isn't.
    fn effective_level(cell: Option<Self>, fluid: Fluid) -> i32 {
        match cell {
            Some(cell) if cell.fluid == Some(fluid) => {
                let level = level(cell.block) as i32;
                if level >= FALLING { 0 } else { level }
            }
            _ => -1,
        }
    }
}

/// The direction `fluid` at `at` runs, normalized, or zero for still
/// fluid (1.0.0's `getFlowVector`): toward lower neighbors, and steeply
/// down for a fall beside a wall. Pushes entities and turns the flowing
/// texture.
pub fn flow_vector(fluid: Fluid, at: IVec3, cell: &impl Fn(IVec3) -> Option<Cell>) -> DVec3 {
    let own = Cell::effective_level(cell(at), fluid);
    let mut flow = DVec3::ZERO;
    for side in SIDEWAYS {
        let next = at + side;
        let level = Cell::effective_level(cell(next), fluid);
        let step = side.as_dvec3();
        if level >= 0 {
            flow += step * (level - own) as f64;
        } else if cell(next).is_some_and(|c| !c.stops) {
            // An opening: how far the fluid would fall into it.
            let below = Cell::effective_level(cell(next - IVec3::Y), fluid);
            if below >= 0 {
                flow += step * (below - (own - FALLING)) as f64;
            }
        }
    }
    let falling = cell(at).is_some_and(|c| c.fluid.is_some() && level(c.block) as i32 >= FALLING);
    if falling {
        let walled = SIDEWAYS.iter().any(|&side| {
            [at + side, at + side + IVec3::Y]
                .into_iter()
                .any(|next| cell(next).is_some_and(|c| c.fluid != Some(fluid) && c.stops))
        });
        if walled {
            flow = flow.normalize_or_zero() + DVec3::new(0.0, -6.0, 0.0);
        }
    }
    flow.normalize_or_zero()
}

/// Height of a fluid's surface at the block corner `corner` (the
/// smallest x and z of the four cells around it), 0–1 above `corner.y`
/// (1.0.0's `RenderBlocks.getFluidHeight`): full if the fluid is above any
/// of the four cells, otherwise an average weighted toward sources and
/// falls, with open cells pulling it down.
pub fn corner_height(fluid: Fluid, corner: IVec3, cell: &impl Fn(IVec3) -> Option<Cell>) -> f32 {
    let mut total = 0.0;
    let mut count = 0.0;
    for (dx, dz) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
        let at = corner - IVec3::new(dx, 0, dz);
        if cell(at + IVec3::Y).is_some_and(|c| c.fluid == Some(fluid)) {
            return 1.0;
        }
        let Some(here) = cell(at) else {
            continue;
        };
        if here.fluid == Some(fluid) {
            let level = level(here.block);
            let empty = height_percent(level);
            if level as i32 >= FALLING || level == 0 {
                total += empty * 10.0;
                count += 10.0;
            }
            total += empty;
            count += 1.0;
        } else if !here.stops {
            total += 1.0;
            count += 1.0;
        }
    }
    if count == 0.0 {
        1.0 - height_percent(0)
    } else {
        1.0 - total / count
    }
}

/// Scheduled fluid updates, and blocks the game changed since the last
/// tick (1.0.0's `scheduleBlockUpdate` and neighbor notifications). Not
/// saved: flowing fluid in a chunk (any level but a source's) is scheduled
/// again when it loads.
#[derive(Debug, Default)]
pub struct Fluids {
    /// By (due tick, order scheduled).
    queue: BTreeMap<(u64, u64), (IVec3, BlockId)>,
    scheduled: FastSet<(IVec3, BlockId)>,
    next: u64,
    changed: Vec<IVec3>,
    /// In the Nether, where lava runs further and faster (M24).
    pub warm: bool,
}

/// Whether fluid at local (x, y, z) could flow into a cell beside or below
/// it in the same chunk (cells in other chunks count as closed).
fn open_beside(chunk: &Chunk, x: usize, y: i32, z: usize) -> bool {
    let own = chunk.get(x, y, z).id();
    [(-1, 0, 0), (1, 0, 0), (0, 0, -1), (0, 0, 1), (0, -1, 0)]
        .into_iter()
        .any(|(dx, dy, dz)| {
            let (nx, ny, nz) = (x as i32 + dx, y + dy, z as i32 + dz);
            let size = CHUNK_SIZE as i32;
            if !(0..size).contains(&nx) || !(0..size).contains(&nz) || ny < crate::config::MIN_Y {
                return false;
            }
            let next = chunk.get(nx as usize, ny, nz as usize);
            next.id() != own && !next.def().stops_fluid() && next.def().fluid.is_none()
        })
}

/// What a tick of fluids did.
#[derive(Debug, Default, PartialEq)]
pub struct Outcome {
    /// Every changed block's new state, once each.
    pub edits: Vec<(IVec3, BlockState)>,
    /// Blocks water washed away, which drop as items.
    pub washed: Vec<(IVec3, BlockState)>,
}

impl Fluids {
    /// Notes that the game set the block at `at`, so fluid there starts
    /// flowing and fluid next to it wakes up.
    pub fn changed(&mut self, at: IVec3) {
        self.changed.push(at);
    }

    /// How many updates are waiting.
    #[cfg(test)]
    pub fn pending(&self) -> usize {
        self.queue.len()
    }

    /// Schedules the flowing blocks of a chunk that just loaded.
    pub fn chunk_loaded(&mut self, chunk: &Chunk, time: u64) {
        let (min_x, min_z) = chunk.pos.min_block();
        for section in (0..SECTION_COUNT).filter(|&s| !chunk.section_is_empty(s)) {
            for y in crate::world::chunk::section_bottom(section)
                ..crate::world::chunk::section_bottom(section + 1)
            {
                for z in 0..CHUNK_SIZE {
                    for x in 0..CHUNK_SIZE {
                        let block = chunk.get(x, y, z);
                        let fluid = match block.id() {
                            blocks::WATER => Fluid::Water,
                            blocks::LAVA => Fluid::Lava,
                            _ => continue,
                        };
                        // A source only if it has somewhere to go (one poured
                        // just before the chunk was saved); oceans don't.
                        if level(block) == 0 && !open_beside(chunk, x, y, z) {
                            continue;
                        }
                        let at = IVec3::new(min_x + x as i32, y, min_z + z as i32);
                        self.schedule(at, block.id(), time + tick_rate(fluid, self.warm));
                    }
                }
            }
        }
    }

    fn schedule(&mut self, at: IVec3, id: BlockId, due: u64) {
        if self.scheduled.insert((at, id)) {
            self.queue.insert((due, self.next), (at, id));
            self.next += 1;
        }
    }

    /// Handles the game's changes, then runs the updates due by `time`.
    /// `random(n)` is a random number below `n`.
    pub fn tick(
        &mut self,
        world: &World,
        blocks: &BlockRegistry,
        time: u64,
        random: &mut dyn FnMut(u32) -> u32,
    ) -> Outcome {
        let changed = std::mem::take(&mut self.changed);
        let mut due = Vec::new();
        while due.len() < MAX_UPDATES {
            let Some(entry) = self.queue.first_entry() else {
                break;
            };
            if entry.key().0 > time {
                break;
            }
            let (at, id) = entry.remove();
            self.scheduled.remove(&(at, id));
            due.push((at, id));
        }
        let mut step = Step {
            world,
            blocks,
            fluids: self,
            time,
            random,
            edits: FastMap::default(),
            order: Vec::new(),
            washed: Vec::new(),
        };
        for at in changed {
            step.added(at);
            step.notify_neighbors(at);
        }
        for (at, id) in due {
            if step.get(at).map(BlockState::id) == Some(id) {
                step.update(at);
            }
        }
        let Step {
            edits,
            order,
            washed,
            ..
        } = step;
        Outcome {
            edits: order.into_iter().map(|at| (at, edits[&at])).collect(),
            washed,
        }
    }
}

/// One tick's work, with its edits laid over the world so later updates
/// see earlier ones.
struct Step<'a> {
    world: &'a World,
    blocks: &'a BlockRegistry,
    fluids: &'a mut Fluids,
    time: u64,
    random: &'a mut dyn FnMut(u32) -> u32,
    edits: FastMap<IVec3, BlockState>,
    order: Vec<IVec3>,
    washed: Vec<(IVec3, BlockState)>,
}

impl Step<'_> {
    fn get(&self, at: IVec3) -> Option<BlockState> {
        if !crate::world::chunk::in_height(at.y) {
            return None;
        }
        match self.edits.get(&at) {
            Some(&block) => Some(block),
            None => self.world.block(at.x, at.y, at.z),
        }
    }

    fn cell(&self, at: IVec3) -> Option<Cell> {
        self.get(at).map(|block| Cell::new(block, self.blocks))
    }

    fn fluid(&self, at: IVec3) -> Option<Fluid> {
        self.cell(at).and_then(|cell| cell.fluid)
    }

    /// The level of `fluid` at `at`, or −1 if it isn't there (1.0.0's
    /// `getFlowDecay`).
    fn level(&self, at: IVec3, fluid: Fluid) -> i32 {
        match self.cell(at) {
            Some(cell) if cell.fluid == Some(fluid) => level(cell.block) as i32,
            _ => -1,
        }
    }

    /// Whether the block at `at` holds fluid back; unloaded cells do.
    fn stops(&self, at: IVec3) -> bool {
        self.cell(at).is_none_or(|cell| cell.stops)
    }

    fn set(&mut self, at: IVec3, block: BlockState) {
        if self.edits.insert(at, block).is_none() {
            self.order.push(at);
        }
    }

    /// Sets a block and tells its neighbors (1.0.0's `setBlockWithNotify`).
    fn set_and_notify(&mut self, at: IVec3, block: BlockState) {
        let old = self.get(at);
        self.set(at, block);
        if old.map(BlockState::id) != Some(block.id()) {
            self.added(at);
        }
        self.notify_neighbors(at);
    }

    /// A fluid block just placed: lava may harden, and the fluid gets its
    /// first update (1.0.0's `onBlockAdded`).
    fn added(&mut self, at: IVec3) {
        let Some(fluid) = self.fluid(at) else {
            return;
        };
        self.harden(at);
        if self.fluid(at) == Some(fluid) {
            self.fluids.schedule(
                at,
                block(fluid),
                self.time + tick_rate(fluid, self.fluids.warm),
            );
        }
    }

    fn notify_neighbors(&mut self, at: IVec3) {
        for offset in [
            IVec3::NEG_X,
            IVec3::X,
            IVec3::NEG_Y,
            IVec3::Y,
            IVec3::NEG_Z,
            IVec3::Z,
        ] {
            self.neighbor_changed(at + offset);
        }
    }

    /// Something next to `at` changed: lava there may harden, and the
    /// fluid gets an update (1.0.0 turned still fluid into flowing here).
    fn neighbor_changed(&mut self, at: IVec3) {
        let Some(fluid) = self.fluid(at) else {
            return;
        };
        self.harden(at);
        if self.fluid(at) == Some(fluid) {
            self.fluids.schedule(
                at,
                block(fluid),
                self.time + tick_rate(fluid, self.fluids.warm),
            );
        }
    }

    /// Lava touching water on a side or from above turns to obsidian if
    /// it's a source, or cobblestone otherwise (modern `LiquidBlock`'s
    /// `shouldSpreadLiquid`; 1.0.0's `checkForHarden` left lava more than
    /// 4 levels from its source alone).
    fn harden(&mut self, at: IVec3) {
        let Some(block) = self.get(at) else {
            return;
        };
        if self.fluid(at) != Some(Fluid::Lava) {
            return;
        }
        let water_near = [IVec3::NEG_Z, IVec3::Z, IVec3::NEG_X, IVec3::X, IVec3::Y]
            .into_iter()
            .any(|offset| self.fluid(at + offset) == Some(Fluid::Water));
        if !water_near {
            return;
        }
        let hardened = if level(block) == 0 {
            OBSIDIAN
        } else {
            COBBLESTONE
        };
        self.set_and_notify(at, hardened);
    }

    /// A scheduled update of the flowing block at `at` (1.0.0's
    /// `BlockFlowing.updateTick`): find its new level from its neighbors,
    /// settle if nothing changed, then run down or spread sideways.
    fn update(&mut self, at: IVec3) {
        let Some(fluid) = self.fluid(at) else {
            return;
        };
        let step = level_step(fluid, self.fluids.warm);
        let mut level = self.level(at, fluid);
        // 1.0.0 turned a flow still when nothing changed; here it just
        // isn't scheduled again, but lava that waited its turn does
        // nothing at all.
        let mut settle = true;
        if level > 0 {
            let mut lowest = -100;
            let mut sources = 0;
            for side in SIDEWAYS {
                lowest = self.lowest_level(at + side, fluid, lowest, &mut sources);
            }
            let mut new = lowest + step;
            if new >= LAST_LEVEL || lowest < 0 {
                new = -1;
            }
            let above = self.level(at + IVec3::Y, fluid);
            if above >= 0 {
                new = if above >= FALLING {
                    above
                } else {
                    above + FALLING
                };
            }
            // Two sources beside water on firm ground, or over a source,
            // make a new source.
            if sources >= 2 && fluid == Fluid::Water {
                let below = at - IVec3::Y;
                if self.stops(below) || self.level(below, fluid) == 0 {
                    new = 0;
                }
            }
            // Lava spreads unevenly: three times in four it waits.
            if fluid == Fluid::Lava
                && level < FALLING
                && new < FALLING
                && new > level
                && (self.random)(4) != 0
            {
                new = level;
                settle = false;
            }
            if new != level {
                level = new;
                if level < 0 {
                    self.set_and_notify(at, BlockState::AIR);
                    return;
                }
                self.set(at, at_level(fluid, level as u8));
                self.fluids.schedule(
                    at,
                    block(fluid),
                    self.time + tick_rate(fluid, self.fluids.warm),
                );
                self.notify_neighbors(at);
            } else if settle {
                self.harden(at);
            }
        } else {
            self.harden(at);
        }
        if self.fluid(at) != Some(fluid) {
            // It hardened.
            return;
        }

        let below = at - IVec3::Y;
        let mut sideways = level >= 0 && (level == 0 || self.stops(below));
        if self.can_displace(below, fluid, true) {
            if fluid == Fluid::Lava && self.fluid(below) == Some(Fluid::Water) {
                // Lava falling on water makes stone.
                self.set_and_notify(below, STONE);
                return;
            }
            let falling = if level >= FALLING {
                level
            } else {
                level + FALLING
            };
            self.flow_into(below, fluid, falling, true);
            // Modern Minecraft: fluid among three sources spreads sideways
            // as well as down.
            let sources = SIDEWAYS
                .into_iter()
                .filter(|&side| self.level(at + side, fluid) == 0)
                .count();
            sideways = sources >= 3;
        }
        if sideways {
            let ways = self.flow_directions(at, fluid);
            let next = if level >= FALLING { 1 } else { level + step };
            if next >= LAST_LEVEL {
                return;
            }
            for (side, way) in SIDEWAYS.into_iter().zip(ways) {
                if way {
                    self.flow_into(at + side, fluid, next, false);
                }
            }
        }
    }

    /// The lower of `lowest` and the level at `at` (falls count as
    /// sources), counting sources (1.0.0's `getSmallestFlowDecay`).
    fn lowest_level(&self, at: IVec3, fluid: Fluid, lowest: i32, sources: &mut u32) -> i32 {
        let mut level = self.level(at, fluid);
        if level < 0 {
            return lowest;
        }
        if level == 0 {
            *sources += 1;
        }
        if level >= FALLING {
            level = 0;
        }
        if lowest >= 0 && level >= lowest {
            lowest
        } else {
            level
        }
    }

    /// Whether `fluid` may flow into `at`, `down` or sideways: not its own
    /// kind or lava, water only for lava falling onto it (modern
    /// `canBeReplacedWith`), and nothing that holds it back (1.0.0's
    /// `liquidCanDisplaceBlock`).
    fn can_displace(&self, at: IVec3, fluid: Fluid, down: bool) -> bool {
        match self.fluid(at) {
            Some(there) if there == fluid => false,
            Some(Fluid::Lava) => false,
            Some(Fluid::Water) => down,
            None => !self.stops(at),
        }
    }

    /// Flows `fluid` at `level` into `at` if it can, washing away whatever
    /// was there (1.0.0's `flowIntoBlock`): water drops it as an item, lava
    /// burns it.
    fn flow_into(&mut self, at: IVec3, fluid: Fluid, level: i32, down: bool) {
        if !self.can_displace(at, fluid, down) {
            return;
        }
        if let Some(old) = self.get(at)
            && !old.is_air()
            && fluid == Fluid::Water
            && self.fluid(at).is_none()
        {
            self.washed.push((at, old));
        }
        self.set_and_notify(at, at_level(fluid, level as u8));
    }

    /// Which sideways neighbors to spread into: those with the shortest
    /// way to a drop within 4 blocks, or all open ones if there's none
    /// (1.0.0's `getOptimalFlowDirections`).
    fn flow_directions(&self, at: IVec3, fluid: Fluid) -> [bool; 4] {
        let mut costs = [1000; 4];
        for (i, side) in SIDEWAYS.into_iter().enumerate() {
            let next = at + side;
            if self.stops(next) || self.level(next, fluid) == 0 {
                continue;
            }
            costs[i] = if self.stops(next - IVec3::Y) {
                self.flow_cost(next, fluid, 1, i)
            } else {
                0
            };
        }
        let best = *costs.iter().min().unwrap();
        costs.map(|cost| cost == best)
    }

    /// Steps from `at` to the nearest drop, not going back the way it
    /// came (1.0.0's `calculateFlowCost`).
    fn flow_cost(&self, at: IVec3, fluid: Fluid, steps: i32, from: usize) -> i32 {
        let mut best = 1000;
        for (i, side) in SIDEWAYS.into_iter().enumerate() {
            // 0 and 1, and 2 and 3, are opposite ways.
            if i == from ^ 1 {
                continue;
            }
            let next = at + side;
            if self.stops(next) || self.level(next, fluid) == 0 {
                continue;
            }
            if !self.stops(next - IVec3::Y) {
                return steps;
            }
            if steps < search_depth(fluid, self.fluids.warm) {
                best = best.min(self.flow_cost(next, fluid, steps + 1, i));
            }
        }
        best
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::blocks;
    use crate::world::chunk::ChunkPos;

    const AIR: BlockState = BlockState::AIR;
    const GLASS: BlockState = BlockState::of(blocks::GLASS);
    const FLOOR: i32 = 63;

    struct Pond {
        world: World,
        blocks: &'static BlockRegistry,
        fluids: Fluids,
        time: u64,
        seed: u32,
    }

    impl Pond {
        /// 5×5 chunks of air over a stone floor at y 60–63.
        fn new() -> Self {
            let mut world = World::default();
            for cz in -2..=2 {
                for cx in -2..=2 {
                    let mut chunk = Chunk::new(ChunkPos::new(cx, cz));
                    for z in 0..CHUNK_SIZE {
                        for x in 0..CHUNK_SIZE {
                            for y in FLOOR - 3..=FLOOR {
                                chunk.set(x, y, z, STONE);
                            }
                        }
                    }
                    world.insert(chunk);
                }
            }
            Self {
                world,
                blocks: crate::world::block::registry(),
                fluids: Fluids::default(),
                time: 0,
                seed: 7,
            }
        }

        fn get(&self, x: i32, y: i32, z: i32) -> BlockState {
            self.world.block(x, y, z).unwrap()
        }

        fn write(&mut self, at: IVec3, block: BlockState) {
            let pos = ChunkPos::of_block(at.x, at.z);
            let size = CHUNK_SIZE as i32;
            let chunk = self.world.get_mut(pos).unwrap();
            let (x, z) = (at.x.rem_euclid(size), at.z.rem_euclid(size));
            chunk.set(x as usize, at.y, z as usize, block);
        }

        /// Sets a block the way the game does, waking fluids around it.
        fn set(&mut self, x: i32, y: i32, z: i32, block: BlockState) {
            let at = IVec3::new(x, y, z);
            self.write(at, block);
            self.fluids.changed(at);
        }

        fn pour(&mut self, x: i32, y: i32, z: i32, fluid: Fluid) {
            self.set(x, y, z, at_level(fluid, 0));
        }

        fn run(&mut self, ticks: u32) -> Vec<(IVec3, BlockState)> {
            let mut washed = Vec::new();
            for _ in 0..ticks {
                let seed = &mut self.seed;
                let mut random = |n: u32| {
                    *seed = seed.wrapping_mul(1_103_515_245).wrapping_add(12345);
                    (*seed >> 16) % n
                };
                let outcome = self
                    .fluids
                    .tick(&self.world, self.blocks, self.time, &mut random);
                for (at, block) in outcome.edits {
                    self.write(at, block);
                }
                washed.extend(outcome.washed);
                self.time += 1;
            }
            washed
        }

        /// The level of the water or lava at a cell, or `None` if there's
        /// none.
        fn level(&self, x: i32, z: i32) -> Option<u8> {
            let block = self.get(x, FLOOR + 1, z);
            matches!(block.id(), blocks::WATER | blocks::LAVA).then_some(level(block))
        }
    }

    #[test]
    fn water_on_flat_ground_spreads_7_blocks_and_settles() {
        let mut pond = Pond::new();
        pond.pour(0, 64, 0, Fluid::Water);
        pond.run(300);
        assert_eq!(
            pond.get(0, 64, 0),
            BlockState::of(blocks::WATER),
            "a source"
        );
        for x in 1..=7 {
            assert_eq!(pond.level(x, 0), Some(x as u8), "x {x}");
        }
        assert_eq!(pond.level(8, 0), None);
        // A diamond: 7 steps round a corner, but no further.
        assert_eq!(pond.level(3, 4), Some(7));
        assert_eq!(pond.level(4, 4), None);
        assert_eq!(pond.fluids.pending(), 0, "everything settled");
        assert_eq!(pond.get(5, 64, -2).id(), blocks::WATER);
    }

    #[test]
    fn lava_spreads_only_3_blocks_and_slowly() {
        let mut pond = Pond::new();
        pond.pour(0, 64, 0, Fluid::Lava);
        pond.run(60);
        assert_eq!(pond.level(3, 0), None, "lava is slow");
        pond.run(3000);
        assert_eq!(pond.level(1, 0), Some(2));
        assert_eq!(pond.level(3, 0), Some(6));
        assert_eq!(pond.level(4, 0), None);
        assert_eq!(pond.get(3, 64, 0).id(), blocks::LAVA);
    }

    #[test]
    fn nether_lava_runs_7_blocks_three_times_as_fast() {
        let mut pond = Pond::new();
        pond.fluids.warm = true;
        pond.pour(0, 64, 0, Fluid::Lava);
        pond.run(60);
        assert_eq!(pond.level(3, 0), Some(3), "three blocks in 3 s");
        pond.run(3000);
        assert_eq!(pond.level(7, 0), Some(7));
        assert_eq!(pond.level(8, 0), None);
    }

    #[test]
    fn two_sources_make_a_third_and_removing_the_source_drains_the_flow() {
        let mut pond = Pond::new();
        // A trench three long.
        for x in -1..=3 {
            pond.set(x, 64, -1, GLASS);
            pond.set(x, 64, 1, GLASS);
        }
        pond.set(-1, 64, 0, GLASS);
        pond.set(3, 64, 0, GLASS);
        pond.pour(0, 64, 0, Fluid::Water);
        pond.pour(2, 64, 0, Fluid::Water);
        pond.run(40);
        assert_eq!(pond.level(1, 0), Some(0), "an infinite source");

        let mut pond = Pond::new();
        pond.pour(0, 64, 0, Fluid::Water);
        pond.run(300);
        pond.set(0, 64, 0, AIR);
        pond.run(300);
        for x in 0..=8 {
            assert_eq!(pond.level(x, 0), None, "x {x}");
        }
        assert_eq!(pond.fluids.pending(), 0);
    }

    #[test]
    fn water_runs_to_a_hole_and_fills_its_floor() {
        let mut pond = Pond::new();
        // A 3×3 hole, 2 deep, starting 3 blocks east of the source.
        for x in 3..=5 {
            for z in -1..=1 {
                pond.set(x, 63, z, AIR);
                pond.set(x, 62, z, AIR);
            }
        }
        pond.pour(0, 64, 0, Fluid::Water);
        pond.run(400);
        // The fall spreads over the bottom; like 1.0.0, falling water
        // doesn't spread sideways over water, so the layer above stays
        // open.
        for x in 3..=5 {
            for z in -1..=1 {
                let block = pond.get(x, 62, z);
                assert!(block.is(blocks::WATER), "({x}, 62, {z}) is {block}");
            }
        }
        assert_eq!(pond.get(4, 63, 0), AIR);
        // It heads for the hole instead of spreading the other way.
        assert_eq!(pond.level(-2, 0), None);
        assert_eq!(pond.level(0, 3), None);
        // Falling water is level 8 and up.
        assert!(level(pond.get(3, 63, 0)) >= 8);
    }

    #[test]
    fn water_washes_away_torches_and_flowers() {
        let mut pond = Pond::new();
        let torch = BlockState::of(blocks::TORCH);
        let flower = BlockState::of(blocks::DANDELION);
        pond.set(2, 64, 0, torch);
        pond.set(0, 64, 2, flower);
        pond.set(-2, 64, 0, GLASS);
        pond.pour(0, 64, 0, Fluid::Water);
        let mut washed = pond.run(100);
        washed.sort_by_key(|(at, _)| at.x);
        assert_eq!(
            washed,
            vec![
                (IVec3::new(0, 64, 2), flower),
                (IVec3::new(2, 64, 0), torch)
            ]
        );
        assert_eq!(pond.get(-2, 64, 0), GLASS, "glass holds it back");
    }

    #[test]
    fn lava_and_water_make_obsidian_cobblestone_and_stone() {
        // Water reaching a lava source turns it to obsidian.
        let mut pond = Pond::new();
        pond.pour(5, 64, 0, Fluid::Lava);
        pond.run(1);
        pond.pour(0, 64, 0, Fluid::Water);
        pond.run(100);
        assert_eq!(pond.get(5, 64, 0), OBSIDIAN);

        // Water poured on the tip of a lava flow turns the lava next to it
        // (level 4) to cobblestone.
        let mut pond = Pond::new();
        for x in -1..=4 {
            pond.set(x, 64, -1, GLASS);
            pond.set(x, 64, 1, GLASS);
        }
        pond.set(-1, 64, 0, GLASS);
        pond.set(4, 64, 0, GLASS);
        pond.pour(0, 64, 0, Fluid::Lava);
        pond.run(3000);
        assert_eq!(pond.level(3, 0), Some(6));
        pond.pour(3, 64, 0, Fluid::Water);
        pond.run(20);
        assert_eq!(pond.get(2, 64, 0), COBBLESTONE);
        assert_eq!(
            pond.get(0, 64, 0).id(),
            blocks::LAVA,
            "the source is boxed off"
        );

        // Lava flowing over water below turns it to stone.
        let mut pond = Pond::new();
        pond.set(2, 63, 0, BlockState::of(blocks::WATER));
        pond.pour(0, 64, 0, Fluid::Lava);
        pond.run(3000);
        assert_eq!(pond.get(2, 63, 0), STONE);
    }

    /// A trench along x at z 0 from x 0 to `end`, walled with glass.
    fn trench(pond: &mut Pond, end: i32) {
        for x in -1..=end + 1 {
            pond.set(x, 64, -1, GLASS);
            pond.set(x, 64, 1, GLASS);
        }
        pond.set(-1, 64, 0, GLASS);
        pond.set(end + 1, 64, 0, GLASS);
    }

    #[test]
    fn a_cobblestone_generator_keeps_making_cobblestone() {
        // Lava at one end of a trench, water at the other, and a hole by
        // the water so it never reaches the lava source. Whichever comes
        // first, the lava hardens where they meet, and again each time
        // the cobblestone is mined.
        for lava_first in [false, true] {
            let mut pond = Pond::new();
            trench(&mut pond, 4);
            pond.set(3, 63, 0, AIR);
            if lava_first {
                pond.pour(0, 64, 0, Fluid::Lava);
                pond.run(3000);
                assert_eq!(pond.level(3, 0), Some(6));
                pond.pour(4, 64, 0, Fluid::Water);
            } else {
                pond.pour(4, 64, 0, Fluid::Water);
                pond.run(100);
                pond.pour(0, 64, 0, Fluid::Lava);
            }
            for round in 0..6 {
                pond.run(600);
                let cobble = (1..=3).find(|&x| pond.get(x, 64, 0) == COBBLESTONE);
                let Some(x) = cobble else {
                    panic!("no cobblestone in round {round}, lava first {lava_first}");
                };
                assert_eq!(pond.get(0, 64, 0).id(), blocks::LAVA, "the source survives");
                pond.set(x, 64, 0, AIR);
            }
        }
    }

    #[test]
    fn surfaces_slope_and_flow_points_downhill() {
        let mut pond = Pond::new();
        pond.pour(0, 64, 0, Fluid::Water);
        pond.run(300);
        let blocks = pond.blocks;
        let world = &pond.world;
        let cell = |at: IVec3| world.block(at.x, at.y, at.z).map(|b| Cell::new(b, blocks));
        // The flow runs away from the source.
        let flow = flow_vector(Fluid::Water, IVec3::new(3, 64, 0), &cell);
        assert!(flow.x > 0.9, "{flow}");
        assert_eq!(
            flow_vector(Fluid::Water, IVec3::new(9, 64, 0), &cell),
            DVec3::ZERO
        );
        // A source's surface is highest; a level 7 edge is nearly flat.
        let source = corner_height(Fluid::Water, IVec3::new(1, 64, 1), &cell);
        let edge = corner_height(Fluid::Water, IVec3::new(8, 64, 1), &cell);
        assert!(source > 0.7 && source < 8.0 / 9.0 + 1e-6, "{source}");
        assert!(edge < 0.2, "{edge}");
        assert_eq!(height_percent(0), 1.0 / 9.0);
        assert_eq!(height_percent(9), 1.0 / 9.0);
        assert_eq!(height_percent(7), 8.0 / 9.0);
    }

    #[test]
    fn flowing_blocks_in_a_loaded_chunk_are_rescheduled() {
        let mut pond = Pond::new();
        pond.write(IVec3::new(0, 64, 0), BlockState::of(blocks::WATER));
        let chunk = pond.world.get(ChunkPos::new(0, 0)).unwrap().clone();
        pond.fluids.chunk_loaded(&chunk, 0);
        assert_eq!(pond.fluids.pending(), 1, "a source with room to flow");
        pond.run(100);
        assert_eq!(pond.level(3, 0), Some(3));
        // Water with nowhere to go, like an ocean, stays asleep.
        let mut pond = Pond::new();
        for y in 61..=63 {
            pond.write(IVec3::new(4, y, 4), BlockState::of(blocks::WATER));
        }
        let chunk = pond.world.get(ChunkPos::new(0, 0)).unwrap().clone();
        pond.fluids.chunk_loaded(&chunk, 0);
        assert_eq!(pond.fluids.pending(), 0);
    }
}
