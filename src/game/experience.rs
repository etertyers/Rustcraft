//! Experience: orbs dropping and being picked up, and where they come
//! from (ore, kills, smelting, breeding, dying).

use glam::DVec3;

use super::Game;
use crate::{
    entity::{
        mob::Mob,
        orb::{self, XpOrb},
    },
    inventory::{enchant::Enchantment, player::OFFHAND},
    player::{collision::WorldCollider, experience},
    world::block::BlockState,
};

/// Durability one point of experience repairs through Mending.
const MENDING_PER_POINT: u16 = 2;

/// Points a mined ore drops, as (least, most), or none.
fn ore_points(block: BlockState) -> Option<(u32, u32)> {
    let name = block.def().name.as_str();
    let name = name.strip_prefix("deepslate_").unwrap_or(name);
    match name {
        "coal_ore" => Some((0, 2)),
        "diamond_ore" | "emerald_ore" => Some((3, 7)),
        "lapis_ore" | "nether_quartz_ore" => Some((2, 5)),
        "redstone_ore" => Some((1, 5)),
        "nether_gold_ore" => Some((0, 1)),
        _ => None,
    }
}

impl Game {
    /// Drops `points` of experience at `at` as orbs.
    pub fn spawn_xp(&mut self, points: u32, at: DVec3) {
        for value in experience::split_into_orbs(points) {
            let rng = &mut self.rng;
            let mut random = || rng.next_f64();
            let velocity = DVec3::new(random() * 0.2 - 0.1, 0.2, random() * 0.2 - 0.1);
            let phase = (random() * std::f64::consts::TAU) as f32;
            orb::spawn(&mut self.orbs, XpOrb::new(value, at, velocity, phase));
        }
    }

    /// The experience a broken block drops (ore only).
    pub(super) fn ore_xp(&mut self, block: BlockState) -> u32 {
        let Some((least, most)) = ore_points(block) else {
            return 0;
        };
        least + self.rng.below(most - least + 1)
    }

    /// What a mob drops on dying: nothing unless the player killed it.
    pub(super) fn mob_xp(&mut self, mob: &Mob) -> u32 {
        if mob.player_hit == 0 {
            return 0;
        }
        let def = self.mob_defs.get(mob.kind);
        let bonus = |game: &mut Game| 1 + game.rng.below(3);
        match def.name.as_str() {
            "villager" | "iron_golem" => 0,
            "slime" => u32::from(mob.data.size),
            _ if def.ai.is_monster() => 5,
            _ => bonus(self),
        }
    }

    /// Moves orbs and lets the player pick them up.
    pub(super) fn tick_orbs(&mut self) {
        let collider = WorldCollider {
            world: self.chunks.world(),
            blocks: self.items.blocks,
        };
        // Each orb drifts to the nearest living player.
        let centers: Vec<DVec3> = (0..self.player_count())
            .map(|i| self.player_at(i))
            .filter(|p| !p.stats.is_dead())
            .map(|p| p.body.position() + DVec3::Y * (p.body.eye_height() * 0.5))
            .collect();
        for orb in &mut self.orbs {
            let at = orb.center();
            let pull = centers
                .iter()
                .copied()
                .min_by(|a, b| a.distance(at).total_cmp(&b.distance(at)));
            orb.tick(&collider, pull);
        }
        self.for_each_player(Game::collect_orbs);
        self.orbs.retain(|orb| !orb.expired());
    }

    /// `self.player` takes the orbs they touch.
    fn collect_orbs(&mut self) {
        if self.player.stats.is_dead() {
            return;
        }
        let body = self.player.body.aabb();
        let mut collected = 0;
        for orb in &mut self.orbs {
            if orb.touches(&body) {
                collected += orb.value;
                orb.value = 0;
            }
        }
        if collected > 0 {
            let pitch = 0.9 + self.rng.next_f64() as f32 * 0.6;
            crate::audio::play_here_with("entity.experience_orb.pickup", 0.1, pitch);
            self.collect_xp(collected);
        }
    }

    /// Gives the player `points`, first repairing a Mending item held in
    /// either hand: two durability a point, the rest into the bar.
    pub fn collect_xp(&mut self, points: u32) {
        let mut left = points;
        for slot in [self.player.inventory.selected, OFFHAND] {
            let Some(stack) = self.player.inventory.slot(slot) else {
                continue;
            };
            if stack.level_of(Enchantment::Mending) == 0 || stack.damage == 0 {
                continue;
            }
            let repair = (left * u32::from(MENDING_PER_POINT)).min(u32::from(stack.damage)) as u16;
            let mut repaired = stack.clone();
            repaired.damage -= repair;
            self.player.inventory.set(slot, Some(repaired));
            left -= u32::from(repair).div_ceil(u32::from(MENDING_PER_POINT));
            if left == 0 {
                break;
            }
        }
        self.player.stats.experience.add(left);
    }

    /// Dying drops seven points a level, at most a hundred, and empties
    /// the bar.
    pub(super) fn drop_experience(&mut self) {
        let points = self.player.stats.experience.death_drop();
        self.player.stats.experience.die();
        let at = self.player.body.position() + DVec3::Y * (self.player.body.eye_height() * 0.5);
        self.spawn_xp(points, at);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        game::tests::flat_game,
        inventory::{items, stack::ItemStack},
        player::mode::GameMode,
        world::blocks,
    };

    fn survival() -> Game {
        flat_game(GameMode::Survival)
    }

    #[test]
    fn spawned_experience_is_picked_up_into_the_bar() {
        let mut game = survival();
        let feet = game.player.body.position();
        game.spawn_xp(17, feet + DVec3::new(0.0, 0.3, 0.0));
        assert!(!game.orbs.is_empty());
        for _ in 0..40 {
            game.tick_orbs();
        }
        assert!(game.orbs.is_empty(), "all collected");
        assert_eq!(game.player.stats.experience.points(), 17);
        assert_eq!(
            game.player.stats.experience.level, 2,
            "7 + 9 points reach level 2"
        );
    }

    #[test]
    fn orbs_near_the_player_drift_in_from_a_few_blocks() {
        let mut game = survival();
        let feet = game.player.body.position();
        game.spawn_xp(3, feet + DVec3::new(4.0, 0.3, 0.0));
        for _ in 0..120 {
            game.tick_orbs();
        }
        assert_eq!(game.player.stats.experience.points(), 3);
    }

    #[test]
    fn mending_spends_experience_on_repairs_first() {
        let mut game = survival();
        let mut pickaxe = ItemStack::with_damage(items::DIAMOND_PICKAXE, 1, 30);
        pickaxe.enchant(Enchantment::Mending, 1);
        let slot = game.player.inventory.selected;
        game.player.inventory.set(slot, Some(pickaxe));
        game.collect_xp(10);
        // 10 points repair 20 durability, leaving 10 damage and no xp.
        assert_eq!(game.player.inventory.selected_stack().unwrap().damage, 10);
        assert_eq!(game.player.stats.experience.points(), 0);
        game.collect_xp(10);
        // 5 points finish the repair; 5 are left for the bar.
        assert_eq!(game.player.inventory.selected_stack().unwrap().damage, 0);
        assert_eq!(game.player.stats.experience.points(), 5);
    }

    #[test]
    fn dying_drops_orbs_worth_seven_a_level_and_resets_the_bar() {
        let mut game = survival();
        game.player
            .stats
            .experience
            .add(experience::points_to_reach(5));
        game.drop_experience();
        let dropped: u32 = game.orbs.iter().map(|orb| orb.value).sum();
        assert_eq!(dropped, 35);
        assert_eq!(game.player.stats.experience.level, 0);
        // Doing it again drops nothing more.
        game.drop_experience();
        assert_eq!(game.orbs.iter().map(|orb| orb.value).sum::<u32>(), 35);
    }

    #[test]
    fn only_ores_drop_experience_in_their_ranges() {
        let mut game = survival();
        let diamond = BlockState::of(blocks::DIAMOND_ORE);
        let stone = BlockState::of(blocks::STONE);
        for _ in 0..200 {
            let xp = game.ore_xp(diamond);
            assert!((3..=7).contains(&xp));
            assert_eq!(game.ore_xp(stone), 0);
        }
    }

    #[test]
    fn taking_smelted_items_pays_the_stored_experience_once() {
        use crate::{
            game::containers::Container,
            inventory::{
                furnace,
                slots::{Click, SlotRef},
            },
            world::block_entity::BlockEntity,
        };
        let mut game = survival();
        let at = glam::IVec3::new(2, 64, 0);
        let furnace_block = BlockState::of(blocks::FURNACE);
        game.chunks.set_blocks(&[(at, furnace_block)]);
        game.open_furnace(at);
        let Some(Container::Furnace(_)) = game.player.container else {
            panic!("the furnace screen is open");
        };
        let Some(BlockEntity::Furnace(f)) = game.chunks.block_entity_mut(at) else {
            panic!("a furnace block entity");
        };
        f.slots[furnace::OUTPUT] = Some(ItemStack::new(items::IRON_INGOT, 4));
        f.xp = 280; // four irons at 0.7
        game.click_slot(SlotRef::Block(furnace::OUTPUT), Click::Shift);
        let dropped: u32 = game.orbs.iter().map(|orb| orb.value).sum();
        assert!(
            (2..=3).contains(&dropped),
            "2.8 points pay 2 or 3: {dropped}"
        );
        // Nothing is left to pay out a second time.
        game.click_slot(SlotRef::Block(furnace::OUTPUT), Click::Shift);
        assert_eq!(game.orbs.iter().map(|orb| orb.value).sum::<u32>(), dropped);
    }
}

#[cfg(test)]
mod kill_tests {
    use glam::DVec3;

    use crate::{
        entity::mob::Mob,
        game::tests::{flat_game, mob_registry},
        player::mode::GameMode,
    };

    /// Kills a mob called `name` the way the player would (`by_player`) or
    /// otherwise, and returns the experience that dropped.
    fn killed(name: &str, by_player: bool) -> u32 {
        let mut game = flat_game(GameMode::Survival);
        game.mob_defs = mob_registry();
        let kind = game.mob_defs.find(name).unwrap();
        let mut mob = Mob::new(
            kind,
            game.mob_defs.get(kind),
            DVec3::new(6.5, 64.0, 6.5),
            0.0,
        );
        if by_player {
            mob.hit_by_player();
        }
        mob.stats.health = 0.0;
        game.mobs.push(mob);
        game.tick();
        game.orbs.iter().map(|orb| orb.value).sum()
    }

    #[test]
    fn monsters_killed_by_the_player_drop_five_and_animals_one_to_three() {
        assert_eq!(killed("zombie", true), 5);
        assert_eq!(killed("skeleton", true), 5);
        assert!((1..=3).contains(&killed("cow", true)));
        assert!((1..=3).contains(&killed("pig", true)));
    }

    #[test]
    fn nothing_drops_unless_the_player_made_the_kill() {
        assert_eq!(killed("zombie", false), 0);
        assert_eq!(killed("cow", false), 0);
        // Villagers and golems never give experience.
        assert_eq!(killed("villager", true), 0);
    }
}
