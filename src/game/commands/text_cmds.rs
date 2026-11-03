//! `/tellraw`, `/title` and `/bossbar` (M44): JSON text to chat and the
//! screen, and the bars at the top of it.

use super::{board_cmds::render_json, *};
use crate::game::board::{BAR_COLORS, BAR_STYLES, Bossbar, TitleState};

pub(super) fn tellraw(
    game: &mut Game,
    caller: &Caller,
    args: &[&str],
) -> Result<Vec<Reply>, String> {
    let who = args.first().ok_or("Usage: /tellraw <targets> <message>")?;
    if args.len() < 2 {
        return Err("Usage: /tellraw <targets> <message>".to_owned());
    }
    let players = game.players_named(caller, who)?;
    let rendered = render_json(game, caller, &rest(args, 1))?;
    caller.report(players.len() as i32);
    Ok(players
        .into_iter()
        .map(|to| Reply::Rich {
            to,
            text: rendered.text.clone(),
            click: rendered.click.clone(),
            hover: rendered.hover.clone(),
        })
        .collect())
}

pub(super) fn title(game: &mut Game, caller: &Caller, args: &[&str]) -> Result<Vec<Reply>, String> {
    let usage = "Usage: /title <targets> (title|subtitle|actionbar|times|clear|reset) ...";
    let (Some(who), Some(what)) = (args.first(), args.get(1).copied()) else {
        return Err(usage.to_owned());
    };
    let players = game.players_named(caller, who)?;
    let shown = if players.len() == 1 {
        game.player_at(players[0]).name.clone()
    } else {
        format!("{} players", players.len())
    };
    let text = match what {
        "title" | "subtitle" | "actionbar" => {
            let text = render_json(game, caller, &rest(args, 2))?.text;
            for &i in &players {
                let text = text.clone();
                game.with_player(i, |g| {
                    let t = &mut g.player.title;
                    match what {
                        "title" => {
                            t.title = text;
                            t.show();
                        }
                        "subtitle" => t.subtitle = text,
                        _ => {
                            t.actionbar = text;
                            t.actionbar_left = TitleState::ACTIONBAR_TICKS;
                        }
                    }
                });
            }
            match what {
                "title" => format!("Showing new title for {shown}"),
                "subtitle" => format!("Showing new subtitle for {shown}"),
                _ => format!("Showing new actionbar title for {shown}"),
            }
        }
        "times" => {
            let times: Vec<u32> = (2..5)
                .map(|i| {
                    args.get(i)
                        .ok_or_else(|| {
                            "Usage: /title <targets> times <fadeIn> <stay> <fadeOut>".to_owned()
                        })
                        .and_then(|w| super::extra::parse_time(w))
                })
                .collect::<Result<_, _>>()?;
            for &i in &players {
                game.with_player(i, |g| {
                    let t = &mut g.player.title;
                    (t.fade_in, t.stay, t.fade_out) = (times[0], times[1], times[2]);
                });
            }
            format!("Changed title display times for {shown}")
        }
        "clear" => {
            for &i in &players {
                game.with_player(i, |g| g.player.title.clear());
            }
            format!("Cleared titles for {shown}")
        }
        "reset" => {
            for &i in &players {
                game.with_player(i, |g| g.player.title.reset());
            }
            format!("Reset title options for {shown}")
        }
        _ => return Err(usage.to_owned()),
    };
    caller.report(players.len() as i32);
    Ok(tell(text))
}

fn bar<'a>(game: &'a mut Game, id: &str) -> Result<&'a mut Bossbar, String> {
    let id = id.strip_prefix("minecraft:").unwrap_or(id);
    game.bossbars
        .get_mut(id)
        .ok_or_else(|| format!("No bossbar exists with the ID '{id}'"))
}

pub(super) fn bossbar_ids(game: &Game) -> Vec<String> {
    game.bossbars
        .keys()
        .map(|k| format!("minecraft:{k}"))
        .collect()
}

/// Sets a bar's value or maximum from `/execute store`, clamped.
pub(super) fn set_bar_number(game: &mut Game, id: &str, what: &str, n: i32) -> Result<(), String> {
    let b = bar(game, id)?;
    if what == "max" {
        b.max = n.max(1);
        b.value = b.value.min(b.max);
    } else {
        b.value = n.clamp(0, b.max);
    }
    Ok(())
}

pub(super) fn bossbar(
    game: &mut Game,
    caller: &Caller,
    args: &[&str],
) -> Result<Vec<Reply>, String> {
    let usage = "Usage: /bossbar (add|remove|list|get|set) ...";
    match args.first().copied().ok_or(usage)? {
        "add" => {
            let (Some(id), true) = (args.get(1), args.len() > 2) else {
                return Err("Usage: /bossbar add <id> <name>".to_owned());
            };
            let key = id.strip_prefix("minecraft:").unwrap_or(id);
            if game.bossbars.contains_key(key) {
                return Err(format!("A bossbar already exists with the ID '{key}'"));
            }
            let name = render_json(game, caller, &rest(args, 2))?.text;
            game.bossbars.insert(key.to_owned(), Bossbar::new(&name));
            Ok(tell(format!("Created custom bossbar [{name}]")))
        }
        "remove" => {
            let id = args.get(1).ok_or("Usage: /bossbar remove <id>")?;
            let name = bar(game, id)?.name.clone();
            game.bossbars
                .remove(id.strip_prefix("minecraft:").unwrap_or(id));
            Ok(tell(format!("Removed custom bossbar [{name}]")))
        }
        "list" => {
            let names: Vec<String> = game
                .bossbars
                .values()
                .map(|b| format!("[{}]", b.name))
                .collect();
            Ok(tell(if names.is_empty() {
                "There are no custom bossbars active".to_owned()
            } else {
                format!(
                    "There are {} custom bossbars active: {}",
                    names.len(),
                    names.join(", ")
                )
            }))
        }
        "get" => {
            let (Some(id), Some(what)) = (args.get(1), args.get(2).copied()) else {
                return Err("Usage: /bossbar get <id> (value|max|visible|players)".to_owned());
            };
            let b = bar(game, id)?.clone();
            let (text, n) = match what {
                "value" => (
                    format!("Custom bossbar [{}] has a value of {}", b.name, b.value),
                    b.value,
                ),
                "max" => (
                    format!("Custom bossbar [{}] has a maximum of {}", b.name, b.max),
                    b.max,
                ),
                "visible" => (
                    format!(
                        "Custom bossbar [{}] is currently {}",
                        b.name,
                        if b.visible { "shown" } else { "hidden" }
                    ),
                    i32::from(b.visible),
                ),
                "players" => {
                    let names: Vec<_> = b.players.iter().cloned().collect();
                    (
                        if names.is_empty() {
                            format!(
                                "Custom bossbar [{}] has no players currently online",
                                b.name
                            )
                        } else {
                            format!(
                                "Custom bossbar [{}] has {} players currently online: {}",
                                b.name,
                                names.len(),
                                names.join(", ")
                            )
                        },
                        names.len() as i32,
                    )
                }
                other => return Err(format!("Unknown option: {other}")),
            };
            caller.report(n);
            Ok(tell(text))
        }
        "set" => {
            let (Some(id), Some(what)) = (args.get(1), args.get(2).copied()) else {
                return Err(
                    "Usage: /bossbar set <id> (name|color|style|value|max|visible|players) ..."
                        .to_owned(),
                );
            };
            bar(game, id)?;
            let value = args.get(3).copied();
            let msg = match what {
                "name" => {
                    let name = render_json(game, caller, &rest(args, 3))?.text;
                    bar(game, id)?.name = name.clone();
                    format!("Custom bossbar [{name}] has changed name")
                }
                "color" => {
                    let c = value.ok_or("Expected a color")?;
                    if !BAR_COLORS.contains(&c) {
                        return Err(format!("Unknown color '{c}'"));
                    }
                    let b = bar(game, id)?;
                    b.color = c.to_owned();
                    format!("Custom bossbar [{}] has changed color", b.name)
                }
                "style" => {
                    let s = value.ok_or("Expected a style")?;
                    if !BAR_STYLES.contains(&s) {
                        return Err(format!("Unknown style '{s}'"));
                    }
                    let b = bar(game, id)?;
                    b.style = s.to_owned();
                    format!("Custom bossbar [{}] has changed style", b.name)
                }
                "value" | "max" => {
                    let n: i32 = number(value.ok_or("Expected a number")?, "integer")?;
                    if what == "max" && n < 1 {
                        return Err("Integer must not be less than 1".to_owned());
                    }
                    if what == "value" && n < 0 {
                        return Err("Integer must not be less than 0".to_owned());
                    }
                    let before = bar(game, id)?.clone();
                    set_bar_number(game, id, what, n)?;
                    let b = bar(game, id)?;
                    if (what == "value" && before.value == b.value)
                        || (what == "max" && before.max == b.max)
                    {
                        return Err(format!(
                            "Nothing changed. That bossbar already has that {what}"
                        ));
                    }
                    caller.report(if what == "max" { b.max } else { b.value });
                    if what == "max" {
                        format!("Custom bossbar [{}] now has a maximum of {}", b.name, b.max)
                    } else {
                        format!("Custom bossbar [{}]'s value is now {}", b.name, b.value)
                    }
                }
                "visible" => {
                    let on = match value {
                        Some("true") => true,
                        Some("false") => false,
                        _ => return Err("Expected true or false".to_owned()),
                    };
                    let b = bar(game, id)?;
                    if b.visible == on {
                        return Err(format!(
                            "Nothing changed. That bossbar is already {}",
                            if on { "visible" } else { "hidden" }
                        ));
                    }
                    b.visible = on;
                    format!(
                        "Custom bossbar [{}] is now {}",
                        b.name,
                        if on { "visible" } else { "hidden" }
                    )
                }
                "players" => {
                    let names: Vec<String> = if args.len() > 3 {
                        let mut names = Vec::new();
                        for i in game.players_named(caller, args[3])? {
                            names.push(game.player_at(i).name.clone());
                        }
                        names
                    } else {
                        Vec::new()
                    };
                    let b = bar(game, id)?;
                    b.players = names.iter().cloned().collect();
                    if names.is_empty() {
                        format!("Custom bossbar [{}] no longer has any players", b.name)
                    } else {
                        format!(
                            "Custom bossbar [{}] now has {} players: {}",
                            b.name,
                            names.len(),
                            names.join(", ")
                        )
                    }
                }
                other => return Err(format!("Unknown option: {other}")),
            };
            Ok(tell(msg))
        }
        _ => Err(usage.to_owned()),
    }
}

pub(super) fn bossbar_tab(game: &Game, before: &[&str]) -> Vec<String> {
    let pick = |o: &[&str]| o.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>();
    match (
        before.first().copied(),
        before.get(2).copied(),
        before.len(),
    ) {
        (_, _, 0) => pick(&["add", "remove", "list", "get", "set"]),
        (Some("remove" | "get" | "set"), _, 1) => bossbar_ids(game),
        (Some("get"), _, 2) => pick(&["value", "max", "visible", "players"]),
        (Some("set"), _, 2) => pick(&[
            "name", "color", "style", "value", "max", "visible", "players",
        ]),
        (Some("set"), Some("color"), 3) => pick(&BAR_COLORS),
        (Some("set"), Some("style"), 3) => pick(&BAR_STYLES),
        (Some("set"), Some("visible"), 3) => pick(&["true", "false"]),
        (Some("set"), Some("players"), 3) => game.options(Arg::Players),
        _ => Vec::new(),
    }
}

pub(super) fn title_tab(game: &Game, before: &[&str]) -> Vec<String> {
    match before.len() {
        0 => game.options(Arg::Players),
        1 => ["title", "subtitle", "actionbar", "times", "clear", "reset"]
            .map(str::to_owned)
            .to_vec(),
        _ => Vec::new(),
    }
}
