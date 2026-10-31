//! Tests for the developer console (see `console.rs`).

use super::*;
use crate::settings::Settings;
use quillstone_script::parse;

const STORY: &str = "---\nvars: { gold: 5, name: \"Ann\" }\n---\n**A:** one {gold}\n**A:** two\n**A:** three\n# shop\n**A:** shop {gold}\n";

fn dev_game<'p>(p: &'p Program, dev: bool) -> Game<'p> {
    let settings = Settings { text_speed: 0.0, ..Settings::default() };
    let mut g = Game::new(p, settings, Box::new(crate::MemoryStorage::default()));
    g.set_dev(dev);
    g.start_story();
    g
}

fn key(g: &mut Game, k: Key) {
    g.handle(Event::Key(k));
}

/// Type `s` and press Enter.
fn run(g: &mut Game, s: &str) {
    g.handle(Event::Text(s.to_string()));
    key(g, Key::Enter);
}

fn answer(g: &Game) -> Vec<String> {
    g.console.as_ref().map(|c| c.lines.clone()).unwrap_or_default()
}

fn line(g: &Game) -> String {
    g.view().dialogue.as_ref().map(|d| d.text.clone()).unwrap_or_default()
}

#[test]
fn without_dev_the_key_does_nothing() {
    let p = parse(STORY).unwrap();
    let mut g = dev_game(&p, false);
    key(&mut g, Key::F12);
    assert!(g.console.is_none() && !g.wants_text());
    assert_eq!(line(&g), "one 5");
    g.set_dev(true);
    g.set_dev(false);
    key(&mut g, Key::F12);
    assert!(g.console.is_none());
}

#[test]
fn f12_opens_and_esc_or_f12_closes_it() {
    let p = parse(STORY).unwrap();
    let mut g = dev_game(&p, true);
    key(&mut g, Key::F12);
    assert!(g.console.is_some() && g.wants_text() && g.needs_ticks());
    key(&mut g, Key::Escape);
    assert!(g.console.is_none() && !g.wants_text());
    key(&mut g, Key::F12);
    key(&mut g, Key::F12);
    assert!(g.console.is_none());
    assert_eq!(g.screen(), Screen::Story, "Esc closed the console, not the game's settings");
}

#[test]
fn while_open_the_game_takes_no_other_keys_and_stands_still() {
    let p = parse(STORY).unwrap();
    let mut g = dev_game(&p, true);
    key(&mut g, Key::F12);
    for k in [Key::Space, Key::Enter, Key::Tab, Key::Char('s'), Key::Left, Key::F5] {
        key(&mut g, k);
    }
    g.handle(Event::Press(Button::Left));
    g.handle(Event::Wheel(1.0));
    g.tick(10.0, false);
    assert_eq!(line(&g), "one 5", "nothing advanced");
    assert_eq!(g.screen(), Screen::Story);
    assert!(g.draw_list(&|_| None).iter().any(|d| matches!(d, Draw::Text { text, .. } if text.starts_with("> "))));
}

#[test]
fn get_vars_help_and_errors_answer_in_lines() {
    let p = parse(STORY).unwrap();
    let mut g = dev_game(&p, true);
    key(&mut g, Key::F12);
    run(&mut g, "get gold");
    assert_eq!(answer(&g), ["gold = 5"]);
    run(&mut g, "name");
    assert_eq!(answer(&g), ["name = \"Ann\""]);
    run(&mut g, "vars");
    assert_eq!(answer(&g), ["gold = 5", "name = \"Ann\""]);
    run(&mut g, "vars na");
    assert_eq!(answer(&g), ["name = \"Ann\""]);
    run(&mut g, "help");
    assert!(answer(&g).len() > 3);
    run(&mut g, "get zzz");
    assert_eq!(answer(&g), ["zzz is not set"]);
    run(&mut g, "dance now");
    assert!(answer(&g)[0].starts_with("error: unknown command"));
    assert!(g.console.is_some(), "an error leaves it open");
}

#[test]
fn set_changes_the_variable_the_screen_and_refuses_a_type_change() {
    let p = parse(STORY).unwrap();
    let mut g = dev_game(&p, true);
    key(&mut g, Key::F12);
    run(&mut g, "set gold = gold * 3");
    assert_eq!(answer(&g), ["gold = 15"]);
    assert_eq!(line(&g), "one 15", "the line on screen uses it");
    run(&mut g, "set gold = \"rich\"");
    assert!(answer(&g)[0].contains("can't be changed to text"), "{:?}", answer(&g));
    run(&mut g, "set name = ");
    assert!(answer(&g)[0].starts_with("error:"));
    run(&mut g, "get gold");
    assert_eq!(answer(&g), ["gold = 15"]);
}

#[test]
fn jump_goes_there_keeps_variables_and_rollback_comes_back() {
    let p = parse(STORY).unwrap();
    let mut g = dev_game(&p, true);
    key(&mut g, Key::F12);
    run(&mut g, "set gold = 9");
    run(&mut g, "jump Shop");
    assert_eq!(line(&g), "shop 9");
    assert!(answer(&g)[0].contains("jumped to `shop`"));
    g.handle(Event::Text("x".into()));
    key(&mut g, Key::Escape);
    g.handle(Event::Wheel(1.0)); // rollback
    assert_eq!(line(&g), "one 9", "back where the jump left");
}

#[test]
fn jump_to_an_unknown_label_is_an_error_line() {
    let p = parse(STORY).unwrap();
    let mut g = dev_game(&p, true);
    key(&mut g, Key::F12);
    run(&mut g, "jump nowhere");
    assert!(answer(&g)[0].starts_with("error: no label `nowhere`"));
    assert_eq!(line(&g), "one 5");
}

#[test]
fn typing_is_capped_and_backspace_deletes() {
    let p = parse(STORY).unwrap();
    let mut g = dev_game(&p, true);
    key(&mut g, Key::F12);
    g.handle(Event::Text("x".repeat(500)));
    assert_eq!(g.console.as_ref().unwrap().text.chars().count(), crate::console::MAX_LEN);
    key(&mut g, Key::Backspace);
    assert_eq!(g.console.as_ref().unwrap().text.chars().count(), crate::console::MAX_LEN - 1);
}

#[test]
fn start_dev_begins_at_the_label_with_the_variables() {
    let p = parse(STORY).unwrap();
    let mut g = Game::new(&p, Settings { text_speed: 0.0, ..Settings::default() }, Box::new(crate::MemoryStorage::default()));
    let at = quillstone_runtime::StartAt { label: Some("shop".into()), sets: vec![("gold".into(), "3".into())] };
    g.start_dev(&at).unwrap();
    assert_eq!(g.screen(), Screen::Story);
    assert_eq!(line(&g), "shop 3");
    let bad = quillstone_runtime::StartAt { label: Some("nope".into()), sets: vec![] };
    assert!(g.start_dev(&bad).unwrap_err().contains("no label"));
}
