//! What is on screen right now, and the backend-neutral list of things to draw.
//!
//! Feed it the runtime's [`Cmd`]s with [`View::apply`], call [`View::tick`] every
//! frame, and render [`View::draw_list`]. No GPU, window or fonts involved.
//!
//! The view owns only *presentation*: fade progress, sprite slides, the typewriter.
//! What is actually in the scene lives in the runtime's `State`, and a rollback
//! sends `Cmd::ClearScene` + the scene again, so the two never drift apart.

use std::collections::BTreeMap;

use quillstone_runtime::{Camera, Cmd, Spot};
use quillstone_script::markup::{rich, Rich};
pub use quillstone_script::markup::TextRun;
use quillstone_script::{anim_id, Anim, Axis, Ease, Fit, HudAnchor, Wipe};

use crate::hud::HudLayer;
use crate::particles::ParticleLayer;
use crate::layout::{clip, estimate_lines, is_wide, one_row, place, Rect, Size, VIRTUAL};
use crate::theme::Theme;

/// The dialogue box, in virtual coordinates.
/// This is the default; a theme may move it (`textbox_rect`, see [`textbox_rect`]).
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
    /// Each line centred across the rect, and the whole text centred down it (buttons).
    Center,
    /// Each line centred across the rect, the text starting at the top (centred dialogue).
    CenterX,
    /// Each line against the right edge of the rect, the text starting at the top.
    Right,
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
    pub hiding: bool,
    fade: f32,
    elapsed: f32,
    from_x: f32,
    from_y: f32,
    from_scale: f32,
    from_xscale: f32,
    from_yscale: f32,
    move_time: f32,
    move_elapsed: f32,
    move_ease: Ease,
    /// A looping move: the other end of the oscillation (x, y, scale, xscale, yscale);
    /// the sprite rests at `x`, `y`, ... The current leg heads for it while `out`.
    loop_far: Option<[f32; 5]>,
    out: bool,
    /// Seconds since this sprite's picture started being shown (an animation's clock),
    /// and the same for the outgoing picture of a pose change.
    anim_time: f32,
    previous_anim_time: f32,
}

impl SpriteView {
    fn new(id: &str, (src, layers): (&str, &[(String, String)]), x: f32, y: f32, z: i32, look: Look, fade: f32) -> SpriteView {
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
            hiding: false,
            fade,
            elapsed: 0.0,
            from_x: x,
            from_y: y,
            from_scale: look.scale,
            from_xscale: look.xscale,
            from_yscale: look.yscale,
            move_time: 0.0,
            move_elapsed: 0.0,
            move_ease: Ease::Linear,
            loop_far: None,
            out: false,
            anim_time: 0.0,
            previous_anim_time: 0.0,
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

    /// Where the current leg of a move (or loop) is heading: x, y, scale, xscale, yscale.
    fn target(&self) -> [f32; 5] {
        match (&self.loop_far, self.out) {
            (Some(far), true) => *far,
            _ => [self.x, self.y, self.scale, self.xscale, self.yscale],
        }
    }

    /// Where the current leg started.
    fn from(&self) -> [f32; 5] {
        [self.from_x, self.from_y, self.from_scale, self.from_xscale, self.from_yscale]
    }

    fn set_from(&mut self, v: [f32; 5]) {
        [self.from_x, self.from_y, self.from_scale, self.from_xscale, self.from_yscale] = v;
    }

    /// Whether the sprite oscillates (a looping move is running).
    pub fn is_looping(&self) -> bool {
        self.loop_far.is_some()
    }

    /// The value `k` (0 x, 1 y, 2 scale, 3 xscale, 4 yscale) right now.
    fn now(&self, k: usize) -> f32 {
        let (from, to) = (self.from()[k], self.target()[k]);
        from + (to - from) * self.move_progress()
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
        Look { scale: self.current_scale(), xscale: self.current_xscale(), yscale: self.current_yscale(), flip: self.flip }
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
                    let start = if self.out { [self.x, self.y, self.scale, self.xscale, self.yscale] } else { self.loop_far.unwrap_or_default() };
                    self.set_from(start);
                }
            }
        } else if self.move_time > 0.0 {
            self.move_elapsed += dt;
            if self.move_elapsed >= self.move_time {
                self.move_time = 0.0;
                self.from_x = self.x;
                self.from_y = self.y;
                self.from_scale = self.scale;
                self.from_xscale = self.xscale;
                self.from_yscale = self.yscale;
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
    alpha: f32,
    /// 1 while fading out or covered, 0 while fading in.
    target: f32,
    /// Opacity change per second (0: at once).
    rate: f32,
}

/// The colour wash over the stage (`::tint`), easing from where it was to `color`/`alpha`.
#[derive(Debug, Clone, PartialEq)]
struct TintFade {
    from: ([f32; 3], f32),
    to: ([f32; 3], f32),
    /// 0 at the start of the ease, 1 when it has arrived.
    progress: f32,
    /// Progress per second (0: at once).
    rate: f32,
}

impl TintFade {
    fn current(&self) -> ([f32; 3], f32) {
        let (a, b, t) = (self.from, self.to, self.progress);
        let mix = |x: f32, y: f32| x + (y - x) * t;
        ([mix(a.0[0], b.0[0]), mix(a.0[1], b.0[1]), mix(a.0[2], b.0[2])], mix(a.1, b.1))
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
    flash: Option<Flash>,
    notice: Option<Notice>,
    cover: Option<Cover>,
    hud: HudLayer,
    particles: ParticleLayer,
    tint: Option<TintFade>,
    /// `::textbox{hide}` is in effect, how visible the box is now (0..1, fading), and
    /// how fast it fades (per second; 0 = at once).
    box_hidden: bool,
    box_alpha: f32,
    box_rate: f32,
    /// The player hid the interface (the `U` key); see [`View::set_hide_ui`].
    hide_ui: bool,
    /// Seconds left of a `::wait`, if one is running.
    wait: Option<f32>,
    chars_per_sec: f32,
    theme: Theme,
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
            sprites: Vec::new(),
            images: Vec::new(),
            dialogue: None,
            choices: Vec::new(),
            hotspots: Vec::new(),
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
            box_hidden: false,
            box_alpha: 1.0,
            box_rate: 0.0,
            hide_ui: false,
            wait: None,
            chars_per_sec: DEFAULT_CHARS_PER_SEC,
            theme: Theme::default(),
            anims: BTreeMap::new(),
            bg_clock: 0.0,
            previous_bg_clock: 0.0,
        }
    }

    /// Know the script's frame animations (`anim:<id>` pictures play them). Clocks
    /// are visual state: every animation starts at its first frame when it appears.
    pub fn with_anims(mut self, anims: &BTreeMap<String, Anim>) -> View {
        self.set_anims(anims);
        self
    }

    pub fn set_anims(&mut self, anims: &BTreeMap<String, Anim>) {
        self.anims = anims.clone();
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
                    self.box_hidden = false;
                    self.box_alpha = 1.0;
                }
                Cmd::Textbox { hidden, fade } => {
                    self.box_hidden = *hidden;
                    self.box_rate = if *fade > 0.0 { 1.0 / fade } else { 0.0 };
                    if *fade <= 0.0 {
                        self.box_alpha = if *hidden { 0.0 } else { 1.0 };
                    }
                }
                Cmd::SetBackground { src, fade, wipe } => self.set_background(src, *fade, *wipe),
                Cmd::ShowSprite { id, src, layers, x, y, z, scale, flip, xscale, yscale, fade } => {
                    let look = Look { scale: *scale, xscale: *xscale, yscale: *yscale, flip: *flip };
                    self.show_sprite(id, (src, layers), (*x, *y), *z, look, *fade)
                }
                Cmd::HideSprite { id, fade } => self.hide_sprite(id, *fade),
                Cmd::LoopSprite { id, x, y, scale, flip, xscale, yscale, time, ease } => {
                    if let Some(s) = self.sprites.iter_mut().find(|s| s.id == *id && !s.hiding) {
                        // The first leg glides from wherever the sprite is to the far values; the
                        // resting values (x, y, ...) stay where they are.
                        let here = [s.current_x(), s.current_y(), s.current_scale(), s.current_xscale(), s.current_yscale()];
                        s.set_from(here);
                        s.loop_far = Some([*x, *y, *scale, *xscale, *yscale]);
                        s.out = true;
                        s.flip = *flip;
                        s.move_time = *time;
                        s.move_elapsed = 0.0;
                        s.move_ease = *ease;
                    }
                }
                Cmd::MoveSprite { id, x, y, scale, flip, xscale, yscale, time, ease } => {
                    if let Some(s) = self.sprites.iter_mut().find(|s| s.id == *id && !s.hiding) {
                        let here = [s.current_x(), s.current_y(), s.current_scale(), s.current_xscale(), s.current_yscale()];
                        s.set_from(here);
                        s.loop_far = None; // a plain move ends a loop
                        s.out = false;
                        s.y = *y;
                        s.x = *x;
                        s.scale = *scale;
                        s.flip = *flip; // mirroring can't be animated: it switches as the move starts
                        s.xscale = *xscale;
                        s.yscale = *yscale;
                        s.move_time = *time;
                        s.move_elapsed = 0.0;
                        s.move_ease = *ease;
                    }
                }
                Cmd::Shake { time, strength, axis } => {
                    self.shake = Some(Shake { time: *time, strength: *strength, axis: *axis, elapsed: 0.0 });
                }
                Cmd::FadeOut { time, color } => {
                    let cover = self.cover.get_or_insert(Cover { color: *color, alpha: 0.0, target: 1.0, rate: 0.0 });
                    cover.color = *color;
                    cover.target = 1.0;
                    cover.rate = if *time > 0.0 { 1.0 / time } else { 0.0 };
                    if *time <= 0.0 {
                        cover.alpha = 1.0;
                    }
                }
                Cmd::FadeIn { time } => {
                    if let Some(cover) = &mut self.cover {
                        cover.target = 0.0;
                        cover.rate = if *time > 0.0 { 1.0 / time } else { 0.0 };
                    }
                    if *time <= 0.0 {
                        self.cover = None;
                    }
                }
                Cmd::HudShow { .. } | Cmd::HudHide { .. } | Cmd::HudClear { .. } => self.hud.apply(cmd),
                Cmd::Particles { set, fade } => self.particles.apply(set, *fade),
                Cmd::Tint { set, fade } => self.set_tint(set.map(|t| (t.color, t.alpha)), *fade),
                Cmd::Flash { time, color } => {
                    self.flash = Some(Flash { time: *time, color: *color, elapsed: 0.0 });
                }
                Cmd::Notify { text, time } => {
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
                Cmd::Stopped { .. } | Cmd::Warning { .. } => {} // for the author and player; the game shows it as a notice
                Cmd::ReplayReached { .. } | Cmd::Unlock { .. } | Cmd::EndReplay => {} // bookkeeping for the session and the game
                // The inventory, shop and text entry are screens of their own (see quillstone-ui).
                Cmd::ShowInventory | Cmd::ShowShop { .. } | Cmd::AskInput { .. } => {}
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

    fn set_background(&mut self, src: &str, fade: f32, wipe: Option<Wipe>) {
        if self.background.as_ref().is_some_and(|b| b.src == src) {
            return;
        }
        self.previous_background = if fade > 0.0 { self.background.take().map(|b| b.src) } else { None };
        self.previous_bg_clock = self.bg_clock;
        self.bg_clock = 0.0;
        self.background = Some(Background { src: src.to_string(), fade, wipe, elapsed: 0.0 });
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
            self.box_alpha
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
        if self.cover.as_ref().is_some_and(|c| c.target == 0.0) {
            self.cover = None;
        }
    }

    fn finish_fade_out(&mut self) {
        if let Some(c) = self.cover.as_mut().filter(|c| c.target == 1.0) {
            c.alpha = 1.0;
        }
    }

    /// Ease the tint to `set` (`None`: to nothing) from what it is now, or from clear.
    fn set_tint(&mut self, set: Option<([f32; 3], f32)>, fade: f32) {
        let now = self.tint();
        let to = match (set, now) {
            (Some(to), _) => to,
            (None, Some((color, _))) => (color, 0.0),
            (None, None) => return,
        };
        let from = now.unwrap_or((to.0, 0.0));
        let rate = if fade > 0.0 { 1.0 / fade } else { 0.0 };
        self.tint = Some(TintFade { from, to, progress: if rate > 0.0 { 0.0 } else { 1.0 }, rate });
        self.drop_cleared_tint();
    }

    fn drop_cleared_tint(&mut self) {
        if self.tint.as_ref().is_some_and(|t| t.progress >= 1.0 && t.to.1 <= 0.0) {
            self.tint = None;
        }
    }

    /// The colour and opacity of the tint right now, if there is one showing.
    pub fn tint(&self) -> Option<([f32; 3], f32)> {
        self.tint.as_ref().map(TintFade::current).filter(|(_, alpha)| *alpha > 0.0)
    }

    /// The colour and opacity the stage is covered with right now, if it is.
    pub fn cover(&self) -> Option<([f32; 3], f32)> {
        self.cover.as_ref().filter(|c| c.alpha > 0.0).map(|c| (c.color, c.alpha))
    }

    /// Where the camera is right now.
    pub fn camera(&self) -> Camera {
        self.camera.current()
    }

    fn show_sprite(&mut self, id: &str, (src, layers): (&str, &[(String, String)]), (x, y): (f32, f32), z: i32, look: Look, fade: f32) {
        // A sprite that is mid-hide and shown again starts over.
        self.sprites.retain(|s| s.id != id || !s.hiding);
        match self.sprites.iter_mut().find(|s| s.id == id) {
            Some(s) => {
                let (here, here_y) = (s.current_x(), s.current_y());
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
                let rest = [s.x, s.y, s.scale, s.xscale, s.yscale];
                if s.loop_far.is_some() && rest == [x, y, look.scale, look.xscale, look.yscale] {
                    s.z = z;
                    s.flip = look.flip;
                    return;
                }
                s.loop_far = None;
                s.out = false;
                s.from_x = here;
                s.from_y = here_y;
                s.x = x;
                s.y = y;
                s.z = z;
                s.move_time = 0.0;
                s.scale = look.scale;
                s.from_scale = look.scale;
                s.flip = look.flip;
                s.xscale = look.xscale;
                s.from_xscale = look.xscale;
                s.yscale = look.yscale;
                s.from_yscale = look.yscale;
            }
            None => self.sprites.push(SpriteView::new(id, (src, layers), x, y, z, look, fade)),
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
        for s in &mut self.sprites {
            s.tick(dt, animate);
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
            c.alpha = if c.rate > 0.0 {
                let step = c.rate * dt;
                if c.alpha < c.target { (c.alpha + step).min(c.target) } else { (c.alpha - step).max(c.target) }
            } else {
                c.target
            };
            if c.target == 0.0 && c.alpha <= 0.0 {
                self.cover = None;
            }
        }
        if let Some(t) = &mut self.tint {
            t.progress = if t.rate > 0.0 { (t.progress + t.rate * dt).min(1.0) } else { 1.0 };
        }
        self.drop_cleared_tint();
        self.sprites.retain(|s| !s.is_gone());
        self.hud.tick(dt);
        self.particles.tick(dt, animate);
        let target = if self.box_hidden { 0.0 } else { 1.0 };
        if self.box_rate > 0.0 {
            let step = self.box_rate * dt;
            self.box_alpha = if self.box_alpha < target { (self.box_alpha + step).min(target) } else { (self.box_alpha - step).max(target) };
        } else {
            self.box_alpha = target;
        }
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
            || self.cover.as_ref().is_some_and(|c| c.alpha != c.target)
            || self.tint.as_ref().is_some_and(|t| t.progress < 1.0)
            || self.hud.fading()
            || self.box_alpha != if self.box_hidden { 0.0 } else { 1.0 }
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

    /// Which choice button (if any) contains this virtual-space point.
    pub fn choice_at(&self, x: f32, y: f32) -> Option<usize> {
        choice_rects(&self.theme, self.choices.len()).iter().position(|r| r.contains(x, y))
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
        self.draw_list_ui(image_size, hover, pointer, true)
    }

    /// Only the picture: stage, inline images, video, flash and hotspot art, without
    /// the textbox, choices, hotspot outline or tooltip. What a full-screen overlay
    /// (settings, backlog, ...) leaves under its dim.
    pub fn draw_scene(&self, image_size: &dyn Fn(&str) -> Option<Size>) -> Vec<Draw> {
        self.draw_list_ui(image_size, None, None, false)
    }

    fn draw_list_ui(
        &self,
        image_size: &dyn Fn(&str) -> Option<Size>,
        hover: Option<usize>,
        pointer: Option<(f32, f32)>,
        ui: bool,
    ) -> Vec<Draw> {
        let mut out = Vec::new();
        let screen = Rect::new(0.0, 0.0, VIRTUAL.w, VIRTUAL.h);

        // The stage: background and sprites, seen through the camera and shake.
        let mut stage = Vec::new();
        if let Some(prev) = &self.previous_background {
            push_backdrop(&mut stage, self.picture(prev, self.previous_bg_clock), screen, 1.0, image_size);
        }
        if let Some(bg) = &self.background {
            let mut picture = Vec::new();
            push_backdrop(&mut picture, self.picture(&bg.src, self.bg_clock), screen, bg.alpha(), image_size);
            for draw in picture {
                match (draw, bg.revealed()) {
                    (Draw::Image { src, dest, uv, alpha }, Some(part)) => {
                        if let Some((dest, uv)) = clip(dest, uv, part) {
                            stage.push(Draw::Image { src, dest, uv, alpha });
                        }
                    }
                    (Draw::Rect { rect, color }, Some(part)) => {
                        if let Some((rect, _)) = clip(rect, Rect::new(0.0, 0.0, 1.0, 1.0), part) {
                            stage.push(Draw::Rect { rect, color });
                        }
                    }
                    (draw, _) => stage.push(draw),
                }
            }
        }
        let bg_end = stage.len();
        // Back to front by z; the sort is stable, so equal z keeps the order sprites were shown in.
        let mut order: Vec<&SpriteView> = self.sprites.iter().collect();
        order.sort_by_key(|s| s.z);
        for s in order {
            let (x, y, look) = (s.current_x(), s.current_y(), s.look());
            if let Some(prev) = &s.previous_src {
                push_sprite(&mut stage, self.picture(prev, s.previous_anim_time), (x, y), look, 1.0, image_size);
            }
            // The layers of a layered sprite: the same place and size for each, back to front, and
            // each one crossfades on its own when its picture changes.
            for (src, t) in &s.previous_layers {
                push_sprite(&mut stage, self.picture(src, *t), (x, y), look, 1.0, image_size);
            }
            push_sprite(&mut stage, self.picture(&s.src, s.anim_time), (x, y), look, s.alpha(), image_size);
            for l in &s.layers {
                let (sprite_alpha, layer_alpha) = (s.alpha(), l.alpha());
                if let Some(prev) = &l.previous_src {
                    // Under a layer that fades in; when the layer fades out to nothing, it fades with it.
                    let alpha = if l.src.is_empty() { sprite_alpha * (1.0 - layer_alpha) } else { sprite_alpha };
                    push_sprite(&mut stage, self.picture(prev, l.previous_anim_time), (x, y), look, alpha, image_size);
                }
                push_sprite(&mut stage, self.picture(&l.src, l.anim_time), (x, y), look, sprite_alpha * layer_alpha, image_size);
            }
        }
        // Particles are screen space: `layer=back` ones go between the background and the sprites, the
        // rest over the sprites; neither follows the camera or the shake.
        let sprites = stage.split_off(bg_end);
        let view = self.stage_transform();
        let place = |out: &mut Vec<Draw>, draw: Draw| match draw {
            Draw::Image { src, dest, uv, alpha } => {
                if let Some((dest, uv)) = clip(view(dest), uv, screen) {
                    out.push(Draw::Image { src, dest, uv, alpha });
                }
            }
            // A solid colour fills the screen whatever the camera and shake do.
            rect @ Draw::Rect { .. } => out.push(rect),
            Draw::Text { .. } => {}
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
                    size: self.theme.text_size,
                    color: self.theme.text_color,
                    align: Align::Center,
                });
            }
        }

        let theme = &self.theme;
        for spot in &self.hotspots {
            if let Some(src) = &spot.image {
                push_image(&mut out, src, hotspot_rect(spot), Fit::Stretch, 1.0, image_size);
            }
        }

        // The tint, the cover and the flash are over the whole picture (stage, particles, inline images,
        // video and hotspot art), in that order, but under the textbox and the UI.
        if let Some(([r, g, b], alpha)) = self.tint() {
            out.push(Draw::Rect { rect: screen, color: [r, g, b, alpha] });
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
            self.hud.draw(&self.theme, image_size, &mut out);
            // A passing message in the top right corner, over the HUD.
            if let Some(n) = &self.notice {
                let alpha = n.alpha();
                let (size, pad) = ((theme.text_size * 0.75).round(), 16.0);
                let (color, back) = (faded(theme.text_color, alpha), Some(faded(theme.panel_color, alpha)));
                crate::hud::draw_text(&mut out, &n.text, (VIRTUAL.w - 40.0, 40.0), HudAnchor::Right, size, color, back, pad);
            }
        }

        let box_alpha = self.box_alpha();
        if let Some(d) = self.dialogue.as_ref().filter(|_| ui && box_alpha > 0.0) {
            // The theme's textbox picture if it is loaded, else a plain panel.
            let picture = theme.textbox_image.as_deref().filter(|src| image_size(src).is_some());
            match picture {
                Some(src) => push_image(&mut out, src, textbox, Fit::Stretch, box_alpha, image_size),
                None => out.push(Draw::Rect { rect: textbox, color: faded(theme.panel_color, box_alpha) }),
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
                size: theme.text_size,
                color: faded(theme.text_color, box_alpha),
                align: theme.text_align,
            });
        }

        // A hovered region's tint and outline are over the textbox, so one that
        // reaches under it can be seen (hit-testing gives it the click, too).
        for (i, spot) in self.hotspots.iter().enumerate() {
            let rect = hotspot_rect(spot);
            if ui && hover == Some(i) && !spot.hidden {
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

        let choices = if ui { self.choices.as_slice() } else { &[] };
        for (i, (rect, label)) in choice_rects(theme, choices.len()).into_iter().zip(choices).enumerate() {
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
        if let Some(spot) = hover.filter(|_| ui).and_then(|i| self.hotspots.get(i)).filter(|s| !s.hidden) {
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
    let fits = |s: f32| (s * 1.35).round() <= height + 1.0 && estimate_lines(name, s, width) <= 1;
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

/// About how many textbox lines `text` (tags removed) takes at `size` px, and how
/// many fit. A line that doesn't fit is drawn smaller by the player.
pub fn dialogue_lines(theme: &Theme, text: &str, with_speaker: bool, size: f32) -> (usize, usize) {
    dialogue_lines_tagged(theme, text, with_speaker, size)
}

/// [`dialogue_lines`] for a line as written (with its `{size}` tags): a bigger run
/// is counted as wide as it will really be drawn. Plain text works too.
pub fn dialogue_lines_tagged(theme: &Theme, tagged: &str, with_speaker: bool, size: f32) -> (usize, usize) {
    let body = dialogue_body(theme, with_speaker);
    let line_h = (size * 1.35).round().max(1.0); // what the text renderer uses
    let Rich { text, runs, .. } = rich(tagged);
    (crate::estimate_lines_styled(&text, &runs, size, body.w), (body.h / line_h).floor() as usize)
}

/// `color` with its opacity scaled by `alpha`.
pub fn faded(color: [f32; 4], alpha: f32) -> [f32; 4] {
    [color[0], color[1], color[2], color[3] * alpha]
}

/// A hotspot's region in virtual coordinates.
pub fn hotspot_rect(spot: &Spot) -> Rect {
    let [x, y, w, h] = spot.rect;
    Rect::new(x * VIRTUAL.w, y * VIRTUAL.h, w * VIRTUAL.w, h * VIRTUAL.h)
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
        out.push(Draw::Image { src: src.to_string(), dest: p.dest, uv: p.uv, alpha });
    }
}

/// A sprite centred on `x` (0..1 across the screen), standing on the bottom edge.
fn push_sprite(
    out: &mut Vec<Draw>,
    src: &str,
    (x, y): (f32, f32),
    look: Look,
    alpha: f32,
    image_size: &dyn Fn(&str) -> Option<Size>,
) {
    if src.is_empty() {
        return; // nothing: a hidden layer, a layered sprite's own picture, an animation this view doesn't know
    }
    let Some(size) = image_size(src) else { return };
    if size.w <= 0.0 || size.h <= 0.0 {
        return;
    }
    let h = SPRITE_HEIGHT * look.scale * look.yscale;
    let w = size.w / size.h * SPRITE_HEIGHT * look.scale * look.xscale;
    // A mirrored sprite reads its texture right to left: the clip below carries the
    // negative width along, so cropping at the screen edge stays right.
    let uv = if look.flip { Rect::new(1.0, 0.0, -1.0, 1.0) } else { Rect::new(0.0, 0.0, 1.0, 1.0) };
    out.push(Draw::Image { src: src.to_string(), dest: Rect::new(x * VIRTUAL.w - w / 2.0, y * VIRTUAL.h - h, w, h), uv, alpha });
}

#[cfg(test)]
mod anim_tests;
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
        Cmd::SetBackground { src: src.into(), fade, wipe: None }
    }

    fn show(id: &str, src: &str, x: f32, fade: f32) -> Cmd {
        Cmd::ShowSprite { id: id.into(), src: src.into(), layers: vec![], x, y: 1.0, z: 0, scale: 1.0, flip: false, xscale: 1.0, yscale: 1.0, fade }
    }

    fn slide(id: &str, x: f32, scale: f32, time: f32, ease: Ease) -> Cmd {
        Cmd::MoveSprite { id: id.into(), x, y: 1.0, scale, flip: false, xscale: 1.0, yscale: 1.0, time, ease }
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
        v.apply(&[Cmd::ShowSprite { id: "m".into(), src: "m.png".into(), layers: vec![], x: 0.25, y: 1.0, z: 0, scale: 0.5, flip: false, xscale: 1.0, yscale: 1.0, fade: 0.0 }]);
        // a 1:2 picture at half scale: 470 tall, 235 wide
        let list = v.draw_list(&|_| Some(Size::new(500.0, 1000.0)), None);
        let Draw::Image { dest, uv, alpha, .. } = &list[0] else { panic!("expected a sprite") };
        assert!(close(dest.h, SPRITE_HEIGHT * 0.5) && close(dest.w, SPRITE_HEIGHT * 0.25));
        assert!(close(dest.y + dest.h, VIRTUAL.h)); // bottom edge
        assert!(close(dest.x + dest.w / 2.0, 0.25 * VIRTUAL.w)); // centred on x
        assert_eq!((*uv, *alpha), (Rect::new(0.0, 0.0, 1.0, 1.0), 1.0));
    }

    fn sprite(id: &str, x: f32, y: f32, z: i32) -> Cmd {
        Cmd::ShowSprite { id: id.into(), src: format!("{id}.png"), layers: vec![], x, y, z, scale: 1.0, flip: false, xscale: 1.0, yscale: 1.0, fade: 0.0 }
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
        Cmd::MoveSprite { id: id.into(), x, y, scale: 1.0, flip: false, xscale: 1.0, yscale: 1.0, time, ease }
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
        v.apply(&[Cmd::Notify { text: "Saved".into(), time: 2.0 }]);
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
        v.apply(&[Cmd::Notify { text: "Saved".into(), time: 5.0 }]);
        v.apply(&[Cmd::Notify { text: "Other".into(), time: 5.0 }]);
        v.tick(0.3);
        assert!(notice_draw(&v.draw_list(&all_1080p, None)).is_none(), "the first one was replaced");
        v.end_flash();
        assert!(!v.busy());
        v.apply(&[Cmd::Notify { text: "Saved".into(), time: 5.0 }, Cmd::ClearScene]);
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
            Cmd::ShowChoices { options: vec!["Go".into()] },
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
        assert_eq!(dialogue_lines(&theme(), "Hi.\nHo.", true, 38.0).0, 2);
        assert_eq!(dialogue_lines(&theme(), "Hi. Ho.", true, 38.0).0, 1);
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
        let (_, fit) = dialogue_lines(&t, "x", true, 40.0);
        assert_eq!(fit, (dialogue_body(&t, true).h / 54.0).floor() as usize);
        assert!(dialogue_lines(&t, &"word ".repeat(60), false, 38.0).0 > dialogue_lines(&theme(), &"word ".repeat(60), false, 38.0).0 - 1);
        // choices stack above the textbox top
        let rects = choice_rects(&t, 3);
        assert!(rects.iter().all(|r| r.y + r.h <= 600.0 - CHOICE_MARGIN + 0.01));
        let mut v = View::new().with_theme(t.clone()).with_speed(0.0);
        v.apply(&[say(Some("Mika"), "Hello"), Cmd::ShowChoices { options: vec!["A".into(), "B".into()] }]);
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
        v.apply(&[bg("bg.png", 0.0), say(Some("Mika"), "Hello"), Cmd::ShowChoices { options: vec!["A".into()] }]);
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
        let spot = quillstone_runtime::Spot { label: "Door".into(), rect: [0.1, 0.1, 0.2, 0.2], image: None, hidden: false };
        let mut v = View::new().with_speed(0.0);
        v.apply(&[say(None, "Where to?"), Cmd::ShowHotspots { spots: vec![spot.clone()] }]);
        assert!(v.draw_list(&no_images, None).iter().any(|d| matches!(d, Draw::Text { text, .. } if text == "Where to?")));
        v.apply(&[Cmd::Textbox { hidden: true, fade: 0.0 }, Cmd::ShowHotspots { spots: vec![spot] }]);
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
            Cmd::ShowHotspots { spots: vec![Spot { label: "Map".into(), rect: [0.0, 0.0, 0.2, 0.2], image: Some("art.png".into()), hidden: false }] },
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
        v.apply(&[Cmd::ShowChoices { options: vec!["Yes".into()] }]);
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
        Cmd::ShowSprite { id: id.into(), src: format!("{id}.png"), layers: vec![], x, y: 1.0, z: 0, scale: 1.0, flip, xscale, yscale, fade: 0.0 }
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
    fn xscale_and_yscale_stretch_one_axis_from_the_feet_and_the_centre() {
        let mut v = View::new();
        v.apply(&[look_show("m", false, 2.0, 0.5, 0.25)]);
        let list = v.draw_list(&|_| Some(Size::new(500.0, 1000.0)), None);
        let (_, dest, _) = images(&list)[0];
        // a 1:2 picture at scale 1 is 940 tall and 470 wide: twice as wide, half as tall
        assert!(close(dest.w, 940.0) && close(dest.h, 470.0), "{dest:?}");
        assert!(close(dest.y + dest.h, VIRTUAL.h), "still standing on the bottom edge");
        assert!(close(dest.x + dest.w / 2.0, 0.25 * VIRTUAL.w), "still centred on x");
        // multiplied with scale
        v.apply(&[Cmd::ShowSprite { id: "m".into(), src: "m.png".into(), layers: vec![], x: 0.25, y: 1.0, z: 0, scale: 0.5, flip: false, xscale: 2.0, yscale: 0.5, fade: 0.0 }]);
        let list = v.draw_list(&|_| Some(Size::new(500.0, 1000.0)), None);
        assert!(close(images(&list)[0].1.w, 470.0) && close(images(&list)[0].1.h, 235.0));
    }

    #[test]
    fn a_move_animates_the_scales_and_switches_flip_at_its_start() {
        let mut v = View::new();
        v.apply(&[look_show("m", false, 1.0, 1.0, 0.5)]);
        v.apply(&[Cmd::MoveSprite { id: "m".into(), x: 0.5, y: 1.0, scale: 1.0, flip: true, xscale: 2.0, yscale: 0.5, time: 1.0, ease: Ease::Linear }]);
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
            // centred at x=0: the left half (235 px) hangs off the screen
            let mut v = View::new();
            v.apply(&[look_show("m", flip, 1.0, 1.0, 0.0)]);
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
        v.apply(&[Cmd::ShowSprite { id: "m".into(), src: "m2.png".into(), layers: vec![], x: 0.5, y: 1.0, z: 0, scale: 1.0, flip: true, xscale: 1.0, yscale: 1.0, fade: 1.0 }]);
        v.tick(0.5);
        let list = v.draw_list(&all_1080p, None);
        let flipped: Vec<_> = images(&list).into_iter().map(|(_, _, uv)| uv.w < 0.0).collect();
        assert_eq!(flipped, [true, true]);
    }
}
