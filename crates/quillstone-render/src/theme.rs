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

use crate::view::Align;

/// When the quick menu (Back, History, Skip, ...) under the textbox is shown.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum QuickMenu {
    /// Always (the default).
    #[default]
    Visible,
    /// Only while the cursor is over the textbox or the quick menu's strip.
    Hover,
    /// Never drawn or clickable (the keyboard and gamepad shortcuts still work).
    Hidden,
}

impl QuickMenu {
    pub fn from_name(name: &str) -> Option<QuickMenu> {
        match name {
            "visible" => Some(QuickMenu::Visible),
            "hover" => Some(QuickMenu::Hover),
            "hidden" => Some(QuickMenu::Hidden),
            _ => None,
        }
    }
}

/// Where the quick menu's flowing row (the buttons without a `_pos`) sits along the textbox.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum QuickAlign {
    /// Starts at the textbox's left edge.
    Left,
    /// Centred on the textbox.
    Center,
    /// Ends at the textbox's right edge (the default, so existing games don't move).
    #[default]
    Right,
}

/// The dialogue box at 1920x1080 (`x, y, w, h`) unless the theme moves it.
pub const DEFAULT_TEXTBOX: [f32; 4] = [160.0, 780.0, 1600.0, 260.0];
/// Text inset `left, top, right, bottom` inside the textbox. The top inset is used
/// for a line without a speaker; with one, the text starts below the name plate.
pub const DEFAULT_TEXTBOX_PADDING: [f32; 4] = [40.0, 36.0, 40.0, 20.0];
/// Least size of the textbox, and of the text area left inside it by the padding.
pub const MIN_TEXTBOX: [f32; 2] = [400.0, 120.0];
pub const MIN_TEXT_AREA: [f32; 2] = [200.0, 60.0];

/// The `::input` panel at 1920x1080 unless the theme moves it, and the least size that holds its parts.
pub const DEFAULT_INPUT_RECT: [f32; 4] = [360.0, 310.0, 1200.0, 460.0];
pub const MIN_INPUT_RECT: [f32; 2] = [600.0, 460.0];
/// The settings panel at 1920x1080 unless the theme moves it, the least size of one, and the text size
/// the screen's layout is made for (`options_text_size` scales rows, buttons and text from it).
pub const DEFAULT_OPTIONS_RECT: [f32; 4] = [400.0, 90.0, 1120.0, 910.0];
pub const MIN_OPTIONS_RECT: [f32; 2] = [640.0, 420.0];
pub const DEFAULT_OPTIONS_TEXT_SIZE: f32 = 36.0;

/// How a cell of the inventory and shop grid is filled: a colour or a picture.
#[derive(Debug, Clone, PartialEq)]
pub enum CellStyle {
    Color([f32; 4]),
    /// An image file (relative to the script), stretched over the cell.
    Image(String),
}

/// The quick menu buttons a theme can give a picture, in the order they are drawn
/// (and in which a later one is on top where two overlap): the ids of the keys
/// `quick_<id>_icon`, `_hover`, `_on`, `_pos` and `_size`.
pub const QUICK_IDS: [&str; 11] =
    ["back", "history", "skip", "auto", "save", "load", "qsave", "qload", "settings", "items", "journal"];

/// The smallest and largest side of a picture button.
pub const QUICK_ICON_SIDE: (f32, f32) = (16.0, 512.0);

/// How a theme dresses one quick menu button (all optional: the default is the text label in the row).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct QuickIcon {
    /// Idle picture; when set the button draws it instead of its text.
    pub icon: Option<String>,
    /// Picture while the cursor is on the button.
    pub hover: Option<String>,
    /// Picture while the toggle (Skip, Auto) is on.
    pub on: Option<String>,
    /// Top-left in the 1920x1080 screen: the button leaves the row and sits there.
    pub pos: Option<[f32; 2]>,
    /// Width and height; by default the picture's own size (a text button: 150x32).
    pub size: Option<[f32; 2]>,
}

/// `quick_<id>_<field>` split into the button's index in [`QUICK_IDS`] (`Err`: an unknown id) and the field.
fn quick_key(key: &str) -> Option<(Result<usize, &str>, &str)> {
    let (id, field) = key.strip_prefix("quick_")?.rsplit_once('_')?;
    ["icon", "hover", "on", "pos", "size"].contains(&field).then_some(())?;
    Some((QUICK_IDS.iter().position(|q| *q == id).ok_or(id), field))
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
    /// A border round the dialogue and name text: `text_outline_size` px (0 = none) in this colour.
    pub text_outline_color: [f32; 4],
    pub text_outline_size: f32,
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
    /// `x, y, w, h` of the dialogue box in the 1920x1080 screen.
    pub textbox_rect: [f32; 4],
    /// Text inset `left, top, right, bottom` (see [`DEFAULT_TEXTBOX_PADDING`]).
    pub textbox_padding: [f32; 4],
    /// Drawn stretched over the name plate (under the speaker's name), if loaded.
    pub name_plate_image: Option<String>,
    /// `x, y, w, h` of the name plate in the screen; `None`: inside the top left of the textbox.
    pub name_plate_rect: Option<[f32; 4]>,
    /// Alignment of the dialogue text and of the speaker's name: `Left` or `CenterX`.
    pub text_align: Align,
    pub name_align: Align,
    /// When the quick menu is shown (the player may override it in Settings).
    pub quick_menu: QuickMenu,
    /// Where the quick menu's row sits along the textbox.
    pub quick_align: QuickAlign,
    /// Picture buttons and positions for the quick menu, indexed like [`QUICK_IDS`].
    pub quick_icons: [QuickIcon; 11],
    /// `quick_buttons = listed`: only buttons with a picture or a position of their own are shown.
    pub quick_listed: bool,
    pub menu_background: Option<String>,
    /// A picture (a frame, vignette or shading) drawn over `menu_background`, under the main menu's buttons.
    pub menu_overlay: Option<String>,
    /// The same behind the Save, Load, Settings and History screens' panels, over their dim or `options_background`.
    pub game_menu_overlay: Option<String>,
    pub menu_music: Option<String>,
    /// Music and sound loops play at this fraction of their volume while a voice clip does (1 = off).
    pub voice_duck: f32,
    /// Drawn stretched over the `::input` panel instead of `panel_color`.
    pub input_image: Option<String>,
    /// `x, y, w, h` of the `::input` panel; `None`: centred ([`DEFAULT_INPUT_RECT`]).
    pub input_rect: Option<[f32; 4]>,
    /// Settings screen: where its panel sits, the text size it is laid out for, a picture behind the
    /// whole screen (instead of `overlay_color`) and a picture for the panel, drawn as a 9-slice whose
    /// corners are `options_panel_border` picture pixels (0: stretched whole).
    pub options_rect: [f32; 4],
    pub options_text_size: f32,
    pub options_background: Option<String>,
    pub options_panel_image: Option<String>,
    pub options_panel_border: f32,
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
    /// The title of the journal screen (`journal_title`, default "Journal").
    pub journal_title: String,
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
    /// The countdown bar of a timed `::choice`; `None` follows `accent_color`.
    pub choice_timer_color: Option<[f32; 4]>,
    /// Its thickness in the 1920x1080 screen.
    pub choice_timer_height: f32,
    /// The `::timing` bar: pictures stretched over the track (instead of `panel_color`) and for the marker
    /// (instead of a bar of `timing_marker_color`, default `text_color`).
    pub timing_bar_image: Option<String>,
    pub timing_marker_image: Option<String>,
    /// The good and perfect zones; `None` follows `accent_color` and `name_color`.
    pub timing_good_color: Option<[f32; 4]>,
    pub timing_perfect_color: Option<[f32; 4]>,
    pub timing_marker_color: Option<[f32; 4]>,
    /// The key cap of a `::press` prompt (instead of a `panel_color` box).
    pub press_key_image: Option<String>,
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
            text_outline_color: [0.0, 0.0, 0.0, 1.0],
            text_outline_size: 0.0,
            text_color: [1.0, 1.0, 1.0, 1.0],
            name_color: [1.0, 0.85, 0.55, 1.0],
            accent_color: [1.0, 0.85, 0.55, 1.0],
            dim_color: [1.0, 1.0, 1.0, 0.55],
            panel_color: [0.0, 0.0, 0.0, 0.72],
            button_color: [0.0, 0.0, 0.0, 0.70],
            button_hover_color: [0.25, 0.20, 0.42, 0.88],
            overlay_color: [0.02, 0.02, 0.05, 0.88],
            textbox_image: None,
            textbox_rect: DEFAULT_TEXTBOX,
            textbox_padding: DEFAULT_TEXTBOX_PADDING,
            name_plate_image: None,
            name_plate_rect: None,
            text_align: Align::Left,
            name_align: Align::Left,
            quick_menu: QuickMenu::Visible,
            quick_align: QuickAlign::Right,
            quick_icons: Default::default(),
            quick_listed: false,
            menu_background: None,
            menu_overlay: None,
            game_menu_overlay: None,
            menu_music: None,
            voice_duck: 1.0,
            input_image: None,
            input_rect: None,
            options_rect: DEFAULT_OPTIONS_RECT,
            options_text_size: DEFAULT_OPTIONS_TEXT_SIZE,
            options_background: None,
            options_panel_image: None,
            options_panel_border: 0.0,
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
            journal_title: "Journal".into(),
            shop_title: "Shop".into(),
            shop_currency: None,
            inventory_description_rect: None,
            inventory_name_color: None,
            inventory_price_color: None,
            inventory_text_color: None,
            choice_timer_color: None,
            choice_timer_height: 8.0,
            timing_bar_image: None,
            timing_marker_image: None,
            timing_good_color: None,
            timing_perfect_color: None,
            timing_marker_color: None,
            press_key_image: None,
        }
    }
}

impl Theme {
    /// Read a theme file. Returns the theme and a message for every line that
    /// was ignored (`line N: ...`); those keep their default.
    pub fn parse(text: &str) -> (Theme, Vec<String>) {
        let mut t = Theme::default();
        let mut problems = Vec::new();
        let (mut rect_line, mut padding_line) = (0, 0);
        let mut quick_lines = [[0usize; 2]; 11]; // the last `_hover` / `_on` line of each button
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
            let path = || (!value.is_empty()).then(|| normal_path(value));
            let color_slot = match key {
                "text_color" => Some(&mut t.text_color),
                "name_color" => Some(&mut t.name_color),
                "accent_color" => Some(&mut t.accent_color),
                "dim_color" => Some(&mut t.dim_color),
                "panel_color" => Some(&mut t.panel_color),
                "button_color" => Some(&mut t.button_color),
                "button_hover_color" => Some(&mut t.button_hover_color),
                "overlay_color" => Some(&mut t.overlay_color),
                "text_outline_color" => Some(&mut t.text_outline_color),
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
                "choice_timer_color" => Some(&mut t.choice_timer_color),
                "timing_good_color" => Some(&mut t.timing_good_color),
                "timing_perfect_color" => Some(&mut t.timing_perfect_color),
                "timing_marker_color" => Some(&mut t.timing_marker_color),
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
            if let Some((id, field)) = quick_key(key) {
                let k = match id {
                    Ok(k) => k,
                    Err(id) => {
                        bad(format!("unknown quick menu button `{id}` (the ids are {})", QUICK_IDS.join(", ")));
                        continue;
                    }
                };
                let slot = &mut t.quick_icons[k];
                let (lo, hi) = QUICK_ICON_SIDE;
                match field {
                    "pos" => match parse_numbers(value, 0.0, 1920.0) {
                        Some(v) if v.len() == 2 && v[1] <= 1080.0 => slot.pos = Some([v[0], v[1]]),
                        _ => bad(format!("`{key}` must be `x, y`: the top left, inside the 1920x1080 screen")),
                    },
                    "size" => match parse_numbers(value, lo, hi) {
                        Some(v) if v.len() == 2 => slot.size = Some([v[0], v[1]]),
                        _ => bad(format!("`{key}` must be `width, height`, each from {lo} to {hi}")),
                    },
                    _ if value.is_empty() => bad(format!("`{key}` needs a picture file")),
                    "icon" => slot.icon = path(),
                    "hover" => {
                        slot.hover = path();
                        quick_lines[k][0] = i + 1;
                    }
                    _ => {
                        slot.on = path();
                        quick_lines[k][1] = i + 1;
                    }
                }
                continue;
            }
            if let Some((_, lo, hi, note, slot)) = NUMBER_KEYS.iter().find(|k| k.0 == key) {
                match value.parse::<f32>().ok().filter(|n| (*lo..=*hi).contains(n)) {
                    Some(n) => *slot(&mut t) = n,
                    None => bad(format!("`{key}` must be a number from {lo} to {hi}{note}")),
                }
                continue;
            }
            if let Some(slot) = t.file_slot(key) {
                *slot = path();
                continue;
            }
            // `x, y, w, h` inside the screen, at least `min` big (none: any size).
            let rect = |min: Option<[f32; 2]>| {
                let [mw, mh] = min.unwrap_or_default();
                parse_rect(value).filter(|r| r[2] >= mw && r[3] >= mh).ok_or_else(|| match min {
                    Some([mw, mh]) => format!("`{key}` must be four numbers `x, y, w, h` inside the 1920x1080 screen, at least {mw}x{mh}"),
                    None => format!("`{key}` must be four numbers `x, y, w, h` that lie inside the 1920x1080 screen"),
                })
            };
            match key {
                "input_rect" => match rect(Some(MIN_INPUT_RECT)) {
                    Ok(r) => t.input_rect = Some(r),
                    Err(m) => bad(m),
                },
                "options_rect" => match rect(Some(MIN_OPTIONS_RECT)) {
                    Ok(r) => t.options_rect = r,
                    Err(m) => bad(m),
                },
                "inventory_panel_rect" => match rect(None) {
                    Ok(r) => t.inventory_panel_rect = r,
                    Err(m) => bad(m),
                },
                "inventory_description_rect" => match rect(None) {
                    Ok(r) => t.inventory_description_rect = Some(r),
                    Err(m) => bad(m),
                },
                "name_plate_rect" => match rect(None) {
                    Ok(r) => t.name_plate_rect = Some(r),
                    Err(m) => bad(m),
                },
                "textbox_rect" => match rect(Some(MIN_TEXTBOX)) {
                    Ok(r) => {
                        t.textbox_rect = r;
                        rect_line = i + 1;
                    }
                    Err(m) => bad(m),
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
                "inventory_title" | "journal_title" | "shop_title" | "shop_currency" => match unquote(value) {
                    "" => bad(format!("`{key}` needs a text")),
                    text if key == "inventory_title" => t.inventory_title = text.to_string(),
                    text if key == "journal_title" => t.journal_title = text.to_string(),
                    text if key == "shop_title" => t.shop_title = text.to_string(),
                    text => t.shop_currency = Some(text.to_string()),
                },
                "textbox_padding" => match parse_numbers(value, 0.0, 400.0) {
                    Some(v) if v.len() == 1 => {
                        t.textbox_padding = [v[0]; 4];
                        padding_line = i + 1;
                    }
                    Some(v) if v.len() == 4 => {
                        t.textbox_padding = [v[0], v[1], v[2], v[3]];
                        padding_line = i + 1;
                    }
                    _ => bad("`textbox_padding` must be one number or `left, top, right, bottom` (each 0 to 400)".into()),
                },
                "text_align" | "name_align" => {
                    let align = match value {
                        "left" => Some(Align::Left),
                        "center" => Some(Align::CenterX),
                        _ => None,
                    };
                    match (align, key) {
                        (Some(a), "text_align") => t.text_align = a,
                        (Some(a), _) => t.name_align = a,
                        (None, _) => bad(format!("`{key}` must be `left` or `center`")),
                    }
                }
                "quick_menu" => match QuickMenu::from_name(value) {
                    Some(m) => t.quick_menu = m,
                    None => bad("`quick_menu` must be `visible`, `hover` or `hidden`".into()),
                },
                "quick_align" => match value {
                    "left" => t.quick_align = QuickAlign::Left,
                    "center" => t.quick_align = QuickAlign::Center,
                    "right" => t.quick_align = QuickAlign::Right,
                    _ => bad("`quick_align` must be `left`, `center` or `right`".into()),
                },
                "quick_buttons" => match value {
                    "all" => t.quick_listed = false,
                    "listed" => t.quick_listed = true,
                    _ => bad("`quick_buttons` must be `all` or `listed`".into()),
                },
                other => bad(format!("unknown key `{other}`")),
            }
        }
        for (k, slot) in t.quick_icons.iter_mut().enumerate() {
            for (field, line, picture) in [("hover", quick_lines[k][0], &mut slot.hover), ("on", quick_lines[k][1], &mut slot.on)] {
                if picture.is_some() && slot.icon.is_none() {
                    *picture = None;
                    let id = QUICK_IDS[k];
                    problems.push(format!("line {line}: `quick_{id}_{field}` does nothing without `quick_{id}_icon`"));
                }
            }
        }
        // The padding must leave room for text in the textbox. Reported at the line that
        // set the padding (or the rect, when the padding is the default).
        let [_, _, w, h] = t.textbox_rect;
        let [l, tp, r, b] = t.textbox_padding;
        if w - l - r < MIN_TEXT_AREA[0] || h - tp - b < MIN_TEXT_AREA[1] {
            let line = if padding_line > 0 { padding_line } else { rect_line };
            problems.push(format!(
                "line {line}: the textbox padding leaves less than {}x{} px for the text inside the textbox; \
                 using the default padding",
                MIN_TEXT_AREA[0], MIN_TEXT_AREA[1]
            ));
            t.textbox_padding = DEFAULT_TEXTBOX_PADDING;
        }
        (t, problems)
    }

    /// Files the theme refers to (relative to the script's folder).
    pub fn assets(&self) -> Vec<&str> {
        let mut out = self.fonts();
        out.extend(self.screen_images());
        out.extend(self.music());
        out.extend(self.inventory_images());
        out.extend(self.quick_pictures());
        out
    }

    /// The pictures of the quick menu's buttons (idle, hover and toggle-on).
    pub fn quick_pictures(&self) -> Vec<&str> {
        self.quick_icons.iter().flat_map(|q| [&q.icon, &q.hover, &q.on]).filter_map(|p| p.as_deref()).collect()
    }

    /// The pictures the inventory and shop screens use.
    pub fn inventory_images(&self) -> Vec<&str> {
        let cells = [&self.inventory_cell, &self.inventory_cell_hover, &self.inventory_cell_owned, &self.inventory_cell_disabled];
        let mut out = self.inventory_files();
        out.extend(cells.into_iter().filter_map(|c| match c {
            Some(CellStyle::Image(p)) => Some(p.as_str()),
            _ => None,
        }));
        out
    }

    /// Images the theme draws (to load along with the script's own).
    pub fn images(&self) -> Vec<&str> {
        let mut out = self.screen_images();
        out.extend(self.inventory_images());
        out.extend(self.quick_pictures());
        out
    }
}

/// The theme file's keys that name a file (each is also a field of [`Theme`]), by what the files are for. Parsing
/// and the file lists (`fonts`, `assets`, `images`) all come from this one list, so a new key can't be left out.
macro_rules! file_keys {
    ($($vis:vis $group:ident: $($key:ident),+;)+) => {
        impl Theme {
            /// The field of the file key `key`.
            fn file_slot(&mut self, key: &str) -> Option<&mut Option<String>> {
                match key {
                    $($(stringify!($key) => Some(&mut self.$key),)+)+
                    _ => None,
                }
            }
            $(
                $vis fn $group(&self) -> Vec<&str> {
                    [$(&self.$key),+].into_iter().filter_map(|p| p.as_deref()).collect()
                }
            )+
        }
    };
}

file_keys! {
    // The font files to load, regular first.
    pub fonts: font, font_bold, font_italic, font_bold_italic;
    // The pictures of the textbox, name plate, menus, `::input` panel, settings screen, `::timing` bar and `::press` key.
    screen_images: textbox_image, name_plate_image, menu_background, menu_overlay, game_menu_overlay, input_image, options_background, options_panel_image,
        timing_bar_image, timing_marker_image, press_key_image;
    music: menu_music;
    // The pictures behind the inventory and shop screens (their cells are in `inventory_images`).
    inventory_files: inventory_background, shop_background, inventory_panel;
}

/// Plain number keys of the theme file: the range each allows, what to add to its message, and its field.
type NumberKey = (&'static str, f32, f32, &'static str, fn(&mut Theme) -> &mut f32);
const NUMBER_KEYS: [NumberKey; 9] = [
    ("text_size", 8.0, 200.0, "", |t| &mut t.text_size),
    ("name_size", 8.0, 200.0, "", |t| &mut t.name_size),
    ("options_text_size", 8.0, 200.0, "", |t| &mut t.options_text_size),
    ("text_outline_size", 0.0, 8.0, "", |t| &mut t.text_outline_size),
    ("options_panel_border", 0.0, 400.0, " (picture pixels)", |t| &mut t.options_panel_border),
    ("inventory_cell_gap", 0.0, 100.0, "", |t| &mut t.inventory_cell_gap),
    ("inventory_icon_size", 16.0, 400.0, "", |t| &mut t.inventory_icon_size),
    ("voice_duck", 0.0, 1.0, " (1 = no ducking)", |t| &mut t.voice_duck),
    ("choice_timer_height", 2.0, 40.0, "", |t| &mut t.choice_timer_height),
];

/// A file path of the theme file, normalised like the script's own paths, so one file under two spellings is one file.
fn normal_path(value: &str) -> String {
    if quillstone_script::is_inside(value) { quillstone_script::normal_path(value) } else { value.to_string() }
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
    Ok(CellStyle::Image(normal_path(value)))
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
        assert_eq!((t.inventory_title.as_str(), t.shop_title.as_str(), t.journal_title.as_str()), ("Items", "Shop", "Journal"));
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
             inventory_title = \"Your bag\"\njournal_title = Quests\nshop_title = Market\nshop_currency = Coins\n\
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
        assert_eq!(t.journal_title, "Quests");
        assert_eq!(t.inventory_description_rect, Some([300.0, 700.0, 1300.0, 160.0]));
        assert_eq!(t.inventory_name_color, Some([1.0, 0.0, 0.0, 1.0]));
        assert_eq!(t.inventory_price_color, Some([0.0, 1.0, 0.0, 1.0]));
        assert_eq!(t.inventory_text_color, Some([0.0, 0.0, 1.0, 1.0]));
        assert_eq!(t.inventory_images(), vec!["gui/bag_bg.png", "gui/shop_bg.png", "gui/panel.png", "gui/hover.png"]);
        assert!(t.images().contains(&"gui/hover.png") && t.assets().contains(&"gui/panel.png"));
        let (spelled, _) = Theme::parse("inventory_cell = ./gui//hover.png\ninventory_panel = ./gui//panel.png\n");
        assert_eq!(spelled.inventory_cell, Some(CellStyle::Image("gui/hover.png".into())), "like the other paths");
        assert_eq!(spelled.inventory_panel.as_deref(), Some("gui/panel.png"));
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

    #[test]
    fn the_textbox_keys_default_to_the_old_look() {
        let t = Theme::default();
        assert_eq!(t.textbox_rect, [160.0, 780.0, 1600.0, 260.0]);
        assert_eq!(t.textbox_rect, [crate::TEXTBOX.x, crate::TEXTBOX.y, crate::TEXTBOX.w, crate::TEXTBOX.h]);
        assert_eq!(t.textbox_padding, [40.0, 36.0, 40.0, 20.0]);
        assert_eq!((t.name_plate_image.as_deref(), t.name_plate_rect), (None, None));
        assert_eq!((t.text_align, t.name_align, t.quick_menu), (Align::Left, Align::Left, QuickMenu::Visible));
        assert_eq!(Theme::parse("").0, t);
    }

    #[test]
    fn the_textbox_keys_are_read() {
        let (t, problems) = Theme::parse(
            "textbox_rect = 100, 500.5, 1400, 400\ntextbox_padding = 10, 20, 30, 40\nname_plate_image = g/plate.png\n\
             name_plate_rect = 120,450,300,60\ntext_align = center\nname_align = center\nquick_menu = hover\n",
        );
        assert!(problems.is_empty(), "{problems:?}");
        assert_eq!(t.textbox_rect, [100.0, 500.5, 1400.0, 400.0]);
        assert_eq!(t.textbox_padding, [10.0, 20.0, 30.0, 40.0]);
        assert_eq!(t.name_plate_image.as_deref(), Some("g/plate.png"));
        assert_eq!(t.name_plate_rect, Some([120.0, 450.0, 300.0, 60.0]));
        assert_eq!((t.text_align, t.name_align, t.quick_menu), (Align::CenterX, Align::CenterX, QuickMenu::Hover));
        assert!(t.assets().contains(&"g/plate.png") && t.images().contains(&"g/plate.png"));
        assert_eq!(Theme::parse("textbox_padding = 24\n").0.textbox_padding, [24.0; 4]);
        assert_eq!(Theme::parse("quick_menu = hidden\n").0.quick_menu, QuickMenu::Hidden);
    }

    #[test]
    fn bad_textbox_values_are_reported_with_their_line_and_ignored() {
        let src = "textbox_rect = 0, 0, 2000, 300\n\
                   textbox_rect = 1700, 800, 400, 200\n\
                   textbox_rect = 100, 100, 200, 300\n\
                   textbox_rect = 1, 2, 3\n\
                   textbox_padding = 1, 2\n\
                   textbox_padding = -4\n\
                   name_plate_rect = 100, 100, 5000, 40\n\
                   text_align = right\n\
                   name_align = middle\n\
                   quick_menu = sometimes\n";
        let (t, problems) = Theme::parse(src);
        assert_eq!(problems.len(), 10, "{problems:?}");
        for (i, p) in problems.iter().enumerate() {
            assert!(p.starts_with(&format!("line {}:", i + 1)), "{p}");
        }
        assert_eq!(t, Theme::default(), "nothing took effect");
    }

    #[test]
    fn quick_align_is_left_center_or_right() {
        assert_eq!(Theme::default().quick_align, QuickAlign::Right);
        for (name, want) in [("left", QuickAlign::Left), ("center", QuickAlign::Center), ("right", QuickAlign::Right)] {
            let (t, problems) = Theme::parse(&format!("quick_align = {name}\n"));
            assert!(t.quick_align == want && problems.is_empty(), "{name}: {problems:?}");
        }
        let (t, problems) = Theme::parse("quick_menu = hover\nquick_align = middle\n");
        assert_eq!(t.quick_align, QuickAlign::Right);
        assert!(problems.len() == 1 && problems[0].contains("line 2") && problems[0].contains("quick_align"), "{problems:?}");
    }

    #[test]
    fn quick_buttons_is_all_or_listed() {
        assert!(!Theme::default().quick_listed && !Theme::parse("quick_buttons = all\n").0.quick_listed);
        let (t, problems) = Theme::parse("quick_buttons = listed\n");
        assert!(t.quick_listed && problems.is_empty());
        let (t, problems) = Theme::parse("quick_buttons = some\n");
        assert!(!t.quick_listed && problems.len() == 1 && problems[0].contains("line 1"), "{problems:?}");
    }

    #[test]
    fn the_quick_icon_keys_default_to_nothing_and_are_read() {
        let t = Theme::default();
        assert!(t.quick_icons.iter().all(|q| *q == QuickIcon::default()) && t.quick_pictures().is_empty());
        assert_eq!(Theme::parse("quick_menu = hover\n").0.quick_icons, t.quick_icons, "quick_menu is not a button");
        let mut text = String::new();
        for id in QUICK_IDS {
            text += &format!("quick_{id}_icon = g/{id}.png\nquick_{id}_hover = g/{id}_h.png\nquick_{id}_on = g/{id}_o.png\n");
        }
        text += "quick_back_pos = 10, 20.5\nquick_back_size = 64,48\nquick_items_pos = 1900, 1080\nquick_items_size = 16, 512\nquick_journal_pos = 5, 6\n";
        let (t, problems) = Theme::parse(&text);
        assert!(problems.is_empty(), "{problems:?}");
        let back = &t.quick_icons[0];
        assert_eq!(back.icon.as_deref(), Some("g/back.png"));
        assert_eq!((back.hover.as_deref(), back.on.as_deref()), (Some("g/back_h.png"), Some("g/back_o.png")));
        assert_eq!((back.pos, back.size), (Some([10.0, 20.5]), Some([64.0, 48.0])));
        assert_eq!(t.quick_icons[9].pos, Some([1900.0, 1080.0]));
        assert_eq!(t.quick_icons[1].pos, None);
        assert_eq!(t.quick_icons[10].icon.as_deref(), Some("g/journal.png"), "`journal` is the 11th button id");
        assert_eq!((t.quick_icons[10].hover.as_deref(), t.quick_icons[10].pos), (Some("g/journal_h.png"), Some([5.0, 6.0])));
        assert_eq!(t.quick_pictures().len(), 33);
        assert!(t.assets().contains(&"g/qload_h.png") && t.images().contains(&"g/settings_o.png"));
        // only a position and a size: a text button moved
        let (t, problems) = Theme::parse("quick_skip_pos = 5, 6\nquick_skip_size = 100, 40\n");
        assert!(problems.is_empty() && t.quick_pictures().is_empty() && t.quick_icons[2].pos == Some([5.0, 6.0]));
    }

    #[test]
    fn bad_quick_icon_lines_are_reported_with_their_line_and_ignored() {
        let src = "quick_menue_icon = a.png\n\
                   quick_back_icon =\n\
                   quick_back_pos = 10\n\
                   quick_back_pos = 10, 20, 30\n\
                   quick_back_pos = -1, 5\n\
                   quick_back_pos = 1921, 5\n\
                   quick_back_pos = 5, 1081\n\
                   quick_back_pos = a, b\n\
                   quick_back_size = 15, 40\n\
                   quick_back_size = 40, 513\n\
                   quick_back_size = 40\n\
                   quick_back_hover = h.png\n\
                   quick_skip_on = o.png\n\
                   quick_nothing_pos = 1, 1\n\
                   quick_back = a.png\n\
                   quick_hover = a.png\n";
        let (t, problems) = Theme::parse(src);
        assert_eq!(problems.len(), 16, "{problems:#?}");
        // the lines in the file order, then the hover/on pictures that had no icon (reported last)
        let lines = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 14, 15, 16, 12, 13];
        for (p, line) in problems.iter().zip(lines) {
            assert!(p.starts_with(&format!("line {line}:")), "{p}");
        }
        assert!(problems[0].contains("unknown quick menu button `menue`") && problems[0].contains("settings"));
        assert!(problems[14].contains("quick_back_hover") && problems[15].contains("quick_skip_on"), "{problems:#?}");
        assert!(problems[11].contains("`nothing`") && problems[12].contains("unknown key `quick_back`"), "{problems:#?}");
        assert!(t.quick_icons.iter().all(|q| *q == QuickIcon::default()), "nothing took effect: {:?}", t.quick_icons);
    }

    #[test]
    fn the_choice_timer_keys_have_defaults_are_read_and_are_checked() {
        let t = Theme::default();
        assert_eq!((t.choice_timer_color, t.choice_timer_height), (None, 8.0));
        let (t, problems) = Theme::parse("choice_timer_color = #ff8040\nchoice_timer_height = 14\n");
        assert!(problems.is_empty(), "{problems:?}");
        assert_eq!(t.choice_timer_color, Some([1.0, 128.0 / 255.0, 64.0 / 255.0, 1.0]));
        assert_eq!(t.choice_timer_height, 14.0);
        let (t, problems) = Theme::parse("choice_timer_color = red\nchoice_timer_height = 1\nchoice_timer_height = 41\n");
        assert_eq!(problems.len(), 3, "{problems:?}");
        assert_eq!(t, Theme::default());
    }

    #[test]
    fn the_timing_and_key_prompt_keys_have_defaults_are_read_and_their_pictures_are_assets() {
        let d = Theme::default();
        assert!(d.timing_bar_image.is_none() && d.timing_marker_image.is_none() && d.timing_good_color.is_none() && d.press_key_image.is_none());
        let (t, problems) = Theme::parse(
            "timing_bar_image = g/bar.png\ntiming_marker_image = g/m.png\ntiming_good_color = #00ff00\n\
             timing_perfect_color = #ff0000\ntiming_marker_color = #0000ff80\npress_key_image = g/cap.png\n",
        );
        assert!(problems.is_empty(), "{problems:?}");
        assert_eq!(t.timing_good_color, Some([0.0, 1.0, 0.0, 1.0]));
        assert_eq!(t.timing_perfect_color, Some([1.0, 0.0, 0.0, 1.0]));
        assert_eq!(t.timing_marker_color.map(|c| c[2]), Some(1.0));
        assert!(t.assets().contains(&"g/bar.png") && t.assets().contains(&"g/m.png") && t.assets().contains(&"g/cap.png"));
        let (t, problems) = Theme::parse("timing_good_color = green\n");
        assert_eq!(problems.len(), 1, "{problems:?}");
        assert_eq!(t, Theme::default());
    }

    #[test]
    fn voice_duck_defaults_to_off_is_read_and_is_checked() {
        assert_eq!(Theme::default().voice_duck, 1.0);
        let (t, problems) = Theme::parse("voice_duck = 0.4\n");
        assert!(problems.is_empty() && t.voice_duck == 0.4, "{problems:?}");
        let (t, problems) = Theme::parse("voice_duck = 1.5\nvoice_duck = quiet\n");
        assert_eq!(problems.len(), 2, "{problems:?}");
        assert_eq!(t, Theme::default());
    }

    #[test]
    fn a_padding_that_leaves_no_room_is_refused() {
        let (t, problems) = Theme::parse("textbox_rect = 100, 100, 500, 200\ntextbox_padding = 40, 100, 300, 50\n");
        assert_eq!(problems.len(), 1);
        assert!(problems[0].starts_with("line 2:") && problems[0].contains("padding"), "{problems:?}");
        assert_eq!(t.textbox_padding, DEFAULT_TEXTBOX_PADDING);
        assert_eq!(t.textbox_rect, [100.0, 100.0, 500.0, 200.0]);
    }

    #[test]
    fn outline_input_and_options_keys_are_read_with_defaults_that_change_nothing() {
        let d = Theme::default();
        assert_eq!((d.text_outline_size, d.input_rect, d.options_rect, d.options_text_size), (0.0, None, DEFAULT_OPTIONS_RECT, 36.0));
        assert!(d.input_image.is_none() && d.options_background.is_none() && d.options_panel_image.is_none());
        let (t, problems) = Theme::parse(
            "text_outline_color = #102030\ntext_outline_size = 1.5\ninput_image = g/in.png\ninput_rect = 100, 100, 900, 500\n\
             options_rect = 40, 50, 800, 900\noptions_text_size = 28\noptions_background = g/o.png\n\
             options_panel_image = g/p.png\noptions_panel_border = 24\n",
        );
        assert!(problems.is_empty(), "{problems:?}");
        assert_eq!((t.text_outline_color, t.text_outline_size), (parse_color("#102030").unwrap(), 1.5));
        assert_eq!((t.input_rect, t.options_rect, t.options_text_size), (Some([100.0, 100.0, 900.0, 500.0]), [40.0, 50.0, 800.0, 900.0], 28.0));
        assert_eq!(t.options_panel_border, 24.0);
        for pic in ["g/in.png", "g/o.png", "g/p.png"] {
            assert!(t.assets().contains(&pic) && t.images().contains(&pic), "{pic}");
        }
    }

    #[test]
    fn bad_outline_input_and_options_values_are_reported_with_their_line() {
        let text = "text_outline_size = 9\ntext_outline_size = -1\ntext_outline_color = black\ninput_rect = 0, 0, 500, 500\n\
                    input_rect = 1000, 0, 1000, 500\noptions_rect = 0, 0, 600, 900\noptions_rect = 0, 0, 900, 400\n\
                    options_text_size = 4\noptions_panel_border = 401\n";
        let (t, problems) = Theme::parse(text);
        assert_eq!(t, Theme::default());
        assert_eq!(problems.len(), 9, "{problems:?}");
        for (i, p) in problems.iter().enumerate() {
            assert!(p.starts_with(&format!("line {}:", i + 1)), "{p}");
        }
        assert!(Theme::parse("text_outline_size = 0\ntext_outline_size = 8").1.is_empty(), "0 and 8 are the ends");
    }
}
