//! The end poem and credits (M27), shown after the exit portal is first
//! entered. The poem is Rustcraft's own: modern Minecraft's is under
//! copyright, so nothing of it is reproduced here.

/// One line of the roll.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Line {
    /// A line of the poem: soft grey-green.
    Poem(&'static str),
    /// A heading in the credits.
    Heading(&'static str),
    /// A name or a plain credit.
    Credit(&'static str),
    /// A blank line.
    Gap,
}

use Line::{Credit, Gap, Heading, Poem};

pub const LINES: &[Line] = &[
    Poem("You came a long way to stand here."),
    Poem("Through grass, and through dark, and through fire."),
    Poem("The world kept count of your steps"),
    Poem("and wrote them down in stone."),
    Gap,
    Poem("The sky above the island has no sun."),
    Poem("It never needed one."),
    Poem("It only needed someone to arrive,"),
    Poem("and look up, and not be afraid."),
    Gap,
    Poem("Something old and large has gone quiet."),
    Poem("It was never your enemy, only the weather"),
    Poem("of a place that had not met anyone yet."),
    Gap,
    Poem("Now the lights come on, one by one,"),
    Poem("along the long way home."),
    Poem("Take what you carry. Keep what you built."),
    Poem("The morning is waiting where you left it."),
    Gap,
    Gap,
    Heading("Rustcraft"),
    Gap,
    Credit("A little Minecraft, written in Rust"),
    Gap,
    Heading("Made by"),
    Credit("Kenzo"),
    Gap,
    Heading("Code"),
    Credit("Claude Code"),
    Gap,
    Heading("Textures and sounds"),
    Credit("Kenzo's own picks, and the resource packs you chose"),
    Gap,
    Heading("Built with"),
    Credit("Rust"),
    Credit("wgpu and winit"),
    Credit("glam and noise"),
    Credit("egui and kira"),
    Gap,
    Heading("Thanks to"),
    Credit("Everyone who ever dug straight down"),
    Credit("and everyone who didn't"),
    Gap,
    Credit("Minecraft belongs to Mojang and Microsoft."),
    Credit("This is an unofficial fan project."),
    Gap,
    Gap,
    Poem("Thank you for playing."),
];

/// Ticks a line takes to scroll past one line's height.
pub const LINE_TICKS: u32 = 24;
/// How many lines tall the screen is taken to be, for how long the roll
/// runs after the last line appears.
pub const SCREEN_LINES: u32 = 18;
/// Ticks the whole roll takes, from the first line appearing at the
/// bottom to the last leaving the top.
pub const TICKS: u32 = (LINES.len() as u32 + SCREEN_LINES) * LINE_TICKS;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_roll_lasts_about_a_minute_and_a_bit_and_has_a_poem_and_credits() {
        let seconds = TICKS / crate::config::TICKS_PER_SECOND;
        assert!((45..=120).contains(&seconds), "{seconds} s");
        assert!(matches!(LINES[0], Poem(_)));
        assert!(LINES.iter().any(|l| matches!(l, Heading("Rustcraft"))));
        assert!(LINES.iter().filter(|l| matches!(l, Poem(_))).count() >= 15);
    }
}
