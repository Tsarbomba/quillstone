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
        yscale: 1.0, alpha: 1.0, dx: 0.0, dy: 0.0, blur: 0.0, tint: [1.0; 3], brightness: 0.0, crop: [0.0, 0.0, 1.0, 1.0], additive: false, fade,
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
    let m = Cmd::MoveSprite { id: "m".into(), flip: true, to: SpriteStep { x: 0.8, y: 0.9, scale: 1.2, xscale: 1.0, yscale: 1.0, alpha: 1.0, dx: 0.0, dy: 0.0, blur: 0.0, tint: [1.0; 3], brightness: 0.0, crop: [0.0, 0.0, 1.0, 1.0], time: 1.0, ease: Ease::Linear, wait: 0.0 }, queued: false };
    v.apply(&[layered("m", &FACE, 0.3, 0.0), m.clone()]);
    plain.apply(&[Cmd::ShowSprite { id: "m".into(), src: "body.png".into(), layers: vec![], x: 0.3, y: 1.0, z: 0, scale: 1.0, flip: false, xscale: 1.0, yscale: 1.0, alpha: 1.0, dx: 0.0, dy: 0.0, blur: 0.0, tint: [1.0; 3], brightness: 0.0, crop: [0.0, 0.0, 1.0, 1.0], additive: false, fade: 0.0 }, m]);
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
    v.apply(&[front, Cmd::ShowSprite { id: "o".into(), src: "other.png".into(), layers: vec![], x: 0.5, y: 1.0, z: 0, scale: 1.0, flip: false, xscale: 1.0, yscale: 1.0, alpha: 1.0, dx: 0.0, dy: 0.0, blur: 0.0, tint: [1.0; 3], brightness: 0.0, crop: [0.0, 0.0, 1.0, 1.0], additive: false, fade: 0.0 }]);
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
    v.apply(&[Cmd::ShowSprite { id: "m".into(), src: "pose.png".into(), layers: vec![], x: 0.5, y: 1.0, z: 0, scale: 1.0, flip: false, xscale: 1.0, yscale: 1.0, alpha: 1.0, dx: 0.0, dy: 0.0, blur: 0.0, tint: [1.0; 3], brightness: 0.0, crop: [0.0, 0.0, 1.0, 1.0], additive: false, fade: 1.0 }]);
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
        "::character{id=mika layers=body,eyes layer_sprites=a/{layer}_{value}.png}\n::show{mika body=x eyes=y x=left}\nOne.\n\
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

/// A view with a typewriter and the `flap` animation, two layered sprites `m` and `j` (each with a mouth) and a line by `speaker`.
fn talking(speed: f32, text: &str, speaker: Option<&str>) -> View {
    let anims = parse("::anim{id=flap frames=\"f1.png 0.1, f2.png 0.1\" loop}\n").unwrap().anims;
    let mut v = View::new().with_speed(speed).with_anims(&anims);
    v.apply(&[layered("m", &FACE, 0.3, 0.0), layered("j", &FACE, 0.7, 0.0)]);
    v.apply(&[Cmd::Say { speaker: speaker.map(str::to_string), color: None, text: text.into(), place: Place::Textbox }]);
    v.set_speaker(speaker.map(str::to_string), true);
    v.set_talk(Some(("mouth".into(), "anim:flap".into())), None, None);
    v
}

fn mouths(v: &View) -> Vec<String> {
    names(v).into_iter().filter(|n| n.contains("mouth") || n.starts_with('f')).collect()
}

#[test]
fn the_talk_animation_runs_from_the_start_of_the_line_not_from_when_the_layer_got_its_value() {
    let anims = parse("::anim{id=flap frames=\"f1.png 0.1, f2.png 0.1\"}\n").unwrap().anims;
    let mut v = View::new().with_speed(10.0).with_anims(&anims);
    v.apply(&[layered("m", &FACE, 0.3, 0.0)]);
    v.tick(5.0); // the layer has had its value for a long while
    v.apply(&[Cmd::Say { speaker: Some("m".into()), color: None, text: "abcdefghij".into(), place: Place::Textbox }]);
    v.set_speaker(Some("m".into()), true);
    v.set_talk(Some(("mouth".into(), "anim:flap".into())), None, None);
    v.tick(0.05);
    assert_eq!(mouths(&v), ["f1.png"], "the first frame at the start of the line");
    v.tick(0.1);
    assert_eq!(mouths(&v), ["f2.png"]);
}

#[test]
fn the_talk_layer_flaps_while_the_speaker_types_and_rests_after() {
    let mut v = talking(10.0, "abcdefghij", Some("m"));
    v.tick(0.05);
    assert_eq!(mouths(&v), ["f1.png", "mouth.png"], "m flaps, j keeps its mouth");
    assert!(v.animating());
    v.tick(0.1);
    assert_eq!(mouths(&v)[0], "f2.png", "the animation plays");
    v.tick(2.0);
    assert_eq!(mouths(&v), ["mouth.png", "mouth.png"], "typed: back to the resting value");
    assert!(!v.animating());
    v.apply(&[layered("m", &[("mouth", "smile.png")], 0.3, 0.0)]);
    assert!(names(&v).contains(&"smile.png".to_string()), "a ::show changes the resting value");
}

#[test]
fn a_pause_in_the_line_is_still_typing() {
    let mut v = talking(10.0, "a{w=5}bcdefghij", Some("m"));
    v.tick(1.0);
    assert_eq!(mouths(&v)[0].len(), 6, "{:?}", mouths(&v));
    assert!(mouths(&v)[0].starts_with('f'));
}

#[test]
fn narration_and_other_speakers_flap_nobody() {
    let mut v = talking(10.0, "abcdefghij", None);
    v.tick(0.05);
    assert_eq!(mouths(&v), ["mouth.png", "mouth.png"]);
    v.set_speaker(Some("ann".into()), true);
    assert_eq!(mouths(&v), ["mouth.png", "mouth.png"]);
    v.set_speaker(Some("j".into()), true);
    assert_eq!(mouths(&v), ["mouth.png", "f1.png"], "j speaks: only j flaps");
    v.set_talk(None, None, None);
    assert_eq!(mouths(&v), ["mouth.png", "mouth.png"], "a character without `talk=`");
}

#[test]
fn skipping_and_instant_text_end_the_flap() {
    let mut v = talking(10.0, "abcdefghij", Some("m"));
    v.tick(0.05);
    assert!(!v.press_advance(), "the press finishes typing");
    assert_eq!(mouths(&v), ["mouth.png", "mouth.png"]);
    let v = talking(0.0, "abcdefghij", Some("m"));
    assert_eq!(mouths(&v), ["mouth.png", "mouth.png"]);
}

#[test]
fn a_hidden_talk_layer_stays_hidden() {
    let mut v = talking(10.0, "abcdefghij", Some("m"));
    v.apply(&[layered("m", &[("mouth", "")], 0.3, 0.0)]);
    v.tick(0.05);
    assert_eq!(mouths(&v), ["mouth.png"], "only j's");
}

#[test]
fn a_value_pattern_flaps_each_value_with_its_own_animation_and_skips_one_without() {
    let anims = parse("::anim{id=flap_smile frames=\"s1.png 0.1, s2.png 0.1\" loop}\n::anim{id=flap_open frames=\"o1.png 0.1, o2.png 0.1\" loop}\n").unwrap().anims;
    let mut v = View::new().with_speed(10.0).with_anims(&anims);
    let shown = |m: &str, j: &str| [layered("m", &[("body", "body.png"), ("mouth", m)], 0.3, 0.0), layered("j", &[("body", "body.png"), ("mouth", j)], 0.7, 0.0)];
    v.apply(&shown("a/mouth_smile.png", "a/mouth_grin.png"));
    v.apply(&[Cmd::Say { speaker: Some("m".into()), color: None, text: "abcdefghij".into(), place: Place::Textbox }]);
    v.set_speaker(Some("m".into()), true);
    v.set_talk(Some(("mouth".into(), "anim:flap_{value}".into())), Some("a/{layer}_{value}.png".into()), None);
    v.tick(0.05);
    assert_eq!(mouths2(&v), ["s1.png", "a/mouth_grin.png"]);
    assert_eq!(v.shown_animations(), ["flap_open", "flap_smile"], "every animation the pattern may pick is loaded");
    v.apply(&shown("a/mouth_open.png", "a/mouth_grin.png"));
    assert_eq!(mouths2(&v)[0], "o1.png", "another value, another animation");
    v.apply(&shown("a/mouth_grin.png", "a/mouth_grin.png"));
    assert_eq!(mouths2(&v)[0], "a/mouth_grin.png", "no `flap_grin`: the mouth keeps its value");
    v.tick(2.0);
    assert_eq!(mouths2(&v), ["a/mouth_grin.png", "a/mouth_grin.png"]);
}

fn mouths2(v: &View) -> Vec<String> {
    names(v).into_iter().filter(|n| n != "body.png").collect()
}

// ---- `::layer` rules ----------------------------------------------------------

const RULES: &str = "---\nvars: { outfit: 0, hurt: false, mood: \"smile\" }\n---\n\
    ::character{id=m layers=body,mouth,bandage layer_sprites=a/{layer}_{value}.png}\n\
    ::layer{m body=casual if=\"outfit == 0\"}\n::layer{m body=dress if=\"outfit == 1\"}\n\
    ::layer{m bandage=on if=hurt}\n::layer{m mouth={mood}}\n";

fn ruled() -> (View, quillstone_script::Vars) {
    let p = parse(RULES).unwrap();
    let mut v = View::new().with_speed(0.0).with_layer_rules(&p.layer_rules);
    v.apply(&[layered("m", &[("body", ""), ("mouth", ""), ("bandage", "")], 0.5, 0.0)]);
    (v, p.vars)
}

#[test]
fn a_ruled_layer_follows_the_variables_with_a_crossfade() {
    let (mut v, mut vars) = ruled();
    v.set_vars(&vars);
    assert_eq!(names(&v), ["a/body_casual.png", "a/mouth_smile.png"], "the first pictures show at once; no rule holds for the bandage");
    assert_eq!(alpha_of(&v, "a/body_casual.png"), 1.0);
    vars.insert("outfit".into(), quillstone_script::Value::Int(1));
    vars.insert("hurt".into(), quillstone_script::Value::Bool(true));
    vars.insert("mood".into(), quillstone_script::Value::Str("open".into()));
    v.set_vars(&vars);
    assert_eq!(names(&v), ["a/body_casual.png", "a/body_dress.png", "a/mouth_smile.png", "a/mouth_open.png", "a/bandage_on.png"]);
    assert_eq!(alpha_of(&v, "a/body_dress.png"), 0.0);
    v.tick(RULE_FADE);
    assert_eq!(names(&v), ["a/body_dress.png", "a/mouth_open.png", "a/bandage_on.png"], "the old pictures are gone once it is done");
    // no rule holds any more: the layer is empty again
    vars.insert("outfit".into(), quillstone_script::Value::Int(7));
    vars.insert("hurt".into(), quillstone_script::Value::Bool(false));
    v.set_vars(&vars);
    v.tick(RULE_FADE);
    assert_eq!(names(&v), ["a/mouth_open.png"]);
}

#[test]
fn a_new_show_keeps_the_ruled_layers_and_a_hidden_sprite_starts_over() {
    let (mut v, mut vars) = ruled();
    v.set_vars(&vars);
    // a `::show` that moves the sprite carries the ruled layers as empty: they keep what the rules gave
    v.apply(&[layered("m", &[("body", ""), ("mouth", ""), ("bandage", "")], 0.2, 0.5)]);
    v.set_vars(&vars);
    assert_eq!(names(&v), ["a/body_casual.png", "a/mouth_smile.png"]);
    assert_eq!(alpha_of(&v, "a/body_casual.png"), 1.0, "no flicker");
    // hidden and shown again: the layers appear whole at once, from the variables now
    v.apply(&[Cmd::HideSprite { id: "m".into(), fade: 0.0 }]);
    vars.insert("outfit".into(), quillstone_script::Value::Int(1));
    v.apply(&[layered("m", &[("body", ""), ("mouth", ""), ("bandage", "")], 0.5, 0.0)]);
    v.set_vars(&vars);
    assert_eq!(names(&v), ["a/body_dress.png", "a/mouth_smile.png"]);
    assert_eq!(alpha_of(&v, "a/body_dress.png"), 1.0);
}

// ---- `::place`d layers ---------------------------------------------------------------------

/// A 280x500 canvas (the body) with a 40x50 hat placed at 100,50 at scale 2: 80x100 of the canvas.
fn hatted(crop: [f32; 4], flip: bool) -> (View, impl Fn(&str) -> Option<Size>) {
    let p = parse("::character{id=m layers=body,hat layer_sprites=a/{layer}_{value}.png}\n::place{m hat=cap at=100,50 scale=2}\n").unwrap();
    let mut v = View::new().with_speed(0.0).with_layer_places(&p.characters);
    let mut show = layered("m", &[("body", "a/body_x.png"), ("hat", "a/hat_cap.png")], 0.5, 0.0);
    if let Cmd::ShowSprite { crop: c, flip: f, .. } = &mut show {
        (*c, *f) = (crop, flip);
    }
    v.apply(&[show]);
    (v, |s: &str| Some(if s.contains("hat") { Size::new(40.0, 50.0) } else { Size::new(280.0, 500.0) }))
}

fn hat_and_body(v: &View, sizes: &dyn Fn(&str) -> Option<Size>) -> (Rect, Rect, Rect) {
    let d: Vec<(Rect, Rect)> = v.draw_list(sizes, None).into_iter().filter_map(|d| if let Draw::Image { dest, uv, .. } = d { Some((dest, uv)) } else { None }).collect();
    assert_eq!(d.len(), 2, "{d:?}");
    (d[0].0, d[1].0, d[1].1)
}

#[test]
fn a_placed_layer_sits_on_the_canvas_at_its_own_size() {
    let (v, sizes) = hatted([0.0, 0.0, 1.0, 1.0], false);
    let (body, hat, uv) = hat_and_body(&v, &sizes);
    let k = body.w / 280.0;
    assert!(close(hat.x, body.x + 100.0 * k) && close(hat.y, body.y + 50.0 * k), "{hat:?} on {body:?}");
    assert!(close(hat.w, 80.0 * k) && close(hat.h, 100.0 * k));
    assert_eq!(uv, FULL_UV);
}

#[test]
fn a_flipped_sprite_mirrors_the_place_of_its_layer() {
    let (v, sizes) = hatted([0.0, 0.0, 1.0, 1.0], true);
    let (body, hat, uv) = hat_and_body(&v, &sizes);
    let k = body.w / 280.0;
    assert!(close(hat.x, body.x + (280.0 - 180.0) * k), "{hat:?} on {body:?}");
    assert!(close(hat.w, 80.0 * k) && close(hat.y, body.y + 50.0 * k));
    assert!(uv.w < 0.0, "read right to left: {uv:?}");
}

#[test]
fn a_crop_cuts_a_placed_layer_to_the_cropped_canvas() {
    // the left half and top half of the canvas: the hat (x 100..180, y 50..150) keeps x 100..140
    let (v, sizes) = hatted([0.0, 0.0, 0.5, 0.5], false);
    let (body, hat, uv) = hat_and_body(&v, &sizes);
    let k = body.w / 140.0;
    assert!(close(hat.x, body.x + 100.0 * k) && close(hat.w, 40.0 * k) && close(hat.h, 100.0 * k), "{hat:?} on {body:?}");
    assert!(close(uv.x, 0.0) && close(uv.w, 0.5) && close(uv.h, 1.0), "{uv:?}");
    // a crop that holds none of it draws no hat
    let (v, sizes) = hatted([0.5, 0.5, 0.5, 0.5], false);
    let n = v.draw_list(&sizes, None).iter().filter(|d| matches!(d, Draw::Image { src, .. } if src.contains("hat"))).count();
    assert_eq!(n, 0);
}

#[test]
fn the_sprites_scale_and_a_layers_own_scale_both_apply() {
    let (mut v, sizes) = hatted([0.0, 0.0, 1.0, 1.0], false);
    let mut big = layered("m", &[("body", "a/body_x.png"), ("hat", "a/hat_cap.png")], 0.5, 0.0);
    if let Cmd::ShowSprite { scale, .. } = &mut big {
        *scale = 0.5;
    }
    v.apply(&[big]);
    let (body, hat, _) = hat_and_body(&v, &sizes);
    assert!(close(hat.w / body.w, 80.0 / 280.0) && close(hat.h / body.h, 100.0 / 500.0), "{hat:?} on {body:?}");
}

#[test]
fn the_canvas_is_the_first_layer_that_is_not_placed_and_with_none_nothing_is_drawn() {
    let p = parse("::character{id=m layers=body,hat layer_sprites=a/{layer}_{value}.png}\n::place{m hat at=0,0}\n").unwrap();
    let mut v = View::new().with_speed(0.0).with_layer_places(&p.characters);
    v.apply(&[layered("m", &[("body", ""), ("hat", "a/hat_cap.png")], 0.5, 0.0)]);
    assert!(v.draw_list(&size, None).iter().all(|d| !matches!(d, Draw::Image { .. })));
    // a layer-wide place also covers an animation's frames
    let q = parse("::anim{id=wob frames=\"a/w1.png 1\"}\n::character{id=m layers=body,hat layer_sprites=a/{layer}_{value}.png}\n::place{m hat at=10,0}\n").unwrap();
    let mut v = View::new().with_speed(0.0).with_anims(&q.anims).with_layer_places(&q.characters);
    v.apply(&[layered("m", &[("body", "a/body_x.png"), ("hat", "anim:wob")], 0.5, 0.0)]);
    let sizes = |s: &str| Some(if s.contains("w1") { Size::new(28.0, 50.0) } else { Size::new(280.0, 500.0) });
    let (body, frame, _) = hat_and_body(&v, &sizes);
    assert!(close(frame.x, body.x + 10.0 * body.w / 280.0) && close(frame.w, body.w / 10.0), "{frame:?} on {body:?}");
}
