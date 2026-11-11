//! Chunk generation offloaded to workers (M72). A request says what to
//! generate (kind, seed, position); the answer is the chunk in its save
//! encoding. In the browser the requests go to generator Web Workers (each a
//! wasm instance of this crate), natively the same functions run in the
//! tests. The result must be bit-identical to generating on the spot, so the
//! golden hashes don't change.

use serde::{Deserialize, Serialize};

use super::{
    chunk::{Chunk, ChunkPos},
    save::{self, SaveError},
    worldgen::{end::End, flat, nether::Nether, terrain::Terrain},
};

/// Which generator a world's chunks come from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum GenKind {
    Terrain,
    Nether,
    End,
    Flat,
}

/// One chunk to generate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct GenRequest {
    pub kind: GenKind,
    pub seed: u64,
    pub pos: ChunkPos,
}

impl GenRequest {
    pub fn to_bytes(&self) -> Vec<u8> {
        bincode::serde::encode_to_vec(self, bincode::config::standard())
            .expect("a request always encodes")
    }

    pub fn from_bytes(bytes: &[u8]) -> Option<Self> {
        bincode::serde::decode_from_slice(bytes, bincode::config::standard())
            .ok()
            .map(|(req, _)| req)
    }
}

/// A generator worker's state: the generator for the world it was last asked
/// about (building one takes noise setup, so it is kept).
#[derive(Default)]
pub struct GeneratorWorker {
    terrain: Option<(u64, Terrain)>,
    nether: Option<(u64, Nether)>,
    end: Option<(u64, End)>,
}

impl GeneratorWorker {
    pub fn generate(&mut self, req: &GenRequest) -> Chunk {
        let (seed, pos) = (req.seed, req.pos);
        match req.kind {
            GenKind::Terrain => {
                if self.terrain.as_ref().is_none_or(|(s, _)| *s != seed) {
                    self.terrain = Some((seed, Terrain::new(seed)));
                }
                self.terrain
                    .as_ref()
                    .map(|(_, t)| t.generate(pos))
                    .unwrap_or_else(|| Chunk::new(pos))
            }
            GenKind::Nether => {
                if self.nether.as_ref().is_none_or(|(s, _)| *s != seed) {
                    self.nether = Some((seed, Nether::new(seed)));
                }
                self.nether
                    .as_ref()
                    .map(|(_, n)| n.generate(pos))
                    .unwrap_or_else(|| Chunk::new(pos))
            }
            GenKind::End => {
                if self.end.as_ref().is_none_or(|(s, _)| *s != seed) {
                    self.end = Some((seed, End::new(seed)));
                }
                self.end
                    .as_ref()
                    .map(|(_, e)| e.generate(pos))
                    .unwrap_or_else(|| Chunk::new(pos))
            }
            GenKind::Flat => flat::flat_chunk(pos),
        }
    }

    /// The whole worker job: request bytes in, the chunk's bytes out.
    pub fn handle(&mut self, request: &[u8]) -> Option<Vec<u8>> {
        let req = GenRequest::from_bytes(request)?;
        Some(save::encode_chunk(&self.generate(&req)))
    }
}

/// Reads a worker's answer.
pub fn decode(bytes: &[u8]) -> Result<Chunk, SaveError> {
    save::decode_chunk(bytes)
}

/// Something that generates chunks elsewhere (the browser's generator
/// workers): `submit` returns at once, `poll` collects what has finished.
pub trait GenPool: Send {
    fn submit(&mut self, req: GenRequest);
    /// Finished chunks as (position, encoded bytes).
    fn poll(&mut self, out: &mut Vec<(ChunkPos, Vec<u8>)>);
    /// How many requests it takes at once.
    fn capacity(&self) -> usize;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{MAX_Y, MIN_Y};

    fn same(a: &Chunk, b: &Chunk) -> bool {
        for y in MIN_Y..MAX_Y {
            for z in 0..16 {
                for x in 0..16 {
                    if a.get(x, y, z) != b.get(x, y, z) {
                        return false;
                    }
                }
            }
        }
        save::encode_chunk(a) == save::encode_chunk(b)
    }

    /// The golden seeds (tests/golden.rs) in all three dimensions: what a
    /// worker answers is what generating on the spot gives.
    #[test]
    fn a_worker_generates_the_same_chunks_as_native_in_every_dimension() {
        let mut worker = GeneratorWorker::default();
        let spots = [
            ChunkPos::new(0, 0),
            ChunkPos::new(-3, 5),
            ChunkPos::new(17, -9),
        ];
        for seed in [12345_u64, 20111118, 1] {
            let (terrain, nether, end) = (Terrain::new(seed), Nether::new(seed), End::new(seed));
            for pos in spots {
                for (kind, direct) in [
                    (GenKind::Terrain, terrain.generate(pos)),
                    (GenKind::Nether, nether.generate(pos)),
                    (GenKind::End, end.generate(pos)),
                ] {
                    let request = GenRequest { kind, seed, pos }.to_bytes();
                    let answer = worker.handle(&request).expect("an answer");
                    let chunk = decode(&answer).expect("a chunk");
                    assert!(same(&direct, &chunk), "{kind:?} seed {seed} at {pos:?}");
                }
            }
        }
        let flat = flat::flat_chunk(ChunkPos::new(2, 2));
        let req = GenRequest {
            kind: GenKind::Flat,
            seed: 0,
            pos: ChunkPos::new(2, 2),
        };
        assert!(same(
            &flat,
            &decode(&worker.handle(&req.to_bytes()).unwrap()).unwrap()
        ));
    }

    #[test]
    fn requests_round_trip_and_junk_is_refused() {
        let req = GenRequest {
            kind: GenKind::Nether,
            seed: u64::MAX,
            pos: ChunkPos::new(-1, 7),
        };
        assert_eq!(GenRequest::from_bytes(&req.to_bytes()), Some(req));
        assert_eq!(GeneratorWorker::default().handle(&[]), None);
    }
}
