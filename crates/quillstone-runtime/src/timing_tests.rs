//! `::timing` in the runtime: the outcome is an input like a choice, so the variable, rollback, saves and
//! the story tester need nothing special. (Where the marker was is the frontend's business.)

use quillstone_script::{parse, Catalog, Program, Value};

use crate::explore::explore;
use crate::{Cmd, Input, Session};

fn program() -> Program {
    parse(
        "---\nvars: { hit: \"\", n: 1 }\n---\n# start\nBefore.\n\
         ::timing{result=hit good=40%..60% perfect=48%..52% time=5 prompt=\"Pick {n} lock!\"}\n\
         ::if{hit == \"perfect\"}\nClean.\n::elif{hit == \"good\"}\nGood.\n::else\nMissed.\n::endif\nAfter.\n",
    )
    .unwrap()
}

fn at_bar(p: &Program) -> Session<'_> {
    let (mut s, _) = Session::start(p);
    let cmds = s.input(Input::Advance);
    assert!(matches!(cmds.last(), Some(Cmd::ShowTiming { .. })), "{cmds:?}");
    s
}

fn says(cmds: &[Cmd], text: &str) -> bool {
    cmds.iter().any(|c| matches!(c, Cmd::Say { text: t, .. } if t == text))
}

fn hit(s: &Session) -> Value {
    s.state().vars["hit"].clone()
}

#[test]
fn the_bar_is_shown_with_its_prompt_filled_in_and_each_outcome_sets_the_variable() {
    let p = program();
    let (mut s, _) = Session::start(&p);
    let cmds = s.input(Input::Advance);
    let Some(Cmd::ShowTiming { prompt, bar }) = cmds.last() else { panic!("{cmds:?}") };
    assert_eq!((prompt.as_str(), bar.time), ("Pick 1 lock!", Some(5.0)));
    for (choose, word, line) in [(2, "perfect", "Clean."), (1, "good", "Good."), (0, "miss", "Missed.")] {
        let mut s = at_bar(&p);
        assert!(says(&s.input(Input::Choose(choose)), line), "{word}");
        assert_eq!(hit(&s), Value::Str(word.into()));
    }
}

#[test]
fn an_outcome_the_bar_cannot_have_is_ignored() {
    let p = parse("---\nvars: { hit: \"\" }\n---\n::timing{result=hit good=40%..60%}\nDone.\n").unwrap();
    let (mut s, first) = Session::start(&p);
    assert!(matches!(first.last(), Some(Cmd::ShowTiming { .. })));
    assert!(s.input(Input::Choose(2)).is_empty(), "no perfect zone");
    assert!(s.input(Input::Choose(9)).is_empty());
    assert!(s.input(Input::Advance).is_empty(), "a bar waits for a press, not for a click to advance");
    assert_eq!(hit(&s), Value::Str(String::new()));
    assert!(says(&s.input(Input::Choose(1)), "Done."));
}

#[test]
fn rollback_returns_to_the_bar_and_roll_forward_reuses_the_recorded_result() {
    let p = program();
    let mut s = at_bar(&p);
    assert!(says(&s.input(Input::Choose(2)), "Clean."));
    let back = s.rollback().unwrap();
    assert!(matches!(back.last(), Some(Cmd::ShowTiming { .. })), "{back:?}");
    assert_eq!(hit(&s), Value::Str(String::new()), "the result is taken back with it");
    // picking again can give another result
    assert!(says(&s.input(Input::Choose(0)), "Missed."));
    // a rollback straight after the first press, then forward: the first result comes back as it was
    let mut s = at_bar(&p);
    s.input(Input::Choose(1));
    s.rollback().unwrap();
    assert!(s.can_roll_forward());
    let fwd = s.roll_forward().unwrap();
    assert!(says(&fwd, "Good."), "{fwd:?}");
    assert_eq!(hit(&s), Value::Str("good".into()));
}

#[test]
fn a_save_at_the_bar_loads_at_the_bar_and_keeps_the_history() {
    let p = program();
    let s = at_bar(&p);
    let text = s.save();
    assert!(text.contains("pending timing") && !text.contains("lock"), "the bar is read from the script: {text}");
    let (mut loaded, cmds) = Session::load(&p, &text).unwrap();
    assert!(matches!(cmds.last(), Some(Cmd::ShowTiming { prompt, .. }) if prompt == "Pick 1 lock!"), "{cmds:?}");
    assert!(says(&loaded.input(Input::Choose(1)), "Good."));
    assert!(loaded.rollback().is_some() && matches!(loaded.state().pending, Some(Cmd::ShowTiming { .. })));
    // a save made after the press holds the result
    let (after, _) = Session::load(&p, &loaded.save()).unwrap_or_else(|e| panic!("{e}"));
    assert!(matches!(after.state().pending, Some(Cmd::ShowTiming { .. })));
    let mut done = at_bar(&p);
    done.input(Input::Choose(2));
    let (again, _) = Session::load(&p, &done.save()).unwrap();
    assert_eq!(hit(&again), Value::Str("perfect".into()));
}

#[test]
fn a_save_that_waited_at_a_bar_that_is_gone_moves_on() {
    let p = program();
    let save = at_bar(&p).save();
    let without = parse("---\nvars: { hit: \"\", n: 1 }\n---\n# start\nBefore.\n\nAfter.\n").unwrap();
    let (loaded, cmds) = Session::load(&without, &save).unwrap();
    assert!(matches!(loaded.state().pending, Some(Cmd::Say { .. })), "{cmds:?}");
}

#[test]
fn the_story_tester_plays_every_outcome_of_a_bar() {
    let p = parse(
        "---\nvars: { hit: \"\" }\n---\n::timing{result=hit good=40%..60% perfect=48%..52%}\n\
         ::if{hit == \"perfect\"}\n::jump{to=a}\n::elif{hit == \"good\"}\n::jump{to=b}\n::endif\n# c\nC.\n::end\n# a\nA.\n::end\n# b\nB.\n::end\n",
    )
    .unwrap();
    let r = explore(&p, 1000);
    assert!(r.errors.is_empty() && r.warnings.is_empty() && !r.partial, "{r:#?}");
    assert_eq!(r.endings.len(), 3, "perfect, good and miss each reach their own ending");
    // without a perfect zone there is one outcome less to play
    let story = |zones: &str| parse(&format!("---\nvars: {{ hit: \"\" }}\n---\n::timing{{result=hit {zones}}}\nDone.\n")).unwrap();
    let (two, three) = (explore(&story("good=40%..60%"), 1000), explore(&story("good=40%..60% perfect=48%..52%"), 1000));
    assert_eq!((two.situations, three.situations), (5, 7), "each outcome adds its own line and end");
}

#[test]
fn the_prompt_is_translated_like_an_input_prompt() {
    let p = program();
    let cat = Catalog::parse("> Pick {n} lock!\n= Dyrka {n} lasen!\n").unwrap();
    let mut s = at_bar(&p);
    let shown = s.relocalized(&cat);
    assert!(shown.iter().any(|c| matches!(c, Cmd::ShowTiming { prompt, .. } if prompt == "Dyrka 1 lasen!")), "{shown:?}");
    s.input(Input::Choose(0));
    assert!(p.translatable().iter().any(|t| t.text == "Pick {n} lock!"));
}

#[test]
fn the_tester_does_not_play_a_miss_a_bar_cannot_have() {
    // all of the bar is good and nothing times out: the `miss` branch is really never reached
    let src = |bar: &str| {
        format!("---\nvars: {{ hit: \"\" }}\n---\n::timing{{result=hit {bar}}}\n::if{{hit == \"miss\"}}\n::jump{{to=m}}\n::endif\nOk.\n::end\n# m\nMissed.\n::end\n")
    };
    let all = explore(&parse(&src("good=0%..100%")).unwrap(), 1000);
    assert!(all.warnings.iter().any(|w| w.message.contains("never reached")), "the miss label is reported never reached: {:?}", all.warnings);
    let timed = explore(&parse(&src("good=0%..100% time=5")).unwrap(), 1000);
    assert!(timed.warnings.is_empty(), "a time limit makes a miss possible: {:?}", timed.warnings);
}
