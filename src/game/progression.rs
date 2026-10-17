//! M10's "done when": the wood → stone → iron → diamond progression in a
//! fresh survival world, played through the same calls the input and
//! screens make: mining by holding the button, clicking crafting grids,
//! placing and right-clicking blocks, and a furnace burning on its own.

use glam::{DVec3, IVec3};

use super::{Game, tests::flat_game};
use crate::{
    inventory::{
        furnace,
        slots::{Click, SlotRef},
    },
    player::{mode::GameMode, physics::Body},
    world::block::BlockState,
};

const LOG: u16 = 17;
const PLANKS: u16 = 5;
const STICK: u16 = 280;
const TABLE: u16 = 58;
const COBBLESTONE: u16 = 4;
const FURNACE: u16 = 61;
const COAL: u16 = 263;
const IRON_ORE: u16 = 15;
const IRON: u16 = 265;
const DIAMOND: u16 = 264;
const WOODEN_PICKAXE: u16 = 270;
const STONE_PICKAXE: u16 = 274;
const IRON_PICKAXE: u16 = 257;

/// The block under the player's feet, which they mine looking down.
const UNDERFOOT: IVec3 = IVec3::new(0, 63, 0);

/// Puts the player on the block at the origin, still.
fn stand_at_origin(game: &mut Game) {
    let feet = DVec3::new(0.5, 64.0, 0.5);
    game.body = Body::new(feet);
    game.prev_feet = feet;
}

fn count(game: &Game, id: u16) -> u32 {
    (0..36)
        .filter_map(|i| game.inventory.slot(i))
        .filter(|stack| stack.id == id)
        .map(|stack| stack.count as u32)
        .sum()
}

/// Selects a hotbar slot holding `id`, moving it into the hotbar first.
fn hold(game: &mut Game, id: u16) {
    let slot = game.inventory.find(id).expect("the item to hold");
    if slot >= 9 {
        let stack = game.inventory.slot(slot).copied();
        let swapped = game.inventory.slot(0).copied();
        game.inventory.set(0, stack);
        game.inventory.set(slot, swapped);
        game.inventory.selected = 0;
    } else {
        game.inventory.selected = slot;
    }
}

/// Stands on `block` (with `tool` in hand, or nothing), mines it by
/// holding the button and picks up what drops.
fn mine(game: &mut Game, block: u16, tool: Option<u16>) {
    game.chunks
        .set_blocks(&[(UNDERFOOT, BlockState::new(block, 0))]);
    stand_at_origin(game);
    game.camera.pitch = -std::f32::consts::FRAC_PI_2;
    match tool {
        Some(tool) => hold(game, tool),
        // An empty hand: a free hotbar slot.
        None => {
            game.inventory.selected = (0..9)
                .find(|&i| game.inventory.slot(i).is_none())
                .expect("a free hotbar slot")
        }
    }
    game.advance(0.0);
    game.breaking = true;
    for _ in 0..2000 {
        game.tick();
        if game.chunks.world().block(0, 63, 0) == Some(BlockState::AIR) {
            break;
        }
    }
    assert_eq!(
        game.chunks.world().block(0, 63, 0),
        Some(BlockState::AIR),
        "mined {block}"
    );
    game.breaking = false;
    for _ in 0..40 {
        game.tick();
    }
}

/// Puts one `id` from the inventory into each grid cell in `cells`, then
/// shift-clicks the result.
fn craft(game: &mut Game, cells: &[(usize, u16)]) {
    for &(cell, id) in cells {
        let from = game.inventory.find(id).expect("an ingredient");
        game.click_slot(SlotRef::Inventory(from), Click::Left);
        game.click_slot(SlotRef::Grid(cell), Click::Right);
        if game.cursor.is_some() {
            game.click_slot(SlotRef::Inventory(from), Click::Left);
        }
        assert_eq!(game.cursor, None);
    }
    assert!(game.crafting_result().is_some(), "a recipe for {cells:?}");
    game.click_slot(SlotRef::Result, Click::Shift);
}

/// Places the held block on the ground two blocks ahead, looking along
/// `yaw`, and returns where it went.
fn place_ahead(game: &mut Game, id: u16, yaw: f32) -> IVec3 {
    game.chunks
        .set_blocks(&[(UNDERFOOT, BlockState::new(2, 0))]);
    stand_at_origin(game);
    hold(game, id);
    game.camera.yaw = yaw;
    game.camera.pitch = -0.7;
    game.advance(0.0);
    let at = game.target.expect("ground ahead").adjacent().unwrap();
    game.place_clicked = true;
    game.tick();
    assert_eq!(
        game.chunks.world().block(at.x, at.y, at.z).map(|b| b.id()),
        Some(id)
    );
    at
}

/// Right-clicks the block at `at` from where the player stands, once the
/// click cooldown from placing has passed.
fn use_block(game: &mut Game, at: IVec3) {
    for _ in 0..crate::player::interaction::PLACE_REPEAT_TICKS {
        game.tick();
    }
    let eye = game.body.position() + DVec3::Y * game.body.eye_height();
    let to = at.as_dvec3() + DVec3::splat(0.5) - eye;
    game.camera.yaw = to.x.atan2(-to.z) as f32;
    game.camera.pitch = (to.y / to.x.hypot(to.z)).atan() as f32;
    game.advance(0.0);
    assert_eq!(game.target.map(|hit| hit.block), Some(at));
    game.place_clicked = true;
    game.tick();
}

#[test]
fn wood_to_stone_to_iron_to_diamond() {
    let mut game = flat_game(GameMode::Survival);

    // Wood: three logs by hand make twelve planks.
    for _ in 0..3 {
        mine(&mut game, LOG, None);
    }
    assert_eq!(count(&game, LOG), 3);
    game.open_inventory();
    for _ in 0..3 {
        craft(&mut game, &[(0, LOG)]);
    }
    assert_eq!(count(&game, PLANKS), 12);
    craft(
        &mut game,
        &[(0, PLANKS), (1, PLANKS), (2, PLANKS), (3, PLANKS)],
    );
    craft(&mut game, &[(0, PLANKS), (2, PLANKS)]);
    game.close_container();
    assert_eq!((count(&game, TABLE), count(&game, STICK)), (1, 4));

    // A crafting table for the pickaxe.
    let table = place_ahead(&mut game, TABLE, 0.0);
    use_block(&mut game, table);
    assert!(game.container.is_some(), "the table opened");
    craft(
        &mut game,
        &[
            (0, PLANKS),
            (1, PLANKS),
            (2, PLANKS),
            (4, STICK),
            (7, STICK),
        ],
    );
    craft(&mut game, &[(0, PLANKS), (3, PLANKS)]);
    game.close_container();
    assert_eq!(count(&game, WOODEN_PICKAXE), 1);

    // Stone: cobblestone for a stone pickaxe and a furnace.
    for _ in 0..11 {
        mine(&mut game, 1, Some(WOODEN_PICKAXE));
    }
    assert_eq!(count(&game, COBBLESTONE), 11);
    use_block(&mut game, table);
    craft(
        &mut game,
        &[
            (0, COBBLESTONE),
            (1, COBBLESTONE),
            (2, COBBLESTONE),
            (4, STICK),
            (7, STICK),
        ],
    );
    let ring = [0, 1, 2, 3, 5, 6, 7, 8].map(|cell| (cell, COBBLESTONE));
    craft(&mut game, &ring);
    game.close_container();
    assert_eq!((count(&game, STONE_PICKAXE), count(&game, FURNACE)), (1, 1));

    // Iron: ore and coal need the stone pickaxe, then the furnace.
    mine(&mut game, 16, Some(STONE_PICKAXE));
    for _ in 0..3 {
        mine(&mut game, IRON_ORE, Some(STONE_PICKAXE));
    }
    assert_eq!((count(&game, COAL), count(&game, IRON_ORE)), (1, 3));
    let furnace_at = place_ahead(&mut game, FURNACE, std::f32::consts::FRAC_PI_2);
    use_block(&mut game, furnace_at);
    let ore = game.inventory.find(IRON_ORE).unwrap();
    game.click_slot(SlotRef::Inventory(ore), Click::Left);
    game.click_slot(SlotRef::Block(furnace::INPUT), Click::Left);
    let coal = game.inventory.find(COAL).unwrap();
    game.click_slot(SlotRef::Inventory(coal), Click::Left);
    game.click_slot(SlotRef::Block(furnace::FUEL), Click::Left);
    game.close_container();
    for _ in 0..3 * furnace::COOK_TICKS + 5 {
        game.tick();
    }
    use_block(&mut game, furnace_at);
    game.click_slot(SlotRef::Block(furnace::OUTPUT), Click::Shift);
    game.close_container();
    assert_eq!(count(&game, IRON), 3);
    use_block(&mut game, table);
    craft(
        &mut game,
        &[(0, IRON), (1, IRON), (2, IRON), (4, STICK), (7, STICK)],
    );
    game.close_container();
    assert_eq!(count(&game, IRON_PICKAXE), 1);

    // Diamond: the stone pickaxe can't get it, the iron one can.
    mine(&mut game, 56, Some(STONE_PICKAXE));
    assert_eq!(count(&game, DIAMOND), 0);
    mine(&mut game, 56, Some(IRON_PICKAXE));
    assert_eq!(count(&game, DIAMOND), 1);
}
