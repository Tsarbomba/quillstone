//! A character's name template on the name plate, the History screen and a save slot preview.

use super::test_util::{instant};
use super::*;
use quillstone_script::parse;

const SRC: &str = "---\nvars: { hero.name: \"Alex\", hname: \"Zed\" }\n---\n::character{id=h name=\"{hero.name}\" color=#ffd98c}\n\
::character{id=z name=\"{hname}\"}\n**z:** Before.\n::input{var=hero.name prompt=\"Name?\" max=12}\n**{hero.name}:** Hello.\n\
**{hname}:** Second.\n**{hero.name}:** Third.\n";

fn speaker(g: &Game) -> Option<String> {
    g.view().dialogue.as_ref().and_then(|d| d.speaker.clone())
}

#[test]
fn the_name_plate_shows_the_typed_name_and_the_backlog_the_old_ones() {
    let p = parse(SRC).unwrap();
    let mut g = Game::new(&p, instant(), Box::new(crate::MemoryStorage::default()));
    g.start_story();
    assert_eq!(speaker(&g).as_deref(), Some("Zed"));
    g.handle(Event::Key(Key::Space));
    g.handle(Event::Text("{b}Rin".into()));
    g.handle(Event::Key(Key::Enter));
    assert_eq!(speaker(&g).as_deref(), Some("{b}Rin"), "typed braces show as typed");
    let plate = g.draw_list(&|_| None).iter().any(|d| matches!(d, Draw::Text { text, .. } if text == "{b}Rin"));
    assert!(plate, "the name plate is drawn with the typed name");
    g.handle(Event::Key(Key::Space));
    assert_eq!(speaker(&g).as_deref(), Some("Zed"), "second template");
    let names: Vec<_> = g
        .session
        .backlog()
        .into_iter()
        .filter_map(|l| match l {
            quillstone_runtime::Logged::Line(l) => l.speaker,
            _ => None,
        })
        .collect();
    assert_eq!(names, ["Zed", "{b}Rin", "Zed"]);
}
