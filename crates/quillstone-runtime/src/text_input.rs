//! Tests for `::input`: the answer's clean-up, the default, the limit, rollback,
//! saves, kept values, the story tester and text that looks like markup.

use quillstone_script::markup::rich;
use quillstone_script::{parse, Program, Value};

use crate::test_util::says;
use crate::{explore::explore, finish_input, interpolate, read_persistent, write_persistent, Cmd, Input, Session};

const STORY: &str = "---\nvars: { hero.name: \"nobody\" }\n---\n# start\n**A:** Before.\n::input{var=hero.name prompt=\"Your name, {hero.name}?\" default=\"Alex\" max=8}\n**A:** Hello {hero.name}!\n";

fn ask(cmds: &[Cmd]) -> Option<(String, Option<String>, usize)> {
    cmds.iter().find_map(|c| match c {
        Cmd::AskInput { prompt, default, max } => Some((prompt.clone(), default.clone(), *max)),
        _ => None,
    })
}

fn name(s: &Session) -> Value {
    s.state().vars["hero.name"].clone()
}

fn at_prompt(p: &Program) -> Session<'_> {
    let (mut s, _) = Session::start(p);
    let cmds = s.input(Input::Advance);
    assert!(ask(&cmds).is_some(), "{cmds:?}");
    s
}

// ---- the clean-up ---------------------------------------------------------------

#[test]
fn an_answer_is_cleaned_trimmed_capped_and_falls_back_to_the_default() {
    assert_eq!(finish_input("Sam", None, 8).as_deref(), Some("Sam"));
    assert_eq!(finish_input("  Sam \t", None, 8).as_deref(), Some("Sam"), "spaces trimmed at both ends");
    assert_eq!(finish_input("S\na\tm\u{7}\r", None, 8).as_deref(), Some("Sam"), "control characters dropped");
    assert_eq!(finish_input("Samuel Jackson", None, 6).as_deref(), Some("Samuel"));
    assert_eq!(finish_input("Sam Jackson", None, 4).as_deref(), Some("Sam"), "cut, then the space at the cut trimmed");
    assert_eq!(finish_input("a  b", None, 8).as_deref(), Some("a  b"), "inner spaces stay");
    // characters, not bytes
    assert_eq!(finish_input("名前名前名前", None, 4).as_deref(), Some("名前名前"));
    assert_eq!(finish_input("héllo wörld", None, 5).as_deref(), Some("héllo"));
    // empty: the default, or nothing
    assert_eq!(finish_input("", Some("Alex"), 8).as_deref(), Some("Alex"));
    assert_eq!(finish_input(" \n ", Some("Alex"), 8).as_deref(), Some("Alex"));
    assert_eq!(finish_input("", None, 8), None);
    assert_eq!(finish_input("   ", None, 8), None);
    assert_eq!(finish_input("\u{1}\u{2}", Some(" "), 8), None, "a default that is empty after clean-up is none");
}

// ---- the story ------------------------------------------------------------------

#[test]
fn the_story_waits_at_the_prompt_and_stores_the_answer() {
    let p = parse(STORY).unwrap();
    let (mut s, first) = Session::start(&p);
    assert!(says(&first, "Before.") && ask(&first).is_none());
    let cmds = s.input(Input::Advance);
    // the prompt is interpolated when it is shown, and its limit and default come along
    assert_eq!(ask(&cmds), Some(("Your name, nobody?".into(), Some("Alex".into()), 8)));
    assert!(matches!(s.state().pending, Some(Cmd::AskInput { .. })));
    // other input does nothing at the prompt
    assert!(s.input(Input::Advance).is_empty() && s.input(Input::Choose(0)).is_empty() && s.input(Input::Buy(0)).is_empty());
    let cmds = s.input(Input::Text("  Sam\n".into()));
    assert_eq!(name(&s), Value::Str("Sam".into()));
    assert!(says(&cmds, "Hello Sam!"), "{cmds:?}");
}

#[test]
fn an_empty_answer_takes_the_default_or_is_ignored() {
    let p = parse(STORY).unwrap();
    let mut s = at_prompt(&p);
    let cmds = s.input(Input::Text("   ".into()));
    assert_eq!(name(&s), Value::Str("Alex".into()));
    assert!(says(&cmds, "Hello Alex!"));

    let bare = parse("---\nvars: { n: \"-\" }\n---\n::input{var=n prompt=\"Name?\"}\n**A:** {n}\n").unwrap();
    let (mut s, first) = Session::start(&bare);
    assert!(ask(&first).is_some());
    assert!(s.input(Input::Text("".into())).is_empty() && s.input(Input::Text(" \t\n".into())).is_empty());
    assert!(matches!(s.state().pending, Some(Cmd::AskInput { .. })), "still waiting");
    assert_eq!(s.state().vars["n"], Value::Str("-".into()), "nothing stored");
    assert!(!s.can_rollback(), "an ignored answer is not a step");
    assert!(says(&s.input(Input::Text("x".into())), "x"));
}

#[test]
fn the_limit_counts_characters_and_a_number_stays_text() {
    let p = parse(STORY).unwrap();
    let mut s = at_prompt(&p);
    s.input(Input::Text("Bartholomew".into()));
    assert_eq!(name(&s), Value::Str("Bartholo".into()));
    let num = parse("---\nvars: { n: \"0\" }\n---\n::input{var=n prompt=\"Number?\" max=3}\n::if{n == \"123\"}\n**A:** text\n::endif\n").unwrap();
    let (mut s, _) = Session::start(&num);
    let cmds = s.input(Input::Text("12345".into()));
    assert_eq!(s.state().vars["n"], Value::Str("123".into()));
    assert!(says(&cmds, "text"), "compared as text: {cmds:?}");
}

#[test]
fn rollback_lands_on_the_prompt_and_the_next_answer_replaces_the_first() {
    let p = parse(STORY).unwrap();
    let mut s = at_prompt(&p);
    s.input(Input::Text("Sam".into()));
    assert_eq!(name(&s), Value::Str("Sam".into()));
    let redraw = s.rollback().unwrap();
    assert!(matches!(redraw.last(), Some(Cmd::AskInput { .. })), "{redraw:?}");
    assert_eq!(name(&s), Value::Str("nobody".into()), "the answer is taken back");
    assert!(says(&s.input(Input::Text("Kim".into())), "Hello Kim!"));
    // roll back twice then forward again: the answers come back as they were
    s.rollback();
    assert!(s.rollback().is_some());
    assert_eq!(name(&s), Value::Str("nobody".into()));
    s.roll_forward();
    s.roll_forward();
    assert_eq!(name(&s), Value::Str("Kim".into()));
}

#[test]
fn the_prompt_and_the_backlog_hold_no_answer() {
    let p = parse(STORY).unwrap();
    let mut s = at_prompt(&p);
    s.input(Input::Text("Sam".into()));
    let log: Vec<String> = s
        .backlog()
        .iter()
        .map(|l| match l {
            crate::Logged::Line(l) => l.text.clone(),
            crate::Logged::Choice(c) => format!("choice {c}"),
        })
        .collect();
    assert_eq!(log, ["Before.", "Hello Sam!"], "the question and the answer are not in the history");
}

// ---- saves ------------------------------------------------------------------------

#[test]
fn a_save_at_the_prompt_rebuilds_it_and_keeps_the_history() {
    let p = parse(STORY).unwrap();
    let mut s = at_prompt(&p);
    let text = s.save();
    assert!(text.contains("pending input"), "{text}");
    assert!(!text.contains("Your name"), "the prompt is read from the script, not stored");
    let (mut loaded, cmds) = Session::load(&p, &text).unwrap();
    assert_eq!(ask(&cmds), Some(("Your name, nobody?".into(), Some("Alex".into()), 8)));
    assert!(says(&loaded.input(Input::Text("Sam".into())), "Hello Sam!"));
    // rolling back after loading still works through the prompt
    assert!(loaded.rollback().is_some() && matches!(loaded.state().pending, Some(Cmd::AskInput { .. })));
    s.input(Input::Text("Z".into()));
}

#[test]
fn history_inputs_are_saved_with_proper_quoting() {
    let p = parse(STORY).unwrap();
    let mut s = at_prompt(&p);
    // The limit is 8: a quote, a backslash and a brace all fit.
    s.input(Input::Text("a\"b\\{c}*".into()));
    let text = s.save();
    let record = text.lines().find(|l| l.starts_with("input text")).unwrap_or_else(|| panic!("{text}"));
    assert_eq!(record, "input text \"a\\\"b\\\\{c}*\"");
    let (mut loaded, _) = Session::load(&p, &text).unwrap();
    assert_eq!(name(&loaded), Value::Str("a\"b\\{c}*".into()), "the variable comes back as typed");
    assert!(loaded.rollback().is_some(), "history entries read back");
    assert_eq!(name(&loaded), Value::Str("nobody".into()));
    // a text input with a hostile record is cleaned, not trusted
    let hostile = text.replace(record, "input text \"x\\ny\\u{0}zzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzz\"");
    assert!(Session::load(&p, &hostile).is_ok());
}

#[test]
fn a_save_from_before_the_input_existed_runs_on_to_it() {
    let old = parse("# start\n**A:** One.\n**A:** Two.\n**A:** Three.\n").unwrap();
    let (mut s, _) = Session::start(&old);
    s.input(Input::Advance);
    let save = s.save(); // waiting at "Two."
    let new = parse("---\nvars: { hero.name: \"nobody\" }\n---\n# start\n**A:** One.\n::input{var=hero.name prompt=\"Name?\"}\n**A:** Two.\n**A:** Three.\n").unwrap();
    let (loaded, cmds) = Session::load(&new, &save).unwrap();
    assert!(ask(&cmds).is_some(), "the story arrives at the new input: {cmds:?}");
    assert!(matches!(loaded.state().pending, Some(Cmd::AskInput { .. })));
}

#[test]
fn a_save_that_waited_at_an_input_that_is_gone_moves_on() {
    let with = parse(STORY).unwrap();
    let save = at_prompt(&with).save();
    let without = parse("---\nvars: { hero.name: \"nobody\" }\n---\n# start\n**A:** Before.\n**A:** After.\n").unwrap();
    let (loaded, cmds) = Session::load(&without, &save).unwrap();
    assert!(matches!(loaded.state().pending, Some(Cmd::Say { .. })), "{cmds:?}");
}

#[test]
fn a_text_variable_is_kept_across_playthroughs_and_rollback_does_not_take_it_back() {
    let p = parse("---\npersistent: { hero.name: \"nobody\" }\n---\n::input{var=hero.name prompt=\"Name?\"}\n**A:** Hi {hero.name}.\n").unwrap();
    let (mut s, _) = Session::start(&p);
    s.input(Input::Text("Sam".into()));
    assert!(s.take_persistent_changed());
    let kept = read_persistent(&write_persistent(s.persistent()));
    assert_eq!(kept["hero.name"], Value::Str("Sam".into()));
    s.rollback();
    assert_eq!(name(&s), Value::Str("Sam".into()), "kept for good, like any persistent value");
    let (next, first) = Session::start_with(&p, kept);
    assert!(ask(&first).is_some(), "a new game still asks");
    assert_eq!(name(&next), Value::Str("Sam".into()));
}

// ---- the tester -------------------------------------------------------------------

#[test]
fn the_story_tester_answers_with_the_default_or_player_and_reports_nothing() {
    let r = explore(&parse(STORY).unwrap(), 1000);
    assert!(r.errors.is_empty() && r.warnings.is_empty(), "{r:#?}");
    assert!(!r.partial && r.endings.len() == 1);
    let bare = "---\nvars: { n: \"-\" }\n---\n::input{var=n prompt=\"Name?\" max=3}\n::if{n == \"Pla\"}\n**A:** cut to the limit\n::endif\n**A:** end\n";
    let r = explore(&parse(bare).unwrap(), 1000);
    assert!(r.errors.is_empty() && r.warnings.is_empty() && !r.partial && r.endings.len() == 1, "{r:#?}");
    // It also tries the texts the script compares the answer to, so a line only another answer reaches is played.
    let other = "---\nvars: { n: \"-\" }\n---\n::input{var=n prompt=\"Name?\" default=\"Alex\"}\n::if{n == \"Zed\"}\nZed only.\n::endif\nEnd.\n";
    let r = explore(&parse(other).unwrap(), 1000);
    assert!(r.warnings.is_empty() && r.errors.is_empty(), "{r:#?}");
    // a line the answer decides is still reached
    let r = explore(&parse("---\nvars: { n: \"-\" }\n---\n::input{var=n prompt=\"Name?\" default=\"Zed\"}\n::if{n == \"Zed\"}\nSeen.\n::endif\n").unwrap(), 1000);
    assert!(r.warnings.is_empty(), "{r:#?}");
}

#[test]
fn a_prompt_naming_an_unset_variable_is_flagged_like_a_line() {
    let r = explore(&parse("---\nvars: { n: \"-\" }\n---\n::input{var=n prompt=\"Hello {ghost}?\"}\n").unwrap(), 1000);
    assert!(r.warnings.iter().any(|w| w.message.contains("ghost")), "{r:#?}");
}

// ---- text that looks like markup --------------------------------------------------

#[test]
fn an_answer_with_braces_stars_and_backslashes_shows_literally() {
    let nasty = "{color=#f00}*x*\\{nw}{p}\\";
    let p = parse("---\nvars: { n: \"-\" }\n---\n::input{var=n prompt=\"Name?\" max=64}\n**A:** Hi {n}\n**A:** {n}\n").unwrap();
    let (mut s, _) = Session::start(&p);
    let cmds = s.input(Input::Text(nasty.into()));
    assert_eq!(s.state().vars["n"], Value::Str(nasty.into()), "stored exactly as typed");
    let Some(Cmd::Say { text, .. }) = cmds.last() else { panic!("{cmds:?}") };
    let r = rich(text);
    assert_eq!(r.text, format!("Hi {nasty}"));
    assert!(r.runs.is_empty() || r.runs.iter().all(|run| run.style == Default::default()), "no styles came from the answer");
    assert!(!r.no_wait && r.pauses.is_empty());
    // the same in a prompt
    let shown = interpolate("Hello {n}?", &s.state().vars);
    assert_eq!(rich(&shown).text, format!("Hello {nasty}?"));
    assert!(!rich(&shown).no_wait);
}

#[test]
fn the_greeting_demo_is_clean_for_the_story_tester() {
    let r = explore(&parse(include_str!("../../../examples/greeting.quill.md")).unwrap(), 10_000);
    assert!(r.errors.is_empty() && r.warnings.is_empty() && !r.partial, "{r:#?}");
}
