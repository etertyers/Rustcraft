//! End gateways (M28, modern `TheEndGatewayBlockEntity`): walking into the
//! one the dragon's death opened sends the player out to the outer islands,
//! to a gateway standing on the island nearest 1024 blocks that way; walking
//! into an outer island's gateway sends them back to the main island.

use glam::{DVec3, IVec3};

use super::Game;
use crate::world::{
    blocks,
    chunk::ChunkPos,
    dimension::Dimension,
    worldgen::end::{self, OUTER_START},
};

/// How far out the outer islands are looked for from a gateway's heading.
const OUTWARD: f64 = 1024.0;
/// Ticks before a gateway takes the player again (modern 100 on the
/// entity; this covers the same wait).
const COOLDOWN: u32 = 100;

impl Game {
    /// The gateway block the player's hitbox is in, if any.
    fn gateway_inside(&self) -> Option<IVec3> {
        let bb = self.body.aabb();
        let min = bb.min.floor().as_ivec3();
        let max = (bb.max - DVec3::splat(1e-7)).floor().as_ivec3();
        let world = self.chunks.world();
        for y in min.y..=max.y {
            for z in min.z..=max.z {
                for x in min.x..=max.x {
                    if world
                        .block(x, y, z)
                        .is_some_and(|b| b.is(blocks::END_GATEWAY))
                    {
                        return Some(IVec3::new(x, y, z));
                    }
                }
            }
        }
        None
    }

    /// One tick of standing in a gateway or not.
    pub(super) fn tick_gateway(&mut self) {
        self.gateway_cooldown = self.gateway_cooldown.saturating_sub(1);
        if self.dimension != Dimension::End || self.is_riding() {
            return;
        }
        let Some(cell) = self.gateway_inside() else {
            return;
        };
        if self.gateway_cooldown > 0 {
            // Standing in one keeps it from working (modern).
            self.gateway_cooldown = COOLDOWN;
            return;
        }
        self.take_gateway(cell);
    }

    /// Where the gateway at `cell` leads: the player's feet.
    fn gateway_exit(&mut self, cell: IVec3) -> Option<DVec3> {
        let distance = f64::from(cell.x).hypot(f64::from(cell.z));
        if distance >= OUTER_START {
            // Back to the main island, beside the fountain.
            let podium = end::PODIUM;
            return Some(DVec3::new(
                f64::from(podium.x) + 5.5,
                f64::from(podium.y) + 1.0,
                f64::from(podium.z) + 0.5,
            ));
        }
        let heading = DVec3::new(f64::from(cell.x), 0.0, f64::from(cell.z)).normalize_or_zero();
        let (tx, tz) = (
            (heading.x * OUTWARD).round() as i32,
            (heading.z * OUTWARD).round() as i32,
        );
        let (island, _) = self.chunks.end()?.island_near(tx, tz)?;
        let gateway = IVec3::new(island.x, island.y + 1, island.z);
        // The gateway the player comes out beside, and its chunk, first.
        let tower = end::exit_gateway(gateway);
        for (at, _) in &tower {
            self.chunks.load_now(ChunkPos::of_block(at.x, at.z));
        }
        self.edit(&tower);
        let stand = gateway + IVec3::new(3, 0, 0);
        self.chunks.load_now(ChunkPos::of_block(stand.x, stand.z));
        Some(DVec3::new(
            f64::from(stand.x) + 0.5,
            f64::from(stand.y),
            f64::from(stand.z) + 0.5,
        ))
    }

    fn take_gateway(&mut self, cell: IVec3) {
        let Some(feet) = self.gateway_exit(cell) else {
            return;
        };
        self.place_player(feet);
        self.gateway_cooldown = COOLDOWN;
        // Wait for the chunks around the new spot to load.
        self.arriving = Some(0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{game::end_tests::end_game, player::mode::GameMode, world::block::BlockState};

    #[test]
    fn walking_into_a_gateway_lands_on_an_outer_island_beside_a_gateway_that_leads_home() {
        let mut game = end_game(GameMode::Creative);
        let seed = game.chunks.seed().unwrap();
        let gateway = end::gateway_position(seed, 0);
        game.chunks
            .load_now(ChunkPos::of_block(gateway.x, gateway.z));
        game.set_blocks(&end::gateway(gateway));
        game.body
            .set_position(gateway.as_dvec3() + DVec3::new(0.5, 0.0, 0.5));
        game.tick_gateway();
        let there = game.body.position();
        assert!(there.x.hypot(there.z) > OUTER_START, "{there}");
        assert!(game.is_arriving());
        // Standing on end stone, three blocks from a gateway.
        let feet = there.floor().as_ivec3();
        let world = game.chunks.world();
        assert!(
            world
                .block(feet.x, feet.y - 1, feet.z)
                .unwrap()
                .is(blocks::END_STONE)
        );
        let back = feet - IVec3::new(3, 0, 0);
        assert!(
            world
                .block(back.x, back.y, back.z)
                .unwrap()
                .is(blocks::END_GATEWAY)
        );
        // It heads the way the gateway lies, roughly.
        let along = there.x * f64::from(gateway.x) + there.z * f64::from(gateway.z);
        assert!(along > 0.0);

        // Walking into that one comes back to the main island.
        game.gateway_cooldown = 0;
        game.arriving = None;
        game.body
            .set_position(back.as_dvec3() + DVec3::new(0.5, 0.0, 0.5));
        game.tick_gateway();
        let home = game.body.position();
        assert!(home.x.hypot(home.z) < 10.0, "{home}");
    }

    #[test]
    fn a_gateway_waits_a_while_before_taking_you_again_and_standing_in_it_keeps_it_waiting() {
        let mut game = end_game(GameMode::Creative);
        let seed = game.chunks.seed().unwrap();
        let gateway = end::gateway_position(seed, 1);
        game.chunks
            .load_now(ChunkPos::of_block(gateway.x, gateway.z));
        game.set_blocks(&end::gateway(gateway));
        game.set_blocks(&[(gateway, BlockState::of(blocks::END_GATEWAY))]);
        game.gateway_cooldown = 50;
        game.body
            .set_position(gateway.as_dvec3() + DVec3::new(0.5, 0.0, 0.5));
        game.tick_gateway();
        assert_eq!(
            game.body.position().x.floor() as i32,
            gateway.x,
            "still here"
        );
        assert_eq!(
            game.gateway_cooldown, COOLDOWN,
            "standing in it resets the wait"
        );
    }
}
