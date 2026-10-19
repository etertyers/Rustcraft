//! Joining template pieces jigsaw-style (modern `JigsawPlacement`): from a
//! start piece, every open connector looks for a piece in its pool with a
//! connector facing back at it, turned to fit, that doesn't overlap
//! what's already there and stands on dry land. Pieces sit on the ground
//! where they join, so a village follows the hills.

use std::collections::VecDeque;

use glam::IVec3;

use super::{
    Bounds, Piece,
    template::{PlacedJigsaw, Registry, Template},
};
use crate::{
    config::SEA_LEVEL,
    world::{
        biome::Biome,
        worldgen::{rng::Rng, terrain::Terrain},
    },
};

pub struct Options {
    /// How many joins away from the start a piece may be.
    pub max_depth: u32,
    /// How far (in blocks, across) any piece may reach from the start.
    pub max_distance: i32,
}

/// Whether column (x, z) is dry land a piece may stand on.
pub fn dry(terrain: &Terrain, x: i32, z: i32) -> bool {
    let column = terrain.column(x, z);
    column.height >= SEA_LEVEL
        && !matches!(
            column.biome,
            Biome::River | Biome::Ocean | Biome::DeepOcean | Biome::Beach
        )
}

/// The names in `pool`, in a random order favoring heavier ones.
fn shuffled(registry: &Registry, pool: &str, rng: &mut Rng) -> Vec<String> {
    let mut left: Vec<(String, u32)> = registry
        .pool(pool)
        .iter()
        .map(|name| {
            (
                name.clone(),
                registry.get(name).map_or(1, |t| t.weight.max(1)),
            )
        })
        .collect();
    let mut order = Vec::with_capacity(left.len());
    while !left.is_empty() {
        let total: u32 = left.iter().map(|(_, w)| w).sum();
        let mut pick = rng.below(total);
        let index = left
            .iter()
            .position(|(_, w)| {
                let hit = pick < *w;
                pick = pick.saturating_sub(*w);
                hit
            })
            .unwrap_or(0);
        order.push(left.remove(index).0);
    }
    order
}

/// The footprint of `template` at `origin` after `turns`.
fn footprint(template: &Template, origin: IVec3, turns: u8) -> Bounds {
    let [w, h, d] = template.turned_size(turns);
    Bounds::new(origin, origin + IVec3::new(w - 1, h - 1, d - 1))
}

/// Whether all of `bounds`'s corners and middle stand on dry land.
fn on_land(terrain: &Terrain, bounds: &Bounds) -> bool {
    let (a, b) = (bounds.min, bounds.max);
    [
        (a.x, a.z),
        (b.x, a.z),
        (a.x, b.z),
        (b.x, b.z),
        ((a.x + b.x) / 2, (a.z + b.z) / 2),
    ]
    .into_iter()
    .all(|(x, z)| dry(terrain, x, z))
}

/// Builds pieces outward from a start piece out of `start_pool`, centered
/// on column `center`. `None` if the start can't stand there.
pub fn assemble(
    terrain: &Terrain,
    registry: &Registry,
    start_pool: &str,
    center: (i32, i32),
    rng: &mut Rng,
    options: &Options,
) -> Option<Vec<Piece>> {
    let start_name = shuffled(registry, start_pool, rng).into_iter().next()?;
    let start = registry.get(&start_name)?;
    let turns = rng.below(4) as u8;
    let [w, _, d] = start.turned_size(turns);
    let ground = terrain.height(center.0, center.1);
    let origin = IVec3::new(center.0 - w / 2, ground, center.1 - d / 2);
    let start_bounds = footprint(start, origin, turns);
    if !on_land(terrain, &start_bounds) {
        return None;
    }
    let mut taken = vec![start_bounds];
    let mut pieces = vec![start.place(origin, turns)];
    let mut open: VecDeque<(PlacedJigsaw, u32)> = start
        .jigsaws_at(origin, turns)
        .into_iter()
        .map(|j| (j, 0))
        .collect();
    let limit = |b: &Bounds| {
        (b.min.x - center.0).abs().max((b.max.x - center.0).abs()) <= options.max_distance
            && (b.min.z - center.1).abs().max((b.max.z - center.1).abs()) <= options.max_distance
    };
    while let Some((jigsaw, depth)) = open.pop_front() {
        if depth >= options.max_depth {
            continue;
        }
        let [nx, _, nz] = jigsaw.facing.normal();
        let target = jigsaw.at + IVec3::new(nx, 0, nz);
        let ground = terrain.height(target.x, target.z);
        let mut placed = false;
        'names: for name in shuffled(registry, &jigsaw.pool, rng) {
            let Some(template) = registry.get(&name) else {
                continue;
            };
            let first_turn = rng.below(4) as u8;
            for turn in 0..4 {
                let turns = (first_turn + turn) % 4;
                // Connectors of this piece that face back at the open one.
                let back = template
                    .jigsaws_at(IVec3::ZERO, turns)
                    .into_iter()
                    .enumerate()
                    .filter(|(_, k)| k.facing == jigsaw.facing.opposite())
                    .collect::<Vec<_>>();
                for (index, k) in back {
                    let origin = IVec3::new(target.x - k.at.x, ground - k.at.y, target.z - k.at.z);
                    let bounds = footprint(template, origin, turns);
                    if !limit(&bounds)
                        || taken.iter().any(|t| t.intersects_xz(&bounds))
                        || !on_land(terrain, &bounds)
                    {
                        continue;
                    }
                    taken.push(bounds);
                    pieces.push(template.place(origin, turns));
                    for (i, next) in template.jigsaws_at(origin, turns).into_iter().enumerate() {
                        if i != index {
                            open.push_back((next, depth + 1));
                        }
                    }
                    placed = true;
                    break 'names;
                }
            }
        }
        let _ = placed;
    }
    Some(pieces)
}
