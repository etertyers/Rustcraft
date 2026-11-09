//! A small, slow, test-only interpreter of vanilla's density-function JSON
//! (the 1.21.11 data fetched by `scripts/fetch-vanilla-assets.sh` into
//! `assets/vanilla/data/`, never committed), to check the caves' port term
//! by term. It uses Rustcraft's own noise sampler, so only the formulas and
//! the noise settings are compared, not vanilla's seeding. Where the JSON
//! asks for `overworld/sloped_cheese` (the terrain, which Rustcraft shapes
//! with its own climate splines rather than vanilla's router) the value
//! the caller gives is used.

use std::{cell::RefCell, collections::HashMap, path::PathBuf, rc::Rc};

use serde_json::Value;

use super::{caves::NOISE_NAMES, normal_noise::NormalNoise};

/// Where a function is evaluated.
#[derive(Clone, Copy)]
pub struct At {
    pub x: f64,
    pub y: f64,
    pub z: f64,
    /// The value `overworld/sloped_cheese` stands for.
    pub sloped: f64,
}

pub struct Reference {
    root: PathBuf,
    seed: u64,
    noises: RefCell<HashMap<String, Rc<NormalNoise>>>,
    files: RefCell<HashMap<PathBuf, Rc<Value>>>,
}

fn short(name: &str) -> &str {
    name.strip_prefix("minecraft:").unwrap_or(name)
}

impl Reference {
    /// The evaluator, if the vanilla data is on disk.
    pub fn load(seed: u64) -> Option<Self> {
        let root = crate::assets::dir().join("vanilla/data/minecraft/worldgen");
        root.join("noise_settings/overworld.json")
            .is_file()
            .then(|| Self {
                root,
                seed,
                noises: RefCell::default(),
                files: RefCell::default(),
            })
    }

    fn file(&self, path: PathBuf) -> Rc<Value> {
        self.files
            .borrow_mut()
            .entry(path.clone())
            .or_insert_with(|| {
                let text = std::fs::read_to_string(&path)
                    .unwrap_or_else(|e| panic!("{}: {e}", path.display()));
                Rc::new(
                    serde_json::from_str(&text)
                        .unwrap_or_else(|e| panic!("{}: {e}", path.display())),
                )
            })
            .clone()
    }

    /// The noise `name`, with the octaves its JSON gives.
    pub fn noise(&self, name: &str) -> Rc<NormalNoise> {
        let name = short(name);
        if let Some(found) = self.noises.borrow().get(name) {
            return found.clone();
        }
        let json = self.file(self.root.join(format!("noise/{name}.json")));
        let first = json["firstOctave"].as_i64().expect("firstOctave") as i32;
        let amplitudes: Vec<f64> = json["amplitudes"]
            .as_array()
            .expect("amplitudes")
            .iter()
            .map(|a| a.as_f64().expect("an amplitude"))
            .collect();
        let index = NOISE_NAMES
            .iter()
            .position(|n| *n == name)
            .unwrap_or_else(|| panic!("no stream for noise {name}")) as u64;
        let noise = Rc::new(NormalNoise::new(self.seed, 100 + index, first, &amplitudes));
        self.noises
            .borrow_mut()
            .insert(name.to_owned(), noise.clone());
        noise
    }

    /// The density function `name` (`overworld/caves/entrances`) at `at`.
    pub fn function(&self, name: &str, at: At) -> f64 {
        match short(name) {
            "y" => at.y,
            "overworld/sloped_cheese" => at.sloped,
            "zero" => 0.0,
            other => {
                let json = self.file(self.root.join(format!("density_function/{other}.json")));
                self.eval(&json, at)
            }
        }
    }

    /// The overworld's `final_density`, from its noise settings.
    pub fn final_density(&self, at: At) -> f64 {
        let json = self.file(self.root.join("noise_settings/overworld.json"));
        self.eval(&json["noise_router"]["final_density"], at)
    }

    fn mapper(kind: &str, v: f64) -> f64 {
        match kind {
            // The 3D spaghetti's: 0.75, 1, 1.5 and 2 at −0.5, 0 and 0.5.
            "type_1" => match v {
                v if v < -0.5 => 0.75,
                v if v < 0.0 => 1.0,
                v if v < 0.5 => 1.5,
                _ => 2.0,
            },
            // The 2D spaghetti's: 0.5, 0.75, 1, 2 and 3 at −0.75, −0.5, 0.5, 0.75.
            "type_2" => match v {
                v if v < -0.75 => 0.5,
                v if v < -0.5 => 0.75,
                v if v < 0.5 => 1.0,
                v if v < 0.75 => 2.0,
                _ => 3.0,
            },
            other => panic!("rarity mapper {other}"),
        }
    }

    fn eval(&self, node: &Value, at: At) -> f64 {
        let arg = |key: &str| self.eval(&node[key], at);
        match node {
            Value::Number(n) => n.as_f64().expect("a number"),
            Value::String(name) => self.function(name, at),
            Value::Object(_) => match short(node["type"].as_str().expect("a type")) {
                "add" => arg("argument1") + arg("argument2"),
                "mul" => arg("argument1") * arg("argument2"),
                "min" => arg("argument1").min(arg("argument2")),
                "max" => arg("argument1").max(arg("argument2")),
                "abs" => arg("argument").abs(),
                "square" => arg("argument").powi(2),
                "cube" => arg("argument").powi(3),
                "half_negative" => {
                    let v = arg("argument");
                    if v > 0.0 { v } else { v / 2.0 }
                }
                "quarter_negative" => {
                    let v = arg("argument");
                    if v > 0.0 { v } else { v / 4.0 }
                }
                "squeeze" => {
                    let c = arg("argument").clamp(-1.0, 1.0);
                    c / 2.0 - c * c * c / 24.0
                }
                "clamp" => arg("input").clamp(
                    node["min"].as_f64().expect("min"),
                    node["max"].as_f64().expect("max"),
                ),
                "y_clamped_gradient" => {
                    let (y0, y1) = (
                        node["from_y"].as_f64().expect("from_y"),
                        node["to_y"].as_f64().expect("to_y"),
                    );
                    let (v0, v1) = (
                        node["from_value"].as_f64().expect("from_value"),
                        node["to_value"].as_f64().expect("to_value"),
                    );
                    let t = ((at.y - y0) / (y1 - y0)).clamp(0.0, 1.0);
                    v0 + (v1 - v0) * t
                }
                "range_choice" => {
                    let v = arg("input");
                    let (lo, hi) = (
                        node["min_inclusive"].as_f64().expect("min_inclusive"),
                        node["max_exclusive"].as_f64().expect("max_exclusive"),
                    );
                    if v >= lo && v < hi {
                        arg("when_in_range")
                    } else {
                        arg("when_out_of_range")
                    }
                }
                "noise" => {
                    let (xz, y) = (
                        node["xz_scale"].as_f64().expect("xz_scale"),
                        node["y_scale"].as_f64().expect("y_scale"),
                    );
                    self.noise(node["noise"].as_str().expect("a noise")).get(
                        at.x * xz,
                        at.y * y,
                        at.z * xz,
                    )
                }
                "weird_scaled_sampler" => {
                    let r = Self::mapper(
                        node["rarity_value_mapper"].as_str().expect("a mapper"),
                        arg("input"),
                    );
                    let noise = self.noise(node["noise"].as_str().expect("a noise"));
                    r * noise.get(at.x / r, at.y / r, at.z / r).abs()
                }
                // Caching and interpolation change when a value is worked
                // out, never what it is; this evaluates exactly.
                "interpolated" | "cache_2d" | "cache_once" | "flat_cache" | "cache_all_in_cell"
                | "blend_density" => arg("argument"),
                "blend_alpha" => 1.0,
                "blend_offset" | "beardifier" => 0.0,
                other => panic!("density function type {other} isn't supported"),
            },
            other => panic!("not a density function: {other}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::worldgen::{caves::Caves, normal_noise::NormalNoise as Noise, rng::Rng};

    const SEED: u64 = 12345;

    /// The data is only on disk after `scripts/fetch-vanilla-assets.sh`.
    fn reference() -> Option<Reference> {
        let found = Reference::load(SEED);
        if found.is_none() {
            eprintln!("skipped: run scripts/fetch-vanilla-assets.sh for vanilla's world-gen data");
        }
        found
    }

    #[test]
    fn the_caves_match_vanillas_density_functions_at_20000_random_points() {
        let Some(reference) = reference() else { return };
        let caves = Caves::new(SEED);
        let mut rng = Rng::new(77);
        let mut worst: [(f64, String); 6] = Default::default();
        for _ in 0..20_000 {
            let at = At {
                x: ((rng.next_f64() - 0.5) * 60_000.0).floor(),
                y: f64::from(rng.below(385)) - 64.0,
                z: ((rng.next_f64() - 0.5) * 60_000.0).floor(),
                sloped: rng.next_f64() * 6.0 - 2.0,
            };
            let (x, y, z) = (at.x, at.y, at.z);
            let checks = [
                (
                    "entrances",
                    caves.entrances(x, y, z),
                    "overworld/caves/entrances",
                ),
                (
                    "spaghetti_2d",
                    caves.spaghetti_2d(x, y, z),
                    "overworld/caves/spaghetti_2d",
                ),
                (
                    "spaghetti_roughness_function",
                    caves.roughness(x, y, z),
                    "overworld/caves/spaghetti_roughness_function",
                ),
                ("noodle", caves.noodle(x, y, z), "overworld/caves/noodle"),
                ("pillars", caves.pillars(x, y, z), "overworld/caves/pillars"),
            ];
            for (i, (name, ours, vanilla)) in checks.iter().enumerate() {
                let want = reference.function(vanilla, at);
                let diff = (ours - want).abs();
                if diff > worst[i].0 {
                    worst[i] = (
                        diff,
                        format!("{name} at {x} {y} {z}: ours {ours}, vanilla {want}"),
                    );
                }
            }
            let ours = caves.final_density(at.sloped, x as i32, y as i32, z as i32);
            let want = reference.final_density(at);
            let diff = (ours - want).abs();
            if diff > worst[5].0 {
                worst[5] = (
                    diff,
                    format!(
                        "final density at {x} {y} {z} (sloped {}): ours {ours}, vanilla {want}",
                        at.sloped
                    ),
                );
            }
        }
        for (diff, what) in &worst {
            assert!(*diff < 1e-6, "{what} (off by {diff})");
        }
    }

    #[test]
    fn every_noise_setting_matches_vanillas_json() {
        let Some(reference) = reference() else { return };
        let caves = Caves::new(SEED);
        let mut rng = Rng::new(5);
        for name in NOISE_NAMES {
            let from_json = reference.noise(name);
            let ours = caves.noise_named(name);
            for _ in 0..50 {
                let [x, y, z] = [0; 3].map(|_| (rng.next_f64() - 0.5) * 20_000.0);
                assert_eq!(from_json.get(x, y, z), ours.get(x, y, z), "{name}");
            }
        }
    }

    #[test]
    fn the_value_factor_follows_the_formula() {
        assert!((Noise::value_factor(&[0.4, 0.5, 1.0]) - 1.25).abs() < 1e-12);
        assert!((Noise::value_factor(&[1.0]) - 0.833_333_333_333_333_3).abs() < 1e-12);
        let cheese = [0.5, 1.0, 2.0, 1.0, 2.0, 1.0, 0.0, 2.0, 0.0];
        let want = (1.0 / 6.0) / (0.1 * (1.0 + 1.0 / 8.0));
        assert!((Noise::value_factor(&cheese) - want).abs() < 1e-12);
    }

    #[test]
    fn every_cave_noise_has_the_deviation_its_octaves_predict() {
        let Some(reference) = reference() else { return };
        let mut rng = Rng::new(9);
        // The deviation of one improved-Perlin octave, measured.
        let one = Noise::new(SEED, 900, 0, &[1.0]);
        let sample = |noise: &Noise, rng: &mut Rng| {
            let n = 100_000;
            let (mut sum, mut square) = (0.0, 0.0);
            for _ in 0..n {
                let [x, y, z] = [0; 3].map(|_| (rng.next_f64() - 0.5) * 4_000_000.0);
                let v = noise.get(x, y, z);
                sum += v;
                square += v * v;
            }
            let mean = sum / n as f64;
            (square / n as f64 - mean * mean).sqrt()
        };
        let unit = sample(&one, &mut rng) / one.deviation_in_octaves();
        for name in NOISE_NAMES {
            let noise = reference.noise(name);
            let (got, want) = (
                sample(&noise, &mut rng),
                noise.deviation_in_octaves() * unit,
            );
            assert!(
                (got / want - 1.0).abs() < 0.05,
                "{name}: {got} against {want}"
            );
        }
    }
}
