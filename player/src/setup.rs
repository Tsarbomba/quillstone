//! Loading a game's theme and fonts, shared by the player.

use std::path::Path;

use quillstone_render::Theme;
use quillstone_script::Program;

/// The theme the script names in its front matter, or the built-in look.
pub fn load_theme(program: &Program, root: &Path) -> Theme {
    let Some(file) = &program.theme else { return Theme::default() };
    match std::fs::read_to_string(root.join(file)) {
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

/// The theme's font files (regular first), as the text renderer wants them.
pub fn load_fonts(theme: &Theme, root: &Path) -> Vec<Vec<u8>> {
    theme
        .fonts()
        .into_iter()
        .filter_map(|f| std::fs::read(root.join(f)).map_err(|e| eprintln!("warning: cannot read the font {f}: {e}")).ok())
        .collect()
}
