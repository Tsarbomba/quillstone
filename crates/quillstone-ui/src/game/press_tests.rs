//! `::press` in the game shell: presses of the asked key count, other keys are swallowed, the clock stops,
//! restarts and is drawn like a timed choice's, and the result goes to the story.

use super::*;
use quillstone_render::Size;
use quillstone_script::parse;

fn story(attrs: &str) -> String {
    format!(
        "---\nvars: {{ ok: \"\", score: 7 }}\n---\n# start\nReady?\n\n::press{{{attrs} result=ok prompt=\"Duck, {{score}}!\"}}\n\
         ::if{{ok == \"hit\"}}\nDodged.\n::else\nHit.\n::endif\n"
    )
}

fn at_prompt(p: &Program) -> Game<'_> {
    let mut g = Game::new(p, Settings { text_speed: 0.0, ..Settings::default() }, Box::new(crate::MemoryStorage::default()));
    g.start_story();
    key(&mut g, Key::Space);
    assert!(on_prompt(&g));
    g
}

fn key(g: &mut Game, k: Key) {
    g.handle(Event::Key(k));
}

fn on_prompt(g: &Game) -> bool {
    matches!(g.session.state().pending, Some(Cmd::ShowPress { .. }))
}

fn line(g: &Game) -> String {
    g.view().dialogue.as_ref().map(|d| d.text.clone()).unwrap_or_default()
}

fn ok(g: &Game) -> Value {
    g.session.state().vars["ok"].clone()
}

fn none(_: &str) -> Option<Size> {
    None
}

#[test]
fn the_right_key_in_time_is_a_hit_and_either_confirm_key_counts_for_space_and_enter() {
    let p = parse(&story("key=space time=1.2")).unwrap();
    for k in [Key::Space, Key::Enter] {
        let mut g = at_prompt(&p);
        g.tick(1.0, false);
        key(&mut g, k);
        assert_eq!((line(&g), ok(&g)), ("Dodged.".to_string(), Value::Str("hit".into())), "{k:?}");
    }
    // letters in either case, arrows and the click
    for (name, event) in [
        ("x", Event::Key(Key::Char('x'))),
        ("x", Event::Key(Key::Char('X'))),
        ("left", Event::Key(Key::Left)),
        ("down", Event::Key(Key::Down)),
        ("click", Event::Press(Button::Left)),
    ] {
        let p = parse(&story(&format!("key={name} time=2"))).unwrap();
        let mut g = at_prompt(&p);
        g.handle(event.clone());
        assert_eq!(ok(&g), Value::Str("hit".into()), "{name} / {event:?}");
    }
}

#[test]
fn a_wrong_key_or_click_is_ignored_and_the_prompt_keeps_waiting() {
    let p = parse(&story("key=space time=1.2")).unwrap();
    let mut g = at_prompt(&p);
    for e in [Event::Key(Key::Char('x')), Event::Key(Key::Left), Event::Key(Key::Up), Event::Press(Button::Left)] {
        g.handle(e);
        assert!(on_prompt(&g) && ok(&g) == Value::Str(String::new()));
    }
    key(&mut g, Key::Space);
    assert_eq!(ok(&g), Value::Str("hit".into()), "the right key still works afterwards");
    // a prompt for a letter does not take Space
    let q = parse(&story("key=z time=1.2")).unwrap();
    let mut g = at_prompt(&q);
    key(&mut g, Key::Space);
    key(&mut g, Key::Enter);
    assert!(on_prompt(&g));
}

#[test]
fn running_out_of_time_is_a_miss() {
    let p = parse(&story("key=space time=1.2")).unwrap();
    let mut g = at_prompt(&p);
    g.tick(1.1, false);
    assert!(on_prompt(&g));
    g.tick(0.2, false);
    assert_eq!((line(&g), ok(&g)), ("Hit.".to_string(), Value::Str("miss".into())));
}

#[test]
fn mashing_needs_every_press_before_the_time_is_up() {
    let p = parse(&story("key=enter time=2 count=3")).unwrap();
    let mut g = at_prompt(&p);
    key(&mut g, Key::Enter);
    g.tick(0.5, false);
    key(&mut g, Key::Space); // the other confirm key counts too
    assert!(on_prompt(&g), "two of three");
    key(&mut g, Key::Enter);
    assert_eq!(ok(&g), Value::Str("hit".into()));
    // two presses and then the time runs out
    let mut g = at_prompt(&p);
    key(&mut g, Key::Enter);
    key(&mut g, Key::Enter);
    g.tick(2.1, false);
    assert_eq!((line(&g), ok(&g)), ("Hit.".to_string(), Value::Str("miss".into())));
    // wrong keys do not count towards it
    let mut g = at_prompt(&p);
    for _ in 0..5 {
        key(&mut g, Key::Char('q'));
    }
    assert!(on_prompt(&g));
}

#[test]
fn the_keys_that_advance_go_back_or_open_menus_are_swallowed_but_escape_still_opens_settings() {
    let p = parse(&story("key=up time=5")).unwrap();
    let mut g = at_prompt(&p);
    for k in [Key::Space, Key::Enter, Key::Right, Key::Left, Key::Backspace, Key::PageUp, Key::Down] {
        key(&mut g, k);
        assert!(on_prompt(&g) && g.screen == Screen::Story, "{k:?}");
    }
    for c in ['h', 'a', 's', 'l', 'i', 'j', 'u', '1'] {
        key(&mut g, Key::Char(c));
        assert!(on_prompt(&g) && g.screen == Screen::Story && !g.auto() && !g.view().hide_ui(), "{c}");
    }
    g.handle(Event::Wheel(1.0)); // the wheel is not a key: it still rolls back (to the line before)
    assert!(!on_prompt(&g));
    let mut g = at_prompt(&p);
    key(&mut g, Key::Escape);
    assert_eq!(g.screen, Screen::Settings);
    g.tick(100.0, false);
    key(&mut g, Key::Escape);
    assert!(on_prompt(&g), "the clock waited");
    key(&mut g, Key::Up);
    assert_eq!(ok(&g), Value::Str("hit".into()));
}

#[test]
fn the_clock_waits_while_minimised_or_unfocused() {
    let p = parse(&story("key=click time=1")).unwrap();
    let mut g = at_prompt(&p);
    g.tick(0.5, false);
    g.set_minimized(true);
    g.tick(100.0, false);
    g.set_minimized(false);
    g.set_focused(false);
    g.tick(100.0, false);
    assert!(!g.needs_ticks());
    g.set_focused(true);
    assert!(g.needs_ticks() && on_prompt(&g));
    g.tick(0.4, false);
    assert!(on_prompt(&g), "0.9 s of play in all");
    g.tick(0.2, false);
    assert_eq!(ok(&g), Value::Str("miss".into()));
}

#[test]
fn rollback_restarts_the_prompt_and_roll_forward_reuses_the_result() {
    let p = parse(&story("key=space time=1 count=2")).unwrap();
    let mut g = at_prompt(&p);
    g.tick(0.9, false);
    key(&mut g, Key::Space);
    key(&mut g, Key::Space);
    assert_eq!(line(&g), "Dodged.");
    g.handle(Event::Wheel(1.0)); // back to the prompt: a fresh clock and no presses
    assert!(on_prompt(&g) && ok(&g) == Value::Str(String::new()));
    assert_eq!(g.press_count, 0);
    g.handle(Event::Wheel(-1.0)); // forward: the recorded result, not new presses
    assert_eq!((line(&g), ok(&g)), ("Dodged.".to_string(), Value::Str("hit".into())));
    g.handle(Event::Wheel(1.0));
    key(&mut g, Key::Space);
    g.tick(1.1, false); // one of two presses, then the clock runs out
    assert_eq!(ok(&g), Value::Str("miss".into()));
}

#[test]
fn a_save_at_the_prompt_loads_at_the_prompt_with_the_presses_and_clock_restarted() {
    let p = parse(&story("key=space time=1 count=2")).unwrap();
    let mut g = at_prompt(&p);
    key(&mut g, Key::Space);
    g.tick(0.9, false);
    key(&mut g, Key::F5);
    g.tick(0.2, false);
    key(&mut g, Key::F9);
    key(&mut g, Key::Enter); // confirm the load
    assert!(on_prompt(&g));
    assert_eq!(g.press_count, 0);
    g.tick(0.9, false);
    assert!(on_prompt(&g), "a full second again");
    key(&mut g, Key::Space);
    key(&mut g, Key::Space);
    assert_eq!(ok(&g), Value::Str("hit".into()));
}

#[test]
fn skip_stops_at_the_prompt_and_auto_forward_never_presses_it() {
    let p = parse(&story("key=space time=3")).unwrap();
    let mut g = Game::new(&p, Settings { text_speed: 0.0, skip_unread: true, ..Settings::default() }, Box::new(crate::MemoryStorage::default()));
    g.start_story();
    key(&mut g, Key::Tab);
    g.tick(0.1, false);
    assert!(on_prompt(&g) && !g.skipping());
    key(&mut g, Key::Char('a'));
    g.tick(2.0, false);
    assert!(on_prompt(&g) && ok(&g) == Value::Str(String::new()));
}

#[test]
fn a_prompt_or_timing_bar_wants_fresh_keys_and_a_line_does_not() {
    let p = parse(&story("key=space time=3")).unwrap();
    let mut g = Game::new(&p, Settings { text_speed: 0.0, ..Settings::default() }, Box::new(crate::MemoryStorage::default()));
    g.start_story();
    assert!(!g.wants_fresh_keys());
    key(&mut g, Key::Space);
    assert!(g.wants_fresh_keys());
    let bar = parse("---\nvars: { hit: \"\" }\n---\n::timing{result=hit good=40%..60%}\nDone.\n").unwrap();
    let mut g = Game::new(&bar, Settings::default(), Box::new(crate::MemoryStorage::default()));
    g.start_story();
    assert!(g.wants_fresh_keys());
}

#[test]
fn the_prompt_is_drawn_with_the_key_cap_the_time_left_and_for_mashing_the_progress() {
    let p = parse(&story("key=space time=2 count=4")).unwrap();
    let mut g = at_prompt(&p);
    g.tick(0.5, false);
    key(&mut g, Key::Space);
    let list = g.draw_list(&none);
    let bar = |y: f32| list.iter().find_map(|d| match d {
        Draw::Rect { rect, color } if rect.y == y && rect.h == 24.0 && color[3] == 1.0 => Some(rect.w),
        _ => None,
    });
    assert_eq!((bar(570.0), bar(606.0)), (Some(750.0), Some(250.0)), "three quarters of the time left, one press of four");
    assert!(list.iter().any(|d| matches!(d, Draw::Text { text, .. } if text == "SPACE")));
    assert!(list.iter().any(|d| matches!(d, Draw::Text { text, .. } if text == "Duck, 7!")), "prompt with its value");
    // a quick-time event has no progress bar; a loaded key picture replaces the plain cap
    let q = parse(&story("key=space time=2")).unwrap();
    let mut g = at_prompt(&q);
    assert!(!g.draw_list(&none).iter().any(|d| matches!(d, Draw::Rect { rect, .. } if rect.y == 606.0)));
    let (theme, _) = Theme::parse("press_key_image = cap.png\n");
    g.set_theme(theme);
    let loaded = |_: &str| Some(Size::new(10.0, 10.0));
    assert!(g.draw_list(&loaded).iter().any(|d| matches!(d, Draw::Image { src, dest, .. } if src == "cap.png" && dest.w == 160.0)));
    assert!(g.draw_list(&none).iter().all(|d| !matches!(d, Draw::Image { .. })), "not loaded yet: the plain cap");
}
