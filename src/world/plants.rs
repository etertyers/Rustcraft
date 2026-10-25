//! Where plants with rules beyond "something under it" can stay.

use glam::IVec3;

use super::{block::BlockState, blocks, props::Prop};

/// What sugar cane grows on (vanilla's `minecraft:dirt` tag, plus sand).
const CANE_GROUND: [super::block::BlockId; 11] = [
    blocks::GRASS_BLOCK,
    blocks::DIRT,
    blocks::COARSE_DIRT,
    blocks::ROOTED_DIRT,
    blocks::PODZOL,
    blocks::MYCELIUM,
    blocks::MOSS_BLOCK,
    blocks::MUD,
    blocks::SAND,
    blocks::RED_SAND,
    blocks::SUSPICIOUS_SAND,
];

/// Whether sugar cane can stand at `at` (modern `SugarCaneBlock.canSurvive`):
/// on more sugar cane, or on dirt, grass, sand and their kin with water
/// (a water block, a waterlogged one or frosted ice) beside that block on
/// one of its four sides.
pub fn cane_can_stay(get: &impl Fn(IVec3) -> Option<BlockState>, at: IVec3) -> bool {
    let below = at - IVec3::Y;
    let Some(ground) = get(below) else {
        return false;
    };
    if ground.is(blocks::SUGAR_CANE) {
        return true;
    }
    if !CANE_GROUND.iter().any(|&id| ground.is(id)) {
        return false;
    }
    [IVec3::X, IVec3::NEG_X, IVec3::Z, IVec3::NEG_Z]
        .into_iter()
        .any(|side| {
            get(below + side).is_some_and(|b| {
                b.is(blocks::WATER)
                    || b.is(blocks::FROSTED_ICE)
                    || (b.has(Prop::Waterlogged) && b.bool(Prop::Waterlogged))
            })
        })
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;

    fn world(cells: &[(IVec3, BlockState)]) -> impl Fn(IVec3) -> Option<BlockState> + use<> {
        let map: HashMap<IVec3, BlockState> = cells.iter().copied().collect();
        move |p| Some(map.get(&p).copied().unwrap_or(BlockState::AIR))
    }

    #[test]
    fn sugar_cane_needs_its_ground_and_water_beside_it() {
        let at = IVec3::new(0, 64, 0);
        let ground = at - IVec3::Y;
        let water = (ground + IVec3::X, BlockState::of(blocks::WATER));
        for (id, ok) in [
            (blocks::GRASS_BLOCK, true),
            (blocks::DIRT, true),
            (blocks::SAND, true),
            (blocks::RED_SAND, true),
            (blocks::MUD, true),
            (blocks::PODZOL, true),
            (blocks::GRAVEL, false),
            (blocks::STONE, false),
        ] {
            let with = world(&[(ground, BlockState::of(id)), water]);
            assert_eq!(cane_can_stay(&with, at), ok, "{id:?}");
        }
        // Dry ground, water only diagonally or one block lower, won't do.
        let dry = world(&[(ground, BlockState::of(blocks::SAND))]);
        assert!(!cane_can_stay(&dry, at));
        let diagonal = world(&[
            (ground, BlockState::of(blocks::SAND)),
            (ground + IVec3::new(1, 0, 1), BlockState::of(blocks::WATER)),
        ]);
        assert!(!cane_can_stay(&diagonal, at));
        let lower = world(&[
            (ground, BlockState::of(blocks::SAND)),
            (ground + IVec3::new(1, -1, 0), BlockState::of(blocks::WATER)),
        ]);
        assert!(!cane_can_stay(&lower, at));
        // Frosted ice counts, and cane stands on cane.
        let iced = world(&[
            (ground, BlockState::of(blocks::DIRT)),
            (ground + IVec3::NEG_Z, BlockState::of(blocks::FROSTED_ICE)),
        ]);
        assert!(cane_can_stay(&iced, at));
        let stacked = world(&[(ground, BlockState::of(blocks::SUGAR_CANE))]);
        assert!(cane_can_stay(&stacked, at));
    }
}
