//! Item definitions loaded from `assets/data/items.ron`.
//!
//! IDs are Minecraft 1.0.0's numeric item IDs (`docs/blocks-and-items.md`).

use std::{collections::BTreeSet, path::Path};

use anyhow::{Context, Result, bail};
use serde::Deserialize;

pub type ItemId = u16;

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ItemDef {
    pub id: ItemId,
    pub name: String,
    /// File stem in `assets/textures/items/`.
    pub texture: String,
}

pub struct ItemRegistry {
    items: Vec<ItemDef>,
}

impl ItemRegistry {
    pub fn load(path: &Path) -> Result<Self> {
        let text =
            std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
        Self::from_ron(&text).with_context(|| format!("parsing {}", path.display()))
    }

    pub fn from_ron(text: &str) -> Result<Self> {
        let mut items: Vec<ItemDef> = ron::from_str(text)?;
        items.sort_by_key(|item| item.id);
        if let Some(pair) = items.windows(2).find(|pair| pair[0].id == pair[1].id) {
            bail!(
                "item ID {} used by both {} and {}",
                pair[0].id,
                pair[0].name,
                pair[1].name
            );
        }
        Ok(Self { items })
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// Texture names referenced by some item that `exists` rejects.
    pub fn missing_textures(&self, exists: impl Fn(&str) -> bool) -> BTreeSet<String> {
        self.items
            .iter()
            .map(|item| item.texture.as_str())
            .filter(|name| !exists(name))
            .map(str::to_owned)
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn real_registry() -> ItemRegistry {
        ItemRegistry::load(&crate::assets::dir().join("data/items.ron")).unwrap()
    }

    impl ItemRegistry {
        fn get(&self, id: ItemId) -> Option<&ItemDef> {
            self.items.iter().find(|item| item.id == id)
        }
    }

    #[test]
    fn every_1_0_0_item_id_is_defined() {
        let registry = real_registry();
        for id in (256..=382).chain(2256..=2266) {
            assert!(
                registry.get(id).is_some(),
                "item {id} missing from items.ron"
            );
        }
        assert_eq!(registry.len(), 127 + 11);
        assert_eq!(registry.get(278).unwrap().name, "diamond_pickaxe");
    }

    #[test]
    fn every_item_texture_exists() {
        let dir = crate::assets::dir().join("textures/items");
        let missing =
            real_registry().missing_textures(|name| dir.join(format!("{name}.png")).is_file());
        assert!(missing.is_empty(), "missing item textures: {missing:?}");
    }

    #[test]
    fn duplicate_ids_are_rejected() {
        let ron = r#"[(id: 256, name: "a", texture: "a"), (id: 256, name: "b", texture: "b")]"#;
        assert!(ItemRegistry::from_ron(ron).is_err());
    }
}
