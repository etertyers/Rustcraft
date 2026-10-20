//! The terrain's shape from the climate, after modern Minecraft's
//! `TerrainProvider`: splines over continentalness, each point a spline
//! over erosion, each of those a spline over peaks-and-valleys, give every
//! column three numbers:
//!
//! - `offset`: the surface height the land is pulled toward (world Y);
//! - `factor`: how strongly it's pulled (high: flat and smooth; low: the 3D
//!   noise can make overhangs);
//! - `jaggedness`: extra sharp height on peaks only (low erosion, high PV).
//!
//! Rising continentalness raises the land, rising erosion flattens and
//! lowers it, rising PV raises it, and valleys (PV near -1) pull it down to
//! just under sea level so rivers cut through even far inland. Heights are
//! Rustcraft's (sea level 63, modern Y -64..63 squeezed into 0..63, above
//! sea level unchanged).

use super::climate::Climate;

/// A piecewise-linear curve over one parameter, whose points may be
/// curves themselves.
#[derive(Debug, Clone)]
pub struct Spline {
    points: Vec<(f64, Value)>,
}

#[derive(Debug, Clone)]
pub enum Value {
    Const(f64),
    /// A spline over the next parameter (erosion, then PV).
    Spline(Spline),
}

impl Spline {
    fn new(points: Vec<(f64, Value)>) -> Self {
        Self { points }
    }

    /// A spline of plain numbers.
    fn of(points: &[(f64, f64)]) -> Self {
        Self::new(points.iter().map(|&(x, y)| (x, Value::Const(y))).collect())
    }

    /// The curve at `at`, the nested curves read with `rest`.
    pub fn eval(&self, at: f64, rest: &[f64]) -> f64 {
        let value = |v: &Value| match v {
            Value::Const(c) => *c,
            Value::Spline(s) => s.eval(rest[0], &rest[1..]),
        };
        let points = &self.points;
        if at <= points[0].0 {
            return value(&points[0].1);
        }
        for pair in points.windows(2) {
            let ((x0, v0), (x1, v1)) = (&pair[0], &pair[1]);
            if at <= *x1 {
                let t = (at - x0) / (x1 - x0);
                return value(v0) + (value(v1) - value(v0)) * t;
            }
        }
        value(&points[points.len() - 1].1)
    }
}

/// The three numbers for a column.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Shape {
    pub offset: f64,
    pub factor: f64,
    pub jaggedness: f64,
}

/// Peaks-and-valleys curves, as (PV, Y) points. Each starts in a river
/// valley under sea level (PV -1) and climbs through the valley's edge.
fn pv(points: &[(f64, f64)]) -> Value {
    Value::Spline(Spline::of(points))
}

/// The erosion curve for one band of continentalness, from mountainous
/// (erosion -1) to worn flat (erosion 1).
fn erosion(points: Vec<(f64, Value)>) -> Value {
    Value::Spline(Spline::new(points))
}

/// The offset: world Y the surface is pulled toward.
pub fn offset_spline() -> Spline {
    // The coast's end and near inland: low, with steep coastal hills
    // where erosion is low.
    let near = erosion(vec![
        (
            -1.0,
            pv(&[
                (-1.0, 57.0),
                (-0.85, 59.0),
                (-0.4, 76.0),
                (0.0, 96.0),
                (0.4, 120.0),
                (0.7, 145.0),
                (1.0, 155.0),
            ]),
        ),
        (
            -0.5,
            pv(&[
                (-1.0, 57.0),
                (-0.85, 59.0),
                (-0.4, 68.0),
                (0.0, 78.0),
                (0.5, 92.0),
                (1.0, 104.0),
            ]),
        ),
        (
            -0.2,
            pv(&[
                (-1.0, 57.0),
                (-0.85, 59.0),
                (-0.4, 66.0),
                (0.0, 70.0),
                (1.0, 80.0),
            ]),
        ),
        (
            0.1,
            pv(&[
                (-1.0, 57.0),
                (-0.85, 59.0),
                (-0.4, 65.0),
                (0.0, 68.0),
                (1.0, 74.0),
            ]),
        ),
        (
            0.5,
            pv(&[(-1.0, 57.0), (-0.85, 59.0), (0.0, 67.0), (1.0, 75.0)]),
        ),
        (
            0.7,
            pv(&[(-1.0, 58.0), (-0.85, 60.0), (0.0, 64.0), (1.0, 66.0)]),
        ),
    ]);
    // Mid inland: rolling hills, mountains where erosion is low.
    let mid = erosion(vec![
        (
            -1.0,
            pv(&[
                (-1.0, 57.0),
                (-0.85, 59.0),
                (-0.4, 92.0),
                (0.0, 135.0),
                (0.4, 180.0),
                (0.7, 210.0),
                (1.0, 222.0),
            ]),
        ),
        (
            -0.5,
            pv(&[
                (-1.0, 57.0),
                (-0.85, 59.0),
                (-0.4, 82.0),
                (0.0, 108.0),
                (0.4, 128.0),
                (1.0, 145.0),
            ]),
        ),
        (
            -0.3,
            pv(&[
                (-1.0, 57.0),
                (-0.85, 59.0),
                (-0.4, 76.0),
                (-0.2, 100.0),
                (0.2, 112.0),
                (1.0, 125.0),
            ]),
        ),
        (
            0.0,
            pv(&[
                (-1.0, 57.0),
                (-0.85, 59.0),
                (-0.4, 71.0),
                (0.0, 80.0),
                (1.0, 95.0),
            ]),
        ),
        (
            0.3,
            pv(&[
                (-1.0, 57.0),
                (-0.85, 59.0),
                (-0.4, 68.0),
                (0.0, 73.0),
                (1.0, 84.0),
            ]),
        ),
        (
            0.5,
            pv(&[(-1.0, 57.0), (-0.85, 59.0), (0.0, 80.0), (1.0, 96.0)]),
        ),
        (
            0.7,
            pv(&[(-1.0, 58.0), (-0.85, 60.0), (0.0, 64.0), (1.0, 67.0)]),
        ),
    ]);
    // Far inland: plateaus and the high peaks.
    let far = |top: f64| {
        erosion(vec![
            (
                -1.0,
                pv(&[
                    (-1.0, 58.0),
                    (-0.85, 59.0),
                    (-0.4, 110.0),
                    (0.0, 165.0),
                    (0.4, 210.0),
                    (0.7, top - 6.0),
                    (1.0, top),
                ]),
            ),
            (
                -0.5,
                pv(&[
                    (-1.0, 57.0),
                    (-0.85, 59.0),
                    (-0.4, 98.0),
                    (0.0, 140.0),
                    (0.4, 170.0),
                    (0.7, 195.0),
                    (1.0, 210.0),
                ]),
            ),
            (
                -0.3,
                pv(&[
                    (-1.0, 57.0),
                    (-0.85, 59.0),
                    (-0.4, 92.0),
                    (-0.2, 120.0),
                    (0.2, 135.0),
                    (1.0, 150.0),
                ]),
            ),
            (
                0.0,
                pv(&[
                    (-1.0, 57.0),
                    (-0.85, 59.0),
                    (-0.4, 76.0),
                    (0.0, 92.0),
                    (1.0, 110.0),
                ]),
            ),
            (
                0.3,
                pv(&[
                    (-1.0, 57.0),
                    (-0.85, 59.0),
                    (-0.4, 70.0),
                    (0.0, 78.0),
                    (1.0, 95.0),
                ]),
            ),
            (
                0.5,
                pv(&[(-1.0, 57.0), (-0.85, 59.0), (0.0, 86.0), (1.0, 105.0)]),
            ),
            (
                0.7,
                pv(&[(-1.0, 58.0), (-0.85, 60.0), (0.0, 64.0), (1.0, 67.0)]),
            ),
        ])
    };
    let flat = |y: f64| erosion(vec![(0.0, pv(&[(0.0, y)]))]);
    Spline::new(vec![
        // Mushroom islands rise out of the deepest sea, as in vanilla.
        (-1.2, flat(71.0)),
        (-1.08, flat(67.0)),
        (-1.02, flat(42.0)),
        // Deep ocean, ocean, then the shelf up to the coast.
        (-0.8, flat(38.0)),
        (-0.455, flat(45.0)),
        (-0.4, flat(49.0)),
        (-0.22, flat(55.0)),
        (-0.19, flat(57.0)),
        (-0.11, near.clone()),
        (0.03, mid.clone()),
        (0.3, far(230.0)),
        (1.0, far(236.0)),
    ])
}

/// The factor: how strongly the land holds to the offset, by erosion
/// (mountains loose, worn land tight), tighter in valleys and under the
/// sea.
pub fn factor(c: &Climate) -> f64 {
    let by_erosion = Spline::of(&[
        (-1.0, 0.035),
        (-0.5, 0.05),
        (-0.3, 0.07),
        (-0.2, 0.15),
        (0.05, 0.18),
        (0.45, 0.2),
        (0.5, 0.12),
        (0.6, 0.35),
    ])
    .eval(c.erosion, &[]);
    let valley = smooth(-0.6, -0.9, c.peaks());
    let sea = smooth(-0.11, -0.19, c.continent);
    let tight = by_erosion + (0.5 - by_erosion).max(0.0) * valley;
    tight + (0.25 - tight) * sea
}

/// The jaggedness: up to this many blocks of sharp extra height, only on
/// peaks (high PV) where erosion is low, inland.
pub fn jaggedness(c: &Climate) -> f64 {
    let peaks = smooth(0.4, 1.0, c.peaks());
    let rough = smooth(-0.2, -0.78, c.erosion);
    let inland = smooth(-0.11, 0.1, c.continent);
    18.0 * peaks * rough * inland
}

fn smooth(edge0: f64, edge1: f64, x: f64) -> f64 {
    let t = ((x - edge0) / (edge1 - edge0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// The shapes of columns, with the offset spline built once.
pub struct Shaper {
    offset: Spline,
}

impl Default for Shaper {
    fn default() -> Self {
        Self::new()
    }
}

impl Shaper {
    pub fn new() -> Self {
        Self {
            offset: offset_spline(),
        }
    }

    pub fn shape(&self, c: &Climate) -> Shape {
        Shape {
            offset: self.offset.eval(c.continent, &[c.erosion, c.peaks()]),
            factor: factor(c),
            jaggedness: jaggedness(c),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn c(continent: f64, erosion: f64, weirdness: f64) -> Climate {
        Climate {
            continent,
            erosion,
            weirdness,
            ..Default::default()
        }
    }

    #[test]
    fn the_land_rises_inland_falls_with_erosion_and_rises_with_pv() {
        let s = Shaper::new();
        let y = |cl: Climate| s.shape(&cl).offset;
        // PV 0.3 at weirdness about 0.43.
        assert!(y(c(-0.6, 0.0, 0.43)) < y(c(-0.3, 0.0, 0.43)));
        assert!(y(c(-0.3, 0.0, 0.43)) < y(c(0.1, 0.0, 0.43)));
        assert!(y(c(0.5, -0.9, 0.43)) > y(c(0.5, 0.3, 0.43)));
        assert!(y(c(0.5, -0.4, 0.66)) > y(c(0.5, -0.4, 0.3)));
    }

    #[test]
    fn heights_land_in_their_target_bands() {
        let s = Shaper::new();
        let y = |cl: Climate| s.shape(&cl).offset;
        assert!((35.0..=45.0).contains(&y(c(-0.7, 0.0, 0.4))), "deep ocean");
        assert!((48.0..=56.0).contains(&y(c(-0.3, 0.0, 0.4))), "ocean");
        assert!((60.0..=68.0).contains(&y(c(-0.15, 0.2, 0.4))), "coast");
        assert!(
            (64.0..=75.0).contains(&y(c(-0.05, 0.5, 0.4))),
            "near inland, eroded"
        );
        assert!((70.0..=110.0).contains(&y(c(0.2, 0.0, 0.4))), "rolling");
        assert!((110.0..=160.0).contains(&y(c(0.5, -0.3, 0.6))), "plateau");
        assert!((180.0..=245.0).contains(&y(c(0.5, -0.9, 0.66))), "peaks");
        // A river valley anywhere inland: under sea level.
        for cont in [-0.05, 0.1, 0.5] {
            for e in [-0.9, -0.3, 0.2] {
                assert!(y(c(cont, e, 0.0)) <= 59.0, "valley at {cont} {e}");
            }
        }
    }

    #[test]
    fn jaggedness_only_on_low_erosion_peaks() {
        assert!(jaggedness(&c(0.5, -0.9, 0.66)) > 10.0);
        assert_eq!(jaggedness(&c(0.5, 0.3, 0.66)), 0.0);
        assert_eq!(jaggedness(&c(0.5, -0.9, 0.2)), 0.0);
        assert!(factor(&c(0.5, -0.9, 0.66)) < factor(&c(0.2, 0.3, 0.3)));
    }
}
