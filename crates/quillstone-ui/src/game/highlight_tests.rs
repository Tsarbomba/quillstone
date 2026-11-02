//! `speaker_highlight`: the game tells the view which character speaks, by the name the line carries.

use super::test_util::{instant_game, line};
use super::*;
use quillstone_render::Size;
use quillstone_script::parse;

const STORY: &str = "::character{id=mika name=Mika}\n::character{id=jo name=Jo}\n::character{id=ann name=Ann}\n\
    ::show{mika src=mika.png x=20%}\n::show{jo src=jo.png x=80%}\n\
    **Mika:** Hi.\n\n**Jo:** Hello.\n\n**Ann:** I am not on stage.\n\nNarration.\n\n**Mika:** Back.\n";

/// The brightness of the mika and jo sprites.
fn shades(g: &Game) -> Vec<f32> {
    let any = |_: &str| Some(Size::new(500.0, 1000.0));
    g.draw_list(&any).into_iter().filter_map(|d| if let Draw::Image { shade, .. } = d { Some(shade) } else { None }).collect()
}

fn game(p: &Program) -> Game<'_> {
    let mut g = instant_game(p);
    g.set_theme(Theme { speaker_highlight: 0.5, ..Theme::default() });
    g
}

#[test]
fn the_speaker_is_found_by_character_and_the_others_dim_also_after_a_rollback() {
    let p = parse(STORY).unwrap();
    let mut g = game(&p);
    g.tick(0.5, false);
    assert_eq!(shades(&g), [1.0, 0.5], "Mika speaks");
    g.handle(Event::Key(Key::Space));
    g.tick(0.5, false);
    assert_eq!(shades(&g), [0.5, 1.0], "Jo speaks");
    g.handle(Event::Key(Key::Space));
    g.tick(0.5, false);
    assert_eq!(shades(&g), [1.0, 1.0], "Ann has no sprite here: nobody dims");
    g.handle(Event::Key(Key::Space));
    g.tick(0.5, false);
    assert_eq!(line(&g), "Narration.");
    assert_eq!(shades(&g), [1.0, 1.0], "narration");
    g.handle(Event::Key(Key::Backspace));
    g.handle(Event::Key(Key::Backspace));
    assert_eq!(shades(&g), [0.5, 1.0], "a rollback to Jo's line is dim at once, no fade");
}

#[test]
fn it_is_off_by_default() {
    let p = parse(STORY).unwrap();
    let mut g = instant_game(&p);
    g.tick(0.5, false);
    assert_eq!(shades(&g), [1.0, 1.0]);
}
