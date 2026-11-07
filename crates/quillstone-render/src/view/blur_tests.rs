//! `blur` on sprites and the background in the view: it rides on the draw items, animates with moves, follows the
//! camera's zoom and crossfades when a background's blur changes.

use super::*;
use quillstone_runtime::SpriteStep;

fn img(_: &str) -> Option<Size> {
    Some(Size::new(500.0, 1000.0))
}

fn show(blur: f32) -> Cmd {
    Cmd::ShowSprite {
        id: "m".into(), src: "m.png".into(), layers: vec![], x: 0.5, y: 1.0, z: 0, scale: 1.0, flip: false, xscale: 1.0, yscale: 1.0,
        alpha: 1.0, dx: 0.0, dy: 0.0, blur, tint: [1.0; 3], additive: false, fade: 0.0,
    }
}

fn leg(blur: f32, time: f32, queued: bool) -> Cmd {
    let to = SpriteStep { x: 0.5, y: 1.0, scale: 1.0, xscale: 1.0, yscale: 1.0, alpha: 1.0, dx: 0.0, dy: 0.0, blur, tint: [1.0; 3], time, ease: Ease::Linear, wait: 0.0 };
    Cmd::MoveSprite { id: "m".into(), flip: false, to, queued }
}

fn bg(src: &str, fade: f32, blur: f32) -> Cmd {
    Cmd::SetBackground { src: src.into(), fade, wipe: None, sound: false, blur, mask: None }
}

/// (picture, blur) of every image drawn, back to front.
fn blurs(v: &View) -> Vec<(String, f32)> {
    v.draw_list(&img, None).iter().filter_map(|d| if let Draw::Image { src, blur, .. } = d { Some((src.clone(), *blur)) } else { None }).collect()
}

#[test]
fn a_blurred_sprite_and_background_carry_their_radius_in_the_draw_list() {
    let mut v = View::new().with_speed(0.0);
    v.apply(&[bg("b.png", 0.0, 12.0), show(8.0)]);
    assert_eq!(blurs(&v), [("b.png".to_string(), 12.0), ("m.png".to_string(), 8.0)]);
    v.apply(&[bg("b.png", 0.0, 0.0)]);
    assert_eq!(blurs(&v)[0].1, 0.0, "the same picture, sharp again at once without a fade");
    v.apply(&[Cmd::ClearScene, show(0.0)]);
    assert_eq!(blurs(&v), [("m.png".to_string(), 0.0)]);
}

#[test]
fn a_move_animates_blur_through_a_sequence() {
    let mut v = View::new().with_speed(0.0);
    v.apply(&[show(0.0), leg(32.0, 1.0, false), leg(0.0, 1.0, true)]);
    let blur = |v: &View| blurs(v)[0].1;
    assert_eq!(blur(&v), 0.0);
    v.tick(0.5);
    assert!((blur(&v) - 16.0).abs() < 1e-3);
    v.tick(0.5);
    assert!((blur(&v) - 32.0).abs() < 1e-3);
    v.tick(0.5);
    assert!((blur(&v) - 16.0).abs() < 1e-3, "back down in the second leg");
    v.tick(5.0);
    assert!(blur(&v).abs() < 1e-3 && !v.animating());
}

#[test]
fn a_change_of_blur_with_a_fade_crossfades_and_the_outgoing_picture_keeps_its_own() {
    let mut v = View::new().with_speed(0.0);
    v.apply(&[bg("b.png", 0.0, 0.0)]);
    v.apply(&[bg("b.png", 1.0, 20.0)]);
    v.tick(0.25);
    let list = v.draw_list(&img, None);
    let seen: Vec<(f32, f32)> = list.iter().filter_map(|d| if let Draw::Image { alpha, blur, .. } = d { Some((*alpha, *blur)) } else { None }).collect();
    assert_eq!(seen, [(1.0, 0.0), (0.25, 20.0)], "sharp underneath, the blurred one fading in");
    v.tick(1.0);
    assert_eq!(blurs(&v), [("b.png".to_string(), 20.0)]);
}

#[test]
fn the_cameras_zoom_widens_the_blur_with_the_picture() {
    let mut v = View::new().with_speed(0.0);
    v.apply(&[show(10.0), Cmd::Camera { camera: Camera { zoom: 2.0, x: 0.5, y: 0.5 }, time: 0.0, ease: Ease::Linear }]);
    assert!((blurs(&v)[0].1 - 20.0).abs() < 1e-3);
}
