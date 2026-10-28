//! Pictures and fonts for `check`: what the player can load, whatever the file is called.

use std::path::Path;

/// Pictures the player loads (it never plays a video where a picture is drawn).
const PICTURE_EXTENSIONS: [&str; 4] = ["png", "jpg", "jpeg", "webp"];

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
        None => Ok(size(&data, &ext).unwrap_or((0, 0))),
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

// The header readers mirror `quillstone_render::image_probe` (which keeps them private).

fn be16(d: &[u8], at: usize) -> Option<u32> {
    Some(u16::from_be_bytes(d.get(at..at + 2)?.try_into().ok()?) as u32)
}

fn le16(d: &[u8], at: usize) -> Option<u32> {
    Some(u16::from_le_bytes(d.get(at..at + 2)?.try_into().ok()?) as u32)
}

fn le24(d: &[u8], at: usize) -> Option<u32> {
    let b = d.get(at..at + 3)?;
    Some(b[0] as u32 | (b[1] as u32) << 8 | (b[2] as u32) << 16)
}

fn size(d: &[u8], ext: &str) -> Option<(u32, u32)> {
    match ext {
        "png" => Some((u32::from_be_bytes(d.get(16..20)?.try_into().ok()?), u32::from_be_bytes(d.get(20..24)?.try_into().ok()?))),
        "jpg" | "jpeg" => jpeg(d),
        _ => webp(d),
    }
}

fn jpeg(d: &[u8]) -> Option<(u32, u32)> {
    let mut i = 2;
    loop {
        if *d.get(i)? != 0xFF {
            return None;
        }
        let marker = *d.get(i + 1)?;
        match marker {
            0xFF => i += 1,
            0x01 | 0xD0..=0xD7 => i += 2,
            0xC0..=0xCF if !matches!(marker, 0xC4 | 0xC8 | 0xCC) => return Some((be16(d, i + 7)?, be16(d, i + 5)?)),
            0xD9 | 0xDA => return None,
            _ => i += 2 + be16(d, i + 2)? as usize,
        }
    }
}

fn webp(d: &[u8]) -> Option<(u32, u32)> {
    match d.get(12..16)? {
        b"VP8 " => Some((le16(d, 26)? & 0x3FFF, le16(d, 28)? & 0x3FFF)),
        b"VP8L" => {
            let bits = u32::from_le_bytes(d.get(21..25)?.try_into().ok()?);
            Some(((bits & 0x3FFF) + 1, ((bits >> 14) & 0x3FFF) + 1))
        }
        b"VP8X" => Some((le24(d, 24)? + 1, le24(d, 27)? + 1)),
        _ => None,
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
