//! Advancements in play (M29): what the game reports as it happens, the
//! periodic looks at the inventory, biome and structures, and the toasts
//! that pop up when one is made (modern `PlayerAdvancements.award`).

use glam::IVec3;

use super::Game;
use crate::{
    advancements::{self, Event, Progress},
    player::effects::Effect,
    world::{dimension::Dimension, structure},
};

/// Ticks a toast stays: five seconds (modern).
pub const TOAST_TICKS: u32 = 100;
/// Ticks between looks at the inventory, and at the biome and structures.
const SCAN_EVERY: u64 = 10;
const SURVEY_EVERY: u64 = 20;
/// How far Levitation must lift the player for "Great View From Up Here".
const LEVITATE_BLOCKS: f64 = 50.0;

/// An advancement just made, still on screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Toast {
    pub index: usize,
    pub ticks: u32,
}

impl Game {
    /// What the world has done.
    pub fn progress(&self) -> &Progress {
        &self.progress
    }

    /// Shows the advancements screen on tab `tab` in a `--shot` scene.
    pub fn stage_advancements(&mut self, tab: usize) {
        self.shot_tab = Some(tab);
    }

    /// The tab a scene wants shown, if it wants the screen.
    pub fn shot_advancements(&self) -> Option<usize> {
        self.shot_tab
    }

    pub fn toasts(&self) -> &[Toast] {
        &self.toasts
    }

    /// Counts `event`; every advancement it completes pops up a toast
    /// (except a tab's first, which the game makes at its start).
    pub fn note(&mut self, event: Event) {
        let registry = advancements::registry();
        for index in self.progress.record(registry, &event) {
            if registry.defs[index].parent.is_some() {
                self.toasts.push(Toast {
                    index,
                    ticks: TOAST_TICKS,
                });
                log::info!("advancement: {}", registry.defs[index].title);
            }
        }
    }

    /// What the dimension is called in the data: `the_nether`.
    fn dimension_slug(dimension: Dimension) -> String {
        crate::world::block::strip_namespace(dimension.name()).to_owned()
    }

    /// Reports the player's arrival in `dimension`.
    pub(super) fn note_dimension(&mut self, dimension: Dimension) {
        self.note(Event::Dimension(Self::dimension_slug(dimension)));
    }

    /// One tick: toasts age, and now and then the inventory, the biome and
    /// the structures round the player are looked at.
    pub(super) fn tick_advancements(&mut self) {
        for toast in &mut self.toasts {
            toast.ticks = toast.ticks.saturating_sub(1);
        }
        self.toasts.retain(|t| t.ticks > 0);
        if self.time == 0 {
            self.note(Event::Used("start".to_owned()));
        }
        if self.time.is_multiple_of(SCAN_EVERY) {
            self.scan_inventory();
        }
        if self.time.is_multiple_of(SURVEY_EVERY) {
            self.survey();
        }
        self.track_levitation();
    }

    /// Everything the player carries (an item a `Have` asks about).
    fn scan_inventory(&mut self) {
        let wanted = advancements::registry().wanted_items();
        let mut have = Vec::new();
        for index in 0..crate::inventory::player::SLOTS {
            if let Some(stack) = self.inventory.slot(index) {
                let name = stack.id.name();
                if wanted.contains(name) && !have.contains(&name) {
                    have.push(name);
                }
            }
        }
        for name in have {
            self.note(Event::Have(name.to_owned()));
        }
    }

    /// The biome and any structure the player is in.
    fn survey(&mut self) {
        let feet = self.body.position();
        let cell = feet.floor().as_ivec3();
        if let Some(biome) = self.chunks.world().biome_at_block(cell.x, cell.y, cell.z) {
            self.note(Event::Biome(biome.name().to_lowercase().replace(' ', "_")));
        }
        let kind = self.structure_here(cell);
        if let Some(kind) = kind {
            self.note(Event::Structure(kind.to_owned()));
        }
    }

    /// The name of the structure whose pieces hold `cell`, if any.
    fn structure_here(&self, cell: IVec3) -> Option<&'static str> {
        if let Some(terrain) = self.chunks.terrain() {
            return structure::containing(terrain, cell).map(|s| s.kind.name());
        }
        if let Some(nether) = self.chunks.nether() {
            return structure::nether::kind_at(nether, cell).map(structure::Kind::name);
        }
        if let Some(end) = self.chunks.end() {
            let pos = crate::world::chunk::ChunkPos::of_block(cell.x, cell.z);
            return structure::end::near(end, pos)
                .into_iter()
                .any(|s| {
                    s.pieces
                        .iter()
                        .filter_map(|p| p.bounds)
                        .any(|b| b.contains(cell))
                })
                .then(|| structure::Kind::EndCity.name());
        }
        None
    }

    /// Levitation lifting the player 50 blocks.
    fn track_levitation(&mut self) {
        let y = self.body.position().y;
        if self.stats.effects.has(Effect::Levitation) {
            let start = *self.levitation_from.get_or_insert(y);
            if y - start >= LEVITATE_BLOCKS {
                self.note(Event::Used("levitate".to_owned()));
            }
        } else if self.body.on_ground {
            self.levitation_from = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        game::tests::flat_game,
        inventory::{items, stack::ItemStack},
        player::mode::GameMode,
    };

    fn made(game: &Game, id: &str) -> bool {
        game.progress().is_done(id)
    }

    #[test]
    fn a_new_world_starts_with_the_story_root_and_no_toast_for_it() {
        let mut game = flat_game(GameMode::Survival);
        game.run_ticks(2);
        assert!(made(&game, "story/root"));
        assert!(game.toasts().is_empty());
    }

    #[test]
    fn carrying_an_item_makes_its_advancement_and_a_toast_that_fades() {
        let mut game = flat_game(GameMode::Survival);
        game.run_ticks(2);
        game.inventory
            .set(3, Some(ItemStack::new(items::COBBLESTONE, 4)));
        game.run_ticks(11);
        assert!(made(&game, "story/mine_stone"));
        assert_eq!(game.toasts().len(), 1);
        let registry = advancements::registry();
        assert_eq!(registry.defs[game.toasts()[0].index].title, "Stone Age");
        game.run_ticks(TOAST_TICKS + 5);
        assert!(game.toasts().is_empty());
        // Having it again does nothing.
        game.inventory
            .set(4, Some(ItemStack::new(items::COBBLESTONE, 4)));
        game.run_ticks(15);
        assert!(game.toasts().is_empty());
    }

    #[test]
    fn mining_killing_eating_and_the_rest_are_counted() {
        use crate::world::{block::BlockState, blocks};
        let mut game = flat_game(GameMode::Creative);
        game.mob_defs = crate::game::tests::mob_registry();
        game.run_ticks(2);
        // Mining.
        let at = glam::IVec3::new(3, 63, 3);
        game.set_blocks(&[(at, BlockState::of(blocks::OBSIDIAN))]);
        game.note(Event::Mined("obsidian".into()));
        game.note(Event::Used("trade".into()));
        assert!(made(&game, "adventure/trade"));
        game.note(Event::Killed {
            mob: "zombie".into(),
            hostile: true,
        });
        assert!(made(&game, "adventure/kill_a_mob") && made(&game, "adventure/root"));
        game.note(Event::Ate("bread".into()));
        assert!(made(&game, "husbandry/root"));
        // The balanced diet needs everything, one by one.
        assert!(!made(&game, "husbandry/balanced_diet"));
        game.note(Event::Dimension("the_nether".into()));
        assert!(made(&game, "nether/root") && made(&game, "story/enter_the_nether"));
        assert!(
            game.progress()
                .tally(advancements::registry(), advancements::Tab::Nether)
                .0
                >= 1
        );
    }

    #[test]
    fn the_end_tab_follows_the_dragon_and_what_comes_after() {
        let mut game = flat_game(GameMode::Creative);
        game.note(Event::Dimension("the_end".into()));
        game.note(Event::Used("kill_dragon".into()));
        game.note(Event::Used("gateway".into()));
        game.note(Event::Structure("end city".into()));
        game.inventory
            .set(0, Some(ItemStack::new(items::ELYTRA, 1)));
        game.run_ticks(12);
        for id in [
            "end/root",
            "end/kill_dragon",
            "end/enter_end_gateway",
            "end/find_end_city",
            "end/elytra",
            "adventure/glide",
        ] {
            assert!(made(&game, id), "{id}");
        }
    }

    #[test]
    fn what_a_world_has_done_is_saved_and_comes_back() {
        use crate::{
            game::tests::{mob_registry, registries},
            level::WorldDir,
            world::save::TempDir,
        };
        let saves = TempDir::new("adv");
        let (items_, models) = registries();
        let mut game = Game::create(
            saves.path(),
            "Advances",
            3,
            GameMode::Survival,
            items_.clone(),
            models.clone(),
        )
        .unwrap()
        .with_mobs(mob_registry());
        game.run_ticks(2);
        game.note(Event::Used("trade".into()));
        game.note(Event::Mined("stone".into()));
        game.save().unwrap();
        drop(game);
        let dir = WorldDir::new(saves.path().join("Advances"));
        let game = Game::open(dir, items_, models).unwrap();
        assert!(made(&game, "adventure/trade"));
        assert!(made(&game, "story/root"));
    }

    #[test]
    fn levitating_fifty_blocks_makes_great_view() {
        let mut game = flat_game(GameMode::Survival);
        game.stats.effects.add(crate::player::effects::Active::new(
            Effect::Levitation,
            0,
            10_000,
        ));
        game.run_ticks(2);
        game.body
            .set_position(game.body.position() + glam::DVec3::Y * 51.0);
        game.run_ticks(2);
        assert!(made(&game, "end/levitate"));
    }

    #[test]
    fn a_survival_playthrough_unlocks_the_story_in_order_across_the_dimensions() {
        let mut game = flat_game(GameMode::Survival);
        game.mob_defs = crate::game::tests::mob_registry();
        let give = |game: &mut Game, slot: usize, id| {
            game.inventory.set(slot, Some(ItemStack::new(id, 1)));
            game.run_ticks(11);
        };
        game.run_ticks(2);
        give(&mut game, 0, items::COBBLESTONE);
        give(&mut game, 1, items::STONE_PICKAXE);
        give(&mut game, 2, items::IRON_INGOT);
        give(&mut game, 3, items::IRON_PICKAXE);
        give(&mut game, 4, items::DIAMOND);
        give(&mut game, 5, items::OBSIDIAN);
        give(&mut game, 6, items::IRON_CHESTPLATE);
        for id in [
            "story/mine_stone",
            "story/upgrade_tools",
            "story/smelt_iron",
            "story/iron_tools",
            "story/mine_diamond",
            "story/form_obsidian",
            "story/obtain_armor",
        ] {
            assert!(made(&game, id), "{id}");
        }
        assert!(!made(&game, "story/enter_the_nether"));
        // Through the portal: two tabs at once.
        game.note(Event::Dimension("the_nether".into()));
        game.note(Event::Structure("nether fortress".into()));
        give(&mut game, 7, items::BLAZE_ROD);
        for id in [
            "story/enter_the_nether",
            "nether/root",
            "nether/find_fortress",
            "nether/obtain_blaze_rod",
        ] {
            assert!(made(&game, id), "{id}");
        }
        game.note(Event::Used("brew".into()));
        game.note(Event::Killed {
            mob: "ghast".into(),
            hostile: true,
        });
        assert!(made(&game, "nether/brew_potion") && made(&game, "nether/return_to_sender"));
        // A stronghold, then the End and its dragon.
        game.note(Event::Structure("stronghold".into()));
        game.note(Event::Dimension("the_end".into()));
        game.note(Event::Used("kill_dragon".into()));
        for id in [
            "story/follow_ender_eye",
            "story/enter_the_end",
            "end/root",
            "end/kill_dragon",
        ] {
            assert!(made(&game, id), "{id}");
        }
        // Every toast was for something with a parent, five at a time at most on screen.
        let registry = advancements::registry();
        assert!(
            game.toasts()
                .iter()
                .all(|t| registry.defs[t.index].parent.is_some())
        );
        let (done, total) = game
            .progress()
            .tally(registry, advancements::Tab::Minecraft);
        assert!(done >= 11 && done < total, "{done}/{total}");
    }
}
