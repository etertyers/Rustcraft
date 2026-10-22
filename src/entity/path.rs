//! Path finding for mobs, after 1.0.0's `Pathfinder`: A* over block cells
//! where a mob can stand, stepping up one block, dropping up to three, and
//! swimming, but never into lava. If the goal can't be reached the path
//! leads to the reachable cell closest to it.

use std::collections::BinaryHeap;

use glam::IVec3;

use crate::world::fast_hash::FastMap;

/// What a path finder needs to know about a cell.
pub trait Terrain {
    /// Nothing solid in the cell.
    fn passable(&self, cell: IVec3) -> bool;
    /// The cell is water (mobs swim through it without ground).
    fn water(&self, cell: IVec3) -> bool;
    /// The cell is lava, which mobs never enter.
    fn lava(&self, cell: IVec3) -> bool;
    /// Standing in the cell burns or pricks (fire, cactus, sweet berry
    /// bushes, powder snow), or standing on it does (magma): mobs path
    /// around it.
    fn hazard(&self, _cell: IVec3) -> bool {
        false
    }
    /// Standing on this cell's top burns (magma blocks).
    fn hot_floor(&self, _cell: IVec3) -> bool {
        false
    }
}

/// Cells searched per block of follow range before giving up (1.0.0
/// searches a bounded area too), within these limits.
const NODES_PER_BLOCK: f32 = 30.0;
const MIN_NODES: usize = 400;
const MAX_NODES: usize = 1200;
/// Furthest drop a path takes (1.0.0's `getSafePoint`).
const MAX_DROP: i32 = 3;

/// A mob that's `height` cells tall can stand with its feet in `cell`.
fn fits(terrain: &impl Terrain, cell: IVec3, height: i32) -> bool {
    (0..height).all(|dy| {
        let c = cell + IVec3::Y * dy;
        terrain.passable(c) && !terrain.lava(c) && !terrain.hazard(c)
    }) && !terrain.hot_floor(cell - IVec3::Y)
}

/// Where a mob moving from `from` one cell sideways by `step` ends up.
fn step_to(
    terrain: &impl Terrain,
    from: IVec3,
    step: IVec3,
    height: i32,
    max_drop: i32,
) -> Option<IVec3> {
    let mut cell = from + step;
    if !fits(terrain, cell, height) {
        // Jump up onto the block if it's solid with room above it, and
        // there's room to jump.
        let up = cell + IVec3::Y;
        let onto_solid = !terrain.passable(cell);
        if onto_solid && fits(terrain, up, height) && fits(terrain, from + IVec3::Y, height) {
            return Some(up);
        }
        return None;
    }
    // Drop down to the ground, or stay up in water.
    let mut dropped = 0;
    let open = |c: IVec3| terrain.passable(c) && !terrain.lava(c);
    while !terrain.water(cell) && open(cell - IVec3::Y) {
        cell -= IVec3::Y;
        dropped += 1;
        if dropped > max_drop || cell.y < 0 {
            return None;
        }
    }
    // Where it lands must be safe to stand in.
    fits(terrain, cell, height).then_some(cell)
}

#[derive(PartialEq)]
struct Open {
    cost: f32,
    cell: IVec3,
}

impl Eq for Open {}

impl Ord for Open {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        // Lowest cost first.
        other.cost.total_cmp(&self.cost)
    }
}

impl PartialOrd for Open {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

/// Cells from `start` (excluded) toward `goal`, for a mob `height` cells
/// tall, no farther than `max_distance` from the start.
pub fn find_path(
    terrain: &impl Terrain,
    start: IVec3,
    goal: IVec3,
    height: i32,
    max_distance: f32,
) -> Vec<IVec3> {
    find_path_dropping(terrain, start, goal, height, max_distance, MAX_DROP)
}

/// `find_path` with a limit on how far one step may drop (a mob that is
/// only wandering stays off cliffs).
pub fn find_path_dropping(
    terrain: &impl Terrain,
    start: IVec3,
    goal: IVec3,
    height: i32,
    max_distance: f32,
    max_drop: i32,
) -> Vec<IVec3> {
    let estimate = |cell: IVec3| (goal - cell).as_vec3().length();
    let mut open = BinaryHeap::new();
    let mut came_from: FastMap<IVec3, IVec3> = FastMap::default();
    let mut walked: FastMap<IVec3, f32> = FastMap::default();
    open.push(Open {
        cost: estimate(start),
        cell: start,
    });
    walked.insert(start, 0.0);
    let mut closest = start;
    let mut searched = 0;
    let max_nodes = ((max_distance * NODES_PER_BLOCK) as usize).clamp(MIN_NODES, MAX_NODES);
    while let Some(Open { cell, .. }) = open.pop() {
        if cell == goal {
            closest = cell;
            break;
        }
        if estimate(cell) < estimate(closest) {
            closest = cell;
        }
        searched += 1;
        if searched > max_nodes {
            break;
        }
        for step in [IVec3::X, IVec3::NEG_X, IVec3::Z, IVec3::NEG_Z] {
            let Some(next) = step_to(terrain, cell, step, height, max_drop) else {
                continue;
            };
            if (next - start).as_vec3().length() > max_distance {
                continue;
            }
            let cost = walked[&cell] + (next - cell).as_vec3().length();
            if walked.get(&next).is_none_or(|&known| cost < known) {
                walked.insert(next, cost);
                came_from.insert(next, cell);
                open.push(Open {
                    cost: cost + estimate(next),
                    cell: next,
                });
            }
        }
    }
    let mut path = vec![closest];
    while let Some(&previous) = came_from.get(path.last().unwrap()) {
        if previous == start {
            break;
        }
        path.push(previous);
    }
    if closest == start {
        return Vec::new();
    }
    path.reverse();
    path
}

#[cfg(test)]
pub mod tests {
    use std::collections::HashSet;

    use super::*;

    /// Solid cells, water cells and lava cells, with a floor at y 63.
    #[derive(Default)]
    pub struct Grid {
        pub solid: HashSet<IVec3>,
        pub water: HashSet<IVec3>,
        pub lava: HashSet<IVec3>,
        pub hazard: HashSet<IVec3>,
        pub hot: HashSet<IVec3>,
    }

    impl Grid {
        pub fn floor() -> Self {
            let mut grid = Self::default();
            for z in -30..30 {
                for x in -30..30 {
                    grid.solid.insert(IVec3::new(x, 63, z));
                }
            }
            grid
        }
    }

    impl Terrain for Grid {
        fn passable(&self, cell: IVec3) -> bool {
            !self.solid.contains(&cell)
        }
        fn water(&self, cell: IVec3) -> bool {
            self.water.contains(&cell)
        }
        fn lava(&self, cell: IVec3) -> bool {
            self.lava.contains(&cell)
        }
        fn hazard(&self, cell: IVec3) -> bool {
            self.hazard.contains(&cell)
        }
        fn hot_floor(&self, cell: IVec3) -> bool {
            self.hot.contains(&cell)
        }
    }

    const START: IVec3 = IVec3::new(0, 64, 0);

    #[test]
    fn walks_straight_across_open_ground() {
        let path = find_path(&Grid::floor(), START, IVec3::new(5, 64, 0), 2, 16.0);
        assert_eq!(path.len(), 5);
        assert_eq!(*path.last().unwrap(), IVec3::new(5, 64, 0));
    }

    #[test]
    fn goes_around_a_wall_and_climbs_a_step() {
        let mut grid = Grid::floor();
        for z in -3..=3 {
            for y in 64..66 {
                grid.solid.insert(IVec3::new(2, y, z));
            }
        }
        let path = find_path(&grid, START, IVec3::new(4, 64, 0), 2, 16.0);
        assert_eq!(*path.last().unwrap(), IVec3::new(4, 64, 0));
        assert!(path.iter().all(|c| c.x != 2 || c.z.abs() > 3), "{path:?}");

        let mut step = Grid::floor();
        step.solid.insert(IVec3::new(1, 64, 0));
        let path = find_path(&step, START, IVec3::new(1, 65, 0), 2, 16.0);
        assert_eq!(path, [IVec3::new(1, 65, 0)]);
    }

    #[test]
    fn avoids_lava_and_long_drops_and_heads_close_when_stuck() {
        let mut grid = Grid::floor();
        for z in -30..30 {
            grid.lava.insert(IVec3::new(3, 64, z));
        }
        let path = find_path(&grid, START, IVec3::new(6, 64, 0), 2, 16.0);
        assert!(path.iter().all(|c| c.x < 3), "never through lava: {path:?}");
        assert_eq!(path.last().unwrap().x, 2, "as close as it can get");

        let mut cliff = Grid::floor();
        for x in 1..30 {
            for z in -30..30 {
                cliff.solid.remove(&IVec3::new(x, 63, z));
                cliff.solid.insert(IVec3::new(x, 58, z));
            }
        }
        let path = find_path(&cliff, START, IVec3::new(5, 59, 0), 2, 16.0);
        assert!(
            path.iter().all(|c| c.x < 1),
            "a 5-block drop is too far: {path:?}"
        );
    }

    #[test]
    fn goes_round_a_one_block_trench_it_cannot_drop_into() {
        // A trench one block wide and five deep across the way: the
        // path goes round its end rather than into it.
        let mut grid = Grid::floor();
        for z in -6..=6 {
            grid.solid.remove(&IVec3::new(2, 63, z));
        }
        let path = find_path(&grid, START, IVec3::new(4, 64, 0), 2, 20.0);
        assert_eq!(*path.last().unwrap(), IVec3::new(4, 64, 0), "{path:?}");
        assert!(path.iter().all(|c| c.x != 2 || c.z.abs() > 6), "{path:?}");
    }

    #[test]
    fn keeps_off_fire_cactus_berries_and_magma() {
        let mut grid = Grid::floor();
        for z in -3..=3 {
            // A cactus and a fire standing in the way, and magma floor.
            grid.hazard.insert(IVec3::new(2, 64, z));
            grid.hot.insert(IVec3::new(3, 63, z));
        }
        let path = find_path(&grid, START, IVec3::new(5, 64, 0), 2, 20.0);
        assert_eq!(*path.last().unwrap(), IVec3::new(5, 64, 0), "{path:?}");
        assert!(
            path.iter()
                .all(|c| !grid.hazard.contains(c) && !grid.hot.contains(&(*c - IVec3::Y))),
            "{path:?}"
        );
    }

    #[test]
    fn a_long_path_within_a_35_block_follow_range_is_found() {
        let grid = Grid::floor();
        let path = find_path(&grid, START, IVec3::new(28, 64, 0), 2, 35.0);
        assert_eq!(*path.last().unwrap(), IVec3::new(28, 64, 0));
    }
}
