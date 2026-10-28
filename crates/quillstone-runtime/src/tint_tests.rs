//! `::tint` in the runtime: state, redraw, rollback, saves, hostile saves.

use quillstone_script::{parse, Program, Tint};

use crate::{Cmd, Input, Session};

const SRC: &str = "\
::tint{color=#ff0000 alpha=0.5 fade=3}
One.
::tint{color=#0000ff}
Two.
::tint{none fade=2}
::tint{off}
Three.
";

fn program() -> Program {
    parse(SRC).unwrap()
}

fn red() -> Tint {
    Tint { color: [1.0, 0.0, 0.0], alpha: 0.5 }
}

fn blue() -> Tint {
    Tint { color: [0.0, 0.0, 1.0], alpha: 0.4 }
}

#[test]
fn a_tint_is_scene_state_and_removing_nothing_says_nothing() {
    let p = program();
    let (mut game, first) = Session::start(&p);
    assert_eq!(game.state().tint, Some(red()));
    assert!(first.contains(&Cmd::Tint { set: Some(red()), fade: 3.0 }));
    let cmds = game.input(Input::Advance);
    assert_eq!(game.state().tint, Some(blue()));
    assert!(cmds.contains(&Cmd::Tint { set: Some(blue()), fade: 0.0 }));
    let cmds = game.input(Input::Advance);
    assert_eq!(game.state().tint, None);
    assert!(cmds.contains(&Cmd::Tint { set: None, fade: 2.0 }));
    assert_eq!(cmds.iter().filter(|c| matches!(c, Cmd::Tint { .. })).count(), 1, "the second `off` has nothing to remove");
}

#[test]
fn a_redraw_sends_it_at_once_and_only_when_there_is_one() {
    let p = program();
    let (mut game, _) = Session::start(&p);
    assert!(game.state().redraw().contains(&Cmd::Tint { set: Some(red()), fade: 0.0 }));
    game.input(Input::Advance);
    game.input(Input::Advance);
    assert!(!game.state().redraw().iter().any(|c| matches!(c, Cmd::Tint { .. })));
}

#[test]
fn rollback_brings_it_back() {
    let p = program();
    let (mut game, _) = Session::start(&p);
    game.input(Input::Advance);
    game.input(Input::Advance);
    let cmds = game.rollback().unwrap();
    assert_eq!(game.state().tint, Some(blue()));
    assert!(cmds.contains(&Cmd::Tint { set: Some(blue()), fade: 0.0 }));
    game.rollback().unwrap();
    assert_eq!(game.state().tint, Some(red()));
}

#[test]
fn a_save_keeps_it_and_the_exact_colour() {
    let p = program();
    let (mut game, _) = Session::start(&p);
    for expected in [Some(red()), Some(blue()), None] {
        let (loaded, cmds) = Session::load(&p, &game.save()).unwrap();
        assert_eq!(loaded.state().tint, expected);
        assert!(cmds.iter().all(|c| !matches!(c, Cmd::Tint { fade, .. } if *fade != 0.0)));
        assert_eq!(cmds.iter().any(|c| matches!(c, Cmd::Tint { .. })), expected.is_some());
        game.input(Input::Advance);
    }
    let odd = parse("::tint{color=#203060 alpha=0.35}\nHi.\n").unwrap();
    let (game, _) = Session::start(&odd);
    let (loaded, _) = Session::load(&odd, &game.save()).unwrap();
    assert_eq!(loaded.state().tint, game.state().tint);
}

#[test]
fn a_save_from_before_tints_loads_none() {
    let p = program();
    let (game, _) = Session::start(&p);
    let old: String = game.save().lines().filter(|l| !l.starts_with("tint ")).map(|l| format!("{l}\n")).collect();
    assert_ne!(old, game.save());
    let (loaded, _) = Session::load(&p, &old).unwrap();
    assert_eq!(loaded.state().tint, None);
}

/// The first state's `tint` records, replaced by `lines`.
fn with_records(save: &str, lines: &[&str]) -> String {
    let mut out = String::new();
    let mut done = false;
    for l in save.lines().filter(|l| !l.starts_with("tint ")) {
        out.push_str(l);
        out.push('\n');
        if l.starts_with("rng ") && !done {
            done = true;
            for r in lines {
                out.push_str(r);
                out.push('\n');
            }
        }
    }
    out
}

#[test]
fn hostile_records_are_refused_or_made_safe() {
    let p = program();
    let (game, _) = Session::start(&p);
    let save = game.save();
    let refused = |rec: &str| Session::load(&p, &with_records(&save, &[rec])).is_err();
    assert!(refused("tint"), "nothing");
    assert!(refused("tint 1,0,0"), "no alpha");
    assert!(refused("tint 1,0 0.5"), "two components");
    assert!(refused("tint 1,0,0,0 0.5"), "four components");
    assert!(refused("tint red 0.5"));
    assert!(refused("tint 1,0,0 heavy"));
    assert!(refused("tint - 0.5"), "no colour");
    let loaded = |rec: &str| Session::load(&p, &with_records(&save, &[rec])).unwrap().0.state().tint.unwrap();
    // Numbers are clamped, NaN falls back.
    let big = loaded("tint 9,-4,0.5 5");
    assert_eq!((big.color, big.alpha), ([1.0, 0.0, 0.5], 0.9));
    let neg = loaded("tint 0,0,0 -3");
    assert_eq!(neg.alpha, 0.0);
    let nan = loaded("tint NaN,inf,0.2 NaN");
    assert_eq!((nan.color, nan.alpha), ([0.0, 0.0, 0.2], 0.0));
    // Two records are one setting: the last wins.
    let (twice, _) = Session::load(&p, &with_records(&save, &["tint 1,0,0 0.5", "tint 0,1,0 0.6"])).unwrap();
    assert_eq!(twice.state().tint, Some(Tint { color: [0.0, 1.0, 0.0], alpha: 0.6 }));
}

#[test]
fn the_tester_has_nothing_to_say() {
    let r = crate::explore(&program(), 1000);
    assert!(r.warnings.is_empty(), "{:?}", r.warnings);
}
