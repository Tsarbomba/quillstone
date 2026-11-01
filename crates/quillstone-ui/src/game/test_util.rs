//! Helpers the `*_tests.rs` files share. A test file with a variant that differs (a drawn frame, a speed) keeps its own.

use super::*;

/// Settings with the text typed out at once.
pub(crate) fn instant() -> Settings {
    Settings { text_speed: 0.0, ..Settings::default() }
}

/// A game with in-memory storage, already in the story.
pub(crate) fn new_game<'p>(p: &'p Program, settings: Settings) -> Game<'p> {
    let mut g = Game::new(p, settings, Box::new(crate::MemoryStorage::default()));
    g.start_story();
    g
}

/// [`new_game`] with [`instant`] text.
pub(crate) fn instant_game(p: &Program) -> Game<'_> {
    new_game(p, instant())
}

pub(crate) fn key(g: &mut Game, k: Key) {
    g.handle(Event::Key(k));
}

/// The dialogue text now shown (without tags).
pub(crate) fn line(g: &Game) -> String {
    g.view().dialogue.as_ref().map(|d| d.text.clone()).unwrap_or_default()
}

/// Move the cursor to the middle of `r` and click.
pub(crate) fn click(g: &mut Game, r: Rect) {
    g.handle(Event::CursorMoved(Some((r.x + r.w / 2.0, r.y + r.h / 2.0))));
    g.handle(Event::Press(Button::Left));
    g.handle(Event::Release(Button::Left));
}
