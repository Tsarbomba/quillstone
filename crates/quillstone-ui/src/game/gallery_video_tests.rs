//! Video entries in the extras gallery: a placeholder tile, a looping full-screen viewer that
//! plays only while it is open, and arrows through pictures and clips alike.

use super::*;
use crate::extras::{cell_rect, Tab};
use quillstone_runtime::write_persistent;
use quillstone_script::{parse, Vars};

const STORY: &str = "---\ngallery: [a.png, loop.webm, b.png, cut.webm, locked.webm]\n---\n**A:** hi\n";

/// Everything but `locked.webm` unlocked, as an earlier playthrough would have left it.
fn game<'p>(p: &'p Program) -> Game<'p> {
    let mut storage = crate::MemoryStorage::default();
    let mut kept = Vars::new();
    for f in ["a.png", "loop.webm", "b.png", "cut.webm", "evil.webm"] {
        kept.insert(format!("unlocked:{f}"), Value::Bool(true));
    }
    storage.files.insert("persistent.txt".into(), write_persistent(&kept));
    let mut g = Game::new(p, Settings::default(), Box::new(storage));
    g.open_extras();
    g
}

fn key(g: &mut Game, k: Key) {
    g.handle(Event::Key(k));
}

fn click_cell(g: &mut Game, i: usize) {
    let r = cell_rect(i);
    g.handle(Event::CursorMoved(Some((r.x + r.w / 2.0, r.y + r.h / 2.0))));
    g.handle(Event::Press(Button::Left));
    g.handle(Event::Release(Button::Left));
}

fn texts(g: &Game) -> Vec<Draw> {
    g.draw_list(&|_: &str| Some(Size::new(640.0, 360.0)))
}

fn wanted(g: &Game) -> Vec<String> {
    g.wanted_videos().into_iter().map(|w| w.src).collect()
}

#[test]
fn a_video_tile_is_a_placeholder_and_nothing_is_decoded_for_the_grid() {
    let p = parse(STORY).unwrap();
    let g = game(&p);
    assert_eq!(g.extras_entries().len(), 5);
    assert!(wanted(&g).is_empty(), "no clip plays in the grid");
    let draws = texts(&g);
    let images = |name: &str| draws.iter().any(|d| matches!(d, Draw::Image { src, .. } if src == name));
    assert!(images("a.png") && images("b.png"));
    assert!(!images("loop.webm") && !images("cut.webm"), "videos are not drawn as pictures in the grid");
    let glyphs = draws.iter().filter(|d| matches!(d, Draw::Text { text, .. } if text == "\u{25B6}")).count();
    assert_eq!(glyphs, 2, "one play glyph per unlocked clip");
    let lock = draws.iter().filter(|d| matches!(d, Draw::Text { text, .. } if text == "?")).count();
    assert_eq!(lock, 1, "the locked clip looks like any locked tile");
}

#[test]
fn opening_a_clip_plays_it_looped_and_closing_stops_it() {
    let p = parse(STORY).unwrap();
    let mut g = game(&p);
    click_cell(&mut g, 1);
    assert_eq!(g.extras.viewing, Some(1));
    let w = g.wanted_videos();
    assert_eq!((w.len(), w[0].src.as_str(), w[0].looped), (1, "loop.webm", true));
    assert!(w[0].sound, "the viewer plays the clip's sound");
    assert!(g.needs_ticks(), "the player keeps drawing while a clip plays");
    assert!(texts(&g).iter().any(|d| matches!(d, Draw::Image { src, .. } if src == "loop.webm")));
    g.handle(Event::Press(Button::Left));
    assert_eq!((g.extras.viewing, wanted(&g)), (None, vec![]));
    assert!(!g.needs_ticks());
}

#[test]
fn the_arrows_walk_through_pictures_and_clips_and_skip_the_locked_one() {
    let p = parse(STORY).unwrap();
    let mut g = game(&p);
    click_cell(&mut g, 0);
    assert!(wanted(&g).is_empty(), "a picture plays nothing");
    let mut seen = Vec::new();
    for _ in 0..4 {
        key(&mut g, Key::Right);
        seen.push((g.extras.viewing.unwrap(), wanted(&g)));
    }
    let clip = |s: &str| vec![s.to_string()];
    assert_eq!(seen[0], (1, clip("loop.webm")), "picture to clip");
    assert_eq!(seen[1], (2, vec![]), "clip to picture: the clip stops");
    assert_eq!(seen[2], (3, clip("cut.webm")));
    assert_eq!(seen[3], (0, vec![]), "wraps past the locked entry");
    key(&mut g, Key::Left);
    assert_eq!(g.extras.viewing, Some(3), "and back");
    key(&mut g, Key::Escape);
    assert!(wanted(&g).is_empty() && g.screen() == Screen::Extras);
}

#[test]
fn leaving_extras_or_reloading_cleans_the_clip_up() {
    let p = parse(STORY).unwrap();
    let mut g = game(&p);
    click_cell(&mut g, 3);
    assert_eq!(wanted(&g), ["cut.webm"]);
    // a reload that drops the entry (or the whole gallery) stops it
    let shorter = parse("---\ngallery: [a.png, loop.webm]\n---\n**A:** hi\n").unwrap();
    g.reload(&shorter).unwrap();
    assert!(wanted(&g).is_empty(), "the viewed index no longer exists");
    // a reload that keeps it leaves it playing
    let mut g = game(&p);
    click_cell(&mut g, 1);
    let same = parse(STORY).unwrap();
    g.reload(&same).unwrap();
    assert_eq!(wanted(&g), ["loop.webm"]);
    // a reload that puts a picture at that index swaps the clip for the picture
    let other = parse("---\ngallery: [a.png, b.png]\n---\n**A:** hi\n").unwrap();
    g.reload(&other).unwrap();
    assert!(wanted(&g).is_empty());
    // Back out of Extras while viewing is impossible (the viewer takes every key), but the screen
    // changing for any reason ends the clip
    let mut g = game(&p);
    click_cell(&mut g, 1);
    g.screen = Screen::MainMenu;
    assert!(wanted(&g).is_empty());
}

#[test]
fn a_locked_clip_does_not_open_and_undeclared_unlocks_offer_nothing() {
    let p = parse(STORY).unwrap();
    let mut g = game(&p);
    click_cell(&mut g, 4);
    assert_eq!(g.extras.viewing, None);
    assert!(wanted(&g).is_empty());
    assert!(g.extras_entries().iter().all(|(name, _)| *name != "evil.webm"), "only the declared list is shown");
}

const THUMBS: &str = "---\ngallery: [a.png | ta.png, loop.webm | tl.png, b.png, cut.webm, locked.webm | tk.png]\n---\n**A:** hi\n";

#[test]
fn an_unlocked_tile_shows_its_own_thumbnail_and_a_locked_one_stays_a_question_mark() {
    let p = parse(THUMBS).unwrap();
    let g = game(&p);
    let draws = texts(&g);
    let images = |name: &str| draws.iter().any(|d| matches!(d, Draw::Image { src, .. } if src == name));
    assert!(images("ta.png") && images("tl.png"), "thumbnails stand in for the entry");
    assert!(!images("a.png") && !images("loop.webm"), "the entry itself isn't drawn in the grid");
    assert!(images("b.png"), "no thumbnail: a picture shows itself");
    assert!(!images("tk.png"), "a locked tile shows no thumbnail");
    let glyphs = draws.iter().filter(|d| matches!(d, Draw::Text { text, .. } if text == "\u{25B6}")).count();
    assert_eq!(glyphs, 1, "only the clip without a thumbnail keeps the play mark");
    assert_eq!(draws.iter().filter(|d| matches!(d, Draw::Text { text, .. } if text == "?")).count(), 1);
}

#[test]
fn opening_a_tile_with_a_thumbnail_views_the_entry_and_unlocking_keys_on_the_entry() {
    let p = parse(THUMBS).unwrap();
    let mut g = game(&p);
    click_cell(&mut g, 0);
    assert!(texts(&g).iter().any(|d| matches!(d, Draw::Image { src, .. } if src == "a.png")), "the full picture");
    g.handle(Event::Press(Button::Left));
    click_cell(&mut g, 4);
    assert_eq!(g.extras.viewing, None, "the locked clip stays shut (its thumbnail doesn't unlock it)");
}

#[test]
fn unlock_all_shows_every_entry_unlocked_and_writes_nothing() {
    let p = parse("---\ngallery: [a.png, b.png]\nmusic_room: [m.ogg]\nreplays: { first: \"One\" }\n---\n# first\n**A:** hi\n").unwrap();
    let storage = crate::MemoryStorage::default();
    let mut g = Game::new(&p, Settings::default(), Box::new(storage));
    g.open_extras();
    assert!(g.extras_entries().iter().all(|e| !e.1));
    g.set_unlock_all(true);
    assert!(g.extras_entries().iter().all(|e| e.1), "gallery");
    click_cell(&mut g, 1);
    assert_eq!(g.extras.viewing, Some(1), "and it opens");
    g.handle(Event::Press(Button::Left));
    g.extras.tab = Tab::Music;
    assert!(g.extras_entries().iter().all(|e| e.1), "music room");
    g.extras.tab = Tab::Scenes;
    assert!(g.extras_entries().iter().all(|e| e.1), "scenes");
    g.save_persistent();
    assert!(!g.session.unlocked("a.png"), "nothing was recorded");
    assert!(g.storage.load("persistent.txt").unwrap().is_none(), "and nothing was written");
}
