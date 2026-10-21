//! What a smithing table makes (modern `SmithingMenu` with the netherite
//! upgrade): an upgrade template, a diamond tool, weapon or armor piece and
//! a netherite ingot give the netherite one, keeping its enchantments,
//! name and wear. Armor trims aren't in.

use super::{
    items,
    stack::{ItemStack, Items},
};

/// The netherite version of `base` with `template` and `addition`, if
/// they make one.
pub fn upgrade(
    items: &Items,
    template: Option<&ItemStack>,
    base: Option<&ItemStack>,
    addition: Option<&ItemStack>,
) -> Option<ItemStack> {
    let (template, base, addition) = (template?, base?, addition?);
    if template.id != items::NETHERITE_UPGRADE_SMITHING_TEMPLATE
        || addition.id != items::NETHERITE_INGOT
    {
        return None;
    }
    let kind = base.id.name().strip_prefix("diamond_")?;
    let upgraded = items::by_name(&format!("netherite_{kind}"))?;
    items.item(upgraded)?;
    let mut made = base.clone();
    made.id = upgraded;
    made.count = 1;
    Some(made)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::inventory::enchant::Enchantment;

    #[test]
    fn a_diamond_pickaxe_becomes_netherite_keeping_its_enchantments_and_wear() {
        let items = Items::new();
        let template = ItemStack::new(items::NETHERITE_UPGRADE_SMITHING_TEMPLATE, 1);
        let ingot = ItemStack::new(items::NETHERITE_INGOT, 3);
        let mut pick = ItemStack::with_damage(items::DIAMOND_PICKAXE, 1, 100);
        pick.enchant(Enchantment::Efficiency, 4);
        let made = upgrade(&items, Some(&template), Some(&pick), Some(&ingot)).unwrap();
        assert_eq!(made.id, items::NETHERITE_PICKAXE);
        assert_eq!((made.count, made.damage), (1, 100));
        assert_eq!(made.level_of(Enchantment::Efficiency), 4);
        for name in [
            "sword",
            "shovel",
            "axe",
            "hoe",
            "helmet",
            "chestplate",
            "leggings",
            "boots",
        ] {
            let base = ItemStack::new(items::by_name(&format!("diamond_{name}")).unwrap(), 1);
            let made = upgrade(&items, Some(&template), Some(&base), Some(&ingot)).unwrap();
            assert_eq!(made.id.name(), format!("netherite_{name}"));
        }
        // Iron doesn't upgrade, and nothing works without the template.
        let iron = ItemStack::new(items::IRON_PICKAXE, 1);
        assert!(upgrade(&items, Some(&template), Some(&iron), Some(&ingot)).is_none());
        assert!(upgrade(&items, None, Some(&pick), Some(&ingot)).is_none());
        let diamond = ItemStack::new(items::DIAMOND, 1);
        assert!(upgrade(&items, Some(&template), Some(&pick), Some(&diamond)).is_none());
    }
}
