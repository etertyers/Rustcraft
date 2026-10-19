//! Trees, plants, ice spikes and the like, placed per biome with shapes
//! after modern Minecraft's, and the snow and ice of cold places.
//!
//! Trees can spill leaves into the next chunk, so each chunk replays the
//! large features of its eight neighbors and keeps the blocks that land
//! inside itself. Placement only asks the pure terrain functions, never
//! another chunk, so the result doesn't depend on which chunks happen to be
//! loaded.

use super::{
    rng::Rng,
    terrain::{Area, Column, SAND, Terrain},
};
use crate::{
    config::{CHUNK_SIZE, SEA_LEVEL, WORLD_HEIGHT},
    world::{
        biome::Biome,
        block::{BlockId, BlockState, Shape},
        blocks,
        chunk::{Chunk, ChunkPos},
        props::{Prop, axis, door_half},
        structure::Bounds,
    },
};

const fn of(id: BlockId) -> BlockState {
    BlockState::of(id)
}

const SHORT_GRASS: BlockState = of(blocks::SHORT_GRASS);
const FERN: BlockState = of(blocks::FERN);
const DEAD_BUSH: BlockState = of(blocks::DEAD_BUSH);
const CACTUS: BlockState = of(blocks::CACTUS);
const SUGAR_CANE: BlockState = of(blocks::SUGAR_CANE);
const VINE: BlockState = of(blocks::VINE);
const SNOW: BlockState = of(blocks::SNOW);
const ICE: BlockState = of(blocks::ICE);
const PACKED_ICE: BlockState = of(blocks::PACKED_ICE);
const SNOW_BLOCK: BlockState = of(blocks::SNOW_BLOCK);
const WATER: BlockState = of(blocks::WATER);
const DIRT: BlockState = of(blocks::DIRT);
const ROOTED_DIRT: BlockState = of(blocks::ROOTED_DIRT);
const MUD: BlockState = of(blocks::MUD);
const LILY_PAD: BlockState = of(blocks::LILY_PAD);
const BAMBOO: BlockState = of(blocks::BAMBOO);
const MELON: BlockState = of(blocks::MELON);

const TREE_SALT: u64 = 0x7EE;
const PLANT_SALT: u64 = 0x91A;

/// Grows trees and plants, keeping out of `keep_out` (structures on the
/// surface).
pub fn populate(terrain: &Terrain, area: &Area, chunk: &mut Chunk, keep_out: &[Bounds]) {
    let center = chunk.pos;
    for dz in -1..=1 {
        for dx in -1..=1 {
            large(terrain, area, center.offset(dx, dz), chunk, keep_out);
        }
    }
    plants(terrain, area, chunk, keep_out);
    freeze(area, chunk);
}

/// Writes into `chunk` only, ignoring positions in other chunks.
struct ChunkWriter<'a> {
    chunk: &'a mut Chunk,
    min_x: i32,
    min_z: i32,
    /// Columns nothing grows in (structures).
    keep_out: &'a [Bounds],
}

impl<'a> ChunkWriter<'a> {
    fn new(chunk: &'a mut Chunk, keep_out: &'a [Bounds]) -> Self {
        let (min_x, min_z) = chunk.pos.min_block();
        Self {
            chunk,
            min_x,
            min_z,
            keep_out,
        }
    }

    /// Whether (x, z) is in or beside a structure.
    fn kept_out(&self, x: i32, z: i32) -> bool {
        self.keep_out
            .iter()
            .any(|b| b.grow(1).contains_column(x, z))
    }

    fn local(&self, x: i32, y: i32, z: i32) -> Option<(usize, usize, usize)> {
        let (lx, lz) = (x - self.min_x, z - self.min_z);
        let size = CHUNK_SIZE as i32;
        ((0..size).contains(&lx)
            && (0..size).contains(&lz)
            && (0..WORLD_HEIGHT as i32).contains(&y))
        .then_some((lx as usize, y as usize, lz as usize))
    }

    fn get(&self, x: i32, y: i32, z: i32) -> Option<BlockState> {
        let (lx, ly, lz) = self.local(x, y, z)?;
        Some(self.chunk.get(lx, ly, lz))
    }

    /// Sets the block if it's in this chunk and `replace` accepts what's there.
    fn set_if(
        &mut self,
        x: i32,
        y: i32,
        z: i32,
        block: BlockState,
        replace: fn(BlockState) -> bool,
    ) {
        if let Some((lx, ly, lz)) = self.local(x, y, z)
            && replace(self.chunk.get(lx, ly, lz))
            && !self.kept_out(x, z)
        {
            self.chunk.set(lx, ly, lz, block);
        }
    }

    fn leaves(&mut self, x: i32, y: i32, z: i32, leaves: BlockState) {
        self.set_if(x, y, z, leaves, is_replaceable);
    }

    fn log(&mut self, x: i32, y: i32, z: i32, log: BlockState) {
        self.set_if(x, y, z, log, log_replaceable);
    }
}

/// Air, water's surface and small plants, which leaves and plants may grow into.
fn is_replaceable(block: BlockState) -> bool {
    let def = block.def();
    block.is_air()
        || (def.shape == Shape::Cross && def.fluid.is_none())
        || block.id() == blocks::SNOW
}

fn is_leaves(block: BlockState) -> bool {
    block.kind() == blocks::OAK_LEAVES
}

/// Logs grow through leaves, plants and water too.
fn log_replaceable(block: BlockState) -> bool {
    is_replaceable(block)
        || is_leaves(block)
        || block.id() == blocks::WATER
        || block.id() == blocks::VINE
}

fn always(_: BlockState) -> bool {
    true
}

fn soil(block: BlockState) -> bool {
    matches!(
        block.id(),
        blocks::GRASS_BLOCK
            | blocks::DIRT
            | blocks::PODZOL
            | blocks::COARSE_DIRT
            | blocks::MUD
            | blocks::MYCELIUM
            | blocks::SAND
            | blocks::RED_SAND
            | blocks::SNOW_BLOCK
            | blocks::STONE
            | blocks::GRAVEL
    )
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Tree {
    Oak,
    FancyOak,
    Birch,
    Spruce,
    Pine,
    Jungle,
    MegaJungle,
    JungleBush,
    Acacia,
    DarkOak,
    Mangrove,
    Cherry,
    SwampOak,
    Azalea,
    BrownMushroom,
    RedMushroom,
    IceSpike,
    Iceberg,
}

/// The large features a biome grows: attempts per chunk, the chance each
/// grows, and what it grows (chosen by a roll in 0..1).
fn trees_for(biome: Biome) -> (u32, f64, fn(f64) -> Tree) {
    use Tree::*;
    match biome {
        Biome::Forest => (10, 1.0, |r| match r {
            r if r < 0.2 => Birch,
            r if r < 0.3 => FancyOak,
            _ => Oak,
        }),
        Biome::BirchForest => (10, 1.0, |_| Birch),
        Biome::DarkForest => (16, 1.0, |r| match r {
            r if r < 0.03 => BrownMushroom,
            r if r < 0.06 => RedMushroom,
            r if r < 0.2 => Oak,
            r if r < 0.25 => Birch,
            _ => DarkOak,
        }),
        Biome::Taiga => (10, 1.0, |r| if r < 0.33 { Pine } else { Spruce }),
        Biome::SnowyTaiga => (8, 1.0, |r| if r < 0.33 { Pine } else { Spruce }),
        Biome::Grove => (8, 0.9, |_| Spruce),
        Biome::WindsweptHills => (3, 0.6, |r| if r < 0.5 { Spruce } else { Oak }),
        Biome::Plains => (1, 0.1, |r| if r < 0.2 { FancyOak } else { Oak }),
        Biome::Meadow => (1, 0.05, |r| if r < 0.5 { Birch } else { FancyOak }),
        Biome::CherryGrove => (6, 0.7, |_| Cherry),
        Biome::Jungle => (30, 1.0, |r| match r {
            r if r < 0.1 => MegaJungle,
            r if r < 0.2 => FancyOak,
            r if r < 0.6 => JungleBush,
            _ => Jungle,
        }),
        Biome::BambooJungle => (12, 1.0, |r| match r {
            r if r < 0.05 => MegaJungle,
            r if r < 0.15 => FancyOak,
            _ => JungleBush,
        }),
        Biome::Savanna => (2, 0.7, |r| if r < 0.2 { Oak } else { Acacia }),
        Biome::Swamp => (2, 0.8, |_| SwampOak),
        Biome::MangroveSwamp => (25, 0.8, |_| Mangrove),
        Biome::MushroomFields => (
            1,
            0.6,
            |r| if r < 0.5 { BrownMushroom } else { RedMushroom },
        ),
        Biome::SnowyPlains => (1, 0.1, |_| Spruce),
        Biome::IceSpikes => (3, 1.0, |_| IceSpike),
        Biome::FrozenOcean | Biome::DeepFrozenOcean => (1, 0.06, |_| Iceberg),
        _ => (0, 0.0, |_| Oak),
    }
}

/// Most large-feature attempts any biome makes in a chunk (jungles').
const MAX_ATTEMPTS: u32 = 30;

/// Replays the large features of chunk `source`, writing the parts inside
/// `chunk`. Each attempt picks a column, and that column's biome says
/// whether it grows and what: attempt `i` grows only if the biome makes
/// more than `i` attempts. One more attempt grows an azalea if lush caves
/// lie below.
fn large(terrain: &Terrain, area: &Area, source: ChunkPos, chunk: &mut Chunk, keep_out: &[Bounds]) {
    let mut rng = Rng::for_chunk(terrain.seed(), source.x, source.z, TREE_SALT);
    let (min_x, min_z) = source.min_block();
    let mut writer = ChunkWriter::new(chunk, keep_out);
    for attempt in 0..=MAX_ATTEMPTS {
        // One draw per attempt keeps every later attempt's numbers the same
        // whether or not this one grows.
        let mut tree_rng = Rng::new(rng.next_u64());
        let x = min_x + tree_rng.below(CHUNK_SIZE as u32) as i32;
        let z = min_z + tree_rng.below(CHUNK_SIZE as u32) as i32;
        // Too far off to reach into this chunk.
        let Some(&column) = area.try_get(x, z) else {
            continue;
        };
        let roll = tree_rng.next_f64();
        let kind = if attempt == MAX_ATTEMPTS {
            if column.cave != Some(Biome::LushCaves) || roll > 0.4 {
                continue;
            }
            Tree::Azalea
        } else {
            let (attempts, chance, choose) = trees_for(column.biome);
            if attempt >= attempts || roll >= chance {
                continue;
            }
            choose(tree_rng.next_f64())
        };
        // A tree next to a building would grow through it.
        if keep_out.iter().any(|b| b.grow(4).contains_column(x, z)) {
            continue;
        }
        grow(&mut writer, &mut tree_rng, kind, x, z, &column);
    }
}

/// Grows one large feature at column (x, z), if the ground suits it.
fn grow(writer: &mut ChunkWriter, rng: &mut Rng, kind: Tree, x: i32, z: i32, column: &Column) {
    let base = column.height + 1;
    let fits = |height: i32| base + height + 4 < WORLD_HEIGHT as i32;
    match kind {
        Tree::IceSpike => {
            if column.top.id() == blocks::SNOW_BLOCK || column.top.id() == blocks::GRASS_BLOCK {
                ice_spike(writer, rng, x, base, z);
            }
            return;
        }
        Tree::Iceberg => {
            if column.height < SEA_LEVEL - 4 {
                iceberg(writer, rng, x, z);
            }
            return;
        }
        Tree::Mangrove => {
            // Mangroves stand in shallow water or on mud.
            if (column.top == MUD || column.height >= SEA_LEVEL - 3) && !column.entrance && fits(12)
            {
                mangrove(writer, rng, x, base, z);
            }
            return;
        }
        Tree::BrownMushroom | Tree::RedMushroom => {
            let ground = matches!(
                column.top.id(),
                blocks::MYCELIUM | blocks::GRASS_BLOCK | blocks::PODZOL
            );
            if ground && !column.entrance && column.height >= SEA_LEVEL && fits(8) {
                huge_mushroom(writer, rng, x, base, z, kind == Tree::RedMushroom);
            }
            return;
        }
        _ => {}
    }
    if !column.grassy() || column.height < SEA_LEVEL || !fits(26) {
        return;
    }
    match kind {
        Tree::Oak => {
            let height = 4 + rng.below(3) as i32;
            small(
                writer,
                rng,
                x,
                base,
                z,
                height,
                blocks::OAK_LOG,
                of(blocks::OAK_LEAVES),
                false,
            );
        }
        Tree::Birch => {
            let height = 5 + rng.below(3) as i32;
            small(
                writer,
                rng,
                x,
                base,
                z,
                height,
                blocks::BIRCH_LOG,
                of(blocks::BIRCH_LEAVES),
                false,
            );
        }
        Tree::SwampOak => {
            let height = 5 + rng.below(4) as i32;
            small(
                writer,
                rng,
                x,
                base,
                z,
                height,
                blocks::OAK_LOG,
                of(blocks::OAK_LEAVES),
                true,
            );
        }
        Tree::Jungle => {
            let height = 4 + rng.below(7) as i32;
            small(
                writer,
                rng,
                x,
                base,
                z,
                height,
                blocks::JUNGLE_LOG,
                of(blocks::JUNGLE_LEAVES),
                true,
            );
        }
        Tree::FancyOak => fancy_oak(writer, rng, x, base, z),
        Tree::Spruce => spruce(writer, rng, x, base, z, false),
        Tree::Pine => spruce(writer, rng, x, base, z, true),
        Tree::MegaJungle => mega_jungle(writer, rng, x, base, z),
        Tree::JungleBush => bush(writer, x, base, z),
        Tree::Acacia => acacia(writer, rng, x, base, z),
        Tree::DarkOak => dark_oak(writer, rng, x, base, z),
        Tree::Cherry => cherry(writer, rng, x, base, z),
        Tree::Azalea => azalea(writer, rng, x, base, z),
        _ => {}
    }
}

/// Modern's small tree (`StraightTrunkPlacer` with blob foliage): a trunk
/// with four layers of leaves, two wide and two narrow, random corners.
/// Swamp oaks and jungle trees spread wider and grow vines.
#[allow(clippy::too_many_arguments)]
fn small(
    writer: &mut ChunkWriter,
    rng: &mut Rng,
    x: i32,
    base: i32,
    z: i32,
    height: i32,
    log: BlockId,
    leaves: BlockState,
    vines: bool,
) {
    let top = base + height;
    let wide = if leaves.id() == blocks::OAK_LEAVES && vines {
        3
    } else {
        2
    };
    for y in top - 3..=top {
        let layer = y - top;
        let radius: i32 = if layer <= -2 { wide } else { 1 };
        for dx in -radius..=radius {
            for dz in -radius..=radius {
                let corner = dx.abs() == radius && dz.abs() == radius;
                if !corner || (rng.below(2) != 0 && layer != 0) {
                    writer.leaves(x + dx, y, z + dz, leaves);
                    if vines
                        && radius >= 2
                        && (dx.abs() == radius || dz.abs() == radius)
                        && rng.below(4) == 0
                    {
                        hang_vines(writer, rng, x + dx, y, z + dz, dx, dz);
                    }
                }
            }
        }
    }
    let log = of(log);
    for y in base..top {
        writer.log(x, y, z, log);
    }
    writer.set_if(x, base - 1, z, DIRT, |b| b.id() == blocks::GRASS_BLOCK);
}

/// Vines hanging from the outside of a leaf at (x, y, z), toward (dx, dz).
fn hang_vines(writer: &mut ChunkWriter, rng: &mut Rng, x: i32, y: i32, z: i32, dx: i32, dz: i32) {
    // The vine sits outside the leaf and clings to it.
    let (vx, vz, side) = if dx.abs() >= dz.abs() {
        (
            x + dx.signum(),
            z,
            if dx > 0 { Prop::West } else { Prop::East },
        )
    } else {
        (
            x,
            z + dz.signum(),
            if dz > 0 { Prop::North } else { Prop::South },
        )
    };
    let vine = VINE.with_bool(side, true);
    let length = 1 + rng.below(4) as i32;
    for k in 0..length {
        if writer.get(vx, y - k, vz).is_some_and(|b| b.is_air()) {
            writer.set_if(vx, y - k, vz, vine, |b| b.is_air());
        } else {
            break;
        }
    }
}

/// A round blob of leaves, squashed top to bottom.
fn blob(writer: &mut ChunkWriter, x: i32, y: i32, z: i32, radius: f64, leaves: BlockState) {
    let r = radius.ceil() as i32;
    for dy in -1..=1 {
        for dx in -r..=r {
            for dz in -r..=r {
                let d = (dx * dx + dz * dz) as f64 + (dy as f64 * 1.8).powi(2);
                if d <= (radius + 0.3).powi(2) {
                    writer.leaves(x + dx, y + dy, z + dz, leaves);
                }
            }
        }
    }
}

/// Modern's fancy oak: a tall trunk with branches ending in leaf clusters.
fn fancy_oak(writer: &mut ChunkWriter, rng: &mut Rng, x: i32, base: i32, z: i32) {
    let height = 7 + rng.below(5) as i32;
    let leaves = of(blocks::OAK_LEAVES);
    let log = of(blocks::OAK_LOG);
    let top = base + height;
    blob(writer, x, top, z, 2.5, leaves);
    let branches = 2 + rng.below(3);
    for _ in 0..branches {
        let start = base + height / 2 + rng.below((height / 2) as u32) as i32;
        let (dx, dz) = [
            (1, 0),
            (-1, 0),
            (0, 1),
            (0, -1),
            (1, 1),
            (-1, -1),
            (1, -1),
            (-1, 1),
        ][rng.below(8) as usize];
        let length = 2 + rng.below(2) as i32;
        let (mut bx, mut by, mut bz) = (x, start, z);
        for _ in 0..length {
            bx += dx;
            bz += dz;
            by += rng.below(2) as i32;
            let along = if dx != 0 { axis::X } else { axis::Z };
            writer.log(bx, by, bz, log.with(Prop::Axis, along));
        }
        blob(writer, bx, by + 1, bz, 2.0, leaves);
    }
    for y in base..top {
        writer.log(x, y, z, log);
    }
    writer.set_if(x, base - 1, z, DIRT, |b| b.id() == blocks::GRASS_BLOCK);
}

/// Spruce: a cone of leaf layers whose radius grows downward and resets
/// (modern `SpruceFoliagePlacer`); pines keep their leaves near the top.
fn spruce(writer: &mut ChunkWriter, rng: &mut Rng, x: i32, base: i32, z: i32, pine: bool) {
    let height = if pine {
        7 + rng.below(5) as i32
    } else {
        6 + rng.below(4) as i32
    };
    let top = base + height;
    let leaves = of(blocks::SPRUCE_LEAVES);
    let bare = if pine {
        height - 4
    } else {
        1 + rng.below(2) as i32
    };
    let (mut radius, mut widest): (i32, i32) = (0, 1);
    let most = if pine { 1 } else { 2 + rng.below(2) as i32 };
    for y in (base + bare..=top + 1).rev() {
        for dx in -radius..=radius {
            for dz in -radius..=radius {
                if radius > 0 && dx.abs() == radius && dz.abs() == radius {
                    continue;
                }
                writer.leaves(x + dx, y, z + dz, leaves);
            }
        }
        radius += 1;
        if radius > widest {
            radius = if pine { 0 } else { 1 };
            widest = (widest + 1).min(most);
        }
    }
    let log = of(blocks::SPRUCE_LOG);
    for y in base..=top {
        writer.log(x, y, z, log);
    }
    writer.set_if(x, base - 1, z, DIRT, |b| b.id() == blocks::GRASS_BLOCK);
}

/// Modern's mega jungle tree: a 2×2 trunk wrapped in vines, branches with
/// leaf clusters, and a wide crown.
fn mega_jungle(writer: &mut ChunkWriter, rng: &mut Rng, x: i32, base: i32, z: i32) {
    let height = 10 + rng.below(15) as i32;
    let top = base + height;
    let log = of(blocks::JUNGLE_LOG);
    let leaves = of(blocks::JUNGLE_LEAVES);
    for (cx, cz, r) in [(0, 0, 4.0), (0, 0, 3.0)] {
        let dy = if r < 4.0 { 1 } else { 0 };
        for dx in -4..=5 {
            for dz in -4..=5 {
                let (fx, fz) = (dx as f64 - 0.5 - cx as f64, dz as f64 - 0.5 - cz as f64);
                if fx * fx + fz * fz <= r * r {
                    writer.leaves(x + dx, top + dy, z + dz, leaves);
                }
            }
        }
    }
    blob(writer, x, top - 1, z, 3.0, leaves);
    let mut y = base + height / 2;
    while y < top - 2 {
        let (dx, dz) = [(1, 0), (-1, 0), (0, 1), (0, -1)][rng.below(4) as usize];
        let (bx, bz) = (
            x + if dx > 0 { 2 } else { dx },
            z + if dz > 0 { 2 } else { dz },
        );
        let along = if dx != 0 { axis::X } else { axis::Z };
        writer.log(bx, y, bz, log.with(Prop::Axis, along));
        blob(writer, bx + dx, y + 1, bz + dz, 2.0, leaves);
        y += 2 + rng.below(3) as i32;
    }
    for y in base..top {
        for (dx, dz) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
            writer.log(x + dx, y, z + dz, log);
        }
        // Vines on the trunk's outside faces.
        for (vx, vz, side) in [
            (-1, 0, Prop::East),
            (-1, 1, Prop::East),
            (2, 0, Prop::West),
            (2, 1, Prop::West),
            (0, -1, Prop::South),
            (1, -1, Prop::South),
            (0, 2, Prop::North),
            (1, 2, Prop::North),
        ] {
            if rng.below(3) != 0 {
                writer.set_if(x + vx, y, z + vz, VINE.with_bool(side, true), |b| {
                    b.is_air()
                });
            }
        }
    }
    for (dx, dz) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
        writer.set_if(x + dx, base - 1, z + dz, DIRT, |b| {
            b.id() == blocks::GRASS_BLOCK
        });
    }
}

/// A jungle bush: one log under a mound of oak leaves.
fn bush(writer: &mut ChunkWriter, x: i32, base: i32, z: i32) {
    let leaves = of(blocks::OAK_LEAVES);
    for (dy, radius) in [(0i32, 2i32), (1, 1)] {
        for dx in -radius..=radius {
            for dz in -radius..=radius {
                if dx.abs() + dz.abs() <= radius + 1 - dy {
                    writer.leaves(x + dx, base + dy, z + dz, leaves);
                }
            }
        }
    }
    writer.log(x, base, z, of(blocks::JUNGLE_LOG));
}

/// Modern's acacia (`ForkingTrunkPlacer`): a trunk that leans off to one
/// side and a flat crown, sometimes with a second branch.
fn acacia(writer: &mut ChunkWriter, rng: &mut Rng, x: i32, base: i32, z: i32) {
    let log = of(blocks::ACACIA_LOG);
    let leaves = of(blocks::ACACIA_LEAVES);
    let height = 5 + rng.below(3) as i32;
    let straight = 2 + rng.below(2) as i32;
    let (dx, dz) = [(1, 0), (-1, 0), (0, 1), (0, -1)][rng.below(4) as usize];
    let (mut tx, mut ty, mut tz) = (x, base, z);
    for i in 0..height {
        if i >= straight && i < straight + 3 {
            tx += dx;
            tz += dz;
        }
        writer.log(tx, ty, tz, log);
        ty += 1;
    }
    let crown = |writer: &mut ChunkWriter, cx: i32, cy: i32, cz: i32, r: i32| {
        for dx in -r..=r {
            for dz in -r..=r {
                if dx.abs() + dz.abs() <= r + 1 && !(dx.abs() == r && dz.abs() == r) {
                    writer.leaves(cx + dx, cy, cz + dz, leaves);
                }
            }
        }
        for dx in -1..=1 {
            for dz in -1..=1 {
                writer.leaves(cx + dx, cy + 1, cz + dz, leaves);
            }
        }
    };
    crown(writer, tx, ty - 1, tz, 3);
    if rng.below(2) == 0 {
        let (mut bx, mut by, mut bz) = (x, base + straight, z);
        for _ in 0..2 + rng.below(2) {
            bx -= dx;
            bz -= dz;
            by += 1;
            writer.log(bx, by, bz, log);
        }
        crown(writer, bx, by + 1, bz, 2);
    }
    writer.set_if(x, base - 1, z, DIRT, |b| b.id() == blocks::GRASS_BLOCK);
}

/// Modern's dark oak: a 2×2 trunk under a wide, flat crown.
fn dark_oak(writer: &mut ChunkWriter, rng: &mut Rng, x: i32, base: i32, z: i32) {
    let log = of(blocks::DARK_OAK_LOG);
    let leaves = of(blocks::DARK_OAK_LEAVES);
    let height = 6 + rng.below(3) as i32;
    let top = base + height;
    for (dy, r) in [(-1, 3.5), (0, 3.5), (1, 2.5)] {
        for dx in -4..=5 {
            for dz in -4..=5 {
                let (fx, fz) = (dx as f64 - 0.5, dz as f64 - 0.5);
                if fx * fx + fz * fz <= r * r && rng.below(10) != 0 {
                    writer.leaves(x + dx, top + dy, z + dz, leaves);
                }
            }
        }
    }
    for y in base..top {
        for (dx, dz) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
            writer.log(x + dx, y, z + dz, log);
        }
    }
    for (dx, dz) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
        writer.set_if(x + dx, base - 1, z + dz, DIRT, |b| {
            b.id() == blocks::GRASS_BLOCK
        });
    }
}

/// Modern's mangrove: a trunk raised on arching prop roots that reach down
/// into the mud, under a round crown.
fn mangrove(writer: &mut ChunkWriter, rng: &mut Rng, x: i32, base: i32, z: i32) {
    let log = of(blocks::MANGROVE_LOG);
    let leaves = of(blocks::MANGROVE_LEAVES);
    let roots = of(blocks::MANGROVE_ROOTS);
    let lift = 2 + rng.below(2) as i32;
    let trunk_base = base.max(SEA_LEVEL - 1) + lift;
    let height = 4 + rng.below(4) as i32;
    let top = trunk_base + height;
    blob(writer, x, top, z, 2.6, leaves);
    blob(writer, x, top + 1, z, 1.6, leaves);
    for y in trunk_base..top {
        writer.log(x, y, z, log);
    }
    // Roots: from under the trunk out and down to the ground.
    writer.set_if(x, trunk_base - 1, z, roots, log_replaceable);
    for (dx, dz) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
        if rng.below(4) == 0 {
            continue;
        }
        let reach = 1 + rng.below(2) as i32;
        let (rx, rz) = (x + dx * reach, z + dz * reach);
        for y in (base - 1..trunk_base).rev() {
            let here = writer.get(rx, y, rz);
            if here.is_some_and(|b| {
                b == MUD || b.id() == blocks::DIRT || b.id() == blocks::GRASS_BLOCK
            }) {
                writer.set_if(rx, y, rz, of(blocks::MUDDY_MANGROVE_ROOTS), always);
                break;
            }
            writer.set_if(rx, y, rz, roots, log_replaceable);
        }
        if reach == 2 {
            writer.set_if(x + dx, trunk_base - 1, z + dz, roots, log_replaceable);
        }
    }
}

/// Modern's cherry tree: a short trunk splitting into bending branches,
/// each under a wide, flat crown with leaves hanging from its rim.
fn cherry(writer: &mut ChunkWriter, rng: &mut Rng, x: i32, base: i32, z: i32) {
    let log = of(blocks::CHERRY_LOG);
    let leaves = of(blocks::CHERRY_LEAVES);
    let height = 3 + rng.below(3) as i32;
    for y in base..base + height {
        writer.log(x, y, z, log);
    }
    let branches = 1 + rng.below(2);
    let first = rng.below(4) as usize;
    for b in 0..branches {
        let (dx, dz) = [(1, 0), (0, 1), (-1, 0), (0, -1)][(first + b as usize * 2) % 4];
        let (mut bx, mut by, mut bz) = (x, base + height - 1, z);
        for step in 0..3 + rng.below(2) {
            if step < 2 {
                bx += dx;
                bz += dz;
            }
            by += 1;
            let along = if step < 2 {
                if dx != 0 { axis::X } else { axis::Z }
            } else {
                axis::Y
            };
            writer.log(bx, by, bz, log.with(Prop::Axis, along));
        }
        // The crown: two wide layers and a narrow top.
        for (dy, r) in [(0, 4.0), (1, 3.5), (2, 2.0)] {
            let ri = r as i32;
            for cx in -ri..=ri {
                for cz in -ri..=ri {
                    let d = (cx * cx + cz * cz) as f64;
                    if d <= r * r && (d < (r - 1.0).powi(2) || rng.below(3) != 0) {
                        writer.leaves(bx + cx, by + dy, bz + cz, leaves);
                        if dy == 0 && d > (r - 1.5).powi(2) && rng.below(4) == 0 {
                            writer.leaves(bx + cx, by - 1, bz + cz, leaves);
                        }
                    }
                }
            }
        }
    }
    writer.set_if(x, base - 1, z, DIRT, |b| b.id() == blocks::GRASS_BLOCK);
}

/// An azalea tree over lush caves: rooted dirt reaching down, a short
/// crooked trunk and a mound of azalea leaves, some flowering.
fn azalea(writer: &mut ChunkWriter, rng: &mut Rng, x: i32, base: i32, z: i32) {
    let log = of(blocks::OAK_LOG);
    let (plain, flowering) = (
        of(blocks::AZALEA_LEAVES),
        of(blocks::FLOWERING_AZALEA_LEAVES),
    );
    for y in base - 6..base {
        writer.set_if(x, y, z, ROOTED_DIRT, |b| {
            matches!(b.id(), blocks::DIRT | blocks::GRASS_BLOCK | blocks::STONE)
        });
    }
    let height = 2 + rng.below(2) as i32;
    let (mut tx, mut tz) = (x, z);
    for y in base..base + height {
        if y == base + height - 1 && rng.below(2) == 0 {
            tx += 1 - rng.below(3) as i32;
            tz += 1 - rng.below(3) as i32;
        }
        writer.log(tx, y, tz, log);
    }
    let top = base + height;
    for dy in -1..=1 {
        let r: i32 = if dy == 1 { 1 } else { 2 };
        for dx in -r..=r {
            for dz in -r..=r {
                if dx.abs() == r && dz.abs() == r && rng.below(2) == 0 {
                    continue;
                }
                let leaf = if rng.below(4) == 0 { flowering } else { plain };
                writer.leaves(tx + dx, top + dy, tz + dz, leaf);
            }
        }
    }
}

/// Modern's huge mushrooms: brown ones have a flat cap, red ones a dome.
fn huge_mushroom(writer: &mut ChunkWriter, rng: &mut Rng, x: i32, base: i32, z: i32, red: bool) {
    let stem = of(blocks::MUSHROOM_STEM);
    let height = 4 + rng.below(3) as i32;
    let top = base + height;
    if red {
        let cap = of(blocks::RED_MUSHROOM_BLOCK);
        for dx in -1..=1 {
            for dz in -1..=1 {
                writer.leaves(x + dx, top, z + dz, cap);
            }
        }
        for y in top - 3..top {
            for dx in -2i32..=2 {
                for dz in -2i32..=2 {
                    let rim = dx.abs() == 2 || dz.abs() == 2;
                    if rim && !(dx.abs() == 2 && dz.abs() == 2) {
                        writer.leaves(x + dx, y, z + dz, cap);
                    }
                }
            }
        }
    } else {
        let cap = of(blocks::BROWN_MUSHROOM_BLOCK);
        for dx in -3i32..=3 {
            for dz in -3i32..=3 {
                if !(dx.abs() == 3 && dz.abs() == 3) {
                    writer.leaves(x + dx, top, z + dz, cap);
                }
            }
        }
    }
    for y in base..top {
        writer.log(x, y, z, stem);
    }
}

/// Modern's ice spike: a tapering spire of packed ice, now and then a
/// very tall one.
fn ice_spike(writer: &mut ChunkWriter, rng: &mut Rng, x: i32, base: i32, z: i32) {
    let tall = rng.below(60) == 0;
    let height = if tall {
        30 + rng.below(20) as i32
    } else {
        7 + rng.below(10) as i32
    };
    let radius = height as f64 / 7.0 + rng.next_f64();
    for dy in -2..height {
        let r = radius * (1.0 - dy.max(0) as f64 / height as f64);
        let ri = r.ceil() as i32;
        for dx in -ri..=ri {
            for dz in -ri..=ri {
                let d = ((dx * dx + dz * dz) as f64).sqrt();
                if d <= r && (d < r - 0.5 || rng.below(4) != 0) {
                    writer.set_if(x + dx, base + dy, z + dz, PACKED_ICE, |b| {
                        is_replaceable(b) || soil(b)
                    });
                }
            }
        }
    }
}

/// A floating berg of packed ice capped with snow, in frozen seas.
fn iceberg(writer: &mut ChunkWriter, rng: &mut Rng, x: i32, z: i32) {
    let radius = 4.0 + rng.next_f64() * 4.0;
    let above = 3 + rng.below(6) as i32;
    let below = 4 + rng.below(5) as i32;
    let r = radius.ceil() as i32;
    for dy in -below..=above {
        let level = if dy > 0 {
            radius * (1.0 - dy as f64 / (above + 1) as f64)
        } else {
            radius * (1.0 - (-dy) as f64 / (below + 2) as f64)
        };
        for dx in -r..=r {
            for dz in -r..=r {
                let d = ((dx * dx + dz * dz) as f64).sqrt();
                if d <= level {
                    let y = SEA_LEVEL + dy;
                    let block = if dy == above || (d > level - 1.0 && dy > 0 && rng.below(3) == 0) {
                        SNOW_BLOCK
                    } else {
                        PACKED_ICE
                    };
                    writer.set_if(x + dx, y, z + dz, block, |b| {
                        b.is_air() || b.id() == blocks::WATER
                    });
                }
            }
        }
    }
}

/// What small plants a biome grows: tall grass attempts and the chance of
/// each, the share that are ferns, flower patches and their flowers.
struct Plants {
    grass: u32,
    grass_chance: f64,
    ferns: f64,
    flower_patches: u32,
    flowers: &'static [BlockId],
}

const NO_PLANTS: Plants = Plants {
    grass: 0,
    grass_chance: 0.0,
    ferns: 0.0,
    flower_patches: 0,
    flowers: &[],
};

fn plants_for(biome: Biome) -> Plants {
    use blocks::*;
    let p = |grass, grass_chance, ferns, flower_patches, flowers| Plants {
        grass,
        grass_chance,
        ferns,
        flower_patches,
        flowers,
    };
    match biome {
        Biome::Plains => p(
            32,
            0.9,
            0.0,
            4,
            &[
                DANDELION,
                POPPY,
                AZURE_BLUET,
                OXEYE_DAISY,
                CORNFLOWER,
                RED_TULIP,
                ORANGE_TULIP,
                WHITE_TULIP,
                PINK_TULIP,
            ],
        ),
        Biome::Meadow => p(
            40,
            0.95,
            0.0,
            10,
            &[
                DANDELION,
                POPPY,
                ALLIUM,
                AZURE_BLUET,
                OXEYE_DAISY,
                CORNFLOWER,
            ],
        ),
        Biome::Forest => p(
            12,
            0.4,
            0.0,
            3,
            &[
                DANDELION,
                POPPY,
                LILY_OF_THE_VALLEY,
                LILAC,
                ROSE_BUSH,
                PEONY,
            ],
        ),
        Biome::BirchForest => p(
            12,
            0.4,
            0.0,
            3,
            &[DANDELION, POPPY, LILY_OF_THE_VALLEY, LILAC, PEONY],
        ),
        Biome::DarkForest => p(
            10,
            0.4,
            0.0,
            2,
            &[LILAC, ROSE_BUSH, PEONY, BROWN_MUSHROOM, RED_MUSHROOM],
        ),
        Biome::Taiga => p(16, 0.6, 0.6, 1, &[BROWN_MUSHROOM, RED_MUSHROOM]),
        Biome::SnowyTaiga => p(6, 0.5, 0.8, 0, &[]),
        Biome::Grove => p(4, 0.3, 0.5, 0, &[]),
        Biome::Jungle => p(25, 0.8, 0.3, 1, &[DANDELION, POPPY]),
        Biome::BambooJungle => p(15, 0.7, 0.3, 0, &[]),
        Biome::Savanna => p(30, 0.9, 0.0, 1, &[DANDELION, POPPY]),
        Biome::Swamp => p(10, 0.5, 0.0, 2, &[BLUE_ORCHID, BROWN_MUSHROOM]),
        Biome::MangroveSwamp => p(4, 0.3, 0.0, 0, &[]),
        Biome::WindsweptHills => p(8, 0.5, 0.0, 1, &[DANDELION, POPPY]),
        Biome::CherryGrove => p(16, 0.6, 0.0, 6, &[PINK_PETALS]),
        Biome::SnowyPlains => p(2, 0.3, 0.0, 0, &[]),
        Biome::River => p(4, 0.5, 0.0, 0, &[]),
        Biome::MushroomFields => p(0, 0.0, 0.0, 2, &[BROWN_MUSHROOM, RED_MUSHROOM]),
        _ => NO_PLANTS,
    }
}

/// Places a two-block plant (tall grass, a sunflower...) at (x, y, z).
fn tall(writer: &mut ChunkWriter, x: i32, y: i32, z: i32, plant: BlockState) {
    let air = |b: Option<BlockState>| b.is_some_and(|b| b.is_air());
    if air(writer.get(x, y, z)) && air(writer.get(x, y + 1, z)) {
        writer.set_if(
            x,
            y,
            z,
            plant.with(Prop::DoubleBlockHalf, door_half::LOWER),
            |b| b.is_air(),
        );
        writer.set_if(
            x,
            y + 1,
            z,
            plant.with(Prop::DoubleBlockHalf, door_half::UPPER),
            |b| b.is_air(),
        );
    }
}

/// Places a one- or two-block plant on top of its column.
fn plant(writer: &mut ChunkWriter, x: i32, y: i32, z: i32, plant: BlockState) {
    if plant.def().placement == crate::world::block::Placement::Tall {
        tall(writer, x, y, z, plant);
    } else {
        writer.set_if(x, y, z, plant, |b| b.is_air());
    }
}

/// Plants stay inside their own chunk, so only this chunk's are placed.
fn plants(terrain: &Terrain, area: &Area, chunk: &mut Chunk, keep_out: &[Bounds]) {
    let pos = chunk.pos;
    let mut rng = Rng::for_chunk(terrain.seed(), pos.x, pos.z, PLANT_SALT);
    let (min_x, min_z) = pos.min_block();
    let biome = area.get(min_x + 8, min_z + 8).biome;
    let kinds = plants_for(biome);
    let mut writer = ChunkWriter::new(chunk, keep_out);
    let pick = |rng: &mut Rng| {
        (
            min_x + rng.below(CHUNK_SIZE as u32) as i32,
            min_z + rng.below(CHUNK_SIZE as u32) as i32,
        )
    };

    // Grass and ferns, a tenth of them two blocks tall.
    for _ in 0..kinds.grass {
        let (x, z) = pick(&mut rng);
        let (roll, fern, tall_one) = (rng.next_f64(), rng.next_f64(), rng.below(10) == 0);
        let column = area.get(x, z);
        if roll < kinds.grass_chance && column.grassy() && column.height >= SEA_LEVEL {
            let block = match (fern < kinds.ferns, tall_one) {
                (true, true) => of(blocks::LARGE_FERN),
                (true, false) => FERN,
                (false, true) => of(blocks::TALL_GRASS),
                (false, false) => SHORT_GRASS,
            };
            plant(&mut writer, x, column.height + 1, z, block);
        }
    }

    // Flower patches.
    for _ in 0..kinds.flower_patches {
        let (cx, cz) = pick(&mut rng);
        let flower = of(kinds.flowers[rng.below(kinds.flowers.len() as u32) as usize]);
        let mushroom = matches!(flower.id(), blocks::BROWN_MUSHROOM | blocks::RED_MUSHROOM);
        for _ in 0..8 {
            let x = (cx + rng.below(7) as i32 - 3).clamp(min_x, min_x + 15);
            let z = (cz + rng.below(7) as i32 - 3).clamp(min_z, min_z + 15);
            let column = area.get(x, z);
            let ground = column.grassy() || (mushroom && column.top.id() == blocks::MYCELIUM);
            if ground && column.height >= SEA_LEVEL {
                plant(&mut writer, x, column.height + 1, z, flower);
            }
        }
    }

    // Deserts and badlands: dead bushes and cactus on open sand.
    let dry = matches!(biome, Biome::Desert | Biome::Badlands);
    for _ in 0..if dry { 4 } else { 0 } {
        let (x, z) = pick(&mut rng);
        let column = area.get(x, z);
        if matches!(column.biome, Biome::Desert | Biome::Badlands) && on_open_ground(column) {
            writer.set_if(x, column.height + 1, z, DEAD_BUSH, |b| b.is_air());
        }
    }
    for _ in 0..match biome {
        Biome::Desert => 10,
        Biome::Badlands => 5,
        _ => 0,
    } {
        let (x, z) = pick(&mut rng);
        let spread = rng.below(3) + 1;
        let height = 1 + rng.below(spread) as i32;
        let column = area.get(x, z);
        let clear_sides = [(1, 0), (-1, 0), (0, 1), (0, -1)].iter().all(|&(dx, dz)| {
            area.try_get(x + dx, z + dz)
                .is_some_and(|c| c.height <= column.height)
                && (1..=height).all(|dy| {
                    writer
                        .get(x + dx, column.height + dy, z + dz)
                        .is_none_or(|b| b.is_air())
                })
        });
        let sandy = matches!(column.top.id(), blocks::SAND | blocks::RED_SAND);
        if sandy && on_open_ground(column) && clear_sides {
            for dy in 1..=height {
                writer.set_if(x, column.height + dy, z, CACTUS, |b| b.is_air());
            }
        }
    }

    // Sugar cane on shores, next to water.
    for _ in 0..20 {
        let (x, z) = pick(&mut rng);
        let spread = rng.below(3) + 1;
        let height = 2 + rng.below(spread) as i32;
        let column = area.get(x, z);
        let shore = column.height == SEA_LEVEL
            && matches!(
                column.top.id(),
                blocks::GRASS_BLOCK | blocks::DIRT | blocks::SAND | blocks::PODZOL | blocks::MUD
            )
            && !column.entrance;
        let by_water = [(1, 0), (-1, 0), (0, 1), (0, -1)].iter().any(|&(dx, dz)| {
            area.try_get(x + dx, z + dz)
                .is_some_and(|c| c.height < SEA_LEVEL)
        });
        if shore && by_water {
            for dy in 1..=height {
                writer.set_if(x, column.height + dy, z, SUGAR_CANE, |b| b.is_air());
            }
        }
    }

    // Bamboo in bamboo jungles (and a little in jungles).
    for _ in 0..match biome {
        Biome::BambooJungle => 40,
        Biome::Jungle => 3,
        _ => 0,
    } {
        let (x, z) = pick(&mut rng);
        let height = 4 + rng.below(11) as i32;
        let column = area.get(x, z);
        if column.grassy() && column.height >= SEA_LEVEL {
            for dy in 1..=height {
                if writer
                    .get(x, column.height + dy, z)
                    .is_none_or(|b| !b.is_air())
                {
                    break;
                }
                writer.set_if(x, column.height + dy, z, BAMBOO, |b| b.is_air());
            }
        }
    }

    // Melons in jungles.
    if matches!(biome, Biome::Jungle | Biome::BambooJungle) && rng.below(4) == 0 {
        let (cx, cz) = pick(&mut rng);
        for _ in 0..6 {
            let x = (cx + rng.below(5) as i32 - 2).clamp(min_x, min_x + 15);
            let z = (cz + rng.below(5) as i32 - 2).clamp(min_z, min_z + 15);
            let column = area.get(x, z);
            if column.grassy() && column.height >= SEA_LEVEL {
                writer.set_if(x, column.height + 1, z, MELON, |b| {
                    b.is_air() || b.def().replaceable
                });
            }
        }
    }

    // Lily pads on swamp water.
    for _ in 0..if biome == Biome::Swamp { 6 } else { 0 } {
        let (x, z) = pick(&mut rng);
        let column = area.get(x, z);
        if column.height < SEA_LEVEL
            && writer.get(x, SEA_LEVEL, z) == Some(WATER)
            && writer.get(x, SEA_LEVEL + 1, z).is_some_and(|b| b.is_air())
        {
            writer.set_if(x, SEA_LEVEL + 1, z, LILY_PAD, |b| b.is_air());
        }
    }

    // A rare patch of sunflowers on the plains.
    if biome == Biome::Plains && rng.below(12) == 0 {
        let (cx, cz) = pick(&mut rng);
        for _ in 0..10 {
            let x = (cx + rng.below(7) as i32 - 3).clamp(min_x, min_x + 15);
            let z = (cz + rng.below(7) as i32 - 3).clamp(min_z, min_z + 15);
            let column = area.get(x, z);
            if column.grassy() && column.height >= SEA_LEVEL {
                tall(&mut writer, x, column.height + 1, z, of(blocks::SUNFLOWER));
            }
        }
    }
}

/// Open sand, red sand or terracotta above sea level, for desert plants.
fn on_open_ground(column: &Column) -> bool {
    let dry = column.top == SAND
        || column.top.id() == blocks::RED_SAND
        || column.top.id().name().ends_with("terracotta");
    dry && column.height >= SEA_LEVEL && !column.entrance
}

/// Snow settles and water freezes where it's cold enough (modern
/// `FreezeTopLayer`): ice on the top water, a snow layer on top of
/// anything with a full top, and snowy grass under it.
fn freeze(area: &Area, chunk: &mut Chunk) {
    let (min_x, min_z) = chunk.pos.min_block();
    for z in 0..CHUNK_SIZE {
        for x in 0..CHUNK_SIZE {
            let column = area.get(min_x + x as i32, min_z + z as i32);
            let start = (column.height.max(SEA_LEVEL) + 40).min(WORLD_HEIGHT as i32 - 2);
            let Some(y) = (1..=start)
                .rev()
                .find(|&y| !chunk.get(x, y as usize, z).is_air())
            else {
                continue;
            };
            let block = chunk.get(x, y as usize, z);
            if block == WATER {
                if column.biome.is_cold_at(y) {
                    chunk.set(x, y as usize, z, ICE);
                }
                continue;
            }
            if !column.biome.is_cold_at(y + 1) {
                continue;
            }
            let def = block.def();
            let holds_snow = (def.is_opaque_cube() || is_leaves(block))
                && !matches!(block.id(), blocks::ICE | blocks::PACKED_ICE);
            if holds_snow {
                chunk.set(x, y as usize + 1, z, SNOW);
                if block.def().props.contains(&Prop::Snowy) {
                    chunk.set(x, y as usize, z, block.with_bool(Prop::Snowy, true));
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::worldgen::terrain::Terrain;
    use std::collections::HashMap;

    /// A chunk whose center column and the centers of its eight neighbors
    /// are all `biome`.
    fn find_biome(terrain: &Terrain, biome: Biome) -> ChunkPos {
        (0..120_000)
            .map(|i: i32| {
                // A coarse square spiral of chunk positions.
                let r = (i as f64).sqrt() as i32;
                ChunkPos::new(
                    (i % (2 * r + 1)) * 3 - r * 3,
                    r * 3 * if i % 2 == 0 { 1 } else { -1 },
                )
            })
            .find(|pos| {
                let (x, z) = pos.min_block();
                (-1..=1).all(|dx| {
                    (-1..=1)
                        .all(|dz| terrain.column(x + 8 + dx * 16, z + 8 + dz * 16).biome == biome)
                })
            })
            .unwrap_or_else(|| panic!("no {biome:?} found"))
    }

    /// Counts every block in the chunks around `center` by id.
    fn census(
        terrain: &Terrain,
        center: ChunkPos,
        radius: i32,
    ) -> (HashMap<ChunkPos, Chunk>, HashMap<BlockId, usize>) {
        let mut chunks = HashMap::new();
        let mut counts = HashMap::new();
        for dz in -radius..=radius {
            for dx in -radius..=radius {
                let pos = center.offset(dx, dz);
                let chunk = terrain.generate(pos);
                for y in SEA_LEVEL as usize - 4..WORLD_HEIGHT {
                    for i in 0..256 {
                        *counts.entry(chunk.get(i % 16, y, i / 16).id()).or_insert(0) += 1;
                    }
                }
                chunks.insert(pos, chunk);
            }
        }
        (chunks, counts)
    }

    fn n(counts: &HashMap<BlockId, usize>, id: BlockId) -> usize {
        counts.get(&id).copied().unwrap_or(0)
    }

    #[test]
    fn each_wooded_biome_grows_its_own_trees() {
        let terrain = Terrain::new(20111118);
        for (biome, log, leaves) in [
            (Biome::Forest, blocks::OAK_LOG, blocks::OAK_LEAVES),
            (Biome::BirchForest, blocks::BIRCH_LOG, blocks::BIRCH_LEAVES),
            (Biome::Taiga, blocks::SPRUCE_LOG, blocks::SPRUCE_LEAVES),
            (Biome::SnowyTaiga, blocks::SPRUCE_LOG, blocks::SPRUCE_LEAVES),
            (Biome::Jungle, blocks::JUNGLE_LOG, blocks::JUNGLE_LEAVES),
            (Biome::Savanna, blocks::ACACIA_LOG, blocks::ACACIA_LEAVES),
            (
                Biome::DarkForest,
                blocks::DARK_OAK_LOG,
                blocks::DARK_OAK_LEAVES,
            ),
            (
                Biome::MangroveSwamp,
                blocks::MANGROVE_LOG,
                blocks::MANGROVE_LEAVES,
            ),
            (
                Biome::CherryGrove,
                blocks::CHERRY_LOG,
                blocks::CHERRY_LEAVES,
            ),
        ] {
            let (_, counts) = census(&terrain, find_biome(&terrain, biome), 1);
            assert!(
                n(&counts, log) > 10,
                "{biome:?}: {} {}",
                n(&counts, log),
                log.name()
            );
            assert!(
                n(&counts, leaves) > 30,
                "{biome:?}: {} {}",
                n(&counts, leaves),
                leaves.name()
            );
        }
    }

    #[test]
    fn open_biomes_have_few_or_no_trees() {
        let terrain = Terrain::new(20111118);
        for biome in [
            Biome::Desert,
            Biome::Badlands,
            Biome::IceSpikes,
            Biome::Beach,
        ] {
            let (_, counts) = census(&terrain, find_biome(&terrain, biome), 1);
            let logs: usize = [
                blocks::OAK_LOG,
                blocks::SPRUCE_LOG,
                blocks::BIRCH_LOG,
                blocks::ACACIA_LOG,
            ]
            .iter()
            .map(|&id| n(&counts, id))
            .sum();
            assert_eq!(logs, 0, "{biome:?} has trees");
        }
    }

    #[test]
    fn biomes_grow_their_plants_and_features() {
        let terrain = Terrain::new(20111118);
        let check = |biome: Biome, ids: &[BlockId], min: usize| {
            let (_, counts) = census(&terrain, find_biome(&terrain, biome), 1);
            let total: usize = ids.iter().map(|&id| n(&counts, id)).sum();
            assert!(total >= min, "{biome:?}: {total} of {ids:?}");
        };
        check(Biome::Desert, &[blocks::CACTUS], 1);
        check(Biome::Desert, &[blocks::DEAD_BUSH], 1);
        check(Biome::Badlands, &[blocks::DEAD_BUSH, blocks::CACTUS], 1);
        check(
            Biome::Plains,
            &[blocks::SHORT_GRASS, blocks::TALL_GRASS],
            50,
        );
        check(
            Biome::Meadow,
            &[
                blocks::ALLIUM,
                blocks::AZURE_BLUET,
                blocks::OXEYE_DAISY,
                blocks::CORNFLOWER,
                blocks::DANDELION,
                blocks::POPPY,
            ],
            10,
        );
        check(Biome::Taiga, &[blocks::FERN, blocks::LARGE_FERN], 5);
        check(Biome::BambooJungle, &[blocks::BAMBOO], 40);
        check(Biome::IceSpikes, &[blocks::PACKED_ICE], 50);
        check(
            Biome::MushroomFields,
            &[blocks::RED_MUSHROOM_BLOCK, blocks::BROWN_MUSHROOM_BLOCK],
            10,
        );
        check(Biome::MangroveSwamp, &[blocks::MANGROVE_ROOTS], 5);
        check(Biome::CherryGrove, &[blocks::PINK_PETALS], 5);
        check(
            Biome::Swamp,
            &[blocks::VINE, blocks::LILY_PAD, blocks::BLUE_ORCHID],
            3,
        );
    }

    #[test]
    fn tall_plants_have_both_halves() {
        let terrain = Terrain::new(20111118);
        let (chunks, _) = census(&terrain, find_biome(&terrain, Biome::Plains), 2);
        let mut found = 0;
        for chunk in chunks.values() {
            for y in SEA_LEVEL as usize..WORLD_HEIGHT - 1 {
                for i in 0..256 {
                    let block = chunk.get(i % 16, y, i / 16);
                    if block.id() == blocks::TALL_GRASS
                        && block.get(Prop::DoubleBlockHalf) == door_half::LOWER
                    {
                        let above = chunk.get(i % 16, y + 1, i / 16);
                        assert_eq!(above.id(), blocks::TALL_GRASS);
                        assert_eq!(above.get(Prop::DoubleBlockHalf), door_half::UPPER);
                        found += 1;
                    }
                }
            }
        }
        assert!(found > 0, "no tall grass on the plains");
    }

    #[test]
    fn trees_on_a_chunk_border_keep_their_leaves_in_the_next_chunk() {
        let terrain = Terrain::new(20111118);
        let center = find_biome(&terrain, Biome::Forest);
        let (chunks, _) = census(&terrain, center, 2);
        let mut checked = 0;
        for (&pos, chunk) in &chunks {
            let Some(east) = chunks.get(&pos.offset(1, 0)) else {
                continue;
            };
            for z in 0..16 {
                for y in SEA_LEVEL as usize..WORLD_HEIGHT - 1 {
                    // The top log of a small oak on the east edge of the chunk.
                    let log = chunk.get(15, y, z);
                    if log.id() != blocks::OAK_LOG
                        || chunk.get(15, y + 1, z).id() != blocks::OAK_LEAVES
                    {
                        continue;
                    }
                    // Leaves one block east of the top log are never a random
                    // corner, so they must be in the neighbor chunk.
                    let beside = east.get(0, y, z);
                    assert!(
                        is_leaves(beside) || beside.id() == blocks::OAK_LOG,
                        "{beside:?} beside a tree top"
                    );
                    checked += 1;
                }
            }
        }
        assert!(checked > 0, "no border trees found to check");
    }
}
