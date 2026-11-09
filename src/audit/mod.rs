//! The texture checker (M60): `--texture-audit` renders every block, item,
//! mob and GUI sprite offscreen into labeled contact sheets, runs the
//! detectors over what it drew, and writes a report; the Texture Viewer
//! (`viewer`) shows the same thumbnails for Kenzo to check by eye and mark.

pub mod detect;
pub mod marks;
pub mod raster;
pub mod sheet;
pub mod viewer;

use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
    sync::Arc,
};

use glam::DVec3;
use image::{Rgba, RgbaImage};
use serde::Serialize;

use self::{
    detect::{Detector, Flag},
    raster::{Stats, View},
};
use crate::{
    entity::{
        mob::Mob,
        mobs::{MobKind, MobRegistry},
    },
    inventory::{item, stack::ItemStack},
    look::Look,
    render::{
        atlas::MISSING_TILE,
        icons::{IconFace, ItemIcon},
        models::add_mob,
    },
    world::{
        Face,
        block::{self, BlockState, Tint},
        chunk::{Chunk, ChunkPos},
        light::light_chunk,
        mesh::{ChunkArea, Mesh, mesh_chunk},
        props::Prop,
    },
};

const BACKGROUND: Rgba<u8> = Rgba([34, 34, 40, 255]);

/// What the viewer and the audit group things by.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
pub enum Category {
    Blocks,
    Items,
    Mobs,
    Gui,
    Particles,
    Environment,
}

impl Category {
    pub const ALL: [Self; 6] = [
        Self::Blocks,
        Self::Items,
        Self::Mobs,
        Self::Gui,
        Self::Particles,
        Self::Environment,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Self::Blocks => "Blocks",
            Self::Items => "Items",
            Self::Mobs => "Mobs",
            Self::Gui => "GUI",
            Self::Particles => "Particles",
            Self::Environment => "Environment",
        }
    }

    /// The names of the views each entry of this category has.
    pub fn views(self) -> &'static [&'static str] {
        match self {
            Self::Blocks => &["3q", "top"],
            Self::Items => &["icon"],
            Self::Mobs => &["front", "side", "back"],
            _ => &["flat"],
        }
    }
}

/// What an entry shows, enough to draw it again at any size and angle.
#[derive(Debug, Clone, PartialEq)]
pub enum Subject {
    Block(BlockState),
    Item(item::ItemId),
    Mob {
        kind: MobKind,
        variant: u8,
        baby: bool,
    },
    /// A texture drawn flat (GUI, particles, environment).
    Flat(String),
}

pub struct Entry {
    pub category: Category,
    pub id: String,
    pub subject: Subject,
    /// One picture per view of the category (`Category::views`).
    pub views: Vec<RgbaImage>,
    pub flags: Vec<Flag>,
    /// The layer or pack its texture came from.
    pub origin: String,
}

impl Entry {
    /// The most severe flag's detector.
    pub fn worst(&self) -> Option<Detector> {
        self.flags
            .iter()
            .map(|f| f.detector)
            .max_by_key(|d| d.severity())
    }
}

/// A texture stack to audit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Stack {
    pub name: String,
    /// Pack file names, top first.
    pub packs: Vec<String>,
}

/// `crate::pack::find_by_words`.
pub fn find_pack(dir: &Path, want: &[&str], deny: &[&str]) -> Option<String> {
    crate::pack::find_by_words(dir, want, deny)
}

/// The stack called `name` (`vanilla`, `barebones`, `realistic`), with the
/// packs it finds in `dir`.
pub fn stack(name: &str, dir: &Path) -> Option<Stack> {
    let style = crate::render::visuals::Style::parse(name)?;
    Some(Stack {
        name: name.to_owned(),
        packs: crate::pack::style_stack(style, dir),
    })
}

/// Builds the look for a stack and makes its custom entity models the
/// active ones, so mobs draw as they would in the game.
pub fn build_look(assets: &Path, stack: &Stack, mobs: &MobRegistry) -> anyhow::Result<Look> {
    let packs = crate::look::open_packs_in(
        &crate::pack::packs_dir(),
        &assets.join("downloads/resourcepacks"),
        &stack.packs,
    );
    let mut look = Look::build(assets, packs, mobs)?;
    crate::cem::install(std::mem::take(&mut look.cem));
    Ok(look)
}

/// One block alone at the middle of an empty chunk, lit by the sky and
/// meshed as the game does.
pub fn block_mesh(look: &Look, state: BlockState) -> Mesh {
    let mut chunk = Chunk::new(ChunkPos::new(0, 0));
    chunk.set(8, 64, 8, state);
    let area = ChunkArea::new(Arc::new(chunk), |_| None);
    let light = light_chunk(&look.models, &area);
    mesh_chunk(&look.models, &area, &light)
}

/// A sprite as one quad facing the camera.
fn sprite_mesh(face: IconFace) -> Mesh {
    use crate::render::vertex::Vertex;
    let mut mesh = Mesh::default();
    let v = |x: f32, y: f32, u: f32, t: f32| Vertex {
        position: [x, y, 0.0],
        uv: [u, t],
        color: face.color,
        light: [15.0, 0.0],
    };
    let [u0, t0, u1, t1] = face.uv;
    mesh.vertices = vec![
        v(-1.0, -1.0, u0, t1),
        v(1.0, -1.0, u1, t1),
        v(1.0, 1.0, u1, t0),
        v(-1.0, 1.0, u0, t0),
    ];
    mesh.indices = vec![0, 1, 2, 0, 2, 3];
    mesh
}

/// What a mob in the viewer is doing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Anim {
    Idle,
    Walk,
    Attack,
}

impl Anim {
    pub const ALL: [Self; 3] = [Self::Idle, Self::Walk, Self::Attack];

    pub fn name(self) -> &'static str {
        match self {
            Self::Idle => "Idle",
            Self::Walk => "Walk",
            Self::Attack => "Attack",
        }
    }
}

/// A mob alone, standing still, lit by the sky.
pub fn mob_mesh(mobs: &MobRegistry, kind: MobKind, variant: u8, baby: bool) -> Mesh {
    mob_mesh_posed(mobs, kind, variant, baby, Anim::Idle, 0.0)
}

/// A mob `time` seconds into an animation.
pub fn mob_mesh_posed(
    mobs: &MobRegistry,
    kind: MobKind,
    variant: u8,
    baby: bool,
    anim: Anim,
    time: f32,
) -> Mesh {
    let def = mobs.get(kind);
    let mut mob = Mob::new(kind, def, DVec3::ZERO, 0.0);
    mob.data.variant = variant;
    if baby {
        mob.data.age = -24_000;
    }
    match anim {
        Anim::Idle => {}
        Anim::Walk => {
            mob.limb_swing = time * 6.0;
            mob.limb_amount = 2.0;
        }
        Anim::Attack => {
            mob.aggressive = true;
            mob.limb_swing = time * 9.0;
            mob.limb_amount = 2.0;
        }
    }
    mob.age = (time * 20.0) as u32;
    crate::cem::begin_frame(time * 20.0, 0.5, DVec3::ZERO);
    let mut mesh = Mesh::default();
    add_mob(&mut mesh, &mob, def, 0.5, (15, 0));
    mesh
}

/// `state` between a grass block and a stone block, to see how it sits.
pub fn block_mesh_in_company(look: &Look, state: BlockState) -> Mesh {
    use crate::world::blocks;
    let mut chunk = Chunk::new(ChunkPos::new(0, 0));
    chunk.set(8, 64, 8, state);
    chunk.set(7, 64, 8, BlockState::of(blocks::GRASS_BLOCK));
    chunk.set(9, 64, 8, BlockState::of(blocks::STONE));
    let area = ChunkArea::new(Arc::new(chunk), |_| None);
    let light = light_chunk(&look.models, &area);
    mesh_chunk(&look.models, &area, &light)
}

/// `texture` enlarged to about `size` pixels with a rectangle drawn around
/// every quad's UV box of `mesh` (UVs are fractions of the texture).
pub fn uv_overlay(texture: &RgbaImage, mesh: &Mesh, size: u32) -> RgbaImage {
    let scale = (size / texture.width().max(texture.height()).max(1)).max(1);
    let mut image = image::imageops::resize(
        texture,
        texture.width() * scale,
        texture.height() * scale,
        image::imageops::FilterType::Nearest,
    );
    let (w, h) = (image.width() as f32, image.height() as f32);
    for quad in mesh.vertices.chunks(4) {
        let us = quad.iter().map(|v| v.uv[0]);
        let ts = quad.iter().map(|v| v.uv[1]);
        let rect = |a: f32, b: f32, lim: f32| {
            (
                (a * lim).clamp(0.0, lim - 1.0) as u32,
                (b * lim).clamp(0.0, lim - 1.0) as u32,
            )
        };
        let (x0, x1) = rect(
            us.clone().fold(f32::MAX, f32::min),
            us.fold(f32::MIN, f32::max),
            w,
        );
        let (y0, y1) = rect(
            ts.clone().fold(f32::MAX, f32::min),
            ts.fold(f32::MIN, f32::max),
            h,
        );
        for x in x0..=x1 {
            image.put_pixel(x, y0, Rgba([255, 255, 0, 255]));
            image.put_pixel(x, y1, Rgba([255, 255, 0, 255]));
        }
        for y in y0..=y1 {
            image.put_pixel(x0, y, Rgba([255, 255, 0, 255]));
            image.put_pixel(x1, y, Rgba([255, 255, 0, 255]));
        }
    }
    image
}

/// Draws `subject` from `view`, `size` pixels square, and says what it
/// sampled.
pub fn render_subject(
    look: &Look,
    mobs: &MobRegistry,
    subject: &Subject,
    view: View,
    size: u32,
) -> (RgbaImage, Stats) {
    let atlas = look.atlas.image();
    let missing = Some(look.atlas.uv_rect(MISSING_TILE));
    match subject {
        Subject::Block(state) => raster::render(
            &block_mesh(look, *state),
            atlas,
            view,
            size,
            BACKGROUND,
            missing,
        ),
        Subject::Item(id) => {
            let registry = item::registry();
            let Some(def) = registry.get(*id) else {
                return (
                    RgbaImage::from_pixel(size, size, BACKGROUND),
                    Stats::default(),
                );
            };
            match (&def.texture, def.places) {
                (None, Some(block)) => raster::render(
                    &block_mesh(look, BlockState::of(block)),
                    atlas,
                    View::THREE_QUARTER,
                    size,
                    BACKGROUND,
                    missing,
                ),
                _ => {
                    let icon = look.icons.icon(&ItemStack::new(*id, 1));
                    let face = match icon {
                        ItemIcon::Flat(face) => face,
                        ItemIcon::Cube { front, .. } => front,
                    };
                    raster::render(
                        &sprite_mesh(face),
                        atlas,
                        View {
                            yaw: 0.0,
                            pitch: 0.0,
                        },
                        size,
                        BACKGROUND,
                        missing,
                    )
                }
            }
        }
        Subject::Mob {
            kind,
            variant,
            baby,
        } => {
            let mesh = mob_mesh(mobs, *kind, *variant, *baby);
            let slot = mobs.texture_slot(*kind, *variant);
            raster::render(
                &mesh,
                &look.mob_textures[slot],
                view,
                size,
                BACKGROUND,
                None,
            )
        }
        Subject::Flat(name) => {
            let mut image = RgbaImage::from_pixel(size, size, BACKGROUND);
            if let Some(texture) = look.res.image(name) {
                let (w, h) = texture.dimensions();
                let scale = (size as f32 / w.max(h) as f32).min(8.0);
                let (dw, dh) = (
                    ((w as f32 * scale) as u32).max(1),
                    ((h as f32 * scale) as u32).max(1),
                );
                let scaled =
                    image::imageops::resize(&texture, dw, dh, image::imageops::FilterType::Nearest);
                image::imageops::overlay(
                    &mut image,
                    &scaled,
                    i64::from((size - dw.min(size)) / 2),
                    i64::from((size - dh.min(size)) / 2),
                );
            }
            (image, Stats::default())
        }
    }
}

/// The views of a category, as angles.
pub fn views_of(category: Category) -> Vec<View> {
    match category {
        Category::Blocks => vec![View::THREE_QUARTER, View::TOP],
        Category::Mobs => vec![View::FRONT, View::SIDE, View::BACK],
        _ => vec![View {
            yaw: 0.0,
            pitch: 0.0,
        }],
    }
}

/// The pack or layer a texture came from.
pub fn origin_of(look: &Look, texture: &str) -> String {
    match look.res.texture(texture) {
        None => "none".to_owned(),
        Some(t) if t.vanilla => "vanilla".to_owned(),
        Some(t) if t.layer == 0 => "Rustcraft textures".to_owned(),
        Some(t) => look
            .res
            .layers()
            .get(t.layer)
            .map_or("pack", |p| p.name())
            .to_owned(),
    }
}

/// Whether the render should show a tint on the block: grass, ground
/// cover and foliage take the biome's color from a color map, and plain
/// textures take none. Water (its texture is already colored), redstone
/// wire and stems (colored by code, by power and growth) are left out.
fn expects_tint(name: &str, tint: Tint) -> Option<bool> {
    if name == "water" || name == "redstone_wire" || name.ends_with("_stem") {
        return None;
    }
    match tint {
        Tint::None => Some(false),
        Tint::GrassTop | Tint::Grass | Tint::Foliage => Some(true),
        _ => None,
    }
}

/// Every block as an entry: both views and the detectors' flags.
pub fn block_entries(look: &Look, only: Option<&BTreeSet<String>>) -> Vec<Entry> {
    let registry = block::registry();
    let missing = look.atlas.uv_rect(MISSING_TILE);
    let atlas = look.atlas.image();
    let mut out = Vec::new();
    let mut sides: Vec<(String, Vec<String>, Vec<[u32; 4]>)> = Vec::new();
    for def in registry.iter() {
        if only.is_some_and(|set| !set.contains(&def.name)) {
            continue;
        }
        let state = BlockState::of(def.id);
        let id = format!("block/{}", def.name);
        let model = look.models.get(state);
        if model.faces.iter().all(|f| f.uv == [0.0; 4]) {
            // Nothing drawn (air, barriers): no picture to check.
            continue;
        }
        let mesh = block_mesh(look, state);
        let mut flags = Vec::new();
        let mut views = Vec::new();
        for (n, view) in views_of(Category::Blocks).into_iter().enumerate() {
            let (image, stats) =
                raster::render(&mesh, atlas, view, sheet::THUMB, BACKGROUND, Some(missing));
            if n == 0
                && let Some(detail) = detect::placeholder_in_render(&stats)
            {
                flags.push(Flag::new(Detector::Placeholder, &id, "", detail));
            }
            views.push(image);
        }
        if let Some(detail) = detect::uv_out_of_range(&mesh) {
            flags.push(Flag::new(Detector::UvOutOfRange, &id, "", detail));
        }
        // An opaque cube with a fully transparent face.
        if model.opaque_cube {
            for face in Face::ALL {
                if detect::face_fully_transparent(atlas, model.faces[face.index()].uv) {
                    flags.push(Flag::new(
                        Detector::TransparentFace,
                        &id,
                        &format!("{face:?}"),
                        "an opaque block has a face with no opaque pixel",
                    ));
                }
            }
        }
        let colors: Vec<[f32; 3]> = mesh.vertices.iter().map(|v| v.color).collect();
        if let Some(expected) = expects_tint(&def.name, def.tint)
            && let Some(detail) = detect::tint_mismatch(expected, &colors)
        {
            flags.push(Flag::new(Detector::Tint, &id, "", detail));
        }
        // Axis and facing orientation against the model's own faces.
        if def.props.contains(&Prop::Axis) {
            let up = look.models.get(state.with(Prop::Axis, 1));
            let along_x = look.models.get(state.with(Prop::Axis, 0));
            let pick = |m: &crate::world::mesh::BlockModel| {
                [Face::Top, Face::East, Face::North].map(|f| m.faces[f.index()].uv)
            };
            if !detect::axis_consistent(pick(up), pick(along_x)) {
                flags.push(Flag::new(
                    Detector::Orientation,
                    &id,
                    "axis",
                    "the end texture is not on the faces the axis points through",
                ));
            }
        }
        if def.props.contains(&Prop::Facing) && !def.name.ends_with("_bed") {
            let front: Vec<[f32; 4]> = [Face::North, Face::East, Face::South, Face::West]
                .into_iter()
                .map(|f| look.models.get(state.with_face(Prop::Facing, f)).faces[f.index()].uv)
                .collect();
            if front.iter().any(|uv| *uv != front[0]) {
                flags.push(Flag::new(
                    Detector::Orientation,
                    &id,
                    "facing",
                    "the front texture is not on the face the block faces",
                ));
            }
        }
        // For the lookup-fell-through check: what each side renders as.
        let names: Vec<String> = def
            .all_textures()
            .flat_map(|t| {
                Face::ALL.map(|f| t.get(f).map(|n| block::texture_location(n).into_owned()))
            })
            .flatten()
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect();
        let quant = |uv: [f32; 4]| uv.map(|c| (c * 65_536.0) as u32);
        // Only plain cubes: a pane, a pot or a cauldron shares its faces
        // with the block it is made from by design.
        if def.shape == block::Shape::Cube {
            sides.push((
                id.clone(),
                names.clone(),
                Face::ALL
                    .iter()
                    .map(|f| quant(model.faces[f.index()].uv))
                    .collect(),
            ));
        }
        let origin = names
            .first()
            .map_or_else(|| "none".to_owned(), |n| origin_of(look, n));
        out.push(Entry {
            category: Category::Blocks,
            id,
            subject: Subject::Block(state),
            views,
            flags,
            origin,
        });
    }
    // Blocks whose sides are the same while their textures differ. The
    // missing-texture tile is every such block's fall-through, so those
    // are the placeholder detector's.
    let missing_uv = missing.map(|c| (c * 65_536.0) as u32);
    for group in detect::duplicate_sides(&sides) {
        let same_as_missing = sides
            .iter()
            .find(|s| s.0 == group[0])
            .is_some_and(|s| s.2.iter().all(|r| *r == missing_uv));
        if same_as_missing {
            continue;
        }
        for id in &group {
            if let Some(e) = out.iter_mut().find(|e| &e.id == id) {
                e.flags.push(Flag::new(
                    Detector::DuplicateSides,
                    id,
                    "",
                    format!("renders the same sides as {}", group.join(", ")),
                ));
            }
        }
    }
    out
}

/// Every item as an entry: its inventory icon at 2×.
pub fn item_entries(look: &Look) -> Vec<Entry> {
    let registry = item::registry();
    let missing = look.atlas.uv_rect(MISSING_TILE);
    let mut out = Vec::new();
    for def in registry.iter() {
        let id = format!("item/{}", def.name);
        let subject = Subject::Item(def.id);
        let (image, stats) = render_subject(
            look,
            &MobRegistry::empty(),
            &subject,
            View::THREE_QUARTER,
            32,
        );
        let mut flags = Vec::new();
        if let Some(detail) = detect::placeholder_in_render(&stats) {
            flags.push(Flag::new(Detector::Placeholder, &id, "", detail));
        }
        // The empty-or-flat check looks at the icon without its backdrop.
        let mut icon = image.clone();
        for p in icon.pixels_mut() {
            if p.0 == BACKGROUND.0 {
                *p = Rgba([0, 0, 0, 0]);
            }
        }
        if let Some(detail) = detect::flat_icon(&icon) {
            flags.push(Flag::new(Detector::FlatIcon, &id, "", detail));
        }
        let _ = missing;
        let origin = def.texture.as_deref().map_or_else(
            || "its block".to_owned(),
            |t| origin_of(look, &item::texture_location(t)),
        );
        out.push(Entry {
            category: Category::Items,
            id,
            subject,
            views: vec![image],
            flags,
            origin,
        });
    }
    out
}

/// Every mob: front, side and back, with each variant and a baby.
pub fn mob_entries(look: &Look, mobs: &MobRegistry) -> Vec<Entry> {
    let mut out = Vec::new();
    for (kind, def) in mobs.iter() {
        let mut variants: Vec<(u8, bool)> = vec![(0, false)];
        variants.extend((1..=def.variants.len() as u8).map(|v| (v, false)));
        if def.ai.is_creature() {
            variants.push((0, true));
        }
        for (variant, baby) in variants {
            let id = format!(
                "mob/{}{}{}",
                def.name,
                if variant > 0 {
                    format!("#{variant}")
                } else {
                    String::new()
                },
                if baby { "#baby" } else { "" }
            );
            let mesh = mob_mesh(mobs, kind, variant, baby);
            let slot = mobs.texture_slot(kind, variant);
            let texture = &look.mob_textures[slot];
            let mut flags = Vec::new();
            let mut views = Vec::new();
            for view in views_of(Category::Mobs) {
                views.push(raster::render(&mesh, texture, view, sheet::THUMB, BACKGROUND, None).0);
            }
            if let Some(detail) = detect::uv_out_of_range(&mesh) {
                flags.push(Flag::new(Detector::UvOutOfRange, &id, "", detail));
            }
            for (part, share) in detect::transparent_parts(&mesh, texture, 0.5) {
                flags.push(Flag::new(
                    Detector::TransparentPart,
                    &id,
                    &format!("box {part}"),
                    format!(
                        "{:.0}% of the box's faces sample transparent pixels",
                        share * 100.0
                    ),
                ));
            }
            let name = if variant == 0 {
                def.texture.as_str()
            } else {
                def.variants[variant as usize - 1].as_str()
            };
            if look.res.texture(name).is_none() {
                flags.push(Flag::new(
                    Detector::UnknownTexture,
                    &id,
                    "",
                    format!("{name} is in no layer"),
                ));
            }
            // The texture the look chose against the size the model's UVs
            // want (a pack's may be a whole multiple of it).
            if variant == 0
                && !baby
                && let Some((_, declared)) =
                    mobs.textures_sized().into_iter().find(|(n, _)| *n == name)
                && detect::size_mismatch(declared, texture.dimensions(), true)
                && look.res.texture(name).is_some()
            {
                flags.push(Flag::new(
                    Detector::SizeMismatch,
                    &id,
                    "",
                    format!(
                        "the texture chosen for {name} is {}×{} but the model wants {}×{}",
                        texture.width(),
                        texture.height(),
                        declared.0,
                        declared.1
                    ),
                ));
            }
            out.push(Entry {
                category: Category::Mobs,
                id,
                subject: Subject::Mob {
                    kind,
                    variant,
                    baby,
                },
                views,
                flags,
                origin: origin_of(look, name),
            });
        }
    }
    out
}

/// The textures under `folder` (`gui/sprites`) in the vanilla and base
/// layers, as names (`gui/sprites/hud/heart/full`).
fn textures_under(assets: &Path, folder: &str) -> BTreeSet<String> {
    fn walk(dir: &Path, root: &Path, out: &mut BTreeSet<String>) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for path in entries.filter_map(|e| e.ok().map(|e| e.path())) {
            if path.is_dir() {
                walk(&path, root, out);
            } else if path.extension().is_some_and(|e| e == "png")
                && let Ok(rel) = path.strip_prefix(root)
            {
                out.insert(rel.with_extension("").to_string_lossy().into_owned());
            }
        }
    }
    let mut out = BTreeSet::new();
    for root in [
        assets.join("vanilla/assets/minecraft/textures"),
        assets.join("textures"),
    ] {
        walk(&root.join(folder), &root, &mut out);
    }
    out
}

/// Flat textures of `folder` as entries of `category`.
pub fn flat_entries(
    look: &Look,
    assets: &Path,
    category: Category,
    folders: &[&str],
) -> Vec<Entry> {
    let mut names = BTreeSet::new();
    for folder in folders {
        names.extend(textures_under(assets, folder));
    }
    names
        .into_iter()
        .map(|name| {
            let subject = Subject::Flat(name.clone());
            let (image, _) = render_subject(
                look,
                &MobRegistry::empty(),
                &subject,
                View {
                    yaw: 0.0,
                    pitch: 0.0,
                },
                sheet::THUMB,
            );
            let mut flags = Vec::new();
            if look.res.texture(&name).is_none() {
                flags.push(Flag::new(
                    Detector::UnknownTexture,
                    &name,
                    "",
                    "no layer has it",
                ));
            }
            Entry {
                category,
                id: name.clone(),
                origin: origin_of(look, &name),
                subject,
                views: vec![image],
                flags,
            }
        })
        .collect()
}

/// Animated textures drawn as one squashed picture.
pub fn animated_flags(look: &Look, assets: &Path) -> (Vec<Flag>, Vec<String>) {
    let mut flags = Vec::new();
    let mut not_animated = Vec::new();
    let tile = look.atlas.tile_size();
    for name in textures_with_mcmeta(assets) {
        let Some(texture) = look.res.texture(&name) else {
            continue;
        };
        let Some(animation) = &texture.animation else {
            continue;
        };
        let frames = animation.split(&texture.image);
        let Some(first) = frames.first() else {
            continue;
        };
        let in_atlas = look.atlas.has(&name);
        if !in_atlas {
            not_animated.push(name.clone());
            continue;
        }
        if detect::squashed_strip(texture.image.dimensions(), first.dimensions(), (tile, tile)) {
            flags.push(Flag::new(
                Detector::SquashedStrip,
                &name,
                "",
                format!(
                    "frames are {}×{} but the atlas tile is {tile}×{tile}",
                    first.width(),
                    first.height()
                ),
            ));
        }
    }
    (flags, not_animated)
}

/// The names of the textures that have a `.png.mcmeta`.
pub fn textures_with_mcmeta(assets: &Path) -> BTreeSet<String> {
    fn walk(dir: &Path, root: &Path, out: &mut BTreeSet<String>) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for path in entries.filter_map(|e| e.ok().map(|e| e.path())) {
            if path.is_dir() {
                walk(&path, root, out);
            } else if path
                .file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.ends_with(".png.mcmeta"))
                && let Ok(rel) = path.strip_prefix(root)
            {
                let rel = rel.to_string_lossy();
                out.insert(rel.trim_end_matches(".png.mcmeta").to_owned());
            }
        }
    }
    let mut out = BTreeSet::new();
    for root in [
        assets.join("vanilla/assets/minecraft/textures"),
        assets.join("textures"),
    ] {
        walk(&root, &root, &mut out);
    }
    out
}

/// A finished audit of one texture stack.
pub struct Audit {
    pub stack: Stack,
    pub entries: Vec<Entry>,
    /// Flags that belong to no single entry (animated strips).
    pub extra_flags: Vec<Flag>,
    pub step0: Vec<String>,
}

/// Runs the whole audit for `stack`.
pub fn run(
    assets: &Path,
    stack: &Stack,
    only_blocks: Option<&BTreeSet<String>>,
) -> anyhow::Result<Audit> {
    let mobs = MobRegistry::load(&assets.join("data/mobs.ron"))?;
    let look = build_look(assets, stack, &mobs)?;
    let mut entries = block_entries(&look, only_blocks);
    entries.extend(item_entries(&look));
    entries.extend(mob_entries(&look, &mobs));
    entries.extend(flat_entries(&look, assets, Category::Gui, &["gui"]));
    entries.extend(flat_entries(
        &look,
        assets,
        Category::Particles,
        &["particle"],
    ));
    entries.extend(flat_entries(
        &look,
        assets,
        Category::Environment,
        &["environment"],
    ));
    let (extra_flags, not_animated) = animated_flags(&look, assets);
    let mut step0 = vec![format!(
        "stack {}: packs {:?}; {} blocks, {} items, {} mob entries drawn; {} animated atlas tiles",
        stack.name,
        stack.packs,
        entries
            .iter()
            .filter(|e| e.category == Category::Blocks)
            .count(),
        entries
            .iter()
            .filter(|e| e.category == Category::Items)
            .count(),
        entries
            .iter()
            .filter(|e| e.category == Category::Mobs)
            .count(),
        look.atlas.animated()
    )];
    let mut tints: BTreeMap<String, usize> = BTreeMap::new();
    for def in block::registry().iter().filter(|d| d.tint != Tint::None) {
        *tints.entry(format!("{:?}", def.tint)).or_default() += 1;
    }
    step0.push(format!(
        "tinted blocks by color source (Grass*/Foliage from the biome color maps, Water from the biome's water color): {tints:?}; code-tinted: redstone wire (power), stems (growth), spawn eggs and leather armor (item colors), potions (effect colors)"
    ));
    let mut per_origin: BTreeMap<&str, usize> = BTreeMap::new();
    for e in &entries {
        *per_origin.entry(e.origin.as_str()).or_default() += 1;
    }
    step0.push(format!("textures by layer: {per_origin:?}"));
    step0.push(format!(
        "animated textures (.png.mcmeta): {}; not animated by the renderer (not in the atlas): {not_animated:?}",
        textures_with_mcmeta(assets).len()
    ));
    for (_, def) in mobs.iter() {
        if let Some(found) = look.res.texture(&def.texture) {
            let declared = mobs
                .textures_sized()
                .into_iter()
                .find(|(n, _)| *n == def.texture)
                .map(|(_, s)| s);
            let actual = found.image.dimensions();
            step0.push(format!(
                "mob {}: {} declared {:?} actual {:?}{}",
                def.name,
                def.texture,
                declared,
                actual,
                if declared.is_some_and(|d| d != actual) {
                    "  MISMATCH"
                } else {
                    ""
                }
            ));
        }
    }
    Ok(Audit {
        stack: stack.clone(),
        entries,
        extra_flags,
        step0,
    })
}

#[derive(Serialize)]
struct ReportFlag<'a> {
    detector: &'a str,
    category: &'a str,
    id: &'a str,
    part: &'a str,
    detail: &'a str,
    severity: u8,
}

impl Audit {
    pub fn flags(&self) -> Vec<(Category, &Flag)> {
        let mut out: Vec<(Category, &Flag)> = self
            .entries
            .iter()
            .flat_map(|e| e.flags.iter().map(move |f| (e.category, f)))
            .collect();
        out.extend(self.extra_flags.iter().map(|f| (Category::Environment, f)));
        out
    }

    /// Flag counts per detector and per category, and the top `n` by severity.
    pub fn summary(&self, n: usize) -> String {
        let flags = self.flags();
        let mut by_detector: BTreeMap<&str, usize> = BTreeMap::new();
        let mut by_category: BTreeMap<&str, usize> = BTreeMap::new();
        for (category, flag) in &flags {
            *by_detector.entry(flag.detector.name()).or_default() += 1;
            *by_category.entry(category.name()).or_default() += 1;
        }
        let mut ranked = flags.clone();
        ranked.sort_by_key(|(_, f)| std::cmp::Reverse(f.detector.severity()));
        let mut out = format!(
            "{} entries, {} flags\nper detector: {by_detector:?}\nper category: {by_category:?}\ntop {n} by severity:\n",
            self.entries.len(),
            flags.len()
        );
        for (_, f) in ranked.into_iter().take(n) {
            out += &format!(
                "  [{}] {} {} {}\n",
                f.detector.name(),
                f.id,
                f.part,
                f.detail
            );
        }
        out
    }

    /// Writes the contact sheets and `report.json` into `dir`.
    pub fn write(&self, dir: &Path) -> anyhow::Result<Vec<PathBuf>> {
        std::fs::create_dir_all(dir)?;
        let mut written = Vec::new();
        for category in Category::ALL {
            let entries: Vec<&Entry> = self
                .entries
                .iter()
                .filter(|e| e.category == category)
                .collect();
            if entries.is_empty() {
                continue;
            }
            for (v, view) in category.views().iter().enumerate() {
                for (page, chunk) in entries.chunks(sheet::PER_SHEET).enumerate() {
                    let cells: Vec<sheet::Cell> = chunk
                        .iter()
                        .map(|e| sheet::Cell {
                            id: &e.id,
                            image: &e.views[v.min(e.views.len() - 1)],
                            flag: e.worst(),
                        })
                        .collect();
                    let path = dir.join(format!(
                        "{}-{}-{}.png",
                        category.name().to_lowercase(),
                        view,
                        page + 1
                    ));
                    sheet::compose(&cells).save(&path)?;
                    written.push(path);
                }
            }
        }
        let flags: Vec<ReportFlag> = self
            .flags()
            .into_iter()
            .map(|(c, f)| ReportFlag {
                detector: f.detector.name(),
                category: c.name(),
                id: &f.id,
                part: &f.part,
                detail: &f.detail,
                severity: f.detector.severity(),
            })
            .collect();
        std::fs::write(
            dir.join("report.json"),
            serde_json::to_string_pretty(&serde_json::json!({
                "stack": self.stack.name,
                "packs": self.stack.packs,
                "entries": self.entries.len(),
                "flags": flags,
            }))?,
        )?;
        std::fs::write(
            dir.join("summary.txt"),
            self.step0.join("\n") + "\n" + &self.summary(20),
        )?;
        Ok(written)
    }
}

/// One block (`block/stone`) or mob (`mob/pig`, `mob/pig#1`, `mob/pig#baby`)
/// drawn large from a few angles side by side, for looking at a single
/// problem: `--texture-audit --entry mob/pig`.
pub fn entry_picture(assets: &Path, stack: &Stack, id: &str) -> anyhow::Result<RgbaImage> {
    let mobs = MobRegistry::load(&assets.join("data/mobs.ron"))?;
    let look = build_look(assets, stack, &mobs)?;
    let subject = if let Some(name) = id.strip_prefix("mob/") {
        let mut parts = name.split('#');
        let base = parts.next().unwrap_or_default();
        let kind = mobs
            .find(base)
            .ok_or_else(|| anyhow::anyhow!("no mob {base:?}"))?;
        let (mut variant, mut baby) = (0, false);
        for part in parts {
            match part {
                "baby" => baby = true,
                n => variant = n.parse()?,
            }
        }
        Subject::Mob {
            kind,
            variant,
            baby,
        }
    } else if let Some(name) = id.strip_prefix("block/") {
        let def = block::registry()
            .iter()
            .find(|d| d.name == name)
            .ok_or_else(|| anyhow::anyhow!("no block {name:?}"))?;
        Subject::Block(BlockState::of(def.id))
    } else {
        anyhow::bail!("--entry wants block/<name> or mob/<name>[#variant][#baby]");
    };
    let views = match subject {
        Subject::Mob { .. } => vec![View::FRONT, View::SIDE, View::BACK, View::TOP],
        _ => vec![View::THREE_QUARTER, View::TOP],
    };
    let size = 256;
    let mut picture = RgbaImage::from_pixel(size * views.len() as u32, size, BACKGROUND);
    for (i, view) in views.into_iter().enumerate() {
        let (image, _) = render_subject(&look, &mobs, &subject, view, size);
        image::imageops::replace(&mut picture, &image, i64::from(size) * i as i64, 0);
    }
    Ok(picture)
}

/// `--texture-audit [--mode <mode>]`: runs the audit, writes
/// `target/texture-audit/<mode>/` and prints its summary.
pub fn cli(mode: &str, entry: Option<&str>) -> anyhow::Result<()> {
    let assets = crate::assets::dir();
    let dir = assets.join("downloads/resourcepacks");
    let stack = stack(mode, &dir)
        .ok_or_else(|| anyhow::anyhow!("unknown mode {mode:?} (vanilla, barebones, realistic)"))?;
    if let Some(id) = entry {
        let out = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("target/texture-audit")
            .join(mode)
            .join(format!("entry-{}.png", id.replace(['/', '#'], "_")));
        std::fs::create_dir_all(out.parent().unwrap())?;
        entry_picture(&assets, &stack, id)?.save(&out)?;
        println!("{}", out.display());
        return Ok(());
    }
    let started = std::time::Instant::now();
    let audit = run(&assets, &stack, None)?;
    let out = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("target/texture-audit")
        .join(mode);
    let sheets = audit.write(&out)?;
    for line in &audit.step0 {
        if !line.starts_with("mob ") || line.ends_with("MISMATCH") {
            println!("{line}");
        }
    }
    print!("{}", audit.summary(20));
    println!(
        "{} sheets in {} ({:.1} s)",
        sheets.len(),
        out.display(),
        started.elapsed().as_secs_f32()
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vanilla_present() -> bool {
        let present = crate::assets::dir()
            .join("vanilla/assets/minecraft")
            .is_dir();
        if !present {
            eprintln!("skipping: no assets/vanilla (run scripts/fetch-vanilla-assets.sh)");
        }
        present
    }

    /// The whole audit runs over every block, item and mob without a panic,
    /// draws every one, and writes sheets of at most 256 and a report.
    #[test]
    fn the_audit_finishes_for_every_block_item_and_mob() {
        if !vanilla_present() {
            return;
        }
        let assets = crate::assets::dir();
        let stack = stack("vanilla", &assets.join("downloads/resourcepacks")).unwrap();
        let audit = run(&assets, &stack, None).unwrap();
        let count = |c| audit.entries.iter().filter(|e| e.category == c).count();
        assert!(
            count(Category::Blocks) > 1000,
            "{} blocks",
            count(Category::Blocks)
        );
        assert_eq!(
            count(Category::Items),
            item::registry().len().saturating_sub(0)
        );
        assert!(
            count(Category::Mobs)
                >= MobRegistry::load(&assets.join("data/mobs.ron"))
                    .unwrap()
                    .iter()
                    .count()
        );
        for e in &audit.entries {
            assert_eq!(e.views.len(), e.category.views().len(), "{}", e.id);
        }
        // Grass is tinted, so the tint detector has nothing to say about it.
        let grass = audit
            .entries
            .iter()
            .find(|e| e.id == "block/grass_block")
            .unwrap();
        assert!(
            grass.flags.iter().all(|f| f.detector != Detector::Tint),
            "{:?}",
            grass.flags
        );
        // Vanilla's textures have no missing-texture placeholder anywhere.
        let placeholders: Vec<_> = audit
            .flags()
            .into_iter()
            .filter(|(_, f)| f.detector == Detector::Placeholder)
            .map(|(_, f)| f.id.clone())
            .collect();
        assert!(placeholders.is_empty(), "{placeholders:?}");
        // The audit's zero-flag rules for vanilla's own textures.
        for (_, f) in audit.flags() {
            assert!(
                !matches!(
                    f.detector,
                    Detector::SizeMismatch | Detector::UnknownTexture
                ),
                "{f:?}"
            );
        }
        let dir = crate::world::save::TempDir::new("audit-out");
        let sheets = audit.write(dir.path()).unwrap();
        assert!(sheets.len() >= 10);
        for path in &sheets {
            let image = image::open(path).unwrap();
            assert!(image.width() <= sheet::COLUMNS * 68 && image.height() <= sheet::COLUMNS * 80);
        }
        let report = std::fs::read_to_string(dir.path().join("report.json")).unwrap();
        assert!(report.contains("\"flags\""));
        assert!(audit.summary(5).contains("per detector"));
    }

    /// Every item has an icon that isn't empty, except the few vanilla
    /// draws nothing for.
    #[test]
    fn a_known_bad_mob_layout_is_still_found() {
        if !vanilla_present() {
            return;
        }
        let assets = crate::assets::dir();
        let stack = stack("vanilla", &assets.join("downloads/resourcepacks")).unwrap();
        let audit = run(&assets, &stack, None).unwrap();
        // The cod's boxes reach past its 32×32 texture: the UV detector
        // sees it (listed in docs/textures.md).
        let flagged = |id: &str, d: Detector| {
            audit
                .flags()
                .iter()
                .any(|(_, f)| f.id == id && f.detector == d)
        };
        assert!(
            flagged("mob/cod", Detector::UvOutOfRange),
            "{}",
            audit.summary(50)
        );
    }

    #[test]
    fn the_barebones_stack_is_found_by_name_top_first() {
        let dir = crate::world::save::TempDir::new("stack-packs");
        for name in [
            "Bare Bones 1.21.11.zip",
            "Bare Bones x Fresh Animations 1.10.4.zip",
            "Barebones+Leaves+Add-on+main.zip",
            "FreshAnimations_v1.11.2_26.1+.zip",
            "Patrix_26.3_32x_basic.zip",
        ] {
            std::fs::write(dir.path().join(name), "x").unwrap();
        }
        let barebones = stack("barebones", dir.path()).unwrap();
        assert_eq!(
            barebones.packs,
            [
                "Bare Bones x Fresh Animations 1.10.4.zip",
                "FreshAnimations_v1.11.2_26.1+.zip",
                "Barebones+Leaves+Add-on+main.zip",
                "Bare Bones 1.21.11.zip"
            ]
        );
        assert_eq!(stack("vanilla", dir.path()).unwrap().packs.len(), 0);
        assert_eq!(
            stack("realistic", dir.path()).unwrap().packs,
            ["Patrix_26.3_32x_basic.zip"]
        );
        // With some missing, the stack has what's there.
        std::fs::remove_file(dir.path().join("Bare Bones x Fresh Animations 1.10.4.zip")).unwrap();
        assert_eq!(stack("barebones", dir.path()).unwrap().packs.len(), 3);
        assert!(stack("nonsense", dir.path()).is_none());
    }

    #[test]
    fn uv_overlays_draw_a_box_around_each_quad() {
        use crate::render::vertex::Vertex;
        let texture = RgbaImage::from_pixel(8, 8, Rgba([0, 0, 0, 255]));
        let mut mesh = Mesh::default();
        let v = |u: f32, t: f32| Vertex {
            position: [0.0; 3],
            uv: [u, t],
            color: [1.0; 3],
            light: [15.0, 0.0],
        };
        mesh.vertices = vec![v(0.25, 0.25), v(0.75, 0.25), v(0.75, 0.75), v(0.25, 0.75)];
        let image = uv_overlay(&texture, &mesh, 64);
        assert_eq!(image.width(), 64);
        assert_eq!(image.get_pixel(16, 16).0, [255, 255, 0, 255]);
        assert_eq!(image.get_pixel(30, 30).0, [0, 0, 0, 255]);
    }

    /// A Fresh Animations pig, cow and chicken at rest stand where
    /// vanilla's do: the same height and length to within a quarter, feet
    /// on the ground.
    /// (Their parts used to hang eight pixels too high.) Skipped, with a
    /// message, where the pack isn't in assets/downloads.
    #[test]
    fn fresh_animations_mobs_have_vanillas_silhouette() {
        let assets = crate::assets::dir();
        let dir = assets.join("downloads/resourcepacks");
        let Some(fa) = find_pack(&dir, &["freshanimations"], &["barebones"]) else {
            eprintln!("skipping: no Fresh Animations in {}", dir.display());
            return;
        };
        if !assets.join("vanilla/assets/minecraft").is_dir() {
            eprintln!("skipping: no assets/vanilla");
            return;
        }
        let mobs = MobRegistry::load(&assets.join("data/mobs.ron")).unwrap();
        let packs = crate::look::open_packs_in(&crate::pack::packs_dir(), &dir, &[fa]);
        let look = Look::build(&assets, packs, &mobs).unwrap();
        let extent = |mesh: &Mesh| {
            let (mut lo, mut hi) = (glam::Vec3::splat(f32::MAX), glam::Vec3::splat(f32::MIN));
            for v in &mesh.vertices {
                lo = lo.min(glam::Vec3::from(v.position));
                hi = hi.max(glam::Vec3::from(v.position));
            }
            (lo, hi)
        };
        for name in ["pig", "cow", "chicken"] {
            let kind = mobs.find(name).unwrap();
            let def = mobs.get(kind);
            let model = look
                .cem
                .models
                .get(name)
                .cloned()
                .unwrap_or_else(|| panic!("no {name} model"));
            let mut mob = Mob::new(kind, def, DVec3::ZERO, 0.0);
            mob.id = 7;
            crate::cem::begin_frame(0.0, 0.5, DVec3::ZERO);
            let mut vanilla = Mesh::default();
            crate::render::models::add_mob_model_using(
                &mut vanilla,
                &mob,
                def,
                &def.model,
                0.5,
                (15, 0),
                [1.0; 3],
                None,
            );
            let mut custom = Mesh::default();
            crate::render::models::add_mob_model_using(
                &mut custom,
                &mob,
                def,
                &def.model,
                0.5,
                (15, 0),
                [1.0; 3],
                Some(model),
            );
            let ((vlo, vhi), (clo, chi)) = (extent(&vanilla), extent(&custom));
            let (vsize, csize) = (vhi - vlo, chi - clo);
            // Height and length: the width is wings (a chicken's are folded
            // in Fresh Animations' rest pose, spread in vanilla's).
            for axis in 1..3 {
                let ratio = csize[axis] / vsize[axis];
                assert!(
                    (0.75..=1.3).contains(&ratio),
                    "{name} axis {axis}: {csize} against vanilla {vsize}"
                );
            }
            assert!(
                (clo.y - vlo.y).abs() < 0.15,
                "{name} feet: {clo} against {vlo}"
            );
        }
    }

    /// Grass, leaves and vines are tinted in a plains chunk (green, not
    /// gray) and stone is not.
    #[test]
    fn grass_and_leaves_are_tinted_green_and_stone_is_not() {
        let assets = crate::assets::dir();
        if !assets.join("vanilla/assets/minecraft").is_dir() {
            eprintln!("skipping: no assets/vanilla");
            return;
        }
        let mobs = MobRegistry::load(&assets.join("data/mobs.ron")).unwrap();
        let look = Look::build(&assets, Vec::new(), &mobs).unwrap();
        let green = |name: &str| {
            let def = block::registry().iter().find(|d| d.name == name).unwrap();
            let mesh = block_mesh(&look, BlockState::of(def.id));
            mesh.vertices
                .iter()
                .any(|v| v.color[1] > v.color[0] + 0.05 && v.color[1] > v.color[2] + 0.05)
        };
        for tinted in ["grass_block", "short_grass", "oak_leaves", "fern"] {
            assert!(green(tinted), "{tinted} is not tinted green");
        }
        assert!(!green("stone"));
    }
}
