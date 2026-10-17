//! How each item stack looks as an icon in the HUD and as a dropped item:
//! blocks that are cubes draw as small cubes, everything else as a flat
//! picture (the item's texture, or the block's for plants, torches, rails...).

use std::{collections::HashMap, sync::Arc};

use super::atlas::{Atlas, ITEM_PREFIX, MISSING_TILE};
use crate::{
    inventory::{
        item::{FIRST_ITEM_ID, ItemId, ItemRegistry},
        stack::ItemStack,
    },
    world::{
        Face,
        block::{BlockState, Shape},
        mesh::BlockModels,
    },
};

/// A textured face: atlas UV rect `[u0, v0, u1, v1]` and an sRGB tint.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct IconFace {
    pub uv: [f32; 4],
    pub color: [f32; 3],
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ItemIcon {
    /// Top, front (south) and side (east) faces, unshaded.
    Cube {
        top: IconFace,
        front: IconFace,
        side: IconFace,
    },
    Flat(IconFace),
}

pub struct ItemIcons {
    models: Arc<BlockModels>,
    /// Item texture UVs by (ID, data value); data 0 is the default.
    items: HashMap<(ItemId, u8), [f32; 4]>,
    missing: [f32; 4],
}

impl ItemIcons {
    pub fn new(atlas: &Atlas, models: Arc<BlockModels>, registry: &ItemRegistry) -> Self {
        let missing = atlas.uv_rect(MISSING_TILE);
        let uv = |texture: &str| {
            atlas
                .get(&format!("{ITEM_PREFIX}{texture}"))
                .map_or(missing, |tile| atlas.uv_rect(tile))
        };
        let mut items = HashMap::new();
        for item in registry.iter() {
            items.insert((item.id, 0), uv(&item.texture));
            for (&data, variant) in &item.variants {
                items.insert((item.id, data), uv(&variant.texture));
            }
        }
        Self {
            models,
            items,
            missing,
        }
    }

    pub fn icon(&self, stack: &ItemStack) -> ItemIcon {
        if stack.id >= FIRST_ITEM_ID {
            let data = u8::try_from(stack.damage).unwrap_or(0);
            let uv = self
                .items
                .get(&(stack.id, data))
                .or_else(|| self.items.get(&(stack.id, 0)))
                .copied()
                .unwrap_or(self.missing);
            return ItemIcon::Flat(IconFace {
                uv,
                color: [1.0; 3],
            });
        }
        let model = self
            .models
            .get(BlockState::new(stack.id, stack.damage as u8));
        let face = |face: Face| {
            let texture = model.faces[face.index()];
            IconFace {
                uv: texture.uv,
                color: self.models.icon_color(texture.tint),
            }
        };
        if is_cube_icon(model.shape) {
            ItemIcon::Cube {
                top: face(Face::Top),
                front: face(Face::South),
                side: face(Face::East),
            }
        } else {
            ItemIcon::Flat(face(Face::South))
        }
    }
}

/// Shapes 1.0.0 draws as a 3D block in the inventory.
fn is_cube_icon(shape: Shape) -> bool {
    matches!(
        shape,
        Shape::Cube
            | Shape::Slab
            | Shape::Stairs
            | Shape::Fence
            | Shape::FenceGate
            | Shape::Cactus
            | Shape::Cake
            | Shape::Liquid
    )
}
