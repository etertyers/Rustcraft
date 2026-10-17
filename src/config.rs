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

/// Game logic rate, like Minecraft.
pub const TICKS_PER_SECOND: u32 = 20;

/// Ticks between autosaves: 5 minutes, as CLAUDE.md asks for "every few
/// minutes" (1.0.0 itself saved a few changed chunks every 40 ticks).
pub const AUTOSAVE_TICKS: u32 = 5 * 60 * TICKS_PER_SECOND;

/// Player physics, in Minecraft's units (blocks, ticks) from 1.0.0.
pub const GRAVITY: f64 = 0.08;
/// Vertical velocity kept each tick (air drag).
pub const VERTICAL_DRAG: f64 = 0.98;
pub const JUMP_VELOCITY: f64 = 0.42;
/// Extra horizontal push of a sprint jump.
pub const SPRINT_JUMP_BOOST: f64 = 0.2;
/// Ground acceleration per tick at full input (`landMovementFactor`).
pub const WALK_ACCELERATION: f64 = 0.1;
/// Air acceleration per tick at full input (`jumpMovementFactor`).
pub const AIR_ACCELERATION: f64 = 0.02;
/// Air acceleration per tick while flying in creative.
pub const FLY_ACCELERATION: f64 = 0.05;
/// Vertical speed added per tick holding jump (up) or sneak (down) in flight.
pub const FLY_VERTICAL_SPEED: f64 = 0.15;
/// Vertical speed kept per tick in flight (there's no gravity).
pub const FLY_VERTICAL_DRAG: f64 = 0.6;
/// Acceleration multiplier while sprinting.
pub const SPRINT_FACTOR: f64 = 1.3;
/// Input multiplier while sneaking.
pub const SNEAK_FACTOR: f64 = 0.3;
/// Horizontal velocity kept each tick in the air; on the ground it's this
/// times the block's slipperiness.
pub const AIR_FRICTION: f64 = 0.91;
pub const PLAYER_WIDTH: f64 = 0.6;
pub const PLAYER_HEIGHT: f64 = 1.8;
pub const EYE_HEIGHT: f64 = 1.62;
/// The camera drops this much while sneaking.
pub const SNEAK_EYE_DROP: f64 = 0.08;
pub const STEP_HEIGHT: f64 = 0.5;
/// Block reach in survival, from the eye.
pub const REACH: f64 = 5.0;

/// Free-fly dev camera speed in blocks/s (Minecraft's creative flying speed).
#[cfg(any(debug_assertions, test))]
pub const FLY_SPEED: f32 = 10.92;

/// Mouse look in radians per pixel of mouse movement.
pub const MOUSE_SENSITIVITY: f32 = 0.0025;

/// Chunk width and depth in blocks; chunks are columns of 16³ sections.
pub const CHUNK_SIZE: usize = 16;

/// World height in blocks (Y 0..127), as in 1.0.0.
pub const WORLD_HEIGHT: usize = 128;

/// Sea level, as in 1.0.0.
pub const SEA_LEVEL: i32 = 63;

/// Chunks drawn in each direction around the player.
pub const RENDER_DISTANCE: i32 = 9;
