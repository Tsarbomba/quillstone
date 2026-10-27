//! Layout, hit-testing and drawing for the quick menu, settings and backlog.
//!
//! Pure functions over virtual coordinates (1920x1080), like the view's own
//! layout; [`crate::Game`] holds the state and decides which screen is up.

use quillstone_render::{estimate_lines, Align, Draw, Rect, Size, Theme, TEXTBOX, VIRTUAL};
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
}

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

/// Right-aligned with the textbox, in the strip below it.
pub fn quick_rect(i: usize) -> Rect {
    let n = QUICK_BUTTONS.len();
    let right = TEXTBOX.x + TEXTBOX.w;
    let x = right - (n - i) as f32 * QUICK_W - (n - 1 - i) as f32 * QUICK_GAP;
    Rect::new(x, TEXTBOX.y + TEXTBOX.h + 4.0, QUICK_W, 32.0)
}

pub fn quick_button_rect(b: QuickButton) -> Rect {
    quick_rect(QUICK_BUTTONS.iter().position(|q| *q == b).unwrap_or(0))
}

pub fn quick_at(x: f32, y: f32) -> Option<QuickButton> {
    (0..QUICK_BUTTONS.len()).find(|&i| quick_rect(i).contains(x, y)).map(|i| QUICK_BUTTONS[i])
}

pub fn draw_quick(out: &mut Vec<Draw>, theme: &Theme, hover: Option<QuickButton>, auto: bool, skipping: bool) {
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
        };
        let on = (*b == QuickButton::Auto && auto) || (*b == QuickButton::Skip && skipping);
        let color = if on {
            theme.accent_color
        } else if hover == Some(*b) {
            theme.text_color
        } else {
            theme.dim_color
        };
        text(out, label, quick_rect(i), 24.0, color, Align::Center);
    }
}

/// "Skipping" in the corner while skip mode runs.
pub fn draw_skip_indicator(out: &mut Vec<Draw>, theme: &Theme) {
    text(out, "Skipping »", Rect::new(40.0, 30.0, 400.0, 50.0), 30.0, theme.accent_color, Align::Left);
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
    text(out, title, Rect::new(0.0, 180.0, VIRTUAL.w, 160.0), 96.0, theme.accent_color, Align::Center);
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
}

pub const ROWS: [Row; 7] =
    [Row::TextSpeed, Row::AutoDelay, Row::SkipUnread, Row::Music, Row::Sfx, Row::Voice, Row::Fullscreen];

impl Row {
    /// On/off rows (a button) rather than sliders.
    pub fn is_toggle(self) -> bool {
        matches!(self, Row::SkipUnread | Row::Fullscreen)
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

const ROW_Y: f32 = 220.0;
const ROW_H: f32 = 80.0;
const TRACK_X: f32 = 880.0;
const TRACK_W: f32 = 440.0;

pub fn row_rect(i: usize) -> Rect {
    Rect::new(400.0, ROW_Y + i as f32 * ROW_H, 1120.0, ROW_H - 12.0)
}

/// The visible slider track of row `i`.
pub fn track_rect(i: usize) -> Rect {
    Rect::new(TRACK_X, row_rect(i).y + 32.0, TRACK_W, 12.0)
}

/// Where a row's control reacts to the mouse (taller than the thin track).
pub fn control_rect(i: usize) -> Rect {
    let row = row_rect(i);
    if ROWS[i].is_toggle() {
        Rect::new(TRACK_X, row.y + 8.0, 160.0, 52.0)
    } else {
        Rect::new(TRACK_X - 12.0, row.y, TRACK_W + 24.0, row.h)
    }
}

/// Where button `b` sits; the buttons are centred side by side.
pub fn button_rect(b: SettingsButton, in_story: bool) -> Rect {
    let buttons = settings_buttons(in_story);
    let i = buttons.iter().position(|x| *x == b).unwrap_or(0);
    let (w, gap) = (240.0, 40.0);
    let total = buttons.len() as f32 * w + (buttons.len() - 1) as f32 * gap;
    let y = ROW_Y + ROWS.len() as f32 * ROW_H + 60.0;
    Rect::new((VIRTUAL.w - total) / 2.0 + i as f32 * (w + gap), y, w, 80.0)
}

pub fn settings_at(x: f32, y: f32, in_story: bool) -> Option<SettingsHit> {
    if let Some(i) = (0..ROWS.len()).find(|&i| control_rect(i).contains(x, y)) {
        return Some(SettingsHit::Control(i));
    }
    settings_buttons(in_story)
        .iter()
        .copied()
        .find(|&b| button_rect(b, in_story).contains(x, y))
        .map(SettingsHit::Button)
}

/// Slider position (0..1) for a mouse x over row `i`'s track.
pub fn slider_pos_at(i: usize, x: f32) -> f32 {
    let t = track_rect(i);
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
        Row::SkipUnread | Row::Fullscreen => None,
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
        Row::SkipUnread | Row::Fullscreen => {}
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
    }
}

pub fn row_value(s: &Settings, row: Row) -> String {
    match row {
        Row::TextSpeed if s.text_speed <= 0.0 => "Instant".into(),
        Row::TextSpeed => format!("{} chars/s", s.text_speed.round()),
        Row::AutoDelay => format!("{:.1} s", s.auto_delay),
        Row::Music => format!("{}%", (s.music_volume * 100.0).round()),
        Row::Sfx => format!("{}%", (s.sfx_volume * 100.0).round()),
        Row::Voice => format!("{}%", (s.voice_volume * 100.0).round()),
        Row::SkipUnread => if s.skip_unread { "On" } else { "Off" }.into(),
        Row::Fullscreen => if s.fullscreen { "On" } else { "Off" }.into(),
    }
}

pub fn draw_settings(
    out: &mut Vec<Draw>,
    theme: &Theme,
    s: &Settings,
    in_story: bool,
    focus: usize,
    hover: Option<SettingsHit>,
) {
    out.push(Draw::Rect { rect: screen(), color: theme.overlay_color });
    text(out, "Settings", Rect::new(0.0, 90.0, VIRTUAL.w, 80.0), 56.0, theme.text_color, Align::Center);

    for (i, &row) in ROWS.iter().enumerate() {
        let r = row_rect(i);
        if i == focus {
            out.push(Draw::Rect { rect: r, color: FOCUS });
        }
        text(out, row_label(row), Rect::new(r.x + 30.0, r.y + 14.0, 440.0, 48.0), 36.0, theme.text_color, Align::Left);
        match row_slider(s, row) {
            Some(pos) => {
                let t = track_rect(i);
                out.push(Draw::Rect { rect: t, color: TRACK });
                let fill = [theme.button_hover_color[0], theme.button_hover_color[1], theme.button_hover_color[2], 1.0];
                out.push(Draw::Rect { rect: Rect::new(t.x, t.y, t.w * pos, t.h), color: fill });
                out.push(Draw::Rect { rect: Rect::new(t.x + t.w * pos - 10.0, r.y + 18.0, 20.0, 40.0), color: theme.text_color });
                let value = Rect::new(TRACK_X + TRACK_W + 30.0, r.y + 14.0, 200.0, 48.0);
                text(out, row_value(s, row), value, 32.0, theme.dim_color, Align::Left);
            }
            None => {
                let c = control_rect(i);
                let color = if hover == Some(SettingsHit::Control(i)) { theme.button_hover_color } else { theme.button_color };
                out.push(Draw::Rect { rect: c, color });
                text(out, row_value(s, row), c, 32.0, theme.text_color, Align::Center);
            }
        }
    }

    for &b in settings_buttons(in_story) {
        let label = match b {
            SettingsButton::Back => "Back",
            SettingsButton::MainMenu => "Main menu",
            SettingsButton::Quit => "Quit",
        };
        let r = button_rect(b, in_story);
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

/// How far back the backlog can scroll (in entries) without running out.
pub fn backlog_max_scroll(entries: &[Logged]) -> usize {
    entries.len().saturating_sub(1)
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

    let line_h = (LOG_SIZE * 1.35).round(); // same line height the text renderer uses
    let mut bottom = LOG_BOTTOM;
    for entry in entries.iter().rev().skip(scroll) {
        let (speaker, color, body, body_color) = match entry {
            Logged::Line(line) => {
                let color = line.color.map(|[r, g, b]| [r, g, b, 1.0]).unwrap_or(theme.name_color);
                (line.speaker.as_deref(), color, plain(&line.text), theme.text_color)
            }
            Logged::Choice(choice) => (None, theme.accent_color, format!("» {}", plain(choice)), theme.accent_color),
        };
        let h = estimate_lines(&body, LOG_SIZE, TEXT_W) as f32 * line_h;
        let top = bottom - h;
        if top < LOG_TOP {
            break; // only whole entries are drawn
        }
        if let Some(name) = speaker {
            text(out, name, Rect::new(NAME_X, top, NAME_W, line_h), LOG_SIZE, color, Align::Left);
        }
        text(out, body, Rect::new(TEXT_X, top, TEXT_W, h), LOG_SIZE, body_color, Align::Left);
        bottom = top - LOG_GAP;
    }
}

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
                text(out, info.preview.clone(), below(32.0, 30.0), 22.0, theme.text_color, Align::Left);
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
    out.push(Draw::Rect { rect: r, color: [0.0, 0.0, 0.0, 0.8] });
    text(out, message, r, 28.0, theme.text_color, Align::Center);
}

#[cfg(test)]
mod tests {
    use super::*;
    use quillstone_runtime::Line;

    fn centre(r: Rect) -> (f32, f32) {
        (r.x + r.w / 2.0, r.y + r.h / 2.0)
    }

    #[test]
    fn quick_menu_sits_under_the_textbox_and_is_hit_testable() {
        for (i, b) in QUICK_BUTTONS.iter().enumerate() {
            let r = quick_rect(i);
            assert!(r.y >= TEXTBOX.y + TEXTBOX.h && r.y + r.h <= VIRTUAL.h);
            let (x, y) = centre(r);
            assert_eq!(quick_at(x, y), Some(*b));
        }
        let last = quick_rect(QUICK_BUTTONS.len() - 1);
        assert!((last.x + QUICK_W - (TEXTBOX.x + TEXTBOX.w)).abs() < 1e-3);
        assert!(quick_rect(0).x >= TEXTBOX.x, "all buttons fit under the textbox");
        assert_eq!(quick_at(10.0, 10.0), None);
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
        for i in 0..ROWS.len() {
            let (x, y) = centre(control_rect(i));
            assert_eq!(settings_at(x, y, true), Some(SettingsHit::Control(i)));
        }
        for in_story in [true, false] {
            for &b in settings_buttons(in_story) {
                let (x, y) = centre(button_rect(b, in_story));
                assert_eq!(settings_at(x, y, in_story), Some(SettingsHit::Button(b)));
            }
        }
        let (x, y) = centre(button_rect(SettingsButton::MainMenu, true));
        assert_eq!(settings_at(x, y, false), None, "no Main menu button at the main menu");
        assert!(button_rect(SettingsButton::Back, true).y > row_rect(ROWS.len() - 1).y);
        assert!(button_rect(SettingsButton::Back, true).y + 80.0 < 960.0, "clear of the hint line");
        assert_eq!(settings_at(5.0, 5.0, true), None);
    }

    #[test]
    fn slider_positions_follow_the_track() {
        let t = track_rect(2);
        assert_eq!(slider_pos_at(2, t.x), 0.0);
        assert_eq!(slider_pos_at(2, t.x + t.w), 1.0);
        assert!((slider_pos_at(2, t.x + t.w / 4.0) - 0.25).abs() < 1e-5);
        assert_eq!(slider_pos_at(2, -100.0), 0.0);
    }

    #[test]
    fn rows_read_and_write_settings() {
        let mut s = Settings::default();
        set_row_slider(&mut s, Row::Music, 0.3);
        assert_eq!(s.music_volume, 0.3);
        assert_eq!(row_value(&s, Row::Music), "30%");
        set_row_slider(&mut s, Row::TextSpeed, 1.0);
        assert_eq!(row_value(&s, Row::TextSpeed), "Instant");
        assert_eq!(row_slider(&s, Row::Fullscreen), None);
        assert_eq!(row_value(&s, Row::Fullscreen), "Off");
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
        assert_eq!(backlog_max_scroll(&entries), 2);
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
    fn an_empty_backlog_says_so() {
        let mut out = Vec::new();
        draw_backlog(&mut out, &Theme::default(), &[], 0);
        assert!(texts(&out).contains(&"Nothing yet.".to_string()));
    }
}
