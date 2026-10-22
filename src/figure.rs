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
        player::{add_player, body_frame, hand_mount, model_matrix, rig},
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
    if game.stats.is_dead() || game.is_sleeping() {
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
                    game.inventory
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
                display: first_person_display(fig, stack, false, *up),
                glint: stack.is_enchanted(),
            });
            let input = HandInput {
                left: false,
                swing: view.attack_anim(alpha),
                equip: view.equip_progress(alpha),
                lag: view.hand_lag(alpha, game.camera.yaw, game.camera.pitch),
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
                    left: true,
                    swing: 0.0,
                    equip: view.offhand_equip_progress(alpha),
                    using: crate::game::view::HandUse::None,
                    item: Some(HandItem {
                        geometry,
                        display: first_person_display(fig, stack, true, *up),
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
            add_body(&mut meshes, game, held, fig, &pose, feet, yaw, body_light);
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
    game: &Game,
    held: &mut HeldCache,
    fig: &Figure,
    pose: &crate::render::player::PlayerPose,
    feet: glam::Vec3,
    yaw: f32,
    light: (u8, u8),
) {
    let icons = fig.icons;
    let pull = game.bow_ticks();
    // The icon a stack shows: a bow in the main hand looks drawn while it is.
    let icon = |stack: &ItemStack, hand: Hand| match (pull, hand) {
        (Some(ticks), Hand::Main) => icons.icon_drawn(stack, ticks),
        _ => icons.icon(stack),
    };
    let blocking = game.is_blocking();
    add_player(&mut meshes.body, pose, fig.slim, feet, yaw, [1.0; 3], light);
    if game
        .inventory
        .slot(crate::inventory::player::ARMOR_CHEST)
        .is_some_and(|s| s.id == items::ELYTRA)
    {
        crate::render::player::add_elytra(&mut meshes.elytra, pose, feet, yaw, light);
    }
    let frame = model_matrix(&body_frame(pose, yaw), feet);
    let rig = rig(pose, fig.slim);
    for hand in [Hand::Main, Hand::Off] {
        let Some(stack) = game.inventory.hand_stack(hand).cloned() else {
            continue;
        };
        let left = hand == Hand::Off;
        let up = blocking && game.shield_hand() == Some(hand);
        let geometry = held.get(&icon(&stack, hand), fig.atlas);
        let display = third_person_display(fig, &stack, left, up);
        let matrix = frame * hand_mount(&rig, !left, fig.slim) * display_matrix(&display, left);
        let light = crate::render::hand::glinted(light, stack.is_enchanted());
        geometry.add_to(&mut meshes.items, &matrix, [1.0; 3], light);
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
        game.inventory
            .set(0, Some(ItemStack::new(items::IRON_SWORD, 1)));
        game.inventory.selected = 0;
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
        game.inventory.set(
            crate::inventory::player::OFFHAND,
            Some(ItemStack::new(items::SHIELD, 1)),
        );
        game.camera.pitch = 0.0;
        game.run_ticks(12);
        if blocking {
            game.on_mouse_button(winit::event::MouseButton::Right, true);
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
        game.inventory.selected = 0;
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
        game.stats.health = 0.0;
        let m = build(&game, &mut held, &fig, (15, 0), (15, 0));
        assert!(m.hand_arm.is_empty() && m.body.is_empty());
    }
}
