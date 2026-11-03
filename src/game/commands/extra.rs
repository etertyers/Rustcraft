//! The everyday commands M43 adds to M38's: chat and server housekeeping,
//! game modes, damage, advancements, sounds and particles, riding,
//! spreading players, and the host's `/op`, `/ban`, `/whitelist`.

use super::*;
use crate::{
    config::{MAX_Y, MIN_Y},
    entity::particle::Particle,
    game::{modes::Spectated, weather::Weather},
};

/// A duration: ticks, or a number with `t`, `s` or `d` (vanilla's time argument).
pub(super) fn parse_time(word: &str) -> Result<u32, String> {
    let (digits, unit) = match word.chars().last() {
        Some(c @ ('t' | 's' | 'd')) => (&word[..word.len() - 1], c),
        _ => (word, 't'),
    };
    let n: f32 = digits
        .parse()
        .map_err(|_| format!("Invalid time '{word}'"))?;
    let ticks = n * match unit {
        's' => 20.0,
        'd' => 24000.0,
        _ => 1.0,
    };
    if ticks < 0.0 {
        return Err("Time must not be negative".to_owned());
    }
    Ok(ticks as u32)
}

pub(super) fn say(game: &mut Game, caller: &Caller, args: &[&str]) -> Result<Vec<Reply>, String> {
    if args.is_empty() {
        return Err("Usage: /say <message>".to_owned());
    }
    let name = &game.player_at(caller.index).name;
    Ok(vec![Reply::Everyone(format!("[{name}] {}", rest(args, 0)))])
}

pub(super) fn weather(game: &mut Game, _: &Caller, args: &[&str]) -> Result<Vec<Reply>, String> {
    let word = args
        .first()
        .ok_or("Usage: /weather <clear|rain|thunder> [duration]")?;
    let kind = Weather::parse(word).ok_or_else(|| format!("Unknown weather: {word}"))?;
    game.weather = kind;
    game.weather_ticks = match args.get(1) {
        Some(d) => parse_time(d)?,
        None => crate::game::weather::DEFAULT_TICKS,
    };
    Ok(tell(format!("Changed the weather to {}", kind.id())))
}

pub(super) fn random(
    game: &mut Game,
    caller: &Caller,
    args: &[&str],
) -> Result<Vec<Reply>, String> {
    match args.first().copied() {
        Some(mode @ ("value" | "roll")) => {
            let range = Range::parse(args.get(1).ok_or("Usage: /random <value|roll> <range>")?)?;
            let (Some(lo), Some(hi)) = (range.min, range.max) else {
                return Err("Range must have both ends".to_owned());
            };
            let (lo, hi) = (lo as i64, hi as i64);
            if hi - lo + 1 > i64::from(i32::MAX) {
                return Err("Range too large".to_owned());
            }
            let n = lo + i64::from(game.rng.below((hi - lo + 1) as u32));
            if mode == "roll" {
                let name = game.player_at(caller.index).name.clone();
                Ok(vec![Reply::Everyone(format!(
                    "{name} rolled {n} ({lo}..{hi})"
                ))])
            } else {
                Ok(tell(format!("Rolled {n}")))
            }
        }
        Some("reset") => Ok(tell("Reset all random sequences".to_owned())),
        _ => Err("Usage: /random <value|roll|reset> ...".to_owned()),
    }
}

pub(super) fn save_all(game: &mut Game, _: &Caller, _: &[&str]) -> Result<Vec<Reply>, String> {
    game.save().map_err(|e| format!("Saving failed: {e}"))?;
    Ok(vec![
        Reply::Sender("Saving the game (this may take a moment!)".to_owned()),
        Reply::Sender("Saved the game".to_owned()),
    ])
}

pub(super) fn save_on(game: &mut Game, _: &Caller, _: &[&str]) -> Result<Vec<Reply>, String> {
    if game.autosave {
        return Err("Saving is already turned on".to_owned());
    }
    game.autosave = true;
    Ok(tell("Turned on world auto-saving".to_owned()))
}

pub(super) fn save_off(game: &mut Game, _: &Caller, _: &[&str]) -> Result<Vec<Reply>, String> {
    if !game.autosave {
        return Err("Saving is already turned off".to_owned());
    }
    game.autosave = false;
    Ok(tell("Turned off world auto-saving".to_owned()))
}

pub(super) fn defaultgamemode(
    game: &mut Game,
    _: &Caller,
    args: &[&str],
) -> Result<Vec<Reply>, String> {
    let word = args.first().ok_or("Usage: /defaultgamemode <mode>")?;
    let mode = GameMode::parse(word)
        .filter(|_| word.len() > 1 && !word.chars().all(|c| c.is_ascii_digit()))
        .ok_or_else(|| format!("Unknown game mode: {word}"))?;
    game.default_mode = Some(mode);
    Ok(tell(format!(
        "The default game mode is now {}",
        mode_name(mode)
    )))
}

pub(super) fn spectate(
    game: &mut Game,
    caller: &Caller,
    args: &[&str],
) -> Result<Vec<Reply>, String> {
    let spectator = match args.get(1) {
        Some(word) => game.players_named(caller, word)?[0],
        None => caller.index,
    };
    let name = game.player_at(spectator).name.clone();
    if !game.player_at(spectator).mode.is_spectator() {
        return Err(format!("{name} is not in spectator mode"));
    }
    let Some(word) = args.first() else {
        game.with_player(spectator, |g| g.player.spectating = None);
        return Ok(tell("No longer spectating an entity".to_owned()));
    };
    let [target] = game.targets(caller, word)?[..] else {
        return Err("Only one entity is allowed, but the selector allows more than one".to_owned());
    };
    let seen = match target {
        Target::Player(i) if i == spectator => return Err("Can't spectate yourself".to_owned()),
        Target::Player(i) => Spectated::Player(game.player_at(i).name.clone()),
        Target::Mob(id) => Spectated::Mob(id),
    };
    let shown = game.target_name(target);
    game.with_player(spectator, |g| g.player.spectating = Some(seen));
    Ok(tell(format!("Now spectating {shown}")))
}

/// Sets where a target looks, in vanilla's degrees (0 south, 90 west;
/// pitch positive down).
fn turn_target(game: &mut Game, target: Target, yaw: f64, pitch: f64) {
    let (yaw_rad, pitch_rad) = (
        wrap_degrees(yaw - 180.0).to_radians() as f32,
        (-pitch).to_radians() as f32,
    );
    match target {
        Target::Player(i) => game.with_player(i, |g| {
            g.player.camera.yaw = yaw_rad;
            g.player.camera.pitch = pitch_rad.clamp(-1.55, 1.55);
            g.player.turned = g.player.turned.wrapping_add(1);
        }),
        Target::Mob(id) => {
            if let Some(mob) = game.mobs.iter_mut().find(|m| m.id == id) {
                mob.yaw = yaw_rad;
                mob.head_yaw = yaw_rad;
                mob.pitch = pitch_rad;
            }
        }
    }
}

/// A target's vanilla yaw and pitch.
fn target_angles(game: &mut Game, target: Target) -> (f64, f64) {
    let dim = game.dimension;
    let me = game.candidates(dim);
    me.iter()
        .find(|c| c.target == target)
        .map_or((0.0, 0.0), |c| (c.yaw, c.pitch))
}

pub(super) fn rotate(
    game: &mut Game,
    caller: &Caller,
    args: &[&str],
) -> Result<Vec<Reply>, String> {
    let who = game.targets(
        caller,
        args.first()
            .ok_or("Usage: /rotate <targets> <yaw> <pitch>")?,
    )?;
    let mut replies = Vec::new();
    for target in who {
        let (yaw0, pitch0) = target_angles(game, target);
        let (yaw, pitch) = match args.get(1).copied() {
            Some("facing") => {
                let at = match args.get(2).copied() {
                    Some("entity") => {
                        let dest =
                            game.targets(caller, args.get(3).ok_or("Expected an entity")?)?;
                        let eyes = dest.first().and_then(|t| game.target_feet(*t));
                        eyes.ok_or("No entity was found")? + DVec3::Y * 1.62
                    }
                    _ => position(
                        args.get(2..5).ok_or("Expected a position")?,
                        game.feet_of(caller.index),
                    )?,
                };
                let from = game.target_feet(target).ok_or("No entity was found")? + DVec3::Y * 1.62;
                let d = at - from;
                let yaw = wrap_degrees(-(d.x.atan2(d.z)).to_degrees());
                let pitch = -(d.y.atan2(d.x.hypot(d.z))).to_degrees();
                (yaw, pitch)
            }
            Some(y) => {
                let rel = |w: &str, base: f64| -> Result<f64, String> {
                    match w.strip_prefix('~') {
                        Some("") => Ok(base),
                        Some(by) => Ok(base + number::<f64>(by, "rotation")?),
                        None => number(w, "rotation"),
                    }
                };
                let p = args.get(2).ok_or("Expected a pitch")?;
                (rel(y, yaw0)?, rel(p, pitch0)?.clamp(-90.0, 90.0))
            }
            None => return Err("Usage: /rotate <targets> <yaw> <pitch>".to_owned()),
        };
        turn_target(game, target, yaw, pitch);
        replies.push(Reply::Sender(format!(
            "Rotated {}",
            game.target_name(target)
        )));
    }
    Ok(replies)
}

pub(super) fn spreadplayers(
    game: &mut Game,
    caller: &Caller,
    args: &[&str],
) -> Result<Vec<Reply>, String> {
    if args.len() < 6 || (args[4] == "under" && args.len() < 8) {
        return Err(
            "Usage: /spreadplayers <x> <z> <spreadDistance> <maxRange> <respectTeams> <targets>"
                .to_owned(),
        );
    }
    let origin = game.feet_of(caller.index);
    let cx = coordinate(args[0], origin.x)?;
    let cz = coordinate(args[1], origin.z)?;
    let spread: f64 = number(args[2], "distance")?;
    let range: f64 = number(args[3], "range")?;
    if spread < 0.0 {
        return Err("The spread distance must be at least 0".to_owned());
    }
    if range < spread + 1.0 {
        return Err(format!(
            "Max range ({range}) must be at least the spread distance ({spread}) plus 1"
        ));
    }
    let at = if args[4] == "under" { 7 } else { 5 };
    let limit: Option<i32> = (args[4] == "under")
        .then(|| number(args[5], "height"))
        .transpose()?;
    let who = game.targets(caller, args.get(at).ok_or("Expected targets")?)?;
    let mut spots: Vec<DVec3> = Vec::new();
    for _ in &who {
        let mut chosen = None;
        for _ in 0..200 {
            let x = (cx + (game.rng.next_f64() * 2.0 - 1.0) * range).floor() + 0.5;
            let z = (cz + (game.rng.next_f64() * 2.0 - 1.0) * range).floor() + 0.5;
            let p = DVec3::new(x, 0.0, z);
            if spots.iter().any(|s| (s.x - x).hypot(s.z - z) < spread) {
                continue;
            }
            let top = limit.unwrap_or(MAX_Y);
            let world = game.chunks.world();
            let ground = (MIN_Y..=top).rev().find(|&y| {
                world
                    .block(x as i32, y, z as i32)
                    .is_some_and(|b| !b.is_air() && b.def().name != "water")
            });
            if let Some(y) = ground {
                chosen = Some(DVec3::new(p.x, f64::from(y) + 1.0, p.z));
                break;
            }
        }
        spots.push(chosen.ok_or_else(|| {
            format!(
                "Could not spread {} entities around {cx:.1}, {cz:.1} (too many entities for space - try spreading less)",
                who.len()
            )
        })?);
    }
    let dimension = game.player_at(caller.index).dimension;
    for (target, spot) in who.iter().zip(&spots) {
        match *target {
            Target::Player(i) => {
                let (yaw, pitch) = (game.player_at(i).camera.yaw, game.player_at(i).camera.pitch);
                if game.player_at(i).dimension == dimension {
                    game.with_player(i, |g| g.teleport(*spot, yaw, pitch));
                }
            }
            Target::Mob(id) => {
                if let Some(m) = game.mobs.iter_mut().find(|m| m.id == id) {
                    m.body.set_position(*spot);
                }
            }
        }
    }
    let n = who.len();
    Ok(tell(format!(
        "Spread {n} {} around {cx:.1}, {cz:.1}",
        if n == 1 { "entity" } else { "entities" }
    )))
}

/// A damage type's vanilla id as a source.
fn damage_source(id: &str) -> Option<DamageSource> {
    Some(match id.strip_prefix("minecraft:").unwrap_or(id) {
        "generic" | "magic" | "indirect_magic" => DamageSource::Magic,
        "fall" => DamageSource::Fall,
        "drown" => DamageSource::Drowning,
        "lava" => DamageSource::Lava,
        "in_fire" => DamageSource::InFire,
        "on_fire" => DamageSource::Fire,
        "starve" => DamageSource::Starving,
        "mob_attack" => DamageSource::Mob,
        "arrow" => DamageSource::Arrow,
        "explosion" | "player_explosion" => DamageSource::Explosion,
        "anvil" | "falling_anvil" => DamageSource::Anvil,
        "hot_floor" => DamageSource::HotFloor,
        "fireball" => DamageSource::Fireball,
        "wither" => DamageSource::Wither,
        "fly_into_wall" => DamageSource::FlyIntoWall,
        "player_attack" => DamageSource::Player,
        "generic_kill" | "out_of_world" => DamageSource::Kill,
        _ => return None,
    })
}

pub(super) const DAMAGE_TYPES: &[&str] = &[
    "generic",
    "magic",
    "fall",
    "drown",
    "lava",
    "in_fire",
    "on_fire",
    "starve",
    "mob_attack",
    "arrow",
    "explosion",
    "falling_anvil",
    "hot_floor",
    "fireball",
    "wither",
    "fly_into_wall",
    "player_attack",
    "generic_kill",
    "out_of_world",
];

pub(super) fn damage(
    game: &mut Game,
    caller: &Caller,
    args: &[&str],
) -> Result<Vec<Reply>, String> {
    let (Some(who), Some(amount)) = (args.first(), args.get(1)) else {
        return Err("Usage: /damage <target> <amount> [type] [at <pos> | by <entity>]".to_owned());
    };
    let amount: f32 = number(amount, "amount")?;
    if amount < 0.0 {
        return Err("Float must not be less than 0".to_owned());
    }
    let source = match args.get(2) {
        Some(id) => damage_source(id).ok_or_else(|| format!("Unknown damage type: {id}"))?,
        None => DamageSource::Magic,
    };
    let from = match args.get(3).copied() {
        Some("at") => Some(position(
            args.get(4..7).ok_or("Expected a position")?,
            game.feet_of(caller.index),
        )?),
        Some("by") => {
            let by = game.targets(caller, args.get(4).ok_or("Expected an entity")?)?;
            by.first().and_then(|t| game.target_feet(*t))
        }
        _ => None,
    };
    let mut replies = Vec::new();
    for target in game.targets(caller, who)? {
        let name = game.target_name(target);
        let hurt = match target {
            Target::Player(i) => game.with_player(i, |g| {
                g.player.mode.is_vulnerable() && g.player.stats.damage(amount, source)
            }),
            Target::Mob(id) => game
                .mobs
                .iter_mut()
                .find(|m| m.id == id)
                .is_some_and(|m| m.hurt(amount, from, source)),
        };
        if !hurt {
            return Err("Unable to apply damage".to_owned());
        }
        replies.push(Reply::Sender(format!("Applied {amount} damage to {name}")));
    }
    Ok(replies)
}

/// The attributes the game has a number for, with their base values.
const ATTRIBUTES: &[(&str, f64)] = &[
    ("generic.max_health", 20.0),
    ("generic.movement_speed", 0.1),
    ("generic.attack_damage", 1.0),
    ("generic.attack_speed", 4.0),
    ("generic.armor", 0.0),
];

pub(super) fn attribute_names(_: &Game) -> Vec<String> {
    ATTRIBUTES.iter().map(|(n, _)| (*n).to_owned()).collect()
}

pub(super) fn attribute(
    game: &mut Game,
    caller: &Caller,
    args: &[&str],
) -> Result<Vec<Reply>, String> {
    let (Some(who), Some(name)) = (args.first(), args.get(1)) else {
        return Err(
            "Usage: /attribute <target> <attribute> <get|base get|base set> [value]".to_owned(),
        );
    };
    let name = name.strip_prefix("minecraft:").unwrap_or(name);
    let &(_, base) = ATTRIBUTES
        .iter()
        .find(|(n, _)| *n == name)
        .ok_or_else(|| format!("Unknown attribute: {name}"))?;
    let targets = game.targets(caller, who)?;
    let [Target::Player(index)] = targets[..] else {
        return Err("That entity has no such attribute".to_owned());
    };
    let shown = game.target_name(Target::Player(index));
    let speed_scale = game.player_at(index).speed_scale;
    let current = match name {
        "generic.movement_speed" => base * speed_scale,
        _ => base,
    };
    match (args.get(2).copied(), args.get(3).copied()) {
        (Some("get"), _) | (Some("base"), Some("get")) => Ok(tell(format!(
            "Attribute {name} for {shown} has a {} of {current}",
            if args[2] == "get" {
                "value"
            } else {
                "base value"
            }
        ))),
        (Some("base"), Some("set")) => {
            let value: f64 = number(args.get(4).ok_or("Expected a value")?, "value")?;
            if name != "generic.movement_speed" {
                return Err(format!("The game has no way to change {name}"));
            }
            if !(0.0..=1024.0).contains(&value) {
                return Err("Value must be between 0 and 1024".to_owned());
            }
            game.with_player(index, |g| g.player.speed_scale = value / base);
            Ok(tell(format!(
                "Base value of attribute {name} for {shown} set to {value}"
            )))
        }
        _ => {
            Err("Usage: /attribute <target> <attribute> <get|base get|base set> [value]".to_owned())
        }
    }
}

pub(super) fn advancement_ids(_: &Game) -> Vec<String> {
    crate::advancements::registry()
        .defs
        .iter()
        .map(|d| format!("minecraft:{}", d.id))
        .collect()
}

pub(super) fn advancement(
    game: &mut Game,
    caller: &Caller,
    args: &[&str],
) -> Result<Vec<Reply>, String> {
    let usage = "Usage: /advancement <grant|revoke> <targets> (everything|only|from|through|until) [advancement]";
    let (Some(action), Some(who), Some(how)) =
        (args.first().copied(), args.get(1), args.get(2).copied())
    else {
        return Err(usage.to_owned());
    };
    let grant = match action {
        "grant" => true,
        "revoke" => false,
        _ => return Err(usage.to_owned()),
    };
    let registry = crate::advancements::registry();
    let named = |w: &str| {
        let w = w.strip_prefix("minecraft:").unwrap_or(w);
        registry
            .index(w)
            .ok_or_else(|| format!("Unknown advancement: {w}"))
    };
    // The advancement indices the choice means: a tree is what hangs from one.
    let children = |root: usize| {
        let mut all = vec![root];
        let mut i = 0;
        while i < all.len() {
            let id = &registry.defs[all[i]].id;
            all.extend(
                registry
                    .defs
                    .iter()
                    .enumerate()
                    .filter(|(_, d)| d.parent.as_deref() == Some(id.as_str()))
                    .map(|(j, _)| j),
            );
            i += 1;
        }
        all
    };
    let ancestors = |leaf: usize| {
        let mut all = vec![leaf];
        while let Some(parent) = registry.defs[*all.last().unwrap()]
            .parent
            .as_deref()
            .and_then(|p| registry.index(p))
        {
            all.insert(0, parent);
        }
        all
    };
    let chosen: Vec<usize> = match how {
        "everything" => (0..registry.defs.len()).collect(),
        "only" => vec![named(args.get(3).ok_or(usage)?)?],
        "from" => children(named(args.get(3).ok_or(usage)?)?),
        "until" => ancestors(named(args.get(3).ok_or(usage)?)?),
        "through" => {
            let at = named(args.get(3).ok_or(usage)?)?;
            let mut all = ancestors(at);
            all.extend(children(at).into_iter().skip(1));
            all
        }
        _ => return Err(usage.to_owned()),
    };
    let mut replies = Vec::new();
    for index in game.players_named(caller, who)? {
        let name = game.player_at(index).name.clone();
        let changed = game.with_player(index, |g| {
            let mut changed = Vec::new();
            for &i in &chosen {
                let def = &registry.defs[i];
                if grant && g.player.progress.done.insert(def.id.clone()) {
                    for (c, _) in &def.criteria {
                        g.player
                            .progress
                            .criteria
                            .remove(&format!("{}#{c}", def.id));
                    }
                    changed.push(i);
                } else if !grant && g.player.progress.done.remove(&def.id) {
                    changed.push(i);
                }
            }
            changed
        });
        if changed.is_empty() {
            return Err(if grant {
                format!("Unable to grant any advancements to {name} (they already have them)")
            } else {
                format!("Unable to revoke any advancements from {name} (they don't have them)")
            });
        }
        let what = if changed.len() == 1 {
            format!("the advancement [{}]", registry.defs[changed[0]].title)
        } else {
            format!("{} advancements", changed.len())
        };
        replies.push(Reply::Sender(if grant {
            format!("Granted {what} to {name}")
        } else {
            format!("Revoked {what} from {name}")
        }));
    }
    Ok(replies)
}

/// Recipe ids: the names of what the book's recipes make.
pub(super) fn recipe_ids(game: &Game) -> Vec<String> {
    let mut ids: Vec<String> = crate::inventory::book::all(&game.items().recipes, game.items())
        .iter()
        .map(|r| r.result.item.name().to_owned())
        .collect();
    ids.sort();
    ids.dedup();
    ids
}

pub(super) fn recipe_names(game: &Game) -> Vec<String> {
    recipe_ids(game)
        .into_iter()
        .map(|r| format!("minecraft:{r}"))
        .collect()
}

pub(super) fn recipe(
    game: &mut Game,
    caller: &Caller,
    args: &[&str],
) -> Result<Vec<Reply>, String> {
    let usage = "Usage: /recipe <give|take> <targets> (*|recipe)";
    let (Some(action), Some(who), Some(which)) = (args.first().copied(), args.get(1), args.get(2))
    else {
        return Err(usage.to_owned());
    };
    if !matches!(action, "give" | "take") {
        return Err(usage.to_owned());
    }
    let all = recipe_ids(game);
    let chosen: Vec<String> = if *which == "*" {
        all
    } else {
        let id = which.strip_prefix("minecraft:").unwrap_or(which);
        if !all.iter().any(|r| r == id) {
            return Err(format!("Unknown recipe: {which}"));
        }
        vec![id.to_owned()]
    };
    let mut replies = Vec::new();
    for index in game.players_named(caller, who)? {
        let name = game.player_at(index).name.clone();
        let n = game.with_player(index, |g| {
            chosen
                .iter()
                .filter(|r| {
                    if action == "give" {
                        g.player.recipes.insert((*r).clone())
                    } else {
                        g.player.recipes.remove(*r)
                    }
                })
                .count()
        });
        if n == 0 {
            return Err(if action == "give" {
                format!("Could not unlock any recipes for {name} (they already know them)")
            } else {
                format!("Could not take any recipes from {name} (they don't know them)")
            });
        }
        replies.push(Reply::Sender(if action == "give" {
            format!("Unlocked {n} recipe(s) for {name}")
        } else {
            format!("Took {n} recipe(s) from {name}")
        }));
    }
    Ok(replies)
}

/// A particle's vanilla id and the color (0xRRGGBB) it is drawn in.
const PARTICLES: &[(&str, u32)] = &[
    ("flame", 0xFF8A1E),
    ("smoke", 0x555555),
    ("large_smoke", 0x444444),
    ("heart", 0xFF3B5C),
    ("happy_villager", 0x2ECC40),
    ("angry_villager", 0xB22222),
    ("note", 0x66FF33),
    ("crit", 0xE0D6A0),
    ("enchant", 0x9B6BFF),
    ("portal", 0xB04BFF),
    ("splash", 0x4A7DFF),
    ("bubble", 0xBFD8FF),
    ("cloud", 0xEEEEEE),
    ("lava", 0xFF5A00),
    ("firework", 0xFFE066),
    ("end_rod", 0xF2F2FF),
    ("witch", 0xA020F0),
    ("dust", 0xFF0000),
];

pub(super) fn particle_names(_: &Game) -> Vec<String> {
    PARTICLES.iter().map(|(n, _)| (*n).to_owned()).collect()
}

pub(super) fn particle(
    game: &mut Game,
    caller: &Caller,
    args: &[&str],
) -> Result<Vec<Reply>, String> {
    let name = args
        .first()
        .ok_or("Usage: /particle <name> [pos] [delta] [speed] [count]")?;
    let name = name.strip_prefix("minecraft:").unwrap_or(name);
    let &(_, color) = PARTICLES
        .iter()
        .find(|(n, _)| *n == name)
        .ok_or_else(|| format!("Unknown particle: {name}"))?;
    let origin = game.feet_of(caller.index);
    let at = match args.get(1..4) {
        Some(words) if args.len() >= 4 => position(words, origin)?,
        _ => origin,
    };
    let delta = match args.get(4..7) {
        Some(words) if args.len() >= 7 => position(words, DVec3::ZERO)?,
        _ => DVec3::ZERO,
    };
    let count: u32 = match args.get(8) {
        Some(n) => number(n, "count")?,
        None => 0,
    };
    let count = count.clamp(1, 500);
    let mut made = Vec::new();
    for _ in 0..count {
        let rng = &mut game.rng;
        let mut random = || rng.next_f64() as f32;
        let jitter = DVec3::new(
            (random() as f64 * 2.0 - 1.0) * delta.x,
            (random() as f64 * 2.0 - 1.0) * delta.y,
            (random() as f64 * 2.0 - 1.0) * delta.z,
        );
        made.push(Particle::colored(at + jitter, color, &mut random));
    }
    game.add_particles(made);
    Ok(tell(format!("Displaying particle minecraft:{name}")))
}

/// Sound events Tab offers (any event in the sound table plays).
pub(super) const SOUNDS: &[&str] = &[
    "entity.player.levelup",
    "entity.experience_orb.pickup",
    "entity.arrow.hit_player",
    "entity.creeper.primed",
    "entity.generic.explode",
    "entity.lightning_bolt.thunder",
    "entity.ender_dragon.growl",
    "entity.enderman.teleport",
    "entity.pig.ambient",
    "entity.cow.ambient",
    "entity.villager.yes",
    "entity.villager.no",
    "block.note_block.harp",
    "block.note_block.bell",
    "block.chest.open",
    "block.chest.close",
    "block.anvil.land",
    "block.bell.use",
    "block.portal.ambient",
    "block.stone.break",
    "block.grass.break",
    "block.wood.break",
    "block.glass.break",
    "block.water.ambient",
    "block.lava.pop",
    "ui.button.click",
    "ui.toast.challenge_complete",
    "music.game",
    "music.creative",
    "ambient.cave",
    "item.firecharge.use",
    "item.trident.thunder",
];

pub(super) fn playsound(
    game: &mut Game,
    caller: &Caller,
    args: &[&str],
) -> Result<Vec<Reply>, String> {
    let (Some(sound), Some(source), Some(who)) = (args.first(), args.get(1), args.get(2)) else {
        return Err(
            "Usage: /playsound <sound> <source> <targets> [pos] [volume] [pitch]".to_owned(),
        );
    };
    const SOURCES: &[&str] = &[
        "master", "music", "record", "weather", "block", "hostile", "neutral", "player", "ambient",
        "voice",
    ];
    if !SOURCES.contains(source) {
        return Err(format!("Unknown sound source: {source}"));
    }
    let event = sound.strip_prefix("minecraft:").unwrap_or(sound).to_owned();
    let targets = game.players_named(caller, who)?;
    let volume: f32 = args
        .get(7)
        .map(|v| number(v, "volume"))
        .transpose()?
        .unwrap_or(1.0);
    let pitch: f32 = args
        .get(8)
        .map(|v| number(v, "pitch"))
        .transpose()?
        .unwrap_or(1.0);
    if !(0.5..=2.0).contains(&pitch) && args.get(8).is_some() {
        return Err("Pitch must be between 0.5 and 2".to_owned());
    }
    let at = match args.get(3..6) {
        Some(words) if args.len() >= 6 => Some(position(words, game.feet_of(caller.index))?),
        _ => None,
    };
    let mut replies = Vec::new();
    for index in targets {
        let name = game.player_at(index).name.clone();
        game.with_player(index, |g| {
            g.player.heard.push(crate::audio::Cue {
                event: event.clone().into(),
                at,
                volume,
                pitch,
            });
        });
        replies.push(Reply::Sender(format!("Played sound {sound} to {name}")));
    }
    Ok(replies)
}

pub(super) fn stopsound(
    game: &mut Game,
    caller: &Caller,
    args: &[&str],
) -> Result<Vec<Reply>, String> {
    let who = args
        .first()
        .ok_or("Usage: /stopsound <targets> [source] [sound]")?;
    let targets = game.players_named(caller, who)?;
    for index in targets {
        let stop = args
            .get(2)
            .map_or_else(|| "*".to_owned(), |s| (*s).to_owned());
        game.with_player(index, |g| {
            g.player.heard.push(crate::audio::Cue {
                event: format!("stop:{stop}").into(),
                at: None,
                volume: 0.0,
                pitch: 1.0,
            });
        });
    }
    Ok(tell(match (args.get(1), args.get(2)) {
        (None, _) => "Stopped all sounds".to_owned(),
        (Some(src), None) => format!("Stopped all '{src}' sounds"),
        (_, Some(s)) => format!("Stopped sound '{s}'"),
    }))
}

pub(super) fn ride(game: &mut Game, caller: &Caller, args: &[&str]) -> Result<Vec<Reply>, String> {
    let usage = "Usage: /ride <target> (mount <vehicle> | dismount)";
    let (Some(who), Some(action)) = (args.first(), args.get(1).copied()) else {
        return Err(usage.to_owned());
    };
    let [target] = game.targets(caller, who)?[..] else {
        return Err("Only one entity is allowed, but the selector allows more than one".to_owned());
    };
    let name = game.target_name(target);
    match (action, target) {
        ("dismount", Target::Player(i)) => {
            let was = game.with_player(i, |g| g.player.mounted.take());
            if was.is_none() {
                return Err(format!("{name} is not riding anything"));
            }
            Ok(tell(format!("{name} is no longer riding anything")))
        }
        ("mount", Target::Player(i)) => {
            let [Target::Mob(id)] = game.targets(caller, args.get(2).ok_or(usage)?)?[..] else {
                return Err("The game can only seat players on mobs".to_owned());
            };
            if game.player_at(i).mounted.is_some() {
                return Err(format!("{name} is already riding something"));
            }
            let vehicle = game.target_name(Target::Mob(id));
            game.with_player(i, |g| g.player.mounted = Some(id));
            Ok(tell(format!("{name} started riding {vehicle}")))
        }
        ("mount" | "dismount", Target::Mob(_)) => {
            Err("The game can only seat players, not mobs, on vehicles".to_owned())
        }
        _ => Err(usage.to_owned()),
    }
}

// ---- the host's lists ----

/// A player named in a host command: online or not.
fn named_or_online(game: &mut Game, caller: &Caller, word: &str) -> Result<Vec<String>, String> {
    if word.starts_with('@') {
        Ok(game
            .players_named(caller, word)?
            .into_iter()
            .map(|i| game.player_at(i).name.clone())
            .collect())
    } else {
        Ok(vec![word.to_owned()])
    }
}

fn save_admin(game: &Game) {
    if let Some(saved) = &game.saved {
        game.admin.save(saved.dir.path());
    }
}

pub(super) fn op(game: &mut Game, caller: &Caller, args: &[&str]) -> Result<Vec<Reply>, String> {
    let names = named_or_online(game, caller, args.first().ok_or("Usage: /op <player>")?)?;
    let mut replies = Vec::new();
    for name in names {
        if !game.admin.ops.insert(name.clone()) {
            return Err("Nothing changed. The player is already an operator".to_owned());
        }
        replies.push(Reply::Sender(format!("Made {name} a server operator")));
    }
    save_admin(game);
    Ok(replies)
}

pub(super) fn deop(game: &mut Game, caller: &Caller, args: &[&str]) -> Result<Vec<Reply>, String> {
    let names = named_or_online(game, caller, args.first().ok_or("Usage: /deop <player>")?)?;
    let mut replies = Vec::new();
    for name in names {
        if !game.admin.ops.remove(&name) {
            return Err("Nothing changed. The player is not an operator".to_owned());
        }
        replies.push(Reply::Sender(format!(
            "Made {name} no longer a server operator"
        )));
    }
    save_admin(game);
    Ok(replies)
}

pub(super) fn ban(game: &mut Game, caller: &Caller, args: &[&str]) -> Result<Vec<Reply>, String> {
    let names = named_or_online(
        game,
        caller,
        args.first().ok_or("Usage: /ban <player> [reason]")?,
    )?;
    let reason = rest(args, 1);
    let mut replies = Vec::new();
    for name in names {
        if game.admin.banned.contains_key(&name) {
            return Err("Nothing changed. The player is already banned".to_owned());
        }
        game.admin.banned.insert(name.clone(), reason.clone());
        replies.push(Reply::Sender(format!(
            "Banned {name}: {}",
            if reason.is_empty() {
                "Banned by an operator."
            } else {
                &reason
            }
        )));
        if let Some(i) = (1..game.player_count()).find(|&i| game.player_at(i).name == name) {
            replies.push(Reply::Kick(i, "You are banned from this server".to_owned()));
        }
    }
    save_admin(game);
    Ok(replies)
}

pub(super) fn pardon(game: &mut Game, _: &Caller, args: &[&str]) -> Result<Vec<Reply>, String> {
    let name = args.first().ok_or("Usage: /pardon <player>")?;
    if game.admin.banned.remove(*name).is_none() {
        return Err("Nothing changed. The player isn't banned".to_owned());
    }
    save_admin(game);
    Ok(tell(format!("Unbanned {name}")))
}

pub(super) fn whitelist(
    game: &mut Game,
    caller: &Caller,
    args: &[&str],
) -> Result<Vec<Reply>, String> {
    let usage = "Usage: /whitelist <on|off|list|add|remove|reload> [player]";
    let reply = match args.first().copied() {
        Some("on") => {
            if game.admin.whitelist_on {
                return Err("Whitelist is already turned on".to_owned());
            }
            game.admin.whitelist_on = true;
            "Whitelist is now turned on".to_owned()
        }
        Some("off") => {
            if !game.admin.whitelist_on {
                return Err("Whitelist is already turned off".to_owned());
            }
            game.admin.whitelist_on = false;
            "Whitelist is now turned off".to_owned()
        }
        Some("list") => {
            let names: Vec<_> = game.admin.whitelist.iter().cloned().collect();
            if names.is_empty() {
                "There are no whitelisted players".to_owned()
            } else {
                format!(
                    "There are {} whitelisted player(s): {}",
                    names.len(),
                    names.join(", ")
                )
            }
        }
        Some("reload") => {
            if let Some(saved) = &game.saved {
                game.admin = crate::game::admin::Admin::load(saved.dir.path());
            }
            "Reloaded the whitelist".to_owned()
        }
        Some(action @ ("add" | "remove")) => {
            let name = named_or_online(game, caller, args.get(1).ok_or(usage)?)?.remove(0);
            if action == "add" {
                if !game.admin.whitelist.insert(name.clone()) {
                    return Err("Player is already whitelisted".to_owned());
                }
                format!("Added {name} to the whitelist")
            } else {
                if !game.admin.whitelist.remove(&name) {
                    return Err("Player is not whitelisted".to_owned());
                }
                format!("Removed {name} from the whitelist")
            }
        }
        _ => return Err(usage.to_owned()),
    };
    save_admin(game);
    Ok(tell(reply))
}

pub(super) fn publish(_: &mut Game, _: &Caller, args: &[&str]) -> Result<Vec<Reply>, String> {
    let port: u16 = match args.first() {
        Some(p) => number(p, "port")?,
        None => 0,
    };
    Ok(vec![Reply::Publish(port)])
}

pub(super) fn stop(_: &mut Game, _: &Caller, _: &[&str]) -> Result<Vec<Reply>, String> {
    Ok(vec![
        Reply::Sender("Stopping the server".to_owned()),
        Reply::Stop,
    ])
}

// ---- /teleport in full ----

/// Whether a word starts a coordinate rather than naming a target.
fn is_coordinate(word: &str) -> bool {
    word.starts_with(['~', '^', '-', '.'])
        || word.chars().next().is_some_and(|c| c.is_ascii_digit())
}

/// Moves `target` to `to` (a player in another dimension travels).
fn move_target(
    game: &mut Game,
    target: Target,
    to: DVec3,
    dimension: crate::world::dimension::Dimension,
) {
    match target {
        Target::Player(index) => {
            let (yaw, pitch) = (
                game.player_at(index).camera.yaw,
                game.player_at(index).camera.pitch,
            );
            if game.player_at(index).dimension == dimension {
                game.with_player(index, |g| g.teleport(to, yaw, pitch));
            } else {
                game.with_player(index, |g| g.change_dimension(dimension, Arrival::At(to)));
            }
        }
        Target::Mob(id) => {
            if let Some(mob) = game.mobs.iter_mut().find(|m| m.id == id) {
                mob.body.set_position(to);
            }
        }
    }
}

pub(super) fn tp(game: &mut Game, caller: &Caller, args: &[&str]) -> Result<Vec<Reply>, String> {
    let usage = "Usage: /tp [targets] (<destination> | <x y z> [<yaw> <pitch> | facing <x y z> | facing entity <target> [eyes|feet]])";
    if args.is_empty() {
        return Err(usage.to_owned());
    }
    let origin = game.feet_of(caller.index);
    let coordinate_first = args.len() >= 3 && args[..3].iter().all(|w| is_coordinate(w));
    let (who, rest): (Vec<Target>, &[&str]) = if args.len() == 1 || coordinate_first {
        (vec![Target::Player(caller.index)], args)
    } else {
        (game.targets(caller, args[0])?, &args[1..])
    };
    let dimension = game.player_at(caller.index).dimension;
    let (to, tail): (DVec3, &[&str]) =
        if rest.len() >= 3 && rest[..3].iter().all(|w| is_coordinate(w)) {
            (position(&rest[..3], origin)?, &rest[3..])
        } else {
            let dest = game.targets(caller, rest.first().ok_or(usage)?)?;
            let [only] = dest.as_slice() else {
                return Err(
                    "Only one entity is allowed, but the selector allows more than one".to_owned(),
                );
            };
            (
                game.target_feet(*only).ok_or("No entity was found")?,
                &rest[1..],
            )
        };
    // What the targets face afterwards, if told.
    enum Turn {
        None,
        Angles(f64, f64),
        Facing(DVec3),
        Entity(Target, bool),
    }
    let turn = match tail {
        [] => Turn::None,
        [yaw, pitch] => {
            let rel =
                |w: &str| -> Result<f64, String> { number(w.trim_start_matches('~'), "rotation") };
            let y = if *yaw == "~" { 0.0 } else { rel(yaw)? };
            let p = if *pitch == "~" { 0.0 } else { rel(pitch)? };
            if yaw.starts_with('~') || pitch.starts_with('~') {
                // Relative angles are added to each target's own below.
                Turn::Angles(f64::NAN.min(y), p)
            } else {
                Turn::Angles(y, p)
            }
        }
        ["facing", "entity", who, rest @ ..] => {
            let [only] = game.targets(caller, who)?[..] else {
                return Err(
                    "Only one entity is allowed, but the selector allows more than one".to_owned(),
                );
            };
            Turn::Entity(only, rest.first().copied() != Some("feet"))
        }
        ["facing", x, y, z] => Turn::Facing(position(&[*x, *y, *z], origin)?),
        _ => return Err(usage.to_owned()),
    };
    let mut replies = Vec::new();
    for target in &who {
        let name = game.target_name(*target);
        move_target(game, *target, to, dimension);
        match &turn {
            Turn::None => {}
            Turn::Angles(yaw, pitch) => {
                let (y0, p0) = target_angles(game, *target);
                let yaw = if yaw.is_nan() { y0 } else { *yaw };
                let _ = p0;
                turn_target(game, *target, yaw, *pitch);
            }
            Turn::Facing(at) => face(game, *target, *at),
            Turn::Entity(other, eyes) => {
                if let Some(feet) = game.target_feet(*other) {
                    face(
                        game,
                        *target,
                        feet + DVec3::Y * if *eyes { 1.62 } else { 0.0 },
                    );
                }
            }
        }
        replies.push(Reply::Sender(format!(
            "Teleported {name} to {:.2}, {:.2}, {:.2}",
            to.x, to.y, to.z
        )));
    }
    Ok(replies)
}

/// Turns `target` to look at `at`.
fn face(game: &mut Game, target: Target, at: DVec3) {
    let from = game.target_feet(target).unwrap_or(at) + DVec3::Y * 1.62;
    let d = at - from;
    let yaw = wrap_degrees(-(d.x.atan2(d.z)).to_degrees());
    let pitch = -(d.y.atan2(d.x.hypot(d.z))).to_degrees();
    turn_target(game, target, yaw, pitch);
}

// ---- /summon with entity data ----

/// Applies `{CustomName:'"x"',Tags:["a"],Health:5f,Rotation:[90f,0f],Age:-24000}`
/// to the mob that was just summoned.
pub(super) fn apply_entity_data(game: &mut Game, id: u64, data: &str) -> Result<(), String> {
    let inner = data
        .strip_prefix('{')
        .and_then(|d| d.strip_suffix('}'))
        .ok_or("Expected '{' to start the entity data")?;
    for entry in components::split_top(inner) {
        let entry = entry.trim();
        if entry.is_empty() {
            continue;
        }
        let (key, value) = entry
            .split_once(':')
            .ok_or_else(|| format!("Expected ':' after '{entry}'"))?;
        let value = value.trim();
        let number = |v: &str| -> Result<f32, String> {
            v.trim_end_matches(['f', 'd', 'b', 's', 'L'])
                .parse()
                .map_err(|_| format!("Invalid number '{v}'"))
        };
        let Some(mob) = game.mobs.iter_mut().find(|m| m.id == id) else {
            return Err("The entity is gone".to_owned());
        };
        match key.trim() {
            "CustomName" => mob.data.custom_name = Some(components::plain_text(value)),
            "Tags" => mob.data.tags = components::strings_in(value),
            "Health" => mob.stats.health = number(value)?,
            "Age" => mob.data.age = number(value)? as i32,
            "Rotation" => {
                let angles: Vec<f32> = components::strings_in(value.trim_matches(['[', ']']))
                    .iter()
                    .map(|a| number(a))
                    .collect::<Result<_, _>>()?;
                if let [yaw, pitch] = angles[..] {
                    mob.yaw = wrap_degrees(f64::from(yaw) - 180.0).to_radians() as f32;
                    mob.head_yaw = mob.yaw;
                    mob.pitch = (-pitch).to_radians();
                }
            }
            other => return Err(format!("Unknown entity data '{other}'")),
        }
    }
    Ok(())
}

pub(super) fn summon(
    game: &mut Game,
    caller: &Caller,
    args: &[&str],
) -> Result<Vec<Reply>, String> {
    let Some(name) = args.first() else {
        return Err("Usage: /summon <entity> [x y z] [data]".to_owned());
    };
    let name = name.strip_prefix("minecraft:").unwrap_or(name);
    let origin = game.feet_of(caller.index);
    let (at, data) = match args.get(1..) {
        Some(words) if words.len() >= 3 && words[..3].iter().all(|w| is_coordinate(w)) => {
            (position(&words[..3], origin)?, words.get(3).copied())
        }
        Some([data]) if data.starts_with('{') => (origin, Some(*data)),
        _ => (origin, None),
    };
    game.enter_realm(game.player_at(caller.index).dimension);
    let yaw = game.player_at(caller.index).camera.yaw;
    let id = game
        .summon(name, at, yaw)
        .ok_or_else(|| format!("Unknown entity type: {name}"))?
        .id;
    if let Some(data) = data
        && let Err(why) = apply_entity_data(game, id, data)
    {
        game.mobs.retain(|m| m.id != id);
        return Err(why);
    }
    Ok(tell(format!(
        "Summoned new {}",
        crate::game::mob_acts::display_name(name)
    )))
}

// ---- /locate poi ----

/// Points of interest and the blocks that make them.
const POIS: &[(&str, &[&str])] = &[
    (
        "bed",
        &[
            "white_bed",
            "red_bed",
            "blue_bed",
            "black_bed",
            "green_bed",
            "yellow_bed",
        ],
    ),
    ("nether_portal", &["nether_portal"]),
    ("farmer", &["composter"]),
    ("librarian", &["lectern"]),
    ("armorer", &["blast_furnace"]),
    ("toolsmith", &["smithing_table"]),
    ("weaponsmith", &["grindstone"]),
    ("cleric", &["brewing_stand"]),
    ("butcher", &["smoker"]),
    ("lightning_rod", &["lightning_rod"]),
    ("lodestone", &["lodestone"]),
];

/// What `/locate`'s second word can be, from its first.
pub(super) fn locate_names(game: &Game, before: &[&str]) -> Vec<String> {
    match before.first().copied() {
        Some("poi") => POIS.iter().map(|(n, _)| (*n).to_owned()).collect(),
        Some("biome") => world::biome_ids(game),
        Some("structure") => STRUCTURES.iter().map(|n| (*n).to_owned()).collect(),
        _ => Vec::new(),
    }
}

const STRUCTURES: &[&str] = &[
    "village",
    "desert_pyramid",
    "jungle_pyramid",
    "shipwreck",
    "ruined_portal",
    "mineshaft",
    "stronghold",
    "fortress",
    "bastion_remnant",
    "end_city",
];

/// The nearest loaded block of the poi's kind within 128 blocks.
pub(super) fn locate_poi(game: &Game, from: DVec3, name: &str) -> Result<IVec3, String> {
    let name = name.strip_prefix("minecraft:").unwrap_or(name);
    let &(_, blocks) = POIS
        .iter()
        .find(|(n, _)| *n == name)
        .ok_or_else(|| format!("Unknown point of interest: {name}"))?;
    let wanted: Vec<_> = blocks
        .iter()
        .filter_map(|b| crate::world::blocks::by_name(b))
        .collect();
    let world = game.chunks.world();
    let mut best: Option<(f64, IVec3)> = None;
    for pos in world.positions() {
        let Some(chunk) = world.get(pos) else {
            continue;
        };
        let (bx, bz) = pos.min_block();
        if (f64::from(bx + 8) - from.x).hypot(f64::from(bz + 8) - from.z) > 128.0 + 12.0 {
            continue;
        }
        for section in chunk.bottom_section()..=chunk.top_section() {
            if chunk.section_is_empty(section) {
                continue;
            }
            let bottom = crate::world::chunk::section_bottom(section);
            for y in bottom..bottom + 16 {
                for z in 0..16usize {
                    for x in 0..16usize {
                        let state = chunk.get(x, y, z);
                        if wanted.contains(&state.id()) {
                            let at = IVec3::new(bx + x as i32, y, bz + z as i32);
                            let d = at.as_dvec3().distance(from);
                            if d <= 128.0 && best.is_none_or(|(bd, _)| d < bd) {
                                best = Some((d, at));
                            }
                        }
                    }
                }
            }
        }
    }
    best.map(|(_, at)| at)
        .ok_or_else(|| format!("Could not find a point of interest of type \"{name}\" nearby"))
}
