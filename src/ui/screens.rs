//! Screens with slots: the inventory opened with E (survival's, with its
//! 2×2 crafting grid, or creative's list of every block and item), the
//! crafting table's, the chest's and the furnace's, laid out as in 1.0.0.

use egui::{Color32, Painter, Pos2, Rect, pos2, vec2};

use super::gui::{self, GuiTextures, SlotView};
use crate::inventory::{
    furnace,
    player::{HOTBAR_SLOTS, MAIN_SLOTS, OFFHAND},
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
    Chest,
    Furnace,
    /// A dispenser's 3×3 slots, or a dropper's.
    Dispenser {
        dropper: bool,
    },
    Hopper,
}

/// What the player did on a screen this frame.
#[derive(Debug, Clone, PartialEq)]
pub enum ScreenAction {
    Click(SlotRef, Click),
    /// Dragged the held stack across these slots with the button down:
    /// spread evenly (left) or one in each (right), as modern Minecraft
    /// does.
    Drag {
        slots: Vec<SlotRef>,
        click: Click,
    },
    /// Double-clicked a slot: gather matching items onto the mouse.
    Collect(SlotRef),
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
    /// The sleep screen's Leave Bed button.
    LeaveBed,
    /// A choice in the pause menu.
    Pause(super::pause::PauseAction),
}

/// Everything a screen shows.
pub struct ScreenView<'a> {
    /// What a slot holds, with its name for the hover tooltip.
    pub slot: &'a dyn Fn(SlotRef) -> Option<(SlotView, String)>,
    /// A furnace's fuel left and cooking progress, each 0–1.
    pub furnace: Option<(f32, f32)>,
    /// The stack held on the mouse.
    pub cursor: Option<SlotView>,
    /// Creative: every block and item, with how it looks and its name.
    pub catalog: &'a [(ItemStack, SlotView, String)],
}

/// A drag across slots in progress: the button held and the slots crossed.
#[derive(Debug, Clone, PartialEq)]
struct Drag {
    click: Click,
    slots: Vec<SlotRef>,
}

/// A second left press on the same slot within this many seconds is a
/// double-click.
const DOUBLE_CLICK: f64 = 0.25;

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

/// Rows of slots in a chest.
const CHEST_ROWS: usize = 3;

/// An inventory slot's top-left in panel pixels, on a 166-pixel-tall
/// screen.
fn inventory_slot(index: usize) -> [f32; 2] {
    if index == OFFHAND {
        // The off-hand slot beside the crafting grid, in the survival screen.
        [77.0, 62.0]
    } else if index < HOTBAR_SLOTS {
        [8.0 + index as f32 * 18.0, 142.0]
    } else {
        let i = index - HOTBAR_SLOTS;
        [8.0 + (i % 9) as f32 * 18.0, 84.0 + (i / 9) as f32 * 18.0]
    }
}

/// Panel size in GUI pixels.
fn panel_size(screen: &Screen) -> [f32; 2] {
    match screen {
        Screen::Inventory | Screen::Crafting | Screen::Furnace | Screen::Dispenser { .. } => {
            [176.0, 166.0]
        }
        Screen::Hopper => [176.0, 133.0],
        Screen::Creative { .. } => [195.0, 136.0],
        // 1.0.0's GuiChest: 114 plus 18 per row.
        Screen::Chest => [176.0, 114.0 + CHEST_ROWS as f32 * 18.0],
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
    // 1.0.0's chest puts the player's rows at 103 + (rows − 4) × 18.
    let below = match screen {
        Screen::Chest => 103.0 + (CHEST_ROWS as f32 - 4.0) * 18.0 - 84.0,
        // The hopper's player rows start at 51.
        Screen::Hopper => 51.0 - 84.0,
        _ => 0.0,
    };
    let inventory = (0..MAIN_SLOTS).map(move |i| {
        let [x, y] = inventory_slot(i);
        (SlotRef::Inventory(i), [x, y + below])
    });
    match screen {
        Screen::Inventory => crafting_slots(2, [88.0, 26.0], [144.0, 36.0])
            .into_iter()
            .chain(inventory)
            .chain([(SlotRef::Inventory(OFFHAND), inventory_slot(OFFHAND))])
            .collect(),
        Screen::Crafting => crafting_slots(3, [30.0, 17.0], [124.0, 35.0])
            .into_iter()
            .chain(inventory)
            .collect(),
        Screen::Creative { .. } => (0..HOTBAR_SLOTS)
            .map(|i| (SlotRef::Inventory(i), [9.0 + i as f32 * 18.0, 112.0]))
            .collect(),
        Screen::Furnace => [
            (SlotRef::Block(furnace::INPUT), [56.0, 17.0]),
            (SlotRef::Block(furnace::FUEL), [56.0, 53.0]),
            (SlotRef::Block(furnace::OUTPUT), [116.0, 35.0]),
        ]
        .into_iter()
        .chain(inventory)
        .collect(),
        Screen::Dispenser { .. } => (0..9)
            .map(|i| {
                let (column, row) = ((i % 3) as f32, (i / 3) as f32);
                (SlotRef::Block(i), [62.0 + column * 18.0, 17.0 + row * 18.0])
            })
            .chain(inventory)
            .collect(),
        Screen::Hopper => (0..5)
            .map(|i| (SlotRef::Block(i), [44.0 + i as f32 * 18.0, 20.0]))
            .chain(inventory)
            .collect(),
        Screen::Chest => (0..CHEST_ROWS * 9)
            .map(|i| {
                let (column, row) = ((i % 9) as f32, (i / 9) as f32);
                (SlotRef::Block(i), [8.0 + column * 18.0, 18.0 + row * 18.0])
            })
            .chain(inventory)
            .collect(),
    }
}

/// Titles drawn on a screen, with where (1.0.0's foreground layers).
fn labels(screen: &Screen) -> &'static [(&'static str, [f32; 2])] {
    match screen {
        Screen::Inventory => &[("Crafting", [86.0, 16.0])],
        Screen::Crafting => &[("Crafting", [28.0, 6.0]), ("Inventory", [8.0, 72.0])],
        Screen::Creative { .. } => &[],
        Screen::Chest => &[("Chest", [8.0, 6.0]), ("Inventory", [8.0, 74.0])],
        Screen::Furnace => &[("Furnace", [60.0, 6.0]), ("Inventory", [8.0, 72.0])],
        Screen::Dispenser { dropper: false } => {
            &[("Dispenser", [60.0, 6.0]), ("Inventory", [8.0, 72.0])]
        }
        Screen::Dispenser { dropper: true } => {
            &[("Dropper", [60.0, 6.0]), ("Inventory", [8.0, 72.0])]
        }
        Screen::Hopper => &[("Item Hopper", [8.0, 6.0]), ("Inventory", [8.0, 39.0])],
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
    let (now, left_down, right_down) =
        ctx.input(|i| (i.time, i.pointer.primary_down(), i.pointer.secondary_down()));
    let drag_id = egui::Id::unique("screen-drag");
    let last_press_id = egui::Id::unique("screen-last-press");
    let mut drag: Option<Drag> = ctx.data(|d| d.get_temp(drag_id));
    let last_press: Option<(SlotRef, f64)> = ctx.data(|d| d.get_temp(last_press_id));
    let click = if shift && left {
        Some(Click::Shift)
    } else if left {
        Some(Click::Left)
    } else if right {
        Some(Click::Right)
    } else {
        None
    };

    let size = egui::Vec2::from(panel_size(screen));
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
        Screen::Chest => &textures.chest,
        Screen::Furnace => &textures.furnace,
        Screen::Dispenser { .. } => &textures.dispenser,
        Screen::Hopper => &textures.hopper,
    };
    if *screen == Screen::Chest {
        // The chest rows, then the inventory part from the bottom of the
        // six-row sheet (1.0.0's GuiChest).
        let top = CHEST_ROWS as f32 * 18.0 + 17.0;
        gui::sheet_image(
            &painter,
            background.id(),
            [0.0, 0.0],
            [size.x, top],
            origin,
            scale,
        );
        gui::sheet_image(
            &painter,
            background.id(),
            [0.0, 126.0],
            [size.x, 96.0],
            origin + vec2(0.0, top) * scale,
            scale,
        );
    } else {
        gui::sheet_image(
            &painter,
            background.id(),
            [0.0, 0.0],
            size.into(),
            origin,
            scale,
        );
    }
    if let (Screen::Furnace, Some((fuel, cooked))) = (*screen, view.furnace) {
        paint_furnace_progress(&painter, background.id(), origin, scale, fuel, cooked);
    }
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

    let mut hovered = None;
    for (slot, at) in layout(screen) {
        let rect = slot_rect(at);
        let shown = (view.slot)(slot);
        if let Some((stack, _)) = &shown {
            gui::paint_slot(&painter, rect.min, stack, scale, textures.atlas.id());
        }
        let dragged = drag.as_ref().is_some_and(|drag| drag.slots.contains(&slot));
        if pointer.is_some_and(|pos| rect.contains(pos)) {
            hovered = Some(slot);
            tooltip = shown.map(|(_, name)| name);
        }
        if dragged || hovered == Some(slot) {
            painter.rect_filled(rect, 0.0, Color32::from_white_alpha(80));
        }
    }
    slot_input(
        &mut actions,
        &mut drag,
        hovered,
        click,
        view.cursor.is_some(),
        last_press,
        now,
    );
    if let Some(started) = &drag {
        let down = match started.click {
            Click::Right => right_down,
            _ => left_down,
        };
        if !down {
            // Released: one slot is an ordinary click, more a spread.
            actions.push(match started.slots.as_slice() {
                &[slot] => ScreenAction::Click(slot, started.click),
                slots => ScreenAction::Drag {
                    slots: slots.to_vec(),
                    click: started.click,
                },
            });
            drag = None;
        }
    }
    if left && let Some(slot) = hovered {
        ctx.data_mut(|d| d.insert_temp(last_press_id, (slot, now)));
    }
    ctx.data_mut(|d| match drag {
        Some(drag) => {
            d.insert_temp(drag_id, drag);
        }
        None => d.remove::<Drag>(drag_id),
    });

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

/// Turns a press or drag over the slot under the mouse into actions
/// (modern Minecraft's rules): with a stack held, a left or right press
/// starts a drag that collects the slots crossed until release, and a
/// second left press on the same slot is a double-click; with nothing
/// held, and for shift-clicks, a press clicks at once.
fn slot_input(
    actions: &mut Vec<ScreenAction>,
    drag: &mut Option<Drag>,
    hovered: Option<SlotRef>,
    click: Option<Click>,
    holding: bool,
    last_press: Option<(SlotRef, f64)>,
    now: f64,
) {
    let Some(slot) = hovered else {
        return;
    };
    if let Some(drag) = drag {
        if !drag.slots.contains(&slot) && slot != SlotRef::Result {
            drag.slots.push(slot);
        }
        return;
    }
    match click {
        Some(Click::Left)
            if holding
                && last_press.is_some_and(|(last, at)| last == slot && now - at < DOUBLE_CLICK) =>
        {
            actions.push(ScreenAction::Collect(slot));
        }
        Some(click @ (Click::Left | Click::Right)) if holding && slot != SlotRef::Result => {
            *drag = Some(Drag {
                click,
                slots: vec![slot],
            });
        }
        Some(click) => actions.push(ScreenAction::Click(slot, click)),
        None => {}
    }
}

/// The furnace's flame, shrinking as the fuel burns, and its arrow, growing
/// as the input cooks (1.0.0's GuiFurnace, with the pack's 14×14 flame).
fn paint_furnace_progress(
    painter: &Painter,
    texture: egui::TextureId,
    origin: Pos2,
    scale: f32,
    fuel: f32,
    cooked: f32,
) {
    if fuel > 0.0 {
        let flame = (fuel * 13.0).floor();
        gui::sheet_image(
            painter,
            texture,
            [176.0, 12.0 - flame],
            [14.0, flame + 1.0],
            origin + vec2(56.0, 36.0 + 12.0 - flame) * scale,
            scale,
        );
    }
    let arrow = (cooked * 24.0).floor();
    gui::sheet_image(
        painter,
        texture,
        [176.0, 14.0],
        [arrow + 1.0, 16.0],
        origin + vec2(79.0, 34.0) * scale,
        scale,
    );
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
        assert_eq!(table.len(), 9 + 1 + MAIN_SLOTS);
        assert!(table.contains(&(SlotRef::Grid(0), [30.0, 17.0])));
        assert!(table.contains(&(SlotRef::Grid(8), [66.0, 53.0])));
        assert!(table.contains(&(SlotRef::Result, [124.0, 35.0])));
        let furnace = layout(&Screen::Furnace);
        assert!(furnace.contains(&(SlotRef::Block(furnace::OUTPUT), [116.0, 35.0])));
        let chest = layout(&Screen::Chest);
        assert!(chest.contains(&(SlotRef::Block(26), [152.0, 54.0])));
        assert!(chest.contains(&(SlotRef::Inventory(9), [8.0, 85.0])));
        assert!(chest.contains(&(SlotRef::Inventory(0), [8.0, 143.0])));
        let inventory = layout(&Screen::Inventory);
        assert!(inventory.contains(&(SlotRef::Grid(3), [106.0, 44.0])));
        assert!(inventory.contains(&(SlotRef::Result, [144.0, 36.0])));
    }

    #[test]
    fn holding_a_stack_a_press_drags_and_a_quick_second_press_collects() {
        let (a, b) = (SlotRef::Inventory(9), SlotRef::Inventory(10));
        let mut actions = Vec::new();
        let mut drag = None;
        // With nothing held, a press clicks at once.
        slot_input(
            &mut actions,
            &mut drag,
            Some(a),
            Some(Click::Left),
            false,
            None,
            0.0,
        );
        assert_eq!(actions, [ScreenAction::Click(a, Click::Left)]);
        // A quick second press on the same slot, now holding the stack.
        actions.clear();
        let last = Some((a, 0.0));
        slot_input(
            &mut actions,
            &mut drag,
            Some(a),
            Some(Click::Left),
            true,
            last,
            0.1,
        );
        assert_eq!(actions, [ScreenAction::Collect(a)]);
        // A slower press starts a drag that picks up slots as it crosses them.
        actions.clear();
        slot_input(
            &mut actions,
            &mut drag,
            Some(a),
            Some(Click::Right),
            true,
            last,
            1.0,
        );
        slot_input(&mut actions, &mut drag, Some(b), None, true, last, 1.1);
        slot_input(
            &mut actions,
            &mut drag,
            Some(SlotRef::Result),
            None,
            true,
            last,
            1.2,
        );
        slot_input(&mut actions, &mut drag, Some(a), None, true, last, 1.3);
        assert!(actions.is_empty());
        assert_eq!(
            drag,
            Some(Drag {
                click: Click::Right,
                slots: vec![a, b]
            })
        );
    }

    #[test]
    fn the_creative_grid_scrolls_by_rows() {
        assert_eq!(creative_scroll_rows(45), 0);
        assert_eq!(creative_scroll_rows(46), 1);
        assert_eq!(creative_scroll_rows(320), 31);
    }
}
