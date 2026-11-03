//! Fishing (M41): casting a rod, the bobber floating and biting, reeling
//! in loot or a mob or player, on the server for every player.

use glam::{DVec3, IVec3};

use super::Game;
use crate::{
    entity::{
        bobber::{self, Bobber, Hooked, Kind, Sign, Surroundings},
        item::ItemEntity,
        particle::Particle,
    },
    inventory::{items, stack::ItemStack},
    world::{World, block::BlockState, blocks},
};

/// Whether `at` is within a little of the box.
fn near(b: &crate::player::collision::Aabb, at: DVec3) -> bool {
    let m = 0.15;
    at.x >= b.min.x - m
        && at.x <= b.max.x + m
        && at.y >= b.min.y - m
        && at.y <= b.max.y + m
        && at.z >= b.min.z - m
        && at.z <= b.max.z + m
}

/// The world as the bobber sees it.
struct Pond<'a>(&'a World);

impl Surroundings for Pond<'_> {
    fn is_water(&self, c: IVec3) -> bool {
        self.0
            .block(c.x, c.y, c.z)
            .is_some_and(|b| b.is(blocks::WATER))
    }
    fn is_solid(&self, c: IVec3) -> bool {
        self.0
            .block(c.x, c.y, c.z)
            .is_none_or(|b| b.def().solid && !b.is(blocks::WATER))
    }
}

impl Game {
    /// Where the line leaves `self.player`'s rod: out in front of the hand.
    fn rod_tip(&self) -> DVec3 {
        let p = &self.player;
        let yaw = f64::from(p.camera.yaw);
        let forward = DVec3::new(yaw.sin(), 0.0, -yaw.cos());
        let right = DVec3::new(yaw.cos(), 0.0, yaw.sin());
        p.body.position() + DVec3::Y * (p.body.eye_height() - 0.35) + forward * 0.55 + right * 0.35
    }

    /// A scene for screenshots: a rod in hand, cast, and a few ticks on.
    pub fn stage_cast(&mut self) {
        let rod = ItemStack::new(items::FISHING_ROD, 1);
        self.player
            .inventory
            .set(self.player.inventory.selected, Some(rod));
        self.use_fishing_rod();
        self.run_ticks(40);
    }

    /// Right-click with a fishing rod in the main hand: cast, or reel in.
    /// Returns whether the item was a rod.
    pub(super) fn use_fishing_rod(&mut self) -> bool {
        if self.held().is_none_or(|s| s.id != items::FISHING_ROD) {
            return false;
        }
        let me = self.turn;
        if let Some(i) = self.bobbers.iter().position(|b| b.owner == me) {
            self.reel_in(i);
        } else {
            let eye = self.player.body.position() + DVec3::Y * self.player.body.eye_height();
            let look = self.player.camera.forward().as_dvec3();
            let lure = self.held_level(crate::inventory::enchant::Enchantment::Lure);
            let luck = self.held_level(crate::inventory::enchant::Enchantment::LuckOfTheSea);
            let mut bobber = Bobber::cast(me, eye, look, lure, luck, &mut self.rng);
            bobber.tip = self.rod_tip();
            crate::audio::play("entity.fishing_bobber.throw", eye);
            self.bobbers.push(bobber);
        }
        self.player.view.swing();
        true
    }

    /// Reels bobber `i` in: loot if a fish is biting, the hooked mob or
    /// player pulled over, or nothing.
    fn reel_in(&mut self, i: usize) {
        let bobber = self.bobbers.remove(i);
        let owner = self.player.body.position() + DVec3::Y;
        crate::audio::play("entity.fishing_bobber.retrieve", bobber.pos);
        let mut wear = 0;
        match bobber.hooked {
            Hooked::Mob(id) => {
                if let Some(mob) = self.mobs.iter_mut().find(|m| m.id == id) {
                    let pull = (owner - mob.feet()) * 0.1;
                    mob.body.velocity += pull + DVec3::Y * 0.1;
                    wear = bobber::WEAR_ENTITY;
                }
            }
            Hooked::Player(other) => {
                if other < self.player_count() && other != self.turn {
                    let from = self.feet_of(other);
                    let pull = (owner - from) * 0.1;
                    self.pull_player(other, pull + DVec3::Y * 0.1);
                    wear = bobber::WEAR_ENTITY;
                }
            }
            Hooked::Nothing if bobber.nibble > 0 => {
                self.catch_fish(&bobber, owner);
                wear = bobber::WEAR_CATCH;
            }
            Hooked::Nothing => {}
        }
        if wear > 0 && self.player.mode.wears_tools() {
            self.wear_selected(wear);
        }
    }

    /// Adds `by` to another player's velocity (the one whose turn it is
    /// keeps theirs in `self.player`).
    fn pull_player(&mut self, index: usize, by: DVec3) {
        let slot = if index == self.turn {
            &mut self.player
        } else if index == 0 {
            &mut self.others[self.turn - 1]
        } else {
            &mut self.others[index - 1]
        };
        slot.body.velocity += by;
    }

    /// The loot of a bite: an item flying to the player, a little experience.
    fn catch_fish(&mut self, bobber: &Bobber, owner: DVec3) {
        let (kind, stack) = bobber::roll_catch(bobber.luck, bobber.open_water, &mut self.rng);
        let Some(mut stack) = stack else { return };
        // Fishing rods and boots from the junk come worn.
        if kind == Kind::Junk
            && (stack.id == items::FISHING_ROD || stack.id == items::LEATHER_BOOTS)
        {
            let max = self
                .items
                .item(stack.id)
                .and_then(|d| d.durability)
                .unwrap_or(0);
            stack = ItemStack::with_damage(
                stack.id,
                1,
                (f64::from(max) * 0.9 * self.rng.next_f64()) as u16,
            );
        }
        let to_player = owner - bobber.pos;
        let velocity = to_player * 0.1 + DVec3::Y * to_player.length().sqrt() * 0.08;
        self.entities
            .push(ItemEntity::new(stack, bobber.pos, velocity, 0.0));
        let points = 1 + self.rng.below(6);
        self.spawn_xp(points, owner);
        if kind == Kind::Fish {
            self.note(crate::advancements::Event::Used("fish_caught".to_owned()));
        }
    }

    /// One tick of every bobber: flight, floating, hooking, fishing.
    pub(super) fn tick_bobbers(&mut self) {
        if self.bobbers.is_empty() {
            return;
        }
        let mut i = 0;
        while i < self.bobbers.len() {
            let owner = self.bobbers[i].owner;
            let alive = owner < self.player_count() && {
                let p = self.player_at(owner);
                p.dimension == self.dimension
                    && !p.stats.is_dead()
                    && p.inventory
                        .selected_stack()
                        .is_some_and(|s| s.id == items::FISHING_ROD)
                    && self.feet_of(owner).distance(self.bobbers[i].pos) <= bobber::MAX_RANGE
            };
            if !alive {
                self.bobbers.remove(i);
                continue;
            }
            self.tick_bobber(i);
            i += 1;
        }
    }

    fn tick_bobber(&mut self, i: usize) {
        let owner = self.bobbers[i].owner;
        let feet = self.feet_of(owner);
        let yaw = f64::from(self.player_at(owner).camera.yaw);
        let eye = self.player_at(owner).body.eye_height();
        let (forward, right) = (
            DVec3::new(yaw.sin(), 0.0, -yaw.cos()),
            DVec3::new(yaw.cos(), 0.0, yaw.sin()),
        );
        self.bobbers[i].tip = feet + DVec3::Y * (eye - 0.35) + forward * 0.55 + right * 0.35;
        // Stuck to something: follows it, or is gone with it.
        match self.bobbers[i].hooked {
            Hooked::Mob(id) => match self.mobs.iter().find(|m| m.id == id && !m.is_dead()) {
                Some(mob) => {
                    let at = mob.feet()
                        + DVec3::Y * (mob.body.aabb().max.y - mob.body.aabb().min.y) * 0.6;
                    self.bobbers[i].prev = self.bobbers[i].pos;
                    self.bobbers[i].pos = at;
                }
                None => self.bobbers[i].hooked = Hooked::Nothing,
            },
            Hooked::Player(other) => {
                let at = self.feet_of(other) + DVec3::Y;
                self.bobbers[i].prev = self.bobbers[i].pos;
                self.bobbers[i].pos = at;
            }
            Hooked::Nothing => {
                let pond = Pond(self.chunks.world());
                self.bobbers[i].step(&pond);
                // A mob or another player in the way gets hooked.
                if !self.bobbers[i].in_water && self.bobbers[i].age > 2 {
                    let at = self.bobbers[i].pos;
                    let hit = self
                        .mobs
                        .iter()
                        .find(|m| !m.is_dead() && near(&m.body.aabb(), at))
                        .map(|m| Hooked::Mob(m.id));
                    let hit = hit.or_else(|| {
                        (0..self.player_count())
                            .filter(|&p| {
                                p != owner && self.player_at(p).dimension == self.dimension
                            })
                            .find(|&p| near(&self.player_at(p).body.aabb(), at))
                            .map(Hooked::Player)
                    });
                    if let Some(hit) = hit {
                        self.bobbers[i].hooked = hit;
                    }
                }
            }
        }
        // Fishing, once it floats.
        if self.bobbers[i].age.is_multiple_of(20) {
            let world = self.chunks.world();
            let cell = self.bobbers[i].cell();
            let open = bobber::open_water(&|p| world.block(p.x, p.y, p.z), cell);
            self.bobbers[i].open_water = open;
        }
        let rain = false;
        let sign = self.bobbers[i].fish(&mut self.rng, rain);
        let at = self.bobbers[i].pos;
        match sign {
            Some(Sign::Bite) => {
                crate::audio::play("entity.fishing_bobber.splash", at);
                self.water_drops(at, 12);
            }
            Some(Sign::Approach(p)) => self.water_drops(p, 2),
            _ => {}
        }
    }

    /// A few drops of water at `at`.
    fn water_drops(&mut self, at: DVec3, count: usize) {
        let rng = &mut self.rng;
        let mut random = || rng.next_f64() as f32;
        let mut burst = Particle::burst(
            at.floor().as_ivec3(),
            BlockState::of(blocks::WATER),
            &mut random,
        );
        burst.truncate(count);
        self.add_particles(burst);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{game::tests::flat_game, inventory::enchant::Enchantment, player::mode::GameMode};

    /// A flat world with a pond (water at Y 63 over X 0..9, Z 2..9) and
    /// a rod in hand, looking at the water.
    fn angler() -> Game {
        let mut game = flat_game(GameMode::Survival);
        let mut edits = Vec::new();
        for x in 0..10 {
            for z in 2..10 {
                edits.push((IVec3::new(x, 63, z), BlockState::of(blocks::WATER)));
            }
        }
        game.edit(&edits);
        let mut rod = ItemStack::new(items::FISHING_ROD, 1);
        rod.enchant(Enchantment::Lure, 3);
        game.player
            .inventory
            .set(game.player.inventory.selected, Some(rod));
        game.player.camera.pitch = -0.25;
        game.player.camera.yaw = std::f32::consts::PI; // looking south, toward +Z
        game
    }

    #[test]
    fn casting_floats_a_bobber_and_a_bite_reels_in_a_fish_with_experience_and_wear() {
        let mut game = angler();
        assert!(game.use_fishing_rod());
        assert_eq!(game.bobbers.len(), 1);
        for _ in 0..40 {
            game.tick();
        }
        assert!(
            game.bobbers[0].in_water,
            "landed in the pond at {:?}",
            game.bobbers[0].pos
        );
        // Wait for a bite (Lure III makes it quick), then reel in.
        let mut ticks = 0;
        while game.bobbers[0].nibble == 0 {
            game.tick();
            ticks += 1;
            assert!(ticks < 2000, "never bit");
        }
        let before = game.entities.len();
        assert!(game.use_fishing_rod());
        assert!(game.bobbers.is_empty());
        assert_eq!(
            game.entities.len(),
            before + 1,
            "a catch flies to the player"
        );
        let rod = game.player.inventory.selected_stack().unwrap();
        assert_eq!(rod.damage, 1, "one point of wear for a catch");
    }

    #[test]
    fn reeling_in_early_catches_nothing_and_the_bobber_comes_back_when_the_rod_is_put_away() {
        let mut game = angler();
        game.use_fishing_rod();
        for _ in 0..30 {
            game.tick();
        }
        let before = game.entities.len();
        assert!(game.bobbers[0].nibble == 0);
        game.use_fishing_rod();
        assert!(game.bobbers.is_empty());
        assert_eq!(game.entities.len(), before, "no fish yet");
        // Switching away from the rod takes the bobber back.
        game.use_fishing_rod();
        game.player
            .inventory
            .set(game.player.inventory.selected, None);
        game.tick();
        assert!(game.bobbers.is_empty());
    }

    #[test]
    fn the_bobber_hooks_a_mob_and_reeling_pulls_it_toward_the_player() {
        let mut game = angler().with_mobs(crate::game::tests::mob_registry());
        game.edit(&[
            (IVec3::new(0, 63, 5), BlockState::of(blocks::GRASS_BLOCK)),
            (IVec3::new(0, 63, 6), BlockState::of(blocks::GRASS_BLOCK)),
        ]);
        let cow = game.summon("cow", DVec3::new(0.5, 64.0, 5.5), 0.0).unwrap();
        cow.body.velocity = DVec3::ZERO;
        let cow_id = cow.id;
        game.player.camera.pitch = 0.0;
        game.player.camera.yaw = std::f32::consts::PI;
        game.use_fishing_rod();
        for _ in 0..60 {
            game.tick();
            if game
                .bobbers
                .first()
                .is_some_and(|b| b.hooked != Hooked::Nothing)
            {
                break;
            }
        }
        assert_eq!(
            game.bobbers[0].hooked,
            Hooked::Mob(cow_id),
            "{:?}",
            game.bobbers[0]
        );
        let z_before = game.mobs.iter().find(|m| m.id == cow_id).unwrap().feet().z;
        game.use_fishing_rod();
        game.tick();
        let after = game.mobs.iter().find(|m| m.id == cow_id).unwrap();
        assert!(
            after.body.velocity.z < 0.0 || after.feet().z < z_before,
            "pulled toward the player"
        );
        assert_eq!(
            game.player.inventory.selected_stack().unwrap().damage,
            5,
            "an entity costs more"
        );
    }
}
