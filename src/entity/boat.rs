//! Boats and rafts (modern `Boat`/`AbstractBoat`): every wood's boat and
//! chest boat, the bamboo raft and chest raft. They float at the surface,
//! drift with nothing to steer them, and a player in the front seat rows:
//! forward and back, turning left and right. Water lets them glide (about
//! 8 blocks a second), ice much faster, and land drags them almost to a
//! stop.

use glam::{DVec3, IVec3};
use serde::{Deserialize, Serialize};

use crate::{
    inventory::stack::ItemStack,
    player::{
        collision::{Aabb, Collider},
        physics::sweep,
    },
    world::{
        World,
        block::{BlockState, Fluid},
        blocks, fluid,
        props::Prop,
    },
};

/// The woods boats come in, by texture name; bamboo makes rafts.
pub const WOODS: [&str; 11] = [
    "oak", "spruce", "birch", "jungle", "acacia", "dark_oak", "mangrove", "cherry", "pale_oak",
    "poplar", "bamboo",
];

/// A boat is 1.375 wide and 0.5625 tall.
pub const WIDTH: f64 = 1.375;
pub const HEIGHT: f64 = 0.5625;
/// Gravity on a boat out of water, per tick.
const GRAVITY: f64 = 0.04;
/// Forward push a tick while rowing ahead, and back or turning on the spot.
const ROW_FORWARD: f64 = 0.04;
const ROW_BACK: f64 = 0.005;
/// Degrees of turn added a tick while turning.
const TURN: f32 = 1.0;
/// Ticks fully under water before the boat throws its passengers off.
const SINK_TICKS: u32 = 60;
/// Damage that breaks a boat; hits add 10 times their damage, and it
/// heals 1 a tick.
const BREAK_DAMAGE: f32 = 40.0;
/// The paddles turn this much a tick while rowing.
const PADDLE_STEP: f32 = std::f32::consts::PI / 8.0;
/// Slots in a chest boat.
pub const CHEST_SLOTS: usize = 27;

/// Which boat: its wood (an index into `WOODS`) and whether it has a chest.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct BoatKind {
    pub wood: usize,
    pub chest: bool,
}

impl BoatKind {
    /// The kind an item places: `oak_boat`, `birch_chest_boat`,
    /// `bamboo_raft`, `bamboo_chest_raft`.
    pub fn of_item(name: &str) -> Option<Self> {
        let (wood, chest) = if let Some(wood) = name.strip_suffix("_chest_boat") {
            (wood, true)
        } else if let Some(wood) = name.strip_suffix("_boat") {
            (wood, false)
        } else if name == "bamboo_raft" {
            ("bamboo", false)
        } else if name == "bamboo_chest_raft" {
            ("bamboo", true)
        } else {
            return None;
        };
        let wood = WOODS.iter().position(|w| *w == wood)?;
        (wood != Self::BAMBOO || name.contains("raft")).then_some(Self { wood, chest })
    }

    const BAMBOO: usize = 10;

    pub fn is_raft(self) -> bool {
        self.wood == Self::BAMBOO
    }

    pub fn wood_name(self) -> &'static str {
        WOODS[self.wood]
    }

    /// The item it drops as.
    pub fn item_name(self) -> String {
        match (self.is_raft(), self.chest) {
            (true, false) => "bamboo_raft".to_owned(),
            (true, true) => "bamboo_chest_raft".to_owned(),
            (false, false) => format!("{}_boat", self.wood_name()),
            (false, true) => format!("{}_chest_boat", self.wood_name()),
        }
    }

    /// How many can sit in it (a chest takes the back seat).
    pub fn seats(self) -> usize {
        if self.chest { 1 } else { 2 }
    }
}

/// Where a boat is (modern `Boat.Status`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub enum Status {
    InWater,
    UnderWater,
    UnderFlowingWater,
    OnLand,
    #[default]
    InAir,
}

/// Who sits in a seat.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Passenger {
    /// The player with this id (`PlayerState::id`).
    Player(u64),
    Mob(u64),
}

/// The keys the rowing player holds.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Rowing {
    pub forward: bool,
    pub back: bool,
    pub left: bool,
    pub right: bool,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Boat {
    pub id: u64,
    pub kind: BoatKind,
    /// The middle of the bottom of its box.
    pub position: DVec3,
    pub prev_position: DVec3,
    /// Facing, as the camera's yaw (radians; 0 faces north).
    pub yaw: f32,
    pub prev_yaw: f32,
    pub velocity: DVec3,
    /// Degrees it turns a tick, easing off as it drifts.
    pub turning: f32,
    pub status: Status,
    water_level: f64,
    land_friction: f64,
    under_ticks: u32,
    /// Each paddle's swing, and whether it's moving.
    pub paddles: [f32; 2],
    pub rowing: [bool; 2],
    /// How hurt it is (it breaks at 40), and the shake of the last hit.
    pub damage: f32,
    pub hurt_time: f32,
    pub hurt_dir: f32,
    /// Front seat first.
    pub passengers: Vec<Passenger>,
    /// A chest boat's 27 slots.
    pub slots: Vec<Option<ItemStack>>,
    /// The dimension it's in.
    pub dimension: crate::world::dimension::Dimension,
}

/// A boat as saved in level.ron.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SavedBoat {
    pub item: String,
    pub position: [f64; 3],
    pub yaw: f32,
    #[serde(default)]
    pub slots: Vec<Option<ItemStack>>,
    #[serde(default)]
    pub dimension: crate::world::dimension::Dimension,
}

impl Boat {
    pub fn new(id: u64, kind: BoatKind, position: DVec3, yaw: f32) -> Self {
        Self {
            id,
            kind,
            position,
            prev_position: position,
            yaw,
            prev_yaw: yaw,
            velocity: DVec3::ZERO,
            turning: 0.0,
            status: Status::InAir,
            water_level: 0.0,
            land_friction: 0.0,
            under_ticks: 0,
            paddles: [0.0; 2],
            rowing: [false; 2],
            damage: 0.0,
            hurt_time: 0.0,
            hurt_dir: 1.0,
            passengers: Vec::new(),
            slots: if kind.chest {
                vec![None; CHEST_SLOTS]
            } else {
                Vec::new()
            },
            dimension: Default::default(),
        }
    }

    pub fn saved(&self) -> SavedBoat {
        SavedBoat {
            item: self.kind.item_name(),
            position: self.position.to_array(),
            yaw: self.yaw,
            slots: self.slots.clone(),
            dimension: self.dimension,
        }
    }

    pub fn from_saved(id: u64, saved: &SavedBoat) -> Option<Self> {
        let kind = BoatKind::of_item(&saved.item)?;
        let mut boat = Self::new(id, kind, DVec3::from_array(saved.position), saved.yaw);
        if kind.chest && saved.slots.len() == CHEST_SLOTS {
            boat.slots = saved.slots.clone();
        }
        boat.dimension = saved.dimension;
        Some(boat)
    }

    pub fn aabb(&self) -> Aabb {
        aabb_at(self.position)
    }

    /// The direction it faces, flat.
    pub fn forward(&self) -> DVec3 {
        let (sin, cos) = (self.yaw as f64).sin_cos();
        DVec3::new(sin, 0.0, -cos)
    }

    /// Where the passenger in `seat` has their feet (modern
    /// `getPassengerAttachmentPoint` less the player's 0.6 vehicle
    /// attachment): one in the middle, or two 0.2 ahead and 0.6 behind.
    pub fn seat(&self, seat: usize, mob: bool) -> DVec3 {
        let mut along = if self.passengers.len() > 1 || self.kind.chest {
            if seat == 0 { 0.2 } else { -0.6 }
        } else {
            0.0
        };
        if mob && self.passengers.len() > 1 {
            along += 0.2;
        }
        let up = if self.kind.is_raft() {
            HEIGHT * 8.0 / 9.0
        } else {
            HEIGHT / 3.0
        };
        self.position + self.forward() * along + DVec3::Y * (up - 0.6)
    }

    /// Whether a seat is free.
    pub fn has_room(&self) -> bool {
        self.passengers.len() < self.kind.seats()
    }

    /// A hit for `amount`: breaks it (returns true) at 40, counting each
    /// point ten times (modern `Boat.hurt`).
    pub fn hurt(&mut self, amount: f32) -> bool {
        self.hurt_dir = -self.hurt_dir;
        self.hurt_time = 10.0;
        self.damage += amount * 10.0;
        self.damage > BREAK_DAMAGE
    }

    /// One tick (modern `Boat.tick`): finds where it is, floats, rows if
    /// someone is `rowing`, moves and turns. Returns true when it has been
    /// under water too long to keep its passengers.
    pub fn tick(
        &mut self,
        world: &World,
        collider: &impl Collider,
        rowing: Option<Rowing>,
    ) -> bool {
        self.prev_position = self.position;
        self.prev_yaw = self.yaw;
        self.hurt_time = (self.hurt_time - 1.0).max(0.0);
        self.damage = (self.damage - 1.0).max(0.0);
        let old = self.status;
        self.status = self.find_status(world);
        if matches!(self.status, Status::UnderWater | Status::UnderFlowingWater) {
            self.under_ticks += 1;
        } else {
            self.under_ticks = 0;
        }
        self.float(old, world);
        let row = rowing.unwrap_or_default();
        if rowing.is_some() {
            self.control(row);
        } else {
            self.rowing = [false; 2];
        }
        let mut boxes = Vec::new();
        let (moved, bb) = sweep(self.aabb(), self.velocity, collider, &mut boxes);
        self.position = DVec3::new(
            (bb.min.x + bb.max.x) / 2.0,
            bb.min.y,
            (bb.min.z + bb.max.z) / 2.0,
        );
        for axis in 0..3 {
            if moved[axis] != self.velocity[axis] {
                self.velocity[axis] = 0.0;
            }
        }
        for side in 0..2 {
            if self.rowing[side] {
                self.paddles[side] += PADDLE_STEP;
            } else {
                self.paddles[side] = 0.0;
            }
        }
        self.under_ticks >= SINK_TICKS
    }

    /// Where it is: under water, in it, on land or in the air.
    fn find_status(&mut self, world: &World) -> Status {
        if let Some(under) = self.under_water(world) {
            self.water_level = self.aabb().max.y;
            return under;
        }
        if self.in_water(world) {
            return Status::InWater;
        }
        let friction = ground_friction(world, &self.aabb());
        if friction > 0.0 {
            self.land_friction = friction;
            Status::OnLand
        } else {
            Status::InAir
        }
    }

    /// Whether water closes over the top of the box (modern `isUnderwater`).
    fn under_water(&self, world: &World) -> Option<Status> {
        let bb = self.aabb();
        let top = bb.max.y + 0.001;
        let mut under = false;
        for cell in cells(&bb, bb.max.y, top) {
            if let Some((surface, source)) = water_top(world, cell)
                && top < surface
            {
                if !source {
                    return Some(Status::UnderFlowingWater);
                }
                under = true;
            }
        }
        under.then_some(Status::UnderWater)
    }

    /// Whether the bottom of the box is below the water's surface, noting
    /// the highest surface (modern `checkInWater`).
    fn in_water(&mut self, world: &World) -> bool {
        let bb = self.aabb();
        self.water_level = f64::MIN;
        let mut wet = false;
        for cell in cells(&bb, bb.min.y, bb.min.y + 0.001) {
            if let Some((surface, _)) = water_top(world, cell) {
                self.water_level = self.water_level.max(surface);
                wet |= bb.min.y < surface;
            }
        }
        wet
    }

    /// Drag, gravity and buoyancy (modern `floatBoat`; its halving of land
    /// friction for a player comes after the friction is used, so it never
    /// counts).
    fn float(&mut self, old: Status, world: &World) {
        if old == Status::InAir && !matches!(self.status, Status::InAir | Status::OnLand) {
            // Landing in water: set on the surface.
            self.water_level = self.surface_above(world);
            self.position.y = self.water_level - HEIGHT + 0.101;
            self.velocity.y = 0.0;
            self.status = Status::InWater;
            return;
        }
        let mut gravity = -GRAVITY;
        let mut lift = 0.0;
        let friction = match self.status {
            Status::InWater => {
                lift = (self.water_level - self.position.y) / HEIGHT;
                0.9
            }
            Status::UnderFlowingWater => {
                gravity = -7.0e-4;
                0.9
            }
            Status::UnderWater => {
                lift = 0.01;
                0.45
            }
            Status::InAir => 0.9,
            Status::OnLand => self.land_friction,
        };
        self.velocity.x *= friction;
        self.velocity.z *= friction;
        self.velocity.y += gravity;
        self.turning *= friction as f32;
        if lift > 0.0 {
            self.velocity.y = (self.velocity.y + lift * 0.061_538_46) * 0.75;
        }
    }

    /// The water's surface at the top of the boat's column of cells.
    fn surface_above(&self, world: &World) -> f64 {
        let bb = self.aabb();
        let mut y = bb.max.y.floor() as i32;
        loop {
            let tops: Vec<f64> = cells(&bb, y as f64, y as f64 + 0.5)
                .filter_map(|c| water_top(world, c).map(|(top, _)| top))
                .collect();
            if tops.is_empty() {
                return y as f64;
            }
            let full = tops.iter().fold(0.0f64, |m, &t| m.max(t - y as f64));
            if full < 1.0 {
                return y as f64 + full;
            }
            y += 1;
            if y > bb.max.y as i32 + 8 {
                return y as f64;
            }
        }
    }

    /// The rower's keys (modern `controlBoat`): turning a degree a tick,
    /// rowing ahead at 0.04 a tick and back or on the spot at 0.005.
    fn control(&mut self, row: Rowing) {
        let mut push = 0.0;
        if row.left {
            self.turning -= TURN;
        }
        if row.right {
            self.turning += TURN;
        }
        if row.right != row.left && !row.forward && !row.back {
            push += ROW_BACK;
        }
        self.yaw += self.turning.to_radians();
        if row.forward {
            push += ROW_FORWARD;
        }
        if row.back {
            push -= ROW_BACK;
        }
        self.velocity += self.forward() * push;
        self.rowing = [
            (row.right && !row.left) || row.forward,
            (row.left && !row.right) || row.forward,
        ];
    }
}

/// A boat's box with its bottom middle at `at`.
pub fn aabb_at(at: DVec3) -> Aabb {
    let half = WIDTH / 2.0;
    Aabb::new(
        at - DVec3::new(half, 0.0, half),
        at + DVec3::new(half, HEIGHT, half),
    )
}

/// The cells under `bb` across, between heights `low` and `high`.
fn cells(bb: &Aabb, low: f64, high: f64) -> impl Iterator<Item = IVec3> {
    let (x0, x1) = (bb.min.x.floor() as i32, bb.max.x.ceil() as i32);
    let (z0, z1) = (bb.min.z.floor() as i32, bb.max.z.ceil() as i32);
    let (y0, y1) = (low.floor() as i32, high.ceil() as i32);
    (x0..x1).flat_map(move |x| {
        (y0..y1.max(y0 + 1)).flat_map(move |y| (z0..z1).map(move |z| IVec3::new(x, y, z)))
    })
}

/// The top of the water in `cell` and whether it's a source: a source or
/// a fall is 8/9 full, a flow less, and water under more water is full
/// (modern `FluidState.getHeight`).
pub fn water_top(world: &World, cell: IVec3) -> Option<(f64, bool)> {
    let state = world.block(cell.x, cell.y, cell.z)?;
    let wet = |b: BlockState| {
        b.def().fluid == Some(Fluid::Water)
            || (b.has(Prop::Waterlogged) && b.bool(Prop::Waterlogged))
    };
    if !wet(state) {
        return None;
    }
    let level = if state.def().fluid == Some(Fluid::Water) {
        fluid::level(state)
    } else {
        0
    };
    let above = world.block(cell.x, cell.y + 1, cell.z).is_some_and(wet);
    let height = if above {
        1.0
    } else if level >= 8 || level == 0 {
        8.0 / 9.0
    } else {
        (8 - level) as f64 / 9.0
    };
    Some((cell.y as f64 + height, level == 0))
}

/// The average slipperiness of the blocks under the box, or 0 if there
/// are none (modern `getGroundFriction`): ice 0.98, blue ice 0.989, slime
/// 0.8, others 0.6.
pub fn ground_friction(world: &World, bb: &Aabb) -> f64 {
    let under = Aabb::new(
        DVec3::new(bb.min.x, bb.min.y - 0.001, bb.min.z),
        DVec3::new(bb.max.x, bb.min.y, bb.max.z),
    );
    let shapes = crate::world::shape::shapes();
    let (mut sum, mut count) = (0.0, 0);
    for cell in cells(&under, under.min.y, under.max.y) {
        let Some(state) = world.block(cell.x, cell.y, cell.z) else {
            continue;
        };
        let corner = cell.as_dvec3();
        let touches = shapes.collision(state).iter().any(|b| {
            Aabb::new(
                corner + DVec3::from(b.min.map(f64::from)),
                corner + DVec3::from(b.max.map(f64::from)),
            )
            .intersects(&under)
        });
        if touches {
            sum += slipperiness(state);
            count += 1;
        }
    }
    if count == 0 { 0.0 } else { sum / count as f64 }
}

fn slipperiness(state: BlockState) -> f64 {
    if state.is(blocks::BLUE_ICE) {
        0.989
    } else if state.is(blocks::ICE) || state.is(blocks::PACKED_ICE) || state.is(blocks::FROSTED_ICE)
    {
        0.98
    } else if state.is(blocks::SLIME_BLOCK) {
        0.8
    } else {
        0.6
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        player::collision::WorldCollider,
        world::chunk::{Chunk, ChunkPos},
    };

    /// A chunk with stone to y 59, then `top` from 60 to 62 (water, ice or
    /// stone), air above.
    fn world(top: BlockState) -> World {
        let mut world = World::default();
        for cx in 0..8 {
            let mut chunk = Chunk::new(ChunkPos::new(cx, 0));
            for x in 0..16 {
                for z in 0..16 {
                    for y in 0..60 {
                        chunk.set(x, y, z, BlockState::of(blocks::STONE));
                    }
                    for y in 60..63 {
                        chunk.set(x, y, z, top);
                    }
                }
            }
            world.insert(chunk);
        }
        world
    }

    fn run(world: &World, boat: &mut Boat, rowing: Option<Rowing>, ticks: u32) {
        let collider = WorldCollider {
            world,
            blocks: crate::world::block::registry(),
        };
        for _ in 0..ticks {
            boat.tick(world, &collider, rowing);
        }
    }

    fn speed(world: &World, start_y: f64) -> f64 {
        let mut boat = Boat::new(
            1,
            BoatKind {
                wood: 0,
                chest: false,
            },
            DVec3::new(8.0, start_y, 8.0),
            std::f32::consts::FRAC_PI_2,
        );
        boat.position.x = 1.0;
        let ahead = Rowing {
            forward: true,
            ..Default::default()
        };
        run(world, &mut boat, Some(ahead), 60);
        let x = boat.position.x;
        run(world, &mut boat, Some(ahead), 1);
        (boat.position.x - x) * 20.0
    }

    #[test]
    fn items_name_their_boats() {
        let oak = BoatKind::of_item("oak_boat").unwrap();
        assert_eq!((oak.wood_name(), oak.chest, oak.seats()), ("oak", false, 2));
        let chest = BoatKind::of_item("dark_oak_chest_boat").unwrap();
        assert_eq!(
            (chest.wood_name(), chest.chest, chest.seats()),
            ("dark_oak", true, 1)
        );
        assert!(BoatKind::of_item("bamboo_raft").unwrap().is_raft());
        assert_eq!(
            BoatKind::of_item("bamboo_chest_raft").unwrap().item_name(),
            "bamboo_chest_raft"
        );
        assert!(BoatKind::of_item("bamboo_boat").is_none());
        assert!(BoatKind::of_item("oak_planks").is_none());
        for wood in WOODS {
            let name = if wood == "bamboo" {
                "bamboo_raft".to_owned()
            } else {
                format!("{wood}_boat")
            };
            assert_eq!(BoatKind::of_item(&name).unwrap().item_name(), name);
        }
    }

    #[test]
    fn a_boat_floats_at_the_surface() {
        let water = world(BlockState::of(blocks::WATER));
        let mut boat = Boat::new(
            1,
            BoatKind {
                wood: 0,
                chest: false,
            },
            DVec3::new(8.0, 64.0, 8.0),
            0.0,
        );
        run(&water, &mut boat, None, 100);
        assert_eq!(boat.status, Status::InWater);
        // The water's top is 62 + 8/9; buoyancy (lift × 0.0615) holds
        // against gravity (0.04) with the bottom 0.366 under it.
        let surface = 62.0 + 8.0 / 9.0;
        let depth = surface - boat.position.y;
        assert!((depth - 0.366).abs() < 0.02, "{depth}");
    }

    #[test]
    fn rowing_speeds_match_vanilla_on_water_ice_and_land() {
        let water = speed(&world(BlockState::of(blocks::WATER)), 62.6);
        assert!((water - 8.0).abs() < 0.5, "water {water}");
        // Ice and blue ice head for 40 and 72 blocks a second; three
        // seconds in they are well past 20.
        let ice = speed(&world(BlockState::of(blocks::ICE)), 63.0);
        assert!(ice > 20.0, "ice {ice}");
        let blue = speed(&world(BlockState::of(blocks::BLUE_ICE)), 63.0);
        assert!(blue > ice, "blue ice {blue}");
        let land = speed(&world(BlockState::of(blocks::STONE)), 63.0);
        assert!((land - 2.0).abs() < 0.1, "land {land}");
    }

    #[test]
    fn turning_keys_turn_and_paddles_move_only_while_rowing() {
        let water = world(BlockState::of(blocks::WATER));
        let mut boat = Boat::new(
            1,
            BoatKind {
                wood: 0,
                chest: false,
            },
            DVec3::new(8.0, 62.6, 8.0),
            0.0,
        );
        let left = Rowing {
            left: true,
            ..Default::default()
        };
        run(&water, &mut boat, Some(left), 20);
        assert!(boat.yaw < -0.3, "{}", boat.yaw);
        assert!(
            boat.rowing[1] && !boat.rowing[0],
            "the left key rows the right paddle"
        );
        assert!(boat.paddles[1] > 0.0);
        run(&water, &mut boat, None, 1);
        assert_eq!(boat.paddles, [0.0; 2]);
    }

    #[test]
    fn hits_add_up_to_breaking() {
        let mut boat = Boat::new(
            1,
            BoatKind {
                wood: 0,
                chest: false,
            },
            DVec3::ZERO,
            0.0,
        );
        assert!(!boat.hurt(1.0) && !boat.hurt(1.0) && !boat.hurt(1.0) && !boat.hurt(1.0));
        assert!(boat.hurt(1.0), "a fifth punch in a row breaks it");
    }
}
