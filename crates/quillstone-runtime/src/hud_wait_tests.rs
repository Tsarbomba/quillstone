//! `::wait{hud}`: the story stops until a HUD button is used.

use quillstone_script::parse;

use crate::explore::explore;
use crate::{can_press, waits_at_line, Cmd, Input, Session};

const SRC: &str = "\
---
vars: { seen: 0 }
---
::hud{id=status text=Status call=#status}
::hud{id=go text=Go jump=#out}
::hud{id=hidden text=Hidden jump=#out if=\"seen > 5\"}
# start
**A:** Press a button.
::wait{hud}
**A:** Never.
::end
# status
::set{seen += 1}
**A:** Status.
::return
# out
**A:** Out.
::end
";

fn at_wait(p: &quillstone_script::Program) -> Session<'_> {
    let (mut s, _) = Session::start(p);
    s.input(Input::Advance);
    assert_eq!(s.state().pending, Some(Cmd::WaitHud));
    s
}

#[test]
fn a_click_does_not_pass_it_but_a_jump_button_leaves() {
    let p = parse(SRC).unwrap();
    let mut s = at_wait(&p);
    assert!(waits_at_line(s.state()) && can_press(&p, s.state(), "go"));
    assert!(s.input(Input::Advance).is_empty());
    assert_eq!(s.state().pending, Some(Cmd::WaitHud));
    let cmds = s.input(Input::Hud("go".into()));
    assert!(matches!(&cmds[..], [.., Cmd::Say { text, .. }] if text == "Out."), "{cmds:?}");
}

#[test]
fn a_call_button_returns_to_the_wait_and_a_hidden_one_is_refused() {
    let p = parse(SRC).unwrap();
    let mut s = at_wait(&p);
    assert!(!can_press(&p, s.state(), "hidden") && s.input(Input::Hud("hidden".into())).is_empty());
    s.input(Input::Hud("status".into()));
    assert_eq!(s.state().vars["seen"].as_int(), Some(1));
    s.input(Input::Advance);
    assert_eq!(s.state().pending, Some(Cmd::WaitHud), "back at the wait");
    assert!(s.state().stack.is_empty());
}

#[test]
fn rollback_stops_at_it_and_a_save_loads_back_into_it() {
    let p = parse(SRC).unwrap();
    let mut s = at_wait(&p);
    s.input(Input::Hud("status".into()));
    s.rollback().expect("back");
    assert_eq!(s.state().pending, Some(Cmd::WaitHud));
    let (loaded, cmds) = Session::load(&p, &s.save()).unwrap();
    assert_eq!(loaded.state().pending, Some(Cmd::WaitHud));
    assert!(cmds.contains(&Cmd::WaitHud) || loaded.state().pending == Some(Cmd::WaitHud));
}

#[test]
fn the_tester_takes_the_shown_buttons_and_reports_a_wait_with_none() {
    let p = parse(SRC).unwrap();
    let report = explore(&p, 5000);
    assert!(report.errors.is_empty(), "{report:?}");
    assert!(report.endings.len() == 1, "{report:?}");
    let none = parse("# a\n::wait{hud}\n::end\n").unwrap();
    let report = explore(&none, 5000);
    assert!(report.errors.iter().any(|e| e.message.contains("none is shown")), "{report:?}");
}
