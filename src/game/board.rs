//! The scoreboard (M44): objectives and their criteria, scores kept for
//! players and other entities, display slots, teams, and bossbars. Pure
//! data and rules; `commands/board_cmds.rs` is the commands that edit it
//! and `Game` feeds it the criteria events. The world keeps the objectives,
//! scores, slots and teams in `scoreboard.ron`.

use crate::platform::fs;
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};

use serde::{Deserialize, Serialize};

/// Vanilla's 16 chat colors by id, with their `§` code.
pub const COLORS: [(&str, char); 16] = [
    ("black", '0'),
    ("dark_blue", '1'),
    ("dark_green", '2'),
    ("dark_aqua", '3'),
    ("dark_red", '4'),
    ("dark_purple", '5'),
    ("gold", '6'),
    ("gray", '7'),
    ("dark_gray", '8'),
    ("blue", '9'),
    ("green", 'a'),
    ("aqua", 'b'),
    ("red", 'c'),
    ("light_purple", 'd'),
    ("yellow", 'e'),
    ("white", 'f'),
];

/// The `§` code letter of a color name, if it is one of the 16.
pub fn color_code(name: &str) -> Option<char> {
    COLORS.iter().find(|(n, _)| *n == name).map(|(_, c)| *c)
}

/// What an objective counts, parsed from its criteria text.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Criteria {
    /// Only commands change it.
    Dummy,
    /// Players raise it with `/trigger` once enabled.
    Trigger,
    DeathCount,
    PlayerKillCount,
    TotalKillCount,
    /// Read from the player each tick.
    Health,
    Food,
    Air,
    Armor,
    Level,
    Xp,
    /// `minecraft.<kind>:minecraft.<id>`: mined, used, killed, ... counts.
    Stat {
        kind: String,
        id: String,
    },
}

impl Criteria {
    pub fn parse(text: &str) -> Result<Criteria, String> {
        Ok(match text {
            "dummy" => Criteria::Dummy,
            "trigger" => Criteria::Trigger,
            "deathCount" => Criteria::DeathCount,
            "playerKillCount" => Criteria::PlayerKillCount,
            "totalKillCount" => Criteria::TotalKillCount,
            "health" => Criteria::Health,
            "food" => Criteria::Food,
            "air" => Criteria::Air,
            "armor" => Criteria::Armor,
            "level" => Criteria::Level,
            "xp" => Criteria::Xp,
            other => {
                let (kind, id) = other
                    .split_once(':')
                    .ok_or_else(|| format!("Unknown scoreboard criteria: {other}"))?;
                let kind = kind
                    .strip_prefix("minecraft.")
                    .ok_or_else(|| format!("Unknown scoreboard criteria: {other}"))?;
                const KINDS: &[&str] = &[
                    "mined",
                    "crafted",
                    "used",
                    "broken",
                    "picked_up",
                    "dropped",
                    "killed",
                    "killed_by",
                    "custom",
                ];
                if !KINDS.contains(&kind) {
                    return Err(format!("Unknown scoreboard criteria: {other}"));
                }
                Criteria::Stat {
                    kind: kind.to_owned(),
                    id: id.strip_prefix("minecraft.").unwrap_or(id).to_owned(),
                }
            }
        })
    }

    /// The text it was made from.
    pub fn text(&self) -> String {
        match self {
            Criteria::Dummy => "dummy".to_owned(),
            Criteria::Trigger => "trigger".to_owned(),
            Criteria::DeathCount => "deathCount".to_owned(),
            Criteria::PlayerKillCount => "playerKillCount".to_owned(),
            Criteria::TotalKillCount => "totalKillCount".to_owned(),
            Criteria::Health => "health".to_owned(),
            Criteria::Food => "food".to_owned(),
            Criteria::Air => "air".to_owned(),
            Criteria::Armor => "armor".to_owned(),
            Criteria::Level => "level".to_owned(),
            Criteria::Xp => "xp".to_owned(),
            Criteria::Stat { kind, id } => format!("minecraft.{kind}:minecraft.{id}"),
        }
    }

    /// The game sets these itself every tick.
    pub fn is_read_only(&self) -> bool {
        matches!(
            self,
            Criteria::Health
                | Criteria::Food
                | Criteria::Air
                | Criteria::Armor
                | Criteria::Level
                | Criteria::Xp
        )
    }
}

/// How a score shows on `below_name` and `list`: as a number or hearts.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum RenderType {
    #[default]
    Integer,
    Hearts,
}

/// What shows in place of the score number (`numberformat`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum NumberFormat {
    Blank,
    /// A fixed text, already with its `§` codes.
    Fixed(String),
    /// The number in a color (`§` code letter).
    Styled(char),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Objective {
    pub criteria: Criteria,
    /// The name shown (with `§` codes), the id if never set.
    pub display_name: String,
    pub render_type: RenderType,
    pub number_format: Option<NumberFormat>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Visibility {
    #[default]
    Always,
    Never,
    HideForOtherTeams,
    HideForOwnTeam,
}

impl Visibility {
    pub const NAMES: [&'static str; 4] = ["always", "never", "hideForOtherTeams", "hideForOwnTeam"];

    pub fn parse(word: &str) -> Option<Self> {
        Some(match word {
            "always" => Visibility::Always,
            "never" => Visibility::Never,
            "hideForOtherTeams" => Visibility::HideForOtherTeams,
            "hideForOwnTeam" => Visibility::HideForOwnTeam,
            _ => return None,
        })
    }

    pub fn name(self) -> &'static str {
        Self::NAMES[self as usize]
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Collision {
    #[default]
    Always,
    Never,
    PushOtherTeams,
    PushOwnTeam,
}

impl Collision {
    pub const NAMES: [&'static str; 4] = ["always", "never", "pushOtherTeams", "pushOwnTeam"];

    pub fn parse(word: &str) -> Option<Self> {
        Some(match word {
            "always" => Collision::Always,
            "never" => Collision::Never,
            "pushOtherTeams" => Collision::PushOtherTeams,
            "pushOwnTeam" => Collision::PushOwnTeam,
            _ => return None,
        })
    }

    pub fn name(self) -> &'static str {
        Self::NAMES[self as usize]
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Team {
    pub display_name: String,
    /// A color name, or `None` for `reset`.
    pub color: Option<String>,
    pub prefix: String,
    pub suffix: String,
    pub friendly_fire: bool,
    pub see_friendly_invisibles: bool,
    pub nametag_visibility: Visibility,
    pub death_message_visibility: Visibility,
    pub collision_rule: Collision,
    pub members: BTreeSet<String>,
}

impl Team {
    pub fn new(name: &str) -> Self {
        Team {
            display_name: name.to_owned(),
            color: None,
            prefix: String::new(),
            suffix: String::new(),
            friendly_fire: true,
            see_friendly_invisibles: true,
            nametag_visibility: Visibility::Always,
            death_message_visibility: Visibility::Always,
            collision_rule: Collision::Always,
            members: BTreeSet::new(),
        }
    }

    /// `name` dressed in the team's prefix, color and suffix.
    pub fn format(&self, name: &str) -> String {
        let color = self
            .color
            .as_deref()
            .and_then(color_code)
            .map(|c| format!("§{c}"))
            .unwrap_or_default();
        let reset = if color.is_empty() { "" } else { "§r" };
        format!("{}{color}{name}{reset}{}", self.prefix, self.suffix)
    }
}

/// Everything the scoreboard command family keeps.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Scoreboard {
    pub objectives: BTreeMap<String, Objective>,
    /// Objective → holder → score. A holder is a player's name, a made-up
    /// name, or `entity.<id>` for a mob (for as long as it lives).
    pub scores: BTreeMap<String, BTreeMap<String, i32>>,
    /// `list`, `sidebar`, `below_name`, `sidebar.team.<color>` → objective.
    pub display: BTreeMap<String, String>,
    pub teams: BTreeMap<String, Team>,
    /// Trigger objective → holders who may use it now.
    pub enabled: BTreeMap<String, BTreeSet<String>>,
}

pub const SLOTS: [&str; 3] = ["list", "sidebar", "below_name"];

/// Whether `slot` is a display slot name (`sidebar.team.red` included).
pub fn is_slot(slot: &str) -> bool {
    SLOTS.contains(&slot)
        || slot
            .strip_prefix("sidebar.team.")
            .is_some_and(|c| color_code(c).is_some())
}

impl Scoreboard {
    pub fn score(&self, objective: &str, holder: &str) -> Option<i32> {
        self.scores.get(objective)?.get(holder).copied()
    }

    pub fn set(&mut self, objective: &str, holder: &str, value: i32) {
        self.scores
            .entry(objective.to_owned())
            .or_default()
            .insert(holder.to_owned(), value);
    }

    /// Adds to a holder's score (a missing score counts as 0) and returns it.
    pub fn add(&mut self, objective: &str, holder: &str, by: i32) -> i32 {
        let entry = self
            .scores
            .entry(objective.to_owned())
            .or_default()
            .entry(holder.to_owned())
            .or_insert(0);
        *entry = entry.wrapping_add(by);
        *entry
    }

    pub fn reset(&mut self, holder: &str, objective: Option<&str>) -> bool {
        let mut any = false;
        for (name, scores) in &mut self.scores {
            if objective.is_none_or(|o| o == name) {
                any |= scores.remove(holder).is_some();
            }
        }
        any
    }

    /// Every holder with a score somewhere.
    pub fn holders(&self) -> BTreeSet<String> {
        self.scores
            .values()
            .flat_map(|scores| scores.keys().cloned())
            .collect()
    }

    /// The scores a holder has: objective and value, in name order.
    pub fn scores_of(&self, holder: &str) -> Vec<(String, i32)> {
        self.scores
            .iter()
            .filter_map(|(o, scores)| scores.get(holder).map(|v| (o.clone(), *v)))
            .collect()
    }

    /// Removes an objective with its scores, display slots and triggers.
    pub fn remove_objective(&mut self, name: &str) -> bool {
        let had = self.objectives.remove(name).is_some();
        self.scores.remove(name);
        self.enabled.remove(name);
        self.display.retain(|_, o| o != name);
        had
    }

    /// Objectives whose criteria is `which`.
    pub fn with_criteria<'a>(&'a self, which: &'a Criteria) -> impl Iterator<Item = &'a str> + 'a {
        self.objectives
            .iter()
            .filter(move |(_, o)| &o.criteria == which)
            .map(|(n, _)| n.as_str())
    }

    pub fn team_of(&self, holder: &str) -> Option<(&str, &Team)> {
        self.teams
            .iter()
            .find(|(_, t)| t.members.contains(holder))
            .map(|(n, t)| (n.as_str(), t))
    }

    /// `holder`'s name as the team dresses it (the plain name without one).
    pub fn dressed(&self, holder: &str) -> String {
        self.team_of(holder)
            .map_or_else(|| holder.to_owned(), |(_, team)| team.format(holder))
    }

    /// Counts a stat event: adds `by` to every objective counting `kind` of `id`.
    pub fn count_stat(&mut self, holder: &str, kind: &str, id: &str, by: i32) {
        let id = id.strip_prefix("minecraft:").unwrap_or(id);
        let hits: Vec<String> = self
            .objectives
            .iter()
            .filter(|(_, o)| {
                matches!(&o.criteria, Criteria::Stat { kind: k, id: i } if k == kind && i == id)
            })
            .map(|(n, _)| n.clone())
            .collect();
        for name in hits {
            self.add(&name, holder, by);
        }
    }

    /// Adds to every objective with this plain `criteria` for `holder`.
    pub fn count(&mut self, holder: &str, criteria: &Criteria, by: i32) {
        let hits: Vec<String> = self.with_criteria(criteria).map(str::to_owned).collect();
        for name in hits {
            self.add(&name, holder, by);
        }
    }

    /// Writes `value` into every objective with this read-only criteria.
    pub fn track(&mut self, holder: &str, criteria: &Criteria, value: i32) {
        let hits: Vec<String> = self.with_criteria(criteria).map(str::to_owned).collect();
        for name in hits {
            self.set(&name, holder, value);
        }
    }

    pub fn load(dir: &Path) -> Self {
        fs::read_to_string(dir.join("scoreboard.ron"))
            .ok()
            .and_then(|text| ron::from_str(&text).ok())
            .unwrap_or_default()
    }

    pub fn save(&self, dir: &Path) {
        match ron::ser::to_string_pretty(self, ron::ser::PrettyConfig::default()) {
            Ok(text) => {
                if let Err(err) = fs::write(dir.join("scoreboard.ron"), text) {
                    log::warn!("couldn't write scoreboard.ron: {err}");
                }
            }
            Err(err) => log::warn!("couldn't encode the scoreboard: {err}"),
        }
    }

    /// Applies one of `/scoreboard players operation`'s operators to a
    /// target's score from a source's. Returns the new target score.
    pub fn operate(op: &str, target: i32, source: i32) -> Result<(i32, Option<i32>), String> {
        Ok(match op {
            "+=" => (target.wrapping_add(source), None),
            "-=" => (target.wrapping_sub(source), None),
            "*=" => (target.wrapping_mul(source), None),
            "/=" if source == 0 => return Err("Cannot divide by zero".to_owned()),
            "/=" => (target.div_euclid(source), None),
            "%=" if source == 0 => return Err("Cannot divide by zero".to_owned()),
            "%=" => (target.rem_euclid(source), None),
            "=" => (source, None),
            "<" => (target.min(source), None),
            ">" => (target.max(source), None),
            // `><` swaps them: the target takes the source's, the source the target's.
            "><" => (source, Some(target)),
            other => return Err(format!("Unknown operation: {other}")),
        })
    }
}

/// A bossbar's color and look (`/bossbar set ... color|style`).
pub const BAR_COLORS: [&str; 7] = ["pink", "blue", "red", "green", "yellow", "purple", "white"];
pub const BAR_STYLES: [&str; 5] = [
    "progress",
    "notched_6",
    "notched_10",
    "notched_12",
    "notched_20",
];

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Bossbar {
    /// The name shown, with `§` codes.
    pub name: String,
    pub color: String,
    pub style: String,
    pub value: i32,
    pub max: i32,
    pub visible: bool,
    /// Who sees it: `None` for nobody until `players` is set.
    pub players: BTreeSet<String>,
}

impl Bossbar {
    pub fn new(name: &str) -> Self {
        Bossbar {
            name: name.to_owned(),
            color: "white".to_owned(),
            style: "progress".to_owned(),
            value: 0,
            max: 100,
            visible: true,
            players: BTreeSet::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn criteria_parse_and_print_the_same_text() {
        for text in [
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
            "minecraft.mined:minecraft.stone",
            "minecraft.used:minecraft.dirt",
            "minecraft.killed:minecraft.zombie",
        ] {
            assert_eq!(Criteria::parse(text).unwrap().text(), text);
        }
        assert!(Criteria::parse("nonsense").is_err());
        assert!(Criteria::parse("minecraft.flown:minecraft.x").is_err());
        assert!(Criteria::Health.is_read_only() && !Criteria::Dummy.is_read_only());
    }

    #[test]
    fn scores_add_reset_and_remove_with_their_objective() {
        let mut board = Scoreboard::default();
        board.objectives.insert(
            "kills".into(),
            Objective {
                criteria: Criteria::Dummy,
                display_name: "kills".into(),
                render_type: RenderType::Integer,
                number_format: None,
            },
        );
        board.display.insert("sidebar".into(), "kills".into());
        assert_eq!(board.add("kills", "Kenzo", 2), 2);
        assert_eq!(board.add("kills", "Kenzo", 3), 5);
        board.set("kills", "Alex", 1);
        assert_eq!(board.scores_of("Kenzo"), [("kills".to_owned(), 5)]);
        assert_eq!(board.holders().len(), 2);
        assert!(board.reset("Alex", None));
        assert!(!board.reset("Alex", None));
        assert!(board.remove_objective("kills"));
        assert!(board.display.is_empty() && board.scores.is_empty());
    }

    #[test]
    fn operations_follow_vanillas_rules() {
        let op = |o, a, b| Scoreboard::operate(o, a, b);
        assert_eq!(op("+=", 5, 3), Ok((8, None)));
        assert_eq!(op("-=", 5, 3), Ok((2, None)));
        assert_eq!(op("*=", 5, 3), Ok((15, None)));
        assert_eq!(op("/=", -7, 2), Ok((-4, None)), "floor division");
        assert_eq!(op("%=", -7, 2), Ok((1, None)), "positive remainder");
        assert_eq!(op("=", 5, 3), Ok((3, None)));
        assert_eq!(op("<", 5, 3), Ok((3, None)));
        assert_eq!(op(">", 5, 3), Ok((5, None)));
        assert_eq!(op("><", 5, 3), Ok((3, Some(5))));
        assert!(op("/=", 5, 0).is_err() && op("%=", 5, 0).is_err());
        assert_eq!(op("*=", i32::MAX, 2), Ok((-2, None)), "wraps like Java");
    }

    #[test]
    fn stats_count_only_the_matching_objectives() {
        let mut board = Scoreboard::default();
        for (name, text) in [
            ("stone", "minecraft.mined:minecraft.stone"),
            ("dirt", "minecraft.mined:minecraft.dirt"),
            ("deaths", "deathCount"),
        ] {
            board.objectives.insert(
                name.into(),
                Objective {
                    criteria: Criteria::parse(text).unwrap(),
                    display_name: name.into(),
                    render_type: RenderType::Integer,
                    number_format: None,
                },
            );
        }
        board.count_stat("Kenzo", "mined", "minecraft:stone", 1);
        board.count_stat("Kenzo", "mined", "stone", 1);
        board.count("Kenzo", &Criteria::DeathCount, 1);
        assert_eq!(board.score("stone", "Kenzo"), Some(2));
        assert_eq!(board.score("dirt", "Kenzo"), None);
        assert_eq!(board.score("deaths", "Kenzo"), Some(1));
    }

    #[test]
    fn teams_dress_names_and_the_board_saves_and_loads() {
        let mut board = Scoreboard::default();
        let mut red = Team::new("red");
        red.color = Some("red".into());
        red.prefix = "[R] ".into();
        red.members.insert("Kenzo".into());
        board.teams.insert("red".into(), red);
        assert_eq!(board.dressed("Kenzo"), "[R] §cKenzo§r");
        assert_eq!(board.dressed("Alex"), "Alex");
        assert_eq!(board.team_of("Kenzo").unwrap().0, "red");
        board.set("kills", "Kenzo", 3);
        let dir = crate::platform::env::temp_dir()
            .join(format!("rc-board-{}", crate::platform::env::process_id()));
        fs::create_dir_all(&dir).unwrap();
        board.save(&dir);
        assert_eq!(Scoreboard::load(&dir), board);
        fs::remove_dir_all(&dir).ok();
        assert!(is_slot("sidebar.team.red") && is_slot("list") && !is_slot("sidebar.team.pink"));
    }
}

/// One bossbar as a viewer sees it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BarView {
    pub name: String,
    pub color: String,
    pub style: String,
    pub value: i32,
    pub max: i32,
}

/// The sidebar: its title and rows (name, score text), best score first.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Sidebar {
    pub title: String,
    pub rows: Vec<(String, String)>,
}

/// What one player is shown of the scoreboard: the sidebar, the tab list's
/// and below-name scores, team-dressed names, and their bossbars.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct BoardView {
    pub sidebar: Option<Sidebar>,
    /// Online players with the score the tab list shows, if a `list`
    /// objective is set.
    pub list: Vec<(String, String)>,
    /// The `below_name` objective's label and each player's score.
    pub below_name: Option<(String, Vec<(String, i32)>)>,
    /// Players' names as their teams dress them (only those on a team).
    pub dressed: Vec<(String, String)>,
    /// Players whose name tag this viewer does not see.
    pub hidden_tags: Vec<String>,
    pub bars: Vec<BarView>,
    /// Teams and their members, for the spectator menu.
    pub teams: Vec<(String, Vec<String>)>,
}

impl Scoreboard {
    /// A score as the number format shows it.
    pub fn score_text(&self, objective: &str, value: i32) -> String {
        match self
            .objectives
            .get(objective)
            .and_then(|o| o.number_format.as_ref())
        {
            Some(NumberFormat::Blank) => String::new(),
            Some(NumberFormat::Fixed(text)) => text.clone(),
            Some(NumberFormat::Styled(c)) => format!("§{c}{value}"),
            None => format!("§c{value}"),
        }
    }

    /// The scoreboard as `viewer` sees it, given everyone online.
    pub fn view_for(&self, viewer: &str, online: &[String]) -> BoardView {
        let slot = self
            .team_of(viewer)
            .and_then(|(_, t)| t.color.as_deref())
            .map(|c| format!("sidebar.team.{c}"))
            .and_then(|s| self.display.get(&s))
            .or_else(|| self.display.get("sidebar"));
        let sidebar = slot.and_then(|name| {
            let objective = self.objectives.get(name)?;
            let mut rows: Vec<(&String, i32)> = self
                .scores
                .get(name)
                .map(|s| s.iter().map(|(h, v)| (h, *v)).collect())
                .unwrap_or_default();
            // Best first; ties in reverse name order, as vanilla lists them.
            rows.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| b.0.cmp(a.0)));
            rows.truncate(15);
            Some(Sidebar {
                title: objective.display_name.clone(),
                rows: rows
                    .into_iter()
                    .map(|(h, v)| (self.dressed(h), self.score_text(name, v)))
                    .collect(),
            })
        });
        let list = self
            .display
            .get("list")
            .map(|name| {
                online
                    .iter()
                    .map(|p| {
                        let score = self.score(name, p).unwrap_or(0);
                        (p.clone(), self.score_text(name, score))
                    })
                    .collect()
            })
            .unwrap_or_default();
        let below_name = self.display.get("below_name").and_then(|name| {
            let objective = self.objectives.get(name)?;
            Some((
                objective.display_name.clone(),
                online
                    .iter()
                    .filter_map(|p| Some((p.clone(), self.score(name, p)?)))
                    .collect(),
            ))
        });
        let mine = self.team_of(viewer).map(|(n, _)| n.to_owned());
        let mut dressed = Vec::new();
        let mut hidden_tags = Vec::new();
        for p in online {
            let Some((team_name, team)) = self.team_of(p) else {
                continue;
            };
            dressed.push((p.clone(), team.format(p)));
            let same = mine.as_deref() == Some(team_name);
            let hide = match team.nametag_visibility {
                Visibility::Always => false,
                Visibility::Never => true,
                Visibility::HideForOtherTeams => !same,
                Visibility::HideForOwnTeam => same,
            };
            if hide {
                hidden_tags.push(p.clone());
            }
        }
        BoardView {
            sidebar,
            list,
            below_name,
            dressed,
            hidden_tags,
            bars: Vec::new(),
            teams: self
                .teams
                .iter()
                .map(|(n, t)| (n.clone(), t.members.iter().cloned().collect()))
                .collect(),
        }
    }
}

#[cfg(test)]
mod view_tests {
    use super::*;

    fn objective(name: &str) -> Objective {
        Objective {
            criteria: Criteria::Dummy,
            display_name: name.to_owned(),
            render_type: RenderType::Integer,
            number_format: None,
        }
    }

    #[test]
    fn the_sidebar_lists_the_best_fifteen_and_a_team_can_have_its_own() {
        let mut board = Scoreboard::default();
        board.objectives.insert("kills".into(), objective("Kills"));
        board.objectives.insert("red_only".into(), objective("Red"));
        board.display.insert("sidebar".into(), "kills".into());
        for i in 0..20 {
            board.set("kills", &format!("p{i:02}"), i);
        }
        let all = vec!["Kenzo".to_owned()];
        let view = board.view_for("Kenzo", &all);
        let side = view.sidebar.unwrap();
        assert_eq!(side.title, "Kills");
        assert_eq!(side.rows.len(), 15);
        assert_eq!(side.rows[0], ("p19".to_owned(), "§c19".to_owned()));
        // A team with a sidebar of its own sees that instead.
        let mut red = Team::new("red");
        red.color = Some("red".into());
        red.members.insert("Kenzo".into());
        board.teams.insert("red".into(), red);
        board
            .display
            .insert("sidebar.team.red".into(), "red_only".into());
        board.set("red_only", "Kenzo", 7);
        let view = board.view_for("Kenzo", &all);
        let side = view.sidebar.unwrap();
        assert_eq!(side.title, "Red");
        assert_eq!(side.rows, [("§cKenzo§r".to_owned(), "§c7".to_owned())]);
    }

    #[test]
    fn list_below_name_formats_and_hidden_tags() {
        let mut board = Scoreboard::default();
        board.objectives.insert("hp".into(), objective("Health"));
        board.objectives.get_mut("hp").unwrap().number_format = Some(NumberFormat::Styled('a'));
        board.display.insert("list".into(), "hp".into());
        board.display.insert("below_name".into(), "hp".into());
        board.set("hp", "Kenzo", 20);
        let online = vec!["Kenzo".to_owned(), "Alex".to_owned()];
        let view = board.view_for("Alex", &online);
        assert_eq!(
            view.list,
            [
                ("Kenzo".to_owned(), "§a20".to_owned()),
                ("Alex".to_owned(), "§a0".to_owned())
            ]
        );
        assert_eq!(
            view.below_name,
            Some(("Health".to_owned(), vec![("Kenzo".to_owned(), 20)]))
        );
        board.objectives.get_mut("hp").unwrap().number_format = Some(NumberFormat::Blank);
        assert_eq!(board.score_text("hp", 5), "");
        board.objectives.get_mut("hp").unwrap().number_format =
            Some(NumberFormat::Fixed("§bfull".into()));
        assert_eq!(board.score_text("hp", 5), "§bfull");
        // Name tags: hidden from other teams, own team or everyone.
        let mut team = Team::new("secret");
        team.nametag_visibility = Visibility::HideForOtherTeams;
        team.members.insert("Kenzo".into());
        board.teams.insert("secret".into(), team);
        assert_eq!(board.view_for("Alex", &online).hidden_tags, ["Kenzo"]);
        assert!(board.view_for("Kenzo", &online).hidden_tags.is_empty());
    }
}

/// A player's on-screen titles (`/title`): what is up, and the fade times.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TitleState {
    pub title: String,
    pub subtitle: String,
    pub actionbar: String,
    /// Fade in, stay and fade out, in ticks (`/title times`).
    pub fade_in: u32,
    pub stay: u32,
    pub fade_out: u32,
    /// Ticks since the title appeared; `None` when none is showing.
    pub age: Option<u32>,
    /// Ticks the action bar text has left.
    pub actionbar_left: u32,
    /// The action bar's color cycles round the rainbow ("Now Playing").
    #[serde(default)]
    pub actionbar_rainbow: bool,
}

impl Default for TitleState {
    fn default() -> Self {
        TitleState {
            title: String::new(),
            subtitle: String::new(),
            actionbar: String::new(),
            fade_in: 10,
            stay: 70,
            fade_out: 20,
            age: None,
            actionbar_left: 0,
            actionbar_rainbow: false,
        }
    }
}

impl TitleState {
    /// The action bar shows for 60 ticks (3 s), as vanilla's.
    pub const ACTIONBAR_TICKS: u32 = 60;

    /// Shows the title (and any subtitle set before it) from the start.
    pub fn show(&mut self) {
        self.age = Some(0);
    }

    /// One game tick.
    pub fn tick(&mut self) {
        if let Some(age) = self.age {
            let total = self.fade_in + self.stay + self.fade_out;
            self.age = (age + 1 < total).then_some(age + 1);
            if self.age.is_none() {
                self.title.clear();
                self.subtitle.clear();
            }
        }
        self.actionbar_left = self.actionbar_left.saturating_sub(1);
    }

    /// How opaque the title is now, 0–1 (`None` when none is up).
    pub fn alpha(&self, partial: f32) -> Option<f32> {
        let age = self.age? as f32 + partial;
        let (a, b, c) = (self.fade_in as f32, self.stay as f32, self.fade_out as f32);
        Some(if age < a {
            age / a.max(1.0)
        } else if age < a + b {
            1.0
        } else {
            (1.0 - (age - a - b) / c.max(1.0)).max(0.0)
        })
    }

    pub fn clear(&mut self) {
        self.title.clear();
        self.subtitle.clear();
        self.actionbar.clear();
        self.age = None;
        self.actionbar_left = 0;
        self.actionbar_rainbow = false;
    }

    /// `/title reset`: clear and put the times back.
    pub fn reset(&mut self) {
        *self = TitleState::default();
    }
}

#[cfg(test)]
mod title_tests {
    use super::*;

    #[test]
    fn a_title_fades_in_stays_fades_out_and_goes() {
        let mut t = TitleState::default();
        assert_eq!(t.alpha(0.0), None);
        t.title = "Hi".into();
        t.show();
        assert_eq!(t.alpha(0.0), Some(0.0));
        for _ in 0..5 {
            t.tick();
        }
        assert_eq!(t.alpha(0.0), Some(0.5));
        for _ in 0..5 {
            t.tick();
        }
        assert_eq!(t.alpha(0.0), Some(1.0));
        for _ in 0..70 {
            t.tick();
        }
        assert_eq!(t.alpha(0.0), Some(1.0), "just starting to fade");
        for _ in 0..10 {
            t.tick();
        }
        assert_eq!(t.alpha(0.0), Some(0.5));
        for _ in 0..10 {
            t.tick();
        }
        assert_eq!(t.alpha(0.0), None);
        assert!(t.title.is_empty(), "it clears itself");
        t.actionbar = "bar".into();
        t.actionbar_left = TitleState::ACTIONBAR_TICKS;
        t.reset();
        assert_eq!(t, TitleState::default());
    }
}
