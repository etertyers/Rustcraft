//! Chunk meshing: turns a chunk's blocks into triangles, skipping hidden faces.

use std::sync::Arc;

use super::{
    Face,
    biome::{Biome, BiomeColors},
    block::{
        BlockRegistry, BlockState, DATA_VALUES, FaceTint, Fluid, RenderLayer, Shape, bed_direction,
        door_box, door_side,
    },
    chunk::{Chunk, ChunkPos, SECTION_COUNT, SECTION_SIZE},
    fluid::{self, Cell},
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
    /// Water or lava, flowing or still.
    pub fluid: Option<Fluid>,
    /// Holds fluids back (`BlockDef::stops_fluid`).
    pub stops_fluid: bool,
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
            fluid: None,
            stops_fluid: true,
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
                let mut faces = Face::ALL.map(|face| FaceTexture {
                    uv: textures
                        .and_then(|t| t.get(face))
                        .and_then(&uv)
                        .unwrap_or(missing_uv),
                    tint: tint.for_face(face),
                });
                // Blocks placed facing the player turn their south face.
                if let Some(front) = def.front(data) {
                    faces.swap(Face::South.index(), front.index());
                }
                models.push(BlockModel {
                    shape: def.shape,
                    opaque_cube: def.is_opaque_cube(),
                    cull_same: def.cull_same,
                    opacity: def.light_opacity(),
                    emission: def.light,
                    translucent: def.layer == RenderLayer::Translucent,
                    fluid: def.fluid,
                    stops_fluid: def.stops_fluid(),
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
/// south and the head's on north); `data` is the direction plus 8 for the
/// head.
fn bed_quads(data: u8, model: &BlockModel) -> Vec<Quad> {
    let head = data & 8 != 0;
    let toward_head = bed_direction(data & 3);
    let along = [
        toward_head.x as f32,
        toward_head.y as f32,
        toward_head.z as f32,
    ];
    let texture = |face: Face| model.faces[face.index()].uv;
    // The top's u runs along +x; turning its corners points it at the head.
    let mut top = face_corners(Face::Top).map(|[x, _, z]| [x, BED_TOP, z]);
    top.rotate_left(3 - (data & 3) as usize);
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
fn door_quads(data: u8, model: &BlockModel) -> [Quad; 6] {
    let (min, max) = door_box(data);
    let side = door_side(data);
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

/// Height of a slab (the bottom half of its cell).
const SLAB_HEIGHT: f32 = 0.5;

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
    biome: Biome,
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
        let color = models
            .tint_color(texture.tint, biome)
            .map(|c| c * face.shade());
        let lit = smooth_face(models, area, light, cell, face);
        let colors = lit.map(|(ao, _)| color.map(|c| c * ao));
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
                        Shape::Bed => {
                            // Both halves of a direction share a model; the
                            // head's (data 8) has its textures.
                            let half = BlockState::new(block.id(), block.data() & 8);
                            for (face, corners, uv) in bed_quads(block.data(), models.get(half)) {
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
                            // Both halves share their textures with data 0
                            // or 8.
                            let half = BlockState::new(block.id(), block.data() & 8);
                            for (face, corners, uv) in door_quads(block.data(), models.get(half)) {
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
                            for (corners, uv) in torch_quads(block.data(), side.uv) {
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
                                    side_color,
                                    own_light,
                                );
                            }
                        }
                        Shape::Liquid => {
                            let cell = [x as i32, y as i32, z as i32];
                            mesh_liquid(&mut mesh, models, area, light, cell, origin, biome);
                        }
                        // Every other shape is a full cube until its model is
                        // built.
                        _ => {
                            let slab = model.shape == Shape::Slab;
                            let height = if slab { SLAB_HEIGHT } else { 1.0 };
                            for face in Face::ALL {
                                let [dx, dy, dz] = face.normal();
                                let neighbor =
                                    area.get(x as i32 + dx, y as i32 + dy, z as i32 + dz);
                                // A slab's top is below whatever sits on it.
                                let open_top = slab && face == Face::Top;
                                if !open_top && !face_visible(models, block, model, neighbor) {
                                    continue;
                                }
                                let mut texture = model.faces[face.index()];
                                if slab && !matches!(face, Face::Top | Face::Bottom) {
                                    // Sides show the texture's lower half.
                                    let [u0, v0, u1, v1] = texture.uv;
                                    texture.uv = [u0, v0 + (v1 - v0) * (1.0 - height), u1, v1];
                                }
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
            (id: 126, name: "wooden_slab", hardness: 2.0, shape: Slab),
            (id: 8, name: "flowing_water", hardness: 100.0, fluid: Some(Water), shape: Liquid, layer: Translucent, solid: false, cull_same: true),
            (id: 9, name: "water", hardness: 100.0, fluid: Some(Water), shape: Liquid, layer: Translucent, solid: false, cull_same: true),
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
        // Foot of a bed whose head is north (direction 2).
        let foot = bed_quads(2, &model);
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
        // The head (data 8 on) shows its north end instead.
        let head = bed_quads(2 | 8, &model);
        assert!(
            head.iter()
                .any(|&(face, _, end)| face == Face::North && end == uv(Face::North))
        );
        assert!(head.iter().all(|&(face, ..)| face != Face::South));
        // Heading east, the north side's texture is flipped to keep the legs
        // at the foot; the south side's isn't.
        let east = bed_quads(3, &model);
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
        // Closed along the south side (direction 3): the wide faces show the
        // whole texture, the north one mirrored, and the thin ones a strip.
        let closed = door_quads(3, &model);
        assert_eq!(find(&closed, Face::South), uv(Face::South));
        assert_eq!(find(&closed, Face::North), mirrored(uv(Face::North)));
        let [u0, _, u1, _] = find(&closed, Face::East);
        assert!(((u1 - u0) - 3.0 / 16.0).abs() < 1e-6, "{u0} {u1}");
        // Open, it lies along the west side; now the west face is mirrored.
        let open = door_quads(3 | 4, &model);
        assert!(
            open.iter()
                .all(|(_, corners, _)| corners.iter().all(|c| c[0] <= 3.0 / 16.0))
        );
        assert_eq!(find(&open, Face::East), uv(Face::East));
        assert_eq!(find(&open, Face::West), mirrored(uv(Face::West)));
    }

    #[test]
    fn a_slab_is_half_a_block_with_its_top_always_shown() {
        const SLAB: BlockState = BlockState::new(126, 0);
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
        // 1.0.0's corner average of one source (weighted 11/9) and three
        // open cells.
        let corner = 1.0 - (11.0 / 9.0 + 3.0) / 14.0;
        assert!((top - (60.0 + corner)).abs() < 1e-5, "{top}");
    }

    #[test]
    fn still_and_flowing_water_join_and_the_surface_slopes_downstream() {
        let flowing = BlockState::new(8, 3);
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
