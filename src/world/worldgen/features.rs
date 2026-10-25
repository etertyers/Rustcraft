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
use crate::world::fast_hash::FastSet;
use crate::{
    config::{CHUNK_SIZE, SEA_LEVEL},
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
    /// `None` for a dry run, which writes nothing and only records logs.
    chunk: Option<&'a mut Chunk>,
    min_x: i32,
    min_z: i32,
    /// Columns nothing grows in (structures).
    keep_out: &'a [Bounds],
    /// Every log a dry run would place, wherever it is.
    logs: Vec<(i32, i32, i32)>,
}

impl<'a> ChunkWriter<'a> {
    fn new(chunk: &'a mut Chunk, keep_out: &'a [Bounds]) -> Self {
        let (min_x, min_z) = chunk.pos.min_block();
        Self {
            chunk: Some(chunk),
            min_x,
            min_z,
            keep_out,
            logs: Vec::new(),
        }
    }

    /// A writer that only records where a tree's logs would go. It sees no
    /// blocks, so only what's below a tree's base (roots) can differ from
    /// the real thing.
    fn dry() -> Self {
        Self {
            chunk: None,
            min_x: 0,
            min_z: 0,
            keep_out: &[],
            logs: Vec::new(),
        }
    }

    /// Whether (x, z) is in or beside a structure.
    fn kept_out(&self, x: i32, z: i32) -> bool {
        self.keep_out
            .iter()
            .any(|b| b.grow(1).contains_column(x, z))
    }

    fn local(&self, x: i32, y: i32, z: i32) -> Option<(usize, i32, usize)> {
        let (lx, lz) = (x - self.min_x, z - self.min_z);
        let size = CHUNK_SIZE as i32;
        (self.chunk.is_some()
            && (0..size).contains(&lx)
            && (0..size).contains(&lz)
            && crate::world::chunk::in_height(y))
        .then_some((lx as usize, y, lz as usize))
    }

    fn get(&self, x: i32, y: i32, z: i32) -> Option<BlockState> {
        let (lx, ly, lz) = self.local(x, y, z)?;
        Some(self.chunk.as_ref()?.get(lx, ly, lz))
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
        let kept_out = self.kept_out(x, z);
        if let Some((lx, ly, lz)) = self.local(x, y, z)
            && let Some(chunk) = self.chunk.as_mut()
            && replace(chunk.get(lx, ly, lz))
            && !kept_out
        {
            chunk.set(lx, ly, lz, block);
        }
    }

    fn leaves(&mut self, x: i32, y: i32, z: i32, leaves: BlockState) {
        self.set_if(x, y, z, leaves, is_replaceable);
    }

    fn log(&mut self, x: i32, y: i32, z: i32, log: BlockState) {
        if self.chunk.is_none() {
            self.logs.push((x, y, z));
        }
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
    /// Vanilla's `super_birch`: a birch up to 13 tall.
    TallBirch,
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

impl Tree {
    /// The columns its trunk stands on, from its (x, z).
    fn trunk_width(self) -> i32 {
        match self {
            Tree::MegaJungle | Tree::DarkOak => 2,
            _ => 1,
        }
    }
}

/// One biome's trees, after vanilla 1.21's placed tree features: `count`
/// attempts in each chunk plus one more with chance `extra`, each at a
/// random column of the chunk.
struct TreeRow {
    biome: Biome,
    count: u32,
    extra: f64,
    /// Deepest water a trunk may stand in.
    water: i32,
    /// One attempt in each 4 × 4 cell instead of anywhere (dark forests).
    grid: bool,
    /// Vanilla's random selector: each (chance, tree) is rolled in turn,
    /// and `default` grows if none comes up.
    mix: &'static [(f64, Tree)],
    default: Tree,
}

const fn row(
    biome: Biome,
    count: u32,
    extra: f64,
    mix: &'static [(f64, Tree)],
    default: Tree,
) -> TreeRow {
    TreeRow {
        biome,
        count,
        extra,
        water: 0,
        grid: false,
        mix,
        default,
    }
}

const JUNGLE_MIX: &[(f64, Tree)] = &[
    (0.1, Tree::FancyOak),
    (0.5, Tree::JungleBush),
    (0.33, Tree::MegaJungle),
];
const TAIGA_MIX: &[(f64, Tree)] = &[(0.33, Tree::Pine)];

/// Every biome's trees (and the ice spikes and icebergs that place like
/// them). Biomes Rustcraft lacks generate as their fallbacks (see
/// `docs/worldgen.md`), so they use the fallback's row; biomes not listed
/// grow none.
const TREES: &[TreeRow] = {
    use Biome as B;
    use Tree::*;
    &[
        row(B::Plains, 0, 0.05, &[(0.33, FancyOak)], Oak),
        // Thirds: oak, fancy oak, tall birch.
        row(
            B::Meadow,
            0,
            0.01,
            &[(1.0 / 3.0, Oak), (0.5, FancyOak)],
            TallBirch,
        ),
        row(B::Forest, 10, 0.1, &[(0.2, Birch), (0.1, FancyOak)], Oak),
        row(B::BirchForest, 10, 0.1, &[], Birch),
        TreeRow {
            grid: true,
            ..row(
                B::DarkForest,
                16,
                0.0,
                &[
                    (0.025, BrownMushroom),
                    (0.05, RedMushroom),
                    (2.0 / 3.0, DarkOak),
                    (0.2, Birch),
                    (0.1, FancyOak),
                ],
                Oak,
            )
        },
        row(B::Taiga, 10, 0.1, TAIGA_MIX, Spruce),
        row(B::SnowyTaiga, 10, 0.1, TAIGA_MIX, Spruce),
        row(B::Grove, 10, 0.1, &[], Pine),
        row(B::SnowyPlains, 0, 0.1, &[], Spruce),
        row(B::WindsweptHills, 0, 0.1, &[(0.66, Spruce)], Oak),
        row(B::Savanna, 1, 0.1, &[(0.8, Acacia)], Oak),
        row(B::Jungle, 50, 0.1, JUNGLE_MIX, Jungle),
        row(B::BambooJungle, 30, 0.1, JUNGLE_MIX, Jungle),
        TreeRow {
            water: 2,
            ..row(B::Swamp, 2, 0.1, &[], SwampOak)
        },
        TreeRow {
            water: 5,
            ..row(B::MangroveSwamp, 25, 0.0, &[], Mangrove)
        },
        row(B::CherryGrove, 10, 0.1, &[], Cherry),
        row(
            B::MushroomFields,
            1,
            0.0,
            &[(0.5, BrownMushroom)],
            RedMushroom,
        ),
        row(B::IceSpikes, 3, 0.0, &[], IceSpike),
        row(B::FrozenOcean, 0, 0.06, &[], Iceberg),
        row(B::DeepFrozenOcean, 0, 0.06, &[], Iceberg),
    ]
};

impl TreeRow {
    fn pick(&self, rng: &mut Rng) -> Tree {
        self.mix
            .iter()
            .find(|&&(chance, _)| rng.next_f64() < chance)
            .map_or(self.default, |&(_, tree)| tree)
    }
}

/// A large feature that passed every check, with the numbers it grows by.
struct Planted {
    kind: Tree,
    x: i32,
    z: i32,
    base: i32,
    column: Column,
    rng: Rng,
}

/// Where a tree of `kind` would stand on `column`: the Y above its ground,
/// or `None` if the ground or the water over it doesn't suit it.
fn base_on(kind: Tree, column: &Column, water: i32) -> Option<i32> {
    if matches!(kind, Tree::IceSpike | Tree::Iceberg) {
        // These check their own ground when they grow.
        return Some(column.height + 1);
    }
    // A swamp pool's top block is water over the filler `Terrain::layer`
    // puts under it.
    let (ground_y, ground) = if column.top == WATER {
        let under = if column.biome == Biome::MangroveSwamp {
            MUD
        } else {
            DIRT
        };
        (column.height - 1, under)
    } else {
        (column.height, column.top)
    };
    let soil = match kind {
        Tree::Mangrove => matches!(
            ground.id(),
            blocks::GRASS_BLOCK | blocks::DIRT | blocks::PODZOL | blocks::MUD
        ),
        Tree::BrownMushroom | Tree::RedMushroom => matches!(
            ground.id(),
            blocks::MYCELIUM | blocks::GRASS_BLOCK | blocks::PODZOL
        ),
        _ => matches!(
            ground.id(),
            blocks::GRASS_BLOCK | blocks::DIRT | blocks::PODZOL
        ),
    };
    (soil && !column.entrance && SEA_LEVEL - ground_y <= water).then_some(ground_y + 1)
}

/// Farthest any large feature reaches from its trunk.
const REACH: i32 = 12;

/// Salt for the azalea attempt over lush caves.
const AZALEA_SALT: u64 = 0xA2A;

/// Decides chunk `source`'s large features. Each biome row makes its
/// attempts at columns inside `source`; an attempt is dropped (not
/// retried) if the column is another biome, its ground or water doesn't
/// suit the tree, or the trunk would run into an earlier tree's logs or
/// the ground. Columns outside `area` come from the terrain, so every
/// chunk that replays `source` gets the same list.
fn plan(terrain: &Terrain, area: Option<&Area>, source: ChunkPos) -> Vec<Planted> {
    let near = |x: i32, z: i32| area.and_then(|area| area.try_get(x, z));
    let column_at = |x: i32, z: i32| near(x, z).copied().unwrap_or_else(|| terrain.column(x, z));
    let (min_x, min_z) = source.min_block();
    // Surface biomes are per 4 × 4 quart, so the chunk's sixteen say where
    // each biome is.
    let quarts: [Biome; 16] = std::array::from_fn(|i| {
        let (x, z) = (min_x + i as i32 % 4 * 4, min_z + i as i32 / 4 * 4);
        near(x, z).map_or_else(|| terrain.biome_at(x, z), |c| c.biome)
    });
    let biome_at = |x: i32, z: i32| quarts[((z - min_z) / 4 * 4 + (x - min_x) / 4) as usize];
    let mut dry = ChunkWriter::dry();
    let mut logs: FastSet<(i32, i32, i32)> = FastSet::default();
    let mut planted = Vec::new();
    let mut attempt = |kind: Tree, x: i32, z: i32, water: i32, rng: Rng| {
        let column = column_at(x, z);
        let Some(base) = base_on(kind, &column, water) else {
            return;
        };
        dry.logs.clear();
        if !grow(&mut dry, &mut rng.clone(), kind, x, base, z, &column) {
            return;
        }
        let width = kind.trunk_width();
        let in_trunk = |&&(lx, ly, lz): &&(i32, i32, i32)| {
            ly >= base && (x..x + width).contains(&lx) && (z..z + width).contains(&lz)
        };
        let blocked = dry.logs.iter().filter(in_trunk).any(|&(lx, ly, lz)| {
            let under = if (lx, lz) == (x, z) {
                base - 1
            } else {
                let c = column_at(lx, lz);
                c.height - i32::from(c.top == WATER)
            };
            ly <= under || logs.contains(&(lx, ly, lz))
        });
        if blocked {
            return;
        }
        // Roots below the base depend on the blocks there, which a dry run
        // can't see; they don't block anything.
        logs.extend(dry.logs.iter().filter(|p| p.1 >= base));
        planted.push(Planted {
            kind,
            x,
            z,
            base,
            column,
            rng,
        });
    };
    for row in TREES.iter().filter(|row| quarts.contains(&row.biome)) {
        let salt = TREE_SALT ^ ((row.biome as u64) << 16);
        let mut rng = Rng::for_chunk(terrain.seed(), source.x, source.z, salt);
        let attempts = row.count + u32::from(rng.next_f64() < row.extra);
        for i in 0..attempts as i32 {
            // One draw per attempt keeps every later attempt's numbers the
            // same whether or not this one grows.
            let mut tree_rng = Rng::new(rng.next_u64());
            let (x, z) = if row.grid {
                (
                    min_x + i % 4 * 4 + tree_rng.below(4) as i32,
                    min_z + i / 4 * 4 + tree_rng.below(4) as i32,
                )
            } else {
                (
                    min_x + tree_rng.below(CHUNK_SIZE as u32) as i32,
                    min_z + tree_rng.below(CHUNK_SIZE as u32) as i32,
                )
            };
            if biome_at(x, z) != row.biome {
                continue;
            }
            let kind = row.pick(&mut tree_rng);
            attempt(kind, x, z, row.water, tree_rng);
        }
    }
    // An azalea tree over lush caves.
    let mut rng = Rng::for_chunk(terrain.seed(), source.x, source.z, AZALEA_SALT);
    let x = min_x + rng.below(CHUNK_SIZE as u32) as i32;
    let z = min_z + rng.below(CHUNK_SIZE as u32) as i32;
    if rng.next_f64() < 0.4 && column_at(x, z).cave == Some(Biome::LushCaves) {
        attempt(Tree::Azalea, x, z, 0, rng);
    }
    planted
}

/// Grows the large features of chunk `source`, writing the parts inside
/// `chunk`.
fn large(terrain: &Terrain, area: &Area, source: ChunkPos, chunk: &mut Chunk, keep_out: &[Bounds]) {
    let mut writer = ChunkWriter::new(chunk, keep_out);
    let (min_x, min_z) = writer.chunk.as_ref().map_or((0, 0), |c| c.pos.min_block());
    let size = CHUNK_SIZE as i32;
    for mut tree in plan(terrain, Some(area), source) {
        let off = |v: i32, min: i32| (min - v).max(v - (min + size - 1));
        if off(tree.x, min_x).max(off(tree.z, min_z)) > REACH {
            continue;
        }
        // A tree next to a building would grow through it.
        if keep_out
            .iter()
            .any(|b| b.grow(4).contains_column(tree.x, tree.z))
        {
            continue;
        }
        grow(
            &mut writer,
            &mut tree.rng,
            tree.kind,
            tree.x,
            tree.base,
            tree.z,
            &tree.column,
        );
    }
}

/// Grows one large feature at column (x, z) from `base` up. Returns false
/// if it doesn't fit (too near the top of the world, or an ice spike or
/// iceberg on the wrong ground).
fn grow(
    writer: &mut ChunkWriter,
    rng: &mut Rng,
    kind: Tree,
    x: i32,
    base: i32,
    z: i32,
    column: &Column,
) -> bool {
    let fits = |height: i32| base + height + 4 < crate::config::MAX_Y;
    match kind {
        Tree::IceSpike => {
            let ground =
                column.top.id() == blocks::SNOW_BLOCK || column.top.id() == blocks::GRASS_BLOCK;
            if ground {
                ice_spike(writer, rng, x, base, z);
            }
            return ground;
        }
        Tree::Iceberg => {
            let deep = column.height < SEA_LEVEL - 4;
            if deep {
                iceberg(writer, rng, x, z);
            }
            return deep;
        }
        Tree::Mangrove => {
            if fits(12) {
                mangrove(writer, rng, x, base, z);
            }
            return fits(12);
        }
        Tree::BrownMushroom | Tree::RedMushroom => {
            if fits(8) {
                huge_mushroom(writer, rng, x, base, z, kind == Tree::RedMushroom);
            }
            return fits(8);
        }
        _ => {}
    }
    if !fits(26) {
        return false;
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
        Tree::TallBirch => {
            let height = 5 + rng.below(3) as i32 + rng.below(7) as i32;
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
    true
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
/// Runs a 2 × 2 trunk down to the ground: a big tree's base is its first
/// column's height, and on a slope the other three stand lower.
fn root(writer: &mut ChunkWriter, x: i32, base: i32, z: i32, log: BlockState) {
    for (dx, dz) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
        for y in (base - 6..base).rev() {
            match writer.get(x + dx, y, z + dz) {
                Some(here) if is_replaceable(here) => writer.log(x + dx, y, z + dz, log),
                _ => break,
            }
        }
    }
}

fn mega_jungle(writer: &mut ChunkWriter, rng: &mut Rng, x: i32, base: i32, z: i32) {
    let height = 10 + rng.below(15) as i32;
    let top = base + height;
    let log = of(blocks::JUNGLE_LOG);
    let leaves = of(blocks::JUNGLE_LEAVES);
    root(writer, x, base, z, log);
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
        // From the trunk's last straight log, so the branch touches it.
        let (mut bx, mut by, mut bz) = (x, base + straight - 1, z);
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
    root(writer, x, base, z, log);
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
    /// Vanilla-style flower patches, for the biomes that have them.
    patch: Option<Patch>,
}

/// A vanilla `random_patch` of flowers (`flower_plains`, `flower_default`):
/// `count` tries per chunk, or `crowded_count` where the noise at the chunk
/// is below `crowded_below`, each kept with chance 1 in `rarity`, then
/// `tries` placements around a random surface spot.
struct Patch {
    count: u32,
    crowded: Option<(f64, u32)>,
    rarity: u32,
    tries: u32,
}

/// Plains: 4 tries per chunk, 15 in the rare chunks the noise marks.
const PLAINS_PATCH: Patch = Patch {
    count: 4,
    crowded: Some((-0.8, 15)),
    rarity: 32,
    tries: 64,
};
/// Forests: one try per chunk.
const DEFAULT_PATCH: Patch = Patch {
    count: 1,
    crowded: None,
    rarity: 32,
    tries: 64,
};
/// Patches spread this far sideways and this far up and down.
const PATCH_SPREAD: u32 = 6;
const PATCH_SPREAD_Y: u32 = 2;
/// The noise that picks the crowded plains chunks, in blocks per cycle.
const PATCH_NOISE_SCALE: f64 = 200.0;
const PATCH_SALT: u64 = 0xF10;

const NO_PLANTS: Plants = Plants {
    grass: 0,
    grass_chance: 0.0,
    ferns: 0.0,
    flower_patches: 0,
    flowers: &[],
    patch: None,
};

fn plants_for(biome: Biome) -> Plants {
    use blocks::*;
    let p = |grass, grass_chance, ferns, flower_patches, flowers| Plants {
        grass,
        grass_chance,
        ferns,
        flower_patches,
        flowers,
        patch: None,
    };
    let with = |plants: Plants, patch| Plants {
        patch: Some(patch),
        flower_patches: 0,
        ..plants
    };
    match biome {
        Biome::Plains => with(
            p(
                32,
                0.9,
                0.0,
                0,
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
            PLAINS_PATCH,
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
        Biome::Forest => with(
            p(
                12,
                0.4,
                0.0,
                0,
                &[
                    DANDELION,
                    POPPY,
                    LILY_OF_THE_VALLEY,
                    LILAC,
                    ROSE_BUSH,
                    PEONY,
                ],
            ),
            DEFAULT_PATCH,
        ),
        Biome::BirchForest => with(
            p(
                12,
                0.4,
                0.0,
                0,
                &[DANDELION, POPPY, LILY_OF_THE_VALLEY, LILAC, PEONY],
            ),
            DEFAULT_PATCH,
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

    // Vanilla flower patches: few chunks have any, and those have many.
    if let Some(patch) = &kinds.patch {
        let mut rng = Rng::for_chunk(terrain.seed(), pos.x, pos.z, PATCH_SALT);
        let tries = match patch.crowded {
            Some((below, crowded)) if patch_noise(terrain.seed(), min_x, min_z) < below => crowded,
            _ => patch.count,
        };
        for _ in 0..tries {
            if rng.below(patch.rarity) != 0 {
                continue;
            }
            let (cx, cz) = pick(&mut rng);
            let surface = area.get(cx, cz).height + 1;
            let flower = of(kinds.flowers[rng.below(kinds.flowers.len() as u32) as usize]);
            let spread = |rng: &mut Rng, n: u32| rng.below(n + 1) as i32 - rng.below(n + 1) as i32;
            for _ in 0..patch.tries {
                let (x, z) = (
                    cx + spread(&mut rng, PATCH_SPREAD),
                    cz + spread(&mut rng, PATCH_SPREAD),
                );
                let y = surface + spread(&mut rng, PATCH_SPREAD_Y);
                if !(min_x..min_x + 16).contains(&x) || !(min_z..min_z + 16).contains(&z) {
                    continue;
                }
                let column = area.get(x, z);
                if column.grassy() && column.height >= SEA_LEVEL && y == column.height + 1 {
                    plant(&mut writer, x, y, z, flower);
                }
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

/// The smooth noise (about -1 to 1) that marks plains chunks for the crowded
/// flower count.
fn patch_noise(seed: u64, x: i32, z: i32) -> f64 {
    use noise::{NoiseFn, Perlin};
    Perlin::new((seed ^ PATCH_SALT) as u32)
        .get([x as f64 / PATCH_NOISE_SCALE, z as f64 / PATCH_NOISE_SCALE])
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
            let start = (column.height.max(SEA_LEVEL) + 40).min(crate::config::MAX_Y - 2);
            let Some(y) = (1..=start).rev().find(|&y| !chunk.get(x, y, z).is_air()) else {
                continue;
            };
            let block = chunk.get(x, y, z);
            if block == WATER {
                if column.biome.is_cold_at(y) {
                    chunk.set(x, y, z, ICE);
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
                chunk.set(x, y + 1, z, SNOW);
                if block.def().props.contains(&Prop::Snowy) {
                    chunk.set(x, y, z, block.with_bool(Prop::Snowy, true));
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

    /// A chunk whose 3 × 3 chunks round it are all `biome` (every quart)
    /// with no structure on the surface there, standing above the sea.
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
                let centers = (-1..=1).all(|dx| {
                    (-1..=1).all(|dz| terrain.biome_at(x + 8 + dx * 16, z + 8 + dz * 16) == biome)
                });
                // Dry land, clear of the shore.
                centers
                    && terrain.height(x + 8, z + 8) > SEA_LEVEL + 3
                    && (-16..32).step_by(4).all(|dx| {
                        (-16..32)
                            .step_by(4)
                            .all(|dz| terrain.biome_at(x + dx, z + dz) == biome)
                    })
                    && (-1..=1).all(|dx| {
                        (-1..=1).all(|dz| {
                            crate::world::structure::near(terrain, pos.offset(dx, dz))
                                .iter()
                                .all(|s| {
                                    use crate::world::structure::Kind;
                                    matches!(
                                        s.kind,
                                        Kind::Dungeon | Kind::Mineshaft | Kind::Stronghold
                                    )
                                })
                        })
                    })
            })
            .unwrap_or_else(|| panic!("no {biome:?} found"))
    }

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
                for y in SEA_LEVEL - 4..crate::config::MAX_Y {
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
        for biome in [Biome::Desert, Biome::Badlands, Biome::IceSpikes] {
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
            for y in SEA_LEVEL..crate::config::MAX_Y - 1 {
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
                for y in SEA_LEVEL..crate::config::MAX_Y - 1 {
                    // The top log of a small oak on the east edge of the chunk.
                    let log = chunk.get(15, y, z);
                    // (A trunk under it and leaves to the west: a small oak's
                    // top, not a fancy oak's branch.)
                    if log.id() != blocks::OAK_LOG
                        || chunk.get(15, y + 1, z).id() != blocks::OAK_LEAVES
                        || !is_leaves(chunk.get(14, y, z))
                        || chunk.get(15, y - 1, z).id() != blocks::OAK_LOG
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

    /// The topmost block of each column that isn't air, a plant, snow
    /// layer or leaves, with the column's biome.
    fn grounds(chunk: &Chunk) -> Vec<(Biome, BlockId)> {
        let mut out = Vec::new();
        for z in 0..16 {
            for x in 0..16 {
                let ground = (crate::config::MIN_Y + 1..crate::config::MAX_Y)
                    .rev()
                    .map(|y| chunk.get(x, y, z))
                    .find(|b| {
                        !b.is_air()
                            && b.def().fluid.is_none()
                            && !matches!(
                                b.def().shape,
                                Shape::Cross | Shape::Crop | Shape::SnowLayer | Shape::Vine
                            )
                            && !is_leaves(*b)
                            && !b.def().name.ends_with("_log")
                    });
                if let Some(ground) = ground {
                    out.push((chunk.biome(x, z), ground.id()));
                }
            }
        }
        out
    }

    /// The share of `biome`'s columns in the 3 × 3 chunks round `center`
    /// whose ground (or, with `snow`, whose top) is one of `ids`.
    fn ground_share(terrain: &Terrain, biome: Biome, ids: &[BlockId], snow: bool) -> f64 {
        let center = find_biome(terrain, biome);
        let (chunks, _) = census(terrain, center, 1);
        let (mut hits, mut all) = (0, 0);
        for chunk in chunks.values() {
            if snow {
                for z in 0..16 {
                    for x in 0..16 {
                        if chunk.biome(x, z) != biome {
                            continue;
                        }
                        all += 1;
                        let top = (crate::config::MIN_Y + 1..crate::config::MAX_Y)
                            .rev()
                            .map(|y| chunk.get(x, y, z))
                            .find(|b| !b.is_air());
                        hits += top.is_some_and(|b| ids.contains(&b.id())) as usize;
                    }
                }
                continue;
            }
            for (b, id) in grounds(chunk) {
                if b == biome {
                    all += 1;
                    hits += ids.contains(&id) as usize;
                }
            }
        }
        hits as f64 / all.max(1) as f64
    }

    #[test]
    fn each_biome_shows_its_own_ground() {
        let terrain = Terrain::new(20111118);
        let terracotta: Vec<BlockId> = blocks::NAMES
            .iter()
            .filter(|n| n.ends_with("terracotta"))
            .filter_map(|n| blocks::by_name(n))
            .chain([blocks::RED_SAND])
            .collect();
        for (biome, ids, snow, want) in [
            (Biome::Desert, vec![blocks::SAND], false, 0.8),
            (Biome::Badlands, terracotta, false, 0.8),
            (Biome::MushroomFields, vec![blocks::MYCELIUM], false, 0.6),
            (
                Biome::SnowyPlains,
                vec![blocks::SNOW, blocks::ICE],
                true,
                0.8,
            ),
            (Biome::Plains, vec![blocks::GRASS_BLOCK], false, 0.6),
        ] {
            let share = ground_share(&terrain, biome, &ids, snow);
            println!("{biome:?}: {:.0}%", share * 100.0);
            assert!(share >= want, "{biome:?}: {share:.2}");
        }
    }

    #[test]
    fn beaches_are_sand_over_sandstone() {
        let terrain = Terrain::new(20111118);
        let (x, z) = crate::world::worldgen::terrain::tests::find(&terrain, "beach", |c| {
            c.biome == Biome::Beach && c.height >= SEA_LEVEL
        });
        let chunk = terrain.generate(ChunkPos::of_block(x, z));
        let (mut sand, mut all) = (0, 0);
        for (b, id) in grounds(&chunk) {
            if b == Biome::Beach {
                all += 1;
                sand += (id == blocks::SAND) as usize;
            }
        }
        let mut kinds: HashMap<&str, usize> = HashMap::new();
        for (b, id) in grounds(&chunk) {
            if b == Biome::Beach {
                *kinds.entry(id.name()).or_default() += 1;
            }
        }
        assert!(sand * 10 >= all * 7, "{sand} of {all}: {kinds:?}");
        let column = terrain.column(x, z);
        let (lx, lz) = (x.rem_euclid(16) as usize, z.rem_euclid(16) as usize);
        let h = column.height as usize;
        assert_eq!(chunk.get(lx, h as i32, lz).id(), blocks::SAND);
        let sandstone = (h - 8..h).any(|y| chunk.get(lx, y as i32, lz).id() == blocks::SANDSTONE);
        assert!(sandstone, "sandstone under the beach");
    }

    #[test]
    fn nothing_grows_on_air_or_water_and_decoration_repeats() {
        let terrain = Terrain::new(20111118);
        for biome in [
            Biome::Forest,
            Biome::Plains,
            Biome::Taiga,
            Biome::Jungle,
            Biome::Desert,
            Biome::Savanna,
            Biome::DarkForest,
            Biome::BirchForest,
            Biome::SnowyTaiga,
            Biome::CherryGrove,
            Biome::Meadow,
        ] {
            let center = find_biome(&terrain, biome);
            let (chunks, _) = census(&terrain, center, 1);
            for chunk in chunks.values() {
                for z in 0..16 {
                    for x in 0..16 {
                        for y in crate::config::MIN_Y + 2..crate::config::MAX_Y {
                            let block = chunk.get(x, y, z);
                            let below = chunk.get(x, y - 1, z);
                            let plant = matches!(
                                block.def().shape,
                                Shape::Cross | Shape::Cactus | Shape::Bamboo
                            );
                            // An upright log with no log under it: a trunk's
                            // foot (branches lie on their side).
                            let trunk = block.def().name.ends_with("_log")
                                && block.has(Prop::Axis)
                                && block.get(Prop::Axis) == axis::Y
                                && !below.def().name.ends_with("_log");
                            if !(plant || trunk) || below.id() == block.id() {
                                continue;
                            }
                            // Acacia trunks lean: the log below may be diagonal.
                            let leans = trunk
                                && (-1..=1).any(|dz: i32| {
                                    (-1..=1).any(|dx: i32| {
                                        let (nx, nz) = (x as i32 + dx, z as i32 + dz);
                                        let pos =
                                            chunk.pos.offset(nx.div_euclid(16), nz.div_euclid(16));
                                        chunks.get(&pos).is_none_or(|c| {
                                            let (lx, lz) = (
                                                nx.rem_euclid(16) as usize,
                                                nz.rem_euclid(16) as usize,
                                            );
                                            c.get(lx, y - 1, lz).id() == block.id()
                                        })
                                    })
                                });
                            if leans {
                                continue;
                            }
                            // A log on leaves is a branch; mushrooms grow anywhere dark.
                            // Cobwebs and dripstone hang in caves; surface
                            // plants grow at sea level or higher.
                            if is_leaves(below)
                                || block.def().name.contains("mushroom")
                                || block.is(blocks::COBWEB)
                                || block.is(blocks::POINTED_DRIPSTONE)
                                || (plant && y < (SEA_LEVEL as usize) as i32)
                            {
                                continue;
                            }
                            assert!(
                                below.def().solid && below.def().fluid.is_none(),
                                "{biome:?}: {} on {} at {x} {y} {z} in {:?}; under: {:?}",
                                block.def().name,
                                below.def().name,
                                chunk.pos,
                                (y.saturating_sub(8)..y)
                                    .rev()
                                    .map(|yy| chunk.get(x, yy, z).def().name.clone())
                                    .collect::<Vec<_>>()
                            );
                        }
                    }
                }
            }
            // The same seed decorates the same way.
            let again = terrain.generate(center);
            let first = &chunks[&center];
            for y in crate::config::MIN_Y..crate::config::MAX_Y {
                for i in 0..256 {
                    assert_eq!(again.get(i % 16, y, i / 16), first.get(i % 16, y, i / 16));
                }
            }
        }
    }

    /// Up to `n` chunks whose center column is `biome`, two chunks apart
    /// along a square spiral from the origin.
    fn chunks_of(terrain: &Terrain, biome: Biome, n: usize) -> Vec<ChunkPos> {
        let mut found = Vec::new();
        let (mut x, mut z, mut dx, mut dz) = (0i32, 0i32, 0i32, -1i32);
        for _ in 0..1_000_000 {
            let pos = ChunkPos::new(x * 2, z * 2);
            let (bx, bz) = pos.min_block();
            if terrain.biome_at(bx + 8, bz + 8) == biome {
                found.push(pos);
                if found.len() == n {
                    break;
                }
            }
            if x == z || (x < 0 && x == -z) || (x > 0 && x == 1 - z) {
                (dx, dz) = (-dz, dx);
            }
            (x, z) = (x + dx, z + dz);
        }
        found
    }

    #[test]
    fn trees_per_chunk_stay_within_each_biomes_attempts() {
        let terrain = Terrain::new(20111118);
        let mut report = Vec::new();
        // Ice spikes and icebergs place the same way but aren't trees.
        for row in TREES
            .iter()
            .filter(|row| !matches!(row.default, Tree::IceSpike | Tree::Iceberg))
        {
            let chunks = chunks_of(&terrain, row.biome, 500);
            assert_eq!(chunks.len(), 500, "{:?}: too few chunks", row.biome);
            let trees: usize = chunks
                .iter()
                .map(|&pos| {
                    plan(&terrain, None, pos)
                        .iter()
                        .filter(|t| t.kind != Tree::Azalea && t.column.biome == row.biome)
                        .count()
                })
                .sum();
            let average = trees as f64 / chunks.len() as f64;
            let most = row.count as f64 + row.extra;
            report.push(format!(
                "{:?}: {average:.3} per chunk (at most {most})",
                row.biome
            ));
            assert!(average <= most, "{:?}: {average} > {most}", row.biome);
            if row.count > 0 {
                assert!(average > 0.0, "{:?} grows nothing", row.biome);
            }
        }
        println!("{}", report.join("\n"));
    }

    #[test]
    fn a_chunks_trees_are_the_same_from_every_chunk_that_replays_them() {
        let terrain = Terrain::new(20111118);
        let key = |trees: Vec<Planted>| {
            trees
                .iter()
                .map(|t| (t.kind as u8, t.x, t.z, t.base))
                .collect::<Vec<_>>()
        };
        for biome in [Biome::Forest, Biome::Jungle, Biome::DarkForest] {
            let source = find_biome(&terrain, biome);
            let alone = key(plan(&terrain, None, source));
            assert!(!alone.is_empty(), "{biome:?}: no trees to compare");
            for dz in -1..=1 {
                for dx in -1..=1 {
                    let area = terrain.area(source.offset(dx, dz));
                    assert_eq!(
                        key(plan(&terrain, Some(&area), source)),
                        alone,
                        "{biome:?} replayed from {dx} {dz}"
                    );
                }
            }
        }
    }

    #[test]
    fn decoration_does_not_depend_on_which_neighbors_load_first() {
        let center = find_biome(&Terrain::new(20111118), Biome::Forest);
        let alone = Terrain::new(20111118).generate(center);
        let offsets: Vec<(i32, i32)> = (-1..=1)
            .flat_map(|dz| (-1..=1).map(move |dx| (dx, dz)))
            .filter(|&o| o != (0, 0))
            .collect();
        for order in [offsets.clone(), offsets.iter().rev().copied().collect()] {
            let terrain = Terrain::new(20111118);
            for (dx, dz) in order {
                terrain.generate(center.offset(dx, dz));
            }
            let after = terrain.generate(center);
            for y in crate::config::MIN_Y..crate::config::MAX_Y {
                for i in 0..256 {
                    assert_eq!(after.get(i % 16, y, i / 16), alone.get(i % 16, y, i / 16));
                }
            }
        }
    }

    #[test]
    fn no_trunk_stands_on_water_sand_stone_or_air() {
        let terrain = Terrain::new(20111118);
        let mut trunks = 0;
        for row in TREES {
            if matches!(row.default, Tree::IceSpike | Tree::Iceberg)
                || row.biome == Biome::MushroomFields
            {
                continue;
            }
            for pos in chunks_of(&terrain, row.biome, 6) {
                let chunk = terrain.generate(pos);
                for y in crate::config::MIN_Y + 1..crate::config::MAX_Y {
                    for i in 0..256 {
                        let (x, z) = (i % 16, i / 16);
                        let block = chunk.get(x, y, z);
                        let below = chunk.get(x, y - 1, z);
                        let upright =
                            block.def().name.ends_with("_log") && block.get(Prop::Axis) == axis::Y;
                        if !upright || below.def().name.ends_with("_log") || is_leaves(below) {
                            continue;
                        }
                        trunks += 1;
                        let bad = below.is_air()
                            || below.def().fluid.is_some()
                            || matches!(
                                below.id(),
                                blocks::SAND | blocks::RED_SAND | blocks::STONE | blocks::GRAVEL
                            );
                        // Acacia trunks lean, so the log below may be beside it.
                        let acacia = block.id() == blocks::ACACIA_LOG && below.is_air();
                        assert!(
                            !bad || acacia,
                            "{:?}: {} on {} at {x} {y} {z} in {pos:?}",
                            row.biome,
                            block.def().name,
                            below.def().name
                        );
                    }
                }
            }
        }
        assert!(trunks > 100, "only {trunks} trunks checked");
    }
}
