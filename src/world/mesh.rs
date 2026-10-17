//! Chunk meshing: turns a chunk's blocks into triangles, skipping hidden faces.

use std::sync::Arc;

use super::{
    Face,
    biome::{Biome, BiomeColors},
    block::{BlockRegistry, BlockState, DATA_VALUES, FaceTint, RenderLayer, Shape},
    chunk::{Chunk, ChunkPos, SECTION_COUNT, SECTION_SIZE},
    light::ChunkLight,
};
use crate::{
    config::{CHUNK_SIZE, WORLD_HEIGHT},
    render::vertex::Vertex,
};

/// How one face of a block is drawn.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FaceTexture {
    /// Atlas UV rect `[u0, v0, u1, v1]`, v0 at the top.
    pub uv: [f32; 4],
    pub tint: FaceTint,
}

/// Everything the mesher needs to know about one (ID, data) pair.
#[derive(Debug, Clone, PartialEq)]
pub struct BlockModel {
    pub shape: Shape,
    /// Hides any face touching it.
    pub opaque_cube: bool,
    /// Hides faces between two blocks with the same ID.
    pub cull_same: bool,
    /// Light lost passing through it, 0–15.
    pub opacity: u8,
    /// Light it gives off, 0–15.
    pub emission: u8,
    /// Drawn in the blended pass (water, ice, glass).
    pub translucent: bool,
    /// Indexed by `Face::index`.
    pub faces: [FaceTexture; 6],
}

/// Block models for every (ID, data) pair, resolved against the texture atlas.
pub struct BlockModels {
    models: Vec<BlockModel>,
    /// Used for IDs the registry doesn't define.
    unknown: BlockModel,
    colors: BiomeColors,
}

impl BlockModels {
    /// `uv` maps a texture name to its atlas rect, or `None` if it's missing;
    /// missing textures use `missing_uv`.
    pub fn new(
        registry: &BlockRegistry,
        uv: impl Fn(&str) -> Option<[f32; 4]>,
        missing_uv: [f32; 4],
        colors: BiomeColors,
    ) -> Self {
        let unknown = BlockModel {
            shape: Shape::Cube,
            opaque_cube: true,
            cull_same: false,
            opacity: 15,
            emission: 0,
            translucent: false,
            faces: [FaceTexture {
                uv: missing_uv,
                tint: FaceTint::None,
            }; 6],
        };
        let max_id = registry.iter().map(|def| def.id).max().unwrap_or(0) as usize;
        let mut models = Vec::with_capacity((max_id + 1) * DATA_VALUES);
        for id in 0..=max_id {
            for data in 0..DATA_VALUES as u8 {
                let Some(def) = registry.get(id as u16) else {
                    models.push(unknown.clone());
                    continue;
                };
                let textures = def.textures_for(data);
                let tint = def.tint_for(data);
                let faces = Face::ALL.map(|face| FaceTexture {
                    uv: textures
                        .and_then(|t| t.get(face))
                        .and_then(&uv)
                        .unwrap_or(missing_uv),
                    tint: tint.for_face(face),
                });
                models.push(BlockModel {
                    shape: def.shape,
                    opaque_cube: def.is_opaque_cube(),
                    cull_same: def.cull_same,
                    opacity: def.light_opacity(),
                    emission: def.light,
                    translucent: def.layer == RenderLayer::Translucent,
                    faces,
                });
            }
        }
        Self {
            models,
            unknown,
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
        let index = block.id() as usize * DATA_VALUES + block.data() as usize;
        self.models.get(index).unwrap_or(&self.unknown)
    }
}

/// A chunk plus its eight neighbors, so faces on the border can be culled.
pub struct ChunkArea {
    /// `chunks[dz + 1][dx + 1]` is the chunk at offset (dx, dz) from the center.
    chunks: [[Option<Arc<Chunk>>; 3]; 3],
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
        Self { chunks }
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

    /// Adds an evenly colored and lit quad visible from both sides.
    fn double_sided_quad(
        &mut self,
        corners: [[f32; 3]; 4],
        uv: [f32; 4],
        color: [f32; 3],
        light: [f32; 2],
    ) {
        let [a, b, c, d] = corners;
        self.quad(corners, uv, [color; 4], [light; 4]);
        self.quad([b, a, d, c], uv, [color; 4], [light; 4]);
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
fn face_corners(face: Face) -> [[f32; 3]; 4] {
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
/// stick, plus a small cap at 10/16. Wall torches (data 1–4, see
/// `Placement::Torch`) sit 0.2 higher, shifted 0.1 and leaning 0.4 at the
/// base toward their wall. Returns (corners, uv) per quad, block-relative.
fn torch_quads(data: u8, uv: [f32; 4]) -> [([[f32; 3]; 4], [f32; 4]); 5] {
    const HALF: f32 = 1.0 / 16.0;
    const CAP: f32 = 0.625;
    let (shift, lean) = match data {
        1 => ([-0.1, 0.2, 0.0], [-0.4, 0.0]),
        2 => ([0.1, 0.2, 0.0], [0.4, 0.0]),
        3 => ([0.0, 0.2, -0.1], [0.0, -0.4]),
        4 => ([0.0, 0.2, 0.1], [0.0, 0.4]),
        _ => ([0.0; 3], [0.0; 2]),
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

/// Height of a block's top: 1, or for a liquid with no liquid of its kind
/// above it, 1.0.0's 8/9 for a source block.
fn liquid_height(
    area: &ChunkArea,
    block: BlockState,
    model: &BlockModel,
    [x, y, z]: [usize; 3],
) -> f32 {
    if model.shape != Shape::Liquid {
        return 1.0;
    }
    let above = area.get(x as i32, y as i32 + 1, z as i32);
    if above.is_some_and(|above| above.id() == block.id()) {
        1.0
    } else {
        LIQUID_SURFACE
    }
}

/// A still liquid's surface, 1/9 below the top of its block (1.0.0's
/// `BlockFluid.getFluidHeightPercent` for a source).
const LIQUID_SURFACE: f32 = 8.0 / 9.0;

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
    if models.get(neighbor).opaque_cube {
        return false;
    }
    !(model.cull_same && neighbor.id() == block.id())
}

/// Builds the mesh for the center chunk of `area`, lit by `light` (from
/// `light::light_chunk` on the same area).
pub fn mesh_chunk(models: &BlockModels, area: &ChunkArea, light: &ChunkLight) -> Mesh {
    let mut mesh = Mesh::default();
    let chunk = area.center();
    let (min_x, min_z) = chunk.pos.min_block();
    for section in (0..SECTION_COUNT).filter(|&s| !chunk.section_is_empty(s)) {
        for y in section * SECTION_SIZE..(section + 1) * SECTION_SIZE {
            for z in 0..CHUNK_SIZE {
                for x in 0..CHUNK_SIZE {
                    let block = chunk.get(x, y, z);
                    if block.is_air() {
                        continue;
                    }
                    let model = models.get(block);
                    mesh.adding_translucent = model.translucent;
                    let origin = [
                        (min_x + x as i32) as f32,
                        y as f32,
                        (min_z + z as i32) as f32,
                    ];
                    let biome = chunk.biome(x, z);
                    let side = model.faces[Face::North.index()];
                    let side_color = models.tint_color(side.tint, biome);
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
                                    side_color,
                                    own_light,
                                );
                            }
                        }
                        Shape::Crop => {
                            for plane in CROP_PLANES {
                                mesh.double_sided_quad(
                                    translate(plane, origin),
                                    side.uv,
                                    side_color,
                                    own_light,
                                );
                            }
                        }
                        Shape::Torch => {
                            for (corners, uv) in torch_quads(block.data(), side.uv) {
                                mesh.quad(
                                    translate(corners, origin),
                                    uv,
                                    [side_color; 4],
                                    [own_light; 4],
                                );
                            }
                        }
                        // Every other shape is a full cube until its model is
                        // built; liquids lower their surface.
                        _ => {
                            let height = liquid_height(area, block, model, [x, y, z]);
                            for face in Face::ALL {
                                let [dx, dy, dz] = face.normal();
                                let neighbor =
                                    area.get(x as i32 + dx, y as i32 + dy, z as i32 + dz);
                                if !face_visible(models, block, model, neighbor) {
                                    continue;
                                }
                                let texture = model.faces[face.index()];
                                let color = models
                                    .tint_color(texture.tint, biome)
                                    .map(|c| c * face.shade());
                                let cell = [x as i32, y as i32, z as i32];
                                let lit = smooth_face(models, area, light, cell, face);
                                let corners =
                                    face_corners(face).map(|[cx, cy, cz]| [cx, cy.min(height), cz]);
                                let colors = lit.map(|(ao, _)| color.map(|c| c * ao));
                                let lights = lit.map(|(_, l)| l);
                                let corners = translate(corners, origin);
                                mesh.quad(corners, texture.uv, colors, lights);
                                if face == Face::Top && height < 1.0 {
                                    // A liquid's surface is seen from below too.
                                    let [a, b, c, d] = corners;
                                    let [ca, cb, cc, cd] = colors;
                                    let [la, lb, lc, ld] = lights;
                                    mesh.quad(
                                        [b, a, d, c],
                                        texture.uv,
                                        [cb, ca, cd, cc],
                                        [lb, la, ld, lc],
                                    );
                                }
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
    use crate::world::light::light_chunk;
    use glam::Vec3;

    const STONE: BlockState = BlockState::new(1, 0);
    const GLASS: BlockState = BlockState::new(20, 0);
    const LEAVES: BlockState = BlockState::new(18, 0);
    const FLOWER: BlockState = BlockState::new(37, 0);
    const WHEAT: BlockState = BlockState::new(59, 0);
    const TORCH: BlockState = BlockState::new(50, 0);
    const FENCE: BlockState = BlockState::new(85, 0);

    fn models() -> BlockModels {
        let ron = r#"[
            (id: 0, name: "air", hardness: 0.0, shape: None, solid: false),
            (id: 1, name: "stone", hardness: 1.5),
            (id: 18, name: "leaves", hardness: 0.2, layer: Cutout),
            (id: 20, name: "glass", hardness: 0.3, layer: Cutout, cull_same: true),
            (id: 37, name: "flower", hardness: 0.0, shape: Cross, layer: Cutout),
            (id: 50, name: "torch", hardness: 0.0, shape: Torch, layer: Cutout, light: 14),
            (id: 59, name: "wheat", hardness: 0.0, shape: Crop, layer: Cutout),
            (id: 85, name: "fence", hardness: 2.0, shape: Fence),
            (id: 9, name: "water", hardness: 100.0, shape: Liquid, layer: Translucent, solid: false, cull_same: true),
        ]"#;
        let registry = BlockRegistry::from_ron(ron).unwrap();
        BlockModels::new(
            &registry,
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

    fn quads(mesh: &Mesh) -> usize {
        assert_eq!(mesh.indices.len() % 6, 0);
        mesh.indices.len() / 6
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
        assert_eq!(quads(&mesh_with(&[(5, 127, 5, STONE)], true)), 6);
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
    fn placeholder_shapes_draw_as_cubes_without_hiding_neighbors() {
        let mesh = mesh_with(&[(5, 64, 5, STONE), (5, 65, 5, FENCE)], true);
        assert_eq!(quads(&mesh), 6 + 5);
    }

    #[test]
    fn torches_are_a_thin_stick_that_leans_off_walls() {
        const WEST_PLANE: f32 = 5.5 - 1.0 / 16.0;
        let floor = mesh_with(&[(5, 64, 5, BlockState::new(50, 5))], true);
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
        let wall = mesh_with(&[(5, 64, 5, BlockState::new(50, 1))], true);
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
    fn faces_get_their_texture_and_biome_tint() {
        let ron = r#"#![enable(implicit_some)]
            [(id: 2, name: "grass", hardness: 0.6, textures: (all: "dirt", top: "grass_top"), tint: GrassTop)]"#;
        let registry = BlockRegistry::from_ron(ron).unwrap();
        let uv = |name: &str| {
            Some(if name == "grass_top" {
                [0.5, 0.0, 1.0, 0.5]
            } else {
                [0.0; 4]
            })
        };
        let models = BlockModels::new(&registry, uv, [9.0; 4], BiomeColors::uniform());
        let model = models.get(BlockState::new(2, 0));
        assert_eq!(model.faces[Face::Top.index()].uv, [0.5, 0.0, 1.0, 0.5]);
        assert_eq!(model.faces[Face::Bottom.index()].uv, [0.0; 4]);
        assert_eq!(model.faces[Face::Top.index()].tint, FaceTint::Grass);
        assert_eq!(model.faces[Face::East.index()].tint, FaceTint::None);
        // Undefined IDs get the missing texture.
        assert_eq!(models.get(BlockState::new(300, 0)).faces[0].uv, [9.0; 4]);
    }

    #[test]
    fn grass_takes_the_color_of_its_column_biome() {
        let ron = r#"#![enable(implicit_some)]
            [(id: 2, name: "grass", hardness: 0.6, textures: (all: "x"), tint: GrassTop)]"#;
        let registry = BlockRegistry::from_ron(ron).unwrap();
        let mut map = image::RgbaImage::new(256, 256);
        map.put_pixel(0, 255, image::Rgba([200, 0, 0, 255])); // desert
        map.put_pixel(50, 173, image::Rgba([0, 200, 0, 255])); // plains
        let colors = BiomeColors::from_colormaps(&map, &map);
        let models = BlockModels::new(&registry, |_| None, [0.0; 4], colors);

        let mut chunk = Chunk::new(ChunkPos::new(0, 0));
        chunk.set(0, 64, 0, BlockState::new(2, 0));
        chunk.set(5, 64, 0, BlockState::new(2, 0));
        chunk.set_biome(5, 0, Biome::Desert);
        let area = ChunkArea::new(Arc::new(chunk), |_| None);
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

    const WATER: BlockState = BlockState::new(9, 0);

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
        assert_eq!(top, 60.0 + LIQUID_SURFACE);
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
