use super::*;
use quillstone_render::{Rect, TEXTBOX};
use crate::settings::QuickMenuSetting;
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
    click_at(&mut g, centre(quick_button_rect(TEXTBOX, QuickButton::Back)));
    assert_eq!(line(&g), "one");
    click_at(&mut g, centre(quick_button_rect(TEXTBOX, QuickButton::History)));
    assert_eq!(g.screen(), Screen::Backlog);
    key(&mut g, Key::Escape);
    click_at(&mut g, centre(quick_button_rect(TEXTBOX, QuickButton::Auto)));
    assert!(g.auto());
    click_at(&mut g, centre(quick_button_rect(TEXTBOX, QuickButton::Settings)));
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
    click_at(&mut g, centre(quillstone_render::view::choice_rects(&Theme::default(), 2)[0]));
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

// ---- {nw}: the story goes on by itself once the line is typed ------------------------------

const NW: &str = "**A:** one\n**A:** two{nw}\n**A:** three\n";

#[test]
fn a_no_wait_line_moves_on_once_typed_and_not_before() {
    let p = parse(NW).unwrap();
    let mut g = new_game(&p, Settings { text_speed: 10.0, ..Settings::default() });
    key(&mut g, Key::Space); // "one" is typed at once by the press
    key(&mut g, Key::Space);
    assert_eq!(line(&g), "two");
    assert!(g.needs_ticks());
    g.tick(0.2, false); // 2 of 3 characters
    assert_eq!(line(&g), "two");
    g.tick(0.2, false); // typed: it goes on without a click or the auto delay
    assert_eq!(line(&g), "three");
    assert!(!g.view().dialogue.as_ref().unwrap().auto_advance);
    g.tick(5.0, false);
    assert_eq!(line(&g), "three", "an ordinary line still waits");
}

#[test]
fn a_no_wait_line_with_instant_text_still_asks_for_ticks() {
    let p = parse(NW).unwrap();
    let mut g = new_game(&p, instant());
    key(&mut g, Key::Space);
    assert_eq!(line(&g), "two");
    assert!(g.needs_ticks(), "nothing is typing, but the line is about to move on");
    g.tick(0.0, false);
    assert_eq!(line(&g), "three");
}

#[test]
fn a_no_wait_line_ending_in_a_pause_waits_it_out_even_with_instant_text() {
    let src = "**A:** Well...{w=3}{nw}\n**A:** next\n";
    let p = parse(src).unwrap();
    for settings in [instant(), Settings { text_speed: 10.0, ..Settings::default() }] {
        let mut g = new_game(&p, settings);
        g.tick(1.5, false); // typed (if it types) and pausing
        assert_eq!(line(&g), "Well...");
        g.tick(1.0, false);
        assert_eq!(line(&g), "Well...", "still inside the 3 s");
        g.tick(1.5, false); // (typing at 10/s adds 0.7 s before the pause starts)
        assert_eq!(line(&g), "next");
    }
    // a click reveals all and cancels the pause: the story then moves on at once
    let mut g = new_game(&p, instant());
    g.tick(0.5, false);
    key(&mut g, Key::Space);
    g.tick(0.0, false);
    assert_eq!(line(&g), "next");
}

#[test]
fn a_click_on_a_no_wait_line_only_finishes_the_typing_then_it_moves_on() {
    let p = parse(NW).unwrap();
    let mut g = new_game(&p, Settings { text_speed: 10.0, ..Settings::default() });
    key(&mut g, Key::Space);
    key(&mut g, Key::Space);
    key(&mut g, Key::Space); // completes "two"; no second press needed
    assert_eq!(line(&g), "two");
    g.tick(0.0, false);
    assert_eq!(line(&g), "three");
}

#[test]
fn no_wait_does_not_run_under_a_menu_or_a_dialog() {
    let p = parse(NW).unwrap();
    let mut g = new_game(&p, instant());
    key(&mut g, Key::Space);
    key(&mut g, Key::Escape); // settings
    g.tick(10.0, false);
    assert_eq!(line(&g), "two");
    key(&mut g, Key::Escape);
    key(&mut g, Key::Char('s')); // the save screen
    g.tick(10.0, false);
    assert_eq!(line(&g), "two");
    key(&mut g, Key::Escape);
    key(&mut g, Key::Char('h')); // history
    g.tick(10.0, false);
    assert_eq!(line(&g), "two");
    key(&mut g, Key::Escape);
    g.tick(0.0, false);
    assert_eq!(line(&g), "three");

    // and a confirm dialog pauses it too
    let mut g = new_game(&p, instant());
    key(&mut g, Key::Space);
    g.ask("Sure?", Then::Quit);
    g.tick(10.0, false);
    assert_eq!(line(&g), "two");
}

#[test]
fn no_wait_lines_are_remembered_in_history_and_rollback_skips_them() {
    let p = parse("**A:** one\n**A:** two{nw}\n**A:** three\n").unwrap();
    let mut g = new_game(&p, instant());
    key(&mut g, Key::Space);
    g.tick(0.0, false);
    assert_eq!(line(&g), "three");
    let log: Vec<String> = g
        .session
        .backlog()
        .into_iter()
        .filter_map(|l| if let Logged::Line(l) = l { Some(quillstone_script::markup::plain(&l.text)) } else { None })
        .collect();
    assert_eq!(log, ["one", "two", "three"], "the tag is not shown in the history");
    key(&mut g, Key::Backspace);
    assert_eq!(line(&g), "one", "not onto the line that would move on again");
    g.tick(1.0, false);
    assert_eq!(line(&g), "one");
    key(&mut g, Key::Space); // forward again: it re-types "two" and goes on
    g.tick(0.0, false);
    assert_eq!(line(&g), "three");
}

#[test]
fn a_no_wait_line_before_a_choice_shows_the_line_then_the_choices() {
    let p = parse("**A:** hold on{nw}\n::choice\n- [Left](#l)\n- [Right](#r)\n::\n# l\n**A:** left\n# r\n**A:** right\n").unwrap();
    let mut g = new_game(&p, Settings { text_speed: 10.0, ..Settings::default() });
    assert_eq!(line(&g), "hold on");
    assert!(g.view().choices.is_empty(), "the line first");
    g.tick(0.5, false); // 7 characters take 0.7 s
    assert!(g.view().choices.is_empty());
    g.tick(0.3, false);
    assert_eq!(g.view().choices.len(), 2);
    assert_eq!(line(&g), "hold on", "the line stays under the choices");
}

#[test]
fn skip_mode_and_auto_forward_need_nothing_special_for_no_wait() {
    let p = parse("**A:** one\n**A:** two{nw}\n**A:** three\n**A:** four\n").unwrap();
    let mut g = new_game(&p, Settings { skip_unread: true, ..instant() });
    key(&mut g, Key::Tab);
    run(&mut g, 20);
    assert_eq!(line(&g), "four", "skipped straight through, once");

    let mut g = new_game(&p, Settings { auto_delay: 1.0, ..instant() });
    key(&mut g, Key::Char('a'));
    for _ in 0..10 {
        g.tick(100.0, false);
    }
    assert_eq!(line(&g), "four");
}

#[test]
fn the_staging_demo_plays_through() {
    let p = parse(include_str!("../../../../examples/staging.quill.md")).unwrap();
    let mut g = new_game(&p, instant());
    let first = g.take_audio();
    assert!(first.iter().any(|a| matches!(a, Action::PlaySfxLoop { src, .. } if src == "assets/calm.ogg")), "{first:?}");
    let (mut flashed, mut broke, mut dark_line, mut flipped, mut stretched) = (false, false, false, false, false);
    for _ in 0..80 {
        if !g.view().choices.is_empty() {
            break;
        }
        flashed |= flash_alpha(&g).is_some();
        broke |= line(&g).contains('\n');
        // the narration that comes over the fully black screen
        dark_line |= line(&g).starts_with("The screen is black") && cover_alpha(&g) == 1.0;
        flipped |= g.view().sprites.iter().any(|s| s.flip);
        stretched |= g.view().sprites.iter().any(|s| s.xscale > 1.0 && s.yscale < 1.0);
        key(&mut g, Key::Space);
        g.tick(0.0, false); // the `{nw}` line goes on by itself
    }
    assert!(flashed && broke, "the demo flashes and has a line with a {{p}} break");
    assert!(dark_line, "a line of narration shows over the black screen");
    assert!(flipped && stretched, "a sprite is flipped and a sprite is stretched");
    g.tick(2.0, false);
    assert_eq!(cover_alpha(&g), 0.0, "the dip is over by the end");
    assert_eq!(g.view().choices.len(), 2);
    assert!(g.take_audio().iter().any(|a| matches!(a, Action::StopSfx { src: Some(s), .. } if s == "assets/calm.ogg")));
    assert!(!g.mixer.has_sfx_loops());
    assert_eq!(line(&g), "This line moves on by itself once it is typed, with no click...");
    // two sprites, with Mika drawn over Ren (her z is higher by then)
    assert!(g.view().sprites.iter().all(|s| s.y <= 1.0 && s.z >= 0));
    key(&mut g, Key::Char('2'));
    assert!(line(&g).contains("That's all"));
}


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
    let t = track_rect(music, 8);
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
    click_at(&mut g, centre(control_rect(row, &ROWS)));
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
    click_at(&mut g, centre(button_rect(SettingsButton::Back, true, 8)));
    assert_eq!(g.screen(), Screen::Story);
    key(&mut g, Key::Escape);
    click_at(&mut g, centre(button_rect(SettingsButton::Quit, true, 8)));
    assert!(g.take_requests().is_empty(), "asks first");
    click_at(&mut g, centre(confirm_button_rect(false)));
    assert!(g.take_requests().is_empty(), "No: still playing");
    assert_eq!(g.screen(), Screen::Settings, "No goes back to Settings, not the story");
    click_at(&mut g, centre(button_rect(SettingsButton::Quit, true, 8)));
    click_at(&mut g, centre(confirm_button_rect(true)));
    assert_eq!(g.take_requests(), vec![Request::Quit]);
}

#[test]
fn a_confirm_launched_from_settings_returns_to_settings_on_no_and_proceeds_on_yes() {
    let p = parse(THREE).unwrap();
    for (button, yes_leaves) in [(SettingsButton::Quit, false), (SettingsButton::MainMenu, true)] {
        let mut g = new_game(&p, instant());
        key(&mut g, Key::Escape);
        click_at(&mut g, centre(button_rect(button, true, 8)));
        assert_eq!(g.screen(), Screen::Settings, "Settings stays open under the question");
        assert!(g.confirm.is_some());
        let list = g.draw_list(&|_| None);
        assert!(has_text(&list, "Settings") && has_text(&list, "Yes"), "both are drawn");
        click_at(&mut g, centre(confirm_button_rect(false)));
        assert!(g.confirm.is_none());
        assert_eq!(g.screen(), Screen::Settings, "{button:?}: No returns to Settings");
        assert!(g.in_story());
        // Escape in the dialog is a No as well
        click_at(&mut g, centre(button_rect(button, true, 8)));
        key(&mut g, Key::Escape);
        assert_eq!(g.screen(), Screen::Settings);
        // Yes proceeds
        click_at(&mut g, centre(button_rect(button, true, 8)));
        click_at(&mut g, centre(confirm_button_rect(true)));
        if yes_leaves {
            assert_eq!(g.screen(), Screen::MainMenu);
        } else {
            assert!(g.take_requests().contains(&Request::Quit));
        }
    }
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
    assert_eq!(g.backlog_scroll, 0, "three short lines all fit: nothing to scroll to");
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
    assert!(settings.iter().any(|d| matches!(d, Draw::Text { text, .. } if text == "Settings")));
}

fn has_text(list: &[Draw], what: &str) -> bool {
    list.iter().any(|d| matches!(d, Draw::Text { text, .. } if text == what))
}

/// Draw list of the game with a full-screen overlay opened by `open`, over a story that shows a
/// line, two choices and the quick menu.
#[test]
fn a_full_screen_overlay_hides_the_stories_text_choices_and_quick_menu() {
    let p = parse(CHOICE).unwrap();
    let none = |_: &str| None;
    type Open = fn(&mut Game);
    let opens: [(&str, Open); 4] = [
        ("settings", |g| key(g, Key::Escape)),
        ("history", |g| key(g, Key::Char('h'))),
        ("save", |g| key(g, Key::Char('s'))),
        ("load", |g| key(g, Key::Char('l'))),
    ];
    for (name, open) in opens {
        let mut g = new_game(&p, instant());
        key(&mut g, Key::Space); // on to the choices
        let story = g.draw_list(&none);
        for what in ["pick", "Left", "Right", "Skip", "Auto", "Q.Save"] {
            assert!(has_text(&story, what), "the story shows {what}");
        }
        open(&mut g);
        assert_ne!(g.screen(), Screen::Story, "{name} is open");
        let list = g.draw_list(&none);
        for what in ["pick", "Left", "Right", "Skip", "Auto", "Q.Save", "Q.Load"] {
            // the backlog lists the line itself; the slot screens have an "Auto" tab
            if (name == "history" && what == "pick") || (what == "Auto" && matches!(name, "save" | "load")) {
                continue;
            }
            assert!(!has_text(&list, what), "{name}: {what} must not show through");
        }
    }
}

#[test]
fn a_hotspot_overlay_leaves_no_tooltip_and_a_confirm_over_the_story_keeps_the_story() {
    let p = parse(MAP).unwrap();
    let mut g = new_game(&p, instant());
    let known = |_: &str| Some(quillstone_render::Size::new(100.0, 100.0));
    g.handle(Event::CursorMoved(spot_centre(&g, 0)));
    assert!(has_text(&g.draw_list(&known), "Library"), "the tooltip shows on the map");
    key(&mut g, Key::Escape);
    assert_eq!(g.screen(), Screen::Settings);
    assert!(!has_text(&g.draw_list(&known), "Library"), "no tooltip under settings");
    key(&mut g, Key::Escape);

    let q = parse(CHOICE).unwrap();
    let mut g = new_game(&q, instant());
    key(&mut g, Key::Space);
    g.ask("Sure?", Then::Quit);
    let list = g.draw_list(&|_| None);
    assert!(has_text(&list, "Sure?"));
    for what in ["pick", "Left", "Right", "Skip"] {
        assert!(has_text(&list, what), "the story stays under a confirm: {what}");
    }
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
    click_at(&mut g, centre(quick_button_rect(TEXTBOX, QuickButton::QuickSave)));
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
    click_at(&mut g, centre(quick_button_rect(TEXTBOX, QuickButton::QuickLoad)));
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
    click_at(&mut g, centre(button_rect(SettingsButton::Quit, true, 8)));
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
    click_at(&mut g, centre(quick_button_rect(TEXTBOX, QuickButton::Skip)));
    assert!(g.skipping(), "the quick menu button works too");
    click_at(&mut g, centre(quick_button_rect(TEXTBOX, QuickButton::Skip)));
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
    click_at(&mut g, centre(control_rect(row, &ROWS)));
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
    click_at(&mut g, centre(button_rect(SettingsButton::MainMenu, true, 8)));
    key(&mut g, Key::Char('y'));
    assert_eq!(g.screen(), Screen::MainMenu);
    let buttons = menu_buttons(g.newest_save.is_some(), false);
    assert_eq!(buttons[1], MenuButton::Continue);
    click_at(&mut g, centre(menu_button_rect(1)));
    assert_eq!(g.screen(), Screen::Story);
    assert_eq!(line(&g), "two", "no confirm needed: nothing to lose at the menu");
}

#[test]
fn an_overlay_opened_from_the_menu_hides_the_menus_title_and_buttons() {
    let p = parse(THREE).unwrap();
    let mut g = at_menu(&p);
    let none = |_: &str| None;
    let menu = g.draw_list(&none);
    for what in ["Start", "Settings", "Quit"] {
        assert!(has_text(&menu, what), "the menu shows {what}");
    }
    for (open, shown) in [(MenuButton::Settings, "Settings"), (MenuButton::Load, "Load")] {
        let at = menu_buttons(false, false).iter().position(|b| *b == open).unwrap();
        click_at(&mut g, centre(menu_button_rect(at)));
        let list = g.draw_list(&none);
        assert!(has_text(&list, shown));
        for what in ["Start", "Quillstone"] {
            assert!(!has_text(&list, what), "{shown}: {what} must not show through");
        }
        key(&mut g, Key::Escape);
        assert_eq!(g.screen(), Screen::MainMenu);
    }
}

#[test]
fn screens_opened_from_the_menu_return_to_it() {
    let p = parse(THREE).unwrap();
    let mut g = at_menu(&p);
    let buttons = menu_buttons(false, false);
    let settings = buttons.iter().position(|b| *b == MenuButton::Settings).unwrap();
    click_at(&mut g, centre(menu_button_rect(settings)));
    assert_eq!(g.screen(), Screen::Settings);
    let (x, y) = centre(button_rect(SettingsButton::MainMenu, true, 8)).unwrap();
    assert_eq!(settings_at(x, y, false, &ROWS), None, "no Main menu button here");
    key(&mut g, Key::Escape);
    assert_eq!(g.screen(), Screen::MainMenu);

    let load = buttons.iter().position(|b| *b == MenuButton::Load).unwrap();
    click_at(&mut g, centre(menu_button_rect(load)));
    assert_eq!(g.screen(), Screen::Load);
    click_at(&mut g, centre(slots_back_rect()));
    assert_eq!(g.screen(), Screen::MainMenu);

    click_at(&mut g, centre(menu_button_rect(settings)));
    click_at(&mut g, centre(button_rect(SettingsButton::Quit, false, 8)));
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
fn a_cutscene_of_the_background_video_file_replaces_it_and_ends() {
    let p = parse("::bg{src=a.webm}\n**A:** before\n::video{src=a.webm}\n**A:** after\n").unwrap();
    let mut g = new_game(&p, instant());
    assert_eq!(wanted(&g), vec![("a.webm".to_string(), true)]);
    key(&mut g, Key::Space);
    assert_eq!(wanted(&g), vec![("a.webm".to_string(), false)], "the looping background must not stand in for it");
    // `Videos` keys clips by file name: with both wanted, the loop was kept and never finished
    let mut videos = quillstone_video::Videos::new(Box::new(|_: &str| {
        Ok(Box::new(quillstone_video::TestPattern::new(16, 9, 10, 3)) as Box<dyn quillstone_video::Decoder>)
    }));
    videos.sync(&g.wanted_videos());
    let mut finished = false;
    for _ in 0..100 {
        videos.advance(std::time::Duration::from_millis(100));
        finished = videos.finished("a.webm");
        if finished {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    assert!(finished, "the cutscene finishes");
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
fn editing_an_included_file_takes_over_at_the_same_spot() {
    let load = |part: &'static str| quillstone_script::parse_files("# a\n**A:** one\n::include{p.quill.md}\n", "m.quill.md", &move |_| Ok(part.into())).unwrap();
    let before = load("# b\n**A:** two\n**A:** three\n");
    let mut g = new_game(&before, instant());
    key(&mut g, Key::Space);
    assert_eq!(line(&g), "two");
    let after = load("# b\n**A:** two\n**A:** three, in the included file\n");
    g.reload(&after).unwrap();
    assert_eq!(line(&g), "two");
    key(&mut g, Key::Space);
    assert_eq!(line(&g), "three, in the included file");
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
    fn load(&self, name: &str) -> Result<Option<String>, String> {
        self.mem.borrow().load(name)
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
fn an_unreadable_persistent_or_read_file_is_not_overwritten() {
    let p = parse("---\ngallery: [cg.png]\n---\n**A:** one\n::bg{src=cg.png}\n**A:** two\n**A:** three\n").unwrap();
    for (file, kept) in [("persistent.txt", None), ("persistent.txt", Some("bad magic\n")), ("read.txt", Some("junk\n"))] {
        let storage = Shared::default();
        match kept {
            Some(text) => drop(storage.mem.borrow_mut().files.insert(file.to_string(), text.to_string())),
            None => drop(storage.mem.borrow_mut().unreadable.insert(file.to_string())),
        }
        let mut g = Game::new(&p, instant(), Box::new(storage.clone()));
        assert!(notice(&g).is_some_and(|n| n.contains(file)), "{file}: {:?}", notice(&g));
        g.start_story();
        key(&mut g, Key::Space); // unlocks cg.png and reads a line
        key(&mut g, Key::Space);
        g.autosave();
        assert_eq!(storage.mem.borrow().files.get(file).map(String::as_str), kept, "{file} was left alone");
        let other = if file == "read.txt" { "persistent.txt" } else { "read.txt" };
        assert!(storage.read(other).is_some(), "the other file is still written");
    }
    // An empty file has nothing to lose: no notice, and it is written again.
    for file in ["persistent.txt", "read.txt"] {
        let storage = Shared::default();
        storage.mem.borrow_mut().files.insert(file.to_string(), String::new());
        let mut g = Game::new(&p, instant(), Box::new(storage.clone()));
        assert!(notice(&g).is_none(), "{file}: {:?}", notice(&g));
        g.start_story();
        key(&mut g, Key::Space);
        key(&mut g, Key::Space);
        g.autosave();
        assert!(storage.read(file).is_some_and(|t| !t.is_empty()), "{file} is written again");
    }
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
    assert!(g.view().dialogue.is_none(), "the map came before any line");
    g.handle(Event::Wheel(-1.0));
    assert_eq!(line(&g), "Books.");
}

#[test]
fn rolling_back_to_a_choice_before_any_line_shows_no_textbox() {
    let p = parse("::choice
- [Go](#a)
::
# a
**A:** Later.
").unwrap();
    let mut g = new_game(&p, instant());
    assert!(g.view().dialogue.is_none());
    key(&mut g, Key::Char('1'));
    assert_eq!(line(&g), "Later.");
    key(&mut g, Key::Backspace);
    assert!(g.view().dialogue.is_none(), "the line from after the choice is gone");
    assert_eq!(g.view().choices.len(), 1);
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

// ---- the inventory and shops ------------------------------------------------------------------------

const BAG: &str = "---\ninventory: bag\nvars: { bag: [key, lamp] }\n---\n::item{id=key name=\"Brass key\" icon=key.png price=15 text=\"Opens the tower.\"}\n::item{id=lamp name=\"Oil lamp\" text=\"Burns for hours.\"}\n**A:** one\n**A:** two\n";

fn drawn_texts(g: &Game) -> Vec<String> {
    g.draw_list(&|_| Some(quillstone_render::Size::new(64.0, 64.0)))
        .into_iter()
        .filter_map(|d| match d {
            quillstone_render::Draw::Text { text, .. } => Some(text),
            _ => None,
        })
        .collect()
}

fn cell(g: &Game, i: usize) -> Option<(f32, f32)> {
    centre(crate::bag::cell_rect(g.theme(), i))
}

fn click_cell(g: &mut Game, i: usize) {
    let at = cell(g, i);
    click_at(g, at);
}

fn click_bag(g: &mut Game, rect: fn(&quillstone_render::Theme) -> Rect) {
    let at = centre(rect(g.theme()));
    click_at(g, at);
}

#[test]
fn the_items_button_and_the_i_key_exist_only_in_games_with_an_inventory() {
    let with = parse(BAG).unwrap();
    let g = new_game(&with, instant());
    assert!(g.has_inventory() && drawn_texts(&g).iter().any(|t| t == "Items"));
    let without = parse(THREE).unwrap();
    let mut g = new_game(&without, instant());
    assert!(!g.has_inventory() && !drawn_texts(&g).iter().any(|t| t == "Items"));
    key(&mut g, Key::Char('i'));
    assert_eq!(g.screen(), Screen::Story, "I does nothing without an inventory");
    assert_eq!(line(&g), "one");
    // where the button would be, a click is an ordinary click on the story
    click_at(&mut g, centre(crate::screens::quick_items_rect(TEXTBOX)));
    assert_eq!(g.screen(), Screen::Story);
    assert_eq!(line(&g), "two");
}

#[test]
fn the_quick_menu_button_and_i_open_the_inventory_and_four_things_close_it() {
    let p = parse(BAG).unwrap();
    let mut g = new_game(&p, instant());
    click_at(&mut g, centre(crate::screens::quick_items_rect(TEXTBOX)));
    assert_eq!(g.screen(), Screen::Inventory);
    assert_eq!(line(&g), "one", "opening it doesn't advance the story");
    key(&mut g, Key::Escape);
    assert_eq!(g.screen(), Screen::Story);
    for closer in [0, 1, 2, 3] {
        key(&mut g, Key::Char('I'));
        assert_eq!(g.screen(), Screen::Inventory, "I opens it");
        match closer {
            0 => key(&mut g, Key::Char('i')),
            1 => g.handle(Event::Press(Button::Right)),
            2 => click_bag(&mut g, crate::bag::close_rect),
            _ => key(&mut g, Key::Escape),
        }
        assert_eq!(g.screen(), Screen::Story, "closer {closer}");
    }
    assert_eq!(line(&g), "one", "the story never moved");
    assert!(g.state().pending.is_some() && !g.state().finished);
}

#[test]
fn the_story_is_paused_while_the_inventory_is_over_it() {
    let p = parse(&format!("{}\n::wait{{time=1}}\n**A:** after\n", BAG.replace("**A:** two\n", ""))).unwrap();
    let mut g = new_game(&p, instant());
    key(&mut g, Key::Space); // past "one": the wait
    assert!(matches!(g.state().pending, Some(Cmd::Wait { .. })));
    key(&mut g, Key::Char('i'));
    g.tick(5.0, false);
    assert!(matches!(g.state().pending, Some(Cmd::Wait { .. })), "the wait doesn't run out under the inventory");
    assert!(g.media_paused());
    key(&mut g, Key::Escape);
    g.tick(5.0, false);
    assert_eq!(line(&g), "after");
}

#[test]
fn the_inventory_lists_the_items_and_describes_the_hovered_or_focused_one() {
    let p = parse(BAG).unwrap();
    let mut g = new_game(&p, instant());
    key(&mut g, Key::Char('i'));
    let texts = drawn_texts(&g);
    assert!(texts.contains(&"Items".to_string()) && texts.contains(&"Brass key".to_string()) && texts.contains(&"Oil lamp".to_string()));
    assert!(!texts.iter().any(|t| t.contains("Opens the tower")), "no description until one is pointed at");
    g.handle(Event::CursorMoved(cell(&g, 0)));
    let texts = drawn_texts(&g);
    assert!(texts.contains(&"Opens the tower.".to_string()) && texts.contains(&"Price: 15".to_string()), "{texts:?}");
    g.handle(Event::CursorMoved(cell(&g, 1)));
    let texts = drawn_texts(&g);
    assert!(texts.contains(&"Burns for hours.".to_string()) && !texts.iter().any(|t| t.starts_with("Price")), "{texts:?}");
    // the keyboard does the same
    g.handle(Event::CursorMoved(None));
    key(&mut g, Key::Right);
    assert!(drawn_texts(&g).contains(&"Opens the tower.".to_string()));
    key(&mut g, Key::Right);
    assert!(drawn_texts(&g).contains(&"Burns for hours.".to_string()));
    key(&mut g, Key::Right);
    assert!(drawn_texts(&g).contains(&"Burns for hours.".to_string()), "stops at the last item");
    key(&mut g, Key::Left);
    assert!(drawn_texts(&g).contains(&"Opens the tower.".to_string()));
    // view-only: clicking an item or pressing Enter changes nothing
    click_cell(&mut g, 1);
    key(&mut g, Key::Enter);
    assert_eq!(g.screen(), Screen::Inventory);
    assert_eq!(g.state().vars["bag"], quillstone_script::Value::List(vec!["key".into(), "lamp".into()]));
}

#[test]
fn an_empty_inventory_says_so_and_an_undeclared_item_shows_by_its_id() {
    let p = parse("---\ninventory: bag\nvars: { bag: [] }\n---\n**A:** one\n::set{bag += \"mystery\"}\n**A:** two\n").unwrap();
    let mut g = new_game(&p, instant());
    key(&mut g, Key::Char('i'));
    assert!(drawn_texts(&g).contains(&"Nothing here yet.".to_string()));
    key(&mut g, Key::Escape);
    key(&mut g, Key::Space);
    key(&mut g, Key::Char('i'));
    assert!(drawn_texts(&g).contains(&"mystery".to_string()));
}

#[test]
fn more_items_than_fit_are_paged_by_wheel_keys_and_arrows() {
    let items: Vec<String> = (1..=12).map(|i| format!("i{i}")).collect();
    let p = parse(&format!("---\ninventory: bag\nvars: {{ bag: [{}] }}\n---\n**A:** one\n", items.join(", "))).unwrap();
    let mut g = new_game(&p, instant());
    key(&mut g, Key::Char('i'));
    let per = crate::bag::per_page(g.theme());
    assert_eq!(per, 10);
    let texts = drawn_texts(&g);
    assert!(texts.contains(&"i10".to_string()) && !texts.contains(&"i11".to_string()) && texts.contains(&"Page 1 of 2".to_string()));
    g.handle(Event::Wheel(-1.0));
    let texts = drawn_texts(&g);
    assert!(texts.contains(&"i11".to_string()) && texts.contains(&"i12".to_string()) && !texts.contains(&"i1".to_string()) && texts.contains(&"Page 2 of 2".to_string()));
    g.handle(Event::Wheel(-1.0));
    assert!(drawn_texts(&g).contains(&"Page 2 of 2".to_string()), "stops at the last page");
    g.handle(Event::Wheel(1.0));
    assert!(drawn_texts(&g).contains(&"Page 1 of 2".to_string()));
    key(&mut g, Key::PageDown);
    assert!(drawn_texts(&g).contains(&"i11".to_string()));
    key(&mut g, Key::PageUp);
    click_bag(&mut g, crate::bag::next_rect);
    assert!(drawn_texts(&g).contains(&"i12".to_string()));
    click_bag(&mut g, crate::bag::prev_rect);
    assert!(drawn_texts(&g).contains(&"i1".to_string()));
    // arrows walk off the end of a page onto the next one
    for _ in 0..per {
        key(&mut g, Key::Right);
    }
    let texts = drawn_texts(&g);
    assert!(texts.contains(&"Page 1 of 2".to_string()), "ten presses leave the focus on i10: {texts:?}");
    key(&mut g, Key::Right);
    let texts = drawn_texts(&g);
    assert!(texts.contains(&"Page 2 of 2".to_string()) && texts.contains(&"i11".to_string()), "{texts:?}");
    key(&mut g, Key::Up);
    assert!(drawn_texts(&g).contains(&"Page 1 of 2".to_string()), "up from the first cell of page two goes back");
}

#[test]
fn down_on_the_last_row_stays_put_when_the_column_has_nothing_below() {
    let items: String = (0..7).map(|i| format!("::item{{id=i{i} name=\"i{i}\"}}\n")).collect();
    let src = format!("---\ninventory: bag\nvars: {{ bag: [i0, i1, i2, i3, i4, i5, i6] }}\n---\n{items}**A:** hi\n");
    let p = parse(&src).unwrap();
    let mut g = new_game(&p, instant());
    key(&mut g, Key::Char('i'));
    assert_eq!(crate::bag::columns(g.theme()), 5);
    key(&mut g, Key::Right);
    key(&mut g, Key::Right);
    key(&mut g, Key::Right); // item 2, first row
    assert_eq!(g.bag.focus, Some(2));
    key(&mut g, Key::Down);
    assert_eq!(g.bag.focus, Some(2), "no item under column 2");
    key(&mut g, Key::Left); // item 1
    key(&mut g, Key::Down);
    assert_eq!(g.bag.focus, Some(6), "item 6 is under item 1");
    key(&mut g, Key::Down);
    assert_eq!(g.bag.focus, Some(6), "the last row");
}

const SCRIPTED: &str = "---\ninventory: bag\nvars: { bag: [key] }\n---\n::item{id=key name=\"Brass key\" text=\"Opens it.\"}\n**A:** one\n::inventory\n**A:** two\n::inventory\n**A:** three\n";

#[test]
fn the_inventory_command_opens_the_screen_and_the_story_waits_for_it_to_close() {
    let p = parse(SCRIPTED).unwrap();
    for closer in 0..2 {
        let mut g = new_game(&p, instant());
        key(&mut g, Key::Space);
        assert_eq!(g.screen(), Screen::Inventory, "the script opened it");
        assert!(matches!(g.state().pending, Some(Cmd::ShowInventory)));
        g.tick(9.0, false);
        assert_eq!(g.screen(), Screen::Inventory, "it stays until the player closes it");
        match closer {
            0 => key(&mut g, Key::Char('i')),
            _ => click_bag(&mut g, crate::bag::close_rect),
        }
        assert_eq!(g.screen(), Screen::Story, "closer {closer}");
        assert_eq!(line(&g), "two", "the story carried on");
    }
}

#[test]
fn a_script_inventory_survives_save_load_rollback_and_a_reload() {
    let p = parse(SCRIPTED).unwrap();
    let mut g = new_game(&p, instant());
    key(&mut g, Key::Space);
    assert_eq!(g.screen(), Screen::Inventory);
    g.quick_save();
    click_bag(&mut g, crate::bag::close_rect);
    assert_eq!(line(&g), "two");
    // loading puts the screen back
    g.quick_load();
    key(&mut g, Key::Char('y'));
    assert_eq!(g.screen(), Screen::Inventory, "the save was made at the inventory");
    assert!(drawn_texts(&g).contains(&"Brass key".to_string()));
    // hot reload keeps it open
    g.reload(&p).unwrap();
    assert_eq!(g.screen(), Screen::Inventory);
    // closing then going back lands on it again
    click_bag(&mut g, crate::bag::close_rect);
    key(&mut g, Key::Backspace);
    assert_eq!(g.screen(), Screen::Inventory);
    key(&mut g, Key::Char('i'));
    assert_eq!(line(&g), "two");
    // a reload that removes the inventory command lets the story go on
    key(&mut g, Key::Space);
    assert_eq!(g.screen(), Screen::Inventory, "the second `::inventory`");
    let edited = parse(&SCRIPTED.replace("::inventory\n**A:** three", "**A:** three")).unwrap();
    g.reload(&edited).unwrap();
    assert_eq!(g.screen(), Screen::Story);
}

const SHOPPING: &str = "---\ninventory: bag\nvars: { gold: 40, bag: [] }\n---\n::item{id=key name=\"Brass key\" price=15 text=\"Opens it.\"}\n::item{id=lamp name=\"Oil lamp\" price=30 text=\"Bright.\"}\n::item{id=map price=5}\n# a\n**A:** Welcome.\n::shop{pay=gold into=bag}\n- key\n- lamp\n- map\n::\n**A:** Bye.\n";

#[test]
fn a_shop_shows_prices_money_and_states_and_stops_the_story() {
    let p = parse(SHOPPING).unwrap();
    let mut g = new_game(&p, instant());
    key(&mut g, Key::Space);
    assert_eq!(g.screen(), Screen::Shop);
    let texts = drawn_texts(&g);
    for want in ["Shop", "Gold: 40", "Brass key", "15 gold", "Oil lamp", "30 gold", "map", "5 gold", "Leave"] {
        assert!(texts.contains(&want.to_string()), "{want} in {texts:?}");
    }
    g.tick(9.0, false);
    assert_eq!(g.screen(), Screen::Shop);
    g.handle(Event::CursorMoved(cell(&g, 1)));
    let texts = drawn_texts(&g);
    assert!(texts.contains(&"Bright.".to_string()) && texts.contains(&"Price: 30 gold".to_string()), "{texts:?}");
}

#[test]
fn clicking_buys_at_once_and_the_screen_stays_open_and_refreshes() {
    let p = parse(SHOPPING).unwrap();
    let mut g = new_game(&p, instant());
    key(&mut g, Key::Space);
    click_cell(&mut g, 0);
    assert_eq!(g.screen(), Screen::Shop);
    assert_eq!(notice(&g).as_deref(), Some("Bought Brass key"));
    assert_eq!(g.state().vars["gold"], quillstone_script::Value::Int(25));
    let texts = drawn_texts(&g);
    assert!(texts.contains(&"Gold: 25".to_string()) && texts.contains(&"Owned".to_string()), "{texts:?}");
    assert!(!texts.contains(&"15 gold".to_string()), "an owned item shows no price");
    // owned: nothing happens, with a notice
    click_cell(&mut g, 0);
    assert_eq!(g.state().vars["gold"], quillstone_script::Value::Int(25));
    assert_eq!(notice(&g).as_deref(), Some("Already owned"));
    // too dear: nothing happens, with a notice
    click_cell(&mut g, 1);
    assert_eq!(g.state().vars["gold"], quillstone_script::Value::Int(25));
    assert_eq!(notice(&g).as_deref(), Some("Not enough gold"));
    assert_eq!(g.state().vars["bag"], quillstone_script::Value::List(vec!["key".into()]));
    // and a cheap one goes through
    click_cell(&mut g, 2);
    assert_eq!(g.state().vars["gold"], quillstone_script::Value::Int(20));
    assert_eq!(notice(&g).as_deref(), Some("Bought map"));
    assert!(drawn_texts(&g).contains(&"Gold: 20".to_string()));
}

#[test]
fn the_keyboard_buys_the_focused_item_and_the_focus_stays_put() {
    let p = parse(SHOPPING).unwrap();
    let mut g = new_game(&p, instant());
    key(&mut g, Key::Space);
    key(&mut g, Key::Enter);
    assert_eq!(g.state().vars["gold"], quillstone_script::Value::Int(40), "nothing focused yet");
    key(&mut g, Key::Right);
    key(&mut g, Key::Right);
    key(&mut g, Key::Left);
    key(&mut g, Key::Left);
    key(&mut g, Key::Enter);
    assert_eq!(g.state().vars["gold"], quillstone_script::Value::Int(25));
    key(&mut g, Key::Enter);
    assert_eq!(notice(&g).as_deref(), Some("Already owned"));
    key(&mut g, Key::Right);
    key(&mut g, Key::Space);
    assert_eq!(notice(&g).as_deref(), Some("Not enough gold"));
}

#[test]
fn leaving_a_shop_is_the_button_only() {
    let p = parse(SHOPPING).unwrap();
    let mut g = new_game(&p, instant());
    key(&mut g, Key::Space);
    click_cell(&mut g, 0);
    click_bag(&mut g, crate::bag::close_rect);
    assert_eq!(g.screen(), Screen::Story);
    assert_eq!(line(&g), "Bye.");
    assert_eq!(g.state().vars["gold"], quillstone_script::Value::Int(25), "what was bought stays bought");
    key(&mut g, Key::Char('h'));
    let entries = g.backlog.clone();
    assert!(entries.iter().all(|e| matches!(e, quillstone_runtime::Logged::Line(_))), "{entries:?}");
}

#[test]
fn backspace_leaves_a_shop_so_a_gamepad_can() {
    let p = parse(SHOPPING).unwrap();
    let mut g = new_game(&p, instant());
    key(&mut g, Key::Space);
    assert_eq!(g.screen(), Screen::Shop);
    key(&mut g, Key::Backspace);
    assert_eq!(g.screen(), Screen::Story);
    assert_eq!(line(&g), "Bye.");
}

#[test]
fn escape_and_right_click_in_a_shop_open_settings_and_closing_them_returns_to_the_shop() {
    let p = parse(SHOPPING).unwrap();
    for how in 0..4 {
        let mut g = new_game(&p, instant());
        key(&mut g, Key::Space);
        click_cell(&mut g, 0); // buy the key
        key(&mut g, Key::Right);
        let (page, focus) = (g.bag.page, g.bag.focus);
        assert!(focus.is_some());
        let open = |g: &mut Game| {
            if how % 2 == 0 {
                key(g, Key::Escape)
            } else {
                g.handle(Event::Press(Button::Right))
            }
        };
        open(&mut g);
        assert_eq!(g.screen(), Screen::Settings, "how {how}");
        assert!(matches!(g.state().pending, Some(Cmd::ShowShop { .. })), "still waiting at the shop");
        if how < 2 {
            open(&mut g); // the same key closes Settings
        } else {
            click_at(&mut g, centre(crate::screens::button_rect(crate::screens::SettingsButton::Back, true, 8)));
        }
        assert_eq!(g.screen(), Screen::Shop, "how {how}: back at the shop");
        assert_eq!((g.bag.page, g.bag.focus), (page, focus));
        assert_eq!(g.state().vars["gold"], quillstone_script::Value::Int(25), "the purchase is kept");
        assert!(drawn_texts(&g).contains(&"Owned".to_string()));
        click_bag(&mut g, crate::bag::close_rect);
        assert_eq!(line(&g), "Bye.");
    }
}

#[test]
fn the_story_keys_work_from_the_shop_and_a_script_inventory() {
    let p = parse(SHOPPING).unwrap();
    let mut g = new_game(&p, instant());
    key(&mut g, Key::Space);
    for (k, want) in [('h', Screen::Backlog), ('s', Screen::Save), ('l', Screen::Load)] {
        key(&mut g, Key::Char(k));
        assert_eq!(g.screen(), want);
        key(&mut g, Key::Escape);
        assert_eq!(g.screen(), Screen::Shop, "{k} closes back to the shop");
    }
    key(&mut g, Key::F5);
    assert!(has_file(&g, "quick-1.save"), "F5 saves at the shop");
    click_cell(&mut g, 0);
    key(&mut g, Key::F9);
    key(&mut g, Key::Char('y'));
    assert_eq!(g.screen(), Screen::Shop);
    assert_eq!(g.state().vars["gold"], quillstone_script::Value::Int(40), "F9 loaded the shop as saved");
    let p = parse(SCRIPTED).unwrap();
    let mut g = new_game(&p, instant());
    key(&mut g, Key::Space);
    assert_eq!(g.screen(), Screen::Inventory);
    key(&mut g, Key::Char('h'));
    key(&mut g, Key::Char('h'));
    assert_eq!(g.screen(), Screen::Inventory);
}

#[test]
fn the_inventory_overlay_still_closes_with_escape_and_right_click() {
    let p = parse(BAG).unwrap();
    for how in 0..2 {
        let mut g = new_game(&p, instant());
        key(&mut g, Key::Char('i'));
        assert_eq!(g.screen(), Screen::Inventory);
        if how == 0 {
            key(&mut g, Key::Escape)
        } else {
            g.handle(Event::Press(Button::Right))
        }
        assert_eq!(g.screen(), Screen::Story);
    }
}

#[test]
fn a_reload_with_settings_open_at_a_shop_keeps_settings_and_shows_the_shop_after() {
    let p = parse(SHOPPING).unwrap();
    // the story is at a shop and Settings is open over it
    let mut g = new_game(&p, instant());
    key(&mut g, Key::Space);
    click_cell(&mut g, 0);
    key(&mut g, Key::Right);
    let focus = g.bag.focus;
    key(&mut g, Key::Escape);
    g.settings.text_speed = 33.0; // an edit that must be flushed on close
    g.reload(&p).unwrap();
    assert_eq!(g.screen(), Screen::Settings, "reload leaves Settings open");
    assert_eq!(g.bag.focus, focus);
    key(&mut g, Key::Escape);
    assert_eq!(g.screen(), Screen::Shop, "the shop is back");
    assert!(g.take_requests().contains(&Request::SaveSettings), "settings were flushed on close");
    assert!(drawn_texts(&g).contains(&"Gold: 25".to_string()));
    click_bag(&mut g, crate::bag::close_rect);
    assert_eq!(line(&g), "Bye.");

    // Settings opened at "Bye." of a script without the shop; the reload brings the shop in
    let without = SHOPPING.replace("::shop{pay=gold into=bag}\n- key\n- lamp\n- map\n::\n", "");
    let p2 = parse(&without).unwrap();
    let mut g = new_game(&p2, instant());
    key(&mut g, Key::Space); // "Bye."
    key(&mut g, Key::Escape);
    g.reload(&p).unwrap();
    assert_eq!(g.screen(), Screen::Settings);
    key(&mut g, Key::Escape);
    assert_eq!(g.screen(), Screen::Shop, "the shop appears when Settings closes");
    // a reload that drops the shop while Settings is open: no shop afterwards
    key(&mut g, Key::Escape);
    g.reload(&p2).unwrap();
    assert_eq!(g.screen(), Screen::Settings);
    key(&mut g, Key::Escape);
    assert_eq!(g.screen(), Screen::Story);
}

#[test]
fn item_icons_are_wanted_only_for_a_shop_or_inventory_coming_up() {
    let mut src = String::from("---\ninventory: bag\nvars: { gold: 40, bag: [i0, i1] }\n---\n");
    for i in 0..150 {
        src.push_str(&format!("::item{{id=i{i} name=\"Item {i}\" price=5 icon=icons/i{i}.png}}\n"));
    }
    src.push_str("**A:** first\n");
    for n in 0..60 {
        src.push_str(&format!("**A:** filler {n}\n"));
    }
    src.push_str("::shop{pay=gold into=bag}\n- i7\n- i8\n::\n**A:** after\n::inventory\n");
    let p = parse(&src).unwrap();
    let mut g = new_game(&p, instant());
    assert_eq!(g.upcoming_images(), Vec::<&str>::new(), "the shop is further than the look-ahead: no icons at all");
    for _ in 0..30 {
        key(&mut g, Key::Space);
    }
    // the shop (and the inventory just after it) is within the 40-op window now: the
    // shop's two items and the two owned ones, not the other 146
    let wanted = g.upcoming_images();
    assert_eq!(wanted, ["icons/i7.png", "icons/i8.png", "icons/i0.png", "icons/i1.png"].as_slice());
    for _ in 0..31 {
        key(&mut g, Key::Space);
    }
    assert_eq!(g.screen(), Screen::Shop);
    click_cell(&mut g, 0); // buys i7
    click_bag(&mut g, crate::bag::close_rect);
    let wanted = g.upcoming_images();
    assert!(wanted.len() <= 3 && wanted.contains(&"icons/i7.png") && wanted.contains(&"icons/i0.png"), "{wanted:?}");
}

#[test]
fn a_shop_is_a_checkpoint_stops_skipping_and_a_purchase_is_not() {
    let p = parse(SHOPPING).unwrap();
    let mut first = new_game(&p, instant());
    key(&mut first, Key::Space);
    first.autosave();
    let mut g = reread(&p, &first, instant());
    assert!(!has_file(&g, "auto-1.save"));
    key(&mut g, Key::Tab);
    run(&mut g, 20);
    assert!(!g.skipping(), "skip stopped at the shop");
    assert_eq!(g.screen(), Screen::Shop);
    assert!(has_file(&g, "auto-1.save"), "arriving at the shop saved");
    click_cell(&mut g, 0);
    assert!(!has_file(&g, "auto-2.save"), "a purchase doesn't save again");
}

#[test]
fn a_shop_survives_save_load_rollback_and_hot_reload() {
    let p = parse(SHOPPING).unwrap();
    let mut g = new_game(&p, instant());
    key(&mut g, Key::Space);
    click_cell(&mut g, 0);
    g.quick_save();
    click_cell(&mut g, 2);
    assert_eq!(g.state().vars["gold"], quillstone_script::Value::Int(20));
    g.quick_load();
    key(&mut g, Key::Char('y'));
    assert_eq!(g.screen(), Screen::Shop);
    assert_eq!(g.state().vars["gold"], quillstone_script::Value::Int(25), "back at the quick save");
    assert!(drawn_texts(&g).contains(&"Owned".to_string()));
    // a reload keeps the shop, page and purchases
    g.reload(&p).unwrap();
    assert_eq!(g.screen(), Screen::Shop);
    assert!(drawn_texts(&g).contains(&"Gold: 25".to_string()));
    // leaving, then rolling back, lands in the shop again with the purchase kept
    click_bag(&mut g, crate::bag::close_rect);
    assert_eq!(line(&g), "Bye.");
    g.handle(Event::Wheel(1.0));
    assert_eq!(g.screen(), Screen::Shop);
    assert_eq!(g.state().vars["gold"], quillstone_script::Value::Int(25));
    // a reload that no longer has the shop there lets the story go on
    let edited = parse(&SHOPPING.replace("::shop{pay=gold into=bag}\n- key\n- lamp\n- map\n::\n", "")).unwrap();
    g.reload(&edited).unwrap();
    assert_eq!(g.screen(), Screen::Story);
}

#[test]
fn a_theme_currency_and_titles_show_in_the_screens() {
    let p = parse(SHOPPING).unwrap();
    let mut g = new_game(&p, instant());
    let (theme, problems) = quillstone_render::Theme::parse("shop_title = Night market\nshop_currency = Crowns\n");
    assert!(problems.is_empty());
    g.set_theme(theme);
    key(&mut g, Key::Space);
    let texts = drawn_texts(&g);
    for want in ["Night market", "Crowns: 40", "15 Crowns"] {
        assert!(texts.contains(&want.to_string()), "{want} in {texts:?}");
    }
    click_cell(&mut g, 1);
    assert_eq!(notice(&g).as_deref(), Some("Bought Oil lamp"));
    click_cell(&mut g, 0);
    assert_eq!(notice(&g).as_deref(), Some("Not enough Crowns"));
}

#[test]
fn the_market_demo_can_be_played_through_the_screens() {
    let p = parse(include_str!("../../../../examples/market.quill.md")).unwrap();
    let (theme, problems) = quillstone_render::Theme::parse(include_str!("../../../../examples/market_theme.txt"));
    assert!(problems.is_empty(), "{problems:?}");
    let mut g = new_game(&p, instant());
    g.set_theme(theme);
    key(&mut g, Key::Char('i'));
    assert!(drawn_texts(&g).contains(&"Nothing here yet.".to_string()), "an empty pack at the start");
    key(&mut g, Key::Char('i'));
    assert_eq!(g.screen(), Screen::Story);
    key(&mut g, Key::Space); // the opening line
    key(&mut g, Key::Char('1')); // greet the stallholder (polite)
    key(&mut g, Key::Space); // past the stallholder's line, into the shop
    assert_eq!(g.screen(), Screen::Shop);
    let texts = drawn_texts(&g);
    assert!(texts.contains(&"Night market".to_string()) && texts.contains(&"Gold: 40".to_string()));
    assert!(texts.contains(&"Lucky charm".to_string()) && texts.contains(&"18 Gold".to_string()), "polite customers see the charm, cheaper: {texts:?}");
    click_cell(&mut g, 3); // the charm: 18
    click_cell(&mut g, 0); // the key: 15, leaves 7
    assert_eq!(g.state().vars["gold"], quillstone_script::Value::Int(7));
    click_cell(&mut g, 1); // the lamp, 12: too dear
    assert_eq!(notice(&g).as_deref(), Some("Not enough Gold"));
    click_cell(&mut g, 0); // the key again: owned
    assert_eq!(notice(&g).as_deref(), Some("Already owned"));
    click_bag(&mut g, crate::bag::close_rect); // leave
    assert_eq!(g.screen(), Screen::Story);
    key(&mut g, Key::Space); // "You have 7 gold left..." -> the script's own ::inventory
    assert_eq!(g.screen(), Screen::Inventory);
    let texts = drawn_texts(&g);
    assert!(texts.contains(&"Your pack".to_string()) && texts.contains(&"Brass key".to_string()) && texts.contains(&"Lucky charm".to_string()));
    assert!(!texts.contains(&"Oil lamp".to_string()));
    key(&mut g, Key::Char('i'));
    assert_eq!(g.screen(), Screen::Story);
    assert!(line(&g).contains("brass key turns"), "{}", line(&g));
    key(&mut g, Key::Space);
    assert!(line(&g).contains("charm is warm"), "no lamp bought: {}", line(&g));
    key(&mut g, Key::Space);
    key(&mut g, Key::Char('1')); // climb the tower: only offered with the key
    key(&mut g, Key::Space);
    assert!(line(&g).contains("worth the gold"));
}

// ---- ::flash, looping sounds ------------------------------------------------------------------

/// The opacity of the white full-screen flash in the draw list, if one is drawn.
fn flash_alpha(g: &Game) -> Option<f32> {
    let screen = Rect::new(0.0, 0.0, quillstone_render::VIRTUAL.w, quillstone_render::VIRTUAL.h);
    g.draw_list(&|_| None).iter().find_map(|d| match d {
        Draw::Rect { rect, color } if *rect == screen && color[..3] == [1.0, 1.0, 1.0] => Some(color[3]),
        _ => None,
    })
}

#[test]
fn a_flash_fades_on_its_own_and_needs_ticks_while_it_does() {
    let p = parse("::flash{time=1}\n**A:** one\n**A:** two\n").unwrap();
    let mut g = new_game(&p, instant());
    assert_eq!(flash_alpha(&g), Some(1.0));
    assert!(g.needs_ticks(), "a fading flash must keep the frames coming");
    g.tick(0.5, false);
    assert!((flash_alpha(&g).unwrap() - 0.5).abs() < 1e-5);
    assert_eq!(line(&g), "one", "it never blocks or advances the story");
    g.tick(0.6, false);
    assert_eq!(flash_alpha(&g), None);
    assert!(!g.needs_ticks());
}

#[test]
fn a_flash_freezes_under_menus_like_other_timers() {
    let p = parse("::flash{time=1}\n**A:** one\n").unwrap();
    let mut g = new_game(&p, instant());
    g.tick(0.25, false);
    key(&mut g, Key::Escape); // settings
    assert_eq!(g.screen(), Screen::Settings);
    for _ in 0..20 {
        g.tick(1.0, false);
    }
    key(&mut g, Key::Escape);
    assert_eq!(g.screen(), Screen::Story);
    assert!((flash_alpha(&g).unwrap() - 0.75).abs() < 1e-5, "still where it was");
}

#[test]
fn a_flash_is_not_replayed_by_rollback_and_skip_ends_it_at_once() {
    let p = parse("**A:** one\n::flash{time=5}\n**A:** two\n::flash{time=5}\n**A:** three\n").unwrap();
    let mut g = new_game(&p, instant());
    key(&mut g, Key::Space);
    assert!(flash_alpha(&g).is_some());
    key(&mut g, Key::Backspace);
    assert_eq!(line(&g), "one");
    assert_eq!(flash_alpha(&g), None, "going back doesn't flash again");

    let mut g = new_game(&p, Settings { skip_unread: true, ..instant() });
    key(&mut g, Key::Tab);
    run(&mut g, 1); // one step: the first flash starts and is cut short
    assert!(g.skipping());
    assert_eq!(flash_alpha(&g), None);
    run(&mut g, 10);
    assert_eq!(flash_alpha(&g), None, "and the second");
}

const RAIN: &str = "::sfx{src=rain.ogg loop volume=0.5}\n**A:** one\n::sfx{src=door.ogg}\n**A:** two\n**A:** three\n::sfx{stop=rain.ogg}\n**A:** four\n::choice\n- [Go](#go)\n::\n# go\n**A:** end\n";

#[test]
fn skipping_mutes_one_shots_but_loops_stay_in_step_with_the_story() {
    let p = parse(RAIN).unwrap();
    let mut g = new_game(&p, Settings { skip_unread: true, ..instant() });
    let start = g.take_audio();
    assert!(start.iter().any(|a| matches!(a, Action::PlaySfxLoop { src, volume, .. } if src == "rain.ogg" && *volume == 0.5)));
    key(&mut g, Key::Tab);
    run(&mut g, 2);
    // mid-way: the door is muted, the rain is still looping
    let mid = g.take_audio();
    assert!(!mid.iter().any(|a| matches!(a, Action::PlaySfx { .. })), "{mid:?}");
    assert!(g.mixer.has_sfx_loops() && g.session.state().sfx_loops.len() == 1);
    run(&mut g, 10); // on to the choice, past the stop
    let end = g.take_audio();
    assert!(end.iter().any(|a| matches!(a, Action::StopSfx { src: Some(s), .. } if s == "rain.ogg")), "{end:?}");
    assert!(!g.mixer.has_sfx_loops() && g.session.state().sfx_loops.is_empty(), "audio and story agree");
    // rolling back to before the stop restarts it exactly once
    key(&mut g, Key::Backspace); // "four": the rain had already stopped
    assert!(!g.mixer.has_sfx_loops());
    key(&mut g, Key::Backspace); // "three"
    let back = g.take_audio();
    assert_eq!(back.iter().filter(|a| matches!(a, Action::PlaySfxLoop { .. })).count(), 1, "{back:?}");
    assert!(g.mixer.has_sfx_loops());
}

#[test]
fn a_loop_started_while_skipping_is_not_lost() {
    let p = parse("**A:** one\n::sfx{src=rain.ogg loop}\n::sfx{src=door.ogg}\n**A:** two\n::choice\n- [Go](#go)\n::\n# go\n**A:** end\n").unwrap();
    let mut g = new_game(&p, Settings { skip_unread: true, ..instant() });
    g.take_audio();
    key(&mut g, Key::Tab);
    run(&mut g, 10);
    let audio = g.take_audio();
    assert_eq!(audio.iter().filter(|a| matches!(a, Action::PlaySfxLoop { .. })).count(), 1, "{audio:?}");
    assert!(!audio.iter().any(|a| matches!(a, Action::PlaySfx { .. })));
    assert!(g.mixer.has_sfx_loops());
    // and rolling back (a redraw) neither doubles nor drops it
    key(&mut g, Key::Backspace);
    assert!(g.take_audio().iter().all(|a| !matches!(a, Action::PlaySfxLoop { .. } | Action::StopSfx { .. } | Action::StopSfxLoop { .. })));
}

#[test]
fn leaving_for_the_main_menu_ends_the_stories_loops() {
    let p = parse("::sfx{src=rain.ogg loop}\n**A:** only\n").unwrap();
    let mut g = new_game(&p, instant());
    assert!(g.mixer.has_sfx_loops());
    g.take_audio();
    key(&mut g, Key::Space); // the last line
    key(&mut g, Key::Space); // the end: back to the menu
    assert_eq!(g.screen(), Screen::MainMenu);
    assert!(!g.mixer.has_sfx_loops());
    assert!(g.take_audio().iter().any(|a| matches!(a, Action::StopSfx { src: None, .. })));
}

#[test]
fn a_loop_the_backend_failed_to_start_is_tried_again_on_a_redraw() {
    let p = parse("::sfx{src=rain.ogg loop volume=0.5}\n**A:** one\n**A:** two\n").unwrap();
    let mut g = new_game(&p, instant());
    assert!(g.take_audio().iter().any(|a| matches!(a, Action::PlaySfxLoop { .. })));
    key(&mut g, Key::Space);
    assert!(g.take_audio().is_empty(), "nothing to do while the mixer thinks it plays");
    g.loop_failed("rain.ogg"); // the file is missing, say
    assert!(!g.mixer.has_sfx_loops());
    key(&mut g, Key::Backspace); // a redraw lists the loop again
    let audio = g.take_audio();
    assert_eq!(audio.iter().filter(|a| matches!(a, Action::PlaySfxLoop { src, .. } if src == "rain.ogg")).count(), 1, "{audio:?}");
}

#[test]
fn a_redraw_that_drops_a_loop_stops_only_the_loop() {
    let p = parse("**A:** one\n::sfx{src=rain.ogg loop}\n::sfx{src=rain.ogg}\n**A:** two\n").unwrap();
    let mut g = new_game(&p, instant());
    g.take_audio();
    key(&mut g, Key::Space);
    g.take_audio();
    key(&mut g, Key::Backspace); // before the loop started
    let audio = g.take_audio();
    assert!(audio.iter().any(|a| matches!(a, Action::StopSfxLoop { src, .. } if src == "rain.ogg")), "{audio:?}");
    assert!(!audio.iter().any(|a| matches!(a, Action::StopSfx { .. })), "{audio:?}");
}

// ---- keyboard focus beats a resting mouse -------------------------------------------------------

#[test]
fn a_key_that_moves_the_focus_silences_the_mouse_hover_until_the_mouse_moves() {
    let p = parse(BAG).unwrap();
    let mut g = new_game(&p, instant());
    key(&mut g, Key::Char('i'));
    g.handle(Event::CursorMoved(cell(&g, 1)));
    assert!(drawn_texts(&g).contains(&"Burns for hours.".to_string()));
    key(&mut g, Key::Right); // focus lands on the first item while the mouse rests on the second
    let texts = drawn_texts(&g);
    assert!(texts.contains(&"Opens the tower.".to_string()), "the description follows the keyboard: {texts:?}");
    assert!(!texts.contains(&"Burns for hours.".to_string()), "{texts:?}");
    g.handle(Event::CursorMoved(cell(&g, 1)));
    assert!(drawn_texts(&g).contains(&"Burns for hours.".to_string()), "the mouse moved: it points again");
}

#[test]
fn every_screen_with_hover_and_focus_lets_the_keyboard_win() {
    let mouse = Some((300.0, 300.0));
    // choices
    let p = parse(CHOICE).unwrap();
    let mut g = new_game(&p, instant());
    key(&mut g, Key::Space);
    g.handle(Event::CursorMoved(mouse));
    assert_eq!(g.pointer(), mouse);
    key(&mut g, Key::Down);
    assert_eq!(g.pointer(), None, "choices");
    g.handle(Event::CursorMoved(mouse));
    assert_eq!(g.pointer(), mouse);
    // a key that moves no focus keeps the hover
    key(&mut g, Key::Char('x'));
    assert_eq!(g.pointer(), mouse);
    // slots
    let p = parse(THREE).unwrap();
    let mut g = new_game(&p, instant());
    key(&mut g, Key::Char('s'));
    g.handle(Event::CursorMoved(mouse));
    key(&mut g, Key::Down);
    assert_eq!(g.pointer(), None, "slots");
    // settings
    let mut g = new_game(&p, instant());
    key(&mut g, Key::Escape);
    g.handle(Event::CursorMoved(mouse));
    key(&mut g, Key::Down);
    assert_eq!(g.pointer(), None, "settings");
    // the main menu
    let mut g = at_menu(&p);
    g.handle(Event::CursorMoved(mouse));
    key(&mut g, Key::Down);
    assert_eq!(g.pointer(), None, "menu");
    // extras
    let p = parse("---\ngallery: [a.png, b.png]\n---\n**A:** hi\n").unwrap();
    let mut g = at_menu(&p);
    open_extras(&mut g);
    g.handle(Event::CursorMoved(mouse));
    key(&mut g, Key::Right);
    assert_eq!(g.pointer(), None, "extras");
}

#[test]
fn a_click_still_lands_where_the_mouse_is_after_the_keyboard_moved_the_focus() {
    let p = parse(BAG).unwrap();
    let mut g = new_game(&p, instant());
    key(&mut g, Key::Char('i'));
    g.handle(Event::CursorMoved(cell(&g, 1)));
    key(&mut g, Key::Right);
    assert_eq!(g.pointer(), None);
    assert!(g.cursor.is_some(), "the real position is kept for clicks");
}

// ---- hotspots over the textbox and the quick menu ------------------------------------------------------

/// A line on screen, then two regions: one over the quick menu's Settings button
/// (and the strip around it), one inside the textbox.
const OVER_UI: &str = "# map\n**A:** Where to?\n::hotspots\n- [Over Settings](#out){rect=0.8385,0.9630,0.0781,0.0370}\n- [In the box](#out){rect=0.3,0.75,0.2,0.2}\n::\n# out\n**A:** Bye.\n::end\n";

#[test]
fn a_hotspot_wins_over_the_quick_menu_where_they_overlap_and_the_menu_works_elsewhere() {
    let p = parse(OVER_UI).unwrap();
    let mut g = new_game(&p, instant());
    key(&mut g, Key::Space); // past the line: the regions appear, the line stays
    let settings = centre(quick_button_rect(TEXTBOX, QuickButton::Settings));
    let spot = centre(quillstone_render::view::hotspot_rect(&g.view().hotspots[0]));
    assert!(quick_button_rect(TEXTBOX, QuickButton::Settings).contains(spot.unwrap().0, spot.unwrap().1), "the region overlaps the button");
    click_at(&mut g, settings);
    assert_eq!(g.screen(), Screen::Story, "Settings did not open");
    assert_eq!(line(&g), "Bye.", "the region was picked");

    let p = parse(OVER_UI).unwrap();
    let mut g = new_game(&p, instant());
    key(&mut g, Key::Space); // past the line: the regions appear, the line stays
    click_at(&mut g, centre(quick_button_rect(TEXTBOX, QuickButton::History)));
    assert_eq!(g.screen(), Screen::Backlog, "a quick-menu button with no region over it still works");
}

#[test]
fn a_hotspot_inside_the_textbox_is_clickable_and_drawn_over_the_textbox() {
    let p = parse(OVER_UI).unwrap();
    let mut g = new_game(&p, instant());
    key(&mut g, Key::Space); // past the line: the regions appear, the line stays
    let at = centre(quillstone_render::view::hotspot_rect(&g.view().hotspots[1]));
    g.handle(Event::CursorMoved(at));
    let list = g.draw_list(&|_| None);
    let box_at = list.iter().position(|d| matches!(d, quillstone_render::Draw::Rect { rect, .. } if *rect == quillstone_render::TEXTBOX)).unwrap();
    let tint_at = list
        .iter()
        .position(|d| matches!(d, quillstone_render::Draw::Rect { rect, color } if *rect == quillstone_render::view::hotspot_rect(&g.view().hotspots[1]) && color[3] < 0.5))
        .expect("the hovered region is tinted");
    assert!(tint_at > box_at, "tint at {tint_at}, textbox at {box_at}");
    g.handle(Event::Press(Button::Left));
    assert_eq!(line(&g), "Bye.");
}

// ---- the auto and skip badge -----------------------------------------------------------------------

fn badge(g: &Game) -> Option<String> {
    g.draw_list(&|_| None).into_iter().find_map(|d| match d {
        Draw::Text { text, .. } if text.starts_with("AUTO") || text.starts_with("SKIP") => Some(text),
        _ => None,
    })
}

#[test]
fn the_mode_badge_shows_auto_and_skip_and_the_old_skipping_label_is_gone() {
    let p = parse(THREE).unwrap();
    let mut g = new_game(&p, Settings { skip_unread: true, ..instant() });
    assert_eq!(badge(&g), None);
    key(&mut g, Key::Char('a'));
    assert_eq!(badge(&g).as_deref(), Some("AUTO"));
    key(&mut g, Key::Tab);
    assert!(g.skipping());
    assert_eq!(badge(&g).as_deref(), Some("AUTO  SKIP »"));
    key(&mut g, Key::Char('a'));
    assert_eq!(badge(&g).as_deref(), Some("SKIP »"));
    assert!(!has_text(&g.draw_list(&|_| None), "Skipping »"), "one indicator, not two");
    // inside the 40 px margin, top right
    let r = mode_badge_rect("SKIP »");
    assert!(r.x + r.w <= 1880.0 + 1e-3 && r.y >= 30.0 && r.x > 960.0);
}

#[test]
fn the_mode_badge_shows_where_there_is_no_quick_menu_and_hides_under_overlays() {
    let p = parse(MAP).unwrap(); // regions and no line: no quick menu
    let mut g = new_game(&p, instant());
    assert!(!has_text(&g.draw_list(&|_| None), "Settings"), "no quick menu here");
    key(&mut g, Key::Char('a'));
    assert!(g.auto());
    assert_eq!(badge(&g).as_deref(), Some("AUTO"));
    g.ask("Sure?", Then::Quit);
    assert_eq!(badge(&g).as_deref(), Some("AUTO"), "still shown over the story under a confirm");
    g.confirm = None;
    key(&mut g, Key::Escape);
    assert_eq!(g.screen(), Screen::Settings);
    assert_eq!(badge(&g), None, "a full-screen overlay hides the story and its badge");
    key(&mut g, Key::Escape);
    let m = parse(THREE).unwrap();
    let mut g = Game::new(&m, instant(), Box::new(crate::MemoryStorage::default()));
    assert_eq!(badge(&g), None, "not at the main menu");
    g.start_story();
}

// ---- quick menu modes, hide the interface, ::textbox --------------------------------

fn quick_theme(mode: quillstone_render::QuickMenu) -> Theme {
    Theme { quick_menu: mode, ..Theme::default() }
}

fn with_theme<'p>(p: &'p Program, settings: Settings, theme: Theme) -> Game<'p> {
    let mut g = new_game(p, settings);
    g.set_theme(theme);
    g
}

fn drawn(g: &Game, what: &str) -> bool {
    has_text(&g.draw_list(&|_| None), what)
}

#[test]
fn hover_mode_shows_the_quick_menu_with_a_fade_and_gates_its_clicks() {
    use quillstone_render::QuickMenu;
    let p = parse(THREE).unwrap();
    let mut g = with_theme(&p, instant(), quick_theme(QuickMenu::Hover));
    g.tick(1.0, false);
    assert!(!drawn(&g, "Back") && !g.needs_ticks(), "idle and away from it: nothing drawn, nothing to tick");

    // the cursor over the textbox starts the fade; the buttons aren't clickable before they are visible
    let back = centre(quick_button_rect(TEXTBOX, QuickButton::Back));
    g.handle(Event::CursorMoved(back));
    assert!(g.needs_ticks(), "a fade is a timer");
    assert!(!drawn(&g, "Back"));
    click_at(&mut g, back);
    assert_eq!(line(&g), "two", "an invisible button doesn't take the click: it advanced");
    g.handle(Event::CursorMoved(back));
    g.tick(QUICK_FADE / 2.0 + 0.001, false);
    assert!(drawn(&g, "Back"), "half way through the fade it is drawn");
    let alpha = |g: &Game| g.draw_list(&|_| None).iter().find_map(|d| match d {
        Draw::Text { text, color, .. } if text == "Back" => Some(color[3]),
        _ => None,
    });
    assert!(alpha(&g).unwrap() < 1.0 && alpha(&g).unwrap() > 0.0, "{:?}", alpha(&g));
    g.tick(1.0, false);
    assert_eq!(alpha(&g), Some(1.0), "fully faded in (and lit: the cursor is on it)");
    assert!(!g.needs_ticks());
    click_at(&mut g, back);
    assert_eq!(line(&g), "one", "now Back goes back");

    // over the strip (not the textbox) counts too, and leaving fades it out again
    let strip = (quick_button_rect(TEXTBOX, QuickButton::Settings).x + 5.0, TEXTBOX.y + TEXTBOX.h + 2.0);
    g.handle(Event::CursorMoved(Some(strip)));
    g.tick(0.3, false);
    assert!(drawn(&g, "Back"), "the gap under the textbox is part of the strip");
    g.handle(Event::CursorMoved(Some((900.0, 200.0))));
    assert!(g.needs_ticks());
    g.tick(QUICK_FADE / 2.0, false);
    assert!(drawn(&g, "Back"), "still fading out");
    g.tick(1.0, false);
    assert!(!drawn(&g, "Back") && !g.needs_ticks());
    g.handle(Event::CursorMoved(None)); // off the picture
    g.tick(1.0, false);
    assert!(!drawn(&g, "Back"));
}

fn theme_dim_alpha() -> f32 {
    Theme::default().dim_color[3]
}

#[test]
fn hidden_mode_never_draws_or_clicks_and_the_keys_still_work() {
    use quillstone_render::QuickMenu;
    let p = parse(THREE).unwrap();
    let mut g = with_theme(&p, instant(), quick_theme(QuickMenu::Hidden));
    let settings = centre(quick_button_rect(TEXTBOX, QuickButton::Settings));
    g.handle(Event::CursorMoved(settings));
    g.tick(1.0, false);
    assert!(!drawn(&g, "Settings") && !drawn(&g, "Back"));
    click_at(&mut g, settings);
    assert_eq!((g.screen(), line(&g).as_str()), (Screen::Story, "two"), "the click advanced instead");
    key(&mut g, Key::Char('h'));
    assert_eq!(g.screen(), Screen::Backlog, "shortcuts are unaffected");
    key(&mut g, Key::Escape);
    key(&mut g, Key::Backspace);
    assert_eq!(line(&g), "one");
    key(&mut g, Key::Char('a'));
    assert!(g.auto());
}

#[test]
fn the_player_setting_overrides_the_theme_and_default_follows_it() {
    use quillstone_render::QuickMenu;
    let p = parse(THREE).unwrap();
    let back = centre(quick_button_rect(TEXTBOX, QuickButton::Back));
    let shown = |theme: QuickMenu, pref: QuickMenuSetting| {
        let settings = Settings { quick_menu: pref, ..instant() };
        let mut g = with_theme(&p, settings, quick_theme(theme));
        g.handle(Event::CursorMoved(None));
        g.tick(1.0, false);
        drawn(&g, "Back")
    };
    assert!(shown(QuickMenu::Visible, QuickMenuSetting::Default));
    assert!(!shown(QuickMenu::Hidden, QuickMenuSetting::Default));
    assert!(!shown(QuickMenu::Hover, QuickMenuSetting::Default), "no cursor: hover mode is hidden");
    assert!(shown(QuickMenu::Hidden, QuickMenuSetting::Visible), "Always beats a hidden theme");
    assert!(!shown(QuickMenu::Visible, QuickMenuSetting::Hidden));
    assert!(!shown(QuickMenu::Visible, QuickMenuSetting::Hover));
    let mut g = with_theme(&p, Settings { quick_menu: QuickMenuSetting::Hover, ..instant() }, quick_theme(QuickMenu::Visible));
    g.handle(Event::CursorMoved(back));
    g.tick(1.0, false);
    assert!(drawn(&g, "Back"));
}

#[test]
fn the_settings_row_cycles_the_quick_menu_and_it_is_saved() {
    let p = parse(THREE).unwrap();
    let mut g = new_game(&p, instant());
    key(&mut g, Key::Escape);
    let row = ROWS.iter().position(|r| *r == Row::QuickMenu).unwrap();
    assert!(drawn(&g, "Quick menu") && drawn(&g, "Game default"));
    click_at(&mut g, centre(control_rect(row, &ROWS)));
    assert_eq!(g.settings().quick_menu, QuickMenuSetting::Visible);
    assert!(drawn(&g, "Always"));
    click_at(&mut g, centre(control_rect(row, &ROWS)));
    assert_eq!(g.settings().quick_menu, QuickMenuSetting::Hover);
    assert!(drawn(&g, "On hover"));
    // the keyboard: Down to the row, Left goes back, Right and Enter go on
    for _ in 0..ROWS.len() {
        key(&mut g, Key::Up);
    }
    for _ in 0..row {
        key(&mut g, Key::Down);
    }
    key(&mut g, Key::Left);
    assert_eq!(g.settings().quick_menu, QuickMenuSetting::Visible);
    key(&mut g, Key::Right);
    key(&mut g, Key::Enter);
    assert_eq!(g.settings().quick_menu, QuickMenuSetting::Hidden);
    assert!(drawn(&g, "Hidden"));
    key(&mut g, Key::Escape);
    assert!(g.take_requests().contains(&Request::SaveSettings));
    assert_eq!(Settings::parse(&g.settings().to_text()).quick_menu, QuickMenuSetting::Hidden);
    assert!(g.settings().to_text().contains("quick_menu = hidden"));
    // it takes effect at once
    assert!(!drawn(&g, "Back"));
}

#[test]
fn the_settings_screen_fits_every_row_and_the_buttons_at_the_screen_size() {
    for i in 0..ROWS.len() {
        let r = row_rect(i, 8);
        assert!(r.y >= 170.0, "row {i} is below the title");
        if i > 0 {
            assert!(r.y >= row_rect(i - 1, 8).y + row_rect(i - 1, 8).h, "row {i} doesn't overlap the one above");
        }
        let c = control_rect(i, &ROWS);
        assert!(c.y >= r.y && c.y + c.h <= r.y + r.h + 1e-3, "row {i}'s control is inside the row vertically");
        assert!(c.x + c.w <= r.x + r.w, "row {i}'s control is inside the row");
    }
    let last = row_rect(ROWS.len() - 1, 8);
    for in_story in [true, false] {
        for &b in settings_buttons(in_story) {
            let r = button_rect(b, in_story, 8);
            assert!(r.y >= last.y + last.h && r.y + r.h < 960.0, "buttons under the rows and above the hint line");
        }
    }
}

const CHOICE_UI: &str = "**A:** one\n**A:** two\n::choice\n- [Left](#l)\n::\n# l\n**A:** three\n";

#[test]
fn u_hides_the_interface_and_the_next_key_or_click_only_brings_it_back() {
    let p = parse(CHOICE_UI).unwrap();
    let mut g = new_game(&p, instant());
    assert!(drawn(&g, "one") && drawn(&g, "Back"));
    key(&mut g, Key::Char('u'));
    let list = g.draw_list(&|_| None);
    assert!(!has_text(&list, "one") && !has_text(&list, "A") && !has_text(&list, "Back"), "no textbox, name plate, line or quick menu");
    assert!(g.view().hide_ui());
    key(&mut g, Key::Space);
    assert_eq!(line(&g), "one", "the restoring key doesn't advance");
    assert!(drawn(&g, "one") && drawn(&g, "Back") && !g.view().hide_ui());
    // again, restored by a click (not by the click's action: on the quick menu's Back, say)
    key(&mut g, Key::Space);
    assert_eq!(line(&g), "two");
    key(&mut g, Key::Char('U'));
    click_at_swallowed(&mut g, centre(quick_button_rect(TEXTBOX, QuickButton::Settings)));
    assert_eq!((g.screen(), line(&g).as_str()), (Screen::Story, "two"), "neither opened Settings nor advanced");
    assert!(drawn(&g, "two"));
    // U toggles back too (as any key does), and keys that do something do nothing the first time
    key(&mut g, Key::Char('u'));
    key(&mut g, Key::Char('u'));
    assert!(drawn(&g, "two"), "the second U is the restoring key");
    key(&mut g, Key::Char('u'));
    key(&mut g, Key::Escape);
    assert_eq!(g.screen(), Screen::Story, "Escape only restored");
    // choices stay when the interface is hidden
    key(&mut g, Key::Space);
    assert!(g.view().choices.len() == 1);
    key(&mut g, Key::Char('u'));
    assert!(drawn(&g, "Left") && !drawn(&g, "two") && !drawn(&g, "Back"));
    key(&mut g, Key::Char('1'));
    assert_eq!(line(&g), "two", "the restoring key was swallowed, no choice made");
    key(&mut g, Key::Char('1'));
    assert_eq!(line(&g), "three");
}

fn click_at_swallowed(g: &mut Game, at: Option<(f32, f32)>) {
    click_at(g, at);
}

#[test]
fn hiding_the_interface_works_only_on_the_story_screen() {
    let p = parse(THREE).unwrap();
    let mut g = new_game(&p, instant());
    key(&mut g, Key::Escape);
    assert_eq!(g.screen(), Screen::Settings);
    key(&mut g, Key::Char('u'));
    key(&mut g, Key::Escape);
    assert!(drawn(&g, "one") && drawn(&g, "Back") && !g.view().hide_ui(), "U in Settings did nothing");
    // over the quit question
    key(&mut g, Key::Escape);
    click_at(&mut g, centre(button_rect(SettingsButton::Quit, true, 8)));
    key(&mut g, Key::Char('u'));
    assert!(!g.view().hide_ui());
    key(&mut g, Key::Char('n'));
    key(&mut g, Key::Escape);
    assert!(drawn(&g, "one"));
    // the history
    key(&mut g, Key::Char('h'));
    key(&mut g, Key::Char('u'));
    key(&mut g, Key::Escape);
    assert!(!g.view().hide_ui() && drawn(&g, "one"));
}

#[test]
fn hidden_interface_draws_no_mode_badge_and_keeps_ticking_the_story() {
    let p = parse("**A:** one\n::wait{time=1}\n**A:** two\n").unwrap();
    let mut g = new_game(&p, instant());
    key(&mut g, Key::Space); // into the wait
    key(&mut g, Key::Char('a'));
    assert!(drawn(&g, "AUTO"));
    key(&mut g, Key::Char('u'));
    assert!(!drawn(&g, "AUTO"));
    g.tick(2.0, false); // the wait runs out while the interface is hidden
    assert_eq!(line(&g), "two");
}

const TEXTBOX_SCRIPT: &str = "**A:** one\n::textbox{hide}\n::wait{time=5}\n**A:** two\n";

#[test]
fn a_scripted_hidden_box_hides_the_line_and_the_quick_menu_and_clicks_advance() {
    let p = parse(TEXTBOX_SCRIPT).unwrap();
    let mut g = new_game(&p, instant());
    key(&mut g, Key::Space); // into the wait, box hidden
    assert!(g.view().textbox_hidden());
    assert!(!drawn(&g, "one") && !drawn(&g, "Back") && !drawn(&g, "Settings"));
    // the quick menu isn't there to click: a click where Settings was just advances
    click_at(&mut g, centre(quick_button_rect(TEXTBOX, QuickButton::Settings)));
    assert_eq!((g.screen(), line(&g).as_str()), (Screen::Story, "two"));
    assert!(drawn(&g, "two") && drawn(&g, "Back"), "the next line shows the box again");
}

#[test]
fn a_scripted_fade_asks_for_ticks_and_the_menu_fades_with_the_box() {
    let p = parse("**A:** one\n::textbox{hide fade=1}\n::wait{time=5}\n**A:** two\n").unwrap();
    let mut g = new_game(&p, instant());
    key(&mut g, Key::Space);
    assert!(g.needs_ticks());
    assert!(drawn(&g, "Back"), "still on screen at the start of the fade");
    g.tick(0.5, false);
    let back_alpha = g.draw_list(&|_| None).iter().find_map(|d| match d {
        Draw::Text { text, color, .. } if text == "Back" => Some(color[3]),
        _ => None,
    });
    assert!(back_alpha.unwrap() < theme_dim_alpha() * 0.6);
    g.tick(1.0, false);
    assert!(!drawn(&g, "Back") && !drawn(&g, "one"));
}

const BOX_MAP: &str = "**A:** one\n::textbox{hide}\n::hotspots\n- [Go](#x){rect=0,0,0.2,0.2}\n::\n# x\n**A:** two\n";

#[test]
fn a_hidden_box_is_restored_by_rollback_and_a_save() {
    let p = parse(BOX_MAP).unwrap();
    let mut g = new_game(&p, instant());
    key(&mut g, Key::Space);
    assert!(g.view().textbox_hidden() && !g.view().hotspots.is_empty());
    assert!(!drawn(&g, "one") && !drawn(&g, "Back"), "a clean map");
    // quick save, move on (the next line shows the box), then load: hidden again
    key(&mut g, Key::F5);
    click_at(&mut g, Some((50.0, 50.0)));
    assert_eq!(line(&g), "two");
    assert!(!g.view().textbox_hidden() && drawn(&g, "two") && drawn(&g, "Back"));
    key(&mut g, Key::F9);
    key(&mut g, Key::Char('y'));
    assert!(g.view().textbox_hidden() && !drawn(&g, "one") && !drawn(&g, "Back"));
    // rolling back from the next line lands on the hidden map; once more, on the shown line
    click_at(&mut g, Some((50.0, 50.0)));
    key(&mut g, Key::Backspace);
    assert!(g.view().textbox_hidden() && !drawn(&g, "one"));
    key(&mut g, Key::Backspace);
    assert!(!g.view().textbox_hidden() && drawn(&g, "one"));
}

#[test]
fn a_custom_textbox_moves_the_quick_menu_and_its_clicks() {
    let p = parse(THREE).unwrap();
    let theme = Theme { textbox_rect: [200.0, 200.0, 1500.0, 300.0], ..Theme::default() };
    let tb = quillstone_render::textbox_rect(&theme);
    let mut g = with_theme(&p, instant(), theme.clone());
    let back = quick_button_rect(tb, QuickButton::Back);
    assert_eq!(back.y, 504.0, "directly below the textbox");
    assert!(back.x + back.w * 8.0 > 0.0);
    // the row is right-aligned to the textbox
    assert_eq!(quick_button_rect(tb, QuickButton::Settings).x + back.w, 1700.0);
    // a textbox too narrow for the row: it moves right so that no button leaves the screen
    let narrow = quillstone_render::Rect::new(100.0, 200.0, 500.0, 300.0);
    assert!(crate::screens::quick_items_rect(narrow).x >= 0.0);
    assert!(quick_button_rect(narrow, QuickButton::Settings).x + back.w <= 1920.0);
    // no room below the textbox: the row sits just inside its bottom edge
    let low = quillstone_render::Rect::new(160.0, 800.0, 1600.0, 280.0);
    assert_eq!(quick_button_rect(low, QuickButton::Back).y + back.h, 1080.0 - 4.0);
    let tight = quillstone_render::Rect::new(160.0, 780.0, 1600.0, 290.0); // 1070 + 4 + 32 > 1080
    assert!(quick_button_rect(tight, QuickButton::Back).y < 1070.0);
    let exact = quillstone_render::Rect::new(160.0, 780.0, 1600.0, 264.0); // 1044 + 4 + 32 = 1080: fits below
    assert_eq!(quick_button_rect(exact, QuickButton::Back).y, 1048.0);
    click_at(&mut g, centre(back));
    key(&mut g, Key::Space);
    click_at(&mut g, centre(back));
    assert_eq!(line(&g), "one", "Back at its new place worked");
    // the old place is just the stage now
    click_at(&mut g, centre(quick_button_rect(TEXTBOX, QuickButton::Settings)));
    assert_eq!(g.screen(), Screen::Story);
}

// ---- ::fade (dip to colour) -------------------------------------------------------

const DIP: &str = "**A:** before\n::fade{out time=1 color=#000000}\n**A:** in the dark\n::fade{in time=1}\n**A:** after\n";

fn cover_alpha(g: &Game) -> f32 {
    g.view().cover().map_or(0.0, |(_, a)| a)
}

#[test]
fn a_fade_out_waits_for_its_time_and_the_line_comes_over_the_black_screen() {
    let p = parse(DIP).unwrap();
    let mut g = new_game(&p, instant());
    key(&mut g, Key::Space); // leave "before": the fade begins
    assert!(g.needs_ticks(), "the fade is a timer");
    g.tick(0.5, false);
    assert_eq!(line(&g), "before", "still waiting for the fade");
    assert!((cover_alpha(&g) - 0.5).abs() < 1e-3);
    g.tick(0.6, false); // the fade is over: the story goes on by itself
    assert_eq!(line(&g), "in the dark");
    assert_eq!(cover_alpha(&g), 1.0, "and the screen stays covered");
    // the dialogue box is not covered: its draw call comes after the cover's
    let draws = g.draw_list(&|_| None);
    let cover = draws.iter().position(|d| matches!(d, Draw::Rect { color, .. } if color[..3] == [0.0; 3] && color[3] == 1.0)).unwrap();
    assert!(draws.iter().skip(cover).any(|d| matches!(d, Draw::Text { text, .. } if text == "in the dark")));
}

#[test]
fn a_click_ends_the_fade_out_at_once_and_the_story_continues() {
    let p = parse(DIP).unwrap();
    let mut g = new_game(&p, instant());
    key(&mut g, Key::Space);
    g.tick(0.2, false);
    key(&mut g, Key::Space); // a click during the fade
    assert_eq!(line(&g), "in the dark");
    assert_eq!(cover_alpha(&g), 1.0, "covered fully, not left half way");
}

#[test]
fn a_fade_in_does_not_wait_and_finishes_by_itself() {
    let p = parse(DIP).unwrap();
    let mut g = new_game(&p, instant());
    key(&mut g, Key::Space);
    key(&mut g, Key::Space); // skip the fade out
    key(&mut g, Key::Space); // leave the dark line: the fade in starts and "after" shows
    assert_eq!(line(&g), "after");
    assert!(cover_alpha(&g) > 0.9);
    g.tick(0.5, false);
    assert!((cover_alpha(&g) - 0.5).abs() < 1e-3);
    g.tick(0.6, false);
    assert_eq!(cover_alpha(&g), 0.0);
    assert!(!g.needs_ticks());
}

#[test]
fn skip_mode_runs_through_fades_without_waiting_for_them() {
    let p = parse(DIP).unwrap();
    let mut first = new_game(&p, instant());
    for _ in 0..3 {
        key(&mut first, Key::Space); // read everything once
    }
    first.autosave();
    let mut g = reread(&p, &first, instant());
    g.handle(Event::Key(Key::Tab)); // toggle skip
    run(&mut g, 12);
    assert_eq!(line(&g), "after");
    assert_eq!(cover_alpha(&g), 0.0, "the fade in was cut short too");
}

#[test]
fn rollback_goes_past_the_fade_wait_and_back_to_the_dark_line_covered_again() {
    let p = parse(DIP).unwrap();
    let mut g = new_game(&p, instant());
    key(&mut g, Key::Space);
    key(&mut g, Key::Space); // "in the dark", covered
    key(&mut g, Key::Backspace);
    assert_eq!(line(&g), "before", "not stuck on the half-faded wait");
    assert_eq!(cover_alpha(&g), 0.0);
    g.handle(Event::Wheel(-1.0)); // roll forward
    assert_eq!(line(&g), "in the dark");
    assert_eq!(cover_alpha(&g), 1.0);
}

#[test]
fn a_saved_covered_scene_loads_covered_and_an_uncovered_one_uncovered() {
    let p = parse(DIP).unwrap();
    let mut g = new_game(&p, instant());
    key(&mut g, Key::Space);
    key(&mut g, Key::Space); // "in the dark", covered
    key(&mut g, Key::F5);
    key(&mut g, Key::Space); // the fade in, and "after"
    g.tick(2.0, false);
    assert_eq!(cover_alpha(&g), 0.0);
    key(&mut g, Key::F9);
    key(&mut g, Key::Enter); // yes, load
    assert_eq!(line(&g), "in the dark");
    assert_eq!(cover_alpha(&g), 1.0, "the cover came back with the save");
    // a save made before the first fade loads uncovered
    key(&mut g, Key::Backspace);
    key(&mut g, Key::F5);
    key(&mut g, Key::Space);
    key(&mut g, Key::Space);
    assert_eq!(cover_alpha(&g), 1.0);
    key(&mut g, Key::F9);
    key(&mut g, Key::Enter);
    assert_eq!(line(&g), "before");
    assert_eq!(cover_alpha(&g), 0.0);
}

#[test]
fn the_host_seed_decides_the_rolls_and_a_new_game_moves_it_on() {
    let p = quillstone_script::parse("---\nvars: { r: 0 }\n---\n::set{r = random(1, 100000)}\nRoll {r}.\n::end\n").unwrap();
    let roll = |seed: Option<u64>| {
        let mut g = Game::new(&p, instant(), Box::new(crate::MemoryStorage::default()));
        if let Some(seed) = seed {
            g.set_seed(seed);
        }
        g.start_story();
        let first = line(&g);
        g.start_story(); // playing again
        (first, line(&g))
    };
    assert_eq!(roll(Some(11)), roll(Some(11)), "the same seed replays");
    assert_ne!(roll(Some(11)).0, roll(Some(12)).0);
    let (a, b) = roll(Some(11));
    assert_ne!(a, b, "a second playthrough rolls differently");
    assert_eq!(roll(None), roll(None), "the default is a fixed seed");
}
