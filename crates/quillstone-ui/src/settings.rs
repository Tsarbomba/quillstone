//! Player preferences, and the small text file they are kept in.
//!
//! The file is `key = value` lines. Unknown keys and bad values are ignored (the
//! default is used), so an old or hand-edited file never stops the game.

use quillstone_render::{QuickMenu, VIRTUAL};

/// The player's say over the quick menu (see [`quillstone_render::QuickMenu`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum QuickMenuSetting {
    /// Whatever the game's theme says.
    #[default]
    Default,
    Visible,
    Hover,
    Hidden,
}

impl QuickMenuSetting {
    pub const ALL: [QuickMenuSetting; 4] =
        [QuickMenuSetting::Default, QuickMenuSetting::Visible, QuickMenuSetting::Hover, QuickMenuSetting::Hidden];

    /// The mode to use, given the theme's.
    pub fn resolve(self, theme: QuickMenu) -> QuickMenu {
        match self {
            QuickMenuSetting::Default => theme,
            QuickMenuSetting::Visible => QuickMenu::Visible,
            QuickMenuSetting::Hover => QuickMenu::Hover,
            QuickMenuSetting::Hidden => QuickMenu::Hidden,
        }
    }

    /// The next one in the list (wrapping), or the previous with `back`.
    pub fn cycled(self, back: bool) -> QuickMenuSetting {
        let n = Self::ALL.len();
        let i = Self::ALL.iter().position(|s| *s == self).unwrap_or(0);
        Self::ALL[(i + if back { n - 1 } else { 1 }) % n]
    }

    pub fn name(self) -> &'static str {
        match self {
            QuickMenuSetting::Default => "default",
            QuickMenuSetting::Visible => "visible",
            QuickMenuSetting::Hover => "hover",
            QuickMenuSetting::Hidden => "hidden",
        }
    }

    /// Unknown names are `None` (the caller keeps the default).
    pub fn from_name(name: &str) -> Option<QuickMenuSetting> {
        Self::ALL.into_iter().find(|s| s.name() == name)
    }
}

/// Everything the settings screen can change.
#[derive(Debug, Clone, PartialEq)]
pub struct Settings {
    /// Typewriter speed in characters per second; `0` shows lines instantly.
    pub text_speed: f32,
    /// Auto-forward pause after a line is fully shown, in seconds.
    pub auto_delay: f32,
    /// Volume slider positions, 0..1.
    pub music_volume: f32,
    pub sfx_volume: f32,
    pub voice_volume: f32,
    pub fullscreen: bool,
    /// Whether skip mode also skips lines the player hasn't read yet.
    pub skip_unread: bool,
    /// When the quick menu under the textbox shows.
    pub quick_menu: QuickMenuSetting,
    /// The language chosen in Settings (a code from the script's `languages:`); `None` is the
    /// script's own. Kept as text: a code the game doesn't offer (an old file) is simply ignored.
    pub language: Option<String>,
    /// The window's size and outer position (physical pixels) when the player last closed it,
    /// not full screen. `None` until then: see [`window_placement`].
    pub window_size: Option<(u32, u32)>,
    pub window_pos: Option<(i32, i32)>,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            text_speed: 60.0,
            auto_delay: 1.5,
            music_volume: 1.0,
            sfx_volume: 1.0,
            voice_volume: 1.0,
            fullscreen: false,
            skip_unread: false,
            quick_menu: QuickMenuSetting::Default,
            language: None,
            window_size: None,
            window_pos: None,
        }
    }
}

pub const MIN_TEXT_SPEED: f32 = 10.0;
pub const MAX_TEXT_SPEED: f32 = 200.0;
pub const MIN_AUTO_DELAY: f32 = 0.5;
pub const MAX_AUTO_DELAY: f32 = 6.0;

impl Settings {
    /// Read a settings file. Anything missing or unreadable keeps its default.
    pub fn parse(text: &str) -> Settings {
        let mut s = Settings::default();
        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let Some((key, value)) = line.split_once('=') else { continue };
            let value = value.trim();
            let num = || value.parse::<f32>().ok().filter(|v| v.is_finite());
            match key.trim() {
                "text_speed" => {
                    if let Some(v) = num() {
                        s.text_speed = if v <= 0.0 { 0.0 } else { v.clamp(MIN_TEXT_SPEED, MAX_TEXT_SPEED) };
                    }
                }
                "auto_delay" => {
                    if let Some(v) = num() {
                        s.auto_delay = v.clamp(MIN_AUTO_DELAY, MAX_AUTO_DELAY);
                    }
                }
                "music_volume" => s.music_volume = num().map_or(s.music_volume, |v| v.clamp(0.0, 1.0)),
                "sfx_volume" => s.sfx_volume = num().map_or(s.sfx_volume, |v| v.clamp(0.0, 1.0)),
                "voice_volume" => s.voice_volume = num().map_or(s.voice_volume, |v| v.clamp(0.0, 1.0)),
                "fullscreen" => s.fullscreen = value.parse().unwrap_or(s.fullscreen),
                "skip_unread" => s.skip_unread = value.parse().unwrap_or(s.skip_unread),
                "quick_menu" => s.quick_menu = QuickMenuSetting::from_name(value).unwrap_or_default(),
                "language" => {
                    let ok = !value.is_empty() && value.len() <= 16 && value.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_'));
                    s.language = ok.then(|| value.to_string());
                }
                "window_size" => {
                    let size = value.split_once('x').and_then(|(w, h)| Some((w.trim().parse().ok()?, h.trim().parse().ok()?)));
                    s.window_size = size.filter(|&(w, h)| (MIN_WINDOW.0..=MAX_WINDOW).contains(&w) && (MIN_WINDOW.1..=MAX_WINDOW).contains(&h));
                }
                "window_pos" => {
                    let pos = value.split_once(',').and_then(|(x, y)| Some((x.trim().parse().ok()?, y.trim().parse().ok()?)));
                    s.window_pos = pos.filter(|&(x, y): &(i32, i32)| x.abs() <= MAX_WINDOW as i32 && y.abs() <= MAX_WINDOW as i32);
                }
                _ => {}
            }
        }
        s
    }

    /// The settings as file text; [`Settings::parse`] reads it back unchanged.
    pub fn to_text(&self) -> String {
        let language = self.language.as_ref().map(|l| format!("language = {l}\n")).unwrap_or_default();
        let size = self.window_size.map(|(w, h)| format!("window_size = {w}x{h}\n")).unwrap_or_default();
        let pos = self.window_pos.map(|(x, y)| format!("window_pos = {x},{y}\n")).unwrap_or_default();
        format!(
            "# Quillstone settings\n\
             text_speed = {}\n\
             auto_delay = {}\n\
             music_volume = {}\n\
             sfx_volume = {}\n\
             voice_volume = {}\n\
             fullscreen = {}\n\
             skip_unread = {}\n\
             quick_menu = {}\n{}{}{}",
            self.text_speed,
            self.auto_delay,
            self.music_volume,
            self.sfx_volume,
            self.voice_volume,
            self.fullscreen,
            self.skip_unread,
            self.quick_menu.name(),
            language,
            size,
            pos
        )
    }
}

/// The smallest window a saved size may restore to, and the largest side a settings file may claim.
const MIN_WINDOW: (u32, u32) = (320, 180);
const MAX_WINDOW: u32 = 16384;
/// Room kept for the title bar and the taskbar (the monitor's size is all the OS tells us).
const FRAME_ROOM: u32 = 48;

/// A monitor's rectangle in physical pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Area {
    pub x: i32,
    pub y: i32,
    pub w: u32,
    pub h: u32,
}

/// Where and how big to open the window (inner size, outer position, physical pixels). `monitors`: the
/// primary one first. First launch (nothing `saved`): the game's own 1920x1080, or the largest 16:9 size that
/// fits the monitor with a margin, centred. Later: the saved size, shrunk to fit the monitor it was on (or the
/// primary one, when that is gone) and moved so the whole window is on it.
pub fn window_placement(size: Option<(u32, u32)>, pos: Option<(i32, i32)>, monitors: &[Area]) -> ((u32, u32), (i32, i32)) {
    let primary = monitors.first().copied().unwrap_or(Area { x: 0, y: 0, w: 1920, h: 1080 });
    let Some((w, h)) = size else {
        // 90% of the width and 85% of the height, at 16:9, never above the game's own size
        let unit = (primary.w * 90 / 100 / 16).min(primary.h * 85 / 100 / 9).min(VIRTUAL.w as u32 / 16).max(1);
        let (w, h) = (unit * 16, unit * 9);
        return ((w, h), (primary.x + (primary.w - w) as i32 / 2, primary.y + (primary.h - h) as i32 / 2));
    };
    let on = pos
        .and_then(|(x, y)| {
            let (cx, cy) = (x as i64 + w as i64 / 2, y as i64 + h as i64 / 2);
            let inside = |m: &&Area| (m.x as i64..m.x as i64 + m.w as i64).contains(&cx) && (m.y as i64..m.y as i64 + m.h as i64).contains(&cy);
            monitors.iter().find(inside)
        })
        .copied()
        .unwrap_or(primary);
    let (min_h, max_h) = (MIN_WINDOW.1.min(on.h), on.h.saturating_sub(FRAME_ROOM));
    let size = (w.clamp(MIN_WINDOW.0.min(on.w), on.w), h.clamp(min_h, max_h.max(min_h)));
    let (x, y) = pos.unwrap_or((on.x + (on.w - size.0) as i32 / 2, on.y + (on.h - size.1) as i32 / 2));
    let x = x.clamp(on.x, on.x + (on.w - size.0) as i32);
    let y = y.clamp(on.y, on.y + (on.h - (size.1 + FRAME_ROOM).min(on.h)) as i32);
    (size, (x, y))
}

// ---- slider mappings (position 0..1 <-> value) ---------------------------------

/// Text speed slider: left is slow, the far right is "instant".
pub fn text_speed_from_slider(pos: f32) -> f32 {
    let pos = pos.clamp(0.0, 1.0);
    if pos >= 0.99 {
        return 0.0;
    }
    let cps = MIN_TEXT_SPEED + pos * (MAX_TEXT_SPEED - MIN_TEXT_SPEED);
    (cps / 5.0).round() * 5.0
}

pub fn text_speed_to_slider(cps: f32) -> f32 {
    if cps <= 0.0 {
        1.0
    } else {
        ((cps - MIN_TEXT_SPEED) / (MAX_TEXT_SPEED - MIN_TEXT_SPEED)).clamp(0.0, 0.98)
    }
}

pub fn auto_delay_from_slider(pos: f32) -> f32 {
    let d = MIN_AUTO_DELAY + pos.clamp(0.0, 1.0) * (MAX_AUTO_DELAY - MIN_AUTO_DELAY);
    (d * 10.0).round() / 10.0
}

pub fn auto_delay_to_slider(delay: f32) -> f32 {
    ((delay - MIN_AUTO_DELAY) / (MAX_AUTO_DELAY - MIN_AUTO_DELAY)).clamp(0.0, 1.0)
}

/// Volume sliders are stored as their position, rounded to whole percent.
pub fn volume_from_slider(pos: f32) -> f32 {
    (pos.clamp(0.0, 1.0) * 100.0).round() / 100.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_file_round_trips() {
        let s = Settings {
            text_speed: 0.0,
            auto_delay: 2.5,
            music_volume: 0.25,
            sfx_volume: 0.0,
            voice_volume: 0.75,
            fullscreen: true,
            skip_unread: true,
            quick_menu: QuickMenuSetting::Hover,
            language: Some("sv".into()),
            window_size: Some((1280, 720)),
            window_pos: Some((-30, 40)),
        };
        assert_eq!(Settings::parse(&s.to_text()), s);
        assert_eq!(Settings::parse(&Settings::default().to_text()), Settings::default());
    }

    #[test]
    fn bad_unknown_and_missing_entries_keep_the_defaults() {
        let s = Settings::parse("music_volume = loud\nwhat = 3\nno equals sign\n  sfx_volume=0.5  \nfullscreen = yes\n");
        assert_eq!(s.music_volume, 1.0);
        assert_eq!(s.sfx_volume, 0.5);
        assert!(!s.fullscreen);
        assert_eq!(Settings::parse(""), Settings::default());
        assert_eq!(Settings::parse("text_speed = NaN\n").text_speed, 60.0);
    }

    #[test]
    fn the_quick_menu_setting_reads_names_and_defaults_otherwise() {
        for s in QuickMenuSetting::ALL {
            assert_eq!(Settings::parse(&format!("quick_menu = {}\n", s.name())).quick_menu, s);
        }
        for bad in ["quick_menu = sometimes\n", "quick_menu =\n", "quick_menu = HOVER\n", "music_volume = 1\n", ""] {
            assert_eq!(Settings::parse(bad).quick_menu, QuickMenuSetting::Default, "{bad:?}");
        }
        // a file from before the setting existed
        let old = "text_speed = 40\nauto_delay = 2\nmusic_volume = 0.5\nsfx_volume = 1\nvoice_volume = 1\nfullscreen = true\nskip_unread = false\n";
        let s = Settings::parse(old);
        assert_eq!((s.quick_menu, s.text_speed, s.fullscreen), (QuickMenuSetting::Default, 40.0, true));
    }

    #[test]
    fn the_window_setting_reads_back_and_a_bad_one_is_dropped() {
        let s = Settings::parse("window_size = 1280x720\nwindow_pos = -5, 30\n");
        assert_eq!((s.window_size, s.window_pos), (Some((1280, 720)), Some((-5, 30))));
        for bad in ["window_size = 10x10\n", "window_size = 99999x720\n", "window_size = wide\n", "window_size = 1280\n"] {
            assert_eq!(Settings::parse(bad).window_size, None, "{bad:?}");
        }
        for bad in ["window_pos = 1\n", "window_pos = a,b\n", "window_pos = 9999999,0\n"] {
            assert_eq!(Settings::parse(bad).window_pos, None, "{bad:?}");
        }
        assert!(!Settings::default().to_text().contains("window"), "nothing written before a window was closed");
    }

    #[test]
    fn the_first_window_is_the_games_size_or_the_biggest_16_9_that_fits() {
        let area = |w, h| Area { x: 0, y: 0, w, h };
        assert_eq!(window_placement(None, None, &[area(2560, 1440)]), ((1920, 1080), (320, 180)));
        assert_eq!(window_placement(None, None, &[area(3840, 2160)]), ((1920, 1080), (960, 540)), "never above 1920x1080");
        let ((w, h), (x, y)) = window_placement(None, None, &[area(1920, 1080)]);
        assert_eq!((w * 9, x, y), (h * 16, (1920 - w as i32) / 2, (1080 - h as i32) / 2));
        assert!(w <= 1728 && h <= 918, "a margin for the taskbar and the title bar: {w}x{h}");
        let ((w, h), _) = window_placement(None, None, &[area(1280, 1024)]);
        assert!(w <= 1152 && h <= 870 && w * 9 == h * 16);
        let second = Area { x: 1920, y: 0, w: 1920, h: 1080 };
        assert!(window_placement(None, None, &[second, area(1920, 1080)]).1 .0 >= 1920, "the first listed is the primary");
    }

    #[test]
    fn a_saved_window_is_clamped_onto_the_monitor() {
        let main = Area { x: 0, y: 0, w: 1920, h: 1080 };
        let side = Area { x: 1920, y: 0, w: 1280, h: 720 };
        let place = |size, pos, m: &[Area]| window_placement(Some(size), pos, m);
        assert_eq!(place((1280, 720), Some((100, 50)), &[main]), ((1280, 720), (100, 50)), "as saved");
        assert_eq!(place((1280, 720), None, &[main]), ((1280, 720), (320, 180)), "no position: centred");
        // bigger than the monitor now: shrunk, and the position pulled in
        let (size, (x, y)) = place((3000, 2000), Some((-500, -500)), &[main]);
        assert!(size.0 <= 1920 && size.1 <= 1080 - FRAME_ROOM && x >= 0 && y >= 0, "{size:?} {x} {y}");
        // far off to the right (a monitor that is gone): back on the primary one
        let (size, (x, y)) = place((1280, 720), Some((5000, 100)), &[main]);
        assert_eq!(size, (1280, 720));
        assert!((0..=1920 - 1280).contains(&x) && (0..=1080 - 720 - FRAME_ROOM as i32).contains(&y));
        // the bottom edge (and its title bar) stays on screen
        let (_, (_, y)) = place((1280, 720), Some((0, 900)), &[main]);
        assert_eq!(y, 1080 - 720 - FRAME_ROOM as i32);
        // on the second monitor, it is clamped to that one
        let (size, (x, _)) = place((1600, 700), Some((2000, 10)), &[main, side]);
        assert_eq!(size.0, 1280);
        assert_eq!(x, 1920);
        // a tiny size is raised to the minimum
        assert_eq!(place((10, 10), Some((0, 0)), &[main]).0, MIN_WINDOW);
        // no monitor known at all: still a sane window
        assert_eq!(window_placement(None, None, &[]).0, (1632, 918));
    }

    #[test]
    fn the_language_setting_is_kept_and_a_bad_code_is_ignored() {
        assert_eq!(Settings::parse("language = pt-BR\n").language.as_deref(), Some("pt-BR"));
        assert_eq!(Settings::parse(&Settings::default().to_text()).language, None);
        assert!(!Settings::default().to_text().contains("language"), "nothing written for the script's own language");
        for bad in ["language =\n", "language = ../../x\n", "language = a b\n", "language = aaaaaaaaaaaaaaaaaaaa\n"] {
            assert_eq!(Settings::parse(bad).language, None, "{bad:?}");
        }
    }

    #[test]
    fn the_quick_menu_setting_cycles_and_resolves_against_the_theme() {
        let mut s = QuickMenuSetting::Default;
        let mut seen = Vec::new();
        for _ in 0..5 {
            seen.push(s);
            s = s.cycled(false);
        }
        assert_eq!(seen[..4], QuickMenuSetting::ALL);
        assert_eq!(seen[4], QuickMenuSetting::Default, "wraps");
        assert_eq!(QuickMenuSetting::Default.cycled(true), QuickMenuSetting::Hidden);
        assert_eq!(QuickMenuSetting::Default.resolve(QuickMenu::Hover), QuickMenu::Hover);
        assert_eq!(QuickMenuSetting::Visible.resolve(QuickMenu::Hover), QuickMenu::Visible);
        assert_eq!(QuickMenuSetting::Hidden.resolve(QuickMenu::Visible), QuickMenu::Hidden);
    }

    #[test]
    fn out_of_range_values_are_clamped() {
        let s = Settings::parse("text_speed = 9999\nauto_delay = 0\nvoice_volume = 2\n");
        assert_eq!((s.text_speed, s.auto_delay, s.voice_volume), (MAX_TEXT_SPEED, MIN_AUTO_DELAY, 1.0));
        assert_eq!(Settings::parse("text_speed = -5\n").text_speed, 0.0); // instant
    }

    #[test]
    fn text_speed_slider_ends_in_instant_and_rounds() {
        assert_eq!(text_speed_from_slider(0.0), MIN_TEXT_SPEED);
        assert_eq!(text_speed_from_slider(1.0), 0.0);
        assert_eq!(text_speed_from_slider(0.5) % 5.0, 0.0);
        assert_eq!(text_speed_to_slider(0.0), 1.0);
        for cps in [10.0, 60.0, 200.0] {
            let back = text_speed_from_slider(text_speed_to_slider(cps));
            assert!((back - cps).abs() <= 5.0, "{cps} -> {back}");
            assert!(back > 0.0, "a real speed never becomes instant");
        }
    }

    #[test]
    fn other_sliders() {
        assert_eq!(auto_delay_from_slider(0.0), MIN_AUTO_DELAY);
        assert_eq!(auto_delay_from_slider(1.0), MAX_AUTO_DELAY);
        assert_eq!(auto_delay_from_slider(auto_delay_to_slider(1.5)), 1.5);
        assert_eq!(volume_from_slider(0.456), 0.46);
        assert_eq!(volume_from_slider(-1.0), 0.0);
    }
}
