//! Loading a game's theme and fonts, shared by the player.

use quillstone_render::Theme;
use quillstone_script::Program;
use quillstone_ui::Assets;

/// The theme the script names in its front matter, or the built-in look.
pub fn load_theme(program: &Program, assets: &Assets) -> Theme {
    let Some(file) = &program.theme else { return Theme::default() };
    match assets.read_to_string(file) {
        Ok(text) => {
            let (theme, problems) = Theme::parse(&text);
            for p in problems {
                eprintln!("warning: {file}: {p}");
            }
            theme
        }
        Err(e) => {
            eprintln!("warning: cannot read the theme {file}: {e}; using the built-in look");
            Theme::default()
        }
    }
}

/// The largest font file read (the biggest CJK fonts are a few tens of MB).
const MAX_FONT_BYTES: u64 = 64 << 20;

/// The theme's font files (regular first), as the text renderer wants them.
pub fn load_fonts(theme: &Theme, assets: &Assets) -> Vec<Vec<u8>> {
    theme
        .fonts()
        .into_iter()
        .filter_map(|f| assets.read_limited(f, MAX_FONT_BYTES).map_err(|e| eprintln!("warning: cannot read the font {f}: {e}")).ok())
        .collect()
}
