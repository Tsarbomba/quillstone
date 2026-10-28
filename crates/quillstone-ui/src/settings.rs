//! Player preferences, and the small text file they are kept in.
//!
//! The file is `key = value` lines. Unknown keys and bad values are ignored (the
//! default is used), so an old or hand-edited file never stops the game.

use quillstone_render::QuickMenu;

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
                _ => {}
            }
        }
        s
    }

    /// The settings as file text; [`Settings::parse`] reads it back unchanged.
    pub fn to_text(&self) -> String {
        let language = self.language.as_ref().map(|l| format!("language = {l}\n")).unwrap_or_default();
        format!(
            "# Quillstone settings\n\
             text_speed = {}\n\
             auto_delay = {}\n\
             music_volume = {}\n\
             sfx_volume = {}\n\
             voice_volume = {}\n\
             fullscreen = {}\n\
             skip_unread = {}\n\
             quick_menu = {}\n{}",
            self.text_speed,
            self.auto_delay,
            self.music_volume,
            self.sfx_volume,
            self.voice_volume,
            self.fullscreen,
            self.skip_unread,
            self.quick_menu.name(),
            language
        )
    }
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
