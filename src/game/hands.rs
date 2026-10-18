//! Using the selected item on the world: mining (with 1.0.0's breaking
//! times, drops and tool wear in survival, instantly in creative), placing
//! blocks from the stack, and throwing items.

use glam::{DVec3, IVec3};

use super::Game;
use crate::{
    entity::{
        arrow::{self, Arrow, ArrowEvent},
        item::ItemEntity,
    },
    inventory::{item::Tool, player::HOTBAR_SLOTS, stack::ItemStack},
    player::{
        interaction::{self, BREAK_REPEAT_TICKS, PLACE_REPEAT_TICKS},
        raycast::Hit,
        stats::DamageSource,
    },
    world::{
        block::{Activate, BlockState, Fluid, Placement},
        harvest,
    },
};

/// The arrow item, given back when a stuck arrow is picked up.
const ARROW: u16 = 262;

/// Exhaustion per block broken (1.0.0's `harvestBlock`).
const BREAK_EXHAUSTION: f32 = 0.025;
/// Ticks of holding right-click to eat (1.0.0's `getMaxItemUseDuration`).
const EAT_TICKS: u32 = 32;
/// Mobs can be hit this far away (1.0.0 limits entity reach to 3).
const ATTACK_REACH: f64 = 3.0;
/// Mob hitboxes grow by this for aiming (1.0.0's `getCollisionBorderSize`).
const TARGET_BORDER: f64 = 0.1;
/// Exhaustion per attack (1.0.0).
const ATTACK_EXHAUSTION: f32 = 0.3;
/// Extra push and lift of a sprinting hit (1.0.0's knockback bonus).
const SPRINT_KNOCKBACK: f64 = 0.5;
const SPRINT_KNOCKBACK_LIFT: f64 = 0.1;

impl Game {
    /// The block being mined and how far along it is (0–1), for the crack.
    pub fn mining_progress(&self) -> Option<(IVec3, f32)> {
        self.mining
    }

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
        let using = self.placing || self.place_clicked;
        if self.keep_drawing(using && !wants_break) {
            self.break_clicked = false;
            self.place_clicked = false;
            self.mining = None;
            return;
        }
        if using && !wants_break && self.keep_eating() {
            self.break_clicked = false;
            self.place_clicked = false;
            self.mining = None;
            return;
        }
        self.eating = None;
        let wants_place = using && self.place_cooldown == 0;
        let place_pressed = self.place_clicked;
        let clicked = self.break_clicked;
        self.break_clicked = false;
        self.place_clicked = false;
        let eye = self.body.position() + DVec3::Y * self.body.eye_height();
        let hit = self.find_target(eye);
        // A mob in front of the targeted block is hit instead, once per click.
        if let Some(mob) = self.target_mob(eye, hit.as_ref())
            && (wants_break || self.mining.is_none())
        {
            self.mining = None;
            if clicked {
                self.attack(mob);
            }
            return;
        }
        if !wants_break || hit.is_none() {
            // Letting go or looking away starts the block over (1.0.0).
            self.mining = None;
        }
        let Some(hit) = hit else {
            return;
        };
        if wants_break {
            self.mine(hit.block);
        } else if using && !self.activate(hit.block, place_pressed) && wants_place {
            self.place(&hit);
        }
    }

    /// One tick of mining `cell`; breaks it once the progress reaches 1.
    fn mine(&mut self, cell: IVec3) {
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
            let in_water = self.eye_in() == Some(Fluid::Water);
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
    pub(super) fn break_block(&mut self, cell: IVec3) {
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
        // A chest or furnace spills what it held, in every game mode (1.0.0).
        for &(at, _) in &edits {
            let Some(entity) = self.chunks.set_block_entity(at, None) else {
                continue;
            };
            let rng = &mut self.rng;
            let mut random = || rng.next_f64() as f32;
            for stack in entity.contents() {
                self.entities
                    .push(ItemEntity::from_block(stack, at, &mut random));
            }
        }
        if self.mode.is_vulnerable() {
            self.stats.add_exhaustion(BREAK_EXHAUSTION);
        }
        if self.mode.wears_tools()
            && let Some(wear) = self.selected_tool().map(|tool| tool.wear)
        {
            self.inventory.damage_selected(wear, &self.items);
        }
        self.chunks.set_blocks(&edits);
    }

    /// Draws a bow while `using` (right-click held) with ammunition, and
    /// shoots on release. Returns true while drawing.
    fn keep_drawing(&mut self, using: bool) -> bool {
        let ammo = self
            .inventory
            .selected_stack()
            .and_then(|stack| self.items.item(stack.id))
            .and_then(|item| item.shoots);
        let Some(ammo) = ammo else {
            self.drawing = None;
            return false;
        };
        let has_ammo = !self.mode.uses_up_items() || self.inventory.find(ammo).is_some();
        if using && has_ammo {
            self.drawing = Some(self.drawing.map_or(1, |ticks| ticks + 1));
            return true;
        }
        if let Some(ticks) = self.drawing.take()
            && let Some(power) = arrow::bow_power(ticks)
        {
            let eye = self.body.position() + DVec3::Y * self.body.eye_height();
            let forward = self.camera.forward().as_dvec3();
            let pickup = self.mode.uses_up_items();
            self.arrows.push(Arrow::shoot(eye, forward, power, pickup));
            if pickup && let Some(slot) = self.inventory.find(ammo) {
                self.inventory.take(slot, 1);
            }
            if self.mode.wears_tools() {
                self.inventory.damage_selected(1, &self.items);
            }
        }
        false
    }

    /// Moves arrows: hits on mobs, sticking in blocks, picking them up.
    pub(super) fn tick_arrows(&mut self) {
        let mut arrows = std::mem::take(&mut self.arrows);
        let player = self.body.aabb();
        for arrow in &mut arrows {
            let world = self.chunks.world();
            let blocks = &self.items.blocks;
            let solid = |cell: glam::IVec3| {
                world
                    .block(cell.x, cell.y, cell.z)
                    .and_then(|block| blocks.get(block.id()))
                    .is_none_or(|def| def.solid)
            };
            let block_hit = |from: DVec3, dir: DVec3, reach: f64| {
                let hit = crate::player::raycast::raycast(from, dir, reach, solid)?;
                crate::player::collision::Aabb::block(hit.block.x, hit.block.y, hit.block.z)
                    .ray_hit(from, dir, reach)
            };
            let in_water = self.fluid_at(arrow.position) == Some(Fluid::Water);
            let rng = &mut self.rng;
            let mut random = || rng.next_f64() as f32;
            let from = arrow.position;
            if let Some(ArrowEvent::HitMob { index, damage }) =
                arrow.tick(block_hit, in_water, &self.mobs, &mut random)
            {
                self.mobs[index].hurt(damage, Some(from), DamageSource::Arrow);
            }
        }
        arrows.retain(|arrow| {
            if arrow.can_pick_up(&player)
                && self
                    .inventory
                    .add(ItemStack::with_damage(ARROW, 1, 0), &self.items)
                    .is_none()
            {
                return false;
            }
            !arrow.expired()
        });
        self.arrows = arrows;
    }

    /// A tick of eating the selected food while right-click is held.
    /// Returns false if the selected item can't be eaten now.
    fn keep_eating(&mut self) -> bool {
        let slot = self.inventory.selected;
        let Some(stack) = self.inventory.selected_stack().copied() else {
            return false;
        };
        let Some(food) = self.items.item(stack.id).and_then(|item| item.food) else {
            return false;
        };
        let hungry = food.always_edible || self.stats.can_eat();
        if !self.mode.is_vulnerable() || !hungry {
            return false;
        }
        let ticks = match self.eating {
            Some((eating_slot, ticks)) if eating_slot == slot => ticks + 1,
            _ => 1,
        };
        if ticks < EAT_TICKS {
            self.eating = Some((slot, ticks));
            return true;
        }
        self.eating = None;
        self.inventory.take(slot, 1);
        self.stats.eat(food.hunger, food.saturation);
        if let Some(leftover) = self.items.item(stack.id).and_then(|item| item.leftover) {
            let bowl = ItemStack::with_damage(leftover, 1, 0);
            if self.inventory.slot(slot).is_none() {
                self.inventory.set(slot, Some(bowl));
            } else if let Some(rest) = self.inventory.add(bowl, &self.items) {
                self.throw(rest);
            }
        }
        true
    }

    /// The living mob under the crosshair within reach, if it's nearer
    /// than the targeted block.
    fn target_mob(&self, eye: DVec3, block: Option<&Hit>) -> Option<usize> {
        let dir = self.camera.forward().as_dvec3();
        let block_distance = block.map_or(f64::MAX, |hit| {
            (hit.block.as_dvec3() + DVec3::splat(0.5)).distance(eye) - 0.5
        });
        self.mobs
            .iter()
            .enumerate()
            .filter(|(_, mob)| !mob.is_dead())
            .filter_map(|(i, mob)| {
                let distance =
                    mob.body
                        .aabb()
                        .grow(TARGET_BORDER)
                        .ray_hit(eye, dir, ATTACK_REACH)?;
                Some((i, distance))
            })
            .filter(|&(_, distance)| distance < block_distance)
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(i, _)| i)
    }

    /// Hits mob `index` with the selected item (1.0.0's
    /// `attackTargetEntityWithCurrentItem`): its attack damage (or 1 bare
    /// handed), a critical bonus while falling, knockback (more when
    /// sprinting), weapon wear and exhaustion.
    fn attack(&mut self, index: usize) {
        let tool = self.selected_tool().cloned();
        let mut damage = tool.as_ref().map_or(1, |tool| tool.attack);
        let falling =
            self.stats.fall_distance > 0.0 && !self.body.on_ground && self.body.liquid.is_none();
        if falling {
            damage += (self.rng.next_f64() * (damage / 2 + 2) as f64) as i32;
        }
        let from = self.body.position();
        let sprinting = self.body.sprinting;
        let yaw = self.camera.yaw as f64;
        let mob = &mut self.mobs[index];
        if mob.hurt(damage, Some(from), DamageSource::Mob) && sprinting {
            mob.body.velocity += DVec3::new(
                yaw.sin() * SPRINT_KNOCKBACK,
                SPRINT_KNOCKBACK_LIFT,
                -yaw.cos() * SPRINT_KNOCKBACK,
            );
            self.body.velocity.x *= 0.6;
            self.body.velocity.z *= 0.6;
            self.body.sprinting = false;
        }
        if self.mode.is_vulnerable() {
            self.stats.add_exhaustion(ATTACK_EXHAUSTION);
        }
        if self.mode.wears_tools()
            && let Some(tool) = tool
        {
            self.inventory.damage_selected(tool.hit_wear, &self.items);
        }
    }

    /// Right-clicks the block at `cell` (1.0.0's `blockActivated`): opens a
    /// crafting table and so on. Sneaking with something in hand places
    /// against it instead. Only a fresh press (`pressed`) uses the block, as
    /// in modern Minecraft: holding the button doesn't open and shut a door
    /// over and over. Returns whether the block is usable, so nothing gets
    /// placed against it.
    fn activate(&mut self, cell: IVec3, pressed: bool) -> bool {
        if self.keys.sneak && self.inventory.selected_stack().is_some() {
            return false;
        }
        let Some(action) = self
            .chunks
            .world()
            .block(cell.x, cell.y, cell.z)
            .and_then(|block| self.items.blocks.get(block.id()))
            .and_then(|def| def.activate)
        else {
            return false;
        };
        if !pressed {
            return true;
        }
        match action {
            Activate::Crafting => self.open_crafting_table(),
            Activate::Chest => self.open_chest(cell),
            Activate::Furnace => self.open_furnace(cell),
            Activate::Bed => self.use_bed(cell),
            Activate::Door => self.toggle_door(cell),
        }
        true
    }

    /// Opens or closes the door at `cell`, both halves (1.0.0's
    /// `BlockDoor.blockActivated`).
    fn toggle_door(&mut self, cell: IVec3) {
        let world = self.chunks.world();
        let Some(block) = world.block(cell.x, cell.y, cell.z) else {
            return;
        };
        let lower = if block.data() & 8 == 0 {
            cell
        } else {
            cell - IVec3::Y
        };
        let edits: Vec<_> = [lower, lower + IVec3::Y]
            .into_iter()
            .filter_map(|at| {
                let half = world.block(at.x, at.y, at.z)?;
                (half.id() == block.id()).then(|| (at, BlockState::new(half.id(), half.data() ^ 4)))
            })
            .collect();
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
        let world = self.chunks.world();
        let player = self.body.aabb();
        let Some((at, placed)) =
            interaction::place_block(world, &self.items.blocks, hit, block, &player)
        else {
            return;
        };
        let placement = self.items.blocks.get(placed.id()).map(|def| def.placement);
        let yaw = self.camera.yaw;
        let edits = match placement {
            Some(Placement::Facing) => {
                vec![(
                    at,
                    BlockState::new(placed.id(), interaction::facing_data(yaw)),
                )]
            }
            Some(Placement::Bed) => {
                let blocks = &self.items.blocks;
                match interaction::place_bed(world, blocks, hit, (at, placed), yaw, &player) {
                    Some(halves) => halves.to_vec(),
                    None => return,
                }
            }
            Some(Placement::Slab { double }) => {
                vec![interaction::place_slab(world, (at, placed), double)]
            }
            Some(Placement::Door) => {
                let blocks = &self.items.blocks;
                match interaction::place_door(world, blocks, hit, (at, placed), yaw, &player) {
                    Some(halves) => halves.to_vec(),
                    None => return,
                }
            }
            _ => vec![(at, placed)],
        };
        self.chunks.set_blocks(&edits);
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

    /// A dev tool for debug builds (F6), a shortcut past crafting for
    /// testing: one pickaxe of each tier, iron shovel, axe and sword,
    /// shears and torches.
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
        let alive = !self.stats.is_dead();
        for item in &mut self.entities {
            item.tick(&collider);
            if alive {
                item.pick_up(&player, &mut self.inventory, &self.items);
            }
        }
        self.entities.retain(|item| !item.expired());
    }
}
