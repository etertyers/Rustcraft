//! Music discs (vanilla 1.21's `JukeboxSong`s): each disc's description
//! line, how long its song lasts and the sound event a jukebox plays.

/// One music disc.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Disc {
    /// The part of the item's name after `music_disc_` (`cat`).
    pub name: &'static str,
    /// The description line under the item's name ("C418 - cat").
    pub title: &'static str,
    /// Length of the song in seconds.
    pub seconds: u32,
}

impl Disc {
    /// The sound event a jukebox plays (`music_disc.cat`).
    pub fn event(&self) -> String {
        format!("music_disc.{}", self.name)
    }

    /// The song's length in ticks.
    pub fn ticks(&self) -> u64 {
        u64::from(self.seconds) * 20
    }

    /// The item's name (`music_disc_cat`).
    pub fn item_name(&self) -> String {
        format!("music_disc_{}", self.name)
    }
}

const fn disc(name: &'static str, title: &'static str, seconds: u32) -> Disc {
    Disc {
        name,
        title,
        seconds,
    }
}

/// Every disc, in vanilla's order.
pub const DISCS: [Disc; 19] = [
    disc("13", "C418 - 13", 178),
    disc("cat", "C418 - cat", 185),
    disc("blocks", "C418 - blocks", 345),
    disc("chirp", "C418 - chirp", 185),
    disc("far", "C418 - far", 174),
    disc("mall", "C418 - mall", 197),
    disc("mellohi", "C418 - mellohi", 96),
    disc("stal", "C418 - stal", 150),
    disc("strad", "C418 - strad", 188),
    disc("ward", "C418 - ward", 251),
    disc("11", "C418 - 11", 71),
    disc("wait", "C418 - wait", 238),
    disc("otherside", "Lena Raine - otherside", 195),
    disc("5", "Samuel Åberg - 5", 178),
    disc("pigstep", "Lena Raine - Pigstep", 149),
    disc("relic", "Aaron Cherof - Relic", 218),
    disc("creator", "Lena Raine - Creator", 176),
    disc("creator_music_box", "Lena Raine - Creator (Music Box)", 73),
    disc("precipice", "Aaron Cherof - Precipice", 299),
];

/// The twelve a creeper killed by a skeleton's or stray's arrow drops
/// (vanilla's `#creeper_drop_music_discs`).
pub const CREEPER_DROPS: [&str; 12] = [
    "13", "cat", "blocks", "chirp", "far", "mall", "mellohi", "stal", "strad", "ward", "11", "wait",
];

/// The disc an item name (`music_disc_cat`) is, if it is one.
pub fn of_item(item_name: &str) -> Option<&'static Disc> {
    let name = item_name.strip_prefix("music_disc_")?;
    DISCS.iter().find(|disc| disc.name == name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_disc_has_an_event_a_length_and_a_line() {
        for disc in DISCS {
            assert!(disc.seconds > 0 && disc.title.contains(" - "), "{disc:?}");
            assert_eq!(of_item(&disc.item_name()), Some(&disc));
            assert!(disc.event().starts_with("music_disc."));
        }
        assert_eq!(of_item("music_disc_cat").unwrap().seconds, 185);
        assert_eq!(of_item("music_disc_13").unwrap().seconds, 178);
        assert_eq!(of_item("music_disc_11").unwrap().seconds, 71);
        assert_eq!(of_item("stick"), None);
    }

    #[test]
    fn creeper_drops_are_the_twelve_older_discs() {
        for name in CREEPER_DROPS {
            assert!(DISCS.iter().any(|disc| disc.name == name));
        }
    }
}
