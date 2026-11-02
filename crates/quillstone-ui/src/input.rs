//! The text-entry screen of `::input`: a centred panel with the prompt, a text
//! field and an OK button. Layout, caret blink, what part of a long text fits the
//! field, and drawing; what typing does is in `game/input.rs`.
//!
//! Everything is laid out in the virtual 1920x1080 screen from fixed sizes inside the panel (the
//! theme's colours and font apply; `input_rect` moves and widens the panel, `input_image` dresses it).

use quillstone_render::theme::DEFAULT_INPUT_RECT;
use quillstone_render::{estimate_lines, Align, Draw, Rect, Size, Theme, FULL_UV, VIRTUAL};
use quillstone_script::markup::rich;

/// The panel's default placement and the height its parts are stacked in (a taller panel centres them).
const PANEL: Rect = Rect::new(DEFAULT_INPUT_RECT[0], DEFAULT_INPUT_RECT[1], DEFAULT_INPUT_RECT[2], DEFAULT_INPUT_RECT[3]);
const PAD: f32 = 20.0;
const PROMPT_SIZE: f32 = 44.0;
const PROMPT_SMALL: f32 = 32.0;
const FIELD_SIZE: f32 = 44.0;
/// Seconds for one blink of the caret (on for the first half).
pub const BLINK_PERIOD: f32 = 1.0;

pub fn panel_rect(theme: &Theme) -> Rect {
    theme.input_rect.map_or(PANEL, |[x, y, w, h]| Rect::new(x, y, w, h))
}

/// A part of the panel: `dx`, `dy` from the stack's top left, `w` less than the panel's width by `narrower`.
fn part(theme: &Theme, dx: f32, dy: f32, narrower: f32, h: f32) -> Rect {
    let p = panel_rect(theme);
    Rect::new(p.x + dx, p.y + (p.h - PANEL.h) / 2.0 + dy, p.w - narrower, h)
}

pub fn prompt_rect(theme: &Theme) -> Rect {
    part(theme, 60.0, 40.0, 120.0, 150.0)
}

pub fn field_rect(theme: &Theme) -> Rect {
    part(theme, 120.0, 210.0, 240.0, 80.0)
}

/// Where the text starts: inside the field, with room on both sides.
fn text_rect(theme: &Theme) -> Rect {
    let f = field_rect(theme);
    // `Align::Left` starts at the top, so the text is moved down to the middle of the field.
    Rect::new(f.x + PAD, f.y + 12.0, f.w - 2.0 * PAD, f.h - 12.0)
}

fn count_rect(theme: &Theme) -> Rect {
    let f = field_rect(theme);
    Rect::new(f.x, f.y + f.h + 6.0, f.w, 32.0)
}

pub fn ok_rect(theme: &Theme) -> Rect {
    let r = part(theme, 0.0, 340.0, 0.0, 80.0);
    Rect::new(r.x + (r.w - 260.0) / 2.0, r.y, 260.0, r.h)
}

/// `true` over the OK button.
pub fn ok_at(theme: &Theme, x: f32, y: f32) -> bool {
    ok_rect(theme).contains(x, y)
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

pub(crate) fn line(out: &mut Vec<Draw>, rect: Rect, color: [f32; 4]) {
    out.push(Draw::Rect { rect, color });
}

/// A 2 px frame just inside `r`.
fn outline(out: &mut Vec<Draw>, r: Rect, color: [f32; 4]) {
    line(out, Rect::new(r.x, r.y, r.w, 2.0), color);
    line(out, Rect::new(r.x, r.y + r.h - 2.0, r.w, 2.0), color);
    line(out, Rect::new(r.x, r.y, 2.0, r.h), color);
    line(out, Rect::new(r.x + r.w - 2.0, r.y, 2.0, r.h), color);
}

pub(crate) fn text(out: &mut Vec<Draw>, s: impl Into<String>, rect: Rect, size: f32, color: [f32; 4], align: Align) {
    out.push(Draw::Text { text: s.into(), runs: Vec::new(), visible_chars: usize::MAX, rect, size, color, align, outline: None });
}

/// [`text`] for a prompt written with `{b}`/`{i}`/colour tags.
pub(crate) fn rich_text(out: &mut Vec<Draw>, prompt: &str, rect: Rect, size: f32, color: [f32; 4]) {
    let r = rich(prompt);
    out.push(Draw::Text { text: r.text, runs: r.runs, visible_chars: usize::MAX, rect, size, color, align: Align::Center, outline: None });
}

/// The whole virtual screen.
pub(crate) fn screen() -> Rect {
    Rect::new(0.0, 0.0, VIRTUAL.w, VIRTUAL.h)
}

/// `dest` filled with the theme picture `pic` when it is loaded, else with `color`.
pub(crate) fn picture_or_rect(out: &mut Vec<Draw>, pic: Option<&str>, dest: Rect, color: [f32; 4], image_size: &dyn Fn(&str) -> Option<Size>) {
    match pic.filter(|src| image_size(src).is_some()) {
        Some(src) => out.push(Draw::Image { src: src.to_string(), dest, uv: FULL_UV, alpha: 1.0, shade: 1.0, blur: 0.0 }),
        None => out.push(Draw::Rect { rect: dest, color }),
    }
}

pub fn draw_input(out: &mut Vec<Draw>, theme: &Theme, s: &InputScreen, image_size: &dyn Fn(&str) -> Option<Size>) {
    out.push(Draw::Rect { rect: screen(), color: theme.overlay_color });
    let panel = panel_rect(theme);
    picture_or_rect(out, theme.input_image.as_deref(), panel, theme.panel_color, image_size);

    let pr = prompt_rect(theme);
    let size = if estimate_lines(&rich(s.prompt).text, PROMPT_SIZE, pr.w) > 3 { PROMPT_SMALL } else { PROMPT_SIZE };
    rich_text(out, s.prompt, pr, size, theme.text_color);

    // The field: a dark well with a thin accent outline.
    let f = field_rect(theme);
    line(out, f, [0.0, 0.0, 0.0, 0.45]);
    outline(out, f, theme.accent_color);

    let tr = text_rect(theme);
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
    text(out, format!("{}/{}", s.text.chars().count(), s.max), count_rect(theme), 22.0, theme.dim_color, Align::Center);

    let ok = ok_rect(theme);
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
        let t = &Theme::default();
        assert!(inside(PANEL, screen));
        for r in [prompt_rect(t), field_rect(t), text_rect(t), count_rect(t), ok_rect(t)] {
            assert!(inside(r, PANEL), "{r:?}");
        }
        assert!(inside(text_rect(t), field_rect(t)));
        let (p, f, c, ok) = (prompt_rect(t), field_rect(t), count_rect(t), ok_rect(t));
        assert!(p.y + p.h <= f.y && f.y + f.h <= c.y && c.y + c.h <= ok.y, "stacked without overlap");
    }

    #[test]
    fn the_panel_is_centred() {
        assert!((PANEL.x * 2.0 + PANEL.w - VIRTUAL.w).abs() < 0.01);
        assert!((PANEL.y * 2.0 + PANEL.h - VIRTUAL.h).abs() < 0.01);
    }

    #[test]
    fn only_the_ok_button_is_a_hit() {
        let t = &Theme::default();
        let ok = ok_rect(t);
        assert!(ok_at(t, ok.x + 1.0, ok.y + 1.0) && ok_at(t, ok.x + ok.w / 2.0, ok.y + ok.h / 2.0));
        assert!(!ok_at(t, ok.x - 1.0, ok.y) && !ok_at(t, ok.x, ok.y + ok.h + 1.0));
        let f = field_rect(t);
        assert!(!ok_at(t, f.x + 5.0, f.y + 5.0));
    }

    #[test]
    fn the_caret_blinks_on_then_off() {
        assert!(caret_visible(0.0) && caret_visible(0.49));
        assert!(!caret_visible(0.5) && !caret_visible(0.99));
        assert!(caret_visible(1.0) && !caret_visible(1.7), "and round again");
    }

    #[test]
    fn a_long_text_shows_its_last_characters_inside_the_field() {
        let width = text_rect(&Theme::default()).w - 24.0;
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
                draw_input(&mut out, &theme, &s, &|_| None);
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
        draw_input(&mut out, &theme, &mk("", true), &|_| None);
        assert!(texts(&out).contains(&"Alex".to_string()) && texts(&out).contains(&"0/16".to_string()));
        let mut out = Vec::new();
        draw_input(&mut out, &theme, &mk("Sam", true), &|_| None);
        assert!(!texts(&out).contains(&"Alex".to_string()));
        assert!(texts(&out).contains(&"Sam|".to_string()), "the caret follows the text: {:?}", texts(&out));
        let ok_fill = |out: &[Draw]| out.iter().find_map(|d| match d {
            Draw::Rect { rect, color } if *rect == ok_rect(&theme) => Some(*color),
            _ => None,
        });
        let (mut a, mut b) = (Vec::new(), Vec::new());
        draw_input(&mut a, &theme, &mk("", true), &|_| None);
        draw_input(&mut b, &theme, &mk("", false), &|_| None);
        assert!(ok_fill(&b).unwrap()[3] < ok_fill(&a).unwrap()[3], "dimmed");
    }

    #[test]
    fn input_rect_and_input_image_move_and_dress_the_panel() {
        let theme = Theme::parse("input_rect = 100, 200, 1000, 600\ninput_image = gui/in.png\n").0;
        let panel = panel_rect(&theme);
        assert_eq!(panel, Rect::new(100.0, 200.0, 1000.0, 600.0));
        for r in [prompt_rect(&theme), field_rect(&theme), count_rect(&theme), ok_rect(&theme)] {
            assert!(inside(r, panel), "{r:?}");
        }
        let ok = ok_rect(&theme);
        assert!(ok_at(&theme, ok.x + 5.0, ok.y + 5.0), "the button is hit where it is drawn");
        let old = ok_rect(&Theme::default());
        assert!(!ok_at(&theme, old.x + 5.0, old.y + 5.0), "and no longer where it was");
        let s = InputScreen { prompt: "Name?", text: "", placeholder: None, max: 8, caret: false, can_confirm: true, hover_ok: false };
        let (mut with, mut without) = (Vec::new(), Vec::new());
        draw_input(&mut with, &theme, &s, &|_| Some(Size::new(64.0, 64.0)));
        draw_input(&mut without, &theme, &s, &|_| None);
        assert!(with.iter().any(|d| matches!(d, Draw::Image { src, dest, .. } if src == "gui/in.png" && *dest == panel)));
        assert!(!without.iter().any(|d| matches!(d, Draw::Image { .. })), "a picture that isn't loaded leaves the panel colour");
        assert!(without.iter().any(|d| matches!(d, Draw::Rect { rect, color } if *rect == panel && *color == theme.panel_color)));
    }
}
