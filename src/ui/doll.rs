//! The player in the inventory's black box, as 1.0.0 and later draw it: the
//! model with its skin and what it holds, standing on the floor of the
//! box and turning its head (and, half as much, its body) toward the
//! mouse. The model's mesh is the same one the F5 view draws; here it is
//! projected flat by hand and painted as egui triangles.

use egui::{Color32, Pos2, Rect, Vec2, pos2};
use glam::Vec3;

use crate::{
    figure::{self, Figure, PlayerMeshes},
    game::Game,
    inventory::player::Hand,
    render::{
        held::HeldCache,
        player::{ArmPose, PlayerPose},
    },
    world::mesh::Mesh,
};

/// Where the player's picture goes on a screen, in GUI pixels from the
/// panel's corner.
#[derive(Debug, Clone, Copy)]
pub struct Spot {
    /// Where the feet stand.
    pub feet: [f32; 2],
    /// The point the mouse is measured from (vanilla's 30 above the feet).
    pub head: [f32; 2],
    /// The black box the picture is clipped to: top-left and size.
    pub clip: ([f32; 2], [f32; 2]),
    /// GUI pixels per block of height: vanilla's `scale`.
    pub per_block: f32,
}

/// The survival inventory's: `drawEntityOnScreen(51, 75, 30, ...)`, which
/// makes the player (1.875 blocks, 1.9 with the hat) about 57 pixels tall
/// in the 70-pixel box.
pub const SURVIVAL: Spot = Spot {
    feet: [51.0, 75.0],
    head: [51.0, 25.0],
    clip: ([26.0, 8.0], [52.0, 70.0]),
    per_block: 30.0,
};

/// How the figure is turned, in radians.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct DollAngles {
    /// The whole body's turn, positive toward east (the viewer's left).
    pub body_yaw: f32,
    /// The head's turn from the body, the same way (as much again).
    pub head_yaw: f32,
    /// The head's pitch, positive looking down.
    pub head_pitch: f32,
}

/// Vanilla's `renderEntityInInventoryFollowsMouse`: `offset` is the mouse
/// minus the head's position in GUI pixels. The head turns twice as far
/// as the body, both by `atan(offset / 40)`, and the head tips up to 20°.
/// The head's turn from the body is held to `HEAD_TURN_LIMIT`, past which
/// the body turns instead.
pub fn angles(offset: Vec2) -> DollAngles {
    let (f, g) = ((offset.x / 40.0).atan(), (offset.y / 40.0).atan());
    // Mouse to the right is the viewer's right: the player's west.
    let turn = -f * 20.0_f32.to_radians();
    let (body_yaw, head_yaw) = follow(turn, turn);
    DollAngles {
        body_yaw,
        head_yaw,
        head_pitch: g * 20.0_f32.to_radians(),
    }
}

/// How far the head may turn from the body (vanilla's body follows the
/// head past 50°).
pub const HEAD_TURN_LIMIT: f32 = 50.0 * std::f32::consts::PI / 180.0;

/// `body_yaw` and the head's turn from it, with the turn held within the
/// limit and the rest given to the body.
pub fn follow(body_yaw: f32, head_yaw: f32) -> (f32, f32) {
    let held = head_yaw.clamp(-HEAD_TURN_LIMIT, HEAD_TURN_LIMIT);
    (body_yaw + (head_yaw - held), held)
}

/// The model to draw: the skin-textured body, the elytra, the armor (and
/// its enchanted pieces again for the glint) and the atlas-textured items
/// it holds.
#[derive(Debug, Default)]
pub struct Doll {
    pub body: Mesh,
    pub elytra: Mesh,
    pub armor: Mesh,
    pub armor_glint: Mesh,
    pub items: Mesh,
}

/// Poses the player standing still, turned by `angles`, with its hands as
/// they are in the game.
pub fn build(game: &Game, held: &mut HeldCache, fig: &Figure, angles: DollAngles) -> Doll {
    let arm = |hand: Hand| {
        if game.player.inventory.hand_stack(hand).is_some() {
            ArmPose::Item
        } else {
            ArmPose::Empty
        }
    };
    let pose = PlayerPose {
        head_yaw: angles.head_yaw,
        head_pitch: angles.head_pitch,
        right: arm(Hand::Main),
        left: arm(Hand::Off),
        ..PlayerPose::default()
    };
    let mut meshes = PlayerMeshes::default();
    figure::add_body(
        &mut meshes,
        &game.wearer(),
        held,
        fig,
        &pose,
        Vec3::ZERO,
        angles.body_yaw,
        (15, 0),
    );
    Doll {
        body: meshes.body,
        elytra: meshes.elytra,
        armor: meshes.armor,
        armor_glint: meshes.armor_glint,
        items: meshes.items,
    }
}

/// One corner of a projected triangle: where, its texture coordinate and color.
type Corner = (Pos2, Pos2, Color32);

/// A corner before projection: the model point (blocks), its texture
/// coordinate and color.
type ModelCorner = (Vec3, Pos2, [f32; 3]);

/// Triangles longer than this (in blocks, about a model pixel and a half)
/// are split before sorting: sorted far to near by their middles, big
/// faces that meet at an angle (a turned head over the shoulders) would
/// paint over each other, small ones come out as a depth buffer would.
const MAX_EDGE: f32 = 0.09;

/// Splits a triangle into four at its edges' middles until every edge is
/// at most `MAX_EDGE` long.
fn split(t: [ModelCorner; 3], out: &mut Vec<[ModelCorner; 3]>) {
    let long = (0..3).any(|i| t[i].0.distance(t[(i + 1) % 3].0) > MAX_EDGE);
    if !long {
        out.push(t);
        return;
    }
    let mid = |a: ModelCorner, b: ModelCorner| -> ModelCorner {
        (
            (a.0 + b.0) / 2.0,
            pos2((a.1.x + b.1.x) / 2.0, (a.1.y + b.1.y) / 2.0),
            std::array::from_fn(|c| (a.2[c] + b.2[c]) / 2.0),
        )
    };
    let [a, b, c] = t;
    let (ab, bc, ca) = (mid(a, b), mid(b, c), mid(c, a));
    for part in [[a, ab, ca], [ab, b, bc], [ca, bc, c], [ab, bc, ca]] {
        split(part, out);
    }
}

/// `mesh` seen from the front (the model faces north, the viewer stands
/// north of it), its feet at `feet` and `per_block` points to a block,
/// back faces dropped and the rest split small and sorted far to near.
pub fn project(mesh: &Mesh, texture: egui::TextureId, feet: Pos2, per_block: f32) -> egui::Mesh {
    let screen = |p: Vec3| pos2(feet.x - p.x * per_block, feet.y - p.y * per_block);
    let corner = |i: u32| -> ModelCorner {
        let v = &mesh.vertices[i as usize];
        (Vec3::from(v.position), pos2(v.uv[0], v.uv[1]), v.color)
    };
    let mut small = Vec::new();
    for &[a, b, c] in mesh.indices.as_chunks::<3>().0 {
        let t = [a, b, c].map(corner);
        let [pa, pb, pc] = t.map(|corner| screen(corner.0));
        // Seen from the front the mesh's triangles wind this way on a
        // screen whose y points down; the others face away.
        let area = (pb - pa).x * (pc - pa).y - (pb - pa).y * (pc - pa).x;
        if area >= 0.0 {
            continue;
        }
        split(t, &mut small);
    }
    let mut faces: Vec<(f32, [Corner; 3])> = small
        .into_iter()
        .map(|t| {
            let depth = (t[0].0.z + t[1].0.z + t[2].0.z) / 3.0;
            let color = |c: [f32; 3]| {
                let [r, g, b] = c.map(|c| (c.clamp(0.0, 1.0) * 255.0).round() as u8);
                Color32::from_rgb(r, g, b)
            };
            (depth, t.map(|(p, uv, c)| (screen(p), uv, color(c))))
        })
        .collect();
    // Far first: the viewer is on the -z side.
    faces.sort_by(|a, b| b.0.total_cmp(&a.0));
    let mut out = egui::Mesh::with_texture(texture);
    for (_, corners) in faces {
        let base = out.vertices.len() as u32;
        for (pos, uv, color) in corners {
            out.vertices.push(egui::epaint::Vertex { pos, uv, color });
        }
        out.indices.extend([base, base + 1, base + 2]);
    }
    out
}

/// The rectangle the doll is clipped to, on a panel at `origin`.
pub fn clip(origin: Pos2, scale: f32, spot: &Spot) -> Rect {
    Rect::from_min_size(
        origin + Vec2::from(spot.clip.0) * scale,
        Vec2::from(spot.clip.1) * scale,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        game::tests::flat_game,
        inventory::{items, stack::ItemStack},
        look::Look,
        player::mode::GameMode,
    };
    use egui::vec2;

    /// Projected triangles whose texture coordinates all lie in the skin
    /// rectangle `[u, v, w, h]` (pixels of the 64 × 64 sheet).
    fn faces_in(mesh: &egui::Mesh, [u, v, w, h]: [f32; 4]) -> Vec<[Pos2; 3]> {
        let inside = |uv: Pos2| {
            let (x, y) = (uv.x * 64.0, uv.y * 64.0);
            x >= u - 0.01 && x <= u + w + 0.01 && y >= v - 0.01 && y <= v + h + 0.01
        };
        mesh.indices
            .as_chunks::<3>()
            .0
            .iter()
            .map(|t| t.map(|i| mesh.vertices[i as usize]))
            .filter(|t| t.iter().all(|vertex| inside(vertex.uv)))
            .map(|t| t.map(|vertex| vertex.pos))
            .collect()
    }

    fn centroid(faces: &[[Pos2; 3]]) -> Pos2 {
        let points: Vec<Pos2> = faces.iter().flatten().copied().collect();
        let sum = points.iter().fold(Vec2::ZERO, |sum, p| sum + p.to_vec2());
        (sum / points.len() as f32).to_pos2()
    }

    /// The doll for a game holding a sword, projected onto the box.
    fn drawn(angles: DollAngles) -> (egui::Mesh, egui::Mesh) {
        let mobs =
            crate::entity::mobs::MobRegistry::load(&crate::assets::dir().join("data/mobs.ron"))
                .unwrap();
        let look = Look::build(&crate::assets::dir(), Vec::new(), &mobs).unwrap();
        let fig = Figure {
            icons: &look.icons,
            item_models: &look.item_models,
            atlas: look.atlas.image(),
            slim: false,
        };
        let mut game = flat_game(GameMode::Survival);
        game.player
            .inventory
            .set(0, Some(ItemStack::new(items::IRON_SWORD, 1)));
        game.player.inventory.selected = 0;
        let doll = build(&game, &mut HeldCache::default(), &fig, angles);
        let feet = pos2(SURVIVAL.feet[0] * 3.0, SURVIVAL.feet[1] * 3.0);
        let id = egui::TextureId::default();
        (
            project(&doll.body, id, feet, SURVIVAL.per_block * 3.0),
            project(&doll.items, id, feet, SURVIVAL.per_block * 3.0),
        )
    }

    #[test]
    fn steve_faces_the_viewer_with_his_feet_on_the_floor_of_the_box() {
        let (body, items) = drawn(DollAngles::default());
        // The front of the head shows and the back of it doesn't.
        assert!(!faces_in(&body, [8.0, 8.0, 8.0, 8.0]).is_empty(), "face");
        assert!(faces_in(&body, [24.0, 8.0, 8.0, 8.0]).is_empty(), "back");
        // Standing: the bottoms of the legs are at the feet and the top of
        // the hat 1.9 blocks (about 57 GUI pixels) above them, inside the box.
        let ys: Vec<f32> = body.vertices.iter().map(|v| v.pos.y / 3.0).collect();
        let (top, bottom) = (
            ys.iter().copied().fold(f32::MAX, f32::min),
            ys.iter().copied().fold(f32::MIN, f32::max),
        );
        assert!((bottom - 75.0).abs() < 0.5, "feet at {bottom}");
        assert!((bottom - top - 57.0).abs() < 1.5, "{} tall", bottom - top);
        let (box_at, box_size) = SURVIVAL.clip;
        assert!(top >= box_at[1] && bottom <= box_at[1] + box_size[1]);
        // The sword is in the picture, beside the body.
        assert!(!items.vertices.is_empty());
    }

    #[test]
    fn the_head_follows_the_mouse_and_the_body_turns_half_as_much() {
        let head = |offset: Vec2| {
            let (body, _) = drawn(angles(offset));
            centroid(&faces_in(&body, [8.0, 8.0, 8.0, 8.0]))
        };
        let torso = |offset: Vec2| {
            let (body, _) = drawn(angles(offset));
            centroid(&faces_in(&body, [20.0, 20.0, 8.0, 12.0]))
        };
        let (still, right, left) = (
            head(Vec2::ZERO),
            head(vec2(200.0, 0.0)),
            head(vec2(-200.0, 0.0)),
        );
        assert!(right.x > still.x + 1.0, "toward a mouse on the right");
        assert!(left.x < still.x - 1.0, "toward a mouse on the left");
        let (up, down) = (head(vec2(0.0, -200.0)), head(vec2(0.0, 200.0)));
        assert!(up.y < still.y - 0.5, "the face lifts for a mouse above");
        assert!(down.y > still.y + 0.5, "and drops for one below");
        // The front of the torso shifts too, but less than the head.
        let body_shift = torso(vec2(200.0, 0.0)).x - torso(Vec2::ZERO).x;
        let head_shift = right.x - still.x;
        assert!(
            body_shift > 0.0 && body_shift < head_shift,
            "{body_shift} {head_shift}"
        );
        // Vanilla's angles: the head is turned 20° from the body at most 31°.
        let far = angles(vec2(1.0e6, 0.0));
        assert!((far.body_yaw.abs().to_degrees() - 31.4).abs() < 0.2);
        assert_eq!(far.body_yaw, far.head_yaw);
    }

    #[test]
    fn faces_are_split_small_and_the_head_never_turns_past_the_limit() {
        let (body, _) = drawn(angles(vec2(200.0, 100.0)));
        let longest = body
            .indices
            .as_chunks::<3>()
            .0
            .iter()
            .flat_map(|t| {
                let p = t.map(|i| body.vertices[i as usize].pos);
                [
                    p[0].distance(p[1]),
                    p[1].distance(p[2]),
                    p[2].distance(p[0]),
                ]
            })
            .fold(0.0, f32::max);
        // Points: three times a block of 30 GUI pixels.
        assert!(longest <= MAX_EDGE * 90.0 + 0.01, "{longest}");
        assert_eq!(follow(0.2, 0.3), (0.2, 0.3));
        let (body_yaw, head) = follow(0.1, 1.2);
        assert!((head - HEAD_TURN_LIMIT).abs() < 1e-6);
        assert!(
            (body_yaw + head - 1.3).abs() < 1e-6,
            "the body takes the rest"
        );
        let (body_yaw, head) = follow(0.0, -1.0);
        assert!((head + HEAD_TURN_LIMIT).abs() < 1e-6 && body_yaw < 0.0);
    }
}
