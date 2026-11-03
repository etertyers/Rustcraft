//! Each look keeps its own resource pack list (v1.4). The Trailer list
//! starts with the three Trailer packs, top first: Fresh Animations,
//! Barebones Better Leaves, Bare Bones, if they're in `resourcepacks/`.
//! None of them is bundled: Kenzo downloads them.
//!
//! A pack is recognised by its file name or its `pack.mcmeta` description,
//! ignoring case, spaces, dashes, underscores and version numbers.

/// One of the packs the Trailer look turns on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrailerPack {
    FreshAnimations,
    BetterLeaves,
    BareBones,
}

impl TrailerPack {
    /// Top first.
    pub const ALL: [TrailerPack; 3] = [
        TrailerPack::FreshAnimations,
        TrailerPack::BetterLeaves,
        TrailerPack::BareBones,
    ];

    /// The normalised name it's matched by (also its key in
    /// `Visuals::offered`).
    pub fn key(self) -> &'static str {
        match self {
            TrailerPack::FreshAnimations => "freshanimations",
            TrailerPack::BetterLeaves => "barebonesbetterleaves",
            TrailerPack::BareBones => "barebones",
        }
    }

    pub fn title(self) -> &'static str {
        match self {
            TrailerPack::FreshAnimations => "Fresh Animations",
            TrailerPack::BetterLeaves => "Barebones Better Leaves",
            TrailerPack::BareBones => "Bare Bones",
        }
    }
}

/// Words dropped when matching: file extensions and "pack".
const NOISE: [&str; 6] = ["zip", "resourcepack", "texturepack", "pack", "mc", "fabric"];

/// `name` lowercased, split into words at anything that isn't a letter or
/// digit and between letters and digits, without version numbers
/// (`1.21.4`, `v2`, `r5`) or noise words, joined.
pub fn normalise(name: &str) -> String {
    let mut words: Vec<String> = Vec::new();
    let mut word = String::new();
    let mut digits = false;
    for c in name.chars().flat_map(char::to_lowercase) {
        if c.is_alphanumeric() {
            let digit = c.is_ascii_digit();
            if !word.is_empty() && digit != digits {
                words.push(std::mem::take(&mut word));
            }
            digits = digit;
            word.push(c);
        } else if !word.is_empty() {
            words.push(std::mem::take(&mut word));
        }
    }
    if !word.is_empty() {
        words.push(word);
    }
    let mut out = String::new();
    for (i, w) in words.iter().enumerate() {
        let number = w.chars().all(|c| c.is_ascii_digit());
        // `v1.9` and `r5`: a lone letter before a number.
        let version = (w == "v" || w == "r")
            && words
                .get(i + 1)
                .is_some_and(|n| n.chars().all(|c| c.is_ascii_digit()));
        if !number && !version && !NOISE.contains(&w.as_str()) {
            out.push_str(w);
        }
    }
    out
}

/// Which Trailer pack a pack is, from its file name or description.
pub fn identify(file_name: &str, description: &str) -> Option<TrailerPack> {
    let names = [normalise(file_name), normalise(description)];
    TrailerPack::ALL
        .into_iter()
        .find(|pack| names.iter().any(|n| n == pack.key()))
}

/// The Trailer list: `current` with each Trailer pack found in `available`
/// (file name, description) and not offered before added, just below the
/// Trailer packs that go above it (so on top, in the Trailer order, the
/// first time). Returns the list and the packs that are missing.
pub fn trailer_list(
    current: &[String],
    available: &[(String, String)],
    offered: &mut Vec<String>,
) -> (Vec<String>, Vec<TrailerPack>) {
    let file_of = |pack: TrailerPack| {
        available
            .iter()
            .find(|(file, description)| identify(file, description) == Some(pack))
            .map(|(file, _)| file.clone())
    };
    let mut list = current.to_vec();
    let mut missing = Vec::new();
    for (i, pack) in TrailerPack::ALL.into_iter().enumerate() {
        let Some(file) = file_of(pack) else {
            missing.push(pack);
            continue;
        };
        if offered.iter().any(|k| k == pack.key()) {
            continue;
        }
        offered.push(pack.key().to_owned());
        if list.contains(&file) {
            continue;
        }
        let at = TrailerPack::ALL[..i]
            .iter()
            .filter_map(|above| file_of(*above))
            .filter_map(|above| list.iter().position(|f| *f == above))
            .max()
            .map_or(0, |p| p + 1);
        list.insert(at, file);
    }
    (list, missing)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn real_download_names_are_recognised() {
        let cases = [
            (
                "FreshAnimations_v1.9.4.zip",
                Some(TrailerPack::FreshAnimations),
            ),
            (
                "Fresh Animations 1.9.2.zip",
                Some(TrailerPack::FreshAnimations),
            ),
            ("fresh-animations-v1.10", Some(TrailerPack::FreshAnimations)),
            ("Bare Bones 1.21.zip", Some(TrailerPack::BareBones)),
            ("BareBones_1.21.4.zip", Some(TrailerPack::BareBones)),
            ("Bare-Bones-v2.1-mc1.21.zip", Some(TrailerPack::BareBones)),
            (
                "Barebones-Better-Leaves-1.21.4.zip",
                Some(TrailerPack::BetterLeaves),
            ),
            (
                "Barebones Better Leaves v1.2.zip",
                Some(TrailerPack::BetterLeaves),
            ),
            ("Ashen_1.8.3.zip", None),
            ("texture-pack-default1.20.5-26.2.zip", None),
            ("Bare Bones Addon.zip", None),
        ];
        for (name, want) in cases {
            assert_eq!(identify(name, ""), want, "{name}");
        }
        // By the description when the file was renamed.
        assert_eq!(
            identify("download (3).zip", "§6Fresh Animations §7v1.9"),
            Some(TrailerPack::FreshAnimations)
        );
    }

    #[test]
    fn the_trailer_list_puts_the_packs_on_top_once() {
        let available = vec![
            ("Ashen_1.8.3.zip".to_owned(), "Ashen".to_owned()),
            ("Bare Bones 1.21.zip".to_owned(), String::new()),
            ("FreshAnimations_v1.9.4.zip".to_owned(), String::new()),
        ];
        let mut offered = Vec::new();
        let current = vec!["Ashen_1.8.3.zip".to_owned()];
        let (list, missing) = trailer_list(&current, &available, &mut offered);
        assert_eq!(
            list,
            [
                "FreshAnimations_v1.9.4.zip",
                "Bare Bones 1.21.zip",
                "Ashen_1.8.3.zip"
            ]
        );
        assert_eq!(missing, [TrailerPack::BetterLeaves]);
        // Taken off by the player: it stays off.
        let (again, _) = trailer_list(&current, &available, &mut offered);
        assert_eq!(again, current);
        // Better Leaves downloaded later: it goes on top.
        let mut more = available.clone();
        more.push((
            "Barebones-Better-Leaves-1.21.4.zip".to_owned(),
            String::new(),
        ));
        let (later, missing) = trailer_list(&list, &more, &mut offered);
        assert!(missing.is_empty());
        assert_eq!(later[1], "Barebones-Better-Leaves-1.21.4.zip");
        assert_eq!(later.len(), 4);
    }
}
