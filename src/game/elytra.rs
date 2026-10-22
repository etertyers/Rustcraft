//! Elytra and firework rockets (M28, modern `LivingEntity.updateFallFlying`
//! and `FireworkRocketEntity`): jumping in the air while falling with
//! wings on the chest starts a glide; the wings wear a point a second; a
//! rocket in the hand pushes the glider along where they look.

use glam::DVec3;

use super::Game;
use crate::{
    inventory::{items, player::ARMOR_CHEST},
    player::stats::DamageSource,
};

/// Ticks a rocket pushes: modern `10 × (flight + 1)` plus a random few
/// (flight 1 here: there are no firework stars to craft).
const ROCKET_TICKS: u32 = 20;
/// Ticks between the points of wear an elytra takes while gliding.
const WEAR_EVERY: u32 = 20;

impl Game {
    /// Whether wings are on the chest and not worn out (modern
    /// `Elytra.isFlyEnabled`: one point of wear short of breaking).
    pub fn elytra_ready(&self) -> bool {
        let Some(stack) = self.inventory.slot(ARMOR_CHEST) else {
            return false;
        };
        stack.id == items::ELYTRA
            && self
                .items
                .max_damage(stack.id)
                .is_some_and(|max| stack.damage + 1 < max)
    }

    /// Whether the player is gliding.
    pub fn is_gliding(&self) -> bool {
        self.body.fall_flying
    }

    /// Ticks spent gliding, for the model's pose.
    pub fn glide_ticks(&self) -> u32 {
        self.glide_ticks
    }

    /// One tick before the body moves: a fresh jump press in the air with
    /// wings on starts a glide; gliding wears the wings and ends when they
    /// give out.
    pub(super) fn tick_elytra(&mut self) {
        let jump = self.keys.jump;
        let pressed = jump && !self.jump_was;
        self.jump_was = jump;
        if !self.body.fall_flying {
            self.glide_ticks = 0;
            self.rocket = 0;
            if pressed
                && !self.body.on_ground
                && !self.body.flying
                && self.body.liquid.is_none()
                && self.body.velocity.y < 0.0
                && !self.is_riding()
                && self.elytra_ready()
            {
                self.body.fall_flying = true;
            }
            return;
        }
        if !self.elytra_ready() {
            self.body.fall_flying = false;
            return;
        }
        self.glide_ticks += 1;
        if self.mode.wears_tools() && self.glide_ticks.is_multiple_of(WEAR_EVERY) {
            self.inventory.damage_slot(ARMOR_CHEST, 1, &self.items);
        }
        if self.rocket > 0 {
            self.rocket -= 1;
            let look = self.camera.forward().as_dvec3();
            let v = self.body.velocity;
            self.body.velocity += look * 0.1 + (look * 1.5 - v) * 0.5;
        }
        // A glide that isn't diving fast isn't a fall (modern keeps the
        // fall distance at a block until it drops faster than half a
        // block a tick).
        if self.body.velocity.y > -0.5 {
            self.stats.fall_distance = self.stats.fall_distance.min(1.0);
        }
    }

    /// What the glide's last tick did to a wall it met.
    pub(super) fn glide_impact(&mut self) {
        let impact = self.body.glide_impact;
        if impact > 0.0 && self.mode.is_vulnerable() {
            self.stats.damage(impact as f32, DamageSource::FlyIntoWall);
        }
    }

    /// Right-click with a firework rocket while gliding: it boosts, and is
    /// used up. Returns whether it did.
    pub(super) fn use_rocket(&mut self) -> bool {
        if !self.body.fall_flying {
            return false;
        }
        let jitter = self.rng.below(6) + self.rng.below(7);
        self.rocket = ROCKET_TICKS + jitter;
        if self.mode.uses_up_items() {
            self.inventory.take(self.held_slot(), 1);
        }
        self.view.swing();
        true
    }

    /// How fast the player is going, for the debug overlay and tests.
    pub fn speed(&self) -> f64 {
        DVec3::length(self.body.velocity)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        game::tests::flat_game,
        inventory::stack::ItemStack,
        player::mode::GameMode,
        world::{block::BlockState, blocks},
    };
    use glam::IVec3;

    /// A player high over a flat world with elytra on, falling.
    fn airborne(mode: GameMode) -> Game {
        let mut game = flat_game(mode);
        game.inventory
            .set(ARMOR_CHEST, Some(ItemStack::new(items::ELYTRA, 1)));
        game.body.set_position(DVec3::new(0.5, 150.0, 0.5));
        game.prev_feet = game.body.position();
        game.camera.pitch = 0.0;
        game.run_ticks(3);
        game
    }

    fn press_jump(game: &mut Game) {
        game.keys.jump = false;
        game.run_ticks(1);
        game.keys.jump = true;
        game.run_ticks(1);
        game.keys.jump = false;
    }

    #[test]
    fn jumping_in_the_air_with_wings_starts_a_glide_and_without_them_doesnt() {
        let mut game = airborne(GameMode::Survival);
        assert!(!game.is_gliding());
        press_jump(&mut game);
        assert!(game.is_gliding());
        let mut bare = flat_game(GameMode::Survival);
        bare.body.set_position(DVec3::new(0.5, 150.0, 0.5));
        bare.run_ticks(3);
        press_jump(&mut bare);
        assert!(!bare.is_gliding());
    }

    #[test]
    fn a_level_glide_sinks_slowly_and_diving_trades_height_for_speed() {
        let mut level = airborne(GameMode::Creative);
        level.mode = GameMode::Survival;
        press_jump(&mut level);
        let y0 = level.body.position().y;
        level.run_ticks(60);
        let sink = y0 - level.body.position().y;
        assert!(level.is_gliding());
        assert!(sink > 5.0 && sink < 60.0, "sank {sink}");

        let mut dive = airborne(GameMode::Survival);
        press_jump(&mut dive);
        dive.camera.pitch = -1.0;
        dive.run_ticks(40);
        assert!(dive.speed() > 1.0, "dove at {}", dive.speed());
        // Pulling up climbs back, spending the speed.
        let low = dive.body.position().y;
        dive.camera.pitch = 0.9;
        dive.run_ticks(30);
        assert!(dive.body.position().y > low - 1.0);
    }

    #[test]
    fn gliding_wears_the_wings_a_point_a_second_and_a_worn_out_pair_drops_you() {
        let mut game = airborne(GameMode::Survival);
        press_jump(&mut game);
        game.run_ticks(60);
        assert_eq!(game.inventory.slot(ARMOR_CHEST).unwrap().damage, 3);
        // One point from breaking is as good as broken.
        game.inventory.set(
            ARMOR_CHEST,
            Some(ItemStack::with_damage(items::ELYTRA, 1, 431)),
        );
        assert!(!game.elytra_ready());
        game.run_ticks(2);
        assert!(!game.is_gliding());
    }

    #[test]
    fn creative_gliders_wear_nothing() {
        let mut game = airborne(GameMode::Creative);
        press_jump(&mut game);
        game.run_ticks(45);
        assert_eq!(game.inventory.slot(ARMOR_CHEST).unwrap().damage, 0);
    }

    #[test]
    fn a_rocket_pushes_along_the_look_and_is_used_up_and_does_nothing_on_the_ground() {
        let mut game = airborne(GameMode::Survival);
        game.inventory
            .set(0, Some(ItemStack::new(items::FIREWORK_ROCKET, 3)));
        game.inventory.selected = 0;
        assert!(!game.use_rocket(), "not gliding");
        press_jump(&mut game);
        game.camera.pitch = 0.3;
        game.run_ticks(5);
        let slow = game.speed();
        assert!(game.use_rocket());
        assert_eq!(game.inventory.slot(0).unwrap().count, 2);
        game.run_ticks(15);
        assert!(game.speed() > slow + 0.5, "{} vs {slow}", game.speed());
        // Heading where it looks: up and north.
        assert!(game.body.velocity.y > 0.0 && game.body.velocity.z < 0.0);
    }

    #[test]
    fn hitting_a_wall_at_speed_hurts_and_landing_ends_the_glide() {
        let mut game = airborne(GameMode::Survival);
        let wall: Vec<_> = (100..200)
            .flat_map(|y| {
                (-8..=8).map(move |x| (IVec3::new(x, y, -12), BlockState::of(blocks::STONE)))
            })
            .collect();
        game.set_blocks(&wall);
        press_jump(&mut game);
        game.camera.pitch = -0.6;
        game.run_ticks(10);
        game.camera.pitch = 0.0;
        let health = game.stats.health;
        game.run_ticks(40);
        assert!(
            game.stats.health < health || game.stats.is_dead(),
            "kinetic energy"
        );
        assert_eq!(game.stats.last_source, Some(DamageSource::FlyIntoWall));
        let mut landing = airborne(GameMode::Creative);
        landing.body.set_position(DVec3::new(0.5, 75.0, 0.5));
        landing.run_ticks(2);
        press_jump(&mut landing);
        landing.camera.pitch = -1.4;
        landing.run_ticks(200);
        assert!(!landing.is_gliding());
        assert!(landing.body.on_ground);
    }
}
