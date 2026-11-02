//! `/locate structure <id>` and `/locate structure #<tag>` in every
//! dimension (M49): the nearest start of a structure, found by walking the
//! generator's own placement cells outward (no chunk is loaded or made), and
//! `/locate biome` in the Nether and the End.

use glam::{DVec3, IVec3};

use super::{Caller, Reply};
use crate::{
    game::Game,
    world::{
        biome::Biome,
        chunk::ChunkPos,
        dimension::Dimension,
        structure::{self, Kind},
        worldgen::{end::End, nether::Nether, terrain::Terrain},
    },
};

/// How far (in chunks, each way) the search goes.
pub const SEARCH_CHUNKS: i32 = 100;

/// Vanilla's structure ids, where each can generate, and the kind of ours
/// it is (`None`: vanilla has it, Rustcraft doesn't, so it is never found).
const IDS: &[(&str, Dimension, Option<Kind>)] = &[
    ("village", Dimension::Overworld, Some(Kind::Village)),
    ("village_plains", Dimension::Overworld, Some(Kind::Village)),
    ("village_desert", Dimension::Overworld, Some(Kind::Village)),
    ("village_savanna", Dimension::Overworld, Some(Kind::Village)),
    ("village_snowy", Dimension::Overworld, Some(Kind::Village)),
    ("village_taiga", Dimension::Overworld, Some(Kind::Village)),
    (
        "desert_pyramid",
        Dimension::Overworld,
        Some(Kind::DesertTemple),
    ),
    (
        "jungle_pyramid",
        Dimension::Overworld,
        Some(Kind::JungleTemple),
    ),
    ("shipwreck", Dimension::Overworld, Some(Kind::Shipwreck)),
    (
        "shipwreck_beached",
        Dimension::Overworld,
        Some(Kind::Shipwreck),
    ),
    (
        "ruined_portal",
        Dimension::Overworld,
        Some(Kind::RuinedPortal),
    ),
    (
        "ruined_portal_desert",
        Dimension::Overworld,
        Some(Kind::RuinedPortal),
    ),
    (
        "ruined_portal_jungle",
        Dimension::Overworld,
        Some(Kind::RuinedPortal),
    ),
    (
        "ruined_portal_swamp",
        Dimension::Overworld,
        Some(Kind::RuinedPortal),
    ),
    (
        "ruined_portal_mountain",
        Dimension::Overworld,
        Some(Kind::RuinedPortal),
    ),
    (
        "ruined_portal_ocean",
        Dimension::Overworld,
        Some(Kind::RuinedPortal),
    ),
    ("mineshaft", Dimension::Overworld, Some(Kind::Mineshaft)),
    (
        "mineshaft_mesa",
        Dimension::Overworld,
        Some(Kind::Mineshaft),
    ),
    ("stronghold", Dimension::Overworld, Some(Kind::Stronghold)),
    ("igloo", Dimension::Overworld, None),
    ("swamp_hut", Dimension::Overworld, None),
    ("pillager_outpost", Dimension::Overworld, None),
    ("mansion", Dimension::Overworld, None),
    ("monument", Dimension::Overworld, None),
    ("ocean_ruin_cold", Dimension::Overworld, None),
    ("ocean_ruin_warm", Dimension::Overworld, None),
    ("buried_treasure", Dimension::Overworld, None),
    ("ancient_city", Dimension::Overworld, None),
    ("trail_ruins", Dimension::Overworld, None),
    ("trial_chambers", Dimension::Overworld, None),
    ("fortress", Dimension::Nether, Some(Kind::Fortress)),
    ("bastion_remnant", Dimension::Nether, Some(Kind::Bastion)),
    (
        "ruined_portal_nether",
        Dimension::Nether,
        Some(Kind::NetherRuinedPortal),
    ),
    (
        "ruined_portal",
        Dimension::Nether,
        Some(Kind::NetherRuinedPortal),
    ),
    ("nether_fossil", Dimension::Nether, None),
    ("end_city", Dimension::End, Some(Kind::EndCity)),
];

/// The structure tags (`#village`) and the ids in each.
const TAGS: &[(&str, &[&str])] = &[
    (
        "village",
        &[
            "village_plains",
            "village_desert",
            "village_savanna",
            "village_snowy",
            "village_taiga",
        ],
    ),
    ("mineshaft", &["mineshaft", "mineshaft_mesa"]),
    ("shipwreck", &["shipwreck", "shipwreck_beached"]),
    (
        "ruined_portal",
        &[
            "ruined_portal",
            "ruined_portal_desert",
            "ruined_portal_jungle",
            "ruined_portal_swamp",
            "ruined_portal_mountain",
            "ruined_portal_ocean",
            "ruined_portal_nether",
        ],
    ),
    ("ocean_ruin", &["ocean_ruin_cold", "ocean_ruin_warm"]),
    ("on_treasure_maps", &["buried_treasure"]),
    ("eye_of_ender_located", &["stronghold"]),
    ("on_ocean_explorer_maps", &["monument"]),
    ("on_woodland_explorer_maps", &["mansion"]),
    ("on_trial_chambers_maps", &["trial_chambers"]),
];

/// Short Rustcraft-only names for vanilla ids; Tab completion offers only
/// the vanilla ids, and the answer names the vanilla id.
const ALIASES: &[(&str, &str)] = &[("bastion", "bastion_remnant")];

/// The vanilla id `name` stands for (an alias is mapped; a `#tag` or an
/// id is left as it is, without any `minecraft:`).
pub fn resolve(name: &str) -> &str {
    let name = name.strip_prefix("minecraft:").unwrap_or(name);
    ALIASES
        .iter()
        .find(|(alias, _)| *alias == name)
        .map_or(name, |(_, id)| id)
}

/// Every id and `#tag` Tab offers after `/locate structure`.
pub fn structure_names() -> Vec<String> {
    let mut names: Vec<String> = IDS.iter().map(|(id, ..)| (*id).to_owned()).collect();
    names.dedup();
    names.sort();
    names.dedup();
    names.extend(TAGS.iter().map(|(tag, _)| format!("#{tag}")));
    names
}

/// The kinds of ours that `name` (an id or a `#tag`) means in `dim`, or an
/// error for a name vanilla doesn't have at all.
pub fn kinds_for(name: &str, dim: Dimension) -> Result<Vec<Kind>, String> {
    let name = resolve(name);
    let ids: Vec<&str> = match name.strip_prefix('#') {
        Some(tag) => {
            let tag = tag.strip_prefix("minecraft:").unwrap_or(tag);
            TAGS.iter()
                .find(|(t, _)| *t == tag)
                .map(|(_, ids)| ids.to_vec())
                .ok_or_else(|| format!("Unknown structure tag: {name}"))?
        }
        None => {
            if !IDS.iter().any(|(id, ..)| *id == name) {
                return Err(format!("Unknown structure: {name}"));
            }
            vec![name]
        }
    };
    let mut kinds = Vec::new();
    for (id, here, kind) in IDS {
        if ids.contains(id)
            && *here == dim
            && let Some(kind) = kind
            && !kinds.contains(kind)
        {
            kinds.push(*kind);
        }
    }
    Ok(kinds)
}

/// The chunks, within `SEARCH_CHUNKS` of `center`, where `kind` starts in
/// `dim` by the generator's own rule.
fn starts(seed: u64, dim: Dimension, kind: Kind, center: ChunkPos) -> Vec<ChunkPos> {
    match dim {
        // Strongholds stand on rings round the world's center, all known
        // at once, so there is no search radius (modern's too).
        Dimension::Overworld if kind == Kind::Stronghold => structure::stronghold::positions(seed),
        Dimension::Overworld => {
            // `starts_near` knows the chunks within a kind's reach of one:
            // walk a lattice of probes that tiles the square.
            let step = 2 * kind.reach() + 1;
            let mut found = Vec::new();
            let n = SEARCH_CHUNKS / step + 1;
            for dz in -n..=n {
                for dx in -n..=n {
                    for start in kind.starts_near(seed, center.offset(dx * step, dz * step)) {
                        if (start.x - center.x).abs() <= SEARCH_CHUNKS
                            && (start.z - center.z).abs() <= SEARCH_CHUNKS
                            && !found.contains(&start)
                        {
                            found.push(start);
                        }
                    }
                }
            }
            found
        }
        Dimension::Nether => {
            structure::nether::starts_within(&Nether::new(seed), kind, center, SEARCH_CHUNKS)
        }
        Dimension::End => structure::end::starts_within(&End::new(seed), center, SEARCH_CHUNKS),
    }
}

/// The start chunk of the nearest structure of one of `kinds` that is
/// really made there (its biome and other checks passing), from `from`.
pub fn nearest(seed: u64, dim: Dimension, kinds: &[Kind], from: DVec3) -> Option<(Kind, ChunkPos)> {
    let center = ChunkPos::of_block(from.x.floor() as i32, from.z.floor() as i32);
    let terrain = (dim == Dimension::Overworld).then(|| Terrain::new(seed));
    let nether = (dim == Dimension::Nether).then(|| Nether::new(seed));
    let end = (dim == Dimension::End).then(|| End::new(seed));
    let mut candidates: Vec<(f64, Kind, ChunkPos)> = Vec::new();
    for &kind in kinds {
        for start in starts(seed, dim, kind, center) {
            let (x, z) = start.min_block();
            let d = (f64::from(x) + 8.0 - from.x).hypot(f64::from(z) + 8.0 - from.z);
            candidates.push((d, kind, start));
        }
    }
    candidates.sort_by(|a, b| a.0.total_cmp(&b.0));
    candidates.into_iter().find_map(|(_, kind, start)| {
        let made = match dim {
            Dimension::Overworld => {
                structure::structure_at(terrain.as_ref()?, kind, start).is_some()
            }
            Dimension::Nether => {
                structure::nether::structure_at(nether.as_ref()?, kind, start).is_some()
            }
            Dimension::End => structure::end::structure_at(end.as_ref()?, start).is_some(),
        };
        made.then_some((kind, start))
    })
}

/// `/locate structure <name>` for `caller`, in the dimension they're in.
pub(super) fn locate_structure(
    game: &Game,
    caller: &Caller,
    name: &str,
) -> Result<Vec<Reply>, String> {
    let kinds = kinds_for(name, caller.dim)?;
    let name = resolve(name);
    let seed = game
        .chunks
        .seed()
        .ok_or("This world has no terrain to search")?;
    let found = nearest(seed, caller.dim, &kinds, caller.pos);
    let Some((_, start)) = found else {
        return Err(format!(
            "Could not find a structure of type \"{name}\" nearby"
        ));
    };
    let (x, z) = start.min_block();
    let (x, z) = (x + 8, z + 8);
    let away = (f64::from(x) - caller.pos.x).hypot(f64::from(z) - caller.pos.z);
    Ok(vec![Reply::Rich {
        to: caller.index,
        text: format!(
            "The nearest {name} is at §a[{x}, ~, {z}]§r ({} blocks away)",
            away.round() as i64
        ),
        click: caller
            .cheats
            .then(|| ("suggest_command".to_owned(), format!("/tp @s {x} ~ {z}"))),
        hover: Some("Click to teleport".to_owned()),
    }])
}

/// The nearest column in `biome` in the Nether or the End, every 32 blocks
/// outward (the Overworld is searched with its terrain, in `commands.rs`).
pub(super) fn nearest_biome_in(
    seed: u64,
    dim: Dimension,
    biome: Biome,
    from: DVec3,
) -> Option<IVec3> {
    let nether = (dim == Dimension::Nether).then(|| Nether::new(seed));
    let at = |x: i32, z: i32| -> Biome {
        match (&nether, dim) {
            (Some(nether), _) => nether.biome(x, z),
            (None, Dimension::End) => Biome::TheEnd,
            _ => Biome::Plains,
        }
    };
    let (x0, z0) = (from.x.floor() as i32, from.z.floor() as i32);
    const STEP: i32 = 32;
    for ring in 0..=6400 / STEP {
        let mut best: Option<(i64, IVec3)> = None;
        for dz in -ring..=ring {
            for dx in -ring..=ring {
                if dx.abs().max(dz.abs()) != ring {
                    continue;
                }
                let (x, z) = (x0 + dx * STEP, z0 + dz * STEP);
                if at(x, z) == biome {
                    let d = i64::from(dx * dx + dz * dz);
                    if best.is_none_or(|(b, _)| d < b) {
                        best = Some((d, IVec3::new(x, 64, z)));
                    }
                }
            }
        }
        if let Some((_, found)) = best {
            return Some(found);
        }
    }
    None
}
