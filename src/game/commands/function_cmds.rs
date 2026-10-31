//! `/function`, `/return`, `/schedule`, `/datapack`, `/reload` and `/tick`
//! (M45), and what the game does with them each tick.

use super::*;
use crate::game::{
    Scheduled,
    datapack::{self, Pack},
    nbt::Nbt,
};

/// `minecraft:` added when a function id has no namespace.
fn full(id: &str) -> String {
    if id.contains(':') {
        id.to_owned()
    } else {
        format!("minecraft:{id}")
    }
}

impl Game {
    /// The caller the server itself runs functions as: at the world's spawn,
    /// with every permission.
    pub(super) fn server_caller(&self) -> Caller {
        let mut caller = Caller::new(self, 0, true, true);
        caller.pos = self.spawn_point();
        caller.dim = crate::world::dimension::Dimension::Overworld;
        caller
    }

    /// Reads the enabled data packs again: functions, tags, predicates and
    /// loot tables. Returns what went wrong, one line each.
    pub fn reload_packs(&mut self) -> Vec<String> {
        let Some(dir) = self.world_dir_for_packs.clone() else {
            return Vec::new();
        };
        let found = datapack::discover(&dir);
        self.packs.reconcile(&found);
        let enabled: Vec<Pack> = self
            .packs
            .enabled
            .iter()
            .filter_map(|n| found.iter().find(|p| &p.name == n).cloned())
            .collect();
        let (loaded, errors) = datapack::load(&enabled);
        for e in &errors {
            log::warn!("data pack: {e}");
        }
        self.functions = loaded.functions;
        self.function_tags = loaded.tags;
        self.predicates = loaded.predicates;
        crate::world::structure::loot::set_pack_tables(loaded.loot.into_iter().collect());
        self.load_pending = true;
        errors
    }

    /// Every function id, tag entries included, for Tab.
    pub fn function_ids(&self) -> Vec<String> {
        let mut ids: Vec<String> = self.functions.keys().cloned().collect();
        ids.extend(self.function_tags.keys().map(|t| format!("#{t}")));
        ids
    }

    /// The functions a tag names, `#tags` inside it expanded, in order.
    pub fn tag_functions(&self, tag: &str) -> Vec<String> {
        fn walk(game: &Game, tag: &str, seen: &mut Vec<String>, out: &mut Vec<String>) {
            if seen.iter().any(|t| t == tag) {
                return;
            }
            seen.push(tag.to_owned());
            for entry in game.function_tags.get(tag).into_iter().flatten() {
                match entry.strip_prefix('#') {
                    Some(inner) => walk(game, &full(inner), seen, out),
                    None => out.push(full(entry)),
                }
            }
        }
        let (mut seen, mut out) = (Vec::new(), Vec::new());
        walk(self, &full(tag), &mut seen, &mut out);
        out
    }

    /// Runs function `id` (or every function of `#tag`) as `caller`, with
    /// macro arguments if given. Returns its result (`/return`'s value, else
    /// the commands that succeeded), the commands run, and what they said.
    pub(super) fn call_function(
        &mut self,
        caller: &Caller,
        id: &str,
        args: Option<&Nbt>,
    ) -> Result<(i32, usize, Vec<Reply>, bool), String> {
        if let Some(tag) = id.strip_prefix('#') {
            let mut total = (0, 0, Vec::new(), true);
            for f in self.tag_functions(tag) {
                let (r, n, replies, _) = self.call_function(caller, &f, args)?;
                total.0 += r;
                total.1 += n;
                total.2.extend(replies);
            }
            return Ok(total);
        }
        let key = full(id);
        let lines = self
            .functions
            .get(&key)
            .cloned()
            .ok_or_else(|| format!("Unknown function: {key}"))?;
        let needs_args = lines.iter().any(|l| l.starts_with('$'));
        if needs_args && args.is_none() {
            return Err(format!("Function {key} needs macro arguments"));
        }
        let outermost = self.fn_budget == 0;
        let start = self.fn_budget;
        let limit = self.rules.max_command_chain_length as usize;
        let mut replies = Vec::new();
        let (mut ok, mut ran) = (0, 0);
        let mut returned = None;
        for line in &lines {
            let line = match line.strip_prefix('$') {
                Some(template) => {
                    fill_macro(template, args.ok_or("Function needs macro arguments")?)?
                }
                None => line.clone(),
            };
            self.fn_budget += 1;
            if self.fn_budget > limit.max(1) {
                if outermost {
                    self.fn_budget = 0;
                }
                return Err(format!("The command chain is too long (limit {limit})"));
            }
            let (out, result) = self.run_as(caller, &line);
            // Functions run with their feedback suppressed: only what
            // commands say to players (`say`, `tellraw`...) is heard.
            replies.extend(out.into_iter().filter(|r| !matches!(r, Reply::Sender(_))));
            ran += 1;
            ok += i32::from(result.is_some());
            if let Some(v) = self.fn_return.take() {
                returned = Some(v);
                break;
            }
        }
        let total_ran = self.fn_budget.saturating_sub(start);
        if outermost {
            self.fn_budget = 0;
        }
        Ok((
            returned.unwrap_or(ok),
            total_ran.max(ran),
            replies,
            returned.is_some(),
        ))
    }

    /// Runs the `minecraft:load` tag once after a load or reload, the
    /// `minecraft:tick` tag every tick, and the scheduled functions that
    /// are due; what they say is for the server to deliver.
    pub(crate) fn run_server_functions(&mut self) {
        if self.functions.is_empty() && self.scheduled.is_empty() {
            self.load_pending = false;
            return;
        }
        let caller = self.server_caller();
        let mut tags = Vec::new();
        if std::mem::take(&mut self.load_pending) {
            tags.push("minecraft:load");
        }
        tags.push("minecraft:tick");
        for tag in tags {
            for f in self.tag_functions(tag) {
                match self.call_function(&caller, &f, None) {
                    Ok((_, _, replies, _)) => self.replies_out.extend(replies),
                    Err(why) => log::warn!("function {f}: {why}"),
                }
            }
        }
        let now = self.time;
        let (due, later): (Vec<Scheduled>, Vec<Scheduled>) = std::mem::take(&mut self.scheduled)
            .into_iter()
            .partition(|s| s.due <= now);
        self.scheduled = later;
        for s in due {
            match self.call_function(&caller, &s.id, None) {
                Ok((_, _, replies, _)) => self.replies_out.extend(replies),
                Err(why) => log::warn!("scheduled function {}: {why}", s.id),
            }
        }
    }
}

/// A macro line with `$(name)` replaced by the argument's text.
fn fill_macro(template: &str, args: &Nbt) -> Result<String, String> {
    let mut out = String::new();
    let mut rest = template;
    while let Some(at) = rest.find("$(") {
        out.push_str(&rest[..at]);
        let after = &rest[at + 2..];
        let end = after.find(')').ok_or("Unclosed macro in a function")?;
        let name = &after[..end];
        let value = args
            .get(name)
            .ok_or_else(|| format!("Missing macro argument '{name}'"))?;
        out.push_str(&match value {
            Nbt::Str(s) => s.clone(),
            Nbt::Byte(n) => n.to_string(),
            Nbt::Short(n) => n.to_string(),
            Nbt::Int(n) => n.to_string(),
            Nbt::Long(n) => n.to_string(),
            Nbt::Float(n) => n.to_string(),
            Nbt::Double(n) => n.to_string(),
            other => other.snbt(),
        });
        rest = &after[end + 1..];
    }
    out.push_str(rest);
    Ok(out)
}

pub(super) fn function(
    game: &mut Game,
    caller: &Caller,
    args: &[&str],
) -> Result<Vec<Reply>, String> {
    let id = args.first().ok_or(
        "Usage: /function <id> [macro arguments | with (entity|block|storage) <source> [path]]",
    )?;
    let macros = match args.get(1).copied() {
        None => None,
        Some("with") => {
            let (target, used) = data_cmds::parse_target(game, caller, &args[2..])?;
            let path = args.get(2 + used).copied();
            let found = data_cmds::values(game, &target, path)?;
            Some(
                found
                    .into_iter()
                    .next()
                    .ok_or("No data found for the macro arguments")?,
            )
        }
        Some(_) => {
            Some(Nbt::parse(&rest(args, 1)).map_err(|e| format!("Invalid macro arguments: {e}"))?)
        }
    };
    if let Some(m) = &macros
        && !matches!(m, Nbt::Compound(_))
    {
        return Err("Macro arguments must be a compound".to_owned());
    }
    let (result, ran, mut replies, returned) = game.call_function(caller, id, macros.as_ref())?;
    caller.report(result);
    replies.push(Reply::Sender(if returned {
        format!("Function {id} returned {result}")
    } else {
        format!("Executed {ran} command(s) from function '{id}'")
    }));
    Ok(replies)
}

pub(super) fn return_cmd(
    game: &mut Game,
    caller: &Caller,
    args: &[&str],
) -> Result<Vec<Reply>, String> {
    let usage = "Usage: /return <value> | /return fail | /return run <command>";
    let value = match args.first().copied().ok_or(usage)? {
        "fail" => 0,
        "run" => {
            let line = rest(args, 1);
            let (replies, result) = game.run_as(caller, &line);
            // The command's own words stay; its result is the return.
            game.fn_return = Some(result.unwrap_or(0));
            caller.report(result.unwrap_or(0));
            return Ok(replies);
        }
        n => number::<i32>(n, "integer")?,
    };
    game.fn_return = Some(value);
    caller.report(value);
    Ok(Vec::new())
}

pub(super) fn schedule(
    game: &mut Game,
    caller: &Caller,
    args: &[&str],
) -> Result<Vec<Reply>, String> {
    let usage = "Usage: /schedule (function <id> <time> [append|replace] | clear <id>)";
    match args.first().copied().ok_or(usage)? {
        "function" => {
            let (Some(id), Some(time)) = (args.get(1), args.get(2)) else {
                return Err(usage.to_owned());
            };
            let key = if id.starts_with('#') {
                (*id).to_owned()
            } else {
                full(id)
            };
            if !key.starts_with('#') && !game.functions.contains_key(&key) {
                return Err(format!("Unknown function: {key}"));
            }
            let ticks = u64::from(extra::parse_time(time)?);
            let due = game.time + ticks.max(1);
            if args.get(3).copied() != Some("append") {
                game.scheduled.retain(|s| s.id != key);
            }
            game.scheduled.push(Scheduled {
                id: key.clone(),
                due,
            });
            caller.report(due as i32);
            Ok(tell(format!(
                "Scheduled function {key} in {ticks} ticks at gametime {due}"
            )))
        }
        "clear" => {
            let id = args.get(1).ok_or(usage)?;
            let key = if id.starts_with('#') {
                (*id).to_owned()
            } else {
                full(id)
            };
            let before = game.scheduled.len();
            game.scheduled.retain(|s| s.id != key);
            let removed = before - game.scheduled.len();
            if removed == 0 {
                return Err(format!("Unknown schedule: {key}"));
            }
            caller.report(removed as i32);
            Ok(tell(format!(
                "Removed {removed} schedules for function {key}"
            )))
        }
        _ => Err(usage.to_owned()),
    }
}

pub(super) fn schedule_tab(game: &Game, before: &[&str]) -> Vec<String> {
    match (before.first().copied(), before.len()) {
        (_, 0) => vec!["function".to_owned(), "clear".to_owned()],
        (_, 1) => {
            let mut ids = game.function_ids();
            if before[0] == "clear" {
                ids = game.scheduled.iter().map(|s| s.id.clone()).collect();
            }
            ids
        }
        (Some("function"), 3) => vec!["append".to_owned(), "replace".to_owned()],
        _ => Vec::new(),
    }
}

pub(super) fn function_tab(game: &Game, before: &[&str]) -> Vec<String> {
    match before.len() {
        0 => game.function_ids(),
        1 => vec!["with".to_owned()],
        2 => vec![
            "entity".to_owned(),
            "block".to_owned(),
            "storage".to_owned(),
        ],
        3 if before[2] == "entity" => game.options(Arg::Entities),
        _ => Vec::new(),
    }
}

pub(super) fn return_tab(_: &Game, before: &[&str]) -> Vec<String> {
    match before.len() {
        0 => vec!["fail".to_owned(), "run".to_owned()],
        _ => Vec::new(),
    }
}

pub(super) fn reload(game: &mut Game, _: &Caller, _: &[&str]) -> Result<Vec<Reply>, String> {
    let errors = game.reload_packs();
    let mut out = vec![Reply::Sender("Reloading!".to_owned())];
    out.extend(errors.into_iter().map(|e| error(&e)));
    Ok(out)
}

// ---- /datapack ----

fn found_packs(game: &Game) -> Vec<Pack> {
    game.world_dir_for_packs
        .as_deref()
        .map(datapack::discover)
        .unwrap_or_default()
}

pub(super) fn datapack_names(game: &Game, _: &[&str]) -> Vec<String> {
    found_packs(game).into_iter().map(|p| p.name).collect()
}

pub(super) fn datapack_tab(game: &Game, before: &[&str]) -> Vec<String> {
    match (before.first().copied(), before.len()) {
        (_, 0) => ["list", "enable", "disable"].map(str::to_owned).to_vec(),
        (Some("list"), 1) => ["available", "enabled"].map(str::to_owned).to_vec(),
        (Some("enable" | "disable"), 1) => datapack_names(game, before),
        (Some("enable"), 2) => ["first", "last", "before", "after"]
            .map(str::to_owned)
            .to_vec(),
        (Some("enable"), 3) => datapack_names(game, before),
        _ => Vec::new(),
    }
}

pub(super) fn datapack(game: &mut Game, _: &Caller, args: &[&str]) -> Result<Vec<Reply>, String> {
    let usage = "Usage: /datapack (list [available|enabled] | enable <name> [first|last|before <pack>|after <pack>] | disable <name>)";
    let found = found_packs(game);
    game.packs.reconcile(&found);
    let describe = |p: &Pack| format!("[{}]", p.name);
    match args.first().copied().ok_or(usage)? {
        "list" => {
            let mode = args.get(1).copied();
            let enabled: Vec<String> = game
                .packs
                .enabled
                .iter()
                .map(|n| format!("[{n}]"))
                .collect();
            let available: Vec<String> = found
                .iter()
                .filter(|p| !game.packs.enabled.contains(&p.name))
                .map(describe)
                .collect();
            let mut out = Vec::new();
            let mut section = |what: &str, list: Vec<String>| {
                out.push(Reply::Sender(if list.is_empty() {
                    format!("There are no data packs {what}")
                } else {
                    format!(
                        "There are {} data pack(s) {what}: {}",
                        list.len(),
                        list.join(", ")
                    )
                }));
            };
            if mode != Some("available") {
                let mut all = vec!["[vanilla (built-in)]".to_owned()];
                all.extend(enabled);
                section("enabled", all);
            }
            if mode != Some("enabled") {
                section("available", available);
            }
            Ok(out)
        }
        "enable" => {
            let name = args.get(1).ok_or(usage)?;
            if !found.iter().any(|p| p.name == *name) {
                return Err(format!("Unknown data pack '{name}'"));
            }
            if game.packs.enabled.iter().any(|n| n == name) {
                return Err(format!("Pack '{name}' is already enabled!"));
            }
            game.packs.disabled.remove(*name);
            let at = match args.get(2).copied() {
                None | Some("last") => game.packs.enabled.len(),
                Some("first") => 0,
                Some(rel @ ("before" | "after")) => {
                    let other = args.get(3).ok_or(usage)?;
                    let i = game
                        .packs
                        .enabled
                        .iter()
                        .position(|n| n == other)
                        .ok_or_else(|| format!("Unknown or disabled data pack '{other}'"))?;
                    if rel == "before" { i } else { i + 1 }
                }
                Some(_) => return Err(usage.to_owned()),
            };
            game.packs.enabled.insert(at, (*name).to_owned());
            let errors = game.reload_packs();
            let mut out = vec![Reply::Sender(format!("Enabled data pack {name}"))];
            out.extend(errors.into_iter().map(|e| error(&e)));
            Ok(out)
        }
        "disable" => {
            let name = args.get(1).ok_or(usage)?;
            if !found.iter().any(|p| p.name == *name) {
                return Err(format!("Unknown data pack '{name}'"));
            }
            if !game.packs.enabled.iter().any(|n| n == name) {
                return Err(format!("Pack '{name}' is already disabled!"));
            }
            game.packs.enabled.retain(|n| n != name);
            game.packs.disabled.insert((*name).to_owned());
            game.reload_packs();
            Ok(tell(format!("Disabled data pack {name}")))
        }
        _ => Err(usage.to_owned()),
    }
}
