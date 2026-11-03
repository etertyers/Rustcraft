//! The message above the hotbar (vanilla's overlay message, the action
//! bar): every notice the game or a mod shows there stays at full
//! opacity for 3 seconds, then fades out over 1, and a new one replaces
//! the old.

/// Ticks a message stays at full opacity.
pub const SHOW_TICKS: u32 = 60;
/// Ticks it then takes to fade out.
pub const FADE_TICKS: u32 = 20;
/// A message's whole life, which is what its countdown starts from.
pub const TOTAL_TICKS: u32 = SHOW_TICKS + FADE_TICKS;

/// How opaque a message with `left` ticks to live is, 0–1.
pub fn alpha(left: f32) -> f32 {
    (left / FADE_TICKS as f32).clamp(0.0, 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn full_for_sixty_ticks_then_gone_in_twenty_more() {
        assert_eq!(alpha(TOTAL_TICKS as f32), 1.0);
        assert_eq!(alpha(FADE_TICKS as f32), 1.0, "tick 60: about to fade");
        assert!((alpha(10.0) - 0.5).abs() < 1e-6);
        assert_eq!(alpha(0.0), 0.0);
    }

    #[test]
    fn a_notice_is_gone_after_eighty_ticks_and_a_new_one_replaces_the_old() {
        use crate::{game::tests::flat_game, player::mode::GameMode};
        let mut game = flat_game(GameMode::Survival);
        game.player.show("Respawn point set");
        game.run_ticks(30);
        game.player.show("Saved screenshot");
        game.run_ticks(79);
        let (text, left) = game.player.message.clone().expect("still fading");
        assert_eq!(text, "Saved screenshot");
        assert_eq!(left, 1);
        game.run_ticks(1);
        assert!(game.player.message.is_none());
    }
}
