use super::test_util::{instant, key, line};
use super::*;
use quillstone_script::parse;

const SRC: &str = "---\npersistent: { warned: false }\n---\n# splash\n::set{warned = true}\n**W:** Content warning\n**W:** Second\n# start\n**A:** story\n";

/// Storage the test can still read after handing it to the game.
#[derive(Clone, Default)]
struct Shared(std::rc::Rc<std::cell::RefCell<crate::MemoryStorage>>);

impl Storage for Shared {
    fn load(&self, name: &str) -> Result<Option<String>, String> {
        self.0.borrow().load(name)
    }
    fn write(&mut self, name: &str, contents: &str) -> Result<(), String> {
        self.0.borrow_mut().write(name, contents)
    }
    fn now(&self) -> crate::Timestamp {
        self.0.borrow().now()
    }
}

impl Shared {
    fn files(&self) -> Vec<String> {
        self.0.borrow().files.keys().cloned().collect()
    }
}

/// A game at its first screen: the splash if it has one, else the main menu.
fn launch<'p>(p: &'p Program, storage: Shared) -> Game<'p> {
    let mut g = Game::new(p, instant(), Box::new(storage));
    g.start_splash();
    g
}

#[test]
fn the_splash_runs_before_the_menu_and_stops_at_its_label_end() {
    let p = parse(SRC).unwrap();
    let mut g = launch(&p, Shared::default());
    assert!(g.in_story() && g.screen() == Screen::Story);
    assert_eq!(line(&g), "Content warning");
    key(&mut g, Key::Space);
    assert_eq!(line(&g), "Second");
    key(&mut g, Key::Space); // the splash ends here, not in `start`
    assert_eq!((g.screen(), g.in_story()), (Screen::MainMenu, false));
    g.start_story();
    assert_eq!(line(&g), "story", "a new game starts past the splash");
}

#[test]
fn escape_skips_the_splash_and_what_it_set_before_stays() {
    let p = parse(SRC).unwrap();
    let storage = Shared::default();
    let mut g = launch(&p, storage.clone());
    key(&mut g, Key::Escape);
    assert_eq!(g.screen(), Screen::MainMenu);
    assert_eq!(g.session.persistent()["warned"], Value::Bool(true));
    assert!(storage.0.borrow().files.get("persistent.txt").is_some_and(|t| t.contains("warned")));
}

#[test]
fn nothing_saves_or_rolls_back_in_the_splash() {
    let p = parse(SRC).unwrap();
    let storage = Shared::default();
    let mut g = launch(&p, storage.clone());
    key(&mut g, Key::Space);
    for k in [Key::F5, Key::Char('s'), Key::Char('l'), Key::Backspace, Key::Left, Key::PageUp, Key::Tab] {
        key(&mut g, k);
    }
    g.handle(Event::Wheel(1.0));
    assert_eq!((g.screen(), line(&g).as_str()), (Screen::Story, "Second"));
    assert!(!g.quick_menu_shown());
    g.autosave();
    assert!(storage.files().iter().all(|f| f.contains("persistent")), "{:?}", storage.files());
}

#[test]
fn a_choice_works_in_the_splash() {
    let p = parse("# start\n**A:** story\n::end\n# splash\n::choice\n- [Yes](#yes)\n- [No](#no)\n::\n# yes\n**A:** yes\n::return\n# no\n**A:** no\n::return\n").unwrap();
    let mut g = launch(&p, Shared::default());
    key(&mut g, Key::Char('2'));
    assert_eq!(line(&g), "no");
    key(&mut g, Key::Space);
    assert_eq!(g.screen(), Screen::MainMenu);
}

#[test]
fn a_splash_with_nothing_to_wait_for_goes_straight_to_the_menu() {
    let p = parse("---\npersistent: { n: 0 }\n---\n# splash\n::set{n = 1}\n# start\n**A:** x\n").unwrap();
    let g = launch(&p, Shared::default());
    assert_eq!(g.screen(), Screen::MainMenu);
    assert_eq!(g.session.persistent()["n"], Value::Int(1));
}

#[test]
fn a_wait_ends_on_a_click_and_by_itself() {
    let p = parse("# splash\n::wait{time=2}\n::wait{time=1}\n# start\n**A:** x\n").unwrap();
    let mut g = launch(&p, Shared::default());
    key(&mut g, Key::Space);
    g.tick(0.5, false);
    assert_eq!(g.screen(), Screen::Story);
    g.tick(0.6, false);
    assert_eq!(g.screen(), Screen::MainMenu);
}

#[test]
fn a_game_without_a_splash_opens_at_the_menu() {
    let p = parse("**A:** x\n").unwrap();
    let g = launch(&p, Shared::default());
    assert_eq!((g.screen(), g.in_story()), (Screen::MainMenu, false));
}

#[test]
fn the_story_never_runs_into_a_splash_that_comes_first() {
    let p = parse("# splash\n**W:** splash\n# start\n**A:** story\n::end\n").unwrap();
    let mut g = Game::new(&p, instant(), Box::new(crate::MemoryStorage::default()));
    g.start_story();
    assert_eq!(line(&g), "story");
}

#[test]
fn lines_read_in_the_splash_are_not_marked_read() {
    let p = parse(SRC).unwrap();
    let storage = Shared::default();
    let mut g = launch(&p, storage.clone());
    key(&mut g, Key::Space);
    key(&mut g, Key::Space);
    g.start_story();
    key(&mut g, Key::Space);
    g.autosave();
    let read = storage.0.borrow().files.get("read.txt").cloned().unwrap_or_default();
    assert_eq!(read.lines().count(), 1 + 1, "the magic line and the story's line only: {read:?}");
}
