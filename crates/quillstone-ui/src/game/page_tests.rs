//! Full-screen pages in the game: advancing through a page passage, going back in the middle and
//! saving and loading in the middle bring the same page back.

use super::test_util::{instant_game, key};
use super::*;
use quillstone_render::Size;
use quillstone_script::parse;

const BODY: &str = "::character{id=mika name=Mika color=#ffd98c}\nBefore.\n::page\nOne.\n\n**Mika:** Two.\n\nThree.\n\n\
    Four.\n::page{off}\nAfter.\n";

/// What is drawn, as the player sees it.
fn picture(g: &Game) -> Vec<Draw> {
    g.draw_list(&|_: &str| None::<Size>)
}

/// The words drawn on the page (the lines that are not the box's own: no menu or button text).
fn page_words(g: &Game) -> Vec<String> {
    let words = ["One.", "Two.", "Three.", "Four."];
    picture(g).into_iter().filter_map(|d| if let Draw::Text { text, .. } = d { Some(text) } else { None }).filter(|t| words.iter().any(|w| t.ends_with(w))).collect()
}

fn advance(g: &mut Game, n: usize) {
    for _ in 0..n {
        key(g, Key::Space);
        g.tick(0.5, false);
    }
}

#[test]
fn advancing_through_a_page_passage_stacks_the_lines_and_the_textbox_comes_back() {
    let p = parse(BODY).unwrap();
    let mut g = instant_game(&p);
    g.tick(0.5, false);
    assert!(page_words(&g).is_empty());
    advance(&mut g, 1);
    assert_eq!(page_words(&g), ["One."]);
    advance(&mut g, 2);
    assert_eq!(page_words(&g), ["One.", "Mika: Two.", "Three."]);
    advance(&mut g, 1);
    assert_eq!(page_words(&g).len(), 4);
    advance(&mut g, 1);
    assert!(page_words(&g).is_empty(), "the textbox line took the page away");
}

#[test]
fn going_back_in_the_middle_of_a_page_shows_the_same_page_as_before() {
    let p = parse(BODY).unwrap();
    let mut g = instant_game(&p);
    g.tick(0.5, false);
    let mut seen = Vec::new();
    for _ in 0..4 {
        advance(&mut g, 1);
        seen.push(picture(&g));
    }
    for want in seen.iter().rev().skip(1) {
        key(&mut g, Key::Backspace);
        g.tick(0.5, false);
        assert_eq!(&picture(&g), want);
    }
    key(&mut g, Key::Backspace);
    g.tick(0.5, false);
    assert!(page_words(&g).is_empty(), "back at the textbox line before the page");
    advance(&mut g, 1);
    assert_eq!(picture(&g), seen[0]);
}

#[test]
fn saving_and_loading_in_the_middle_of_a_page_brings_the_page_back() {
    let p = parse(BODY).unwrap();
    let mut g = instant_game(&p);
    g.tick(0.5, false);
    advance(&mut g, 3);
    let saved = picture(&g);
    key(&mut g, Key::F5);
    advance(&mut g, 2);
    assert_ne!(picture(&g), saved);
    key(&mut g, Key::F9);
    key(&mut g, Key::Enter);
    g.tick(NOTICE_SECONDS + 0.1, false); // the "Loaded" notice goes away
    assert_eq!(picture(&g), saved);
    assert_eq!(page_words(&g), ["One.", "Mika: Two.", "Three."]);
    advance(&mut g, 1);
    assert_eq!(page_words(&g).len(), 4, "and it plays on from there");
}

#[test]
fn the_page_text_steps_back_while_choices_are_over_it() {
    let p = parse("::page\nOne.\n::choice\n- [Go](#a)\n::\n# a\nEnd.\n").unwrap();
    let mut g = instant_game(&p);
    advance(&mut g, 1); // past the line, to the choice
    let alpha = picture(&g).into_iter().find_map(|d| match d {
        Draw::Text { text, color, .. } if text == "One." => Some(color[3]),
        _ => None,
    });
    assert!(alpha.is_some_and(|a| a < 0.5), "{alpha:?}");
}
