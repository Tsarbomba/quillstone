//! Quick menu buttons drawn as pictures at the places a theme gives them.

use super::test_util::{instant, key, line};
use super::*;
use crate::settings::Settings;
use quillstone_script::parse;

const THREE: &str = "**A:** one\n**A:** two\n**A:** three\n";

fn sized(_: &str) -> Option<Size> {
    Some(Size::new(60.0, 50.0))
}

fn game<'p>(p: &'p Program, theme: &str) -> Game<'p> {
    game_with(p, instant(), theme)
}

fn game_with<'p>(p: &'p Program, settings: Settings, theme: &str) -> Game<'p> {
    let (theme, problems) = Theme::parse(theme);
    assert!(problems.is_empty(), "{problems:?}");
    let mut g = Game::new(p, settings, Box::new(crate::MemoryStorage::default()));
    g.start_story();
    g.set_theme(theme);
    g.draw_list(&sized); // the first frame teaches the game the pictures' sizes
    g
}

fn images(g: &Game) -> Vec<(String, f32)> {
    g.draw_list(&sized)
        .iter()
        .filter_map(|d| match d {
            Draw::Image { src, alpha, .. } if src.ends_with("_i.png") || src.contains("btn") => Some((src.clone(), *alpha)),
            _ => None,
        })
        .collect()
}

fn texts(g: &Game) -> Vec<String> {
    g.draw_list(&sized).into_iter().filter_map(|d| if let Draw::Text { text, .. } = d { Some(text) } else { None }).collect()
}

fn click(g: &mut Game, x: f32, y: f32) {
    g.handle(Event::CursorMoved(Some((x, y))));
    g.handle(Event::Press(Button::Left));
    g.handle(Event::Release(Button::Left));
}

const PLACED: &str = "quick_back_icon = btn_back.png\nquick_back_pos = 100, 100\nquick_settings_icon = btn_set.png\n\
                      quick_settings_pos = 1700, 40\nquick_history_icon = btn_hist.png\n";

#[test]
fn a_placed_picture_button_is_clicked_where_it_is_and_the_row_keeps_the_rest() {
    let p = parse(THREE).unwrap();
    let mut g = game(&p, PLACED);
    assert!(!texts(&g).contains(&"Back".to_string()) && !texts(&g).contains(&"Settings".to_string()));
    assert!(texts(&g).contains(&"Skip".to_string()), "the others are text in the row");
    assert_eq!(images(&g).len(), 3);
    click(&mut g, 130.0, 120.0);
    assert_eq!(line(&g), "one", "Back at the start: nothing to go back to, and it didn't advance either");
    click(&mut g, 900.0, 300.0);
    assert_eq!(line(&g), "two");
    click(&mut g, 130.0, 120.0);
    assert_eq!(line(&g), "one", "Back");
    click(&mut g, 1730.0, 60.0);
    assert_eq!(g.screen(), Screen::Settings);
    assert!(images(&g).is_empty(), "overlays hide the story's quick menu, pictures too");
}

#[test]
fn before_its_size_is_known_a_picture_button_is_a_text_button_and_never_a_phantom() {
    let p = parse(THREE).unwrap();
    let (theme, _) = Theme::parse(PLACED);
    let mut g = Game::new(&p, instant(), Box::new(crate::MemoryStorage::default()));
    g.start_story();
    g.set_theme(theme);
    click(&mut g, 130.0, 120.0); // no draw yet: no sizes; the placed (default-size) rect is still there
    assert_eq!(line(&g), "one");
}

#[test]
fn hover_mode_fades_picture_buttons_in_over_a_placed_button_too() {
    let p = parse(THREE).unwrap();
    let mut g = game(&p, &format!("quick_menu = hover\n{PLACED}"));
    key(&mut g, Key::Space); // Back has somewhere to go
    g.tick(1.0, false);
    assert!(images(&g).is_empty() && !g.needs_ticks(), "idle: nothing drawn");
    g.handle(Event::CursorMoved(Some((130.0, 120.0)))); // on the placed Back, far from the textbox
    assert!(g.needs_ticks());
    g.tick(0.05, false);
    let half = images(&g);
    assert!(!half.is_empty() && half.iter().all(|(_, a)| *a > 0.0 && *a < 1.0), "{half:?}");
    g.tick(1.0, false);
    assert!(images(&g).iter().any(|(s, a)| s == "btn_back.png" && *a == 1.0), "lit under the cursor");
    click(&mut g, 130.0, 120.0);
    assert_eq!(line(&g), "one", "Back worked from its placed spot");
    g.handle(Event::CursorMoved(Some((900.0, 300.0))));
    g.tick(1.0, false);
    assert!(images(&g).is_empty(), "faded out again");
}

#[test]
fn hidden_mode_ui_hide_and_a_hidden_textbox_remove_the_pictures_and_their_clicks() {
    let p = parse(THREE).unwrap();
    let mut g = game(&p, &format!("quick_menu = hidden\n{PLACED}"));
    assert!(images(&g).is_empty());
    click(&mut g, 1730.0, 60.0);
    assert_eq!(g.screen(), Screen::Story, "no click on an invisible picture");
    assert_eq!(line(&g), "two", "it advanced instead");

    let mut g = game(&p, PLACED);
    assert_eq!(images(&g).len(), 3);
    key(&mut g, Key::Char('u'));
    assert!(images(&g).is_empty());
    key(&mut g, Key::Char('u'));
    assert_eq!(images(&g).len(), 3, "the key that brought the UI back was swallowed");
    assert_eq!(line(&g), "one");

    let p2 = parse("**A:** one\n::textbox{hide}\n::wait{time=5}\n**A:** two\n").unwrap();
    let mut g = game(&p2, PLACED);
    key(&mut g, Key::Space);
    g.tick(1.0, false);
    assert!(g.view().textbox_hidden());
    assert!(images(&g).is_empty());
    click(&mut g, 1730.0, 60.0);
    assert_eq!(g.screen(), Screen::Story);
}

#[test]
fn the_pictures_are_asked_for_every_story_frame_even_while_the_menu_is_hidden() {
    let p = parse(THREE).unwrap();
    let theme = format!("quick_menu = hidden\n{PLACED}quick_back_hover = btn_back_h.png\nquick_skip_icon = a_i.png\nquick_skip_on = b_i.png\n");
    let g = game(&p, &theme);
    let asked = std::cell::RefCell::new(Vec::new());
    g.draw_list(&|s: &str| {
        asked.borrow_mut().push(s.to_string());
        sized(s)
    });
    let asked = asked.into_inner();
    for want in ["btn_back.png", "btn_back_h.png", "btn_set.png", "btn_hist.png", "a_i.png", "b_i.png"] {
        assert!(asked.iter().any(|a| a == want), "{want} in {asked:?}");
    }
    // not at the main menu (no story): nothing to keep
    let mut m = Game::new(&p, instant(), Box::new(crate::MemoryStorage::default()));
    m.set_theme(Theme::parse(PLACED).0);
    let seen = std::cell::RefCell::new(Vec::new());
    m.draw_list(&|s: &str| {
        seen.borrow_mut().push(s.to_string());
        None
    });
    assert!(seen.into_inner().iter().all(|s| !s.starts_with("btn")));
    // a theme without any: no asking at all
    let plain = game(&p, "");
    let none = std::cell::Cell::new(0);
    plain.draw_list(&|_| {
        none.set(none.get() + 1);
        None
    });
    assert_eq!(none.get(), 0);
}

#[test]
fn disabled_pictures_are_dimmed_back_at_the_start_and_q_load_without_a_quick_save() {
    let p = parse(THREE).unwrap();
    let mut g = game(&p, "quick_back_icon = btn_back.png\nquick_qload_icon = btn_ql.png\nquick_qload_hover = btn_qlh.png\n");
    let alpha = |g: &Game, s: &str| images(g).into_iter().find(|(n, _)| n == s).map(|(_, a)| a);
    assert_eq!(alpha(&g, "btn_back.png"), Some(0.4));
    assert_eq!(alpha(&g, "btn_ql.png"), Some(0.4));
    key(&mut g, Key::Space);
    assert_eq!(alpha(&g, "btn_back.png"), Some(0.8), "can go back now (idle, no hover picture)");
    assert_eq!(alpha(&g, "btn_ql.png"), Some(0.4));
    key(&mut g, Key::F5);
    assert_eq!(alpha(&g, "btn_ql.png"), Some(1.0), "a quick save exists (this one has a hover picture)");
}

#[test]
fn a_quick_save_from_an_earlier_session_enables_q_load_from_the_start() {
    let p = parse(THREE).unwrap();
    let mut first = Game::new(&p, instant(), Box::new(crate::MemoryStorage::default()));
    first.start_story();
    key(&mut first, Key::F5);
    let mut files = crate::MemoryStorage::default();
    let name = "quick-1.save";
    files.write(name, &first.storage.read(name).expect("the quick save was written")).unwrap();
    let mut g = Game::new(&p, instant(), Box::new(files));
    g.start_story();
    g.set_theme(Theme::parse("quick_qload_icon = btn_ql.png\n").0);
    g.draw_list(&sized);
    assert_eq!(images(&g), vec![("btn_ql.png".to_string(), 0.8)], "enabled: the idle look, not the dimmed one");
}

#[test]
fn skip_and_auto_show_their_on_picture_while_they_run() {
    let p = parse(THREE).unwrap();
    let theme = "quick_skip_icon = btn_s.png\nquick_skip_on = btn_so.png\nquick_auto_icon = btn_a.png\nquick_auto_pos = 50, 50\n";
    let mut g = game_with(&p, Settings { skip_unread: true, ..instant() }, theme);
    let names = |g: &Game| images(g).into_iter().map(|(n, _)| n).collect::<Vec<_>>();
    assert_eq!(names(&g), ["btn_s.png", "btn_a.png"]);
    key(&mut g, Key::Char('a'));
    assert!(g.auto());
    let list = g.draw_list(&sized);
    assert!(list.iter().any(|d| matches!(d, Draw::Rect { color, .. } if *color == g.theme().accent_color)), "bar under Auto");
    click(&mut g, 70.0, 70.0); // the placed Auto toggles it off
    assert!(!g.auto());
    let r = crate::screens::quick_layout(g.textbox(), g.theme(), false, false, &sized).rect(crate::screens::QuickButton::Skip).unwrap();
    click(&mut g, r.x + 5.0, r.y + 5.0);
    assert!(g.skip.is_some());
    assert!(names(&g).contains(&"btn_so.png".to_string()), "{:?}", names(&g));
}

#[test]
fn the_keyboard_shortcuts_do_not_depend_on_the_pictures() {
    let p = parse(THREE).unwrap();
    let mut g = game(&p, &format!("quick_menu = hidden\n{PLACED}"));
    key(&mut g, Key::Space);
    key(&mut g, Key::Backspace);
    assert_eq!(line(&g), "one");
    key(&mut g, Key::Char('h'));
    assert_eq!(g.screen(), Screen::Backlog);
}
