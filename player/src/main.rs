//! Quillstone player: opens a window and plays a script.
//!
//! Controls: click / Space / Enter / Right = advance (first press finishes typing),
//! 1-9 or click = choose, Backspace / Left / mouse wheel up = go back,
//! H / Page Up = history, A = auto-forward, S / L = save / load screens,
//! F5 / F9 = quick save / quick load, Tab = skip on/off, hold Ctrl = skip,
//! Esc / right-click = settings, F11 = fullscreen. The game logic lives in `quillstone-ui`; this file only
//! turns window events into its `Event`s and carries out what comes back.

// A shipped game opens no console window (build with `--features no-console`).
#![cfg_attr(feature = "no-console", windows_subsystem = "windows")]

mod audio;
#[cfg(feature = "audio")]
mod ffaudio;
mod gamepad;
mod images;
mod reload;
mod gfx;
mod setup;
mod text;
mod video;

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::exit;
use std::sync::Arc;
use std::time::{Duration, Instant};

use quillstone_render::{build_frame, letterbox, to_virtual, Size};
use quillstone_script::{AssetKind, Op, Program};
use quillstone_ui::{Button, Event, Game, Key, Request, Settings, Storage, Timestamp, MAX_SAVE_BYTES};
use quillstone_video::Videos;
use winit::application::ApplicationHandler;
use winit::dpi::LogicalSize;
use winit::event::{ElementState, Ime, KeyEvent, MouseButton, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::{Key as WinitKey, NamedKey};
use winit::window::{Fullscreen, Window, WindowId};

use audio::Audio;
use gfx::Gfx;
use setup::{load_fonts, load_theme};

struct App {
    title: String,
    game: Game<'static>,
    gamepads: gamepad::Gamepads,
    /// The script file and its folder, for hot reload.
    script_path: PathBuf,
    root: PathBuf,
    watch: reload::Watch,
    /// Where settings are saved; `None` if no per-user folder could be found.
    settings_file: Option<PathBuf>,
    /// `None` when there is no sound device (or the `audio` feature is off).
    audio: Option<Audio>,
    /// Images on the GPU come and go as the story needs them (see `images.rs`).
    images: images::Images,
    image_sizes: HashMap<String, Size>,
    /// The theme's font files (regular first), handed to the text renderer when the GPU exists.
    fonts: Vec<Vec<u8>>,
    videos: Videos,
    /// The frame of each playing video that is on the GPU (to upload only new ones).
    uploaded: HashMap<String, u64>,
    window: Option<Arc<Window>>,
    gfx: Option<Gfx>,
    last_frame: Instant,
    /// The last frame's draw list, and whether it settled: the same as the one
    /// before and nothing timed going on. A settled game isn't redrawn until an
    /// input arrives (or now and then, just in case), so a still screen costs
    /// no CPU or GPU.
    last_draws: Vec<quillstone_render::Draw>,
    settled: bool,
    /// An input arrived since the last frame.
    dirty: bool,
    /// The screen needs images still being decoded: the previous frame stays up, the
    /// game's clock stands still and story input is ignored (see `redraw`).
    holding: bool,
    /// The window's text-input method (IME) is on: only while the game wants typed text.
    ime_on: bool,
    /// When the current wait for images began, and what the last shown frame used.
    hold_since: Option<Instant>,
    last_asked: Vec<String>,
}

/// The longest a frame is held back for images still loading.
const HOLD_MAX: Duration = Duration::from_secs(1);

/// How often a settled screen is drawn anyway, and how often gamepads are polled meanwhile.
const SETTLED_REDRAW: Duration = Duration::from_millis(250);
const GAMEPAD_POLL: Duration = Duration::from_millis(16);

fn main() {
    // A crash in a double-clicked game would vanish with its console: keep the message.
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        default_hook(info);
        // A decoder panic is caught and only fails that image: not a crash.
        if std::thread::current().name() == Some(images::DECODER) {
            return;
        }
        if let Some(dir) = exe_dir() {
            let _ = std::fs::write(dir.join("error.txt"), format!("the game crashed: {info}\n"));
        }
    }));
    let path = match std::env::args().nth(1) {
        Some(p) => PathBuf::from(p),
        None => find_game().unwrap_or_else(|| {
            fatal(
                "usage: quillstone-player <script.quill.md>\n\
                 (or put the script next to the player, or in a `game` folder beside it, as main.quill.md)"
                    .into(),
            )
        }),
    };
    let source = std::fs::read_to_string(&path).unwrap_or_else(|e| fatal(format!("cannot read {}: {e}", path.display())));
    let program = quillstone_ui::source::parse_script(&path, &source)
        .unwrap_or_else(|errors| fatal(quillstone_ui::source::error_lines(&path, &errors).join("\n")));
    // The program lives as long as the app does; leaking it keeps the game simple.
    let program: &'static Program = Box::leak(Box::new(program));
    let root = path.parent().map(PathBuf::from).unwrap_or_default();

    let theme = load_theme(program, &root);
    let fonts = load_fonts(&theme, &root);

    let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("game").trim_end_matches(".quill");
    let dir = data_dir(&game_id(program.title.as_deref().unwrap_or(stem)));
    if dir.is_none() {
        eprintln!("warning: no per-user folder found; settings and saves won't be kept");
    }
    let settings_file = dir.as_ref().map(|d| d.join("settings.txt"));
    let settings = settings_file
        .as_ref()
        .and_then(|f| std::fs::read_to_string(f).ok())
        .map(|text| Settings::parse(&text))
        .unwrap_or_default();

    // One-shot sound effects are preloaded so they start instantly; music, voice and
    // looping effects stream (a long ambience is never decoded at startup).
    let sfx = program.ops.iter().filter_map(|op| match op {
        Op::Sfx { src, .. } => Some(src.as_str()),
        _ => None,
    });
    let audio = Audio::new(&root, sfx);

    let mut game = Game::new(program, settings, Box::new(FileStorage { dir }));
    game.set_voice_files(voice_files(program, &root));
    game.set_theme(theme);
    let (translations, problems) = quillstone_ui::source::load_translations(&path, program);
    for p in &problems {
        eprintln!("warning: {p}");
    }
    game.set_translations(translations);
    if let Some(first) = problems.first() {
        game.show_notice(format!("Translation not loaded: {first}"));
    }
    // `random(lo, hi)` rolls start from the clock, so every new game rolls differently.
    game.set_seed(std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos() as u64));
    let mut app = App {
        title: game.title(),
        game,
        gamepads: gamepad::Gamepads::new(),
        watch: reload::Watch::new(&path, program, &root),
        script_path: path.clone(),
        root: root.clone(),
        settings_file,
        audio,
        images: images::Images::new(root.clone()),
        image_sizes: HashMap::new(),
        fonts,
        videos: Videos::new(video::opener(&root)),
        uploaded: HashMap::new(),
        window: None,
        gfx: None,
        last_frame: Instant::now(),
        last_draws: Vec::new(),
        settled: false,
        dirty: true,
        holding: false,
        ime_on: false,
        hold_since: None,
        last_asked: Vec::new(),
    };
    app.play_audio();

    let event_loop = EventLoop::new().expect("failed to create the event loop");
    event_loop.run_app(&mut app).expect("event loop error");
}

/// Auto-voice: the sound files in the script's `voice:` folder, by name without
/// extension (which is the line's `voice_name`), as paths relative to the script.
fn voice_files(program: &Program, root: &Path) -> HashMap<String, String> {
    let Some(folder) = &program.voice_folder else { return HashMap::new() };
    let entries = match std::fs::read_dir(root.join(folder)) {
        Ok(entries) => entries,
        Err(e) => {
            eprintln!("warning: cannot read the voice folder {folder}: {e}");
            return HashMap::new();
        }
    };
    entries
        .filter_map(|e| e.ok()?.file_name().into_string().ok())
        .filter(|name| quillstone_script::kind_from_path(name) == AssetKind::Audio)
        .filter_map(|name| Some((name.rsplit_once('.')?.0.to_string(), format!("{folder}/{name}"))))
        .collect()
}

/// A folder-safe name for this game's per-user data.
fn game_id(name: &str) -> String {
    let mut id = String::new();
    for c in name.chars() {
        if c.is_alphanumeric() {
            id.extend(c.to_lowercase());
        } else if !id.ends_with('-') && !id.is_empty() {
            id.push('-');
        }
    }
    let id = id.trim_end_matches('-').to_string();
    if id.is_empty() {
        "game".into()
    } else {
        id
    }
}

fn exe_dir() -> Option<PathBuf> {
    Some(std::env::current_exe().ok()?.parent()?.to_path_buf())
}

/// A shipped game, started by double-clicking: the script next to the executable
/// or in a `game` folder beside it, `main.quill.md` or else the only `.quill.md` file.
fn find_game() -> Option<PathBuf> {
    let exe = exe_dir()?;
    [exe.join("game"), exe].into_iter().find_map(|dir| {
        let main = dir.join("main.quill.md");
        if main.is_file() {
            return Some(main);
        }
        let mut scripts = std::fs::read_dir(&dir)
            .ok()?
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| p.is_file() && p.to_string_lossy().ends_with(".quill.md"));
        match (scripts.next(), scripts.next()) {
            (Some(only), None) => Some(only),
            _ => None,
        }
    })
}

/// Stop with an error. Also written to `error.txt` next to the executable,
/// because a double-clicked game's console window closes before anyone reads it.
fn fatal(message: String) -> ! {
    eprintln!("{message}");
    if let Some(dir) = exe_dir() {
        let _ = std::fs::write(dir.join("error.txt"), format!("{message}\n"));
    }
    exit(1);
}

/// The per-user folder for this game's settings and saves.
/// Windows: %APPDATA%\Quillstone\<id>; macOS: ~/Library/Application Support/Quillstone/<id>;
/// elsewhere: $XDG_DATA_HOME (or ~/.local/share)/quillstone/<id>.
fn data_dir(id: &str) -> Option<PathBuf> {
    let env = |k: &str| std::env::var_os(k).filter(|v| !v.is_empty()).map(PathBuf::from);
    let base = if cfg!(windows) {
        env("APPDATA")?.join("Quillstone")
    } else if cfg!(target_os = "macos") {
        env("HOME")?.join("Library/Application Support/Quillstone")
    } else {
        env("XDG_DATA_HOME").or_else(|| env("HOME").map(|h| h.join(".local/share")))?.join("quillstone")
    };
    Some(base.join(id))
}

/// Write a file so that a crash or power cut midway can't leave it half
/// written: write a temporary file next to it, then rename it into place.
fn write_file(path: &Path, text: &str) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, text)?;
    std::fs::rename(&tmp, path)
}

/// Saves live in the per-user folder, next to the settings.
struct FileStorage {
    dir: Option<PathBuf>,
}

impl Storage for FileStorage {
    fn read(&self, name: &str) -> Option<String> {
        // At most one byte past the limit is read: a huge file is not loaded into memory,
        // and `SaveFile::parse` refuses what is over the limit ("Unreadable save").
        use std::io::Read;
        let file = std::fs::File::open(self.dir.as_ref()?.join(name)).ok()?;
        let mut bytes = Vec::new();
        file.take(MAX_SAVE_BYTES as u64 + 1).read_to_end(&mut bytes).ok()?;
        String::from_utf8(bytes).ok()
    }

    fn write(&mut self, name: &str, contents: &str) -> Result<(), String> {
        let dir = self.dir.as_ref().ok_or("no folder to save in")?;
        write_file(&dir.join(name), contents).map_err(|e| e.to_string())
    }

    fn now(&self) -> Timestamp {
        let now = chrono::Local::now();
        Timestamp { millis: now.timestamp_millis().max(0) as u64, text: now.format("%Y-%m-%d %H:%M").to_string() }
    }
}

fn fullscreen(on: bool) -> Option<Fullscreen> {
    on.then_some(Fullscreen::Borderless(None))
}

impl App {
    /// The script or theme was saved: carry on with the new version (see `reload.rs`).
    fn reload(&mut self) {
        let parsed = std::fs::read_to_string(&self.script_path).map_err(|e| e.to_string()).and_then(|source| {
            quillstone_ui::source::parse_script(&self.script_path, &source).map_err(|errors| {
                for line in quillstone_ui::source::error_lines(&self.script_path, &errors) {
                    eprintln!("{line}");
                }
                // An included file with an error is saved again to fix it: watch it even though no program came out.
                self.watch.add(errors.iter().filter_map(|e| e.file.as_ref()).map(|f| self.root.join(f)));
                errors[0].to_string() // names the file when it is an included one
            })
        });
        let program = match parsed {
            Ok(program) => program,
            Err(e) => return self.game.show_notice(format!("Not reloaded: {e}")),
        };
        // Each version lives as long as the app (a few kilobytes per edit, while developing).
        let program: &'static Program = Box::leak(Box::new(program));
        let theme = load_theme(program, &self.root);
        let (translations, problems) = quillstone_ui::source::load_translations(&self.script_path, program);
        for p in &problems {
            eprintln!("warning: {p}");
        }
        match self.game.reload(program) {
            Ok(()) => {
                self.game.set_theme(theme);
                self.game.set_translations(translations);
                self.game.set_voice_files(voice_files(program, &self.root));
                match problems.first() {
                    Some(first) => self.game.show_notice(format!("Script reloaded; translation not loaded: {first}")),
                    None => self.game.show_notice("Script reloaded"),
                }
            }
            Err(e) => self.game.show_notice(format!("Not reloaded: {e}")),
        }
        self.videos.retry_failed();
        self.images.retry_failed();
        self.watch = reload::Watch::new(&self.script_path, program, &self.root);
    }

    fn play_audio(&mut self) {
        let actions = self.game.take_audio();
        if let Some(audio) = &mut self.audio {
            for action in actions {
                audio.run(action);
            }
            for src in audio.take_failed_loops() {
                self.game.loop_failed(&src);
            }
        }
    }

    /// Carry out whatever the game asked for since the last call.
    fn pump(&mut self, event_loop: &ActiveEventLoop) {
        self.play_audio();
        // The title comes in the language in use (a game with one language never changes it).
        if !self.game.languages().is_empty() {
            let title = self.game.title();
            if title != self.title {
                if let Some(window) = &self.window {
                    window.set_title(&title);
                }
                self.title = title;
            }
        }
        for request in self.game.take_requests() {
            match request {
                Request::Quit => event_loop.exit(),
                Request::Fullscreen(on) => {
                    if let Some(window) = &self.window {
                        window.set_fullscreen(fullscreen(on));
                    }
                }
                Request::SaveSettings => {
                    if let Some(file) = &self.settings_file {
                        if let Err(e) = write_file(file, &self.game.settings().to_text()) {
                            eprintln!("warning: cannot save settings to {}: {e}", file.display());
                            self.game.show_notice(format!("Could not save settings: {e}"));
                        }
                    }
                }
            }
        }
    }

    fn redraw(&mut self) {
        // Nothing to draw into while minimized.
        let minimized = self.window.as_ref().is_some_and(|w| {
            let s = w.inner_size();
            s.width == 0 || s.height == 0
        });
        if minimized {
            self.last_frame = Instant::now(); // don't let the typewriter jump on restore
            self.settled = true; // nothing to show: look again now and then, don't spin
            return;
        }
        if self.gfx.is_none() {
            return;
        }

        let now = Instant::now();
        // While holding for images the game's clock stands still, so a loaded scene starts its fades whole.
        let dt = if self.holding { 0.0 } else { now.duration_since(self.last_frame).as_secs_f32().min(0.1) };
        self.last_frame = now;
        if let Some(audio) = &mut self.audio {
            if audio.update() {
                self.game.music_ended();
            }
            for src in audio.take_failed_loops() {
                self.game.loop_failed(&src);
            }
        }
        self.update_videos(dt);
        let voice = self.audio.as_ref().is_some_and(|a| a.voice_playing());
        self.game.tick(dt, voice);

        let Some(gfx) = self.gfx.as_mut() else { return };
        let (w, h) = gfx.size();
        // Every image the screen asks the size of is one it wants drawn: load what isn't yet.
        let sizes = &self.image_sizes;
        let asked = std::cell::RefCell::new(Vec::new());
        let draws = self.game.draw_list(&|src: &str| {
            asked.borrow_mut().push(src.to_string());
            sizes.get(src).copied()
        });
        self.images.next_frame();
        let asked = asked.into_inner();
        let is_image = |s: &&str| quillstone_script::kind_from_path(s) == AssetKind::Image;
        for src in asked.iter().map(String::as_str).filter(is_image) {
            self.images.want(src, true);
        }
        for src in self.game.upcoming_images().into_iter().filter(is_image) {
            self.images.want(src, false);
        }
        // Wait for the pictures the screen asked for, but not longer than a slow disk may take.
        if asked.iter().any(|s| self.images.is_pending(s)) {
            let since = *self.hold_since.get_or_insert(now);
            self.holding = now.duration_since(since) < HOLD_MAX;
        } else {
            self.hold_since = None;
            self.holding = false;
        }
        if self.holding {
            for src in self.last_asked.iter().map(String::as_str).filter(is_image) {
                self.images.want(src, true); // still on screen: keep them loaded
            }
            gfx.render(&build_frame(&self.last_draws, Size::new(w as f32, h as f32)));
            self.images.update(gfx, &mut self.image_sizes);
            self.settled = false;
            return;
        }
        gfx.render(&build_frame(&draws, Size::new(w as f32, h as f32)));
        self.images.update(gfx, &mut self.image_sizes);
        self.settled = draws == self.last_draws && !self.game.needs_ticks() && !self.images.busy();
        self.last_draws = draws;
        self.last_asked = asked;
    }

    /// Keep the playing clips in line with the story, put new frames on the GPU
    /// (they are drawn like images, under their file name), and tell the game
    /// how its cutscene is doing.
    fn update_videos(&mut self, dt: f32) {
        self.videos.sync(&self.game.wanted_videos());
        if !self.game.media_paused() {
            self.videos.advance(std::time::Duration::from_secs_f32(dt));
        }
        for (src, e) in self.videos.take_errors() {
            eprintln!("warning: cannot play video {src}: {e}");
        }
        let Some(gfx) = self.gfx.as_mut() else { return };
        let playing = self.videos.sources();
        self.uploaded.retain(|src, _| {
            let keep = playing.contains(src);
            if !keep {
                gfx.remove_frame(src);
                self.image_sizes.remove(src);
            }
            keep
        });
        for src in &playing {
            if let Some((picture, id)) = self.videos.frame(src) {
                if self.uploaded.get(src) != Some(&id) {
                    gfx.set_frame(src, picture.width, picture.height, &picture.rgba);
                    self.image_sizes.insert(src.clone(), Size::new(picture.width as f32, picture.height as f32));
                    self.uploaded.insert(src.clone(), id);
                }
            }
        }
        if let Some(cutscene) = self.game.view().video.as_ref().map(|v| v.src.clone()) {
            self.game.video_update(self.videos.playing(&cutscene), self.videos.finished(&cutscene));
        }
    }

    /// A window position in physical pixels, as a point on the virtual screen.
    fn to_virtual(&self, x: f32, y: f32) -> Option<(f32, f32)> {
        let (w, h) = self.gfx.as_ref()?.size();
        to_virtual(x, y, letterbox(Size::new(w as f32, h as f32)))
    }
}

/// The game's name for a key, if it uses it. Held-down repeats only count for
/// keys where repeating makes sense (moving through lists and sliders), and
/// Backspace while the player is typing text.
fn map_key(key: &WinitKey, repeat: bool, typing: bool) -> Option<Key> {
    let k = match key {
        WinitKey::Named(named) => match named {
            NamedKey::Space => Key::Space,
            NamedKey::Enter => Key::Enter,
            NamedKey::ArrowLeft => Key::Left,
            NamedKey::ArrowRight => Key::Right,
            NamedKey::ArrowUp => Key::Up,
            NamedKey::ArrowDown => Key::Down,
            NamedKey::Backspace => Key::Backspace,
            NamedKey::Escape => Key::Escape,
            NamedKey::PageUp => Key::PageUp,
            NamedKey::PageDown => Key::PageDown,
            NamedKey::Home => Key::Home,
            NamedKey::End => Key::End,
            NamedKey::Tab => Key::Tab,
            NamedKey::Control => Key::Ctrl,
            NamedKey::F5 => Key::F5,
            NamedKey::F9 => Key::F9,
            NamedKey::F11 => Key::F11,
            _ => return None,
        },
        WinitKey::Character(s) => Key::Char(s.chars().next()?),
        _ => return None,
    };
    let repeats = matches!(k, Key::Left | Key::Right | Key::Up | Key::Down | Key::PageUp | Key::PageDown)
        || (typing && k == Key::Backspace);
    (!repeat || repeats).then_some(k)
}

impl App {
    /// Switch the window's input method on while the game asks for typed text, off otherwise.
    fn sync_ime(&mut self) {
        let want = self.game.wants_text();
        if want != self.ime_on {
            if let Some(window) = &self.window {
                window.set_ime_allowed(want);
            }
            self.ime_on = want;
        }
    }
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }
        let attributes = Window::default_attributes()
            .with_title(&self.title)
            .with_inner_size(LogicalSize::new(1280.0, 720.0))
            .with_fullscreen(fullscreen(self.game.settings().fullscreen));
        let window = Arc::new(event_loop.create_window(attributes).expect("failed to create window"));

        self.gfx = Some(Gfx::new(event_loop, window.clone(), std::mem::take(&mut self.fonts)));
        self.last_frame = Instant::now();
        window.request_redraw();
        self.window = Some(window);
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        if !matches!(event, WindowEvent::RedrawRequested) {
            self.dirty = true; // input, resize, focus...: draw again
        }
        match event {
            WindowEvent::CloseRequested => {
                self.game.autosave();
                event_loop.exit();
            }
            WindowEvent::Resized(size) => {
                // Minimizing reports 0x0; keep the old surface and just stop drawing
                // (see `redraw`) until a real size comes back.
                if size.width > 0 && size.height > 0 {
                    if let Some(gfx) = &mut self.gfx {
                        gfx.resize(size.width, size.height);
                    }
                }
                if let Some(window) = &self.window {
                    window.request_redraw();
                }
            }
            WindowEvent::CursorMoved { position, .. } => {
                let at = self.to_virtual(position.x as f32, position.y as f32);
                self.game.handle(Event::CursorMoved(at));
            }
            WindowEvent::CursorLeft { .. } => self.game.handle(Event::CursorMoved(None)),
            WindowEvent::MouseInput { state, button, .. } => {
                let button = match button {
                    MouseButton::Left => Button::Left,
                    MouseButton::Right => Button::Right,
                    _ => return,
                };
                match state {
                    ElementState::Pressed if !self.holding => self.game.handle(Event::Press(button)),
                    ElementState::Pressed => {}
                    ElementState::Released => self.game.handle(Event::Release(button)),
                }
            }
            WindowEvent::MouseWheel { delta, .. } => {
                let dy = match delta {
                    MouseScrollDelta::LineDelta(_, y) => y,
                    MouseScrollDelta::PixelDelta(p) => p.y as f32 / 40.0, // touchpads: ~40 px a notch
                };
                if dy != 0.0 && !self.holding {
                    self.game.handle(Event::Wheel(dy));
                }
            }
            WindowEvent::KeyboardInput { event: KeyEvent { logical_key, state, repeat, text, .. }, .. } => {
                let typing = self.game.wants_text();
                if let Some(key) = map_key(&logical_key, repeat, typing) {
                    match state {
                        ElementState::Pressed if !self.holding => self.game.handle(Event::Key(key)),
                        ElementState::Pressed => {} // a line the player hasn't seen must not be skipped
                        ElementState::Released => self.game.handle(Event::KeyReleased(key)),
                    }
                }
                // A text input also gets what the key types (repeats included). The game
                // ignores letter keys there, so the same press is never both a shortcut and text;
                // control characters (Enter, Backspace, Tab) are dropped by the game.
                if let (true, ElementState::Pressed, Some(text)) = (typing, state, text) {
                    if !self.holding && self.game.wants_text() {
                        self.game.handle(Event::Text(text.to_string()));
                    }
                }
            }
            // Text an input method committed (Chinese, Japanese, Korean, dead keys, emoji picker...).
            WindowEvent::Ime(Ime::Commit(text)) => {
                if !self.holding && self.game.wants_text() {
                    self.game.handle(Event::Text(text));
                }
            }
            // A Ctrl held while switching away would never be seen let go.
            WindowEvent::Focused(false) => {
                self.game.set_focused(false);
                self.game.handle(Event::KeyReleased(Key::Ctrl));
            }
            WindowEvent::Focused(true) => self.game.set_focused(true),
            WindowEvent::RedrawRequested => self.redraw(),
            _ => {}
        }
        self.pump(event_loop);
        self.sync_ime();
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        if self.watch.changed() {
            self.reload();
            self.pump(event_loop);
        }
        let pad = self.gamepads.poll();
        if !pad.is_empty() {
            self.dirty = true;
            for event in pad {
                if !self.holding || matches!(event, Event::Release(_) | Event::KeyReleased(_)) {
                    self.game.handle(event);
                }
            }
            self.pump(event_loop);
            self.sync_ime();
        }
        let Some(window) = &self.window else { return };
        let next_anyway = self.last_frame + SETTLED_REDRAW;
        if self.dirty || !self.settled || Instant::now() >= next_anyway {
            // Keep animating (typewriter, fades); the surface's vsync paces this.
            self.dirty = false;
            window.request_redraw();
            event_loop.set_control_flow(ControlFlow::Wait);
        } else {
            // Settled: sleep until an input, the next look, or the next gamepad poll.
            let wake = if self.gamepads.any_connected() { next_anyway.min(Instant::now() + GAMEPAD_POLL) } else { next_anyway };
            event_loop.set_control_flow(ControlFlow::WaitUntil(wake));
        }
    }
}
