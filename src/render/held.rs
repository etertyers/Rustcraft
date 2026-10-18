//! Items held in the hand, in first and third person: blocks as small
//! cubes and flat items as sprites with thickness, built from the item
//! icon and placed with the model's `display` transforms (vanilla's
//! `ItemTransform.apply`). Geometry is in the item's own unit cube, with
//! y up, and is moved by the caller's matrix.

use std::collections::HashMap;

use glam::{EulerRot, Mat4, Quat, Vec3};
use image::RgbaImage;

use super::icons::{IconFace, ItemIcon};
use crate::{pack::model::Transform, world::mesh::Mesh};

/// A sprite's thickness: vanilla's generated item models run from z 7.5 to
/// 8.5 of 16.
const FRONT_Z: f32 = 8.5 / 16.0;
const BACK_Z: f32 = 7.5 / 16.0;
/// A pixel with at least this much alpha is solid.
const SOLID: u8 = 128;
/// Vanilla clamps display translations to this many blocks.
const MAX_TRANSLATION: f32 = 5.0;

/// One face of an item's geometry.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HeldQuad {
    /// Counter-clockwise seen from outside, bottom-left first.
    pub corners: [Vec3; 4],
    /// Atlas UV rect for `Mesh::quad`.
    pub uv: [f32; 4],
    /// Tint color, with the face's brightness folded in.
    pub color: [f32; 3],
}

/// An item's geometry.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct HeldGeometry {
    pub quads: Vec<HeldQuad>,
}

/// A rectangle facing `normal` centered on `center`, `half_u` wide either
/// way along `u` and `half_v` along `v` (with `u × v = normal`).
fn face(center: Vec3, u: Vec3, v: Vec3, half_u: f32, half_v: f32) -> [Vec3; 4] {
    let (u, v) = (u * half_u, v * half_v);
    [
        center - u - v,
        center + u - v,
        center + u + v,
        center - u + v,
    ]
}

impl HeldGeometry {
    /// A block icon as a unit cube.
    pub fn cube(top: IconFace, front: IconFace, side: IconFace) -> Self {
        let c = Vec3::splat(0.5);
        let shaded = |face: IconFace, shade: f32| (face.uv, face.color.map(|v| v * shade));
        let mut quads = Vec::with_capacity(6);
        for (normal, u, v, (uv, color)) in [
            (Vec3::Y, Vec3::X, Vec3::NEG_Z, shaded(top, 1.0)),
            (Vec3::NEG_Y, Vec3::X, Vec3::Z, shaded(top, 0.5)),
            (Vec3::Z, Vec3::X, Vec3::Y, shaded(front, 0.8)),
            (Vec3::NEG_Z, Vec3::NEG_X, Vec3::Y, shaded(front, 0.8)),
            (Vec3::X, Vec3::NEG_Z, Vec3::Y, shaded(side, 0.6)),
            (Vec3::NEG_X, Vec3::Z, Vec3::Y, shaded(side, 0.6)),
        ] {
            quads.push(HeldQuad {
                corners: face(c + normal * 0.5, u, v, 0.5, 0.5),
                uv,
                color,
            });
        }
        Self { quads }
    }

    /// A flat item as a sprite one pixel thick: a front and a back, and
    /// an edge wherever a solid pixel meets a see-through one. `tile` is
    /// the sprite's pixels (the part of the atlas under `face.uv`).
    pub fn sprite(face_icon: IconFace, tile: &RgbaImage) -> Self {
        let (w, h) = tile.dimensions();
        let solid = |x: i64, y: i64| {
            x >= 0
                && y >= 0
                && (x as u32) < w
                && (y as u32) < h
                && tile.get_pixel(x as u32, y as u32).0[3] >= SOLID
        };
        let [u0, v0, u1, v1] = face_icon.uv;
        let color = face_icon.color;
        let mut quads = Vec::new();
        let mid = (BACK_Z + FRONT_Z) / 2.0;
        // Front, seen from +z, and back, seen from -z; the back shows the
        // same pixel at the same place, so its u runs the other way.
        quads.push(HeldQuad {
            corners: face(Vec3::new(0.5, 0.5, FRONT_Z), Vec3::X, Vec3::Y, 0.5, 0.5),
            uv: [u0, v0, u1, v1],
            color,
        });
        quads.push(HeldQuad {
            corners: face(Vec3::new(0.5, 0.5, BACK_Z), Vec3::NEG_X, Vec3::Y, 0.5, 0.5),
            uv: [u1, v0, u0, v1],
            color,
        });
        let (pw, ph) = (1.0 / w as f32, 1.0 / h as f32);
        for py in 0..h as i64 {
            for px in 0..w as i64 {
                if !solid(px, py) {
                    continue;
                }
                // A point in the middle of this pixel's texels.
                let uc = u0 + (u1 - u0) * (px as f32 + 0.5) * pw;
                let vc = v0 + (v1 - v0) * (py as f32 + 0.5) * ph;
                let uv = [uc, vc, uc, vc];
                let x = px as f32 * pw + pw / 2.0;
                let y = 1.0 - (py as f32 * ph + ph / 2.0);
                let half = (FRONT_Z - BACK_Z) / 2.0;
                let edges = [
                    // (neighbor open, center, u axis, v axis, half widths)
                    (
                        !solid(px, py - 1),
                        Vec3::new(x, y + ph / 2.0, mid),
                        Vec3::Z,
                        Vec3::X,
                        (half, pw / 2.0),
                    ),
                    (
                        !solid(px, py + 1),
                        Vec3::new(x, y - ph / 2.0, mid),
                        Vec3::X,
                        Vec3::Z,
                        (pw / 2.0, half),
                    ),
                    (
                        !solid(px + 1, py),
                        Vec3::new(x + pw / 2.0, y, mid),
                        Vec3::NEG_Z,
                        Vec3::Y,
                        (half, ph / 2.0),
                    ),
                    (
                        !solid(px - 1, py),
                        Vec3::new(x - pw / 2.0, y, mid),
                        Vec3::Z,
                        Vec3::Y,
                        (half, ph / 2.0),
                    ),
                ];
                for (open, center, u, v, (hu, hv)) in edges {
                    if open {
                        quads.push(HeldQuad {
                            corners: face(center, u, v, hu, hv),
                            uv,
                            color: color.map(|c| c * 0.8),
                        });
                    }
                }
            }
        }
        Self { quads }
    }

    /// Adds the geometry to `mesh`, moved by `matrix`, lit by (sky, block)
    /// `light`.
    pub fn add_to(&self, mesh: &mut Mesh, matrix: &Mat4, tint: [f32; 3], light: (u8, u8)) {
        let light = [light.0 as f32, light.1 as f32];
        for quad in &self.quads {
            let corners = quad.corners.map(|c| matrix.transform_point3(c).to_array());
            let color = [
                quad.color[0] * tint[0],
                quad.color[1] * tint[1],
                quad.color[2] * tint[2],
            ];
            mesh.quad(corners, quad.uv, [color; 4], [light; 4]);
        }
    }
}

/// Builds and remembers items' geometry, by icon.
#[derive(Default)]
pub struct HeldCache {
    map: HashMap<[u32; 8], HeldGeometry>,
}

impl HeldCache {
    /// The geometry for `icon`, whose sprites are read from `atlas`.
    pub fn get(&mut self, icon: &ItemIcon, atlas: &RgbaImage) -> &HeldGeometry {
        let key = match icon {
            ItemIcon::Cube { top, front, side } => {
                let b = |f: &IconFace| f.uv[0].to_bits() ^ f.uv[1].to_bits().rotate_left(7);
                [
                    1,
                    b(top),
                    b(front),
                    b(side),
                    color_bits(top),
                    color_bits(front),
                    color_bits(side),
                    0,
                ]
            }
            ItemIcon::Flat(face) => [
                2,
                face.uv[0].to_bits(),
                face.uv[1].to_bits(),
                face.uv[2].to_bits(),
                face.uv[3].to_bits(),
                color_bits(face),
                0,
                0,
            ],
        };
        self.map.entry(key).or_insert_with(|| match icon {
            ItemIcon::Cube { top, front, side } => HeldGeometry::cube(*top, *front, *side),
            ItemIcon::Flat(face) => HeldGeometry::sprite(*face, &sprite_pixels(atlas, face)),
        })
    }

    /// Forgets everything, after the atlas changes.
    pub fn clear(&mut self) {
        self.map.clear();
    }
}

fn color_bits(face: &IconFace) -> u32 {
    face.color
        .iter()
        .fold(0u32, |acc, c| acc.rotate_left(11) ^ c.to_bits())
}

/// The atlas pixels under a face's UV rect.
fn sprite_pixels(atlas: &RgbaImage, face: &IconFace) -> RgbaImage {
    let (aw, ah) = atlas.dimensions();
    let [u0, v0, u1, v1] = face.uv;
    // UV rects are inset by a hundredth of a texel, so rounding finds the tile.
    let x0 = (u0 * aw as f32).round() as u32;
    let y0 = (v0 * ah as f32).round() as u32;
    let w = (((u1 - u0) * aw as f32).round() as u32).max(1);
    let h = (((v1 - v0) * ah as f32).round() as u32).max(1);
    image::imageops::crop_imm(
        atlas,
        x0.min(aw.saturating_sub(1)),
        y0.min(ah.saturating_sub(1)),
        w.min(aw),
        h.min(ah),
    )
    .to_image()
}

/// Vanilla's `ItemTransform.apply`: move, turn (about x, then y, then z
/// as `rotationXYZ`) and scale the model, then put its center at the
/// origin. A left hand mirrors the translation and turns.
pub fn display_matrix(t: &Transform, left: bool) -> Mat4 {
    let [rx, ry, rz] = t.rotation;
    let (ry, rz) = if left { (-ry, -rz) } else { (ry, rz) };
    let side = if left { -1.0 } else { 1.0 };
    let translation = Vec3::new(side * t.translation[0], t.translation[1], t.translation[2]) / 16.0;
    let translation =
        translation.clamp(Vec3::splat(-MAX_TRANSLATION), Vec3::splat(MAX_TRANSLATION));
    Mat4::from_translation(translation)
        * Mat4::from_quat(Quat::from_euler(
            EulerRot::XYZ,
            rx.to_radians(),
            ry.to_radians(),
            rz.to_radians(),
        ))
        * Mat4::from_scale(Vec3::from(t.scale))
        * Mat4::from_translation(Vec3::splat(-0.5))
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::Rgba;

    fn icon_face() -> IconFace {
        IconFace {
            uv: [0.25, 0.5, 0.5, 0.75],
            color: [1.0; 3],
        }
    }

    /// A 4×4 sprite: a 2×2 solid block in the middle.
    fn blob() -> RgbaImage {
        RgbaImage::from_fn(4, 4, |x, y| {
            if (1..3).contains(&x) && (1..3).contains(&y) {
                Rgba([255, 0, 0, 255])
            } else {
                Rgba([0, 0, 0, 0])
            }
        })
    }

    /// The outward normal of a quad from its corner order.
    fn normal(q: &HeldQuad) -> Vec3 {
        let [a, b, _, d] = q.corners;
        (b - a).cross(d - a).normalize()
    }

    #[test]
    fn a_sprite_has_two_faces_and_edges_around_its_solid_pixels() {
        let g = HeldGeometry::sprite(icon_face(), &blob());
        // Front and back, plus 8 open pixel sides (2 per pixel).
        assert_eq!(g.quads.len(), 2 + 4 * 2);
        let front = &g.quads[0];
        assert!((normal(front) - Vec3::Z).length() < 1e-5);
        assert!((normal(&g.quads[1]) - Vec3::NEG_Z).length() < 1e-5);
        // One pixel thick, 16ths from z 7.5 to 8.5.
        assert!((front.corners[0].z - 8.5 / 16.0).abs() < 1e-6);
        assert!((g.quads[1].corners[0].z - 7.5 / 16.0).abs() < 1e-6);
    }

    #[test]
    fn every_face_points_away_from_the_middle_of_the_item() {
        let center = Vec3::new(0.5, 0.5, 0.5);
        let sprite = HeldGeometry::sprite(icon_face(), &blob());
        let cube = HeldGeometry::cube(icon_face(), icon_face(), icon_face());
        for (name, geometry) in [("sprite", sprite), ("cube", cube)] {
            for quad in &geometry.quads {
                let middle: Vec3 = quad.corners.iter().copied().sum::<Vec3>() / 4.0;
                let out = normal(quad);
                assert!(
                    out.dot(middle - center) > 0.0,
                    "{name}: a face points inward: {quad:?}"
                );
                // Every face is a rectangle: opposite sides are equal.
                let [a, b, c, d] = quad.corners;
                assert!(((b - a) - (c - d)).length() < 1e-5);
            }
        }
    }

    #[test]
    fn a_sprite_edge_samples_its_own_pixel() {
        let g = HeldGeometry::sprite(icon_face(), &blob());
        // The top edge of pixel (1,1) is at y = 1 - 1/4 and takes the
        // texel in the middle of that pixel: u 0.25 + 0.25*(1.5/4).
        let top = g
            .quads
            .iter()
            .find(|q| (normal(q) - Vec3::Y).length() < 1e-5 && q.corners[0].x < 0.5)
            .unwrap();
        assert!((top.corners[0].y - 0.75).abs() < 1e-6);
        assert!((top.uv[0] - (0.25 + 0.25 * 1.5 / 4.0)).abs() < 1e-6);
        assert!((top.uv[1] - (0.5 + 0.25 * 1.5 / 4.0)).abs() < 1e-6);
    }

    #[test]
    fn a_fully_solid_sprite_only_has_edges_around_its_border() {
        let full = RgbaImage::from_pixel(16, 16, Rgba([1, 2, 3, 255]));
        let g = HeldGeometry::sprite(icon_face(), &full);
        // The 64 pixels on the border show one side, the 4 corners two.
        assert_eq!(g.quads.len(), 2 + 16 * 4);
    }

    #[test]
    fn a_cube_uses_the_icons_faces_with_brightness() {
        let top = IconFace {
            uv: [0.0; 4],
            color: [0.5, 1.0, 0.5],
        };
        let side = IconFace {
            uv: [1.0; 4],
            color: [1.0; 3],
        };
        let g = HeldGeometry::cube(top, side, side);
        assert_eq!(g.quads.len(), 6);
        assert_eq!(g.quads[0].color, [0.5, 1.0, 0.5]);
        assert_eq!(g.quads[0].uv, [0.0; 4]);
        assert!((g.quads[4].color[0] - 0.6).abs() < 1e-6);
    }

    #[test]
    fn display_transforms_move_turn_and_scale_about_the_items_center() {
        let t = Transform {
            rotation: [0.0, 90.0, 0.0],
            translation: [4.0, 8.0, -16.0],
            scale: [0.5, 0.5, 0.5],
        };
        let m = display_matrix(&t, false);
        // The item's center lands at the translation.
        let center = m.transform_point3(Vec3::splat(0.5));
        assert!(
            (center - Vec3::new(0.25, 0.5, -1.0)).length() < 1e-5,
            "{center}"
        );
        // A corner is half as far from it, and turned a quarter about y.
        let corner = m.transform_point3(Vec3::new(1.0, 0.5, 0.5)) - center;
        assert!(
            (corner - Vec3::new(0.0, 0.0, -0.25)).length() < 1e-5,
            "{corner}"
        );
        // A left hand mirrors the move and the turn.
        let left = display_matrix(&t, true);
        let center = left.transform_point3(Vec3::splat(0.5));
        assert!((center - Vec3::new(-0.25, 0.5, -1.0)).length() < 1e-5);
        let corner = left.transform_point3(Vec3::new(1.0, 0.5, 0.5)) - center;
        assert!(
            (corner - Vec3::new(0.0, 0.0, 0.25)).length() < 1e-5,
            "{corner}"
        );
        // Huge moves are clamped.
        let far = Transform {
            translation: [1000.0, 0.0, 0.0],
            ..t
        };
        let c = display_matrix(&far, false).transform_point3(Vec3::splat(0.5));
        assert!((c.x - 5.0).abs() < 1e-5);
    }

    #[test]
    fn the_cache_builds_each_icon_once_from_the_atlas() {
        let mut atlas = RgbaImage::from_pixel(8, 8, Rgba([0, 0, 0, 0]));
        // Tile at (4,4) size 4: the blob.
        for y in 0..4 {
            for x in 0..4 {
                atlas.put_pixel(4 + x, 4 + y, *blob().get_pixel(x, y));
            }
        }
        let face = IconFace {
            // Inset by a hundredth of a texel, like Atlas::uv_rect.
            uv: [
                0.5 + 0.01 / 8.0,
                0.5 + 0.01 / 8.0,
                1.0 - 0.01 / 8.0,
                1.0 - 0.01 / 8.0,
            ],
            color: [1.0; 3],
        };
        let mut cache = HeldCache::default();
        let g = cache.get(&ItemIcon::Flat(face), &atlas).clone();
        assert_eq!(g.quads.len(), 2 + 4 * 2);
        assert_eq!(cache.get(&ItemIcon::Flat(face), &atlas), &g);
        assert_eq!(cache.map.len(), 1);
        cache.clear();
        assert!(cache.map.is_empty());
    }
}
