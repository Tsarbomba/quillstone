//! The Chapters screen: the menu button, locked and open tiles, starting a new game at a chapter, and paging.

use super::test_util::{click, instant, key, line};
use super::*;
use crate::chapters::{tile_rect, PER_PAGE};
use quillstone_runtime::write_persistent;
use quillstone_script::{parse, Value, Vars};

const SRC: &str = "---\nvars: { trust: 3 }\npersistent: { runs: 0 }\nchapters: { one: \"Part one\", two: \"Part two\", three: \"Part three\" {open} }\n---\n# start\n::set{trust = 9}\n**A:** begin\n# one\n**A:** one with {trust}\n# two\n**A:** two with {trust}\n# three\n**A:** three with {trust}\n";

fn menu_game<'p>(p: &'p Program, kept: &[&str]) -> Game<'p> {
    let mut storage = crate::MemoryStorage::default();
    let mut vars = Vars::new();
    for label in kept {
        vars.insert(format!("chapter:{label}"), Value::Bool(true));
    }
    storage.files.insert("persistent.txt".into(), write_persistent(&vars));
    Game::new(p, instant(), Box::new(storage))
}

fn open_chapters(g: &mut Game) {
    let buttons = menu_buttons(g.newest_save.is_some(), g.has_extras(), g.has_chapters());
    let at = buttons.iter().position(|b| *b == MenuButton::Chapters).expect("a Chapters button");
    click(g, menu_button_rect(at, &Theme::default()));
    assert_eq!(g.screen(), Screen::Chapters);
}

fn texts(g: &Game) -> Vec<String> {
    g.draw_list(&|_| None).iter().filter_map(|d| if let Draw::Text { text, .. } = d { Some(text.clone()) } else { None }).collect()
}

#[test]
fn the_menu_offers_chapters_only_to_a_game_that_has_them() {
    let with = parse(SRC).unwrap();
    assert!(menu_buttons(false, false, !with.chapters.is_empty()).contains(&MenuButton::Chapters));
    let without = parse("Hi.\n").unwrap();
    let g = menu_game(&without, &[]);
    assert!(!g.has_chapters() && !menu_buttons(false, false, g.has_chapters()).contains(&MenuButton::Chapters));
    assert!(texts(&menu_game(&with, &[])).contains(&"Chapters".to_string()));
}

#[test]
fn an_open_tile_is_titled_and_a_locked_one_is_a_question_mark() {
    let p = parse(SRC).unwrap();
    let mut g = menu_game(&p, &["one"]);
    open_chapters(&mut g);
    let shown = texts(&g);
    assert!(shown.contains(&"Part one".to_string()) && shown.contains(&"Part three".to_string()), "{shown:?}");
    assert!(shown.contains(&"?".to_string()) && !shown.contains(&"Part two".to_string()), "the locked one hides its title: {shown:?}");
    // a locked tile does nothing but say so
    click(&mut g, tile_rect(1));
    assert!(g.screen() == Screen::Chapters && !g.in_story);
    // unlock-all opens it
    g.set_unlock_all(true);
    assert!(texts(&g).contains(&"Part two".to_string()));
}

#[test]
fn picking_an_open_tile_starts_a_new_game_at_its_label_with_the_front_matter_variables() {
    let p = parse(SRC).unwrap();
    let mut g = menu_game(&p, &[]);
    open_chapters(&mut g);
    click(&mut g, tile_rect(2)); // `three` is open from the first launch
    assert_eq!((g.screen(), g.in_story, g.in_replay()), (Screen::Story, true, false));
    assert_eq!(line(&g), "three with 3", "the story's own `trust = 9` never ran");
    assert!(g.session.persistent().get("chapter:three").is_none(), "an open chapter writes nothing");
    // an unlocked one starts too, by keyboard
    let mut g = menu_game(&p, &["one"]);
    open_chapters(&mut g);
    key(&mut g, Key::Right);
    key(&mut g, Key::Enter);
    assert_eq!(line(&g), "one with 3");
}

#[test]
fn escape_and_back_return_to_the_main_menu() {
    let p = parse(SRC).unwrap();
    let mut g = menu_game(&p, &[]);
    open_chapters(&mut g);
    key(&mut g, Key::Escape);
    assert_eq!(g.screen(), Screen::MainMenu);
}

#[test]
fn more_chapters_than_a_page_hold_turn_pages() {
    let list: Vec<String> = (0..16).map(|i| format!("c{i}: \"Chapter {i}\" {{open}}")).collect();
    let body: String = (0..16).map(|i| format!("# c{i}\n**A:** chapter {i}\n")).collect();
    let p = parse(&format!("---\nchapters: {{ {} }}\n---\n{body}", list.join(", "))).unwrap();
    let mut g = menu_game(&p, &[]);
    open_chapters(&mut g);
    assert!(texts(&g).contains(&"Page 1 of 2".to_string()), "{:?}", texts(&g));
    assert!(!texts(&g).contains(&"Chapter 12".to_string()));
    key(&mut g, Key::Right); // focus the first tile
    for _ in 0..3 {
        key(&mut g, Key::Right);
    }
    key(&mut g, Key::Right); // off the side of the row: next page
    assert!(g.screen() == Screen::Chapters && texts(&g).contains(&"Chapter 12".to_string()), "{:?}", texts(&g));
    click(&mut g, tile_rect(PER_PAGE - 12 + 3));
    assert_eq!(line(&g), "chapter 15");
}
