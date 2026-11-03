//! `/scoreboard`, `/team`, `/tag` and `/trigger` (M44).

use std::cell::RefCell;

use super::*;
use crate::game::{
    board::{
        self, COLORS, Collision, Criteria, NumberFormat, Objective, RenderType, Team, Visibility,
    },
    text::{self, TextContext},
};

/// The name a score is kept under for a player or mob.
pub(super) fn holder_of(game: &Game, target: Target) -> String {
    match target {
        Target::Player(i) => game.player_at(i).name.clone(),
        Target::Mob(id) => format!("entity.{id}"),
    }
}

/// Score holders `word` names: `*` (everyone with a score), a selector, or
/// any name at all (a "fake player").
pub(super) fn holders(game: &mut Game, caller: &Caller, word: &str) -> Result<Vec<String>, String> {
    if word == "*" {
        return Ok(game.board.holders().into_iter().collect());
    }
    if word.starts_with('@') {
        let found = game.targets(caller, word)?;
        return Ok(found.into_iter().map(|t| holder_of(game, t)).collect());
    }
    Ok(vec![word.to_owned()])
}

/// The single holder `word` names.
fn one_holder(game: &mut Game, caller: &Caller, word: &str) -> Result<String, String> {
    let all = holders(game, caller, word)?;
    match all.as_slice() {
        [only] => Ok(only.clone()),
        _ => Err(
            "Only one entity is allowed, but the provided selector allows more than one".to_owned(),
        ),
    }
}

/// Writes a JSON text component out as `§` text, resolving scores and selectors.
pub(super) fn render_json(
    game: &mut Game,
    caller: &Caller,
    json: &str,
) -> Result<text::Rendered, String> {
    let cell = RefCell::new(game);
    let ctx = TextContext {
        score: &|holder, objective| cell.borrow().board.score(objective, holder),
        selector: &|word| {
            let mut game = cell.borrow_mut();
            let found = game.targets(caller, word)?;
            Ok(found
                .into_iter()
                .map(|t| game.target_name(t))
                .collect::<Vec<_>>())
        },
    };
    text::render(json, &ctx)
}

fn objective_named<'a>(game: &'a Game, name: &str) -> Result<&'a Objective, String> {
    game.board
        .objectives
        .get(name)
        .ok_or_else(|| format!("Unknown scoreboard objective '{name}'"))
}

pub(super) fn objective_names(game: &Game) -> Vec<String> {
    game.board.objectives.keys().cloned().collect()
}

pub(super) fn team_names(game: &Game) -> Vec<String> {
    game.board.teams.keys().cloned().collect()
}

pub(super) fn criteria_names(_: &Game) -> Vec<String> {
    let mut names: Vec<String> = [
        "dummy",
        "trigger",
        "deathCount",
        "playerKillCount",
        "totalKillCount",
        "health",
        "food",
        "air",
        "armor",
        "level",
        "xp",
    ]
    .map(str::to_owned)
    .to_vec();
    for kind in [
        "mined",
        "used",
        "killed",
        "crafted",
        "broken",
        "picked_up",
        "dropped",
    ] {
        names.push(format!("minecraft.{kind}:minecraft."));
    }
    names
}

pub(super) fn slot_names(game: &Game) -> Vec<String> {
    let _ = game;
    let mut slots: Vec<String> = board::SLOTS.map(str::to_owned).to_vec();
    slots.extend(COLORS.iter().map(|(n, _)| format!("sidebar.team.{n}")));
    slots
}

pub(super) fn scoreboard(
    game: &mut Game,
    caller: &Caller,
    args: &[&str],
) -> Result<Vec<Reply>, String> {
    match (args.first().copied(), args.get(1).copied()) {
        (Some("objectives"), Some(action)) => objectives(game, caller, action, &args[2..]),
        (Some("players"), Some(action)) => players(game, caller, action, &args[2..]),
        _ => Err("Usage: /scoreboard <objectives|players> ...".to_owned()),
    }
}

fn objectives(
    game: &mut Game,
    caller: &Caller,
    action: &str,
    args: &[&str],
) -> Result<Vec<Reply>, String> {
    match action {
        "list" => {
            let names: Vec<String> = game
                .board
                .objectives
                .iter()
                .map(|(n, o)| {
                    format!(
                        "[{}]",
                        if o.display_name.is_empty() {
                            n
                        } else {
                            &o.display_name
                        }
                    )
                })
                .collect();
            Ok(tell(if names.is_empty() {
                "There are no objectives".to_owned()
            } else {
                format!("There are {} objectives: {}", names.len(), names.join(", "))
            }))
        }
        "add" => {
            let (Some(name), Some(criteria)) = (args.first(), args.get(1)) else {
                return Err(
                    "Usage: /scoreboard objectives add <name> <criteria> [displayName]".to_owned(),
                );
            };
            if game.board.objectives.contains_key(*name) {
                return Err("An objective already exists by that name".to_owned());
            }
            let criteria = Criteria::parse(criteria)?;
            let display = if args.len() > 2 {
                render_json(game, caller, &rest(args, 2))?.text
            } else {
                (*name).to_owned()
            };
            game.board.objectives.insert(
                (*name).to_owned(),
                Objective {
                    criteria,
                    display_name: display,
                    render_type: RenderType::Integer,
                    number_format: None,
                },
            );
            Ok(tell(format!("Created new objective [{name}]")))
        }
        "remove" => {
            let name = args
                .first()
                .ok_or("Usage: /scoreboard objectives remove <name>")?;
            objective_named(game, name)?;
            game.board.remove_objective(name);
            Ok(tell(format!("Removed objective [{name}]")))
        }
        "setdisplay" => {
            let slot = args
                .first()
                .ok_or("Usage: /scoreboard objectives setdisplay <slot> [objective]")?;
            if !board::is_slot(slot) {
                return Err(format!("Unknown display slot '{slot}'"));
            }
            match args.get(1) {
                Some(name) => {
                    objective_named(game, name)?;
                    if game.board.display.get(*slot).map(String::as_str) == Some(*name) {
                        return Err(
                            "Nothing changed. That display slot is already showing that objective"
                                .to_owned(),
                        );
                    }
                    game.board
                        .display
                        .insert((*slot).to_owned(), (*name).to_owned());
                    Ok(tell(format!(
                        "Set display slot {slot} to show objective [{name}]"
                    )))
                }
                None => {
                    if game.board.display.remove(*slot).is_none() {
                        return Err(
                            "Nothing changed. That display slot is already empty".to_owned()
                        );
                    }
                    Ok(tell(format!("Cleared objective display slot '{slot}'")))
                }
            }
        }
        "modify" => {
            let (Some(name), Some(what)) = (args.first(), args.get(1).copied()) else {
                return Err("Usage: /scoreboard objectives modify <name> <displayname|rendertype|numberformat> ...".to_owned());
            };
            objective_named(game, name)?;
            match what {
                "displayname" => {
                    let shown = render_json(game, caller, &rest(args, 2))?.text;
                    game.board.objectives.get_mut(*name).unwrap().display_name = shown;
                    Ok(tell(format!("Changed objective [{name}] display name")))
                }
                "rendertype" => {
                    let kind = match args.get(2).copied() {
                        Some("integer") => RenderType::Integer,
                        Some("hearts") => RenderType::Hearts,
                        _ => return Err("Usage: ... rendertype <integer|hearts>".to_owned()),
                    };
                    game.board.objectives.get_mut(*name).unwrap().render_type = kind;
                    Ok(tell(format!("Changed objective [{name}] render type")))
                }
                "numberformat" => {
                    let format = number_format(game, caller, &args[2..])?;
                    game.board.objectives.get_mut(*name).unwrap().number_format = format;
                    Ok(tell(format!("Changed number format of objective [{name}]")))
                }
                other => Err(format!("Unknown option: {other}")),
            }
        }
        other => Err(format!("Unknown option: {other}")),
    }
}

/// `blank`, `fixed <text>`, `styled <style>`, or nothing to clear.
fn number_format(
    game: &mut Game,
    caller: &Caller,
    args: &[&str],
) -> Result<Option<NumberFormat>, String> {
    Ok(match args.first().copied() {
        None => None,
        Some("blank") => Some(NumberFormat::Blank),
        Some("fixed") => Some(NumberFormat::Fixed(
            render_json(game, caller, &rest(args, 1))?.text,
        )),
        Some("styled") => {
            let style = rest(args, 1);
            let color = style
                .split("\"color\":\"")
                .nth(1)
                .and_then(|c| c.split('"').next())
                .and_then(board::color_code)
                .ok_or("Expected a style with a color, like {\"color\":\"red\"}")?;
            Some(NumberFormat::Styled(color))
        }
        Some(other) => return Err(format!("Unknown number format: {other}")),
    })
}

fn players(
    game: &mut Game,
    caller: &Caller,
    action: &str,
    args: &[&str],
) -> Result<Vec<Reply>, String> {
    let shown = |game: &Game, o: &str| {
        let d = game.board.objectives.get(o).map(|o| o.display_name.clone());
        format!(
            "[{}]",
            d.filter(|d| !d.is_empty()).unwrap_or_else(|| o.to_owned())
        )
    };
    let named = |game: &mut Game, word: Option<&&str>| -> Result<Vec<String>, String> {
        holders(game, caller, word.ok_or("Expected a target")?)
    };
    let who_text = |holders: &[String]| {
        if holders.len() == 1 {
            holders[0].clone()
        } else {
            format!("{} entities", holders.len())
        }
    };
    match action {
        "list" => match args.first() {
            None => {
                let all: Vec<String> = game.board.holders().into_iter().collect();
                Ok(tell(if all.is_empty() {
                    "There are no tracked entities".to_owned()
                } else {
                    format!(
                        "There are {} tracked entities: {}",
                        all.len(),
                        all.join(", ")
                    )
                }))
            }
            Some(word) => {
                let who = one_holder(game, caller, word)?;
                let scores = game.board.scores_of(&who);
                if scores.is_empty() {
                    return Ok(tell(format!("{who} has no scores to show")));
                }
                let mut out = vec![Reply::Sender(format!("{who} has {} scores:", scores.len()))];
                for (o, v) in scores {
                    out.push(Reply::Sender(format!("- {}: {v}", shown(game, &o))));
                }
                Ok(out)
            }
        },
        "get" => {
            let who = one_holder(game, caller, args.first().ok_or("Expected a target")?)?;
            let o = args.get(1).ok_or("Expected an objective")?;
            objective_named(game, o)?;
            let v = game.board.score(o, &who).ok_or_else(|| {
                format!(
                    "Can't get value of {} for {who}; none is set",
                    shown(game, o)
                )
            })?;
            caller.report(v);
            Ok(tell(format!("{who} has {v} {}", shown(game, o))))
        }
        "set" | "add" | "remove" => {
            let who = named(game, args.first())?;
            let o = args.get(1).ok_or("Expected an objective")?;
            objective_named(game, o)?;
            let n: i32 = number(args.get(2).ok_or("Expected a value")?, "integer")?;
            if action != "set" && n < 0 {
                return Err("Integer must not be less than 0".to_owned());
            }
            let mut last = n;
            for holder in &who {
                last = match action {
                    "set" => {
                        game.board.set(o, holder, n);
                        n
                    }
                    "add" => game.board.add(o, holder, n),
                    _ => game.board.add(o, holder, n.wrapping_neg()),
                };
            }
            caller.report(last);
            let d = shown(game, o);
            let who_t = who_text(&who);
            Ok(tell(match action {
                "set" => format!("Set {d} for {who_t} to {n}"),
                "add" => format!("Added {n} to {d} for {who_t} (now {last})"),
                _ => format!("Removed {n} from {d} for {who_t} (now {last})"),
            }))
        }
        "reset" => {
            let who = named(game, args.first())?;
            let only = args.get(1).copied();
            if let Some(o) = only {
                objective_named(game, o)?;
            }
            for holder in &who {
                game.board.reset(holder, only);
            }
            let who_t = who_text(&who);
            Ok(tell(match only {
                Some(o) => format!("Reset score {} for {who_t}", shown(game, o)),
                None => format!("Reset all scores for {who_t}"),
            }))
        }
        "enable" => {
            let who = named(game, args.first())?;
            let o = args.get(1).ok_or("Expected an objective")?;
            if objective_named(game, o)?.criteria != Criteria::Trigger {
                return Err("You can only trigger objectives that are 'trigger' type".to_owned());
            }
            let mut changed = 0;
            for holder in &who {
                if game
                    .board
                    .enabled
                    .entry((*o).to_owned())
                    .or_default()
                    .insert(holder.clone())
                {
                    changed += 1;
                }
            }
            if changed == 0 {
                return Err(format!(
                    "Nothing changed. That trigger is already enabled for {}",
                    who_text(&who)
                ));
            }
            Ok(tell(format!(
                "Enabled trigger {} for {}",
                shown(game, o),
                who_text(&who)
            )))
        }
        "operation" => {
            let (Some(t), Some(to), Some(op), Some(s), Some(so)) = (
                args.first(),
                args.get(1),
                args.get(2).copied(),
                args.get(3),
                args.get(4),
            ) else {
                return Err(
                    "Usage: /scoreboard players operation <targets> <objective> <operation> <source> <sourceObjective>"
                        .to_owned(),
                );
            };
            let targets = holders(game, caller, t)?;
            let sources = holders(game, caller, s)?;
            objective_named(game, to)?;
            objective_named(game, so)?;
            let mut last = 0;
            for target in &targets {
                for source in &sources {
                    let a = game.board.score(to, target).unwrap_or(0);
                    let b = game.board.score(so, source).ok_or_else(|| {
                        format!("Can't get value of [{so}] for {source}; none is set")
                    })?;
                    let (new, swapped) = board::Scoreboard::operate(op, a, b)?;
                    game.board.set(to, target, new);
                    if let Some(back) = swapped {
                        game.board.set(so, source, back);
                    }
                    last = new;
                }
            }
            caller.report(last);
            Ok(tell(format!(
                "Operation applied to {} for {} (now {last})",
                shown(game, to),
                who_text(&targets)
            )))
        }
        other => Err(format!("Unknown option: {other}")),
    }
}

pub(super) fn trigger(
    game: &mut Game,
    caller: &Caller,
    args: &[&str],
) -> Result<Vec<Reply>, String> {
    let o = args
        .first()
        .ok_or("Usage: /trigger <objective> [add|set <value>]")?;
    let Target::Player(index) = caller.executor else {
        return Err("Only players can trigger objectives".to_owned());
    };
    let name = game.player_at(index).name.clone();
    if objective_named(game, o)?.criteria != Criteria::Trigger {
        return Err("You can only trigger objectives that are 'trigger' type".to_owned());
    }
    let allowed = game
        .board
        .enabled
        .get(*o)
        .is_some_and(|set| set.contains(&name));
    if !allowed {
        return Err("You cannot trigger this objective yet".to_owned());
    }
    let (how, value) = match (args.get(1).copied(), args.get(2)) {
        (Some(h @ ("add" | "set")), Some(v)) => (h, number::<i32>(v, "integer")?),
        (None, _) => ("add", 1),
        _ => return Err("Usage: /trigger <objective> [add|set <value>]".to_owned()),
    };
    let now = if how == "set" {
        game.board.set(o, &name, value);
        value
    } else {
        game.board.add(o, &name, value)
    };
    if let Some(set) = game.board.enabled.get_mut(*o) {
        set.remove(&name);
    }
    caller.report(now);
    Ok(tell(match how {
        "set" => format!("Triggered [{o}] (set value to {value})"),
        _ if args.len() == 1 => format!("Triggered [{o}]"),
        _ => format!("Triggered [{o}] (added {value} to value)"),
    }))
}

// ---- teams ----

fn team_named<'a>(game: &'a mut Game, name: &str) -> Result<&'a mut Team, String> {
    game.board
        .teams
        .get_mut(name)
        .ok_or_else(|| format!("Unknown team '{name}'"))
}

fn member_names(game: &mut Game, caller: &Caller, words: &[&str]) -> Result<Vec<String>, String> {
    let mut out = Vec::new();
    for w in words {
        out.extend(holders(game, caller, w)?);
    }
    Ok(out)
}

pub(super) fn team(game: &mut Game, caller: &Caller, args: &[&str]) -> Result<Vec<Reply>, String> {
    let usage = "Usage: /team <add|remove|empty|join|leave|list|modify> ...";
    match args.first().copied().ok_or(usage)? {
        "add" => {
            let name = args.get(1).ok_or("Usage: /team add <name> [displayName]")?;
            if game.board.teams.contains_key(*name) {
                return Err("A team with the name already exists".to_owned());
            }
            let mut team = Team::new(name);
            if args.len() > 2 {
                team.display_name = render_json(game, caller, &rest(args, 2))?.text;
            }
            game.board.teams.insert((*name).to_owned(), team);
            Ok(tell(format!("Created team {name}")))
        }
        "remove" => {
            let name = args.get(1).ok_or("Usage: /team remove <team>")?;
            team_named(game, name)?;
            game.board.teams.remove(*name);
            game.board.display.remove(&format!("sidebar.team.{name}"));
            Ok(tell(format!("Removed team {name}")))
        }
        "empty" => {
            let name = args.get(1).ok_or("Usage: /team empty <team>")?;
            let team = team_named(game, name)?;
            if team.members.is_empty() {
                return Err("Unable to empty team; it is already empty".to_owned());
            }
            let n = team.members.len();
            team.members.clear();
            Ok(tell(format!("Removed {n} member(s) from team {name}")))
        }
        "join" => {
            let name = args.get(1).ok_or("Usage: /team join <team> [members]")?;
            team_named(game, name)?;
            let members = if args.len() > 2 {
                member_names(game, caller, &args[2..])?
            } else {
                vec![holder_of(game, caller.executor)]
            };
            for m in &members {
                for team in game.board.teams.values_mut() {
                    team.members.remove(m);
                }
                game.board
                    .teams
                    .get_mut(*name)
                    .unwrap()
                    .members
                    .insert(m.clone());
            }
            Ok(tell(if members.len() == 1 {
                format!("Added {} to team {name}", members[0])
            } else {
                format!("Added {} member(s) to team {name}", members.len())
            }))
        }
        "leave" => {
            let members = member_names(game, caller, &args[1..])?;
            let mut left = 0;
            for m in &members {
                for team in game.board.teams.values_mut() {
                    if team.members.remove(m) {
                        left += 1;
                    }
                }
            }
            if left == 0 {
                return Err("Unable to leave team; they are not in a team".to_owned());
            }
            Ok(tell(if members.len() == 1 {
                format!("Removed {} from any team", members[0])
            } else {
                format!("Removed {left} member(s) from any team")
            }))
        }
        "list" => match args.get(1) {
            None => {
                let names: Vec<String> = game
                    .board
                    .teams
                    .iter()
                    .map(|(n, t)| {
                        format!(
                            "[{}]",
                            if t.display_name.is_empty() {
                                n
                            } else {
                                &t.display_name
                            }
                        )
                    })
                    .collect();
                Ok(tell(if names.is_empty() {
                    "There are no teams".to_owned()
                } else {
                    format!("There are {} teams: {}", names.len(), names.join(", "))
                }))
            }
            Some(name) => {
                let team = team_named(game, name)?;
                let members: Vec<String> = team.members.iter().cloned().collect();
                Ok(tell(if members.is_empty() {
                    format!("Team {name} has no members")
                } else {
                    format!(
                        "Team {name} has {} member(s): {}",
                        members.len(),
                        members.join(", ")
                    )
                }))
            }
        },
        "modify" => modify_team(game, caller, &args[1..]),
        _ => Err(usage.to_owned()),
    }
}

fn flag(word: Option<&&str>) -> Result<bool, String> {
    match word.copied() {
        Some("true") => Ok(true),
        Some("false") => Ok(false),
        Some(other) => Err(format!(
            "Invalid boolean, expected 'true' or 'false' but found '{other}'"
        )),
        None => Err("Expected a value".to_owned()),
    }
}

fn modify_team(game: &mut Game, caller: &Caller, args: &[&str]) -> Result<Vec<Reply>, String> {
    let (Some(name), Some(what)) = (args.first(), args.get(1).copied()) else {
        return Err("Usage: /team modify <team> <option> <value>".to_owned());
    };
    team_named(game, name)?;
    let value = args.get(2);
    let msg = match what {
        "displayName" => {
            let shown = render_json(game, caller, &rest(args, 2))?.text;
            team_named(game, name)?.display_name = shown.clone();
            format!("Changed the display name of team {name} to {shown}")
        }
        "color" => {
            let word = value.ok_or("Expected a color")?;
            let color = if *word == "reset" {
                None
            } else if board::color_code(word).is_some() {
                Some((*word).to_owned())
            } else {
                return Err(format!("Unknown color '{word}'"));
            };
            let team = team_named(game, name)?;
            if team.color == color {
                return Err("Nothing changed. That team already has that color".to_owned());
            }
            team.color = color;
            format!("Set the color of team {name} to {word}")
        }
        "prefix" => {
            let shown = render_json(game, caller, &rest(args, 2))?.text;
            team_named(game, name)?.prefix = shown.clone();
            format!("Changed the team prefix of team {name} to {shown}")
        }
        "suffix" => {
            let shown = render_json(game, caller, &rest(args, 2))?.text;
            team_named(game, name)?.suffix = shown.clone();
            format!("Changed the team suffix of team {name} to {shown}")
        }
        "friendlyFire" => {
            let on = flag(value)?;
            let team = team_named(game, name)?;
            if team.friendly_fire == on {
                return Err(format!(
                    "Nothing changed. Friendly fire is already {} for that team",
                    if on { "enabled" } else { "disabled" }
                ));
            }
            team.friendly_fire = on;
            format!(
                "Friendly fire is now {} for team {name}",
                if on { "enabled" } else { "disabled" }
            )
        }
        "seeFriendlyInvisibles" => {
            let on = flag(value)?;
            let team = team_named(game, name)?;
            if team.see_friendly_invisibles == on {
                return Err("Nothing changed. That setting is already that value".to_owned());
            }
            team.see_friendly_invisibles = on;
            if on {
                format!("Team {name} can now see invisible team members")
            } else {
                format!("Team {name} can no longer see invisible team members")
            }
        }
        "nametagVisibility" => {
            let v = Visibility::parse(value.ok_or("Expected a visibility")?)
                .ok_or("Unknown visibility")?;
            team_named(game, name)?.nametag_visibility = v;
            format!("Nametag visibility for team {name} is now \"{}\"", v.name())
        }
        "deathMessageVisibility" => {
            let v = Visibility::parse(value.ok_or("Expected a visibility")?)
                .ok_or("Unknown visibility")?;
            team_named(game, name)?.death_message_visibility = v;
            format!(
                "Death message visibility for team {name} is now \"{}\"",
                v.name()
            )
        }
        "collisionRule" => {
            let v = Collision::parse(value.ok_or("Expected a rule")?).ok_or("Unknown rule")?;
            team_named(game, name)?.collision_rule = v;
            format!("Collision rule for team {name} is now \"{}\"", v.name())
        }
        other => return Err(format!("Unknown option: {other}")),
    };
    Ok(tell(msg))
}

// ---- tags ----

fn tags_of(game: &Game, target: Target) -> Vec<String> {
    match target {
        Target::Player(i) => game.player_at(i).tags.clone(),
        Target::Mob(id) => game
            .mobs
            .iter()
            .find(|m| m.id == id)
            .map(|m| m.data.tags.clone())
            .unwrap_or_default(),
    }
}

fn with_tags(game: &mut Game, target: Target, f: impl FnOnce(&mut Vec<String>) -> bool) -> bool {
    match target {
        Target::Player(i) => game.with_player(i, |g| f(&mut g.player.tags)),
        Target::Mob(id) => game
            .mobs
            .iter_mut()
            .find(|m| m.id == id)
            .is_some_and(|m| f(&mut m.data.tags)),
    }
}

pub(super) fn tag(game: &mut Game, caller: &Caller, args: &[&str]) -> Result<Vec<Reply>, String> {
    let usage = "Usage: /tag <targets> (add|remove|list) [tag]";
    let (Some(who), Some(action)) = (args.first(), args.get(1).copied()) else {
        return Err(usage.to_owned());
    };
    let targets = game.targets(caller, who)?;
    let label = |game: &Game, targets: &[Target]| {
        if targets.len() == 1 {
            game.target_name(targets[0])
        } else {
            format!("{} entities", targets.len())
        }
    };
    match action {
        "list" => {
            let mut all: Vec<String> = Vec::new();
            for t in &targets {
                all.extend(tags_of(game, *t));
            }
            all.sort();
            all.dedup();
            let who = label(game, &targets);
            Ok(tell(if all.is_empty() {
                format!("{who} has no tags")
            } else {
                format!("{who} has {} tags: {}", all.len(), all.join(", "))
            }))
        }
        "add" | "remove" => {
            let name = args.get(2).ok_or(usage)?;
            if name.is_empty() || name.contains(char::is_whitespace) {
                return Err("Invalid tag".to_owned());
            }
            let mut changed = 0;
            for t in &targets {
                let did = with_tags(game, *t, |tags| {
                    if action == "add" {
                        if tags.iter().any(|x| x == name) || tags.len() >= 1024 {
                            false
                        } else {
                            tags.push((*name).to_owned());
                            true
                        }
                    } else {
                        let before = tags.len();
                        tags.retain(|x| x != name);
                        tags.len() != before
                    }
                });
                changed += usize::from(did);
            }
            let who = label(game, &targets);
            if changed == 0 {
                return Err(if action == "add" {
                    format!("Unable to add tag '{name}'; {who} already has it or has too many tags")
                } else {
                    format!("Unable to remove tag '{name}'; {who} doesn't have it")
                });
            }
            caller.report(changed as i32);
            Ok(tell(if action == "add" {
                format!("Added tag '{name}' to {who}")
            } else {
                format!("Removed tag '{name}' from {who}")
            }))
        }
        _ => Err(usage.to_owned()),
    }
}

/// `/teammsg <message>`: the sender's team hears it.
pub(super) fn teammsg(
    game: &mut Game,
    caller: &Caller,
    args: &[&str],
) -> Result<Vec<Reply>, String> {
    if args.is_empty() {
        return Err("Usage: /teammsg <message>".to_owned());
    }
    let sender = holder_of(game, caller.executor);
    let Some((team_name, team)) = game.board.team_of(&sender) else {
        return Err("You can't send a team message because you are not on a team".to_owned());
    };
    let (team_name, display, members) = (
        team_name.to_owned(),
        team.display_name.clone(),
        team.members.clone(),
    );
    let _ = team_name;
    let line = rest(args, 0);
    let dressed = game.board.dressed(&sender);
    let mut replies = Vec::new();
    for i in 0..game.player_count() {
        let name = game.player_at(i).name.clone();
        if !members.contains(&name) {
            continue;
        }
        let text = if name == sender {
            format!("-> {display} <{dressed}> {line}")
        } else {
            format!("{display} <{dressed}> {line}")
        };
        replies.push(Reply::Player(i, text));
    }
    Ok(replies)
}

// ---- Tab completion ----

fn pick(options: &[&str]) -> Vec<String> {
    options.iter().map(|s| (*s).to_owned()).collect()
}

/// `/scoreboard`'s Tab: what the word after `before` can be.
pub(super) fn scoreboard_tab(game: &Game, before: &[&str]) -> Vec<String> {
    let targets = || game.options(Arg::Entities);
    let objectives = || objective_names(game);
    match (
        before.first().copied(),
        before.get(1).copied(),
        before.len(),
    ) {
        (_, _, 0) => pick(&["objectives", "players"]),
        (Some("objectives"), _, 1) => pick(&["list", "add", "remove", "setdisplay", "modify"]),
        (Some("objectives"), Some("add"), 3) => criteria_names(game),
        (Some("objectives"), Some("remove"), 2) => objectives(),
        (Some("objectives"), Some("setdisplay"), 2) => slot_names(game),
        (Some("objectives"), Some("setdisplay"), 3) => objectives(),
        (Some("objectives"), Some("modify"), 2) => objectives(),
        (Some("objectives"), Some("modify"), 3) => {
            pick(&["displayname", "rendertype", "numberformat"])
        }
        (Some("objectives"), Some("modify"), 4) => match before.get(3).copied() {
            Some("rendertype") => pick(&["integer", "hearts"]),
            Some("numberformat") => pick(&["blank", "fixed", "styled"]),
            _ => Vec::new(),
        },
        (Some("players"), _, 1) => pick(&[
            "list",
            "get",
            "set",
            "add",
            "remove",
            "reset",
            "enable",
            "operation",
        ]),
        (Some("players"), _, 2) => {
            let mut t = targets();
            t.push("*".to_owned());
            t
        }
        (Some("players"), Some("operation"), 3) => objectives(),
        (Some("players"), Some("operation"), 4) => {
            pick(&["+=", "-=", "*=", "/=", "%=", "=", "<", ">", "><"])
        }
        (Some("players"), Some("operation"), 5) => {
            let mut t = targets();
            t.push("*".to_owned());
            t
        }
        (Some("players"), Some("operation"), 6) => objectives(),
        (Some("players"), Some("list"), _) => Vec::new(),
        (Some("players"), _, 3) => objectives(),
        _ => Vec::new(),
    }
}

/// `/team`'s Tab.
pub(super) fn team_tab(game: &Game, before: &[&str]) -> Vec<String> {
    match (
        before.first().copied(),
        before.get(1).copied(),
        before.len(),
    ) {
        (_, _, 0) => pick(&["add", "remove", "empty", "join", "leave", "list", "modify"]),
        (Some("remove" | "empty" | "join" | "list" | "modify"), _, 1) => team_names(game),
        (Some("join" | "leave"), _, n) if n >= 2 || before[0] == "leave" => {
            game.options(Arg::Entities)
        }
        (Some("modify"), _, 2) => pick(&[
            "displayName",
            "color",
            "friendlyFire",
            "seeFriendlyInvisibles",
            "nametagVisibility",
            "deathMessageVisibility",
            "collisionRule",
            "prefix",
            "suffix",
        ]),
        (Some("modify"), _, 3) => match before.get(2).copied() {
            Some("color") => {
                let mut c: Vec<String> = COLORS.iter().map(|(n, _)| (*n).to_owned()).collect();
                c.push("reset".to_owned());
                c
            }
            Some("friendlyFire" | "seeFriendlyInvisibles") => pick(&["true", "false"]),
            Some("nametagVisibility" | "deathMessageVisibility") => pick(&Visibility::NAMES),
            Some("collisionRule") => pick(&Collision::NAMES),
            _ => Vec::new(),
        },
        _ => Vec::new(),
    }
}

/// `/tag`'s Tab: targets, then the action, then (to remove) their tags.
pub(super) fn tag_tab(game: &Game, before: &[&str]) -> Vec<String> {
    match before.len() {
        0 => game.options(Arg::Entities),
        1 => pick(&["add", "remove", "list"]),
        2 if before[1] == "remove" => {
            let mut all: Vec<String> = game
                .mobs
                .iter()
                .flat_map(|m| m.data.tags.clone())
                .chain((0..game.player_count()).flat_map(|i| game.player_at(i).tags.clone()))
                .collect();
            all.sort();
            all.dedup();
            all
        }
        _ => Vec::new(),
    }
}

pub(super) fn trigger_tab(game: &Game, before: &[&str]) -> Vec<String> {
    match before.len() {
        0 => game
            .board
            .objectives
            .iter()
            .filter(|(_, o)| o.criteria == Criteria::Trigger)
            .map(|(n, _)| n.clone())
            .collect(),
        1 => pick(&["add", "set"]),
        _ => Vec::new(),
    }
}
