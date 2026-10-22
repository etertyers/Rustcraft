//! Placeholder blocks and items, generated from vanilla's own data.
//!
//! Every block in vanilla's `blockstates/` and every item in `items/` that
//! `blocks.ron` and `items.ron` don't register gets an entry in
//! `assets/data/placeholder_blocks.ron` or `placeholder_items.ron`, with its
//! name from `lang/en_us.json` and its model and textures from the model
//! JSON, and a line in `docs/placeholders.md`. Placeholders can be placed,
//! broken and dropped, and do nothing else; later versions give them real
//! behavior. `cargo run -- --gen-placeholders` rewrites the three files
//! (it needs `assets/vanilla/`, see `scripts/fetch-vanilla-assets.sh`); the
//! files are committed, so the game and its tests don't need vanilla.

use std::{
    collections::{BTreeMap, BTreeSet},
    fmt::Write as _,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, bail};
use serde::Deserialize;
use serde_json::Value;

/// Technical blocks and items, and what is out of scope for v1.1: command
/// and structure blocks, jigsaws, barriers and light blocks, test blocks,
/// the debug stick and knowledge book, and anything for the Wither or ocean
/// monuments (CLAUDE.md §1.1).
pub const SKIPPED: &[&str] = &[
    "command_block",
    "chain_command_block",
    "repeating_command_block",
    "command_block_minecart",
    "structure_block",
    "structure_void",
    "jigsaw",
    "barrier",
    "light",
    "debug_stick",
    "test_block",
    "test_instance_block",
    "knowledge_book",
    // The Wither.
    "wither_spawn_egg",
    "wither_rose",
    "nether_star",
    // Ocean monuments.
    "ocean_monument_map",
    // Air that isn't air.
    "cave_air",
    "void_air",
];

/// Whether a placeholder with this name is left out: the technical and
/// out-of-scope names, and spawn eggs for mobs Rustcraft doesn't have.
pub fn skipped(name: &str, mobs: &BTreeSet<String>) -> bool {
    if SKIPPED.contains(&name) {
        return true;
    }
    name.strip_suffix("_spawn_egg")
        .is_some_and(|mob| !mobs.contains(mob))
}

/// Most of an item in a stack: 1 for tools, armor, buckets of something,
/// boats, minecarts and music discs, else 64.
pub fn max_stack(name: &str) -> u8 {
    const ONE: &[&str] = &[
        "_sword",
        "_pickaxe",
        "_axe",
        "_shovel",
        "_hoe",
        "_spear",
        "_helmet",
        "_chestplate",
        "_leggings",
        "_boots",
        "_bucket",
        "_boat",
        "_raft",
        "_minecart",
    ];
    if name == "bucket" || name.starts_with("music_disc_") || ONE.iter().any(|s| name.ends_with(s))
    {
        1
    } else {
        64
    }
}

/// A block or item already in `blocks.ron` or `items.ron`.
#[derive(Deserialize)]
struct Named {
    name: String,
    #[serde(default = "yes")]
    item: bool,
}

fn yes() -> bool {
    true
}

fn names(text: &str) -> Result<Vec<Named>> {
    Ok(ron::from_str(text)?)
}

/// The faces of a model element or block, in the order
/// top, bottom, north, south, east, west.
const FACES: [&str; 6] = ["up", "down", "north", "south", "east", "west"];
const FIELDS: [&str; 6] = ["top", "bottom", "north", "south", "east", "west"];

/// One box of a model.
#[derive(Debug, Clone, PartialEq)]
struct Part {
    from: [f32; 3],
    to: [f32; 3],
    /// Texture locations (`block/stone`) by face, in `FIELDS` order.
    faces: [Option<String>; 6],
}

impl Part {
    fn is_full(&self) -> bool {
        self.from == [0.0; 3] && self.to == [16.0; 3]
    }

    /// Faces the element leaves out show the texture of a face it has
    /// (they are mostly hidden against another block in vanilla).
    fn filled(&self) -> [String; 6] {
        let any = self.faces.iter().flatten().next().cloned();
        std::array::from_fn(|i| {
            self.faces[i]
                .clone()
                .or_else(|| any.clone())
                .unwrap_or_default()
        })
    }
}

/// A model with its parents' textures and elements folded in.
#[derive(Debug, Default)]
struct Model {
    /// The model and its parents, nearest first.
    chain: Vec<String>,
    textures: BTreeMap<String, String>,
    parts: Vec<Part>,
}

impl Model {
    fn has_parent(&self, name: &str) -> bool {
        self.chain.iter().any(|n| n == name)
    }

    /// A texture variable (`all`) as a location (`block/stone`).
    fn texture(&self, var: &str) -> Option<String> {
        self.resolve(self.textures.get(var)?)
    }

    fn resolve(&self, value: &str) -> Option<String> {
        let mut value = value;
        for _ in 0..16 {
            // Newer files may leave out the `#` of a variable.
            let var = value.strip_prefix('#').or_else(|| {
                (!value.contains(['/', ':']) && self.textures.contains_key(value)).then_some(value)
            });
            match var {
                Some(var) => value = self.textures.get(var)?,
                None => return Some(strip(value).to_owned()),
            }
        }
        None
    }

    /// Any texture the model names, the particle one first.
    fn any_texture(&self) -> Option<String> {
        self.texture("particle")
            .or_else(|| self.textures.keys().find_map(|var| self.texture(var)))
    }
}

fn strip(name: &str) -> &str {
    name.strip_prefix("minecraft:").unwrap_or(name)
}

/// Vanilla's data, read from `assets/vanilla/assets/minecraft`.
pub struct Vanilla {
    root: PathBuf,
    lang: BTreeMap<String, String>,
}

impl Vanilla {
    pub fn open(assets: &Path) -> Result<Self> {
        let root = assets.join("vanilla/assets/minecraft");
        if !root.is_dir() {
            bail!(
                "no vanilla assets in {}: run scripts/fetch-vanilla-assets.sh",
                root.display()
            );
        }
        let path = root.join("lang/en_us.json");
        let lang = serde_json::from_str(
            &std::fs::read_to_string(&path)
                .with_context(|| format!("reading {}", path.display()))?,
        )
        .with_context(|| format!("parsing {}", path.display()))?;
        Ok(Self { root, lang })
    }

    fn json(&self, path: &str) -> Option<Value> {
        let text = std::fs::read_to_string(self.root.join(path)).ok()?;
        crate::pack::parse_json(&text).ok()
    }

    fn has_texture(&self, location: &str) -> bool {
        self.root.join(format!("textures/{location}.png")).is_file()
    }

    /// The block's name in English, from the language file.
    fn display(&self, name: &str) -> String {
        for kind in ["block", "item"] {
            if let Some(text) = self.lang.get(&format!("{kind}.minecraft.{name}")) {
                // A music disc is told apart by its track, as in vanilla's lore.
                if let Some(track) = self.lang.get(&format!("{kind}.minecraft.{name}.desc")) {
                    return format!("{text} ({track})");
                }
                return text.clone();
            }
        }
        name.split('_')
            .map(|word| {
                let mut chars = word.chars();
                chars
                    .next()
                    .map(|c| c.to_ascii_uppercase().to_string() + chars.as_str())
                    .unwrap_or_default()
            })
            .collect::<Vec<_>>()
            .join(" ")
    }

    fn model(&self, name: &str) -> Model {
        let mut model = Model::default();
        let mut next = Some(strip(name).to_owned());
        for _ in 0..16 {
            let Some(name) = next.take() else { break };
            model.chain.push(name.clone());
            let Some(json) = self.json(&format!("models/{name}.json")) else {
                break;
            };
            if let Some(textures) = json.get("textures").and_then(Value::as_object) {
                for (var, value) in textures {
                    // Newer files give a texture as {"sprite": ...}.
                    let value = value.as_str().or_else(|| value.get("sprite")?.as_str());
                    if let Some(value) = value {
                        model
                            .textures
                            .entry(var.clone())
                            .or_insert_with(|| value.to_owned());
                    }
                }
            }
            if model.parts.is_empty()
                && let Some(elements) = json.get("elements").and_then(Value::as_array)
            {
                model.parts = elements.iter().filter_map(|e| element(e, &model)).collect();
            }
            next = json
                .get("parent")
                .and_then(Value::as_str)
                .map(|p| strip(p).to_owned());
        }
        // Textures named by the elements may come from parents read later.
        let parts = std::mem::take(&mut model.parts);
        model.parts = parts
            .into_iter()
            .map(|mut part| {
                for face in part.faces.iter_mut() {
                    if let Some(var) = face.take() {
                        *face = model.resolve(&var);
                    }
                }
                part
            })
            .collect();
        model
    }

    /// The model a blockstates file shows for the block's default state
    /// (or all the parts that always show, for multipart ones).
    fn default_models(&self, name: &str) -> Vec<String> {
        let Some(json) = self.json(&format!("blockstates/{name}.json")) else {
            return Vec::new();
        };
        let model_of = |apply: &Value| -> Option<String> {
            let apply = apply.as_array().and_then(|a| a.first()).unwrap_or(apply);
            apply.get("model")?.as_str().map(str::to_owned)
        };
        if let Some(variants) = json.get("variants").and_then(Value::as_object) {
            let pick = variants.get("").or_else(|| variants.values().next());
            return pick.and_then(model_of).into_iter().collect();
        }
        let Some(parts) = json.get("multipart").and_then(Value::as_array) else {
            return Vec::new();
        };
        let mut always: Vec<String> = parts
            .iter()
            .filter(|p| p.get("when").is_none())
            .filter_map(|p| model_of(p.get("apply")?))
            .collect();
        if always.is_empty() {
            always.extend(parts.first().and_then(|p| model_of(p.get("apply")?)));
        }
        always
    }

    /// The model name an item's definition shows.
    fn item_model(&self, name: &str) -> Option<String> {
        fn find(value: &Value) -> Option<String> {
            match value {
                Value::Object(map) => {
                    for key in ["model", "base"] {
                        if let Some(Value::String(s)) = map.get(key) {
                            return Some(s.clone());
                        }
                    }
                    map.values().find_map(find)
                }
                Value::Array(list) => list.iter().find_map(find),
                _ => None,
            }
        }
        find(&self.json(&format!("items/{name}.json"))?)
    }

    /// The flat sprite an item shows (`item/stick`, or another folder), or
    /// `None` for an item drawn as a block.
    fn item_sprite(&self, name: &str) -> Option<String> {
        let model = self.model(
            &self
                .item_model(name)
                .unwrap_or_else(|| format!("item/{name}")),
        );
        if model.chain.iter().any(|n| n.starts_with("block/"))
            && !model.has_parent("item/generated")
        {
            return None;
        }
        let sprite = model
            .texture("layer0")
            .or_else(|| model.any_texture())
            .filter(|t| self.has_texture(t));
        sprite.or_else(|| {
            [format!("item/{name}"), format!("block/{name}")]
                .into_iter()
                .find(|t| self.has_texture(t))
        })
    }

    /// Whether any pixel of the textures is see-through: 1 for some fully
    /// transparent ones (leaves, glass), 2 for some partly (stained glass).
    fn transparency(&self, locations: &BTreeSet<String>) -> u8 {
        let mut level = 0;
        for location in locations {
            let path = self.root.join(format!("textures/{location}.png"));
            let Ok(image) = image::open(&path) else {
                continue;
            };
            for pixel in image.to_rgba8().pixels() {
                match pixel.0[3] {
                    255 => {}
                    0 => level = level.max(1),
                    _ => return 2,
                }
            }
        }
        level
    }
}

fn element(json: &Value, model: &Model) -> Option<Part> {
    let corner = |key: &str| -> Option<[f32; 3]> {
        let list = json.get(key)?.as_array()?;
        let at = |i: usize| {
            list.get(i)
                .and_then(Value::as_f64)
                .map(|v| (v as f32).clamp(0.0, 16.0))
        };
        Some([at(0)?, at(1)?, at(2)?])
    };
    let faces = json.get("faces")?;
    let _ = model;
    Some(Part {
        from: corner("from")?,
        to: corner("to")?,
        faces: std::array::from_fn(|i| {
            faces
                .get(FACES[i])
                .and_then(|f| f.get("texture"))
                .and_then(Value::as_str)
                .map(str::to_owned)
        }),
    })
}

/// How a placeholder block is drawn.
#[derive(Debug, PartialEq)]
enum Look {
    /// A full block with a texture per face.
    Cube([String; 6]),
    /// Two crossed planes, walked through.
    Cross(String),
    /// Boxes from the model's elements.
    Model(Vec<Part>),
}

impl Look {
    fn textures(&self) -> BTreeSet<String> {
        match self {
            Look::Cube(faces) => faces.iter().cloned().collect(),
            Look::Cross(texture) => BTreeSet::from([texture.clone()]),
            Look::Model(parts) => parts.iter().flat_map(|p| p.filled()).collect(),
        }
    }

    fn kind(&self) -> &'static str {
        match self {
            Look::Cube(_) => "Full blocks",
            Look::Cross(_) => "Plants (walk-through)",
            Look::Model(_) => "Modeled blocks",
        }
    }
}

/// How a block's default model is drawn: a full block for `cube_all`,
/// `cube_column`, `cube_bottom_top`, `orientable` (and a single full
/// element), a plant for `cross`, else its elements.
fn classify(vanilla: &Vanilla, models: &[String]) -> Option<Look> {
    if let [one] = models {
        let model = vanilla.model(one);
        let var = |name: &str| model.texture(name);
        let cube = |faces: [Option<String>; 6]| -> Option<Look> {
            Some(Look::Cube(
                faces
                    .map(|f| f)
                    .into_iter()
                    .collect::<Option<Vec<_>>>()?
                    .try_into()
                    .ok()?,
            ))
        };
        if model.has_parent("block/cube_all") {
            let all = var("all");
            return cube(std::array::from_fn(|_| all.clone()));
        }
        if model.has_parent("block/cube_column") || model.has_parent("block/cube_column_horizontal")
        {
            let (end, side) = (var("end"), var("side"));
            return cube([
                end.clone(),
                end,
                side.clone(),
                side.clone(),
                side.clone(),
                side,
            ]);
        }
        if model.has_parent("block/cube_bottom_top") {
            let side = var("side");
            return cube([
                var("top"),
                var("bottom"),
                side.clone(),
                side.clone(),
                side.clone(),
                side,
            ]);
        }
        if model.has_parent("block/orientable") || model.has_parent("block/orientable_with_bottom")
        {
            let (side, top) = (var("side"), var("top"));
            return cube([
                top.clone(),
                var("bottom").or(top),
                side.clone(),
                var("front").or(side.clone()),
                side.clone(),
                side,
            ]);
        }
        if model.has_parent("block/cross") || model.has_parent("block/tinted_cross") {
            return var("cross").map(Look::Cross);
        }
        if let [part] = model.parts.as_slice()
            && part.is_full()
        {
            let faces = part.filled();
            if faces.iter().all(|f| !f.is_empty()) {
                return Some(Look::Cube(faces));
            }
        }
    }
    let mut parts = Vec::new();
    let mut fallback = None;
    for name in models {
        let model = vanilla.model(name);
        parts.extend(model.parts.iter().cloned());
        fallback = fallback.or_else(|| model.any_texture());
    }
    parts.retain(|p| p.faces.iter().any(Option::is_some));
    if parts.is_empty() {
        // Drawn by the game as an entity (beds, signs, banners, chests,
        // heads, shulker boxes): a full block wearing its particle texture.
        let texture = fallback?;
        return Some(Look::Cube(std::array::from_fn(|_| texture.clone())));
    }
    Some(Look::Model(parts))
}

/// A texture location as a name in the data files: `block/stone` is
/// `stone`, other folders are kept.
fn short(location: &str) -> String {
    match location.strip_prefix("block/") {
        Some(rest) if !rest.contains('/') => rest.to_owned(),
        _ => location.to_owned(),
    }
}

/// An item texture location as a name in `items.ron`.
fn short_item(location: &str) -> String {
    match location.strip_prefix("item/") {
        Some(rest) if !rest.contains('/') => rest.to_owned(),
        _ => location.to_owned(),
    }
}

/// `ron` text for a string (names are plain ASCII).
fn quote(text: &str) -> String {
    let clean: String = text
        .chars()
        .map(|c| {
            if c.is_ascii() && c != '"' && c != '\\' {
                c
            } else {
                '?'
            }
        })
        .collect();
    format!("\"{clean}\"")
}

/// `(top: "a", ...)` for six faces, collapsing equal ones.
fn texture_fields(faces: &[String; 6]) -> String {
    let first = &faces[0];
    if faces.iter().all(|f| f == first) {
        return format!("(all: {})", quote(&short(first)));
    }
    let parts: Vec<String> = FIELDS
        .iter()
        .zip(faces)
        .map(|(field, f)| format!("{field}: {}", quote(&short(f))))
        .collect();
    format!("({})", parts.join(", "))
}

fn num(v: f32) -> String {
    if v.fract() == 0.0 {
        format!("{v:.0}")
    } else {
        format!("{v}")
    }
}

/// One placeholder block.
#[derive(Debug)]
pub struct BlockEntry {
    pub name: String,
    pub display: String,
    pub has_item: bool,
    pub item_texture: Option<String>,
    look: Look,
    transparency: u8,
}

impl BlockEntry {
    pub fn kind(&self) -> &'static str {
        self.look.kind()
    }

    fn ron(&self) -> String {
        let mut out = format!(
            "    (name: {}, hardness: {}, placeholder: true, display: {}",
            quote(&self.name),
            if matches!(self.look, Look::Cross(_)) {
                "0.0"
            } else {
                "1.0"
            },
            quote(&self.display)
        );
        if !self.has_item {
            out.push_str(", item: false");
        }
        if let Some(texture) = &self.item_texture {
            let _ = write!(out, ", item_texture: {}", quote(&short_item(texture)));
        }
        if max_stack(&self.name) == 1 {
            out.push_str(", stack: 1");
        }
        // Plants are always cutouts; the rest by what their pixels are.
        let transparency = match self.look {
            Look::Cross(_) => self.transparency.max(1),
            _ => self.transparency,
        };
        match transparency {
            1 => out.push_str(", layer: Cutout"),
            2 => out.push_str(", layer: Translucent, cull_same: true"),
            _ => {}
        }
        match &self.look {
            Look::Cube(faces) => {
                let _ = write!(out, ", textures: {}", texture_fields(faces));
            }
            Look::Cross(texture) => {
                let _ = write!(
                    out,
                    ", shape: Cross, solid: false, textures: (all: {})",
                    quote(&short(texture))
                );
            }
            Look::Model(parts) => {
                out.push_str(", shape: Model, model: [");
                for (i, part) in parts.iter().enumerate() {
                    if i > 0 {
                        out.push_str(", ");
                    }
                    let _ = write!(
                        out,
                        "(from: ({}, {}, {}), to: ({}, {}, {}), textures: {})",
                        num(part.from[0]),
                        num(part.from[1]),
                        num(part.from[2]),
                        num(part.to[0]),
                        num(part.to[1]),
                        num(part.to[2]),
                        texture_fields(&part.filled())
                    );
                }
                out.push(']');
            }
        }
        out.push_str("),\n");
        out
    }
}

/// One placeholder item.
#[derive(Debug)]
pub struct ItemEntry {
    pub name: String,
    pub display: String,
    pub texture: String,
}

impl ItemEntry {
    /// The group it is listed under in `docs/placeholders.md`.
    pub fn kind(&self) -> &'static str {
        let n = self.name.as_str();
        if n.ends_with("_spawn_egg") {
            "Spawn eggs"
        } else if max_stack(n) == 1 && n.starts_with("music_disc_") {
            "Music discs"
        } else if max_stack(n) == 1 {
            "Tools, armor, vehicles and buckets"
        } else if n.ends_with("_pottery_sherd") || n.ends_with("_smithing_template") {
            "Pottery sherds and smithing templates"
        } else if n.ends_with("_banner_pattern") || n.ends_with("_map") {
            "Patterns and maps"
        } else {
            "Other items"
        }
    }

    fn ron(&self) -> String {
        format!(
            "    (name: {}, placeholder: true, display: {}, texture: {}, max_stack: {}),\n",
            quote(&self.name),
            quote(&self.display),
            quote(&short_item(&self.texture)),
            max_stack(&self.name)
        )
    }
}

/// What `generate` found.
#[derive(Debug, Default)]
pub struct Generated {
    pub blocks: Vec<BlockEntry>,
    pub items: Vec<ItemEntry>,
    /// Names with no usable model or texture, left out (listed in the log).
    pub unresolved: Vec<String>,
}

/// The unregistered blocks and items of vanilla's data as placeholders.
pub fn collect(assets: &Path) -> Result<Generated> {
    let vanilla = Vanilla::open(assets)?;
    let data = assets.join("data");
    let read = |file: &str| -> Result<String> {
        std::fs::read_to_string(data.join(file)).with_context(|| format!("reading {file}"))
    };
    let blocks = names(&read("blocks.ron")?)?;
    let items = names(&read("items.ron")?)?;
    let mobs: BTreeSet<String> = names(&read("mobs.ron")?)?
        .into_iter()
        .map(|m| m.name)
        .collect();

    let have_block: BTreeSet<&str> = blocks.iter().map(|b| b.name.as_str()).collect();
    let mut have_item: BTreeSet<&str> = blocks
        .iter()
        .filter(|b| b.item)
        .map(|b| b.name.as_str())
        .collect();
    have_item.extend(items.iter().map(|i| i.name.as_str()));

    let listing = |dir: &str| -> Result<BTreeSet<String>> {
        let path = vanilla.root.join(dir);
        let mut names = BTreeSet::new();
        for entry in
            std::fs::read_dir(&path).with_context(|| format!("reading {}", path.display()))?
        {
            let file = entry?.file_name().to_string_lossy().into_owned();
            if let Some(name) = file.strip_suffix(".json")
                && !name.starts_with('_')
            {
                names.insert(name.to_owned());
            }
        }
        Ok(names)
    };
    let vanilla_blocks = listing("blockstates")?;
    let vanilla_items = listing("items")?;

    let mut out = Generated::default();
    let mut block_items = BTreeSet::new();
    for name in &vanilla_blocks {
        if have_block.contains(name.as_str()) || skipped(name, &mobs) {
            continue;
        }
        let models = vanilla.default_models(name);
        let Some(look) = classify(&vanilla, &models) else {
            out.unresolved.push(format!("block {name}: no model"));
            continue;
        };
        if let Some(missing) = look
            .textures()
            .into_iter()
            .find(|t| !vanilla.has_texture(t))
        {
            out.unresolved
                .push(format!("block {name}: no texture {missing}"));
            continue;
        }
        // An item of that name that already exists doesn't place this block.
        let has_item = vanilla_items.contains(name) && !have_item.contains(name.as_str());
        if has_item {
            block_items.insert(name.clone());
        }
        let item_texture = has_item.then(|| vanilla.item_sprite(name)).flatten();
        let transparency = vanilla.transparency(&look.textures());
        out.blocks.push(BlockEntry {
            name: name.clone(),
            display: vanilla.display(name),
            has_item,
            item_texture,
            look,
            transparency,
        });
    }
    for name in &vanilla_items {
        if have_item.contains(name.as_str()) || block_items.contains(name) || skipped(name, &mobs) {
            continue;
        }
        // Items of blocks that are skipped or unresolved aren't placeholders.
        if vanilla_blocks.contains(name) && !have_block.contains(name.as_str()) {
            continue;
        }
        let texture = vanilla.item_sprite(name).filter(|t| vanilla.has_texture(t));
        let Some(texture) = texture else {
            out.unresolved.push(format!("item {name}: no texture"));
            continue;
        };
        out.items.push(ItemEntry {
            name: name.clone(),
            display: vanilla.display(name),
            texture,
        });
    }
    Ok(out)
}

const HEADER_BLOCKS: &str = "\
// Placeholder blocks, generated from vanilla 26.3's blockstates and models by
// `cargo run -- --gen-placeholders` (src/placeholders.rs); don't edit by hand.
// They can be placed, broken and dropped and do nothing else. Same fields as
// blocks.ron, plus placeholder, display (the English name) and model (boxes
// with their own textures). Listed in docs/placeholders.md.
#![enable(implicit_some)]
[
";

const HEADER_ITEMS: &str = "\
// Placeholder items, generated from vanilla 26.3's item definitions by
// `cargo run -- --gen-placeholders` (src/placeholders.rs); don't edit by hand.
// Plain items with their texture; they do nothing when used. Listed in
// docs/placeholders.md.
#![enable(implicit_some)]
[
";

/// The text of `placeholder_blocks.ron`.
pub fn blocks_ron(found: &Generated) -> String {
    let mut text = HEADER_BLOCKS.to_owned();
    for block in &found.blocks {
        text.push_str(&block.ron());
    }
    text.push_str("]\n");
    text
}

/// The text of `placeholder_items.ron`.
pub fn items_ron(found: &Generated) -> String {
    let mut text = HEADER_ITEMS.to_owned();
    for item in &found.items {
        text.push_str(&item.ron());
    }
    text.push_str("]\n");
    text
}

/// The text of `docs/placeholders.md`.
pub fn markdown(found: &Generated) -> String {
    let mut text = String::from(
        "# Placeholder blocks and items\n\n\
         Generated by `cargo run -- --gen-placeholders` from vanilla 26.3's data. Each one has \
         its English name, texture or model and stack size from vanilla, and does nothing \
         beyond being placed, broken and dropped (blocks) or held (items). Later versions \
         give them real behavior; when one gets some, move its entry from \
         `assets/data/placeholder_*.ron` into `blocks.ron` or `items.ron`.\n\n",
    );
    let _ = writeln!(
        text,
        "{} blocks and {} items.\n",
        found.blocks.len(),
        found.items.len()
    );
    let mut groups: BTreeMap<&str, Vec<String>> = BTreeMap::new();
    for block in &found.blocks {
        let note = if block.has_item { "" } else { " (no item)" };
        groups
            .entry(block.kind())
            .or_default()
            .push(format!("`{}`: {}{note}", block.name, block.display));
    }
    for (kind, lines) in &groups {
        let _ = writeln!(text, "## Blocks: {kind} ({})\n", lines.len());
        for line in lines {
            let _ = writeln!(text, "- {line}");
        }
        text.push('\n');
    }
    let mut groups: BTreeMap<&str, Vec<String>> = BTreeMap::new();
    for item in &found.items {
        groups.entry(item.kind()).or_default().push(format!(
            "`{}`: {} (stack {})",
            item.name,
            item.display,
            max_stack(&item.name)
        ));
    }
    for (kind, lines) in &groups {
        let _ = writeln!(text, "## Items: {kind} ({})\n", lines.len());
        for line in lines {
            let _ = writeln!(text, "- {line}");
        }
        text.push('\n');
    }
    text.push_str(
        "## Left out on purpose\n\nCommand, structure and jigsaw blocks, barriers, light blocks, \
         test blocks, the debug stick, the knowledge book, spawn eggs for mobs Rustcraft doesn't \
         have, and anything for the Wither or ocean monuments (`wither_spawn_egg`, \
         `wither_rose`, `nether_star`, `ocean_monument_map`): see `placeholders::SKIPPED`.\n",
    );
    text
}

/// Writes the two data files and `docs/placeholders.md`.
pub fn generate(assets: &Path, docs: &Path) -> Result<Generated> {
    let found = collect(assets)?;
    let data = assets.join("data");
    std::fs::write(data.join("placeholder_blocks.ron"), blocks_ron(&found))?;
    std::fs::write(data.join("placeholder_items.ron"), items_ron(&found))?;
    std::fs::write(docs.join("placeholders.md"), markdown(&found))?;
    Ok(found)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mobs() -> BTreeSet<String> {
        ["pig", "zombie"].map(str::to_owned).into()
    }

    #[test]
    fn technical_and_out_of_scope_names_are_skipped() {
        for name in [
            "command_block",
            "structure_block",
            "jigsaw",
            "barrier",
            "light",
            "structure_void",
            "debug_stick",
            "test_block",
            "knowledge_book",
            "wither_spawn_egg",
            "ocean_monument_map",
            "bat_spawn_egg",
        ] {
            assert!(skipped(name, &mobs()), "{name}");
        }
        assert!(!skipped("pig_spawn_egg", &mobs()));
        assert!(!skipped("andesite_slab", &mobs()));
    }

    #[test]
    fn tools_armor_buckets_boats_carts_and_discs_stack_to_one() {
        for name in [
            "copper_sword",
            "copper_pickaxe",
            "copper_axe",
            "copper_shovel",
            "copper_hoe",
            "copper_helmet",
            "copper_chestplate",
            "copper_leggings",
            "copper_boots",
            "axolotl_bucket",
            "powder_snow_bucket",
            "pale_oak_boat",
            "pale_oak_chest_boat",
            "command_block_minecart",
            "music_disc_pigstep",
        ] {
            assert_eq!(max_stack(name), 1, "{name}");
        }
        for name in ["baked_potato", "andesite_slab", "pig_spawn_egg", "bundle"] {
            assert_eq!(max_stack(name), 64, "{name}");
        }
    }

    use crate::{
        inventory::{item, items, stack::ItemStack},
        world::{
            block::{self, BlockState, Shape},
            blocks,
            chunk::{Chunk, ChunkPos},
            save::{decode_chunk, encode_chunk},
        },
    };

    fn placeholder_blocks() -> Vec<&'static block::BlockDef> {
        block::registry().iter().filter(|d| d.placeholder).collect()
    }

    fn placeholder_items() -> Vec<&'static item::ItemDef> {
        item::registry().iter().filter(|d| d.placeholder).collect()
    }

    #[test]
    fn placeholders_are_registered() {
        // Counts go up as vanilla adds blocks; the point is that there are many.
        assert!(
            placeholder_blocks().len() > 500,
            "{}",
            placeholder_blocks().len()
        );
        assert!(
            placeholder_items().len() > 150,
            "{}",
            placeholder_items().len()
        );
        // A block item is one of the items, and a placeholder one.
        let slab = item::registry().of_block(blocks::by_name("andesite_slab").unwrap());
        assert!(slab.is_some_and(|id| item::registry().get(id).unwrap().placeholder));
    }

    #[test]
    fn every_placeholder_has_a_name_a_texture_or_model_and_a_stack_size() {
        for def in placeholder_blocks() {
            assert!(
                def.display.as_deref().is_some_and(|d| !d.is_empty()),
                "{} has no name",
                def.name
            );
            assert!(
                def.textures.is_some() || !def.model.is_empty(),
                "{} has no texture or model",
                def.name
            );
            assert!(def.stack >= 1 && def.stack <= 64, "{}", def.name);
            assert_eq!(def.stack, max_stack(&def.name), "{}", def.name);
            assert_eq!(
                def.model.is_empty(),
                def.shape != Shape::Model,
                "{}",
                def.name
            );
            assert!(def.hardness >= 0.0, "{} is unbreakable", def.name);
        }
        for def in placeholder_items() {
            assert!(
                def.display.as_deref().is_some_and(|d| !d.is_empty()),
                "{} has no name",
                def.name
            );
            assert_eq!(def.max_stack, max_stack(&def.name), "{}", def.name);
            // Plain items have a texture and do nothing; block items place
            // their (placeholder) block.
            match def.places {
                None => assert!(def.texture.is_some(), "{} has no texture", def.name),
                Some(block) => assert!(block::registry().def(block).placeholder, "{}", def.name),
            }
            assert!(def.food.is_none() && def.tool.is_none(), "{}", def.name);
        }
        // Block items stack as their blocks do, and are named like them.
        let stairs = item::registry()
            .of_block(blocks::by_name("andesite_stairs").unwrap())
            .unwrap();
        assert_eq!(item::registry().get(stairs).unwrap().max_stack, 64);
        let sword = items::by_name("copper_sword").unwrap();
        assert_eq!(item::registry().get(sword).unwrap().max_stack, 1);
        assert_eq!(
            item::registry().get(sword).unwrap().display.as_deref(),
            Some("Copper Sword")
        );
    }

    #[test]
    fn none_of_the_skipped_ids_are_registered() {
        let mobs: BTreeSet<String> =
            names(&std::fs::read_to_string(crate::assets::dir().join("data/mobs.ron")).unwrap())
                .unwrap()
                .into_iter()
                .map(|m| m.name)
                .collect();
        for name in SKIPPED {
            assert!(blocks::by_name(name).is_none(), "block {name}");
            assert!(items::by_name(name).is_none(), "item {name}");
        }
        for def in item::registry().iter() {
            assert!(!skipped(&def.name, &mobs), "{} is registered", def.name);
        }
        // Eggs of mobs that do exist are there, and not the others'.
        assert!(items::by_name("pig_spawn_egg").is_some());
        assert!(items::by_name("bat_spawn_egg").is_none());
    }

    #[test]
    fn placeholders_save_and_load() {
        let mut chunk = Chunk::new(ChunkPos::new(0, 0));
        let placed: Vec<_> = placeholder_blocks().into_iter().take(40).collect();
        for (i, def) in placed.iter().enumerate() {
            chunk.set(i % 16, 70 + i / 16, 3, BlockState::of(def.id));
        }
        let decoded = decode_chunk(&encode_chunk(&chunk)).unwrap();
        for (i, def) in placed.iter().enumerate() {
            assert_eq!(decoded.get(i % 16, 70 + i / 16, 3), BlockState::of(def.id));
        }
        // An item in a chest comes back by name.
        let mut entity = crate::world::block_entity::BlockEntity::chest();
        let egg = items::by_name("pig_spawn_egg").unwrap();
        entity.slots_mut()[0] = Some(ItemStack::new(egg, 3));
        chunk.set(0, 90, 0, BlockState::of(blocks::CHEST));
        chunk.set_block_entity([0, 90, 0], Some(entity));
        let decoded = decode_chunk(&encode_chunk(&chunk)).unwrap();
        let slots = decoded.block_entity([0, 90, 0]).unwrap().slots().to_vec();
        assert_eq!(slots[0], Some(ItemStack::new(egg, 3)));
    }

    /// Every texture a placeholder names is in vanilla's folder (when it
    /// is downloaded), and none is missing from the merged layers.
    #[test]
    fn placeholders_have_all_their_textures_with_the_vanilla_folder() {
        let assets = crate::assets::dir();
        if !assets.join("vanilla/assets/minecraft").is_dir() {
            return;
        }
        let res = crate::pack::Resources::load(&assets, Vec::new()).unwrap();
        let blocks = block::registry().missing_textures_in(|n| res.texture(n).is_some(), true);
        let items = item::registry().missing_textures_in(|n| res.texture(n).is_some(), true);
        assert!(blocks.is_empty(), "block textures missing: {blocks:?}");
        assert!(items.is_empty(), "item textures missing: {items:?}");
    }
}
