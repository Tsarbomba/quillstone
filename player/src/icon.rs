//! The window icon: the game's `icon:` picture (or the Quillstone icon), shrunk to what a title bar and taskbar draw.

use quillstone_script::Program;
use quillstone_ui::Assets;

/// Biggest side of the window icon: Windows draws it at 16 to 48 px, so a 256 px picture only costs memory.
const WINDOW_ICON_SIDE: u32 = 64;
const MAX_ICON_BYTES: u64 = 16 << 20;
/// The Quillstone icon (256x256) a game without `icon:` gets.
const DEFAULT_ICON: &[u8] = include_bytes!("../../assets/quillstone-icon-256.png");

/// A picture (PNG, JPEG or WebP) as straight RGBA pixels and their size, shrunk to fit `WINDOW_ICON_SIDE`.
fn icon_pixels(picture: &[u8]) -> Result<(Vec<u8>, u32, u32), String> {
    let mut img = image::load_from_memory(picture).map_err(|e| e.to_string())?;
    if img.width().max(img.height()) > WINDOW_ICON_SIDE {
        img = img.resize(WINDOW_ICON_SIDE, WINDOW_ICON_SIDE, image::imageops::FilterType::Lanczos3);
    }
    let rgba = img.to_rgba8();
    let (w, h) = (rgba.width(), rgba.height());
    Ok((rgba.into_raw(), w, h))
}

fn window_icon(picture: &[u8]) -> Result<winit::window::Icon, String> {
    let (rgba, w, h) = icon_pixels(picture)?;
    winit::window::Icon::from_rgba(rgba, w, h).map_err(|e| e.to_string())
}

/// The window icon the script's `icon:` names, else the Quillstone icon. A picture that can't be read only costs a
/// warning, and the Quillstone icon takes its place.
pub fn load_icon(program: &Program, assets: &Assets) -> Option<winit::window::Icon> {
    if let Some((_, file)) = program.icon.as_ref() {
        let icon = assets.read_limited(file, MAX_ICON_BYTES).map_err(|e| e.to_string()).and_then(|b| window_icon(&b));
        match icon {
            Ok(icon) => return Some(icon),
            Err(e) => eprintln!("warning: cannot use the icon {file}: {e}"),
        }
    }
    window_icon(DEFAULT_ICON).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn png(w: u32, h: u32) -> Vec<u8> {
        let mut out = std::io::Cursor::new(Vec::new());
        image::RgbaImage::from_pixel(w, h, image::Rgba([10, 20, 30, 255])).write_to(&mut out, image::ImageFormat::Png).unwrap();
        out.into_inner()
    }

    #[test]
    fn a_big_icon_is_shrunk_keeping_its_shape_and_a_small_one_is_kept() {
        let (rgba, w, h) = icon_pixels(&png(256, 256)).unwrap();
        assert_eq!((w, h, rgba.len()), (64, 64, 64 * 64 * 4));
        assert_eq!(&rgba[..4], [10, 20, 30, 255]);
        let size = |w, h| icon_pixels(&png(w, h)).map(|(_, w, h)| (w, h)).unwrap();
        assert_eq!(size(512, 256), (64, 32));
        assert_eq!(size(32, 32), (32, 32));
        assert!(winit::window::Icon::from_rgba(rgba, w, h).is_ok());
    }

    #[test]
    fn the_built_in_icon_is_a_valid_window_icon() {
        assert!(window_icon(DEFAULT_ICON).is_ok());
    }

    #[test]
    fn something_that_is_no_picture_is_an_error_not_a_crash() {
        assert!(icon_pixels(b"not a picture").is_err());
    }
}
