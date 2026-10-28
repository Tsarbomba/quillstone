//! Layered sprites in the view: stacking order, one layer crossfading at a time, animations
//! as layers, hiding, and everything else (move, flip, fade, z) acting on the whole.

use super::*;
use quillstone_script::parse;

fn view() -> View {
    let anims = parse("::anim{id=blink frames=\"open.png 3, closed.png 0.1\" loop}\n").unwrap().anims;
    View::new().with_speed(0.0).with_anims(&anims)
}

fn size(src: &str) -> Option<Size> {
    // an unknown picture is not loaded (yet); everything else is one canvas size
    (src != "unloaded.png").then(|| Size::new(280.0, 500.0))
}

fn layered(id: &str, layers: &[(&str, &str)], x: f32, fade: f32) -> Cmd {
    Cmd::ShowSprite {
        id: id.into(),
        src: String::new(),
        layers: layers.iter().map(|(a, b)| (a.to_string(), b.to_string())).collect(),
        x,
        y: 1.0,
        z: 0,
        scale: 1.0,
        flip: false,
        xscale: 1.0,
        yscale: 1.0,
        fade,
    }
}

const FACE: [(&str, &str); 3] = [("body", "body.png"), ("eyes", "eyes.png"), ("mouth", "mouth.png")];

/// (picture, destination, alpha) of every image drawn, back to front.
fn drawn(v: &View) -> Vec<(String, Rect, f32)> {
    v.draw_list(&size, None)
        .into_iter()
        .filter_map(|d| if let Draw::Image { src, dest, alpha, .. } = d { Some((src, dest, alpha)) } else { None })
        .collect()
}

fn names(v: &View) -> Vec<String> {
    drawn(v).into_iter().map(|d| d.0).collect()
}

fn alpha_of(v: &View, src: &str) -> f32 {
    drawn(v).iter().find(|d| d.0 == src).map_or(-1.0, |d| d.2)
}

fn close(a: f32, b: f32) -> bool {
    (a - b).abs() < 1e-4
}

#[test]
fn layers_are_drawn_back_to_front_in_the_same_place_and_size() {
    let mut v = view();
    v.apply(&[layered("m", &FACE, 0.3, 0.0)]);
    let d = drawn(&v);
    assert_eq!(names(&v), ["body.png", "eyes.png", "mouth.png"]);
    assert!(d.iter().all(|(_, dest, a)| *dest == d[0].1 && *a == 1.0), "{d:?}");
    assert!(d[0].1.h > 0.0 && d[0].1.w > 0.0);
    // the sprite's own (empty) picture is never asked for
    let asked = std::cell::RefCell::new(Vec::new());
    v.draw_list(&|s: &str| { asked.borrow_mut().push(s.to_string()); size(s) }, None);
    assert!(asked.borrow().iter().all(|s| !s.is_empty()), "{:?}", asked.borrow());
}

#[test]
fn the_layers_follow_the_one_sprite_in_every_way() {
    let mut v = view();
    let mut plain = view();
    let m = Cmd::MoveSprite { id: "m".into(), x: 0.8, y: 0.9, scale: 1.2, flip: true, xscale: 1.0, yscale: 1.0, time: 1.0, ease: Ease::Linear };
    v.apply(&[layered("m", &FACE, 0.3, 0.0), m.clone()]);
    plain.apply(&[Cmd::ShowSprite { id: "m".into(), src: "body.png".into(), layers: vec![], x: 0.3, y: 1.0, z: 0, scale: 1.0, flip: false, xscale: 1.0, yscale: 1.0, fade: 0.0 }, m]);
    for t in [0.0, 0.3, 0.5, 1.0] {
        v.tick(t);
        plain.tick(t);
        let (a, b) = (v.draw_list(&size, None), plain.draw_list(&size, None));
        assert_eq!(a.len(), 3);
        for layer in &a {
            // the same rectangle and mirroring as the plain sprite, at every moment of the move
            let (Draw::Image { dest, uv, .. }, Draw::Image { dest: want, uv: want_uv, .. }) = (layer, &b[0]) else { panic!() };
            assert_eq!((dest, uv), (want, want_uv), "t={t}");
        }
    }
    // z puts the whole stack in front of or behind another sprite
    let mut v = view();
    let mut front = layered("m", &FACE, 0.5, 0.0);
    if let Cmd::ShowSprite { z, .. } = &mut front {
        *z = 5;
    }
    v.apply(&[front, Cmd::ShowSprite { id: "o".into(), src: "other.png".into(), layers: vec![], x: 0.5, y: 1.0, z: 0, scale: 1.0, flip: false, xscale: 1.0, yscale: 1.0, fade: 0.0 }]);
    assert_eq!(names(&v), ["other.png", "body.png", "eyes.png", "mouth.png"]);
}

#[test]
fn only_the_layer_that_changed_crossfades() {
    let mut v = view();
    v.apply(&[layered("m", &FACE, 0.5, 0.0)]);
    v.tick(1.0);
    let mut next = FACE;
    next[2] = ("mouth", "open.png");
    v.apply(&[layered("m", &next, 0.5, 1.0)]);
    assert_eq!(names(&v), ["body.png", "eyes.png", "mouth.png", "open.png"], "the old mouth stays under the new one");
    assert_eq!((alpha_of(&v, "body.png"), alpha_of(&v, "eyes.png"), alpha_of(&v, "mouth.png")), (1.0, 1.0, 1.0));
    assert_eq!(alpha_of(&v, "open.png"), 0.0);
    v.tick(0.25);
    assert!(close(alpha_of(&v, "open.png"), 0.25));
    assert_eq!(alpha_of(&v, "body.png"), 1.0, "the others do not flicker");
    v.tick(0.75);
    assert_eq!(names(&v), ["body.png", "eyes.png", "open.png"]);
    assert_eq!(alpha_of(&v, "open.png"), 1.0);
    // without a fade the swap is at once
    next[0] = ("body", "formal.png");
    v.apply(&[layered("m", &next, 0.5, 0.0)]);
    assert_eq!(names(&v), ["formal.png", "eyes.png", "open.png"]);
    // the same layers again change nothing, whatever the fade
    v.apply(&[layered("m", &next, 0.5, 1.0)]);
    assert_eq!(names(&v), ["formal.png", "eyes.png", "open.png"]);
}

#[test]
fn a_layer_that_is_hidden_fades_out_and_one_that_comes_back_fades_in() {
    let mut v = view();
    v.apply(&[layered("m", &FACE, 0.5, 0.0)]);
    v.tick(1.0);
    v.apply(&[layered("m", &[("body", "body.png"), ("eyes", ""), ("mouth", "mouth.png")], 0.5, 1.0)]);
    assert_eq!(alpha_of(&v, "eyes.png"), 1.0);
    v.tick(0.25);
    assert!(close(alpha_of(&v, "eyes.png"), 0.75));
    v.tick(1.0);
    assert_eq!(names(&v), ["body.png", "mouth.png"]);
    v.apply(&[layered("m", &FACE, 0.5, 0.5)]);
    assert_eq!(names(&v), ["body.png", "eyes.png", "mouth.png"]);
    assert_eq!(alpha_of(&v, "eyes.png"), 0.0);
    v.tick(0.25);
    assert!(close(alpha_of(&v, "eyes.png"), 0.5));
}

#[test]
fn an_animation_layer_blinks_by_itself_and_keeps_the_view_ticking() {
    let mut v = view();
    v.apply(&[layered("m", &[("body", "body.png"), ("eyes", "anim:blink"), ("mouth", "mouth.png")], 0.5, 0.0)]);
    assert_eq!(names(&v), ["body.png", "open.png", "mouth.png"]);
    assert!(v.animating() && v.busy(), "a running animation layer needs frames");
    v.tick(3.05);
    assert_eq!(names(&v), ["body.png", "closed.png", "mouth.png"]);
    v.tick(0.1);
    assert_eq!(names(&v), ["body.png", "open.png", "mouth.png"]);
    assert_eq!(v.shown_animations(), ["blink"]);
    // the other layers' changes do not restart the blink
    v.tick(1.0);
    v.apply(&[layered("m", &[("body", "body.png"), ("eyes", "anim:blink"), ("mouth", "open.png")], 0.5, 0.5)]);
    v.tick(0.5);
    v.tick(1.5);
    assert_eq!(names(&v)[1], "closed.png", "the clock ran on: 3.15 + 1 + 0.5 + 1.5 = 6.15 is in the second blink");
    // settling (skip) leaves the clock alone but finishes fades
    let mut v = view();
    v.apply(&[layered("m", &FACE, 0.5, 0.0)]);
    v.apply(&[layered("m", &[("body", "body.png"), ("eyes", "anim:blink"), ("mouth", "mouth.png")], 0.5, 2.0)]);
    v.settle(10.0);
    assert_eq!(names(&v), ["body.png", "open.png", "mouth.png"]);
}

#[test]
fn hiding_fades_every_layer_together() {
    let mut v = view();
    v.apply(&[layered("m", &FACE, 0.5, 0.5)]);
    v.tick(1.0);
    v.apply(&[Cmd::HideSprite { id: "m".into(), fade: 1.0 }]);
    v.tick(0.5);
    let d = drawn(&v);
    assert_eq!(d.len(), 3);
    assert!(d.iter().all(|(_, _, a)| close(*a, 0.5)), "{d:?}");
    v.tick(0.6);
    assert!(drawn(&v).is_empty() && v.sprites.is_empty());
    // a fade-in of the whole sprite fades each layer in
    let mut v = view();
    v.apply(&[layered("m", &FACE, 0.5, 1.0)]);
    v.tick(0.5);
    assert!(drawn(&v).iter().all(|(_, _, a)| close(*a, 0.5)));
}

#[test]
fn a_pose_and_layers_replace_each_other_with_a_crossfade() {
    let mut v = view();
    v.apply(&[layered("m", &FACE, 0.5, 0.0)]);
    v.apply(&[Cmd::ShowSprite { id: "m".into(), src: "pose.png".into(), layers: vec![], x: 0.5, y: 1.0, z: 0, scale: 1.0, flip: false, xscale: 1.0, yscale: 1.0, fade: 1.0 }]);
    assert_eq!(names(&v), ["body.png", "eyes.png", "mouth.png", "pose.png"], "the layers stay under the fading-in pose");
    v.tick(0.5);
    assert!(close(alpha_of(&v, "pose.png"), 0.5));
    v.tick(0.6);
    assert_eq!(names(&v), ["pose.png"]);
    // and back: the pose stays under the layers until they are in
    v.apply(&[layered("m", &FACE, 0.5, 1.0)]);
    assert_eq!(names(&v), ["pose.png", "body.png", "eyes.png", "mouth.png"]);
    assert_eq!(alpha_of(&v, "body.png"), 0.0);
    v.tick(1.1);
    assert_eq!(names(&v), ["body.png", "eyes.png", "mouth.png"]);
    assert!(v.sprites[0].src.is_empty());
}

#[test]
fn a_layer_that_is_not_loaded_yet_is_left_out_and_a_new_scene_clears_the_layers() {
    let mut v = view();
    v.apply(&[layered("m", &[("body", "body.png"), ("eyes", "unloaded.png")], 0.5, 0.0)]);
    assert_eq!(names(&v), ["body.png"]);
    v.apply(&[Cmd::ClearScene]);
    assert!(drawn(&v).is_empty() && v.sprites.is_empty());
}

#[test]
fn a_whole_game_with_layers_replays_through_the_view() {
    let p = parse(
        "::character{id=mika layers=body,eyes layer_sprites=a/{layer}_{value}.png}\n::show{mika body=x eyes=y at=left}\nOne.\n\
         ::show{mika eyes=z}\nTwo.\n",
    )
    .unwrap();
    let (mut game, first) = quillstone_runtime::Session::start(&p);
    let mut v = view();
    v.apply(&first);
    assert_eq!(names(&v), ["a/body_x.png", "a/eyes_y.png"]);
    v.apply(&game.input(quillstone_runtime::Input::Advance));
    v.settle(10.0);
    assert_eq!(names(&v), ["a/body_x.png", "a/eyes_z.png"]);
    // a rollback redraws the earlier look
    let back = game.rollback().unwrap();
    v.apply(&back);
    assert_eq!(names(&v), ["a/body_x.png", "a/eyes_y.png"]);
}
