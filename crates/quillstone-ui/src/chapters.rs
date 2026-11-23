//! The main menu's Chapters screen: a grid of tiles, one per `chapters:` entry, each starting a new game at its
//! label once open. Layout, hit-testing and drawing; what the input does is in `game/chapters.rs`.

use quillstone_render::{push_image, Align, Draw, Rect, Size, Theme, VIRTUAL};
use quillstone_script::Fit;

use crate::extras::{draw_pager, pager_hit};
use crate::input::text;
use crate::messages::lit;
use crate::screens::{draw_backdrop, fit_value};
use crate::texts::Tr;

/// Tiles per page: 4 columns, 3 rows.
pub const COLS: usize = 4;
pub const PER_PAGE: usize = 12;

const TILE_W: f32 = 360.0;
const TILE_H: f32 = 170.0;
const TILE_GAP: f32 = 36.0;
/// A tile and the title under it.
const ROW: f32 = TILE_H + 44.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Hit {
    /// A tile on the current page (0-based on that page).
    Item(usize),
    Prev,
    Next,
    Back,
}

pub fn pages(count: usize) -> usize {
    count.div_ceil(PER_PAGE).max(1)
}

pub fn tile_rect(i: usize) -> Rect {
    let x0 = (VIRTUAL.w - (COLS as f32 * TILE_W + (COLS - 1) as f32 * TILE_GAP)) / 2.0;
    let (col, row) = (i % COLS, i / COLS);
    Rect::new(x0 + col as f32 * (TILE_W + TILE_GAP), 160.0 + row as f32 * ROW, TILE_W, TILE_H)
}

/// Where a tile's title goes: right under it.
fn title_rect(i: usize) -> Rect {
    let r = tile_rect(i);
    Rect::new(r.x, r.y + r.h + 4.0, r.w, 34.0)
}

pub fn hit(x: f32, y: f32, on_page: usize, page: usize, pages: usize) -> Option<Hit> {
    if let Some(i) = (0..on_page).find(|&i| tile_rect(i).contains(x, y)) {
        return Some(Hit::Item(i));
    }
    pager_hit(x, y, page, pages, Hit::Prev, Hit::Next, Hit::Back)
}

/// One tile of the page.
pub struct Tile<'a> {
    pub title: &'a str,
    pub open: bool,
    pub thumb: Option<&'a str>,
}

/// Everything the chapters screen shows.
pub struct ChaptersScreen<'a> {
    pub page: usize,
    pub pages: usize,
    /// This page's tiles.
    pub tiles: &'a [Tile<'a>],
    /// The tile the keyboard is on (on this page).
    pub focus: Option<usize>,
    pub hover: Option<Hit>,
}

pub fn draw_chapters(out: &mut Vec<Draw>, theme: &Theme, tr: Tr, s: &ChaptersScreen, image_size: &dyn Fn(&str) -> Option<Size>) {
    draw_backdrop(out, theme, &theme.chapters_background, image_size);
    text(out, tr.t("Chapters"), Rect::new(0.0, 40.0, VIRTUAL.w, 70.0), 48.0, theme.text_color, Align::Center);
    for (i, tile) in s.tiles.iter().enumerate() {
        let r = tile_rect(i);
        let on = s.hover == Some(Hit::Item(i)) || s.focus == Some(i);
        out.push(Draw::Rect { rect: r, color: lit(theme, on) });
        if !tile.open {
            text(out, "?", r, 72.0, theme.dim_color, Align::Center);
            text(out, "???", title_rect(i), 26.0, theme.dim_color, Align::Center);
            continue;
        }
        let inner = Rect::new(r.x + 6.0, r.y + 6.0, r.w - 12.0, r.h - 12.0);
        // No picture: a dark tile with the title on it.
        if !tile.thumb.is_some_and(|pic| push_image(out, pic, inner, Fit::Cover, 1.0, image_size)) {
            out.push(Draw::Rect { rect: inner, color: [0.04, 0.04, 0.06, 1.0] });
            text(out, tile.title, Rect::new(inner.x + 16.0, inner.y, inner.w - 32.0, inner.h), 30.0, theme.accent_color, Align::Center);
        }
        let t = title_rect(i);
        let (title, size) = fit_value(tile.title, 26.0, t.w, None);
        text(out, title, t, size, theme.text_color, Align::Center);
    }
    draw_pager(out, theme, tr, s.page, s.pages, 890.0, [s.hover == Some(Hit::Prev), s.hover == Some(Hit::Next), s.hover == Some(Hit::Back)]);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::extras::{next_rect, prev_rect};

    #[test]
    fn the_chapters_background_picture_replaces_the_overlay_rect_when_known() {
        let t = Theme::parse("chapters_background = bg.png\n").0;
        let screen = ChaptersScreen { page: 0, pages: 1, tiles: &[], focus: None, hover: None };
        let draw = |size: &dyn Fn(&str) -> Option<Size>| {
            let mut out = Vec::new();
            draw_chapters(&mut out, &t, Tr::default(), &screen, size);
            out
        };
        let (with, without) = (draw(&|_| Some(Size::new(1920.0, 1080.0))), draw(&|_| None));
        assert!(matches!(&with[0], Draw::Image { src, .. } if src == "bg.png"));
        assert!(!with.iter().any(|d| matches!(d, Draw::Rect { color, .. } if *color == t.overlay_color)));
        assert!(matches!(&without[0], Draw::Rect { color, .. } if *color == t.overlay_color));
    }

    #[test]
    fn pages_hold_twelve_tiles_and_the_grid_clears_the_page_label_and_back() {
        assert_eq!((pages(0), pages(12), pages(13), pages(16)), (1, 1, 2, 2));
        let last = title_rect(PER_PAGE - 1);
        assert!(last.y + last.h < 890.0 && tile_rect(COLS - 1).x + TILE_W < VIRTUAL.w);
        let c = tile_rect(5);
        assert_eq!(hit(c.x + 3.0, c.y + 3.0, 12, 0, 1), Some(Hit::Item(5)));
        assert_eq!(hit(c.x + 3.0, c.y + 3.0, 3, 0, 1), None, "past the last tile");
        assert_eq!(hit(prev_rect().x + 1.0, prev_rect().y + 1.0, 12, 1, 2), Some(Hit::Prev));
        assert_eq!(hit(next_rect().x + 1.0, next_rect().y + 1.0, 12, 1, 2), None, "no page after the last");
    }
}
