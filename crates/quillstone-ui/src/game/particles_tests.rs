//! Ambient particles in the game shell: they keep the screen moving, freeze under menus,
//! and come back with a loaded save.

use super::test_util::{instant_game, key};
use super::*;
use quillstone_render::Size;
use quillstone_script::parse;

fn flakes(g: &Game) -> Vec<Draw> {
    let any = |_: &str| Some(Size::new(100.0, 100.0));
    g.draw_list(&any).into_iter().filter(|d| matches!(d, Draw::Rect { rect, .. } if rect.w < 10.0)).collect()
}

fn run(g: &mut Game, seconds: f32) {
    for _ in 0..(seconds / 0.05).round() as usize {
        g.tick(0.05, false);
    }
}

const STORY: &str = "One.\n::particles{snow}\nTwo.\n::particles{none fade=0}\nThree.\n";

#[test]
fn they_move_while_the_story_runs_and_freeze_under_the_settings() {
    let p = parse(STORY).unwrap();
    let mut g = instant_game(&p);
    assert!(flakes(&g).is_empty());
    key(&mut g, Key::Space);
    run(&mut g, 1.5);
    assert!(g.needs_ticks(), "the screen changes by itself");
    let (a, b) = (flakes(&g), {
        run(&mut g, 0.5);
        flakes(&g)
    });
    assert!(!a.is_empty() && a != b, "they fall");
    key(&mut g, Key::Escape); // settings
    let frozen = flakes(&g);
    run(&mut g, 2.0);
    assert_eq!(flakes(&g), frozen, "the same clock as the other animations: stopped under a menu");
    key(&mut g, Key::Escape);
    run(&mut g, 0.5);
    assert_ne!(flakes(&g), frozen);
}

#[test]
fn a_stop_ends_them_and_a_loaded_save_brings_them_back() {
    let p = parse(STORY).unwrap();
    let mut g = instant_game(&p);
    key(&mut g, Key::Space);
    run(&mut g, 2.0);
    key(&mut g, Key::F5);
    key(&mut g, Key::Space); // Three: stopped
    run(&mut g, 0.2);
    assert!(flakes(&g).is_empty());
    key(&mut g, Key::F9);
    key(&mut g, Key::Char('y')); // confirm the load
    run(&mut g, 1.5);
    assert!(!flakes(&g).is_empty() && g.needs_ticks());
}
