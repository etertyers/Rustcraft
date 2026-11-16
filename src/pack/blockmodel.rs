//! Block models drawn from model JSON `elements` (M61 fixes).
//!
//! Most blocks keep their built-in shapes. Blocks with `Shape::Model`
//! (the placeholders, whose boxes were flattened from vanilla's models) and
//! the blocks with `json_model: true` in `blocks.ron` are drawn from the
//! real model instead: the blockstate file picks a model for each state
//! (`variants`, or the matching `multipart` pieces), the model's `parent`
//! chain gives its textures and elements, and each face becomes a quad the
//! way vanilla's `FaceBakery` makes one: element rotation (with `rescale`),
//! the blockstate's `x` then `y` rotation about the block's center, per-face
//! `uv`, `rotation`, `cullface`, `tintindex` and `shade`.
//!
//! A state with no elements (a plain `cube_all`) gets no baked quads and
//! keeps its built-in look. Collision and outline boxes are unchanged.

use std::{
    collections::{BTreeSet, HashMap},
    sync::Arc,
};

use serde_json::Value;

use super::{Resources, resources::location};
use crate::world::{
    Face,
    block::{BlockDef, BlockRegistry, BlockState, Shape},
};

/// One face of a model, as vanilla bakes it. Corners are block-relative
/// (0–1) ordered bottom-left, bottom-right, top-right, top-left seen from
/// the front, with a texture coordinate (0–1 within the texture) each.
#[derive(Debug, Clone, PartialEq)]
pub struct RawQuad {
    pub corners: [[f32; 3]; 4],
    pub uvs: [[f32; 2]; 4],
    /// A texture location: `block/candle`.
    pub texture: String,
    /// Hidden when the neighbor on this side hides it.
    pub cull: Option<Face>,
    pub tinted: bool,
    /// Vanilla's directional shading, 0–1.
    pub shade: f32,
}

/// Every baked state, by `BlockRegistry::state_index`.
#[derive(Debug, Default)]
pub struct JsonModels {
    states: Vec<Option<Arc<Vec<RawQuad>>>>,
    covered: usize,
}

type Vec3 = [f32; 3];

/// Longest `parent` chain followed.
const MAX_DEPTH: usize = 16;

impl JsonModels {
    /// Bakes every block that draws from its JSON model.
    pub fn load(res: &Resources, registry: &BlockRegistry) -> Self {
        let mut loader = Loader {
            res,
            models: HashMap::new(),
        };
        let mut states = Vec::with_capacity(registry.state_count());
        let mut covered = 0;
        let mut blockstates: HashMap<String, Option<Value>> = HashMap::new();
        for state in registry.states() {
            let def = state.def();
            let wanted = def.shape == Shape::Model || def.json_model;
            let baked = wanted
                .then(|| {
                    let json = blockstates
                        .entry(def.name.clone())
                        .or_insert_with(|| loader.json(&format!("blockstates/{}.json", def.name)))
                        .as_ref()?;
                    loader.bake_state(def, state, json)
                })
                .flatten()
                .filter(|quads| !quads.is_empty());
            covered += usize::from(baked.is_some());
            states.push(baked.map(Arc::new));
        }
        Self { states, covered }
    }

    /// The quads of the state at `index`, if its model has elements.
    pub fn get(&self, index: usize) -> Option<&Arc<Vec<RawQuad>>> {
        self.states.get(index)?.as_ref()
    }

    /// Every texture the baked states use.
    pub fn textures(&self) -> BTreeSet<String> {
        self.states
            .iter()
            .flatten()
            .flat_map(|quads| quads.iter().map(|q| q.texture.clone()))
            .collect()
    }

    /// How many states are drawn from JSON.
    pub fn len(&self) -> usize {
        self.covered
    }

    pub fn is_empty(&self) -> bool {
        self.covered == 0
    }
}

/// A model with its parents folded in.
#[derive(Debug, Clone)]
struct Resolved {
    textures: HashMap<String, String>,
    elements: Vec<Value>,
}

struct Loader<'a> {
    res: &'a Resources,
    models: HashMap<String, Option<Resolved>>,
}

impl Loader<'_> {
    /// A JSON file from the highest layer, vanilla last.
    fn json(&self, path: &str) -> Option<Value> {
        self.res.json(path).or_else(|| self.res.vanilla_json(path))
    }

    /// `block/candle` with its parents; the nearest model's elements win,
    /// and the nearest textures.
    fn model(&mut self, name: &str) -> Option<Resolved> {
        let path = model_path(name);
        if let Some(found) = self.models.get(&path) {
            return found.clone();
        }
        let mut textures: HashMap<String, String> = HashMap::new();
        let mut elements: Option<Vec<Value>> = None;
        let mut next = Some(path.clone());
        let mut found = false;
        for _ in 0..MAX_DEPTH {
            let Some(name) = next.take() else { break };
            if name.starts_with("builtin/") {
                break;
            }
            let Some(json) = self.json(&format!("models/{name}.json")) else {
                break;
            };
            found = true;
            if let Some(vars) = json.get("textures").and_then(Value::as_object) {
                for (key, value) in vars {
                    if let Some(value) = value.as_str() {
                        textures
                            .entry(key.clone())
                            .or_insert_with(|| value.to_owned());
                    }
                }
            }
            if elements.is_none()
                && let Some(list) = json.get("elements").and_then(Value::as_array)
            {
                elements = Some(list.clone());
            }
            next = json.get("parent").and_then(Value::as_str).map(model_path);
        }
        let resolved = found.then(|| Resolved {
            textures,
            elements: elements.unwrap_or_default(),
        });
        self.models.insert(path, resolved.clone());
        resolved
    }

    /// The quads of one state: the first matching variant, or the
    /// matching multipart pieces.
    fn bake_state(
        &mut self,
        def: &BlockDef,
        state: BlockState,
        json: &Value,
    ) -> Option<Vec<RawQuad>> {
        let mut quads = Vec::new();
        if let Some(variants) = json.get("variants").and_then(Value::as_object) {
            let apply = variants
                .iter()
                .find(|(key, _)| variant_matches(def, state, key))
                .map(|(_, apply)| apply)?;
            self.bake_apply(apply, &mut quads);
        } else if let Some(parts) = json.get("multipart").and_then(Value::as_array) {
            for part in parts {
                let applies = part
                    .get("when")
                    .is_none_or(|when| when_matches(def, state, when));
                if applies && let Some(apply) = part.get("apply") {
                    self.bake_apply(apply, &mut quads);
                }
            }
        }
        Some(quads)
    }

    /// One `apply` / variant value: a model with a rotation (the first of
    /// a weighted list).
    fn bake_apply(&mut self, apply: &Value, out: &mut Vec<RawQuad>) {
        let apply = match apply {
            Value::Array(list) => match list.first() {
                Some(first) => first,
                None => return,
            },
            other => other,
        };
        let Some(name) = apply.get("model").and_then(Value::as_str) else {
            return;
        };
        let Some(model) = self.model(name) else {
            return;
        };
        let turn = |key: &str| apply.get(key).and_then(Value::as_i64).unwrap_or(0) as i32;
        let (x, y) = (turn("x"), turn("y"));
        for element in &model.elements {
            bake_element(element, &model.textures, x, y, out);
        }
    }
}

/// `minecraft:block/stone` → `block/stone`; names without a folder are
/// under `block/`.
fn model_path(name: &str) -> String {
    let name = location(name);
    if name.contains('/') {
        name.to_owned()
    } else {
        format!("block/{name}")
    }
}

/// Whether `state` meets a `variants` key (`facing=north,half=top`);
/// properties the block doesn't have don't rule it out.
fn variant_matches(def: &BlockDef, state: BlockState, key: &str) -> bool {
    key.split(',')
        .filter_map(|pair| pair.split_once('='))
        .all(|(name, value)| {
            def.props
                .iter()
                .find(|prop| prop.name() == name)
                .is_none_or(|&prop| prop.values()[state.get(prop) as usize] == value)
        })
}

/// A multipart `when`: property lists (`"east": "low|tall"`), `OR` and
/// `AND`. A property the block doesn't have never matches.
fn when_matches(def: &BlockDef, state: BlockState, when: &Value) -> bool {
    let Some(map) = when.as_object() else {
        return false;
    };
    map.iter().all(|(key, value)| match key.as_str() {
        "OR" => value
            .as_array()
            .is_some_and(|list| list.iter().any(|w| when_matches(def, state, w))),
        "AND" => value
            .as_array()
            .is_some_and(|list| list.iter().all(|w| when_matches(def, state, w))),
        name => {
            let wanted = match value {
                Value::String(s) => s.clone(),
                other => other.to_string(),
            };
            let have = match def.props.iter().find(|prop| prop.name() == name) {
                Some(&prop) => prop.values()[state.get(prop) as usize],
                // The placeholder blocks have a single state: the one
                // with vanilla's defaults.
                None => default_value(name),
            };
            wanted.split('|').any(|w| w == have)
        }
    })
}

/// A block state property's value in vanilla's default state, for blocks
/// that don't have the property yet.
fn default_value(name: &str) -> &'static str {
    match name {
        "facing" => "north",
        "rotation" => "0",
        "candles" => "1",
        "side_chain" => "unconnected",
        "axis" => "y",
        "half" | "type" => "bottom",
        "face" => "floor",
        "tilt" => "none",
        "thickness" => "tip",
        "vertical_direction" => "up",
        "sculk_sensor_phase" => "inactive",
        "orientation" => "north_up",
        "attachment" => "floor",
        "berries" => "false",
        "age" | "stage" | "level" | "power" | "delay" | "bites" | "layers" | "eggs" | "hatch" => {
            "0"
        }
        // Every other property in these files is a flag.
        _ => "false",
    }
}

/// A texture variable (`#all`) resolved to a texture location.
fn resolve_texture(textures: &HashMap<String, String>, name: &str) -> Option<String> {
    let mut value = name;
    for _ in 0..MAX_DEPTH {
        match value.strip_prefix('#') {
            Some(other) => value = textures.get(other)?,
            // heavy_core names its slot without the `#`.
            None if textures.contains_key(value) => value = &textures[value],
            None => return Some(location(value).to_owned()),
        }
    }
    None
}

fn number(value: &Value) -> f32 {
    value.as_f64().unwrap_or(0.0) as f32
}

fn vec3(value: Option<&Value>) -> Option<Vec3> {
    let list = value?.as_array()?;
    (list.len() == 3).then(|| [number(&list[0]), number(&list[1]), number(&list[2])])
}

const DIRECTIONS: [(&str, Face); 6] = [
    ("down", Face::Bottom),
    ("up", Face::Top),
    ("north", Face::North),
    ("south", Face::South),
    ("west", Face::West),
    ("east", Face::East),
];

/// Vanilla's corner order for a face of the box `from`..`to` (top-left,
/// bottom-left, bottom-right, top-right of the texture).
fn face_vertices(face: Face, from: Vec3, to: Vec3) -> [Vec3; 4] {
    let [x0, y0, z0] = from;
    let [x1, y1, z1] = to;
    match face {
        Face::Bottom => [[x0, y0, z1], [x0, y0, z0], [x1, y0, z0], [x1, y0, z1]],
        Face::Top => [[x0, y1, z0], [x0, y1, z1], [x1, y1, z1], [x1, y1, z0]],
        Face::North => [[x1, y1, z0], [x1, y0, z0], [x0, y0, z0], [x0, y1, z0]],
        Face::South => [[x0, y1, z1], [x0, y0, z1], [x1, y0, z1], [x1, y1, z1]],
        Face::West => [[x0, y1, z0], [x0, y0, z0], [x0, y0, z1], [x0, y1, z1]],
        Face::East => [[x1, y1, z1], [x1, y0, z1], [x1, y0, z0], [x1, y1, z0]],
    }
}

/// The texture rectangle `[u0, v0, u1, v1]` (0–16) a face shows when its
/// model gives no `uv`.
fn default_uv(face: Face, from: Vec3, to: Vec3) -> [f32; 4] {
    let [x0, y0, z0] = from;
    let [x1, y1, z1] = to;
    match face {
        Face::Bottom => [x0, 16.0 - z1, x1, 16.0 - z0],
        Face::Top => [x0, z0, x1, z1],
        Face::North => [16.0 - x1, 16.0 - y1, 16.0 - x0, 16.0 - y0],
        Face::South => [x0, 16.0 - y1, x1, 16.0 - y0],
        Face::West => [z0, 16.0 - y1, z1, 16.0 - y0],
        Face::East => [16.0 - z1, 16.0 - y1, 16.0 - z0, 16.0 - y0],
    }
}

/// Rotates `p` by `degrees` around `axis` (0 x, 1 y, 2 z) through `origin`,
/// right-handed.
fn rotate(p: Vec3, origin: Vec3, axis: usize, degrees: f32) -> Vec3 {
    let (s, c) = degrees.to_radians().sin_cos();
    let [x, y, z] = [p[0] - origin[0], p[1] - origin[1], p[2] - origin[2]];
    let r = match axis {
        0 => [x, y * c - z * s, y * s + z * c],
        1 => [x * c + z * s, y, -x * s + z * c],
        _ => [x * c - y * s, x * s + y * c, z],
    };
    [r[0] + origin[0], r[1] + origin[1], r[2] + origin[2]]
}

/// A blockstate's `x` and `y` rotations (vanilla turns them against the
/// right-handed direction): x first, then y.
fn state_rotate(p: Vec3, x: i32, y: i32) -> Vec3 {
    let center = [8.0; 3];
    let p = rotate(p, center, 0, -(x as f32));
    rotate(p, center, 1, -(y as f32))
}

/// The face a unit direction points to, if it's an axis.
fn face_of(n: Vec3) -> Option<Face> {
    let near = |v: f32| (v.abs() - 1.0).abs() < 1e-3;
    let zero = |v: f32| v.abs() < 1e-3;
    match n {
        [x, y, z] if near(x) && zero(y) && zero(z) => {
            Some(if x > 0.0 { Face::East } else { Face::West })
        }
        [x, y, z] if zero(x) && near(y) && zero(z) => {
            Some(if y > 0.0 { Face::Top } else { Face::Bottom })
        }
        [x, y, z] if zero(x) && zero(y) && near(z) => {
            Some(if z > 0.0 { Face::South } else { Face::North })
        }
        _ => None,
    }
}

fn bake_element(
    element: &Value,
    textures: &HashMap<String, String>,
    state_x: i32,
    state_y: i32,
    out: &mut Vec<RawQuad>,
) {
    let (Some(from), Some(to)) = (vec3(element.get("from")), vec3(element.get("to"))) else {
        return;
    };
    let faces = match element.get("faces").and_then(Value::as_object) {
        Some(faces) => faces,
        None => return,
    };
    let shaded = element
        .get("shade")
        .and_then(Value::as_bool)
        .unwrap_or(true);
    let rotation = element.get("rotation").and_then(|r| {
        let origin = vec3(r.get("origin"))?;
        let axis = match r.get("axis")?.as_str()? {
            "x" => 0,
            "y" => 1,
            _ => 2,
        };
        let angle = number(r.get("angle")?);
        let rescale = r.get("rescale").and_then(Value::as_bool).unwrap_or(false);
        Some((origin, axis, angle, rescale))
    });
    for (name, face) in DIRECTIONS {
        let Some(spec) = faces.get(name) else {
            continue;
        };
        let Some(texture) = spec
            .get("texture")
            .and_then(Value::as_str)
            .and_then(|t| resolve_texture(textures, t))
        else {
            continue;
        };
        let uv = match spec.get("uv").and_then(Value::as_array) {
            Some(list) if list.len() == 4 => [
                number(&list[0]),
                number(&list[1]),
                number(&list[2]),
                number(&list[3]),
            ],
            _ => default_uv(face, from, to),
        };
        let turn =
            (spec.get("rotation").and_then(Value::as_i64).unwrap_or(0) / 90).rem_euclid(4) as usize;
        let vertex_uv = |index: usize| {
            let shifted = (index + turn) % 4;
            let u = if shifted == 0 || shifted == 1 {
                uv[0]
            } else {
                uv[2]
            };
            let v = if shifted == 0 || shifted == 3 {
                uv[1]
            } else {
                uv[3]
            };
            [u / 16.0, v / 16.0]
        };
        let mut corners = face_vertices(face, from, to);
        if let Some((origin, axis, angle, rescale)) = rotation {
            for corner in &mut corners {
                let mut p = rotate(*corner, origin, axis, angle);
                if rescale {
                    let k = 1.0 / angle.to_radians().cos().abs().max(1e-3);
                    for (i, value) in p.iter_mut().enumerate() {
                        if i != axis {
                            *value = origin[i] + (*value - origin[i]) * k;
                        }
                    }
                }
                *corner = p;
            }
        }
        for corner in &mut corners {
            *corner = state_rotate(*corner, state_x, state_y);
        }
        // Vanilla's order is top-left, bottom-left, bottom-right,
        // top-right; the mesher's is bottom-left, bottom-right, top-right,
        // top-left.
        let order = [1, 2, 3, 0];
        let unit = |p: Vec3| [p[0] / 16.0, p[1] / 16.0, p[2] / 16.0];
        let quad_corners = order.map(|i| unit(corners[i]));
        let uvs = order.map(vertex_uv);
        // The face's normal after all the turns.
        let [a, b, c, _] = quad_corners.map(glam::Vec3::from);
        let normal = (b - a).cross(c - a).normalize_or_zero();
        let shade = if shaded {
            let n = normal.to_array();
            n[0] * n[0] * Face::East.shade()
                + n[1]
                    * n[1]
                    * if n[1] >= 0.0 {
                        1.0
                    } else {
                        Face::Bottom.shade()
                    }
                + n[2] * n[2] * Face::North.shade()
        } else {
            1.0
        };
        let cull = spec
            .get("cullface")
            .and_then(Value::as_str)
            .and_then(|name| DIRECTIONS.iter().find(|(n, _)| *n == name))
            .and_then(|&(_, face)| {
                let normal = face.normal();
                let turned = state_rotate_dir(
                    [normal[0] as f32, normal[1] as f32, normal[2] as f32],
                    state_x,
                    state_y,
                );
                face_of(turned)
            });
        out.push(RawQuad {
            corners: quad_corners,
            uvs,
            texture,
            cull,
            tinted: spec
                .get("tintindex")
                .and_then(Value::as_i64)
                .is_some_and(|i| i >= 0),
            shade,
        });
    }
}

/// `state_rotate` for a direction (around the origin).
fn state_rotate_dir(d: Vec3, x: i32, y: i32) -> Vec3 {
    let p = rotate(d, [0.0; 3], 0, -(x as f32));
    rotate(p, [0.0; 3], 1, -(y as f32))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn element(json: &str) -> Value {
        serde_json::from_str(json).unwrap()
    }

    fn textures() -> HashMap<String, String> {
        HashMap::from([("all".to_owned(), "minecraft:block/stone".to_owned())])
    }

    #[test]
    fn a_full_box_gives_six_faces_that_face_outward() {
        let e = element(
            r##"{"from":[0,0,0],"to":[16,16,16],"faces":{
            "down":{"texture":"#all","cullface":"down"},"up":{"texture":"#all","cullface":"up"},
            "north":{"texture":"#all","cullface":"north"},"south":{"texture":"#all","cullface":"south"},
            "west":{"texture":"#all","cullface":"west"},"east":{"texture":"#all","cullface":"east"}}}"##,
        );
        let mut quads = Vec::new();
        bake_element(&e, &textures(), 0, 0, &mut quads);
        assert_eq!(quads.len(), 6);
        for quad in &quads {
            let [a, b, c, _] = quad.corners.map(glam::Vec3::from);
            let normal = (b - a).cross(c - a).normalize();
            let center = quad
                .corners
                .iter()
                .fold(glam::Vec3::ZERO, |s, p| s + glam::Vec3::from(*p))
                / 4.0
                - glam::Vec3::splat(0.5);
            assert!(normal.dot(center) > 0.0, "{:?} faces inward", quad.cull);
            assert_eq!(quad.texture, "block/stone");
            // The corner uvs span the whole texture.
            let us: Vec<f32> = quad.uvs.iter().map(|uv| uv[0]).collect();
            assert!(us.contains(&0.0) && us.contains(&1.0));
        }
        let top = quads.iter().find(|q| q.cull == Some(Face::Top)).unwrap();
        assert_eq!(top.shade, 1.0);
        let bottom = quads.iter().find(|q| q.cull == Some(Face::Bottom)).unwrap();
        assert_eq!(bottom.shade, 0.5);
    }

    #[test]
    fn default_uvs_show_the_part_of_the_texture_a_face_covers() {
        // A 2×4×2 post at the middle: its south face shows x 7–9 across and
        // the top part of its height down.
        let e = element(r##"{"from":[7,0,7],"to":[9,4,9],"faces":{"south":{"texture":"#all"}}}"##);
        let mut quads = Vec::new();
        bake_element(&e, &textures(), 0, 0, &mut quads);
        let uvs = quads[0].uvs;
        // Bottom-left corner: u 7/16, v 16/16; top-right: u 9/16, v 12/16.
        assert_eq!(uvs[0], [7.0 / 16.0, 1.0]);
        assert_eq!(uvs[2], [9.0 / 16.0, 12.0 / 16.0]);
    }

    #[test]
    fn a_blockstate_x_turn_tips_the_top_toward_the_north_like_a_piston() {
        let e = element(
            r##"{"from":[0,0,0],"to":[16,16,16],"faces":{"up":{"texture":"#all","cullface":"up"}}}"##,
        );
        let mut quads = Vec::new();
        bake_element(&e, &textures(), 90, 0, &mut quads);
        assert_eq!(quads[0].cull, Some(Face::North));
        let mut quads = Vec::new();
        bake_element(&e, &textures(), 0, 90, &mut quads);
        // A y turn leaves the top where it is.
        assert_eq!(quads[0].cull, Some(Face::Top));
        // And turns a north face clockwise seen from above, to the east.
        let e = element(
            r##"{"from":[0,0,0],"to":[16,16,16],"faces":{"north":{"texture":"#all","cullface":"north"}}}"##,
        );
        let mut quads = Vec::new();
        bake_element(&e, &textures(), 0, 90, &mut quads);
        assert_eq!(quads[0].cull, Some(Face::East));
    }

    #[test]
    fn element_rotation_tilts_a_plane() {
        let e = element(
            r##"{"from":[0,0,8],"to":[16,16,8],"rotation":{"origin":[8,8,8],"axis":"x","angle":45},
            "faces":{"south":{"texture":"#all"}}}"##,
        );
        let mut quads = Vec::new();
        bake_element(&e, &textures(), 0, 0, &mut quads);
        let ys: Vec<f32> = quads[0].corners.iter().map(|c| c[1]).collect();
        let zs: Vec<f32> = quads[0].corners.iter().map(|c| c[2]).collect();
        assert!(zs.iter().any(|&z| (z - 0.5).abs() > 0.1), "{zs:?}");
        assert!(ys.iter().all(|y| (0.0..=1.0).contains(y)));
        // Not facing a single axis any more: its shade blends.
        assert!(quads[0].shade < 0.8 && quads[0].shade > 0.5);
    }

    #[test]
    fn a_face_rotation_turns_the_texture() {
        let plain = element(
            r##"{"from":[0,0,0],"to":[16,16,16],"faces":{"up":{"texture":"#all","uv":[0,0,8,16]}}}"##,
        );
        let turned = element(
            r##"{"from":[0,0,0],"to":[16,16,16],"faces":{"up":{"texture":"#all","uv":[0,0,8,16],"rotation":90}}}"##,
        );
        let (mut a, mut b) = (Vec::new(), Vec::new());
        bake_element(&plain, &textures(), 0, 0, &mut a);
        bake_element(&turned, &textures(), 0, 0, &mut b);
        assert_ne!(a[0].uvs, b[0].uvs);
        let mut sorted_a = a[0].uvs.to_vec();
        let mut sorted_b = b[0].uvs.to_vec();
        sorted_a.sort_by(|p, q| p.partial_cmp(q).unwrap());
        sorted_b.sort_by(|p, q| p.partial_cmp(q).unwrap());
        assert_eq!(sorted_a, sorted_b);
    }

    #[test]
    fn variables_follow_references() {
        let vars = HashMap::from([
            ("a".to_owned(), "#b".to_owned()),
            ("b".to_owned(), "minecraft:block/candle".to_owned()),
        ]);
        assert_eq!(
            resolve_texture(&vars, "#a").as_deref(),
            Some("block/candle")
        );
        assert_eq!(resolve_texture(&vars, "#missing"), None);
    }

    /// Bakes the real vanilla files, if downloaded.
    fn vanilla() -> Option<(Resources, JsonModels)> {
        let assets = crate::assets::dir();
        if !crate::platform::fs::is_dir(assets.join("vanilla")) {
            eprintln!("skipped: assets/vanilla isn't downloaded");
            return None;
        }
        let res = Resources::load(&assets, Vec::new()).ok()?;
        let models = JsonModels::load(&res, crate::world::block::registry());
        Some((res, models))
    }

    #[test]
    fn the_marked_blocks_bake_from_vanilla_models_with_textures_that_exist() {
        use crate::world::block::registry;
        let Some((res, models)) = vanilla() else {
            return;
        };
        let registry = registry();
        let quads_of = |name: &str| -> usize {
            registry
                .states()
                .filter(|s| s.def().name == name)
                .map(|s| models.get(registry.state_index(s)).map_or(0, |q| q.len()))
                .max()
                .unwrap_or(0)
        };
        for name in [
            "white_candle",
            "iron_chain",
            "copper_lantern",
            "lightning_rod",
            "oak_shelf",
            "oak_sign",
            "oak_wall_hanging_sign",
            "potted_poppy",
            "brain_coral_fan",
            "tube_coral_wall_fan",
            "spore_blossom",
            "big_dripleaf",
            "copper_bars",
            "turtle_egg",
        ] {
            if registry.iter().any(|d| d.name == name) {
                assert!(quads_of(name) > 0, "{name} bakes no quads");
            }
        }
        for texture in models.textures() {
            assert!(
                res.texture(&texture).is_some(),
                "{texture} is named by a baked model but isn't in any layer"
            );
        }
        assert!(models.len() > 500, "only {} states baked", models.len());
    }
}
