//! A filter in the game shell: it eases in by itself, and comes back with a loaded save without its fade.

use super::test_util::instant_game;
use super::*;
use quillstone_render::Size;
use quillstone_script::parse;

fn matrices(g: &Game) -> Vec<[[f32; 4]; 3]> {
    let any = |_: &str| Some(Size::new(100.0, 100.0));
    g.draw_list(&any).into_iter().filter_map(|d| if let Draw::Filter(Some(m)) = d { Some(m) } else { None }).collect()
}

fn run(g: &mut Game, seconds: f32) {
    for _ in 0..(seconds / 0.05).round() as usize {
        g.tick(0.05, false);
    }
}

const STORY: &str = "One.\n::filter{gray=1 fade=2}\nTwo.\n::filter{none}\nThree.\n";

#[test]
fn it_eases_in_while_the_story_runs_and_clears() {
    let p = parse(STORY).unwrap();
    let mut g = instant_game(&p);
    assert!(matrices(&g).is_empty());
    g.handle(Event::Key(Key::Space));
    assert!(g.needs_ticks(), "the fade changes the screen by itself");
    run(&mut g, 1.0);
    let half = matrices(&g);
    assert_eq!(half.len(), 1);
    // Half gray: the red row keeps half of its own red and half of the luma weight.
    assert!((half[0][0][0] - (0.5 + 0.5 * 0.2126)).abs() < 0.02, "{half:?}");
    run(&mut g, 1.5);
    assert!((matrices(&g)[0][0][0] - 0.2126).abs() < 1e-4);
    g.handle(Event::Key(Key::Space));
    assert!(matrices(&g).is_empty());
}

#[test]
fn a_loaded_save_has_it_at_once() {
    let p = parse(STORY).unwrap();
    let mut g = instant_game(&p);
    g.handle(Event::Key(Key::Space));
    run(&mut g, 3.0);
    g.handle(Event::Key(Key::F5));
    g.handle(Event::Key(Key::Space)); // Three: cleared
    assert!(matrices(&g).is_empty());
    g.handle(Event::Key(Key::F9));
    g.handle(Event::Key(Key::Char('y'))); // confirm the load
    assert_eq!(matrices(&g).len(), 1);
    assert!((matrices(&g)[0][0][0] - 0.2126).abs() < 1e-4, "no fade after a load");
}
