//! `mask=` on `::bg`: the new background follows a greyscale picture; the draw list carries the mask and the
//! threshold, only while the transition runs, and the quads of the batch carry them (stage space).

use super::*;
use crate::batch::build_frame;
use crate::pixels::{mask_coverage, mask_reveal};

fn img(_: &str) -> Option<Size> {
    Some(Size::new(1920.0, 1080.0))
}

fn bg(src: &str, fade: f32, mask: Option<&str>) -> Cmd {
    Cmd::SetBackground { src: src.into(), fade, wipe: None, sound: false, blur: 0.0, mask: mask.map(String::from) }
}

fn mask_of(v: &View) -> Option<(String, f32)> {
    v.draw_list(&img, None).into_iter().find_map(|d| if let Draw::Mask(m) = d { m } else { None })
}

#[test]
fn the_threshold_runs_from_nothing_to_everything_and_the_old_picture_stays_under_it() {
    let mut v = View::new();
    v.apply(&[bg("a.png", 0.0, None)]);
    assert!(mask_of(&v).is_none(), "an ordinary background has no mask");
    v.apply(&[bg("b.png", 2.0, Some("m.png"))]);
    let (name, t0) = mask_of(&v).unwrap();
    assert_eq!((name.as_str(), t0), ("m.png", 0.0));
    let list = v.draw_list(&img, None);
    assert!(list.iter().any(|d| matches!(d, Draw::Image { src, alpha, .. } if src == "a.png" && *alpha == 1.0)));
    assert!(list.iter().any(|d| matches!(d, Draw::Image { src, alpha, .. } if src == "b.png" && *alpha == 1.0)), "opaque: the mask does the fading");
    v.tick(1.0);
    assert_eq!(mask_of(&v).unwrap().1, mask_reveal(0.5));
    v.tick(1.0);
    assert!(mask_of(&v).is_none(), "finished: a plain background again");
    assert!(v.previous_background.is_none());
}

#[test]
fn a_colour_background_is_masked_too_and_quads_carry_the_mask_in_stage_space() {
    let mut v = View::new();
    v.apply(&[bg("a.png", 0.0, None), bg("color:#ff0000", 1.0, Some("m.png"))]);
    v.tick(0.5);
    let f = build_frame(&v.draw_list(&img, None), Size::new(1920.0, 1080.0));
    let masked: Vec<_> = f.quads.iter().filter(|q| q.mask.is_some()).collect();
    assert_eq!(masked.len(), 1, "only the new background");
    let m = masked[0].mask.as_ref().unwrap();
    assert_eq!((m.src.as_str(), m.uv, m.reveal), ("m.png", [0.0, 0.0, 1.0, 1.0], mask_reveal(0.5)));
    assert!(f.quads[0].mask.is_none());
    // half way, a dark spot shows and a white one does not
    assert_eq!((mask_coverage(0.0, m.reveal), mask_coverage(1.0, m.reveal)), (1.0, 0.0));
}

#[test]
fn a_camera_zoom_does_not_move_the_mask() {
    let mut v = View::new().with_speed(0.0);
    v.apply(&[bg("a.png", 0.0, None), bg("b.png", 1.0, Some("m.png")), Cmd::Camera { camera: Camera { zoom: 2.0, x: 0.0, y: 0.0 }, time: 0.0, ease: Ease::Linear }]);
    v.tick(0.5);
    let f = build_frame(&v.draw_list(&img, None), Size::new(1920.0, 1080.0));
    let m = f.quads.iter().find_map(|q| q.mask.as_ref()).unwrap();
    assert_eq!(m.uv, [0.0, 0.0, 1.0, 1.0], "the visible part of the zoomed picture still spans the stage");
}
