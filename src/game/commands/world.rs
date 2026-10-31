//! The block-editing commands in full (M43): block states in arguments,
//! `/setblock`, `/fill`, `/clone`, `/fillbiome` and `/place`.

use super::*;
use crate::world::{biome::Biome, block_entity::BlockEntity};

/// A block with its state: `oak_stairs[facing=north,half=top]`. A trailing
/// `{...}` of block entity data is accepted and ignored (DECISIONS.md).
pub(super) fn block_state_named(text: &str) -> Result<BlockState, String> {
    let text = text.split('{').next().unwrap_or(text);
    let (name, props) = match text.split_once('[') {
        Some((name, rest)) => (
            name,
            Some(
                rest.strip_suffix(']')
                    .ok_or("Expected ']' to close the block states")?,
            ),
        ),
        None => (text, None),
    };
    let mut state = block_named(name)?;
    let layout = crate::world::block::registry().layout(state.id());
    for pair in props.unwrap_or("").split(',').filter(|p| !p.is_empty()) {
        let (key, value) = pair
            .split_once('=')
            .ok_or_else(|| format!("Expected '=' after '{pair}'"))?;
        let prop = layout
            .props
            .iter()
            .copied()
            .find(|p| p.name() == key.trim())
            .ok_or_else(|| format!("Block {name} does not have property '{key}'"))?;
        let index = prop
            .index_of(value.trim())
            .ok_or_else(|| format!("Block {name} property '{key}' does not accept '{value}'"))?;
        state = state.with(prop, index);
    }
    Ok(state)
}

/// What Tab offers for a block word that is already part-way into its
/// `[state]`: the properties not yet given, then a property's values.
pub(super) fn state_completions(word: &str) -> Vec<String> {
    let Some((name, rest)) = word.split_once('[') else {
        return Vec::new();
    };
    let Ok(state) = block_named(name) else {
        return Vec::new();
    };
    let layout = crate::world::block::registry().layout(state.id());
    let (done, current) = match rest.rsplit_once(',') {
        Some((done, current)) => (format!("{done},"), current),
        None => (String::new(), rest),
    };
    let head = format!("{name}[{done}");
    match current.split_once('=') {
        Some((key, _)) => layout
            .props
            .iter()
            .find(|p| p.name() == key)
            .map(|p| {
                p.values()
                    .iter()
                    .map(|v| format!("{head}{key}={v}"))
                    .collect()
            })
            .unwrap_or_default(),
        None => layout
            .props
            .iter()
            .map(|p| format!("{head}{}=", p.name()))
            .collect(),
    }
}

fn loaded(game: &Game, at: IVec3) -> Result<BlockState, String> {
    game.chunks
        .world()
        .block(at.x, at.y, at.z)
        .ok_or_else(|| "That position is not loaded".to_owned())
}

/// Breaks the block at `at` as a player's hand would, with its drops.
fn destroy(game: &mut Game, index: usize, at: IVec3) {
    game.with_player(index, |g| {
        let mode = g.player.mode;
        g.player.mode = GameMode::Survival;
        g.break_block(at);
        g.player.mode = mode;
    });
}

pub(super) fn setblock(
    game: &mut Game,
    caller: &Caller,
    args: &[&str],
) -> Result<Vec<Reply>, String> {
    let (Some(words), Some(block)) = (args.get(0..3), args.get(3)) else {
        return Err("Usage: /setblock <x y z> <block> [replace|keep|destroy]".to_owned());
    };
    let at = block_position(words, caller.pos)?;
    let block = block_state_named(block)?;
    let mode = args.get(4).copied().unwrap_or("replace");
    game.enter_realm(caller.dim);
    let here = loaded(game, at)?;
    match mode {
        "keep" if !here.is_air() => return Err("Could not set the block".to_owned()),
        "replace" | "keep" => {}
        "destroy" => {
            if !here.is_air() {
                destroy(game, caller.index, at);
            }
        }
        other => return Err(format!("Unknown mode: {other}")),
    }
    if mode != "destroy" && here == block {
        return Err("Could not set the block".to_owned());
    }
    game.edit(&[(at, block)]);
    Ok(tell(format!(
        "Changed the block at {}, {}, {}",
        at.x, at.y, at.z
    )))
}

pub(super) fn fill(game: &mut Game, caller: &Caller, args: &[&str]) -> Result<Vec<Reply>, String> {
    let usage =
        "Usage: /fill <x1 y1 z1> <x2 y2 z2> <block> [replace [filter]|keep|hollow|outline|destroy]";
    let (Some(from), Some(to), Some(block)) = (args.get(0..3), args.get(3..6), args.get(6)) else {
        return Err(usage.to_owned());
    };
    let origin = caller.pos;
    let (a, b) = (block_position(from, origin)?, block_position(to, origin)?);
    let (lo, hi) = (a.min(b), a.max(b));
    let size = (hi - lo + 1).as_i64vec3();
    let count = size.x * size.y * size.z;
    if count > FILL_LIMIT {
        return Err(format!(
            "Too many blocks in the specified area (maximum {FILL_LIMIT}, specified {count})"
        ));
    }
    let block = block_state_named(block)?;
    let mode = args.get(7).copied().unwrap_or("replace");
    if !["replace", "keep", "hollow", "outline", "destroy"].contains(&mode) {
        return Err(format!("Unknown mode: {mode}"));
    }
    // `replace <filter>` only replaces the filter's blocks; a state in the
    // filter must match.
    let filter = match (mode, args.get(8)) {
        ("replace", Some(f)) => Some(block_state_named(f)?),
        _ => None,
    };
    let filter_has_state = args.get(8).is_some_and(|f| f.contains('['));
    game.enter_realm(caller.dim);
    let air = BlockState::AIR;
    let mut edits = Vec::new();
    let mut broken = Vec::new();
    for y in lo.y..=hi.y {
        for z in lo.z..=hi.z {
            for x in lo.x..=hi.x {
                let at = IVec3::new(x, y, z);
                let edge =
                    x == lo.x || x == hi.x || y == lo.y || y == hi.y || z == lo.z || z == hi.z;
                let here = loaded(game, at)?;
                if let Some(f) = filter
                    && (if filter_has_state {
                        here != f
                    } else {
                        here.id() != f.id()
                    })
                {
                    continue;
                }
                let new = match mode {
                    "keep" if !here.is_air() => continue,
                    "hollow" if !edge => air,
                    "outline" if !edge => continue,
                    _ => block,
                };
                if mode == "destroy" && !here.is_air() {
                    broken.push(at);
                }
                if new != here || mode == "destroy" {
                    edits.push((at, new));
                }
            }
        }
    }
    if edits.is_empty() {
        return Err("No blocks were filled".to_owned());
    }
    for at in broken {
        destroy(game, caller.index, at);
    }
    game.edit(&edits);
    Ok(tell(format!(
        "Successfully filled {} block(s)",
        edits.len()
    )))
}

pub(super) fn clone(game: &mut Game, caller: &Caller, args: &[&str]) -> Result<Vec<Reply>, String> {
    let usage = "Usage: /clone <begin> <end> <destination> [replace|masked|filtered <block>] [force|move|normal]";
    let (Some(b), Some(e), Some(d)) = (args.get(0..3), args.get(3..6), args.get(6..9)) else {
        return Err(usage.to_owned());
    };
    let origin = caller.pos;
    let (a, c) = (block_position(b, origin)?, block_position(e, origin)?);
    let (lo, hi) = (a.min(c), a.max(c));
    let dest = block_position(d, origin)?;
    let size = hi - lo + 1;
    let count = i64::from(size.x) * i64::from(size.y) * i64::from(size.z);
    if count > FILL_LIMIT {
        return Err(format!(
            "Too many blocks in the specified area (maximum {FILL_LIMIT}, specified {count})"
        ));
    }
    let dest_hi = dest + size - 1;
    let mut rest = &args[9..];
    let mask = match rest.first().copied() {
        Some("masked") => {
            rest = &rest[1..];
            "masked"
        }
        Some("filtered") => {
            rest = &rest[1..];
            "filtered"
        }
        Some("replace") => {
            rest = &rest[1..];
            "replace"
        }
        _ => "replace",
    };
    let filter = if mask == "filtered" {
        let f = rest.first().ok_or(usage)?;
        rest = &rest[1..];
        Some(block_state_named(f)?)
    } else {
        None
    };
    let how = rest.first().copied().unwrap_or("normal");
    if !["normal", "force", "move"].contains(&how) {
        return Err(format!("Unknown mode: {how}"));
    }
    let overlaps = lo.x <= dest_hi.x
        && hi.x >= dest.x
        && lo.y <= dest_hi.y
        && hi.y >= dest.y
        && lo.z <= dest_hi.z
        && hi.z >= dest.z;
    if overlaps && how != "force" && how != "move" {
        return Err("The source and destination areas cannot overlap".to_owned());
    }
    game.enter_realm(caller.dim);
    // Read everything first, so an overlap copies what was there.
    let mut moved = Vec::new();
    for y in 0..size.y {
        for z in 0..size.z {
            for x in 0..size.x {
                let from = lo + IVec3::new(x, y, z);
                let state = loaded(game, from)?;
                loaded(game, dest + IVec3::new(x, y, z))?;
                let take = match mask {
                    "masked" => !state.is_air(),
                    "filtered" => filter.is_some_and(|f| f.id() == state.id()),
                    _ => true,
                };
                if take {
                    let entity = game.chunks.block_entity(from).cloned();
                    moved.push((from, dest + IVec3::new(x, y, z), state, entity));
                }
            }
        }
    }
    if moved.is_empty() {
        return Err("No blocks were cloned".to_owned());
    }
    let mut edits = Vec::new();
    if how == "move" {
        for (from, _, _, _) in &moved {
            game.chunks.set_block_entity(*from, None);
            edits.push((*from, BlockState::AIR));
        }
    }
    for (_, to, state, _) in &moved {
        edits.push((*to, *state));
    }
    game.edit(&edits);
    for (_, to, _, entity) in moved {
        if let Some(entity) = entity {
            game.chunks.set_block_entity(to, Some(entity));
        }
    }
    let n = edits.len() - if how == "move" { edits.len() / 2 } else { 0 };
    Ok(tell(format!("Successfully cloned {n} block(s)")))
}

pub(super) fn biome_ids(_: &Game) -> Vec<String> {
    Biome::ALL
        .iter()
        .map(|b| format!("minecraft:{}", biome_id(*b)))
        .collect()
}

pub(super) fn fillbiome(
    game: &mut Game,
    caller: &Caller,
    args: &[&str],
) -> Result<Vec<Reply>, String> {
    let usage = "Usage: /fillbiome <from> <to> <biome> [replace <filter>]";
    let (Some(f), Some(t), Some(name)) = (args.get(0..3), args.get(3..6), args.get(6)) else {
        return Err(usage.to_owned());
    };
    let origin = caller.pos;
    let (a, b) = (block_position(f, origin)?, block_position(t, origin)?);
    let (lo, hi) = (a.min(b), a.max(b));
    let id = name.strip_prefix("minecraft:").unwrap_or(name);
    let biome = Biome::ALL
        .iter()
        .copied()
        .find(|b| biome_id(*b) == id)
        .ok_or_else(|| format!("Unknown biome: {name}"))?;
    let replace = match (args.get(7).copied(), args.get(8)) {
        (Some("replace"), Some(w)) => {
            let w = w.strip_prefix("minecraft:").unwrap_or(w);
            Some(
                Biome::ALL
                    .iter()
                    .copied()
                    .find(|b| biome_id(*b) == w)
                    .ok_or_else(|| format!("Unknown biome: {w}"))?,
            )
        }
        (None, _) => None,
        _ => return Err(usage.to_owned()),
    };
    let count =
        i64::from(hi.x - lo.x + 1) * i64::from(hi.z - lo.z + 1) * i64::from(hi.y - lo.y + 1);
    if count > 32768 * 4 {
        return Err(format!(
            "Too many blocks in the specified area (maximum {}, specified {count})",
            32768 * 4
        ));
    }
    game.enter_realm(caller.dim);
    let mut columns = Vec::new();
    for z in lo.z..=hi.z {
        for x in lo.x..=hi.x {
            let here = game
                .chunks
                .world()
                .biome_at(x, z)
                .ok_or("That position is not loaded")?;
            if replace.is_none_or(|r| r == here) {
                columns.push(((x, z), biome));
            }
        }
    }
    let changed = game.chunks.set_biomes(&columns);
    if changed == 0 {
        return Err("No biomes were changed".to_owned());
    }
    Ok(tell(format!(
        "{changed} biome entries set between {}, {}, {} and {}, {}, {}",
        lo.x, lo.y, lo.z, hi.x, hi.y, hi.z
    )))
}

pub(super) fn template_names(_: &Game) -> Vec<String> {
    let mut names: Vec<String> = crate::world::structure::template::registry()
        .templates
        .keys()
        .map(|n| format!("minecraft:{n}"))
        .collect();
    names.sort();
    names
}

pub(super) fn place(game: &mut Game, caller: &Caller, args: &[&str]) -> Result<Vec<Reply>, String> {
    let usage = "Usage: /place (template <name> | structure <name> | feature <name> | jigsaw <pool> <target> <depth>) [pos] [rotation]";
    let (Some(kind), Some(name)) = (args.first().copied(), args.get(1)) else {
        return Err(usage.to_owned());
    };
    let name = name.strip_prefix("minecraft:").unwrap_or(name);
    let origin = caller.pos;
    let pos_at = |from: usize| -> Result<IVec3, String> {
        match args.get(from..from + 3) {
            Some(words) => block_position(words, origin),
            None => Ok(origin.floor().as_ivec3()),
        }
    };
    match kind {
        "template" | "structure" => {
            let registry = crate::world::structure::template::registry();
            let template = registry
                .get(name)
                .or_else(|| registry.pool(name).first().and_then(|n| registry.get(n)))
                .ok_or_else(|| format!("Unknown {kind}: {name}"))?;
            let at = pos_at(2)?;
            let turns = match args.get(5).copied() {
                None | Some("none") => 0,
                Some("clockwise_90") => 1,
                Some("180") => 2,
                Some("counterclockwise_90") => 3,
                Some(other) => return Err(format!("Unknown rotation: {other}")),
            };
            game.enter_realm(caller.dim);
            let piece = template.place(at, turns);
            let edits: Vec<_> = piece.blocks.iter().map(|&(p, s)| (p, s)).collect();
            for (p, _) in &edits {
                loaded(game, *p)?;
            }
            game.edit(&edits);
            Ok(tell(format!(
                "Placed {name} at {}, {}, {}",
                at.x, at.y, at.z
            )))
        }
        "feature" | "jigsaw" => Err(format!(
            "The game has no {kind} named {name} to place; use template or structure"
        )),
        _ => Err(usage.to_owned()),
    }
}

/// Block entities that survive `/clone` (all of them), named for the tests.
#[allow(dead_code)]
pub(super) fn keeps_contents(entity: &BlockEntity) -> bool {
    matches!(
        entity,
        BlockEntity::Chest(_) | BlockEntity::Furnace(_) | BlockEntity::Dispenser(_)
    )
}
