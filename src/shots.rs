//! `--shot <name>`: scripted scenes for checking a milestone. The game
//! opens the scene's world (or the one named with `--world`), puts the
//! player where the scene says, waits until every chunk in view is loaded
//! and meshed, stages the scene (placing blocks, running ticks), and saves
//! a frame as a PNG (`docs/screenshots/<name>.png`, or `--out <file>`).
//! Game time stands still between those steps, so a scene looks the same
//! every run.

use glam::IVec3;

use crate::{
    app::WorldKind,
    game::Game,
    player::mode::GameMode,
    world::{
        Face,
        block::{BlockId, BlockState},
        blocks::*,
        props::{Prop, attach_face, axis, bed_part, door_half, half, hinge, slab_type},
    },
};

pub struct Shot {
    pub name: &'static str,
    /// What the screenshot shows.
    pub about: &'static str,
    /// The world, unless the command line names one.
    pub world: Option<WorldKind>,
    /// Feet, yaw and pitch to view the scene from; `None` keeps the
    /// player where the world left them.
    pub view: Option<([f64; 3], f32, f32)>,
    /// Time of day in ticks (6000 is noon), and the game mode; `None`
    /// keeps a saved world's.
    pub time: Option<u64>,
    pub mode: Option<GameMode>,
    /// Shows the F3 overlay.
    pub overlay: bool,
    /// Places the scene's blocks and runs its ticks, once the chunks
    /// around are ready.
    pub stage: fn(&mut Game),
}

/// Every scene, by milestone.
pub static SHOTS: &[Shot] = &[
    Shot {
        name: "m14-gallery",
        about: "every block in its default state and each variant",
        world: Some(WorldKind::Gallery),
        view: Some(([22.5, 64.0, -4.0], std::f32::consts::PI, -0.3)),
        time: Some(6000),
        mode: Some(GameMode::Creative),
        overlay: false,
        stage: nothing,
    },
    Shot {
        name: "m14-block-states",
        about: "stairs (corners worked out from their neighbors), logs on each axis, slabs, doors, trapdoors, torches, levers and rails",
        world: Some(WorldKind::Flat),
        view: Some(([7.0, 68.0, -4.5], std::f32::consts::PI, -0.55)),
        time: Some(6000),
        mode: Some(GameMode::Creative),
        overlay: false,
        stage: block_states,
    },
    Shot {
        name: "m14-block-states-2",
        about: "fences, panes and walls joined up, gates, snow layers, cake, chest, beds, carpets, anvils and more",
        world: Some(WorldKind::Flat),
        view: Some(([7.0, 68.0, 6.0], std::f32::consts::PI, -0.6)),
        time: Some(6000),
        mode: Some(GameMode::Creative),
        overlay: false,
        stage: block_states,
    },
    Shot {
        name: "m14-v1-world-upgraded",
        about: "a v1.0 world (open with --world) after its one-time upgrade, as the player left it",
        world: None,
        view: None,
        time: None,
        mode: None,
        overlay: true,
        stage: nothing,
    },
    Shot {
        name: "m14-build-limit",
        about: "a pillar built from the ground to Y 255, the new top of the world",
        world: Some(WorldKind::Flat),
        view: Some(([-15.5, 222.0, 15.5], 0.785, 0.32)),
        time: Some(6000),
        mode: Some(GameMode::Creative),
        overlay: true,
        stage: build_limit,
    },
    Shot {
        name: "m14-slab-and-torch",
        about: "standing on a bottom slab (feet at Y 64.5), the crosshair on a torch's small outline",
        world: Some(WorldKind::Flat),
        view: Some(([0.5, 64.5, -1.5], 0.0, -1.07)),
        time: Some(6000),
        mode: Some(GameMode::Survival),
        overlay: true,
        stage: slab_and_torch,
    },
    Shot {
        name: "m14-falling-sand",
        about: "a sand pillar falling into the hole dug out under it",
        world: Some(WorldKind::Flat),
        view: Some(([3.5, 65.0, 0.5], -0.6, -0.15)),
        time: Some(6000),
        mode: Some(GameMode::Creative),
        overlay: false,
        stage: falling_sand,
    },
    Shot {
        name: "m16-hand-empty",
        about: "first person with an empty hand: the skin's right arm at the lower right",
        world: Some(WorldKind::Flat),
        view: Some(([0.5, 64.0, 0.5], 0.0, 0.0)),
        time: Some(6000),
        mode: Some(GameMode::Survival),
        overlay: false,
        stage: nothing,
    },
    Shot {
        name: "m16-hand-sword",
        about: "first person holding an iron sword, extruded from its sprite",
        world: Some(WorldKind::Flat),
        view: Some(([0.5, 64.0, 0.5], 0.0, 0.0)),
        time: Some(6000),
        mode: Some(GameMode::Survival),
        overlay: false,
        stage: hold_sword,
    },
    Shot {
        name: "m16-hand-block",
        about: "first person holding a grass block, a small cube",
        world: Some(WorldKind::Flat),
        view: Some(([0.5, 64.0, 0.5], 0.0, 0.0)),
        time: Some(6000),
        mode: Some(GameMode::Survival),
        overlay: false,
        stage: hold_block,
    },
    Shot {
        name: "m16-hand-swing",
        about: "first person, a swing under way with a sword in hand",
        world: Some(WorldKind::Flat),
        view: Some(([0.5, 64.0, 0.5], 0.0, 0.0)),
        time: Some(6000),
        mode: Some(GameMode::Survival),
        overlay: false,
        stage: swing_sword,
    },
    Shot {
        name: "m16-third-back",
        about: "third person behind the player, holding a sword",
        world: Some(WorldKind::Flat),
        view: Some(([0.5, 64.0, 0.5], 0.0, 0.0)),
        time: Some(6000),
        mode: Some(GameMode::Survival),
        overlay: false,
        stage: third_back,
    },
    Shot {
        name: "m16-third-front",
        about: "third person in front of the player, looking back at them",
        world: Some(WorldKind::Flat),
        view: Some(([0.5, 64.0, 0.5], 0.0, 0.0)),
        time: Some(6000),
        mode: Some(GameMode::Survival),
        overlay: false,
        stage: third_front,
    },
    Shot {
        name: "m16-third-sneak",
        about: "third person behind the player, sneaking",
        world: Some(WorldKind::Flat),
        view: Some(([0.5, 64.0, 0.5], 0.0, 0.0)),
        time: Some(6000),
        mode: Some(GameMode::Survival),
        overlay: false,
        stage: third_sneak,
    },
    Shot {
        name: "m23-village",
        about: "a plains village by spawn on seed 80: the well, streets, houses, farms and lamps",
        world: Some(WorldKind::Generated { seed: 80 }),
        view: Some(([8.5, 96.0, 30.5], std::f32::consts::PI, -0.42)),
        time: Some(4000),
        mode: Some(GameMode::Creative),
        overlay: false,
        stage: nothing,
    },
    Shot {
        name: "m22-glint",
        about: "an enchanted pickaxe in hand, enchanted and plain swords lying ahead, experience orbs and the XP bar",
        world: Some(WorldKind::Flat),
        view: Some(([0.5, 64.0, 0.5], 0.0, -0.45)),
        time: Some(6000),
        mode: Some(GameMode::Survival),
        overlay: false,
        stage: glint_scene,
    },
    Shot {
        name: "m22-workstations",
        about: "an enchanting table ringed by bookshelves, an anvil and a grindstone",
        world: Some(WorldKind::Flat),
        view: Some(([0.5, 64.0, 5.5], 0.0, -0.35)),
        time: Some(6000),
        mode: Some(GameMode::Survival),
        overlay: false,
        stage: workstations,
    },
    Shot {
        name: "m21-badlands",
        about: "badlands on seed 20111118: terracotta bands on the slopes, red sand on top",
        world: Some(WorldKind::Generated { seed: 20111118 }),
        view: Some(([3336.5, 168.0, -39.5], std::f32::consts::PI, -0.35)),
        time: Some(6000),
        mode: Some(GameMode::Creative),
        overlay: false,
        stage: nothing,
    },
    Shot {
        name: "m21-peaks",
        about: "jagged peaks on seed 20111118: snow blocks on the tops, bare stone on steep faces",
        world: Some(WorldKind::Generated { seed: 20111118 }),
        view: Some(([370.5, 208.0, 1130.5], std::f32::consts::PI, -0.35)),
        time: Some(6000),
        mode: Some(GameMode::Creative),
        overlay: false,
        stage: nothing,
    },
    Shot {
        name: "m21-mushroom",
        about: "a mushroom island on seed 20111118: mycelium and huge mushrooms",
        world: Some(WorldKind::Generated { seed: 20111118 }),
        view: Some(([256.5, 88.0, -635.5], std::f32::consts::PI, -0.35)),
        time: Some(6000),
        mode: Some(GameMode::Creative),
        overlay: false,
        stage: nothing,
    },
    Shot {
        name: "m24-wastes",
        about: "nether wastes on seed 20111118: netherrack caverns over the lava sea, glowstone under the ceiling",
        world: Some(WorldKind::Nether { seed: 20111118 }),
        view: Some(([-96.5, 45.0, 96.5], std::f32::consts::PI, -0.25)),
        time: Some(6000),
        mode: Some(GameMode::Creative),
        overlay: true,
        stage: nothing,
    },
    Shot {
        name: "m24-crimson",
        about: "crimson forest on seed 20111118: crimson nylium, huge crimson fungi with shroomlight and weeping vines",
        world: Some(WorldKind::Nether { seed: 20111118 }),
        view: Some(([176.5, 54.0, 176.5], std::f32::consts::PI, -0.25)),
        time: Some(6000),
        mode: Some(GameMode::Creative),
        overlay: true,
        stage: nothing,
    },
    Shot {
        name: "m24-warped",
        about: "warped forest on seed 20111118: warped nylium, huge warped fungi, roots, sprouts and twisting vines",
        world: Some(WorldKind::Nether { seed: 20111118 }),
        view: Some(([0.5, 68.0, 208.5], std::f32::consts::PI, -0.25)),
        time: Some(6000),
        mode: Some(GameMode::Creative),
        overlay: true,
        stage: nothing,
    },
    Shot {
        name: "m24-valley",
        about: "soul sand valley on seed 20111118: soul sand and soul soil, basalt pillars and soul fire, in blue fog",
        world: Some(WorldKind::Nether { seed: 20111118 }),
        view: Some(([-176.5, 54.0, -176.5], std::f32::consts::PI, -0.25)),
        time: Some(6000),
        mode: Some(GameMode::Creative),
        overlay: true,
        stage: nothing,
    },
    Shot {
        name: "m24-deltas",
        about: "basalt deltas on seed 20111118: basalt and blackstone, basalt columns and lava pools rimmed with magma",
        world: Some(WorldKind::Nether { seed: 20111118 }),
        view: Some(([-16.5, 84.0, 16.5], std::f32::consts::PI, -0.25)),
        time: Some(6000),
        mode: Some(GameMode::Creative),
        overlay: true,
        stage: nothing,
    },
    Shot {
        name: "m26-brewing",
        about: "a brewing stand mid-brew: three awkward potions becoming fire resistance with magma cream, blaze powder burning",
        world: Some(WorldKind::Flat),
        view: Some(([0.5, 64.0, 2.5], 0.0, -0.4)),
        time: Some(6000),
        mode: Some(GameMode::Survival),
        overlay: false,
        stage: brewing_scene,
    },
    Shot {
        name: "m26-effects",
        about: "fire resistance, swiftness, strength and night vision on the HUD with their timers, standing in lava unhurt",
        world: Some(WorldKind::Flat),
        view: Some(([0.5, 64.0, 0.5], 0.0, -0.3)),
        time: Some(18000),
        mode: Some(GameMode::Survival),
        overlay: false,
        stage: effects_scene,
    },
    Shot {
        name: "m25-fortress",
        about: "a Nether fortress on seed 20111118: bridges on pillars into the lava, a blaze throne, the castle",
        world: Some(WorldKind::Nether { seed: 20111118 }),
        view: Some(([-399.5, 63.0, -489.5], 0.0, -0.15)),
        time: Some(6000),
        mode: Some(GameMode::Creative),
        overlay: true,
        stage: nothing,
    },
    Shot {
        name: "m25-bastion",
        about: "a bastion remnant on seed 20111118: the blackstone keep with its ramparts and the hoglin stable",
        world: Some(WorldKind::Nether { seed: 20111118 }),
        view: Some(([-136.5, 50.0, 135.5], 0.0, -0.25)),
        time: Some(6000),
        mode: Some(GameMode::Creative),
        overlay: true,
        stage: nothing,
    },
    Shot {
        name: "m25-ruined-portal",
        about: "a ruined portal in the Nether on seed 20111118: broken obsidian frame, crying obsidian, a chest",
        world: Some(WorldKind::Nether { seed: 20111118 }),
        view: Some(([56.5, 59.0, -162.5], 0.0, -0.1)),
        time: Some(6000),
        mode: Some(GameMode::Creative),
        overlay: true,
        stage: nothing,
    },
    Shot {
        name: "fixes4-bow-and-horse",
        about: "third person: a skeleton holding its bow beside a saddled, armored horse with markings",
        world: Some(WorldKind::Flat),
        view: Some(([-0.5, 64.0, 8.5], 0.0, 0.05)),
        time: Some(6000),
        mode: Some(GameMode::Creative),
        overlay: false,
        stage: bow_and_horse,
    },
    Shot {
        name: "m25-mobs",
        about: "a blaze, a wither skeleton and a piglin brute",
        world: Some(WorldKind::Flat),
        view: Some(([0.5, 64.0, 7.5], 0.0, 0.0)),
        time: Some(6000),
        mode: Some(GameMode::Creative),
        overlay: false,
        stage: fortress_mobs,
    },
    Shot {
        name: "m24-mobs",
        about: "the Nether's mobs: a ghast, a big magma cube, a zombified piglin, a piglin, a hoglin and a saddled strider on lava",
        world: Some(WorldKind::Flat),
        view: Some(([0.5, 64.0, 9.5], 0.0, -0.05)),
        time: Some(6000),
        mode: Some(GameMode::Creative),
        overlay: false,
        stage: nether_mobs,
    },
    Shot {
        name: "m24-portal",
        about: "a lit Nether portal in a frame of obsidian, and a larger 4 × 5 one beside it",
        world: Some(WorldKind::Flat),
        view: Some(([2.5, 64.0, 9.5], 0.0, -0.1)),
        time: Some(6000),
        mode: Some(GameMode::Creative),
        overlay: false,
        stage: portals,
    },
    Shot {
        name: "m27-island",
        about: "the End's main island from its edge: end stone, the obsidian pillars with their crystals (two caged), the fountain, and the dragon",
        world: Some(WorldKind::End { seed: 20111118 }),
        view: Some(([0.5, 64.0, 12.5], 0.0, 0.3)),
        time: Some(6000),
        mode: Some(GameMode::Creative),
        overlay: false,
        stage: end_island,
    },
    Shot {
        name: "m27-dragon",
        about: "the ender dragon close up, mouth open, with the boss bar over it",
        world: Some(WorldKind::End { seed: 20111118 }),
        view: Some(([24.5, 64.0, 6.5], -1.57, 0.25)),
        time: Some(6000),
        mode: Some(GameMode::Creative),
        overlay: false,
        stage: end_dragon,
    },
    Shot {
        name: "m27-end-portal",
        about: "a ring of twelve end portal frames, each holding an eye, round a lit portal",
        world: Some(WorldKind::Flat),
        view: Some(([0.5, 66.0, 9.5], 0.0, 0.35)),
        time: Some(6000),
        mode: Some(GameMode::Creative),
        overlay: false,
        stage: end_portal,
    },
    Shot {
        name: "fixes1-end-portal-eyes",
        about: "the ring of end portal frames from above and close: each frame is 13/16 tall with its eye, 8×3×8, in the eye texture on top and sides",
        world: Some(WorldKind::Flat),
        view: Some(([0.5, 66.0, 4.5], 0.0, -0.62)),
        time: Some(6000),
        mode: Some(GameMode::Creative),
        overlay: false,
        stage: end_portal,
    },
    Shot {
        name: "m28-end-city",
        about: "an End city on an outer island 1000 blocks out: the purpur and end stone tower with its side towers and bridges, a ship off the south, shulkers",
        world: Some(WorldKind::End { seed: 20111118 }),
        view: Some(([27.5, 100.0, -940.0], 0.0, 0.12)),
        time: Some(6000),
        mode: Some(GameMode::Creative),
        overlay: false,
        stage: nothing,
    },
    Shot {
        name: "m29-advancements",
        about: "the advancements screen on the Minecraft tab part-way through a playthrough: made ones framed and lit, the next ones open, the rest dim",
        world: Some(WorldKind::Flat),
        view: Some(([0.5, 64.0, 0.5], 0.0, 0.0)),
        time: Some(6000),
        mode: Some(GameMode::Survival),
        overlay: false,
        stage: advancements,
    },
    Shot {
        name: "fixes-inventory",
        about: "the survival inventory with the player turned toward the pointer, the recipe book open with craftable recipes first, and the pointer on a slot",
        world: Some(WorldKind::Flat),
        view: Some(([0.5, 64.0, 0.5], 0.0, 0.0)),
        time: Some(6000),
        mode: Some(GameMode::Survival),
        overlay: false,
        stage: inventory_with_book,
    },
    Shot {
        name: "fixes-creative",
        about: "the creative inventory on its Building Blocks tab, with the tabs along the top and bottom and the hotbar under the grid",
        world: Some(WorldKind::Flat),
        view: Some(([0.5, 64.0, 0.5], 0.0, 0.0)),
        time: Some(6000),
        mode: Some(GameMode::Creative),
        overlay: false,
        stage: creative_tabs,
    },
    Shot {
        name: "m31-armor",
        about: "the survival inventory with armor on the player (an enchanted diamond helmet, an iron chestplate, leather leggings, golden boots), all text in the Minecraft font, and a tooltip with section-sign colors",
        world: Some(WorldKind::Flat),
        view: Some(([0.5, 64.0, 0.5], 0.0, 0.0)),
        time: Some(6000),
        mode: Some(GameMode::Survival),
        overlay: false,
        stage: inventory_with_armor,
    },
    Shot {
        name: "m32-torchlight",
        about: "at night, a torch-lit stone room with a doorway onto a pool, an oak boat floating on the water outside",
        world: Some(WorldKind::Flat),
        view: Some(([0.5, 64.0, 2.5], 0.0, -0.12)),
        time: Some(18000),
        mode: Some(GameMode::Creative),
        overlay: false,
        stage: torchlit_room,
    },
    Shot {
        name: "m33-ruined-portal",
        about: "a ruined portal on the land surface in a forest on seed 20111118: broken obsidian frame, netherrack and stone bricks, its chest, the inside clear",
        world: Some(WorldKind::Generated { seed: 20111118 }),
        view: Some(([199.5, 86.0, 391.5], 0.0, -0.45)),
        time: Some(6000),
        mode: Some(GameMode::Creative),
        overlay: false,
        stage: nothing,
    },
    Shot {
        name: "m34-cheese-cave",
        about: "a deepslate cheese cave at Y -18 on seed 20111118, lit by torches on its floor",
        world: Some(WorldKind::Generated { seed: 20111118 }),
        view: Some(([-79.5, -17.0, 66.5], 2.6, -0.2)),
        time: Some(18000),
        mode: Some(GameMode::Creative),
        overlay: false,
        stage: torches_in_cave,
    },
    Shot {
        name: "m35-key-binds",
        about: "Options, Controls, Key Binds over the paused game: every action by category, Sneak moved onto W so both show red, and Jump waiting for a new key",
        world: Some(WorldKind::Flat),
        view: Some(([0.5, 64.0, 0.5], 0.0, -0.2)),
        time: Some(6000),
        mode: Some(GameMode::Survival),
        overlay: false,
        stage: key_binds,
    },
    Shot {
        name: "m27-credits",
        about: "the poem scrolling up the black screen after the exit portal",
        world: Some(WorldKind::End { seed: 20111118 }),
        view: None,
        time: Some(6000),
        mode: Some(GameMode::Survival),
        overlay: false,
        stage: end_credits,
    },
];

/// Puts `item` in the hotbar's first slot and selects it, settling the
/// hand's equip dip.
fn hold(game: &mut Game, item: crate::inventory::item::ItemId) {
    game.player
        .inventory
        .set(0, Some(crate::inventory::stack::ItemStack::new(item, 1)));
    game.player.inventory.selected = 0;
    game.run_ticks(10);
}

fn hold_sword(game: &mut Game) {
    hold(game, crate::inventory::items::IRON_SWORD);
}

fn hold_block(game: &mut Game) {
    hold(game, crate::inventory::items::GRASS_BLOCK);
}

fn swing_sword(game: &mut Game) {
    hold_sword(game);
    game.swing_for_shot(3);
}

/// An enchanted pickaxe held, enchanted and plain swords on the ground,
/// orbs falling and a half-full bar at level 12.
fn glint_scene(game: &mut Game) {
    use crate::{
        entity::item::ItemEntity,
        inventory::{enchant::Enchantment, items, stack::ItemStack},
    };
    let mut pickaxe = ItemStack::new(items::DIAMOND_PICKAXE, 1);
    pickaxe.enchant(Enchantment::Efficiency, 5);
    game.player.inventory.set(0, Some(pickaxe));
    game.player.inventory.selected = 0;
    let mut sword = ItemStack::new(items::DIAMOND_SWORD, 1);
    sword.enchant(Enchantment::Sharpness, 5);
    for (x, stack) in [
        (-1.0, sword),
        (0.0, ItemStack::new(items::DIAMOND_SWORD, 1)),
        (1.0, ItemStack::new(items::DIAMOND_PICKAXE, 1)),
    ] {
        game.entities.push(ItemEntity::new(
            stack,
            glam::DVec3::new(0.5 + x, 64.2, -2.0),
            glam::DVec3::ZERO,
            0.0,
        ));
    }
    game.player
        .stats
        .experience
        .add(crate::player::experience::points_to_reach(12) + 14);
    game.spawn_xp(37, glam::DVec3::new(0.5, 64.6, -3.5));
    game.spawn_xp(11, glam::DVec3::new(1.5, 64.6, -3.5));
    game.run_ticks(12);
}

/// A table with its ring of bookshelves, an anvil and a grindstone.
fn workstations(game: &mut Game) {
    let table = at(0, 64, 0);
    let mut edits = vec![(table, BlockState::of(ENCHANTING_TABLE))];
    for x in -2..=2 {
        for z in -2..=2 {
            if i32::abs(x) == 2 || i32::abs(z) == 2 {
                // Leave the south side open to look in from.
                if z == 2 {
                    continue;
                }
                for y in 0..2 {
                    edits.push((table + at(x, y, z), BlockState::of(BOOKSHELF)));
                }
            }
        }
    }
    edits.push((at(3, 64, 3), facing(ANVIL, Face::East)));
    edits.push((at(-3, 64, 3), facing(GRINDSTONE, Face::East)));
    game.set_blocks(&edits);
    game.run_ticks(5);
}

/// A stone room lit by four torches, its north doorway onto a pool with a
/// boat on it.
fn torchlit_room(game: &mut Game) {
    let mut edits = Vec::new();
    for x in -4..=4 {
        for z in -4..=4 {
            for y in 64..=67 {
                let wall = x == -4 || x == 4 || z == -4 || z == 4;
                let door = z == -4 && (-1..=1).contains(&x) && y <= 65;
                if (wall && !door) || y == 67 {
                    edits.push((at(x, y, z), BlockState::of(COBBLESTONE)));
                }
            }
        }
    }
    for (x, z) in [(-3, -3), (3, -3), (-3, 3), (3, 3)] {
        edits.push((at(x, 64, z), BlockState::of(TORCH)));
    }
    for x in -7..=7 {
        for z in -16..=-5 {
            for y in 60..=63 {
                edits.push((at(x, y, z), BlockState::of(WATER)));
            }
        }
    }
    game.set_blocks(&edits);
    let kind = crate::entity::boat::BoatKind::of_item("oak_boat").unwrap();
    game.boats.push(crate::entity::boat::Boat::new(
        1,
        kind,
        glam::DVec3::new(1.5, 63.5, -9.0),
        0.5,
    ));
    game.run_ticks(40);
}

/// Torches on the cave floor around (−72, −18, 56), every few blocks.
fn torches_in_cave(game: &mut Game) {
    let world = game.chunks.world();
    let mut edits = Vec::new();
    for x in (-88..=-56).step_by(5) {
        for z in (40..=72).step_by(5) {
            let floor = (-40..-10).rev().find(|&y| {
                world.block(x, y, z).is_some_and(|b| b.def().solid)
                    && world.block(x, y + 1, z).is_some_and(BlockState::is_air)
            });
            if let Some(y) = floor {
                edits.push((at(x, y + 1, z), BlockState::of(TORCH)));
            }
        }
    }
    game.set_blocks(&edits);
    game.run_ticks(5);
}

fn third_back(game: &mut Game) {
    hold_sword(game);
    game.set_view_mode(crate::game::view::ViewMode::ThirdBack);
}

fn third_front(game: &mut Game) {
    hold_sword(game);
    game.set_view_mode(crate::game::view::ViewMode::ThirdFront);
}

fn third_sneak(game: &mut Game) {
    hold_sword(game);
    game.set_view_mode(crate::game::view::ViewMode::ThirdBack);
    game.sneak_for_shot();
}

/// A brewing stand by a cauldron, opened mid-brew.
fn brewing_scene(game: &mut Game) {
    use crate::inventory::{brewing, items, potion, stack::ItemStack};
    let stand = at(0, 64, 0);
    game.set_blocks(&[
        (stand, BlockState::of(BREWING_STAND)),
        (
            at(1, 64, 0),
            BlockState::of(WATER_CAULDRON).with_number(Prop::LevelCauldron, 2),
        ),
    ]);
    game.open_brewing(stand);
    if let Some(crate::world::block_entity::BlockEntity::BrewingStand(s)) =
        game.chunks.block_entity_mut(stand)
    {
        for i in brewing::BOTTLES {
            s.slots[i] = Some(potion::stack(items::POTION, "awkward"));
        }
        s.slots[brewing::INGREDIENT] = Some(ItemStack::new(items::MAGMA_CREAM, 3));
        s.slots[brewing::FUEL] = Some(ItemStack::new(items::BLAZE_POWDER, 4));
    }
    game.player
        .inventory
        .set(0, Some(potion::stack(items::POTION, "fire_resistance")));
    game.player
        .inventory
        .set(1, Some(potion::stack(items::SPLASH_POTION, "healing")));
    game.player
        .inventory
        .set(2, Some(potion::stack(items::POTION, "water")));
    game.run_ticks(180);
}

/// Four effects running, in a pool of lava.
fn effects_scene(game: &mut Game) {
    use crate::player::effects::{Active, Effect};
    for (effect, amplifier, ticks) in [
        (Effect::FireResistance, 0, 3600),
        (Effect::Speed, 1, 1800),
        (Effect::Strength, 0, 2400),
        (Effect::NightVision, 0, 9600),
    ] {
        game.player
            .stats
            .apply(Active::new(effect, amplifier, ticks), false, true);
    }
    let mut lava = Vec::new();
    for x in -2..=2 {
        for z in -4..=0 {
            lava.push((at(x, 63, z), BlockState::of(LAVA)));
        }
    }
    game.set_blocks(&lava);
    game.run_ticks(40);
}

/// A skeleton with its bow beside a horse, seen over the player's shoulder.
fn bow_and_horse(game: &mut Game) {
    let south = std::f32::consts::PI;
    game.set_view_mode(crate::game::view::ViewMode::ThirdBack);
    if let Some(skeleton) = game.summon("skeleton", glam::DVec3::new(1.2, 64.0, 3.0), south) {
        skeleton.fire_immune = true;
    }
    if let Some(horse) = game.summon("horse", glam::DVec3::new(4.0, 64.0, 3.0), south + 0.4) {
        horse.data.variant = 2;
        horse.data.markings = 3;
        horse.data.saddled = true;
        horse.data.armor = 2;
    }
    game.run_ticks(2);
}

/// The fortresses' and bastions' mobs.
fn fortress_mobs(game: &mut Game) {
    let south = std::f32::consts::PI;
    game.summon("blaze", glam::DVec3::new(-2.5, 65.0, 2.5), south);
    game.summon("wither_skeleton", glam::DVec3::new(0.5, 64.0, 2.5), south);
    game.summon("piglin_brute", glam::DVec3::new(3.0, 64.0, 2.5), south);
    game.run_ticks(2);
}

/// The six Nether mobs in a row, the strider on a pool of lava.
fn nether_mobs(game: &mut Game) {
    let mut pool = Vec::new();
    for x in 4..=7 {
        for z in -1..=2 {
            pool.push((at(x, 63, z), BlockState::of(LAVA)));
        }
    }
    game.set_blocks(&pool);
    let south = std::f32::consts::PI;
    game.summon("ghast", glam::DVec3::new(0.5, 70.0, -6.0), south);
    if let Some(cube) = game.summon("magma_cube", glam::DVec3::new(-5.5, 64.0, 1.5), south) {
        cube.data.size = 4;
    }
    game.summon("zombified_piglin", glam::DVec3::new(-3.0, 64.0, 2.5), south);
    game.summon("piglin", glam::DVec3::new(-1.0, 64.0, 2.5), south);
    game.summon("hoglin", glam::DVec3::new(2.0, 64.0, 1.5), south);
    if let Some(strider) = game.summon("strider", glam::DVec3::new(5.5, 64.0, 1.0), south) {
        strider.data.saddled = true;
    }
    game.run_ticks(2);
}

/// Two portals: 2 × 3 inside, and 4 × 5 beside it.
fn portals(game: &mut Game) {
    let mut edits = Vec::new();
    for (x0, width, height) in [(-2, 2, 3), (3, 4, 5)] {
        for x in x0 - 1..=x0 + width {
            for y in 63..=64 + height {
                let side = x == x0 - 1 || x == x0 + width;
                let end = y == 63 || y == 64 + height;
                let block = match (side, end) {
                    (true, true) => continue,
                    (false, false) => BlockState::of(NETHER_PORTAL),
                    _ => BlockState::of(OBSIDIAN),
                };
                edits.push((at(x, y, 0), block));
            }
        }
    }
    game.set_blocks(&edits);
    game.run_ticks(5);
}

/// The island with the dragon circling over it, high up and near.
fn end_island(game: &mut Game) {
    if let Some(dragon) = game.dragon.as_mut() {
        // Over the north of the island, coming back toward the fountain.
        let spot = glam::DVec3::new(0.5, 84.0, -18.0);
        dragon.position = spot;
        dragon.prev_position = spot;
        dragon.yaw = std::f32::consts::PI;
        dragon.prev_yaw = std::f32::consts::PI;
    }
    game.run_ticks(2);
}

/// The dragon right in front of the player, jaws open.
fn end_dragon(game: &mut Game) {
    if let Some(dragon) = game.dragon.as_mut() {
        let spot = glam::DVec3::new(-2.0, 78.0, 6.5);
        dragon.position = spot;
        dragon.prev_position = spot;
        dragon.yaw = 2.2;
        dragon.prev_yaw = 2.2;
        dragon.hurt(crate::entity::dragon::Part::Head, 40.0);
    }
    game.run_ticks(1);
}

/// Twelve frames with eyes and the portal between them.
fn end_portal(game: &mut Game) {
    let center = at(0, 63, 0);
    let mut edits = Vec::new();
    for (cell, face) in crate::world::end_portal::ring(center) {
        edits.push((
            cell,
            facing(END_PORTAL_FRAME, face).with_bool(Prop::Eye, true),
        ));
    }
    for cell in crate::world::end_portal::middle(center) {
        edits.push((cell, BlockState::of(END_PORTAL)));
    }
    game.set_blocks(&edits);
    game.run_ticks(2);
}

/// The advancements screen with the first goals of the story made.
fn advancements(game: &mut Game) {
    use crate::advancements::Event;
    game.run_ticks(2);
    for name in [
        "cobblestone",
        "stone_pickaxe",
        "iron_ingot",
        "iron_pickaxe",
        "diamond",
        "obsidian",
        "iron_chestplate",
        "lava_bucket",
    ] {
        game.note(Event::Have(name.to_owned()));
    }
    game.note(Event::Dimension("the_nether".to_owned()));
    game.stage_advancements(0);
}

/// The survival inventory with some materials, a sword in hand and the
/// recipe book open.
fn inventory_with_book(game: &mut Game) {
    use crate::{
        game::{ShotScreen, ShotUi},
        inventory::{items, slots::SlotRef, stack::ItemStack},
    };
    game.run_ticks(2);
    for (slot, stack) in [
        (0, ItemStack::new(items::IRON_SWORD, 1)),
        (1, ItemStack::new(items::OAK_LOG, 12)),
        (2, ItemStack::new(items::OAK_PLANKS, 24)),
        (3, ItemStack::new(items::STICK, 9)),
        (9, ItemStack::new(items::COBBLESTONE, 40)),
        (10, ItemStack::new(items::IRON_INGOT, 6)),
        (11, ItemStack::new(items::COAL, 16)),
        (12, ItemStack::new(items::BREAD, 5)),
    ] {
        game.player.inventory.set(slot, Some(stack));
    }
    game.player.inventory.selected = 0;
    game.player.recipe_book.open = true;
    game.stage_ui(ShotUi {
        screen: ShotScreen::Inventory,
        hover: Some(SlotRef::Inventory(10)),
    });
}

/// The inventory with armor worn and a renamed sword hovered.
fn key_binds(game: &mut Game) {
    game.stage_ui(crate::game::ShotUi {
        screen: crate::game::ShotScreen::KeyBinds,
        hover: None,
    });
}

fn inventory_with_armor(game: &mut Game) {
    use crate::{
        game::{ShotScreen, ShotUi},
        inventory::{
            enchant::Enchantment,
            items,
            player::{ARMOR_CHEST, ARMOR_FEET, ARMOR_HEAD, ARMOR_LEGS},
            slots::SlotRef,
            stack::ItemStack,
        },
    };
    game.run_ticks(2);
    let mut helmet = ItemStack::new(items::DIAMOND_HELMET, 1);
    helmet.enchant(Enchantment::Protection, 4);
    let mut sword = ItemStack::new(items::IRON_SWORD, 1);
    sword.set_custom_name(Some("§6Golden §lEdge".to_owned()));
    sword.enchant(Enchantment::Sharpness, 3);
    for (slot, stack) in [
        (ARMOR_HEAD, helmet),
        (ARMOR_CHEST, ItemStack::new(items::IRON_CHESTPLATE, 1)),
        (ARMOR_LEGS, ItemStack::new(items::LEATHER_LEGGINGS, 1)),
        (ARMOR_FEET, ItemStack::new(items::GOLDEN_BOOTS, 1)),
        (0, sword),
        (1, ItemStack::new(items::OAK_DOOR, 64)),
        (2, ItemStack::new(items::OAK_SIGN, 16)),
        (9, ItemStack::new(items::BUCKET, 16)),
    ] {
        game.player.inventory.set(slot, Some(stack));
    }
    game.player.inventory.selected = 0;
    game.player.recipe_book.open = false;
    game.stage_ui(ShotUi {
        screen: ShotScreen::Inventory,
        hover: Some(SlotRef::Inventory(0)),
    });
}

/// The creative inventory on Building Blocks.
fn creative_tabs(game: &mut Game) {
    use crate::{
        game::{ShotScreen, ShotUi},
        inventory::{items, slots::SlotRef, stack::ItemStack},
    };
    game.run_ticks(2);
    game.player
        .inventory
        .set(0, Some(ItemStack::new(items::DIAMOND_PICKAXE, 1)));
    game.player
        .inventory
        .set(1, Some(ItemStack::new(items::STONE_BRICKS, 64)));
    game.stage_ui(ShotUi {
        screen: ShotScreen::Creative(0),
        hover: Some(SlotRef::Inventory(1)),
    });
}

/// Part-way through the credits.
fn end_credits(game: &mut Game) {
    game.stage_credits(crate::credits::TICKS / 3);
}

fn at(x: i32, y: i32, z: i32) -> IVec3 {
    IVec3::new(x, y, z)
}

fn facing(id: BlockId, face: Face) -> BlockState {
    BlockState::of(id).with_face(Prop::Facing, face)
}

const SIDES: [Face; 4] = [Face::North, Face::East, Face::South, Face::West];

/// Rows of blocks in their states on the flat world's grass (y 64).
fn block_states(game: &mut Game) {
    let mut edits = Vec::new();
    // Row 0: stairs facing each way, upside down, and the four corners.
    for (i, &face) in SIDES.iter().enumerate() {
        let stairs = facing(OAK_STAIRS, face);
        edits.push((at(i as i32, 64, 0), stairs));
        edits.push((at(i as i32 + 5, 64, 0), stairs.with(Prop::Half, half::TOP)));
    }
    // Corners, worked out from their neighbors: stairs facing north with
    // west-facing stairs behind (an outer corner) and in front (inner).
    let north = facing(COBBLESTONE_STAIRS, Face::North);
    let west = facing(COBBLESTONE_STAIRS, Face::West);
    edits.extend([
        (at(11, 64, 0), north),
        (at(11, 64, -1), west),
        (at(14, 64, 0), north),
        (at(14, 64, 1), west),
    ]);
    // Row 2: logs along each axis, slabs on the bottom, top and doubled.
    for (i, a) in [axis::Y, axis::X, axis::Z].into_iter().enumerate() {
        edits.push((
            at(i as i32, 64, 2),
            BlockState::of(OAK_LOG).with(Prop::Axis, a),
        ));
        edits.push((
            at(i as i32 + 3, 64, 2),
            BlockState::of(BIRCH_LOG).with(Prop::Axis, a),
        ));
    }
    for (i, t) in [slab_type::BOTTOM, slab_type::TOP, slab_type::DOUBLE]
        .into_iter()
        .enumerate()
    {
        edits.push((
            at(i as i32 + 7, 64, 2),
            BlockState::of(OAK_SLAB).with(Prop::SlabType, t),
        ));
        edits.push((
            at(i as i32 + 10, 64, 2),
            BlockState::of(STONE_BRICK_SLAB).with(Prop::SlabType, t),
        ));
    }
    // Row 4: doors closed, open on either hinge; trapdoors.
    let door = |x: i32, id, open: bool, side: u8| {
        let lower = facing(id, Face::North)
            .with_bool(Prop::Open, open)
            .with(Prop::Hinge, side);
        [
            (at(x, 64, 4), lower),
            (
                at(x, 65, 4),
                lower.with(Prop::DoubleBlockHalf, door_half::UPPER),
            ),
        ]
    };
    edits.extend(door(0, OAK_DOOR, false, hinge::LEFT));
    edits.extend(door(2, OAK_DOOR, true, hinge::LEFT));
    edits.extend(door(4, OAK_DOOR, true, hinge::RIGHT));
    edits.extend(door(6, IRON_DOOR, false, hinge::LEFT));
    let trapdoor = facing(OAK_TRAPDOOR, Face::North);
    edits.push((at(8, 64, 4), trapdoor));
    edits.push((at(9, 64, 4), trapdoor.with(Prop::Half, half::TOP)));
    edits.push((at(10, 64, 4), trapdoor.with_bool(Prop::Open, true)));
    edits.push((
        at(11, 64, 4),
        facing(OAK_TRAPDOOR, Face::East).with_bool(Prop::Open, true),
    ));
    // Row 6: a stone post with a torch on each side, floor torches,
    // levers, a button and plates, and rails.
    edits.push((at(1, 64, 6), BlockState::of(STONE)));
    for face in SIDES {
        let beside = at(1, 64, 6) + IVec3::from_array(face.normal());
        edits.push((beside, facing(WALL_TORCH, face)));
    }
    edits.push((at(4, 64, 6), BlockState::of(TORCH)));
    edits.push((at(5, 64, 6), BlockState::of(REDSTONE_TORCH)));
    edits.push((
        at(6, 64, 6),
        facing(LEVER, Face::North).with(Prop::AttachFace, attach_face::FLOOR),
    ));
    edits.push((at(7, 64, 6), BlockState::of(STONE)));
    edits.push((at(7, 64, 5), facing(STONE_BUTTON, Face::North)));
    edits.push((at(8, 64, 6), BlockState::of(STONE_PRESSURE_PLATE)));
    edits.push((at(9, 64, 6), BlockState::of(RAIL)));
    edits.push((at(10, 64, 6), BlockState::of(RAIL).with(Prop::RailShape, 6)));
    edits.push((
        at(11, 64, 6),
        BlockState::of(POWERED_RAIL).with_bool(Prop::Powered, true),
    ));
    // Row 8: fences, panes and bars joined in lines, a gate shut and open.
    // They join each other as they go down.
    for x in 0..3 {
        edits.push((at(x, 64, 8), BlockState::of(OAK_FENCE)));
        edits.push((at(x + 4, 64, 8), BlockState::of(GLASS_PANE)));
        edits.push((at(x + 8, 64, 8), BlockState::of(IRON_BARS)));
    }
    edits.push((at(12, 64, 8), facing(OAK_FENCE_GATE, Face::South)));
    edits.push((
        at(13, 64, 8),
        facing(OAK_FENCE_GATE, Face::South).with_bool(Prop::Open, true),
    ));
    // Row 10: snow layers, cake, chest, farmland, cactus, bed, and the rest.
    for layers in 1..=8 {
        edits.push((
            at(layers as i32 - 1, 64, 10),
            BlockState::of(SNOW).with_number(Prop::Layers, layers),
        ));
    }
    edits.push((
        at(8, 64, 10),
        BlockState::of(CAKE).with_number(Prop::Bites, 3),
    ));
    edits.push((at(9, 64, 10), facing(CHEST, Face::North)));
    edits.push((
        at(10, 63, 10),
        BlockState::of(FARMLAND).with_number(Prop::Moisture, 7),
    ));
    edits.push((
        at(10, 64, 10),
        BlockState::of(WHEAT).with_number(Prop::Age7, 7),
    ));
    edits.push((at(11, 64, 10), BlockState::of(CACTUS)));
    edits.push((at(12, 64, 10), facing(LADDER, Face::North)));
    edits.push((at(12, 64, 11), BlockState::of(STONE)));
    edits.push((at(13, 64, 10), BlockState::of(ENCHANTING_TABLE)));
    edits.push((
        at(14, 64, 10),
        facing(END_PORTAL_FRAME, Face::North).with_bool(Prop::Eye, true),
    ));
    let bed = facing(RED_BED, Face::East);
    edits.push((at(0, 64, 12), bed));
    edits.push((at(1, 64, 12), bed.with(Prop::BedPart, bed_part::HEAD)));
    edits.push((at(3, 64, 12), BlockState::of(BREWING_STAND)));
    edits.push((at(4, 64, 12), BlockState::of(CAULDRON)));
    edits.push((at(5, 64, 12), facing(REPEATER, Face::North)));
    edits.push((at(6, 64, 12), BlockState::of(DRAGON_EGG)));
    edits.push((
        at(7, 64, 12),
        BlockState::of(REDSTONE_WIRE).with_number(Prop::Power, 15),
    ));
    edits.push((at(8, 64, 12), BlockState::of(STONE)));
    edits.push((
        at(8, 64, 13),
        BlockState::of(VINE).with_bool(Prop::North, true),
    ));
    // Row 14: carpets, walls joining in an L, anvils, red sand, concrete
    // powder.
    for (i, id) in [WHITE_CARPET, RED_CARPET, BLUE_CARPET, LIME_CARPET]
        .into_iter()
        .enumerate()
    {
        edits.push((at(i as i32, 64, 14), BlockState::of(id)));
    }
    for (x, z) in [(5, 14), (6, 14), (7, 14), (7, 15)] {
        edits.push((at(x, 64, z), BlockState::of(COBBLESTONE_WALL)));
    }
    edits.push((at(9, 64, 14), facing(ANVIL, Face::East)));
    edits.push((at(10, 64, 14), facing(DAMAGED_ANVIL, Face::North)));
    edits.push((at(12, 64, 14), BlockState::of(RED_SAND)));
    edits.push((at(13, 64, 14), BlockState::of(ORANGE_CONCRETE_POWDER)));
    game.set_blocks(&edits);
}

/// Stone from the ground to the top of the world, banded with wool every
/// 32 blocks (orange at 128, v1.0's limit) and glowstone at Y 255.
fn build_limit(game: &mut Game) {
    let mut edits = line(at(0, 64, 0), IVec3::Y, 192, BlockState::of(STONE));
    for y in (96..255).step_by(32) {
        let band = if y == 128 { ORANGE_WOOL } else { WHITE_WOOL };
        edits.push((at(0, y, 0), BlockState::of(band)));
    }
    edits.push((at(0, 255, 0), BlockState::of(GLOWSTONE)));
    game.set_blocks(&edits);
}

/// A path of bottom slabs to stand on, and a torch on the ground ahead.
fn slab_and_torch(game: &mut Game) {
    let slab = BlockState::of(OAK_SLAB);
    let mut edits = line(at(0, 64, 0), IVec3::NEG_Z, 3, slab);
    edits.push((at(0, 64, -3), BlockState::of(TORCH)));
    edits.push((at(1, 64, -3), BlockState::of(DANDELION)));
    game.set_blocks(&edits);
}

/// Sand stacked three high on the grass, then the ground under it dug out
/// three deep, a few ticks into its fall.
fn falling_sand(game: &mut Game) {
    let sand = BlockState::of(SAND);
    game.set_blocks(&line(at(0, 64, -4), IVec3::Y, 3, sand));
    game.set_blocks(&line(at(0, 63, -4), IVec3::NEG_Y, 3, BlockState::AIR));
    game.run_ticks(9);
}

pub fn find(name: &str) -> Option<&'static Shot> {
    SHOTS.iter().find(|shot| shot.name == name)
}

/// A scene that places nothing.
pub fn nothing(_: &mut Game) {}

/// `count` blocks in a line from `from` along `step`.
pub fn line(from: IVec3, step: IVec3, count: i32, block: BlockState) -> Vec<(IVec3, BlockState)> {
    (0..count).map(|i| (from + step * i, block)).collect()
}
