//! Clickable HUD elements (`::hud{call=#label}` / `open=screen`): which one is under the mouse or has the
//! keyboard focus, and what a press does. At a plain waiting line Up/Down (the D-pad) pick a button out and
//! Enter/Space (A) presses it; any other input, or the story moving, drops the focus.

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

    /// Ids of the buttons the keyboard can pick now, in the order they were declared: pressable and shown (an
    /// `if=` that is false hides it), at a line with no choices or hotspots (those own Up/Down).
    pub(super) fn hud_buttons(&self) -> Vec<String> {
        let state = self.session.state();
        if !self.hud_ready() || !self.view.choices.is_empty() || !self.view.hotspots.is_empty() {
            return Vec::new();
        }
        let pressable = |h: &&quillstone_script::HudElem| h.action.is_some() && can_press(self.program, state, &h.id);
        state.hud.iter().filter(pressable).map(|h| h.id.clone()).collect()
    }

    /// The focused button, if it can still be pressed.
    pub(super) fn hud_focused(&self) -> Option<&str> {
        let id = self.hud_focus.as_deref()?;
        self.hud_buttons().iter().any(|b| b == id).then_some(id)
    }

    /// Up/Down: the first (last) button, then the next (previous), wrapping round as the hotspots do.
    pub(super) fn hud_move(&mut self, backwards: bool) {
        let buttons = self.hud_buttons();
        let at = self.hud_focused().and_then(|f| buttons.iter().position(|b| b == f));
        let n = buttons.len();
        let next = match at {
            _ if n == 0 => return,
            None if backwards => n - 1,
            None => 0,
            Some(i) if backwards => (i + n - 1) % n,
            Some(i) => (i + 1) % n,
        };
        self.hud_focus = Some(buttons[next].clone());
    }

    /// Enter/Space with a button focused presses it as a click does; false when nothing is focused.
    pub(super) fn hud_press_focused(&mut self) -> bool {
        let Some(id) = self.hud_focused().map(str::to_string) else { return false };
        let action = self.session.state().hud.iter().find(|h| h.id == id).and_then(|h| h.action.clone());
        action.is_some_and(|a| {
            self.hud_press(&id, a);
            true
        })
    }

    /// A click on a HUD button.
    pub(super) fn hud_press(&mut self, id: &str, action: HudAction) {
        let allowed = self.hud_ready() && can_press(self.program, self.session.state(), id);
        if !allowed {
            return self.notify_ui("Can't use that now", &[]);
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
