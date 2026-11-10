//! Languages in the game shell: the Settings row, switching mid-story, and that only the
//! presentation changes (rollback, saves, hot reload and settings all keep working).

use super::test_util::{instant, key, line};
use super::*;
use quillstone_render::Size;
use quillstone_script::{parse, Catalog};

const HEAD: &str = "---\ntitle: Tale\nlanguages: en=English, sv=Svenska\nvars: { gold: 3 }\n---\n";
const BODY: &str = "\
::character{id=mika name=Mika}
# start
**Mika:** Hello, you have {gold} coins.

**Mika:** Second line.

::choice
- [Left](#l)
- [Right](#l)
::
# l
Narration.

::end
";

const SV: &str = "\
> Tale\n= Sagan\n\n> Mika\n= Mikaela\n\n> Hello, you have {gold} coins.\n= Hej, du har {gold} mynt.\n\n\
> Second line.\n= Andra raden.\n\n> Left\n= Vänster\n\n> Right\n= Höger\n\n> Narration.\n= Berättelse.\n";

fn story() -> String {
    format!("{HEAD}{BODY}")
}

fn game<'p>(p: &'p Program, settings: Settings) -> Game<'p> {
    let mut g = Game::new(p, settings, Box::new(crate::MemoryStorage::default()));
    g.set_translations([("sv".to_string(), Catalog::parse(SV).unwrap())].into());
    g.start_story();
    g
}

fn texts(g: &Game) -> Vec<String> {
    let draws = g.draw_list(&|_: &str| Some(Size::new(64.0, 64.0)));
    draws.into_iter().filter_map(|d| if let Draw::Text { text, .. } = d { Some(text) } else { None }).collect()
}

/// Open Settings, down to the Language row (the tenth), and one step with `Right` (or `Left`).
fn switch(g: &mut Game, k: Key) {
    g.open_settings();
    for _ in 0..9 {
        key(g, Key::Down);
    }
    key(g, k);
    key(g, Key::Escape);
}

#[test]
fn a_one_language_game_has_no_language_row() {
    let p = parse(BODY).unwrap();
    let mut g = Game::new(&p, instant(), Box::new(crate::MemoryStorage::default()));
    g.start_story();
    g.open_settings();
    assert!(!texts(&g).iter().any(|t| t == "Language"));
    assert_eq!(g.settings().language, None);
    for _ in 0..12 {
        key(&mut g, Key::Down);
        key(&mut g, Key::Right);
    }
    assert_eq!(g.settings().language, None, "nothing to change");
    assert_eq!(g.languages().len(), 0);
}

#[test]
fn the_row_cycles_with_keys_and_the_line_is_translated_at_once() {
    let p = parse(&story()).unwrap();
    let mut g = game(&p, instant());
    assert_eq!(line(&g), "Hello, you have 3 coins.");
    g.open_settings();
    let t = texts(&g);
    assert!(t.contains(&"Language".to_string()) && t.contains(&"English".to_string()), "{t:?}");
    for _ in 0..9 {
        key(&mut g, Key::Down);
    }
    key(&mut g, Key::Right);
    assert!(texts(&g).contains(&"Svenska".to_string()));
    key(&mut g, Key::Escape);
    assert_eq!(g.language(), Some("sv"));
    assert_eq!(line(&g), "Hej, du har 3 mynt.");
    assert_eq!(g.view().dialogue.as_ref().unwrap().speaker.as_deref(), Some("Mikaela"));
    assert_eq!(g.state().line.as_ref().unwrap().text, "Hello, you have 3 coins.", "the story keeps its own text");
    // and back (wraps both ways)
    switch(&mut g, Key::Left);
    assert_eq!((g.language(), line(&g).as_str()), (None, "Hello, you have 3 coins."));
    switch(&mut g, Key::Left);
    assert_eq!(g.language(), Some("sv"), "Left from the first wraps to the last");
}

#[test]
fn a_click_on_the_row_cycles_and_the_title_follows() {
    let p = parse(&story()).unwrap();
    let mut g = game(&p, instant());
    assert_eq!(g.title(), "Tale");
    g.open_settings();
    let c = crate::screens::control_rect(9, crate::screens::rows(true), &Theme::default());
    g.handle(Event::CursorMoved(Some((c.x + c.w / 2.0, c.y + c.h / 2.0))));
    g.handle(Event::Press(Button::Left));
    g.handle(Event::Release(Button::Left));
    assert_eq!(g.language(), Some("sv"));
    assert_eq!(g.title(), "Sagan");
    key(&mut g, Key::Escape);
}

#[test]
fn choices_history_and_rollback_follow_the_language() {
    let p = parse(&story()).unwrap();
    let mut g = game(&p, instant());
    key(&mut g, Key::Space);
    key(&mut g, Key::Space);
    assert_eq!(g.view().choices, ["Left", "Right"]);
    switch(&mut g, Key::Right);
    assert_eq!(g.view().choices, ["Vänster", "Höger"]);
    assert_eq!(line(&g), "Andra raden.", "the line under the choices too");
    key(&mut g, Key::Char('1'));
    assert_eq!(line(&g), "Berättelse.");
    key(&mut g, Key::Char('h'));
    let log = texts(&g).join("|");
    assert!(log.contains("Hej, du har 3 mynt.") && log.contains("Vänster") && log.contains("Berättelse."), "{log}");
    assert!(!log.contains("Narration."));
    key(&mut g, Key::Escape);
    // switching back: the history is in English again
    switch(&mut g, Key::Left);
    key(&mut g, Key::Char('h'));
    assert!(texts(&g).join("|").contains("Left"));
    key(&mut g, Key::Escape);
    // rollback to the choices (still English), then Swedish again: nothing was rewritten
    key(&mut g, Key::Backspace);
    assert_eq!(g.view().choices, ["Left", "Right"]);
    switch(&mut g, Key::Right);
    assert_eq!(g.view().choices, ["Vänster", "Höger"]);
}

#[test]
fn saves_carry_no_translated_text_and_load_in_any_language() {
    let p = parse(&story()).unwrap();
    let mut g = game(&p, instant());
    switch(&mut g, Key::Right);
    key(&mut g, Key::Space);
    key(&mut g, Key::F5);
    let saved = g.session.save();
    assert!(!saved.contains("Hej") && !saved.contains("Mikaela") && !saved.contains("sv"), "{saved}");
    key(&mut g, Key::Space);
    key(&mut g, Key::F9); // quick load asks first
    key(&mut g, Key::Char('y'));
    assert_eq!(line(&g), "Andra raden.", "loaded, shown in the language in use");
    switch(&mut g, Key::Left);
    assert_eq!(line(&g), "Second line.");
}

#[test]
fn the_choice_persists_with_the_other_settings_and_hot_reload_keeps_it() {
    let p = parse(&story()).unwrap();
    let mut g = game(&p, instant());
    g.open_settings();
    for _ in 0..9 {
        key(&mut g, Key::Down);
    }
    key(&mut g, Key::Right);
    key(&mut g, Key::Escape);
    assert!(g.take_requests().contains(&Request::SaveSettings));
    let text = g.settings().to_text();
    assert!(text.contains("language = sv"));
    // a new run with the saved settings starts in Swedish
    let restored = Settings::parse(&text);
    let mut again = game(&p, Settings { text_speed: 0.0, ..restored });
    assert_eq!(line(&again), "Hej, du har 3 mynt.");
    // hot reload with an edited script
    let edited = parse(&story().replace("Second line.", "Second line!")).unwrap();
    again.reload(&edited).unwrap();
    assert_eq!(again.language(), Some("sv"));
    assert_eq!(line(&again), "Hej, du har 3 mynt.");
    // a language the game doesn't offer (an old settings file) is the script's own
    let odd = Settings { language: Some("xx".into()), ..instant() };
    assert_eq!(game(&p, odd).language(), None);
}

#[test]
fn a_language_without_a_file_shows_the_script_text() {
    let p = parse(&story()).unwrap();
    let mut g = Game::new(&p, instant(), Box::new(crate::MemoryStorage::default()));
    g.start_story();
    switch(&mut g, Key::Right);
    assert_eq!(g.language(), Some("sv"));
    assert_eq!(line(&g), "Hello, you have 3 coins.");
}

#[test]
fn the_main_menu_title_and_extras_are_translated() {
    let src = format!("{HEAD}::character{{id=mika name=Mika}}\n# a\nHi.\n");
    let p = parse(&src.replace("languages:", "replays: { a: \"Scene A\" }\nlanguages:")).unwrap();
    let cat = Catalog::parse("> Tale\n= Sagan\n\n> Scene A\n= Scen A\n").unwrap();
    let mut g = Game::new(&p, instant(), Box::new(crate::MemoryStorage::default()));
    g.set_translations([("sv".to_string(), cat.clone())].into());
    assert!(texts(&g).contains(&"Tale".to_string()));
    let sv = Settings { language: Some("sv".into()), ..instant() };
    let mut g = Game::new(&p, sv, Box::new(crate::MemoryStorage::default()));
    g.set_translations([("sv".to_string(), cat)].into());
    assert!(texts(&g).contains(&"Sagan".to_string()), "{:?}", texts(&g));
}

#[test]
fn the_input_prompt_and_the_inventory_are_drawn_in_the_language_in_use() {
    let src = "---\ninventory: bag\nlanguages: en=English, sv=Svenska\nvars: { name: \"A\", bag: [lamp] }\n---\n\
               ::item{id=lamp name=\"Brass lamp\" text=\"Burns for hours.\"}\n# a\n::input{var=name prompt=\"Your name?\" default=\"Alex\"}\n::inventory\nDone.\n";
    let p = parse(src).unwrap();
    let cat = Catalog::parse("> Your name?\n= Ditt namn?\n\n> Brass lamp\n= Mässingslampa\n\n> Burns for hours.\n= Brinner i timmar.\n").unwrap();
    let sv = Settings { language: Some("sv".into()), ..instant() };
    let mut g = Game::new(&p, sv, Box::new(crate::MemoryStorage::default()));
    g.set_translations([("sv".to_string(), cat)].into());
    g.start_story();
    assert_eq!(g.screen(), Screen::Input);
    assert!(texts(&g).contains(&"Ditt namn?".to_string()), "{:?}", texts(&g));
    key(&mut g, Key::Enter); // the default answer
    assert_eq!(g.screen(), Screen::Inventory);
    assert!(texts(&g).contains(&"Mässingslampa".to_string()), "{:?}", texts(&g));
}

/// A story (in the language-test frame) and a game at its first line, with Svenska available.
fn plain_game(body: &str) -> (Program, Settings) {
    (parse(&format!("{HEAD}{body}")).unwrap(), instant())
}

#[test]
fn switching_language_during_a_wait_doesnt_stop_it_ending() {
    let (p, s) = plain_game("**Mika:** one\n::wait{time=1}\n**Mika:** two\n");
    let mut g = game(&p, s);
    key(&mut g, Key::Space); // into the wait
    switch(&mut g, Key::Right);
    g.tick(2.0, false);
    assert_eq!(line(&g), "two", "the wait still ended by itself");
}

#[test]
fn switching_language_keeps_the_cutscene_the_inline_image_and_the_end() {
    let (p, s) = plain_game("**Mika:** one\n::video{src=clip.webm}\n![a scarf](scarf.png)\n**Mika:** two\n::end\n");
    let mut g = game(&p, s);
    key(&mut g, Key::Space); // the cutscene
    switch(&mut g, Key::Right);
    assert!(g.view().video.is_some(), "the video is still wanted");
    g.video_update(true, true); // it ends
    assert_eq!(g.view().images.len(), 1);
    switch(&mut g, Key::Right);
    assert_eq!(g.view().images.len(), 1, "the inline image is still there");
    key(&mut g, Key::Space);
    assert!(g.view().finished);
    switch(&mut g, Key::Right);
    assert!(g.view().finished, "the end screen is still the end");
}
