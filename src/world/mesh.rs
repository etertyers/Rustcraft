//! Chunk meshing: turns a chunk's blocks into triangles, skipping hidden faces.

use std::sync::Arc;

use super::{
    Face,
    biome::{Biome, BiomeColors},
    block::{BlockRegistry, BlockState, DATA_VALUES, FaceTint, Shape},
    chunk::{Chunk, ChunkPos, SECTION_COUNT, SECTION_SIZE},
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
    pub indices: Vec<u32>,
}

impl Mesh {
    pub fn is_empty(&self) -> bool {
        self.indices.is_empty()
    }

    /// Adds a quad from corners ordered bottom-left, bottom-right, top-right,
    /// top-left as seen from its front (counter-clockwise).
    fn quad(&mut self, corners: [[f32; 3]; 4], uv: [f32; 4], color: [f32; 3]) {
        let [u0, v0, u1, v1] = uv;
        let uvs = [[u0, v1], [u1, v1], [u1, v0], [u0, v0]];
        let base = self.vertices.len() as u32;
        for (position, uv) in corners.into_iter().zip(uvs) {
            self.vertices.push(Vertex {
                position,
                uv,
                color,
            });
        }
        self.indices
            .extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
    }

    /// Adds a quad visible from both sides.
    fn double_sided_quad(&mut self, corners: [[f32; 3]; 4], uv: [f32; 4], color: [f32; 3]) {
        let [a, b, c, d] = corners;
        self.quad(corners, uv, color);
        self.quad([b, a, d, c], uv, color);
    }
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

/// Builds the mesh for the center chunk of `area`, in world coordinates.
pub fn mesh_chunk(models: &BlockModels, area: &ChunkArea) -> Mesh {
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
                    let origin = [
                        (min_x + x as i32) as f32,
                        y as f32,
                        (min_z + z as i32) as f32,
                    ];
                    let biome = chunk.biome(x, z);
                    let side = model.faces[Face::North.index()];
                    let side_color = models.tint_color(side.tint, biome);
                    match model.shape {
                        Shape::None => {}
                        Shape::Cross => {
                            for plane in CROSS_PLANES {
                                mesh.double_sided_quad(
                                    translate(plane, origin),
                                    side.uv,
                                    side_color,
                                );
                            }
                        }
                        Shape::Crop => {
                            for plane in CROP_PLANES {
                                mesh.double_sided_quad(
                                    translate(plane, origin),
                                    side.uv,
                                    side_color,
                                );
                            }
                        }
                        // Every other shape is a full cube until its model is built.
                        _ => {
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
                                mesh.quad(translate(face_corners(face), origin), texture.uv, color);
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
    use glam::Vec3;

    const STONE: BlockState = BlockState::new(1, 0);
    const GLASS: BlockState = BlockState::new(20, 0);
    const LEAVES: BlockState = BlockState::new(18, 0);
    const FLOWER: BlockState = BlockState::new(37, 0);
    const WHEAT: BlockState = BlockState::new(59, 0);
    const TORCH: BlockState = BlockState::new(50, 0);

    fn models() -> BlockModels {
        let ron = r#"[
            (id: 0, name: "air", hardness: 0.0, shape: None, solid: false),
            (id: 1, name: "stone", hardness: 1.5),
            (id: 18, name: "leaves", hardness: 0.2, layer: Cutout),
            (id: 20, name: "glass", hardness: 0.3, layer: Cutout, cull_same: true),
            (id: 37, name: "flower", hardness: 0.0, shape: Cross, layer: Cutout),
            (id: 50, name: "torch", hardness: 0.0, shape: Torch, layer: Cutout),
            (id: 59, name: "wheat", hardness: 0.0, shape: Crop, layer: Cutout),
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
        mesh_chunk(&models(), &area)
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
        let mesh = mesh_with(&[(5, 64, 5, STONE), (5, 65, 5, TORCH)], true);
        assert_eq!(quads(&mesh), 6 + 5);
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
            uv_mesh.quad(face_corners(face), [0.0, 0.0, 1.0, 1.0], [1.0; 3]);
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
        let mesh = mesh_chunk(&models, &ChunkArea::new(Arc::new(chunk), |_| None));
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
}
