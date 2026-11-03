//! Who may do what on a hosted world (M43): operators, bans and the
//! whitelist, kept in plain text files beside the world's level.

use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::Path,
};

/// Operators, banned players (with a reason) and the whitelist.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Admin {
    pub ops: BTreeSet<String>,
    pub banned: BTreeMap<String, String>,
    pub whitelist: BTreeSet<String>,
    pub whitelist_on: bool,
}

const OPS: &str = "ops.txt";
const BANNED: &str = "banned-players.txt";
const WHITELIST: &str = "whitelist.txt";

impl Admin {
    /// Reads the lists in `dir`; missing files are empty lists.
    pub fn load(dir: &Path) -> Self {
        let lines = |name: &str| -> Vec<String> {
            fs::read_to_string(dir.join(name))
                .map(|text| text.lines().map(str::to_owned).collect())
                .unwrap_or_default()
        };
        let mut whitelist: BTreeSet<String> = lines(WHITELIST).into_iter().collect();
        let whitelist_on = whitelist.remove("#on");
        Self {
            ops: lines(OPS).into_iter().filter(|l| !l.is_empty()).collect(),
            banned: lines(BANNED)
                .into_iter()
                .filter_map(|line| {
                    let (name, reason) = line.split_once('\t').unwrap_or((&line, ""));
                    (!name.is_empty()).then(|| (name.to_owned(), reason.to_owned()))
                })
                .collect(),
            whitelist,
            whitelist_on,
        }
    }

    /// Writes the lists into `dir`.
    pub fn save(&self, dir: &Path) {
        let write = |name: &str, text: String| {
            if let Err(err) = fs::write(dir.join(name), text) {
                log::warn!("couldn't write {name}: {err}");
            }
        };
        write(OPS, self.ops.iter().map(|n| format!("{n}\n")).collect());
        write(
            BANNED,
            self.banned
                .iter()
                .map(|(n, why)| format!("{n}\t{why}\n"))
                .collect(),
        );
        let mut white: String = self.whitelist.iter().map(|n| format!("{n}\n")).collect();
        if self.whitelist_on {
            white.push_str("#on\n");
        }
        write(WHITELIST, white);
    }

    /// Why `name` may not join, if they may not.
    pub fn refusal(&self, name: &str) -> Option<String> {
        if let Some(reason) = self.banned.get(name) {
            let why = if reason.is_empty() {
                "Banned by an operator."
            } else {
                reason
            };
            return Some(format!("You are banned from this server: {why}"));
        }
        if self.whitelist_on && !self.whitelist.contains(name) && !self.ops.contains(name) {
            return Some("You are not white-listed on this server!".to_owned());
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lists_survive_a_save_and_bans_and_the_whitelist_refuse() {
        let dir = std::env::temp_dir().join(format!("rc-admin-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let mut admin = Admin::default();
        admin.ops.insert("Kenzo".into());
        admin.banned.insert("Griefer".into(), "Griefing".into());
        admin.whitelist.insert("Alex".into());
        admin.whitelist_on = true;
        admin.save(&dir);
        let back = Admin::load(&dir);
        assert_eq!(back, admin);
        assert_eq!(
            back.refusal("Griefer").as_deref(),
            Some("You are banned from this server: Griefing")
        );
        assert_eq!(
            back.refusal("Stranger").as_deref(),
            Some("You are not white-listed on this server!")
        );
        assert_eq!(back.refusal("Alex"), None);
        assert_eq!(back.refusal("Kenzo"), None, "operators always get in");
        fs::remove_dir_all(&dir).ok();
    }
}
