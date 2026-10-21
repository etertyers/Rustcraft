//! A brewing stand's slots and brewing (M26, modern
//! `BrewingStandBlockEntity`): three bottles, an ingredient on top and
//! blaze powder to burn. A brew takes 20 seconds; a blaze powder lasts 20
//! brews.

use serde::{Deserialize, Serialize};

use super::{items, potion, stack::ItemStack};

pub const BOTTLES: [usize; 3] = [0, 1, 2];
pub const INGREDIENT: usize = 3;
pub const FUEL: usize = 4;
pub const SLOTS: usize = 5;
/// Ticks a brew takes, and brews a blaze powder is good for.
pub const BREW_TICKS: u16 = 400;
pub const BREWS_PER_FUEL: u8 = 20;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BrewingStand {
    pub slots: Vec<Option<ItemStack>>,
    /// Ticks left of the brew under way (0 when idle).
    pub brew_time: u16,
    /// Brews left from the blaze powder burnt.
    pub fuel: u8,
}

impl Default for BrewingStand {
    fn default() -> Self {
        Self {
            slots: vec![None; SLOTS],
            brew_time: 0,
            fuel: 0,
        }
    }
}

impl BrewingStand {
    /// Whether the ingredient brews any of the bottles into something.
    pub fn can_brew(&self) -> bool {
        let Some(ingredient) = &self.slots[INGREDIENT] else {
            return false;
        };
        BOTTLES.iter().any(|&i| {
            self.slots[i]
                .as_ref()
                .is_some_and(|bottle| potion::brew(bottle, ingredient.id).is_some())
        })
    }

    /// Whether a brew is under way or could start.
    pub fn is_idle(&self) -> bool {
        self.brew_time == 0 && !(self.can_brew() && (self.fuel > 0 || self.has_powder()))
    }

    fn has_powder(&self) -> bool {
        self.slots[FUEL]
            .as_ref()
            .is_some_and(|s| s.id == items::BLAZE_POWDER)
    }

    /// One tick: burn a blaze powder when out of fuel, count a brew down,
    /// and finish it (every bottle brewed, one ingredient used up). A brew
    /// whose ingredient is taken away stops. Returns whether it finished.
    pub fn tick(&mut self) -> bool {
        if self.fuel == 0 && self.has_powder() {
            self.fuel = BREWS_PER_FUEL;
            take_one(&mut self.slots[FUEL]);
        }
        let brewable = self.can_brew();
        if self.brew_time > 0 {
            self.brew_time -= 1;
            if !brewable {
                self.brew_time = 0;
            } else if self.brew_time == 0 {
                self.finish();
                return true;
            }
        } else if brewable && self.fuel > 0 {
            self.fuel -= 1;
            self.brew_time = BREW_TICKS;
        }
        false
    }

    fn finish(&mut self) {
        let Some(ingredient) = self.slots[INGREDIENT].clone() else {
            return;
        };
        for i in BOTTLES {
            if let Some(made) = self.slots[i]
                .as_ref()
                .and_then(|bottle| potion::brew(bottle, ingredient.id))
            {
                self.slots[i] = Some(made);
            }
        }
        take_one(&mut self.slots[INGREDIENT]);
    }

    /// The brew's progress, 0–1, and the fuel left, 0–1, for the screen.
    pub fn progress(&self) -> (f32, f32) {
        let brewing = if self.brew_time == 0 {
            0.0
        } else {
            1.0 - f32::from(self.brew_time) / f32::from(BREW_TICKS)
        };
        (brewing, f32::from(self.fuel) / f32::from(BREWS_PER_FUEL))
    }

    /// Which bottle slots hold something (the stand's `has_bottle_N`).
    pub fn bottles(&self) -> [bool; 3] {
        BOTTLES.map(|i| self.slots[i].is_some())
    }
}

fn take_one(slot: &mut Option<ItemStack>) {
    if let Some(stack) = slot.as_mut() {
        stack.count -= 1;
        if stack.count == 0 {
            *slot = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn water_and_nether_wart_brew_three_awkward_potions_in_twenty_seconds() {
        let mut stand = BrewingStand::default();
        for i in BOTTLES {
            stand.slots[i] = Some(potion::stack(items::POTION, "water"));
        }
        stand.slots[INGREDIENT] = Some(ItemStack::new(items::NETHER_WART, 2));
        stand.slots[FUEL] = Some(ItemStack::new(items::BLAZE_POWDER, 1));
        let finished = (1..=BREW_TICKS + 1).find(|_| stand.tick());
        assert_eq!(
            finished,
            Some(BREW_TICKS + 1),
            "a tick to start, 400 to brew"
        );
        for i in BOTTLES {
            assert_eq!(
                potion::of(stand.slots[i].as_ref().unwrap()).unwrap().id,
                "awkward"
            );
        }
        assert_eq!(stand.slots[INGREDIENT].as_ref().unwrap().count, 1);
        assert!(stand.slots[FUEL].is_none(), "the powder burnt");
        assert_eq!(stand.fuel, BREWS_PER_FUEL - 1);
        // Taking the ingredient away stops a brew.
        stand.slots[INGREDIENT] = Some(ItemStack::new(items::MAGMA_CREAM, 1));
        stand.tick();
        assert!(stand.brew_time > 0);
        stand.slots[INGREDIENT] = None;
        stand.tick();
        assert_eq!(stand.brew_time, 0);
    }

    #[test]
    fn no_fuel_no_brew() {
        let mut stand = BrewingStand::default();
        stand.slots[0] = Some(potion::stack(items::POTION, "water"));
        stand.slots[INGREDIENT] = Some(ItemStack::new(items::NETHER_WART, 1));
        for _ in 0..500 {
            stand.tick();
        }
        assert_eq!(
            potion::of(stand.slots[0].as_ref().unwrap()).unwrap().id,
            "water"
        );
        assert!(stand.is_idle());
    }
}
