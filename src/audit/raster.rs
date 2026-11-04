//! A small software rasterizer for the texture checker: draws one of the
//! game's meshes (a block, a mob) from a chosen angle with its texture,
//! with no window and no GPU, so the audit runs anywhere and its pictures
//! are the same every time. It also counts what the render sampled, which
//! the detectors read.

use glam::{Mat3, Vec3};
use image::{Rgba, RgbaImage};

use crate::world::mesh::Mesh;

/// Which way the camera looks at the model, in radians.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct View {
    /// Turn about the vertical axis (0 looks at the model's south face).
    pub yaw: f32,
    /// Tilt down from the horizon (π/2 is straight down).
    pub pitch: f32,
}

impl View {
    /// The block's fixed three-quarter view.
    pub const THREE_QUARTER: Self = Self {
        yaw: std::f32::consts::FRAC_PI_4,
        pitch: std::f32::consts::FRAC_PI_6,
    };
    /// Straight down.
    pub const TOP: Self = Self {
        yaw: 0.0,
        pitch: std::f32::consts::FRAC_PI_2,
    };
    /// A mob from the front, the side and the back, slightly from above.
    pub const FRONT: Self = Self {
        yaw: 0.0,
        pitch: 0.2,
    };
    pub const SIDE: Self = Self {
        yaw: std::f32::consts::FRAC_PI_2,
        pitch: 0.2,
    };
    pub const BACK: Self = Self {
        yaw: std::f32::consts::PI,
        pitch: 0.2,
    };
}

/// What a render sampled.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct Stats {
    /// Pixels drawn.
    pub samples: u32,
    /// Of them, pixels that sampled the missing-texture placeholder.
    pub placeholder: u32,
}

/// Draws `mesh` (UVs are fractions of `texture`) into a `size`×`size`
/// picture on `background`. `missing` is the placeholder's UV rectangle, if
/// the texture has one (the atlas does).
pub fn render(
    mesh: &Mesh,
    texture: &RgbaImage,
    view: View,
    size: u32,
    background: Rgba<u8>,
    missing: Option<[f32; 4]>,
) -> (RgbaImage, Stats) {
    let mut image = RgbaImage::from_pixel(size, size, background);
    let mut stats = Stats::default();
    if mesh.vertices.is_empty() {
        return (image, stats);
    }
    // The model's bounding box, to center and fit it.
    let (mut lo, mut hi) = (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN));
    for v in &mesh.vertices {
        let p = Vec3::from(v.position);
        lo = lo.min(p);
        hi = hi.max(p);
    }
    let center = (lo + hi) * 0.5;
    let radius = ((hi - lo).length() * 0.5).max(1e-3);
    // Camera space: x right, y up, z toward the camera.
    let rotation = Mat3::from_rotation_x(view.pitch) * Mat3::from_rotation_y(-view.yaw);
    let scale = size as f32 * 0.46 / radius;
    let project = |position: [f32; 3]| {
        let p = rotation * (Vec3::from(position) - center);
        (
            size as f32 * 0.5 + p.x * scale,
            size as f32 * 0.5 - p.y * scale,
            p.z,
        )
    };
    let mut depth = vec![f32::MIN; (size * size) as usize];
    let (tw, th) = (texture.width() as f32, texture.height() as f32);
    for list in [&mesh.indices, &mesh.translucent] {
        #[allow(clippy::chunks_exact_to_as_chunks)]
        for tri in list.chunks_exact(3) {
            let v = [tri[0], tri[1], tri[2]].map(|i| &mesh.vertices[i as usize]);
            let p = v.map(|v| project(v.position));
            let min_x = p
                .iter()
                .map(|p| p.0)
                .fold(f32::MAX, f32::min)
                .floor()
                .max(0.0) as i32;
            let max_x = p
                .iter()
                .map(|p| p.0)
                .fold(f32::MIN, f32::max)
                .ceil()
                .min(size as f32 - 1.0) as i32;
            let min_y = p
                .iter()
                .map(|p| p.1)
                .fold(f32::MAX, f32::min)
                .floor()
                .max(0.0) as i32;
            let max_y = p
                .iter()
                .map(|p| p.1)
                .fold(f32::MIN, f32::max)
                .ceil()
                .min(size as f32 - 1.0) as i32;
            let area =
                (p[1].0 - p[0].0) * (p[2].1 - p[0].1) - (p[2].0 - p[0].0) * (p[1].1 - p[0].1);
            if area.abs() < 1e-6 {
                continue;
            }
            for y in min_y..=max_y {
                for x in min_x..=max_x {
                    let (fx, fy) = (x as f32 + 0.5, y as f32 + 0.5);
                    let w0 = ((p[1].0 - fx) * (p[2].1 - fy) - (p[2].0 - fx) * (p[1].1 - fy)) / area;
                    let w1 = ((p[2].0 - fx) * (p[0].1 - fy) - (p[0].0 - fx) * (p[2].1 - fy)) / area;
                    let w2 = 1.0 - w0 - w1;
                    if w0 < -1e-4 || w1 < -1e-4 || w2 < -1e-4 {
                        continue;
                    }
                    let z = w0 * p[0].2 + w1 * p[1].2 + w2 * p[2].2;
                    let at = (y as u32 * size + x as u32) as usize;
                    if z < depth[at] {
                        continue;
                    }
                    let u = w0 * v[0].uv[0] + w1 * v[1].uv[0] + w2 * v[2].uv[0];
                    let t = w0 * v[0].uv[1] + w1 * v[1].uv[1] + w2 * v[2].uv[1];
                    let tx = ((u * tw).floor() as i32).clamp(0, texture.width() as i32 - 1);
                    let ty = ((t * th).floor() as i32).clamp(0, texture.height() as i32 - 1);
                    let texel = texture.get_pixel(tx as u32, ty as u32).0;
                    if texel[3] < 8 {
                        continue;
                    }
                    let color: [f32; 3] = std::array::from_fn(|i| {
                        w0 * v[0].color[i] + w1 * v[1].color[i] + w2 * v[2].color[i]
                    });
                    let shade = |c: u8, k: f32| (f32::from(c) * k).clamp(0.0, 255.0) as u8;
                    let src = [
                        shade(texel[0], color[0]),
                        shade(texel[1], color[1]),
                        shade(texel[2], color[2]),
                    ];
                    let out = if texel[3] == 255 {
                        depth[at] = z;
                        Rgba([src[0], src[1], src[2], 255])
                    } else {
                        let a = f32::from(texel[3]) / 255.0;
                        let below = image.get_pixel(x as u32, y as u32).0;
                        let mix =
                            |s: u8, b: u8| (f32::from(s) * a + f32::from(b) * (1.0 - a)) as u8;
                        Rgba([
                            mix(src[0], below[0]),
                            mix(src[1], below[1]),
                            mix(src[2], below[2]),
                            255,
                        ])
                    };
                    image.put_pixel(x as u32, y as u32, out);
                    stats.samples += 1;
                    if let Some(m) = missing
                        && u >= m[0]
                        && u <= m[2]
                        && t >= m[1]
                        && t <= m[3]
                    {
                        stats.placeholder += 1;
                    }
                }
            }
        }
    }
    (image, stats)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render::vertex::Vertex;

    /// One quad facing the camera, UV covering all of a texture.
    pub fn quad(uv: [f32; 4], z: f32) -> Mesh {
        let mut mesh = Mesh::default();
        let v = |x: f32, y: f32, u: f32, t: f32| Vertex {
            position: [x, y, z],
            uv: [u, t],
            color: [1.0; 3],
            light: [15.0, 0.0],
        };
        mesh.vertices = vec![
            v(-1.0, -1.0, uv[0], uv[3]),
            v(1.0, -1.0, uv[2], uv[3]),
            v(1.0, 1.0, uv[2], uv[1]),
            v(-1.0, 1.0, uv[0], uv[1]),
        ];
        mesh.indices = vec![0, 1, 2, 0, 2, 3];
        mesh
    }

    #[test]
    fn a_quad_facing_the_camera_shows_its_texture_and_a_hidden_one_does_not() {
        let mut texture = RgbaImage::from_pixel(2, 1, Rgba([255, 0, 0, 255]));
        texture.put_pixel(1, 0, Rgba([0, 255, 0, 255]));
        let (image, stats) = render(
            &quad([0.0, 0.0, 1.0, 1.0], 0.0),
            &texture,
            View {
                yaw: 0.0,
                pitch: 0.0,
            },
            32,
            Rgba([0, 0, 0, 255]),
            None,
        );
        // The left half is the texture's first pixel, the right its second.
        assert_eq!(image.get_pixel(8, 16).0, [255, 0, 0, 255]);
        assert_eq!(image.get_pixel(24, 16).0, [0, 255, 0, 255]);
        assert!(stats.samples > 300);
        // Seen edge-on, it draws (almost) nothing.
        let (edge, _) = render(
            &quad([0.0, 0.0, 1.0, 1.0], 0.0),
            &texture,
            View {
                yaw: std::f32::consts::FRAC_PI_2,
                pitch: 0.0,
            },
            32,
            Rgba([0, 0, 0, 255]),
            None,
        );
        assert_eq!(edge.get_pixel(8, 16).0, [0, 0, 0, 255]);
    }

    #[test]
    fn samples_of_the_placeholder_are_counted() {
        let texture = RgbaImage::from_pixel(2, 2, Rgba([255, 0, 255, 255]));
        let (_, stats) = render(
            &quad([0.0, 0.0, 0.5, 0.5], 0.0),
            &texture,
            View {
                yaw: 0.0,
                pitch: 0.0,
            },
            16,
            Rgba([0, 0, 0, 255]),
            Some([0.0, 0.0, 0.5, 0.5]),
        );
        assert!(stats.placeholder > 100 && stats.placeholder == stats.samples);
    }
}
