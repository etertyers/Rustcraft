//! Redstone in the played world: each tick runs the engine in
//! `world::redstone`, applies what it changed, finds the plates something
//! stands on, and carries out what it reports: pistons pushing and pulling
//! (`world::piston`), note blocks playing and so on. Levers, buttons,
//! repeaters and note blocks are used by right-click.

use glam::{DVec3, IVec3};

use super::Game;
use crate::{
    entity::item::ItemEntity,
    player::collision::Aabb,
    world::{
        Face,
        block::BlockState,
        blocks,
        fast_hash::{FastMap, FastSet},
        harvest, piston,
        props::Prop,
        redstone::{self, Event, priority},
    },
};

/// How far an entity's box must reach into a plate's cell for it to be
/// pressed: the plate's 15/16-wide, 1/4-high sensing box (modern
/// `TOUCH_AABB`).
const PLATE_INSET: f64 = 1.0 / 16.0;
const PLATE_HEIGHT: f64 = 0.25;
/// Most notes kept for the sound code to play.
const NOTE_LOG: usize = 64;

impl Game {
    /// Sets blocks the engine changed. They were already settled there, so
    /// the engine isn't told again; everything else is.
    fn apply_redstone(&mut self, edits: &[(IVec3, BlockState)]) {
        self.chunks.set_blocks_later(edits);
        for &(at, _) in edits {
            self.fluids.changed(at);
            self.fires.changed(at);
        }
        let cells: Vec<IVec3> = edits.iter().map(|&(at, _)| at).collect();
        self.update_neighbors(&cells);
    }

    /// One tick of redstone.
    pub(super) fn tick_redstone(&mut self) {
        let pressed = self.press_plates();
        let outcome = self.redstone.tick(self.chunks.world(), self.time, &pressed);
        if !outcome.edits.is_empty() {
            self.apply_redstone(&outcome.edits);
        }
        for event in outcome.events {
            self.carry_out(event);
        }
    }

    /// Presses the plates and detector rails something is on, which at
    /// once, and returns the cells that are pressed.
    fn press_plates(&mut self) -> FastSet<IVec3> {
        let mut pressed = FastSet::default();
        let mut boxes = self.pressers();
        boxes.extend(self.rail_pressers());
        let world = self.chunks.world();
        let mut presses = Vec::new();
        for (bounds, kind) in boxes {
            let (lo, hi) = (bounds.min, bounds.max);
            for x in lo.x.floor() as i32..=hi.x.floor() as i32 {
                for z in lo.z.floor() as i32..=hi.z.floor() as i32 {
                    for y in (lo.y - PLATE_HEIGHT).floor() as i32..=hi.y.floor() as i32 {
                        let Some(state) = world.block(x, y, z) else {
                            continue;
                        };
                        if !kind.presses(state) {
                            continue;
                        }
                        let cell = Aabb::block(x, y, z);
                        let sense = Aabb::new(
                            cell.min + DVec3::new(PLATE_INSET, 0.0, PLATE_INSET),
                            DVec3::new(
                                cell.max.x - PLATE_INSET,
                                cell.min.y + PLATE_HEIGHT,
                                cell.max.z - PLATE_INSET,
                            ),
                        );
                        if overlaps(&bounds, &sense) {
                            let at = IVec3::new(x, y, z);
                            if pressed.insert(at) && !state.bool(Prop::Powered) {
                                presses.push((at, state));
                            }
                        }
                    }
                }
            }
        }
        for (at, state) in presses {
            self.edit(&[(at, state.with_bool(Prop::Powered, true))]);
            self.redstone.schedule(
                at,
                self.time,
                redstone::press_ticks(state),
                priority::NORMAL,
            );
        }
        pressed
    }

    /// Boxes that press pressure plates: everything on the ground for a
    /// wooden plate, living things for a stone one.
    fn pressers(&self) -> Vec<(Aabb, Presser)> {
        let mut boxes = Vec::new();
        if !self.stats.is_dead() {
            boxes.push((self.body.aabb(), Presser::Living));
        }
        boxes.extend(
            self.mobs
                .iter()
                .filter(|mob| !mob.is_dead())
                .map(|mob| (mob.body.aabb(), Presser::Living)),
        );
        boxes.extend(
            self.entities
                .iter()
                .map(|item| (item.bounds(), Presser::Anything)),
        );
        boxes.extend(
            self.arrows
                .iter()
                .map(|arrow| (arrow.bounds(), Presser::Anything)),
        );
        boxes
    }

    /// Records a note for the sound code.
    fn play_note(&mut self, at: IVec3, instrument: u8, note: u8) {
        if self.played_notes.len() >= NOTE_LOG {
            self.played_notes.remove(0);
        }
        self.played_notes.push((at, instrument, note));
    }

    fn carry_out(&mut self, event: Event) {
        match event {
            Event::PlayNote {
                at,
                instrument,
                note,
            } => self.play_note(at, instrument, note),
            Event::Piston { at, extend } => {
                self.move_piston(at, extend);
                self.redstone.piston_done(at);
            }
            Event::Dispense(at) => self.dispense(at),
            Event::Prime(at) => self.prime_tnt(at),
        }
    }

    /// Extends or retracts the piston at `at` if it still can (modern
    /// `PistonBaseBlock.triggerEvent`): the blocks in front move a step,
    /// anything in the way that breaks drops, and entities in the cells
    /// they move into are shoved on.
    fn move_piston(&mut self, at: IVec3, extend: bool) {
        let world = self.chunks.world();
        let Some(base) = world.block(at.x, at.y, at.z) else {
            return;
        };
        if !matches!(base.id(), blocks::PISTON | blocks::STICKY_PISTON)
            || base.bool(Prop::Extended) == extend
        {
            return;
        }
        let facing = base.face(Prop::FacingAll);
        let sticky = base.is(blocks::STICKY_PISTON);
        let front = at + IVec3::from_array(facing.normal());
        let get = |p: IVec3| world.block(p.x, p.y, p.z);
        let mut moved: FastMap<IVec3, BlockState> = FastMap::default();
        let mut order = Vec::new();
        let mut destroyed = Vec::new();
        let mut shove = Vec::new();
        let mut direction = facing;
        if extend {
            let Some(plan) = piston::resolve(&get, at, facing, true) else {
                return;
            };
            direction = plan.direction;
            let step = IVec3::from_array(direction.normal());
            for &p in &plan.push {
                moved.insert(p, BlockState::AIR);
                order.push(p);
            }
            for &p in &plan.push {
                let state = get(p).unwrap_or(BlockState::AIR);
                moved.insert(p + step, state);
                order.push(p + step);
                shove.push(p + step);
            }
            destroyed = plan.destroy;
            let head = BlockState::of(blocks::PISTON_HEAD)
                .with_face(Prop::FacingAll, facing)
                .with(Prop::PistonType, u8::from(sticky))
                .with_bool(Prop::Short, false);
            moved.insert(front, head);
            order.push(front);
            shove.push(front);
            moved.insert(at, base.with_bool(Prop::Extended, true));
            order.push(at);
        } else {
            if get(front).is_some_and(|head| head.is(blocks::PISTON_HEAD)) {
                moved.insert(front, BlockState::AIR);
                order.push(front);
            }
            moved.insert(at, base.with_bool(Prop::Extended, false));
            order.push(at);
            if sticky {
                // The head is gone from the world the piston looks at.
                let pulled = {
                    let get_without_head = |p: IVec3| {
                        if p == front {
                            Some(BlockState::AIR)
                        } else {
                            get(p)
                        }
                    };
                    let far = front + IVec3::from_array(facing.normal());
                    let block = get_without_head(far).unwrap_or(BlockState::AIR);
                    let pulls = !block.is_air()
                        && piston::pushable(block, far, facing.opposite(), false)
                        && piston::reaction(block) == piston::Reaction::Normal;
                    pulls
                        .then(|| piston::resolve(&get_without_head, at, facing, false))
                        .flatten()
                };
                if let Some(plan) = pulled {
                    direction = plan.direction;
                    let step = IVec3::from_array(direction.normal());
                    for &p in &plan.push {
                        moved.insert(p, BlockState::AIR);
                    }
                    for &p in &plan.push {
                        let state = get(p).unwrap_or(BlockState::AIR);
                        moved.insert(p + step, state);
                        order.push(p + step);
                        shove.push(p + step);
                    }
                    // Source cells go first so what moves in wins.
                    for &p in &plan.push {
                        order.push(p);
                    }
                }
            }
        }
        let mut seen = FastSet::default();
        let edits: Vec<(IVec3, BlockState)> = order
            .into_iter()
            .filter(|p| seen.insert(*p))
            .map(|p| (p, moved[&p]))
            .collect();
        for cell in destroyed {
            let Some(state) = self.chunks.world().block(cell.x, cell.y, cell.z) else {
                continue;
            };
            let rng = &mut self.rng;
            let mut random = || rng.next_f64() as f32;
            for stack in harvest::drops(state, None, &mut random) {
                self.entities
                    .push(ItemEntity::from_block(stack, cell, &mut random));
            }
        }
        self.edit(&edits);
        self.shove_entities(&shove, direction);
    }

    /// Pushes entities out of `cells` along `toward`, as blocks moving
    /// into them do.
    fn shove_entities(&mut self, cells: &[IVec3], toward: Face) {
        let step = DVec3::from_array(toward.normal().map(f64::from));
        let shift = |bounds: &Aabb| -> Option<DVec3> {
            let mut by: Option<f64> = None;
            for cell in cells {
                let block = Aabb::block(cell.x, cell.y, cell.z);
                if !overlaps(bounds, &block) {
                    continue;
                }
                let axis = if step.x != 0.0 {
                    0
                } else if step.y != 0.0 {
                    1
                } else {
                    2
                };
                let amount = if step[axis] > 0.0 {
                    block.max[axis] - bounds.min[axis]
                } else {
                    bounds.max[axis] - block.min[axis]
                };
                by = Some(by.map_or(amount, |b| b.max(amount)));
            }
            by.map(|amount| step * (amount + 1e-3))
        };
        if let Some(by) = shift(&self.body.aabb()) {
            let feet = self.body.position() + by;
            self.body.set_position(feet);
        }
        for mob in &mut self.mobs {
            if let Some(by) = shift(&mob.body.aabb()) {
                let feet = mob.body.position() + by;
                mob.body.set_position(feet);
            }
        }
        for item in &mut self.entities {
            if let Some(by) = shift(&item.bounds()) {
                item.shift(by);
            }
        }
    }

    /// Right-click on a lever, button, repeater or note block. Returns
    /// whether the block did anything.
    pub(super) fn use_redstone_block(&mut self, cell: IVec3) -> bool {
        let Some(state) = self.chunks.world().block(cell.x, cell.y, cell.z) else {
            return false;
        };
        match state.kind() {
            blocks::LEVER => {
                let on = !state.bool(Prop::Powered);
                self.edit(&[(cell, state.with_bool(Prop::Powered, on))]);
            }
            blocks::STONE_BUTTON | blocks::OAK_BUTTON => {
                if state.bool(Prop::Powered) {
                    return true;
                }
                self.edit(&[(cell, state.with_bool(Prop::Powered, true))]);
                self.redstone.schedule(
                    cell,
                    self.time,
                    redstone::press_ticks(state),
                    priority::NORMAL,
                );
            }
            blocks::REPEATER => {
                let delay = state.number(Prop::Delay) % 4 + 1;
                self.edit(&[(cell, state.with_number(Prop::Delay, delay))]);
            }
            blocks::NOTE_BLOCK => {
                if self
                    .chunks
                    .world()
                    .block(cell.x, cell.y + 1, cell.z)
                    .is_some_and(|above| !above.is_air())
                {
                    return true;
                }
                let note = (state.get(Prop::Note) + 1) % 25;
                let tuned = state.with(Prop::Note, note);
                self.edit(&[(cell, tuned)]);
                self.play_note(cell, tuned.get(Prop::Instrument), note);
            }
            _ => return false,
        }
        true
    }
}

/// What can press a plate.
#[derive(Debug, Clone, Copy)]
pub(super) enum Presser {
    /// Players and mobs (stone plates, and detector rails for minecarts).
    Living,
    /// Everything: items and arrows too (wooden plates).
    Anything,
    /// Minecarts, which press detector rails.
    Cart,
}

impl Presser {
    /// Whether this kind of thing presses the block `state`.
    fn presses(self, state: BlockState) -> bool {
        match state.kind() {
            blocks::STONE_PRESSURE_PLATE => matches!(self, Self::Living),
            blocks::OAK_PRESSURE_PLATE => true,
            blocks::DETECTOR_RAIL => matches!(self, Self::Cart),
            _ => false,
        }
    }
}

fn overlaps(a: &Aabb, b: &Aabb) -> bool {
    a.min.cmplt(b.max).all() && a.max.cmpgt(b.min).all()
}

#[cfg(test)]
mod tests {
    use glam::IVec3;

    use super::*;
    use crate::{
        game::tests::flat_game,
        player::mode::GameMode,
        world::{
            block::BlockState,
            blocks, connect,
            props::{Prop, attach_face},
        },
    };

    /// Y of the first air layer in the flat world.
    const Y: i32 = 64;

    fn at(x: i32, z: i32) -> IVec3 {
        IVec3::new(x, Y, z)
    }

    fn block_at(game: &Game, at: IVec3) -> BlockState {
        game.chunks.world().block(at.x, at.y, at.z).unwrap()
    }

    /// Places blocks and joins up the dust among them, as placing by hand
    /// does.
    fn put(game: &mut Game, cells: &[(IVec3, BlockState)]) {
        game.set_blocks(cells);
        for _ in 0..3 {
            for &(cell, _) in cells {
                let world = game.chunks.world();
                let Some(state) = world.block(cell.x, cell.y, cell.z) else {
                    continue;
                };
                let shaped = connect::update_shape(state, cell, &|p| world.block(p.x, p.y, p.z));
                if shaped != state {
                    game.set_blocks(&[(cell, shaped)]);
                }
            }
        }
    }

    fn dust() -> BlockState {
        BlockState::of(blocks::REDSTONE_WIRE)
    }

    fn lever_on_floor() -> BlockState {
        BlockState::of(blocks::LEVER).with(Prop::AttachFace, attach_face::FLOOR)
    }

    fn power(game: &Game, at: IVec3) -> u8 {
        block_at(game, at).number(Prop::Power)
    }

    fn lit(game: &Game, at: IVec3) -> bool {
        block_at(game, at).bool(Prop::Lit)
    }

    #[test]
    fn dust_carries_a_signal_fifteen_blocks_and_fades() {
        let mut game = flat_game(GameMode::Creative);
        let mut cells: Vec<_> = (1..=17).map(|x| (at(x, 4), dust())).collect();
        cells.push((at(0, 4), BlockState::of(blocks::REDSTONE_BLOCK)));
        put(&mut game, &cells);
        game.run_ticks(2);
        for x in 1..=15 {
            assert_eq!(power(&game, at(x, 4)), (16 - x) as u8, "dust at {x}");
        }
        assert_eq!(
            power(&game, at(16, 4)),
            0,
            "fifteen blocks is as far as it goes"
        );
        // Taking the source away drains the whole line.
        game.set_blocks(&[(at(0, 4), BlockState::AIR)]);
        game.run_ticks(2);
        for x in 1..=17 {
            assert_eq!(power(&game, at(x, 4)), 0, "dust at {x} after");
        }
    }

    #[test]
    fn a_lever_through_dust_lights_a_lamp_and_it_goes_out_four_ticks_after() {
        let mut game = flat_game(GameMode::Creative);
        put(
            &mut game,
            &[
                (at(0, 8), lever_on_floor()),
                (at(1, 8), dust()),
                (at(2, 8), dust()),
                (at(3, 8), BlockState::of(blocks::REDSTONE_LAMP)),
            ],
        );
        game.run_ticks(2);
        assert!(!lit(&game, at(3, 8)));
        assert!(game.use_redstone_block(at(0, 8)));
        game.run_ticks(1);
        assert!(lit(&game, at(3, 8)), "the lamp lights at once");
        assert!(game.use_redstone_block(at(0, 8)));
        game.run_ticks(3);
        assert!(lit(&game, at(3, 8)), "and stays lit for a moment");
        game.run_ticks(2);
        assert!(!lit(&game, at(3, 8)), "then goes out");
    }

    #[test]
    fn a_repeater_delays_by_its_setting_and_renews_the_signal() {
        let mut game = flat_game(GameMode::Creative);
        // Lever, repeater facing west (input from the west), dust.
        let repeater = BlockState::of(blocks::REPEATER).with_face(Prop::Facing, Face::West);
        let mut cells = vec![(at(0, 12), lever_on_floor()), (at(1, 12), repeater)];
        cells.extend((2..=17).map(|x| (at(x, 12), dust())));
        put(&mut game, &cells);
        game.run_ticks(2);
        game.use_redstone_block(at(0, 12));
        game.run_ticks(1);
        assert!(!block_at(&game, at(1, 12)).bool(Prop::Powered), "not yet");
        game.run_ticks(2);
        assert!(
            block_at(&game, at(1, 12)).bool(Prop::Powered),
            "after one delay step"
        );
        assert_eq!(power(&game, at(2, 12)), 15, "a fresh full signal");
        assert_eq!(power(&game, at(16, 12)), 1);
        // Turn it up to the longest delay: four steps, eight ticks.
        game.use_redstone_block(at(0, 12));
        game.run_ticks(4);
        assert!(!block_at(&game, at(1, 12)).bool(Prop::Powered));
        for _ in 0..3 {
            game.use_redstone_block(at(1, 12));
        }
        assert_eq!(block_at(&game, at(1, 12)).number(Prop::Delay), 4);
        game.use_redstone_block(at(0, 12));
        game.run_ticks(7);
        assert!(!block_at(&game, at(1, 12)).bool(Prop::Powered));
        game.run_ticks(2);
        assert!(block_at(&game, at(1, 12)).bool(Prop::Powered));
    }

    #[test]
    fn a_torch_inverts_and_burns_out_when_toggled_too_fast() {
        let mut game = flat_game(GameMode::Creative);
        // Lever on a stone block with a torch on its far side.
        let torch = BlockState::of(blocks::REDSTONE_WALL_TORCH).with_face(Prop::Facing, Face::East);
        put(
            &mut game,
            &[
                (at(0, 16), lever_on_floor()),
                (at(1, 16), BlockState::of(blocks::STONE)),
                (at(2, 16), torch),
            ],
        );
        game.run_ticks(3);
        assert!(lit(&game, at(2, 16)), "lit with nothing powering its block");
        // The lever powers the stone only through the strong power of a
        // block it is attached to, so put it on the stone itself instead.
        game.set_blocks(&[
            (at(0, 16), BlockState::AIR),
            (IVec3::new(1, Y + 1, 16), lever_on_floor()),
        ]);
        game.use_redstone_block(IVec3::new(1, Y + 1, 16));
        game.run_ticks(3);
        assert!(!lit(&game, at(2, 16)), "a powered block puts the torch out");
        game.use_redstone_block(IVec3::new(1, Y + 1, 16));
        game.run_ticks(3);
        assert!(lit(&game, at(2, 16)), "and it lights again");
        // Flicking the lever every two ticks burns the torch out.
        for _ in 0..20 {
            game.use_redstone_block(IVec3::new(1, Y + 1, 16));
            game.run_ticks(3);
        }
        game.set_blocks(&[(IVec3::new(1, Y + 1, 16), BlockState::AIR)]);
        game.run_ticks(10);
        assert!(
            !lit(&game, at(2, 16)),
            "burnt out, even with nothing powering it"
        );
        game.run_ticks(170);
        assert!(lit(&game, at(2, 16)), "it recovers");
    }

    #[test]
    fn a_pressure_plate_is_pressed_by_the_player_and_releases_after_they_leave() {
        let mut game = flat_game(GameMode::Survival);
        let lamp = IVec3::new(2, Y, 0);
        let plate = BlockState::of(blocks::STONE_PRESSURE_PLATE);
        put(
            &mut game,
            &[
                (at(0, 0), plate),
                (lamp, BlockState::of(blocks::REDSTONE_LAMP)),
            ],
        );
        // The player stands at (0.5, 64, 0.5), on the plate. Dust joins them.
        put(&mut game, &[(at(1, 0), dust())]);
        game.run_ticks(3);
        assert!(block_at(&game, at(0, 0)).bool(Prop::Powered));
        assert!(lit(&game, lamp));
        // Step off.
        game.body.set_position(glam::DVec3::new(5.5, 64.0, 5.5));
        game.run_ticks(30);
        assert!(!block_at(&game, at(0, 0)).bool(Prop::Powered));
        assert!(!lit(&game, lamp));
    }

    #[test]
    fn doors_trapdoors_and_gates_open_when_powered() {
        let mut game = flat_game(GameMode::Creative);
        let trapdoor = BlockState::of(blocks::IRON_TRAPDOOR);
        let gate = BlockState::of(blocks::OAK_FENCE_GATE);
        put(
            &mut game,
            &[
                (at(0, 20), trapdoor),
                (at(1, 20), BlockState::of(blocks::REDSTONE_BLOCK)),
                (at(0, 22), gate),
            ],
        );
        game.run_ticks(2);
        assert!(
            block_at(&game, at(0, 20)).bool(Prop::Open),
            "iron trapdoors open for redstone"
        );
        assert!(!block_at(&game, at(0, 22)).bool(Prop::Open));
        game.set_blocks(&[(at(1, 22), BlockState::of(blocks::REDSTONE_BLOCK))]);
        game.run_ticks(2);
        assert!(block_at(&game, at(0, 22)).bool(Prop::Open));
        game.set_blocks(&[(at(1, 20), BlockState::AIR), (at(1, 22), BlockState::AIR)]);
        game.run_ticks(2);
        assert!(!block_at(&game, at(0, 20)).bool(Prop::Open));
        assert!(!block_at(&game, at(0, 22)).bool(Prop::Open));
    }

    #[test]
    fn an_iron_door_opens_from_either_half() {
        use crate::world::props::door_half;
        let mut game = flat_game(GameMode::Creative);
        let door = BlockState::of(blocks::IRON_DOOR);
        let lower = door.with(Prop::DoubleBlockHalf, door_half::LOWER);
        let upper = door.with(Prop::DoubleBlockHalf, door_half::UPPER);
        put(
            &mut game,
            &[
                (at(0, 26), lower),
                (IVec3::new(0, Y + 1, 26), upper),
                (at(1, 26), BlockState::of(blocks::REDSTONE_BLOCK)),
            ],
        );
        game.run_ticks(2);
        assert!(block_at(&game, at(0, 26)).bool(Prop::Open));
        assert!(block_at(&game, IVec3::new(0, Y + 1, 26)).bool(Prop::Open));
    }

    #[test]
    fn a_note_block_plays_its_tuned_note_over_its_material_when_powered() {
        let mut game = flat_game(GameMode::Creative);
        put(
            &mut game,
            &[
                (IVec3::new(0, Y - 1, 30), BlockState::of(blocks::OAK_PLANKS)),
                (at(0, 30), BlockState::of(blocks::NOTE_BLOCK)),
            ],
        );
        game.run_ticks(2);
        assert!(game.use_redstone_block(at(0, 30)));
        assert!(game.use_redstone_block(at(0, 30)));
        assert_eq!(block_at(&game, at(0, 30)).get(Prop::Note), 2);
        game.played_notes.clear();
        game.set_blocks(&[(at(1, 30), BlockState::of(blocks::REDSTONE_BLOCK))]);
        game.run_ticks(2);
        assert_eq!(game.played_notes.len(), 1);
        let (_, instrument, note) = game.played_notes[0];
        assert_eq!((instrument, note), (4, 2), "bass over wood, tuned to 2");
    }

    #[test]
    fn a_repeater_ring_pulses_as_a_clock() {
        let mut game = flat_game(GameMode::Creative);
        // A 3×3 ring of dust with a repeater in the middle of two sides
        // sending the signal round clockwise (east along the north side,
        // west along the south side).
        let east = BlockState::of(blocks::REPEATER).with_face(Prop::Facing, Face::West);
        let west = BlockState::of(blocks::REPEATER).with_face(Prop::Facing, Face::East);
        let (x0, z0) = (6, -10);
        let ring = [
            (0, 0, dust()),
            (1, 0, east),
            (2, 0, dust()),
            (2, 1, dust()),
            (2, 2, dust()),
            (1, 2, west),
            (0, 2, dust()),
            (0, 1, dust()),
        ];
        let cells: Vec<_> = ring
            .iter()
            .map(|&(x, z, s)| (at(x0 + x, z0 + z), s))
            .collect();
        put(&mut game, &cells);
        // A lamp watches one corner of the ring; a redstone block there for a
        // tick starts a pulse shorter than the ring's lap, which then goes round.
        put(
            &mut game,
            &[
                (at(x0 + 3, z0 + 1), dust()),
                (at(x0 + 4, z0 + 1), BlockState::of(blocks::REDSTONE_LAMP)),
                (at(x0 - 1, z0), BlockState::of(blocks::REDSTONE_BLOCK)),
            ],
        );
        game.run_ticks(1);
        game.set_blocks(&[(at(x0 - 1, z0), BlockState::AIR)]);
        let mut changes = 0;
        let mut last = lit(&game, at(x0 + 4, z0 + 1));
        for _ in 0..120 {
            game.run_ticks(1);
            let now = lit(&game, at(x0 + 4, z0 + 1));
            if now != last {
                changes += 1;
                last = now;
            }
        }
        assert!(
            changes >= 4,
            "the pulse kept going round: {changes} changes"
        );
    }

    fn piston(sticky: bool, facing: Face) -> BlockState {
        let id = if sticky {
            blocks::STICKY_PISTON
        } else {
            blocks::PISTON
        };
        BlockState::of(id).with_face(Prop::FacingAll, facing)
    }

    fn id_at(game: &Game, at: IVec3) -> crate::world::block::BlockId {
        block_at(game, at).id()
    }

    #[test]
    fn a_piston_pushes_a_line_of_blocks_and_pulls_back_when_unpowered() {
        let mut game = flat_game(GameMode::Creative);
        let z = -12;
        put(
            &mut game,
            &[
                (at(5, z), piston(false, Face::East)),
                (at(6, z), BlockState::of(blocks::STONE)),
                (at(7, z), BlockState::of(blocks::DIRT)),
            ],
        );
        game.set_blocks(&[(at(5, z + 1), BlockState::of(blocks::REDSTONE_BLOCK))]);
        game.run_ticks(2);
        assert!(block_at(&game, at(5, z)).bool(Prop::Extended));
        assert_eq!(id_at(&game, at(6, z)), blocks::PISTON_HEAD);
        assert_eq!(id_at(&game, at(7, z)), blocks::STONE);
        assert_eq!(id_at(&game, at(8, z)), blocks::DIRT);
        // Take the power away: a plain piston leaves what it pushed.
        game.set_blocks(&[(at(5, z + 1), BlockState::AIR)]);
        game.run_ticks(2);
        assert!(!block_at(&game, at(5, z)).bool(Prop::Extended));
        assert_eq!(id_at(&game, at(6, z)), blocks::AIR);
        assert_eq!(id_at(&game, at(7, z)), blocks::STONE);
    }

    #[test]
    fn a_sticky_piston_brings_its_block_back() {
        let mut game = flat_game(GameMode::Creative);
        let z = -9;
        put(
            &mut game,
            &[
                (at(12, z), piston(true, Face::West)),
                (at(11, z), BlockState::of(blocks::STONE)),
            ],
        );
        game.set_blocks(&[(at(12, z + 1), BlockState::of(blocks::REDSTONE_BLOCK))]);
        game.run_ticks(2);
        assert_eq!(id_at(&game, at(10, z)), blocks::STONE, "pushed out");
        assert_eq!(id_at(&game, at(11, z)), blocks::PISTON_HEAD);
        game.set_blocks(&[(at(12, z + 1), BlockState::AIR)]);
        game.run_ticks(2);
        assert_eq!(id_at(&game, at(11, z)), blocks::STONE, "pulled back");
        assert_eq!(id_at(&game, at(10, z)), blocks::AIR);
    }

    #[test]
    fn a_piston_stays_put_when_blocked() {
        let mut game = flat_game(GameMode::Creative);
        let z = -6;
        put(
            &mut game,
            &[
                (at(5, z), piston(false, Face::East)),
                (at(6, z), BlockState::of(blocks::OBSIDIAN)),
            ],
        );
        game.set_blocks(&[(at(5, z + 1), BlockState::of(blocks::REDSTONE_BLOCK))]);
        game.run_ticks(5);
        assert!(!block_at(&game, at(5, z)).bool(Prop::Extended));
        assert_eq!(id_at(&game, at(6, z)), blocks::OBSIDIAN);
        // Thirteen blocks are too many; twelve go.
        let mut cells = vec![(at(5, z + 4), piston(false, Face::East))];
        cells.extend((6..=18).map(|x| (at(x, z + 4), BlockState::of(blocks::STONE))));
        put(&mut game, &cells);
        game.set_blocks(&[(at(5, z + 5), BlockState::of(blocks::REDSTONE_BLOCK))]);
        game.run_ticks(3);
        assert!(
            !block_at(&game, at(5, z + 4)).bool(Prop::Extended),
            "13 is too many"
        );
        // It doesn't try again by itself; cycling the power does.
        game.set_blocks(&[(at(18, z + 4), BlockState::AIR)]);
        game.set_blocks(&[(at(5, z + 5), BlockState::AIR)]);
        game.run_ticks(2);
        game.set_blocks(&[(at(5, z + 5), BlockState::of(blocks::REDSTONE_BLOCK))]);
        game.run_ticks(3);
        assert!(
            block_at(&game, at(5, z + 4)).bool(Prop::Extended),
            "12 is fine"
        );
    }

    #[test]
    fn a_piston_is_powered_by_a_source_beside_the_cell_above_it() {
        let mut game = flat_game(GameMode::Creative);
        let z = -3;
        put(
            &mut game,
            &[
                (at(5, z), piston(false, Face::East)),
                (at(6, z), BlockState::of(blocks::STONE)),
            ],
        );
        // Diagonally above: it powers the cell above, not the piston.
        game.set_blocks(&[(
            IVec3::new(4, Y + 1, z),
            BlockState::of(blocks::REDSTONE_BLOCK),
        )]);
        game.run_ticks(3);
        assert!(
            block_at(&game, at(5, z)).bool(Prop::Extended),
            "quasi-connectivity"
        );
    }

    #[test]
    fn a_piston_shoves_the_player_out_of_the_way() {
        let mut game = flat_game(GameMode::Creative);
        put(&mut game, &[(at(1, 0), piston(false, Face::West))]);
        game.set_blocks(&[(at(1, 1), BlockState::of(blocks::REDSTONE_BLOCK))]);
        game.run_ticks(3);
        assert_eq!(id_at(&game, at(0, 0)), blocks::PISTON_HEAD);
        let x = game.body.aabb().max.x;
        assert!(
            x <= 0.0 + 1e-3,
            "the player's box was pushed clear of the head: {x}"
        );
    }

    #[test]
    fn pistons_break_torches_in_their_way_and_drop_them() {
        let mut game = flat_game(GameMode::Survival);
        game.entities.clear();
        let z = 3;
        put(
            &mut game,
            &[
                (at(5, z), piston(false, Face::East)),
                (at(6, z), BlockState::of(blocks::TORCH)),
            ],
        );
        game.set_blocks(&[(at(5, z + 1), BlockState::of(blocks::REDSTONE_BLOCK))]);
        game.run_ticks(3);
        assert_eq!(id_at(&game, at(6, z)), blocks::PISTON_HEAD);
        assert_eq!(game.entities.len(), 1, "the torch dropped as an item");
    }

    #[test]
    fn a_broken_head_or_piston_takes_the_other_with_it() {
        let mut game = flat_game(GameMode::Creative);
        let z = 8;
        put(&mut game, &[(at(5, z), piston(false, Face::East))]);
        game.set_blocks(&[(at(5, z + 1), BlockState::of(blocks::REDSTONE_BLOCK))]);
        game.run_ticks(2);
        assert_eq!(id_at(&game, at(6, z)), blocks::PISTON_HEAD);
        game.set_blocks(&[(at(5, z), BlockState::AIR)]);
        game.run_ticks(2);
        assert_eq!(
            id_at(&game, at(6, z)),
            blocks::AIR,
            "no head without its piston"
        );
        // And the other way round.
        put(&mut game, &[(at(5, z), piston(false, Face::East))]);
        game.run_ticks(3);
        assert!(block_at(&game, at(5, z)).bool(Prop::Extended));
        game.set_blocks(&[(at(6, z), BlockState::AIR)]);
        game.run_ticks(3);
        assert_eq!(
            id_at(&game, at(6, z)),
            blocks::PISTON_HEAD,
            "the piston is still powered, so it extends again"
        );
    }
}
