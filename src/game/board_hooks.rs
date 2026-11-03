//! What the game feeds the scoreboard (M44): each tick's read-only criteria
//! (`health`, `food`, ...), counted events (mined, used, killed, deaths) and
//! each player's view of the board for the snapshot.

use super::{
    Game,
    board::{BarView, BoardView, Criteria},
};
use crate::{advancements::Event, player::stats::DamageSource};

impl Game {
    /// The name a score is kept under for `self.player`.
    fn board_holder(&self) -> String {
        self.player.name.clone()
    }

    /// Counts the advancement events that stat criteria watch for.
    pub(super) fn count_for_board(&mut self, event: &Event) {
        if self.board.objectives.is_empty() {
            return;
        }
        let holder = self.board_holder();
        match event {
            Event::Mined(id) => self.board.count_stat(&holder, "mined", id, 1),
            Event::Placed(id) | Event::Ate(id) => self.board.count_stat(&holder, "used", id, 1),
            Event::Killed { mob, .. } => {
                self.board.count_stat(&holder, "killed", mob, 1);
                self.board.count(&holder, &Criteria::TotalKillCount, 1);
            }
            _ => {}
        }
    }

    /// `self.player` died: counts the death, and a player's kill for whoever did it.
    pub(super) fn count_deaths(&mut self, source: DamageSource, attacker: Option<String>) {
        if self.board.objectives.is_empty() {
            return;
        }
        let holder = self.board_holder();
        self.board.count(&holder, &Criteria::DeathCount, 1);
        if source == DamageSource::Player
            && let Some(killer) = attacker
        {
            self.board.count(&killer, &Criteria::PlayerKillCount, 1);
            self.board.count(&killer, &Criteria::TotalKillCount, 1);
        }
    }

    /// Writes `self.player`'s health, food, air, armor and experience into
    /// the objectives that read them.
    pub(super) fn track_for_board(&mut self) {
        if self.board.objectives.is_empty() {
            return;
        }
        let holder = self.board_holder();
        let stats = &self.player.stats;
        let (health, food, air) = (stats.health.ceil() as i32, stats.food, stats.air);
        let (level, xp) = (stats.experience.level as i32, stats.experience.total as i32);
        let armor = self.armor_totals().0 as i32;
        let board = &mut self.board;
        board.track(&holder, &Criteria::Health, health);
        board.track(&holder, &Criteria::Food, food);
        board.track(&holder, &Criteria::Air, air);
        board.track(&holder, &Criteria::Armor, armor);
        board.track(&holder, &Criteria::Level, level);
        board.track(&holder, &Criteria::Xp, xp);
    }

    /// The scoreboard and bossbars as `self.player` sees them.
    pub fn board_view_for_player(&self) -> BoardView {
        let online: Vec<String> = std::iter::once(&self.player)
            .chain(&self.others)
            .map(|p| p.name.clone())
            .collect();
        let mut view = self.board.view_for(&self.player.name, &online);
        view.bars = self
            .bossbars
            .values()
            .filter(|b| b.visible && b.players.contains(&self.player.name))
            .map(|b| BarView {
                name: b.name.clone(),
                color: b.color.clone(),
                style: b.style.clone(),
                value: b.value,
                max: b.max,
            })
            .collect();
        view
    }
}
