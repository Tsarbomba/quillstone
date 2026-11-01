//! Tests for the `::input` screen (see `input.rs`).

use super::test_util::{click, instant, key, line, new_game};
use super::*;
use crate::settings::Settings;
use quillstone_render::Rect;
use quillstone_script::markup::plain;
use quillstone_script::parse;

const ASK: &str = "---\nvars: { hero.name: \"nobody\" }\n---\n**A:** First.\n::input{var=hero.name prompt=\"Your name?\" default=\"Alex\" max=8}\n**A:** Hi {hero.name}!\n**A:** Last.\n";
const NO_DEFAULT: &str = "---\nvars: { n: \"-\" }\n---\n**A:** First.\n::input{var=n prompt=\"Your name?\" max=5}\n**A:** Hi {n}!\n";

fn type_in(g: &mut Game, s: &str) {
    g.handle(Event::Text(s.to_string()));
}

/// A game waiting at the input of `src`.
fn at_input<'p>(p: &'p Program) -> Game<'p> {
    let mut g = new_game(p, instant());
    key(&mut g, Key::Space);
    assert_eq!(g.screen(), Screen::Input);
    g
}

fn texts(g: &Game) -> Vec<String> {
    g.draw_list(&|_| None).iter().filter_map(|d| if let Draw::Text { text, .. } = d { Some(text.clone()) } else { None }).collect()
}

fn field(g: &Game) -> String {
    g.input.text.clone()
}

#[test]
fn the_story_waits_at_a_panel_with_the_prompt_and_a_field() {
    let p = parse(ASK).unwrap();
    let mut g = new_game(&p, instant());
    assert!(!g.wants_text());
    key(&mut g, Key::Space);
    assert_eq!(g.screen(), Screen::Input);
    assert!(g.wants_text());
    let t = texts(&g);
    assert!(t.contains(&"Your name?".to_string()), "{t:?}");
    assert!(t.contains(&"Alex".to_string()), "the default is the placeholder: {t:?}");
    assert!(t.contains(&"0/8".to_string()) && t.contains(&"OK".to_string()));
    assert!(g.needs_ticks(), "the caret blinks");
    // a tick runs the blink, and the story does not move
    g.tick(0.3, false);
    assert_eq!(g.screen(), Screen::Input);
}

#[test]
fn typing_enter_and_the_answer_in_a_later_line() {
    let p = parse(ASK).unwrap();
    let mut g = at_input(&p);
    type_in(&mut g, "S");
    type_in(&mut g, "am");
    assert_eq!(field(&g), "Sam");
    assert!(texts(&g).iter().any(|t| t.starts_with("Sam")) && !texts(&g).contains(&"Alex".to_string()), "no placeholder once typed");
    key(&mut g, Key::Enter);
    assert_eq!(g.screen(), Screen::Story);
    assert!(!g.wants_text());
    assert_eq!(line(&g), "Hi Sam!");
    assert_eq!(g.state().vars["hero.name"], Value::Str("Sam".into()));
    assert_eq!(field(&g), "", "the field is emptied");
}

#[test]
fn a_name_with_braces_and_stars_stays_literal() {
    let p = parse(ASK).unwrap();
    let mut g = at_input(&p);
    type_in(&mut g, "{p}*a*\\");
    key(&mut g, Key::Enter);
    assert_eq!(line(&g), "Hi {p}*a*\\!");
    assert!(!g.view().dialogue.as_ref().unwrap().auto_advance);
}

#[test]
fn text_is_cleaned_while_typing_and_the_limit_counts_characters() {
    let p = parse(ASK).unwrap();
    let mut g = at_input(&p);
    type_in(&mut g, "a\nb\tc\u{7}d\r");
    assert_eq!(field(&g), "abcd", "control characters never get in");
    type_in(&mut g, "名前名前名前");
    assert_eq!(field(&g).chars().count(), 8, "capped at max=8 characters");
    assert_eq!(field(&g), "abcd名前名前");
    type_in(&mut g, "z");
    assert_eq!(field(&g).chars().count(), 8, "typing past the limit does nothing");
    key(&mut g, Key::Backspace);
    type_in(&mut g, "z");
    assert_eq!(field(&g), "abcd名前名z", "room again after deleting");
    // spaces at both ends are trimmed on confirm
    let mut g = at_input(&p);
    type_in(&mut g, "  Sam ");
    key(&mut g, Key::Enter);
    assert_eq!(line(&g), "Hi Sam!");
}

#[test]
fn backspace_deletes_and_never_rolls_back() {
    let p = parse(ASK).unwrap();
    let mut g = at_input(&p);
    let pc = g.state().pc;
    type_in(&mut g, "Sam");
    key(&mut g, Key::Backspace);
    assert_eq!(field(&g), "Sa");
    for _ in 0..5 {
        key(&mut g, Key::Backspace); // past empty: nothing happens, least of all a rollback
    }
    assert_eq!(field(&g), "");
    assert_eq!((g.screen(), g.state().pc), (Screen::Input, pc));
    // the mouse wheel and Left don't go back either
    g.handle(Event::Wheel(1.0));
    key(&mut g, Key::Left);
    assert_eq!((g.screen(), g.state().pc), (Screen::Input, pc));
}

#[test]
fn letters_are_not_shortcuts_and_a_key_sent_as_both_key_and_text_types_once() {
    let p = parse(ASK).unwrap();
    let mut g = at_input(&p);
    for c in ['a', 'h', 's', 'l', 'i', 'u', 'A', 'H', 'S', 'L', 'I', 'U', '1', 'n', 'y'] {
        key(&mut g, Key::Char(c));
    }
    for k in [Key::Tab, Key::Space, Key::Ctrl, Key::PageUp, Key::Home, Key::Right, Key::Up, Key::Down] {
        key(&mut g, k);
    }
    assert!(!g.auto() && !g.skipping(), "a and Tab did nothing");
    assert_eq!((g.screen(), field(&g)), (Screen::Input, String::new()), "no screens opened, nothing typed from keys");
    // what the frontend does for a key press: the key and its text
    for c in "hasl i".chars() {
        key(&mut g, if c == ' ' { Key::Space } else { Key::Char(c) });
        type_in(&mut g, &c.to_string());
    }
    assert_eq!(field(&g), "hasl i");
    assert_eq!(g.screen(), Screen::Input);
}

#[test]
fn text_is_ignored_outside_the_input_screen() {
    let p = parse(ASK).unwrap();
    let mut g = new_game(&p, instant());
    type_in(&mut g, "abc");
    assert_eq!((g.screen(), line(&g)), (Screen::Story, "First.".to_string()));
    key(&mut g, Key::Escape);
    type_in(&mut g, "abc");
    assert_eq!(g.screen(), Screen::Settings);
    assert!(!g.wants_text());
}

#[test]
fn enter_with_an_empty_field_takes_the_default_and_without_one_does_nothing() {
    let p = parse(ASK).unwrap();
    let mut g = at_input(&p);
    key(&mut g, Key::Enter);
    assert_eq!(line(&g), "Hi Alex!");

    let p = parse(NO_DEFAULT).unwrap();
    let mut g = at_input(&p);
    key(&mut g, Key::Enter);
    type_in(&mut g, "  ");
    key(&mut g, Key::Enter);
    assert_eq!(g.screen(), Screen::Input, "nothing to confirm");
    click(&mut g, crate::input::ok_rect(&Theme::default()));
    assert_eq!(g.screen(), Screen::Input, "OK is dimmed and does nothing");
    type_in(&mut g, "Bo");
    key(&mut g, Key::Enter);
    assert_eq!(line(&g), "Hi Bo!");
}

#[test]
fn the_ok_button_confirms_and_clicks_elsewhere_do_not() {
    let p = parse(ASK).unwrap();
    let mut g = at_input(&p);
    type_in(&mut g, "Kim");
    click(&mut g, crate::input::field_rect(&Theme::default()));
    click(&mut g, Rect::new(10.0, 10.0, 5.0, 5.0));
    assert_eq!((g.screen(), field(&g)), (Screen::Input, "Kim".to_string()));
    click(&mut g, crate::input::ok_rect(&Theme::default()));
    assert_eq!(g.screen(), Screen::Story);
    assert_eq!(line(&g), "Hi Kim!");
}

#[test]
fn ok_is_dimmed_only_while_there_is_nothing_to_confirm() {
    let p = parse(NO_DEFAULT).unwrap();
    let mut g = at_input(&p);
    let ok_alpha = |g: &Game| {
        g.draw_list(&|_| None)
            .iter()
            .find_map(|d| match d {
                Draw::Rect { rect, color } if *rect == crate::input::ok_rect(g.theme()) => Some(color[3]),
                _ => None,
            })
            .unwrap()
    };
    let dim = ok_alpha(&g);
    type_in(&mut g, "x");
    assert!(ok_alpha(&g) > dim);
    key(&mut g, Key::Backspace);
    assert_eq!(ok_alpha(&g), dim);
}

#[test]
fn escape_and_right_click_open_settings_and_closing_returns_with_the_text_kept() {
    let p = parse(ASK).unwrap();
    let mut g = at_input(&p);
    type_in(&mut g, "Sa");
    key(&mut g, Key::Escape);
    assert_eq!(g.screen(), Screen::Settings);
    assert!(!g.wants_text());
    type_in(&mut g, "zzz");
    key(&mut g, Key::Escape);
    assert_eq!(g.screen(), Screen::Input);
    assert_eq!(field(&g), "Sa", "typed text kept, nothing added behind the settings");
    g.handle(Event::Press(Button::Right));
    assert_eq!(g.screen(), Screen::Settings);
    g.handle(Event::Press(Button::Right));
    assert_eq!(g.screen(), Screen::Input);
    type_in(&mut g, "m");
    key(&mut g, Key::Enter);
    assert_eq!(line(&g), "Hi Sam!");
}

#[test]
fn skip_and_auto_forward_stop_at_the_input() {
    let p = parse(ASK).unwrap();
    // skip: with "skip unread" on, Tab runs through the line and stops at the input
    let mut g = new_game(&p, Settings { skip_unread: true, ..instant() });
    key(&mut g, Key::Tab);
    assert!(g.skipping());
    for _ in 0..20 {
        g.tick(0.1, false);
    }
    assert_eq!(g.screen(), Screen::Input);
    assert!(!g.skipping(), "skip stopped at the input");
    // auto-forward: it waits at the input however long it takes
    let mut g = new_game(&p, instant());
    key(&mut g, Key::Char('a'));
    assert!(g.auto());
    for _ in 0..200 {
        g.tick(0.5, false);
    }
    assert_eq!(g.screen(), Screen::Input);
    for _ in 0..200 {
        g.tick(0.5, false);
    }
    assert_eq!(g.screen(), Screen::Input);
    assert_eq!(g.state().vars["hero.name"], Value::Str("nobody".into()));
}

#[test]
fn arriving_at_the_input_is_an_auto_save_and_the_answer_is_not_in_the_history() {
    let p = parse(ASK).unwrap();
    let mut g = new_game(&p, instant());
    assert!(g.storage.read("auto-1.save").is_none());
    key(&mut g, Key::Space);
    assert!(g.storage.read("auto-1.save").is_some(), "a checkpoint, like a choice");
    type_in(&mut g, "Sam");
    key(&mut g, Key::Enter);
    key(&mut g, Key::Char('h'));
    assert_eq!(g.screen(), Screen::Backlog);
    let lines: Vec<String> = g
        .backlog
        .iter()
        .map(|l| match l {
            Logged::Line(l) => plain(&l.text),
            Logged::Choice(c) => c.clone(),
        })
        .collect();
    assert_eq!(lines, ["First.", "Hi Sam!"]);
}

#[test]
fn rolling_back_from_the_next_line_lands_on_the_prompt_with_an_empty_field() {
    let p = parse(ASK).unwrap();
    let mut g = at_input(&p);
    type_in(&mut g, "Sam");
    key(&mut g, Key::Enter);
    assert_eq!(line(&g), "Hi Sam!");
    key(&mut g, Key::Backspace); // in the story, Backspace goes back
    assert_eq!(g.screen(), Screen::Input);
    assert_eq!(field(&g), "");
    assert_eq!(g.state().vars["hero.name"], Value::Str("nobody".into()));
    type_in(&mut g, "Kim");
    key(&mut g, Key::Enter);
    assert_eq!(line(&g), "Hi Kim!");
}

#[test]
fn loading_a_save_made_at_the_input_opens_it_with_an_empty_field() {
    let p = parse(ASK).unwrap();
    let mut g = at_input(&p);
    type_in(&mut g, "Sam");
    key(&mut g, Key::F5);
    key(&mut g, Key::Enter);
    assert_eq!(line(&g), "Hi Sam!");
    key(&mut g, Key::F9);
    key(&mut g, Key::Char('y'));
    assert_eq!(g.screen(), Screen::Input);
    assert_eq!(field(&g), "", "what was typed isn't saved");
    type_in(&mut g, "Lee");
    key(&mut g, Key::Enter);
    assert_eq!(line(&g), "Hi Lee!");
}

#[test]
fn the_caret_blinks_and_stays_solid_while_typing() {
    let p = parse(ASK).unwrap();
    let mut g = at_input(&p);
    type_in(&mut g, "Sam");
    let shown = |g: &Game| texts(g).into_iter().find(|t| t.starts_with("Sam")).unwrap();
    assert_eq!(shown(&g), "Sam|");
    g.tick(0.6, false);
    assert_eq!(shown(&g), "Sam", "off half of the blink");
    g.tick(0.5, false);
    assert_eq!(shown(&g), "Sam|");
    g.tick(0.6, false);
    type_in(&mut g, "!");
    assert_eq!(shown(&g), "Sam!|", "a keystroke makes it solid again");
}

#[test]
fn a_reload_keeps_the_typed_text_and_a_script_without_the_input_returns_to_the_story() {
    let p = parse(ASK).unwrap();
    let mut g = at_input(&p);
    type_in(&mut g, "Sam");
    g.reload(&p).unwrap();
    assert_eq!((g.screen(), field(&g)), (Screen::Input, "Sam".to_string()));
    // under Settings too
    key(&mut g, Key::Escape);
    g.reload(&p).unwrap();
    key(&mut g, Key::Escape);
    assert_eq!((g.screen(), field(&g)), (Screen::Input, "Sam".to_string()));
    let shorter = parse("---\nvars: { hero.name: \"nobody\" }\n---\n**A:** First.\n**A:** Moved on.\n").unwrap();
    g.reload(&shorter).unwrap();
    assert_eq!(g.screen(), Screen::Story);
    assert!(!g.wants_text());
    // a script that lowers the limit cuts the text
    let p2 = parse(&ASK.replace("max=8", "max=2").replace(" default=\"Alex\"", "")).unwrap();
    let mut g = at_input(&p);
    type_in(&mut g, "Sam");
    g.reload(&p2).unwrap();
    assert_eq!(field(&g), "Sa");
}

#[test]
fn the_wheel_and_other_keys_do_nothing_but_f5_saves() {
    let p = parse(ASK).unwrap();
    let mut g = at_input(&p);
    g.handle(Event::Wheel(-1.0));
    g.handle(Event::Wheel(1.0));
    key(&mut g, Key::Tab);
    assert_eq!(g.screen(), Screen::Input);
    key(&mut g, Key::F5);
    assert!(g.storage.read("quick-1.save").is_some());
    assert_eq!(g.screen(), Screen::Input);
}

#[test]
fn the_greeting_demo_plays_with_a_typed_name() {
    let p = parse(include_str!("../../../../examples/greeting.quill.md")).unwrap();
    for (typed, shown) in [("Sam", "Sam"), ("", "Alex"), ("{hero.name}*x*", "{hero.name}*x*"), ("  Zoe  ", "Zoe")] {
        let mut g = new_game(&p, instant());
        key(&mut g, Key::Space); // past Mika's first line, to the question
        assert_eq!(g.screen(), Screen::Input, "{typed:?}");
        type_in(&mut g, typed);
        key(&mut g, Key::Enter);
        assert_eq!(line(&g), format!("Nice to meet you, {shown}!"));
        key(&mut g, Key::Space); // the player's own line: the name plate is the typed name
        assert_eq!(g.view().dialogue.as_ref().and_then(|d| d.speaker.clone()).as_deref(), Some(shown));
        key(&mut g, Key::Space);
        assert!(line(&g).contains(shown), "{}", line(&g));
        key(&mut g, Key::Space);
        key(&mut g, Key::Char('1')); // ask what the club does
        assert!(line(&g).contains(shown), "{}", line(&g));
    }
}
