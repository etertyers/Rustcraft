//! Where a screen's panel and slots are on the window. Drawing, hit
//! testing and the highlight all go through one `Geometry`, in egui
//! points (what the pointer position is in too), so they can't disagree
//! at any window size or scale factor.

use egui::{Pos2, Rect, Vec2, pos2, vec2};

use super::gui;
use crate::inventory::slots::SlotRef;

/// A slot's size in GUI pixels.
pub const SLOT: f32 = 16.0;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Geometry {
    /// Panel top-left, on a whole physical pixel.
    pub origin: Pos2,
    /// Points per GUI pixel; a whole number of physical pixels.
    pub scale: f32,
    /// Panel size in GUI pixels.
    pub size: Vec2,
    /// Physical pixels per point.
    pub ppp: f32,
}

impl Geometry {
    /// A `size` panel centered in `window`, then moved `shift` GUI pixels
    /// right (the recipe book pushes the screen over).
    pub fn new(window: Rect, ppp: f32, size: [f32; 2], shift: f32) -> Self {
        // Minecraft's auto scale, on whole physical pixels per GUI pixel
        // so slot edges fall between screen pixels, not inside them.
        let physical = (gui::gui_scale(window) * ppp).floor().max(1.0);
        let scale = physical / ppp;
        let size = Vec2::from(size);
        let snap = |p: f32| (p * ppp).round() / ppp;
        let top_left = window.center() - size * scale / 2.0 + vec2(shift * scale, 0.0);
        Self {
            origin: pos2(snap(top_left.x), snap(top_left.y)),
            scale,
            size,
            ppp,
        }
    }

    pub fn panel(&self) -> Rect {
        Rect::from_min_size(self.origin, self.size * self.scale)
    }

    /// The rectangle of a `size` region at `at` GUI pixels into the panel.
    pub fn rect(&self, at: [f32; 2], size: [f32; 2]) -> Rect {
        Rect::from_min_size(
            self.origin + vec2(at[0], at[1]) * self.scale,
            vec2(size[0], size[1]) * self.scale,
        )
    }

    /// The 16 × 16 rectangle of a slot at `at`.
    pub fn slot_rect(&self, at: [f32; 2]) -> Rect {
        self.rect(at, [SLOT, SLOT])
    }

    /// The slot of `slots` under `pointer`, if any.
    pub fn slot_at(&self, slots: &[(SlotRef, [f32; 2])], pointer: Pos2) -> Option<SlotRef> {
        slots
            .iter()
            .find(|&&(_, at)| self.slot_rect(at).contains(pointer))
            .map(|&(slot, _)| slot)
    }

    /// GUI pixels into the panel for a point in the window.
    pub fn to_panel(&self, point: Pos2) -> Vec2 {
        (point - self.origin) / self.scale
    }
}

/// Window sizes and scale factors the layout tests run at.
#[cfg(test)]
pub mod tests_support {
    use egui::{Pos2, Rect, vec2};

    /// Window sizes in points: a small window, 720p and a MacBook's
    /// default (what 2560 × 1600 and 2880 × 1800 Retina screens report).
    pub const WINDOWS: [(f32, f32); 3] = [(854.0, 480.0), (1280.0, 720.0), (1440.0, 900.0)];
    /// Physical pixels per point: a plain display and a Retina one.
    pub const FACTORS: [f32; 2] = [1.0, 2.0];

    pub fn window(size: (f32, f32)) -> Rect {
        Rect::from_min_size(Pos2::ZERO, vec2(size.0, size.1))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tests_support::{FACTORS, WINDOWS, window};

    fn on_a_physical_pixel(points: f32, ppp: f32) -> bool {
        ((points * ppp) - (points * ppp).round()).abs() < 1e-3
    }

    #[test]
    fn the_panel_is_centered_and_sits_on_whole_physical_pixels() {
        for size in WINDOWS {
            for ppp in FACTORS {
                let window = window(size);
                let geo = Geometry::new(window, ppp, [176.0, 166.0], 0.0);
                assert!(on_a_physical_pixel(geo.origin.x, ppp));
                assert!(on_a_physical_pixel(geo.origin.y, ppp));
                assert!(on_a_physical_pixel(geo.scale, ppp));
                let off = geo.panel().center() - window.center();
                assert!(
                    off.x.abs() <= 1.0 / ppp && off.y.abs() <= 1.0 / ppp,
                    "{off:?}"
                );
                // The same GUI scale as the HUD, so screens and hotbar agree.
                assert_eq!(geo.scale, gui::gui_scale(window));
            }
        }
    }

    #[test]
    fn a_shift_moves_the_whole_panel_right_by_that_many_gui_pixels() {
        let window = window(WINDOWS[1]);
        let plain = Geometry::new(window, 2.0, [176.0, 166.0], 0.0);
        let shifted = Geometry::new(window, 2.0, [176.0, 166.0], 77.0);
        let moved = (shifted.origin.x - plain.origin.x) / plain.scale;
        assert!((moved - 77.0).abs() < 0.2, "{moved}");
        assert_eq!(shifted.origin.y, plain.origin.y);
    }

    #[test]
    fn every_slot_of_every_screen_is_hit_exactly_where_it_is_drawn() {
        use crate::ui::{
            creative::CreativeTab,
            screens::{Screen, layout, panel_size},
        };
        let screens = [
            Screen::Inventory,
            Screen::Crafting,
            Screen::Chest,
            Screen::Furnace,
            Screen::Brewing,
            Screen::Dispenser { dropper: false },
            Screen::Hopper,
            Screen::Enchanting,
            Screen::Anvil,
            Screen::Grindstone,
            Screen::Smithing,
            Screen::Stonecutter,
            Screen::Merchant,
            Screen::Creative {
                tab: CreativeTab::Items(0),
                scroll: 0.0,
            },
            Screen::Creative {
                tab: CreativeTab::Search,
                scroll: 0.0,
            },
            Screen::Creative {
                tab: CreativeTab::Survival,
                scroll: 0.0,
            },
        ];
        for screen in screens {
            let slots = layout(&screen);
            for size in WINDOWS {
                for ppp in FACTORS {
                    let geo = Geometry::new(window(size), ppp, panel_size(&screen), 0.0);
                    for &(slot, at) in &slots {
                        let rect = geo.slot_rect(at);
                        // The highlight is the slot's rectangle: on whole
                        // physical pixels, 16 GUI pixels square.
                        assert!(on_a_physical_pixel(rect.min.x, ppp));
                        assert!(on_a_physical_pixel(rect.min.y, ppp));
                        assert_eq!(rect.width(), SLOT * geo.scale);
                        // Its middle and its inside corners hit it.
                        let inset = 0.25 / ppp;
                        for point in [
                            rect.center(),
                            rect.min + vec2(inset, inset),
                            rect.max - vec2(inset, inset),
                        ] {
                            assert_eq!(
                                geo.slot_at(&slots, point),
                                Some(slot),
                                "{screen:?} {slot:?} at {size:?} x{ppp}"
                            );
                        }
                        // A pixel past any edge hits something else or nothing.
                        for point in [
                            rect.min - vec2(0.5 / ppp, 0.0),
                            rect.min - vec2(0.0, 0.5 / ppp),
                            rect.max + vec2(0.5 / ppp, 0.0),
                            rect.max + vec2(0.0, 0.5 / ppp),
                        ] {
                            assert_ne!(
                                geo.slot_at(&slots, point),
                                Some(slot),
                                "{screen:?} {slot:?} at {size:?} x{ppp}"
                            );
                        }
                    }
                }
            }
        }
    }
}
