//! How each item stack looks as an icon in the HUD and as a dropped item:
//! blocks that are cubes draw as small cubes, everything else as a flat
//! picture (the item's texture, or the block's for plants, torches, rails...).

use std::sync::Arc;

use super::atlas::{Atlas, MISSING_TILE};
use crate::{
    inventory::{item::ItemRegistry, stack::ItemStack},
    pack::model::ItemModels,
    world::{
        Face,
        block::{BlockState, Shape},
        mesh::BlockModels,
        props::Prop,
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

/// The textures of the bow being drawn, by how far.
pub const BOW_PULLING: [&str; 3] = [
    "item/bow_pulling_0",
    "item/bow_pulling_1",
    "item/bow_pulling_2",
];

/// Which of the bow's drawn pictures shows after `ticks` of drawing (the
/// bow model's `pull` thresholds: 0.65 and 0.9 of a full 20-tick draw).
pub fn bow_stage(ticks: f32) -> usize {
    let pull = ticks / 20.0;
    if pull >= 0.9 {
        2
    } else if pull >= 0.65 {
        1
    } else {
        0
    }
}

pub struct ItemIcons {
    models: Arc<BlockModels>,
    /// Each item's icon, by ID.
    icons: Vec<ItemIcon>,
    /// The bow's pictures while it's drawn.
    bow: [ItemIcon; 3],
    missing: [f32; 4],
}

impl ItemIcons {
    /// `item_models` swaps in the sprites packs' item models give.
    pub fn new(
        atlas: &Atlas,
        models: Arc<BlockModels>,
        registry: &ItemRegistry,
        item_models: &ItemModels,
    ) -> Self {
        let missing = atlas.uv_rect(MISSING_TILE);
        let mut icons = Self {
            models,
            icons: Vec::with_capacity(registry.len()),
            bow: BOW_PULLING.map(|name| {
                ItemIcon::Flat(IconFace {
                    uv: atlas.uv_or_missing(name),
                    color: [1.0; 3],
                })
            }),
            missing,
        };
        for item in registry.iter() {
            let icon = match (&item.texture, item.places) {
                (Some(texture), _) => ItemIcon::Flat(IconFace {
                    uv: atlas.uv_or_missing(
                        &item_models
                            .texture(&item.name)
                            .map_or_else(|| format!("item/{texture}"), str::to_owned),
                    ),
                    color: [1.0; 3],
                }),
                // Facing the viewer, as an icon shows the south face.
                (None, Some(block)) => icons.block_icon(
                    BlockState::of(block)
                        .with_face(Prop::Facing, Face::South)
                        .with_face(Prop::FacingAll, Face::South),
                ),
                (None, None) => ItemIcon::Flat(IconFace {
                    uv: missing,
                    color: [1.0; 3],
                }),
            };
            icons.icons.push(icon);
        }
        icons
    }

    pub fn icon(&self, stack: &ItemStack) -> ItemIcon {
        self.icons
            .get(stack.id.index())
            .copied()
            .unwrap_or(ItemIcon::Flat(IconFace {
                uv: self.missing,
                color: [1.0; 3],
            }))
    }

    /// The icon of `stack` while the bow in it has been drawn for `ticks`
    /// (anything else looks as usual).
    pub fn icon_drawn(&self, stack: &ItemStack, ticks: f32) -> ItemIcon {
        if stack.id == crate::inventory::items::BOW {
            self.bow[bow_stage(ticks)]
        } else {
            self.icon(stack)
        }
    }

    /// A block drawn as an icon: a small cube, or its south face flat.
    fn block_icon(&self, state: BlockState) -> ItemIcon {
        let model = self.models.get(state);
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
