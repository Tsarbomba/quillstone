//! `::filter` in the runtime: state, redraw, rollback, saves, hostile saves.

use quillstone_script::{parse, Filter, Program};

use crate::test_util::replace_records;
use crate::{Cmd, Input, Session};

const SRC: &str = "\
::filter{gray=1 fade=3}
One.
::filter{sepia=0.8 contrast=1.1}
Two.
::filter{none fade=2}
::filter{off}
Three.
";

fn program() -> Program {
    parse(SRC).unwrap()
}

fn gray() -> Filter {
    Filter { gray: 1.0, ..Filter::default() }
}

fn sepia() -> Filter {
    Filter { sepia: 0.8, contrast: 1.1, ..Filter::default() }
}

#[test]
fn a_filter_is_scene_state_and_removing_nothing_says_nothing() {
    let p = program();
    let (mut game, first) = Session::start(&p);
    assert_eq!(game.state().filter, Some(gray()));
    assert!(first.contains(&Cmd::Filter { set: Some(gray()), fade: 3.0 }));
    let cmds = game.input(Input::Advance);
    assert_eq!(game.state().filter, Some(sepia()), "it replaces, not merges");
    assert!(cmds.contains(&Cmd::Filter { set: Some(sepia()), fade: 0.0 }));
    let cmds = game.input(Input::Advance);
    assert_eq!(game.state().filter, None);
    assert!(cmds.contains(&Cmd::Filter { set: None, fade: 2.0 }));
    assert_eq!(cmds.iter().filter(|c| matches!(c, Cmd::Filter { .. })).count(), 1, "the second `off` has nothing to remove");
}

#[test]
fn a_redraw_sends_it_at_once_and_only_when_there_is_one() {
    let p = program();
    let (mut game, _) = Session::start(&p);
    assert!(game.state().redraw().contains(&Cmd::Filter { set: Some(gray()), fade: 0.0 }));
    game.input(Input::Advance);
    game.input(Input::Advance);
    assert!(!game.state().redraw().iter().any(|c| matches!(c, Cmd::Filter { .. })));
}

#[test]
fn rollback_brings_it_back() {
    let p = program();
    let (mut game, _) = Session::start(&p);
    game.input(Input::Advance);
    game.input(Input::Advance);
    let cmds = game.rollback().unwrap();
    assert_eq!(game.state().filter, Some(sepia()));
    assert!(cmds.contains(&Cmd::Filter { set: Some(sepia()), fade: 0.0 }));
    game.rollback().unwrap();
    assert_eq!(game.state().filter, Some(gray()));
}

#[test]
fn a_save_keeps_it_without_its_fade() {
    let p = program();
    let (mut game, _) = Session::start(&p);
    for expected in [Some(gray()), Some(sepia()), None] {
        let (loaded, cmds) = Session::load(&p, &game.save()).unwrap();
        assert_eq!(loaded.state().filter, expected);
        assert!(cmds.iter().all(|c| !matches!(c, Cmd::Filter { fade, .. } if *fade != 0.0)));
        assert_eq!(cmds.iter().any(|c| matches!(c, Cmd::Filter { .. })), expected.is_some());
        game.input(Input::Advance);
    }
    let odd = parse("::filter{gray=0.3 sepia=0.125 brightness=0.7 contrast=2.5 saturation=0.4}\nHi.\n").unwrap();
    let (game, _) = Session::start(&odd);
    let (loaded, _) = Session::load(&odd, &game.save()).unwrap();
    assert_eq!(loaded.state().filter, game.state().filter);
    assert!(loaded.state().filter.is_some());
}

#[test]
fn a_save_from_before_filters_loads_none() {
    let p = program();
    let (game, _) = Session::start(&p);
    let old: String = game.save().lines().filter(|l| !l.starts_with("filter ")).map(|l| format!("{l}\n")).collect();
    assert_ne!(old, game.save());
    let (loaded, _) = Session::load(&p, &old).unwrap();
    assert_eq!(loaded.state().filter, None);
}

#[test]
fn hostile_records_are_refused_or_made_safe() {
    let p = program();
    let (game, _) = Session::start(&p);
    let save = game.save();
    let refused = |rec: &str| Session::load(&p, &replace_records(&save, "filter ", &[rec])).is_err();
    assert!(refused("filter"), "nothing");
    assert!(refused("filter 1 0 1 1"), "four numbers");
    assert!(refused("filter 1 0 1 1 high"));
    assert!(refused("filter gray 0 1 1 1"));
    let loaded = |rec: &str| Session::load(&p, &replace_records(&save, "filter ", &[rec])).unwrap().0.state().filter.unwrap();
    // Numbers are clamped, NaN falls back to neutral.
    assert_eq!(loaded("filter 9 -4 50 -2 7"), Filter { gray: 1.0, sepia: 0.0, brightness: 3.0, contrast: 0.0, saturation: 3.0 });
    assert_eq!(loaded("filter NaN inf NaN NaN -inf"), Filter::default());
    let (twice, _) = Session::load(&p, &replace_records(&save, "filter ", &["filter 1 0 1 1 1", "filter 0 1 1 1 1"])).unwrap();
    assert_eq!(twice.state().filter, Some(Filter { sepia: 1.0, ..Filter::default() }), "the last wins");
}

#[test]
fn the_tester_has_nothing_to_say() {
    let r = crate::explore(&program(), 1000);
    assert!(r.warnings.is_empty(), "{:?}", r.warnings);
}
