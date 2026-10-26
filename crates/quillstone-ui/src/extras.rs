//! The extras screen: a gallery of pictures and a music room, both unlocked by
//! playing (see `quillstone_runtime::Session::unlocked`). Layout, hit-testing and
//! drawing; what the input does is in `game/extras.rs`.

use quillstone_render::{place, Align, Draw, Rect, Size, Theme, VIRTUAL};
use quillstone_script::Fit;

use crate::screens::slots_back_rect;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    Gallery,
    Music,
}

/// Pictures per gallery page (a 4 x 3 grid) and tracks per music room page.
pub const GALLERY_COLS: usize = 4;
pub const GALLERY_PAGE: usize = 12;
pub const MUSIC_PAGE: usize = 8;

const CELL_W: f32 = 360.0;
const CELL_H: f32 = 203.0;
const CELL_GAP: f32 = 36.0;
const TAB_W: f32 = 260.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Hit {
    Tab(Tab),
    /// An entry on the current page (0-based on that page).
    Item(usize),
    Prev,
    Next,
    Back,
}

/// The tabs there are: only kinds the game has entries for.
pub fn tabs(gallery: usize, music: usize) -> Vec<Tab> {
    let mut out = Vec::new();
    if gallery > 0 {
        out.push(Tab::Gallery);
    }
    if music > 0 {
        out.push(Tab::Music);
    }
    out
}

pub fn per_page(tab: Tab) -> usize {
    match tab {
        Tab::Gallery => GALLERY_PAGE,
        Tab::Music => MUSIC_PAGE,
    }
}

pub fn pages(tab: Tab, count: usize) -> usize {
    count.div_ceil(per_page(tab)).max(1)
}

pub fn tab_rect(i: usize, count: usize) -> Rect {
    let total = count as f32 * TAB_W + (count.max(1) - 1) as f32 * 20.0;
    Rect::new((VIRTUAL.w - total) / 2.0 + i as f32 * (TAB_W + 20.0), 130.0, TAB_W, 56.0)
}

pub fn cell_rect(i: usize) -> Rect {
    let x0 = (VIRTUAL.w - (GALLERY_COLS as f32 * CELL_W + (GALLERY_COLS - 1) as f32 * CELL_GAP)) / 2.0;
    let (col, row) = (i % GALLERY_COLS, i / GALLERY_COLS);
    Rect::new(x0 + col as f32 * (CELL_W + CELL_GAP), 230.0 + row as f32 * (CELL_H + CELL_GAP), CELL_W, CELL_H)
}

pub fn track_rect(i: usize) -> Rect {
    Rect::new((VIRTUAL.w - 1000.0) / 2.0, 230.0 + i as f32 * 84.0, 1000.0, 72.0)
}

pub fn prev_rect() -> Rect {
    Rect::new(40.0, 500.0, 80.0, 120.0)
}

pub fn next_rect() -> Rect {
    Rect::new(VIRTUAL.w - 120.0, 500.0, 80.0, 120.0)
}

fn item_rect(tab: Tab, i: usize) -> Rect {
    match tab {
        Tab::Gallery => cell_rect(i),
        Tab::Music => track_rect(i),
    }
}

/// What is under the cursor. `on_page`: entries on the current page; `pages`: page count.
pub fn hit(x: f32, y: f32, tabs: &[Tab], tab: Tab, on_page: usize, pages: usize) -> Option<Hit> {
    if let Some(i) = (0..tabs.len()).find(|&i| tab_rect(i, tabs.len()).contains(x, y)) {
        return Some(Hit::Tab(tabs[i]));
    }
    if let Some(i) = (0..on_page).find(|&i| item_rect(tab, i).contains(x, y)) {
        return Some(Hit::Item(i));
    }
    if pages > 1 && prev_rect().contains(x, y) {
        return Some(Hit::Prev);
    }
    if pages > 1 && next_rect().contains(x, y) {
        return Some(Hit::Next);
    }
    slots_back_rect().contains(x, y).then_some(Hit::Back)
}

/// A track's display name: its file name without folder or extension, tidied
/// (`assets/rainy_day-2.ogg` -> `Rainy day 2`).
pub fn track_name(src: &str) -> String {
    let file = src.rsplit(['/', '\\']).next().unwrap_or(src);
    let stem = file.rsplit_once('.').map_or(file, |(s, _)| s);
    let spaced: String = stem.chars().map(|c| if c == '_' || c == '-' { ' ' } else { c }).collect();
    let mut chars = spaced.trim().chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => file.to_string(),
    }
}

/// Everything the extras screen shows.
pub struct ExtrasScreen<'a> {
    pub tabs: &'a [Tab],
    pub tab: Tab,
    pub page: usize,
    pub pages: usize,
    /// This page's entries: file, and whether it is unlocked.
    pub entries: &'a [(&'a str, bool)],
    /// The entry the keyboard is on (on this page).
    pub focus: Option<usize>,
    /// The music room track playing (on this page).
    pub playing: Option<usize>,
    pub hover: Option<Hit>,
}

fn text(out: &mut Vec<Draw>, s: impl Into<String>, rect: Rect, size: f32, color: [f32; 4], align: Align) {
    out.push(Draw::Text { text: s.into(), runs: Vec::new(), visible_chars: usize::MAX, rect, size, color, align });
}

fn screen() -> Rect {
    Rect::new(0.0, 0.0, VIRTUAL.w, VIRTUAL.h)
}

pub fn draw_extras(out: &mut Vec<Draw>, theme: &Theme, s: &ExtrasScreen, image_size: &dyn Fn(&str) -> Option<Size>) {
    let lit = |on: bool| if on { theme.button_hover_color } else { theme.button_color };
    out.push(Draw::Rect { rect: screen(), color: theme.overlay_color });
    text(out, "Extras", Rect::new(0.0, 40.0, VIRTUAL.w, 70.0), 48.0, theme.text_color, Align::Center);
    for (i, &t) in s.tabs.iter().enumerate() {
        let r = tab_rect(i, s.tabs.len());
        out.push(Draw::Rect { rect: r, color: lit(t == s.tab || s.hover == Some(Hit::Tab(t))) });
        let label = if t == Tab::Gallery { "Gallery" } else { "Music room" };
        text(out, label, r, 30.0, if t == s.tab { theme.accent_color } else { theme.text_color }, Align::Center);
    }
    for (i, &(src, unlocked)) in s.entries.iter().enumerate() {
        let r = item_rect(s.tab, i);
        let on = s.hover == Some(Hit::Item(i)) || s.focus == Some(i);
        out.push(Draw::Rect { rect: r, color: lit(on) });
        match (s.tab, unlocked) {
            (Tab::Gallery, true) => {
                let inner = Rect::new(r.x + 6.0, r.y + 6.0, r.w - 12.0, r.h - 12.0);
                if let Some(size) = image_size(src) {
                    let p = place(size, inner, Fit::Cover);
                    out.push(Draw::Image { src: src.to_string(), dest: p.dest, uv: p.uv, alpha: 1.0 });
                }
            }
            (Tab::Gallery, false) => text(out, "?", r, 72.0, theme.dim_color, Align::Center),
            (Tab::Music, true) => {
                let mark = if s.playing == Some(i) { "\u{25B6}  " } else { "" };
                let label = Rect::new(r.x + 30.0, r.y, r.w - 60.0, r.h);
                let color = if s.playing == Some(i) { theme.accent_color } else { theme.text_color };
                text(out, format!("{mark}{}", track_name(src)), label, 34.0, color, Align::Left);
            }
            (Tab::Music, false) => {
                text(out, "???", Rect::new(r.x + 30.0, r.y, r.w - 60.0, r.h), 34.0, theme.dim_color, Align::Left);
            }
        }
    }
    if s.pages > 1 {
        for (hit, r, arrow) in [(Hit::Prev, prev_rect(), "\u{2039}"), (Hit::Next, next_rect(), "\u{203A}")] {
            out.push(Draw::Rect { rect: r, color: lit(s.hover == Some(hit)) });
            text(out, arrow, r, 64.0, theme.text_color, Align::Center);
        }
        let label = format!("Page {} of {}", s.page + 1, s.pages);
        text(out, label, Rect::new(0.0, 912.0, VIRTUAL.w, 36.0), 26.0, theme.dim_color, Align::Center);
    }
    let back = slots_back_rect();
    out.push(Draw::Rect { rect: back, color: lit(s.hover == Some(Hit::Back)) });
    text(out, "Back", back, 34.0, theme.text_color, Align::Center);
}

/// One gallery picture filling the screen (letterboxed), over black.
pub fn draw_picture(out: &mut Vec<Draw>, theme: &Theme, src: &str, image_size: &dyn Fn(&str) -> Option<Size>) {
    out.push(Draw::Rect { rect: screen(), color: [0.0, 0.0, 0.0, 1.0] });
    if let Some(size) = image_size(src) {
        let p = place(size, screen(), Fit::Contain);
        out.push(Draw::Image { src: src.to_string(), dest: p.dest, uv: p.uv, alpha: 1.0 });
    }
    let hint = "Click or Esc to go back  \u{00B7}  Left/Right for the others";
    text(out, hint, Rect::new(0.0, VIRTUAL.h - 44.0, VIRTUAL.w, 32.0), 22.0, theme.dim_color, Align::Center);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn track_names_come_from_file_names() {
        assert_eq!(track_name("assets/rainy_day-2.ogg"), "Rainy day 2");
        assert_eq!(track_name("calm.ogg"), "Calm");
        assert_eq!(track_name("music\\Ending Theme.opus"), "Ending Theme");
        assert_eq!(track_name("noext"), "Noext");
    }

    #[test]
    fn only_tabs_with_entries_are_shown() {
        assert_eq!(tabs(3, 0), [Tab::Gallery]);
        assert_eq!(tabs(0, 2), [Tab::Music]);
        assert_eq!(tabs(1, 1), [Tab::Gallery, Tab::Music]);
        assert_eq!(pages(Tab::Gallery, 13), 2);
        assert_eq!(pages(Tab::Music, 0), 1);
    }

    #[test]
    fn the_grid_fits_above_the_back_button_and_is_hit_testable() {
        let last = cell_rect(GALLERY_PAGE - 1);
        assert!(last.y + last.h < slots_back_rect().y - 20.0 && last.x + last.w < VIRTUAL.w);
        assert!(track_rect(MUSIC_PAGE - 1).y + 72.0 < 912.0, "tracks clear the page label");
        let tabs = tabs(1, 1);
        let c = cell_rect(5);
        assert_eq!(hit(c.x + 5.0, c.y + 5.0, &tabs, Tab::Gallery, 12, 1), Some(Hit::Item(5)));
        assert_eq!(hit(c.x + 5.0, c.y + 5.0, &tabs, Tab::Gallery, 3, 1), None, "past the last entry");
        let p = prev_rect();
        assert_eq!(hit(p.x + 1.0, p.y + 1.0, &tabs, Tab::Gallery, 0, 1), None, "no arrows on one page");
        assert_eq!(hit(p.x + 1.0, p.y + 1.0, &tabs, Tab::Gallery, 0, 2), Some(Hit::Prev));
    }
}
