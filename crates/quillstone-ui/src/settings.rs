//! Player preferences, and the small text file they are kept in.
//!
//! The file is `key = value` lines. Unknown keys and bad values are ignored (the
//! default is used), so an old or hand-edited file never stops the game.

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
                _ => {}
            }
        }
        s
    }

    /// The settings as file text; [`Settings::parse`] reads it back unchanged.
    pub fn to_text(&self) -> String {
        format!(
            "# Quillstone settings\n\
             text_speed = {}\n\
             auto_delay = {}\n\
             music_volume = {}\n\
             sfx_volume = {}\n\
             voice_volume = {}\n\
             fullscreen = {}\n\
             skip_unread = {}\n",
            self.text_speed,
            self.auto_delay,
            self.music_volume,
            self.sfx_volume,
            self.voice_volume,
            self.fullscreen,
            self.skip_unread
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
