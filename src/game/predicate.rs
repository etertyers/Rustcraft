//! Predicates (M44): the JSON conditions `/execute if predicate`, selectors'
//! `predicate=` and data packs use. The game has: `random_chance`,
//! `time_check`, `weather_check`, `entity_properties` (type, nbt-lite),
//! `entity_scores`, `location_check` (block, biome, dimension), `inverted`,
//! `all_of` and `any_of`.

use glam::DVec3;
use serde_json::Value;

use super::{
    Game,
    selector::{Range, Target},
    weather::Weather,
};
use crate::world::dimension::Dimension;

#[derive(Debug, Clone, PartialEq)]
pub enum Predicate {
    RandomChance(f64),
    TimeCheck {
        period: Option<u64>,
        min: i64,
        max: i64,
    },
    Weather {
        raining: Option<bool>,
        thundering: Option<bool>,
    },
    EntityType(String),
    EntityScores(Vec<(String, Range)>),
    Block(Vec<String>),
    Biome(Vec<String>),
    DimensionIs(String),
    Inverted(Box<Predicate>),
    AllOf(Vec<Predicate>),
    AnyOf(Vec<Predicate>),
}

fn bare(id: &str) -> &str {
    id.strip_prefix("minecraft:").unwrap_or(id)
}

fn range(v: &Value) -> Option<(f64, f64)> {
    match v {
        Value::Number(n) => n.as_f64().map(|n| (n, n)),
        Value::Object(o) => Some((
            o.get("min").and_then(Value::as_f64).unwrap_or(f64::MIN),
            o.get("max").and_then(Value::as_f64).unwrap_or(f64::MAX),
        )),
        _ => None,
    }
}

impl Predicate {
    pub fn parse(json: &str) -> Result<Predicate, String> {
        let value: Value =
            serde_json::from_str(json).map_err(|e| format!("Invalid predicate: {e}"))?;
        Self::from_value(&value)
    }

    pub fn from_value(v: &Value) -> Result<Predicate, String> {
        let kind = v
            .get("condition")
            .and_then(Value::as_str)
            .ok_or("A predicate needs a condition")?;
        let terms = |key: &str| -> Result<Vec<Predicate>, String> {
            v.get(key)
                .and_then(Value::as_array)
                .ok_or_else(|| format!("{kind} needs {key}"))?
                .iter()
                .map(Self::from_value)
                .collect()
        };
        Ok(match bare(kind) {
            "random_chance" => Predicate::RandomChance(
                v.get("chance")
                    .and_then(Value::as_f64)
                    .ok_or("random_chance needs a chance")?,
            ),
            "time_check" => {
                let (min, max) = range(v.get("value").ok_or("time_check needs a value")?)
                    .ok_or("time_check's value must be a number or range")?;
                Predicate::TimeCheck {
                    period: v.get("period").and_then(Value::as_u64),
                    min: min.max(i64::MIN as f64) as i64,
                    max: max.min(i64::MAX as f64) as i64,
                }
            }
            "weather_check" => Predicate::Weather {
                raining: v.get("raining").and_then(Value::as_bool),
                thundering: v.get("thundering").and_then(Value::as_bool),
            },
            "entity_properties" => {
                let pred = v
                    .get("predicate")
                    .ok_or("entity_properties needs a predicate")?;
                match pred.get("type").and_then(Value::as_str) {
                    Some(t) => Predicate::EntityType(bare(t).to_owned()),
                    None => Predicate::AllOf(Vec::new()),
                }
            }
            "entity_scores" => {
                let scores = v
                    .get("scores")
                    .and_then(Value::as_object)
                    .ok_or("entity_scores needs scores")?;
                Predicate::EntityScores(
                    scores
                        .iter()
                        .map(|(name, r)| {
                            let (min, max) =
                                range(r).ok_or("A score range must be a number or range")?;
                            Ok((
                                name.clone(),
                                Range {
                                    min: (min > f64::MIN).then_some(min),
                                    max: (max < f64::MAX).then_some(max),
                                },
                            ))
                        })
                        .collect::<Result<_, String>>()?,
                )
            }
            "location_check" => {
                let pred = v
                    .get("predicate")
                    .ok_or("location_check needs a predicate")?;
                let mut all = Vec::new();
                if let Some(b) = pred.get("block").and_then(|b| b.get("blocks")) {
                    let list: Vec<String> = match b {
                        Value::String(s) => vec![bare(s).to_owned()],
                        Value::Array(a) => a
                            .iter()
                            .filter_map(Value::as_str)
                            .map(|s| bare(s).to_owned())
                            .collect(),
                        _ => Vec::new(),
                    };
                    all.push(Predicate::Block(list));
                }
                if let Some(b) = pred.get("biomes").or(pred.get("biome")) {
                    let list: Vec<String> = match b {
                        Value::String(s) => vec![bare(s).to_owned()],
                        Value::Array(a) => a
                            .iter()
                            .filter_map(Value::as_str)
                            .map(|s| bare(s).to_owned())
                            .collect(),
                        _ => Vec::new(),
                    };
                    all.push(Predicate::Biome(list));
                }
                if let Some(d) = pred.get("dimension").and_then(Value::as_str) {
                    all.push(Predicate::DimensionIs(bare(d).to_owned()));
                }
                Predicate::AllOf(all)
            }
            "inverted" => Predicate::Inverted(Box::new(Self::from_value(
                v.get("term").ok_or("inverted needs a term")?,
            )?)),
            "all_of" => Predicate::AllOf(terms("terms")?),
            "any_of" => Predicate::AnyOf(terms("terms")?),
            other => return Err(format!("Unknown predicate condition '{other}'")),
        })
    }

    /// Whether it holds for `executor` standing at `pos` in `dim`.
    pub fn test(&self, game: &mut Game, executor: Target, pos: DVec3, dim: Dimension) -> bool {
        match self {
            Predicate::RandomChance(p) => game.rng.next_f64() < *p,
            Predicate::TimeCheck { period, min, max } => {
                let t = game.day_time() as i64;
                let t = period.map_or(t, |p| t.rem_euclid(p.max(1) as i64));
                (*min..=*max).contains(&t)
            }
            Predicate::Weather {
                raining,
                thundering,
            } => {
                let rain = game.weather != Weather::Clear;
                let thunder = game.weather == Weather::Thunder;
                raining.is_none_or(|r| r == rain) && thundering.is_none_or(|t| t == thunder)
            }
            Predicate::EntityType(kind) => game
                .candidates(dim)
                .iter()
                .any(|c| c.target == executor && c.kind == *kind),
            Predicate::EntityScores(scores) => {
                let holder = match executor {
                    Target::Player(i) => game.player_at(i).name.clone(),
                    Target::Mob(id) => format!("entity.{id}"),
                };
                scores.iter().all(|(o, r)| {
                    game.board
                        .score(o, &holder)
                        .is_some_and(|v| r.contains(f64::from(v)))
                })
            }
            Predicate::Block(names) => {
                let cell = pos.floor().as_ivec3();
                game.chunks
                    .world()
                    .block(cell.x, cell.y, cell.z)
                    .is_some_and(|b| names.iter().any(|n| *n == b.def().name))
            }
            Predicate::Biome(names) => game
                .chunks
                .world()
                .biome_at(pos.x.floor() as i32, pos.z.floor() as i32)
                .is_some_and(|b| {
                    let id = b
                        .name()
                        .to_lowercase()
                        .replace([' ', '\''], "_")
                        .replace("__", "_");
                    names.contains(&id)
                }),
            Predicate::DimensionIs(name) => bare(dim.name()) == name,
            Predicate::Inverted(p) => !p.test(game, executor, pos, dim),
            Predicate::AllOf(ps) => ps.iter().all(|p| p.test(game, executor, pos, dim)),
            Predicate::AnyOf(ps) => ps.iter().any(|p| p.test(game, executor, pos, dim)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn predicates_parse_every_condition_and_reject_the_rest() {
        for json in [
            r#"{"condition":"minecraft:random_chance","chance":0.5}"#,
            r#"{"condition":"time_check","value":{"min":0,"max":6000},"period":24000}"#,
            r#"{"condition":"weather_check","raining":true}"#,
            r#"{"condition":"entity_properties","entity":"this","predicate":{"type":"minecraft:pig"}}"#,
            r#"{"condition":"entity_scores","entity":"this","scores":{"kills":{"min":1},"x":3}}"#,
            r#"{"condition":"location_check","predicate":{"block":{"blocks":["minecraft:stone"]},"dimension":"minecraft:overworld"}}"#,
            r#"{"condition":"inverted","term":{"condition":"random_chance","chance":1}}"#,
            r#"{"condition":"all_of","terms":[{"condition":"random_chance","chance":1}]}"#,
            r#"{"condition":"any_of","terms":[]}"#,
        ] {
            assert!(Predicate::parse(json).is_ok(), "{json}");
        }
        for bad in [
            r#"{"condition":"explode"}"#,
            r#"{"chance":1}"#,
            "nope",
            r#"{"condition":"inverted"}"#,
        ] {
            assert!(Predicate::parse(bad).is_err(), "{bad}");
        }
    }
}
