//! Spawn eggs (M61): `<mob>_spawn_egg` makes that mob. On a block it
//! spawns an adult in the cell in front of the clicked face, facing the
//! player (in water, if that cell is water, which is how water mobs get
//! in). On a spawner it sets the spawner's mob (creative, or cheats). On a
//! mob of the same kind that has babies it spawns a baby. A dispenser
//! spawns one in front of it. The egg's custom name names the mob. Eggs
//! are used up in survival and kept in creative. All of it runs on the
//! server; clients see the mob the usual way.

use glam::{DVec3, IVec3};

use super::{Game, animals::has_babies};
use crate::{
    entity::mobs::MobKind,
    inventory::{
        item::{self, ItemId},
        stack::ItemStack,
    },
    player::{mode::GameMode, raycast::Hit},
    world::block_entity::BlockEntity,
};

impl Game {
    /// The mob an egg of item `id` spawns, if `id` is a spawn egg of a mob
    /// this game has.
    pub fn egg_mob(&self, id: ItemId) -> Option<MobKind> {
        let name = item::registry()
            .get(id)?
            .name
            .strip_suffix("_spawn_egg")?
            .to_owned();
        self.mob_defs.find(&name)
    }

    /// Uses up the held egg (survival only).
    fn spend_egg(&mut self) {
        if self.player.mode.uses_up_items() {
            self.player.inventory.take(self.held_slot(), 1);
        }
    }

    /// A new `kind` at `feet`, facing `yaw`, named like the egg if it is
    /// named. `baby` makes it a calf, lamb...
    fn spawn_from_egg(
        &mut self,
        kind: MobKind,
        feet: DVec3,
        yaw: f32,
        name: Option<String>,
        baby: bool,
    ) {
        let mut mob = self.new_mob(kind, feet, yaw);
        mob.data.custom_name = name;
        if baby {
            mob.data.age = -crate::config::farming::BABY_TICKS;
        }
        self.add_mob(mob);
    }

    /// Right-click with an egg on a block. Returns whether it did anything.
    pub(super) fn use_spawn_egg_on_block(&mut self, hit: &Hit) -> bool {
        let Some(stack) = self.held().cloned() else {
            return false;
        };
        let Some(kind) = self.egg_mob(stack.id) else {
            return false;
        };
        // A spawner takes the egg's mob (it is a game-master block).
        if matches!(
            self.chunks.block_entity(hit.block),
            Some(BlockEntity::Spawner(_))
        ) {
            if !(self.player.mode == GameMode::Creative || self.cheats) {
                return false;
            }
            let name = self.mob_defs.get(kind).name.clone();
            if let Some(BlockEntity::Spawner(spawner)) = self.chunks.block_entity_mut(hit.block) {
                spawner.mob = name;
                spawner.delay = 20;
            }
            self.spend_egg();
            return true;
        }
        let Some(cell) = hit.adjacent() else {
            return false;
        };
        let feet = cell.as_dvec3() + DVec3::new(0.5, 0.0, 0.5);
        // Facing the player: turned half a circle from where they look.
        let yaw = self.player.camera.yaw + std::f32::consts::PI;
        self.spawn_from_egg(
            kind,
            feet,
            yaw,
            stack.custom_name().map(str::to_owned),
            false,
        );
        self.spend_egg();
        true
    }

    /// Right-click with an egg on mob `index`: a baby, if it is the egg's
    /// kind and its kind has babies and it is grown.
    pub(super) fn use_spawn_egg_on_mob(&mut self, index: usize) -> bool {
        let Some(stack) = self.held().cloned() else {
            return false;
        };
        let Some(kind) = self.egg_mob(stack.id) else {
            return false;
        };
        let Some(mob) = self.mobs.get(index) else {
            return false;
        };
        let name = self.mob_defs.get(kind).name.clone();
        if mob.kind != kind || !has_babies(&name) || mob.data.age < 0 {
            return false;
        }
        let (feet, yaw, variant) = (mob.feet(), mob.yaw, mob.data.variant);
        self.spawn_from_egg(
            kind,
            feet,
            yaw,
            stack.custom_name().map(str::to_owned),
            true,
        );
        if let Some(baby) = self.mobs.last_mut() {
            baby.data.variant = variant;
        }
        self.spend_egg();
        true
    }

    /// A dispenser at the cell `front` in front of it fires egg `one`: the
    /// mob stands there.
    pub(super) fn dispense_spawn_egg(&mut self, front: IVec3, one: &ItemStack) -> bool {
        let Some(kind) = self.egg_mob(one.id) else {
            return false;
        };
        let feet = front.as_dvec3() + DVec3::new(0.5, 0.0, 0.5);
        self.spawn_from_egg(kind, feet, 0.0, one.custom_name().map(str::to_owned), false);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        game::tests::{flat_game, mob_registry},
        inventory::items,
        world::{Face, block::BlockState, blocks},
    };

    fn egg(mob: &str, count: u8) -> ItemStack {
        ItemStack::new(items::by_name(&format!("{mob}_spawn_egg")).unwrap(), count)
    }

    fn hold(game: &mut Game, stack: ItemStack) {
        game.mob_defs = mob_registry();
        game.player.inventory.set(0, Some(stack));
        game.player.inventory.selected = 0;
    }

    /// Looking at the top of the grass block at (2, 63, 2).
    fn on_grass() -> Hit {
        Hit {
            block: IVec3::new(2, 63, 2),
            face: Some(Face::Top),
            point: DVec3::new(2.5, 64.0, 2.5),
        }
    }

    fn count(game: &Game, mob: &str) -> usize {
        let kind = game.mob_defs.find(mob).unwrap();
        game.mobs.iter().filter(|m| m.kind == kind).count()
    }

    fn held_count(game: &Game) -> u8 {
        game.player.inventory.slot(0).map_or(0, |stack| stack.count)
    }

    /// Every mob has an egg, and each egg's icon is its own texture, not
    /// the placeholder, with no tint.
    #[test]
    fn every_mob_has_an_egg_with_a_texture_that_is_not_the_placeholder() {
        let assets = crate::assets::dir();
        if !crate::platform::fs::is_dir(assets.join("vanilla/assets/minecraft")) {
            eprintln!("skipping: no assets/vanilla");
            return;
        }
        let mobs = crate::entity::mobs::MobRegistry::load(&assets.join("data/mobs.ron")).unwrap();
        let look = crate::look::Look::build(&assets, Vec::new(), &mobs).unwrap();
        let missing = look.atlas.uv_rect(crate::render::atlas::MISSING_TILE);
        let mut without = Vec::new();
        for (_, def) in mobs.iter() {
            let Some(id) = items::by_name(&format!("{}_spawn_egg", def.name)) else {
                without.push(def.name.clone());
                continue;
            };
            let icon = look.icons.icon(&ItemStack::new(id, 1));
            let crate::render::icons::ItemIcon::Flat(face) = icon else {
                panic!("{} egg is drawn as a cube", def.name);
            };
            assert_ne!(face.uv, missing, "{} egg has no texture", def.name);
            assert_eq!(face.color, [1.0; 3], "{} egg is tinted", def.name);
        }
        assert!(without.is_empty(), "mobs with no egg: {without:?}");
    }

    #[test]
    fn an_egg_on_grass_spawns_its_mob_on_top_and_survival_uses_it_up() {
        let mut game = flat_game(crate::player::mode::GameMode::Survival);
        hold(&mut game, egg("pig", 2));
        assert!(game.use_spawn_egg_on_block(&on_grass()));
        assert_eq!(count(&game, "pig"), 1);
        let pig = &game.mobs[game.mobs.len() - 1];
        assert!(pig.data.age >= 0, "an adult");
        assert!(
            (pig.feet() - DVec3::new(2.5, 64.0, 2.5)).length() < 1e-6,
            "{:?}",
            pig.feet()
        );
        assert_eq!(held_count(&game), 1, "one egg used");
        // Creative keeps it.
        let mut creative = flat_game(crate::player::mode::GameMode::Creative);
        hold(&mut creative, egg("pig", 1));
        assert!(creative.use_spawn_egg_on_block(&on_grass()));
        assert_eq!((count(&creative, "pig"), held_count(&creative)), (1, 1));
    }

    #[test]
    fn an_egg_on_a_spawner_sets_its_mob_in_creative_or_with_cheats_only() {
        let at = IVec3::new(5, 64, 5);
        let hit = Hit {
            block: at,
            face: Some(Face::Top),
            point: at.as_dvec3(),
        };
        for (mode, cheats, works) in [
            (crate::player::mode::GameMode::Survival, false, false),
            (crate::player::mode::GameMode::Survival, true, true),
            (crate::player::mode::GameMode::Creative, false, true),
        ] {
            let mut game = flat_game(mode);
            game.cheats = cheats;
            hold(&mut game, egg("cow", 1));
            game.chunks
                .set_blocks(&[(at, BlockState::of(blocks::SPAWNER))]);
            game.chunks.set_block_entity(
                at,
                Some(BlockEntity::Spawner(
                    crate::world::block_entity::Spawner::new("pig"),
                )),
            );
            assert_eq!(
                game.use_spawn_egg_on_block(&hit),
                works,
                "{mode:?} cheats {cheats}"
            );
            let Some(BlockEntity::Spawner(spawner)) = game.chunks.block_entity(at) else {
                panic!("the spawner is gone");
            };
            assert_eq!(spawner.mob, if works { "cow" } else { "pig" });
            assert_eq!(count(&game, "cow"), 0, "the egg doesn't spawn beside it");
        }
    }

    #[test]
    fn an_egg_on_a_grown_mob_of_its_kind_makes_a_calf_and_other_eggs_do_not() {
        let mut game = flat_game(crate::player::mode::GameMode::Survival);
        hold(&mut game, egg("cow", 3));
        let cow = game.summon("cow", DVec3::new(4.5, 64.0, 4.5), 0.0).unwrap();
        cow.data.variant = 0;
        let index = game.mobs.len() - 1;
        assert!(game.use_spawn_egg_on_mob(index));
        assert_eq!(count(&game, "cow"), 2);
        assert_eq!(
            game.mobs.iter().filter(|m| m.data.age < 0).count(),
            1,
            "one calf"
        );
        assert_eq!(held_count(&game), 2);
        // A calf can't have a calf; another kind's egg isn't used on it.
        let calf = game.mobs.iter().position(|m| m.data.age < 0).unwrap();
        assert!(!game.use_spawn_egg_on_mob(calf));
        hold(&mut game, egg("pig", 1));
        assert!(!game.use_spawn_egg_on_mob(index));
        // A mob with no babies (a creeper) gets none.
        hold(&mut game, egg("creeper", 1));
        game.summon("creeper", DVec3::new(6.5, 64.0, 6.5), 0.0)
            .unwrap();
        let creeper = game.mobs.len() - 1;
        assert!(!game.use_spawn_egg_on_mob(creeper));
    }

    #[test]
    fn a_dispenser_spawns_the_mob_in_front_and_the_eggs_name_names_it() {
        let mut game = flat_game(crate::player::mode::GameMode::Survival);
        hold(&mut game, egg("sheep", 1));
        let mut named = egg("zombie", 1);
        named.set_custom_name(Some("Bob".to_owned()));
        assert!(game.dispense_spawn_egg(IVec3::new(6, 64, 6), &named));
        let zombie = &game.mobs[game.mobs.len() - 1];
        assert_eq!(zombie.data.custom_name.as_deref(), Some("Bob"));
        assert!((zombie.feet() - DVec3::new(6.5, 64.0, 6.5)).length() < 1e-6);
        // Used on a block, a named egg names that mob too.
        let mut sheep = egg("sheep", 1);
        sheep.set_custom_name(Some("Dolly".to_owned()));
        game.player.inventory.set(0, Some(sheep));
        assert!(game.use_spawn_egg_on_block(&on_grass()));
        assert_eq!(
            game.mobs[game.mobs.len() - 1].data.custom_name.as_deref(),
            Some("Dolly")
        );
        // Anything that isn't an egg does nothing.
        assert!(!game.dispense_spawn_egg(IVec3::new(7, 64, 7), &ItemStack::new(items::STICK, 1)));
    }

    /// Through the real right-click: looking down at the ground with an egg
    /// in hand spawns the mob, uses the egg up, and a calf comes from using
    /// one on a cow.
    #[test]
    fn right_clicking_with_an_egg_spawns_its_mob_through_the_normal_use_path() {
        use crate::bindings::Action;
        let mut game = flat_game(crate::player::mode::GameMode::Survival);
        hold(&mut game, egg("pig", 2));
        let mut spawned = false;
        for pitch in [-1.4_f32, 1.4] {
            game.player.camera.pitch = pitch;
            game.on_action(Action::Use, true, false, true);
            game.on_action(Action::Use, false, false, true);
            game.run_ticks(2);
            if count(&game, "pig") == 1 {
                spawned = true;
                break;
            }
        }
        assert!(spawned, "no pig after right-clicking the ground");
        assert_eq!(held_count(&game), 1);
    }
}
