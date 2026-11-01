//! Timed `::hotspots{time=.. default=#label}` and hotspots that follow a sprite, in the game shell:
//! the same countdown as a timed choice, and clicks on a sprite where it is now.

use super::test_util::{key, line};
use super::*;
use quillstone_render::Size;
use quillstone_script::parse;

/// A bug that slides from the left edge to the right one (its picture is 500x1000, so 470x940 on
/// screen), and a door in the top right corner that is never under it.
const CATCH: &str = "\
---
vars: { hits: 0 }
---
# start
::bg{src=map.png}
::show{id=bug src=assets/bug.png x=left y=bottom}
::move{bug x=right time=2 ease=linear}
::hotspots{time=4 default=#slow}
- [Bug](#caught){sprite=bug, hits += 1}
- [Door](#door){rect=0.9,0,0.1,0.1}
::
# caught
Caught.
::end
# door
Door.
::end
# slow
Too slow.
::end
";

fn picture(_: &str) -> Option<Size> {
    Some(Size::new(500.0, 1000.0))
}

fn new_game<'p>(p: &'p Program) -> Game<'p> {
    let settings = Settings { text_speed: 0.0, ..Settings::default() };
    let mut g = Game::new(p, settings, Box::new(crate::MemoryStorage::default()));
    g.start_story();
    g.draw_list(&picture); // a frame notes the picture sizes, as the player always draws before the next click
    g
}

/// Tick in small steps like frames, drawing each time.
fn run(g: &mut Game, seconds: f32) {
    for _ in 0..(seconds * 10.0).round() as usize {
        g.tick(0.1, false);
        g.draw_list(&picture);
    }
}

fn click(g: &mut Game, at: (f32, f32)) {
    g.handle(Event::CursorMoved(Some(at)));
    g.handle(Event::Press(Button::Left));
    g.handle(Event::Release(Button::Left));
}

fn bug_centre(g: &Game) -> (f32, f32) {
    let r = g.view().spot_rect(&g.view().hotspots[0]).expect("the bug is shown");
    (r.x + r.w / 2.0, r.y + r.h / 2.0)
}

/// The width of the countdown bar's fill, as in `timed_tests`.
fn fill(g: &Game) -> Option<f32> {
    let h = g.theme().choice_timer_height;
    g.draw_list(&picture).iter().find_map(|d| match d {
        Draw::Rect { rect, color } if rect.h == h && *color == g.theme().accent_color => Some(rect.w),
        _ => None,
    })
}

#[test]
fn time_running_out_takes_the_default_jump_and_the_bar_runs_along_the_top() {
    let p = parse(CATCH).unwrap();
    let mut g = new_game(&p);
    assert_eq!(g.view().hotspots.len(), 3, "bug, door and the invisible timeout");
    assert_eq!(fill(&g), Some(1000.0), "full on arrival");
    let bar = g.draw_list(&picture).iter().find_map(|d| match d {
        Draw::Rect { rect, color } if *color == g.theme().accent_color && rect.h == g.theme().choice_timer_height => Some(*rect),
        _ => None,
    });
    assert!(bar.unwrap().y < 100.0, "no buttons, so the bar is at the top: {bar:?}");
    run(&mut g, 1.0);
    assert_eq!(fill(&g), Some(750.0));
    assert!(g.needs_ticks());
    run(&mut g, 2.8);
    assert_eq!(g.view().hotspots.len(), 3, "not yet");
    run(&mut g, 0.3);
    assert!(g.view().hotspots.is_empty());
    assert_eq!(line(&g), "Too slow.");
    assert_eq!(g.state().vars.get("hits"), Some(&Value::Int(0)), "no region was clicked, so no effects ran");
    assert_eq!(fill(&g), None);
}

#[test]
fn a_click_on_the_moving_sprite_hits_where_it_is_now_and_misses_where_it_was() {
    let p = parse(CATCH).unwrap();
    let mut g = new_game(&p);
    let start = bug_centre(&g);
    assert!(start.0 < 300.0, "it starts at the left edge: {start:?}");
    run(&mut g, 1.0);
    let now = bug_centre(&g);
    assert!(now.0 > 800.0, "half way across: {now:?}");
    g.handle(Event::CursorMoved(Some(start)));
    g.handle(Event::Press(Button::Left));
    g.handle(Event::Release(Button::Left));
    assert_eq!(g.view().hotspots.len(), 3, "the old place is empty now");
    click(&mut g, now);
    assert_eq!(line(&g), "Caught.");
    assert_eq!(g.state().vars.get("hits"), Some(&Value::Int(1)));
    g.tick(60.0, false);
    assert_eq!(line(&g), "Caught.", "a pick leaves no timer behind");
}

#[test]
fn the_highlight_follows_the_sprite() {
    let p = parse(CATCH).unwrap();
    let mut g = new_game(&p);
    run(&mut g, 1.0);
    let at = bug_centre(&g);
    g.handle(Event::CursorMoved(Some(at)));
    let rect = g.view().spot_rect(&g.view().hotspots[0]).unwrap();
    let tinted = g.draw_list(&picture).iter().any(|d| matches!(d, Draw::Rect { rect: r, color } if *r == rect && color[3] < 0.5));
    assert!(tinted, "the hover tint is on the sprite's rect {rect:?}");
}

#[test]
fn a_hotspot_whose_sprite_is_not_shown_is_not_clickable_or_focused() {
    let hidden = CATCH.replace("::move{bug x=right time=2 ease=linear}\n", "::hide{bug}\n");
    let p = parse(&hidden).unwrap();
    let mut g = new_game(&p);
    assert!(g.view().sprites.is_empty());
    assert_eq!(g.view().spot_rect(&g.view().hotspots[0]), None);
    click(&mut g, (235.0, 700.0));
    assert_eq!(g.view().hotspots.len(), 3, "nothing under the missing sprite");
    key(&mut g, Key::Right);
    key(&mut g, Key::Enter);
    assert_eq!(line(&g), "Door.", "the arrows skip the hidden sprite's region and reach the door");
}

#[test]
fn the_arrows_cycle_through_the_moving_region_too() {
    let p = parse(CATCH).unwrap();
    let mut g = new_game(&p);
    key(&mut g, Key::Right);
    key(&mut g, Key::Enter);
    assert_eq!(line(&g), "Caught.", "the bug is the first region");
    let mut g = new_game(&p);
    key(&mut g, Key::Right);
    key(&mut g, Key::Right);
    key(&mut g, Key::Enter);
    assert_eq!(line(&g), "Door.");
    let mut g = new_game(&p);
    key(&mut g, Key::Left); // from nothing, backwards: the last visible one (the timeout is hidden)
    key(&mut g, Key::Enter);
    assert_eq!(line(&g), "Door.");
}

#[test]
fn a_minimised_window_and_an_unfocused_one_pause_the_countdown() {
    let p = parse(CATCH).unwrap();
    let mut g = new_game(&p);
    run(&mut g, 1.0);
    g.set_minimized(true);
    g.tick(100.0, false);
    assert_eq!(fill(&g), Some(750.0), "no time passed, and the default was not taken");
    g.set_minimized(false);
    g.set_focused(false);
    g.tick(100.0, false);
    assert_eq!(fill(&g), Some(750.0));
    g.set_focused(true);
    run(&mut g, 1.0);
    assert!((fill(&g).unwrap() - 500.0).abs() < 1.0, "it goes on from where it was: {:?}", fill(&g));
}

#[test]
fn rollback_and_load_start_the_countdown_afresh() {
    let p = parse(CATCH).unwrap();
    let mut g = new_game(&p);
    run(&mut g, 3.0);
    key(&mut g, Key::F5);
    let at = bug_centre(&g);
    click(&mut g, at);
    assert_eq!(line(&g), "Caught.");
    key(&mut g, Key::Backspace);
    assert_eq!(g.view().hotspots.len(), 3, "back at the screen");
    assert_eq!(fill(&g), Some(1000.0), "full again");
    run(&mut g, 2.0);
    key(&mut g, Key::F9); // the quick save made at 3 s: the clock still starts full
    key(&mut g, Key::Enter);
    assert_eq!(g.view().hotspots.len(), 3);
    assert_eq!(fill(&g), Some(1000.0));
    run(&mut g, 4.1);
    assert_eq!(line(&g), "Too slow.");
}

#[test]
fn auto_mode_never_takes_the_timeout_and_an_untimed_screen_draws_no_bar() {
    let p = parse(CATCH).unwrap();
    let mut g = new_game(&p);
    key(&mut g, Key::Char('a'));
    run(&mut g, 3.0);
    assert_eq!(g.view().hotspots.len(), 3, "auto doesn't pick");
    let plain = parse("# a\n::hotspots\n- [x](#b){rect=0,0,0.5,0.5}\n::\n# b\nDone.\n").unwrap();
    let mut g = new_game(&plain);
    assert_eq!(fill(&g), None);
    assert!(!g.needs_ticks());
    g.tick(10_000.0, false);
    assert_eq!(g.view().hotspots.len(), 1);
}
