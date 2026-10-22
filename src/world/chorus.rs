//! Chorus plants, after modern `ChorusFlowerBlock.generatePlant` and
//! `ChorusPlantBlock`: how one grows (a stem up, then branches, each ending
//! in a flower), how a stem joins its neighbors, and what holds it up.

use std::collections::HashMap;

use glam::IVec3;

use super::{Face, block::BlockState, blocks, props::Prop, worldgen::rng::Rng};

/// A branch stays within this many blocks of the base, sideways.
pub const MAX_HORIZONTAL: i32 = 8;
/// Branches stop after this many levels.
const MAX_DEPTH: u32 = 4;
/// A flower at the end of a branch is fully grown.
const TIP_AGE: u8 = 5;
const SIDES: [Face; 4] = [Face::North, Face::East, Face::South, Face::West];

fn step(face: Face) -> IVec3 {
    IVec3::from_array(face.normal())
}

fn opposite(face: Face) -> Face {
    match face {
        Face::North => Face::South,
        Face::South => Face::North,
        Face::East => Face::West,
        Face::West => Face::East,
        Face::Top => Face::Bottom,
        Face::Bottom => Face::Top,
    }
}

/// A plant or a flower: what a stem joins.
fn is_chorus(state: BlockState) -> bool {
    state.is(blocks::CHORUS_PLANT) || state.is(blocks::CHORUS_FLOWER)
}

/// A fully grown flower.
pub fn tip() -> BlockState {
    BlockState::of(blocks::CHORUS_FLOWER).with_number(Prop::Age5, TIP_AGE)
}

/// `plant` joined to the chorus blocks and end stone around it (modern
/// `ChorusPlantBlock.getStateForPlacement`), read through `get`.
pub fn connect(
    plant: BlockState,
    at: IVec3,
    get: &impl Fn(IVec3) -> Option<BlockState>,
) -> BlockState {
    let near = |face: Face| get(at + step(face)).unwrap_or(BlockState::AIR);
    Face::ALL.iter().fold(plant, |state, &face| {
        let other = near(face);
        let joins = is_chorus(other) || (face == Face::Bottom && other.is(blocks::END_STONE));
        state.with_bool(Prop::side(face), joins)
    })
}

/// Whether the plant at `at` stays up (modern `ChorusPlantBlock.canSurvive`):
/// it stands on a plant or end stone, or a plant beside it does, though a
/// plant with something above and below can't also have one beside it.
pub fn plant_survives(at: IVec3, get: &impl Fn(IVec3) -> Option<BlockState>) -> bool {
    let at_ = |p: IVec3| get(p).unwrap_or(BlockState::AIR);
    let below = at_(at - IVec3::Y);
    let boxed_in = !at_(at + IVec3::Y).is_air() && !below.is_air();
    for face in SIDES {
        let side = at + step(face);
        if at_(side).is(blocks::CHORUS_PLANT) {
            if boxed_in {
                return false;
            }
            let under = at_(side - IVec3::Y);
            if under.is(blocks::CHORUS_PLANT) || under.is(blocks::END_STONE) {
                return true;
            }
        }
    }
    below.is(blocks::CHORUS_PLANT) || below.is(blocks::END_STONE)
}

/// Whether the flower at `at` stays up (modern `ChorusFlowerBlock.canSurvive`):
/// on a plant or end stone, or over air with exactly one plant beside it.
pub fn flower_survives(at: IVec3, get: &impl Fn(IVec3) -> Option<BlockState>) -> bool {
    let at_ = |p: IVec3| get(p).unwrap_or(BlockState::AIR);
    let below = at_(at - IVec3::Y);
    if below.is(blocks::CHORUS_PLANT) || below.is(blocks::END_STONE) {
        return true;
    }
    if !below.is_air() {
        return false;
    }
    SIDES
        .iter()
        .filter(|&&face| at_(at + step(face)).is(blocks::CHORUS_PLANT))
        .count()
        == 1
}

/// What a growing plant sees: what's placed so far over the ground.
struct Growth<'a, F: Fn(IVec3) -> bool> {
    solid: &'a F,
    placed: HashMap<IVec3, BlockState>,
}

impl<F: Fn(IVec3) -> bool> Growth<'_, F> {
    fn empty(&self, at: IVec3) -> bool {
        !(self.solid)(at) && !self.placed.contains_key(&at)
    }

    /// Every side but `except` is empty.
    fn sides_empty(&self, at: IVec3, except: Option<Face>) -> bool {
        SIDES
            .iter()
            .filter(|&&face| Some(face) != except)
            .all(|&face| self.empty(at + step(face)))
    }

    fn put(&mut self, at: IVec3, state: BlockState) {
        self.placed.insert(at, state);
    }

    /// `ChorusFlowerBlock.growTreeRecursive`: a stem up from `base`, then
    /// branches, or a flower if no branch could grow.
    fn grow(&mut self, base: IVec3, origin: IVec3, rng: &mut Rng, depth: u32) {
        let plant = BlockState::of(blocks::CHORUS_PLANT);
        let mut height = rng.below(4) as i32 + 1;
        if depth == 0 {
            height += 1;
        }
        for i in 0..height {
            let at = base + IVec3::Y * (i + 1);
            if !self.sides_empty(at, None) {
                // Blocked: the stem ends here, in a flower.
                self.put(base + IVec3::Y * i, tip());
                return;
            }
            self.put(at, plant);
        }
        let mut branched = false;
        if depth < MAX_DEPTH {
            let mut branches = rng.below(4);
            if depth == 0 {
                branches += 1;
            }
            for _ in 0..branches {
                let face = SIDES[rng.below(4) as usize];
                let at = base + IVec3::Y * height + step(face);
                if (at.x - origin.x).abs() < MAX_HORIZONTAL
                    && (at.z - origin.z).abs() < MAX_HORIZONTAL
                    && self.empty(at)
                    && self.empty(at - IVec3::Y)
                    && self.sides_empty(at, Some(opposite(face)))
                {
                    branched = true;
                    self.put(at, plant);
                    self.grow(at, origin, rng, depth + 1);
                }
            }
        }
        if !branched {
            self.put(base + IVec3::Y * height, tip());
        }
    }
}

/// A whole plant grown from `origin` (the cell above end stone), as
/// `ChorusFlowerBlock.generatePlant` does, over ground where `solid` says so.
/// The blocks come back joined to their neighbors.
pub fn grow_plant(
    origin: IVec3,
    rng: &mut Rng,
    solid: &impl Fn(IVec3) -> bool,
) -> Vec<(IVec3, BlockState)> {
    let mut growth = Growth {
        solid,
        placed: HashMap::new(),
    };
    growth.put(origin, BlockState::of(blocks::CHORUS_PLANT));
    growth.grow(origin, origin, rng, 0);
    let placed = growth.placed.clone();
    let get = |p: IVec3| {
        placed
            .get(&p)
            .copied()
            .or_else(|| solid(p).then(|| BlockState::of(blocks::END_STONE)))
    };
    let mut blocks: Vec<_> = placed
        .iter()
        .map(|(&at, &state)| {
            if state.is(blocks::CHORUS_PLANT) {
                (at, connect(state, at, &get))
            } else {
                (at, state)
            }
        })
        .collect();
    blocks.sort_by_key(|&(at, _)| (at.y, at.x, at.z));
    blocks
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::harvest;

    const FLOOR: i32 = 63;

    fn ground(at: IVec3) -> bool {
        at.y <= FLOOR
    }

    fn plant(seed: u64) -> Vec<(IVec3, BlockState)> {
        grow_plant(IVec3::new(0, FLOOR + 1, 0), &mut Rng::new(seed), &ground)
    }

    #[test]
    fn a_plant_is_stems_with_flowers_at_every_tip() {
        let mut tall = 0;
        for seed in 0..300 {
            let blocks = plant(seed);
            let at: HashMap<_, _> = blocks.iter().copied().collect();
            let top = blocks.iter().map(|b| b.0.y).max().unwrap();
            tall = tall.max(top - FLOOR);
            for &(pos, state) in &blocks {
                assert!(
                    state.is(blocks::CHORUS_PLANT) || state.is(blocks::CHORUS_FLOWER),
                    "{state:?}"
                );
                let joined = Face::ALL
                    .iter()
                    .filter(|&&face| {
                        at.get(&(pos + step(face))).is_some_and(|&s| is_chorus(s))
                            || (face == Face::Bottom && ground(pos + step(face)))
                    })
                    .count();
                assert!(joined >= 1, "seed {seed}: {pos} joins nothing");
                if state.is(blocks::CHORUS_PLANT) {
                    assert!(
                        joined >= 2,
                        "seed {seed}: a stem ends at {pos} with no flower"
                    );
                } else {
                    assert_eq!(state.number(Prop::Age5), 5);
                    assert_eq!(joined, 1, "a flower tops one stem");
                }
            }
            assert!(blocks.iter().any(|(_, s)| s.is(blocks::CHORUS_FLOWER)));
            assert!(top - FLOOR <= 24, "seed {seed}: {} tall", top - FLOOR);
        }
        assert!(tall >= 10, "some plants are tall: {tall}");
    }

    #[test]
    fn stems_are_joined_to_the_blocks_around_them() {
        let blocks = plant(7);
        let at: HashMap<_, _> = blocks.iter().copied().collect();
        let base = at[&IVec3::new(0, FLOOR + 1, 0)];
        assert!(base.bool(Prop::Down), "joined to the end stone");
        assert!(base.bool(Prop::Up));
        assert!(!base.bool(Prop::North));
    }

    #[test]
    fn a_plant_stays_within_eight_blocks_of_its_base() {
        for seed in 0..300 {
            for (pos, _) in plant(seed) {
                assert!(pos.x.abs() < MAX_HORIZONTAL && pos.z.abs() < MAX_HORIZONTAL);
            }
        }
    }

    #[test]
    fn a_thousand_stems_drop_about_five_hundred_fruit() {
        let mut rng = Rng::new(99);
        let mut random = || rng.next_f64() as f32;
        let state = BlockState::of(blocks::CHORUS_PLANT);
        let fruit: u32 = (0..1000)
            .flat_map(|_| harvest::drops_with(state, None, 3, false, &mut random))
            .map(|stack| u32::from(stack.count))
            .sum();
        assert!((450..=550).contains(&fruit), "{fruit} fruit, Fortune III");
    }

    #[test]
    fn support_rules_follow_vanilla() {
        let plant = BlockState::of(blocks::CHORUS_PLANT);
        let end = BlockState::of(blocks::END_STONE);
        let world: HashMap<IVec3, BlockState> = [
            (IVec3::new(0, 0, 0), end),
            (IVec3::new(0, 1, 0), plant),
            (IVec3::new(0, 2, 0), plant),
            (IVec3::new(1, 2, 0), plant),
            (IVec3::new(5, 5, 5), plant),
        ]
        .into();
        let get = |p: IVec3| world.get(&p).copied();
        assert!(plant_survives(IVec3::new(0, 1, 0), &get), "on end stone");
        assert!(plant_survives(IVec3::new(0, 2, 0), &get), "on a plant");
        assert!(
            plant_survives(IVec3::new(1, 2, 0), &get),
            "beside a supported plant"
        );
        assert!(!plant_survives(IVec3::new(5, 5, 5), &get), "floating");
        assert!(flower_survives(IVec3::new(0, 3, 0), &get), "on a plant");
        assert!(
            flower_survives(IVec3::new(2, 2, 0), &get),
            "beside one plant, over air"
        );
        assert!(!flower_survives(IVec3::new(9, 9, 9), &get), "floating");
    }
}
