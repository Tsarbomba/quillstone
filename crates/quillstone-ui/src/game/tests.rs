use super::*;
use quillstone_render::Rect;
use quillstone_script::parse;

/// A game past its main menu, at the start of the story.
fn new_game<'p>(p: &'p Program, settings: Settings) -> Game<'p> {
    let mut g = Game::new(p, settings, Box::new(crate::MemoryStorage::default()));
    g.start_story();
    g
}

fn instant() -> Settings {
    Settings { text_speed: 0.0, ..Settings::default() }
}

fn line(g: &Game) -> String {
    g.view().dialogue.as_ref().map(|d| d.text.clone()).unwrap_or_default()
}

fn centre(r: Rect) -> Option<(f32, f32)> {
    Some((r.x + r.w / 2.0, r.y + r.h / 2.0))
}

fn click_at(g: &mut Game, at: Option<(f32, f32)>) {
    g.handle(Event::CursorMoved(at));
    g.handle(Event::Press(Button::Left));
    g.handle(Event::Release(Button::Left));
}

fn key(g: &mut Game, k: Key) {
    g.handle(Event::Key(k));
}

const THREE: &str = "**A:** one\n**A:** two\n**A:** three\n";
const CHOICE: &str = "**A:** pick\n::choice\n- [Left](#l)\n- [Right](#r)\n::\n# l\n**A:** went left\n::end\n# r\n**A:** went right\n";

// ---- the story ----------------------------------------------------------------

#[test]
fn a_touchpad_swipe_rolls_back_by_notches_not_by_events() {
    let p = parse(THREE).unwrap();
    let mut g = new_game(&p, instant());
    key(&mut g, Key::Space);
    key(&mut g, Key::Space);
    assert_eq!(line(&g), "three");
    for _ in 0..8 {
        g.handle(Event::Wheel(0.15)); // 1.2 notches in eight small steps
    }
    assert_eq!(line(&g), "two", "one step back, not eight");
    g.handle(Event::Wheel(-0.5)); // the other way: the leftover 0.2 is dropped
    g.handle(Event::Wheel(0.9));
    assert_eq!(line(&g), "two");
    g.handle(Event::Wheel(0.1));
    assert_eq!(line(&g), "one");
}

#[test]
fn wheel_down_rolls_forward_after_wheel_up() {
    let p = parse(THREE).unwrap();
    let mut g = new_game(&p, instant());
    key(&mut g, Key::Space);
    key(&mut g, Key::Space);
    g.handle(Event::Wheel(1.0));
    g.handle(Event::Wheel(1.0));
    assert_eq!(line(&g), "one");
    g.handle(Event::Wheel(-1.0));
    assert_eq!(line(&g), "two");
    g.handle(Event::Wheel(-1.0));
    assert_eq!(line(&g), "three");
    assert_eq!(g.screen(), Screen::Story);
}

#[test]
fn wheel_down_with_nothing_to_redo_does_nothing() {
    let p = parse(THREE).unwrap();
    let mut g = new_game(&p, instant());
    key(&mut g, Key::Space);
    g.handle(Event::Wheel(-1.0));
    assert_eq!(g.screen(), Screen::Story);
    assert_eq!(line(&g), "two");
    g.handle(Event::Wheel(1.0));
    key(&mut g, Key::Space); // a new input drops the redo
    g.handle(Event::Wheel(-1.0));
    assert_eq!(line(&g), "two");
    assert_eq!(g.screen(), Screen::Story);
}

#[test]
fn a_still_screen_needs_no_ticks_but_timers_and_typing_do() {
    let p = parse("**A:** one\n::wait{time=2}\n**A:** two\n").unwrap();
    let mut g = new_game(&p, Settings { text_speed: 30.0, ..Settings::default() });
    assert!(g.needs_ticks(), "typing");
    key(&mut g, Key::Space); // finish typing
    assert!(!g.needs_ticks(), "a finished line just waits for the player");
    key(&mut g, Key::Space);
    assert!(g.needs_ticks(), "a wait counts down");
    key(&mut g, Key::Space); // skip the wait
    key(&mut g, Key::Space);
    key(&mut g, Key::Char('a'));
    assert!(g.needs_ticks(), "auto-forward counts down");
}

#[test]
fn clicks_and_keys_advance_and_go_back() {
    let p = parse(THREE).unwrap();
    let mut g = new_game(&p, instant());
    click_at(&mut g, Some((900.0, 300.0)));
    assert_eq!(line(&g), "two");
    key(&mut g, Key::Space);
    assert_eq!(line(&g), "three");
    g.handle(Event::Wheel(1.0));
    assert_eq!(line(&g), "two");
    key(&mut g, Key::Backspace);
    assert_eq!(line(&g), "one");
    click_at(&mut g, None); // a click on the letterbox bars still advances
    assert_eq!(line(&g), "two");
}

#[test]
fn the_quick_menu_acts_instead_of_advancing() {
    let p = parse(THREE).unwrap();
    let mut g = new_game(&p, instant());
    key(&mut g, Key::Space);
    click_at(&mut g, centre(quick_button_rect(QuickButton::Back)));
    assert_eq!(line(&g), "one");
    click_at(&mut g, centre(quick_button_rect(QuickButton::History)));
    assert_eq!(g.screen(), Screen::Backlog);
    key(&mut g, Key::Escape);
    click_at(&mut g, centre(quick_button_rect(QuickButton::Auto)));
    assert!(g.auto());
    click_at(&mut g, centre(quick_button_rect(QuickButton::Settings)));
    assert_eq!(g.screen(), Screen::Settings);
    assert_eq!(line(&g), "one", "none of that advanced the story");
}

#[test]
fn choices_take_clicks_and_number_keys_but_not_advance() {
    let p = parse(CHOICE).unwrap();
    let mut g = new_game(&p, instant());
    key(&mut g, Key::Space);
    assert_eq!(g.view().choices.len(), 2);
    key(&mut g, Key::Space);
    click_at(&mut g, Some((5.0, 5.0))); // not on a button
    assert_eq!(g.view().choices.len(), 2);
    key(&mut g, Key::Char('2'));
    assert_eq!(line(&g), "went right");

    let mut g = new_game(&p, instant());
    key(&mut g, Key::Space);
    click_at(&mut g, centre(quillstone_render::view::choice_rects(2)[0]));
    assert_eq!(line(&g), "went left");
}

#[test]
fn advancing_past_the_end_returns_to_the_main_menu() {
    let p = parse("**A:** only\n").unwrap();
    let mut g = new_game(&p, instant());
    key(&mut g, Key::Space);
    assert_eq!(g.screen(), Screen::Story, "the last line stays up until the player moves on");
    key(&mut g, Key::Space);
    assert_eq!(g.screen(), Screen::MainMenu);
    assert!(!g.in_story());
    assert!(g.take_requests().is_empty());
}

#[test]
fn audio_starts_with_the_channel_volumes_then_the_music() {
    let p = parse("::music{src=m.ogg}\n**A:** hi\n").unwrap();
    let settings = Settings { music_volume: 0.5, ..instant() };
    let mut g = new_game(&p, settings);
    let audio = g.take_audio();
    assert_eq!(audio[0], Action::SetChannelVolume { channel: Channel::Music, volume: 0.5 });
    assert_eq!(audio.len(), 4);
    assert!(matches!(&audio[3], Action::PlayMusic { src, .. } if src == "m.ogg"));
    assert!(g.take_audio().is_empty());
}

// ---- auto-forward ------------------------------------------------------------------------

#[test]
fn auto_forward_waits_for_the_delay_plus_reading_time() {
    let p = parse(THREE).unwrap();
    let mut g = new_game(&p, Settings { auto_delay: 1.0, ..instant() });
    key(&mut g, Key::Char('a'));
    let needed = 1.0 + 3.0 * AUTO_PER_CHAR; // "one" is 3 characters
    g.tick(needed - 0.1, false);
    assert_eq!(line(&g), "one");
    g.tick(0.2, false);
    assert_eq!(line(&g), "two");
}

#[test]
fn auto_forward_waits_for_typing_and_voice() {
    let p = parse(THREE).unwrap();
    let mut g = new_game(&p, Settings { auto_delay: 0.5, text_speed: 10.0, ..Settings::default() });
    key(&mut g, Key::Char('A'));
    g.tick(0.25, false); // still typing "one"
    g.tick(0.25, false); // done typing at 0.3; the delay only starts now
    assert_eq!(line(&g), "one");
    for _ in 0..10 {
        g.tick(0.5, true); // voice still talking
    }
    assert_eq!(line(&g), "one");
    g.tick(0.6, false);
    assert_eq!(line(&g), "two");
}

#[test]
fn auto_forward_stops_at_choices_never_quits_and_is_cancelled_by_rollback() {
    let p = parse(CHOICE).unwrap();
    let mut g = new_game(&p, instant());
    key(&mut g, Key::Char('a'));
    for _ in 0..20 {
        g.tick(1.0, false);
    }
    assert_eq!(g.view().choices.len(), 2);
    key(&mut g, Key::Char('1'));
    for _ in 0..20 {
        g.tick(1.0, false);
    }
    assert_eq!(line(&g), "went left");
    assert!(g.view().finished, "auto-forward reached the end...");
    assert!(!g.take_requests().contains(&Request::Quit), "...but leaving is the player's call");
    key(&mut g, Key::Backspace);
    assert!(!g.auto());
}

// ---- settings --------------------------------------------------------------------------------

#[test]
fn the_settings_screen_pauses_the_story_and_saves_only_changes() {
    let p = parse("**A:** one\n::wait{time=1}\n**A:** two\n").unwrap();
    let mut g = new_game(&p, instant());
    key(&mut g, Key::Space); // now in the wait
    key(&mut g, Key::Escape);
    assert_eq!(g.screen(), Screen::Settings);
    key(&mut g, Key::Space);
    g.tick(5.0, false);
    assert_eq!(line(&g), "one", "neither a key nor the wait moved the story");
    key(&mut g, Key::Escape);
    assert_eq!(g.screen(), Screen::Story);
    assert!(g.take_requests().is_empty(), "nothing changed, nothing to save");

    g.handle(Event::Press(Button::Right));
    key(&mut g, Key::Down);
    key(&mut g, Key::Right); // auto-forward delay up a notch
    g.handle(Event::Press(Button::Right));
    assert_eq!(g.take_requests(), vec![Request::SaveSettings]);
    assert!(g.settings().auto_delay > Settings::default().auto_delay);
}

#[test]
fn clicking_and_dragging_a_volume_slider() {
    let p = parse(THREE).unwrap();
    let mut g = new_game(&p, instant());
    g.take_audio();
    key(&mut g, Key::Escape);
    let music = ROWS.iter().position(|r| *r == Row::Music).unwrap();
    let t = track_rect(music);
    g.handle(Event::CursorMoved(Some((t.x + t.w * 0.25, t.y))));
    g.handle(Event::Press(Button::Left));
    assert_eq!(g.settings().music_volume, 0.25);
    g.handle(Event::CursorMoved(Some((t.x + t.w * 0.75, t.y + 200.0)))); // drag; leaving the row is fine
    assert_eq!(g.settings().music_volume, 0.75);
    g.handle(Event::Release(Button::Left));
    g.handle(Event::CursorMoved(Some((t.x, t.y))));
    assert_eq!(g.settings().music_volume, 0.75, "released: no longer dragging");
    assert_eq!(
        g.take_audio().last(),
        Some(&Action::SetChannelVolume { channel: Channel::Music, volume: 0.75 })
    );
    assert_eq!(line(&g), "one");
}

#[test]
fn text_speed_changes_apply_to_the_line_on_screen() {
    let p = parse(THREE).unwrap();
    let mut g = new_game(&p, Settings { text_speed: 10.0, ..Settings::default() });
    assert!(!g.view().dialogue.as_ref().unwrap().is_complete());
    key(&mut g, Key::Escape);
    for _ in 0..30 {
        key(&mut g, Key::Right); // text speed is the first row: all the way to instant
    }
    assert_eq!(g.settings().text_speed, 0.0);
    key(&mut g, Key::Escape);
    g.tick(0.0, false);
    assert!(g.view().dialogue.as_ref().unwrap().is_complete());
}

#[test]
fn fullscreen_from_f11_or_the_toggle() {
    let p = parse(THREE).unwrap();
    let mut g = new_game(&p, instant());
    key(&mut g, Key::F11);
    assert_eq!(g.take_requests(), vec![Request::Fullscreen(true), Request::SaveSettings]);
    key(&mut g, Key::Escape);
    let row = ROWS.iter().position(|r| *r == Row::Fullscreen).unwrap();
    click_at(&mut g, centre(control_rect(row)));
    assert!(!g.settings().fullscreen);
    assert_eq!(g.take_requests(), vec![Request::Fullscreen(false)]);
    key(&mut g, Key::Escape);
    assert_eq!(g.take_requests(), vec![Request::SaveSettings], "turned off since the screen opened");
}

#[test]
fn quit_and_back_buttons() {
    let p = parse(THREE).unwrap();
    let mut g = new_game(&p, instant());
    key(&mut g, Key::Escape);
    click_at(&mut g, centre(button_rect(SettingsButton::Back, true)));
    assert_eq!(g.screen(), Screen::Story);
    key(&mut g, Key::Escape);
    click_at(&mut g, centre(button_rect(SettingsButton::Quit, true)));
    assert!(g.take_requests().is_empty(), "asks first");
    click_at(&mut g, centre(confirm_button_rect(false)));
    assert!(g.take_requests().is_empty(), "No: still playing");
    key(&mut g, Key::Escape);
    click_at(&mut g, centre(button_rect(SettingsButton::Quit, true)));
    click_at(&mut g, centre(confirm_button_rect(true)));
    assert_eq!(g.take_requests(), vec![Request::Quit]);
}

// ---- backlog -----------------------------------------------------------------------------------

#[test]
fn the_backlog_lists_what_was_read_and_scrolls() {
    let p = parse(THREE).unwrap();
    let mut g = new_game(&p, instant());
    key(&mut g, Key::Space);
    key(&mut g, Key::Space);
    key(&mut g, Key::Char('h'));
    assert_eq!(g.screen(), Screen::Backlog);
    assert_eq!(g.backlog.len(), 3);
    g.handle(Event::Wheel(1.0));
    g.handle(Event::Wheel(1.0));
    g.handle(Event::Wheel(1.0));
    assert_eq!(g.backlog_scroll, 2, "clamped to the oldest entry");
    key(&mut g, Key::End);
    g.handle(Event::Wheel(-1.0)); // wheeling down at the newest entry stays put
    assert_eq!(g.screen(), Screen::Backlog);
    key(&mut g, Key::Escape);
    assert_eq!(g.screen(), Screen::Story);
    assert_eq!(line(&g), "three");
    key(&mut g, Key::PageUp);
    assert_eq!(g.screen(), Screen::Backlog);
    g.handle(Event::Press(Button::Left));
    assert_eq!(g.screen(), Screen::Story);
    assert_eq!(line(&g), "three", "closing with a click doesn't advance");
}

// ---- drawing ---------------------------------------------------------------------------------------

#[test]
fn overlays_draw_on_top_of_the_story() {
    let p = parse(THREE).unwrap();
    let mut g = new_game(&p, instant());
    let none = |_: &str| None;
    let story = g.draw_list(&none);
    assert!(story.iter().any(|d| matches!(d, Draw::Text { text, .. } if text == "History")));
    key(&mut g, Key::Escape);
    let settings = g.draw_list(&none);
    assert!(settings.len() > story.len());
    assert_eq!(&settings[..story.len()], &story[..]);
}

#[test]
fn the_demo_plays_through_with_mouse_and_keys() {
    let program = parse(include_str!("../../../../examples/demo.quill.md")).unwrap();
    let mut g = new_game(&program, instant());
    for _ in 0..30 {
        if !g.view().choices.is_empty() {
            break;
        }
        key(&mut g, Key::Enter);
        g.tick(1.0, false);
    }
    assert_eq!(g.view().choices.len(), 4);
    key(&mut g, Key::Char('3'));
    for _ in 0..10 {
        key(&mut g, Key::Space);
        g.tick(1.0, false);
    }
    assert_eq!(g.screen(), Screen::MainMenu, "the end leads back to the title screen");
}

// ---- saving and loading ---------------------------------------------------------------------

use crate::slots::{Page, Slot};

fn has_file(g: &Game, name: &str) -> bool {
    g.storage.read(name).is_some()
}

fn notice(g: &Game) -> Option<String> {
    g.notice.as_ref().map(|(m, _)| m.clone())
}

#[test]
fn saving_to_a_slot_then_overwriting_it_asks_first() {
    let p = parse(THREE).unwrap();
    let mut g = new_game(&p, instant());
    key(&mut g, Key::Char('s'));
    assert_eq!(g.screen(), Screen::Save);
    click_at(&mut g, centre(slot_rect(0)));
    assert!(has_file(&g, "1-1.save"));
    assert_eq!(g.slots[0].as_ref().unwrap().preview, "A: one");

    key(&mut g, Key::Escape);
    key(&mut g, Key::Space); // now on "two"
    key(&mut g, Key::Char('s'));
    click_at(&mut g, centre(slot_rect(0)));
    assert!(g.confirm.is_some(), "the slot is taken");
    key(&mut g, Key::Space);
    assert!(g.confirm.is_some(), "a confirm dialog swallows other keys");
    key(&mut g, Key::Char('n'));
    assert_eq!(g.slots[0].as_ref().unwrap().preview, "A: one", "No keeps the old save");
    click_at(&mut g, centre(slot_rect(0)));
    click_at(&mut g, centre(confirm_button_rect(true)));
    assert_eq!(g.slots[0].as_ref().unwrap().preview, "A: two");
}

#[test]
fn loading_a_slot_restores_the_game() {
    let p = parse("::show{src=s.png id=s}\n**A:** one\n::hide{s}\n**A:** two\n**A:** three\n").unwrap();
    let mut g = new_game(&p, instant());
    key(&mut g, Key::Char('s'));
    click_at(&mut g, centre(slot_rect(2)));
    key(&mut g, Key::Escape);
    key(&mut g, Key::Space);
    key(&mut g, Key::Space);
    assert_eq!(line(&g), "three");
    assert!(g.view().sprites.is_empty());

    key(&mut g, Key::Char('l'));
    click_at(&mut g, centre(slot_rect(2)));
    click_at(&mut g, centre(confirm_button_rect(true)));
    assert_eq!(g.screen(), Screen::Story);
    assert_eq!(line(&g), "one");
    assert_eq!(g.view().sprites.len(), 1, "the saved scene is back");
    assert_eq!(notice(&g).as_deref(), Some("Loaded"));
    key(&mut g, Key::Space);
    assert_eq!(line(&g), "two", "and it plays on from there");
}

#[test]
fn clicking_an_empty_or_damaged_slot_on_the_load_screen_does_nothing() {
    let p = parse(THREE).unwrap();
    let mut g = new_game(&p, instant());
    g.storage.write("1-2.save", "garbage").unwrap();
    key(&mut g, Key::Char('l'));
    click_at(&mut g, centre(slot_rect(0)));
    click_at(&mut g, centre(slot_rect(1)));
    assert!(g.confirm.is_none());
    assert_eq!(g.screen(), Screen::Load);
}

#[test]
fn quick_save_and_quick_load() {
    let p = parse(THREE).unwrap();
    let mut g = new_game(&p, instant());
    key(&mut g, Key::F9);
    assert_eq!(notice(&g).as_deref(), Some("No quick save yet"));
    key(&mut g, Key::Space);
    click_at(&mut g, centre(quick_button_rect(QuickButton::QuickSave)));
    assert_eq!(notice(&g).as_deref(), Some("Quick saved"));
    assert!(has_file(&g, "quick-1.save"));
    g.tick(NOTICE_SECONDS + 0.1, false);
    assert_eq!(notice(&g), None, "notices go away");

    key(&mut g, Key::Space);
    key(&mut g, Key::F5); // a second quick save goes to the next slot
    assert!(has_file(&g, "quick-2.save"));
    key(&mut g, Key::Backspace);
    key(&mut g, Key::Backspace);
    assert_eq!(line(&g), "one");
    click_at(&mut g, centre(quick_button_rect(QuickButton::QuickLoad)));
    key(&mut g, Key::Enter); // yes
    assert_eq!(line(&g), "three", "the newest quick save");
}

#[test]
fn reaching_a_choice_and_quitting_save_automatically() {
    let p = parse(CHOICE).unwrap();
    let mut g = new_game(&p, instant());
    g.autosave();
    assert!(!has_file(&g, "auto-1.save"), "nothing to save at the very start");
    key(&mut g, Key::Space);
    assert!(has_file(&g, "auto-1.save"), "arrived at a choice");
    key(&mut g, Key::Escape);
    click_at(&mut g, centre(button_rect(SettingsButton::Quit, true)));
    key(&mut g, Key::Char('y'));
    assert!(has_file(&g, "auto-2.save"), "saved on the way out");
    assert_eq!(g.take_requests(), vec![Request::Quit]);

    // The load screen offers the auto page; the save screen doesn't.
    key(&mut g, Key::Char('l'));
    let tabs = g.slot_tabs();
    assert_eq!(tabs[0], Page::Auto);
    click_at(&mut g, centre(tab_rect(0, tabs.len())));
    assert_eq!(g.page, Page::Auto);
    assert!(g.slots[0].is_some() && g.slots[1].is_some());
    key(&mut g, Key::Escape);
    key(&mut g, Key::Char('s'));
    assert!(g.slot_tabs().iter().all(|p| matches!(p, Page::Numbered(_))));
    assert_eq!(g.page, Page::Numbered(1));
    key(&mut g, Key::Right);
    assert_eq!(g.page, Page::Numbered(2));
}

#[test]
fn loading_switches_the_music_to_the_saved_track() {
    let p = parse("::music{src=a.ogg}\n**A:** one\n::music{src=b.ogg}\n**A:** two\n").unwrap();
    let mut g = new_game(&p, instant());
    g.save_to(Slot { page: Page::Numbered(1), index: 0 });
    key(&mut g, Key::Space);
    g.take_audio();
    g.load_from(Slot { page: Page::Numbered(1), index: 0 });
    let audio = g.take_audio();
    assert!(audio.iter().any(|a| matches!(a, Action::PlayMusic { src, .. } if src == "a.ogg")));
}

#[test]
fn slot_screens_draw_thumbnails_and_dialogs_on_top() {
    let p = parse("::bg{src=bg.png}\n**A:** one\n").unwrap();
    let mut g = new_game(&p, instant());
    g.save_to(Slot { page: Page::Numbered(1), index: 0 });
    key(&mut g, Key::Char('s'));
    let sizes = |_: &str| Some(Size::new(1920.0, 1080.0));
    let list = g.draw_list(&sizes);
    let thumb = thumb_rect(0);
    assert!(list.iter().any(|d| matches!(d, Draw::Image { dest, .. } if *dest == thumb)));
    click_at(&mut g, centre(slot_rect(0)));
    let list = g.draw_list(&sizes);
    let yes = list.iter().rposition(|d| matches!(d, Draw::Text { text, .. } if text == "Yes")).unwrap();
    assert!(yes > list.iter().position(|d| matches!(d, Draw::Image { .. })).unwrap());
}

// ---- skip mode -------------------------------------------------------------------------------

const LONG: &str = "**A:** one\n**A:** two\n::wait{time=5}\n**A:** three\n**A:** four\n::choice\n- [Go](#go)\n::\n# go\n**A:** after\n";

/// A new game that remembers what an earlier one read.
fn reread<'p>(p: &'p Program, earlier: &Game, settings: Settings) -> Game<'p> {
    let mut storage = crate::MemoryStorage::default();
    if let Some(text) = earlier.storage.read("read.txt") {
        storage.files.insert("read.txt".into(), text);
    }
    let mut g = Game::new(p, settings, Box::new(storage));
    g.start_story();
    g
}

fn run(g: &mut Game, frames: usize) {
    for _ in 0..frames {
        g.tick(SKIP_DELAY, false);
    }
}

#[test]
fn lines_count_as_read_once_left_and_the_record_is_kept() {
    let p = parse(LONG).unwrap();
    let mut g = new_game(&p, instant());
    assert!(!g.current_line_read());
    key(&mut g, Key::Space);
    key(&mut g, Key::Backspace);
    assert!(g.current_line_read(), "\"one\" was left once");
    key(&mut g, Key::Space);
    key(&mut g, Key::Space); // left "two" for the wait
    g.autosave(); // also writes the record
    assert!(g.storage.read("read.txt").is_some());

    let fresh = reread(&p, &g, instant());
    assert!(fresh.current_line_read(), "a new playthrough remembers");
}

#[test]
fn the_ending_is_remembered_as_read_without_another_choice() {
    let p = parse(THREE).unwrap();
    let mut g = new_game(&p, instant());
    for _ in 0..3 {
        key(&mut g, Key::Space); // read all three; the story ends
    }
    key(&mut g, Key::Space); // back to the main menu
    assert_eq!(g.screen(), Screen::MainMenu);
    let fresh = reread(&p, &g, instant());
    assert!(fresh.current_line_read(), "no choice came after, yet the record was written");
}

#[test]
fn skip_runs_through_read_lines_and_waits_then_stops_at_the_first_unread_one() {
    let p = parse(LONG).unwrap();
    let mut first = new_game(&p, instant());
    key(&mut first, Key::Space); // read "one"
    key(&mut first, Key::Space); // read "two"
    first.autosave();

    let mut g = reread(&p, &first, instant());
    key(&mut g, Key::Tab);
    assert!(g.skipping());
    run(&mut g, 20);
    assert_eq!(line(&g), "three", "past the wait, stopped at the unread line");
    assert!(!g.skipping());
    assert!(!g.view().dialogue.as_ref().unwrap().text.is_empty());
}

#[test]
fn skip_wont_start_on_an_unread_line_unless_the_setting_allows_it() {
    let p = parse(LONG).unwrap();
    let mut g = new_game(&p, instant());
    key(&mut g, Key::Tab);
    assert!(!g.skipping());
    assert!(notice(&g).is_some());

    let mut g = new_game(&p, Settings { skip_unread: true, ..instant() });
    key(&mut g, Key::Tab);
    run(&mut g, 30);
    assert_eq!(g.view().choices.len(), 1, "skipped everything up to the choice");
    assert!(!g.skipping(), "choices always stop it");
}

#[test]
fn a_click_stops_skipping_without_advancing_and_tab_toggles() {
    let p = parse(LONG).unwrap();
    let mut g = new_game(&p, Settings { skip_unread: true, ..instant() });
    key(&mut g, Key::Tab);
    run(&mut g, 1);
    let at = line(&g);
    click_at(&mut g, Some((900.0, 300.0)));
    assert!(!g.skipping());
    assert_eq!(line(&g), at, "the click only stopped skipping");
    key(&mut g, Key::Tab);
    assert!(g.skipping());
    key(&mut g, Key::Tab);
    assert!(!g.skipping());
    click_at(&mut g, centre(quick_button_rect(QuickButton::Skip)));
    assert!(g.skipping(), "the quick menu button works too");
    click_at(&mut g, centre(quick_button_rect(QuickButton::Skip)));
    assert!(!g.skipping());
}

#[test]
fn holding_ctrl_skips_until_it_is_let_go() {
    let p = parse(LONG).unwrap();
    let mut g = new_game(&p, Settings { skip_unread: true, ..instant() });
    key(&mut g, Key::Ctrl);
    run(&mut g, 1);
    assert_eq!(line(&g), "two");
    g.handle(Event::KeyReleased(Key::Ctrl));
    assert!(!g.skipping());
    run(&mut g, 10);
    assert_eq!(line(&g), "two");
}

#[test]
fn skipping_mutes_sound_effects_and_voice_but_not_music() {
    let src = "**A:** one\n::sfx{src=door.ogg}\n::voice{src=v.ogg}\n::music{src=m.ogg}\n**A:** two\n**A:** three\n";
    let p = parse(src).unwrap();
    let mut g = new_game(&p, Settings { skip_unread: true, ..instant() });
    g.take_audio();
    key(&mut g, Key::Tab);
    run(&mut g, 5);
    let audio = g.take_audio();
    assert!(!audio.iter().any(|a| matches!(a, Action::PlaySfx { .. } | Action::PlayVoice { .. })));
    assert!(audio.iter().any(|a| matches!(a, Action::PlayMusic { .. })));
}

#[test]
fn going_back_or_opening_a_screen_stops_skipping() {
    let p = parse(LONG).unwrap();
    for stop in [Key::Backspace, Key::Escape, Key::Char('h'), Key::Char('s'), Key::F9] {
        let mut g = new_game(&p, Settings { skip_unread: true, ..instant() });
        key(&mut g, Key::Space);
        key(&mut g, Key::F5); // so F9 has something to ask about
        key(&mut g, Key::Tab);
        key(&mut g, stop);
        assert!(!g.skipping(), "{stop:?}");
    }
}

#[test]
fn skip_unread_is_a_settings_toggle() {
    let p = parse(THREE).unwrap();
    let mut g = new_game(&p, instant());
    key(&mut g, Key::Escape);
    let row = ROWS.iter().position(|r| *r == Row::SkipUnread).unwrap();
    click_at(&mut g, centre(control_rect(row)));
    assert!(g.settings().skip_unread);
    key(&mut g, Key::Escape);
    assert_eq!(g.take_requests(), vec![Request::SaveSettings]);
}

// ---- main menu and theme -------------------------------------------------------------------

fn at_menu<'p>(p: &'p Program) -> Game<'p> {
    Game::new(p, instant(), Box::new(crate::MemoryStorage::default()))
}

#[test]
fn the_game_opens_at_the_main_menu_and_start_begins_the_story() {
    let p = parse("::music{src=story.ogg}\n**A:** one\n").unwrap();
    let mut g = at_menu(&p);
    assert_eq!(g.screen(), Screen::MainMenu);
    assert!(!g.in_story());
    assert!(!g.take_audio().iter().any(|a| matches!(a, Action::PlayMusic { .. })), "the story hasn't begun");
    key(&mut g, Key::Space);
    assert_eq!(g.screen(), Screen::MainMenu, "keys don't reach the story");
    click_at(&mut g, centre(menu_button_rect(0))); // Start
    assert_eq!(g.screen(), Screen::Story);
    assert_eq!(line(&g), "one");
    assert!(g.take_audio().iter().any(|a| matches!(a, Action::PlayMusic { src, .. } if src == "story.ogg")));
}

#[test]
fn the_menu_has_keyboard_focus_and_a_title() {
    let p = parse("---\ntitle: My Novel\n---\n**A:** one\n").unwrap();
    let mut g = at_menu(&p);
    let list = g.draw_list(&|_| None);
    assert!(list.iter().any(|d| matches!(d, Draw::Text { text, .. } if text == "My Novel")));
    key(&mut g, Key::Down); // focus on the first button (Start)
    key(&mut g, Key::Enter);
    assert_eq!(line(&g), "one");
}

#[test]
fn menu_music_comes_from_the_theme_and_stops_when_the_story_starts() {
    let p = parse("**A:** one\n").unwrap();
    let mut g = at_menu(&p);
    g.take_audio();
    g.set_theme(Theme { menu_music: Some("menu.ogg".into()), ..Theme::default() });
    assert!(g.take_audio().iter().any(|a| matches!(a, Action::PlayMusic { src, .. } if src == "menu.ogg")));
    g.start_story();
    assert!(g.take_audio().iter().any(|a| matches!(a, Action::StopMusic { .. })));
}

#[test]
fn continue_appears_once_there_is_a_save_and_loads_the_newest() {
    let p = parse(THREE).unwrap();
    let mut g = at_menu(&p);
    assert_eq!(menu_buttons(g.newest_save.is_some(), false).len(), 4, "no Continue yet");
    g.start_story();
    key(&mut g, Key::Space);
    key(&mut g, Key::F5); // quick save on "two"
    key(&mut g, Key::Escape);
    click_at(&mut g, centre(button_rect(SettingsButton::MainMenu, true)));
    key(&mut g, Key::Char('y'));
    assert_eq!(g.screen(), Screen::MainMenu);
    let buttons = menu_buttons(g.newest_save.is_some(), false);
    assert_eq!(buttons[1], MenuButton::Continue);
    click_at(&mut g, centre(menu_button_rect(1)));
    assert_eq!(g.screen(), Screen::Story);
    assert_eq!(line(&g), "two", "no confirm needed: nothing to lose at the menu");
}

#[test]
fn screens_opened_from_the_menu_return_to_it() {
    let p = parse(THREE).unwrap();
    let mut g = at_menu(&p);
    let buttons = menu_buttons(false, false);
    let settings = buttons.iter().position(|b| *b == MenuButton::Settings).unwrap();
    click_at(&mut g, centre(menu_button_rect(settings)));
    assert_eq!(g.screen(), Screen::Settings);
    let (x, y) = centre(button_rect(SettingsButton::MainMenu, true)).unwrap();
    assert_eq!(settings_at(x, y, false), None, "no Main menu button here");
    key(&mut g, Key::Escape);
    assert_eq!(g.screen(), Screen::MainMenu);

    let load = buttons.iter().position(|b| *b == MenuButton::Load).unwrap();
    click_at(&mut g, centre(menu_button_rect(load)));
    assert_eq!(g.screen(), Screen::Load);
    click_at(&mut g, centre(slots_back_rect()));
    assert_eq!(g.screen(), Screen::MainMenu);

    click_at(&mut g, centre(menu_button_rect(settings)));
    click_at(&mut g, centre(button_rect(SettingsButton::Quit, false)));
    assert_eq!(g.take_requests(), vec![Request::Quit], "quitting from the menu doesn't ask");
}

#[test]
fn closing_the_window_at_the_menu_saves_nothing() {
    let p = parse(THREE).unwrap();
    let mut g = at_menu(&p);
    g.autosave();
    assert!(crate::slots::newest_save(&*g.storage).is_none());
}

#[test]
fn the_theme_survives_loading_and_returning_to_the_menu() {
    let p = parse(THREE).unwrap();
    let mut g = new_game(&p, instant());
    g.set_theme(Theme { text_size: 51.0, ..Theme::default() });
    key(&mut g, Key::F5);
    key(&mut g, Key::F9);
    key(&mut g, Key::Char('y'));
    assert_eq!(g.theme().text_size, 51.0);
    for _ in 0..4 {
        key(&mut g, Key::Space); // "two", "three", the end, then the main menu
    }
    assert_eq!(g.screen(), Screen::MainMenu);
    assert_eq!(g.theme().text_size, 51.0);
}

// ---- video -------------------------------------------------------------------------------------

const MOVIE: &str = "::bg{src=loop.webm}\n**A:** before\n::video{src=clip.webm}\n**A:** after\n::video{src=forced.webm}\n**A:** last\n";

fn wanted(g: &Game) -> Vec<(String, bool)> {
    g.wanted_videos().into_iter().map(|w| (w.src, w.looped)).collect()
}

#[test]
fn the_game_says_which_videos_should_play() {
    let p = parse(MOVIE).unwrap();
    let mut g = new_game(&p, instant());
    assert_eq!(wanted(&g), vec![("loop.webm".to_string(), true)], "a video background loops");
    key(&mut g, Key::Space);
    assert_eq!(wanted(&g), vec![("clip.webm".to_string(), false), ("loop.webm".to_string(), true)]);

    let mut menu = Game::new(&p, instant(), Box::new(crate::MemoryStorage::default()));
    assert!(wanted(&menu).is_empty());
    menu.set_theme(Theme { menu_background: Some("menu.webm".into()), ..Theme::default() });
    assert_eq!(wanted(&menu), vec![("menu.webm".to_string(), true)]);
}

#[test]
fn the_end_of_a_cutscene_moves_the_story_on() {
    let p = parse(MOVIE).unwrap();
    let mut g = new_game(&p, instant());
    key(&mut g, Key::Space);
    g.video_update(true, false);
    assert!(g.view().video.is_some());
    g.video_update(true, true);
    assert_eq!(line(&g), "after");
    g.video_update(false, true); // a stale report must not skip the next line
    assert_eq!(line(&g), "after");
}

#[test]
fn unskippable_clips_ignore_clicks_but_only_while_really_playing() {
    let p = parse(MOVIE).unwrap();
    let mut g = new_game(&p, instant());
    key(&mut g, Key::Space);
    key(&mut g, Key::Space); // clip.webm is skippable: a press skips it
    assert_eq!(line(&g), "after");
    key(&mut g, Key::Space); // forced.webm isn't skippable
    g.video_update(true, false);
    click_at(&mut g, Some((900.0, 300.0)));
    assert!(g.view().video.is_some(), "the click was ignored");
    g.video_update(false, false); // e.g. no decoder for it: never trap the player
    click_at(&mut g, Some((900.0, 300.0)));
    assert_eq!(line(&g), "last");
}

#[test]
fn rolling_back_past_a_cutscene_skips_it_and_menus_pause_it() {
    let p = parse(MOVIE).unwrap();
    let mut g = new_game(&p, instant());
    key(&mut g, Key::Space);
    assert_eq!(g.wanted_videos()[0].src, "clip.webm");
    assert!(!g.media_paused());
    key(&mut g, Key::Escape);
    assert!(g.media_paused());
    g.video_update(true, true);
    assert!(g.view().video.is_some(), "an ending reported while paused waits");
    key(&mut g, Key::Escape);
    g.video_update(true, true);
    assert_eq!(line(&g), "after");
    key(&mut g, Key::Backspace);
    assert_eq!(line(&g), "before", "rollback does not land on the cutscene");
    assert!(g.view().video.is_none());
}

// ---- extras and persistent data --------------------------------------------------------------

const EXTRAS: &str = "---\npersistent: { finished_once: false }\ngallery: [cg.png, other.png]\nmusic_room: [song.ogg]\n---\n::if{finished_once}\n**A:** Welcome back.\n::endif\n**A:** one\n::bg{src=cg.png}\n::music{src=song.ogg}\n**A:** two\n::set{finished_once=true}\n::end\n";

/// A new game (at the menu) that remembers what an earlier one kept for good.
fn next_playthrough<'p>(p: &'p Program, earlier: &Game) -> Game<'p> {
    let mut storage = crate::MemoryStorage::default();
    if let Some(text) = earlier.storage.read("persistent.txt") {
        storage.files.insert("persistent.txt".into(), text);
    }
    Game::new(p, instant(), Box::new(storage))
}

fn open_extras(g: &mut Game) {
    let buttons = menu_buttons(g.newest_save.is_some(), g.has_extras());
    let at = buttons.iter().position(|b| *b == MenuButton::Extras).expect("an Extras button");
    click_at(g, centre(menu_button_rect(at)));
    assert_eq!(g.screen(), Screen::Extras);
}

#[test]
fn extras_unlock_by_playing_and_stay_unlocked() {
    let p = parse(EXTRAS).unwrap();
    let mut g = at_menu(&p);
    open_extras(&mut g);
    assert_eq!(g.extras_entries(), [("cg.png", false), ("other.png", false)]);
    click_at(&mut g, centre(crate::extras::cell_rect(0)));
    assert_eq!(g.extras.viewing, None);
    assert_eq!(notice(&g).as_deref(), Some("Not unlocked yet"));
    key(&mut g, Key::Escape);
    assert_eq!(g.screen(), Screen::MainMenu);

    g.start_story();
    assert_eq!(line(&g), "one", "no welcome on the first playthrough");
    for _ in 0..3 {
        key(&mut g, Key::Space); // "two", the end, then back at the menu
    }
    assert_eq!(g.screen(), Screen::MainMenu);

    let mut again = next_playthrough(&p, &g);
    open_extras(&mut again);
    assert_eq!(again.extras_entries(), [("cg.png", true), ("other.png", false)]);
    click_at(&mut again, centre(crate::extras::cell_rect(0)));
    assert_eq!(again.extras.viewing, Some(0));
    let size = |_: &str| Some(Size { w: 1920.0, h: 1080.0 });
    assert!(again.draw_list(&size).iter().any(|d| matches!(d, Draw::Image { src, .. } if src == "cg.png")));
    key(&mut again, Key::Right);
    assert_eq!(again.extras.viewing, Some(0), "the other picture is still locked");
    click_at(&mut again, Some((10.0, 10.0)));
    assert_eq!(again.extras.viewing, None, "a click closes the picture");

    key(&mut again, Key::Tab);
    again.take_audio();
    click_at(&mut again, centre(crate::extras::track_rect(0)));
    assert!(again.take_audio().iter().any(|a| matches!(a, Action::PlayMusic { src, .. } if src == "song.ogg")));
    key(&mut again, Key::Escape);
    assert!(again.take_audio().iter().any(|a| matches!(a, Action::StopMusic { .. })), "leaving stops the track");

    again.start_story();
    assert_eq!(line(&again), "Welcome back.", "persistent variables carry into the next playthrough");
}

#[test]
fn the_keyboard_walks_the_gallery_grid() {
    let src = "---\ngallery: [a.png, b.png, c.png, d.png, e.png]\n---\n**A:** hi\n";
    let p = parse(src).unwrap();
    let mut g = at_menu(&p);
    open_extras(&mut g);
    key(&mut g, Key::Right);
    assert_eq!(g.extras.focus, Some(0), "the first arrow press lands on the first entry");
    key(&mut g, Key::Right);
    key(&mut g, Key::Down);
    assert_eq!(g.extras.focus, Some(1), "nothing below b in a 4-wide grid of 5");
    key(&mut g, Key::Left);
    key(&mut g, Key::Down);
    assert_eq!(g.extras.focus, Some(4));
    key(&mut g, Key::Enter);
    assert_eq!(notice(&g).as_deref(), Some("Not unlocked yet"));
}

#[test]
fn games_without_extras_have_no_extras_button() {
    let p = parse(THREE).unwrap();
    let g = at_menu(&p);
    assert!(!g.has_extras());
    assert!(!menu_buttons(true, g.has_extras()).contains(&MenuButton::Extras));
    assert_eq!(menu_buttons(true, true).len(), 6);
    let last = menu_button_rect(5);
    assert!(last.y + last.h <= 1000.0, "six buttons fit on screen");
}

// ---- auto-voice ------------------------------------------------------------------------------

#[test]
fn recorded_lines_play_their_voice_by_themselves() {
    let src = "---\nvoice: voice\n---\n# talk\n**A:** Hello.\n**A:** No recording.\n::voice{src=own.ogg}\n**A:** Its own voice.\n";
    let p = parse(src).unwrap();
    let name = quillstone_runtime::voice_name(&p, p.labels["talk"]).unwrap();
    assert!(name.starts_with("talk-") && name.len() == "talk-".len() + 8, "{name}");
    let mut g = at_menu(&p);
    let own = quillstone_runtime::voice_name(&p, p.labels["talk"] + 3).unwrap();
    let files = [(name, "voice/hello.ogg".to_string()), (own, "voice/unused.ogg".to_string())];
    g.set_voice_files(files.into_iter().collect());
    g.start_story();
    let voices = |g: &mut Game| -> Vec<String> {
        g.take_audio().into_iter().filter_map(|a| match a {
            Action::PlayVoice { src } => Some(src),
            _ => None,
        }).collect()
    };
    assert_eq!(voices(&mut g), ["voice/hello.ogg"]);
    key(&mut g, Key::Space);
    assert!(voices(&mut g).is_empty(), "a line without a recording is silent");
    key(&mut g, Key::Space);
    assert_eq!(voices(&mut g), ["own.ogg"], "`::voice` wins over the voice folder");
    key(&mut g, Key::Backspace);
    key(&mut g, Key::Backspace);
    assert_eq!(voices(&mut g), ["voice/hello.ogg"], "rolling back to the line plays it again");
}

// ---- keyboard / gamepad focus ----------------------------------------------------------------

#[test]
fn arrows_pick_out_a_choice_and_enter_takes_it() {
    let p = parse(CHOICE).unwrap();
    let mut g = new_game(&p, instant());
    key(&mut g, Key::Space);
    key(&mut g, Key::Enter);
    assert_eq!(g.view().choices.len(), 2, "nothing picked out yet: Enter does nothing");
    key(&mut g, Key::Down);
    key(&mut g, Key::Down);
    key(&mut g, Key::Down);
    assert_eq!(g.choice_focus, Some(1), "stops at the last one");
    key(&mut g, Key::Enter);
    assert_eq!(line(&g), "went right");
    assert_eq!(g.choice_focus, None);
}

#[test]
fn arrows_walk_the_slot_grid_and_enter_saves() {
    let p = parse(THREE).unwrap();
    let mut g = new_game(&p, instant());
    key(&mut g, Key::Char('s'));
    key(&mut g, Key::Right);
    assert_eq!((g.page, g.slot_focus), (Page::Numbered(2), None), "no slot picked out: Right turns the page");
    key(&mut g, Key::Left);
    key(&mut g, Key::Down);
    key(&mut g, Key::Right);
    key(&mut g, Key::Down);
    assert_eq!(g.slot_focus, Some(SLOT_COLUMNS + 1));
    key(&mut g, Key::Right);
    key(&mut g, Key::Right);
    assert_eq!((g.page, g.slot_focus), (Page::Numbered(2), Some(SLOT_COLUMNS)), "off the row's end: next page");
    key(&mut g, Key::Enter);
    assert!(has_file(&g, "2-4.save"));
}

// ---- hot reload ------------------------------------------------------------------------------

#[test]
fn an_edited_script_takes_over_at_the_same_spot() {
    let before = parse("# a\n**A:** one\n**A:** two\n**A:** three\n").unwrap();
    let mut g = new_game(&before, instant());
    key(&mut g, Key::Space);
    assert_eq!(line(&g), "two");
    let after = parse("**A:** a new opening\n# a\n**A:** one\n**A:** two\n**A:** three, reworded\n").unwrap();
    g.reload(&after).unwrap();
    assert_eq!(line(&g), "two", "still on the same line");
    key(&mut g, Key::Space);
    assert_eq!(line(&g), "three, reworded", "and the rest is the new script");
    key(&mut g, Key::Backspace);
    key(&mut g, Key::Backspace);
    assert_eq!(line(&g), "one", "rollback still works across the reload");

    let renamed = parse("# opening\n**A:** a new opening\n# start\n**A:** one\n**A:** two\n").unwrap();
    g.reload(&renamed).unwrap();
    assert_eq!(line(&g), "one", "the label we were in was renamed: found by its words");
    key(&mut g, Key::Space);
    assert_eq!(line(&g), "two");
}

#[test]
fn reloading_at_the_menu_just_swaps_the_script() {
    let before = parse(THREE).unwrap();
    let mut g = at_menu(&before);
    let after = parse("**A:** brand new\n").unwrap();
    g.reload(&after).unwrap();
    assert_eq!(g.screen(), Screen::MainMenu);
    g.start_story();
    assert_eq!(line(&g), "brand new");
}

/// Storage the test can still reach after handing it to the game, and make fail.
#[derive(Clone, Default)]
struct Shared {
    mem: std::rc::Rc<std::cell::RefCell<crate::MemoryStorage>>,
    fail: std::rc::Rc<std::cell::Cell<bool>>,
}

impl Storage for Shared {
    fn read(&self, name: &str) -> Option<String> {
        self.mem.borrow().read(name)
    }

    fn write(&mut self, name: &str, contents: &str) -> Result<(), String> {
        if self.fail.get() {
            return Err("disk busy".into());
        }
        self.mem.borrow_mut().write(name, contents)
    }

    fn now(&self) -> crate::Timestamp {
        self.mem.borrow().now()
    }
}

#[test]
fn an_unlock_that_could_not_be_written_is_written_later() {
    // Nothing after the unlock changes what is kept, so only a retry writes it.
    let p = parse("---\ngallery: [cg.png]\n---\n**A:** one\n::bg{src=cg.png}\n**A:** two\n**A:** three\n").unwrap();
    let storage = Shared::default();
    let mut g = Game::new(&p, instant(), Box::new(storage.clone()));
    g.start_story();
    storage.fail.set(true);
    key(&mut g, Key::Space); // shows cg.png: unlocked, but the write fails
    assert!(notice(&g).is_some_and(|n| n.contains("Could not save what was unlocked")));
    storage.fail.set(false);
    g.autosave(); // the player closes the window right away
    let kept = storage.read("persistent.txt").expect("written once the disk works again");
    assert!(kept.contains("unlocked:cg.png"), "{kept}");
}

#[test]
fn a_clock_set_back_doesnt_make_new_saves_look_old() {
    let p = parse("**A:** one\n**A:** two\n").unwrap();
    let storage = Shared::default();
    storage.mem.borrow().clock.set(10_000_000);
    let mut g = Game::new(&p, instant(), Box::new(storage.clone()));
    g.start_story();
    key(&mut g, Key::F5);
    storage.mem.borrow().clock.set(0); // the PC's clock jumps back
    key(&mut g, Key::Space);
    key(&mut g, Key::F5);
    let second = crate::slots::Slot { page: crate::slots::Page::Quick, index: 1 };
    assert_eq!(crate::slots::newest_save(&storage).map(|(_, s)| s), Some(second), "Continue picks the latest save");
}

// ---- hotspots ----------------------------------------------------------------------------------------

const MAP: &str = "---\nvars: { got: false }\n---\n# map\n::bg{src=map.png}\n::hotspots\n- [Library](#lib){rect=0.1,0.2,0.2,0.3}\n- [Key](#key){rect=0.6,0.7,0.1,0.1 image=key.png, got=true, if not got}\n- [Leave](#out){rect=0.8,0.8,0.1,0.1}\n::\n# lib\n**A:** Books.\n::jump{to=map}\n# key\n**A:** A key.\n::jump{to=map}\n# out\n**A:** Bye.\n::end\n";

/// The middle of the nth hotspot as listed on screen right now.
fn spot_centre(g: &Game, i: usize) -> Option<(f32, f32)> {
    centre(quillstone_render::view::hotspot_rect(&g.view().hotspots[i]))
}

fn click_spot(g: &mut Game, i: usize) {
    let at = spot_centre(g, i);
    click_at(g, at);
}

#[test]
fn clicking_a_hotspot_picks_it_and_clicks_elsewhere_do_nothing() {
    let p = parse(MAP).unwrap();
    let mut g = new_game(&p, instant());
    assert_eq!(g.view().hotspots.len(), 3, "no line needed: the map shows at once");
    click_at(&mut g, Some((5.0, 5.0)));
    assert_eq!(g.view().hotspots.len(), 3, "a click on nothing doesn't advance");
    key(&mut g, Key::Char('1'));
    assert_eq!(g.view().hotspots.len(), 3, "number keys don't apply to hotspots");
    click_spot(&mut g, 1); // the key
    assert_eq!(line(&g), "A key.");
    key(&mut g, Key::Space);
    assert_eq!(g.view().hotspots.len(), 2, "the key is gone");
    click_spot(&mut g, 1); // "Leave" is now the second visible one
    assert_eq!(line(&g), "Bye.");
}

#[test]
fn hovering_a_hotspot_highlights_it_and_shows_its_label() {
    let p = parse(MAP).unwrap();
    let mut g = new_game(&p, instant());
    let texts = |g: &Game| -> Vec<String> {
        g.draw_list(&|_| None)
            .into_iter()
            .filter_map(|d| match d {
                quillstone_render::Draw::Text { text, .. } => Some(text),
                _ => None,
            })
            .collect()
    };
    assert!(texts(&g).is_empty(), "nothing to read while idle");
    g.handle(Event::CursorMoved(spot_centre(&g, 0)));
    assert_eq!(texts(&g), ["Library"]);
    g.handle(Event::CursorMoved(Some((5.0, 5.0))));
    assert!(texts(&g).is_empty());
}

#[test]
fn the_arrow_keys_cycle_the_hotspots_and_enter_takes_the_focused_one() {
    let p = parse(MAP).unwrap();
    let mut g = new_game(&p, instant());
    key(&mut g, Key::Enter);
    assert_eq!(g.view().hotspots.len(), 3, "nothing focused yet: Enter does nothing");
    key(&mut g, Key::Down);
    assert_eq!(g.choice_focus, Some(0));
    key(&mut g, Key::Right);
    key(&mut g, Key::Right);
    assert_eq!(g.choice_focus, Some(2));
    key(&mut g, Key::Right);
    assert_eq!(g.choice_focus, Some(0), "wraps round");
    key(&mut g, Key::Left);
    assert_eq!(g.choice_focus, Some(2));
    key(&mut g, Key::Up);
    assert_eq!(g.choice_focus, Some(1));
    // the focused region is highlighted like a hovered one, with its label by it
    let drawn = g.draw_list(&|_| None);
    assert!(drawn.iter().any(|d| matches!(d, quillstone_render::Draw::Text { text, .. } if text == "Key")));
    key(&mut g, Key::Space);
    assert_eq!(line(&g), "A key.");
    assert_eq!(g.choice_focus, None);
}

const HIDDEN_MAP: &str = "# map\n::bg{src=map.png}\n::hotspots\n- [Library](#lib){rect=0.1,0.2,0.2,0.3}\n- [Scarf](#scarf){rect=0.6,0.7,0.1,0.1, hidden}\n- [Leave](#out){rect=0.8,0.8,0.1,0.1}\n::\n# lib\n**A:** Books.\n::jump{to=map}\n# scarf\n**A:** A scarf.\n::jump{to=map}\n# out\n**A:** Bye.\n::end\n";

#[test]
fn a_hidden_hotspot_is_clickable_but_arrow_keys_skip_it() {
    let p = parse(HIDDEN_MAP).unwrap();
    let mut g = new_game(&p, instant());
    key(&mut g, Key::Down);
    assert_eq!(g.choice_focus, Some(0));
    key(&mut g, Key::Down);
    assert_eq!(g.choice_focus, Some(2), "skips the hidden one");
    key(&mut g, Key::Down);
    assert_eq!(g.choice_focus, Some(0));
    key(&mut g, Key::Up);
    assert_eq!(g.choice_focus, Some(2));
    key(&mut g, Key::Up);
    assert_eq!(g.choice_focus, Some(0));
    click_spot(&mut g, 1);
    assert_eq!(line(&g), "A scarf.");
}

#[test]
fn arrow_keys_do_nothing_when_every_hotspot_is_hidden() {
    let p = parse("# map\n::bg{src=map.png}\n::hotspots\n- [Scarf](#end){rect=0.6,0.7,0.1,0.1, hidden}\n::\n# end\n**A:** A scarf.\n::end\n").unwrap();
    let mut g = new_game(&p, instant());
    for k in [Key::Down, Key::Up, Key::Left, Key::Right] {
        key(&mut g, k);
        assert_eq!(g.choice_focus, None);
    }
    click_spot(&mut g, 0);
    assert_eq!(line(&g), "A scarf.");
}

#[test]
fn a_hotspot_screen_is_a_checkpoint_and_its_pick_is_not_in_the_backlog() {
    let p = parse(MAP).unwrap();
    let mut g = new_game(&p, instant());
    assert!(!has_file(&g, "auto-1.save"), "the first screen is where the game started");
    click_spot(&mut g, 0);
    key(&mut g, Key::Space); // back at the map
    assert!(has_file(&g, "auto-1.save"), "arriving at the map saved");
    key(&mut g, Key::Char('h'));
    let entries = g.backlog.clone();
    assert!(entries.iter().all(|e| matches!(e, quillstone_runtime::Logged::Line(_))), "{entries:?}");
    assert_eq!(entries.len(), 1);
}

#[test]
fn the_wheel_and_backspace_roll_back_over_hotspots_without_picking() {
    let p = parse(MAP).unwrap();
    let mut g = new_game(&p, instant());
    click_spot(&mut g, 0);
    key(&mut g, Key::Space);
    assert_eq!(g.view().hotspots.len(), 3);
    g.handle(Event::Wheel(-1.0)); // nothing to roll forward to
    assert_eq!(g.view().hotspots.len(), 3, "the wheel doesn't pick or advance");
    g.handle(Event::Wheel(1.0)); // back to the line "Books."
    assert_eq!(line(&g), "Books.");
    g.handle(Event::Wheel(1.0)); // back to the map
    assert_eq!(g.view().hotspots.len(), 3);
    assert!(g.view().dialogue.is_some());
    g.handle(Event::Wheel(-1.0));
    assert_eq!(line(&g), "Books.");
}

#[test]
fn skip_and_auto_stop_at_hotspots() {
    let p = parse(MAP).unwrap();
    let mut first = new_game(&p, instant());
    click_spot(&mut first, 0);
    key(&mut first, Key::Space);
    first.autosave();
    let mut g = reread(&p, &first, instant());
    click_spot(&mut g, 0);
    key(&mut g, Key::Tab);
    run(&mut g, 20);
    assert!(!g.skipping(), "skip stops at the hotspots");
    assert_eq!(g.view().hotspots.len(), 3, "back at the map, having skipped the read line");
}
