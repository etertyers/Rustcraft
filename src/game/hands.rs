//! Using the selected item on the world: mining (with 1.0.0's breaking
//! times, drops and tool wear in survival, instantly in creative), placing
//! blocks from the stack, and throwing items.

use glam::{DVec3, IVec3};

use super::Game;
use crate::{
    entity::item::ItemEntity,
    inventory::{item::Tool, player::HOTBAR_SLOTS, stack::ItemStack},
    player::{
        interaction::{self, BREAK_REPEAT_TICKS, PLACE_REPEAT_TICKS},
        raycast::Hit,
    },
    world::harvest,
};

/// Water blocks, which slow mining when the player's eyes are in them.
const WATER: [u16; 2] = [8, 9];

impl Game {
    /// The tool in the selected slot, if it's a tool.
    fn selected_tool(&self) -> Option<&Tool> {
        let stack = self.inventory.selected_stack()?;
        self.items.tool(stack.id)
    }

    /// Breaks or places once per tick while a button is held.
    pub(super) fn use_hands(&mut self) {
        self.break_cooldown = self.break_cooldown.saturating_sub(1);
        self.place_cooldown = self.place_cooldown.saturating_sub(1);
        let wants_break = self.breaking || self.break_clicked;
        let wants_place = (self.placing || self.place_clicked) && self.place_cooldown == 0;
        self.break_clicked = false;
        self.place_clicked = false;
        let eye = self.body.position() + DVec3::Y * self.body.eye_height();
        let hit = self.find_target(eye);
        if !wants_break || hit.is_none() {
            // Letting go or looking away starts the block over (1.0.0).
            self.mining = None;
        }
        let Some(hit) = hit else {
            return;
        };
        if wants_break {
            self.mine(hit.block, eye);
        } else if wants_place {
            self.place(&hit);
        }
    }

    /// One tick of mining `cell`; breaks it once the progress reaches 1.
    fn mine(&mut self, cell: IVec3, eye: DVec3) {
        if self.break_cooldown > 0 {
            return;
        }
        let world = self.chunks.world();
        let Some(def) = world
            .block(cell.x, cell.y, cell.z)
            .and_then(|block| self.items.blocks.get(block.id()))
        else {
            return;
        };
        let progress = if self.mode.breaks_instantly() {
            1.0
        } else {
            let eye_cell = eye.floor();
            let in_water = world
                .block(eye_cell.x as i32, eye_cell.y as i32, eye_cell.z as i32)
                .is_some_and(|block| WATER.contains(&block.id()));
            let per_tick = harvest::progress_per_tick(
                def,
                self.selected_tool(),
                self.body.on_ground,
                in_water,
            );
            let so_far = match self.mining {
                Some((mining, progress)) if mining == cell => progress,
                _ => 0.0,
            };
            so_far + per_tick
        };
        if progress < 1.0 {
            self.mining = Some((cell, progress));
            return;
        }
        self.mining = None;
        self.break_cooldown = BREAK_REPEAT_TICKS;
        self.break_block(cell);
    }

    /// Removes the block at `cell` and whatever it held up, dropping items
    /// and wearing the tool as the game mode says.
    fn break_block(&mut self, cell: IVec3) {
        let world = self.chunks.world();
        let edits = interaction::break_block(world, &self.items.blocks, cell);
        if edits.is_empty() {
            return;
        }
        if self.mode.drops_items() {
            let tool = self
                .inventory
                .selected_stack()
                .and_then(|stack| self.items.tool(stack.id));
            let rng = &mut self.rng;
            let mut random = || rng.next_f64() as f32;
            for &(at, _) in &edits {
                let Some(block) = world.block(at.x, at.y, at.z) else {
                    continue;
                };
                let Some(def) = self.items.blocks.get(block.id()) else {
                    continue;
                };
                // The mined block needs the right tool; blocks it held up
                // (torches, flowers) always drop.
                if at == cell && !harvest::can_harvest(def, tool) {
                    continue;
                }
                let kind = tool.map(|tool| tool.kind);
                for stack in harvest::drops(def, block.data(), kind, &mut random) {
                    self.entities
                        .push(ItemEntity::from_block(stack, at, &mut random));
                }
            }
        }
        if self.mode.wears_tools()
            && let Some(wear) = self.selected_tool().map(|tool| tool.wear)
        {
            self.inventory.damage_selected(wear, &self.items);
        }
        self.chunks.set_blocks(&edits);
    }

    /// Places the selected stack's block against the clicked face.
    fn place(&mut self, hit: &Hit) {
        let Some(stack) = self.inventory.selected_stack().copied() else {
            return;
        };
        let Some(block) = self.items.places(&stack) else {
            return;
        };
        let Some(edit) = interaction::place_block(
            self.chunks.world(),
            &self.items.blocks,
            hit,
            block,
            &self.body.aabb(),
        ) else {
            return;
        };
        self.chunks.set_blocks(&[edit]);
        if self.mode.uses_up_items() {
            self.inventory.take(self.inventory.selected, 1);
        }
        self.place_cooldown = PLACE_REPEAT_TICKS;
    }

    /// Middle click (1.0.0's pick block): selects the hotbar slot holding
    /// the targeted block, or with every item to hand, puts a stack of it
    /// in the selected slot.
    pub fn pick_block(&mut self) {
        let Some(hit) = self.target else {
            return;
        };
        let Some(block) = self
            .chunks
            .world()
            .block(hit.block.x, hit.block.y, hit.block.z)
        else {
            return;
        };
        let Some(def) = self.items.blocks.get(block.id()) else {
            return;
        };
        let data = if def.variants.is_empty() {
            0
        } else {
            block.data()
        };
        let stack = ItemStack::with_damage(block.id(), 1, data as u16);
        let in_hotbar = (0..HOTBAR_SLOTS).find(|&i| {
            self.inventory
                .slot(i)
                .is_some_and(|held| held.stacks_with(&stack))
        });
        if let Some(slot) = in_hotbar {
            self.inventory.selected = slot;
        } else if self.mode.has_every_item() {
            let full = ItemStack {
                count: self.items.max_stack(stack.id),
                ..stack
            };
            self.inventory.set(self.inventory.selected, Some(full));
        }
    }

    /// A dev tool for debug builds (F6), until crafting exists in M10: one
    /// pickaxe of each tier, iron shovel, axe and sword, shears and torches.
    #[cfg(debug_assertions)]
    pub fn give_dev_kit(&mut self) {
        const KIT: [(u16, u8); 10] = [
            (270, 1),
            (274, 1),
            (257, 1),
            (285, 1),
            (278, 1),
            (256, 1),
            (258, 1),
            (267, 1),
            (359, 1),
            (50, 64),
        ];
        for (id, count) in KIT {
            if let Some(rest) = self.inventory.add(ItemStack::new(id, count), &self.items) {
                self.throw(rest);
            }
        }
        log::info!("dev kit given");
    }

    /// Throws one of the selected item forward (1.0.0's Q).
    pub fn throw_selected(&mut self) {
        let Some(stack) = self.inventory.take(self.inventory.selected, 1) else {
            return;
        };
        self.throw(stack);
    }

    /// Throws `stack` from the player's eyes.
    pub fn throw(&mut self, stack: ItemStack) {
        let eye = self.body.position() + DVec3::Y * self.body.eye_height();
        let forward = self.camera.forward().as_dvec3();
        let rng = &mut self.rng;
        let mut random = || rng.next_f64() as f32;
        self.entities
            .push(ItemEntity::thrown(stack, eye, forward, &mut random));
    }

    /// Moves dropped items and lets the player pick them up.
    pub(super) fn tick_items(&mut self) {
        let collider = crate::player::collision::WorldCollider {
            world: self.chunks.world(),
            blocks: &self.items.blocks,
        };
        let player = self.body.aabb();
        for item in &mut self.entities {
            item.tick(&collider);
            item.pick_up(&player, &mut self.inventory, &self.items);
        }
        self.entities.retain(|item| !item.expired());
    }
}
