//! The fishing bobber (M41): thrown with vanilla's physics, it floats and
//! bobs in water while a fish is lured, bites and dips, and is reeled in
//! for loot. Pure logic: the game gives it the world and the owner.

use glam::{DVec3, IVec3};
use serde::{Deserialize, Serialize};

use crate::{
    inventory::stack::ItemStack,
    world::{block::BlockState, blocks, worldgen::rng::Rng},
};

/// Vanilla's bobber physics, per tick.
pub const GRAVITY: f64 = 0.03;
pub const DRAG: f64 = 0.92;
/// How fast it leaves the rod.
pub const THROW_SPEED: f64 = 0.6;
/// Ticks of waiting before fish come: 5–30 s, less 5 s per level of Lure.
pub const WAIT: (u32, u32) = (100, 600);
pub const LURE_TICKS_PER_LEVEL: u32 = 100;
/// Ticks the fish swims up (bubbles approach), then the window to reel in
/// after the dip (about a second).
pub const APPROACH: (u32, u32) = (20, 80);
pub const NIBBLE: (u32, u32) = (20, 40);
/// Farther from its owner than this and it comes back by itself.
pub const MAX_RANGE: f64 = 32.0;
/// Rod wear: a catch, an entity pulled in, or a hook stuck on land.
pub const WEAR_CATCH: u16 = 1;
pub const WEAR_ENTITY: u16 = 5;

/// What a bobber is stuck to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Hooked {
    Nothing,
    Mob(u64),
    /// Another player, by index (LAN).
    Player(usize),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Bobber {
    /// The player who cast it, by index.
    pub owner: usize,
    pub pos: DVec3,
    pub prev: DVec3,
    pub velocity: DVec3,
    /// Where the line leaves the rod, for drawing.
    pub tip: DVec3,
    /// In the water, floating.
    pub in_water: bool,
    pub hooked: Hooked,
    /// The rod's Lure and Luck of the Sea, as cast.
    pub lure: u8,
    pub luck: u8,
    pub age: u32,
    /// Ticks until fish are lured; then ticks they approach; then ticks
    /// the bite lasts (a nibble: reel in now).
    wait: u32,
    approach: u32,
    pub nibble: u32,
    /// Whether it floats in open water (treasure only there).
    pub open_water: bool,
}

/// What the bobber's tick says happened.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Sign {
    /// Bubbles swim toward the bobber.
    Approach(DVec3),
    /// A fish bites: the bobber dips with a splash.
    Bite,
    /// The fish got away.
    Lost,
}

/// What the world around the bobber is like.
pub trait Surroundings {
    fn is_water(&self, cell: IVec3) -> bool;
    fn is_solid(&self, cell: IVec3) -> bool;
}

impl Bobber {
    /// Thrown from `eye` along `look` (a unit vector).
    pub fn cast(owner: usize, eye: DVec3, look: DVec3, lure: u8, luck: u8, rng: &mut Rng) -> Self {
        let jitter = |rng: &mut Rng| (rng.next_f64() - 0.5) * 0.0206;
        let direction = (look + DVec3::new(jitter(rng), jitter(rng), jitter(rng))).normalize();
        let pos = eye + look * 0.3;
        Self {
            owner,
            pos,
            prev: pos,
            velocity: direction * THROW_SPEED,
            tip: eye,
            in_water: false,
            hooked: Hooked::Nothing,
            lure,
            luck,
            age: 0,
            wait: 0,
            approach: 0,
            nibble: 0,
            open_water: false,
        }
    }

    pub fn cell(&self) -> IVec3 {
        self.pos.floor().as_ivec3()
    }

    /// Moves it a tick: flying and falling until it lands in water (where
    /// it floats at the surface) or on something solid (where it stops).
    pub fn step(&mut self, around: &impl Surroundings) {
        self.prev = self.pos;
        self.age += 1;
        if self.hooked != Hooked::Nothing {
            return;
        }
        let cell = self.cell();
        let wet = around.is_water(cell);
        if wet {
            // Buoyancy: pulled up to the surface (the top of its cell),
            // bobbing, and slowed.
            let surface = f64::from(cell.y) + 0.9;
            self.velocity.y += (surface - self.pos.y) * 0.1;
            self.velocity.y *= 0.8;
            self.velocity.x *= 0.5;
            self.velocity.z *= 0.5;
        } else {
            self.velocity.y -= GRAVITY;
        }
        if !self.in_water && wet {
            self.in_water = true;
            self.velocity.y *= 0.2;
        } else if !wet && self.in_water && !around.is_water(cell - IVec3::Y) {
            self.in_water = false;
        }
        let next = self.pos + self.velocity;
        if around.is_solid(next.floor().as_ivec3()) {
            self.velocity = DVec3::ZERO;
        } else {
            self.pos = next;
        }
        if !wet {
            self.velocity *= DRAG;
        }
    }

    /// One tick of the fishing itself, once it floats: the wait, the fish
    /// approaching, the bite and its window. `rain` makes lures a quarter
    /// quicker.
    pub fn fish(&mut self, rng: &mut Rng, rain: bool) -> Option<Sign> {
        if !self.in_water || self.hooked != Hooked::Nothing {
            return None;
        }
        if self.nibble > 0 {
            self.nibble -= 1;
            if self.nibble == 0 {
                self.wait = 0;
                self.approach = 0;
                return Some(Sign::Lost);
            }
            return None;
        }
        if self.approach > 0 {
            self.approach -= 1;
            if self.approach == 0 {
                self.nibble = between(rng, NIBBLE);
                self.velocity.y -= 0.2;
                return Some(Sign::Bite);
            }
            // Bubbles come closer as it nears.
            let reach = f64::from(self.approach) / 20.0 + 0.3;
            let angle = rng.next_f64() * std::f64::consts::TAU;
            return (self.approach.is_multiple_of(4))
                .then(|| self.pos + DVec3::new(angle.cos() * reach, -0.1, angle.sin() * reach))
                .map(Sign::Approach);
        }
        if self.wait == 0 {
            self.wait = wait_ticks(self.lure, rng);
        }
        self.wait -= 1;
        if rain && rng.below(4) == 0 {
            self.wait = self.wait.saturating_sub(1);
        }
        if self.wait == 0 {
            self.approach = between(rng, APPROACH);
        }
        None
    }
}

fn between(rng: &mut Rng, (lo, hi): (u32, u32)) -> u32 {
    lo + rng.below(hi - lo + 1)
}

/// Ticks before fish are lured: 100–600 less 100 per level of Lure (at
/// least 20, so even Lure III takes a moment).
pub fn wait_ticks(lure: u8, rng: &mut Rng) -> u32 {
    between(rng, WAIT)
        .saturating_sub(u32::from(lure) * LURE_TICKS_PER_LEVEL)
        .max(20)
}

/// Whether the bobber's cell has a 5×4×5 area around it of only water,
/// air and lily pads: the one place treasure is found.
pub fn open_water(get: &impl Fn(IVec3) -> Option<BlockState>, cell: IVec3) -> bool {
    (-2..=2).all(|dx| {
        (-1..=2).all(|dy| {
            (-2..=2).all(|dz| {
                get(cell + IVec3::new(dx, dy, dz))
                    .is_some_and(|b| b.is_air() || b.is(blocks::WATER) || b.is(blocks::LILY_PAD))
            })
        })
    })
}

/// What a catch was.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Fish,
    Junk,
    Treasure,
}

/// Rolls a catch: fish 85, junk 10, treasure 5, each Luck of the Sea level
/// moving 2 weights from junk (and 1 from fish) to treasure. Treasure out
/// of open water is junk instead. Then the item from that table.
pub fn roll_catch(luck: u8, open_water: bool, rng: &mut Rng) -> (Kind, Option<ItemStack>) {
    let luck = u32::from(luck);
    let fish = 85u32.saturating_sub(luck);
    let junk = 10u32.saturating_sub(2 * luck);
    let treasure = 5 + 2 * luck;
    let mut pick = rng.below(fish + junk + treasure);
    let mut kind = if pick < fish {
        Kind::Fish
    } else {
        pick -= fish;
        if pick < junk {
            Kind::Junk
        } else {
            Kind::Treasure
        }
    };
    if kind == Kind::Treasure && !open_water {
        kind = Kind::Junk;
    }
    let table = match kind {
        Kind::Fish => "fishing_fish",
        Kind::Junk => "fishing_junk",
        Kind::Treasure => "fishing_treasure",
    };
    let stack = crate::world::structure::loot::fill(table, rng)
        .into_iter()
        .flatten()
        .next();
    (kind, stack)
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;

    struct Pond(HashMap<IVec3, bool>);

    impl Surroundings for Pond {
        fn is_water(&self, cell: IVec3) -> bool {
            self.0.get(&cell) == Some(&true)
        }
        fn is_solid(&self, cell: IVec3) -> bool {
            self.0.get(&cell) == Some(&false)
        }
    }

    /// Water at y 63 (cells below 64), solid floor at 62 and below.
    fn pond() -> Pond {
        let mut cells = HashMap::new();
        for x in -40..40 {
            for z in -40..40 {
                cells.insert(IVec3::new(x, 63, z), true);
                for y in 40..63 {
                    cells.insert(IVec3::new(x, y, z), false);
                }
            }
        }
        Pond(cells)
    }

    #[test]
    fn a_cast_arcs_out_lands_in_water_and_floats() {
        let pond = pond();
        let mut rng = Rng::new(1);
        let eye = DVec3::new(0.5, 65.6, 0.5);
        let look = DVec3::new(0.0, 0.0, 1.0);
        let mut bobber = Bobber::cast(0, eye, look, 0, 0, &mut rng);
        for _ in 0..200 {
            bobber.step(&pond);
        }
        assert!(bobber.in_water, "{:?}", bobber.pos);
        assert!(bobber.pos.z > 4.0, "it travelled {}", bobber.pos.z);
        assert!(
            (bobber.pos.y - 63.9).abs() < 0.3,
            "it floats at the surface: {}",
            bobber.pos.y
        );
    }

    fn floating() -> Bobber {
        let mut rng = Rng::new(7);
        let mut b = Bobber::cast(0, DVec3::new(0.5, 65.6, 0.5), DVec3::Z, 0, 0, &mut rng);
        b.pos = DVec3::new(0.5, 63.9, 8.5);
        b.in_water = true;
        b
    }

    /// Ticks until the bite, for a bobber with `lure`.
    fn ticks_to_bite(lure: u8, seed: u64, rain: bool) -> u32 {
        let mut rng = Rng::new(seed);
        let mut b = floating();
        b.lure = lure;
        for t in 1..5000 {
            if b.fish(&mut rng, rain) == Some(Sign::Bite) {
                return t;
            }
        }
        panic!("no bite");
    }

    #[test]
    fn a_bite_comes_after_5_to_30_seconds_with_a_window_of_about_a_second() {
        for seed in 0..40 {
            let t = ticks_to_bite(0, seed, false);
            assert!((100 + 20..=600 + 80).contains(&t), "{t}");
        }
        // The window.
        let mut rng = Rng::new(3);
        let mut b = floating();
        let mut bitten = false;
        let mut open = 0;
        for _ in 0..2000 {
            match b.fish(&mut rng, false) {
                Some(Sign::Bite) => bitten = true,
                Some(Sign::Lost) => break,
                _ => {}
            }
            if bitten && b.nibble > 0 {
                open += 1;
            }
        }
        assert!((19..=40).contains(&open), "{open}");
    }

    #[test]
    fn lure_speeds_up_bites_by_five_seconds_a_level_and_rain_a_little() {
        let average =
            |lure, rain| (0..200).map(|s| ticks_to_bite(lure, s, rain)).sum::<u32>() / 200;
        let (none, three) = (average(0, false), average(3, false));
        assert!(none > 380 && none < 460, "average {none}");
        assert!(
            none - three > 200 && none - three < 320,
            "Lure III saves about 300 ticks: {none} → {three}"
        );
        assert!(average(0, true) < none, "rain helps");
    }

    #[test]
    fn a_thousand_catches_follow_the_vanilla_rates_and_luck_shifts_them() {
        let count = |luck, open| {
            let mut rng = Rng::new(99);
            let (mut fish, mut junk, mut treasure) = (0, 0, 0);
            let mut species = HashMap::new();
            for _ in 0..1000 {
                let (kind, stack) = roll_catch(luck, open, &mut rng);
                let stack = stack.expect("every roll gives something");
                match kind {
                    Kind::Fish => {
                        fish += 1;
                        *species.entry(stack.id.name()).or_insert(0) += 1;
                    }
                    Kind::Junk => junk += 1,
                    Kind::Treasure => treasure += 1,
                }
            }
            (fish, junk, treasure, species)
        };
        let (fish, junk, treasure, species) = count(0, true);
        assert!((810..=890).contains(&fish), "fish {fish}");
        assert!((60..=140).contains(&junk), "junk {junk}");
        assert!((25..=80).contains(&treasure), "treasure {treasure}");
        let share = |n: &str| species.get(n).copied().unwrap_or(0) as f64 / fish as f64;
        assert!((0.54..0.66).contains(&share("cod")), "cod {}", share("cod"));
        assert!(
            (0.20..0.30).contains(&share("salmon")),
            "salmon {}",
            share("salmon")
        );
        assert!(
            (0.09..0.17).contains(&share("pufferfish")),
            "puffer {}",
            share("pufferfish")
        );
        assert!(share("tropical_fish") < 0.05);
        // Out of open water the treasure turns to junk.
        let (_, junk_closed, treasure_closed, _) = count(0, false);
        assert_eq!(treasure_closed, 0);
        assert!(junk_closed > junk + 20);
        // Luck of the Sea III: less junk, more treasure.
        let (_, junk_lucky, treasure_lucky, _) = count(3, true);
        assert!(
            junk_lucky < junk && treasure_lucky > treasure + 40,
            "{junk_lucky} {treasure_lucky}"
        );
    }

    #[test]
    fn open_water_needs_only_water_air_or_lily_pads_in_five_by_four_by_five() {
        let mut cells: HashMap<IVec3, BlockState> = HashMap::new();
        for x in -3..=3 {
            for y in -3..=4 {
                for z in -3..=3 {
                    let b = if y <= 0 {
                        BlockState::of(blocks::WATER)
                    } else {
                        BlockState::AIR
                    };
                    cells.insert(IVec3::new(x, y, z), b);
                }
            }
        }
        let get = |p: IVec3| cells.get(&p).copied();
        assert!(open_water(&get, IVec3::ZERO));
        let mut covered = cells.clone();
        covered.insert(IVec3::new(1, 2, 1), BlockState::of(blocks::STONE));
        assert!(
            !open_water(&|p| covered.get(&p).copied(), IVec3::ZERO),
            "a roof"
        );
        let mut pads = cells.clone();
        pads.insert(IVec3::new(1, 1, 1), BlockState::of(blocks::LILY_PAD));
        assert!(
            open_water(&|p| pads.get(&p).copied(), IVec3::ZERO),
            "lily pads are fine"
        );
        let mut bank = cells.clone();
        bank.insert(IVec3::new(2, 0, 0), BlockState::of(blocks::DIRT));
        assert!(
            !open_water(&|p| bank.get(&p).copied(), IVec3::ZERO),
            "a bank beside it"
        );
    }
}
