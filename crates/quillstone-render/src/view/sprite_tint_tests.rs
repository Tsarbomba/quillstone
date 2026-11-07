//! `tint` on sprites in the view: a marker round the sprite's pictures, eased per channel by moves, the whole
//! stack of a layered sprite, and the quad colour it ends up as (times the speaker shade).

use super::*;
use crate::batch::build_frame;
use quillstone_runtime::SpriteStep;

fn img(_: &str) -> Option<Size> {
    Some(Size::new(500.0, 1000.0))
}

fn show(id: &str, layers: &[(&str, &str)], tint: [f32; 3]) -> Cmd {
    Cmd::ShowSprite {
        id: id.into(), src: if layers.is_empty() { format!("{id}.png") } else { String::new() },
        layers: layers.iter().map(|(a, b)| (a.to_string(), b.to_string())).collect(), x: 0.5, y: 1.0, z: 0, scale: 1.0, flip: false,
        xscale: 1.0, yscale: 1.0, alpha: 1.0, dx: 0.0, dy: 0.0, blur: 0.0, tint, additive: false, fade: 0.0,
    }
}

fn leg(tint: [f32; 3], time: f32, queued: bool) -> Cmd {
    let to = SpriteStep { x: 0.5, y: 1.0, scale: 1.0, xscale: 1.0, yscale: 1.0, alpha: 1.0, dx: 0.0, dy: 0.0, blur: 0.0, tint, time, ease: Ease::Linear, wait: 0.0 };
    Cmd::MoveSprite { id: "m".into(), flip: false, to, queued }
}

/// What the stage draws, as `bg`, `tint(..)` and `end` markers around picture names.
fn shape(v: &View) -> Vec<String> {
    v.draw_list(&img, None)
        .iter()
        .filter_map(|d| match d {
            Draw::Image { src, .. } => Some(src.clone()),
            Draw::Tint(Some(t)) => Some(format!("tint{t:?}")),
            Draw::Tint(None) => Some("end".into()),
            _ => None,
        })
        .collect()
}

fn background(src: &str) -> Cmd {
    Cmd::SetBackground { src: src.into(), fade: 0.0, wipe: None, sound: false, blur: 0.0, mask: None }
}

#[test]
fn only_the_tinted_sprites_pictures_are_bracketed() {
    let mut v = View::new().with_speed(0.0);
    v.apply(&[background("b.png"), show("m", &[], [0.5, 0.25, 1.0]), show("j", &[], [1.0; 3])]);
    assert_eq!(shape(&v), ["b.png", "tint[0.5, 0.25, 1.0]", "m.png", "end", "j.png"]);
    v.apply(&[show("m", &[], [1.0; 3])]);
    assert_eq!(shape(&v), ["b.png", "m.png", "j.png"], "white is no marker at all");
}

#[test]
fn a_layered_sprite_is_tinted_as_one_stack() {
    let mut v = View::new().with_speed(0.0);
    v.apply(&[show("m", &[("body", "body.png"), ("eyes", "eyes.png")], [0.0; 3])]);
    assert_eq!(shape(&v), ["tint[0.0, 0.0, 0.0]", "body.png", "eyes.png", "end"]);
}

#[test]
fn a_move_eases_each_channel_and_a_sequence_goes_on() {
    let mut v = View::new().with_speed(0.0);
    v.apply(&[show("m", &[], [1.0; 3]), leg([0.0, 0.5, 1.0], 1.0, false), leg([1.0; 3], 1.0, true)]);
    let tint = |v: &View| v.sprites.iter().find(|s| s.id == "m").unwrap().current_tint();
    assert_eq!(tint(&v), [1.0; 3]);
    v.tick(0.5);
    let t = tint(&v);
    assert!((t[0] - 0.5).abs() < 1e-3 && (t[1] - 0.75).abs() < 1e-3 && (t[2] - 1.0).abs() < 1e-3, "{t:?}");
    v.tick(0.5);
    assert!((tint(&v)[0]).abs() < 1e-3);
    v.tick(5.0);
    assert_eq!(tint(&v), [1.0; 3]);
    assert!(!v.animating());
}

#[test]
fn a_loop_runs_between_the_resting_tint_and_the_far_one() {
    let mut v = View::new().with_speed(0.0);
    let far = SpriteStep { tint: [0.0; 3], time: 1.0, ..match leg([0.0; 3], 1.0, false) { Cmd::MoveSprite { to, .. } => to, _ => unreachable!() } };
    v.apply(&[show("m", &[], [1.0; 3]), Cmd::LoopSprite { id: "m".into(), flip: false, to: far }]);
    v.tick(1.0);
    assert!(v.sprites[0].current_tint()[1].abs() < 1e-3, "at the far end");
    v.tick(1.0);
    assert!((v.sprites[0].current_tint()[1] - 1.0).abs() < 1e-3, "back at rest");
}

#[test]
fn the_quad_colour_is_the_tint_times_the_speaker_shade() {
    let mut v = View::new().with_speed(0.0);
    v.apply(&[show("m", &[], [0.5, 0.25, 1.0])]);
    v.sprites[0].shade = 0.5;
    let frame = build_frame(&v.draw_list(&img, None), Size::new(1920.0, 1080.0));
    assert_eq!(frame.quads.len(), 1);
    assert_eq!(frame.quads[0].color, [0.25, 0.125, 0.5, 1.0]);
    // the marker ends with the sprite: what follows is not tinted
    let after = build_frame(&[Draw::Tint(Some([0.0; 3])), Draw::Tint(None), Draw::Rect { rect: Rect::new(0.0, 0.0, 1.0, 1.0), color: [1.0; 4] }], Size::new(1920.0, 1080.0));
    assert_eq!(after.quads[0].color, [1.0; 4]);
}
