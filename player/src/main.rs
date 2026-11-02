//! Quillstone player: opens a window and plays a script.
//!
//! Controls: click / Space / Enter / Right = advance (first press finishes typing),
//! 1-9 or click = choose, Backspace / Left / mouse wheel up = go back,
//! H / Page Up = history, A = auto-forward, S / L = save / load screens,
//! F5 / F9 = quick save / quick load, Tab = skip on/off, hold Ctrl = skip,
//! Esc / right-click = save screen (with tabs to Load, Settings, History), F11 = fullscreen. The game
//! logic lives in `quillstone-ui`; this file only turns window events into its `Event`s and carries out
//! what comes back.

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
use quillstone_ui::settings::{window_placement, Area, FRAME_ROOM};
use quillstone_ui::{Assets, Button, Event, Game, Key, Request, Settings, Storage, Timestamp, MAX_SAVE_BYTES};
use quillstone_video::Videos;
use winit::application::ApplicationHandler;
use winit::dpi::{PhysicalPosition, PhysicalSize};
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
    /// Where the game's files come from (a folder, or an archive with loose files over it).
    assets: Assets,
    /// The script as the game's files name it (`main.quill.md`), and where it is on disk when it is a
    /// folder game (the folder is also what hot reload watches; an archive game is never reloaded).
    script_name: String,
    script_path: PathBuf,
    root: PathBuf,
    watch: reload::Watch,
    /// Where settings are saved; `None` if no per-user folder could be found.
    settings_file: Option<PathBuf>,
    /// The window's size and position while it is a normal window, saved with the settings.
    window_size: Option<(u32, u32)>,
    window_pos: Option<(i32, i32)>,
    window_maximized: bool,
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
    /// The window is minimised or hidden (see `set_minimized`).
    minimized: bool,
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
    // `--unlock-all` (anywhere on the line) shows every extras entry as unlocked, for testing.
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    let unlock_all = args.iter().any(|a| a == "--unlock-all");
    args.retain(|a| a != "--unlock-all");
    // `--dev` turns on the developer console (F12) and allows `--start <label>` / `--set var=value`.
    let dev = args.iter().any(|a| a == "--dev");
    args.retain(|a| a != "--dev");
    let start = quillstone_ui::start_flags(&mut args).unwrap_or_else(|e| fatal(e));
    if !dev && start != quillstone_ui::StartAt::default() {
        fatal("--start and --set need --dev".into());
    }
    let (assets, path) = match args.first().cloned() {
        Some(p) if is_archive(&p) => open_archive(Path::new(&p)),
        Some(p) => open_folder(PathBuf::from(p)),
        None => find_game().unwrap_or_else(|| {
            fatal(
                "usage: quillstone-player <script.quill.md | game.qsa>\n\
                 (or put game.qsa next to the player, or the script in a `game` folder beside it as main.quill.md;\n\
                 loose files in a `mods` folder beside game.qsa replace the ones inside it)"
                    .into(),
            )
        }),
    };
    let script_name = match assets.main_script() {
        Some(main) => main.to_string(),
        None => path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(),
    };
    let source = assets.read_to_string(&script_name).unwrap_or_else(|e| fatal(format!("cannot read {}: {e}", path.display())));
    let program = quillstone_ui::source::parse_script_in(&assets, &script_name, &source)
        .unwrap_or_else(|errors| fatal(quillstone_ui::source::error_lines(&path, &errors).join("\n")));
    // The program lives as long as the app does; leaking it keeps the game simple.
    let program: &'static Program = Box::leak(Box::new(program));
    let root = path.parent().map(PathBuf::from).unwrap_or_default();
    let reloadable = assets.main_script().is_none();

    let theme = load_theme(program, &assets);
    let fonts = load_fonts(&theme, &assets);

    let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("game").trim_end_matches(".quill");
    let dir = data_dir(&save_folder(program, stem));
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
    let audio = Audio::new(&assets, sfx);

    let game_settings_window = (settings.window_size, settings.window_pos, settings.window_maximized);
    let mut game = Game::new(program, settings, Box::new(FileStorage { dir }));
    game.set_voice_files(quillstone_ui::source::voice_files(&assets, program).into_iter().collect());
    game.set_unlock_all(unlock_all);
    game.set_dev(dev);
    game.set_theme(theme);
    let (translations, problems) = quillstone_ui::source::load_translations_in(&assets, program);
    for p in &problems {
        eprintln!("warning: {p}");
    }
    game.set_translations(translations);
    if let Some(first) = problems.first() {
        game.notify(format!("Translation not loaded: {first}"));
    }
    // `random(lo, hi)` rolls start from the clock, so every new game rolls differently.
    game.set_seed(std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos() as u64));
    if start != quillstone_ui::StartAt::default() {
        game.start_dev(&start).unwrap_or_else(|e| fatal(e));
    }
    let mut app = App {
        title: game.title(),
        game,
        gamepads: gamepad::Gamepads::new(),
        watch: if reloadable { reload::Watch::new(&path, program, &root) } else { reload::Watch::none() },
        assets: assets.clone(),
        script_name,
        script_path: path.clone(),
        root: root.clone(),
        window_size: game_settings_window.0,
        window_pos: game_settings_window.1,
        window_maximized: game_settings_window.2,
        settings_file,
        audio,
        images: images::Images::new(assets.clone()),
        image_sizes: HashMap::new(),
        fonts,
        videos: Videos::new(video::opener(&assets)),
        uploaded: HashMap::new(),
        window: None,
        gfx: None,
        last_frame: Instant::now(),
        minimized: false,
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

/// The per-user folder name: the script's `id:`, else the title, else the script's file name.
fn save_folder(program: &Program, stem: &str) -> String {
    program.id.clone().unwrap_or_else(|| game_id(program.title.as_deref().unwrap_or(stem)))
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

/// Whether an argument names a game archive; Windows users type `GAME.QSA` too.
fn is_archive(arg: &str) -> bool {
    Path::new(arg).extension().is_some_and(|e| e.eq_ignore_ascii_case("qsa"))
}

/// A game archive: its files come from it, except those that also sit loose in a `mods` folder beside it
/// (not `game`: that is what a folder release ships, and an old one left beside a new archive would win).
/// The script is the archive's main one (or the loose copy of it).
fn open_archive(qsa: &Path) -> (Assets, PathBuf) {
    let archive = quillstone_ui::archive::Archive::open(qsa).unwrap_or_else(|e| fatal(e));
    let script = PathBuf::from(archive.main());
    let overlay = qsa.parent().map(|d| d.join("mods")).filter(|d| d.is_dir());
    (Assets::archive(archive, overlay), script)
}

/// A game in a folder: the script's own folder holds its files.
fn open_folder(script: PathBuf) -> (Assets, PathBuf) {
    (Assets::folder(quillstone_ui::source::script_dir(&script)), script)
}

/// A shipped game, started by double-clicking: `game.qsa` beside the executable, else the script next to
/// the executable or in a `game` folder beside it, `main.quill.md` or else the only `.quill.md` file.
fn find_game() -> Option<(Assets, PathBuf)> {
    let exe = exe_dir()?;
    if exe.join("game.qsa").is_file() {
        return Some(open_archive(&exe.join("game.qsa")));
    }
    [exe.join("game"), exe].into_iter().find_map(|dir| {
        let main = dir.join("main.quill.md");
        if main.is_file() {
            return Some(open_folder(main));
        }
        let mut scripts = std::fs::read_dir(&dir)
            .ok()?
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| p.is_file() && p.to_string_lossy().ends_with(".quill.md"));
        match (scripts.next(), scripts.next()) {
            (Some(only), None) => Some(open_folder(only)),
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

/// Saves live in the per-user folder, next to the settings.
struct FileStorage {
    dir: Option<PathBuf>,
}

impl Storage for FileStorage {
    fn load(&self, name: &str) -> Result<Option<String>, String> {
        // At most one byte past the limit is read: a huge file is not loaded into memory, and it is an
        // error, not a cut-short text (the caller would then write that text back over the file).
        use std::io::Read;
        let Some(dir) = self.dir.as_ref() else { return Ok(None) };
        let file = match std::fs::File::open(dir.join(name)) {
            Ok(file) => file,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(e.to_string()),
        };
        let mut bytes = Vec::new();
        file.take(MAX_SAVE_BYTES as u64 + 1).read_to_end(&mut bytes).map_err(|e| e.to_string())?;
        if bytes.len() > MAX_SAVE_BYTES {
            return Err(format!("{name} is larger than {} MB", MAX_SAVE_BYTES / (1024 * 1024)));
        }
        String::from_utf8(bytes).map(Some).map_err(|e| e.to_string())
    }

    fn write(&mut self, name: &str, contents: &str) -> Result<(), String> {
        let dir = self.dir.as_ref().ok_or("no folder to save in")?;
        quillstone_ui::write_atomic(&dir.join(name), contents).map_err(|e| e.to_string())
    }

    fn now(&self) -> Timestamp {
        let now = chrono::Local::now();
        Timestamp { millis: now.timestamp_millis().max(0) as u64, text: now.format("%Y-%m-%d %H:%M").to_string() }
    }
}

/// Whether a pad event reaches the game. While `blocked` (the window is minimised or a mouse drag is held)
/// only releases do, so a held skip still ends.
fn pad_event_passes(blocked: bool, event: &Event) -> bool {
    !blocked || matches!(event, Event::Release(_) | Event::KeyReleased(_))
}

fn fullscreen(on: bool) -> Option<Fullscreen> {
    on.then_some(Fullscreen::Borderless(None))
}

impl App {
    /// The script or theme was saved: carry on with the new version (see `reload.rs`).
    fn reload(&mut self) {
        let parsed = self.assets.read_to_string(&self.script_name).map_err(|e| e.to_string()).and_then(|source| {
            quillstone_ui::source::parse_script_in(&self.assets, &self.script_name, &source).map_err(|errors| {
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
            Err(e) => return self.game.notify(format!("Not reloaded: {e}")),
        };
        // Each version lives as long as the app (a few kilobytes per edit, while developing).
        let program: &'static Program = Box::leak(Box::new(program));
        let theme = load_theme(program, &self.assets);
        let (translations, problems) = quillstone_ui::source::load_translations_in(&self.assets, program);
        for p in &problems {
            eprintln!("warning: {p}");
        }
        match self.game.reload(program) {
            Ok(()) => {
                self.game.set_theme(theme);
                self.game.set_translations(translations);
                self.game.set_voice_files(quillstone_ui::source::voice_files(&self.assets, program).into_iter().collect());
                match problems.first() {
                    Some(first) => self.game.notify(format!("Script reloaded; translation not loaded: {first}")),
                    None => self.game.notify("Script reloaded"),
                }
            }
            Err(e) => self.game.notify(format!("Not reloaded: {e}")),
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
                Request::Quit => {
                    self.save_settings();
                    event_loop.exit();
                }
                Request::Fullscreen(on) => {
                    if let Some(window) = &self.window {
                        window.set_fullscreen(fullscreen(on));
                    }
                }
                Request::SaveSettings => self.save_settings(),
            }
        }
    }

    /// Write the settings file, with the window's size and position.
    fn save_settings(&mut self) {
        let Some(file) = &self.settings_file else { return };
        let mut settings = self.game.settings().clone();
        (settings.window_size, settings.window_pos) = (self.window_size, self.window_pos);
        settings.window_maximized = self.window_maximized;
        if let Err(e) = quillstone_ui::write_atomic(file, &settings.to_text()) {
            eprintln!("warning: cannot save settings to {}: {e}", file.display());
            self.game.notify(format!("Could not save settings: {e}"));
        }
    }

    /// Remember the window's size and position, unless it is full screen, maximised or minimised (their values are no use:
    /// a maximised window would come back as a huge normal one). Maximised is remembered as a flag instead.
    fn note_window(&mut self) {
        let Some(w) = &self.window else { return };
        if w.fullscreen().is_some() || w.is_minimized() == Some(true) {
            return;
        }
        self.window_maximized = w.is_maximized();
        if self.window_maximized {
            return;
        }
        let size = w.inner_size();
        if size.width > 0 && size.height > 0 {
            self.window_size = Some((size.width, size.height));
        }
        if let Ok(p) = w.outer_position() {
            self.window_pos = Some((p.x, p.y));
        }
    }

    /// The window was minimised or restored: the game and the sound stand still, and the clock
    /// starts over on return so the first frame after it doesn't jump.
    fn set_minimized(&mut self, minimized: bool) {
        if self.minimized == minimized {
            return;
        }
        self.minimized = minimized;
        self.game.set_minimized(minimized);
        if let Some(audio) = &mut self.audio {
            audio.set_paused(minimized);
            audio.set_video_paused(self.game.media_paused());
        }
        self.last_frame = Instant::now();
        self.settled = minimized;
    }

    fn redraw(&mut self) {
        // Nothing to draw into while minimized.
        let size_zero = self.window.as_ref().is_some_and(|w| {
            let s = w.inner_size();
            s.width == 0 || s.height == 0
        });
        if size_zero {
            self.set_minimized(true);
        }
        if self.minimized {
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
            if audio.take_voice_ended() {
                self.game.voice_ended();
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
        // Opening a clip blocks for a moment: that pause is not game time, or the next frame's
        // `dt` would skip the clip's first frames and jump every fade and the typewriter. Only the
        // `sync` is taken out; the audio update before it and the rendering after it are real time.
        let synced = Instant::now();
        self.videos.sync(&self.game.wanted_videos());
        self.last_frame += synced.elapsed();
        if !self.game.media_paused() {
            self.videos.advance(std::time::Duration::from_secs_f32(dt));
        }
        if let Some(audio) = &mut self.audio {
            audio.set_video_sound(self.videos.sound());
            audio.set_video_paused(self.game.media_paused());
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
/// Backspace while the player is typing text. With `fresh` (a timing bar or key prompt is up) nothing
/// repeats: a held key carried onto it must not press it.
fn map_key(key: &WinitKey, repeat: bool, typing: bool, fresh: bool) -> Option<Key> {
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
            NamedKey::F12 => Key::F12,
            _ => return None,
        },
        WinitKey::Character(s) => Key::Char(s.chars().next()?),
        _ => return None,
    };
    let repeats = (!fresh && matches!(k, Key::Left | Key::Right | Key::Up | Key::Down | Key::PageUp | Key::PageDown))
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
        let monitors: Vec<Area> = event_loop
            .primary_monitor()
            .into_iter()
            .chain(event_loop.available_monitors())
            .map(|m| Area {
                x: m.position().x,
                y: m.position().y,
                w: m.size().width,
                h: m.size().height,
                room: (FRAME_ROOM as f64 * m.scale_factor()).round() as u32,
            })
            .collect();
        // (the primary monitor is listed twice if it is also in the list: harmless)
        let (size, pos) = window_placement(self.window_size, self.window_pos, &monitors);
        let attributes = Window::default_attributes()
            .with_title(&self.title)
            .with_inner_size(PhysicalSize::new(size.0, size.1))
            .with_position(PhysicalPosition::new(pos.0, pos.1))
            .with_maximized(self.window_maximized)
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
                self.save_settings();
                event_loop.exit();
            }
            // macOS and others tell when the window is hidden; Windows only reports a 0x0 size.
            WindowEvent::Occluded(hidden) => self.set_minimized(hidden),
            WindowEvent::Moved(_) => self.note_window(),
            WindowEvent::Resized(size) => {
                // Minimizing reports 0x0; keep the old surface, pause the game and sound and stop
                // drawing (see `redraw`) until a real size comes back.
                self.set_minimized(size.width == 0 || size.height == 0);
                if size.width > 0 && size.height > 0 {
                    self.note_window();
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
                if let Some(key) = map_key(&logical_key, repeat, typing, self.game.wants_fresh_keys()) {
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
                if pad_event_passes(self.holding || self.minimized, &event) {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_archive_extension_is_matched_in_any_case() {
        assert!(is_archive("game.qsa") && is_archive("C:\\g\\GAME.QSA") && is_archive("a.Qsa"));
        assert!(!is_archive("game.qsa.txt") && !is_archive("qsa") && !is_archive("main.quill.md"));
    }

    #[test]
    fn the_save_folder_follows_the_id_and_falls_back_to_the_title_then_the_file_name() {
        let parse = |front: &str| quillstone_script::parse(&format!("---\n{front}\n---\n# a\nHi.\n")).unwrap();
        assert_eq!(save_folder(&parse("title: Night Shift\nid: night-shift"), "main"), "night-shift");
        assert_eq!(save_folder(&parse("title: Night Shift!\nid: night-shift"), "main"), "night-shift");
        assert_eq!(save_folder(&parse("title: Night Shift!"), "main"), "night-shift");
        assert_eq!(save_folder(&parse("vars: { a: 1 }"), "main"), "main");
    }

    #[test]
    fn a_file_over_the_save_limit_is_an_error_not_a_cut_short_text() {
        let dir = std::env::temp_dir().join(format!("qs-storage-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("ok.txt"), "small").unwrap();
        std::fs::write(dir.join("big.txt"), vec![b'x'; MAX_SAVE_BYTES + 1]).unwrap();
        let storage = FileStorage { dir: Some(dir.clone()) };
        assert_eq!(storage.load("ok.txt"), Ok(Some("small".to_string())));
        assert!(storage.load("big.txt").is_err());
        assert_eq!(storage.load("none.txt"), Ok(None));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn only_a_mods_folder_overrides_an_archive_not_an_old_game_folder() {
        let dir = std::env::temp_dir().join(format!("qs-overlay-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        for (folder, text) in [("src", "packed"), ("game", "old release"), ("mods", "patched")] {
            std::fs::create_dir_all(dir.join(folder)).unwrap();
            std::fs::write(dir.join(folder).join("a.txt"), text).unwrap();
        }
        std::fs::write(dir.join("src/m.quill.md"), "# a\nHi.\n").unwrap();
        let files = [("m.quill.md".to_string(), dir.join("src/m.quill.md")), ("a.txt".to_string(), dir.join("src/a.txt"))];
        quillstone_ui::archive::write(&dir.join("game.qsa"), "m.quill.md", &files).unwrap();
        let (assets, _) = open_archive(&dir.join("game.qsa"));
        assert_eq!(assets.read_to_string("a.txt").unwrap(), "patched");
        std::fs::remove_dir_all(dir.join("mods")).unwrap();
        let (assets, _) = open_archive(&dir.join("game.qsa"));
        assert_eq!(assets.read_to_string("a.txt").unwrap(), "packed", "a game/ folder no longer overrides");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_held_arrow_repeats_except_where_a_press_has_to_be_fresh() {
        let right = WinitKey::Named(NamedKey::ArrowRight);
        assert_eq!(map_key(&right, false, false, true), Some(Key::Right), "the first press always counts");
        assert_eq!(map_key(&right, true, false, false), Some(Key::Right), "lists and sliders repeat");
        assert_eq!(map_key(&right, true, false, true), None, "a held key does not press a bar or key prompt");
    }

    #[test]
    fn a_pad_does_nothing_while_the_window_is_minimised_except_let_go() {
        use quillstone_ui::Key;
        assert!(pad_event_passes(false, &Event::Key(Key::Enter)));
        assert!(!pad_event_passes(true, &Event::Key(Key::Enter)));
        assert!(pad_event_passes(true, &Event::KeyReleased(Key::Ctrl)));
    }
}
