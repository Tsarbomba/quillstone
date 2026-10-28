//! Layered sprites in the game shell: what is drawn, what is loaded ahead, saving and loading, a
//! hot reload and the demo.

use super::*;
use quillstone_render::Size;
use quillstone_script::parse;

const STORY: &str = "\
::anim{id=blink frames=\"e/open.png 3, e/closed.png 0.1\" loop}
::character{id=mika layers=body,eyes,mouth layer_sprites=a/{layer}_{value}.png}
::show{mika body=casual eyes=anim:blink mouth=smile}
One.
::show{mika mouth=open}
Two.
::show{mika body=formal eyes=none}
Three.
";

fn new_game<'p>(p: &'p Program) -> Game<'p> {
    let mut g = Game::new(p, Settings { text_speed: 0.0, ..Settings::default() }, Box::new(crate::MemoryStorage::default()));
    g.start_story();
    g
}

fn big(_: &str) -> Option<Size> {
    Some(Size::new(280.0, 500.0))
}

/// The picture files drawn, back to front, and every name the draw list asked the size of.
fn pics(g: &Game) -> Vec<String> {
    g.draw_list(&big).iter().filter_map(|d| if let Draw::Image { src, .. } = d { Some(src.clone()) } else { None }).collect()
}

fn asked(g: &Game) -> Vec<String> {
    let asked = std::cell::RefCell::new(Vec::new());
    g.draw_list(&|s: &str| {
        asked.borrow_mut().push(s.to_string());
        big(s)
    });
    asked.into_inner()
}

fn run(g: &mut Game, seconds: f32) {
    for _ in 0..(seconds / 0.05).round() as usize {
        g.tick(0.05, false);
    }
}

#[test]
fn the_layers_are_drawn_and_every_file_is_asked_for_by_its_own_name() {
    let p = parse(STORY).unwrap();
    let mut g = new_game(&p);
    assert_eq!(pics(&g), ["a/body_casual.png", "e/open.png", "a/mouth_smile.png"]);
    assert!(asked(&g).iter().all(|s| !s.is_empty()), "the player would try to load an empty name: {:?}", asked(&g));
    run(&mut g, 3.05);
    assert_eq!(pics(&g)[1], "e/closed.png", "the blink plays by itself");
    assert!(g.needs_ticks(), "an animated layer needs frames");
}

#[test]
fn the_files_of_the_layers_ahead_and_every_blink_frame_are_loaded_before_they_are_needed() {
    let p = parse(STORY).unwrap();
    let mut g = new_game(&p);
    let up = g.upcoming_images();
    for f in ["a/body_formal.png", "a/mouth_open.png", "e/open.png", "e/closed.png"] {
        assert!(up.contains(&f), "{f} in {up:?}");
    }
    g.handle(Event::Key(Key::Space));
    g.handle(Event::Key(Key::Space));
    // eyes=none: the blink is gone from the stage, so its frames need not be kept ready
    assert_eq!(pics(&g), ["a/body_formal.png", "a/mouth_open.png"]);
    assert!(g.view().shown_animations().is_empty());
}

fn key(g: &mut Game, k: Key) {
    g.handle(Event::Key(k));
}

#[test]
fn a_saved_and_loaded_game_shows_the_same_layers() {
    let p = parse(STORY).unwrap();
    let mut g = new_game(&p);
    key(&mut g, Key::Space);
    let before = pics(&g);
    assert_eq!(before, ["a/body_casual.png", "e/open.png", "a/mouth_open.png"]);
    key(&mut g, Key::F5);
    key(&mut g, Key::Space);
    key(&mut g, Key::Space);
    assert_ne!(pics(&g), before);
    key(&mut g, Key::F9);
    key(&mut g, Key::Char('y')); // confirm the load
    assert_eq!(pics(&g), before);
}

#[test]
fn a_hot_reload_keeps_the_layers_and_drops_what_the_edit_removed() {
    let p = parse(STORY).unwrap();
    let mut g = new_game(&p);
    key(&mut g, Key::Space);
    let same = parse(STORY).unwrap();
    g.reload(&same).unwrap();
    assert_eq!(pics(&g), ["a/body_casual.png", "e/open.png", "a/mouth_open.png"]);
    // the blink is edited away (its uses become a plain file): the eyes layer goes, the rest stays
    let edited = parse(&STORY.replace("::anim{id=blink frames=\"e/open.png 3, e/closed.png 0.1\" loop}\n", "").replace("anim:blink", "x")).unwrap();
    g.reload(&edited).unwrap();
    assert_eq!(pics(&g), ["a/body_casual.png", "a/mouth_open.png"]);
}

#[test]
fn the_layers_demo_plays_and_never_asks_for_a_pattern_or_an_empty_name() {
    let p = parse(include_str!("../../../../examples/layers.quill.md")).unwrap();
    let mut g = new_game(&p);
    for _ in 0..30 {
        let asked = asked(&g);
        assert!(asked.iter().all(|s| !s.is_empty() && !s.contains('{') && !s.starts_with("anim:")), "{asked:?}");
        key(&mut g, Key::Space);
        key(&mut g, Key::Space);
        run(&mut g, 0.5);
    }
}
