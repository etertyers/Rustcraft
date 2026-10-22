//! The recipe book beside the crafting table and the inventory: the green
//! book button, and the panel it opens to the left of the grid with the
//! category tabs, a search box, the "Showing craftable" / "Showing all"
//! toggle and pages of 5 × 4 recipes. Laid out as 1.12's (the sheet
//! `gui/recipe_book`), the screen shifted right 77 pixels while it is open
//! as in 1.21.

use egui::{Color32, Event, Key, Painter, Pos2, Rect, vec2};

use super::{
    creative,
    geometry::Geometry,
    gui::{self, GuiTextures, SlotView},
    screens::ScreenAction,
};
use crate::inventory::book::{Category, Settings};

/// How far the screen moves right when the book opens, in GUI pixels.
const SHIFT: f32 = 77.0;
/// The book panel's left edge from the shifted screen's, and when the
/// window is too narrow to shift (the book covers the screen's left).
const PANEL_X: f32 = -148.0;
const NARROW_PANEL_X: f32 = 14.0;
/// Windows narrower than this many GUI pixels don't shift the screen.
const NARROW: f32 = 379.0;
const PANEL: [f32; 2] = [147.0, 166.0];
const COLUMNS: usize = 5;
const ROWS: usize = 4;
const PER_PAGE: usize = COLUMNS * ROWS;
const BUTTON: f32 = 25.0;
const SEARCH: ([f32; 2], [f32; 2]) = ([25.0, 13.0], [81.0, 14.0]);
const TOGGLE: [f32; 2] = [110.0, 12.0];
const TOGGLE_SIZE: [f32; 2] = [26.0, 16.0];
const GRID: [f32; 2] = [11.0, 31.0];
const BACK: [f32; 2] = [38.0, 137.0];
const FORWARD: [f32; 2] = [93.0, 137.0];
const ARROW: [f32; 2] = [12.0, 17.0];
const TAB: [f32; 2] = [30.0, 26.0];

/// One recipe as the book lists it.
pub struct RecipeEntry {
    /// Its place in the list of every recipe.
    pub index: usize,
    pub icon: SlotView,
    /// What it makes (the tooltip and what the search matches).
    pub name: String,
    /// The player has what it takes.
    pub craftable: bool,
    pub category: Category,
    /// Fits the inventory's 2 × 2 grid.
    pub fits_small: bool,
}

/// What the book shows in the empty cells of the grid after a click on a
/// recipe that can't be made: each missing ingredient's choices.
pub struct Ghost {
    /// (grid cell, the items that fit it).
    pub cells: Vec<(usize, Vec<SlotView>)>,
}

pub struct BookView<'a> {
    pub settings: &'a Settings,
    pub entries: &'a [RecipeEntry],
    pub ghost: Option<&'a Ghost>,
    /// The icon on each category tab, in `Category::ALL`'s order.
    pub tab_icons: &'a [SlotView],
}

/// Which screen has the book: the grid's size and where its button is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Host {
    /// The survival inventory's 2 × 2 grid.
    Inventory,
    /// The crafting table's 3 × 3 grid.
    Table,
}

impl Host {
    pub fn size(self) -> usize {
        match self {
            Host::Inventory => 2,
            Host::Table => 3,
        }
    }

    /// The book button's place on the screen.
    fn button(self) -> [f32; 2] {
        match self {
            Host::Inventory => [104.0, 61.0],
            Host::Table => [5.0, 34.0],
        }
    }

    /// Where the grid's first cell is.
    pub fn grid_origin(self) -> [f32; 2] {
        match self {
            Host::Inventory => [88.0, 26.0],
            Host::Table => [30.0, 17.0],
        }
    }
}

/// How far the screen is shifted right, in GUI pixels: 77 while the book is
/// open, unless the window is narrow.
pub fn shift(open: bool, window: Rect) -> f32 {
    if open && !is_narrow(window) {
        SHIFT
    } else {
        0.0
    }
}

fn is_narrow(window: Rect) -> bool {
    window.width() / gui::gui_scale(window) < NARROW
}

fn panel_x(window: Rect) -> f32 {
    if is_narrow(window) {
        NARROW_PANEL_X
    } else {
        PANEL_X
    }
}

/// The book panel's rectangle.
pub fn panel_rect(geo: &Geometry, window: Rect) -> Rect {
    geo.rect([panel_x(window), 0.0], PANEL)
}

/// The green book button's rectangle.
pub fn button_rect(geo: &Geometry, host: Host) -> Rect {
    geo.rect(host.button(), [20.0, 18.0])
}

/// The button of the `slot`th recipe on a page (row by row).
pub fn recipe_rect(geo: &Geometry, window: Rect, slot: usize) -> Rect {
    let (column, row) = (slot % COLUMNS, slot / COLUMNS);
    geo.rect(
        [
            panel_x(window) + GRID[0] + column as f32 * BUTTON,
            GRID[1] + row as f32 * BUTTON,
        ],
        [BUTTON, BUTTON],
    )
}

fn tab_rect(geo: &Geometry, window: Rect, n: usize) -> Rect {
    geo.rect([panel_x(window) - 30.0, 3.0 + 27.0 * n as f32], TAB)
}

/// Whether the search box has the keyboard (the screen's E key then types).
pub fn search_focused(ctx: &egui::Context) -> bool {
    ctx.data(|d| d.get_temp(focus_id())).unwrap_or(false)
}

fn focus_id() -> egui::Id {
    egui::Id::unique("book-search-focus")
}
fn text_id() -> egui::Id {
    egui::Id::unique("book-search-text")
}
fn page_id() -> egui::Id {
    egui::Id::unique("book-page")
}

/// Forgets the search and page (the screen was just opened).
pub fn reset(ctx: &egui::Context) {
    ctx.data_mut(|d| {
        d.remove::<String>(text_id());
        d.remove::<bool>(focus_id());
        d.remove::<usize>(page_id());
    });
}

/// What the player did with the book this frame.
#[derive(Debug, Clone, PartialEq)]
pub enum BookAction {
    Open(bool),
    Filter(bool),
    Category(Category),
    /// Clicked a recipe that can be made (shift: as many as possible).
    Place {
        index: usize,
        max: bool,
    },
    /// Clicked one that can't: show what's missing.
    Ghost(usize),
}

#[derive(Default)]
pub struct Out {
    pub actions: Vec<ScreenAction>,
    pub tooltip: Option<String>,
    /// The pointer is on the book (its panel, tabs or button).
    pub inside: bool,
}

/// What the player gave the screen this frame (see `creative::Input`).
pub use creative::Input;

/// The entries the book lists now: this category and filter and search,
/// the inventory's only those that fit, craftable ones first.
pub fn listed<'a>(
    entries: &'a [RecipeEntry],
    settings: &Settings,
    host: Host,
    search: &str,
) -> Vec<&'a RecipeEntry> {
    let mut shown: Vec<&RecipeEntry> = entries
        .iter()
        .filter(|e| host.size() >= 3 || e.fits_small)
        .filter(|e| settings.category == Category::All || e.category == settings.category)
        .filter(|e| !settings.craftable_only || e.craftable)
        .filter(|e| crate::inventory::creative::matches(&e.name, search))
        .collect();
    // Stable, so what can be made comes first in the recipes' own order.
    shown.sort_by_key(|e| !e.craftable);
    shown
}

/// Draws the button, the panel when open, and the ghost recipe, and
/// handles their clicks. `grid_free` says whether grid cell `i` is empty.
#[allow(clippy::too_many_arguments)]
pub fn show(
    ctx: &egui::Context,
    painter: &Painter,
    geo: &Geometry,
    window: Rect,
    host: Host,
    view: &BookView,
    grid_free: &dyn Fn(usize) -> bool,
    input: &Input,
    textures: &GuiTextures,
) -> Out {
    let mut out = Out::default();
    let pointer = input.pointer;
    let settings = view.settings;
    let over = |rect: Rect| pointer.is_some_and(|p| rect.contains(p));

    // The book button.
    let button = button_rect(geo, host);
    let hovered = over(button);
    gui::sheet_image(
        painter,
        textures.recipe_button.id(),
        [0.0, if hovered { 19.0 } else { 0.0 }],
        [20.0, 18.0],
        button.min,
        geo.scale,
    );
    if hovered {
        out.inside = true;
        if input.left {
            out.actions
                .push(ScreenAction::Book(BookAction::Open(!settings.open)));
        }
    }
    if !settings.open {
        return out;
    }

    let panel = panel_rect(geo, window);
    out.inside |= over(panel);
    gui::sheet_image(
        painter,
        textures.recipe_book.id(),
        [1.0, 1.0],
        PANEL,
        panel.min,
        geo.scale,
    );
    let at = |x: f32, y: f32| geo.origin + vec2(panel_x(window) + x, y) * geo.scale;

    // The search box: clicking it gives it the keyboard.
    let mut text: String = ctx.data(|d| d.get_temp(text_id())).unwrap_or_default();
    let mut focused = search_focused(ctx);
    let search_rect = geo.rect([panel_x(window) + SEARCH.0[0], SEARCH.0[1]], SEARCH.1);
    if input.left {
        focused = over(search_rect);
    }
    let before = text.clone();
    if focused {
        creative::edit(&mut text, &input.events);
        if input.events.iter().any(|e| {
            matches!(
                e,
                Event::Key {
                    key: Key::Escape,
                    pressed: true,
                    ..
                }
            )
        }) {
            focused = false;
        }
    }
    painter.rect_filled(search_rect, 0.0, Color32::from_rgb(0xa0, 0xa0, 0xa0));
    painter.rect_filled(search_rect.shrink(geo.scale), 0.0, Color32::BLACK);
    let shown_text = if text.is_empty() && !focused {
        ("Search...".to_owned(), Color32::from_gray(0x70))
    } else {
        let caret = if focused && ((input.time * 2.0) as u64).is_multiple_of(2) {
            "_"
        } else {
            ""
        };
        (format!("{text}{caret}"), Color32::WHITE)
    };
    painter.text(
        search_rect.left_center() + vec2(3.0, 0.0) * geo.scale,
        egui::Align2::LEFT_CENTER,
        shown_text.0,
        egui::FontId::proportional(8.0 * geo.scale),
        shown_text.1,
    );
    ctx.data_mut(|d| {
        d.insert_temp(text_id(), text.clone());
        d.insert_temp(focus_id(), focused);
    });

    // The category tabs.
    for (n, category) in Category::ALL.iter().enumerate() {
        let rect = tab_rect(geo, window, n);
        let selected = settings.category == *category;
        let (from, size) = if selected {
            ([188.0, 2.0], [35.0, 26.0])
        } else {
            ([153.0, 2.0], [30.0, 26.0])
        };
        // The selected tab sticks out toward the panel.
        let rect = if selected {
            Rect::from_min_size(rect.min, vec2(size[0], size[1]) * geo.scale)
        } else {
            rect
        };
        gui::sheet_image(
            painter,
            textures.recipe_book.id(),
            from,
            size,
            rect.min,
            geo.scale,
        );
        if let Some(icon) = view.tab_icons.get(n) {
            gui::paint_slot(
                painter,
                rect.min + vec2(7.0, 5.0) * geo.scale,
                icon,
                geo.scale,
                textures.atlas.id(),
            );
        }
        if over(rect) {
            out.inside = true;
            out.tooltip = Some(category.title().to_owned());
            if input.left && !selected {
                out.actions
                    .push(ScreenAction::Book(BookAction::Category(*category)));
            }
        }
    }

    // The craftable filter.
    let toggle = geo.rect([panel_x(window) + TOGGLE[0], TOGGLE[1]], TOGGLE_SIZE);
    gui::sheet_image(
        painter,
        textures.recipe_book.id(),
        [
            if settings.craftable_only {
                180.0
            } else {
                152.0
            },
            if over(toggle) { 59.0 } else { 41.0 },
        ],
        TOGGLE_SIZE,
        toggle.min,
        geo.scale,
    );
    if over(toggle) {
        out.tooltip = Some(
            if settings.craftable_only {
                "Showing craftable"
            } else {
                "Showing all"
            }
            .to_owned(),
        );
        if input.left {
            out.actions.push(ScreenAction::Book(BookAction::Filter(
                !settings.craftable_only,
            )));
        }
    }

    // The recipes, a page at a time.
    let list = listed(view.entries, settings, host, &text);
    let pages = list.len().div_ceil(PER_PAGE).max(1);
    let mut page: usize = ctx.data(|d| d.get_temp(page_id())).unwrap_or(0);
    if text != before {
        page = 0;
    }
    page = page.min(pages - 1);
    if pages > 1 {
        for (forward, spot) in [(false, BACK), (true, FORWARD)] {
            let rect = geo.rect([panel_x(window) + spot[0], spot[1]], ARROW);
            let hot = over(rect);
            gui::sheet_image(
                painter,
                textures.recipe_book.id(),
                [
                    if forward { 15.0 } else { 1.0 },
                    if hot { 226.0 } else { 208.0 },
                ],
                [11.0, 17.0],
                rect.min,
                geo.scale,
            );
            if hot && input.left {
                page = if forward {
                    (page + 1) % pages
                } else {
                    (page + pages - 1) % pages
                };
            }
        }
        painter.text(
            at(73.5, 146.0),
            egui::Align2::CENTER_BOTTOM,
            format!("{}/{pages}", page + 1),
            egui::FontId::proportional(8.0 * geo.scale),
            Color32::from_gray(0x30),
        );
    }
    ctx.data_mut(|d| d.insert_temp(page_id(), page));
    if list.is_empty() {
        painter.text(
            at(PANEL[0] / 2.0, 80.0),
            egui::Align2::CENTER_CENTER,
            if settings.craftable_only {
                "Nothing you can craft yet"
            } else {
                "No recipes"
            },
            egui::FontId::proportional(8.0 * geo.scale),
            Color32::from_gray(0x80),
        );
    }
    for (slot, entry) in list.iter().skip(page * PER_PAGE).take(PER_PAGE).enumerate() {
        let rect = recipe_rect(geo, window, slot);
        // Craftable recipes sit on a plain button, the others on a red one.
        gui::sheet_image(
            painter,
            textures.recipe_book.id(),
            [if entry.craftable { 29.0 } else { 54.0 }, 206.0],
            [BUTTON, BUTTON],
            rect.min,
            geo.scale,
        );
        gui::paint_slot(
            painter,
            rect.min + vec2(4.0, 4.0) * geo.scale,
            &entry.icon,
            geo.scale,
            textures.atlas.id(),
        );
        if over(rect) {
            painter.rect_filled(rect, 0.0, Color32::from_white_alpha(60));
            out.tooltip = Some(if entry.craftable {
                entry.name.clone()
            } else {
                format!("{}\nMissing ingredients", entry.name)
            });
            if input.left || input.right {
                out.actions.push(ScreenAction::Book(if entry.craftable {
                    BookAction::Place {
                        index: entry.index,
                        max: input.shift,
                    }
                } else {
                    BookAction::Ghost(entry.index)
                }));
            }
        }
    }

    // The missing ingredients, faint in the empty cells of the grid.
    if let Some(ghost) = view.ghost {
        let [gx, gy] = host.grid_origin();
        for (cell, choices) in &ghost.cells {
            if !grid_free(*cell) || choices.is_empty() {
                continue;
            }
            let size = host.size();
            let rect = geo.slot_rect([
                gx + (cell % size) as f32 * 18.0,
                gy + (cell / size) as f32 * 18.0,
            ]);
            // Cycle through the items that would do.
            let choice = &choices[(input.time as usize) % choices.len()];
            gui::paint_slot(painter, rect.min, choice, geo.scale, textures.atlas.id());
            painter.rect_filled(
                rect,
                0.0,
                Color32::from_rgba_unmultiplied(0x8b, 0x8b, 0x8b, 0xb0),
            );
        }
    }
    out
}

/// The rectangle a click can fall in without leaving the screen: the
/// book's button, panel and tabs.
pub fn covers(geo: &Geometry, window: Rect, host: Host, open: bool, point: Pos2) -> bool {
    if button_rect(geo, host).contains(point) {
        return true;
    }
    open && (panel_rect(geo, window).contains(point)
        || (0..Category::ALL.len()).any(|n| tab_rect(geo, window, n).expand(2.0).contains(point)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::geometry::tests_support::{FACTORS, WINDOWS, window};

    fn entry(
        index: usize,
        name: &str,
        craftable: bool,
        category: Category,
        small: bool,
    ) -> RecipeEntry {
        RecipeEntry {
            index,
            icon: SlotView {
                icon: crate::render::icons::ItemIcon::Flat(crate::render::icons::IconFace {
                    uv: [0.0; 4],
                    color: [1.0; 3],
                }),
                count: 1,
                durability: None,
                glint: false,
            },
            name: name.to_owned(),
            craftable,
            category,
            fits_small: small,
        }
    }

    #[test]
    fn the_book_lists_by_tab_filter_and_search_with_craftable_ones_first() {
        let entries = [
            entry(0, "Oak Planks", false, Category::Building, true),
            entry(1, "Stick", true, Category::Misc, true),
            entry(2, "Wooden Pickaxe", false, Category::ToolsCombat, false),
            entry(3, "Dark Oak Planks", true, Category::Building, true),
            entry(4, "Piston", true, Category::Redstone, false),
        ];
        let mut settings = Settings::default();
        let names =
            |list: Vec<&RecipeEntry>| -> Vec<usize> { list.iter().map(|e| e.index).collect() };
        // At the table everything is listed, craftable first, in order.
        assert_eq!(
            names(listed(&entries, &settings, Host::Table, "")),
            [1, 3, 4, 0, 2]
        );
        // The inventory lists only what fits its 2 × 2 grid.
        assert_eq!(
            names(listed(&entries, &settings, Host::Inventory, "")),
            [1, 3, 0]
        );
        settings.category = Category::Building;
        assert_eq!(names(listed(&entries, &settings, Host::Table, "")), [3, 0]);
        settings.craftable_only = true;
        assert_eq!(names(listed(&entries, &settings, Host::Table, "")), [3]);
        settings = Settings::default();
        // Search matches the start of any word of the name.
        assert_eq!(
            names(listed(&entries, &settings, Host::Table, "oak")),
            [3, 0]
        );
        assert_eq!(names(listed(&entries, &settings, Host::Table, "pick")), [2]);
    }

    #[test]
    fn the_screen_shifts_77_pixels_when_open_and_not_at_all_in_a_narrow_window() {
        let wide = window((1280.0, 720.0));
        let narrow = window((640.0, 480.0));
        assert_eq!(shift(false, wide), 0.0);
        assert_eq!(shift(true, wide), 77.0);
        assert_eq!(shift(true, narrow), 0.0, "320 GUI pixels is too narrow");
    }

    #[test]
    fn the_book_sits_left_of_the_grid_and_the_button_is_where_vanilla_puts_it() {
        for size in WINDOWS {
            for ppp in FACTORS {
                let win = window(size);
                let open = shift(true, win);
                let geo = Geometry::new(win, ppp, [176.0, 166.0], open);
                let panel = panel_rect(&geo, win);
                if open > 0.0 {
                    // To the left of the screen with a pixel or two between.
                    let gap = (geo.panel().min.x - panel.max.x) / geo.scale;
                    assert!((0.0..=3.0).contains(&gap), "gap {gap}");
                    assert!(panel.min.x >= 0.0, "on screen at {size:?}");
                }
                assert_eq!(panel.height(), 166.0 * geo.scale);
                let button = button_rect(&geo, Host::Inventory);
                assert_eq!(button.min, geo.origin + vec2(104.0, 61.0) * geo.scale);
            }
        }
    }

    /// One frame of the inventory screen at 1280 × 720 with the book.
    struct Rig {
        ctx: egui::Context,
        textures: GuiTextures,
        time: f64,
    }

    impl Rig {
        fn new() -> Self {
            let mobs =
                crate::entity::mobs::MobRegistry::load(&crate::assets::dir().join("data/mobs.ron"))
                    .unwrap();
            let look = crate::look::Look::build(&crate::assets::dir(), Vec::new(), &mobs).unwrap();
            let ctx = egui::Context::default();
            let textures =
                GuiTextures::load(&ctx, look.atlas.image(), look.skin(), &look.res).unwrap();
            Self {
                ctx,
                textures,
                time: 0.0,
            }
        }

        fn frame(
            &mut self,
            settings: &Settings,
            entries: &[RecipeEntry],
            ghost: Option<&Ghost>,
            events: Vec<Event>,
            modifiers: egui::Modifiers,
        ) -> (Vec<ScreenAction>, Vec<String>) {
            use crate::{
                inventory::slots::SlotRef,
                ui::screens::{Screen, ScreenView, show},
            };
            self.time += 0.05;
            let slot = |_: SlotRef| None;
            let view = ScreenView {
                slot: &slot,
                furnace: None,
                brewing: None,
                cursor: None,
                catalog: &[],
                tabs: &[],
                bench: None,
                merchant: None,
                doll: None,
                book: Some(BookView {
                    settings,
                    entries,
                    ghost,
                    tab_icons: &[],
                }),
            };
            let mut all = vec![Event::ModifiersChanged(modifiers)];
            all.extend(events);
            let input = egui::RawInput {
                screen_rect: Some(window(WINDOWS[1])),
                events: all,
                time: Some(self.time),
                ..Default::default()
            };
            let mut actions = Vec::new();
            let mut screen = Screen::Inventory;
            let mut output = self.ctx.run_ui(input, |ui| {
                actions = show(ui.ctx(), &mut screen, &view, &self.textures);
            });
            output.textures_delta.clear();
            let texts = output
                .shapes
                .iter()
                .filter_map(|c| match &c.shape {
                    egui::Shape::Text(t) => Some(t.galley.text().to_owned()),
                    _ => None,
                })
                .collect();
            (actions, texts)
        }
    }

    fn press(pos: Pos2) -> Vec<Event> {
        let button = |pressed| Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        };
        vec![Event::PointerMoved(pos), button(true)]
    }

    #[test]
    fn clicking_recipes_places_or_ghosts_them_and_the_widgets_report_their_state() {
        let mut rig = Rig::new();
        let win = window(WINDOWS[1]);
        let open = Settings {
            open: true,
            ..Settings::default()
        };
        let entries = [
            entry(0, "Stick", true, Category::Misc, true),
            entry(1, "Torch", false, Category::Misc, true),
        ];
        let geo = Geometry::new(win, 1.0, [176.0, 166.0], shift(true, win));
        // A craftable recipe is placed, with shift as many as possible.
        let first = recipe_rect(&geo, win, 0).center();
        let (actions, _) = rig.frame(&open, &entries, None, press(first), egui::Modifiers::NONE);
        assert_eq!(
            actions,
            [ScreenAction::Book(BookAction::Place {
                index: 0,
                max: false
            })]
        );
        let (actions, _) = rig.frame(&open, &entries, None, press(first), egui::Modifiers::SHIFT);
        assert_eq!(
            actions,
            [ScreenAction::Book(BookAction::Place {
                index: 0,
                max: true
            })]
        );
        // One that can't be made shows its missing ingredients instead.
        let second = recipe_rect(&geo, win, 1).center();
        let (actions, _) = rig.frame(&open, &entries, None, press(second), egui::Modifiers::NONE);
        assert_eq!(actions, [ScreenAction::Book(BookAction::Ghost(1))]);
        // The green book closes it; the toggle and tabs change the filter.
        let button = button_rect(&geo, Host::Inventory).center();
        let (actions, _) = rig.frame(&open, &entries, None, press(button), egui::Modifiers::NONE);
        assert_eq!(actions, [ScreenAction::Book(BookAction::Open(false))]);
        let toggle = geo
            .rect(
                [panel_x(win) + TOGGLE[0] + 4.0, TOGGLE[1] + 4.0],
                [4.0, 4.0],
            )
            .center();
        let (actions, _) = rig.frame(&open, &entries, None, press(toggle), egui::Modifiers::NONE);
        assert_eq!(actions, [ScreenAction::Book(BookAction::Filter(true))]);
        let redstone = tab_rect(&geo, win, 4).center();
        let (actions, _) = rig.frame(
            &open,
            &entries,
            None,
            press(redstone),
            egui::Modifiers::NONE,
        );
        assert_eq!(
            actions,
            [ScreenAction::Book(BookAction::Category(Category::Redstone))]
        );
        // Closed, the book draws only its button and nothing is clickable
        // where the panel was.
        let closed = Settings::default();
        let plain = Geometry::new(win, 1.0, [176.0, 166.0], 0.0);
        let (actions, _) = rig.frame(&closed, &entries, None, press(first), egui::Modifiers::NONE);
        assert!(actions.is_empty());
        let (actions, _) = rig.frame(
            &closed,
            &entries,
            None,
            press(button_rect(&plain, Host::Inventory).center()),
            egui::Modifiers::NONE,
        );
        assert_eq!(actions, [ScreenAction::Book(BookAction::Open(true))]);
    }

    #[test]
    fn the_toggle_and_empty_lists_say_what_they_show() {
        let mut rig = Rig::new();
        let win = window(WINDOWS[1]);
        let settings = Settings {
            open: true,
            craftable_only: true,
            ..Settings::default()
        };
        let (_, texts) = rig.frame(&settings, &[], None, vec![], egui::Modifiers::NONE);
        assert!(
            texts.iter().any(|t| t == "Nothing you can craft yet"),
            "{texts:?}"
        );
        let geo = Geometry::new(win, 1.0, [176.0, 166.0], shift(true, win));
        // Hovering the toggle names the state.
        let at = geo
            .rect(
                [panel_x(win) + TOGGLE[0] + 4.0, TOGGLE[1] + 4.0],
                [4.0, 4.0],
            )
            .center();
        let (_, texts) = rig.frame(
            &settings,
            &[],
            None,
            vec![Event::PointerMoved(at)],
            egui::Modifiers::NONE,
        );
        assert!(texts.iter().any(|t| t == "Showing craftable"), "{texts:?}");
        let all = Settings {
            craftable_only: false,
            ..settings
        };
        let (_, texts) = rig.frame(
            &all,
            &[],
            None,
            vec![Event::PointerMoved(at)],
            egui::Modifiers::NONE,
        );
        assert!(texts.iter().any(|t| t == "Showing all"), "{texts:?}");
    }
}
