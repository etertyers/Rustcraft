//! Drawing Minecraft-style GUI with the resource packs' `gui/` images:
//! the GUI scale, item icons (flat pictures or little isometric cubes),
//! stack counts and durability bars, and the hotbar.

use anyhow::{Context, Result};
use egui::{Color32, Mesh, Painter, Pos2, Rect, Shape, TextureHandle, TextureId, pos2, vec2};
use image::{RgbaImage, imageops};

use crate::{
    inventory::stack::{ItemStack, Items},
    pack::Resources,
    render::icons::{IconFace, ItemIcon, ItemIcons},
};

/// What one slot holds, ready to draw.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SlotView {
    pub icon: ItemIcon,
    pub count: u8,
    /// Durability left, 0–1, once a tool or armor piece has been used.
    pub durability: Option<f32>,
    /// Enchanted: the pattern shimmers over it.
    pub glint: bool,
}

impl SlotView {
    pub fn of(stack: &ItemStack, icons: &ItemIcons, items: &Items) -> Self {
        Self::with_icon(stack, icons.icon(stack), items)
    }

    /// A slot showing `stack` as `icon` (a bow being drawn looks different).
    pub fn with_icon(stack: &ItemStack, icon: ItemIcon, items: &Items) -> Self {
        let durability = items
            .max_damage(stack.id)
            .filter(|_| stack.damage > 0)
            .map(|max| 1.0 - stack.damage as f32 / max as f32);
        Self {
            icon,
            count: stack.count,
            durability,
            glint: stack.is_enchanted(),
        }
    }
}

/// GUI images loaded into egui.
pub struct GuiTextures {
    pub atlas: TextureHandle,
    pub widgets: TextureHandle,
    pub inventory: TextureHandle,
    pub crafting: TextureHandle,
    pub chest: TextureHandle,
    pub furnace: TextureHandle,
    pub dispenser: TextureHandle,
    pub hopper: TextureHandle,
    pub enchanting: TextureHandle,
    pub anvil: TextureHandle,
    pub grindstone: TextureHandle,
    pub brewing: TextureHandle,
    /// The recipe book's panel, tabs and buttons, and the green book button.
    pub recipe_book: TextureHandle,
    pub recipe_button: TextureHandle,
    /// The player's skin, armor sheet and elytra, for the figure in the
    /// inventory.
    pub skin: TextureHandle,
    pub armor: TextureHandle,
    /// Where the armor sheet has pixels, for the glint (`glint::mask`).
    pub armor_mask: TextureHandle,
    pub elytra: TextureHandle,
    /// Falls back to the anvil's sheet: Kenzo's 1.12 GUI has no smithing
    /// table, so the slots then sit over the wrong outlines.
    pub smithing: TextureHandle,
    /// The stonecutter's, which also falls back to the anvil's sheet.
    pub stonecutter: TextureHandle,
    pub creative: TextureHandle,
    /// The Search tab's panel, with its text box.
    pub creative_search: TextureHandle,
    /// The Survival Inventory tab's panel.
    pub creative_inventory: TextureHandle,
    pub tabs: TextureHandle,
    pub icons: TextureHandle,
    /// Each status effect's icon, in `Effect::ALL`'s order, if any layer
    /// has one (M26).
    pub effects: Vec<Option<TextureHandle>>,
}

/// GUI images are 256×256 sheets.
const SHEET: f32 = 256.0;

/// Most pixels per GUI pixel kept in a sheet (1024×1024); sharper packs
/// are scaled down to it.
pub const MAX_SHEET_SCALE: u32 = 4;

/// A modern pack's sprite (`gui/sprites/...`, since 1.20.2) and the
/// region `[x, y, w, h]` it fills on the old 256×256 sheet.
type Sprite = (&'static str, [u32; 4]);

const WIDGETS: &[Sprite] = &[
    ("gui/sprites/hud/hotbar", [0, 0, 182, 22]),
    ("gui/sprites/hud/hotbar_selection", [0, 22, 24, 24]),
    ("gui/sprites/hud/hotbar_offhand_left", [24, 22, 29, 24]),
];
const ICONS: &[Sprite] = &[
    ("gui/sprites/hud/heart/container", [16, 0, 9, 9]),
    ("gui/sprites/hud/heart/full", [52, 0, 9, 9]),
    ("gui/sprites/hud/heart/half", [61, 0, 9, 9]),
    ("gui/sprites/hud/air", [16, 18, 9, 9]),
    ("gui/sprites/hud/air_bursting", [25, 18, 9, 9]),
    ("gui/sprites/hud/food_empty", [16, 27, 9, 9]),
    ("gui/sprites/hud/food_full", [52, 27, 9, 9]),
    ("gui/sprites/hud/food_half", [61, 27, 9, 9]),
    ("gui/sprites/hud/experience_bar_background", [0, 64, 182, 5]),
    ("gui/sprites/hud/experience_bar_progress", [0, 69, 182, 5]),
    (
        "gui/sprites/hud/crosshair_attack_indicator_background",
        [36, 94, 16, 4],
    ),
    (
        "gui/sprites/hud/crosshair_attack_indicator_progress",
        [52, 94, 16, 4],
    ),
    (
        "gui/sprites/hud/crosshair_attack_indicator_full",
        [68, 94, 16, 16],
    ),
];
const FURNACE: &[Sprite] = &[
    (
        "gui/sprites/container/furnace/lit_progress",
        [176, 0, 14, 14],
    ),
    (
        "gui/sprites/container/furnace/burn_progress",
        [176, 14, 24, 17],
    ),
];
/// The recipe book's pieces on 1.12's sheet, which a modern pack draws as
/// separate sprites.
const RECIPE_BOOK: &[Sprite] = &[
    ("gui/sprites/recipe_book/tab", [153, 2, 30, 26]),
    ("gui/sprites/recipe_book/tab_selected", [188, 2, 35, 26]),
    ("gui/sprites/recipe_book/filter_disabled", [152, 41, 26, 16]),
    (
        "gui/sprites/recipe_book/filter_disabled_highlighted",
        [152, 59, 26, 16],
    ),
    ("gui/sprites/recipe_book/filter_enabled", [180, 41, 26, 16]),
    (
        "gui/sprites/recipe_book/filter_enabled_highlighted",
        [180, 59, 26, 16],
    ),
    ("gui/sprites/recipe_book/slot_craftable", [29, 206, 25, 25]),
    (
        "gui/sprites/recipe_book/slot_uncraftable",
        [54, 206, 25, 25],
    ),
    ("gui/sprites/recipe_book/page_backward", [1, 208, 11, 17]),
    (
        "gui/sprites/recipe_book/page_backward_highlighted",
        [1, 226, 11, 17],
    ),
    ("gui/sprites/recipe_book/page_forward", [15, 208, 11, 17]),
    (
        "gui/sprites/recipe_book/page_forward_highlighted",
        [15, 226, 11, 17],
    ),
];
/// The green book button, in 1.12's sheet of two (plain, then hovered).
const RECIPE_BUTTON: &[Sprite] = &[
    ("gui/sprites/recipe_book/button", [0, 0, 20, 18]),
    (
        "gui/sprites/recipe_book/button_highlighted",
        [0, 19, 20, 18],
    ),
];
const TABS: &[Sprite] = &[(
    "gui/sprites/container/creative_inventory/scroller",
    [232, 0, 12, 15],
)];

impl GuiTextures {
    /// Builds each sheet from the resource layers (see `compose_sheet`).
    pub fn load(
        ctx: &egui::Context,
        atlas: &RgbaImage,
        figure: crate::look::FigureImages,
        res: &Resources,
    ) -> Result<Self> {
        let load = |name: &str, image: &RgbaImage| {
            let size = [image.width() as usize, image.height() as usize];
            let pixels = egui::ColorImage::from_rgba_unmultiplied(size, image.as_raw());
            ctx.load_texture(name, pixels, egui::TextureOptions::NEAREST)
        };
        let widths = std::cell::RefCell::new(Vec::new());
        let sheet = |name: &str, sprites: &[Sprite]| -> Result<TextureHandle> {
            let image = compose_sheet(res, name, sprites)
                .with_context(|| format!("no GUI image {name}"))?;
            let short = name.rsplit('/').next().unwrap_or(name);
            widths
                .borrow_mut()
                .push(format!("{short} {}", image.width()));
            Ok(load(name, &image))
        };
        let textures = Self {
            atlas: load("atlas", atlas),
            widgets: sheet("gui/widgets", WIDGETS)?,
            inventory: sheet("gui/container/inventory", &[])?,
            crafting: sheet("gui/container/crafting_table", &[])?,
            chest: sheet("gui/container/generic_54", &[])?,
            furnace: sheet("gui/container/furnace", FURNACE)?,
            dispenser: sheet("gui/container/dispenser", &[])?,
            hopper: sheet("gui/container/hopper", &[])?,
            enchanting: sheet("gui/container/enchanting_table", &[])?,
            anvil: sheet("gui/container/anvil", &[])?,
            grindstone: sheet("gui/container/grindstone", &[])?,
            brewing: sheet("gui/container/brewing_stand", &[])?,
            recipe_book: sheet("gui/recipe_book", RECIPE_BOOK)?,
            recipe_button: sheet("gui/recipe_button", RECIPE_BUTTON)?,
            skin: load("skin", figure.skin),
            armor: load("armor", figure.armor),
            armor_mask: ctx.load_texture(
                "armor mask",
                super::glint::mask(figure.armor),
                egui::TextureOptions::NEAREST,
            ),
            elytra: load("elytra", figure.elytra),
            smithing: sheet("gui/container/smithing", &[])
                .or_else(|_| sheet("gui/container/anvil", &[]))?,
            stonecutter: sheet("gui/container/stonecutter", &[])
                .or_else(|_| sheet("gui/container/anvil", &[]))?,
            creative: sheet("gui/container/creative_inventory/tab_items", &[])?,
            creative_search: sheet("gui/container/creative_inventory/tab_item_search", &[])?,
            creative_inventory: sheet("gui/container/creative_inventory/tab_inventory", &[])?,
            tabs: sheet("gui/container/creative_inventory/tabs", TABS)?,
            icons: sheet("gui/icons", ICONS)?,
            effects: crate::player::effects::Effect::ALL
                .iter()
                .map(|&effect| effect_icon(res, effect).map(|image| load(effect.id(), &image)))
                .collect(),
        };
        super::glint::install(ctx, atlas, res.image(crate::render::GLINT_TEXTURE));
        log::info!("GUI sheets (px wide): {}", widths.borrow().join(", "));
        Ok(textures)
    }
}

/// A status effect's 18 × 18 icon: modern's `mob_effect/<effect>`, or its
/// cell of the old inventory sheet's grid at (0, 198).
fn effect_icon(res: &Resources, effect: crate::player::effects::Effect) -> Option<RgbaImage> {
    if let Some(image) = res.image(&format!("mob_effect/{}", effect.id())) {
        return Some(image);
    }
    let (column, row) = effect.legacy_icon()?;
    let sheet = res.image("gui/container/inventory")?;
    let scale = (sheet.width() / 256).max(1);
    let (x, y) = ((column * 18) * scale, (198 + row * 18) * scale);
    let size = 18 * scale;
    (x + size <= sheet.width() && y + size <= sheet.height())
        .then(|| image::imageops::crop_imm(&sheet, x, y, size, size).to_image())
}

/// One of the old 256×256 GUI sheets, built up from the bottom layer:
/// a layer's own copy of the sheet replaces everything below it, then its
/// modern sprites are drawn over their regions. So a 1.20.2+ pack, which
/// splits the HUD into sprites, still re-skins the sheet, and anything it
/// leaves out shows the layer below. `None` if no layer has any of it.
pub fn compose_sheet(res: &Resources, sheet: &str, sprites: &[Sprite]) -> Option<RgbaImage> {
    let mut canvas: Option<RgbaImage> = None;
    for layer in 0..res.layers().len() {
        if let Some(texture) = res.texture_in(layer, sheet) {
            let image = texture.first_frame();
            let scale = (image.width() / 256).clamp(1, MAX_SHEET_SCALE);
            canvas = Some(resized(&image, 256 * scale, 256 * scale));
        }
        for &(name, [x, y, w, h]) in sprites {
            let Some(texture) = res.texture_in(layer, name) else {
                continue;
            };
            let sprite = texture.first_frame();
            let canvas = canvas.get_or_insert_with(|| RgbaImage::new(256, 256));
            // The sheet grows to the sharpest picture drawn on it.
            let wanted = (sprite.width() / w).clamp(1, MAX_SHEET_SCALE);
            if wanted * 256 > canvas.width() {
                *canvas = resized(canvas, 256 * wanted, 256 * wanted);
            }
            let scale = canvas.width() / 256;
            // Clear the old region, then draw the sprite at its own shape.
            let clear = RgbaImage::new(w * scale, h * scale);
            imageops::replace(canvas, &clear, (x * scale).into(), (y * scale).into());
            let height = (sprite.height() * w * scale)
                .div_ceil(sprite.width())
                .max(1);
            let sprite = resized(&sprite, w * scale, height.min(h * scale));
            imageops::replace(canvas, &sprite, (x * scale).into(), (y * scale).into());
        }
    }
    canvas
}

/// `image` at `width`×`height`: blocky when growing, smooth when shrinking.
fn resized(image: &RgbaImage, width: u32, height: u32) -> RgbaImage {
    if image.dimensions() == (width, height) {
        return image.clone();
    }
    let filter = if width < image.width() {
        imageops::FilterType::Triangle
    } else {
        imageops::FilterType::Nearest
    };
    imageops::resize(image, width, height, filter)
}

/// Minecraft's "auto" GUI scale: the largest whole number of screen points
/// per GUI pixel that leaves at least 320×240 GUI pixels.
pub fn gui_scale(screen: Rect) -> f32 {
    scale_for(
        screen,
        CHOSEN_SCALE.load(std::sync::atomic::Ordering::Relaxed),
    )
}

/// The GUI Scale option: 0 for Auto.
static CHOSEN_SCALE: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

pub fn set_gui_scale(scale: u32) {
    CHOSEN_SCALE.store(scale, std::sync::atomic::Ordering::Relaxed);
}

/// The largest scale that fits (Auto), or the chosen one if it fits too.
fn scale_for(screen: Rect, chosen: u32) -> f32 {
    let fit = (screen.width() / 320.0).min(screen.height() / 240.0);
    let fit = fit.floor().max(1.0);
    match chosen {
        0 => fit,
        n => (n as f32).min(fit),
    }
}

/// Draws the `size` region of a 256×256 GUI sheet starting at `from`
/// (both in sheet pixels) with its top-left at `at`, `scale` points per pixel.
pub fn sheet_image(
    painter: &Painter,
    texture: TextureId,
    from: [f32; 2],
    size: [f32; 2],
    at: Pos2,
    scale: f32,
) {
    let uv = Rect::from_min_size(
        pos2(from[0] / SHEET, from[1] / SHEET),
        vec2(size[0] / SHEET, size[1] / SHEET),
    );
    let rect = Rect::from_min_size(at, vec2(size[0], size[1]) * scale);
    painter.image(texture, rect, uv, Color32::WHITE);
}

fn color(rgb: [f32; 3], shade: f32) -> Color32 {
    let c = |v: f32| (v * shade * 255.0).round().clamp(0.0, 255.0) as u8;
    Color32::from_rgb(c(rgb[0]), c(rgb[1]), c(rgb[2]))
}

fn uv_rect(face: &IconFace) -> Rect {
    let [u0, v0, u1, v1] = face.uv;
    Rect::from_min_max(pos2(u0, v0), pos2(u1, v1))
}

/// Draws an item icon filling `rect` (a 16×16 GUI-pixel square).
pub fn paint_icon(painter: &Painter, rect: Rect, icon: &ItemIcon, atlas: TextureId) {
    match icon {
        ItemIcon::Flat(face) => {
            painter.image(atlas, rect, uv_rect(face), color(face.color, 1.0));
        }
        ItemIcon::Cube { top, front, side } => {
            // An isometric cube: top, left (front) and right (side) faces,
            // shaded like 1.0.0's inventory blocks.
            let (c, w, h) = (rect.center(), rect.width() * 0.45, rect.height() * 0.5);
            let top_point = pos2(c.x, c.y - h);
            let left = pos2(c.x - w, c.y - h / 2.0);
            let right = pos2(c.x + w, c.y - h / 2.0);
            let bottom = pos2(c.x, c.y + h);
            let low_left = pos2(c.x - w, c.y + h / 2.0);
            let low_right = pos2(c.x + w, c.y + h / 2.0);
            let mut mesh = Mesh::with_texture(atlas);
            // Corners in UV order: top-left, top-right, bottom-right, bottom-left.
            let mut quad = |corners: [Pos2; 4], face: &IconFace, shade: f32| {
                let uv = uv_rect(face);
                let uvs = [
                    uv.left_top(),
                    uv.right_top(),
                    uv.right_bottom(),
                    uv.left_bottom(),
                ];
                let base = mesh.vertices.len() as u32;
                for (pos, uv) in corners.into_iter().zip(uvs) {
                    mesh.vertices.push(egui::epaint::Vertex {
                        pos,
                        uv,
                        color: color(face.color, shade),
                    });
                }
                mesh.indices
                    .extend([base, base + 1, base + 2, base, base + 2, base + 3]);
            };
            quad([left, top_point, right, c], top, 1.0);
            quad([left, c, bottom, low_left], front, 0.8);
            quad([c, right, low_right, bottom], side, 0.6);
            painter.add(Shape::mesh(mesh));
        }
    }
}

/// Draws a slot's icon, its count (if more than one) and its durability
/// bar (once worn), at `at` (the slot's top-left) with `scale`.
pub fn paint_slot(painter: &Painter, at: Pos2, slot: &SlotView, scale: f32, atlas: TextureId) {
    let rect = Rect::from_min_size(at, vec2(16.0, 16.0) * scale);
    paint_icon(painter, rect, &slot.icon, atlas);
    if slot.glint {
        super::glint::paint(painter, rect, &slot.icon);
    }
    if let Some(left) = slot.durability {
        // 1.0.0: a 13-pixel bar on black, green to red as it wears.
        let bar = |x: f32, width: f32, height: f32, color: Color32| {
            let min = at + vec2(2.0 + x, 13.0) * scale;
            painter.rect_filled(
                Rect::from_min_size(min, vec2(width, height) * scale),
                0.0,
                color,
            );
        };
        bar(0.0, 13.0, 2.0, Color32::BLACK);
        let shade = (left.clamp(0.0, 1.0) * 255.0) as u8;
        bar(
            0.0,
            (13.0 * left).round(),
            1.0,
            Color32::from_rgb(255 - shade, shade, 0),
        );
    }
    if slot.count > 1 {
        let font = egui::FontId::monospace(8.0 * scale);
        let corner = at + vec2(17.0, 17.5) * scale;
        let text = slot.count.to_string();
        painter.text(
            corner + vec2(scale, scale),
            egui::Align2::RIGHT_BOTTOM,
            &text,
            font.clone(),
            Color32::from_gray(63),
        );
        painter.text(
            corner,
            egui::Align2::RIGHT_BOTTOM,
            &text,
            font,
            Color32::WHITE,
        );
    }
}

/// Survival numbers for the HUD.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SurvivalHud {
    pub health: i32,
    pub food: i32,
    /// Air while the eyes are under water.
    pub air: Option<i32>,
    /// The experience level and how far to the next, 0–1.
    pub level: u32,
    pub progress: f32,
}

/// Hearts, drumsticks and air bubbles above the hotbar, from icons.png,
/// as in 1.0.0's `GuiIngame`: each icon is two points, full or half.
pub fn paint_survival(painter: &Painter, screen: Rect, hud: &SurvivalHud, textures: &GuiTextures) {
    let scale = gui_scale(screen);
    let icons = textures.icons.id();
    let left = screen.center().x - 91.0 * scale;
    let right = screen.center().x + 91.0 * scale;
    let row = screen.bottom() - 39.0 * scale;
    let icon = |from: [f32; 2], x: f32, y: f32| {
        sheet_image(painter, icons, from, [9.0, 9.0], pos2(x, y), scale);
    };
    for i in 0..10 {
        let points = i * 2;
        // Hearts fill from the left, drumsticks from the right.
        let heart_x = left + i as f32 * 8.0 * scale;
        icon([16.0, 0.0], heart_x, row);
        if hud.health > points + 1 {
            icon([52.0, 0.0], heart_x, row);
        } else if hud.health == points + 1 {
            icon([61.0, 0.0], heart_x, row);
        }
        let food_x = right - (i as f32 * 8.0 + 9.0) * scale;
        icon([16.0, 27.0], food_x, row);
        if hud.food > points + 1 {
            icon([52.0, 27.0], food_x, row);
        } else if hud.food == points + 1 {
            icon([61.0, 27.0], food_x, row);
        }
    }
    // The experience bar sits just above the hotbar, its level over it.
    let bar = pos2(left, screen.bottom() - 29.0 * scale);
    sheet_image(painter, icons, [0.0, 64.0], [182.0, 5.0], bar, scale);
    let filled = (hud.progress.clamp(0.0, 1.0) * 183.0).floor().min(182.0);
    if filled > 0.0 {
        sheet_image(painter, icons, [0.0, 69.0], [filled, 5.0], bar, scale);
    }
    if hud.level > 0 {
        let font = egui::FontId::monospace(8.0 * scale);
        let at = pos2(screen.center().x, screen.bottom() - 30.0 * scale);
        let text = hud.level.to_string();
        for (dx, dy) in [(1.0, 0.0), (-1.0, 0.0), (0.0, 1.0), (0.0, -1.0)] {
            painter.text(
                at + vec2(dx, dy) * scale,
                egui::Align2::CENTER_BOTTOM,
                &text,
                font.clone(),
                Color32::BLACK,
            );
        }
        painter.text(
            at,
            egui::Align2::CENTER_BOTTOM,
            &text,
            font,
            Color32::from_rgb(128, 255, 32),
        );
    }
    if let Some(air) = hud.air {
        let full = ((air - 2) as f32 * 10.0 / 300.0).ceil().max(0.0) as i32;
        let popping = (air as f32 * 10.0 / 300.0).ceil() as i32 - full;
        for i in 0..full + popping {
            let x = right - (i as f32 * 8.0 + 9.0) * scale;
            let from = if i < full { [16.0, 18.0] } else { [25.0, 18.0] };
            icon(from, x, row - 10.0 * scale);
        }
    }
}

/// The off-hand's item in its own little slot left of the hotbar (shown
/// only when it holds something).
pub fn paint_offhand(painter: &Painter, screen: Rect, slot: &SlotView, textures: &GuiTextures) {
    let scale = gui_scale(screen);
    let at = pos2(
        screen.center().x - 120.0 * scale,
        screen.bottom() - 23.0 * scale,
    );
    sheet_image(
        painter,
        textures.widgets.id(),
        [24.0, 22.0],
        [29.0, 24.0],
        at,
        scale,
    );
    let item_at = pos2(
        screen.center().x - 117.0 * scale,
        screen.bottom() - 19.0 * scale,
    );
    paint_slot(painter, item_at, slot, scale, textures.atlas.id());
}

/// The attack indicator under the crosshair (modern `renderCrosshair`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AttackIndicator {
    /// How charged the next swing is, 0–1.
    pub charge: f32,
    /// Fully charged with a mob under the crosshair and a weapon that
    /// recharges slowly: the sword icon shows.
    pub ready: bool,
}

/// Draws the attack indicator: a bar filling as the swing recharges, or the
/// sword once it is ready on a target; nothing when recharged without one.
pub fn paint_attack_indicator(
    painter: &Painter,
    screen: Rect,
    indicator: AttackIndicator,
    textures: &GuiTextures,
) {
    let scale = gui_scale(screen);
    let at = pos2(
        screen.center().x - 8.0 * scale,
        screen.center().y + 9.0 * scale,
    );
    let icons = textures.icons.id();
    if indicator.ready {
        sheet_image(painter, icons, [68.0, 94.0], [16.0, 16.0], at, scale);
    } else if indicator.charge < 1.0 {
        sheet_image(painter, icons, [36.0, 94.0], [16.0, 4.0], at, scale);
        let filled = (indicator.charge * 17.0).floor().min(16.0);
        if filled > 0.0 {
            sheet_image(painter, icons, [52.0, 94.0], [filled, 4.0], at, scale);
        }
    }
}

/// The hotbar at the bottom of the screen, as in 1.0.0's `GuiIngame`.
pub fn paint_hotbar(
    painter: &Painter,
    screen: Rect,
    slots: &[Option<SlotView>],
    selected: usize,
    textures: &GuiTextures,
) {
    let scale = gui_scale(screen);
    let at = pos2(
        screen.center().x - 91.0 * scale,
        screen.bottom() - 22.0 * scale,
    );
    sheet_image(
        painter,
        textures.widgets.id(),
        [0.0, 0.0],
        [182.0, 22.0],
        at,
        scale,
    );
    let highlight = at + vec2(-1.0 + selected as f32 * 20.0, -1.0) * scale;
    sheet_image(
        painter,
        textures.widgets.id(),
        [0.0, 22.0],
        [24.0, 24.0],
        highlight,
        scale,
    );
    for (i, slot) in slots.iter().enumerate() {
        if let Some(slot) = slot {
            let slot_at = at + vec2(3.0 + i as f32 * 20.0, 3.0) * scale;
            paint_slot(painter, slot_at, slot, scale, textures.atlas.id());
        }
    }
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use super::*;
    use crate::pack::{
        Pack,
        tests::{ASHEN, MCMETA, PRIME, kenzos_pack, write_zip},
    };

    fn png(width: u32, height: u32, rgba: [u8; 4]) -> Vec<u8> {
        let mut bytes = Vec::new();
        RgbaImage::from_pixel(width, height, image::Rgba(rgba))
            .write_to(&mut Cursor::new(&mut bytes), image::ImageFormat::Png)
            .unwrap();
        bytes
    }

    #[test]
    fn modern_sprites_draw_over_the_sheet_below() {
        let dir = crate::world::save::TempDir::new("gui");
        let base = dir.path().join("base");
        std::fs::create_dir_all(base.join("textures/gui")).unwrap();
        std::fs::write(
            base.join("textures/gui/widgets.png"),
            png(256, 256, [1, 1, 1, 255]),
        )
        .unwrap();
        std::fs::write(
            base.join("textures/gui/icons.png"),
            png(256, 256, [5, 5, 5, 255]),
        )
        .unwrap();
        // A 1.20.2+ pack at 2×: a hotbar sprite but no selection sprite.
        let zip = dir.path().join("modern.zip");
        let hotbar = png(364, 44, [2, 2, 2, 255]);
        write_zip(
            &zip,
            &[
                ("pack.mcmeta", MCMETA),
                (
                    "assets/minecraft/textures/gui/sprites/hud/hotbar.png",
                    &hotbar,
                ),
            ],
        );
        let res = Resources::new(
            Pack::base(&base),
            vec![Pack::open(&zip).unwrap()],
            Default::default(),
        );
        let sheet = compose_sheet(&res, "gui/widgets", WIDGETS).unwrap();
        assert_eq!(sheet.dimensions(), (512, 512));
        assert_eq!(sheet.get_pixel(0, 0).0, [2, 2, 2, 255]);
        assert_eq!(sheet.get_pixel(363, 43).0, [2, 2, 2, 255]);
        // The selection box falls back to the base sheet, scaled up.
        assert_eq!(sheet.get_pixel(10, 50).0, [1, 1, 1, 255]);
        // Untouched sheets come straight from the layer that has them.
        let icons = compose_sheet(&res, "gui/icons", ICONS).unwrap();
        assert_eq!(icons.dimensions(), (256, 256));
        assert!(compose_sheet(&res, "gui/nothing", &[]).is_none());
    }

    #[test]
    fn very_sharp_packs_are_capped() {
        let dir = crate::world::save::TempDir::new("gui-hd");
        let base = dir.path().join("base");
        std::fs::create_dir_all(base.join("textures/gui")).unwrap();
        std::fs::write(
            base.join("textures/gui/icons.png"),
            png(4096, 4096, [7, 7, 7, 255]),
        )
        .unwrap();
        let res = Resources::new(Pack::base(&base), Vec::new(), Default::default());
        let sheet = compose_sheet(&res, "gui/icons", ICONS).unwrap();
        assert_eq!(sheet.width(), 256 * MAX_SHEET_SCALE);
    }

    #[test]
    fn kenzos_packs_reskin_the_gui() {
        let assets = crate::assets::dir();
        let base = Resources::load(&assets, Vec::new()).unwrap();
        let sheets = [
            ("gui/widgets", WIDGETS),
            ("gui/icons", ICONS),
            ("gui/container/inventory", &[][..]),
            ("gui/container/furnace", FURNACE),
        ];
        for name in [ASHEN, PRIME] {
            let Some(pack) = kenzos_pack(name) else {
                continue;
            };
            let res = Resources::load(&assets, vec![pack]).unwrap();
            for (sheet, sprites) in sheets {
                let plain = compose_sheet(&base, sheet, sprites).unwrap();
                let image = compose_sheet(&res, sheet, sprites).unwrap();
                // Compared at the base sheet's size.
                let image = resized(&image, plain.width(), plain.height());
                assert_ne!(image, plain, "{name} doesn't change {sheet}");
            }
        }
    }

    #[test]
    fn gui_scale_is_minecrafts_auto_scale() {
        let screen = |w: f32, h: f32| Rect::from_min_size(Pos2::ZERO, vec2(w, h));
        assert_eq!(gui_scale(screen(1920.0, 1080.0)), 4.0);
        assert_eq!(gui_scale(screen(1280.0, 720.0)), 3.0);
        assert_eq!(gui_scale(screen(854.0, 480.0)), 2.0);
        assert_eq!(gui_scale(screen(300.0, 200.0)), 1.0);
        // A chosen scale applies when it fits.
        assert_eq!(scale_for(screen(1920.0, 1080.0), 2), 2.0);
        assert_eq!(scale_for(screen(854.0, 480.0), 5), 2.0);
    }
}
