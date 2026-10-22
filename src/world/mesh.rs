//! Chunk meshing: turns a chunk's blocks into triangles, skipping hidden faces.

use std::sync::Arc;

use super::{
    Face,
    biome::{Biome, BiomeColors},
    blend::Tints,
    block::{
        BlockRegistry, BlockState, FaceTint, Fluid, RenderLayer, Shape, Textures, door_box,
        door_side, texture_location,
    },
    chunk::{Chunk, ChunkPos, SECTION_COUNT, SECTION_SIZE},
    fluid::{self, Cell},
    light::ChunkLight,
    props::{Prop, axis, bed_part, hinge, rail_shape},
    shape::{self, Cuboid},
};
use crate::{
    config::{self, CHUNK_SIZE, WORLD_HEIGHT},
    pack::model::BlockOverrides,
    render::vertex::Vertex,
};

/// How one face of a block is drawn.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FaceTexture {
    /// Atlas UV rect `[u0, v0, u1, v1]`, v0 at the top.
    pub uv: [f32; 4],
    pub tint: FaceTint,
    /// Quarter turns of the texture on the face, clockwise (a log lying
    /// on its side turns its bark).
    pub rotation: u8,
}

impl FaceTexture {
    /// Texture coordinates for the corners of `face_corners` order
    /// (bottom-left, bottom-right, top-right, top-left), turned by
    /// `rotation`.
    pub fn corner_uvs(&self) -> [[f32; 2]; 4] {
        let [u0, v0, u1, v1] = self.uv;
        let mut uvs = [[u0, v1], [u1, v1], [u1, v0], [u0, v0]];
        uvs.rotate_left(self.rotation as usize % 4);
        uvs
    }
}

/// Everything the mesher needs to know about one block state.
#[derive(Debug, Clone, PartialEq)]
pub struct BlockModel {
    pub shape: Shape,
    /// Hides any face touching it.
    pub opaque_cube: bool,
    /// Hides faces between two blocks of the same kind.
    pub cull_same: bool,
    /// Light lost passing through it, 0–15.
    pub opacity: u8,
    /// Light it gives off, 0–15.
    pub emission: u8,
    /// Drawn in the blended pass (water, ice, glass).
    pub translucent: bool,
    /// Water or lava, flowing or still.
    pub fluid: Option<Fluid>,
    /// Holds fluids back (`BlockDef::stops_fluid`).
    pub stops_fluid: bool,
    /// Indexed by `Face::index`.
    pub faces: [FaceTexture; 6],
    /// The boxes it's drawn as, for shapes drawn from boxes.
    pub boxes: Option<Vec<Cuboid>>,
    /// Faces of each of `boxes` when they have textures of their own (a
    /// model's elements); otherwise every box takes `faces`.
    pub part_faces: Vec<[FaceTexture; 6]>,
}

/// Block models for every block state, resolved against the texture atlas.
pub struct BlockModels {
    registry: &'static BlockRegistry,
    /// By `BlockRegistry::state_index`.
    models: Vec<BlockModel>,
    /// By `BlockRegistry::state_index`: the looks a pack's blockstate file
    /// picks between by position, with running weight totals; empty for
    /// most states.
    choices: Vec<Vec<(u32, BlockModel)>>,
    colors: BiomeColors,
}

impl BlockModels {
    /// `uv` maps a texture location (`block/stone`) to its atlas rect, or
    /// `None` if it's missing; missing textures use `missing_uv`.
    pub fn new(
        registry: &'static BlockRegistry,
        uv: impl Fn(&str) -> Option<[f32; 4]>,
        missing_uv: [f32; 4],
        colors: BiomeColors,
    ) -> Self {
        Self::with_packs(registry, uv, missing_uv, colors, &BlockOverrides::default())
    }

    /// Like `new`, with textures from resource pack models where they
    /// cover a state.
    pub fn with_packs(
        registry: &'static BlockRegistry,
        uv: impl Fn(&str) -> Option<[f32; 4]>,
        missing_uv: [f32; 4],
        colors: BiomeColors,
        overrides: &BlockOverrides,
    ) -> Self {
        let mut models = Vec::with_capacity(registry.state_count());
        let mut choices = Vec::with_capacity(registry.state_count());
        for state in registry.states() {
            let def = state.def();
            let ours = def.textures_for(state);
            let build = |textures: Option<&Textures>| build_model(state, textures, &uv, missing_uv);
            match overrides.get(state) {
                Some(list) => {
                    let mut total = 0;
                    let looks: Vec<(u32, BlockModel)> = list
                        .iter()
                        .map(|choice| {
                            // Every template names all six faces.
                            total += choice.weight;
                            (total, build(choice.textures.as_ref().or(ours.as_ref())))
                        })
                        .collect();
                    models.push(looks[0].1.clone());
                    choices.push(if looks.len() > 1 { looks } else { Vec::new() });
                }
                None => {
                    models.push(build(ours.as_ref()));
                    choices.push(Vec::new());
                }
            }
        }
        Self {
            registry,
            models,
            choices,
            colors,
        }
    }

    /// sRGB color for a face tint on an item icon or a dropped item, which
    /// isn't in any biome: plains colors.
    pub fn icon_color(&self, tint: FaceTint) -> [f32; 3] {
        self.tint_color(tint, Biome::Plains)
    }

    /// sRGB color for a face tint in `biome`.
    fn tint_color(&self, tint: FaceTint, biome: Biome) -> [f32; 3] {
        match tint {
            FaceTint::None => [1.0; 3],
            FaceTint::Grass => self.colors.grass(biome),
            FaceTint::Foliage => self.colors.foliage(biome),
            FaceTint::Rgb(rgb) => rgb,
        }
    }

    pub fn get(&self, block: BlockState) -> &BlockModel {
        &self.models[self.registry.state_index(block)]
    }

    /// The model of `block` at a position: a pack can give a state several
    /// weighted looks, picked by position as vanilla does (stone with
    /// random variations).
    pub fn get_at(&self, block: BlockState, x: i32, y: i32, z: i32) -> &BlockModel {
        let index = self.registry.state_index(block);
        let choices = &self.choices[index];
        let Some(&(total, _)) = choices.last() else {
            return &self.models[index];
        };
        let pick = (position_seed(x, y, z) as u64 % total as u64) as u32;
        choices
            .iter()
            .find(|(running, _)| pick < *running)
            .map_or(&self.models[index], |(_, model)| model)
    }
}

/// Vanilla's `Mth.getSeed`: a hash of a block position that picks its
/// random model.
fn position_seed(x: i32, y: i32, z: i32) -> i64 {
    let mut l =
        (x.wrapping_mul(3_129_871) as i64) ^ (z as i64).wrapping_mul(116_129_781) ^ y as i64;
    l = l
        .wrapping_mul(l)
        .wrapping_mul(42_317_861)
        .wrapping_add(l.wrapping_mul(11));
    (l >> 16).wrapping_abs()
}

/// One block state's model from its textures.
fn build_model(
    state: BlockState,
    textures: Option<&Textures>,
    uv: &impl Fn(&str) -> Option<[f32; 4]>,
    missing_uv: [f32; 4],
) -> BlockModel {
    let def = state.def();
    let tint = def.tint_for(state);
    let texture = |face: Face| FaceTexture {
        uv: textures
            .and_then(|t| t.get(face))
            .and_then(|name| uv(&texture_location(name)))
            .unwrap_or(missing_uv),
        tint: tint.for_face(face),
        rotation: 0,
    };
    let mut faces = Face::ALL.map(texture);
    // Blocks facing a way turn their south face that way.
    if let Some(front) = def.front(state) {
        faces.swap(Face::South.index(), front.index());
    }
    // A log lying along x or z shows its ends on that axis and its
    // bark turned to run along it.
    if state.has(Prop::Axis) {
        let (end, side) = (faces[Face::Top.index()], faces[Face::North.index()]);
        let turned = FaceTexture {
            rotation: 1,
            ..side
        };
        match state.get(Prop::Axis) {
            axis::X => {
                faces = Face::ALL.map(|face| match face {
                    Face::East | Face::West => end,
                    _ => turned,
                });
            }
            axis::Z => {
                faces = Face::ALL.map(|face| match face {
                    Face::North | Face::South => end,
                    Face::East | Face::West => turned,
                    _ => side,
                });
            }
            _ => {}
        }
    }
    // A model's boxes each wear their own textures, turned with the block.
    let part_faces: Vec<[FaceTexture; 6]> = def
        .parts_for(state)
        .iter()
        .map(|part| {
            let mut faces = Face::ALL.map(|face| FaceTexture {
                uv: part
                    .textures
                    .get(face)
                    .and_then(|name| uv(&texture_location(name)))
                    .unwrap_or(missing_uv),
                tint: FaceTint::None,
                rotation: 0,
            });
            // The sides of the block turn with its box.
            if state.has(Prop::Facing) {
                for _ in 0..crate::world::shape::turns_of(state.face(Prop::Facing)) {
                    let before = faces;
                    for face in [Face::North, Face::East, Face::South, Face::West] {
                        faces[face.rotate_cw().index()] = before[face.index()];
                    }
                }
            }
            faces
        })
        .collect();
    if let (true, Some(first)) = (
        def.textures.is_none() && def.shape == Shape::Model,
        part_faces.first(),
    ) {
        faces = *first;
    }
    BlockModel {
        shape: def.shape,
        opaque_cube: def.is_opaque_cube_state(state),
        cull_same: def.cull_same,
        opacity: def.light_opacity_for(state),
        emission: def.light_for(state),
        translucent: def.layer == RenderLayer::Translucent,
        fluid: def.fluid,
        stops_fluid: def.stops_fluid(),
        faces,
        boxes: shape::render_boxes(state),
        part_faces,
    }
}

/// A chunk plus its eight neighbors, so faces on the border can be culled
/// and biome colors blended across it.
pub struct ChunkArea {
    /// `chunks[dz + 1][dx + 1]` is the chunk at offset (dx, dz) from the center.
    chunks: [[Option<Arc<Chunk>>; 3]; 3],
    /// Biome blend radius in columns (0 is off).
    pub blend: i32,
}

impl ChunkArea {
    pub fn new(center: Arc<Chunk>, neighbor: impl Fn(ChunkPos) -> Option<Arc<Chunk>>) -> Self {
        let pos = center.pos;
        let mut chunks: [[Option<Arc<Chunk>>; 3]; 3] = Default::default();
        for (dz, row) in chunks.iter_mut().enumerate() {
            for (dx, slot) in row.iter_mut().enumerate() {
                *slot = neighbor(pos.offset(dx as i32 - 1, dz as i32 - 1));
            }
        }
        chunks[1][1] = Some(center);
        Self {
            chunks,
            blend: config::BIOME_BLEND,
        }
    }

    /// The biome of a column relative to the center chunk's origin, up to a
    /// chunk into the neighbors. Where a neighbor isn't loaded, the nearest
    /// column of the center chunk stands in.
    pub fn biome(&self, x: i32, z: i32) -> Biome {
        let size = CHUNK_SIZE as i32;
        let last = size as usize - 1;
        let (cx, cz) = (x.div_euclid(size), z.div_euclid(size));
        let neighbor = self
            .chunks
            .get((cz + 1) as usize)
            .and_then(|row| row.get((cx + 1) as usize))
            .and_then(|chunk| chunk.as_deref());
        match neighbor {
            Some(chunk) => chunk.biome(x.rem_euclid(size) as usize, z.rem_euclid(size) as usize),
            None => self.center().biome(
                x.clamp(0, last as i32) as usize,
                z.clamp(0, last as i32) as usize,
            ),
        }
    }

    /// One past the highest non-empty section of any loaded chunk in the area.
    pub fn top_section(&self) -> usize {
        self.chunks
            .iter()
            .flatten()
            .flatten()
            .map(|chunk| chunk.top_section())
            .max()
            .unwrap_or(0)
    }

    pub fn center(&self) -> &Chunk {
        self.chunks[1][1]
            .as_deref()
            .expect("center chunk is always set")
    }

    /// Block at coordinates relative to the center chunk's origin (x and z may
    /// reach into neighbors). Above the world is air; below the world or in an
    /// unloaded chunk is `None`.
    pub fn get(&self, x: i32, y: i32, z: i32) -> Option<BlockState> {
        if y >= WORLD_HEIGHT as i32 {
            return Some(BlockState::AIR);
        }
        if y < 0 {
            return None;
        }
        let size = CHUNK_SIZE as i32;
        let (cx, cz) = (x.div_euclid(size), z.div_euclid(size));
        let chunk = self.chunks.get((cz + 1) as usize)?.get((cx + 1) as usize)?;
        let (lx, lz) = (x.rem_euclid(size), z.rem_euclid(size));
        Some(chunk.as_ref()?.get(lx as usize, y as usize, lz as usize))
    }
}

#[derive(Debug, Default, Clone, PartialEq)]
pub struct Mesh {
    pub vertices: Vec<Vertex>,
    /// Opaque and cutout faces.
    pub indices: Vec<u32>,
    /// Faces for the blended pass, drawn after everything opaque.
    pub translucent: Vec<u32>,
    /// Where `quad` puts new faces.
    adding_translucent: bool,
}

impl Mesh {
    pub fn is_empty(&self) -> bool {
        self.indices.is_empty() && self.translucent.is_empty()
    }

    /// Adds another mesh's faces after this one's.
    pub fn append(&mut self, other: &Mesh) {
        let base = self.vertices.len() as u32;
        self.vertices.extend_from_slice(&other.vertices);
        self.indices.extend(other.indices.iter().map(|i| i + base));
        self.translucent
            .extend(other.translucent.iter().map(|i| i + base));
    }

    /// Adds a quad from corners ordered bottom-left, bottom-right, top-right,
    /// top-left as seen from its front (counter-clockwise), with a color and
    /// (sky, block) light per corner.
    pub fn quad(
        &mut self,
        corners: [[f32; 3]; 4],
        uv: [f32; 4],
        colors: [[f32; 3]; 4],
        lights: [[f32; 2]; 4],
    ) {
        let [u0, v0, u1, v1] = uv;
        let uvs = [[u0, v1], [u1, v1], [u1, v0], [u0, v0]];
        self.quad_uvs(corners, uvs, colors, lights);
    }

    /// `quad` with a texture coordinate per corner.
    fn quad_uvs(
        &mut self,
        corners: [[f32; 3]; 4],
        uvs: [[f32; 2]; 4],
        colors: [[f32; 3]; 4],
        lights: [[f32; 2]; 4],
    ) {
        let base = self.vertices.len() as u32;
        for i in 0..4 {
            self.vertices.push(Vertex {
                position: corners[i],
                uv: uvs[i],
                color: colors[i],
                light: lights[i],
            });
        }
        // Split along the brighter diagonal so a single dark corner doesn't
        // smear across the whole face.
        let weight =
            |i: usize| colors[i][0] + colors[i][1] + colors[i][2] + lights[i][0].max(lights[i][1]);
        let order = if weight(0) + weight(2) < weight(1) + weight(3) {
            [1, 2, 3, 1, 3, 0]
        } else {
            [0, 1, 2, 0, 2, 3]
        };
        let indices = if self.adding_translucent {
            &mut self.translucent
        } else {
            &mut self.indices
        };
        indices.extend(order.map(|i| base + i));
    }

    /// Adds an evenly lit quad visible from both sides, with a color per
    /// corner.
    fn double_sided_quad(
        &mut self,
        corners: [[f32; 3]; 4],
        uv: [f32; 4],
        colors: [[f32; 3]; 4],
        light: [f32; 2],
    ) {
        let [a, b, c, d] = corners;
        let [ca, cb, cc, cd] = colors;
        self.quad(corners, uv, colors, [light; 4]);
        self.quad([b, a, d, c], uv, [cb, ca, cd, cc], [light; 4]);
    }
}

/// 1.0.0's ambient occlusion value for a block next to a vertex.
fn occlusion(opaque: bool) -> f32 {
    if opaque { 0.2 } else { 1.0 }
}

/// Smooth lighting for one face of the block at chunk-local `cell`, like
/// 1.0.0: each corner averages the light of the four blocks touching it on
/// the face's outer side (dark solid blocks take the face neighbor's light)
/// and darkens by how many of them are opaque. Returns (occlusion, [sky,
/// block]) per corner of `face_corners(face)`.
fn smooth_face(
    models: &BlockModels,
    area: &ChunkArea,
    light: &ChunkLight,
    cell: [i32; 3],
    face: Face,
) -> [(f32, [f32; 2]); 4] {
    let normal = face.normal();
    let front = [
        cell[0] + normal[0],
        cell[1] + normal[1],
        cell[2] + normal[2],
    ];
    let [a1, a2] = match face {
        Face::Top | Face::Bottom => [0, 2],
        Face::North | Face::South => [0, 1],
        Face::East | Face::West => [1, 2],
    };
    let opaque = |p: [i32; 3]| {
        area.get(p[0], p[1], p[2])
            .is_some_and(|b| !b.is_air() && models.get(b).opaque_cube)
    };
    let light_at = |p: [i32; 3]| light.get(p[0], p[1], p[2]);
    let front_light = light_at(front);
    face_corners(face).map(|corner| {
        let step = |p: [i32; 3], axis: usize| {
            let mut p = p;
            p[axis] += if corner[axis] > 0.5 { 1 } else { -1 };
            p
        };
        let side1 = step(front, a1);
        let side2 = step(front, a2);
        let diagonal = step(side1, a2);
        let (o1, o2) = (opaque(side1), opaque(side2));
        // With both sides solid the diagonal is hidden; 1.0.0 uses a side instead.
        let (oc, diagonal) = if o1 && o2 {
            (o1, side1)
        } else {
            (opaque(diagonal), diagonal)
        };
        let ao = (1.0 + occlusion(o1) + occlusion(o2) + occlusion(oc)) / 4.0;
        let mut sum = [front_light.0 as f32, front_light.1 as f32];
        for p in [side1, side2, diagonal] {
            let mut l = light_at(p);
            if l == (0, 0) {
                l = front_light;
            }
            sum[0] += l.0 as f32;
            sum[1] += l.1 as f32;
        }
        (ao, sum.map(|v| v / 4.0))
    })
}

/// Unit cube face corners, counter-clockwise from outside, starting at the
/// texture's bottom-left.
pub(crate) fn face_corners(face: Face) -> [[f32; 3]; 4] {
    match face {
        Face::Top => [[0., 1., 1.], [1., 1., 1.], [1., 1., 0.], [0., 1., 0.]],
        Face::Bottom => [[0., 0., 0.], [1., 0., 0.], [1., 0., 1.], [0., 0., 1.]],
        Face::North => [[1., 0., 0.], [0., 0., 0.], [0., 1., 0.], [1., 1., 0.]],
        Face::South => [[0., 0., 1.], [1., 0., 1.], [1., 1., 1.], [0., 1., 1.]],
        Face::East => [[1., 0., 1.], [1., 0., 0.], [1., 1., 0.], [1., 1., 1.]],
        Face::West => [[0., 0., 0.], [0., 0., 1.], [0., 1., 1.], [0., 1., 0.]],
    }
}

/// Planes of the X-shaped plant model.
const CROSS_PLANES: [[[f32; 3]; 4]; 2] = [
    [[0., 0., 0.], [1., 0., 1.], [1., 1., 1.], [0., 1., 0.]],
    [[0., 0., 1.], [1., 0., 0.], [1., 1., 0.], [0., 1., 1.]],
];

/// How tall fire's planes reach, into the cell above (1.0.0's 1.4).
const FIRE_HEIGHT: f32 = 1.4;

/// Fire's planes (1.0.0's `renderBlockFire`): on a floor, four planes
/// rising from its edges and leaning in; otherwise one leaning out from
/// each solid wall beside it (`walls` in −x, +x, −z, +z order).
fn fire_planes(floor: bool, walls: [bool; 4]) -> Vec<[[f32; 3]; 4]> {
    let h = FIRE_HEIGHT;
    if floor || !walls.contains(&true) {
        return vec![
            [[0., 0., 0.], [0., 0., 1.], [0.7, h, 1.], [0.7, h, 0.]],
            [[1., 0., 1.], [1., 0., 0.], [0.3, h, 0.], [0.3, h, 1.]],
            [[0., 0., 1.], [1., 0., 1.], [1., h, 0.3], [0., h, 0.3]],
            [[1., 0., 0.], [0., 0., 0.], [0., h, 0.7], [1., h, 0.7]],
        ];
    }
    let leaning = [
        [[0., 0., 1.], [0., 0., 0.], [0.2, h, 0.], [0.2, h, 1.]],
        [[1., 0., 0.], [1., 0., 1.], [0.8, h, 1.], [0.8, h, 0.]],
        [[0., 0., 0.], [1., 0., 0.], [1., h, 0.2], [0., h, 0.2]],
        [[1., 0., 1.], [0., 0., 1.], [0., h, 0.8], [1., h, 0.8]],
    ];
    leaning
        .into_iter()
        .zip(walls)
        .filter_map(|(plane, wall)| wall.then_some(plane))
        .collect()
}

/// Planes of the #-shaped crop model, 4 pixels in from each edge.
const CROP_PLANES: [[[f32; 3]; 4]; 4] = [
    [
        [0.25, 0., 0.],
        [0.25, 0., 1.],
        [0.25, 1., 1.],
        [0.25, 1., 0.],
    ],
    [
        [0.75, 0., 0.],
        [0.75, 0., 1.],
        [0.75, 1., 1.],
        [0.75, 1., 0.],
    ],
    [
        [0., 0., 0.25],
        [1., 0., 0.25],
        [1., 1., 0.25],
        [0., 1., 0.25],
    ],
    [
        [0., 0., 0.75],
        [1., 0., 0.75],
        [1., 1., 0.75],
        [0., 1., 0.75],
    ],
];

/// 1.0.0's torch (`RenderBlocks.renderTorchAtAngle`): four planes cut from
/// the full texture 1/16 either side of the center, which leaves a 2×2-pixel
/// stick, plus a small cap at 10/16. Wall torches sit 0.2 higher, shifted
/// 0.1 and leaning 0.4 at the base toward their wall. Returns (corners, uv)
/// per quad, block-relative.
fn torch_quads(state: BlockState, uv: [f32; 4]) -> [([[f32; 3]; 4], [f32; 4]); 5] {
    const HALF: f32 = 1.0 / 16.0;
    const CAP: f32 = 0.625;
    let (shift, lean) = if state.has(Prop::Facing) {
        // `facing` points away from the wall.
        let [nx, _, nz] = state.face(Prop::Facing).normal().map(|n| n as f32);
        ([-0.1 * nx, 0.2, -0.1 * nz], [-0.4 * nx, -0.4 * nz])
    } else {
        ([0.0; 3], [0.0; 2])
    };
    let place = |c: [f32; 3], lean_by: f32| {
        [
            c[0] + shift[0] + lean[0] * lean_by,
            c[1] + shift[1],
            c[2] + shift[2] + lean[1] * lean_by,
        ]
    };
    // Sides: move the face plane in to 7/16 or 9/16, then lean the base.
    let side = |face: Face, axis: usize| {
        face_corners(face).map(|mut c| {
            c[axis] = 0.5 + if c[axis] > 0.5 { HALF } else { -HALF };
            place(c, if c[1] == 0.0 { 1.0 } else { 0.0 })
        })
    };
    let cap = face_corners(Face::Top).map(|c| {
        let squeeze = |v: f32| 0.5 + if v > 0.5 { HALF } else { -HALF };
        place([squeeze(c[0]), CAP, squeeze(c[2])], 1.0 - CAP)
    });
    let [u0, v0, u1, v1] = uv;
    let (du, dv) = ((u1 - u0) / 16.0, (v1 - v0) / 16.0);
    let cap_uv = [u0 + 7.0 * du, v0 + 6.0 * dv, u0 + 9.0 * du, v0 + 8.0 * dv];
    [
        (side(Face::West, 0), uv),
        (side(Face::East, 0), uv),
        (side(Face::North, 2), uv),
        (side(Face::South, 2), uv),
        (cap, cap_uv),
    ]
}

/// One face of a shaped block: the face it shows (for culling and shade),
/// its block-relative corners and its uv rect.
type Quad = (Face, [[f32; 3]; 4], [f32; 4]);

/// Height of a bed's top, and of the planks under it (1.0.0's
/// `renderBlockBed`).
const BED_TOP: f32 = 9.0 / 16.0;
const BED_BOTTOM: f32 = 3.0 / 16.0;

/// A bed half's quads, block-relative: the top (its texture running from
/// foot to head), the planks underneath, the two long sides and the end
/// away from the other half, each with the face it shows and is shaded as.
/// `model` holds the half's textures (`blocks.ron` puts the foot's end on
/// south and the head's on north).
fn bed_quads(state: BlockState, model: &BlockModel) -> Vec<Quad> {
    let head = state.get(Prop::BedPart) == bed_part::HEAD;
    let facing = state.face(Prop::Facing);
    let toward_head = glam::IVec3::from_array(facing.normal());
    let along = [
        toward_head.x as f32,
        toward_head.y as f32,
        toward_head.z as f32,
    ];
    let texture = |face: Face| model.faces[face.index()].uv;
    // The top's u runs along +x; turning its corners points it at the head.
    let mut top = face_corners(Face::Top).map(|[x, _, z]| [x, BED_TOP, z]);
    // Quarter turns from south: 1.0.0's direction 0–3 (south, west,
    // north, east).
    let turns = match facing {
        Face::West => 1,
        Face::North => 2,
        Face::East => 3,
        _ => 0,
    };
    top.rotate_left(3 - turns);
    let bottom = face_corners(Face::Bottom).map(|[x, _, z]| [x, BED_BOTTOM, z]);
    let mut quads = vec![
        (Face::Top, top, texture(Face::Top)),
        (Face::Bottom, bottom, texture(Face::Bottom)),
    ];
    let end = texture(if head { Face::North } else { Face::South });
    for face in [Face::North, Face::South, Face::East, Face::West] {
        let normal = face.normal().map(|n| n as f32);
        let facing_along = normal[0] * along[0] + normal[2] * along[2];
        let corners = face_corners(face);
        if facing_along == 0.0 {
            // A long side: flip the texture where the face's u runs from head
            // to foot, so the legs stay at the foot.
            let u = [
                corners[1][0] - corners[0][0],
                0.0,
                corners[1][2] - corners[0][2],
            ];
            let [u0, v0, u1, v1] = texture(Face::East);
            let uv = if u[0] * along[0] + u[2] * along[2] < 0.0 {
                [u1, v0, u0, v1]
            } else {
                [u0, v0, u1, v1]
            };
            quads.push((face, corners, uv));
        } else if (facing_along > 0.0) == head {
            // The end away from the other half.
            quads.push((face, corners, end));
        }
    }
    quads
}

/// A door half's six faces, block-relative: its slab (`door_box`) with
/// each face's texture cut to the part of the slab it covers, and the
/// wide faces mirrored as 1.0.0's `BlockDoor.getBlockTexture` does so the
/// handle sits away from the hinge.
fn door_quads(state: BlockState, model: &BlockModel) -> [Quad; 6] {
    let (min, max) = door_box(state);
    let data = legacy_door_data(state);
    let side = match door_side(state) {
        Face::North => 0,
        Face::East => 1,
        Face::South => 2,
        _ => 3,
    };
    Face::ALL.map(|face| {
        let corners = face_corners(face)
            .map(|c| [0, 1, 2].map(|axis| if c[axis] > 0.5 { max[axis] } else { min[axis] }));
        // Where along the texture each corner falls: u from the face's
        // first corner toward its second, v down from the top.
        let (u_axis, u_flip, v_axis, v_flip) = match face {
            Face::Top => (0, false, 2, false),
            Face::Bottom => (0, false, 2, true),
            Face::North => (0, true, 1, true),
            Face::South => (0, false, 1, true),
            Face::East => (2, true, 1, true),
            Face::West => (2, false, 1, true),
        };
        let along = |flip: bool, value: f32| if flip { 1.0 - value } else { value };
        let (a, b) = (
            along(u_flip, corners[0][u_axis]),
            along(u_flip, corners[1][u_axis]),
        );
        let (c, d) = (
            along(v_flip, corners[3][v_axis]),
            along(v_flip, corners[0][v_axis]),
        );
        let [u0, v0, u1, v1] = model.faces[face.index()].uv;
        let lerp = |from: f32, to: f32, t: f32| from + (to - from) * t;
        let mut uv = [
            lerp(u0, u1, a),
            lerp(v0, v1, c),
            lerp(u0, u1, b),
            lerp(v0, v1, d),
        ];
        if door_mirrored(data, side, face) {
            uv.swap(0, 2);
        }
        (face, corners, uv)
    })
}

/// The 1.0.0 data value (direction 0–3 plus 4 if open) that draws like a
/// door state: with its hinge on the left, direction 3 faces north, 0
/// east, 1 south and 2 west; a right hinge is 1.0.0's way of hinging a
/// door the other side, a quarter turn back with "open" flipped.
fn legacy_door_data(state: BlockState) -> u8 {
    let dir = match state.face(Prop::Facing) {
        Face::East => 0,
        Face::South => 1,
        Face::West => 2,
        _ => 3,
    };
    let open = state.bool(Prop::Open);
    if state.get(Prop::Hinge) == hinge::LEFT {
        dir | if open { 4 } else { 0 }
    } else {
        ((dir + 3) & 3) | if open { 0 } else { 4 }
    }
}

/// Whether a door face's texture is drawn mirrored (1.0.0's negative
/// texture index for the wide faces).
fn door_mirrored(data: u8, side: u8, face: Face) -> bool {
    let mc_side = match face {
        Face::North => 2,
        Face::South => 3,
        Face::West => 4,
        Face::East => 5,
        Face::Top | Face::Bottom => return false,
    };
    // Only the wide faces: north and south for a door along x.
    if (side == 0 || side == 2) != (mc_side <= 3) {
        return false;
    }
    let turn = side / 2 + ((mc_side & 1) ^ side) + (data & 4) / 4;
    turn & 1 != 0
}

/// Water or lava at chunk-local `cell` (1.0.0's
/// `RenderBlocks.renderBlockFluids`): each corner of the surface at its
/// own height, the flowing texture turned to run downstream at half
/// scale (still fluid shows the still texture), and sides cut to the
/// surface. The surface is seen from below too.
fn mesh_liquid(
    mesh: &mut Mesh,
    models: &BlockModels,
    area: &ChunkArea,
    light: &ChunkLight,
    cell: [i32; 3],
    origin: [f32; 3],
    tints: &Tints,
) {
    let [x, y, z] = cell;
    let Some(block) = area.get(x, y, z) else {
        return;
    };
    let model = models.get(block);
    let Some(fluid) = model.fluid else {
        return;
    };
    let lookup = |at: glam::IVec3| {
        let block = area.get(at.x, at.y, at.z)?;
        let model = models.get(block);
        Some(Cell {
            block,
            fluid: model.fluid,
            stops: model.stops_fluid,
        })
    };
    let at = glam::IVec3::from_array(cell);
    let heights = [[0, 0], [0, 1], [1, 0], [1, 1]]
        .map(|[cx, cz]| fluid::corner_height(fluid, at + glam::IVec3::new(cx, 0, cz), &lookup));
    let height = |cx: f32, cz: f32| heights[(cx as usize) * 2 + cz as usize];
    let flow = fluid::flow_vector(fluid, at, &lookup);
    for face in Face::ALL {
        let [dx, dy, dz] = face.normal();
        let neighbor = area.get(x + dx, y + dy, z + dz);
        let same_above = face == Face::Top
            && neighbor.is_some_and(|above| models.get(above).fluid == Some(fluid));
        // The surface shows under any block unless the fluid goes on up.
        let visible = if face == Face::Top {
            !same_above && neighbor.is_some()
        } else {
            face_visible(models, block, model, neighbor)
        };
        if !visible {
            continue;
        }
        let corners = face_corners(face).map(|[cx, cy, cz]| {
            let top = if cy > 0.0 { height(cx, cz) } else { 0.0 };
            [cx, top, cz]
        });
        let texture = model.faces[face.index()];
        let flowing = model.faces[Face::North.index()].uv;
        let uvs = match face {
            Face::Top => surface_uvs(corners, texture.uv, flowing, flow),
            Face::Bottom => {
                let [u0, v0, u1, v1] = texture.uv;
                [[u0, v1], [u1, v1], [u1, v0], [u0, v0]]
            }
            // Sides show the flowing texture at half scale, cut at the
            // surface.
            _ => {
                let [u0, v0, u1, v1] = texture.uv;
                let (half_u, half_v) = ((u1 - u0) / 2.0, (v1 - v0) / 2.0);
                [0, 1, 2, 3].map(|i| {
                    let u = if i == 1 || i == 2 { u0 + half_u } else { u0 };
                    let v = if i >= 2 {
                        v0 + (1.0 - corners[i][1]) * half_v
                    } else {
                        v0 + half_v
                    };
                    [u, v]
                })
            }
        };
        let tinted = tints
            .corners(texture.tint, x, z, corners)
            .map(|color| color.map(|c| c * face.shade()));
        let lit = smooth_face(models, area, light, cell, face);
        let colors = [0, 1, 2, 3].map(|i| tinted[i].map(|c| c * lit[i].0));
        let lights = lit.map(|(_, l)| l);
        let corners = translate(corners, origin);
        mesh.quad_uvs(corners, uvs, colors, lights);
        if face == Face::Top {
            let [a, b, c, d] = corners;
            let [ua, ub, uc, ud] = uvs;
            let [ca, cb, cc, cd] = colors;
            let [la, lb, lc, ld] = lights;
            mesh.quad_uvs(
                [b, a, d, c],
                [ub, ua, ud, uc],
                [cb, ca, cd, cc],
                [lb, la, ld, lc],
            );
        }
    }
}

/// Texture coordinates for a fluid surface's corners: the still texture
/// `still` filling the face if there's no `flow`, or else the middle half
/// of the flowing texture `flowing` turned to run along it (1.0.0 rotates
/// by the flow's angle around the texture's middle).
fn surface_uvs(
    corners: [[f32; 3]; 4],
    still: [f32; 4],
    flowing: [f32; 4],
    flow: glam::DVec3,
) -> [[f32; 2]; 4] {
    let (rect, reach, angle) = if flow.x == 0.0 && flow.z == 0.0 {
        (still, 0.5, 0.0)
    } else {
        let angle = flow.z.atan2(flow.x) as f32 - std::f32::consts::FRAC_PI_2;
        (flowing, 0.25, angle)
    };
    let [u0, v0, u1, v1] = rect;
    let (mid_u, mid_v) = ((u0 + u1) / 2.0, (v0 + v1) / 2.0);
    let (sin, cos) = angle.sin_cos();
    let (s, c) = (sin * reach, cos * reach);
    corners.map(|[cx, _, cz]| {
        let (sx, sz) = (cx * 2.0 - 1.0, cz * 2.0 - 1.0);
        [
            mid_u + (u1 - u0) * (c * sx + s * sz),
            mid_v + (v1 - v0) * (c * sz - s * sx),
        ]
    })
}

fn translate(corners: [[f32; 3]; 4], origin: [f32; 3]) -> [[f32; 3]; 4] {
    corners.map(|c| [c[0] + origin[0], c[1] + origin[1], c[2] + origin[2]])
}

/// Whether a face of `block` touching `neighbor` can be seen.
fn face_visible(
    models: &BlockModels,
    block: BlockState,
    model: &BlockModel,
    neighbor: Option<BlockState>,
) -> bool {
    // Unloaded chunks and the world floor hide faces; the face is rebuilt
    // when the neighbor chunk arrives.
    let Some(neighbor) = neighbor else {
        return false;
    };
    if neighbor.is_air() {
        return true;
    }
    let other = models.get(neighbor);
    if other.opaque_cube {
        return false;
    }
    // Flowing and still water hide each other's faces too.
    if model.fluid.is_some() && other.fluid == model.fluid {
        return false;
    }
    !(model.cull_same && neighbor.id() == block.id())
}

/// Where a face's corner `p` (block-relative) falls on its texture, 0–1
/// across and down, as Minecraft's block models map elements by default:
/// a box's faces show the part of the texture the face covers.
fn face_uv(face: Face, p: [f32; 3]) -> [f32; 2] {
    let [x, y, z] = p;
    match face {
        Face::Top => [x, z],
        Face::Bottom => [x, 1.0 - z],
        Face::North => [1.0 - x, 1.0 - y],
        Face::South => [x, 1.0 - y],
        Face::West => [z, 1.0 - y],
        Face::East => [1.0 - z, 1.0 - y],
    }
}

/// The two axes a face spans, as `smooth_face` orders them.
fn face_axes(face: Face) -> [usize; 2] {
    match face {
        Face::Top | Face::Bottom => [0, 2],
        Face::North | Face::South => [0, 1],
        Face::East | Face::West => [1, 2],
    }
}

/// Draws box `b` of the block at chunk-local `cell`: faces on the cell's
/// edge are hidden by solid neighbors like a cube's, the rest always show;
/// each face shows the part of its texture it covers and takes the smooth
/// light of its whole face, blended to where its corners are.
#[allow(clippy::too_many_arguments)]
fn mesh_box(
    mesh: &mut Mesh,
    models: &BlockModels,
    area: &ChunkArea,
    light: &ChunkLight,
    cell: [i32; 3],
    origin: [f32; 3],
    tints: &Tints,
    block: BlockState,
    model: &BlockModel,
    faces: &[FaceTexture; 6],
    b: Cuboid,
) {
    let [x, y, z] = cell;
    let full = b == Cuboid::FULL;
    for face in Face::ALL {
        let normal = face.normal();
        let axis = normal.iter().position(|&n| n != 0).unwrap_or(1);
        let outward = normal[axis] > 0;
        let on_edge = if outward {
            b.max[axis] >= 1.0
        } else {
            b.min[axis] <= 0.0
        };
        if on_edge {
            let neighbor = area.get(x + normal[0], y + normal[1], z + normal[2]);
            if !face_visible(models, block, model, neighbor) {
                continue;
            }
        }
        let corners = face_corners(face)
            .map(|c| [0, 1, 2].map(|i| if c[i] > 0.5 { b.max[i] } else { b.min[i] }));
        let texture = faces[face.index()];
        let [u0, v0, u1, v1] = texture.uv;
        let uvs = if full {
            texture.corner_uvs()
        } else {
            corners.map(|p| {
                let [u, v] = face_uv(face, p);
                [u0 + (u1 - u0) * u, v0 + (v1 - v0) * v]
            })
        };
        let tinted = tints
            .corners(texture.tint, x, z, corners)
            .map(|color| color.map(|c| c * face.shade()));
        let lit = smooth_face(models, area, light, cell, face);
        let full_corners = face_corners(face);
        let [a1, a2] = face_axes(face);
        // Each corner blends the four full-face corners by where it is.
        let at = |p: [f32; 3]| {
            let mut ao = 0.0;
            let mut l = [0.0; 2];
            for (corner, &(c_ao, c_light)) in full_corners.iter().zip(&lit) {
                let w = |axis: usize| {
                    if corner[axis] > 0.5 {
                        p[axis]
                    } else {
                        1.0 - p[axis]
                    }
                };
                let weight = w(a1).clamp(0.0, 1.0) * w(a2).clamp(0.0, 1.0);
                ao += c_ao * weight;
                l[0] += c_light[0] * weight;
                l[1] += c_light[1] * weight;
            }
            (ao, l)
        };
        let blended = corners.map(at);
        let colors = [0, 1, 2, 3].map(|i| tinted[i].map(|c| c * blended[i].0));
        let lights = blended.map(|(_, l)| l);
        mesh.quad_uvs(translate(corners, origin), uvs, colors, lights);
    }
}

/// Cactus (Minecraft's model): a cube whose sides are drawn a pixel in.
fn mesh_cactus(
    mesh: &mut Mesh,
    models: &BlockModels,
    area: &ChunkArea,
    light: &ChunkLight,
    cell: [i32; 3],
    origin: [f32; 3],
    tints: &Tints,
) {
    let [x, y, z] = cell;
    let Some(block) = area.get(x, y, z) else {
        return;
    };
    let model = models.get(block);
    const INSET: f32 = 1.0 / 16.0;
    for face in Face::ALL {
        let normal = face.normal();
        if face == Face::Top || face == Face::Bottom {
            let neighbor = area.get(x + normal[0], y + normal[1], z + normal[2]);
            if !face_visible(models, block, model, neighbor) {
                continue;
            }
        }
        let corners = face_corners(face).map(|c| {
            [0, 1, 2].map(|i| {
                if normal[i] != 0 && i != 1 {
                    // Pull the side in by a pixel.
                    if c[i] > 0.5 { 1.0 - INSET } else { INSET }
                } else {
                    c[i]
                }
            })
        });
        let texture = model.faces[face.index()];
        let tinted = tints
            .corners(texture.tint, x, z, corners)
            .map(|color| color.map(|c| c * face.shade()));
        let lit = smooth_face(models, area, light, cell, face);
        let colors = [0, 1, 2, 3].map(|i| tinted[i].map(|c| c * lit[i].0));
        let lights = lit.map(|(_, l)| l);
        mesh.quad_uvs(
            translate(corners, origin),
            texture.corner_uvs(),
            colors,
            lights,
        );
    }
}

/// How far flat blocks sit off the surface they lie on, in blocks.
const FLAT_OFFSET: f32 = 1.0 / 64.0;

/// A rail's quad: flat just above the ground, turned along its shape, or
/// sloping up a block for ascending shapes. The curved texture's turn runs
/// from south to east, as for shape `south_east`.
fn rail_quads(state: BlockState, uv: [f32; 4]) -> [([[f32; 3]; 4], [f32; 4]); 1] {
    let shape = if state.has(Prop::RailShape) {
        state.get(Prop::RailShape)
    } else {
        state.get(Prop::StraightRailShape)
    };
    let h = FLAT_OFFSET;
    // Corners counter-clockwise from above, the texture running north to
    // south (v) for a north_south rail.
    let mut corners = [[0.0, h, 1.0], [1.0, h, 1.0], [1.0, h, 0.0], [0.0, h, 0.0]];
    // Quarter turns, and the edge (north, south, east, west) raised a block.
    let (turn, raise): (usize, Option<Face>) = match shape {
        rail_shape::EAST_WEST => (1, None),
        2 => (1, Some(Face::East)),
        3 => (1, Some(Face::West)),
        4 => (0, Some(Face::North)),
        5 => (0, Some(Face::South)),
        6 => (0, None),
        7 => (1, None),
        8 => (2, None),
        9 => (3, None),
        _ => (0, None),
    };
    corners.rotate_left(turn);
    if let Some(edge) = raise {
        for c in &mut corners {
            let up = match edge {
                Face::North => c[2] < 0.5,
                Face::South => c[2] > 0.5,
                Face::East => c[0] > 0.5,
                _ => c[0] < 0.5,
            };
            if up {
                c[1] = 1.0 + h;
            }
        }
    }
    [(corners, uv)]
}

/// The flat quads of a ladder, vine, lily pad or redstone dust,
/// block-relative, each drawn from both sides with the block's texture.
fn flat_quads(state: BlockState) -> Vec<[[f32; 3]; 4]> {
    let ground = |h: f32| [[0.0, h, 1.0], [1.0, h, 1.0], [1.0, h, 0.0], [0.0, h, 0.0]];
    // A quad standing a little in from the side `face` of the cell.
    let against = |face: Face| -> [[f32; 3]; 4] {
        let d = FLAT_OFFSET;
        match face {
            Face::North => [[1.0, 0.0, d], [0.0, 0.0, d], [0.0, 1.0, d], [1.0, 1.0, d]],
            Face::South => [
                [0.0, 0.0, 1.0 - d],
                [1.0, 0.0, 1.0 - d],
                [1.0, 1.0, 1.0 - d],
                [0.0, 1.0, 1.0 - d],
            ],
            Face::West => [[d, 0.0, 0.0], [d, 0.0, 1.0], [d, 1.0, 1.0], [d, 1.0, 0.0]],
            Face::East => [
                [1.0 - d, 0.0, 1.0],
                [1.0 - d, 0.0, 0.0],
                [1.0 - d, 1.0, 0.0],
                [1.0 - d, 1.0, 1.0],
            ],
            Face::Top => ground(1.0 - d),
            Face::Bottom => ground(d),
        }
    };
    match state.def().shape {
        // On the wall behind it.
        Shape::Ladder => vec![against(state.face(Prop::Facing).opposite())],
        Shape::Vine => {
            let mut quads: Vec<_> = [Face::North, Face::East, Face::South, Face::West, Face::Top]
                .into_iter()
                .filter(|&face| state.bool(Prop::side(face)))
                .map(against)
                .collect();
            if quads.is_empty() {
                quads.push(against(Face::Top));
            }
            quads
        }
        _ => vec![ground(FLAT_OFFSET)],
    }
}

/// Redstone dust's color by its power, from dark red at 0 to bright red
/// at 15 (modern `RedStoneWireBlock.getColorForPower`, roughly).
fn wire_color(state: BlockState) -> [f32; 3] {
    let p = state.number(Prop::Power) as f32 / 15.0;
    [0.3 + 0.7 * p, if p > 0.0 { 0.2 * p * p } else { 0.0 }, 0.0]
}

/// Builds the mesh for the center chunk of `area`, lit by `light` (from
/// `light::light_chunk` on the same area).
pub fn mesh_chunk(models: &BlockModels, area: &ChunkArea, light: &ChunkLight) -> Mesh {
    let mut mesh = Mesh::default();
    let chunk = area.center();
    let (min_x, min_z) = chunk.pos.min_block();
    let tints = Tints::new(&models.colors, area.blend, |x, z| area.biome(x, z));
    for section in (0..SECTION_COUNT).filter(|&s| !chunk.section_is_empty(s)) {
        for y in section * SECTION_SIZE..(section + 1) * SECTION_SIZE {
            for z in 0..CHUNK_SIZE {
                for x in 0..CHUNK_SIZE {
                    let block = chunk.get(x, y, z);
                    if block.is_air() {
                        continue;
                    }
                    let origin = [
                        (min_x + x as i32) as f32,
                        y as f32,
                        (min_z + z as i32) as f32,
                    ];
                    let model = models.get_at(block, min_x + x as i32, y as i32, min_z + z as i32);
                    mesh.adding_translucent = model.translucent;
                    let side = model.faces[Face::North.index()];
                    let side_color = tints.flat(side.tint, x as i32, z as i32);
                    let side_colors =
                        |corners| tints.corners(side.tint, x as i32, z as i32, corners);
                    // Plants take the light of their own cell.
                    let own = light.get(x as i32, y as i32, z as i32);
                    let own_light = [own.0 as f32, own.1 as f32];
                    match model.shape {
                        Shape::None => {}
                        Shape::Cross => {
                            for plane in CROSS_PLANES {
                                mesh.double_sided_quad(
                                    translate(plane, origin),
                                    side.uv,
                                    side_colors(plane),
                                    own_light,
                                );
                            }
                        }
                        Shape::Crop => {
                            for plane in CROP_PLANES {
                                mesh.double_sided_quad(
                                    translate(plane, origin),
                                    side.uv,
                                    side_colors(plane),
                                    own_light,
                                );
                            }
                        }
                        Shape::Bed => {
                            for (face, corners, uv) in bed_quads(block, model) {
                                let [dx, dy, dz] = face.normal();
                                let neighbor =
                                    area.get(x as i32 + dx, y as i32 + dy, z as i32 + dz);
                                let inset = matches!(face, Face::Top | Face::Bottom);
                                if !inset && !face_visible(models, block, model, neighbor) {
                                    continue;
                                }
                                let color = side_color.map(|c| c * face.shade());
                                mesh.quad(
                                    translate(corners, origin),
                                    uv,
                                    [color; 4],
                                    [own_light; 4],
                                );
                            }
                        }
                        Shape::Door => {
                            for (face, corners, uv) in door_quads(block, model) {
                                let color = side_color.map(|c| c * face.shade());
                                mesh.quad(
                                    translate(corners, origin),
                                    uv,
                                    [color; 4],
                                    [own_light; 4],
                                );
                            }
                        }
                        Shape::Torch => {
                            for (corners, uv) in torch_quads(block, side.uv) {
                                mesh.quad(
                                    translate(corners, origin),
                                    uv,
                                    [side_color; 4],
                                    [own_light; 4],
                                );
                            }
                        }
                        Shape::Fire => {
                            let solid = |dx: i32, dy: i32, dz: i32| {
                                let next = area.get(x as i32 + dx, y as i32 + dy, z as i32 + dz);
                                next.is_some_and(|next| models.get(next).opaque_cube)
                            };
                            let floor = solid(0, -1, 0);
                            let walls = [
                                solid(-1, 0, 0),
                                solid(1, 0, 0),
                                solid(0, 0, -1),
                                solid(0, 0, 1),
                            ];
                            for plane in fire_planes(floor, walls) {
                                mesh.double_sided_quad(
                                    translate(plane, origin),
                                    side.uv,
                                    side_colors(plane),
                                    own_light,
                                );
                            }
                        }
                        Shape::Liquid => {
                            let cell = [x as i32, y as i32, z as i32];
                            mesh_liquid(&mut mesh, models, area, light, cell, origin, &tints);
                        }
                        Shape::Cube => {
                            let cell = [x as i32, y as i32, z as i32];
                            mesh_box(
                                &mut mesh,
                                models,
                                area,
                                light,
                                cell,
                                origin,
                                &tints,
                                block,
                                model,
                                &model.faces,
                                Cuboid::FULL,
                            );
                        }
                        Shape::Cactus => {
                            let cell = [x as i32, y as i32, z as i32];
                            mesh_cactus(&mut mesh, models, area, light, cell, origin, &tints);
                        }
                        Shape::Rail => {
                            for (corners, uv) in rail_quads(block, side.uv) {
                                mesh.double_sided_quad(
                                    translate(corners, origin),
                                    uv,
                                    side_colors(corners),
                                    own_light,
                                );
                            }
                        }
                        Shape::Ladder | Shape::Vine | Shape::LilyPad | Shape::RedstoneWire => {
                            for corners in flat_quads(block) {
                                let colors = match model.shape {
                                    Shape::RedstoneWire => [wire_color(block); 4],
                                    _ => side_colors(corners),
                                };
                                mesh.double_sided_quad(
                                    translate(corners, origin),
                                    side.uv,
                                    colors,
                                    own_light,
                                );
                            }
                        }
                        _ => {
                            let cell = [x as i32, y as i32, z as i32];
                            for (i, &b) in model.boxes.iter().flatten().enumerate() {
                                let faces = model.part_faces.get(i).unwrap_or(&model.faces);
                                mesh_box(
                                    &mut mesh, models, area, light, cell, origin, &tints, block,
                                    model, faces, b,
                                );
                            }
                        }
                    }
                }
            }
        }
    }
    mesh
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::{blocks, light::light_chunk};
    use glam::Vec3;

    const STONE: BlockState = BlockState::of(blocks::STONE);
    const GLASS: BlockState = BlockState::of(blocks::GLASS);
    const LEAVES: BlockState = BlockState::of(blocks::OAK_LEAVES);
    const FLOWER: BlockState = BlockState::of(blocks::DANDELION);
    const WHEAT: BlockState = BlockState::of(blocks::WHEAT);
    const TORCH: BlockState = BlockState::of(blocks::TORCH);
    const FENCE: BlockState = BlockState::of(blocks::OAK_FENCE);

    fn models() -> BlockModels {
        BlockModels::new(
            crate::world::block::registry(),
            |_| None,
            [0.0, 0.0, 1.0, 1.0],
            BiomeColors::uniform(),
        )
    }

    /// Meshes chunk (0, 0) after placing `blocks`; `neighbors` adds stone-free
    /// empty chunks around it (or none, to leave them unloaded).
    fn mesh_with(blocks: &[(i32, i32, i32, BlockState)], neighbors: bool) -> Mesh {
        let mut chunks: std::collections::HashMap<ChunkPos, Chunk> = Default::default();
        for dz in -1..=1 {
            for dx in -1..=1 {
                if neighbors || (dx, dz) == (0, 0) {
                    let pos = ChunkPos::new(dx, dz);
                    chunks.insert(pos, Chunk::new(pos));
                }
            }
        }
        for &(x, y, z, block) in blocks {
            let pos = ChunkPos::of_block(x, z);
            let chunk = chunks.get_mut(&pos).expect("block in a missing chunk");
            let size = CHUNK_SIZE as i32;
            chunk.set(
                x.rem_euclid(size) as usize,
                y as usize,
                z.rem_euclid(size) as usize,
                block,
            );
        }
        let mut chunks: std::collections::HashMap<_, _> =
            chunks.into_iter().map(|(p, c)| (p, Arc::new(c))).collect();
        let center = chunks.remove(&ChunkPos::new(0, 0)).unwrap();
        let area = ChunkArea::new(center, |pos| chunks.get(&pos).cloned());
        let models = models();
        mesh_chunk(&models, &area, &light_chunk(&models, &area))
    }

    /// Faces drawn, opaque or translucent.
    fn quads(mesh: &Mesh) -> usize {
        let indices = mesh.indices.len() + mesh.translucent.len();
        assert_eq!(indices % 6, 0);
        indices / 6
    }

    /// A bed model whose faces have the uvs [i, 0, i + 1, 1] for face index i.
    fn bed_model() -> BlockModel {
        let mut model = models().get(STONE).clone();
        for face in Face::ALL {
            let i = face.index() as f32;
            model.faces[face.index()].uv = [i, 0.0, i + 1.0, 1.0];
        }
        model
    }

    #[test]
    fn a_bed_half_has_a_turned_top_planks_two_sides_and_its_end() {
        let model = bed_model();
        let uv = |face: Face| {
            let i = face.index() as f32;
            [i, 0.0, i + 1.0, 1.0]
        };
        // Foot of a bed whose head is north.
        let bed = BlockState::of(blocks::RED_BED).with_face(Prop::Facing, Face::North);
        let foot = bed_quads(bed, &model);
        let faces: Vec<Face> = foot.iter().map(|(face, ..)| *face).collect();
        assert_eq!(
            faces,
            [Face::Top, Face::Bottom, Face::South, Face::East, Face::West]
        );
        let (_, top, top_uv) = foot[0];
        assert!(top.iter().all(|c| c[1] == BED_TOP));
        assert_eq!(top_uv, uv(Face::Top));
        let u = [top[1][0] - top[0][0], top[1][2] - top[0][2]];
        assert_eq!(u, [0.0, -1.0], "the top's texture runs north, to the head");
        assert_eq!(foot[2].2, uv(Face::South), "the foot's end");
        assert!(foot[1].1.iter().all(|c| c[1] == BED_BOTTOM));
        // The head shows its north end instead.
        let head = bed_quads(bed.with(Prop::BedPart, bed_part::HEAD), &model);
        assert!(
            head.iter()
                .any(|&(face, _, end)| face == Face::North && end == uv(Face::North))
        );
        assert!(head.iter().all(|&(face, ..)| face != Face::South));
        // Heading east, the north side's texture is flipped to keep the legs
        // at the foot; the south side's isn't.
        let east = bed_quads(bed.with_face(Prop::Facing, Face::East), &model);
        let side = |face: Face| east.iter().find(|q| q.0 == face).unwrap().2;
        let [u0, _, u1, _] = uv(Face::East);
        assert_eq!(side(Face::North), [u1, 0.0, u0, 1.0]);
        assert_eq!(side(Face::South), [u0, 0.0, u1, 1.0]);
    }

    #[test]
    fn door_faces_cut_their_texture_and_mirror_when_open() {
        let model = bed_model();
        let uv = |face: Face| {
            let i = face.index() as f32;
            [i, 0.0, i + 1.0, 1.0]
        };
        let mirrored = |[u0, v0, u1, v1]: [f32; 4]| [u1, v0, u0, v1];
        let find = |quads: &[Quad], face: Face| quads.iter().find(|q| q.0 == face).unwrap().2;
        // Placed looking north, closed along the south side: the wide faces
        // show the whole texture, the north one mirrored, and the thin ones
        // a strip.
        let door = BlockState::of(blocks::OAK_DOOR).with_face(Prop::Facing, Face::North);
        let closed = door_quads(door, &model);
        assert_eq!(find(&closed, Face::South), uv(Face::South));
        assert_eq!(find(&closed, Face::North), mirrored(uv(Face::North)));
        let [u0, _, u1, _] = find(&closed, Face::East);
        assert!(((u1 - u0) - 3.0 / 16.0).abs() < 1e-6, "{u0} {u1}");
        // Open, it lies along the west side; now the west face is mirrored.
        let open = door_quads(door.with_bool(Prop::Open, true), &model);
        assert!(
            open.iter()
                .all(|(_, corners, _)| corners.iter().all(|c| c[0] <= 3.0 / 16.0))
        );
        assert_eq!(find(&open, Face::East), uv(Face::East));
        assert_eq!(find(&open, Face::West), mirrored(uv(Face::West)));
    }

    #[test]
    fn a_slab_is_half_a_block_with_its_top_always_shown() {
        const SLAB: BlockState = BlockState::of(blocks::OAK_SLAB);
        let mesh = mesh_with(&[(5, 64, 5, SLAB), (5, 65, 5, STONE)], true);
        // Six slab faces (its top shows under the stone) and five of the
        // stone's (its bottom faces a slab, which doesn't hide it).
        assert_eq!(quads(&mesh), 12);
        let slab_top = mesh
            .vertices
            .iter()
            .filter(|v| v.position[1] == 64.5)
            .count();
        assert_eq!(
            slab_top,
            4 + 4 * 2,
            "the top face and the sides' upper corners"
        );
        // Side faces show the lower half of the texture: their top corners
        // sit at v 0.5 of the test's 0–1 texture.
        let half_way = mesh
            .vertices
            .iter()
            .filter(|v| v.position[1] == 64.5 && v.uv[1] == 0.5)
            .count();
        assert_eq!(half_way, 4 * 2);
    }

    #[test]
    fn lone_block_has_six_faces() {
        assert_eq!(quads(&mesh_with(&[(5, 64, 5, STONE)], true)), 6);
    }

    #[test]
    fn touching_blocks_hide_shared_faces() {
        let mesh = mesh_with(&[(5, 64, 5, STONE), (6, 64, 5, STONE)], true);
        assert_eq!(quads(&mesh), 10);
        let column = mesh_with(
            &[(5, 64, 5, STONE), (5, 65, 5, STONE), (5, 66, 5, STONE)],
            true,
        );
        assert_eq!(quads(&column), 14);
    }

    #[test]
    fn faces_against_neighbor_chunks_are_culled() {
        // x = 15 in chunk 0 touches x = 16 (local 0) in chunk 1.
        let mesh = mesh_with(&[(15, 64, 5, STONE), (16, 64, 5, STONE)], true);
        assert_eq!(quads(&mesh), 5);
        let mesh = mesh_with(
            &[(0, 64, 0, STONE), (-1, 64, 0, STONE), (0, 64, -1, STONE)],
            true,
        );
        assert_eq!(quads(&mesh), 4);
    }

    #[test]
    fn unloaded_neighbors_hide_border_faces() {
        let mesh = mesh_with(&[(15, 64, 5, STONE)], false);
        assert_eq!(quads(&mesh), 5);
    }

    #[test]
    fn bottom_of_world_is_hidden_and_top_is_open() {
        assert_eq!(quads(&mesh_with(&[(5, 0, 5, STONE)], true)), 5);
        let top = WORLD_HEIGHT as i32 - 1;
        assert_eq!(quads(&mesh_with(&[(5, top, 5, STONE)], true)), 6);
    }

    #[test]
    fn see_through_blocks_show_what_is_behind_them() {
        // Stone keeps its face toward the glass; the glass face touching the
        // opaque stone is hidden.
        let mesh = mesh_with(&[(5, 64, 5, STONE), (6, 64, 5, GLASS)], true);
        assert_eq!(quads(&mesh), 6 + 5);
    }

    #[test]
    fn glass_hides_faces_between_glass_but_leaves_do_not() {
        assert_eq!(
            quads(&mesh_with(&[(5, 64, 5, GLASS), (6, 64, 5, GLASS)], true)),
            10
        );
        assert_eq!(
            quads(&mesh_with(&[(5, 64, 5, LEAVES), (6, 64, 5, LEAVES)], true)),
            12
        );
    }

    #[test]
    fn plants_are_double_sided_planes_and_hide_nothing() {
        assert_eq!(quads(&mesh_with(&[(5, 64, 5, FLOWER)], true)), 4);
        assert_eq!(quads(&mesh_with(&[(5, 64, 5, WHEAT)], true)), 8);
        let mesh = mesh_with(&[(5, 64, 5, STONE), (5, 65, 5, FLOWER)], true);
        assert_eq!(quads(&mesh), 6 + 4);
    }

    #[test]
    fn a_fence_post_draws_inside_its_cell_without_hiding_neighbors() {
        let mesh = mesh_with(&[(5, 64, 5, STONE), (5, 65, 5, FENCE)], true);
        assert_eq!(quads(&mesh), 6 + 5);
    }

    #[test]
    fn torches_are_a_thin_stick_that_leans_off_walls() {
        const WEST_PLANE: f32 = 5.5 - 1.0 / 16.0;
        let floor = mesh_with(&[(5, 64, 5, BlockState::of(blocks::TORCH))], true);
        assert_eq!(quads(&floor), 5);
        let upright = floor
            .vertices
            .iter()
            .filter(|v| v.position[0] == WEST_PLANE)
            .count();
        // The west plane's four corners plus the cap's west edge.
        assert_eq!(upright, 6, "the west plane stands straight at 7/16");
        // Attached to a wall to the west: raised 0.2, shifted 0.1 west, and
        // the base leans another 0.4 west.
        let east = BlockState::of(blocks::WALL_TORCH).with_face(Prop::Facing, Face::East);
        let wall = mesh_with(&[(5, 64, 5, east)], true);
        // The first quad is the west plane.
        for v in &wall.vertices[..4] {
            let expected = if v.position[1] > 65.0 {
                WEST_PLANE - 0.1
            } else {
                WEST_PLANE - 0.5
            };
            assert!((v.position[0] - expected).abs() < 1e-5, "{v:?}");
        }
    }

    /// Vertices of the top face of the block at (x, y, z) in chunk (0, 0).
    fn top_vertices(mesh: &Mesh, x: f32, y: f32, z: f32) -> Vec<Vertex> {
        let top = y + 1.0;
        let on_top = |v: &&Vertex| {
            v.position[1] == top
                && (x..=x + 1.0).contains(&v.position[0])
                && (z..=z + 1.0).contains(&v.position[2])
        };
        // Top faces are the quads whose four corners all sit at `top`.
        mesh.vertices
            .chunks(4)
            .filter(|quad| quad.iter().filter(on_top).count() == 4)
            .flatten()
            .copied()
            .collect()
    }

    #[test]
    fn open_faces_are_fully_sky_lit_without_occlusion() {
        let mesh = mesh_with(&[(5, 64, 5, STONE)], true);
        let top = top_vertices(&mesh, 5.0, 64.0, 5.0);
        assert_eq!(top.len(), 4);
        for v in top {
            assert_eq!(v.light, [15.0, 0.0]);
            assert_eq!(v.color, [1.0; 3]);
        }
    }

    #[test]
    fn corners_against_walls_are_darkened_like_1_0_0() {
        // A floor block with walls on its north (z - 1) and west (x - 1) sides.
        let mesh = mesh_with(
            &[
                (5, 64, 5, STONE),
                (5, 65, 4, STONE),
                (4, 65, 5, STONE),
                (4, 65, 4, STONE),
            ],
            true,
        );
        let ao_at = |x: f32, z: f32| {
            top_vertices(&mesh, 5.0, 64.0, 5.0)
                .iter()
                .find(|v| v.position[0] == x && v.position[2] == z)
                .unwrap()
                .color[0]
        };
        assert!((ao_at(6.0, 6.0) - 1.0).abs() < 1e-6);
        // One wall: (1 + 0.2 + 1 + 1) / 4.
        assert!((ao_at(6.0, 5.0) - 0.8).abs() < 1e-6);
        assert!((ao_at(5.0, 6.0) - 0.8).abs() < 1e-6);
        // Both walls hide the corner: (1 + 0.2 + 0.2 + 0.2) / 4.
        assert!((ao_at(5.0, 5.0) - 0.4).abs() < 1e-6);
    }

    #[test]
    fn faces_near_a_torch_get_block_light() {
        let mesh = mesh_with(&[(5, 64, 5, STONE), (5, 65, 5, TORCH)], true);
        // The torch's placeholder cube sits on the stone, hiding its top; the
        // stone's east face sees the torch's light one block away.
        let east = mesh
            .vertices
            .iter()
            .find(|v| v.position == [6.0, 65.0, 5.0] && v.light[1] > 0.0);
        assert!(east.is_some());
    }

    #[test]
    fn vertices_are_in_world_space() {
        let mesh = mesh_with(&[(-3, 64, -14, STONE)], true);
        assert!(mesh.vertices.is_empty(), "block is in another chunk");
        let mesh = mesh_with(&[(3, 64, 14, STONE)], true);
        let min = mesh
            .vertices
            .iter()
            .fold(Vec3::splat(f32::MAX), |m, v| m.min(v.position.into()));
        assert_eq!(min, Vec3::new(3.0, 64.0, 14.0));
    }

    #[test]
    fn cube_faces_wind_counter_clockwise_from_outside() {
        let mesh = mesh_with(&[(5, 64, 5, STONE)], true);
        let center = Vec3::new(5.5, 64.5, 5.5);
        for tri in mesh.indices.chunks(3) {
            let [a, b, c] = [0, 1, 2].map(|i| Vec3::from(mesh.vertices[tri[i] as usize].position));
            let normal = (b - a).cross(c - a);
            assert!(
                normal.dot((a + b + c) / 3.0 - center) > 0.0,
                "triangle {tri:?} faces inward"
            );
        }
    }

    #[test]
    fn side_textures_are_upright() {
        // The texture's top edge (v0) must be on each side face's top edge.
        let mut uv_mesh = Mesh::default();
        for face in [Face::North, Face::South, Face::East, Face::West] {
            uv_mesh.quad(
                face_corners(face),
                [0.0, 0.0, 1.0, 1.0],
                [[1.0; 3]; 4],
                [[15.0, 0.0]; 4],
            );
        }
        for v in &uv_mesh.vertices {
            assert_eq!(v.uv[1] == 0.0, v.position[1] == 1.0, "{v:?}");
        }
    }

    #[test]
    fn pack_blockstates_give_weighted_looks_by_position() {
        use crate::pack::{Pack, Resources, model::BlockOverrides, tests::*};
        let dir = crate::world::save::TempDir::new("looks");
        let zip = dir.path().join("looks.zip");
        write_zip(
            &zip,
            &[
                ("pack.mcmeta", MCMETA),
                (
                    "assets/minecraft/blockstates/stone.json",
                    br#"{"variants": {"": [{"model": "block/stone"}, {"model": "block/stone_b", "weight": 3}]}}"#,
                ),
                (
                    "assets/minecraft/models/block/stone_b.json",
                    br#"{"parent": "block/cube_all", "textures": {"all": "block/stone_b"}}"#,
                ),
            ],
        );
        let base = dir.path().join("base");
        std::fs::create_dir_all(&base).unwrap();
        let res = Resources::new(
            Pack::base(&base),
            vec![Pack::open(&zip).unwrap()],
            Default::default(),
        );
        let overrides = BlockOverrides::load(&res, crate::world::block::registry());
        let uv = |name: &str| match name {
            "block/stone" => Some([1.0; 4]),
            "block/stone_b" => Some([2.0; 4]),
            _ => None,
        };
        let models = BlockModels::with_packs(
            crate::world::block::registry(),
            uv,
            [0.0; 4],
            BiomeColors::uniform(),
            &overrides,
        );
        let mut seen = [0; 3];
        for x in 0..40 {
            for z in 0..40 {
                let model = models.get_at(STONE, x, 64, z);
                seen[model.faces[0].uv[0] as usize] += 1;
                // The same spot always looks the same.
                assert_eq!(models.get_at(STONE, x, 64, z), model);
            }
        }
        // Vanilla's own look keeps blocks.ron's texture; about 3 in 4 take
        // the heavier one.
        assert_eq!(seen[0], 0);
        assert!(seen[1] > 200 && seen[2] > 1000, "{seen:?}");
        // Blocks no pack covers have one look everywhere.
        assert_eq!(
            models.get_at(BlockState::of(blocks::DIRT), 5, 64, 9),
            models.get(BlockState::of(blocks::DIRT))
        );
    }

    #[test]
    fn faces_get_their_texture_and_biome_tint() {
        let uv = |name: &str| {
            Some(if name == "block/grass_block_top" {
                [0.5, 0.0, 1.0, 0.5]
            } else {
                [0.0; 4]
            })
        };
        let models = BlockModels::new(
            crate::world::block::registry(),
            uv,
            [9.0; 4],
            BiomeColors::uniform(),
        );
        let model = models.get(BlockState::of(blocks::GRASS_BLOCK));
        assert_eq!(model.faces[Face::Top.index()].uv, [0.5, 0.0, 1.0, 0.5]);
        assert_eq!(model.faces[Face::Bottom.index()].uv, [0.0; 4]);
        assert_eq!(model.faces[Face::Top.index()].tint, FaceTint::Grass);
        assert_eq!(model.faces[Face::East.index()].tint, FaceTint::None);
        // Missing textures get the missing texture.
        let missing = BlockModels::new(
            crate::world::block::registry(),
            |_| None,
            [9.0; 4],
            BiomeColors::uniform(),
        );
        assert_eq!(missing.get(STONE).faces[0].uv, [9.0; 4]);
    }

    #[test]
    fn logs_lying_down_show_their_ends_along_their_axis() {
        let models = models();
        let log = |axis_value: u8| {
            models.get(BlockState::of(blocks::OAK_LOG).with(Prop::Axis, axis_value))
        };
        let upright = log(axis::Y).clone();
        let end = upright.faces[Face::Top.index()];
        let bark = upright.faces[Face::North.index()];
        let along_x = log(axis::X);
        assert_eq!(along_x.faces[Face::East.index()], end);
        assert_eq!(
            along_x.faces[Face::Top.index()].rotation,
            1,
            "bark turned to run along x"
        );
        assert_eq!(along_x.faces[Face::Top.index()].uv, bark.uv);
        let along_z = log(axis::Z);
        assert_eq!(along_z.faces[Face::South.index()], end);
        assert_eq!(along_z.faces[Face::Top.index()], bark);
    }

    #[test]
    fn grass_takes_the_color_of_its_column_biome() {
        let mut map = image::RgbaImage::new(256, 256);
        map.put_pixel(0, 255, image::Rgba([200, 0, 0, 255])); // desert
        map.put_pixel(50, 173, image::Rgba([0, 200, 0, 255])); // plains
        let colors = BiomeColors::from_colormaps(&map, &map);
        let models = BlockModels::new(crate::world::block::registry(), |_| None, [0.0; 4], colors);
        let mut chunk = Chunk::new(ChunkPos::new(0, 0));
        chunk.set(0, 64, 0, BlockState::of(blocks::GRASS_BLOCK));
        chunk.set(5, 64, 0, BlockState::of(blocks::GRASS_BLOCK));
        chunk.set_biome(5, 0, Biome::Desert);
        let mut area = ChunkArea::new(Arc::new(chunk), |_| None);
        // Unblended, so each block shows its own biome's color.
        area.blend = 0;
        let mesh = mesh_chunk(&models, &area, &light_chunk(&models, &area));
        let top_color = |x: f32| {
            mesh.vertices
                .iter()
                .find(|v| v.position == [x, 65.0, 0.0])
                .unwrap()
                .color
        };
        assert_eq!(top_color(0.0), [0.0, 200.0 / 255.0, 0.0]);
        assert_eq!(top_color(5.0), [200.0 / 255.0, 0.0, 0.0]);
    }

    #[test]
    fn grass_and_leaves_fade_across_a_biome_border() {
        let map =
            image::RgbaImage::from_fn(256, 256, |x, y| image::Rgba([x as u8, y as u8, 90, 255]));
        let colors = BiomeColors::from_colormaps(&map, &map);
        let models = BlockModels::new(
            crate::world::block::registry(),
            |_| None,
            [0.0; 4],
            colors.clone(),
        );
        let mut chunk = Chunk::new(ChunkPos::new(0, 0));
        for z in 0..16 {
            for x in 0..16 {
                chunk.set(x, 64, z, BlockState::of(blocks::GRASS_BLOCK));
                chunk.set(x, 70, z, LEAVES);
                chunk.set(x, 72, z, BlockState::of(blocks::SPRUCE_LEAVES));
                let biome = if x < 8 { Biome::Forest } else { Biome::Plains };
                chunk.set_biome(x, z, biome);
            }
        }
        let area = ChunkArea::new(Arc::new(chunk), |_| None);
        assert_eq!(area.blend, config::BIOME_BLEND);
        let mesh = mesh_chunk(&models, &area, &light_chunk(&models, &area));
        // Vertex colors along z = 8 on the grass and leaf tops.
        let row = |y: f32| -> Vec<[f32; 3]> {
            (0..=16)
                .map(|x| {
                    mesh.vertices
                        .iter()
                        .find(|v| v.position == [x as f32, y, 8.0])
                        .unwrap()
                        .color
                })
                .collect()
        };
        let span = (2 * config::BIOME_BLEND + 1) as f32;
        for (y, a, b) in [
            (
                65.0,
                colors.grass(Biome::Forest),
                colors.grass(Biome::Plains),
            ),
            (
                71.0,
                colors.foliage(Biome::Forest),
                colors.foliage(Biome::Plains),
            ),
        ] {
            let row = row(y);
            for pair in row.windows(2) {
                for c in 0..3 {
                    let most = (a[c] - b[c]).abs() / span + 1e-4;
                    assert!((pair[0][c] - pair[1][c]).abs() <= most, "y {y}: {pair:?}");
                }
            }
            // The full change happens, just spread out.
            let (first, last) = (row[0], row[16]);
            assert!((first[0] - a[0]).abs() < 1e-4 && (last[0] - b[0]).abs() < 1e-4);
        }
        // Spruce leaves keep their fixed color on both sides.
        let spruce = row(73.0);
        assert!(spruce.iter().all(|&c| c == spruce[0]), "{spruce:?}");
    }

    const WATER: BlockState = BlockState::of(blocks::WATER);

    #[test]
    fn water_goes_in_the_translucent_list_with_a_lowered_surface_seen_from_below() {
        let mesh = mesh_with(&[(4, 60, 4, WATER)], true);
        assert!(mesh.indices.is_empty(), "nothing opaque");
        // Six faces plus the surface seen from below.
        assert_eq!(mesh.translucent.len() / 6, 7);
        let top = mesh
            .vertices
            .iter()
            .map(|v| v.position[1])
            .fold(f32::MIN, f32::max);
        // 1.0.0's corner average of one source (weighted 11/9) and three
        // open cells.
        let corner = 1.0 - (11.0 / 9.0 + 3.0) / 14.0;
        assert!((top - (60.0 + corner)).abs() < 1e-5, "{top}");
    }

    #[test]
    fn sources_and_flowing_water_join_and_the_surface_slopes_downstream() {
        let flowing = fluid::at_level(Fluid::Water, 3);
        let mesh = mesh_with(&[(4, 60, 4, WATER), (5, 60, 4, flowing)], true);
        // Each: the surface both ways, the bottom and three sides.
        assert_eq!(mesh.translucent.len() / 6, 12);
        let height_at = |x: f32| {
            mesh.vertices
                .iter()
                .filter(|v| v.position[0] == x && v.position[1] > 60.0)
                .map(|v| v.position[1])
                .fold(f32::MIN, f32::max)
        };
        assert!(height_at(4.0) > height_at(6.0) + 0.1);
    }

    #[test]
    fn water_under_water_is_full_height_and_hides_shared_faces() {
        let mesh = mesh_with(&[(4, 60, 4, WATER), (4, 61, 4, WATER)], true);
        // Two blocks of 5 visible faces each, plus the surface from below.
        assert_eq!(mesh.translucent.len() / 6, 11);
        let lower_top = mesh
            .vertices
            .iter()
            .map(|v| v.position[1])
            .filter(|&y| y < 61.5)
            .fold(f32::MIN, f32::max);
        assert_eq!(lower_top, 61.0, "the lower block reaches the one above");
    }
}
