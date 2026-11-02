use super::*;

fn stage(highlight: f32) -> View {
    let mut v = View::new().with_theme(Theme { speaker_highlight: highlight, ..Theme::default() });
    let sprite = |id: &str, x: f32| Cmd::ShowSprite { id: id.into(), src: format!("{id}.png"), layers: vec![], x, y: 1.0, z: 0, scale: 1.0, flip: false, xscale: 1.0, yscale: 1.0, fade: 0.0 };
    v.apply(&[sprite("mika", 0.2), sprite("jo", 0.8)]);
    v
}

/// The brightness each sprite is drawn at, in the order of `mika`, `jo`.
fn shades(v: &View) -> Vec<f32> {
    let list = v.draw_list(&|_| Some(Size::new(500.0, 1000.0)), None);
    list.iter().filter_map(|d| if let Draw::Image { shade, .. } = d { Some(*shade) } else { None }).collect()
}

#[test]
fn the_others_dim_while_a_character_on_stage_speaks() {
    let mut v = stage(0.6);
    assert_eq!(shades(&v), [1.0, 1.0], "nobody speaks yet");
    v.set_speaker(Some("mika".into()), true);
    assert_eq!(shades(&v), [1.0, 0.6]);
    v.set_speaker(Some("jo".into()), true);
    assert_eq!(shades(&v), [0.6, 1.0]);
}

#[test]
fn narration_and_a_speaker_without_a_sprite_dim_nobody() {
    let mut v = stage(0.6);
    v.set_speaker(None, true);
    assert_eq!(shades(&v), [1.0, 1.0]);
    v.set_speaker(Some("narrator".into()), true);
    assert_eq!(shades(&v), [1.0, 1.0]);
    v.apply(&[Cmd::HideSprite { id: "mika".into(), fade: 5.0 }]);
    v.set_speaker(Some("mika".into()), true);
    assert_eq!(shades(&v)[1], 1.0, "a sprite on its way out is not on stage");
}

#[test]
fn it_is_off_unless_the_theme_asks() {
    let mut v = stage(1.0);
    v.set_speaker(Some("mika".into()), true);
    assert_eq!(shades(&v), [1.0, 1.0]);
}

#[test]
fn a_change_of_speaker_fades_over_a_short_time_and_settle_finishes_it() {
    let mut v = stage(0.6);
    v.set_speaker(Some("mika".into()), true);
    v.tick(0.01); // no longer new
    v.set_speaker(Some("jo".into()), false);
    assert_eq!(shades(&v), [1.0, 0.6], "not yet");
    v.tick(SHADE_TIME / 2.0);
    let [mika, jo] = shades(&v)[..] else { panic!() };
    assert!(mika < 1.0 && mika > 0.6 && jo > 0.6 && jo < 1.0, "{mika} {jo}");
    v.tick(SHADE_TIME);
    assert_eq!(shades(&v), [0.6, 1.0]);
    v.set_speaker(Some("mika".into()), false);
    v.settle(10.0);
    assert_eq!(shades(&v), [1.0, 0.6]);
}

#[test]
fn a_sprite_that_has_just_appeared_is_dim_at_once_and_a_redraw_is_never_a_fade() {
    let mut v = stage(0.6);
    v.set_speaker(Some("mika".into()), false);
    assert_eq!(shades(&v), [1.0, 0.6], "both new: no ramp from full brightness");
    v.tick(1.0);
    v.apply(&[Cmd::ClearScene, Cmd::ShowSprite { id: "mika".into(), src: "mika.png".into(), layers: vec![], x: 0.2, y: 1.0, z: 0, scale: 1.0, flip: false, xscale: 1.0, yscale: 1.0, fade: 0.0 }]);
    v.set_speaker(Some("jo".into()), true);
    assert_eq!(shades(&v), [1.0], "jo is not on stage after the redraw: nobody dims");
}
