//! Game-wide constants.

/// Window title.
pub const WINDOW_TITLE: &str = "Rustcraft";

/// Initial window size in logical pixels.
pub const WINDOW_WIDTH: u32 = 1280;
pub const WINDOW_HEIGHT: u32 = 720;

/// Daytime sky color (sRGB, 0..1), close to Minecraft 1.0.0's plains sky (#78A7FF).
pub const SKY_COLOR: [f64; 3] = [120.0 / 255.0, 167.0 / 255.0, 1.0];

/// Vertical field of view in degrees (Minecraft's default "Normal" FOV).
pub const FOV_Y_DEGREES: f32 = 70.0;

/// Near and far clip planes in blocks.
pub const Z_NEAR: f32 = 0.05;
pub const Z_FAR: f32 = 1000.0;

/// Free-fly dev camera speed in blocks/s (Minecraft's creative flying speed).
pub const FLY_SPEED: f32 = 10.92;

/// Mouse look in radians per pixel of mouse movement.
pub const MOUSE_SENSITIVITY: f32 = 0.0025;
