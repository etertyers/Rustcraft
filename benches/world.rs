//! The performance gate's benchmarks (CLAUDE.md §7.2): chunk generation,
//! lighting, meshing and fluid ticks, the work behind every frame of
//! exploring. Run with `cargo bench`; compare against the last milestone's
//! numbers in `docs/perf/`. Redstone ticks join these in M19.

use std::{hint::black_box, sync::Arc};

use criterion::{Criterion, criterion_group, criterion_main};
use glam::IVec3;
use rustcraft::world::{
    World,
    biome::BiomeColors,
    block::{self, BlockState, Fluid},
    blocks,
    chunk::{Chunk, ChunkPos},
    fluid::{self, Fluids},
    light::light_chunk,
    mesh::{BlockModels, ChunkArea, mesh_chunk},
    worldgen::terrain::Terrain,
};

/// The seed `--perf-test` flies over.
const SEED: u64 = rustcraft::bench::SEED;

fn models() -> BlockModels {
    BlockModels::new(
        block::registry(),
        |_| Some([0.0, 0.0, 1.0, 1.0]),
        [0.0; 4],
        BiomeColors::uniform(),
    )
}

/// A generated chunk and its eight neighbors.
fn area(terrain: &Terrain, center: ChunkPos) -> ChunkArea {
    let chunks: Vec<Arc<Chunk>> = (-1..=1)
        .flat_map(|dz| (-1..=1).map(move |dx| center.offset(dx, dz)))
        .map(|pos| Arc::new(terrain.generate(pos)))
        .collect();
    let center_chunk = chunks[4].clone();
    ChunkArea::new(center_chunk, |pos| {
        chunks.iter().find(|chunk| chunk.pos == pos).cloned()
    })
}

fn generation(c: &mut Criterion) {
    let terrain = Terrain::new(SEED);
    let mut x = 0;
    c.bench_function("generate a chunk", |b| {
        b.iter(|| {
            x += 1;
            black_box(terrain.generate(ChunkPos::new(x, 3)))
        })
    });
}

fn lighting_and_meshing(c: &mut Criterion) {
    let terrain = Terrain::new(SEED);
    let models = models();
    let area = area(&terrain, ChunkPos::new(2, -5));
    c.bench_function("light a chunk", |b| {
        b.iter(|| black_box(light_chunk(&models, &area)))
    });
    let light = light_chunk(&models, &area);
    c.bench_function("mesh a chunk", |b| {
        b.iter(|| black_box(mesh_chunk(&models, &area, &light)))
    });
}

/// Water poured on a flat floor spreading for 100 ticks.
fn fluids(c: &mut Criterion) {
    let mut base = World::default();
    for cz in -1..=1 {
        for cx in -1..=1 {
            let mut chunk = Chunk::new(ChunkPos::new(cx, cz));
            for z in 0..16 {
                for x in 0..16 {
                    chunk.set(x, 63, z, BlockState::of(blocks::STONE));
                }
            }
            base.insert(chunk);
        }
    }
    let registry = block::registry();
    c.bench_function("100 ticks of spreading water", |b| {
        b.iter(|| {
            let mut world = World::default();
            for cz in -1..=1 {
                for cx in -1..=1 {
                    let pos = ChunkPos::new(cx, cz);
                    world.insert_shared(base.get(pos).unwrap().clone());
                }
            }
            let source = IVec3::new(8, 64, 8);
            world.get_mut(ChunkPos::new(0, 0)).unwrap().set(
                8,
                64,
                8,
                fluid::at_level(Fluid::Water, 0),
            );
            let mut fluids = Fluids::default();
            fluids.changed(source);
            let mut seed = 7u32;
            for time in 0..100 {
                let mut random = |n: u32| {
                    seed = seed.wrapping_mul(1_103_515_245).wrapping_add(12345);
                    (seed >> 16) % n
                };
                let outcome = fluids.tick(&world, registry, time, &mut random);
                for (at, block) in outcome.edits {
                    let pos = ChunkPos::of_block(at.x, at.z);
                    if let Some(chunk) = world.get_mut(pos) {
                        let (x, z) = (at.x.rem_euclid(16), at.z.rem_euclid(16));
                        chunk.set(x as usize, at.y as usize, z as usize, block);
                    }
                }
            }
            black_box(world.len())
        })
    });
}

criterion_group! {
    name = benches;
    config = Criterion::default().sample_size(50);
    targets = generation, lighting_and_meshing, fluids
}
criterion_main!(benches);
