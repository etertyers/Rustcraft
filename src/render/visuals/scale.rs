//! Render Scale (M68): the 3D world and its post passes are drawn at a share
//! of the window's size and upscaled with a sharp filter before the UI,
//! which is always drawn at full resolution.
//!
//! The setting is `AUTO` (0) or 50 to 100 percent. Auto keeps the world at
//! no more than about 4 million pixels, in steps of a quarter: 100% at
//! 1080p, 75% on a 14-inch MacBook Pro's 3024 × 1964, 50% at 4K.

use std::sync::atomic::{AtomicU64, Ordering};

/// The window's size in pixels as the renderer last set it, for the options
/// screen to say what Auto means (`width << 32 | height`).
static WINDOW: AtomicU64 = AtomicU64::new(1920 << 32 | 1080);

pub fn set_window(width: u32, height: u32) {
    WINDOW.store(
        u64::from(width) << 32 | u64::from(height),
        Ordering::Relaxed,
    );
}

pub fn window() -> (u32, u32) {
    let packed = WINDOW.load(Ordering::Relaxed);
    ((packed >> 32) as u32, packed as u32)
}

/// The setting that picks the scale from the window's size.
pub const AUTO: u32 = 0;
/// The smallest scale the player can choose, in percent.
pub const MIN_PERCENT: u32 = 50;
/// What Auto keeps the world under, in pixels.
pub const AUTO_PIXELS: f64 = 4_000_000.0;

/// The percent Auto picks for a window of `width` × `height`.
pub fn auto_percent(width: u32, height: u32) -> u32 {
    let pixels = f64::from(width.max(1)) * f64::from(height.max(1));
    if pixels <= AUTO_PIXELS {
        return 100;
    }
    let share = (AUTO_PIXELS / pixels).sqrt();
    let quarters = (share * 4.0).floor() as u32;
    (quarters * 25).clamp(MIN_PERCENT, 100)
}

/// The percent a `setting` means in a window of `width` × `height`.
pub fn percent(setting: u32, width: u32, height: u32) -> u32 {
    if setting == AUTO {
        auto_percent(width, height)
    } else {
        setting.clamp(MIN_PERCENT, 100)
    }
}

/// The size the world is drawn at for `percent`.
pub fn scaled_size(width: u32, height: u32, percent: u32) -> (u32, u32) {
    if percent >= 100 {
        return (width, height);
    }
    let scale = |n: u32| ((u64::from(n) * u64::from(percent)).div_ceil(100) as u32).max(1);
    (scale(width), scale(height))
}

/// The sizes a frame is drawn at.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Sizes {
    /// The world and its post passes: the window's size, or with shaders on
    /// a share of it.
    pub scene: (u32, u32),
    /// The UI (and the hand-held HUD): always the window's, whatever the
    /// scale.
    pub ui: (u32, u32),
}

/// The sizes for a `window` with shaders on or off and the Render Scale
/// `setting`. Without shaders the world is drawn straight into the window.
pub fn sizes(window: (u32, u32), shaders: bool, setting: u32) -> Sizes {
    let scene = if shaders {
        scaled_size(window.0, window.1, percent(setting, window.0, window.1))
    } else {
        window
    };
    Sizes { scene, ui: window }
}

/// The next setting in the Video Settings button: Auto, then 100% down to
/// 50% in steps of 10, then Auto again.
pub fn next(setting: u32) -> u32 {
    match setting {
        AUTO => 100,
        s if s > MIN_PERCENT => ((s - 1) / 10 * 10).max(MIN_PERCENT),
        _ => AUTO,
    }
}

/// What the button says: "Auto (75%)" or "80%".
pub fn label(setting: u32, width: u32, height: u32) -> String {
    if setting == AUTO {
        format!("Auto ({}%)", auto_percent(width, height))
    } else {
        format!("{}%", setting.clamp(MIN_PERCENT, 100))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn auto_is_full_at_1080p_and_under_four_million_pixels_on_big_screens() {
        assert_eq!(auto_percent(1920, 1080), 100);
        assert_eq!(auto_percent(2560, 1440), 100);
        // A 14-inch MacBook Pro at its native 3024 × 1964.
        assert_eq!(auto_percent(3024, 1964), 75);
        let (w, h) = scaled_size(3024, 1964, auto_percent(3024, 1964));
        assert!(u64::from(w) * u64::from(h) < 4_100_000, "{w} × {h}");
        // 4K and bigger bottom out at 50%.
        assert_eq!(auto_percent(3840, 2160), 50);
        assert_eq!(auto_percent(7680, 4320), 50);
    }

    #[test]
    fn the_setting_picks_auto_or_a_clamped_percent() {
        assert_eq!(percent(AUTO, 1920, 1080), 100);
        assert_eq!(percent(80, 3024, 1964), 80);
        assert_eq!(percent(10, 1920, 1080), MIN_PERCENT);
        assert_eq!(percent(250, 1920, 1080), 100);
        assert_eq!(scaled_size(1920, 1080, 100), (1920, 1080));
        assert_eq!(scaled_size(1920, 1080, 50), (960, 540));
        assert_eq!(scaled_size(3, 3, 50), (2, 2), "rounds up, never to zero");
    }

    #[test]
    fn the_ui_is_always_drawn_at_the_windows_full_size() {
        for window in [(1920, 1080), (3024, 1964), (3840, 2160), (1, 1)] {
            for setting in [AUTO, 50, 60, 75, 100] {
                for shaders in [false, true] {
                    let sizes = sizes(window, shaders, setting);
                    assert_eq!(sizes.ui, window, "{window:?} at {setting}");
                    assert!(sizes.scene.0 <= window.0 && sizes.scene.1 <= window.1);
                }
            }
        }
        // Without shaders the world is the window; with them it is smaller
        // at 50%.
        assert_eq!(sizes((1920, 1080), false, 50).scene, (1920, 1080));
        assert_eq!(sizes((1920, 1080), true, 50).scene, (960, 540));
    }

    #[test]
    fn the_button_cycles_through_every_choice() {
        let mut seen = vec![AUTO];
        let mut setting = next(AUTO);
        while setting != AUTO {
            seen.push(setting);
            setting = next(setting);
        }
        assert_eq!(seen, [AUTO, 100, 90, 80, 70, 60, 50]);
        assert_eq!(label(AUTO, 3024, 1964), "Auto (75%)");
        assert_eq!(label(80, 1920, 1080), "80%");
    }
}
