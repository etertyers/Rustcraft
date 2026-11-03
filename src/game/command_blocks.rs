//! Command blocks (M45): impulse (orange), chain (green) and repeating
//! (purple), conditional or not, needing redstone or always active. Only
//! operators in creative place, break or edit them. A chain runs in the
//! tick that starts it, block after block along their facings.

use glam::IVec3;

use super::{
    Game,
    commands::{Caller, Reply},
};
use crate::world::{
    Face,
    block::BlockState,
    block_entity::{BlockEntity, CommandBlock},
    blocks,
    props::Prop,
};

/// The three kinds, in the editor's order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Mode {
    /// "Impulse" in the editor: once when powered.
    Impulse,
    /// "Chain": when the block behind it ran.
    Chain,
    /// "Repeat": every tick while powered or active.
    Repeat,
}

impl Mode {
    pub fn of(state: BlockState) -> Option<Mode> {
        match state.kind() {
            blocks::COMMAND_BLOCK => Some(Mode::Impulse),
            blocks::CHAIN_COMMAND_BLOCK => Some(Mode::Chain),
            blocks::REPEATING_COMMAND_BLOCK => Some(Mode::Repeat),
            _ => None,
        }
    }

    pub fn block(self) -> crate::world::block::BlockId {
        match self {
            Mode::Impulse => blocks::COMMAND_BLOCK,
            Mode::Chain => blocks::CHAIN_COMMAND_BLOCK,
            Mode::Repeat => blocks::REPEATING_COMMAND_BLOCK,
        }
    }

    pub fn next(self) -> Mode {
        match self {
            Mode::Impulse => Mode::Chain,
            Mode::Chain => Mode::Repeat,
            Mode::Repeat => Mode::Impulse,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Mode::Impulse => "Impulse",
            Mode::Chain => "Chain",
            Mode::Repeat => "Repeat",
        }
    }
}

/// What the editor shows and sends back.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Edit {
    pub at: [i32; 3],
    pub command: String,
    pub mode: Mode,
    pub conditional: bool,
    pub auto: bool,
    pub track_output: bool,
    pub last_output: String,
}

fn step(face: Face) -> IVec3 {
    IVec3::from_array(face.normal())
}

impl Game {
    /// Whether `self.player` may place, break or edit command blocks:
    /// an operator in creative.
    pub fn may_edit_command_blocks(&self) -> bool {
        let p = &self.player;
        p.mode == crate::player::mode::GameMode::Creative
            && (self.admin.ops.contains(&p.name)
                || (self.turn == 0 && self.cheats)
                || (self.turn != 0 && self.guests_cheat))
    }

    /// A command block's entity, made if the block has none yet.
    fn command_entity(&mut self, at: IVec3) -> Option<&mut CommandBlock> {
        let state = self.chunks.world().block(at.x, at.y, at.z)?;
        Mode::of(state)?;
        if !matches!(
            self.chunks.block_entity(at),
            Some(BlockEntity::CommandBlock(_))
        ) {
            self.chunks
                .set_block_entity(at, Some(BlockEntity::CommandBlock(CommandBlock::default())));
        }
        match self.chunks.block_entity_mut(at)? {
            BlockEntity::CommandBlock(c) => Some(c),
            _ => None,
        }
    }

    /// Redstone reached (or left) a command block: an impulse block runs
    /// on the rising edge.
    pub(super) fn command_block_power(&mut self, at: IVec3, powered: bool) {
        let Some(state) = self.chunks.world().block(at.x, at.y, at.z) else {
            return;
        };
        let Some(mode) = Mode::of(state) else {
            return;
        };
        let Some(c) = self.command_entity(at) else {
            return;
        };
        if powered && !c.powered && mode == Mode::Impulse && !c.auto {
            c.pending = true;
        }
        c.powered = powered;
    }

    /// The editor for the block at `cell`, for `self.player`.
    pub(super) fn open_command_block(&mut self, cell: IVec3) {
        if !self.may_edit_command_blocks() {
            self.player
                .show("§cOnly operators in creative mode can edit command blocks");
            return;
        }
        let Some(state) = self.chunks.world().block(cell.x, cell.y, cell.z) else {
            return;
        };
        let Some(mode) = Mode::of(state) else {
            return;
        };
        let conditional = state.bool(Prop::Conditional);
        let Some(c) = self.command_entity(cell).cloned() else {
            return;
        };
        self.player.command_block_edit = Some(Edit {
            at: cell.to_array(),
            command: c.command,
            mode,
            conditional,
            auto: c.auto,
            track_output: c.track_output,
            last_output: c.last_output,
        });
    }

    /// Opens the editor as a right-click would (for tests).
    #[cfg(test)]
    pub(crate) fn open_command_block_for_test(&mut self, at: IVec3) {
        self.open_command_block(at);
    }

    /// The editor's Done (`Some`) or Cancel (`None`), for `self.player`.
    pub fn finish_command_block_edit(&mut self, edit: Option<Edit>) {
        self.player.command_block_edit = None;
        let Some(edit) = edit else {
            return;
        };
        if !self.may_edit_command_blocks() {
            return;
        }
        // A command block cart is addressed by its index.
        if edit.at[1] == super::minecarts::CART_EDIT {
            if let Some(cart) = self.carts.get_mut(edit.at[0] as usize) {
                cart.command = edit.command.chars().take(32_500).collect();
            }
            return;
        }
        let at = IVec3::from_array(edit.at);
        let Some(state) = self.chunks.world().block(at.x, at.y, at.z) else {
            return;
        };
        if Mode::of(state).is_none() {
            return;
        }
        // The mode is the block; facing and conditional stay in its state.
        let facing = state.face(Prop::FacingAll);
        let new_state = BlockState::of(edit.mode.block())
            .with_face(Prop::FacingAll, facing)
            .with_bool(Prop::Conditional, edit.conditional);
        let was_auto = {
            let Some(c) = self.command_entity(at) else {
                return;
            };
            let was = c.auto;
            c.command = edit.command.chars().take(32_500).collect();
            c.auto = edit.auto;
            c.track_output = edit.track_output;
            // An impulse block set to always active runs once.
            c.pending = c.pending || (edit.mode == Mode::Impulse && edit.auto && !was);
            was
        };
        let _ = was_auto;
        if new_state != state {
            let entity = self.chunks.block_entity(at).cloned();
            self.edit(&[(at, new_state)]);
            if let Some(entity) = entity {
                self.chunks.set_block_entity(at, Some(entity));
            }
        }
    }

    /// One tick of every loaded command block that starts a run: impulse
    /// ones that were triggered, repeating ones that are powered or active.
    pub(super) fn tick_command_blocks(&mut self) {
        if !self.rules.command_blocks_enabled {
            return;
        }
        let mut starts: Vec<IVec3> = Vec::new();
        {
            let world = self.chunks.world();
            for pos in world.positions() {
                let Some(chunk) = world.get(pos) else {
                    continue;
                };
                let (bx, bz) = pos.min_block();
                for (local, entity) in chunk.block_entities() {
                    if let BlockEntity::CommandBlock(c) = entity {
                        let at = IVec3::new(
                            bx + i32::from(local[0]),
                            i32::from(local[1]),
                            bz + i32::from(local[2]),
                        );
                        let Some(state) = world.block(at.x, at.y, at.z) else {
                            continue;
                        };
                        let runs = match Mode::of(state) {
                            Some(Mode::Impulse) => c.pending,
                            Some(Mode::Repeat) => c.powered || c.auto,
                            _ => false,
                        };
                        if runs {
                            starts.push(at);
                        }
                    }
                }
            }
        }
        starts.sort_by_key(|a| (a.y, a.z, a.x));
        for at in starts {
            let Some(state) = self.chunks.world().block(at.x, at.y, at.z) else {
                continue;
            };
            if let Some(c) = self.command_entity(at) {
                c.pending = false;
            }
            // A conditional block runs only if the one behind it did.
            let ok = self.command_block_may_run(at, state);
            let success = if ok { self.run_command_block(at) } else { 0 };
            self.run_chain(at, state, success);
        }
    }

    /// Whether a conditional block's predecessor succeeded (any other
    /// block may run).
    fn command_block_may_run(&mut self, at: IVec3, state: BlockState) -> bool {
        if !state.bool(Prop::Conditional) {
            return true;
        }
        let behind = at - step(state.face(Prop::FacingAll));
        match self.chunks.block_entity(behind) {
            Some(BlockEntity::CommandBlock(c)) => c.success > 0,
            _ => false,
        }
    }

    /// Runs the chain blocks in front of `from`, in order, in this tick.
    fn run_chain(&mut self, from: IVec3, state: BlockState, mut previous: i32) {
        let (mut at, mut facing) = (from, state.face(Prop::FacingAll));
        let limit = self.rules.max_command_chain_length as usize;
        for _ in 0..limit.min(65_536) {
            let next = at + step(facing);
            let Some(s) = self.chunks.world().block(next.x, next.y, next.z) else {
                return;
            };
            if Mode::of(s) != Some(Mode::Chain) {
                return;
            }
            let Some(c) = self.command_entity(next).cloned() else {
                return;
            };
            // A chain block that needs redstone and has none ends the chain.
            if !c.auto && !c.powered {
                return;
            }
            let conditional_ok = !s.bool(Prop::Conditional) || previous > 0;
            previous = if conditional_ok {
                self.run_command_block(next)
            } else {
                if let Some(e) = self.command_entity(next) {
                    e.success = 0;
                }
                0
            };
            at = next;
            facing = s.face(Prop::FacingAll);
        }
    }

    /// Runs one block's command now. Returns what it returned (0 if it
    /// failed or has none).
    fn run_command_block(&mut self, at: IVec3) -> i32 {
        let Some(c) = self.command_entity(at).cloned() else {
            return 0;
        };
        let command = c.command.trim().trim_start_matches('/').to_owned();
        if command.is_empty() {
            return 0;
        }
        let caller = Caller::for_block(self, at);
        let (replies, result) = self.run_command_as_server(&caller, &command);
        let success = result.unwrap_or(0);
        let mut first = None;
        let mut spoken = Vec::new();
        for reply in replies {
            match reply {
                Reply::Sender(text) => {
                    first.get_or_insert(crate::ui::chat::strip_codes(&text));
                }
                other => spoken.push(other),
            }
        }
        self.replies_out.extend(spoken);
        let output = first.clone().unwrap_or_default();
        if let Some(e) = self.command_entity(at) {
            e.success = success;
            if first.is_some() || result.is_none() {
                e.last_output.clone_from(&output);
            }
        }
        if self.rules.command_block_output
            && c.track_output
            && let Some(text) = first
        {
            let ops: Vec<usize> = (0..self.player_count())
                .filter(|&i| {
                    self.admin.ops.contains(&self.player_at(i).name)
                        || (i == 0 && self.cheats)
                        || (i != 0 && self.guests_cheat)
                })
                .collect();
            for i in ops {
                self.chat_to.push((i, format!("§7§o[@: {text}]")));
            }
        }
        success
    }
}
