//! Villagers' lives: claiming a job site (which sets the profession) and a
//! bed, the day's schedule (work at the job site, meet at the bell, sleep
//! in bed), restocking, farmers tending wheat, breeding when well fed with
//! a free bed about, iron golems for a village of five or more, and
//! trading with the player.

use glam::{DVec3, IVec3};

use super::{Container, Game};
use crate::{
    entity::{
        mob::Mob,
        mobs::Ai,
        villager::{Profession, VillagerData},
    },
    inventory::{items, player::MAIN_SLOTS, stack::ItemStack},
    world::{
        block::BlockState,
        blocks,
        props::{Prop, bed_part},
        worldgen::rng::Rng,
    },
};

/// How far a villager looks for job sites, beds and bells.
const SEARCH: i32 = 24;
const SEARCH_DOWN: i32 = 6;
/// Ticks between a villager's searches.
const SCAN_TICKS: u32 = 200;
/// A day's schedule (modern `Schedule.VILLAGER_DEFAULT`), in ticks of the
/// day.
const WORK_FROM: u64 = 2000;
const MEET_FROM: u64 = 9000;
const WANDER_FROM: u64 = 11000;
const SLEEP_FROM: u64 = 12000;
/// Food a villager needs to breed (bread is 4), how long until it can
/// again, and how long a baby takes to grow up.
const BREED_FOOD: u32 = 12;
const BREED_COOLDOWN: u32 = 6000;
const BABY_TICKS: u32 = 24000;
/// A village of this many villagers within 16 blocks keeps an iron golem,
/// one for every ten; the check comes this often.
const GOLEM_VILLAGERS: usize = 5;
const GOLEM_TICKS: u64 = 700;
/// What a golem may appear standing on.
const GOLEM_GROUND: &[crate::world::block::BlockId] = &[
    blocks::GRASS_BLOCK,
    blocks::DIRT,
    blocks::DIRT_PATH,
    blocks::SAND,
    blocks::SNOW_BLOCK,
    blocks::COBBLESTONE,
    blocks::SMOOTH_SANDSTONE,
    blocks::CUT_SANDSTONE,
    blocks::PODZOL,
    blocks::COARSE_DIRT,
];

fn is_villager(game: &Game, mob: &Mob) -> bool {
    matches!(game.mob_defs.get(mob.kind).ai, Ai::Villager)
}

impl Game {
    /// A fresh villager's data.
    pub(super) fn new_villager_data() -> Option<Box<VillagerData>> {
        Some(Box::default())
    }

    /// The villager with id `id`, if it's here.
    fn villager_index(&self, id: u64) -> Option<usize> {
        self.mobs
            .iter()
            .position(|m| m.id == id && m.data.villager.is_some())
    }

    /// Job sites, beds (heads) and bells near `around`.
    fn points_of_interest(
        &self,
        around: IVec3,
    ) -> (Vec<(IVec3, Profession)>, Vec<IVec3>, Vec<IVec3>) {
        let world = self.chunks.world();
        let (mut sites, mut beds, mut bells) = (Vec::new(), Vec::new(), Vec::new());
        for y in around.y - SEARCH_DOWN..=around.y + SEARCH_DOWN {
            for z in around.z - SEARCH..=around.z + SEARCH {
                for x in around.x - SEARCH..=around.x + SEARCH {
                    let Some(block) = world.block(x, y, z) else {
                        continue;
                    };
                    if block.is_air() {
                        continue;
                    }
                    let at = IVec3::new(x, y, z);
                    if let Some(profession) = Profession::of_site(block.id()) {
                        sites.push((at, profession));
                    } else if block.def().name.ends_with("_bed")
                        && block.get(Prop::BedPart) == bed_part::HEAD
                    {
                        beds.push(at);
                    } else if block.is(blocks::BELL) {
                        bells.push(at);
                    }
                }
            }
        }
        (sites, beds, bells)
    }

    /// Whether the block at `at` is still `id`.
    fn still(&self, at: IVec3, test: impl Fn(BlockState) -> bool) -> bool {
        self.chunks
            .world()
            .block(at.x, at.y, at.z)
            .is_some_and(test)
    }

    /// One tick of every villager.
    pub(super) fn tick_villagers(&mut self) {
        let day_time = self.time % 24000;
        let day = self.time / 24000;
        let indices: Vec<usize> = (0..self.mobs.len())
            .filter(|&i| is_villager(self, &self.mobs[i]) && !self.mobs[i].is_dead())
            .collect();
        for &i in &indices {
            if self.mobs[i].data.villager.is_none() {
                self.mobs[i].data.villager = Self::new_villager_data();
            }
            self.tick_villager(i, day_time, day);
        }
        self.villagers_eat();
        self.villagers_breed(&indices);
        if self.time.is_multiple_of(GOLEM_TICKS) {
            self.summon_golems(&indices);
        }
    }

    fn tick_villager(&mut self, i: usize, day_time: u64, day: u64) {
        let feet = self.mobs[i].feet();
        let cell = feet.floor().as_ivec3();
        let Some(mut v) = self.mobs[i].data.villager.take() else {
            return;
        };
        v.baby = v.baby.saturating_sub(1);
        v.breed_cooldown = v.breed_cooldown.saturating_sub(1);
        v.scan = v.scan.saturating_sub(1);
        if v.scan == 0 {
            v.scan = SCAN_TICKS + (self.mobs[i].id % 40) as u32;
            self.claim(i, &mut v, cell);
        }
        // What the time of day sends it to do.
        let site = v.job_site();
        let bed = v.bed();
        v.errand = match day_time {
            t if (WORK_FROM..MEET_FROM).contains(&t) => site,
            t if (MEET_FROM..WANDER_FROM).contains(&t) => self.nearest_bell(cell),
            t if t >= SLEEP_FROM => bed,
            _ => None,
        }
        .map(|at| at.as_dvec3() + DVec3::new(0.5, 0.0, 0.5));
        // At work: restock, and a farmer tends the wheat.
        let working = (WORK_FROM..MEET_FROM).contains(&day_time);
        if working && let Some(site) = site {
            let near = feet.distance(site.as_dvec3() + DVec3::splat(0.5)) < 3.0;
            if near {
                v.restock(day);
                if v.profession == Some(Profession::Farmer)
                    && self.time.is_multiple_of(40)
                    && self.harvest_near(site)
                {
                    v.food += 2;
                }
            }
        }
        self.mobs[i].data.villager = Some(v);
    }

    /// Takes a free job site and bed nearby, and lets go of ones that are
    /// gone.
    fn claim(&mut self, i: usize, v: &mut VillagerData, cell: IVec3) {
        if let Some(site) = v.job_site() {
            let job = v.profession.map(Profession::job_site);
            if !self.still(site, |b| Some(b.id()) == job) {
                v.job_site = None;
                if !v.experienced() {
                    v.unemploy();
                }
            }
        }
        if let Some(bed) = v.bed()
            && !self.still(bed, |b| b.def().name.ends_with("_bed"))
        {
            v.bed = None;
        }
        if v.job_site.is_some() && v.bed.is_some() || v.baby > 0 && v.bed.is_some() {
            return;
        }
        let (sites, beds, _) = self.points_of_interest(cell);
        let taken = |at: IVec3, of: fn(&VillagerData) -> Option<IVec3>| {
            self.mobs
                .iter()
                .enumerate()
                .any(|(j, m)| j != i && m.data.villager.as_deref().and_then(of) == Some(at))
        };
        if v.job_site.is_none() && v.baby == 0 {
            let wanted = |p: Profession| v.profession.is_none_or(|have| have == p);
            let free = sites
                .iter()
                .filter(|&&(at, p)| wanted(p) && !taken(at, VillagerData::job_site))
                .min_by_key(|(at, _)| at.distance_squared(cell));
            if let Some(&(at, profession)) = free {
                v.job_site = Some(at.to_array());
                if v.profession.is_none() {
                    let mut rng = Rng::new(self.rng.next_u64());
                    let items = self.items.clone();
                    v.employ(profession, &items, &mut rng);
                }
            }
        }
        if v.bed.is_none()
            && let Some(at) = beds
                .iter()
                .filter(|&&at| !taken(at, VillagerData::bed))
                .min_by_key(|at| at.distance_squared(cell))
        {
            v.bed = Some(at.to_array());
        }
    }

    fn nearest_bell(&self, cell: IVec3) -> Option<IVec3> {
        // Bells are rare: look round only at meeting time, once a second.
        if !self.time.is_multiple_of(20) {
            return None;
        }
        let (_, _, bells) = self.points_of_interest(cell);
        bells.into_iter().min_by_key(|at| at.distance_squared(cell))
    }

    /// Harvests and replants one ripe wheat within 6 blocks of `site`.
    fn harvest_near(&mut self, site: IVec3) -> bool {
        let world = self.chunks.world();
        let mut ripe = None;
        'search: for y in site.y - 2..=site.y + 2 {
            for z in site.z - 6..=site.z + 6 {
                for x in site.x - 6..=site.x + 6 {
                    if let Some(block) = world.block(x, y, z)
                        && block.is(blocks::WHEAT)
                        && block.number(Prop::Age7) == 7
                    {
                        ripe = Some((IVec3::new(x, y, z), block));
                        break 'search;
                    }
                }
            }
        }
        let Some((at, block)) = ripe else {
            return false;
        };
        self.edit(&[(at, block.with_number(Prop::Age7, 0))]);
        true
    }

    /// Villagers pick up bread lying beside them and eat it.
    fn villagers_eat(&mut self) {
        let villagers: Vec<(usize, DVec3)> = self
            .mobs
            .iter()
            .enumerate()
            .filter(|(_, m)| m.data.villager.is_some() && !m.is_dead())
            .map(|(i, m)| (i, m.feet()))
            .collect();
        let mut eaten = Vec::new();
        for (e, item) in self.entities.iter().enumerate() {
            if item.stack.id != items::BREAD || item.pickup_delayed() {
                continue;
            }
            if let Some(&(i, _)) = villagers
                .iter()
                .find(|(_, feet)| feet.distance(item.center()) < 1.5)
            {
                eaten.push((e, i, u32::from(item.stack.count)));
            }
        }
        for &(e, i, count) in eaten.iter().rev() {
            if let Some(v) = self.mobs[i].data.villager.as_mut() {
                v.food += 4 * count;
            }
            self.entities.remove(e);
        }
    }

    /// Two willing villagers near each other, with a free bed about, have
    /// a baby.
    fn villagers_breed(&mut self, indices: &[usize]) {
        let willing = |m: &Mob| {
            m.data
                .villager
                .as_deref()
                .is_some_and(|v| v.food >= BREED_FOOD && v.breed_cooldown == 0 && v.baby == 0)
        };
        let ready: Vec<usize> = indices
            .iter()
            .copied()
            .filter(|&i| willing(&self.mobs[i]))
            .collect();
        for (n, &a) in ready.iter().enumerate() {
            for &b in &ready[n + 1..] {
                let (fa, fb) = (self.mobs[a].feet(), self.mobs[b].feet());
                if fa.distance(fb) > 4.0 {
                    continue;
                }
                let cell = fa.floor().as_ivec3();
                let (_, beds, _) = self.points_of_interest(cell);
                let villagers = indices
                    .iter()
                    .filter(|&&i| self.mobs[i].feet().distance(fa) < 32.0)
                    .count();
                if beds.len() <= villagers {
                    continue;
                }
                for i in [a, b] {
                    if let Some(v) = self.mobs[i].data.villager.as_mut() {
                        v.food -= BREED_FOOD;
                        v.breed_cooldown = BREED_COOLDOWN;
                    }
                }
                let kind = self.mobs[a].kind;
                let mut baby = self.new_mob(kind, (fa + fb) / 2.0, 0.0);
                if let Some(v) = baby.data.villager.as_mut() {
                    v.baby = BABY_TICKS;
                }
                self.add_mob(baby);
                return;
            }
        }
    }

    /// A village of five or more villagers within 16 blocks of each other
    /// gets an iron golem, one for every ten.
    fn summon_golems(&mut self, indices: &[usize]) {
        let Some(golem) = self.mob_defs.find("iron_golem") else {
            return;
        };
        for &i in indices {
            let feet = self.mobs[i].feet();
            let villagers = indices
                .iter()
                .filter(|&&j| self.mobs[j].feet().distance(feet) < 16.0)
                .count();
            let golems = self
                .mobs
                .iter()
                .filter(|m| m.kind == golem && !m.is_dead() && m.feet().distance(feet) < 32.0)
                .count();
            if villagers < GOLEM_VILLAGERS || golems * 10 >= villagers.max(10) {
                continue;
            }
            for _ in 0..10 {
                let dx = self.rng.below(17) as i32 - 8;
                let dz = self.rng.below(17) as i32 - 8;
                let (x, z) = (feet.x.floor() as i32 + dx, feet.z.floor() as i32 + dz);
                let Some(spot) = self.ground_spot(x, z, GOLEM_GROUND) else {
                    continue;
                };
                if (spot.y - feet.y).abs() > 6.0 {
                    continue;
                }
                let mob = self.new_mob(golem, spot, 0.0);
                self.add_mob(mob);
                return;
            }
        }
    }

    /// Right-clicked a villager: trading opens with one that has a trade.
    pub(super) fn talk_to_villager(&mut self, index: usize) -> bool {
        let Some(mob) = self.mobs.get(index) else {
            return false;
        };
        let can_trade = mob
            .data
            .villager
            .as_deref()
            .is_some_and(|v| v.baby == 0 && !v.trades.is_empty());
        if !can_trade {
            return false;
        }
        self.container = Some(Container::Merchant(mob.id));
        self.release_input();
        true
    }

    /// The villager the trading screen shows.
    pub fn merchant(&self) -> Option<&VillagerData> {
        let Some(Container::Merchant(id)) = self.container else {
            return None;
        };
        let index = self.villager_index(id)?;
        self.mobs[index].data.villager.as_deref()
    }

    /// How many plain (unenchanted, unnamed) `stack.id` the player carries.
    fn carried(&self, stack: &ItemStack) -> u32 {
        (0..MAIN_SLOTS)
            .filter_map(|i| self.inventory.slot(i))
            .filter(|s| s.id == stack.id && s.extra.is_none())
            .map(|s| u32::from(s.count))
            .sum()
    }

    fn pay(&mut self, stack: &ItemStack) {
        let mut left = stack.count;
        for i in 0..MAIN_SLOTS {
            if left == 0 {
                break;
            }
            let Some(slot) = self.inventory.slot(i) else {
                continue;
            };
            if slot.id != stack.id || slot.extra.is_some() {
                continue;
            }
            let taken = slot.count.min(left);
            self.inventory.take(i, taken);
            left -= taken;
        }
    }

    /// Whether the player can afford trade `index` of the open villager.
    pub fn can_trade(&self, index: usize) -> bool {
        let Some(trade) = self.merchant().and_then(|v| v.trades.get(index)) else {
            return false;
        };
        !trade.out_of_stock()
            && self.carried(&trade.buy) >= u32::from(trade.buy.count)
            && trade
                .buy_b
                .as_ref()
                .is_none_or(|b| self.carried(b) >= u32::from(b.count))
    }

    /// Makes trade `index` with the open villager once: pays, takes the
    /// goods (into the inventory, or dropped), and gives experience to both.
    pub fn trade(&mut self, index: usize) -> bool {
        if !self.can_trade(index) {
            return false;
        }
        let Some(Container::Merchant(id)) = self.container else {
            return false;
        };
        let Some(m) = self.villager_index(id) else {
            return false;
        };
        let Some(trade) = self.mobs[m]
            .data
            .villager
            .as_ref()
            .and_then(|v| v.trades.get(index))
            .cloned()
        else {
            return false;
        };
        self.pay(&trade.buy);
        if let Some(b) = &trade.buy_b {
            self.pay(b);
        }
        let items = self.items.clone();
        if let Some(left) = self.inventory.add(trade.sell.clone(), &items) {
            self.throw(left);
        }
        let mut rng = Rng::new(self.rng.next_u64());
        let levelled = self.mobs[m]
            .data
            .villager
            .as_mut()
            .is_some_and(|v| v.traded(index, &items, &mut rng));
        // The player gets 3 to 6 points, and more when the villager levels.
        let mut points = 3 + self.rng.below(4);
        if levelled {
            points += 5;
        }
        let at = self.mobs[m].feet() + DVec3::Y;
        self.spawn_xp(points, at);
        self.note(crate::advancements::Event::Used("trade".to_owned()));
        true
    }

    /// Puts what the structures in a newly seen chunk hold: villagers,
    /// golems, penned animals and chest minecarts.
    pub(super) fn spawn_structure_mobs(&mut self, pos: crate::world::chunk::ChunkPos) {
        let spawns = if let Some(terrain) = self.chunks.terrain() {
            crate::world::structure::spawns_in(terrain, pos)
        } else if let Some(nether) = self.chunks.nether() {
            crate::world::structure::nether::spawns_in(nether, pos)
        } else if let Some(end) = self.chunks.end() {
            crate::world::structure::end::spawns_in(end, pos)
        } else {
            return;
        };
        for spawn in spawns {
            if spawn.mob == "chest_minecart" {
                let loot = spawn.tag.as_deref().unwrap_or("abandoned_mineshaft");
                let mut rng = Rng::new(self.rng.next_u64());
                let mut cart = crate::entity::minecart::Minecart::new(
                    crate::entity::minecart::CartKind::Chest,
                    spawn.at,
                );
                cart.slots = crate::world::structure::loot::fill(loot, &mut rng);
                self.carts.push(cart);
                continue;
            }
            let Some(kind) = self.mob_defs.find(&spawn.mob) else {
                continue;
            };
            let yaw = self.rng.next_f64() as f32 * std::f32::consts::TAU;
            let mob = self.new_mob(kind, spawn.at, yaw);
            self.add_mob(mob);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        game::tests::{flat_game, mob_registry},
        player::mode::GameMode,
    };

    fn village_game() -> Game {
        let mut game = flat_game(GameMode::Survival);
        game.mob_defs = mob_registry();
        game
    }

    fn villager_at(game: &mut Game, x: f64, z: f64) -> usize {
        let kind = game.mob_defs.find("villager").unwrap();
        let mob = game.new_mob(kind, DVec3::new(x, 64.0, z), 0.0);
        game.add_mob(mob);
        game.mobs.len() - 1
    }

    #[test]
    fn a_villager_takes_a_job_site_and_bed_and_its_profession() {
        let mut game = village_game();
        let i = villager_at(&mut game, 4.5, 4.5);
        game.chunks.set_blocks(&[
            (IVec3::new(6, 64, 4), BlockState::of(blocks::LECTERN)),
            (
                IVec3::new(2, 64, 2),
                BlockState::parse("red_bed[part=foot,facing=south]").unwrap(),
            ),
            (
                IVec3::new(2, 64, 3),
                BlockState::parse("red_bed[part=head,facing=south]").unwrap(),
            ),
        ]);
        game.tick_villagers();
        let v = game.mobs[i].data.villager.as_deref().unwrap();
        assert_eq!(v.profession, Some(Profession::Librarian));
        assert_eq!(v.job_site(), Some(IVec3::new(6, 64, 4)));
        assert_eq!(v.bed(), Some(IVec3::new(2, 64, 3)));
        assert_eq!(v.trades.len(), 2);
        // A second villager finds the site taken.
        let j = villager_at(&mut game, 5.5, 6.5);
        game.tick_villagers();
        assert_eq!(
            game.mobs[j].data.villager.as_deref().unwrap().profession,
            None
        );
        // Breaking the lectern before any trade leaves it jobless.
        game.chunks
            .set_blocks(&[(IVec3::new(6, 64, 4), BlockState::AIR)]);
        game.mobs[i].data.villager.as_mut().unwrap().scan = 0;
        game.tick_villagers();
        assert_eq!(
            game.mobs[i].data.villager.as_deref().unwrap().profession,
            None
        );
    }

    #[test]
    fn the_schedule_sends_villagers_to_work_and_to_bed() {
        let mut game = village_game();
        let i = villager_at(&mut game, 4.5, 4.5);
        game.chunks.set_blocks(&[
            (IVec3::new(8, 64, 4), BlockState::of(blocks::SMOKER)),
            (
                IVec3::new(2, 64, 9),
                BlockState::parse("red_bed[part=head,facing=south]").unwrap(),
            ),
        ]);
        game.time = 3000;
        game.tick_villagers();
        let errand = game.mobs[i].data.villager.as_ref().unwrap().errand;
        assert_eq!(errand, Some(DVec3::new(8.5, 64.0, 4.5)), "work");
        game.time = 15000;
        game.tick_villagers();
        let errand = game.mobs[i].data.villager.as_ref().unwrap().errand;
        assert_eq!(errand, Some(DVec3::new(2.5, 64.0, 9.5)), "bed");
    }

    #[test]
    fn trading_pays_and_gives_and_levels_the_villager() {
        let mut game = village_game();
        let i = villager_at(&mut game, 4.5, 4.5);
        let items = game.items.clone();
        let mut rng = Rng::new(4);
        let v = game.mobs[i].data.villager.as_mut().unwrap();
        v.employ(Profession::Farmer, &items, &mut rng);
        // A known first trade: 20 wheat for an emerald.
        v.trades[0] = crate::entity::villager::Trade {
            buy: ItemStack::new(items::WHEAT, 20),
            buy_b: None,
            sell: ItemStack::new(items::EMERALD, 1),
            uses: 0,
            max_uses: 16,
            xp: 2,
        };
        let id = game.mobs[i].id;
        assert!(game.talk_to_villager(i));
        assert_eq!(game.container, Some(Container::Merchant(id)));
        assert!(!game.trade(0), "no wheat yet");
        game.inventory
            .set(3, Some(ItemStack::new(items::WHEAT, 64)));
        for _ in 0..3 {
            assert!(game.trade(0));
        }
        assert_eq!(game.inventory.slot(3).map(|s| s.count), Some(4));
        assert_eq!(
            game.inventory
                .find(items::EMERALD)
                .map(|s| game.inventory.slot(s).unwrap().count),
            Some(3)
        );
        assert!(!game.orbs.is_empty(), "the player got experience");
        let v = game.merchant().unwrap();
        assert_eq!((v.xp, v.trades[0].uses), (6, 3));
    }

    #[test]
    fn well_fed_villagers_with_a_spare_bed_have_a_baby() {
        let mut game = village_game();
        let a = villager_at(&mut game, 4.5, 4.5);
        let b = villager_at(&mut game, 5.5, 4.5);
        let mut beds = Vec::new();
        for x in 0..3 {
            beds.push((
                IVec3::new(x * 2, 64, 8),
                BlockState::parse("red_bed[part=head,facing=south]").unwrap(),
            ));
        }
        game.chunks.set_blocks(&beds);
        for i in [a, b] {
            game.mobs[i].data.villager.as_mut().unwrap().food = 12;
        }
        let before = game.mobs.len();
        game.tick_villagers();
        assert_eq!(game.mobs.len(), before + 1);
        let baby = game.mobs.last().unwrap().data.villager.as_deref().unwrap();
        assert!(baby.baby > 0);
        // They need feeding again before another.
        game.tick_villagers();
        assert_eq!(game.mobs.len(), before + 1);
    }

    #[test]
    fn five_villagers_get_an_iron_golem() {
        let mut game = village_game();
        for i in 0..5 {
            villager_at(&mut game, 2.5 + i as f64, 2.5);
        }
        let golem = game.mob_defs.find("iron_golem").unwrap();
        let indices: Vec<usize> = (0..5).collect();
        game.summon_golems(&indices);
        assert_eq!(game.mobs.iter().filter(|m| m.kind == golem).count(), 1);
        game.summon_golems(&indices);
        assert_eq!(
            game.mobs.iter().filter(|m| m.kind == golem).count(),
            1,
            "one is enough"
        );
    }
}
