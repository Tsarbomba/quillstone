//! [`Game`]: the whole player loop minus the window, GPU and sound card.
//!
//! The frontend feeds it [`Event`]s (mouse in virtual coordinates, abstract keys)
//! and calls [`Game::tick`] every frame; it draws [`Game::draw_list`], plays
//! [`Game::take_audio`] and carries out [`Game::take_requests`]. Which screen is
//! up (story, settings, backlog) and what each input means there lives here.

use quillstone_audio::{Action, Channel, Mixer};
use quillstone_render::{Draw, Size, Theme, View};
use quillstone_runtime::{voice_name, Cmd, Input, Logged, Session, State};
use quillstone_script::{kind_from_path, AssetKind, Program};
use quillstone_video::Wanted;

use crate::screens::*;
use crate::settings::Settings;
use crate::slots::{Page, SlotInfo, Storage};

mod extras;
mod saves;
mod skip;
use extras::Extras;
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
    Char(char),
}

#[derive(Debug, Clone, Copy, PartialEq)]
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
}

/// Extra auto-forward time per character, on top of the delay setting.
pub const AUTO_PER_CHAR: f32 = 0.02;

/// How far one keyboard press moves a settings slider.
const SLIDER_STEP: f32 = 0.05;

pub struct Game<'p> {
    program: &'p Program,
    storage: Box<dyn Storage>,
    session: Session<'p>,
    view: View,
    mixer: Mixer,
    settings: Settings,
    screen: Screen,
    auto: bool,
    /// Time the current line has been ready for auto-forward.
    auto_elapsed: f32,
    cursor: Option<(f32, f32)>,
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
}

impl<'p> Game<'p> {
    /// Open the game at its main menu. Saves are read from and written to `storage`.
    pub fn new(program: &'p Program, settings: Settings, storage: Box<dyn Storage>) -> Game<'p> {
        let (mut session, _) = Session::start(program); // replaced by Start, Continue or Load
        session.set_persistent(extras::load_persistent(&*storage));
        let read = skip::load_read(&*storage);
        let mut game = Game {
            program,
            storage,
            session,
            view: View::new().with_speed(settings.text_speed),
            mixer: Mixer::new(),
            settings_before: settings.clone(),
            settings,
            screen: Screen::Story,
            auto: false,
            auto_elapsed: 0.0,
            cursor: None,
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
            in_story: false,
            menu_focus: None,
            newest_save: None,
            video_generation: 0,
            cutscene_playing: false,
            extras: Extras::default(),
            voice_files: std::collections::HashMap::new(),
            choice_focus: None,
            slot_focus: None,
        };
        for channel in [Channel::Music, Channel::Sfx, Channel::Voice] {
            game.audio.push(Action::SetChannelVolume { channel, volume: game.channel_volume(channel) });
        }
        game.go_to_menu();
        game
    }

    /// Begin the story from the top (the main menu's Start).
    pub fn start_story(&mut self) {
        let (session, first) = Session::start_with(self.program, self.session.persistent().clone());
        self.session = session;
        self.fresh_view();
        self.audio.extend(self.mixer.apply(&[Cmd::StopMusic { fade: 0.5 }])); // the menu's music
        self.in_story = true;
        self.screen = Screen::Story;
        self.show(&first);
    }

    /// Leave the story (or start up) at the main menu.
    fn go_to_menu(&mut self) {
        self.save_read(); // the lines since the last choice (often the ending)
        self.stop_skip();
        self.auto = false;
        self.in_story = false;
        self.screen = Screen::MainMenu;
        self.menu_focus = None;
        self.fresh_view();
        let mut cmds = vec![Cmd::StopMusic { fade: 1.0 }];
        if let Some(src) = self.theme().menu_music.clone() {
            cmds.push(Cmd::PlayMusic { src, fade: 1.0, looped: true, volume: 1.0 });
        }
        self.show(&cmds);
        self.newest_save = crate::slots::newest_save(&*self.storage);
    }

    /// A view with nothing in it yet (same speed and theme).
    fn fresh_view(&mut self) {
        let theme = self.view.theme().clone();
        self.view = View::new().with_speed(self.settings.text_speed).with_theme(theme);
    }

    /// Where closing a screen goes back to.
    fn home(&self) -> Screen {
        if self.in_story {
            Screen::Story
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
        let persistent = self.session.persistent().clone();
        if self.in_story {
            let (session, cmds) = Session::load_with(program, &self.session.save(), persistent)?;
            self.program = program;
            self.session = session;
            self.fresh_view();
            self.show(&cmds);
            if self.screen == Screen::Backlog {
                self.backlog = self.session.backlog();
            }
        } else {
            self.program = program;
            let (mut session, _) = Session::start(program); // replaced by Start, Continue or Load
            session.set_persistent(persistent);
            self.session = session;
        }
        self.choice_focus = None;
        match self.screen {
            Screen::Extras if self.has_extras() => self.open_extras(),
            Screen::Extras => self.screen = Screen::MainMenu,
            Screen::Save | Screen::Load => self.refresh_slots(),
            _ => {}
        }
        Ok(())
    }

    /// Show a short message over the game (like "Saved").
    pub fn show_notice(&mut self, message: impl Into<String>) {
        self.notify(message);
    }

    /// Draw everything with this theme (see `quillstone_render::Theme`). At the
    /// main menu this also starts the theme's menu music.
    pub fn set_theme(&mut self, theme: Theme) {
        let music = theme.menu_music.clone();
        self.view.set_theme(theme);
        if !self.in_story {
            let cmd = match music {
                Some(src) => Cmd::PlayMusic { src, fade: 1.0, looped: true, volume: 1.0 },
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

    pub fn handle(&mut self, event: Event) {
        if event == Event::Key(Key::F11) {
            return self.toggle_fullscreen();
        }
        if event == Event::KeyReleased(Key::Ctrl) {
            return self.release_skip();
        }
        if let Event::CursorMoved(p) = event {
            self.cursor = p;
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
        self.route(event);
    }

    /// Send an event to the screen it is for.
    fn route(&mut self, event: Event) {
        if self.confirm.is_some() {
            return self.confirm_event(event);
        }
        match self.screen {
            Screen::MainMenu => self.menu_event(event),
            Screen::Story => self.story_event(event),
            Screen::Settings => self.settings_event(event),
            Screen::Backlog => self.backlog_event(event),
            Screen::Save | Screen::Load => self.slots_event(event),
            Screen::Extras => self.extras_event(event),
        }
    }

    /// Advance animations and timers by `dt` seconds. The story is paused while
    /// the settings or backlog screen is up. `voice_playing`: whether a voice clip
    /// is still audible (auto-forward waits for it).
    pub fn tick(&mut self, dt: f32, voice_playing: bool) {
        if let Some((_, left)) = &mut self.notice {
            *left -= dt;
            if *left <= 0.0 {
                self.notice = None;
            }
        }
        if self.screen != Screen::Story || self.confirm.is_some() {
            return;
        }
        self.view.tick(dt);
        if self.view.take_wait_finished() {
            self.forward(Input::Advance);
        }
        if self.skip.is_some() {
            return self.skip_tick(dt);
        }

        let line_ready = matches!(self.session.state().pending, Some(Cmd::Say { .. }))
            && self.view.choices.is_empty()
            && self.view.dialogue.as_ref().is_some_and(|d| d.is_complete());
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

    /// Whether [`Game::tick`] must keep being called even if the screen looks
    /// the same: timers (waits, auto-forward, skip, notices), typing, or video.
    /// When this is false and the draw list doesn't change, a frontend may stop
    /// drawing until the next input.
    pub fn needs_ticks(&self) -> bool {
        self.notice.is_some()
            || self.auto
            || self.skip.is_some()
            || (self.screen == Screen::Story && self.view.busy())
            || !self.wanted_videos().is_empty()
    }

    /// Everything to draw this frame, back to front. See [`View::draw_list`].
    pub fn draw_list(&self, image_size: &dyn Fn(&str) -> Option<Size>) -> Vec<Draw> {
        let story = self.screen == Screen::Story;
        let theme = self.view.theme();
        let mut out = if self.in_story {
            let hover_choice = self.cursor.filter(|_| story).and_then(|(x, y)| self.view.choice_at(x, y));
            let hover_choice = hover_choice.or(self.choice_focus.filter(|_| story));
            self.view.draw_list(image_size, hover_choice)
        } else {
            let buttons = menu_buttons(self.newest_save.is_some(), self.has_extras());
            let menu = self.screen == Screen::MainMenu;
            let hover = self.cursor.filter(|_| menu).and_then(|(x, y)| menu_at(x, y, &buttons));
            let title = self.program.title.as_deref().unwrap_or("Quillstone");
            let mut out = Vec::new();
            draw_main_menu(&mut out, theme, title, &buttons, self.menu_focus, hover, image_size);
            out
        };
        if self.in_story && self.quick_menu_visible() {
            let hover = self.cursor.filter(|_| story).and_then(|(x, y)| quick_at(x, y));
            draw_quick(&mut out, theme, hover, self.auto, self.skip.is_some());
        }
        if self.skip.is_some() && story {
            draw_skip_indicator(&mut out, theme);
        }
        match self.screen {
            Screen::MainMenu | Screen::Story => {}
            Screen::Settings => {
                let hover = self.cursor.and_then(|(x, y)| settings_at(x, y, self.in_story));
                draw_settings(&mut out, theme, &self.settings, self.in_story, self.focus, hover);
            }
            Screen::Backlog => draw_backlog(&mut out, theme, &self.backlog, self.backlog_scroll),
            Screen::Save | Screen::Load => {
                let tabs = self.slot_tabs();
                let hover = self.cursor.and_then(|(x, y)| slots_at(x, y, &tabs, self.slots.len()));
                let hover = hover.or(self.slot_focus.map(SlotHit::Slot));
                let title = if self.screen == Screen::Save { "Save" } else { "Load" };
                let view = SlotScreen { title, tabs: &tabs, page: self.page, slots: &self.slots, hover };
                draw_slots(&mut out, theme, &view, image_size);
            }
            Screen::Extras => self.draw_extras_screen(&mut out, image_size),
        }
        if let Some(confirm) = &self.confirm {
            let hover = self.cursor.and_then(|(x, y)| confirm_at(x, y));
            draw_confirm(&mut out, theme, &confirm.message, hover);
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
        self.program.images_ahead(self.session.state().pc, AHEAD)
    }

    // ---- video ---------------------------------------------------------------------------

    /// The videos that should be playing now: the cutscene, a video background
    /// (and the one it is crossfading from), or a video menu background.
    pub fn wanted_videos(&self) -> Vec<Wanted> {
        let is_video = |src: &str| kind_from_path(src) == AssetKind::Video;
        let looping = |src: &str| Wanted { src: src.to_string(), looped: true, generation: 0 };
        let mut out = Vec::new();
        if !self.in_story {
            out.extend(self.theme().menu_background.as_deref().filter(|s| is_video(s)).map(looping));
            return out;
        }
        if let Some(v) = &self.view.video {
            out.push(Wanted { src: v.src.clone(), looped: v.looped, generation: self.video_generation });
        }
        out.extend(self.view.background.as_ref().map(|b| b.src.as_str()).filter(|s| is_video(s)).map(looping));
        out.extend(self.view.previous_background.as_deref().filter(|s| is_video(s)).map(looping));
        out
    }

    /// Whether video clocks should stand still (a menu or dialog is over the story).
    pub fn media_paused(&self) -> bool {
        self.in_story && (self.screen != Screen::Story || self.confirm.is_some())
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
        // While skipping, a click or an advance key just stops it.
        let interrupts = matches!(
            event,
            Event::Press(Button::Left) | Event::Key(Key::Space | Key::Enter | Key::Right)
        );
        let on_quick_menu = self.cursor.is_some_and(|(x, y)| quick_at(x, y).is_some()) && self.quick_menu_visible();
        if self.skip.is_some() && interrupts && !(on_quick_menu && event == Event::Press(Button::Left)) {
            return self.stop_skip();
        }
        match event {
            Event::Press(Button::Left) => self.story_click(),
            Event::Press(Button::Right) => self.open_settings(),
            Event::Wheel(dy) if dy > 0.0 => self.rollback(),
            Event::Key(key) => match key {
                Key::Space | Key::Enter if !self.view.choices.is_empty() => {
                    if let Some(f) = self.choice_focus {
                        self.choose(f);
                    }
                }
                Key::Up | Key::Down if !self.view.choices.is_empty() => {
                    let last = self.view.choices.len() - 1;
                    self.choice_focus = Some(match (self.choice_focus, key) {
                        (None, Key::Up) => last,
                        (None, _) => 0,
                        (Some(f), Key::Up) => f.saturating_sub(1),
                        (Some(f), _) => (f + 1).min(last),
                    });
                }
                Key::Space | Key::Enter | Key::Right => {
                    if self.view.choices.is_empty() {
                        self.advance();
                    }
                }
                Key::Backspace | Key::Left => self.rollback(),
                Key::Escape => self.open_settings(),
                Key::PageUp | Key::Char('h' | 'H') => self.open_backlog(),
                Key::Char('a' | 'A') => self.toggle_auto(),
                Key::Char('s' | 'S') => self.open_slots(Screen::Save),
                Key::Char('l' | 'L') => self.open_slots(Screen::Load),
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
        let quick = self.cursor.filter(|_| self.quick_menu_visible()).and_then(|(x, y)| quick_at(x, y));
        if let Some(button) = quick {
            return match button {
                QuickButton::Back => self.rollback(),
                QuickButton::History => self.open_backlog(),
                QuickButton::Skip => self.start_skip(SkipMode::Toggled),
                QuickButton::Auto => self.toggle_auto(),
                QuickButton::Save => self.open_slots(Screen::Save),
                QuickButton::Load => self.open_slots(Screen::Load),
                QuickButton::QuickSave => self.quick_save(),
                QuickButton::QuickLoad => self.quick_load(),
                QuickButton::Settings => self.open_settings(),
            };
        }
        if self.view.choices.is_empty() {
            return self.advance();
        }
        if let Some(i) = self.cursor.and_then(|(x, y)| self.view.choice_at(x, y)) {
            self.choose(i);
        }
    }

    fn quick_menu_visible(&self) -> bool {
        self.view.dialogue.is_some() && self.view.video.is_none() && !self.view.finished
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
        out.insert(say, Cmd::PlayVoice { src: src.clone() });
        Some(out)
    }

    fn show(&mut self, cmds: &[Cmd]) {
        let voiced = self.with_auto_voice(cmds);
        let cmds = voiced.as_deref().unwrap_or(cmds);
        if cmds.iter().any(|c| matches!(c, Cmd::ShowChoices { .. })) {
            self.choice_focus = None; // new choices: nothing picked out yet
        }
        if cmds.iter().any(|c| matches!(c, Cmd::PlayVideo { .. })) {
            self.video_generation += 1;
            self.cutscene_playing = false;
        }
        if let Some(Cmd::Stopped { reason }) = cmds.iter().find(|c| matches!(c, Cmd::Stopped { .. })) {
            self.notify(format!("The story was stopped: {reason}"));
        }
        self.view.apply(cmds);
        let mut actions = self.mixer.apply(cmds);
        if self.skip.is_some() {
            // Skipping through dozens of lines a second: don't play their sounds.
            actions.retain(|a| !matches!(a, Action::PlaySfx { .. } | Action::PlayVoice { .. }));
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
        if self.view.press_advance() {
            self.forward(Input::Advance);
        }
    }

    fn choose(&mut self, index: usize) {
        if index < self.view.choices.len() {
            self.choice_focus = None;
            self.forward(Input::Choose(index));
        }
    }

    /// Move the story on. Arriving at a choice is a checkpoint: auto save.
    fn forward(&mut self, input: Input) {
        self.mark_current_read();
        let cmds = self.session.input(input);
        self.show(&cmds);
        if self.skip.is_some() {
            self.skip_arrived();
        }
        if cmds.iter().any(|c| matches!(c, Cmd::ShowChoices { .. })) {
            self.autosave();
        }
    }

    fn rollback(&mut self) {
        self.stop_skip();
        if let Some(cmds) = self.session.rollback() {
            self.show(&cmds);
            self.auto = false; // the player went back to re-read: don't hurry them on
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
        self.screen = Screen::Settings;
        self.dragging = None;
        self.settings_before = self.settings.clone();
    }

    fn close_settings(&mut self) {
        self.screen = self.home();
        self.dragging = None;
        if self.settings != self.settings_before {
            self.requests.push(Request::SaveSettings);
        }
    }

    fn settings_event(&mut self, event: Event) {
        match event {
            Event::CursorMoved(Some((x, _))) => {
                if let Some(i) = self.dragging {
                    self.set_slider(i, slider_pos_at(i, x));
                }
            }
            Event::Press(Button::Left) => match self.cursor.and_then(|(x, y)| settings_at(x, y, self.in_story)) {
                Some(SettingsHit::Control(i)) => {
                    self.focus = i;
                    match (ROWS[i].is_toggle(), self.cursor) {
                        (true, _) => self.toggle_row(ROWS[i]),
                        (false, Some((x, _))) => {
                            self.dragging = Some(i);
                            self.set_slider(i, slider_pos_at(i, x));
                        }
                        _ => {}
                    }
                }
                Some(SettingsHit::Button(SettingsButton::Back)) => self.close_settings(),
                Some(SettingsHit::Button(SettingsButton::MainMenu)) => {
                    self.close_settings();
                    self.ask("Return to the main menu?\nUnsaved progress will be lost.", Then::MainMenu);
                }
                Some(SettingsHit::Button(SettingsButton::Quit)) => {
                    self.close_settings();
                    if self.in_story {
                        self.ask("Quit the game?", Then::Quit);
                    } else {
                        self.save_read();
                        self.requests.push(Request::Quit); // nothing else to lose at the main menu
                    }
                }
                None => {}
            },
            Event::Release(Button::Left) => self.dragging = None,
            Event::Press(Button::Right) | Event::Key(Key::Escape) => self.close_settings(),
            Event::Key(Key::Up) => self.focus = self.focus.saturating_sub(1),
            Event::Key(Key::Down) => self.focus = (self.focus + 1).min(ROWS.len() - 1),
            Event::Key(Key::Left) => self.nudge(-SLIDER_STEP),
            Event::Key(Key::Right) => self.nudge(SLIDER_STEP),
            Event::Key(Key::Enter | Key::Space) if ROWS[self.focus].is_toggle() => self.toggle_row(ROWS[self.focus]),
            _ => {}
        }
    }

    fn nudge(&mut self, delta: f32) {
        match row_slider(&self.settings, ROWS[self.focus]) {
            Some(pos) => self.set_slider(self.focus, pos + delta),
            None => self.toggle_row(ROWS[self.focus]),
        }
    }

    fn toggle_row(&mut self, row: Row) {
        match row {
            Row::Fullscreen => self.toggle_fullscreen(),
            Row::SkipUnread => self.settings.skip_unread = !self.settings.skip_unread,
            _ => {}
        }
    }

    fn set_slider(&mut self, i: usize, pos: f32) {
        let row = ROWS[i];
        set_row_slider(&mut self.settings, row, pos);
        let channel = match row {
            Row::TextSpeed => return self.view.set_speed(self.settings.text_speed),
            Row::Music => Channel::Music,
            Row::Sfx => Channel::Sfx,
            Row::Voice => Channel::Voice,
            Row::AutoDelay | Row::SkipUnread | Row::Fullscreen => return,
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
        // The story can't move while the backlog is up, so a snapshot is enough.
        self.backlog = self.session.backlog();
        self.backlog_scroll = 0;
        self.screen = Screen::Backlog;
    }

    fn backlog_event(&mut self, event: Event) {
        let max = backlog_max_scroll(&self.backlog);
        let scroll_by = |s: usize, d: isize| (s as isize + d).clamp(0, max as isize) as usize;
        match event {
            // Wheeling down past the newest entry goes back to the story.
            Event::Wheel(dy) if dy < 0.0 && self.backlog_scroll == 0 => self.screen = Screen::Story,
            Event::Wheel(dy) => self.backlog_scroll = scroll_by(self.backlog_scroll, if dy > 0.0 { 1 } else { -1 }),
            Event::Key(Key::Up) => self.backlog_scroll = scroll_by(self.backlog_scroll, 1),
            Event::Key(Key::Down) => self.backlog_scroll = scroll_by(self.backlog_scroll, -1),
            Event::Key(Key::PageUp) => self.backlog_scroll = scroll_by(self.backlog_scroll, 5),
            Event::Key(Key::PageDown) => self.backlog_scroll = scroll_by(self.backlog_scroll, -5),
            Event::Key(Key::Home) => self.backlog_scroll = max,
            Event::Key(Key::End) => self.backlog_scroll = 0,
            Event::Key(Key::Escape | Key::Char('h' | 'H')) | Event::Press(_) => self.screen = Screen::Story,
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests;
