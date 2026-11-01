//! Esc and a right click open the Save screen, and the game-menu tabs along the top move between
//! Save, Load, Settings and History (and ask before Main menu and Quit).

use super::test_util::{click, instant, key, new_game};
use super::*;
use quillstone_script::parse;

const STORY: &str = "**A:** one\n**A:** two\n";
const INVENTORY: &str = "---\ninventory: bag\nvars: { bag: [key] }\n---\n::item{id=key name=\"Key\" text=\"Opens it.\"}\n**A:** one\n::inventory\n**A:** two\n";

fn tab(g: &mut Game, t: GameTab) {
    let tabs = game_tabs(g.in_story());
    let i = tabs.iter().position(|x| *x == t).expect("the tab is offered");
    click(g, game_tab_rect(i, tabs.len()));
}

#[test]
fn escape_and_right_click_in_the_story_open_save_and_close_back_to_it() {
    let p = parse(STORY).unwrap();
    for how in 0..2 {
        let mut g = new_game(&p, instant());
        let open = |g: &mut Game| if how == 0 { key(g, Key::Escape) } else { g.handle(Event::Press(Button::Right)) };
        open(&mut g);
        assert_eq!(g.screen(), Screen::Save, "how {how}");
        open(&mut g); // the same key closes it
        assert_eq!(g.screen(), Screen::Story, "how {how}");
    }
}

#[test]
fn escape_and_right_click_over_a_scripted_inventory_open_save_and_return_to_it() {
    let p = parse(INVENTORY).unwrap();
    for how in 0..2 {
        let mut g = new_game(&p, instant());
        key(&mut g, Key::Space);
        assert_eq!(g.screen(), Screen::Inventory);
        if how == 0 { key(&mut g, Key::Escape) } else { g.handle(Event::Press(Button::Right)) }
        assert_eq!(g.screen(), Screen::Save, "how {how}");
        // Settings through the tab and Esc from there still lands back at the inventory
        tab(&mut g, GameTab::Settings);
        assert_eq!(g.screen(), Screen::Settings);
        key(&mut g, Key::Escape);
        assert_eq!(g.screen(), Screen::Inventory, "how {how}");
    }
}

#[test]
fn the_tabs_switch_screens_with_the_mouse_and_the_highlight_follows() {
    let p = parse(STORY).unwrap();
    let mut g = new_game(&p, instant());
    key(&mut g, Key::Escape);
    for (t, screen) in [(GameTab::Load, Screen::Load), (GameTab::Settings, Screen::Settings), (GameTab::History, Screen::Backlog), (GameTab::Save, Screen::Save)] {
        tab(&mut g, t);
        assert_eq!(g.screen(), screen);
        assert_eq!(g.current_tab(), Some(t));
    }
    // the current tab is drawn in the accent colour
    let list = g.draw_list(&|_| None);
    let accent = g.theme().accent_color;
    let r = game_tab_rect(0, 6);
    assert!(list.iter().any(|d| matches!(d, Draw::Text { text, rect, color, .. } if text == "Save" && *rect == r && *color == accent)));
    tab(&mut g, GameTab::Settings);
    key(&mut g, Key::Escape);
    assert_eq!(g.screen(), Screen::Story, "closing from a tab goes back to the story");
}

#[test]
fn tab_walks_through_the_four_screens_and_round_again() {
    let p = parse(STORY).unwrap();
    let mut g = new_game(&p, instant());
    key(&mut g, Key::Escape);
    let seen: Vec<Screen> = (0..5)
        .map(|_| {
            key(&mut g, Key::Tab);
            g.screen()
        })
        .collect();
    assert_eq!(seen, [Screen::Load, Screen::Settings, Screen::Backlog, Screen::Save, Screen::Load]);
    key(&mut g, Key::Escape);
    assert_eq!(g.screen(), Screen::Story);
}

#[test]
fn leaving_settings_by_a_tab_saves_what_changed() {
    let p = parse(STORY).unwrap();
    let mut g = new_game(&p, instant());
    key(&mut g, Key::Escape);
    tab(&mut g, GameTab::Settings);
    g.take_requests();
    key(&mut g, Key::Down);
    key(&mut g, Key::Right); // the auto-forward delay, the second row
    tab(&mut g, GameTab::Load);
    assert_eq!(g.take_requests(), vec![Request::SaveSettings]);
}

#[test]
fn the_main_menu_and_quit_tabs_ask_first() {
    let p = parse(STORY).unwrap();
    for screen_tab in [GameTab::Save, GameTab::Load, GameTab::Settings, GameTab::History] {
        for (t, then) in [(GameTab::MainMenu, Then::MainMenu), (GameTab::Quit, Then::Quit)] {
            let mut g = new_game(&p, instant());
            key(&mut g, Key::Escape);
            tab(&mut g, screen_tab);
            let shown = g.screen();
            tab(&mut g, t);
            assert_eq!(g.confirm.as_ref().map(|c| c.then), Some(then), "{screen_tab:?} {t:?}");
            assert!(g.take_requests().is_empty() && g.in_story());
            key(&mut g, Key::Char('n'));
            assert_eq!(g.screen(), shown, "No comes back to the screen it was asked from");
            tab(&mut g, t);
            key(&mut g, Key::Char('y'));
            match then {
                Then::Quit => assert!(g.take_requests().contains(&Request::Quit)),
                _ => assert_eq!(g.screen(), Screen::MainMenu),
            }
        }
    }
}

#[test]
fn from_the_main_menu_there_is_no_save_history_or_main_menu_tab() {
    let p = parse(STORY).unwrap();
    let mut g = Game::new(&p, instant(), Box::new(crate::MemoryStorage::default()));
    assert_eq!(game_tabs(false), [GameTab::Load, GameTab::Settings, GameTab::Quit]);
    g.open_slots(Screen::Load);
    let labels = |g: &Game| -> Vec<String> {
        g.draw_list(&|_| None).into_iter().filter_map(|d| if let Draw::Text { text, .. } = d { Some(text) } else { None }).collect()
    };
    let shown = labels(&g);
    assert!(shown.contains(&"Load".to_string()) && shown.contains(&"Quit".to_string()));
    assert!(!shown.contains(&"Save".to_string()) && !shown.contains(&"Main menu".to_string()), "{shown:?}");
    // Tab goes between the two screens it has, and a click where Save would be does nothing
    key(&mut g, Key::Tab);
    assert_eq!(g.screen(), Screen::Settings);
    key(&mut g, Key::Tab);
    assert_eq!(g.screen(), Screen::Load);
    tab(&mut g, GameTab::Settings);
    assert_eq!(g.screen(), Screen::Settings);
    key(&mut g, Key::Escape);
    assert_eq!(g.screen(), Screen::MainMenu);
    // Quit from the strip leaves at once: nothing to lose at the menu
    g.open_slots(Screen::Load);
    tab(&mut g, GameTab::Quit);
    assert!(g.take_requests().contains(&Request::Quit));
}

#[test]
fn escape_in_a_replay_opens_settings_where_main_menu_leaves_it() {
    // a replay has nothing to save, so the Save screen would only say "not in a replay"
    let p = parse(STORY).unwrap();
    let mut g = new_game(&p, instant());
    g.replay = Some(super::replay::ReplayRun { label: "x".into(), persistent: Default::default() });
    key(&mut g, Key::Escape);
    assert_eq!(g.screen(), Screen::Settings);
}
