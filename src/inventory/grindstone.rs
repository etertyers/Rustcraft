//! What a grindstone makes: an item with its enchantments (the curses
//! stay) taken off, two of an item joined into one, and the experience
//! that gives (modern `GrindstoneMenu`).

use super::{
    enchant::Enchantment,
    items,
    stack::{ItemStack, Items},
};
use crate::world::worldgen::rng::Rng;

/// Of the second item's durability the join keeps this much more, as a
/// share of the maximum.
const JOIN_BONUS_PERCENT: i32 = 5;

/// What the grindstone produces, and the experience points it gives.
#[derive(Debug, Clone, PartialEq)]
pub struct GrindResult {
    pub output: ItemStack,
    /// The least experience the output pays; see `experience`.
    removed_cost: i32,
}

impl GrindResult {
    /// Points paid out on taking the output: half the cost of what was
    /// removed (rounded up), plus up to as much again at random.
    pub fn experience(&self, rng: &mut Rng) -> u32 {
        if self.removed_cost <= 0 {
            return 0;
        }
        let half = (self.removed_cost as f64 / 2.0).ceil() as u32;
        half + rng.below(half)
    }
}

/// Strips the removable enchantments from `stack`, returning their
/// total minimum cost.
fn strip(stack: &mut ItemStack) -> i32 {
    let kept: Vec<_> = stack
        .enchantments()
        .iter()
        .filter(|(e, _)| e.is_curse())
        .copied()
        .collect();
    let removed: i32 = stack
        .enchantments()
        .iter()
        .filter(|(e, _)| !e.is_curse())
        .map(|&(e, level): &(Enchantment, u8)| e.cost_range(level).0)
        .sum();
    stack.clear_enchantments();
    for (e, level) in kept {
        stack.enchant(e, level);
    }
    stack.set_work(0);
    removed
}

/// The result of the two slots, if the grindstone does anything.
pub fn grind(
    items: &Items,
    top: Option<&ItemStack>,
    bottom: Option<&ItemStack>,
) -> Option<GrindResult> {
    match (top, bottom) {
        (None, None) => None,
        (Some(only), None) | (None, Some(only)) => {
            if !only.is_enchanted() && only.work() == 0 {
                return None;
            }
            let mut output = only.clone();
            let removed_cost = strip(&mut output);
            // A book with nothing left to give is a plain book.
            if output.id.name() == "enchanted_book" && !output.is_enchanted() {
                output = ItemStack::new(items::BOOK, 1);
            }
            Some(GrindResult {
                output,
                removed_cost,
            })
        }
        (Some(a), Some(b)) => {
            if a.id != b.id || a.count != 1 || b.count != 1 {
                return None;
            }
            let max = i32::from(items.max_damage(a.id)?);
            let first = max - i32::from(a.damage);
            let second = max - i32::from(b.damage);
            let bonus = max * JOIN_BONUS_PERCENT / 100;
            let remaining = (max - (first + second + bonus)).max(0);
            let mut output = a.clone();
            output.damage = remaining as u16;
            let mut removed_cost = strip(&mut output);
            let mut other = b.clone();
            removed_cost += strip(&mut other);
            // The curses of both come along.
            for &(e, level) in other.enchantments() {
                if output.level_of(e) < level {
                    output.enchant(e, level);
                }
            }
            Some(GrindResult {
                output,
                removed_cost,
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pickaxe(damage: u16, enchants: &[(Enchantment, u8)]) -> ItemStack {
        let mut stack = ItemStack::with_damage(items::DIAMOND_PICKAXE, 1, damage);
        for &(e, l) in enchants {
            stack.enchant(e, l);
        }
        stack
    }

    #[test]
    fn disenchanting_keeps_curses_and_pays_for_the_rest() {
        let items = Items::new();
        let worn = pickaxe(
            0,
            &[
                (Enchantment::Efficiency, 5),
                (Enchantment::VanishingCurse, 1),
            ],
        );
        let made = grind(&items, Some(&worn), None).unwrap();
        assert_eq!(made.output.level_of(Enchantment::Efficiency), 0);
        assert_eq!(made.output.level_of(Enchantment::VanishingCurse), 1);
        // Efficiency V needs 41 at least, so 21 plus up to 21 more.
        let mut rng = Rng::new(1);
        for _ in 0..100 {
            let xp = made.experience(&mut rng);
            assert!((21..42).contains(&xp), "{xp}");
        }
    }

    #[test]
    fn a_plain_item_does_nothing_and_a_book_becomes_a_book() {
        let items = Items::new();
        assert_eq!(grind(&items, Some(&pickaxe(0, &[])), None), None);
        let mut book = ItemStack::new(items::ENCHANTED_BOOK, 1);
        book.enchant(Enchantment::Mending, 1);
        let made = grind(&items, None, Some(&book)).unwrap();
        assert_eq!(made.output, ItemStack::new(items::BOOK, 1));
    }

    #[test]
    fn two_worn_items_join_with_a_five_percent_bonus() {
        let items = Items::new();
        let max = items.max_damage(items::DIAMOND_PICKAXE).unwrap() as i32;
        let a = pickaxe(1000, &[(Enchantment::Unbreaking, 3)]);
        let b = pickaxe(1200, &[]);
        let made = grind(&items, Some(&a), Some(&b)).unwrap();
        let left = (max - 1000) + (max - 1200) + max * 5 / 100;
        assert_eq!(i32::from(made.output.damage), (max - left).max(0));
        assert!(!made.output.is_enchanted());
        // Different items don't join.
        let shovel = ItemStack::new(items::DIAMOND_SHOVEL, 1);
        assert_eq!(grind(&items, Some(&a), Some(&shovel)), None);
    }
}
