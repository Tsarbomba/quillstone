//! Pictures and fonts for `check`: what the player can load, whatever the file is called.

use std::path::Path;

use quillstone_script::PICTURE_EXTENSIONS; // pictures the player loads (never a video where a picture is drawn)

/// Opens with one of these when the file is a font cosmic-text can read
/// (TrueType, OpenType/CFF, Apple's `true`, a collection).
const FONT_MAGIC: [&[u8; 4]; 4] = [&[0, 1, 0, 0], b"OTTO", b"true", b"ttcf"];

fn extension(file: &Path) -> String {
    file.extension().map(|e| e.to_string_lossy().to_ascii_lowercase()).unwrap_or_default()
}

/// The size of a picture the player can show, or why it can't (a `.webm` is no picture, however it is used).
pub fn read_picture(file: &Path) -> Result<(u32, u32), String> {
    let ext = extension(file);
    if !PICTURE_EXTENSIONS.contains(&ext.as_str()) {
        let what = if ext.is_empty() { "a file without an extension".to_string() } else { format!("`.{ext}`") };
        return Err(format!("must be a PNG, JPEG or WebP picture, not {what}"));
    }
    // The whole file: JPEG metadata before the size can be long.
    let data = std::fs::read(file).map_err(|e| format!("cannot read it: {e}"))?;
    match quillstone_render::image_probe::unshowable_image(&data, &ext) {
        Some(why) => Err(why),
        None => Ok(quillstone_render::image_probe::image_size(&data, &ext).unwrap_or((0, 0))),
    }
}

/// Why a theme font can't be used, or `None`.
pub fn font_problem(file: &Path) -> Option<String> {
    let ext = extension(file);
    if !matches!(ext.as_str(), "ttf" | "otf") {
        return Some(format!("must be a .ttf or .otf font file, not `.{ext}`"));
    }
    let mut head = [0u8; 4];
    let read = std::fs::File::open(file).and_then(|mut f| std::io::Read::read_exact(&mut f, &mut head));
    match read {
        Ok(()) if FONT_MAGIC.iter().any(|m| **m == head) => None,
        Ok(()) => Some("not a TrueType or OpenType font (or a damaged one), despite its name".into()),
        Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => Some("too short to be a font file".into()),
        Err(e) => Some(format!("cannot read it: {e}")),
    }
}

/// A PNG header of the given size (for tests).
#[cfg(test)]
pub fn fake_png(w: u32, h: u32) -> Vec<u8> {
    let mut d = b"\x89PNG\r\n\x1a\n\0\0\0\x0dIHDR".to_vec();
    d.extend(w.to_be_bytes());
    d.extend(h.to_be_bytes());
    d.extend([8, 6, 0, 0, 0]);
    d
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sizes_come_from_the_header() {
        let dir = std::env::temp_dir().join(format!("quillstone-picture-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("a.png"), fake_png(640, 480)).unwrap();
        assert_eq!(read_picture(&dir.join("a.png")), Ok((640, 480)));
        std::fs::write(dir.join("a.webm"), fake_png(1, 1)).unwrap();
        assert!(read_picture(&dir.join("a.webm")).unwrap_err().contains("must be a PNG, JPEG or WebP picture"));
        std::fs::write(dir.join("f.ttf"), [0, 1, 0, 0, 9]).unwrap();
        std::fs::write(dir.join("g.ttf"), b"nope").unwrap();
        assert_eq!(font_problem(&dir.join("f.ttf")), None);
        assert!(font_problem(&dir.join("g.ttf")).is_some() && font_problem(&dir.join("a.png")).is_some());
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
