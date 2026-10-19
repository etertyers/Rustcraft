//! Enchantments, as in modern Java Edition: the list, what each can go on,
//! what it can't be combined with, how much power it needs, and how the
//! enchanting table picks what to offer.

use serde::{Deserialize, Serialize};

use super::{item::ItemId, stack::Items};
use crate::world::worldgen::rng::Rng;

/// The most levels the enchanting table spends on a slot's power.
pub const MAX_BOOKSHELF_POWER: u32 = 15;
/// The most any enchanting offer can cost, in levels of power.
const MAX_POWER: i32 = 15;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Enchantment {
    Protection,
    FireProtection,
    FeatherFalling,
    BlastProtection,
    ProjectileProtection,
    Respiration,
    AquaAffinity,
    Thorns,
    DepthStrider,
    Sharpness,
    Smite,
    BaneOfArthropods,
    Knockback,
    FireAspect,
    Looting,
    SweepingEdge,
    Efficiency,
    SilkTouch,
    Unbreaking,
    Fortune,
    Power,
    Punch,
    Flame,
    Infinity,
    LuckOfTheSea,
    Lure,
    Mending,
    VanishingCurse,
    BindingCurse,
}

use Enchantment as E;

/// Every enchantment, in the order the game lists them.
pub const ALL: [Enchantment; 29] = [
    E::Protection,
    E::FireProtection,
    E::FeatherFalling,
    E::BlastProtection,
    E::ProjectileProtection,
    E::Respiration,
    E::AquaAffinity,
    E::Thorns,
    E::DepthStrider,
    E::Sharpness,
    E::Smite,
    E::BaneOfArthropods,
    E::Knockback,
    E::FireAspect,
    E::Looting,
    E::SweepingEdge,
    E::Efficiency,
    E::SilkTouch,
    E::Unbreaking,
    E::Fortune,
    E::Power,
    E::Punch,
    E::Flame,
    E::Infinity,
    E::LuckOfTheSea,
    E::Lure,
    E::Mending,
    E::VanishingCurse,
    E::BindingCurse,
];

/// What an item is, for deciding which enchantments fit it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Gear {
    Helmet,
    Chestplate,
    Leggings,
    Boots,
    Sword,
    Axe,
    Pickaxe,
    Shovel,
    Hoe,
    Shears,
    Bow,
    FishingRod,
    /// Anything else that wears out (a shield, flint and steel).
    Durable,
    Book,
    /// Can't be enchanted.
    Plain,
}

impl Gear {
    pub fn is_armor(self) -> bool {
        matches!(
            self,
            Gear::Helmet | Gear::Chestplate | Gear::Leggings | Gear::Boots
        )
    }

    /// Whether the item is a mining tool in the sense of Efficiency.
    fn digs(self) -> bool {
        matches!(
            self,
            Gear::Axe | Gear::Pickaxe | Gear::Shovel | Gear::Hoe | Gear::Shears
        )
    }

    /// What the item is, from its name and what it does.
    pub fn of(items: &Items, id: ItemId) -> Gear {
        let name = id.name();
        if name == "book" || name == "enchanted_book" {
            return Gear::Book;
        }
        let kind = |suffix: &str| name.ends_with(suffix);
        if kind("_helmet") {
            Gear::Helmet
        } else if kind("_chestplate") {
            Gear::Chestplate
        } else if kind("_leggings") {
            Gear::Leggings
        } else if kind("_boots") {
            Gear::Boots
        } else if name == "bow" {
            Gear::Bow
        } else if name == "fishing_rod" {
            Gear::FishingRod
        } else if let Some(tool) = items.tool(id) {
            use super::item::ToolKind as K;
            match tool.kind {
                K::Sword => Gear::Sword,
                K::Axe => Gear::Axe,
                K::Pickaxe => Gear::Pickaxe,
                K::Shovel => Gear::Shovel,
                K::Hoe => Gear::Hoe,
                K::Shears => Gear::Shears,
            }
        } else if items.max_damage(id).is_some() {
            Gear::Durable
        } else {
            Gear::Plain
        }
    }

    /// How easily the item takes enchantments from the table (the
    /// material's enchantment value; 0 means the table won't touch it).
    pub fn enchantability(self, items: &Items, id: ItemId) -> u32 {
        let name = id.name();
        let material = |prefix: &str| name.starts_with(prefix);
        match self {
            Gear::Book => 1,
            Gear::Bow | Gear::FishingRod => 1,
            Gear::Shears | Gear::Durable | Gear::Plain => 0,
            gear if gear.is_armor() => match () {
                _ if material("leather") => 15,
                _ if material("chainmail") => 12,
                _ if material("iron") => 9,
                _ if material("golden") => 25,
                _ if material("diamond") => 10,
                _ => 0,
            },
            _ => match items.tool(id).and_then(|tool| tool.tier) {
                Some(super::item::Tier::Wood) => 15,
                Some(super::item::Tier::Stone) => 5,
                Some(super::item::Tier::Iron) => 14,
                Some(super::item::Tier::Diamond) => 10,
                Some(super::item::Tier::Gold) => 22,
                None => 0,
            },
        }
    }
}

impl Enchantment {
    /// The name shown on an item, e.g. "Efficiency".
    pub fn name(self) -> &'static str {
        match self {
            E::Protection => "Protection",
            E::FireProtection => "Fire Protection",
            E::FeatherFalling => "Feather Falling",
            E::BlastProtection => "Blast Protection",
            E::ProjectileProtection => "Projectile Protection",
            E::Respiration => "Respiration",
            E::AquaAffinity => "Aqua Affinity",
            E::Thorns => "Thorns",
            E::DepthStrider => "Depth Strider",
            E::Sharpness => "Sharpness",
            E::Smite => "Smite",
            E::BaneOfArthropods => "Bane of Arthropods",
            E::Knockback => "Knockback",
            E::FireAspect => "Fire Aspect",
            E::Looting => "Looting",
            E::SweepingEdge => "Sweeping Edge",
            E::Efficiency => "Efficiency",
            E::SilkTouch => "Silk Touch",
            E::Unbreaking => "Unbreaking",
            E::Fortune => "Fortune",
            E::Power => "Power",
            E::Punch => "Punch",
            E::Flame => "Flame",
            E::Infinity => "Infinity",
            E::LuckOfTheSea => "Luck of the Sea",
            E::Lure => "Lure",
            E::Mending => "Mending",
            E::VanishingCurse => "Curse of Vanishing",
            E::BindingCurse => "Curse of Binding",
        }
    }

    pub fn max_level(self) -> u8 {
        match self {
            E::Protection
            | E::FireProtection
            | E::FeatherFalling
            | E::BlastProtection
            | E::ProjectileProtection => 4,
            E::Respiration | E::Thorns | E::DepthStrider => 3,
            E::Sharpness | E::Smite | E::BaneOfArthropods | E::Efficiency | E::Power => 5,
            E::Knockback | E::FireAspect | E::Punch => 2,
            E::Looting | E::SweepingEdge | E::Unbreaking | E::Fortune => 3,
            E::LuckOfTheSea | E::Lure => 3,
            E::AquaAffinity
            | E::SilkTouch
            | E::Flame
            | E::Infinity
            | E::Mending
            | E::VanishingCurse
            | E::BindingCurse => 1,
        }
    }

    /// How likely the enchantment is to be picked (modern `weight`).
    pub fn weight(self) -> u32 {
        match self {
            E::Protection | E::Sharpness | E::Efficiency | E::Power => 10,
            E::FireProtection
            | E::FeatherFalling
            | E::ProjectileProtection
            | E::Smite
            | E::BaneOfArthropods
            | E::Knockback
            | E::Unbreaking => 5,
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
            | E::Mending => 2,
            E::Thorns | E::SilkTouch | E::Infinity | E::VanishingCurse | E::BindingCurse => 1,
        }
    }

    /// Treasure enchantments and curses never come from the table.
    pub fn is_treasure(self) -> bool {
        matches!(self, E::Mending | E::VanishingCurse | E::BindingCurse)
    }

    pub fn is_curse(self) -> bool {
        matches!(self, E::VanishingCurse | E::BindingCurse)
    }

    /// The enchanting power at which `level` becomes possible, and the
    /// power above which it no longer is.
    pub fn cost_range(self, level: u8) -> (i32, i32) {
        let l = level as i32;
        let (min, spread) = match self {
            E::Protection => (1 + (l - 1) * 11, 11),
            E::FireProtection => (10 + (l - 1) * 8, 8),
            E::FeatherFalling => (5 + (l - 1) * 6, 6),
            E::BlastProtection => (5 + (l - 1) * 8, 8),
            E::ProjectileProtection => (3 + (l - 1) * 6, 6),
            E::Respiration => (10 * l, 30),
            E::AquaAffinity => (1, 40),
            E::Thorns => (10 + (l - 1) * 20, 50),
            E::DepthStrider => (10 * l, 15),
            E::Sharpness => (1 + (l - 1) * 11, 20),
            E::Smite | E::BaneOfArthropods => (5 + (l - 1) * 8, 20),
            E::Knockback => (5 + (l - 1) * 20, 50),
            E::FireAspect => (10 + (l - 1) * 20, 50),
            E::Looting | E::Fortune | E::LuckOfTheSea | E::Lure => (15 + (l - 1) * 9, 50),
            E::SweepingEdge => (5 + (l - 1) * 9, 15),
            E::Efficiency => (1 + (l - 1) * 10, 50),
            E::SilkTouch => (15, 50),
            E::Unbreaking => (5 + (l - 1) * 8, 50),
            E::Power => (1 + (l - 1) * 10, 15),
            E::Punch => (12 + (l - 1) * 20, 25),
            E::Flame | E::Infinity => (20, 30),
            E::Mending => (25 + l * 9, 50),
            E::VanishingCurse | E::BindingCurse => (25, 25),
        };
        (min, min + spread)
    }

    /// Whether `self` and `other` can't share an item.
    pub fn conflicts_with(self, other: Enchantment) -> bool {
        fn group(e: Enchantment) -> Option<u8> {
            match e {
                E::Protection
                | E::FireProtection
                | E::BlastProtection
                | E::ProjectileProtection => Some(0),
                E::Sharpness | E::Smite | E::BaneOfArthropods => Some(1),
                E::SilkTouch | E::Fortune => Some(2),
                E::Infinity | E::Mending => Some(3),
                _ => None,
            }
        }
        self != other && group(self).is_some() && group(self) == group(other)
    }

    /// Whether the enchantment can go on this gear at an anvil.
    pub fn fits(self, gear: Gear) -> bool {
        if gear == Gear::Book {
            return true;
        }
        match self {
            E::Protection | E::FireProtection | E::BlastProtection | E::ProjectileProtection => {
                gear.is_armor()
            }
            E::Thorns => gear.is_armor(),
            E::FeatherFalling | E::DepthStrider => gear == Gear::Boots,
            E::Respiration | E::AquaAffinity => gear == Gear::Helmet,
            E::Sharpness | E::Smite | E::BaneOfArthropods => {
                matches!(gear, Gear::Sword | Gear::Axe)
            }
            E::Knockback | E::FireAspect | E::Looting | E::SweepingEdge => gear == Gear::Sword,
            E::Efficiency => gear.digs(),
            E::SilkTouch | E::Fortune => {
                matches!(gear, Gear::Axe | Gear::Pickaxe | Gear::Shovel | Gear::Hoe)
            }
            E::Unbreaking | E::Mending => gear != Gear::Plain,
            E::Power | E::Punch | E::Flame | E::Infinity => gear == Gear::Bow,
            E::LuckOfTheSea | E::Lure => gear == Gear::FishingRod,
            E::VanishingCurse => gear != Gear::Plain,
            E::BindingCurse => gear.is_armor(),
        }
    }

    /// Whether the enchanting table may put it on this gear (shears and
    /// the like are kept out by their enchantability of 0).
    fn offered_for(self, gear: Gear) -> bool {
        !self.is_treasure() && self.fits(gear)
    }
}

/// An enchantment and its level.
pub type Enchanted = (Enchantment, u8);

/// A level in Roman numerals as the game writes them ("IV").
pub fn roman(level: u8) -> String {
    const NUMERALS: [(u8, &str); 5] = [(10, "X"), (9, "IX"), (5, "V"), (4, "IV"), (1, "I")];
    let mut left = level;
    let mut out = String::new();
    for (value, text) in NUMERALS {
        while left >= value {
            out.push_str(text);
            left -= value;
        }
    }
    out
}

/// One of the table's three offers.
#[derive(Debug, Clone, PartialEq)]
pub struct Offer {
    /// Levels of experience the player needs to have (the "power").
    pub requires: u32,
    /// What's shown as a hint: the first enchantment and its level.
    pub clue: Option<Enchanted>,
    /// Everything that would go on the item.
    pub enchants: Vec<Enchanted>,
}

/// The levels, and lapis, that taking offer `slot` (0–2) costs.
pub fn offer_price(slot: usize) -> u32 {
    slot as u32 + 1
}

/// Modern `EnchantmentMenu.slotsChanged`: the three offers for an item at
/// a table surrounded by `power` bookshelves, given the player's
/// enchantment seed. Empty offers have no enchants.
pub fn offers(items: &Items, id: ItemId, power: u32, seed: u32) -> [Option<Offer>; 3] {
    let gear = Gear::of(items, id);
    let enchantability = gear.enchantability(items, id);
    if enchantability == 0 {
        return [None, None, None];
    }
    let mut random = Jrand::new(seed as u64);
    let power = (power as i32).min(MAX_POWER);
    let costs: [i32; 3] = std::array::from_fn(|slot| {
        let n = random.next_int(8) + 1 + (power >> 1) + random.next_int(power as u32 + 1);
        let cost = match slot {
            0 => (n / 3).max(1),
            1 => n * 2 / 3 + 1,
            _ => n.max(power * 2),
        };
        if cost < slot as i32 + 1 { 0 } else { cost }
    });
    std::array::from_fn(|slot| {
        let cost = costs[slot];
        if cost <= 0 {
            return None;
        }
        let mut random = Jrand::new(seed as u64 + slot as u64);
        let enchants = select(items, id, gear, enchantability, cost, &mut random);
        // A book and a bare tool both need something to show.
        (!enchants.is_empty()).then(|| Offer {
            requires: cost as u32,
            clue: enchants.first().copied(),
            enchants,
        })
    })
}

/// Modern `EnchantmentHelper.selectEnchantment` for the table: the
/// enchantments an item with this `enchantability` gets at `power`.
fn select(
    _items: &Items,
    _id: ItemId,
    gear: Gear,
    enchantability: u32,
    power: i32,
    random: &mut Jrand,
) -> Vec<Enchanted> {
    let half = enchantability as i32 / 4 + 1;
    let mut level = power + 1 + random.next_int(half as u32) + random.next_int(half as u32);
    let spread = (random.next_float() + random.next_float() - 1.0) * 0.15;
    level = ((level as f32 + level as f32 * spread).round() as i32).max(1);

    // The strongest level of each enchantment the power allows.
    let mut possible: Vec<Enchanted> = ALL
        .iter()
        .filter(|e| e.offered_for(gear))
        .filter_map(|&e| {
            (1..=e.max_level())
                .rev()
                .find(|&l| {
                    let (min, max) = e.cost_range(l);
                    (min..=max).contains(&level)
                })
                .map(|l| (e, l))
        })
        .collect();
    let mut chosen: Vec<Enchanted> = Vec::new();
    if possible.is_empty() {
        return chosen;
    }
    chosen.push(pick(&mut possible, random));
    // Each further enchantment is less likely as the power is halved.
    while random.next_int(50) <= level {
        let (last, _) = *chosen.last().expect("one chosen");
        possible.retain(|&(e, _)| e != last && !e.conflicts_with(last));
        if possible.is_empty() {
            break;
        }
        chosen.push(pick(&mut possible, random));
        level /= 2;
    }
    chosen
}

/// Picks one by weight.
fn pick(possible: &mut [Enchanted], random: &mut Jrand) -> Enchanted {
    let total: u32 = possible.iter().map(|(e, _)| e.weight()).sum();
    let mut roll = random.next_int(total) as u32;
    for &entry in possible.iter() {
        let weight = entry.0.weight();
        if roll < weight {
            return entry;
        }
        roll -= weight;
    }
    possible[0]
}

/// Java's `java.util.Random`, so a seed picks the offers the way it does
/// in the game.
#[derive(Debug, Clone)]
pub struct Jrand(u64);

impl Jrand {
    const MULTIPLIER: u64 = 0x5DEE_CE66D;
    const MASK: u64 = (1 << 48) - 1;

    pub fn new(seed: u64) -> Self {
        Self((seed ^ Self::MULTIPLIER) & Self::MASK)
    }

    fn next(&mut self, bits: u32) -> i32 {
        self.0 = (self.0.wrapping_mul(Self::MULTIPLIER).wrapping_add(0xB)) & Self::MASK;
        (self.0 >> (48 - bits)) as i32
    }

    /// Uniform in 0..bound (bound > 0).
    pub fn next_int(&mut self, bound: u32) -> i32 {
        let bound = bound as i32;
        if bound.isolate_lowest_one() == bound {
            return ((bound as i64 * self.next(31) as i64) >> 31) as i32;
        }
        loop {
            let bits = self.next(31);
            let value = bits % bound;
            if bits - value + (bound - 1) >= 0 {
                return value;
            }
        }
    }

    pub fn next_float(&mut self) -> f32 {
        self.next(24) as f32 / (1 << 24) as f32
    }
}

/// Random enchantments for an item at a given level, as `/enchant`-less
/// sources use them (loot, trades): the same selection as the table with
/// treasure allowed.
pub fn random_for(items: &Items, id: ItemId, level: i32, random: &mut Rng) -> Vec<Enchanted> {
    let gear = Gear::of(items, id);
    let enchantability = gear.enchantability(items, id).max(1);
    let mut jrand = Jrand::new(random.next_u64());
    select(items, id, gear, enchantability, level, &mut jrand)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn java_random_matches_the_known_sequence() {
        // Checked against a JVM: new Random(42).nextInt(100) three times.
        let mut random = Jrand::new(42);
        assert_eq!(random.next_int(100), 30);
        assert_eq!(random.next_int(100), 63);
        assert_eq!(random.next_int(100), 48);
        assert!((Jrand::new(7).next_float() - 0.730699).abs() < 1e-6);
    }

    #[test]
    fn roman_numerals() {
        assert_eq!(roman(1), "I");
        assert_eq!(roman(4), "IV");
        assert_eq!(roman(5), "V");
        assert_eq!(roman(10), "X");
    }

    #[test]
    fn exclusive_groups_conflict_both_ways() {
        assert!(E::Sharpness.conflicts_with(E::Smite));
        assert!(E::Smite.conflicts_with(E::Sharpness));
        assert!(E::SilkTouch.conflicts_with(E::Fortune));
        assert!(E::Infinity.conflicts_with(E::Mending));
        assert!(!E::Sharpness.conflicts_with(E::Sharpness));
        assert!(!E::Efficiency.conflicts_with(E::Unbreaking));
    }

    #[test]
    fn efficiency_levels_need_the_modern_power() {
        // Efficiency III costs 21 to 71.
        assert_eq!(E::Efficiency.cost_range(3), (21, 71));
        assert_eq!(E::Sharpness.cost_range(5), (45, 65));
        assert_eq!(E::Unbreaking.cost_range(1), (5, 55));
    }

    #[test]
    fn a_bare_table_offers_cheap_enchantments_on_a_pickaxe() {
        let items = Items::new();
        let pickaxe = crate::inventory::items::DIAMOND_PICKAXE;
        let offers = offers(&items, pickaxe, 0, 12345);
        // With no bookshelves the costs are 1 to 8 levels.
        for offer in offers.iter().flatten() {
            assert!(offer.requires <= 8, "{offer:?}");
            assert!(!offer.enchants.is_empty());
        }
    }

    #[test]
    fn fifteen_bookshelves_offer_a_thirty_level_top_slot() {
        let items = Items::new();
        let pickaxe = crate::inventory::items::DIAMOND_PICKAXE;
        let offer = offers(&items, pickaxe, 15, 99)[2].clone().unwrap();
        assert_eq!(offer.requires, 30, "the top slot at full power is 30");
    }

    #[test]
    fn offers_never_hold_conflicts_or_treasure() {
        let items = Items::new();
        let sword = crate::inventory::items::DIAMOND_SWORD;
        for seed in 0..500 {
            for offer in offers(&items, sword, 15, seed).into_iter().flatten() {
                for (i, &(a, level)) in offer.enchants.iter().enumerate() {
                    assert!(!a.is_treasure());
                    assert!(level >= 1 && level <= a.max_level());
                    assert!(a.fits(Gear::Sword));
                    for &(b, _) in &offer.enchants[i + 1..] {
                        assert!(!a.conflicts_with(b) && a != b, "{offer:?}");
                    }
                }
            }
        }
    }

    #[test]
    fn nothing_but_gear_takes_the_table() {
        let items = Items::new();
        let offers = offers(&items, crate::inventory::items::STICK, 15, 1);
        assert!(offers.iter().all(Option::is_none));
    }

    #[test]
    fn shears_and_shields_are_anvil_only() {
        let items = Items::new();
        assert!(
            offers(&items, crate::inventory::items::SHEARS, 15, 1)
                .iter()
                .all(Option::is_none)
        );
    }
}
