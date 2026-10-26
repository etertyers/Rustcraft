//! Nether portals in play (M24): lighting a frame, portals breaking with
//! their frame, standing in one, and travel to the linked portal in the
//! other dimension (modern `NetherPortalBlock`, `PortalForcer`).
//!
//! Portals are remembered by dimension when lit or built. Going through
//! one looks for a remembered portal near the matching spot in the other
//! dimension (x and z divided by 8 going to the Nether, times 8 coming
//! back); without one, it builds a 2 × 3 portal on the nearest ground it
//! fits, or on an obsidian ledge it makes.

use glam::{DVec3, IVec3};

use super::Game;
use crate::{
    player::mode::GameMode,
    world::{
        block::BlockState,
        blocks,
        chunk::ChunkPos,
        dimension::Dimension,
        portal::{self, Shape},
        props::axis,
        worldgen::nether,
    },
};

/// Ticks a survival player stands in a portal before it takes them; it
/// takes a creative one at once (modern `getPortalWaitTime`).
const WAIT: u32 = 80;
/// Ticks out of a portal before one works again; standing in one keeps it
/// from running down (modern `getDimensionChangingDelay`).
const COOLDOWN: u32 = 10;
/// How far from the matching spot a portal can be to link (modern
/// `PortalForcer`: 16 in the Nether, 128 in the Overworld).
const LINK_RADIUS_NETHER: i32 = 16;
const LINK_RADIUS_OVERWORLD: i32 = 128;
/// How far a new portal may be built from the matching spot.
const BUILD_RADIUS: i32 = 16;
/// How fast the portal's swirl comes on while inside and fades after
/// (modern `spinningEffectIntensity`).
const SWIRL_IN: f32 = 1.0 / WAIT as f32;
const SWIRL_OUT: f32 = 0.05;

/// Standing in portals and the swirl on screen.
#[derive(Debug, Default, Clone, serde::Serialize, serde::Deserialize)]
pub(super) struct PortalState {
    ticks: u32,
    cooldown: u32,
    pub swirl: f32,
}

impl Game {
    /// Lights the obsidian frame around `at`, filling it with portal, if
    /// there's a whole empty one. Returns whether it did.
    pub(super) fn light_portal(&mut self, at: IVec3) -> bool {
        if !matches!(self.dimension, Dimension::Overworld | Dimension::Nether) {
            return false;
        }
        let world = self.chunks.world();
        let Some(shape) = portal::find_any(at, &|p| world.block(p.x, p.y, p.z)) else {
            return false;
        };
        let edits: Vec<(IVec3, BlockState)> =
            shape.cells().map(|cell| (cell, shape.portal())).collect();
        self.edit(&edits);
        self.remember_portal(self.dimension, shape.corner);
        true
    }

    fn remember_portal(&mut self, dimension: Dimension, corner: IVec3) {
        let entry = (dimension, corner.to_array());
        if !self.portals.contains(&entry) {
            self.portals.push(entry);
        }
    }

    /// A portal block at `at` whose frame or neighbors are gone turns to
    /// air (the rest of the portal follows as its updates spread).
    pub(super) fn break_loose_portal(&mut self, at: IVec3) -> bool {
        let world = self.chunks.world();
        let Some(state) = world.block(at.x, at.y, at.z) else {
            return false;
        };
        if !state.is(blocks::NETHER_PORTAL)
            || portal::holds(at, state, &|p| world.block(p.x, p.y, p.z))
        {
            return false;
        }
        self.chunks.set_blocks(&[(at, BlockState::AIR)]);
        true
    }

    /// The portal block the player's hitbox is in, if any.
    fn portal_inside(&self) -> Option<(IVec3, BlockState)> {
        let bb = self.player.body.aabb();
        let world = self.chunks.world();
        let min = bb.min.floor().as_ivec3();
        let max = (bb.max - DVec3::splat(1e-7)).floor().as_ivec3();
        for y in min.y..=max.y {
            for z in min.z..=max.z {
                for x in min.x..=max.x {
                    if let Some(state) = world.block(x, y, z)
                        && state.is(blocks::NETHER_PORTAL)
                    {
                        return Some((IVec3::new(x, y, z), state));
                    }
                }
            }
        }
        None
    }

    /// One tick of standing in a portal or not, and travel when it's time.
    pub(super) fn tick_portal(&mut self) {
        let inside = self.portal_inside();
        let blocked = self.is_riding() || self.player.stats.is_dead();
        let state = &mut self.player.portal;
        match inside {
            Some(_) => state.swirl = (state.swirl + SWIRL_IN).min(1.0),
            None => state.swirl = (state.swirl - SWIRL_OUT).max(0.0),
        }
        let Some((cell, block)) = inside else {
            state.cooldown = state.cooldown.saturating_sub(1);
            state.ticks = 0;
            return;
        };
        if state.cooldown > 0 || blocked {
            state.cooldown = COOLDOWN;
            return;
        }
        state.ticks += 1;
        if state.ticks == 1 {
            crate::audio::play_here_with("block.portal.trigger", 0.25, 1.0);
        }
        let wait = if self.player.mode == GameMode::Creative {
            1
        } else {
            WAIT
        };
        if state.ticks >= wait {
            self.travel_through_portal(cell, portal::axis_of(block));
        }
    }

    /// How strongly the portal's swirl covers the screen, 0–1.
    pub fn portal_swirl(&self) -> f32 {
        self.player.portal.swirl
    }

    /// Takes the player through the portal at `cell` to the linked portal
    /// in the other dimension, making one there if needed.
    pub(super) fn travel_through_portal(&mut self, cell: IVec3, axis: u8) {
        let (to, scale) = match self.dimension {
            Dimension::Overworld => (Dimension::Nether, 1.0 / 8.0),
            Dimension::Nether => (Dimension::Overworld, 8.0),
            Dimension::End => return,
        };
        let feet = self.player.body.position();
        let (lo, hi) = height_range(to);
        let target = IVec3::new(
            (feet.x * scale).floor() as i32,
            cell.y.clamp(lo, hi),
            (feet.z * scale).floor() as i32,
        );
        crate::audio::play_here_with("block.portal.travel", 0.25, 1.0);
        self.change_dimension(to, target.as_dvec3() + DVec3::new(0.5, 0.0, 0.5));
        let exit = self.portal_near(target, axis);
        self.place_player(exit);
        self.player.portal.ticks = 0;
        self.player.portal.cooldown = COOLDOWN;
        if let Err(err) = self.save() {
            log::error!("saving after going to {}: {err}", to.name());
        }
    }

    /// Where a player arriving near `target` comes out: a known portal close
    /// enough, or a new one.
    fn portal_near(&mut self, target: IVec3, axis: u8) -> DVec3 {
        let radius = if self.dimension == Dimension::Nether {
            LINK_RADIUS_NETHER
        } else {
            LINK_RADIUS_OVERWORLD
        };
        let mut known: Vec<IVec3> = self
            .portals
            .iter()
            .filter(|(dimension, _)| *dimension == self.dimension)
            .map(|(_, corner)| IVec3::from_array(*corner))
            .filter(|c| (c.x - target.x).abs() <= radius && (c.z - target.z).abs() <= radius)
            .collect();
        known.sort_by_key(|c| (*c - target).length_squared());
        for corner in known {
            self.chunks.load_now(ChunkPos::of_block(corner.x, corner.z));
            if let Some(shape) = self.portal_at(corner) {
                return shape.exit();
            }
            // Broken since: forget it.
            let entry = (self.dimension, corner.to_array());
            self.portals.retain(|known| *known != entry);
        }
        self.build_portal(target, axis).exit()
    }

    /// The whole portal holding the portal block at `cell`, if it's there.
    fn portal_at(&self, cell: IVec3) -> Option<Shape> {
        let world = self.chunks.world();
        let block = world.block(cell.x, cell.y, cell.z)?;
        if !block.is(blocks::NETHER_PORTAL) {
            return None;
        }
        portal::find(cell, portal::axis_of(block), &|p| {
            world.block(p.x, p.y, p.z)
        })
    }

    /// Builds a new portal as near `target` as there's room, or on a ledge
    /// of obsidian at the target's height (modern `createPortal`).
    fn build_portal(&mut self, target: IVec3, axis: u8) -> Shape {
        let chunks = ChunkPos::of_block(target.x, target.z);
        // Chunks covering the search, plus one for the frame's edge.
        let reach = (BUILD_RADIUS + 15) / 16 + 1;
        for dz in -reach..=reach {
            for dx in -reach..=reach {
                self.chunks.load_now(chunks.offset(dx, dz));
            }
        }
        let (lo, hi) = height_range(self.dimension);
        let mut edits = Vec::new();
        let (corner, axis) = match self.portal_spot(target, axis) {
            Some(spot) => spot,
            None => {
                // A 3 × 2 ledge of obsidian with room above it.
                let corner = IVec3::new(target.x, target.y.clamp(lo.max(70), hi), target.z);
                let step = if axis == axis::Z { IVec3::Z } else { IVec3::X };
                let across = IVec3::ONE - step - IVec3::Y;
                for side in -1..=1 {
                    for along in 0..=1 {
                        let base = corner + step * along + across * side;
                        edits.push((base - IVec3::Y, BlockState::of(blocks::OBSIDIAN)));
                        for up in 0..3 {
                            edits.push((base + IVec3::Y * up, BlockState::AIR));
                        }
                    }
                }
                (corner, axis)
            }
        };
        edits.extend(portal::build(corner, axis));
        self.edit(&edits);
        self.remember_portal(self.dimension, corner);
        Shape {
            axis,
            corner,
            width: portal::MIN_WIDTH,
            height: portal::MIN_HEIGHT,
        }
    }

    /// The nearest spot to `target` within `BUILD_RADIUS` where a 4 × 5
    /// frame stands on solid ground with nothing in its way, preferring
    /// ones with room on both sides; either way round, `axis` first.
    fn portal_spot(&self, target: IVec3, axis: u8) -> Option<(IVec3, u8)> {
        let world = self.chunks.world();
        let (lo, hi) = height_range(self.dimension);
        let free = |p: IVec3| {
            world
                .block(p.x, p.y, p.z)
                .is_some_and(|s| s.is_air() || s.def().replaceable && s.def().fluid.is_none())
        };
        let ground = |p: IVec3| world.block(p.x, p.y, p.z).is_some_and(|s| s.def().solid);
        let other = if axis == axis::X { axis::Z } else { axis::X };
        for roomy in [true, false] {
            let mut best: Option<(i32, IVec3, u8)> = None;
            for dz in -BUILD_RADIUS..=BUILD_RADIUS {
                for dx in -BUILD_RADIUS..=BUILD_RADIUS {
                    let (x, z) = (target.x + dx, target.z + dz);
                    for y in (lo..=hi).rev() {
                        let at = IVec3::new(x, y, z);
                        if !free(at) || !ground(at - IVec3::Y) {
                            continue;
                        }
                        let distance = (at - target).length_squared();
                        if best.is_some_and(|(d, _, _)| d <= distance) {
                            continue;
                        }
                        for turn in [axis, other] {
                            let step = if turn == axis::Z { IVec3::Z } else { IVec3::X };
                            let across = IVec3::ONE - step - IVec3::Y;
                            let sides: &[i32] = if roomy { &[-1, 0, 1] } else { &[0] };
                            let fits = (-1..=2).all(|along| {
                                let base = at + step * along;
                                ground(base - IVec3::Y)
                                    && sides.iter().all(|&side| {
                                        (0..=3).all(|up| free(base + across * side + IVec3::Y * up))
                                    })
                            });
                            if fits {
                                best = Some((distance, at, turn));
                                break;
                            }
                        }
                    }
                }
            }
            if let Some((_, at, turn)) = best {
                return Some((at, turn));
            }
        }
        None
    }
}

/// The heights a portal's bottom row may be at in `dimension`: under the
/// Nether's roof, or anywhere in the Overworld's height.
fn height_range(dimension: Dimension) -> (i32, i32) {
    match dimension {
        Dimension::Nether => (nether::LAVA_LEVEL + 1, nether::HEIGHT - 10),
        _ => (1, crate::config::MAX_Y - 10),
    }
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use super::*;
    use crate::game::tests::flat_game;

    /// Ticks until the player has arrived and the chunks around them are
    /// drawn, loading chunks as the app would.
    fn settle(game: &mut Game) {
        let deadline = Instant::now() + Duration::from_secs(30);
        loop {
            let feet = game.player.body.position();
            let center = ChunkPos::of_block(feet.x.floor() as i32, feet.z.floor() as i32);
            let changes = game.chunks.update(center);
            game.on_chunks_loaded(&changes.loaded);
            game.tick();
            if !game.is_arriving() && game.chunks.ready_around(center, 1) {
                return;
            }
            assert!(Instant::now() < deadline, "chunks never loaded");
            std::thread::sleep(Duration::from_millis(1));
        }
    }

    /// An obsidian frame with a 2 × 3 inside from (4, 65, 0) along x.
    fn frame(game: &mut Game) {
        let obsidian = BlockState::of(blocks::OBSIDIAN);
        let mut edits = Vec::new();
        for x in 3..=6 {
            for y in 64..=68 {
                let edge = x == 3 || x == 6 || y == 64 || y == 68;
                if edge {
                    edits.push((IVec3::new(x, y, 0), obsidian));
                }
            }
        }
        game.edit(&edits);
    }

    fn block(game: &Game, at: IVec3) -> BlockState {
        game.chunks.world().block(at.x, at.y, at.z).unwrap()
    }

    #[test]
    fn a_lit_frame_takes_you_to_the_nether_and_back_through_the_linked_portal() {
        let mut game = flat_game(GameMode::Creative);
        frame(&mut game);
        assert!(!game.light_portal(IVec3::new(8, 65, 0)), "no frame there");
        assert!(game.light_portal(IVec3::new(5, 66, 0)));
        for (x, y) in [(4, 65), (5, 65), (4, 67), (5, 67)] {
            assert!(block(&game, IVec3::new(x, y, 0)).is(blocks::NETHER_PORTAL));
        }

        // Step in: creative players go at once.
        game.place_player(DVec3::new(5.0, 65.0, 0.5));
        game.tick();
        settle(&mut game);
        assert_eq!(game.dimension(), Dimension::Nether);
        let feet = game.player.body.position();
        let (cell, _) = game.portal_inside().expect("standing in the new portal");
        assert!(cell.x.abs() <= BUILD_RADIUS && cell.z.abs() <= BUILD_RADIUS);
        assert!(feet.y > nether::LAVA_LEVEL as f64 && feet.y < nether::HEIGHT as f64);
        assert_eq!(game.portals.len(), 2);
        // Something under the player to stand on.
        let below = block(&game, feet.floor().as_ivec3() - IVec3::Y);
        assert!(below.is(blocks::OBSIDIAN), "on {}", below.def().name);

        // Standing still doesn't take you back; stepping out and in does.
        for _ in 0..40 {
            game.tick();
        }
        assert_eq!(game.dimension(), Dimension::Nether);
        let back_in = game.player.body.position();
        game.place_player(back_in + DVec3::new(0.0, 0.0, 3.0));
        game.chunks.set_blocks(&[(
            back_in.floor().as_ivec3() + IVec3::new(0, -1, 3),
            BlockState::of(blocks::OBSIDIAN),
        )]);
        for _ in 0..COOLDOWN + 1 {
            game.tick();
        }
        game.place_player(back_in);
        game.tick();
        settle(&mut game);
        assert_eq!(game.dimension(), Dimension::Overworld);
        assert_eq!(
            game.player.body.position(),
            DVec3::new(5.0, 65.0, 0.5),
            "out of the first portal"
        );
        assert_eq!(game.portals.len(), 2, "linked, not a third portal");
    }

    #[test]
    fn breaking_the_frame_puts_the_portal_out() {
        let mut game = flat_game(GameMode::Creative);
        frame(&mut game);
        assert!(game.light_portal(IVec3::new(4, 65, 0)));
        game.edit(&[(IVec3::new(6, 66, 0), BlockState::AIR)]);
        for (x, y) in [(4, 65), (5, 65), (4, 67), (5, 67)] {
            assert!(block(&game, IVec3::new(x, y, 0)).is_air(), "{x},{y}");
        }
    }

    #[test]
    fn survival_players_wait_four_seconds_in_a_portal() {
        let mut game = flat_game(GameMode::Survival);
        frame(&mut game);
        assert!(game.light_portal(IVec3::new(4, 65, 0)));
        game.place_player(DVec3::new(5.0, 65.0, 0.5));
        for _ in 0..WAIT - 1 {
            game.tick();
            game.place_player(DVec3::new(5.0, 65.0, 0.5));
        }
        assert_eq!(game.dimension(), Dimension::Overworld);
        assert!(game.portal_swirl() > 0.9);
        game.tick();
        assert_eq!(game.dimension(), Dimension::Nether);
    }
}
