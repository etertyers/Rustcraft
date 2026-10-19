//! Reading v1.0 saves, which stored Minecraft 1.0.0's numeric block and
//! item IDs with 4-bit data values. `assets/data/legacy.ron` names the
//! modern block or item each became; this module turns the data bits that
//! held orientation, growth or fluid level into block state properties,
//! and decodes v1.0 chunks and item stacks. A v1.0 world is upgraded once,
//! when it's first opened (see `level::WorldDir::migrate_v1`).

use std::{collections::HashMap, sync::LazyLock};

use anyhow::{Result, bail};
use serde::Deserialize;

use super::{
    Face,
    biome::Biome,
    block::{BlockId, BlockState},
    block_entity::BlockEntity,
    blocks,
    chunk::{Chunk, ChunkPos, LocalPos, SECTION_SIZE, SECTION_VOLUME},
    props::{Prop, attach_face, bed_part, door_half, half, slab_type},
    save::SaveError,
};
use crate::{
    config::CHUNK_SIZE,
    inventory::{furnace::Furnace, item::ItemId, items, stack::ItemStack},
};

#[derive(Deserialize)]
struct Entry {
    id: u16,
    #[serde(default)]
    data: Option<u16>,
    name: String,
}

#[derive(Deserialize)]
struct TableFile {
    blocks: Vec<Entry>,
    items: Vec<Entry>,
}

/// The v1.0 ID table, by (ID, data value or `None` for the rest).
struct Table {
    blocks: HashMap<(u16, Option<u16>), BlockId>,
    items: HashMap<(u16, Option<u16>), ItemId>,
}

fn table() -> &'static Table {
    static TABLE: LazyLock<Table> = LazyLock::new(|| {
        let file: TableFile = ron::from_str(include_str!("../../assets/data/legacy.ron"))
            .expect("assets/data/legacy.ron is valid");
        let blocks = file
            .blocks
            .iter()
            .map(|e| {
                let id = blocks::by_name(&e.name)
                    .unwrap_or_else(|| panic!("legacy.ron names unknown block {}", e.name));
                ((e.id, e.data), id)
            })
            .collect();
        let items = file
            .items
            .iter()
            .map(|e| {
                let id = items::by_name(&e.name)
                    .unwrap_or_else(|| panic!("legacy.ron names unknown item {}", e.name));
                ((e.id, e.data), id)
            })
            .collect();
        Table { blocks, items }
    });
    &TABLE
}

/// The bits of a 1.0.0 block's data value that pick its variant.
fn variant_bits(id: u16, data: u8) -> u8 {
    match id {
        6 | 17 | 18 => data & 3,
        43 | 44 | 125 | 126 => data & 7,
        _ => data,
    }
}

/// 1.0.0's four horizontal directions in the order beds, pumpkins, fence
/// gates and end portal frames number them.
const SOUTH_WEST_NORTH_EAST: [Face; 4] = [Face::South, Face::West, Face::North, Face::East];

/// The facing a 1.0.0 data value 2–5 names (furnaces, chests, ladders, wall
/// signs: north, south, west, east). Other values faced south, the way
/// v1.0 drew them.
fn facing_2_to_5(data: u8) -> Face {
    match data {
        2 => Face::North,
        3 => Face::South,
        4 => Face::West,
        5 => Face::East,
        _ => Face::South,
    }
}

/// The modern state of 1.0.0 block `id` with data value `data`.
pub fn block(id: u16, data: u8) -> BlockState {
    let data = data & 0xF;
    let table = table();
    let name = match (id, data) {
        // A torch on a wall became a separate wall torch.
        (50 | 75 | 76, 1..=4) => match id {
            50 => blocks::WALL_TORCH,
            _ => blocks::REDSTONE_WALL_TORCH,
        },
        // Water in a cauldron became a water cauldron.
        (118, 1..=3) => blocks::WATER_CAULDRON,
        // A mushroom block's stem became its own block.
        (99 | 100, 10 | 15) => blocks::MUSHROOM_STEM,
        _ => {
            let variant = variant_bits(id, data) as u16;
            // Data values 1.0.0 didn't use fall back to the first variant.
            match table
                .blocks
                .get(&(id, Some(variant)))
                .or_else(|| table.blocks.get(&(id, None)))
                .or_else(|| table.blocks.get(&(id, Some(0))))
            {
                Some(&name) => name,
                None => return BlockState::AIR,
            }
        }
    };
    let state = BlockState::of(name);
    let horizontal = |faces: [Face; 4]| state.with_face(Prop::Facing, faces[(data & 3) as usize]);
    match id {
        6 => state.with_number(Prop::Stage, (data >> 3) & 1),
        8..=11 => state.with_number(Prop::Level, data),
        23 | 54 | 61 | 62 | 95 => {
            let lit = id == 62;
            let face = facing_2_to_5(data);
            state
                .with_face(Prop::FacingAll, face)
                .with_face(Prop::Facing, face)
                .with_bool(Prop::Lit, lit)
        }
        26 => horizontal(SOUTH_WEST_NORTH_EAST)
            .with(
                Prop::BedPart,
                if data & 8 != 0 {
                    bed_part::HEAD
                } else {
                    bed_part::FOOT
                },
            )
            .with_bool(Prop::Occupied, data & 4 != 0),
        27 | 28 => state
            .with(Prop::StraightRailShape, (data & 7).min(5))
            .with_bool(Prop::Powered, data & 8 != 0),
        66 => state.with(Prop::RailShape, data.min(9)),
        29 | 33 | 34 => {
            let face = [
                Face::Bottom,
                Face::Top,
                Face::North,
                Face::South,
                Face::West,
                Face::East,
            ][((data & 7) as usize).min(5)];
            let state = state.with_face(Prop::FacingAll, face);
            if id == 34 {
                state.with(Prop::PistonType, (data >> 3) & 1)
            } else {
                state.with_bool(Prop::Extended, data & 8 != 0)
            }
        }
        43 | 125 => state.with(Prop::SlabType, slab_type::DOUBLE),
        44 | 126 if data & 8 != 0 => state.with(Prop::SlabType, slab_type::TOP),
        50 | 75 | 76 => {
            let lit = id != 75;
            let state = state.with_bool(Prop::Lit, lit);
            match data {
                1 => state.with_face(Prop::Facing, Face::East),
                2 => state.with_face(Prop::Facing, Face::West),
                3 => state.with_face(Prop::Facing, Face::South),
                4 => state.with_face(Prop::Facing, Face::North),
                _ => state,
            }
        }
        51 => state.with_number(Prop::Age15, data),
        53 | 67 | 108 | 109 | 114 => horizontal([Face::East, Face::West, Face::South, Face::North])
            .with(
                Prop::Half,
                if data & 4 != 0 {
                    half::TOP
                } else {
                    half::BOTTOM
                },
            ),
        55 => state.with_number(Prop::Power, data),
        59 | 104 | 105 => state.with_number(Prop::Age7, data & 7),
        60 => state.with_number(Prop::Moisture, data & 7),
        63 => state.with_number(Prop::Rotation, data),
        64 | 71 => {
            // With its hinge on the left, 1.0.0's direction 0–3 faces east,
            // south, west, north; a door hinged the other way was the next
            // direction round with "open" flipped, which looks and opens
            // exactly like this.
            horizontal([Face::East, Face::South, Face::West, Face::North])
                .with_bool(Prop::Open, data & 4 != 0)
                .with(
                    Prop::DoubleBlockHalf,
                    if data & 8 != 0 {
                        door_half::UPPER
                    } else {
                        door_half::LOWER
                    },
                )
        }
        65 | 68 => state.with_face(
            Prop::Facing,
            if (2..=5).contains(&data) {
                facing_2_to_5(data)
            } else {
                Face::North
            },
        ),
        69 | 77 => {
            let wall = |face| {
                state
                    .with(Prop::AttachFace, attach_face::WALL)
                    .with_face(Prop::Facing, face)
            };
            let state = match data & 7 {
                1 => wall(Face::East),
                2 => wall(Face::West),
                3 => wall(Face::South),
                4 => wall(Face::North),
                6 => state
                    .with(Prop::AttachFace, attach_face::FLOOR)
                    .with_face(Prop::Facing, Face::East),
                7 => state.with(Prop::AttachFace, attach_face::CEILING),
                // 5 stood on the floor; 0 is what v1.0 placed, with no
                // orientation at all.
                _ => state.with(Prop::AttachFace, attach_face::FLOOR),
            };
            state.with_bool(Prop::Powered, data & 8 != 0)
        }
        70 | 72 => state.with_bool(Prop::Powered, data & 1 != 0),
        73 | 74 => state.with_bool(Prop::Lit, id == 74),
        78 => state.with_number(Prop::Layers, (data & 7) + 1),
        81 | 83 => state.with_number(Prop::Age15, data),
        84 => state.with_bool(Prop::HasRecord, data != 0),
        86 | 91 | 107 | 120 => {
            let state = horizontal(SOUTH_WEST_NORTH_EAST);
            match id {
                107 => state.with_bool(Prop::Open, data & 4 != 0),
                120 => state.with_bool(Prop::Eye, data & 4 != 0),
                _ => state,
            }
        }
        92 => state.with_number(Prop::Bites, data.min(6)),
        93 | 94 => horizontal([Face::North, Face::East, Face::South, Face::West])
            .with_number(Prop::Delay, (data >> 2) + 1)
            .with_bool(Prop::Powered, id == 94),
        96 => horizontal([Face::North, Face::South, Face::West, Face::East])
            .with_bool(Prop::Open, data & 4 != 0),
        99 | 100 => mushroom_sides(state, data),
        106 => {
            let sides = [
                (Face::South, 1),
                (Face::West, 2),
                (Face::North, 4),
                (Face::East, 8),
            ];
            let mut state = state.with_bool(Prop::Up, data == 0);
            for (face, bit) in sides {
                state = state.with_bool(Prop::side(face), data & bit != 0);
            }
            state
        }
        115 => state.with_number(Prop::Age3, data & 3),
        117 => state
            .with_bool(Prop::HasBottle0, data & 1 != 0)
            .with_bool(Prop::HasBottle1, data & 2 != 0)
            .with_bool(Prop::HasBottle2, data & 4 != 0),
        118 if data > 0 => state.with_number(Prop::LevelCauldron, data),
        _ => state,
    }
}

/// Which faces of a 1.0.0 mushroom block (data 0–15) show the cap's skin:
/// 1–9 are the cap's corners, edges and middle, 10 a stem with pores on
/// top and bottom, 14 cap all round and 15 stem all round.
fn mushroom_sides(state: BlockState, data: u8) -> BlockState {
    let (up, down, north, east, south, west) = match data {
        1 => (true, false, true, false, false, true),
        2 => (true, false, true, false, false, false),
        3 => (true, false, true, true, false, false),
        4 => (true, false, false, false, false, true),
        5 => (true, false, false, false, false, false),
        6 => (true, false, false, true, false, false),
        7 => (true, false, false, false, true, true),
        8 => (true, false, false, false, true, false),
        9 => (true, false, false, true, true, false),
        10 => (false, false, true, true, true, true),
        14 | 15 => (true, true, true, true, true, true),
        _ => (false, false, false, false, false, false),
    };
    [
        (Face::Top, up),
        (Face::Bottom, down),
        (Face::North, north),
        (Face::East, east),
        (Face::South, south),
        (Face::West, west),
    ]
    .into_iter()
    .fold(state, |state, (face, shows)| {
        state.with_bool(Prop::side(face), shows)
    })
}

/// Blocks v1.0 let a creative player hold that have no item today, and the
/// item that places them now.
fn item_for_block(id: u16) -> Option<ItemId> {
    Some(match id {
        8 | 9 => items::WATER_BUCKET,
        10 | 11 => items::LAVA_BUCKET,
        26 => items::RED_BED,
        55 => items::REDSTONE,
        59 => items::WHEAT_SEEDS,
        63 | 68 => items::OAK_SIGN,
        75 | 76 => items::REDSTONE_TORCH,
        104 => items::PUMPKIN_SEEDS,
        105 => items::MELON_SEEDS,
        _ => return None,
    })
}

/// The modern stack for a v1.0 one: item `id` with `damage` (the data
/// value for variants, or wear for tools and armor). `None` for things
/// with no item any more (fire, portals).
pub fn stack(id: u16, count: u8, damage: u16) -> Option<ItemStack> {
    let registry = crate::inventory::item::registry();
    let item = if id < 256 {
        let block = block(id, damage as u8);
        registry
            .of_block(block.id())
            .or_else(|| item_for_block(id))?
    } else {
        let table = table();
        *table
            .items
            .get(&(id, Some(damage)))
            .or_else(|| table.items.get(&(id, None)))?
    };
    let wears = registry.get(item)?.max_damage().is_some();
    Some(ItemStack::with_damage(
        item,
        count,
        if wears { damage } else { 0 },
    ))
}

/// A v1.0 item stack as saved.
#[derive(Deserialize)]
struct StackV1 {
    id: u16,
    count: u8,
    damage: u16,
}

impl StackV1 {
    fn upgrade(slot: Option<StackV1>) -> Option<ItemStack> {
        let old = slot?;
        let stack = stack(old.id, old.count, old.damage);
        if stack.is_none() {
            log::warn!(
                "dropping v1.0 item {}:{} with no item today",
                old.id,
                old.damage
            );
        }
        stack
    }
}

#[derive(Deserialize)]
struct FurnaceV1 {
    slots: [Option<StackV1>; 3],
    burn: u32,
    burn_total: u32,
    cook: u32,
}

/// A v1.0 block entity as saved (variants in the same order as today's).
#[derive(Deserialize)]
enum BlockEntityV1 {
    Chest(Vec<Option<StackV1>>),
    Furnace(FurnaceV1),
}

impl BlockEntityV1 {
    fn upgrade(self) -> BlockEntity {
        match self {
            BlockEntityV1::Chest(slots) => {
                BlockEntity::Chest(slots.into_iter().map(StackV1::upgrade).collect())
            }
            BlockEntityV1::Furnace(furnace) => BlockEntity::Furnace(Furnace {
                slots: furnace.slots.map(StackV1::upgrade),
                burn: furnace.burn,
                burn_total: furnace.burn_total,
                cook: furnace.cook,
                ..Furnace::default()
            }),
        }
    }
}

#[derive(Deserialize)]
struct SectionV1 {
    index: u8,
    /// 12-bit ID and 4-bit data value per block, in `y, z, x` order.
    blocks: Vec<u16>,
}

/// A v1.0 chunk as saved since M10.
#[derive(Deserialize)]
struct ChunkV1 {
    x: i32,
    z: i32,
    sections: Vec<SectionV1>,
    biomes: Vec<u8>,
    block_entities: Vec<(LocalPos, BlockEntityV1)>,
}

/// A chunk saved before block entities existed (M6–M9).
#[derive(Deserialize)]
struct ChunkV1Old {
    x: i32,
    z: i32,
    sections: Vec<SectionV1>,
    biomes: Vec<u8>,
}

fn bincode_config() -> impl bincode::config::Config {
    bincode::config::standard().with_fixed_int_encoding()
}

/// Decompresses and decodes a v1.0 chunk, upgrading its blocks and the
/// items in its chests and furnaces. Blocks whose state hangs on their
/// neighbors (which v1.0 didn't store) are listed in `post_process`.
pub fn decode_chunk(bytes: &[u8]) -> Result<Chunk, SaveError> {
    let corrupt = |what: String| SaveError::Corrupt(what);
    let bytes = lz4_flex::decompress_size_prepended(bytes)
        .map_err(|err| corrupt(format!("v1 chunk decompression: {err}")))?;
    let decoded: Result<(ChunkV1, usize), _> =
        bincode::serde::decode_from_slice(&bytes, bincode_config());
    let data = match decoded {
        Ok((data, _)) => data,
        Err(err) => {
            let (old, _): (ChunkV1Old, usize) =
                bincode::serde::decode_from_slice(&bytes, bincode_config())
                    .map_err(|_| corrupt(format!("v1 chunk decoding: {err}")))?;
            ChunkV1 {
                x: old.x,
                z: old.z,
                sections: old.sections,
                biomes: old.biomes,
                block_entities: Vec::new(),
            }
        }
    };
    let mut chunk = Chunk::new(ChunkPos::new(data.x, data.z));
    // v1.0 had 8 sections; every one fits in today's 16.
    for section in data.sections {
        let index = section.index as usize;
        if index >= 8 || section.blocks.len() != SECTION_VOLUME {
            return Err(corrupt(format!("bad v1 section {index}")));
        }
        let blocks: Vec<BlockState> = section
            .blocks
            .iter()
            .map(|&bits| block(bits & 0x0FFF, (bits >> 12) as u8))
            .collect();
        for (i, state) in blocks.iter().enumerate() {
            if state.def().shaped_by_neighbors() {
                let (x, z, y) = (i % SECTION_SIZE, (i / SECTION_SIZE) % SECTION_SIZE, i / 256);
                chunk
                    .post_process
                    .push([x as u8, (index * SECTION_SIZE + y) as u8, z as u8]);
            }
        }
        chunk.set_section(index, &blocks);
    }
    if data.biomes.len() != CHUNK_SIZE * CHUNK_SIZE {
        return Err(corrupt(format!("{} biomes", data.biomes.len())));
    }
    for (i, &id) in data.biomes.iter().enumerate() {
        let biome = Biome::from_id(id).ok_or_else(|| corrupt(format!("biome id {id}")))?;
        chunk.set_biome(i % CHUNK_SIZE, i / CHUNK_SIZE, biome);
    }
    for (at, entity) in data.block_entities {
        chunk.set_block_entity(at, Some(entity.upgrade()));
    }
    Ok(chunk)
}

/// Upgrades a v1.0 `level.ron` to today's format in memory: item stacks
/// (`(id: 270, count: 1, damage: 12)`, the only structs with an `id`) go
/// from numeric IDs to names, and stacks with no item today become `None`.
/// Everything else is kept as written.
pub fn upgrade_level(text: &str) -> Result<String> {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some((before, (id, count, damage), after)) = next_stack(rest) {
        match stack(id, count, damage) {
            Some(new) => {
                out.push_str(before);
                out.push_str(&ron::to_string(&new)?);
            }
            // The item is gone: `Some(stack)` becomes `None`.
            None => {
                log::warn!("dropping v1.0 item {id}:{damage} with no item today");
                let kept = before.trim_end();
                let Some(kept) = kept.strip_suffix("Some(") else {
                    bail!("a v1.0 stack outside an Option");
                };
                out.push_str(kept);
                out.push_str("None");
                let Some(closed) = after.trim_start().strip_prefix(')') else {
                    bail!("a v1.0 stack outside an Option");
                };
                rest = closed;
                continue;
            }
        }
        rest = after;
    }
    out.push_str(rest);
    Ok(out)
}

/// Finds the next `(id: N, count: N, damage: N)` in `text`: the text
/// before it, its numbers, and the text after it.
fn next_stack(text: &str) -> Option<(&str, (u16, u8, u16), &str)> {
    let mut from = 0;
    while let Some(open) = text[from..].find('(').map(|i| i + from) {
        from = open + 1;
        let mut cursor = &text[open + 1..];
        let mut numbers = [0u32; 3];
        let mut ok = true;
        for (i, key) in ["id", "count", "damage"].iter().enumerate() {
            cursor = cursor.trim_start();
            let Some(after_key) = cursor
                .strip_prefix(key)
                .map(str::trim_start)
                .and_then(|c| c.strip_prefix(':'))
            else {
                ok = false;
                break;
            };
            let after_key = after_key.trim_start();
            let digits = after_key
                .find(|c: char| !c.is_ascii_digit())
                .unwrap_or(after_key.len());
            let Ok(number) = after_key[..digits].parse() else {
                ok = false;
                break;
            };
            numbers[i] = number;
            cursor = after_key[digits..].trim_start();
            cursor = cursor.strip_prefix(',').unwrap_or(cursor);
        }
        if !ok {
            continue;
        }
        let Some(after) = cursor.trim_start().strip_prefix(')') else {
            continue;
        };
        let end = text.len() - after.len();
        let [id, count, damage] = numbers;
        return Some((
            &text[..open],
            (id as u16, count as u8, damage as u16),
            &text[end..],
        ));
    }
    None
}

/// Writing v1.0 files, for testing the upgrade.
#[cfg(test)]
pub mod v1 {
    use serde::Serialize;

    use super::*;

    #[derive(Serialize)]
    pub struct Stack {
        pub id: u16,
        pub count: u8,
        pub damage: u16,
    }

    #[derive(Serialize)]
    pub enum Entity {
        Chest(Vec<Option<Stack>>),
    }

    #[derive(Serialize)]
    struct Section {
        index: u8,
        blocks: Vec<u16>,
    }

    #[derive(Serialize)]
    struct ChunkData {
        x: i32,
        z: i32,
        sections: Vec<Section>,
        biomes: Vec<u8>,
        block_entities: Vec<(LocalPos, Entity)>,
    }

    /// A v1.0 chunk's bytes: `blocks` as (x, y, z, 1.0.0 ID, data), the
    /// rest air, every column plains.
    pub fn chunk(
        pos: ChunkPos,
        blocks: &[(usize, usize, usize, u16, u8)],
        entities: Vec<(LocalPos, Entity)>,
    ) -> Vec<u8> {
        let mut sections: Vec<Section> = Vec::new();
        for &(x, y, z, id, data) in blocks {
            let index = (y / SECTION_SIZE) as u8;
            let section = match sections.iter_mut().position(|s| s.index == index) {
                Some(i) => &mut sections[i],
                None => {
                    sections.push(Section {
                        index,
                        blocks: vec![0; SECTION_VOLUME],
                    });
                    sections.last_mut().unwrap()
                }
            };
            let i = ((y % SECTION_SIZE) * SECTION_SIZE + z) * SECTION_SIZE + x;
            section.blocks[i] = id | (data as u16) << 12;
        }
        let data = ChunkData {
            x: pos.x,
            z: pos.z,
            sections,
            biomes: vec![Biome::Plains.id(); CHUNK_SIZE * CHUNK_SIZE],
            block_entities: entities,
        };
        let bytes = bincode::serde::encode_to_vec(&data, bincode_config()).unwrap();
        lz4_flex::compress_prepend_size(&bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::props::{axis, hinge, wall_side};

    #[test]
    fn every_1_0_0_block_and_variant_has_a_modern_name() {
        for id in (0..=122).chain([125, 126]) {
            for data in 0..16 {
                let state = block(id, data);
                assert!(id == 0 || !state.is_air(), "{id}:{data} turned into air");
            }
        }
        // IDs 1.0.0 didn't have (123, 124) are air.
        assert!(block(123, 0).is_air());
    }

    #[test]
    fn variants_become_their_own_blocks() {
        assert_eq!(block(17, 2), BlockState::of(blocks::BIRCH_LOG));
        assert_eq!(block(17, 2).get(Prop::Axis), axis::Y);
        assert_eq!(block(35, 14), BlockState::of(blocks::RED_WOOL));
        assert_eq!(block(98, 1), BlockState::of(blocks::MOSSY_STONE_BRICKS));
        assert_eq!(block(31, 2), BlockState::of(blocks::FERN));
        assert_eq!(block(31, 0), BlockState::of(blocks::DEAD_BUSH));
        assert_eq!(block(2, 0), BlockState::of(blocks::GRASS_BLOCK));
        assert_eq!(
            block(44, 4),
            BlockState::of(blocks::BRICK_SLAB),
            "a bottom slab"
        );
        assert_eq!(
            block(43, 1),
            BlockState::of(blocks::SANDSTONE_SLAB).with(Prop::SlabType, slab_type::DOUBLE)
        );
        assert_eq!(
            block(125, 0),
            BlockState::of(blocks::OAK_SLAB).with(Prop::SlabType, slab_type::DOUBLE)
        );
    }

    #[test]
    fn orientation_and_growth_become_properties() {
        // A torch attached to the wall on its west faces east.
        assert_eq!(
            block(50, 1),
            BlockState::of(blocks::WALL_TORCH).with_face(Prop::Facing, Face::East)
        );
        assert_eq!(block(50, 5), BlockState::of(blocks::TORCH));
        assert!(!block(75, 5).bool(Prop::Lit), "an unlit redstone torch");
        assert!(block(76, 3).is(blocks::REDSTONE_WALL_TORCH));
        // Furnaces: data 2–5 face north, south, west, east; lit is a property.
        assert_eq!(block(61, 5).face(Prop::Facing), Face::East);
        assert!(block(62, 2).bool(Prop::Lit));
        assert!(block(62, 2).is(blocks::FURNACE));
        // Water levels carry over; still and flowing water are one block.
        assert_eq!(block(9, 0), BlockState::of(blocks::WATER));
        assert_eq!(block(8, 11).number(Prop::Level), 11);
        // Wheat stages, snow layers, a bed's head.
        assert_eq!(block(59, 7).number(Prop::Age7), 7);
        assert_eq!(block(78, 0).number(Prop::Layers), 1);
        let head = block(26, 2 | 8);
        assert_eq!(head.face(Prop::Facing), Face::North);
        assert_eq!(head.get(Prop::BedPart), bed_part::HEAD);
        // Stairs placed by v1.0 (data 0) ascend east.
        assert_eq!(block(53, 0).face(Prop::Facing), Face::East);
        // A cauldron with water in it.
        assert_eq!(block(118, 2).number(Prop::LevelCauldron), 2);
        // A mushroom cap's north-west corner and a stem.
        let corner = block(99, 1);
        assert!(corner.bool(Prop::North) && corner.bool(Prop::West) && corner.bool(Prop::Up));
        assert!(!corner.bool(Prop::South));
        assert!(block(100, 10).is(blocks::MUSHROOM_STEM));
        assert!(!block(100, 10).bool(Prop::Up));
        // Vines on the north side.
        let vine = block(106, 4);
        assert!(vine.bool(Prop::North) && !vine.bool(Prop::East));
        let _ = wall_side::NONE;
    }

    #[test]
    fn doors_keep_their_shape_and_swing() {
        use crate::world::block::door_box;
        // v1.0 door data (direction, open) against how it drew: closed
        // along side (d + 3) & 3, open along side d (0 north, 1 east, 2
        // south, 3 west).
        let sides = [Face::North, Face::East, Face::South, Face::West];
        let side_box = |face: Face| {
            let t = 3.0 / 16.0;
            match face {
                Face::North => ([0.0, 0.0, 0.0], [1.0, 1.0, t]),
                Face::East => ([1.0 - t, 0.0, 0.0], [1.0, 1.0, 1.0]),
                Face::South => ([0.0, 0.0, 1.0 - t], [1.0, 1.0, 1.0]),
                _ => ([0.0, 0.0, 0.0], [t, 1.0, 1.0]),
            }
        };
        for data in 0..8u8 {
            let d = (data & 3) as usize;
            let expected = if data & 4 == 0 {
                sides[(d + 3) % 4]
            } else {
                sides[d]
            };
            let door = block(64, data);
            assert_eq!(door_box(door), side_box(expected), "data {data}");
            assert_eq!(door.get(Prop::Hinge), hinge::LEFT);
            // Opening it moves it where 1.0.0 would.
            let toggled = door.with_bool(Prop::Open, !door.bool(Prop::Open));
            let expected = if data & 4 == 0 {
                sides[d]
            } else {
                sides[(d + 3) % 4]
            };
            assert_eq!(door_box(toggled), side_box(expected), "data {data} toggled");
        }
        assert_eq!(
            block(71, 3 | 8).get(Prop::DoubleBlockHalf),
            door_half::UPPER
        );
    }

    #[test]
    fn stacks_get_modern_items_and_keep_tool_wear() {
        let s = |id, count, damage| stack(id, count, damage).map(|s| (s.id, s.count, s.damage));
        assert_eq!(s(278, 1, 40), Some((items::DIAMOND_PICKAXE, 1, 40)));
        assert_eq!(s(35, 12, 14), Some((items::RED_WOOL, 12, 0)));
        assert_eq!(s(263, 5, 1), Some((items::CHARCOAL, 5, 0)));
        assert_eq!(s(351, 3, 4), Some((items::LAPIS_LAZULI, 3, 0)));
        assert_eq!(s(351, 3, 15), Some((items::BONE_MEAL, 3, 0)));
        assert_eq!(s(17, 64, 1), Some((items::SPRUCE_LOG, 64, 0)));
        assert_eq!(s(338, 9, 0), Some((items::SUGAR_CANE, 9, 0)));
        assert_eq!(s(355, 1, 0), Some((items::RED_BED, 1, 0)));
        assert_eq!(s(2256, 1, 0), Some((items::MUSIC_DISC_13, 1, 0)));
        // Blocks that were only in v1.0's creative inventory.
        assert_eq!(s(55, 3, 0), Some((items::REDSTONE, 3, 0)));
        assert_eq!(s(62, 1, 0), Some((items::FURNACE, 1, 0)));
        assert_eq!(s(51, 1, 0), None, "fire has no item");
    }

    #[test]
    fn a_v1_level_gets_named_items() {
        let text = r#"(version: 1, name: "Old", seed: 5, game_mode: Survival, spawn: (0.5, 70.0, 0.5),
            player: (position: (1.0, 70.0, 2.0), yaw: 0.0, pitch: 0.0,
                inventory: (slots: [Some((id: 270, count: 1, damage: 12)), None, Some((id: 35, count: 40, damage: 14))], selected: 2)),
            time: 10, last_played: 0)"#;
        let level: crate::level::Level = ron::from_str(&upgrade_level(text).unwrap()).unwrap();
        let inventory = &level.player.inventory;
        assert_eq!(
            inventory.slot(0),
            Some(&ItemStack::with_damage(items::WOODEN_PICKAXE, 1, 12))
        );
        assert_eq!(
            inventory.slot(2),
            Some(&ItemStack::new(items::RED_WOOL, 40))
        );
        assert_eq!(inventory.selected, 2);
        // Written out by ron's pretty printer, with fire (no item today).
        let pretty = "(\n    version: 1,\n    player: (\n        inventory: (\n            slots: [\n                Some((\n                    id: 51,\n                    count: 1,\n                    damage: 0,\n                )),\n                Some((\n                    id: 17,\n                    count: 3,\n                    damage: 2,\n                )),\n            ],\n        ),\n    ),\n)";
        let upgraded = upgrade_level(pretty).unwrap();
        assert!(upgraded.contains("None,"), "{upgraded}");
        assert!(upgraded.contains("minecraft:birch_log"), "{upgraded}");
    }
}
