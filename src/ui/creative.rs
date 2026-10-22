//! The creative inventory as modern Minecraft lays it out: tabs along the
//! top and bottom (Building Blocks to Redstone Blocks and Search above,
//! Tools & Utilities to Spawn Eggs and the Survival Inventory below), a
//! 9 × 5 grid that scrolls, and the hotbar under it. The items of each tab
//! come from `assets/data/creative_tabs.ron` (see `inventory::creative`).

use egui::{Color32, Event, Key, Painter, Pos2, Rect, vec2};

use super::{
    geometry::Geometry,
    gui::{self, GuiTextures, SlotView},
    screens::ScreenAction,
};
use crate::inventory::{
    creative::{self, Pick},
    player::{ARMOR_CHEST, ARMOR_FEET, ARMOR_HEAD, ARMOR_LEGS, HOTBAR_SLOTS},
    slots::SlotRef,
    stack::ItemStack,
};

/// A tab of the creative screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CreativeTab {
    /// One of `creative::tabs()`.
    Items(usize),
    Search,
    /// The player's own inventory, with its armor and picture.
    Survival,
}

impl CreativeTab {
    /// Where the tab sits: its row (top or bottom), column and whether
    /// it is pushed to the right edge.
    fn place(self) -> (bool, usize, bool) {
        match self {
            // Five to a row.
            CreativeTab::Items(n) => (n < PER_ROW, n % PER_ROW, false),
            CreativeTab::Search => (true, PER_ROW, true),
            CreativeTab::Survival => (false, PER_ROW, true),
        }
    }

    /// Every tab in the order the keyboard and the screen read them.
    pub fn all() -> impl Iterator<Item = CreativeTab> {
        (0..creative::tabs().len())
            .map(CreativeTab::Items)
            .chain([CreativeTab::Search, CreativeTab::Survival])
    }
}

/// Tabs before the right-aligned one, in each row.
const PER_ROW: usize = 5;
/// A tab's size on the sheet.
const TAB: [f32; 2] = [28.0, 32.0];
/// Panel size.
pub const PANEL: [f32; 2] = [195.0, 136.0];
const COLUMNS: usize = 9;
const ROWS: usize = 5;
const GRID: [f32; 2] = [9.0, 18.0];
const HOTBAR: [f32; 2] = [9.0, 112.0];
const DESTROY: [f32; 2] = [173.0, 112.0];
const SEARCH_BOX: [f32; 2] = [82.0, 6.0];
const SEARCH_SIZE: [f32; 2] = [89.0, 9.0];
const SCROLL_TRACK: ([f32; 2], [f32; 2]) = ([175.0, 18.0], [14.0, 112.0]);
const SCROLLER: [f32; 2] = [12.0, 15.0];

/// One of the table's tabs, ready to draw.
pub struct TabView {
    pub title: String,
    pub icon: SlotView,
    /// What it lists: indices into the catalog.
    pub entries: Vec<usize>,
}

/// Rows the grid can scroll through.
pub fn scroll_rows(entries: usize) -> usize {
    entries.div_ceil(COLUMNS).saturating_sub(ROWS)
}

/// The slots a tab shows besides the grid, and where: the hotbar, and in
/// the Survival Inventory the armor and the 27 slots above it.
pub fn slots(tab: CreativeTab) -> Vec<(SlotRef, [f32; 2])> {
    let hotbar = (0..HOTBAR_SLOTS).map(|i| {
        (
            SlotRef::Inventory(i),
            [HOTBAR[0] + i as f32 * 18.0, HOTBAR[1]],
        )
    });
    if tab != CreativeTab::Survival {
        return hotbar.collect();
    }
    let armor = [
        (ARMOR_HEAD, [9.0, 6.0]),
        (ARMOR_CHEST, [9.0, 33.0]),
        (ARMOR_LEGS, [63.0, 6.0]),
        (ARMOR_FEET, [63.0, 33.0]),
    ]
    .map(|(slot, at)| (SlotRef::Inventory(slot), at));
    let main = (0..27).map(|i| {
        (
            SlotRef::Inventory(HOTBAR_SLOTS + i),
            [9.0 + (i % 9) as f32 * 18.0, 54.0 + (i / 9) as f32 * 18.0],
        )
    });
    hotbar.chain(armor).chain(main).collect()
}

/// Where the player's picture goes in the Survival Inventory (1.12's
/// `drawEntityOnScreen(43, 45, 20, ...)`).
pub const DOLL: super::doll::Spot = super::doll::Spot {
    feet: [43.0, 45.0],
    head: [43.0, 15.0],
    clip: ([28.0, 6.0], [32.0, 43.0]),
    per_block: 20.0,
};

/// The tab's rectangle (the part that is a button).
pub fn tab_rect(geo: &Geometry, tab: CreativeTab) -> Rect {
    let (top, column, right) = tab.place();
    let x = if right {
        PANEL[0] - TAB[0] * (6 - column) as f32
    } else {
        TAB[0] * column as f32
    };
    let y = if top { -28.0 } else { PANEL[1] - 4.0 };
    geo.rect([x, y], TAB)
}

/// Where a tab's picture is on the sheet.
fn tab_source(tab: CreativeTab, selected: bool) -> [f32; 2] {
    let (top, column, _) = tab.place();
    let row = match (top, selected) {
        (true, false) => 0.0,
        (true, true) => 32.0,
        (false, false) => 64.0,
        (false, true) => 96.0,
    };
    [TAB[0] * column as f32, row]
}

/// What the player gave the screen this frame.
pub struct Input {
    pub pointer: Option<Pos2>,
    pub left: bool,
    pub right: bool,
    pub middle: bool,
    pub shift: bool,
    /// Wheel notches (positive scrolls up).
    pub wheel: f32,
    pub left_down: bool,
    pub events: Vec<Event>,
    pub time: f64,
}

/// What a frame of the creative screen did.
#[derive(Default)]
pub struct Out {
    pub actions: Vec<ScreenAction>,
    pub tooltip: Option<String>,
    /// The pointer is over a tab or the grid, so it isn't "outside" the
    /// panel (a click there doesn't throw what's held).
    pub inside: bool,
}

/// The text typed since the last frame, if some of it is a letter: that
/// is what starts a search from any tab.
pub fn letters_typed(events: &[Event]) -> Option<String> {
    let text: String = events
        .iter()
        .filter_map(|e| match e {
            Event::Text(text) => Some(text.as_str()),
            _ => None,
        })
        .flat_map(str::chars)
        .filter(|c| !c.is_control())
        .collect();
    text.chars().any(char::is_alphabetic).then_some(text)
}

/// `text` after this frame's typing and backspaces.
pub fn edit(text: &mut String, events: &[Event]) {
    for event in events {
        match event {
            Event::Text(typed) => text.extend(typed.chars().filter(|c| !c.is_control())),
            Event::Key {
                key: Key::Backspace,
                pressed: true,
                ..
            } => {
                text.pop();
            }
            _ => {}
        }
    }
}

fn search_id() -> egui::Id {
    egui::Id::unique("creative-search-text")
}

/// The search box's text.
pub fn search_text(ctx: &egui::Context) -> String {
    ctx.data(|d| d.get_temp(search_id())).unwrap_or_default()
}

/// Empties the search box (the screen was just opened).
pub fn reset_search(ctx: &egui::Context) {
    ctx.data_mut(|d| d.remove::<String>(search_id()));
}

/// The catalog indices a tab shows.
pub fn entries<'a>(
    tab: CreativeTab,
    tabs: &'a [TabView],
    catalog: &[(ItemStack, SlotView, String)],
    search: &str,
) -> std::borrow::Cow<'a, [usize]> {
    match tab {
        CreativeTab::Items(n) => tabs.get(n).map_or(&[][..], |t| &t.entries).into(),
        // The name is the tooltip's first line; the rest is enchantments
        // and effects.
        CreativeTab::Search => (0..catalog.len())
            .filter(|&i| creative::matches(catalog[i].2.lines().next().unwrap_or(""), search))
            .collect::<Vec<_>>()
            .into(),
        CreativeTab::Survival => (&[][..]).into(),
    }
}

/// The grid slot rectangle at (`column`, `row`).
pub fn grid_rect(geo: &Geometry, column: usize, row: usize) -> Rect {
    geo.slot_rect([GRID[0] + column as f32 * 18.0, GRID[1] + row as f32 * 18.0])
}

/// The tab buttons not selected, drawn under the panel.
pub fn paint_tabs_behind(
    painter: &Painter,
    geo: &Geometry,
    selected: CreativeTab,
    tabs: &[TabView],
    textures: &GuiTextures,
) {
    for tab in CreativeTab::all().filter(|&t| t != selected) {
        paint_tab(painter, geo, tab, false, tabs, textures);
    }
}

fn paint_tab(
    painter: &Painter,
    geo: &Geometry,
    tab: CreativeTab,
    selected: bool,
    tabs: &[TabView],
    textures: &GuiTextures,
) {
    let rect = tab_rect(geo, tab);
    let (top, _, _) = tab.place();
    gui::sheet_image(
        painter,
        textures.tabs.id(),
        tab_source(tab, selected),
        TAB,
        rect.min,
        geo.scale,
    );
    if let Some(view) = tabs.get(view_index(tab, tabs.len())) {
        let at = rect.min + vec2(6.0, if top { 6.0 } else { 10.0 }) * geo.scale;
        gui::paint_slot(painter, at, &view.icon, geo.scale, textures.atlas.id());
    }
}

/// Where `tab` is among the tab views: the table's tabs, then Search's
/// and the Survival Inventory's.
fn view_index(tab: CreativeTab, count: usize) -> usize {
    match tab {
        CreativeTab::Items(n) => n,
        CreativeTab::Search => count.wrapping_sub(2),
        CreativeTab::Survival => count.wrapping_sub(1),
    }
}

/// Draws and handles everything the creative screen has except the real
/// slots (the hotbar and, in the Survival Inventory, the armor and the
/// rest, which `screens::show` does): the selected tab, the grid, the
/// scroll bar, the search box, the destroy slot and the tab buttons.
#[allow(clippy::too_many_arguments)]
pub fn show(
    ctx: &egui::Context,
    painter: &Painter,
    geo: &Geometry,
    tab: &mut CreativeTab,
    scroll: &mut f32,
    catalog: &[(ItemStack, SlotView, String)],
    tabs: &[TabView],
    holding: bool,
    input: &Input,
    textures: &GuiTextures,
) -> Out {
    let mut out = Out::default();
    let pointer = input.pointer;

    // Typing starts a search from any tab, and goes into the box on Search.
    let mut text = search_text(ctx);
    if *tab != CreativeTab::Search
        && let Some(typed) = letters_typed(&input.events)
    {
        *tab = CreativeTab::Search;
        *scroll = 0.0;
        text = typed;
    } else if *tab == CreativeTab::Search {
        edit(&mut text, &input.events);
    }
    ctx.data_mut(|d| d.insert_temp(search_id(), text.clone()));

    // The tab buttons.
    for button in CreativeTab::all() {
        let rect = tab_rect(geo, button);
        if pointer.is_some_and(|p| rect.contains(p)) {
            out.inside = true;
            out.tooltip = tabs
                .get(view_index(button, tabs.len()))
                .map(|t| t.title.clone());
            if input.left && button != *tab {
                *tab = button;
                *scroll = 0.0;
            }
        }
    }

    let shown = entries(*tab, tabs, catalog, &text);
    let rows = scroll_rows(shown.len());
    if let Some(pos) = pointer
        && geo.panel().contains(pos)
    {
        *scroll = (*scroll - input.wheel).clamp(0.0, rows as f32);
    }
    let (track_at, track_size) = SCROLL_TRACK;
    let track = geo.rect(track_at, track_size);
    // Dragging the bar sets the scroll from where the pointer is on it.
    if rows > 0
        && input.left_down
        && let Some(pos) = pointer
        && (track.contains(pos) || input.left && track.contains(pos))
    {
        let fraction = ((pos.y - track.min.y) / track.height()).clamp(0.0, 1.0);
        *scroll = fraction * rows as f32;
    }
    *scroll = scroll.clamp(0.0, rows as f32);
    let top_row = scroll.round() as usize;

    // The title or the search box.
    match *tab {
        CreativeTab::Search => {
            let box_rect = geo.rect(SEARCH_BOX, SEARCH_SIZE);
            let caret = if ((input.time * 2.0) as u64).is_multiple_of(2) {
                "_"
            } else {
                ""
            };
            painter.text(
                box_rect.left_center() + vec2(2.0, 0.0) * geo.scale,
                egui::Align2::LEFT_CENTER,
                format!("{text}{caret}"),
                egui::FontId::proportional(8.0 * geo.scale),
                Color32::WHITE,
            );
        }
        CreativeTab::Items(n) => {
            if let Some(t) = tabs.get(n) {
                paint_title(painter, geo, &t.title);
            }
        }
        CreativeTab::Survival => paint_title(painter, geo, "Survival Inventory"),
    }

    // The scroller: lit when there is more than a screenful.
    let fraction = if rows == 0 {
        0.0
    } else {
        top_row as f32 / rows as f32
    };
    gui::sheet_image(
        painter,
        textures.tabs.id(),
        [if rows == 0 { 244.0 } else { 232.0 }, 0.0],
        SCROLLER,
        geo.origin + vec2(175.0, 18.0 + (SCROLL_TRACK.1[1] - 17.0) * fraction) * geo.scale,
        geo.scale,
    );

    // The grid.
    for row in 0..ROWS {
        for column in 0..COLUMNS {
            let Some(&index) = shown.get((top_row + row) * COLUMNS + column) else {
                continue;
            };
            let (stack, view, name) = &catalog[index];
            let rect = grid_rect(geo, column, row);
            gui::paint_slot(painter, rect.min, view, geo.scale, textures.atlas.id());
            if pointer.is_some_and(|p| rect.contains(p)) {
                out.inside = true;
                painter.rect_filled(rect, 0.0, Color32::from_white_alpha(80));
                out.tooltip = Some(name.clone());
                let pick = if input.middle {
                    Some(Pick::Clone)
                } else if input.left && input.shift {
                    Some(Pick::Shift)
                } else if input.left {
                    Some(Pick::Left)
                } else if input.right {
                    Some(Pick::Right)
                } else {
                    None
                };
                if let Some(pick) = pick {
                    out.actions.push(ScreenAction::CreativePick {
                        stack: stack.clone(),
                        pick,
                    });
                }
            }
        }
    }

    // The destroy slot of the Survival Inventory.
    if *tab == CreativeTab::Survival {
        let rect = geo.slot_rect(DESTROY);
        if pointer.is_some_and(|p| rect.contains(p)) {
            painter.rect_filled(rect, 0.0, Color32::from_white_alpha(80));
            out.tooltip = Some("Destroy Item".to_owned());
            if input.left && (holding || input.shift) {
                out.actions
                    .push(ScreenAction::CreativeDestroy { shift: input.shift });
            }
        }
    }
    // A tab, the grid and the scroll bar are part of the panel.
    out.inside |= pointer.is_some_and(|p| track.contains(p));
    // Selected tab, over the panel's edge.
    paint_tab(painter, geo, *tab, true, tabs, textures);
    out
}

fn paint_title(painter: &Painter, geo: &Geometry, title: &str) {
    painter.text(
        geo.origin + vec2(8.0, 6.0) * geo.scale,
        egui::Align2::LEFT_TOP,
        title,
        egui::FontId::proportional(8.0 * geo.scale),
        Color32::from_gray(0x40),
    );
}

/// Every block and item with how it looks and its tooltip, in the order
/// of the registry (enchanted books, potions and splash potions once for
/// each variant).
pub fn catalog(
    items: &crate::inventory::stack::Items,
    icons: &crate::render::icons::ItemIcons,
) -> Vec<(ItemStack, SlotView, String)> {
    items
        .items
        .iter()
        .flat_map(|def| creative::variants(def.id))
        .map(|stack| {
            let (view, name) = (SlotView::of(&stack, icons, items), items.tooltip(&stack));
            (stack, view, name)
        })
        .collect()
}

/// A stack's identity for finding it in the catalog.
fn key(stack: &ItemStack) -> String {
    format!(
        "{}:{}:{:?}",
        stack.id.index(),
        stack.damage,
        stack.enchantments()
    )
}

/// The tab buttons for `catalog`: the table's tabs with what they list,
/// then Search (a compass) and the Survival Inventory (a chest).
pub fn tab_views(
    items: &crate::inventory::stack::Items,
    icons: &crate::render::icons::ItemIcons,
    catalog: &[(ItemStack, SlotView, String)],
) -> Vec<TabView> {
    use crate::inventory::items as ids;
    let index: std::collections::HashMap<String, usize> = catalog
        .iter()
        .enumerate()
        .map(|(i, (stack, _, _))| (key(stack), i))
        .collect();
    let icon = |id| SlotView::of(&ItemStack::new(id, 1), icons, items);
    let mut views: Vec<TabView> = creative::tabs()
        .iter()
        .map(|tab| TabView {
            title: tab.title.clone(),
            icon: icon(tab.icon),
            entries: tab
                .items
                .iter()
                .flat_map(|&id| creative::variants(id))
                .filter_map(|stack| index.get(&key(&stack)).copied())
                .collect(),
        })
        .collect();
    views.push(TabView {
        title: "Search Items".to_owned(),
        icon: icon(ids::COMPASS),
        entries: Vec::new(),
    });
    views.push(TabView {
        title: "Survival Inventory".to_owned(),
        icon: icon(ids::CHEST),
        entries: Vec::new(),
    });
    views
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::geometry::tests_support::{FACTORS, WINDOWS, window};

    fn typed(text: &str) -> Event {
        Event::Text(text.to_owned())
    }

    fn backspace() -> Event {
        Event::Key {
            key: Key::Backspace,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        }
    }

    #[test]
    fn typing_builds_the_search_text_and_backspace_takes_a_letter_off() {
        let mut text = String::new();
        edit(&mut text, &[typed("o"), typed("a"), typed("k")]);
        assert_eq!(text, "oak");
        edit(&mut text, &[backspace(), typed("e"), typed("\u{7f}")]);
        assert_eq!(text, "oae");
        // E is a letter like the others: the box takes it.
        edit(&mut text, &[typed("e")]);
        assert_eq!(text, "oaee");
    }

    #[test]
    fn letters_start_a_search_but_digits_and_controls_do_not() {
        assert_eq!(letters_typed(&[typed("a")]).as_deref(), Some("a"));
        assert_eq!(letters_typed(&[typed("4")]), None);
        assert_eq!(letters_typed(&[typed("\r")]), None);
        assert_eq!(
            letters_typed(&[typed("o"), typed("k")]).as_deref(),
            Some("ok")
        );
        assert_eq!(letters_typed(&[]), None);
    }

    #[test]
    fn the_grid_scrolls_by_rows_and_the_slots_of_each_tab_are_where_the_sheets_draw_them() {
        assert_eq!(scroll_rows(45), 0);
        assert_eq!(scroll_rows(46), 1);
        assert_eq!(scroll_rows(1592), 177 - 5);
        let hotbar = slots(CreativeTab::Items(0));
        assert_eq!(hotbar.len(), 9);
        assert_eq!(hotbar[8], (SlotRef::Inventory(8), [153.0, 112.0]));
        let survival = slots(CreativeTab::Survival);
        // Hotbar, four armor pieces and 27 more.
        assert_eq!(survival.len(), 9 + 4 + 27);
        assert!(survival.contains(&(SlotRef::Inventory(ARMOR_HEAD), [9.0, 6.0])));
        assert!(survival.contains(&(SlotRef::Inventory(ARMOR_FEET), [63.0, 33.0])));
        assert!(survival.contains(&(SlotRef::Inventory(9), [9.0, 54.0])));
        assert!(survival.contains(&(SlotRef::Inventory(35), [153.0, 90.0])));
    }

    #[test]
    fn tabs_sit_five_to_a_row_with_search_and_the_survival_inventory_at_the_right() {
        use crate::ui::screens::panel_size;
        for size in WINDOWS {
            for ppp in FACTORS {
                let geo = Geometry::new(
                    window(size),
                    ppp,
                    panel_size(&crate::ui::screens::Screen::Creative {
                        tab: CreativeTab::Items(0),
                        scroll: 0.0,
                    }),
                    0.0,
                );
                let panel = geo.panel();
                let mut seen: Vec<Rect> = Vec::new();
                for tab in CreativeTab::all() {
                    let rect = tab_rect(&geo, tab);
                    let (top, _, right) = tab.place();
                    // Top tabs stand above the panel and overlap it by four
                    // pixels; bottom tabs hang below.
                    if top {
                        assert!((rect.min.y - (panel.min.y - 28.0 * geo.scale)).abs() < 0.01);
                    } else {
                        assert!((rect.min.y - (panel.max.y - 4.0 * geo.scale)).abs() < 0.01);
                    }
                    if right {
                        assert!(
                            (rect.max.x - panel.max.x).abs() < 0.01,
                            "{tab:?} flush right"
                        );
                    }
                    assert!(
                        seen.iter().all(|other| !other.intersects(rect.shrink(0.5))),
                        "{tab:?} overlaps"
                    );
                    seen.push(rect);
                }
                assert_eq!(seen.len(), 12);
            }
        }
    }

    /// Runs one frame of the creative screen at 854 × 480 with `events`.
    struct Rig {
        ctx: egui::Context,
        textures: GuiTextures,
        catalog: Vec<(ItemStack, SlotView, String)>,
        tabs: Vec<TabView>,
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
            let items = crate::inventory::stack::Items::new();
            let catalog = catalog(&items, &look.icons);
            let tabs = tab_views(&items, &look.icons, &catalog);
            Self {
                ctx,
                textures,
                catalog,
                tabs,
                time: 0.0,
            }
        }

        fn frame(
            &mut self,
            screen: &mut crate::ui::screens::Screen,
            events: Vec<Event>,
        ) -> Vec<ScreenAction> {
            use crate::ui::screens::{ScreenView, show};
            self.time += 0.05;
            let slot = |_: SlotRef| None;
            let view = ScreenView {
                slot: &slot,
                furnace: None,
                brewing: None,
                cursor: None,
                catalog: &self.catalog,
                tabs: &self.tabs,
                bench: None,
                merchant: None,
                doll: None,
            };
            let input = egui::RawInput {
                screen_rect: Some(window(WINDOWS[0])),
                events,
                time: Some(self.time),
                ..Default::default()
            };
            let mut actions = Vec::new();
            let mut output = self.ctx.run_ui(input, |ui| {
                actions = show(ui.ctx(), screen, &view, &self.textures);
            });
            output.textures_delta.clear();
            actions
        }
    }

    fn click_at(pos: Pos2) -> Vec<Event> {
        let button = |pressed| Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        };
        vec![Event::PointerMoved(pos), button(true)]
    }

    fn creative(tab: CreativeTab) -> crate::ui::screens::Screen {
        crate::ui::screens::Screen::Creative { tab, scroll: 0.0 }
    }

    #[test]
    fn typing_anywhere_switches_to_search_and_the_grid_filters_as_you_type() {
        let mut rig = Rig::new();
        let mut screen = creative(CreativeTab::Items(0));
        rig.frame(&mut screen, vec![typed("o")]);
        assert_eq!(
            screen,
            creative(CreativeTab::Search),
            "a letter opens Search"
        );
        rig.frame(&mut screen, vec![typed("a"), typed("k")]);
        assert_eq!(search_text(&rig.ctx), "oak");
        // The grid now lists only names with a word starting "oak"; clicking
        // the first one picks it.
        let geo = Geometry::new(window(WINDOWS[0]), 1.0, PANEL, 0.0);
        let actions = rig.frame(&mut screen, click_at(grid_rect(&geo, 0, 0).center()));
        let [ScreenAction::CreativePick { stack, pick }] = &actions[..] else {
            panic!("{actions:?}");
        };
        assert_eq!(*pick, Pick::Left);
        let name = rig
            .catalog
            .iter()
            .find(|(s, ..)| s == stack)
            .unwrap()
            .2
            .clone();
        assert!(creative::matches(&name, "oak"), "{name}");
        // More typing narrows it; backspaces widen it again.
        rig.frame(&mut screen, vec![typed(" log")]);
        let narrow = entries(CreativeTab::Search, &rig.tabs, &rig.catalog, "oak log").len();
        let wide = entries(CreativeTab::Search, &rig.tabs, &rig.catalog, "oak").len();
        assert!(0 < narrow && narrow < wide, "{narrow} < {wide}");
        rig.frame(
            &mut screen,
            vec![backspace(), backspace(), backspace(), backspace()],
        );
        assert_eq!(search_text(&rig.ctx), "oak");
    }

    #[test]
    fn clicking_a_tab_selects_it_and_digits_alone_start_no_search() {
        let mut rig = Rig::new();
        let mut screen = creative(CreativeTab::Items(0));
        rig.frame(&mut screen, vec![typed("5")]);
        assert_eq!(screen, creative(CreativeTab::Items(0)));
        let geo = Geometry::new(window(WINDOWS[0]), 1.0, PANEL, 0.0);
        rig.frame(
            &mut screen,
            click_at(tab_rect(&geo, CreativeTab::Items(7)).center()),
        );
        assert_eq!(screen, creative(CreativeTab::Items(7)));
        rig.frame(
            &mut screen,
            click_at(tab_rect(&geo, CreativeTab::Survival).center()),
        );
        assert_eq!(screen, creative(CreativeTab::Survival));
        // The destroy slot takes the held stack (here: nothing held, no action).
        let actions = rig.frame(&mut screen, click_at(geo.slot_rect(DESTROY).center()));
        assert!(actions.is_empty());
    }
}
