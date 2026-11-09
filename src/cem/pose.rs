//! Running a model's animations and drawing its parts (M58): each frame
//! the parts go back to their resting pose, the animation lines run in
//! file order (reading the entity's variables, the variables the model
//! keeps for this mob, and the parts as they stand), and the boxes are
//! placed through each part's chain of turns.

use glam::{Mat4, Vec3};

use super::{
    expr::{PartField, RenderField, Slot},
    model::CemModel,
    vars::EntityVars,
};
use crate::world::mesh::Mesh;

/// A part's pose this frame.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PartState {
    pub translate: [f32; 3],
    pub rotate: [f32; 3],
    pub scale: [f32; 3],
    pub visible: bool,
}

/// What one mob keeps of a model between frames.
#[derive(Debug, Clone, PartialEq)]
pub struct Runtime {
    /// `var.` and `varb.` values, by slot.
    pub vars: Vec<f32>,
    pub parts: Vec<PartState>,
    /// `render.shadow_size`, `render.shadow_opacity`, `render.model_scale`.
    pub render: [f32; 3],
    random: u64,
    /// The frame this was last used in, to forget mobs that are gone.
    pub last_used: u64,
    /// The frame it was last animated in.
    pub animated_in: Option<u64>,
}

impl Runtime {
    /// A mob's state for `model`, its random numbers started from `seed`
    /// (its id), every part at rest.
    pub fn new(model: &CemModel, seed: u64) -> Self {
        let mut runtime = Self {
            vars: vec![0.0; model.var_count],
            parts: Vec::new(),
            render: [model.shadow_size, 1.0, 1.0],
            random: seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1,
            last_used: 0,
            animated_in: None,
        };
        runtime.rest(model);
        runtime
    }

    /// The state of a layer's model (a sheep's wool) moving with this one:
    /// each of its parts takes the pose of the part of `main` with the
    /// same name, so a layer is posed from the same values as the body.
    /// Parts `main` doesn't have stay at rest, on their parents.
    pub fn following(&self, main: &CemModel, layer: &CemModel) -> Runtime {
        let mut out = Runtime::new(layer, 0);
        for (state, part) in out.parts.iter_mut().zip(&layer.parts) {
            if let Some(j) = main.part_named(&part.name) {
                *state = self.parts[j];
            }
        }
        out
    }

    fn rest(&mut self, model: &CemModel) {
        self.parts.clear();
        self.parts.extend(model.parts.iter().map(|p| PartState {
            // A top-level part's animated translate is an offset on its
            // own (the part keeps its file translate as its place, added in
            // `matrices`); a child's is its whole offset from its parent.
            translate: if p.parent.is_none() {
                [0.0; 3]
            } else {
                p.translate
            },
            rotate: p.rotate,
            scale: p.scale,
            visible: p.visible,
        }));
    }

    /// One frame: back to the resting pose, then every animation line in
    /// order.
    pub fn animate(&mut self, model: &CemModel, entity: &EntityVars) {
        self.rest(model);
        self.render = [model.shadow_size, 1.0, 1.0];
        for animation in &model.animations {
            let mut random = self.random;
            let value = {
                let parts = &self.parts;
                let kept = &self.vars;
                let render = &self.render;
                let mut draw = || {
                    random ^= random << 13;
                    random ^= random >> 7;
                    random ^= random << 17;
                    ((random >> 40) as f32) / (1u64 << 24) as f32
                };
                animation.program.eval(
                    &mut |slot| read(slot, entity, kept, parts, render),
                    &mut draw,
                )
            };
            self.random = random;
            self.write(model, animation.target, value);
        }
    }

    fn write(&mut self, model: &CemModel, slot: Slot, value: f32) {
        if !value.is_finite() {
            return;
        }
        match slot {
            Slot::Var(i) => {
                let boolean = model
                    .var_names
                    .get(usize::from(i))
                    .is_some_and(|n| n.starts_with("varb."));
                self.vars[usize::from(i)] = if boolean {
                    f32::from(value != 0.0)
                } else {
                    value
                };
            }
            Slot::Part(i, field) => {
                let Some(part) = self.parts.get_mut(usize::from(i)) else {
                    return;
                };
                match field {
                    PartField::Tx => part.translate[0] = value,
                    PartField::Ty => part.translate[1] = value,
                    PartField::Tz => part.translate[2] = value,
                    PartField::Rx => part.rotate[0] = value,
                    PartField::Ry => part.rotate[1] = value,
                    PartField::Rz => part.rotate[2] = value,
                    PartField::Sx => part.scale[0] = value,
                    PartField::Sy => part.scale[1] = value,
                    PartField::Sz => part.scale[2] = value,
                    PartField::Visible => part.visible = value != 0.0,
                }
            }
            Slot::Render(field) => match field {
                RenderField::ShadowSize => self.render[0] = value,
                RenderField::ShadowOpacity => self.render[1] = value,
                RenderField::ModelScale => self.render[2] = value,
            },
            // Entity variables can't be set.
            Slot::Entity(_) => {}
        }
    }

    /// Each part's matrix from model pixels to the mob's model space: its
    /// own pivot, turns and scale after its parent's. A part that is
    /// hidden (or has a hidden parent) gets `None`.
    pub fn matrices(&self, model: &CemModel) -> Vec<Option<Mat4>> {
        let mut out: Vec<Option<Mat4>> = Vec::with_capacity(self.parts.len());
        for (i, state) in self.parts.iter().enumerate() {
            let parent = match model.parts[i].parent {
                Some(p) => out[p],
                None => Some(Mat4::IDENTITY),
            };
            out.push(parent.filter(|_| state.visible).map(|parent| {
                let [rx, ry, rz] = state.rotate;
                let place = if model.parts[i].parent.is_none() {
                    Vec3::from(model.parts[i].translate)
                } else {
                    Vec3::ZERO
                };
                // Turned about z, then y, then x, like vanilla's parts.
                parent
                    * Mat4::from_translation(Vec3::from(state.translate) + place)
                    * Mat4::from_rotation_z(rz)
                    * Mat4::from_rotation_y(ry)
                    * Mat4::from_rotation_x(rx)
                    * Mat4::from_scale(Vec3::from(state.scale))
            }));
        }
        out
    }
}

fn read(
    slot: Slot,
    entity: &EntityVars,
    kept: &[f32],
    parts: &[PartState],
    render: &[f32; 3],
) -> f32 {
    match slot {
        Slot::Entity(i) => entity.0[usize::from(i)],
        Slot::Var(i) => kept.get(usize::from(i)).copied().unwrap_or(0.0),
        Slot::Part(i, field) => {
            let Some(part) = parts.get(usize::from(i)) else {
                return 0.0;
            };
            match field {
                PartField::Tx => part.translate[0],
                PartField::Ty => part.translate[1],
                PartField::Tz => part.translate[2],
                PartField::Rx => part.rotate[0],
                PartField::Ry => part.rotate[1],
                PartField::Rz => part.rotate[2],
                PartField::Sx => part.scale[0],
                PartField::Sy => part.scale[1],
                PartField::Sz => part.scale[2],
                PartField::Visible => f32::from(part.visible),
            }
        }
        Slot::Render(field) => match field {
            RenderField::ShadowSize => render[0],
            RenderField::ShadowOpacity => render[1],
            RenderField::ModelScale => render[2],
        },
    }
}

/// Face shades, as vanilla's boxes (the same order as `FACE_ORDER`).
const SHADES: [f32; 6] = [0.6, 0.6, 1.0, 0.5, 0.8, 0.8];

/// Adds `model`'s parts as posed by `runtime` to `mesh`, each corner
/// going through `place` (model pixels to the world). `skip` says which
/// parts the caller draws itself.
pub fn add_parts(
    mesh: &mut Mesh,
    model: &CemModel,
    runtime: &Runtime,
    place: impl Fn(Vec3) -> [f32; 3],
    tint: [f32; 3],
    light: [f32; 2],
) {
    let (tw, th) = model.texture_size;
    let matrices = runtime.matrices(model);
    for (part, matrix) in model.parts.iter().zip(&matrices) {
        let Some(matrix) = matrix else {
            continue;
        };
        for b in &part.boxes {
            let [w, h, d] = b.size;
            let (mut x1, mut x2) = (b.from[0] - b.grow, b.from[0] + w + b.grow);
            let (y1, y2) = (b.from[1] - b.grow, b.from[1] + h + b.grow);
            let (z1, z2) = (b.from[2] - b.grow, b.from[2] + d + b.grow);
            // A box on a part with a negative scale would show its
            // insides; flipping x keeps it right.
            if matrix.determinant() < 0.0 {
                std::mem::swap(&mut x1, &mut x2);
            }
            let at = |x: f32, y: f32, z: f32| matrix.transform_point3(Vec3::new(x, y, z));
            let v0 = at(x1, y1, z1);
            let v1 = at(x2, y1, z1);
            let v2 = at(x2, y2, z1);
            let v3 = at(x1, y2, z1);
            let v4 = at(x1, y1, z2);
            let v5 = at(x2, y1, z2);
            let v6 = at(x2, y2, z2);
            let v7 = at(x1, y2, z2);
            let [u, v] = b.offset.unwrap_or([0.0, 0.0]);
            // The standard layout from the offset, in vanilla's face
            // order: x2, x1, top, bottom, front (z1), back (z2).
            let mut rects: [[f32; 4]; 6] = [
                [u + d + w, v + d, u + d + w + d, v + d + h],
                [u, v + d, u + d, v + d + h],
                [u + d, v, u + d + w, v + d],
                [u + d + w, v + d, u + d + w + w, v],
                [u + d, v + d, u + d + w, v + d + h],
                [u + d + w + d, v + d, u + d + w + d + w, v + d + h],
            ];
            // The file's own rects: north = front, south = back, east = the
            // x2 side, west = the x1 side, up = top, down = bottom.
            for (explicit, face) in b.faces.iter().zip([4, 1 + 4, 0, 1, 2, 3]) {
                if let Some(rect) = explicit {
                    rects[face % 6] = *rect;
                }
            }
            if part.mirror_u {
                for rect in &mut rects {
                    rect.swap(0, 2);
                }
                rects.swap(0, 1);
            }
            if part.mirror_v {
                for rect in &mut rects {
                    rect.swap(1, 3);
                }
            }
            let faces = [
                [v5, v1, v2, v6],
                [v0, v4, v7, v3],
                [v5, v4, v0, v1],
                [v2, v3, v7, v6],
                [v1, v0, v3, v2],
                [v4, v5, v6, v7],
            ];
            for ((corners, [a, b2, c, e]), shade) in faces.into_iter().zip(rects).zip(SHADES) {
                let [m0, m1, m2, m3] = corners;
                let uv = [a / tw, b2 / th, c / tw, e / th];
                let color = tint.map(|t| t * shade);
                mesh.quad(
                    [place(m2), place(m3), place(m0), place(m1)],
                    uv,
                    [color; 4],
                    [light; 4],
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cem::{model, vars};

    /// Two parts: an arm that swings by the limb swing and a hand on it
    /// that follows a kept variable.
    const ARM: &str = r#"{
        "textureSize": [64, 64],
        "models": [
            {"id": "arm", "translate": [0, 10, 0],
             "boxes": [{"coordinates": [-1, 0, -1, 2, 8, 2], "textureOffset": [0, 0]}],
             "submodels": [{"id": "hand", "translate": [0, 8, 0],
                "boxes": [{"coordinates": [-1, 0, -1, 2, 2, 2], "textureOffset": [8, 0]}],
                "animations": [{"var.t": "var.t + 1", "hand.rx": "var.t * 0.1", "varb.on": "var.t > 2"}]}],
             "animations": [{"arm.rx": "limb_swing * 0.5", "arm.visible": "!is_hurt"}]}
        ]
    }"#;

    fn arm() -> CemModel {
        model::parse(ARM, &|_| None, &mut |_| {}).unwrap()
    }

    fn vars_of(swing: f32, hurt: bool) -> EntityVars {
        let mut v = EntityVars::default();
        v.0[usize::from(vars::slot("limb_swing").unwrap())] = swing;
        v.0[usize::from(vars::slot("is_hurt").unwrap())] = f32::from(hurt);
        v
    }

    #[test]
    fn animations_pose_the_parts_and_a_child_follows_its_parent() {
        let model = arm();
        let mut runtime = Runtime::new(&model, 1);
        runtime.animate(&model, &vars_of(2.0, false));
        assert_eq!(runtime.parts[0].rotate[0], 1.0, "arm.rx = limb_swing * 0.5");
        let at_rest = Runtime::new(&model, 1).matrices(&model);
        let posed = runtime.matrices(&model);
        // The hand's pivot is 8 below the arm's: swung with the arm.
        let rest = at_rest[1].unwrap().transform_point3(Vec3::ZERO);
        let swung = posed[1].unwrap().transform_point3(Vec3::ZERO);
        assert!((rest - Vec3::new(0.0, 18.0, 0.0)).length() < 1e-4, "{rest}");
        assert!((swung - rest).length() > 1.0, "{swung} {rest}");
        // Turning the arm about x by 1 rad (hand not yet turned by its own
        // frame-0 var.t = 1 → 0.1 rad) keeps the hand 8 from the shoulder.
        let shoulder = posed[0].unwrap().transform_point3(Vec3::ZERO);
        assert!(((swung - shoulder).length() - 8.0).abs() < 1e-3);
    }

    #[test]
    fn variables_a_model_keeps_persist_between_frames_per_mob() {
        let model = arm();
        let mut a = Runtime::new(&model, 1);
        let mut b = Runtime::new(&model, 2);
        for _ in 0..3 {
            a.animate(&model, &vars_of(0.0, false));
        }
        b.animate(&model, &vars_of(0.0, false));
        // var.t counts frames, kept for each mob on its own.
        assert_eq!(a.vars[0], 3.0);
        assert_eq!(b.vars[0], 1.0);
        // varb. holds 0 or 1: 3 > 2 on a, 1 > 2 on b.
        assert_eq!((a.vars[1], b.vars[1]), (1.0, 0.0));
        assert!((a.parts[1].rotate[0] - 0.3).abs() < 1e-6);
        // Parts go back to rest each frame; kept variables don't.
        a.animate(&model, &vars_of(0.0, false));
        assert_eq!(a.vars[0], 4.0);
    }

    #[test]
    fn a_hidden_part_hides_its_children_and_draws_nothing() {
        let model = arm();
        let mut runtime = Runtime::new(&model, 1);
        runtime.animate(&model, &vars_of(0.0, true));
        let matrices = runtime.matrices(&model);
        assert!(matrices[0].is_none() && matrices[1].is_none());
        let mut mesh = Mesh::default();
        add_parts(
            &mut mesh,
            &model,
            &runtime,
            |m| m.to_array(),
            [1.0; 3],
            [15.0, 0.0],
        );
        assert!(mesh.is_empty());
        runtime.animate(&model, &vars_of(0.0, false));
        add_parts(
            &mut mesh,
            &model,
            &runtime,
            |m| m.to_array(),
            [1.0; 3],
            [15.0, 0.0],
        );
        // Two boxes, six faces each.
        assert_eq!(mesh.indices.len() / 6, 12);
    }

    #[test]
    fn boxes_use_the_standard_uv_layout_and_per_face_rects() {
        let text = r#"{"textureSize": [64, 32], "models": [{"id": "a",
            "boxes": [{"coordinates": [0, 0, 0, 4, 4, 4], "textureOffset": [8, 4]}]},
            {"id": "b", "boxes": [{"coordinates": [0, 0, 0, 4, 4, 4], "textureOffset": [8, 4], "uvUp": [32, 16, 36, 20]}]}]}"#;
        let model = model::parse(text, &|_| None, &mut |_| {}).unwrap();
        let runtime = Runtime::new(&model, 1);
        let mut mesh = Mesh::default();
        add_parts(
            &mut mesh,
            &model,
            &runtime,
            |m| m.to_array(),
            [1.0; 3],
            [15.0, 0.0],
        );
        assert_eq!(mesh.vertices.len(), 48);
        let uv_rect = |first: usize| {
            let uvs: Vec<[f32; 2]> = mesh.vertices[first..first + 4]
                .iter()
                .map(|v| v.uv)
                .collect();
            let (u0, u1) = (
                uvs.iter().map(|p| p[0]).fold(f32::MAX, f32::min),
                uvs.iter().map(|p| p[0]).fold(f32::MIN, f32::max),
            );
            let (v0, v1) = (
                uvs.iter().map(|p| p[1]).fold(f32::MAX, f32::min),
                uvs.iter().map(|p| p[1]).fold(f32::MIN, f32::max),
            );
            [u0 * 64.0, v0 * 32.0, u1 * 64.0, v1 * 32.0]
        };
        // Face 2 is the top: vanilla puts it at u + d, v, d wide and tall.
        let top = uv_rect(2 * 4);
        assert!(
            (top[0] - 12.0).abs() < 1e-3 && (top[1] - 4.0).abs() < 1e-3,
            "{top:?}"
        );
        assert!(
            (top[2] - 16.0).abs() < 1e-3 && (top[3] - 8.0).abs() < 1e-3,
            "{top:?}"
        );
        // The second box's top is what the file says.
        let second = uv_rect(24 + 2 * 4);
        assert!(
            (second[0] - 32.0).abs() < 1e-3 && (second[3] - 20.0).abs() < 1e-3,
            "{second:?}"
        );
    }

    #[test]
    fn random_is_deterministic_per_mob_and_frames_are_repeatable() {
        let text = r#"{"models": [{"id": "a", "animations": [{"var.r": "random()"}]}]}"#;
        let model = model::parse(text, &|_| None, &mut |_| {}).unwrap();
        let run = |seed| {
            let mut r = Runtime::new(&model, seed);
            (0..5)
                .map(|_| {
                    r.animate(&model, &EntityVars::default());
                    r.vars[0]
                })
                .collect::<Vec<_>>()
        };
        assert_eq!(run(7), run(7), "the same mob gets the same numbers");
        assert_ne!(run(7), run(8));
        assert!(run(7).iter().all(|v| (0.0..1.0).contains(v)));
    }

    /// A top-level part's animated translate is an offset from its file
    /// translate; a child's is its whole offset from its parent. Fresh
    /// Animations' pig: the body's `ty` of 16 on a file translate of -8
    /// puts it at 24 (the ground), and the head's -12 under it at 12, where
    /// vanilla's pig head pivots.
    #[test]
    fn a_top_level_parts_animation_adds_to_its_place_and_a_childs_replaces_its_own() {
        let model = model::parse(
            r#"{"textureSize": [64, 64], "models": [
                {"part": "body", "id": "body", "invertAxis": "xy", "translate": [0, -8, 0],
                 "animations": [{"body.ty": "16"}],
                 "submodels": [
                   {"id": "head2", "invertAxis": "xy", "translate": [0, 12, -6],
                    "animations": [{"head2.ty": "-12"}],
                    "boxes": [{"coordinates": [-4, -4, -8, 8, 8, 8], "textureOffset": [0, 0]}]}]},
                {"part": "leg1", "id": "leg1", "invertAxis": "xy", "translate": [-3, 0, -7],
                 "animations": [{"leg1.tx": "-3.2", "leg1.ty": "24", "leg1.tz": "7"}]}
            ]}"#,
            &|_| None,
            &mut |_| {},
        )
        .unwrap();
        let mut runtime = Runtime::new(&model, 1);
        runtime.animate(&model, &EntityVars::default());
        let at = |i: usize| {
            runtime.matrices(&model)[i]
                .unwrap()
                .transform_point3(Vec3::ZERO)
        };
        assert!(
            (at(0) - Vec3::new(0.0, 24.0, 0.0)).length() < 1e-4,
            "body {}",
            at(0)
        );
        assert!(
            (at(1) - Vec3::new(0.0, 12.0, -6.0)).length() < 1e-4,
            "head {}",
            at(1)
        );
        // Unanimated, a top-level part sits at its file translate.
        let rest = Runtime::new(&model, 1).matrices(&model)[0]
            .unwrap()
            .transform_point3(Vec3::ZERO);
        assert!((rest - Vec3::new(0.0, 8.0, 0.0)).length() < 1e-4, "{rest}");
        // The leg's offset lands on the file translate (3, 0, -7) too.
        assert!(
            (at(2) - Vec3::new(-0.2, 24.0, 0.0)).length() < 1e-4,
            "leg {}",
            at(2)
        );
    }

    /// Prints a real pack's mob at rest and animated (run by hand).
    #[test]
    #[ignore]
    fn dump_pack_mob() {
        let (Ok(pack), Ok(name)) = (std::env::var("CEM_PACK"), std::env::var("CEM_MOB")) else {
            return;
        };
        let file = std::fs::File::open(&pack).unwrap();
        let mut zip = zip::ZipArchive::new(file).unwrap();
        let names: Vec<String> = zip
            .file_names()
            .filter_map(Result::ok)
            .map(|n| n.to_string())
            .collect();
        let mut read = |path: &str| {
            use std::io::Read;
            let mut text = String::new();
            zip.by_name(&format!("assets/minecraft/{path}"))
                .ok()?
                .read_to_string(&mut text)
                .ok()?;
            Some(text)
        };
        let text = read(&format!("optifine/cem/{name}.jem")).unwrap();
        let mut files = std::collections::HashMap::new();
        for n in names.iter().filter(|n| n.ends_with(".jpm")) {
            let rel = n.trim_start_matches("assets/minecraft/optifine/cem/");
            if let Some(t) = read(&format!("optifine/cem/{rel}")) {
                files.insert(rel.to_owned(), t);
            }
        }
        let model = model::parse(&text, &|p| files.get(p).cloned(), &mut |l| {
            println!("log: {l}")
        })
        .unwrap();
        println!(
            "texture {:?} size {:?} unknown {:?}",
            model.texture, model.texture_size, model.unknown
        );
        let mut runtime = Runtime::new(&model, 1);
        let rest = runtime.matrices(&model);
        runtime.animate(&model, &EntityVars::default());
        let posed = runtime.matrices(&model);
        for (i, part) in model.parts.iter().enumerate() {
            let at = |m: &Option<Mat4>| m.map(|m| m.transform_point3(Vec3::ZERO));
            println!(
                "{:2} {:28} parent {:?} rest {:?} anim {:?} boxes {}",
                i,
                part.name,
                part.parent,
                at(&rest[i]),
                at(&posed[i]),
                part.boxes.len()
            );
        }
    }
}
