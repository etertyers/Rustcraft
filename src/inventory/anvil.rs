//! What an anvil makes from its two inputs, following modern Java
//! Edition's `AnvilMenu.createResult`: repairing with the material,
//! combining two of an item, merging enchantments, renaming, and the
//! growing "prior work" cost that ends in "Too Expensive!".

use super::{
    enchant::{Enchanted, Enchantment, Gear},
    item::ItemId,
    stack::{ItemStack, Items},
};

/// From this cost up the anvil refuses outside creative mode.
pub const TOO_EXPENSIVE: u32 = 40;

/// What putting two stacks in an anvil makes.
#[derive(Debug, Clone, PartialEq)]
pub struct AnvilResult {
    pub output: ItemStack,
    /// Levels of experience it costs.
    pub cost: u32,
    /// How many of the right-hand item taking the output uses up (all of
    /// them unless it's a repair material).
    pub right_used: u8,
}

/// Why there's no result.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refusal {
    /// Nothing to do with these two.
    Nothing,
    /// It would cost 40 levels or more.
    TooExpensive,
}

/// Levels a prior-work count adds (modern `RepairCost`: 2ⁿ − 1).
pub fn work_cost(work: u8) -> u32 {
    (1u32 << work.min(30)) - 1
}

/// What an enchantment adds to the cost per level (modern `anvil_cost`).
fn anvil_cost(enchantment: Enchantment) -> u32 {
    use Enchantment as E;
    match enchantment {
        E::Protection | E::Sharpness | E::Efficiency | E::Power => 1,
        E::FireProtection
        | E::FeatherFalling
        | E::ProjectileProtection
        | E::Smite
        | E::BaneOfArthropods
        | E::Knockback
        | E::Unbreaking => 2,
        E::BlastProtection
        | E::Respiration
        | E::AquaAffinity
        | E::DepthStrider
        | E::FireAspect
        | E::Looting
        | E::SweepingEdge
        | E::Fortune
        | E::Punch
        | E::Flame
        | E::LuckOfTheSea
        | E::Lure
        | E::Mending => 4,
        E::Thorns | E::SilkTouch | E::Infinity | E::VanishingCurse | E::BindingCurse => 8,
    }
}

/// Whether `material` repairs `tool` (the anvil's material repair).
pub fn repairs(tool: ItemId, material: ItemId) -> bool {
    let (tool, material) = (tool.name(), material.name());
    let made_of = |prefix: &str| tool.starts_with(prefix);
    if tool == "shield" {
        return material.ends_with("_planks");
    }
    if tool == "shears" {
        return material == "iron_ingot";
    }
    if made_of("wooden_") {
        material.ends_with("_planks")
    } else if made_of("stone_") {
        matches!(material, "cobblestone" | "cobbled_deepslate" | "blackstone")
    } else if made_of("iron_") || made_of("chainmail_") {
        material == "iron_ingot"
    } else if made_of("golden_") {
        material == "gold_ingot"
    } else if made_of("diamond_") {
        material == "diamond"
    } else if made_of("netherite_") {
        material == "netherite_ingot"
    } else if made_of("leather_") {
        material == "leather"
    } else {
        false
    }
}

/// The anvil's work on `left` and `right` with `name` typed in. In
/// creative mode (`free`) nothing is too expensive.
pub fn combine(
    items: &Items,
    left: &ItemStack,
    right: Option<&ItemStack>,
    name: &str,
    free: bool,
) -> Result<AnvilResult, Refusal> {
    let mut result = left.clone();
    let max_damage = items.max_damage(left.id);
    let left_gear = Gear::of(items, left.id);
    let mut cost = 0u32;
    let mut base = work_cost(left.work());
    let mut enchants: Vec<Enchanted> = left.enchantments().to_vec();
    let mut right_used = 1;
    let mut changed_something = false;

    if let Some(right) = right {
        base += work_cost(right.work());
        let right_is_book = right.id.name() == "enchanted_book" && right.is_enchanted();
        if let Some(max) = max_damage
            && !right_is_book
            && repairs(left.id, right.id)
        {
            // Repair with the material: a quarter of the item at a time.
            let step = |damage: u16| damage.min(max / 4);
            let mut amount = step(result.damage);
            if amount == 0 {
                return Err(Refusal::Nothing);
            }
            let mut used = 0;
            while amount > 0 && used < right.count {
                result.damage -= amount;
                cost += 1;
                amount = step(result.damage);
                used += 1;
            }
            right_used = used;
            changed_something = true;
        } else {
            if !right_is_book && (right.id != left.id || max_damage.is_none()) {
                return Err(Refusal::Nothing);
            }
            if let Some(max) = max_damage
                && !right_is_book
            {
                // Two of the same item: the second's durability and a
                // bonus go into the first.
                let max = i32::from(max);
                let first = max - i32::from(left.damage);
                let second = max - i32::from(right.damage) + max * 12 / 100;
                let remaining = (max - (first + second)).max(0);
                if remaining < i32::from(result.damage) {
                    result.damage = remaining as u16;
                    cost += 2;
                    changed_something = true;
                }
            }
            let mut any_fit = false;
            let mut conflict = false;
            for &(enchantment, from_right) in right.enchantments() {
                let on_left = enchants
                    .iter()
                    .find(|(e, _)| *e == enchantment)
                    .map_or(0, |&(_, level)| level);
                let mut level = if on_left == from_right {
                    from_right + 1
                } else {
                    from_right.max(on_left)
                };
                let mut fits = free || left_gear == Gear::Book || enchantment.fits(left_gear);
                for &(other, _) in &enchants {
                    if other != enchantment && enchantment.conflicts_with(other) {
                        fits = false;
                        cost += 1;
                    }
                }
                if !fits {
                    conflict = true;
                    continue;
                }
                any_fit = true;
                level = level.min(enchantment.max_level());
                match enchants.iter_mut().find(|(e, _)| *e == enchantment) {
                    Some(entry) => entry.1 = level,
                    None => enchants.push((enchantment, level)),
                }
                let mut per_level = anvil_cost(enchantment);
                if right_is_book {
                    per_level = (per_level / 2).max(1);
                }
                cost += per_level * u32::from(level);
                if left.count > 1 {
                    cost = 40;
                }
            }
            if conflict && !any_fit && !changed_something {
                return Err(Refusal::Nothing);
            }
            changed_something |= any_fit;
        }
    }

    let mut rename_only = 0;
    if name.trim().is_empty() {
        if left.custom_name().is_some() {
            rename_only = 1;
            cost += 1;
            result.set_custom_name(None);
        }
    } else if left.custom_name() != Some(name) {
        rename_only = 1;
        cost += 1;
        result.set_custom_name(Some(name.to_owned()));
    }
    if !changed_something && rename_only == 0 {
        return Err(Refusal::Nothing);
    }
    // Renaming alone never costs 40, however worked the item is.
    let mut total = base + cost;
    if rename_only > 0 && cost == rename_only && total >= TOO_EXPENSIVE {
        total = TOO_EXPENSIVE - 1;
    }
    if total >= TOO_EXPENSIVE && !free {
        return Err(Refusal::TooExpensive);
    }

    // The item remembers one more round of work, unless this was only a
    // rename.
    let mut work = left.work().max(right.map_or(0, ItemStack::work));
    if !(rename_only > 0 && cost == rename_only) {
        work = work.saturating_add(1);
    }
    result.set_work(work);
    result.clear_enchantments();
    for (enchantment, level) in enchants {
        result.enchant(enchantment, level);
    }
    result.count = 1;
    Ok(AnvilResult {
        output: result,
        cost: total,
        right_used,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::inventory::items;

    fn items() -> Items {
        Items::new()
    }

    fn book(enchants: &[(Enchantment, u8)]) -> ItemStack {
        let mut stack = ItemStack::new(items::ENCHANTED_BOOK, 1);
        for &(e, l) in enchants {
            stack.enchant(e, l);
        }
        stack
    }

    fn pickaxe(damage: u16) -> ItemStack {
        ItemStack::with_damage(items::DIAMOND_PICKAXE, 1, damage)
    }

    #[test]
    fn prior_work_costs_double_each_time() {
        assert_eq!(
            (0..7).map(work_cost).collect::<Vec<_>>(),
            [0, 1, 3, 7, 15, 31, 63]
        );
    }

    #[test]
    fn two_books_of_the_same_level_make_the_next_level() {
        let items = items();
        let a = book(&[(Enchantment::Efficiency, 3)]);
        let b = book(&[(Enchantment::Efficiency, 3)]);
        let made = combine(&items, &a, Some(&b), "", false).unwrap();
        assert_eq!(made.output.level_of(Enchantment::Efficiency), 4);
        // Efficiency costs 1 a level, halved for a book (at least 1): 4.
        // Plus no prior work.
        assert_eq!(made.cost, 4);
        assert_eq!(made.output.work(), 1);
    }

    #[test]
    fn levels_cap_and_unequal_levels_keep_the_higher() {
        let items = items();
        let five = book(&[(Enchantment::Efficiency, 5)]);
        let made = combine(&items, &five, Some(&five), "", false).unwrap();
        assert_eq!(made.output.level_of(Enchantment::Efficiency), 5);
        let two = book(&[(Enchantment::Unbreaking, 2)]);
        let one = book(&[(Enchantment::Unbreaking, 1)]);
        let made = combine(&items, &two, Some(&one), "", false).unwrap();
        assert_eq!(made.output.level_of(Enchantment::Unbreaking), 2);
    }

    #[test]
    fn a_book_enchants_a_pickaxe_and_costs_by_the_enchantment() {
        let items = items();
        let made = combine(
            &items,
            &pickaxe(0),
            Some(&book(&[(Enchantment::Efficiency, 5)])),
            "",
            false,
        )
        .unwrap();
        assert_eq!(made.output.level_of(Enchantment::Efficiency), 5);
        // 1 per level, halved to 1 (minimum) for a book: 5.
        assert_eq!(made.cost, 5);
        // Mending (4, halved to 2) once.
        let made = combine(
            &items,
            &pickaxe(0),
            Some(&book(&[(Enchantment::Mending, 1)])),
            "",
            false,
        )
        .unwrap();
        assert_eq!(made.cost, 2);
    }

    #[test]
    fn things_that_do_not_fit_are_refused() {
        let items = items();
        // Power is for bows.
        assert_eq!(
            combine(
                &items,
                &pickaxe(0),
                Some(&book(&[(Enchantment::Power, 1)])),
                "",
                false
            ),
            Err(Refusal::Nothing)
        );
        // Silk Touch can't go over Fortune; the conflict costs a level
        // but an anvil with nothing else to do refuses.
        let mut fortunate = pickaxe(0);
        fortunate.enchant(Enchantment::Fortune, 3);
        assert_eq!(
            combine(
                &items,
                &fortunate,
                Some(&book(&[(Enchantment::SilkTouch, 1)])),
                "",
                false
            ),
            Err(Refusal::Nothing)
        );
        // A pickaxe and a shovel don't combine.
        assert_eq!(
            combine(
                &items,
                &pickaxe(0),
                Some(&ItemStack::new(items::DIAMOND_SHOVEL, 1)),
                "",
                false
            ),
            Err(Refusal::Nothing)
        );
    }

    #[test]
    fn diamonds_repair_a_quarter_at_a_time() {
        let items = items();
        let max = items.max_damage(items::DIAMOND_PICKAXE).unwrap();
        let worn = pickaxe(max - 10);
        let made = combine(
            &items,
            &worn,
            Some(&ItemStack::new(items::DIAMOND, 3)),
            "",
            false,
        )
        .unwrap();
        // Each diamond takes a quarter of the maximum off the damage.
        assert_eq!(made.output.damage, max - 10 - 3 * (max / 4));
        assert_eq!((made.right_used, made.cost), (3, 3));
    }

    #[test]
    fn two_worn_pickaxes_merge_with_a_bonus() {
        let items = items();
        let max = items.max_damage(items::DIAMOND_PICKAXE).unwrap();
        let made = combine(&items, &pickaxe(1000), Some(&pickaxe(1000)), "", false).unwrap();
        // Each has max − 1000 left; 12% of max is added.
        let left = i32::from(max) - 1000;
        let expected = i32::from(max) - (left + left + i32::from(max) * 12 / 100);
        assert_eq!(i32::from(made.output.damage), expected.max(0));
        assert_eq!(made.cost, 2);
    }

    #[test]
    fn renaming_costs_a_level_and_never_too_much() {
        let items = items();
        let made = combine(&items, &pickaxe(0), None, "Digger", false).unwrap();
        assert_eq!(made.output.custom_name(), Some("Digger"));
        assert_eq!(made.cost, 1);
        assert_eq!(made.output.work(), 0, "renaming adds no prior work");
        // Same name again changes nothing.
        let named = made.output;
        assert_eq!(
            combine(&items, &named, None, "Digger", false),
            Err(Refusal::Nothing)
        );
        // Clearing the name costs a level too.
        let cleared = combine(&items, &named, None, "", false).unwrap();
        assert_eq!(cleared.output.custom_name(), None);
        // A heavily worked item renames for 39 at most.
        let mut worked = pickaxe(0);
        worked.set_work(10);
        let made = combine(&items, &worked, None, "Heavy", false).unwrap();
        assert_eq!(made.cost, 39);
    }

    #[test]
    fn work_piles_up_into_too_expensive() {
        let items = items();
        let mut tool = pickaxe(0);
        tool.set_work(6); // 63 levels of prior work
        let mending = book(&[(Enchantment::Mending, 1)]);
        assert_eq!(
            combine(&items, &tool, Some(&mending), "", false),
            Err(Refusal::TooExpensive)
        );
        // Creative mode doesn't care.
        assert!(combine(&items, &tool, Some(&mending), "", true).is_ok());
        // One fewer round and it's fine: 31 + 2.
        tool.set_work(5);
        let made = combine(&items, &tool, Some(&mending), "", false).unwrap();
        assert_eq!(made.cost, 31 + 2);
        assert_eq!(made.output.work(), 6);
    }

    #[test]
    fn stacks_of_more_than_one_cannot_take_enchantments() {
        let items = items();
        let planks = ItemStack::new(items::OAK_PLANKS, 4);
        assert_eq!(
            combine(&items, &planks, None, "", false),
            Err(Refusal::Nothing)
        );
    }
}
