//! The player as drawn each frame: in first person the hand, in third
//! person the full model with what it holds. Builds the meshes from the
//! game's view state; the renderer draws them.

use image::RgbaImage;

use crate::{
    game::Game,
    inventory::{
        item::{self, ItemId},
        items,
        player::Hand,
        stack::ItemStack,
    },
    pack::model::{DisplayContext, ItemModels, Transform},
    render::{
        hand::{self, HandInput, HandItem},
        held::{HeldCache, display_matrix},
        icons::ItemIcons,
        player::{WornArmor, add_armor, add_player, body_frame, hand_mount, model_matrix, rig},
    },
    world::mesh::Mesh,
};

/// What drawing the player needs besides the game.
pub struct Figure<'a> {
    pub icons: &'a ItemIcons,
    pub item_models: &'a ItemModels,
    /// The atlas picture, for sprites' outlines.
    pub atlas: &'a RgbaImage,
    /// Whether the skin has slim arms.
    pub slim: bool,
}

/// The meshes for one frame.
#[derive(Debug, Default)]
pub struct PlayerMeshes {
    /// The model, textured with the skin (third person).
    pub body: Mesh,
    /// The elytra on its back, textured with its own skin (third person).
    pub elytra: Mesh,
    /// The armor it wears, textured with the armor sheet (third person).
    pub armor: Mesh,
    /// The enchanted pieces of it again, for the inventory's glint.
    pub armor_glint: Mesh,
    /// What the model holds, textured with the atlas (third person).
    pub items: Mesh,
    /// The first-person arm, textured with the skin, in camera space.
    pub hand_arm: Mesh,
    /// The first-person item, textured with the atlas, in camera space.
    pub hand_item: Mesh,
}

/// Builds what shows of the player in the current view. `body_light` and
/// `hand_light` are the (sky, block) light on the model and the hand.
pub fn build(
    game: &Game,
    held: &mut HeldCache,
    fig: &Figure,
    body_light: (u8, u8),
    hand_light: (u8, u8),
) -> PlayerMeshes {
    use crate::game::view::ViewMode;
    let mut meshes = PlayerMeshes::default();
    // Dead or in bed, there's nothing to see of the player.
    if game.player.stats.is_dead() || game.is_sleeping() {
        return meshes;
    }
    let alpha = game.tick_alpha() as f32;
    let view = game.view();
    let icons = fig.icons;
    let pull = game.bow_ticks();
    // The icon a stack shows: a bow in the main hand looks drawn while it is.
    let icon = |stack: &ItemStack, hand: Hand| match (pull, hand) {
        (Some(ticks), Hand::Main) => icons.icon_drawn(stack, ticks),
        _ => icons.icon(stack),
    };
    let blocking = game.is_blocking();
    match view.mode {
        ViewMode::First => {
            // What each hand shows lags the selection while it dips.
            let shown = |id: Option<ItemId>, hand: Hand| {
                id.map(|id| {
                    game.player
                        .inventory
                        .hand_stack(hand)
                        .cloned()
                        .filter(|stack| stack.id == id)
                        .unwrap_or_else(|| ItemStack::new(id, 1))
                })
            };
            let main = shown(view.hand_item(), Hand::Main).map(|stack| {
                let up = blocking && game.shield_hand() == Some(Hand::Main);
                (
                    held.get(&icon(&stack, Hand::Main), fig.atlas).clone(),
                    stack,
                    up,
                )
            });
            let item = main.as_ref().map(|(geometry, stack, up)| HandItem {
                geometry,
                display: first_person_display(fig, stack, game.left_handed(), *up),
                glint: stack.is_enchanted(),
            });
            let input = HandInput {
                left: game.left_handed(),
                swing: view.attack_anim(alpha),
                equip: view.equip_progress(alpha),
                lag: view.hand_lag(alpha, game.player.camera.yaw, game.player.camera.pitch),
                bob: view.view_bob(alpha),
                using: game.use_progress(),
                item,
                slim: fig.slim,
                light: hand_light,
            };
            let hand = hand::build(&input);
            meshes.hand_arm = hand.arm;
            meshes.hand_item = hand.item;
            // The off-hand's item, if it holds one, mirrored on the left.
            let off = shown(view.offhand_item(), Hand::Off).map(|stack| {
                let up = blocking && game.shield_hand() == Some(Hand::Off);
                (
                    held.get(&icon(&stack, Hand::Off), fig.atlas).clone(),
                    stack,
                    up,
                )
            });
            if let Some((geometry, stack, up)) = &off {
                let input = HandInput {
                    left: !game.left_handed(),
                    swing: 0.0,
                    equip: view.offhand_equip_progress(alpha),
                    using: crate::game::view::HandUse::None,
                    item: Some(HandItem {
                        geometry,
                        display: first_person_display(fig, stack, !game.left_handed(), *up),
                        glint: stack.is_enchanted(),
                    }),
                    ..input
                };
                meshes.hand_item.append(&hand::build(&input).item);
            }
        }
        ViewMode::ThirdBack | ViewMode::ThirdFront => {
            let pose = game.player_pose();
            let (feet, yaw) = game.player_body();
            let wearer = game.wearer();
            add_body(
                &mut meshes,
                &wearer,
                held,
                fig,
                &pose,
                feet,
                yaw,
                body_light,
            );
        }
    }
    meshes
}

/// Adds the model in `pose` standing at `feet` facing `yaw`, with the elytra
/// it wears and what it holds in its hands, to `meshes`' `body`, `elytra`
/// and `items`.
#[allow(clippy::too_many_arguments)]
pub fn add_body(
    meshes: &mut PlayerMeshes,
    wearer: &Wearer,
    held: &mut HeldCache,
    fig: &Figure,
    pose: &crate::render::player::PlayerPose,
    feet: glam::Vec3,
    yaw: f32,
    light: (u8, u8),
) {
    let icons = fig.icons;
    let pull = wearer.bow_ticks;
    // The icon a stack shows: a bow in the main hand looks drawn while it is.
    let icon = |stack: &ItemStack, hand: Hand| match (pull, hand) {
        (Some(ticks), Hand::Main) => icons.icon_drawn(stack, ticks),
        _ => icons.icon(stack),
    };
    add_player(
        &mut meshes.body,
        pose,
        fig.slim,
        feet,
        yaw,
        [1.0; 3],
        light,
        wearer.layers,
    );
    if wearer.armor[1]
        .as_ref()
        .is_some_and(|s| s.id == items::ELYTRA)
    {
        crate::render::player::add_elytra(&mut meshes.elytra, pose, feet, yaw, light);
    }
    let worn = worn_armor(&wearer.armor);
    let rows = crate::look::ARMOR_MATERIALS.len() * 2;
    add_armor(
        &mut meshes.armor,
        pose,
        fig.slim,
        feet,
        yaw,
        &worn,
        rows,
        light,
    );
    let glinting: Vec<_> = worn.iter().copied().filter(|piece| piece.glint).collect();
    add_armor(
        &mut meshes.armor_glint,
        pose,
        fig.slim,
        feet,
        yaw,
        &glinting,
        rows,
        light,
    );
    let frame = model_matrix(&body_frame(pose, yaw), feet);
    let rig = rig(pose, fig.slim);
    for (hand, stack) in [(Hand::Main, &wearer.main), (Hand::Off, &wearer.off)] {
        let Some(stack) = stack.clone() else {
            continue;
        };
        let left = (hand == Hand::Off) != wearer.left_handed;
        let up = wearer.shield_up == Some(hand);
        let geometry = held.get(&icon(&stack, hand), fig.atlas);
        let display = third_person_display(fig, &stack, left, up);
        let matrix = frame * hand_mount(&rig, !left, fig.slim) * display_matrix(&display, left);
        let light = crate::render::hand::glinted(light, stack.is_enchanted());
        geometry.add_to(&mut meshes.items, &matrix, [1.0; 3], light);
    }
}

/// Undyed leather armor's color (vanilla's `DyedItemColor.LEATHER_COLOR`).
pub const LEATHER_COLOR: u32 = 0xA0_6540;

/// What a player holds and wears and how they're drawn: what the figure
/// is made from, for the local player and for others (sent in snapshots).
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Wearer {
    pub main: Option<ItemStack>,
    pub off: Option<ItemStack>,
    /// Head, chest, legs, feet.
    pub armor: [Option<ItemStack>; 4],
    pub layers: crate::render::player::OuterLayers,
    pub left_handed: bool,
    /// Ticks a bow in the main hand has been drawn.
    pub bow_ticks: Option<f32>,
    /// The hand holding up a shield.
    pub shield_up: Option<Hand>,
}

impl Game {
    /// What `self.player` holds and wears, for drawing them.
    pub fn wearer(&self) -> Wearer {
        use crate::inventory::player::{ARMOR_CHEST, ARMOR_FEET, ARMOR_HEAD, ARMOR_LEGS};
        let inventory = &self.player.inventory;
        Wearer {
            main: inventory.hand_stack(Hand::Main).cloned(),
            off: inventory.hand_stack(Hand::Off).cloned(),
            armor: [ARMOR_HEAD, ARMOR_CHEST, ARMOR_LEGS, ARMOR_FEET]
                .map(|slot| inventory.slot(slot).cloned()),
            layers: self.skin_layers(),
            left_handed: self.left_handed(),
            bow_ticks: self.bow_ticks(),
            shield_up: self.shield_hand().filter(|_| self.is_blocking()),
        }
    }
}

/// The armor in `armor` (head, chest, legs, feet) as drawn: leather twice
/// (tinted, then its untinted overlay), the elytra not at all (it has a
/// model of its own).
pub fn worn_armor(armor: &[Option<ItemStack>; 4]) -> Vec<WornArmor> {
    use crate::inventory::armor::{ArmorSlot, armor_of};
    let mut worn = Vec::new();
    for (slot, piece) in [
        (ArmorSlot::Feet, &armor[3]),
        (ArmorSlot::Legs, &armor[2]),
        (ArmorSlot::Chest, &armor[1]),
        (ArmorSlot::Head, &armor[0]),
    ] {
        let Some(stack) = piece else {
            continue;
        };
        let name = stack.id.name();
        let Some(material) = armor_material(name) else {
            continue;
        };
        if armor_of(name).is_some_and(|armor| armor.glider) {
            continue;
        }
        let layer = usize::from(slot == ArmorSlot::Legs);
        let glint = stack.is_enchanted();
        let row = |material| crate::look::armor_row(material, layer);
        if material == "leather" {
            let c = LEATHER_COLOR;
            let tint = [(c >> 16) & 0xFF, (c >> 8) & 0xFF, c & 0xFF].map(|v| v as f32 / 255.0);
            worn.extend(row("leather").map(|row| WornArmor {
                slot,
                row,
                tint,
                glint,
            }));
            worn.extend(row("leather_overlay").map(|row| WornArmor {
                slot,
                row,
                tint: [1.0; 3],
                glint: false,
            }));
        } else {
            worn.extend(row(material).map(|row| WornArmor {
                slot,
                row,
                tint: [1.0; 3],
                glint,
            }));
        }
    }
    worn
}

/// The armor sheet's material for a piece of armor called `name`.
pub fn armor_material(name: &str) -> Option<&'static str> {
    if name == "turtle_helmet" {
        return Some("turtle_scute");
    }
    let (prefix, piece) = name.rsplit_once('_')?;
    if !matches!(piece, "helmet" | "chestplate" | "leggings" | "boots") {
        return None;
    }
    Some(match prefix {
        "leather" => "leather",
        "chainmail" => "chainmail",
        "iron" => "iron",
        "golden" => "gold",
        "diamond" => "diamond",
        "netherite" => "netherite",
        "copper" => "copper",
        _ => return None,
    })
}

/// Adds what mobs hold in their main hands to `mesh` (textured with the
/// atlas), for those `draw` approves: a skeleton's bow, drawn back while it
/// draws, a piglin's sword.
pub fn add_mob_items(
    mesh: &mut Mesh,
    game: &Game,
    held: &mut HeldCache,
    fig: &Figure,
    alpha: f32,
    draw: impl Fn(&crate::entity::mob::Mob) -> bool,
    light_at: impl Fn(glam::DVec3) -> (u8, u8),
) {
    let defs = game.mob_defs();
    for mob in game.mobs.iter().filter(|mob| draw(mob)) {
        let Some(id) = mob.data.held else {
            continue;
        };
        if mob.is_gone() {
            continue;
        }
        let def = defs.get(mob.kind);
        let Some(mount) = crate::render::models::hand_mount(mob, def, alpha) else {
            continue;
        };
        let stack = ItemStack::new(id, 1);
        let icon = match mob.ai.draw {
            0 => fig.icons.icon(&stack),
            ticks => fig.icons.icon_drawn(&stack, ticks as f32),
        };
        let geometry = held.get(&icon, fig.atlas);
        let display = third_person_display(fig, &stack, false, false);
        let matrix = mount * display_matrix(&display, false);
        geometry.add_to(mesh, &matrix, [1.0; 3], light_at(mob.eye()));
    }
}

/// A shield's own display transforms: it is held as a plate, flat to the
/// front, lowered at rest and raised in front of the player blocking.
/// (The inventory picture is extruded, so these stand in for the shield
/// model's.)
fn shield_display(first_person: bool, up: bool) -> Transform {
    // Translations move toward the middle of the screen with a negative
    // x in either hand, since a hand mirrors them.
    match (first_person, up) {
        (true, false) => Transform {
            rotation: [0.0, -20.0, 0.0],
            translation: [-1.1, 3.8, 0.0],
            scale: [0.35; 3],
        },
        (true, true) => Transform {
            rotation: [0.0, -8.0, 0.0],
            translation: [-4.2, 7.2, 0.0],
            scale: [0.45; 3],
        },
        // Hanging at the side, facing outward.
        (false, false) => Transform {
            rotation: [-90.0, 90.0, -90.0],
            translation: [0.0; 3],
            scale: [0.6; 3],
        },
        // Held up in front of the chest, facing forward.
        (false, true) => Transform {
            rotation: [45.0, -45.0, -90.0],
            translation: [0.0; 3],
            scale: [0.6; 3],
        },
    }
}

/// The display transform of `stack` in a first-person hand.
fn first_person_display(fig: &Figure, stack: &ItemStack, left: bool, up: bool) -> Transform {
    if stack.id == items::SHIELD {
        return shield_display(true, up);
    }
    let context = if left {
        DisplayContext::FirstPersonLeftHand
    } else {
        DisplayContext::FirstPersonRightHand
    };
    display_of(fig, stack, context)
}

/// The display transform of `stack` held in a hand of the third-person model.
fn third_person_display(fig: &Figure, stack: &ItemStack, left: bool, up: bool) -> Transform {
    if stack.id == items::SHIELD {
        return shield_display(false, up);
    }
    let context = if left {
        DisplayContext::ThirdPersonLeftHand
    } else {
        DisplayContext::ThirdPersonRightHand
    };
    display_of(fig, stack, context)
}

/// The item's display transform for `context`, with the built-in defaults
/// when no model gives one.
fn display_of(
    fig: &Figure,
    stack: &ItemStack,
    context: DisplayContext,
) -> crate::pack::model::Transform {
    item::registry()
        .get(stack.id)
        .and_then(|def| fig.item_models.display(&def.name))
        .map(|display| display.get(context))
        .unwrap_or(crate::pack::model::Transform::IDENTITY)
}

#[cfg(test)]
mod tests {
    use glam::Vec3;

    use super::*;
    use crate::{
        game::{tests::flat_game, view::ViewMode},
        inventory::items,
        look::Look,
        player::mode::GameMode,
    };

    fn look() -> Look {
        let mobs =
            crate::entity::mobs::MobRegistry::load(&crate::assets::dir().join("data/mobs.ron"))
                .unwrap();
        Look::build(&crate::assets::dir(), Vec::new(), &mobs).unwrap()
    }

    fn bounds(mesh: &Mesh) -> (Vec3, Vec3) {
        let points = mesh.vertices.iter().map(|v| Vec3::from(v.position));
        let min = points.clone().fold(Vec3::splat(f32::MAX), Vec3::min);
        let max = points.fold(Vec3::splat(f32::MIN), Vec3::max);
        (min, max)
    }

    #[test]
    fn first_person_draws_the_arm_or_the_item_and_third_person_the_model() {
        let look = look();
        let fig = Figure {
            icons: &look.icons,
            item_models: &look.item_models,
            atlas: look.atlas.image(),
            slim: false,
        };
        let mut held = HeldCache::default();
        let mut game = flat_game(GameMode::Survival);
        // Empty-handed in first person: just the arm.
        let m = build(&game, &mut held, &fig, (15, 0), (15, 0));
        assert!(!m.hand_arm.is_empty() && m.hand_item.is_empty());
        assert!(m.body.is_empty() && m.items.is_empty());

        // With a sword: the item and no arm.
        game.player
            .inventory
            .set(0, Some(ItemStack::new(items::IRON_SWORD, 1)));
        game.player.inventory.selected = 0;
        game.run_ticks(10);
        let m = build(&game, &mut held, &fig, (15, 0), (15, 0));
        assert!(m.hand_arm.is_empty() && !m.hand_item.is_empty());

        // Third person: the model and, in its hand, the sword; no hand.
        game.cycle_view();
        assert_eq!(game.view().mode, ViewMode::ThirdBack);
        let m = build(&game, &mut held, &fig, (15, 0), (15, 0));
        assert!(m.hand_arm.is_empty() && m.hand_item.is_empty());
        assert_eq!(m.body.indices.len() / 6, 12 * 6);
        assert!(!m.items.is_empty());
        // The sword is in the right hand, beside the body, at about hip height.
        let (feet, _) = game.player_body();
        let (min, max) = bounds(&m.items);
        let middle = (min + max) / 2.0 - feet;
        assert!(middle.y > 0.4 && middle.y < 1.6, "{middle}");
        assert!(
            middle.x > 0.1,
            "on the right, east when facing north: {middle}"
        );
    }

    /// A game with a shield in the off-hand, blocking or not.
    fn shield_game(blocking: bool) -> Game {
        let mut game = flat_game(GameMode::Survival);
        game.player.inventory.set(
            crate::inventory::player::OFFHAND,
            Some(ItemStack::new(items::SHIELD, 1)),
        );
        game.player.camera.pitch = 0.0;
        game.run_ticks(12);
        if blocking {
            game.on_action(crate::bindings::Action::Use, true, false, true);
            game.run_ticks(8);
            assert!(game.is_blocking());
        }
        game
    }

    fn figure<'a>(look: &'a Look) -> Figure<'a> {
        Figure {
            icons: &look.icons,
            item_models: &look.item_models,
            atlas: look.atlas.image(),
            slim: false,
        }
    }

    /// The hand item's extent on screen, in clip coordinates.
    fn screen_box(mesh: &Mesh) -> (glam::Vec2, glam::Vec2) {
        let project = |p: Vec3| {
            let c = crate::render::hand::hand_projection(16.0 / 9.0) * p.extend(1.0);
            (c.truncate() / c.w).truncate()
        };
        let points: Vec<_> = mesh
            .vertices
            .iter()
            .map(|v| project(Vec3::from(v.position)))
            .collect();
        (
            points.iter().fold(glam::Vec2::splat(9.0), |a, &b| a.min(b)),
            points
                .iter()
                .fold(glam::Vec2::splat(-9.0), |a, &b| a.max(b)),
        )
    }

    #[test]
    fn the_off_hand_shield_sits_low_on_the_left_and_comes_up_to_block() {
        let look = look();
        let fig = figure(&look);
        let mut held = HeldCache::default();
        let resting = shield_game(false);
        let m = build(&resting, &mut held, &fig, (15, 0), (15, 0));
        assert!(!m.hand_item.is_empty(), "the shield shows in the off-hand");
        let (low, high) = screen_box(&m.hand_item);
        // Low in the left corner, about a third of the screen tall.
        assert!(high.x < -0.2 && low.x > -1.0, "{low} {high}");
        assert!(high.y < 0.0 && low.y > -1.2, "{low} {high}");
        assert!((0.6..1.0).contains(&(high.y - low.y)), "{}", high.y - low.y);

        let up = shield_game(true);
        let m = build(&up, &mut held, &fig, (15, 0), (15, 0));
        let (low, high) = screen_box(&m.hand_item);
        // Raised toward the middle, over half the screen tall.
        assert!(low.x > -1.0 && high.x < 0.3, "{low} {high}");
        assert!(high.y > 0.2 && low.y < -0.3, "{low} {high}");
        assert!(high.y - low.y > 0.9, "{}", high.y - low.y);
    }

    #[test]
    fn in_third_person_the_shield_hangs_at_the_left_side_and_rises_in_front() {
        let look = look();
        let fig = figure(&look);
        let mut held = HeldCache::default();
        for (blocking, check) in [
            (
                false,
                (|min: Vec3, max: Vec3| {
                    // A plate on the left (west) side, edge-on from the front.
                    assert!(max.x < -0.25 && max.x - min.x < 0.2, "{min} {max}");
                    assert!(min.y > 0.3 && max.y < 1.3, "{min} {max}");
                }) as fn(Vec3, Vec3),
            ),
            (
                true,
                (|min: Vec3, max: Vec3| {
                    // In front of the chest, facing forward.
                    assert!(max.z < 0.0 && min.y > 0.5 && max.y < 1.6, "{min} {max}");
                    assert!(max.x - min.x > 0.3, "{min} {max}");
                }) as fn(Vec3, Vec3),
            ),
        ] {
            let mut game = shield_game(blocking);
            game.set_view_mode(crate::game::view::ViewMode::ThirdBack);
            let m = build(&game, &mut held, &fig, (15, 0), (15, 0));
            let (feet, _) = game.player_body();
            let (min, max) = bounds(&m.items);
            check(min - feet, max - feet);
        }
    }

    #[test]
    fn a_shield_in_the_main_hand_mirrors_to_the_right() {
        let look = look();
        let fig = figure(&look);
        let mut held = HeldCache::default();
        let mut game = shield_game(false);
        game.player.inventory.selected = 0;
        game.swap_hands();
        game.set_view_mode(crate::game::view::ViewMode::ThirdBack);
        let m = build(&game, &mut held, &fig, (15, 0), (15, 0));
        let (feet, _) = game.player_body();
        let (min, max) = bounds(&m.items);
        assert!(min.x - feet.x > 0.25, "{min} {max}");
    }

    #[test]
    fn a_drawn_bow_changes_its_picture() {
        let look = look();
        let stack = ItemStack::new(items::BOW, 1);
        let rest = look.icons.icon(&stack);
        let drawn: Vec<_> = [0.0, 14.0, 19.0]
            .iter()
            .map(|&ticks| look.icons.icon_drawn(&stack, ticks))
            .collect();
        assert_ne!(drawn[0], drawn[1]);
        assert_ne!(drawn[1], drawn[2]);
        assert_ne!(rest, drawn[2]);
        // Other items don't change.
        let sword = ItemStack::new(items::IRON_SWORD, 1);
        assert_eq!(look.icons.icon_drawn(&sword, 19.0), look.icons.icon(&sword));
    }

    #[test]
    fn nothing_of_the_player_shows_when_dead() {
        let look = look();
        let fig = Figure {
            icons: &look.icons,
            item_models: &look.item_models,
            atlas: look.atlas.image(),
            slim: true,
        };
        let mut held = HeldCache::default();
        let mut game = flat_game(GameMode::Survival);
        game.player.stats.health = 0.0;
        let m = build(&game, &mut held, &fig, (15, 0), (15, 0));
        assert!(m.hand_arm.is_empty() && m.body.is_empty());
    }

    #[test]
    fn a_skeleton_spawns_with_a_bow_that_is_drawn_in_its_hand() {
        let look = look();
        let fig = Figure {
            icons: &look.icons,
            item_models: &look.item_models,
            atlas: look.atlas.image(),
            slim: false,
        };
        let mut held = HeldCache::default();
        let mut game = crate::game::tests::lit_flat_game(GameMode::Creative);
        let kind = game.mob_defs().find("skeleton").unwrap();
        let at = glam::DVec3::new(0.5, 64.0, 5.5);
        let mob = crate::entity::mob::Mob::new(kind, game.mob_defs().get(kind), at, 0.0);
        assert_eq!(mob.data.held, Some(items::BOW), "a new skeleton has a bow");
        game.mobs.push(mob);
        let mut mesh = Mesh::default();
        add_mob_items(
            &mut mesh,
            &game,
            &mut held,
            &fig,
            1.0,
            |_| true,
            |_| (15, 0),
        );
        assert!(!mesh.is_empty(), "the bow is drawn");
        // Within arm's reach of the skeleton, at about waist height.
        let (min, max) = bounds(&mesh);
        let middle = (min + max) / 2.0 - at.as_vec3();
        assert!(middle.length() < 1.2, "{middle}");
        assert!(middle.y > 0.3 && middle.y < 1.7, "{middle}");

        // Without it, nothing is drawn; with a sword it holds that.
        game.mobs[0].data.held = None;
        let mut mesh = Mesh::default();
        add_mob_items(
            &mut mesh,
            &game,
            &mut held,
            &fig,
            1.0,
            |_| true,
            |_| (15, 0),
        );
        assert!(mesh.is_empty());
        game.mobs[0].data.held = Some(items::STONE_SWORD);
        add_mob_items(
            &mut mesh,
            &game,
            &mut held,
            &fig,
            1.0,
            |_| true,
            |_| (15, 0),
        );
        assert!(!mesh.is_empty());
    }

    #[test]
    fn worn_armor_picks_its_rows_tints_leather_and_glints_when_enchanted() {
        use crate::inventory::{
            armor::ArmorSlot,
            enchant::Enchantment,
            player::{ARMOR_CHEST, ARMOR_HEAD, ARMOR_LEGS},
        };
        let mut game = flat_game(GameMode::Survival);
        let mut cap = ItemStack::new(items::LEATHER_HELMET, 1);
        cap.enchant(Enchantment::Protection, 1);
        game.player.inventory.set(ARMOR_HEAD, Some(cap));
        game.player
            .inventory
            .set(ARMOR_LEGS, Some(ItemStack::new(items::IRON_LEGGINGS, 1)));
        game.player
            .inventory
            .set(ARMOR_CHEST, Some(ItemStack::new(items::ELYTRA, 1)));
        let worn = worn_armor(&game.wearer().armor);
        assert_eq!(worn.len(), 3, "{worn:?}");
        let legs = worn.iter().find(|w| w.slot == ArmorSlot::Legs).unwrap();
        assert_eq!(Some(legs.row), crate::look::armor_row("iron", 1));
        let caps: Vec<_> = worn.iter().filter(|w| w.slot == ArmorSlot::Head).collect();
        assert_eq!(caps[0].tint, [160.0 / 255.0, 101.0 / 255.0, 64.0 / 255.0]);
        assert!(caps[0].glint);
        assert_eq!(caps[1].tint, [1.0; 3], "the overlay isn't dyed");
        assert_eq!(armor_material("golden_boots"), Some("gold"));
        assert_eq!(armor_material("turtle_helmet"), Some("turtle_scute"));
        assert_eq!(armor_material("iron_sword"), None);
    }
}
