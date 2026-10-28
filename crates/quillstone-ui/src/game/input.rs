//! The `::input` screen's behaviour: what typing, Backspace, Enter, OK and Esc do.
//!
//! How text gets here: while this screen is up, [`Event::Text`] is the only way
//! characters arrive. [`Key::Char`] and [`Key::Space`] are ignored on it, so a
//! frontend that sends both for one key press (the player does) never types twice,
//! and a letter can never be a shortcut (`a`, `h`, `s`, `l`, `i`, `u`, Tab...).
//! [`Key::Backspace`] deletes (it never rolls back) and [`Key::Enter`] confirms.
//! There is no caret movement: text is added at the end and deleted from the end.
//! The typed text is only in the game's memory, not in saves.

use quillstone_runtime::{filter_typed, finish_input};

use super::*;
use crate::input::{self, InputScreen};

/// What the field holds while the screen is up (it starts empty at every arrival).
#[derive(Debug, Clone, Default)]
pub(super) struct InputField {
    pub text: String,
    /// Seconds since the last keystroke (the caret is solid while typing, then blinks).
    pub since_key: f32,
}

impl InputField {
    fn cut(&mut self, max: usize) {
        if self.text.chars().count() > max {
            self.text = self.text.chars().take(max).collect();
        }
    }
}

impl Game<'_> {
    /// Whether the player is typing into the game: the frontend sends [`Event::Text`]
    /// (and lets Backspace repeat) only then, and may switch its text-input method on.
    pub fn wants_text(&self) -> bool {
        self.in_story && self.screen == Screen::Input && self.confirm.is_none()
    }

    /// The prompt, default and character limit of the input the story waits at.
    fn asking(&self) -> Option<(&str, Option<&str>, usize)> {
        match &self.session.state().pending {
            Some(Cmd::AskInput { prompt, default, max }) => Some((prompt, default.as_deref(), *max)),
            _ => None,
        }
    }

    /// The story arrived at (or was redrawn at) a text entry.
    pub(super) fn input_arrived(&mut self, cmds: &[Cmd]) {
        if !self.in_story {
            return;
        }
        let Some(max) = cmds.iter().rev().find_map(|c| match c {
            Cmd::AskInput { max, .. } => Some(*max),
            _ => None,
        }) else {
            return;
        };
        match self.screen {
            // A reload under the open screen: keep what was typed (cut to the new limit).
            Screen::Input => self.input.cut(max),
            Screen::Story => self.open_input(),
            _ if self.under == Some(Screen::Input) => self.input.cut(max),
            // Settings (or another screen) is open: show the entry when it closes.
            _ => {
                self.input = InputField::default();
                self.under = Some(Screen::Input);
            }
        }
    }

    fn open_input(&mut self) {
        self.stop_skip();
        self.screen = Screen::Input;
        self.input = InputField::default();
        self.choice_focus = None;
    }

    /// After the script changed under the screen: back to the story when it no
    /// longer waits at a text entry.
    pub(super) fn input_resync(&mut self) {
        if self.asking().is_some() {
            return;
        }
        if self.screen == Screen::Input {
            self.screen = Screen::Story;
        } else if self.under == Some(Screen::Input) {
            self.under = None;
        }
    }

    /// Whether confirming now would work: something typed, or a default to use.
    fn can_confirm_input(&self) -> bool {
        self.asking().is_some_and(|(_, default, max)| finish_input(&self.input.text, default, max).is_some())
    }

    pub(super) fn tick_input(&mut self, dt: f32) {
        self.input.since_key = (self.input.since_key + dt) % input::BLINK_PERIOD;
    }

    pub(super) fn draw_input_screen(&self, out: &mut Vec<Draw>) {
        let Some((prompt, default, max)) = self.asking() else { return };
        let hover_ok = self.pointer().is_some_and(|(x, y)| input::ok_at(x, y)) && self.can_confirm_input();
        let screen = InputScreen {
            prompt,
            text: &self.input.text,
            placeholder: default,
            max,
            caret: input::caret_visible(self.input.since_key),
            can_confirm: self.can_confirm_input(),
            hover_ok,
        };
        input::draw_input(out, self.view.theme(), &screen);
    }

    pub(super) fn input_event(&mut self, event: Event) {
        match event {
            Event::Text(s) => self.type_text(&s),
            Event::Key(Key::Backspace) => {
                if self.input.text.pop().is_some() {
                    self.input.since_key = 0.0;
                }
            }
            Event::Key(Key::Enter) => self.confirm_input(),
            Event::Press(Button::Left) if self.ok_under_cursor() => self.confirm_input(),
            Event::Press(Button::Right) | Event::Key(Key::Escape) => self.open_settings(),
            Event::Key(Key::F5) => self.quick_save(),
            Event::Key(Key::F9) => self.quick_load(),
            // Letters, digits, Space, Tab, the wheel and the rest do nothing here.
            _ => {}
        }
    }

    fn ok_under_cursor(&self) -> bool {
        self.cursor.is_some_and(|(x, y)| input::ok_at(x, y))
    }

    /// Add typed characters (control characters dropped, the limit kept).
    fn type_text(&mut self, s: &str) {
        let Some((_, _, max)) = self.asking() else { return };
        let room = max.saturating_sub(self.input.text.chars().count());
        let add: String = filter_typed(s).chars().take(room).collect();
        if !add.is_empty() {
            self.input.text.push_str(&add);
            self.input.since_key = 0.0;
        }
    }

    fn confirm_input(&mut self) {
        if !self.can_confirm_input() {
            return;
        }
        let answer = std::mem::take(&mut self.input.text);
        self.screen = Screen::Story;
        self.forward(Input::Text(answer));
    }
}
