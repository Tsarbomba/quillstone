//! Whether an image file is one the player can show, from its first bytes
//! (for `quillstone check`; the player itself decodes PNG, JPEG and WebP).

/// The largest width or height the player uploads (wgpu's default limit,
/// which every GPU it runs on supports).
pub const MAX_IMAGE_SIDE: u32 = 8192;

/// Why the image can't be shown, or `None` if it can. `ext` is the file
/// extension in lowercase; `data` is the whole file.
pub fn unshowable_image(data: &[u8], ext: &str) -> Option<String> {
    let format = match ext {
        "png" => "PNG",
        "jpg" | "jpeg" => "JPEG",
        "webp" => "WebP",
        _ => {
            return Some(format!("`.{ext}` images aren't supported (use PNG, JPEG or WebP)"));
        }
    };
    let size = match format {
        "PNG" => png_size(data),
        "JPEG" => jpeg_size(data),
        _ => webp_size(data),
    };
    match size {
        None => Some(format!("not a {format} file (or a damaged one), despite its name")),
        Some((w, h)) if w > MAX_IMAGE_SIDE || h > MAX_IMAGE_SIDE => {
            Some(format!("it is {w}x{h}; images can be at most {MAX_IMAGE_SIDE} pixels on a side"))
        }
        Some(_) => None,
    }
}

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

/// PNG: the signature, then the IHDR chunk with width and height.
fn png_size(d: &[u8]) -> Option<(u32, u32)> {
    if !d.starts_with(b"\x89PNG\r\n\x1a\n") || d.get(12..16)? != b"IHDR" {
        return None;
    }
    let w = u32::from_be_bytes(d.get(16..20)?.try_into().ok()?);
    let h = u32::from_be_bytes(d.get(20..24)?.try_into().ok()?);
    Some((w, h))
}

/// JPEG: walk the segments to the frame header (SOFn).
fn jpeg_size(d: &[u8]) -> Option<(u32, u32)> {
    if !d.starts_with(&[0xFF, 0xD8]) {
        return None;
    }
    let mut i = 2;
    loop {
        if *d.get(i)? != 0xFF {
            return None;
        }
        let marker = *d.get(i + 1)?;
        match marker {
            0xFF => i += 1, // padding
            0x01 | 0xD0..=0xD7 => i += 2, // no length
            0xC0..=0xCF if !matches!(marker, 0xC4 | 0xC8 | 0xCC) => {
                return Some((be16(d, i + 7)?, be16(d, i + 5)?));
            }
            0xD9 | 0xDA => return None, // end, or image data before any frame header
            _ => i += 2 + be16(d, i + 2)? as usize,
        }
    }
}

/// WebP: a RIFF file whose first chunk is lossy (`VP8 `), lossless (`VP8L`) or extended (`VP8X`).
fn webp_size(d: &[u8]) -> Option<(u32, u32)> {
    if d.get(0..4)? != b"RIFF" || d.get(8..12)? != b"WEBP" {
        return None;
    }
    match d.get(12..16)? {
        b"VP8 " if d.get(23..26)? == [0x9D, 0x01, 0x2A] => Some((le16(d, 26)? & 0x3FFF, le16(d, 28)? & 0x3FFF)),
        b"VP8L" if *d.get(20)? == 0x2F => {
            let bits = u32::from_le_bytes(d.get(21..25)?.try_into().ok()?);
            Some(((bits & 0x3FFF) + 1, ((bits >> 14) & 0x3FFF) + 1))
        }
        b"VP8X" => Some((le24(d, 24)? + 1, le24(d, 27)? + 1)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn png(w: u32, h: u32) -> Vec<u8> {
        let mut d = b"\x89PNG\r\n\x1a\n\0\0\0\x0dIHDR".to_vec();
        d.extend(w.to_be_bytes());
        d.extend(h.to_be_bytes());
        d.extend([8, 6, 0, 0, 0]);
        d
    }

    #[test]
    fn png_sizes_and_limits() {
        assert_eq!(png_size(&png(1920, 1080)), Some((1920, 1080)));
        assert_eq!(unshowable_image(&png(1920, 1080), "png"), None);
        assert!(unshowable_image(&png(9000, 100), "png").unwrap().contains("9000x100"));
        assert!(unshowable_image(b"GIF89a....", "png").unwrap().contains("not a PNG"));
    }

    #[test]
    fn jpeg_skips_to_the_frame_header() {
        // SOI, an APP0 segment (length 16), then SOF0: length, precision, height 600, width 800.
        let mut d = vec![0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0x10];
        d.extend([0; 14]);
        d.extend([0xFF, 0xC0, 0x00, 0x11, 0x08, 0x02, 0x58, 0x03, 0x20, 0x03]);
        assert_eq!(jpeg_size(&d), Some((800, 600)));
        assert_eq!(unshowable_image(&d, "jpeg"), None);
        assert_eq!(jpeg_size(&[0xFF, 0xD8, 0xFF, 0xD9]), None);
    }

    #[test]
    fn webp_all_three_kinds() {
        let riff = |chunk: &[u8], body: &[u8]| {
            let mut d = b"RIFF\0\0\0\0WEBP".to_vec();
            d.extend(chunk);
            d.extend([0; 4]); // chunk size
            d.extend(body);
            d
        };
        let lossy = riff(b"VP8 ", &[0, 0, 0, 0x9D, 0x01, 0x2A, 0x80, 0x07, 0x38, 0x04]);
        assert_eq!(webp_size(&lossy), Some((1920, 1080)));
        let bits: u32 = 1919 | (1079 << 14);
        let mut body = vec![0x2F];
        body.extend(bits.to_le_bytes());
        assert_eq!(webp_size(&riff(b"VP8L", &body)), Some((1920, 1080)));
        let ext = riff(b"VP8X", &[0, 0, 0, 0, 0x7F, 0x07, 0x00, 0x37, 0x04, 0x00]);
        assert_eq!(webp_size(&ext), Some((1920, 1080)));
    }

    #[test]
    fn other_formats_are_named() {
        let why = unshowable_image(b"GIF89a", "gif").unwrap();
        assert!(why.contains("`.gif`") && why.contains("PNG, JPEG or WebP"), "{why}");
    }
}
