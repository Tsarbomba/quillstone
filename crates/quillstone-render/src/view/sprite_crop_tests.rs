//! `crop=` and `brightness=` on sprites in the view: the part of the picture drawn and its size, the whole stack of a
//! layered sprite, easing by moves, the brightness reaching the quad's colour matrix, and `none` animation frames.

use super::*;
use crate::batch::build_frame;
use quillstone_runtime::SpriteStep;
use quillstone_script::parse;

fn img(_: &str) -> Option<Size> {
    Some(Size::new(500.0, 1000.0))
}

fn show(id: &str, layers: &[(&str, &str)], crop: [f32; 4], brightness: f32, flip: bool) -> Cmd {
    Cmd::ShowSprite {
        id: id.into(), src: if layers.is_empty() { format!("{id}.png") } else { String::new() },
        layers: layers.iter().map(|(a, b)| (a.to_string(), b.to_string())).collect(), x: 0.5, y: 1.0, z: 0, scale: 1.0, flip,
        xscale: 1.0, yscale: 1.0, alpha: 1.0, dx: 0.0, dy: 0.0, blur: 0.0, tint: [1.0; 3], brightness, crop, additive: false, fade: 0.0,
    }
}

fn to(crop: [f32; 4], brightness: f32, time: f32) -> Cmd {
    let to = SpriteStep { x: 0.5, y: 1.0, scale: 1.0, xscale: 1.0, yscale: 1.0, alpha: 1.0, dx: 0.0, dy: 0.0, blur: 0.0, tint: [1.0; 3], brightness, crop, time, ease: Ease::Linear, wait: 0.0 };
    Cmd::MoveSprite { id: "m".into(), flip: false, to, queued: false }
}

const WHOLE: [f32; 4] = [0.0, 0.0, 1.0, 1.0];

/// `(dest, uv, brightness)` of every picture drawn.
fn drawn(v: &View) -> Vec<(Rect, Rect, f32)> {
    v.draw_list(&img, None).iter().filter_map(|d| if let Draw::Image { dest, uv, brightness, .. } = d { Some((*dest, *uv, *brightness)) } else { None }).collect()
}

fn near(a: f32, b: f32) -> bool {
    (a - b).abs() < 1e-3
}

#[test]
fn a_crop_draws_only_that_part_at_its_own_size_times_scale() {
    let mut v = View::new().with_speed(0.0);
    v.apply(&[show("m", &[], WHOLE, 0.0, false)]);
    let (whole, uv, _) = drawn(&v)[0];
    assert_eq!(uv, FULL_UV);
    v.apply(&[show("m", &[], [0.25, 0.0, 0.5, 0.5], 0.0, false)]);
    let (part, uv, _) = drawn(&v)[0];
    assert_eq!((uv.x, uv.y, uv.w, uv.h), (0.25, 0.0, 0.5, 0.5));
    assert!(near(part.w, whole.w * 0.5) && near(part.h, whole.h * 0.5), "{part:?} of {whole:?}");
    // The same crop at twice the scale is a close-up: the whole picture's size again.
    let mut zoomed = show("m", &[], [0.25, 0.0, 0.5, 0.5], 0.0, false);
    if let Cmd::ShowSprite { scale, .. } = &mut zoomed {
        *scale = 2.0;
    }
    v.apply(&[zoomed]);
    let (close, _, _) = drawn(&v)[0];
    assert!(near(close.w, whole.w) && near(close.h, whole.h), "{close:?}");
}

#[test]
fn a_mirrored_crop_reads_its_part_right_to_left() {
    let mut v = View::new().with_speed(0.0);
    v.apply(&[show("m", &[], [0.25, 0.1, 0.5, 0.5], 0.0, true)]);
    let (_, uv, _) = drawn(&v)[0];
    assert_eq!((uv.x, uv.y, uv.w, uv.h), (0.75, 0.1, -0.5, 0.5));
}

#[test]
fn a_layered_sprite_crops_every_layer_the_same() {
    let mut v = View::new().with_speed(0.0);
    v.apply(&[show("m", &[("body", "body.png"), ("eyes", "eyes.png")], [0.0, 0.0, 1.0, 0.25], 0.2, false)]);
    let d = drawn(&v);
    assert_eq!(d.len(), 2);
    assert_eq!(d[0], d[1]);
    assert_eq!((d[0].1.h, d[0].2), (0.25, 0.2));
}

#[test]
fn a_move_eases_the_crop_and_the_brightness_and_the_sprite_rests_there() {
    let mut v = View::new().with_speed(0.0);
    v.apply(&[show("m", &[], WHOLE, 0.0, false), to([0.0, 0.0, 0.5, 0.5], 0.5, 1.0)]);
    v.tick(0.5);
    let (_, uv, b) = drawn(&v)[0];
    assert!(near(uv.w, 0.75) && near(uv.h, 0.75) && near(uv.x, 0.0), "{uv:?}");
    assert!(near(b, 0.25), "{b}");
    v.tick(0.6);
    let (_, uv, b) = drawn(&v)[0];
    assert_eq!((uv.w, uv.h, b), (0.5, 0.5, 0.5));
}

#[test]
fn brightness_is_added_before_the_stage_filter_and_only_to_that_sprite() {
    let mut v = View::new().with_speed(0.0);
    v.apply(&[show("m", &[], WHOLE, 0.25, false), show("j", &[], WHOLE, 0.0, false)]);
    let f = build_frame(&v.draw_list(&img, None), Size::new(1920.0, 1080.0));
    assert_eq!(f.quads[0].filter, [[1.0, 0.0, 0.0, 0.25], [0.0, 1.0, 0.0, 0.25], [0.0, 0.0, 1.0, 0.25]]);
    assert_eq!(f.quads[1].filter, crate::filter::IDENTITY);
}

#[test]
fn a_none_frame_draws_nothing_for_its_hold() {
    let anims = parse("::anim{id=blink frames=\"none 1, e.png 0.2\" loop}\n").unwrap().anims;
    let mut v = View::new().with_speed(0.0).with_anims(&anims);
    v.apply(&[show("m", &[("eyes", "anim:blink")], WHOLE, 0.0, false)]);
    assert!(drawn(&v).is_empty(), "hidden at first");
    v.tick(1.1);
    assert_eq!(drawn(&v).len(), 1);
    v.tick(0.2);
    assert!(drawn(&v).is_empty(), "and hidden again after the lap");
}
