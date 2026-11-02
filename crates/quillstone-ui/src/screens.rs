//! Layout, hit-testing and drawing for the quick menu, settings and backlog.
//!
//! Pure functions over virtual coordinates (1920x1080), like the view's own
//! layout; [`crate::Game`] holds the state and decides which screen is up.

use quillstone_render::theme::{QuickAlign, QuickIcon, DEFAULT_OPTIONS_TEXT_SIZE, QUICK_ICON_SIDE};
use quillstone_render::{estimate_lines, line_height, Align, Draw, Rect, Size, Theme, FULL_UV, VIRTUAL};
use quillstone_runtime::Logged;
use quillstone_script::markup::plain;
use quillstone_script::{KeyPress, TimingBar};

use crate::input::{picture_or_rect, rich_text, screen, text};
use crate::settings::*;
use crate::slots::{thumbnail, Page, SlotInfo};

const FOCUS: [f32; 4] = [1.0, 1.0, 1.0, 0.06];
const TRACK: [f32; 4] = [0.35, 0.35, 0.42, 1.0];

// ---- quick menu (under the textbox) -------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QuickButton {
    Back,
    History,
    Skip,
    Auto,
    Save,
    Load,
    QuickSave,
    QuickLoad,
    Settings,
    /// The inventory; only offered by games that have one, left of Back.
    Items,
    /// The journal; only offered by games that have one, left of Items.
    Journal,
}

const QUICK_W: f32 = 150.0;
const QUICK_GAP: f32 = 8.0;
const QUICK_H: f32 = 32.0;
const QUICK_TEXT: f32 = 24.0;
/// Space between the textbox and the row below it.
const QUICK_MARGIN: f32 = 4.0;
/// Where a row `h` tall has its top edge: directly below the textbox, or, when there is no
/// room for it below, just inside the textbox's bottom edge.
fn quick_y_for(tb: Rect, h: f32) -> f32 {
    let below = tb.y + tb.h + QUICK_MARGIN;
    if below + h <= VIRTUAL.h {
        below
    } else {
        (tb.y + tb.h - QUICK_MARGIN - h).max(0.0)
    }
}

/// The left edge of a row `total` wide, aligned to the textbox; right keeps a gap from the screen's left
/// edge when the textbox is too narrow for the row. Always kept on the screen.
fn quick_left_for(tb: Rect, total: f32, align: QuickAlign) -> f32 {
    let left = match align {
        QuickAlign::Left => tb.x,
        QuickAlign::Center => tb.x + (tb.w - total) / 2.0,
        QuickAlign::Right => (tb.x + tb.w - total).max(QUICK_GAP),
    };
    left.min(VIRTUAL.w - total).max(0.0)
}

/// The size a button's text label is drawn at: [`QUICK_TEXT`], smaller (not below 10) when the label
/// would be wider than the button, with a little room on both sides. Same estimate as the other one-line labels.
fn quick_text_size(label: &str, width: f32) -> f32 {
    let needed = label.chars().count() as f32 * 0.55 * QUICK_TEXT;
    (QUICK_TEXT * (width - 8.0) / needed).clamp(10.0, QUICK_TEXT)
}

/// Every button, in the order of the theme's ids ([`quillstone_render::theme::QUICK_IDS`]).
const QUICK_ALL: [QuickButton; 11] = [
    QuickButton::Back,
    QuickButton::History,
    QuickButton::Skip,
    QuickButton::Auto,
    QuickButton::Save,
    QuickButton::Load,
    QuickButton::QuickSave,
    QuickButton::QuickLoad,
    QuickButton::Settings,
    QuickButton::Items,
    QuickButton::Journal,
];

impl QuickButton {
    /// The id in the theme's `quick_<id>_...` keys.
    pub fn id(self) -> &'static str {
        quillstone_render::theme::QUICK_IDS[quick_index(self)]
    }
}

fn quick_index(b: QuickButton) -> usize {
    QUICK_ALL.iter().position(|q| *q == b).unwrap_or(0)
}

/// How much of an icon without a `_hover` picture shows while the cursor is not on it.
const ICON_IDLE_ALPHA: f32 = 0.8;
/// An icon of a button that does nothing right now (Back at the start, Q.Load without a quick save).
const ICON_DISABLED_ALPHA: f32 = 0.4;

/// A button's picture, if the theme has one and it is loaded: its file and own size.
fn quick_picture<'a>(spec: &'a QuickIcon, size_of: &dyn Fn(&str) -> Option<Size>) -> Option<(&'a str, Size)> {
    let src = spec.icon.as_deref()?;
    size_of(src).map(|size| (src, size))
}

fn loaded_picture<'a>(p: &'a Option<String>, size_of: &dyn Fn(&str) -> Option<Size>) -> Option<&'a str> {
    p.as_deref().filter(|s| size_of(s).is_some())
}

/// A button's width and height: the theme's `size`, else its picture's own (kept within the
/// allowed sides), else a text button's.
pub fn quick_button_size(spec: &QuickIcon, size_of: &dyn Fn(&str) -> Option<Size>) -> (f32, f32) {
    let (lo, hi) = QUICK_ICON_SIDE;
    match (spec.size, quick_picture(spec, size_of)) {
        (Some([w, h]), _) => (w, h),
        (None, Some((_, s))) => (s.w.clamp(lo, hi), s.h.clamp(lo, hi)),
        (None, None) => (QUICK_W, QUICK_H),
    }
}

/// Where the quick menu's buttons are, from the theme.
#[derive(Debug, Clone, PartialEq)]
pub struct QuickLayout {
    /// In drawing order: where two overlap the later one is on top and gets the click. Items and Journal are
    /// there only in a game with an inventory or a journal.
    pub buttons: Vec<(QuickButton, Rect)>,
    /// Where else than over the textbox the cursor makes the `hover` mode show the menu: the row's
    /// strip and every button the theme placed.
    pub zone: Vec<Rect>,
}

impl QuickLayout {
    /// The button at a point (the topmost one).
    pub fn at(&self, x: f32, y: f32) -> Option<QuickButton> {
        self.buttons.iter().rev().find(|(_, r)| r.contains(x, y)).map(|(b, _)| *b)
    }

    pub fn rect(&self, b: QuickButton) -> Option<Rect> {
        self.buttons.iter().find(|(q, _)| *q == b).map(|(_, r)| *r)
    }

    /// Whether a point is where the quick menu shows in its `hover` mode: over the textbox `tb`,
    /// in the strip of the row (the gap between them included), or on a placed button.
    pub fn in_zone(&self, tb: Rect, x: f32, y: f32) -> bool {
        tb.contains(x, y) || self.zone.iter().any(|r| r.contains(x, y))
    }
}

/// Lay the buttons out. The ones without a `quick_<id>_pos` keep the row below the textbox
/// (placed by `quick_align`, Journal and Items leftmost); the others sit at their position, kept on the screen.
/// `size_of` gives a loaded picture's size: a button whose picture isn't loaded is a text button.
pub fn quick_layout(tb: Rect, theme: &Theme, inventory: bool, journal: bool, size_of: &dyn Fn(&str) -> Option<Size>) -> QuickLayout {
    let specs = &theme.quick_icons;
    let dims: Vec<(f32, f32)> = specs.iter().map(|q| quick_button_size(q, size_of)).collect();
    // `quick_buttons = listed` drops the built-in buttons that the theme doesn't give a picture or a place.
    let listed = |k: usize| !theme.quick_listed || specs[k].icon.is_some() || specs[k].pos.is_some();
    let flow: Vec<usize> =
        [10, 9].into_iter().chain(0..9).filter(|&k| specs[k].pos.is_none() && listed(k) && (k != 9 || inventory) && (k != 10 || journal))
        .collect();
    let total = flow.iter().map(|&k| dims[k].0).sum::<f32>() + flow.len().saturating_sub(1) as f32 * QUICK_GAP;
    let row_h = flow.iter().map(|&k| dims[k].1).fold(0.0, f32::max);
    let (left, y) = (quick_left_for(tb, total, theme.quick_align), quick_y_for(tb, row_h));
    let mut rects = [None; 11];
    let mut x = left;
    for &k in &flow {
        let (w, h) = dims[k];
        rects[k] = Some(Rect::new(x, y + (row_h - h) / 2.0, w, h));
        x += w + QUICK_GAP;
    }
    for (k, spec) in specs.iter().enumerate() {
        if let Some([px, py]) = spec.pos {
            let (w, h) = dims[k];
            rects[k] = Some(Rect::new(px.min(VIRTUAL.w - w).max(0.0), py.min(VIRTUAL.h - h).max(0.0), w, h));
        }
    }
    let mut zone = Vec::new();
    if !flow.is_empty() {
        let top = y.min(tb.y + tb.h);
        zone.push(Rect::new(left, top, total, y + row_h - top));
    }
    let shown = |k: usize| (k != 9 || inventory) && (k != 10 || journal) && listed(k);
    zone.extend((0..11).filter(|&k| shown(k) && specs[k].pos.is_some()).filter_map(|k| rects[k]));
    let buttons = (0..11).filter(|&k| shown(k)).filter_map(|k| Some((QUICK_ALL[k], rects[k]?))).collect();
    QuickLayout { buttons, zone }
}

/// What the quick menu looks like right now.
#[derive(Debug, Clone, Copy, Default)]
pub struct QuickLook {
    pub hover: Option<QuickButton>,
    pub auto: bool,
    pub skipping: bool,
    /// Back has nowhere to go; Q.Load has no quick save to load (a picture is drawn dimmed).
    pub back_off: bool,
    pub qload_off: bool,
}

/// Draw the quick menu at opacity `alpha` (0..1: the hover mode fades it).
pub fn draw_quick(
    out: &mut Vec<Draw>,
    theme: &Theme,
    layout: &QuickLayout,
    look: &QuickLook,
    size_of: &dyn Fn(&str) -> Option<Size>,
    alpha: f32,
) {
    if alpha <= 0.0 {
        return;
    }
    let fade = |c: [f32; 4]| quillstone_render::faded(c, alpha);
    for &(b, rect) in &layout.buttons {
        let spec = &theme.quick_icons[quick_index(b)];
        let on = (b == QuickButton::Auto && look.auto) || (b == QuickButton::Skip && look.skipping);
        let hovered = look.hover == Some(b);
        if let Some((idle, _)) = quick_picture(spec, size_of) {
            let off = (b == QuickButton::Back && look.back_off) || (b == QuickButton::QuickLoad && look.qload_off);
            let loaded = |p| loaded_picture(p, size_of);
            let (src, a) = match (loaded(&spec.on).filter(|_| on && !off), loaded(&spec.hover).filter(|_| hovered && !off)) {
                (Some(on), _) => (on, 1.0),
                (None, Some(hover)) => (hover, 1.0),
                _ if off => (idle, ICON_DISABLED_ALPHA),
                _ if on || hovered || spec.hover.is_some() => (idle, 1.0),
                _ => (idle, ICON_IDLE_ALPHA),
            };
            let uv = FULL_UV;
            out.push(Draw::Image { src: src.to_string(), dest: rect, uv, alpha: a * alpha, shade: 1.0 });
            if on && loaded(&spec.on).is_none() {
                let bar = Rect::new(rect.x, rect.y + rect.h - 3.0, rect.w, 3.0);
                out.push(Draw::Rect { rect: bar, color: fade(theme.accent_color) });
            }
            continue;
        }
        let label = match b {
            QuickButton::Back => "Back",
            QuickButton::History => "History",
            QuickButton::Skip => "Skip",
            QuickButton::Auto => "Auto",
            QuickButton::Save => "Save",
            QuickButton::Load => "Load",
            QuickButton::QuickSave => "Q.Save",
            QuickButton::QuickLoad => "Q.Load",
            QuickButton::Settings => "Settings",
            QuickButton::Items => "Items",
            QuickButton::Journal => "Journal",
        };
        let color = if on {
            theme.accent_color
        } else if hovered {
            theme.text_color
        } else {
            theme.dim_color
        };
        text(out, label, rect, quick_text_size(label, rect.w), fade(color), Align::Center);
    }
}

/// The badge text for the modes that run: "AUTO", "SKIP »" or both; `None` when neither does.
pub fn mode_badge_text(auto: bool, skipping: bool) -> Option<String> {
    match (auto, skipping) {
        (false, false) => None,
        (true, false) => Some("AUTO".into()),
        (false, true) => Some("SKIP »".into()),
        (true, true) => Some("AUTO  SKIP »".into()),
    }
}

/// Where the mode badge sits: top-right, inside a 40 px margin, sized for its text.
pub fn mode_badge_rect(label: &str) -> Rect {
    let w = 36.0 + label.chars().count() as f32 * 19.0;
    Rect::new(VIRTUAL.w - 40.0 - w, 30.0, w, 44.0)
}

/// A small badge in the corner while auto-forward and/or skip is on, wherever the
/// quick menu is or is not (it only tints its labels).
pub fn draw_mode_badge(out: &mut Vec<Draw>, theme: &Theme, auto: bool, skipping: bool) {
    let Some(label) = mode_badge_text(auto, skipping) else { return };
    let r = mode_badge_rect(&label);
    out.push(Draw::Rect { rect: r, color: [0.0, 0.0, 0.0, 0.6] });
    text(out, label, r, 26.0, theme.text_color, Align::Center);
}

/// The countdown bar of a timed `::choice`: a dim track above the first button with the
/// time left (`left`, 0..1) filling it from the left. With no buttons (a timed `::hotspots`)
/// it runs along the top of the screen, as wide as the buttons would be.
pub fn draw_choice_timer(out: &mut Vec<Draw>, theme: &Theme, buttons: usize, left: f32) {
    let Some(first) = quillstone_render::choice_rects(theme, buttons.max(1)).first().copied() else { return };
    let h = theme.choice_timer_height;
    let y = if buttons == 0 { 24.0 + h } else { first.y };
    let track = Rect::new(first.x, (y - 8.0 - h).max(0.0), first.w, h);
    out.push(Draw::Rect { rect: track, color: [0.0, 0.0, 0.0, 0.45] });
    let left = left.clamp(0.0, 1.0);
    if left > 0.0 {
        let color = theme.choice_timer_color.unwrap_or(theme.accent_color);
        out.push(Draw::Rect { rect: Rect::new(track.x, track.y, track.w * left, h), color });
    }
}

/// The `::timing` bar in the middle of the screen: a track (the theme's picture or `panel_color`), the
/// good and perfect zones over it, the marker at `pos` (0..1) and the prompt above. A picture that is
/// not loaded yet is skipped, so the bar is never missing.
pub fn draw_timing_bar(out: &mut Vec<Draw>, theme: &Theme, prompt: &str, bar: &TimingBar, pos: f32, image_size: &dyn Fn(&str) -> Option<Size>) {
    let track = Rect::new(460.0, 520.0, 1000.0, 56.0);
    picture_or_rect(out, theme.timing_bar_image.as_deref(), track, theme.panel_color, image_size);
    let zone = |z: &[f32; 2], color: [f32; 4]| Draw::Rect { rect: Rect::new(track.x + track.w * z[0], track.y, track.w * (z[1] - z[0]), track.h), color };
    let [gr, gg, gb, ga] = theme.timing_good_color.unwrap_or(theme.accent_color);
    out.push(zone(&bar.good, [gr, gg, gb, ga * 0.6]));
    if let Some(p) = &bar.perfect {
        out.push(zone(p, theme.timing_perfect_color.unwrap_or(theme.name_color)));
    }
    let x = track.x + track.w * pos.clamp(0.0, 1.0);
    let marker = Rect::new(x - 5.0, track.y - 12.0, 10.0, track.h + 24.0);
    match theme.timing_marker_image.as_deref().and_then(|s| Some((s, image_size(s)?))) {
        Some((src, size)) => {
            let w = marker.h * size.w / size.h.max(1.0);
            let dest = Rect::new(x - w / 2.0, marker.y, w, marker.h);
            out.push(Draw::Image { src: src.to_string(), dest, uv: FULL_UV, alpha: 1.0, shade: 1.0 });
        }
        None => out.push(Draw::Rect { rect: marker, color: theme.timing_marker_color.unwrap_or(theme.text_color) }),
    }
    if !prompt.is_empty() {
        rich_text(out, prompt, Rect::new(track.x, track.y - 100.0, track.w, 60.0), 44.0, theme.text_color);
    }
}

/// The `::press` prompt in the middle of the screen: the prompt, a key cap with the key's name, a bar
/// of the time left and, for mashing, a bar of the presses made. The cap is `press_key_image` when it is
/// loaded, else `panel_color`; the time bar is the timed choice's colour, the presses fill `name_color`.
pub fn draw_press_prompt(out: &mut Vec<Draw>, theme: &Theme, prompt: &str, press: &KeyPress, presses: u32, elapsed: f32, image_size: &dyn Fn(&str) -> Option<Size>) {
    if !prompt.is_empty() {
        rich_text(out, prompt, Rect::new(460.0, 330.0, 1000.0, 60.0), 44.0, theme.text_color);
    }
    let cap = Rect::new(880.0, 410.0, 160.0, 120.0);
    picture_or_rect(out, theme.press_key_image.as_deref(), cap, theme.panel_color, image_size);
    text(out, press.key.to_uppercase(), Rect::new(cap.x, cap.y + 30.0, cap.w, 60.0), 52.0, theme.text_color, Align::Center);
    let mut bar = |y: f32, fraction: f32, color: [f32; 4]| {
        let track = Rect::new(460.0, y, 1000.0, 24.0);
        out.push(Draw::Rect { rect: track, color: [0.0, 0.0, 0.0, 0.45] });
        if fraction > 0.0 {
            out.push(Draw::Rect { rect: Rect::new(track.x, track.y, track.w * fraction, track.h), color });
        }
    };
    bar(570.0, press.time_left(elapsed), theme.choice_timer_color.unwrap_or(theme.accent_color));
    if press.count > 1 {
        bar(606.0, press.progress(presses), theme.timing_perfect_color.unwrap_or(theme.name_color));
    }
}

// ---- main menu ----------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MenuButton {
    Start,
    /// Load the newest save; only offered when there is one.
    Continue,
    Load,
    /// The gallery and music room; only offered when the game has either.
    Extras,
    Settings,
    Quit,
}

pub fn menu_buttons(can_continue: bool, extras: bool) -> Vec<MenuButton> {
    let mut out = vec![MenuButton::Start];
    if can_continue {
        out.push(MenuButton::Continue);
    }
    out.push(MenuButton::Load);
    if extras {
        out.push(MenuButton::Extras);
    }
    out.extend([MenuButton::Settings, MenuButton::Quit]);
    out
}

/// Up to six buttons fit under the title.
pub fn menu_button_rect(i: usize) -> Rect {
    Rect::new((VIRTUAL.w - 420.0) / 2.0, 400.0 + i as f32 * 96.0, 420.0, 76.0)
}

pub fn menu_at(x: f32, y: f32, buttons: &[MenuButton]) -> Option<MenuButton> {
    (0..buttons.len()).find(|&i| menu_button_rect(i).contains(x, y)).map(|i| buttons[i])
}

/// A theme picture over the whole screen (nothing if there is none or it isn't loaded).
fn draw_full_picture(out: &mut Vec<Draw>, src: Option<&str>, image_size: &dyn Fn(&str) -> Option<Size>) {
    if let Some((src, size)) = src.and_then(|src| image_size(src).map(|size| (src, size))) {
        let p = quillstone_render::place(size, screen(), quillstone_script::Fit::Cover);
        out.push(Draw::Image { src: src.to_string(), dest: p.dest, uv: p.uv, alpha: 1.0, shade: 1.0 });
    }
}

/// The main menu: the theme's background picture (or black), its overlay picture, the title, buttons.
/// `focus` is the button the keyboard is on.
/// An empty `title` and no `buttons` draw just the menu's background (what an overlay opened from the menu leaves).
pub fn draw_main_menu(
    out: &mut Vec<Draw>,
    theme: &Theme,
    title: &str,
    buttons: &[MenuButton],
    focus: Option<usize>,
    hover: Option<MenuButton>,
    image_size: &dyn Fn(&str) -> Option<Size>,
) {
    out.push(Draw::Rect { rect: screen(), color: [0.0, 0.0, 0.0, 1.0] });
    draw_full_picture(out, theme.menu_background.as_deref(), image_size);
    draw_full_picture(out, theme.menu_overlay.as_deref(), image_size);
    if !title.is_empty() {
        text(out, title, Rect::new(0.0, 180.0, VIRTUAL.w, 160.0), 96.0, theme.accent_color, Align::Center);
    }
    for (i, &b) in buttons.iter().enumerate() {
        let label = match b {
            MenuButton::Start => "Start",
            MenuButton::Continue => "Continue",
            MenuButton::Load => "Load",
            MenuButton::Extras => "Extras",
            MenuButton::Settings => "Settings",
            MenuButton::Quit => "Quit",
        };
        let r = menu_button_rect(i);
        let lit = hover == Some(b) || focus == Some(i);
        out.push(Draw::Rect { rect: r, color: if lit { theme.button_hover_color } else { theme.button_color } });
        text(out, label, r, 40.0, theme.text_color, Align::Center);
    }
}

// ---- settings ---------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Row {
    TextSpeed,
    AutoDelay,
    SkipUnread,
    Music,
    Sfx,
    Voice,
    Fullscreen,
    QuickMenu,
    /// Only for a game with several languages (`languages:` in the front matter).
    Language,
}

pub const ROWS: [Row; 8] =
    [Row::TextSpeed, Row::AutoDelay, Row::SkipUnread, Row::Music, Row::Sfx, Row::Voice, Row::Fullscreen, Row::QuickMenu];

const ROWS_WITH_LANGUAGE: [Row; 9] = [
    Row::TextSpeed,
    Row::AutoDelay,
    Row::SkipUnread,
    Row::Music,
    Row::Sfx,
    Row::Voice,
    Row::Fullscreen,
    Row::QuickMenu,
    Row::Language,
];

/// The rows of the settings screen: the Language row only when the game has several languages.
pub fn rows(language: bool) -> &'static [Row] {
    if language {
        &ROWS_WITH_LANGUAGE
    } else {
        &ROWS
    }
}

impl Row {
    /// Rows that are a button (on/off, or a list the button cycles through) rather than sliders.
    pub fn is_toggle(self) -> bool {
        matches!(self, Row::SkipUnread | Row::Fullscreen | Row::QuickMenu | Row::Language)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingsButton {
    Back,
    /// Only offered during a story.
    MainMenu,
    Quit,
}

/// The buttons under the settings rows: "Main menu" only makes sense in a story.
pub fn settings_buttons(in_story: bool) -> &'static [SettingsButton] {
    if in_story {
        &[SettingsButton::Back, SettingsButton::MainMenu, SettingsButton::Quit]
    } else {
        &[SettingsButton::Back, SettingsButton::Quit]
    }
}

/// What a point on the settings screen is over.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingsHit {
    /// The control of a row: a slider track, or an on/off button.
    Control(usize),
    Button(SettingsButton),
}

/// The settings panel (where the theme puts it) and the factor the theme's `options_text_size` makes
/// of every size on the screen (1 at the default size, so the default layout is the old fixed one).
fn panel(t: &Theme) -> Rect {
    let [x, y, w, h] = t.options_rect;
    Rect::new(x, y, w, h)
}

/// Also shrunk to fit the panel (the rows and buttons need about 860 x factor px of height, 800 of width),
/// so a small `options_rect` never pushes the buttons off it; the default panel is not shrunk.
fn factor(t: &Theme) -> f32 {
    let [_, _, w, h] = t.options_rect;
    (t.options_text_size / DEFAULT_OPTIONS_TEXT_SIZE).min(h / 860.0).min(w / 800.0)
}

/// Top and height of a row: tighter with nine rows so the buttons still clear the hint line.
fn row_y(n: usize, t: &Theme) -> f32 {
    panel(t).y + factor(t) * if n > 8 { 80.0 } else { 100.0 }
}

fn row_h(n: usize, t: &Theme) -> f32 {
    factor(t) * if n > 8 { 66.0 } else { 72.0 }
}

/// The slider tracks start 480/1120 and are 440/1120 of the panel's width across.
fn track_x(t: &Theme) -> f32 {
    let p = panel(t);
    p.x + p.w * 480.0 / 1120.0
}

fn track_w(t: &Theme) -> f32 {
    panel(t).w * 440.0 / 1120.0
}

pub fn row_rect(i: usize, n: usize, t: &Theme) -> Rect {
    let p = panel(t);
    Rect::new(p.x, row_y(n, t) + i as f32 * row_h(n, t), p.w, row_h(n, t) - 12.0 * factor(t))
}

/// The visible slider track of row `i`.
pub fn track_rect(i: usize, n: usize, t: &Theme) -> Rect {
    Rect::new(track_x(t), row_rect(i, n, t).y + 24.0 * factor(t), track_w(t), 12.0 * factor(t))
}

/// Where a row's control reacts to the mouse (taller than the thin track).
pub fn control_rect(i: usize, rows: &[Row], t: &Theme) -> Rect {
    let (row, k) = (row_rect(i, rows.len(), t), factor(t));
    if rows[i].is_toggle() {
        let w = if matches!(rows[i], Row::QuickMenu | Row::Language) { 260.0 } else { 160.0 };
        Rect::new(track_x(t), row.y + 4.0 * k, w * k, 52.0 * k)
    } else {
        Rect::new(track_x(t) - 12.0, row.y, track_w(t) + 24.0, row.h)
    }
}

/// Where button `b` sits; the buttons are centred side by side in the panel.
pub fn button_rect(b: SettingsButton, in_story: bool, n: usize, t: &Theme) -> Rect {
    let buttons = settings_buttons(in_story);
    let i = buttons.iter().position(|x| *x == b).unwrap_or(0);
    let (p, k, count) = (panel(t), factor(t), buttons.len() as f32);
    let gap = 40.0 * k;
    let w = (240.0 * k).min((p.w - gap * (count - 1.0)) / count); // a narrow panel makes them narrower
    let total = count * w + (count - 1.0) * gap;
    let y = row_y(n, t) + n as f32 * row_h(n, t) + 60.0 * k;
    Rect::new(p.x + (p.w - total) / 2.0 + i as f32 * (w + gap), y, w, 80.0 * k)
}

pub fn settings_at(x: f32, y: f32, in_story: bool, rows: &[Row], t: &Theme) -> Option<SettingsHit> {
    if let Some(i) = (0..rows.len()).find(|&i| control_rect(i, rows, t).contains(x, y)) {
        return Some(SettingsHit::Control(i));
    }
    settings_buttons(in_story)
        .iter()
        .copied()
        .find(|&b| button_rect(b, in_story, rows.len(), t).contains(x, y))
        .map(SettingsHit::Button)
}

/// Slider position (0..1) for a mouse x over row `i`'s track.
pub fn slider_pos_at(x: f32, t: &Theme) -> f32 {
    let t = track_rect(0, 8, t);
    ((x - t.x) / t.w).clamp(0.0, 1.0)
}

/// The slider position of a row, or `None` for the on/off rows.
pub fn row_slider(s: &Settings, row: Row) -> Option<f32> {
    match row {
        Row::TextSpeed => Some(text_speed_to_slider(s.text_speed)),
        Row::AutoDelay => Some(auto_delay_to_slider(s.auto_delay)),
        Row::Music => Some(s.music_volume),
        Row::Sfx => Some(s.sfx_volume),
        Row::Voice => Some(s.voice_volume),
        Row::SkipUnread | Row::Fullscreen | Row::QuickMenu | Row::Language => None,
    }
}

/// Set a slider row from a position (0..1). Does nothing for the on/off rows.
pub fn set_row_slider(s: &mut Settings, row: Row, pos: f32) {
    match row {
        Row::TextSpeed => s.text_speed = text_speed_from_slider(pos),
        Row::AutoDelay => s.auto_delay = auto_delay_from_slider(pos),
        Row::Music => s.music_volume = volume_from_slider(pos),
        Row::Sfx => s.sfx_volume = volume_from_slider(pos),
        Row::Voice => s.voice_volume = volume_from_slider(pos),
        Row::SkipUnread | Row::Fullscreen | Row::QuickMenu | Row::Language => {}
    }
}

fn row_label(row: Row) -> &'static str {
    match row {
        Row::TextSpeed => "Text speed",
        Row::AutoDelay => "Auto-forward delay",
        Row::SkipUnread => "Skip unread text",
        Row::Music => "Music volume",
        Row::Sfx => "Sound volume",
        Row::Voice => "Voice volume",
        Row::Fullscreen => "Fullscreen",
        Row::QuickMenu => "Quick menu",
        Row::Language => "Language",
    }
}

/// The text of a row's value; `language` is the name of the language in use (for the Language row).
pub fn row_value(s: &Settings, row: Row, language: &str) -> String {
    match row {
        Row::TextSpeed if s.text_speed <= 0.0 => "Instant".into(),
        Row::TextSpeed => format!("{} chars/s", s.text_speed.round()),
        Row::AutoDelay => format!("{:.1} s", s.auto_delay),
        Row::Music => format!("{}%", (s.music_volume * 100.0).round()),
        Row::Sfx => format!("{}%", (s.sfx_volume * 100.0).round()),
        Row::Voice => format!("{}%", (s.voice_volume * 100.0).round()),
        Row::SkipUnread => if s.skip_unread { "On" } else { "Off" }.into(),
        Row::Fullscreen => if s.fullscreen { "On" } else { "Off" }.into(),
        Row::Language => language.to_string(),
        Row::QuickMenu => match s.quick_menu {
            QuickMenuSetting::Default => "Game default",
            QuickMenuSetting::Visible => "Always",
            QuickMenuSetting::Hover => "On hover",
            QuickMenuSetting::Hidden => "Hidden",
        }
        .into(),
    }
}

/// A slider row's value column, with a margin before the row's right edge.
pub fn value_rect(i: usize, n: usize, t: &Theme) -> Rect {
    let row = row_rect(i, n, t);
    let x = track_x(t) + track_w(t) + 20.0;
    Rect::new(x, row.y + 6.0 * factor(t), row.x + row.w - 20.0 - x, 48.0 * factor(t))
}

/// `src` stretched over `dest` as nine pieces: corners of `border` picture pixels keep their size, edges
/// stretch one way and the middle both, so a frame's rounded corners stay round at any panel size.
/// `size` is the picture's own size; a border that would eat the picture or the panel is cut back.
pub fn nine_slice(out: &mut Vec<Draw>, src: &str, size: Size, dest: Rect, border: f32) {
    let b = border.min(size.w / 2.0 - 0.5).min(size.h / 2.0 - 0.5).min(dest.w / 2.0).min(dest.h / 2.0).max(0.0);
    let (ux, uy) = (b / size.w, b / size.h);
    let xs = [(dest.x, b, 0.0, ux), (dest.x + b, dest.w - 2.0 * b, ux, 1.0 - 2.0 * ux), (dest.x + dest.w - b, b, 1.0 - ux, ux)];
    let ys = [(dest.y, b, 0.0, uy), (dest.y + b, dest.h - 2.0 * b, uy, 1.0 - 2.0 * uy), (dest.y + dest.h - b, b, 1.0 - uy, uy)];
    for (y, h, v, vh) in ys {
        for (x, w, u, uw) in xs {
            if w > 0.0 && h > 0.0 {
                out.push(Draw::Image { src: src.to_string(), dest: Rect::new(x, y, w, h), uv: Rect::new(u, v, uw, vh), alpha: 1.0, shade: 1.0 });
            }
        }
    }
}

/// A value and the size to draw it at so that it fits one row `width` wide: stepped
/// down from `size` (not below 60%), then cut with an ellipsis.
pub(crate) fn fit_value(value: &str, size: f32, width: f32) -> (String, f32) {
    let mut s = size;
    while s > size * 0.6 && estimate_lines(value, s, width) > 1 {
        s -= 1.0;
    }
    (one_row(value, s, width), s)
}

#[allow(clippy::too_many_arguments)]
pub fn draw_settings(
    out: &mut Vec<Draw>,
    theme: &Theme,
    s: &Settings,
    in_story: bool,
    focus: usize,
    hover: Option<SettingsHit>,
    language: Option<&str>,
    image_size: &dyn Fn(&str) -> Option<Size>,
) {
    let rows = rows(language.is_some());
    let language = language.unwrap_or("");
    fn loaded<'a>(p: &'a Option<String>, image_size: &dyn Fn(&str) -> Option<Size>) -> Option<(&'a str, Size)> {
        p.as_deref().and_then(|src| image_size(src).map(|size| (src, size)))
    }
    match loaded(&theme.options_background, image_size) {
        Some((src, size)) => {
            let p = quillstone_render::place(size, screen(), quillstone_script::Fit::Cover);
            out.push(Draw::Image { src: src.to_string(), dest: p.dest, uv: p.uv, alpha: 1.0, shade: 1.0 });
        }
        None => out.push(Draw::Rect { rect: screen(), color: theme.overlay_color }),
    }
    draw_full_picture(out, theme.game_menu_overlay.as_deref(), image_size);
    let (p, k) = (panel(theme), factor(theme));
    if let Some((src, size)) = loaded(&theme.options_panel_image, image_size) {
        nine_slice(out, src, size, p, theme.options_panel_border);
    }
    text(out, "Settings", Rect::new(p.x, p.y, p.w, 80.0 * k), 56.0 * k, theme.text_color, Align::Center);

    let n = rows.len();
    for (i, &row) in rows.iter().enumerate() {
        let r = row_rect(i, n, theme);
        if i == focus {
            out.push(Draw::Rect { rect: r, color: FOCUS });
        }
        let label_w = track_x(theme) - r.x - 30.0 * k - 10.0;
        text(out, row_label(row), Rect::new(r.x + 30.0 * k, r.y + 6.0 * k, label_w, 48.0 * k), 36.0 * k, theme.text_color, Align::Left);
        match row_slider(s, row) {
            Some(pos) => {
                let t = track_rect(i, n, theme);
                out.push(Draw::Rect { rect: t, color: TRACK });
                let fill = [theme.button_hover_color[0], theme.button_hover_color[1], theme.button_hover_color[2], 1.0];
                out.push(Draw::Rect { rect: Rect::new(t.x, t.y, t.w * pos, t.h), color: fill });
                out.push(Draw::Rect { rect: Rect::new(t.x + t.w * pos - 10.0 * k, r.y + 10.0 * k, 20.0 * k, 40.0 * k), color: theme.text_color });
                let value = value_rect(i, n, theme);
                let (shown, size) = fit_value(&row_value(s, row, language), 32.0 * k, value.w);
                text(out, shown, value, size, theme.dim_color, Align::Left);
            }
            None => {
                let c = control_rect(i, rows, theme);
                let color = if hover == Some(SettingsHit::Control(i)) { theme.button_hover_color } else { theme.button_color };
                out.push(Draw::Rect { rect: c, color });
                text(out, row_value(s, row, language), c, 32.0 * k, theme.text_color, Align::Center);
            }
        }
    }

    for &b in settings_buttons(in_story) {
        let label = match b {
            SettingsButton::Back => "Back",
            SettingsButton::MainMenu => "Main menu",
            SettingsButton::Quit => "Quit",
        };
        let r = button_rect(b, in_story, n, theme);
        let color = if hover == Some(SettingsHit::Button(b)) { theme.button_hover_color } else { theme.button_color };
        out.push(Draw::Rect { rect: r, color });
        text(out, label, r, 36.0 * k, theme.text_color, Align::Center);
    }
    let hint = "Esc to close  ·  Up/Down to pick  ·  Left/Right to adjust";
    text(out, hint, Rect::new(p.x, p.y + p.h - 40.0 * k, p.w, 40.0 * k), 24.0 * k, theme.dim_color, Align::Center);
}

// ---- backlog ----------------------------------------------------------------------------

const LOG_TOP: f32 = 130.0;
const LOG_BOTTOM: f32 = 960.0;
const LOG_SIZE: f32 = 32.0;
const LOG_GAP: f32 = 20.0;
const NAME_X: f32 = 160.0;
const NAME_W: f32 = 320.0;
const TEXT_X: f32 = 500.0;
const TEXT_W: f32 = 1260.0;

/// The text of a history row, without styling.
fn log_body(entry: &Logged) -> String {
    match entry {
        Logged::Line(line) => plain(&line.text),
        Logged::Choice(choice) => format!("» {}", plain(choice)),
    }
}

fn log_line_height() -> f32 {
    line_height(LOG_SIZE)
}

fn log_entry_height(entry: &Logged) -> f32 {
    estimate_lines(&log_body(entry), LOG_SIZE, TEXT_W) as f32 * log_line_height()
}

/// How far back the backlog can scroll (in entries) without running out: until the oldest
/// entry is on screen (more would only show it alone on an empty page). 0 if all fit.
pub fn backlog_max_scroll(entries: &[Logged]) -> usize {
    let room = LOG_BOTTOM - LOG_TOP;
    // Scrolling back by `s` shows the oldest `len - s` entries, bottom-aligned: the oldest is
    // in view once they fit, which is the first `s` for which that prefix is no taller than the room.
    let mut s = 0;
    let mut total: f32 = entries.iter().map(log_entry_height).sum::<f32>() + LOG_GAP * entries.len().saturating_sub(1) as f32;
    while total > room && s + 1 < entries.len() {
        let newest = &entries[entries.len() - 1 - s];
        total -= log_entry_height(newest) + LOG_GAP;
        s += 1;
    }
    s
}

/// Draw the backlog, newest entry at the bottom, `scroll` entries back from the end.
pub fn draw_backlog(
    out: &mut Vec<Draw>,
    theme: &Theme,
    entries: &[Logged],
    scroll: usize,
    image_size: &dyn Fn(&str) -> Option<Size>,
) {
    out.push(Draw::Rect { rect: screen(), color: theme.overlay_color });
    draw_full_picture(out, theme.game_menu_overlay.as_deref(), image_size);
    text(out, "History", Rect::new(0.0, 40.0, VIRTUAL.w, 70.0), 48.0, theme.text_color, Align::Center);
    let hint = "Wheel / Page Up / Page Down to scroll  ·  Esc to close";
    text(out, hint, Rect::new(0.0, 990.0, VIRTUAL.w, 40.0), 24.0, theme.dim_color, Align::Center);
    if entries.is_empty() {
        text(out, "Nothing yet.", screen(), 36.0, theme.dim_color, Align::Center);
        return;
    }

    let line_h = log_line_height();
    let mut bottom = LOG_BOTTOM;
    for entry in entries.iter().rev().skip(scroll) {
        let (speaker, color, body_color) = match entry {
            Logged::Line(line) => {
                let color = line.color.map(|[r, g, b]| [r, g, b, 1.0]).unwrap_or(theme.name_color);
                (line.speaker.as_deref(), color, theme.text_color)
            }
            Logged::Choice(_) => (None, theme.accent_color, theme.accent_color),
        };
        let body = log_body(entry);
        let h = log_entry_height(entry);
        let top = bottom - h;
        if top < LOG_TOP {
            break; // only whole entries are drawn
        }
        if let Some(name) = speaker {
            // A long name steps down in size until it fits on one row (the row is one line tall).
            let mut size = LOG_SIZE;
            while size > 20.0 && estimate_lines(name, size, NAME_W) > 1 {
                size -= 2.0;
            }
            text(out, name, Rect::new(NAME_X, top, NAME_W, line_h), size, color, Align::Left);
        }
        text(out, body, Rect::new(TEXT_X, top, TEXT_W, h), LOG_SIZE, body_color, Align::Left);
        bottom = top - LOG_GAP;
    }
}

pub(crate) use quillstone_render::one_row;

// ---- save / load slots ---------------------------------------------------------------

/// What a point on a slot screen is over.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SlotHit {
    Tab(Page),
    Slot(usize),
    Back,
}

const TAB_W: f32 = 90.0;
const TAB_GAP: f32 = 10.0;
const SLOT_W: f32 = 480.0;
const SLOT_H: f32 = 330.0;
/// Slots per row on the save and load screens.
pub const SLOT_COLUMNS: usize = 3;

pub fn tab_rect(i: usize, count: usize) -> Rect {
    let total = count as f32 * TAB_W + (count.max(1) - 1) as f32 * TAB_GAP;
    Rect::new((VIRTUAL.w - total) / 2.0 + i as f32 * (TAB_W + TAB_GAP), 130.0, TAB_W, 56.0)
}

pub fn slot_rect(i: usize) -> Rect {
    let x0 = (VIRTUAL.w - (SLOT_COLUMNS as f32 * SLOT_W + (SLOT_COLUMNS - 1) as f32 * 40.0)) / 2.0;
    let (col, row) = (i % SLOT_COLUMNS, i / SLOT_COLUMNS);
    Rect::new(x0 + col as f32 * (SLOT_W + 40.0), 220.0 + row as f32 * (SLOT_H + 36.0), SLOT_W, SLOT_H)
}

/// The 16:9 picture area of a slot.
pub fn thumb_rect(i: usize) -> Rect {
    let r = slot_rect(i);
    Rect::new(r.x + 16.0, r.y + 16.0, r.w - 32.0, (r.w - 32.0) * 9.0 / 16.0)
}

pub fn slots_back_rect() -> Rect {
    Rect::new((VIRTUAL.w - 240.0) / 2.0, 960.0, 240.0, 70.0)
}

pub fn slots_at(x: f32, y: f32, tabs: &[Page], slot_count: usize) -> Option<SlotHit> {
    if let Some(i) = (0..tabs.len()).find(|&i| tab_rect(i, tabs.len()).contains(x, y)) {
        return Some(SlotHit::Tab(tabs[i]));
    }
    if let Some(i) = (0..slot_count).find(|&i| slot_rect(i).contains(x, y)) {
        return Some(SlotHit::Slot(i));
    }
    slots_back_rect().contains(x, y).then_some(SlotHit::Back)
}

fn page_label(page: Page) -> String {
    match page {
        Page::Auto => "Auto".into(),
        Page::Quick => "Quick".into(),
        Page::Numbered(n) => n.to_string(),
    }
}

/// What the save or load screen shows.
pub struct SlotScreen<'a> {
    pub title: &'a str,
    pub tabs: &'a [Page],
    pub page: Page,
    pub slots: &'a [Option<SlotInfo>],
    pub hover: Option<SlotHit>,
}

/// The save or load screen: page tabs, a grid of slots, and a Back button.
pub fn draw_slots(
    out: &mut Vec<Draw>,
    theme: &Theme,
    shown: &SlotScreen,
    image_size: &dyn Fn(&str) -> Option<Size>,
) {
    let SlotScreen { title, tabs, page, slots, hover } = *shown;
    out.push(Draw::Rect { rect: screen(), color: theme.overlay_color });
    draw_full_picture(out, theme.game_menu_overlay.as_deref(), image_size);
    text(out, title, Rect::new(0.0, 40.0, VIRTUAL.w, 70.0), 48.0, theme.text_color, Align::Center);
    for (i, &tab) in tabs.iter().enumerate() {
        let r = tab_rect(i, tabs.len());
        let color = if tab == page || hover == Some(SlotHit::Tab(tab)) { theme.button_hover_color } else { theme.button_color };
        out.push(Draw::Rect { rect: r, color });
        text(out, page_label(tab), r, 28.0, if tab == page { theme.accent_color } else { theme.text_color }, Align::Center);
    }
    for (i, slot) in slots.iter().enumerate() {
        let r = slot_rect(i);
        let color = if hover == Some(SlotHit::Slot(i)) { theme.button_hover_color } else { theme.button_color };
        out.push(Draw::Rect { rect: r, color });
        let thumb = thumb_rect(i);
        let below = |dy: f32, h: f32| Rect::new(thumb.x, thumb.y + thumb.h + dy, thumb.w, h);
        match slot {
            None => {
                out.push(Draw::Rect { rect: thumb, color: [1.0, 1.0, 1.0, 0.04] });
                text(out, "Empty", thumb, 30.0, theme.dim_color, Align::Center);
            }
            Some(info) => {
                match &info.scene {
                    Some(scene) => out.extend(thumbnail(scene, image_size, thumb)),
                    None => {
                        out.push(Draw::Rect { rect: thumb, color: [0.3, 0.05, 0.05, 0.6] });
                    }
                }
                text(out, info.time.clone(), below(4.0, 30.0), 22.0, theme.dim_color, Align::Left);
                let row = below(32.0, 30.0);
                text(out, one_row(&info.preview, 22.0, row.w), row, 22.0, theme.text_color, Align::Left);
            }
        }
    }
    let back = slots_back_rect();
    let color = if hover == Some(SlotHit::Back) { theme.button_hover_color } else { theme.button_color };
    out.push(Draw::Rect { rect: back, color });
    text(out, "Back", back, 34.0, theme.text_color, Align::Center);
}

// ---- game-menu tabs ------------------------------------------------------------------

/// The tabs along the top of the Save, Load, Settings and History screens, like a game menu.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GameTab {
    Save,
    Load,
    Settings,
    History,
    MainMenu,
    Quit,
}

impl GameTab {
    /// Switches to a screen (the others ask a question first).
    pub fn is_screen(self) -> bool {
        !matches!(self, GameTab::MainMenu | GameTab::Quit)
    }

    fn label(self) -> &'static str {
        match self {
            GameTab::Save => "Save",
            GameTab::Load => "Load",
            GameTab::Settings => "Settings",
            GameTab::History => "History",
            GameTab::MainMenu => "Main menu",
            GameTab::Quit => "Quit",
        }
    }
}

/// The tabs on offer: from the main menu there is no story to save, read back or leave, so those are left out.
pub fn game_tabs(in_story: bool) -> &'static [GameTab] {
    if in_story {
        &[GameTab::Save, GameTab::Load, GameTab::Settings, GameTab::History, GameTab::MainMenu, GameTab::Quit]
    } else {
        &[GameTab::Load, GameTab::Settings, GameTab::Quit]
    }
}

const GAME_TAB_W: f32 = 200.0;
const GAME_TAB_GAP: f32 = 10.0;
/// The strip ends above every screen's title (the lowest starts at 40).
const GAME_TAB_Y: f32 = 4.0;
const GAME_TAB_H: f32 = 36.0;

pub fn game_tab_rect(i: usize, count: usize) -> Rect {
    let total = count as f32 * GAME_TAB_W + (count.max(1) - 1) as f32 * GAME_TAB_GAP;
    Rect::new((VIRTUAL.w - total) / 2.0 + i as f32 * (GAME_TAB_W + GAME_TAB_GAP), GAME_TAB_Y, GAME_TAB_W, GAME_TAB_H)
}

pub fn game_tab_at(x: f32, y: f32, tabs: &[GameTab]) -> Option<GameTab> {
    (0..tabs.len()).find(|&i| game_tab_rect(i, tabs.len()).contains(x, y)).map(|i| tabs[i])
}

/// Draw the strip; `current` is the screen being shown (highlighted like the slot page tabs).
pub fn draw_game_tabs(out: &mut Vec<Draw>, theme: &Theme, tabs: &[GameTab], current: Option<GameTab>, hover: Option<GameTab>) {
    for (i, &tab) in tabs.iter().enumerate() {
        let r = game_tab_rect(i, tabs.len());
        let color = if Some(tab) == current || hover == Some(tab) { theme.button_hover_color } else { theme.button_color };
        out.push(Draw::Rect { rect: r, color });
        text(out, tab.label(), r, 26.0, if Some(tab) == current { theme.accent_color } else { theme.text_color }, Align::Center);
    }
}

// ---- confirm dialog and notices --------------------------------------------------------------

const DIALOG: Rect = Rect::new(560.0, 380.0, 800.0, 300.0);

/// `true` for the Yes button, `false` for No.
pub fn confirm_button_rect(yes: bool) -> Rect {
    let x = if yes { 700.0 } else { 1000.0 };
    Rect::new(x, DIALOG.y + 190.0, 220.0, 70.0)
}

/// `Some(true)` over Yes, `Some(false)` over No.
pub fn confirm_at(x: f32, y: f32) -> Option<bool> {
    [true, false].into_iter().find(|&yes| confirm_button_rect(yes).contains(x, y))
}

pub fn draw_confirm(out: &mut Vec<Draw>, theme: &Theme, message: &str, hover: Option<bool>) {
    out.push(Draw::Rect { rect: screen(), color: [0.0, 0.0, 0.0, 0.5] });
    out.push(Draw::Rect { rect: DIALOG, color: [0.08, 0.07, 0.12, 0.97] });
    let msg = Rect::new(DIALOG.x + 40.0, DIALOG.y + 30.0, DIALOG.w - 80.0, 140.0);
    text(out, message, msg, 34.0, theme.text_color, Align::Center);
    for (yes, label) in [(true, "Yes"), (false, "No")] {
        let r = confirm_button_rect(yes);
        let color = if hover == Some(yes) { theme.button_hover_color } else { theme.button_color };
        out.push(Draw::Rect { rect: r, color });
        text(out, label, r, 34.0, theme.text_color, Align::Center);
    }
}

/// A short message at the top of the screen ("Saved", "Quick saved", errors).
pub fn draw_notice(out: &mut Vec<Draw>, theme: &Theme, message: &str) {
    let r = Rect::new((VIRTUAL.w - 700.0) / 2.0, 30.0, 700.0, 60.0);
    out.push(Draw::Rect { rect: r, color: [0.0, 0.0, 0.0, 0.95] }); // nearly opaque: a title under it must not show through
    text(out, message, r, 28.0, theme.text_color, Align::Center);
}

#[cfg(test)]
mod tests {
    use super::*;
    use quillstone_render::TEXTBOX;
    use quillstone_runtime::Line;

    fn centre(r: Rect) -> (f32, f32) {
        (r.x + r.w / 2.0, r.y + r.h / 2.0)
    }

    fn plain(inventory: bool) -> QuickLayout {
        quick_layout(TEXTBOX, &Theme::default(), inventory, false, &|_| None)
    }

    #[test]
    fn quick_menu_sits_under_the_textbox_and_is_hit_testable() {
        assert_eq!(plain(false).buttons.len(), 9);
        for (b, r) in &plain(false).buttons {
            assert!(r.y >= TEXTBOX.y + TEXTBOX.h && r.y + r.h <= VIRTUAL.h);
            let (x, y) = centre(*r);
            assert_eq!(plain(false).at(x, y), Some(*b));
            assert_eq!(plain(true).at(x, y), Some(*b), "Items doesn't move the others");
        }
        let items = plain(true).rect(QuickButton::Items).unwrap();
        let (x, y) = centre(items);
        assert_eq!(plain(true).at(x, y), Some(QuickButton::Items));
        assert_eq!(plain(false).at(x, y), None, "only games with an inventory have the button");
        assert!(items.x >= TEXTBOX.x && items.x + items.w <= plain(true).rect(QuickButton::Back).unwrap().x, "Items fits under the textbox, left of Back");
        let last = plain(true).rect(QuickButton::Settings).unwrap();
        assert!((last.x + QUICK_W - (TEXTBOX.x + TEXTBOX.w)).abs() < 1e-3);
        assert!(items.x >= TEXTBOX.x, "all buttons fit under the textbox");
        assert_eq!(plain(true).at(10.0, 10.0), None);
    }

    #[test]
    fn without_picture_keys_the_layout_is_one_row_under_the_textbox() {
        let l = plain(true);
        assert_eq!(l.buttons.len(), 10);
        assert_eq!(l.zone.len(), 1);
        for tb in [Rect::new(100.0, 200.0, 500.0, 300.0), Rect::new(160.0, 800.0, 1600.0, 280.0), Rect::new(200.0, 200.0, 1500.0, 300.0)] {
            let l = quick_layout(tb, &Theme::default(), false, false, &|_| None);
            assert_eq!(l.buttons.len(), 9);
            assert!(l.buttons.iter().all(|(_, r)| r.x >= 0.0 && r.x + r.w <= VIRTUAL.w && r.y + r.h <= VIRTUAL.h));
            assert!(l.in_zone(tb, tb.x + 5.0, tb.y + 5.0) && l.in_zone(tb, centre(l.zone[0]).0, centre(l.zone[0]).1));
            assert!(!l.in_zone(tb, 5.0, 5.0));
        }
    }

    fn themed(text: &str) -> Theme {
        let (t, problems) = Theme::parse(text);
        assert!(problems.is_empty(), "{problems:?}");
        t
    }

    fn sized(s: &str) -> Option<Size> {
        match s {
            "wide.png" => Some(Size::new(90.0, 40.0)),
            "huge.png" => Some(Size::new(2000.0, 4.0)),
            "gone.png" => None,
            _ => Some(Size::new(48.0, 48.0)),
        }
    }

    #[test]
    fn listed_mode_shows_only_buttons_with_a_picture_or_a_position() {
        let tb = Rect::new(160.0, 700.0, 1600.0, 260.0);
        let t = themed("quick_buttons = listed\nquick_back_icon = a.png\nquick_skip_pos = 10, 20\n");
        let l = quick_layout(tb, &t, true, false, &sized);
        let ids: Vec<QuickButton> = l.buttons.iter().map(|b| b.0).collect();
        assert_eq!(ids, [QuickButton::Back, QuickButton::Skip]);
        let back = l.rect(QuickButton::Back).unwrap();
        assert_eq!(back.x + back.w, tb.x + tb.w, "the only flow button is right-aligned alone");
        assert_eq!(l.rect(QuickButton::Skip).unwrap().x, 10.0);
        assert!(!l.in_zone(tb, 1000.0, tb.y + tb.h + QUICK_MARGIN + 5.0), "no strip where hidden buttons were");
        // without the key the same theme keeps every button
        let t = themed("quick_back_icon = a.png\nquick_skip_pos = 10, 20\n");
        assert_eq!(quick_layout(tb, &t, true, false, &sized).buttons.len(), 10);
        // nothing listed: no buttons at all
        assert!(quick_layout(tb, &themed("quick_buttons = listed\n"), true, false, &sized).buttons.is_empty());
    }

    #[test]
    fn a_picture_button_without_a_position_stays_in_the_row_sized_by_its_picture() {
        let tb = Rect::new(160.0, 700.0, 1600.0, 260.0);
        let t = themed("quick_back_icon = wide.png\nquick_settings_icon = a.png\n");
        let l = quick_layout(tb, &t, false, false, &sized);
        let (back, history, settings) = (
            l.rect(QuickButton::Back).unwrap(),
            l.rect(QuickButton::History).unwrap(),
            l.rect(QuickButton::Settings).unwrap(),
        );
        let huge = themed("quick_back_icon = huge.png\n");
        assert_eq!(quick_button_size(&huge.quick_icons[0], &sized), (512.0, 16.0), "clamped to the allowed sides");
        assert_eq!((back.w, back.h, settings.w, settings.h), (90.0, 40.0, 48.0, 48.0));
        assert_eq!((history.w, history.h), (QUICK_W, QUICK_H));
        assert_eq!(settings.x + settings.w, tb.x + tb.w, "still right-aligned");
        assert!(back.x + back.w + QUICK_GAP <= history.x, "in the row, in order");
        let row_top = tb.y + tb.h + QUICK_MARGIN;
        assert_eq!(settings.y, row_top, "the tallest sets the row");
        assert_eq!(back.y, row_top + 4.0, "the others are centred on it");
        // an explicit size beats the picture's
        let t = themed("quick_back_icon = wide.png\nquick_back_size = 100, 30\n");
        assert_eq!(quick_layout(TEXTBOX, &t, false, false, &sized).rect(QuickButton::Back).unwrap().w, 100.0);
        // a picture that isn't loaded is a plain text button
        let t = themed("quick_back_icon = gone.png\n");
        let r = quick_layout(TEXTBOX, &t, false, false, &sized).rect(QuickButton::Back).unwrap();
        assert_eq!((r.w, r.h), (QUICK_W, QUICK_H));
    }

    #[test]
    fn a_placed_button_leaves_the_row_and_is_kept_on_the_screen() {
        let t = themed("quick_back_icon = a.png\nquick_back_pos = 20, 30\nquick_load_pos = 1900, 1070\n");
        let l = quick_layout(TEXTBOX, &t, true, false, &sized);
        assert_eq!(l.rect(QuickButton::Back), Some(Rect::new(20.0, 30.0, 48.0, 48.0)));
        let load = l.rect(QuickButton::Load).unwrap();
        assert_eq!((load.x + load.w, load.y + load.h), (1920.0, 1080.0), "a text button too, pulled back in");
        let plain = plain(true);
        assert_eq!(l.rect(QuickButton::Settings), plain.rect(QuickButton::Settings), "right-aligned ones stay");
        assert_eq!(l.rect(QuickButton::Items), plain.rect(QuickButton::Items).map(|r| Rect::new(r.x + 158.0 * 2.0, r.y, r.w, r.h)),
            "Items stays leftmost of the row, which closed up over the two placed buttons");
        assert_eq!(l.rect(QuickButton::Save), plain.rect(QuickButton::Save).map(|r| Rect::new(r.x + 158.0, r.y, r.w, r.h)));
        // hit-testing follows
        assert_eq!(l.at(30.0, 40.0), Some(QuickButton::Back));
        let old = plain.rect(QuickButton::Back).unwrap();
        assert_eq!(l.at(old.x + 1.0, old.y + 1.0), None, "its old place in the row is empty now... ");
    }

    #[test]
    fn where_placed_buttons_overlap_the_later_one_in_the_id_order_is_on_top() {
        let t = themed("quick_settings_pos = 100, 100\nquick_back_pos = 120, 110\nquick_items_pos = 130, 120\n");
        let l = quick_layout(TEXTBOX, &t, true, false, &sized);
        assert_eq!(l.at(125.0, 115.0), Some(QuickButton::Settings), "settings comes after back");
        assert_eq!(l.at(125.0, 105.0), Some(QuickButton::Settings));
        assert_eq!(l.at(135.0, 125.0), Some(QuickButton::Items), "items last of all");
        assert_eq!(l.at(121.0, 111.0), Some(QuickButton::Settings));
        assert_eq!(l.buttons.last().map(|b| b.0), Some(QuickButton::Items), "drawn last: on top");
        // without an inventory Items isn't there at all
        let l = quick_layout(TEXTBOX, &t, false, false, &sized);
        assert_eq!(l.at(135.0, 125.0), Some(QuickButton::Settings));
    }

    #[test]
    fn the_hover_zone_covers_the_textbox_the_row_and_every_placed_button() {
        let t = themed("quick_back_pos = 20, 30\nquick_items_pos = 900, 20\n");
        let l = quick_layout(TEXTBOX, &t, false, false, &sized);
        assert!(l.in_zone(TEXTBOX, 30.0, 40.0), "a placed button far from the box");
        assert!(!l.in_zone(TEXTBOX, 905.0, 25.0), "Items isn't offered here");
        assert!(l.in_zone(TEXTBOX, TEXTBOX.x + 5.0, TEXTBOX.y + 5.0));
        let row = l.rect(QuickButton::Settings).unwrap();
        assert!(l.in_zone(TEXTBOX, row.x + 1.0, row.y + 1.0));
        assert!(!l.in_zone(TEXTBOX, 400.0, 400.0));
        assert!(quick_layout(TEXTBOX, &t, true, false, &sized).in_zone(TEXTBOX, 905.0, 25.0));
        // every button placed: no row, no strip
        let all: String = quillstone_render::theme::QUICK_IDS.iter().map(|id| format!("quick_{id}_pos = 10, 10\n")).collect();
        let l = quick_layout(TEXTBOX, &themed(&all), true, true, &sized);
        assert_eq!(l.buttons.len(), 11);
        assert!(l.in_zone(TEXTBOX, 12.0, 12.0) && !l.in_zone(TEXTBOX, 1000.0, TEXTBOX.y + TEXTBOX.h + 10.0));
    }

    #[test]
    fn the_journal_button_is_left_of_items_and_only_in_a_game_with_a_journal() {
        let t = Theme::default();
        let with = quick_layout(TEXTBOX, &t, true, true, &sized);
        let (journal, items) = (with.rect(QuickButton::Journal).unwrap(), with.rect(QuickButton::Items).unwrap());
        assert_eq!((with.buttons.len(), with.buttons[0].0), (11, QuickButton::Back));
        assert!(journal.x + journal.w < items.x && journal.x >= 0.0 && journal.y == items.y);
        assert_eq!(quick_layout(TEXTBOX, &t, true, false, &sized).rect(QuickButton::Journal), None);
        assert_eq!(quick_layout(TEXTBOX, &t, false, true, &sized).rect(QuickButton::Items), None, "a journal alone has no Items");
        // listed: a journal the theme gave no picture or place is not offered; one with a picture is
        let listed = themed("quick_buttons = listed\nquick_journal_icon = j.png\n");
        let l = quick_layout(TEXTBOX, &listed, true, true, &sized);
        assert_eq!(l.buttons.iter().map(|b| b.0).collect::<Vec<_>>(), [QuickButton::Journal]);
        assert!(quick_layout(TEXTBOX, &listed, true, false, &sized).buttons.is_empty());
    }

    fn row_span(l: &QuickLayout) -> (f32, f32) {
        let rects = l.buttons.iter().map(|b| b.1);
        (rects.clone().map(|r| r.x).fold(f32::MAX, f32::min), rects.map(|r| r.x + r.w).fold(0.0, f32::max))
    }

    #[test]
    fn quick_align_places_the_row_on_the_textbox_and_keeps_it_on_screen() {
        let row = |align: &str, tb: Rect| row_span(&quick_layout(tb, &themed(&format!("quick_align = {align}\n")), false, false, &sized));
        let total = 9.0 * QUICK_W + 8.0 * QUICK_GAP;
        let (l, r) = row("left", TEXTBOX);
        assert!((l - TEXTBOX.x).abs() < 1e-3 && (r - l - total).abs() < 1e-3);
        let (l, r) = row("center", TEXTBOX);
        assert!(((l + r) / 2.0 - (TEXTBOX.x + TEXTBOX.w / 2.0)).abs() < 1e-3);
        let (_, r) = row("right", TEXTBOX);
        assert!((r - (TEXTBOX.x + TEXTBOX.w)).abs() < 1e-3);
        // a textbox far right or left, or narrower than the row: every mode stays within 0..1920
        for tb in [Rect::new(1500.0, 780.0, 400.0, 260.0), Rect::new(0.0, 780.0, 300.0, 260.0), Rect::new(1400.0, 780.0, 520.0, 260.0)] {
            for align in ["left", "center", "right"] {
                let (l, r) = row(align, tb);
                assert!(l >= 0.0 && r <= VIRTUAL.w, "{align} {tb:?}: {l}..{r}");
            }
        }
        // the hover zone follows the row
        let l = quick_layout(TEXTBOX, &themed("quick_align = center\n"), false, false, &sized);
        let (a, b) = row_span(&l);
        assert!((l.zone[0].x - a).abs() < 1e-3 && (l.zone[0].w - (b - a)).abs() < 1e-3);
    }

    #[test]
    fn a_game_without_an_inventory_reserves_no_items_slot() {
        let (with, without) = (plain(true), plain(false));
        assert!((row_span(&with).1 - row_span(&without).1).abs() < 1e-3);
        assert!((row_span(&without).0 - row_span(&with).0 - (QUICK_W + QUICK_GAP)).abs() < 1e-3);
        assert!((with.zone[0].w - without.zone[0].w - (QUICK_W + QUICK_GAP)).abs() < 1e-3);
        let t = themed("quick_align = left\n");
        let l = quick_layout(TEXTBOX, &t, false, false, &sized);
        assert_eq!((l.buttons[0].0, l.buttons[0].1.x, l.zone[0].x), (QuickButton::Back, TEXTBOX.x, TEXTBOX.x), "no gap before Back");
    }

    #[test]
    fn a_label_wider_than_its_button_is_drawn_smaller_inside_it() {
        // the cell keeps the theme's size; the text shrinks to fit it
        let t = themed("quick_settings_size = 60, 32\nquick_back_size = 300, 32\n");
        let layout = quick_layout(TEXTBOX, &t, false, false, &sized);
        let mut out = Vec::new();
        draw_quick(&mut out, &t, &layout, &QuickLook::default(), &sized, 1.0);
        let size_of_label = |what: &str| out.iter().find_map(|d| match d {
            Draw::Text { text, size, rect, .. } if text == what => Some((*size, *rect)),
            _ => None,
        });
        let (size, rect) = size_of_label("Settings").unwrap();
        assert_eq!(rect, layout.rect(QuickButton::Settings).unwrap(), "drawn in the hit-test rect");
        assert!((10.0..24.0).contains(&size) && "Settings".len() as f32 * 0.55 * size <= rect.w, "{size}");
        assert_eq!(size_of_label("Back").unwrap().0, 24.0, "a roomy button keeps the normal size");
        assert_eq!(size_of_label("Q.Save").unwrap().0, 24.0);
    }

    fn images_of(list: &[Draw]) -> Vec<(&str, f32)> {
        list.iter().filter_map(|d| match d {
            Draw::Image { src, alpha, .. } => Some((src.as_str(), *alpha)),
            _ => None,
        }).collect()
    }

    fn drawn_quick(t: &Theme, inventory: bool, look: QuickLook, alpha: f32) -> Vec<Draw> {
        let layout = quick_layout(TEXTBOX, t, inventory, false, &sized);
        let mut out = Vec::new();
        draw_quick(&mut out, t, &layout, &look, &sized, alpha);
        out
    }

    fn labelled(list: &[Draw], what: &str) -> bool {
        list.iter().any(|d| matches!(d, Draw::Text { text, .. } if text == what))
    }

    #[test]
    fn a_picture_button_draws_its_picture_instead_of_its_text() {
        let t = themed("quick_back_icon = a.png\nquick_back_hover = h.png\nquick_skip_icon = s.png\nquick_skip_on = so.png\n\
                        quick_auto_icon = au.png\n");
        let list = drawn_quick(&t, false, QuickLook::default(), 1.0);
        assert!(!labelled(&list, "Back") && !labelled(&list, "Skip") && labelled(&list, "History"));
        assert_eq!(images_of(&list), vec![("a.png", 1.0), ("s.png", ICON_IDLE_ALPHA), ("au.png", ICON_IDLE_ALPHA)],
            "a button with a hover picture is whole when idle, without one it is a little dimmer");
        let dest = list.iter().find_map(|d| match d {
            Draw::Image { dest, uv, .. } => Some((*dest, *uv)),
            _ => None,
        });
        assert_eq!(dest, Some((Rect::new(dest.unwrap().0.x, dest.unwrap().0.y, 48.0, 48.0), FULL_UV)));
        let look = QuickLook { hover: Some(QuickButton::Back), ..QuickLook::default() };
        assert_eq!(images_of(&drawn_quick(&t, false, look, 1.0))[0], ("h.png", 1.0));
        let look = QuickLook { hover: Some(QuickButton::Skip), ..QuickLook::default() };
        assert_eq!(images_of(&drawn_quick(&t, false, look, 1.0))[1], ("s.png", 1.0), "no hover picture: lit");
        // the toggle's own picture, or the idle one with an accent bar under it
        let look = QuickLook { skipping: true, auto: true, ..QuickLook::default() };
        let list = drawn_quick(&t, false, look, 1.0);
        assert_eq!(images_of(&list)[1], ("so.png", 1.0));
        assert_eq!(images_of(&list)[2], ("au.png", 1.0));
        let bars = list.iter().filter(|d| matches!(d, Draw::Rect { color, .. } if *color == t.accent_color)).count();
        assert_eq!(bars, 1, "only Auto, which has no `on` picture");
        // fading and nothing at zero
        assert_eq!(images_of(&drawn_quick(&t, false, QuickLook::default(), 0.5))[0], ("a.png", 0.5));
        assert!(drawn_quick(&t, false, QuickLook::default(), 0.0).is_empty());
    }

    #[test]
    fn a_disabled_picture_is_dimmed_and_a_missing_one_falls_back_to_text() {
        let t = themed("quick_back_icon = a.png\nquick_back_hover = h.png\nquick_qload_icon = q.png\nquick_save_icon = gone.png\n");
        let look = QuickLook { back_off: true, qload_off: true, hover: Some(QuickButton::Back), ..QuickLook::default() };
        let list = drawn_quick(&t, false, look, 1.0);
        let imgs = images_of(&list);
        assert_eq!(imgs, vec![("a.png", ICON_DISABLED_ALPHA), ("q.png", ICON_DISABLED_ALPHA)], "no hover look on a disabled one");
        assert!(labelled(&list, "Save"), "`gone.png` isn't loaded: the text shows");
        // text buttons look as before when "disabled"
        let list = drawn_quick(&Theme::default(), true, look, 1.0);
        assert!(labelled(&list, "Back") && labelled(&list, "Q.Load") && labelled(&list, "Items"));
    }

    #[test]
    fn slot_screens_lay_out_and_hit_test() {
        let tabs = crate::slots::pages();
        for (i, tab) in tabs.iter().enumerate() {
            let r = tab_rect(i, tabs.len());
            assert!(r.x >= 0.0 && r.x + r.w <= VIRTUAL.w);
            let (x, y) = centre(r);
            assert_eq!(slots_at(x, y, &tabs, 6), Some(SlotHit::Tab(*tab)));
        }
        for i in 0..6 {
            let (x, y) = centre(slot_rect(i));
            assert_eq!(slots_at(x, y, &tabs, 6), Some(SlotHit::Slot(i)));
            let t = thumb_rect(i);
            assert!((t.w / t.h - 16.0 / 9.0).abs() < 1e-3);
        }
        assert!(slot_rect(5).y + slot_rect(5).h < slots_back_rect().y);
        let (x, y) = centre(slots_back_rect());
        assert_eq!(slots_at(x, y, &tabs, 6), Some(SlotHit::Back));
    }

    #[test]
    fn confirm_buttons() {
        let (x, y) = centre(confirm_button_rect(true));
        assert_eq!(confirm_at(x, y), Some(true));
        let (x, y) = centre(confirm_button_rect(false));
        assert_eq!(confirm_at(x, y), Some(false));
        assert_eq!(confirm_at(0.0, 0.0), None);
    }

    #[test]
    fn main_menu_buttons() {
        assert_eq!(menu_buttons(false, false).len(), 4);
        let with = menu_buttons(true, false);
        assert_eq!(with[1], MenuButton::Continue);
        for (i, b) in with.iter().enumerate() {
            let (x, y) = centre(menu_button_rect(i));
            assert_eq!(menu_at(x, y, &with), Some(*b));
        }
        assert!(menu_button_rect(with.len() - 1).y + 80.0 <= VIRTUAL.h);
    }

    #[test]
    fn settings_controls_and_buttons_are_found() {
        for language in [false, true] {
            let rows = rows(language);
            for i in 0..rows.len() {
                let (x, y) = centre(control_rect(i, rows, &Theme::default()));
                assert_eq!(settings_at(x, y, true, rows, &Theme::default()), Some(SettingsHit::Control(i)));
            }
            for in_story in [true, false] {
                for &b in settings_buttons(in_story) {
                    let (x, y) = centre(button_rect(b, in_story, rows.len(), &Theme::default()));
                    assert_eq!(settings_at(x, y, in_story, rows, &Theme::default()), Some(SettingsHit::Button(b)));
                }
            }
            let (x, y) = centre(button_rect(SettingsButton::MainMenu, true, rows.len(), &Theme::default()));
            assert_eq!(settings_at(x, y, false, rows, &Theme::default()), None, "no Main menu button at the main menu");
            let n = rows.len();
            assert!(button_rect(SettingsButton::Back, true, n, &Theme::default()).y > row_rect(n - 1, n, &Theme::default()).y);
            assert!(button_rect(SettingsButton::Back, true, n, &Theme::default()).y + 80.0 < 960.0, "clear of the hint line");
            assert!(row_rect(0, n, &Theme::default()).y >= 160.0, "clear of the title");
            assert_eq!(settings_at(5.0, 5.0, true, rows, &Theme::default()), None);
        }
    }

    #[test]
    fn slider_positions_follow_the_track() {
        let t = track_rect(2, 8, &Theme::default());
        assert_eq!(slider_pos_at(t.x, &Theme::default()), 0.0);
        assert_eq!(slider_pos_at(t.x + t.w, &Theme::default()), 1.0);
        assert!((slider_pos_at(t.x + t.w / 4.0, &Theme::default()) - 0.25).abs() < 1e-5);
        assert_eq!(slider_pos_at(-100.0, &Theme::default()), 0.0);
    }

    #[test]
    fn rows_read_and_write_settings() {
        let mut s = Settings::default();
        set_row_slider(&mut s, Row::Music, 0.3);
        assert_eq!(s.music_volume, 0.3);
        assert_eq!(row_value(&s, Row::Music, ""), "30%");
        set_row_slider(&mut s, Row::TextSpeed, 1.0);
        assert_eq!(row_value(&s, Row::TextSpeed, ""), "Instant");
        assert_eq!(row_slider(&s, Row::Fullscreen), None);
        assert_eq!(row_value(&s, Row::Fullscreen, ""), "Off");
    }

    fn line(t: &str) -> Logged {
        Logged::Line(Line { speaker: Some("Mika".into()), color: None, text: t.into() })
    }

    fn texts(draws: &[Draw]) -> Vec<String> {
        draws
            .iter()
            .filter_map(|d| match d {
                Draw::Text { text, .. } => Some(text.clone()),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn backlog_shows_newest_at_the_bottom_and_scrolls() {
        let entries = vec![line("one"), Logged::Choice("Go".into()), line("three")];
        let mut out = Vec::new();
        draw_backlog(&mut out, &Theme::default(), &entries, 0, &|_| None);
        let t = texts(&out);
        let pos = |s: &str| t.iter().position(|x| x == s).unwrap();
        assert!(pos("three") < pos("» Go") && pos("» Go") < pos("one")); // drawn newest first
        let y = |s: &str| {
            out.iter()
                .find_map(|d| match d {
                    Draw::Text { text, rect, .. } if text == s => Some(rect.y),
                    _ => None,
                })
                .unwrap()
        };
        assert!(y("one") < y("» Go") && y("» Go") < y("three"));

        let mut out = Vec::new();
        draw_backlog(&mut out, &Theme::default(), &entries, 1, &|_| None);
        assert!(!texts(&out).contains(&"three".to_string()));
        assert_eq!(backlog_max_scroll(&entries), 0, "all of it fits: nothing to scroll to");
    }

    #[test]
    fn a_paragraph_break_in_the_backlog_is_a_real_line_break_with_room_for_it() {
        let tall = vec![line("a{p}b{p}c")];
        let mut out = Vec::new();
        draw_backlog(&mut out, &Theme::default(), &tall, 0, &|_| None);
        let h = out.iter().find_map(|d| match d {
            Draw::Text { text, rect, .. } if text == "a\nb\nc" => Some(rect.h),
            _ => None,
        });
        let one = (LOG_SIZE * 1.35).round();
        assert_eq!(h, Some(3.0 * one), "three lines of room");
    }

    #[test]
    fn a_long_backlog_only_draws_what_fits() {
        let entries: Vec<Logged> = (0..100).map(|i| line(&format!("line {i}"))).collect();
        let mut out = Vec::new();
        draw_backlog(&mut out, &Theme::default(), &entries, 0, &|_| None);
        let shown = texts(&out).iter().filter(|t| t.starts_with("line ")).count();
        assert!(shown > 5 && shown < 30, "{shown}");
        assert!(out.iter().all(|d| match d {
            Draw::Text { rect, text, .. } if text.starts_with("line ") => rect.y >= LOG_TOP,
            _ => true,
        }));
    }

    #[test]
    fn scrolling_stops_when_the_oldest_entry_is_on_screen() {
        let entries: Vec<Logged> = (0..100).map(|i| line(&format!("line {i}"))).collect();
        let max = backlog_max_scroll(&entries);
        assert!(max > 50 && max < 99, "{max}: not all the way to a lone entry");
        let shown = |scroll: usize| {
            let mut out = Vec::new();
            draw_backlog(&mut out, &Theme::default(), &entries, scroll, &|_| None);
            texts(&out).iter().filter(|t| t.starts_with("line ")).count()
        };
        let mut out = Vec::new();
        draw_backlog(&mut out, &Theme::default(), &entries, max, &|_| None);
        assert!(texts(&out).contains(&"line 0".to_string()), "the oldest line is there at the end");
        assert!(shown(max) > 5, "and so is a page of others");
        assert_eq!(backlog_max_scroll(&entries[..3]), 0, "a short history has nothing to scroll");
    }

    #[test]
    fn a_long_speaker_name_is_smaller_so_that_it_fits_its_row() {
        let mut long = line("hello");
        if let Logged::Line(l) = &mut long {
            l.speaker = Some("Professor Bartholomew Fitzgerald-Montgomery".into());
        }
        let mut out = Vec::new();
        draw_backlog(&mut out, &Theme::default(), &[long], 0, &|_| None);
        let (rect, size) = out
            .iter()
            .find_map(|d| match d {
                Draw::Text { text, rect, size, .. } if text.starts_with("Professor") => Some((*rect, *size)),
                _ => None,
            })
            .unwrap();
        // Too long for one row even when smaller, so it wraps: the renderer may shrink it to 60% to fit.
        let rows = quillstone_render::estimate_lines("Professor Bartholomew Fitzgerald-Montgomery", size, rect.w);
        assert!(rows as f32 * (size * 0.6 * 1.35).round() <= rect.h, "{rows} rows of {size}px fit {}px", rect.h);
        assert!((20.0..LOG_SIZE).contains(&size));
    }

    #[test]
    fn a_slots_preview_is_cut_to_one_row_so_nothing_is_clipped() {
        let long = "Professor Bartholomew Fitzgerald-Montgomery: Line 30: a longer line from the professor";
        let cut = one_row(long, 22.0, 448.0);
        assert!(cut.ends_with('…') && cut.len() < long.len());
        assert_eq!(estimate_lines(&cut, 22.0, 448.0), 1);
        assert_eq!(one_row("Mika: Hi.", 22.0, 448.0), "Mika: Hi.");
    }

    #[test]
    fn a_settings_value_stays_inside_its_row_with_a_margin_and_shrinks_to_fit() {
        for i in 0..ROWS.len() {
            let (v, row) = (value_rect(i, 8, &Theme::default()), row_rect(i, 8, &Theme::default()));
            assert!(v.x + v.w <= row.x + row.w - 20.0 + 1e-3, "row {i}: value column ends before the row does");
            assert!(v.x >= track_x(&Theme::default()) + track_w(&Theme::default()), "row {i}: and starts after the track");
        }
        let (text, size) = fit_value("60 chars/s", 32.0, value_rect(0, 8, &Theme::default()).w);
        assert_eq!(estimate_lines(&text, size, value_rect(0, 8, &Theme::default()).w), 1);
        assert!(size >= 32.0 * 0.6 && text == "60 chars/s");
        let (text, size) = fit_value("999999999 characters per second", 32.0, value_rect(0, 8, &Theme::default()).w);
        assert!(text.ends_with('…') && estimate_lines(&text, size, value_rect(0, 8, &Theme::default()).w) == 1);
        let mut out = Vec::new();
        draw_settings(&mut out, &Theme::default(), &Settings::default(), true, 0, None, None, &|_| None);
        assert!(out.iter().any(|d| matches!(d, Draw::Text { rect, .. } if *rect == value_rect(0, 8, &Theme::default()))), "drawn in its column");
    }

    #[test]
    fn an_empty_backlog_says_so() {
        let mut out = Vec::new();
        draw_backlog(&mut out, &Theme::default(), &[], 0, &|_| None);
        assert!(texts(&out).contains(&"Nothing yet.".to_string()));
    }

    #[test]
    fn without_options_keys_the_settings_layout_is_the_old_fixed_one() {
        let t = Theme::default();
        assert_eq!(row_rect(0, 8, &t), Rect::new(400.0, 190.0, 1120.0, 60.0));
        assert_eq!(track_rect(0, 8, &t), Rect::new(880.0, 214.0, 440.0, 12.0));
        assert_eq!(value_rect(0, 8, &t), Rect::new(1340.0, 196.0, 160.0, 48.0));
        assert_eq!(control_rect(6, &ROWS, &t), Rect::new(880.0, 190.0 + 6.0 * 72.0 + 4.0, 160.0, 52.0));
        assert_eq!(button_rect(SettingsButton::Back, true, 8, &t), Rect::new(560.0, 826.0, 240.0, 80.0));
        assert_eq!(row_rect(0, 9, &t).y, 170.0, "nine rows are tighter");
    }

    #[test]
    fn a_short_settings_panel_shrinks_its_rows_so_the_buttons_stay_inside_it() {
        for rect in ["400, 660, 1120, 420", "0, 0, 640, 420", "100, 100, 700, 600"] {
            let t = Theme::parse(&format!("options_rect = {rect}\n")).0;
            let [x, y, w, h] = t.options_rect;
            for language in [false, true] {
                for &b in settings_buttons(true) {
                    let r = button_rect(b, true, rows(language).len(), &t);
                    assert!(r.x >= x && r.x + r.w <= x + w && r.y + r.h <= y + h, "{rect}: {b:?} {r:?}");
                }
            }
        }
    }

    #[test]
    fn a_moved_and_smaller_settings_panel_is_where_hits_and_drawing_happen() {
        let t = Theme::parse("options_rect = 80, 60, 800, 960\noptions_text_size = 28\n").0;
        assert!(Theme::parse("options_rect = 80, 60, 800, 960\noptions_text_size = 28\n").1.is_empty());
        for language in [false, true] {
            let rows = rows(language);
            for i in 0..rows.len() {
                let c = control_rect(i, rows, &t);
                assert!(c.x >= 80.0 && c.x + c.w <= 880.0 && c.y + c.h <= 1020.0, "row {i} inside the panel: {c:?}");
                let (x, y) = centre(c);
                assert_eq!(settings_at(x, y, true, rows, &t), Some(SettingsHit::Control(i)));
                let (ox, oy) = centre(control_rect(i, rows, &Theme::default()));
                assert_eq!(settings_at(ox, oy, true, rows, &t), None, "the old spot of row {i} is empty now");
            }
            for &b in settings_buttons(true) {
                let r = button_rect(b, true, rows.len(), &t);
                assert!(r.x >= 80.0 && r.x + r.w <= 880.0 && r.y + r.h <= 1020.0, "{b:?}: {r:?}");
                let (x, y) = centre(r);
                assert_eq!(settings_at(x, y, true, rows, &t), Some(SettingsHit::Button(b)));
            }
        }
        let track = track_rect(2, 8, &t);
        assert_eq!(slider_pos_at(track.x, &t), 0.0);
        assert!((slider_pos_at(track.x + track.w / 2.0, &t) - 0.5).abs() < 1e-5);
        let mut out = Vec::new();
        draw_settings(&mut out, &t, &Settings::default(), true, 1, None, None, &|_| None);
        assert!(out.iter().any(|d| matches!(d, Draw::Rect { rect, color } if *rect == row_rect(1, 8, &t) && *color == FOCUS)), "focus follows the row");
        let size = |text: &str| out.iter().find_map(|d| match d { Draw::Text { text: s, size, .. } if s == text => Some(*size), _ => None });
        assert_eq!(size("Back"), Some(28.0), "the buttons use the screen's text size");
    }

    #[test]
    fn the_settings_pictures_replace_the_overlay_and_dress_the_panel() {
        let t = Theme::parse("options_background = bg.png\noptions_panel_image = frame.png\noptions_panel_border = 20\n").0;
        let sized = |s: &str| (s == "bg.png").then(|| Size::new(1920.0, 1080.0)).or((s == "frame.png").then(|| Size::new(100.0, 100.0)));
        let draw = |t: &Theme, size: &dyn Fn(&str) -> Option<Size>| {
            let mut out = Vec::new();
            draw_settings(&mut out, t, &Settings::default(), true, 0, None, None, size);
            out
        };
        let (with, without) = (draw(&t, &sized), draw(&t, &|_| None));
        let images = |out: &[Draw]| out.iter().filter(|d| matches!(d, Draw::Image { .. })).count();
        assert_eq!((images(&with), images(&without)), (1 + 9, 0));
        assert!(!with.iter().any(|d| matches!(d, Draw::Rect { color, .. } if *color == t.overlay_color)), "the picture is the background");
        assert!(without.iter().any(|d| matches!(d, Draw::Rect { color, .. } if *color == t.overlay_color)), "not loaded: today's look");
    }

    #[test]
    fn the_menu_overlays_sit_over_the_background_and_under_what_the_screen_shows() {
        let t = Theme::parse("menu_background = bg.png\nmenu_overlay = ov.png\ngame_menu_overlay = gm.png\n").0;
        let sized = |_: &str| Some(Size::new(1920.0, 1080.0));
        let pos = |out: &[Draw], f: &dyn Fn(&Draw) -> bool| out.iter().position(f).unwrap();
        let image = |name: &'static str| move |d: &Draw| matches!(d, Draw::Image { src, .. } if src == name);
        let anything = |d: &Draw| matches!(d, Draw::Text { .. });

        let mut out = Vec::new();
        draw_main_menu(&mut out, &t, "T", &[MenuButton::Start], None, None, &sized);
        let button = pos(&out, &|d| matches!(d, Draw::Rect { rect, .. } if *rect == menu_button_rect(0)));
        assert!(pos(&out, &image("bg.png")) < pos(&out, &image("ov.png")) && pos(&out, &image("ov.png")) < pos(&out, &anything));
        assert!(pos(&out, &anything) < button);
        assert!(!out.iter().any(image("gm.png")), "the game menu's overlay is not the main menu's");

        let mut screens = Vec::new();
        let mut a = Vec::new();
        draw_settings(&mut a, &t, &Settings::default(), true, 0, None, None, &sized);
        screens.push(a);
        let mut b = Vec::new();
        draw_backlog(&mut b, &t, &[], 0, &sized);
        screens.push(b);
        let mut c = Vec::new();
        let view = SlotScreen { title: "Save", tabs: &[], page: Page::Auto, slots: &[], hover: None };
        draw_slots(&mut c, &t, &view, &sized);
        screens.push(c);
        for out in &screens {
            let dim = pos(out, &|d| matches!(d, Draw::Rect { color, .. } if *color == t.overlay_color));
            assert!(dim < pos(out, &image("gm.png")) && pos(out, &image("gm.png")) < pos(out, &anything));
            assert!(!out.iter().any(image("ov.png")));
        }
        let mut none = Vec::new();
        draw_backlog(&mut none, &t, &[], 0, &|_| None);
        assert!(!none.iter().any(image("gm.png")), "not loaded: today's look");
    }

    #[test]
    fn a_nine_slice_keeps_its_corners_and_covers_the_panel() {
        let mut out = Vec::new();
        let dest = Rect::new(100.0, 50.0, 400.0, 300.0);
        nine_slice(&mut out, "f.png", Size::new(80.0, 40.0), dest, 10.0);
        assert_eq!(out.len(), 9);
        let parts: Vec<(Rect, Rect)> = out.iter().map(|d| match d { Draw::Image { dest, uv, .. } => (*dest, *uv), _ => panic!() }).collect();
        assert_eq!(parts[0].0, Rect::new(100.0, 50.0, 10.0, 10.0), "top-left corner at its own size");
        assert_eq!(parts[0].1, Rect::new(0.0, 0.0, 10.0 / 80.0, 10.0 / 40.0));
        assert_eq!(parts[8].0, Rect::new(490.0, 340.0, 10.0, 10.0));
        assert_eq!(parts[4].0, Rect::new(110.0, 60.0, 380.0, 280.0), "the middle takes the rest");
        let area: f32 = parts.iter().map(|(d, _)| d.w * d.h).sum();
        assert!((area - dest.w * dest.h).abs() < 1e-2, "no gaps or overlaps");
        // a border larger than the picture or the panel is cut back, never inside out
        let mut out = Vec::new();
        nine_slice(&mut out, "f.png", Size::new(80.0, 40.0), Rect::new(0.0, 0.0, 30.0, 30.0), 500.0);
        assert!(out.iter().all(|d| matches!(d, Draw::Image { dest, uv, .. } if dest.w > 0.0 && dest.h > 0.0 && uv.w > 0.0 && uv.h > 0.0)));
    }

    #[test]
    fn the_game_tabs_are_hit_where_they_are_drawn_and_clear_every_screens_content() {
        for in_story in [true, false] {
            let tabs = game_tabs(in_story);
            let mut out = Vec::new();
            draw_game_tabs(&mut out, &Theme::default(), tabs, None, None);
            for (i, &tab) in tabs.iter().enumerate() {
                let r = game_tab_rect(i, tabs.len());
                assert!(out.iter().any(|d| matches!(d, Draw::Rect { rect, .. } if *rect == r)), "drawn at its rect");
                assert_eq!(game_tab_at(r.x + r.w / 2.0, r.y + r.h / 2.0, tabs), Some(tab));
                assert!(r.x >= 0.0 && r.x + r.w <= VIRTUAL.w);
                let next = tabs.get(i + 1).map(|_| game_tab_rect(i + 1, tabs.len()));
                assert!(next.is_none_or(|n| n.x >= r.x + r.w), "tabs do not overlap");
            }
            assert_eq!(game_tab_at(0.0, 0.0, tabs), None);
        }
        // above the slot grid's page tabs, the history's and the slot screens' titles, and the default settings panel
        let strip_bottom = game_tab_rect(0, 6).y + game_tab_rect(0, 6).h;
        let settings = Theme::default().options_rect;
        assert!(strip_bottom <= tab_rect(0, 3).y && strip_bottom <= 40.0 && strip_bottom <= settings[1]);
        assert!(!game_tabs(false).contains(&GameTab::Save) && !game_tabs(false).contains(&GameTab::MainMenu));
    }
}
