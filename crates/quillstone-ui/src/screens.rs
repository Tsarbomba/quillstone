//! Layout, hit-testing and drawing for the quick menu, settings and backlog.
//!
//! Pure functions over virtual coordinates (1920x1080), like the view's own
//! layout; [`crate::Game`] holds the state and decides which screen is up.

use quillstone_render::{estimate_lines, Align, Draw, Rect, Size, Theme, VIRTUAL};
use quillstone_runtime::Logged;
use quillstone_script::markup::plain;

use crate::settings::*;
use crate::slots::{thumbnail, Page, SlotInfo};

const FOCUS: [f32; 4] = [1.0, 1.0, 1.0, 0.06];
const TRACK: [f32; 4] = [0.35, 0.35, 0.42, 1.0];

fn text(out: &mut Vec<Draw>, s: impl Into<String>, rect: Rect, size: f32, color: [f32; 4], align: Align) {
    out.push(Draw::Text { text: s.into(), runs: Vec::new(), visible_chars: usize::MAX, rect, size, color, align });
}

fn screen() -> Rect {
    Rect::new(0.0, 0.0, VIRTUAL.w, VIRTUAL.h)
}

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
}

/// The buttons every game has. A game with an inventory also gets [`QuickButton::Items`].
pub const QUICK_BUTTONS: [QuickButton; 9] = [
    QuickButton::Back,
    QuickButton::History,
    QuickButton::Skip,
    QuickButton::Auto,
    QuickButton::Save,
    QuickButton::Load,
    QuickButton::QuickSave,
    QuickButton::QuickLoad,
    QuickButton::Settings,
];

const QUICK_W: f32 = 150.0;
const QUICK_GAP: f32 = 8.0;
const QUICK_H: f32 = 32.0;
/// Space between the textbox and the row below it.
const QUICK_MARGIN: f32 = 4.0;
/// Slots in the row: the buttons every game has, and Items to the left of them.
const QUICK_SLOTS: usize = QUICK_BUTTONS.len() + 1;

fn quick_total() -> f32 {
    QUICK_SLOTS as f32 * QUICK_W + (QUICK_SLOTS - 1) as f32 * QUICK_GAP
}

/// Where the row's top edge is: directly below the textbox, or, when there is no
/// room for it below, just inside the textbox's bottom edge.
fn quick_y(tb: Rect) -> f32 {
    let below = tb.y + tb.h + QUICK_MARGIN;
    if below + QUICK_H <= VIRTUAL.h {
        below
    } else {
        tb.y + tb.h - QUICK_MARGIN - QUICK_H
    }
}

/// The row's right edge: the textbox's, or further right when the textbox is too
/// narrow for the whole row (so no button leaves the screen).
fn quick_right(tb: Rect) -> f32 {
    (tb.x + tb.w).max(quick_total() + QUICK_GAP)
}

/// Right-aligned with the textbox `tb`, in the strip below it (see [`quick_y`]).
pub fn quick_rect(tb: Rect, i: usize) -> Rect {
    let n = QUICK_BUTTONS.len();
    let x = quick_right(tb) - (n - i) as f32 * QUICK_W - (n - 1 - i) as f32 * QUICK_GAP;
    Rect::new(x, quick_y(tb), QUICK_W, QUICK_H)
}

pub fn quick_button_rect(tb: Rect, b: QuickButton) -> Rect {
    quick_rect(tb, QUICK_BUTTONS.iter().position(|q| *q == b).unwrap_or(0))
}

/// The Items button: one place left of the first button, so no other button moves.
pub fn quick_items_rect(tb: Rect) -> Rect {
    let first = quick_rect(tb, 0);
    Rect::new(first.x - QUICK_GAP - QUICK_W, first.y, QUICK_W, first.h)
}

/// The quick menu button at a point. `inventory`: whether the game offers Items.
pub fn quick_at(tb: Rect, x: f32, y: f32, inventory: bool) -> Option<QuickButton> {
    if inventory && quick_items_rect(tb).contains(x, y) {
        return Some(QuickButton::Items);
    }
    (0..QUICK_BUTTONS.len()).find(|&i| quick_rect(tb, i).contains(x, y)).map(|i| QUICK_BUTTONS[i])
}

/// Whether a point is where the quick menu shows in its `hover` mode: over the
/// textbox, or in the strip of the row (the gap between them included).
pub fn quick_hover_zone(tb: Rect, x: f32, y: f32) -> bool {
    let right = quick_right(tb);
    let top = quick_y(tb).min(tb.y + tb.h);
    let strip = Rect::new(right - quick_total(), top, quick_total(), quick_y(tb) + QUICK_H - top);
    tb.contains(x, y) || strip.contains(x, y)
}

/// Draw the quick menu at opacity `alpha` (0..1: the hover mode fades it).
pub fn draw_quick(
    out: &mut Vec<Draw>,
    theme: &Theme,
    hover: Option<QuickButton>,
    auto: bool,
    skipping: bool,
    inventory: bool,
    alpha: f32,
) {
    if alpha <= 0.0 {
        return;
    }
    let tb = quillstone_render::textbox_rect(theme);
    let fade = |c: [f32; 4]| quillstone_render::faded(c, alpha);
    if inventory {
        let color = if hover == Some(QuickButton::Items) { theme.text_color } else { theme.dim_color };
        text(out, "Items", quick_items_rect(tb), 24.0, fade(color), Align::Center);
    }
    for (i, b) in QUICK_BUTTONS.iter().enumerate() {
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
        };
        let on = (*b == QuickButton::Auto && auto) || (*b == QuickButton::Skip && skipping);
        let color = if on {
            theme.accent_color
        } else if hover == Some(*b) {
            theme.text_color
        } else {
            theme.dim_color
        };
        text(out, label, quick_rect(tb, i), 24.0, fade(color), Align::Center);
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
/// time left (`left`, 0..1) filling it from the left.
pub fn draw_choice_timer(out: &mut Vec<Draw>, theme: &Theme, buttons: usize, left: f32) {
    let Some(first) = quillstone_render::choice_rects(theme, buttons).first().copied() else { return };
    let h = theme.choice_timer_height;
    let track = Rect::new(first.x, (first.y - 8.0 - h).max(0.0), first.w, h);
    out.push(Draw::Rect { rect: track, color: [0.0, 0.0, 0.0, 0.45] });
    let left = left.clamp(0.0, 1.0);
    if left > 0.0 {
        let color = theme.choice_timer_color.unwrap_or(theme.accent_color);
        out.push(Draw::Rect { rect: Rect::new(track.x, track.y, track.w * left, h), color });
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

/// The main menu: the theme's background picture (or black), the title, buttons.
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
    if let Some(src) = theme.menu_background.as_deref() {
        if let Some(size) = image_size(src) {
            let p = quillstone_render::place(size, screen(), quillstone_script::Fit::Cover);
            out.push(Draw::Image { src: src.to_string(), dest: p.dest, uv: p.uv, alpha: 1.0 });
        }
    }
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

/// Top and height of a row: tighter with nine rows so the buttons still clear the hint line.
fn row_y(n: usize) -> f32 {
    if n > 8 { 170.0 } else { 190.0 }
}

fn row_h(n: usize) -> f32 {
    if n > 8 { 66.0 } else { 72.0 }
}
const TRACK_X: f32 = 880.0;
const TRACK_W: f32 = 440.0;

pub fn row_rect(i: usize, n: usize) -> Rect {
    Rect::new(400.0, row_y(n) + i as f32 * row_h(n), 1120.0, row_h(n) - 12.0)
}

/// The visible slider track of row `i`.
pub fn track_rect(i: usize, n: usize) -> Rect {
    Rect::new(TRACK_X, row_rect(i, n).y + 24.0, TRACK_W, 12.0)
}

/// Where a row's control reacts to the mouse (taller than the thin track).
pub fn control_rect(i: usize, rows: &[Row]) -> Rect {
    let row = row_rect(i, rows.len());
    if rows[i].is_toggle() {
        let w = if matches!(rows[i], Row::QuickMenu | Row::Language) { 260.0 } else { 160.0 };
        Rect::new(TRACK_X, row.y + 4.0, w, 52.0)
    } else {
        Rect::new(TRACK_X - 12.0, row.y, TRACK_W + 24.0, row.h)
    }
}

/// Where button `b` sits; the buttons are centred side by side.
pub fn button_rect(b: SettingsButton, in_story: bool, n: usize) -> Rect {
    let buttons = settings_buttons(in_story);
    let i = buttons.iter().position(|x| *x == b).unwrap_or(0);
    let (w, gap) = (240.0, 40.0);
    let total = buttons.len() as f32 * w + (buttons.len() - 1) as f32 * gap;
    let y = row_y(n) + n as f32 * row_h(n) + 60.0;
    Rect::new((VIRTUAL.w - total) / 2.0 + i as f32 * (w + gap), y, w, 80.0)
}

pub fn settings_at(x: f32, y: f32, in_story: bool, rows: &[Row]) -> Option<SettingsHit> {
    if let Some(i) = (0..rows.len()).find(|&i| control_rect(i, rows).contains(x, y)) {
        return Some(SettingsHit::Control(i));
    }
    settings_buttons(in_story)
        .iter()
        .copied()
        .find(|&b| button_rect(b, in_story, rows.len()).contains(x, y))
        .map(SettingsHit::Button)
}

/// Slider position (0..1) for a mouse x over row `i`'s track.
pub fn slider_pos_at(x: f32) -> f32 {
    let t = track_rect(0, 8);
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
pub fn value_rect(i: usize, n: usize) -> Rect {
    let row = row_rect(i, n);
    let x = TRACK_X + TRACK_W + 20.0;
    Rect::new(x, row.y + 6.0, row.x + row.w - 20.0 - x, 48.0)
}

/// A value and the size to draw it at so that it fits one row `width` wide: stepped
/// down from `size` (not below 60%), then cut with an ellipsis.
fn fit_value(value: &str, size: f32, width: f32) -> (String, f32) {
    let mut s = size;
    while s > size * 0.6 && estimate_lines(value, s, width) > 1 {
        s -= 1.0;
    }
    (one_row(value, s, width), s)
}

pub fn draw_settings(
    out: &mut Vec<Draw>,
    theme: &Theme,
    s: &Settings,
    in_story: bool,
    focus: usize,
    hover: Option<SettingsHit>,
    language: Option<&str>,
) {
    let rows = rows(language.is_some());
    let language = language.unwrap_or("");
    out.push(Draw::Rect { rect: screen(), color: theme.overlay_color });
    text(out, "Settings", Rect::new(0.0, 90.0, VIRTUAL.w, 80.0), 56.0, theme.text_color, Align::Center);

    let n = rows.len();
    for (i, &row) in rows.iter().enumerate() {
        let r = row_rect(i, n);
        if i == focus {
            out.push(Draw::Rect { rect: r, color: FOCUS });
        }
        text(out, row_label(row), Rect::new(r.x + 30.0, r.y + 6.0, 440.0, 48.0), 36.0, theme.text_color, Align::Left);
        match row_slider(s, row) {
            Some(pos) => {
                let t = track_rect(i, n);
                out.push(Draw::Rect { rect: t, color: TRACK });
                let fill = [theme.button_hover_color[0], theme.button_hover_color[1], theme.button_hover_color[2], 1.0];
                out.push(Draw::Rect { rect: Rect::new(t.x, t.y, t.w * pos, t.h), color: fill });
                out.push(Draw::Rect { rect: Rect::new(t.x + t.w * pos - 10.0, r.y + 10.0, 20.0, 40.0), color: theme.text_color });
                let value = value_rect(i, n);
                let (shown, size) = fit_value(&row_value(s, row, language), 32.0, value.w);
                text(out, shown, value, size, theme.dim_color, Align::Left);
            }
            None => {
                let c = control_rect(i, rows);
                let color = if hover == Some(SettingsHit::Control(i)) { theme.button_hover_color } else { theme.button_color };
                out.push(Draw::Rect { rect: c, color });
                text(out, row_value(s, row, language), c, 32.0, theme.text_color, Align::Center);
            }
        }
    }

    for &b in settings_buttons(in_story) {
        let label = match b {
            SettingsButton::Back => "Back",
            SettingsButton::MainMenu => "Main menu",
            SettingsButton::Quit => "Quit",
        };
        let r = button_rect(b, in_story, n);
        let color = if hover == Some(SettingsHit::Button(b)) { theme.button_hover_color } else { theme.button_color };
        out.push(Draw::Rect { rect: r, color });
        text(out, label, r, 36.0, theme.text_color, Align::Center);
    }
    let hint = "Esc to close  ·  Up/Down to pick  ·  Left/Right to adjust";
    text(out, hint, Rect::new(0.0, 960.0, VIRTUAL.w, 40.0), 24.0, theme.dim_color, Align::Center);
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
    (LOG_SIZE * 1.35).round() // same line height the text renderer uses
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
pub fn draw_backlog(out: &mut Vec<Draw>, theme: &Theme, entries: &[Logged], scroll: usize) {
    out.push(Draw::Rect { rect: screen(), color: theme.overlay_color });
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

    #[test]
    fn quick_menu_sits_under_the_textbox_and_is_hit_testable() {
        for (i, b) in QUICK_BUTTONS.iter().enumerate() {
            let r = quick_rect(TEXTBOX, i);
            assert!(r.y >= TEXTBOX.y + TEXTBOX.h && r.y + r.h <= VIRTUAL.h);
            let (x, y) = centre(r);
            assert_eq!(quick_at(TEXTBOX, x, y, false), Some(*b));
            assert_eq!(quick_at(TEXTBOX, x, y, true), Some(*b), "Items doesn't move the others");
        }
        let items = quick_items_rect(TEXTBOX);
        let (x, y) = centre(items);
        assert_eq!(quick_at(TEXTBOX, x, y, true), Some(QuickButton::Items));
        assert_eq!(quick_at(TEXTBOX, x, y, false), None, "only games with an inventory have the button");
        assert!(items.x >= TEXTBOX.x && items.x + items.w <= quick_rect(TEXTBOX, 0).x, "Items fits under the textbox, left of Back");
        let last = quick_rect(TEXTBOX, QUICK_BUTTONS.len() - 1);
        assert!((last.x + QUICK_W - (TEXTBOX.x + TEXTBOX.w)).abs() < 1e-3);
        assert!(quick_rect(TEXTBOX, 0).x >= TEXTBOX.x, "all buttons fit under the textbox");
        assert_eq!(quick_at(TEXTBOX, 10.0, 10.0, true), None);
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
                let (x, y) = centre(control_rect(i, rows));
                assert_eq!(settings_at(x, y, true, rows), Some(SettingsHit::Control(i)));
            }
            for in_story in [true, false] {
                for &b in settings_buttons(in_story) {
                    let (x, y) = centre(button_rect(b, in_story, rows.len()));
                    assert_eq!(settings_at(x, y, in_story, rows), Some(SettingsHit::Button(b)));
                }
            }
            let (x, y) = centre(button_rect(SettingsButton::MainMenu, true, rows.len()));
            assert_eq!(settings_at(x, y, false, rows), None, "no Main menu button at the main menu");
            let n = rows.len();
            assert!(button_rect(SettingsButton::Back, true, n).y > row_rect(n - 1, n).y);
            assert!(button_rect(SettingsButton::Back, true, n).y + 80.0 < 960.0, "clear of the hint line");
            assert!(row_rect(0, n).y >= 160.0, "clear of the title");
            assert_eq!(settings_at(5.0, 5.0, true, rows), None);
        }
    }

    #[test]
    fn slider_positions_follow_the_track() {
        let t = track_rect(2, 8);
        assert_eq!(slider_pos_at(t.x), 0.0);
        assert_eq!(slider_pos_at(t.x + t.w), 1.0);
        assert!((slider_pos_at(t.x + t.w / 4.0) - 0.25).abs() < 1e-5);
        assert_eq!(slider_pos_at(-100.0), 0.0);
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
        draw_backlog(&mut out, &Theme::default(), &entries, 0);
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
        draw_backlog(&mut out, &Theme::default(), &entries, 1);
        assert!(!texts(&out).contains(&"three".to_string()));
        assert_eq!(backlog_max_scroll(&entries), 0, "all of it fits: nothing to scroll to");
    }

    #[test]
    fn a_paragraph_break_in_the_backlog_is_a_real_line_break_with_room_for_it() {
        let tall = vec![line("a{p}b{p}c")];
        let mut out = Vec::new();
        draw_backlog(&mut out, &Theme::default(), &tall, 0);
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
        draw_backlog(&mut out, &Theme::default(), &entries, 0);
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
            draw_backlog(&mut out, &Theme::default(), &entries, scroll);
            texts(&out).iter().filter(|t| t.starts_with("line ")).count()
        };
        let mut out = Vec::new();
        draw_backlog(&mut out, &Theme::default(), &entries, max);
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
        draw_backlog(&mut out, &Theme::default(), &[long], 0);
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
            let (v, row) = (value_rect(i, 8), row_rect(i, 8));
            assert!(v.x + v.w <= row.x + row.w - 20.0 + 1e-3, "row {i}: value column ends before the row does");
            assert!(v.x >= TRACK_X + TRACK_W, "row {i}: and starts after the track");
        }
        let (text, size) = fit_value("60 chars/s", 32.0, value_rect(0, 8).w);
        assert_eq!(estimate_lines(&text, size, value_rect(0, 8).w), 1);
        assert!(size >= 32.0 * 0.6 && text == "60 chars/s");
        let (text, size) = fit_value("999999999 characters per second", 32.0, value_rect(0, 8).w);
        assert!(text.ends_with('…') && estimate_lines(&text, size, value_rect(0, 8).w) == 1);
        let mut out = Vec::new();
        draw_settings(&mut out, &Theme::default(), &Settings::default(), true, 0, None, None);
        assert!(out.iter().any(|d| matches!(d, Draw::Text { rect, .. } if *rect == value_rect(0, 8))), "drawn in its column");
    }

    #[test]
    fn an_empty_backlog_says_so() {
        let mut out = Vec::new();
        draw_backlog(&mut out, &Theme::default(), &[], 0);
        assert!(texts(&out).contains(&"Nothing yet.".to_string()));
    }
}
