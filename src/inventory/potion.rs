//! Potions (M26, modern `Potions` and `PotionBrewing`): what each one
//! gives, and what brewing an ingredient into it makes.
//!
//! A potion, splash potion or water bottle is the item `potion` or
//! `splash_potion` with the potion's number in its stack's damage (as
//! Minecraft 1.8 did), so saves need nothing new. The numbers are the
//! places in `POTIONS` and only ever grow at the end.

use super::{item::ItemId, items, stack::ItemStack};
use crate::player::effects::{Active, Effect};

/// One kind of potion: its name (modern `minecraft:long_swiftness`, less
/// the namespace), how it's shown, and its effects (amplifier, ticks).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Potion {
    pub id: &'static str,
    pub name: &'static str,
    pub effects: &'static [(Effect, u8, u32)],
    /// The 1.12 picture of the filled bottle (`potion_bottle_<icon>`).
    pub icon: &'static str,
}

const fn p(
    id: &'static str,
    name: &'static str,
    icon: &'static str,
    effects: &'static [(Effect, u8, u32)],
) -> Potion {
    Potion {
        id,
        name,
        effects,
        icon,
    }
}

/// Minutes of ticks.
const fn m(minutes: u32, seconds: u32) -> u32 {
    (minutes * 60 + seconds) * 20
}

use Effect::*;

/// Every potion, by number. Durations are modern's.
pub const POTIONS: [Potion; 37] = [
    p("water", "Water Bottle", "drinkable", &[]),
    p("mundane", "Mundane Potion", "drinkable", &[]),
    p("thick", "Thick Potion", "drinkable", &[]),
    p("awkward", "Awkward Potion", "drinkable", &[]),
    p(
        "night_vision",
        "Potion of Night Vision",
        "nightVision",
        &[(NightVision, 0, m(3, 0))],
    ),
    p(
        "long_night_vision",
        "Potion of Night Vision",
        "nightVision",
        &[(NightVision, 0, m(8, 0))],
    ),
    p(
        "invisibility",
        "Potion of Invisibility",
        "invisibility",
        &[(Invisibility, 0, m(3, 0))],
    ),
    p(
        "long_invisibility",
        "Potion of Invisibility",
        "invisibility",
        &[(Invisibility, 0, m(8, 0))],
    ),
    p(
        "leaping",
        "Potion of Leaping",
        "jump",
        &[(JumpBoost, 0, m(3, 0))],
    ),
    p(
        "long_leaping",
        "Potion of Leaping",
        "jump",
        &[(JumpBoost, 0, m(8, 0))],
    ),
    p(
        "strong_leaping",
        "Potion of Leaping",
        "jump",
        &[(JumpBoost, 1, m(1, 30))],
    ),
    p(
        "fire_resistance",
        "Potion of Fire Resistance",
        "fireResistance",
        &[(FireResistance, 0, m(3, 0))],
    ),
    p(
        "long_fire_resistance",
        "Potion of Fire Resistance",
        "fireResistance",
        &[(FireResistance, 0, m(8, 0))],
    ),
    p(
        "swiftness",
        "Potion of Swiftness",
        "moveSpeed",
        &[(Speed, 0, m(3, 0))],
    ),
    p(
        "long_swiftness",
        "Potion of Swiftness",
        "moveSpeed",
        &[(Speed, 0, m(8, 0))],
    ),
    p(
        "strong_swiftness",
        "Potion of Swiftness",
        "moveSpeed",
        &[(Speed, 1, m(1, 30))],
    ),
    p(
        "slowness",
        "Potion of Slowness",
        "moveSlowdown",
        &[(Slowness, 0, m(1, 30))],
    ),
    p(
        "long_slowness",
        "Potion of Slowness",
        "moveSlowdown",
        &[(Slowness, 0, m(4, 0))],
    ),
    p(
        "strong_slowness",
        "Potion of Slowness",
        "moveSlowdown",
        &[(Slowness, 3, m(0, 20))],
    ),
    p(
        "water_breathing",
        "Potion of Water Breathing",
        "waterBreathing",
        &[(WaterBreathing, 0, m(3, 0))],
    ),
    p(
        "long_water_breathing",
        "Potion of Water Breathing",
        "waterBreathing",
        &[(WaterBreathing, 0, m(8, 0))],
    ),
    p(
        "healing",
        "Potion of Healing",
        "heal",
        &[(InstantHealth, 0, 1)],
    ),
    p(
        "strong_healing",
        "Potion of Healing",
        "heal",
        &[(InstantHealth, 1, 1)],
    ),
    p(
        "harming",
        "Potion of Harming",
        "harm",
        &[(InstantDamage, 0, 1)],
    ),
    p(
        "strong_harming",
        "Potion of Harming",
        "harm",
        &[(InstantDamage, 1, 1)],
    ),
    p(
        "poison",
        "Potion of Poison",
        "poison",
        &[(Poison, 0, m(0, 45))],
    ),
    p(
        "long_poison",
        "Potion of Poison",
        "poison",
        &[(Poison, 0, m(1, 30))],
    ),
    p(
        "strong_poison",
        "Potion of Poison",
        "poison",
        &[(Poison, 1, m(0, 21) + 12)],
    ),
    p(
        "regeneration",
        "Potion of Regeneration",
        "regeneration",
        &[(Regeneration, 0, m(0, 45))],
    ),
    p(
        "long_regeneration",
        "Potion of Regeneration",
        "regeneration",
        &[(Regeneration, 0, m(1, 30))],
    ),
    p(
        "strong_regeneration",
        "Potion of Regeneration",
        "regeneration",
        &[(Regeneration, 1, m(0, 22) + 10)],
    ),
    p(
        "strength",
        "Potion of Strength",
        "damageBoost",
        &[(Strength, 0, m(3, 0))],
    ),
    p(
        "long_strength",
        "Potion of Strength",
        "damageBoost",
        &[(Strength, 0, m(8, 0))],
    ),
    p(
        "strong_strength",
        "Potion of Strength",
        "damageBoost",
        &[(Strength, 1, m(1, 30))],
    ),
    p(
        "weakness",
        "Potion of Weakness",
        "weakness",
        &[(Weakness, 0, m(1, 30))],
    ),
    p(
        "long_weakness",
        "Potion of Weakness",
        "weakness",
        &[(Weakness, 0, m(4, 0))],
    ),
    p(
        "slow_falling",
        "Potion of Slow Falling",
        "slowFall",
        &[(SlowFalling, 0, m(1, 30))],
    ),
];

/// The potion numbered `n`.
pub fn get(n: u16) -> Option<&'static Potion> {
    POTIONS.get(n as usize)
}

/// The number of the potion called `id`.
pub fn number(id: &str) -> Option<u16> {
    POTIONS.iter().position(|p| p.id == id).map(|n| n as u16)
}

/// A potion item (`potion` or `splash_potion`) holding potion `id`.
pub fn stack(item: ItemId, id: &str) -> ItemStack {
    ItemStack::with_damage(item, 1, number(id).unwrap_or(0))
}

/// Whether `id` is a potion item, which keeps its potion in its damage.
pub fn is_potion(id: ItemId) -> bool {
    matches!(id, items::POTION | items::SPLASH_POTION)
}

/// The potion a potion stack holds.
pub fn of(stack: &ItemStack) -> Option<&'static Potion> {
    is_potion(stack.id).then(|| get(stack.damage)).flatten()
}

/// The effects drinking or splashing `potion` gives.
pub fn effects(potion: &Potion) -> Vec<Active> {
    potion
        .effects
        .iter()
        .map(|&(effect, amplifier, duration)| Active::new(effect, amplifier, duration))
        .collect()
}

/// What brewing `ingredient` into potion `from` makes (modern
/// `PotionBrewing`'s mixes, less turtle master, whose turtles aren't
/// here).
fn mix(from: &str, ingredient: &str) -> Option<&'static str> {
    Some(match (from, ingredient) {
        ("water", "nether_wart") => "awkward",
        ("water", "glowstone_dust") => "thick",
        ("water", "redstone") => "mundane",
        (
            "water",
            "sugar"
            | "rabbit_foot"
            | "glistering_melon_slice"
            | "spider_eye"
            | "pufferfish"
            | "magma_cream"
            | "ghast_tear"
            | "blaze_powder",
        ) => "mundane",
        ("water", "fermented_spider_eye") => "weakness",
        ("awkward", "golden_carrot") => "night_vision",
        ("awkward", "magma_cream") => "fire_resistance",
        ("awkward", "rabbit_foot") => "leaping",
        ("awkward", "sugar") => "swiftness",
        ("awkward", "pufferfish") => "water_breathing",
        ("awkward", "glistering_melon_slice") => "healing",
        ("awkward", "spider_eye") => "poison",
        ("awkward", "ghast_tear") => "regeneration",
        ("awkward", "blaze_powder") => "strength",
        ("awkward", "phantom_membrane") => "slow_falling",
        ("awkward", "fermented_spider_eye") => "weakness",
        // Fermented spider eyes turn potions bad.
        ("night_vision", "fermented_spider_eye") => "invisibility",
        ("long_night_vision", "fermented_spider_eye") => "long_invisibility",
        ("swiftness" | "leaping", "fermented_spider_eye") => "slowness",
        ("long_swiftness" | "long_leaping", "fermented_spider_eye") => "long_slowness",
        ("healing" | "poison" | "long_poison", "fermented_spider_eye") => "harming",
        ("strong_healing" | "strong_poison", "fermented_spider_eye") => "strong_harming",
        // Redstone makes them last, glowstone makes them stronger.
        (base, "redstone") => long(base)?,
        (base, "glowstone_dust") => strong(base)?,
        _ => return None,
    })
}

fn long(base: &str) -> Option<&'static str> {
    let id = format!("long_{base}");
    POTIONS.iter().find(|p| p.id == id).map(|p| p.id)
}

fn strong(base: &str) -> Option<&'static str> {
    let id = format!("strong_{base}");
    POTIONS.iter().find(|p| p.id == id).map(|p| p.id)
}

/// What a brewing stand makes of `bottle` with `ingredient`: the potion
/// changed, or gunpowder making it a splash potion.
pub fn brew(bottle: &ItemStack, ingredient: ItemId) -> Option<ItemStack> {
    let potion = of(bottle)?;
    if ingredient == items::GUNPOWDER {
        return (bottle.id == items::POTION)
            .then(|| ItemStack::with_damage(items::SPLASH_POTION, 1, bottle.damage));
    }
    let made = mix(potion.id, ingredient.name())?;
    Some(stack(bottle.id, made))
}

/// Whether `ingredient` brews into anything at all (what the stand's top
/// slot takes).
pub fn is_ingredient(ingredient: ItemId) -> bool {
    ingredient == items::GUNPOWDER
        || POTIONS
            .iter()
            .any(|p| mix(p.id, ingredient.name()).is_some())
}

/// How the stack is called: "Potion of Fire Resistance", "Splash Potion
/// of Healing", "Water Bottle".
pub fn display_name(stack: &ItemStack) -> Option<String> {
    let potion = of(stack)?;
    Some(if stack.id == items::SPLASH_POTION {
        match potion.name.strip_prefix("Potion") {
            Some(rest) => format!("Splash Potion{rest}"),
            None => format!("Splash {}", potion.name),
        }
    } else {
        potion.name.to_owned()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn named(id: &str) -> ItemStack {
        stack(items::POTION, id)
    }

    fn brews(from: &str, ingredient: &str) -> Option<String> {
        let made = brew(&named(from), items::by_name(ingredient).unwrap())?;
        Some(of(&made)?.id.to_owned())
    }

    #[test]
    fn the_modern_tree_from_water_to_fire_resistance_and_beyond() {
        assert_eq!(brews("water", "nether_wart").as_deref(), Some("awkward"));
        assert_eq!(
            brews("awkward", "magma_cream").as_deref(),
            Some("fire_resistance")
        );
        assert_eq!(
            brews("fire_resistance", "redstone").as_deref(),
            Some("long_fire_resistance")
        );
        assert_eq!(
            brews("fire_resistance", "glowstone_dust"),
            None,
            "no strong fire resistance"
        );
        assert_eq!(
            brews("swiftness", "glowstone_dust").as_deref(),
            Some("strong_swiftness")
        );
        assert_eq!(
            brews("night_vision", "fermented_spider_eye").as_deref(),
            Some("invisibility")
        );
        assert_eq!(
            brews("strong_healing", "fermented_spider_eye").as_deref(),
            Some("strong_harming")
        );
        assert_eq!(brews("water", "sugar").as_deref(), Some("mundane"));
        assert_eq!(brews("water", "glowstone_dust").as_deref(), Some("thick"));
        assert_eq!(brews("awkward", "dirt"), None);
        // Gunpowder: the same potion, to throw.
        let splash = brew(&named("healing"), items::GUNPOWDER).unwrap();
        assert_eq!(splash.id, items::SPLASH_POTION);
        assert_eq!(of(&splash).unwrap().id, "healing");
        assert!(brew(&splash, items::GUNPOWDER).is_none());
        assert_eq!(display_name(&splash).unwrap(), "Splash Potion of Healing");
    }

    #[test]
    fn every_ingredient_exists_and_every_long_or_strong_potion_has_a_base() {
        for p in POTIONS {
            for prefix in ["long_", "strong_"] {
                if let Some(base) = p.id.strip_prefix(prefix) {
                    assert!(number(base).is_some(), "{}", p.id);
                }
            }
        }
        for ingredient in [
            "nether_wart",
            "golden_carrot",
            "magma_cream",
            "rabbit_foot",
            "sugar",
            "pufferfish",
            "glistering_melon_slice",
            "spider_eye",
            "ghast_tear",
            "blaze_powder",
            "phantom_membrane",
            "fermented_spider_eye",
            "redstone",
            "glowstone_dust",
            "gunpowder",
        ] {
            let id = items::by_name(ingredient).unwrap_or_else(|| panic!("{ingredient}"));
            assert!(is_ingredient(id), "{ingredient}");
        }
        assert!(!is_ingredient(items::DIRT));
    }
}
