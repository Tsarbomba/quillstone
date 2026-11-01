//! `::press` in the runtime: the outcome is an input like a choice (0 miss, 1 hit), so the variable,
//! rollback, saves and the story tester need nothing special. (The presses are the frontend's business.)

use quillstone_script::{parse, Catalog, Program, Value};

use crate::explore::explore;
use crate::{Cmd, Input, Session};

fn program() -> Program {
    parse(
        "---\nvars: { ok: \"\", n: 2 }\n---\n# start\nBefore.\n\
         ::press{key=space time=1.2 count=3 result=ok prompt=\"Duck {n}!\"}\n\
         ::if{ok == \"hit\"}\nDodged.\n::else\nHit.\n::endif\nAfter.\n",
    )
    .unwrap()
}

fn at_prompt(p: &Program) -> Session<'_> {
    let (mut s, _) = Session::start(p);
    let cmds = s.input(Input::Advance);
    assert!(matches!(cmds.last(), Some(Cmd::ShowPress { .. })), "{cmds:?}");
    s
}

fn says(cmds: &[Cmd], text: &str) -> bool {
    cmds.iter().any(|c| matches!(c, Cmd::Say { text: t, .. } if t == text))
}

fn ok(s: &Session) -> Value {
    s.state().vars["ok"].clone()
}

#[test]
fn the_prompt_is_shown_with_its_text_filled_in_and_each_outcome_sets_the_variable() {
    let p = program();
    let (mut s, _) = Session::start(&p);
    let cmds = s.input(Input::Advance);
    let Some(Cmd::ShowPress { prompt, press }) = cmds.last() else { panic!("{cmds:?}") };
    assert_eq!((prompt.as_str(), press.key.as_str(), press.count, press.time), ("Duck 2!", "space", 3, 1.2));
    for (choose, word, line) in [(1, "hit", "Dodged."), (0, "miss", "Hit.")] {
        let mut s = at_prompt(&p);
        assert!(says(&s.input(Input::Choose(choose)), line), "{word}");
        assert_eq!(ok(&s), Value::Str(word.into()));
    }
    let mut s = at_prompt(&p);
    assert!(s.input(Input::Choose(2)).is_empty(), "only miss and hit");
    assert!(s.input(Input::Advance).is_empty(), "it waits for the presses, not for a click to advance");
}

#[test]
fn rollback_returns_to_the_prompt_and_roll_forward_reuses_the_recorded_result() {
    let p = program();
    let mut s = at_prompt(&p);
    assert!(says(&s.input(Input::Choose(1)), "Dodged."));
    let back = s.rollback().unwrap();
    assert!(matches!(back.last(), Some(Cmd::ShowPress { .. })), "{back:?}");
    assert_eq!(ok(&s), Value::Str(String::new()), "the result is taken back with it");
    assert!(s.can_roll_forward());
    assert!(says(&s.roll_forward().unwrap(), "Dodged."));
    assert_eq!(ok(&s), Value::Str("hit".into()));
}

#[test]
fn a_save_at_the_prompt_loads_at_the_prompt_and_a_gone_prompt_moves_on() {
    let p = program();
    let text = at_prompt(&p).save();
    assert!(text.contains("pending press") && !text.contains("Duck"), "the prompt is read from the script: {text}");
    let (mut loaded, cmds) = Session::load(&p, &text).unwrap();
    assert!(matches!(cmds.last(), Some(Cmd::ShowPress { prompt, .. }) if prompt == "Duck 2!"), "{cmds:?}");
    assert!(says(&loaded.input(Input::Choose(0)), "Hit."));
    let (after, _) = Session::load(&p, &loaded.save()).unwrap();
    assert_eq!(ok(&after), Value::Str("miss".into()));
    let without = parse("---\nvars: { ok: \"\", n: 2 }\n---\n# start\nBefore.\n\nAfter.\n").unwrap();
    let (moved, _) = Session::load(&without, &text).unwrap();
    assert!(matches!(moved.state().pending, Some(Cmd::Say { .. })));
}

#[test]
fn the_story_tester_plays_hit_and_miss_as_two_branches() {
    let p = parse(
        "---\nvars: { ok: \"\" }\n---\n::press{key=a time=1 result=ok}\n\
         ::if{ok == \"hit\"}\n::jump{to=win}\n::endif\n# lose\nLose.\n::end\n# win\nWin.\n::end\n",
    )
    .unwrap();
    let r = explore(&p, 1000);
    assert!(r.errors.is_empty() && r.warnings.is_empty() && !r.partial, "{r:#?}");
    assert_eq!(r.endings.len(), 2, "hit and miss reach their own ending");
}

#[test]
fn the_prompt_is_translated_like_an_input_prompt() {
    let p = program();
    let cat = Catalog::parse("> Duck {n}!\n= Uhyb {n}!\n").unwrap();
    let s = at_prompt(&p);
    let shown = s.relocalized(&cat);
    assert!(shown.iter().any(|c| matches!(c, Cmd::ShowPress { prompt, .. } if prompt == "Uhyb 2!")), "{shown:?}");
    assert!(p.translatable().iter().any(|t| t.text == "Duck {n}!"));
}
