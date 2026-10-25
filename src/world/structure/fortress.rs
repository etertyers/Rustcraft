//! Nether fortresses (modern `NetherFortressPieces`, simplified): nether
//! brick bridges on pillars running down through the lava to the ground,
//! joined at crossings, with blaze spawners on raised thrones, leading into
//! an enclosed castle of corridors, crossings and turns (some with chests)
//! and nether wart rooms. The deck stands between y 48 and 70.

use glam::IVec3;

use super::{Bounds, Piece, build::Build};
use crate::world::{
    Face,
    block::BlockState,
    blocks,
    chunk::ChunkPos,
    props::Prop,
    worldgen::{nether::Probe, rng::Rng},
};

/// Pieces reach at most this far from the start, this many joins out.
const MAX_DISTANCE: i32 = 112;
const MAX_DEPTH: u32 = 24;
const MAX_PIECES: usize = 70;
/// Blaze thrones and wart rooms per fortress, at most (modern 2 each).
const MAX_THRONES: u32 = 2;
const MAX_WART_ROOMS: u32 = 2;
/// A castle turn holds a chest one time in three.
const CHEST_CHANCE: u32 = 3;
/// Deepest a pillar reaches, for a fortress over the bedrock.
const PILLAR_FLOOR: i32 = 1;

fn bricks() -> BlockState {
    BlockState::of(blocks::NETHER_BRICKS)
}

fn fence() -> BlockState {
    BlockState::of(blocks::NETHER_BRICK_FENCE)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    BridgeCrossing,
    Bridge,
    BridgeEnd,
    Throne,
    Corridor,
    CastleCrossing,
    Turn,
    WartRoom,
}

/// Where a piece may join: the first cell past the last piece, at walking
/// height, the way out, and how many joins from the start.
#[derive(Debug, Clone, Copy)]
struct Exit {
    at: IVec3,
    facing: Face,
    depth: u32,
    /// Leads on from the castle (corridors) rather than a bridge.
    castle: bool,
}

/// A piece's own axes: `u` to its right, `v` up, `w` forward from the
/// entrance at walking height.
#[derive(Debug, Clone, Copy)]
struct Frame {
    origin: IVec3,
    dir: IVec3,
    side: IVec3,
    facing: Face,
}

impl Frame {
    fn new(origin: IVec3, facing: Face) -> Self {
        let [nx, _, nz] = facing.normal();
        Self {
            origin,
            dir: IVec3::new(nx, 0, nz),
            side: IVec3::new(-nz, 0, nx),
            facing,
        }
    }

    fn at(&self, u: i32, v: i32, w: i32) -> IVec3 {
        self.origin + self.side * u + IVec3::Y * v + self.dir * w
    }

    fn fill(
        &self,
        build: &mut Build,
        (u0, u1): (i32, i32),
        (v0, v1): (i32, i32),
        (w0, w1): (i32, i32),
        block: BlockState,
    ) {
        for w in w0..=w1 {
            for v in v0..=v1 {
                for u in u0..=u1 {
                    build.set(self.at(u, v, w), block);
                }
            }
        }
    }

    fn bounds(&self, half: i32, height: i32, length: i32) -> Bounds {
        Bounds::new(self.at(-half, -1, 0), self.at(half, height, length - 1))
    }

    /// The exit `w` blocks forward along `turn` (straight, left or right),
    /// from the piece's middle `mid` blocks in.
    fn exit(&self, turn: Turn, length: i32, half: i32, mid: i32) -> (IVec3, Face) {
        match turn {
            Turn::Straight => (self.at(0, 0, length), self.facing),
            Turn::Left => (self.at(-half - 1, 0, mid), turned(self.facing, false)),
            Turn::Right => (self.at(half + 1, 0, mid), turned(self.facing, true)),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Turn {
    Straight,
    Left,
    Right,
}

fn turned(facing: Face, right: bool) -> Face {
    let order = [Face::North, Face::East, Face::South, Face::West];
    let i = order.iter().position(|&f| f == facing).unwrap_or(0);
    order[(i + if right { 1 } else { 3 }) % 4]
}

struct Layout<'a, 'p> {
    probe: &'a mut Probe<'p>,
    rng: &'a mut Rng,
    center: IVec3,
    taken: Vec<Bounds>,
    pieces: Vec<Piece>,
    thrones: u32,
    wart_rooms: u32,
}

impl Layout<'_, '_> {
    fn fits(&self, bounds: &Bounds) -> bool {
        let far = [bounds.min, bounds.max].iter().any(|c| {
            (c.x - self.center.x).abs() > MAX_DISTANCE || (c.z - self.center.z).abs() > MAX_DISTANCE
        });
        !far && bounds.max.y < 120 && !self.taken.iter().any(|t| t.intersects(bounds))
    }

    /// Pillars of nether brick from just under `tops` down through air and
    /// lava to the ground.
    fn pillars(&mut self, tops: impl IntoIterator<Item = IVec3>) {
        let mut build = Build::new();
        for top in tops {
            let mut y = top.y;
            while y > PILLAR_FLOOR && !self.probe.solid(top.x, y, top.z) {
                build.set(IVec3::new(top.x, y, top.z), bricks());
                y -= 1;
            }
        }
        let mut piece = build.finish();
        piece.support = true;
        if piece.bounds.is_some() {
            self.pieces.push(piece);
        }
    }

    /// Lays `kind` at `exit`, returning its exits, or `None` if it doesn't
    /// fit.
    fn place(&mut self, kind: Kind, exit: Exit) -> Option<Vec<Exit>> {
        let frame = Frame::new(exit.at, exit.facing);
        let (half, height, length) = match kind {
            Kind::BridgeCrossing => (4, 3, 9),
            Kind::Bridge => (2, 2, 13),
            Kind::BridgeEnd => (2, 2, 3),
            Kind::Throne => (3, 4, 8),
            Kind::Corridor => (2, 4, 5),
            Kind::CastleCrossing | Kind::Turn => (2, 4, 5),
            Kind::WartRoom => (6, 6, 13),
        };
        let bounds = frame.bounds(half, height, length);
        if !self.fits(&bounds) {
            return None;
        }
        self.taken.push(bounds);
        let depth = exit.depth + 1;
        let mut build = Build::new();
        let mut exits = Vec::new();
        let mut leave = |turn: Turn, mid: i32, castle: bool| {
            let (at, facing) = frame.exit(turn, length, half, mid);
            exits.push(Exit {
                at,
                facing,
                depth,
                castle,
            });
        };
        let mut supports = Vec::new();
        match kind {
            Kind::BridgeCrossing => {
                frame.fill(
                    &mut build,
                    (-half, half),
                    (-1, -1),
                    (0, length - 1),
                    bricks(),
                );
                frame.fill(
                    &mut build,
                    (-half, half),
                    (0, height),
                    (0, length - 1),
                    BlockState::AIR,
                );
                // Railings round the edge but for the three ways on and the
                // way in.
                for w in 0..length {
                    for u in [-half, half] {
                        if (w - length / 2).abs() > 2 {
                            build.set(frame.at(u, 0, w), fence());
                        }
                    }
                }
                for u in -half..=half {
                    if u.abs() > 2 {
                        build.set(frame.at(u, 0, length - 1), fence());
                        build.set(frame.at(u, 0, 0), fence());
                    }
                }
                for u in [-2, 2] {
                    for w in [2, length - 3] {
                        supports.push(frame.at(u, -2, w));
                    }
                }
                leave(Turn::Straight, 0, false);
                leave(Turn::Left, length / 2, false);
                leave(Turn::Right, length / 2, false);
            }
            Kind::Bridge | Kind::BridgeEnd => {
                frame.fill(
                    &mut build,
                    (-half, half),
                    (-1, -1),
                    (0, length - 1),
                    bricks(),
                );
                frame.fill(
                    &mut build,
                    (-half, half),
                    (0, height),
                    (0, length - 1),
                    BlockState::AIR,
                );
                // A beam along each edge under the deck, and railings.
                frame.fill(
                    &mut build,
                    (-half, -half),
                    (-2, -2),
                    (0, length - 1),
                    bricks(),
                );
                frame.fill(
                    &mut build,
                    (half, half),
                    (-2, -2),
                    (0, length - 1),
                    bricks(),
                );
                frame.fill(&mut build, (-half, -half), (0, 0), (0, length - 1), fence());
                frame.fill(&mut build, (half, half), (0, 0), (0, length - 1), fence());
                if kind == Kind::Bridge {
                    for w in [1, length - 2] {
                        for u in -half..=half {
                            supports.push(frame.at(u, -3, w));
                        }
                    }
                    leave(Turn::Straight, 0, false);
                } else {
                    frame.fill(
                        &mut build,
                        (-half, half),
                        (0, 0),
                        (length - 1, length - 1),
                        fence(),
                    );
                    for u in -half..=half {
                        supports.push(frame.at(u, -2, length - 1));
                    }
                }
            }
            Kind::Throne => {
                frame.fill(
                    &mut build,
                    (-half, half),
                    (-1, -1),
                    (0, length - 1),
                    bricks(),
                );
                frame.fill(
                    &mut build,
                    (-half, half),
                    (0, height),
                    (0, length - 1),
                    BlockState::AIR,
                );
                frame.fill(&mut build, (-half, -half), (0, 0), (0, length - 1), fence());
                frame.fill(&mut build, (half, half), (0, 0), (0, length - 1), fence());
                frame.fill(
                    &mut build,
                    (-half, half),
                    (0, 0),
                    (length - 1, length - 1),
                    fence(),
                );
                // The dais, the spawner on it, and posts at its corners.
                frame.fill(&mut build, (-1, 1), (0, 0), (3, 5), bricks());
                for (u, w) in [(-1, 3), (1, 3), (-1, 5), (1, 5)] {
                    build.set(frame.at(u, 1, w), fence());
                }
                build.spawner(frame.at(0, 1, 4), "blaze");
                for u in [-half, half] {
                    for w in [1, length - 1] {
                        supports.push(frame.at(u, -2, w));
                    }
                }
                self.thrones += 1;
            }
            Kind::Corridor | Kind::CastleCrossing | Kind::Turn | Kind::WartRoom => {
                frame.fill(
                    &mut build,
                    (-half, half),
                    (-1, -1),
                    (0, length - 1),
                    bricks(),
                );
                frame.fill(
                    &mut build,
                    (-half, half),
                    (height, height),
                    (0, length - 1),
                    bricks(),
                );
                frame.fill(
                    &mut build,
                    (-half, -half),
                    (0, height - 1),
                    (0, length - 1),
                    bricks(),
                );
                frame.fill(
                    &mut build,
                    (half, half),
                    (0, height - 1),
                    (0, length - 1),
                    bricks(),
                );
                frame.fill(
                    &mut build,
                    (-half + 1, half - 1),
                    (0, height - 1),
                    (0, length - 1),
                    BlockState::AIR,
                );
                // Fence windows along the walls.
                for w in (1..length - 1).step_by(2) {
                    for u in [-half, half] {
                        build.set(frame.at(u, 1, w), fence());
                        build.set(frame.at(u, 2, w), fence());
                    }
                }
                let door = |build: &mut Build, u: i32, w: i32, across: bool| {
                    for v in 0..3 {
                        for k in -1..=1 {
                            let (du, dw) = if across { (0, k) } else { (k, 0) };
                            build.set(frame.at(u + du, v, w + dw), BlockState::AIR);
                        }
                    }
                };
                match kind {
                    Kind::Corridor => leave(Turn::Straight, 0, true),
                    Kind::CastleCrossing => {
                        door(&mut build, -half, length / 2, true);
                        door(&mut build, half, length / 2, true);
                        leave(Turn::Straight, 0, true);
                        leave(Turn::Left, length / 2, true);
                        leave(Turn::Right, length / 2, true);
                    }
                    Kind::Turn => {
                        let right = self.rng.below(2) == 0;
                        let side = if right { half } else { -half };
                        door(&mut build, side, length / 2, true);
                        // Closed ahead.
                        frame.fill(
                            &mut build,
                            (-half, half),
                            (0, height - 1),
                            (length - 1, length - 1),
                            bricks(),
                        );
                        let turn = if right { Turn::Right } else { Turn::Left };
                        leave(turn, length / 2, true);
                        if self.rng.below(CHEST_CHANCE) == 0 {
                            let corner = frame.at(-side.signum(), 0, length - 2);
                            build.chest(corner, BlockState::of(blocks::CHEST), "nether_bridge");
                        }
                    }
                    _ => {
                        // Two beds of soul sand with nether wart, and a way
                        // on ahead and to each side.
                        for u in [-4, -3, 3, 4] {
                            for w in 2..=10 {
                                build.set(frame.at(u, -1, w), BlockState::of(blocks::SOUL_SAND));
                                let age = self.rng.below(4) as u8;
                                let wart =
                                    BlockState::of(blocks::NETHER_WART).with(Prop::Age3, age);
                                build.set(frame.at(u, 0, w), wart);
                            }
                        }
                        door(&mut build, 0, length - 1, false);
                        door(&mut build, -half, length / 2, true);
                        door(&mut build, half, length / 2, true);
                        leave(Turn::Straight, 0, true);
                        leave(Turn::Left, length / 2, true);
                        leave(Turn::Right, length / 2, true);
                        self.wart_rooms += 1;
                    }
                }
                // The castle stands on brick down to the ground.
                for w in 0..length {
                    for u in [-half, 0, half] {
                        supports.push(frame.at(u, -2, w));
                    }
                }
            }
        }
        self.pieces.push(build.finish());
        self.pillars(supports);
        Some(exits)
    }

    /// The piece to try at `exit`, by weight (modern's piece weights,
    /// fewer kinds).
    fn pick(&mut self, exit: &Exit) -> Kind {
        let mut options: Vec<(Kind, u32)> = if exit.castle {
            vec![
                (Kind::Corridor, 25),
                (Kind::CastleCrossing, 15),
                (Kind::Turn, 15),
            ]
        } else {
            vec![
                (Kind::Bridge, 30),
                (Kind::BridgeCrossing, 10),
                (Kind::Corridor, 10),
            ]
        };
        // Every fortress gets a blaze throne: once half the pieces are down
        // without one, the next bridge exit takes it.
        if !exit.castle && self.thrones == 0 && self.pieces.len() >= MAX_PIECES / 2 {
            return Kind::Throne;
        }
        if !exit.castle && self.thrones < MAX_THRONES {
            options.push((Kind::Throne, 15));
        }
        if exit.castle && self.wart_rooms < MAX_WART_ROOMS {
            options.push((Kind::WartRoom, 10));
        }
        let total: u32 = options.iter().map(|o| o.1).sum();
        let mut roll = self.rng.below(total);
        for (kind, weight) in &options {
            if roll < *weight {
                return *kind;
            }
            roll -= weight;
        }
        options[0].0
    }

    /// Closes an exit nothing joined: a railing across a bridge's end, a
    /// wall over a doorway.
    fn cap(&mut self, exit: &Exit) {
        let frame = Frame::new(exit.at, exit.facing);
        let mut build = Build::new();
        if exit.castle {
            frame.fill(&mut build, (-1, 1), (0, 2), (-1, -1), bricks());
        } else {
            frame.fill(&mut build, (-2, 2), (0, 0), (-1, -1), fence());
        }
        self.pieces.push(build.finish());
    }
}

/// Lays out the fortress starting in chunk `start`.
pub fn generate(probe: &mut Probe, start: ChunkPos, rng: &mut Rng) -> Option<Vec<Piece>> {
    let (x, z) = start.min_block();
    let deck = 48 + rng.below(23) as i32;
    let center = IVec3::new(x + 8, deck, z + 8);
    let facing = [Face::North, Face::East, Face::South, Face::West][rng.below(4) as usize];
    let mut layout = Layout {
        probe,
        rng,
        center,
        taken: Vec::new(),
        pieces: Vec::new(),
        thrones: 0,
        wart_rooms: 0,
    };
    let first = Exit {
        at: center,
        facing,
        depth: 0,
        castle: false,
    };
    let mut open = layout.place(Kind::BridgeCrossing, first)?;
    // The way in the start crossing has, too.
    let back = Frame::new(center, facing);
    open.push(Exit {
        at: back.at(0, 0, -1),
        facing: turned(turned(facing, true), true),
        depth: 1,
        castle: false,
    });
    while !open.is_empty() && layout.pieces.len() < MAX_PIECES {
        let i = layout.rng.below(open.len() as u32) as usize;
        let exit = open.swap_remove(i);
        if exit.depth >= MAX_DEPTH {
            layout.cap(&exit);
            continue;
        }
        let kind = layout.pick(&exit);
        let fallback = if exit.castle {
            Kind::Corridor
        } else {
            Kind::BridgeEnd
        };
        match layout
            .place(kind, exit)
            .or_else(|| layout.place(fallback, exit))
        {
            Some(more) => open.extend(more),
            None => layout.cap(&exit),
        }
    }
    // Still no throne: try one at each open bridge exit before capping.
    let mut capped = Vec::new();
    for exit in open {
        if layout.thrones == 0
            && !exit.castle
            && let Some(more) = layout.place(Kind::Throne, exit)
        {
            capped.extend(more);
        } else {
            capped.push(exit);
        }
    }
    for exit in capped {
        layout.cap(&exit);
    }
    // A fortress without a blaze spawner isn't one.
    (layout.thrones > 0).then_some(layout.pieces)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::{
        structure::{Kind as StructureKind, nether},
        worldgen::nether::Nether,
    };

    #[test]
    fn every_fortress_has_a_blaze_spawner() {
        let world = Nether::new(20111118);
        let mut seen = std::collections::HashSet::new();
        for i in 0..12 {
            for j in 0..3 {
                let Some(found) = nether::find(
                    &world,
                    StructureKind::Fortress,
                    ChunkPos::new(i * 40 - 240, j * 60 - 60),
                    30,
                ) else {
                    continue;
                };
                if !seen.insert(found.start) {
                    continue;
                }
                let blazes = found
                    .pieces
                    .iter()
                    .flat_map(|p| &p.spawners)
                    .filter(|(_, m)| m == "blaze")
                    .count();
                assert!(
                    blazes >= 1,
                    "fortress at {:?} has no blaze spawner",
                    found.start
                );
            }
        }
        assert!(seen.len() >= 5, "only {} fortresses sampled", seen.len());
    }

    #[test]
    fn a_fortress_has_bridges_on_pillars_blaze_spawners_and_castle_rooms() {
        let world = Nether::new(20111118);
        let found = (0..20)
            .filter_map(|i| {
                nether::find(
                    &world,
                    StructureKind::Fortress,
                    ChunkPos::new(i * 40, 0),
                    60,
                )
            })
            .next()
            .expect("a fortress near the origin");
        let blocks: Vec<_> = found.pieces.iter().flat_map(|p| &p.blocks).collect();
        let count = |id| blocks.iter().filter(|(_, b)| b.is(id)).count();
        assert!(found.pieces.len() > 20, "{} pieces", found.pieces.len());
        assert!(count(blocks::NETHER_BRICKS) > 2000);
        assert!(count(blocks::NETHER_BRICK_FENCE) > 50);
        let spawners: Vec<_> = found.pieces.iter().flat_map(|p| &p.spawners).collect();
        assert!(!spawners.is_empty() && spawners.iter().all(|(_, m)| m == "blaze"));
        assert!(found.pieces.iter().any(|p| p.support), "pillars");
        let deck = found.bounds.max.y;
        assert!(deck <= 80, "deck near y 48–70, top at {deck}");
        // The same seed lays it out the same.
        let again = nether::structure_at(&world, StructureKind::Fortress, found.start).unwrap();
        assert_eq!(again.pieces.len(), found.pieces.len());
    }
}
