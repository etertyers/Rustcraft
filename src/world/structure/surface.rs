//! Single-template structures on the surface or the sea floor: desert
//! and jungle temples, shipwrecks and ruined portals.

use glam::IVec3;

use super::{Kind, Piece, jigsaw, template::registry};
use crate::{
    config::SEA_LEVEL,
    world::{
        biome::Biome,
        block::BlockState,
        blocks,
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

/// Where a ruined portal goes (modern `RuinedPortalPiece.VerticalPlacement`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Placement {
    OnLandSurface,
    PartlyBuried,
    OnOceanFloor,
    InMountain,
    Underground,
}

/// Modern `RuinedPortalStructure`'s setups by biome: deserts half bury
/// it, jungles grow over it, oceans and swamps sink it, mountains hold it,
/// and elsewhere it's on the surface or down in a cave, half and half.
pub fn portal_placement(biome: Biome, rng: &mut Rng) -> Placement {
    match biome {
        Biome::Desert => Placement::PartlyBuried,
        Biome::Jungle | Biome::BambooJungle => Placement::OnLandSurface,
        Biome::Swamp | Biome::MangroveSwamp => Placement::OnOceanFloor,
        b if ocean(b) && b != Biome::Beach => Placement::OnOceanFloor,
        Biome::WindsweptHills
        | Biome::Badlands
        | Biome::StonyShore
        | Biome::Meadow
        | Biome::Grove
        | Biome::SnowySlopes
        | Biome::JaggedPeaks
        | Biome::FrozenPeaks
        | Biome::CherryGrove => Placement::InMountain,
        _ if rng.below(2) == 0 => Placement::Underground,
        _ => Placement::OnLandSurface,
    }
}

/// Cave floors this deep below the surface at least, and no lower than
/// this, hold an underground portal.
const UNDERGROUND_COVER: i32 = 15;
const UNDERGROUND_MIN_Y: i32 = 15;

/// A ruined portal by column (`x`, `z`): placed for its biome, its inside
/// (the template's box above the floor) cleared to air, or to water below
/// sea level, so the frame and the chest can be reached; mossy in jungles,
/// swamps and oceans.
fn ruined_portal(terrain: &Terrain, x: i32, z: i32, rng: &mut Rng) -> Option<Vec<Piece>> {
    let registry = registry();
    let names = registry.pool("ruined_portal");
    if names.is_empty() {
        return None;
    }
    let name = &names[rng.below(names.len() as u32) as usize];
    let template = registry.get(name)?;
    let turns = rng.below(4) as u8;
    let [w, h, d] = template.turned_size(turns);
    let (origin_x, origin_z) = (x - w / 2, z - d / 2);
    let biome = terrain.column(x, z).biome;
    let mut placement = portal_placement(biome, rng);
    // The ground under three of the four corners (modern `findSuitableY`).
    let mut corners: Vec<i32> = [
        (origin_x, origin_z),
        (origin_x + w - 1, origin_z),
        (origin_x, origin_z + d - 1),
        (origin_x + w - 1, origin_z + d - 1),
    ]
    .into_iter()
    .map(|(x, z)| terrain.height(x, z))
    .collect();
    corners.sort_unstable();
    let ground = corners[1];
    let lowest = corners[0];
    let (cx, cz) = (origin_x + w / 2, origin_z + d / 2);
    let floor = loop {
        break match placement {
            Placement::OnLandSurface | Placement::OnOceanFloor => ground,
            Placement::PartlyBuried => ground - h + 2 + rng.below(4) as i32,
            Placement::InMountain => lowest - 2 - rng.below(5) as i32,
            Placement::Underground => {
                // The floor of a cave, never solid rock all round.
                let top = lowest - UNDERGROUND_COVER;
                let start =
                    UNDERGROUND_MIN_Y + rng.below((top - UNDERGROUND_MIN_Y).max(1) as u32) as i32;
                let open =
                    |y: i32| terrain.is_cave(cx, y + 1, cz) && terrain.is_cave(cx, y + 2, cz);
                let found = (UNDERGROUND_MIN_Y..top.max(UNDERGROUND_MIN_Y + 1))
                    .map(|y| start + (y - UNDERGROUND_MIN_Y))
                    .map(|y| {
                        if y >= top {
                            y - (top - UNDERGROUND_MIN_Y)
                        } else {
                            y
                        }
                    })
                    .find(|&y| open(y) && !terrain.is_cave(cx, y, cz));
                match found {
                    Some(y) => y,
                    None => {
                        placement = Placement::OnLandSurface;
                        continue;
                    }
                }
            }
        };
    };
    let origin = IVec3::new(origin_x, floor, origin_z);
    let mut piece = template.place(origin, turns);
    // Clear the box above the floor where the template has nothing.
    let taken: std::collections::HashSet<IVec3> = piece.blocks.iter().map(|&(p, _)| p).collect();
    let mut blocks = Vec::new();
    for dy in 1..h {
        for dx in 0..w {
            for dz in 0..d {
                let at = origin + IVec3::new(dx, dy, dz);
                if taken.contains(&at) {
                    continue;
                }
                // Under the sea (or a river) the inside is water, not air.
                let surface =
                    placement != Placement::Underground && placement != Placement::InMountain;
                let wet = at.y <= SEA_LEVEL && terrain.height(at.x, at.z) < SEA_LEVEL;
                let fill = if surface && wet {
                    blocks::WATER
                } else {
                    blocks::AIR
                };
                blocks.push((at, BlockState::of(fill)));
            }
        }
    }
    let mossy = match biome {
        Biome::Jungle | Biome::BambooJungle => 0.8,
        _ if placement == Placement::OnOceanFloor => 0.5,
        _ => 0.2,
    };
    for (_, block) in &mut piece.blocks {
        if (block.is(blocks::STONE_BRICKS) || block.is(blocks::CRACKED_STONE_BRICKS))
            && rng.next_f64() < mossy
        {
            *block = BlockState::of(blocks::MOSSY_STONE_BRICKS);
        }
    }
    blocks.append(&mut piece.blocks);
    piece.blocks = blocks;
    piece.compute_bounds();
    Some(vec![piece])
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
        Kind::RuinedPortal => return ruined_portal(terrain, x, z, rng),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ruined_portals_are_placed_by_biome() {
        let mut rng = Rng::for_chunk(1, 2, 3, 4);
        let mut rolls = |biome| portal_placement(biome, &mut rng);
        assert_eq!(rolls(Biome::Desert), Placement::PartlyBuried);
        assert_eq!(rolls(Biome::Jungle), Placement::OnLandSurface);
        assert_eq!(rolls(Biome::Swamp), Placement::OnOceanFloor);
        assert_eq!(rolls(Biome::DeepColdOcean), Placement::OnOceanFloor);
        assert_eq!(rolls(Biome::JaggedPeaks), Placement::InMountain);
        let plains: Vec<_> = (0..200).map(|_| rolls(Biome::Plains)).collect();
        let under = plains
            .iter()
            .filter(|p| **p == Placement::Underground)
            .count();
        assert!(
            (70..130).contains(&under),
            "about half underground: {under}"
        );
        assert!(
            plains
                .iter()
                .all(|p| matches!(p, Placement::Underground | Placement::OnLandSurface))
        );
    }
}
