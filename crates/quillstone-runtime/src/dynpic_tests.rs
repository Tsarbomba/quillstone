//! Picture names built from variables (`assets/places/{place}.png`): what the state, the
//! commands and the saves carry, rollback, hostile values, and a typed name.

use quillstone_script::{parse, Program, MISSING_PICTURE};

use crate::test_util::next;
use crate::{Cmd, Input, Session};

const SRC: &str = "\
---
vars: { place: \"rooftop\", mood: \"happy\", hero.name: \"Alex\", count: 3 }
---
::character{id=mika sprites=art/mika_{pose}.png}
# start
::bg{src=\"bg/{place}.png\"}
::show{mika {mood} x=left}
![](items/{hero.name}.png)
One.
::set{place=\"night\", mood=\"sad\"}
::bg{src=\"bg/{place}.png\"}
::show{mika {mood}}
Two.
::set{place=\"../x\"}
::bg{src=\"bg/{place}.png\"}
Three.
";

fn program() -> Program {
    parse(SRC).unwrap()
}

fn warnings(cmds: &[Cmd]) -> Vec<&str> {
    cmds.iter().filter_map(|c| if let Cmd::Warning { message } = c { Some(message.as_str()) } else { None }).collect()
}

#[test]
fn the_state_and_the_commands_carry_the_resolved_path() {
    let p = program();
    let (mut game, first) = Session::start(&p);
    assert_eq!(game.state().background.as_deref(), Some("bg/rooftop.png"));
    assert_eq!(game.state().sprites[0].src, "art/mika_happy.png", "a pose from a variable goes through the sprites template");
    assert_eq!(game.state().inline[0].0, "items/Alex.png", "dotted names work, inline images too");
    assert!(first.contains(&Cmd::SetBackground { src: "bg/rooftop.png".into(), fade: 0.0, wipe: None, sound: false }));
    assert!(first.iter().any(|c| matches!(c, Cmd::ShowSprite { src, .. } if src == "art/mika_happy.png")));
    assert!(first.iter().any(|c| matches!(c, Cmd::ShowImage { src, .. } if src == "items/Alex.png")));
    assert!(warnings(&first).is_empty());
    next(&mut game);
    assert_eq!(game.state().background.as_deref(), Some("bg/night.png"));
    assert_eq!(game.state().sprites[0].src, "art/mika_sad.png");
    assert_eq!(game.state().sprites[0].x, 0.0, "the sprite kept its place");
    // A redraw is built from what the state holds, never from the pattern.
    assert!(game.state().redraw().contains(&Cmd::SetBackground { src: "bg/night.png".into(), fade: 0.0, wipe: None, sound: false }));
}

#[test]
fn rollback_restores_the_picture_that_was_shown() {
    let p = program();
    let (mut game, _) = Session::start(&p);
    next(&mut game);
    assert_eq!(game.state().background.as_deref(), Some("bg/night.png"));
    let back = game.rollback().unwrap();
    assert_eq!(game.state().background.as_deref(), Some("bg/rooftop.png"));
    assert_eq!(game.state().sprites[0].src, "art/mika_happy.png");
    assert!(back.contains(&Cmd::SetBackground { src: "bg/rooftop.png".into(), fade: 0.0, wipe: None, sound: false }));
    assert!(warnings(&back).is_empty());
    // And rolling forward again shows the later picture.
    let fwd = game.roll_forward().unwrap();
    assert_eq!(game.state().background.as_deref(), Some("bg/night.png"));
    assert!(fwd.iter().any(|c| matches!(c, Cmd::SetBackground { src, .. } if src == "bg/night.png")));
}

#[test]
fn saves_hold_resolved_paths_and_load_unchanged() {
    let p = program();
    let (mut game, _) = Session::start(&p);
    next(&mut game);
    let text = game.save();
    assert!(text.contains("bg \"bg/night.png\"") && text.contains("art/mika_sad.png"), "{text}");
    assert!(!text.contains("{place}") && !text.contains("{mood}"), "no pattern in a save: {text}");
    let (loaded, cmds) = Session::load(&p, &text).unwrap();
    assert_eq!(loaded.state().background.as_deref(), Some("bg/night.png"));
    assert!(cmds.iter().any(|c| matches!(c, Cmd::SetBackground { src, .. } if src == "bg/night.png")));
    // A hostile save naming a path outside the folder is still refused, whatever the script does.
    assert!(Session::load(&p, &text.replace("bg/night.png", "../night.png")).is_err());
    assert!(Session::load(&p, &text.replace("bg/night.png", "C:night.png")).is_err());
    // A save written by a script without patterns loads in a script with them.
    let q = parse("::bg{src=bg/a.png}\nHi.\n").unwrap();
    let (old, _) = Session::start(&q);
    assert_eq!(Session::load(&p, &old.save()).unwrap().0.state().background.as_deref(), Some("bg/a.png"));
}

#[test]
fn an_unsafe_value_shows_nothing_and_warns_once_per_run() {
    let p = program();
    let (mut game, _) = Session::start(&p);
    next(&mut game);
    let cmds = next(&mut game); // place = "../x"
    assert_eq!(game.state().background.as_deref(), Some(MISSING_PICTURE));
    assert!(cmds.contains(&Cmd::SetBackground { src: MISSING_PICTURE.into(), fade: 0.0, wipe: None, sound: false }));
    let w = warnings(&cmds);
    assert_eq!(w.len(), 1, "{w:?}");
    assert!(w[0].contains("{place}") && w[0].contains("bg/{place}.png") && !w[0].contains("../x"), "{}", w[0]);
    // The placeholder is a path a save accepts.
    let text = game.save();
    assert!(Session::load(&p, &text).is_ok(), "{text}");
    let back = game.rollback().unwrap();
    assert_eq!(game.state().background.as_deref(), Some("bg/night.png"));
    assert!(warnings(&back).is_empty(), "a rollback replays the state, not the names");
}

fn shown_with(value: &str) -> (Option<String>, Vec<String>) {
    let p = parse("---\nvars: { v: \"x\" }\n---\n::bg{src=\"bg/{v}.png\"}\nHi.\n").unwrap();
    let mut s = crate::State::new(&p);
    s.vars.insert("v".into(), quillstone_script::Value::Str(value.into()));
    let cmds = crate::step(&mut s, &p, None);
    (s.background.clone(), warnings(&cmds).iter().map(|m| m.to_string()).collect())
}

#[test]
fn every_kind_of_unsafe_value_is_a_missing_picture() {
    let long = "a".repeat(65);
    for bad in ["../x", "a/b", "a\\b", "C:x", "", &long, "..", ".", ".hidden", "a\nb", "a\0b", "a\u{1b}b", "/etc/passwd", "x/../../y", " ", "con", "a..b"] {
        let (bg, w) = shown_with(bad);
        assert_eq!(bg.as_deref(), Some(MISSING_PICTURE), "{bad:?}");
        assert_eq!(w.len(), 1, "{bad:?}");
    }
    for good in ["rooftop", "Night 2", "a.b", "é夜", &"a".repeat(64)] {
        let (bg, w) = shown_with(good);
        assert_eq!(bg, Some(format!("bg/{good}.png")));
        assert!(w.is_empty());
    }
}

#[test]
fn a_value_that_is_not_text_is_a_missing_picture() {
    let p = parse("---\nvars: { n: 3 }\n---\n::bg{src=\"bg/{n}.png\"}\n::bg{src=\"bg/{unset}.png\"}\nHi.\n");
    assert!(p.is_err(), "a declared number can never name a picture");
    let p = parse("::bg{src=\"bg/{unset}.png\"}\nHi.\n").unwrap();
    let (game, cmds) = Session::start(&p);
    assert_eq!(game.state().background.as_deref(), Some(MISSING_PICTURE));
    assert_eq!(warnings(&cmds).len(), 1);
}

#[test]
fn a_name_typed_by_the_player_flows_into_a_picture_name() {
    let src = "---\nvars: { hero.name: \"Alex\" }\n---\n::input{var=hero.name prompt=\"Name?\" default=\"Alex\" max=64}\n::bg{src=\"faces/{hero.name}.png\"}\nHi.\n";
    let p = parse(src).unwrap();
    for (typed, want) in [("Mika", Some("faces/Mika.png")), ("Sam Lee", Some("faces/Sam Lee.png")), ("../../x", None), ("a/b", None), ("C:x", None), ("a\\b", None), ("..", None)] {
        let (mut game, _) = Session::start(&p);
        let cmds = game.input(Input::Text(typed.to_string()));
        assert_eq!(game.state().background.as_deref(), Some(want.unwrap_or(MISSING_PICTURE)), "{typed}");
        assert_eq!(warnings(&cmds).len(), usize::from(want.is_none()), "{typed}");
    }
    // Control characters are dropped by the input itself, leaving a safe name.
    let (mut game, _) = Session::start(&p);
    game.input(Input::Text("Mi\nka\u{1b}".to_string()));
    assert_eq!(game.state().background.as_deref(), Some("faces/Mika.png"));
}

#[test]
fn a_hostile_save_variable_cannot_make_a_path_outside_the_folder() {
    let p = parse("---\nvars: { place: \"rooftop\" }\n---\nOne.\n::bg{src=\"bg/{place}.png\"}\nTwo.\n").unwrap();
    let (game, _) = Session::start(&p);
    let text = game.save().replace("var \"place\" text \"rooftop\"", "var \"place\" text \"../../etc/x\"");
    assert_ne!(text, game.save(), "the save names the variable as a quoted text: {}", game.save());
    let (mut loaded, _) = Session::load(&p, &text).unwrap();
    let cmds = next(&mut loaded);
    assert_eq!(loaded.state().background.as_deref(), Some(MISSING_PICTURE));
    assert_eq!(warnings(&cmds).len(), 1);
}
