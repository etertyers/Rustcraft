//! Target selectors in full (M43): `@p @a @r @s @e @n` with vanilla's
//! `[key=value,...]` arguments. Parsing is pure; `Game::select` picks from
//! the players and mobs of the caller's dimension. Teams and scores belong
//! to the scoreboard (M44) and read what it keeps.

use glam::DVec3;

use super::Game;
use crate::player::mode::GameMode;

/// A player or a mob, by position in `Game::others` + 1 (players) or id.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Target {
    Player(usize),
    Mob(u64),
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Range {
    pub min: Option<f64>,
    pub max: Option<f64>,
}

impl Range {
    pub fn contains(&self, v: f64) -> bool {
        self.min.is_none_or(|m| v >= m) && self.max.is_none_or(|m| v <= m)
    }

    /// `5`, `..5`, `5..` or `1..5`.
    pub fn parse(text: &str) -> Result<Range, String> {
        let bad = || format!("Invalid range '{text}'");
        let num = |s: &str| s.parse::<f64>().map_err(|_| bad());
        match text.split_once("..") {
            None => {
                let v = num(text)?;
                Ok(Range {
                    min: Some(v),
                    max: Some(v),
                })
            }
            Some((lo, hi)) => {
                let range = Range {
                    min: (!lo.is_empty()).then(|| num(lo)).transpose()?,
                    max: (!hi.is_empty()).then(|| num(hi)).transpose()?,
                };
                if range.min.is_none() && range.max.is_none() {
                    return Err(bad());
                }
                if range.min.zip(range.max).is_some_and(|(a, b)| a > b) {
                    return Err("Min cannot be bigger than max".to_owned());
                }
                Ok(range)
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sort {
    Nearest,
    Furthest,
    Random,
    Arbitrary,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// `@p`
    NearestPlayer,
    /// `@a`
    AllPlayers,
    /// `@r`
    RandomPlayer,
    /// `@s`
    Own,
    /// `@e`
    Entities,
    /// `@n`
    NearestEntity,
}

/// One `key=value` that may be negated with `!`.
pub type Negatable<T> = Vec<(bool, T)>;

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Filters {
    pub kinds: Negatable<String>,
    pub names: Negatable<String>,
    pub tags: Negatable<String>,
    pub teams: Negatable<String>,
    pub modes: Negatable<GameMode>,
    pub predicates: Negatable<String>,
    pub distance: Option<Range>,
    pub level: Option<Range>,
    pub x_rotation: Option<Range>,
    pub y_rotation: Option<Range>,
    pub scores: Vec<(String, Range)>,
    /// `nbt-lite`: `path=value` pairs over the data paths the game has.
    pub nbt: Vec<(String, String)>,
    pub origin: [Option<f64>; 3],
    pub volume: [Option<f64>; 3],
    pub limit: Option<usize>,
    pub sort: Option<Sort>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Selector {
    Name(String),
    Pick(Kind, Box<Filters>),
}

impl Selector {
    /// Parses a target word: a player name, or `@x` with `[...]`.
    pub fn parse(word: &str) -> Result<Selector, String> {
        let Some(rest) = word.strip_prefix('@') else {
            return Ok(Selector::Name(word.to_owned()));
        };
        let (letter, args) = match rest.split_once('[') {
            Some((letter, args)) => {
                let args = args
                    .strip_suffix(']')
                    .ok_or_else(|| "Expected end of options".to_owned())?;
                (letter, Some(args))
            }
            None => (rest, None),
        };
        let kind = match letter {
            "p" => Kind::NearestPlayer,
            "a" => Kind::AllPlayers,
            "r" => Kind::RandomPlayer,
            "s" => Kind::Own,
            "e" => Kind::Entities,
            "n" => Kind::NearestEntity,
            other => return Err(format!("Unknown selector type '@{other}'")),
        };
        let mut filters = Filters::default();
        for pair in args.map(split_top_level).unwrap_or_default() {
            if pair.trim().is_empty() {
                continue;
            }
            let (key, value) = pair
                .split_once('=')
                .ok_or_else(|| format!("Expected value for option '{}'", pair.trim()))?;
            filters.set(key.trim(), value.trim())?;
        }
        Ok(Selector::Pick(kind, Box::new(filters)))
    }
}

/// Splits `a=1,b={c:1,d:2},e=3` at the commas outside braces and brackets.
fn split_top_level(text: &str) -> Vec<&str> {
    let (mut parts, mut depth, mut start) = (Vec::new(), 0i32, 0);
    for (i, c) in text.char_indices() {
        match c {
            '{' | '[' => depth += 1,
            '}' | ']' => depth -= 1,
            ',' if depth == 0 => {
                parts.push(&text[start..i]);
                start = i + 1;
            }
            _ => {}
        }
    }
    parts.push(&text[start..]);
    parts
}

fn negated(value: &str) -> (bool, &str) {
    match value.strip_prefix('!') {
        Some(rest) => (true, rest),
        None => (false, value),
    }
}

fn bare(id: &str) -> &str {
    id.strip_prefix("minecraft:").unwrap_or(id)
}

impl Filters {
    fn set(&mut self, key: &str, value: &str) -> Result<(), String> {
        let number = |v: &str| {
            v.parse::<f64>()
                .map_err(|_| format!("Invalid number '{v}'"))
        };
        match key {
            "type" => {
                let (not, v) = negated(value);
                self.kinds
                    .push((not, bare(v.trim_start_matches('#')).to_owned()));
            }
            "name" => {
                let (not, v) = negated(value);
                self.names.push((not, v.trim_matches('"').to_owned()));
            }
            "tag" => {
                let (not, v) = negated(value);
                self.tags.push((not, v.to_owned()));
            }
            "team" => {
                let (not, v) = negated(value);
                self.teams.push((not, v.to_owned()));
            }
            "predicate" => {
                let (not, v) = negated(value);
                self.predicates.push((not, bare(v).to_owned()));
            }
            "gamemode" => {
                let (not, v) = negated(value);
                let mode = GameMode::parse(v)
                    .filter(|_| v.len() > 1 && !v.chars().all(|c| c.is_ascii_digit()))
                    .ok_or_else(|| format!("Invalid game mode '{v}'"))?;
                self.modes.push((not, mode));
            }
            "distance" => self.distance = Some(Range::parse(value)?),
            "level" => self.level = Some(Range::parse(value)?),
            "x_rotation" => self.x_rotation = Some(Range::parse(value)?),
            "y_rotation" => self.y_rotation = Some(Range::parse(value)?),
            "x" => self.origin[0] = Some(number(value)?),
            "y" => self.origin[1] = Some(number(value)?),
            "z" => self.origin[2] = Some(number(value)?),
            "dx" => self.volume[0] = Some(number(value)?),
            "dy" => self.volume[1] = Some(number(value)?),
            "dz" => self.volume[2] = Some(number(value)?),
            "limit" => {
                let n: usize = value
                    .parse()
                    .map_err(|_| format!("Invalid integer '{value}'"))?;
                if n < 1 {
                    return Err("Limit must be at least 1".to_owned());
                }
                self.limit = Some(n);
            }
            "sort" => {
                self.sort = Some(match value {
                    "nearest" => Sort::Nearest,
                    "furthest" => Sort::Furthest,
                    "random" => Sort::Random,
                    "arbitrary" => Sort::Arbitrary,
                    other => return Err(format!("Invalid or unknown sort type '{other}'")),
                })
            }
            "scores" => {
                let inner = value
                    .strip_prefix('{')
                    .and_then(|v| v.strip_suffix('}'))
                    .ok_or("Expected '{' for scores")?;
                for entry in split_top_level(inner) {
                    let (name, range) = entry
                        .split_once('=')
                        .ok_or_else(|| format!("Expected a score range in '{entry}'"))?;
                    self.scores
                        .push((name.trim().to_owned(), Range::parse(range.trim())?));
                }
            }
            "nbt" => {
                let inner = value
                    .strip_prefix('{')
                    .and_then(|v| v.strip_suffix('}'))
                    .ok_or("Expected '{' for nbt")?;
                for entry in split_top_level(inner) {
                    let (path, v) = entry
                        .split_once(':')
                        .ok_or_else(|| format!("Expected a value in '{entry}'"))?;
                    self.nbt.push((path.trim().to_owned(), v.trim().to_owned()));
                }
            }
            other => return Err(format!("Unknown option '{other}'")),
        }
        Ok(())
    }
}

/// What the filters look at, for one player or mob.
pub struct Candidate {
    pub target: Target,
    pub feet: DVec3,
    pub is_player: bool,
    /// Vanilla's yaw (0 south, 90 west) and pitch (positive down), degrees.
    pub yaw: f64,
    pub pitch: f64,
    pub kind: String,
    pub name: String,
    pub tags: Vec<String>,
    pub level: i32,
    pub mode: Option<GameMode>,
    pub health: f32,
    pub on_ground: bool,
}

/// Wraps degrees to −180..180, as vanilla's `wrapDegrees`.
pub fn wrap_degrees(degrees: f64) -> f64 {
    let d = degrees.rem_euclid(360.0);
    if d >= 180.0 { d - 360.0 } else { d }
}

fn in_range(range: &Option<Range>, value: f64) -> bool {
    range.as_ref().is_none_or(|r| r.contains(value))
}

impl Filters {
    /// Whether `c` passes everything but `limit` and `sort`. `from` is the
    /// selector's origin; `score` and `team` read the scoreboard.
    pub fn accepts(
        &self,
        c: &Candidate,
        from: DVec3,
        team: Option<&str>,
        score: &dyn Fn(&str) -> Option<i32>,
    ) -> bool {
        if let Some(range) = &self.distance
            && !range.contains(c.feet.distance(from))
        {
            return false;
        }
        if self.volume.iter().any(Option::is_some) {
            let d = |i: usize| self.volume[i].unwrap_or(0.0);
            let (lo, hi) = (
                DVec3::new(
                    from.x.min(from.x + d(0)),
                    from.y.min(from.y + d(1)),
                    from.z.min(from.z + d(2)),
                ),
                DVec3::new(
                    from.x.max(from.x + d(0)),
                    from.y.max(from.y + d(1)),
                    from.z.max(from.z + d(2)),
                ),
            );
            // An entity is in the volume if its (1×1 block) box touches it.
            let inside = c.feet.x >= lo.x - 0.3
                && c.feet.x <= hi.x + 1.3
                && c.feet.y >= lo.y - 1.8
                && c.feet.y <= hi.y + 1.0
                && c.feet.z >= lo.z - 0.3
                && c.feet.z <= hi.z + 1.3;
            if !inside {
                return false;
            }
        }
        if !self.kinds.is_empty() {
            let mut positive = false;
            let mut any_positive = false;
            for (not, kind) in &self.kinds {
                if *not {
                    if *kind == c.kind {
                        return false;
                    }
                } else {
                    any_positive = true;
                    positive |= *kind == c.kind;
                }
            }
            if any_positive && !positive {
                return false;
            }
        }
        if self.names.iter().any(|(not, n)| (*n == c.name) == *not) {
            return false;
        }
        for (not, tag) in &self.tags {
            // An empty tag means "has no tags"; `!` flips it.
            let has = if tag.is_empty() {
                !c.tags.is_empty()
            } else {
                c.tags.iter().any(|t| t == tag)
            };
            if has == *not {
                return false;
            }
        }
        for (not, wanted) in &self.teams {
            let is = if wanted.is_empty() {
                team.is_some()
            } else {
                team == Some(wanted.as_str())
            };
            if is == *not {
                return false;
            }
        }
        if !self.modes.is_empty() {
            let Some(mode) = c.mode else { return false };
            for (not, wanted) in &self.modes {
                if (mode == *wanted) == *not {
                    return false;
                }
            }
        }
        if self.level.is_some() && (!c.is_player || !in_range(&self.level, f64::from(c.level))) {
            return false;
        }
        if !in_range(&self.x_rotation, wrap_degrees(c.pitch))
            || !in_range(&self.y_rotation, wrap_degrees(c.yaw))
        {
            return false;
        }
        for (objective, range) in &self.scores {
            match score(objective) {
                Some(v) if range.contains(f64::from(v)) => {}
                _ => return false,
            }
        }
        for (path, wanted) in &self.nbt {
            if !nbt_matches(c, path, wanted) {
                return false;
            }
        }
        true
    }
}

/// The data paths `nbt={...}` understands (listed in DECISIONS.md):
/// `Health`, `OnGround`, `CustomName`, `Tags` and `Invulnerable`.
fn nbt_matches(c: &Candidate, path: &str, wanted: &str) -> bool {
    let wanted = wanted.trim_matches('"');
    let flag = |b: bool| {
        let w = wanted.trim_end_matches('b');
        (w == "1" && b) || (w == "0" && !b) || (w == "true" && b) || (w == "false" && !b)
    };
    match path {
        "Health" => wanted
            .trim_end_matches(['f', 'd'])
            .parse::<f32>()
            .is_ok_and(|w| (c.health - w).abs() < 0.01),
        "OnGround" => flag(c.on_ground),
        "CustomName" => c.name == wanted,
        "Invulnerable" => flag(c.mode.is_some_and(|m| !m.is_vulnerable())),
        "Tags" => c
            .tags
            .iter()
            .any(|t| t == wanted.trim_matches(['[', ']', '"'])),
        _ => false,
    }
}

impl Game {
    /// Everything a selector can pick in the caller's dimension: the
    /// players there and the living mobs.
    pub(super) fn candidates(
        &mut self,
        dimension: crate::world::dimension::Dimension,
    ) -> Vec<Candidate> {
        self.enter_realm(dimension);
        let mut found = Vec::new();
        for i in self.players_here() {
            let p = self.player_at(i);
            found.push(Candidate {
                target: Target::Player(i),
                feet: self.feet_of(i),
                is_player: true,
                yaw: wrap_degrees(180.0 + f64::from(p.camera.yaw).to_degrees()),
                pitch: -f64::from(p.camera.pitch).to_degrees(),
                kind: "player".to_owned(),
                name: p.name.clone(),
                tags: p.tags.clone(),
                level: p.stats.experience.level as i32,
                mode: Some(p.mode),
                health: p.stats.health,
                on_ground: p.body.on_ground,
            });
        }
        for m in self.mobs.iter().filter(|m| !m.is_dead()) {
            let def = self.mob_defs.get(m.kind);
            found.push(Candidate {
                target: Target::Mob(m.id),
                feet: m.feet(),
                is_player: false,
                yaw: wrap_degrees(180.0 + f64::from(m.yaw).to_degrees()),
                pitch: -f64::from(m.pitch).to_degrees(),
                kind: def.name.clone(),
                name: m
                    .data
                    .custom_name
                    .clone()
                    .unwrap_or_else(|| super::mob_acts::display_name(&def.name)),
                tags: m.data.tags.clone(),
                level: 0,
                mode: None,
                health: m.stats.health,
                on_ground: m.body.on_ground,
            });
        }
        found
    }

    /// The players and mobs `word` names, as the caller at `caller`
    /// (a player index) would resolve it, at `at` for `distance` and
    /// the `@p` kinds. Empty when nothing matches.
    pub(super) fn select(
        &mut self,
        executor: Target,
        dimension: crate::world::dimension::Dimension,
        at: DVec3,
        word: &str,
    ) -> Result<Vec<Target>, String> {
        let selector = Selector::parse(word)?;
        let mut pool = self.candidates(dimension);
        let (kind, filters) = match selector {
            Selector::Name(name) => {
                return Ok(pool
                    .into_iter()
                    .filter(|c| c.is_player && c.name == name)
                    .map(|c| c.target)
                    .collect());
            }
            Selector::Pick(kind, filters) => (kind, filters),
        };
        match kind {
            Kind::NearestPlayer | Kind::AllPlayers | Kind::RandomPlayer => {
                pool.retain(|c| c.is_player);
            }
            Kind::Own => pool.retain(|c| c.target == executor),
            Kind::Entities | Kind::NearestEntity => {}
        }
        let from = DVec3::new(
            filters.origin[0].unwrap_or(at.x),
            filters.origin[1].unwrap_or(at.y),
            filters.origin[2].unwrap_or(at.z),
        );
        let board = &self.board;
        pool.retain(|c| {
            let holder = match c.target {
                Target::Player(_) => c.name.clone(),
                Target::Mob(id) => format!("entity.{id}"),
            };
            let team = board.team_of(&holder).map(|(n, _)| n);
            filters.accepts(c, from, team, &|o| board.score(o, &holder))
        });
        // `predicate=` runs each candidate's named predicate as that entity.
        for (not, name) in &filters.predicates {
            let predicate = self
                .predicates
                .get(name)
                .cloned()
                .ok_or_else(|| format!("Unknown predicate: {name}"))?;
            let mut kept = Vec::new();
            for c in pool {
                if predicate.test(self, c.target, c.feet, dimension) != *not {
                    kept.push(c);
                }
            }
            pool = kept;
        }
        let sort = filters.sort.unwrap_or(match kind {
            Kind::NearestPlayer | Kind::NearestEntity => Sort::Nearest,
            Kind::RandomPlayer => Sort::Random,
            _ => Sort::Arbitrary,
        });
        match sort {
            Sort::Nearest => {
                pool.sort_by(|a, b| a.feet.distance(from).total_cmp(&b.feet.distance(from)))
            }
            Sort::Furthest => {
                pool.sort_by(|a, b| b.feet.distance(from).total_cmp(&a.feet.distance(from)))
            }
            Sort::Random => {
                for i in (1..pool.len()).rev() {
                    let j = self.rng.below(i as u32 + 1) as usize;
                    pool.swap(i, j);
                }
            }
            Sort::Arbitrary => {}
        }
        let limit = filters.limit.unwrap_or(match kind {
            Kind::NearestPlayer | Kind::RandomPlayer | Kind::NearestEntity => 1,
            _ => usize::MAX,
        });
        pool.truncate(limit);
        Ok(pool.into_iter().map(|c| c.target).collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pick(word: &str) -> (Kind, Box<Filters>) {
        match Selector::parse(word).unwrap() {
            Selector::Pick(k, f) => (k, f),
            other => panic!("not a pick: {other:?}"),
        }
    }

    #[test]
    fn a_name_is_a_name_and_at_signs_are_kinds() {
        assert_eq!(Selector::parse("Kenzo"), Ok(Selector::Name("Kenzo".into())));
        assert_eq!(pick("@a").0, Kind::AllPlayers);
        assert_eq!(pick("@n").0, Kind::NearestEntity);
        assert!(Selector::parse("@q").is_err());
    }

    #[test]
    fn ranges_parse_like_vanilla() {
        assert_eq!(Range::parse("5").unwrap().min, Some(5.0));
        let open = Range::parse("..5").unwrap();
        assert!(open.contains(-100.0) && open.contains(5.0) && !open.contains(5.1));
        let from = Range::parse("3..").unwrap();
        assert!(from.contains(1e9) && !from.contains(2.9));
        assert!(Range::parse("5..1").is_err());
        assert!(Range::parse("..").is_err());
        assert!(Range::parse("abc").is_err());
    }

    #[test]
    fn every_option_in_the_list_parses() {
        let (_, f) = pick(
            "@e[type=!zombie,name=Bob,tag=a,team=red,distance=..10,x=1,y=2,z=3,dx=4,dy=5,dz=6,\
             limit=2,sort=furthest,level=5..,gamemode=!creative,scores={kills=1..,deaths=..2},\
             x_rotation=-90..0,y_rotation=0..90,nbt={Health:20f},predicate=foo:bar]",
        );
        assert_eq!(f.kinds, vec![(true, "zombie".to_owned())]);
        assert_eq!(f.origin, [Some(1.0), Some(2.0), Some(3.0)]);
        assert_eq!(f.volume, [Some(4.0), Some(5.0), Some(6.0)]);
        assert_eq!(f.limit, Some(2));
        assert_eq!(f.sort, Some(Sort::Furthest));
        assert_eq!(f.modes, vec![(true, GameMode::Creative)]);
        assert_eq!(f.scores.len(), 2);
        assert_eq!(f.nbt, vec![("Health".to_owned(), "20f".to_owned())]);
        assert_eq!(f.predicates, vec![(false, "foo:bar".to_owned())]);
    }

    #[test]
    fn bad_options_give_vanilla_style_errors() {
        assert_eq!(
            Selector::parse("@e[bogus=1]"),
            Err("Unknown option 'bogus'".to_owned())
        );
        assert!(Selector::parse("@e[limit=0]").is_err());
        assert!(Selector::parse("@e[sort=sideways]").is_err());
        assert!(Selector::parse("@e[type=pig").is_err());
    }

    fn candidate(kind: &str, feet: DVec3) -> Candidate {
        Candidate {
            target: Target::Mob(1),
            feet,
            is_player: false,
            yaw: 0.0,
            pitch: 0.0,
            kind: kind.to_owned(),
            name: "Pig".to_owned(),
            tags: vec!["farm".to_owned()],
            level: 0,
            mode: None,
            health: 10.0,
            on_ground: true,
        }
    }

    #[test]
    fn filters_check_type_distance_tags_and_data() {
        let near = candidate("pig", DVec3::new(3.0, 0.0, 0.0));
        let none = |_: &str| None;
        let ok = |sel: &str| pick(sel).1.accepts(&near, DVec3::ZERO, None, &none);
        assert!(ok("@e[type=pig]"));
        assert!(!ok("@e[type=!pig]"));
        assert!(!ok("@e[type=cow]"));
        assert!(ok("@e[distance=..5]"));
        assert!(!ok("@e[distance=4..]"));
        assert!(ok("@e[tag=farm]") && !ok("@e[tag=!farm]") && !ok("@e[tag=other]"));
        assert!(ok("@e[nbt={Health:10f}]") && !ok("@e[nbt={Health:5f}]"));
        assert!(ok("@e[dx=5]") && !ok("@e[dx=-1]"));
        assert!(!ok("@e[level=1..]"));
    }

    #[test]
    fn scores_come_from_the_scoreboard() {
        let c = candidate("pig", DVec3::ZERO);
        let f = pick("@e[scores={kills=2..}]").1;
        assert!(f.accepts(&c, DVec3::ZERO, None, &|_| Some(3)));
        assert!(!f.accepts(&c, DVec3::ZERO, None, &|_| Some(1)));
        assert!(!f.accepts(&c, DVec3::ZERO, None, &|_| None));
    }

    #[test]
    fn degrees_wrap_to_half_turns() {
        assert_eq!(wrap_degrees(190.0), -170.0);
        assert_eq!(wrap_degrees(-190.0), 170.0);
        assert_eq!(wrap_degrees(180.0), -180.0);
    }
}
