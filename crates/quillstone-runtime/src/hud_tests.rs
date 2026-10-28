//! `::hud` and `::bg{color}` in the runtime: state, redraw, rollback, saves, hostile saves.

use quillstone_script::{bg_color, parse, HudAnchor, HudKind, Program};

use crate::{Cmd, Input, Session, State};

const SRC: &str = "\
---
vars: { hero.hp: 4, hero.max_hp: 10, gold: 2 }
---
# start
::bg{color=#102030}
::hud{id=hp bar=hero.hp max=hero.max_hp at=0.02,0.04 size=320,26 color=#cc3333 border=#fff}
::hud{id=gold text=\"Gold {gold} a\\\\b\" at=0.9,0.1 anchor=right size=30 back=#00000099 pad=8 if=\"gold > 0\"}
::hud{id=icon image=assets/heart.png at=0.5,0.5 anchor=center scale=0.5}
Line one.
::hud{id=gold text=\"Gold now {gold}\" at=0.9,0.1 anchor=right}
Line two.
::hud{hide=icon}
::hud{hide=nothing}
Line three.
::hud{clear}
Line four.
";

fn hud_ids(s: &State) -> Vec<&str> {
    s.hud.iter().map(|h| h.id.as_str()).collect()
}

fn program() -> Program {
    parse(SRC).unwrap()
}

#[test]
fn elements_are_created_replaced_in_place_hidden_and_cleared() {
    let p = program();
    let (mut game, first) = Session::start(&p);
    assert_eq!(hud_ids(game.state()), ["hp", "gold", "icon"]);
    assert_eq!(first.iter().filter(|c| matches!(c, Cmd::HudShow { .. })).count(), 3);
    game.input(Input::Advance);
    assert_eq!(hud_ids(game.state()), ["hp", "gold", "icon"], "an id used again keeps its place");
    let HudKind::Text { text, .. } = &game.state().hud[1].kind else { panic!() };
    assert_eq!(text, "Gold now {gold}", "and is replaced, not added");
    let cmds = game.input(Input::Advance);
    assert_eq!(hud_ids(game.state()), ["hp", "gold"]);
    assert!(cmds.contains(&Cmd::HudHide { id: "icon".into(), fade: 0.0 }));
    assert!(!cmds.iter().any(|c| matches!(c, Cmd::HudHide { id, .. } if id == "nothing")), "an unknown id is a no-op");
    let cmds = game.input(Input::Advance);
    assert!(game.state().hud.is_empty());
    assert!(cmds.contains(&Cmd::HudClear { fade: 0.0 }));
}

#[test]
fn a_redraw_sends_every_element_again_in_order_and_at_once() {
    let p = program();
    let (game, _) = Session::start(&p);
    let redraw = game.state().redraw();
    let shown: Vec<&str> = redraw
        .iter()
        .filter_map(|c| match c {
            Cmd::HudShow { elem, fade } => {
                assert_eq!(*fade, 0.0);
                Some(elem.id.as_str())
            }
            _ => None,
        })
        .collect();
    assert_eq!(shown, ["hp", "gold", "icon"]);
    assert_eq!(redraw[0], Cmd::ClearScene);
}

#[test]
fn rollback_and_roll_forward_restore_the_elements() {
    let p = program();
    let (mut game, _) = Session::start(&p);
    game.input(Input::Advance);
    game.input(Input::Advance);
    game.input(Input::Advance);
    assert!(game.state().hud.is_empty());
    let back = game.rollback().unwrap();
    assert_eq!(hud_ids(game.state()), ["hp", "gold"]);
    assert_eq!(back.iter().filter(|c| matches!(c, Cmd::HudShow { .. })).count(), 2);
    game.rollback();
    assert_eq!(hud_ids(game.state()), ["hp", "gold", "icon"]);
    game.roll_forward();
    game.roll_forward();
    assert!(game.state().hud.is_empty());
}

#[test]
fn saves_keep_the_elements_exactly() {
    let p = program();
    let (mut game, _) = Session::start(&p);
    for step in 0..4 {
        let (loaded, cmds) = Session::load(&p, &game.save()).unwrap();
        assert_eq!(loaded.state(), game.state(), "step {step}");
        assert_eq!(loaded.state().hud, game.state().hud);
        assert_eq!(cmds, game.state().redraw());
        game.input(Input::Advance);
    }
    let (game, _) = Session::start(&p);
    let text = game.save();
    assert!(text.lines().any(|l| l.starts_with("hud bar \"hp\" 0.02 0.04 left \"\" \"hero.hp\" \"hero.max_hp\" 320 26 ")), "{text}");
    assert!(text.lines().any(|l| l.starts_with("hud text \"gold\" 0.9 0.1 right \"gold > 0\" \"Gold {gold} a")), "{text}");
}

#[test]
fn quotes_and_backslashes_in_a_text_survive_a_save() {
    let p = program();
    let (mut game, _) = Session::start(&p);
    let mut elem = game.state().hud[1].clone();
    elem.kind = HudKind::Text { text: "say \"hi\" \\ {gold}\nnext".into(), size: None, color: None, back: None, pad: 1.5 };
    let mut state = game.state().clone();
    state.set_hud(elem);
    game.state = state;
    let (loaded, _) = Session::load(&p, &game.save()).unwrap();
    assert_eq!(loaded.state().hud, game.state().hud);
}

#[test]
fn a_save_from_before_hud_loads_none() {
    let p = program();
    let (game, _) = Session::start(&p);
    let old: String = game.save().lines().filter(|l| !l.starts_with("hud ")).map(|l| format!("{l}\n")).collect();
    let (loaded, _) = Session::load(&p, &old).unwrap();
    assert!(loaded.state().hud.is_empty());
}

/// The first state's `hud` records, replaced by `lines`.
fn with_hud_records(save: &str, lines: &[&str]) -> String {
    let mut out = String::new();
    let mut done = false;
    for l in save.lines().filter(|l| !l.starts_with("hud ")) {
        out.push_str(l);
        out.push('\n');
        if l.starts_with("rng ") && !done {
            done = true;
            for h in lines {
                out.push_str(h);
                out.push('\n');
            }
        }
    }
    out
}

#[test]
fn hostile_hud_records_are_refused_or_made_safe() {
    let p = program();
    let (game, _) = Session::start(&p);
    let save = game.save();
    let refused = |rec: &str| Session::load(&p, &with_hud_records(&save, &[rec])).is_err();
    assert!(refused("hud image \"icon\" 0.5 0.5 left \"\" \"../../secret.png\" 1"));
    assert!(refused("hud image \"icon\" 0.5 0.5 left \"\" \"/etc/passwd\" 1"));
    assert!(refused("hud image \"icon\" 0.5 0.5 left \"\" \"C:x.png\" 1"));
    assert!(refused("hud blob \"a\" 0 0 left \"\""));
    assert!(refused("hud text \"a\" 0 0 left \"\""), "too short");
    assert!(refused("hud text \"a b\" 0 0 left \"\" \"x\" - - - 0"), "bad id");
    assert!(refused("hud text \"a\" 0 0 sideways \"\" \"x\" - - - 0"), "bad anchor");
    assert!(refused("hud text \"a\" zero 0 left \"\" \"x\" - - - 0"), "bad number");
    assert!(refused("hud text \"a\" 0 0 left \"1 +\" \"x\" - - - 0"), "bad expression");
    assert!(refused("hud text \"a\" 0 0 left \"random(1,2)\" \"x\" - - - 0"), "random in a condition");
    assert!(refused("hud bar \"a\" 0 0 left \"\" \"1 +\" \"2\" 10 10 - 0,0,0,1 - 0"));
    assert!(refused("hud text \"a\" 0 0 left \"\" \"x\" - 1,2 - 0"), "bad colour");
    let long = "x".repeat(5000);
    assert!(refused(&format!("hud text \"a\" 0 0 left \"\" \"{long}\" - - - 0")));
    let many: Vec<String> = (0..300).map(|i| format!("hud text \"e{i}\" 0 0 left \"\" \"x\" - - - 0")).collect();
    let many: Vec<&str> = many.iter().map(String::as_str).collect();
    assert!(Session::load(&p, &with_hud_records(&save, &many)).is_err());
    // Numbers are clamped, NaN falls back.
    let (loaded, _) =
        Session::load(&p, &with_hud_records(&save, &["hud text \"a\" 7 -3 right \"\" \"x\" 99999 2,2,2,NaN - -1"])).unwrap();
    let h = &loaded.state().hud[0];
    assert_eq!(h.at, [1.0, 0.0]);
    assert_eq!(h.anchor, HudAnchor::Right);
    let HudKind::Text { size, color, pad, .. } = &h.kind else { panic!() };
    assert_eq!((*size, *pad), (Some(300.0), 0.0));
    assert_eq!(*color, Some([1.0, 1.0, 1.0, 0.0]));
    // The same id twice is one element.
    let twice = ["hud text \"a\" 0 0 left \"\" \"x\" - - - 0", "hud text \"a\" 0.5 0 left \"\" \"y\" - - - 0"];
    let (loaded, _) = Session::load(&p, &with_hud_records(&save, &twice)).unwrap();
    assert_eq!(loaded.state().hud.len(), 1);
}

#[test]
fn the_tester_sees_what_a_hud_reads() {
    let warnings = |src: &str| crate::explore(&parse(src).unwrap(), 1000).warnings.into_iter().map(|w| w.message).collect::<Vec<_>>();
    let w = warnings("---\nvars: { ok: 1 }\n---\n::hud{id=a text=\"{typo}\"}\n::hud{id=b bar=hpp max=ok size=10,10}\n::hud{id=c bar=ok max=maxx size=10,10}\n::hud{id=d text=x if=\"flagg\"}\nHi.\n");
    for name in ["typo", "hpp", "maxx", "flagg"] {
        assert!(w.iter().any(|m| m.contains(&format!("`{name}`")) || m.contains(&format!("{{{name}}}"))), "{name}: {w:?}");
    }
    let clean = warnings("---\nvars: { hp: 3 }\n---\n::hud{id=a text=\"{hp}\"}\n::hud{id=b bar=hp max=10 size=10,10 if=\"hp > 1\"}\nHi.\n");
    assert!(clean.is_empty(), "{clean:?}");
}

// ---- solid-colour backgrounds ----------------------------------------------------------

#[test]
fn a_colour_background_is_scene_state() {
    let p = program();
    let (game, first) = Session::start(&p);
    assert_eq!(game.state().background.as_deref(), Some("color:#102030"));
    assert!(first.contains(&Cmd::SetBackground { src: "color:#102030".into(), fade: 0.0, wipe: None }));
    assert!(game.state().redraw().contains(&Cmd::SetBackground { src: "color:#102030".into(), fade: 0.0, wipe: None }));
    assert_eq!(bg_color(game.state().background.as_deref().unwrap()), Some([16.0 / 255.0, 32.0 / 255.0, 48.0 / 255.0]));
}

#[test]
fn a_colour_background_survives_saving_and_a_hostile_one_does_not() {
    let p = program();
    let (game, _) = Session::start(&p);
    let (loaded, _) = Session::load(&p, &game.save()).unwrap();
    assert_eq!(loaded.state().background.as_deref(), Some("color:#102030"));
    let save = game.save();
    assert!(save.contains("bg \"color:#102030\""));
    // a record that merely looks like a colour is a path, and paths with `:` are refused
    for bad in ["color:#12", "color:#gggggg", "color:red", "color:#1020304"] {
        let hostile = save.replace("color:#102030", bad);
        assert!(Session::load(&p, &hostile).is_err(), "{bad}");
    }
}

#[test]
fn a_colour_through_a_dip_changes_after_the_fade_out() {
    let p = parse("::bg{color=#ff0000}\nA.\n::bg{color=#0000ff fade=1 through=#000}\nB.\n").unwrap();
    let (mut game, _) = Session::start(&p);
    let cmds = game.input(Input::Advance);
    assert_eq!(cmds[0], Cmd::FadeOut { time: 0.5, color: [0.0; 3] });
    let cmds = game.input(Input::Advance);
    assert_eq!(cmds[0], Cmd::SetBackground { src: "color:#0000ff".into(), fade: 0.0, wipe: None });
}

#[test]
fn the_hud_demo_is_clean_and_every_branch_plays() {
    let r = crate::explore(&parse(include_str!("../../../examples/hud.quill.md")).unwrap(), 100_000);
    assert!(r.warnings.is_empty() && r.errors.is_empty(), "{:?} {:?}", r.warnings, r.errors);
    assert!(!r.partial);
}
