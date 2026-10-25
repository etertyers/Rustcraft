//! The performance gate's benchmarks (CLAUDE.md §7.2): chunk generation,
//! lighting, meshing and fluid ticks, the work behind every frame of
//! exploring. Run with `cargo bench`; compare against the last milestone's
//! numbers in `docs/perf/`. Redstone ticks joined these in M19.

use std::{hint::black_box, sync::Arc};

use criterion::{Criterion, criterion_group, criterion_main};
use glam::IVec3;
use rustcraft::world::{
    World,
    biome::BiomeColors,
    block::{self, BlockState, Fluid},
    blocks,
    chunk::{Chunk, ChunkPos},
    fast_hash::FastSet,
    fluid::{self, Fluids},
    light::light_chunk,
    mesh::{BlockModels, ChunkArea, mesh_chunk},
    props::Prop,
    redstone::Redstone,
    worldgen::{end::End, nether::Nether, terrain::Terrain},
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

/// Added in M24.
fn nether_generation(c: &mut Criterion) {
    let nether = Nether::new(SEED);
    let mut x = 0;
    c.bench_function("generate a nether chunk", |b| {
        b.iter(|| {
            x += 1;
            black_box(nether.generate(ChunkPos::new(x, 3)))
        })
    });
}

/// Added in M27: chunks on the main island, where the pillars are.
fn end_generation(c: &mut Criterion) {
    let end = End::new(SEED);
    let mut x = -4;
    c.bench_function("generate an end chunk", |b| {
        b.iter(|| {
            x = if x >= 4 { -4 } else { x + 1 };
            black_box(end.generate(ChunkPos::new(x, 2)))
        })
    });
}

/// Added in M28: chunks over an End city, tower and bridges.
fn end_city_generation(c: &mut Criterion) {
    let end = End::new(SEED);
    let city = rustcraft::world::structure::end::find(&end, ChunkPos::new(1, -64), 10).unwrap();
    let center = city.bounds.center();
    let at = ChunkPos::of_block(center.x, center.z);
    let mut dx = -1;
    c.bench_function("generate an end city chunk", |b| {
        b.iter(|| {
            dx = if dx >= 1 { -1 } else { dx + 1 };
            black_box(end.generate(at.offset(dx, 0)))
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
                        chunk.set(x as usize, at.y, z as usize, block);
                    }
                }
            }
            black_box(world.len())
        })
    });
}

/// Eight 15-block dust lines fed by a lever flipped every ten ticks, and a
/// ring of two repeaters pulsing as a clock, for 100 ticks.
fn redstone(c: &mut Criterion) {
    let mut base = World::default();
    let mut chunk = Chunk::new(ChunkPos::new(0, 0));
    for z in 0..16 {
        for x in 0..16 {
            chunk.set(x, 63, z, BlockState::of(blocks::STONE));
        }
    }
    let dust = BlockState::of(blocks::REDSTONE_WIRE)
        .with(Prop::WireEast, rustcraft::world::props::wire_side::SIDE)
        .with(Prop::WireWest, rustcraft::world::props::wire_side::SIDE);
    for line in 0..8 {
        for x in 1..16 {
            chunk.set(x, 64, line * 2, dust);
        }
    }
    base.insert(chunk);
    let source = BlockState::of(blocks::REDSTONE_BLOCK);
    c.bench_function("100 ticks of redstone lines", |b| {
        b.iter(|| {
            let mut world = World::default();
            world.insert_shared(base.get(ChunkPos::new(0, 0)).unwrap().clone());
            let mut redstone = Redstone::default();
            let pressed = FastSet::default();
            for time in 0..100u64 {
                if time % 10 == 0 {
                    let on = (time / 10) % 2 == 0;
                    for line in 0..8 {
                        let at = IVec3::new(0, 64, line * 2);
                        let state = if on { source } else { BlockState::AIR };
                        world.get_mut(ChunkPos::new(0, 0)).unwrap().set(
                            0,
                            64,
                            (line * 2) as usize,
                            state,
                        );
                        redstone.changed(at);
                    }
                }
                let outcome = redstone.tick(&world, time, &pressed);
                for (at, block) in outcome.edits {
                    world.get_mut(ChunkPos::of_block(at.x, at.z)).unwrap().set(
                        at.x.rem_euclid(16) as usize,
                        at.y,
                        at.z.rem_euclid(16) as usize,
                        block,
                    );
                }
            }
            black_box(world.len())
        })
    });
}

criterion_group! {
    name = benches;
    config = Criterion::default().sample_size(50);
    targets = generation, nether_generation, end_generation, end_city_generation, lighting_and_meshing, fluids, redstone
}
criterion_main!(benches);
