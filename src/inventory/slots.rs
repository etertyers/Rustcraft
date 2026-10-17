//! 1.0.0's rules for clicking a slot with a stack held on the cursor
//! (`Container.slotClick`) and for moving a stack into a run of slots
//! (`Container.mergeItemStack`), shared by every screen.

use super::stack::{ItemStack, Items};

/// A slot on an open screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SlotRef {
    /// One of the player's 36 slots, hotbar first.
    Inventory(usize),
    /// A crafting grid cell, row by row.
    Grid(usize),
    /// The crafting result.
    Result,
    /// A slot of the open chest or furnace.
    Block(usize),
}

/// A mouse click on a slot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Click {
    Left,
    Right,
    /// Shift + left click: move the stack to the other part of the screen.
    Shift,
}

/// Left or right click on an ordinary slot: pick up all or half, put down
/// all or one, add onto a matching stack, or swap.
pub fn click(
    slot: &mut Option<ItemStack>,
    click: Click,
    cursor: &mut Option<ItemStack>,
    items: &Items,
) {
    match (slot.as_mut(), cursor.as_mut()) {
        (None, None) => {}
        (Some(_), None) => pick_up(slot, click, cursor),
        // Put down everything that fits, or one.
        (None, Some(held)) => {
            let put = match click {
                Click::Right => 1,
                _ => held.count.min(items.max_stack(held.id)),
            };
            *slot = Some(ItemStack {
                count: put,
                ..*held
            });
            held.count -= put;
            if held.count == 0 {
                *cursor = None;
            }
        }
        // Add onto a matching stack.
        (Some(stack), Some(held)) if stack.stacks_with(held) => {
            let room = items.max_stack(stack.id).saturating_sub(stack.count);
            let put = match click {
                Click::Right => 1.min(room),
                _ => held.count.min(room),
            };
            stack.count += put;
            held.count -= put;
            if held.count == 0 {
                *cursor = None;
            }
        }
        // Different items swap.
        (Some(_), Some(_)) => std::mem::swap(slot, cursor),
    }
}

/// A click on a slot items can only be taken from (a furnace's output):
/// pick up all or half, or with a matching stack held, take the whole slot
/// if it fits on the cursor.
pub fn click_take_only(
    slot: &mut Option<ItemStack>,
    click: Click,
    cursor: &mut Option<ItemStack>,
    items: &Items,
) {
    let Some(stack) = *slot else {
        return;
    };
    match cursor {
        None => pick_up(slot, click, cursor),
        Some(held) if held.stacks_with(&stack) => {
            if held.count + stack.count <= items.max_stack(held.id) {
                held.count += stack.count;
                *slot = None;
            }
        }
        Some(_) => {}
    }
}

/// Puts all of `stack` on the cursor if it fits there (a crafting result).
pub fn take_onto_cursor(stack: ItemStack, cursor: &mut Option<ItemStack>, items: &Items) -> bool {
    match cursor {
        None => {
            *cursor = Some(stack);
            true
        }
        Some(held)
            if held.stacks_with(&stack) && held.count + stack.count <= items.max_stack(held.id) =>
        {
            held.count += stack.count;
            true
        }
        Some(_) => false,
    }
}

/// Picks up a slot's stack, or half of it rounded up for a right click.
fn pick_up(slot: &mut Option<ItemStack>, click: Click, cursor: &mut Option<ItemStack>) {
    let Some(stack) = slot.as_mut() else {
        return;
    };
    let taken = match click {
        Click::Right => stack.count.div_ceil(2),
        _ => stack.count,
    };
    *cursor = Some(ItemStack {
        count: taken,
        ..*stack
    });
    stack.count -= taken;
    if stack.count == 0 {
        *slot = None;
    }
}

/// Moves `stack` into `slots` at the indices in `order`: onto matching
/// stacks first, then into empty slots, each in that order. Returns what
/// didn't fit.
pub fn merge(
    slots: &mut [Option<ItemStack>],
    order: impl Iterator<Item = usize> + Clone,
    mut stack: ItemStack,
    items: &Items,
) -> Option<ItemStack> {
    let max = items.max_stack(stack.id);
    for i in order.clone() {
        if let Some(slot) = slots[i].as_mut()
            && slot.stacks_with(&stack)
            && slot.count < max
        {
            let moved = stack.count.min(max - slot.count);
            slot.count += moved;
            stack.count -= moved;
            if stack.count == 0 {
                return None;
            }
        }
    }
    for i in order {
        if slots[i].is_none() {
            let moved = stack.count.min(max);
            slots[i] = Some(ItemStack {
                count: moved,
                ..stack
            });
            stack.count -= moved;
            if stack.count == 0 {
                return None;
            }
        }
    }
    Some(stack)
}
