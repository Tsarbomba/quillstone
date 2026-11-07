//! `::filter` in the view: it eases like a tint, ends in nothing, and brackets the stage in the draw list.

use super::*;
use quillstone_script::Filter;

fn img(_: &str) -> Option<Size> {
    Some(Size::new(500.0, 1000.0))
}

fn gray(amount: f32) -> Filter {
    Filter { gray: amount, ..Filter::default() }
}

fn set(f: Filter, fade: f32) -> Cmd {
    Cmd::Filter { set: Some(f), fade }
}

fn bg(src: &str) -> Cmd {
    Cmd::SetBackground { src: src.into(), fade: 0.0, wipe: None, sound: false, blur: 0.0, mask: None }
}

fn tint() -> Cmd {
    Cmd::Tint { set: Some(quillstone_script::Tint { color: [0.0, 0.0, 1.0], alpha: 0.4 }), fade: 0.0 }
}

#[test]
fn it_eases_from_none_then_from_where_it_is_and_out_to_nothing() {
    let mut v = View::new();
    v.apply(&[set(gray(1.0), 2.0)]);
    assert!(v.busy(), "the fade keeps the player redrawing");
    assert_eq!(v.filter(), None, "starts neutral");
    v.tick(1.0);
    assert!((v.filter().unwrap().gray - 0.5).abs() < 1e-5);
    // A new filter mid-fade eases from the current values and replaces the rest.
    v.apply(&[set(Filter { sepia: 1.0, ..Filter::default() }, 1.0)]);
    v.tick(0.5);
    let f = v.filter().unwrap();
    assert!((f.gray - 0.25).abs() < 1e-5 && (f.sepia - 0.5).abs() < 1e-5, "{f:?}");
    v.tick(1.0);
    assert_eq!(v.filter(), Some(Filter { sepia: 1.0, ..Filter::default() }));
    assert!(!v.busy());
    v.apply(&[Cmd::Filter { set: None, fade: 1.0 }]);
    v.tick(0.5);
    assert!((v.filter().unwrap().sepia - 0.5).abs() < 1e-5);
    v.tick(0.6);
    assert_eq!(v.filter(), None);
    assert!(!v.busy());
    // Removing nothing, and a set without a fade, are at once.
    v.apply(&[Cmd::Filter { set: None, fade: 1.0 }]);
    assert!(!v.busy());
    v.apply(&[set(gray(1.0), 0.0)]);
    assert_eq!(v.filter(), Some(gray(1.0)));
    v.apply(&[Cmd::ClearScene]);
    assert_eq!(v.filter(), None, "a new scene clears it");
}

#[test]
fn the_stage_is_bracketed_and_the_tint_and_ui_are_outside() {
    let mut v = View::new();
    v.apply(&[bg("b.png"), tint(), set(gray(1.0), 0.0), Cmd::ShowImage { src: "i.png".into(), alt: String::new() }]);
    let list = v.draw_list(&img, None);
    let at = |f: &dyn Fn(&Draw) -> bool| list.iter().position(f).unwrap();
    let on = at(&|d| matches!(d, Draw::Filter(Some(_))));
    let off = at(&|d| matches!(d, Draw::Filter(None)));
    let bg_at = at(&|d| matches!(d, Draw::Image { src, .. } if src == "b.png"));
    let inline = at(&|d| matches!(d, Draw::Image { src, .. } if src == "i.png"));
    let wash = at(&|d| matches!(d, Draw::Rect { color, .. } if *color == [0.0, 0.0, 1.0, 0.4]));
    assert!(on < bg_at && inline < off && off < wash, "{on} {bg_at} {inline} {off} {wash}");
    assert_eq!(list.iter().filter(|d| matches!(d, Draw::Filter(_))).count(), 2);
    // No filter: no markers at all.
    v.apply(&[Cmd::Filter { set: None, fade: 0.0 }]);
    assert!(!v.draw_list(&img, None).iter().any(|d| matches!(d, Draw::Filter(_))));
    // A filter that changes nothing is none.
    v.apply(&[set(Filter::default(), 0.0)]);
    assert!(!v.draw_list(&img, None).iter().any(|d| matches!(d, Draw::Filter(_))));
}
