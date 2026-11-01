//! A tint in the game shell: it eases in by itself, stays under menus' clock rules, and comes back with a loaded save.

use super::test_util::{instant_game};
use super::*;
use quillstone_render::Size;
use quillstone_script::parse;

fn washes(g: &Game) -> Vec<[f32; 4]> {
    let any = |_: &str| Some(Size::new(100.0, 100.0));
    g.draw_list(&any)
        .into_iter()
        .filter_map(|d| match d {
            Draw::Rect { rect, color } if rect.w >= 1920.0 && rect.h >= 1080.0 && color[3] < 1.0 => Some(color),
            _ => None,
        })
        .collect()
}

fn run(g: &mut Game, seconds: f32) {
    for _ in 0..(seconds / 0.05).round() as usize {
        g.tick(0.05, false);
    }
}

const STORY: &str = "One.\n::tint{color=#0000ff alpha=0.5 fade=2}\nTwo.\n::tint{none}\nThree.\n";

#[test]
fn it_eases_in_while_the_story_runs_and_clears() {
    let p = parse(STORY).unwrap();
    let mut g = instant_game(&p);
    assert!(washes(&g).is_empty());
    g.handle(Event::Key(Key::Space));
    assert!(g.needs_ticks(), "the fade changes the screen by itself");
    run(&mut g, 1.0);
    let half = washes(&g);
    assert_eq!(half.len(), 1);
    assert!((half[0][3] - 0.25).abs() < 0.02, "{half:?}");
    run(&mut g, 1.5);
    assert_eq!(washes(&g), [[0.0, 0.0, 1.0, 0.5]]);
    g.handle(Event::Key(Key::Space));
    assert!(washes(&g).is_empty());
}

#[test]
fn a_loaded_save_has_it_at_once() {
    let p = parse(STORY).unwrap();
    let mut g = instant_game(&p);
    g.handle(Event::Key(Key::Space));
    run(&mut g, 3.0);
    g.handle(Event::Key(Key::F5));
    g.handle(Event::Key(Key::Space)); // Three: cleared
    assert!(washes(&g).is_empty());
    g.handle(Event::Key(Key::F9));
    g.handle(Event::Key(Key::Char('y'))); // confirm the load
    assert_eq!(washes(&g), [[0.0, 0.0, 1.0, 0.5]], "no fade after a load");
}
