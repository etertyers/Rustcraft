//! A furnace's slots and burning, following 1.0.0's `TileEntityFurnace`:
//! fuel burns while there's something to smelt, and each item takes 200
//! ticks.

use serde::{Deserialize, Serialize};

use super::stack::{ItemStack, Items};

/// Ticks to smelt one item.
pub const COOK_TICKS: u32 = 200;

/// The furnace's slots.
pub const INPUT: usize = 0;
pub const FUEL: usize = 1;
pub const OUTPUT: usize = 2;

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Furnace {
    /// Input, fuel and output.
    pub slots: [Option<ItemStack>; 3],
    /// Ticks the current fuel keeps burning.
    pub burn: u32,
    /// Ticks the current fuel burned for in all.
    pub burn_total: u32,
    /// Ticks the input has cooked.
    pub cook: u32,
}

impl Furnace {
    pub fn is_burning(&self) -> bool {
        self.burn > 0
    }

    /// Whether a tick would change nothing: out, and with nothing to start
    /// on.
    pub fn is_idle(&self, items: &Items) -> bool {
        let fuel = self.slots[FUEL]
            .as_ref()
            .is_some_and(|fuel| items.recipes.burn_ticks(fuel.id) > 0);
        !self.is_burning() && self.cook == 0 && !(fuel && self.can_smelt(items))
    }

    /// Whether the input smelts into something with room in the output.
    fn can_smelt(&self, items: &Items) -> bool {
        let Some(result) = self.slots[INPUT]
            .as_ref()
            .and_then(|input| items.recipes.smelt(input))
        else {
            return false;
        };
        match &self.slots[OUTPUT] {
            None => true,
            Some(output) => {
                output.stacks_with(&result)
                    && output.count + result.count <= items.max_stack(output.id)
            }
        }
    }

    /// One tick (1.0.0's `updateEntity`). Returns true when it lights or
    /// goes out, so the block can change.
    pub fn tick(&mut self, items: &Items) -> bool {
        let was_burning = self.is_burning();
        self.burn = self.burn.saturating_sub(1);
        if self.burn == 0 && self.can_smelt(items) {
            self.light(items);
        }
        if self.is_burning() && self.can_smelt(items) {
            self.cook += 1;
            if self.cook == COOK_TICKS {
                self.cook = 0;
                self.smelt(items);
            }
        } else {
            self.cook = 0;
        }
        was_burning != self.is_burning()
    }

    /// Burns one fuel item, leaving its container (a lava bucket's bucket).
    fn light(&mut self, items: &Items) {
        let Some(fuel) = self.slots[FUEL].as_mut() else {
            return;
        };
        let ticks = items.recipes.burn_ticks(fuel.id);
        if ticks == 0 {
            return;
        }
        self.burn = ticks;
        self.burn_total = ticks;
        fuel.count -= 1;
        if fuel.count == 0 {
            let leftover = items.item(fuel.id).and_then(|item| item.leftover);
            self.slots[FUEL] = leftover.map(|id| ItemStack::with_damage(id, 1, 0));
        }
    }

    /// Turns one input item into its result.
    fn smelt(&mut self, items: &Items) {
        let Some(input) = self.slots[INPUT].as_mut() else {
            return;
        };
        let Some(result) = items.recipes.smelt(input) else {
            return;
        };
        input.count -= 1;
        if input.count == 0 {
            self.slots[INPUT] = None;
        }
        match self.slots[OUTPUT].as_mut() {
            Some(output) => output.count += result.count,
            None => self.slots[OUTPUT] = Some(result),
        }
    }

    /// How much fuel is left (0–1), for the flame.
    pub fn fuel_left(&self) -> f32 {
        if self.burn_total == 0 {
            return 0.0;
        }
        self.burn as f32 / self.burn_total as f32
    }

    /// How far the input has cooked (0–1), for the arrow.
    pub fn progress(&self) -> f32 {
        self.cook as f32 / COOK_TICKS as f32
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::inventory::crafting::Recipes;
    use crate::inventory::items;

    fn items() -> Items {
        let recipes = Recipes::load(&crate::assets::dir().join("data/recipes.ron")).unwrap();
        crate::inventory::player::tests::items().with_recipes(Arc::new(recipes))
    }

    fn run(furnace: &mut Furnace, items: &Items, ticks: u32) -> u32 {
        (0..ticks).filter(|_| furnace.tick(items)).count() as u32
    }

    #[test]
    fn coal_smelts_eight_iron_ore_200_ticks_each() {
        let items = items();
        let mut furnace = Furnace::default();
        furnace.slots[INPUT] = Some(ItemStack::new(items::IRON_ORE, 10));
        furnace.slots[FUEL] = Some(ItemStack::new(items::COAL, 1));
        assert!(!furnace.is_idle(&items));
        assert_eq!(run(&mut furnace, &items, 1), 1, "lights");
        assert_eq!(furnace.slots[FUEL], None);
        run(&mut furnace, &items, 198);
        assert_eq!(furnace.slots[OUTPUT], None);
        run(&mut furnace, &items, 1);
        assert_eq!(
            furnace.slots[OUTPUT],
            Some(ItemStack::new(items::IRON_INGOT, 1))
        );
        // 1600 ticks of coal smelt 8; it goes out the tick after (fuel
        // lights on the first tick and counts down from the second).
        assert_eq!(run(&mut furnace, &items, 1400), 0);
        assert_eq!(run(&mut furnace, &items, 1), 1, "goes out");
        assert_eq!(
            furnace.slots[OUTPUT],
            Some(ItemStack::new(items::IRON_INGOT, 8))
        );
        assert_eq!(
            furnace.slots[INPUT],
            Some(ItemStack::new(items::IRON_ORE, 2))
        );
        assert!(!furnace.is_burning());
        assert!(furnace.is_idle(&items));
    }

    #[test]
    fn fuel_isnt_burned_without_something_to_smelt_or_room_for_it() {
        let items = items();
        let mut furnace = Furnace::default();
        furnace.slots[FUEL] = Some(ItemStack::new(items::OAK_PLANKS, 4));
        furnace.slots[INPUT] = Some(ItemStack::new(items::DIRT, 4));
        run(&mut furnace, &items, 10);
        assert_eq!(
            furnace.slots[FUEL],
            Some(ItemStack::new(items::OAK_PLANKS, 4)),
            "dirt doesn't smelt"
        );
        furnace.slots[INPUT] = Some(ItemStack::new(items::SAND, 4));
        furnace.slots[OUTPUT] = Some(ItemStack::new(items::GLASS, 64));
        run(&mut furnace, &items, 10);
        assert_eq!(
            furnace.slots[FUEL],
            Some(ItemStack::new(items::OAK_PLANKS, 4)),
            "the output is full"
        );
        assert!(furnace.is_idle(&items));
    }

    #[test]
    fn a_lava_bucket_leaves_its_bucket_and_logs_make_charcoal() {
        let items = items();
        let mut furnace = Furnace::default();
        furnace.slots[INPUT] = Some(ItemStack::with_damage(items::OAK_LOG, 1, 2));
        furnace.slots[FUEL] = Some(ItemStack::new(items::LAVA_BUCKET, 1));
        run(&mut furnace, &items, COOK_TICKS);
        assert_eq!(furnace.slots[FUEL], Some(ItemStack::new(items::BUCKET, 1)));
        assert_eq!(
            furnace.slots[OUTPUT],
            Some(ItemStack::new(items::CHARCOAL, 1))
        );
        assert!(furnace.is_burning(), "lava burns on");
        assert!((furnace.fuel_left() - 19801.0 / 20000.0).abs() < 1e-6);
        assert_eq!(furnace.progress(), 0.0);
    }
}
