//! Timed `::choice{time=..}` in the runtime: the timer is worked out from the script (never
//! stored), so rollback, saves (even hostile ones) and edited scripts always agree with it.

use quillstone_script::{parse, Program};

use crate::{choice_timer, ChoiceTimer, Cmd, Input, Session};

const SRC: &str = "\
---
vars: { calm: 1 }
---
# start
Before.
::choice{time=8}
- [Speak](#spoke)
- [Stay quiet](#quiet){default, if calm > 0}
- [Run](#spoke){calm = 0}
::
# spoke
Spoke.
::end
# quiet
Quiet.
::end
";

fn program() -> Program {
    parse(SRC).unwrap()
}

fn at_choice(p: &Program) -> Session<'_> {
    let (mut game, _) = Session::start(p);
    game.input(Input::Advance);
    assert!(matches!(game.state().pending, Some(Cmd::ShowChoices { .. })));
    game
}

fn timer(game: &Session, p: &Program) -> Option<ChoiceTimer> {
    choice_timer(game.state(), p)
}

#[test]
fn the_timer_carries_the_limit_and_the_index_of_the_default_among_the_visible_options() {
    let p = program();
    let mut game = at_choice(&p);
    assert_eq!(timer(&game, &p), Some(ChoiceTimer { seconds: 8.0, default: 1 }));
    // not a timed choice, nothing to count
    let (before, _) = Session::start(&p);
    assert_eq!(timer(&before, &p), None);
    // picking the default as the frontend does when time is up
    game.input(Input::Choose(1));
    assert!(matches!(&game.state().pending, Some(Cmd::Say { text, .. }) if text == "Quiet."));
    assert_eq!(timer(&game, &p), None);
}

#[test]
fn a_default_hidden_by_its_condition_falls_back_to_the_first_visible_option() {
    let p = parse(&SRC.replace("calm: 1", "calm: 0")).unwrap();
    let game = at_choice(&p);
    assert_eq!(timer(&game, &p), Some(ChoiceTimer { seconds: 8.0, default: 0 }));
    let Some(Cmd::ShowChoices { options, .. }) = &game.state().pending else { panic!() };
    assert_eq!(options, &["Speak", "Run"]);
    // the default is the middle option when all show: its index follows the visible list
    let p2 = parse(&SRC.replace("- [Speak](#spoke)\n", "- [Speak](#spoke){if 0}\n")).unwrap();
    assert_eq!(timer(&at_choice(&p2), &p2), Some(ChoiceTimer { seconds: 8.0, default: 0 }));
}

#[test]
fn rollback_and_roll_forward_bring_the_same_timer_back() {
    let p = program();
    let mut game = at_choice(&p);
    game.input(Input::Choose(0));
    assert_eq!(timer(&game, &p), None);
    game.rollback().unwrap();
    assert_eq!(timer(&game, &p), Some(ChoiceTimer { seconds: 8.0, default: 1 }));
    game.roll_forward().unwrap();
    assert_eq!(timer(&game, &p), None);
}

#[test]
fn a_loaded_save_waits_at_the_choice_with_the_scripts_timer() {
    let p = program();
    let game = at_choice(&p);
    let (loaded, cmds) = Session::load(&p, &game.save()).unwrap();
    assert!(cmds.iter().any(|c| matches!(c, Cmd::ShowChoices { .. })));
    assert_eq!(timer(&loaded, &p), Some(ChoiceTimer { seconds: 8.0, default: 1 }));
}

#[test]
fn nothing_in_a_hostile_save_can_change_the_limit_or_the_default() {
    let p = program();
    let save = at_choice(&p).save();
    assert!(save.contains("pending choices"));
    // invented timer lines and edited option text change nothing: the limit comes from the script
    for line in ["time 99999", "choice_time 0", "timer NaN 7", "default 9", "pending choices \"x\" \"y\" \"z\" time=1e9"] {
        let text = save.replace("pending choices", &format!("{line}\npending choices"));
        let pending_edit = text.replace("\"Stay quiet\"", "\"Stay quiet\" \"Extra\"");
        for t in [&text, &pending_edit] {
            // unknown records may be refused; a save that does load must carry the script's timer
            if let Ok((loaded, _)) = Session::load(&p, t) {
                assert_eq!(timer(&loaded, &p), Some(ChoiceTimer { seconds: 8.0, default: 1 }), "{line}");
            }
        }
    }
    // a variable changed so the default is hidden: the timer is the fallback, and in range
    let text = save.replace("var \"calm\" int 1", "var \"calm\" int 0");
    let (loaded, _) = Session::load(&p, &text).unwrap();
    let t = timer(&loaded, &p).unwrap();
    let Some(Cmd::ShowChoices { options, .. }) = &loaded.state().pending else { panic!() };
    assert_eq!((t.seconds, t.default), (8.0, 0));
    assert!(t.default < options.len());
}

#[test]
fn a_save_moved_to_a_script_where_the_choice_is_not_timed_has_no_timer() {
    let p = program();
    let save = at_choice(&p).save();
    let untimed = parse(&SRC.replace("::choice{time=8}", "::choice").replace("{default, if calm > 0}", "{if calm > 0}")).unwrap();
    let (loaded, _) = Session::load(&untimed, &save).unwrap();
    assert_eq!(timer(&loaded, &untimed), None);
}

// ---- timed hotspots ------------------------------------------------------------------------------

const HOT: &str = "\
---
vars: { open: true }
---
# start
::show{id=bug src=bug.png}
::hotspots{time=3 default=#slow}
- [Bug](#caught){sprite=bug}
- [Door](#door){rect=0,0,0.1,0.1, if open}
::
# caught
Caught.
::end
# door
Door.
::end
# slow
Too slow.
::end
";

#[test]
fn a_timed_hotspot_screen_has_a_timer_whose_default_is_the_timeout_jump() {
    let p = parse(HOT).unwrap();
    let (mut game, _) = Session::start(&p);
    assert_eq!(timer(&game, &p), Some(ChoiceTimer { seconds: 3.0, default: 2 }), "after the bug and the door");
    // the door is gone: the timeout moves up in the visible list, still last
    let gated = parse(&HOT.replace("open: true", "open: false")).unwrap();
    let (closed, _) = Session::start(&gated);
    assert_eq!(timer(&closed, &gated), Some(ChoiceTimer { seconds: 3.0, default: 1 }));
    let cmds = game.input(Input::Choose(2));
    assert!(cmds.iter().any(|c| matches!(c, Cmd::Say { text, .. } if text == "Too slow.")));
    assert_eq!(timer(&game, &p), None);
}

#[test]
fn a_timed_hotspot_screen_with_nothing_to_click_is_skipped_not_waited_out() {
    let p = parse("---\nvars: { open: false }\n---\n# a\n::hotspots{time=3 default=#slow}\n- [Door](#a){rect=0,0,0.1,0.1, if open}\n::\n# slow\nToo slow.\n::end\n").unwrap();
    let (game, _) = Session::start(&p);
    assert!(matches!(&game.state().pending, Some(Cmd::Say { text, .. }) if text == "Too slow."));
}

#[test]
fn a_timed_hotspot_screen_survives_a_save_with_the_same_timer() {
    let p = parse(HOT).unwrap();
    let (game, _) = Session::start(&p);
    let text = game.save();
    assert!(text.contains("pending hotspots"));
    let (loaded, _) = Session::load(&p, &text).unwrap();
    assert_eq!(timer(&loaded, &p), Some(ChoiceTimer { seconds: 3.0, default: 2 }));
    let untimed = parse(&HOT.replace("::hotspots{time=3 default=#slow}", "::hotspots")).unwrap();
    let (loaded, _) = Session::load(&untimed, &text).unwrap();
    assert_eq!(timer(&loaded, &untimed), None, "the script decides, not the save");
}
