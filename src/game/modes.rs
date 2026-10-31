//! What spectator and adventure mode change (M43): spectators touch
//! nothing but may take the eyes of a mob or player, and adventurers break
//! and place only what their item's `can_break` / `can_place_on` allow.

use glam::{DVec3, IVec3};

use super::Game;

/// Whom a spectator is looking through.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Spectated {
    Mob(u64),
    /// A player, by name.
    Player(String),
}

/// A block id in a rule list without its namespace.
fn bare(name: &str) -> &str {
    name.strip_prefix("minecraft:").unwrap_or(name)
}

/// Whether `rules` (block ids) name `block`; tags (`#...`) never match.
pub fn rules_allow(rules: &[String], block: &str) -> bool {
    rules.iter().any(|rule| bare(rule) == bare(block))
}

impl Game {
    /// Puts the current player in `mode` (`with_player` picks who).
    pub fn set_game_mode(&mut self, mode: crate::player::mode::GameMode) {
        let p = &mut self.player;
        if p.mode != mode {
            p.previous_mode = p.mode;
        }
        p.mode = mode;
        p.spectating = None;
        p.spectator_menu = false;
        p.body.may_fly = mode.can_fly();
        p.body.no_clip = mode.is_spectator();
        p.body.flying = mode.is_spectator() || (mode.can_fly() && p.body.flying);
        p.mining = None;
        p.breaking = false;
        p.placing = false;
    }

    /// A spectator's pick in the menu: go to player `name`, if they're in
    /// the same dimension. Returns whether it happened.
    pub fn spectator_teleport(&mut self, index: usize, name: &str) -> bool {
        if !self.player_at(index).mode.is_spectator() {
            return false;
        }
        let dimension = self.player_at(index).dimension;
        let Some(to) = (0..self.player_count()).find(|&i| {
            i != index && self.player_at(i).name == name && self.player_at(i).dimension == dimension
        }) else {
            return false;
        };
        let feet = self.feet_of(to);
        let (yaw, pitch) = (
            self.player_at(index).camera.yaw,
            self.player_at(index).camera.pitch,
        );
        self.with_player(index, |g| {
            g.player.spectating = None;
            g.teleport(feet, yaw, pitch);
        });
        true
    }

    /// A spectator's pick of a team: go to the first of its members in the
    /// same dimension.
    pub fn spectator_teleport_team(&mut self, index: usize, team: &str) -> bool {
        let members: Vec<String> = self
            .board
            .teams
            .get(team)
            .map(|t| t.members.iter().cloned().collect())
            .unwrap_or_default();
        members.iter().any(|m| self.spectator_teleport(index, m))
    }

    /// F3+N: between spectator and the mode before it (creative if none).
    pub fn toggle_spectator(&mut self) {
        let mode = if self.player.mode.is_spectator() {
            self.player.previous_mode
        } else {
            crate::player::mode::GameMode::Spectator
        };
        let mode = if mode.is_spectator() {
            crate::player::mode::GameMode::Creative
        } else {
            mode
        };
        self.set_game_mode(mode);
    }

    /// A spectator's tick of hands: clicks do nothing, except a left click
    /// on a mob or player, which spectates it; Shift lets go.
    pub(super) fn spectator_hands(&mut self) {
        let eye = self.player.body.position() + DVec3::Y * self.player.body.eye_height();
        let clicked = self.player.break_clicked;
        self.player.break_clicked = false;
        self.player.place_clicked = false;
        self.player.mining = None;
        if self.player.spectating.is_none() && clicked {
            if let Some(mob) = self.target_mob(eye, None) {
                self.player.spectating = Some(Spectated::Mob(self.mobs[mob].id));
            } else if let Some((other, _)) = self.target_player(eye, None) {
                self.player.spectating = Some(Spectated::Player(self.others[other].name.clone()));
            }
        }
        if self.player.keys.sneak {
            self.player.spectating = None;
        }
        self.follow_spectated();
    }

    /// Puts a spectating player's eyes where the mob's or player's are, and
    /// looks where they look. A target that is gone or dead is let go.
    fn follow_spectated(&mut self) {
        let Some(who) = self.player.spectating.clone() else {
            return;
        };
        let seen = match who {
            Spectated::Mob(id) => self
                .mobs
                .iter()
                .find(|m| m.id == id && !m.is_dead())
                .map(|m| (m.feet(), m.body.eye_height(), m.head_yaw, m.pitch)),
            Spectated::Player(name) => std::iter::once(&self.player)
                .chain(&self.others)
                .find(|p| p.name == name && p.dimension == self.player.dimension)
                .filter(|p| !p.stats.is_dead())
                .map(|p| {
                    (
                        p.body.position(),
                        p.body.eye_height(),
                        p.camera.yaw,
                        p.camera.pitch,
                    )
                }),
        };
        let Some((feet, eye, yaw, pitch)) = seen else {
            self.player.spectating = None;
            return;
        };
        // The spectator's own eyes sit at their body's eye height.
        let own = self.player.body.eye_height();
        self.player.body.set_position(feet + DVec3::Y * (eye - own));
        self.player.body.velocity = DVec3::ZERO;
        self.player.prev_feet = self.player.body.position();
        self.player.camera.yaw = yaw;
        self.player.camera.pitch = pitch;
        self.player.turned = self.player.turned.wrapping_add(1);
    }

    /// Whether an adventurer's held item lets them break `cell`.
    pub(super) fn adventure_may_break(&self, cell: IVec3) -> bool {
        if !self.player.mode.needs_permission_to_edit() {
            return true;
        }
        let Some(block) = self.chunks.world().block(cell.x, cell.y, cell.z) else {
            return false;
        };
        self.held_rules(true, &block.def().name)
    }

    /// Whether an adventurer's held item lets them place against `cell`.
    pub(super) fn adventure_may_place(&self, cell: IVec3) -> bool {
        if !self.player.mode.needs_permission_to_edit() {
            return true;
        }
        let Some(block) = self.chunks.world().block(cell.x, cell.y, cell.z) else {
            return false;
        };
        self.held_rules(false, &block.def().name)
    }

    fn held_rules(&self, breaking: bool, block: &str) -> bool {
        self.player.inventory.selected_stack().is_some_and(|stack| {
            let rules = if breaking {
                stack.can_break()
            } else {
                stack.can_place_on()
            };
            rules_allow(rules, block)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rules_ignore_the_namespace() {
        let rules = vec!["minecraft:stone".to_owned(), "dirt".to_owned()];
        assert!(rules_allow(&rules, "stone"));
        assert!(rules_allow(&rules, "minecraft:dirt"));
        assert!(!rules_allow(&rules, "sand"));
        assert!(!rules_allow(&[], "stone"));
    }
}
