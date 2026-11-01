//! Frame animations and looping moves in the game shell: ticking, freezing under menus,
//! loading ahead, hot reload, and the demo.

use super::test_util::{instant_game, key};
use super::*;
use quillstone_render::Size;
use quillstone_script::parse;

fn big(_: &str) -> Option<Size> {
    Some(Size::new(500.0, 1000.0))
}

/// The picture files drawn under the textbox, back to front.
fn pics(g: &Game) -> Vec<String> {
    g.draw_list(&big)
        .iter()
        .filter_map(|d| match d {
            Draw::Image { src, .. } => Some(src.clone()),
            _ => None,
        })
        .collect()
}

/// Where the first sprite's picture is drawn (top edge), to see a loop move.
fn top(g: &Game) -> f32 {
    g.draw_list(&big)
        .iter()
        .find_map(|d| match d {
            Draw::Image { dest, .. } if dest.h < 1000.0 => Some(dest.y),
            _ => None,
        })
        .unwrap()
}

fn run(g: &mut Game, seconds: f32) {
    let steps = (seconds / 0.05).round() as usize;
    for _ in 0..steps {
        g.tick(0.05, false);
    }
}

const STORY: &str = "\
::anim{id=talk frames=\"a.png 0.2, b.png 0.2\" loop}
::anim{id=once frames=\"one.png 1, two.png 1\"}
::anim{id=still frames=\"s.png 3\" loop}
::anim{id=sky frames=\"day.png 1, night.png 1\" loop}
# start
::bg{src=plain.png}
::show{a src=still.png id=idle}
Nothing moves.
::show{mika anim=talk x=0.3}
Talking.
::hide{mika}
::show{x anim=once}
One shot.
::hide{x}
::bg{anim=sky}
Sky.
::bg{anim=still}
::move{idle y=0.8 time=1 ease=linear loop}
Bob.
::move{idle y=1 time=0.1}
Done.
";

#[test]
fn nothing_ticks_while_nothing_is_animated() {
    let p = parse(STORY).unwrap();
    let mut g = instant_game(&p);
    g.tick(0.1, false);
    assert!(!g.needs_ticks());
    assert_eq!(pics(&g), ["plain.png", "still.png"]);
}

#[test]
fn a_talking_sprite_changes_picture_and_keeps_the_frontend_ticking() {
    let p = parse(STORY).unwrap();
    let mut g = instant_game(&p);
    key(&mut g, Key::Space); // Talking
    assert!(g.needs_ticks(), "a multi-frame animation is visible");
    let mut seen = Vec::new();
    for _ in 0..10 {
        g.tick(0.1, false);
        assert!(g.needs_ticks());
        seen.push(pics(&g).last().unwrap().clone());
    }
    assert_eq!(seen, ["a.png", "b.png", "b.png", "a.png", "a.png", "b.png", "b.png", "a.png", "a.png", "b.png"]);
    let first = g.draw_list(&big);
    g.tick(0.2, false);
    assert_ne!(first, g.draw_list(&big), "the draw list differs when the frame changes");
}

#[test]
fn a_one_shot_animation_holds_and_then_stops_asking_for_ticks() {
    let p = parse(STORY).unwrap();
    let mut g = instant_game(&p);
    key(&mut g, Key::Space);
    key(&mut g, Key::Space); // One shot
    assert_eq!(pics(&g).last().unwrap(), "one.png");
    assert!(g.needs_ticks());
    run(&mut g, 0.95);
    assert_eq!(pics(&g).last().unwrap(), "one.png");
    run(&mut g, 0.1);
    assert_eq!(pics(&g).last().unwrap(), "two.png");
    run(&mut g, 1.0);
    assert!(!g.needs_ticks(), "it reached its last frame and holds it");
    assert_eq!(pics(&g).last().unwrap(), "two.png");
}

#[test]
fn a_background_animation_ticks_and_a_single_frame_one_does_not() {
    let p = parse(STORY).unwrap();
    let mut g = instant_game(&p);
    for _ in 0..3 {
        key(&mut g, Key::Space);
    }
    assert_eq!(pics(&g)[0], "day.png");
    assert!(g.needs_ticks());
    run(&mut g, 1.05);
    assert_eq!(pics(&g)[0], "night.png");
    key(&mut g, Key::Space); // Bob: a single-frame animation as the background + a loop
    assert_eq!(pics(&g)[0], "s.png");
    assert!(g.needs_ticks(), "the bob");
    key(&mut g, Key::Space); // Done: the plain move ends the loop and takes 0.1 s
    run(&mut g, 0.2);
    assert!(!g.needs_ticks(), "a still background needs no ticks");
}

#[test]
fn a_looping_move_moves_the_sprite_up_and_down_for_ever() {
    let p = parse(STORY).unwrap();
    let mut g = instant_game(&p);
    for _ in 0..4 {
        key(&mut g, Key::Space);
    }
    let rest = top(&g);
    run(&mut g, 0.5);
    let up = top(&g);
    assert!(up < rest - 1.0, "rose by half the way in half a leg: {rest} {up}");
    run(&mut g, 0.5);
    let high = top(&g);
    assert!(high < up);
    run(&mut g, 1.0);
    assert!((top(&g) - rest).abs() < 0.5, "and back");
    assert!(g.needs_ticks());
    run(&mut g, 100.5);
    assert!(g.needs_ticks());
    assert!((top(&g) - up).abs() < 0.5, "the same phase 100 s later: half a leg");
    // Another move or a hide ends it.
    key(&mut g, Key::Space);
    run(&mut g, 0.3);
    assert!((top(&g) - rest).abs() < 0.5);
    assert!(!g.needs_ticks());
}

#[test]
fn menus_freeze_animations_and_loops_like_other_timers() {
    let p = parse(STORY).unwrap();
    let mut g = instant_game(&p);
    key(&mut g, Key::Space); // talking
    run(&mut g, 0.3);
    let frozen = g.draw_list(&big);
    assert_eq!(pics(&g).last().unwrap(), "b.png");
    g.open_settings();
    assert!(!g.needs_ticks(), "nothing to tick under a menu");
    run(&mut g, 5.0);
    let under = pics(&g);
    assert_eq!(under.last().unwrap(), "b.png", "frozen under Settings");
    key(&mut g, Key::Escape);
    assert_eq!(g.draw_list(&big), frozen, "back where it was");
    run(&mut g, 0.1);
    assert_eq!(pics(&g).last().unwrap(), "a.png", "and it carries on");
    // A confirm dialog freezes it too, and so does the backlog.
    key(&mut g, Key::Char('h'));
    run(&mut g, 3.0);
    key(&mut g, Key::Escape);
    assert_eq!(pics(&g).last().unwrap(), "a.png");

    // A bobbing sprite too.
    let mut g = instant_game(&p);
    for _ in 0..4 {
        key(&mut g, Key::Space);
    }
    run(&mut g, 0.5);
    let before = top(&g);
    key(&mut g, Key::Escape);
    run(&mut g, 7.3);
    assert_eq!(top(&g), before);
    key(&mut g, Key::Escape);
    run(&mut g, 0.25);
    assert!(top(&g) < before);
}

#[test]
fn skipping_does_not_speed_an_animation_up() {
    let p = parse(STORY).unwrap();
    let mut g = Game::new(&p, Settings { text_speed: 0.0, skip_unread: true, ..Settings::default() }, Box::new(crate::MemoryStorage::default()));
    g.start_story();
    key(&mut g, Key::Space);
    key(&mut g, Key::Tab);
    for _ in 0..3 {
        g.tick(0.06, false);
        if g.view().sprites.iter().any(|s| s.src == "anim:once") {
            break;
        }
    }
    assert!(g.view().sprites.iter().any(|s| s.src == "anim:once"), "skip reached the one-shot");
    assert_eq!(pics(&g).last().unwrap(), "one.png", "settling a skipped step does not run the animation clock out");
}

#[test]
fn every_frame_of_an_animation_on_stage_is_wanted_before_it_is_needed() {
    let p = parse(STORY).unwrap();
    let mut g = instant_game(&p);
    assert!(g.upcoming_images().contains(&"a.png"), "an animation shown within a few lines is loaded ahead");
    key(&mut g, Key::Space); // Talking: on stage now, and the lines ahead never name `b.png`
    let up = g.upcoming_images();
    assert!(up.contains(&"a.png") && up.contains(&"b.png"), "{up:?}");
    key(&mut g, Key::Space);
    key(&mut g, Key::Space); // Sky: the background animation is on stage
    let up = g.upcoming_images();
    assert!(up.contains(&"day.png") && up.contains(&"night.png"), "both frames of the background that is showing: {up:?}");
}

#[test]
fn a_hot_reload_keeps_playing_from_the_first_frame_and_survives_a_removed_animation() {
    let p = parse(STORY).unwrap();
    let mut g = instant_game(&p);
    key(&mut g, Key::Space); // talking
    run(&mut g, 0.25);
    assert_eq!(pics(&g).last().unwrap(), "b.png");
    let same = parse(STORY).unwrap();
    g.reload(&same).unwrap();
    assert_eq!(pics(&g).last().unwrap(), "a.png", "a redraw restarts the clock");
    assert!(g.needs_ticks());
    // The animation is edited out (and its uses with it): the sprite is gone, the game goes on.
    let edited = parse(&STORY.replace("::anim{id=talk frames=\"a.png 0.2, b.png 0.2\" loop}\n", "").replace("::show{mika anim=talk x=0.3}", "::show{mika src=a.png x=0.3}").replace("::hide{mika}", "::hide{all}")).unwrap();
    g.reload(&edited).unwrap();
    let sprites = &g.view().sprites;
    assert!(sprites.iter().all(|s| !s.src.starts_with("anim:")), "no picture of a removed animation is left");
}

#[test]
fn a_saved_game_comes_back_playing_from_the_first_frame() {
    let p = parse(STORY).unwrap();
    let mut g = instant_game(&p);
    key(&mut g, Key::Space);
    run(&mut g, 0.3);
    key(&mut g, Key::F5);
    run(&mut g, 0.1);
    key(&mut g, Key::F9);
    key(&mut g, Key::Char('y')); // confirm the load
    assert_eq!(pics(&g).last().unwrap(), "a.png");
    assert!(g.needs_ticks());
}

#[test]
fn the_demo_plays_through_and_the_picture_changes_over_time() {
    let p = parse(include_str!("../../../../examples/anim.quill.md")).unwrap();
    let mut g = instant_game(&p);
    // Mika talks: her two poses swap every 0.2 s.
    let mut lines = 0;
    let mut talked = std::collections::BTreeSet::new();
    let mut bg_frames = std::collections::BTreeSet::new();
    let mut bobbed = std::collections::BTreeSet::new();
    while !g.view().finished && lines < 60 {
        for _ in 0..80 {
            g.tick(0.05, false);
            let list = pics(&g);
            for src in &list {
                if src.contains("mika_") {
                    talked.insert(src.clone());
                }
            }
            if list.first().is_some_and(|f| f.contains("rooftop") || f.contains("night")) {
                bg_frames.insert(list[0].clone());
            }
            if g.view().sprites.iter().any(|s| s.is_looping()) {
                bobbed.insert((top(&g) * 10.0) as i64);
            }
        }
        key(&mut g, Key::Space);
        lines += 1;
    }
    assert!(g.view().finished, "the demo ends");
    assert!(talked.contains("assets/mika_neutral.png") && talked.contains("assets/mika_happy.png"));
    assert!(bg_frames.len() == 2, "{bg_frames:?}");
    assert!(bobbed.len() >= 3, "{bobbed:?}");
}
