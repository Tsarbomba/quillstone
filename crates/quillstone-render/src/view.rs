//! What is on screen right now, and the backend-neutral list of things to draw.
//!
//! Feed it the runtime's [`Cmd`]s with [`View::apply`], call [`View::tick`] every
//! frame, and render [`View::draw_list`]. No GPU, window or fonts involved.
//!
//! The view owns only *presentation*: fade progress, sprite slides, the typewriter.
//! What is actually in the scene lives in the runtime's `State`, and a rollback
//! sends `Cmd::ClearScene` + the scene again, so the two never drift apart.

use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap, VecDeque};

use quillstone_runtime::{Camera, Cmd, Spot, SpriteStep};
use quillstone_script::markup::{rich, Rich};
pub use quillstone_script::markup::TextRun;
use quillstone_script::{anim_id, Anim, Axis, Ease, Filter, Fit, Imagemap, Vignette, Wipe};

use crate::fade::Fade;
use crate::pixels::vignette_zoom;
use crate::filter::ColorMatrix;
use crate::hud::{HudLayer, HudLook};
use crate::particles::ParticleLayer;
use crate::layout::{clip, estimate_lines, is_wide, line_height, one_row, place, Rect, Size, FULL_UV, VIRTUAL};
use crate::page::{screen_page, PageLine};
use crate::theme::Theme;

/// The dialogue box, in virtual coordinates.
/// This is the default; a theme may move it (`textbox_rect`, see [`textbox_rect`]).
pub const TEXTBOX: Rect = Rect::new(160.0, 780.0, 1600.0, 260.0);

/// Height of a sprite at `scale = 1`, in virtual pixels. Sprites stand on the
/// bottom edge of the screen (the textbox draws over their lower part).
pub const SPRITE_HEIGHT: f32 = 940.0;

/// Thickness of the outline around a hovered hotspot.
/// Seconds a sprite takes to dim or brighten when the speaker changes.
const SHADE_TIME: f32 = 0.25;
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
    /// Each line centred across the rect, and the whole text centred down it (buttons).
    Center,
    /// Each line centred across the rect, the text starting at the top (centred dialogue).
    CenterX,
    /// Each line against the right edge of the rect, the text starting at the top.
    Right,
}

/// A thin border round text's letters: `size` px wide (virtual px; window px in a [`crate::TextJob`]).
/// The border's opacity follows the text's.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Outline {
    pub size: f32,
    pub color: [f32; 4],
}

/// One thing for a backend to draw. Coordinates are virtual (1920x1080);
/// draw in list order, back to front.
#[derive(Debug, Clone, PartialEq)]
pub enum Draw {
    /// `shade` multiplies the picture's colour (1 as it is, less is darker; a dimmed or tinted sprite). `blur` is a
    /// blur radius in virtual pixels (0 is sharp). `additive`: drawn as light (`dst + src`) instead of source-over.
    Image { src: String, dest: Rect, uv: Rect, alpha: f32, shade: [f32; 3], blur: f32, additive: bool },
    Rect { rect: Rect, color: [f32; 4] },
    /// Dark edges: the vignette picture (white, alpha rising outwards) over `rect`, shown through the window
    /// `zoom` of it across (see [`crate::pixels::vignette_zoom`]) and multiplied by `color`.
    Vignette { rect: Rect, color: [f32; 4], zoom: f32 },
    /// From here on the pictures that follow (the stage) get this colour matrix; `None` ends it.
    Filter(Option<ColorMatrix>),
    /// The pictures that follow (the new background) appear where this greyscale picture is dark first: `(mask picture,
    /// reveal)` with `reveal` from [`crate::pixels::mask_reveal`]; `None` ends it. The mask is laid over the stage.
    Mask(Option<(String, f32)>),
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
        outline: Option<Outline>,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub struct Background {
    pub src: String,
    /// Transition duration in seconds (0 = instant).
    pub fade: f32,
    /// Reveal the picture from one side instead of fading it in.
    pub wipe: Option<Wipe>,
    /// Reveal the picture following this greyscale picture (dark first) instead of fading it in.
    pub mask: Option<String>,
    /// A video background plays its own sound.
    pub sound: bool,
    /// Blur radius in stage pixels (0 is sharp).
    pub blur: f32,
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

    /// Opacity: rises with the transition, except for a wipe or a mask (which are opaque).
    pub fn alpha(&self) -> f32 {
        if self.wipe.is_some() || self.mask.is_some() {
            1.0
        } else {
            self.progress()
        }
    }

    /// The mask picture and its threshold while the mask transition runs; `None` otherwise.
    pub fn masked(&self) -> Option<(String, f32)> {
        let m = self.mask.as_ref().filter(|_| self.progress() < 1.0)?;
        Some((m.clone(), crate::pixels::mask_reveal(self.progress())))
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

/// One layer of a layered sprite. It crossfades by itself when its picture changes.
#[derive(Debug, Clone, PartialEq)]
pub struct LayerView {
    pub name: String,
    /// The picture (a file, or `anim:<id>`); empty when the layer is hidden.
    pub src: String,
    /// The outgoing picture while this layer crossfades.
    pub previous_src: Option<String>,
    fade: f32,
    elapsed: f32,
    anim_time: f32,
    previous_anim_time: f32,
}

impl LayerView {
    fn new(name: &str, src: &str, fade: f32) -> LayerView {
        LayerView {
            name: name.to_string(),
            src: src.to_string(),
            previous_src: None,
            fade,
            elapsed: 0.0,
            anim_time: 0.0,
            previous_anim_time: 0.0,
        }
    }

    /// How far this layer's own crossfade has come (1 when it is not fading).
    pub fn alpha(&self) -> f32 {
        if self.fade <= 0.0 { 1.0 } else { (self.elapsed / self.fade).clamp(0.0, 1.0) }
    }

    fn tick(&mut self, dt: f32, animate: bool) {
        self.elapsed += dt;
        if animate {
            self.anim_time += dt;
            self.previous_anim_time += dt;
        }
        if self.elapsed >= self.fade {
            self.previous_src = None;
        }
    }
}

/// A sprite on stage, with its fade and slide animations.
#[derive(Debug, Clone, PartialEq)]
pub struct SpriteView {
    pub id: String,
    pub src: String,
    /// The outgoing pose while a pose change crossfades.
    pub previous_src: Option<String>,
    /// The layers of a layered sprite, back to front (`src` is then empty); empty for a plain sprite.
    pub layers: Vec<LayerView>,
    /// The outgoing layers (picture, its animation clock) while a layered sprite turns into a plain pose.
    previous_layers: Vec<(String, f32)>,
    /// Target position, 0..1 across the screen (the sprite's centre).
    pub x: f32,
    /// Target height of the bottom edge, as a fraction of the screen height.
    pub y: f32,
    /// Drawing order among sprites (higher is in front).
    pub z: i32,
    /// Target size.
    pub scale: f32,
    /// Mirrored left to right (from the start of a move that switches it).
    pub flip: bool,
    /// Target stretch along one axis, on top of `scale`.
    pub xscale: f32,
    pub yscale: f32,
    /// Target opacity, and pixel offset (of the 1920x1080 stage) added after placing by `x`, `y`.
    pub alpha_to: f32,
    pub dx: f32,
    pub dy: f32,
    /// Target blur radius in stage pixels.
    pub blur: f32,
    /// Target colour the picture is multiplied by (white is none).
    pub tint: [f32; 3],
    /// Drawn with additive blending (`blend=add`).
    pub additive: bool,
    pub hiding: bool,
    fade: f32,
    elapsed: f32,
    /// Where the current leg of a move started: x, y, scale, xscale, yscale, alpha, dx, dy, blur, tint red, green, blue.
    from: [f32; 12],
    move_time: f32,
    /// Seconds into the current leg; negative while it still waits (`wait=`).
    move_elapsed: f32,
    move_ease: Ease,
    /// A looping move: the other end of the oscillation (same order as `from`);
    /// the sprite rests at `x`, `y`, ... The current leg heads for it while `out`.
    loop_far: Option<[f32; 12]>,
    /// A sequence of moves: the legs still to run after the current one, every leg of it, and whether the
    /// legs start over once the last is done (`loop` with `then`).
    queue: VecDeque<SpriteStep>,
    legs: Vec<SpriteStep>,
    cycle: bool,
    out: bool,
    /// Seconds since this sprite's picture started being shown (an animation's clock),
    /// and the same for the outgoing picture of a pose change.
    anim_time: f32,
    previous_anim_time: f32,
    /// How bright it is drawn, 1 as it is: dimmed while someone else speaks (`speaker_highlight`).
    shade: f32,
}

impl SpriteView {
    fn new(id: &str, (src, layers): (&str, &[(String, String)]), (x, y): (f32, f32), z: i32, look: Look, fx: [f32; 7], fade: f32) -> SpriteView {
        SpriteView {
            id: id.to_string(),
            src: src.to_string(),
            previous_src: None,
            layers: layers.iter().map(|(n, p)| LayerView::new(n, p, 0.0)).collect(),
            previous_layers: Vec::new(),
            x,
            y,
            z,
            scale: look.scale,
            flip: look.flip,
            xscale: look.xscale,
            yscale: look.yscale,
            alpha_to: fx[0],
            dx: fx[1],
            dy: fx[2],
            blur: fx[3],
            tint: [fx[4], fx[5], fx[6]],
            additive: look.additive,
            hiding: false,
            fade,
            elapsed: 0.0,
            from: [x, y, look.scale, look.xscale, look.yscale, fx[0], fx[1], fx[2], fx[3], fx[4], fx[5], fx[6]],
            move_time: 0.0,
            move_elapsed: 0.0,
            move_ease: Ease::Linear,
            loop_far: None,
            queue: VecDeque::new(),
            legs: Vec::new(),
            cycle: false,
            out: false,
            anim_time: 0.0,
            previous_anim_time: 0.0,
            shade: 1.0,
        }
    }

    /// Take the layers a `ShowSprite` names: a layer whose picture changed crossfades on its own
    /// (over `fade`), the others stay as they are. Empty `layers` makes the sprite a plain one.
    fn set_layers(&mut self, layers: &[(String, String)], fade: f32) {
        let old = std::mem::take(&mut self.layers);
        if layers.is_empty() {
            let keep = fade > 0.0;
            self.previous_layers = old.into_iter().filter(|l| keep && !l.src.is_empty()).map(|l| (l.src, l.anim_time)).collect();
            return;
        }
        self.previous_layers.clear();
        self.layers = layers
            .iter()
            .map(|(name, src)| match old.iter().find(|l| l.name == *name) {
                Some(o) if o.src == *src => o.clone(),
                Some(o) => LayerView {
                    previous_src: (fade > 0.0 && !o.src.is_empty()).then(|| o.src.clone()),
                    previous_anim_time: o.anim_time,
                    ..LayerView::new(name, src, fade)
                },
                None => LayerView::new(name, src, fade),
            })
            .collect();
    }

    /// Every picture this sprite draws or is fading out: plain, outgoing, and each layer's.
    fn pictures(&self) -> impl Iterator<Item = &str> {
        let layers = self.layers.iter().flat_map(|l| std::iter::once(l.src.as_str()).chain(l.previous_src.as_deref()));
        let plain = std::iter::once(self.src.as_str()).chain(self.previous_src.as_deref());
        plain.chain(layers).chain(self.previous_layers.iter().map(|(p, _)| p.as_str()))
    }

    /// Where the current leg of a move (or loop) is heading: x, y, scale, xscale, yscale, alpha, dx, dy, blur, tint red, green, blue.
    fn target(&self) -> [f32; 12] {
        match (&self.loop_far, self.out) {
            (Some(far), true) => *far,
            _ => self.rest(),
        }
    }

    /// The resting pose: x, y, scale, xscale, yscale, alpha, dx, dy, blur, tint red, green, blue.
    fn rest(&self) -> [f32; 12] {
        [self.x, self.y, self.scale, self.xscale, self.yscale, self.alpha_to, self.dx, self.dy, self.blur, self.tint[0], self.tint[1], self.tint[2]]
    }

    /// The values the sprite rests at for good: where a looping sequence's last leg ends, else `rest`.
    fn home(&self) -> [f32; 12] {
        match self.legs.last() {
            Some(last) if self.cycle => last.values(),
            _ => self.rest(),
        }
    }

    /// Whether a leg of a move is running (or waiting to start).
    fn moving(&self) -> bool {
        self.move_time > 0.0 || self.move_elapsed < 0.0
    }

    /// Start a leg of a move from `from`; the sprite's own values become the leg's.
    fn start_leg(&mut self, from: [f32; 12], to: &SpriteStep) {
        self.from = from;
        (self.x, self.y, self.scale, self.xscale, self.yscale, self.alpha_to, self.dx, self.dy, self.blur, self.tint) =
            (to.x, to.y, to.scale, to.xscale, to.yscale, to.alpha, to.dx, to.dy, to.blur, to.tint);
        self.move_time = to.time;
        self.move_elapsed = -to.wait;
        self.move_ease = to.ease;
    }

    /// Every sequence leg ever started stops: the next move starts a sequence of its own.
    fn end_sequence(&mut self) {
        self.queue.clear();
        self.legs.clear();
        self.cycle = false;
    }

    /// Whether the sprite oscillates (a looping move or looping sequence is running).
    pub fn is_looping(&self) -> bool {
        self.loop_far.is_some() || self.cycle
    }

    /// The value `k` (0 x, 1 y, 2 scale, 3 xscale, 4 yscale, 5 alpha, 6 dx, 7 dy, 8 blur, 9-11 tint) right now.
    fn now(&self, k: usize) -> f32 {
        let (from, to) = (self.from[k], self.target()[k]);
        from + (to - from) * self.move_progress()
    }

    /// Progress through the current move, eased (1 when not moving).
    fn move_progress(&self) -> f32 {
        if self.move_elapsed < 0.0 {
            0.0
        } else if self.move_time > 0.0 {
            self.move_ease.apply(self.move_elapsed / self.move_time)
        } else {
            1.0
        }
    }

    /// All the animated values right now.
    fn here(&self) -> [f32; 12] {
        std::array::from_fn(|k| self.now(k))
    }

    /// How big the sprite is right now (growing towards `scale` during a move).
    pub fn current_scale(&self) -> f32 {
        self.now(2)
    }

    /// The horizontal stretch right now (moving towards `xscale` during a move).
    pub fn current_xscale(&self) -> f32 {
        self.now(3)
    }

    /// The vertical stretch right now (moving towards `yscale` during a move).
    pub fn current_yscale(&self) -> f32 {
        self.now(4)
    }

    /// Size, stretch and mirroring right now.
    fn look(&self) -> Look {
        Look { scale: self.current_scale(), xscale: self.current_xscale(), yscale: self.current_yscale(), flip: self.flip, additive: self.additive }
    }

    /// How opaque the sprite is right now: the fade in or out of showing and hiding times `alpha=` (which moves can animate).
    pub fn opacity(&self) -> f32 {
        self.alpha() * self.now(5).clamp(0.0, 1.0)
    }

    /// The pixel offset (of the 1920x1080 stage) right now.
    pub fn current_offset(&self) -> (f32, f32) {
        (self.now(6), self.now(7))
    }

    /// The blur radius in stage pixels right now.
    pub fn current_blur(&self) -> f32 {
        self.now(8).clamp(0.0, *quillstone_script::SPRITE_BLUR.end())
    }

    /// The colour the picture is multiplied by right now (eased per channel).
    pub fn current_tint(&self) -> [f32; 3] {
        std::array::from_fn(|k| self.now(9 + k).clamp(0.0, 1.0))
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
        self.now(0)
    }

    /// Where the bottom edge is right now (sliding towards `y` while a move runs).
    pub fn current_y(&self) -> f32 {
        self.now(1)
    }

    /// Advance by `dt`. With `animate` off (settling a skipped scene) a loop and the
    /// picture's clock stand still: they have no end to settle at.
    fn tick(&mut self, dt: f32, animate: bool) {
        self.elapsed += dt;
        if animate {
            self.anim_time += dt;
            self.previous_anim_time += dt;
            for (_, t) in &mut self.previous_layers {
                *t += dt;
            }
        }
        for l in &mut self.layers {
            l.tick(dt, animate);
        }
        if self.loop_far.is_some() {
            if animate && self.move_time > 0.0 {
                self.move_elapsed += dt;
                if self.move_elapsed >= self.move_time {
                    // Every leg that ended turns the sprite round; the rest is progress into the next.
                    let legs = (self.move_elapsed / self.move_time).floor();
                    self.move_elapsed = (self.move_elapsed - legs * self.move_time).max(0.0);
                    if legs % 2.0 == 1.0 {
                        self.out = !self.out;
                    }
                    self.from = if self.out { self.rest() } else { self.loop_far.unwrap_or_default() };
                }
            }
        } else if self.moving() || !self.queue.is_empty() {
            // Legs of a sequence that end within `dt` pass their leftover time on to the next.
            let mut left = dt;
            for _ in 0..10_000 {
                if self.moving() {
                    let need = self.move_time - self.move_elapsed;
                    if left < need {
                        self.move_elapsed += left;
                        break;
                    }
                    left -= need;
                    self.move_time = 0.0;
                    self.move_elapsed = 0.0;
                    self.from = self.rest();
                }
                if self.queue.is_empty() && self.cycle && animate && self.legs.iter().any(|l| l.time + l.wait > 0.0) {
                    self.queue = self.legs.iter().copied().collect();
                }
                let Some(next) = self.queue.pop_front() else { break };
                let from = self.rest();
                self.start_leg(from, &next);
            }
        }
        if !self.hiding && self.elapsed >= self.fade {
            self.previous_src = None;
            self.previous_layers.clear();
        }
    }

    fn is_gone(&self) -> bool {
        self.hiding && self.alpha() <= 0.0
    }
}

/// How big a sprite is drawn and whether it is mirrored: `scale` times the per-axis stretch.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Look {
    pub scale: f32,
    pub xscale: f32,
    pub yscale: f32,
    pub flip: bool,
    pub additive: bool,
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
        let mut d = Dialogue { speaker, color, text, runs, pauses, hold: 0.0, total, revealed: 0.0, auto_advance: no_wait };
        if instant {
            d.show_text();
        }
        d
    }

    /// Instant text: every word is shown at once, but a pause at the very end
    /// still counts (so `Well...{w=3}{nw}` waits).
    fn show_text(&mut self) {
        self.revealed = self.total as f32;
        let total = self.total;
        self.pauses.retain(|&(at, _)| at >= total);
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

    /// Everything typed and no pause (`{w=..}`) left to sit through.
    pub fn is_complete(&self) -> bool {
        self.visible_chars() >= self.total && self.pauses.is_empty() && self.hold <= 0.0
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
    axis: Axis,
    elapsed: f32,
}

impl Shake {
    /// How strong the shake is now: eases in over a short attack, then dies away
    /// linearly, so the offset and the cover zoom both start and end at zero.
    fn envelope(&self) -> f32 {
        let t = self.elapsed;
        if t >= self.time || self.time <= 0.0 {
            return 0.0;
        }
        let attack = (self.time / 4.0).min(0.1);
        self.strength.abs() * (t / attack).min(1.0) * (1.0 - t / self.time)
    }

    /// The zoom about the screen centre that keeps the shifted stage covering the
    /// whole screen. Only the shaken axis needs covering: the largest offset across
    /// is `envelope`; down it is `envelope` for a vertical shake, 0.6 of it in a
    /// shake along both axes.
    fn cover_zoom(&self) -> f32 {
        let e = self.envelope();
        let across = 1.0 + 2.0 * e / VIRTUAL.w;
        let down = |k: f32| 1.0 + 2.0 * k * e / VIRTUAL.h;
        match self.axis {
            Axis::X => across,
            Axis::Y => down(1.0),
            Axis::Both => across.max(down(0.6)),
        }
    }

    /// The stage's offset now: a fast wobble that dies away (deterministic).
    fn offset(&self) -> (f32, f32) {
        let t = self.elapsed;
        let fade = self.envelope();
        if fade == 0.0 {
            return (0.0, 0.0);
        }
        let tau = std::f32::consts::TAU;
        let across = fade * (t * tau * 13.0).sin();
        let down = |k: f32| fade * k * (t * tau * 17.0 + 1.3).sin();
        match self.axis {
            Axis::X => (across, 0.0),
            Axis::Y => (0.0, down(1.0)),
            Axis::Both => (across, down(0.6)),
        }
    }
}

/// A full-screen flash in progress: full strength at the start, gone after `time`.
#[derive(Debug, Clone, PartialEq)]
struct Flash {
    time: f32,
    color: [f32; 3],
    elapsed: f32,
}

impl Flash {
    fn alpha(&self) -> f32 {
        (1.0 - self.elapsed / self.time).clamp(0.0, 1.0)
    }
}

/// A corner message in progress (`::notify`): fades in, stays, fades out, gone after `time`.
#[derive(Debug, Clone, PartialEq)]
struct Notice {
    text: String,
    time: f32,
    elapsed: f32,
}

impl Notice {
    fn alpha(&self) -> f32 {
        (self.elapsed / 0.2).min((self.time - self.elapsed) / 0.5).clamp(0.0, 1.0)
    }
}

/// The stage covered with a colour (`::fade{out}`), fading to full cover or away.
#[derive(Debug, Clone, PartialEq)]
struct Cover {
    color: [f32; 3],
    /// Target 1 while fading out or covered, 0 while fading in.
    fade: Fade,
}

/// A value that can ease: the tint, vignette and filter over the stage.
trait Eased: Copy + PartialEq {
    fn mix(a: Self, b: Self, t: f32) -> Self;
    /// The same look at nothing: what it eases from when none was showing, and to when it is removed.
    fn cleared(self) -> Self;
    /// Whether it changes the picture.
    fn shown(&self) -> bool;
}

fn lerp(x: f32, y: f32, t: f32) -> f32 {
    x + (y - x) * t
}

/// The colour wash (`::tint`): colour and opacity.
impl Eased for ([f32; 3], f32) {
    fn mix(a: Self, b: Self, t: f32) -> Self {
        ([lerp(a.0[0], b.0[0], t), lerp(a.0[1], b.0[1], t), lerp(a.0[2], b.0[2], t)], lerp(a.1, b.1, t))
    }
    fn cleared(self) -> Self {
        (self.0, 0.0)
    }
    fn shown(&self) -> bool {
        self.1 > 0.0
    }
}

/// The dark edges (`::vignette`).
impl Eased for Vignette {
    fn mix(a: Self, b: Self, t: f32) -> Self {
        Vignette {
            color: [lerp(a.color[0], b.color[0], t), lerp(a.color[1], b.color[1], t), lerp(a.color[2], b.color[2], t)],
            strength: lerp(a.strength, b.strength, t),
            size: lerp(a.size, b.size, t),
        }
    }
    fn cleared(self) -> Self {
        Vignette { strength: 0.0, ..self }
    }
    fn shown(&self) -> bool {
        self.strength > 0.0
    }
}

/// The colour filter (`::filter`).
impl Eased for Filter {
    fn mix(a: Self, b: Self, t: f32) -> Self {
        Filter {
            gray: lerp(a.gray, b.gray, t),
            sepia: lerp(a.sepia, b.sepia, t),
            brightness: lerp(a.brightness, b.brightness, t),
            contrast: lerp(a.contrast, b.contrast, t),
            saturation: lerp(a.saturation, b.saturation, t),
        }
    }
    fn cleared(self) -> Self {
        Filter::default()
    }
    fn shown(&self) -> bool {
        *self != Filter::default()
    }
}

/// An effect over the stage easing from where it was to `to`.
#[derive(Debug, Clone, PartialEq)]
struct Fading<T> {
    from: T,
    to: T,
    /// Its `alpha` is the progress: 0 at the start of the ease, 1 when it has arrived.
    progress: Fade,
}

impl<T: Eased> Fading<T> {
    fn current(&self) -> T {
        T::mix(self.from, self.to, self.progress.alpha)
    }

    /// What shows right now, if anything.
    fn showing(slot: &Option<Self>) -> Option<T> {
        slot.as_ref().map(Self::current).filter(T::shown)
    }

    /// Ease `slot` to `set` (`None`: to nothing) from what it is now, or from clear.
    fn set(slot: &mut Option<Self>, set: Option<T>, fade: f32) {
        let now = Self::showing(slot);
        let Some(to) = set.or(now.map(T::cleared)) else { return };
        let from = now.unwrap_or(to.cleared());
        let rate = Fade::rate_over(fade);
        *slot = Some(Fading { from, to, progress: Fade { alpha: if rate > 0.0 { 0.0 } else { 1.0 }, target: 1.0, rate } });
        Self::drop_cleared(slot);
    }

    fn drop_cleared(slot: &mut Option<Self>) {
        if slot.as_ref().is_some_and(|e| e.progress.done() && !e.to.shown()) {
            *slot = None;
        }
    }

    fn tick(slot: &mut Option<Self>, dt: f32) {
        if let Some(e) = slot {
            e.progress.tick(dt);
        }
        Self::drop_cleared(slot);
    }

    fn busy(slot: &Option<Self>) -> bool {
        slot.as_ref().is_some_and(|e| !e.progress.done())
    }
}

#[derive(Debug, Clone)]
pub struct View {
    pub background: Option<Background>,
    /// The outgoing background while a crossfade is running.
    pub previous_background: Option<String>,
    /// The blur the outgoing background had.
    previous_blur: f32,
    pub sprites: Vec<SpriteView>,
    pub images: Vec<InlineImage>,
    pub dialogue: Option<Dialogue>,
    /// The lines of the full-screen page (`::page`), the newest last (it is also `dialogue`, which types out);
    /// empty with the textbox.
    pub page: Vec<PageLine>,
    pub choices: Vec<String>,
    /// Alongside `choices`: `Some(reason)` for a greyed option that can't be picked (the reason may be empty).
    pub locked: Vec<Option<String>>,
    /// Clickable regions over the background (`::hotspots`), in the order the runtime lists them.
    pub hotspots: Vec<Spot>,
    /// The imagemap pictures of the hotspot block on screen, if it has them.
    pub imagemap: Option<Imagemap>,
    pub video: Option<VideoView>,
    pub finished: bool,
    camera: CameraMove,
    shake: Option<Shake>,
    flash: Option<Flash>,
    notice: Option<Notice>,
    cover: Option<Cover>,
    hud: HudLayer,
    particles: ParticleLayer,
    tint: Option<Fading<([f32; 3], f32)>>,
    vignette: Option<Fading<Vignette>>,
    filter: Option<Fading<Filter>>,
    /// `::textbox{hide}` is in effect, and how visible the box is now (0..1, fading).
    box_hidden: bool,
    box_fade: Fade,
    /// The player hid the interface (the `U` key); see [`View::set_hide_ui`].
    hide_ui: bool,
    /// Seconds left of a `::wait`, if one is running.
    wait: Option<f32>,
    chars_per_sec: f32,
    /// The player's text size setting (1.0..=1.5): dialogue and the other text sized from the theme's `text_size`.
    text_scale: f32,
    theme: Theme,
    /// The id of the character whose line is on screen, if it is one (see [`View::set_speaker`]).
    speaker_id: Option<String>,
    /// The portrait picture of that character, drawn by the textbox while their line is up.
    speaker_portrait: Option<String>,
    /// `Some(k)`: sprites are drawn at their picture's pixel size times `k` (front matter `sprite_size: pixels`).
    art_scale: Option<f32>,
    /// The size of each picture a sprite is drawn with (by path, so layers and crossfading poses differ), noted by every draw while a hotspot follows a sprite: hit-testing
    /// has no way to ask for sizes, and uses the same geometry as drawing (`push_sprite`).
    sprite_sizes: RefCell<HashMap<String, Size>>,
    /// The script's frame animations (a picture `anim:<id>` plays one).
    anims: BTreeMap<String, Anim>,
    /// Seconds since the background / the outgoing background started being shown.
    bg_clock: f32,
    previous_bg_clock: f32,
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
            previous_blur: 0.0,
            sprites: Vec::new(),
            images: Vec::new(),
            dialogue: None,
            page: Vec::new(),
            choices: Vec::new(),
            locked: Vec::new(),
            hotspots: Vec::new(),
            imagemap: None,
            video: None,
            finished: false,
            camera: CameraMove { from: Camera::default(), to: Camera::default(), time: 0.0, elapsed: 0.0, ease: Ease::Linear },
            shake: None,
            flash: None,
            notice: None,
            cover: None,
            hud: HudLayer::default(),
            particles: ParticleLayer::default(),
            tint: None,
            vignette: None,
            filter: None,
            box_hidden: false,
            box_fade: Fade::at(1.0),
            hide_ui: false,
            wait: None,
            chars_per_sec: DEFAULT_CHARS_PER_SEC,
            text_scale: 1.0,
            theme: Theme::default(),
            speaker_id: None,
            speaker_portrait: None,
            art_scale: None,
            sprite_sizes: RefCell::default(),
            anims: BTreeMap::new(),
            bg_clock: 0.0,
            previous_bg_clock: 0.0,
        }
    }

    /// Draw sprites at their picture's pixel size times `art_scale` (`Program::art_scale`); `None` is the fixed height.
    pub fn with_art_scale(mut self, art_scale: Option<f32>) -> View {
        self.art_scale = art_scale;
        self
    }

    /// Know the script's frame animations (`anim:<id>` pictures play them). Clocks
    /// are visual state: every animation starts at its first frame when it appears.
    pub fn with_anims(mut self, anims: &BTreeMap<String, Anim>) -> View {
        self.anims = anims.clone();
        self
    }

    /// The picture file to draw for `src` at `t` seconds into it: the file itself, or
    /// the frame of an animation (nothing for one this view doesn't know).
    fn picture<'a>(&'a self, src: &'a str, t: f32) -> &'a str {
        match anim_id(src) {
            Some(id) => self.anims.get(id).map_or("", |a| a.frame_at(t)),
            None => src,
        }
    }

    /// Whether `src` is an animation that can still change `t` seconds in.
    fn playing(&self, src: &str, t: f32) -> bool {
        anim_id(src).and_then(|id| self.anims.get(id)).is_some_and(|a| a.running(t))
    }

    /// The ids of the animations on stage (background, sprites, outgoing pictures), each once.
    pub fn shown_animations(&self) -> Vec<&str> {
        let srcs = self
            .background
            .iter()
            .map(|b| b.src.as_str())
            .chain(self.previous_background.as_deref())
            .chain(self.sprites.iter().flat_map(SpriteView::pictures));
        let mut out: Vec<&str> = Vec::new();
        for id in srcs.filter_map(anim_id) {
            if !out.contains(&id) {
                out.push(id);
            }
        }
        out
    }

    /// Whether any picture on stage changes by itself: a multi-frame animation that
    /// is still running (a one-shot one that reached its last frame is still), a
    /// sprite in a looping move, or ambient particles.
    pub fn animating(&self) -> bool {
        let sprite = |s: &SpriteView| {
            s.is_looping()
                || s.moving() // a leg may be waiting to start, with nothing yet changing on screen
                || !s.queue.is_empty()
                || self.playing(&s.src, s.anim_time)
                || s.previous_src.as_deref().is_some_and(|p| self.playing(p, s.previous_anim_time))
                || s.layers.iter().any(|l| {
                    self.playing(&l.src, l.anim_time) || l.previous_src.as_deref().is_some_and(|p| self.playing(p, l.previous_anim_time))
                })
                || s.previous_layers.iter().any(|(p, t)| self.playing(p, *t))
        };
        self.background.as_ref().is_some_and(|b| self.playing(&b.src, self.bg_clock))
            || self.previous_background.as_deref().is_some_and(|p| self.playing(p, self.previous_bg_clock))
            || self.sprites.iter().any(sprite)
            || self.particles.active()
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

    /// Tell the view whose line is on screen (a character id; `None` for narration or a name that is no
    /// character). With the theme's `speaker_highlight`, the sprites of the others dim. `snap` sets the
    /// brightness at once (a redraw); otherwise it eases, except for a sprite that has only just appeared.
    pub fn set_speaker(&mut self, id: Option<String>, snap: bool) {
        self.speaker_id = id;
        for i in 0..self.sprites.len() {
            if snap || self.sprites[i].elapsed == 0.0 {
                self.sprites[i].shade = self.shade_target(&self.sprites[i].id);
            }
        }
    }

    /// The portrait picture of the character whose line is on screen (`None`: narration, or no portrait).
    pub fn set_portrait(&mut self, src: Option<String>) {
        self.speaker_portrait = src;
    }

    /// The brightness the sprite `id` is heading for: dimmed when a character who is on stage speaks and it is not them.
    fn shade_target(&self, id: &str) -> f32 {
        match &self.speaker_id {
            Some(who) if who != id && self.sprites.iter().any(|s| s.id == *who && !s.hiding) => self.theme.speaker_highlight,
            _ => 1.0,
        }
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

    /// Scale the theme's text size by the player's setting (1.0 is the theme's own).
    pub fn with_text_scale(mut self, scale: f32) -> View {
        self.text_scale = scale;
        self
    }

    pub fn set_text_scale(&mut self, scale: f32) {
        self.text_scale = scale;
    }

    /// The size to draw dialogue `d` at: the theme's times the text scale, or when the line doesn't fit the
    /// textbox that big, the largest 10% step down that does (the theme's own size at worst, as `check` judged).
    fn dialogue_size(&self, d: &Dialogue, has_name: bool) -> f32 {
        let body = dialogue_body(&self.theme, has_name);
        fitted_size(&self.theme, self.text_scale, |size| {
            crate::estimate_lines_styled(&d.text, &d.runs, size, body.w) <= (body.h / line_height(size)).floor() as usize
        })
    }

    /// Apply one batch of commands from the runtime.
    pub fn apply(&mut self, cmds: &[Cmd]) {
        // A new batch means the player acted: per-line extras go away. (A redraw
        // after rollback re-sends them, since the runtime tracks them.)
        self.images.clear();
        self.choices.clear();
        self.locked.clear();
        self.hotspots.clear();
        self.imagemap = None;
        self.video = None;
        self.wait = None;
        // Only the latest batch can say the story is over (matters after rollback).
        self.finished = false;
        // `::fade{out}` waits for its end, so a new batch means it is over (a click or
        // skip cut it short): the cover is complete. A fade in carries on by itself.
        self.finish_fade_out();
        // A redraw (rollback, later: loading a save) returns to a line the player
        // has already read, so it is shown whole instead of typed out again.
        let redraw = cmds.first() == Some(&Cmd::ClearScene);
        self.apply_each(cmds, redraw);
    }

    /// Apply commands one by one without starting a new batch: the images, video, wait and
    /// `finished` of the current line stay. `redraw` shows a line whole instead of typing it
    /// out. For showing some of what is on screen again (a language change).
    pub fn apply_each(&mut self, cmds: &[Cmd], redraw: bool) {
        for cmd in cmds {
            match cmd {
                Cmd::ClearScene => {
                    if !self.page.is_empty() {
                        self.page.clear();
                        self.dialogue = None; // a page line was the line; the redraw sends the one that is
                    }
                    self.background = None;
                    self.previous_background = None;
                    self.sprites.clear();
                    self.set_camera(Camera::default(), 0.0, Ease::Linear);
                    self.shake = None;
                    self.flash = None;
                    self.notice = None;
                    self.cover = None;
                    self.hud.clear_all();
                    self.particles.clear();
                    self.tint = None;
                    self.vignette = None;
                    self.filter = None;
                    self.box_hidden = false;
                    self.box_fade = Fade::at(1.0);
                }
                Cmd::Textbox { hidden, fade } => {
                    self.box_hidden = *hidden;
                    self.box_fade.target = if *hidden { 0.0 } else { 1.0 };
                    self.box_fade.rate = Fade::rate_over(*fade);
                    if *fade <= 0.0 {
                        self.box_fade.alpha = self.box_fade.target;
                    }
                }
                Cmd::SetBackground { src, fade, wipe, sound, blur, mask } => self.set_background(src, *fade, *wipe, *sound, *blur, mask.clone()),
                Cmd::ShowSprite { id, src, layers, x, y, z, scale, flip, xscale, yscale, alpha, dx, dy, blur, tint, additive, fade } => {
                    let look = Look { scale: *scale, xscale: *xscale, yscale: *yscale, flip: *flip, additive: *additive };
                    self.show_sprite(id, (src, layers), (*x, *y), *z, look, [*alpha, *dx, *dy, *blur, tint[0], tint[1], tint[2]], *fade)
                }
                Cmd::HideSprite { id, fade } => self.hide_sprite(id, *fade),
                Cmd::LoopSprite { id, flip, to } => {
                    if let Some(s) = self.sprites.iter_mut().find(|s| s.id == *id && !s.hiding) {
                        // The first leg glides from wherever the sprite is to the far values; the
                        // resting values (x, y, ...) stay where they are.
                        s.end_sequence();
                        s.from = s.here();
                        s.loop_far = Some(to.values());
                        s.out = true;
                        s.flip = *flip;
                        s.move_time = to.time;
                        s.move_elapsed = 0.0;
                        s.move_ease = to.ease;
                    }
                }
                Cmd::MoveSprite { id, flip, to, queued } => {
                    if let Some(s) = self.sprites.iter_mut().find(|s| s.id == *id && !s.hiding) {
                        let here = s.here(); // before a loop ends: it decides where the sprite is
                        s.loop_far = None; // a plain move ends a loop
                        s.out = false;
                        if *queued && (s.moving() || !s.queue.is_empty()) {
                            s.queue.push_back(*to);
                            s.legs.push(*to);
                        } else {
                            if !*queued {
                                s.end_sequence();
                            }
                            s.flip = *flip; // mirroring can't be animated: it switches as the move starts
                            s.start_leg(here, to);
                            s.legs.push(*to);
                        }
                    }
                }
                Cmd::CycleSprite { id } => {
                    if let Some(s) = self.sprites.iter_mut().find(|s| s.id == *id && !s.hiding) {
                        s.cycle = true;
                    }
                }
                Cmd::Shake { time, strength, axis } => {
                    self.shake = Some(Shake { time: *time, strength: *strength, axis: *axis, elapsed: 0.0 });
                }
                Cmd::FadeOut { time, color } => {
                    let cover = self.cover.get_or_insert(Cover { color: *color, fade: Fade { alpha: 0.0, target: 1.0, rate: 0.0 } });
                    cover.color = *color;
                    cover.fade.target = 1.0;
                    cover.fade.rate = Fade::rate_over(*time);
                    if *time <= 0.0 {
                        cover.fade.alpha = 1.0;
                    }
                }
                Cmd::FadeIn { time } => {
                    if let Some(cover) = &mut self.cover {
                        cover.fade.target = 0.0;
                        cover.fade.rate = Fade::rate_over(*time);
                    }
                    if *time <= 0.0 {
                        self.cover = None;
                    }
                }
                Cmd::HudShow { .. } | Cmd::HudHide { .. } | Cmd::HudClear { .. } => self.hud.apply(cmd),
                Cmd::Particles { set, fade } => self.particles.apply(set, *fade),
                Cmd::Tint { set, fade } => self.set_tint(set.map(|t| (t.color, t.alpha)), *fade),
                Cmd::Vignette { set, fade } => self.set_vignette(*set, *fade),
                Cmd::Filter { set, fade } => self.set_filter(*set, *fade),
                Cmd::Flash { time, color } => {
                    self.flash = Some(Flash { time: *time, color: *color, elapsed: 0.0 });
                }
                Cmd::Notify { text, time, .. } => {
                    self.notice = Some(Notice { text: text.clone(), time: *time, elapsed: 0.0 });
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
                    if redraw {
                        if let Some(d) = &mut self.dialogue {
                            d.reveal_all();
                        }
                    }
                }
                Cmd::Page { lines } => {
                    if lines.is_empty() && !self.page.is_empty() {
                        self.dialogue = None; // the page's last line goes with it
                    }
                    self.page = lines.iter().map(|l| PageLine::new(l.speaker.clone(), l.color, &l.text)).collect();
                }
                Cmd::ShowChoices { options, locked } => {
                    self.choices = options.clone();
                    self.locked = locked.clone();
                }
                Cmd::ShowHotspots { spots, map } => {
                    self.hotspots = spots.clone();
                    self.imagemap = map.clone();
                }
                Cmd::Wait { seconds } => self.wait = Some(*seconds),
                Cmd::WaitClick => {} // the screen stays as it is; a click is the only way on
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
                Cmd::Stopped { .. } | Cmd::Warning { .. } => {} // for the author and player; the game shows it as a notice
                Cmd::ReplayReached { .. } | Cmd::Unlock { .. } | Cmd::EndReplay => {} // bookkeeping for the session and the game
                // The inventory, shop and text entry are screens of their own (see quillstone-ui).
                Cmd::ShowInventory | Cmd::ShowJournal | Cmd::ShowShop { .. } | Cmd::AskInput { .. } | Cmd::ShowTiming { .. } | Cmd::ShowPress { .. } => {}
                // Audio is not the view's business.
                Cmd::PlayMusic { .. }
                | Cmd::StopMusic { .. }
                | Cmd::PlaySfx { .. }
                | Cmd::PlaySfxLoop { .. }
                | Cmd::StopSfx { .. }
                | Cmd::PlayVoice { .. } => {}
            }
        }
    }

    fn set_background(&mut self, src: &str, fade: f32, wipe: Option<Wipe>, sound: bool, blur: f32, mask: Option<String>) {
        // The same clip keeps playing: only its sound is turned on or off, and a blur that changes
        // crossfades from the old look when there is a `fade` (else it just changes).
        if let Some(b) = self.background.as_mut().filter(|b| b.src == src && (b.blur == blur || fade <= 0.0)) {
            (b.sound, b.blur) = (sound, blur);
            return;
        }
        self.previous_blur = self.background.as_ref().map_or(0.0, |b| b.blur);
        let previous_src = self.background.as_ref().map(|b| b.src.clone());
        self.previous_background = if fade > 0.0 { self.background.take().map(|b| b.src) } else { None };
        self.previous_bg_clock = self.bg_clock;
        if previous_src.as_deref() != Some(src) {
            self.bg_clock = 0.0; // the same clip with a new blur carries on from where it is
        }
        self.background = Some(Background { src: src.to_string(), fade, wipe, mask, sound, blur, elapsed: 0.0 });
    }

    /// Start moving the camera from wherever it is now.
    fn set_camera(&mut self, to: Camera, time: f32, ease: Ease) {
        let from = self.camera.current();
        self.camera = CameraMove { from, to, time, elapsed: 0.0, ease };
    }

    /// Whether `::textbox{hide}` is in effect (the box may still be fading out).
    pub fn textbox_hidden(&self) -> bool {
        self.box_hidden
    }

    /// How visible the dialogue box is: 1 shown, 0 hidden (by the script or by
    /// [`View::set_hide_ui`]), in between while it fades.
    pub fn box_alpha(&self) -> f32 {
        if self.hide_ui {
            0.0
        } else {
            self.box_fade.alpha
        }
    }

    /// The player hides (or brings back) the whole interface: textbox, name plate,
    /// line and quick menu. Choices and hotspots stay (see `quillstone-ui`).
    pub fn set_hide_ui(&mut self, hide: bool) {
        self.hide_ui = hide;
    }

    pub fn hide_ui(&self) -> bool {
        self.hide_ui
    }

    /// Hand over the story's variables: HUD text, bars and conditions read them each time
    /// the draw list is built.
    pub fn set_vars(&mut self, vars: &quillstone_script::Vars) {
        self.hud.set_vars(vars);
    }

    /// The HUD elements, in drawing order.
    pub fn hud(&self) -> &HudLayer {
        &self.hud
    }

    /// End a flash and a corner message that are running (skipping doesn't wait for them).
    pub fn end_flash(&mut self) {
        self.flash = None;
        self.notice = None;
    }

    /// Finish the cover's fades at once (skipping doesn't wait for them): a fade out
    /// is complete, a fade in is gone.
    pub fn end_cover_fade(&mut self) {
        self.hud.end_fades();
        self.finish_fade_out();
        if self.cover.as_ref().is_some_and(|c| c.fade.target == 0.0) {
            self.cover = None;
        }
    }

    fn finish_fade_out(&mut self) {
        if let Some(c) = self.cover.as_mut().filter(|c| c.fade.target == 1.0) {
            c.fade.alpha = 1.0;
        }
    }

    /// Ease the tint to `set` (`None`: to nothing) from what it is now, or from clear.
    fn set_tint(&mut self, set: Option<([f32; 3], f32)>, fade: f32) {
        Fading::set(&mut self.tint, set, fade);
    }

    /// The colour and opacity of the tint right now, if there is one showing.
    pub fn tint(&self) -> Option<([f32; 3], f32)> {
        Fading::showing(&self.tint)
    }

    /// Ease the vignette to `set` (`None`: to nothing) from what it is now, or from clear.
    fn set_vignette(&mut self, set: Option<Vignette>, fade: f32) {
        Fading::set(&mut self.vignette, set, fade);
    }

    /// The dark edges right now, if there are any showing.
    pub fn vignette(&self) -> Option<Vignette> {
        Fading::showing(&self.vignette)
    }

    /// Ease the filter to `set` (`None`: to nothing) from what it is now, or from none.
    fn set_filter(&mut self, set: Option<Filter>, fade: f32) {
        Fading::set(&mut self.filter, set, fade);
    }

    /// The filter right now, if there is one showing.
    pub fn filter(&self) -> Option<Filter> {
        Fading::showing(&self.filter)
    }

    /// The colour and opacity the stage is covered with right now, if it is.
    pub fn cover(&self) -> Option<([f32; 3], f32)> {
        self.cover.as_ref().filter(|c| c.fade.alpha > 0.0).map(|c| (c.color, c.fade.alpha))
    }

    /// Where the camera is right now.
    pub fn camera(&self) -> Camera {
        self.camera.current()
    }

    #[allow(clippy::too_many_arguments)]
    fn show_sprite(&mut self, id: &str, (src, layers): (&str, &[(String, String)]), (x, y): (f32, f32), z: i32, look: Look, fx: [f32; 7], fade: f32) {
        // A sprite that is mid-hide and shown again starts over.
        self.sprites.retain(|s| s.id != id || !s.hiding);
        match self.sprites.iter_mut().find(|s| s.id == id) {
            Some(s) => {
                if !layers.is_empty() || !s.layers.is_empty() {
                    s.set_layers(layers, fade);
                }
                if s.src != src {
                    let old = std::mem::replace(&mut s.src, src.to_string());
                    s.previous_src = if fade > 0.0 { Some(old) } else { None };
                    s.previous_anim_time = s.anim_time;
                    s.anim_time = 0.0;
                    s.elapsed = 0.0;
                    s.fade = fade;
                }
                // A new pose that leaves the resting values alone does not disturb a loop.
                let values = [x, y, look.scale, look.xscale, look.yscale, fx[0], fx[1], fx[2], fx[3], fx[4], fx[5], fx[6]];
                if (s.loop_far.is_some() || s.cycle) && s.home() == values {
                    s.z = z;
                    s.flip = look.flip;
                    s.additive = look.additive;
                    return;
                }
                s.loop_far = None;
                s.end_sequence();
                s.out = false;
                s.from = values;
                (s.alpha_to, s.dx, s.dy, s.blur, s.tint) = (fx[0], fx[1], fx[2], fx[3], [fx[4], fx[5], fx[6]]);
                s.x = x;
                s.y = y;
                s.z = z;
                s.additive = look.additive;
                s.move_time = 0.0;
                s.move_elapsed = 0.0;
                s.scale = look.scale;
                s.flip = look.flip;
                s.xscale = look.xscale;
                s.yscale = look.yscale;
            }
            None => self.sprites.push(SpriteView::new(id, (src, layers), (x, y), z, look, fx, fade)),
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
            s.previous_layers.clear();
            for l in &mut s.layers {
                l.previous_src = None;
            }
        }
    }

    /// Advance animations by `dt` seconds.
    pub fn tick(&mut self, dt: f32) {
        self.advance(dt, true);
    }

    /// Run out what has an end (fades, slides, typing) as [`View::tick`] with a long `dt`
    /// would, but leave animation clocks and loops where they are (skip mode uses this).
    pub fn settle(&mut self, dt: f32) {
        self.advance(dt, false);
    }

    fn advance(&mut self, dt: f32, animate: bool) {
        if animate {
            self.bg_clock += dt;
            self.previous_bg_clock += dt;
        }
        if let Some(bg) = &mut self.background {
            bg.elapsed += dt;
            if bg.progress() >= 1.0 {
                self.previous_background = None;
            }
        }
        let targets: Vec<f32> = self.sprites.iter().map(|s| self.shade_target(&s.id)).collect();
        for (s, target) in self.sprites.iter_mut().zip(targets) {
            s.tick(dt, animate);
            // The whole dim-and-back range takes SHADE_TIME, however deep the theme dims.
            let step = dt / SHADE_TIME * (1.0 - self.theme.speaker_highlight).max(0.01);
            s.shade = if (target - s.shade).abs() <= step { target } else { s.shade + step.copysign(target - s.shade) };
        }
        self.camera.elapsed += dt;
        if let Some(shake) = &mut self.shake {
            shake.elapsed += dt;
            if shake.elapsed >= shake.time {
                self.shake = None;
            }
        }
        if let Some(flash) = &mut self.flash {
            flash.elapsed += dt;
            if flash.elapsed >= flash.time {
                self.flash = None;
            }
        }
        if let Some(n) = &mut self.notice {
            n.elapsed += dt;
            if n.elapsed >= n.time {
                self.notice = None;
            }
        }
        if let Some(c) = &mut self.cover {
            c.fade.tick(dt);
            if c.fade.target == 0.0 && c.fade.alpha <= 0.0 {
                self.cover = None;
            }
        }
        Fading::tick(&mut self.tint, dt);
        Fading::tick(&mut self.vignette, dt);
        Fading::tick(&mut self.filter, dt);
        self.sprites.retain(|s| !s.is_gone());
        self.hud.tick(dt);
        self.particles.tick(dt, animate);
        self.box_fade.tick(dt);
        if let Some(left) = &mut self.wait {
            *left -= dt;
        }
        if let Some(d) = &mut self.dialogue {
            if self.chars_per_sec > 0.0 {
                d.type_on(dt, self.chars_per_sec);
            } else {
                d.show_text();
                d.type_on(dt, 1.0);
            }
        }
    }

    /// Whether time matters even when nothing on screen moves: a `::wait`
    /// counting down, a flash fading, or a typewriter that is still typing (or
    /// paused at `{w=}`), or a frame animation or looping move that is running.
    /// Other animations show up as a changing draw list anyway.
    pub fn busy(&self) -> bool {
        self.wait.is_some()
            || self.flash.is_some()
            || self.notice.is_some()
            || self.cover.as_ref().is_some_and(|c| !c.fade.done())
            || Fading::busy(&self.tint)
            || Fading::busy(&self.vignette)
            || Fading::busy(&self.filter)
            || self.hud.fading()
            || !self.box_fade.done()
            || self.dialogue.as_ref().is_some_and(|d| !d.is_complete())
            || self.animating()
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

    /// Whether choice button `i` is greyed out: it can't be hovered, focused or picked.
    pub fn is_locked(&self, i: usize) -> bool {
        self.locked.get(i).is_some_and(Option::is_some)
    }

    /// Which pickable choice button (if any) contains this virtual-space point.
    pub fn choice_at(&self, x: f32, y: f32) -> Option<usize> {
        choice_rects(&self.theme, self.choices.len()).iter().position(|r| r.contains(x, y)).filter(|&i| !self.is_locked(i))
    }

    /// Which hotspot (if any) contains this virtual-space point. Later ones are
    /// drawn on top, so they win where regions overlap.
    pub fn hotspot_at(&self, x: f32, y: f32) -> Option<usize> {
        self.hotspots.iter().rposition(|s| self.spot_rect(s).is_some_and(|r| r.contains(x, y)))
    }

    /// Where a hotspot is on screen now. A plain one is its rect; one bound to a sprite is `rect` (a
    /// fraction of it) of where that sprite is drawn this moment, camera and shake included, and
    /// `None` while the sprite isn't shown (or no frame has drawn it yet): it can't be clicked or focused.
    pub fn spot_rect(&self, spot: &Spot) -> Option<Rect> {
        let Some(id) = &spot.sprite else { return Some(hotspot_rect(spot)) };
        let sprite = self.sprites.iter().find(|s| s.id == *id).filter(|s| !s.hiding)?;
        let mut drawn = Vec::new();
        self.push_sprite_views(&mut drawn, sprite, &|src| self.sprite_sizes.borrow().get(src).copied());
        let view = self.stage_transform();
        let mut rects = drawn.into_iter().filter_map(|d| match d {
            Draw::Image { dest, .. } => Some(view(dest)),
            _ => None,
        });
        let first = rects.next()?;
        let (l, t, r, b) = rects.fold((first.x, first.y, first.x + first.w, first.y + first.h), |(l, t, r, b), q| {
            (l.min(q.x), t.min(q.y), r.max(q.x + q.w), b.max(q.y + q.h))
        });
        let [x, y, w, h] = spot.rect;
        let (sw, sh) = (r - l, b - t);
        Some(Rect::new(l + x * sw, t + y * sh, w * sw, h * sh))
    }

    /// Every picture of one sprite at its place and size (the pose, an outgoing pose, layers).
    fn push_sprite_views(&self, stage: &mut Vec<Draw>, s: &SpriteView, image_size: &dyn Fn(&str) -> Option<Size>) {
        let first = stage.len();
        self.push_sprite_pictures(stage, s, image_size);
        let (dx, dy) = s.current_offset();
        let tint = s.current_tint();
        for d in &mut stage[first..] {
            if let Draw::Image { shade, dest, blur, additive, .. } = d {
                *shade = [s.shade * tint[0], s.shade * tint[1], s.shade * tint[2]];
                *additive = s.additive;
                *blur = s.current_blur();
                // The offset comes after placement, in pixels of the stage (VIRTUAL).
                dest.x += dx;
                dest.y += dy;
            }
        }
    }

    fn push_sprite_pictures(&self, stage: &mut Vec<Draw>, s: &SpriteView, image_size: &dyn Fn(&str) -> Option<Size>) {
        let (x, y, look, base) = (s.current_x(), s.current_y(), s.look(), s.now(5).clamp(0.0, 1.0));
        if let Some(prev) = &s.previous_src {
            push_sprite(stage, self.picture(prev, s.previous_anim_time), (x, y), look, base, self.art_scale, image_size);
        }
        // The layers of a layered sprite: the same place and size for each, back to front, and
        // each one crossfades on its own when its picture changes.
        for (src, t) in &s.previous_layers {
            push_sprite(stage, self.picture(src, *t), (x, y), look, base, self.art_scale, image_size);
        }
        push_sprite(stage, self.picture(&s.src, s.anim_time), (x, y), look, s.opacity(), self.art_scale, image_size);
        for l in &s.layers {
            let (sprite_alpha, layer_alpha) = (s.opacity(), l.alpha());
            if let Some(prev) = &l.previous_src {
                // Under a layer that fades in; when the layer fades out to nothing, it fades with it.
                let alpha = if l.src.is_empty() { sprite_alpha * (1.0 - layer_alpha) } else { sprite_alpha };
                push_sprite(stage, self.picture(prev, l.previous_anim_time), (x, y), look, alpha, self.art_scale, image_size);
            }
            push_sprite(stage, self.picture(&l.src, l.anim_time), (x, y), look, sprite_alpha * layer_alpha, self.art_scale, image_size);
        }
    }

    /// Everything to draw this frame, back to front.
    ///
    /// `image_size` reports the pixel size of a loaded image; images it doesn't
    /// know are skipped. `hover` is the choice (or hotspot) under the mouse or
    /// the keyboard focus, if any.
    pub fn draw_list(&self, image_size: &dyn Fn(&str) -> Option<Size>, hover: Option<usize>) -> Vec<Draw> {
        self.draw_list_hud(image_size, hover, None, HudLook::default())
    }

    /// [`View::draw_list`] that also knows where the mouse is (virtual coordinates, so a hovered
    /// hotspot's label can float next to it) and the HUD buttons' look: whether they work now.
    pub fn draw_list_hud(
        &self,
        image_size: &dyn Fn(&str) -> Option<Size>,
        hover: Option<usize>,
        pointer: Option<(f32, f32)>,
        hud: HudLook,
    ) -> Vec<Draw> {
        self.draw_list_ui(image_size, hover, pointer, true, hud)
    }

    /// The pictures of the HUD buttons (see [`HudLayer::button_pictures`]).
    pub fn hud_button_pictures(&self) -> impl Iterator<Item = &str> {
        self.hud.button_pictures()
    }

    /// The HUD button under the point (see [`HudLayer::button_at`]); `image_size` is as for the draw list.
    pub fn hud_button_at(&self, image_size: &dyn Fn(&str) -> Option<Size>, x: f32, y: f32) -> Option<&quillstone_script::HudElem> {
        if self.hide_ui {
            return None;
        }
        self.hud.button_at(&self.theme, image_size, x, y)
    }

    /// Only the picture: stage, inline images, video, flash and hotspot art, without
    /// the textbox, choices, hotspot outline or tooltip. What a full-screen overlay
    /// (settings, backlog, ...) leaves under its dim.
    pub fn draw_scene(&self, image_size: &dyn Fn(&str) -> Option<Size>) -> Vec<Draw> {
        self.draw_list_ui(image_size, None, None, false, HudLook::default())
    }

    fn draw_list_ui(
        &self,
        image_size: &dyn Fn(&str) -> Option<Size>,
        hover: Option<usize>,
        pointer: Option<(f32, f32)>,
        ui: bool,
        hud: HudLook,
    ) -> Vec<Draw> {
        let mut out = Vec::new();
        let screen = Rect::new(0.0, 0.0, VIRTUAL.w, VIRTUAL.h);

        // The filter covers everything up to the tint (stage, particles, inline images, video, hotspot art).
        let filter = self.filter().and_then(|f| crate::filter::matrix_unless_neutral(&f));
        if filter.is_some() {
            out.push(Draw::Filter(filter));
        }

        // The stage: background and sprites, seen through the camera and shake.
        let mut stage = Vec::new();
        if let Some(prev) = &self.previous_background {
            push_backdrop(&mut stage, self.picture(prev, self.previous_bg_clock), screen, 1.0, image_size);
            blur_all(&mut stage, self.previous_blur);
        }
        if let Some(bg) = &self.background {
            let mut picture = Vec::new();
            push_backdrop(&mut picture, self.picture(&bg.src, self.bg_clock), screen, bg.alpha(), image_size);
            blur_all(&mut picture, bg.blur);
            let masked = bg.masked();
            if let Some((mask, _)) = &masked {
                image_size(mask); // asks for the mask picture (the frame waits until it is loaded)
                stage.push(Draw::Mask(masked.clone()));
            }
            for draw in picture {
                match (draw, bg.revealed()) {
                    (Draw::Image { src, dest, uv, alpha, shade, blur, additive }, Some(part)) => {
                        if let Some((dest, uv)) = clip(dest, uv, part) {
                            stage.push(Draw::Image { src, dest, uv, alpha, shade, blur, additive });
                        }
                    }
                    (Draw::Rect { rect, color }, Some(part)) => {
                        if let Some((rect, _)) = clip(rect, FULL_UV, part) {
                            stage.push(Draw::Rect { rect, color });
                        }
                    }
                    (draw, _) => stage.push(draw),
                }
            }
            if masked.is_some() {
                stage.push(Draw::Mask(None));
            }
        }
        let bg_end = stage.len();
        // Back to front by z; the sort is stable, so equal z keeps the order sprites were shown in.
        let mut order: Vec<&SpriteView> = self.sprites.iter().collect();
        order.sort_by_key(|s| s.z);
        let follows = self.hotspots.iter().any(|h| h.sprite.is_some());
        for s in order {
            let noted = |src: &str| {
                let size = image_size(src);
                if let Some(size) = size.filter(|_| follows) {
                    self.sprite_sizes.borrow_mut().insert(src.to_string(), size);
                }
                size
            };
            self.push_sprite_views(&mut stage, s, &noted);
        }
        // Particles are screen space: `layer=back` ones go between the background and the sprites, the
        // rest over the sprites; neither follows the camera or the shake.
        let sprites = stage.split_off(bg_end);
        let view = self.stage_transform();
        let place = |out: &mut Vec<Draw>, draw: Draw| match draw {
            Draw::Image { src, dest, uv, alpha, shade, blur, additive } => {
                let zoomed = view(dest);
                let blur = if dest.w > 0.0 { blur * zoomed.w / dest.w } else { blur }; // a zoomed-in camera blurs as much more
                if let Some((dest, uv)) = clip(zoomed, uv, screen) {
                    out.push(Draw::Image { src, dest, uv, alpha, shade, blur, additive });
                }
            }
            // A solid colour fills the screen whatever the camera and shake do.
            rect @ Draw::Rect { .. } => out.push(rect),
            mask @ Draw::Mask(_) => out.push(mask),
            Draw::Text { .. } | Draw::Filter(_) | Draw::Vignette { .. } => {}
        };
        for draw in stage {
            place(&mut out, draw);
        }
        self.particles.draw(true, image_size, &mut out);
        for draw in sprites {
            place(&mut out, draw);
        }
        self.particles.draw(false, image_size, &mut out);

        let textbox = textbox_rect(&self.theme);
        let stage = Rect::new(0.0, 0.0, VIRTUAL.w, textbox.y);
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
                    size: self.theme.text_size * self.text_scale,
                    color: self.theme.text_color,
                    align: Align::Center,
                    outline: None,
                });
            }
        }

        let theme = &self.theme;
        // The region under the mouse or keyboard (never a hidden one: nothing may give it away). It is over
        // the textbox below, with its tooltip.
        let hovered_spot = hover
            .filter(|_| ui)
            .and_then(|i| self.hotspots.get(i))
            .filter(|s| !s.hidden)
            .and_then(|s| Some((s, self.spot_rect(s)?)));
        if let Some(ground) = self.imagemap.as_ref().and_then(|m| m.ground.as_deref()) {
            push_image(&mut out, ground, screen, Fit::Stretch, 1.0, image_size);
        }
        // A hover picture that isn't loaded counts as none: the region keeps its idle picture and its outline.
        let has_hover = |s: &Spot| s.hover.as_ref().is_some_and(|h| image_size(h).is_some());
        for spot in &self.hotspots {
            let on = hovered_spot.is_some_and(|(h, _)| std::ptr::eq(h, spot));
            let picture = spot.hover.as_ref().filter(|_| on && has_hover(spot)).or(spot.image.as_ref());
            if let (Some(src), Some(rect)) = (picture, self.spot_rect(spot)) {
                push_image(&mut out, src, rect, Fit::Stretch, 1.0, image_size);
            }
        }
        // An imagemap shows the same rect of its hover picture, unless the region has pictures of its own.
        if let (Some(map), Some((spot, rect))) = (&self.imagemap, hovered_spot) {
            if spot.image.is_none() && !has_hover(spot) && image_size(&map.hover).is_some() {
                if let Some((dest, uv)) = clip(screen, FULL_UV, rect) {
                    out.push(Draw::Image { src: map.hover.clone(), dest, uv, alpha: 1.0, shade: [1.0; 3], blur: 0.0, additive: false });
                }
            }
        }

        if filter.is_some() {
            out.push(Draw::Filter(None));
        }
        // The tint, the vignette, the cover and the flash are over the whole picture (stage, particles, inline images,
        // video and hotspot art), in that order, but under the textbox and the UI.
        if let Some(([r, g, b], alpha)) = self.tint() {
            out.push(Draw::Rect { rect: screen, color: [r, g, b, alpha] });
        }
        if let Some(Vignette { color: [r, g, b], strength, size }) = self.vignette() {
            out.push(Draw::Vignette { rect: screen, color: [r, g, b, strength], zoom: vignette_zoom(size) });
        }
        if let Some((color, alpha)) = self.cover() {
            let [r, g, b] = color;
            out.push(Draw::Rect { rect: screen, color: [r, g, b, alpha] });
        }
        if let Some(flash) = &self.flash {
            let [r, g, b] = flash.color;
            out.push(Draw::Rect { rect: screen, color: [r, g, b, flash.alpha()] });
        }

        // The HUD is story UI: over the cover and the flash (a title card can sit on a black
        // screen), under the textbox. Not with a full-screen overlay up, nor when the player hid the UI.
        if ui && !self.hide_ui {
            self.hud.draw(&self.theme, image_size, hud, &mut out);
            // A passing message in the top right corner, over the HUD.
            if let Some(n) = &self.notice {
                let alpha = n.alpha();
                let (size, pad) = ((theme.text_size * self.text_scale * 0.75).round(), 16.0);
                let (color, back) = (faded(theme.text_color, alpha), Some(faded(theme.panel_color, alpha)));
                crate::hud::draw_text(&mut out, &n.text, (1.0, 0.0), 40.0, size, color, back, pad);
            }
        }

        let box_alpha = self.box_alpha();
        let shown = self.dialogue.as_ref().filter(|_| ui && box_alpha > 0.0);
        let paged = !self.page.is_empty();
        if let Some(d) = shown.filter(|_| paged) {
            // A page line: no textbox, name plate or portrait; the page's lines over a dimmed screen.
            out.push(Draw::Rect { rect: screen, color: faded(theme.page_color, box_alpha) });
            let mut paras: Vec<_> = self.page[..self.page.len() - 1].iter().map(|l| l.paragraph(theme)).collect();
            paras.push(PageLine { speaker: d.speaker.clone(), color: d.color, text: d.text.clone(), runs: d.runs.clone() }.paragraph(theme));
            let last = paras.len() - 1;
            // Choices sit over the page: its text steps back so the buttons can be read.
            let text_alpha = if self.choices.is_empty() { box_alpha } else { box_alpha * 0.3 };
            for p in screen_page(theme, self.text_scale, &paras) {
                let para = &paras[p.index];
                out.push(Draw::Text {
                    text: para.text.clone(),
                    runs: para.runs.clone(), // coloured runs don't fade (the colour has no alpha)
                    visible_chars: if p.index == last { para.name_chars + d.visible_chars() } else { usize::MAX },
                    rect: p.rect,
                    size: p.size,
                    color: faded(theme.text_color, text_alpha),
                    align: Align::Left,
                    outline: text_outline(theme),
                });
            }
        }
        if let Some(d) = shown.filter(|_| !paged) {
            // The theme's textbox picture if it is loaded, else a plain panel.
            let picture = theme.textbox_image.as_deref().filter(|src| image_size(src).is_some());
            match picture {
                Some(src) => push_image(&mut out, src, textbox, Fit::Stretch, box_alpha, image_size),
                None => out.push(Draw::Rect { rect: textbox, color: faded(theme.panel_color, box_alpha) }),
            }
            if let Some(src) = &self.speaker_portrait {
                push_portrait(&mut out, src, portrait_rect(theme), box_alpha, image_size);
            }
            let has_name = match &d.speaker {
                Some(name) => {
                    let color = d.color.map(|[r, g, b]| [r, g, b, 1.0]).unwrap_or(theme.name_color);
                    let plate = name_plate_rect(theme);
                    if let Some(src) = theme.name_plate_image.as_deref() {
                        push_image(&mut out, src, plate, Fit::Stretch, box_alpha, image_size);
                    }
                    let (name, size) = fit_name(name, theme.name_size, plate.w, plate.h);
                    out.push(Draw::Text {
                        text: name,
                        runs: Vec::new(),
                        visible_chars: usize::MAX,
                        rect: plate,
                        size,
                        color: faded(color, box_alpha),
                        align: theme.name_align,
                        outline: text_outline(theme),
                    });
                    true
                }
                None => false,
            };
            out.push(Draw::Text {
                text: d.text.clone(),
                runs: d.runs.clone(), // coloured runs don't fade (the colour has no alpha)
                visible_chars: d.visible_chars(),
                rect: dialogue_body(theme, has_name),
                size: self.dialogue_size(d, has_name),
                color: faded(theme.text_color, box_alpha),
                align: theme.text_align,
                outline: text_outline(theme),
            });
        }

        // A hovered region's tint and outline are over the textbox, so one that
        // reaches under it can be seen (hit-testing gives it the click, too).
        // A picture is the highlight when there is one (a hover picture or an imagemap); the tooltip stays.
        let map_hover = self.imagemap.as_ref().is_some_and(|m| image_size(&m.hover).is_some());
        let outlined = hovered_spot.filter(|(s, _)| !has_hover(s) && !map_hover);
        if let Some((_, rect)) = outlined {
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

        let choices = if ui { self.choices.as_slice() } else { &[] };
        for (i, (rect, label)) in choice_rects(theme, choices.len()).into_iter().zip(choices).enumerate() {
            // A locked option looks like the shop's unaffordable item: faint button, dim text.
            let reason = self.locked.get(i).and_then(|r| r.as_deref()).filter(|r| !r.is_empty());
            let locked = self.is_locked(i);
            let [r, g, b, a] = theme.button_color;
            let color = match (locked, hover == Some(i)) {
                (true, _) => [r, g, b, a * 0.45],
                (false, true) => theme.button_hover_color,
                (false, false) => theme.button_color,
            };
            out.push(Draw::Rect { rect, color });
            let size = (theme.text_size * self.text_scale).min(rect.h * 0.5);
            // With a reason the label moves up and the reason sits under it, smaller.
            let label_rect = if reason.is_some() { Rect::new(rect.x, rect.y, rect.w, rect.h * 0.62) } else { rect };
            let Rich { text, runs, .. } = rich(label);
            out.push(Draw::Text {
                text,
                runs,
                visible_chars: usize::MAX,
                rect: label_rect,
                size: if reason.is_some() { size * 0.8 } else { size },
                color: if locked { theme.dim_color } else { theme.text_color },
                align: Align::Center,
                outline: None,
            });
            if let Some(reason) = reason {
                let Rich { text, runs, .. } = rich(reason);
                out.push(Draw::Text {
                    text,
                    runs,
                    visible_chars: usize::MAX,
                    rect: Rect::new(rect.x, rect.y + rect.h * 0.55, rect.w, rect.h * 0.4),
                    size: size * 0.55,
                    color: theme.dim_color,
                    align: Align::Center,
                    outline: None,
                });
            }
        }

        // The hovered hotspot's label, next to the mouse (or under the region when the keyboard is on it).
        if let Some((spot, r)) = hovered_spot {
            let Rich { text, runs, .. } = rich(&spot.label);
            let size = (theme.text_size * self.text_scale * 0.8).round();
            let w = (text.chars().map(|c| if is_wide(c) { size } else { size * 0.55 }).sum::<f32>() + 2.0 * TIP_PAD)
                .min(VIRTUAL.w - 2.0 * TIP_MARGIN);
            let h = size * 1.35 + TIP_PAD;
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
                outline: None,
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
        let (left, top) = (c.x * (VIRTUAL.w - w), c.y * (VIRTUAL.h - h)); // the same rule as a sprite's `x=` / `y=`
        let (dx, dy) = self.shake.as_ref().map_or((0.0, 0.0), Shake::offset);
        // While shaking, zoom about the screen centre so the offset never shows the edge.
        let s = self.shake.as_ref().map_or(1.0, Shake::cover_zoom);
        let (cx, cy) = (VIRTUAL.w / 2.0, VIRTUAL.h / 2.0);
        move |r: Rect| {
            let (x, y, w, h) = ((r.x - left) * z, (r.y - top) * z, r.w * z, r.h * z);
            Rect::new((x - cx) * s + cx + dx, (y - cy) * s + cy + dy, w * s, h * s)
        }
    }
}

/// Button rectangles for `count` choices, stacked and centred above the textbox.
/// Height of the speaker's name plate.
const NAME_PLATE_H: f32 = 48.0;

/// The speaker's name and the size to draw it at so that it fits the plate: the
/// configured size stepped down (never below 60% of it) until one line fits the
/// plate's height and width, then cut with an ellipsis if it still doesn't.
fn fit_name(name: &str, size: f32, width: f32, height: f32) -> (String, f32) {
    let floor = size * 0.6;
    let fits = |s: f32| line_height(s) <= height + 1.0 && estimate_lines(name, s, width) <= 1;
    let mut s = size;
    while s > floor && !fits(s) {
        s -= 1.0;
    }
    let s = s.max(floor);
    (one_row(name, s, width), s)
}

/// The dialogue box for this theme (`textbox_rect`), in virtual coordinates.
pub fn textbox_rect(theme: &Theme) -> Rect {
    let [x, y, w, h] = theme.textbox_rect;
    Rect::new(x, y, w, h)
}

/// Where the speaker's portrait goes: the theme's `portrait_rect`, else a 320 px square
/// standing on the textbox's top edge at its left edge.
pub fn portrait_rect(theme: &Theme) -> Rect {
    if let Some([x, y, w, h]) = theme.portrait_rect {
        return Rect::new(x, y, w, h);
    }
    let tb = textbox_rect(theme);
    Rect::new(tb.x, (tb.y - DEFAULT_PORTRAIT).max(0.0), DEFAULT_PORTRAIT, DEFAULT_PORTRAIT.min(tb.y))
}

/// The side of the default portrait square.
const DEFAULT_PORTRAIT: f32 = 320.0;

/// The picture fitted inside `rect` whole, standing on its bottom edge, in the middle of it.
fn push_portrait(out: &mut Vec<Draw>, src: &str, rect: Rect, alpha: f32, image_size: &dyn Fn(&str) -> Option<Size>) {
    let Some(size) = image_size(src).filter(|s| s.w > 0.0 && s.h > 0.0) else { return };
    let k = (rect.w / size.w).min(rect.h / size.h);
    let (w, h) = (size.w * k, size.h * k);
    let dest = Rect::new(rect.x + (rect.w - w) / 2.0, rect.y + rect.h - h, w, h);
    out.push(Draw::Image { src: src.to_string(), dest, uv: FULL_UV, alpha, shade: [1.0; 3], blur: 0.0, additive: false });
}

/// The speaker's name plate: the theme's `name_plate_rect`, else a strip inside the
/// textbox's top left (inset by the padding's left and right).
pub fn name_plate_rect(theme: &Theme) -> Rect {
    if let Some([x, y, w, h]) = theme.name_plate_rect {
        return Rect::new(x, y, w, h);
    }
    let tb = textbox_rect(theme);
    let [l, _, r, _] = theme.textbox_padding;
    Rect::new(tb.x + l, tb.y + 22.0, tb.w - l - r, NAME_PLATE_H)
}

/// Where a line's words go in the textbox: inside the padding, and below the name
/// plate if there is a speaker and the plate is inside the box.
pub fn dialogue_body(theme: &Theme, with_speaker: bool) -> Rect {
    let tb = textbox_rect(theme);
    let [l, t, r, b] = theme.textbox_padding;
    let mut top = tb.y + t;
    if with_speaker {
        let plate = name_plate_rect(theme);
        if plate.y + plate.h > tb.y {
            top = top.max(plate.y + plate.h + 10.0);
        }
    }
    Rect::new(tb.x + l, top, tb.w - l - r, (tb.y + tb.h - top - b).max(0.0))
}

/// About how many textbox lines `tagged` (a line as written, with its `{size}` tags) takes at
/// `size` px, and how many fit: a bigger run is counted as wide as it will really be drawn. A
/// line that doesn't fit is drawn smaller by the player. Plain text works too.
pub fn dialogue_lines_tagged(theme: &Theme, tagged: &str, with_speaker: bool, size: f32) -> (usize, usize) {
    let body = dialogue_body(theme, with_speaker);
    let line_h = line_height(size);
    let Rich { text, runs, .. } = rich(tagged);
    (crate::estimate_lines_styled(&text, &runs, size, body.w), (body.h / line_h).floor() as usize)
}

/// [`dialogue_lines_tagged`] for a page line: about how many lines it takes at `size` px (the speaker's name
/// in front), and how many the page area fits.
pub fn page_lines_tagged(theme: &Theme, speaker: Option<&str>, tagged: &str, size: f32) -> (usize, usize) {
    let para = PageLine::new(speaker.map(str::to_string), None, tagged).paragraph(theme);
    let [_, _, w, h] = theme.page_rect;
    (crate::estimate_lines_styled(&para.text, &para.runs, size, w), (h / line_height(size)).floor() as usize)
}

/// The theme's outline for dialogue and name text, if it asks for one.
fn text_outline(theme: &Theme) -> Option<Outline> {
    (theme.text_outline_size > 0.0).then_some(Outline { size: theme.text_outline_size, color: theme.text_outline_color })
}

/// `color` with its opacity scaled by `alpha`.
pub fn faded(color: [f32; 4], alpha: f32) -> [f32; 4] {
    [color[0], color[1], color[2], color[3] * alpha]
}

/// The size text is drawn at when the player scales it: the theme's size times the scale, or the largest of the
/// steps down from there (a tenth of the scale each) that `fits`, never below the theme's size. The textbox
/// and the full-screen page both use it, for a line that is too long for its room.
pub(crate) fn fitted_size(theme: &Theme, text_scale: f32, fits: impl Fn(f32) -> bool) -> f32 {
    let steps = ((text_scale - 1.0) * 10.0).round() as i32;
    (1..=steps)
        .rev()
        .map(|i| theme.text_size * (1.0 + (text_scale - 1.0) * i as f32 / steps as f32))
        .find(|&size| fits(size))
        .unwrap_or(theme.text_size)
}

/// A hotspot's region in virtual coordinates.
pub fn hotspot_rect(spot: &Spot) -> Rect {
    let [x, y, w, h] = spot.rect;
    Rect::new(x * VIRTUAL.w, y * VIRTUAL.h, w * VIRTUAL.w, h * VIRTUAL.h)
}

/// The text size of a choice button: the theme's, smaller on buttons squeezed by many options.
pub fn choice_text_size(theme: &Theme, rect: Rect) -> f32 {
    theme.text_size.min(rect.h * 0.5)
}

pub fn choice_rects(theme: &Theme, count: usize) -> Vec<Rect> {
    if count == 0 {
        return Vec::new();
    }
    let textbox = textbox_rect(theme);
    // Many options: buttons and gaps shrink together so all of them stay above the textbox.
    let room = textbox.y - 2.0 * CHOICE_MARGIN;
    let full = count as f32 * CHOICE_H + (count - 1) as f32 * CHOICE_GAP;
    let k = (room / full).min(1.0);
    let (h, gap) = (CHOICE_H * k, CHOICE_GAP * k);
    let top = (textbox.y - full * k) / 2.0;
    let x = (VIRTUAL.w - CHOICE_W) / 2.0;
    (0..count).map(|i| Rect::new(x, top + i as f32 * (h + gap), CHOICE_W, h)).collect()
}

/// A background: its picture, or the whole screen in its colour for a `color:#rrggbb` one.
/// Blur every picture in `draws` by `radius` stage pixels.
fn blur_all(draws: &mut [Draw], radius: f32) {
    for d in draws {
        if let Draw::Image { blur, .. } = d {
            *blur = radius;
        }
    }
}

fn push_backdrop(out: &mut Vec<Draw>, src: &str, screen: Rect, alpha: f32, image_size: &dyn Fn(&str) -> Option<Size>) {
    match quillstone_script::bg_color(src) {
        Some([r, g, b]) => out.push(Draw::Rect { rect: screen, color: [r, g, b, alpha] }),
        None => push_image(out, src, screen, Fit::Cover, alpha, image_size),
    }
}

fn push_image(
    out: &mut Vec<Draw>,
    src: &str,
    container: Rect,
    fit: Fit,
    alpha: f32,
    image_size: &dyn Fn(&str) -> Option<Size>,
) {
    if src.is_empty() {
        return; // an animation this view doesn't know
    }
    if let Some(size) = image_size(src) {
        let p = place(size, container, fit);
        out.push(Draw::Image { src: src.to_string(), dest: p.dest, uv: p.uv, alpha, shade: [1.0; 3], blur: 0.0, additive: false });
    }
}

/// Where an object of `size` starts on a screen of `screen` when placed at `v`: inside 0..1 the point
/// that far across it is that far across the screen; beyond that it keeps sliding by whole screen widths,
/// whatever its size (a bigger-than-screen object would otherwise move the wrong way).
fn place_axis(v: f32, screen: f32, size: f32) -> f32 {
    let c = v.clamp(0.0, 1.0);
    c * (screen - size) + (v - c) * screen
}

/// A sprite placed by `x`, `y`: the point that far across (down) it is that far across (down) the screen.
fn push_sprite(
    out: &mut Vec<Draw>,
    src: &str,
    (x, y): (f32, f32),
    look: Look,
    alpha: f32,
    art_scale: Option<f32>,
    image_size: &dyn Fn(&str) -> Option<Size>,
) {
    if src.is_empty() {
        return; // nothing: a hidden layer, a layered sprite's own picture, an animation this view doesn't know
    }
    let Some(size) = image_size(src) else { return };
    if size.w <= 0.0 || size.h <= 0.0 {
        return;
    }
    // Fixed: every picture is SPRITE_HEIGHT tall. Pixels: the picture's own size, times the game's art scale.
    let (base_w, base_h) = match art_scale {
        Some(k) => (size.w * k, size.h * k),
        None => (size.w / size.h * SPRITE_HEIGHT, SPRITE_HEIGHT),
    };
    let h = base_h * look.scale * look.yscale;
    let w = base_w * look.scale * look.xscale;
    // A mirrored sprite reads its texture right to left: the clip below carries the
    // negative width along, so cropping at the screen edge stays right.
    let uv = if look.flip { Rect::new(1.0, 0.0, -1.0, 1.0) } else { FULL_UV };
    out.push(Draw::Image { src: src.to_string(), dest: Rect::new(place_axis(x, VIRTUAL.w, w), place_axis(y, VIRTUAL.h, h), w, h), uv, alpha, shade: [1.0; 3], blur: 0.0, additive: false });
}

#[cfg(test)]
mod anim_tests;
#[cfg(test)]
mod blur_tests;
#[cfg(test)]
mod additive_tests;
#[cfg(test)]
mod sprite_tint_tests;
#[cfg(test)]
mod filter_tests;
#[cfg(test)]
mod vignette_tests;
#[cfg(test)]
mod mask_tests;
#[cfg(test)]
mod highlight_tests;
#[cfg(test)]
mod portrait_tests;
#[cfg(test)]
mod layers_tests;

#[cfg(test)]
mod tests {
    use super::*;
    use quillstone_runtime::{Input, Session};
    use quillstone_script::parse;

    fn say(speaker: Option<&str>, text: &str) -> Cmd {
        Cmd::Say { speaker: speaker.map(str::to_string), color: None, text: text.into() }
    }

    fn bg(src: &str, fade: f32) -> Cmd {
        Cmd::SetBackground { src: src.into(), fade, wipe: None, sound: false, blur: 0.0, mask: None }
    }

    fn show(id: &str, src: &str, x: f32, fade: f32) -> Cmd {
        Cmd::ShowSprite { id: id.into(), src: src.into(), layers: vec![], x, y: 1.0, z: 0, scale: 1.0, flip: false, xscale: 1.0, yscale: 1.0, alpha: 1.0, dx: 0.0, dy: 0.0, blur: 0.0, tint: [1.0; 3], additive: false, fade }
    }

    fn slide(id: &str, x: f32, scale: f32, time: f32, ease: Ease) -> Cmd {
        Cmd::MoveSprite { id: id.into(), flip: false, to: SpriteStep { x, y: 1.0, scale, xscale: 1.0, yscale: 1.0, alpha: 1.0, dx: 0.0, dy: 0.0, blur: 0.0, tint: [1.0; 3], time, ease, wait: 0.0 }, queued: false }
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
        v.apply(&[Cmd::ShowChoices { options: vec!["*Softly*".into()], locked: Vec::new() }]);
        assert!(v.draw_list(&sizes, None).iter().any(|d| matches!(d, Draw::Text { text, runs, .. } if text == "Softly" && !runs.is_empty())));
    }

    #[test]
    fn ruby_and_underline_ride_with_the_dialogue_text_which_types_by_its_base() {
        let mut v = View::new().with_speed(10.0);
        v.apply(&[say(None, "Is {ruby=かんじ}漢字{/ruby} {u}hard{/u}?")]);
        let d = v.dialogue.as_ref().unwrap();
        assert_eq!(d.text, "Is 漢字 hard?", "the reading is not a typed character");
        assert!(d.runs.iter().any(|r| r.ruby.as_deref() == Some("かんじ") && &d.text[r.start..r.end] == "漢字"));
        assert!(d.runs.iter().any(|r| r.style.underline && &d.text[r.start..r.end] == "hard"));
        v.tick(0.5);
        assert_eq!(v.dialogue.as_ref().unwrap().visible_chars(), 5);
        let shown = v.draw_list(&|_| None, None).into_iter().find_map(|d| match d {
            Draw::Text { visible_chars, runs, .. } => Some((visible_chars, runs.iter().any(|r| r.ruby.is_some()))),
            _ => None,
        });
        assert_eq!(shown, Some((5, true)));
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
    fn a_trailing_pause_must_run_out_before_the_line_is_complete() {
        let mut v = View::new().with_speed(10.0);
        v.apply(&[say(None, "Well{w=2}{nw}")]);
        let done = |v: &View| v.dialogue.as_ref().unwrap().is_complete();
        v.tick(0.5);
        assert!(!done(&v), "typed, now pausing");
        v.tick(1.0);
        assert!(!done(&v));
        v.tick(1.0);
        assert!(done(&v), "the pause has run out");
        // a plain line ending in a pause too, and a click still skips it
        v.apply(&[say(None, "Hm{w=5}")]);
        v.tick(1.0);
        assert!(!done(&v));
        assert!(!v.press_advance(), "the click only cancels the pause");
        assert!(done(&v));
        assert!(v.press_advance());
    }

    #[test]
    fn instant_text_still_honours_a_trailing_pause() {
        let mut v = View::new().with_speed(0.0);
        v.apply(&[say(None, "Well...{w=3}{nw}")]);
        let done = |v: &View| v.dialogue.as_ref().unwrap().is_complete();
        assert!(!done(&v));
        v.tick(2.0);
        assert!(!done(&v));
        v.tick(1.5);
        assert!(done(&v));
        // a pause in the middle is not waited for
        v.apply(&[say(None, "A{w=3}B")]);
        assert!(done(&v));
        // a redraw shows the line at once, trailing pause included
        v.set_speed(10.0);
        v.apply(&[Cmd::ClearScene, say(None, "Back{w=3}")]);
        assert!(done(&v));
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
        v.apply(&[Cmd::ShowChoices { options: vec!["a".into(), "b".into()], locked: Vec::new() }]);
        assert!(!v.press_advance());
    }

    #[test]
    fn a_redraw_shows_its_line_at_once() {
        let mut v = View::new().with_speed(10.0);
        v.apply(&[Cmd::ClearScene, say(None, "already read"), Cmd::ShowChoices { options: vec!["a".into()], locked: Vec::new() }]);
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
    fn x_and_y_put_the_point_that_far_across_a_sprite_that_far_across_the_screen() {
        let mut v = View::new();
        v.apply(&[Cmd::ShowSprite { id: "m".into(), src: "m.png".into(), layers: vec![], x: 0.25, y: 1.0, z: 0, scale: 0.5, flip: false, xscale: 1.0, yscale: 1.0, alpha: 1.0, dx: 0.0, dy: 0.0, blur: 0.0, tint: [1.0; 3], additive: false, fade: 0.0 }]);
        // a 1:2 picture at half scale: 470 tall, 235 wide
        let list = v.draw_list(&|_| Some(Size::new(500.0, 1000.0)), None);
        let Draw::Image { dest, uv, alpha, .. } = &list[0] else { panic!("expected a sprite") };
        assert!(close(dest.h, SPRITE_HEIGHT * 0.5) && close(dest.w, SPRITE_HEIGHT * 0.25));
        assert!(close(dest.y + dest.h, VIRTUAL.h)); // bottom edge
        assert!(close(dest.x, 0.25 * (VIRTUAL.w - dest.w))); // a quarter of the free room
        assert_eq!((*uv, *alpha), (FULL_UV, 1.0));
        // 0 is flush left, 1 flush right, 0.5 centred
        v.apply(&[Cmd::ClearScene, sprite("a", 0.0, 1.0, 0), sprite("b", 1.0, 1.0, 0), sprite("c", 0.5, 1.0, 0)]);
        let list = v.draw_list(&|_| Some(Size::new(500.0, 1000.0)), None);
        let xs: Vec<f32> = images(&list).iter().map(|(_, d, _)| d.x).collect();
        assert!(close(xs[0], 0.0) && close(xs[1], VIRTUAL.w - 470.0) && close(xs[2], (VIRTUAL.w - 470.0) / 2.0), "{xs:?}");
    }

    fn sprite(id: &str, x: f32, y: f32, z: i32) -> Cmd {
        Cmd::ShowSprite { id: id.into(), src: format!("{id}.png"), layers: vec![], x, y, z, scale: 1.0, flip: false, xscale: 1.0, yscale: 1.0, alpha: 1.0, dx: 0.0, dy: 0.0, blur: 0.0, tint: [1.0; 3], additive: false, fade: 0.0 }
    }

    fn drawn_order(v: &View) -> Vec<String> {
        images(&v.draw_list(&all_1080p, None)).iter().map(|(src, _, _)| src.to_string()).collect()
    }

    #[test]
    fn y_puts_the_point_that_far_down_a_sprite_that_far_down_the_screen() {
        let mut v = View::new();
        v.apply(&[sprite("m", 0.5, 0.95, 0)]);
        let list = v.draw_list(&|_| Some(Size::new(500.0, 1000.0)), None);
        let (_, dest, uv) = images(&list)[0];
        assert!(close(dest.y, 0.95 * (VIRTUAL.h - 940.0)), "95% of the 140 px of free room above the sprite");
        assert!(close(dest.h, SPRITE_HEIGHT) && close(uv.h, 1.0));
    }

    #[test]
    fn a_sprite_taller_than_the_screen_still_moves_the_way_y_says() {
        // a 1880 px tall sprite has -800 px of free room: the old rule `y * room` sent y=1.5 upwards
        assert!(close(place_axis(1.0, 1080.0, 1880.0), -800.0));
        assert!(close(place_axis(1.5, 1080.0, 1880.0), -260.0), "down from y=1, by half a screen");
        assert!(close(place_axis(-0.5, 1080.0, 1880.0), -540.0), "up from y=0, by half a screen");
        assert!(close(place_axis(0.5, 1080.0, 940.0), 70.0) && close(place_axis(-1.0, 1920.0, 470.0), -1920.0));
    }

    #[test]
    fn sprites_below_or_above_the_screen_are_clipped() {
        let mut v = View::new();
        v.apply(&[sprite("low", 0.5, 1.2, 0), sprite("high", 0.5, -0.1, 0)]);
        let list = v.draw_list(&|_| Some(Size::new(500.0, 1000.0)), None);
        let shown = images(&list);
        // y = 1.2: the free room (140 px) plus 20% of a screen (216 px): it starts 356 px down, so its last 216 px hang below
        let (_, dest, uv) = shown[0];
        assert!(close(dest.y + dest.h, VIRTUAL.h));
        assert!(close(uv.y, 0.0) && close(uv.h, (940.0 - 216.0) / 940.0), "cut at the bottom edge: {uv:?}");
        // y = -0.1: it starts 108 px (a tenth of a screen) above the screen, so its top 108 px are cut off
        let (_, dest, uv) = shown[1];
        assert!(close(dest.y, 0.0) && close(uv.y, 108.0 / 940.0), "cut at the top edge: {uv:?}");
        // far enough sideways, nothing of it shows
        v.apply(&[Cmd::ClearScene, sprite("gone", -1.0, 1.0, 0)]);
        assert!(images(&v.draw_list(&|_| Some(Size::new(500.0, 1000.0)), None)).is_empty(), "a sprite wholly off-screen is dropped");
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
        Cmd::MoveSprite { id: id.into(), flip: false, to: SpriteStep { x, y, scale: 1.0, xscale: 1.0, yscale: 1.0, alpha: 1.0, dx: 0.0, dy: 0.0, blur: 0.0, tint: [1.0; 3], time, ease, wait: 0.0 }, queued: false }
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
        v.apply(&[show("m", "m.png", -235.0 / 1920.0, 0.0)]); // left edge 235 px (half the sprite) off the screen
        let list = v.draw_list(&|_| Some(Size::new(500.0, 1000.0)), None);
        let (_, dest, uv) = images(&list)[0];
        assert_eq!(dest.x, 0.0);
        assert!(close(uv.x, 0.5) && close(uv.w, 0.5), "only the right half shows");
    }

    #[test]
    fn a_wipe_reveals_the_new_background_from_one_side() {
        let mut v = View::new();
        v.apply(&[bg("a.png", 0.0)]);
        v.apply(&[Cmd::SetBackground { src: "b.png".into(), fade: 1.0, wipe: Some(Wipe::Right), sound: false, blur: 0.0, mask: None }]);
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
        let camera = Camera { zoom: 2.0, x: 0.0, y: 0.0 };
        v.apply(&[bg("bg.png", 0.0), say(None, "hi"), Cmd::Camera { camera, time: 1.0, ease: Ease::Linear }]);
        v.tick(1.0);
        assert_eq!(v.camera(), camera);
        let list = v.draw_list(&all_1080p, None);
        let (_, dest, uv) = images(&list)[0];
        assert_eq!(dest, Rect::new(0.0, 0.0, VIRTUAL.w, VIRTUAL.h), "still fills the screen");
        assert!(close(uv.w, 0.5) && close(uv.x, 0.0) && close(uv.y, 0.0), "showing the top-left quarter");
        assert!(list.iter().any(|d| matches!(d, Draw::Rect { rect, .. } if *rect == TEXTBOX)));
        // the same rule as a sprite: a quarter of the free room (half the stage), from the left and from the top
        v.apply(&[Cmd::Camera { camera: Camera { zoom: 2.0, x: 0.25, y: 0.25 }, time: 0.0, ease: Ease::Linear }]);
        let (_, _, uv) = images(&v.draw_list(&all_1080p, None))[0];
        assert!(close(uv.x, 0.125) && close(uv.y, 0.125), "{uv:?}");
        // the view can't leave the stage: x=0 keeps the left edge on screen
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
        v.apply(&[bg("bg.png", 0.0), Cmd::Shake { time: 0.5, strength: 30.0, axis: Axis::Both }]);
        let mut moved = false;
        for _ in 0..4 {
            v.tick(0.05);
            let list = v.draw_list(&all_1080p, None);
            let (_, dest, uv) = images(&list)[0];
            // The stage is zoomed in and shifted, then clipped: the picture moves in the crop.
            moved |= uv.x != (1.0 - uv.w) / 2.0 || uv.y != (1.0 - uv.h) / 2.0;
            assert_eq!(dest, Rect::new(0.0, 0.0, VIRTUAL.w, VIRTUAL.h));
        }
        assert!(moved);
        v.tick(1.0);
        let (_, dest, _) = images(&v.draw_list(&all_1080p, None))[0];
        assert_eq!(dest, Rect::new(0.0, 0.0, VIRTUAL.w, VIRTUAL.h), "back in place");
    }

    #[test]
    fn a_strong_shake_never_exposes_the_screen_edge() {
        let mut v = View::new();
        v.apply(&[bg("bg.png", 0.0), Cmd::Shake { time: 1.0, strength: 200.0, axis: Axis::Both }]);
        let mut zoomed = false;
        for _ in 0..200 {
            v.tick(0.005);
            let list = v.draw_list(&all_1080p, None);
            let (_, dest, uv) = images(&list)[0];
            // Clipped to the screen, so a full-screen quad means nothing is uncovered.
            assert!(dest.x.abs() < 1e-2 && dest.y.abs() < 1e-2 && (dest.w - VIRTUAL.w).abs() < 1e-2 && (dest.h - VIRTUAL.h).abs() < 1e-2, "{dest:?}");
            assert!(uv.x >= -1e-4 && uv.y >= -1e-4 && uv.x + uv.w <= 1.0001 && uv.y + uv.h <= 1.0001, "{uv:?}");
            zoomed |= uv.w < 0.99;
        }
        assert!(zoomed);
    }

    #[test]
    fn no_shake_means_no_zoom_and_the_zoom_starts_smoothly() {
        let mut v = View::new();
        v.apply(&[bg("bg.png", 0.0)]);
        let (_, dest, _) = images(&v.draw_list(&all_1080p, None))[0];
        assert_eq!(dest, Rect::new(0.0, 0.0, VIRTUAL.w, VIRTUAL.h));
        v.apply(&[Cmd::Shake { time: 1.0, strength: 200.0, axis: Axis::Both }]);
        let (_, dest, _) = images(&v.draw_list(&all_1080p, None))[0];
        assert_eq!(dest, Rect::new(0.0, 0.0, VIRTUAL.w, VIRTUAL.h), "no pop at the start");
    }

    // ---- notify -----------------------------------------------------------------

    fn notice_draw(list: &[Draw]) -> Option<(&str, f32)> {
        list.iter().find_map(|d| match d {
            Draw::Text { text, color, .. } if text == "Saved" => Some((text.as_str(), color[3])),
            _ => None,
        })
    }

    #[test]
    fn a_notice_fades_in_stays_fades_out_and_is_gone_after_its_time() {
        let mut v = View::new().with_speed(0.0);
        v.apply(&[Cmd::Notify { text: "Saved".into(), time: 2.0, source: String::new() }]);
        assert!(v.busy(), "a running notice needs ticks");
        assert_eq!(notice_draw(&v.draw_list(&all_1080p, None)).unwrap().1, 0.0, "starts faded out");
        v.tick(0.2);
        assert_eq!(notice_draw(&v.draw_list(&all_1080p, None)).unwrap().1, 1.0);
        v.tick(1.0);
        assert_eq!(notice_draw(&v.draw_list(&all_1080p, None)).unwrap().1, 1.0);
        v.tick(0.55); // 1.75 s: 0.25 s left of the 0.5 s fade
        let a = notice_draw(&v.draw_list(&all_1080p, None)).unwrap().1;
        assert!((a - 0.5).abs() < 0.01, "{a}");
        v.tick(0.5);
        assert!(notice_draw(&v.draw_list(&all_1080p, None)).is_none() && !v.busy());
    }

    #[test]
    fn a_notice_does_not_block_ends_when_skipping_and_a_new_one_replaces_it() {
        let mut v = View::new().with_speed(0.0);
        v.apply(&[Cmd::Notify { text: "Saved".into(), time: 5.0, source: String::new() }]);
        v.apply(&[Cmd::Notify { text: "Other".into(), time: 5.0, source: String::new() }]);
        v.tick(0.3);
        assert!(notice_draw(&v.draw_list(&all_1080p, None)).is_none(), "the first one was replaced");
        v.end_flash();
        assert!(!v.busy());
        v.apply(&[Cmd::Notify { text: "Saved".into(), time: 5.0, source: String::new() }, Cmd::ClearScene]);
        assert!(!v.busy(), "a new scene drops it");
    }

    // ---- flash ------------------------------------------------------------------

    /// The full-screen rectangle the flash draws, if any.
    fn flash_rect(list: &[Draw]) -> Option<(usize, [f32; 4])> {
        let screen = Rect::new(0.0, 0.0, VIRTUAL.w, VIRTUAL.h);
        list.iter().enumerate().find_map(|(i, d)| match d {
            Draw::Rect { rect, color } if *rect == screen && color[3] < 1.0 + 1e-6 && color[..3] != [0.0, 0.0, 0.0] => Some((i, *color)),
            _ => None,
        })
    }

    #[test]
    fn a_flash_starts_at_full_strength_and_fades_out() {
        let mut v = View::new();
        v.apply(&[bg("bg.png", 0.0), Cmd::Flash { time: 0.5, color: [1.0, 0.5, 0.0] }]);
        assert!(v.busy(), "a fading flash needs ticks");
        let (_, c) = flash_rect(&v.draw_list(&all_1080p, None)).unwrap();
        assert_eq!(c, [1.0, 0.5, 0.0, 1.0], "jumps to full opacity");
        v.tick(0.25);
        let (_, c) = flash_rect(&v.draw_list(&all_1080p, None)).unwrap();
        assert!(close(c[3], 0.5) && c[..3] == [1.0, 0.5, 0.0]);
        v.tick(0.3);
        assert!(flash_rect(&v.draw_list(&all_1080p, None)).is_none(), "gone after its time");
        assert!(!v.busy());
    }

    #[test]
    fn a_flash_is_over_the_stage_and_images_but_under_the_textbox_and_choices() {
        let mut v = View::new().with_speed(0.0);
        v.apply(&[
            bg("bg.png", 0.0),
            Cmd::ShowImage { src: "pic.png".into(), alt: String::new() },
            say(Some("A"), "hi"),
            Cmd::ShowChoices { options: vec!["Go".into()], locked: Vec::new() },
            Cmd::Flash { time: 1.0, color: [1.0; 3] },
        ]);
        let list = v.draw_list(&all_1080p, None);
        let (at, _) = flash_rect(&list).unwrap();
        let last_image = list.iter().rposition(|d| matches!(d, Draw::Image { .. })).unwrap();
        let textbox = list.iter().position(|d| matches!(d, Draw::Rect { rect, .. } if *rect == TEXTBOX)).unwrap();
        let first_text = list.iter().position(|d| matches!(d, Draw::Text { .. })).unwrap();
        assert!(last_image < at && at < textbox && textbox < first_text, "{at} {last_image} {textbox}");
    }

    #[test]
    fn a_flash_is_a_moment_that_a_redraw_and_a_new_line_do_not_replay() {
        let mut v = View::new();
        v.apply(&[Cmd::Flash { time: 1.0, color: [1.0; 3] }]);
        // the next batch (the player moved on) leaves a running flash alone...
        v.apply(&[say(Some("A"), "next")]);
        assert!(flash_rect(&v.draw_list(&all_1080p, None)).is_some());
        // ...but a redraw (rollback, load, hot reload) clears it and never starts one
        v.apply(&[Cmd::ClearScene, say(Some("A"), "back")]);
        assert!(flash_rect(&v.draw_list(&all_1080p, None)).is_none() && !v.busy());
        v.apply(&[Cmd::Flash { time: 1.0, color: [1.0; 3] }]);
        v.end_flash();
        assert!(flash_rect(&v.draw_list(&all_1080p, None)).is_none());
        // a long tick (skip mode) finishes it at once
        v.apply(&[Cmd::Flash { time: 10.0, color: [1.0; 3] }]);
        v.tick(60.0);
        assert!(flash_rect(&v.draw_list(&all_1080p, None)).is_none());
    }

    // ---- {p} --------------------------------------------------------------------

    #[test]
    fn a_paragraph_break_types_as_one_character_and_counts_as_a_line() {
        let mut v = View::new().with_speed(10.0);
        v.apply(&[say(Some("A"), "Hi.{p}{color=#f00}Ho.{/color}")]);
        let d = v.dialogue.as_ref().unwrap();
        assert_eq!(d.text, "Hi.\nHo.");
        v.tick(0.35); // "Hi."
        assert_eq!(v.dialogue.as_ref().unwrap().visible_text(), "Hi.");
        v.tick(0.1); // the break itself
        assert_eq!(v.dialogue.as_ref().unwrap().visible_text(), "Hi.\n");
        assert!(!v.dialogue.as_ref().unwrap().is_complete());
        v.tick(10.0);
        assert!(v.dialogue.as_ref().unwrap().is_complete());
        let list = v.draw_list(&no_images, None);
        let body = list.iter().find_map(|d| match d {
            Draw::Text { text, runs, visible_chars, .. } if text.contains('\n') => Some((text.clone(), runs.len(), *visible_chars)),
            _ => None,
        });
        assert_eq!(body, Some(("Hi.\nHo.".to_string(), 2, 7)));
        // the long-line estimate sees two lines where there is one short paragraph each
        assert_eq!(dialogue_lines_tagged(&theme(), "Hi.\nHo.", true, 38.0).0, 2);
        assert_eq!(dialogue_lines_tagged(&theme(), "Hi. Ho.", true, 38.0).0, 1);
    }

    #[test]
    fn a_big_shout_is_not_under_counted_in_the_fit_estimate() {
        let plain = "Stop right there, you are not going anywhere near that door tonight, not after what happened. ".repeat(2);
        let plain = plain.as_str();
        let shout = format!("{{size=+40}}{plain}{{/size}}");
        assert!(dialogue_lines_tagged(&theme(), plain, true, 38.0).0 <= 3, "the plain line fits");
        let (small, fit) = dialogue_lines_tagged(&theme(), plain, true, 38.0);
        let (big, _) = dialogue_lines_tagged(&theme(), &shout, true, 38.0);
        assert!(big > small, "{small} vs {big}");
        assert!(big > fit, "the shout cannot fit the textbox ({fit} lines)");
    }

    // ---- speaker colour -------------------------------------------------------

    #[test]
    fn a_big_name_size_shrinks_to_fit_the_plate() {
        let name_draw = |size: f32, name: &str| {
            let theme = Theme { name_size: size, ..Theme::default() };
            let mut v = View::new().with_speed(0.0);
            v.set_theme(theme);
            v.apply(&[say(Some(name), "hi")]);
            v.draw_list(&no_images, None).into_iter().find_map(|d| match d {
                Draw::Text { text, size, rect, .. } if text.starts_with(&name[..2]) => Some((text, size, rect)),
                _ => None,
            })
        };
        let (_, size, rect) = name_draw(50.0, "Mika").unwrap();
        assert!((30.0..50.0).contains(&size) && (size * 1.35).round() <= rect.h + 1.0, "{size}");
        let (_, size, _) = name_draw(36.0, "Mika").unwrap();
        assert_eq!(size, 36.0, "the default fits untouched");
        let long = "Professor Bartholomew Fitzgerald-Montgomery the Third of Nowhere ".repeat(4);
        let (text, size, rect) = name_draw(50.0, &long).unwrap();
        assert!(size >= 30.0 && text.ends_with('…') && crate::estimate_lines(&text, size, rect.w) == 1);
    }

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
        let rects = choice_rects(&theme(), 2);
        assert_eq!(rects.len(), 2);
        assert!(rects[0].y < rects[1].y);
        assert_eq!(rects[0].x, (VIRTUAL.w - CHOICE_W) / 2.0);
        assert!(rects[1].y + rects[1].h < TEXTBOX.y); // both sit above the textbox
        assert_eq!(rects[0].h, CHOICE_H, "few options keep the full size");
        for n in [6, 7, 9, 15] {
            let rects = choice_rects(&theme(), n);
            let last = rects[n - 1];
            assert!(rects[0].y >= CHOICE_MARGIN - 0.01 && last.y + last.h <= TEXTBOX.y - CHOICE_MARGIN + 0.01, "{n} fit");
            assert!(rects.windows(2).all(|w| w[0].y + w[0].h < w[1].y), "{n} don't overlap");
        }

        let mut v = View::new();
        v.apply(&[Cmd::ShowChoices { options: vec!["a".into(), "b".into()], locked: Vec::new() }]);
        assert_eq!(v.choice_at(rects[0].x + 10.0, rects[0].y + 10.0), Some(0));
        assert_eq!(v.choice_at(rects[1].x + 10.0, rects[1].y + 10.0), Some(1));
        assert_eq!(v.choice_at(5.0, 5.0), None);
    }

    #[test]
    fn a_new_batch_clears_choices_images_and_video_but_keeps_the_last_line() {
        let mut v = View::new();
        v.apply(&[say(Some("A"), "hi"), Cmd::ShowImage { src: "x.png".into(), alt: "".into() }]);
        assert_eq!(v.images.len(), 1);
        v.apply(&[Cmd::ShowChoices { options: vec!["go".into()], locked: Vec::new() }]);
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
        Spot { label: label.into(), rect, sprite: None, image: image.map(String::from), hover: None, hidden: false }
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
            map: None,
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
                map: None,
            },
        ]);
        let known = |s: &str| (s == "bg.png" || s == "key.png").then_some(Size::new(1920.0, 1080.0));
        let idle = v.draw_list(&known, None);
        let key = Rect::new(960.0, 540.0, 192.0, 108.0);
        assert!(matches!(&idle[1], Draw::Image { src, dest, .. } if src == "key.png" && *dest == key), "stretched into the rect");
        assert_eq!(idle.len(), 2, "nothing else while idle: unimage'd regions are invisible");

        let hot = v.draw_list_hud(&known, Some(0), Some((970.0, 560.0)), HudLook::default());
        let outline = hot.iter().filter(|d| matches!(d, Draw::Rect { color, .. } if *color == theme().accent_color)).count();
        assert_eq!(outline, 4, "four outline bars");
        let tip = hot.iter().find_map(|d| match d {
            Draw::Text { text, rect, .. } if text == "Key" => Some(*rect),
            _ => None,
        });
        let tip = tip.expect("the label is drawn");
        assert!(close(tip.x, 994.0) && close(tip.y, 588.0), "next to the cursor: {tip:?}");

        // at the bottom right corner the tooltip is pushed back on screen
        v.apply(&[Cmd::ShowHotspots { spots: vec![spot("A rather long place name", [0.9, 0.9, 0.1, 0.1], None)], map: None }]);
        let hot = v.draw_list_hud(&all_1080p, Some(0), Some((1915.0, 1075.0)), HudLook::default());
        let tip = hot.iter().find_map(|d| match d {
            Draw::Text { rect, .. } => Some(*rect),
            _ => None,
        });
        let tip = tip.unwrap();
        assert!(tip.x >= 0.0 && tip.y >= 0.0 && tip.x + tip.w <= VIRTUAL.w && tip.y + tip.h <= VIRTUAL.h, "{tip:?}");
        // keyboard focus (no pointer): under the region, still on screen
        let hot = v.draw_list_hud(&all_1080p, Some(0), None, HudLook::default());
        assert!(hot.iter().any(|d| matches!(d, Draw::Text { rect, .. } if rect.y + rect.h <= VIRTUAL.h)));
    }

    #[test]
    fn a_hidden_hotspot_shows_no_tint_outline_or_tooltip_but_keeps_its_image_and_hit_test() {
        let mut v = View::new();
        let mut s = spot("Scarf", [0.5, 0.5, 0.1, 0.1], Some("key.png"));
        s.hidden = true;
        let mut plain = spot("Nook", [0.0, 0.0, 0.1, 0.1], None);
        plain.hidden = true;
        v.apply(&[Cmd::ShowHotspots { spots: vec![s, plain], map: None }]);
        let known = |s: &str| (s == "key.png").then_some(Size::new(1920.0, 1080.0));
        for hover in [Some(0), Some(1)] {
            let d = v.draw_list_hud(&known, hover, Some((970.0, 560.0)), HudLook::default());
            assert!(!d.iter().any(|d| matches!(d, Draw::Text { .. })), "no tooltip");
            assert!(!d.iter().any(|d| matches!(d, Draw::Rect { .. })), "no tint or outline");
        }
        let d = v.draw_list_hud(&known, Some(1), None, HudLook::default());
        assert!(d.iter().any(|d| matches!(d, Draw::Image { src, .. } if src == "key.png")), "image still drawn");
        assert_eq!(v.hotspot_at(970.0, 560.0), Some(0));
        assert_eq!(v.hotspot_at(20.0, 20.0), Some(1));
    }

    #[test]
    fn hotspots_draw_below_the_textbox() {
        let mut v = View::new();
        v.apply(&[say(None, "Look."), Cmd::ShowHotspots { spots: vec![spot("x", [0.1, 0.1, 0.1, 0.1], Some("k.png"))], map: None }]);
        let list = v.draw_list(&all_1080p, None);
        let key = list.iter().position(|d| matches!(d, Draw::Image { src, .. } if src == "k.png")).unwrap();
        let panel = list.iter().position(|d| matches!(d, Draw::Rect { rect, .. } if *rect == TEXTBOX)).unwrap();
        assert!(key < panel);
    }

    fn mapped(spots: Vec<Spot>, ground: Option<&str>) -> Cmd {
        let map = Imagemap { ground: ground.map(String::from), hover: "map_hover.png".into() };
        Cmd::ShowHotspots { spots, map: Some(map) }
    }

    #[test]
    fn a_hover_picture_replaces_the_image_while_hovered_and_is_the_highlight() {
        let mut v = View::new();
        let mut door = spot("Door", [0.5, 0.5, 0.1, 0.1], Some("door.png"));
        door.hover = Some("door_hover.png".into());
        let mut lamp = spot("Lamp", [0.0, 0.0, 0.1, 0.1], None);
        lamp.hover = Some("lamp_hover.png".into());
        v.apply(&[Cmd::ShowHotspots { spots: vec![door, lamp], map: None }]);
        let srcs = |list: &[Draw]| images(list).iter().map(|(s, _, _)| s.to_string()).collect::<Vec<_>>();
        assert_eq!(srcs(&v.draw_list(&all_1080p, None)), ["door.png"], "no image, no hover picture: nothing while idle");
        let hot = v.draw_list_hud(&all_1080p, Some(0), Some((970.0, 560.0)), HudLook::default());
        assert_eq!(srcs(&hot), ["door_hover.png"], "replaces, not added");
        let at = images(&hot)[0].1;
        assert_eq!(at, Rect::new(960.0, 540.0, 192.0, 108.0));
        assert!(!hot.iter().any(|d| matches!(d, Draw::Rect { color, .. } if *color != theme().panel_color)), "no tint or outline");
        assert!(hot.iter().any(|d| matches!(d, Draw::Text { text, .. } if text == "Door")), "the tooltip stays");
        let hot = v.draw_list_hud(&all_1080p, Some(1), None, HudLook::default());
        assert_eq!(srcs(&hot), ["door.png", "lamp_hover.png"], "a region without an image shows its hover picture when focused");
    }

    #[test]
    fn a_hover_picture_that_is_not_loaded_leaves_the_idle_picture_and_the_outline() {
        let mut v = View::new();
        let mut door = spot("Door", [0.5, 0.5, 0.1, 0.1], Some("door.png"));
        door.hover = Some("door_hover.png".into());
        v.apply(&[Cmd::ShowHotspots { spots: vec![door], map: None }]);
        let unknown_hover = |s: &str| (s != "door_hover.png").then(|| Size::new(100.0, 100.0));
        let hot = v.draw_list_hud(&unknown_hover, Some(0), Some((970.0, 560.0)), HudLook::default());
        let srcs: Vec<_> = images(&hot).iter().map(|(s, _, _)| s.to_string()).collect();
        assert_eq!(srcs, ["door.png"], "the idle picture stays");
        assert!(hot.iter().any(|d| matches!(d, Draw::Rect { color, .. } if *color != theme().panel_color)), "outlined like a plain region");
    }

    #[test]
    fn a_hidden_region_never_shows_its_hover_picture() {
        let mut v = View::new();
        let mut hidden = spot("Nook", [0.0, 0.0, 0.1, 0.1], None);
        hidden.hover = Some("nook_hover.png".into());
        hidden.hidden = true;
        v.apply(&[Cmd::ShowHotspots { spots: vec![hidden], map: None }]);
        assert!(images(&v.draw_list_hud(&all_1080p, Some(0), None, HudLook::default())).is_empty());
    }

    #[test]
    fn an_imagemap_draws_the_ground_then_the_hovered_rect_of_the_hover_picture() {
        let mut v = View::new();
        let spots = vec![spot("Door", [0.5, 0.25, 0.25, 0.5], None), spot("Own", [0.0, 0.0, 0.1, 0.1], Some("own.png"))];
        v.apply(&[mapped(spots, Some("map.png"))]);
        let idle = v.draw_list(&all_1080p, None);
        let idle = images(&idle);
        assert_eq!(idle.len(), 2, "ground and the region's own image");
        assert_eq!((idle[0].0, idle[0].1, idle[0].2), ("map.png", Rect::new(0.0, 0.0, 1920.0, 1080.0), Rect::new(0.0, 0.0, 1.0, 1.0)));
        let hot = v.draw_list_hud(&all_1080p, Some(0), Some((1000.0, 400.0)), HudLook::default());
        let (src, dest, uv) = *images(&hot).last().unwrap();
        assert_eq!(src, "map_hover.png");
        assert_eq!(dest, Rect::new(960.0, 270.0, 480.0, 540.0));
        assert_eq!(uv, Rect::new(0.5, 0.25, 0.25, 0.5), "the same rect of the picture");
        assert!(!hot.iter().any(|d| matches!(d, Draw::Rect { color, .. } if *color != theme().panel_color)), "the picture is the highlight: no tint or outline");
        assert!(hot.iter().any(|d| matches!(d, Draw::Text { text, .. } if text == "Door")), "the tooltip stays");
        // a region with a picture of its own keeps it, without the crop; with no ground the background is the ground
        let hot = v.draw_list_hud(&all_1080p, Some(1), None, HudLook::default());
        assert!(!images(&hot).iter().any(|(s, _, _)| *s == "map_hover.png"));
        v.apply(&[mapped(vec![spot("Door", [0.5, 0.25, 0.25, 0.5], None)], None)]);
        let idle = v.draw_list(&all_1080p, None);
        assert!(images(&idle).is_empty(), "no ground: nothing of its own");
        let hot = v.draw_list_hud(&all_1080p, Some(0), None, HudLook::default());
        assert_eq!(images(&hot).len(), 1);
        v.apply(&[say(None, "next")]);
        assert!(v.imagemap.is_none(), "gone with the screen");
    }

    fn follower(sprite: &str, rect: [f32; 4]) -> Cmd {
        Cmd::ShowHotspots { spots: vec![Spot { label: "Fly".into(), rect, sprite: Some(sprite.into()), image: None, hover: None, hidden: false }], map: None }
    }

    /// Where the first (only) picture is drawn now, and where the first hotspot is: they must agree.
    fn drawn_and_spot(v: &View, size: &dyn Fn(&str) -> Option<Size>) -> (Rect, Rect) {
        let drawn = images(&v.draw_list(size, None))[0].1;
        (drawn, v.spot_rect(&v.hotspots[0]).expect("the sprite is shown"))
    }

    #[test]
    fn a_hotspot_bound_to_a_sprite_is_where_the_sprite_is_drawn_through_moves_scale_stretch_and_flip() {
        let size = |_: &str| Some(Size::new(500.0, 1000.0));
        let mut v = View::new();
        v.apply(&[show("fly", "fly.png", 0.0, 0.0), follower("fly", [0.0, 0.0, 1.0, 1.0])]);
        assert_eq!(v.spot_rect(&v.hotspots[0]), None, "no frame has drawn it yet");
        let (drawn, spot) = drawn_and_spot(&v, &size);
        assert_eq!(spot, drawn);
        assert_eq!(v.hotspot_at(drawn.x + 5.0, drawn.y + 5.0), Some(0));
        let old = (drawn.x + drawn.w / 2.0, drawn.y + drawn.h / 2.0);
        // half way through a slide, with no draw in between: the hotspot has moved with the sprite
        v.apply_each(&[slide("fly", 1.0, 1.0, 1.0, Ease::Linear)], false);
        v.tick(0.5);
        let (drawn, spot) = drawn_and_spot(&v, &size);
        assert_eq!(spot, drawn);
        assert!(drawn.x > 100.0, "it did move: {drawn:?}");
        assert_eq!(v.hotspot_at(old.0, old.1), None, "its old place misses");
        assert_eq!(v.hotspot_at(drawn.x + drawn.w / 2.0, drawn.y + drawn.h / 2.0), Some(0));
        // scale, stretch and flip are the sprite's own
        v.apply_each(&[Cmd::ShowSprite { id: "fly".into(), src: "fly.png".into(), layers: vec![], x: 0.5, y: 1.0, z: 0, scale: 0.5, flip: true, xscale: 2.0, yscale: 1.0, alpha: 1.0, dx: 0.0, dy: 0.0, blur: 0.0, tint: [1.0; 3], additive: false, fade: 0.0 }], false);
        v.tick(10.0);
        let (drawn, spot) = drawn_and_spot(&v, &size);
        assert_eq!(spot, drawn);
        assert!(close(drawn.w, 470.0) && close(drawn.h, 470.0), "{drawn:?}");
        // a rect is a fraction of the sprite: its top right quarter
        v.apply_each(&[follower("fly", [0.5, 0.0, 0.5, 0.5])], false);
        let spot = v.spot_rect(&v.hotspots[0]).unwrap();
        assert_eq!(spot, Rect::new(drawn.x + drawn.w / 2.0, drawn.y, drawn.w / 2.0, drawn.h / 2.0));
        assert_eq!(v.hotspot_at(spot.x + 1.0, spot.y + 1.0), Some(0));
        assert_eq!(v.hotspot_at(drawn.x + 1.0, drawn.y + 1.0), None);
    }

    #[test]
    fn a_sprite_hotspot_follows_a_pixel_sized_sprite_and_the_camera() {
        let size = |_: &str| Some(Size::new(400.0, 300.0));
        let mut v = View::new().with_art_scale(Some(2.0));
        v.apply(&[show("fly", "fly.png", 0.5, 0.0), follower("fly", [0.0, 0.0, 1.0, 1.0])]);
        let (drawn, spot) = drawn_and_spot(&v, &size);
        assert!(close(drawn.w, 800.0) && close(drawn.h, 600.0), "{drawn:?}");
        assert_eq!(spot, drawn);
        v.apply_each(&[Cmd::Camera { camera: Camera { zoom: 2.0, x: 1.0, y: 0.0 }, time: 0.0, ease: Ease::Linear }], false);
        // zoomed 2x: the hotspot is the whole sprite's place, of which the screen edge crops the part drawn
        let (drawn, spot) = drawn_and_spot(&v, &size);
        assert!(close(spot.w, 1600.0) && close(spot.h, 1200.0), "{spot:?}");
        assert!(spot.x <= drawn.x && spot.y <= drawn.y && spot.x + spot.w >= drawn.x + drawn.w, "{spot:?} holds {drawn:?}");
    }

    #[test]
    fn a_hotspot_on_a_layered_sprite_covers_every_layer_whatever_its_size() {
        let size = |s: &str| Some(if s == "body.png" { Size::new(500.0, 1000.0) } else { Size::new(100.0, 100.0) });
        let mut v = View::new();
        let layers = vec![("body".to_string(), "body.png".to_string()), ("face".to_string(), "face.png".to_string())];
        v.apply(&[Cmd::ShowSprite { id: "m".into(), src: String::new(), layers, x: 0.5, y: 1.0, z: 0, scale: 1.0, flip: false, xscale: 1.0, yscale: 1.0, alpha: 1.0, dx: 0.0, dy: 0.0, blur: 0.0, tint: [1.0; 3], additive: false, fade: 0.0 }, follower("m", [0.0, 0.0, 1.0, 1.0])]);
        let drawn: Vec<Rect> = images(&v.draw_list(&size, None)).iter().map(|(_, r, _)| *r).collect();
        assert_eq!(drawn.len(), 2, "{drawn:?}");
        let (l, t) = (drawn.iter().map(|r| r.x).fold(f32::MAX, f32::min), drawn.iter().map(|r| r.y).fold(f32::MAX, f32::min));
        let (r, b) = (drawn.iter().map(|r| r.x + r.w).fold(0.0, f32::max), drawn.iter().map(|r| r.y + r.h).fold(0.0, f32::max));
        let spot = v.spot_rect(&v.hotspots[0]).unwrap();
        assert_eq!(spot, Rect::new(l, t, r - l, b - t), "the hit rect is the union of what is drawn");
        assert!(drawn.iter().any(|d| d.h > 300.0) && spot.h > 300.0, "the big body is in it: {spot:?}");
    }

    #[test]
    fn a_hotspot_of_a_sprite_that_is_not_shown_is_not_clickable() {
        let mut v = View::new();
        v.apply(&[follower("ghost", [0.0, 0.0, 1.0, 1.0])]);
        v.draw_list(&all_1080p, None);
        assert_eq!(v.spot_rect(&v.hotspots[0]), None);
        assert_eq!(v.hotspot_at(960.0, 540.0), None, "nothing shown under the sprite's name");
        v.apply_each(&[show("ghost", "g.png", 0.5, 0.0)], false);
        v.draw_list(&all_1080p, None);
        assert_eq!(v.hotspot_at(960.0, 900.0), Some(0));
        v.apply_each(&[hide("ghost", 1.0)], false);
        v.tick(0.1);
        assert_eq!(v.hotspot_at(960.0, 900.0), None, "fading out counts as gone");
        v.tick(2.0);
        assert_eq!(v.hotspot_at(960.0, 900.0), None);
        assert!(v.draw_list(&all_1080p, None).iter().all(|d| !matches!(d, Draw::Rect { color, .. } if *color == v.theme().accent_color)));
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

    /// The size of the dialogue text in `v`'s draw list.
    fn dialogue_size_drawn(v: &View, text: &str) -> f32 {
        let list = v.draw_list(&no_images, None);
        list.iter().find_map(|d| match d {
            Draw::Text { text: t, size, .. } if t == text => Some(*size),
            _ => None,
        })
        .expect("the line is drawn")
    }

    #[test]
    fn the_text_scale_makes_a_short_line_bigger_and_scale_one_changes_nothing() {
        let size = Theme::default().text_size;
        let mut v = View::new().with_speed(0.0).with_text_scale(1.5);
        v.apply(&[say(None, "hi")]);
        assert!((dialogue_size_drawn(&v, "hi") - size * 1.5).abs() < 1e-3);
        v.set_text_scale(1.0);
        assert_eq!(dialogue_size_drawn(&v, "hi"), size);
    }

    #[test]
    fn a_line_too_long_for_the_scaled_size_is_drawn_at_the_largest_size_that_fits() {
        let theme = Theme::default();
        let size = theme.text_size;
        // grow a line until it fits at the theme's size but not at 1.5 times it
        let mut line = String::from("word");
        while dialogue_lines_tagged(&theme, &line, false, size * 1.5).0 <= dialogue_lines_tagged(&theme, &line, false, size * 1.5).1 {
            line.push_str(" word");
        }
        let (lines, fit) = dialogue_lines_tagged(&theme, &line, false, size);
        assert!(lines <= fit, "fits at the theme's size");
        let mut v = View::new().with_speed(0.0).with_text_scale(1.5);
        v.apply(&[say(None, &line)]);
        let drawn = dialogue_size_drawn(&v, &line);
        assert!(drawn >= size && drawn < size * 1.5, "{drawn}");
        let (lines, fit) = dialogue_lines_tagged(&theme, &line, false, drawn);
        assert!(lines <= fit, "and fits at the size it is drawn at");
        let (lines, fit) = dialogue_lines_tagged(&theme, &line, false, drawn + size * 0.1);
        assert!(lines > fit || drawn + size * 0.1 > size * 1.5 + 1e-3, "no bigger step fits");
        // typing on doesn't change it
        let mut v = View::new().with_speed(10.0).with_text_scale(1.5);
        v.apply(&[say(None, &line)]);
        v.tick(0.5);
        assert_eq!(dialogue_size_drawn(&v, &line), drawn);
    }

    #[test]
    fn hovered_choice_gets_the_highlight_colour() {
        let mut v = View::new();
        v.apply(&[Cmd::ShowChoices { options: vec!["a".into(), "b".into()], locked: Vec::new() }]);
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
    fn a_locked_choice_is_faint_dim_has_its_reason_beneath_and_cannot_be_hovered_or_hit() {
        let mut v = View::new();
        let locked = vec![None, Some("Needs STR 5".into()), Some(String::new())];
        v.apply(&[Cmd::ShowChoices { options: vec!["a".into(), "b".into(), "c".into()], locked }]);
        let rects = choice_rects(&theme(), 3);
        assert_eq!(v.choice_at(rects[0].x + 5.0, rects[0].y + 5.0), Some(0));
        assert_eq!(v.choice_at(rects[1].x + 5.0, rects[1].y + 5.0), None, "a greyed button isn't a target");
        let list = v.draw_list(&no_images, Some(1)); // the pointer on it changes nothing
        let fills: Vec<f32> = list.iter().filter_map(|d| if let Draw::Rect { color, .. } = d { Some(color[3]) } else { None }).collect();
        let a = theme().button_color[3];
        assert_eq!(fills, [a, a * 0.45, a * 0.45]);
        let texts: Vec<(&str, [f32; 4])> =
            list.iter().filter_map(|d| if let Draw::Text { text, color, .. } = d { Some((text.as_str(), *color)) } else { None }).collect();
        let dim = theme().dim_color;
        assert_eq!(texts, [("a", theme().text_color), ("b", dim), ("Needs STR 5", dim), ("c", dim)], "no reason, no second line");
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
        assert!(close(view.sprites[0].x, 0.8)); // `x=80%`
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

        let r = choice_rects(&theme(), 3)[2];
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

    // ---- theme-driven textbox geometry, hiding the box ---------------------------------

    fn themed(text: &str) -> Theme {
        let (t, problems) = Theme::parse(text);
        assert!(problems.is_empty(), "{problems:?}");
        t
    }

    #[test]
    fn the_default_geometry_is_the_old_fixed_layout() {
        let t = theme();
        assert_eq!(textbox_rect(&t), TEXTBOX);
        assert_eq!(textbox_rect(&t), Rect::new(160.0, 780.0, 1600.0, 260.0));
        assert_eq!(name_plate_rect(&t), Rect::new(200.0, 802.0, 1520.0, 48.0));
        assert_eq!(dialogue_body(&t, true), Rect::new(200.0, 860.0, 1520.0, 160.0));
        assert_eq!(dialogue_body(&t, false), Rect::new(200.0, 816.0, 1520.0, 204.0));
        let rects = choice_rects(&t, 3);
        assert_eq!((rects[0].x, rects[0].w, rects[0].h), (460.0, 1000.0, 96.0));
        assert_eq!(rects[0].y, (780.0 - (3.0 * 96.0 + 2.0 * 28.0)) / 2.0);
    }

    #[test]
    fn a_custom_textbox_moves_the_body_the_plate_the_choices_and_the_fit() {
        let t = themed("textbox_rect = 360, 600, 1200, 400\ntextbox_padding = 50, 30, 70, 40\n");
        let tb = textbox_rect(&t);
        assert_eq!(tb, Rect::new(360.0, 600.0, 1200.0, 400.0));
        assert_eq!(name_plate_rect(&t), Rect::new(410.0, 622.0, 1080.0, 48.0));
        assert_eq!(dialogue_body(&t, false), Rect::new(410.0, 630.0, 1080.0, 330.0));
        assert_eq!(dialogue_body(&t, true).y, 680.0, "below the plate (622 + 48 + 10)");
        // the long-line estimate uses the real width and height
        let (_, fit) = dialogue_lines_tagged(&t, "x", true, 40.0);
        assert_eq!(fit, (dialogue_body(&t, true).h / 54.0).floor() as usize);
        assert!(dialogue_lines_tagged(&t, &"word ".repeat(60), false, 38.0).0 > dialogue_lines_tagged(&theme(), &"word ".repeat(60), false, 38.0).0 - 1);
        // choices stack above the textbox top
        let rects = choice_rects(&t, 3);
        assert!(rects.iter().all(|r| r.y + r.h <= 600.0 - CHOICE_MARGIN + 0.01));
        let mut v = View::new().with_theme(t.clone()).with_speed(0.0);
        v.apply(&[say(Some("Mika"), "Hello"), Cmd::ShowChoices { options: vec!["A".into(), "B".into()], locked: Vec::new() }]);
        let r = choice_rects(&t, 2)[1];
        assert_eq!(v.choice_at(r.x + 4.0, r.y + 4.0), Some(1));
        let list = v.draw_list(&no_images, None);
        assert!(list.contains(&Draw::Rect { rect: tb, color: t.panel_color }));
        let body = list.iter().find_map(|d| match d {
            Draw::Text { text, rect, .. } if text == "Hello" => Some(*rect),
            _ => None,
        });
        assert_eq!(body, Some(dialogue_body(&t, true)));
    }

    #[test]
    fn a_name_plate_picture_and_alignment_follow_the_theme() {
        let t = themed("name_plate_image = plate.png\nname_plate_rect = 100, 700, 400, 60\ntext_align = center\nname_align = center\n");
        let mut v = View::new().with_theme(t.clone()).with_speed(0.0);
        v.apply(&[say(Some("Mika"), "Hello")]);
        let list = v.draw_list(&all_1080p, None);
        let plate = Rect::new(100.0, 700.0, 400.0, 60.0);
        assert!(list.iter().any(|d| matches!(d, Draw::Image { src, dest, .. } if src == "plate.png" && *dest == plate)));
        assert!(list.iter().any(|d| matches!(d, Draw::Text { text, rect, align, .. } if text == "Mika" && *rect == plate && *align == Align::CenterX)));
        assert!(list.iter().any(|d| matches!(d, Draw::Text { text, align, .. } if text == "Hello" && *align == Align::CenterX)));
        // the plate is outside the box (above it): the text still starts at the padding
        assert_eq!(dialogue_body(&t, true), dialogue_body(&t, false));
        // narration has no plate; a plate picture that isn't loaded is skipped
        v.apply(&[say(None, "Wind.")]);
        assert!(!v.draw_list(&all_1080p, None).iter().any(|d| matches!(d, Draw::Image { src, .. } if src == "plate.png")));
    }

    #[test]
    fn the_theme_outline_goes_on_dialogue_and_name_text_only() {
        let outlines = |t: Theme| {
            let mut v = View::new().with_theme(t).with_speed(0.0);
            v.apply(&[say(Some("Mika"), "Hello")]);
            v.draw_list(&no_images, None)
                .into_iter()
                .filter_map(|d| match d {
                    Draw::Text { text, outline, .. } => Some((text, outline)),
                    _ => None,
                })
                .collect::<Vec<_>>()
        };
        assert!(outlines(Theme::default()).iter().all(|(_, o)| o.is_none()), "no key: no outline");
        let on = outlines(themed("text_outline_size = 2\ntext_outline_color = #102030\n"));
        let want = Some(Outline { size: 2.0, color: crate::theme::parse_color("#102030").unwrap() });
        assert!(on.contains(&("Mika".into(), want)) && on.contains(&("Hello".into(), want)));
    }

    #[test]
    fn a_hidden_textbox_is_not_drawn_and_fades_in_between() {
        let mut v = View::new().with_speed(0.0);
        v.apply(&[say(Some("Mika"), "Hello")]);
        let boxed = |v: &View| v.draw_list(&no_images, None).iter().filter(|d| matches!(d, Draw::Text { .. } | Draw::Rect { .. })).count();
        assert_eq!(boxed(&v), 3, "panel, name and line");
        v.apply(&[Cmd::Textbox { hidden: true, fade: 0.0 }]);
        assert_eq!(boxed(&v), 0);
        assert!(v.textbox_hidden() && v.box_alpha() == 0.0);
        v.apply(&[Cmd::Textbox { hidden: false, fade: 1.0 }]);
        assert!(!v.textbox_hidden());
        assert_eq!(v.box_alpha(), 0.0, "starts from hidden");
        assert!(v.busy(), "ticks are needed while it fades");
        v.tick(0.5);
        assert!(close(v.box_alpha(), 0.5));
        let panel = v.draw_list(&no_images, None).into_iter().find_map(|d| match d {
            Draw::Rect { color, .. } => Some(color),
            _ => None,
        });
        assert!(close(panel.unwrap()[3], theme().panel_color[3] * 0.5));
        v.tick(1.0);
        assert_eq!(v.box_alpha(), 1.0);
        assert!(!v.busy() || v.dialogue.as_ref().is_some_and(|d| !d.is_complete()));
        // a redraw (rollback, load) starts from a shown box
        v.apply(&[Cmd::Textbox { hidden: true, fade: 0.0 }]);
        v.apply(&[Cmd::ClearScene, say(None, "x")]);
        assert_eq!(v.box_alpha(), 1.0);
    }

    #[test]
    fn hide_ui_hides_the_box_but_keeps_choices_and_the_stage() {
        let mut v = View::new().with_speed(0.0);
        v.apply(&[bg("bg.png", 0.0), say(Some("Mika"), "Hello"), Cmd::ShowChoices { options: vec!["A".into()], locked: Vec::new() }]);
        let n = v.draw_list(&all_1080p, None).len();
        v.set_hide_ui(true);
        let list = v.draw_list(&all_1080p, None);
        assert!(list.len() < n);
        assert!(matches!(&list[0], Draw::Image { src, .. } if src == "bg.png"));
        assert!(list.iter().any(|d| matches!(d, Draw::Text { text, .. } if text == "A")), "choices stay");
        assert!(!list.iter().any(|d| matches!(d, Draw::Text { text, .. } if text == "Hello" || text == "Mika")));
    }

    #[test]
    fn a_hotspot_map_with_a_hidden_box_draws_no_line() {
        let spot = quillstone_runtime::Spot { label: "Door".into(), rect: [0.1, 0.1, 0.2, 0.2], sprite: None, image: None, hover: None, hidden: false };
        let mut v = View::new().with_speed(0.0);
        v.apply(&[say(None, "Where to?"), Cmd::ShowHotspots { spots: vec![spot.clone()], map: None }]);
        assert!(v.draw_list(&no_images, None).iter().any(|d| matches!(d, Draw::Text { text, .. } if text == "Where to?")));
        v.apply(&[Cmd::Textbox { hidden: true, fade: 0.0 }, Cmd::ShowHotspots { spots: vec![spot], map: None }]);
        let list = v.draw_list(&no_images, Some(0));
        assert!(!list.iter().any(|d| matches!(d, Draw::Text { text, .. } if text == "Where to?")));
        assert!(list.iter().any(|d| matches!(d, Draw::Text { text, .. } if text == "Door")), "the tooltip stays");
    }

    // ---- shake axis ----------------------------------------------------------------

    /// Shake `axis` hard and report, over its whole length, the largest movement of the
    /// background's crop along x and along y, and whether the screen edge ever showed.
    fn shake_extent(axis: Axis) -> (f32, f32, bool) {
        let mut v = View::new();
        v.apply(&[bg("bg.png", 0.0), Cmd::Shake { time: 1.0, strength: 200.0, axis }]);
        let (mut mx, mut my, mut exposed) = (0.0_f32, 0.0_f32, false);
        for _ in 0..200 {
            v.tick(0.005);
            let list = v.draw_list(&all_1080p, None);
            let (_, dest, uv) = images(&list)[0];
            exposed |= dest.x.abs() > 1e-2 || dest.y.abs() > 1e-2 || (dest.w - VIRTUAL.w).abs() > 1e-2 || (dest.h - VIRTUAL.h).abs() > 1e-2;
            exposed |= uv.x < -1e-4 || uv.y < -1e-4 || uv.x + uv.w > 1.0001 || uv.y + uv.h > 1.0001;
            // how far the crop is from centred, along each axis
            mx = mx.max((uv.x - (1.0 - uv.w) / 2.0).abs());
            my = my.max((uv.y - (1.0 - uv.h) / 2.0).abs());
        }
        (mx, my, exposed)
    }

    #[test]
    fn a_horizontal_shake_only_moves_along_x_and_exposes_no_edge() {
        let (mx, my, exposed) = shake_extent(Axis::X);
        assert!(mx > 0.01, "it moves sideways ({mx})");
        assert!(my < 1e-5, "and not up or down ({my})");
        assert!(!exposed);
    }

    #[test]
    fn a_vertical_shake_only_moves_along_y_and_exposes_no_edge() {
        let (mx, my, exposed) = shake_extent(Axis::Y);
        assert!(my > 0.01, "it moves up and down ({my})");
        assert!(mx < 1e-5, "and not sideways ({mx})");
        assert!(!exposed);
    }

    #[test]
    fn a_shake_along_both_axes_moves_both_and_exposes_no_edge() {
        let (mx, my, exposed) = shake_extent(Axis::Both);
        assert!(mx > 0.01 && my > 0.01 && !exposed, "{mx} {my} {exposed}");
    }

    #[test]
    fn a_one_axis_shake_zooms_only_as_far_as_that_axis_needs() {
        let shake = |axis| Shake { time: 1.0, strength: 100.0, axis, elapsed: 0.5 };
        let e = shake(Axis::X).envelope();
        assert!(close(shake(Axis::X).cover_zoom(), 1.0 + 2.0 * e / VIRTUAL.w));
        assert!(close(shake(Axis::Y).cover_zoom(), 1.0 + 2.0 * e / VIRTUAL.h));
        assert!(shake(Axis::Both).cover_zoom() >= shake(Axis::X).cover_zoom());
        // the stage is unmoved and unzoomed when the shake is over
        let mut done = shake(Axis::Y);
        done.elapsed = 1.0;
        assert_eq!((done.cover_zoom(), done.offset()), (1.0, (0.0, 0.0)));
    }

    // ---- tint ----------------------------------------------------------------------------

    fn tint_cmd(rgb: [f32; 3], alpha: f32, fade: f32) -> Cmd {
        Cmd::Tint { set: Some(quillstone_script::Tint { color: rgb, alpha }), fade }
    }

    fn screen_rects(list: &[Draw]) -> Vec<(usize, [f32; 4])> {
        let screen = Rect::new(0.0, 0.0, VIRTUAL.w, VIRTUAL.h);
        list.iter()
            .enumerate()
            .filter_map(|(i, d)| match d {
                Draw::Rect { rect, color } if *rect == screen => Some((i, *color)),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn a_tint_is_one_translucent_screen_rect_and_needs_no_ticks_without_a_fade() {
        let mut v = View::new();
        v.apply(&[bg("bg.png", 0.0), tint_cmd([0.1, 0.2, 0.4], 0.35, 0.0)]);
        assert!(!v.busy());
        assert_eq!(screen_rects(&v.draw_list(&all_1080p, None)).iter().map(|r| r.1).collect::<Vec<_>>(), [[0.1, 0.2, 0.4, 0.35]]);
    }

    #[test]
    fn a_tint_eases_from_clear_then_from_where_it_is_and_out_to_nothing() {
        let mut v = View::new();
        v.apply(&[tint_cmd([1.0, 0.0, 0.0], 0.8, 2.0)]);
        assert!(v.busy());
        assert_eq!(v.tint(), None, "starts clear");
        v.tick(1.0);
        let (c, a) = v.tint().unwrap();
        assert!(close(a, 0.4) && c == [1.0, 0.0, 0.0], "{c:?} {a}");
        // A new tint mid-fade eases from the current colour and opacity.
        v.apply(&[tint_cmd([0.0, 0.0, 1.0], 0.2, 1.0)]);
        v.tick(0.5);
        let (c, a) = v.tint().unwrap();
        assert!(close(a, 0.3) && close(c[0], 0.5) && close(c[2], 0.5), "{c:?} {a}");
        v.tick(1.0);
        assert_eq!(v.tint(), Some(([0.0, 0.0, 1.0], 0.2)));
        assert!(!v.busy());
        v.apply(&[Cmd::Tint { set: None, fade: 1.0 }]);
        v.tick(0.5);
        let (c, a) = v.tint().unwrap();
        assert!(close(a, 0.1) && c == [0.0, 0.0, 1.0], "keeps its colour while it fades away");
        v.tick(0.6);
        assert_eq!(v.tint(), None);
        assert!(screen_rects(&v.draw_list(&all_1080p, None)).is_empty());
        v.apply(&[Cmd::Tint { set: None, fade: 1.0 }]);
        assert!(!v.busy(), "removing nothing is nothing");
    }

    #[test]
    fn a_redraw_and_a_settle_finish_it() {
        let mut v = View::new();
        v.apply(&[tint_cmd([1.0; 3], 0.5, 5.0)]);
        v.settle(30.0);
        assert_eq!(v.tint(), Some(([1.0; 3], 0.5)));
        v.apply(&[Cmd::ClearScene]);
        assert_eq!(v.tint(), None);
        v.apply(&[Cmd::ClearScene, tint_cmd([1.0; 3], 0.5, 0.0)]);
        assert_eq!(v.tint(), Some(([1.0; 3], 0.5)));
    }

    #[test]
    fn a_tint_is_over_the_stage_and_inline_images_and_under_the_cover_flash_and_text() {
        let set = |src: &str| Cmd::Particles {
            set: Some(quillstone_script::Particles::new(quillstone_script::ParticleKind::Image(src.into()))),
            fade: 0.0,
        };
        let mut v = View::new();
        v.apply(&[
            bg("bg.png", 0.0),
            show("a", "a.png", 0.5, 0.0),
            set("p.png"),
            Cmd::ShowImage { src: "inline.png".into(), alt: String::new() },
            tint_cmd([0.0, 0.0, 1.0], 0.4, 0.0),
            Cmd::FadeOut { time: 0.0, color: [0.0; 3] },
            Cmd::Flash { time: 1.0, color: [1.0; 3] },
            say(None, "hello"),
        ]);
        let list = v.draw_list(&all_1080p, None);
        let pos = |f: &dyn Fn(&Draw) -> bool| list.iter().position(f).unwrap();
        let tint_at = pos(&|d| matches!(d, Draw::Rect { color, .. } if color == &[0.0, 0.0, 1.0, 0.4]));
        let last_image = list.iter().rposition(|d| matches!(d, Draw::Image { .. })).unwrap();
        assert!(tint_at > last_image, "over background, sprite, particles and inline image");
        let cover_at = pos(&|d| matches!(d, Draw::Rect { color, .. } if color == &[0.0, 0.0, 0.0, 1.0]));
        let flash_at = pos(&|d| matches!(d, Draw::Rect { color, .. } if color[..3] == [1.0; 3]));
        let text_at = pos(&|d| matches!(d, Draw::Text { .. }));
        assert!(tint_at < cover_at && cover_at < flash_at && flash_at < text_at, "{tint_at} {cover_at} {flash_at} {text_at}");
        // Screen space: the camera does not move it.
        v.apply(&[Cmd::Camera { camera: Camera { zoom: 3.0, x: 0.1, y: 0.1 }, time: 0.0, ease: Ease::Linear }]);
        let screen = Rect::new(0.0, 0.0, VIRTUAL.w, VIRTUAL.h);
        assert!(v.draw_list(&all_1080p, None).iter().any(|d| matches!(d, Draw::Rect { rect, color } if *rect == screen && color[2] == 1.0 && color[3] == 0.4)));
        // The scene under an overlay menu has it too.
        assert!(v.draw_scene(&all_1080p).iter().any(|d| matches!(d, Draw::Rect { color, .. } if color == &[0.0, 0.0, 1.0, 0.4])));
    }

    // ---- fade to a colour ----------------------------------------------------------------

    fn cover_rect(list: &[Draw]) -> Option<(usize, [f32; 4])> {
        let screen = Rect::new(0.0, 0.0, VIRTUAL.w, VIRTUAL.h);
        list.iter().enumerate().find_map(|(i, d)| match d {
            Draw::Rect { rect, color } if *rect == screen && color[3] < 1.0 + 1e-6 => Some((i, *color)),
            _ => None,
        })
    }

    #[test]
    fn a_fade_out_rises_to_full_cover_and_stays() {
        let mut v = View::new();
        v.apply(&[bg("bg.png", 0.0), Cmd::FadeOut { time: 1.0, color: [0.0, 0.0, 1.0] }, Cmd::Wait { seconds: 1.0 }]);
        assert!(v.busy());
        assert!(cover_rect(&v.draw_list(&all_1080p, None)).is_none(), "nothing yet at the start");
        v.tick(0.5);
        let (_, c) = cover_rect(&v.draw_list(&all_1080p, None)).unwrap();
        assert!(close(c[3], 0.5) && c[..3] == [0.0, 0.0, 1.0], "{c:?}");
        v.tick(0.6);
        let (_, c) = cover_rect(&v.draw_list(&all_1080p, None)).unwrap();
        assert_eq!(c, [0.0, 0.0, 1.0, 1.0]);
        v.tick(30.0);
        assert!(cover_rect(&v.draw_list(&all_1080p, None)).is_some(), "it stays until faded in");
    }

    #[test]
    fn a_fade_in_clears_the_cover_over_its_time_and_does_not_wait() {
        let mut v = View::new();
        v.apply(&[bg("bg.png", 0.0), Cmd::FadeOut { time: 0.0, color: [0.0; 3] }]);
        assert_eq!(v.cover(), Some(([0.0; 3], 1.0)), "time 0 covers at once (a redraw)");
        v.apply(&[Cmd::FadeIn { time: 1.0 }, say(None, "hi")]);
        assert!(v.busy());
        v.tick(0.25);
        assert!(close(v.cover().unwrap().1, 0.75));
        v.tick(0.8);
        assert_eq!(v.cover(), None);
        assert!(cover_rect(&v.draw_list(&all_1080p, None)).is_none());
        // a fade in with nothing covering is nothing
        v.apply(&[Cmd::FadeIn { time: 1.0 }]);
        assert_eq!(v.cover(), None);
    }

    #[test]
    fn the_next_batch_completes_a_fade_out_that_a_click_cut_short() {
        let mut v = View::new();
        v.apply(&[Cmd::FadeOut { time: 2.0, color: [0.0; 3] }, Cmd::Wait { seconds: 2.0 }]);
        v.tick(0.5);
        assert!(close(v.cover().unwrap().1, 0.25));
        v.apply(&[say(None, "in the dark")]);
        assert_eq!(v.cover(), Some(([0.0; 3], 1.0)), "the click ended the fade: fully covered");
        // ...but a fade in under way is not cut short by the next batch
        v.apply(&[Cmd::FadeIn { time: 1.0 }]);
        v.tick(0.25);
        v.apply(&[say(None, "again")]);
        assert!(close(v.cover().unwrap().1, 0.75));
        // skip mode ends both kinds at once
        v.end_cover_fade();
        assert_eq!(v.cover(), None);
        v.apply(&[Cmd::FadeOut { time: 2.0, color: [1.0; 3] }]);
        v.end_cover_fade();
        assert_eq!(v.cover(), Some(([1.0; 3], 1.0)));
    }

    #[test]
    fn a_redraw_replaces_the_cover() {
        let mut v = View::new();
        v.apply(&[Cmd::FadeOut { time: 0.0, color: [1.0, 0.0, 0.0] }]);
        v.apply(&[Cmd::ClearScene, bg("bg.png", 0.0)]);
        assert_eq!(v.cover(), None, "a rollback to before the fade uncovers");
        v.apply(&[Cmd::ClearScene, Cmd::FadeOut { time: 0.0, color: [0.0, 1.0, 0.0] }]);
        assert_eq!(v.cover(), Some(([0.0, 1.0, 0.0], 1.0)));
    }

    #[test]
    fn the_cover_is_over_the_picture_and_hotspot_art_under_the_flash_and_the_textbox() {
        let mut v = View::new();
        v.apply(&[
            bg("bg.png", 0.0),
            show("m", "m.png", 0.5, 0.0),
            Cmd::ShowImage { src: "inline.png".into(), alt: String::new() },
            Cmd::ShowHotspots { spots: vec![Spot { label: "Map".into(), rect: [0.0, 0.0, 0.2, 0.2], sprite: None, image: Some("art.png".into()), hover: None, hidden: false }], map: None },
            say(Some("A"), "hello"),
            Cmd::FadeOut { time: 0.0, color: [0.0; 3] },
            Cmd::Flash { time: 1.0, color: [1.0; 3] },
        ]);
        let list = v.draw_list(&all_1080p, None);
        let cover = list.iter().position(|d| matches!(d, Draw::Rect { rect, color } if *rect == Rect::new(0.0, 0.0, VIRTUAL.w, VIRTUAL.h) && color[..3] == [0.0; 3])).unwrap();
        let flash = list.iter().position(|d| matches!(d, Draw::Rect { rect, color } if *rect == Rect::new(0.0, 0.0, VIRTUAL.w, VIRTUAL.h) && color[..3] == [1.0; 3])).unwrap();
        let last_image = list.iter().rposition(|d| matches!(d, Draw::Image { .. })).unwrap();
        let first_text = list.iter().position(|d| matches!(d, Draw::Text { .. })).unwrap();
        let textbox = list.iter().position(|d| matches!(d, Draw::Rect { rect, .. } if *rect == textbox_rect(&Theme::default()))).unwrap();
        assert!(last_image < cover, "above the stage, inline image and hotspot art");
        assert!(cover < flash, "the flash is above the cover");
        assert!(flash < textbox && flash < first_text, "the textbox and its text are not covered");
        // choices are not covered either
        v.apply(&[Cmd::ShowChoices { options: vec!["Yes".into()], locked: Vec::new() }]);
        let list = v.draw_list(&all_1080p, None);
        let cover = cover_rect(&list).map(|(i, _)| i).unwrap();
        assert!(list.iter().skip(cover + 1).any(|d| matches!(d, Draw::Text { text, .. } if text == "Yes")));
    }

    #[test]
    fn a_covered_screen_still_draws_the_dialogue_over_it() {
        let mut v = View::new();
        v.apply(&[bg("bg.png", 0.0), Cmd::FadeOut { time: 0.0, color: [0.0; 3] }, say(None, "Black.")]);
        v.tick(0.0);
        let list = v.draw_list(&all_1080p, None);
        assert!(list.iter().any(|d| matches!(d, Draw::Text { text, .. } if text == "Black.")));
        // while an overlay screen asks only for the picture, the cover is part of it
        assert!(cover_rect(&v.draw_scene(&all_1080p)).is_some());
    }

    // ---- flip and per-axis scale ------------------------------------------------------------

    fn look_show(id: &str, flip: bool, xscale: f32, yscale: f32, x: f32) -> Cmd {
        Cmd::ShowSprite { id: id.into(), src: format!("{id}.png"), layers: vec![], x, y: 1.0, z: 0, scale: 1.0, flip, xscale, yscale, alpha: 1.0, dx: 0.0, dy: 0.0, blur: 0.0, tint: [1.0; 3], additive: false, fade: 0.0 }
    }

    #[test]
    fn a_flipped_sprite_reads_its_texture_right_to_left_and_keeps_its_place() {
        let mut v = View::new();
        v.apply(&[look_show("m", true, 1.0, 1.0, 0.5)]);
        let size = |_: &str| Some(Size::new(500.0, 1000.0));
        let list = v.draw_list(&size, None);
        let (_, dest, uv) = images(&list)[0];
        assert_eq!(uv, Rect::new(1.0, 0.0, -1.0, 1.0));
        let mut plain = View::new();
        plain.apply(&[look_show("m", false, 1.0, 1.0, 0.5)]);
        assert_eq!(images(&plain.draw_list(&size, None))[0].1, dest, "same size and position, only mirrored");
        // the batch turns the corners into [u0, v0, u1, v1] with u0 > u1: a mirrored quad
        let frame = crate::build_frame(&list, Size::new(1920.0, 1080.0));
        assert_eq!(frame.quads[0].uv, [1.0, 0.0, 0.0, 1.0]);
    }

    #[test]
    fn pixel_sized_sprites_use_the_pictures_own_size_times_the_art_scale() {
        let size = |_: &str| Some(Size::new(1280.0, 720.0));
        let mut v = View::new().with_art_scale(Some(1.5));
        v.apply(&[sprite("m", 0.5, 1.0, 0)]);
        let dest = images(&v.draw_list(&size, None))[0].1;
        assert!(close(dest.w, VIRTUAL.w) && close(dest.h, VIRTUAL.h) && close(dest.x, 0.0) && close(dest.y, 0.0), "fills the stage: {dest:?}");
        // scale and yscale multiply on top; the fixed height ignores the picture's size
        v.apply(&[look_show("m", false, 1.0, 0.5, 0.5)]);
        let dest = images(&v.draw_list(&size, None))[0].1;
        assert!(close(dest.w, VIRTUAL.w) && close(dest.h, VIRTUAL.h / 2.0), "{dest:?}");
        let mut fixed = View::new();
        fixed.apply(&[sprite("m", 0.5, 1.0, 0)]);
        assert!(close(images(&fixed.draw_list(&size, None))[0].1.h, SPRITE_HEIGHT));
    }

    #[test]
    fn xscale_and_yscale_stretch_one_axis_from_the_feet_and_the_centre() {
        let mut v = View::new();
        v.apply(&[look_show("m", false, 2.0, 0.5, 0.25)]);
        let list = v.draw_list(&|_| Some(Size::new(500.0, 1000.0)), None);
        let (_, dest, _) = images(&list)[0];
        // a 1:2 picture at scale 1 is 940 tall and 470 wide: twice as wide, half as tall
        assert!(close(dest.w, 940.0) && close(dest.h, 470.0), "{dest:?}");
        assert!(close(dest.y + dest.h, VIRTUAL.h), "still standing on the bottom edge");
        assert!(close(dest.x, 0.25 * (VIRTUAL.w - dest.w)), "still placed by x on its new size");
        // multiplied with scale
        v.apply(&[Cmd::ShowSprite { id: "m".into(), src: "m.png".into(), layers: vec![], x: 0.25, y: 1.0, z: 0, scale: 0.5, flip: false, xscale: 2.0, yscale: 0.5, alpha: 1.0, dx: 0.0, dy: 0.0, blur: 0.0, tint: [1.0; 3], additive: false, fade: 0.0 }]);
        let list = v.draw_list(&|_| Some(Size::new(500.0, 1000.0)), None);
        assert!(close(images(&list)[0].1.w, 470.0) && close(images(&list)[0].1.h, 235.0));
    }

    #[test]
    fn a_move_animates_the_scales_and_switches_flip_at_its_start() {
        let mut v = View::new();
        v.apply(&[look_show("m", false, 1.0, 1.0, 0.5)]);
        v.apply(&[Cmd::MoveSprite { id: "m".into(), flip: true, to: SpriteStep { x: 0.5, y: 1.0, scale: 1.0, xscale: 2.0, yscale: 0.5, alpha: 1.0, dx: 0.0, dy: 0.0, blur: 0.0, tint: [1.0; 3], time: 1.0, ease: Ease::Linear, wait: 0.0 }, queued: false }]);
        let size = |_: &str| Some(Size::new(500.0, 1000.0));
        let at = |v: &View| {
            let list = v.draw_list(&size, None);
            let (_, dest, uv) = images(&list)[0];
            (dest, uv)
        };
        let (dest, uv) = at(&v);
        assert_eq!(uv.w, -1.0, "mirrored from the first frame");
        assert!(close(dest.w, 470.0) && close(dest.h, 940.0));
        v.tick(0.5);
        let (dest, _) = at(&v);
        assert!(close(dest.w, 470.0 * 1.5) && close(dest.h, 940.0 * 0.75), "{dest:?}");
        v.tick(0.6);
        let (dest, _) = at(&v);
        assert!(close(dest.w, 940.0) && close(dest.h, 470.0));
    }

    #[test]
    fn a_flipped_sprite_is_cropped_at_the_screen_edge_with_the_right_part_of_the_picture() {
        let size = |_: &str| Some(Size::new(500.0, 1000.0)); // 470 wide
        for flip in [false, true] {
            // placed so the left half (235 px) hangs off the screen
            let mut v = View::new();
            v.apply(&[look_show("m", flip, 1.0, 1.0, -235.0 / 1920.0)]);
            let list = v.draw_list(&size, None);
            let (_, dest, uv) = images(&list)[0];
            assert!(close(dest.x, 0.0) && close(dest.w, 235.0), "{dest:?}");
            // what stays on screen is the picture's right half; mirrored, that is the texture's left half
            let (from, to) = (uv.x, uv.x + uv.w);
            if flip {
                assert!(close(from, 0.5) && close(to, 0.0), "{uv:?}");
            } else {
                assert!(close(from, 0.5) && close(to, 1.0), "{uv:?}");
            }
            // fully off screen: nothing is drawn
            let mut off = View::new();
            off.apply(&[look_show("m", flip, 1.0, 1.0, -0.5)]);
            assert!(images(&off.draw_list(&size, None)).is_empty());
        }
    }

    #[test]
    fn a_pose_change_crossfade_keeps_the_flip_for_both_poses() {
        let mut v = View::new();
        v.apply(&[look_show("m", true, 1.0, 1.0, 0.5)]);
        v.apply(&[Cmd::ShowSprite { id: "m".into(), src: "m2.png".into(), layers: vec![], x: 0.5, y: 1.0, z: 0, scale: 1.0, flip: true, xscale: 1.0, yscale: 1.0, alpha: 1.0, dx: 0.0, dy: 0.0, blur: 0.0, tint: [1.0; 3], additive: false, fade: 1.0 }]);
        v.tick(0.5);
        let list = v.draw_list(&all_1080p, None);
        let flipped: Vec<_> = images(&list).into_iter().map(|(_, _, uv)| uv.w < 0.0).collect();
        assert_eq!(flipped, [true, true]);
    }
}
