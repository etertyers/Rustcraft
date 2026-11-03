//! Jukeboxes (M47): a disc goes in with a right-click and plays its song
//! for as long as it lasts, a note drifts up every second, the redstone
//! gets 15 while it plays, and a second right-click (or a hopper, or
//! breaking the box) takes the disc out. The server owns all of it and
//! tells the clients in hearing range when a song starts and stops.

use glam::{DVec3, IVec3};

use super::Game;
use crate::{
    entity::{item::ItemEntity, particle::Particle},
    inventory::{discs, stack::ItemStack},
    world::{
        block_entity::{BlockEntity, Jukebox},
        blocks,
        dimension::Dimension,
        props::Prop,
    },
};

/// How far a jukebox's song carries: volume 4.0 reaches 16 × 4 blocks.
pub const HEARING_RANGE: f64 = 64.0;
/// A note drifts up this often (ticks).
const NOTE_EVERY: u64 = 20;

/// A song starting or stopping, for the server to tell the clients.
#[derive(Debug, Clone, PartialEq)]
pub struct RecordEvent {
    pub dimension: Dimension,
    pub at: IVec3,
    /// The disc's name (`cat`) when its song starts, `None` when it stops.
    pub song: Option<String>,
}

impl Game {
    /// The disc `stack` is, if it is one.
    pub(super) fn disc_of(&self, stack: &ItemStack) -> Option<&'static discs::Disc> {
        self.items
            .item(stack.id)
            .and_then(|item| discs::of_item(&item.name))
    }

    /// Right-click on a jukebox: takes the disc out if it holds one, or
    /// puts in the disc in hand. Returns whether it did anything.
    pub(super) fn use_jukebox(&mut self, cell: IVec3) -> bool {
        let Some(state) = self.chunks.world().block(cell.x, cell.y, cell.z) else {
            return false;
        };
        if state.bool(Prop::HasRecord) {
            self.take_out_disc(cell, true);
            return true;
        }
        let Some(held) = self.held().cloned() else {
            return false;
        };
        if self.disc_of(&held).is_none() {
            return false;
        }
        let slot = self.held_slot();
        if self.player.mode.uses_up_items() {
            self.player.inventory.take(slot, 1);
        }
        self.insert_disc(cell, ItemStack { count: 1, ..held });
        true
    }

    /// Puts `disc` into the empty jukebox at `cell` and starts its song.
    /// Returns what couldn't go in.
    pub(crate) fn insert_disc(&mut self, cell: IVec3, disc: ItemStack) -> Option<ItemStack> {
        let Some(song) = self.disc_of(&disc) else {
            return Some(disc);
        };
        let Some(state) = self.chunks.world().block(cell.x, cell.y, cell.z) else {
            return Some(disc);
        };
        if state.id() != blocks::JUKEBOX || state.bool(Prop::HasRecord) {
            return Some(disc);
        }
        let mut jukebox = Jukebox::new();
        jukebox.slots[0] = Some(disc);
        jukebox.ends = self.time + song.ticks();
        self.chunks
            .set_block_entity(cell, Some(BlockEntity::Jukebox(jukebox)));
        self.edit(&[(
            cell,
            state
                .with_bool(Prop::HasRecord, true)
                .with_bool(Prop::Powered, true),
        )]);
        self.jukeboxes.push((self.dimension, cell));
        self.record_events.push(RecordEvent {
            dimension: self.dimension,
            at: cell,
            song: Some(song.name.to_owned()),
        });
        self.announce_song(cell, song.title);
        None
    }

    /// Shows "Now Playing: <title>" to the players who hear the jukebox.
    fn announce_song(&mut self, cell: IVec3, title: &str) {
        let middle = cell.as_dvec3() + DVec3::splat(0.5);
        let line = format!("Now Playing: {title}");
        self.for_each_player(|game| {
            if game.player.body.position().distance(middle) <= HEARING_RANGE {
                let t = &mut game.player.title;
                t.actionbar = line.clone();
                t.actionbar_left = crate::game::board::TitleState::ACTIONBAR_TICKS;
                t.actionbar_rainbow = true;
            }
        });
    }

    /// Takes the disc out of the jukebox at `cell` and stops its song; it
    /// lands on top of the box if `drop`, or is gone (a hopper has it).
    pub(crate) fn take_out_disc(&mut self, cell: IVec3, drop: bool) {
        let Some(state) = self.chunks.world().block(cell.x, cell.y, cell.z) else {
            return;
        };
        let disc = match self.chunks.block_entity_mut(cell) {
            Some(BlockEntity::Jukebox(jukebox)) => {
                jukebox.ends = 0;
                jukebox.slots[0].take()
            }
            _ => None,
        };
        self.stop_song(cell);
        if state.id() == blocks::JUKEBOX {
            self.edit(&[(
                cell,
                state
                    .with_bool(Prop::HasRecord, false)
                    .with_bool(Prop::Powered, false),
            )]);
        }
        if let Some(disc) = disc.filter(|_| drop) {
            let jitter = DVec3::new(
                (self.rng.next_f64() - 0.5) * 0.1,
                0.1,
                (self.rng.next_f64() - 0.5) * 0.1,
            );
            let above = cell.as_dvec3() + DVec3::new(0.5, 1.1, 0.5);
            self.entities
                .push(ItemEntity::new(disc, above, jitter, 0.0));
        }
    }

    /// Ends the song at `cell` for the clients, and forgets it.
    fn stop_song(&mut self, cell: IVec3) {
        let dimension = self.dimension;
        let before = self.jukeboxes.len();
        self.jukeboxes.retain(|&(d, c)| (d, c) != (dimension, cell));
        if before != self.jukeboxes.len() {
            self.record_events.push(RecordEvent {
                dimension,
                at: cell,
                song: None,
            });
        }
    }

    /// One tick of every song playing in this dimension: notes rise, and a
    /// song that has run its length lets go of the redstone (the disc stays
    /// in). A jukebox that was broken or lost its disc stops.
    pub(super) fn tick_jukeboxes(&mut self) {
        let here = self.dimension;
        let playing: Vec<IVec3> = self
            .jukeboxes
            .iter()
            .filter(|(d, _)| *d == here)
            .map(|&(_, cell)| cell)
            .collect();
        for cell in playing {
            let ends = match self.chunks.block_entity(cell) {
                Some(BlockEntity::Jukebox(jukebox)) if jukebox.slots[0].is_some() => jukebox.ends,
                _ => 0,
            };
            let state = self.chunks.world().block(cell.x, cell.y, cell.z);
            let Some(state) = state.filter(|s| s.id() == blocks::JUKEBOX && ends > 0) else {
                // Broken, emptied or unloaded: the song is over.
                if self.chunks.world().block(cell.x, cell.y, cell.z).is_some() {
                    self.stop_song(cell);
                }
                continue;
            };
            if self.time >= ends {
                if let Some(BlockEntity::Jukebox(jukebox)) = self.chunks.block_entity_mut(cell) {
                    jukebox.ends = 0;
                }
                self.stop_song(cell);
                self.edit(&[(cell, state.with_bool(Prop::Powered, false))]);
            } else if (ends - self.time).is_multiple_of(NOTE_EVERY) {
                let at = cell.as_dvec3() + DVec3::new(0.5, 1.2, 0.5);
                // A note's color goes round the rainbow with the second.
                let hue = ((ends - self.time) / NOTE_EVERY % 24) as f32 / 24.0;
                let mut random = || self.rng.next_f64() as f32;
                let note = Particle::colored(at, rainbow(hue), &mut random);
                self.add_particles([note]);
            }
        }
    }

    /// A chunk coming in from disk: a song doesn't carry on from a save, so
    /// a jukebox that was playing is quiet and lets go of the redstone.
    pub(super) fn settle_jukeboxes(&mut self, chunks: &[crate::world::chunk::ChunkPos]) {
        for &pos in chunks {
            let stale: Vec<IVec3> = match self.chunks.world().get(pos) {
                Some(chunk) => chunk
                    .block_entities()
                    .filter(|(_, e)| matches!(e, BlockEntity::Jukebox(j) if j.ends > 0))
                    .map(|([x, y, z], _)| {
                        let (x0, z0) = pos.min_block();
                        IVec3::new(x0 + i32::from(x), i32::from(y), z0 + i32::from(z))
                    })
                    .collect(),
                None => continue,
            };
            for cell in stale {
                if let Some(BlockEntity::Jukebox(jukebox)) = self.chunks.block_entity_mut(cell) {
                    jukebox.ends = 0;
                }
                if let Some(state) = self.chunks.world().block(cell.x, cell.y, cell.z)
                    && state.bool(Prop::Powered)
                {
                    self.edit(&[(cell, state.with_bool(Prop::Powered, false))]);
                }
            }
        }
    }
}

/// An 0xRRGGBB color for a hue (0–1) at full saturation and value.
fn rainbow(hue: f32) -> u32 {
    hsv(hue, 1.0, 1.0)
}

/// Vanilla's `Mth.hsvToRgb`.
pub fn hsv(hue: f32, saturation: f32, value: f32) -> u32 {
    let h = (hue.rem_euclid(1.0) * 6.0) as u32;
    let f = hue.rem_euclid(1.0) * 6.0 - h as f32;
    let p = value * (1.0 - saturation);
    let q = value * (1.0 - f * saturation);
    let t = value * (1.0 - (1.0 - f) * saturation);
    let (r, g, b) = match h {
        0 => (value, t, p),
        1 => (q, value, p),
        2 => (p, value, t),
        3 => (p, q, value),
        4 => (t, p, value),
        _ => (value, p, q),
    };
    let byte = |c: f32| (c * 255.0 + 0.5).clamp(0.0, 255.0) as u32;
    byte(r) << 16 | byte(g) << 8 | byte(b)
}

/// A state with the given jukebox flags, for tests.
#[cfg(test)]
pub fn jukebox_state(has_record: bool, powered: bool) -> crate::world::block::BlockState {
    crate::world::block::BlockState::of(blocks::JUKEBOX)
        .with_bool(Prop::HasRecord, has_record)
        .with_bool(Prop::Powered, powered)
}
