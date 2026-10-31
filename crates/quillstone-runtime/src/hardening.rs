//! Tests for the second review's runtime findings: persistent values and the
//! screen, saves that wait at nothing, hostile saves, and the history cap.

use std::time::{Duration, Instant};

use quillstone_script::{parse, Program, Value, Vars};

use crate::{read_persistent, rebuild_pending, Cmd, Input, Session, MAX_HISTORY, MAX_LIST_ITEMS, SAVE_HISTORY};

fn persistent(pairs: &[(&str, Value)]) -> Vars {
    pairs.iter().map(|(k, v)| (k.to_string(), v.clone())).collect()
}

/// What the screen shows must be what the script and the variables say it is.
fn screen_matches_state(s: &Session) {
    let st = s.state();
    if let Some(op) = s.program.ops.get(st.pc) {
        if let (Some(pending @ (Cmd::ShowChoices { .. } | Cmd::ShowHotspots { .. } | Cmd::ShowShop { .. })), Some(want)) =
            (&st.pending, rebuild_pending(s.program, op, &st.vars))
        {
            assert_eq!(*pending, want, "the waiting screen is out of step with the variables");
        }
    }
}

fn labels(cmds: &[Cmd]) -> Vec<String> {
    for c in cmds {
        match c {
            Cmd::ShowHotspots { spots } => return spots.iter().map(|s| s.label.clone()).collect(),
            Cmd::ShowChoices { options } => return options.clone(),
            _ => {}
        }
    }
    Vec::new()
}

fn shop_names(cmds: &[Cmd]) -> Vec<String> {
    cmds.iter()
        .find_map(|c| match c {
            Cmd::ShowShop { items, .. } => Some(items.iter().map(|i| i.item.id.clone()).collect()),
            _ => None,
        })
        .unwrap_or_default()
}

// ---- 1: overlay rebuilds what is waiting --------------------------------------------

#[test]
fn the_hotspots_demo_rolled_back_over_the_persistent_scarf_picks_the_spot_shown() {
    let p = parse(include_str!("../../../examples/hotspots.quill.md")).unwrap();
    let (mut game, _) = Session::start(&p);
    let map = game.input(Input::Advance);
    assert_eq!(labels(&map).len(), 5);
    game.input(Input::Choose(3)); // the hidden scarf: found_scarf = true, kept for good
    let after = game.input(Input::Advance);
    assert_eq!(labels(&after), ["The rooftop", "The old bridge", "The clock tower", "Leave town"]);
    game.rollback();
    let back = game.rollback().unwrap();
    // the scarf stays found, so the map is shown without it, and that is what Choose(i) means
    assert_eq!(labels(&back), ["The rooftop", "The old bridge", "The clock tower", "Leave town"]);
    screen_matches_state(&game);
    game.input(Input::Choose(3));
    assert!(game.state().line.as_ref().unwrap().text.contains("leave town"), "index 3 is the Leave town spot that was shown");
    // and forward again
    game.rollback();
    assert!(game.roll_forward().is_some());
    screen_matches_state(&game);
}

const CHOICES: &str = "---\npersistent: { met: false, who: \"nobody\" }\n---\n# start\n::choice\n- [Meet {who}](#a){met=true, if not met}\n- [Walk on](#b)\n::\n# a\nMet.\n::jump{to=start}\n# b\nWalked.\n";

#[test]
fn choices_follow_a_persistent_value_through_rollback_roll_forward_and_set_persistent() {
    let p = parse(CHOICES).unwrap();
    let (mut game, first) = Session::start(&p);
    assert_eq!(labels(&first), ["Meet nobody", "Walk on"]);
    game.input(Input::Choose(0));
    let again = game.input(Input::Advance);
    assert_eq!(labels(&again), ["Walk on"]);
    let back = game.rollback().and_then(|_| game.rollback()).unwrap();
    assert_eq!(labels(&back), ["Walk on"], "rollback keeps what was kept for good");
    screen_matches_state(&game);
    game.roll_forward();
    game.roll_forward();
    screen_matches_state(&game);
    assert_eq!(labels(&game.state().redraw()), ["Walk on"]);

    // set_persistent, as the frontend does after reading the file
    let (mut fresh, first) = Session::start(&p);
    assert_eq!(labels(&first).len(), 2);
    fresh.set_persistent(persistent(&[("met", Value::Bool(true)), ("who", Value::Str("Mika".into()))]));
    screen_matches_state(&fresh);
    assert_eq!(labels(&fresh.state().redraw()), ["Walk on"]);
    fresh.input(Input::Choose(0));
    assert_eq!(fresh.state().pc, p.labels["b"], "Choose(0) picks the option on screen");
}

#[test]
fn load_with_rebuilds_the_screen_for_changed_persistent_values() {
    let p = parse(CHOICES).unwrap();
    let (game, _) = Session::start(&p);
    let save = game.save();
    let kept = persistent(&[("met", Value::Bool(true)), ("who", Value::Str("Mika".into()))]);
    let (mut loaded, cmds) = Session::load_with(&p, &save, kept).unwrap();
    assert_eq!(labels(&cmds), ["Walk on"], "the commands drawn are the rebuilt screen");
    screen_matches_state(&loaded);
    loaded.input(Input::Choose(0));
    assert_eq!(loaded.state().pc, p.labels["b"]);
}

#[test]
fn a_waiting_line_is_interpolated_again_and_a_choice_that_empties_is_passed() {
    let p = parse("---\npersistent: { rank: \"none\", open: true }\n---\nRank: {rank}.\n::choice\n- [Only](#x){if open}\n::\nPast it.\n# x\nTook it.\n").unwrap();
    let (mut game, first) = Session::start(&p);
    assert!(matches!(&first[0], Cmd::Say { text, .. } if text == "Rank: none."));
    game.set_persistent(persistent(&[("rank", Value::Str("gold".into()))]));
    assert!(matches!(&game.state().pending, Some(Cmd::Say { text, .. }) if text == "Rank: gold."));
    assert_eq!(game.state().line.as_ref().unwrap().text, "Rank: gold.");
    game.input(Input::Advance);
    assert_eq!(labels(&game.state().redraw()), ["Only"]);
    // the only option goes away while the story waits at it: it does not stay stuck
    game.set_persistent(persistent(&[("open", Value::Bool(false))]));
    assert!(matches!(&game.state().pending, Some(Cmd::Say { text, .. }) if text == "Past it."), "{:?}", game.state().pending);
}

const SHOP: &str = "---\ninventory: bag\npersistent: { vip: false }\nvars: { gold: 40, bag: [] }\n---\n::item{id=key price=15}\n::item{id=lamp price=30}\n# shop\n::shop{pay=gold into=bag}\n- key\n- lamp{if vip}\n::\nDone.\n";

#[test]
fn a_shop_follows_persistent_stock_so_buy_i_buys_what_is_listed() {
    let p = parse(SHOP).unwrap();
    let (mut game, first) = Session::start(&p);
    assert_eq!(shop_names(&first), ["key"]);
    game.set_persistent(persistent(&[("vip", Value::Bool(true))]));
    assert_eq!(shop_names(&game.state().redraw()), ["key", "lamp"]);
    screen_matches_state(&game);
    game.input(Input::Buy(1));
    assert_eq!(game.state().vars["bag"], Value::List(vec!["lamp".into()]));
    assert_eq!(game.state().vars["gold"], Value::Int(10));
}

// ---- 2: nothing on offer ------------------------------------------------------------------

const GATED: &str = "---\nvars: { flag: true }\n---\n::choice\n- [Go](#a){if flag}\n::\nAfter.\n# a\nWent.\n";

#[test]
fn a_save_at_a_choice_with_nothing_on_offer_moves_on_instead_of_sticking() {
    let p = parse(GATED).unwrap();
    let (game, _) = Session::start(&p);
    let save = game.save();
    assert!(matches!(game.state().pending, Some(Cmd::ShowChoices { .. })));
    // hand-edited variable
    let hand = save.replace("var \"flag\" bool true", "var \"flag\" bool false");
    let (loaded, cmds) = Session::load(&p, &hand).unwrap();
    assert!(!matches!(loaded.state().pending, Some(Cmd::ShowChoices { .. })));
    assert!(cmds.iter().any(|c| matches!(c, Cmd::Say { text, .. } if text == "After.")), "{cmds:?}");
    // the script edited so the option is never on offer (hot reload goes through load)
    let edited = parse(&GATED.replace("if flag", "if never")).unwrap();
    let (loaded, cmds) = Session::load(&edited, &save).unwrap();
    assert!(cmds.iter().any(|c| matches!(c, Cmd::Say { text, .. } if text == "After.")));
    assert!(matches!(loaded.state().pending, Some(Cmd::Say { .. })));
}

#[test]
fn a_save_at_an_empty_shop_moves_on_too() {
    let p = parse(SHOP).unwrap();
    let (game, _) = Session::start(&p);
    let src = SHOP.replace("- key\n", "- key{if never}\n").replace("- lamp{if vip}", "- lamp{if never}");
    let none = parse(&src).unwrap();
    let (loaded, cmds) = Session::load(&none, &game.save()).unwrap();
    assert!(cmds.iter().any(|c| matches!(c, Cmd::Say { text, .. } if text == "Done.")), "{cmds:?}");
    assert!(matches!(loaded.state().pending, Some(Cmd::Say { .. })));
}

#[test]
fn saved_choice_text_is_not_trusted() {
    let p = parse(CHOICES).unwrap();
    let (game, _) = Session::start(&p);
    let save = game.save();
    let forged = save.replace("pending choices \"Meet nobody\" \"Walk on\"", "pending choices \"Nope\"");
    assert_ne!(save, forged, "the record is there to edit");
    let (loaded, cmds) = Session::load(&p, &forged).unwrap();
    assert_eq!(labels(&cmds), ["Meet nobody", "Walk on"]);
    assert_eq!(loaded.state().pending, game.state().pending);
}

// ---- 3 and 4: hostile saves --------------------------------------------------------------------

const INV: &str = "---\ninventory: inv\nvars: { gold: 20, inv: [], n: 0 }\n---\n::item{id=k price=5}\n# shop\n::shop{pay=gold into=inv}\n- k\n::\nDone.\n";

fn save_with_var(p: &Program, record: &str) -> String {
    let (game, _) = Session::start(p);
    let save = game.save();
    let end = save.rfind("end\n").unwrap();
    format!("{}{record}\n{}", &save[..end], &save[end..])
}

#[test]
fn a_huge_list_in_a_save_is_refused_fast() {
    let p = parse(INV).unwrap();
    let items: String = (0..200_000).map(|i| format!(" i{i}")).collect();
    let text = save_with_var(&p, &format!("var \"inv\" list{items}"));
    let t = Instant::now();
    assert!(Session::load(&p, &text).is_err());
    assert!(read_persistent(&format!("quillstone-persistent 1\nvar \"x\" list{items}\n")).is_empty());
    assert!(t.elapsed() < Duration::from_secs(2), "took {:?}", t.elapsed());
}

#[test]
fn a_list_at_the_limit_with_many_repeats_loads_fast_and_deduplicates() {
    let p = parse(INV).unwrap();
    let repeats: String = (0..MAX_LIST_ITEMS).map(|i| format!(" r{}", i % 7)).collect();
    let t = Instant::now();
    let (s, _) = Session::load(&p, &save_with_var(&p, &format!("var \"inv\" list{repeats}"))).unwrap();
    assert!(t.elapsed() < Duration::from_secs(1));
    assert_eq!(s.state().vars["inv"], Value::List((0..7).map(|i| format!("r{i}")).collect()));
}

#[test]
fn a_save_with_too_many_states_or_stack_records_is_refused() {
    let p = parse(INV).unwrap();
    let (game, _) = Session::start(&p);
    let one = game.save().replacen("quillstone-session 1\n", "", 1);
    let many = format!("quillstone-session 1\n{}", one.repeat(1_001));
    // states without inputs between them are refused for that before anything else
    assert!(Session::load(&p, &many).is_err());
    let stack = "stack \"\" 0\n".repeat(300);
    assert!(Session::load(&p, &game.save().replacen("state\n", &format!("state\n{stack}"), 1)).err().unwrap().contains("stack"));
}

#[test]
fn a_save_with_too_many_repeated_records_in_a_state_is_refused() {
    let p = parse(INV).unwrap();
    // (what the error names, the limit, one record with number i)
    let cases = [
        ("sprite", 256, "sprite \"s{i}\" \"a.png\" 0.5 1"),
        ("inline", 256, "inline \"a.png\" \"alt\""),
        ("sfxloop", 64, "sfxloop \"a{i}.ogg\" 1"),
        ("undeclared variables", 1_024, "var \"v{i}\" int 1"),
    ];
    for (what, limit, record) in cases {
        let records = |n: usize| (0..n).map(|i| record.replace("{i}", &i.to_string())).collect::<Vec<_>>().join("\n");
        assert!(Session::load(&p, &save_with_var(&p, &records(limit))).is_ok(), "{what} at the limit loads");
        let err = Session::load(&p, &save_with_var(&p, &records(limit + 1))).err().expect(what);
        assert!(err.contains(&format!("more than {limit}")) && err.contains(what), "{err}");
    }
}

#[test]
fn a_huge_position_offset_does_not_overflow() {
    let p = parse("# a\n**A:** one\n# b\n**A:** two\n").unwrap();
    let text = format!("quillstone-session 1\nstate\nat \"b\" {}\nend\n", usize::MAX);
    let (s, _) = Session::load(&p, &text).unwrap();
    assert!(s.state().pc <= p.ops.len());
    let stack = format!("quillstone-session 1\nstate\nat \"a\" 0\nstack \"b\" {}\nend\n", usize::MAX);
    assert!(Session::load(&p, &stack).is_ok());
}

#[test]
fn a_float_from_a_save_is_sane() {
    let p = parse("::bg{src=a.png}\n::show{x src=x.png}\n**A:** hi\n").unwrap();
    let (game, _) = Session::start(&p);
    let mut text = game.save();
    let junk = "state\nsprite \"x\" \"x.png\" NaN inf -inf 500 5\ncamera 1e30 1e30 -1e30\nmusic \"m.ogg\" true NaN\nsfxloop \"w.ogg\" inf\n";
    text = text.replacen("state\n", junk, 1);
    let (s, _) = Session::load(&p, &text).unwrap();
    let st = s.state();
    let sp = &st.sprites[0];
    assert_eq!((sp.x, sp.scale, sp.y, sp.z), (0.5, 1.0, 1.0, 100), "NaN and inf fall back, huge clamps");
    assert!((0.0..=1.0).contains(&sp.x) && (0.05..=10.0).contains(&sp.scale) && (-0.5..=1.5).contains(&sp.y));
    assert_eq!((st.camera.zoom, st.camera.x, st.camera.y), (4.0, 1.0, 0.0));
    assert_eq!(st.music.as_ref().unwrap().volume, 1.0);
    assert_eq!(st.sfx_loops[0].volume, 1.0);
    for bad in ["NaN", "inf", "1e30", "-5"] {
        let w = parse("::wait{time=1}\n").unwrap();
        let (g, _) = Session::start(&w);
        let text = g.save().replace("pending wait 1", &format!("pending wait {bad}"));
        let (s, _) = Session::load(&w, &text).unwrap();
        let Some(Cmd::Wait { seconds }) = s.state().pending else { panic!("{:?}", s.state().pending) };
        assert!(seconds.is_finite() && seconds > 0.0 && seconds <= 3600.0, "{bad} -> {seconds}");
    }
}

// ---- 5: wrong-typed vars and buy ---------------------------------------------------------------

#[test]
fn a_var_of_the_wrong_kind_is_ignored_and_buy_refuses_without_charging() {
    let p = parse(INV).unwrap();
    let text = save_with_var(&p, "var \"inv\" int 3\nvar \"n\" list \"a\"");
    let (mut s, _) = Session::load(&p, &text).unwrap();
    assert_eq!(s.state().vars["inv"], Value::List(vec![]), "the declared list stays a list");
    assert_eq!(s.state().vars["n"], Value::Int(0), "and the number a number");
    // numbers, text and booleans may turn into each other as the story runs
    let text = save_with_var(&p, "var \"n\" text \"x\"");
    assert_eq!(Session::load(&p, &text).unwrap().0.state().vars["n"], Value::Str("x".into()));
    // even if a bad value got in some other way, a purchase takes nothing
    s.state.vars.insert("inv".into(), Value::Int(3));
    assert!(s.input(Input::Buy(0)).is_empty());
    assert_eq!(s.state.vars["gold"], Value::Int(20));
    s.state.vars.insert("inv".into(), Value::List(vec![]));
    s.state.vars.insert("gold".into(), Value::Str("lots".into()));
    assert!(s.input(Input::Buy(0)).is_empty());
    assert_eq!(s.state.vars["inv"], Value::List(vec![]));
    s.state.vars.insert("gold".into(), Value::Int(20));
    assert!(!s.input(Input::Buy(0)).is_empty(), "a sound state still buys");
}

// ---- history cap ---------------------------------------------------------------------------------

#[test]
fn history_is_capped_rollback_stops_and_saves_keep_their_window() {
    let p = parse("# a\n**A:** Again.\n::jump{to=a}\n").unwrap();
    let (mut game, _) = Session::start(&p);
    for _ in 0..MAX_HISTORY + 100 {
        game.input(Input::Advance);
    }
    assert_eq!(game.history.len(), MAX_HISTORY);
    assert_eq!(game.backlog().len(), MAX_HISTORY + 1, "the backlog covers the retained history");
    let save = game.save();
    assert_eq!(save.matches("\nstate\n").count(), SAVE_HISTORY + 1);
    let mut back = 0;
    while game.rollback().is_some() {
        back += 1;
    }
    assert_eq!(back, MAX_HISTORY, "rollback past the cap simply stops");
    assert!(game.redo.len() <= MAX_HISTORY);
    while game.roll_forward().is_some() {}
    assert_eq!(game.history.len(), MAX_HISTORY);
}
