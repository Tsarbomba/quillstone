//! The inventory and shop screens: one grid of items, laid out and styled by the
//! theme (`inventory_*` keys). Layout, hit-testing and drawing only; what the
//! input does is in `game/bag.rs`.
//!
//! The panel (`inventory_panel_rect`, 1500x900 by default) holds a title, in a
//! shop the player's money, a grid of cells (icon, name, price), the description
//! of the hovered or selected item, a page control and a Close/Leave button.

use quillstone_render::{place, Align, CellStyle, Draw, Rect, Size, Theme, VIRTUAL};
use quillstone_script::Fit;

const MARGIN: f32 = 40.0;
const GRID_TOP: f32 = 150.0;
const DESC_H: f32 = 170.0;
const BOTTOM_ROW_H: f32 = 56.0;
const CLOSE_W: f32 = 220.0;
const PAGER_W: f32 = 70.0;

/// How a cell is shown (and whether a click on it does anything in a shop).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CellState {
    Normal,
    /// Already owned: marked, not for sale again.
    Owned,
    /// Too dear: greyed.
    Disabled,
}

/// One cell of the grid.
#[derive(Debug, Clone, PartialEq)]
pub struct BagCell {
    pub name: String,
    pub icon: Option<String>,
    /// Shown under the name (a shop's price).
    pub price: Option<String>,
    pub state: CellState,
}

/// The description box: the hovered or selected item.
#[derive(Debug, Clone, PartialEq)]
pub struct Detail {
    pub name: String,
    pub text: String,
    pub price: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Hit {
    /// A cell on the current page (0-based on that page).
    Cell(usize),
    Prev,
    Next,
    Close,
}

/// Everything the screen shows.
pub struct BagScreen<'a> {
    pub title: &'a str,
    pub background: Option<&'a str>,
    /// "Gold: 40", in a shop.
    pub money: Option<&'a str>,
    /// This page's cells.
    pub cells: &'a [BagCell],
    pub page: usize,
    pub pages: usize,
    /// The cell the keyboard is on (on this page).
    pub focus: Option<usize>,
    pub hover: Option<Hit>,
    pub detail: Option<&'a Detail>,
    pub close_label: &'a str,
    /// Shown instead of the grid when there are no cells.
    pub empty: &'a str,
}

fn text(out: &mut Vec<Draw>, s: impl Into<String>, rect: Rect, size: f32, color: [f32; 4], align: Align) {
    out.push(Draw::Text { text: s.into(), runs: Vec::new(), visible_chars: usize::MAX, rect, size, color, align, outline: None });
}

fn screen() -> Rect {
    Rect::new(0.0, 0.0, VIRTUAL.w, VIRTUAL.h)
}

/// Move `r` (without resizing) so it lies inside the screen.
fn inside(r: Rect) -> Rect {
    let (w, h) = (r.w.min(VIRTUAL.w), r.h.min(VIRTUAL.h));
    Rect::new(r.x.clamp(0.0, VIRTUAL.w - w), r.y.clamp(0.0, VIRTUAL.h - h), w, h)
}

pub fn panel_rect(theme: &Theme) -> Rect {
    let [x, y, w, h] = theme.inventory_panel_rect;
    inside(Rect::new(x, y, w, h))
}

fn bottom_row_y(theme: &Theme) -> f32 {
    let p = panel_rect(theme);
    p.y + p.h - 76.0
}

pub fn description_rect(theme: &Theme) -> Rect {
    match theme.inventory_description_rect {
        Some([x, y, w, h]) => inside(Rect::new(x, y, w, h)),
        None => {
            let p = panel_rect(theme);
            inside(Rect::new(p.x + MARGIN, bottom_row_y(theme) - 20.0 - DESC_H, p.w - 2.0 * MARGIN, DESC_H))
        }
    }
}

fn cell_size(theme: &Theme) -> (f32, f32) {
    let [w, h] = theme.inventory_cell_size;
    (w.min(VIRTUAL.w), h.min(VIRTUAL.h))
}

/// Columns that fit the panel, at most the theme's.
pub fn columns(theme: &Theme) -> usize {
    let (w, _) = cell_size(theme);
    let gap = theme.inventory_cell_gap;
    let room = panel_rect(theme).w - 2.0 * MARGIN;
    let fit = (((room + gap) / (w + gap)).floor() as usize).max(1);
    theme.inventory_columns.clamp(1, fit)
}

/// Rows that fit above the description box (or the buttons, when the theme moved it).
pub fn rows(theme: &Theme) -> usize {
    let (_, h) = cell_size(theme);
    let gap = theme.inventory_cell_gap;
    let p = panel_rect(theme);
    let bottom = if theme.inventory_description_rect.is_some() { bottom_row_y(theme) - 20.0 } else { description_rect(theme).y - 16.0 };
    let room = bottom - (p.y + GRID_TOP);
    (((room + gap) / (h + gap)).floor() as usize).max(1)
}

pub fn per_page(theme: &Theme) -> usize {
    columns(theme) * rows(theme)
}

pub fn pages(theme: &Theme, count: usize) -> usize {
    count.div_ceil(per_page(theme)).max(1)
}

/// The `i`th cell of a page.
pub fn cell_rect(theme: &Theme, i: usize) -> Rect {
    let (cols, gap) = (columns(theme), theme.inventory_cell_gap);
    let (w, h) = cell_size(theme);
    let p = panel_rect(theme);
    let grid_w = cols as f32 * w + (cols - 1) as f32 * gap;
    let (col, row) = (i % cols, i / cols);
    inside(Rect::new(
        p.x + (p.w - grid_w) / 2.0 + col as f32 * (w + gap),
        p.y + GRID_TOP + row as f32 * (h + gap),
        w,
        h,
    ))
}

pub fn close_rect(theme: &Theme) -> Rect {
    let p = panel_rect(theme);
    inside(Rect::new(p.x + p.w - MARGIN - CLOSE_W, bottom_row_y(theme), CLOSE_W, BOTTOM_ROW_H))
}

pub fn prev_rect(theme: &Theme) -> Rect {
    inside(Rect::new(panel_rect(theme).x + MARGIN, bottom_row_y(theme), PAGER_W, BOTTOM_ROW_H))
}

pub fn next_rect(theme: &Theme) -> Rect {
    inside(Rect::new(panel_rect(theme).x + MARGIN + PAGER_W + 220.0, bottom_row_y(theme), PAGER_W, BOTTOM_ROW_H))
}

fn page_label_rect(theme: &Theme) -> Rect {
    inside(Rect::new(panel_rect(theme).x + MARGIN + PAGER_W, bottom_row_y(theme), 220.0, BOTTOM_ROW_H))
}

fn title_rect(theme: &Theme) -> Rect {
    let p = panel_rect(theme);
    inside(Rect::new(p.x, p.y + 24.0, p.w, 64.0))
}

fn money_rect(theme: &Theme) -> Rect {
    let p = panel_rect(theme);
    inside(Rect::new(p.x + MARGIN, p.y + 98.0, p.w - 2.0 * MARGIN, 40.0))
}

/// What is under the cursor. `on_page`: cells on this page; `pages`: page count.
pub fn hit(theme: &Theme, x: f32, y: f32, on_page: usize, pages: usize) -> Option<Hit> {
    if close_rect(theme).contains(x, y) {
        return Some(Hit::Close);
    }
    if pages > 1 && prev_rect(theme).contains(x, y) {
        return Some(Hit::Prev);
    }
    if pages > 1 && next_rect(theme).contains(x, y) {
        return Some(Hit::Next);
    }
    (0..on_page).find(|&i| cell_rect(theme, i).contains(x, y)).map(Hit::Cell)
}

/// A one-line text size that keeps `s` from wrapping in `width`.
fn fit_size(s: &str, base: f32, width: f32) -> f32 {
    let needed = s.chars().count() as f32 * 0.55 * base;
    if needed <= width {
        base
    } else {
        (base * width / needed).max(18.0)
    }
}

/// Fill `rect` with a cell style, or with `fallback` when its picture isn't loaded.
fn fill(out: &mut Vec<Draw>, style: Option<&CellStyle>, fallback: [f32; 4], rect: Rect, image_size: &dyn Fn(&str) -> Option<Size>) {
    match style {
        Some(CellStyle::Color(c)) => out.push(Draw::Rect { rect, color: *c }),
        Some(CellStyle::Image(src)) => match image_size(src) {
            Some(size) => {
                let p = place(size, rect, Fit::Stretch);
                out.push(Draw::Image { src: src.clone(), dest: p.dest, uv: p.uv, alpha: 1.0 });
            }
            None => out.push(Draw::Rect { rect, color: fallback }),
        },
        None => out.push(Draw::Rect { rect, color: fallback }),
    }
}

pub fn draw_bag(out: &mut Vec<Draw>, theme: &Theme, s: &BagScreen, image_size: &dyn Fn(&str) -> Option<Size>) {
    let lit = |on: bool| if on { theme.button_hover_color } else { theme.button_color };
    let name_color = theme.inventory_name_color.unwrap_or(theme.text_color);
    let price_color = theme.inventory_price_color.unwrap_or(theme.accent_color);
    let text_color = theme.inventory_text_color.unwrap_or(theme.text_color);

    // Behind everything: the theme's picture, or the usual dimming of the story.
    match s.background.and_then(|src| Some((src, image_size(src)?))) {
        Some((src, size)) => {
            let p = place(size, screen(), Fit::Stretch);
            out.push(Draw::Image { src: src.to_string(), dest: p.dest, uv: p.uv, alpha: 1.0 });
        }
        None => out.push(Draw::Rect { rect: screen(), color: theme.overlay_color }),
    }
    if let Some((src, size)) = theme.inventory_panel.as_deref().and_then(|src| Some((src, image_size(src)?))) {
        let p = place(size, panel_rect(theme), Fit::Stretch);
        out.push(Draw::Image { src: src.to_string(), dest: p.dest, uv: p.uv, alpha: 1.0 });
    }
    text(out, s.title, title_rect(theme), 48.0, theme.text_color, Align::Center);
    if let Some(money) = s.money {
        text(out, money, money_rect(theme), 30.0, price_color, Align::Center);
    }

    if s.cells.is_empty() {
        let r = cell_rect(theme, 0);
        let area = Rect::new(panel_rect(theme).x, r.y, panel_rect(theme).w, r.h);
        text(out, s.empty, area, 32.0, theme.dim_color, Align::Center);
    }
    for (i, cell) in s.cells.iter().enumerate() {
        let r = cell_rect(theme, i);
        let on = s.hover == Some(Hit::Cell(i)) || s.focus == Some(i);
        let dim = theme.dim_color;
        let [br, bg, bb, ba] = theme.button_color;
        // Under the pointer a cell takes the theme's hover look; a theme that styles its cells
        // but gives no hover look gets a light tint over its own.
        let mut tint = false;
        let (style, fallback) = match cell.state {
            CellState::Normal if on && theme.inventory_cell_hover.is_some() => {
                (theme.inventory_cell_hover.as_ref(), theme.button_hover_color)
            }
            CellState::Normal if on && theme.inventory_cell.is_some() => {
                tint = true;
                (theme.inventory_cell.as_ref(), theme.button_color)
            }
            CellState::Normal if on => (None, theme.button_hover_color),
            CellState::Normal => (theme.inventory_cell.as_ref(), theme.button_color),
            CellState::Owned => (theme.inventory_cell_owned.as_ref().or(theme.inventory_cell.as_ref()), theme.button_color),
            CellState::Disabled => (theme.inventory_cell_disabled.as_ref(), [br, bg, bb, ba * 0.45]),
        };
        fill(out, style, fallback, r, image_size);
        if tint {
            out.push(Draw::Rect { rect: r, color: [1.0, 1.0, 1.0, 0.14] });
        }

        let icon = theme.inventory_icon_size.min(r.w - 16.0).min((r.h - 100.0).max(16.0));
        let icon_rect = Rect::new(r.x + (r.w - icon) / 2.0, r.y + 12.0, icon, icon);
        if let Some(src) = &cell.icon {
            if let Some(size) = image_size(src) {
                let p = place(size, icon_rect, Fit::Contain);
                let alpha = if cell.state == CellState::Disabled { 0.45 } else { 1.0 };
                out.push(Draw::Image { src: src.clone(), dest: p.dest, uv: p.uv, alpha });
            }
        }
        let name_rect = Rect::new(r.x + 8.0, icon_rect.y + icon + 8.0, r.w - 16.0, 36.0);
        let name_col = if cell.state == CellState::Disabled { dim } else { name_color };
        let size = fit_size(&cell.name, 28.0, name_rect.w);
        text(out, crate::screens::one_row(&cell.name, size, name_rect.w), name_rect, size, name_col, Align::Center);
        let line = Rect::new(r.x + 8.0, name_rect.y + 38.0, r.w - 16.0, 32.0);
        match (cell.state, &cell.price) {
            (CellState::Owned, _) => text(out, "Owned", line, 26.0, theme.accent_color, Align::Center),
            (CellState::Disabled, Some(p)) => text(out, p.clone(), line, 26.0, dim, Align::Center),
            (_, Some(p)) => text(out, p.clone(), line, fit_size(p, 26.0, line.w), price_color, Align::Center),
            _ => {}
        }
    }

    let d = description_rect(theme);
    out.push(Draw::Rect { rect: d, color: theme.panel_color });
    if let Some(detail) = s.detail {
        let inner = Rect::new(d.x + 24.0, d.y + 10.0, d.w - 48.0, d.h - 20.0);
        text(out, detail.name.clone(), Rect::new(inner.x, inner.y, inner.w, 40.0), 32.0, name_color, Align::Left);
        let below = if detail.price.is_some() { 38.0 } else { 0.0 };
        let body = Rect::new(inner.x, inner.y + 46.0, inner.w, (inner.h - 46.0 - below).max(20.0));
        text(out, detail.text.clone(), body, 26.0, text_color, Align::Left);
        if let Some(price) = &detail.price {
            let row = Rect::new(inner.x, inner.y + inner.h - 34.0, inner.w, 34.0);
            text(out, price.clone(), row, 26.0, price_color, Align::Left);
        }
    }

    if s.pages > 1 {
        for (h, r, arrow) in [(Hit::Prev, prev_rect(theme), "\u{2039}"), (Hit::Next, next_rect(theme), "\u{203A}")] {
            out.push(Draw::Rect { rect: r, color: lit(s.hover == Some(h)) });
            text(out, arrow, r, 44.0, theme.text_color, Align::Center);
        }
        let label = format!("Page {} of {}", s.page + 1, s.pages);
        text(out, label, page_label_rect(theme), 26.0, theme.dim_color, Align::Center);
    }
    let close = close_rect(theme);
    out.push(Draw::Rect { rect: close, color: lit(s.hover == Some(Hit::Close)) });
    text(out, s.close_label, close, 34.0, theme.text_color, Align::Center);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cells(n: usize) -> Vec<BagCell> {
        (0..n)
            .map(|i| BagCell { name: format!("Item {i}"), icon: Some("i.png".into()), price: None, state: CellState::Normal })
            .collect()
    }

    fn inside_screen(r: Rect) -> bool {
        r.x >= 0.0 && r.y >= 0.0 && r.x + r.w <= VIRTUAL.w + 1e-3 && r.y + r.h <= VIRTUAL.h + 1e-3
    }

    fn themes() -> Vec<Theme> {
        let parse = |s: &str| Theme::parse(s).0;
        vec![
            Theme::default(),
            parse("inventory_columns = 12\ninventory_cell_size = 600\ninventory_cell_gap = 100"),
            parse("inventory_panel_rect = 0,0,1920,1080\ninventory_columns = 8\ninventory_cell_size = 150,120\ninventory_cell_gap = 6"),
            parse("inventory_panel_rect = 1500,900,420,180\ninventory_cell_size = 300"),
            parse("inventory_description_rect = 100,900,900,180\ninventory_columns = 3\ninventory_cell_size = 100,120"),
            parse("inventory_cell_size = 48,120\ninventory_cell_gap = 0\ninventory_columns = 12"),
        ]
    }

    #[test]
    fn the_default_grid_is_five_by_two_and_pages_follow() {
        let t = Theme::default();
        assert_eq!((columns(&t), rows(&t), per_page(&t)), (5, 2, 10));
        assert_eq!([pages(&t, 0), pages(&t, 10), pages(&t, 11), pages(&t, 25)], [1, 1, 2, 3]);
    }

    #[test]
    fn every_rect_lies_inside_the_screen_for_any_theme_and_item_count() {
        for t in themes() {
            let per = per_page(&t);
            assert!(per >= 1);
            for count in [0, 1, per - 1, per, per + 1, 3 * per + 2] {
                let on_page = count.min(per);
                for page in 0..pages(&t, count) {
                    let shown = cells(count.saturating_sub(page * per).min(per));
                    let screen = BagScreen {
                        title: "T",
                        background: None,
                        money: Some("Gold: 1"),
                        cells: &shown,
                        page,
                        pages: pages(&t, count),
                        focus: None,
                        hover: None,
                        detail: Some(&Detail { name: "n".into(), text: "t".into(), price: Some("p".into()) }),
                        close_label: "Close",
                        empty: "Nothing",
                    };
                    let mut out = Vec::new();
                    draw_bag(&mut out, &t, &screen, &|_| Some(Size::new(64.0, 64.0)));
                    for d in &out {
                        match d {
                            Draw::Rect { rect, .. } | Draw::Text { rect, .. } => assert!(inside_screen(*rect), "{rect:?} for {t:?}"),
                            Draw::Image { dest, .. } => assert!(inside_screen(*dest), "{dest:?}"),
                        }
                    }
                }
                for i in 0..on_page {
                    assert!(inside_screen(cell_rect(&t, i)));
                }
            }
            for r in [close_rect(&t), prev_rect(&t), next_rect(&t), description_rect(&t), panel_rect(&t)] {
                assert!(inside_screen(r), "{r:?}");
            }
        }
    }

    #[test]
    fn name_and_price_stay_inside_the_smallest_cell() {
        let t = Theme::parse(&format!(
            "inventory_cell_size = 48,{}\ninventory_cell_gap = 0\ninventory_columns = 12",
            quillstone_render::theme::MIN_CELL_HEIGHT
        ))
        .0;
        assert_eq!(t.inventory_cell_size[1], quillstone_render::theme::MIN_CELL_HEIGHT);
        let shown = vec![BagCell { name: "Key".into(), icon: Some("i.png".into()), price: Some("5 gold".into()), state: CellState::Normal }];
        let screen = BagScreen {
            title: "T", background: None, money: None, cells: &shown, page: 0, pages: 1, focus: None, hover: None,
            detail: None, close_label: "Close", empty: "Nothing",
        };
        let mut out = Vec::new();
        draw_bag(&mut out, &t, &screen, &|_| Some(Size::new(64.0, 64.0)));
        let cell = cell_rect(&t, 0);
        let within = |r: Rect| r.x >= cell.x - 1e-3 && r.y >= cell.y - 1e-3 && r.x + r.w <= cell.x + cell.w + 1e-3 && r.y + r.h <= cell.y + cell.h + 1e-3;
        let mut seen = 0;
        for d in &out {
            match d {
                Draw::Text { text, rect, .. } if text == "Key" || text == "5 gold" => {
                    assert!(within(*rect), "{text}: {rect:?} outside {cell:?}");
                    seen += 1;
                }
                Draw::Image { dest, .. } => {
                    assert!(within(*dest), "icon {dest:?} outside {cell:?}");
                    seen += 1;
                }
                _ => {}
            }
        }
        assert_eq!(seen, 3, "icon, name and price were drawn");
    }

    #[test]
    fn cells_do_not_overlap_each_other_or_the_description_box() {
        let t = Theme::default();
        let per = per_page(&t);
        let d = description_rect(&t);
        for i in 0..per {
            let a = cell_rect(&t, i);
            assert!(a.y + a.h <= d.y, "cell {i} reaches into the description");
            for j in i + 1..per {
                let b = cell_rect(&t, j);
                let apart = a.x + a.w <= b.x || b.x + b.w <= a.x || a.y + a.h <= b.y || b.y + b.h <= a.y;
                assert!(apart, "{i} and {j} overlap");
            }
        }
        let close = close_rect(&t);
        assert!(d.y + d.h <= close.y, "the buttons sit under the description");
    }

    #[test]
    fn hits_find_cells_pager_and_close() {
        let t = Theme::default();
        let c = cell_rect(&t, 3);
        assert_eq!(hit(&t, c.x + 5.0, c.y + 5.0, 10, 1), Some(Hit::Cell(3)));
        assert_eq!(hit(&t, c.x + 5.0, c.y + 5.0, 3, 1), None, "past the last cell");
        let n = next_rect(&t);
        assert_eq!(hit(&t, n.x + 1.0, n.y + 1.0, 10, 1), None, "no pager on one page");
        assert_eq!(hit(&t, n.x + 1.0, n.y + 1.0, 10, 2), Some(Hit::Next));
        let p = prev_rect(&t);
        assert_eq!(hit(&t, p.x + 1.0, p.y + 1.0, 10, 2), Some(Hit::Prev));
        let x = close_rect(&t);
        assert_eq!(hit(&t, x.x + 1.0, x.y + 1.0, 10, 2), Some(Hit::Close));
    }

    #[test]
    fn a_theme_moves_and_resizes_the_grid() {
        let (t, _) = Theme::parse("inventory_panel_rect = 100,100,1000,800\ninventory_columns = 3\ninventory_cell_size = 200,130");
        assert_eq!(columns(&t), 3);
        let a = cell_rect(&t, 0);
        assert_eq!((a.w, a.h), (200.0, 130.0));
        assert!(a.x >= 100.0 && a.x + 3.0 * 200.0 <= 1100.0);
        // more columns than fit are cut down
        let (t, _) = Theme::parse("inventory_panel_rect = 100,100,600,800\ninventory_columns = 6\ninventory_cell_size = 200");
        assert_eq!(columns(&t), 2);
    }

    #[test]
    fn the_screen_draws_background_panel_cells_and_states() {
        let (t, _) = Theme::parse("inventory_panel = p.png\ninventory_cell = #112233\ninventory_cell_owned = o.png");
        let shown = vec![
            BagCell { name: "A".into(), icon: Some("a.png".into()), price: Some("5 gold".into()), state: CellState::Normal },
            BagCell { name: "B".into(), icon: None, price: None, state: CellState::Owned },
            BagCell { name: "C".into(), icon: Some("c.png".into()), price: Some("50 gold".into()), state: CellState::Disabled },
        ];
        let screen = BagScreen {
            title: "Shop",
            background: Some("bg.png"),
            money: Some("Gold: 10"),
            cells: &shown,
            page: 0,
            pages: 1,
            focus: None,
            hover: Some(Hit::Cell(0)),
            detail: None,
            close_label: "Leave",
            empty: "",
        };
        let mut out = Vec::new();
        draw_bag(&mut out, &t, &screen, &|_| Some(Size::new(10.0, 10.0)));
        let images: Vec<&str> = out.iter().filter_map(|d| if let Draw::Image { src, .. } = d { Some(src.as_str()) } else { None }).collect();
        assert_eq!(&images[..2], ["bg.png", "p.png"], "background, then the panel");
        assert!(images.contains(&"o.png") && images.contains(&"a.png") && images.contains(&"c.png"));
        let texts: Vec<&str> = out.iter().filter_map(|d| if let Draw::Text { text, .. } = d { Some(text.as_str()) } else { None }).collect();
        for want in ["Shop", "Gold: 10", "A", "5 gold", "Owned", "50 gold", "Leave"] {
            assert!(texts.contains(&want), "{want} in {texts:?}");
        }
        assert!(out.iter().any(|d| matches!(d, Draw::Rect { color, .. } if *color == [17.0 / 255.0, 34.0 / 255.0, 51.0 / 255.0, 1.0])));
        // a picture that isn't loaded yet falls back to the plain look
        let mut out = Vec::new();
        draw_bag(&mut out, &t, &screen, &|_| None);
        assert!(out.iter().all(|d| !matches!(d, Draw::Image { .. })));
        assert!(matches!(&out[0], Draw::Rect { color, .. } if *color == t.overlay_color));
    }

    #[test]
    fn long_names_shrink_instead_of_wrapping() {
        assert_eq!(fit_size("Key", 28.0, 224.0), 28.0);
        let s = fit_size("The extremely long name of a thing", 28.0, 224.0);
        assert!((18.0..28.0).contains(&s));
    }

    #[test]
    fn a_name_too_long_even_when_small_is_cut_with_an_ellipsis_not_clipped() {
        let t = Theme::default();
        let name = "An extraordinarily long item name that keeps going and going past any sensible cell width";
        let shown = vec![BagCell { name: name.into(), icon: None, price: None, state: CellState::Normal }];
        let mut out = Vec::new();
        let screen = BagScreen {
            title: "T", background: None, money: None, cells: &shown, page: 0, pages: 1, focus: None, hover: None,
            detail: None, close_label: "Close", empty: "Nothing",
        };
        draw_bag(&mut out, &t, &screen, &|_| None);
        let (text, rect, size) = out
            .iter()
            .find_map(|d| match d {
                Draw::Text { text, rect, size, .. } if text.starts_with("An extra") => Some((text.clone(), *rect, *size)),
                _ => None,
            })
            .expect("the name is drawn");
        assert!(text.ends_with('…') && text.len() < name.len());
        assert_eq!(quillstone_render::estimate_lines(&text, size, rect.w), 1);
    }
}
