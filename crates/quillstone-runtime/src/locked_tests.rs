//! Locked options (`{if .., show}` / `{if .., show="reason"}`): drawn greyed while the condition is false,
//! never pickable, and never a way to hang the story.

use quillstone_script::{parse, Catalog, Program};

use crate::explore::explore;
use crate::{choice_timer, ChoiceTimer, Cmd, Input, Session};

const SRC: &str = "\
---
vars: { str: 2, gold: 0 }
---
# start
::choice
- [Open](#open)
- [Lift the gate](#gate){if str >= 5, show=\"Needs STR 5, you have {str}\"}
- [Pick the lock](#gate){if gold > 0, show}
- [Hidden one](#gate){if gold > 99}
::
# open
Opened.
::end
# gate
Gate.
::end
";

fn choices(s: &Session) -> (Vec<String>, Vec<Option<String>>) {
    match &s.state().pending {
        Some(Cmd::ShowChoices { options, locked }) => (options.clone(), locked.clone()),
        other => panic!("not at choices: {other:?}"),
    }
}

#[test]
fn a_locked_option_is_listed_with_its_reason_and_a_hidden_one_is_not() {
    let p = parse(SRC).unwrap();
    let (s, _) = Session::start(&p);
    let (options, locked) = choices(&s);
    assert_eq!(options, ["Open", "Lift the gate", "Pick the lock"]);
    // the reason has its `{str}` filled in; the flag form has an empty reason
    assert_eq!(locked, [None, Some("Needs STR 5, you have 2".into()), Some(String::new())]);
}

#[test]
fn a_locked_option_cannot_be_picked_and_is_pickable_once_its_condition_holds() {
    let p = parse(SRC).unwrap();
    let (mut s, _) = Session::start(&p);
    assert!(s.input(Input::Choose(1)).is_empty());
    assert!(s.input(Input::Choose(2)).is_empty());
    assert!(matches!(s.state().pending, Some(Cmd::ShowChoices { .. })), "still waiting");

    let strong = parse(&SRC.replace("str: 2", "str: 5")).unwrap();
    let (mut s, _) = Session::start(&strong);
    assert_eq!(choices(&s).1, [None, None, Some(String::new())]);
    let cmds = s.input(Input::Choose(1));
    assert!(cmds.iter().any(|c| matches!(c, Cmd::Say { text, .. } if text == "Gate.")));
}

#[test]
fn the_timed_default_is_never_a_locked_option() {
    let timed = |opts: &str| {
        let p: &'static Program = Box::leak(Box::new(parse(&format!("---\nvars: {{ k: 0 }}\n---\n::choice{{time=5}}\n{opts}::\n# a\nA.\n")).unwrap()));
        let (s, _) = Session::start(p);
        choice_timer(s.state(), p)
    };
    // the marked default is locked: the first pickable option stands in for it
    let t = timed("- [A](#a){if k > 0, show}\n- [B](#a)\n- [C](#a){default, if k > 0, show}\n");
    assert_eq!(t, Some(ChoiceTimer { seconds: 5.0, default: 1 }));
    // an unlocked default keeps its place among the listed options
    let t = timed("- [A](#a){if k > 0, show}\n- [B](#a)\n- [C](#a){default}\n");
    assert_eq!(t, Some(ChoiceTimer { seconds: 5.0, default: 2 }));
}

#[test]
fn a_choice_with_only_locked_options_is_skipped_like_one_with_nothing_on_offer() {
    let p = parse("---\nvars: { k: 0 }\n---\n::choice\n- [A](#x){if k > 0, show=\"???\"}\n- [B](#x){if k > 1, show}\n::\nAfter.\n::end\n# x\nX.\n").unwrap();
    let (s, cmds) = Session::start(&p);
    assert!(cmds.iter().any(|c| matches!(c, Cmd::Say { text, .. } if text == "After.")));
    assert!(!matches!(s.state().pending, Some(Cmd::ShowChoices { .. })));
    // and the tester agrees: no hang, no pickable option
    let r = explore(&p, 1000);
    assert!(r.errors.is_empty(), "{:?}", r.errors);
}

#[test]
fn a_once_option_already_picked_stays_hidden_not_greyed() {
    let menu = |once: &str| {
        let src = format!("---\nvars: {{ k: 1 }}\n---\n# a\n::choice\n- [Ask](#a){{{once}k = 0, if k > 0, show=\"Later\"}}\n- [Go](#b)\n::\n# b\nB.\n");
        let p: &'static Program = Box::leak(Box::new(parse(&src).unwrap()));
        let (mut s, _) = Session::start(p);
        assert_eq!(choices(&s).1, [None, None], "open at first");
        s.input(Input::Choose(0)); // picking it sets k = 0 and loops back to the menu
        choices(&s)
    };
    assert_eq!(menu(""), (vec!["Ask".into(), "Go".into()], vec![Some("Later".into()), None]), "no `once`: greyed");
    assert_eq!(menu("once, "), (vec!["Go".into()], vec![None]), "`once`: gone for good");
}

#[test]
fn a_save_at_a_locked_choice_comes_back_with_the_lock() {
    let p = parse(SRC).unwrap();
    let (s, _) = Session::start(&p);
    let (loaded, cmds) = Session::load(&p, &s.save()).unwrap();
    assert_eq!(choices(&loaded), choices(&s));
    assert!(cmds.iter().any(|c| matches!(c, Cmd::ShowChoices { locked, .. } if locked[1].is_some())));
}

#[test]
fn the_reason_is_in_the_translation_catalog_and_is_translated_with_its_variables() {
    let p = parse(SRC).unwrap();
    let texts: Vec<String> = p.translatable().into_iter().map(|t| t.text).collect();
    assert!(texts.contains(&"Needs STR 5, you have {str}".to_string()));
    assert!(!texts.iter().any(|t| t.is_empty()), "a flag `show` has no reason to translate");
    let sv = Catalog::parse("> Lift the gate\n= Lyft porten\n\n> Needs STR 5, you have {str}\n= Kräver STR 5, du har {str}\n").unwrap();
    let (s, cmds) = Session::start(&p);
    let cmds = s.localize(cmds, &sv);
    assert!(cmds.iter().any(|c| matches!(c, Cmd::ShowChoices { options, locked }
        if options[1] == "Lyft porten" && locked[1].as_deref() == Some("Kräver STR 5, du har 2") && locked[2].as_deref() == Some(""))));
}

#[test]
fn the_tester_never_picks_a_locked_option_and_says_one_that_stays_locked_is_never_offered() {
    let p = parse(SRC).unwrap();
    let r = explore(&p, 1000);
    // "Lift the gate" and "Pick the lock" stay locked in every playthrough: #gate is unreachable
    let warnings: Vec<&str> = r.warnings.iter().map(|w| w.message.as_str()).collect();
    assert!(warnings.iter().any(|m| m.contains("Lift the gate") && m.contains("never offered")), "{warnings:?}");
    assert!(!r.endings.is_empty() && r.errors.is_empty());
}
