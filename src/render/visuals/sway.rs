//! Swaying plants and leaves (M57), as a tested copy of `sway.wgsl`: the
//! offset a vertex gets by its material, whether it is at the top of its
//! block, its place, the game time and the wind.

use glam::Vec3;

use crate::world::materials::Class;

/// The most each kind moves, in blocks: 1/16 for leaves, 3/16 for plants
/// (so they never clip through the block above), 3/32 for vines.
pub const LEAVES: f32 = 0.0625;
pub const PLANT: f32 = 0.1875;
pub const VINE: f32 = 0.09375;

/// The offset of a vertex of material `class` at `position`.
/// `top` is the vertex being in the upper half of its block; `wind` 0–1.
pub fn offset(class: Class, top: bool, position: Vec3, ticks: f32, wind: f32) -> Vec3 {
    let reach = match class {
        Class::CutoutLeaves => LEAVES,
        Class::CutoutPlant | Class::Crops if top => PLANT,
        Class::Vines if !top => VINE,
        _ => return Vec3::ZERO,
    };
    let t = ticks / 20.0;
    let wind = wind.clamp(0.0, 1.0);
    let phase = position.x * 0.9 + position.z * 1.3 + position.y * 0.45;
    let strength = reach * (0.55 + 0.45 * wind) * std::f32::consts::FRAC_1_SQRT_2;
    let dx = (t * (1.6 + wind * 1.4) + phase).sin();
    let dz = (t * (1.3 + wind * 1.2) + phase * 1.3 + 1.7).sin();
    Vec3::new(dx * strength, 0.0, dz * strength)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Samples every material over places, times and winds.
    fn samples() -> impl Iterator<Item = (Vec3, f32, f32)> {
        (0..40).flat_map(|i| {
            (0..6).flat_map(move |w| {
                (0..25).map(move |t| {
                    (
                        Vec3::new(
                            i as f32 * 3.7 - 50.0,
                            60.0 + i as f32 * 0.3,
                            i as f32 * -2.9,
                        ),
                        t as f32 * 41.0,
                        w as f32 / 5.0,
                    )
                })
            })
        })
    }

    #[test]
    fn a_plants_bottom_never_moves_and_its_top_stays_within_three_sixteenths() {
        let mut moved = 0.0f32;
        for class in [Class::CutoutPlant, Class::Crops] {
            for (p, t, w) in samples() {
                assert_eq!(offset(class, false, p, t, w), Vec3::ZERO);
                let top = offset(class, true, p, t, w);
                assert!(top.length() <= PLANT + 1e-6, "{top:?}");
                assert_eq!(top.y, 0.0, "plants bend, they don't stretch");
                moved = moved.max(top.length());
            }
        }
        assert!(moved > PLANT * 0.5, "{moved}");
    }

    #[test]
    fn leaves_stay_within_a_sixteenth_in_the_worst_storm_too() {
        let mut moved = 0.0f32;
        for (p, t, w) in samples() {
            for top in [false, true] {
                let o = offset(Class::CutoutLeaves, top, p, t, w);
                assert!(o.length() <= LEAVES + 1e-6, "{o:?}");
                moved = moved.max(o.length());
            }
        }
        assert!(moved > LEAVES * 0.4);
        // Vines swing at their lower end, never more than 3/32.
        for (p, t, w) in samples() {
            assert_eq!(offset(Class::Vines, true, p, t, w), Vec3::ZERO);
            assert!(offset(Class::Vines, false, p, t, w).length() <= VINE + 1e-6);
        }
    }

    #[test]
    fn storms_move_things_more_and_stone_never_moves() {
        let p = Vec3::new(3.0, 64.0, 5.0);
        let swing = |wind: f32| {
            (0..400)
                .map(|t| offset(Class::CutoutPlant, true, p, t as f32, wind).length())
                .fold(0.0, f32::max)
        };
        assert!(
            swing(1.0) > swing(0.0) * 1.3,
            "{} {}",
            swing(1.0),
            swing(0.0)
        );
        for class in [Class::Opaque, Class::Water, Class::Glass, Class::Emissive] {
            assert_eq!(offset(class, true, p, 100.0, 1.0), Vec3::ZERO);
        }
    }
}
