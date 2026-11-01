//! Ambient particles (`::particles`): snow, rain or a picture falling over the stage.
//!
//! Nothing is stored per particle and nothing is spawned. Particle `i` is a fixed
//! function of its index (a hash gives its size, speed, drift and start place) and of
//! three clocks that advance with the animation time, so the field is full from the first
//! frame, wraps around the screen, and looks the same every time. Everything is drawn
//! with the ordinary [`Draw::Rect`] and [`Draw::Image`] primitives, in screen space:
//! the camera and shake don't move it. The runtime only keeps the settings.

use quillstone_script::{ParticleKind, Particles, MAX_PARTICLES};

use crate::fade::Fade;
use crate::layout::{clip, Rect, Size, FULL_UV, VIRTUAL};
use crate::view::Draw;

/// A slanted raindrop is drawn as at most this many rectangles.
const MAX_STREAK_PIECES: usize = 12;

const SCREEN: Rect = Rect::new(0.0, 0.0, VIRTUAL.w, VIRTUAL.h);

/// A deterministic number in `0..1` for particle `i` and purpose `k`.
fn unit(i: u32, k: u32) -> f32 {
    let mut h = i.wrapping_mul(0x9E37_79B1) ^ k.wrapping_mul(0x85EB_CA6B).wrapping_add(0x68E3_1DA4);
    h ^= h >> 15;
    h = h.wrapping_mul(0x2C1B_3C6D);
    h ^= h >> 12;
    h = h.wrapping_mul(0x297A_2D39);
    h ^= h >> 15;
    (h >> 8) as f32 / 16_777_216.0
}

/// `v` wrapped into `0..span`.
fn wrap(v: f64, span: f32) -> f32 {
    v.rem_euclid(span as f64) as f32
}

/// One set of settings on screen with its own clocks, so two that overlap in a
/// crossfade don't disturb each other and a change of `speed` or `wind` is smooth.
#[derive(Debug, Clone)]
struct Layer {
    set: Particles,
    /// How visible it is (`alpha`, 0..1): fading in, or out when it is the older set of a crossfade.
    fade: Fade,
    /// Seconds of falling, of swaying, and of being pushed by the wind (the last is
    /// signed, and counts `speed * wind`).
    fall: f64,
    sway: f64,
    push: f64,
}

/// The particles of a [`View`](crate::view::View).
#[derive(Debug, Clone, Default)]
pub struct ParticleLayer {
    now: Option<Layer>,
    /// What `now` replaced, fading out.
    old: Option<Layer>,
}

impl ParticleLayer {
    /// `::particles`: show these (`Some`) or stop them (`None`), easing over `fade` seconds.
    pub fn apply(&mut self, set: &Option<Particles>, fade: f32) {
        let rate = Fade::rate_over(fade);
        let out = |l: Layer| Layer { fade: Fade { target: 0.0, rate, ..l.fade }, ..l };
        let Some(set) = set else {
            self.old = match self.now.take() {
                Some(l) if fade > 0.0 => Some(out(l)),
                _ => None,
            };
            return;
        };
        // The same kind of thing only changes how it falls; anything else crossfades.
        if let Some(l) = self.now.as_mut().filter(|l| l.set.kind == set.kind) {
            l.set = set.clone();
            return;
        }
        self.old = self.now.take().filter(|_| fade > 0.0).map(out);
        let fade = Fade { alpha: if fade > 0.0 { 0.0 } else { 1.0 }, target: 1.0, rate };
        self.now = Some(Layer { set: set.clone(), fade, fall: 0.0, sway: 0.0, push: 0.0 });
    }

    pub fn clear(&mut self) {
        *self = ParticleLayer::default();
    }

    /// Whether anything is falling (so the screen changes by itself).
    pub fn active(&self) -> bool {
        self.now.is_some() || self.old.is_some()
    }

    /// Advance the fades; the clocks only when `animate` (not while skipping).
    pub fn tick(&mut self, dt: f32, animate: bool) {
        for l in [&mut self.now, &mut self.old].into_iter().flatten() {
            l.fade.tick(dt);
            if animate {
                let dt = dt as f64 * l.set.speed as f64;
                l.fall += dt;
                l.sway += dt;
                l.push += dt * l.set.wind as f64;
            }
        }
        if self.old.as_ref().is_some_and(|l| l.fade.alpha <= 0.0) {
            self.old = None;
        }
    }

    /// Add what is drawn behind the sprites (`back`) or over them. The older set of a
    /// crossfade goes first. `image_size` is as for the rest of the view.
    pub fn draw(&self, back: bool, image_size: &dyn Fn(&str) -> Option<Size>, out: &mut Vec<Draw>) {
        for l in [&self.old, &self.now].into_iter().flatten().filter(|l| l.set.back == back && l.fade.alpha > 0.0) {
            let n = l.set.amount.min(MAX_PARTICLES);
            match &l.set.kind {
                ParticleKind::Snow => (0..n).for_each(|i| snowflake(l, i, out)),
                ParticleKind::Rain => (0..n).for_each(|i| raindrop(l, i, out)),
                ParticleKind::Image(src) => {
                    if let Some(size) = image_size(src) {
                        (0..n).for_each(|i| picture(l, i, src, size, out));
                    }
                }
            }
        }
    }
}

/// A rectangle cut to the screen; nothing when it is all outside.
fn push_rect(out: &mut Vec<Draw>, rect: Rect, color: [f32; 4]) {
    if let Some((rect, _)) = clip(rect, FULL_UV, SCREEN) {
        out.push(Draw::Rect { rect, color });
    }
}

/// Where a falling particle is: `(x, y)` of its top left corner. `vy` is its fall speed in
/// px/s, `wind_px` how far one unit of wind pushes it sideways, `margin` how far off screen
/// it goes before wrapping (its own size).
fn place(l: &Layer, i: u32, vy: f32, wind_px: f32, sway: f32, margin: f32) -> (f32, f32) {
    let (span_x, span_y) = (VIRTUAL.w + 2.0 * margin, VIRTUAL.h + 2.0 * margin);
    let freq = 0.5 + 0.9 * unit(i, 5) as f64;
    let swing = sway * (l.sway * freq + std::f64::consts::TAU * unit(i, 6) as f64).sin() as f32;
    let y = wrap(unit(i, 2) as f64 * span_y as f64 + l.fall * vy as f64, span_y) - margin;
    let x = wrap(unit(i, 3) as f64 * span_x as f64 + swing as f64 + l.push * wind_px as f64, span_x) - margin;
    (x, y)
}

/// Small soft white squares; bigger ones are nearer: they fall faster and are brighter.
fn snowflake(l: &Layer, i: u32, out: &mut Vec<Draw>) {
    let near = unit(i, 0);
    let size = 2.2 + 3.3 * near * near;
    let vy = 45.0 + 75.0 * near + 20.0 * unit(i, 1);
    let (x, y) = place(l, i, vy, 70.0 + 60.0 * near, 15.0 + 35.0 * unit(i, 4), 10.0);
    let alpha = (0.5 + 0.45 * near) * l.fade.alpha;
    // a faint halo around the core makes the flake soft
    let halo = size * 2.6;
    let pad = (halo - size) / 2.0;
    push_rect(out, Rect::new(x - pad, y - pad, halo, halo), [0.95, 0.97, 1.0, alpha * 0.28]);
    push_rect(out, Rect::new(x, y, size, size), [0.97, 0.98, 1.0, alpha]);
}

/// Thin streaks, slanted by the wind. A rectangle can't lean, so a slanted streak is a few
/// short ones stepped along its line.
fn raindrop(l: &Layer, i: u32, out: &mut Vec<Draw>) {
    const SLANT: f32 = 0.3; // sideways px per px fallen at wind 1
    let near = unit(i, 0);
    let (thick, len) = (1.5 + near, 30.0 + 40.0 * near);
    let vy = 1000.0 + 600.0 * near + 100.0 * unit(i, 1);
    let (x, y) = place(l, i, vy, vy * SLANT, 0.0, len);
    let lean = (l.set.wind * SLANT).clamp(-1.0, 1.0);
    let steps = ((lean.abs() * len / 2.5).ceil() as usize).clamp(1, MAX_STREAK_PIECES);
    let (dx, dy) = (lean * len / steps as f32, len / steps as f32);
    let color = [0.78, 0.85, 0.95, (0.18 + 0.25 * near) * l.fade.alpha];
    for k in 0..steps {
        // from the tail (up-wind of the head at x, y + len) down to the head
        let (sx, sy) = (x - lean * len + dx * k as f32, y + dy * k as f32);
        push_rect(out, Rect::new(sx.min(sx + dx), sy, thick + dx.abs(), dy + 0.5), color);
    }
}

/// A picture, at its own pixel size scaled a little per particle, falling and drifting like snow.
fn picture(l: &Layer, i: u32, src: &str, size: Size, out: &mut Vec<Draw>) {
    let near = unit(i, 0);
    let scale = 0.7 + 0.5 * near;
    let (w, h) = (size.w * scale, size.h * scale);
    let vy = 55.0 + 70.0 * unit(i, 1);
    let (x, y) = place(l, i, vy, 60.0 + 60.0 * near, 25.0 + 55.0 * unit(i, 4), w.max(h));
    if let Some((dest, uv)) = clip(Rect::new(x, y, w, h), FULL_UV, SCREEN) {
        out.push(Draw::Image { src: src.to_string(), dest, uv, alpha: (0.75 + 0.25 * near) * l.fade.alpha });
    }
}

#[cfg(test)]
mod tests {
    use quillstone_runtime::{Camera, Cmd};
    use quillstone_script::Ease;

    use super::*;
    use crate::view::View;

    fn set(kind: ParticleKind, amount: u32) -> Particles {
        Particles { amount, ..Particles::new(kind) }
    }

    fn cmd(p: Particles, fade: f32) -> Cmd {
        Cmd::Particles { set: Some(p), fade }
    }

    fn sizes(name: &str) -> Option<Size> {
        (name == "petal.png" || name == "bg.png").then(|| if name == "bg.png" { Size::new(1920.0, 1080.0) } else { Size::new(32.0, 24.0) })
    }

    /// Everything drawn after the one background image, until the first image or the end.
    fn drawn(v: &View) -> Vec<Draw> {
        v.draw_list(&sizes, None).into_iter().filter(|d| !matches!(d, Draw::Image { src, .. } if src == "bg.png")).collect()
    }

    fn inside(r: Rect) -> bool {
        r.x >= 0.0 && r.y >= 0.0 && r.x + r.w <= VIRTUAL.w + 0.01 && r.y + r.h <= VIRTUAL.h + 0.01 && r.w > 0.0 && r.h > 0.0
    }

    #[test]
    fn the_hash_is_deterministic_and_spreads_out() {
        assert_eq!(unit(7, 3), unit(7, 3));
        let xs: Vec<f32> = (0..200).map(|i| unit(i, 3)).collect();
        assert!(xs.iter().all(|x| (0.0..1.0).contains(x)));
        let mean = xs.iter().sum::<f32>() / 200.0;
        assert!((0.4..0.6).contains(&mean), "{mean}");
        // no two of the first 200 share a column band of 4 px (a grid would)
        let mut cols: Vec<i32> = xs.iter().map(|x| (x * 480.0) as i32).collect();
        cols.sort();
        cols.dedup();
        assert!(cols.len() > 140, "{}", cols.len());
    }

    #[test]
    fn nothing_is_drawn_until_particles_are_asked_for_and_after_they_are_cleared() {
        let mut v = View::new();
        assert!(drawn(&v).is_empty() && !v.busy());
        v.apply(&[cmd(set(ParticleKind::Snow, 60), 0.0)]);
        assert!(!drawn(&v).is_empty() && v.busy(), "particles keep the screen moving");
        v.apply(&[Cmd::ClearScene]);
        assert!(drawn(&v).is_empty() && !v.busy());
    }

    #[test]
    fn the_count_is_what_was_asked_for_and_never_over_the_cap() {
        let mut v = View::new();
        v.apply(&[cmd(set(ParticleKind::Snow, 3), 0.0)]);
        let n = drawn(&v).len();
        assert!((3..=6).contains(&n), "{n}: a square and a halo each (some cut off)");
        // `amount` is clamped even when the runtime hands over more (a hostile save is clamped there too)
        let count = |kind: ParticleKind| {
            let mut l = ParticleLayer::default();
            l.apply(&Some(Particles { amount: 100_000, wind: 2.0, ..Particles::new(kind) }), 0.0);
            let mut out = Vec::new();
            l.draw(false, &sizes, &mut out);
            out.len()
        };
        let cap = MAX_PARTICLES as usize;
        assert!(count(ParticleKind::Image("petal.png".into())) <= cap);
        assert!(count(ParticleKind::Snow) <= 2 * cap, "a square and its halo");
        assert!(count(ParticleKind::Rain) <= MAX_STREAK_PIECES * cap);
        assert!(count(ParticleKind::Snow) > cap / 2);
    }

    #[test]
    fn every_particle_stays_on_screen_whatever_the_time() {
        for kind in [ParticleKind::Snow, ParticleKind::Rain, ParticleKind::Image("petal.png".into())] {
            for wind in [-2.0, 0.0, 2.0] {
                let mut v = View::new();
                v.apply(&[cmd(Particles { amount: 200, speed: 5.0, wind, ..Particles::new(kind.clone()) }, 0.0)]);
                for _ in 0..40 {
                    v.tick(0.37);
                    for d in drawn(&v) {
                        match d {
                            Draw::Rect { rect, color } => {
                                assert!(inside(rect), "{kind:?} {rect:?}");
                                assert!(color.iter().all(|c| (0.0..=1.0).contains(c)));
                            }
                            Draw::Image { dest, uv, .. } => assert!(inside(dest) && uv.w > 0.0 && uv.x >= 0.0 && uv.x + uv.w <= 1.001),
                            Draw::Text { .. } => panic!("no text"),
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn the_same_time_gives_the_same_picture_and_time_moves_it() {
        let run = |steps: usize| {
            let mut v = View::new();
            v.apply(&[cmd(set(ParticleKind::Snow, 100), 0.0)]);
            for _ in 0..steps {
                v.tick(0.1);
            }
            drawn(&v)
        };
        assert_eq!(run(10), run(10));
        let (a, b) = (run(10), run(20));
        assert_ne!(a, b);
        // and it falls: the first flake is lower a second later
        let y = |l: &[Draw]| match &l[0] {
            Draw::Rect { rect, .. } => rect.y,
            _ => unreachable!(),
        };
        let (y0, y1) = (y(&run(0)), y(&run(10)));
        assert!(y1 > y0 || y1 < y0 - 500.0, "{y0} -> {y1} (down, or wrapped round the top)");
        // starting again from nothing gives the first picture again
        let mut v = View::new();
        v.apply(&[cmd(set(ParticleKind::Snow, 100), 0.0)]);
        let first = drawn(&v);
        v.tick(5.0);
        v.apply(&[Cmd::ClearScene, cmd(set(ParticleKind::Snow, 100), 0.0)]);
        assert_eq!(drawn(&v), first);
    }

    #[test]
    fn settling_moves_nothing_but_the_fades() {
        let mut v = View::new();
        v.apply(&[cmd(set(ParticleKind::Rain, 50), 1.0)]);
        v.settle(5.0);
        let after_settle = drawn(&v);
        let mut w = View::new();
        w.apply(&[cmd(set(ParticleKind::Rain, 50), 0.0)]);
        assert_eq!(after_settle, drawn(&w), "faded in, but where they started");
    }

    #[test]
    fn wind_leans_rain_and_pushes_everything_sideways() {
        let rain = |wind: f32| {
            let mut l = ParticleLayer::default();
            l.apply(&Some(Particles { amount: 1, wind, ..Particles::new(ParticleKind::Rain) }), 0.0);
            let mut out = Vec::new();
            l.draw(false, &sizes, &mut out);
            out
        };
        assert_eq!(rain(0.0).len(), 1, "calm rain falls straight: one piece");
        let leaning = rain(2.0);
        assert!(leaning.len() > 2, "{}: a slanted streak is stepped", leaning.len());
        let xs: Vec<f32> = leaning.iter().map(|d| if let Draw::Rect { rect, .. } = d { rect.x } else { 0.0 }).collect();
        assert!(xs.windows(2).all(|w| w[1] > w[0]), "head is to the right of the tail when the wind blows right: {xs:?}");
        let mut l = ParticleLayer::default();
        l.apply(&Some(set(ParticleKind::Snow, 1)), 0.0);
        let x = |l: &ParticleLayer| {
            let mut out = Vec::new();
            l.draw(false, &sizes, &mut out);
            match out.last() {
                Some(Draw::Rect { rect, .. }) => rect.x,
                _ => f32::NAN,
            }
        };
        let calm = x(&l);
        l.apply(&Some(Particles { wind: 2.0, ..set(ParticleKind::Snow, 1) }), 0.0);
        l.tick(0.5, true);
        assert_ne!(x(&l), calm, "the wind moved the flake");
    }

    #[test]
    fn fades_ease_in_and_out_and_a_change_of_kind_crossfades() {
        let alpha = |v: &View| {
            drawn(v).iter().filter_map(|d| if let Draw::Rect { color, .. } = d { Some(color[3]) } else { None }).fold(0.0f32, f32::max)
        };
        let mut v = View::new();
        v.apply(&[cmd(set(ParticleKind::Snow, 100), 2.0)]);
        assert!(drawn(&v).is_empty(), "starts invisible");
        v.tick(1.0);
        let half = alpha(&v);
        v.tick(1.0);
        let full = alpha(&v);
        assert!(half > 0.0 && half < full * 0.75, "{half} {full}");
        v.apply(&[Cmd::Particles { set: Some(set(ParticleKind::Rain, 100)), fade: 2.0 }]);
        v.tick(1.0);
        let list = drawn(&v);
        let colours: std::collections::BTreeSet<bool> =
            list.iter().filter_map(|d| if let Draw::Rect { color, .. } = d { Some(color[0] > 0.9) } else { None }).collect();
        assert_eq!(colours.len(), 2, "snow and rain at once while they cross");
        v.tick(1.5);
        assert!(drawn(&v).iter().all(|d| matches!(d, Draw::Rect { color, .. } if color[0] < 0.9)), "only rain is left");
        v.apply(&[Cmd::Particles { set: None, fade: 1.0 }]);
        v.tick(0.5);
        assert!(!drawn(&v).is_empty());
        v.tick(0.6);
        assert!(drawn(&v).is_empty() && !v.busy(), "faded out and gone");
        v.apply(&[cmd(set(ParticleKind::Snow, 10), 3.0)]);
        v.apply(&[Cmd::Particles { set: None, fade: 0.0 }]);
        assert!(drawn(&v).is_empty() && !v.busy(), "a stop with no fade is at once");
    }

    #[test]
    fn same_kind_changes_keep_the_particles_where_they_are() {
        let mut v = View::new();
        v.apply(&[cmd(set(ParticleKind::Snow, 100), 0.0)]);
        v.tick(3.0);
        let before = drawn(&v).len();
        v.apply(&[cmd(Particles { amount: 100, ..set(ParticleKind::Snow, 100) }, 5.0)]);
        assert_eq!(drawn(&v).len(), before, "no restart, no fade-out");
    }

    #[test]
    fn the_layer_decides_whether_they_are_behind_the_sprites() {
        let mut v = View::new();
        let sprite = Cmd::ShowSprite {
            id: "a".into(), src: "petal.png".into(), layers: vec![], x: 0.5, y: 1.0, z: 0, scale: 1.0, flip: false, xscale: 1.0, yscale: 1.0, fade: 0.0,
        };
        let order = |v: &View| -> Vec<&'static str> {
            let mut o: Vec<&'static str> = v
                .draw_list(&sizes, None)
                .iter()
                .map(|d| match d {
                    Draw::Image { src, .. } if src == "bg.png" => "bg",
                    Draw::Image { .. } => "sprite",
                    _ => "particle",
                })
                .collect();
            o.dedup();
            o
        };
        let bg = Cmd::SetBackground { src: "bg.png".into(), fade: 0.0, wipe: None };
        v.apply(&[bg.clone(), sprite.clone(), cmd(set(ParticleKind::Snow, 20), 0.0)]);
        assert_eq!(order(&v), ["bg", "sprite", "particle"]);
        v.apply(&[Cmd::ClearScene, bg, sprite, cmd(Particles { back: true, ..set(ParticleKind::Snow, 20) }, 0.0)]);
        assert_eq!(order(&v), ["bg", "particle", "sprite"]);
    }

    #[test]
    fn they_are_screen_space_and_sit_under_the_text_and_over_a_cover() {
        let mut v = View::new();
        v.apply(&[cmd(set(ParticleKind::Snow, 30), 0.0)]);
        v.tick(2.0);
        let calm = drawn(&v);
        v.apply(&[
            Cmd::Camera { camera: Camera { zoom: 3.0, x: 0.1, y: 0.9 }, time: 0.0, ease: Ease::Linear },
            Cmd::Shake { time: 5.0, strength: 100.0, axis: quillstone_script::Axis::Both },
        ]);
        assert_eq!(drawn(&v), calm, "the camera and the shake don't move them");
        let say = Cmd::Say { speaker: None, color: None, text: "Hi".into() };
        v.apply(&[say, Cmd::FadeOut { time: 0.0, color: [0.0; 3] }]);
        let list = v.draw_list(&sizes, None);
        let last_particle = list.iter().rposition(|d| matches!(d, Draw::Rect { color, .. } if color[3] < 0.99 && color[0] > 0.9)).unwrap();
        let cover = list.iter().position(|d| matches!(d, Draw::Rect { rect, .. } if *rect == Rect::new(0.0, 0.0, VIRTUAL.w, VIRTUAL.h))).unwrap();
        let text = list.iter().position(|d| matches!(d, Draw::Text { .. })).unwrap();
        assert!(last_particle < cover && cover < text, "{last_particle} {cover} {text}");
    }

    #[test]
    fn a_picture_particle_waits_for_its_image_and_keeps_its_size() {
        let mut v = View::new();
        v.apply(&[cmd(set(ParticleKind::Image("missing.png".into()), 10), 0.0)]);
        assert!(drawn(&v).is_empty(), "an unloaded picture is skipped, not guessed");
        v.apply(&[cmd(set(ParticleKind::Image("petal.png".into()), 10), 0.0)]);
        let list = drawn(&v);
        assert_eq!(list.len(), 10);
        for d in list {
            let Draw::Image { src, dest, .. } = d else { panic!() };
            assert_eq!(src, "petal.png");
            // 32x24 scaled 0.7..1.2 (less where it is cut by the screen edge)
            assert!(dest.w <= 32.0 * 1.2 + 0.01 && dest.h <= 24.0 * 1.2 + 0.01);
        }
    }
}
