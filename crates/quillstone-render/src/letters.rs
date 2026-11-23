//! The moving letters of `{shake}`, `{wave}` and `{glitch}` text (see `quillstone_script::markup`).
//!
//! A line with such a run is rasterized once without those letters. Each of them is drawn once as a small
//! picture of its own (by the text backend, which lays them out) and then placed every frame at its place in
//! the line plus the motion of its effect, which is a function of the time alone: nothing here uses the story's
//! random numbers, so saves, rollback and tests don't see it.

use std::ops::Range;
use std::rc::Rc;

pub use quillstone_script::markup::Anim;

use crate::batch::{Frame, Quad, Tex, TextJob};
use crate::layout::{to_ndc, Rect, Size};

/// What the symbols are picked from: wide ones, then narrow ones for the narrow letters.
pub const GLITCH_CHARS: &str = "#%&@$?!*+=~/<>|':;.,()[]";
/// How many symbols a glitching run draws (all of them, once).
pub const GLITCH_SYMBOLS: usize = GLITCH_CHARS.len();

/// A letter's picture: `dx`, `dy` is where its top left is, from the letter's pen position.
#[derive(Debug, Clone, PartialEq)]
pub struct Pic {
    pub dx: i32,
    pub dy: i32,
    pub w: u32,
    pub h: u32,
}

/// One letter that moves.
#[derive(Debug, Clone, PartialEq)]
pub struct Letter {
    /// Which character of the text it is (it shows with the typing).
    pub index: usize,
    pub anim: Anim,
    /// The tag's value: shake strength, wave height, or the share of glitching letters.
    pub amount: f32,
    /// How wide the letter is (its advance), which a symbol is centred in.
    pub width: i32,
    /// Its pen position in the text's rectangle, in window pixels.
    pub x: i32,
    pub y: i32,
    /// Its own picture, in [`Letters::pics`].
    pub pic: usize,
    /// The pictures of the symbols a glitch swaps it for (empty for the others).
    pub symbols: Range<usize>,
    /// Of those, sorted by width, how many are no wider than the letter (at least one: the narrowest).
    pub fit: usize,
}

/// The letters of a line and their pictures' sizes.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Letters {
    pub letters: Vec<Letter>,
    pub pics: Vec<Pic>,
}

/// Moves per second of a shake, and of a glitch's chances.
const SHAKE_RATE: f32 = 25.0;
const GLITCH_RATE: f32 = 10.0;
/// How far a shake of strength 1 moves a letter, in text heights, along each axis.
const SHAKE_REACH: f32 = 0.02;
/// A glitching letter picks its symbol among this many of the widest that fit it.
const GLITCH_PICK: usize = 4;
/// The most a glitching symbol jumps sideways, as a share of the width of the letter it replaces.
const GLITCH_JUMP: f32 = 0.15;
/// Radians per second of a wave, and between neighbouring letters.
const WAVE_SPEED: f32 = 5.0;
const WAVE_LAG: f32 = 0.6;

fn hash(index: usize, step: u64, salt: u64) -> u64 {
    let mut z = (index as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ step.wrapping_mul(0xBF58_476D_1CE4_E5B9) ^ salt.wrapping_mul(0x94D0_49BB_1331_11EB);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

/// -1 to 1 from the top bits of a hash.
fn unit(h: u64) -> f32 {
    (h >> 40) as f32 / (1u64 << 23) as f32 - 1.0
}

/// Where `letter` is drawn `time` seconds in, for text `size` pixels tall: the offset in pixels, and which picture
/// (its own, or one of a glitch's symbols).
pub fn motion(letter: &Letter, time: f32, size: f32) -> ((f32, f32), usize) {
    let i = letter.index;
    match letter.anim {
        Anim::Shake => {
            let step = (time * SHAKE_RATE).floor() as u64;
            ((unit(hash(i, step, 1)) * SHAKE_REACH * size * letter.amount, unit(hash(i, step, 2)) * SHAKE_REACH * size * letter.amount), letter.pic)
        }
        Anim::Wave => ((0.0, (time * WAVE_SPEED - i as f32 * WAVE_LAG).sin() * 0.12 * size * letter.amount), letter.pic),
        Anim::Glitch => {
            let h = hash(i, (time * GLITCH_RATE).floor() as u64, 3);
            if (h >> 40) as f32 / (1u64 << 24) as f32 >= letter.amount || letter.symbols.is_empty() {
                return ((0.0, 0.0), letter.pic);
            }
            // a small share of the letter's own width, so a symbol stays in its slot
            let jump = (0.5 + 0.5 * (h >> 20 & 0xff) as f32 / 255.0) * GLITCH_JUMP * letter.width as f32;
            // among the widest few that fit: wide letters get the bold symbols, narrow ones the thin
            let fit = letter.fit.clamp(1, letter.symbols.len());
            let first = fit.saturating_sub(GLITCH_PICK);
            let symbol = letter.symbols.start + first + (h >> 8) as usize % (fit - first);
            (((if h >> 16 & 1 == 0 { jump } else { -jump }), 0.0), symbol)
        }
    }
}

/// Where a symbol's picture is from the letter's pen position: centred in the letter's width, plus the glitch's
/// sideways `jump`, which never takes the picture out of that width (a symbol that fills it doesn't jump).
pub fn symbol_dx(letter: &Letter, pic: &Pic, jump: f32) -> i32 {
    let room = ((letter.width - pic.w as i32) / 2).max(0) as f32;
    (letter.width - pic.w as i32).div_euclid(2) + jump.clamp(-room, room).round() as i32
}

impl TextJob {
    /// Some of the text moves.
    pub fn animated(&self) -> bool {
        self.runs.iter().any(|r| r.style.anim.is_some())
    }

    /// Identifies the letters' pictures: like [`TextJob::key`], but they don't depend on how much is typed.
    pub fn letters_key(&self) -> String {
        let [r, g, b, a] = self.color.map(|c| (c.clamp(0.0, 1.0) * 255.0).round() as u8);
        let outline = self.outline.map_or(String::new(), |o| format!("|{:.2}:{:?}", o.size, o.color));
        format!("{}|{r},{g},{b},{a}{outline}", self.layout_key())
    }
}

impl Frame {
    /// Some text of the frame has moving letters, so it must be drawn again every frame.
    pub fn animated(&self) -> bool {
        self.texts.iter().any(TextJob::animated)
    }

    /// Put the typed moving letters of each animated text right after the quad of its line, `time` seconds into
    /// the animation. `letters` gives the letters of a job (`None`: no text backend, nothing moves);
    /// they are drawn from `Tex::Letter(job, picture)`.
    pub fn add_letters(&mut self, window: Size, time: f32, letters: &mut dyn FnMut(&TextJob) -> Option<Rc<Letters>>) {
        let mut quads = Vec::with_capacity(self.quads.len());
        for quad in std::mem::take(&mut self.quads) {
            let job = match quad.texture {
                Tex::Text(i) => Some(i),
                _ => None,
            };
            let filter = quad.filter;
            quads.push(quad);
            let Some(i) = job else { continue };
            let job = &self.texts[i];
            let Some(set) = job.animated().then(|| letters(job)).flatten() else { continue };
            for letter in set.letters.iter().filter(|l| l.index < job.visible_chars) {
                let ((dx, dy), pic) = motion(letter, time, job.size_px);
                let Some(p) = set.pics.get(pic) else { continue };
                // a symbol is centred in the width of the letter it replaces
                let px = if pic == letter.pic { p.dx } else { symbol_dx(letter, p, dx) };
                // whole pixels keep the letter as sharp as the line is
                let (x, y) = (job.rect.x + (letter.x + px) as f32 + if pic == letter.pic { dx.round() } else { 0.0 }, job.rect.y + (letter.y + p.dy) as f32 + dy.round());
                quads.push(Quad {
                    texture: Tex::Letter(i, pic),
                    rect: to_ndc(Rect::new(x, y, p.w as f32, p.h as f32), window),
                    uv: [0.0, 0.0, 1.0, 1.0],
                    color: [1.0; 4],
                    blur: 0.0,
                    filter,
                    additive: false,
                    mask: None,
                });
            }
        }
        self.quads = quads;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn letter(index: usize, anim: Anim) -> Letter {
        Letter { index, anim, amount: 1.0, width: 20, x: 100 + 20 * index as i32, y: 50, pic: index, symbols: 10..10 + GLITCH_SYMBOLS, fit: GLITCH_SYMBOLS }
    }

    #[test]
    fn a_shake_jitters_a_few_pixels_and_changes_over_time() {
        let l = letter(3, Anim::Shake);
        let offsets: Vec<_> = (0..40).map(|k| motion(&l, k as f32 * 0.04, 40.0).0).collect();
        assert!(offsets.iter().all(|(x, y)| x.abs() <= 0.8 && y.abs() <= 0.8), "a gentle tremble scaled to the size: {offsets:?}");
        assert!(offsets.windows(2).any(|w| w[0] != w[1]) && offsets.iter().any(|o| o.0.abs() > 0.2));
        // the strength scales the movement
        let strong = Letter { amount: 3.0, ..l.clone() };
        assert!((0..40).all(|k| {
            let (a, b) = (motion(&l, k as f32 * 0.04, 40.0).0, motion(&strong, k as f32 * 0.04, 40.0).0);
            (b.0 - 3.0 * a.0).abs() < 1e-4 && (b.1 - 3.0 * a.1).abs() < 1e-4
        }));
        assert!((0..40).any(|k| motion(&strong, k as f32 * 0.04, 40.0).0 .0.abs() > 1.8), "up to about a sixth of the height");
        assert_eq!(motion(&l, 1.0, 40.0), motion(&l, 1.0, 40.0), "a function of the time alone");
        assert_ne!(motion(&l, 1.0, 40.0), motion(&letter(4, Anim::Shake), 1.0, 40.0), "each letter on its own");
        assert!(motion(&l, 1.0, 80.0).0 .0.abs() >= motion(&l, 1.0, 40.0).0 .0.abs());
    }

    #[test]
    fn a_wave_bobs_up_and_down_travelling_along_the_line() {
        let ys = |i, t| motion(&letter(i, Anim::Wave), t, 40.0).0;
        assert!((0..100).all(|k| ys(0, k as f32 * 0.05).0 == 0.0 && ys(0, k as f32 * 0.05).1.abs() <= 4.8 + 1e-3));
        let ups = (0..100).filter(|k| ys(0, *k as f32 * 0.05).1 > 2.0).count();
        let downs = (0..100).filter(|k| ys(0, *k as f32 * 0.05).1 < -2.0).count();
        assert!(ups > 10 && downs > 10, "both ways: {ups} {downs}");
        // the next letter does what this one did a moment ago
        let lag = WAVE_LAG / WAVE_SPEED;
        assert!((ys(1, 1.0 + lag).1 - ys(0, 1.0).1).abs() < 1e-3);
        assert_eq!(ys(0, 0.0), (0.0, 0.0), "starts level");
        let high = Letter { amount: 2.0, ..letter(0, Anim::Wave) };
        assert!((motion(&high, 0.5, 40.0).0 .1 - 2.0 * ys(0, 0.5).1).abs() < 1e-4, "the height factor scales the bob");
    }

    #[test]
    fn a_glitch_now_and_then_swaps_a_letter_for_a_symbol_and_jumps_it_sideways() {
        let l = Letter { amount: 0.07, ..letter(2, Anim::Glitch) };
        let samples: Vec<_> = (0..400).map(|k| motion(&l, k as f32 / GLITCH_RATE, 40.0)).collect();
        let hit = samples.iter().filter(|s| s.1 != l.pic).count();
        assert!(hit > 5 && hit < 100, "now and then: {hit} of 400");
        for ((dx, dy), pic) in samples {
            if pic == l.pic {
                assert_eq!((dx, dy), (0.0, 0.0));
            } else {
                assert!(l.symbols.contains(&pic) && dx.abs() >= 1.5 && dx.abs() <= 3.0 && dy == 0.0, "{dx} {dy} {pic}");
            }
        }
        let bare = Letter { symbols: 0..0, ..l.clone() };
        assert!((0..400).all(|k| motion(&bare, k as f32 / GLITCH_RATE, 40.0).1 == bare.pic), "no symbols, no swap");
    }

    #[test]
    fn the_share_of_glitching_letters_is_the_value() {
        for share in [0.0f32, 0.1, 0.5, 0.9, 1.0] {
            let letters: Vec<_> = (0..20).map(|i| Letter { amount: share, pic: 100 + i, ..letter(i, Anim::Glitch) }).collect();
            let total = 20 * 500;
            let hit = letters.iter().map(|l| (0..500).filter(|k| motion(l, *k as f32 / GLITCH_RATE, 40.0).1 != l.pic).count()).sum::<usize>();
            let got = hit as f32 / total as f32;
            assert!((got - share).abs() < 0.03, "share {share}: {got}");
        }
        // at a full share the symbols vary from letter to letter and step to step
        let all: std::collections::BTreeSet<_> = (0..20).flat_map(|i| (0..50).map(move |k| motion(&Letter { amount: 1.0, ..letter(i, Anim::Glitch) }, k as f32 / GLITCH_RATE, 40.0).1)).collect();
        assert_eq!(all.len(), GLITCH_PICK);
    }

    #[test]
    fn a_symbol_is_centred_in_the_letter_it_replaces() {
        let set = Rc::new(Letters {
            letters: vec![Letter { amount: 1.0, width: 30, symbols: 1..2, fit: 1, x: 100, y: 50, pic: 0, ..letter(1, Anim::Glitch) }],
            pics: vec![Pic { dx: 3, dy: -30, w: 20, h: 40 }, Pic { dx: -7, dy: -28, w: 10, h: 30 }],
        });
        let r = quillstone_script::markup::rich("a{glitch=1}bcd{/glitch}");
        let d = crate::view::Draw::Text { text: r.text, runs: r.runs, visible_chars: 9, rect: Rect::new(0.0, 0.0, 1920.0, 200.0), size: 40.0, color: [1.0; 4], align: crate::view::Align::Left, outline: None };
        let left = |time: f32| {
            let mut f = crate::batch::build_frame(std::slice::from_ref(&d), Size::new(1920.0, 1080.0));
            f.add_letters(Size::new(1920.0, 1080.0), time, &mut |_| Some(set.clone()));
            f.quads.iter().find_map(|q| match q.texture {
                Tex::Letter(_, p) => Some((p, ((q.rect[0] + 1.0) / 2.0 * 1920.0).round() as i32 - motion(&set.letters[0], time, 40.0).0 .0.round() as i32)),
                _ => None,
            })
            .unwrap()
        };
        let (pic, x) = left(0.0);
        assert_eq!(pic, 1, "a full share is a symbol at once");
        assert_eq!(x, 100 + (30 - 10) / 2, "the middle of the letter's width, not its own pen position");
        // wherever the glitch jumps it, the symbol's box stays inside the letter's advance (a pixel or two over)
        for k in 0..200 {
            let (_, x) = left(k as f32 / GLITCH_RATE);
            let jump = motion(&set.letters[0], k as f32 / GLITCH_RATE, 40.0).0 .0.round() as i32;
            assert!(x + jump >= 100 - 2 && x + jump + 10 <= 100 + 30 + 2, "{}", x + jump);
        }
    }

    #[test]
    fn a_narrow_letter_only_gets_symbols_that_fit_or_the_narrowest() {
        // pictures sorted by width, as the text backend makes them: 4, 6, 8, ... 26 px
        let widths: Vec<u32> = (0..GLITCH_SYMBOLS as u32).map(|k| 4 + 2 * k).collect();
        for width in [3, 4, 9, 14, 40] {
            let fit = widths.iter().filter(|w| **w as i32 <= width).count().max(1);
            let l = Letter { amount: 1.0, width, symbols: 0..widths.len(), fit, ..letter(0, Anim::Glitch) };
            for k in 0..300 {
                let pic = motion(&l, k as f32 / GLITCH_RATE, 40.0).1;
                assert!(widths[pic] as i32 <= width || pic == 0, "width {width}: symbol {pic} is {} wide", widths[pic]);
                assert!(pic < fit && pic + GLITCH_PICK >= fit.max(GLITCH_PICK), "one of the widest {GLITCH_PICK} that fit: {pic} of {fit}");
                if pic == l.pic || widths[pic] as i32 > width {
                    continue;
                }
                // with the jump, the drawn box stays within the advance (a pixel or two over)
                let left = symbol_dx(&l, &Pic { dx: 0, dy: 0, w: widths[pic], h: 1 }, motion(&l, k as f32 / GLITCH_RATE, 40.0).0 .0);
                assert!(left >= 0 && left + widths[pic] as i32 <= width + 1, "width {width}, symbol {pic}: {left}");
            }
        }
    }

    fn animated_frame(visible: usize) -> Frame {
        use crate::view::{Align, Draw};
        let r = quillstone_script::markup::rich("a{shake}bcd{/shake}");
        let d = Draw::Text { text: r.text, runs: r.runs, visible_chars: visible, rect: Rect::new(0.0, 0.0, 1920.0, 200.0), size: 40.0, color: [1.0; 4], align: Align::Left, outline: None };
        crate::batch::build_frame(&[d], Size::new(1920.0, 1080.0))
    }

    #[test]
    fn typed_letters_become_quads_after_their_line_and_they_move() {
        let set = Rc::new(Letters {
            letters: (1..4).map(|i| Letter { symbols: 0..0, ..letter(i, Anim::Shake) }).collect(),
            pics: vec![Pic { dx: -1, dy: -30, w: 24, h: 40 }; 4],
        });
        let at = |visible: usize, time: f32| {
            let mut f = animated_frame(visible);
            assert!(f.animated());
            f.add_letters(Size::new(1920.0, 1080.0), time, &mut |_| Some(set.clone()));
            f
        };
        let f = at(3, 0.0);
        assert_eq!(f.quads.len(), 1 + 2, "the line and the two letters typed");
        assert!(matches!(f.quads[0].texture, Tex::Text(0)) && matches!(f.quads[1].texture, Tex::Letter(0, 1)));
        assert_eq!(at(1, 0.0).quads.len(), 1, "none shown yet");
        assert_eq!(at(4, 0.0).quads.len(), 4);
        assert_ne!(at(4, 0.0).quads[1].rect, at(4, 0.2).quads[1].rect, "the positions differ over time");
        assert_eq!(at(4, 0.2).quads[1].rect, at(4, 0.2).quads[1].rect);
        // a letter is a whole-pixel place: pen + picture offset + the shake
        let [l, t, _, _] = at(4, 0.0).quads[1].rect;
        let px = ((l + 1.0) / 2.0 * 1920.0, (1.0 - t) / 2.0 * 1080.0);
        let (dx, dy) = motion(&set.letters[0], 0.0, 40.0).0;
        assert!((px.0 - (100.0 + 20.0 - 1.0 + dx.round())).abs() < 1e-2 && (px.1 - (50.0 - 30.0 + dy.round())).abs() < 1e-2, "{px:?}");
        // no text backend: nothing is added
        let mut none = animated_frame(4);
        none.add_letters(Size::new(1920.0, 1080.0), 0.0, &mut |_| None);
        assert_eq!(none.quads.len(), 1);
    }

    #[test]
    fn a_line_without_moving_letters_asks_for_none() {
        use crate::view::{Align, Draw};
        let r = quillstone_script::markup::rich("a {u}b{/u}");
        let d = Draw::Text { text: r.text, runs: r.runs, visible_chars: 9, rect: Rect::new(0.0, 0.0, 400.0, 100.0), size: 40.0, color: [1.0; 4], align: Align::Left, outline: None };
        let mut f = crate::batch::build_frame(&[d], Size::new(1920.0, 1080.0));
        assert!(!f.animated());
        f.add_letters(Size::new(1920.0, 1080.0), 0.0, &mut |_| panic!("not asked"));
        assert_eq!(f.quads.len(), 1);
    }
}
