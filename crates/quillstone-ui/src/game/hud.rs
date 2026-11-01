//! Clickable HUD elements (`::hud{call=#label}` / `open=screen`): which one is under the mouse, and what a
//! press does. Mouse only: the keyboard and gamepad have their own keys for the screens, and a `call=`
//! button has no focus of its own.

use quillstone_runtime::{can_press, waits_at_line};
use quillstone_script::{HudAction, HudScreen};

use super::*;

impl Game<'_> {
    /// HUD buttons work now: the story waits at a normal line, no replay or skip is running.
    /// Otherwise they are drawn dimmed and a press only says so.
    pub(super) fn hud_ready(&self) -> bool {
        self.screen == Screen::Story && self.replay.is_none() && self.skip.is_none() && waits_at_line(self.session.state())
    }

    /// The HUD button under the cursor: not where a choice or hotspot is (they come first), and
    /// positioned by the same rect it is drawn at. Gives its id and what it does.
    pub(super) fn hud_button_under_cursor(&self) -> Option<(String, HudAction)> {
        let (x, y) = self.cursor.filter(|_| self.screen == Screen::Story)?;
        if self.view.choice_at(x, y).is_some() || self.view.hotspot_at(x, y).is_some() {
            return None;
        }
        let sizes = self.quick_sizes.borrow();
        let elem = self.view.hud_button_at(&|s| sizes.get(s).copied(), x, y)?;
        Some((elem.id.clone(), elem.action.clone()?))
    }

    /// A click on a HUD button.
    pub(super) fn hud_press(&mut self, id: &str, action: HudAction) {
        let allowed = self.hud_ready() && can_press(self.program, self.session.state(), id);
        if !allowed {
            return self.notify("Can't use that now");
        }
        match action {
            HudAction::Call(_) => self.forward(Input::Hud(id.to_string())),
            HudAction::Open(HudScreen::Inventory) => self.open_inventory_overlay(),
            HudAction::Open(HudScreen::Journal) => self.open_journal_overlay(),
            HudAction::Open(HudScreen::History) => self.open_backlog(),
            HudAction::Open(HudScreen::Save) => self.open_slots(Screen::Save),
            HudAction::Open(HudScreen::Load) => self.open_slots(Screen::Load),
            HudAction::Open(HudScreen::Settings) => self.open_settings(),
        }
    }
}
