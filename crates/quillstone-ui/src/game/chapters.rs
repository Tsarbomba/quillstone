//! The main menu's Chapters screen: tiles for the `chapters:` entries; an open one starts a new game at its label.

use super::*;
use crate::chapters::{self as ch, Hit, Tile};

/// Where the chapters screen is.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(super) struct Chapters {
    pub page: usize,
    /// The tile the keyboard is on, on this page.
    pub focus: Option<usize>,
}

impl Game<'_> {
    /// Whether the game has chapters (the main menu offers Chapters).
    pub fn has_chapters(&self) -> bool {
        !self.program.chapters.is_empty()
    }

    /// Whether the chapter at `index` can be started: its label was reached once (kept across playthroughs), or it is
    /// `{open}`, or the player ran `--unlock-all`.
    fn chapter_open(&self, index: usize) -> bool {
        self.unlock_all || self.program.chapters.get(index).is_some_and(|c| self.session.chapter_unlocked(&c.label))
    }

    pub(super) fn open_chapters(&mut self) {
        self.chapters = Chapters::default();
        self.screen = Screen::Chapters;
    }

    /// After a reload: the page stays in range.
    pub(super) fn refresh_chapters(&mut self) {
        self.chapters.page = self.chapters.page.min(ch::pages(self.program.chapters.len()) - 1);
        let on_page = self.chapters_on_page();
        self.chapters.focus = self.chapters.focus.map(|f| f.min(on_page.saturating_sub(1))).filter(|_| on_page > 0);
    }

    fn chapters_on_page(&self) -> usize {
        self.program.chapters.len().saturating_sub(self.chapters.page * ch::PER_PAGE).min(ch::PER_PAGE)
    }

    /// Start a new game at the chapter `label`: the script's starting variables and the player's persistent data, like
    /// New Game, but the story begins at the label. Saving works as in any game. `false` if there is no such chapter
    /// or it is not open.
    pub fn start_chapter(&mut self, label: &str) -> bool {
        let Some(index) = self.program.chapters.iter().position(|c| c.label == label) else { return false };
        if self.in_story || !self.chapter_open(index) {
            return false;
        }
        let seed = quillstone_runtime::next_seed(&mut self.seed);
        let Some((session, first)) = Session::start_replay(self.program, self.session.persistent().clone(), seed, label) else {
            return false;
        };
        self.enter_story(session, &first);
        true
    }

    pub(super) fn draw_chapters_screen(&self, out: &mut Vec<Draw>, image_size: &dyn Fn(&str) -> Option<Size>) {
        let start = self.chapters.page * ch::PER_PAGE;
        let tiles: Vec<Tile> = (start..self.program.chapters.len().min(start + ch::PER_PAGE))
            .map(|i| {
                let c = &self.program.chapters[i];
                Tile { title: self.tr(&c.title), open: self.chapter_open(i), thumb: c.thumb.as_deref() }
            })
            .collect();
        let pages = ch::pages(self.program.chapters.len());
        let hover = self.pointer().and_then(|(x, y)| ch::hit(x, y, tiles.len(), self.chapters.page, pages));
        let shown = ch::ChaptersScreen { page: self.chapters.page, pages, tiles: &tiles, focus: self.chapters.focus, hover };
        ch::draw_chapters(out, self.view.theme(), self.ui(), &shown, image_size);
    }

    pub(super) fn chapters_event(&mut self, event: Event) {
        let (on_page, pages) = (self.chapters_on_page(), ch::pages(self.program.chapters.len()));
        match event {
            Event::Press(Button::Left) => match self.cursor.and_then(|(x, y)| ch::hit(x, y, on_page, self.chapters.page, pages)) {
                Some(Hit::Item(i)) => self.pick_chapter(i),
                Some(Hit::Prev) => self.turn_chapters_page(-1),
                Some(Hit::Next) => self.turn_chapters_page(1),
                Some(Hit::Back) => self.screen = Screen::MainMenu,
                None => {}
            },
            Event::Press(Button::Right) | Event::Key(Key::Escape | Key::Backspace) => self.screen = Screen::MainMenu,
            Event::Key(Key::Enter | Key::Space) => {
                if let Some(f) = self.chapters.focus {
                    self.pick_chapter(f);
                }
            }
            Event::Key(key @ (Key::Up | Key::Down | Key::Left | Key::Right)) => self.move_chapters_focus(key, on_page),
            _ => {}
        }
    }

    /// Arrow keys: around the grid; off the side turns the page.
    fn move_chapters_focus(&mut self, key: Key, on_page: usize) {
        if on_page == 0 {
            return;
        }
        let Some(f) = self.chapters.focus else {
            self.chapters.focus = Some(0);
            return;
        };
        match key {
            Key::Left if f % ch::COLS > 0 => self.chapters.focus = Some(f - 1),
            Key::Right if f % ch::COLS < ch::COLS - 1 && f + 1 < on_page => self.chapters.focus = Some(f + 1),
            Key::Left => self.turn_chapters_page(-1),
            Key::Right => self.turn_chapters_page(1),
            Key::Up if f >= ch::COLS => self.chapters.focus = Some(f - ch::COLS),
            Key::Down if f + ch::COLS < on_page => self.chapters.focus = Some(f + ch::COLS),
            _ => {}
        }
    }

    fn turn_chapters_page(&mut self, by: isize) {
        let page = self.chapters.page as isize + by;
        if page < 0 || page >= ch::pages(self.program.chapters.len()) as isize {
            return;
        }
        self.chapters.page = page as usize;
        let on_page = self.chapters_on_page();
        self.chapters.focus = self.chapters.focus.map(|f| f.min(on_page.saturating_sub(1)));
    }

    fn pick_chapter(&mut self, i: usize) {
        let index = self.chapters.page * ch::PER_PAGE + i;
        let Some(label) = self.program.chapters.get(index).map(|c| c.label.clone()) else { return };
        if !self.start_chapter(&label) {
            self.notify_ui("Not unlocked yet", &[]);
        }
    }
}
