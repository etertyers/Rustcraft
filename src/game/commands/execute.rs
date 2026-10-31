//! `/execute` (M44) with every subcommand: `as at positioned rotated facing
//! align anchored in on summon if unless store run`. A chain walks left to
//! right; `as` and `at` fork it once per target. Every `run` is counted
//! against `maxCommandChainLength`.

use super::{
    board_cmds::{holders, objective_names},
    data_cmds::{self, DataTarget},
    text_cmds,
    world::block_state_named,
    *,
};
use crate::world::block::BlockState;

/// What a running chain has: who, where, which way and in what dimension.
#[derive(Clone)]
struct Ctx {
    executor: Target,
    pos: DVec3,
    look: (f32, f32),
    dim: Dimension,
    eyes: bool,
    stores: Vec<(bool, Store)>,
}

#[derive(Clone)]
enum Store {
    Score {
        holders: Vec<String>,
        objective: String,
    },
    Bar {
        id: String,
        what: String,
    },
    Data {
        target: DataTarget,
        path: String,
        ty: String,
        scale: f64,
    },
}

struct Run {
    replies: Vec<Reply>,
    forks: usize,
    runs: usize,
    ran_ok: usize,
    /// What the last condition counted, for a chain with no `run`.
    terminal: Option<i32>,
    ended: bool,
}

fn strs(o: &[&str]) -> Vec<String> {
    o.iter().map(|s| (*s).to_owned()).collect()
}

const ANCHOR_EYES: f64 = 1.62;
const SUBCOMMANDS: &[&str] = &[
    "as",
    "at",
    "positioned",
    "rotated",
    "facing",
    "align",
    "anchored",
    "in",
    "on",
    "summon",
    "if",
    "unless",
    "store",
    "run",
];

fn dimension_named(name: &str) -> Result<Dimension, String> {
    match name.strip_prefix("minecraft:").unwrap_or(name) {
        "overworld" => Ok(Dimension::Overworld),
        "the_nether" => Ok(Dimension::Nether),
        "the_end" => Ok(Dimension::End),
        other => Err(format!("Unknown dimension '{other}'")),
    }
}

/// The names `/execute in` takes.
const DIMENSIONS: &[&str] = &["overworld", "the_nether", "the_end"];

fn eye_pos(ctx: &Ctx) -> DVec3 {
    ctx.pos + DVec3::Y * if ctx.eyes { ANCHOR_EYES } else { 0.0 }
}

impl Ctx {
    fn set_look(&self) {
        super::LOOK.set((
            self.look.0,
            self.look.1,
            if self.eyes { ANCHOR_EYES } else { 0.0 },
        ));
    }

    fn pos_from(&self, words: &[&str]) -> Result<DVec3, String> {
        self.set_look();
        position(words, self.pos)
    }

    fn cell_from(&self, words: &[&str]) -> Result<IVec3, String> {
        Ok(self.pos_from(words)?.floor().as_ivec3())
    }
}

fn pick(game: &mut Game, ctx: &Ctx, word: &str) -> Result<Vec<Target>, String> {
    game.select(ctx.executor, ctx.dim, ctx.pos, word)
}

/// A target's position, look and dimension as a context.
fn as_context(game: &mut Game, ctx: &Ctx, target: Target) -> Ctx {
    let mut next = ctx.clone();
    next.executor = target;
    let dim = match target {
        Target::Player(i) => game.player_at(i).dimension,
        Target::Mob(_) => ctx.dim,
    };
    let found = game
        .candidates(dim)
        .into_iter()
        .find(|c| c.target == target);
    if let Some(c) = found {
        next.pos = c.feet;
        next.look = (
            wrap_degrees(c.yaw - 180.0).to_radians() as f32,
            (-c.pitch).to_radians() as f32,
        );
        next.dim = dim;
    }
    next
}

/// `~`-relative angle words: vanilla yaw and pitch in degrees.
fn angle(word: &str, base: f64) -> Result<f64, String> {
    match word.strip_prefix('~') {
        Some("") => Ok(base),
        Some(by) => Ok(base + number::<f64>(by, "rotation")?),
        None => number(word, "rotation"),
    }
}

fn face_towards(ctx: &mut Ctx, at: DVec3) {
    let from = eye_pos(ctx);
    let d = at - from;
    let yaw = wrap_degrees(-(d.x.atan2(d.z)).to_degrees());
    let pitch = -(d.y.atan2(d.x.hypot(d.z))).to_degrees();
    ctx.look = (
        wrap_degrees(yaw - 180.0).to_radians() as f32,
        (-pitch).to_radians() as f32,
    );
}

/// The top of the highest block of a heightmap kind at a column.
fn height_at(game: &Game, x: i32, z: i32, kind: &str) -> Result<f64, String> {
    let world = game.chunks.world();
    let top = crate::config::MAX_Y;
    for y in (crate::config::MIN_Y..=top).rev() {
        let Some(b) = world.block(x, y, z) else {
            return Err("That position is not loaded".to_owned());
        };
        if b.is_air() {
            continue;
        }
        let name = b.def().name.as_str();
        let fluid = name == "water" || name == "lava";
        let hit = match kind {
            "world_surface" => true,
            "motion_blocking" => b.def().solid || fluid,
            "motion_blocking_no_leaves" => (b.def().solid || fluid) && !name.ends_with("_leaves"),
            "ocean_floor" => b.def().solid && !fluid,
            other => return Err(format!("Unknown heightmap '{other}'")),
        };
        if hit {
            return Ok(f64::from(y) + 1.0);
        }
    }
    Ok(f64::from(crate::config::MIN_Y))
}

/// Block tags the game can test (by what the block's name ends with).
fn tag_matches(tag: &str, name: &str) -> bool {
    let tag = tag.strip_prefix("minecraft:").unwrap_or(tag);
    let ends = |suffixes: &[&str]| suffixes.iter().any(|s| name.ends_with(s));
    match tag {
        "logs" | "logs_that_burn" => ends(&["_log", "_wood", "_stem", "_hyphae"]),
        "planks" => ends(&["_planks"]),
        "leaves" => ends(&["_leaves"]),
        "wool" => ends(&["_wool"]),
        "slabs" => ends(&["_slab"]),
        "stairs" => ends(&["_stairs"]),
        "doors" => ends(&["_door"]),
        "fences" => ends(&["_fence"]),
        "walls" => ends(&["_wall"]),
        "buttons" => ends(&["_button"]),
        "beds" => ends(&["_bed"]),
        "saplings" => ends(&["_sapling"]),
        "signs" => ends(&["_sign"]),
        "terracotta" => ends(&["terracotta"]),
        "ores" => ends(&["_ore"]),
        "base_stone_overworld" => matches!(
            name,
            "stone" | "granite" | "diorite" | "andesite" | "deepslate" | "tuff"
        ),
        "air" => name == "air",
        _ => false,
    }
}

/// Whether `state` is what `spec` asks for: `stone`, `oak_stairs[facing=north]`
/// (only the properties given must match) or `#logs`.
fn block_matches(state: BlockState, spec: &str) -> Result<bool, String> {
    if let Some(tag) = spec.strip_prefix('#') {
        return Ok(tag_matches(tag, &state.def().name));
    }
    let wanted = block_state_named(spec)?;
    if wanted.id() != state.id() {
        return Ok(false);
    }
    let Some((_, props)) = spec.split_once('[') else {
        return Ok(true);
    };
    let layout = crate::world::block::registry().layout(state.id());
    for pair in props
        .trim_end_matches(']')
        .split(',')
        .filter(|p| !p.is_empty())
    {
        let (key, _) = pair.split_once('=').unwrap_or((pair, ""));
        if let Some(prop) = layout
            .props
            .iter()
            .copied()
            .find(|p| p.name() == key.trim())
            && state.get(prop) != wanted.get(prop)
        {
            return Ok(false);
        }
    }
    Ok(true)
}

/// A condition: how many it counted (0 fails) and the words it used.
fn condition(game: &mut Game, ctx: &Ctx, args: &[&str]) -> Result<(i32, usize), String> {
    let missing = || "Incomplete condition".to_owned();
    let kind = args.first().copied().ok_or_else(missing)?;
    let rest = &args[1..];
    game.enter_realm(ctx.dim);
    match kind {
        "block" => {
            let at = ctx.cell_from(rest.get(0..3).ok_or_else(missing)?)?;
            let spec = rest.get(3).ok_or_else(missing)?;
            let state = game
                .chunks
                .world()
                .block(at.x, at.y, at.z)
                .ok_or("That position is not loaded")?;
            Ok((i32::from(block_matches(state, spec)?), 5))
        }
        "blocks" => {
            let a = ctx.cell_from(rest.get(0..3).ok_or_else(missing)?)?;
            let b = ctx.cell_from(rest.get(3..6).ok_or_else(missing)?)?;
            let dest = ctx.cell_from(rest.get(6..9).ok_or_else(missing)?)?;
            let masked = match rest.get(9).copied() {
                Some("all") => false,
                Some("masked") => true,
                _ => return Err(missing()),
            };
            let (lo, hi) = (a.min(b), a.max(b));
            let size = hi - lo + 1;
            let volume = i64::from(size.x) * i64::from(size.y) * i64::from(size.z);
            if volume > FILL_LIMIT {
                return Err(format!(
                    "Too many blocks in the specified area (maximum {FILL_LIMIT}, specified {volume})"
                ));
            }
            for y in 0..size.y {
                for z in 0..size.z {
                    for x in 0..size.x {
                        let off = IVec3::new(x, y, z);
                        let (s, d) = (lo + off, dest + off);
                        let src = game
                            .chunks
                            .world()
                            .block(s.x, s.y, s.z)
                            .ok_or("That position is not loaded")?;
                        let dst = game
                            .chunks
                            .world()
                            .block(d.x, d.y, d.z)
                            .ok_or("That position is not loaded")?;
                        if masked && src.is_air() {
                            continue;
                        }
                        if src != dst {
                            return Ok((0, 10 + 1));
                        }
                    }
                }
            }
            Ok((volume as i32, 11))
        }
        "entity" => {
            let word = rest.first().ok_or_else(missing)?;
            Ok((pick(game, ctx, word)?.len() as i32, 2))
        }
        "score" => {
            let (t, to) = (
                rest.first().ok_or_else(missing)?,
                rest.get(1).ok_or_else(missing)?,
            );
            let caller = ctx_caller(game, ctx);
            let target = one(holders(game, &caller, t)?)?;
            let Some(a) = game.board.score(to, &target) else {
                return Ok((0, 1 + score_words(rest)));
            };
            match rest.get(2).copied().ok_or_else(missing)? {
                "matches" => {
                    let r = Range::parse(rest.get(3).ok_or_else(missing)?)?;
                    Ok((i32::from(r.contains(f64::from(a))), 5))
                }
                op @ ("<" | "<=" | "=" | ">" | ">=") => {
                    let (s, so) = (
                        rest.get(3).ok_or_else(missing)?,
                        rest.get(4).ok_or_else(missing)?,
                    );
                    let source = one(holders(game, &caller, s)?)?;
                    let Some(b) = game.board.score(so, &source) else {
                        return Ok((0, 6));
                    };
                    let ok = match op {
                        "<" => a < b,
                        "<=" => a <= b,
                        "=" => a == b,
                        ">" => a > b,
                        _ => a >= b,
                    };
                    Ok((i32::from(ok), 6))
                }
                other => Err(format!("Unknown comparison '{other}'")),
            }
        }
        "data" => {
            let caller = ctx_caller(game, ctx);
            let (target, used) = data_cmds::parse_target(game, &caller, rest)?;
            let path = rest.get(used).ok_or_else(missing)?;
            let found = data_cmds::values(game, &target, Some(path)).unwrap_or_default();
            Ok((found.len() as i32, 2 + used + 1 - 1))
        }
        "predicate" => {
            let spec = rest.first().ok_or_else(missing)?;
            // An inline predicate runs to the end of its braces, which the
            // tokenizer may have split at spaces; take the words that close it.
            let (text, used) = if spec.starts_with('{') {
                let mut depth = 0i32;
                let mut text = String::new();
                let mut used = 0;
                for word in rest {
                    if used > 0 {
                        text.push(' ');
                    }
                    text.push_str(word);
                    used += 1;
                    depth += word.matches('{').count() as i32 - word.matches('}').count() as i32;
                    if depth <= 0 {
                        break;
                    }
                }
                (text, used)
            } else {
                ((*spec).to_owned(), 1)
            };
            let predicate = if text.starts_with('{') {
                crate::game::predicate::Predicate::parse(&text)?
            } else {
                let id = text.strip_prefix("minecraft:").unwrap_or(&text);
                game.predicates
                    .get(id)
                    .or_else(|| game.predicates.get(&text))
                    .cloned()
                    .ok_or_else(|| format!("Unknown predicate: {text}"))?
            };
            let ok = predicate.test(game, ctx.executor, ctx.pos, ctx.dim);
            Ok((i32::from(ok), 1 + used))
        }
        "biome" => {
            let at = ctx.cell_from(rest.get(0..3).ok_or_else(missing)?)?;
            let want = rest.get(3).ok_or_else(missing)?;
            let here = game
                .chunks
                .world()
                .biome_at(at.x, at.z)
                .ok_or("That position is not loaded")?;
            let id = biome_id(here);
            let want = want.strip_prefix("minecraft:").unwrap_or(want);
            Ok((i32::from(id == want), 5))
        }
        "dimension" => {
            let want = dimension_named(rest.first().ok_or_else(missing)?)?;
            Ok((i32::from(ctx.dim == want), 2))
        }
        "loaded" => {
            let at = ctx.cell_from(rest.get(0..3).ok_or_else(missing)?)?;
            Ok((
                i32::from(game.chunks.world().block(at.x, at.y, at.z).is_some()),
                4,
            ))
        }
        "items" => {
            let caller = ctx_caller(game, ctx);
            let (target, used) = data_cmds::parse_target(game, &caller, rest)?;
            let slots = rest.get(used).ok_or_else(missing)?;
            let wanted = rest.get(used + 1).ok_or_else(missing)?;
            let count = data_cmds::count_items(game, &target, slots, wanted)?;
            Ok((count, 1 + used + 2))
        }
        "function" => {
            let id = rest.first().ok_or_else(missing)?;
            let caller = ctx_caller(game, ctx);
            let result = game.call_function(&caller, id)?;
            Ok((result, 2))
        }
        other => Err(format!("Unknown condition '{other}'")),
    }
}

/// How many words a `score` condition after its target and objective used.
fn score_words(rest: &[&str]) -> usize {
    match rest.get(2).copied() {
        Some("matches") => 4,
        _ => 5,
    }
}

fn one(mut v: Vec<String>) -> Result<String, String> {
    if v.len() == 1 {
        Ok(v.remove(0))
    } else {
        Err("Only one entity is allowed, but the provided selector allows more than one".to_owned())
    }
}

/// A `Caller` for helpers that read the chain's position and executor.
fn ctx_caller(game: &Game, ctx: &Ctx) -> Caller {
    let _ = game;
    Caller {
        index: 0,
        cheats: true,
        host: true,
        executor: ctx.executor,
        pos: ctx.pos,
        look: ctx.look,
        dim: ctx.dim,
        eyes: ctx.eyes,
        result: std::cell::Cell::new(None),
    }
}

/// How many words a store target takes after `store result|success`.
fn parse_store(game: &mut Game, ctx: &Ctx, args: &[&str]) -> Result<(bool, Store, usize), String> {
    let missing = || "Incomplete store".to_owned();
    let result = match args.first().copied().ok_or_else(missing)? {
        "result" => true,
        "success" => false,
        other => return Err(format!("Unknown store type '{other}'")),
    };
    let caller = ctx_caller(game, ctx);
    match args.get(1).copied().ok_or_else(missing)? {
        "score" => {
            let targets = holders(game, &caller, args.get(2).ok_or_else(missing)?)?;
            let objective = args.get(3).ok_or_else(missing)?;
            if !game.board.objectives.contains_key(*objective) {
                return Err(format!("Unknown scoreboard objective '{objective}'"));
            }
            Ok((
                result,
                Store::Score {
                    holders: targets,
                    objective: (*objective).to_owned(),
                },
                4,
            ))
        }
        "bossbar" => {
            let id = args.get(2).ok_or_else(missing)?;
            let what = args.get(3).copied().ok_or_else(missing)?;
            if !matches!(what, "value" | "max") {
                return Err(format!("Unknown bossbar field '{what}'"));
            }
            if !game
                .bossbars
                .contains_key(id.strip_prefix("minecraft:").unwrap_or(id))
            {
                return Err(format!("No bossbar exists with the ID '{id}'"));
            }
            Ok((
                result,
                Store::Bar {
                    id: (*id).to_owned(),
                    what: what.to_owned(),
                },
                4,
            ))
        }
        "entity" | "block" | "storage" => {
            let (target, used) = data_cmds::parse_target(game, &caller, &args[1..])?;
            let path = args.get(1 + used).ok_or_else(missing)?;
            let ty = args.get(2 + used).ok_or_else(missing)?;
            let scale: f64 = number(args.get(3 + used).ok_or_else(missing)?, "scale")?;
            Ok((
                result,
                Store::Data {
                    target,
                    path: (*path).to_owned(),
                    ty: (*ty).to_owned(),
                    scale,
                },
                4 + used,
            ))
        }
        other => Err(format!("Unknown store target '{other}'")),
    }
}

fn apply_store(game: &mut Game, store: &Store, value: i32) -> Result<(), String> {
    match store {
        Store::Score { holders, objective } => {
            for h in holders {
                game.board.set(objective, h, value);
            }
            Ok(())
        }
        Store::Bar { id, what } => text_cmds::set_bar_number(game, id, what, value),
        Store::Data {
            target,
            path,
            ty,
            scale,
        } => data_cmds::store_number(game, target, path, ty, *scale, value),
    }
}

fn exec(
    game: &mut Game,
    base: &Caller,
    args: &[&str],
    ctx: Ctx,
    st: &mut Run,
) -> Result<(), String> {
    if st.ended {
        return Ok(());
    }
    let Some(&sub) = args.first() else {
        // A chain that ends without `run`: a condition's count is its answer.
        if st.terminal.is_none() {
            st.terminal = Some(1);
        }
        st.ran_ok += 1;
        return Ok(());
    };
    let rest = &args[1..];
    let missing = || format!("Incomplete /execute: {sub} needs more");
    let limit = game.rules.max_command_chain_length as usize;
    st.forks += 1;
    if st.forks > limit.max(1) {
        st.ended = true;
        return Err(format!("The command chain is too long (limit {limit})"));
    }
    match sub {
        "as" => {
            let word = rest.first().ok_or_else(missing)?;
            for target in pick(game, &ctx, word)? {
                let mut next = ctx.clone();
                next.executor = target;
                exec(game, base, &rest[1..], next, st)?;
            }
            Ok(())
        }
        "at" => {
            let word = rest.first().ok_or_else(missing)?;
            for target in pick(game, &ctx, word)? {
                let there = as_context(game, &ctx, target);
                let mut next = ctx.clone();
                (next.pos, next.look, next.dim) = (there.pos, there.look, there.dim);
                exec(game, base, &rest[1..], next, st)?;
            }
            Ok(())
        }
        "positioned" => match rest.first().copied().ok_or_else(missing)? {
            "as" => {
                let word = rest.get(1).ok_or_else(missing)?;
                for target in pick(game, &ctx, word)? {
                    let there = as_context(game, &ctx, target);
                    let mut next = ctx.clone();
                    next.pos = there.pos;
                    exec(game, base, &rest[2..], next, st)?;
                }
                Ok(())
            }
            "over" => {
                let kind = rest.get(1).ok_or_else(missing)?;
                game.enter_realm(ctx.dim);
                let y = height_at(
                    game,
                    ctx.pos.x.floor() as i32,
                    ctx.pos.z.floor() as i32,
                    kind,
                )?;
                let mut next = ctx.clone();
                next.pos.y = y;
                exec(game, base, &rest[2..], next, st)
            }
            _ => {
                let words = rest.get(0..3).ok_or_else(missing)?;
                let mut next = ctx.clone();
                next.pos = ctx.pos_from(words)?;
                exec(game, base, &rest[3..], next, st)
            }
        },
        "rotated" => {
            if rest.first() == Some(&"as") {
                let word = rest.get(1).ok_or_else(missing)?;
                for target in pick(game, &ctx, word)? {
                    let there = as_context(game, &ctx, target);
                    let mut next = ctx.clone();
                    next.look = there.look;
                    exec(game, base, &rest[2..], next, st)?;
                }
                return Ok(());
            }
            let (y, p) = (
                rest.first().ok_or_else(missing)?,
                rest.get(1).ok_or_else(missing)?,
            );
            let (yaw0, pitch0) = (
                wrap_degrees(f64::from(ctx.look.0).to_degrees() + 180.0),
                -f64::from(ctx.look.1).to_degrees(),
            );
            let (yaw, pitch) = (angle(y, yaw0)?, angle(p, pitch0)?.clamp(-90.0, 90.0));
            let mut next = ctx.clone();
            next.look = (
                wrap_degrees(yaw - 180.0).to_radians() as f32,
                (-pitch).to_radians() as f32,
            );
            exec(game, base, &rest[2..], next, st)
        }
        "facing" => {
            if rest.first() == Some(&"entity") {
                let word = rest.get(1).ok_or_else(missing)?;
                let anchor = rest.get(2).copied().ok_or_else(missing)?;
                for target in pick(game, &ctx, word)? {
                    let there = as_context(game, &ctx, target);
                    let mut next = ctx.clone();
                    let at =
                        there.pos + DVec3::Y * if anchor == "eyes" { ANCHOR_EYES } else { 0.0 };
                    face_towards(&mut next, at);
                    exec(game, base, &rest[3..], next, st)?;
                }
                return Ok(());
            }
            let words = rest.get(0..3).ok_or_else(missing)?;
            let mut next = ctx.clone();
            face_towards(&mut next, ctx.pos_from(words)?);
            exec(game, base, &rest[3..], next, st)
        }
        "align" => {
            let axes = rest.first().ok_or_else(missing)?;
            let mut next = ctx.clone();
            for c in axes.chars() {
                match c {
                    'x' => next.pos.x = next.pos.x.floor(),
                    'y' => next.pos.y = next.pos.y.floor(),
                    'z' => next.pos.z = next.pos.z.floor(),
                    _ => {
                        return Err(format!(
                            "Invalid swizzle '{axes}', expected a combination of x, y and z"
                        ));
                    }
                }
            }
            exec(game, base, &rest[1..], next, st)
        }
        "anchored" => {
            let mut next = ctx.clone();
            next.eyes = match rest.first().copied().ok_or_else(missing)? {
                "eyes" => true,
                "feet" => false,
                other => return Err(format!("Unknown anchor '{other}'")),
            };
            exec(game, base, &rest[1..], next, st)
        }
        "in" => {
            let mut next = ctx.clone();
            next.dim = dimension_named(rest.first().ok_or_else(missing)?)?;
            exec(game, base, &rest[1..], next, st)
        }
        "on" => {
            let relation = rest.first().copied().ok_or_else(missing)?;
            let related = relation_targets(game, &ctx, relation)?;
            for target in related {
                let mut next = ctx.clone();
                next.executor = target;
                exec(game, base, &rest[1..], next, st)?;
            }
            Ok(())
        }
        "summon" => {
            let name = rest.first().ok_or_else(missing)?;
            let name = name.strip_prefix("minecraft:").unwrap_or(name);
            game.enter_realm(ctx.dim);
            let yaw = ctx.look.0;
            let id = game
                .summon(name, ctx.pos, yaw)
                .ok_or_else(|| format!("Unknown entity type: {name}"))?
                .id;
            let mut next = ctx.clone();
            next.executor = Target::Mob(id);
            exec(game, base, &rest[1..], next, st)
        }
        "if" | "unless" => {
            let (count, used) = condition(game, &ctx, rest)?;
            let passes = (count > 0) == (sub == "if");
            let terminal = rest.len() <= used;
            if terminal {
                // A test that ends the chain stores its count (or success).
                let value = if passes {
                    if sub == "if" { count } else { 1 }
                } else {
                    0
                };
                for (wants_result, store) in &ctx.stores {
                    let v = if *wants_result {
                        value
                    } else {
                        i32::from(passes)
                    };
                    apply_store(game, store, v)?;
                }
                if passes {
                    st.terminal = Some(value);
                    st.ran_ok += 1;
                }
                return Ok(());
            }
            if !passes {
                return Ok(());
            }
            exec(game, base, &rest[used..], ctx, st)
        }
        "store" => {
            let (result, store, used) = parse_store(game, &ctx, rest)?;
            let mut next = ctx.clone();
            next.stores.push((result, store));
            exec(game, base, rest.get(used..).unwrap_or(&[]), next, st)
        }
        "run" => {
            let line = rest.join(" ");
            if line.is_empty() {
                return Err(missing());
            }
            st.runs += 1;
            if st.runs > limit.max(1) {
                st.ended = true;
                return Err(format!("The command chain is too long (limit {limit})"));
            }
            let caller = Caller {
                index: base.index,
                cheats: base.cheats,
                host: base.host,
                executor: ctx.executor,
                pos: ctx.pos,
                look: ctx.look,
                dim: ctx.dim,
                eyes: ctx.eyes,
                result: std::cell::Cell::new(None),
            };
            game.enter_realm(ctx.dim);
            let (replies, result) = game.run_as(&caller, &line);
            st.replies.extend(replies);
            if result.is_some() {
                st.ran_ok += 1;
            }
            for (wants_result, store) in &ctx.stores {
                let value = if *wants_result {
                    result.unwrap_or(0)
                } else {
                    i32::from(result.is_some())
                };
                apply_store(game, store, value)?;
            }
            Ok(())
        }
        other => Err(format!("Unknown subcommand '{other}'")),
    }
}

/// The targets one step away: `vehicle`, `passengers`, `owner`...
fn relation_targets(game: &mut Game, ctx: &Ctx, relation: &str) -> Result<Vec<Target>, String> {
    match relation {
        "vehicle" => Ok(match ctx.executor {
            Target::Player(i) => game
                .player_at(i)
                .mounted
                .map(Target::Mob)
                .into_iter()
                .collect(),
            Target::Mob(_) => Vec::new(),
        }),
        "passengers" | "controller" => Ok(match ctx.executor {
            Target::Mob(id) => (0..game.player_count())
                .filter(|&i| game.player_at(i).mounted == Some(id))
                .map(Target::Player)
                .collect(),
            Target::Player(_) => Vec::new(),
        }),
        "owner" => {
            // The game doesn't record who tamed a mob: a tame mob's owner is
            // the nearest player.
            let Target::Mob(id) = ctx.executor else {
                return Ok(Vec::new());
            };
            let tame = game
                .mobs
                .iter()
                .find(|m| m.id == id)
                .is_some_and(|m| m.data.tamed);
            if !tame {
                return Ok(Vec::new());
            }
            Ok((0..game.player_count())
                .map(|i| (i, game.feet_of(i).distance(ctx.pos)))
                .min_by(|a, b| a.1.total_cmp(&b.1))
                .map(|(i, _)| Target::Player(i))
                .into_iter()
                .collect())
        }
        "attacker" | "leasher" | "origin" | "target" => Ok(Vec::new()),
        other => Err(format!("Unknown relation '{other}'")),
    }
}

pub(super) fn execute(
    game: &mut Game,
    caller: &Caller,
    args: &[&str],
) -> Result<Vec<Reply>, String> {
    if args.is_empty() {
        return Err("Usage: /execute <subcommand> ...".to_owned());
    }
    let ctx = Ctx {
        executor: caller.executor,
        pos: caller.pos,
        look: caller.look,
        dim: caller.dim,
        eyes: caller.eyes,
        stores: Vec::new(),
    };
    let has_run = args.contains(&"run");
    let mut st = Run {
        replies: Vec::new(),
        forks: 0,
        runs: 0,
        ran_ok: 0,
        terminal: None,
        ended: false,
    };
    exec(game, caller, args, ctx, &mut st)?;
    if has_run {
        if st.ran_ok == 0 {
            // A command that ran and failed has said why; a chain that never
            // got to run says nothing, as vanilla's.
            return if st.replies.is_empty() {
                Err(String::new())
            } else {
                Ok(st.replies)
            };
        }
        caller.report(st.ran_ok as i32);
        return Ok(st.replies);
    }
    // No `run`: a plain test.
    match st.terminal {
        Some(count) if st.ran_ok > 0 => {
            caller.report(count);
            Ok(tell(if count > 1 {
                format!("Test passed, count: {count}")
            } else {
                "Test passed".to_owned()
            }))
        }
        _ => Err("Test failed".to_owned()),
    }
}

// ---- Tab completion ----

/// What the next word of `/execute ...` can be, given the words before it.
pub(super) fn execute_tab(game: &Game, before: &[&str]) -> Vec<String> {
    let subs = || strs(SUBCOMMANDS);
    let pos = |at: usize, n: usize| {
        // Coordinates take ~ and ^ for each of their words.
        let _ = (at, n);
        strs(&["~", "^"])
    };
    let mut i = 0;
    loop {
        let rest = &before[i..];
        let Some(&sub) = rest.first() else {
            return subs();
        };
        // The words this step takes after its keyword, and what the one
        // being typed could be if it's within them.
        let (arity, options): (usize, Box<dyn Fn(usize) -> Vec<String>>) = match sub {
            "as" | "at" => (1, Box::new(|_| game.options(Arg::Entities))),
            "positioned" => match rest.get(1).copied() {
                Some("as") => (2, Box::new(|_| game.options(Arg::Entities))),
                Some("over") => (
                    2,
                    Box::new(|_| {
                        strs(&[
                            "world_surface",
                            "motion_blocking",
                            "motion_blocking_no_leaves",
                            "ocean_floor",
                        ])
                    }),
                ),
                None => (
                    3,
                    Box::new(|_| {
                        let mut o = strs(&["as", "over", "~", "^"]);
                        o.sort();
                        o
                    }),
                ),
                Some(_) => (3, Box::new(move |n| pos(n, 3))),
            },
            "rotated" => match rest.get(1).copied() {
                Some("as") => (2, Box::new(|_| game.options(Arg::Entities))),
                None => (2, Box::new(|_| strs(&["as", "~"]))),
                Some(_) => (2, Box::new(|_| strs(&["~"]))),
            },
            "facing" => match rest.get(1).copied() {
                Some("entity") => (
                    3,
                    Box::new(|n| {
                        if n == 2 {
                            game.options(Arg::Entities)
                        } else {
                            strs(&["eyes", "feet"])
                        }
                    }),
                ),
                None => (3, Box::new(|_| strs(&["entity", "~", "^"]))),
                Some(_) => (3, Box::new(move |n| pos(n, 3))),
            },
            "align" => (
                1,
                Box::new(|_| strs(&["x", "y", "z", "xy", "xz", "yz", "xyz"])),
            ),
            "anchored" => (1, Box::new(|_| strs(&["eyes", "feet"]))),
            "in" => (1, Box::new(|_| strs(DIMENSIONS))),
            "on" => (
                1,
                Box::new(|_| {
                    strs(&[
                        "attacker",
                        "controller",
                        "leasher",
                        "origin",
                        "owner",
                        "passengers",
                        "target",
                        "vehicle",
                    ])
                }),
            ),
            "summon" => (1, Box::new(|_| game.options(Arg::Mob))),
            "if" | "unless" => {
                let (arity, f) = condition_tab(game, &rest[1..]);
                (arity, f)
            }
            "store" => store_tab(game, &rest[1..]),
            "run" => {
                let line = format!("{} ", rest[1..].join(" "));
                return game.complete_command(&line, true, true);
            }
            _ => return Vec::new(),
        };
        // Words typed after the keyword: if all of this step's are there the
        // next word starts another step; otherwise it's inside this one.
        let typed = rest.len() - 1;
        if typed < arity {
            return options(typed);
        }
        i += 1 + arity;
        if i > before.len() {
            return Vec::new();
        }
    }
}

type Options<'a> = Box<dyn Fn(usize) -> Vec<String> + 'a>;

fn condition_tab<'a>(game: &'a Game, rest: &[&str]) -> (usize, Options<'a>) {
    let rest: Vec<String> = rest.iter().map(|s| (*s).to_owned()).collect();
    let kinds = || {
        strs(&[
            "block",
            "blocks",
            "entity",
            "score",
            "data",
            "predicate",
            "biome",
            "dimension",
            "loaded",
            "items",
            "function",
        ])
    };
    let second: Option<String> = rest.get(1).cloned();
    let fourth: Option<String> = rest.get(3).cloned();
    let Some(kind) = rest.first().cloned() else {
        return (1, Box::new(move |_| kinds()));
    };
    let coords = || strs(&["~", "^"]);
    match kind.as_str() {
        "block" => (
            5,
            Box::new(move |n| {
                if n == 4 {
                    let mut b: Vec<String> = crate::world::block::registry()
                        .iter()
                        .map(|d| d.name.clone())
                        .collect();
                    b.extend(
                        ["#logs", "#planks", "#leaves", "#wool", "#slabs", "#stairs"]
                            .map(str::to_owned),
                    );
                    b
                } else if n == 0 {
                    kinds()
                } else {
                    coords()
                }
            }),
        ),
        "blocks" => (
            11,
            Box::new(move |n| {
                if n == 10 {
                    strs(&["all", "masked"])
                } else {
                    coords()
                }
            }),
        ),
        "entity" => (2, Box::new(move |_| game.options(Arg::Entities))),
        "score" => {
            let arity = if fourth.as_deref() == Some("matches") {
                5
            } else {
                6
            };
            (
                arity,
                Box::new(move |n| match n {
                    1 => {
                        let mut t = game.options(Arg::Entities);
                        t.push("*".to_owned());
                        t
                    }
                    2 | 5 => objective_names(game),
                    3 => strs(&["<", "<=", "=", ">", ">=", "matches"]),
                    4 if arity == 6 => game.options(Arg::Entities),
                    _ => Vec::new(),
                }),
            )
        }
        "data" => {
            let used = match second.as_deref() {
                Some("block") => 4,
                _ => 2,
            };
            (
                1 + used + 1,
                Box::new(move |n| match n {
                    1 => strs(&["entity", "block", "storage"]),
                    2 if second.as_deref() == Some("entity") => game.options(Arg::Entities),
                    _ => Vec::new(),
                }),
            )
        }
        "predicate" => (
            2,
            Box::new(move |_| {
                game.predicates
                    .keys()
                    .map(|k| format!("minecraft:{k}"))
                    .collect()
            }),
        ),
        "biome" => (
            5,
            Box::new(move |n| {
                if n == 4 {
                    super::world::biome_ids(game)
                } else {
                    coords()
                }
            }),
        ),
        "dimension" => (2, Box::new(move |_| strs(DIMENSIONS))),
        "loaded" => (4, Box::new(move |_| coords())),
        "items" => {
            let used = match second.as_deref() {
                Some("block") => 4,
                _ => 2,
            };
            (
                1 + used + 2,
                Box::new(move |n| match n {
                    1 => strs(&["entity", "block"]),
                    2 if second.as_deref() == Some("entity") => game.options(Arg::Entities),
                    n if n == 1 + used => data_cmds::slot_names(game),
                    n if n == 2 + used => game.options(Arg::Item),
                    _ => Vec::new(),
                }),
            )
        }
        "function" => (2, Box::new(move |_| game.function_ids())),
        _ => (1, Box::new(move |_| kinds())),
    }
}

fn store_tab<'a>(game: &'a Game, rest: &[&str]) -> (usize, Options<'a>) {
    let kind: Option<String> = rest.get(1).cloned().map(str::to_owned).or(None);
    let Some(_) = rest.first() else {
        return (1, Box::new(move |_| strs(&["result", "success"])));
    };
    let arity = match kind.as_deref() {
        Some("score") => 4,
        Some("bossbar") => 4,
        Some("entity" | "storage") => 6,
        Some("block") => 8,
        _ => 2,
    };
    (
        arity,
        Box::new(move |n| match n {
            0 => strs(&["result", "success"]),
            1 => strs(&["score", "bossbar", "entity", "block", "storage"]),
            2 => match kind.as_deref() {
                Some("score") => {
                    let mut t = game.options(Arg::Entities);
                    t.push("*".to_owned());
                    t
                }
                Some("bossbar") => text_cmds::bossbar_ids(game),
                Some("entity") => game.options(Arg::Entities),
                _ => Vec::new(),
            },
            3 => match kind.as_deref() {
                Some("score") => objective_names(game),
                Some("bossbar") => strs(&["value", "max"]),
                _ if false => Vec::new(),
                _ => Vec::new(),
            },
            n if matches!(kind.as_deref(), Some("entity" | "block" | "storage"))
                && n == arity - 2 =>
            {
                strs(&["byte", "short", "int", "long", "float", "double"])
            }
            _ => Vec::new(),
        }),
    )
}

impl Game {
    /// The ids of every loaded function, for Tab.
    pub fn function_ids(&self) -> Vec<String> {
        self.functions
            .keys()
            .map(|k| format!("minecraft:{}", k.strip_prefix("minecraft:").unwrap_or(k)))
            .collect()
    }

    /// Runs function `id` as `caller`; its result is the number of its
    /// commands that succeeded.
    fn call_function(&mut self, caller: &Caller, id: &str) -> Result<i32, String> {
        let key = id.strip_prefix("minecraft:").unwrap_or(id);
        let lines = self
            .functions
            .get(key)
            .or_else(|| self.functions.get(id))
            .cloned()
            .ok_or_else(|| format!("Unknown function: {id}"))?;
        let mut ok = 0;
        for line in lines {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let (_, result) = self.run_as(caller, line.strip_prefix('/').unwrap_or(line));
            ok += i32::from(result.is_some());
        }
        Ok(ok)
    }
}
