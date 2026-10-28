//! The text-entry screen of `::input`: a centred panel with the prompt, a text
//! field and an OK button. Layout, caret blink, what part of a long text fits the
//! field, and drawing; what typing does is in `game/input.rs`.
//!
//! Everything is laid out in the virtual 1920x1080 screen from fixed sizes (the
//! theme's colours, font and text size apply; there are no theme keys of its own).

use quillstone_render::{estimate_lines, Align, Draw, Rect, Theme, VIRTUAL};
use quillstone_script::markup::rich;

const PANEL: Rect = Rect::new(360.0, 310.0, 1200.0, 460.0);
const PAD: f32 = 20.0;
const PROMPT_SIZE: f32 = 44.0;
const PROMPT_SMALL: f32 = 32.0;
const FIELD_SIZE: f32 = 44.0;
/// Seconds for one blink of the caret (on for the first half).
pub const BLINK_PERIOD: f32 = 1.0;

pub fn panel_rect() -> Rect {
    PANEL
}

pub fn prompt_rect() -> Rect {
    Rect::new(PANEL.x + 60.0, PANEL.y + 40.0, PANEL.w - 120.0, 150.0)
}

pub fn field_rect() -> Rect {
    Rect::new(PANEL.x + 120.0, PANEL.y + 210.0, PANEL.w - 240.0, 80.0)
}

/// Where the text starts: inside the field, with room on both sides.
fn text_rect() -> Rect {
    let f = field_rect();
    // `Align::Left` starts at the top, so the text is moved down to the middle of the field.
    Rect::new(f.x + PAD, f.y + 12.0, f.w - 2.0 * PAD, f.h - 12.0)
}

fn count_rect() -> Rect {
    let f = field_rect();
    Rect::new(f.x, f.y + f.h + 6.0, f.w, 32.0)
}

pub fn ok_rect() -> Rect {
    Rect::new(PANEL.x + (PANEL.w - 260.0) / 2.0, PANEL.y + 340.0, 260.0, 80.0)
}

/// `true` over the OK button.
pub fn ok_at(x: f32, y: f32) -> bool {
    ok_rect().contains(x, y)
}

/// Whether the caret is drawn `elapsed` seconds after the last keystroke (it is
/// solid while typing: the game restarts the clock on every keystroke).
pub fn caret_visible(elapsed: f32) -> bool {
    elapsed.rem_euclid(BLINK_PERIOD) < BLINK_PERIOD / 2.0
}

/// How wide `c` is, as a fraction of the text size. A guess: the backend's text
/// engine does the real layout; this errs on the wide side so what is shown fits.
fn em(c: char) -> f32 {
    match c {
        'i' | 'j' | 'l' | 't' | 'f' | 'I' | '.' | ',' | ':' | ';' | '\'' | '!' | '|' | ' ' => 0.38,
        'm' | 'w' | 'M' | 'W' => 0.9,
        c if c.is_uppercase() => 0.75,
        '\u{1100}'..='\u{11FF}' | '\u{2E80}'..='\u{D7FF}' | '\u{F900}'..='\u{FAFF}' | '\u{FE30}'..='\u{FE6F}' | '\u{FF00}'..='\u{FF60}'
        | '\u{FFE0}'..='\u{FFE6}' | '\u{1F000}'..='\u{1FFFF}' | '\u{20000}'..='\u{3FFFF}' => 1.05,
        _ => 0.62,
    }
}

/// The end of `s` that fits `width` px at `size` px: a field shows the last
/// characters typed, so the caret stays in view. (Whole text when it fits.)
pub fn visible_tail(s: &str, size: f32, width: f32) -> &str {
    let mut used = 0.0;
    let mut start = s.len();
    for (at, c) in s.char_indices().rev() {
        used += em(c) * size;
        if used > width {
            break;
        }
        start = at;
    }
    &s[start..]
}

/// Everything the screen shows.
pub struct InputScreen<'a> {
    /// Tagged text, `{name}` values already put in.
    pub prompt: &'a str,
    /// What was typed so far.
    pub text: &'a str,
    /// Shown dimmed while the field is empty.
    pub placeholder: Option<&'a str>,
    pub max: usize,
    /// The caret is in its visible half of the blink.
    pub caret: bool,
    /// OK would accept the answer (something typed, or a default to use).
    pub can_confirm: bool,
    pub hover_ok: bool,
}

fn line(out: &mut Vec<Draw>, rect: Rect, color: [f32; 4]) {
    out.push(Draw::Rect { rect, color });
}

/// A 2 px frame just inside `r`.
fn outline(out: &mut Vec<Draw>, r: Rect, color: [f32; 4]) {
    line(out, Rect::new(r.x, r.y, r.w, 2.0), color);
    line(out, Rect::new(r.x, r.y + r.h - 2.0, r.w, 2.0), color);
    line(out, Rect::new(r.x, r.y, 2.0, r.h), color);
    line(out, Rect::new(r.x + r.w - 2.0, r.y, 2.0, r.h), color);
}

fn text(out: &mut Vec<Draw>, s: impl Into<String>, rect: Rect, size: f32, color: [f32; 4], align: Align) {
    out.push(Draw::Text { text: s.into(), runs: Vec::new(), visible_chars: usize::MAX, rect, size, color, align });
}

pub fn draw_input(out: &mut Vec<Draw>, theme: &Theme, s: &InputScreen) {
    out.push(Draw::Rect { rect: Rect::new(0.0, 0.0, VIRTUAL.w, VIRTUAL.h), color: theme.overlay_color });
    out.push(Draw::Rect { rect: PANEL, color: theme.panel_color });

    let prompt = rich(s.prompt);
    let pr = prompt_rect();
    let size = if estimate_lines(&prompt.text, PROMPT_SIZE, pr.w) > 3 { PROMPT_SMALL } else { PROMPT_SIZE };
    out.push(Draw::Text {
        text: prompt.text,
        runs: prompt.runs,
        visible_chars: usize::MAX,
        rect: pr,
        size,
        color: theme.text_color,
        align: Align::Center,
    });

    // The field: a dark well with a thin accent outline.
    let f = field_rect();
    line(out, f, [0.0, 0.0, 0.0, 0.45]);
    outline(out, f, theme.accent_color);

    let tr = text_rect();
    let caret = if s.caret { "|" } else { "" };
    if s.text.is_empty() {
        text(out, caret, tr, FIELD_SIZE, theme.text_color, Align::Left);
        if let Some(hint) = s.placeholder {
            // Starts just after the caret; cut short when it can't fit.
            let room = Rect::new(tr.x + 14.0, tr.y, tr.w - 14.0, tr.h);
            text(out, visible_head(hint, FIELD_SIZE, room.w), room, FIELD_SIZE, theme.dim_color, Align::Left);
        }
    } else {
        // The caret is part of the text, after its last character, so it follows it.
        let shown = visible_tail(s.text, FIELD_SIZE, tr.w - 24.0);
        text(out, format!("{shown}{caret}"), tr, FIELD_SIZE, theme.text_color, Align::Left);
    }
    text(out, format!("{}/{}", s.text.chars().count(), s.max), count_rect(), 22.0, theme.dim_color, Align::Center);

    let ok = ok_rect();
    if s.can_confirm {
        line(out, ok, if s.hover_ok { theme.button_hover_color } else { theme.button_color });
        outline(out, ok, theme.dim_color); // a button colour close to the panel's would vanish
        text(out, "OK", ok, 40.0, theme.text_color, Align::Center);
    } else {
        let [r, g, b, a] = theme.button_color;
        line(out, ok, [r, g, b, a * 0.4]);
        let [r, g, b, a] = theme.dim_color;
        outline(out, ok, [r, g, b, a * 0.25]);
        text(out, "OK", ok, 40.0, [r, g, b, a * 0.6], Align::Center);
    }
}

/// The start of `s` that fits `width` px (the placeholder is cut, not scrolled).
fn visible_head(s: &str, size: f32, width: f32) -> &str {
    let mut used = 0.0;
    for (at, c) in s.char_indices() {
        used += em(c) * size;
        if used > width {
            return &s[..at];
        }
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    fn inside(r: Rect, outer: Rect) -> bool {
        r.x >= outer.x && r.y >= outer.y && r.x + r.w <= outer.x + outer.w && r.y + r.h <= outer.y + outer.h
    }

    #[test]
    fn the_parts_lie_inside_the_panel_and_the_screen() {
        let screen = Rect::new(0.0, 0.0, VIRTUAL.w, VIRTUAL.h);
        assert!(inside(PANEL, screen));
        for r in [prompt_rect(), field_rect(), text_rect(), count_rect(), ok_rect()] {
            assert!(inside(r, PANEL), "{r:?}");
        }
        assert!(inside(text_rect(), field_rect()));
        let (p, f, c, ok) = (prompt_rect(), field_rect(), count_rect(), ok_rect());
        assert!(p.y + p.h <= f.y && f.y + f.h <= c.y && c.y + c.h <= ok.y, "stacked without overlap");
    }

    #[test]
    fn the_panel_is_centred() {
        assert!((PANEL.x * 2.0 + PANEL.w - VIRTUAL.w).abs() < 0.01);
        assert!((PANEL.y * 2.0 + PANEL.h - VIRTUAL.h).abs() < 0.01);
    }

    #[test]
    fn only_the_ok_button_is_a_hit() {
        let ok = ok_rect();
        assert!(ok_at(ok.x + 1.0, ok.y + 1.0) && ok_at(ok.x + ok.w / 2.0, ok.y + ok.h / 2.0));
        assert!(!ok_at(ok.x - 1.0, ok.y) && !ok_at(ok.x, ok.y + ok.h + 1.0));
        let f = field_rect();
        assert!(!ok_at(f.x + 5.0, f.y + 5.0));
    }

    #[test]
    fn the_caret_blinks_on_then_off() {
        assert!(caret_visible(0.0) && caret_visible(0.49));
        assert!(!caret_visible(0.5) && !caret_visible(0.99));
        assert!(caret_visible(1.0) && !caret_visible(1.7), "and round again");
    }

    #[test]
    fn a_long_text_shows_its_last_characters_inside_the_field() {
        let width = text_rect().w - 24.0;
        assert_eq!(visible_tail("Sam", FIELD_SIZE, width), "Sam");
        assert_eq!(visible_tail("", FIELD_SIZE, width), "");
        let long = "abcdefghijklmnopqrstuvwxyz0123456789abcdefghijklmnopqrstuvwxyz0123";
        let tail = visible_tail(long, FIELD_SIZE, width);
        assert!(tail.len() < long.len() && long.ends_with(tail) && !tail.is_empty());
        let wide: f32 = tail.chars().map(|c| em(c) * FIELD_SIZE).sum();
        assert!(wide <= width, "{wide} > {width}");
        // The widest letters and CJK fit as well (fewer of them).
        let ws = "W".repeat(64);
        assert!(visible_tail(&ws, FIELD_SIZE, width).chars().count() < 30);
        let cjk = "名".repeat(64);
        let n = visible_tail(&cjk, FIELD_SIZE, width).chars().count();
        assert!((15..25).contains(&n), "{n}");
        // Cut on character boundaries.
        assert!(visible_tail(&"é".repeat(70), FIELD_SIZE, width).chars().all(|c| c == 'é'));
    }

    #[test]
    fn the_drawing_stays_inside_the_screen_for_any_text() {
        let theme = Theme::default();
        let screen = Rect::new(0.0, 0.0, VIRTUAL.w, VIRTUAL.h);
        let texts = ["", "Sam", &"W".repeat(64), &"名".repeat(64)];
        for t in texts {
            for (can, caret, hint) in [(true, true, Some("Alex")), (false, false, None), (true, true, Some(&"Q".repeat(80)[..]))] {
                let mut out = Vec::new();
                let s = InputScreen {
                    prompt: "What is **your** name?",
                    text: t,
                    placeholder: hint,
                    max: 64,
                    caret,
                    can_confirm: can,
                    hover_ok: false,
                };
                draw_input(&mut out, &theme, &s);
                for d in &out {
                    match d {
                        Draw::Rect { rect, .. } => assert!(inside(*rect, screen), "{rect:?}"),
                        Draw::Text { rect, .. } => assert!(inside(*rect, PANEL), "{rect:?}"),
                        _ => {}
                    }
                }
            }
        }
    }

    #[test]
    fn the_placeholder_shows_only_while_the_field_is_empty_and_ok_dims_without_an_answer() {
        let theme = Theme::default();
        let texts = |out: &[Draw]| -> Vec<String> {
            out.iter().filter_map(|d| if let Draw::Text { text, .. } = d { Some(text.clone()) } else { None }).collect()
        };
        let mk = |text: &'static str, can| InputScreen {
            prompt: "Name?",
            text,
            placeholder: Some("Alex"),
            max: 16,
            caret: true,
            can_confirm: can,
            hover_ok: false,
        };
        let mut out = Vec::new();
        draw_input(&mut out, &theme, &mk("", true));
        assert!(texts(&out).contains(&"Alex".to_string()) && texts(&out).contains(&"0/16".to_string()));
        let mut out = Vec::new();
        draw_input(&mut out, &theme, &mk("Sam", true));
        assert!(!texts(&out).contains(&"Alex".to_string()));
        assert!(texts(&out).contains(&"Sam|".to_string()), "the caret follows the text: {:?}", texts(&out));
        let ok_fill = |out: &[Draw]| out.iter().find_map(|d| match d {
            Draw::Rect { rect, color } if *rect == ok_rect() => Some(*color),
            _ => None,
        });
        let (mut a, mut b) = (Vec::new(), Vec::new());
        draw_input(&mut a, &theme, &mk("", true));
        draw_input(&mut b, &theme, &mk("", false));
        assert!(ok_fill(&b).unwrap()[3] < ok_fill(&a).unwrap()[3], "dimmed");
    }
}
