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

/// How many symbols a glitching run draws its letters as, now and then.
pub const GLITCH_SYMBOLS: usize = 4;
/// What the symbols are picked from.
pub const GLITCH_CHARS: &str = "#%&@$?!*+=~/<>";

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
    /// Its pen position in the text's rectangle, in window pixels.
    pub x: i32,
    pub y: i32,
    /// Its own picture, in [`Letters::pics`].
    pub pic: usize,
    /// The pictures of the symbols a glitch swaps it for (empty for the others).
    pub symbols: Range<usize>,
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
/// A glitch hits a letter in one of this many steps.
const GLITCH_ODDS: u64 = 14;
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
            ((unit(hash(i, step, 1)) * 0.06 * size, unit(hash(i, step, 2)) * 0.06 * size), letter.pic)
        }
        Anim::Wave => ((0.0, (time * WAVE_SPEED - i as f32 * WAVE_LAG).sin() * 0.12 * size), letter.pic),
        Anim::Glitch => {
            let h = hash(i, (time * GLITCH_RATE).floor() as u64, 3);
            if !h.is_multiple_of(GLITCH_ODDS) || letter.symbols.is_empty() {
                return ((0.0, 0.0), letter.pic);
            }
            let jump = (0.08 + 0.1 * (h >> 20 & 0xff) as f32 / 255.0) * size;
            let symbol = letter.symbols.start + (h >> 8) as usize % letter.symbols.len();
            (((if h >> 16 & 1 == 0 { jump } else { -jump }), 0.0), symbol)
        }
    }
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
                // whole pixels keep the letter as sharp as the line is
                let (x, y) = (job.rect.x + (letter.x + p.dx) as f32 + dx.round(), job.rect.y + (letter.y + p.dy) as f32 + dy.round());
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
        Letter { index, anim, x: 100 + 20 * index as i32, y: 50, pic: index, symbols: 10..10 + GLITCH_SYMBOLS }
    }

    #[test]
    fn a_shake_jitters_a_few_pixels_and_changes_over_time() {
        let l = letter(3, Anim::Shake);
        let offsets: Vec<_> = (0..40).map(|k| motion(&l, k as f32 * 0.04, 40.0).0).collect();
        assert!(offsets.iter().all(|(x, y)| x.abs() <= 2.4 && y.abs() <= 2.4), "scaled to the size: {offsets:?}");
        assert!(offsets.windows(2).any(|w| w[0] != w[1]) && offsets.iter().any(|o| o.0.abs() > 0.5));
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
    }

    #[test]
    fn a_glitch_now_and_then_swaps_a_letter_for_a_symbol_and_jumps_it_sideways() {
        let l = letter(2, Anim::Glitch);
        let samples: Vec<_> = (0..400).map(|k| motion(&l, k as f32 / GLITCH_RATE, 40.0)).collect();
        let hit = samples.iter().filter(|s| s.1 != l.pic).count();
        assert!(hit > 5 && hit < 100, "now and then: {hit} of 400");
        for ((dx, dy), pic) in samples {
            if pic == l.pic {
                assert_eq!((dx, dy), (0.0, 0.0));
            } else {
                assert!(l.symbols.contains(&pic) && dx.abs() >= 3.2 && dx.abs() <= 7.3 && dy == 0.0, "{dx} {dy} {pic}");
            }
        }
        let bare = Letter { symbols: 0..0, ..l };
        assert!((0..400).all(|k| motion(&bare, k as f32 / GLITCH_RATE, 40.0).1 == bare.pic), "no symbols, no swap");
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
