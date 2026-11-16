//! The sheep's face stays on its head (the Step 0 / fix check): the face
//! quad of the skin is measured against the head of the wool layer (or the
//! skin's own head when sheared) over 120 frames at 60 FPS.

use glam::{DVec3, Vec3};

use super::models::{add_mob, add_mob_model};
use crate::{
    entity::{
        mob::Mob,
        mobs::{MobDef, MobRegistry},
    },
    world::mesh::Mesh,
};

#[derive(Clone, Copy, Debug)]
pub enum State {
    Idle,
    Looking,
    Eating,
    Walking,
}

pub const STATES: [State; 4] = [State::Idle, State::Looking, State::Eating, State::Walking];

/// Where the quads of a mesh whose texture rect lies in `rect` (pixels of a
/// 64x32 texture: u0, v0, u1, v1) are: four corners each.
fn quads_in(mesh: &Mesh, rect: [f32; 4]) -> Vec<[Vec3; 4]> {
    let eps = 0.2;
    mesh.vertices
        .chunks(4)
        .filter(|q| {
            q.iter().all(|v| {
                let (u, w) = (v.uv[0] * 64.0, v.uv[1] * 32.0);
                u >= rect[0] - eps && u <= rect[2] + eps && w >= rect[1] - eps && w <= rect[3] + eps
            })
        })
        .map(|q| std::array::from_fn(|i| Vec3::from(q[i].position)))
        .collect()
}

const ADULT_FACE: [f32; 4] = [8.0, 8.0, 14.0, 14.0];
const ADULT_TOP: [f32; 4] = [8.0, 0.0, 14.0, 8.0];
const BABY_FACE: [f32; 4] = [5.0, 5.0, 10.0, 10.0];
const BABY_TOP: [f32; 4] = [5.0, 0.0, 10.0, 5.0];
const WOOL_FACE: [f32; 4] = [6.0, 6.0, 12.0, 12.0];

fn skin_has(mesh: &Mesh, rect: [f32; 4]) -> bool {
    !quads_in(mesh, rect).is_empty()
}

/// An orthonormal frame of a quad: origin and unit axes.
fn frame(q: &[Vec3; 4]) -> (Vec3, [Vec3; 3]) {
    let a = (q[1] - q[0]).normalize();
    let b = (q[3] - q[0]).normalize();
    (q[0], [a, b, a.cross(b)])
}

fn local(frame: &(Vec3, [Vec3; 3]), p: Vec3) -> Vec3 {
    let d = p - frame.0;
    Vec3::new(d.dot(frame.1[0]), d.dot(frame.1[1]), d.dot(frame.1[2]))
}

/// One frame's meshes: (skin, wool).
fn draw(mob: &Mob, def: &MobDef, alpha: f32) -> (Mesh, Mesh) {
    let mut skin = Mesh::default();
    add_mob(&mut skin, mob, def, alpha, (15, 0));
    let mut wool = Mesh::default();
    for layer in &def.layers {
        if layer.wool && !mob.data.sheared {
            add_mob_model(&mut wool, mob, def, &layer.model, alpha, (15, 0), [1.0; 3]);
        }
    }
    (skin, wool)
}

/// The face's four corners in the head frame, per frame. The head frame is
/// the wool head's front quad, or the skin head's top when sheared. The
/// returned value is how far those moved from the first frame's (idle)
/// values, at most, in blocks, and the wool/skin head transform spread.
pub fn measure(name: &str, setup: &dyn Fn(&mut Mob), state: State) -> Option<(f32, f32)> {
    let mobs = MobRegistry::load(&crate::assets::dir().join("data/mobs.ron")).unwrap();
    let kind = mobs.find(name).unwrap();
    let def = mobs.get(kind);
    let mut mob = Mob::new(kind, def, DVec3::new(0.5, 64.0, 0.5), 0.0);
    setup(&mut mob);
    let mut rest: Option<[Vec3; 4]> = None;
    let (mut worst, mut spread) = (0.0f32, 0.0f32);
    let mut rest_top: Option<Vec3> = None;
    for i in 0..120u32 {
        let time = i as f32 / 60.0 * 20.0;
        let (tick, alpha) = (time.floor(), time.fract());
        crate::cem::begin_frame(tick as u64, alpha, DVec3::ZERO);
        mob.age = tick as u32;
        let t = i as f32 / 119.0;
        match state {
            State::Idle => {}
            State::Looking => {
                mob.head_yaw = (t * 6.0).sin() * 0.7;
                mob.pitch = (t * 4.0).sin() * 0.5;
            }
            State::Eating => {
                // Head down for 40 ticks (2 s = 120 frames): down, hold, up.
                mob.pitch = (t * std::f32::consts::PI).sin().powf(0.3) * 1.2;
            }
            State::Walking => {
                mob.limb_swing = time * 0.67;
                mob.limb_amount = 1.0;
            }
        }
        let (skin, wool) = draw(&mob, def, alpha);
        let (faces, tops) = if mob.data.age < 0 && !skin_has(&skin, [8.0, 8.0, 14.0, 14.0]) {
            (BABY_FACE, BABY_TOP)
        } else {
            (ADULT_FACE, ADULT_TOP)
        };
        let face = quads_in(&skin, faces);
        if face.len() != 1 {
            return None;
        }
        let face = face[0];
        // A baby's wool has no head; its face is held against the skin head.
        let wool_head = if mob.data.sheared {
            Vec::new()
        } else {
            quads_in(&wool, WOOL_FACE)
        };
        let reference = if wool_head.is_empty() {
            quads_in(&skin, tops)
        } else {
            wool_head
        };
        assert_eq!(reference.len(), 1, "one head reference quad");
        let f = frame(&reference[0]);
        let pts: [Vec3; 4] = std::array::from_fn(|k| local(&f, face[k]));
        let base = *rest.get_or_insert(pts);
        for k in 0..4 {
            worst = worst.max((pts[k] - base[k]).length());
        }
        // The wool head's own size shows in its frame: constant if the
        // two heads share one transform; here the skin's top quad in the
        // wool frame (they share the head turn).
        let skin_top = quads_in(&skin, tops);
        if !mob.data.sheared
            && let Some(top) = skin_top.first()
        {
            let c = (top[0] + top[2]) * 0.5;
            let p = local(&f, c);
            let b = *rest_top.get_or_insert(p);
            spread = spread.max((p - b).length());
        }
    }
    Some((worst, spread))
}

fn pack_registry(contains: &str) -> Option<crate::cem::Registry> {
    let dir = crate::assets::dir().join("downloads/resourcepacks");
    let path = crate::platform::fs::read_dir(dir)
        .ok()?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .find(|p| p.to_string_lossy().contains(contains))?;
    let pack = crate::pack::Pack::open(&path).ok()?;
    let read = |p: &str| pack.read(p);
    let mut registry = crate::cem::Registry::load(&read, &["sheep"]);
    registry.load_layer(&read, "sheep", "sheep_wool");
    Some(registry)
}

type Setup = Box<dyn Fn(&mut Mob)>;

#[test]
fn the_face_stays_on_the_head_in_every_style_and_state() {
    let variants: Vec<(&str, Setup)> = vec![
        ("wool", Box::new(|_| {})),
        ("sheared", Box::new(|m| m.data.sheared = true)),
        ("baby", Box::new(|m| m.data.age = -24000)),
        ("colored", Box::new(|m| m.data.color = 14)),
    ];
    for (label, contains) in [
        ("builtin", ""),
        ("FreshAnimations", "FreshAnimations"),
        ("Patrix", "Patrix"),
    ] {
        let registry = if contains.is_empty() {
            Some(crate::cem::Registry::default())
        } else {
            pack_registry(contains)
        };
        let Some(registry) = registry else {
            println!("{label}: pack missing");
            continue;
        };
        for (vlabel, setup) in &variants {
            for state in STATES {
                crate::cem::install(crate::cem::Registry {
                    models: registry.models.clone(),
                });
                let Some((worst, spread)) = measure("sheep", setup, state) else {
                    println!(
                        "STEP0 {label:16} {vlabel:8} {state:?}: face quad not found (other UV layout)"
                    );
                    continue;
                };
                assert!(
                    worst <= 0.01 && spread <= 0.01,
                    "{label} {vlabel} {state:?}: face off by {worst}, heads apart by {spread}"
                );
                println!(
                    "STEP0 {label:16} {vlabel:8} {state:?}: face leaves head by {worst:.4} blocks; skin/wool head spread {spread:.4}"
                );
            }
        }
    }
    crate::cem::install(crate::cem::Registry::default());
}

/// Writes `target/sheep-face-sheet.png`: the sheep, skin and wool in one
/// picture, in Vanilla and in Barebones (Fresh Animations), idle and with
/// its head down. Run with `--ignored`.
#[test]
#[ignore = "writes a picture for a person to look at"]
fn contact_sheet() {
    use crate::audit::raster::{View, render};
    use image::{GenericImage, RgbaImage};
    let assets = crate::assets::dir();
    let mobs = MobRegistry::load(&assets.join("data/mobs.ron")).unwrap();
    let kind = mobs.find("sheep").unwrap();
    let def = mobs.get(kind);
    let size = 400u32;
    let mut sheet = RgbaImage::new(size * 4, size);
    let shots = [
        ("vanilla", View::FRONT, 0.0),
        ("vanilla", View::SIDE, 1.0),
        ("barebones", View::FRONT, 1.0),
        ("barebones", View::SIDE, 1.0),
    ];
    for (i, (style, view, pitch)) in shots.into_iter().enumerate() {
        let stack = crate::audit::stack(style, &assets.join("downloads/resourcepacks")).unwrap();
        let look = crate::audit::build_look(&assets, &stack, &mobs).unwrap();
        let mut mob = Mob::new(kind, def, DVec3::ZERO, 0.0);
        mob.pitch = pitch;
        crate::cem::begin_frame(10, 0.5, DVec3::ZERO);
        let (skin, wool) = draw(&mob, def, 0.5);
        // Skin and wool textures side by side, so one render has both.
        let a = &look.mob_textures[mobs.texture_slot(kind, 0)];
        let b = &look.mob_textures[mobs.layer_slot(kind, 0)];
        let mut atlas = RgbaImage::new(a.width() + b.width(), a.height().max(b.height()));
        atlas.copy_from(a, 0, 0).unwrap();
        atlas.copy_from(b, a.width(), 0).unwrap();
        let (fa, fb) = (
            a.width() as f32 / atlas.width() as f32,
            b.width() as f32 / atlas.width() as f32,
        );
        let mut mesh = Mesh::default();
        for v in &skin.vertices {
            let mut v = *v;
            v.uv = [
                v.uv[0] * fa,
                v.uv[1] * a.height() as f32 / atlas.height() as f32,
            ];
            mesh.vertices.push(v);
        }
        let base = mesh.vertices.len() as u32;
        for v in &wool.vertices {
            let mut v = *v;
            v.uv = [
                fa + v.uv[0] * fb,
                v.uv[1] * b.height() as f32 / atlas.height() as f32,
            ];
            mesh.vertices.push(v);
        }
        mesh.indices = skin.indices.clone();
        mesh.indices.extend(wool.indices.iter().map(|i| i + base));
        let (image, _) = render(
            &mesh,
            &atlas,
            view,
            size,
            image::Rgba([60, 70, 90, 255]),
            None,
        );
        sheet.copy_from(&image, size * i as u32, 0).unwrap();
    }
    sheet.save("target/sheep-face-sheet.png").unwrap();
}
