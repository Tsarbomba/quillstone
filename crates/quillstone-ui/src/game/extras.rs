//! The extras screen's input (the main menu's Extras): switching between the
//! gallery and the music room, turning pages, opening a picture, playing a track.
//! Also the persistent data file that remembers what is unlocked.

use quillstone_runtime::{read_persistent, write_persistent};
use quillstone_script::Vars;

use super::*;
use crate::extras::{self as ex, Hit, Tab};

/// What is kept across playthroughs (persistent variables, unlocked extras).
pub(super) const PERSISTENT_FILE: &str = "persistent.txt";

pub(super) fn load_persistent(storage: &dyn Storage) -> Vars {
    storage.read(PERSISTENT_FILE).map(|text| read_persistent(&text)).unwrap_or_default()
}

/// Where the extras screen is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Extras {
    pub tab: Tab,
    pub page: usize,
    /// The entry the keyboard is on, on this page.
    pub focus: Option<usize>,
    /// The gallery picture open full screen (index into the whole gallery).
    pub viewing: Option<usize>,
    /// The music room track playing (index into the whole music room).
    pub playing: Option<usize>,
}

impl Default for Extras {
    fn default() -> Extras {
        Extras { tab: Tab::Gallery, page: 0, focus: None, viewing: None, playing: None }
    }
}

impl Game<'_> {
    /// Whether the game has a gallery or music room (the main menu offers Extras).
    pub fn has_extras(&self) -> bool {
        !self.program.gallery.is_empty() || !self.program.music_room.is_empty()
    }

    /// Write the persistent data if the story changed it.
    pub(super) fn save_persistent(&mut self) {
        self.persistent_dirty |= self.session.take_persistent_changed();
        if !self.persistent_dirty {
            return;
        }
        match self.storage.write(PERSISTENT_FILE, &write_persistent(self.session.persistent())) {
            Ok(()) => self.persistent_dirty = false,
            Err(e) => self.notify(format!("Could not save what was unlocked: {e}")),
        }
    }

    fn extras_tabs(&self) -> Vec<Tab> {
        ex::tabs(self.program.gallery.len(), self.program.music_room.len())
    }

    fn extras_list(&self) -> &[String] {
        match self.extras.tab {
            Tab::Gallery => &self.program.gallery,
            Tab::Music => &self.program.music_room,
        }
    }

    pub(super) fn open_extras(&mut self) {
        let Some(&tab) = self.extras_tabs().first() else { return };
        self.extras = Extras { tab, ..Extras::default() };
        self.screen = Screen::Extras;
    }

    fn close_extras(&mut self) {
        if self.extras.playing.take().is_some() {
            // Back to the menu's own music.
            let mut cmds = vec![Cmd::StopMusic { fade: 1.0 }];
            if let Some(src) = self.theme().menu_music.clone() {
                cmds.push(Cmd::PlayMusic { src, intro: None, fade: 1.0, looped: true, volume: 1.0 });
            }
            self.show(&cmds);
        }
        self.screen = Screen::MainMenu;
    }

    /// The current page's entries and whether each is unlocked.
    pub(super) fn extras_entries(&self) -> Vec<(&str, bool)> {
        let per = ex::per_page(self.extras.tab);
        let list = self.extras_list();
        list.iter().skip(self.extras.page * per).take(per).map(|s| (s.as_str(), self.session.unlocked(s))).collect()
    }

    fn extras_pages(&self) -> usize {
        ex::pages(self.extras.tab, self.extras_list().len())
    }

    /// The extras screen as drawn (`hover` from the cursor).
    pub(super) fn draw_extras_screen(&self, out: &mut Vec<Draw>, image_size: &dyn Fn(&str) -> Option<Size>) {
        let theme = self.view.theme();
        if let Some(i) = self.extras.viewing {
            return ex::draw_picture(out, theme, &self.program.gallery[i], image_size);
        }
        let (tabs, entries, pages) = (self.extras_tabs(), self.extras_entries(), self.extras_pages());
        let per = ex::per_page(self.extras.tab);
        let start = self.extras.page * per;
        let playing = self
            .extras
            .playing
            .filter(|_| self.extras.tab == Tab::Music)
            .and_then(|p| p.checked_sub(start))
            .filter(|&p| p < per);
        let hover = self.pointer().and_then(|(x, y)| ex::hit(x, y, &tabs, self.extras.tab, entries.len(), self.extras.page, pages));
        let shown = ex::ExtrasScreen {
            tabs: &tabs,
            tab: self.extras.tab,
            page: self.extras.page,
            pages,
            entries: &entries,
            focus: self.extras.focus,
            playing,
            hover,
        };
        ex::draw_extras(out, theme, &shown, image_size);
    }

    pub(super) fn extras_event(&mut self, event: Event) {
        if let Some(i) = self.extras.viewing {
            return self.picture_event(event, i);
        }
        let (tabs, on_page, pages) = (self.extras_tabs(), self.extras_entries().len(), self.extras_pages());
        match event {
            Event::Press(Button::Left) => {
                match self.cursor.and_then(|(x, y)| ex::hit(x, y, &tabs, self.extras.tab, on_page, self.extras.page, pages)) {
                    Some(Hit::Tab(tab)) => self.extras = Extras { tab, playing: self.extras.playing, ..Extras::default() },
                    Some(Hit::Item(i)) => self.activate(i),
                    Some(Hit::Prev) => self.turn_extras_page(-1),
                    Some(Hit::Next) => self.turn_extras_page(1),
                    Some(Hit::Back) => self.close_extras(),
                    None => {}
                }
            }
            Event::Press(Button::Right) | Event::Key(Key::Escape | Key::Backspace) => self.close_extras(),
            Event::Key(Key::Enter | Key::Space) => {
                if let Some(f) = self.extras.focus {
                    self.activate(f);
                }
            }
            Event::Key(Key::Tab) if tabs.len() > 1 => {
                let next = tabs.iter().position(|&t| t == self.extras.tab).map_or(0, |i| (i + 1) % tabs.len());
                self.extras = Extras { tab: tabs[next], playing: self.extras.playing, ..Extras::default() };
            }
            Event::Key(key @ (Key::Up | Key::Down | Key::Left | Key::Right)) => self.move_extras_focus(key, on_page),
            _ => {}
        }
    }

    /// Arrow keys: around the grid (or down the list); off the side turns the page.
    fn move_extras_focus(&mut self, key: Key, on_page: usize) {
        if on_page == 0 {
            return;
        }
        let Some(f) = self.extras.focus else {
            self.extras.focus = Some(0);
            return;
        };
        let cols = if self.extras.tab == Tab::Gallery { ex::GALLERY_COLS } else { 1 };
        match key {
            Key::Left if cols > 1 && f % cols > 0 => self.extras.focus = Some(f - 1),
            Key::Right if cols > 1 && f % cols < cols - 1 && f + 1 < on_page => self.extras.focus = Some(f + 1),
            Key::Left => self.turn_extras_page(-1),
            Key::Right => self.turn_extras_page(1),
            Key::Up if f >= cols => self.extras.focus = Some(f - cols),
            Key::Down if f + cols < on_page => self.extras.focus = Some(f + cols),
            _ => {}
        }
    }

    fn turn_extras_page(&mut self, by: isize) {
        let page = self.extras.page as isize + by;
        if page < 0 || page >= self.extras_pages() as isize {
            return;
        }
        self.extras.page = page as usize;
        let on_page = self.extras_entries().len();
        self.extras.focus = self.extras.focus.map(|f| f.min(on_page.saturating_sub(1)));
    }

    /// Open a picture or play (or stop) a track; locked entries do nothing.
    fn activate(&mut self, i: usize) {
        let index = self.extras.page * ex::per_page(self.extras.tab) + i;
        let Some(src) = self.extras_list().get(index).cloned() else { return };
        if !self.session.unlocked(&src) {
            return self.notify("Not unlocked yet");
        }
        match self.extras.tab {
            Tab::Gallery => self.extras.viewing = Some(index),
            Tab::Music if self.extras.playing == Some(index) => {
                self.extras.playing = None;
                self.show(&[Cmd::StopMusic { fade: 0.5 }]);
            }
            Tab::Music => {
                self.extras.playing = Some(index);
                self.show(&[Cmd::PlayMusic { src, intro: None, fade: 0.5, looped: true, volume: 1.0 }]);
            }
        }
    }

    /// A picture is open: Left/Right go to the other unlocked ones, anything else closes it.
    fn picture_event(&mut self, event: Event, i: usize) {
        let step = match event {
            Event::Key(Key::Left) => self.program.gallery.len() - 1,
            Event::Key(Key::Right) => 1,
            Event::Press(_) | Event::Key(Key::Escape | Key::Enter | Key::Space | Key::Backspace) => {
                self.extras.viewing = None;
                return;
            }
            _ => return,
        };
        let n = self.program.gallery.len();
        let next = (1..n).map(|k| (i + k * step) % n).find(|&j| self.session.unlocked(&self.program.gallery[j]));
        if let Some(j) = next {
            self.extras.viewing = Some(j);
        }
    }
}
