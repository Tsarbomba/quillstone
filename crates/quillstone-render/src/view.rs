//! What is on screen right now, and the backend-neutral list of things to draw.
//!
//! Feed it the runtime's [`Cmd`]s with [`View::apply`], call [`View::tick`] every
//! frame, and render [`View::draw_list`]. No GPU, window or fonts involved.
//!
//! The view owns only *presentation*: fade progress, sprite slides, the typewriter.
//! What is actually in the scene lives in the runtime's `State`, and a rollback
//! sends `Cmd::ClearScene` + the scene again, so the two never drift apart.

use quillstone_runtime::{Camera, Cmd, Spot};
use quillstone_script::markup::{rich, Rich};
pub use quillstone_script::markup::TextRun;
use quillstone_script::{Ease, Fit, Wipe};

use crate::layout::{clip, is_wide, place, Rect, Size, VIRTUAL};
use crate::theme::Theme;

/// The dialogue box, in virtual coordinates.
pub const TEXTBOX: Rect = Rect::new(160.0, 780.0, 1600.0, 260.0);

/// Height of a sprite at `scale = 1`, in virtual pixels. Sprites stand on the
/// bottom edge of the screen (the textbox draws over their lower part).
pub const SPRITE_HEIGHT: f32 = 940.0;

/// Thickness of the outline around a hovered hotspot.
const HOTSPOT_OUTLINE: f32 = 4.0;
/// Hotspot tooltip: padding around the label, and the gap kept to the screen edge.
const TIP_PAD: f32 = 20.0;
const TIP_MARGIN: f32 = 8.0;
const CHOICE_W: f32 = 1000.0;
const CHOICE_H: f32 = 96.0;
const CHOICE_GAP: f32 = 28.0;
/// Space kept free above the first and below the last choice button.
const CHOICE_MARGIN: f32 = 24.0;


/// Typewriter speed unless changed with [`View::with_speed`].
pub const DEFAULT_CHARS_PER_SEC: f32 = 60.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Align {
    Left,
    Center,
}

/// One thing for a backend to draw. Coordinates are virtual (1920x1080);
/// draw in list order, back to front.
#[derive(Debug, Clone, PartialEq)]
pub enum Draw {
    Image { src: String, dest: Rect, uv: Rect, alpha: f32 },
    Rect { rect: Rect, color: [f32; 4] },
    /// `text` is the *full* string so wrapping stays stable; show only the first
    /// `visible_chars` characters (the typewriter effect) without re-wrapping.
    /// `runs`: bold / italic / coloured stretches (empty: all in `color`).
    Text {
        text: String,
        runs: Vec<TextRun>,
        visible_chars: usize,
        rect: Rect,
        size: f32,
        color: [f32; 4],
        align: Align,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub struct Background {
    pub src: String,
    /// Transition duration in seconds (0 = instant).
    pub fade: f32,
    /// Reveal the picture from one side instead of fading it in.
    pub wipe: Option<Wipe>,
    pub elapsed: f32,
}

impl Background {
    /// How far the transition is, 0..1.
    pub fn progress(&self) -> f32 {
        if self.fade <= 0.0 {
            1.0
        } else {
            (self.elapsed / self.fade).clamp(0.0, 1.0)
        }
    }

    /// Opacity: rises with the transition, except for a wipe (which is opaque).
    pub fn alpha(&self) -> f32 {
        if self.wipe.is_some() {
            1.0
        } else {
            self.progress()
        }
    }

    /// The part of the screen showing this picture during a wipe; `None` for all of it.
    pub fn revealed(&self) -> Option<Rect> {
        let wipe = self.wipe?;
        let p = self.progress();
        if p >= 1.0 {
            return None;
        }
        let (w, h) = (VIRTUAL.w, VIRTUAL.h);
        Some(match wipe {
            Wipe::Right => Rect::new(0.0, 0.0, w * p, h),
            Wipe::Left => Rect::new(w * (1.0 - p), 0.0, w * p, h),
            Wipe::Down => Rect::new(0.0, 0.0, w, h * p),
            Wipe::Up => Rect::new(0.0, h * (1.0 - p), w, h * p),
        })
    }
}

/// A sprite on stage, with its fade and slide animations.
#[derive(Debug, Clone, PartialEq)]
pub struct SpriteView {
    pub id: String,
    pub src: String,
    /// The outgoing pose while a pose change crossfades.
    pub previous_src: Option<String>,
    /// Target position, 0..1 across the screen (the sprite's centre).
    pub x: f32,
    /// Target height of the bottom edge, as a fraction of the screen height.
    pub y: f32,
    /// Drawing order among sprites (higher is in front).
    pub z: i32,
    /// Target size.
    pub scale: f32,
    pub hiding: bool,
    fade: f32,
    elapsed: f32,
    from_x: f32,
    from_y: f32,
    from_scale: f32,
    move_time: f32,
    move_elapsed: f32,
    move_ease: Ease,
}

impl SpriteView {
    fn new(id: &str, src: &str, x: f32, y: f32, z: i32, scale: f32, fade: f32) -> SpriteView {
        SpriteView {
            id: id.to_string(),
            src: src.to_string(),
            previous_src: None,
            x,
            y,
            z,
            scale,
            hiding: false,
            fade,
            elapsed: 0.0,
            from_x: x,
            from_y: y,
            from_scale: scale,
            move_time: 0.0,
            move_elapsed: 0.0,
            move_ease: Ease::Linear,
        }
    }

    /// Progress through the current move, eased (1 when not moving).
    fn move_progress(&self) -> f32 {
        if self.move_time > 0.0 {
            self.move_ease.apply(self.move_elapsed / self.move_time)
        } else {
            1.0
        }
    }

    /// How big the sprite is right now (growing towards `scale` during a move).
    pub fn current_scale(&self) -> f32 {
        self.from_scale + (self.scale - self.from_scale) * self.move_progress()
    }

    /// Fade progress as opacity: rising when showing, falling when hiding.
    pub fn alpha(&self) -> f32 {
        let t = if self.fade <= 0.0 { 1.0 } else { (self.elapsed / self.fade).clamp(0.0, 1.0) };
        if self.hiding {
            1.0 - t
        } else {
            t
        }
    }

    /// Where the sprite is right now (sliding towards `x` while a move runs).
    pub fn current_x(&self) -> f32 {
        self.from_x + (self.x - self.from_x) * self.move_progress()
    }

    /// Where the bottom edge is right now (sliding towards `y` while a move runs).
    pub fn current_y(&self) -> f32 {
        self.from_y + (self.y - self.from_y) * self.move_progress()
    }

    fn tick(&mut self, dt: f32) {
        self.elapsed += dt;
        if self.move_time > 0.0 {
            self.move_elapsed += dt;
            if self.move_elapsed >= self.move_time {
                self.move_time = 0.0;
                self.from_x = self.x;
                self.from_y = self.y;
                self.from_scale = self.scale;
            }
        }
        if !self.hiding && self.elapsed >= self.fade {
            self.previous_src = None;
        }
    }

    fn is_gone(&self) -> bool {
        self.hiding && self.alpha() <= 0.0
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct InlineImage {
    pub src: String,
    pub alt: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct VideoView {
    pub src: String,
    pub fit: Fit,
    pub looped: bool,
    pub skippable: bool,
}

/// A line being typed out.
#[derive(Debug, Clone, PartialEq)]
pub struct Dialogue {
    pub speaker: Option<String>,
    /// Name plate colour (0..1 RGB), if the speaker's character declared one.
    pub color: Option<[f32; 3]>,
    /// The words shown (text tags taken out; see `quillstone_script::markup`).
    pub text: String,
    pub runs: Vec<TextRun>,
    /// `{w=...}` pauses still ahead: (characters shown when it pauses, seconds).
    pauses: Vec<(usize, f32)>,
    /// Seconds left of the pause the typewriter is in.
    hold: f32,
    total: usize,
    revealed: f32,
    /// The line ends in `{nw}`: the story goes on by itself once it is typed.
    pub auto_advance: bool,
}

impl Dialogue {
    fn new(speaker: Option<String>, color: Option<[f32; 3]>, tagged: &str, instant: bool) -> Dialogue {
        let Rich { text, runs, pauses, no_wait } = rich(tagged);
        let total = text.chars().count();
        let revealed = if instant { total as f32 } else { 0.0 };
        Dialogue { speaker, color, text, runs, pauses, hold: 0.0, total, revealed, auto_advance: no_wait }
    }

    /// Type on for `dt` seconds at `speed` characters a second, stopping at pauses.
    fn type_on(&mut self, mut dt: f32, speed: f32) {
        while dt > 0.0 && !self.is_complete() {
            if self.hold > 0.0 {
                let held = self.hold.min(dt);
                self.hold -= held;
                dt -= held;
                continue;
            }
            let target = self.revealed + dt * speed;
            match self.pauses.first().copied() {
                Some((at, seconds)) if at as f32 <= target => {
                    dt -= (at as f32 - self.revealed).max(0.0) / speed;
                    self.revealed = self.revealed.max(at as f32);
                    self.hold = seconds;
                    self.pauses.remove(0);
                }
                _ => {
                    self.revealed = target.min(self.total as f32);
                    dt = 0.0;
                }
            }
        }
    }

    /// How many characters (not bytes) are visible.
    pub fn visible_chars(&self) -> usize {
        (self.revealed.floor() as usize).min(self.total)
    }

    pub fn visible_text(&self) -> &str {
        let n = self.visible_chars();
        if n >= self.total {
            return &self.text;
        }
        let end = self.text.char_indices().nth(n).map(|(i, _)| i).unwrap_or(self.text.len());
        &self.text[..end]
    }

    pub fn is_complete(&self) -> bool {
        self.visible_chars() >= self.total
    }

    pub fn reveal_all(&mut self) {
        self.revealed = self.total as f32;
        self.pauses.clear();
        self.hold = 0.0;
    }
}

/// A camera move in progress (or finished: then `elapsed >= time`).
#[derive(Debug, Clone, PartialEq)]
struct CameraMove {
    from: Camera,
    to: Camera,
    time: f32,
    elapsed: f32,
    ease: Ease,
}

impl CameraMove {
    fn current(&self) -> Camera {
        let t = if self.time > 0.0 { self.ease.apply(self.elapsed / self.time) } else { 1.0 };
        let lerp = |a: f32, b: f32| a + (b - a) * t;
        Camera { zoom: lerp(self.from.zoom, self.to.zoom), x: lerp(self.from.x, self.to.x), y: lerp(self.from.y, self.to.y) }
    }
}

#[derive(Debug, Clone, PartialEq)]
struct Shake {
    time: f32,
    strength: f32,
    elapsed: f32,
}

impl Shake {
    /// The stage's offset now: a fast wobble that dies away (deterministic).
    fn offset(&self) -> (f32, f32) {
        let t = self.elapsed;
        if t >= self.time {
            return (0.0, 0.0);
        }
        let fade = self.strength * (1.0 - t / self.time);
        let tau = std::f32::consts::TAU;
        (fade * (t * tau * 13.0).sin(), fade * 0.6 * (t * tau * 17.0 + 1.3).sin())
    }
}

#[derive(Debug, Clone)]
pub struct View {
    pub background: Option<Background>,
    /// The outgoing background while a crossfade is running.
    pub previous_background: Option<String>,
    pub sprites: Vec<SpriteView>,
    pub images: Vec<InlineImage>,
    pub dialogue: Option<Dialogue>,
    pub choices: Vec<String>,
    /// Clickable regions over the background (`::hotspots`), in the order the runtime lists them.
    pub hotspots: Vec<Spot>,
    pub video: Option<VideoView>,
    pub finished: bool,
    camera: CameraMove,
    shake: Option<Shake>,
    /// Seconds left of a `::wait`, if one is running.
    wait: Option<f32>,
    chars_per_sec: f32,
    theme: Theme,
}

impl Default for View {
    fn default() -> Self {
        View::new()
    }
}

impl View {
    pub fn new() -> View {
        View {
            background: None,
            previous_background: None,
            sprites: Vec::new(),
            images: Vec::new(),
            dialogue: None,
            choices: Vec::new(),
            hotspots: Vec::new(),
            video: None,
            finished: false,
            camera: CameraMove { from: Camera::default(), to: Camera::default(), time: 0.0, elapsed: 0.0, ease: Ease::Linear },
            shake: None,
            wait: None,
            chars_per_sec: DEFAULT_CHARS_PER_SEC,
            theme: Theme::default(),
        }
    }

    /// Draw with this theme's colours, sizes and textbox image.
    pub fn with_theme(mut self, theme: Theme) -> View {
        self.theme = theme;
        self
    }

    pub fn set_theme(&mut self, theme: Theme) {
        self.theme = theme;
    }

    pub fn theme(&self) -> &Theme {
        &self.theme
    }

    /// Set the typewriter speed. `0` (or less) shows each line instantly.
    pub fn with_speed(mut self, chars_per_sec: f32) -> View {
        self.chars_per_sec = chars_per_sec;
        self
    }

    /// Change the typewriter speed, e.g. from the settings screen. A line that is
    /// typing carries on at the new speed (or completes at once for `0`).
    pub fn set_speed(&mut self, chars_per_sec: f32) {
        self.chars_per_sec = chars_per_sec;
    }

    /// Apply one batch of commands from the runtime.
    pub fn apply(&mut self, cmds: &[Cmd]) {
        // A new batch means the player acted: per-line extras go away. (A redraw
        // after rollback re-sends them, since the runtime tracks them.)
        self.images.clear();
        self.choices.clear();
        self.hotspots.clear();
        self.video = None;
        self.wait = None;
        // Only the latest batch can say the story is over (matters after rollback).
        self.finished = false;
        // A redraw (rollback, later: loading a save) returns to a line the player
        // has already read, so it is shown whole instead of typed out again.
        let redraw = cmds.first() == Some(&Cmd::ClearScene);

        for cmd in cmds {
            match cmd {
                Cmd::ClearScene => {
                    self.background = None;
                    self.previous_background = None;
                    self.sprites.clear();
                    self.set_camera(Camera::default(), 0.0, Ease::Linear);
                    self.shake = None;
                }
                Cmd::SetBackground { src, fade, wipe } => self.set_background(src, *fade, *wipe),
                Cmd::ShowSprite { id, src, x, y, z, scale, fade } => self.show_sprite(id, src, (*x, *y), *z, *scale, *fade),
                Cmd::HideSprite { id, fade } => self.hide_sprite(id, *fade),
                Cmd::MoveSprite { id, x, y, scale, time, ease } => {
                    if let Some(s) = self.sprites.iter_mut().find(|s| s.id == *id && !s.hiding) {
                        s.from_x = s.current_x();
                        s.from_y = s.current_y();
                        s.y = *y;
                        s.from_scale = s.current_scale();
                        s.x = *x;
                        s.scale = *scale;
                        s.move_time = *time;
                        s.move_elapsed = 0.0;
                        s.move_ease = *ease;
                    }
                }
                Cmd::Shake { time, strength } => {
                    self.shake = Some(Shake { time: *time, strength: *strength, elapsed: 0.0 });
                }
                Cmd::Camera { camera, time, ease } => self.set_camera(*camera, *time, *ease),
                Cmd::ShowImage { src, alt } => {
                    self.images.push(InlineImage { src: src.clone(), alt: alt.clone() });
                }
                Cmd::Say { speaker, color, text } => {
                    self.dialogue = Some(Dialogue::new(
                        speaker.clone(),
                        *color,
                        text,
                        redraw || self.chars_per_sec <= 0.0,
                    ));
                }
                Cmd::ShowChoices { options } => self.choices = options.clone(),
                Cmd::ShowHotspots { spots } => self.hotspots = spots.clone(),
                Cmd::Wait { seconds } => self.wait = Some(*seconds),
                Cmd::PlayVideo { src, fit, looped, skippable } => {
                    self.dialogue = None;
                    self.video = Some(VideoView {
                        src: src.clone(),
                        fit: *fit,
                        looped: *looped,
                        skippable: *skippable,
                    });
                }
                Cmd::Finished => self.finished = true,
                Cmd::Stopped { .. } => {} // for the author; the game shows it as a notice
                // The inventory and shop are screens of their own (see quillstone-ui).
                Cmd::ShowInventory | Cmd::ShowShop { .. } => {}
                // Audio is not the view's business.
                Cmd::PlayMusic { .. } | Cmd::StopMusic { .. } | Cmd::PlaySfx { .. } | Cmd::PlayVoice { .. } => {}
            }
        }
    }

    fn set_background(&mut self, src: &str, fade: f32, wipe: Option<Wipe>) {
        if self.background.as_ref().is_some_and(|b| b.src == src) {
            return;
        }
        self.previous_background = if fade > 0.0 { self.background.take().map(|b| b.src) } else { None };
        self.background = Some(Background { src: src.to_string(), fade, wipe, elapsed: 0.0 });
    }

    /// Start moving the camera from wherever it is now.
    fn set_camera(&mut self, to: Camera, time: f32, ease: Ease) {
        let from = self.camera.current();
        self.camera = CameraMove { from, to, time, elapsed: 0.0, ease };
    }

    /// Where the camera is right now.
    pub fn camera(&self) -> Camera {
        self.camera.current()
    }

    fn show_sprite(&mut self, id: &str, src: &str, (x, y): (f32, f32), z: i32, scale: f32, fade: f32) {
        // A sprite that is mid-hide and shown again starts over.
        self.sprites.retain(|s| s.id != id || !s.hiding);
        match self.sprites.iter_mut().find(|s| s.id == id) {
            Some(s) => {
                let (here, here_y) = (s.current_x(), s.current_y());
                if s.src != src {
                    let old = std::mem::replace(&mut s.src, src.to_string());
                    s.previous_src = if fade > 0.0 { Some(old) } else { None };
                    s.elapsed = 0.0;
                    s.fade = fade;
                }
                s.from_x = here;
                s.from_y = here_y;
                s.x = x;
                s.y = y;
                s.z = z;
                s.move_time = 0.0;
                s.scale = scale;
                s.from_scale = scale;
            }
            None => self.sprites.push(SpriteView::new(id, src, x, y, z, scale, fade)),
        }
    }

    fn hide_sprite(&mut self, id: &str, fade: f32) {
        if fade <= 0.0 {
            self.sprites.retain(|s| s.id != id);
        } else if let Some(s) = self.sprites.iter_mut().find(|s| s.id == id && !s.hiding) {
            s.hiding = true;
            s.fade = fade;
            s.elapsed = 0.0;
            s.previous_src = None;
        }
    }

    /// Advance animations by `dt` seconds.
    pub fn tick(&mut self, dt: f32) {
        if let Some(bg) = &mut self.background {
            bg.elapsed += dt;
            if bg.progress() >= 1.0 {
                self.previous_background = None;
            }
        }
        for s in &mut self.sprites {
            s.tick(dt);
        }
        self.camera.elapsed += dt;
        if let Some(shake) = &mut self.shake {
            shake.elapsed += dt;
            if shake.elapsed >= shake.time {
                self.shake = None;
            }
        }
        self.sprites.retain(|s| !s.is_gone());
        if let Some(left) = &mut self.wait {
            *left -= dt;
        }
        if let Some(d) = &mut self.dialogue {
            if self.chars_per_sec > 0.0 {
                d.type_on(dt, self.chars_per_sec);
            } else {
                d.reveal_all();
            }
        }
    }

    /// Whether time matters even when nothing on screen moves: a `::wait`
    /// counting down, or a typewriter that is still typing (or paused at `{w=}`).
    /// Visible animations show up as a changing draw list anyway.
    pub fn busy(&self) -> bool {
        self.wait.is_some() || self.dialogue.as_ref().is_some_and(|d| !d.is_complete())
    }

    /// `true` exactly once when a `::wait` has run out: send `Input::Advance`.
    pub fn take_wait_finished(&mut self) -> bool {
        if self.wait.is_some_and(|left| left <= 0.0) {
            self.wait = None;
            true
        } else {
            false
        }
    }

    /// The player pressed "advance" (click, space, enter).
    ///
    /// Returns `true` if the session should now step. Returns `false` when the
    /// press only finished typing the line, or while choices are on screen.
    /// During a `::wait` it always returns `true`: the press skips the wait.
    pub fn press_advance(&mut self) -> bool {
        if self.wait.is_some() {
            return true;
        }
        if !self.choices.is_empty() || !self.hotspots.is_empty() {
            return false;
        }
        if let Some(d) = &mut self.dialogue {
            if !d.is_complete() {
                d.reveal_all();
                return false;
            }
        }
        true
    }

    /// Which choice button (if any) contains this virtual-space point.
    pub fn choice_at(&self, x: f32, y: f32) -> Option<usize> {
        choice_rects(self.choices.len()).iter().position(|r| r.contains(x, y))
    }

    /// Which hotspot (if any) contains this virtual-space point. Later ones are
    /// drawn on top, so they win where regions overlap.
    pub fn hotspot_at(&self, x: f32, y: f32) -> Option<usize> {
        self.hotspots.iter().rposition(|s| hotspot_rect(s).contains(x, y))
    }

    /// Everything to draw this frame, back to front.
    ///
    /// `image_size` reports the pixel size of a loaded image; images it doesn't
    /// know are skipped. `hover` is the choice (or hotspot) under the mouse or
    /// the keyboard focus, if any.
    pub fn draw_list(&self, image_size: &dyn Fn(&str) -> Option<Size>, hover: Option<usize>) -> Vec<Draw> {
        self.draw_list_at(image_size, hover, None)
    }

    /// [`View::draw_list`] that also knows where the mouse is (virtual
    /// coordinates), so a hovered hotspot's label can float next to it.
    pub fn draw_list_at(
        &self,
        image_size: &dyn Fn(&str) -> Option<Size>,
        hover: Option<usize>,
        pointer: Option<(f32, f32)>,
    ) -> Vec<Draw> {
        let mut out = Vec::new();
        let screen = Rect::new(0.0, 0.0, VIRTUAL.w, VIRTUAL.h);

        // The stage: background and sprites, seen through the camera and shake.
        let mut stage = Vec::new();
        if let Some(prev) = &self.previous_background {
            push_image(&mut stage, prev, screen, Fit::Cover, 1.0, image_size);
        }
        if let Some(bg) = &self.background {
            let mut picture = Vec::new();
            push_image(&mut picture, &bg.src, screen, Fit::Cover, bg.alpha(), image_size);
            for draw in picture {
                match (draw, bg.revealed()) {
                    (Draw::Image { src, dest, uv, alpha }, Some(part)) => {
                        if let Some((dest, uv)) = clip(dest, uv, part) {
                            stage.push(Draw::Image { src, dest, uv, alpha });
                        }
                    }
                    (draw, _) => stage.push(draw),
                }
            }
        }
        // Back to front by z; the sort is stable, so equal z keeps the order sprites were shown in.
        let mut order: Vec<&SpriteView> = self.sprites.iter().collect();
        order.sort_by_key(|s| s.z);
        for s in order {
            let (x, y, scale) = (s.current_x(), s.current_y(), s.current_scale());
            if let Some(prev) = &s.previous_src {
                push_sprite(&mut stage, prev, (x, y), scale, 1.0, image_size);
            }
            push_sprite(&mut stage, &s.src, (x, y), scale, s.alpha(), image_size);
        }
        let view = self.stage_transform();
        for draw in stage {
            if let Draw::Image { src, dest, uv, alpha } = draw {
                if let Some((dest, uv)) = clip(view(dest), uv, screen) {
                    out.push(Draw::Image { src, dest, uv, alpha });
                }
            }
        }

        let stage = Rect::new(0.0, 0.0, VIRTUAL.w, TEXTBOX.y);
        for img in &self.images {
            push_image(&mut out, &img.src, stage, Fit::Contain, 1.0, image_size);
        }

        if let Some(video) = &self.video {
            out.push(Draw::Rect { rect: screen, color: [0.0, 0.0, 0.0, 1.0] });
            // The backend registers a playing video's current frame under its
            // file name, like an image. No frame (no decoder, or not started
            // yet): a placeholder says what should be playing.
            if image_size(&video.src).is_some() {
                push_image(&mut out, &video.src, screen, video.fit, 1.0, image_size);
            } else {
                out.push(Draw::Text {
                    text: format!("[video: {}]", video.src),
                    runs: Vec::new(),
                    visible_chars: usize::MAX,
                    rect: screen,
                    size: self.theme.text_size,
                    color: self.theme.text_color,
                    align: Align::Center,
                });
            }
        }

        let theme = &self.theme;
        for (i, spot) in self.hotspots.iter().enumerate() {
            let rect = hotspot_rect(spot);
            if let Some(src) = &spot.image {
                push_image(&mut out, src, rect, Fit::Stretch, 1.0, image_size);
            }
            if hover == Some(i) && !spot.hidden {
                let [r, g, b, _] = theme.accent_color;
                out.push(Draw::Rect { rect, color: [r, g, b, 0.18] });
                let t = HOTSPOT_OUTLINE;
                for bar in [
                    Rect::new(rect.x, rect.y, rect.w, t),
                    Rect::new(rect.x, rect.y + rect.h - t, rect.w, t),
                    Rect::new(rect.x, rect.y, t, rect.h),
                    Rect::new(rect.x + rect.w - t, rect.y, t, rect.h),
                ] {
                    out.push(Draw::Rect { rect: bar, color: theme.accent_color });
                }
            }
        }

        if let Some(d) = &self.dialogue {
            // The theme's textbox picture if it is loaded, else a plain panel.
            let picture = theme.textbox_image.as_deref().filter(|src| image_size(src).is_some());
            match picture {
                Some(src) => push_image(&mut out, src, TEXTBOX, Fit::Stretch, 1.0, image_size),
                None => out.push(Draw::Rect { rect: TEXTBOX, color: theme.panel_color }),
            }
            let pad = 40.0;
            let inner_w = TEXTBOX.w - 2.0 * pad;
            let has_name = match &d.speaker {
                Some(name) => {
                    let color = d.color.map(|[r, g, b]| [r, g, b, 1.0]).unwrap_or(theme.name_color);
                    out.push(Draw::Text {
                        text: name.clone(),
                        runs: Vec::new(),
                        visible_chars: usize::MAX,
                        rect: Rect::new(TEXTBOX.x + pad, TEXTBOX.y + 22.0, inner_w, 48.0),
                        size: theme.name_size,
                        color,
                        align: Align::Left,
                    });
                    true
                }
                None => false,
            };
            out.push(Draw::Text {
                text: d.text.clone(),
                runs: d.runs.clone(),
                visible_chars: d.visible_chars(),
                rect: dialogue_body(has_name),
                size: theme.text_size,
                color: theme.text_color,
                align: Align::Left,
            });
        }

        for (i, (rect, label)) in choice_rects(self.choices.len()).into_iter().zip(&self.choices).enumerate() {
            let color = if hover == Some(i) { theme.button_hover_color } else { theme.button_color };
            out.push(Draw::Rect { rect, color });
            let Rich { text, runs, .. } = rich(label);
            out.push(Draw::Text {
                text,
                runs,
                visible_chars: usize::MAX,
                rect,
                size: theme.text_size.min(rect.h * 0.5), // fits buttons squeezed by many options
                color: theme.text_color,
                align: Align::Center,
            });
        }

        // The hovered hotspot's label, next to the mouse (or under the region when the keyboard is on it).
        if let Some(spot) = hover.and_then(|i| self.hotspots.get(i)).filter(|s| !s.hidden) {
            let Rich { text, runs, .. } = rich(&spot.label);
            let size = (theme.text_size * 0.8).round();
            let w = (text.chars().map(|c| if is_wide(c) { size } else { size * 0.55 }).sum::<f32>() + 2.0 * TIP_PAD)
                .min(VIRTUAL.w - 2.0 * TIP_MARGIN);
            let h = size * 1.35 + TIP_PAD;
            let r = hotspot_rect(spot);
            let (ax, ay) = pointer.map_or((r.x + r.w / 2.0 - w / 2.0, r.y + r.h + 8.0), |(px, py)| (px + 24.0, py + 28.0));
            let tip = Rect::new(
                ax.clamp(TIP_MARGIN, VIRTUAL.w - w - TIP_MARGIN),
                ay.clamp(TIP_MARGIN, VIRTUAL.h - h - TIP_MARGIN),
                w,
                h,
            );
            out.push(Draw::Rect { rect: tip, color: theme.panel_color });
            out.push(Draw::Text {
                text,
                runs,
                visible_chars: usize::MAX,
                rect: tip,
                size,
                color: theme.text_color,
                align: Align::Center,
            });
        }

        out
    }
}

impl View {
    /// Maps a stage rectangle to the screen for the current camera and shake.
    fn stage_transform(&self) -> impl Fn(Rect) -> Rect {
        let c = self.camera.current();
        let z = c.zoom.max(1.0);
        let (w, h) = (VIRTUAL.w / z, VIRTUAL.h / z); // the part of the stage in view
        let left = (c.x * VIRTUAL.w - w / 2.0).clamp(0.0, VIRTUAL.w - w);
        let top = (c.y * VIRTUAL.h - h / 2.0).clamp(0.0, VIRTUAL.h - h);
        let (dx, dy) = self.shake.as_ref().map_or((0.0, 0.0), Shake::offset);
        move |r: Rect| Rect::new((r.x - left) * z + dx, (r.y - top) * z + dy, r.w * z, r.h * z)
    }
}

/// Button rectangles for `count` choices, stacked and centred above the textbox.
/// Where a line's words go in the textbox: below the name plate if there is one.
pub fn dialogue_body(with_speaker: bool) -> Rect {
    let pad = 40.0;
    let top = TEXTBOX.y + if with_speaker { 80.0 } else { 36.0 };
    Rect::new(TEXTBOX.x + pad, top, TEXTBOX.w - 2.0 * pad, TEXTBOX.y + TEXTBOX.h - top - 20.0)
}

/// About how many textbox lines `text` (tags removed) takes at `size` px, and how
/// many fit. A line that doesn't fit is drawn smaller by the player.
pub fn dialogue_lines(text: &str, with_speaker: bool, size: f32) -> (usize, usize) {
    let body = dialogue_body(with_speaker);
    let line_h = (size * 1.35).round().max(1.0); // what the text renderer uses
    (crate::estimate_lines(text, size, body.w), (body.h / line_h).floor() as usize)
}

/// A hotspot's region in virtual coordinates.
pub fn hotspot_rect(spot: &Spot) -> Rect {
    let [x, y, w, h] = spot.rect;
    Rect::new(x * VIRTUAL.w, y * VIRTUAL.h, w * VIRTUAL.w, h * VIRTUAL.h)
}

pub fn choice_rects(count: usize) -> Vec<Rect> {
    if count == 0 {
        return Vec::new();
    }
    // Many options: buttons and gaps shrink together so all of them stay above the textbox.
    let room = TEXTBOX.y - 2.0 * CHOICE_MARGIN;
    let full = count as f32 * CHOICE_H + (count - 1) as f32 * CHOICE_GAP;
    let k = (room / full).min(1.0);
    let (h, gap) = (CHOICE_H * k, CHOICE_GAP * k);
    let top = (TEXTBOX.y - full * k) / 2.0;
    let x = (VIRTUAL.w - CHOICE_W) / 2.0;
    (0..count).map(|i| Rect::new(x, top + i as f32 * (h + gap), CHOICE_W, h)).collect()
}

fn push_image(
    out: &mut Vec<Draw>,
    src: &str,
    container: Rect,
    fit: Fit,
    alpha: f32,
    image_size: &dyn Fn(&str) -> Option<Size>,
) {
    if let Some(size) = image_size(src) {
        let p = place(size, container, fit);
        out.push(Draw::Image { src: src.to_string(), dest: p.dest, uv: p.uv, alpha });
    }
}

/// A sprite centred on `x` (0..1 across the screen), standing on the bottom edge.
fn push_sprite(
    out: &mut Vec<Draw>,
    src: &str,
    (x, y): (f32, f32),
    scale: f32,
    alpha: f32,
    image_size: &dyn Fn(&str) -> Option<Size>,
) {
    let Some(size) = image_size(src) else { return };
    if size.w <= 0.0 || size.h <= 0.0 {
        return;
    }
    let h = SPRITE_HEIGHT * scale;
    let w = size.w / size.h * h;
    out.push(Draw::Image {
        src: src.to_string(),
        dest: Rect::new(x * VIRTUAL.w - w / 2.0, y * VIRTUAL.h - h, w, h),
        uv: Rect::new(0.0, 0.0, 1.0, 1.0),
        alpha,
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use quillstone_runtime::{Input, Session};
    use quillstone_script::parse;

    fn say(speaker: Option<&str>, text: &str) -> Cmd {
        Cmd::Say { speaker: speaker.map(str::to_string), color: None, text: text.into() }
    }

    fn bg(src: &str, fade: f32) -> Cmd {
        Cmd::SetBackground { src: src.into(), fade, wipe: None }
    }

    fn show(id: &str, src: &str, x: f32, fade: f32) -> Cmd {
        Cmd::ShowSprite { id: id.into(), src: src.into(), x, y: 1.0, z: 0, scale: 1.0, fade }
    }

    fn slide(id: &str, x: f32, scale: f32, time: f32, ease: Ease) -> Cmd {
        Cmd::MoveSprite { id: id.into(), x, y: 1.0, scale, time, ease }
    }

    fn images(list: &[Draw]) -> Vec<(&str, Rect, Rect)> {
        list.iter()
            .filter_map(|d| match d {
                Draw::Image { src, dest, uv, .. } => Some((src.as_str(), *dest, *uv)),
                _ => None,
            })
            .collect()
    }

    fn hide(id: &str, fade: f32) -> Cmd {
        Cmd::HideSprite { id: id.into(), fade }
    }

    fn no_images(_: &str) -> Option<Size> {
        None
    }

    fn all_1080p(_: &str) -> Option<Size> {
        Some(Size::new(1920.0, 1080.0))
    }

    fn close(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-4
    }

    fn theme() -> Theme {
        Theme::default()
    }

    // ---- typewriter and advancing ------------------------------------------

    #[test]
    fn typewriter_reveals_over_time_and_never_overshoots() {
        let mut v = View::new().with_speed(10.0);
        v.apply(&[say(None, "Hello")]);
        assert_eq!(v.dialogue.as_ref().unwrap().visible_text(), "");
        v.tick(0.25);
        assert_eq!(v.dialogue.as_ref().unwrap().visible_text(), "He");
        v.tick(100.0);
        let d = v.dialogue.as_ref().unwrap();
        assert_eq!(d.visible_text(), "Hello");
        assert!(d.is_complete());
    }

    #[test]
    fn text_tags_become_styled_runs_of_plain_text() {
        let mut v = View::new();
        v.apply(&[say(None, "A **bold** {color=#ff0000}red{/color} word.")]);
        let d = v.dialogue.as_ref().unwrap();
        assert_eq!(d.text, "A bold red word.");
        let sizes = |_: &str| None;
        let drawn = v.draw_list(&sizes, None);
        let runs = drawn.iter().find_map(|d| match d {
            Draw::Text { text, runs, .. } if text == "A bold red word." => Some(runs.clone()),
            _ => None,
        });
        let runs = runs.expect("the line is drawn");
        assert!(runs.iter().any(|r| r.style.bold && &d.text[r.start..r.end] == "bold"));
        assert!(runs.iter().any(|r| r.style.color == Some([1.0, 0.0, 0.0]) && &d.text[r.start..r.end] == "red"));
        v.apply(&[Cmd::ShowChoices { options: vec!["*Softly*".into()] }]);
        assert!(v.draw_list(&sizes, None).iter().any(|d| matches!(d, Draw::Text { text, runs, .. } if text == "Softly" && !runs.is_empty())));
    }

    #[test]
    fn pauses_hold_the_typewriter_and_a_click_skips_them() {
        let mut v = View::new().with_speed(10.0);
        v.apply(&[say(None, "Ab{w=1}cd")]);
        let shown = |v: &View| v.dialogue.as_ref().unwrap().visible_chars();
        v.tick(0.2);
        assert_eq!(shown(&v), 2, "typed up to the pause");
        v.tick(0.5);
        assert_eq!(shown(&v), 2, "still pausing");
        v.tick(0.6);
        assert_eq!(shown(&v), 3, "the pause ran out 0.1 s ago");
        v.apply(&[say(None, "Ab{w=1}cd")]);
        v.tick(0.3);
        assert!(!v.press_advance(), "the first press finishes the line");
        assert!(v.dialogue.as_ref().unwrap().is_complete());
    }

    #[test]
    fn typewriter_counts_characters_not_bytes() {
        let mut v = View::new().with_speed(10.0);
        v.apply(&[say(None, "héllo→✓")]);
        v.tick(0.2); // 2 chars
        assert_eq!(v.dialogue.as_ref().unwrap().visible_text(), "hé");
        v.tick(0.5); // 7 chars total
        assert_eq!(v.dialogue.as_ref().unwrap().visible_text(), "héllo→✓");
    }

    #[test]
    fn zero_speed_shows_lines_instantly() {
        let mut v = View::new().with_speed(0.0);
        v.apply(&[say(None, "Instant")]);
        assert!(v.dialogue.as_ref().unwrap().is_complete());
        assert!(v.press_advance());
    }

    #[test]
    fn switching_to_instant_mid_line_completes_it() {
        let mut v = View::new().with_speed(10.0);
        v.apply(&[say(None, "A long enough line")]);
        v.tick(0.2);
        v.set_speed(0.0);
        v.tick(0.0);
        assert!(v.dialogue.as_ref().unwrap().is_complete());
    }

    #[test]
    fn first_press_completes_the_line_second_press_advances() {
        let mut v = View::new().with_speed(10.0);
        v.apply(&[say(Some("A"), "A long enough line")]);
        v.tick(0.3);
        assert!(!v.press_advance()); // finishes typing
        assert!(v.dialogue.as_ref().unwrap().is_complete());
        assert!(v.press_advance()); // now advance
    }

    #[test]
    fn advance_is_ignored_while_choices_are_showing() {
        let mut v = View::new();
        v.apply(&[Cmd::ShowChoices { options: vec!["a".into(), "b".into()] }]);
        assert!(!v.press_advance());
    }

    #[test]
    fn a_redraw_shows_its_line_at_once() {
        let mut v = View::new().with_speed(10.0);
        v.apply(&[Cmd::ClearScene, say(None, "already read"), Cmd::ShowChoices { options: vec!["a".into()] }]);
        assert!(v.dialogue.as_ref().unwrap().is_complete());
        // the next ordinary line types out again
        v.apply(&[say(None, "new")]);
        assert!(!v.dialogue.as_ref().unwrap().is_complete());
    }

    // ---- background --------------------------------------------------------

    #[test]
    fn background_crossfades_then_drops_the_old_one() {
        let mut v = View::new();
        v.apply(&[bg("a.png", 0.0)]);
        assert_eq!(v.background.as_ref().unwrap().alpha(), 1.0);
        assert!(v.previous_background.is_none());

        v.apply(&[bg("b.png", 1.0)]);
        assert_eq!(v.previous_background.as_deref(), Some("a.png"));
        v.tick(0.5);
        assert!(close(v.background.as_ref().unwrap().alpha(), 0.5));
        assert!(v.previous_background.is_some());
        v.tick(0.6);
        assert_eq!(v.background.as_ref().unwrap().alpha(), 1.0);
        assert!(v.previous_background.is_none());
    }

    #[test]
    fn setting_the_same_background_again_does_nothing() {
        let mut v = View::new();
        v.apply(&[bg("a.png", 0.0)]);
        v.tick(5.0);
        v.apply(&[bg("a.png", 2.0)]);
        assert_eq!(v.background.as_ref().unwrap().alpha(), 1.0);
        assert!(v.previous_background.is_none());
    }

    // ---- sprites -----------------------------------------------------------

    #[test]
    fn sprites_fade_in() {
        let mut v = View::new();
        v.apply(&[show("mika", "m.png", 0.5, 1.0)]);
        assert_eq!(v.sprites[0].alpha(), 0.0);
        v.tick(0.25);
        assert!(close(v.sprites[0].alpha(), 0.25));
        v.tick(2.0);
        assert_eq!(v.sprites[0].alpha(), 1.0);
    }

    #[test]
    fn sprites_with_no_fade_appear_at_once() {
        let mut v = View::new();
        v.apply(&[show("mika", "m.png", 0.5, 0.0)]);
        assert_eq!(v.sprites[0].alpha(), 1.0);
    }

    #[test]
    fn hiding_fades_out_and_then_removes_the_sprite() {
        let mut v = View::new();
        v.apply(&[show("mika", "m.png", 0.5, 0.0)]);
        v.apply(&[hide("mika", 1.0)]);
        assert_eq!(v.sprites.len(), 1);
        v.tick(0.5);
        assert!(close(v.sprites[0].alpha(), 0.5));
        v.tick(0.6);
        assert!(v.sprites.is_empty());
    }

    #[test]
    fn hiding_with_no_fade_removes_immediately() {
        let mut v = View::new();
        v.apply(&[show("mika", "m.png", 0.5, 0.0), hide("mika", 0.0)]);
        assert!(v.sprites.is_empty());
    }

    #[test]
    fn a_pose_change_crossfades_and_keeps_one_sprite() {
        let mut v = View::new();
        v.apply(&[show("mika", "neutral.png", 0.3, 0.0)]);
        v.apply(&[show("mika", "happy.png", 0.3, 1.0)]);
        assert_eq!(v.sprites.len(), 1);
        assert_eq!(v.sprites[0].src, "happy.png");
        assert_eq!(v.sprites[0].previous_src.as_deref(), Some("neutral.png"));
        v.tick(0.5);
        let list = v.draw_list(&|_| Some(Size::new(500.0, 1000.0)), None);
        let alphas: Vec<(&str, f32)> = list
            .iter()
            .filter_map(|d| match d {
                Draw::Image { src, alpha, .. } => Some((src.as_str(), *alpha)),
                _ => None,
            })
            .collect();
        assert_eq!(alphas.len(), 2);
        assert_eq!(alphas[0], ("neutral.png", 1.0)); // old pose underneath
        assert_eq!(alphas[1].0, "happy.png");
        assert!(close(alphas[1].1, 0.5));
        v.tick(1.0);
        assert!(v.sprites[0].previous_src.is_none());
    }

    #[test]
    fn sprites_slide_between_positions() {
        let mut v = View::new();
        v.apply(&[show("mika", "m.png", 0.2, 0.0)]);
        v.apply(&[slide("mika", 0.8, 1.0, 1.0, Ease::Linear)]);
        assert!(close(v.sprites[0].current_x(), 0.2));
        v.tick(0.5);
        assert!(close(v.sprites[0].current_x(), 0.5));
        v.tick(1.0);
        assert!(close(v.sprites[0].current_x(), 0.8));
        // a second move starts from where the sprite is
        v.apply(&[slide("mika", 0.0, 1.0, 1.0, Ease::Linear)]);
        v.tick(0.5);
        assert!(close(v.sprites[0].current_x(), 0.4));
    }

    #[test]
    fn showing_at_a_new_position_without_a_move_jumps() {
        let mut v = View::new();
        v.apply(&[show("mika", "m.png", 0.2, 0.0), show("mika", "m.png", 0.9, 0.0)]);
        assert!(close(v.sprites[0].current_x(), 0.9));
    }

    #[test]
    fn showing_a_sprite_that_is_fading_out_starts_it_over() {
        let mut v = View::new();
        v.apply(&[show("mika", "m.png", 0.5, 0.0), hide("mika", 1.0)]);
        v.tick(0.5);
        v.apply(&[show("mika", "m.png", 0.5, 1.0)]);
        assert_eq!(v.sprites.len(), 1);
        assert!(!v.sprites[0].hiding);
        assert_eq!(v.sprites[0].alpha(), 0.0);
    }

    #[test]
    fn sprites_stand_on_the_bottom_edge_centred_on_x() {
        let mut v = View::new();
        v.apply(&[Cmd::ShowSprite { id: "m".into(), src: "m.png".into(), x: 0.25, y: 1.0, z: 0, scale: 0.5, fade: 0.0 }]);
        // a 1:2 picture at half scale: 470 tall, 235 wide
        let list = v.draw_list(&|_| Some(Size::new(500.0, 1000.0)), None);
        let Draw::Image { dest, uv, alpha, .. } = &list[0] else { panic!("expected a sprite") };
        assert!(close(dest.h, SPRITE_HEIGHT * 0.5) && close(dest.w, SPRITE_HEIGHT * 0.25));
        assert!(close(dest.y + dest.h, VIRTUAL.h)); // bottom edge
        assert!(close(dest.x + dest.w / 2.0, 0.25 * VIRTUAL.w)); // centred on x
        assert_eq!((*uv, *alpha), (Rect::new(0.0, 0.0, 1.0, 1.0), 1.0));
    }

    fn sprite(id: &str, x: f32, y: f32, z: i32) -> Cmd {
        Cmd::ShowSprite { id: id.into(), src: format!("{id}.png"), x, y, z, scale: 1.0, fade: 0.0 }
    }

    fn drawn_order(v: &View) -> Vec<String> {
        images(&v.draw_list(&all_1080p, None)).iter().map(|(src, _, _)| src.to_string()).collect()
    }

    #[test]
    fn sprites_sit_where_y_puts_their_bottom_edge() {
        let mut v = View::new();
        v.apply(&[sprite("m", 0.5, 0.95, 0)]);
        let list = v.draw_list(&|_| Some(Size::new(500.0, 1000.0)), None);
        let (_, dest, uv) = images(&list)[0];
        assert!(close(dest.y + dest.h, 0.95 * VIRTUAL.h), "bottom edge at 95% of the height");
        assert!(close(dest.h, SPRITE_HEIGHT) && close(uv.h, 1.0));
    }

    #[test]
    fn sprites_below_or_above_the_screen_are_clipped() {
        let mut v = View::new();
        v.apply(&[sprite("low", 0.5, 1.2, 0), sprite("high", 0.5, -0.1, 0)]);
        let list = v.draw_list(&|_| Some(Size::new(500.0, 1000.0)), None);
        let shown = images(&list);
        // y = 1.2: the bottom 216 px of the 940 px sprite hang below the screen
        let (_, dest, uv) = shown[0];
        assert!(close(dest.y + dest.h, VIRTUAL.h));
        assert!(close(uv.y, 0.0) && close(uv.h, (940.0 - 0.2 * VIRTUAL.h) / 940.0), "cut at the bottom edge: {uv:?}");
        // y = -0.1: its bottom edge is above the screen, so nothing of it shows
        assert_eq!(shown.len(), 1, "a sprite wholly off-screen is dropped");
        // y = 0.5: 540 of 940 px stay above the screen's middle, the rest is cut at the top
        v.apply(&[Cmd::ClearScene, sprite("mid", 0.5, 0.5, 0)]);
        let list = v.draw_list(&|_| Some(Size::new(500.0, 1000.0)), None);
        let (_, dest, uv) = images(&list)[0];
        assert!(close(dest.y, 0.0) && close(dest.h, 540.0));
        assert!(close(uv.y, 400.0 / 940.0), "the top 400 px are cut off: {uv:?}");
    }

    #[test]
    fn sprites_slide_vertically_with_a_move() {
        let mut v = View::new();
        v.apply(&[sprite("m", 0.5, 1.0, 0), slide_y("m", 0.5, 0.8, 1.0, Ease::Linear)]);
        assert!(close(v.sprites[0].current_y(), 1.0));
        v.tick(0.5);
        assert!(close(v.sprites[0].current_y(), 0.9));
        v.tick(1.0);
        assert!(close(v.sprites[0].current_y(), 0.8));
        // showing it again with another y puts it there at once
        v.apply(&[sprite("m", 0.5, 0.6, 0)]);
        assert!(close(v.sprites[0].current_y(), 0.6));
    }

    fn slide_y(id: &str, x: f32, y: f32, time: f32, ease: Ease) -> Cmd {
        Cmd::MoveSprite { id: id.into(), x, y, scale: 1.0, time, ease }
    }

    #[test]
    fn higher_z_draws_in_front_and_ties_keep_show_order() {
        let mut v = View::new();
        v.apply(&[sprite("a", 0.3, 1.0, 0), sprite("b", 0.4, 1.0, 0), sprite("c", 0.5, 1.0, 0)]);
        assert_eq!(drawn_order(&v), ["a.png", "b.png", "c.png"]);
        v.apply(&[sprite("a", 0.3, 1.0, 5)]); // shown again with a higher z
        assert_eq!(drawn_order(&v), ["b.png", "c.png", "a.png"]);
        v.apply(&[sprite("c", 0.5, 1.0, -1)]);
        assert_eq!(drawn_order(&v), ["c.png", "b.png", "a.png"]);
        v.apply(&[sprite("c", 0.5, 1.0, 0)]);
        assert_eq!(drawn_order(&v), ["b.png", "c.png", "a.png"], "equal z: the order they were shown in");
    }

    #[test]
    fn z_orders_only_sprites_not_the_background_or_the_textbox() {
        let mut v = View::new().with_speed(0.0);
        v.apply(&[bg("bg.png", 0.0), sprite("lo", 0.5, 1.0, -100), say(Some("A"), "hi")]);
        let list = v.draw_list(&all_1080p, None);
        let pos = |name: &str| list.iter().position(|d| matches!(d, Draw::Image { src, .. } if src == name)).unwrap();
        let panel = list.iter().position(|d| matches!(d, Draw::Rect { rect, .. } if *rect == TEXTBOX)).unwrap();
        assert!(pos("bg.png") < pos("lo.png") && pos("lo.png") < panel);
        v.apply(&[sprite("hi", 0.5, 1.0, 100)]);
        let list = v.draw_list(&all_1080p, None);
        let pos = |name: &str| list.iter().position(|d| matches!(d, Draw::Image { src, .. } if src == name)).unwrap();
        let panel = list.iter().position(|d| matches!(d, Draw::Rect { rect, .. } if *rect == TEXTBOX)).unwrap();
        assert!(pos("bg.png") < pos("lo.png") && pos("hi.png") < panel && pos("lo.png") < pos("hi.png"));
    }

    #[test]
    fn sprites_draw_over_the_background_and_under_the_textbox() {
        let mut v = View::new().with_speed(0.0);
        v.apply(&[bg("bg.png", 0.0), show("m", "m.png", 0.5, 0.0), say(Some("A"), "hi")]);
        let list = v.draw_list(&all_1080p, None);
        let pos = |name: &str| list.iter().position(|d| matches!(d, Draw::Image { src, .. } if src == name)).unwrap();
        let panel = list.iter().position(|d| matches!(d, Draw::Rect { rect, .. } if *rect == TEXTBOX)).unwrap();
        assert!(pos("bg.png") < pos("m.png") && pos("m.png") < panel);
    }

    #[test]
    fn clear_scene_removes_background_and_sprites_but_not_the_line() {
        let mut v = View::new();
        v.apply(&[bg("bg.png", 0.0), show("m", "m.png", 0.5, 0.0), say(None, "keep me")]);
        v.apply(&[Cmd::ClearScene]);
        assert!(v.background.is_none() && v.sprites.is_empty());
        assert!(v.dialogue.is_some());
    }

    #[test]
    fn sprites_grow_and_ease() {
        let mut v = View::new();
        v.apply(&[show("m", "m.png", 0.5, 0.0), slide("m", 0.5, 2.0, 1.0, Ease::InOut)]);
        assert!(close(v.sprites[0].current_scale(), 1.0));
        v.tick(0.25);
        let quarter = v.sprites[0].current_scale();
        assert!(quarter > 1.0 && quarter < 1.25, "eased: slow at the start ({quarter})");
        v.tick(0.25);
        assert!(close(v.sprites[0].current_scale(), 1.5));
        v.tick(1.0);
        assert!(close(v.sprites[0].current_scale(), 2.0));
        let list = v.draw_list(&|_| Some(Size::new(500.0, 1000.0)), None);
        let (_, dest, _) = images(&list)[0];
        assert!(close(dest.h, VIRTUAL.h), "2x a 940 px sprite is cut at the screen's top: {}", dest.h);
    }

    #[test]
    fn sprites_hanging_off_the_screen_are_cut_at_its_edge() {
        let mut v = View::new();
        v.apply(&[show("m", "m.png", 0.0, 0.0)]); // centred on the left edge
        let list = v.draw_list(&|_| Some(Size::new(500.0, 1000.0)), None);
        let (_, dest, uv) = images(&list)[0];
        assert_eq!(dest.x, 0.0);
        assert!(close(uv.x, 0.5) && close(uv.w, 0.5), "only the right half shows");
    }

    #[test]
    fn a_wipe_reveals_the_new_background_from_one_side() {
        let mut v = View::new();
        v.apply(&[bg("a.png", 0.0)]);
        v.apply(&[Cmd::SetBackground { src: "b.png".into(), fade: 1.0, wipe: Some(Wipe::Right) }]);
        v.tick(0.25);
        let list = v.draw_list(&all_1080p, None);
        let shown = images(&list);
        assert_eq!(shown[0].0, "a.png");
        assert_eq!(shown[0].1, Rect::new(0.0, 0.0, VIRTUAL.w, VIRTUAL.h), "the old one stays whole underneath");
        let (src, dest, uv) = shown[1];
        assert_eq!(src, "b.png");
        assert!(close(dest.w, VIRTUAL.w * 0.25) && dest.x == 0.0);
        assert!(close(uv.w, 0.25));
        assert!(list.iter().all(|d| !matches!(d, Draw::Image { alpha, .. } if *alpha < 1.0)), "no fading");
        v.tick(1.0);
        assert!(v.previous_background.is_none());
        assert_eq!(images(&v.draw_list(&all_1080p, None)).len(), 1);
    }

    #[test]
    fn the_camera_zooms_into_the_stage_but_not_the_textbox() {
        let mut v = View::new().with_speed(0.0);
        let camera = Camera { zoom: 2.0, x: 0.25, y: 0.25 };
        v.apply(&[bg("bg.png", 0.0), say(None, "hi"), Cmd::Camera { camera, time: 1.0, ease: Ease::Linear }]);
        v.tick(1.0);
        assert_eq!(v.camera(), camera);
        let list = v.draw_list(&all_1080p, None);
        let (_, dest, uv) = images(&list)[0];
        assert_eq!(dest, Rect::new(0.0, 0.0, VIRTUAL.w, VIRTUAL.h), "still fills the screen");
        assert!(close(uv.w, 0.5) && close(uv.x, 0.0) && close(uv.y, 0.0), "showing the top-left quarter");
        assert!(list.iter().any(|d| matches!(d, Draw::Rect { rect, .. } if *rect == TEXTBOX)));
        // the view can't leave the stage: x=0 still keeps the left edge on screen
        v.apply(&[Cmd::Camera { camera: Camera { zoom: 2.0, x: 0.0, y: 0.5 }, time: 0.0, ease: Ease::Linear }]);
        let (_, _, uv) = images(&v.draw_list(&all_1080p, None))[0];
        assert!(close(uv.x, 0.0) && close(uv.y, 0.25));
        // a redraw resets it
        v.apply(&[Cmd::ClearScene]);
        assert_eq!(v.camera(), Camera::default());
    }

    #[test]
    fn camera_moves_start_where_the_camera_is() {
        let mut v = View::new();
        let near = Camera { zoom: 2.0, x: 0.5, y: 0.5 };
        v.apply(&[Cmd::Camera { camera: near, time: 2.0, ease: Ease::Linear }]);
        v.tick(1.0);
        assert!(close(v.camera().zoom, 1.5));
        v.apply(&[Cmd::Camera { camera: Camera::default(), time: 1.0, ease: Ease::Linear }]);
        v.tick(0.5);
        assert!(close(v.camera().zoom, 1.25));
    }

    #[test]
    fn a_shake_wobbles_the_stage_and_dies_away() {
        let mut v = View::new();
        v.apply(&[bg("bg.png", 0.0), Cmd::Shake { time: 0.5, strength: 30.0 }]);
        let mut moved = false;
        for _ in 0..4 {
            v.tick(0.05);
            let list = v.draw_list(&all_1080p, None);
            let (_, dest, _) = images(&list)[0];
            moved |= dest.x != 0.0 || dest.y != 0.0;
            assert!(dest.x.abs() <= 30.0 && dest.y.abs() <= 30.0);
        }
        assert!(moved);
        v.tick(1.0);
        let (_, dest, _) = images(&v.draw_list(&all_1080p, None))[0];
        assert_eq!(dest, Rect::new(0.0, 0.0, VIRTUAL.w, VIRTUAL.h), "back in place");
    }

    // ---- speaker colour -------------------------------------------------------

    #[test]
    fn name_plate_uses_the_speakers_colour() {
        let mut v = View::new().with_speed(0.0);
        v.apply(&[Cmd::Say { speaker: Some("Mika".into()), color: Some([1.0, 0.0, 0.5]), text: "hi".into() }]);
        let list = v.draw_list(&no_images, None);
        let name_color = list
            .iter()
            .find_map(|d| match d {
                Draw::Text { text, color, .. } if text == "Mika" => Some(*color),
                _ => None,
            })
            .unwrap();
        assert_eq!(name_color, [1.0, 0.0, 0.5, 1.0]);

        // no colour declared: the default
        v.apply(&[say(Some("Bob"), "hi")]);
        let list = v.draw_list(&no_images, None);
        assert!(list.iter().any(|d| matches!(d, Draw::Text { text, color, .. } if text == "Bob" && *color == theme().name_color)));
    }

    // ---- wait ---------------------------------------------------------------------

    #[test]
    fn wait_counts_down_and_reports_once() {
        let mut v = View::new();
        v.apply(&[Cmd::Wait { seconds: 1.0 }]);
        assert!(!v.take_wait_finished());
        v.tick(0.6);
        assert!(!v.take_wait_finished());
        v.tick(0.6);
        assert!(v.take_wait_finished());
        assert!(!v.take_wait_finished()); // only once
    }

    #[test]
    fn a_press_skips_a_wait_even_while_text_is_typing() {
        let mut v = View::new().with_speed(1.0);
        v.apply(&[say(None, "slow slow slow"), Cmd::Wait { seconds: 5.0 }]);
        assert!(v.press_advance());
        // the next batch ends the wait
        v.apply(&[say(None, "next")]);
        assert!(!v.take_wait_finished());
    }

    // ---- misc state -------------------------------------------------------------------

    #[test]
    fn choice_buttons_are_stacked_centred_and_hit_testable() {
        let rects = choice_rects(2);
        assert_eq!(rects.len(), 2);
        assert!(rects[0].y < rects[1].y);
        assert_eq!(rects[0].x, (VIRTUAL.w - CHOICE_W) / 2.0);
        assert!(rects[1].y + rects[1].h < TEXTBOX.y); // both sit above the textbox
        assert_eq!(rects[0].h, CHOICE_H, "few options keep the full size");
        for n in [6, 7, 9, 15] {
            let rects = choice_rects(n);
            let last = rects[n - 1];
            assert!(rects[0].y >= CHOICE_MARGIN - 0.01 && last.y + last.h <= TEXTBOX.y - CHOICE_MARGIN + 0.01, "{n} fit");
            assert!(rects.windows(2).all(|w| w[0].y + w[0].h < w[1].y), "{n} don't overlap");
        }

        let mut v = View::new();
        v.apply(&[Cmd::ShowChoices { options: vec!["a".into(), "b".into()] }]);
        assert_eq!(v.choice_at(rects[0].x + 10.0, rects[0].y + 10.0), Some(0));
        assert_eq!(v.choice_at(rects[1].x + 10.0, rects[1].y + 10.0), Some(1));
        assert_eq!(v.choice_at(5.0, 5.0), None);
    }

    #[test]
    fn a_new_batch_clears_choices_images_and_video_but_keeps_the_last_line() {
        let mut v = View::new();
        v.apply(&[say(Some("A"), "hi"), Cmd::ShowImage { src: "x.png".into(), alt: "".into() }]);
        assert_eq!(v.images.len(), 1);
        v.apply(&[Cmd::ShowChoices { options: vec!["go".into()] }]);
        assert!(v.images.is_empty());
        assert!(v.dialogue.is_some());
        v.apply(&[say(None, "next")]);
        assert!(v.choices.is_empty());
    }

    #[test]
    fn video_replaces_the_dialogue_and_is_flagged() {
        let mut v = View::new();
        v.apply(&[say(None, "before")]);
        v.apply(&[Cmd::PlayVideo { src: "c.webm".into(), fit: Fit::Cover, looped: false, skippable: true }]);
        assert!(v.dialogue.is_none());
        assert_eq!(v.video.as_ref().unwrap().src, "c.webm");
    }

    #[test]
    fn a_playing_video_draws_its_frame_with_its_fit() {
        let mut v = View::new();
        v.apply(&[Cmd::PlayVideo { src: "c.webm".into(), fit: Fit::Contain, looped: false, skippable: true }]);
        // not playing (yet): the placeholder
        assert!(v.draw_list(&no_images, None).iter().any(|d| matches!(d, Draw::Text { .. })));
        // a 4:3 frame, contained: pillarboxed on black
        let frame = |src: &str| (src == "c.webm").then_some(Size::new(640.0, 480.0));
        let list = v.draw_list(&frame, None);
        assert!(matches!(list[0], Draw::Rect { color, .. } if color == [0.0, 0.0, 0.0, 1.0]));
        let (src, dest, _) = images(&list)[0];
        assert_eq!(src, "c.webm");
        assert!(close(dest.h, VIRTUAL.h) && close(dest.w, VIRTUAL.h * 4.0 / 3.0));
        assert!(!list.iter().any(|d| matches!(d, Draw::Text { .. })));
    }

    fn spot(label: &str, rect: [f32; 4], image: Option<&str>) -> Spot {
        Spot { label: label.into(), rect, image: image.map(String::from), hidden: false }
    }

    #[test]
    fn hotspots_are_hit_tested_in_virtual_coordinates_and_the_top_one_wins() {
        let mut v = View::new();
        v.apply(&[Cmd::ShowHotspots {
            spots: vec![
                spot("big", [0.0, 0.0, 0.5, 0.5], None),
                spot("small", [0.1, 0.1, 0.1, 0.1], None),
                spot("far", [0.9, 0.9, 0.1, 0.1], None),
            ],
        }]);
        assert_eq!(hotspot_rect(&v.hotspots[1]), Rect::new(192.0, 108.0, 192.0, 108.0));
        assert_eq!(v.hotspot_at(250.0, 150.0), Some(1), "later regions are on top");
        assert_eq!(v.hotspot_at(700.0, 400.0), Some(0));
        assert_eq!(v.hotspot_at(1900.0, 1070.0), Some(2));
        assert_eq!(v.hotspot_at(1000.0, 1000.0), None);
        assert!(!v.press_advance(), "advance doesn't skip over a hotspot screen");
        v.apply(&[say(None, "next")]);
        assert!(v.hotspots.is_empty() && v.hotspot_at(250.0, 150.0).is_none());
    }

    #[test]
    fn hotspot_draws_images_highlight_and_a_tooltip_kept_on_screen() {
        let mut v = View::new();
        v.apply(&[
            bg("bg.png", 0.0),
            Cmd::ShowHotspots {
                spots: vec![spot("Key", [0.5, 0.5, 0.1, 0.1], Some("key.png")), spot("Hidden", [0.0, 0.0, 0.1, 0.1], None)],
            },
        ]);
        let known = |s: &str| (s == "bg.png" || s == "key.png").then_some(Size::new(1920.0, 1080.0));
        let idle = v.draw_list(&known, None);
        let key = Rect::new(960.0, 540.0, 192.0, 108.0);
        assert!(matches!(&idle[1], Draw::Image { src, dest, .. } if src == "key.png" && *dest == key), "stretched into the rect");
        assert_eq!(idle.len(), 2, "nothing else while idle: unimage'd regions are invisible");

        let hot = v.draw_list_at(&known, Some(0), Some((970.0, 560.0)));
        let outline = hot.iter().filter(|d| matches!(d, Draw::Rect { color, .. } if *color == theme().accent_color)).count();
        assert_eq!(outline, 4, "four outline bars");
        let tip = hot.iter().find_map(|d| match d {
            Draw::Text { text, rect, .. } if text == "Key" => Some(*rect),
            _ => None,
        });
        let tip = tip.expect("the label is drawn");
        assert!(close(tip.x, 994.0) && close(tip.y, 588.0), "next to the cursor: {tip:?}");

        // at the bottom right corner the tooltip is pushed back on screen
        v.apply(&[Cmd::ShowHotspots { spots: vec![spot("A rather long place name", [0.9, 0.9, 0.1, 0.1], None)] }]);
        let hot = v.draw_list_at(&all_1080p, Some(0), Some((1915.0, 1075.0)));
        let tip = hot.iter().find_map(|d| match d {
            Draw::Text { rect, .. } => Some(*rect),
            _ => None,
        });
        let tip = tip.unwrap();
        assert!(tip.x >= 0.0 && tip.y >= 0.0 && tip.x + tip.w <= VIRTUAL.w && tip.y + tip.h <= VIRTUAL.h, "{tip:?}");
        // keyboard focus (no pointer): under the region, still on screen
        let hot = v.draw_list_at(&all_1080p, Some(0), None);
        assert!(hot.iter().any(|d| matches!(d, Draw::Text { rect, .. } if rect.y + rect.h <= VIRTUAL.h)));
    }

    #[test]
    fn a_hidden_hotspot_shows_no_tint_outline_or_tooltip_but_keeps_its_image_and_hit_test() {
        let mut v = View::new();
        let mut s = spot("Scarf", [0.5, 0.5, 0.1, 0.1], Some("key.png"));
        s.hidden = true;
        let mut plain = spot("Nook", [0.0, 0.0, 0.1, 0.1], None);
        plain.hidden = true;
        v.apply(&[Cmd::ShowHotspots { spots: vec![s, plain] }]);
        let known = |s: &str| (s == "key.png").then_some(Size::new(1920.0, 1080.0));
        for hover in [Some(0), Some(1)] {
            let d = v.draw_list_at(&known, hover, Some((970.0, 560.0)));
            assert!(!d.iter().any(|d| matches!(d, Draw::Text { .. })), "no tooltip");
            assert!(!d.iter().any(|d| matches!(d, Draw::Rect { .. })), "no tint or outline");
        }
        let d = v.draw_list_at(&known, Some(1), None);
        assert!(d.iter().any(|d| matches!(d, Draw::Image { src, .. } if src == "key.png")), "image still drawn");
        assert_eq!(v.hotspot_at(970.0, 560.0), Some(0));
        assert_eq!(v.hotspot_at(20.0, 20.0), Some(1));
    }

    #[test]
    fn hotspots_draw_below_the_textbox() {
        let mut v = View::new();
        v.apply(&[say(None, "Look."), Cmd::ShowHotspots { spots: vec![spot("x", [0.1, 0.1, 0.1, 0.1], Some("k.png"))] }]);
        let list = v.draw_list(&all_1080p, None);
        let key = list.iter().position(|d| matches!(d, Draw::Image { src, .. } if src == "k.png")).unwrap();
        let panel = list.iter().position(|d| matches!(d, Draw::Rect { rect, .. } if *rect == TEXTBOX)).unwrap();
        assert!(key < panel);
    }

    #[test]
    fn finished_follows_the_latest_batch() {
        let mut v = View::new();
        v.apply(&[Cmd::Finished]);
        assert!(v.finished);
        // e.g. after a rollback redraw
        v.apply(&[say(None, "earlier line")]);
        assert!(!v.finished);
    }

    // ---- the draw list -------------------------------------------------------------------

    #[test]
    fn draw_list_for_a_spoken_line() {
        let mut v = View::new().with_speed(10.0);
        v.apply(&[bg("bg.png", 0.0), say(Some("Mika"), "Hello")]);
        v.tick(0.3);
        let list = v.draw_list(&all_1080p, None);

        assert!(matches!(&list[0], Draw::Image { src, alpha, .. } if src == "bg.png" && *alpha == 1.0));
        assert!(list.contains(&Draw::Rect { rect: TEXTBOX, color: theme().panel_color }));
        assert!(list.iter().any(|d| matches!(d, Draw::Text { text, .. } if text == "Mika")));
        let body = list
            .iter()
            .find_map(|d| match d {
                Draw::Text { text, visible_chars, .. } if text == "Hello" => Some(*visible_chars),
                _ => None,
            })
            .expect("body text");
        assert_eq!(body, 3); // full text is passed, only 3 chars visible yet
    }

    #[test]
    fn narration_has_no_name_plate() {
        let mut v = View::new().with_speed(0.0);
        v.apply(&[say(None, "The wind blows.")]);
        let list = v.draw_list(&no_images, None);
        assert_eq!(list.iter().filter(|d| matches!(d, Draw::Text { .. })).count(), 1);
    }

    #[test]
    fn images_the_app_has_not_loaded_are_skipped_not_fatal() {
        let mut v = View::new();
        v.apply(&[bg("missing.png", 0.0), show("m", "missing.png", 0.5, 0.0)]);
        assert!(v.draw_list(&no_images, None).is_empty());
    }

    #[test]
    fn cover_background_uses_uv_cropping() {
        let mut v = View::new();
        v.apply(&[bg("wide.png", 0.0)]);
        let wide = |_: &str| Some(Size::new(4000.0, 1080.0));
        let list = v.draw_list(&wide, None);
        let Draw::Image { dest, uv, .. } = &list[0] else { panic!("expected image") };
        assert_eq!(*dest, Rect::new(0.0, 0.0, VIRTUAL.w, VIRTUAL.h));
        assert!(uv.w < 1.0 && uv.x > 0.0);
    }

    #[test]
    fn the_theme_sets_colours_sizes_and_the_textbox_picture() {
        let theme = Theme {
            text_size: 50.0,
            text_color: [1.0, 0.0, 0.0, 1.0],
            textbox_image: Some("box.png".into()),
            ..Theme::default()
        };
        let mut v = View::new().with_speed(0.0).with_theme(theme);
        v.apply(&[say(None, "hi")]);
        let list = v.draw_list(&all_1080p, None);
        assert!(list.iter().any(|d| matches!(d, Draw::Image { src, dest, .. } if src == "box.png" && *dest == TEXTBOX)));
        assert!(list.iter().any(|d| matches!(d, Draw::Text { size, color, .. } if *size == 50.0 && color[0] == 1.0 && color[1] == 0.0)));
        // The picture isn't loaded: fall back to the panel.
        let list = v.draw_list(&no_images, None);
        assert!(list.iter().any(|d| matches!(d, Draw::Rect { rect, .. } if *rect == TEXTBOX)));
    }

    #[test]
    fn hovered_choice_gets_the_highlight_colour() {
        let mut v = View::new();
        v.apply(&[Cmd::ShowChoices { options: vec!["a".into(), "b".into()] }]);
        let list = v.draw_list(&no_images, Some(1));
        let colors: Vec<[f32; 4]> = list
            .iter()
            .filter_map(|d| match d {
                Draw::Rect { color, .. } => Some(*color),
                _ => None,
            })
            .collect();
        assert_eq!(colors, vec![theme().button_color, theme().button_hover_color]);
    }

    #[test]
    fn an_image_directly_before_a_video_is_covered_by_it() {
        // Documents the rule: images show with the next *waiting* op, here the video.
        let mut view = View::new();
        view.apply(&[
            Cmd::ShowImage { src: "x.png".into(), alt: "".into() },
            Cmd::PlayVideo { src: "c.webm".into(), fit: Fit::Cover, looped: false, skippable: true },
        ]);
        let list = view.draw_list(&all_1080p, None);
        let image_at = list.iter().position(|d| matches!(d, Draw::Image { .. })).unwrap();
        let cover_at = list
            .iter()
            .position(|d| matches!(d, Draw::Rect { color, .. } if *color == [0.0, 0.0, 0.0, 1.0]))
            .unwrap();
        assert!(cover_at > image_at, "the video draws over the image");
    }

    // ---- driven by the real runtime and the real demo ---------------------------------------

    fn demo() -> quillstone_script::Program {
        parse(include_str!("../../../examples/demo.quill.md")).unwrap()
    }

    #[test]
    fn the_demo_opens_with_a_background_a_sprite_and_a_coloured_name() {
        let program = demo();
        let (_, first) = Session::start(&program);
        let mut view = View::new().with_speed(0.0);
        view.apply(&first);
        assert_eq!(view.background.as_ref().unwrap().src, "assets/rooftop.png");
        assert_eq!(view.sprites.len(), 1);
        assert_eq!(view.sprites[0].src, "assets/mika_neutral.png");
        assert!(close(view.sprites[0].x, 0.72)); // "right"
        let d = view.dialogue.as_ref().unwrap();
        assert_eq!((d.speaker.as_deref(), d.color.is_some()), (Some("Mika"), true));
    }

    #[test]
    fn an_inline_image_appears_together_with_the_next_line_in_the_demo() {
        let program = demo();
        let (mut session, first) = Session::start(&program);
        let mut view = View::new().with_speed(0.0);
        view.apply(&first);
        assert!(view.images.is_empty());

        // Advancing past Mika's first line reveals the scarf with the narration.
        let cmds = session.input(Input::Advance);
        view.apply(&cmds);
        assert_eq!(view.images.len(), 1);
        assert_eq!(view.images[0].src, "assets/scarf.png");
        assert!(view.dialogue.as_ref().unwrap().text.contains("scarf"));

        let scarf = |_: &str| Some(Size::new(700.0, 460.0));
        let list = view.draw_list(&scarf, None);
        assert!(list.iter().any(|d| matches!(d, Draw::Image { src, .. } if src == "assets/scarf.png")));
    }

    #[test]
    fn rolling_back_restores_the_scarf_and_removes_later_sprites() {
        let program = demo();
        let (mut session, first) = Session::start(&program);
        let mut view = View::new().with_speed(0.0);
        view.apply(&first);
        view.apply(&session.input(Input::Advance)); // narration with the scarf
        assert_eq!(view.images.len(), 1);
        view.apply(&session.input(Input::Advance)); // the flashback video
        assert!(view.images.is_empty() && view.video.is_some());

        let back = session.rollback().unwrap();
        view.apply(&back);
        assert_eq!(view.images.len(), 1, "the inline image comes back after rollback");
        assert!(view.video.is_none());
        assert_eq!(view.sprites.len(), 1);
        assert!(view.dialogue.as_ref().unwrap().text.contains("scarf"));
    }

    #[test]
    fn drives_a_real_script_through_to_the_choices_and_an_ending() {
        let program = demo();
        let (mut session, first) = Session::start(&program);
        let mut view = View::new().with_speed(0.0);
        view.apply(&first);

        // Press through until the choices appear, exactly as the player loop does.
        for _ in 0..20 {
            if !view.choices.is_empty() {
                break;
            }
            if view.press_advance() {
                view.apply(&session.input(Input::Advance));
            }
        }
        // `met` was set by the flashback, so the third option is offered (the fourth is `once`).
        assert_eq!(view.choices, vec!["Tell the truth", "Lie", "Tell her everything", "Look at the city first"]);
        let dialogue = &view.dialogue.as_ref().unwrap().text;
        assert_eq!(dialogue, "Remember that day on the rooftop? Her voice is quieter now.");

        let r = choice_rects(3)[2];
        let picked = view.choice_at(r.x + 5.0, r.y + 5.0).unwrap();
        view.apply(&session.input(Input::Choose(picked)));
        assert_eq!(view.dialogue.as_ref().unwrap().text, "You remembered all of it?");
        // `::move` slides her to the centre; she is still the only sprite
        assert_eq!(view.sprites.len(), 1);
        assert!(close(view.sprites[0].x, 0.5));

        // on to the `::wait` before the ending, which a press skips
        view.apply(&session.input(Input::Advance));
        assert!(view.press_advance());
        view.apply(&session.input(Input::Advance));
        assert_eq!(view.dialogue.as_ref().unwrap().text, "I'm glad it was you. I really am.");
        assert!(!view.draw_list(&all_1080p, None).is_empty());
    }
}
