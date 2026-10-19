//! The player as drawn each frame: in first person the hand, in third
//! person the full model with what it holds. Builds the meshes from the
//! game's view state; the renderer draws them.

use image::RgbaImage;

use crate::{
    game::Game,
    inventory::{item, stack::ItemStack},
    pack::model::{DisplayContext, ItemModels},
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
    match view.mode {
        ViewMode::First => {
            let selected = game.inventory.selected_stack().copied();
            let shown = view.hand_item().map(|id| {
                // The item the hand still shows while it dips can differ
                // from the selected one; it's drawn as a plain stack.
                selected
                    .filter(|stack| stack.id == id)
                    .unwrap_or_else(|| ItemStack::new(id, 1))
            });
            let geometry =
                shown.map(|stack| (held.get(&fig.icons.icon(&stack), fig.atlas).clone(), stack));
            let item = geometry.as_ref().map(|(geometry, stack)| HandItem {
                geometry,
                display: display_of(fig, stack, DisplayContext::FirstPersonRightHand),
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
        }
        ViewMode::ThirdBack | ViewMode::ThirdFront => {
            let pose = game.player_pose();
            let (feet, yaw) = game.player_body();
            add_player(
                &mut meshes.body,
                &pose,
                fig.slim,
                feet,
                yaw,
                [1.0; 3],
                body_light,
            );
            if let Some(stack) = game.inventory.selected_stack().copied() {
                let geometry = held.get(&fig.icons.icon(&stack), fig.atlas);
                let display = display_of(fig, &stack, DisplayContext::ThirdPersonRightHand);
                let matrix = model_matrix(&body_frame(&pose, yaw), feet)
                    * hand_mount(&rig(&pose, fig.slim), true, fig.slim)
                    * display_matrix(&display, false);
                geometry.add_to(&mut meshes.items, &matrix, [1.0; 3], body_light);
            }
        }
    }
    meshes
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
