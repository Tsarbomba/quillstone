//! `::message` and `::reply`: threads, unread marks, both reply kinds, rollback, saves and the story tester.

use quillstone_script::parse;

use crate::explore::explore;
use crate::test_util::says;
use crate::{can_reply, reply_options, Cmd, Input, Session, State};

const FRONT: &str = "---\nvars: { met: 0 }\n---\n::character{id=mika name=Mika}\n";

fn story(body: &str) -> String {
    format!("{FRONT}{body}")
}

const NOW: &str = "# start\n::message{from=mika text=\"Running late, {met}!\"}\n::message{to=mika text=\"ok\" image=a.png}\n::message{from=mika video=c.webm thumb=t.png quiet}\n::reply{to=mika}\n- [Sure](#yes){met+=1}\n- [No](#no)\n::\n# yes\n**A:** yes\n::end\n# no\n**A:** no\n";

fn thread(s: &State) -> &crate::Thread {
    &s.threads[0]
}

#[test]
fn messages_join_the_thread_unread_until_opened_and_quiet_ones_still_arrive() {
    let p = parse(&story(NOW)).unwrap();
    let (s, cmds) = Session::start(&p);
    let t = thread(s.state());
    assert_eq!((t.id.as_str(), t.msgs.len(), t.unread), ("mika", 3, 2), "two from her, one from me");
    assert_eq!(t.msgs[0].text.as_deref(), Some("Running late, 0!"));
    assert!(t.msgs[1].mine && t.msgs[1].image.as_deref() == Some("a.png"));
    assert_eq!(t.msgs[2].video, Some(("c.webm".into(), Some("t.png".into()))));
    let notices: Vec<_> = cmds.iter().filter_map(|c| if let Cmd::Message { name, mine, quiet, .. } = c { Some((name.as_str(), *mine, *quiet)) } else { None }).collect();
    assert_eq!(notices, [("Mika", false, false), ("Mika", true, false), ("Mika", false, true)]);
    let mut s = s;
    s.mark_read("mika");
    assert_eq!(s.state().threads[0].unread, 0);
}

#[test]
fn a_reply_the_story_waits_for_adds_the_pick_as_my_message_and_rolls_back() {
    let p = parse(&story(NOW)).unwrap();
    let (mut s, _) = Session::start(&p);
    assert!(matches!(s.state().pending, Some(Cmd::ShowChoices { .. })));
    let cmds = s.input(Input::Choose(0));
    assert!(says(&cmds, "yes"));
    let t = thread(s.state());
    assert_eq!(t.msgs.last().unwrap().text.as_deref(), Some("Sure"));
    assert!(t.msgs.last().unwrap().mine && s.state().vars["met"] == quillstone_script::Value::Int(1));
    s.rollback();
    assert_eq!(thread(s.state()).msgs.len(), 3, "going back un-sends it");
}

const LATER: &str = "# start\n::reply{to=mika later}\n- [Fine](#fine){met+=1}\n- [Hmm](#hmm){if met > 5}\n::\n**A:** one\n**A:** two\n::end\n# fine\n**B:** fine!\n::return\n# hmm\n**B:** hmm\n::return\n";

#[test]
fn a_later_reply_waits_in_the_thread_and_picking_it_calls_the_label_and_returns_to_the_line() {
    let p = parse(&story(LATER)).unwrap();
    let (mut s, first) = Session::start(&p);
    assert!(says(&first, "one"), "the story did not wait");
    let st = s.state().clone();
    assert_eq!(reply_options(&p, &st, "mika", &|t| t.to_string()), [(0, "Fine".to_string())], "the `if` hides one");
    assert!(can_reply(&p, &st, "mika", 0) && !can_reply(&p, &st, "mika", 1));
    assert!(s.input(Input::Reply("mika".into(), 1)).is_empty(), "a hidden option can't be picked");
    let cmds = s.input(Input::Reply("mika".into(), 0));
    assert!(says(&cmds, "fine!"));
    assert!(thread(s.state()).reply.is_none() && thread(s.state()).msgs[0].mine);
    let cmds = s.input(Input::Advance);
    assert!(says(&cmds, "one"), "back at the line it was at: {cmds:?}");
    assert_eq!(s.state().vars["met"], quillstone_script::Value::Int(1));
    // rollback takes the reply back
    s.rollback();
    s.rollback();
    assert_eq!(thread(s.state()).reply, Some(0));
}

#[test]
fn a_new_reply_replaces_a_pending_one_and_none_cancels_it() {
    let body = "# start\n::reply{to=mika later}\n- [A](#a)\n::\n::reply{to=mika later}\n- [B](#a)\n::\n**X:** x\n::reply{to=mika none}\n**X:** y\n# a\n**X:** a\n";
    let p = parse(&story(body)).unwrap();
    let (mut s, _) = Session::start(&p);
    assert_eq!(reply_options(&p, s.state(), "mika", &|t| t.to_string()), [(0, "B".to_string())]);
    s.input(Input::Advance);
    assert!(reply_options(&p, s.state(), "mika", &|t| t.to_string()).is_empty());
}

#[test]
fn threads_and_a_pending_reply_survive_a_save_and_a_load_with_the_history() {
    let p = parse(&story(LATER)).unwrap();
    let (mut s, _) = Session::start(&p);
    s.input(Input::Reply("mika".into(), 0));
    let text = s.save();
    let (mut loaded, _) = Session::load(&p, &text).unwrap();
    assert_eq!(loaded.state().threads, s.state().threads);
    assert!(loaded.rollback().is_some());
    assert_eq!(thread(loaded.state()).reply, Some(0), "the reply input is in the saved history");
    let now = parse(&story(NOW)).unwrap();
    let (s2, _) = Session::start(&now);
    let (l2, _) = Session::load(&now, &s2.save()).unwrap();
    assert_eq!(l2.state().threads, s2.state().threads);
    assert!(Session::load(&now, &s2.save().replace("\"mika\" 2", "\"zzz zzz\" 2")).is_err());
}

#[test]
fn the_story_tester_tries_a_later_reply_option_as_a_branch() {
    let body = "# start\n::reply{to=mika later}\n- [Fine](#fine)\n::\n**A:** one\n::end\n# fine\n**B:** fine!\n::return\n";
    let p = parse(&story(body)).unwrap();
    let r = explore(&p, 1000);
    assert!(r.errors.is_empty() && r.warnings.is_empty() && !r.partial, "{r:?}");
}
