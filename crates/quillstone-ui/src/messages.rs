//! The Messages screen: a list of contacts and, opened from it, a thread of bubbles (theirs left, the player's
//! right) with the reply options at the bottom, plus the enlarged picture or video panel. Layout, hit-testing
//! and drawing; what the input does is in `game/messages.rs`. The look uses the theme's `button_color` (their
//! bubbles), `accent_color` (the player's), `overlay_color`, `text_color` and `dim_color`, unless the theme's `bubble_*`,
//! `messages_*` keys say otherwise.

use quillstone_render::{fit_row, line_height, lines_of, push_image, Align, Draw, Measure, Rect, Size, Theme, VIRTUAL};
use quillstone_script::markup::plain;
use quillstone_script::Fit;

use crate::input::{picture_or_rect, rich_text, screen, text};
use crate::screens::draw_full_picture;
use crate::texts::Tr;

/// Most reply options drawn at the foot of a thread.
pub const MAX_OPTIONS: usize = 6;

const MARGIN: f32 = 24.0;
const CONTENT_MAX: f32 = 1400.0;
const HEAD: f32 = 104.0;
const ROW_H: f32 = 112.0;
const ROW_GAP: f32 = 10.0;
const PAD: f32 = 18.0;
const PIC_MAX: Size = Size::new(520.0, 340.0);
const GAP: f32 = 14.0;
const OPTION_H: f32 = 58.0;
const OPTION_GAP: f32 = 8.0;

/// Where everything is: inside the theme's `messages_rect` (the whole screen by default), the content as wide as
/// the area but at most [`CONTENT_MAX`], text at `messages_text_size`.
#[derive(Debug, Clone, Copy)]
pub struct Geo {
    pub area: Rect,
    pub x: f32,
    pub w: f32,
    pub text: f32,
    /// Width of the Back control: a labelled button, or a small picture when the theme has `messages_back_image`.
    pub back_w: f32,
}

impl Geo {
    pub fn new(theme: &Theme) -> Geo {
        let [x, y, w, h] = theme.messages_rect;
        let cw = (w - 2.0 * MARGIN).min(CONTENT_MAX);
        Geo { area: Rect::new(x, y, w, h), x: x + (w - cw) / 2.0, w: cw, text: theme.messages_text_size, back_w: if theme.messages_back_image.is_some() { 56.0 } else { 150.0 } }
    }

    pub fn back_rect(&self) -> Rect {
        Rect::new(self.area.x + MARGIN, self.area.y + MARGIN, self.back_w, 52.0)
    }

    /// The round portrait in a chat's header, right of Back.
    pub fn avatar_rect(&self) -> Rect {
        let b = self.back_rect();
        Rect::new(b.x + b.w + 10.0, b.y - 2.0, 56.0, 56.0)
    }

    /// Centred over the area, or (in a narrow one, like a phone's) in the space right of Back; with an avatar,
    /// left-aligned right of it.
    fn title_rect(&self, avatar: bool) -> Rect {
        if avatar {
            let a = self.avatar_rect();
            let x = a.x + a.w + 12.0;
            return Rect::new(x, self.area.y + MARGIN, (self.area.x + self.area.w - x - MARGIN).max(60.0), 52.0);
        }
        let left = self.back_rect().x + self.back_rect().w + 16.0 - self.area.x;
        let wide = self.area.w - 2.0 * left;
        if wide >= 240.0 {
            Rect::new(self.area.x + left, self.area.y + MARGIN, wide, 52.0)
        } else {
            Rect::new(self.area.x + left, self.area.y + MARGIN, (self.area.w - left - MARGIN).max(60.0), 52.0)
        }
    }

    /// Contact rows that fit.
    pub fn rows(&self) -> usize {
        (((self.area.h - HEAD - MARGIN) / (ROW_H + ROW_GAP)).floor() as usize).max(1)
    }

    pub fn contact_rect(&self, i: usize) -> Rect {
        Rect::new(self.x, self.area.y + HEAD + i as f32 * (ROW_H + ROW_GAP), self.w, ROW_H)
    }

    /// The contact row at the point (`n` rows are visible), if any.
    pub fn contact_at(&self, x: f32, y: f32, n: usize) -> Option<usize> {
        (0..n.min(self.rows())).find(|&i| self.contact_rect(i).contains(x, y))
    }

    /// The reply option buttons, top to bottom, at the foot of a thread.
    pub fn option_rect(&self, i: usize, n: usize) -> Rect {
        let n = n.min(MAX_OPTIONS);
        let bottom = self.area.y + self.area.h - MARGIN;
        Rect::new(self.x, bottom - (n - i.min(n)) as f32 * (OPTION_H + OPTION_GAP) + OPTION_GAP, self.w, OPTION_H)
    }

    pub fn option_at(&self, x: f32, y: f32, n: usize) -> Option<usize> {
        (0..n.min(MAX_OPTIONS)).find(|&i| self.option_rect(i, n).contains(x, y))
    }

    /// Where the bubbles end: above the options.
    fn thread_bottom(&self, options: usize) -> f32 {
        if options == 0 {
            self.area.y + self.area.h - MARGIN
        } else {
            self.option_rect(0, options).y - 16.0
        }
    }
}

/// The picture or video of a bubble. `src` is what is drawn in it: the picture, or the video's thumbnail (none: a placeholder).
#[derive(Debug, Clone, PartialEq)]
pub struct Pic {
    pub src: Option<String>,
    pub video: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Bubble {
    pub mine: bool,
    pub text: Option<String>,
    pub pic: Option<Pic>,
}

pub struct ContactRow {
    pub name: String,
    pub portrait: Option<String>,
    pub unread: usize,
    pub preview: String,
    pub waiting: bool,
}

/// A bubble placed on the screen: the whole bubble and the picture inside it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Placed {
    pub index: usize,
    pub rect: Rect,
    pub pic: Option<Rect>,
}

/// The size a picture is drawn at in its bubble (fitted into `PIC_MAX` and the bubble's width, never enlarged), or a
/// video's default shape.
fn pic_size(pic: &Pic, max_w: f32, image_size: &dyn Fn(&str) -> Option<Size>) -> Size {
    let natural = pic.src.as_deref().and_then(image_size).unwrap_or(Size::new(480.0, 270.0));
    let k = (PIC_MAX.w.min(max_w) / natural.w).min(PIC_MAX.h / natural.h).min(1.0);
    Size::new((natural.w * k).max(1.0), (natural.h * k).max(1.0))
}

/// The bubbles that fit above `options` reply buttons, newest at the bottom; `scroll` skips that many of the newest.
pub fn layout_thread(g: &Geo, bubbles: &[Bubble], scroll: usize, options: usize, image_size: &dyn Fn(&str) -> Option<Size>, measure: Option<Measure>) -> Vec<Placed> {
    let top = g.area.y + HEAD;
    let mut bottom = g.thread_bottom(options);
    let mut out = Vec::new();
    let bubble_max = (g.w * 0.78).max(120.0);
    for (index, b) in bubbles.iter().enumerate().rev().skip(scroll) {
        let plain_text = b.text.as_deref().map(plain);
        let inner_max = bubble_max - 2.0 * PAD;
        let pic = b.pic.as_ref().map(|p| pic_size(p, inner_max, image_size));
        // The bubble is as wide as its widest line and as tall as the lines the text really wraps to.
        let (lines, text_w) = plain_text.as_deref().map_or((0, 0.0), |t| lines_of(measure, t, g.text, inner_max));
        let inner_w = (text_w + 3.0).min(inner_max).max(pic.map_or(0.0, |s| s.w));
        let text_h = lines as f32 * line_height(g.text);
        let h = 2.0 * PAD + pic.map_or(0.0, |s| s.h + if lines > 0 { 10.0 } else { 0.0 }) + text_h;
        let w = inner_w + 2.0 * PAD;
        let y = bottom - h;
        if y < top && !out.is_empty() {
            break; // only whole bubbles are drawn (a tall one alone is still shown)
        }
        let x = if b.mine { g.x + g.w - w } else { g.x };
        let rect = Rect::new(x, y, w, h);
        let pic = pic.map(|s| Rect::new(x + PAD, y + PAD, s.w, s.h));
        out.push(Placed { index, rect, pic });
        bottom = y - GAP;
    }
    out
}

/// The bubble picture at the point, if any (`placed` from [`layout_thread`]).
pub fn picture_at(placed: &[Placed], x: f32, y: f32) -> Option<usize> {
    placed.iter().find(|p| p.pic.is_some_and(|r| r.contains(x, y))).map(|p| p.index)
}

pub(crate) fn lit(theme: &Theme, on: bool) -> [f32; 4] {
    if on { theme.button_hover_color } else { theme.button_color }
}

fn image_in(out: &mut Vec<Draw>, src: &str, into: Rect, image_size: &dyn Fn(&str) -> Option<Size>) {
    push_image(out, src, into, Fit::Contain, 1.0, image_size);
}

/// The backdrop (the dim, then the theme's `messages_image` covering the screen), the title and the Back button.
/// `pic` over `dest`: as a 9-slice with `border` picture pixels (stretched whole when 0), or `color` without a picture.
pub(crate) fn skin(out: &mut Vec<Draw>, pic: Option<&str>, border: f32, dest: Rect, color: [f32; 4], image_size: &dyn Fn(&str) -> Option<Size>) {
    match pic.and_then(|s| image_size(s).map(|z| (s, z))) {
        Some((s, z)) if border > 0.0 => crate::screens::nine_slice(out, s, z, dest, border),
        _ => picture_or_rect(out, pic, dest, color, image_size),
    }
}

/// A contact row, reply option or Back button: the theme's button picture (its hover one while lit), else the colour.
fn button(out: &mut Vec<Draw>, theme: &Theme, dest: Rect, on: bool, image_size: &dyn Fn(&str) -> Option<Size>) {
    let pic = if on { theme.messages_button_hover.as_deref().or(theme.messages_button_image.as_deref()) } else { theme.messages_button_image.as_deref() };
    skin(out, pic, theme.messages_button_border, dest, lit(theme, on), image_size);
}

#[allow(clippy::too_many_arguments)]
fn draw_frame(out: &mut Vec<Draw>, theme: &Theme, tr: Tr, g: &Geo, title: &str, avatar: Option<&str>, back_hover: bool, image_size: &dyn Fn(&str) -> Option<Size>) {
    out.push(Draw::Rect { rect: screen(), color: theme.overlay_color });
    draw_full_picture(out, theme.messages_image.as_deref(), image_size);
    // `avatar`: a chat header (with the contact's portrait); none: the list.
    text(out, title, g.title_rect(avatar.is_some()), 40.0, theme.text_color, if avatar.is_some() { Align::Left } else { Align::Center });
    if let Some(src) = avatar {
        let r = g.avatar_rect();
        image_in(out, src, r, image_size);
        skin(out, theme.messages_avatar_frame.as_deref(), 0.0, r, [0.0; 4], image_size);
    }
    let back = if back_hover { theme.messages_back_hover.as_deref().or(theme.messages_back_image.as_deref()) } else { theme.messages_back_image.as_deref() };
    match back.filter(|s| image_size(s).is_some()) {
        Some(src) => image_in(out, src, g.back_rect(), image_size),
        None => {
            button(out, theme, g.back_rect(), back_hover, image_size);
            text(out, tr.t("Back"), g.back_rect(), 28.0, theme.text_color, Align::Center);
        }
    }
}

/// The contact list, `rows` already cut to the visible window. `hover` is a visible row.
#[allow(clippy::too_many_arguments)]
pub fn draw_contacts(out: &mut Vec<Draw>, theme: &Theme, tr: Tr, rows: &[ContactRow], hover: Option<usize>, back_hover: bool, image_size: &dyn Fn(&str) -> Option<Size>, measure: Option<Measure>) {
    let g = Geo::new(theme);
    draw_frame(out, theme, tr, &g, tr.t("Messages"), None, back_hover, image_size);
    if rows.is_empty() {
        text(out, tr.t("No messages yet."), Rect::new(g.area.x, g.area.y + HEAD + 40.0, g.area.w, 60.0), g.text, theme.dim_color, Align::Center);
        return;
    }
    for (i, row) in rows.iter().enumerate() {
        let r = g.contact_rect(i);
        button(out, theme, r, hover == Some(i), image_size);
        let face = Rect::new(r.x + 12.0, r.y + 12.0, r.h - 24.0, r.h - 24.0);
        out.push(Draw::Rect { rect: face, color: theme.panel_color });
        if let Some(src) = &row.portrait {
            image_in(out, src, face, image_size);
        }
        let x = face.x + face.w + 20.0;
        let w = r.x + r.w - x - 20.0;
        let (name_size, small) = (g.text * 1.05, g.text * 0.8);
        let name_color = if row.unread > 0 { theme.accent_color } else { theme.text_color };
        text(out, fit_row(measure, &row.name, name_size, w * 0.6), Rect::new(x, r.y + 10.0, w * 0.6, name_size * 1.3), name_size, name_color, Align::Left);
        text(out, fit_row(measure, &row.preview, small, w), Rect::new(x, r.y + r.h - small * 1.3 - 12.0, w, small * 1.3), small, theme.dim_color, Align::Left);
        let badge = if row.waiting {
            Some(tr.t("Waiting for your reply").to_string())
        } else if row.unread > 0 {
            Some(tr.f("{n} new", &[("n", &row.unread)]))
        } else {
            None
        };
        if let Some(label) = badge {
            text(out, label, Rect::new(x + w * 0.5, r.y + 12.0, w * 0.5, small * 1.3), small, theme.accent_color, Align::Right);
        }
    }
}

/// A thread: `placed` from [`layout_thread`], then the reply `options` (the picked-out one lit).
#[allow(clippy::too_many_arguments)]
pub fn draw_thread(
    out: &mut Vec<Draw>,
    theme: &Theme,
    tr: Tr,
    name: &str,
    portrait: Option<&str>,
    bubbles: &[Bubble],
    placed: &[Placed],
    options: &[String],
    option_hover: Option<usize>,
    back_hover: bool,
    image_size: &dyn Fn(&str) -> Option<Size>,
) {
    let g = Geo::new(theme);
    draw_frame(out, theme, tr, &g, name, portrait, back_hover, image_size);
    if bubbles.is_empty() {
        text(out, tr.t("No messages yet."), Rect::new(g.area.x, g.area.y + HEAD + 40.0, g.area.w, 60.0), g.text, theme.dim_color, Align::Center);
    }
    for p in placed {
        let b = &bubbles[p.index];
        let [r, gr, bl, _] = theme.accent_color;
        let (color, image, text_color) = if b.mine {
            (theme.bubble_me.unwrap_or([r, gr, bl, 0.35]), theme.bubble_me_image.as_deref(), theme.bubble_text_me)
        } else {
            (theme.bubble_them.unwrap_or(theme.button_color), theme.bubble_them_image.as_deref(), theme.bubble_text_them)
        };
        let text_color = text_color.unwrap_or(theme.text_color);
        skin(out, image, theme.bubble_border, p.rect, color, image_size);
        let mut y = p.rect.y + PAD;
        if let (Some(pic), Some(area)) = (&b.pic, p.pic) {
            match pic.src.as_deref().filter(|s| image_size(s).is_some()) {
                Some(src) => image_in(out, src, area, image_size),
                None => out.push(Draw::Rect { rect: area, color: theme.panel_color }),
            }
            if pic.video {
                let mark = Rect::new(area.x + area.w / 2.0 - 36.0, area.y + area.h / 2.0 - 36.0, 72.0, 72.0);
                out.push(Draw::Rect { rect: mark, color: [0.0, 0.0, 0.0, 0.6] });
                text(out, "▶", mark, 44.0, [1.0; 4], Align::Center);
            }
            y = area.y + area.h + 10.0;
        }
        if let Some(t) = &b.text {
            let rect = Rect::new(p.rect.x + PAD, y, p.rect.w - 2.0 * PAD, p.rect.y + p.rect.h - PAD - y);
            rich_text(out, t, rect, g.text, text_color, Align::Left);
        }
    }
    for (i, o) in options.iter().enumerate().take(MAX_OPTIONS) {
        let r = g.option_rect(i, options.len());
        button(out, theme, r, option_hover == Some(i), image_size);
        rich_text(out, o, r, g.text * 0.85, theme.accent_color, Align::Center);
    }
}

/// The centred panel an enlarged picture or video fills: it fits `at most` 2/3 of the screen, keeping its shape
/// (a small picture is enlarged at most twice).
pub fn viewer_rect(natural: Option<Size>) -> Rect {
    let natural = natural.unwrap_or(Size::new(1280.0, 720.0));
    let (max_w, max_h) = (VIRTUAL.w * 2.0 / 3.0, VIRTUAL.h * 2.0 / 3.0);
    let k = (max_w / natural.w).min(max_h / natural.h).min(2.0);
    let (w, h) = (natural.w * k, natural.h * k);
    Rect::new((VIRTUAL.w - w) / 2.0, (VIRTUAL.h - h) / 2.0 - 20.0, w, h)
}

/// The enlarged view over the dimmed screen: `src` is the picture, or the playing video's frame with the thumbnail as fallback.
pub fn draw_viewer(out: &mut Vec<Draw>, theme: &Theme, tr: Tr, srcs: &[&str], image_size: &dyn Fn(&str) -> Option<Size>) {
    out.push(Draw::Rect { rect: screen(), color: [0.0, 0.0, 0.0, 0.7] });
    let natural = srcs.iter().find_map(|s| image_size(s));
    let panel = viewer_rect(natural);
    out.push(Draw::Rect { rect: Rect::new(panel.x - 6.0, panel.y - 6.0, panel.w + 12.0, panel.h + 12.0), color: theme.panel_color });
    for s in srcs {
        image_in(out, s, panel, image_size);
    }
    text(out, tr.t("Click outside or Esc to go back"), Rect::new(0.0, VIRTUAL.h - 60.0, VIRTUAL.w, 36.0), 24.0, theme.dim_color, Align::Center);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn none(_: &str) -> Option<Size> {
        None
    }

    fn msg(mine: bool, t: &str) -> Bubble {
        Bubble { mine, text: Some(t.into()), pic: None }
    }

    #[test]
    fn bubbles_stack_up_from_the_bottom_on_their_own_side_and_scroll_by_whole_bubbles() {
        let g = Geo::new(&Theme::default());
        let bubbles: Vec<Bubble> = (0..30).map(|i| msg(i % 2 == 1, &format!("Message number {i}"))).collect();
        let placed = layout_thread(&g, &bubbles, 0, 0, &none, None);
        assert_eq!(placed[0].index, 29, "the newest is at the bottom");
        assert!(placed[0].rect.x > g.x && placed[1].rect.x == g.x, "the player's on the right, theirs on the left");
        assert!(placed.len() < 30 && placed.iter().all(|p| p.rect.y >= g.area.y + HEAD && p.rect.y + p.rect.h <= VIRTUAL.h));
        assert_eq!(layout_thread(&g, &bubbles, 5, 0, &none, None)[0].index, 24);
        let with = layout_thread(&g, &bubbles, 0, 2, &none, None);
        assert!(with[0].rect.y + with[0].rect.h < g.option_rect(0, 2).y);
        assert_eq!(g.option_at(g.option_rect(1, 2).x + 5.0, g.option_rect(1, 2).y + 5.0, 2), Some(1));
    }

    #[test]
    fn a_bubble_is_one_line_tall_when_the_text_really_measures_one_line() {
        let g = Geo::new(&Theme::default());
        let b = [msg(true, "Sure, I'll bring tea. A longer phrase the estimate would call two lines at this width")];
        let measure = |_: &str, _: f32, _: f32| Some((1, 240.0));
        let p = layout_thread(&g, &b, 0, 0, &none, Some(&measure));
        assert_eq!(p[0].rect.h, 2.0 * PAD + line_height(g.text), "one line plus padding");
        assert_eq!(p[0].rect.w, 243.0 + 2.0 * PAD, "as wide as the widest line");
        let two = |_: &str, _: f32, _: f32| Some((2, 300.0));
        assert_eq!(layout_thread(&g, &b, 0, 0, &none, Some(&two))[0].rect.h, 2.0 * PAD + 2.0 * line_height(g.text));
    }

    #[test]
    fn a_picture_is_fitted_into_its_bubble_and_found_by_a_click() {
        let g = Geo::new(&Theme::default());
        let b = [Bubble { mine: false, text: Some("look".into()), pic: Some(Pic { src: Some("big.png".into()), video: false }) }];
        let size = |s: &str| (s == "big.png").then_some(Size::new(2000.0, 1000.0));
        let placed = layout_thread(&g, &b, 0, 0, &size, None);
        let r = placed[0].pic.unwrap();
        assert!(r.w <= PIC_MAX.w && (r.w / r.h - 2.0).abs() < 0.01, "{r:?}");
        assert_eq!(picture_at(&placed, r.x + 3.0, r.y + 3.0), Some(0));
        assert_eq!(picture_at(&placed, 0.0, 0.0), None);
    }

    #[test]
    fn a_messages_rect_holds_everything_in_a_phone_sized_area() {
        let theme = Theme::parse("messages_rect = 700, 100, 500, 880\nmessages_text_size = 26\n").0;
        let g = Geo::new(&theme);
        let inside = |r: Rect| r.x >= 700.0 && r.y >= 100.0 && r.x + r.w <= 1200.0 && r.y + r.h <= 980.0;
        assert!(inside(g.back_rect()) && (0..g.rows()).all(|i| inside(g.contact_rect(i))) && (0..3).all(|i| inside(g.option_rect(i, 3))));
        let bubbles = vec![msg(false, "A fairly long message that must wrap inside the narrow phone"), msg(true, "ok")];
        assert!(layout_thread(&g, &bubbles, 0, 2, &none, None).iter().all(|p| inside(p.rect)));
    }

    #[test]
    fn the_viewer_panel_fits_two_thirds_of_the_screen() {
        let r = viewer_rect(Some(Size::new(3840.0, 2160.0)));
        assert!(r.w <= VIRTUAL.w * 2.0 / 3.0 + 0.1 && r.h <= VIRTUAL.h * 2.0 / 3.0 + 0.1);
        assert_eq!(viewer_rect(Some(Size::new(100.0, 100.0))).w, 200.0, "enlarged at most twice");
    }
}
