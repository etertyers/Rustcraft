//! Modern (1.9+) combat, after `Player.attack`: swings recharge over a
//! time set by the item's attack speed and hit for less when they haven't;
//! a strong swing can be critical while falling, knock back more when
//! sprinting, and sweep from a sword. Constants are in `config::combat`.

use glam::DVec3;

use super::Game;
use crate::{
    config::combat as c,
    entity::{effects::Effect, mob::push},
    inventory::item::{Tool, ToolKind},
    inventory::{items, player::Hand},
    player::stats::DamageSource,
};

/// How charged a swing is after `ticks` since the last one with an item of
/// `speed` swings a second, `partial` of the way into the next tick
/// (modern `getAttackStrengthScale`).
pub fn charge(ticks: u32, speed: f32, partial: f32) -> f32 {
    let delay = 20.0 / speed;
    ((ticks as f32 + partial + 0.5) / delay).clamp(0.0, 1.0)
}

/// What a swing's damage is multiplied by at `charge`.
pub fn charge_scale(charge: f32) -> f32 {
    c::MIN_CHARGE_DAMAGE + charge * charge * (1.0 - c::MIN_CHARGE_DAMAGE)
}

/// Whether a swing is charged enough to count as strong.
pub fn is_strong(charge: f32) -> bool {
    charge > c::STRONG_CHARGE
}

/// What the attacker is doing, for crits and sweeps.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Stance {
    pub charge: f32,
    pub sprinting: bool,
    /// Has fallen since last standing on something.
    pub falling: bool,
    pub on_ground: bool,
    pub in_water: bool,
    pub in_lava: bool,
    pub on_climbable: bool,
    /// Horizontal distance walked this tick, in vanilla's walk units, and
    /// the walking speed it must stay under for a sweep.
    pub walked: f32,
    pub speed: f32,
}

impl Stance {
    /// A critical hit: a strong swing while falling, not on the ground,
    /// in a liquid, on a ladder or vine, or sprinting.
    pub fn is_critical(&self) -> bool {
        is_strong(self.charge)
            && self.falling
            && !self.on_ground
            && !self.on_climbable
            && !self.in_water
            && !self.in_lava
            && !self.sprinting
    }

    /// Sprinting into a strong swing knocks the target back further.
    pub fn sprint_knockback(&self) -> bool {
        is_strong(self.charge) && self.sprinting
    }

    /// A sword sweeps on a strong, uncritical swing from the ground while
    /// standing nearly still.
    pub fn sweeps(&self, sword: bool) -> bool {
        sword
            && is_strong(self.charge)
            && !self.is_critical()
            && !self.sprint_knockback()
            && self.on_ground
            && self.walked < self.speed
    }
}

/// A blow on the player.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Strike {
    pub amount: f32,
    pub source: DamageSource,
    /// Where it came from, which a shield must face to block it.
    pub from: Option<DVec3>,
    /// A projectile, which a shield blocks without pushing back anyone.
    pub ranged: bool,
    /// Struck with an axe, which puts a raised shield out of use.
    pub axe: bool,
}

/// What a blow did.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Outcome {
    /// A raised shield took it.
    Blocked,
    /// It hurt; `fresh` hits knock the player back.
    Hurt { fresh: bool },
    /// Nothing happened (creative, dead, or a smaller hit while hurt).
    Unhurt,
}

/// The result of a swing at a mob.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Blow {
    pub damage: f32,
    pub critical: bool,
    pub sprint_knockback: bool,
    pub sweep: bool,
}

/// Works out a swing with `base` damage (the item's attack damage).
pub fn resolve(base: f32, stance: &Stance, sword: bool) -> Blow {
    let mut damage = base * charge_scale(stance.charge);
    let critical = stance.is_critical();
    if critical {
        damage *= c::CRIT_MULTIPLIER;
    }
    Blow {
        damage,
        critical,
        sprint_knockback: stance.sprint_knockback(),
        sweep: stance.sweeps(sword),
    }
}

impl Game {
    /// Swings a second of the main hand's item.
    pub(super) fn attack_speed(&self) -> f32 {
        self.selected_tool()
            .map_or(c::HAND_SPEED, |tool| tool.attack_speed)
    }

    /// Damage of a full swing with the main hand's item.
    pub(super) fn attack_damage(&self) -> f32 {
        self.selected_tool()
            .map_or(c::HAND_DAMAGE, |tool| tool.attack)
    }

    /// How charged the next swing is, `partial` of the way into a tick.
    pub fn attack_charge(&self, partial: f32) -> f32 {
        charge(self.attack_ticker, self.attack_speed(), partial)
    }

    /// One tick of recharging; switching items starts over.
    pub(super) fn tick_combat(&mut self) {
        let item = self.inventory.selected_stack().map(|stack| stack.id);
        if item != self.last_main_item {
            self.last_main_item = item;
            self.attack_ticker = 0;
        } else {
            self.attack_ticker = self.attack_ticker.saturating_add(1);
        }
        self.shield_cooldown = self.shield_cooldown.saturating_sub(1);
    }

    /// What the attack indicator shows now: the bar while recharging, the
    /// sword when charged on a mob under the crosshair (and the weapon
    /// takes over a quarter second to recharge).
    pub fn attack_indicator(&self) -> crate::ui::gui::AttackIndicator {
        let charge = self.attack_charge(self.tick_alpha() as f32);
        let eye = self.body.position() + DVec3::Y * self.body.eye_height();
        let on_mob = self.target_mob(eye, self.target.as_ref()).is_some();
        crate::ui::gui::AttackIndicator {
            charge,
            ready: charge >= 1.0 && on_mob && 20.0 / self.attack_speed() > 5.0,
        }
    }

    /// Swinging at nothing wastes the swing.
    pub(super) fn swing_at_air(&mut self) {
        self.attack_ticker = 0;
    }

    /// The attacker's situation this tick.
    fn stance(&self, charge: f32) -> Stance {
        let (walked, speed) = self.view.walk_step();
        Stance {
            charge,
            sprinting: self.body.sprinting,
            falling: self.stats.fall_distance > 0.0,
            on_ground: self.body.on_ground,
            in_water: self.body.liquid == Some(crate::world::block::Fluid::Water),
            in_lava: self.body.liquid == Some(crate::world::block::Fluid::Lava),
            on_climbable: self.body.on_climbable,
            walked,
            speed: if self.body.sprinting {
                speed * 1.3
            } else {
                speed
            },
        }
    }

    /// Hits mob `index` with the main hand's item (modern
    /// `Player.attack`): the swing's damage by charge, a critical bonus,
    /// knockback (more when sprinting), a sweep from swords, weapon wear
    /// and exhaustion.
    pub(super) fn attack(&mut self, index: usize) {
        let tool = self.selected_tool().cloned();
        let sword = tool.as_ref().is_some_and(|t| t.kind == ToolKind::Sword);
        let charge = self.attack_charge(0.5);
        let blow = resolve(self.attack_damage(), &self.stance(charge), sword);
        self.attack_ticker = 0;
        let from = self.body.position();
        let yaw = self.camera.yaw as f64;
        let facing = DVec3::new(yaw.sin(), 0.0, -yaw.cos());
        let target = self.mobs[index].body.aabb();
        let hit = self.mobs[index].hurt(blow.damage, Some(from), DamageSource::Mob);
        if hit {
            if blow.sprint_knockback {
                push(&mut self.mobs[index].body, c::SPRINT_KNOCKBACK, facing);
                self.body.velocity.x *= 0.6;
                self.body.velocity.z *= 0.6;
                self.body.sprinting = false;
            }
            if blow.sweep {
                self.sweep(index, target, facing);
            }
            if blow.critical {
                let at = self.mobs[index].feet() + DVec3::Y * (target.max.y - target.min.y) * 0.5;
                let rng = &mut self.rng;
                let mut random = || rng.next_f64() as f32;
                self.effects.extend(Effect::crit(at, &mut random));
            }
        }
        if self.mode.is_vulnerable() {
            self.stats.add_exhaustion(c::EXHAUSTION);
        }
        if self.mode.wears_tools()
            && let Some(tool) = tool
        {
            self.inventory.damage_selected(tool.hit_wear, &self.items);
        }
    }

    /// A sword's sweep: everything else living near the target that is
    /// within three blocks of the player takes a point and is pushed along
    /// the swing.
    fn sweep(&mut self, hit: usize, target: crate::player::collision::Aabb, facing: DVec3) {
        let (gx, gy, gz) = c::SWEEP_GROW;
        let area = crate::player::collision::Aabb::new(
            target.min - DVec3::new(gx, gy, gz),
            target.max + DVec3::new(gx, gy, gz),
        );
        let from = self.body.position();
        for (i, mob) in self.mobs.iter_mut().enumerate() {
            if i == hit
                || mob.is_dead()
                || !mob.body.aabb().intersects(&area)
                || mob.feet().distance_squared(from) >= c::SWEEP_RANGE * c::SWEEP_RANGE
            {
                continue;
            }
            push(&mut mob.body, c::SWEEP_KNOCKBACK, facing);
            mob.hurt(c::SWEEP_DAMAGE, Some(from), DamageSource::Mob);
        }
        let yaw = self.camera.yaw as f64;
        let in_front = self.body.position()
            + DVec3::new(yaw.sin(), 0.0, -yaw.cos()) * 2.0
            + DVec3::Y * self.body.aabb().max.y * 0.5;
        self.effects
            .extend(Effect::sweep(in_front, self.camera.yaw));
    }

    /// Keeps the shield in `hand` raised while right-click is held with it,
    /// or lowers it. A disabled shield stays down.
    pub(super) fn keep_blocking(&mut self, hand: Option<Hand>) {
        let raised = hand.filter(|&h| {
            self.shield_cooldown == 0
                && self
                    .inventory
                    .hand_stack(h)
                    .is_some_and(|stack| stack.id == items::SHIELD)
        });
        match raised {
            Some(h) => {
                self.shield_hand = Some(h);
                self.shield_ticks += 1;
            }
            None => {
                self.shield_hand = None;
                self.shield_ticks = 0;
            }
        }
    }

    /// Whether a shield is up and ready to take a blow (it takes 5 ticks to
    /// raise).
    pub fn is_blocking(&self) -> bool {
        self.shield_hand.is_some() && self.shield_ticks >= c::SHIELD_RAISE_TICKS
    }

    /// The hand holding a raised shield, for drawing it.
    pub fn shield_hand(&self) -> Option<Hand> {
        self.shield_hand
    }

    /// Ticks left before a disabled shield can be raised again.
    pub fn shield_cooldown(&self) -> u32 {
        self.shield_cooldown
    }

    /// Puts the shield out of use for `ticks` (an axe blow: 100).
    pub fn disable_shield(&mut self, ticks: u32) {
        self.shield_cooldown = ticks;
        self.shield_hand = None;
        self.shield_ticks = 0;
    }

    /// Hurts the player, unless a raised shield facing the blow takes it
    /// (modern `LivingEntity.hurt` with `isDamageSourceBlocked`): the
    /// shield wears by one plus the damage for blows of three or more, and
    /// an axe disables it for five seconds.
    pub fn hurt_player(&mut self, strike: Strike) -> Outcome {
        let blockable = matches!(strike.source, DamageSource::Mob | DamageSource::Arrow);
        if blockable && self.is_blocking() && self.faces(strike.from) {
            if strike.amount >= c::SHIELD_WEAR_FROM
                && self.mode.wears_tools()
                && let Some(hand) = self.shield_hand
            {
                let wear = 1 + strike.amount.floor() as u16;
                if self.inventory.damage_hand(hand, wear, &self.items) {
                    self.shield_hand = None;
                    self.shield_ticks = 0;
                }
            }
            if strike.axe {
                self.disable_shield(c::SHIELD_DISABLE_TICKS);
            }
            return Outcome::Blocked;
        }
        if self.stats.damage(strike.amount, strike.source) {
            Outcome::Hurt { fresh: true }
        } else {
            Outcome::Unhurt
        }
    }

    /// Whether the player is turned toward `from`.
    fn faces(&self, from: Option<DVec3>) -> bool {
        let Some(from) = from else {
            return false;
        };
        let to_player = self.body.position() - from;
        let flat = DVec3::new(to_player.x, 0.0, to_player.z);
        flat.length_squared() > 1e-9 && flat.normalize().dot(self.camera.forward().as_dvec3()) < 0.0
    }

    /// Whether the main hand's item is an axe.
    pub fn holds_axe(&self) -> bool {
        self.selected_tool()
            .is_some_and(|tool: &Tool| tool.kind == ToolKind::Axe)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        entity::mob::Mob,
        game::tests::{flat_game, mob_registry},
        inventory::{items, stack::ItemStack},
        player::mode::GameMode,
    };

    #[test]
    fn swings_recharge_over_the_items_attack_speed() {
        // A sword swings 1.6 a second: 12.5 ticks to recharge.
        assert!((charge(0, 1.6, 0.0) - 0.04).abs() < 1e-6);
        assert!((charge(6, 1.6, 0.0) - 6.5 / 12.5).abs() < 1e-6);
        assert_eq!(charge(12, 1.6, 0.5), 1.0);
        // Bare hands swing 4 a second: 5 ticks.
        assert_eq!(charge(5, 4.0, 0.0), 1.0);
        assert!((charge(2, 4.0, 0.0) - 0.5).abs() < 1e-6);
        // A slow iron axe (0.9 a second) takes 22 ticks.
        assert!(charge(20, 0.9, 0.0) < 1.0 && charge(22, 0.9, 0.0) == 1.0);
        // Always between 0 and 1.
        assert_eq!(charge(1000, 0.8, 0.9), 1.0);
    }

    #[test]
    fn the_charge_curve_scales_damage_from_a_fifth_to_all() {
        assert!((charge_scale(0.0) - 0.2).abs() < 1e-6);
        assert!((charge_scale(0.5) - 0.4).abs() < 1e-6);
        assert!((charge_scale(0.9) - 0.848).abs() < 1e-6);
        assert!((charge_scale(1.0) - 1.0).abs() < 1e-6);
        assert!(is_strong(0.95) && !is_strong(0.9) && !is_strong(0.5));
    }

    fn falling() -> Stance {
        Stance {
            charge: 1.0,
            falling: true,
            speed: 0.1,
            ..Stance::default()
        }
    }

    #[test]
    fn criticals_need_a_strong_swing_while_falling_free_of_everything() {
        assert!(falling().is_critical());
        assert!(
            !Stance {
                charge: 0.85,
                ..falling()
            }
            .is_critical()
        );
        assert!(
            !Stance {
                on_ground: true,
                ..falling()
            }
            .is_critical()
        );
        assert!(
            !Stance {
                in_water: true,
                ..falling()
            }
            .is_critical()
        );
        assert!(
            !Stance {
                in_lava: true,
                ..falling()
            }
            .is_critical()
        );
        assert!(
            !Stance {
                on_climbable: true,
                ..falling()
            }
            .is_critical()
        );
        assert!(
            !Stance {
                sprinting: true,
                ..falling()
            }
            .is_critical()
        );
        assert!(
            !Stance {
                falling: false,
                ..falling()
            }
            .is_critical()
        );
    }

    #[test]
    fn sweeps_need_a_sword_a_strong_uncritical_swing_on_the_ground_standing_nearly_still() {
        let still = Stance {
            charge: 1.0,
            on_ground: true,
            speed: 0.1,
            walked: 0.05,
            ..Stance::default()
        };
        assert!(still.sweeps(true));
        assert!(!still.sweeps(false), "only swords sweep");
        assert!(
            !Stance {
                charge: 0.8,
                ..still
            }
            .sweeps(true)
        );
        assert!(
            !Stance {
                walked: 0.13,
                ..still
            }
            .sweeps(true),
            "walking at full speed"
        );
        assert!(
            !Stance {
                sprinting: true,
                ..still
            }
            .sweeps(true)
        );
        assert!(
            !Stance {
                on_ground: false,
                falling: true,
                ..still
            }
            .sweeps(true)
        );
    }

    #[test]
    fn a_blow_is_the_charged_damage_times_one_and_a_half_when_critical() {
        let full = resolve(7.0, &falling(), true);
        assert_eq!(full.damage, 10.5);
        assert!(full.critical && !full.sweep);
        let weak = resolve(
            7.0,
            &Stance {
                charge: 0.0,
                ..falling()
            },
            true,
        );
        assert!((weak.damage - 1.4).abs() < 1e-5 && !weak.critical);
        let sprint = resolve(
            6.0,
            &Stance {
                charge: 1.0,
                sprinting: true,
                on_ground: true,
                speed: 0.13,
                ..Stance::default()
            },
            true,
        );
        assert_eq!(sprint.damage, 6.0);
        assert!(sprint.sprint_knockback && !sprint.sweep);
    }

    #[test]
    fn the_item_data_has_modern_attack_values() {
        let items = crate::inventory::stack::Items::new();
        let tool = |id| items.tool(id).unwrap().clone();
        let check = |id, damage: f32, speed: f32| {
            let t = tool(id);
            assert_eq!((t.attack, t.attack_speed), (damage, speed), "{id:?}");
        };
        check(items::WOODEN_SWORD, 4.0, 1.6);
        check(items::STONE_SWORD, 5.0, 1.6);
        check(items::IRON_SWORD, 6.0, 1.6);
        check(items::GOLDEN_SWORD, 4.0, 1.6);
        check(items::DIAMOND_SWORD, 7.0, 1.6);
        check(items::WOODEN_AXE, 7.0, 0.8);
        check(items::STONE_AXE, 9.0, 0.8);
        check(items::IRON_AXE, 9.0, 0.9);
        check(items::GOLDEN_AXE, 7.0, 1.0);
        check(items::DIAMOND_AXE, 9.0, 1.0);
        check(items::WOODEN_PICKAXE, 2.0, 1.2);
        check(items::DIAMOND_PICKAXE, 5.0, 1.2);
        check(items::WOODEN_SHOVEL, 2.5, 1.0);
        check(items::DIAMOND_SHOVEL, 5.5, 1.0);
        check(items::WOODEN_HOE, 1.0, 1.0);
        check(items::STONE_HOE, 1.0, 2.0);
        check(items::IRON_HOE, 1.0, 3.0);
        check(items::DIAMOND_HOE, 1.0, 4.0);
        // Axes hit harder per swing than swords but recover slower.
        assert!(tool(items::IRON_AXE).attack > tool(items::IRON_SWORD).attack);
        assert!(tool(items::IRON_AXE).attack_speed < tool(items::IRON_SWORD).attack_speed);
        // Anything else is a bare hand.
        assert!(items.tool(items::STICK).is_none());
    }

    /// A survival game with a zombie 1.5 blocks ahead and `item` in the
    /// first slot, ready to swing at full charge.
    fn duel(item: crate::inventory::item::ItemId) -> Game {
        let mut game = flat_game(GameMode::Survival);
        game.mob_defs = mob_registry();
        let kind = game.mob_defs.find("zombie").unwrap();
        let zombie = Mob::new(
            kind,
            game.mob_defs.get(kind),
            DVec3::new(0.5, 64.0, -1.5),
            0.0,
        );
        game.mobs.push(zombie);
        game.inventory.set(0, Some(ItemStack::new(item, 1)));
        game.inventory.selected = 0;
        game.camera.pitch = 0.0;
        game.time = 18000;
        game.run_ticks(1);
        game.attack_ticker = 100;
        game.mobs[0].body.set_position(DVec3::new(0.5, 64.0, -1.5));
        game
    }

    fn swing(game: &mut Game) {
        game.break_clicked = true;
        game.tick();
    }

    #[test]
    fn a_full_swing_deals_the_items_damage_and_a_hasty_one_a_fifth() {
        let mut game = duel(items::IRON_SWORD);
        swing(&mut game);
        assert_eq!(game.mobs[0].stats.health, 20.0 - 6.0);
        // Straight away again is absorbed; the charge is spent.
        assert_eq!(game.attack_charge(0.0), game.attack_charge(0.0));
        assert!(game.attack_charge(0.0) < 0.2);

        // A swing right after switching to the sword does about a fifth.
        let mut game = duel(items::IRON_SWORD);
        game.inventory
            .set(1, Some(ItemStack::new(items::DIAMOND_SWORD, 1)));
        game.inventory.selected = 1;
        game.tick();
        game.break_clicked = true;
        game.tick();
        let dealt = 20.0 - game.mobs[0].stats.health;
        assert!(
            (dealt - 7.0 * charge_scale(charge(0, 1.6, 0.5))).abs() < 0.01,
            "{dealt}"
        );
        assert!(dealt < 2.0, "{dealt}");
    }

    #[test]
    fn the_indicator_shows_a_bar_while_recharging_and_a_sword_when_ready_on_a_mob() {
        let mut game = duel(items::IRON_SWORD);
        game.advance(0.0);
        let ready = game.attack_indicator();
        assert!(ready.ready && ready.charge == 1.0, "{ready:?}");
        swing(&mut game);
        let after = game.attack_indicator();
        assert!(!after.ready && after.charge < 0.3, "{after:?}");
        // Charged but nobody there: nothing to show.
        game.mobs.clear();
        game.run_ticks(20);
        let alone = game.attack_indicator();
        assert!(!alone.ready && alone.charge == 1.0, "{alone:?}");
        // Bare hands recharge in a quarter second, too fast for the sword icon.
        let mut hands = duel(items::STICK);
        hands.inventory.set(0, None);
        hands.advance(0.0);
        hands.attack_ticker = 100;
        assert!(!hands.attack_indicator().ready);
    }

    #[test]
    fn switching_items_and_swinging_at_air_start_the_charge_over() {
        let mut game = duel(items::IRON_SWORD);
        assert_eq!(game.attack_charge(0.0), 1.0);
        game.inventory
            .set(1, Some(ItemStack::new(items::IRON_AXE, 1)));
        game.inventory.selected = 1;
        game.tick();
        assert!(game.attack_charge(0.0) < 0.1, "{}", game.attack_charge(0.0));
        for _ in 0..30 {
            game.tick();
        }
        assert_eq!(game.attack_charge(0.0), 1.0);
        // Nothing in reach: a swing at the air.
        game.mobs.clear();
        game.camera.pitch = 0.4;
        game.advance(0.0);
        game.break_clicked = true;
        game.tick();
        assert!(game.attack_charge(0.0) < 0.1, "{}", game.attack_charge(0.0));
    }

    #[test]
    fn a_falling_strong_hit_is_critical_and_throws_stars() {
        let mut game = duel(items::DIAMOND_SWORD);
        game.body.on_ground = false;
        game.stats.fall_distance = 1.0;
        game.attack(0);
        assert_eq!(game.mobs[0].stats.health, 20.0 - 10.5);
        assert!(game.effects.len() >= 40, "{}", game.effects.len());
    }

    #[test]
    fn a_sprinting_strong_hit_knocks_back_further_and_ends_the_sprint() {
        let kick = |sprint: bool| {
            let mut game = duel(items::WOODEN_SWORD);
            game.body.sprinting = sprint;
            game.attack(0);
            (game.mobs[0].body.velocity.z, game.body.sprinting)
        };
        let (plain, _) = kick(false);
        let (sprinted, still_sprinting) = kick(true);
        assert!(plain < 0.0 && sprinted < plain - 0.25, "{plain} {sprinted}");
        assert!(!still_sprinting);
    }

    #[test]
    fn a_sword_sweeps_a_neighbour_for_a_point() {
        let mut game = duel(items::IRON_SWORD);
        let kind = game.mob_defs.find("zombie").unwrap();
        let near = Mob::new(
            kind,
            game.mob_defs.get(kind),
            DVec3::new(1.5, 64.0, -1.5),
            0.0,
        );
        let far = Mob::new(
            kind,
            game.mob_defs.get(kind),
            DVec3::new(0.5, 64.0, -3.5),
            0.0,
        );
        game.mobs.push(near);
        game.mobs.push(far);
        game.body.on_ground = true;
        swing(&mut game);
        assert_eq!(game.mobs[0].stats.health, 14.0, "the target");
        assert_eq!(game.mobs[1].stats.health, 19.0, "swept");
        assert_eq!(game.mobs[2].stats.health, 20.0, "too far away");
        assert!(!game.effects.is_empty(), "the arc shows");
        // An axe doesn't sweep.
        let mut axe = duel(items::IRON_AXE);
        let kind = axe.mob_defs.find("zombie").unwrap();
        let near = Mob::new(
            kind,
            axe.mob_defs.get(kind),
            DVec3::new(1.5, 64.0, -1.5),
            0.0,
        );
        axe.mobs.push(near);
        axe.body.on_ground = true;
        swing(&mut axe);
        assert_eq!(axe.mobs[0].stats.health, 20.0 - 9.0);
        assert_eq!(axe.mobs[1].stats.health, 20.0);
    }
}
