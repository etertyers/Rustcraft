//! The screens open over the game, and so whether the mouse belongs to the
//! camera. Opening or closing a screen only changes this stack; the
//! cursor is grabbed exactly when no screen is open (and the player has
//! clicked into the window), and mouse movement, clicks and scrolling
//! reach the player only while it is.

/// Something drawn over the game that needs the mouse.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Layer {
    /// The inventory, a chest, a crafting table, a villager's trades and
    /// the rest of the screens with slots.
    Container,
    Advancements,
    Pause,
    /// Lying in bed, with its Leave Bed button.
    Bed,
    /// The death screen and its Respawn button.
    Death,
}

#[derive(Debug, Default)]
pub struct ScreenStack {
    layers: Vec<Layer>,
    /// In a world rather than the main menu.
    in_world: bool,
    /// The player has clicked into the window (and not left it since), so
    /// with no screen open the mouse looks around.
    engaged: bool,
    /// What the window was last told.
    grabbed: bool,
}

impl ScreenStack {
    /// Puts `layer` on top (once).
    pub fn push(&mut self, layer: Layer) {
        self.remove(layer);
        self.layers.push(layer);
    }

    pub fn remove(&mut self, layer: Layer) {
        self.layers.retain(|&open| open != layer);
    }

    /// Opens or closes `layer` to match `open`.
    pub fn set(&mut self, layer: Layer, open: bool) {
        if open {
            if !self.contains(layer) {
                self.layers.push(layer);
            }
        } else {
            self.remove(layer);
        }
    }

    pub fn contains(&self, layer: Layer) -> bool {
        self.layers.contains(&layer)
    }

    /// The screen on top: the one Escape closes.
    pub fn top(&self) -> Option<Layer> {
        self.layers.last().copied()
    }

    pub fn is_empty(&self) -> bool {
        self.layers.is_empty()
    }

    /// Entering a world (`engaged`: grab the mouse at once) or leaving it
    /// for the title screen, which closes everything.
    pub fn set_in_world(&mut self, in_world: bool, engaged: bool) {
        self.layers.clear();
        self.in_world = in_world;
        self.engaged = in_world && engaged;
    }

    /// The window lost focus (`false`) or the player clicked back into it.
    pub fn set_engaged(&mut self, engaged: bool) {
        self.engaged = engaged;
    }

    /// A click on the window with no screen open takes the mouse back.
    /// Returns whether it did, so the click isn't also a mining swing.
    pub fn click_in(&mut self) -> bool {
        if self.in_world && self.layers.is_empty() && !self.engaged {
            self.engaged = true;
            return true;
        }
        false
    }

    /// Whether the cursor should be hidden and held for mouse look.
    pub fn wants_grab(&self) -> bool {
        self.in_world && self.engaged && self.layers.is_empty()
    }

    /// Whether the camera has the mouse; only then do movement, clicks and
    /// scrolling reach the player.
    pub fn grabbed(&self) -> bool {
        self.grabbed
    }

    /// Whether mouse movement, clicks and scrolling go to the player: the
    /// camera has the mouse and no screen has been opened since.
    pub fn reaches_game(&self) -> bool {
        self.grabbed && self.wants_grab()
    }

    /// The change the window needs, if any: `Some(true)` to grab,
    /// `Some(false)` to release.
    pub fn pending(&self) -> Option<bool> {
        (self.wants_grab() != self.grabbed).then(|| self.wants_grab())
    }

    /// Records what the window now is.
    pub fn confirm(&mut self, grabbed: bool) {
        self.grabbed = grabbed;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn settled(stack: &mut ScreenStack) -> bool {
        if let Some(grab) = stack.pending() {
            stack.confirm(grab);
        }
        stack.grabbed()
    }

    #[test]
    fn the_mouse_is_free_while_any_screen_is_open_and_back_after_the_last_closes() {
        let mut stack = ScreenStack::default();
        assert!(!settled(&mut stack), "the title screen has no camera");
        stack.set_in_world(true, true);
        assert!(settled(&mut stack));

        stack.push(Layer::Container);
        stack.push(Layer::Advancements);
        assert!(!settled(&mut stack));
        stack.remove(Layer::Advancements);
        assert!(!settled(&mut stack), "the inventory is still open");
        assert_eq!(stack.top(), Some(Layer::Container));
        stack.remove(Layer::Container);
        assert!(settled(&mut stack));
    }

    #[test]
    fn clicking_with_a_screen_open_does_not_take_the_mouse_back() {
        let mut stack = ScreenStack::default();
        stack.set_in_world(true, false);
        assert!(
            !settled(&mut stack),
            "a world started from the command line waits for a click"
        );
        assert!(stack.click_in());
        assert!(settled(&mut stack));

        stack.set_engaged(false);
        stack.push(Layer::Advancements);
        assert!(!stack.click_in());
        assert!(!settled(&mut stack));
        stack.remove(Layer::Advancements);
        assert!(!settled(&mut stack), "still outside the window");
        assert!(stack.click_in());
        assert!(settled(&mut stack));
    }
}
