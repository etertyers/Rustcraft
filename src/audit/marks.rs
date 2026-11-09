//! Kenzo's marks from the Texture Viewer: which textures he found broken
//! or fine. Saved at once to `texture-marks.ron` in the game folder; a
//! later session fixes from that file.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Verdict {
    Broken,
    Ok,
}

/// One mark: a texture (block, item, mob...) in one texture set.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Mark {
    pub id: String,
    /// The texture set it was looked at in ("vanilla", "barebones").
    pub set: String,
    pub verdict: Verdict,
    pub note: String,
    /// `YYYY-MM-DD`.
    pub date: String,
}

#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Marks {
    pub marks: Vec<Mark>,
}

/// `texture-marks.ron` beside the saves folder.
pub fn marks_path() -> PathBuf {
    crate::level::saves_dir().with_file_name("texture-marks.ron")
}

/// Today as `YYYY-MM-DD` (UTC).
pub fn today() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    date_of(secs)
}

/// The civil date of a Unix time.
pub fn date_of(secs: u64) -> String {
    let days = (secs / 86_400) as i64 + 719_468;
    let era = days.div_euclid(146_097);
    let doe = days.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}-{m:02}-{d:02}")
}

impl Marks {
    /// The marks in `path`, none if it's missing or unreadable.
    pub fn load(path: &Path) -> Self {
        std::fs::read_to_string(path)
            .ok()
            .and_then(|text| ron::from_str(&text).ok())
            .unwrap_or_default()
    }

    /// Writes the marks to `path`.
    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        let text = ron::ser::to_string_pretty(self, ron::ser::PrettyConfig::default())
            .map_err(std::io::Error::other)?;
        std::fs::write(path, text)
    }

    pub fn get(&self, id: &str, set: &str) -> Option<&Mark> {
        self.marks.iter().find(|m| m.id == id && m.set == set)
    }

    /// Marks `id` in `set`, replacing an earlier mark.
    pub fn mark(&mut self, id: &str, set: &str, verdict: Verdict, note: &str, date: &str) {
        self.clear(id, set);
        self.marks.push(Mark {
            id: id.to_owned(),
            set: set.to_owned(),
            verdict,
            note: note.to_owned(),
            date: date.to_owned(),
        });
    }

    pub fn clear(&mut self, id: &str, set: &str) {
        self.marks.retain(|m| !(m.id == id && m.set == set));
    }

    /// (broken, ok) in `set`.
    pub fn counts(&self, set: &str) -> (usize, usize) {
        let of = |v| {
            self.marks
                .iter()
                .filter(|m| m.set == set && m.verdict == v)
                .count()
        };
        (of(Verdict::Broken), of(Verdict::Ok))
    }

    /// The marks as a table, for `--texture-marks`.
    pub fn table(&self) -> String {
        let mut out = format!(
            "{:<40} {:<10} {:<7} {:<11} note\n",
            "id", "set", "mark", "date"
        );
        for m in &self.marks {
            out += &format!(
                "{:<40} {:<10} {:<7} {:<11} {}\n",
                m.id,
                m.set,
                format!("{:?}", m.verdict).to_lowercase(),
                m.date,
                m.note
            );
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn marks_round_trip_replace_and_clear() {
        let dir = crate::world::save::TempDir::new("marks");
        let path = dir.path().join("texture-marks.ron");
        let mut marks = Marks::default();
        marks.mark("block/stone", "vanilla", Verdict::Ok, "", "2026-11-04");
        marks.mark(
            "mob/cow",
            "vanilla",
            Verdict::Broken,
            "legs inside out",
            "2026-11-04",
        );
        marks.mark("mob/cow", "barebones", Verdict::Broken, "", "2026-11-04");
        marks.save(&path).unwrap();
        let loaded = Marks::load(&path);
        assert_eq!(loaded, marks);
        assert_eq!(loaded.counts("vanilla"), (1, 1));
        assert_eq!(
            loaded.get("mob/cow", "vanilla").unwrap().note,
            "legs inside out"
        );
        // A new mark replaces the old one for the same texture and set.
        let mut marks = loaded;
        marks.mark("mob/cow", "vanilla", Verdict::Ok, "fixed", "2026-11-05");
        assert_eq!(marks.counts("vanilla"), (0, 2));
        assert_eq!(marks.counts("barebones"), (1, 0));
        marks.clear("mob/cow", "vanilla");
        assert!(marks.get("mob/cow", "vanilla").is_none());
        assert!(marks.table().contains("block/stone"));
    }

    #[test]
    fn a_missing_or_broken_file_is_no_marks() {
        let dir = crate::world::save::TempDir::new("marks-none");
        assert_eq!(Marks::load(&dir.path().join("nope.ron")), Marks::default());
        std::fs::write(dir.path().join("bad.ron"), "(((").unwrap();
        assert_eq!(Marks::load(&dir.path().join("bad.ron")), Marks::default());
    }

    #[test]
    fn dates_are_civil() {
        assert_eq!(date_of(0), "1970-01-01");
        assert_eq!(date_of(1_793_750_400), "2026-11-04");
    }
}
