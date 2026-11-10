//! A character's `portrait`: the game finds the speaker by the name the line carries, also translated.

use super::test_util::{instant, instant_game, key};
use super::*;
use quillstone_render::Size;
use quillstone_script::{parse, Catalog};

const BODY: &str = "::character{id=mika name=Mika portrait=faces/mika.png}\n::character{id=jo name=Jo}\n\
    **Mika:** Hi.\n\n**Jo:** Hello.\n\nNarration.\n\n**Mika:** Back.\n";

/// The pictures drawn, by file name.
fn pictures(g: &Game) -> Vec<String> {
    let any = |_: &str| Some(Size::new(100.0, 100.0));
    g.draw_list(&any).into_iter().filter_map(|d| if let Draw::Image { src, .. } = d { Some(src) } else { None }).collect()
}

#[test]
fn the_portrait_shows_with_its_characters_lines_only_also_after_a_rollback() {
    let p = parse(BODY).unwrap();
    let mut g = instant_game(&p);
    g.tick(0.5, false);
    assert_eq!(pictures(&g), ["faces/mika.png"]);
    for _ in 0..3 {
        g.handle(Event::Key(Key::Space));
        g.tick(0.5, false);
    }
    assert_eq!(pictures(&g), ["faces/mika.png"], "Mika again");
    g.handle(Event::Key(Key::Backspace));
    assert!(pictures(&g).is_empty(), "narration, after a rollback");
    g.handle(Event::Key(Key::Backspace));
    assert!(pictures(&g).is_empty(), "Jo has none");
    g.handle(Event::Key(Key::Backspace));
    assert_eq!(pictures(&g), ["faces/mika.png"]);
}

#[test]
fn hiding_the_interface_hides_the_portrait() {
    let p = parse(BODY).unwrap();
    let mut g = instant_game(&p);
    g.tick(0.5, false);
    g.handle(Event::Key(Key::Char('u')));
    g.tick(1.0, false);
    assert!(pictures(&g).is_empty());
}

#[test]
fn a_translated_speaker_name_still_finds_the_portrait() {
    let head = "---\ntitle: Tale\nlanguages: en=English, sv=Svenska\n---\n";
    let p = parse(&format!("{head}{BODY}")).unwrap();
    let mut g = Game::new(&p, instant(), Box::new(crate::MemoryStorage::default()));
    g.set_translations([("sv".to_string(), Catalog::parse("> Mika\n= Mikaela\n\n> Hi.\n= Hej.\n").unwrap())].into());
    g.start_story();
    g.open_settings();
    for _ in 0..9 {
        key(&mut g, Key::Down);
    }
    key(&mut g, Key::Right);
    key(&mut g, Key::Escape);
    assert_eq!(g.view().dialogue.as_ref().unwrap().speaker.as_deref(), Some("Mikaela"));
    assert_eq!(pictures(&g), ["faces/mika.png"]);
}
