//! The Messages screen in the game shell: opening it, reading, both kinds of reply, and the enlarged picture or video.

use super::test_util::{instant, key, line, new_game};
use super::*;
use quillstone_script::parse;

const PHONE: &str = "---\nvars: { met: 0 }\n---\n::character{id=mika name=Mika}\n::hud{id=ph text=Phone open=messages x=0.9 y=0.05}\n**A:** one\n::message{from=mika text=\"Hi\"}\n::message{from=mika video=c.webm}\n::message{from=mika image=a.png}\n::reply{to=mika later}\n- [Fine](#fine){met+=1}\n::\n**A:** two\n::reply{to=mika}\n- [Sure](#end)\n::\n# end\n**A:** done\n::end\n# fine\n**B:** fine!\n::return\n";

fn texts(g: &Game) -> Vec<String> {
    g.draw_list(&|_| None).iter().filter_map(|d| if let Draw::Text { text, .. } = d { Some(text.clone()) } else { None }).collect()
}

/// At "two" with the phone's HUD button focused and pressed: the contact list.
fn open_list(g: &mut Game) {
    key(g, Key::Space);
    assert_eq!(line(g), "two");
    key(g, Key::Down);
    key(g, Key::Enter);
    assert_eq!(g.screen(), Screen::Messages);
}

fn open_thread(g: &mut Game) {
    open_list(g);
    key(g, Key::Down);
    key(g, Key::Enter);
    assert_eq!(g.msgs.thread.as_deref(), Some("mika"));
}

#[test]
fn the_list_shows_the_contact_unread_and_waiting_and_opening_the_thread_marks_it_read() {
    let p = parse(PHONE).unwrap();
    let mut g = new_game(&p, instant());
    open_list(&mut g);
    let t = texts(&g);
    assert!(t.contains(&"Mika".to_string()) && t.contains(&"Waiting for your reply".to_string()), "{t:?}");
    assert_eq!(g.state().threads[0].unread, 3);
    key(&mut g, Key::Down);
    key(&mut g, Key::Enter);
    assert_eq!(g.state().threads[0].unread, 0);
    assert!(texts(&g).contains(&"Fine".to_string()), "the later reply's option waits at the foot");
    key(&mut g, Key::Escape); // back to the list, then out
    assert_eq!(g.msgs.thread, None);
    key(&mut g, Key::Escape);
    assert_eq!((g.screen(), line(&g).as_str()), (Screen::Story, "two"));
}

#[test]
fn a_later_reply_is_picked_in_the_thread_and_the_story_returns_to_its_line_then_waits_at_the_reply() {
    let p = parse(PHONE).unwrap();
    let mut g = new_game(&p, instant());
    open_thread(&mut g);
    key(&mut g, Key::Down); // the first option
    key(&mut g, Key::Enter);
    assert_eq!((g.screen(), line(&g).as_str()), (Screen::Story, "fine!"));
    assert_eq!(g.state().vars["met"], Value::Int(1));
    key(&mut g, Key::Space);
    assert_eq!(line(&g), "two", "back at the line it was at");
    key(&mut g, Key::Space);
    // a reply the story waits for opens the screen on its own, at that thread
    assert_eq!((g.screen(), g.msgs.thread.as_deref()), (Screen::Messages, Some("mika")));
    assert!(texts(&g).contains(&"Sure".to_string()));
    key(&mut g, Key::Down);
    key(&mut g, Key::Enter);
    assert_eq!((g.screen(), line(&g).as_str()), (Screen::Story, "done"));
    let last = g.state().threads[0].msgs.last().unwrap().clone();
    assert!(last.mine && last.text.as_deref() == Some("Sure"));
}

#[test]
fn a_picture_or_video_opens_in_a_panel_and_the_video_plays_once_with_sound() {
    let p = parse(PHONE).unwrap();
    let mut g = new_game(&p, instant());
    open_thread(&mut g);
    key(&mut g, Key::Left); // the newest picture: a.png
    key(&mut g, Key::Enter);
    assert_eq!(g.msgs.viewing.as_ref().map(|v| (v.src.as_str(), v.video)), Some(("a.png", false)));
    assert!(g.wanted_videos().is_empty());
    key(&mut g, Key::Escape);
    assert!(g.msgs.viewing.is_none() && g.screen() == Screen::Messages);
    key(&mut g, Key::Left);
    key(&mut g, Key::Enter);
    let wanted = g.wanted_videos();
    assert_eq!(wanted.len(), 1);
    assert_eq!((wanted[0].src.as_str(), wanted[0].looped, wanted[0].sound), ("c.webm", false, true));
    assert!(!g.media_paused(), "its clock runs although a screen is over the story");
    let generation = wanted[0].generation;
    key(&mut g, Key::Enter);
    assert_ne!(g.wanted_videos()[0].generation, generation, "a click plays it again");
    key(&mut g, Key::Escape);
    assert!(g.wanted_videos().is_empty() && g.media_paused());
}

#[test]
fn a_message_notice_is_shown_unless_quiet_and_never_for_the_players_own() {
    let src = PHONE.replace("::message{from=mika text=\"Hi\"}", "::message{from=mika text=\"Hi\"}\n::message{to=mika text=\"Yo\"}\n::message{from=mika text=\"Shh\" quiet}");
    let p = parse(&src).unwrap();
    let mut g = new_game(&p, instant());
    key(&mut g, Key::Space);
    assert_eq!(g.notice.as_ref().map(|(m, _)| m.as_str()), Some("New message from Mika"));
    g.notice = None;
    let p2 = parse(&PHONE.replace("::message{from=mika text=\"Hi\"}\n::message{from=mika video=c.webm}\n::message{from=mika image=a.png}", "::message{to=mika text=\"Yo\"}\n::message{from=mika text=\"Shh\" quiet}")).unwrap();
    let mut g = new_game(&p2, instant());
    key(&mut g, Key::Space);
    assert!(g.notice.is_none());
}
