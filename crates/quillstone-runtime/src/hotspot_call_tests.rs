//! `call` regions of a `::hotspots` screen: the call comes back to the same screen, which is shown again.

use quillstone_script::parse;

use crate::explore::explore;
use crate::{step_traced, Cmd, Frame, Input, Session, MAX_CALL_DEPTH};

const SRC: &str = "\
---
vars: { looked: 0 }
---
# map
::bg{src=map.png}
::hotspots
- [Door](#door){rect=0,0,0.3,0.3, call, looked += 1}
- [Note](#note){rect=0.4,0,0.2,0.2, call, once}
- [Hall](#hall){rect=0.7,0,0.3,0.3, if seen(map) > 0}
::
# door
**A:** A door.
::return
# note
**A:** A note.
::return
# hall
**A:** The hall.
::end
";

fn labels(cmds: &[Cmd]) -> Vec<String> {
    cmds.iter().find_map(|c| match c {
        Cmd::ShowHotspots { spots, .. } => Some(spots.iter().map(|s| s.label.clone()).collect()),
        _ => None,
    })
    .expect("a hotspot screen")
}

/// The hidden counter behind `seen(label)`, now.
fn seen(p: &quillstone_script::Program, s: &Session) -> i64 {
    let name = p.counted.values().flatten().next().expect("a `seen` counter");
    s.state().vars[name].as_int().unwrap()
}

#[test]
fn a_call_region_returns_to_the_same_screen_and_reevaluates_once() {
    let p = parse(SRC).unwrap();
    let (mut s, first) = Session::start(&p);
    assert_eq!(labels(&first), ["Door", "Note", "Hall"]);
    let cmds = s.input(Input::Choose(0));
    assert!(matches!(&cmds[..], [.., Cmd::Say { text, .. }] if text == "A door."), "{cmds:?}");
    assert_eq!(s.state().stack.len(), 1);
    assert_eq!(labels(&s.input(Input::Advance)), ["Door", "Note", "Hall"], "the screen again, the door can be used again");
    assert!(s.state().stack.is_empty());
    assert_eq!(s.state().vars["looked"].as_int(), Some(1), "the region's effects ran once");
    s.input(Input::Choose(1));
    assert_eq!(labels(&s.input(Input::Advance)), ["Door", "Hall"], "a `once` region is gone when the screen comes back");
    assert_eq!(seen(&p, &s), 1, "coming back is not another visit");
    // a region without `call` jumps as before: nothing comes back
    let end = s.input(Input::Choose(1));
    assert!(matches!(&end[..], [.., Cmd::Say { text, .. }] if text == "The hall."), "{end:?}");
    assert!(s.state().stack.is_empty());
}

#[test]
fn rolling_back_a_call_returns_to_the_screen_and_a_save_inside_the_label_comes_back() {
    let p = parse(SRC).unwrap();
    let (mut s, _) = Session::start(&p);
    s.input(Input::Choose(0));
    let text = s.save();
    s.rollback().expect("back");
    assert!(matches!(s.state().pending, Some(Cmd::ShowHotspots { .. })));
    assert_eq!((s.state().stack.len(), s.state().vars["looked"].as_int()), (0, Some(0)));
    s.roll_forward().expect("and forward");
    assert_eq!(s.state().stack.len(), 1);

    let (mut loaded, _) = Session::load(&p, &text).unwrap();
    assert_eq!(loaded.state(), s.state());
    assert_eq!(labels(&loaded.input(Input::Advance)), ["Door", "Note", "Hall"], "the loaded label returns to the screen");
}

#[test]
fn the_tester_follows_calls_and_a_hub_left_by_a_plain_region_is_no_endless_loop() {
    let p = parse(SRC).unwrap();
    let report = explore(&p, 5000);
    assert!(report.errors.is_empty(), "{report:?}");
    assert!(report.warnings.iter().all(|w| !w.message.contains("door") && !w.message.contains("note")), "{:?}", report.warnings);
}

#[test]
fn a_call_region_is_ignored_when_the_call_stack_is_full() {
    let p = parse(SRC).unwrap();
    let (s, _) = Session::start(&p);
    let mut state = s.state().clone();
    state.stack = vec![Frame { ret: 0, line: false }; MAX_CALL_DEPTH];
    assert!(step_traced(&mut state, &p, Some(Input::Choose(0)), &mut |_| {}, None).is_empty());
    assert_eq!((state.stack.len(), state.vars["looked"].as_int()), (MAX_CALL_DEPTH, Some(0)), "nothing ran");
}

#[test]
fn coming_back_to_a_screen_that_starts_its_label_is_not_another_visit() {
    let p = parse("# hub\n::hotspots\n- [A](#a){rect=0,0,1,1, call}\n- [B](#b){rect=0,0,0.1,0.1, if seen(hub) > 5}\n::\n# a\n::return\n# b\n::end\n").unwrap();
    let (mut s, _) = Session::start(&p);
    assert_eq!(seen(&p, &s), 1);
    s.input(Input::Choose(0));
    assert!(matches!(s.state().pending, Some(Cmd::ShowHotspots { .. })));
    assert_eq!(seen(&p, &s), 1);
}

#[test]
fn returning_to_the_screen_hides_the_box_and_it_survives_rollback_and_save() {
    let p = parse(SRC).unwrap();
    let (mut s, _) = Session::start(&p);
    let said = s.input(Input::Choose(0));
    assert!(!s.state().textbox_hidden, "the label's line has its box");
    assert!(!said.iter().any(|c| matches!(c, Cmd::Textbox { hidden: true, .. })));
    let back = s.input(Input::Advance);
    assert!(back.iter().any(|c| matches!(c, Cmd::Textbox { hidden: true, .. })), "{back:?}");
    assert!(s.state().textbox_hidden);
    let text = s.save();
    let (loaded, _) = Session::load(&p, &text).unwrap();
    assert!(loaded.state().textbox_hidden, "a save on the screen keeps it hidden");
    // a rollback to the label's line shows its box again, and forward hides it again
    s.rollback().expect("back to the line");
    assert!(!s.state().textbox_hidden);
    s.roll_forward().expect("forward");
    assert!(s.state().textbox_hidden);
    // the next line shows the box as usual
    let hall = s.input(Input::Choose(2));
    assert!(!s.state().textbox_hidden && matches!(&hall[..], [.., Cmd::Say { .. }]), "{hall:?}");
}
