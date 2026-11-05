//! The particle extras (M64), built from what each mod does (its page, its
//! jar's config and sprites), never its code: Visuality's small particles,
//! Cave Dust, Particle Rain's rain, snow, splashes and ripples, Make
//! Bubbles Pop's rising bubbles and Wakes' wakes and splashes.
//!
//! Everything is client-side, made from what this client sees once per
//! game tick, and capped per effect. The pool never touches the server.

use std::collections::HashMap;

use glam::DVec3;

use crate::world::{biome::Biome, block::BlockState, blocks};

/// Most live particles per effect.
pub const CAP_DUST: usize = 250;
pub const CAP_VISUAL: usize = 160;
pub const CAP_RAIN: usize = 1400;
pub const CAP_SPLASH: usize = 260;
pub const CAP_BUBBLES: usize = 220;
pub const CAP_WAKES: usize = 360;

/// How far from the camera effects are made.
const RANGE: f64 = 16.0;

/// Which effect a particle belongs to (for caps and for toggling off).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Dust,
    Visual,
    Rain,
    Splash,
    Bubble,
    Wake,
}

impl Kind {
    pub fn cap(self) -> usize {
        match self {
            Kind::Dust => CAP_DUST,
            Kind::Visual => CAP_VISUAL,
            Kind::Rain => CAP_RAIN,
            Kind::Splash => CAP_SPLASH,
            Kind::Bubble => CAP_BUBBLES,
            Kind::Wake => CAP_WAKES,
        }
    }
}

/// The pictures particles wear. Jar sprites are `fx/<mod>/<name>`; the rest
/// are vanilla's `particle/…`. A sprite whose jar is missing wears a
/// vanilla stand-in (see `Look`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sprite {
    /// Vanilla's soft round puff: dust, rain, snow, foam.
    Puff,
    Bubble,
    BubblePop,
    Splash,
    Sparkle,
    WaterCircle,
    Feather,
    ColdFeather,
    WarmFeather,
    Bone,
    WitherBone,
    Emerald,
    SmallBlob,
    MediumBlob,
    BigBlob,
    SplashCloud,
}

impl Sprite {
    pub const ALL: [Sprite; 16] = [
        Sprite::Puff,
        Sprite::Bubble,
        Sprite::BubblePop,
        Sprite::Splash,
        Sprite::Sparkle,
        Sprite::WaterCircle,
        Sprite::Feather,
        Sprite::ColdFeather,
        Sprite::WarmFeather,
        Sprite::Bone,
        Sprite::WitherBone,
        Sprite::Emerald,
        Sprite::SmallBlob,
        Sprite::MediumBlob,
        Sprite::BigBlob,
        Sprite::SplashCloud,
    ];

    pub fn index(self) -> usize {
        self as usize
    }

    /// The atlas names of its frames, and what to use if the first isn't
    /// there.
    pub fn names(self) -> (Vec<String>, &'static str) {
        use super::jar::frame_names;
        let fx = |m: &str, base: &str, n: u8| {
            frame_names(base, n)
                .into_iter()
                .map(|f| format!("fx/{m}/{f}"))
                .collect()
        };
        let vanilla = |base: &str, n: u8| -> Vec<String> {
            frame_names(base, n)
                .into_iter()
                .map(|f| format!("particle/{f}"))
                .collect()
        };
        let names = match self {
            Sprite::Puff => vanilla("generic_0", 0),
            Sprite::Bubble => vanilla("bubble", 0),
            Sprite::BubblePop => vanilla("bubble_pop", 5),
            Sprite::Splash => vanilla("splash", 4),
            Sprite::Sparkle => fx("visuality", "sparkle", 6),
            Sprite::WaterCircle => fx("visuality", "water_circle", 5),
            Sprite::Feather => fx("visuality", "feather", 0),
            Sprite::ColdFeather => fx("visuality", "cold_feather", 0),
            Sprite::WarmFeather => fx("visuality", "warm_feather", 0),
            Sprite::Bone => fx("visuality", "bone", 0),
            Sprite::WitherBone => fx("visuality", "wither_bone", 0),
            Sprite::Emerald => fx("visuality", "emerald", 0),
            Sprite::SmallBlob => fx("visuality", "small_slime_blob", 4),
            Sprite::MediumBlob => fx("visuality", "medium_slime_blob", 4),
            Sprite::BigBlob => fx("visuality", "big_slime_blob", 4),
            Sprite::SplashCloud => (1..=4)
                .map(|i| format!("fx/wakes/splash_cloud_{i}"))
                .collect(),
        };
        let stand_in = match self {
            Sprite::Sparkle | Sprite::WaterCircle => "particle/glint",
            Sprite::SplashCloud => "particle/generic_0",
            Sprite::Feather
            | Sprite::ColdFeather
            | Sprite::WarmFeather
            | Sprite::Bone
            | Sprite::WitherBone
            | Sprite::Emerald
            | Sprite::SmallBlob
            | Sprite::MediumBlob
            | Sprite::BigBlob => "particle/generic_0",
            _ => "particle/generic_0",
        };
        (names, stand_in)
    }
}

/// Atlas rects of every sprite's frames, by `Sprite::index`.
#[derive(Debug, Clone, Default)]
pub struct FxUvs(Vec<Vec<[f32; 4]>>);

impl FxUvs {
    /// `uv` gives a texture's atlas rect if it's there; a sprite whose
    /// pictures are missing (its jar isn't downloaded) uses its vanilla
    /// stand-in, with one log line.
    pub fn build(uv: impl Fn(&str) -> Option<[f32; 4]>) -> Self {
        let mut all = Vec::new();
        for sprite in Sprite::ALL {
            let (names, stand_in) = sprite.names();
            let mut frames: Vec<[f32; 4]> = names.iter().filter_map(|n| uv(n)).collect();
            if frames.len() != names.len() {
                log::info!("extras: {sprite:?} has no pictures; using {stand_in}");
                frames = uv(stand_in).into_iter().collect();
            }
            all.push(frames);
        }
        Self(all)
    }

    pub fn frames(&self, sprite: Sprite) -> &[[f32; 4]] {
        self.0.get(sprite.index()).map_or(&[], Vec::as_slice)
    }
}

/// How a particle is drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Draw {
    /// Lit and tinted, cut out by the texture's alpha.
    Cutout,
    /// Blended, with its own alpha and no tint (the entity-shadow path).
    Alpha,
}

#[derive(Debug, Clone, PartialEq)]
pub struct FxParticle {
    pub pos: DVec3,
    prev: DVec3,
    vel: DVec3,
    age: u32,
    life: u32,
    pub size: f32,
    grow: f32,
    pub sprite: Sprite,
    pub kind: Kind,
    pub draw: Draw,
    /// Lying flat on the water or ground, turned by `yaw`.
    pub flat: bool,
    pub yaw: f32,
    /// Stretched along its fall (rain), as a multiple of its width.
    pub stretch: f32,
    pub tint: [f32; 3],
    /// Top alpha, for `Draw::Alpha`.
    alpha: f32,
    gravity: f64,
    /// Whether it plays its frames over its life (else it holds the first).
    animate: bool,
    /// Whether it fades in and out.
    fade: bool,
    /// Bubbles: pops at the surface.
    rises: bool,
    /// Rain and snow: stops at the first solid block or water.
    falls: bool,
    snow: bool,
}

impl FxParticle {
    fn base(pos: DVec3, kind: Kind, sprite: Sprite, size: f32, life: u32) -> Self {
        Self {
            pos,
            prev: pos,
            vel: DVec3::ZERO,
            age: 0,
            life,
            size,
            grow: 0.0,
            sprite,
            kind,
            draw: Draw::Cutout,
            flat: false,
            yaw: 0.0,
            stretch: 1.0,
            tint: [1.0; 3],
            alpha: 1.0,
            gravity: 0.0,
            animate: false,
            fade: false,
            rises: false,
            falls: false,
            snow: false,
        }
    }

    pub fn position_between_ticks(&self, alpha: f64) -> DVec3 {
        self.prev.lerp(self.pos, alpha)
    }

    /// Which of its sprite's `frames` it shows now.
    pub fn frame(&self, frames: usize) -> usize {
        if self.animate && frames > 1 {
            ((self.age as usize * frames) / self.life.max(1) as usize).min(frames - 1)
        } else {
            0
        }
    }

    /// Its alpha now (`Draw::Alpha`): in and out over its life.
    pub fn alpha_now(&self) -> f32 {
        if !self.fade {
            return self.alpha;
        }
        let t = self.age as f32 / self.life.max(1) as f32;
        self.alpha * (t * 5.0).min(1.0).min(((1.0 - t) * 2.5).clamp(0.0, 1.0))
    }

    /// Its width now.
    pub fn width(&self, alpha: f32) -> f32 {
        self.size + self.grow * (self.age as f32 + alpha)
    }
}

/// What the effects see of the world.
pub trait World {
    fn block(&self, x: i32, y: i32, z: i32) -> Option<BlockState>;
    /// (sky, block) light.
    fn light(&self, x: i32, y: i32, z: i32) -> (u8, u8);
    fn biome(&self, x: i32, y: i32, z: i32) -> Option<Biome>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BodyKind {
    Player,
    Mob,
    Boat,
}

/// Something that moves through the water.
#[derive(Debug, Clone, Copy)]
pub struct Body {
    pub id: u64,
    pub kind: BodyKind,
    pub feet: DVec3,
    pub velocity: DVec3,
    pub width: f64,
    pub height: f64,
    pub yaw: f32,
    /// Its feet are in water.
    pub in_water: bool,
}

/// A mob that was just hurt.
#[derive(Debug, Clone)]
pub struct Hit {
    pub at: DVec3,
    pub mob: String,
    pub size: u8,
}

/// Which extras are on.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Enabled {
    pub visual: bool,
    pub dust: bool,
    pub rain: bool,
    pub bubbles: bool,
    pub wakes: bool,
}

/// 0 clear, 1 rain, 2 thunder.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sky {
    Clear,
    Rain,
    Thunder,
}

pub struct Input<'a> {
    pub tick: u64,
    pub camera: DVec3,
    pub sky: Sky,
    pub bodies: &'a [Body],
    pub hits: &'a [Hit],
}

#[derive(Debug, Default)]
pub struct Fx {
    particles: Vec<FxParticle>,
    seed: u64,
    counts: [usize; 6],
    was_in_water: HashMap<u64, bool>,
    last_wake: HashMap<u64, u64>,
    /// Where bubbles popped this tick, for the pop sound.
    pub pops: Vec<DVec3>,
}

fn kind_slot(kind: Kind) -> usize {
    kind as usize
}

impl Fx {
    pub fn particles(&self) -> &[FxParticle] {
        &self.particles
    }

    pub fn clear(&mut self) {
        self.particles.clear();
        self.counts = [0; 6];
        self.was_in_water.clear();
        self.last_wake.clear();
    }

    pub fn count(&self, kind: Kind) -> usize {
        self.counts[kind_slot(kind)]
    }

    fn rand(&mut self) -> f32 {
        // xorshift64*
        let mut x = if self.seed == 0 {
            0x9E37_79B9_7F4A_7C15
        } else {
            self.seed
        };
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.seed = x;
        ((x.wrapping_mul(0x2545_F491_4F6C_DD1D) >> 40) as f32) / (1u64 << 24) as f32
    }

    fn range(&mut self, lo: f32, hi: f32) -> f32 {
        lo + (hi - lo) * self.rand()
    }

    fn push(&mut self, p: FxParticle) -> bool {
        let slot = kind_slot(p.kind);
        if self.counts[slot] >= p.kind.cap() {
            return false;
        }
        self.counts[slot] += 1;
        self.particles.push(p);
        true
    }

    /// One game tick: moves what's there, then makes new particles for the
    /// effects that are on.
    pub fn tick(&mut self, world: &impl World, input: &Input, on: Enabled) {
        self.pops.clear();
        self.seed ^= input.tick.wrapping_mul(0x9E37_79B9);
        // Effects switched off lose their particles at once.
        let keep = |k: Kind| match k {
            Kind::Dust => on.dust,
            Kind::Visual => on.visual,
            Kind::Rain | Kind::Splash => on.rain || on.visual,
            Kind::Bubble => on.bubbles,
            Kind::Wake => on.wakes,
        };
        self.advance(world);
        self.particles.retain(|p| keep(p.kind));
        self.recount();
        if on.dust {
            self.cave_dust(world, input);
        }
        if on.rain {
            self.weather(world, input);
        }
        if on.visual {
            self.visuality(world, input, on.rain);
        }
        if on.bubbles {
            self.bubbles(world, input);
        }
        if on.wakes {
            self.wakes(world, input);
        }
        self.recount();
    }

    fn recount(&mut self) {
        self.counts = [0; 6];
        for p in &self.particles {
            self.counts[kind_slot(p.kind)] += 1;
        }
    }

    /// Moves every particle; rain lands, bubbles pop, the rest age out.
    fn advance(&mut self, world: &impl World) {
        let mut landed: Vec<(DVec3, bool)> = Vec::new();
        let mut popped: Vec<DVec3> = Vec::new();
        for p in &mut self.particles {
            p.prev = p.pos;
            p.age += 1;
            p.vel.y -= p.gravity;
            let mut next = p.pos + p.vel;
            if p.snow {
                next.x += f64::from((p.age as f32 * 0.15 + p.yaw).sin()) * 0.012;
            }
            let cell = next.floor().as_ivec3();
            let block = world.block(cell.x, cell.y, cell.z);
            if p.falls {
                match block {
                    Some(b) if b.is(blocks::WATER) => {
                        // Rain on water: a ring on the surface.
                        if !p.snow {
                            landed
                                .push((DVec3::new(next.x, f64::from(cell.y) + 0.88, next.z), true));
                        }
                        p.age = p.life;
                    }
                    Some(b) if !b.is_air() && b.def().solid => {
                        if !p.snow {
                            landed.push((
                                DVec3::new(next.x, f64::from(cell.y) + 1.02, next.z),
                                false,
                            ));
                        }
                        p.age = p.life;
                    }
                    Some(_) => p.pos = next,
                    None => p.age = p.life,
                }
            } else if p.rises {
                match block {
                    Some(b) if b.is(blocks::WATER) => {
                        p.vel.x = f64::from((p.age as f32 * 0.35 + p.yaw).sin()) * 0.012;
                        p.vel.z = f64::from((p.age as f32 * 0.29 + p.yaw).cos()) * 0.012;
                        p.pos = next;
                    }
                    // Out of the water, or into a ceiling: it pops.
                    _ => {
                        popped.push(p.pos);
                        p.age = p.life;
                    }
                }
            } else {
                p.pos = next;
                p.size += p.grow;
                p.vel *= 0.96;
            }
            if p.age + 1 == p.life && p.rises {
                popped.push(p.pos);
            }
        }
        self.particles.retain(|p| p.age < p.life);
        for (at, water) in landed {
            if water {
                self.ripple(at);
            } else {
                self.splash_on_ground(at);
            }
        }
        for at in popped {
            self.pop(at);
        }
        self.recount();
    }

    fn pop(&mut self, at: DVec3) {
        let mut p = FxParticle::base(at, Kind::Bubble, Sprite::BubblePop, 0.16, 5);
        p.animate = true;
        p.tint = [0.85, 0.93, 1.0];
        if self.push(p) {
            self.pops.push(at);
        }
    }

    fn ripple(&mut self, at: DVec3) {
        let mut p = FxParticle::base(at, Kind::Splash, Sprite::WaterCircle, 0.16, 9);
        p.draw = Draw::Alpha;
        p.flat = true;
        p.animate = true;
        p.alpha = 0.7;
        p.fade = true;
        p.grow = 0.025;
        self.push(p);
    }

    fn splash_on_ground(&mut self, at: DVec3) {
        let mut p = FxParticle::base(at + DVec3::Y * 0.04, Kind::Splash, Sprite::Splash, 0.18, 4);
        p.animate = true;
        p.tint = [0.7, 0.8, 1.0];
        self.push(p);
    }

    /// Cave Dust: slow motes in dark, enclosed spaces below sea level, more
    /// the deeper you are. Not in lush caves, which have their own
    /// particles.
    fn cave_dust(&mut self, world: &impl World, input: &Input) {
        let cam = input.camera;
        let cell = cam.floor().as_ivec3();
        if cell.y >= 63 || world.light(cell.x, cell.y, cell.z).0 > 0 {
            return;
        }
        if world.biome(cell.x, cell.y, cell.z) == Some(Biome::LushCaves) {
            return;
        }
        let depth = ((63 - cell.y) as f32 / 127.0).clamp(0.0, 1.0);
        let tries = 1 + (depth * 5.0) as usize;
        for _ in 0..tries {
            if self.rand() > 0.35 + depth * 0.5 {
                continue;
            }
            let at = cam
                + DVec3::new(
                    f64::from(self.range(-10.0, 10.0)),
                    f64::from(self.range(-4.0, 6.0)),
                    f64::from(self.range(-10.0, 10.0)),
                );
            let c = at.floor().as_ivec3();
            if !world.block(c.x, c.y, c.z).is_some_and(BlockState::is_air)
                || world.light(c.x, c.y, c.z).0 > 0
            {
                continue;
            }
            let life = self.range(120.0, 260.0) as u32;
            let size = self.range(0.025, 0.05);
            let mut p = FxParticle::base(at, Kind::Dust, Sprite::Puff, size, life);
            p.draw = Draw::Alpha;
            p.alpha = 0.55;
            p.fade = true;
            p.vel = DVec3::new(
                f64::from(self.range(-0.004, 0.004)),
                f64::from(self.range(-0.001, 0.004)),
                f64::from(self.range(-0.004, 0.004)),
            );
            self.push(p);
        }
    }

    /// Particle Rain: rain and snow as particles that fall from open sky
    /// near the camera, stop at roofs and ground, splash and ripple; light
    /// dust in deserts.
    fn weather(&mut self, world: &impl World, input: &Input) {
        let cam = input.camera;
        let wind_angle = (input.tick as f32 * 0.002).sin() * 1.2;
        let storm = input.sky == Sky::Thunder;
        let wind = if storm { 0.8 } else { 0.4 };
        let wind = DVec3::new(
            f64::from(wind_angle.cos()),
            0.0,
            f64::from(wind_angle.sin()),
        ) * wind
            * 0.12;
        let precipitation = input.sky != Sky::Clear;
        if precipitation {
            let drops = if storm { 46 } else { 28 };
            for _ in 0..drops {
                let angle = self.range(0.0, std::f32::consts::TAU);
                let radius = self.range(0.0, RANGE as f32).sqrt() * (RANGE as f32).sqrt();
                let x = cam.x + f64::from(angle.cos() * radius);
                let z = cam.z + f64::from(angle.sin() * radius);
                let y = cam.y + f64::from(self.range(7.0, 13.0));
                let c = DVec3::new(x, y, z).floor().as_ivec3();
                if !world.block(c.x, c.y, c.z).is_some_and(BlockState::is_air)
                    || world.light(c.x, c.y, c.z).0 < 15
                {
                    continue;
                }
                let Some(biome) = world.biome(c.x, c.y, c.z) else {
                    continue;
                };
                if dry(biome) {
                    continue;
                }
                let at = DVec3::new(x, y, z);
                let mut p;
                if biome.is_cold_at(c.y) {
                    p = FxParticle::base(at, Kind::Rain, Sprite::Puff, self.range(0.04, 0.09), 150);
                    p.vel = DVec3::new(
                        wind.x * 0.3,
                        -f64::from(self.range(0.06, 0.1)),
                        wind.z * 0.3,
                    );
                    p.snow = true;
                    p.yaw = self.range(0.0, std::f32::consts::TAU);
                } else {
                    p = FxParticle::base(at, Kind::Rain, Sprite::Puff, 0.05, 40);
                    p.vel = DVec3::new(wind.x, -f64::from(self.range(0.8, 1.0)), wind.z);
                    p.stretch = 12.0;
                    p.tint = [0.8, 0.88, 1.0];
                }
                p.falls = true;
                self.push(p);
            }
        }
        // Sand blowing over deserts, sparse, harder in a storm.
        let cell = cam.floor().as_ivec3();
        if world.biome(cell.x, cell.y, cell.z).is_some_and(desert)
            && self.rand() < if storm { 0.9 } else { 0.25 }
        {
            let at = cam
                + DVec3::new(
                    f64::from(self.range(-12.0, 12.0)),
                    f64::from(self.range(-1.0, 2.0)),
                    f64::from(self.range(-12.0, 12.0)),
                );
            let c = at.floor().as_ivec3();
            if world.block(c.x, c.y, c.z).is_some_and(BlockState::is_air) {
                let mut p = FxParticle::base(at, Kind::Rain, Sprite::Puff, 0.1, 50);
                p.vel = wind * 2.5 + DVec3::new(0.0, 0.002, 0.0);
                p.tint = [0.86, 0.76, 0.52];
                self.push(p);
            }
        }
    }

    /// Visuality: sparkles on ores, particles when mobs are hit, rings on
    /// water in rain (when Particle Rain isn't making them).
    fn visuality(&mut self, world: &impl World, input: &Input, rain_extra: bool) {
        let cam = input.camera;
        for _ in 0..8 {
            let at = cam
                + DVec3::new(
                    f64::from(self.range(-9.0, 9.0)),
                    f64::from(self.range(-6.0, 6.0)),
                    f64::from(self.range(-9.0, 9.0)),
                );
            let c = at.floor().as_ivec3();
            let Some(block) = world.block(c.x, c.y, c.z) else {
                continue;
            };
            if !shiny(block) {
                continue;
            }
            let dir = [
                (1, 0, 0),
                (-1, 0, 0),
                (0, 1, 0),
                (0, -1, 0),
                (0, 0, 1),
                (0, 0, -1),
            ][(self.rand() * 5.99) as usize];
            let n = (c.x + dir.0, c.y + dir.1, c.z + dir.2);
            if !world.block(n.0, n.1, n.2).is_some_and(BlockState::is_air) {
                continue;
            }
            let center = DVec3::new(
                f64::from(c.x) + 0.5,
                f64::from(c.y) + 0.5,
                f64::from(c.z) + 0.5,
            );
            let face = DVec3::new(f64::from(dir.0), f64::from(dir.1), f64::from(dir.2));
            let jitter = DVec3::new(
                f64::from(self.range(-0.4, 0.4)),
                f64::from(self.range(-0.4, 0.4)),
                f64::from(self.range(-0.4, 0.4)),
            );
            let pos = center + face * 0.55 + jitter - face * jitter.dot(face).abs();
            let mut p = FxParticle::base(pos, Kind::Visual, Sprite::Sparkle, 0.12, 12);
            p.animate = true;
            self.push(p);
        }
        for hit in input.hits {
            self.hit_particles(hit, world);
        }
        if input.sky != Sky::Clear && !rain_extra {
            for _ in 0..3 {
                let at = cam
                    + DVec3::new(
                        f64::from(self.range(-10.0, 10.0)),
                        f64::from(self.range(-3.0, 3.0)),
                        f64::from(self.range(-10.0, 10.0)),
                    );
                let c = at.floor().as_ivec3();
                if world
                    .block(c.x, c.y, c.z)
                    .is_some_and(|b| b.is(blocks::WATER))
                    && world
                        .block(c.x, c.y + 1, c.z)
                        .is_some_and(BlockState::is_air)
                    && world.light(c.x, c.y + 1, c.z).0 == 15
                {
                    let ring = DVec3::new(at.x, f64::from(c.y) + 0.88, at.z);
                    self.ripple(ring);
                }
            }
        }
    }

    fn hit_particles(&mut self, hit: &Hit, world: &impl World) {
        let cold = world
            .biome(hit.at.x as i32, hit.at.y as i32, hit.at.z as i32)
            .is_some_and(|b| b.is_cold_at(hit.at.y as i32));
        let sprite = match hit.mob.as_str() {
            "chicken" if cold => Sprite::ColdFeather,
            "chicken" => Sprite::Feather,
            "skeleton" | "stray" => Sprite::Bone,
            "wither_skeleton" => Sprite::WitherBone,
            "villager" | "wandering_trader" => Sprite::Emerald,
            "slime" | "magma_cube" => match hit.size {
                0 | 1 => Sprite::SmallBlob,
                2 => Sprite::MediumBlob,
                _ => Sprite::BigBlob,
            },
            _ => return,
        };
        for _ in 0..5 {
            let mut p = FxParticle::base(hit.at, Kind::Visual, sprite, self.range(0.1, 0.18), 30);
            p.gravity = 0.03;
            p.vel = DVec3::new(
                f64::from(self.range(-0.12, 0.12)),
                f64::from(self.range(0.05, 0.2)),
                f64::from(self.range(-0.12, 0.12)),
            );
            p.animate = matches!(
                sprite,
                Sprite::SmallBlob | Sprite::MediumBlob | Sprite::BigBlob
            );
            self.push(p);
        }
    }

    /// Make Bubbles Pop: things moving under water leave bubbles that wobble
    /// up to the surface and pop there (their lifetime is longer than
    /// vanilla's so they get there).
    fn bubbles(&mut self, world: &impl World, input: &Input) {
        for body in input.bodies {
            if !body.in_water || body.feet.distance(input.camera) > 24.0 {
                continue;
            }
            let speed = body.velocity.x.hypot(body.velocity.z) + body.velocity.y.abs() * 0.5;
            if speed < 0.06 || self.rand() > 0.3 {
                continue;
            }
            let at = body.feet + DVec3::new(0.0, body.height * 0.8, 0.0);
            let c = at.floor().as_ivec3();
            if !world
                .block(c.x, c.y, c.z)
                .is_some_and(|b| b.is(blocks::WATER))
            {
                continue;
            }
            let mut p = FxParticle::base(
                at,
                Kind::Bubble,
                Sprite::Bubble,
                self.range(0.05, 0.1),
                self.range(90.0, 170.0) as u32,
            );
            p.vel = DVec3::new(0.0, f64::from(self.range(0.06, 0.1)), 0.0);
            p.yaw = self.range(0.0, std::f32::consts::TAU);
            p.rises = true;
            p.tint = [0.8, 0.9, 1.0];
            self.push(p);
        }
    }

    /// Wakes: a trail of foam on the water behind boats, swimmers and mobs,
    /// and a splash of cloud when something falls in fast.
    fn wakes(&mut self, world: &impl World, input: &Input) {
        let mut seen: Vec<u64> = Vec::new();
        for body in input.bodies {
            seen.push(body.id);
            let was = self
                .was_in_water
                .insert(body.id, body.in_water)
                .unwrap_or(body.in_water);
            if !body.in_water || body.feet.distance(input.camera) > 40.0 {
                continue;
            }
            let c = (body.feet + DVec3::Y * 0.2).floor().as_ivec3();
            // The surface: up through the water to its top.
            let mut top = c.y;
            while world
                .block(c.x, top + 1, c.z)
                .is_some_and(|b| b.is(blocks::WATER))
                && top < c.y + 8
            {
                top += 1;
            }
            if world.block(c.x, top + 1, c.z).is_none() {
                continue;
            }
            let surface = f64::from(top) + 0.88;
            if !was && body.velocity.y < -0.25 {
                for _ in 0..6 {
                    let mut p = FxParticle::base(
                        DVec3::new(body.feet.x, surface, body.feet.z),
                        Kind::Wake,
                        Sprite::SplashCloud,
                        self.range(0.3, 0.5),
                        self.range(14.0, 22.0) as u32,
                    );
                    p.draw = Draw::Alpha;
                    p.alpha = 0.6;
                    p.fade = true;
                    p.animate = true;
                    p.grow = 0.02;
                    p.vel = DVec3::new(
                        f64::from(self.range(-0.05, 0.05)),
                        f64::from(self.range(0.04, 0.1)),
                        f64::from(self.range(-0.05, 0.05)),
                    );
                    self.push(p);
                }
            }
            let speed = body.velocity.x.hypot(body.velocity.z);
            let last = self.last_wake.get(&body.id).copied().unwrap_or(0);
            if speed < 0.04 || input.tick < last + 2 {
                continue;
            }
            self.last_wake.insert(body.id, input.tick);
            let heading = DVec3::new(body.velocity.x, 0.0, body.velocity.z).normalize_or_zero();
            let side = DVec3::new(-heading.z, 0.0, heading.x);
            let behind = DVec3::new(body.feet.x, surface, body.feet.z) - heading * body.width * 0.5;
            // A boat leaves two lines along its sides; others one in the middle.
            let offsets: &[f64] = if body.kind == BodyKind::Boat {
                &[-0.55, 0.55]
            } else {
                &[0.0]
            };
            for offset in offsets {
                let mut p = FxParticle::base(
                    behind + side * offset * body.width * 1.6,
                    Kind::Wake,
                    Sprite::Puff,
                    (body.width * 0.45) as f32,
                    50,
                );
                p.draw = Draw::Alpha;
                p.flat = true;
                p.alpha = 0.4;
                p.fade = true;
                p.grow = 0.018;
                p.yaw = heading.x.atan2(heading.z) as f32;
                self.push(p);
            }
        }
        self.was_in_water.retain(|id, _| seen.contains(id));
        self.last_wake.retain(|id, _| seen.contains(id));
    }
}

/// Biomes with no rain or snow.
fn dry(biome: Biome) -> bool {
    matches!(biome, Biome::Desert | Biome::Badlands | Biome::Savanna)
}

fn desert(biome: Biome) -> bool {
    matches!(biome, Biome::Desert | Biome::Badlands)
}

/// Ores and gem blocks that sparkle (Visuality).
fn shiny(block: BlockState) -> bool {
    let name = &block.def().name;
    name.ends_with("_ore")
        || name.ends_with("_ore_deepslate")
        || matches!(
            name.as_str(),
            "diamond_block"
                | "emerald_block"
                | "gold_block"
                | "amethyst_block"
                | "budding_amethyst"
        )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::block::Fluid;
    use std::collections::HashMap;

    /// A map of blocks, with sky light 15 above `roof` (none: everywhere
    /// above y 60) and 0 below it.
    struct Map {
        blocks: HashMap<(i32, i32, i32), BlockState>,
        biome: Biome,
        sky_below: Option<i32>,
    }

    impl Map {
        fn open() -> Self {
            Self {
                blocks: HashMap::new(),
                biome: Biome::Plains,
                sky_below: None,
            }
        }
        fn floor(mut self, y: i32) -> Self {
            for x in -20..20 {
                for z in -20..20 {
                    self.blocks.insert((x, y, z), BlockState::of(blocks::STONE));
                }
            }
            self
        }
    }

    impl World for Map {
        fn block(&self, x: i32, y: i32, z: i32) -> Option<BlockState> {
            if y < -64 {
                return None;
            }
            Some(*self.blocks.get(&(x, y, z)).unwrap_or(&BlockState::AIR))
        }
        fn light(&self, _: i32, y: i32, _: i32) -> (u8, u8) {
            match self.sky_below {
                Some(roof) if y < roof => (0, 0),
                _ => (15, 0),
            }
        }
        fn biome(&self, _: i32, _: i32, _: i32) -> Option<Biome> {
            Some(self.biome)
        }
    }

    fn on(f: impl Fn(&mut Enabled)) -> Enabled {
        let mut e = Enabled::default();
        f(&mut e);
        e
    }

    fn input<'a>(tick: u64, camera: DVec3, sky: Sky, bodies: &'a [Body]) -> Input<'a> {
        Input {
            tick,
            camera,
            sky,
            bodies,
            hits: &[],
        }
    }

    fn run(fx: &mut Fx, world: &Map, camera: DVec3, sky: Sky, on: Enabled, ticks: u64) {
        for t in 0..ticks {
            fx.tick(world, &input(t, camera, sky, &[]), on);
        }
    }

    #[test]
    fn rain_falls_in_the_rain_only_stays_under_the_cap_and_stops_at_a_roof() {
        let world = Map::open().floor(63);
        let cam = DVec3::new(0.5, 64.5, 0.5);
        let rain = on(|e| e.rain = true);
        let mut dry = Fx::default();
        run(&mut dry, &world, cam, Sky::Clear, rain, 40);
        assert_eq!(dry.count(Kind::Rain), 0, "no rain in clear weather");
        let mut wet = Fx::default();
        run(&mut wet, &world, cam, Sky::Thunder, rain, 120);
        assert!(wet.count(Kind::Rain) > 100);
        assert!(wet.count(Kind::Rain) <= CAP_RAIN && wet.count(Kind::Splash) <= CAP_SPLASH);
        assert!(wet.count(Kind::Splash) > 0, "drops splash on the floor");
        // Under a roof at y 70 with sky light only above it: nothing falls
        // through the roof, so none is below it.
        let mut roofed = Map::open().floor(63);
        for x in -20..20 {
            for z in -20..20 {
                roofed
                    .blocks
                    .insert((x, 70, z), BlockState::of(blocks::STONE));
            }
        }
        roofed.sky_below = Some(71);
        let mut under = Fx::default();
        run(&mut under, &roofed, cam, Sky::Rain, rain, 120);
        let below = under
            .particles()
            .iter()
            .filter(|p| p.kind == Kind::Rain && p.pos.y < 70.0)
            .count();
        assert_eq!(below, 0, "no rain particles under a roof");
    }

    #[test]
    fn cold_biomes_make_snow_and_deserts_no_rain() {
        let cam = DVec3::new(0.5, 64.5, 0.5);
        let rain = on(|e| e.rain = true);
        let mut snowy = Map::open().floor(63);
        snowy.biome = Biome::SnowyPlains;
        let mut fx = Fx::default();
        run(&mut fx, &snowy, cam, Sky::Rain, rain, 20);
        assert!(fx.particles().iter().all(|p| p.snow) && fx.count(Kind::Rain) > 0);
        let mut desert = Map::open().floor(63);
        desert.biome = Biome::Desert;
        let mut fx = Fx::default();
        run(&mut fx, &desert, cam, Sky::Rain, rain, 20);
        assert!(fx.particles().iter().all(|p| !p.falls), "only blown sand");
    }

    #[test]
    fn cave_dust_needs_darkness_depth_and_no_lush_caves() {
        let cam = DVec3::new(0.5, 20.5, 0.5);
        let dust = on(|e| e.dust = true);
        let mut cave = Map::open();
        cave.sky_below = Some(100);
        let mut fx = Fx::default();
        run(&mut fx, &cave, cam, Sky::Clear, dust, 80);
        assert!(fx.count(Kind::Dust) > 0 && fx.count(Kind::Dust) <= CAP_DUST);
        // Daylight (sky light at the camera): none.
        let mut daylit = Fx::default();
        run(&mut daylit, &Map::open(), cam, Sky::Clear, dust, 80);
        assert_eq!(daylit.count(Kind::Dust), 0);
        // Above sea level: none even in the dark.
        let mut high = Fx::default();
        let mut roofed = Map::open();
        roofed.sky_below = Some(200);
        run(
            &mut high,
            &roofed,
            DVec3::new(0.5, 80.5, 0.5),
            Sky::Clear,
            dust,
            80,
        );
        assert_eq!(high.count(Kind::Dust), 0);
        // Lush caves: none.
        let mut lush = cave;
        lush.biome = Biome::LushCaves;
        let mut fx = Fx::default();
        run(&mut fx, &lush, cam, Sky::Clear, dust, 80);
        assert_eq!(fx.count(Kind::Dust), 0);
        // Deeper is dustier.
        let deep = |y: f64| {
            let mut m = Map::open();
            m.sky_below = Some(100);
            let mut fx = Fx::default();
            run(&mut fx, &m, DVec3::new(0.5, y, 0.5), Sky::Clear, dust, 40);
            fx.count(Kind::Dust)
        };
        assert!(deep(-50.5) > deep(55.5));
    }

    fn lake() -> Map {
        let mut map = Map::open().floor(55);
        for x in -20..20 {
            for z in -20..20 {
                for y in 56..=62 {
                    map.blocks
                        .insert((x, y, z), crate::world::fluid::at_level(Fluid::Water, 0));
                }
            }
        }
        map
    }

    fn swimmer(id: u64, feet: DVec3, velocity: DVec3, kind: BodyKind) -> Body {
        Body {
            id,
            kind,
            feet,
            velocity,
            width: if kind == BodyKind::Boat { 1.4 } else { 0.6 },
            height: 1.8,
            yaw: 0.0,
            in_water: true,
        }
    }

    #[test]
    fn bubbles_rise_pop_at_the_surface_and_wakes_only_form_on_water() {
        let world = lake();
        let cam = DVec3::new(0.5, 64.0, 0.5);
        let mover = swimmer(
            1,
            DVec3::new(2.0, 57.0, 2.0),
            DVec3::new(0.1, 0.0, 0.0),
            BodyKind::Player,
        );
        let mut fx = Fx::default();
        let enabled = on(|e| {
            e.bubbles = true;
            e.wakes = true;
        });
        let mut popped = 0;
        let mut highest: f64 = 0.0;
        for t in 0..200 {
            fx.tick(
                &world,
                &input(t, cam, Sky::Clear, std::slice::from_ref(&mover)),
                enabled,
            );
            popped += fx.pops.len();
            for p in fx
                .particles()
                .iter()
                .filter(|p| p.kind == Kind::Bubble && p.rises)
            {
                highest = highest.max(p.pos.y);
            }
        }
        assert!(popped > 0, "bubbles pop");
        assert!(
            highest > 60.0 && highest < 63.2,
            "they rise to the surface: {highest}"
        );
        assert!(fx.count(Kind::Bubble) <= CAP_BUBBLES);
        // Wake decals sit on the water's surface.
        assert!(fx.count(Kind::Wake) > 0);
        assert!(
            fx.particles()
                .iter()
                .filter(|p| p.kind == Kind::Wake)
                .all(|p| (p.pos.y - 62.88).abs() < 0.2 || !p.flat)
        );
        // On dry ground: no wake, no bubbles.
        let ground = Map::open().floor(63);
        let walker = Body {
            in_water: false,
            ..mover
        };
        let mut dry = Fx::default();
        for t in 0..60 {
            dry.tick(
                &ground,
                &input(t, cam, Sky::Clear, std::slice::from_ref(&walker)),
                enabled,
            );
        }
        assert_eq!(dry.count(Kind::Wake) + dry.count(Kind::Bubble), 0);
    }

    #[test]
    fn a_boat_leaves_two_lines_and_a_fast_fall_makes_a_splash_cloud() {
        let world = lake();
        let cam = DVec3::new(0.5, 64.0, 0.5);
        let boat = swimmer(
            2,
            DVec3::new(3.0, 62.0, 3.0),
            DVec3::new(0.0, 0.0, 0.2),
            BodyKind::Boat,
        );
        let mut fx = Fx::default();
        let wakes = on(|e| e.wakes = true);
        for t in 0..4 {
            fx.tick(
                &world,
                &input(t, cam, Sky::Clear, std::slice::from_ref(&boat)),
                wakes,
            );
        }
        let xs: Vec<f64> = fx
            .particles()
            .iter()
            .filter(|p| p.flat)
            .map(|p| p.pos.x)
            .collect();
        assert!(
            xs.iter().any(|x| *x < 3.0) && xs.iter().any(|x| *x > 3.0),
            "{xs:?}"
        );
        let mut fx = Fx::default();
        let mut falling = swimmer(
            3,
            DVec3::new(1.0, 70.0, 1.0),
            DVec3::new(0.0, -0.8, 0.0),
            BodyKind::Mob,
        );
        falling.in_water = false;
        fx.tick(
            &world,
            &input(0, cam, Sky::Clear, std::slice::from_ref(&falling)),
            wakes,
        );
        falling.in_water = true;
        falling.feet.y = 61.0;
        fx.tick(
            &world,
            &input(1, cam, Sky::Clear, std::slice::from_ref(&falling)),
            wakes,
        );
        assert!(
            fx.particles()
                .iter()
                .any(|p| p.sprite == Sprite::SplashCloud)
        );
    }

    #[test]
    fn sparkles_land_on_ores_hits_scatter_by_mob_and_toggling_off_clears_each() {
        let mut world = Map::open().floor(63);
        // A ring of diamond ore around the camera.
        for x in -4..=4 {
            for z in -4..=4 {
                if (x, z) != (0, 0) {
                    world
                        .blocks
                        .insert((x, 64, z), BlockState::of(blocks::DIAMOND_ORE));
                }
            }
        }
        let cam = DVec3::new(0.5, 66.0, 0.5);
        let visual = on(|e| e.visual = true);
        let mut fx = Fx::default();
        let mut sparkled = false;
        for t in 0..300 {
            fx.tick(&world, &input(t, cam, Sky::Clear, &[]), visual);
            sparkled |= fx.particles().iter().any(|p| p.sprite == Sprite::Sparkle);
        }
        assert!(sparkled, "ores sparkle");
        // A chicken hit gives feathers, a skeleton bones, a zombie nothing.
        let mut fx = Fx::default();
        let hit = |mob: &str| Hit {
            at: DVec3::new(0.0, 64.0, 0.0),
            mob: mob.to_owned(),
            size: 0,
        };
        let empty = Map::open();
        for (mob, sprite) in [
            ("chicken", Some(Sprite::Feather)),
            ("skeleton", Some(Sprite::Bone)),
            ("zombie", None),
        ] {
            let mut fx2 = Fx::default();
            let hits = [hit(mob)];
            fx2.tick(
                &empty,
                &Input {
                    tick: 0,
                    camera: cam,
                    sky: Sky::Clear,
                    bodies: &[],
                    hits: &hits,
                },
                visual,
            );
            assert_eq!(fx2.particles().first().map(|p| p.sprite), sprite, "{mob}");
        }
        fx.tick(&world, &input(0, cam, Sky::Clear, &[]), visual);
        // Turning an effect off removes its particles.
        let mut fx = Fx::default();
        let mut rings = 0;
        for t in 0..80 {
            fx.tick(
                &lake(),
                &input(t, DVec3::new(0.5, 64.0, 0.5), Sky::Rain, &[]),
                on(|e| e.visual = true),
            );
            rings += fx.count(Kind::Splash);
        }
        assert!(rings > 0, "rings in rain without Particle Rain");
        fx.tick(&lake(), &input(99, cam, Sky::Rain, &[]), Enabled::default());
        assert!(fx.particles().is_empty());
    }

    #[test]
    fn particle_rain_replaces_visualitys_rings_so_there_are_no_doubles() {
        let cam = DVec3::new(0.5, 64.0, 0.5);
        let rings = |enabled: Enabled, tick: u64| {
            let mut fx = Fx::default();
            // One tick: no drop has landed yet, so any ring is Visuality's.
            fx.tick(&lake(), &input(tick, cam, Sky::Rain, &[]), enabled);
            fx.particles()
                .iter()
                .filter(|p| p.sprite == Sprite::WaterCircle)
                .count()
        };
        let visual = on(|e| e.visual = true);
        let both = on(|e| {
            e.visual = true;
            e.rain = true;
        });
        let mut made = 0;
        for tick in 0..40 {
            made += rings(visual, tick);
        }
        assert!(made > 0, "Visuality alone makes rings in rain");
        assert_eq!(
            (0..40).map(|t| rings(both, t)).sum::<usize>(),
            0,
            "with Particle Rain on, only its ripples"
        );
    }

    /// M64's share: all five effects together in a thunderstorm with three
    /// boats and swimmers cost at most 0.4 ms of CPU a game tick, making
    /// the particles and building their quads (release builds).
    #[test]
    fn all_five_effects_in_a_storm_with_three_boats_fit_the_cpu_share() {
        let world = lake();
        let cam = DVec3::new(0.5, 64.0, 0.5);
        let uvs = FxUvs::build(|_| Some([0.0, 0.0, 1.0, 1.0]));
        let camera = crate::render::camera::Camera::new(cam.as_vec3(), 0.0, 0.0);
        let bodies: Vec<Body> = (0..3)
            .map(|i| {
                swimmer(
                    i,
                    DVec3::new(2.0 + 3.0 * i as f64, 62.0, 2.0),
                    DVec3::new(0.0, 0.0, 0.2),
                    BodyKind::Boat,
                )
            })
            .chain((3..6).map(|i| {
                swimmer(
                    i,
                    DVec3::new(-3.0, 58.0, i as f64),
                    DVec3::new(0.1, 0.0, 0.0),
                    BodyKind::Player,
                )
            }))
            .collect();
        let all = Enabled {
            visual: true,
            dust: true,
            rain: true,
            bubbles: true,
            wakes: true,
        };
        let mut fx = Fx::default();
        for t in 0..200 {
            fx.tick(&world, &input(t, cam, Sky::Thunder, &bodies), all);
        }
        let ticks = 400u64;
        let started = std::time::Instant::now();
        let mut quads = 0;
        for t in 0..ticks {
            fx.tick(&world, &input(200 + t, cam, Sky::Thunder, &bodies), all);
            let mut mesh = crate::world::mesh::Mesh::default();
            crate::render::entities::add_fx(&mut mesh, fx.particles(), &uvs, &camera, 0.5, |_| {
                (15, 0)
            });
            quads += mesh.indices.len() / 6;
        }
        let per_tick = started.elapsed().as_secs_f64() * 1000.0 / ticks as f64;
        println!(
            "storm with three boats: {per_tick:.3} ms a tick, {} quads a tick",
            quads / ticks as usize
        );
        assert!(quads > 0);
        let allowed = if cfg!(debug_assertions) { 6.0 } else { 0.4 };
        assert!(per_tick < allowed, "{per_tick:.3} ms a tick");
        for kind in [
            Kind::Dust,
            Kind::Visual,
            Kind::Rain,
            Kind::Splash,
            Kind::Bubble,
            Kind::Wake,
        ] {
            assert!(fx.count(kind) <= kind.cap());
        }
    }
}
