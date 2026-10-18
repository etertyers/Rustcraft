//! Block and item model JSON from resource packs, where practical: a model
//! is followed up its `parent` chain to one of vanilla's templates
//! (`cube_all`, `cube_column`, `cross`, `orientable`, ...), and its texture
//! variables become the block's per-face textures. Models with their own
//! `elements` are left to the game's built-in shapes. Item models give
//! their `layer0` texture and their `display` transforms (used from M16).
//!
//! Packs don't ship vanilla's template files, so the templates and their
//! `display` transforms are built in here, with vanilla 1.21's values.

use std::collections::HashMap;

use log::debug;

use super::resources::{Resources, location};
use crate::world::block::{BlockDef, BlockId, BlockRegistry, BlockState, Shape, Textures};

/// Vanilla model templates a pack's models can build on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Template {
    /// `block/block`: no faces of its own, only display transforms.
    Block,
    CubeAll,
    Leaves,
    CubeColumn,
    CubeBottomTop,
    Cube,
    Orientable,
    OrientableWithBottom,
    Cross,
    Crop,
    /// Slabs and stairs: `bottom`, `top` and `side`.
    SlabOrStairs,
    /// `item/generated`: a flat sprite from `layer0`.
    Generated,
    /// `item/handheld`: a flat sprite held like a tool.
    Handheld,
}

impl Template {
    fn builtin(name: &str) -> Option<Self> {
        Some(match name {
            "block/block" | "block/thin_block" => Self::Block,
            "block/cube_all" | "block/cube_mirrored_all" => Self::CubeAll,
            "block/leaves" => Self::Leaves,
            "block/cube_column" | "block/cube_column_horizontal" | "block/cube_column_mirrored" => {
                Self::CubeColumn
            }
            "block/cube_bottom_top" => Self::CubeBottomTop,
            "block/cube" | "block/cube_directional" => Self::Cube,
            "block/orientable" => Self::Orientable,
            "block/orientable_with_bottom" => Self::OrientableWithBottom,
            "block/cross" | "block/tinted_cross" => Self::Cross,
            "block/crop" => Self::Crop,
            "block/slab" | "block/slab_top" | "block/stairs" | "block/inner_stairs"
            | "block/outer_stairs" => Self::SlabOrStairs,
            "item/generated" | "builtin/generated" => Self::Generated,
            "item/handheld" | "item/handheld_rod" | "item/handheld_mace" => Self::Handheld,
            _ => return None,
        })
    }

    /// Whether a block drawn as `shape` can take this template's faces.
    fn fits(self, shape: Shape) -> bool {
        match self {
            Self::CubeAll
            | Self::Leaves
            | Self::CubeColumn
            | Self::CubeBottomTop
            | Self::Cube
            | Self::Orientable
            | Self::OrientableWithBottom => shape == Shape::Cube,
            Self::Cross => shape == Shape::Cross,
            Self::Crop => shape == Shape::Crop,
            Self::SlabOrStairs => matches!(shape, Shape::Slab | Shape::Stairs),
            Self::Block | Self::Generated | Self::Handheld => false,
        }
    }

    /// Faces as `(field, variable)`: which texture variable each of our
    /// `Textures` fields reads. Fronts go on south, as `blocks.ron` puts them.
    fn faces(self) -> &'static [(&'static str, &'static str)] {
        match self {
            Self::CubeAll | Self::Leaves => &[("all", "all")],
            Self::CubeColumn => &[("end", "end"), ("side", "side")],
            Self::CubeBottomTop | Self::SlabOrStairs => {
                &[("top", "top"), ("bottom", "bottom"), ("side", "side")]
            }
            Self::Cube => &[
                ("top", "up"),
                ("bottom", "down"),
                ("north", "north"),
                ("south", "south"),
                ("east", "east"),
                ("west", "west"),
            ],
            Self::Orientable => &[
                ("top", "top"),
                ("bottom", "top"),
                ("side", "side"),
                ("south", "front"),
            ],
            Self::OrientableWithBottom => &[
                ("top", "top"),
                ("bottom", "bottom"),
                ("side", "side"),
                ("south", "front"),
            ],
            Self::Cross => &[("all", "cross")],
            Self::Crop => &[("all", "crop")],
            Self::Block | Self::Generated | Self::Handheld => &[],
        }
    }
}

/// One display transform, in model units as written in the JSON
/// (rotation in degrees, translation in 1/16 blocks).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Transform {
    pub rotation: [f32; 3],
    pub translation: [f32; 3],
    pub scale: [f32; 3],
}

impl Transform {
    pub const IDENTITY: Self = Self {
        rotation: [0.0; 3],
        translation: [0.0; 3],
        scale: [1.0; 3],
    };

    const fn new(rotation: [f32; 3], translation: [f32; 3], scale: f32) -> Self {
        Self {
            rotation,
            translation,
            scale: [scale; 3],
        }
    }

    fn parse(json: &serde_json::Value) -> Self {
        let vector = |key: &str, default: [f32; 3]| {
            json.get(key)
                .and_then(|v| v.as_array())
                .filter(|a| a.len() == 3)
                .map_or(default, |a| {
                    [0, 1, 2].map(|i| a[i].as_f64().unwrap_or(default[i] as f64) as f32)
                })
        };
        Self {
            rotation: vector("rotation", [0.0; 3]),
            translation: vector("translation", [0.0; 3]),
            scale: vector("scale", [1.0; 3]),
        }
    }
}

/// Where an item is being drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DisplayContext {
    ThirdPersonRightHand,
    ThirdPersonLeftHand,
    FirstPersonRightHand,
    FirstPersonLeftHand,
    Gui,
    Head,
    Ground,
    Fixed,
}

impl DisplayContext {
    pub const ALL: [Self; 8] = [
        Self::ThirdPersonRightHand,
        Self::ThirdPersonLeftHand,
        Self::FirstPersonRightHand,
        Self::FirstPersonLeftHand,
        Self::Gui,
        Self::Head,
        Self::Ground,
        Self::Fixed,
    ];

    pub fn key(self) -> &'static str {
        match self {
            Self::ThirdPersonRightHand => "thirdperson_righthand",
            Self::ThirdPersonLeftHand => "thirdperson_lefthand",
            Self::FirstPersonRightHand => "firstperson_righthand",
            Self::FirstPersonLeftHand => "firstperson_lefthand",
            Self::Gui => "gui",
            Self::Head => "head",
            Self::Ground => "ground",
            Self::Fixed => "fixed",
        }
    }

    /// A left hand without its own transform uses the right hand's.
    fn fallback(self) -> Option<Self> {
        match self {
            Self::ThirdPersonLeftHand => Some(Self::ThirdPersonRightHand),
            Self::FirstPersonLeftHand => Some(Self::FirstPersonRightHand),
            _ => None,
        }
    }
}

/// An item model's display transforms.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Display(HashMap<&'static str, Transform>);

impl Display {
    /// The transform for `context`, as vanilla's `ItemTransforms`: a left
    /// hand falls back to the right, and anything else unset is identity.
    pub fn get(&self, context: DisplayContext) -> Transform {
        self.0
            .get(context.key())
            .or_else(|| context.fallback().and_then(|f| self.0.get(f.key())))
            .copied()
            .unwrap_or(Transform::IDENTITY)
    }

    /// Fills the contexts this doesn't set from `parent`.
    fn inherit(&mut self, parent: impl IntoIterator<Item = (&'static str, Transform)>) {
        for (key, transform) in parent {
            self.0.entry(key).or_insert(transform);
        }
    }

    fn read(&mut self, json: &serde_json::Value) {
        let Some(display) = json.get("display").and_then(|d| d.as_object()) else {
            return;
        };
        for context in DisplayContext::ALL {
            if let Some(transform) = display.get(context.key()) {
                self.0
                    .entry(context.key())
                    .or_insert_with(|| Transform::parse(transform));
            }
        }
    }

    /// The built-in display of a template (vanilla's `block/block.json`,
    /// `item/generated.json` and `item/handheld.json`).
    pub fn builtin(template: Template) -> Self {
        let mut display = Self::default();
        match template {
            Template::Generated | Template::Handheld => {
                if template == Template::Handheld {
                    display.inherit(HANDHELD.iter().copied());
                }
                display.inherit(GENERATED.iter().copied());
            }
            _ => display.inherit(BLOCK.iter().copied()),
        }
        display
    }
}

/// Vanilla's `block/block.json` display.
const BLOCK: &[(&str, Transform)] = &[
    ("gui", Transform::new([30.0, 225.0, 0.0], [0.0; 3], 0.625)),
    ("ground", Transform::new([0.0; 3], [0.0, 3.0, 0.0], 0.25)),
    ("fixed", Transform::new([0.0; 3], [0.0; 3], 0.5)),
    (
        "thirdperson_righthand",
        Transform::new([75.0, 45.0, 0.0], [0.0, 2.5, 0.0], 0.375),
    ),
    (
        "firstperson_righthand",
        Transform::new([0.0, 45.0, 0.0], [0.0; 3], 0.4),
    ),
    (
        "firstperson_lefthand",
        Transform::new([0.0, 225.0, 0.0], [0.0; 3], 0.4),
    ),
];

/// Vanilla's `item/generated.json` display.
const GENERATED: &[(&str, Transform)] = &[
    ("ground", Transform::new([0.0; 3], [0.0, 2.0, 0.0], 0.5)),
    (
        "head",
        Transform::new([0.0, 180.0, 0.0], [0.0, 13.0, 7.0], 1.0),
    ),
    (
        "thirdperson_righthand",
        Transform::new([0.0; 3], [0.0, 3.0, 1.0], 0.55),
    ),
    (
        "firstperson_righthand",
        Transform::new([0.0, -90.0, 25.0], [1.13, 3.2, 1.13], 0.68),
    ),
    ("fixed", Transform::new([0.0, 180.0, 0.0], [0.0; 3], 1.0)),
];

/// Vanilla's `item/handheld.json` display (over `item/generated`).
const HANDHELD: &[(&str, Transform)] = &[
    (
        "thirdperson_righthand",
        Transform::new([0.0, -90.0, 55.0], [0.0, 4.0, 0.5], 0.85),
    ),
    (
        "thirdperson_lefthand",
        Transform::new([0.0, 90.0, -55.0], [0.0, 4.0, 0.5], 0.85),
    ),
    (
        "firstperson_righthand",
        Transform::new([0.0, -90.0, 25.0], [1.13, 3.2, 1.13], 0.68),
    ),
    (
        "firstperson_lefthand",
        Transform::new([0.0, 90.0, -25.0], [1.13, 3.2, 1.13], 0.68),
    ),
];

/// A model followed up its parents.
#[derive(Debug, Clone, PartialEq)]
pub struct Model {
    /// The template it ends at, if any.
    pub template: Option<Template>,
    /// Whether some model in the chain draws its own `elements`.
    pub elements: bool,
    /// Texture variables, nearest model first.
    textures: HashMap<String, String>,
    pub display: Display,
}

/// Longest `parent` chain followed (vanilla models nest a few deep).
const MAX_DEPTH: usize = 16;

/// A model name as a path under `models/`: `minecraft:block/stone` →
/// `block/stone`; pre-1.13 names without a folder were under `block/`.
fn model_path(name: &str) -> String {
    let name = location(name);
    if name.contains('/') {
        name.to_owned()
    } else {
        format!("block/{name}")
    }
}

impl Model {
    /// Loads `name` (`block/stone`) and its parents. `None` if the model
    /// isn't in any layer.
    pub fn load(res: &Resources, name: &str) -> Option<Self> {
        let mut model = Self {
            template: None,
            elements: false,
            textures: HashMap::new(),
            display: Display::default(),
        };
        let mut next = Some(model_path(name));
        let mut found = false;
        for _ in 0..MAX_DEPTH {
            let Some(name) = next.take() else { break };
            if let Some(template) = Template::builtin(&name) {
                model.template = Some(template);
                model.display.inherit(Display::builtin(template).0);
                break;
            }
            let json = res.json(&format!("models/{name}.json"))?;
            found = true;
            if let Some(vars) = json.get("textures").and_then(|t| t.as_object()) {
                for (key, value) in vars {
                    if let Some(value) = value.as_str() {
                        model
                            .textures
                            .entry(key.clone())
                            .or_insert_with(|| value.to_owned());
                    }
                }
            }
            model.display.read(&json);
            // Elements in a model hide its parents' (vanilla's rule), so a
            // chain that draws its own boxes isn't a plain template.
            if json.get("elements").is_some() && model.template.is_none() {
                model.elements = true;
            }
            next = json.get("parent").and_then(|p| p.as_str()).map(model_path);
        }
        found.then_some(model)
    }

    /// The texture a variable names, following `#other` references.
    pub fn texture(&self, var: &str) -> Option<String> {
        let mut value = self.textures.get(var)?;
        for _ in 0..MAX_DEPTH {
            match value.strip_prefix('#') {
                Some(other) => value = self.textures.get(other)?,
                None => return Some(location(value).to_owned()),
            }
        }
        None
    }

    /// Per-face textures, if it's a block template that fits `shape`.
    pub fn block_textures(&self, shape: Shape) -> Option<Textures> {
        let template = self.template.filter(|t| !self.elements && t.fits(shape))?;
        let mut textures = Textures::default();
        for &(field, var) in template.faces() {
            let texture = Some(self.texture(var)?);
            match field {
                "all" => textures.all = texture,
                "end" => textures.end = texture,
                "side" => textures.side = texture,
                "top" => textures.top = texture,
                "bottom" => textures.bottom = texture,
                "north" => textures.north = texture,
                "south" => textures.south = texture,
                "east" => textures.east = texture,
                _ => textures.west = texture,
            }
        }
        Some(textures)
    }

    /// The sprite of a flat item model.
    pub fn item_texture(&self) -> Option<String> {
        matches!(
            self.template,
            Some(Template::Generated | Template::Handheld)
        )
        .then(|| self.texture("layer0"))
        .flatten()
    }
}

/// One choice in a blockstate variant list.
#[derive(Debug, Clone, PartialEq)]
pub struct Choice {
    /// `None` for vanilla's own model of the block, which packs don't
    /// ship: the block keeps its `blocks.ron` textures.
    pub textures: Option<Textures>,
    pub weight: u32,
}

/// A blockstate variant's condition as (property, value) pairs.
type Condition = Vec<(String, String)>;

/// Pack textures for every block state the packs' models cover.
#[derive(Debug, Default)]
pub struct BlockOverrides {
    /// Per block: (condition as name/value pairs, choices), in file order.
    blocks: HashMap<BlockId, Vec<(Condition, Vec<Choice>)>>,
}

impl BlockOverrides {
    /// Reads `blockstates/<block>.json` for every block (or, without one,
    /// `models/block/<block>.json`). Only models built on a template that
    /// fits the block's shape count; the rest keep `blocks.ron`'s textures.
    pub fn load(res: &Resources, registry: &BlockRegistry) -> Self {
        let mut models: HashMap<String, Option<Model>> = HashMap::new();
        let mut model = |name: &str| {
            models
                .entry(name.to_owned())
                .or_insert_with(|| Model::load(res, name))
                .clone()
        };
        let mut blocks = HashMap::new();
        for def in registry.iter().filter(|def| def.shape != Shape::None) {
            let variants = match res.json(&format!("blockstates/{}.json", def.name)) {
                Some(json) => variants_of(&json),
                None => vec![(String::new(), vec![(format!("block/{}", def.name), 1)])],
            };
            let mut entries = Vec::new();
            for (condition, list) in variants {
                // A model no layer has is vanilla's own (Ashen lists
                // `block/stone` among its stone looks); one that isn't a
                // template that fits rules the whole list out.
                let choices: Option<Vec<Choice>> = list
                    .iter()
                    .map(|(name, weight)| {
                        let textures = match model(name) {
                            Some(model) => Some(model.block_textures(def.shape)?),
                            None => None,
                        };
                        Some(Choice {
                            textures,
                            weight: *weight,
                        })
                    })
                    .collect();
                if let Some(choices) =
                    choices.filter(|c| c.iter().any(|choice| choice.textures.is_some()))
                {
                    entries.push((parse_condition(&condition), choices));
                }
            }
            if !entries.is_empty() {
                debug!("{} takes its textures from pack models", def.name);
                blocks.insert(def.id, entries);
            }
        }
        Self { blocks }
    }

    /// The texture choices for `state`, if a pack model covers it.
    pub fn get(&self, state: BlockState) -> Option<&[Choice]> {
        let entries = self.blocks.get(&state.id())?;
        let def = state.def();
        entries
            .iter()
            .find(|(condition, _)| matches(def, state, condition))
            .map(|(_, choices)| choices.as_slice())
    }

    /// Every texture the covered states use.
    pub fn textures(&self) -> impl Iterator<Item = &str> {
        self.blocks
            .values()
            .flatten()
            .flat_map(|(_, choices)| choices)
            .filter_map(|choice| choice.textures.as_ref())
            .flat_map(|t| {
                [
                    &t.all, &t.end, &t.side, &t.top, &t.bottom, &t.north, &t.south, &t.east,
                    &t.west,
                ]
            })
            .filter_map(|name| name.as_deref())
    }

    /// How many blocks packs re-model.
    pub fn len(&self) -> usize {
        self.blocks.len()
    }

    pub fn is_empty(&self) -> bool {
        self.blocks.is_empty()
    }
}

/// A blockstate file's `variants`: (condition, [(model, weight)]). Files
/// with only `multipart` (fences, walls) give nothing.
fn variants_of(json: &serde_json::Value) -> Vec<(String, Vec<(String, u32)>)> {
    let Some(variants) = json.get("variants").and_then(|v| v.as_object()) else {
        return Vec::new();
    };
    let choice = |v: &serde_json::Value| {
        let model = v.get("model")?.as_str()?.to_owned();
        let weight = v.get("weight").and_then(|w| w.as_u64()).unwrap_or(1) as u32;
        Some((model, weight.max(1)))
    };
    variants
        .iter()
        .map(|(condition, value)| {
            let list = match value {
                serde_json::Value::Array(list) => list.iter().filter_map(choice).collect(),
                other => choice(other).into_iter().collect(),
            };
            (condition.clone(), list)
        })
        .collect()
}

/// `axis=y,lit=true` → [("axis", "y"), ("lit", "true")]; `""` and
/// `normal` (pre-1.13) are empty.
fn parse_condition(text: &str) -> Condition {
    text.split(',')
        .filter_map(|pair| pair.split_once('='))
        .map(|(k, v)| (k.to_owned(), v.to_owned()))
        .collect()
}

/// Whether `state` meets every pair. Properties the block doesn't have
/// here (vanilla's leaves have `distance`) don't rule a variant out.
fn matches(def: &BlockDef, state: BlockState, condition: &[(String, String)]) -> bool {
    condition.iter().all(|(key, value)| {
        def.props
            .iter()
            .find(|prop| prop.name() == key)
            .is_none_or(|&prop| prop.values()[state.get(prop) as usize] == value)
    })
}

/// Item model results for every item: sprites packs swap in, and display
/// transforms.
#[derive(Debug, Default)]
pub struct ItemModels {
    textures: HashMap<String, String>,
    displays: HashMap<String, Display>,
}

impl ItemModels {
    /// Reads `models/item/<name>.json` for each of `items`: (name, drawn as
    /// a block, held like a tool).
    pub fn load<'a>(
        res: &Resources,
        items: impl IntoIterator<Item = (&'a str, bool, bool)>,
    ) -> Self {
        let mut models = Self::default();
        for (name, block, tool) in items {
            let model = Model::load(res, &format!("item/{name}"));
            if let Some(texture) = model.as_ref().and_then(Model::item_texture) {
                models.textures.insert(name.to_owned(), texture);
            }
            let fallback = if block {
                Template::Block
            } else if tool {
                Template::Handheld
            } else {
                Template::Generated
            };
            let mut display = model.map(|m| m.display).unwrap_or_default();
            display.inherit(Display::builtin(fallback).0);
            models.displays.insert(name.to_owned(), display);
        }
        models
    }

    /// The sprite a pack's item model gives `item`, if it gives one.
    pub fn texture(&self, item: &str) -> Option<&str> {
        self.textures.get(item).map(String::as_str)
    }

    /// The display transforms of `item`, with built-in defaults.
    pub fn display(&self, item: &str) -> Option<&Display> {
        self.displays.get(item)
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;
    use crate::{
        pack::{Pack, tests::*},
        world::{blocks, props::Prop, save::TempDir},
    };

    fn pack_with(dir: &Path, files: &[(&str, &str)]) -> Resources {
        let zip = dir.join("models.zip");
        let mut entries: Vec<(String, &[u8])> = vec![("pack.mcmeta".into(), MCMETA)];
        for (path, text) in files {
            entries.push((format!("assets/minecraft/{path}"), text.as_bytes()));
        }
        let refs: Vec<(&str, &[u8])> = entries.iter().map(|(p, b)| (p.as_str(), *b)).collect();
        write_zip(&zip, &refs);
        let base = dir.join("base");
        std::fs::create_dir_all(&base).unwrap();
        Resources::new(
            Pack::base(&base),
            vec![Pack::open(&zip).unwrap()],
            HashMap::new(),
        )
    }

    #[test]
    fn templates_give_faces_through_parent_chains() {
        let dir = TempDir::new("models");
        let res = pack_with(
            dir.path(),
            &[
                (
                    "models/block/stone.json",
                    r#"{"parent": "minecraft:block/cube_all", "textures": {"all": "minecraft:block/stone_alt"}}"#,
                ),
                (
                    "models/block/my_log.json",
                    r##"{"parent": "block/cube_column", "textures": {"end": "#top", "top": "block/oak_log_top", "side": "block/oak_log"}}"##,
                ),
                (
                    "models/block/furnace.json",
                    r#"{"parent": "block/orientable", "textures": {"top": "block/furnace_top", "front": "block/furnace_front", "side": "block/furnace_side"}}"#,
                ),
                (
                    "models/block/middle.json",
                    r#"{"parent": "block/cross", "textures": {"cross": "block/far"}}"#,
                ),
                (
                    "models/block/flower.json",
                    r#"{"parent": "block/middle", "textures": {"cross": "block/near"}}"#,
                ),
                (
                    "models/block/fancy.json",
                    r#"{"parent": "block/block", "elements": [], "textures": {"all": "block/x"}}"#,
                ),
                ("models/block/loop.json", r#"{"parent": "block/loop"}"#),
            ],
        );
        let stone = Model::load(&res, "block/stone").unwrap();
        assert_eq!(stone.template, Some(Template::CubeAll));
        let textures = stone.block_textures(Shape::Cube).unwrap();
        assert_eq!(textures.all.as_deref(), Some("block/stone_alt"));
        // A cube template doesn't fit a flower.
        assert!(stone.block_textures(Shape::Cross).is_none());

        let log = Model::load(&res, "block/my_log")
            .unwrap()
            .block_textures(Shape::Cube)
            .unwrap();
        assert_eq!(log.end.as_deref(), Some("block/oak_log_top"));
        assert_eq!(log.side.as_deref(), Some("block/oak_log"));

        let furnace = Model::load(&res, "furnace")
            .unwrap()
            .block_textures(Shape::Cube)
            .unwrap();
        assert_eq!(furnace.south.as_deref(), Some("block/furnace_front"));
        assert_eq!(furnace.bottom.as_deref(), Some("block/furnace_top"));

        // The nearest model's variables win.
        let flower = Model::load(&res, "block/flower").unwrap();
        assert_eq!(flower.texture("cross").as_deref(), Some("block/near"));
        assert!(flower.block_textures(Shape::Cross).is_some());

        let fancy = Model::load(&res, "block/fancy").unwrap();
        assert!(fancy.elements);
        assert!(fancy.block_textures(Shape::Cube).is_none());
        // A model naming itself as parent stops without hanging.
        assert!(Model::load(&res, "block/loop").unwrap().template.is_none());
        assert!(Model::load(&res, "block/missing").is_none());
    }

    #[test]
    fn blockstates_pick_weighted_choices_by_state() {
        let dir = TempDir::new("blockstates");
        let res = pack_with(
            dir.path(),
            &[
                (
                    "blockstates/stone.json",
                    r#"{"variants": {"": [{"model": "block/stone"}, {"model": "block/stone2", "weight": 3}]}}"#,
                ),
                (
                    "models/block/stone.json",
                    r#"{"parent": "block/cube_all", "textures": {"all": "block/stone"}}"#,
                ),
                (
                    "models/block/stone2.json",
                    r#"{"parent": "block/cube_all", "textures": {"all": "block/stone2"}}"#,
                ),
                (
                    "blockstates/oak_log.json",
                    r#"{"variants": {"axis=x": {"model": "block/log_x"}, "axis=y": {"model": "block/log_y"}, "axis=z": {"model": "block/log_x"}}}"#,
                ),
                (
                    "models/block/log_x.json",
                    r#"{"parent": "block/cube_column", "textures": {"end": "block/x_end", "side": "block/x_side"}}"#,
                ),
                (
                    "models/block/log_y.json",
                    r#"{"parent": "block/cube_column", "textures": {"end": "block/y_end", "side": "block/y_side"}}"#,
                ),
                (
                    "models/block/dirt.json",
                    r#"{"parent": "block/cube_all", "textures": {"all": "block/dirt2"}}"#,
                ),
                ("blockstates/oak_fence.json", r#"{"multipart": []}"#),
                (
                    "blockstates/grass_block.json",
                    r#"{"variants": {"snowy=false": {"model": "block/grass_custom"}}}"#,
                ),
                (
                    "models/block/grass_custom.json",
                    r#"{"parent": "block/block", "elements": []}"#,
                ),
            ],
        );
        let overrides = BlockOverrides::load(&res, crate::world::block::registry());
        assert_eq!(overrides.len(), 3);
        let stone = overrides.get(BlockState::of(blocks::STONE)).unwrap();
        assert_eq!(stone.len(), 2);
        assert_eq!(stone[1].weight, 3);
        let all = |choice: &Choice| choice.textures.as_ref().unwrap().all.clone();
        assert_eq!(all(&stone[1]).as_deref(), Some("block/stone2"));
        let log = BlockState::of(blocks::OAK_LOG);
        let up = overrides.get(log).unwrap();
        let end = |choice: &Choice| choice.textures.as_ref().unwrap().end.clone();
        assert_eq!(end(&up[0]).as_deref(), Some("block/y_end"));
        let sideways = overrides.get(log.with(Prop::Axis, 0)).unwrap();
        assert_eq!(end(&sideways[0]).as_deref(), Some("block/x_end"));
        // A model with no blockstate file still counts.
        assert!(overrides.get(BlockState::of(blocks::DIRT)).is_some());
        assert!(overrides.get(BlockState::of(blocks::OAK_FENCE)).is_none());
        assert!(overrides.get(BlockState::of(blocks::GRASS_BLOCK)).is_none());
        assert!(overrides.get(BlockState::of(blocks::COBBLESTONE)).is_none());
    }

    #[test]
    fn item_models_swap_sprites_and_carry_display_transforms() {
        let dir = TempDir::new("items");
        let res = pack_with(
            dir.path(),
            &[
                (
                    "models/item/stick.json",
                    r#"{"parent": "item/handheld", "textures": {"layer0": "item/fancy_stick"}}"#,
                ),
                (
                    "models/item/apple.json",
                    r#"{"parent": "item/generated", "textures": {"layer0": "item/apple"},
                    "display": {"firstperson_righthand": {"rotation": [1, 2, 3], "scale": [0.5, 0.5, 0.5]}}}"#,
                ),
            ],
        );
        let models = ItemModels::load(
            &res,
            [
                ("stick", false, false),
                ("apple", false, false),
                ("stone", true, false),
                ("iron_sword", false, true),
            ],
        );
        assert_eq!(models.texture("stick"), Some("item/fancy_stick"));
        assert_eq!(models.texture("stone"), None);
        let first = DisplayContext::FirstPersonRightHand;
        let apple = models.display("apple").unwrap();
        assert_eq!(
            apple.get(first),
            Transform {
                rotation: [1.0, 2.0, 3.0],
                translation: [0.0; 3],
                scale: [0.5; 3]
            }
        );
        // Unset contexts come from the template; left hands mirror right.
        assert_eq!(
            apple.get(DisplayContext::Ground).translation,
            [0.0, 2.0, 0.0]
        );
        assert_eq!(
            apple.get(DisplayContext::FirstPersonLeftHand),
            apple.get(first)
        );
        assert_eq!(apple.get(DisplayContext::Gui), Transform::IDENTITY);
        // Built-in defaults when no pack has a model.
        let stone = models.display("stone").unwrap();
        assert_eq!(stone.get(DisplayContext::Gui).rotation, [30.0, 225.0, 0.0]);
        assert_eq!(stone.get(first).scale, [0.4; 3]);
        let sword = models.display("iron_sword").unwrap();
        assert_eq!(
            sword.get(DisplayContext::ThirdPersonRightHand).rotation,
            [0.0, -90.0, 55.0]
        );
        assert_eq!(sword.get(DisplayContext::Ground).scale, [0.5; 3]);
    }

    #[test]
    fn kenzos_packs_models_resolve() {
        let assets = crate::assets::dir();
        let ashen = Pack::open(&assets.join("Ashen_1.8.3.zip")).unwrap();
        let res = Resources::new(Pack::base(&assets), vec![ashen], HashMap::new());
        let overrides = BlockOverrides::load(&res, crate::world::block::registry());
        // Ashen gives stone four looks: vanilla's and three of its own.
        let stone = overrides.get(BlockState::of(blocks::STONE)).unwrap();
        assert_eq!(stone.len(), 4, "{stone:?}");
        assert_eq!(stone[0].textures, None);
        for choice in &stone[1..] {
            let name = choice.textures.as_ref().unwrap().all.as_deref().unwrap();
            assert!(res.texture(name).is_some(), "{name}");
        }
    }
}
