//! Locked choice options in the game shell: nothing but a pickable option can be hovered,
//! focused, clicked or chosen by number, and the timer never picks a locked one.

use super::*;
use quillstone_render::Size;
use quillstone_script::parse;

const STORY: &str = "\
---
vars: { str: 1 }
---
# start
Before.
::choice{time=4}
- [Lift](#lift){if str >= 5, show=\"Needs STR 5\"}
- [Look](#look)
- [Force](#lift){if str >= 9, show}
- [Wait](#wait){default, if str >= 5, show}
::
# lift
Lifted.
::end
# look
Looked.
::end
# wait
Waited.
::end
";

fn at_choice<'p>(p: &'p Program) -> Game<'p> {
    let mut g = Game::new(p, Settings { text_speed: 0.0, ..Settings::default() }, Box::new(crate::MemoryStorage::default()));
    g.start_story();
    g.handle(Event::Key(Key::Space));
    assert_eq!(g.view().choices.len(), 4);
    g
}

fn line(g: &Game) -> String {
    g.view().dialogue.as_ref().map(|d| d.text.clone()).unwrap_or_default()
}

fn none(_: &str) -> Option<Size> {
    None
}

#[test]
fn locked_options_are_drawn_but_clicks_and_number_keys_on_them_do_nothing() {
    let p = parse(STORY).unwrap();
    let mut g = at_choice(&p);
    assert!(g.draw_list(&none).iter().any(|d| matches!(d, Draw::Text { text, .. } if text == "Needs STR 5")));
    let first = quillstone_render::choice_rects(g.theme(), 4)[0];
    g.handle(Event::CursorMoved(Some((first.x + 5.0, first.y + 5.0))));
    g.handle(Event::Press(Button::Left));
    g.handle(Event::Key(Key::Char('1')));
    g.handle(Event::Key(Key::Char('3')));
    assert_eq!(g.view().choices.len(), 4, "still at the choice");
    g.handle(Event::Key(Key::Char('2')));
    assert_eq!(line(&g), "Looked.");
}

#[test]
fn the_keyboard_focus_skips_locked_options_and_enter_picks_the_focused_one() {
    let p = parse(STORY).unwrap();
    let mut g = at_choice(&p);
    let key = |g: &mut Game, k| g.handle(Event::Key(k));
    key(&mut g, Key::Down);
    assert_eq!(g.choice_focus, Some(1), "the first pickable one");
    key(&mut g, Key::Down);
    assert_eq!(g.choice_focus, Some(1), "the rest are locked: it stays");
    key(&mut g, Key::Up);
    assert_eq!(g.choice_focus, Some(1));
    key(&mut g, Key::Enter);
    assert_eq!(line(&g), "Looked.");
}

#[test]
fn time_running_out_picks_the_first_pickable_option_when_the_default_is_locked() {
    let p = parse(STORY).unwrap();
    let mut g = at_choice(&p);
    g.tick(5.0, false);
    assert_eq!(line(&g), "Looked.", "the default `Wait` is locked");
}
