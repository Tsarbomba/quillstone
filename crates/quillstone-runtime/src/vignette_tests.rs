//! `::vignette` in the runtime: state, redraw, rollback, saves, hostile saves.

use quillstone_script::{parse, Program, Vignette};

use crate::test_util::replace_records;
use crate::{Cmd, Input, Session};

const SRC: &str = "\
::vignette{strength=0.8 size=0.3 fade=3}
One.
::vignette{color=#ff0000}
Two.
::vignette{none fade=2}
::vignette{off}
Three.
";

fn program() -> Program {
    parse(SRC).unwrap()
}

fn dark() -> Vignette {
    Vignette { color: [0.0; 3], strength: 0.8, size: 0.3 }
}

fn red() -> Vignette {
    Vignette { color: [1.0, 0.0, 0.0], strength: 0.5, size: 0.5 }
}

#[test]
fn a_vignette_is_scene_state_and_removing_nothing_says_nothing() {
    let p = program();
    let (mut game, first) = Session::start(&p);
    assert_eq!(game.state().vignette, Some(dark()));
    assert!(first.contains(&Cmd::Vignette { set: Some(dark()), fade: 3.0 }));
    let cmds = game.input(Input::Advance);
    assert_eq!(game.state().vignette, Some(red()));
    assert!(cmds.contains(&Cmd::Vignette { set: Some(red()), fade: 0.0 }));
    let cmds = game.input(Input::Advance);
    assert_eq!(game.state().vignette, None);
    assert!(cmds.contains(&Cmd::Vignette { set: None, fade: 2.0 }));
    assert_eq!(cmds.iter().filter(|c| matches!(c, Cmd::Vignette { .. })).count(), 1, "the second `off` has nothing to remove");
}

#[test]
fn a_redraw_sends_it_at_once_and_only_when_there_is_one() {
    let p = program();
    let (mut game, _) = Session::start(&p);
    assert!(game.state().redraw().contains(&Cmd::Vignette { set: Some(dark()), fade: 0.0 }));
    game.input(Input::Advance);
    game.input(Input::Advance);
    assert!(!game.state().redraw().iter().any(|c| matches!(c, Cmd::Vignette { .. })));
}

#[test]
fn rollback_brings_it_back() {
    let p = program();
    let (mut game, _) = Session::start(&p);
    game.input(Input::Advance);
    game.input(Input::Advance);
    let cmds = game.rollback().unwrap();
    assert_eq!(game.state().vignette, Some(red()));
    assert!(cmds.contains(&Cmd::Vignette { set: Some(red()), fade: 0.0 }));
    game.rollback().unwrap();
    assert_eq!(game.state().vignette, Some(dark()));
}

#[test]
fn a_save_keeps_it_and_the_exact_values() {
    let p = program();
    let (mut game, _) = Session::start(&p);
    for expected in [Some(dark()), Some(red()), None] {
        let (loaded, cmds) = Session::load(&p, &game.save()).unwrap();
        assert_eq!(loaded.state().vignette, expected);
        assert!(cmds.iter().all(|c| !matches!(c, Cmd::Vignette { fade, .. } if *fade != 0.0)));
        assert_eq!(cmds.iter().any(|c| matches!(c, Cmd::Vignette { .. })), expected.is_some());
        game.input(Input::Advance);
    }
    let odd = parse("::vignette{color=#203060 strength=0.35 size=0.12}\nHi.\n").unwrap();
    let (game, _) = Session::start(&odd);
    let (loaded, _) = Session::load(&odd, &game.save()).unwrap();
    assert_eq!(loaded.state().vignette, game.state().vignette);
}

#[test]
fn a_save_from_before_vignettes_loads_none() {
    let p = program();
    let (game, _) = Session::start(&p);
    let old: String = game.save().lines().filter(|l| !l.starts_with("vignette ")).map(|l| format!("{l}\n")).collect();
    assert_ne!(old, game.save());
    let (loaded, _) = Session::load(&p, &old).unwrap();
    assert_eq!(loaded.state().vignette, None);
}

#[test]
fn hostile_records_are_refused_or_made_safe() {
    let p = program();
    let (game, _) = Session::start(&p);
    let save = game.save();
    let refused = |rec: &str| Session::load(&p, &replace_records(&save, "vignette ", &[rec])).is_err();
    assert!(refused("vignette"), "nothing");
    assert!(refused("vignette 0,0,0 0.5"), "no size");
    assert!(refused("vignette 0,0 0.5 0.5"), "two components");
    assert!(refused("vignette red 0.5 0.5"));
    assert!(refused("vignette 0,0,0 heavy 0.5"));
    let loaded = |rec: &str| Session::load(&p, &replace_records(&save, "vignette ", &[rec])).unwrap().0.state().vignette.unwrap();
    // Numbers are clamped, NaN falls back.
    let big = loaded("vignette 9,-4,0.5 5 -3");
    assert_eq!((big.color, big.strength, big.size), ([1.0, 0.0, 0.5], 1.0, 0.0));
    let nan = loaded("vignette 0,0,0 NaN inf");
    assert_eq!((nan.strength, nan.size), (0.0, 0.5));
    // Two records are one setting: the last wins.
    let (twice, _) = Session::load(&p, &replace_records(&save, "vignette ", &["vignette 1,0,0 0.5 0.5", "vignette 0,1,0 0.6 0.1"])).unwrap();
    assert_eq!(twice.state().vignette, Some(Vignette { color: [0.0, 1.0, 0.0], strength: 0.6, size: 0.1 }));
}

#[test]
fn the_tester_has_nothing_to_say() {
    let r = crate::explore(&program(), 1000);
    assert!(r.warnings.is_empty(), "{:?}", r.warnings);
}
