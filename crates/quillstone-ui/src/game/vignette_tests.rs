//! A vignette in the game shell: it eases in by itself and comes back with a loaded save.

use super::test_util::instant_game;
use super::*;
use quillstone_render::Size;
use quillstone_script::parse;

fn strengths(g: &Game) -> Vec<f32> {
    let any = |_: &str| Some(Size::new(100.0, 100.0));
    g.draw_list(&any)
        .into_iter()
        .filter_map(|d| match d {
            Draw::Vignette { color, .. } => Some(color[3]),
            _ => None,
        })
        .collect()
}

fn run(g: &mut Game, seconds: f32) {
    for _ in 0..(seconds / 0.05).round() as usize {
        g.tick(0.05, false);
    }
}

const STORY: &str = "One.\n::vignette{strength=0.8 fade=2}\nTwo.\n::vignette{none}\nThree.\n";

#[test]
fn it_eases_in_while_the_story_runs_and_clears() {
    let p = parse(STORY).unwrap();
    let mut g = instant_game(&p);
    assert!(strengths(&g).is_empty());
    g.handle(Event::Key(Key::Space));
    assert!(g.needs_ticks(), "the fade changes the screen by itself");
    run(&mut g, 1.0);
    let half = strengths(&g);
    assert!(half.len() == 1 && (half[0] - 0.4).abs() < 0.02, "{half:?}");
    run(&mut g, 1.5);
    assert_eq!(strengths(&g), [0.8]);
    g.handle(Event::Key(Key::Space));
    assert!(strengths(&g).is_empty());
}

#[test]
fn a_loaded_save_has_it_at_once() {
    let p = parse(STORY).unwrap();
    let mut g = instant_game(&p);
    g.handle(Event::Key(Key::Space));
    run(&mut g, 3.0);
    g.handle(Event::Key(Key::F5));
    g.handle(Event::Key(Key::Space)); // Three: cleared
    assert!(strengths(&g).is_empty());
    g.handle(Event::Key(Key::F9));
    g.handle(Event::Key(Key::Char('y'))); // confirm the load
    assert_eq!(strengths(&g), [0.8], "no fade after a load");
}
