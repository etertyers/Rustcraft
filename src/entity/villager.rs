//! What a villager is besides a mob: its profession and level, its job
//! site and bed, and its trades, after modern Java Edition's
//! `VillagerTrades` (cut to the items Rustcraft has, and without
//! reputation-based prices).

use glam::{DVec3, IVec3};
use serde::{Deserialize, Serialize};

use crate::{
    inventory::{
        enchant::{self, ALL, Enchantment},
        item::ItemId,
        items,
        stack::{ItemStack, Items},
    },
    world::{block::BlockId, blocks, worldgen::rng::Rng},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Profession {
    Farmer,
    Librarian,
    Armorer,
    Toolsmith,
    Weaponsmith,
    Cleric,
    Butcher,
}

impl Profession {
    pub const ALL: [Profession; 7] = [
        Profession::Farmer,
        Profession::Librarian,
        Profession::Armorer,
        Profession::Toolsmith,
        Profession::Weaponsmith,
        Profession::Cleric,
        Profession::Butcher,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Profession::Farmer => "farmer",
            Profession::Librarian => "librarian",
            Profession::Armorer => "armorer",
            Profession::Toolsmith => "toolsmith",
            Profession::Weaponsmith => "weaponsmith",
            Profession::Cleric => "cleric",
            Profession::Butcher => "butcher",
        }
    }

    pub fn named(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|p| p.name() == name)
    }

    /// The block a villager of this profession works at.
    pub fn job_site(self) -> BlockId {
        match self {
            Profession::Farmer => blocks::COMPOSTER,
            Profession::Librarian => blocks::LECTERN,
            Profession::Armorer => blocks::BLAST_FURNACE,
            Profession::Toolsmith => blocks::SMITHING_TABLE,
            Profession::Weaponsmith => blocks::GRINDSTONE,
            Profession::Cleric => blocks::BREWING_STAND,
            Profession::Butcher => blocks::SMOKER,
        }
    }

    /// The profession whose job site `block` is.
    pub fn of_site(block: BlockId) -> Option<Self> {
        Self::ALL.into_iter().find(|p| p.job_site() == block)
    }
}

/// Novice, apprentice, journeyman, expert and master.
pub const LEVEL_NAMES: [&str; 5] = ["Novice", "Apprentice", "Journeyman", "Expert", "Master"];
/// Villager experience needed to reach each level (modern).
pub const LEVEL_XP: [u32; 5] = [0, 10, 70, 150, 250];
/// Times a day a villager restocks at its job site.
pub const RESTOCKS_PER_DAY: u8 = 2;

/// One trade: what the villager wants and what it gives.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Trade {
    pub buy: ItemStack,
    #[serde(default)]
    pub buy_b: Option<ItemStack>,
    pub sell: ItemStack,
    pub uses: u32,
    pub max_uses: u32,
    /// Villager experience each trade gives.
    pub xp: u32,
}

impl Trade {
    pub fn out_of_stock(&self) -> bool {
        self.uses >= self.max_uses
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct VillagerData {
    pub profession: Option<Profession>,
    /// 1 (novice) to 5 (master).
    pub level: u8,
    pub xp: u32,
    pub trades: Vec<Trade>,
    pub job_site: Option<[i32; 3]>,
    pub bed: Option<[i32; 3]>,
    /// The day number of the last restock and how many that day.
    pub restock_day: u64,
    pub restocks: u8,
    /// Food eaten toward breeding (bread is 4).
    pub food: u32,
    /// Ticks until it can breed again, and (for a baby) until grown.
    pub breed_cooldown: u32,
    pub baby: u32,
    /// Where it's headed, worked out by the game from the time of day.
    #[serde(skip)]
    pub errand: Option<DVec3>,
    /// Ticks until it looks round for a job site and a bed again.
    #[serde(skip)]
    pub scan: u32,
}

impl VillagerData {
    pub fn job_site(&self) -> Option<IVec3> {
        self.job_site.map(IVec3::from_array)
    }

    pub fn bed(&self) -> Option<IVec3> {
        self.bed.map(IVec3::from_array)
    }

    pub fn level_name(&self) -> &'static str {
        LEVEL_NAMES[usize::from(self.level.clamp(1, 5)) - 1]
    }

    /// Takes on `profession` at level 1 with its first two trades.
    pub fn employ(&mut self, profession: Profession, items: &Items, rng: &mut Rng) {
        self.profession = Some(profession);
        self.level = 1;
        self.xp = 0;
        self.trades.clear();
        self.unlock(items, rng);
    }

    /// Adds the trades of the current level.
    fn unlock(&mut self, items: &Items, rng: &mut Rng) {
        let Some(profession) = self.profession else {
            return;
        };
        let mut pool = offers(profession, self.level, items, rng);
        // Two of the level's offers (a master has one or two).
        let count = if self.level == 5 {
            1 + rng.below(2) as usize
        } else {
            2
        };
        for _ in 0..count.min(pool.len()) {
            let index = rng.below(pool.len() as u32) as usize;
            self.trades.push(pool.remove(index));
        }
    }

    /// The trade numbered `index` was made: experience, and the next level
    /// once there's enough. Returns whether it levelled up.
    pub fn traded(&mut self, index: usize, items: &Items, rng: &mut Rng) -> bool {
        let Some(trade) = self.trades.get_mut(index) else {
            return false;
        };
        trade.uses += 1;
        self.xp += trade.xp;
        if self.level < 5 && self.xp >= LEVEL_XP[usize::from(self.level)] {
            self.level += 1;
            self.unlock(items, rng);
            return true;
        }
        false
    }

    /// Fills every trade back up, at most twice a day.
    pub fn restock(&mut self, day: u64) -> bool {
        if day != self.restock_day {
            self.restock_day = day;
            self.restocks = 0;
        }
        if self.restocks >= RESTOCKS_PER_DAY || self.trades.iter().all(|t| t.uses == 0) {
            return false;
        }
        self.restocks += 1;
        for trade in &mut self.trades {
            trade.uses = 0;
        }
        true
    }

    /// Whether it has traded, which keeps its profession for good.
    pub fn experienced(&self) -> bool {
        self.xp > 0
    }

    /// Forgets its job (its job site is gone, and it never traded).
    pub fn unemploy(&mut self) {
        self.profession = None;
        self.level = 0;
        self.trades.clear();
        self.job_site = None;
    }
}

fn stack(name: &str, count: u8) -> ItemStack {
    ItemStack::new(items::by_name(name).unwrap_or(items::EMERALD), count)
}

fn emeralds(count: u8) -> ItemStack {
    ItemStack::new(items::EMERALD, count)
}

/// Buying `count` of `item` for an emerald.
fn buys(item: &str, count: u8, max_uses: u32, xp: u32) -> Trade {
    Trade {
        buy: stack(item, count),
        buy_b: None,
        sell: emeralds(1),
        uses: 0,
        max_uses,
        xp,
    }
}

/// Selling `count` of `item` for `price` emeralds.
fn sells(price: u8, item: &str, count: u8, max_uses: u32, xp: u32) -> Trade {
    Trade {
        buy: emeralds(price),
        buy_b: None,
        sell: stack(item, count),
        uses: 0,
        max_uses,
        xp,
    }
}

/// Selling `item` enchanted at `power`, for its base price plus the
/// enchanting power (modern `EnchantedItemForEmeralds`).
fn sells_enchanted(
    base: u8,
    item: &str,
    power: i32,
    items: &Items,
    rng: &mut Rng,
    xp: u32,
) -> Trade {
    let mut sell = stack(item, 1);
    let level = 5 + rng.below(power as u32 - 4) as i32;
    for (e, l) in enchant::random_for(items, sell.id, level, rng) {
        sell.enchant(e, l);
    }
    let price = (u32::from(base) + level as u32).min(64) as u8;
    Trade {
        buy: emeralds(price),
        buy_b: None,
        sell,
        uses: 0,
        max_uses: 3,
        xp,
    }
}

/// A librarian's enchanted book: any enchantment at a random level, for a
/// book and 2 + rand(5 + 10 × level) + 3 × level emeralds (doubled for
/// treasure), at most 64 (modern `EnchantBookForEmeralds`).
fn book(rng: &mut Rng, xp: u32) -> Trade {
    let enchantment: Enchantment = ALL[rng.below(ALL.len() as u32) as usize];
    let level = 1 + rng.below(u32::from(enchantment.max_level())) as u8;
    let mut price = 2 + rng.below(5 + u32::from(level) * 10) + 3 * u32::from(level);
    if enchantment.is_treasure() {
        price *= 2;
    }
    let mut sell = ItemStack::new(items::ENCHANTED_BOOK, 1);
    sell.enchant(enchantment, level);
    Trade {
        buy: emeralds(price.min(64) as u8),
        buy_b: Some(ItemStack::new(items::BOOK, 1)),
        sell,
        uses: 0,
        max_uses: 12,
        xp,
    }
}

/// Everything a villager of `profession` may offer at `level`.
pub fn offers(profession: Profession, level: u8, items: &Items, rng: &mut Rng) -> Vec<Trade> {
    use Profession::*;
    match (profession, level) {
        (Farmer, 1) => vec![
            buys("wheat", 20, 16, 2),
            sells(1, "bread", 6, 16, 1),
            buys("melon_slice", 16, 16, 2),
        ],
        (Farmer, 2) => vec![sells(1, "apple", 4, 16, 5), buys("melon", 4, 12, 10)],
        (Farmer, 3) => vec![
            sells(3, "cookie", 18, 12, 10),
            buys("sugar_cane", 24, 12, 20),
        ],
        (Farmer, 4) => vec![sells(1, "cake", 1, 12, 15), buys("egg", 16, 12, 20)],
        (Farmer, 5) => vec![
            sells(4, "glistering_melon_slice", 3, 12, 30),
            sells(3, "golden_apple", 1, 12, 30),
        ],
        (Librarian, 1) => vec![
            buys("paper", 24, 16, 2),
            sells(9, "bookshelf", 1, 12, 1),
            book(rng, 1),
        ],
        (Librarian, 2) => vec![
            buys("book", 4, 12, 10),
            sells(1, "lantern", 1, 12, 5),
            book(rng, 5),
        ],
        (Librarian, 3) => vec![
            buys("ink_sac", 5, 12, 20),
            sells(1, "glass", 4, 12, 10),
            book(rng, 10),
        ],
        (Librarian, 4) => vec![
            sells(5, "clock", 1, 12, 15),
            sells(4, "compass", 1, 12, 15),
            book(rng, 15),
        ],
        (Librarian, 5) => vec![book(rng, 30), book(rng, 30)],
        (Armorer, 1) => vec![
            buys("coal", 15, 16, 2),
            sells(7, "iron_leggings", 1, 12, 1),
            sells(4, "iron_boots", 1, 12, 1),
            sells(5, "iron_helmet", 1, 12, 1),
            sells(9, "iron_chestplate", 1, 12, 1),
        ],
        (Armorer, 2) => vec![
            buys("iron_ingot", 4, 12, 10),
            sells(36, "bell", 1, 12, 5),
            sells(1, "chainmail_boots", 1, 12, 5),
            sells(3, "chainmail_leggings", 1, 12, 5),
        ],
        (Armorer, 3) => vec![
            buys("lava_bucket", 1, 12, 20),
            buys("diamond", 1, 12, 20),
            sells(1, "chainmail_helmet", 1, 12, 10),
            sells(4, "chainmail_chestplate", 1, 12, 10),
            sells(5, "shield", 1, 12, 10),
        ],
        (Armorer, 4) => vec![
            sells_enchanted(14, "diamond_leggings", 19, items, rng, 15),
            sells_enchanted(8, "diamond_boots", 19, items, rng, 15),
        ],
        (Armorer, 5) => vec![
            sells_enchanted(8, "diamond_helmet", 19, items, rng, 30),
            sells_enchanted(16, "diamond_chestplate", 19, items, rng, 30),
        ],
        (Toolsmith, 1) => vec![
            buys("coal", 15, 16, 2),
            sells(1, "stone_axe", 1, 12, 1),
            sells(1, "stone_shovel", 1, 12, 1),
            sells(1, "stone_pickaxe", 1, 12, 1),
            sells(1, "stone_hoe", 1, 12, 1),
        ],
        (Toolsmith, 2) => vec![buys("iron_ingot", 4, 12, 10), sells(36, "bell", 1, 12, 5)],
        (Toolsmith, 3) => vec![
            buys("flint", 30, 12, 20),
            sells_enchanted(1, "iron_axe", 19, items, rng, 10),
            sells_enchanted(2, "iron_shovel", 19, items, rng, 10),
            sells_enchanted(3, "iron_pickaxe", 19, items, rng, 10),
            sells(4, "diamond_hoe", 1, 3, 10),
        ],
        (Toolsmith, 4) => vec![
            buys("diamond", 1, 12, 30),
            sells_enchanted(12, "diamond_axe", 19, items, rng, 15),
            sells_enchanted(5, "diamond_shovel", 19, items, rng, 15),
        ],
        (Toolsmith, 5) => vec![sells_enchanted(13, "diamond_pickaxe", 19, items, rng, 30)],
        (Weaponsmith, 1) => vec![
            buys("coal", 15, 16, 2),
            sells(3, "iron_axe", 1, 12, 1),
            sells_enchanted(2, "iron_sword", 19, items, rng, 1),
        ],
        (Weaponsmith, 2) => vec![buys("iron_ingot", 4, 12, 10), sells(36, "bell", 1, 12, 5)],
        (Weaponsmith, 3) => vec![buys("flint", 24, 12, 20)],
        (Weaponsmith, 4) => vec![
            buys("diamond", 1, 12, 30),
            sells_enchanted(12, "diamond_axe", 19, items, rng, 15),
        ],
        (Weaponsmith, 5) => vec![sells_enchanted(8, "diamond_sword", 19, items, rng, 30)],
        (Cleric, 1) => vec![
            buys("rotten_flesh", 32, 16, 2),
            sells(1, "redstone", 2, 12, 1),
        ],
        (Cleric, 2) => vec![
            buys("gold_ingot", 3, 12, 10),
            sells(1, "lapis_lazuli", 1, 12, 5),
        ],
        (Cleric, 3) => vec![
            buys("rabbit_foot", 2, 12, 20),
            sells(4, "glowstone", 1, 12, 10),
        ],
        (Cleric, 4) => vec![
            buys("glass_bottle", 9, 12, 30),
            sells(5, "ender_pearl", 1, 12, 15),
        ],
        (Cleric, 5) => vec![
            sells(3, "glowstone_dust", 4, 12, 30),
            sells(5, "ender_pearl", 1, 12, 30),
        ],
        (Butcher, 1) => vec![
            buys("chicken", 14, 16, 2),
            buys("porkchop", 7, 16, 2),
            buys("rabbit", 4, 16, 2),
        ],
        (Butcher, 2) => vec![
            buys("coal", 15, 16, 2),
            sells(1, "cooked_porkchop", 5, 16, 5),
            sells(1, "cooked_chicken", 8, 16, 5),
        ],
        (Butcher, 3) => vec![buys("mutton", 7, 16, 20), buys("beef", 10, 16, 20)],
        (Butcher, 4) => vec![
            sells(1, "cooked_mutton", 6, 12, 15),
            sells(1, "cooked_beef", 5, 12, 15),
        ],
        (Butcher, 5) => vec![sells(1, "cooked_rabbit", 4, 12, 30)],
        _ => Vec::new(),
    }
}

/// The item a trade's stack names, for checking tables.
pub fn trade_items(trade: &Trade) -> Vec<ItemId> {
    let mut ids = vec![trade.buy.id, trade.sell.id];
    ids.extend(trade.buy_b.as_ref().map(|s| s.id));
    ids
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_offer_names_real_items_for_every_profession_and_level() {
        let items = Items::new();
        let mut rng = Rng::new(5);
        for profession in Profession::ALL {
            for level in 1..=5 {
                let offers = offers(profession, level, &items, &mut rng);
                assert!(!offers.is_empty(), "{profession:?} {level}");
                for trade in offers {
                    // `stack` falls back to an emerald for an unknown name,
                    // which would show as emeralds for emeralds.
                    assert!(
                        trade.buy.id != items::EMERALD || trade.sell.id != items::EMERALD,
                        "{profession:?} {level}: {trade:?}"
                    );
                    assert!(trade.buy.count >= 1 && trade.buy.count <= 64);
                }
            }
        }
    }

    #[test]
    fn a_villager_levels_up_to_master_unlocking_trades() {
        let items = Items::new();
        let mut rng = Rng::new(1);
        let mut villager = VillagerData::default();
        villager.employ(Profession::Toolsmith, &items, &mut rng);
        assert_eq!((villager.level, villager.trades.len()), (1, 2));
        let mut levels = vec![1];
        for _ in 0..500 {
            let index = villager
                .trades
                .iter()
                .position(|t| !t.out_of_stock())
                .unwrap_or_else(|| {
                    villager.restock_day += 1;
                    villager.restock(villager.restock_day + 1);
                    0
                });
            if villager.traded(index, &items, &mut rng) {
                levels.push(villager.level);
            }
            if villager.level == 5 {
                break;
            }
        }
        assert_eq!(levels, vec![1, 2, 3, 4, 5]);
        assert_eq!(villager.level_name(), "Master");
        assert!(villager.trades.len() >= 9, "{}", villager.trades.len());
    }

    #[test]
    fn restocking_refills_at_most_twice_a_day() {
        let items = Items::new();
        let mut rng = Rng::new(2);
        let mut villager = VillagerData::default();
        villager.employ(Profession::Farmer, &items, &mut rng);
        villager.trades[0].uses = villager.trades[0].max_uses;
        assert!(villager.trades[0].out_of_stock());
        assert!(villager.restock(10));
        assert_eq!(villager.trades[0].uses, 0);
        villager.trades[0].uses = 3;
        assert!(villager.restock(10));
        villager.trades[0].uses = 3;
        assert!(!villager.restock(10), "a third time the same day");
        assert!(villager.restock(11), "the next day");
    }

    #[test]
    fn library_books_cost_a_book_and_emeralds() {
        let mut rng = Rng::new(3);
        for _ in 0..200 {
            let trade = book(&mut rng, 1);
            assert_eq!(trade.buy_b.as_ref().map(|s| s.id), Some(items::BOOK));
            assert!((2..=64).contains(&trade.buy.count));
            assert_eq!(trade.sell.enchantments().len(), 1);
        }
    }

    #[test]
    fn job_sites_name_their_professions() {
        for profession in Profession::ALL {
            assert_eq!(Profession::of_site(profession.job_site()), Some(profession));
            assert_eq!(Profession::named(profession.name()), Some(profession));
        }
        assert_eq!(Profession::of_site(blocks::STONE), None);
    }
}
