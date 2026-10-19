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
