use super::*;

fn speaking(theme: Theme, portrait: Option<&str>) -> View {
    let mut v = View::new().with_theme(theme);
    v.apply(&[Cmd::Say { speaker: Some("Mika".into()), color: None, text: "Hi.".into() }]);
    v.set_portrait(portrait.map(str::to_string));
    v
}

/// A 200x400 face: tall, so the rectangle's width decides nothing and its height everything.
fn list(v: &View) -> Vec<Draw> {
    v.draw_list(&|_| Some(Size::new(200.0, 400.0)), None)
}

fn faces(list: &[Draw]) -> Vec<(Rect, f32)> {
    list.iter().filter_map(|d| if let Draw::Image { dest, alpha, .. } = d { Some((*dest, *alpha)) } else { None }).collect()
}

#[test]
fn the_default_portrait_is_a_square_on_the_textbox_left_end_and_the_picture_stands_in_it_whole() {
    let theme = Theme::default();
    assert_eq!(portrait_rect(&theme), Rect::new(160.0, 460.0, 320.0, 320.0));
    let l = list(&speaking(theme, Some("mika.png")));
    // 200x400 contained in 320x320: 160x320, in the middle, on the bottom edge.
    assert_eq!(faces(&l), [(Rect::new(240.0, 460.0, 160.0, 320.0), 1.0)]);
}

#[test]
fn the_portrait_is_drawn_after_the_textbox_panel_and_before_the_name_and_the_line() {
    let l = list(&speaking(Theme::default(), Some("mika.png")));
    let at = |f: &dyn Fn(&Draw) -> bool| l.iter().position(f).unwrap();
    let panel = at(&|d| matches!(d, Draw::Rect { rect, .. } if *rect == TEXTBOX));
    let face = at(&|d| matches!(d, Draw::Image { .. }));
    let first_text = at(&|d| matches!(d, Draw::Text { .. }));
    assert!(panel < face && face < first_text, "{panel} {face} {first_text}");
}

#[test]
fn narration_and_a_speaker_without_a_portrait_draw_none() {
    let mut narrated = speaking(Theme::default(), None);
    narrated.apply(&[Cmd::Say { speaker: None, color: None, text: "Rain.".into() }]);
    narrated.set_portrait(None);
    assert!(faces(&list(&narrated)).is_empty());
    assert!(faces(&list(&speaking(Theme::default(), None))).is_empty());
}

#[test]
fn a_portrait_rect_moves_the_portrait() {
    let theme = Theme { portrait_rect: Some([1000.0, 100.0, 400.0, 200.0]), ..Theme::default() };
    // 200x400 in 400x200: 100x200, centred, on the bottom edge.
    assert_eq!(faces(&list(&speaking(theme, Some("mika.png")))), [(Rect::new(1150.0, 100.0, 100.0, 200.0), 1.0)]);
}

#[test]
fn the_default_portrait_follows_a_moved_textbox() {
    let theme = Theme { textbox_rect: [300.0, 600.0, 1000.0, 300.0], ..Theme::default() };
    assert_eq!(portrait_rect(&theme), Rect::new(300.0, 280.0, 320.0, 320.0));
}

#[test]
fn a_hidden_textbox_hides_the_portrait_and_a_fading_one_fades_it() {
    let mut v = speaking(Theme::default(), Some("mika.png"));
    v.set_hide_ui(true);
    assert!(faces(&list(&v)).is_empty(), "the U key");
    v.set_hide_ui(false);
    v.apply(&[Cmd::Textbox { hidden: true, fade: 0.0 }]);
    assert!(faces(&list(&v)).is_empty(), "::textbox{{hide}}");
    v.apply(&[Cmd::Textbox { hidden: false, fade: 1.0 }]);
    v.tick(0.5);
    assert_eq!(faces(&list(&v)), [(Rect::new(240.0, 460.0, 160.0, 320.0), 0.5)], "half way back");
}
