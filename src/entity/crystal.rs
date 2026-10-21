//! End crystals (M27, modern `EndCrystal`): the spinning gems on the
//! pillars that heal the dragon, and that blow up when hit.

use glam::DVec3;

use crate::player::collision::Aabb;

/// Its power when it goes off (modern 6).
pub const BLAST_POWER: f32 = 6.0;
/// Side of its box.
const SIZE: f64 = 2.0;

#[derive(Debug, Clone, PartialEq)]
pub struct EndCrystal {
    /// The middle of its bottom.
    pub position: DVec3,
    /// Whether it floats over a plate of bedrock of its own (crystals put
    /// down by hand do; the pillars' have a bedrock block).
    pub show_base: bool,
    pub age: u32,
    /// Where the beam of the dragon it heals goes to, if it has one.
    pub beam_to: Option<DVec3>,
}

impl EndCrystal {
    pub fn new(position: DVec3, show_base: bool) -> Self {
        Self {
            position,
            show_base,
            age: 0,
            beam_to: None,
        }
    }

    pub fn aabb(&self) -> Aabb {
        Aabb::new(
            self.position - DVec3::new(SIZE / 2.0, 0.0, SIZE / 2.0),
            self.position + DVec3::new(SIZE / 2.0, SIZE, SIZE / 2.0),
        )
    }

    pub fn center(&self) -> DVec3 {
        self.position + DVec3::Y
    }

    /// How far up it bobs, in blocks (modern `EndCrystalRenderer.getY`).
    pub fn bob(&self, partial: f32) -> f32 {
        let t = (self.age as f32 + partial) * 0.2;
        let h = t.sin() * 0.5 + 0.5;
        (h * h + h) * 0.4
    }

    /// How far it has spun, in radians.
    pub fn spin(&self, partial: f32) -> f32 {
        (self.age as f32 + partial) * 3.0_f32.to_radians()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_crystal_is_two_blocks_across_and_tall() {
        let crystal = EndCrystal::new(DVec3::new(0.5, 80.0, 0.5), false);
        let bb = crystal.aabb();
        assert_eq!(bb.max - bb.min, DVec3::splat(2.0));
        assert!(bb.contains(crystal.center()));
    }

    #[test]
    fn it_bobs_between_nothing_and_most_of_a_block_and_keeps_spinning() {
        let mut crystal = EndCrystal::new(DVec3::ZERO, true);
        let (mut low, mut high) = (f32::MAX, f32::MIN);
        for age in 0..200 {
            crystal.age = age;
            let b = crystal.bob(0.0);
            low = low.min(b);
            high = high.max(b);
        }
        assert!(low < 0.01 && (0.7..0.9).contains(&high), "{low}..{high}");
        crystal.age = 120;
        assert!((crystal.spin(0.0) - 2.0 * std::f32::consts::PI).abs() < 1e-4);
    }
}
