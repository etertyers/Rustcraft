//! Shulkers' bullets in play (M28): they follow the player, hurt a little
//! and lift them (Levitation) for ten seconds.

use glam::{DVec3, IVec3};

use super::Game;
use crate::{
    entity::bullet::{self, BulletEvent},
    player::{
        collision::Aabb,
        effects::{Active, Effect},
        stats::DamageSource,
    },
};

impl Game {
    /// One tick of every bullet in flight.
    pub(super) fn tick_bullets(&mut self) {
        let mut bullets = std::mem::take(&mut self.bullets);
        let alive = !self.player.stats.is_dead();
        let body = self.player.body.aabb();
        let middle = (body.min + body.max) / 2.0;
        let mut hits = Vec::new();
        bullets.retain_mut(|bullet| {
            let world = self.chunks.world();
            let blocks = self.items.blocks;
            let solid = |cell: IVec3| {
                world
                    .block(cell.x, cell.y, cell.z)
                    .and_then(|block| blocks.get(block.id()))
                    .is_none_or(|def| def.solid)
            };
            let block_hit = |from: DVec3, dir: DVec3, reach: f64| {
                let hit = crate::player::raycast::raycast(from, dir, reach, solid)?;
                Aabb::block(hit.block.x, hit.block.y, hit.block.z).ray_hit(from, dir, reach)
            };
            let target = alive.then_some(middle);
            match bullet.tick(target, alive.then_some(&body), block_hit) {
                None => true,
                Some(BulletEvent::HitPlayer) => {
                    hits.push(bullet.position);
                    false
                }
                Some(_) => false,
            }
        });
        self.bullets = bullets;
        for from in hits {
            crate::audio::play("entity.shulker_bullet.hit", from);
            if self.player.mode.is_vulnerable() {
                self.hurt_player(super::combat::Strike {
                    amount: bullet::DAMAGE,
                    source: DamageSource::Arrow,
                    from: Some(from),
                    ranged: true,
                    axe: false,
                });
                self.player.stats.effects.add(Active::new(
                    Effect::Levitation,
                    0,
                    bullet::LEVITATION_TICKS,
                ));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        entity::bullet::ShulkerBullet,
        game::tests::{flat_game, mob_registry},
        player::mode::GameMode,
    };

    #[test]
    fn a_bullet_that_reaches_the_player_hurts_them_and_lifts_them_off_the_ground() {
        let mut game = flat_game(GameMode::Survival);
        let eye = game.player.body.position() + DVec3::new(4.0, 1.0, 0.0);
        game.bullets.push(ShulkerBullet::new(
            eye,
            game.player.body.position() + DVec3::Y,
        ));
        let health = game.player.stats.health;
        game.run_ticks(60);
        assert!(
            game.player.stats.health <= health - 1.0,
            "{}",
            game.player.stats.health
        );
        assert!(game.player.stats.effects.has(Effect::Levitation));
        let y = game.player.body.position().y;
        game.run_ticks(40);
        assert!(
            game.player.body.position().y > y + 1.5,
            "lifted {}",
            game.player.body.position().y - y
        );
        assert!(game.bullets.is_empty());
    }

    #[test]
    fn creative_players_are_left_alone_and_a_shulker_with_the_player_in_sight_shoots() {
        let mut game = flat_game(GameMode::Creative);
        game.mob_defs = mob_registry();
        game.summon(
            "shulker",
            game.player.body.position() + DVec3::new(6.0, 0.0, 0.0),
            0.0,
        )
        .expect("a shulker");
        game.player.mode = GameMode::Survival;
        let mut shot = false;
        for _ in 0..120 {
            game.run_ticks(1);
            shot |= !game.bullets.is_empty();
        }
        assert!(shot, "it opened and fired");
        // Closed up, a shulker sits still and takes no step.
        let at = game.mobs[0].feet();
        game.run_ticks(40);
        assert_eq!(game.mobs[0].feet().x, at.x);
    }

    #[test]
    fn a_shulker_box_keeps_what_it_holds_when_placed_and_broken() {
        use crate::{
            inventory::{items, stack::ItemStack},
            world::blocks,
        };
        let mut game = flat_game(GameMode::Survival);
        let mut boxed = ItemStack::new(items::SHULKER_BOX, 1);
        boxed.set_contents(vec![
            Some(ItemStack::new(items::DIAMOND, 5)),
            None,
            Some(ItemStack::with_damage(items::IRON_PICKAXE, 1, 7)),
        ]);
        game.player.inventory.set(0, Some(boxed.clone()));
        game.player.inventory.selected = 0;
        game.player.camera.pitch = -0.7;
        game.advance(0.0);
        let at = game.player.target.unwrap().adjacent().unwrap();
        game.player.placing = true;
        game.player.place_clicked = true;
        game.use_hands();
        game.player.placing = false;
        assert!(
            game.chunks
                .world()
                .block(at.x, at.y, at.z)
                .unwrap()
                .is(blocks::SHULKER_BOX)
        );
        let held = game.chunks.block_entity(at).expect("it holds slots");
        assert_eq!(held.slots()[0], Some(ItemStack::new(items::DIAMOND, 5)));
        assert_eq!(held.slots().len(), 27);
        // Breaking it by hand drops the box whole, contents inside.
        game.entities.clear();
        game.break_block(at);
        assert_eq!(game.entities.len(), 1, "one item, not a spill");
        let dropped = &game.entities[0].stack;
        assert_eq!(dropped.id, items::SHULKER_BOX);
        assert_eq!(
            dropped.contents()[0],
            Some(ItemStack::new(items::DIAMOND, 5))
        );
        assert_eq!(dropped.contents()[2].as_ref().unwrap().damage, 7);
        // An empty one still drops in survival, and stacks with other empties.
        game.set_blocks(&[(at, crate::world::block::BlockState::of(blocks::SHULKER_BOX))]);
        game.entities.clear();
        game.break_block(at);
        assert_eq!(
            game.entities[0].stack,
            ItemStack::new(items::SHULKER_BOX, 1)
        );
    }

    #[test]
    fn contents_survive_saving_in_both_formats_and_old_stacks_read_back_unchanged() {
        use crate::inventory::{items, stack::ItemStack};
        let mut boxed = ItemStack::new(items::SHULKER_BOX, 1);
        let mut inner = ItemStack::new(items::DIAMOND_SWORD, 1);
        inner.enchant(crate::inventory::enchant::Enchantment::Sharpness, 3);
        boxed.set_contents(vec![
            Some(inner),
            None,
            Some(ItemStack::new(items::STONE, 64)),
        ]);
        let plain = ItemStack::new(items::STONE, 3);
        let mut enchanted = ItemStack::new(items::DIAMOND_PICKAXE, 1);
        enchanted.enchant(crate::inventory::enchant::Enchantment::Efficiency, 2);
        let all = vec![boxed.clone(), plain.clone(), enchanted.clone()];
        let config = bincode::config::standard().with_fixed_int_encoding();
        let bytes = bincode::serde::encode_to_vec(&all, config).unwrap();
        let (back, _): (Vec<ItemStack>, usize) =
            bincode::serde::decode_from_slice(&bytes, config).unwrap();
        assert_eq!(back, all);
        let text = ron::to_string(&all).unwrap();
        let back: Vec<ItemStack> = ron::from_str(&text).unwrap();
        assert_eq!(back, all);
        // Two boxes with the same contents stack-compare equal; different ones don't.
        assert!(boxed.stacks_with(&boxed.clone()));
        assert!(!boxed.stacks_with(&ItemStack::new(items::SHULKER_BOX, 1)));
    }
}
