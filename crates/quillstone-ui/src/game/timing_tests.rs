//! `::timing` in the game shell: a press stops the marker where the clock has put it, the result goes to
//! the story, and the clock stops, restarts and is drawn like a timed choice's.

use super::test_util::{key, line};
use super::*;
use quillstone_render::Size;
use quillstone_script::parse;

/// One sweep a second, so the marker is at `t` (0..1) after `t` seconds; perfect is 48..52%, good 40..60%.
const STORY: &str = "\
---
vars: { hit: \"\", score: 0 }
---
# start
Ready?

::timing{result=hit speed=1 good=40%..60% perfect=48%..52% time=4 prompt=\"Stop it, {score}!\"}
::if{hit == \"perfect\"}
Perfect.
::elif{hit == \"good\"}
Good.
::else
Miss.
::endif
";

fn new_game<'p>(p: &'p Program, settings: Settings) -> Game<'p> {
    let mut g = Game::new(p, Settings { text_speed: 0.0, ..settings }, Box::new(crate::MemoryStorage::default()));
    g.start_story();
    g
}

fn at_bar(p: &Program) -> Game<'_> {
    let mut g = new_game(p, Settings::default());
    key(&mut g, Key::Space);
    assert!(on_bar(&g));
    g
}

fn on_bar(g: &Game) -> bool {
    matches!(g.session.state().pending, Some(Cmd::ShowTiming { .. }))
}

fn hit(g: &Game) -> Value {
    g.session.state().vars["hit"].clone()
}

fn picture(_: &str) -> Option<Size> {
    Some(Size::new(40.0, 80.0))
}

fn none(_: &str) -> Option<Size> {
    None
}

/// The x of the marker bar in a draw list (a 10 px wide rect in the marker colour, `text_color` by default).
fn marker_x(g: &Game) -> Option<f32> {
    g.draw_list(&none).iter().find_map(|d| match d {
        Draw::Rect { rect, color } if rect.w == 10.0 && *color == g.theme().text_color => Some(rect.x + 5.0),
        _ => None,
    })
}

#[test]
fn a_press_stops_the_marker_where_the_clock_has_put_it_and_sets_the_variable() {
    let p = parse(STORY).unwrap();
    // (seconds, what the player sees next): 0.5 s is mid-bar; 1.5 s is mid-bar again, on the way back
    for (t, next, word) in [(0.5, "Perfect.", "perfect"), (1.5, "Perfect.", "perfect"), (0.45, "Good.", "good"), (0.2, "Miss.", "miss"), (0.9, "Miss.", "miss")] {
        let mut g = at_bar(&p);
        g.tick(t, false);
        key(&mut g, Key::Enter);
        assert_eq!((line(&g), hit(&g)), (next.to_string(), Value::Str(word.into())), "{t} s");
    }
    // a click presses it too
    let mut g = at_bar(&p);
    g.tick(0.5, false);
    g.handle(Event::Press(Button::Left));
    assert_eq!(line(&g), "Perfect.");
}

#[test]
fn the_time_limit_is_a_miss() {
    let p = parse(STORY).unwrap();
    let mut g = at_bar(&p);
    g.tick(3.9, false);
    assert!(on_bar(&g));
    g.tick(0.2, false);
    assert_eq!((line(&g), hit(&g)), ("Miss.".to_string(), Value::Str("miss".into())));
    // no `time=`: it waits for ever
    let q = parse(&STORY.replace(" time=4", "")).unwrap();
    let mut g = at_bar(&q);
    g.tick(10_000.0, false);
    assert!(on_bar(&g));
}

#[test]
fn the_clock_waits_while_minimised_unfocused_or_a_screen_is_open() {
    let p = parse(STORY).unwrap();
    let mut g = at_bar(&p);
    g.tick(0.25, false);
    g.set_minimized(true);
    g.tick(100.0, false);
    g.set_minimized(false);
    g.set_focused(false);
    g.tick(100.0, false);
    assert!(!g.needs_ticks(), "nothing moves while it waits");
    g.set_focused(true);
    key(&mut g, Key::Escape); // settings
    g.tick(100.0, false);
    key(&mut g, Key::Escape);
    key(&mut g, Key::Char('h')); // history
    g.tick(100.0, false);
    key(&mut g, Key::Escape);
    assert!(on_bar(&g) && g.needs_ticks());
    g.tick(0.25, false); // 0.5 s of play in all
    key(&mut g, Key::Space);
    assert_eq!(hit(&g), Value::Str("perfect".into()));
}

#[test]
fn rollback_restarts_the_bar_and_roll_forward_reuses_the_result() {
    let p = parse(STORY).unwrap();
    let mut g = at_bar(&p);
    g.tick(0.5, false);
    key(&mut g, Key::Space);
    assert_eq!(line(&g), "Perfect.");
    key(&mut g, Key::Backspace);
    assert!(on_bar(&g));
    assert_eq!(hit(&g), Value::Str(String::new()));
    assert_eq!(marker_x(&g), Some(460.0), "restarted at the left end");
    g.handle(Event::Wheel(-1.0)); // roll forward: the recorded result, not a new press
    assert_eq!((line(&g), hit(&g)), ("Perfect.".to_string(), Value::Str("perfect".into())));
    key(&mut g, Key::Backspace);
    key(&mut g, Key::Space); // pressed at once: the marker is still at the start
    assert_eq!(line(&g), "Miss.");
}

#[test]
fn a_save_at_the_bar_loads_at_the_bar_with_the_marker_restarted() {
    let p = parse(STORY).unwrap();
    let mut g = at_bar(&p);
    g.tick(0.5, false);
    key(&mut g, Key::F5);
    g.tick(1.5, false);
    key(&mut g, Key::F9);
    key(&mut g, Key::Enter);
    assert!(on_bar(&g));
    assert_eq!(marker_x(&g), Some(460.0));
    g.tick(0.45, false);
    key(&mut g, Key::Space);
    assert_eq!(hit(&g), Value::Str("good".into()));
}

#[test]
fn skip_stops_at_the_bar_and_auto_forward_never_presses_it() {
    let p = parse(STORY).unwrap();
    let mut g = new_game(&p, Settings { skip_unread: true, ..Settings::default() });
    key(&mut g, Key::Tab);
    assert!(g.skipping());
    g.tick(0.1, false);
    assert!(on_bar(&g) && !g.skipping(), "skip stopped at the bar");
    key(&mut g, Key::Char('a'));
    assert!(g.auto());
    g.tick(3.0, false);
    assert!(on_bar(&g), "auto-forward leaves it to the player");
}

#[test]
fn the_bar_is_drawn_with_the_prompt_the_zones_and_the_marker_and_follows_the_theme() {
    let p = parse(STORY).unwrap();
    let mut g = at_bar(&p);
    g.tick(0.25, false);
    let list = g.draw_list(&none);
    let accent = g.theme().accent_color;
    let rects: Vec<Rect> = list.iter().filter_map(|d| match d {
        Draw::Rect { rect, .. } if rect.h == 56.0 => Some(*rect),
        _ => None,
    }).collect();
    // track, good zone (40%..60% of 1000 px), perfect zone (48%..52%)
    assert_eq!(rects.iter().map(|r| (r.x.round(), r.w.round())).collect::<Vec<_>>(), [(460.0, 1000.0), (860.0, 200.0), (940.0, 40.0)]);
    assert!(list.iter().any(|d| matches!(d, Draw::Rect { color, .. } if color[..3] == accent[..3] && color[3] < 1.0)), "good zone is a see-through accent");
    assert_eq!(marker_x(&g), Some(460.0 + 250.0), "a quarter of the way");
    assert!(list.iter().any(|d| matches!(d, Draw::Text { text, .. } if text == "Stop it, 0!")), "prompt with its value");
    let (theme, problems) = Theme::parse(
        "timing_good_color = #00ff00\ntiming_perfect_color = #ff0000\ntiming_marker_color = #0000ff\ntiming_bar_image = bar.png\n",
    );
    assert!(problems.is_empty());
    g.set_theme(theme);
    let list = g.draw_list(&picture);
    assert!(list.iter().any(|d| matches!(d, Draw::Image { src, dest, .. } if src == "bar.png" && dest.w == 1000.0)));
    assert!(list.iter().any(|d| matches!(d, Draw::Rect { color, .. } if *color == [1.0, 0.0, 0.0, 1.0])));
    assert!(list.iter().any(|d| matches!(d, Draw::Rect { rect, color } if *color == [0.0, 0.0, 1.0, 1.0] && rect.w == 10.0)));
    // a marker picture is drawn at the marker's place, keeping its proportions (40x80)
    let (theme, _) = Theme::parse("timing_marker_image = m.png\n");
    g.set_theme(theme);
    let list = g.draw_list(&picture);
    assert!(list.iter().any(|d| matches!(d, Draw::Image { src, dest, .. } if src == "m.png" && dest.w * 2.0 == dest.h && (dest.x + dest.w / 2.0 - 710.0).abs() < 0.01)));
    // while the picture is not loaded, the plain bar is drawn so it is never missing
    assert!(g.draw_list(&none).iter().all(|d| !matches!(d, Draw::Image { .. })));
}
