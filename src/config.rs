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

/// Height of the cloud layer: modern Minecraft's 192 (1.18+) plus 1.0.0's
/// 0.33 offset, above the taller world's mountains.
pub const CLOUD_HEIGHT: f32 = 192.33;

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
/// Hitbox and eye heights when sneaking, and when swimming or crawling
/// (modern `Pose` dimensions).
pub const CROUCH_HEIGHT: f64 = 1.5;
pub const CROUCH_EYE_HEIGHT: f64 = 1.27;
pub const SWIM_HEIGHT: f64 = 0.6;
pub const SWIM_EYE_HEIGHT: f64 = 0.4;
pub const STEP_HEIGHT: f64 = 0.6;
/// Block reach in survival, from the eye.
pub const REACH: f64 = 5.0;

/// Free-fly dev camera speed in blocks/s (Minecraft's creative flying speed).
#[cfg(any(debug_assertions, test))]
pub const FLY_SPEED: f32 = 10.92;

/// Mouse look in radians per pixel of mouse movement.
pub const MOUSE_SENSITIVITY: f32 = 0.0025;

/// Chunk width and depth in blocks; chunks are columns of 16³ sections.
pub const CHUNK_SIZE: usize = 16;

/// The lowest and one past the highest block a chunk holds: modern Y −64
/// to 319 since M34 (0..255 in v1.1, 0..127 in v1.0). The Nether and the
/// End keep their own heights inside it.
pub const MIN_Y: i32 = -64;
pub const MAX_Y: i32 = 320;
/// Blocks in a chunk column (24 sections).
pub const WORLD_HEIGHT: usize = (MAX_Y - MIN_Y) as usize;

/// Sea level, as in 1.0.0.
pub const SEA_LEVEL: i32 = 63;

/// How much bigger than vanilla's the climate noise (and so the biomes)
/// is spread: 1.0 is vanilla, 2.0 doubles every biome's size.
pub const BIOME_SCALE: f64 = 1.0;

/// Chunks drawn in each direction around the player.
pub const RENDER_DISTANCE: i32 = 9;

/// Default biome blend radius in columns: 2 averages grass and foliage
/// colors over 5 × 5 columns (modern's default "Biome Blend: 5x5"); 0 is
/// off.
pub const BIOME_BLEND: i32 = 2;

/// Combat, after modern (1.9+) Java Edition's `Player.attack`. Damage is in
/// health points (half hearts); speeds in swings a second.
pub mod combat {
    /// Damage and speed bare handed, and for any item that isn't a weapon
    /// or tool.
    pub const HAND_DAMAGE: f32 = 1.0;
    pub const HAND_SPEED: f32 = 4.0;
    /// A swing's damage is scaled by `MIN + (1 − MIN) × charge²`.
    pub const MIN_CHARGE_DAMAGE: f32 = 0.2;
    /// A swing must be charged past this for a critical hit, a sweep or
    /// extra knockback from sprinting ("strong").
    pub const STRONG_CHARGE: f32 = 0.9;
    pub const CRIT_MULTIPLIER: f32 = 1.5;
    /// Knockback every hit gives, and the extra of a sprinting strong hit
    /// (vanilla's `i × 0.5` with one level).
    pub const KNOCKBACK: f64 = 0.4;
    pub const SPRINT_KNOCKBACK: f64 = 0.5;
    /// A sword's sweep: damage to others near the target (no Sweeping
    /// Edge), their knockback, how far from the target's hitbox (x, y, z)
    /// and how far from the player they can be.
    pub const SWEEP_DAMAGE: f32 = 1.0;
    pub const SWEEP_KNOCKBACK: f64 = 0.4;
    pub const SWEEP_GROW: (f64, f64, f64) = (1.0, 0.25, 1.0);
    pub const SWEEP_RANGE: f64 = 3.0;
    /// How far the player reaches for mobs, and how much a mob's hitbox
    /// grows for aiming (modern `entityInteractionRange` and pick radius).
    pub const REACH: f64 = 3.0;
    pub const TARGET_GROW: f64 = 0.0;
    /// Exhaustion each attack costs.
    pub const EXHAUSTION: f32 = 0.1;

    /// Enchantments on the weapon (modern `getDamageBonus`): Sharpness
    /// adds a point for the first level and half a point for each more;
    /// Smite and Bane of Arthropods 2.5 a level against their kind.
    pub const SHARPNESS_BASE: f32 = 1.0;
    pub const SHARPNESS_PER_LEVEL: f32 = 0.5;
    pub const SMITE_PER_LEVEL: f32 = 2.5;
    pub const BANE_PER_LEVEL: f32 = 2.5;
    /// Each Knockback level pushes as a sprinting hit does.
    pub const KNOCKBACK_PER_LEVEL: f64 = 0.5;
    /// Fire Aspect sets the target alight for 4 seconds a level.
    pub const FIRE_ASPECT_TICKS: i32 = 80;
    /// Punch pushes the target this much a level.
    pub const PUNCH_PER_LEVEL: f64 = 0.6;
    /// Flame sets what an arrow hits alight for 5 seconds.
    pub const FLAME_TICKS: i32 = 100;

    /// A raised shield blocks once it has been up this long.
    pub const SHIELD_RAISE_TICKS: u32 = 5;
    /// An axe blow puts a raised shield out of use this long (5 s).
    pub const SHIELD_DISABLE_TICKS: u32 = 100;
    /// Blocking a blow of this much or more wears the shield by one plus
    /// the damage.
    pub const SHIELD_WEAR_FROM: f32 = 3.0;
    pub const SHIELD_DURABILITY: u16 = 336;
    /// A blocked melee blow pushes the attacker back this much.
    pub const SHIELD_KNOCKBACK: f64 = 0.5;
}

/// Natural spawning, after modern (1.21) `NaturalSpawner`: the categories and
/// their caps, how often each is tried, and where and in what light a mob
/// may appear. One table for the numbers; `game/mobs.rs` does the rest.
pub mod spawning {
    /// A mob category: its cap per player, and how often its spawn cycle
    /// runs (every `every` ticks of game time).
    pub struct Category {
        pub name: &'static str,
        pub cap: usize,
        pub every: u64,
    }

    pub const MONSTER: Category = Category {
        name: "monster",
        cap: 70,
        every: 1,
    };
    /// Animals: tried only once every 400 ticks.
    pub const CREATURE: Category = Category {
        name: "creature",
        cap: 10,
        every: 400,
    };
    pub const AMBIENT: Category = Category {
        name: "ambient",
        cap: 15,
        every: 1,
    };
    pub const WATER_CREATURE: Category = Category {
        name: "water_creature",
        cap: 5,
        every: 1,
    };
    pub const WATER_AMBIENT: Category = Category {
        name: "water_ambient",
        cap: 20,
        every: 1,
    };
    pub const UNDERGROUND_WATER_CREATURE: Category = Category {
        name: "underground_water_creature",
        cap: 5,
        every: 1,
    };
    pub const AXOLOTLS: Category = Category {
        name: "axolotls",
        cap: 5,
        every: 1,
    };

    /// The cap that applies is `cap × chunks ÷ CAP_CHUNKS`, where `chunks`
    /// are the loaded chunks near a player (17 × 17 with one player).
    pub const CAP_CHUNKS: usize = 289;
    /// Chunks this far from a player (Chebyshev) take part.
    pub const CHUNK_RADIUS: i32 = 8;
    /// Never closer than this to a player or the world spawn, and never
    /// farther than `MAX_DISTANCE` from the player.
    pub const MIN_DISTANCE: f64 = 24.0;
    pub const MAX_DISTANCE: f64 = 128.0;
    /// At each random spot: this many packs, each of at most this many steps
    /// of `rand(0..STEP) − rand(0..STEP)` blocks on x and z.
    pub const PACKS: u32 = 3;
    pub const PACK_STEPS: u32 = 4;
    pub const PACK_STEP: u32 = 6;
    /// Animals need more block or sky light than this.
    pub const ANIMAL_LIGHT_ABOVE: u8 = 8;
    /// Monsters need block light at most this, and a light level at most a
    /// uniform random number below `MONSTER_LIGHT_BELOW`.
    pub const MONSTER_BLOCK_LIGHT: u8 = 0;
    pub const MONSTER_LIGHT_BELOW: u32 = 8;
    /// The chance a new chunk gets a pack of animals; snowy plains and
    /// badlands have their own.
    pub const CHUNK_PACK_CHANCE: f32 = 0.1;
    pub const SNOWY_PLAINS_PACK_CHANCE: f32 = 0.07;
    pub const BADLANDS_PACK_CHANCE: f32 = 0.03;
}

/// Farming and breeding (M40), vanilla's values.
pub mod farming {
    /// Light a crop needs to grow.
    pub const CROP_LIGHT: u8 = 9;
    /// Bone meal advances a crop by this many stages, inclusive.
    pub const BONE_MEAL_STAGES: (u32, u32) = (2, 5);
    /// Ticks in love mode after being fed (30 s).
    pub const LOVE_TICKS: u32 = 600;
    /// Ticks before an animal can breed again (5 minutes).
    pub const BREED_COOLDOWN: u32 = 6000;
    /// Ticks a baby takes to grow up (20 minutes).
    pub const BABY_TICKS: i32 = 24000;
    /// Feeding a baby removes this much of the time it has left.
    pub const FEED_SPEEDUP: f32 = 0.1;
    /// Ticks after a composter fills to level 7 before it is ready.
    pub const COMPOSTER_READY_TICKS: u64 = 20;
    /// Blocks within which two animals in love find each other.
    pub const MATE_RANGE: f64 = 8.0;
    /// Sugar cane and cactus grow to this many blocks.
    pub const STALK_HEIGHT: i32 = 3;
}
