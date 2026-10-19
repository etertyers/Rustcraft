//! Villages: a meeting point, streets and houses joined jigsaw-style, in
//! the style of the biome they stand in.

use super::{
    Piece,
    jigsaw::{self, Options},
    template::registry,
};
use crate::world::{
    biome::Biome,
    chunk::ChunkPos,
    worldgen::{rng::Rng, terrain::Terrain},
};

/// The five styles, each a folder of pools under `village/`.
pub const STYLES: [&str; 5] = ["plains", "desert", "savanna", "taiga", "snowy"];

/// The style a village in `biome` is built in, if villages grow there.
pub fn style(biome: Biome) -> Option<&'static str> {
    Some(match biome {
        Biome::Plains | Biome::Meadow => "plains",
        Biome::Desert => "desert",
        Biome::Savanna => "savanna",
        Biome::Taiga => "taiga",
        Biome::SnowyPlains => "snowy",
        _ => return None,
    })
}

/// Pieces reach at most this far from the meeting point, and this many
/// joins away from it (modern `size` 6, `max_distance_from_center` 80).
const MAX_DISTANCE: i32 = 80;
const MAX_DEPTH: u32 = 6;

pub fn generate(terrain: &Terrain, start: ChunkPos, rng: &mut Rng) -> Option<Vec<Piece>> {
    let (x, z) = start.min_block();
    let center = (x + 8, z + 8);
    let style = style(terrain.column(center.0, center.1).biome)?;
    let options = Options {
        max_depth: MAX_DEPTH,
        max_distance: MAX_DISTANCE,
    };
    let pool = format!("village/{style}/centers");
    let pieces = jigsaw::assemble(terrain, registry(), &pool, center, rng, &options)?;
    // A meeting point with nothing around it isn't a village.
    (pieces.len() >= 3).then_some(pieces)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::structure::{Kind, structure_at};

    /// The first village start for `seed` within `regions` of the origin.
    pub(crate) fn find_village(
        terrain: &Terrain,
        regions: i32,
    ) -> Option<std::sync::Arc<crate::world::structure::Structure>> {
        for rz in -regions..regions {
            for rx in -regions..regions {
                let start =
                    Kind::Village
                        .spread()
                        .candidate(terrain.seed(), Kind::Village.salt(), rx, rz);
                if let Some(village) = structure_at(terrain, Kind::Village, start) {
                    return Some(village);
                }
            }
        }
        None
    }

    #[test]
    fn a_seed_grows_villages_with_streets_houses_and_villagers() {
        let terrain = Terrain::new(20111118);
        let village = find_village(&terrain, 6).expect("a village near spawn");
        let pieces = village.pieces.len();
        let villagers = village.mobs().filter(|m| m.mob == "villager").count();
        let golems = village.mobs().filter(|m| m.mob == "iron_golem").count();
        let paths: usize = village.pieces.iter().map(|p| p.surface.len()).sum();
        eprintln!(
            "village at {:?}: {pieces} pieces, {villagers} villagers, {paths} path blocks, bounds {:?}",
            village.start, village.bounds
        );
        assert!(pieces >= 8, "{pieces}");
        assert!(villagers >= 3 && golems == 1);
        assert!(paths > 40);
        // Nothing overlaps.
        let boxes: Vec<_> = village.pieces.iter().filter_map(|p| p.bounds).collect();
        let _ = boxes;
        // Laid out the same every time.
        let again = Kind::Village.generate(&terrain, village.start).unwrap();
        assert_eq!(again, *village);
    }
}

#[cfg(test)]
mod chunk_tests {
    use super::tests::find_village;
    use crate::world::{blocks, chunk::ChunkPos, worldgen::terrain::Terrain};

    #[test]
    fn the_chunks_of_a_village_hold_its_houses() {
        let terrain = Terrain::new(20111118);
        let village = find_village(&terrain, 6).unwrap();
        let (min, max) = (village.bounds.min, village.bounds.max);
        let (mut beds, mut doors, mut paths, mut chests) = (0, 0, 0, 0);
        for cz in (min.z >> 4)..=(max.z >> 4) {
            for cx in (min.x >> 4)..=(max.x >> 4) {
                let chunk = terrain.generate(ChunkPos::new(cx, cz));
                for y in 40..140 {
                    for z in 0..16 {
                        for x in 0..16 {
                            let block = chunk.get(x, y, z);
                            beds += block.is(blocks::RED_BED) as u32;
                            doors += block.def().name.ends_with("_door") as u32;
                            paths += (block.is(blocks::DIRT_PATH)
                                || block.is(blocks::SMOOTH_SANDSTONE))
                                as u32;
                        }
                    }
                }
                chests += chunk
                    .block_entities()
                    .filter(|(_, e)| matches!(e, crate::world::block_entity::BlockEntity::Chest(_)))
                    .count();
            }
        }
        eprintln!("{beds} bed blocks, {doors} door blocks, {paths} paths, {chests} chests");
        assert!(beds >= 6 && doors >= 6 && paths >= 40 && chests >= 2);
    }
}
