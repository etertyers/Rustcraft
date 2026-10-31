//! `/data`, `/item` and `/loot` (M44): NBT-shaped views of players, mobs,
//! containers and command storage, item slots, and loot from the tables
//! the game has. Which data paths exist is listed in `docs/DECISIONS.md`.

use super::{board_cmds::render_json, *};
use crate::{
    game::nbt::{self, Nbt, Step},
    inventory::{player as slots, stack::ItemStack},
    world::block_entity::BlockEntity,
};

/// Where data lives.
#[derive(Debug, Clone, PartialEq)]
pub(super) enum DataTarget {
    Entity(Target),
    Block(IVec3),
    Storage(String),
}

fn f(n: f64) -> Nbt {
    Nbt::Double(n)
}

/// An item stack as vanilla's compound: `{id, count, components}`.
pub(super) fn stack_nbt(stack: &ItemStack) -> Nbt {
    let mut map = std::collections::BTreeMap::new();
    map.insert(
        "id".to_owned(),
        Nbt::Str(format!("minecraft:{}", stack.id.name())),
    );
    map.insert("count".to_owned(), Nbt::Int(i32::from(stack.count)));
    let mut comps = std::collections::BTreeMap::new();
    if stack.damage > 0 {
        comps.insert("damage".to_owned(), Nbt::Int(i32::from(stack.damage)));
    }
    if let Some(name) = stack.custom_name() {
        comps.insert("custom_name".to_owned(), Nbt::Str(name.to_owned()));
    }
    if stack.is_enchanted() {
        let levels = stack
            .enchantments()
            .iter()
            .map(|(e, l)| {
                (
                    format!("minecraft:{}", enchantment_id(*e)),
                    Nbt::Int(i32::from(*l)),
                )
            })
            .collect();
        comps.insert(
            "enchantments".to_owned(),
            Nbt::compound([("levels", Nbt::Compound(levels))]),
        );
    }
    for (key, list) in [
        ("can_break", stack.can_break()),
        ("can_place_on", stack.can_place_on()),
    ] {
        if !list.is_empty() {
            comps.insert(
                key.to_owned(),
                Nbt::List(list.iter().cloned().map(Nbt::Str).collect()),
            );
        }
    }
    if !comps.is_empty() {
        map.insert("components".to_owned(), Nbt::Compound(comps));
    }
    Nbt::Compound(map)
}

/// The stack a compound describes (`count` defaults to 1).
pub(super) fn nbt_stack(value: &Nbt) -> Result<ItemStack, String> {
    let id = value
        .get("id")
        .and_then(Nbt::as_str)
        .ok_or("An item needs an id")?;
    let item = item_named(id)?;
    let count = value
        .get("count")
        .or(value.get("Count"))
        .and_then(Nbt::as_i32)
        .unwrap_or(1)
        .clamp(1, 99);
    let mut stack = ItemStack::new(item, count as u8);
    if let Some(Nbt::Compound(map)) = value.get("components") {
        let mut parts: Vec<String> = Vec::new();
        for (k, v) in map {
            parts.push(match k.as_str() {
                "damage" => format!("damage={}", v.as_i32().unwrap_or(0)),
                "custom_name" => format!("custom_name='\"{}\"'", v.as_str().unwrap_or("")),
                "enchantments" => format!("enchantments={}", v.snbt()),
                "can_break" => format!("can_break={}", v.snbt()),
                "can_place_on" => format!("can_place_on={}", v.snbt()),
                other => return Err(format!("Unknown item component '{other}'")),
            });
        }
        components::apply(&mut stack, &parts.join(","))?;
    }
    Ok(stack)
}

fn items_nbt(slots: &[Option<ItemStack>]) -> Nbt {
    Nbt::List(
        slots
            .iter()
            .enumerate()
            .filter_map(|(i, s)| {
                let Nbt::Compound(mut map) = stack_nbt(s.as_ref()?) else {
                    return None;
                };
                map.insert("Slot".to_owned(), Nbt::Byte(i as i8));
                Some(Nbt::Compound(map))
            })
            .collect(),
    )
}

fn slots_from(list: &Nbt, size: usize) -> Result<Vec<Option<ItemStack>>, String> {
    let Nbt::List(items) = list else {
        return Err("Items must be a list".to_owned());
    };
    let mut out = vec![None; size];
    for item in items {
        let slot = item
            .get("Slot")
            .and_then(Nbt::as_i32)
            .ok_or("An item needs a Slot")?;
        let at = usize::try_from(slot)
            .ok()
            .filter(|s| *s < size)
            .ok_or_else(|| format!("Slot {slot} is out of range"))?;
        out[at] = Some(nbt_stack(item)?);
    }
    Ok(out)
}

/// The data of a player or mob, as far as the game has it.
pub(super) fn entity_nbt(game: &mut Game, target: Target) -> Option<Nbt> {
    let dim = game.dimension;
    let cand = game
        .candidates(dim)
        .into_iter()
        .find(|c| c.target == target)?;
    let mut map: std::collections::BTreeMap<String, Nbt> = std::collections::BTreeMap::new();
    let mut put = |k: &str, v: Nbt| {
        map.insert(k.to_owned(), v);
    };
    put(
        "Pos",
        Nbt::List(vec![f(cand.feet.x), f(cand.feet.y), f(cand.feet.z)]),
    );
    put(
        "Rotation",
        Nbt::List(vec![
            Nbt::Float(cand.yaw as f32),
            Nbt::Float(cand.pitch as f32),
        ]),
    );
    put("Health", Nbt::Float(cand.health));
    put("OnGround", Nbt::Byte(i8::from(cand.on_ground)));
    put(
        "Tags",
        Nbt::List(cand.tags.iter().cloned().map(Nbt::Str).collect()),
    );
    match target {
        Target::Player(i) => {
            let p = game.player_at(i);
            put("id", Nbt::Str("minecraft:player".to_owned()));
            put("foodLevel", Nbt::Int(p.stats.food));
            put("foodSaturationLevel", Nbt::Float(p.stats.saturation));
            put("XpLevel", Nbt::Int(p.stats.experience.level as i32));
            put("XpTotal", Nbt::Int(p.stats.experience.total as i32));
            put("XpP", Nbt::Float(p.stats.experience.progress));
            put("Air", Nbt::Short(p.stats.air as i16));
            put("Fire", Nbt::Short(p.stats.fire as i16));
            put(
                "playerGameType",
                Nbt::Int(
                    crate::player::mode::GameMode::ALL
                        .iter()
                        .position(|m| *m == p.mode)
                        .unwrap_or(0) as i32,
                ),
            );
            put("SelectedItemSlot", Nbt::Int(p.inventory.selected as i32));
            put(
                "Dimension",
                Nbt::Str(format!("minecraft:{}", Game::dimension_slug(p.dimension))),
            );
            let all: Vec<Option<ItemStack>> = (0..slots::SLOTS)
                .map(|s| p.inventory.slot(s).cloned())
                .collect();
            // Vanilla numbers the armor slots 100–103 and the off-hand −106.
            let mut list = Vec::new();
            if let Nbt::List(items) = items_nbt(&all) {
                for item in items {
                    let Nbt::Compound(mut m) = item else { continue };
                    let slot = m.get("Slot").and_then(Nbt::as_i32).unwrap_or(0) as usize;
                    let mapped = match slot {
                        slots::ARMOR_FEET => 100,
                        slots::ARMOR_LEGS => 101,
                        slots::ARMOR_CHEST => 102,
                        slots::ARMOR_HEAD => 103,
                        slots::OFFHAND => -106,
                        s => s as i32,
                    };
                    m.insert("Slot".to_owned(), Nbt::Byte(mapped as i8));
                    list.push(Nbt::Compound(m));
                }
            }
            put("Inventory", Nbt::List(list));
        }
        Target::Mob(id) => {
            let m = game.mobs.iter().find(|m| m.id == id)?;
            put(
                "id",
                Nbt::Str(format!("minecraft:{}", game.mob_defs.get(m.kind).name)),
            );
            put("Age", Nbt::Int(m.data.age));
            put("Air", Nbt::Short(m.stats.air as i16));
            put("Fire", Nbt::Short(m.stats.fire as i16));
            put(
                "Motion",
                Nbt::List(vec![
                    f(m.body.velocity.x),
                    f(m.body.velocity.y),
                    f(m.body.velocity.z),
                ]),
            );
            if let Some(name) = &m.data.custom_name {
                put("CustomName", Nbt::Str(format!("{{\"text\":\"{name}\"}}")));
            }
        }
    }
    Some(Nbt::Compound(map))
}

/// Writes the keys of `new` that differ from `old` back into a mob. Players
/// can't be modified, as in vanilla.
fn apply_mob(game: &mut Game, id: u64, old: &Nbt, new: &Nbt) -> Result<(), String> {
    let Nbt::Compound(new_map) = new else {
        return Err("Entity data must be a compound".to_owned());
    };
    for (key, value) in new_map {
        if old.get(key) == Some(value) {
            continue;
        }
        let num = |v: &Nbt| v.as_f64().ok_or_else(|| format!("{key} must be a number"));
        let triple = |v: &Nbt| -> Result<DVec3, String> {
            let Nbt::List(items) = v else {
                return Err(format!("{key} must be a list of three numbers"));
            };
            match items
                .iter()
                .map(Nbt::as_f64)
                .collect::<Option<Vec<_>>>()
                .as_deref()
            {
                Some([x, y, z]) => Ok(DVec3::new(*x, *y, *z)),
                _ => Err(format!("{key} must be a list of three numbers")),
            }
        };
        let Some(mob) = game.mobs.iter_mut().find(|m| m.id == id) else {
            return Err("The entity is gone".to_owned());
        };
        match key.as_str() {
            "Health" => mob.stats.health = num(value)? as f32,
            "Air" => mob.stats.air = num(value)? as i32,
            "Fire" => mob.stats.fire = num(value)? as i32,
            "Age" => mob.data.age = num(value)? as i32,
            "Tags" => {
                mob.data.tags = match value {
                    Nbt::List(l) => l
                        .iter()
                        .filter_map(|t| t.as_str().map(str::to_owned))
                        .collect(),
                    _ => return Err("Tags must be a list".to_owned()),
                }
            }
            "CustomName" => {
                let json = value.as_str().ok_or("CustomName must be a string")?;
                let plain = components::plain_text(json);
                mob.data.custom_name = (!plain.is_empty()).then_some(plain);
            }
            "Pos" => mob.body.set_position(triple(value)?),
            "Motion" => mob.body.velocity = triple(value)?,
            "Rotation" => {
                if let Nbt::List(a) = value
                    && let [yaw, pitch] = a.iter().filter_map(Nbt::as_f64).collect::<Vec<_>>()[..]
                {
                    mob.yaw = wrap_degrees(yaw - 180.0).to_radians() as f32;
                    mob.head_yaw = mob.yaw;
                    mob.pitch = (-pitch).to_radians() as f32;
                }
            }
            "id" | "OnGround" => {}
            other => return Err(format!("The game can't change entity data '{other}'")),
        }
    }
    Ok(())
}

/// A block entity's data.
pub(super) fn block_nbt(game: &Game, at: IVec3) -> Option<Nbt> {
    let entity = game.chunks.block_entity(at)?;
    let mut map = std::collections::BTreeMap::new();
    let id = match entity {
        BlockEntity::Chest(s) => {
            map.insert("Items".to_owned(), items_nbt(s));
            "minecraft:chest"
        }
        BlockEntity::Dispenser(s) => {
            map.insert("Items".to_owned(), items_nbt(s));
            "minecraft:dispenser"
        }
        BlockEntity::Hopper(h) => {
            map.insert("Items".to_owned(), items_nbt(&h.slots));
            map.insert(
                "TransferCooldown".to_owned(),
                Nbt::Int(i32::from(h.cooldown)),
            );
            "minecraft:hopper"
        }
        BlockEntity::Spawner(s) => {
            map.insert("Delay".to_owned(), Nbt::Short(s.delay as i16));
            map.insert(
                "SpawnData".to_owned(),
                Nbt::compound([(
                    "entity",
                    Nbt::compound([("id", Nbt::Str(format!("minecraft:{}", s.mob)))]),
                )]),
            );
            "minecraft:mob_spawner"
        }
        BlockEntity::Furnace(_) => "minecraft:furnace",
        BlockEntity::BrewingStand(_) => "minecraft:brewing_stand",
    };
    map.insert("id".to_owned(), Nbt::Str(id.to_owned()));
    map.insert("x".to_owned(), Nbt::Int(at.x));
    map.insert("y".to_owned(), Nbt::Int(at.y));
    map.insert("z".to_owned(), Nbt::Int(at.z));
    Some(Nbt::Compound(map))
}

fn apply_block(game: &mut Game, at: IVec3, new: &Nbt) -> Result<(), String> {
    let Some(Nbt::List(_)) = new.get("Items") else {
        return Ok(());
    };
    let items = new.get("Items").unwrap();
    let Some(entity) = game.chunks.block_entity_mut(at) else {
        return Err("The target block is not a block entity".to_owned());
    };
    match entity {
        BlockEntity::Chest(s) => *s = slots_from(items, s.len())?,
        BlockEntity::Dispenser(s) => *s = slots_from(items, s.len())?,
        BlockEntity::Hopper(h) => h.slots = slots_from(items, h.slots.len())?,
        _ => {
            return Err(
                "The game can only change the contents of chests, dispensers and hoppers"
                    .to_owned(),
            );
        }
    }
    Ok(())
}

/// The current data of `target`.
pub(super) fn read(game: &mut Game, target: &DataTarget) -> Result<Nbt, String> {
    match target {
        DataTarget::Entity(t) => {
            entity_nbt(game, *t).ok_or_else(|| "No entity was found".to_owned())
        }
        DataTarget::Block(at) => {
            block_nbt(game, *at).ok_or_else(|| "The target block is not a block entity".to_owned())
        }
        DataTarget::Storage(id) => Ok(game
            .storage
            .get(id)
            .cloned()
            .unwrap_or_else(|| Nbt::Compound(Default::default()))),
    }
}

/// Stores `new` as the data of `target` (what changed from `old`).
fn write(game: &mut Game, target: &DataTarget, old: &Nbt, new: &Nbt) -> Result<(), String> {
    match target {
        DataTarget::Entity(Target::Player(_)) => Err("Unable to modify player data".to_owned()),
        DataTarget::Entity(Target::Mob(id)) => apply_mob(game, *id, old, new),
        DataTarget::Block(at) => apply_block(game, *at, new),
        DataTarget::Storage(id) => {
            game.storage.insert(id.clone(), new.clone());
            Ok(())
        }
    }
}

fn describe(game: &Game, target: &DataTarget) -> (String, String) {
    match target {
        DataTarget::Entity(t) => (game.target_name(*t), "entity data".to_owned()),
        DataTarget::Block(at) => (
            format!("{}, {}, {}", at.x, at.y, at.z),
            "block data".to_owned(),
        ),
        DataTarget::Storage(id) => (format!("storage {id}"), "storage".to_owned()),
    }
}

/// `Modified entity data of Pig`, `Modified storage mine:box`.
fn modified(game: &Game, target: &DataTarget) -> String {
    match target {
        DataTarget::Storage(id) => format!("Modified storage {id}"),
        _ => {
            let (who, kind) = describe(game, target);
            format!("Modified {kind} of {who}")
        }
    }
}

/// `Pig has the following entity data: ...`, `Storage x has the following contents: ...`.
fn has_text(game: &Game, target: &DataTarget, value: &Nbt) -> String {
    match target {
        DataTarget::Storage(id) => {
            format!("Storage {id} has the following contents: {}", value.snbt())
        }
        _ => {
            let (who, kind) = describe(game, target);
            format!("{who} has the following {kind}: {}", value.snbt())
        }
    }
}

/// `entity <selector>`, `block <x y z>` or `storage <id>`; the words used.
pub(super) fn parse_target(
    game: &mut Game,
    caller: &Caller,
    args: &[&str],
) -> Result<(DataTarget, usize), String> {
    match args.first().copied() {
        Some("entity") => {
            let word = args.get(1).ok_or("Expected an entity")?;
            let found = game.targets(caller, word)?;
            let [only] = found[..] else {
                return Err(
                    "Only one entity is allowed, but the provided selector allows more than one"
                        .to_owned(),
                );
            };
            Ok((DataTarget::Entity(only), 2))
        }
        Some("block") => {
            let words = args.get(1..4).ok_or("Expected a position")?;
            Ok((DataTarget::Block(block_position(words, caller.pos)?), 4))
        }
        Some("storage") => {
            let id = args.get(1).ok_or("Expected a storage id")?;
            Ok((
                DataTarget::Storage(id.strip_prefix("minecraft:").unwrap_or(id).to_owned()),
                2,
            ))
        }
        _ => Err("Expected entity, block or storage".to_owned()),
    }
}

fn parse_nbt(text: &str) -> Result<Nbt, String> {
    Nbt::parse(text).map_err(|e| format!("Invalid NBT: {e}"))
}

fn parse_compound(text: &str) -> Result<Nbt, String> {
    match parse_nbt(text)? {
        c @ Nbt::Compound(_) => Ok(c),
        _ => Err("Expected a compound".to_owned()),
    }
}

fn parse_path_arg(text: &str) -> Result<Vec<Step>, String> {
    nbt::parse_path(text)
}

/// Values at `path` in the data of `target`.
pub(super) fn values(
    game: &mut Game,
    target: &DataTarget,
    path: Option<&str>,
) -> Result<Vec<Nbt>, String> {
    let root = read(game, target)?;
    match path {
        None => Ok(vec![root]),
        Some(p) => {
            let steps = parse_path_arg(p)?;
            Ok(nbt::get_all(&root, &steps).into_iter().cloned().collect())
        }
    }
}

pub(super) fn data(game: &mut Game, caller: &Caller, args: &[&str]) -> Result<Vec<Reply>, String> {
    let usage = "Usage: /data (get|merge|modify|remove) (entity|block|storage) ...";
    match args.first().copied().ok_or(usage)? {
        "get" => {
            let (target, used) = parse_target(game, caller, &args[1..])?;
            let path = args.get(1 + used).copied();
            let scale: Option<f64> = args.get(2 + used).map(|s| number(s, "scale")).transpose()?;
            let found = values(game, &target, path)?;
            let (who, kind) = describe(game, &target);
            let Some(first) = found.first() else {
                return Err(format!(
                    "Found no elements matching {} for {who}",
                    path.unwrap_or("")
                ));
            };
            if let Some(scale) = scale {
                let n = first.as_f64().ok_or("Expected a numeric value")?;
                let r = (n * scale) as i32;
                caller.report(r);
                return Ok(tell(format!("{who} after scale factor of {scale} is {r}")));
            }
            let result = match first {
                Nbt::List(l) => l.len() as i32,
                Nbt::Compound(c) => c.len() as i32,
                Nbt::Str(s) => s.chars().count() as i32,
                n => n.as_f64().map_or(1, |v| v as i32),
            };
            caller.report(result);
            let _ = kind;
            Ok(tell(has_text(game, &target, first)))
        }
        "merge" => {
            let (target, used) = parse_target(game, caller, &args[1..])?;
            let patch = parse_compound(&rest(args, 1 + used))?;
            let old = read(game, &target)?;
            let mut new = old.clone();
            if !nbt::merge(&mut new, &patch) {
                return Err(
                    "Nothing changed. The specified properties already have these values"
                        .to_owned(),
                );
            }
            write(game, &target, &old, &new)?;
            Ok(tell(modified(game, &target)))
        }
        "remove" => {
            let (target, used) = parse_target(game, caller, &args[1..])?;
            let path = parse_path_arg(args.get(1 + used).ok_or("Expected a path")?)?;
            let old = read(game, &target)?;
            let mut new = old.clone();
            if nbt::remove(&mut new, &path) == 0 {
                return Err("Nothing changed. That property doesn't exist".to_owned());
            }
            write(game, &target, &old, &new)?;
            Ok(tell(modified(game, &target)))
        }
        "modify" => modify(game, caller, &args[1..]),
        _ => Err(usage.to_owned()),
    }
}

fn modify(game: &mut Game, caller: &Caller, args: &[&str]) -> Result<Vec<Reply>, String> {
    let (target, used) = parse_target(game, caller, args)?;
    let path_text = args.get(used).ok_or("Expected a path")?;
    let path = parse_path_arg(path_text)?;
    let mut at = used + 1;
    let how = args
        .get(at)
        .copied()
        .ok_or("Expected set, merge, append, prepend or insert")?;
    at += 1;
    let index: Option<i32> = if how == "insert" {
        let i = number(args.get(at).ok_or("Expected an index")?, "index")?;
        at += 1;
        Some(i)
    } else {
        None
    };
    let source = args
        .get(at)
        .copied()
        .ok_or("Expected from, value or string")?;
    at += 1;
    let incoming: Vec<Nbt> = match source {
        "value" => vec![parse_nbt(&rest(args, at))?],
        "from" | "string" => {
            let (src, n) = parse_target(game, caller, &args[at..])?;
            let src_path = args.get(at + n).copied();
            let found = values(game, &src, src_path)?;
            if found.is_empty() {
                return Err("Found no elements matching the source path".to_owned());
            }
            if source == "string" {
                let text = found[0]
                    .as_str()
                    .ok_or("The source isn't a string")?
                    .to_owned();
                let chars: Vec<char> = text.chars().collect();
                let start: usize = args
                    .get(at + n + 1)
                    .map(|s| number(s, "start"))
                    .transpose()?
                    .unwrap_or(0);
                let end: usize = args
                    .get(at + n + 2)
                    .map(|s| number(s, "end"))
                    .transpose()?
                    .unwrap_or(chars.len());
                vec![Nbt::Str(
                    chars[start.min(chars.len())..end.min(chars.len())]
                        .iter()
                        .collect(),
                )]
            } else {
                found
            }
        }
        other => return Err(format!("Unknown source: {other}")),
    };
    let old = read(game, &target)?;
    let mut new = old.clone();
    match how {
        "set" => {
            let value = incoming.last().unwrap();
            nbt::set(&mut new, &path, value);
        }
        "merge" => {
            let mut any = false;
            for value in &incoming {
                if !matches!(value, Nbt::Compound(_)) {
                    return Err("Expected a compound to merge".to_owned());
                }
                let Some(target_at) = nbt::get_all(&new, &path).first().map(|v| (*v).clone())
                else {
                    return Err("Found no element at the path to merge into".to_owned());
                };
                let mut merged = target_at;
                any |= nbt::merge(&mut merged, value);
                nbt::set(&mut new, &path, &merged);
            }
            let _ = any;
        }
        "append" | "prepend" | "insert" => {
            if nbt::get_all(&new, &path).is_empty() {
                nbt::set(&mut new, &path, &Nbt::List(Vec::new()));
            }
            let mut ok = true;
            nbt::for_each_mut(&mut new, &path, &mut |list| match list {
                Nbt::List(items) => {
                    for (n, value) in incoming.iter().enumerate() {
                        match how {
                            "append" => items.push(value.clone()),
                            "prepend" => items.insert(n.min(items.len()), value.clone()),
                            _ => {
                                let len = items.len() as i32;
                                let i = index.unwrap_or(0);
                                let i = if i < 0 { len + i + 1 } else { i };
                                let i = (i.max(0) as usize).min(items.len()) + n;
                                items.insert(i.min(items.len()), value.clone());
                            }
                        }
                    }
                }
                _ => ok = false,
            });
            if !ok {
                return Err("Expected a list at the path".to_owned());
            }
        }
        other => return Err(format!("Unknown modification: {other}")),
    }
    if new == old {
        return Err(
            "Nothing changed. The specified properties already have these values".to_owned(),
        );
    }
    write(game, &target, &old, &new)?;
    Ok(tell(modified(game, &target)))
}

/// Sets the value at `path` of `target` to a number of type `ty` times `scale`
/// (`/execute store ... data`).
pub(super) fn store_number(
    game: &mut Game,
    target: &DataTarget,
    path: &str,
    ty: &str,
    scale: f64,
    value: i32,
) -> Result<(), String> {
    let scaled = f64::from(value) * scale;
    let nbt_value = match ty {
        "byte" => Nbt::Byte(scaled as i8),
        "short" => Nbt::Short(scaled as i16),
        "int" => Nbt::Int(scaled as i32),
        "long" => Nbt::Long(scaled as i64),
        "float" => Nbt::Float(scaled as f32),
        "double" => Nbt::Double(scaled),
        other => return Err(format!("Unknown data type: {other}")),
    };
    let steps = parse_path_arg(path)?;
    let old = read(game, target)?;
    let mut new = old.clone();
    nbt::set(&mut new, &steps, &nbt_value);
    if new != old {
        write(game, target, &old, &new)?;
    }
    Ok(())
}

pub(super) fn data_tab(game: &Game, before: &[&str]) -> Vec<String> {
    let pick = |o: &[&str]| o.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>();
    match (before.first().copied(), before.len()) {
        (_, 0) => pick(&["get", "merge", "modify", "remove"]),
        (_, 1) => pick(&["entity", "block", "storage"]),
        (_, 2) if before[1] == "entity" => game.options(Arg::Entities),
        (_, 2) if before[1] == "storage" => game
            .storage
            .keys()
            .map(|k| format!("minecraft:{k}"))
            .collect(),
        (Some("modify"), n) if n >= 3 => {
            let tail = &before[3..];
            match tail.len() {
                0 | 1 => Vec::new(),
                _ if before.len() > 3 && tail.len() == 2 => {
                    pick(&["set", "merge", "append", "prepend", "insert"])
                }
                _ if tail.len() == 3 => pick(&["from", "value", "string"]),
                _ => Vec::new(),
            }
        }
        _ => Vec::new(),
    }
}

// ---- /item ----

/// An inventory slot named as vanilla does: `hotbar.0`, `container.5`,
/// `armor.head`, `weapon.mainhand`, `inventory.3`.
fn player_slot(name: &str, selected: usize) -> Result<usize, String> {
    let n = |s: &str, max: usize| -> Result<usize, String> {
        let v: usize = s.parse().map_err(|_| format!("Unknown slot '{name}'"))?;
        (v < max)
            .then_some(v)
            .ok_or_else(|| format!("Unknown slot '{name}'"))
    };
    match name.split_once('.') {
        Some(("hotbar", i)) => n(i, 9),
        Some(("inventory", i)) => Ok(9 + n(i, 27)?),
        Some(("container", i)) => {
            let c = n(i, 54)?;
            // Container 0–8 is the hotbar, 9–35 the main inventory.
            if c < 36 {
                Ok(c)
            } else {
                Err(format!("Unknown slot '{name}'"))
            }
        }
        Some(("armor", "head")) => Ok(slots::ARMOR_HEAD),
        Some(("armor", "chest")) => Ok(slots::ARMOR_CHEST),
        Some(("armor", "legs")) => Ok(slots::ARMOR_LEGS),
        Some(("armor", "feet")) => Ok(slots::ARMOR_FEET),
        Some(("weapon", "mainhand")) => Ok(selected),
        Some(("weapon", "offhand")) => Ok(slots::OFFHAND),
        None if name == "weapon" => Ok(selected),
        _ => Err(format!("Unknown slot '{name}'")),
    }
}

pub(super) fn slot_names(_: &Game) -> Vec<String> {
    let mut v: Vec<String> = (0..9).map(|i| format!("hotbar.{i}")).collect();
    v.extend((0..27).map(|i| format!("inventory.{i}")));
    v.extend((0..36).map(|i| format!("container.{i}")));
    v.extend(
        [
            "armor.head",
            "armor.chest",
            "armor.legs",
            "armor.feet",
            "weapon",
            "weapon.mainhand",
            "weapon.offhand",
        ]
        .map(str::to_owned),
    );
    v
}

/// What a slot of a target holds.
fn slot_get(game: &mut Game, target: &DataTarget, slot: &str) -> Result<Option<ItemStack>, String> {
    match target {
        DataTarget::Entity(Target::Player(i)) => {
            let sel = game.player_at(*i).inventory.selected;
            let s = player_slot(slot, sel)?;
            Ok(game.player_at(*i).inventory.slot(s).cloned())
        }
        DataTarget::Block(at) => {
            let n: usize = slot
                .strip_prefix("container.")
                .and_then(|s| s.parse().ok())
                .ok_or_else(|| format!("Unknown slot '{slot}'"))?;
            match game.chunks.block_entity(*at) {
                Some(BlockEntity::Chest(s) | BlockEntity::Dispenser(s)) => s
                    .get(n)
                    .cloned()
                    .ok_or_else(|| format!("Unknown slot '{slot}'")),
                Some(BlockEntity::Hopper(h)) => h
                    .slots
                    .get(n)
                    .cloned()
                    .ok_or_else(|| format!("Unknown slot '{slot}'")),
                _ => Err("That block has no inventory".to_owned()),
            }
        }
        DataTarget::Entity(Target::Mob(_)) => {
            Err("The game's mobs hold nothing in slots".to_owned())
        }
        DataTarget::Storage(_) => Err("Storage has no slots".to_owned()),
    }
}

fn slot_set(
    game: &mut Game,
    target: &DataTarget,
    slot: &str,
    stack: Option<ItemStack>,
) -> Result<(), String> {
    match target {
        DataTarget::Entity(Target::Player(i)) => {
            let sel = game.player_at(*i).inventory.selected;
            let s = player_slot(slot, sel)?;
            game.with_player(*i, |g| g.player.inventory.set(s, stack));
            Ok(())
        }
        DataTarget::Block(at) => {
            let n: usize = slot
                .strip_prefix("container.")
                .and_then(|s| s.parse().ok())
                .ok_or_else(|| format!("Unknown slot '{slot}'"))?;
            match game.chunks.block_entity_mut(*at) {
                Some(BlockEntity::Chest(s) | BlockEntity::Dispenser(s)) if n < s.len() => {
                    s[n] = stack;
                    Ok(())
                }
                Some(BlockEntity::Hopper(h)) if n < h.slots.len() => {
                    h.slots[n] = stack;
                    Ok(())
                }
                Some(
                    BlockEntity::Chest(_) | BlockEntity::Dispenser(_) | BlockEntity::Hopper(_),
                ) => Err(format!("Unknown slot '{slot}'")),
                _ => Err("That block has no inventory".to_owned()),
            }
        }
        DataTarget::Entity(Target::Mob(_)) => {
            Err("The game's mobs hold nothing in slots".to_owned())
        }
        DataTarget::Storage(_) => Err("Storage has no slots".to_owned()),
    }
}

/// An item modifier: `{"function":"minecraft:set_count","count":5}` (or a list).
fn apply_modifier(
    stack: &mut ItemStack,
    json: &str,
    game: &mut Game,
    caller: &Caller,
) -> Result<(), String> {
    let _ = (game, caller);
    let value: serde_json::Value =
        serde_json::from_str(json).map_err(|e| format!("Invalid item modifier: {e}"))?;
    let list = match value {
        serde_json::Value::Array(a) => a,
        other => vec![other],
    };
    for m in list {
        let function = m
            .get("function")
            .and_then(|f| f.as_str())
            .ok_or("A modifier needs a function")?;
        match function.strip_prefix("minecraft:").unwrap_or(function) {
            "set_count" => {
                let n = m
                    .get("count")
                    .and_then(|c| c.as_i64())
                    .ok_or("set_count needs a count")?;
                let add = m.get("add").and_then(|a| a.as_bool()).unwrap_or(false);
                let n = if add { i64::from(stack.count) + n } else { n };
                stack.count = n.clamp(1, 99) as u8;
            }
            "set_damage" => {
                let d = m
                    .get("damage")
                    .and_then(|c| c.as_i64())
                    .ok_or("set_damage needs a damage")?;
                stack.damage = d.max(0) as u16;
            }
            "set_name" => {
                let name = m
                    .get("name")
                    .map(|n| components::plain_text(&n.to_string()));
                stack.set_custom_name(name);
            }
            "set_enchantments" => {
                let map = m
                    .get("enchantments")
                    .and_then(|e| e.as_object())
                    .ok_or("set_enchantments needs enchantments")?;
                for (name, level) in map {
                    let name = name.strip_prefix("minecraft:").unwrap_or(name);
                    let e = crate::inventory::enchant::ALL
                        .iter()
                        .copied()
                        .find(|e| enchantment_id(*e) == name)
                        .ok_or_else(|| format!("Unknown enchantment: {name}"))?;
                    stack.enchant(e, level.as_u64().unwrap_or(1) as u8);
                }
            }
            "clear_enchantments" => stack.clear_enchantments(),
            other => return Err(format!("Unknown item modifier function '{other}'")),
        }
    }
    Ok(())
}

pub(super) fn item(game: &mut Game, caller: &Caller, args: &[&str]) -> Result<Vec<Reply>, String> {
    let usage = "Usage: /item (replace|modify) (entity|block) <target> <slot> ...";
    let action = args.first().copied().ok_or(usage)?;
    let (target, used) = parse_target(game, caller, &args[1..])?;
    let slot = args.get(1 + used).ok_or("Expected a slot")?;
    let at = 2 + used;
    let (who, _) = describe(game, &target);
    match action {
        "replace" => {
            let stack = match args.get(at).copied() {
                Some("with") => {
                    let (_, mut stack) = components::item_with_components(
                        args.get(at + 1).ok_or("Expected an item")?,
                    )?;
                    let count: u8 = args
                        .get(at + 2)
                        .map(|c| number(c, "count"))
                        .transpose()?
                        .unwrap_or(1);
                    stack.count = count.clamp(1, 99);
                    Some(stack)
                }
                Some("from") => {
                    let (src, n) = parse_target(game, caller, &args[at + 1..])?;
                    let src_slot = args.get(at + 1 + n).ok_or("Expected a source slot")?;
                    let mut stack = slot_get(game, &src, src_slot)?;
                    if let (Some(s), Some(_)) = (stack.as_mut(), args.get(at + 2 + n)) {
                        apply_modifier(s, &rest(args, at + 2 + n), game, caller)?;
                    }
                    stack
                }
                _ => return Err("Expected with or from".to_owned()),
            };
            slot_set(game, &target, slot, stack.clone())?;
            caller.report(1);
            Ok(tell(match stack {
                Some(s) => format!("Replaced a slot on {who} with [{}]", items_display(s.id)),
                None => format!("Cleared a slot on {who}"),
            }))
        }
        "modify" => {
            let mut stack = slot_get(game, &target, slot)?.ok_or("There's no item in that slot")?;
            apply_modifier(&mut stack, &rest(args, at), game, caller)?;
            slot_set(game, &target, slot, Some(stack.clone()))?;
            caller.report(1);
            Ok(tell(format!(
                "Modified item on {who}: [{}]",
                items_display(stack.id)
            )))
        }
        _ => Err(usage.to_owned()),
    }
}

pub(super) fn item_tab(game: &Game, before: &[&str]) -> Vec<String> {
    let pick = |o: &[&str]| o.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>();
    match before.len() {
        0 => pick(&["replace", "modify"]),
        1 => pick(&["entity", "block"]),
        2 if before[1] == "entity" => game.options(Arg::Players),
        2 => vec!["~".to_owned()],
        n if (before[1] == "entity" && n == 3) || (before[1] == "block" && n == 5) => {
            slot_names(game)
        }
        n if (before[1] == "entity" && n == 4 && before[0] == "replace")
            || (before[1] == "block" && n == 6 && before[0] == "replace") =>
        {
            pick(&["with", "from"])
        }
        n if (before[1] == "entity" && n == 5 && before.get(4) == Some(&"with"))
            || (before[1] == "block" && n == 7 && before.get(6) == Some(&"with")) =>
        {
            game.options(Arg::Item)
        }
        _ => Vec::new(),
    }
}

// ---- /loot ----

fn roll_source(
    game: &mut Game,
    caller: &Caller,
    args: &[&str],
) -> Result<(Vec<ItemStack>, usize), String> {
    let kind = args
        .first()
        .copied()
        .ok_or("Expected fish, loot, kill or mine")?;
    match kind {
        "loot" => {
            let table = args.get(1).ok_or("Expected a loot table")?;
            let name = table.strip_prefix("minecraft:").unwrap_or(table);
            let name = name.strip_prefix("chests/").unwrap_or(name);
            if crate::world::structure::loot::table(name).is_none() {
                return Err(format!("Unknown loot table '{table}'"));
            }
            let rng = &mut game.rng;
            let stacks: Vec<ItemStack> = crate::world::structure::loot::fill(name, rng)
                .into_iter()
                .flatten()
                .collect();
            Ok((stacks, 2))
        }
        "fish" => {
            let table = args.get(1).ok_or("Expected a loot table")?;
            let name = table.strip_prefix("minecraft:").unwrap_or(table);
            if !name.starts_with("gameplay/fishing") {
                return Err(format!("Unknown loot table '{table}'"));
            }
            let pos_words = args.get(2..5).ok_or("Expected a position")?;
            let at = block_position(pos_words, caller.pos)?;
            let open_water = (-2..=2).all(|dx: i32| {
                (-2..=2).all(|dz: i32| {
                    game.chunks
                        .world()
                        .block(at.x + dx, at.y, at.z + dz)
                        .is_some_and(|b| b.def().name == "water")
                })
            });
            let rng = &mut game.rng;
            let (_, stack) = crate::entity::bobber::roll_catch(0, open_water, rng);
            // A tool word (`mainhand`, an item) may follow; it is accepted.
            let used = 5 + usize::from(args.get(5).is_some_and(|w| *w != "kill"));
            Ok((stack.into_iter().collect(), used))
        }
        "kill" => {
            let word = args.get(1).ok_or("Expected an entity")?;
            let found = game.targets(caller, word)?;
            let [Target::Mob(id)] = found[..] else {
                return Err(
                    "Only one mob is allowed, but the provided selector allows more than one"
                        .to_owned(),
                );
            };
            let mob = game
                .mobs
                .iter()
                .find(|m| m.id == id)
                .cloned()
                .ok_or("No entity was found")?;
            let def = game.mob_defs.get(mob.kind);
            let rng = &mut game.rng;
            let mut random = || rng.next_f64() as f32;
            let stacks = mob.drops(def, 0, &mut random);
            Ok((stacks, 2))
        }
        "mine" => {
            let words = args.get(1..4).ok_or("Expected a position")?;
            let at = block_position(words, caller.pos)?;
            let state = game
                .chunks
                .world()
                .block(at.x, at.y, at.z)
                .ok_or("That position is not loaded")?;
            let (tool, fortune, silk, used) = match args.get(4).copied() {
                Some("mainhand") | Some("offhand") => {
                    let stack = game
                        .player_at(caller.index)
                        .inventory
                        .selected_stack()
                        .cloned();
                    let kind = stack
                        .as_ref()
                        .and_then(|s| game.items().tool(s.id))
                        .cloned();
                    let fortune = stack.as_ref().map_or(0, |s| {
                        s.level_of(crate::inventory::enchant::Enchantment::Fortune)
                    });
                    let silk = stack.as_ref().is_some_and(|s| {
                        s.level_of(crate::inventory::enchant::Enchantment::SilkTouch) > 0
                    });
                    (kind, fortune, silk, 5)
                }
                Some(word) if !matches!(word, "replace" | "give" | "insert" | "spawn") => {
                    let (id, stack) = components::item_with_components(word)?;
                    let kind = game.items().tool(id).cloned();
                    (
                        kind,
                        stack.level_of(crate::inventory::enchant::Enchantment::Fortune),
                        stack.level_of(crate::inventory::enchant::Enchantment::SilkTouch) > 0,
                        5,
                    )
                }
                _ => (None, 0, false, 4),
            };
            if !crate::world::harvest::can_harvest(state.def(), tool.as_ref()) {
                return Ok((Vec::new(), used));
            }
            let rng = &mut game.rng;
            let mut random = || rng.next_f64() as f32;
            Ok((
                crate::world::harvest::drops_with(
                    state,
                    tool.map(|t| t.kind),
                    fortune,
                    silk,
                    &mut random,
                ),
                used,
            ))
        }
        other => Err(format!("Unknown loot source: {other}")),
    }
}

pub(super) fn loot(game: &mut Game, caller: &Caller, args: &[&str]) -> Result<Vec<Reply>, String> {
    let usage = "Usage: /loot <spawn|give|insert|replace> ... <fish|loot|kill|mine> ...";
    let (target_kind, mut at) = (args.first().copied().ok_or(usage)?, 1usize);
    enum Into {
        Spawn(DVec3),
        Give(Vec<usize>),
        Insert(IVec3),
        Replace(DataTarget, String, usize),
    }
    let into = match target_kind {
        "spawn" => {
            let words = args.get(at..at + 3).ok_or("Expected a position")?;
            at += 3;
            Into::Spawn(position(words, caller.pos)?)
        }
        "give" => {
            let who = args.get(at).ok_or("Expected players")?;
            at += 1;
            Into::Give(game.players_named(caller, who)?)
        }
        "insert" => {
            let words = args.get(at..at + 3).ok_or("Expected a position")?;
            at += 3;
            Into::Insert(block_position(words, caller.pos)?)
        }
        "replace" => {
            let (target, used) = parse_target(game, caller, &args[at..])?;
            at += used;
            let slot = args.get(at).ok_or("Expected a slot")?.to_string();
            at += 1;
            let count: usize = match args.get(at).and_then(|w| w.parse().ok()) {
                Some(c) => {
                    at += 1;
                    c
                }
                None => usize::MAX,
            };
            Into::Replace(target, slot, count)
        }
        _ => return Err(usage.to_owned()),
    };
    let (stacks, _) = roll_source(game, caller, &args[at..])?;
    let n: u32 = stacks.iter().map(|s| u32::from(s.count)).sum();
    let items_text = if let [one] = &stacks[..] {
        format!("{} [{}]", one.count, items_display(one.id))
    } else {
        format!("{n} items")
    };
    let reply = match into {
        Into::Spawn(at) => {
            let cell = at.floor().as_ivec3();
            game.enter_realm(caller.dim);
            for stack in stacks {
                let rng = &mut game.rng;
                let mut random = || rng.next_f64() as f32;
                game.entities
                    .push(crate::entity::item::ItemEntity::from_block(
                        stack,
                        cell,
                        &mut random,
                    ));
            }
            format!("Dropped {items_text}")
        }
        Into::Give(players) => {
            let items = game.items().clone();
            for &p in &players {
                for stack in &stacks {
                    let stack = stack.clone();
                    game.with_player(p, |g| {
                        if let Some(rest) = g.player.inventory.add(stack, &items) {
                            g.throw(rest);
                        }
                    });
                }
            }
            let who = if players.len() == 1 {
                game.player_at(players[0]).name.clone()
            } else {
                format!("{} players", players.len())
            };
            format!("Gave {items_text} to {who}")
        }
        Into::Insert(at) => {
            let Some(entity) = game.chunks.block_entity_mut(at) else {
                return Err("The target block is not a container".to_owned());
            };
            let slots: &mut Vec<Option<ItemStack>> = match entity {
                BlockEntity::Chest(s) | BlockEntity::Dispenser(s) => s,
                BlockEntity::Hopper(h) => &mut h.slots,
                _ => return Err("The target block is not a container".to_owned()),
            };
            let mut left = 0;
            for stack in stacks {
                match slots.iter_mut().find(|s| s.is_none()) {
                    Some(free) => *free = Some(stack),
                    None => left += 1,
                }
            }
            if left > 0 {
                return Err("The container is full".to_owned());
            }
            format!("Inserted {items_text} into {}, {}, {}", at.x, at.y, at.z)
        }
        Into::Replace(target, slot, count) => {
            let (who, _) = describe(game, &target);
            let mut placed = 0;
            let base = slot.clone();
            // Successive stacks go into successive slots (`container.3`, `.4`...).
            for (i, stack) in stacks.into_iter().take(count).enumerate() {
                let name = match base.rsplit_once('.') {
                    Some((head, tail)) if tail.parse::<usize>().is_ok() => {
                        format!("{head}.{}", tail.parse::<usize>().unwrap() + i)
                    }
                    _ => base.clone(),
                };
                slot_set(game, &target, &name, Some(stack))?;
                placed += 1;
            }
            format!("Replaced {placed} slot(s) on {who} with {items_text}")
        }
    };
    caller.report(n as i32);
    Ok(tell(reply))
}

pub(super) fn loot_tab(game: &Game, before: &[&str]) -> Vec<String> {
    let pick = |o: &[&str]| o.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>();
    match before.len() {
        0 => pick(&["spawn", "give", "insert", "replace"]),
        1 if before[0] == "give" => game.options(Arg::Players),
        _ => {
            if before.contains(&"loot") && before.last() == Some(&"loot") {
                [
                    "chests/simple_dungeon",
                    "chests/abandoned_mineshaft",
                    "chests/desert_pyramid",
                    "chests/jungle_temple",
                    "chests/stronghold_library",
                    "chests/shipwreck_treasure",
                    "chests/ruined_portal",
                    "chests/village_weaponsmith",
                    "chests/end_city_treasure",
                    "chests/bastion_treasure",
                    "gameplay/fishing",
                ]
                .iter()
                .map(|n| format!("minecraft:{n}"))
                .collect()
            } else {
                pick(&["fish", "loot", "kill", "mine"])
            }
        }
    }
}

/// `/tellraw`-style helper kept for `/execute`: renders JSON text for a title.
#[allow(dead_code)]
pub(super) fn json_text(game: &mut Game, caller: &Caller, json: &str) -> Result<String, String> {
    render_json(game, caller, json).map(|r| r.text)
}

/// How many items in the slots of `target` that `pattern` names (`hotbar.*`,
/// `container.3`, `armor.*`) are `wanted` (`*`, an item id, or `#tag`-less
/// id with components ignored).
pub(super) fn count_items(
    game: &mut Game,
    target: &DataTarget,
    pattern: &str,
    wanted: &str,
) -> Result<i32, String> {
    let want_id = if wanted == "*" {
        None
    } else {
        Some(item_named(wanted.split('[').next().unwrap_or(wanted))?)
    };
    let names: Vec<String> = match target {
        DataTarget::Block(_) => (0..54).map(|i| format!("container.{i}")).collect(),
        _ => slot_names(game),
    };
    let matching: Vec<String> = if let Some(prefix) = pattern.strip_suffix(".*") {
        names
            .into_iter()
            .filter(|n| n.starts_with(&format!("{prefix}.")))
            .collect()
    } else {
        vec![pattern.to_owned()]
    };
    let mut total = 0;
    for slot in matching {
        if let Ok(Some(stack)) = slot_get(game, target, &slot)
            && want_id.is_none_or(|id| id == stack.id)
        {
            total += i32::from(stack.count);
        }
    }
    Ok(total)
}
