//! Single-template structures on the surface or the sea floor: desert
//! and jungle temples, shipwrecks and ruined portals.

use glam::IVec3;

use super::{Kind, Piece, jigsaw, template::registry};
use crate::{
    config::SEA_LEVEL,
    world::{
        biome::Biome,
        chunk::ChunkPos,
        worldgen::{rng::Rng, terrain::Terrain},
    },
};

fn ocean(biome: Biome) -> bool {
    matches!(
        biome,
        Biome::Ocean
            | Biome::DeepOcean
            | Biome::ColdOcean
            | Biome::DeepColdOcean
            | Biome::LukewarmOcean
            | Biome::DeepLukewarmOcean
            | Biome::WarmOcean
            | Biome::FrozenOcean
            | Biome::DeepFrozenOcean
            | Biome::Beach
    )
}

pub fn generate(
    kind: Kind,
    terrain: &Terrain,
    start: ChunkPos,
    rng: &mut Rng,
) -> Option<Vec<Piece>> {
    let (x, z) = start.min_block();
    let (x, z) = (x + 8, z + 8);
    let column = terrain.column(x, z);
    let biome = column.biome;
    let (pool, sink) = match kind {
        Kind::DesertTemple if biome == Biome::Desert => ("desert_temple", 14),
        Kind::JungleTemple if matches!(biome, Biome::Jungle | Biome::BambooJungle) => {
            ("jungle_temple", 0)
        }
        Kind::Shipwreck if ocean(biome) && column.height < SEA_LEVEL - 3 => {
            ("shipwreck", 1 + rng.below(3) as i32)
        }
        Kind::Shipwreck if biome == Biome::Beach => ("shipwreck_beached", 2),
        Kind::RuinedPortal => ("ruined_portal", 1 + rng.below(2) as i32),
        _ => return None,
    };
    let registry = registry();
    let names = registry.pool(pool);
    if names.is_empty() {
        return None;
    }
    let name = &names[rng.below(names.len() as u32) as usize];
    let template = registry.get(name)?;
    let turns = rng.below(4) as u8;
    let [w, _, d] = template.turned_size(turns);
    let origin_x = x - w / 2;
    let origin_z = z - d / 2;
    // Stand on the lowest corner, so nothing hangs in the air.
    let ground = [
        (origin_x, origin_z),
        (origin_x + w - 1, origin_z),
        (origin_x, origin_z + d - 1),
        (origin_x + w - 1, origin_z + d - 1),
    ]
    .into_iter()
    .map(|(x, z)| terrain.height(x, z))
    .min()?;
    let land = matches!(kind, Kind::DesertTemple | Kind::JungleTemple);
    if land && !jigsaw::dry(terrain, x, z) {
        return None;
    }
    let origin = IVec3::new(origin_x, ground - sink, origin_z);
    Some(vec![template.place(origin, turns)])
}
