//! Scene replays in the game shell: unlocking, the Scenes tab, what a replay
//! changes (nothing that lasts), and the ways it ends.

use super::test_util::{click, instant, key, line};
use super::*;
use crate::extras::{cell_rect, tab_rect, track_rect, Tab};
use quillstone_script::{parse, Vars};
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

/// Storage the test can still read after handing it to the game, and make fail.
#[derive(Clone, Default)]
struct Shared {
    mem: Rc<RefCell<crate::MemoryStorage>>,
    fail: Rc<Cell<bool>>,
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

impl Shared {
    fn files(&self) -> HashMap<String, String> {
        self.mem.borrow().files.clone()
    }
}

const SRC: &str = "---\nvars: { trust: 0 }\npersistent: { roof_visits: 0 }\ngallery: [cg.png]\nreplays: { roof: \"The roof\", walk: \"Night walk\" }\n---\n# start\n**A:** intro\n::choice\n- [Roof](#roof)\n- [Walk](#walk)\n::\n# roof\n::set{roof_visits += 1}\n::bg{src=cg.png}\n**A:** on the roof\n::choice\n- [Kind](#kind){trust+=1}\n- [Cold](#cold)\n::\n# kind\n**A:** kind\n::end_replay\n::jump{to=finale}\n# cold\n**A:** cold\n::end_replay\n::jump{to=finale}\n# walk\n**A:** walking\n::end_replay\n::jump{to=finale}\n# finale\n**A:** finale {trust}\n";

fn notice(g: &Game) -> Option<String> {
    g.notice.as_ref().map(|(m, _)| m.clone())
}

fn open_extras(g: &mut Game) {
    let buttons = menu_buttons(g.newest_save.is_some(), g.has_extras());
    let at = buttons.iter().position(|b| *b == MenuButton::Extras).expect("an Extras button");
    click(g, menu_button_rect(at, &Theme::default()));
    assert_eq!(g.screen(), Screen::Extras);
}

/// Settings -> Main menu -> yes.
fn to_menu(g: &mut Game) {
    g.open_settings();
    click(g, button_rect(SettingsButton::MainMenu, true, 8, &Theme::default()));
    key(g, Key::Char('y'));
}

/// A game whose player took the roof route (so "The roof" is unlocked) and is back at the menu.
fn after_the_roof<'p>(p: &'p Program, storage: &Shared) -> Game<'p> {
    let mut g = Game::new(p, instant(), Box::new(storage.clone()));
    g.start_story();
    key(&mut g, Key::Space); // intro -> choice
    key(&mut g, Key::Char('1')); // the roof
    assert_eq!(line(&g), "on the roof");
    to_menu(&mut g);
    assert_eq!(g.screen(), Screen::MainMenu);
    g
}

fn to_scenes(g: &mut Game) {
    open_extras(g);
    key(g, Key::Tab); // gallery -> scenes
    assert_eq!(g.extras.tab, Tab::Scenes);
}

// ---- the tab ------------------------------------------------------------------------------

#[test]
fn the_scenes_tab_shows_unlocked_titles_and_locked_rows() {
    let p = parse(SRC).unwrap();
    let storage = Shared::default();
    let mut g = after_the_roof(&p, &storage);
    to_scenes(&mut g);
    assert_eq!(g.extras_entries(), [("The roof", true), ("Night walk", false)]);
    let none = |_: &str| None;
    let list = g.draw_list(&none);
    let has = |t: &str| list.iter().any(|d| matches!(d, Draw::Text { text, .. } if text == t));
    assert!(has("Scenes") && has("The roof") && has("???") && !has("Night walk"), "locked titles stay secret");
    // a click on the locked row is refused the way a locked picture is
    click(&mut g, track_rect(1));
    assert_eq!(notice(&g).as_deref(), Some("Not unlocked yet"));
    assert!(!g.in_replay());
    // keyboard: Down walks the list, Enter on a locked row is the same refusal
    key(&mut g, Key::Down);
    key(&mut g, Key::Down);
    assert_eq!(g.extras.focus, Some(1));
    g.notice = None;
    key(&mut g, Key::Enter);
    assert_eq!(notice(&g).as_deref(), Some("Not unlocked yet"));
    key(&mut g, Key::Up);
    key(&mut g, Key::Enter);
    assert!(g.in_replay());
}

#[test]
fn tab_cycles_through_the_tabs_a_game_has() {
    let tabs_of = |src: &str| {
        let p = parse(src).unwrap();
        let mut g = Game::new(&p, instant(), Box::new(crate::MemoryStorage::default()));
        open_extras(&mut g);
        let mut seen = vec![g.extras.tab];
        for _ in 0..4 {
            key(&mut g, Key::Tab);
            seen.push(g.extras.tab);
        }
        seen
    };
    use Tab::*;
    let scenes = "replays: { a: \"A\" }";
    let front = |lines: &str| format!("---\n{lines}\n---\n# a\nHi.\n");
    assert_eq!(tabs_of(&front(scenes)), [Scenes; 5], "alone: Tab does nothing");
    assert_eq!(tabs_of(&front(&format!("gallery: [a.png]\n{scenes}"))), [Gallery, Scenes, Gallery, Scenes, Gallery]);
    assert_eq!(tabs_of(&front(&format!("music_room: [a.ogg]\n{scenes}"))), [Music, Scenes, Music, Scenes, Music]);
    assert_eq!(tabs_of(&front(&format!("gallery: [a.png]\nmusic_room: [a.ogg]\n{scenes}")))[..4], [Gallery, Music, Scenes, Gallery]);
    assert_eq!(tabs_of(&front("gallery: [a.png]\nmusic_room: [a.ogg]"))[..3], [Gallery, Music, Gallery], "no scenes: as before");
    // clicking the third tab works
    let p = parse(&front(&format!("gallery: [a.png]\nmusic_room: [a.ogg]\n{scenes}"))).unwrap();
    let mut g = Game::new(&p, instant(), Box::new(crate::MemoryStorage::default()));
    open_extras(&mut g);
    click(&mut g, tab_rect(2, 3));
    assert_eq!(g.extras.tab, Scenes);
    let _ = cell_rect(0);
}

#[test]
fn replays_alone_make_an_extras_button() {
    let p = parse("---\nreplays: { a: \"A\" }\n---\n# a\nHi.\n").unwrap();
    let g = Game::new(&p, instant(), Box::new(crate::MemoryStorage::default()));
    assert!(g.has_extras());
    assert!(menu_buttons(false, g.has_extras()).contains(&MenuButton::Extras));
}

#[test]
fn many_scenes_are_paged_like_the_music_room() {
    let labels: String = (0..9).map(|i| format!("# s{i}\nHi.\n::end_replay\n")).collect();
    let list: Vec<String> = (0..9).map(|i| format!("s{i}: \"Scene {i}\"")).collect();
    let p = parse(&format!("---\nreplays: {{ {} }}\n---\n{labels}", list.join(", "))).unwrap();
    let mut g = Game::new(&p, instant(), Box::new(crate::MemoryStorage::default()));
    open_extras(&mut g);
    assert_eq!(g.extras_entries().len(), 8);
    key(&mut g, Key::Down); // focus the first row
    key(&mut g, Key::Right); // off the side: next page
    assert_eq!(g.extras.page, 1);
    let rows = g.extras_entries();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].0, "Scene 8");
    assert!(!rows[0].1, "no game has been played");
    key(&mut g, Key::Left);
    assert_eq!(g.extras.page, 0);
    click(&mut g, crate::extras::next_rect());
    assert_eq!(g.extras.page, 1);
}

// ---- running one --------------------------------------------------------------------------

#[test]
fn a_replay_runs_the_scene_and_ends_back_on_the_scenes_tab() {
    let p = parse(SRC).unwrap();
    let storage = Shared::default();
    let mut g = after_the_roof(&p, &storage);
    let (persistent, read, files) = (storage.read("persistent.txt"), storage.read("read.txt"), storage.files());
    assert!(persistent.as_deref().is_some_and(|t| t.contains("replay:roof") && !t.contains("replay:walk")), "{persistent:?}");
    g.take_audio();
    to_scenes(&mut g);
    click(&mut g, track_rect(0));
    assert!(g.in_replay() && g.in_story() && g.screen() == Screen::Story);
    assert_eq!(line(&g), "on the roof");
    assert_eq!(g.state().vars["trust"], Value::Int(0), "the script's starting variables");
    assert_eq!(g.state().vars["roof_visits"], Value::Int(2), "the player's persistent data (1), plus this scene's own `+= 1`");
    assert!(g.view().background.is_some(), "the scene's own background");
    key(&mut g, Key::Space); // the choice
    assert_eq!(g.view().choices.len(), 2);
    key(&mut g, Key::Char('1')); // Kind
    assert_eq!(g.state().vars["trust"], Value::Int(1));
    assert_eq!(line(&g), "kind");
    assert!(g.in_replay());
    key(&mut g, Key::Space); // ::end_replay
    assert!(!g.in_replay() && !g.in_story());
    assert_eq!(g.screen(), Screen::Extras);
    assert_eq!(g.extras.tab, Tab::Scenes);
    assert_eq!(g.extras.focus, Some(0), "back on the row that was replayed");
    assert_eq!(g.extras_entries(), [("The roof", true), ("Night walk", false)]);
    // nothing that lasts changed
    assert_eq!(storage.read("persistent.txt"), persistent);
    assert_eq!(storage.read("read.txt"), read);
    assert_eq!(storage.files(), files, "no save, no auto save");
    assert_eq!(g.session.persistent()["roof_visits"], Value::Int(1), "the replay's `::set` was thrown away");
    assert_eq!(g.state().vars["trust"], Value::Int(0), "and `trust` is not in the real game");
    // the menu music's stop/resume: leaving ended the scene's music (none here) and the menu is quiet
    key(&mut g, Key::Escape);
    g.start_story();
    assert_eq!(g.state().vars["trust"], Value::Int(0));
    assert!(!g.in_replay());
}

#[test]
fn a_replay_stops_the_menu_music_and_brings_it_back() {
    let p = parse(SRC).unwrap();
    let storage = Shared::default();
    let mut g = after_the_roof(&p, &storage);
    g.set_theme(Theme { menu_music: Some("menu.ogg".into()), ..Theme::default() });
    to_scenes(&mut g);
    g.take_audio();
    click(&mut g, track_rect(0));
    assert!(g.take_audio().iter().any(|a| matches!(a, Action::StopMusic { .. })));
    key(&mut g, Key::Space);
    key(&mut g, Key::Char('2'));
    key(&mut g, Key::Space);
    assert_eq!(g.screen(), Screen::Extras);
    assert!(g.take_audio().iter().any(|a| matches!(a, Action::PlayMusic { src, .. } if src == "menu.ogg")));
}

#[test]
fn a_replay_changes_nothing_that_lasts_not_even_other_unlocks() {
    let src = "---\npersistent: { done: false }\ngallery: [cg.png]\nreplays: { a: \"A\", b: \"B\" }\n---\n# a\n::bg{src=cg.png}\n::set{done=true}\n**A:** in a\n# b\n**A:** in b\n";
    let p = parse(src).unwrap();
    let mut kept = Vars::new();
    kept.insert("replay:a".into(), Value::Bool(true));
    let storage = Shared::default();
    storage.mem.borrow_mut().files.insert("persistent.txt".into(), quillstone_runtime::write_persistent(&kept));
    let mut g = Game::new(&p, instant(), Box::new(storage.clone()));
    let before = storage.files();
    open_extras(&mut g);
    key(&mut g, Key::Tab);
    assert_eq!(g.extras.tab, Tab::Scenes);
    click(&mut g, track_rect(0));
    assert!(g.in_replay());
    assert_eq!(g.state().vars["done"], Value::Bool(true));
    key(&mut g, Key::Space); // runs on into b, whose label is "reached" inside the replay
    assert_eq!(line(&g), "in b");
    key(&mut g, Key::Space); // the end of the story ends the replay
    assert_eq!(g.screen(), Screen::Extras);
    assert_eq!(storage.files(), before, "nothing written");
    assert!(!g.session.replay_unlocked("b") && !g.session.unlocked("cg.png"));
    assert_eq!(g.session.persistent().get("done"), None);
    g.start_story();
    assert!(g.session.replay_unlocked("a"), "a real game reaching `a` unlocks it again");
}

#[test]
fn nothing_is_saved_or_loaded_during_a_replay() {
    let p = parse(SRC).unwrap();
    let storage = Shared::default();
    let mut g = after_the_roof(&p, &storage);
    let before = storage.files();
    to_scenes(&mut g);
    click(&mut g, track_rect(0));
    for k in [Key::Char('s'), Key::Char('l'), Key::F5, Key::F9] {
        g.notice = None;
        key(&mut g, k);
        assert_eq!(g.screen(), Screen::Story, "{k:?}");
        assert_eq!(notice(&g).as_deref(), Some("Not available during a replay"), "{k:?}");
    }
    // the quick menu's buttons say the same
    for button in [QuickButton::Save, QuickButton::Load, QuickButton::QuickSave, QuickButton::QuickLoad] {
        g.notice = None;
        g.quick_press(button);
        assert_eq!(notice(&g).as_deref(), Some("Not available during a replay"), "{button:?}");
        assert_eq!(g.screen(), Screen::Story);
    }
    key(&mut g, Key::Space); // arriving at a choice autosaves in a real game
    assert_eq!(g.view().choices.len(), 2);
    g.autosave(); // so does closing the window
    assert_eq!(storage.files(), before, "no save files of any kind");
    assert!(g.confirm.is_none());
}

#[test]
fn skip_uses_the_real_read_record_and_never_adds_to_it() {
    let p = parse(SRC).unwrap();
    let storage = Shared::default();
    let mut g = after_the_roof(&p, &storage);
    let read = storage.read("read.txt");
    to_scenes(&mut g);
    click(&mut g, track_rect(0));
    key(&mut g, Key::Tab); // skip: "on the roof" was never read in the real game
    assert!(!g.skipping());
    assert_eq!(notice(&g).as_deref(), Some("Nothing here has been read yet"));
    key(&mut g, Key::Space);
    key(&mut g, Key::Char('1'));
    key(&mut g, Key::Space);
    assert_eq!(storage.read("read.txt"), read);
    assert!(!g.read_dirty);
}

#[test]
fn rolling_back_in_a_replay_works_and_stops_at_the_start() {
    let p = parse(SRC).unwrap();
    let storage = Shared::default();
    let mut g = after_the_roof(&p, &storage);
    to_scenes(&mut g);
    click(&mut g, track_rect(0));
    g.handle(Event::Wheel(1.0));
    assert!(g.in_replay());
    assert_eq!(line(&g), "on the roof", "can't go before the start");
    key(&mut g, Key::Space);
    key(&mut g, Key::Char('1'));
    assert_eq!(line(&g), "kind");
    key(&mut g, Key::Backspace);
    assert_eq!(g.view().choices.len(), 2, "back at the choice");
    assert_eq!(g.state().vars["trust"], Value::Int(0));
    key(&mut g, Key::Char('2'));
    assert_eq!(line(&g), "cold");
    key(&mut g, Key::Space);
    assert_eq!(g.screen(), Screen::Extras);
    // the History screen works for the replay's own lines
    to_scenes_again(&mut g);
    key(&mut g, Key::Enter);
    key(&mut g, Key::Char('h'));
    assert_eq!(g.screen(), Screen::Backlog);
    key(&mut g, Key::Escape);
    assert_eq!(g.screen(), Screen::Story);
}

fn to_scenes_again(g: &mut Game) {
    assert_eq!(g.screen(), Screen::Extras);
    assert_eq!(g.extras.tab, Tab::Scenes);
    g.extras.focus = Some(0);
}

#[test]
fn a_replay_that_runs_to_the_end_of_the_story_ends_too() {
    let src = "---\nreplays: { tail: \"Tail\" }\n---\n# start\n**A:** one\n# tail\n**A:** two\n**A:** three\n";
    let p = parse(src).unwrap();
    let mut g = Game::new(&p, instant(), Box::new(crate::MemoryStorage::default()));
    open_extras(&mut g);
    assert_eq!(g.extras_entries(), [("Tail", false)], "nobody has reached it yet");
    g.start_story();
    key(&mut g, Key::Space);
    key(&mut g, Key::Space);
    key(&mut g, Key::Space); // the real game ends: the menu
    key(&mut g, Key::Space);
    assert_eq!(g.screen(), Screen::MainMenu);
    open_extras(&mut g);
    assert_eq!(g.extras_entries(), [("Tail", true)]);
    click(&mut g, track_rect(0));
    assert_eq!(line(&g), "two");
    key(&mut g, Key::Space);
    assert_eq!(line(&g), "three");
    key(&mut g, Key::Space);
    assert_eq!(g.screen(), Screen::Extras, "the story's end is the replay's end");
    assert_eq!(g.extras.tab, Tab::Scenes);
}

#[test]
fn the_end_of_a_runaway_replay_ends_it_with_the_notice() {
    let src = "---\nreplays: { spin: \"Spin\" }\n---\n# start\n**A:** hi\n# spin\n::jump{to=spin}\n";
    let p = parse(src).unwrap();
    let mut kept = Vars::new();
    kept.insert("replay:spin".into(), Value::Bool(true));
    let mut storage = crate::MemoryStorage::default();
    storage.files.insert("persistent.txt".into(), quillstone_runtime::write_persistent(&kept));
    let mut g = Game::new(&p, instant(), Box::new(storage));
    open_extras(&mut g);
    click(&mut g, track_rect(0));
    assert_eq!(g.screen(), Screen::Extras);
    assert!(notice(&g).is_some_and(|n| n.contains("stopped")), "{:?}", notice(&g));
}

#[test]
fn settings_main_menu_leaves_a_replay_for_extras_and_quit_still_quits() {
    let p = parse(SRC).unwrap();
    let storage = Shared::default();
    let mut g = after_the_roof(&p, &storage);
    to_scenes(&mut g);
    click(&mut g, track_rect(0));
    key(&mut g, Key::Escape);
    assert_eq!(g.screen(), Screen::Settings);
    click(&mut g, button_rect(SettingsButton::MainMenu, true, 8, &Theme::default()));
    assert!(g.confirm.as_ref().is_some_and(|c| c.message.contains("replay") && !c.message.contains("Unsaved")));
    key(&mut g, Key::Escape); // No: still in the replay, settings still open
    assert!(g.in_replay() && g.screen() == Screen::Settings);
    click(&mut g, button_rect(SettingsButton::MainMenu, true, 8, &Theme::default()));
    key(&mut g, Key::Enter);
    assert!(!g.in_replay());
    assert_eq!((g.screen(), g.extras.tab), (Screen::Extras, Tab::Scenes));
    // Quit still quits the program
    key(&mut g, Key::Escape);
    assert_eq!(g.screen(), Screen::MainMenu);
    to_scenes(&mut g);
    click(&mut g, track_rect(0));
    key(&mut g, Key::Escape);
    click(&mut g, button_rect(SettingsButton::Quit, true, 8, &Theme::default()));
    key(&mut g, Key::Enter);
    assert!(g.take_requests().contains(&Request::Quit));
}

#[test]
fn a_replays_menu_tabs_leave_out_save_and_load_and_tab_cycles_between_settings_and_history() {
    let p = parse(SRC).unwrap();
    let storage = Shared::default();
    let mut g = after_the_roof(&p, &storage);
    to_scenes(&mut g);
    click(&mut g, track_rect(0));
    key(&mut g, Key::Escape);
    assert_eq!(g.screen(), Screen::Settings);
    assert!(!game_tabs(true, true).contains(&GameTab::Save) && !game_tabs(true, true).contains(&GameTab::Load));
    key(&mut g, Key::Tab);
    assert_eq!(g.screen(), Screen::Backlog);
    key(&mut g, Key::Tab);
    assert_eq!(g.screen(), Screen::Settings, "round again, not stuck on History");
}

// ---- failures and reloads -----------------------------------------------------------------

#[test]
fn a_scene_unlock_that_could_not_be_written_is_retried() {
    let p = parse(SRC).unwrap();
    let storage = Shared::default();
    storage.fail.set(true);
    let mut g = Game::new(&p, instant(), Box::new(storage.clone()));
    g.start_story();
    key(&mut g, Key::Space);
    key(&mut g, Key::Char('1')); // reaches `roof`, but the disk is busy
    assert!(notice(&g).is_some_and(|n| n.contains("Could not save what was unlocked")), "{:?}", notice(&g));
    storage.fail.set(false);
    g.autosave();
    let kept = storage.read("persistent.txt").expect("written once the disk works again");
    assert!(kept.contains("replay:roof"), "{kept}");
}

#[test]
fn a_failed_write_is_retried_with_the_real_data_even_during_a_replay() {
    let p = parse(SRC).unwrap();
    let storage = Shared::default();
    let mut g = after_the_roof(&p, &storage);
    to_scenes(&mut g);
    click(&mut g, track_rect(0));
    g.persistent_dirty = true; // an earlier write failed
    g.autosave();
    let kept = storage.read("persistent.txt").unwrap();
    assert!(kept.contains("replay:roof") && kept.contains("\"roof_visits\" int 1"), "{kept}");
    assert!(!g.persistent_dirty);
}

#[test]
fn a_script_reload_during_a_replay_goes_on_or_leaves_for_extras() {
    let p = parse(SRC).unwrap();
    let edited = parse(&SRC.replace("on the roof", "on the roof, edited")).unwrap();
    let gone = parse("---\ngallery: [cg.png]\nreplays: { walk: \"Night walk\" }\n---\n# walk\n**A:** walking\n").unwrap();
    let storage = Shared::default();
    let mut g = after_the_roof(&p, &storage);
    to_scenes(&mut g);
    click(&mut g, track_rect(0));
    key(&mut g, Key::Space);
    key(&mut g, Key::Char('1'));
    assert_eq!(line(&g), "kind");
    g.reload(&edited).unwrap();
    assert!(g.in_replay(), "same label: the replay carries on");
    assert_eq!(line(&g), "kind");
    assert_eq!(g.state().vars["trust"], Value::Int(1));
    g.reload(&gone).unwrap();
    assert!(!g.in_replay());
    assert_eq!(g.screen(), Screen::Extras);
    assert!(notice(&g).is_some_and(|n| n.contains("removed")), "{:?}", notice(&g));
    assert!(g.session.persistent().contains_key("replay:roof"), "the player's data is kept");
}

#[test]
fn a_reload_that_ends_the_replay_at_the_current_spot_leaves_for_extras() {
    let p = parse(SRC).unwrap();
    let storage = Shared::default();
    for edit in [("**A:** kind\n::end_replay", "::end_replay\n::end_replay"), ("**A:** kind\n::end_replay\n::jump{to=finale}", "::end")] {
        let edited = parse(&SRC.replace(edit.0, edit.1)).unwrap();
        let mut g = after_the_roof(&p, &storage);
        to_scenes(&mut g);
        click(&mut g, track_rect(0));
        key(&mut g, Key::Space);
        key(&mut g, Key::Char('1'));
        assert_eq!(line(&g), "kind");
        g.reload(&edited).unwrap();
        assert!(!g.in_replay(), "{edit:?}: the replay is over, not stuck on an empty screen");
        assert_eq!(g.screen(), Screen::Extras);
        assert_eq!(g.extras.tab, Tab::Scenes);
    }
}

#[test]
fn end_replay_in_a_real_game_is_a_no_op_for_the_game_too() {
    let p = parse(SRC).unwrap();
    let mut g = Game::new(&p, instant(), Box::new(crate::MemoryStorage::default()));
    g.start_story();
    key(&mut g, Key::Space);
    key(&mut g, Key::Char('2')); // walk
    assert_eq!(line(&g), "walking");
    key(&mut g, Key::Space); // ::end_replay, then the finale
    assert_eq!(line(&g), "finale 0");
    assert!(g.in_story() && !g.in_replay());
}

#[test]
fn the_replay_demo_plays_as_described() {
    let p = parse(include_str!("../../../../examples/replay.quill.md")).unwrap();
    let storage = Shared::default();
    let mut g = Game::new(&p, instant(), Box::new(storage.clone()));
    g.start_story();
    key(&mut g, Key::Space);
    key(&mut g, Key::Char('1'));
    to_menu(&mut g);
    to_scenes(&mut g);
    assert_eq!(g.extras_entries(), [("The rooftop", true), ("Night walk", false)]);
    click(&mut g, track_rect(0));
    assert!(g.in_replay());
    key(&mut g, Key::Space);
    key(&mut g, Key::Char('1'));
    assert_eq!(line(&g), "Then stay a while.");
    key(&mut g, Key::Space);
    assert_eq!(g.screen(), Screen::Extras, "{}", line(&g));
    assert_eq!(storage.read("persistent.txt").unwrap().matches("roof_visits\" int 1").count(), 1);
}
