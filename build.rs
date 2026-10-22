//! Generates the block and item ID constants (`world::blocks::STONE`,
//! `inventory::items::DIAMOND_PICKAXE`, ...) from `assets/data/blocks.ron`
//! and `assets/data/items.ron`, so code names blocks and items the way
//! modern Minecraft does and a misspelled name doesn't compile.
//!
//! Runtime IDs are positions in those files: blocks in file order, then
//! items for every block that has one (in block order) followed by the
//! entries of items.ron. Saves store names, so the order can change freely.

use std::{env, fmt::Write as _, fs, path::Path};

use serde::Deserialize;

#[derive(Deserialize)]
struct BlockEntry {
    name: String,
    /// Whether the block has an item of its own (most do).
    #[serde(default = "yes")]
    item: bool,
}

#[derive(Deserialize)]
struct ItemEntry {
    name: String,
}

fn yes() -> bool {
    true
}

fn read<T: for<'de> Deserialize<'de>>(path: &str) -> Vec<T> {
    println!("cargo::rerun-if-changed={path}");
    let text = fs::read_to_string(path).unwrap_or_else(|err| panic!("reading {path}: {err}"));
    ron::from_str(&text).unwrap_or_else(|err| panic!("parsing {path}: {err}"))
}

/// `read`, then the placeholders generated from vanilla's data
/// (`placeholder_*.ron`, see `src/placeholders.rs`) after them.
fn read_with_placeholders<T: for<'de> Deserialize<'de>>(path: &str, placeholders: &str) -> Vec<T> {
    let mut all = read(path);
    all.extend(read::<T>(placeholders));
    all
}

/// `pub const NAME: Ty = Ty(n);` for each name, then `COUNT`, `NAMES` and
/// `by_name`.
fn constants(ty: &str, names: &[&str]) -> String {
    let mut out = String::new();
    for (index, name) in names.iter().enumerate() {
        let ident = name.to_ascii_uppercase();
        writeln!(out, "pub const {ident}: {ty} = {ty}({index});").unwrap();
    }
    writeln!(out, "pub const COUNT: usize = {};", names.len()).unwrap();
    writeln!(out, "pub static NAMES: [&str; COUNT] = [").unwrap();
    for name in names {
        writeln!(out, "    {name:?},").unwrap();
    }
    writeln!(out, "];").unwrap();
    writeln!(
        out,
        "/// The {ty} named `name` (without the `minecraft:` namespace).\n\
         pub fn by_name(name: &str) -> Option<{ty}> {{\n    match name {{"
    )
    .unwrap();
    for name in names {
        let ident = name.to_ascii_uppercase();
        writeln!(out, "        {name:?} => Some({ident}),").unwrap();
    }
    writeln!(out, "        _ => None,\n    }}\n}}").unwrap();
    out
}

fn main() {
    let blocks: Vec<BlockEntry> = read_with_placeholders(
        "assets/data/blocks.ron",
        "assets/data/placeholder_blocks.ron",
    );
    let items: Vec<ItemEntry> =
        read_with_placeholders("assets/data/items.ron", "assets/data/placeholder_items.ron");

    let block_names: Vec<&str> = blocks.iter().map(|b| b.name.as_str()).collect();
    let mut item_names: Vec<&str> = blocks
        .iter()
        .filter(|b| b.item)
        .map(|b| b.name.as_str())
        .collect();
    item_names.extend(items.iter().map(|i| i.name.as_str()));

    for (kind, names) in [("block", &block_names), ("item", &item_names)] {
        let mut sorted = names.to_vec();
        sorted.sort_unstable();
        if let Some(pair) = sorted.windows(2).find(|pair| pair[0] == pair[1]) {
            panic!("two {kind}s are named {:?}", pair[0]);
        }
    }

    let out = env::var("OUT_DIR").unwrap();
    let out = Path::new(&out);
    fs::write(out.join("blocks.rs"), constants("BlockId", &block_names)).unwrap();
    fs::write(out.join("items.rs"), constants("ItemId", &item_names)).unwrap();
}
