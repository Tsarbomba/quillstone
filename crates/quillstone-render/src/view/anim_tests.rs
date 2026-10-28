//! Frame animations and looping moves in the view: which picture is drawn when, and
//! where a looping sprite is.

use super::*;
use quillstone_script::parse;

fn anims() -> BTreeMap<String, Anim> {
    parse(
        "::anim{id=talk frames=\"a.png 0.2, b.png 0.2\" loop}\n\
         ::anim{id=once frames=\"one.png 1, two.png 0.5, three.png 2\"}\n\
         ::anim{id=still frames=\"s.png 1\" loop}\n\
         ::anim{id=bgloop frames=\"day.png 1.5, night.png 1.5\" loop}\n",
    )
    .unwrap()
    .anims
}

fn view() -> View {
    View::new().with_speed(0.0).with_anims(&anims())
}

fn big(_: &str) -> Option<Size> {
    Some(Size::new(500.0, 1000.0))
}

fn show(id: &str, src: &str, x: f32, y: f32, scale: f32, fade: f32) -> Cmd {
    Cmd::ShowSprite { id: id.into(), src: src.into(), layers: vec![], x, y, z: 0, scale, flip: false, xscale: 1.0, yscale: 1.0, fade }
}

fn bg(src: &str, fade: f32) -> Cmd {
    Cmd::SetBackground { src: src.into(), fade, wipe: None }
}

fn bob(id: &str, y: f32, time: f32, ease: Ease) -> Cmd {
    Cmd::LoopSprite { id: id.into(), x: 0.5, y, scale: 1.0, flip: false, xscale: 1.0, yscale: 1.0, time, ease }
}

fn plain_move(id: &str, x: f32, y: f32, time: f32) -> Cmd {
    Cmd::MoveSprite { id: id.into(), x, y, scale: 1.0, flip: false, xscale: 1.0, yscale: 1.0, time, ease: Ease::Linear }
}

/// The picture files drawn, back to front.
fn files(v: &View) -> Vec<String> {
    v.draw_list(&big, None)
        .iter()
        .filter_map(|d| if let Draw::Image { src, .. } = d { Some(src.clone()) } else { None })
        .collect()
}

fn close(a: f32, b: f32) -> bool {
    (a - b).abs() < 1e-4
}

#[test]
fn a_sprite_that_is_an_animation_draws_the_frame_its_clock_is_at() {
    let mut v = view();
    v.apply(&[show("m", "anim:talk", 0.5, 1.0, 1.0, 0.0)]);
    assert_eq!(files(&v), ["a.png"], "it starts at the first frame");
    let mut seen = vec![];
    for _ in 0..12 {
        v.tick(0.1);
        seen.push(files(&v)[0].clone());
    }
    // 0.1, 0.2, ...: a frame's last instant belongs to the next one.
    assert_eq!(seen, ["a.png", "b.png", "b.png", "a.png", "a.png", "b.png", "b.png", "a.png", "a.png", "b.png", "b.png", "a.png"]);
}

#[test]
fn frames_change_exactly_at_their_boundaries_whatever_the_tick_size() {
    let mut v = view();
    v.apply(&[show("m", "anim:once", 0.5, 1.0, 1.0, 0.0)]);
    v.tick(0.999);
    assert_eq!(files(&v), ["one.png"]);
    v.tick(0.001_5);
    assert_eq!(files(&v), ["two.png"]);
    v.tick(0.49);
    assert_eq!(files(&v), ["two.png"]);
    v.tick(0.02);
    assert_eq!(files(&v), ["three.png"]);
    // A huge step lands on the same frame as many small ones.
    let mut w = view();
    w.apply(&[show("m", "anim:talk", 0.5, 1.0, 1.0, 0.0)]);
    w.tick(1000.05);
    assert_eq!(files(&w), ["a.png"]);
    w.tick(0.2);
    assert_eq!(files(&w), ["b.png"]);
}

#[test]
fn a_one_shot_animation_holds_its_last_frame_and_a_loop_goes_round() {
    let mut v = view();
    v.apply(&[show("o", "anim:once", 0.3, 1.0, 1.0, 0.0), show("t", "anim:talk", 0.7, 1.0, 1.0, 0.0)]);
    assert!(v.animating());
    v.tick(3.49);
    assert_eq!(files(&v)[0], "three.png");
    v.tick(100.0);
    assert_eq!(files(&v)[0], "three.png", "held");
    assert!(v.animating(), "the loop still runs");
    v.apply(&[hide("t")]);
    assert!(!v.animating(), "a finished one-shot has nothing left to change");
    assert!(!v.busy());
}

fn hide(id: &str) -> Cmd {
    Cmd::HideSprite { id: id.into(), fade: 0.0 }
}

#[test]
fn a_still_animation_is_just_a_picture_and_needs_no_ticks() {
    let mut v = view();
    v.apply(&[show("m", "anim:still", 0.5, 1.0, 1.0, 0.0), bg("anim:still", 0.0)]);
    assert_eq!(files(&v), ["s.png", "s.png"]);
    assert!(!v.animating() && !v.busy());
    v.tick(50.0);
    assert_eq!(files(&v), ["s.png", "s.png"]);
}

#[test]
fn an_unknown_animation_draws_nothing() {
    let mut v = view();
    v.apply(&[show("m", "anim:gone", 0.5, 1.0, 1.0, 0.0), bg("anim:gone", 0.0)]);
    assert!(files(&v).is_empty() && !v.animating());
    let mut bare = View::new();
    bare.apply(&[show("m", "anim:talk", 0.5, 1.0, 1.0, 0.0)]);
    assert!(files(&bare).is_empty(), "a view that was never told about animations");
}

#[test]
fn a_background_animation_plays_with_fades_and_wipes() {
    let mut v = view();
    v.apply(&[bg("anim:bgloop", 0.0)]);
    assert_eq!(files(&v), ["day.png"]);
    v.tick(1.5);
    assert_eq!(files(&v), ["night.png"]);
    v.tick(1.5);
    assert_eq!(files(&v), ["day.png"], "it loops");
    assert!(v.animating());
    // A wipe reveals the animated picture, which keeps playing underneath it.
    let mut w = view();
    w.apply(&[bg("plain.png", 0.0)]);
    w.apply(&[Cmd::SetBackground { src: "anim:bgloop".into(), fade: 2.0, wipe: Some(quillstone_script::Wipe::Right) }]);
    w.tick(1.0);
    let list = files(&w);
    assert_eq!(list, ["plain.png", "day.png"]);
    w.tick(0.6);
    assert_eq!(files(&w), ["plain.png", "night.png"], "the clock runs during the wipe");
    // A crossfade from one animation to another keeps the old one playing.
    let mut c = view();
    c.apply(&[bg("anim:bgloop", 0.0)]);
    c.tick(1.0);
    c.apply(&[bg("anim:once", 2.0)]);
    assert_eq!(files(&c), ["day.png", "one.png"]);
    c.tick(0.6);
    assert_eq!(files(&c), ["night.png", "one.png"], "outgoing at 1.6 s, incoming at 0.6 s");
    c.tick(2.0);
    assert_eq!(files(&c), ["three.png"]);
}

#[test]
fn the_clock_starts_when_the_animation_appears_and_restarts_after_a_redraw() {
    let mut v = view();
    v.apply(&[show("m", "anim:talk", 0.5, 1.0, 1.0, 0.0)]);
    v.tick(0.3);
    assert_eq!(files(&v), ["b.png"]);
    // The same animation shown again (a move of its values, say) keeps its clock.
    v.apply(&[show("m", "anim:talk", 0.6, 1.0, 1.0, 0.0)]);
    assert_eq!(files(&v), ["b.png"]);
    // Another picture and back starts it over.
    v.apply(&[show("m", "x.png", 0.6, 1.0, 1.0, 0.0)]);
    v.apply(&[show("m", "anim:talk", 0.6, 1.0, 1.0, 0.0)]);
    assert_eq!(files(&v), ["a.png"]);
    // A redraw (rollback, load, reload) is a fresh start for everything.
    v.tick(0.3);
    v.apply(&[Cmd::ClearScene, show("m", "anim:talk", 0.6, 1.0, 1.0, 0.0), bg("anim:bgloop", 0.0)]);
    assert_eq!(files(&v), ["day.png", "a.png"]);
    v.tick(1.6);
    v.apply(&[Cmd::ClearScene, bg("anim:bgloop", 0.0)]);
    assert_eq!(files(&v), ["day.png"]);
}

#[test]
fn a_pose_change_crossfade_keeps_the_old_animation_playing() {
    let mut v = view();
    v.apply(&[show("m", "anim:talk", 0.5, 1.0, 1.0, 0.0)]);
    v.tick(0.3);
    v.apply(&[show("m", "still.png", 0.5, 1.0, 1.0, 1.0)]);
    assert_eq!(files(&v), ["b.png", "still.png"]);
    v.tick(0.2);
    assert_eq!(files(&v), ["a.png", "still.png"], "the outgoing animation moved on from 0.3 s to 0.5 s");
    v.tick(1.0);
    assert_eq!(files(&v), ["still.png"]);
    // And the other way round: a still pose changes into an animation, which starts at its first frame.
    v.apply(&[show("m", "anim:once", 0.5, 1.0, 1.0, 1.0)]);
    assert_eq!(files(&v), ["still.png", "one.png"]);
}

#[test]
fn an_animated_sprite_moves_flips_and_scales_like_any_other() {
    let mut v = view();
    v.apply(&[show("m", "anim:talk", 0.25, 1.0, 0.5, 0.0), plain_move("m", 0.75, 1.0, 1.0)]);
    v.tick(0.5);
    let list = v.draw_list(&big, None);
    let Draw::Image { src, dest, .. } = &list[0] else { panic!() };
    assert_eq!(src, "a.png", "0.5 s into a 0.2 s + 0.2 s loop is 0.1 s into the second lap");
    assert!(close(dest.x + dest.w / 2.0, 0.5 * VIRTUAL.w), "half way along: {dest:?}");
    assert!(close(dest.h, SPRITE_HEIGHT * 0.75), "the scale moved from 0.5 to 1 as well");
}

#[test]
fn thumbnails_and_menus_see_the_first_frame() {
    // A scene built for a save slot is never ticked: frame 0.
    let mut v = view();
    v.apply(&[Cmd::ClearScene, bg("anim:bgloop", 0.0), show("m", "anim:talk", 0.5, 1.0, 1.0, 0.0)]);
    assert_eq!(files(&v), ["day.png", "a.png"]);
    assert_eq!(v.draw_scene(&big).len(), 2);
    assert_eq!(v.shown_animations(), ["bgloop", "talk"]);
    v.apply(&[show("m", "q.png", 0.5, 1.0, 1.0, 0.0)]);
    assert_eq!(v.shown_animations(), ["bgloop"]);
}

#[test]
fn settling_does_not_run_animation_clocks() {
    let mut v = view();
    v.apply(&[show("m", "anim:talk", 0.5, 1.0, 1.0, 0.0), bg("anim:bgloop", 0.0), bob("m", 0.9, 1.0, Ease::Linear)]);
    v.tick(0.25);
    let before = files(&v);
    let y = v.sprites[0].current_y();
    v.settle(60.0);
    assert_eq!(files(&v), before, "settling is not time passing for pictures");
    assert!(close(v.sprites[0].current_y(), y), "nor for loops");
}

// ---- looping moves ----------------------------------------------------------------------------

fn y_after(v: &mut View, t: f32) -> f32 {
    v.tick(t);
    v.sprites[0].current_y()
}

#[test]
fn a_loop_goes_to_the_target_back_to_the_start_and_forth_again() {
    let mut v = view();
    v.apply(&[show("m", "x.png", 0.5, 1.0, 1.0, 0.0), bob("m", 0.8, 1.0, Ease::Linear)]);
    assert!(v.animating() && v.busy() && v.sprites[0].is_looping());
    assert!(close(v.sprites[0].current_y(), 1.0), "phase 0 is the starting value");
    let mut got = vec![];
    for _ in 0..12 {
        got.push(y_after(&mut v, 0.25));
    }
    // one leg is 1 s: 1.0 -> 0.8 -> 1.0 -> 0.8 -> 1.0
    let want = [0.95, 0.9, 0.85, 0.8, 0.85, 0.9, 0.95, 1.0, 0.95, 0.9, 0.85, 0.8];
    for (g, w) in got.iter().zip(want) {
        assert!(close(*g, w), "{got:?}");
    }
}

#[test]
fn the_leg_boundaries_are_exact_for_one_two_and_many_legs() {
    for (t, want) in [(0.0, 1.0), (1.0, 0.8), (2.0, 1.0), (3.0, 0.8), (4.0, 1.0), (100.0, 1.0), (101.0, 0.8), (100.5, 0.9)] {
        let mut v = view();
        v.apply(&[show("m", "x.png", 0.5, 1.0, 1.0, 0.0), bob("m", 0.8, 1.0, Ease::Linear)]);
        // One big step and many small ones agree.
        v.tick(t);
        let mut w = view();
        w.apply(&[show("m", "x.png", 0.5, 1.0, 1.0, 0.0), bob("m", 0.8, 1.0, Ease::Linear)]);
        for _ in 0..(t * 8.0) as usize {
            w.tick(0.125);
        }
        assert!(close(v.sprites[0].current_y(), want), "t={t}: {}", v.sprites[0].current_y());
        assert!(close(w.sprites[0].current_y(), want), "t={t} in steps: {}", w.sprites[0].current_y());
    }
}

#[test]
fn easing_applies_to_each_leg() {
    let mut v = view();
    v.apply(&[show("m", "x.png", 0.5, 1.0, 1.0, 0.0), bob("m", 0.0, 1.0, Ease::In)]);
    assert!(close(y_after(&mut v, 0.5), 1.0 - Ease::In.apply(0.5)), "out leg, in-ease: slow start");
    assert!(close(y_after(&mut v, 1.0), Ease::In.apply(0.5)), "the back leg eases in as well (1.5 s): slow away from the far end");
    let mut w = view();
    w.apply(&[show("m", "x.png", 0.5, 1.0, 1.0, 0.0), bob("m", 0.0, 1.0, Ease::InOut)]);
    assert!(close(y_after(&mut w, 0.5), 0.5));
    assert!(close(y_after(&mut w, 0.25), 1.0 - Ease::InOut.apply(0.75)));
    assert!(close(y_after(&mut w, 0.5), Ease::InOut.apply(0.25)), "1.25 s: a quarter into the back leg");
}

#[test]
fn a_loop_animates_every_value_together() {
    let mut v = view();
    v.apply(&[
        show("m", "x.png", 0.25, 1.0, 1.0, 0.0),
        Cmd::LoopSprite { id: "m".into(), x: 0.75, y: 0.9, scale: 1.2, flip: false, xscale: 2.0, yscale: 0.5, time: 2.0, ease: Ease::Linear },
    ]);
    v.tick(1.0);
    let s = &v.sprites[0];
    assert!(close(s.current_x(), 0.5) && close(s.current_y(), 0.95) && close(s.current_scale(), 1.1));
    assert!(close(s.current_xscale(), 1.5) && close(s.current_yscale(), 0.75));
    v.tick(1.0);
    let s = &v.sprites[0];
    assert!(close(s.current_x(), 0.75) && close(s.current_xscale(), 2.0), "after leg 1");
    v.tick(2.0);
    assert!(close(v.sprites[0].current_x(), 0.25) && close(v.sprites[0].current_yscale(), 1.0), "back at the start");
}

#[test]
fn a_loop_starts_from_where_the_sprite_is_and_returns_to_its_resting_values() {
    let mut v = view();
    v.apply(&[show("m", "x.png", 0.5, 1.0, 1.0, 0.0), plain_move("m", 0.5, 0.5, 1.0)]);
    v.tick(0.5); // y is at 0.75, heading for 0.5
    v.apply(&[bob("m", 0.9, 1.0, Ease::Linear)]);
    // The runtime's state says the sprite rests at y 0.5 (the last move); the first leg starts at 0.75.
    assert!(close(v.sprites[0].current_y(), 0.75));
    v.tick(0.5);
    assert!(close(v.sprites[0].current_y(), 0.825));
    v.tick(0.5);
    assert!(close(v.sprites[0].current_y(), 0.9));
    v.tick(1.0);
    assert!(close(v.sprites[0].current_y(), 0.5), "back to rest");
}

#[test]
fn a_plain_move_ends_the_loop_and_glides_from_where_the_sprite_is() {
    let mut v = view();
    v.apply(&[show("m", "x.png", 0.5, 1.0, 1.0, 0.0), bob("m", 0.8, 1.0, Ease::Linear)]);
    v.tick(0.5);
    assert!(close(v.sprites[0].current_y(), 0.9));
    v.apply(&[plain_move("m", 0.5, 1.0, 1.0)]);
    assert!(!v.sprites[0].is_looping() && !v.animating() || v.busy());
    assert!(close(v.sprites[0].current_y(), 0.9), "no jump");
    v.tick(0.5);
    assert!(close(v.sprites[0].current_y(), 0.95), "half way from 0.9 to 1.0");
    v.tick(5.0);
    assert!(close(v.sprites[0].current_y(), 1.0));
    assert!(!v.animating(), "nothing moves any more");
    // An instant move ends it too.
    let mut w = view();
    w.apply(&[show("m", "x.png", 0.5, 1.0, 1.0, 0.0), bob("m", 0.8, 1.0, Ease::Linear)]);
    w.tick(0.25);
    w.apply(&[plain_move("m", 0.5, 0.7, 0.0)]);
    assert!(close(w.sprites[0].current_y(), 0.7) && !w.animating());
}

#[test]
fn a_new_pose_keeps_a_loop_going_but_new_values_end_it() {
    let mut v = view();
    v.apply(&[show("m", "x.png", 0.5, 1.0, 1.0, 0.0), bob("m", 0.8, 1.0, Ease::Linear)]);
    v.tick(0.5);
    // The runtime shows the new pose at the resting values.
    v.apply(&[show("m", "y.png", 0.5, 1.0, 1.0, 0.0)]);
    assert!(v.sprites[0].is_looping() && close(v.sprites[0].current_y(), 0.9), "the phase went on");
    v.apply(&[show("m", "anim:talk", 0.5, 1.0, 1.0, 0.5)]);
    v.tick(0.25);
    assert!(close(v.sprites[0].current_y(), 0.85), "still bobbing through a fading pose change");
    // Other resting values: the loop is over, the sprite snaps (a show never glides).
    v.apply(&[show("m", "anim:talk", 0.3, 1.0, 1.0, 0.0)]);
    assert!(!v.sprites[0].is_looping() && close(v.sprites[0].current_y(), 1.0) && close(v.sprites[0].current_x(), 0.3));
    v.tick(5.0);
    assert!(close(v.sprites[0].current_y(), 1.0));
}

#[test]
fn hiding_and_clearing_end_a_loop() {
    let mut v = view();
    v.apply(&[show("m", "x.png", 0.5, 1.0, 1.0, 0.0), bob("m", 0.8, 1.0, Ease::Linear), hide("m")]);
    assert!(v.sprites.is_empty() && !v.animating());
    v.apply(&[show("m", "x.png", 0.5, 1.0, 1.0, 0.0), bob("m", 0.8, 1.0, Ease::Linear)]);
    v.apply(&[Cmd::ClearScene]);
    assert!(v.sprites.is_empty() && !v.animating());
    // A fading-out sprite keeps its place on screen while it fades.
    v.apply(&[show("m", "x.png", 0.5, 1.0, 1.0, 0.0), bob("m", 0.8, 1.0, Ease::Linear)]);
    v.apply(&[Cmd::HideSprite { id: "m".into(), fade: 1.0 }]);
    v.tick(1.5);
    assert!(v.sprites.is_empty());
}

#[test]
fn a_redraw_restarts_the_phase_at_the_resting_values() {
    let mut v = view();
    v.apply(&[show("m", "x.png", 0.5, 1.0, 1.0, 0.0), bob("m", 0.8, 1.0, Ease::Linear)]);
    v.tick(0.7);
    v.apply(&[Cmd::ClearScene, show("m", "x.png", 0.5, 1.0, 1.0, 0.0), bob("m", 0.8, 1.0, Ease::Linear)]);
    assert!(close(v.sprites[0].current_y(), 1.0), "phase 0 again");
    v.tick(0.5);
    assert!(close(v.sprites[0].current_y(), 0.9));
}

#[test]
fn only_the_sprite_named_loops() {
    let mut v = view();
    v.apply(&[show("a", "x.png", 0.3, 1.0, 1.0, 0.0), show("b", "x.png", 0.7, 1.0, 1.0, 0.0), bob("a", 0.8, 1.0, Ease::Linear)]);
    v.tick(0.5);
    assert!(close(v.sprites[0].current_y(), 0.9) && close(v.sprites[1].current_y(), 1.0));
    v.apply(&[bob("ghost", 0.5, 1.0, Ease::Linear)]);
    assert_eq!(v.sprites.iter().filter(|s| s.is_looping()).count(), 1);
}

#[test]
fn a_looping_view_needs_ticks_until_it_stops() {
    let mut v = view();
    v.apply(&[show("m", "x.png", 0.5, 1.0, 1.0, 0.0)]);
    assert!(!v.busy());
    v.apply(&[bob("m", 0.8, 0.02, Ease::Linear)]);
    v.tick(1000.0);
    assert!(v.busy() && v.animating(), "forever");
    assert!(v.sprites[0].current_y() >= 0.8 - 1e-3 && v.sprites[0].current_y() <= 1.0 + 1e-3);
}
