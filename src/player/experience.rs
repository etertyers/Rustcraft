//! The experience bar: points, levels and the progress to the next level,
//! with modern Java Edition's level costs.

use serde::{Deserialize, Serialize};

/// Most points an orb is worth when the game splits a lump into orbs.
const ORB_SIZES: [u32; 11] = [2477, 1237, 617, 307, 149, 73, 37, 17, 7, 3, 1];
/// Death drops 7 points a level, at most this many.
const DEATH_DROP_CAP: u32 = 100;

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Experience {
    pub level: u32,
    /// How far through the current level, 0 up to (not including) 1.
    pub progress: f32,
    /// Everything ever earned (the death screen's score).
    pub total: u32,
}

/// Points needed to go from `level` to the next one.
pub fn points_for_next(level: u32) -> u32 {
    match level {
        0..=15 => 2 * level + 7,
        16..=30 => 5 * level - 38,
        _ => 9 * level - 158,
    }
}

/// Points it takes to reach `level` from nothing.
pub fn points_to_reach(level: u32) -> u32 {
    let level = level as i64;
    let points = match level {
        0..=16 => level * level + 6 * level,
        17..=31 => (5 * level * level - 81 * level) / 2 + 360,
        _ => (9 * level * level - 325 * level) / 2 + 2220,
    };
    points.max(0) as u32
}

impl Experience {
    /// Points of experience held right now (levels and the bar).
    pub fn points(&self) -> u32 {
        points_to_reach(self.level) + (self.progress * points_for_next(self.level) as f32) as u32
    }

    /// Picks up `points`, rolling over into new levels.
    pub fn add(&mut self, points: u32) {
        self.total = self.total.saturating_add(points);
        let mut into_level = self.progress as f64 * points_for_next(self.level) as f64;
        into_level += points as f64;
        while into_level >= points_for_next(self.level) as f64 {
            into_level -= points_for_next(self.level) as f64;
            self.level += 1;
        }
        self.progress = (into_level / points_for_next(self.level) as f64) as f32;
    }

    /// Spends `levels` levels (enchanting, the anvil), keeping the bar.
    pub fn spend_levels(&mut self, levels: u32) {
        self.level = self.level.saturating_sub(levels);
        if self.level == 0 && levels > 0 && self.points() == 0 {
            self.progress = 0.0;
        }
    }

    /// Takes `points` back out (never below nothing), for a grindstone or
    /// a repair that costs experience.
    pub fn remove_points(&mut self, points: u32) {
        let left = self.points().saturating_sub(points);
        self.set_points(left);
    }

    /// Sets the bar to hold exactly `points`, keeping `total`.
    pub fn set_points(&mut self, points: u32) {
        let total = self.total;
        *self = Self::default();
        self.add(points);
        self.total = total;
    }

    /// What a dying player leaves behind: seven points a level, at most
    /// 100.
    pub fn death_drop(&self) -> u32 {
        (self.level * 7).min(DEATH_DROP_CAP)
    }

    /// Dying empties the bar (the score stays).
    pub fn die(&mut self) {
        self.level = 0;
        self.progress = 0.0;
    }
}

/// Splits `points` into orbs, biggest sizes first, as the game does when
/// it spawns experience.
pub fn split_into_orbs(mut points: u32) -> Vec<u32> {
    let mut orbs = Vec::new();
    while points > 0 {
        let size = ORB_SIZES
            .iter()
            .copied()
            .find(|&size| size <= points)
            .unwrap_or(1);
        orbs.push(size);
        points -= size;
    }
    orbs
}

/// Which of the orb texture's 11 pictures shows an orb worth `value`.
pub fn orb_icon(value: u32) -> usize {
    ORB_SIZES
        .iter()
        .position(|&size| size <= value)
        .map_or(0, |big_first| ORB_SIZES.len() - 1 - big_first)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn level_costs_follow_the_modern_curve() {
        assert_eq!(points_for_next(0), 7);
        assert_eq!(points_for_next(1), 9);
        assert_eq!(points_for_next(15), 37);
        assert_eq!(points_for_next(16), 42);
        assert_eq!(points_for_next(30), 112);
        assert_eq!(points_for_next(31), 121);
    }

    #[test]
    fn total_points_for_levels_match_the_wiki_table() {
        assert_eq!(points_to_reach(5), 55);
        assert_eq!(points_to_reach(10), 160);
        assert_eq!(points_to_reach(15), 315);
        assert_eq!(points_to_reach(16), 352);
        assert_eq!(points_to_reach(20), 550);
        assert_eq!(points_to_reach(30), 1395);
        assert_eq!(points_to_reach(31), 1507);
        assert_eq!(points_to_reach(40), 2920);
    }

    #[test]
    fn the_two_formulas_agree_level_by_level() {
        let mut total = 0;
        for level in 0..60 {
            assert_eq!(points_to_reach(level), total, "level {level}");
            total += points_for_next(level);
        }
    }

    #[test]
    fn adding_points_fills_the_bar_and_levels_up() {
        let mut xp = Experience::default();
        xp.add(3);
        assert_eq!((xp.level, xp.points()), (0, 3));
        xp.add(4);
        assert_eq!((xp.level, xp.points()), (1, 7));
        xp.add(1000);
        assert_eq!(xp.points(), 1007);
        assert_eq!(xp.level, 26, "1007 points is level 26");
        assert_eq!(xp.total, 1007);
    }

    #[test]
    fn spending_levels_keeps_the_bar_and_the_score() {
        let mut xp = Experience::default();
        xp.add(points_to_reach(30) + 20);
        let bar = xp.progress;
        xp.spend_levels(3);
        assert_eq!(xp.level, 27);
        assert_eq!(xp.progress, bar);
        xp.spend_levels(100);
        assert_eq!(xp.level, 0);
    }

    #[test]
    fn dying_drops_seven_a_level_up_to_a_hundred() {
        let mut xp = Experience::default();
        xp.add(points_to_reach(5));
        assert_eq!(xp.death_drop(), 35);
        xp.add(points_to_reach(30));
        assert_eq!(xp.death_drop(), 100);
        let total = xp.total;
        xp.die();
        assert_eq!((xp.level, xp.points(), xp.total), (0, 0, total));
    }

    #[test]
    fn lumps_split_into_the_orb_sizes() {
        assert_eq!(split_into_orbs(5), vec![3, 1, 1]);
        assert_eq!(split_into_orbs(100), vec![73, 17, 7, 3]);
        assert_eq!(split_into_orbs(0), Vec::<u32>::new());
        for points in 0..3000 {
            assert_eq!(split_into_orbs(points).iter().sum::<u32>(), points);
        }
    }

    #[test]
    fn orb_icons_run_small_to_big() {
        assert_eq!(orb_icon(1), 0);
        assert_eq!(orb_icon(2), 0);
        assert_eq!(orb_icon(3), 1);
        assert_eq!(orb_icon(2477), 10);
        assert_eq!(orb_icon(100_000), 10);
    }
}
