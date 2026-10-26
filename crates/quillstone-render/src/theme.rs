//! The look of a game: colours, font, sizes, and a few images, read from a
//! small `key = value` file that the script names in its front matter
//! (`theme: theme.txt`). Everything is optional; [`Theme::default`] is the
//! built-in look.
//!
//! ```text
//! # theme.txt
//! font = fonts/Literata.ttf
//! text_size = 38
//! text_color = #ffffff
//! accent_color = #ffd98c
//! panel_color = #000000b8
//! textbox_image = gui/textbox.png
//! menu_background = gui/menu.png
//! menu_music = audio/theme.ogg
//! ```

#[derive(Debug, Clone, PartialEq)]
pub struct Theme {
    /// A `.ttf` / `.otf` file for all text (paths are relative to the script).
    /// `None` uses the system's fonts.
    pub font: Option<String>,
    /// The same font's bold, italic and bold italic files, for `**bold**` and
    /// `*italic*` text. Without them those use whatever the system offers.
    pub font_bold: Option<String>,
    pub font_italic: Option<String>,
    pub font_bold_italic: Option<String>,
    pub text_size: f32,
    pub name_size: f32,
    pub text_color: [f32; 4],
    /// Name plates of speakers without their own `color=`.
    pub name_color: [f32; 4],
    /// Highlights: active toggles, the current page, headings.
    pub accent_color: [f32; 4],
    /// Hints and secondary text.
    pub dim_color: [f32; 4],
    /// The textbox, when there is no `textbox_image`.
    pub panel_color: [f32; 4],
    pub button_color: [f32; 4],
    pub button_hover_color: [f32; 4],
    /// Behind the settings, backlog and save/load screens.
    pub overlay_color: [f32; 4],
    /// Drawn stretched over the textbox area (1600x260 at 1920x1080).
    pub textbox_image: Option<String>,
    pub menu_background: Option<String>,
    pub menu_music: Option<String>,
}

impl Default for Theme {
    fn default() -> Self {
        Theme {
            font: None,
            font_bold: None,
            font_italic: None,
            font_bold_italic: None,
            text_size: 38.0,
            name_size: 36.0,
            text_color: [1.0, 1.0, 1.0, 1.0],
            name_color: [1.0, 0.85, 0.55, 1.0],
            accent_color: [1.0, 0.85, 0.55, 1.0],
            dim_color: [1.0, 1.0, 1.0, 0.55],
            panel_color: [0.0, 0.0, 0.0, 0.72],
            button_color: [0.0, 0.0, 0.0, 0.70],
            button_hover_color: [0.25, 0.20, 0.42, 0.88],
            overlay_color: [0.02, 0.02, 0.05, 0.88],
            textbox_image: None,
            menu_background: None,
            menu_music: None,
        }
    }
}

impl Theme {
    /// Read a theme file. Returns the theme and a message for every line that
    /// was ignored (`line N: ...`); those keep their default.
    pub fn parse(text: &str) -> (Theme, Vec<String>) {
        let mut t = Theme::default();
        let mut problems = Vec::new();
        for (i, raw) in text.lines().enumerate() {
            let line = raw.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let mut bad = |msg: String| problems.push(format!("line {}: {msg}", i + 1));
            let Some((key, value)) = line.split_once('=') else {
                bad(format!("expected `key = value`, got `{line}`"));
                continue;
            };
            let (key, value) = (key.trim(), value.trim());
            let path = || (!value.is_empty()).then(|| value.to_string());
            let size = |v: &str| v.parse::<f32>().ok().filter(|s| (8.0..=200.0).contains(s));
            let color_slot = match key {
                "text_color" => Some(&mut t.text_color),
                "name_color" => Some(&mut t.name_color),
                "accent_color" => Some(&mut t.accent_color),
                "dim_color" => Some(&mut t.dim_color),
                "panel_color" => Some(&mut t.panel_color),
                "button_color" => Some(&mut t.button_color),
                "button_hover_color" => Some(&mut t.button_hover_color),
                "overlay_color" => Some(&mut t.overlay_color),
                _ => None,
            };
            if let Some(slot) = color_slot {
                match parse_color(value) {
                    Some(c) => *slot = c,
                    None => bad(format!("`{value}` is not a colour like #ffd98c or #000000b8")),
                }
                continue;
            }
            match key {
                "font" => t.font = path(),
                "font_bold" => t.font_bold = path(),
                "font_italic" => t.font_italic = path(),
                "font_bold_italic" => t.font_bold_italic = path(),
                "textbox_image" => t.textbox_image = path(),
                "menu_background" => t.menu_background = path(),
                "menu_music" => t.menu_music = path(),
                "text_size" | "name_size" => match size(value) {
                    Some(s) if key == "text_size" => t.text_size = s,
                    Some(s) => t.name_size = s,
                    None => bad(format!("`{key}` must be a number from 8 to 200")),
                },
                other => bad(format!("unknown key `{other}`")),
            }
        }
        (t, problems)
    }

    /// Files the theme refers to (relative to the script's folder).
    pub fn assets(&self) -> Vec<&str> {
        let mut out = self.fonts();
        out.extend([&self.textbox_image, &self.menu_background, &self.menu_music].into_iter().filter_map(|p| p.as_deref()));
        out
    }

    /// The font files to load, regular first.
    pub fn fonts(&self) -> Vec<&str> {
        [&self.font, &self.font_bold, &self.font_italic, &self.font_bold_italic]
            .into_iter()
            .filter_map(|p| p.as_deref())
            .collect()
    }

    /// Images the theme draws (to load along with the script's own).
    pub fn images(&self) -> Vec<&str> {
        [&self.textbox_image, &self.menu_background].into_iter().filter_map(|p| p.as_deref()).collect()
    }
}

/// `#rrggbb` or `#rrggbbaa` (sRGB, 0..1 components).
pub fn parse_color(s: &str) -> Option<[f32; 4]> {
    let hex = s.strip_prefix('#')?;
    if !(hex.len() == 6 || hex.len() == 8) || !hex.is_ascii() {
        return None;
    }
    let byte = |i: usize| u8::from_str_radix(&hex[i..i + 2], 16).ok().map(|b| b as f32 / 255.0);
    let a = if hex.len() == 8 { byte(6)? } else { 1.0 };
    Some([byte(0)?, byte(2)?, byte(4)?, a])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colours() {
        assert_eq!(parse_color("#ffffff"), Some([1.0; 4]));
        assert_eq!(parse_color("#00000080").unwrap()[3], 128.0 / 255.0);
        for bad in ["fff", "#fff", "#gggggg", "#ffffff0", "#ééé"] {
            assert_eq!(parse_color(bad), None, "{bad}");
        }
    }

    #[test]
    fn a_theme_file_overrides_what_it_names() {
        let (t, problems) = Theme::parse(
            "# comment\nfont = fonts/A B.ttf\ntext_size = 42\ntext_color = #ff0000\nmenu_music = m.ogg\n",
        );
        assert!(problems.is_empty(), "{problems:?}");
        assert_eq!(t.font.as_deref(), Some("fonts/A B.ttf"));
        assert_eq!(t.text_size, 42.0);
        assert_eq!(t.text_color, [1.0, 0.0, 0.0, 1.0]);
        assert_eq!(t.name_size, Theme::default().name_size);
        assert_eq!(t.assets(), vec!["fonts/A B.ttf", "m.ogg"]);
        assert!(t.images().is_empty());
    }

    #[test]
    fn mistakes_are_reported_with_their_line_and_ignored() {
        let (t, problems) = Theme::parse("text_size = huge\ncolour = #fff\npanel_color = red\njust words\n");
        assert_eq!(t, Theme::default());
        assert_eq!(problems.len(), 4);
        assert!(problems[0].starts_with("line 1:"));
        assert!(problems[1].contains("unknown key `colour`"));
    }
}
