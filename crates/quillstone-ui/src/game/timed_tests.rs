//! Timed `::choice{time=..}` in the game shell: the countdown, what pauses it, what picks the
//! default, and that nothing but the player's own pick or the expiry moves the story.

use super::test_util::{key, line};
use super::*;
use quillstone_render::Size;
use quillstone_script::parse;

const STORY: &str = "\
---
vars: { calm: 1, hits: 0 }
---
# start
Before the choice.
::choice{time=4}
- [Speak](#spoke){hits += 1}
- [Stay quiet](#quiet){default, if calm > 0}
- [Run](#spoke)
::
# spoke
Spoke.
::end
# quiet
Quiet.
::end
";

fn new_game<'p>(p: &'p Program, text_speed: f32) -> Game<'p> {
    let settings = Settings { text_speed, ..Settings::default() };
    let mut g = Game::new(p, settings, Box::new(crate::MemoryStorage::default()));
    g.start_story();
    g
}

fn none(_: &str) -> Option<Size> {
    None
}

/// Walk to the options.
fn at_choice<'p>(p: &'p Program) -> Game<'p> {
    let mut g = new_game(p, 0.0);
    key(&mut g, Key::Space);
    assert_eq!(g.view().choices.len(), 3);
    g
}

/// The width of the countdown bar's fill, if one is drawn: it is the accent-coloured rect
/// that is exactly as thick as the theme says.
fn fill(g: &Game) -> Option<f32> {
    let h = g.theme().choice_timer_height;
    g.draw_list(&none).iter().find_map(|d| match d {
        Draw::Rect { rect, color } if rect.h == h && *color == g.theme().accent_color => Some(rect.w),
        _ => None,
    })
}

#[test]
fn the_bar_shrinks_and_time_running_out_picks_the_default_as_if_clicked() {
    let p = parse(STORY).unwrap();
    let mut g = at_choice(&p);
    assert_eq!(fill(&g), Some(1000.0), "full on arrival");
    g.tick(1.0, false);
    assert_eq!(fill(&g), Some(750.0));
    g.tick(2.0, false);
    assert_eq!(fill(&g), Some(250.0));
    assert!(g.needs_ticks(), "the bar needs frames");
    g.tick(0.9, false);
    assert_eq!(g.view().choices.len(), 3, "not yet");
    g.tick(0.2, false);
    assert!(g.view().choices.is_empty());
    assert_eq!(line(&g), "Quiet.", "the default is the second option");
    assert_eq!(fill(&g), None);
    // picked like a click: the effects ran and the arrival was an auto save
    assert_eq!(g.state().vars.get("hits"), Some(&Value::Int(0)));
}

#[test]
fn an_early_pick_by_key_or_click_works_as_normal_and_hover_does_not_touch_the_clock() {
    let p = parse(STORY).unwrap();
    let mut g = at_choice(&p);
    g.tick(3.0, false);
    let first = quillstone_render::choice_rects(g.theme(), 3)[0];
    g.handle(Event::CursorMoved(Some((first.x + 5.0, first.y + 5.0))));
    key(&mut g, Key::Down); // keyboard focus moves too
    assert_eq!(fill(&g), Some(250.0), "hover and focus change nothing");
    key(&mut g, Key::Char('1'));
    assert_eq!(line(&g), "Spoke.");
    assert_eq!(g.state().vars.get("hits"), Some(&Value::Int(1)));
    g.tick(60.0, false);
    assert_eq!(line(&g), "Spoke.", "a pick leaves no timer behind");
}

#[test]
fn settings_history_and_confirms_pause_the_clock() {
    let p = parse(STORY).unwrap();
    let mut g = at_choice(&p);
    g.tick(1.0, false);
    key(&mut g, Key::Escape); // settings
    g.tick(100.0, false);
    key(&mut g, Key::Escape);
    assert_eq!((g.screen(), g.view().choices.len()), (Screen::Story, 3));
    assert_eq!(fill(&g), Some(750.0));
    key(&mut g, Key::Char('h')); // history
    g.tick(100.0, false);
    key(&mut g, Key::Escape);
    assert_eq!(fill(&g), Some(750.0));
    g.ask("Sure?", Then::Quit);
    g.tick(100.0, false);
    key(&mut g, Key::Escape); // "no"
    assert_eq!(fill(&g), Some(750.0));
    g.tick(3.1, false);
    assert_eq!(line(&g), "Quiet.");
}

#[test]
fn an_unfocused_window_pauses_the_clock() {
    let p = parse(STORY).unwrap();
    let mut g = at_choice(&p);
    g.tick(1.0, false);
    g.set_focused(false);
    g.tick(100.0, false);
    assert_eq!(fill(&g), Some(750.0));
    assert!(!g.needs_ticks(), "nothing to redraw while it waits");
    g.set_focused(true);
    g.tick(1.0, false);
    assert_eq!(fill(&g), Some(500.0));
}

#[test]
fn the_clock_does_not_run_while_the_line_over_the_options_is_still_typing() {
    let p = parse(STORY).unwrap();
    let mut g = at_choice(&p);
    // (a line typing out with the options already up: not something the script can do today)
    g.view.set_speed(10.0);
    let say = Cmd::Say { speaker: None, color: None, text: "A line that takes a while to type".into() };
    let options = ["Speak", "Stay quiet", "Run"].map(String::from).to_vec();
    g.view.apply(&[say, Cmd::ShowChoices { options, locked: Vec::new() }]);
    g.choice_elapsed = 0.0;
    assert_eq!(g.view().choices.len(), 3);
    assert!(!g.view().dialogue.as_ref().unwrap().is_complete(), "still typing");
    g.tick(0.5, false);
    assert_eq!(fill(&g), Some(1000.0), "typing time is not counted");
    for _ in 0..100 {
        g.tick(0.1, false);
        if g.view().dialogue.as_ref().unwrap().is_complete() {
            break;
        }
    }
    g.tick(1.0, false);
    // (the tick that finished the line counts too, so up to 0.1 s more than the 1 s)
    assert!((700.0..=750.0).contains(&fill(&g).unwrap()), "it starts once the line is typed: {:?}", fill(&g));
}

#[test]
fn a_hidden_default_falls_back_to_the_first_visible_option() {
    let p = parse(&STORY.replace("calm: 1", "calm: 0")).unwrap();
    let mut g = new_game(&p, 0.0);
    key(&mut g, Key::Space);
    assert_eq!(g.view().choices.len(), 2);
    g.tick(4.0, false);
    assert_eq!(line(&g), "Spoke.", "Speak is first now");
    assert_eq!(g.state().vars.get("hits"), Some(&Value::Int(1)));
}

#[test]
fn auto_mode_never_picks_and_skip_stops_at_the_choice() {
    let p = parse(STORY).unwrap();
    let mut g = new_game(&p, 0.0);
    g.handle(Event::Key(Key::Char('a')));
    assert!(g.auto());
    g.tick(5.0, false); // auto moves on to the options
    assert_eq!(g.view().choices.len(), 3);
    g.tick(3.0, false);
    assert_eq!(g.view().choices.len(), 3, "auto never picks: only the timer or the player does");
    g.tick(1.1, false);
    assert_eq!(line(&g), "Quiet.", "the timer does, even with auto on");
    // skip stops at the choice, which then counts down as usual
    let settings = Settings { text_speed: 0.0, skip_unread: true, ..Settings::default() };
    let mut g = Game::new(&p, settings, Box::new(crate::MemoryStorage::default()));
    g.start_story();
    key(&mut g, Key::Tab);
    assert!(g.skipping());
    g.tick(SKIP_DELAY, false); // reads the first line and stops at the options
    assert!(!g.skipping(), "skip stopped at the choice");
    assert_eq!(g.view().choices.len(), 3);
    assert_eq!(fill(&g), Some(1000.0), "skip did not use up any of the time");
    g.tick(1.0, false);
    assert_eq!(fill(&g), Some(750.0));
}

#[test]
fn rollback_and_load_start_the_countdown_afresh() {
    let p = parse(STORY).unwrap();
    let mut g = at_choice(&p);
    g.tick(3.0, false);
    key(&mut g, Key::F5);
    key(&mut g, Key::Char('3'));
    assert_eq!(line(&g), "Spoke.");
    key(&mut g, Key::Backspace); // back to the options
    assert_eq!(g.view().choices.len(), 3);
    assert_eq!(fill(&g), Some(1000.0), "full again");
    g.tick(2.0, false);
    key(&mut g, Key::F9); // load the quick save made at 3 s: the clock still starts full
    key(&mut g, Key::Enter);
    assert_eq!(g.view().choices.len(), 3);
    assert_eq!(fill(&g), Some(1000.0));
    g.tick(4.1, false);
    assert_eq!(line(&g), "Quiet.");
}

#[test]
fn the_bar_follows_the_theme() {
    let p = parse(STORY).unwrap();
    let mut g = at_choice(&p);
    let (theme, problems) = Theme::parse("choice_timer_color = #ff0000\nchoice_timer_height = 20\n");
    assert!(problems.is_empty());
    g.set_theme(theme);
    let list = g.draw_list(&none);
    let bar = list.iter().find_map(|d| match d {
        Draw::Rect { rect, color } if *color == [1.0, 0.0, 0.0, 1.0] => Some(*rect),
        _ => None,
    });
    let first = quillstone_render::choice_rects(g.theme(), 3)[0];
    let bar = bar.expect("a red bar");
    assert_eq!((bar.h, bar.w, bar.x), (20.0, first.w, first.x));
    assert!(bar.y + bar.h <= first.y, "above the first option");
}

#[test]
fn untimed_choices_draw_no_bar_and_never_time_out() {
    let p = parse("# a\n::choice\n- [x](#b)\n- [y](#b)\n::\n# b\nDone.\n").unwrap();
    let mut g = new_game(&p, 0.0);
    assert_eq!(g.view().choices.len(), 2);
    assert_eq!(fill(&g), None);
    assert!(!g.needs_ticks());
    g.tick(10_000.0, false);
    assert_eq!(g.view().choices.len(), 2);
}

#[test]
fn a_minimised_window_stops_the_countdown_the_wait_and_the_typing_until_it_is_back() {
    let p = parse(STORY).unwrap();
    let mut g = at_choice(&p);
    g.tick(1.0, false);
    g.set_minimized(true);
    assert!(g.media_paused(), "videos stand still too");
    g.tick(100.0, false);
    assert_eq!(fill(&g), Some(750.0), "no time passed, and the default was not picked");
    g.set_minimized(false);
    g.tick(1.0, false);
    assert_eq!(fill(&g), Some(500.0), "it goes on from where it was");

    let waits = parse("::wait{time=2}\nAfter.\n").unwrap();
    let mut g = new_game(&waits, 0.0);
    g.tick(0.5, false);
    g.set_minimized(true);
    g.tick(100.0, false);
    assert_ne!(line(&g), "After.");
    g.set_minimized(false);
    g.tick(1.4, false);
    assert_ne!(line(&g), "After.", "1.9 s of the 2 s wait have run");
    g.tick(0.2, false);
    assert_eq!(line(&g), "After.");

    let typed = parse("Hello there, a long enough line.\n").unwrap();
    let mut g = new_game(&typed, 20.0);
    g.tick(0.25, false);
    let shown = g.view().dialogue.as_ref().unwrap().visible_chars();
    g.set_minimized(true);
    g.tick(100.0, false);
    assert_eq!(g.view().dialogue.as_ref().unwrap().visible_chars(), shown);
    g.set_minimized(false);
    g.tick(0.25, false);
    assert!(g.view().dialogue.as_ref().unwrap().visible_chars() > shown);
}
