//! `blend=add` on sprites in the view: a marker round the sprite's pictures, kept through moves, and the
//! quads it ends up as.

use super::*;
use crate::batch::build_frame;
use quillstone_runtime::SpriteStep;

fn img(_: &str) -> Option<Size> {
    Some(Size::new(500.0, 1000.0))
}

fn show(id: &str, additive: bool) -> Cmd {
    Cmd::ShowSprite {
        id: id.into(), src: format!("{id}.png"), layers: vec![], x: 0.5, y: 1.0, z: 0, scale: 1.0, flip: false,
        xscale: 1.0, yscale: 1.0, alpha: 1.0, dx: 0.0, dy: 0.0, blur: 0.0, tint: [0.5; 3], additive, fade: 0.0,
    }
}

fn shape(v: &View) -> Vec<String> {
    v.draw_list(&img, None)
        .iter()
        .filter_map(|d| match d {
            Draw::Image { src, .. } => Some(src.clone()),
            Draw::Additive(on) => Some(format!("add={on}")),
            _ => None,
        })
        .collect()
}

#[test]
fn only_the_additive_sprites_pictures_are_bracketed() {
    let mut v = View::new().with_speed(0.0);
    v.apply(&[show("a", false), show("glow", true), show("b", false)]);
    assert_eq!(shape(&v), ["a.png", "add=true", "glow.png", "add=false", "b.png"]);
    v.apply(&[show("glow", false)]);
    assert_eq!(shape(&v), ["a.png", "glow.png", "b.png"], "normal is no marker at all");
}

#[test]
fn a_move_keeps_the_blend_and_the_quads_carry_it_with_the_tint_inside() {
    let mut v = View::new().with_speed(0.0);
    v.apply(&[show("glow", true), show("b", false)]);
    v.apply(&[Cmd::MoveSprite { id: "glow".into(), flip: false, to: SpriteStep { x: 0.2, y: 1.0, scale: 1.0, xscale: 1.0, yscale: 1.0, alpha: 1.0, dx: 0.0, dy: 0.0, blur: 0.0, tint: [0.5; 3], time: 0.0, ease: Ease::Linear, wait: 0.0 }, queued: false }]);
    let f = build_frame(&v.draw_list(&img, None), Size::new(1920.0, 1080.0));
    let flags: Vec<(bool, f32)> = f.quads.iter().map(|q| (q.additive, q.color[0])).collect();
    assert_eq!(flags, [(true, 0.5), (false, 0.5)]);
}
