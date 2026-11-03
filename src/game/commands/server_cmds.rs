//! `/tick`, `/worldborder` and `/forceload` (M45).

use super::*;

pub(super) fn tick_cmd(
    game: &mut Game,
    caller: &Caller,
    args: &[&str],
) -> Result<Vec<Reply>, String> {
    let usage =
        "Usage: /tick (query | rate <rate> | freeze | unfreeze | step [time] | sprint <time|stop>)";
    match args.first().copied().ok_or(usage)? {
        "query" => {
            let target = game.tick_rate;
            let state = if game.frozen {
                "The game is frozen"
            } else if game.tick_sprint > 0 {
                "The game is sprinting"
            } else {
                "The game is running normally"
            };
            caller.report(target as i32);
            Ok(vec![
                Reply::Sender(state.to_owned()),
                Reply::Sender(format!(
                    "Target tick rate: {target:.1} per second. Average time per tick: {:.1}ms (Target: {:.1}ms)",
                    game.tick_ms,
                    1000.0 / target
                )),
            ])
        }
        "rate" => {
            let rate: f32 = number(args.get(1).ok_or(usage)?, "rate")?;
            if !(1.0..=10_000.0).contains(&rate) {
                return Err("Rate must be between 1 and 10000".to_owned());
            }
            game.tick_rate = rate;
            caller.report(rate as i32);
            Ok(tell(format!(
                "Set the target tick rate to {rate:.1} per second"
            )))
        }
        "freeze" => {
            game.frozen = true;
            Ok(tell("The game is frozen".to_owned()))
        }
        "unfreeze" => {
            game.frozen = false;
            Ok(tell("The game is running normally".to_owned()))
        }
        "step" => {
            let n = match args.get(1) {
                Some(t) => extra::parse_time(t)?,
                None => 1,
            };
            game.frozen = true;
            game.tick_steps += n;
            caller.report(n as i32);
            Ok(tell(format!("Stepping {n} tick(s)")))
        }
        "sprint" => match args.get(1).copied().ok_or(usage)? {
            "stop" => {
                if game.tick_sprint == 0 {
                    return Err("No sprint is in progress".to_owned());
                }
                game.tick_sprint = 0;
                Ok(tell("Sprint stopped".to_owned()))
            }
            t => {
                let n = extra::parse_time(t)?;
                game.frozen = false;
                game.tick_sprint = n;
                caller.report(n as i32);
                Ok(tell(format!("Starting a sprint of {n} ticks")))
            }
        },
        _ => Err(usage.to_owned()),
    }
}

pub(super) fn tick_tab(_: &Game, before: &[&str]) -> Vec<String> {
    match (before.first().copied(), before.len()) {
        (_, 0) => ["query", "rate", "freeze", "unfreeze", "step", "sprint"]
            .map(str::to_owned)
            .to_vec(),
        (Some("sprint"), 1) => vec!["stop".to_owned()],
        _ => Vec::new(),
    }
}

/// How long a `/worldborder` change takes, in ticks, from a seconds word.
fn seconds(word: Option<&&str>) -> Result<u64, String> {
    match word {
        None => Ok(0),
        Some(w) => {
            let s: f64 = number(w, "time")?;
            if s < 0.0 {
                return Err("Time must not be negative".to_owned());
            }
            Ok((s * 20.0) as u64)
        }
    }
}

fn blocks(n: f64) -> String {
    if n.fract() == 0.0 {
        format!("{n:.0}")
    } else {
        format!("{n:.1}")
    }
}

pub(super) fn worldborder(
    game: &mut Game,
    caller: &Caller,
    args: &[&str],
) -> Result<Vec<Reply>, String> {
    let usage = "Usage: /worldborder (set|add|center|damage|warning|get) ...";
    let now = game.time();
    let size = game.border.size_at(now);
    match args.first().copied().ok_or(usage)? {
        "get" => {
            caller.report(size.round() as i32);
            Ok(tell(format!(
                "The world border is currently {} blocks wide",
                blocks(size)
            )))
        }
        action @ ("set" | "add") => {
            let amount: f64 = number(args.get(1).ok_or(usage)?, "size")?;
            let ticks = seconds(args.get(2))?;
            let target = if action == "add" {
                size + amount
            } else {
                amount
            };
            if !(1.0..=crate::game::border::DEFAULT_SIZE).contains(&target) {
                return Err(if target < 1.0 {
                    "The world border cannot be smaller than 1 block wide".to_owned()
                } else {
                    "The world border cannot be bigger than 59999968 blocks wide".to_owned()
                });
            }
            if (target - size).abs() < f64::EPSILON && game.border.ticks == 0 {
                return Err("Nothing changed. The world border is already that size".to_owned());
            }
            game.border.resize(now, target, ticks);
            caller.report(target.round() as i32);
            let secs = ticks as f64 / 20.0;
            Ok(tell(if ticks == 0 {
                format!("Set the world border to {} blocks wide", blocks(target))
            } else if target > size {
                format!(
                    "Growing the world border to {} blocks wide over {} seconds",
                    blocks(target),
                    blocks(secs)
                )
            } else {
                format!(
                    "Shrinking the world border to {} blocks wide over {} seconds",
                    blocks(target),
                    blocks(secs)
                )
            }))
        }
        "center" => {
            let words = args.get(1..3).ok_or(usage)?;
            let x = coordinate(words[0], caller.pos.x)?;
            let z = coordinate(words[1], caller.pos.z)?;
            if x.abs() > 29_999_984.0 || z.abs() > 29_999_984.0 {
                return Err("The world border center is too far away".to_owned());
            }
            if game.border.center == [x, z] {
                return Err(
                    "Nothing changed. The world border is already centered there".to_owned(),
                );
            }
            game.border.center = [x, z];
            Ok(tell(format!(
                "Set the center of the world border to {x:.2}, {z:.2}"
            )))
        }
        "damage" => match (args.get(1).copied(), args.get(2)) {
            (Some("amount"), Some(v)) => {
                let n: f64 = number(v, "damage")?;
                if n < 0.0 {
                    return Err("Damage must not be negative".to_owned());
                }
                game.border.damage = n;
                Ok(tell(format!(
                    "Set the world border damage to {} per block each second",
                    blocks(n)
                )))
            }
            (Some("buffer"), Some(v)) => {
                let n: f64 = number(v, "distance")?;
                if n < 0.0 {
                    return Err("Distance must not be negative".to_owned());
                }
                game.border.buffer = n;
                Ok(tell(format!(
                    "Set the world border damage buffer to {} blocks",
                    blocks(n)
                )))
            }
            _ => Err("Usage: /worldborder damage (amount|buffer) <value>".to_owned()),
        },
        "warning" => match (args.get(1).copied(), args.get(2)) {
            (Some("distance"), Some(v)) => {
                let n: f64 = number(v, "distance")?;
                game.border.warning_distance = n.max(0.0);
                Ok(tell(format!(
                    "Set the world border warning distance to {} blocks",
                    blocks(n)
                )))
            }
            (Some("time"), Some(v)) => {
                let n: f64 = number(v, "time")?;
                game.border.warning_time = n.max(0.0);
                Ok(tell(format!(
                    "Set the world border warning time to {} seconds",
                    blocks(n)
                )))
            }
            _ => Err("Usage: /worldborder warning (distance|time) <value>".to_owned()),
        },
        _ => Err(usage.to_owned()),
    }
}

pub(super) fn worldborder_tab(_: &Game, before: &[&str]) -> Vec<String> {
    let pick = |o: &[&str]| o.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>();
    match (before.first().copied(), before.len()) {
        (_, 0) => pick(&["set", "add", "center", "damage", "warning", "get"]),
        (Some("damage"), 1) => pick(&["amount", "buffer"]),
        (Some("warning"), 1) => pick(&["distance", "time"]),
        (Some("center"), _) => pick(&["~"]),
        _ => Vec::new(),
    }
}

// ---- /forceload ----

fn chunk_of(word_x: &str, word_z: &str, caller: &Caller) -> Result<(i32, i32), String> {
    let x = coordinate(word_x, caller.pos.x)?.floor() as i32;
    let z = coordinate(word_z, caller.pos.z)?.floor() as i32;
    Ok((x.div_euclid(16), z.div_euclid(16)))
}

pub(super) fn forceload(
    game: &mut Game,
    caller: &Caller,
    args: &[&str],
) -> Result<Vec<Reply>, String> {
    let usage = "Usage: /forceload (add <from> [to] | remove (<from> [to] | all) | query [pos])";
    match args.first().copied().ok_or(usage)? {
        action @ ("add" | "remove") => {
            if action == "remove" && args.get(1) == Some(&"all") {
                let n = game.forced.len();
                game.forced.clear();
                game.sync_forced();
                return Ok(tell(format!("All {n} force loaded chunks were unmarked")));
            }
            let (x1, z1) = (args.get(1).ok_or(usage)?, args.get(2).ok_or(usage)?);
            let from = chunk_of(x1, z1, caller)?;
            let to = match (args.get(3), args.get(4)) {
                (Some(x), Some(z)) => chunk_of(x, z, caller)?,
                _ => from,
            };
            let (lo, hi) = (
                (from.0.min(to.0), from.1.min(to.1)),
                (from.0.max(to.0), from.1.max(to.1)),
            );
            let count = i64::from(hi.0 - lo.0 + 1) * i64::from(hi.1 - lo.1 + 1);
            if count > 256 {
                return Err(format!(
                    "Too many chunks specified (maximum 256, specified {count})"
                ));
            }
            let mut changed = 0;
            for cx in lo.0..=hi.0 {
                for cz in lo.1..=hi.1 {
                    let did = if action == "add" {
                        game.forced.insert((cx, cz))
                    } else {
                        game.forced.remove(&(cx, cz))
                    };
                    changed += usize::from(did);
                }
            }
            if changed == 0 {
                return Err(if action == "add" {
                    "No chunks were marked for force loading".to_owned()
                } else {
                    "No chunks were unmarked for force loading".to_owned()
                });
            }
            game.sync_forced();
            caller.report(changed as i32);
            Ok(tell(match (action, count) {
                ("add", 1) => format!("Marked chunk {}, {} to be force loaded", lo.0, lo.1),
                ("add", _) => format!(
                    "Marked {changed} chunks to be force loaded from {}, {} to {}, {}",
                    lo.0, lo.1, hi.0, hi.1
                ),
                (_, 1) => format!(
                    "Unmarked chunk {}, {} to no longer be force loaded",
                    lo.0, lo.1
                ),
                _ => format!(
                    "Unmarked {changed} chunks to no longer be force loaded from {}, {} to {}, {}",
                    lo.0, lo.1, hi.0, hi.1
                ),
            }))
        }
        "query" => match (args.get(1), args.get(2)) {
            (Some(x), Some(z)) => {
                let c = chunk_of(x, z, caller)?;
                if game.forced.contains(&c) {
                    caller.report(1);
                    Ok(tell(format!(
                        "Chunk {}, {} is marked for force loading",
                        c.0, c.1
                    )))
                } else {
                    Err(format!(
                        "Chunk {}, {} is not marked for force loading",
                        c.0, c.1
                    ))
                }
            }
            _ => {
                caller.report(game.forced.len() as i32);
                if game.forced.is_empty() {
                    return Ok(tell("No chunks are force loaded".to_owned()));
                }
                let list: Vec<String> = game
                    .forced
                    .iter()
                    .map(|(x, z)| format!("({x}, {z})"))
                    .collect();
                Ok(tell(format!(
                    "{} force loaded chunks were found: {}",
                    game.forced.len(),
                    list.join(", ")
                )))
            }
        },
        _ => Err(usage.to_owned()),
    }
}

pub(super) fn forceload_tab(_: &Game, before: &[&str]) -> Vec<String> {
    let pick = |o: &[&str]| o.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>();
    match (before.first().copied(), before.len()) {
        (_, 0) => pick(&["add", "remove", "query"]),
        (Some("remove"), 1) => pick(&["all", "~"]),
        _ => pick(&["~"]),
    }
}
