//! Advancements (M29, modern `PlayerAdvancements`): a tree of goals in five
//! tabs, each made of criteria that the game's events tick off. The tree is
//! in `assets/data/advancements.ron`; what a world has done is a set of
//! names kept in its level file.
//!
//! The game reports what happens as `Event`s ("mined stone", "entered the
//! Nether", "has a diamond"); an advancement completes when every one of its
//! criteria has been met, and the newly completed ones are handed back so
//! the game can show their toasts.

use std::{
    collections::{BTreeMap, BTreeSet},
    sync::LazyLock,
};

use serde::{Deserialize, Serialize};

/// The five tabs, in the order they are shown.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Deserialize)]
pub enum Tab {
    Minecraft,
    Nether,
    End,
    Adventure,
    Husbandry,
}

impl Tab {
    pub const ALL: [Tab; 5] = [
        Tab::Minecraft,
        Tab::Nether,
        Tab::End,
        Tab::Adventure,
        Tab::Husbandry,
    ];

    pub fn title(self) -> &'static str {
        match self {
            Tab::Minecraft => "Minecraft",
            Tab::Nether => "Nether",
            Tab::End => "The End",
            Tab::Adventure => "Adventure",
            Tab::Husbandry => "Husbandry",
        }
    }

    /// The icon on its tab: an item name.
    pub fn icon(self) -> &'static str {
        match self {
            Tab::Minecraft => "grass_block",
            Tab::Nether => "netherrack",
            Tab::End => "end_stone",
            Tab::Adventure => "map",
            Tab::Husbandry => "hay_block",
        }
    }
}

/// How an advancement's frame looks and what its toast says.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
pub enum Frame {
    Task,
    Goal,
    Challenge,
}

impl Frame {
    /// The header of its toast.
    pub fn toast(self) -> &'static str {
        match self {
            Frame::Task => "Advancement Made!",
            Frame::Goal => "Goal Reached!",
            Frame::Challenge => "Challenge Complete!",
        }
    }
}

/// What ticks a criterion off. Names are modern's, less the namespace.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub enum Trigger {
    /// Always, from the first tick of a world.
    Always,
    /// Has this item (or any item whose name has this suffix with `*`).
    Have(String),
    /// Breaks this block.
    Mined(String),
    /// Puts this block down.
    Placed(String),
    /// Eats this item; `*` is anything.
    Ate(String),
    /// Kills this mob; `*` is any mob, `*hostile` any monster.
    Killed(String),
    /// Goes into this dimension (`the_nether`, `the_end`, `overworld`).
    Dimension(String),
    /// Is in this biome.
    Biome(String),
    /// Is inside this kind of structure.
    Structure(String),
    /// Something the game reports by name: `enchant`, `trade`, `sleep`,
    /// `brew`, `tame`, `shoot`, `shear`, `levitate`, `gateway`, `kill_dragon`.
    Used(String),
}

/// What happened, to match against triggers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    Have(String),
    Mined(String),
    Placed(String),
    Ate(String),
    Killed { mob: String, hostile: bool },
    Dimension(String),
    Biome(String),
    Structure(String),
    Used(String),
}

impl Trigger {
    fn matches(&self, event: &Event) -> bool {
        match (self, event) {
            (Trigger::Always, _) => true,
            (Trigger::Have(a), Event::Have(b))
            | (Trigger::Mined(a), Event::Mined(b))
            | (Trigger::Placed(a), Event::Placed(b))
            | (Trigger::Dimension(a), Event::Dimension(b))
            | (Trigger::Biome(a), Event::Biome(b))
            | (Trigger::Structure(a), Event::Structure(b))
            | (Trigger::Used(a), Event::Used(b)) => a == b,
            (Trigger::Ate(a), Event::Ate(b)) => a == "*" || a == b,
            (Trigger::Killed(a), Event::Killed { mob, hostile }) => {
                a == "*" || (a == "*hostile" && *hostile) || a == mob
            }
            _ => false,
        }
    }
}

/// One advancement as written in the data file.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Def {
    pub id: String,
    pub tab: Tab,
    /// The advancement it hangs from; `None` for a tab's first.
    #[serde(default)]
    pub parent: Option<String>,
    pub title: String,
    pub description: String,
    /// Its icon: an item name.
    pub icon: String,
    pub frame: Frame,
    /// Each must be met (name and what meets it).
    pub criteria: Vec<(String, Trigger)>,
}

/// A node placed on its tab's page: columns across by depth, rows down.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Place {
    pub column: u32,
    pub row: f32,
}

/// Every advancement, with the tree laid out.
pub struct Registry {
    pub defs: Vec<Def>,
    by_id: BTreeMap<String, usize>,
    places: Vec<Place>,
}

impl Registry {
    pub fn load(path: &std::path::Path) -> anyhow::Result<Self> {
        let text = std::fs::read_to_string(path)?;
        let defs: Vec<Def> = ron::from_str(&text)?;
        Self::new(defs)
    }

    pub fn new(defs: Vec<Def>) -> anyhow::Result<Self> {
        let by_id: BTreeMap<String, usize> = defs
            .iter()
            .enumerate()
            .map(|(i, d)| (d.id.clone(), i))
            .collect();
        anyhow::ensure!(by_id.len() == defs.len(), "an advancement id is used twice");
        for def in &defs {
            if let Some(parent) = &def.parent {
                let &p = by_id
                    .get(parent)
                    .ok_or_else(|| anyhow::anyhow!("{} has no parent {parent}", def.id))?;
                anyhow::ensure!(defs[p].tab == def.tab, "{} is in another tab", def.id);
            }
        }
        let places = layout(&defs, &by_id);
        Ok(Self {
            defs,
            by_id,
            places,
        })
    }

    pub fn index(&self, id: &str) -> Option<usize> {
        self.by_id.get(id).copied()
    }

    pub fn place(&self, index: usize) -> Place {
        self.places[index]
    }

    /// The advancements of `tab`, in the data file's order.
    pub fn in_tab(&self, tab: Tab) -> impl Iterator<Item = usize> + '_ {
        (0..self.defs.len()).filter(move |&i| self.defs[i].tab == tab)
    }

    /// Every item name some `Have` trigger asks about, for the game to
    /// check the inventory against.
    pub fn wanted_items(&self) -> BTreeSet<&str> {
        self.defs
            .iter()
            .flat_map(|d| &d.criteria)
            .filter_map(|(_, t)| match t {
                Trigger::Have(name) => Some(name.as_str()),
                _ => None,
            })
            .collect()
    }
}

/// Columns by depth; rows so that each parent sits at the middle of its
/// children and no two nodes of a column share a row.
fn layout(defs: &[Def], by_id: &BTreeMap<String, usize>) -> Vec<Place> {
    let mut children: Vec<Vec<usize>> = vec![Vec::new(); defs.len()];
    let mut roots = Vec::new();
    for (i, def) in defs.iter().enumerate() {
        match def.parent.as_ref().and_then(|p| by_id.get(p)) {
            Some(&p) => children[p].push(i),
            None => roots.push(i),
        }
    }
    let mut places = vec![
        Place {
            column: 0,
            row: 0.0
        };
        defs.len()
    ];
    // Each tab's trees are stacked from row 0.
    let mut next_row: BTreeMap<Tab, f32> = BTreeMap::new();
    fn walk(
        i: usize,
        depth: u32,
        children: &[Vec<usize>],
        places: &mut [Place],
        next: &mut f32,
    ) -> f32 {
        let row = if children[i].is_empty() {
            let row = *next;
            *next += 1.0;
            row
        } else {
            let rows: Vec<f32> = children[i]
                .iter()
                .map(|&c| walk(c, depth + 1, children, places, next))
                .collect();
            (rows[0] + rows[rows.len() - 1]) / 2.0
        };
        places[i] = Place { column: depth, row };
        row
    }
    for root in roots {
        let next = next_row.entry(defs[root].tab).or_insert(0.0);
        walk(root, 0, &children, &mut places, next);
    }
    places
}

static REGISTRY: LazyLock<Registry> = LazyLock::new(|| {
    Registry::load(&crate::assets::dir().join("data/advancements.ron"))
        .expect("assets/data/advancements.ron")
});

/// The game's advancements, loaded once.
pub fn registry() -> &'static Registry {
    &REGISTRY
}

/// What a world has done, as saved.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Progress {
    /// Advancements completed.
    pub done: BTreeSet<String>,
    /// Criteria met, as `id#criterion`, for ones not yet complete.
    pub criteria: BTreeSet<String>,
}

impl Progress {
    pub fn is_done(&self, id: &str) -> bool {
        self.done.contains(id)
    }

    /// How many criteria of `def` are met.
    pub fn met(&self, def: &Def) -> usize {
        if self.is_done(&def.id) {
            return def.criteria.len();
        }
        def.criteria
            .iter()
            .filter(|(name, _)| self.criteria.contains(&format!("{}#{name}", def.id)))
            .count()
    }

    /// Counts `event` against every advancement not yet done; returns the
    /// indices of those it completed.
    pub fn record(&mut self, registry: &Registry, event: &Event) -> Vec<usize> {
        let mut completed = Vec::new();
        for (i, def) in registry.defs.iter().enumerate() {
            if self.is_done(&def.id) {
                continue;
            }
            let mut changed = false;
            for (name, trigger) in &def.criteria {
                if trigger.matches(event) {
                    changed |= self.criteria.insert(format!("{}#{name}", def.id));
                }
            }
            if changed && self.met(def) == def.criteria.len() {
                for (name, _) in &def.criteria {
                    self.criteria.remove(&format!("{}#{name}", def.id));
                }
                self.done.insert(def.id.clone());
                completed.push(i);
            }
        }
        completed
    }

    /// How many of `tab`'s advancements are done, and how many there are.
    pub fn tally(&self, registry: &Registry, tab: Tab) -> (usize, usize) {
        let all: Vec<usize> = registry.in_tab(tab).collect();
        let done = all
            .iter()
            .filter(|&&i| self.is_done(&registry.defs[i].id))
            .count();
        (done, all.len())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn def(id: &str, parent: Option<&str>, criteria: Vec<(&str, Trigger)>) -> Def {
        Def {
            id: id.into(),
            tab: Tab::Minecraft,
            parent: parent.map(Into::into),
            title: id.into(),
            description: String::new(),
            icon: "stone".into(),
            frame: Frame::Task,
            criteria: criteria
                .into_iter()
                .map(|(n, t)| (n.to_owned(), t))
                .collect(),
        }
    }

    #[test]
    fn an_advancement_completes_when_its_last_criterion_is_met_and_only_once() {
        let registry = Registry::new(vec![
            def("root", None, vec![("start", Trigger::Always)]),
            def(
                "both",
                Some("root"),
                vec![
                    ("stone", Trigger::Mined("stone".into())),
                    ("pick", Trigger::Have("stone_pickaxe".into())),
                ],
            ),
        ])
        .unwrap();
        let mut progress = Progress::default();
        assert_eq!(
            progress.record(&registry, &Event::Used("anything".into())),
            [0]
        );
        assert!(
            progress
                .record(&registry, &Event::Mined("stone".into()))
                .is_empty()
        );
        assert_eq!(progress.met(&registry.defs[1]), 1);
        // The same event again changes nothing.
        assert!(
            progress
                .record(&registry, &Event::Mined("stone".into()))
                .is_empty()
        );
        let done = progress.record(&registry, &Event::Have("stone_pickaxe".into()));
        assert_eq!(done, [1]);
        assert!(progress.is_done("both"));
        assert!(
            progress
                .record(&registry, &Event::Have("stone_pickaxe".into()))
                .is_empty()
        );
        assert_eq!(progress.tally(&registry, Tab::Minecraft), (2, 2));
        assert_eq!(progress.tally(&registry, Tab::End), (0, 0));
    }

    #[test]
    fn triggers_match_their_own_kind_and_wildcards() {
        let any_kill = Trigger::Killed("*".into());
        let hostile = Trigger::Killed("*hostile".into());
        let zombie = Trigger::Killed("zombie".into());
        let pig = Event::Killed {
            mob: "pig".into(),
            hostile: false,
        };
        let husk = Event::Killed {
            mob: "husk".into(),
            hostile: true,
        };
        assert!(any_kill.matches(&pig) && any_kill.matches(&husk));
        assert!(!hostile.matches(&pig) && hostile.matches(&husk));
        assert!(!zombie.matches(&husk));
        assert!(Trigger::Ate("*".into()).matches(&Event::Ate("bread".into())));
        assert!(!Trigger::Ate("apple".into()).matches(&Event::Ate("bread".into())));
        // A block named like an item isn't the item.
        assert!(!Trigger::Have("stone".into()).matches(&Event::Mined("stone".into())));
    }

    #[test]
    fn the_tree_is_laid_out_by_depth_with_parents_between_their_children() {
        let registry = Registry::new(vec![
            def("a", None, vec![]),
            def("b", Some("a"), vec![]),
            def("c", Some("a"), vec![]),
            def("d", Some("b"), vec![]),
        ])
        .unwrap();
        let place = |id: &str| registry.place(registry.index(id).unwrap());
        assert_eq!(place("a").column, 0);
        assert_eq!((place("b").column, place("d").column), (1, 2));
        assert_eq!(place("d").row, 0.0);
        assert_eq!(place("c").row, 1.0);
        assert_eq!(place("b").row, 0.0);
        assert_eq!(place("a").row, 0.5, "between b and c");
    }

    #[test]
    fn a_bad_tree_is_refused() {
        assert!(Registry::new(vec![def("a", Some("missing"), vec![])]).is_err());
        assert!(Registry::new(vec![def("a", None, vec![]), def("a", None, vec![])]).is_err());
    }

    /// The names of what `Used` triggers the game reports.
    pub const USED: &[&str] = &[
        "enchant",
        "trade",
        "sleep",
        "brew",
        "tame",
        "shoot",
        "shear",
        "levitate",
        "gateway",
        "kill_dragon",
    ];

    #[test]
    fn the_shipped_tree_only_names_things_that_exist_and_every_tab_has_a_root() {
        use crate::{
            entity::mobs::MobRegistry,
            inventory::items,
            world::{biome::Biome, block},
        };
        let registry = registry();
        let mobs = MobRegistry::load(&crate::assets::dir().join("data/mobs.ron")).unwrap();
        let slug = |b: Biome| b.name().to_lowercase().replace(' ', "_");
        let biomes: Vec<String> = Biome::ALL.iter().map(|&b| slug(b)).collect();
        let structures = [
            "village",
            "desert temple",
            "jungle temple",
            "shipwreck",
            "ruined portal",
            "dungeon",
            "mineshaft",
            "stronghold",
            "nether fortress",
            "bastion remnant",
            "end city",
        ];
        for def in &registry.defs {
            assert!(
                items::by_name(&def.icon).is_some()
                    || block::registry().iter().any(|b| b.name == def.icon),
                "{}: icon {}",
                def.id,
                def.icon
            );
            assert!(
                !def.title.is_empty() && !def.description.is_empty() && !def.criteria.is_empty()
            );
            for (name, trigger) in &def.criteria {
                let id = format!("{}#{name}", def.id);
                match trigger {
                    Trigger::Always => {}
                    Trigger::Have(item) | Trigger::Ate(item) if item != "*" => {
                        assert!(items::by_name(item).is_some(), "{id}: item {item}");
                    }
                    Trigger::Mined(b) | Trigger::Placed(b) => {
                        assert!(
                            block::registry().iter().any(|d| d.name == *b),
                            "{id}: block {b}"
                        );
                    }
                    Trigger::Killed(m) if !m.starts_with('*') => {
                        assert!(mobs.find(m).is_some(), "{id}: mob {m}");
                    }
                    Trigger::Dimension(d) => {
                        assert!(
                            ["overworld", "the_nether", "the_end"].contains(&d.as_str()),
                            "{id}"
                        );
                    }
                    Trigger::Biome(b) => assert!(biomes.contains(b), "{id}: biome {b}"),
                    Trigger::Structure(s) => assert!(structures.contains(&s.as_str()), "{id}: {s}"),
                    Trigger::Used(u) => assert!(USED.contains(&u.as_str()), "{id}: {u}"),
                    _ => {}
                }
            }
        }
        for tab in Tab::ALL {
            let roots = registry
                .in_tab(tab)
                .filter(|&i| registry.defs[i].parent.is_none())
                .count();
            assert_eq!(roots, 1, "{tab:?} has one root");
            assert!(
                registry.in_tab(tab).count() >= 6,
                "{tab:?} has enough to do"
            );
        }
        // Nodes in a column never share a row.
        for tab in Tab::ALL {
            let mut seen = std::collections::BTreeSet::new();
            for i in registry.in_tab(tab) {
                let place = registry.place(i);
                assert!(
                    seen.insert((place.column, (place.row * 2.0) as i32)),
                    "{} overlaps",
                    registry.defs[i].id
                );
            }
        }
        assert!(registry.wanted_items().contains("diamond"));
    }
}
