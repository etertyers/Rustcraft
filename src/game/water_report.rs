//! `--shot m51-underwater`: what the eyes see under water, in numbers.
//! Fog distances and formula as the shader gets them, the water vision,
//! the full-screen overlay's opacity, and how many water faces the view
//! ray crosses in the 10 blocks ahead (the layers that would turn an
//! ocean opaque).

use glam::Vec3;

use super::Game;
use crate::world::{
    chunk::ChunkPos,
    light::light_chunk,
    mesh::{Mesh, mesh_chunk},
};

/// The translucent faces of `mesh` that the segment from `from` along `dir`
/// (a unit vector) crosses within `len` blocks.
pub fn faces_along(mesh: &Mesh, from: Vec3, dir: Vec3, len: f32) -> usize {
    mesh.translucent
        .as_chunks::<6>()
        .0
        .iter()
        .filter(|quad| {
            let mut low = Vec3::splat(f32::MAX);
            let mut high = Vec3::splat(f32::MIN);
            for &i in quad.iter() {
                let p = Vec3::from_array(mesh.vertices[i as usize].position);
                low = low.min(p);
                high = high.max(p);
            }
            // The slab test against the face's box, a hair thick.
            let (low, high) = (low - Vec3::splat(1e-3), high + Vec3::splat(1e-3));
            let (mut near, mut far) = (0.0f32, len);
            for axis in 0..3 {
                if dir[axis].abs() < 1e-9 {
                    if from[axis] < low[axis] || from[axis] > high[axis] {
                        return false;
                    }
                    continue;
                }
                let (a, b) = (
                    (low[axis] - from[axis]) / dir[axis],
                    (high[axis] - from[axis]) / dir[axis],
                );
                near = near.max(a.min(b));
                far = far.min(a.max(b));
            }
            near <= far
        })
        .count()
}

/// How many translucent (water) faces the view ray crosses in the next
/// `len` blocks, from the chunks as the game meshes them.
pub fn water_faces_ahead(game: &Game, len: f32) -> usize {
    let eye = game.player.camera.position;
    let dir = game.player.camera.forward();
    let mut chunks: Vec<ChunkPos> = Vec::new();
    for step in 0..=(len as i32 / 4 + 1) {
        let p = eye + dir * (step as f32 * 4.0).min(len);
        let pos = ChunkPos::of_block(p.x.floor() as i32, p.z.floor() as i32);
        if !chunks.contains(&pos) {
            chunks.push(pos);
        }
    }
    chunks
        .into_iter()
        .filter_map(|pos| {
            let area = game.chunks.world().area(pos)?;
            let models = game.chunks.models();
            let light = light_chunk(models, &area);
            Some(faces_along(
                &mesh_chunk(models, &area, &light),
                eye,
                dir,
                len,
            ))
        })
        .sum()
}

/// The numbers for the camera where it is, at 0, 100 and 600 ticks under
/// water.
pub fn report(game: &mut Game) -> String {
    let mut out = String::new();
    let eye = game.player.camera.position;
    out.push_str(&format!(
        "underwater report: eye {:.1} {:.1} {:.1}, in {:?}, looking {:.2} {:.2}\n",
        eye.x,
        eye.y,
        eye.z,
        game.eye_in(),
        game.player.camera.yaw,
        game.player.camera.pitch
    ));
    for ticks in [0, 100, 600] {
        for min_light in [0.0, 15.0] {
            game.player.water_vision = ticks;
            let env = game.environment_with(min_light);
            out.push_str(&format!(
                "  {ticks:>3} ticks under, Fullbright {}: fog {} from {} to {:.1} blocks (spherical, from the eye); \
                 exponential density {}; water vision {:.3}; fog color {:.3} {:.3} {:.3}\n",
                if min_light > 0.0 { "on " } else { "off" },
                if env.fog_density > 0.0 { "exponential" } else { "linear" },
                env.fog_start,
                env.fog_end,
                env.fog_density,
                super::environment::water_vision(ticks),
                env.fog_color.x,
                env.fog_color.y,
                env.fog_color.z,
            ));
        }
    }
    let overlay = game.underwater_overlay(0.0).map_or(0.0, |(alpha, _)| alpha);
    out.push_str(&format!(
        "  full-screen overlay opacity: {overlay:.3} (Fullbright on: {:.3})\n  water faces crossed by the view ray in 10 blocks: {}\n",
        game.underwater_overlay(15.0).map_or(0.0, |(alpha, _)| alpha),
        water_faces_ahead(game, 10.0),
    ));
    out
}

/// A `--shot` stage: prints the report, then leaves the eyes used to the
/// water (30 s under).
pub fn stage(game: &mut Game) {
    print!("{}", report(game));
    game.player.water_vision = super::environment::WATER_VISION_TICKS;
}
