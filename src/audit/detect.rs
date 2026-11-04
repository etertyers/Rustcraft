//! The texture detectors. Each is a pure function of what was rendered or
//! declared, with its own test on a fixture made to break it. A flag names
//! the detector, the texture (or block, item, mob) and the part.

use image::RgbaImage;
use serde::{Deserialize, Serialize};

use super::raster::Stats;
use crate::world::mesh::Mesh;

/// What looked for the problem.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Detector {
    /// The missing-texture checkerboard shows in the render.
    Placeholder,
    /// A model or mob references a texture no layer has.
    UnknownTexture,
    /// A face is fully transparent where the model is opaque.
    TransparentFace,
    /// A UV lies outside the texture.
    UvOutOfRange,
    /// A mob part's faces sample mostly transparent pixels.
    TransparentPart,
    /// A declared texture size differs from the file's.
    SizeMismatch,
    /// An animated strip is drawn as one squashed picture.
    SquashedStrip,
    /// A tinted texture drawn gray, or a tint on a texture that has none.
    Tint,
    /// An item icon that's empty or one flat color.
    FlatIcon,
    /// A face's orientation differs from the blockstate's.
    Orientation,
    /// Two blocks with different textures render the same sides.
    DuplicateSides,
}

impl Detector {
    /// Higher is worse, for ranking.
    pub fn severity(self) -> u8 {
        match self {
            Self::Placeholder | Self::UnknownTexture => 10,
            Self::TransparentFace | Self::TransparentPart | Self::UvOutOfRange => 8,
            Self::SquashedStrip | Self::SizeMismatch => 7,
            Self::Orientation | Self::DuplicateSides => 6,
            Self::Tint => 5,
            Self::FlatIcon => 4,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Placeholder => "placeholder",
            Self::UnknownTexture => "unknown-texture",
            Self::TransparentFace => "transparent-face",
            Self::UvOutOfRange => "uv-out-of-range",
            Self::TransparentPart => "transparent-part",
            Self::SizeMismatch => "size-mismatch",
            Self::SquashedStrip => "squashed-strip",
            Self::Tint => "tint",
            Self::FlatIcon => "flat-icon",
            Self::Orientation => "orientation",
            Self::DuplicateSides => "duplicate-sides",
        }
    }
}

/// One finding.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Flag {
    pub detector: Detector,
    /// The block, item, mob or sprite it was found on.
    pub id: String,
    /// The model or mob part ("" for the whole).
    pub part: String,
    pub detail: String,
}

impl Flag {
    pub fn new(detector: Detector, id: &str, part: &str, detail: impl Into<String>) -> Self {
        Self {
            detector,
            id: id.to_owned(),
            part: part.to_owned(),
            detail: detail.into(),
        }
    }
}

/// The placeholder shows in the render.
pub fn placeholder_in_render(stats: &Stats) -> Option<String> {
    (stats.placeholder > 0).then(|| {
        format!(
            "{} of {} drawn pixels are the missing-texture placeholder",
            stats.placeholder, stats.samples
        )
    })
}

/// The names in `references` that no layer has.
pub fn unknown_textures<'a>(
    references: impl IntoIterator<Item = &'a str>,
    has: impl Fn(&str) -> bool,
) -> Vec<String> {
    references
        .into_iter()
        .filter(|name| !has(name))
        .map(str::to_owned)
        .collect()
}

/// Every pixel in the atlas rectangle `uv` is transparent.
pub fn face_fully_transparent(atlas: &RgbaImage, uv: [f32; 4]) -> bool {
    let (w, h) = (atlas.width() as f32, atlas.height() as f32);
    let (x0, x1) = ((uv[0] * w).floor() as u32, (uv[2] * w).ceil() as u32);
    let (y0, y1) = ((uv[1] * h).floor() as u32, (uv[3] * h).ceil() as u32);
    let mut any = false;
    for y in y0..y1.min(atlas.height()) {
        for x in x0..x1.min(atlas.width()) {
            any = true;
            if atlas.get_pixel(x, y).0[3] != 0 {
                return false;
            }
        }
    }
    any
}

/// The first vertex whose UV is outside the texture.
pub fn uv_out_of_range(mesh: &Mesh) -> Option<String> {
    const EPS: f32 = 1e-4;
    mesh.vertices.iter().enumerate().find_map(|(i, v)| {
        let [u, t] = v.uv;
        (!(-EPS..=1.0 + EPS).contains(&u) || !(-EPS..=1.0 + EPS).contains(&t))
            .then(|| format!("vertex {i} has UV ({u:.3}, {t:.3}) outside the texture"))
    })
}

/// Mob parts (each box is 24 vertices: six quads) whose faces sample more
/// than `limit` transparent pixels: (part index, the share).
pub fn transparent_parts(mesh: &Mesh, texture: &RgbaImage, limit: f32) -> Vec<(usize, f32)> {
    let (w, h) = (texture.width() as f32, texture.height() as f32);
    let mut out = Vec::new();
    for (part, vertices) in mesh.vertices.chunks(24).enumerate() {
        let (mut total, mut clear) = (0u32, 0u32);
        for quad in vertices.chunks(4) {
            let us = quad.iter().map(|v| v.uv[0]);
            let ts = quad.iter().map(|v| v.uv[1]);
            let (u0, u1) = (
                us.clone().fold(f32::MAX, f32::min),
                us.fold(f32::MIN, f32::max),
            );
            let (t0, t1) = (
                ts.clone().fold(f32::MAX, f32::min),
                ts.fold(f32::MIN, f32::max),
            );
            // A 6×6 grid of samples inside the face's UV rectangle.
            for j in 0..6 {
                for i in 0..6 {
                    let u = u0 + (u1 - u0) * (i as f32 + 0.5) / 6.0;
                    let t = t0 + (t1 - t0) * (j as f32 + 0.5) / 6.0;
                    let x = ((u * w) as i32).clamp(0, texture.width() as i32 - 1) as u32;
                    let y = ((t * h) as i32).clamp(0, texture.height() as i32 - 1) as u32;
                    total += 1;
                    if texture.get_pixel(x, y).0[3] < 32 {
                        clear += 1;
                    }
                }
            }
        }
        if total > 0 && clear as f32 / total as f32 > limit {
            out.push((part, clear as f32 / total as f32));
        }
    }
    out
}

/// A declared texture size that doesn't match the file's. Packs may be a
/// whole multiple (an HD pack), the vanilla layer must be exact.
pub fn size_mismatch(declared: (u32, u32), actual: (u32, u32), allow_multiple: bool) -> bool {
    if declared == actual {
        return false;
    }
    if !allow_multiple || declared.0 == 0 || declared.1 == 0 {
        return true;
    }
    let whole_multiple = actual.0.is_multiple_of(declared.0)
        && actual.1.is_multiple_of(declared.1)
        && actual.0 / declared.0 == actual.1 / declared.1;
    !whole_multiple
}

/// An animated strip drawn as one squashed picture: the file holds more
/// than one frame, and the face it's drawn on (`drawn`) has the aspect of
/// the whole strip instead of one frame's.
pub fn squashed_strip(image: (u32, u32), frame: (u32, u32), drawn: (u32, u32)) -> bool {
    let frames = (image.0 / frame.0.max(1)) * (image.1 / frame.1.max(1));
    if frames <= 1 {
        return false;
    }
    // Aspects compared by cross-multiplication.
    let same = |a: (u32, u32), b: (u32, u32)| {
        u64::from(a.0) * u64::from(b.1) == u64::from(a.1) * u64::from(b.0)
    };
    !same(drawn, frame)
}

/// Whether a color has any hue (a tint), against the gray of plain shading.
fn is_colored(color: [f32; 3]) -> bool {
    let (lo, hi) = color
        .iter()
        .fold((f32::MAX, f32::MIN), |(lo, hi), &c| (lo.min(c), hi.max(c)));
    hi - lo > 0.06
}

/// A tinted texture drawn gray, or a tint on a texture that has none.
pub fn tint_mismatch(expects_tint: bool, colors: &[[f32; 3]]) -> Option<&'static str> {
    if colors.is_empty() {
        return None;
    }
    let colored = colors.iter().any(|&c| is_colored(c));
    match (expects_tint, colored) {
        (true, false) => Some("a tinted texture is drawn untinted (gray)"),
        (false, true) => Some("a texture with no tint is drawn tinted"),
        _ => None,
    }
}

/// An icon that is empty or a single flat color.
pub fn flat_icon(icon: &RgbaImage) -> Option<&'static str> {
    let mut first: Option<[u8; 4]> = None;
    for p in icon.pixels().filter(|p| p.0[3] > 0) {
        match first {
            None => first = Some(p.0),
            Some(c) if c != p.0 => return None,
            _ => {}
        }
    }
    Some(if first.is_none() {
        "the icon is empty"
    } else {
        "the icon is a single flat color"
    })
}

/// A log or pillar's end texture must sit on the faces its axis points
/// through. `along_y`/`along_x` are the UV rectangles of the (top, east,
/// north) faces with the axis up and along x.
pub fn axis_consistent(along_y: [[f32; 4]; 3], along_x: [[f32; 4]; 3]) -> bool {
    let [top_y, east_y, _] = along_y;
    let [top_x, east_x, _] = along_x;
    // With the axis along x the ends are the east faces: the end texture
    // (the top when upright), and the top shows the side texture.
    east_x == top_y && top_x == east_y
}

/// Groups of blocks whose rendered sides are identical although their
/// declared textures differ: a lookup that fell through to the same wrong
/// texture. `blocks` are (id, declared texture names, rendered UV rects).
/// A block's id, its declared texture names and the UV rectangles of its
/// six faces.
pub type Sides = (String, Vec<String>, Vec<[u32; 4]>);

pub fn duplicate_sides(blocks: &[Sides]) -> Vec<Vec<String>> {
    let mut groups: std::collections::BTreeMap<&Vec<[u32; 4]>, Vec<&Sides>> = Default::default();
    for block in blocks {
        groups.entry(&block.2).or_default().push(block);
    }
    groups
        .into_values()
        .filter(|g| g.len() > 1 && g.iter().any(|b| b.1 != g[0].1))
        .map(|g| g.iter().map(|b| b.0.clone()).collect())
        .collect()
}

#[cfg(test)]
mod tests {
    use image::Rgba;

    use super::*;
    use crate::render::vertex::Vertex;

    fn mesh_with_uvs(uvs: &[[f32; 2]]) -> Mesh {
        let mut mesh = Mesh::default();
        mesh.vertices = uvs
            .iter()
            .map(|&uv| Vertex {
                position: [0.0; 3],
                uv,
                color: [1.0; 3],
                light: [15.0, 0.0],
            })
            .collect();
        mesh
    }

    #[test]
    fn a_placeholder_in_the_render_is_flagged_and_a_clean_one_isnt() {
        let bad = Stats {
            samples: 100,
            placeholder: 40,
        };
        assert!(placeholder_in_render(&bad).unwrap().contains("40 of 100"));
        let good = Stats {
            samples: 100,
            placeholder: 0,
        };
        assert!(placeholder_in_render(&good).is_none());
    }

    #[test]
    fn a_reference_to_a_texture_no_layer_has_is_flagged() {
        let has = |n: &str| n == "block/stone";
        assert_eq!(
            unknown_textures(["block/stone", "block/nope"], has),
            ["block/nope"]
        );
        assert!(unknown_textures(["block/stone"], has).is_empty());
    }

    #[test]
    fn a_face_with_no_opaque_pixel_is_transparent() {
        let mut atlas = RgbaImage::from_pixel(4, 4, Rgba([0, 0, 0, 0]));
        assert!(face_fully_transparent(&atlas, [0.0, 0.0, 0.5, 0.5]));
        atlas.put_pixel(1, 1, Rgba([9, 9, 9, 255]));
        assert!(!face_fully_transparent(&atlas, [0.0, 0.0, 0.5, 0.5]));
        assert!(face_fully_transparent(&atlas, [0.5, 0.5, 1.0, 1.0]));
    }

    #[test]
    fn a_uv_outside_the_texture_is_flagged() {
        let bad = mesh_with_uvs(&[[0.0, 0.0], [1.2, 0.5]]);
        assert!(uv_out_of_range(&bad).unwrap().contains("vertex 1"));
        let good = mesh_with_uvs(&[[0.0, 0.0], [1.0, 0.5]]);
        assert!(uv_out_of_range(&good).is_none());
    }

    #[test]
    fn a_mob_part_over_empty_pixels_is_flagged() {
        // A 8×8 texture, the left half opaque. One box (24 vertices) with
        // every face on the clear right half, one on the opaque left.
        let mut texture = RgbaImage::from_pixel(8, 8, Rgba([0, 0, 0, 0]));
        for y in 0..8 {
            for x in 0..4 {
                texture.put_pixel(x, y, Rgba([200, 100, 50, 255]));
            }
        }
        let mut uvs = Vec::new();
        for _ in 0..6 {
            uvs.extend([[0.6, 0.1], [0.9, 0.1], [0.9, 0.9], [0.6, 0.9]]);
        }
        for _ in 0..6 {
            uvs.extend([[0.1, 0.1], [0.4, 0.1], [0.4, 0.9], [0.1, 0.9]]);
        }
        let flagged = transparent_parts(&mesh_with_uvs(&uvs), &texture, 0.5);
        assert_eq!(flagged.len(), 1);
        assert_eq!(flagged[0].0, 0);
        assert!(flagged[0].1 > 0.99);
    }

    #[test]
    fn a_64x32_texture_declared_64x64_is_a_mismatch() {
        assert!(size_mismatch((64, 64), (64, 32), true));
        assert!(!size_mismatch((64, 64), (64, 64), false));
        // An HD pack's 128×128 is fine in a pack, not in vanilla.
        assert!(!size_mismatch((64, 64), (128, 128), true));
        assert!(size_mismatch((64, 64), (128, 128), false));
    }

    #[test]
    fn a_strip_drawn_in_one_square_tile_is_squashed() {
        // 16×512 holds 32 frames of 16×16; drawn 16×16 is right…
        assert!(!squashed_strip((16, 512), (16, 16), (16, 16)));
        // …drawn at the strip's aspect (16×512) it's squashed.
        assert!(squashed_strip((16, 512), (16, 16), (16, 512)));
        // Non-square frames in a square tile are squashed too.
        assert!(squashed_strip((16, 64), (16, 32), (16, 16)));
        // A plain texture is never a strip.
        assert!(!squashed_strip((16, 16), (16, 16), (16, 16)));
    }

    #[test]
    fn grass_drawn_gray_is_flagged_and_so_is_a_tint_on_stone() {
        let gray = [[0.8; 3], [0.6; 3]];
        let green = [[0.4, 0.8, 0.3]];
        assert!(tint_mismatch(true, &gray).unwrap().contains("untinted"));
        assert!(tint_mismatch(true, &green).is_none());
        assert!(tint_mismatch(false, &gray).is_none());
        assert!(tint_mismatch(false, &green).unwrap().contains("no tint"));
    }

    #[test]
    fn an_empty_or_single_color_icon_is_flagged() {
        let empty = RgbaImage::new(4, 4);
        assert_eq!(flat_icon(&empty), Some("the icon is empty"));
        let flat = RgbaImage::from_pixel(4, 4, Rgba([5, 5, 5, 255]));
        assert!(flat_icon(&flat).unwrap().contains("flat"));
        let mut fine = flat.clone();
        fine.put_pixel(0, 0, Rgba([9, 9, 9, 255]));
        assert!(flat_icon(&fine).is_none());
    }

    #[test]
    fn a_log_lying_on_its_side_has_its_ends_on_the_east_faces() {
        let end = [0.0, 0.0, 0.1, 0.1];
        let bark = [0.2, 0.0, 0.3, 0.1];
        let other = [0.4, 0.0, 0.5, 0.1];
        // Upright: end on top, bark on the sides.
        let along_y = [end, bark, bark];
        assert!(axis_consistent(along_y, [bark, end, bark]));
        // A log whose ends stayed on top when lying down.
        assert!(!axis_consistent(along_y, [end, bark, bark]));
        assert!(!axis_consistent(along_y, [other, other, other]));
    }

    #[test]
    fn blocks_with_different_textures_but_identical_sides_are_grouped() {
        let rect = vec![[1, 2, 3, 4]; 6];
        let blocks = vec![
            ("a".to_owned(), vec!["block/a".to_owned()], rect.clone()),
            ("b".to_owned(), vec!["block/b".to_owned()], rect.clone()),
            (
                "c".to_owned(),
                vec!["block/c".to_owned()],
                vec![[9, 9, 9, 9]; 6],
            ),
            // The same texture name twice is no fault (a copy of a block).
            (
                "d".to_owned(),
                vec!["block/c".to_owned()],
                vec![[9, 9, 9, 9]; 6],
            ),
        ];
        assert_eq!(duplicate_sides(&blocks), vec![vec!["a", "b"]]);
    }
}
