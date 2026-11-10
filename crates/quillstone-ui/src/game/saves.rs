//! Saving and loading for [`Game`]: the slot screens, quick save/load, auto
//! saves, and the confirm dialog that guards overwriting, loading and quitting.

use super::*;
use crate::slots::{
    newest_slot, next_rotating_slot, pages, read_slot, Page, SaveFile, Slot, SLOTS_PER_PAGE,
};

/// What a confirm dialog does when the player says yes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Then {
    Quit,
    MainMenu,
    Save(Slot),
    Load(Slot),
}

#[derive(Debug, Clone, PartialEq)]
pub(super) struct Confirm {
    pub message: String,
    pub then: Then,
}

/// How long a notice ("Quick saved") stays up, in seconds.
pub(super) const NOTICE_SECONDS: f32 = 2.0;

impl Game<'_> {
    /// Save to the next auto slot, if anything has happened since the start.
    /// Called at every choice and when quitting; the frontend should call it
    /// when the window is closed, too.
    pub fn autosave(&mut self) {
        if self.replay.is_some() {
            return self.save_persistent(); // a replay is never saved (this only retries an earlier failed write)
        }
        self.save_read();
        self.save_persistent(); // a write that failed earlier gets its last chance
        if self.in_story && self.session.can_rollback() && !self.session.state().finished {
            let slot = next_rotating_slot(&*self.storage, Page::Auto);
            self.save_to(slot);
        }
    }

    /// Show a short message over the game (like "Saved").
    pub fn notify(&mut self, message: impl Into<String>) {
        self.notice = Some((message.into(), NOTICE_SECONDS));
    }

    /// `ask` with one of the engine's own questions, in the language in use.
    pub(super) fn ask_ui(&mut self, message: &str, then: Then) {
        let message = self.ui().t(message).to_string();
        self.ask(&message, then);
    }

    pub(super) fn ask(&mut self, message: &str, then: Then) {
        self.stop_skip();
        self.confirm = Some(Confirm { message: message.to_string(), then });
        self.dragging = None;
    }

    pub(super) fn confirm_event(&mut self, event: Event) {
        let yes = match event {
            Event::Press(Button::Left) => match self.cursor.and_then(|(x, y)| confirm_at(x, y)) {
                Some(answer) => answer,
                None => return,
            },
            Event::Press(Button::Right) | Event::Key(Key::Escape | Key::Char('n' | 'N')) => false,
            Event::Key(Key::Enter | Key::Char('y' | 'Y')) => true,
            _ => return,
        };
        let Some(confirm) = self.confirm.take() else { return };
        if !yes {
            return;
        }
        if self.screen == Screen::Settings && matches!(confirm.then, Then::Quit | Then::MainMenu) {
            self.close_settings(); // asked from Settings, which stayed open: close it (and save its changes) first
        }
        match confirm.then {
            Then::Quit => {
                self.autosave();
                self.requests.push(Request::Quit);
            }
            Then::MainMenu if self.replay.is_some() => self.end_replay(),
            Then::MainMenu => {
                self.autosave();
                self.go_to_menu();
            }
            Then::Save(slot) => {
                self.save_to(slot);
            }
            Then::Load(slot) => self.load_from(slot),
        }
    }

    // ---- the slot screens ----------------------------------------------------------

    pub(super) fn open_slots(&mut self, screen: Screen) {
        if self.replay.is_some() {
            return self.notify_ui(super::replay::NOT_IN_REPLAY, &[]);
        }
        self.stop_skip();
        self.cover_bag();
        self.screen = screen;
        self.slot_focus = None;
        if screen == Screen::Save && !matches!(self.page, Page::Numbered(_)) {
            self.page = Page::Numbered(1); // auto and quick pages are written by the game
        }
        self.refresh_slots();
    }

    /// The page tabs the current slot screen offers.
    pub(super) fn slot_tabs(&self) -> Vec<Page> {
        let all = pages();
        match self.screen {
            Screen::Save => all.into_iter().filter(|p| matches!(p, Page::Numbered(_))).collect(),
            _ => all,
        }
    }

    pub(super) fn refresh_slots(&mut self) {
        self.slots = (0..SLOTS_PER_PAGE)
            .map(|index| read_slot(&*self.storage, self.program, Slot { page: self.page, index }))
            .collect();
    }

    pub(super) fn slots_event(&mut self, event: Event) {
        match event {
            Event::Press(Button::Left) => {
                let tabs = self.slot_tabs();
                match self.cursor.and_then(|(x, y)| slots_at(x, y, &tabs, self.slots.len())) {
                    Some(SlotHit::Tab(page)) => {
                        self.page = page;
                        self.refresh_slots();
                    }
                    Some(SlotHit::Slot(i)) => self.pick_slot(i),
                    Some(SlotHit::Back) => self.screen = self.home(),
                    None => {}
                }
            }
            Event::Press(Button::Right) | Event::Key(Key::Escape) => self.screen = self.home(),
            Event::Key(Key::Enter | Key::Space) => {
                if let Some(f) = self.slot_focus {
                    self.pick_slot(f);
                }
            }
            Event::Key(key @ (Key::Up | Key::Down | Key::Left | Key::Right)) => self.move_slot_focus(key),
            _ => {}
        }
    }

    /// Up/Down walk into and around the slot grid; Left/Right move along a row and
    /// turn the page at its ends (or straight away while no slot is picked out).
    fn move_slot_focus(&mut self, key: Key) {
        let (count, cols) = (self.slots.len(), SLOT_COLUMNS);
        match (self.slot_focus, key) {
            (None, Key::Left) => self.turn_page(-1),
            (None, Key::Right) => self.turn_page(1),
            (None, _) if count > 0 => self.slot_focus = Some(0),
            (Some(f), Key::Up) if f >= cols => self.slot_focus = Some(f - cols),
            (Some(f), Key::Down) if f + cols < count => self.slot_focus = Some(f + cols),
            (Some(f), Key::Left) if f % cols > 0 => self.slot_focus = Some(f - 1),
            (Some(f), Key::Right) if f % cols < cols - 1 && f + 1 < count => self.slot_focus = Some(f + 1),
            (Some(f), Key::Left | Key::Right) => {
                let before = self.page;
                self.turn_page(if key == Key::Left { -1 } else { 1 });
                if self.page != before {
                    // Same row, at the other end, on the new page.
                    let at = if key == Key::Left { f + cols - 1 } else { f + 1 - cols };
                    self.slot_focus = Some(at.min(self.slots.len().saturating_sub(1)));
                }
            }
            _ => {}
        }
    }

    fn turn_page(&mut self, by: isize) {
        let tabs = self.slot_tabs();
        let at = tabs.iter().position(|p| *p == self.page).unwrap_or(0) as isize;
        let next = (at + by).clamp(0, tabs.len() as isize - 1) as usize;
        self.page = tabs[next];
        self.refresh_slots();
    }

    fn pick_slot(&mut self, index: usize) {
        let slot = Slot { page: self.page, index };
        let filled = self.slots.get(index).and_then(Option::as_ref);
        match self.screen {
            Screen::Save if filled.is_some() => self.ask_ui("Overwrite this save?", Then::Save(slot)),
            Screen::Save => {
                self.save_to(slot);
            }
            Screen::Load if filled.is_some_and(|info| info.scene.is_some()) => {
                self.ask_ui("Load this save?\nUnsaved progress will be lost.", Then::Load(slot));
            }
            _ => {}
        }
    }

    // ---- writing and reading ------------------------------------------------------------

    /// Write the game to `slot`. `false` (with a notice) if it couldn't be written.
    pub(super) fn save_to(&mut self, slot: Slot) -> bool {
        let mut file = SaveFile::new(&self.session, self.storage.now());
        // A clock set back must not make this save look older than the last one.
        file.seq = file.seq.max(self.newest_save.map_or(0, |(seq, _)| seq.saturating_add(1)));
        match self.storage.write(&slot.file_name(), &file.to_text()) {
            Ok(()) => {
                self.newest_save = Some((file.seq, slot));
                if matches!(slot.page, Page::Quick) {
                    self.quick_saved.set(Some(true));
                }
                if matches!(self.screen, Screen::Save | Screen::Load) {
                    self.refresh_slots();
                }
                true
            }
            Err(e) => {
                self.notify_ui("Could not save: {error}", &[("error", &e)]);
                false
            }
        }
    }

    pub(super) fn load_from(&mut self, slot: Slot) {
        let file = self.storage.read(&slot.file_name()).and_then(|text| SaveFile::parse(&text));
        let loaded = match file {
            Some(file) => Session::load_with(self.program, &file.session, self.session.persistent().clone()),
            None => Err("the file is missing or damaged".into()),
        };
        match loaded {
            Ok((session, cmds)) => {
                self.session = session;
                // A fresh view: nothing of the old scene (line, fades) may linger.
                self.fresh_view();
                self.in_story = true;
                self.under = None;
                self.screen = Screen::Story;
                self.show(&cmds); // may open the screen the save was waiting at
                self.auto = false;
                self.notify_ui("Loaded", &[]);
            }
            Err(e) => self.notify_ui("Could not load: {error}", &[("error", &e)]),
        }
    }

    pub(super) fn quick_save(&mut self) {
        if self.replay.is_some() {
            return self.notify_ui(super::replay::NOT_IN_REPLAY, &[]);
        }
        let slot = next_rotating_slot(&*self.storage, Page::Quick);
        if self.save_to(slot) {
            self.notify_ui("Quick saved", &[]);
        }
    }

    pub(super) fn quick_load(&mut self) {
        if self.replay.is_some() {
            return self.notify_ui(super::replay::NOT_IN_REPLAY, &[]);
        }
        match newest_slot(&*self.storage, Page::Quick) {
            Some(slot) => self.ask_ui("Load the quick save?\nUnsaved progress will be lost.", Then::Load(slot)),
            None => self.notify_ui("No quick save yet", &[]),
        }
    }
}
