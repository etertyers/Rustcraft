//! M67's reference reader: counts exposed dirt in a world Mojang's own
//! server generated, the way `--grass-test` does for Rustcraft's, so the
//! two can be compared as statistics (the seeds don't give the same land).
//!
//!     cargo run --release --example vanilla_dirt -- assets/vanilla/server/world/region [first last]
//!
//! It reads the Anvil region files (zlib or uncompressed chunks), takes the
//! chunks from `first` to `last` (default −32 to 31, the 1,024 × 1,024
//! blocks the server was asked to generate), and prints the same rows.
//! Test-only: nothing in the game uses it, and the server's files are
//! never committed.

use std::{
    collections::{HashMap, HashSet},
    io::Read,
    path::Path,
};

use rustcraft::world::worldgen::grass_stats::{patches, pit_report, sees_through};

#[derive(Debug, Clone)]
enum Nbt {
    Byte(i8),
    Str(String),
    List(Vec<Nbt>),
    Compound(HashMap<String, Nbt>),
    Longs(Vec<i64>),
    Other,
}

struct Reader<'a>(&'a [u8]);

impl Reader<'_> {
    fn take(&mut self, n: usize) -> &[u8] {
        let (head, rest) = self.0.split_at(n);
        self.0 = rest;
        head
    }
    fn u8(&mut self) -> u8 {
        self.take(1)[0]
    }
    fn i16(&mut self) -> i16 {
        i16::from_be_bytes(self.take(2).try_into().unwrap())
    }
    fn i32(&mut self) -> i32 {
        i32::from_be_bytes(self.take(4).try_into().unwrap())
    }
    fn i64(&mut self) -> i64 {
        i64::from_be_bytes(self.take(8).try_into().unwrap())
    }
    fn string(&mut self) -> String {
        let n = self.i16() as usize;
        String::from_utf8_lossy(self.take(n)).into_owned()
    }
    fn payload(&mut self, tag: u8) -> Nbt {
        match tag {
            1 => Nbt::Byte(self.u8() as i8),
            2 => {
                self.take(2);
                Nbt::Other
            }
            3 => {
                self.take(4);
                Nbt::Other
            }
            4 | 6 => {
                self.take(8);
                Nbt::Other
            }
            5 => {
                self.take(4);
                Nbt::Other
            }
            7 => {
                let n = self.i32() as usize;
                self.take(n);
                Nbt::Other
            }
            8 => Nbt::Str(self.string()),
            9 => {
                let inner = self.u8();
                let n = self.i32().max(0) as usize;
                Nbt::List((0..n).map(|_| self.payload(inner)).collect())
            }
            10 => {
                let mut map = HashMap::new();
                loop {
                    let tag = self.u8();
                    if tag == 0 {
                        break;
                    }
                    let name = self.string();
                    map.insert(name, self.payload(tag));
                }
                Nbt::Compound(map)
            }
            11 => {
                let n = self.i32() as usize;
                self.take(n * 4);
                Nbt::Other
            }
            12 => {
                let n = self.i32() as usize;
                Nbt::Longs((0..n).map(|_| self.i64()).collect())
            }
            _ => panic!("unknown NBT tag {tag}"),
        }
    }
}

fn get<'a>(nbt: &'a Nbt, key: &str) -> Option<&'a Nbt> {
    match nbt {
        Nbt::Compound(map) => map.get(key),
        _ => None,
    }
}

/// What a palette entry means for the count.
#[derive(Clone, Copy, PartialEq)]
enum Kind {
    SeeThrough,
    Dirt,
    BiomeDirt,
    Other,
}

fn kind(name: &str) -> Kind {
    match name.strip_prefix("minecraft:").unwrap_or(name) {
        "dirt" => Kind::Dirt,
        "coarse_dirt" | "podzol" | "rooted_dirt" => Kind::BiomeDirt,
        n if sees_through(n) => Kind::SeeThrough,
        _ => Kind::Other,
    }
}

/// One section's block kinds, `[y][z][x]` flattened, from its palette and
/// packed indices.
fn decode(section: &Nbt) -> Option<Vec<Kind>> {
    let states = get(section, "block_states")?;
    let Some(Nbt::List(palette)) = get(states, "palette") else {
        return None;
    };
    let kinds: Vec<Kind> = palette
        .iter()
        .map(|p| match get(p, "Name") {
            Some(Nbt::Str(name)) => kind(name),
            _ => Kind::Other,
        })
        .collect();
    if kinds.len() == 1 {
        return Some(vec![kinds[0]; 4096]);
    }
    let Some(Nbt::Longs(data)) = get(states, "data") else {
        return None;
    };
    let bits = (usize::BITS - (kinds.len() - 1).leading_zeros()).max(4) as usize;
    let per_long = 64 / bits;
    let mask = (1u64 << bits) - 1;
    Some(
        (0..4096)
            .map(|i| {
                let word = data[i / per_long] as u64;
                kinds[((word >> ((i % per_long) * bits)) & mask) as usize]
            })
            .collect(),
    )
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let dir = args
        .get(1)
        .expect("usage: vanilla_dirt <region folder> [first last]");
    let first: i32 = args.get(2).and_then(|a| a.parse().ok()).unwrap_or(-32);
    let last: i32 = args.get(3).and_then(|a| a.parse().ok()).unwrap_or(31);
    let (mut dirt, mut biome, mut tops) = (
        HashSet::new(),
        HashSet::new(),
        HashMap::<(i32, i32), i32>::new(),
    );
    let mut chunks = 0u64;
    for rz in first.div_euclid(32)..=last.div_euclid(32) {
        for rx in first.div_euclid(32)..=last.div_euclid(32) {
            let path = Path::new(dir).join(format!("r.{rx}.{rz}.mca"));
            let Ok(bytes) = std::fs::read(&path) else {
                eprintln!("missing {}", path.display());
                continue;
            };
            for index in 0..1024usize {
                let header =
                    u32::from_be_bytes(bytes[index * 4..index * 4 + 4].try_into().unwrap());
                if header == 0 {
                    continue;
                }
                let (cx, cz) = (rx * 32 + (index % 32) as i32, rz * 32 + (index / 32) as i32);
                if !(first..=last).contains(&cx) || !(first..=last).contains(&cz) {
                    continue;
                }
                let at = (header >> 8) as usize * 4096;
                let len = u32::from_be_bytes(bytes[at..at + 4].try_into().unwrap()) as usize;
                let (compression, body) = (bytes[at + 4], &bytes[at + 5..at + 4 + len]);
                let raw = match compression {
                    2 => {
                        let mut out = Vec::new();
                        flate2::read::ZlibDecoder::new(body)
                            .read_to_end(&mut out)
                            .unwrap();
                        out
                    }
                    3 => body.to_vec(),
                    other => panic!("compression {other} (set region-file-compression=deflate)"),
                };
                let mut reader = Reader(&raw);
                assert_eq!(reader.u8(), 10);
                reader.string();
                let root = reader.payload(10);
                // Only finished chunks.
                if !matches!(get(&root, "Status"), Some(Nbt::Str(s)) if s == "minecraft:full") {
                    continue;
                }
                let Some(Nbt::List(sections)) = get(&root, "sections") else {
                    continue;
                };
                let mut by_y: Vec<(i8, Vec<Kind>)> = sections
                    .iter()
                    .filter_map(|s| {
                        let Some(Nbt::Byte(y)) = get(s, "Y") else {
                            return None;
                        };
                        Some((*y, decode(s)?))
                    })
                    .collect();
                by_y.sort_by_key(|(y, _)| std::cmp::Reverse(*y));
                chunks += 1;
                for z in 0..16usize {
                    for x in 0..16usize {
                        'column: for (sy, kinds) in &by_y {
                            for yy in (0..16usize).rev() {
                                match kinds[yy * 256 + z * 16 + x] {
                                    Kind::SeeThrough => continue,
                                    k => {
                                        let at = (cx * 16 + x as i32, cz * 16 + z as i32);
                                        tops.insert(at, i32::from(*sy) * 16 + yy as i32);
                                        match k {
                                            Kind::Dirt => {
                                                dirt.insert(at);
                                            }
                                            Kind::BiomeDirt => {
                                                biome.insert(at);
                                            }
                                            _ => {}
                                        }
                                        break 'column;
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    let area = chunks * 256;
    let per = |n: usize| n as f64 * 1_000_000.0 / area as f64;
    println!("vanilla: {chunks} chunks ({area} blocks²). Exposed dirt per 1,000 × 1,000 blocks:");
    for (name, set) in [
        ("dirt of the finished chunks", &dirt),
        ("biome ground (coarse dirt, podzol, rooted)", &biome),
    ] {
        let sizes = patches(set);
        println!(
            "  {name:<44} {:>8.1} blocks  {:>7.1} patches  (largest {})",
            per(set.len()),
            per(sizes.len()),
            sizes.iter().max().copied().unwrap_or(0)
        );
    }
    print!("{}", pit_report(&tops, area));
}
