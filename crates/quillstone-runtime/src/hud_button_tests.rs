//! Clickable HUD elements (`call=#label`): the press as a call from the line, what refuses it, and saves.

use quillstone_script::parse;

use crate::explore::explore;
use crate::{can_press, Cmd, Input, Session};

const SRC: &str = "\
---
vars: { calls: 0, name: \"\", quests: [] }
journal: quests
---
::hud{id=phone image=assets/phone.png call=#phone hover=assets/phone_hover.png x=0.9 y=0.1}
::hud{id=log text=Log open=journal}
::hud{id=off text=Off call=#phone if=\"calls > 5\"}
# start
**A:** one
**A:** two
::choice
- [Go](#next)
::
# next
::input{var=name prompt=\"Name?\"}
::wait{time=1}
**A:** three{nw}
::end
# phone
::set{calls += 1}
**A:** Ring.
::return
";

fn calls(s: &Session) -> i64 {
    s.state().vars["calls"].as_int().unwrap()
}

#[test]
fn a_press_calls_the_label_and_returns_to_the_line() {
    let p = parse(SRC).unwrap();
    let (mut s, _) = Session::start(&p);
    assert!(can_press(&p, s.state(), "phone"));
    let cmds = s.input(Input::Hud("phone".into()));
    assert!(matches!(&cmds[..], [.., Cmd::Say { text, .. }] if text == "Ring."), "{cmds:?}");
    assert_eq!((calls(&s), s.state().stack.len()), (1, 1));
    let back = s.input(Input::Advance);
    assert!(matches!(&back[..], [.., Cmd::Say { text, .. }] if text == "one"), "the same line again: {back:?}");
    assert!(s.state().stack.is_empty());
}

#[test]
fn a_press_is_refused_unless_the_story_waits_at_a_line_and_the_element_is_a_shown_call_button() {
    let p = parse(SRC).unwrap();
    let (mut s, _) = Session::start(&p);
    for id in ["off", "nothing"] {
        assert!(!can_press(&p, s.state(), id), "{id}: hidden by its `if=`, or not there");
        assert!(s.input(Input::Hud(id.into())).is_empty());
    }
    assert!(s.input(Input::Hud("log".into())).is_empty(), "an `open=` button is the screen's business, not a call");
    s.input(Input::Advance);
    s.input(Input::Advance);
    assert!(matches!(s.state().pending, Some(Cmd::ShowChoices { .. })));
    assert!(!can_press(&p, s.state(), "phone") && s.input(Input::Hud("phone".into())).is_empty(), "over a choice");
    s.input(Input::Choose(0));
    assert!(matches!(s.state().pending, Some(Cmd::AskInput { .. })));
    assert!(s.input(Input::Hud("phone".into())).is_empty(), "over an input");
    s.input(Input::Text("Bo".into()));
    assert!(matches!(s.state().pending, Some(Cmd::Wait { .. })));
    assert!(s.input(Input::Hud("phone".into())).is_empty(), "over a wait");
    s.input(Input::Advance);
    assert!(matches!(s.state().pending, Some(Cmd::Say { .. })));
    assert!(!can_press(&p, s.state(), "phone"), "a {{nw}} line moves on by itself");
    assert_eq!(calls(&s), 0);
}

#[test]
fn rolling_back_a_press_returns_to_before_it() {
    let p = parse(SRC).unwrap();
    let (mut s, _) = Session::start(&p);
    s.input(Input::Hud("phone".into()));
    s.rollback().expect("back");
    assert_eq!((calls(&s), s.state().stack.len()), (0, 0));
    assert!(matches!(&s.state().pending, Some(Cmd::Say { text, .. }) if text == "one"));
    s.roll_forward().expect("and forward");
    assert_eq!(calls(&s), 1);
}

#[test]
fn a_save_made_inside_the_label_loads_and_finishes_the_press() {
    let p = parse(SRC).unwrap();
    let (mut s, _) = Session::start(&p);
    s.input(Input::Hud("phone".into()));
    let text = s.save();
    assert!(text.contains("input hud ") && text.contains("\"call:phone\" \"assets/phone_hover.png\""), "{text}");
    let (mut loaded, _) = Session::load(&p, &text).unwrap();
    assert_eq!(loaded.state(), s.state());
    loaded.input(Input::Advance);
    assert!(matches!(&loaded.state().pending, Some(Cmd::Say { text, .. }) if text == "one"));
    assert!(can_press(&p, loaded.state(), "phone"), "the button came back with the save");
    assert!(loaded.rollback().is_some() && loaded.rollback().is_some() && calls(&loaded) == 0);
}

#[test]
fn a_hand_edited_save_cannot_make_a_button_call_a_label_nobody_offered() {
    let p = parse(SRC).unwrap();
    let text = Session::start(&p).0.save();
    let line = text.lines().find(|l| l.starts_with("hud image")).expect("the phone is saved").to_string();
    let edit = |from: &str, to: &str| text.replace(&line, &line.replace(from, to));
    // `next` is a real label, but no `::hud` offers it
    for bad in [edit("call:phone", "call:next"), edit("call:phone", "call:nowhere"), edit("call:phone", "open:nothing"), edit("assets/phone_hover.png", "../x.png")] {
        assert!(Session::load(&p, &bad).is_err(), "loaded: {bad}");
    }
    assert!(Session::load(&p, &edit("call:phone", "open:history")).is_ok(), "a built-in screen is fine");
    // a screen the game does not have is refused as well
    let plain = parse("::hud{id=a text=A open=settings}\n**A:** hi\n").unwrap();
    let hacked = Session::start(&plain).0.save().replace("open:settings", "open:journal");
    assert!(Session::load(&plain, &hacked).is_err());
    // a script edited under a running game drops the button instead of failing the load
    let edited = parse(&SRC.replace(" call=#phone", "").replace(" hover=assets/phone_hover.png", "").replace("# phone", "# ring")).unwrap();
    let (loaded, _) = Session::load_edited(&edited, &text).unwrap();
    assert!(loaded.state().hud.iter().any(|h| h.id == "phone" && h.action.is_none() && h.hover.is_none()));
}

#[test]
fn the_tester_presses_a_hud_button_once_and_does_not_call_it_unreached() {
    let p = parse(SRC).unwrap();
    let report = explore(&p, 5000);
    assert!(report.errors.is_empty(), "{report:?}");
    assert!(report.warnings.iter().all(|w| !w.message.contains("phone") && !w.message.contains("Ring")), "{:?}", report.warnings);
    let without = parse(&SRC.replace("call=#phone hover=assets/phone_hover.png", "open=journal")).unwrap();
    assert!(explore(&without, 5000).warnings.iter().any(|w| w.message.contains("phone")), "the label is unreached without the button");
}
