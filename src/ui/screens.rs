//! Screens with slots: the inventory opened with E (survival's, with its
//! 2×2 crafting grid, or creative's list of every block and item), the
//! crafting table's, the chest's and the furnace's, laid out as in 1.0.0.

use egui::{Color32, Painter, Pos2, Rect, pos2, vec2};

use super::gui::{self, GuiTextures, SlotView};
use crate::inventory::{
    brewing, furnace,
    player::{ARMOR_FEET, ARMOR_HEAD, HOTBAR_SLOTS, MAIN_SLOTS, OFFHAND, is_armor_slot},
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
    /// A brewing stand's bottles, ingredient and fuel (M26).
    Brewing,
    /// A dispenser's 3×3 slots, or a dropper's.
    Dispenser {
        dropper: bool,
    },
    Hopper,
    /// An enchanting table's item and lapis slots, and its three offers.
    Enchanting,
    Anvil,
    Grindstone,
    /// A smithing table's template, base and addition (M25).
    Smithing,
    /// Trading with a villager.
    Merchant,
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
    /// Took the enchanting table's offer 0–2.
    Enchant(usize),
    /// Typed in the anvil's name box.
    AnvilName(String),
    /// Made the villager's trade with this number.
    Trade(usize),
}

/// One of a villager's trades, ready to draw.
#[derive(Debug, Clone, PartialEq)]
pub struct TradeView {
    pub buy: SlotView,
    pub buy_b: Option<SlotView>,
    pub sell: SlotView,
    /// What the trade gives, for the tooltip.
    pub name: String,
    pub uses_left: u32,
    /// The player has what it costs and it's in stock.
    pub affordable: bool,
}

/// The trading screen's title, level progress and trades.
#[derive(Debug, Clone, PartialEq)]
pub struct MerchantView {
    pub title: String,
    /// How far to the next level, 0–1 (`None` at master).
    pub progress: Option<f32>,
    pub trades: Vec<TradeView>,
}

/// One of an enchanting table's offers.
#[derive(Debug, Clone, PartialEq)]
pub struct OfferView {
    /// Levels the player needs to have.
    pub requires: u32,
    /// The hint shown on hover: the first enchantment.
    pub clue: String,
}

/// What the anvil says about its result.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum AnvilStatus {
    #[default]
    Nothing,
    Cost {
        levels: u32,
        affordable: bool,
    },
    TooExpensive,
}

/// Everything an enchanting table, anvil or grindstone screen shows
/// besides its slots.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct BenchView {
    pub offers: [Option<OfferView>; 3],
    pub lapis: u32,
    pub level: u32,
    /// Creative: nothing costs anything.
    pub free: bool,
    pub anvil: AnvilStatus,
    /// The anvil's name box.
    pub name: String,
    /// Whether the anvil's first slot holds something to rename.
    pub has_item: bool,
    /// Something is in the slots but nothing can come out (the red X).
    pub blocked: bool,
}

/// Everything a screen shows.
pub struct ScreenView<'a> {
    /// What a slot holds, with its name for the hover tooltip.
    pub slot: &'a dyn Fn(SlotRef) -> Option<(SlotView, String)>,
    /// A furnace's fuel left and cooking progress, each 0–1.
    pub furnace: Option<(f32, f32)>,
    /// The open brewing stand's progress and fuel, 0–1.
    pub brewing: Option<(f32, f32)>,
    /// The stack held on the mouse.
    pub cursor: Option<SlotView>,
    /// Creative: every block and item, with how it looks and its name.
    pub catalog: &'a [(ItemStack, SlotView, String)],
    /// The workstation screens' extras.
    pub bench: Option<BenchView>,
    pub merchant: Option<MerchantView>,
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
    if is_armor_slot(index) {
        // Helmet at the top of the column left of the crafting grid, down
        // to the boots.
        [8.0, 8.0 + (ARMOR_HEAD - index) as f32 * 18.0]
    } else if index == OFFHAND {
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
        Screen::Inventory
        | Screen::Crafting
        | Screen::Furnace
        | Screen::Brewing
        | Screen::Dispenser { .. }
        | Screen::Enchanting
        | Screen::Anvil
        | Screen::Grindstone
        | Screen::Smithing => [176.0, 166.0],
        Screen::Hopper => [176.0, 133.0],
        Screen::Merchant => [176.0, 222.0],
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
        // Below the trades, where a six-row chest's inventory is.
        Screen::Merchant => 103.0 + 2.0 * 18.0 - 84.0,
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
            .chain((ARMOR_FEET..=ARMOR_HEAD).map(|i| (SlotRef::Inventory(i), inventory_slot(i))))
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
        // 1.9+'s layout, with the fuel slot top left.
        Screen::Brewing => [
            (SlotRef::Block(brewing::BOTTLES[0]), [56.0, 51.0]),
            (SlotRef::Block(brewing::BOTTLES[1]), [79.0, 58.0]),
            (SlotRef::Block(brewing::BOTTLES[2]), [102.0, 51.0]),
            (SlotRef::Block(brewing::INGREDIENT), [79.0, 17.0]),
            (SlotRef::Block(brewing::FUEL), [17.0, 17.0]),
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
        Screen::Enchanting => [
            (SlotRef::Grid(0), [15.0, 47.0]),
            (SlotRef::Grid(1), [35.0, 47.0]),
        ]
        .into_iter()
        .chain(inventory)
        .collect(),
        Screen::Anvil => [
            (SlotRef::Grid(0), [27.0, 47.0]),
            (SlotRef::Grid(1), [76.0, 47.0]),
            (SlotRef::Result, [134.0, 47.0]),
        ]
        .into_iter()
        .chain(inventory)
        .collect(),
        Screen::Merchant => inventory.collect(),
        Screen::Grindstone => [
            (SlotRef::Grid(0), [49.0, 19.0]),
            (SlotRef::Grid(1), [49.0, 40.0]),
            (SlotRef::Result, [129.0, 34.0]),
        ]
        .into_iter()
        .chain(inventory)
        .collect(),
        // Modern (1.20) layout.
        Screen::Smithing => [
            (SlotRef::Grid(0), [8.0, 48.0]),
            (SlotRef::Grid(1), [26.0, 48.0]),
            (SlotRef::Grid(2), [44.0, 48.0]),
            (SlotRef::Result, [98.0, 48.0]),
        ]
        .into_iter()
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
        Screen::Brewing => &[("Brewing Stand", [57.0, 6.0]), ("Inventory", [8.0, 72.0])],
        Screen::Dispenser { dropper: false } => {
            &[("Dispenser", [60.0, 6.0]), ("Inventory", [8.0, 72.0])]
        }
        Screen::Dispenser { dropper: true } => {
            &[("Dropper", [60.0, 6.0]), ("Inventory", [8.0, 72.0])]
        }
        Screen::Hopper => &[("Item Hopper", [8.0, 6.0]), ("Inventory", [8.0, 39.0])],
        Screen::Enchanting => &[("Enchant", [12.0, 5.0]), ("Inventory", [8.0, 73.0])],
        Screen::Anvil => &[("Repair & Name", [60.0, 6.0]), ("Inventory", [8.0, 73.0])],
        Screen::Grindstone => &[
            ("Repair & Disenchant", [8.0, 6.0]),
            ("Inventory", [8.0, 73.0]),
        ],
        Screen::Smithing => &[("Upgrade Gear", [44.0, 15.0]), ("Inventory", [8.0, 73.0])],
        Screen::Merchant => &[("Inventory", [8.0, 128.0])],
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
        Screen::Brewing => &textures.brewing,
        Screen::Dispenser { .. } => &textures.dispenser,
        Screen::Hopper => &textures.hopper,
        Screen::Enchanting => &textures.enchanting,
        Screen::Anvil => &textures.anvil,
        Screen::Grindstone => &textures.grindstone,
        Screen::Smithing => &textures.smithing,
        Screen::Merchant => &textures.chest,
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
    if let (Screen::Brewing, Some((brewed, fuel))) = (*screen, view.brewing) {
        paint_brewing_progress(&painter, background.id(), origin, scale, brewed, fuel);
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
                            ScreenAction::Take(stack.clone())
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
    if let (Screen::Merchant, Some(merchant)) = (*screen, &view.merchant) {
        let widgets = BenchWidgets {
            ctx,
            painter: &painter,
            texture: background.id(),
            origin,
            scale,
            pointer,
            pressed: left,
        };
        if let Some(text) = paint_merchant(&widgets, merchant, &textures.atlas, &mut actions) {
            tooltip = Some(text);
        }
    }
    if let Some(bench) = &view.bench {
        let widgets = BenchWidgets {
            ctx,
            painter: &painter,
            texture: background.id(),
            origin,
            scale,
            pointer,
            pressed: left,
        };
        if let Some(text) = paint_bench(&widgets, *screen, bench, &mut actions) {
            tooltip = Some(text);
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

/// The trades in two columns of five, each its price, an arrow and what it
/// gives, with how many are left; the villager's name and level over
/// them. A trade the player can't afford is dimmed.
fn paint_merchant(
    w: &BenchWidgets,
    merchant: &MerchantView,
    atlas: &egui::TextureHandle,
    actions: &mut Vec<ScreenAction>,
) -> Option<String> {
    let at = |x: f32, y: f32| w.origin + vec2(x, y) * w.scale;
    // Cover the chest's slots: the trades go there.
    w.painter.rect_filled(
        Rect::from_min_max(at(7.0, 16.0), at(169.0, 125.0)),
        0.0,
        Color32::from_rgb(198, 198, 198),
    );
    w.painter.text(
        at(8.0, 5.0),
        egui::Align2::LEFT_TOP,
        &merchant.title,
        egui::FontId::proportional(8.0 * w.scale),
        Color32::from_gray(0x40),
    );
    if let Some(progress) = merchant.progress {
        let bar = Rect::from_min_size(at(110.0, 7.0), vec2(58.0, 4.0) * w.scale);
        w.painter.rect_filled(bar, 0.0, Color32::from_gray(80));
        let mut filled = bar;
        filled.set_width(bar.width() * progress.clamp(0.0, 1.0));
        w.painter
            .rect_filled(filled, 0.0, Color32::from_rgb(128, 255, 32));
    }
    let mut tooltip = None;
    for (i, trade) in merchant.trades.iter().enumerate().take(10) {
        let (x, y) = (8.0 + (i / 5) as f32 * 81.0, 18.0 + (i % 5) as f32 * 21.0);
        let row = Rect::from_min_size(at(x, y), vec2(79.0, 19.0) * w.scale);
        let hovered = w.pointer.is_some_and(|pos| row.contains(pos));
        let shade = if hovered && trade.affordable {
            Color32::from_rgb(160, 200, 160)
        } else {
            Color32::from_rgb(139, 139, 139)
        };
        w.painter.rect_filled(row, 0.0, shade);
        gui::paint_slot(
            w.painter,
            at(x + 1.0, y + 1.5),
            &trade.buy,
            w.scale,
            atlas.id(),
        );
        if let Some(b) = &trade.buy_b {
            gui::paint_slot(w.painter, at(x + 18.0, y + 1.5), b, w.scale, atlas.id());
        }
        w.painter.text(
            at(x + 40.0, y + 9.5),
            egui::Align2::CENTER_CENTER,
            "→",
            egui::FontId::proportional(9.0 * w.scale),
            Color32::WHITE,
        );
        gui::paint_slot(
            w.painter,
            at(x + 48.0, y + 1.5),
            &trade.sell,
            w.scale,
            atlas.id(),
        );
        let (left, color) = if trade.uses_left == 0 {
            ("X".to_owned(), Color32::from_rgb(0xFF, 0x50, 0x50))
        } else {
            (trade.uses_left.to_string(), Color32::from_gray(0x30))
        };
        w.painter.text(
            at(x + 77.0, y + 10.0),
            egui::Align2::RIGHT_CENTER,
            left,
            egui::FontId::proportional(7.0 * w.scale),
            color,
        );
        if !trade.affordable {
            w.painter
                .rect_filled(row, 0.0, Color32::from_black_alpha(70));
        }
        if hovered {
            tooltip = Some(trade.name.clone());
            if w.pressed && trade.affordable {
                actions.push(ScreenAction::Trade(i));
            }
        }
    }
    tooltip
}

/// What drawing a workstation screen's extras needs.
struct BenchWidgets<'a> {
    ctx: &'a egui::Context,
    painter: &'a Painter,
    texture: egui::TextureId,
    origin: Pos2,
    scale: f32,
    pointer: Option<Pos2>,
    pressed: bool,
}

/// Text with a dark shadow, as the game draws numbers on its screens.
fn shadowed(
    painter: &Painter,
    at: Pos2,
    align: egui::Align2,
    text: &str,
    color: Color32,
    scale: f32,
) {
    let font = egui::FontId::proportional(8.0 * scale);
    painter.text(
        at + vec2(scale, scale),
        align,
        text,
        font.clone(),
        Color32::from_black_alpha(160),
    );
    painter.text(at, align, text, font, color);
}

/// The enchanting table's three buttons, the anvil's name box and cost,
/// and the red X on a grindstone or anvil that can't make anything.
/// Returns the tooltip of a button under the mouse.
fn paint_bench(
    w: &BenchWidgets,
    screen: Screen,
    bench: &BenchView,
    actions: &mut Vec<ScreenAction>,
) -> Option<String> {
    let at = |x: f32, y: f32| w.origin + vec2(x, y) * w.scale;
    let sheet = |from: [f32; 2], size: [f32; 2], to: Pos2| {
        gui::sheet_image(w.painter, w.texture, from, size, to, w.scale);
    };
    let mut tooltip = None;
    match screen {
        Screen::Enchanting => {
            for (i, offer) in bench.offers.iter().enumerate() {
                let (x, y) = (60.0, 14.0 + 19.0 * i as f32);
                let price = i as u32 + 1;
                let Some(offer) = offer else {
                    sheet([0.0, 166.0], [108.0, 19.0], at(x, y));
                    continue;
                };
                let affordable = bench.free
                    || (bench.level >= offer.requires
                        && bench.level >= price
                        && bench.lapis >= price);
                let rect = Rect::from_min_size(at(x, y), vec2(108.0, 19.0) * w.scale);
                let hovered = w.pointer.is_some_and(|pos| rect.contains(pos));
                let v = match (affordable, hovered) {
                    (false, _) => 166.0,
                    (true, true) => 204.0,
                    (true, false) => 185.0,
                };
                sheet([0.0, v], [108.0, 19.0], at(x, y));
                let icon_v = if affordable { 223.0 } else { 239.0 };
                sheet(
                    [16.0 * i as f32, icon_v],
                    [16.0, 16.0],
                    at(x + 1.0, y + 1.0),
                );
                let color = if affordable {
                    Color32::from_rgb(0xC8, 0xFF, 0x8F)
                } else {
                    Color32::from_rgb(0x40, 0x7F, 0x10)
                };
                shadowed(
                    w.painter,
                    at(x + 106.0, y + 5.0),
                    egui::Align2::RIGHT_TOP,
                    &offer.requires.to_string(),
                    color,
                    w.scale,
                );
                if hovered {
                    let good = |ok: bool| if ok { "" } else { " (not enough)" };
                    let mut text = format!("{} . . . ?\n", offer.clue);
                    text += &format!(
                        "\n{} Lapis Lazuli{}",
                        price,
                        good(bench.free || bench.lapis >= price)
                    );
                    text += &format!(
                        "\n{} Enchantment Level{}{}",
                        price,
                        if price > 1 { "s" } else { "" },
                        good(bench.free || bench.level >= price)
                    );
                    if bench.level < offer.requires && !bench.free {
                        text += &format!("\nRequires level {}", offer.requires);
                    }
                    tooltip = Some(text);
                    if w.pressed && affordable {
                        actions.push(ScreenAction::Enchant(i));
                    }
                }
            }
        }
        Screen::Anvil => {
            let strip_v = if bench.has_item { 166.0 } else { 182.0 };
            sheet([0.0, strip_v], [110.0, 16.0], at(59.0, 20.0));
            if bench.has_item {
                let mut text = bench.name.clone();
                let area = egui::Area::new(egui::Id::unique("anvil-name"))
                    .order(egui::Order::Foreground)
                    .fixed_pos(at(62.0, 24.0))
                    .show(w.ctx, |ui| {
                        ui.add(
                            egui::TextEdit::singleline(&mut text)
                                .frame(egui::Frame::NONE)
                                .desired_width(103.0 * w.scale)
                                .font(egui::FontId::proportional(8.0 * w.scale))
                                .text_color(Color32::WHITE)
                                .char_limit(50),
                        )
                    });
                if !area.inner.has_focus() {
                    area.inner.request_focus();
                }
                if text != bench.name {
                    actions.push(ScreenAction::AnvilName(text));
                }
            }
            if bench.blocked {
                sheet([176.0, 0.0], [28.0, 21.0], at(99.0, 45.0));
            }
            let (text, color) = match bench.anvil {
                AnvilStatus::Nothing => (None, Color32::WHITE),
                AnvilStatus::TooExpensive => (
                    Some("Too Expensive!".to_owned()),
                    Color32::from_rgb(0xFF, 0x60, 0x60),
                ),
                AnvilStatus::Cost { levels, affordable } => (
                    Some(format!("Enchantment Cost: {levels}")),
                    if affordable {
                        Color32::from_rgb(0x80, 0xFF, 0x20)
                    } else {
                        Color32::from_rgb(0xFF, 0x60, 0x60)
                    },
                ),
            };
            if let Some(text) = text {
                shadowed(
                    w.painter,
                    at(168.0, 69.0),
                    egui::Align2::RIGHT_TOP,
                    &text,
                    color,
                    w.scale,
                );
            }
        }
        Screen::Grindstone if bench.blocked => {
            sheet([176.0, 0.0], [28.0, 21.0], at(92.0, 31.0));
        }
        _ => {}
    }
    tooltip
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

/// The brewing stand's arrow, growing down as the brew goes on, and its
/// blaze bar, shrinking as the fuel burns (1.9+'s `BrewingStandScreen`).
fn paint_brewing_progress(
    painter: &Painter,
    texture: egui::TextureId,
    origin: Pos2,
    scale: f32,
    brewed: f32,
    fuel: f32,
) {
    let arrow = (brewed * 28.0).floor();
    if arrow > 0.0 {
        gui::sheet_image(
            painter,
            texture,
            [176.0, 0.0],
            [9.0, arrow],
            origin + vec2(97.0, 16.0) * scale,
            scale,
        );
    }
    let bar = (fuel * 18.0).ceil();
    if bar > 0.0 {
        gui::sheet_image(
            painter,
            texture,
            [176.0, 29.0],
            [bar, 4.0],
            origin + vec2(60.0, 44.0) * scale,
            scale,
        );
    }
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
