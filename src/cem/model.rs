//! OptiFine's custom entity model files (`.jem`, with `.jpm` part files
//! they may point to), as Entity Model Features reads them: a tree of
//! parts, each with a pivot, a rotation, boxes and animations, in model
//! pixels (the same space as `mobs.ron`'s: y down, the feet at 24).
//! Unknown fields are skipped, with one line in the log per file.

use std::collections::HashMap;

use serde_json::Value;

use super::{
    expr::{PartField, Program, RenderField, Resolver, Slot},
    vars,
};

/// One box of a part.
#[derive(Debug, Clone, PartialEq)]
pub struct CemBox {
    /// Corner and size, relative to the part's pivot, in the model's
    /// space (after the file's axes are turned to it).
    pub from: [f32; 3],
    pub size: [f32; 3],
    /// Where its texture starts (the usual layout from a size), if it
    /// has an offset.
    pub offset: Option<[f32; 2]>,
    /// Texture rects (u1, v1, u2, v2) for the faces the file spells out:
    /// north, east, south, west, up, down.
    pub faces: [Option<[f32; 4]>; 6],
    /// Pixels it is grown on every side.
    pub grow: f32,
}

/// One part of a model.
#[derive(Debug, Clone, PartialEq)]
pub struct CemPart {
    /// What animations call it (its `id`, or its `part`).
    pub name: String,
    /// The vanilla part it replaces (`part`), if it says.
    pub vanilla: Option<String>,
    pub parent: Option<usize>,
    /// Where the part turns, relative to its parent.
    pub translate: [f32; 3],
    /// Resting rotation, radians.
    pub rotate: [f32; 3],
    pub scale: [f32; 3],
    pub mirror_u: bool,
    pub mirror_v: bool,
    pub boxes: Vec<CemBox>,
    /// Draws on top of the vanilla part of the same name instead of
    /// replacing it.
    pub attach: bool,
    pub visible: bool,
}

/// One animation line: set `target` to the expression's value.
#[derive(Debug, Clone, PartialEq)]
pub struct Animation {
    pub target: Slot,
    pub program: Program,
}

/// A loaded model.
#[derive(Debug, Clone, PartialEq)]
pub struct CemModel {
    /// The texture's path as the file says it (`pig.png`).
    pub texture: Option<String>,
    pub texture_size: (f32, f32),
    pub shadow_size: f32,
    /// Every part, a parent before its children.
    pub parts: Vec<CemPart>,
    /// Animation lines, in file order.
    pub animations: Vec<Animation>,
    /// How many `var.` and `varb.` values it keeps.
    pub var_count: usize,
    /// The names of `var.` and `varb.` values, by slot (a `varb.` has the
    /// prefix).
    pub var_names: Vec<String>,
    /// Names the animations use that nothing gives a value to.
    pub unknown: Vec<String>,
    /// It is the model of a baby (a `_baby` or `baby_` file), already
    /// small: the mob isn't also drawn at half size.
    pub baby: bool,
}

impl CemModel {
    /// The index of the part an animation calls `name`.
    pub fn part_named(&self, name: &str) -> Option<usize> {
        self.parts.iter().position(|p| p.name == name)
    }

    /// The part whose vanilla name is `name`, if the file says so.
    pub fn replaces(&self, name: &str) -> Option<usize> {
        self.parts
            .iter()
            .position(|p| p.vanilla.as_deref() == Some(name) || p.name == name)
    }
}

/// How the loaded files are read: a relative path to its text.
pub type Reader<'a> = &'a dyn Fn(&str) -> Option<String>;

struct Names {
    parts: HashMap<String, usize>,
    vars: HashMap<String, u16>,
    var_names: Vec<String>,
    unknown: Vec<String>,
}

impl Resolver for Names {
    fn resolve(&mut self, name: &str) -> Option<Slot> {
        if let Some(i) = vars::slot(name) {
            return Some(Slot::Entity(i));
        }
        if name.starts_with("var.") || name.starts_with("varb.") {
            let next = self.vars.len() as u16;
            let slot = *self.vars.entry(name.to_owned()).or_insert_with(|| {
                self.var_names.push(name.to_owned());
                next
            });
            return Some(Slot::Var(slot));
        }
        if let Some(field) = name.strip_prefix("render.") {
            return RenderField::named(field).map(Slot::Render);
        }
        let (part, field) = name.rsplit_once('.')?;
        let field = PartField::named(field)?;
        let index = *self.parts.get(part)?;
        Some(Slot::Part(index as u16, field))
    }
}

/// Reads a `.jem` (`text`), its parts from `.jpm` files via `read`.
/// `log` gets one line for fields it skipped.
pub fn parse(text: &str, read: Reader, log: &mut impl FnMut(String)) -> Result<CemModel, String> {
    let json: Value = serde_json::from_str(text).map_err(|e| format!("not JSON: {e}"))?;
    let texture_size = json
        .get("textureSize")
        .and_then(numbers::<2>)
        .map_or((16.0, 16.0), |[w, h]| (w, h));
    let mut skipped = Vec::new();
    let mut model = CemModel {
        texture: json
            .get("texture")
            .and_then(Value::as_str)
            .map(str::to_owned),
        texture_size: (texture_size.0, texture_size.1),
        shadow_size: json
            .get("shadow_size")
            .and_then(Value::as_f64)
            .unwrap_or(1.0) as f32,
        parts: Vec::new(),
        animations: Vec::new(),
        var_count: 0,
        var_names: Vec::new(),
        unknown: Vec::new(),
        baby: false,
    };
    let mut raw_animations: Vec<(usize, Vec<(String, String)>)> = Vec::new();
    for key in json.as_object().into_iter().flat_map(|o| o.keys()) {
        if !["texture", "textureSize", "shadow_size", "models"].contains(&key.as_str()) {
            skipped.push(key.clone());
        }
    }
    let models = json
        .get("models")
        .and_then(Value::as_array)
        .ok_or("no \"models\"")?;
    for part in models {
        add_part(
            part,
            None,
            read,
            &mut model,
            &mut raw_animations,
            &mut skipped,
            0,
        )?;
    }
    // Names are known once every part is: now the animations.
    let mut names = Names {
        parts: HashMap::new(),
        vars: HashMap::new(),
        var_names: Vec::new(),
        unknown: Vec::new(),
    };
    for (i, part) in model.parts.iter().enumerate() {
        names.parts.entry(part.name.clone()).or_insert(i);
        if let Some(vanilla) = &part.vanilla {
            names.parts.entry(vanilla.clone()).or_insert(i);
        }
    }
    for (_, lines) in &raw_animations {
        for (target, expression) in lines {
            let Some(slot) = names.resolve(target) else {
                if !names.unknown.contains(target) {
                    names.unknown.push(target.clone());
                }
                continue;
            };
            let mut unknown = Vec::new();
            let program =
                match Program::parse(expression, &mut names, &mut |n| unknown.push(n.to_owned())) {
                    Ok(program) => program,
                    Err(error) => {
                        skipped.push(format!("{target} = {expression} ({error})"));
                        continue;
                    }
                };
            for name in unknown {
                if !names.unknown.contains(&name) {
                    names.unknown.push(name);
                }
            }
            model.animations.push(Animation {
                target: slot,
                program,
            });
        }
    }
    model.var_count = names.var_names.len();
    model.var_names = names.var_names;
    model.unknown = names.unknown;
    if !skipped.is_empty() {
        skipped.truncate(8);
        log(format!("skipped {}", skipped.join(", ")));
    }
    Ok(model)
}

fn numbers<const N: usize>(value: &Value) -> Option<[f32; N]> {
    let array = value.as_array()?;
    if array.len() < N {
        return None;
    }
    let mut out = [0.0; N];
    for (slot, v) in out.iter_mut().zip(array) {
        *slot = v.as_f64()? as f32;
    }
    Some(out)
}

/// Which axes a file turns: its `invertAxis` ("xy").
fn inverted(part: &Value) -> [bool; 3] {
    let text = part.get("invertAxis").and_then(Value::as_str).unwrap_or("");
    [text.contains('x'), text.contains('y'), text.contains('z')]
}

#[allow(clippy::too_many_arguments)]
fn add_part(
    part: &Value,
    parent: Option<usize>,
    read: Reader,
    model: &mut CemModel,
    animations: &mut Vec<(usize, Vec<(String, String)>)>,
    skipped: &mut Vec<String>,
    depth: usize,
) -> Result<(), String> {
    if depth > 16 {
        return Err("parts nested too deep".to_owned());
    }
    // A part can live in a `.jpm` of its own, which says the rest.
    let mut merged = part.clone();
    if let Some(path) = part.get("model").and_then(Value::as_str)
        && let Some(text) = read(path)
        && let Ok(Value::Object(file)) = serde_json::from_str::<Value>(&text)
        && let Value::Object(map) = &mut merged
    {
        for (key, value) in file {
            map.insert(key, value);
        }
    }
    let part = &merged;
    let flip = inverted(part);
    let signed = |v: [f32; 3]| {
        [
            if flip[0] { -v[0] } else { v[0] },
            if flip[1] { -v[1] } else { v[1] },
            if flip[2] { -v[2] } else { v[2] },
        ]
    };
    let vanilla = part.get("part").and_then(Value::as_str).map(str::to_owned);
    let name = part
        .get("id")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
        .or_else(|| vanilla.clone())
        .unwrap_or_else(|| format!("part{}", model.parts.len()));
    let translate = part
        .get("translate")
        .and_then(numbers::<3>)
        .unwrap_or([0.0; 3]);
    // The pivot's y is measured from the feet in the file's own up (so
    // it is inverted to the model's down), the rest like the boxes.
    let translate = signed(translate);
    let rotate = part
        .get("rotate")
        .and_then(numbers::<3>)
        .unwrap_or([0.0; 3]);
    // Turning an axis over turns rotations about the others the other way.
    let rotate = [
        rotate[0].to_radians() * if flip[1] ^ flip[2] { -1.0 } else { 1.0 },
        rotate[1].to_radians() * if flip[0] ^ flip[2] { -1.0 } else { 1.0 },
        rotate[2].to_radians() * if flip[0] ^ flip[1] { -1.0 } else { 1.0 },
    ];
    let mirror = part
        .get("mirrorTexture")
        .and_then(Value::as_str)
        .unwrap_or("");
    let scale = part.get("scale").and_then(Value::as_f64).unwrap_or(1.0) as f32;
    let boxes = part
        .get("boxes")
        .and_then(Value::as_array)
        .map(|boxes| {
            boxes
                .iter()
                .filter_map(|b| parse_box(b, flip, skipped))
                .collect()
        })
        .unwrap_or_default();
    let index = model.parts.len();
    model.parts.push(CemPart {
        name,
        vanilla,
        parent,
        translate,
        rotate,
        scale: [scale; 3],
        mirror_u: mirror.contains('u'),
        mirror_v: mirror.contains('v'),
        boxes,
        attach: part.get("attach").and_then(Value::as_bool).unwrap_or(false),
        visible: true,
    });
    for key in part.as_object().into_iter().flat_map(|o| o.keys()) {
        if ![
            "part",
            "id",
            "invertAxis",
            "translate",
            "rotate",
            "mirrorTexture",
            "boxes",
            "submodel",
            "submodels",
            "animations",
            "attach",
            "model",
            "scale",
            "baseId",
            "texture",
            "textureSize",
        ]
        .contains(&key.as_str())
        {
            let note = format!("{key} on {}", model.parts[index].name);
            if !skipped.contains(&note) {
                skipped.push(note);
            }
        }
    }
    // Animations: a list of objects of `target: expression`, in order.
    let mut lines = Vec::new();
    if let Some(list) = part.get("animations").and_then(Value::as_array) {
        for entry in list {
            for (target, expression) in entry.as_object().into_iter().flatten() {
                match expression {
                    Value::String(text) => lines.push((target.clone(), text.clone())),
                    Value::Number(n) => lines.push((target.clone(), n.to_string())),
                    Value::Bool(b) => lines.push((target.clone(), b.to_string())),
                    _ => {}
                }
            }
        }
    }
    animations.push((index, lines));
    let subs = part
        .get("submodels")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .chain(part.get("submodel"));
    for sub in subs {
        add_part(
            sub,
            Some(index),
            read,
            model,
            animations,
            skipped,
            depth + 1,
        )?;
    }
    Ok(())
}

fn parse_box(b: &Value, flip: [bool; 3], skipped: &mut Vec<String>) -> Option<CemBox> {
    let c = b.get("coordinates").and_then(numbers::<6>)?;
    let size = [c[3], c[4], c[5]];
    let mut from = [c[0], c[1], c[2]];
    // A turned-over axis puts the box the other side of the pivot.
    for axis in 0..3 {
        if flip[axis] {
            from[axis] = -(from[axis] + size[axis]);
        }
    }
    let faces = ["uvNorth", "uvEast", "uvSouth", "uvWest", "uvUp", "uvDown"]
        .map(|key| b.get(key).and_then(numbers::<4>));
    for key in b.as_object().into_iter().flat_map(|o| o.keys()) {
        if ![
            "coordinates",
            "textureOffset",
            "sizeAdd",
            "uvNorth",
            "uvEast",
            "uvSouth",
            "uvWest",
            "uvUp",
            "uvDown",
        ]
        .contains(&key.as_str())
        {
            let note = format!("box {key}");
            if !skipped.contains(&note) {
                skipped.push(note);
            }
        }
    }
    Some(CemBox {
        from,
        size,
        offset: b.get("textureOffset").and_then(numbers::<2>),
        faces,
        grow: b.get("sizeAdd").and_then(Value::as_f64).unwrap_or(0.0) as f32,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A model written for these tests (not Fresh Animations').
    const PIG: &str = r#"{
        "texture": "pig.png",
        "textureSize": [64, 32],
        "shadow_size": 0.7,
        "mystery": true,
        "models": [
            {
                "part": "head", "id": "head", "invertAxis": "xy",
                "translate": [0, -12, 6],
                "boxes": [
                    {"coordinates": [-4, -4, -8, 8, 8, 8], "textureOffset": [0, 0]},
                    {"coordinates": [-2, -1, -9, 4, 3, 1], "textureOffset": [16, 16], "sizeAdd": 0.1}
                ],
                "submodels": [
                    {"id": "ear", "invertAxis": "xy", "translate": [3, 4, 0], "rotate": [0, 0, 30], "mirrorTexture": "u",
                     "boxes": [{"coordinates": [0, 0, 0, 2, 2, 1], "uvEast": [1, 2, 3, 4]}],
                     "animations": [{"ear.rz": "sin(age * 0.2) * 0.3"}]}
                ],
                "animations": [
                    {"var.look": "head_yaw * 0.5", "head.ry": "torad(var.look)"},
                    {"head.rx": "torad(head_pitch)"}
                ]
            },
            {
                "part": "leg1", "id": "leg1", "invertAxis": "xy", "translate": [3, -6, -7],
                "boxes": [{"coordinates": [-2, -6, -2, 4, 6, 4], "textureOffset": [0, 16]}],
                "animations": [{"leg1.rx": "cos(limb_swing * 0.6662) * 1.4 * limb_speed", "varb.moving": "limb_speed > 0.1", "bogus.rx": "1"}]
            }
        ]
    }"#;

    fn load(text: &str) -> (CemModel, Vec<String>) {
        let mut log = Vec::new();
        let model = parse(text, &|_| None, &mut |l| log.push(l)).unwrap();
        (model, log)
    }

    #[test]
    fn a_fixture_loads_with_its_part_tree_pivots_and_uvs() {
        let (model, log) = load(PIG);
        assert_eq!(model.texture.as_deref(), Some("pig.png"));
        assert_eq!(model.texture_size, (64.0, 32.0));
        assert_eq!(model.shadow_size, 0.7);
        let names: Vec<&str> = model.parts.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(
            names,
            ["head", "ear", "leg1"],
            "a parent before its children"
        );
        assert_eq!(model.parts[1].parent, Some(0));
        assert_eq!(model.parts[2].parent, None);
        // invertAxis "xy" turns the pivot's x and y over.
        assert_eq!(model.parts[0].translate, [0.0, 12.0, 6.0]);
        assert_eq!(model.parts[2].translate, [-3.0, 6.0, -7.0]);
        // Boxes: from is turned over to the other side, size is kept.
        let head = &model.parts[0].boxes[0];
        assert_eq!(
            (head.from, head.size),
            ([4.0 - 8.0, -4.0, -8.0], [8.0, 8.0, 8.0])
        );
        assert_eq!(head.offset, Some([0.0, 0.0]));
        let leg = &model.parts[2].boxes[0];
        assert_eq!(
            leg.from,
            [-2.0, 0.0, -2.0],
            "a leg hangs down from its pivot"
        );
        assert_eq!(model.parts[0].boxes[1].grow, 0.1);
        // Per-face uvs and the mirror flag.
        let ear = &model.parts[1];
        assert!(ear.mirror_u && !ear.mirror_v);
        assert_eq!(ear.boxes[0].faces[1], Some([1.0, 2.0, 3.0, 4.0]));
        assert_eq!(ear.boxes[0].faces[0], None);
        // The ear's rotation is in radians (and flipped for the axes).
        assert!((ear.rotate[2].abs() - 30f32.to_radians()).abs() < 1e-5);
        // Fields the loader doesn't read are said once, never a crash.
        assert_eq!(log.len(), 1, "{log:?}");
        assert!(log[0].contains("mystery"), "{log:?}");
    }

    #[test]
    fn animations_keep_their_order_and_know_their_targets() {
        let (model, _) = load(PIG);
        // head: var.look, head.ry, head.rx; ear: ear.rz; leg1: leg1.rx and
        // varb.moving (bogus.rx names no part).
        let targets: Vec<Slot> = model.animations.iter().map(|a| a.target).collect();
        assert_eq!(
            targets,
            [
                Slot::Var(0),
                Slot::Part(0, PartField::Ry),
                Slot::Part(0, PartField::Rx),
                Slot::Part(1, PartField::Rz),
                Slot::Part(2, PartField::Rx),
                Slot::Var(1),
            ]
        );
        assert_eq!(model.var_count, 2);
        assert_eq!(model.var_names, ["var.look", "varb.moving"]);
        assert_eq!(model.unknown, ["bogus.rx"]);
    }

    #[test]
    fn a_part_file_is_merged_in_and_bad_files_are_errors_not_panics() {
        let jem = r#"{"textureSize": [16, 16], "models": [{"part": "body", "id": "body", "model": "body.jpm"}]}"#;
        let jpm = r#"{"invertAxis": "xy", "translate": [0, -12, 0], "boxes": [{"coordinates": [-4, -12, -2, 8, 12, 4], "textureOffset": [16, 16]}]}"#;
        let read = |path: &str| (path == "body.jpm").then(|| jpm.to_owned());
        let model = parse(jem, &read, &mut |_| {}).unwrap();
        assert_eq!(model.parts[0].boxes.len(), 1);
        assert_eq!(model.parts[0].translate, [0.0, 12.0, 0.0]);
        assert!(parse("not json", &|_| None, &mut |_| {}).is_err());
        assert!(parse("{}", &|_| None, &mut |_| {}).is_err());
        // A missing part file just leaves the part empty.
        let model = parse(jem, &|_| None, &mut |_| {}).unwrap();
        assert!(model.parts[0].boxes.is_empty());
        // Bad animation lines are skipped, the rest kept.
        let broken = r#"{"models": [{"id": "a", "animations": [{"a.rx": "1 +", "a.ry": "2"}]}]}"#;
        let (model, log) = load(broken);
        assert_eq!(model.animations.len(), 1);
        assert!(log[0].contains("a.rx"), "{log:?}");
    }

    #[test]
    fn parts_can_be_found_by_their_id_or_the_vanilla_part_they_replace() {
        let (model, _) = load(PIG);
        assert_eq!(model.part_named("ear"), Some(1));
        assert_eq!(model.replaces("head"), Some(0));
        assert_eq!(model.replaces("leg1"), Some(2));
        assert_eq!(model.replaces("tail"), None);
    }
}
