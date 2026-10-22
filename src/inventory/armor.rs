//! Armor and wings (M28): what each worn piece protects against, derived
//! from the item's name, and the elytra, which takes the chest slot.
//!
//! Modern `CombatRules`: a hit of `damage` against `defense` points with
//! `toughness` is cut to `damage × (1 − min(20, max(defense / 5, defense −
//! damage / (2 + toughness / 4))) / 25)`.

use super::player::{ARMOR_CHEST, ARMOR_FEET, ARMOR_HEAD, ARMOR_LEGS};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArmorSlot {
    Feet,
    Legs,
    Chest,
    Head,
}

impl ArmorSlot {
    /// The inventory slot a piece for this place goes in.
    pub fn index(self) -> usize {
        match self {
            ArmorSlot::Feet => ARMOR_FEET,
            ArmorSlot::Legs => ARMOR_LEGS,
            ArmorSlot::Chest => ARMOR_CHEST,
            ArmorSlot::Head => ARMOR_HEAD,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Armor {
    pub slot: ArmorSlot,
    pub defense: f32,
    pub toughness: f32,
    /// The elytra: wings in the chest slot.
    pub glider: bool,
}

/// (helmet, chestplate, leggings, boots) points and toughness, by material.
const MATERIALS: [(&str, [f32; 4], f32); 6] = [
    ("leather_", [1.0, 3.0, 2.0, 1.0], 0.0),
    ("chainmail_", [2.0, 5.0, 4.0, 1.0], 0.0),
    ("iron_", [2.0, 6.0, 5.0, 2.0], 0.0),
    ("golden_", [2.0, 5.0, 3.0, 1.0], 0.0),
    ("diamond_", [3.0, 8.0, 6.0, 3.0], 2.0),
    ("netherite_", [3.0, 8.0, 6.0, 3.0], 3.0),
];

/// What the item called `name` is as armor, if it is.
pub fn armor_of(name: &str) -> Option<Armor> {
    if name == "elytra" {
        return Some(Armor {
            slot: ArmorSlot::Chest,
            defense: 0.0,
            toughness: 0.0,
            glider: true,
        });
    }
    for (prefix, points, toughness) in MATERIALS {
        let Some(piece) = name.strip_prefix(prefix) else {
            continue;
        };
        let (slot, index) = match piece {
            "helmet" => (ArmorSlot::Head, 0),
            "chestplate" => (ArmorSlot::Chest, 1),
            "leggings" => (ArmorSlot::Legs, 2),
            "boots" => (ArmorSlot::Feet, 3),
            _ => continue,
        };
        return Some(Armor {
            slot,
            defense: points[index],
            toughness,
            glider: false,
        });
    }
    None
}

/// `damage` after `defense` and `toughness` (modern `getDamageAfterAbsorb`).
pub fn reduce(damage: f32, defense: f32, toughness: f32) -> f32 {
    if defense <= 0.0 {
        return damage;
    }
    let p = (defense - damage / (2.0 + toughness / 4.0))
        .max(defense / 5.0)
        .min(20.0);
    damage * (1.0 - p / 25.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_armor_name_has_its_modern_points() {
        let iron = armor_of("iron_chestplate").unwrap();
        assert_eq!(
            (iron.slot, iron.defense, iron.toughness),
            (ArmorSlot::Chest, 6.0, 0.0)
        );
        assert_eq!(armor_of("diamond_boots").unwrap().defense, 3.0);
        assert_eq!(armor_of("netherite_helmet").unwrap().toughness, 3.0);
        assert_eq!(armor_of("golden_leggings").unwrap().slot, ArmorSlot::Legs);
        let total: f32 = ["helmet", "chestplate", "leggings", "boots"]
            .iter()
            .map(|p| armor_of(&format!("diamond_{p}")).unwrap().defense)
            .sum();
        assert_eq!(total, 20.0, "a full diamond set");
        assert!(armor_of("iron_sword").is_none() && armor_of("iron_ingot").is_none());
        assert!(armor_of("elytra").unwrap().glider);
    }

    #[test]
    fn damage_is_cut_by_defense_with_toughness_helping_against_big_hits() {
        assert_eq!(reduce(5.0, 0.0, 0.0), 5.0);
        // Full iron (15) against a 5-point hit: 5 × (1 − 12/25)... max(3, 15 − 2.5) = 12.5.
        assert!((reduce(5.0, 15.0, 0.0) - 5.0 * (1.0 - 12.5 / 25.0)).abs() < 1e-5);
        // Toughness 8 (full diamond) against 20: more than none at all.
        assert!(reduce(20.0, 20.0, 8.0) < reduce(20.0, 20.0, 0.0));
        // Never more than 80% off.
        assert!(reduce(1.0, 40.0, 0.0) >= 0.2 - 1e-6);
    }
}
