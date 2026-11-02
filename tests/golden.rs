//! Golden tests: a fixed set of chunks is generated, lit and meshed on one
//! thread, and a hash of each part (blocks, light, mesh) is compared with
//! `tests/golden/hashes.ron`. A mismatch names the chunk and the part.
//!
//! Only update the hashes when the milestone's entry says that output is
//! meant to change: `UPDATE_GOLDEN=1 cargo test --test golden`.

use std::{collections::BTreeMap, collections::HashMap, path::PathBuf, sync::Arc};

use rustcraft::world::{
    biome::BiomeColors,
    block::{self, BlockState},
    chunk::{Chunk, ChunkPos},
    light::light_chunk,
    mesh::{BlockModels, ChunkArea, mesh_chunk},
    worldgen::{end::End, nether::Nether, terrain::Terrain},
};
use serde::{Deserialize, Serialize};

/// FNV-1a, 64 bits: a hash that never changes between Rust versions.
struct Fnv(u64);

impl Fnv {
    fn new() -> Self {
        Fnv(0xcbf2_9ce4_8422_2325)
    }
    fn bytes(&mut self, bytes: &[u8]) {
        for &b in bytes {
            self.0 = (self.0 ^ u64::from(b)).wrapping_mul(0x0000_0100_0000_01b3);
        }
    }
    fn u64(&mut self, v: u64) {
        self.bytes(&v.to_le_bytes());
    }
    fn hex(&self) -> String {
        format!("{:016x}", self.0)
    }
}

#[derive(Serialize, Deserialize, PartialEq, Debug, Clone)]
struct Hashes {
    blocks: String,
    light: String,
    mesh: String,
}

/// A dimension's name, its chunk generator and the chunks to hash.
type Dimension<'a> = (&'a str, &'a dyn Fn(ChunkPos) -> Chunk, &'a [ChunkPos]);

const SEEDS: [u64; 3] = [12345, 20111118, 1];

fn models() -> BlockModels {
    BlockModels::new(
        block::registry(),
        |_| Some([0.0, 0.0, 1.0, 1.0]),
        [0.0; 4],
        BiomeColors::uniform(),
    )
}

/// Hashes of block states by name, so a reordered `blocks.ron` doesn't change them.
fn block_hash(chunk: &Chunk, names: &mut HashMap<BlockState, u64>) -> String {
    let mut h = Fnv::new();
    for y in rustcraft::config::MIN_Y..rustcraft::config::MAX_Y {
        for z in 0..16 {
            for x in 0..16 {
                let state = chunk.get(x, y, z);
                let id = *names.entry(state).or_insert_with(|| {
                    let mut n = Fnv::new();
                    n.bytes(state.to_string().as_bytes());
                    n.0
                });
                h.u64(id);
            }
        }
    }
    h.hex()
}

fn hash_chunk(
    generate: &dyn Fn(ChunkPos) -> Chunk,
    cache: &mut HashMap<ChunkPos, Arc<Chunk>>,
    names: &mut HashMap<BlockState, u64>,
    models: &BlockModels,
    pos: ChunkPos,
) -> Hashes {
    for dz in -1..=1 {
        for dx in -1..=1 {
            let p = pos.offset(dx, dz);
            cache.entry(p).or_insert_with(|| Arc::new(generate(p)));
        }
    }
    let area = ChunkArea::new(cache[&pos].clone(), |p| cache.get(&p).cloned());
    let light = light_chunk(models, &area);
    let mesh = mesh_chunk(models, &area, &light);

    let mut l = Fnv::new();
    for y in rustcraft::config::MIN_Y..rustcraft::config::MAX_Y {
        for z in 0..16 {
            for x in 0..16 {
                let (sky, block) = light.get(x, y, z);
                l.bytes(&[sky, block]);
            }
        }
    }
    let mut m = Fnv::new();
    m.bytes(bytemuck::cast_slice(&mesh.vertices));
    for list in [&mesh.indices, &mesh.translucent] {
        m.u64(list.len() as u64);
        m.bytes(bytemuck::cast_slice(list));
    }
    Hashes {
        blocks: block_hash(&cache[&pos], names),
        light: l.hex(),
        mesh: m.hex(),
    }
}

fn current() -> BTreeMap<String, Hashes> {
    let models = models();
    let mut out = BTreeMap::new();
    let mut names = HashMap::new();
    let overworld: Vec<ChunkPos> = (-1..=1)
        .flat_map(|z| (-1..=1).map(move |x| ChunkPos::new(x, z)))
        .chain([ChunkPos::new(100, -40), ChunkPos::new(-300, 250)])
        .collect();
    let square: Vec<ChunkPos> = (0..2)
        .flat_map(|z| (0..2).map(move |x| ChunkPos::new(x, z)))
        .collect();
    for seed in SEEDS {
        let terrain = Terrain::new(seed);
        let nether = Nether::new(seed);
        let end = End::new(seed);
        let dims: [Dimension; 3] = [
            ("overworld", &|p| terrain.generate(p), &overworld),
            ("nether", &|p| nether.generate(p), &square),
            ("end", &|p| end.generate(p), &square),
        ];
        for (name, generate, chunks) in dims {
            let mut cache = HashMap::new();
            for &pos in chunks {
                let hashes = hash_chunk(generate, &mut cache, &mut names, &models, pos);
                out.insert(format!("{seed}/{name}/{},{}", pos.x, pos.z), hashes);
            }
        }
    }
    out
}

fn golden_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/golden/hashes.ron")
}

#[test]
fn chunks_hash_the_same_as_the_golden_file() {
    let now = current();
    if std::env::var_os("UPDATE_GOLDEN").is_some() {
        let text = ron::ser::to_string_pretty(&now, ron::ser::PrettyConfig::default()).unwrap();
        std::fs::write(golden_path(), text + "\n").unwrap();
        return;
    }
    let text = std::fs::read_to_string(golden_path())
        .expect("tests/golden/hashes.ron is missing: run UPDATE_GOLDEN=1 cargo test --test golden");
    let golden: BTreeMap<String, Hashes> = ron::from_str(&text).unwrap();
    let mut problems = Vec::new();
    for (key, want) in &golden {
        match now.get(key) {
            None => problems.push(format!("{key}: no longer covered")),
            Some(got) => {
                for (part, a, b) in [
                    ("blocks", &want.blocks, &got.blocks),
                    ("light", &want.light, &got.light),
                    ("mesh", &want.mesh, &got.mesh),
                ] {
                    if a != b {
                        problems.push(format!("{key}: {part} changed ({a} -> {b})"));
                    }
                }
            }
        }
    }
    for key in now.keys().filter(|k| !golden.contains_key(*k)) {
        problems.push(format!("{key}: not in the golden file"));
    }
    assert!(
        problems.is_empty(),
        "golden hashes differ (seed/dimension/chunk):\n{}",
        problems.join("\n")
    );
}
