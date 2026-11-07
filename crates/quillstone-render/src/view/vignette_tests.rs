//! `::vignette` in the view: it eases like a tint, sits over the tint and filter and under the cover and text.

use super::*;
use quillstone_script::Vignette;

fn img(_: &str) -> Option<Size> {
    Some(Size::new(500.0, 1000.0))
}

fn dark(strength: f32, size: f32) -> Vignette {
    Vignette { color: [0.0; 3], strength, size }
}

fn set(v: Vignette, fade: f32) -> Cmd {
    Cmd::Vignette { set: Some(v), fade }
}

fn bg(src: &str) -> Cmd {
    Cmd::SetBackground { src: src.into(), fade: 0.0, wipe: None, sound: false, blur: 0.0, mask: None }
}

fn close(a: f32, b: f32) -> bool {
    (a - b).abs() < 1e-5
}

#[test]
fn it_eases_from_clear_then_from_where_it_is_and_out_to_nothing() {
    let mut v = View::new();
    v.apply(&[set(dark(0.8, 0.2), 2.0)]);
    assert!(v.busy(), "the fade keeps the player redrawing");
    assert_eq!(v.vignette(), None, "starts clear");
    v.tick(1.0);
    let now = v.vignette().unwrap();
    assert!(close(now.strength, 0.4) && close(now.size, 0.2), "from nothing: only the strength grows, {now:?}");
    // A new one mid-fade eases every parameter from the current values.
    v.apply(&[set(Vignette { color: [1.0, 0.0, 0.0], strength: 0.2, size: 0.6 }, 1.0)]);
    v.tick(0.5);
    let now = v.vignette().unwrap();
    assert!(close(now.strength, 0.3) && close(now.size, 0.4) && close(now.color[0], 0.5), "{now:?}");
    v.tick(1.0);
    assert_eq!(v.vignette(), Some(Vignette { color: [1.0, 0.0, 0.0], strength: 0.2, size: 0.6 }));
    assert!(!v.busy());
    v.apply(&[Cmd::Vignette { set: None, fade: 1.0 }]);
    v.tick(0.5);
    let now = v.vignette().unwrap();
    assert!(close(now.strength, 0.1) && close(now.size, 0.6), "keeps its look while it fades away");
    v.tick(0.6);
    assert_eq!(v.vignette(), None);
    assert!(!v.busy());
    // Removing nothing, and a set without a fade, are at once.
    v.apply(&[Cmd::Vignette { set: None, fade: 1.0 }]);
    assert!(!v.busy());
    v.apply(&[set(dark(0.5, 0.5), 0.0)]);
    assert_eq!(v.vignette(), Some(dark(0.5, 0.5)));
    assert!(!v.busy());
    v.apply(&[Cmd::ClearScene]);
    assert_eq!(v.vignette(), None, "a new scene clears it");
    v.apply(&[set(dark(0.5, 0.5), 5.0)]);
    v.settle(30.0);
    assert_eq!(v.vignette(), Some(dark(0.5, 0.5)), "a settle finishes the fade");
}

#[test]
fn it_is_over_the_tint_and_the_filter_and_under_the_cover_flash_and_text() {
    let mut v = View::new();
    v.apply(&[
        bg("b.png"),
        show_a(),
        Cmd::Tint { set: Some(quillstone_script::Tint { color: [0.0, 0.0, 1.0], alpha: 0.4 }), fade: 0.0 },
        Cmd::Filter { set: Some(quillstone_script::Filter { gray: 1.0, ..Default::default() }), fade: 0.0 },
        set(dark(0.6, 0.5), 0.0),
        Cmd::FadeOut { time: 0.0, color: [0.0; 3] },
        Cmd::Flash { time: 1.0, color: [1.0; 3] },
        Cmd::Say { speaker: None, text: "hello".into(), color: None },
    ]);
    let list = v.draw_list(&img, None);
    let at = |f: &dyn Fn(&Draw) -> bool| list.iter().position(f).unwrap();
    let vig = at(&|d| matches!(d, Draw::Vignette { .. }));
    let wash = at(&|d| matches!(d, Draw::Rect { color, .. } if *color == [0.0, 0.0, 1.0, 0.4]));
    let off = at(&|d| matches!(d, Draw::Filter(None)));
    let last_image = list.iter().rposition(|d| matches!(d, Draw::Image { .. })).unwrap();
    let cover = at(&|d| matches!(d, Draw::Rect { color, .. } if *color == [0.0, 0.0, 0.0, 1.0]));
    let flash = at(&|d| matches!(d, Draw::Rect { color, .. } if color[..3] == [1.0; 3]));
    let text = at(&|d| matches!(d, Draw::Text { .. }));
    assert!(last_image < off && off < wash && wash < vig, "{last_image} {off} {wash} {vig}");
    assert!(vig < cover && cover < flash && flash < text, "{vig} {cover} {flash} {text}");
    let screen = Rect::new(0.0, 0.0, VIRTUAL.w, VIRTUAL.h);
    assert!(matches!(&list[vig], Draw::Vignette { rect, color, zoom }
        if *rect == screen && *color == [0.0, 0.0, 0.0, 0.6] && *zoom == crate::pixels::vignette_zoom(0.5)));
    // Screen space: the camera does not move it, and the scene under a menu has it too.
    v.apply(&[Cmd::Camera { camera: Camera { zoom: 3.0, x: 0.1, y: 0.1 }, time: 0.0, ease: Ease::Linear }]);
    assert!(v.draw_list(&img, None).iter().any(|d| matches!(d, Draw::Vignette { rect, .. } if *rect == screen)));
    assert!(v.draw_scene(&img).iter().any(|d| matches!(d, Draw::Vignette { .. })));
    // Gone: no draw at all.
    v.apply(&[Cmd::Vignette { set: None, fade: 0.0 }]);
    assert!(!v.draw_list(&img, None).iter().any(|d| matches!(d, Draw::Vignette { .. })));
}

fn show_a() -> Cmd {
    Cmd::ShowSprite {
        id: "a".into(), src: "a.png".into(), layers: vec![], x: 0.5, y: 1.0, z: 0, scale: 1.0, flip: false, xscale: 1.0, yscale: 1.0,
        alpha: 1.0, dx: 0.0, dy: 0.0, blur: 0.0, tint: [1.0; 3], additive: false, fade: 0.0,
    }
}
