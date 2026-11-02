//! [`Game`]: the whole player loop minus the window, GPU and sound card.
//!
//! The frontend feeds it [`Event`]s (mouse in virtual coordinates, abstract keys)
//! and calls [`Game::tick`] every frame; it draws [`Game::draw_list`], plays
//! [`Game::take_audio`] and carries out [`Game::take_requests`]. Which screen is
//! up (story, settings, backlog) and what each input means there lives here.

use quillstone_audio::{Action, Channel, Mixer};
use quillstone_render::{hud::HudLook, textbox_rect, Draw, QuickMenu, Rect, Size, Theme, View};
use quillstone_runtime::{choice_timer, voice_name, ChoiceTimer, Cmd, Input, Logged, Session, State};
use quillstone_script::{kind_from_path, AssetKind, Hit, Program, KeyPress, TimingBar, Value, Vars};
use quillstone_video::Wanted;

use crate::screens::*;
use crate::settings::Settings;
use crate::slots::{Page, SlotInfo, Storage};

mod bag;
mod extras;
mod console;
mod hud;
mod input;
mod lang;
mod replay;
mod saves;
mod skip;
use bag::Bag;
use extras::Extras;
use input::InputField;
use replay::ReplayRun;
use crate::extras as ex;
use saves::{Confirm, Then};
pub use skip::{SkipMode, SKIP_DELAY};
#[cfg(test)]
use saves::NOTICE_SECONDS;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Button {
    Left,
    Right,
}

/// Keys the game reacts to; the frontend maps its own key events onto these.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Key {
    Space,
    Enter,
    Left,
    Right,
    Up,
    Down,
    Backspace,
    Escape,
    PageUp,
    PageDown,
    Home,
    End,
    Tab,
    /// Either Ctrl key; send [`Event::KeyReleased`] when it is let go.
    Ctrl,
    F5,
    F9,
    F11,
    /// Opens the developer console (only with [`Game::set_dev`]).
    F12,
    Char(char),
}

#[derive(Debug, Clone, PartialEq)]
pub enum Event {
    /// The cursor in virtual coordinates; `None` when it is off the picture.
    CursorMoved(Option<(f32, f32)>),
    Press(Button),
    Release(Button),
    /// Mouse wheel in notches (lines); positive is up (away from the player).
    /// Fractions add up, so touchpads can send many small steps: about 40 px of
    /// pixel scrolling is one notch.
    Wheel(f32),
    Key(Key),
    /// A key was let go (only [`Key::Ctrl`] matters).
    KeyReleased(Key),
    /// Text the player typed (what the platform delivers for a key press, or an input
    /// method's committed text). Only used while [`Game::wants_text`] is true, at an
    /// `::input`; elsewhere it is ignored. There [`Key::Char`] and [`Key::Space`] are
    /// ignored, so a frontend may send both for the same key press.
    Text(String),
}

/// Something only the frontend can do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Request {
    Quit,
    Fullscreen(bool),
    /// Write [`Game::settings`] to disk.
    SaveSettings,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Screen {
    /// The title screen: Start, Continue, Load, Settings, Quit.
    MainMenu,
    Story,
    Settings,
    Backlog,
    Save,
    Load,
    /// The main menu's Extras: gallery and music room.
    Extras,
    /// The items the game's inventory list holds (the quick menu's Items, the `I`
    /// key, or `::inventory`, which keeps the story waiting). The journal (Journal, `J`,
    /// `::journal`) is this screen too, fed by its own list.
    Inventory,
    /// A `::shop`: the story waits here, buying changes it, leaving moves on.
    Shop,
    /// An `::input`: the story waits for the player to type a text (see [`Game::wants_text`]).
    Input,
}

/// Extra auto-forward time per character, on top of the delay setting.
pub const AUTO_PER_CHAR: f32 = 0.02;

/// How long the quick menu takes to fade in or out in its `hover` mode, in seconds.
pub const QUICK_FADE: f32 = 0.15;

/// How far one keyboard press moves a settings slider.
const SLIDER_STEP: f32 = 0.05;

pub struct Game<'p> {
    program: &'p Program,
    storage: Box<dyn Storage>,
    session: Session<'p>,
    view: View,
    mixer: Mixer,
    settings: Settings,
    /// Translations by language code (see [`Game::set_translations`]).
    translations: std::collections::BTreeMap<String, quillstone_script::Catalog>,
    screen: Screen,
    auto: bool,
    /// Time the current line has been ready for auto-forward.
    auto_elapsed: f32,
    cursor: Option<(f32, f32)>,
    /// A key moved the keyboard focus: the cursor's hover look stays off until the mouse moves again.
    hover_muted: bool,
    /// Warnings from the story already shown as a notice (each shows once per run of the program).
    warned: std::collections::HashSet<String>,
    /// The settings row whose slider is being dragged.
    dragging: Option<usize>,
    /// Wheel movement not yet a whole notch (see [`Event::Wheel`]).
    wheel: f32,
    /// The settings row the keyboard is on.
    focus: usize,
    /// Settings as they were when the settings screen opened.
    settings_before: Settings,
    backlog: Vec<Logged>,
    backlog_scroll: usize,
    audio: Vec<Action>,
    requests: Vec<Request>,
    /// The page the slot screens are on, and its slots.
    page: Page,
    slots: Vec<Option<SlotInfo>>,
    /// A yes/no question on top of everything; it takes all input.
    confirm: Option<Confirm>,
    /// A short message and how many seconds it has left.
    notice: Option<(String, f32)>,
    skip: Option<SkipMode>,
    /// Time since the last skipped line.
    skip_elapsed: f32,
    /// Lines read in any playthrough (see `skip.rs`), and whether it needs writing.
    read: std::collections::HashSet<u64>,
    read_dirty: bool,
    /// What is kept across playthroughs changed and isn't written yet.
    persistent_dirty: bool,
    /// `read.txt` / `persistent.txt` exists but couldn't be read: not written this session, so it isn't lost.
    read_locked: bool,
    persistent_locked: bool,
    /// Whether a story is running (false at the main menu; overlays return here).
    in_story: bool,
    /// The main-menu button the keyboard is on.
    menu_focus: Option<usize>,
    /// The newest save and its `seq`, for the main menu's Continue and to order new saves.
    newest_save: Option<(u64, crate::slots::Slot)>,
    /// Counts cutscenes started, so the same file shown again starts over.
    video_generation: u64,
    /// Whether the frontend is really playing the cutscene (it may have no decoder).
    cutscene_playing: bool,
    extras: Extras,
    /// Auto-voice: recordings found in the script's voice folder, by
    /// `voice_name` (see [`Game::set_voice_files`]).
    voice_files: std::collections::HashMap<String, String>,
    /// The choice the keyboard (or gamepad) is on.
    choice_focus: Option<usize>,
    /// The save slot the keyboard is on, on the slot screens.
    slot_focus: Option<usize>,
    /// The inventory or shop screen.
    bag: Bag,
    /// What is typed at an `::input`.
    input: InputField,
    /// The developer console is allowed (`--dev`), and it is open.
    dev: bool,
    console: Option<crate::console::Console>,
    /// The shop, inventory or text-entry screen that Settings, the history or a save screen
    /// is open over (or that is waiting for one to close after a reload): closing
    /// that screen goes back to it instead of to the story.
    under: Option<Screen>,
    /// How visible the quick menu is in its `hover` mode (0..1); the other modes ignore it.
    quick_alpha: f32,
    /// Sizes of the theme's quick menu pictures, noted by every draw (hit-testing needs them
    /// and has no picture loader of its own).
    quick_sizes: std::cell::RefCell<std::collections::HashMap<String, Size>>,
    /// Whether a quick save exists, once known (only asked for a picture Q.Load button).
    quick_saved: std::cell::Cell<Option<bool>>,
    /// Where the next new game's `random` rolls start (see [`Game::set_seed`]).
    seed: u64,
    /// A scene replay in progress (see `replay.rs`): nothing it does is saved.
    replay: Option<ReplayRun>,
    /// Seconds a timed `::choice` has been counting down. UI only: it starts again from 0
    /// whenever the options are shown (arrival, rollback, load, reload).
    choice_elapsed: f32,
    /// Presses so far at a `::press` prompt (its clock is `choice_elapsed`); starts again with it.
    press_count: u32,
    /// Whether the window has the focus (the countdown waits while it hasn't).
    focused: bool,
    /// The window is minimised (or hidden): the whole game stands still, see [`Game::set_minimized`].
    minimized: bool,
    /// Every extras entry shows as unlocked (for testing); nothing is written for it.
    unlock_all: bool,
}

/// The session the game holds while no story runs: only the persistent values matter, and Start,
/// Continue or Load replaces it.
pub(super) fn idle_session(program: &Program, persistent: Vars) -> Session<'_> {
    let (mut session, _) = Session::start(program);
    session.set_persistent(persistent);
    session
}

impl<'p> Game<'p> {
    /// Open the game at its main menu. Saves are read from and written to `storage`.
    pub fn new(program: &'p Program, settings: Settings, storage: Box<dyn Storage>) -> Game<'p> {
        let (persistent, persistent_locked) = extras::load_persistent(&*storage);
        let session = idle_session(program, persistent);
        let (read, read_locked) = skip::load_read(&*storage);
        let mut game = Game {
            program,
            storage,
            session,
            view: View::new().with_speed(settings.text_speed).with_anims(&program.anims).with_art_scale(program.art_scale),
            mixer: Mixer::new(),
            settings_before: settings.clone(),
            settings,
            translations: std::collections::BTreeMap::new(),
            screen: Screen::Story,
            under: None,
            auto: false,
            auto_elapsed: 0.0,
            cursor: None,
            hover_muted: false,
            warned: std::collections::HashSet::new(),
            dragging: None,
            wheel: 0.0,
            focus: 0,
            backlog: Vec::new(),
            backlog_scroll: 0,
            audio: Vec::new(),
            requests: Vec::new(),
            page: Page::Numbered(1),
            slots: Vec::new(),
            confirm: None,
            notice: None,
            skip: None,
            skip_elapsed: 0.0,
            read,
            read_dirty: false,
            persistent_dirty: false,
            read_locked,
            persistent_locked,
            in_story: false,
            menu_focus: None,
            newest_save: None,
            video_generation: 0,
            cutscene_playing: false,
            extras: Extras::default(),
            voice_files: std::collections::HashMap::new(),
            choice_focus: None,
            slot_focus: None,
            bag: Bag::default(),
            input: InputField::default(),
            dev: false,
            console: None,
            quick_alpha: 0.0,
            quick_sizes: Default::default(),
            quick_saved: std::cell::Cell::new(None),
            seed: quillstone_runtime::DEFAULT_SEED,
            replay: None,
            choice_elapsed: 0.0,
            press_count: 0,
            focused: true,
            minimized: false,
            unlock_all: false,
        };
        for channel in [Channel::Music, Channel::Sfx, Channel::Voice] {
            game.audio.push(Action::SetChannelVolume { channel, volume: game.channel_volume(channel) });
        }
        game.go_to_menu();
        if persistent_locked || read_locked {
            let file = if persistent_locked { "persistent.txt" } else { "read.txt" };
            game.notify(format!("Could not read {file}: it is left alone, and changes to it won't be saved"));
        }
        game
    }

    /// Where the `random(lo, hi)` rolls of the next new game start. Frontends pass
    /// something unpredictable (the clock); the default is a fixed seed, so tests
    /// and the command line replay the same game. Each new game moves the seed
    /// on, so playing again rolls differently. A loaded save brings its own.
    pub fn set_seed(&mut self, seed: u64) {
        self.seed = seed;
    }

    /// Begin the story from the top (the main menu's Start).
    pub fn start_story(&mut self) {
        let seed = quillstone_runtime::next_seed(&mut self.seed);
        let (session, first) = Session::start_seeded(self.program, self.session.persistent().clone(), seed);
        self.enter_story(session, &first);
    }

    /// Make `session` the story on screen: a fresh view, the menu's music stopped, `first` (what its
    /// start showed) put up. (A loaded save does the same without stopping the music: see `load_from`.)
    pub(super) fn enter_story(&mut self, session: Session<'p>, first: &[Cmd]) {
        self.session = session;
        self.fresh_view();
        self.audio.extend(self.mixer.apply(&[Cmd::StopMusic { fade: 0.5 }]));
        self.in_story = true;
        self.under = None;
        self.screen = Screen::Story;
        self.show(first);
    }

    /// Leave the story (or start up) at the main menu.
    fn go_to_menu(&mut self) {
        self.save_read(); // the lines since the last choice (often the ending)
        self.stop_skip();
        self.auto = false;
        self.in_story = false;
        self.under = None;
        self.screen = Screen::MainMenu;
        self.menu_focus = None;
        self.fresh_view();
        let mut cmds = vec![Cmd::StopMusic { fade: 1.0 }];
        if self.mixer.has_sfx_loops() {
            cmds.push(Cmd::StopSfx { src: None, fade: 1.0 }); // the story's ambience ends with it
        }
        if let Some(src) = self.theme().menu_music.clone() {
            cmds.push(Cmd::PlayMusic { src, intro: None, fade: 1.0, looped: true, volume: 1.0 });
        }
        self.show(&cmds);
        self.newest_save = crate::slots::newest_save(&*self.storage);
    }

    /// A view with nothing in it yet (same speed and theme).
    fn fresh_view(&mut self) {
        let theme = self.view.theme().clone();
        self.view = View::new().with_speed(self.settings.text_speed).with_theme(theme).with_anims(&self.program.anims).with_art_scale(self.program.art_scale);
    }

    /// Where closing a screen goes back to.
    fn home(&mut self) -> Screen {
        if self.in_story {
            self.under.take().unwrap_or(Screen::Story)
        } else {
            Screen::MainMenu
        }
    }

    pub fn in_story(&self) -> bool {
        self.in_story
    }

    pub fn settings(&self) -> &Settings {
        &self.settings
    }

    pub fn screen(&self) -> Screen {
        self.screen
    }

    pub fn auto(&self) -> bool {
        self.auto
    }

    pub fn view(&self) -> &View {
        &self.view
    }

    /// Swap in an edited script (hot reload): the story carries on at the same
    /// spot, as a save of it would load (see `quillstone_runtime::Session::load`),
    /// keeping what was unlocked. On an error the old script keeps running.
    pub fn reload(&mut self, program: &'p Program) -> Result<(), String> {
        if self.replay.as_ref().is_some_and(|r| !program.labels.contains_key(&r.label)) {
            // The scene being replayed is gone: back to Extras.
            self.program = program;
            self.end_replay();
            self.notify("The replayed scene was removed from the script");
        }
        let persistent = self.session.persistent().clone();
        if self.in_story {
            let (session, cmds) = Session::reload_with(program, &self.session.save(), persistent)?;
            self.program = program;
            self.session = session;
            self.fresh_view();
            self.show(&cmds);
            // the edit may finish the story (or hit `::end_replay`) right where the player is
            self.replay_check(&cmds);
            if self.screen == Screen::Backlog {
                self.backlog = self.backlog_now();
            }
        } else {
            self.program = program;
            self.session = idle_session(program, persistent);
        }
        self.choice_focus = None;
        self.bag_resync();
        self.input_resync();
        match self.screen {
            Screen::Extras if self.has_extras() => self.refresh_extras(),
            Screen::Extras => self.screen = Screen::MainMenu,
            Screen::Save | Screen::Load => self.refresh_slots(),
            _ => {}
        }
        Ok(())
    }

    /// Draw everything with this theme (see `quillstone_render::Theme`). At the
    /// main menu this also starts the theme's menu music.
    pub fn set_theme(&mut self, theme: Theme) {
        let music = theme.menu_music.clone();
        self.mixer.set_voice_duck(theme.voice_duck);
        self.view.set_theme(theme);
        if !self.in_story {
            let cmd = match music {
                Some(src) => Cmd::PlayMusic { src, intro: None, fade: 1.0, looped: true, volume: 1.0 },
                None => Cmd::StopMusic { fade: 1.0 },
            };
            self.audio.extend(self.mixer.apply(&[cmd]));
        }
    }

    pub fn theme(&self) -> &Theme {
        self.view.theme()
    }

    pub fn state(&self) -> &State {
        self.session.state()
    }

    /// Audio actions to carry out, in order.
    pub fn take_audio(&mut self) -> Vec<Action> {
        std::mem::take(&mut self.audio)
    }

    pub fn take_requests(&mut self) -> Vec<Request> {
        std::mem::take(&mut self.requests)
    }

    /// The backend reports that a one-shot music track has ended.
    pub fn music_ended(&mut self) {
        self.mixer.music_ended();
    }

    /// The backend reports that the voice clip has ended (or could not start): music comes back up.
    pub fn voice_ended(&mut self) {
        let actions = self.mixer.voice_ended();
        self.audio.extend(actions);
    }

    /// The backend could not start the looping sound effect `src`, or lost it:
    /// the mixer forgets it, so the next `::sfx{loop}` or redraw starts it again.
    pub fn loop_failed(&mut self, src: &str) {
        self.mixer.loop_failed(src);
    }

    pub fn handle(&mut self, event: Event) {
        if event == Event::Key(Key::F11) {
            return self.toggle_fullscreen();
        }
        if event == Event::KeyReleased(Key::Ctrl) {
            return self.release_skip();
        }
        if let Event::CursorMoved(p) = event {
            self.cursor = p;
            self.hover_muted = false;
        }
        if let Event::Wheel(dy) = event {
            // One step per whole notch; turning the other way starts afresh.
            if dy.signum() != self.wheel.signum() {
                self.wheel = 0.0;
            }
            self.wheel += dy;
            let notches = self.wheel.trunc();
            self.wheel -= notches;
            for _ in 0..notches.abs() as usize {
                self.route(Event::Wheel(notches.signum()));
            }
            return;
        }
        let before = self.focus_state();
        let is_key = matches!(event, Event::Key(_));
        self.route(event);
        if is_key && self.focus_state() != before {
            self.hover_muted = true;
        }
    }

    /// Every keyboard focus there is, to tell whether a key moved one.
    #[allow(clippy::type_complexity)]
    fn focus_state(&self) -> (Option<usize>, Option<usize>, Option<usize>, usize, Option<usize>, Option<usize>) {
        (self.menu_focus, self.choice_focus, self.slot_focus, self.focus, self.bag.focus, self.extras.focus)
    }

    /// The cursor as far as hover looks go: none while the keyboard has the focus.
    fn pointer(&self) -> Option<(f32, f32)> {
        self.cursor.filter(|_| !self.hover_muted)
    }

    /// Send an event to the screen it is for.
    fn route(&mut self, event: Event) {
        if self.console.is_some() {
            return self.console_event(event);
        }
        if event == Event::Key(Key::F12) {
            return self.open_console();
        }
        if self.confirm.is_some() {
            return self.confirm_event(event);
        }
        if self.game_tab_event(&event) {
            return;
        }
        match self.screen {
            Screen::MainMenu => self.menu_event(event),
            Screen::Story => self.story_event(event),
            Screen::Settings => self.settings_event(event),
            Screen::Backlog => self.backlog_event(event),
            Screen::Save | Screen::Load => self.slots_event(event),
            Screen::Extras => self.extras_event(event),
            Screen::Inventory | Screen::Shop => self.bag_event(event),
            Screen::Input => self.input_event(event),
        }
    }

    /// Advance animations and timers by `dt` seconds. The story is paused while
    /// the settings or backlog screen is up. `voice_playing`: whether a voice clip
    /// is still audible (auto-forward waits for it).
    pub fn tick(&mut self, dt: f32, voice_playing: bool) {
        if self.minimized {
            return; // waits, typing, fades, timers, skip and auto all stand still
        }
        if self.screen != Screen::Story && self.view.hide_ui() {
            self.view.set_hide_ui(false); // hiding the interface is for the story screen only
        }
        if let Some((_, left)) = &mut self.notice {
            *left -= dt;
            if *left <= 0.0 {
                self.notice = None;
            }
        }
        if let Some(c) = &mut self.console {
            c.since_key = (c.since_key + dt) % crate::input::BLINK_PERIOD;
            return; // the game stands still under the console
        }
        if self.screen == Screen::Input {
            self.tick_input(dt);
        }
        if self.screen != Screen::Story || self.confirm.is_some() {
            return;
        }
        self.view.tick(dt);
        self.fade_quick_menu(dt);
        if self.view.take_wait_finished() {
            self.forward(Input::Advance);
        }
        if let Some(limit) = self.prompt_deadline() {
            self.choice_elapsed += dt;
            if self.choice_elapsed >= limit {
                return match self.session.state().pending {
                    Some(Cmd::ShowTiming { .. }) => self.forward(Input::Choose(Hit::Miss as usize)), // no press in time
                    Some(Cmd::ShowPress { .. }) => self.forward(Input::Choose(0)), // not pressed in time
                    // time is up: as if the default had been clicked
                    _ => {
                        if let Some(timer) = self.running_timer() {
                            self.pick(timer.default);
                        }
                    }
                };
            }
        }
        // A line ending in `{nw}` moves the story on once it is typed (no click, no auto delay).
        if self.no_wait_line() && self.view.dialogue.as_ref().is_some_and(|d| d.is_complete()) {
            return self.forward(Input::Advance);
        }
        if self.skip.is_some() {
            return self.skip_tick(dt);
        }

        // A click wait has no line of its own: auto-forward waits out the delay for it too.
        let line_ready = (self.waiting_on_a_line() || matches!(self.session.state().pending, Some(Cmd::WaitClick)))
            && self.view.dialogue.as_ref().is_none_or(|d| d.is_complete());
        if !(self.auto && line_ready && !voice_playing) {
            self.auto_elapsed = 0.0;
            return;
        }
        self.auto_elapsed += dt;
        let chars = self.view.dialogue.as_ref().map_or(0, |d| d.text.chars().count());
        if self.auto_elapsed >= self.settings.auto_delay + chars as f32 * AUTO_PER_CHAR {
            self.advance();
        }
    }

    /// The timer of the timed `::choice` or `::hotspots` on screen (shown, whether or not it counts now).
    fn choice_timer(&self) -> Option<ChoiceTimer> {
        if !self.in_story || self.screen != Screen::Story || (self.view.choices.is_empty() && self.view.hotspots.is_empty()) {
            return None;
        }
        choice_timer(self.session.state(), self.program)
    }

    /// The timer, while it counts (see [`Game::counting`]).
    fn running_timer(&self) -> Option<ChoiceTimer> {
        self.choice_timer().filter(|_| self.counting())
    }

    /// Whether the clocks of a timed choice or a timing bar run: not while the window is unfocused or the
    /// line above is still being typed (Settings, History and confirms pause the whole tick).
    fn counting(&self) -> bool {
        self.focused && self.view.dialogue.as_ref().is_none_or(|d| d.is_complete())
    }

    /// How long the prompt on screen runs before it times out, while its clock counts (`choice_elapsed`):
    /// a timed choice, a timing bar (no limit: infinite, the clock still moves its marker) or a key prompt.
    fn prompt_deadline(&self) -> Option<f32> {
        if !self.counting() {
            return None;
        }
        let in_story = self.in_story && self.screen == Screen::Story;
        match &self.session.state().pending {
            Some(Cmd::ShowTiming { bar, .. }) if in_story => Some(bar.time.unwrap_or(f32::INFINITY)),
            Some(Cmd::ShowPress { press, .. }) if in_story => Some(press.time),
            _ => self.running_timer().map(|t| t.seconds),
        }
    }

    /// The timing bar on screen. `choice_elapsed` is its clock, as for a timed choice.
    fn timing_bar(&self) -> Option<TimingBar> {
        match &self.session.state().pending {
            Some(Cmd::ShowTiming { bar, .. }) if self.in_story && self.screen == Screen::Story => Some(bar.clone()),
            _ => None,
        }
    }

    /// The key prompt on screen; `choice_elapsed` is its clock and `press_count` its presses.
    fn key_press(&self) -> Option<KeyPress> {
        match &self.session.state().pending {
            Some(Cmd::ShowPress { press, .. }) if self.in_story && self.screen == Screen::Story => Some(press.clone()),
            _ => None,
        }
    }

    /// Whether a held-down key's auto-repeats must not reach the game: at a timing bar or key prompt a
    /// press has to be a fresh one (the frontend lets the arrow keys repeat otherwise).
    pub fn wants_fresh_keys(&self) -> bool {
        self.timing_bar().is_some() || self.key_press().is_some()
    }

    /// An event at a key prompt: counts a press of its key (the last one is a hit) and swallows the keys
    /// and clicks that would otherwise advance, go back or open a menu. `false`: not for the prompt.
    fn press_event(&mut self, press: &KeyPress, event: &Event) -> bool {
        let hit = match event {
            Event::Key(Key::Escape) => return false, // the game menu, as everywhere
            Event::Key(k) => match (press.key.as_str(), k) {
                ("space" | "enter", Key::Space | Key::Enter) | ("left", Key::Left) | ("right", Key::Right) | ("up", Key::Up) | ("down", Key::Down) => true,
                (n, Key::Char(c)) if n.len() == 1 && n.starts_with(c.to_ascii_lowercase()) => true,
                (_, Key::Space | Key::Enter | Key::Left | Key::Right | Key::Up | Key::Down | Key::Backspace | Key::PageUp | Key::Char(_)) => false,
                _ => return false, // F5, F9, Tab, Ctrl: still work
            },
            Event::Press(Button::Left) if self.quick_button_under_cursor().is_none() && self.hud_button_under_cursor().is_none() => press.key == "click",
            _ => return false,
        };
        // A press while the line above is still typing only finishes it.
        if hit && self.view.press_advance() {
            self.press_count += 1;
            if self.press_count >= press.count {
                self.forward(Input::Choose(1));
            }
        }
        true
    }

    /// The window gained or lost the keyboard focus.
    pub fn set_focused(&mut self, focused: bool) {
        self.focused = focused;
    }

    /// The window was minimised or hidden (`true`) or is back (`false`). While it is, [`Game::tick`]
    /// does nothing and [`Game::media_paused`] is true, so everything resumes exactly where it stopped.
    /// (The frontend stops drawing and pauses the sound itself.)
    pub fn set_minimized(&mut self, minimized: bool) {
        self.minimized = minimized;
    }

    /// Show every gallery, music room and scene entry as unlocked for this run (the player's `--unlock-all`).
    /// Only the screen changes: the unlocks kept in `persistent.txt` are not touched.
    pub fn set_unlock_all(&mut self, on: bool) {
        self.unlock_all = on;
    }

    /// Whether the line waiting is one that ends in `{nw}`.
    fn no_wait_line(&self) -> bool {
        self.waiting_on_a_line() && self.view.dialogue.as_ref().is_some_and(|d| d.auto_advance)
    }

    /// Whether [`Game::tick`] must keep being called even if the screen looks
    /// the same: timers (waits, auto-forward, skip, notices), typing, or video.
    /// When this is false and the draw list doesn't change, a frontend may stop
    /// drawing until the next input.
    pub fn needs_ticks(&self) -> bool {
        self.notice.is_some()
            || self.screen == Screen::Input // the caret blinks
            || self.console.is_some()
            || self.auto
            || self.skip.is_some()
            || self.prompt_deadline().is_some()
            || (self.screen == Screen::Story && (self.view.busy() || self.no_wait_line() || self.quick_fading()))
            || !self.wanted_videos().is_empty()
    }

    /// Everything to draw this frame, back to front. See [`View::draw_list`].
    pub fn draw_list(&self, image_size: &dyn Fn(&str) -> Option<Size>) -> Vec<Draw> {
        let story = self.screen == Screen::Story;
        let theme = self.view.theme();
        let mut out = if self.in_story {
            let at = |x: f32, y: f32| self.view.choice_at(x, y).or_else(|| self.view.hotspot_at(x, y));
            let hover_choice = self.pointer().filter(|_| story).and_then(|(x, y)| at(x, y));
            let pointed = hover_choice.is_some();
            let hover_choice = hover_choice.or(self.choice_focus.filter(|_| story));
            // The tooltip of a hotspot follows the mouse, or sits by the region when the keyboard picked it.
            let pointer = self.pointer().filter(|_| pointed);
            if story || self.screen == Screen::MainMenu {
                let look = HudLook { pointer: self.pointer().filter(|_| story), active: self.hud_ready() };
                self.view.draw_list_hud(image_size, hover_choice, pointer, look)
            } else {
                // A full-screen overlay is up: only the picture stays under its dim.
                self.view.draw_scene(image_size)
            }
        } else {
            let buttons = menu_buttons(self.newest_save.is_some(), self.has_extras());
            let menu = self.screen == Screen::MainMenu;
            let hover = self.pointer().filter(|_| menu).and_then(|(x, y)| menu_at(x, y, &buttons));
            let title = self.tr(self.program.title.as_deref().unwrap_or("Quillstone"));
            let mut out = Vec::new();
            if menu {
                draw_main_menu(&mut out, theme, title, &buttons, self.menu_focus, hover, image_size);
            } else {
                // Settings, Load or Extras opened from the menu: only its background stays under them.
                draw_main_menu(&mut out, theme, "", &[], None, None, image_size);
            }
            out
        };
        // Only a timing bar or key prompt needs the translated pending command.
        match (self.timing_bar(), self.key_press()) {
            (Some(bar), _) => {
                if let Some(Cmd::ShowTiming { prompt, .. }) = self.pending_shown() {
                    draw_timing_bar(&mut out, theme, &prompt, &bar, bar.marker(self.choice_elapsed), image_size);
                }
            }
            (_, Some(press)) => {
                if let Some(Cmd::ShowPress { prompt, .. }) = self.pending_shown() {
                    draw_press_prompt(&mut out, theme, &prompt, &press, self.press_count, self.choice_elapsed, image_size);
                }
            }
            _ => {}
        }
        if let Some(timer) = self.choice_timer() {
            draw_choice_timer(&mut out, theme, self.view.choices.len(), 1.0 - self.choice_elapsed / timer.seconds);
        }
        if self.in_story {
            // The quick menu's pictures: asked for every frame of the story (so they stay loaded
            // even while the menu is hidden), their sizes kept for hit-testing.
            let mut sizes = self.quick_sizes.borrow_mut();
            for src in theme.quick_pictures().into_iter().chain(self.view.hud_button_pictures()) {
                if let Some(size) = image_size(src) {
                    sizes.insert(src.to_string(), size);
                }
            }
        }
        if self.in_story && story && self.quick_menu_shown() {
            let layout = self.quick_layout();
            let hover = self.pointer().filter(|_| story).and_then(|(x, y)| layout.at(x, y)).filter(|_| {
                !self.pointer().is_some_and(|(x, y)| self.view.hotspot_at(x, y).is_some())
            });
            let alpha = self.quick_menu_alpha() * self.view.box_alpha();
            let look = QuickLook {
                hover,
                auto: self.auto,
                skipping: self.skip.is_some(),
                back_off: !self.session.can_rollback(),
                qload_off: self.quick_load_missing(),
            };
            let sizes = self.quick_sizes.borrow();
            draw_quick(&mut out, theme, &layout, &look, &|s| sizes.get(s).copied(), alpha);
        }
        if self.in_story && story && !self.view.hide_ui() {
            draw_mode_badge(&mut out, theme, self.auto, self.skip.is_some());
        }
        match self.screen {
            Screen::MainMenu | Screen::Story => {}
            Screen::Settings => {
                let rows = rows(self.has_language_row());
                let hover = self.pointer().filter(|_| self.confirm.is_none()).and_then(|(x, y)| settings_at(x, y, self.in_story, rows, theme));
                let language = self.has_language_row().then(|| self.language_name());
                draw_settings(&mut out, theme, &self.settings, self.in_story, self.focus, hover, language, image_size);
            }
            Screen::Backlog => draw_backlog(&mut out, theme, &self.backlog, self.backlog_scroll, image_size),
            Screen::Save | Screen::Load => {
                let tabs = self.slot_tabs();
                let hover = self.pointer().and_then(|(x, y)| slots_at(x, y, &tabs, self.slots.len()));
                let hover = hover.or(self.slot_focus.map(SlotHit::Slot));
                let title = if self.screen == Screen::Save { "Save" } else { "Load" };
                let view = SlotScreen { title, tabs: &tabs, page: self.page, slots: &self.slots, hover };
                draw_slots(&mut out, theme, &view, image_size);
            }
            Screen::Extras => self.draw_extras_screen(&mut out, image_size),
            Screen::Inventory | Screen::Shop => self.draw_bag_screen(&mut out, image_size),
            Screen::Input => self.draw_input_screen(&mut out, image_size),
        }
        if self.current_tab().is_some() {
            let tabs = game_tabs(self.in_story);
            let hover = self.pointer().filter(|_| self.confirm.is_none()).and_then(|(x, y)| game_tab_at(x, y, tabs));
            draw_game_tabs(&mut out, theme, tabs, self.current_tab(), hover);
        }
        if let Some(confirm) = &self.confirm {
            let hover = self.pointer().and_then(|(x, y)| confirm_at(x, y));
            draw_confirm(&mut out, theme, &confirm.message, hover);
        }
        if let Some(c) = &self.console {
            crate::console::draw_console(&mut out, theme, c);
        }
        if let Some((message, _)) = &self.notice {
            draw_notice(&mut out, theme, message);
        }
        out
    }

    /// Images the story may show soon, nearest first, to load before they are needed
    /// (what is on screen now the frontend sees in the draw list).
    pub fn upcoming_images(&self) -> Vec<&'p str> {
        const AHEAD: usize = 40; // ops: a few lines' worth on every branch
        if !self.in_story {
            return Vec::new();
        }
        let pc = self.session.state().pc;
        let mut out = self.program.images_ahead(pc, AHEAD);
        // Every frame of an animation on stage now: later frames are not in the draw list yet.
        for id in self.view.shown_animations() {
            if let Some(anim) = self.program.anims.get(id) {
                out.extend(anim.frames.iter().map(|f| f.file.as_str()));
            }
        }
        // Item pictures: only those of a shop or inventory the story is about to open
        // (the ones a screen is showing come from its draw list), never the whole game's.
        let (inventory, journal, shops) = self.program.bag_icons_ahead(pc, AHEAD);
        out.extend(shops);
        let vars = &self.session.state().vars;
        let lists = [(inventory, &self.program.inventory), (journal, &self.program.journal), (journal, &self.program.journal_done)];
        for name in lists.into_iter().filter_map(|(on, name)| name.as_ref().filter(|_| on)) {
            if let Some(Value::List(held)) = vars.get(name) {
                out.extend(held.iter().filter_map(|id| self.program.items.get(id)?.icon.as_deref()));
            }
        }
        out
    }

    // ---- video ---------------------------------------------------------------------------

    /// The videos that should be playing now: the cutscene, a video background
    /// (and the one it is crossfading from), a video menu background, or the gallery clip being viewed.
    pub fn wanted_videos(&self) -> Vec<Wanted> {
        let is_video = |src: &str| kind_from_path(src) == AssetKind::Video;
        let looping = |src: &str| Wanted { src: src.to_string(), looped: true, sound: false, generation: 0 };
        let mut out = Vec::new();
        // A gallery clip open full screen is the only thing playing, and the only looping one with sound
        // (backgrounds are muted unless `::bg{.. sound}`: they sit under the story's own music).
        if let Some(src) = self.viewed_video() {
            return vec![Wanted { sound: true, ..looping(src) }];
        }
        if !self.in_story {
            out.extend(self.theme().menu_background.as_deref().filter(|s| is_video(s)).map(looping));
            return out;
        }
        // The cutscene takes over its own file: `Videos` keys clips by file, so a looping background of
        // the same file would be kept and stand in for it, and the cutscene would never end.
        let cutscene = self.view.video.as_ref().map(|v| v.src.as_str());
        if let Some(v) = &self.view.video {
            out.push(Wanted { src: v.src.clone(), looped: v.looped, sound: true, generation: self.video_generation });
        }
        // The one being crossfaded away is muted at once; the sound of the new one starts as its picture does.
        let backgrounds = [
            self.view.background.as_ref().map(|b| (b.src.as_str(), b.sound)),
            self.view.previous_background.as_deref().map(|s| (s, false)),
        ];
        let wanted = backgrounds.into_iter().flatten().filter(|(s, _)| is_video(s) && Some(*s) != cutscene);
        out.extend(wanted.map(|(s, sound)| Wanted { sound, ..looping(s) }));
        out
    }

    /// Whether video clocks should stand still (a menu or dialog is over the story, or the window is minimised).
    pub fn media_paused(&self) -> bool {
        self.minimized || self.in_story && (self.screen != Screen::Story || self.confirm.is_some())
    }

    /// The frontend reports on the cutscene each frame: whether it is really
    /// playing, and whether it has played to its end (then the story moves on).
    pub fn video_update(&mut self, playing: bool, finished: bool) {
        self.cutscene_playing = playing;
        let waiting = matches!(self.session.state().pending, Some(Cmd::PlayVideo { .. }));
        if finished && waiting && !self.media_paused() {
            self.cutscene_playing = false;
            self.forward(Input::Advance);
        }
    }

    // ---- the main menu ---------------------------------------------------------------

    fn menu_event(&mut self, event: Event) {
        let buttons = menu_buttons(self.newest_save.is_some(), self.has_extras());
        let pick = match event {
            Event::Press(Button::Left) => self.cursor.and_then(|(x, y)| menu_at(x, y, &buttons)),
            Event::Key(Key::Up) => {
                self.menu_focus = Some(self.menu_focus.map_or(0, |f| f.saturating_sub(1)));
                None
            }
            Event::Key(Key::Down) => {
                self.menu_focus = Some(self.menu_focus.map_or(0, |f| (f + 1).min(buttons.len() - 1)));
                None
            }
            Event::Key(Key::Enter | Key::Space) => self.menu_focus.and_then(|f| buttons.get(f).copied()),
            _ => None,
        };
        match pick {
            Some(MenuButton::Start) => self.start_story(),
            Some(MenuButton::Continue) => {
                if let Some((_, slot)) = self.newest_save {
                    self.load_from(slot);
                }
            }
            Some(MenuButton::Load) => self.open_slots(Screen::Load),
            Some(MenuButton::Extras) => self.open_extras(),
            Some(MenuButton::Settings) => self.open_settings(),
            Some(MenuButton::Quit) => {
                self.save_read();
                self.requests.push(Request::Quit);
            }
            None => {}
        }
    }

    // ---- the story ----------------------------------------------------------------

    fn story_event(&mut self, event: Event) {
        // With the interface hidden, the next key press or click only brings it back.
        if self.view.hide_ui() && matches!(event, Event::Key(_) | Event::Press(_)) {
            return self.view.set_hide_ui(false);
        }
        // While skipping, a click or an advance key just stops it.
        let interrupts = matches!(
            event,
            Event::Press(Button::Left) | Event::Key(Key::Space | Key::Enter | Key::Right)
        );
        let on_quick_menu = self.quick_button_under_cursor().is_some();
        if self.skip.is_some() && interrupts && !(on_quick_menu && event == Event::Press(Button::Left)) {
            return self.stop_skip();
        }
        if let Some(press) = self.key_press() {
            if self.press_event(&press, &event) {
                return;
            }
        }
        match event {
            Event::Press(Button::Left) => self.story_click(),
            Event::Press(Button::Right) => self.open_game_menu(),
            Event::Wheel(dy) if dy > 0.0 => self.rollback(),
            Event::Wheel(dy) if dy < 0.0 => self.roll_forward(),
            Event::Key(key) => match key {
                Key::Space | Key::Enter if !self.view.choices.is_empty() || !self.view.hotspots.is_empty() => {
                    // A region following a sprite that has gone can't be picked (the mouse can't hit it either).
                    let gone = |g: &Game, f: usize| g.view.hotspots.get(f).is_some_and(|s| g.view.spot_rect(s).is_none());
                    if let Some(f) = self.choice_focus.filter(|&f| !gone(self, f)) {
                        self.pick(f);
                    }
                }
                // Hotspots: the arrows cycle through the regions (Backspace still goes back).
                Key::Up | Key::Down | Key::Left | Key::Right if !self.view.hotspots.is_empty() => {
                    // Hidden regions are skipped: focus would give them away. So are those following a sprite
                    // that isn't on screen.
                    let shown: Vec<usize> = (0..self.view.hotspots.len())
                        .filter(|&i| !self.view.hotspots[i].hidden && self.view.spot_rect(&self.view.hotspots[i]).is_some())
                        .collect();
                    let backwards = matches!(key, Key::Up | Key::Left);
                    let next = match (self.choice_focus, backwards) {
                        _ if shown.is_empty() => None,
                        (None, true) => shown.last().copied(),
                        (None, false) => shown.first().copied(),
                        (Some(f), true) => shown.iter().rev().find(|&&i| i < f).or(shown.last()).copied(),
                        (Some(f), false) => shown.iter().find(|&&i| i > f).or(shown.first()).copied(),
                    };
                    if next.is_some() {
                        self.choice_focus = next;
                    }
                }
                Key::Up | Key::Down if !self.view.choices.is_empty() => {
                    // Greyed options are skipped; at either end the focus stays.
                    let open: Vec<usize> = (0..self.view.choices.len()).filter(|&i| !self.view.is_locked(i)).collect();
                    let up = matches!(key, Key::Up);
                    let next = match self.choice_focus {
                        None if up => open.last(),
                        None => open.first(),
                        Some(f) if up => open.iter().rev().find(|&&i| i < f),
                        Some(f) => open.iter().find(|&&i| i > f),
                    };
                    self.choice_focus = next.copied().or(self.choice_focus);
                }
                Key::Space | Key::Enter | Key::Right => {
                    if self.view.choices.is_empty() {
                        self.advance();
                    }
                }
                Key::Backspace | Key::Left => self.rollback(),
                Key::Escape => self.open_game_menu(),
                Key::PageUp | Key::Char('h' | 'H') => self.open_backlog(),
                Key::Char('a' | 'A') => self.toggle_auto(),
                Key::Char('s' | 'S') => self.open_slots(Screen::Save),
                Key::Char('l' | 'L') => self.open_slots(Screen::Load),
                Key::Char('i' | 'I') => self.open_inventory_overlay(),
                Key::Char('j' | 'J') => self.open_journal_overlay(),
                Key::Char('u' | 'U') => self.view.set_hide_ui(true),
                Key::F5 => self.quick_save(),
                Key::F9 => self.quick_load(),
                Key::Tab => self.start_skip(SkipMode::Toggled),
                Key::Ctrl => self.start_skip(SkipMode::Held),
                Key::Char(c) => {
                    if let Some(n) = c.to_digit(10).filter(|&n| n >= 1) {
                        self.choose(n as usize - 1);
                    }
                }
                _ => {}
            },
            _ => {}
        }
    }

    fn story_click(&mut self) {
        if let Some(button) = self.quick_button_under_cursor() {
            return self.quick_press(button);
        }
        if let Some((id, action)) = self.hud_button_under_cursor() {
            return self.hud_press(&id, action);
        }
        self.story_click_on_stage();
    }

    fn quick_press(&mut self, button: QuickButton) {
        match button {
            QuickButton::Back => self.rollback(),
            QuickButton::History => self.open_backlog(),
            QuickButton::Skip => self.start_skip(SkipMode::Toggled),
            QuickButton::Auto => self.toggle_auto(),
            QuickButton::Save => self.open_slots(Screen::Save),
            QuickButton::Load => self.open_slots(Screen::Load),
            QuickButton::QuickSave => self.quick_save(),
            QuickButton::QuickLoad => self.quick_load(),
            QuickButton::Settings => self.open_settings(),
            QuickButton::Items => self.open_inventory_overlay(),
            QuickButton::Journal => self.open_journal_overlay(),
        }
    }

    /// A click that isn't on the quick menu: advance, or pick a choice or hotspot.
    fn story_click_on_stage(&mut self) {
        if self.view.choices.is_empty() && self.view.hotspots.is_empty() {
            return self.advance();
        }
        if let Some(i) = self.cursor.and_then(|(x, y)| self.view.choice_at(x, y).or_else(|| self.view.hotspot_at(x, y))) {
            self.pick(i);
        }
    }

    /// The quick menu button under the cursor; none where a hotspot overlaps it
    /// (a region has priority over the quick menu and the textbox).
    fn quick_button_under_cursor(&self) -> Option<QuickButton> {
        let (x, y) = self.cursor.filter(|_| self.quick_menu_clickable())?;
        self.quick_layout().at(x, y).filter(|_| self.view.hotspot_at(x, y).is_none())
    }

    /// Where the quick menu's buttons are now (the theme's, with the pictures' known sizes).
    fn quick_layout(&self) -> QuickLayout {
        let sizes = self.quick_sizes.borrow();
        quick_layout(self.textbox(), self.theme(), self.has_inventory(), self.has_journal(), &|s| sizes.get(s).copied())
    }

    /// There is no quick save for Q.Load to load (only worked out when Q.Load is a picture).
    fn quick_load_missing(&self) -> bool {
        if self.theme().quick_icons[7].icon.is_none() {
            return false;
        }
        let saved = self.quick_saved.get().unwrap_or_else(|| {
            let found = crate::slots::newest_slot(&*self.storage, crate::slots::Page::Quick).is_some();
            self.quick_saved.set(Some(found));
            found
        });
        !saved
    }

    /// The textbox as the theme places it (the quick menu is laid out from it).
    fn textbox(&self) -> Rect {
        textbox_rect(self.theme())
    }

    /// The quick menu's mode: the player's choice, else the theme's.
    pub fn quick_menu_mode(&self) -> QuickMenu {
        self.settings.quick_menu.resolve(self.theme().quick_menu)
    }

    /// There is a dialogue box for the quick menu to belong to.
    fn quick_menu_possible(&self) -> bool {
        self.view.dialogue.is_some() && self.view.video.is_none() && !self.view.finished
    }

    /// Whether the cursor is where the `hover` mode shows the menu.
    fn cursor_in_quick_zone(&self) -> bool {
        let (tb, layout) = (self.textbox(), self.quick_layout());
        self.cursor.is_some_and(|(x, y)| layout.in_zone(tb, x, y))
    }

    /// How visible the menu is by its mode alone (`hover` fades with time).
    fn quick_menu_alpha(&self) -> f32 {
        match self.quick_menu_mode() {
            QuickMenu::Visible => 1.0,
            QuickMenu::Hover => self.quick_alpha,
            QuickMenu::Hidden => 0.0,
        }
    }

    /// Whether the menu is drawn at all (it may be fading): not when the script
    /// hides the box (once the fade is over) or the player hid the interface.
    fn quick_menu_shown(&self) -> bool {
        self.quick_menu_possible() && self.quick_menu_alpha() * self.view.box_alpha() > 0.0
    }

    /// Whether its buttons react to the mouse: shown for real (half way through a
    /// fade-in at least), not hidden by the script, `U` or the mode.
    fn quick_menu_clickable(&self) -> bool {
        self.quick_menu_possible()
            && !self.view.textbox_hidden()
            && !self.view.hide_ui()
            && self.quick_menu_alpha() >= 0.5
            && (self.quick_menu_mode() != QuickMenu::Hover || self.cursor_in_quick_zone())
    }

    /// Move the `hover` mode's fade towards where the cursor says it should be.
    fn fade_quick_menu(&mut self, dt: f32) {
        let step = dt / QUICK_FADE;
        let target = self.quick_target();
        self.quick_alpha =
            if self.quick_alpha < target { (self.quick_alpha + step).min(target) } else { (self.quick_alpha - step).max(target) };
    }

    fn quick_target(&self) -> f32 {
        let shown = self.quick_menu_mode() == QuickMenu::Hover && self.cursor_in_quick_zone();
        if shown {
            1.0
        } else {
            0.0
        }
    }

    fn quick_fading(&self) -> bool {
        self.quick_menu_mode() == QuickMenu::Hover && self.quick_alpha != self.quick_target()
    }

    /// Auto-voice: the recordings there are, as `voice_name` -> file (relative to
    /// the script). A line with one plays it as if it had a `::voice`.
    pub fn set_voice_files(&mut self, files: std::collections::HashMap<String, String>) {
        self.voice_files = files;
    }

    /// The batch with the auto-voice of the line it shows, if it has one and no
    /// `::voice` of its own. (The batch always ends at the line the session is on.)
    fn with_auto_voice(&self, cmds: &[Cmd]) -> Option<Vec<Cmd>> {
        if self.voice_files.is_empty() || cmds.iter().any(|c| matches!(c, Cmd::PlayVoice { .. })) {
            return None;
        }
        let say = cmds.iter().position(|c| matches!(c, Cmd::Say { .. }))?;
        let src = self.voice_files.get(&voice_name(self.program, self.session.state().pc)?)?;
        let mut out = cmds.to_vec();
        out.insert(say, Cmd::PlayVoice { src: src.clone(), volume: 1.0 });
        Some(out)
    }

    /// Tell the view which character is speaking (for `speaker_highlight`): the one whose declared name is the
    /// line's speaker, in the script's language or the translated one. A name with `{variables}` matches nobody.
    fn sync_speaker(&mut self, snap: bool) {
        let name = self.view.dialogue.as_ref().and_then(|d| d.speaker.as_deref());
        let is = |c: &&quillstone_script::Character| Some(c.name.as_str()) == name || self.catalog().is_some_and(|cat| Some(cat.text(None, &c.name)) == name);
        let id = name.and_then(|_| self.program.characters.values().find(is)).map(|c| c.id.clone());
        self.view.set_speaker(id, snap);
    }

    fn show(&mut self, cmds: &[Cmd]) {
        let localized = self.localized(cmds);
        let cmds = localized.as_deref().unwrap_or(cmds);
        let voiced = self.with_auto_voice(cmds);
        let cmds = voiced.as_deref().unwrap_or(cmds);
        if cmds.iter().any(|c| matches!(c, Cmd::ShowChoices { .. } | Cmd::ShowHotspots { .. } | Cmd::ShowTiming { .. } | Cmd::ShowPress { .. })) {
            self.choice_focus = None; // new choices: nothing picked out yet
            self.choice_elapsed = 0.0; // and a timed one (or a timing bar) starts counting from full
            self.press_count = 0;
        }
        if cmds.iter().any(|c| matches!(c, Cmd::PlayVideo { .. })) {
            self.video_generation += 1;
            self.cutscene_playing = false;
        }
        if let Some(Cmd::Stopped { reason }) = cmds.iter().find(|c| matches!(c, Cmd::Stopped { .. })) {
            self.notify(format!("The story was stopped: {reason}"));
        }
        for message in cmds.iter().filter_map(|c| match c {
            Cmd::Warning { message } => Some(message),
            _ => None,
        }) {
            if self.warned.insert(message.clone()) {
                self.notify(message.clone());
            }
        }
        self.view.apply(cmds);
        self.sync_speaker(cmds.first() == Some(&Cmd::ClearScene));
        if !self.view.hud().is_empty() {
            self.view.set_vars(&self.session.state().vars); // the HUD shows them live
        }
        self.bag_arrived(cmds);
        self.input_arrived(cmds);
        // Skipping plays no voices, so the mixer must not duck for one.
        let mut actions = if self.skip.is_some() {
            let kept: Vec<Cmd> = cmds.iter().filter(|c| !matches!(c, Cmd::PlayVoice { .. })).cloned().collect();
            self.mixer.apply(&kept)
        } else {
            self.mixer.apply(cmds)
        };
        if self.skip.is_some() {
            // Skipping through dozens of lines a second: no flashes, and don't play
            // one-shot sounds or voices. Looping sounds belong to the scene and are
            // never muted, so what plays still agrees with the story afterwards.
            self.view.end_flash();
            self.view.end_cover_fade();
            actions.retain(|a| !matches!(a, Action::PlaySfx { .. }));
        }
        self.audio.extend(actions);
        self.auto_elapsed = 0.0;
        self.save_persistent();
    }

    fn advance(&mut self) {
        if self.view.finished {
            return self.go_to_menu(); // the end: back to the title screen
        }
        let unskippable = matches!(self.session.state().pending, Some(Cmd::PlayVideo { skippable: false, .. }));
        if unskippable && self.cutscene_playing {
            return; // it advances by itself when it ends
        }
        if let Some(bar) = self.timing_bar() {
            // Like any press it first finishes the line being typed; then it stops the marker where it is.
            if self.view.press_advance() {
                self.forward(Input::Choose(bar.hit(self.choice_elapsed) as usize));
            }
            return;
        }
        if self.view.press_advance() {
            self.forward(Input::Advance);
        }
    }

    /// Number keys: pick a choice button (hotspots can't be picked by number).
    fn choose(&mut self, index: usize) {
        if index < self.view.choices.len() {
            self.pick(index);
        }
    }

    /// Pick the choice or hotspot at `index` of the visible list (not a greyed option).
    fn pick(&mut self, index: usize) {
        if self.view.is_locked(index) {
            return;
        }
        if index < self.view.choices.len() || index < self.view.hotspots.len() {
            self.choice_focus = None;
            self.forward(Input::Choose(index));
        }
    }

    /// Move the story on. Arriving at a choice, shop or text entry is a checkpoint: auto save.
    fn forward(&mut self, input: Input) {
        // Using an item leaves the line unread: it comes back after the label.
        if !matches!(input, Input::Use(_) | Input::Hud(_)) {
            self.mark_current_read();
        }
        let purchase = matches!(input, Input::Buy(_));
        let cmds = self.session.input(input);
        self.show(&cmds);
        if self.replay.is_some() {
            self.replay_check(&cmds);
            if self.replay.is_none() {
                return;
            }
        }
        if self.skip.is_some() {
            self.skip_arrived();
        }
        // Arriving at a choice, a shop or a text entry is a checkpoint (a purchase, which shows the shop again, is not).
        let arrived = cmds
            .iter()
            .any(Cmd::is_prompt);
        if arrived && !purchase && self.replay.is_none() {
            self.autosave();
        }
    }

    fn rollback(&mut self) {
        self.stop_skip();
        if let Some(cmds) = self.session.rollback() {
            self.view.dialogue = None; // redraw sends a line only when the screen has one
            self.show(&cmds);
            self.auto = false; // the player went back to re-read: don't hurry them on
        }
    }

    /// Undo a rollback (wheel down). Unlike going back it leaves auto alone.
    fn roll_forward(&mut self) {
        if !self.session.can_roll_forward() {
            return;
        }
        self.stop_skip();
        self.mark_current_read();
        if let Some(cmds) = self.session.roll_forward() {
            self.view.dialogue = None;
            self.show(&cmds);
        }
    }

    fn toggle_auto(&mut self) {
        self.auto = !self.auto;
        self.auto_elapsed = 0.0;
    }

    fn toggle_fullscreen(&mut self) {
        self.settings.fullscreen = !self.settings.fullscreen;
        self.requests.push(Request::Fullscreen(self.settings.fullscreen));
        if self.screen != Screen::Settings {
            self.requests.push(Request::SaveSettings); // otherwise saved on close
        }
    }

    // ---- settings ---------------------------------------------------------------------

    fn open_settings(&mut self) {
        self.stop_skip();
        self.cover_bag();
        self.screen = Screen::Settings;
        self.dragging = None;
        self.settings_before = self.settings.clone();
    }

    fn close_settings(&mut self) {
        self.screen = self.home();
        self.leave_settings();
    }

    /// Settings is being left (closed, or another game-menu tab opened): save what changed.
    fn leave_settings(&mut self) {
        self.dragging = None;
        if self.settings != self.settings_before {
            self.requests.push(Request::SaveSettings);
        }
    }

    /// What Esc and a right click open over the story, a shop, an inventory or a text entry: the Save
    /// screen. A replay has nothing to save, so it gets Settings (where "Main menu" leaves it).
    pub(super) fn open_game_menu(&mut self) {
        if self.replay.is_some() {
            self.open_settings();
        } else {
            self.open_slots(Screen::Save);
        }
    }

    fn current_tab(&self) -> Option<GameTab> {
        match self.screen {
            Screen::Save => Some(GameTab::Save),
            Screen::Load => Some(GameTab::Load),
            Screen::Settings => Some(GameTab::Settings),
            Screen::Backlog => Some(GameTab::History),
            _ => None,
        }
    }

    /// A click on a game-menu tab, or Tab (next screen, round again). `true`: the event was for the tabs.
    fn game_tab_event(&mut self, event: &Event) -> bool {
        let Some(current) = self.current_tab() else { return false };
        let tabs = game_tabs(self.in_story);
        let pick = match event {
            Event::Press(Button::Left) => match self.cursor.and_then(|(x, y)| game_tab_at(x, y, tabs)) {
                Some(tab) => tab,
                None => return false,
            },
            Event::Key(Key::Tab) => {
                let screens: Vec<GameTab> = tabs.iter().copied().filter(|t| t.is_screen()).collect();
                let at = screens.iter().position(|&t| t == current).unwrap_or(0);
                screens[(at + 1) % screens.len()]
            }
            _ => return false,
        };
        match pick {
            _ if pick == current => {}
            GameTab::MainMenu => self.ask_main_menu(),
            GameTab::Quit => self.ask_quit(),
            _ => {
                if self.screen == Screen::Settings {
                    self.leave_settings();
                }
                match pick {
                    GameTab::Save => self.open_slots(Screen::Save),
                    GameTab::Load => self.open_slots(Screen::Load),
                    GameTab::Settings => self.open_settings(),
                    _ => self.open_backlog(),
                }
            }
        }
        true
    }

    /// The main menu button: asks first (the screen stays open beneath the question, so No comes back to it).
    fn ask_main_menu(&mut self) {
        if self.replay.is_some() {
            self.ask("Leave this replay and go back to Extras?", Then::MainMenu);
        } else {
            self.ask("Return to the main menu?\nUnsaved progress will be lost.", Then::MainMenu);
        }
    }

    fn ask_quit(&mut self) {
        if self.in_story {
            self.ask("Quit the game?", Then::Quit);
        } else {
            if self.screen == Screen::Settings {
                self.close_settings();
            }
            self.save_read();
            self.requests.push(Request::Quit); // nothing else to lose at the main menu
        }
    }

    fn settings_event(&mut self, event: Event) {
        let rows = rows(self.has_language_row());
        self.focus = self.focus.min(rows.len() - 1);
        match event {
            Event::CursorMoved(Some((x, _))) => {
                if let Some(i) = self.dragging {
                    self.set_slider(i, slider_pos_at(x, self.view.theme()));
                }
            }
            Event::Press(Button::Left) => match self.cursor.and_then(|(x, y)| settings_at(x, y, self.in_story, rows, self.view.theme())) {
                Some(SettingsHit::Control(i)) => {
                    self.focus = i;
                    match (rows[i].is_toggle(), self.cursor) {
                        (true, _) => self.toggle_row(rows[i]),
                        (false, Some((x, _))) => {
                            self.dragging = Some(i);
                            self.set_slider(i, slider_pos_at(x, self.view.theme()));
                        }
                        _ => {}
                    }
                }
                Some(SettingsHit::Button(SettingsButton::Back)) => self.close_settings(),
                Some(SettingsHit::Button(SettingsButton::MainMenu)) => self.ask_main_menu(),
                Some(SettingsHit::Button(SettingsButton::Quit)) => self.ask_quit(),
                None => {}
            },
            Event::Release(Button::Left) => self.dragging = None,
            Event::Press(Button::Right) | Event::Key(Key::Escape) => self.close_settings(),
            Event::Key(Key::Up) => self.focus = self.focus.saturating_sub(1),
            Event::Key(Key::Down) => self.focus = (self.focus + 1).min(rows.len() - 1),
            Event::Key(Key::Left) if rows[self.focus] == Row::QuickMenu => self.cycle_quick_menu(true),
            Event::Key(Key::Left) if rows[self.focus] == Row::Language => self.cycle_language(true),
            Event::Key(Key::Left) => self.nudge(-SLIDER_STEP),
            Event::Key(Key::Right) => self.nudge(SLIDER_STEP),
            Event::Key(Key::Enter | Key::Space) if rows[self.focus].is_toggle() => self.toggle_row(rows[self.focus]),
            _ => {}
        }
    }

    fn nudge(&mut self, delta: f32) {
        let row = rows(self.has_language_row())[self.focus];
        match row_slider(&self.settings, row) {
            Some(pos) => self.set_slider(self.focus, pos + delta),
            None => self.toggle_row(row),
        }
    }

    fn toggle_row(&mut self, row: Row) {
        match row {
            Row::Fullscreen => self.toggle_fullscreen(),
            Row::SkipUnread => self.settings.skip_unread = !self.settings.skip_unread,
            Row::QuickMenu => self.cycle_quick_menu(false),
            Row::Language => self.cycle_language(false),
            _ => {}
        }
    }

    /// Game default -> Always -> On hover -> Hidden -> (round again), or the other way.
    fn cycle_quick_menu(&mut self, back: bool) {
        self.settings.quick_menu = self.settings.quick_menu.cycled(back);
    }

    fn set_slider(&mut self, i: usize, pos: f32) {
        let row = rows(self.has_language_row())[i];
        set_row_slider(&mut self.settings, row, pos);
        let channel = match row {
            Row::TextSpeed => return self.view.set_speed(self.settings.text_speed),
            Row::Music => Channel::Music,
            Row::Sfx => Channel::Sfx,
            Row::Voice => Channel::Voice,
            Row::AutoDelay | Row::SkipUnread | Row::Fullscreen | Row::QuickMenu | Row::Language => return,
        };
        self.audio.push(Action::SetChannelVolume { channel, volume: self.channel_volume(channel) });
    }

    fn channel_volume(&self, channel: Channel) -> f32 {
        match channel {
            Channel::Music => self.settings.music_volume,
            Channel::Sfx => self.settings.sfx_volume,
            Channel::Voice => self.settings.voice_volume,
        }
    }

    // ---- backlog ------------------------------------------------------------------------

    fn open_backlog(&mut self) {
        self.stop_skip();
        self.cover_bag();
        // The story can't move while the backlog is up, so a snapshot is enough.
        self.backlog = self.backlog_now();
        self.backlog_scroll = 0;
        self.screen = Screen::Backlog;
    }

    fn backlog_event(&mut self, event: Event) {
        let max = backlog_max_scroll(&self.backlog);
        let scroll_by = |s: usize, d: isize| (s as isize + d).clamp(0, max as isize) as usize;
        match event {
            Event::Wheel(dy) => self.backlog_scroll = scroll_by(self.backlog_scroll, if dy > 0.0 { 1 } else { -1 }),
            Event::Key(Key::Up) => self.backlog_scroll = scroll_by(self.backlog_scroll, 1),
            Event::Key(Key::Down) => self.backlog_scroll = scroll_by(self.backlog_scroll, -1),
            Event::Key(Key::PageUp) => self.backlog_scroll = scroll_by(self.backlog_scroll, 5),
            Event::Key(Key::PageDown) => self.backlog_scroll = scroll_by(self.backlog_scroll, -5),
            Event::Key(Key::Home) => self.backlog_scroll = max,
            Event::Key(Key::End) => self.backlog_scroll = 0,
            Event::Key(Key::Escape | Key::Char('h' | 'H')) | Event::Press(_) => self.screen = self.home(),
            _ => {}
        }
    }
}

#[cfg(test)]
mod test_util;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod input_tests;
#[cfg(test)]
mod anim_tests;
#[cfg(test)]
mod layers_tests;
#[cfg(test)]
mod dynpic_tests;
#[cfg(test)]
mod highlight_tests;
#[cfg(test)]
mod hud_tests;
#[cfg(test)]
mod particles_tests;
#[cfg(test)]
mod tint_tests;
#[cfg(test)]
mod replay_tests;
#[cfg(test)]
mod timed_tests;
#[cfg(test)]
mod press_tests;
#[cfg(test)]
mod timing_tests;
#[cfg(test)]
mod hotspot_timed_tests;
#[cfg(test)]
mod locked_tests;
#[cfg(test)]
mod lang_tests;
#[cfg(test)]
mod name_tests;
#[cfg(test)]
mod gallery_video_tests;
#[cfg(test)]
mod quickicon_tests;
#[cfg(test)]
mod console_tests;
#[cfg(test)]
mod menu_tabs_tests;
