//! Measuring text for layouts that size themselves to it (chat bubbles, journal rows). The frontend's text engine
//! knows the real wrapping; it hands a measuring function to [`crate::game::Game::set_text_measure`]. Without one
//! (tests, a frontend that has none) the cautious estimate of `quillstone_render` is used.

use quillstone_render::{estimate_lines, one_row};

/// `(text, size px, width px)` -> `(lines it wraps to, width of the widest line)`; `None`: can't say.
pub type Measure<'a> = &'a dyn Fn(&str, f32, f32) -> Option<(usize, f32)>;

/// The same as an owned function, as the game keeps it.
pub type MeasureFn = Box<dyn Fn(&str, f32, f32) -> Option<(usize, f32)>>;

/// How `text` wraps at `size` in `width`: the real measure, else the estimate (a one-line guess is as wide as the
/// estimate says; a wrapped one fills the width).
pub fn lines_of(measure: Option<Measure>, text: &str, size: f32, width: f32) -> (usize, f32) {
    if let Some(m) = measure.and_then(|m| m(text, size, width)) {
        return m;
    }
    let lines = estimate_lines(text, size, width);
    let one: f32 = text.chars().map(|c| if c.is_ascii() { size * 0.55 } else { size }).sum();
    (lines, if lines > 1 { width } else { one.min(width) })
}

/// `s` on one row `width` wide: whole when it fits, else cut with an ellipsis (as `one_row`, but by the real measure).
pub fn fit_row(measure: Option<Measure>, s: &str, size: f32, width: f32) -> String {
    let Some(m) = measure else { return one_row(s, size, width) };
    let fits = |t: &str| m(t, size, width).is_none_or(|(lines, w)| lines <= 1 && w <= width);
    if fits(s) {
        return s.to_string();
    }
    let chars: Vec<char> = s.chars().collect();
    let (mut lo, mut hi) = (0, chars.len()); // the longest cut that fits
    while lo < hi {
        let mid = (lo + hi).div_ceil(2);
        let cut = chars[..mid].iter().collect::<String>().trim_end().to_string() + "…";
        if fits(&cut) { lo = mid } else { hi = mid - 1 }
    }
    chars[..lo].iter().collect::<String>().trim_end().to_string() + "…"
}
