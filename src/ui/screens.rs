//! Screens with slots: the inventory opened with E (survival's, with its
//! 2×2 crafting grid, or creative's list of every block and item) and the
//! crafting table's, laid out as in 1.0.0.

use egui::{Color32, Painter, Pos2, Rect, pos2, vec2};

use super::gui::{self, GuiTextures, SlotView};
use crate::inventory::{
    player::{HOTBAR_SLOTS, SLOTS},
    slots::{Click, SlotRef},
    stack::ItemStack,
};

/// Which screen is open over the game.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Screen {
    Inventory,
    /// `scroll` is how many rows the item grid has scrolled; it's
    /// fractional while a trackpad moves it.
    Creative {
        scroll: f32,
    },
    /// A crafting table's 3×3 grid.
    Crafting,
}

/// What the player did on a screen this frame.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ScreenAction {
    Click(SlotRef, Click),
    /// Clicked outside the panel: throw the held stack (all, or one).
    Throw {
        all: bool,
    },
    /// Creative: put a full stack of this in the selected hotbar slot.
    Take(ItemStack),
    /// Creative: clicked the item grid while holding something.
    Discard,
    /// The death screen's Respawn button.
    Respawn,
}

/// Everything a screen shows.
pub struct ScreenView<'a> {
    /// What a slot holds, with its name for the hover tooltip.
    pub slot: &'a dyn Fn(SlotRef) -> Option<(SlotView, String)>,
    /// The stack held on the mouse.
    pub cursor: Option<SlotView>,
    /// Creative: every block and item, with how it looks and its name.
    pub catalog: &'a [(ItemStack, SlotView, String)],
}

/// Creative grid size.
const COLUMNS: usize = 9;
const ROWS: usize = 5;

/// Rows the creative grid can scroll through.
pub fn creative_scroll_rows(catalog_len: usize) -> usize {
    catalog_len.div_ceil(COLUMNS).saturating_sub(ROWS)
}

/// Mouse wheel movement this frame in grid rows (positive scrolls up):
/// a wheel notch is one row, trackpad movement one row per slot height.
fn wheel_rows(input: &egui::InputState, scale: f32) -> f32 {
    input
        .raw
        .events
        .iter()
        .filter_map(|event| match event {
            egui::Event::MouseWheel { unit, delta, .. } => Some(match unit {
                egui::MouseWheelUnit::Point => delta.y / (18.0 * scale),
                _ => delta.y.signum(),
            }),
            _ => None,
        })
        .sum()
}

/// An inventory slot's top-left in panel pixels, below the top part of a
/// 166-pixel-tall screen.
fn inventory_slot(index: usize) -> [f32; 2] {
    if index < HOTBAR_SLOTS {
        [8.0 + index as f32 * 18.0, 142.0]
    } else {
        let i = index - HOTBAR_SLOTS;
        [8.0 + (i % 9) as f32 * 18.0, 84.0 + (i / 9) as f32 * 18.0]
    }
}

/// A `size` × `size` crafting grid's slots from `at`, and the result's.
fn crafting_slots(size: usize, at: [f32; 2], result: [f32; 2]) -> Vec<(SlotRef, [f32; 2])> {
    let cells = (0..size * size).map(|i| {
        let (column, row) = ((i % size) as f32, (i / size) as f32);
        (
            SlotRef::Grid(i),
            [at[0] + column * 18.0, at[1] + row * 18.0],
        )
    });
    cells.chain([(SlotRef::Result, result)]).collect()
}

/// The slots `screen` shows and where (1.0.0's container layouts). The
/// creative item grid isn't included.
fn layout(screen: &Screen) -> Vec<(SlotRef, [f32; 2])> {
    let inventory = (0..SLOTS).map(|i| (SlotRef::Inventory(i), inventory_slot(i)));
    match screen {
        Screen::Inventory => crafting_slots(2, [88.0, 26.0], [144.0, 36.0])
            .into_iter()
            .chain(inventory)
            .collect(),
        Screen::Crafting => crafting_slots(3, [30.0, 17.0], [124.0, 35.0])
            .into_iter()
            .chain(inventory)
            .collect(),
        Screen::Creative { .. } => (0..HOTBAR_SLOTS)
            .map(|i| (SlotRef::Inventory(i), [9.0 + i as f32 * 18.0, 112.0]))
            .collect(),
    }
}

/// Titles drawn on a screen, with where (1.0.0's foreground layers).
fn labels(screen: &Screen) -> &'static [(&'static str, [f32; 2])] {
    match screen {
        Screen::Inventory => &[("Crafting", [86.0, 16.0])],
        Screen::Crafting => &[("Crafting", [28.0, 6.0]), ("Inventory", [8.0, 72.0])],
        Screen::Creative { .. } => &[],
    }
}

/// Draws `screen` and returns what was clicked.
pub fn show(
    ctx: &egui::Context,
    screen: &mut Screen,
    view: &ScreenView,
    textures: &GuiTextures,
) -> Vec<ScreenAction> {
    let painter = ctx.layer_painter(egui::LayerId::new(
        egui::Order::Foreground,
        egui::Id::unique("screen"),
    ));
    let full = ctx.content_rect();
    let scale = gui::gui_scale(full);
    // 1.0.0 darkens the world behind open screens.
    painter.rect_filled(full, 0.0, Color32::from_black_alpha(140));
    let (pointer, left, right, shift, wheel) = ctx.input(|i| {
        (
            i.pointer.hover_pos(),
            i.pointer.primary_pressed(),
            i.pointer.secondary_pressed(),
            i.modifiers.shift,
            wheel_rows(i, scale),
        )
    });
    let click = if shift && left {
        Some(Click::Shift)
    } else if left {
        Some(Click::Left)
    } else if right {
        Some(Click::Right)
    } else {
        None
    };

    let size = match screen {
        Screen::Inventory | Screen::Crafting => vec2(176.0, 166.0),
        Screen::Creative { .. } => vec2(195.0, 136.0),
    };
    let origin = full.center() - size * scale / 2.0;
    let panel = Rect::from_min_size(origin, size * scale);
    let slot_rect = |at: [f32; 2]| {
        Rect::from_min_size(
            origin + vec2(at[0], at[1]) * scale,
            vec2(16.0, 16.0) * scale,
        )
    };
    let mut actions = Vec::new();
    let mut tooltip = None;

    let background = match screen {
        Screen::Inventory => &textures.inventory,
        Screen::Crafting => &textures.crafting,
        Screen::Creative { .. } => &textures.creative,
    };
    gui::sheet_image(
        &painter,
        background.id(),
        [0.0, 0.0],
        size.into(),
        origin,
        scale,
    );
    for &(text, at) in labels(screen) {
        painter.text(
            origin + vec2(at[0], at[1]) * scale,
            egui::Align2::LEFT_TOP,
            text,
            egui::FontId::proportional(8.0 * scale),
            Color32::from_gray(0x40),
        );
    }
    if let Screen::Creative { scroll } = screen {
        let rows = creative_scroll_rows(view.catalog.len());
        if let Some(pos) = pointer
            && panel.contains(pos)
        {
            *scroll = (*scroll - wheel).clamp(0.0, rows as f32);
        }
        let top_row = scroll.round() as usize;
        let fraction = if rows == 0 {
            0.0
        } else {
            top_row as f32 / rows as f32
        };
        let scroller = origin + vec2(175.0, 18.0 + (112.0 - 15.0) * fraction) * scale;
        gui::sheet_image(
            &painter,
            textures.tabs.id(),
            [232.0, 0.0],
            [12.0, 15.0],
            scroller,
            scale,
        );
        for row in 0..ROWS {
            for column in 0..COLUMNS {
                let index = (top_row + row) * COLUMNS + column;
                let Some((stack, slot, name)) = view.catalog.get(index) else {
                    continue;
                };
                let rect = slot_rect([9.0 + column as f32 * 18.0, 18.0 + row as f32 * 18.0]);
                gui::paint_slot(&painter, rect.min, slot, scale, textures.atlas.id());
                if pointer.is_some_and(|pos| rect.contains(pos)) {
                    painter.rect_filled(rect, 0.0, Color32::from_white_alpha(80));
                    tooltip = Some(name.clone());
                    if click.is_some() {
                        actions.push(if view.cursor.is_some() {
                            ScreenAction::Discard
                        } else {
                            ScreenAction::Take(*stack)
                        });
                    }
                }
            }
        }
    }

    for (slot, at) in layout(screen) {
        let rect = slot_rect(at);
        let shown = (view.slot)(slot);
        if let Some((stack, _)) = &shown {
            gui::paint_slot(&painter, rect.min, stack, scale, textures.atlas.id());
        }
        if pointer.is_some_and(|pos| rect.contains(pos)) {
            painter.rect_filled(rect, 0.0, Color32::from_white_alpha(80));
            tooltip = shown.map(|(_, name)| name);
            if let Some(click) = click {
                actions.push(ScreenAction::Click(slot, click));
            }
        }
    }

    if let Some(pos) = pointer {
        if click.is_some() && !panel.contains(pos) && view.cursor.is_some() {
            actions.push(ScreenAction::Throw { all: left });
        }
        if let Some(cursor) = &view.cursor {
            gui::paint_slot(
                &painter,
                pos - vec2(8.0, 8.0) * scale,
                cursor,
                scale,
                textures.atlas.id(),
            );
        } else if let Some(name) = tooltip {
            paint_tooltip(&painter, pos, &name, scale);
        }
    }
    actions
}

/// 1.0.0's item name tooltip: white text on a dark box above-right of the mouse.
fn paint_tooltip(painter: &Painter, pointer: Pos2, name: &str, scale: f32) {
    let font = egui::FontId::proportional(8.0 * scale);
    let galley = painter.layout_no_wrap(name.to_owned(), font, Color32::WHITE);
    let at = pointer + vec2(12.0, -12.0) * scale;
    let rect = Rect::from_min_size(at, galley.size()).expand(3.0 * scale);
    painter.rect_filled(rect, 0.0, Color32::from_rgba_unmultiplied(16, 0, 16, 240));
    painter.rect_stroke(
        rect,
        0.0,
        egui::Stroke::new(scale, Color32::from_rgb(40, 0, 127)),
        egui::StrokeKind::Inside,
    );
    painter.galley(pos2(at.x, at.y), galley, Color32::WHITE);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inventory_slots_sit_where_1_0_0_puts_them() {
        assert_eq!(inventory_slot(0), [8.0, 142.0]);
        assert_eq!(inventory_slot(8), [152.0, 142.0]);
        assert_eq!(inventory_slot(9), [8.0, 84.0]);
        assert_eq!(inventory_slot(35), [152.0, 120.0]);
    }

    #[test]
    fn crafting_slots_sit_where_1_0_0_puts_them() {
        let table = layout(&Screen::Crafting);
        assert_eq!(table.len(), 9 + 1 + SLOTS);
        assert!(table.contains(&(SlotRef::Grid(0), [30.0, 17.0])));
        assert!(table.contains(&(SlotRef::Grid(8), [66.0, 53.0])));
        assert!(table.contains(&(SlotRef::Result, [124.0, 35.0])));
        let inventory = layout(&Screen::Inventory);
        assert!(inventory.contains(&(SlotRef::Grid(3), [106.0, 44.0])));
        assert!(inventory.contains(&(SlotRef::Result, [144.0, 36.0])));
    }

    #[test]
    fn the_creative_grid_scrolls_by_rows() {
        assert_eq!(creative_scroll_rows(45), 0);
        assert_eq!(creative_scroll_rows(46), 1);
        assert_eq!(creative_scroll_rows(320), 31);
    }
}
