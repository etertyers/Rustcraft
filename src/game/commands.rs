//! Commands (M38), in vanilla's syntax: typed in chat after `/`, run on the
//! server for the player who typed them, answering in their chat (errors in
//! red). Without cheats only the harmless ones run (`/help`, `/list`,
//! `/msg`, `/me`, and `/seed` for the host). Targets take player names and
//! the selectors `@p`, `@a`, `@s`, `@r` and `@e[type=...]`; coordinates take
//! `~` for "where I am". Tab completes command names and arguments.

use glam::{DVec3, IVec3};

mod components;
mod extra;
mod world;

use super::{
    Game,
    dimensions::Arrival,
    selector::{Range, Target, wrap_degrees},
};
use crate::{
    inventory::{
        enchant::{self, Enchantment, Gear},
        items,
        stack::ItemStack,
    },
    level::Difficulty,
    player::{
        effects::{Active, Effect},
        mode::GameMode,
        stats::DamageSource,
    },
    world::{
        biome::Biome,
        block::BlockState,
        chunk::ChunkPos,
        dimension::Dimension,
        structure::{self, Kind},
    },
};

/// Where a command's answer goes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Reply {
    /// The player who typed it.
    Sender(String),
    /// Another player (by index).
    Player(usize, String),
    Everyone(String),
    /// Send this player away with this reason (`/kick`).
    Kick(usize, String),
    /// Open the world to LAN on this port (`/publish`; 0 for any).
    Publish(u16),
    /// Close the world (`/stop`).
    Stop,
}

/// The most blocks one `/fill` changes (vanilla's limit).
const FILL_LIMIT: i64 = 32768;
/// How far `/locate` looks, in chunks and blocks.
const LOCATE_CHUNKS: i32 = 100;
const LOCATE_BIOME_BLOCKS: i32 = 6400;

/// Who typed a command and what they may do.
struct Caller {
    index: usize,
    cheats: bool,
    host: bool,
}

/// What a command's argument is, for Tab completion.
#[derive(Clone, Copy)]
enum Arg {
    Players,
    Entities,
    Item,
    Block,
    Mob,
    Effect,
    Enchantment,
    Rule,
    Mode,
    Difficulty,
    Words(&'static [&'static str]),
    /// A list worked out from the game: sounds, advancements, biomes...
    Dynamic(fn(&Game) -> Vec<String>),
    /// A list that depends on the words typed before it.
    After(fn(&Game, &[&str]) -> Vec<String>),
    Coordinate,
    Free,
}

/// What a command does: the replies, or the error to show in red.
type Run = fn(&mut Game, &Caller, &[&str]) -> Result<Vec<Reply>, String>;

struct Command {
    name: &'static str,
    aliases: &'static [&'static str],
    /// Only with cheats on.
    cheat: bool,
    /// Only for the host.
    host: bool,
    usage: &'static str,
    args: &'static [Arg],
    run: Run,
}

/// A table entry written short: name, aliases, who may run it
/// (`cheat`, `host`), usage, Tab completion for each word, and the function.
macro_rules! cmd {
    ($name:expr, [$($alias:expr),*], $cheat:expr, $host:expr, $usage:expr, [$($arg:expr),*], $run:path) => {
        Command {
            name: $name,
            aliases: &[$($alias),*],
            cheat: $cheat,
            host: $host,
            usage: $usage,
            args: &[$($arg),*],
            run: $run,
        }
    };
}

const COMMANDS: &[Command] = &[
    Command {
        name: "help",
        aliases: &[],
        cheat: false,
        host: false,
        usage: "/help [command]",
        args: &[Arg::Free],
        run: help,
    },
    Command {
        name: "list",
        aliases: &[],
        cheat: false,
        host: false,
        usage: "/list",
        args: &[],
        run: list,
    },
    Command {
        name: "msg",
        aliases: &["tell", "w"],
        cheat: false,
        host: false,
        usage: "/msg <player> <message>",
        args: &[Arg::Players, Arg::Free],
        run: msg,
    },
    Command {
        name: "me",
        aliases: &[],
        cheat: false,
        host: false,
        usage: "/me <action>",
        args: &[Arg::Free],
        run: me,
    },
    Command {
        name: "seed",
        aliases: &[],
        cheat: false,
        host: true,
        usage: "/seed",
        args: &[],
        run: seed,
    },
    Command {
        name: "gamemode",
        aliases: &[],
        cheat: true,
        host: false,
        usage: "/gamemode <survival|creative|adventure|spectator> [player]",
        args: &[Arg::Mode, Arg::Players],
        run: gamemode,
    },
    Command {
        name: "give",
        aliases: &[],
        cheat: true,
        host: false,
        usage: "/give <player> <item>[components] [count]",
        args: &[Arg::Players, Arg::Item, Arg::Free],
        run: give,
    },
    Command {
        name: "clear",
        aliases: &[],
        cheat: true,
        host: false,
        usage: "/clear [player] [item] [maxCount]",
        args: &[Arg::Players, Arg::Item, Arg::Free],
        run: clear,
    },
    Command {
        name: "tp",
        aliases: &["teleport"],
        cheat: true,
        host: false,
        usage: "/tp [targets] <destination | x y z>",
        args: &[
            Arg::Entities,
            Arg::Entities,
            Arg::Coordinate,
            Arg::Coordinate,
        ],
        run: extra::tp,
    },
    Command {
        name: "time",
        aliases: &[],
        cheat: true,
        host: false,
        usage: "/time <set|add|query> <value>",
        args: &[
            Arg::Words(&["set", "add", "query"]),
            Arg::Words(&["day", "noon", "night", "midnight", "daytime", "gametime"]),
        ],
        run: time,
    },
    Command {
        name: "kill",
        aliases: &[],
        cheat: true,
        host: false,
        usage: "/kill [targets]",
        args: &[Arg::Entities],
        run: kill,
    },
    Command {
        name: "effect",
        aliases: &[],
        cheat: true,
        host: false,
        usage: "/effect <give|clear> <targets> [effect] [seconds] [amplifier]",
        args: &[
            Arg::Words(&["give", "clear"]),
            Arg::Entities,
            Arg::Effect,
            Arg::Free,
            Arg::Free,
        ],
        run: effect,
    },
    Command {
        name: "enchant",
        aliases: &[],
        cheat: true,
        host: false,
        usage: "/enchant <player> <enchantment> [level]",
        args: &[Arg::Players, Arg::Enchantment, Arg::Free],
        run: enchant_command,
    },
    Command {
        name: "xp",
        aliases: &["experience"],
        cheat: true,
        host: false,
        usage: "/xp <add|set|query> <player> [amount] [points|levels]",
        args: &[
            Arg::Words(&["add", "set", "query"]),
            Arg::Players,
            Arg::Free,
            Arg::Words(&["points", "levels"]),
        ],
        run: xp,
    },
    Command {
        name: "summon",
        aliases: &[],
        cheat: true,
        host: false,
        usage: "/summon <entity> [x y z]",
        args: &[Arg::Mob, Arg::Coordinate, Arg::Coordinate, Arg::Coordinate],
        run: extra::summon,
    },
    Command {
        name: "setblock",
        aliases: &[],
        cheat: true,
        host: false,
        usage: "/setblock <x y z> <block> [replace|keep|destroy]",
        args: &[
            Arg::Coordinate,
            Arg::Coordinate,
            Arg::Coordinate,
            Arg::Block,
            Arg::Words(&["replace", "keep", "destroy"]),
        ],
        run: world::setblock,
    },
    Command {
        name: "fill",
        aliases: &[],
        cheat: true,
        host: false,
        usage: "/fill <x1 y1 z1> <x2 y2 z2> <block> [replace|keep|hollow|outline|destroy]",
        args: &[
            Arg::Coordinate,
            Arg::Coordinate,
            Arg::Coordinate,
            Arg::Coordinate,
            Arg::Coordinate,
            Arg::Coordinate,
            Arg::Block,
            Arg::Words(&["replace", "keep", "hollow", "outline", "destroy"]),
        ],
        run: world::fill,
    },
    Command {
        name: "gamerule",
        aliases: &[],
        cheat: true,
        host: false,
        usage: "/gamerule <rule> [value]",
        args: &[Arg::Rule, Arg::Words(&["true", "false"])],
        run: gamerule,
    },
    Command {
        name: "spawnpoint",
        aliases: &[],
        cheat: true,
        host: false,
        usage: "/spawnpoint [player] [x y z]",
        args: &[
            Arg::Players,
            Arg::Coordinate,
            Arg::Coordinate,
            Arg::Coordinate,
        ],
        run: spawnpoint,
    },
    Command {
        name: "setworldspawn",
        aliases: &[],
        cheat: true,
        host: false,
        usage: "/setworldspawn [x y z]",
        args: &[Arg::Coordinate, Arg::Coordinate, Arg::Coordinate],
        run: setworldspawn,
    },
    Command {
        name: "locate",
        aliases: &[],
        cheat: true,
        host: false,
        usage: "/locate <structure|biome|poi> <name>",
        args: &[
            Arg::Words(&["structure", "biome", "poi"]),
            Arg::After(extra::locate_names),
        ],
        run: locate,
    },
    Command {
        name: "difficulty",
        aliases: &[],
        cheat: true,
        host: false,
        usage: "/difficulty [peaceful|easy|normal|hard]",
        args: &[Arg::Difficulty],
        run: difficulty,
    },
    Command {
        name: "kick",
        aliases: &[],
        cheat: true,
        host: true,
        usage: "/kick <player> [reason]",
        args: &[Arg::Players, Arg::Free],
        run: kick,
    },
    cmd!(
        "clone",
        [],
        true,
        false,
        "/clone <begin> <end> <destination> [replace|masked|filtered <block>] [force|move|normal]",
        [
            Arg::Coordinate,
            Arg::Coordinate,
            Arg::Coordinate,
            Arg::Coordinate,
            Arg::Coordinate,
            Arg::Coordinate,
            Arg::Coordinate,
            Arg::Coordinate,
            Arg::Coordinate,
            Arg::Words(&["replace", "masked", "filtered", "force", "move", "normal"]),
            Arg::Block,
            Arg::Words(&["force", "move", "normal"])
        ],
        world::clone
    ),
    cmd!(
        "fillbiome",
        [],
        true,
        false,
        "/fillbiome <from> <to> <biome> [replace <filter>]",
        [
            Arg::Coordinate,
            Arg::Coordinate,
            Arg::Coordinate,
            Arg::Coordinate,
            Arg::Coordinate,
            Arg::Coordinate,
            Arg::Dynamic(world::biome_ids),
            Arg::Words(&["replace"]),
            Arg::Dynamic(world::biome_ids)
        ],
        world::fillbiome
    ),
    cmd!(
        "place",
        [],
        true,
        false,
        "/place (template <name> | structure <name> | feature <name> | jigsaw <pool> <target> <depth>) [pos] [rotation]",
        [
            Arg::Words(&["template", "structure", "feature", "jigsaw"]),
            Arg::Dynamic(world::template_names),
            Arg::Coordinate,
            Arg::Coordinate,
            Arg::Coordinate,
            Arg::Words(&["none", "clockwise_90", "180", "counterclockwise_90"])
        ],
        world::place
    ),
    cmd!(
        "say",
        [],
        true,
        false,
        "/say <message>",
        [Arg::Free],
        extra::say
    ),
    cmd!(
        "weather",
        [],
        true,
        false,
        "/weather <clear|rain|thunder> [duration]",
        [Arg::Words(&["clear", "rain", "thunder"]), Arg::Free],
        extra::weather
    ),
    cmd!(
        "random",
        [],
        true,
        false,
        "/random <value|roll|reset> <range>",
        [Arg::Words(&["value", "roll", "reset"]), Arg::Free],
        extra::random
    ),
    cmd!(
        "save-all",
        [],
        true,
        true,
        "/save-all [flush]",
        [Arg::Words(&["flush"])],
        extra::save_all
    ),
    cmd!("save-on", [], true, true, "/save-on", [], extra::save_on),
    cmd!("save-off", [], true, true, "/save-off", [], extra::save_off),
    cmd!(
        "defaultgamemode",
        [],
        true,
        false,
        "/defaultgamemode <survival|creative|adventure|spectator>",
        [Arg::Mode],
        extra::defaultgamemode
    ),
    cmd!(
        "spectate",
        [],
        true,
        false,
        "/spectate [target] [spectator]",
        [Arg::Entities, Arg::Players],
        extra::spectate
    ),
    cmd!(
        "rotate",
        [],
        true,
        false,
        "/rotate <targets> (<yaw> <pitch> | facing <x y z> | facing entity <target>)",
        [
            Arg::Entities,
            Arg::Words(&["facing", "~"]),
            Arg::Words(&["entity", "~"]),
            Arg::Free
        ],
        extra::rotate
    ),
    cmd!(
        "spreadplayers",
        [],
        true,
        false,
        "/spreadplayers <x> <z> <spreadDistance> <maxRange> [under <maxHeight>] <respectTeams> <targets>",
        [
            Arg::Coordinate,
            Arg::Coordinate,
            Arg::Free,
            Arg::Free,
            Arg::Words(&["under", "true", "false"]),
            Arg::Words(&["true", "false"]),
            Arg::Entities
        ],
        extra::spreadplayers
    ),
    cmd!(
        "damage",
        [],
        true,
        false,
        "/damage <target> <amount> [type] [at <pos> | by <entity>]",
        [
            Arg::Entities,
            Arg::Free,
            Arg::Words(extra::DAMAGE_TYPES),
            Arg::Words(&["at", "by"]),
            Arg::Entities
        ],
        extra::damage
    ),
    cmd!(
        "attribute",
        [],
        true,
        false,
        "/attribute <target> <attribute> (get | base get | base set <value>)",
        [
            Arg::Entities,
            Arg::Dynamic(extra::attribute_names),
            Arg::Words(&["get", "base"]),
            Arg::Words(&["get", "set"])
        ],
        extra::attribute
    ),
    cmd!(
        "advancement",
        [],
        true,
        false,
        "/advancement <grant|revoke> <targets> (everything | only | from | through | until) [advancement]",
        [
            Arg::Words(&["grant", "revoke"]),
            Arg::Players,
            Arg::Words(&["everything", "only", "from", "through", "until"]),
            Arg::Dynamic(extra::advancement_ids)
        ],
        extra::advancement
    ),
    cmd!(
        "recipe",
        [],
        true,
        false,
        "/recipe <give|take> <targets> (* | recipe)",
        [
            Arg::Words(&["give", "take"]),
            Arg::Players,
            Arg::Dynamic(extra::recipe_names)
        ],
        extra::recipe
    ),
    cmd!(
        "particle",
        [],
        true,
        false,
        "/particle <name> [pos] [delta] [speed] [count]",
        [
            Arg::Dynamic(extra::particle_names),
            Arg::Coordinate,
            Arg::Coordinate,
            Arg::Coordinate
        ],
        extra::particle
    ),
    cmd!(
        "playsound",
        [],
        true,
        false,
        "/playsound <sound> <source> <targets> [pos] [volume] [pitch] [minVolume]",
        [
            Arg::Dynamic(Game::sound_names),
            Arg::Words(&[
                "master", "music", "record", "weather", "block", "hostile", "neutral", "player",
                "ambient", "voice"
            ]),
            Arg::Players,
            Arg::Coordinate,
            Arg::Coordinate,
            Arg::Coordinate
        ],
        extra::playsound
    ),
    cmd!(
        "stopsound",
        [],
        true,
        false,
        "/stopsound <targets> [source] [sound]",
        [
            Arg::Players,
            Arg::Words(&[
                "*", "master", "music", "record", "weather", "block", "hostile", "neutral",
                "player", "ambient", "voice"
            ]),
            Arg::Dynamic(Game::sound_names)
        ],
        extra::stopsound
    ),
    cmd!(
        "ride",
        [],
        true,
        false,
        "/ride <target> (mount <vehicle> | dismount)",
        [
            Arg::Entities,
            Arg::Words(&["mount", "dismount"]),
            Arg::Entities
        ],
        extra::ride
    ),
    cmd!(
        "op",
        [],
        true,
        true,
        "/op <player>",
        [Arg::Players],
        extra::op
    ),
    cmd!(
        "deop",
        [],
        true,
        true,
        "/deop <player>",
        [Arg::Players],
        extra::deop
    ),
    cmd!(
        "ban",
        [],
        true,
        true,
        "/ban <player> [reason]",
        [Arg::Players, Arg::Free],
        extra::ban
    ),
    cmd!(
        "pardon",
        [],
        true,
        true,
        "/pardon <player>",
        [Arg::Free],
        extra::pardon
    ),
    cmd!(
        "whitelist",
        [],
        true,
        true,
        "/whitelist <on|off|list|add|remove|reload> [player]",
        [
            Arg::Words(&["on", "off", "list", "add", "remove", "reload"]),
            Arg::Players
        ],
        extra::whitelist
    ),
    cmd!(
        "publish",
        [],
        true,
        true,
        "/publish [port]",
        [Arg::Free],
        extra::publish
    ),
    cmd!("stop", [], true, true, "/stop", [], extra::stop),
];

fn find(name: &str) -> Option<&'static Command> {
    COMMANDS
        .iter()
        .find(|c| c.name == name || c.aliases.contains(&name))
}

impl Game {
    /// Runs `line` (without its `/`) for player `index`, who may cheat or
    /// not and is the host or not. What to say comes back.
    pub fn run_command(
        &mut self,
        index: usize,
        line: &str,
        cheats: bool,
        host: bool,
    ) -> Vec<Reply> {
        let caller = Caller {
            index,
            cheats,
            host,
        };
        let words = tokens(line);
        let Some((&name, args)) = words.split_first() else {
            return vec![error("Unknown or incomplete command")];
        };
        let p = self.player_at(index);
        LOOK.set((p.camera.yaw, p.camera.pitch));
        let name = name.strip_prefix("minecraft:").unwrap_or(name);
        let Some(command) = find(name) else {
            return self.mod_command_replies(&caller, name, args);
        };
        if (command.cheat && !cheats) || (command.host && !host) {
            return vec![error("You do not have permission to use this command")];
        }
        match (command.run)(self, &caller, args) {
            Ok(replies) if !self.rules.send_command_feedback => replies
                .into_iter()
                .filter(|r| !matches!(r, Reply::Sender(t) if !t.starts_with("§c")))
                .collect(),
            Ok(replies) => replies,
            Err(why) => vec![error(&why)],
        }
    }

    /// A mod's command, or the error for an unknown one.
    fn mod_command_replies(&mut self, caller: &Caller, name: &str, args: &[&str]) -> Vec<Reply> {
        let spec = crate::mods::active()
            .commands
            .iter()
            .find(|c| c.name == name);
        let Some(spec) = spec else {
            return vec![error(&format!("Unknown command: {name}"))];
        };
        if (spec.cheat && !caller.cheats) || (spec.host && !caller.host) {
            return vec![error("You do not have permission to use this command")];
        }
        match self.run_mod_command(name, caller.index, args) {
            Some(Ok(lines)) => lines.into_iter().map(Reply::Sender).collect(),
            Some(Err(why)) => vec![error(&why)],
            None => vec![error(&format!("Unknown command: {name}"))],
        }
    }

    /// What Tab can put in place of `line`'s last word (without the `/`).
    pub fn complete_command(&self, line: &str, cheats: bool, host: bool) -> Vec<String> {
        let words = tokens(line);
        let last = if line.ends_with(' ') {
            ""
        } else {
            words.last().copied().unwrap_or("")
        };
        let typed = if line.ends_with(' ') {
            words.len()
        } else {
            words.len().saturating_sub(1)
        };
        let mut options: Vec<String> = if typed == 0 {
            COMMANDS
                .iter()
                .filter(|c| (!c.cheat || cheats) && (!c.host || host))
                .map(|c| c.name.to_owned())
                .collect()
        } else {
            let Some(command) = find(words[0]) else {
                return Vec::new();
            };
            match command.args.get(typed - 1) {
                Some(Arg::After(list)) => list(self, &words[1..typed]),
                Some(arg) => self.options(*arg),
                None => Vec::new(),
            }
        };
        if last.contains('[') {
            options = world::state_completions(last);
            options.extend(components::completions(last));
        }
        options.retain(|o| o.starts_with(last));
        options.sort();
        options.dedup();
        options
    }

    /// The sound events the game knows, for Tab.
    fn sound_names(&self) -> Vec<String> {
        extra::SOUNDS
            .iter()
            .map(|s| format!("minecraft:{s}"))
            .collect()
    }

    fn options(&self, arg: Arg) -> Vec<String> {
        let players = || {
            (0..self.player_count())
                .map(|i| self.player_at(i).name.clone())
                .chain(["@p", "@a", "@s", "@r"].map(str::to_owned))
                .collect::<Vec<_>>()
        };
        match arg {
            Arg::Players => players(),
            Arg::Entities => {
                let mut all = players();
                all.push("@e".to_owned());
                all
            }
            Arg::Item => crate::inventory::item::registry()
                .iter()
                .map(|def| def.name.clone())
                .collect(),
            Arg::Block => crate::world::block::registry()
                .iter()
                .map(|def| def.name.clone())
                .collect(),
            Arg::Mob => self.mob_defs.names().map(str::to_owned).collect(),
            Arg::Effect => Effect::ALL.iter().map(|e| e.id().to_owned()).collect(),
            Arg::Enchantment => enchant::ALL.iter().map(|e| enchantment_id(*e)).collect(),
            Arg::Rule => crate::level::GameRules::NAMES.map(str::to_owned).to_vec(),
            Arg::Mode => GameMode::ALL.map(|m| m.id().to_owned()).to_vec(),
            Arg::Difficulty => ["peaceful", "easy", "normal", "hard"]
                .map(str::to_owned)
                .to_vec(),
            Arg::Words(words) => words.iter().map(|w| (*w).to_owned()).collect(),
            Arg::Dynamic(list) => list(self),
            Arg::After(_) => Vec::new(),
            Arg::Coordinate => vec!["~".to_owned(), "^".to_owned()],
            Arg::Free => Vec::new(),
        }
    }

    /// The players `word` names: a name or a selector for players.
    fn players_named(&mut self, caller: &Caller, word: &str) -> Result<Vec<usize>, String> {
        let at = self.feet_of(caller.index);
        let chosen: Vec<usize> = self
            .select(caller.index, at, word)?
            .into_iter()
            .filter_map(|t| match t {
                Target::Player(i) => Some(i),
                Target::Mob(_) => None,
            })
            .collect();
        if chosen.is_empty() {
            return Err(if word.starts_with('@') {
                "No player was found".to_owned()
            } else {
                format!("No player was found named {word}")
            });
        }
        Ok(chosen)
    }

    /// The players and mobs `word` names, in the caller's dimension.
    fn targets(&mut self, caller: &Caller, word: &str) -> Result<Vec<Target>, String> {
        let at = self.feet_of(caller.index);
        let found = self.select(caller.index, at, word)?;
        if found.is_empty() {
            return Err(if word.starts_with('@') {
                "No entity was found".to_owned()
            } else {
                format!("No player was found named {word}")
            });
        }
        Ok(found)
    }

    fn target_name(&self, target: Target) -> String {
        match target {
            Target::Player(i) => self.player_at(i).name.clone(),
            Target::Mob(id) => self
                .mobs
                .iter()
                .find(|m| m.id == id)
                .map_or_else(String::new, |m| {
                    super::mob_acts::display_name(&self.mob_defs.get(m.kind).name)
                }),
        }
    }

    fn target_feet(&self, target: Target) -> Option<DVec3> {
        match target {
            Target::Player(i) => Some(self.feet_of(i)),
            Target::Mob(id) => self.mobs.iter().find(|m| m.id == id).map(|m| m.feet()),
        }
    }
}

/// The words of a command line; a selector's `[...]` stays whole.
fn tokens(line: &str) -> Vec<&str> {
    let mut words = Vec::new();
    let mut start = None;
    let mut depth = 0;
    for (i, c) in line.char_indices() {
        match c {
            '[' => depth += 1,
            ']' => depth -= 1,
            _ => {}
        }
        if c.is_whitespace() && depth == 0 {
            if let Some(s) = start.take() {
                words.push(&line[s..i]);
            }
        } else if start.is_none() {
            start = Some(i);
        }
    }
    if let Some(s) = start {
        words.push(&line[s..]);
    }
    words
}

/// The rest of the line from word `from` on (a message).
fn rest(args: &[&str], from: usize) -> String {
    args.get(from..).unwrap_or_default().join(" ")
}

fn error(text: &str) -> Reply {
    Reply::Sender(format!("§c{text}"))
}

fn tell(text: String) -> Vec<Reply> {
    vec![Reply::Sender(text)]
}

fn number<T: std::str::FromStr>(word: &str, what: &str) -> Result<T, String> {
    word.parse().map_err(|_| format!("Invalid {what} '{word}'"))
}

/// One coordinate: a number, or `~` (`~2`) from `origin`.
fn coordinate(word: &str, origin: f64) -> Result<f64, String> {
    match word.strip_prefix('~') {
        Some("") => Ok(origin),
        Some(by) => Ok(origin + number::<f64>(by, "coordinate")?),
        None => number(word, "coordinate"),
    }
}

thread_local! {
    /// Where the running command is looking (camera yaw and pitch, radians),
    /// for `^` coordinates; `run_command` sets it for the caller.
    static LOOK: std::cell::Cell<(f32, f32)> = const { std::cell::Cell::new((0.0, 0.0)) };
}

/// `x y z` from three words, relative to `origin`: numbers, `~` offsets,
/// or all three `^` local offsets (left, up, forward) along the look.
fn position(words: &[&str], origin: DVec3) -> Result<DVec3, String> {
    let [x, y, z] = words else {
        return Err("Expected three coordinates".to_owned());
    };
    let local = [x, y, z].map(|w| w.starts_with('^'));
    if local.iter().any(|&l| l) {
        if !local.iter().all(|&l| l) {
            return Err(
                "Cannot mix world & local coordinates (everything must either use ^ or not)"
                    .to_owned(),
            );
        }
        let offset = |w: &str| match w.strip_prefix('^') {
            Some("") => Ok(0.0),
            Some(by) => number::<f64>(by, "coordinate"),
            None => unreachable!(),
        };
        let (left, up, forward) = (offset(x)?, offset(y)?, offset(z)?);
        let (yaw, pitch) = LOOK.with(|l| l.get());
        let (sy, cy) = f64::from(yaw).sin_cos();
        let (sp, cp) = f64::from(pitch).sin_cos();
        let fwd = DVec3::new(sy * cp, sp, -cy * cp);
        let up_v = DVec3::new(-sy * sp, cp, cy * sp);
        let left_v = DVec3::new(-cy, 0.0, -sy);
        return Ok(origin + left_v * left + up_v * up + fwd * forward);
    }
    Ok(DVec3::new(
        coordinate(x, origin.x)?,
        coordinate(y, origin.y)?,
        coordinate(z, origin.z)?,
    ))
}

/// A block position from three words (a whole number is a block's
/// corner; `~` is the block the caller stands in).
fn block_position(words: &[&str], origin: DVec3) -> Result<IVec3, String> {
    Ok(position(words, origin)?.floor().as_ivec3())
}

/// A block by name, its default state.
fn block_named(name: &str) -> Result<BlockState, String> {
    let name = name.strip_prefix("minecraft:").unwrap_or(name);
    crate::world::blocks::by_name(name)
        .map(BlockState::of)
        .ok_or_else(|| format!("Unknown block type '{name}'"))
}

fn item_named(name: &str) -> Result<crate::inventory::item::ItemId, String> {
    let name = name.strip_prefix("minecraft:").unwrap_or(name);
    items::by_name(name).ok_or_else(|| format!("Unknown item '{name}'"))
}

/// An enchantment's vanilla id: `sharpness`, `vanishing_curse`.
fn enchantment_id(e: Enchantment) -> String {
    match e {
        Enchantment::VanishingCurse => "vanishing_curse".to_owned(),
        Enchantment::BindingCurse => "binding_curse".to_owned(),
        _ => e.name().to_lowercase().replace(' ', "_"),
    }
}

/// A biome's id: `plains`, `dark_forest`.
fn biome_id(biome: Biome) -> String {
    biome
        .name()
        .to_lowercase()
        .replace([' ', '\''], "_")
        .replace("__", "_")
}

fn mode_name(mode: GameMode) -> &'static str {
    match mode {
        GameMode::Survival => "Survival Mode",
        GameMode::Creative => "Creative Mode",
        GameMode::Adventure => "Adventure Mode",
        GameMode::Spectator => "Spectator Mode",
    }
}

fn help(_: &mut Game, caller: &Caller, args: &[&str]) -> Result<Vec<Reply>, String> {
    if let Some(name) = args.first() {
        let command = find(name).ok_or_else(|| format!("Unknown command: {name}"))?;
        return Ok(tell(command.usage.to_owned()));
    }
    Ok(COMMANDS
        .iter()
        .filter(|c| (!c.cheat || caller.cheats) && (!c.host || caller.host))
        .map(|c| Reply::Sender(c.usage.to_owned()))
        .collect())
}

fn list(game: &mut Game, _: &Caller, _: &[&str]) -> Result<Vec<Reply>, String> {
    let names: Vec<String> = (0..game.player_count())
        .map(|i| game.player_at(i).name.clone())
        .collect();
    Ok(tell(format!(
        "There are {} of a max of {} players online: {}",
        names.len(),
        crate::server::MAX_PLAYERS,
        names.join(", ")
    )))
}

fn msg(game: &mut Game, caller: &Caller, args: &[&str]) -> Result<Vec<Reply>, String> {
    let (Some(target), true) = (args.first(), args.len() > 1) else {
        return Err("Usage: /msg <player> <message>".to_owned());
    };
    let text = rest(args, 1);
    let from = game.player_at(caller.index).name.clone();
    let mut replies = Vec::new();
    for to in game.players_named(caller, target)? {
        let name = game.player_at(to).name.clone();
        replies.push(Reply::Player(
            to,
            format!("§7§o{from} whispers to you: {text}"),
        ));
        replies.push(Reply::Sender(format!("§7§oYou whisper to {name}: {text}")));
    }
    Ok(replies)
}

fn me(game: &mut Game, caller: &Caller, args: &[&str]) -> Result<Vec<Reply>, String> {
    if args.is_empty() {
        return Err("Usage: /me <action>".to_owned());
    }
    let name = &game.player_at(caller.index).name;
    Ok(vec![Reply::Everyone(format!("* {name} {}", rest(args, 0)))])
}

fn seed(game: &mut Game, _: &Caller, _: &[&str]) -> Result<Vec<Reply>, String> {
    let seed = game
        .chunks
        .seed()
        .map_or("unknown".to_owned(), |s| s.to_string());
    Ok(tell(format!("Seed: [§a{seed}§r]")))
}

fn gamemode(game: &mut Game, caller: &Caller, args: &[&str]) -> Result<Vec<Reply>, String> {
    let mode = match args.first().copied() {
        Some(word) => GameMode::parse(word)
            .filter(|_| word.len() > 1 && !word.chars().all(|c| c.is_ascii_digit()))
            .ok_or_else(|| format!("Unknown game mode: {word}"))?,
        None => {
            return Err(
                "Usage: /gamemode <survival|creative|adventure|spectator> [player]".to_owned(),
            );
        }
    };
    let who = match args.get(1) {
        Some(word) => game.players_named(caller, word)?,
        None => vec![caller.index],
    };
    let mut replies = Vec::new();
    for index in who {
        game.with_player(index, |g| g.set_game_mode(mode));
        let name = game.player_at(index).name.clone();
        if index == caller.index {
            replies.push(Reply::Sender(format!(
                "Set own game mode to {}",
                mode_name(mode)
            )));
        } else {
            replies.push(Reply::Sender(format!(
                "Set {name}'s game mode to {}",
                mode_name(mode)
            )));
            replies.push(Reply::Player(
                index,
                format!("Your game mode has been updated to {}", mode_name(mode)),
            ));
        }
    }
    Ok(replies)
}

fn give(game: &mut Game, caller: &Caller, args: &[&str]) -> Result<Vec<Reply>, String> {
    let (Some(who), Some(item)) = (args.first(), args.get(1)) else {
        return Err("Usage: /give <player> <item> [count]".to_owned());
    };
    let (id, template) = components::item_with_components(item)?;
    let count: u32 = match args.get(2) {
        Some(n) => number(n, "count")?,
        None => 1,
    };
    if count == 0 || count > 6400 {
        return Err("The count must be from 1 to 6400".to_owned());
    }
    let mut replies = Vec::new();
    for index in game.players_named(caller, who)? {
        let items = game.items().clone();
        game.with_player(index, |g| {
            let max = u32::from(items.max_stack(id));
            let mut left = count;
            while left > 0 {
                let take = left.min(max);
                left -= take;
                if let Some(rest) = g
                    .player
                    .inventory
                    .add(template.with_count(take as u8), &items)
                {
                    g.throw(rest);
                }
            }
        });
        let name = game.player_at(index).name.clone();
        replies.push(Reply::Sender(format!(
            "Gave {count} [{}] to {name}",
            items_display(id)
        )));
    }
    Ok(replies)
}

/// An item's name as shown.
fn items_display(id: crate::inventory::item::ItemId) -> String {
    let def = crate::inventory::item::registry().get(id);
    def.and_then(|d| d.display.clone())
        .unwrap_or_else(|| super::mob_acts::display_name(id.name()))
}

fn clear(game: &mut Game, caller: &Caller, args: &[&str]) -> Result<Vec<Reply>, String> {
    let who = match args.first() {
        Some(word) => game.players_named(caller, word)?,
        None => vec![caller.index],
    };
    let only = args
        .get(1)
        .map(|name| components::item_with_components(name))
        .transpose()?;
    let wanted = only.as_ref().map(|(id, _)| *id);
    let max: Option<u32> = args.get(2).map(|n| number(n, "count")).transpose()?;
    let mut replies = Vec::new();
    for index in who {
        if max == Some(0) {
            // A count of 0 only asks how many there are.
            let found: u32 = game.with_player(index, |g| {
                (0..crate::inventory::player::SLOTS)
                    .filter_map(|slot| g.player.inventory.slot(slot))
                    .filter(|stack| wanted.is_none_or(|id| id == stack.id))
                    .map(|stack| u32::from(stack.count))
                    .sum()
            });
            let name = game.player_at(index).name.clone();
            if found == 0 {
                return Err(format!("No items were found on player {name}"));
            }
            replies.push(Reply::Sender(format!(
                "Found {found} matching item(s) on player {name}"
            )));
            continue;
        }
        let removed = game.with_player(index, |g| {
            let mut removed = 0u32;
            for slot in 0..crate::inventory::player::SLOTS {
                let Some(mut stack) = g.player.inventory.slot(slot).cloned() else {
                    continue;
                };
                if wanted.is_some_and(|id| id != stack.id) {
                    continue;
                }
                let room = max.map_or(u32::MAX, |m| m - removed.min(m));
                let take = u32::from(stack.count).min(room);
                removed += take;
                stack.count -= take as u8;
                g.player
                    .inventory
                    .set(slot, (stack.count > 0).then_some(stack));
            }
            removed
        });
        let name = game.player_at(index).name.clone();
        if removed == 0 {
            return Err(format!("No items were found on player {name}"));
        }
        replies.push(Reply::Sender(format!(
            "Removed {removed} item(s) from player {name}"
        )));
    }
    Ok(replies)
}

fn time(game: &mut Game, _: &Caller, args: &[&str]) -> Result<Vec<Reply>, String> {
    let named = |word: &str| -> Result<u64, String> {
        Ok(match word {
            "day" => 1000,
            "noon" => 6000,
            "night" => 13000,
            "midnight" => 18000,
            n => u64::from(extra::parse_time(n)?),
        })
    };
    match (args.first().copied(), args.get(1).copied()) {
        (Some("set"), Some(value)) => {
            let day = named(value)?;
            // The day count stays; the time within it is set.
            let now = game.day_time();
            let target = now / 24000 * 24000 + day % 24000;
            game.day_offset += target as i64 - now as i64;
            Ok(tell(format!("Set the time to {day}")))
        }
        (Some("add"), Some(value)) => {
            let ticks = i64::from(extra::parse_time(value)?);
            game.day_offset += ticks;
            Ok(tell(format!("Set the time to {}", game.day_time() % 24000)))
        }
        (Some("query"), Some("daytime")) => {
            Ok(tell(format!("The time is {}", game.day_time() % 24000)))
        }
        (Some("query"), Some("gametime")) => Ok(tell(format!("The time is {}", game.time()))),
        (Some("query"), Some("day")) => {
            Ok(tell(format!("The time is {}", game.day_time() / 24000)))
        }
        _ => Err("Usage: /time <set|add|query> <value>".to_owned()),
    }
}

fn kill(game: &mut Game, caller: &Caller, args: &[&str]) -> Result<Vec<Reply>, String> {
    let who = match args.first() {
        Some(word) => game.targets(caller, word)?,
        None => vec![Target::Player(caller.index)],
    };
    let mut replies = Vec::new();
    for target in who {
        let name = game.target_name(target);
        match target {
            // /kill reaches creative players too, as in vanilla.
            Target::Player(index) => game.with_player(index, |g| {
                g.player.stats.health = 0.0;
                g.player.stats.last_source = Some(DamageSource::Kill);
                g.player.stats.last_attacker = None;
            }),
            Target::Mob(id) => {
                if let Some(mob) = game.mobs.iter_mut().find(|m| m.id == id) {
                    mob.hurt_by_world(f32::MAX, DamageSource::Kill);
                }
            }
        }
        replies.push(Reply::Sender(format!("Killed {name}")));
    }
    Ok(replies)
}

fn effect(game: &mut Game, caller: &Caller, args: &[&str]) -> Result<Vec<Reply>, String> {
    let (Some(action), Some(who)) = (args.first().copied(), args.get(1)) else {
        return Err(
            "Usage: /effect <give|clear> <targets> [effect] [seconds] [amplifier]".to_owned(),
        );
    };
    let named = |word: &str| {
        let word = word.strip_prefix("minecraft:").unwrap_or(word);
        Effect::ALL
            .iter()
            .copied()
            .find(|e| e.id() == word)
            .ok_or_else(|| format!("Unknown effect: {word}"))
    };
    let targets = game.targets(caller, who)?;
    let mut replies = Vec::new();
    match action {
        "give" => {
            let effect = named(
                args.get(2)
                    .ok_or("Usage: /effect give <targets> <effect> [seconds] [amplifier]")?,
            )?;
            let seconds: u32 = args
                .get(3)
                .map(|n| number(n, "seconds"))
                .transpose()?
                .unwrap_or(30);
            let amplifier: u8 = args
                .get(4)
                .map(|n| number(n, "amplifier"))
                .transpose()?
                .unwrap_or(0);
            let active = Active::new(effect, amplifier, seconds * 20);
            for target in targets {
                let name = game.target_name(target);
                match target {
                    Target::Player(index) => game.with_player(index, |g| {
                        let vulnerable = g.player.mode.is_vulnerable();
                        g.player.stats.apply(active, false, vulnerable);
                    }),
                    Target::Mob(id) => {
                        let undead = game.mobs.iter().find(|m| m.id == id).is_some_and(|m| {
                            super::gear::kind_of(&game.mob_defs.get(m.kind).name)
                                == super::gear::Kind::Undead
                        });
                        if let Some(mob) = game.mobs.iter_mut().find(|m| m.id == id) {
                            mob.stats.apply(active, undead, true);
                        }
                    }
                }
                replies.push(Reply::Sender(format!(
                    "Applied effect {} to {name}",
                    effect.name()
                )));
            }
        }
        "clear" => {
            let only = args.get(2).map(|w| named(w)).transpose()?;
            for target in targets {
                let name = game.target_name(target);
                let clear = |stats: &mut crate::player::stats::Stats| {
                    stats
                        .effects
                        .0
                        .retain(|a| only.is_some_and(|e| e != a.effect));
                };
                match target {
                    Target::Player(index) => {
                        game.with_player(index, |g| clear(&mut g.player.stats))
                    }
                    Target::Mob(id) => {
                        if let Some(mob) = game.mobs.iter_mut().find(|m| m.id == id) {
                            clear(&mut mob.stats);
                        }
                    }
                }
                replies.push(Reply::Sender(format!("Removed effects from {name}")));
            }
        }
        other => return Err(format!("Unknown action: {other}")),
    }
    Ok(replies)
}

fn enchant_command(game: &mut Game, caller: &Caller, args: &[&str]) -> Result<Vec<Reply>, String> {
    let (Some(who), Some(name)) = (args.first(), args.get(1)) else {
        return Err("Usage: /enchant <player> <enchantment> [level]".to_owned());
    };
    let name = name.strip_prefix("minecraft:").unwrap_or(name);
    let enchantment = enchant::ALL
        .iter()
        .copied()
        .find(|e| enchantment_id(*e) == name)
        .ok_or_else(|| format!("Unknown enchantment: {name}"))?;
    let level: u8 = args
        .get(2)
        .map(|n| number(n, "level"))
        .transpose()?
        .unwrap_or(1);
    if level == 0 || level > enchantment.max_level() {
        return Err(format!(
            "{level} is higher than the maximum level of {} supported by that enchantment",
            enchantment.max_level()
        ));
    }
    let items = game.items().clone();
    let mut replies = Vec::new();
    for index in game.players_named(caller, who)? {
        let name = game.player_at(index).name.clone();
        let done = game.with_player(index, |g| {
            let slot = g.player.inventory.selected;
            let Some(mut stack) = g.player.inventory.slot(slot).cloned() else {
                return Err(format!("{name} isn't holding an item"));
            };
            // The same rules as the table and the anvil (M31).
            if !enchantment.fits(Gear::of(&items, stack.id)) {
                return Err(format!(
                    "{} cannot support that enchantment",
                    items_display(stack.id)
                ));
            }
            if stack
                .enchantments()
                .iter()
                .any(|e| e.0 != enchantment && e.0.conflicts_with(enchantment))
            {
                return Err(format!(
                    "{} can't have that enchantment with the ones it has",
                    items_display(stack.id)
                ));
            }
            stack.enchant(enchantment, level);
            g.player.inventory.set(slot, Some(stack));
            Ok(())
        });
        done?;
        replies.push(Reply::Sender(format!(
            "Applied enchantment {} {} to {name}'s item",
            enchantment.name(),
            roman(level)
        )));
    }
    Ok(replies)
}

fn roman(n: u8) -> &'static str {
    [
        "", "I", "II", "III", "IV", "V", "VI", "VII", "VIII", "IX", "X",
    ]
    .get(usize::from(n))
    .copied()
    .unwrap_or("?")
}

fn xp(game: &mut Game, caller: &Caller, args: &[&str]) -> Result<Vec<Reply>, String> {
    let (Some(action), Some(who)) = (args.first().copied(), args.get(1)) else {
        return Err("Usage: /xp <add|set|query> <player> [amount] [points|levels]".to_owned());
    };
    let levels =
        args.get(3) == Some(&"levels") || (action == "query" && args.get(2) == Some(&"levels"));
    let mut replies = Vec::new();
    for index in game.players_named(caller, who)? {
        let name = game.player_at(index).name.clone();
        let line = game.with_player(index, |g| -> Result<String, String> {
            let xp = &mut g.player.stats.experience;
            match action {
                "add" => {
                    let amount: u32 = number(args.get(2).ok_or("Missing amount")?, "amount")?;
                    if levels {
                        xp.level += amount;
                        Ok(format!("Gave {amount} experience levels to {name}"))
                    } else {
                        xp.add(amount);
                        Ok(format!("Gave {amount} experience points to {name}"))
                    }
                }
                "set" => {
                    let amount: u32 = number(args.get(2).ok_or("Missing amount")?, "amount")?;
                    if levels {
                        xp.level = amount;
                        Ok(format!("Set {amount} experience levels on {name}"))
                    } else {
                        let level = xp.level;
                        xp.set_points(crate::player::experience::points_to_reach(level) + amount);
                        Ok(format!("Set {amount} experience points on {name}"))
                    }
                }
                "query" if levels => Ok(format!("{name} has {} experience levels", xp.level)),
                "query" => Ok(format!("{name} has {} experience points", xp.points())),
                other => Err(format!("Unknown action: {other}")),
            }
        })?;
        replies.push(Reply::Sender(line));
    }
    Ok(replies)
}

fn gamerule(game: &mut Game, _: &Caller, args: &[&str]) -> Result<Vec<Reply>, String> {
    let Some(rule) = args.first() else {
        return Err("Usage: /gamerule <rule> [value]".to_owned());
    };
    match args.get(1) {
        None => {
            let value = game
                .rules
                .get(rule)
                .ok_or_else(|| format!("Unknown game rule: {rule}"))?;
            Ok(tell(format!(
                "Gamerule {rule} is currently set to: {value}"
            )))
        }
        Some(value) => {
            game.rules.set(rule, value)?;
            Ok(tell(format!("Gamerule {rule} is now set to: {value}")))
        }
    }
}

fn spawnpoint(game: &mut Game, caller: &Caller, args: &[&str]) -> Result<Vec<Reply>, String> {
    let who = match args.first() {
        Some(word) => game.players_named(caller, word)?,
        None => vec![caller.index],
    };
    let mut replies = Vec::new();
    for index in who {
        let at = match args.get(1..4) {
            Some(words) => block_position(words, game.feet_of(caller.index))?,
            None => game.feet_of(index).floor().as_ivec3(),
        };
        game.with_player(index, |g| g.player.spawn_point = Some(at));
        let name = game.player_at(index).name.clone();
        let dimension = game.player_at(index).dimension;
        replies.push(Reply::Sender(format!(
            "Set spawn point to {}, {}, {} in minecraft:{} for {name}",
            at.x,
            at.y,
            at.z,
            Game::dimension_slug(dimension)
        )));
    }
    Ok(replies)
}

fn setworldspawn(game: &mut Game, caller: &Caller, args: &[&str]) -> Result<Vec<Reply>, String> {
    let origin = game.feet_of(caller.index);
    let at = match args.get(0..3) {
        Some(words) => block_position(words, origin)?,
        None => origin.floor().as_ivec3(),
    };
    game.spawn = at.as_dvec3() + DVec3::new(0.5, 0.0, 0.5);
    if let Some(saved) = game.saved.as_mut() {
        saved.level.spawn = game.spawn.to_array();
    }
    Ok(tell(format!(
        "Set the world spawn point to {}, {}, {}",
        at.x, at.y, at.z
    )))
}

fn locate(game: &mut Game, caller: &Caller, args: &[&str]) -> Result<Vec<Reply>, String> {
    let (Some(what), Some(name)) = (args.first().copied(), args.get(1)) else {
        return Err("Usage: /locate <structure|biome> <name>".to_owned());
    };
    let name = name.strip_prefix("minecraft:").unwrap_or(name);
    let from = game.feet_of(caller.index);
    if game.player_at(caller.index).dimension != Dimension::Overworld {
        return Err("Only the Overworld can be searched".to_owned());
    }
    if what == "poi" {
        let at = extra::locate_poi(game, from, name)?;
        let away = at.as_dvec3().distance(from);
        return Ok(tell(format!(
            "The nearest {name} is at §a[{}, ~, {}]§r ({} blocks away)",
            at.x,
            at.z,
            away.round() as i64
        )));
    }
    let terrain = game
        .chunks
        .terrain()
        .ok_or("This world has no terrain to search")?;
    let found = match what {
        "structure" => {
            let kind = structure_kind(name).ok_or_else(|| format!("Unknown structure: {name}"))?;
            if kind == Kind::Stronghold {
                game.nearest_stronghold(from)
                    .map(|(x, z)| IVec3::new(x as i32, 64, z as i32))
            } else {
                nearest_structure(terrain, kind, from)
            }
        }
        "biome" => {
            let biome = Biome::ALL
                .iter()
                .copied()
                .find(|b| biome_id(*b) == name)
                .ok_or_else(|| format!("Unknown biome: {name}"))?;
            nearest_biome(terrain, biome, from)
        }
        other => return Err(format!("Unknown kind: {other}")),
    };
    let Some(at) = found else {
        return Err(format!("Could not find a {what} of type \"{name}\" nearby"));
    };
    let away = DVec3::new(f64::from(at.x) - from.x, 0.0, f64::from(at.z) - from.z).length();
    Ok(tell(format!(
        "The nearest {name} is at §a[{}, ~, {}]§r ({} blocks away)",
        at.x,
        at.z,
        away.round() as i64
    )))
}

/// A structure's vanilla id.
fn structure_kind(name: &str) -> Option<Kind> {
    Some(match name {
        "village" | "village_plains" | "village_desert" | "village_savanna" | "village_taiga"
        | "village_snowy" => Kind::Village,
        "desert_pyramid" => Kind::DesertTemple,
        "jungle_pyramid" => Kind::JungleTemple,
        "shipwreck" => Kind::Shipwreck,
        "ruined_portal" => Kind::RuinedPortal,
        "mineshaft" => Kind::Mineshaft,
        "stronghold" => Kind::Stronghold,
        _ => return None,
    })
}

/// The nearest structure of `kind` that's made, searching outward.
fn nearest_structure(
    terrain: &crate::world::worldgen::terrain::Terrain,
    kind: Kind,
    from: DVec3,
) -> Option<IVec3> {
    let center = ChunkPos::of_block(from.x.floor() as i32, from.z.floor() as i32);
    let mut best: Option<(f64, IVec3)> = None;
    let mut seen = std::collections::HashSet::new();
    for ring in 0..LOCATE_CHUNKS {
        // Anything found closer than this ring can't be beaten.
        if best.is_some_and(|(d, _)| d < f64::from((ring - 1).max(0) * 16)) {
            break;
        }
        for dz in -ring..=ring {
            for dx in -ring..=ring {
                if dx.abs().max(dz.abs()) != ring {
                    continue;
                }
                let pos = center.offset(dx, dz);
                for start in kind.starts_near(terrain.seed(), pos) {
                    if !seen.insert(start) {
                        continue;
                    }
                    if let Some(found) = structure::structure_at(terrain, kind, start) {
                        let c = found.bounds.center();
                        let d = DVec3::new(f64::from(c.x), 0.0, f64::from(c.z))
                            .distance(DVec3::new(from.x, 0.0, from.z));
                        if best.is_none_or(|(b, _)| d < b) {
                            best = Some((d, c));
                        }
                    }
                }
            }
        }
    }
    best.map(|(_, at)| at)
}

/// The nearest column in `biome`, searching outward every 32 blocks.
fn nearest_biome(
    terrain: &crate::world::worldgen::terrain::Terrain,
    biome: Biome,
    from: DVec3,
) -> Option<IVec3> {
    let (x0, z0) = (from.x.floor() as i32, from.z.floor() as i32);
    const STEP: i32 = 32;
    for ring in 0..=LOCATE_BIOME_BLOCKS / STEP {
        let mut best: Option<(i64, IVec3)> = None;
        for dz in -ring..=ring {
            for dx in -ring..=ring {
                if dx.abs().max(dz.abs()) != ring {
                    continue;
                }
                let (x, z) = (x0 + dx * STEP, z0 + dz * STEP);
                if terrain.biome_at(x, z) == biome {
                    let d = i64::from(dx * dx + dz * dz);
                    if best.is_none_or(|(b, _)| d < b) {
                        best = Some((d, IVec3::new(x, 64, z)));
                    }
                }
            }
        }
        if let Some((_, at)) = best {
            return Some(at);
        }
    }
    None
}

fn difficulty(game: &mut Game, _: &Caller, args: &[&str]) -> Result<Vec<Reply>, String> {
    let Some(word) = args.first() else {
        return Ok(tell(format!(
            "The difficulty is {}",
            game.difficulty.name()
        )));
    };
    let chosen = Difficulty::ALL
        .into_iter()
        .find(|d| d.name().eq_ignore_ascii_case(word))
        .ok_or_else(|| format!("Unknown difficulty: {word}"))?;
    if chosen == game.difficulty {
        return Err(format!(
            "The difficulty did not change; it is already set to {}",
            chosen.name()
        ));
    }
    game.difficulty = chosen;
    Ok(tell(format!(
        "The difficulty has been set to {}",
        chosen.name()
    )))
}

fn kick(game: &mut Game, caller: &Caller, args: &[&str]) -> Result<Vec<Reply>, String> {
    let Some(who) = args.first() else {
        return Err("Usage: /kick <player> [reason]".to_owned());
    };
    let reason = match rest(args, 1) {
        r if r.is_empty() => "Kicked by an operator".to_owned(),
        r => r,
    };
    let mut replies = Vec::new();
    for index in game.players_named(caller, who)? {
        if index == 0 {
            return Err("The host can't be kicked".to_owned());
        }
        let name = game.player_at(index).name.clone();
        replies.push(Reply::Kick(index, reason.clone()));
        replies.push(Reply::Sender(format!("Kicked {name}: {reason}")));
    }
    Ok(replies)
}

#[cfg(test)]
mod m43_tests;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game::{PlayerState, tests::flat_game};

    pub(super) fn world() -> Game {
        let mut game = flat_game(GameMode::Survival);
        game.mob_defs = crate::game::tests::mob_registry();
        game.player.name = "Kenzo".to_owned();
        let mut guest = PlayerState::new(DVec3::new(6.5, 64.0, 0.5), 0.0, 0.0);
        guest.name = "Alex".to_owned();
        game.others.push(guest);
        game
    }

    pub(super) fn run(game: &mut Game, line: &str) -> Vec<Reply> {
        game.run_command(0, line, true, true)
    }

    pub(super) fn said(replies: &[Reply]) -> String {
        replies
            .iter()
            .map(|r| match r {
                Reply::Sender(t) | Reply::Everyone(t) | Reply::Player(_, t) | Reply::Kick(_, t) => {
                    t.as_str()
                }
                Reply::Publish(_) => "<publish>",
                Reply::Stop => "<stop>",
            })
            .collect::<Vec<_>>()
            .join(" | ")
    }

    #[test]
    fn words_keep_selectors_whole_and_coordinates_can_be_relative() {
        assert_eq!(
            tokens("tp @e[type=zombie] ~ ~2 5"),
            ["tp", "@e[type=zombie]", "~", "~2", "5"]
        );
        assert_eq!(tokens("  me  waves "), ["me", "waves"]);
        let origin = DVec3::new(10.5, 64.0, -3.25);
        assert_eq!(
            position(&["~", "~2", "5"], origin).unwrap(),
            DVec3::new(10.5, 66.0, 5.0)
        );
        assert!(position(&["~", "x", "5"], origin).is_err());
        assert_eq!(
            enchantment_id(Enchantment::BaneOfArthropods),
            "bane_of_arthropods"
        );
        assert_eq!(
            enchantment_id(Enchantment::VanishingCurse),
            "vanishing_curse"
        );
    }

    #[test]
    fn selectors_find_players_and_mobs() {
        let mut game = world();
        let me = Caller {
            index: 0,
            cheats: true,
            host: true,
        };
        assert_eq!(game.players_named(&me, "@s").unwrap(), [0]);
        assert_eq!(game.players_named(&me, "@a").unwrap(), [0, 1]);
        assert_eq!(game.players_named(&me, "Alex").unwrap(), [1]);
        assert_eq!(
            game.players_named(&me, "@p").unwrap(),
            [0],
            "the nearest is yourself"
        );
        let alex = Caller {
            index: 1,
            cheats: true,
            host: false,
        };
        assert_eq!(game.players_named(&alex, "@p").unwrap(), [1]);
        assert!(game.players_named(&me, "Nobody").is_err());
        game.summon("zombie", DVec3::new(3.5, 64.0, 3.5), 0.0);
        game.summon("pig", DVec3::new(-3.5, 64.0, 3.5), 0.0);
        let zombies = game.targets(&me, "@e[type=zombie]").unwrap();
        assert_eq!(zombies.len(), 1);
        assert!(matches!(zombies[0], Target::Mob(_)));
        assert_eq!(game.targets(&me, "@e").unwrap().len(), 4);
        assert!(game.targets(&me, "@e[type=dragonfly]").is_err());
    }

    #[test]
    fn without_cheats_only_the_harmless_commands_run() {
        let mut game = world();
        let refused = game.run_command(1, "give @s diamond", false, false);
        assert!(said(&refused).contains("permission"), "{}", said(&refused));
        assert!(game.others[0].inventory.find(items::DIAMOND).is_none());
        assert!(said(&game.run_command(1, "seed", true, false)).contains("permission"));
        let list = game.run_command(1, "list", false, false);
        assert_eq!(
            said(&list),
            "There are 2 of a max of 8 players online: Kenzo, Alex"
        );
        let me = game.run_command(1, "me waves", false, false);
        assert_eq!(me, [Reply::Everyone("* Alex waves".to_owned())]);
        let msg = game.run_command(1, "tell Kenzo hi there", false, false);
        assert!(msg.contains(&Reply::Player(
            0,
            "§7§oAlex whispers to you: hi there".to_owned()
        )));
        assert!(said(&game.run_command(0, "nonsense", true, true)).contains("Unknown command"));
    }

    #[test]
    fn cheat_commands_do_what_they_say() {
        let mut game = world();
        // gamemode, give, clear
        assert_eq!(
            said(&run(&mut game, "gamemode creative")),
            "Set own game mode to Creative Mode"
        );
        assert_eq!(game.player.mode, GameMode::Creative);
        run(&mut game, "gamemode survival Alex");
        assert_eq!(game.others[0].mode, GameMode::Survival);
        assert_eq!(
            said(&run(&mut game, "give Alex minecraft:diamond 70")),
            "Gave 70 [Diamond] to Alex"
        );
        assert!(game.others[0].inventory.find(items::DIAMOND).is_some());
        assert_eq!(
            said(&run(&mut game, "clear Alex diamond")),
            "Removed 70 item(s) from player Alex"
        );
        assert!(game.others[0].inventory.find(items::DIAMOND).is_none());
        // tp
        run(&mut game, "tp 20.5 70 -4.5");
        assert_eq!(game.player.body.position(), DVec3::new(20.5, 70.0, -4.5));
        run(&mut game, "tp Alex Kenzo");
        assert_eq!(game.others[0].body.position(), DVec3::new(20.5, 70.0, -4.5));
        run(&mut game, "tp @s ~1 ~ ~");
        assert_eq!(game.player.body.position(), DVec3::new(21.5, 70.0, -4.5));
        // time
        run(&mut game, "time set noon");
        assert_eq!(game.day_time() % 24000, 6000);
        run(&mut game, "time add 1000");
        assert_eq!(game.day_time() % 24000, 7000);
        assert_eq!(
            said(&run(&mut game, "time query daytime")),
            "The time is 7000"
        );
        // effect, enchant, xp
        run(&mut game, "effect give Alex speed 10 1");
        assert!(game.others[0].stats.effects.has(Effect::Speed));
        run(&mut game, "effect clear Alex");
        assert!(!game.others[0].stats.effects.has(Effect::Speed));
        game.player
            .inventory
            .set(0, Some(ItemStack::new(items::DIAMOND_SWORD, 1)));
        game.player.inventory.selected = 0;
        assert_eq!(
            said(&run(&mut game, "enchant @s sharpness 5")),
            "Applied enchantment Sharpness V to Kenzo's item"
        );
        let protection = run(&mut game, "enchant @s protection 1");
        assert!(
            said(&protection).starts_with("§c"),
            "a sword takes no Protection"
        );
        run(&mut game, "xp add Kenzo 5 levels");
        assert_eq!(game.player.stats.experience.level, 5);
        assert_eq!(
            said(&run(&mut game, "xp query Kenzo levels")),
            "Kenzo has 5 experience levels"
        );
        // summon, kill
        run(&mut game, "summon zombie ~2 ~ ~");
        assert!(
            game.mobs
                .iter()
                .any(|m| game.mob_defs.get(m.kind).name == "zombie")
        );
        assert_eq!(
            said(&run(&mut game, "kill @e[type=zombie]")),
            "Killed Zombie"
        );
        assert!(
            game.mobs
                .iter()
                .filter(|m| game.mob_defs.get(m.kind).name == "zombie")
                .all(|m| m.is_dead())
        );
        run(&mut game, "kill Alex");
        assert!(game.others[0].stats.is_dead());
        // setblock, fill
        assert_eq!(
            said(&run(&mut game, "setblock 2 64 2 stone")),
            "Changed the block at 2, 64, 2"
        );
        assert!(
            game.chunks
                .world()
                .block(2, 64, 2)
                .unwrap()
                .is(crate::world::blocks::STONE)
        );
        assert_eq!(
            said(&run(&mut game, "fill 0 65 0 2 67 2 glass hollow")),
            "Successfully filled 26 block(s)"
        );
        assert!(
            game.chunks.world().block(1, 66, 1).unwrap().is_air(),
            "hollow inside"
        );
        assert!(said(&run(&mut game, "fill 0 0 0 100 100 100 stone")).contains("Too many blocks"));
        // gamerule, difficulty, spawn points
        assert_eq!(
            said(&run(&mut game, "gamerule keepInventory true")),
            "Gamerule keepInventory is now set to: true"
        );
        assert!(game.rules.keep_inventory);
        run(&mut game, "gamerule doDaylightCycle false");
        let day = game.day_time();
        game.run_ticks(20);
        assert_eq!(game.day_time(), day, "the sun stands still");
        assert!(said(&run(&mut game, "gamerule randomTickSpeed x")).starts_with("§c"));
        run(&mut game, "difficulty hard");
        assert_eq!(game.difficulty, Difficulty::Hard);
        run(&mut game, "spawnpoint Kenzo 5 64 5");
        assert_eq!(game.player.spawn_point, Some(IVec3::new(5, 64, 5)));
        run(&mut game, "setworldspawn 1 64 1");
        assert_eq!(game.spawn_point(), DVec3::new(1.5, 64.0, 1.5));
        // kick
        assert!(run(&mut game, "kick Alex bye").contains(&Reply::Kick(1, "bye".to_owned())));
        assert!(said(&game.run_command(1, "kick Kenzo", true, false)).contains("permission"));
    }

    #[test]
    fn tab_completes_commands_and_their_arguments() {
        let game = world();
        assert_eq!(
            game.complete_command("gam", true, true),
            ["gamemode", "gamerule"]
        );
        assert!(
            game.complete_command("gam", false, false).is_empty(),
            "not without cheats"
        );
        assert_eq!(
            game.complete_command("gamemode cr", true, true),
            ["creative"]
        );
        assert_eq!(game.complete_command("give Al", true, true), ["Alex"]);
        assert!(
            game.complete_command("give Alex diamond_sw", true, true)
                .contains(&"diamond_sword".to_owned())
        );
        assert_eq!(
            game.complete_command("gamerule keepInv", true, true),
            ["keepInventory"]
        );
        assert_eq!(
            game.complete_command("effect give @s regen", true, true),
            ["regeneration"]
        );
    }
}
