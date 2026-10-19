//! Redstone, following modern Minecraft's signal model: parts give a
//! `getSignal` to every side (or only to the blocks around them) and a
//! `getDirectSignal` that solid blocks pass on, dust takes the strongest
//! signal around it less one, and repeaters, torches, lamps and buttons
//! change on scheduled ticks. A change to a block re-evaluates the blocks
//! within two steps of it until nothing more changes, which settles dust
//! at once as Minecraft does. Pistons, droppers, note blocks and TNT only
//! report what they should do (`Event`); the game carries it out.

use std::collections::{BTreeMap, VecDeque};

use glam::IVec3;

use super::{
    Face, World,
    block::{BlockState, Shape},
    blocks,
    chunk::{Chunk, SECTION_COUNT, SECTION_SIZE},
    fast_hash::{FastMap, FastSet},
    props::{Prop, attach_face, door_half, rail_shape, wire_side},
};
use crate::config::{CHUNK_SIZE, WORLD_HEIGHT};

/// Most blocks looked at in one tick, so a long chain can't stall it.
const MAX_UPDATES: usize = 200_000;
/// Ticks a stone button stays down, and a wooden one.
pub const STONE_BUTTON_TICKS: u64 = 20;
pub const WOOD_BUTTON_TICKS: u64 = 30;
/// Ticks between a pressure plate's checks while it's down.
const PLATE_TICKS: u64 = 20;
/// Ticks before a lamp goes out, and before a dispenser fires.
const LAMP_TICKS: u64 = 4;
const DISPENSE_TICKS: u64 = 4;
/// Torches burn out after this many toggles in `BURNOUT_WINDOW` ticks, and
/// stay out this long.
const BURNOUT_TOGGLES: usize = 8;
const BURNOUT_WINDOW: u64 = 60;
const BURNOUT_TICKS: u64 = 160;
/// Powered rails pass power along this many rails.
const RAIL_POWER_RANGE: u32 = 8;

const HORIZONTAL: [Face; 4] = [Face::North, Face::East, Face::South, Face::West];

/// The order scheduled ticks of the same tick run in (modern
/// `TickPriority`): lower runs first.
pub mod priority {
    pub const EXTREMELY_HIGH: i8 = -3;
    pub const VERY_HIGH: i8 = -2;
    pub const HIGH: i8 = -1;
    pub const NORMAL: i8 = 0;
}

/// What the game has to do about a redstone change.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Event {
    /// A dispenser or dropper fires.
    Dispense(IVec3),
    /// A note block plays (instrument and note as in its state).
    PlayNote { at: IVec3, instrument: u8, note: u8 },
    /// TNT was powered: it's gone from the world and primed.
    Prime(IVec3),
    /// A piston should extend or retract.
    Piston { at: IVec3, extend: bool },
}

/// What one tick of redstone did.
#[derive(Debug, Default)]
pub struct Outcome {
    /// Every changed block's new state, once each, in the order changed.
    pub edits: Vec<(IVec3, BlockState)>,
    pub events: Vec<Event>,
}

/// Scheduled redstone updates, and blocks changed since the last tick.
/// Not saved: parts that were mid-delay are looked at again when their
/// chunk loads.
#[derive(Debug, Default)]
pub struct Redstone {
    /// By (due tick, priority, order scheduled).
    queue: BTreeMap<(u64, i8, u64), IVec3>,
    scheduled: FastSet<IVec3>,
    next: u64,
    changed: Vec<IVec3>,
    /// When each torch last toggled, for burning out.
    toggles: FastMap<IVec3, Vec<u64>>,
    /// Pistons with a move already reported and not yet carried out.
    pending_pistons: FastSet<IVec3>,
}

impl Redstone {
    /// Notes that the block at `at` changed.
    pub fn changed(&mut self, at: IVec3) {
        self.changed.push(at);
    }

    /// How many ticks are waiting.
    pub fn pending(&self) -> usize {
        self.queue.len()
    }

    /// Schedules the block at `at` to be updated `delay` ticks after
    /// `time`, unless it's scheduled already.
    pub fn schedule(&mut self, at: IVec3, time: u64, delay: u64, priority: i8) {
        if self.scheduled.insert(at) {
            self.queue.insert((time + delay, priority, self.next), at);
            self.next += 1;
        }
    }

    /// The game carried out (or dropped) the piston move at `at`.
    pub fn piston_done(&mut self, at: IVec3) {
        self.pending_pistons.remove(&at);
    }

    /// Looks again at the redstone parts in a chunk that just loaded, and
    /// schedules the releases of buttons and plates that were down when it
    /// was saved.
    pub fn chunk_loaded(&mut self, chunk: &Chunk, time: u64) {
        let (min_x, min_z) = chunk.pos.min_block();
        for section in (0..SECTION_COUNT).filter(|&s| !chunk.section_is_empty(s)) {
            for y in section * SECTION_SIZE..(section + 1) * SECTION_SIZE {
                for z in 0..CHUNK_SIZE {
                    for x in 0..CHUNK_SIZE {
                        let state = chunk.get(x, y, z);
                        if !is_part(state) {
                            continue;
                        }
                        let at = IVec3::new(min_x + x as i32, y as i32, min_z + z as i32);
                        if releases_itself(state) && state.bool(Prop::Powered) {
                            self.schedule(at, time, press_ticks(state), priority::NORMAL);
                        } else {
                            self.changed.push(at);
                        }
                    }
                }
            }
        }
    }

    /// Runs the updates due by `time` and settles everything that changed
    /// since the last tick. `pressed` holds the plates and detector rails
    /// something is standing on now.
    pub fn tick(&mut self, world: &World, time: u64, pressed: &FastSet<IVec3>) -> Outcome {
        let changed = std::mem::take(&mut self.changed);
        let mut due = Vec::new();
        while let Some(entry) = self.queue.first_entry() {
            if entry.key().0 > time {
                break;
            }
            let at = entry.remove();
            self.scheduled.remove(&at);
            due.push(at);
        }
        if changed.is_empty() && due.is_empty() {
            return Outcome::default();
        }
        let mut step = Step {
            world,
            redstone: self,
            time,
            pressed,
            edits: FastMap::default(),
            order: Vec::new(),
            events: Vec::new(),
            work: VecDeque::new(),
            queued: FastSet::default(),
            budget: MAX_UPDATES,
        };
        for at in changed {
            step.around(at);
        }
        step.settle();
        for at in due {
            step.tick_block(at);
            step.settle();
        }
        let Step {
            edits,
            order,
            events,
            ..
        } = step;
        Outcome {
            edits: order.into_iter().map(|at| (at, edits[&at])).collect(),
            events,
        }
    }
}

/// Whether `state` is a part redstone looks at when its chunk loads.
fn is_part(state: BlockState) -> bool {
    matches!(
        state.id(),
        blocks::REDSTONE_WIRE
            | blocks::REDSTONE_TORCH
            | blocks::REDSTONE_WALL_TORCH
            | blocks::REPEATER
            | blocks::REDSTONE_LAMP
            | blocks::STONE_BUTTON
            | blocks::OAK_BUTTON
            | blocks::STONE_PRESSURE_PLATE
            | blocks::OAK_PRESSURE_PLATE
            | blocks::DETECTOR_RAIL
            | blocks::POWERED_RAIL
            | blocks::ACTIVATOR_RAIL
            | blocks::PISTON
            | blocks::STICKY_PISTON
            | blocks::HOPPER
    )
}

/// Buttons, plates and detector rails let go on their own.
fn releases_itself(state: BlockState) -> bool {
    matches!(
        state.id(),
        blocks::STONE_BUTTON
            | blocks::OAK_BUTTON
            | blocks::STONE_PRESSURE_PLATE
            | blocks::OAK_PRESSURE_PLATE
            | blocks::DETECTOR_RAIL
    )
}

/// How long after being pressed `state` is looked at again.
pub fn press_ticks(state: BlockState) -> u64 {
    match state.id() {
        blocks::STONE_BUTTON => STONE_BUTTON_TICKS,
        blocks::OAK_BUTTON => WOOD_BUTTON_TICKS,
        _ => PLATE_TICKS,
    }
}

fn step_of(face: Face) -> IVec3 {
    IVec3::from_array(face.normal())
}

/// A repeater (the only diode here).
fn is_diode(state: BlockState) -> bool {
    state.is(blocks::REPEATER)
}

/// The direction from a lever or button to the block it's attached to,
/// as seen from that block (modern `getConnectedDirection`).
fn attached_direction(state: BlockState) -> Face {
    match state.get(Prop::AttachFace) {
        attach_face::FLOOR => Face::Top,
        attach_face::CEILING => Face::Bottom,
        _ => state.face(Prop::Facing),
    }
}

/// Whether `state` is a block signals pass through (modern
/// `isRedstoneConductor`): an opaque solid cube that isn't a power source
/// or an extended piston.
pub fn conductor(state: BlockState) -> bool {
    let def = state.def();
    if !def.solid || !def.is_opaque_cube_state(state) {
        return false;
    }
    match state.id() {
        blocks::REDSTONE_BLOCK => false,
        blocks::PISTON | blocks::STICKY_PISTON => !state.bool(Prop::Extended),
        _ => true,
    }
}

/// Whether the wire at `state` joins toward horizontal `face`.
fn wire_joins(state: BlockState, face: Face) -> bool {
    state.get(Prop::wire_side(face)) != wire_side::NONE
}

/// The instrument a note block plays over `below` (modern
/// `NoteBlockInstrument.byState`).
pub fn instrument_over(below: BlockState) -> u8 {
    let name = below.id().name();
    let wood = [
        "planks",
        "log",
        "wood",
        "fence",
        "door",
        "chest",
        "bookshelf",
    ]
    .iter()
    .any(|part| name.contains(part))
        || matches!(name, "note_block" | "crafting_table" | "jukebox");
    let wool = name.ends_with("_wool");
    let (harp, basedrum, snare, hat, bass, flute, bell) = (0, 1, 2, 3, 4, 5, 6);
    let (guitar, chime, xylophone, iron, cow_bell, didgeridoo) = (7, 8, 9, 10, 11, 12);
    let (bit, banjo, pling) = (13, 14, 15);
    if wool {
        return guitar;
    }
    match name {
        "sand" | "gravel" => snare,
        "glass" | "sea_lantern" => hat,
        "clay" => flute,
        "gold_block" => bell,
        "packed_ice" => chime,
        "bone_block" => xylophone,
        "iron_block" => iron,
        "soul_sand" => cow_bell,
        "pumpkin" | "carved_pumpkin" => didgeridoo,
        "emerald_block" => bit,
        "hay_block" => banjo,
        "glowstone" => pling,
        _ if wood => bass,
        _ if name.ends_with("_concrete_powder") => snare,
        _ if name.contains("glass") => hat,
        _ if matches!(
            below.def().tool,
            Some(crate::inventory::item::ToolKind::Pickaxe)
        ) && below.def().solid =>
        {
            basedrum
        }
        _ => harp,
    }
}

/// One tick's work, with its edits laid over the world.
struct Step<'a> {
    world: &'a World,
    redstone: &'a mut Redstone,
    time: u64,
    pressed: &'a FastSet<IVec3>,
    edits: FastMap<IVec3, BlockState>,
    order: Vec<IVec3>,
    events: Vec<Event>,
    work: VecDeque<IVec3>,
    queued: FastSet<IVec3>,
    budget: usize,
}

impl Step<'_> {
    fn get(&self, at: IVec3) -> Option<BlockState> {
        if !(0..WORLD_HEIGHT as i32).contains(&at.y) {
            return None;
        }
        match self.edits.get(&at) {
            Some(&block) => Some(block),
            None => self.world.block(at.x, at.y, at.z),
        }
    }

    fn state(&self, at: IVec3) -> BlockState {
        self.get(at).unwrap_or(BlockState::AIR)
    }

    /// Sets a block and looks at the blocks around it.
    fn set(&mut self, at: IVec3, block: BlockState) {
        if self.get(at) == Some(block) {
            return;
        }
        if self.edits.insert(at, block).is_none() {
            self.order.push(at);
        }
        self.around(at);
    }

    fn queue(&mut self, at: IVec3) {
        if self.queued.insert(at) {
            self.work.push_back(at);
        }
    }

    /// Queues `at` and everything within two steps of it along the axes.
    fn around(&mut self, at: IVec3) {
        self.queue(at);
        for first in Face::ALL {
            let near = at + step_of(first);
            self.queue(near);
            for second in Face::ALL {
                if second != first.opposite() {
                    self.queue(near + step_of(second));
                }
            }
        }
    }

    fn schedule(&mut self, at: IVec3, delay: u64, priority: i8) {
        self.redstone.schedule(at, self.time, delay, priority);
    }

    fn is_scheduled(&self, at: IVec3) -> bool {
        self.redstone.scheduled.contains(&at)
    }

    /// Looks at queued blocks until nothing changes any more.
    fn settle(&mut self) {
        while let Some(at) = self.work.pop_front() {
            self.queued.remove(&at);
            if self.budget == 0 {
                self.work.clear();
                self.queued.clear();
                return;
            }
            self.budget -= 1;
            self.update(at);
        }
    }

    // --- Signals ---------------------------------------------------------

    /// The signal the block at `pos` gives to something looking at it from
    /// `dir` away (modern `Level.getSignal`). `wires` is false while dust
    /// works out its own power, when dust gives nothing.
    fn signal(&self, pos: IVec3, dir: Face, wires: bool) -> u8 {
        let Some(state) = self.get(pos) else {
            return 0;
        };
        let direct = self.output(state, dir, wires);
        if conductor(state) {
            direct.max(self.direct_into(pos, wires))
        } else {
            direct
        }
    }

    /// The strongest direct signal anything gives the block at `pos`.
    fn direct_into(&self, pos: IVec3, wires: bool) -> u8 {
        Face::ALL
            .iter()
            .map(|&dir| {
                let from = pos + step_of(dir);
                self.get(from)
                    .map_or(0, |state| self.direct(state, dir, wires))
            })
            .max()
            .unwrap_or(0)
    }

    /// What `state` gives toward the block looking at it from `dir` away
    /// (`getSignal`).
    fn output(&self, state: BlockState, dir: Face, wires: bool) -> u8 {
        let on = |yes: bool| if yes { 15 } else { 0 };
        match state.id() {
            blocks::REDSTONE_WIRE => self.wire_output(state, dir, wires),
            blocks::REDSTONE_TORCH => on(lit(state) && dir != Face::Top),
            blocks::REDSTONE_WALL_TORCH => on(lit(state) && state.face(Prop::Facing) != dir),
            blocks::LEVER
            | blocks::STONE_BUTTON
            | blocks::OAK_BUTTON
            | blocks::STONE_PRESSURE_PLATE
            | blocks::OAK_PRESSURE_PLATE
            | blocks::DETECTOR_RAIL => on(state.bool(Prop::Powered)),
            blocks::REDSTONE_BLOCK => 15,
            blocks::REPEATER => on(state.bool(Prop::Powered) && state.face(Prop::Facing) == dir),
            _ => 0,
        }
    }

    /// What dust gives toward the block looking at it from `dir` away.
    fn wire_output(&self, state: BlockState, dir: Face, wires: bool) -> u8 {
        let power = state.number(Prop::Power);
        if !wires || power == 0 || dir == Face::Bottom {
            0
        } else if dir == Face::Top || wire_joins(state, dir.opposite()) {
            power
        } else {
            0
        }
    }

    /// What `state` gives a solid block it touches, looked at from `dir`
    /// away (`getDirectSignal`).
    fn direct(&self, state: BlockState, dir: Face, wires: bool) -> u8 {
        match state.id() {
            blocks::REDSTONE_WIRE => self.wire_output(state, dir, wires),
            blocks::REDSTONE_TORCH | blocks::REDSTONE_WALL_TORCH => {
                if dir == Face::Bottom {
                    self.output(state, dir, wires)
                } else {
                    0
                }
            }
            blocks::LEVER | blocks::STONE_BUTTON | blocks::OAK_BUTTON => {
                if state.bool(Prop::Powered) && attached_direction(state) == dir {
                    15
                } else {
                    0
                }
            }
            blocks::STONE_PRESSURE_PLATE | blocks::OAK_PRESSURE_PLATE | blocks::DETECTOR_RAIL => {
                if state.bool(Prop::Powered) && dir == Face::Top {
                    15
                } else {
                    0
                }
            }
            blocks::REPEATER => self.output(state, dir, wires),
            _ => 0,
        }
    }

    /// The strongest signal around `pos`, not counting dust (modern
    /// `getBestNeighborSignal` while dust works out its power).
    fn best_neighbor_without_dust(&self, pos: IVec3) -> u8 {
        Face::ALL
            .iter()
            .map(|&dir| self.signal(pos + step_of(dir), dir, false))
            .max()
            .unwrap_or(0)
    }

    /// Whether anything powers the block at `pos` from any side (modern
    /// `hasNeighborSignal`).
    fn has_neighbor_signal(&self, pos: IVec3) -> bool {
        Face::ALL
            .iter()
            .any(|&dir| self.signal(pos + step_of(dir), dir, true) > 0)
    }

    /// Whether the block at `pos` is powered from `dir` (modern
    /// `hasSignal`).
    fn has_signal(&self, pos: IVec3, dir: Face) -> bool {
        self.signal(pos, dir, true) > 0
    }

    // --- Updates ---------------------------------------------------------

    fn update(&mut self, at: IVec3) {
        let Some(state) = self.get(at) else {
            return;
        };
        match state.id() {
            blocks::REDSTONE_WIRE => self.update_wire(at, state),
            blocks::REPEATER => self.update_repeater(at, state),
            blocks::REDSTONE_TORCH | blocks::REDSTONE_WALL_TORCH => self.update_torch(at, state),
            blocks::REDSTONE_LAMP => {
                let powered = self.has_neighbor_signal(at);
                if lit(state) && !powered {
                    self.schedule(at, LAMP_TICKS, priority::NORMAL);
                } else if !lit(state) && powered {
                    self.set(at, state.with_bool(Prop::Lit, true));
                }
            }
            blocks::NOTE_BLOCK => self.update_note_block(at, state),
            blocks::TNT => {
                if self.has_neighbor_signal(at) {
                    self.set(at, BlockState::AIR);
                    self.events.push(Event::Prime(at));
                }
            }
            blocks::DISPENSER | blocks::DROPPER => {
                let powered =
                    self.has_neighbor_signal(at) || self.has_neighbor_signal(at + IVec3::Y);
                let triggered = state.bool(Prop::Triggered);
                if powered && !triggered {
                    self.schedule(at, DISPENSE_TICKS, priority::NORMAL);
                    self.set(at, state.with_bool(Prop::Triggered, true));
                } else if !powered && triggered {
                    self.set(at, state.with_bool(Prop::Triggered, false));
                }
            }
            blocks::PISTON | blocks::STICKY_PISTON => self.update_piston(at, state),
            blocks::PISTON_HEAD => {
                // A head without its piston goes too.
                let facing = state.face(Prop::FacingAll);
                let base = self.state(at - step_of(facing));
                let holds = matches!(base.id(), blocks::PISTON | blocks::STICKY_PISTON)
                    && base.bool(Prop::Extended)
                    && base.face(Prop::FacingAll) == facing;
                if !holds {
                    self.set(at, BlockState::AIR);
                }
            }
            blocks::HOPPER => {
                let locked = self.has_neighbor_signal(at);
                if state.bool(Prop::Powered) != locked {
                    self.set(at, state.with_bool(Prop::Powered, locked));
                }
            }
            blocks::POWERED_RAIL | blocks::ACTIVATOR_RAIL => self.update_rail(at, state),
            _ => match state.def().shape {
                Shape::Door => self.update_door(at, state),
                Shape::Trapdoor | Shape::FenceGate => {
                    let powered = self.has_neighbor_signal(at);
                    if powered != state.bool(Prop::Powered) {
                        self.set(
                            at,
                            state
                                .with_bool(Prop::Powered, powered)
                                .with_bool(Prop::Open, powered),
                        );
                    }
                }
                _ => {}
            },
        }
    }

    fn update_wire(&mut self, at: IVec3, state: BlockState) {
        let power = self.wire_power(at);
        if power != state.number(Prop::Power) {
            self.set(at, state.with_number(Prop::Power, power));
        }
    }

    /// The power dust at `at` should have (modern
    /// `RedStoneWireBlock.calculateTargetPower`).
    fn wire_power(&self, at: IVec3) -> u8 {
        let from_parts = self.best_neighbor_without_dust(at);
        if from_parts >= 15 {
            return from_parts;
        }
        let dust = |pos: IVec3| {
            let state = self.state(pos);
            if state.is(blocks::REDSTONE_WIRE) {
                state.number(Prop::Power)
            } else {
                0
            }
        };
        let above_blocks = conductor(self.state(at + IVec3::Y));
        let mut from_dust = 0;
        for face in HORIZONTAL {
            let side = at + step_of(face);
            from_dust = from_dust.max(dust(side));
            let side_state = self.state(side);
            if conductor(side_state) {
                if !above_blocks {
                    from_dust = from_dust.max(dust(side + IVec3::Y));
                }
            } else {
                from_dust = from_dust.max(dust(side - IVec3::Y));
            }
        }
        from_parts.max(from_dust.saturating_sub(1))
    }

    /// The signal reaching a repeater at `at` from behind it.
    fn repeater_input(&self, at: IVec3, state: BlockState) -> u8 {
        let facing = state.face(Prop::Facing);
        let from = at + step_of(facing);
        let power = self.signal(from, facing, true);
        if power >= 15 {
            return power;
        }
        let behind = self.state(from);
        if behind.is(blocks::REDSTONE_WIRE) {
            power.max(behind.number(Prop::Power))
        } else {
            power
        }
    }

    /// Whether another repeater feeding a repeater's side holds it (modern
    /// `isLocked`).
    fn repeater_locked(&self, at: IVec3, state: BlockState) -> bool {
        let facing = state.face(Prop::Facing);
        [facing.rotate_cw(), facing.rotate_ccw()]
            .into_iter()
            .any(|side| {
                let other = self.state(at + step_of(side));
                is_diode(other) && self.output(other, side, true) > 0
            })
    }

    fn update_repeater(&mut self, at: IVec3, state: BlockState) {
        let locked = self.repeater_locked(at, state);
        let state = if state.bool(Prop::Locked) != locked {
            let state = state.with_bool(Prop::Locked, locked);
            self.set(at, state);
            state
        } else {
            state
        };
        if locked {
            return;
        }
        let on = state.bool(Prop::Powered);
        let wanted = self.repeater_input(at, state) > 0;
        if on == wanted || self.is_scheduled(at) {
            return;
        }
        // One feeding another across gets to go first.
        let facing = state.face(Prop::Facing);
        let ahead = self.state(at + step_of(facing.opposite()));
        let priority = if is_diode(ahead) && ahead.face(Prop::Facing) != facing {
            priority::EXTREMELY_HIGH
        } else if on {
            priority::VERY_HIGH
        } else {
            priority::HIGH
        };
        let delay = 2 * u64::from(state.number(Prop::Delay));
        self.schedule(at, delay, priority);
    }

    fn repeater_tick(&mut self, at: IVec3, state: BlockState) {
        if self.repeater_locked(at, state) {
            return;
        }
        let on = state.bool(Prop::Powered);
        let wanted = self.repeater_input(at, state) > 0;
        if on && !wanted {
            self.set(at, state.with_bool(Prop::Powered, false));
        } else if !on {
            self.set(at, state.with_bool(Prop::Powered, true));
            if !wanted {
                let delay = 2 * u64::from(state.number(Prop::Delay));
                self.schedule(at, delay, priority::VERY_HIGH);
            }
        }
    }

    /// Whether the block a torch hangs on is powered.
    fn torch_input(&self, at: IVec3, state: BlockState) -> bool {
        if state.is(blocks::REDSTONE_WALL_TORCH) {
            let toward = state.face(Prop::Facing).opposite();
            self.has_signal(at + step_of(toward), toward)
        } else {
            self.has_signal(at - IVec3::Y, Face::Bottom)
        }
    }

    fn update_torch(&mut self, at: IVec3, state: BlockState) {
        if lit(state) == self.torch_input(at, state) && !self.is_scheduled(at) {
            self.schedule(at, 2, priority::NORMAL);
        }
    }

    /// Whether the torch at `at` has toggled too often lately; `add` notes
    /// a toggle now.
    fn burnt_out(&mut self, at: IVec3, add: bool) -> bool {
        let time = self.time;
        let toggles = self.redstone.toggles.entry(at).or_default();
        toggles.retain(|&when| time - when <= BURNOUT_WINDOW);
        if add {
            toggles.push(time);
        }
        let too_many = toggles.len() >= BURNOUT_TOGGLES;
        if toggles.is_empty() {
            self.redstone.toggles.remove(&at);
        }
        too_many
    }

    fn torch_tick(&mut self, at: IVec3, state: BlockState) {
        let powered = self.torch_input(at, state);
        if lit(state) {
            if powered {
                self.set(at, state.with_bool(Prop::Lit, false));
                if self.burnt_out(at, true) {
                    self.schedule(at, BURNOUT_TICKS, priority::NORMAL);
                }
            }
        } else if !powered && !self.burnt_out(at, false) {
            self.set(at, state.with_bool(Prop::Lit, true));
        }
    }

    fn update_note_block(&mut self, at: IVec3, state: BlockState) {
        let instrument = instrument_over(self.state(at - IVec3::Y));
        let state = if state.get(Prop::Instrument) != instrument {
            let state = state.with(Prop::Instrument, instrument);
            self.set(at, state);
            state
        } else {
            state
        };
        let powered = self.has_neighbor_signal(at);
        if powered == state.bool(Prop::Powered) {
            return;
        }
        self.set(at, state.with_bool(Prop::Powered, powered));
        if powered && self.state(at + IVec3::Y).is_air() {
            self.events.push(Event::PlayNote {
                at,
                instrument,
                note: state.get(Prop::Note),
            });
        }
    }

    fn update_door(&mut self, at: IVec3, state: BlockState) {
        let other = if state.get(Prop::DoubleBlockHalf) == door_half::LOWER {
            at + IVec3::Y
        } else {
            at - IVec3::Y
        };
        let powered = self.has_neighbor_signal(at) || self.has_neighbor_signal(other);
        if powered != state.bool(Prop::Powered) {
            self.set(
                at,
                state
                    .with_bool(Prop::Powered, powered)
                    .with_bool(Prop::Open, powered),
            );
        }
    }

    /// Whether a piston at `at` is powered (modern
    /// `PistonBaseBlock.getNeighborSignal`): from any side but its front,
    /// from below it, or like a block above it (quasi-connectivity).
    fn piston_powered(&self, at: IVec3, state: BlockState) -> bool {
        let facing = state.face(Prop::FacingAll);
        if Face::ALL
            .iter()
            .any(|&dir| dir != facing && self.has_signal(at + step_of(dir), dir))
        {
            return true;
        }
        if self.has_signal(at, Face::Bottom) {
            return true;
        }
        let above = at + IVec3::Y;
        Face::ALL
            .iter()
            .any(|&dir| dir != Face::Bottom && self.has_signal(above + step_of(dir), dir))
    }

    fn update_piston(&mut self, at: IVec3, state: BlockState) {
        let facing = state.face(Prop::FacingAll);
        let head = self.state(at + step_of(facing));
        // An extended piston whose head was broken is a plain one again.
        if state.bool(Prop::Extended)
            && !(head.is(blocks::PISTON_HEAD) && head.face(Prop::FacingAll) == facing)
        {
            self.set(at, state.with_bool(Prop::Extended, false));
            return;
        }
        let powered = self.piston_powered(at, state);
        let extended = state.bool(Prop::Extended);
        if powered != extended && !self.redstone.pending_pistons.contains(&at) {
            self.redstone.pending_pistons.insert(at);
            self.events.push(Event::Piston {
                at,
                extend: powered,
            });
        }
    }

    /// Whether a rail of the same kind in line with the rail at `at`, up
    /// to `RAIL_POWER_RANGE` rails away `forward` or back, is powered
    /// (modern `PoweredRailBlock.findPoweredRailSignal`).
    fn rail_powered_along(&self, at: IVec3, state: BlockState, forward: bool, depth: u32) -> bool {
        if depth >= RAIL_POWER_RANGE {
            return false;
        }
        let mut next = at;
        let mut check_below = true;
        let shape = state.get(Prop::StraightRailShape);
        // The shape the next rail must run along.
        let axis_x = match shape {
            rail_shape::NORTH_SOUTH => {
                next.z += if forward { 1 } else { -1 };
                false
            }
            rail_shape::EAST_WEST => {
                next.x += if forward { -1 } else { 1 };
                true
            }
            rail_shape::ASCENDING_EAST => {
                if forward {
                    next.x -= 1;
                } else {
                    next.x += 1;
                    next.y += 1;
                    check_below = false;
                }
                true
            }
            rail_shape::ASCENDING_WEST => {
                if forward {
                    next.x -= 1;
                    next.y += 1;
                    check_below = false;
                } else {
                    next.x += 1;
                }
                true
            }
            rail_shape::ASCENDING_NORTH => {
                if forward {
                    next.z += 1;
                } else {
                    next.z -= 1;
                    next.y += 1;
                    check_below = false;
                }
                false
            }
            _ => {
                if forward {
                    next.z += 1;
                    next.y += 1;
                    check_below = false;
                } else {
                    next.z -= 1;
                }
                false
            }
        };
        self.rail_carries_power(next, state, axis_x, forward, depth)
            || (check_below
                && self.rail_carries_power(next - IVec3::Y, state, axis_x, forward, depth))
    }

    fn rail_carries_power(
        &self,
        at: IVec3,
        like: BlockState,
        axis_x: bool,
        forward: bool,
        depth: u32,
    ) -> bool {
        let state = self.state(at);
        if state.id() != like.id() {
            return false;
        }
        let shape = state.get(Prop::StraightRailShape);
        let runs_x = matches!(
            shape,
            rail_shape::EAST_WEST | rail_shape::ASCENDING_EAST | rail_shape::ASCENDING_WEST
        );
        if runs_x != axis_x || !state.bool(Prop::Powered) {
            return false;
        }
        self.has_neighbor_signal(at)
            || self.has_neighbor_signal(at + IVec3::Y)
            || self.rail_powered_along(at, state, forward, depth + 1)
    }

    fn update_rail(&mut self, at: IVec3, state: BlockState) {
        let powered = self.has_neighbor_signal(at)
            || self.has_neighbor_signal(at + IVec3::Y)
            || self.rail_powered_along(at, state, true, 0)
            || self.rail_powered_along(at, state, false, 0);
        if powered != state.bool(Prop::Powered) {
            self.set(at, state.with_bool(Prop::Powered, powered));
        }
    }

    // --- Scheduled ticks -------------------------------------------------

    fn tick_block(&mut self, at: IVec3) {
        let Some(state) = self.get(at) else {
            return;
        };
        match state.id() {
            blocks::REPEATER => self.repeater_tick(at, state),
            blocks::REDSTONE_TORCH | blocks::REDSTONE_WALL_TORCH => self.torch_tick(at, state),
            blocks::REDSTONE_LAMP => {
                if lit(state) && !self.has_neighbor_signal(at) {
                    self.set(at, state.with_bool(Prop::Lit, false));
                }
            }
            blocks::STONE_BUTTON | blocks::OAK_BUTTON => {
                if state.bool(Prop::Powered) {
                    self.set(at, state.with_bool(Prop::Powered, false));
                }
            }
            blocks::STONE_PRESSURE_PLATE | blocks::OAK_PRESSURE_PLATE | blocks::DETECTOR_RAIL => {
                if state.bool(Prop::Powered) {
                    if self.pressed.contains(&at) {
                        self.schedule(at, PLATE_TICKS, priority::NORMAL);
                    } else {
                        self.set(at, state.with_bool(Prop::Powered, false));
                    }
                }
            }
            blocks::DISPENSER | blocks::DROPPER => self.events.push(Event::Dispense(at)),
            _ => {}
        }
    }
}

fn lit(state: BlockState) -> bool {
    state.bool(Prop::Lit)
}
