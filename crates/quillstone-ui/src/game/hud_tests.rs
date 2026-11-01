//! The HUD in the game shell: live text, where it is drawn and where it is not, and the demo.

use super::*;
use quillstone_render::Size;
use quillstone_script::parse;

fn new_game<'p>(p: &'p Program) -> Game<'p> {
    let mut g = Game::new(p, Settings { text_speed: 0.0, ..Settings::default() }, Box::new(crate::MemoryStorage::default()));
    g.start_story();
    g
}

fn key(g: &mut Game, k: Key) {
    g.handle(Event::Key(k));
}

fn has_text(list: &[Draw], what: &str) -> bool {
    list.iter().any(|d| matches!(d, Draw::Text { text, .. } if text == what))
}

fn none(_: &str) -> Option<Size> {
    None
}

const STORY: &str = "\
---
vars: { gold: 1, hp: 8 }
---
::hud{id=gold text=\"Gold {gold}\" x=0.9 y=0.05}
::hud{id=hp bar=hp max=10 size=200,20 color=#ff0000 back=#000000}
::hud{id=low text=LOW x=0.1 y=0.2 if=\"hp < 4\"}
Start.
::set{gold += 4, hp -= 6}
Next.
::choice
- [Pay](#pay){gold -= 3}
- [Keep](#keep)
::
# pay
Paid.
::end
# keep
Kept.
";

fn fill(list: &[Draw]) -> Option<f32> {
    list.iter().find_map(|d| match d {
        Draw::Rect { rect, color } if color[0] == 1.0 && color[1] == 0.0 && color[2] == 0.0 => Some(rect.w),
        _ => None,
    })
}

#[test]
fn the_draw_list_follows_the_variables() {
    let p = parse(STORY).unwrap();
    let mut g = new_game(&p);
    let list = g.draw_list(&none);
    assert!(has_text(&list, "Gold 1") && !has_text(&list, "LOW"));
    assert_eq!(fill(&list), Some(160.0));
    key(&mut g, Key::Space);
    let list = g.draw_list(&none);
    assert!(has_text(&list, "Gold 5") && has_text(&list, "LOW"), "updated, and the condition came true");
    assert_eq!(fill(&list), Some(40.0));
    key(&mut g, Key::Space); // on to the choices
    key(&mut g, Key::Char('1'));
    assert!(has_text(&g.draw_list(&none), "Gold 2"), "a choice's effect shows too");
    // rolling back puts the old numbers back
    for _ in 0..4 {
        key(&mut g, Key::Backspace);
    }
    assert!(has_text(&g.draw_list(&none), "Gold 1"));
    assert_eq!(fill(&g.draw_list(&none)), Some(160.0));
    key(&mut g, Key::Space);
    assert!(has_text(&g.draw_list(&none), "Gold 5"), "and forward again");
}

#[test]
fn the_hud_is_under_a_confirm_but_not_under_full_screen_screens() {
    let p = parse(STORY).unwrap();
    let mut g = new_game(&p);
    g.ask("Sure?", Then::Quit);
    let list = g.draw_list(&none);
    assert!(has_text(&list, "Sure?") && has_text(&list, "Gold 1"), "the story, HUD included, stays under a confirm");
    type Open = fn(&mut Game);
    let opens: [(&str, Open); 4] = [
        ("settings", |g| key(g, Key::Escape)),
        ("history", |g| key(g, Key::Char('h'))),
        ("save", |g| key(g, Key::Char('s'))),
        ("load", |g| key(g, Key::Char('l'))),
    ];
    for (name, open) in opens {
        let mut g = new_game(&p);
        assert!(has_text(&g.draw_list(&none), "Gold 1"));
        open(&mut g);
        assert_ne!(g.screen(), Screen::Story, "{name}");
        let list = g.draw_list(&none);
        assert!(!has_text(&list, "Gold 1") && !has_text(&list, "LOW") && fill(&list).is_none(), "{name}: no HUD");
        key(&mut g, Key::Escape);
        if g.screen() != Screen::Story {
            key(&mut g, Key::Escape);
        }
        assert!(has_text(&g.draw_list(&none), "Gold 1"), "{name}: and back");
    }
}

#[test]
fn the_input_and_inventory_screens_have_no_hud() {
    let p = parse("---\nvars: { name: \"A\", gold: 3, bag: [] }\ninventory: bag\n---\n::hud{id=g text=\"Gold {gold}\"}\n::input{var=name prompt=\"Name?\"}\n::inventory\nDone.\n").unwrap();
    let mut g = new_game(&p);
    assert_eq!(g.screen(), Screen::Input);
    assert!(!has_text(&g.draw_list(&none), "Gold 3"));
    g.handle(Event::Text("Bo".into()));
    key(&mut g, Key::Enter);
    assert_eq!(g.screen(), Screen::Inventory);
    assert!(!has_text(&g.draw_list(&none), "Gold 3"));
}

#[test]
fn the_u_key_hides_the_hud_with_the_rest_of_the_ui_and_the_next_key_brings_it_back() {
    let p = parse(STORY).unwrap();
    let mut g = new_game(&p);
    key(&mut g, Key::Char('u'));
    let list = g.draw_list(&none);
    assert!(!has_text(&list, "Gold 1") && fill(&list).is_none() && !has_text(&list, "Start."));
    key(&mut g, Key::Space); // swallowed: restores
    let list = g.draw_list(&none);
    assert!(has_text(&list, "Gold 1") && has_text(&list, "Start."));
    assert_eq!(fill(&list), Some(160.0));
}

#[test]
fn textbox_hide_does_not_hide_the_hud() {
    let p = parse("::hud{id=g text=Gold}\n::textbox{hide}\n::wait{time=5}\n").unwrap();
    let g = new_game(&p);
    assert!(g.view().textbox_hidden());
    assert!(has_text(&g.draw_list(&none), "Gold"));
}

#[test]
fn the_main_menu_has_no_hud_and_continuing_brings_it_back() {
    let p = parse(STORY).unwrap();
    let mut g = Game::new(&p, Settings { text_speed: 0.0, ..Settings::default() }, Box::new(crate::MemoryStorage::default()));
    assert_eq!(g.screen(), Screen::MainMenu);
    assert!(!has_text(&g.draw_list(&none), "Gold 1"));
    g.start_story();
    key(&mut g, Key::Space);
    assert!(has_text(&g.draw_list(&none), "Gold 5"));
    key(&mut g, Key::F5); // quick save
    key(&mut g, Key::Space); // ... more story
    key(&mut g, Key::F9); // quick load
    assert!(has_text(&g.draw_list(&none), "Gold 5"), "the HUD comes back with a load");
    let saved = g.state().clone();
    assert_eq!(saved.hud.len(), 3);
}

#[test]
fn a_hot_reload_keeps_the_elements_and_picks_up_the_edited_ones() {
    let p = parse(STORY).unwrap();
    let mut g = new_game(&p);
    key(&mut g, Key::Space);
    let edited = parse(&STORY.replace("Gold {gold}", "Coins {gold}")).unwrap();
    g.reload(&edited).unwrap();
    assert!(has_text(&g.draw_list(&none), "Gold 5") || has_text(&g.draw_list(&none), "Coins 5"), "{:?}", g.state().hud);
}

#[test]
fn skipping_does_not_wait_for_a_fade() {
    let p = parse("::hud{id=g text=Gold fade=5}\nOne.\nTwo.\n").unwrap();
    let mut g = new_game(&p);
    assert!(!has_text(&g.draw_list(&none), "Gold"), "fading in");
    g.tick(1.0, false);
    let list = g.draw_list(&none);
    assert!(list.iter().any(|d| matches!(d, Draw::Text { text, color, .. } if text == "Gold" && color[3] > 0.1 && color[3] < 0.5)));
    assert!(g.needs_ticks(), "a fade keeps the frontend drawing");
}

#[test]
fn the_hud_demo_plays_to_the_end_with_live_numbers() {
    let p = parse(include_str!("../../../../examples/hud.quill.md")).unwrap();
    let mut g = new_game(&p);
    // the title card sits on black: the text over the cover colour, before the textbox
    for _ in 0..10 {
        g.tick(0.5, false);
    }
    // the card, two waits and a hide later the roof scene is up with every element
    for _ in 0..40 {
        if g.view().dialogue.is_some() {
            break;
        }
        g.tick(0.5, false);
    }
    g.tick(1.0, false); // the elements fade in over half a second
    let list = g.draw_list(&none);
    assert!(has_text(&list, "Gold: 5"), "{:?}", g.state().hud);
    let demo_fill = |l: &[Draw]| l.iter().find_map(|d| match d { Draw::Rect { rect, color } if (color[0] - 0.8).abs() < 0.01 && color[1] < 0.3 => Some(rect.w), _ => None });
    assert_eq!(demo_fill(&list), Some(320.0), "full");
    assert!(!has_text(&list, "LOW HP"));
    while g.view().choices.is_empty() {
        key(&mut g, Key::Space); // to the camp choices
    }
    let pick = |g: &mut Game, label: &str| {
        let i = g.view().choices.iter().position(|c| c.starts_with(label)).unwrap_or_else(|| panic!("{label} in {:?}", g.view().choices));
        key(g, Key::Char(char::from_digit(i as u32 + 1, 10).unwrap()));
        for _ in 0..6 {
            if !g.view().choices.is_empty() || g.view().finished {
                break;
            }
            key(g, Key::Space);
        }
    };
    pick(&mut g, "Find a coin");
    assert!(has_text(&g.draw_list(&none), "Gold: 8"));
    pick(&mut g, "Fight");
    assert_eq!(demo_fill(&g.draw_list(&none)), Some(192.0), "6 of 10");
    pick(&mut g, "Fight");
    assert_eq!(demo_fill(&g.draw_list(&none)), Some(64.0), "2 of 10");
    assert!(has_text(&g.draw_list(&none), "LOW HP"), "the warning appears below 3 hit points");
    pick(&mut g, "Rest");
    assert!(has_text(&g.draw_list(&none), "Gold: 7"));
    assert!(!has_text(&g.draw_list(&none), "LOW HP"), "healed above the limit: the warning is gone");
    g.handle(Event::CursorMoved(None));
    let i = g.view().choices.iter().position(|c| c.starts_with("Leave")).unwrap();
    key(&mut g, Key::Char(char::from_digit(i as u32 + 1, 10).unwrap()));
    assert!(has_text(&g.draw_list(&none), "Gold: 7 (final)"), "the same id replaced the old element");
    key(&mut g, Key::Space);
    for _ in 0..4 {
        g.tick(0.5, false);
    }
    let list = g.draw_list(&none);
    assert!(!has_text(&list, "Gold: 7 (final)"), "cleared at the end");
    assert!(g.state().hud.is_empty());
}

// ---- clickable elements (`call=` / `open=`) ----

const PHONE: &str = "\
---
vars: { calls: 0, quests: [] }
journal: quests
---
::hud{id=phone image=assets/phone.png call=#phone hover=assets/phone_hover.png x=0.9 y=0.1}
::hud{id=log text=Log open=journal x=0 y=0}
# start
**A:** one
**A:** two
::choice
- [Go](#next)
::
# next
::wait{time=5}
**A:** after
::end
# phone
::set{calls += 1}
**A:** Ring.
::return
";

/// The phone is 100 x 100 at x=0.9, y=0.1: (1638, 98) to (1738, 198). "Log" is at the top left corner.
const ON_PHONE: Option<(f32, f32)> = Some((1688.0, 148.0));
const ON_LOG: Option<(f32, f32)> = Some((5.0, 5.0));

fn sized(_: &str) -> Option<Size> {
    Some(Size::new(100.0, 100.0))
}

fn click(g: &mut Game, at: Option<(f32, f32)>) {
    g.draw_list(&sized); // the frontend's draw is what tells the game how big the pictures are
    g.handle(Event::CursorMoved(at));
    g.handle(Event::Press(Button::Left));
    g.handle(Event::Release(Button::Left));
}

fn said(g: &Game) -> String {
    g.view().dialogue.as_ref().map(|d| d.text.clone()).unwrap_or_default()
}

fn calls(g: &Game) -> i64 {
    g.state().vars["calls"].as_int().unwrap()
}

/// The phone picture's file and opacity as drawn.
fn phone(g: &Game) -> (String, f32) {
    g.draw_list(&sized)
        .iter()
        .find_map(|d| match d {
            Draw::Image { src, alpha, .. } if src.contains("phone") => Some((src.clone(), *alpha)),
            _ => None,
        })
        .expect("the phone is drawn")
}

#[test]
fn a_click_on_a_hud_button_calls_the_label_and_the_line_comes_back_without_advancing() {
    let p = parse(PHONE).unwrap();
    let mut g = new_game(&p);
    assert_eq!(said(&g), "one");
    click(&mut g, ON_PHONE);
    assert_eq!((said(&g).as_str(), calls(&g)), ("Ring.", 1));
    key(&mut g, Key::Space);
    assert_eq!(said(&g), "one", "the click did not move the story on, and the label returned to the same line");
    key(&mut g, Key::Space);
    assert_eq!(said(&g), "two");
}

#[test]
fn a_hud_button_is_hit_where_it_is_drawn_and_shows_its_hover_picture_under_the_mouse() {
    let p = parse(PHONE).unwrap();
    let mut g = new_game(&p);
    assert_eq!(phone(&g).0, "assets/phone.png");
    g.draw_list(&sized);
    g.handle(Event::CursorMoved(ON_PHONE));
    assert_eq!(phone(&g).0, "assets/phone_hover.png");
    g.handle(Event::CursorMoved(Some((1700.0, 400.0))));
    assert_eq!(phone(&g).0, "assets/phone.png");
    // one pixel outside the picture is only a click on the story
    click(&mut g, Some((1637.0, 148.0)));
    assert_eq!((said(&g).as_str(), calls(&g)), ("two", 0));
    click(&mut g, Some((1639.0, 148.0)));
    assert_eq!(calls(&g), 1);
    // a text button lights up under the mouse
    g.handle(Event::CursorMoved(None));
    let lit = |g: &Game| g.draw_list(&sized).iter().any(|d| matches!(d, Draw::Rect { color, .. } if *color == g.view().theme().button_hover_color));
    assert!(!lit(&g));
    g.handle(Event::CursorMoved(ON_LOG));
    assert!(lit(&g));
}

#[test]
fn a_hud_button_is_dimmed_and_does_nothing_over_a_choice_or_a_wait() {
    let p = parse(PHONE).unwrap();
    let mut g = new_game(&p);
    assert_eq!(phone(&g).1, 1.0);
    key(&mut g, Key::Space);
    key(&mut g, Key::Space);
    assert!(matches!(g.state().pending, Some(Cmd::ShowChoices { .. })));
    assert_eq!(phone(&g).1, 0.45, "dimmed over choices");
    click(&mut g, ON_PHONE);
    assert_eq!((calls(&g), g.screen()), (0, Screen::Story));
    assert!(matches!(g.state().pending, Some(Cmd::ShowChoices { .. })), "the click did not pick or advance");
    assert_eq!(g.notice.as_ref().map(|(m, _)| m.as_str()), Some("Can't use that now"));
    g.pick(0);
    assert!(matches!(g.state().pending, Some(Cmd::Wait { .. })));
    assert_eq!(phone(&g).1, 0.45, "dimmed over a wait");
    click(&mut g, ON_PHONE);
    click(&mut g, ON_LOG);
    assert_eq!((calls(&g), g.screen()), (0, Screen::Story), "inert, `open=` buttons too");
}

#[test]
fn an_open_button_opens_the_screen_like_its_key_and_does_not_advance() {
    let p = parse(PHONE).unwrap();
    let mut g = new_game(&p);
    click(&mut g, ON_LOG);
    assert!(g.screen() == Screen::Inventory && g.bag.journal, "the journal");
    key(&mut g, Key::Char('j'));
    assert_eq!((g.screen(), said(&g).as_str()), (Screen::Story, "one"));
    let p = parse(&PHONE.replace("open=journal", "open=settings")).unwrap();
    let mut g = new_game(&p);
    click(&mut g, ON_LOG);
    assert_eq!(g.screen(), Screen::Settings);
}
