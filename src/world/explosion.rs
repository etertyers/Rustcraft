//! Which blocks an explosion destroys, following modern Minecraft's
//! `Explosion.explode`: 16×16×16 rays leave the center of the blast on the
//! faces of a cube, each starting at the blast power with a random 70–130%,
//! losing strength to every block's blast resistance as it goes, and the
//! blocks they get through are destroyed.

use glam::{DVec3, IVec3};

use super::{World, block::BlockState};

/// Rays along each edge of the cube they leave through.
const RAYS: i32 = 16;
/// How far a ray moves between looks, and the strength it loses on each.
const STEP: f64 = 0.3;
const FADE: f64 = 0.225;

/// How much blast `state` soaks up (modern `blastResistance`), roughly:
/// stone-like blocks 6, obsidian 1200, fluids 100, bedrock out of reach,
/// anything else its hardness.
pub fn blast_resistance(state: BlockState) -> f32 {
    let def = state.def();
    if def.fluid.is_some() {
        return 100.0;
    }
    let name = state.id().name();
    match name {
        "obsidian" | "crying_obsidian" | "enchanting_table" | "anvil" | "respawn_anchor" => {
            return 1200.0;
        }
        "ender_chest" => return 600.0,
        "end_stone" => return 9.0,
        _ => {}
    }
    if def.hardness < 0.0 {
        return 3_600_000.0;
    }
    let stony = [
        "stone",
        "cobble",
        "brick",
        "andesite",
        "diorite",
        "granite",
        "deepslate",
    ]
    .iter()
    .any(|part| name.contains(part))
        && !name.ends_with("_ore");
    if stony {
        6.0f32.max(def.hardness)
    } else if name.ends_with("_block") && def.hardness >= 3.0 {
        // Metal and gem blocks.
        6.0
    } else {
        def.hardness
    }
}

/// The blocks an explosion of `power` at `center` destroys. `random`
/// returns numbers in 0..1.
pub fn destroyed(
    world: &World,
    center: DVec3,
    power: f32,
    random: &mut impl FnMut() -> f32,
) -> Vec<IVec3> {
    let mut hit = super::fast_hash::FastSet::default();
    let mut order = Vec::new();
    let last = RAYS - 1;
    for i in 0..RAYS {
        for j in 0..RAYS {
            for k in 0..RAYS {
                if !(i == 0 || i == last || j == 0 || j == last || k == 0 || k == last) {
                    continue;
                }
                let to = |n: i32| f64::from(n) / f64::from(last) * 2.0 - 1.0;
                let dir = DVec3::new(to(i), to(j), to(k)).normalize() * STEP;
                let mut at = center;
                let mut strength = f64::from(power) * (0.7 + f64::from(random()) * 0.6);
                while strength > 0.0 {
                    let cell = at.floor().as_ivec3();
                    let Some(state) = world.block(cell.x, cell.y, cell.z) else {
                        break;
                    };
                    if !state.is_air() {
                        strength -= (f64::from(blast_resistance(state)) + 0.3) * STEP;
                        if strength > 0.0 && hit.insert(cell) {
                            order.push(cell);
                        }
                    }
                    at += dir;
                    strength -= FADE;
                }
            }
        }
    }
    order
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::{
        blocks,
        chunk::{Chunk, ChunkPos},
    };

    /// A 16×16 chunk of `fill` up to y=40 with a pocket of air in its
    /// middle for the blast.
    fn solid(fill: BlockState) -> World {
        let mut world = World::default();
        let mut chunk = Chunk::new(ChunkPos::new(0, 0));
        for x in 0..16 {
            for z in 0..16 {
                for y in 0..40 {
                    chunk.set(x, y, z, fill);
                }
            }
        }
        for x in 7..=9 {
            for z in 7..=9 {
                for y in 19..=21 {
                    chunk.set(x, y, z, BlockState::AIR);
                }
            }
        }
        world.insert(chunk);
        world
    }

    fn blast(world: &World, power: f32) -> Vec<IVec3> {
        let mut seed = 12345u32;
        let mut random = move || {
            seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
            (seed >> 8) as f32 / (1u32 << 24) as f32
        };
        destroyed(world, DVec3::new(8.0, 20.5, 8.0), power, &mut random)
    }

    #[test]
    fn tnt_digs_a_ball_a_few_blocks_across_in_stone() {
        let world = solid(BlockState::of(blocks::STONE));
        let cells = blast(&world, 4.0);
        let farthest = cells
            .iter()
            .map(|c| (c.as_dvec3() + DVec3::splat(0.5)).distance(DVec3::new(8.0, 20.5, 8.0)))
            .fold(0.0f64, f64::max);
        assert!((20..300).contains(&cells.len()), "{} blocks", cells.len());
        assert!(farthest < 5.5, "reaches {farthest}");
    }

    #[test]
    fn dirt_goes_farther_than_stone_and_obsidian_resists_it_all() {
        let dirt = blast(&solid(BlockState::of(blocks::DIRT)), 4.0).len();
        let stone = blast(&solid(BlockState::of(blocks::STONE)), 4.0).len();
        assert!(dirt > stone, "dirt {dirt} vs stone {stone}");
        let world = solid(BlockState::of(blocks::OBSIDIAN));
        assert!(blast(&world, 4.0).is_empty(), "obsidian holds");
    }

    #[test]
    fn a_bigger_blast_digs_deeper_and_water_soaks_it_up() {
        let stone = solid(BlockState::of(blocks::STONE));
        assert!(blast(&stone, 6.0).len() > blast(&stone, 4.0).len());
        let water = blast(&solid(BlockState::of(blocks::WATER)), 4.0).len();
        assert!(water < blast(&stone, 4.0).len() / 2, "water: {water}");
    }

    #[test]
    fn blast_resistances_are_in_modern_ranges() {
        assert_eq!(blast_resistance(BlockState::of(blocks::STONE)), 6.0);
        assert_eq!(blast_resistance(BlockState::of(blocks::OBSIDIAN)), 1200.0);
        assert_eq!(blast_resistance(BlockState::of(blocks::WATER)), 100.0);
        assert!(blast_resistance(BlockState::of(blocks::BEDROCK)) > 1_000_000.0);
        assert!(blast_resistance(BlockState::of(blocks::DIRT)) < 1.0);
    }
}
