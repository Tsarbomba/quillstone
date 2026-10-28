//! `::particles` in the runtime: state, redraw, rollback, saves, hostile saves.

use quillstone_script::{parse, ParticleKind, Particles, Program};

use crate::{Cmd, Input, Session};

const SRC: &str = "\
::particles{snow amount=80 speed=2 wind=-1 layer=back fade=3}
One.
::particles{src=assets/petal.png amount=20}
Two.
::particles{none fade=2}
::particles{none}
Three.
";

fn program() -> Program {
    parse(SRC).unwrap()
}

fn snow() -> Particles {
    Particles { kind: ParticleKind::Snow, amount: 80, speed: 2.0, wind: -1.0, back: true }
}

fn petals() -> Particles {
    Particles { kind: ParticleKind::Image("assets/petal.png".into()), amount: 20, speed: 1.0, wind: 0.0, back: false }
}

#[test]
fn particles_are_scene_state_and_stopping_nothing_says_nothing() {
    let p = program();
    let (mut game, first) = Session::start(&p);
    assert_eq!(game.state().particles, Some(snow()));
    assert!(first.contains(&Cmd::Particles { set: Some(snow()), fade: 3.0 }));
    let cmds = game.input(Input::Advance);
    assert_eq!(game.state().particles, Some(petals()));
    assert!(cmds.contains(&Cmd::Particles { set: Some(petals()), fade: 1.0 }));
    let cmds = game.input(Input::Advance);
    assert_eq!(game.state().particles, None);
    assert!(cmds.contains(&Cmd::Particles { set: None, fade: 2.0 }));
    assert_eq!(cmds.iter().filter(|c| matches!(c, Cmd::Particles { .. })).count(), 1, "the second `none` has nothing to stop");
}

#[test]
fn a_redraw_sends_them_at_once_and_only_when_there_are_some() {
    let p = program();
    let (mut game, _) = Session::start(&p);
    let redraw = game.state().redraw();
    assert!(redraw.contains(&Cmd::Particles { set: Some(snow()), fade: 0.0 }));
    game.input(Input::Advance);
    game.input(Input::Advance);
    assert!(!game.state().redraw().iter().any(|c| matches!(c, Cmd::Particles { .. })));
}

#[test]
fn rollback_brings_the_settings_back() {
    let p = program();
    let (mut game, _) = Session::start(&p);
    game.input(Input::Advance);
    game.input(Input::Advance);
    assert_eq!(game.state().particles, None);
    let cmds = game.rollback().unwrap();
    assert_eq!(game.state().particles, Some(petals()));
    assert!(cmds.contains(&Cmd::Particles { set: Some(petals()), fade: 0.0 }));
    game.rollback().unwrap();
    assert_eq!(game.state().particles, Some(snow()));
}

#[test]
fn a_save_keeps_every_kind_of_particle() {
    let p = program();
    let (mut game, _) = Session::start(&p);
    for expected in [Some(snow()), Some(petals()), None] {
        let save = game.save();
        let (loaded, cmds) = Session::load(&p, &save).unwrap();
        assert_eq!(loaded.state().particles, expected);
        assert_eq!(cmds.iter().any(|c| matches!(c, Cmd::Particles { .. })), expected.is_some());
        game.input(Input::Advance);
    }
    let rain = parse("::particles{rain amount=7 speed=0.5 wind=1.5}\nHi.\n").unwrap();
    let (game, _) = Session::start(&rain);
    let (loaded, _) = Session::load(&rain, &game.save()).unwrap();
    assert_eq!(loaded.state().particles.as_ref().unwrap().kind, ParticleKind::Rain);
}

#[test]
fn a_save_from_before_particles_loads_none() {
    let p = program();
    let (game, _) = Session::start(&p);
    let old: String = game.save().lines().filter(|l| !l.starts_with("particles ")).map(|l| format!("{l}\n")).collect();
    assert_ne!(old, game.save());
    let (loaded, _) = Session::load(&p, &old).unwrap();
    assert_eq!(loaded.state().particles, None);
}

/// The first state's `particles` records, replaced by `lines`.
fn with_records(save: &str, lines: &[&str]) -> String {
    let mut out = String::new();
    let mut done = false;
    for l in save.lines().filter(|l| !l.starts_with("particles ")) {
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
    assert!(refused("particles image 10 1 0 false \"../../secret.png\""));
    assert!(refused("particles image 10 1 0 false \"/etc/passwd\""));
    assert!(refused("particles image 10 1 0 false \"C:x.png\""));
    assert!(refused("particles image 10 1 0 false \"a\\\\b.png\""), "backslash");
    assert!(refused("particles image 10 1 0 false \"voice.ogg\""), "not a picture");
    assert!(refused("particles image 10 1 0 false"), "no picture");
    assert!(refused("particles hail 10 1 0 false"));
    assert!(refused("particles snow ten 1 0 false"));
    assert!(refused("particles snow 10 fast 0 false"));
    assert!(refused("particles snow 10 1 0 maybe"));
    assert!(refused("particles snow 10 1 0"), "too short");
    assert!(refused("particles snow 99999999999999999999 1 0 false"));
    let loaded = |rec: &str| Session::load(&p, &with_records(&save, &[rec])).unwrap().0.state().particles.clone().unwrap();
    // Numbers are clamped, NaN falls back.
    let big = loaded("particles rain 1000000 100 -100 true");
    assert_eq!((big.amount, big.speed, big.wind, big.back), (200, 5.0, -2.0, true));
    let small = loaded("particles snow -5 0 5 false");
    assert_eq!((small.amount, small.speed, small.wind), (1, 0.2, 2.0));
    let nan = loaded("particles snow 0 NaN inf false");
    assert_eq!((nan.amount, nan.speed, nan.wind), (1, 1.0, 0.0));
    // Two records are one setting: the last wins.
    let (twice, _) = Session::load(&p, &with_records(&save, &["particles rain 5 1 0 false", "particles snow 6 1 0 false"])).unwrap();
    assert_eq!(twice.state().particles.as_ref().map(|p| (p.kind.clone(), p.amount)), Some((ParticleKind::Snow, 6)));
}

#[test]
fn the_tester_has_nothing_to_say() {
    let r = crate::explore(&program(), 1000);
    assert!(r.warnings.is_empty(), "{:?}", r.warnings);
}
