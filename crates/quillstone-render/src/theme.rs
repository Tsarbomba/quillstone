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
//! inventory_panel = gui/bag.png
//! inventory_cell = #20283a
//! ```

/// How a cell of the inventory and shop grid is filled: a colour or a picture.
#[derive(Debug, Clone, PartialEq)]
pub enum CellStyle {
    Color([f32; 4]),
    /// An image file (relative to the script), stretched over the cell.
    Image(String),
}

/// The least height of an inventory/shop cell: the icon (at least 16 px), name row
/// and price row of `quillstone-ui`'s `bag.rs` need 106 px.
pub const MIN_CELL_HEIGHT: f32 = 120.0;

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
    /// Full-screen picture behind the inventory (and the shop, unless
    /// `shop_background` is set). Without one the screen is dimmed with `overlay_color`.
    pub inventory_background: Option<String>,
    pub shop_background: Option<String>,
    /// A picture drawn stretched over `inventory_panel_rect`, under the grid.
    pub inventory_panel: Option<String>,
    /// `x, y, w, h` in the 1920x1080 screen: the area the title, grid, description
    /// and buttons are laid out in.
    pub inventory_panel_rect: [f32; 4],
    pub inventory_columns: usize,
    /// Width and height of one cell.
    pub inventory_cell_size: [f32; 2],
    pub inventory_cell_gap: f32,
    /// The icon is drawn in a square this big, centred at the top of the cell.
    pub inventory_icon_size: f32,
    /// Cell looks. `None` keeps the built-in colours (from `button_color` and friends).
    pub inventory_cell: Option<CellStyle>,
    pub inventory_cell_hover: Option<CellStyle>,
    pub inventory_cell_owned: Option<CellStyle>,
    pub inventory_cell_disabled: Option<CellStyle>,
    pub inventory_title: String,
    pub shop_title: String,
    /// What the shop calls its money ("Gold"); the pay variable's name if unset.
    pub shop_currency: Option<String>,
    /// Where the description of the hovered item goes; `None` puts it under the grid.
    pub inventory_description_rect: Option<[f32; 4]>,
    /// Item names; `None` follows `text_color`.
    pub inventory_name_color: Option<[f32; 4]>,
    /// Prices and the money line; `None` follows `accent_color`.
    pub inventory_price_color: Option<[f32; 4]>,
    /// The description; `None` follows `text_color`.
    pub inventory_text_color: Option<[f32; 4]>,
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
            inventory_background: None,
            shop_background: None,
            inventory_panel: None,
            inventory_panel_rect: [210.0, 90.0, 1500.0, 900.0],
            inventory_columns: 5,
            inventory_cell_size: [240.0, 200.0],
            inventory_cell_gap: 24.0,
            inventory_icon_size: 96.0,
            inventory_cell: None,
            inventory_cell_hover: None,
            inventory_cell_owned: None,
            inventory_cell_disabled: None,
            inventory_title: "Items".into(),
            shop_title: "Shop".into(),
            shop_currency: None,
            inventory_description_rect: None,
            inventory_name_color: None,
            inventory_price_color: None,
            inventory_text_color: None,
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
            let optional_color = match key {
                "inventory_name_color" => Some(&mut t.inventory_name_color),
                "inventory_price_color" => Some(&mut t.inventory_price_color),
                "inventory_text_color" => Some(&mut t.inventory_text_color),
                _ => None,
            };
            if let Some(slot) = optional_color {
                match parse_color(value) {
                    Some(c) => *slot = Some(c),
                    None => bad(format!("`{value}` is not a colour like #ffd98c or #000000b8")),
                }
                continue;
            }
            let cell_slot = match key {
                "inventory_cell" => Some(&mut t.inventory_cell),
                "inventory_cell_hover" => Some(&mut t.inventory_cell_hover),
                "inventory_cell_owned" => Some(&mut t.inventory_cell_owned),
                "inventory_cell_disabled" => Some(&mut t.inventory_cell_disabled),
                _ => None,
            };
            if let Some(slot) = cell_slot {
                match parse_cell_style(value) {
                    Ok(style) => *slot = Some(style),
                    Err(m) => bad(m),
                }
                continue;
            }
            let number = |lo: f32, hi: f32| value.parse::<f32>().ok().filter(|n| (lo..=hi).contains(n));
            match key {
                "inventory_background" => t.inventory_background = path(),
                "shop_background" => t.shop_background = path(),
                "inventory_panel" => t.inventory_panel = path(),
                "inventory_panel_rect" | "inventory_description_rect" => match parse_rect(value) {
                    Some(r) if key == "inventory_panel_rect" => t.inventory_panel_rect = r,
                    Some(r) => t.inventory_description_rect = Some(r),
                    None => bad(format!(
                        "`{key}` must be four numbers `x, y, w, h` that lie inside the 1920x1080 screen"
                    )),
                },
                "inventory_columns" => match value.parse::<usize>().ok().filter(|n| (1..=12).contains(n)) {
                    Some(n) => t.inventory_columns = n,
                    None => bad("`inventory_columns` must be a whole number from 1 to 12".into()),
                },
                "inventory_cell_size" => match parse_numbers(value, 48.0, 600.0) {
                    Some(v) if v.len() == 1 && v[0] >= MIN_CELL_HEIGHT => t.inventory_cell_size = [v[0], v[0]],
                    Some(v) if v.len() == 2 && v[1] >= MIN_CELL_HEIGHT => t.inventory_cell_size = [v[0], v[1]],
                    _ => bad(format!(
                        "`inventory_cell_size` must be a size from {MIN_CELL_HEIGHT} to 600, or `width, height` \
                         (width from 48, height from {MIN_CELL_HEIGHT}: a cell holds an icon, a name and a price)"
                    )),
                },
                "inventory_cell_gap" => match number(0.0, 100.0) {
                    Some(n) => t.inventory_cell_gap = n,
                    None => bad("`inventory_cell_gap` must be a number from 0 to 100".into()),
                },
                "inventory_icon_size" => match number(16.0, 400.0) {
                    Some(n) => t.inventory_icon_size = n,
                    None => bad("`inventory_icon_size` must be a number from 16 to 400".into()),
                },
                "inventory_title" | "shop_title" | "shop_currency" => match unquote(value) {
                    "" => bad(format!("`{key}` needs a text")),
                    text if key == "inventory_title" => t.inventory_title = text.to_string(),
                    text if key == "shop_title" => t.shop_title = text.to_string(),
                    text => t.shop_currency = Some(text.to_string()),
                },
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
        out.extend(self.inventory_images());
        out
    }

    /// The pictures the inventory and shop screens use.
    pub fn inventory_images(&self) -> Vec<&str> {
        let cells = [&self.inventory_cell, &self.inventory_cell_hover, &self.inventory_cell_owned, &self.inventory_cell_disabled];
        let mut out: Vec<&str> =
            [&self.inventory_background, &self.shop_background, &self.inventory_panel].into_iter().filter_map(|p| p.as_deref()).collect();
        out.extend(cells.into_iter().filter_map(|c| match c {
            Some(CellStyle::Image(p)) => Some(p.as_str()),
            _ => None,
        }));
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
        let mut out: Vec<&str> =
            [&self.textbox_image, &self.menu_background].into_iter().filter_map(|p| p.as_deref()).collect();
        out.extend(self.inventory_images());
        out
    }
}

/// `#rrggbb[aa]` is a colour; anything else is an image file name.
fn parse_cell_style(value: &str) -> Result<CellStyle, String> {
    if value.is_empty() {
        return Err("a cell needs a colour like #20283a or an image file".into());
    }
    if value.starts_with('#') {
        return parse_color(value)
            .map(CellStyle::Color)
            .ok_or_else(|| format!("`{value}` is not a colour like #20283a or #20283acc"));
    }
    Ok(CellStyle::Image(value.to_string()))
}

/// Comma-separated numbers, each within `lo..=hi`.
fn parse_numbers(value: &str, lo: f32, hi: f32) -> Option<Vec<f32>> {
    value.split(',').map(|n| n.trim().parse::<f32>().ok().filter(|n| (lo..=hi).contains(n))).collect()
}

/// `x, y, w, h` inside the 1920x1080 screen, with some size.
fn parse_rect(value: &str) -> Option<[f32; 4]> {
    let v = parse_numbers(value, 0.0, 1920.0)?;
    let [x, y, w, h] = v[..] else { return None };
    (w >= 16.0 && h >= 16.0 && x + w <= 1920.0 && y + h <= 1080.0).then_some([x, y, w, h])
}

fn unquote(s: &str) -> &str {
    for q in ['"', '\''] {
        if let Some(inner) = s.strip_prefix(q).and_then(|x| x.strip_suffix(q)) {
            return inner.trim();
        }
    }
    s
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

    #[test]
    fn the_inventory_keys_have_defaults_that_need_no_theme_file() {
        let t = Theme::default();
        assert_eq!((t.inventory_columns, t.inventory_cell_size, t.inventory_cell_gap), (5, [240.0, 200.0], 24.0));
        assert_eq!((t.inventory_title.as_str(), t.shop_title.as_str()), ("Items", "Shop"));
        assert!(t.inventory_cell.is_none() && t.inventory_background.is_none() && t.inventory_panel.is_none());
        assert!(t.inventory_images().is_empty());
        let [x, y, w, h] = t.inventory_panel_rect;
        assert!(x >= 0.0 && y >= 0.0 && x + w <= 1920.0 && y + h <= 1080.0);
    }

    #[test]
    fn the_inventory_keys_are_read() {
        let (t, problems) = Theme::parse(
            "inventory_background = gui/bag_bg.png\nshop_background = gui/shop_bg.png\ninventory_panel = gui/panel.png\n\
             inventory_panel_rect = 100, 80, 1700, 900\ninventory_columns = 4\ninventory_cell_size = 200, 220\n\
             inventory_cell_gap = 10\ninventory_icon_size = 120\ninventory_cell = #20283a\n\
             inventory_cell_hover = gui/hover.png\ninventory_cell_owned = #20283a80\ninventory_cell_disabled = #101010\n\
             inventory_title = \"Your bag\"\nshop_title = Market\nshop_currency = Coins\n\
             inventory_description_rect = 300,700,1300,160\ninventory_name_color = #ff0000\n\
             inventory_price_color = #00ff00\ninventory_text_color = #0000ff\n",
        );
        assert!(problems.is_empty(), "{problems:?}");
        assert_eq!(t.inventory_panel_rect, [100.0, 80.0, 1700.0, 900.0]);
        assert_eq!((t.inventory_columns, t.inventory_cell_size, t.inventory_cell_gap, t.inventory_icon_size), (4, [200.0, 220.0], 10.0, 120.0));
        assert_eq!(t.inventory_cell, Some(CellStyle::Color([32.0 / 255.0, 40.0 / 255.0, 58.0 / 255.0, 1.0])));
        assert_eq!(t.inventory_cell_hover, Some(CellStyle::Image("gui/hover.png".into())));
        assert!(matches!(t.inventory_cell_owned, Some(CellStyle::Color(c)) if (c[3] - 128.0 / 255.0).abs() < 1e-6));
        assert_eq!((t.inventory_title.as_str(), t.shop_title.as_str(), t.shop_currency.as_deref()), ("Your bag", "Market", Some("Coins")));
        assert_eq!(t.inventory_description_rect, Some([300.0, 700.0, 1300.0, 160.0]));
        assert_eq!(t.inventory_name_color, Some([1.0, 0.0, 0.0, 1.0]));
        assert_eq!(t.inventory_price_color, Some([0.0, 1.0, 0.0, 1.0]));
        assert_eq!(t.inventory_text_color, Some([0.0, 0.0, 1.0, 1.0]));
        assert_eq!(t.inventory_images(), vec!["gui/bag_bg.png", "gui/shop_bg.png", "gui/panel.png", "gui/hover.png"]);
        assert!(t.images().contains(&"gui/hover.png") && t.assets().contains(&"gui/panel.png"));
        let (square, _) = Theme::parse("inventory_cell_size = 180");
        assert_eq!(square.inventory_cell_size, [180.0, 180.0]);
    }

    #[test]
    fn a_cell_must_be_tall_enough_for_its_icon_name_and_price() {
        let (t, problems) = Theme::parse("inventory_cell_size = 119\ninventory_cell_size = 300, 100\ninventory_cell_size = 48, 119.9");
        assert_eq!(t.inventory_cell_size, Theme::default().inventory_cell_size);
        assert_eq!(problems.len(), 3, "{problems:?}");
        assert!(problems[0].contains("120"), "{}", problems[0]);
        let (t, problems) = Theme::parse("inventory_cell_size = 48, 120");
        assert!(problems.is_empty() && t.inventory_cell_size == [48.0, MIN_CELL_HEIGHT]);
        let (t, problems) = Theme::parse("inventory_cell_size = 120");
        assert!(problems.is_empty() && t.inventory_cell_size == [120.0, 120.0]);
    }

    #[test]
    fn bad_inventory_lines_are_reported_with_their_line_and_ignored() {
        let (t, problems) = Theme::parse(
            "inventory_columns = 0\ninventory_columns = many\ninventory_cell_size = 10\ninventory_cell_size = 1,2,3\n\
             inventory_cell_gap = -4\ninventory_icon_size = huge\ninventory_panel_rect = 0,0,2000,100\n\
             inventory_panel_rect = 1,2,3\ninventory_description_rect = 0,1000,100,200\ninventory_cell = #xyz\n\
             inventory_cell_hover =\ninventory_name_color = red\ninventory_title =\ninventory_text_color = #12\n",
        );
        assert_eq!(t, Theme::default());
        assert_eq!(problems.len(), 14, "{problems:#?}");
        for (i, p) in problems.iter().enumerate() {
            assert!(p.starts_with(&format!("line {}:", i + 1)), "{p}");
        }
    }
}
