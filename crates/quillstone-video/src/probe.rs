//! What's inside a video or sound file, read from its first bytes: enough to tell
//! a game's author, before shipping, that a file won't play. Videos must suit the
//! slim FFmpeg build (`tools/build-ffmpeg.sh`): WebM with VP8 or VP9, sound in
//! Vorbis or Opus. Developers build against a full FFmpeg, where an MP4 would play
//! fine; without this check it would first fail on players' machines. Sound files
//! are WAV, MP3, FLAC, Ogg Vorbis (kira) or Opus (FFmpeg).

/// How many bytes of a file [`probe`] wants: track headers sit at the start.
pub const PROBE_BYTES: usize = 64 * 1024;

/// Codecs a game's videos can use (video, then sound). Names as reported by [`probe`].
pub const VIDEO_CODECS: [&str; 4] = ["VP8", "VP9", "Vorbis", "Opus"];

/// The container and the codecs of its tracks, from the start of a file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Probed {
    pub container: &'static str,
    /// Track codecs (video and sound), in file order. Subtitle tracks are skipped.
    pub codecs: Vec<String>,
}

/// Look at the first bytes of a video file (up to [`PROBE_BYTES`]).
pub fn probe(head: &[u8]) -> Probed {
    if head.starts_with(&[0x1a, 0x45, 0xdf, 0xa3]) {
        let webm = find(head, b"webm").is_some_and(|i| i < 64);
        return Probed { container: if webm { "WebM" } else { "Matroska" }, codecs: matroska_codecs(head) };
    }
    if head.starts_with(b"OggS") {
        return Probed { container: "Ogg", codecs: ogg_codecs(head) };
    }
    let container = if head.get(4..8) == Some(b"ftyp") {
        "MP4/MOV"
    } else if head.starts_with(b"RIFF") && head.get(8..11) == Some(b"AVI") {
        "AVI"
    } else if head.first() == Some(&0x47) || head.starts_with(&[0, 0, 1, 0xba]) {
        "MPEG"
    } else if head.starts_with(b"FLV") {
        "FLV"
    } else {
        "an unknown format"
    };
    Probed { container, codecs: Vec::new() }
}

/// Why a video won't play in a shipped game, or `None` if it will.
pub fn unplayable_video(head: &[u8]) -> Option<String> {
    let p = probe(head);
    if !matches!(p.container, "WebM" | "Matroska") {
        return Some(format!("it is {}; games play WebM (.webm) videos", p.container));
    }
    let bad: Vec<&str> = p.codecs.iter().map(String::as_str).filter(|c| !VIDEO_CODECS.contains(c)).collect();
    if !bad.is_empty() {
        return Some(format!("it uses {}; games play VP8 or VP9 video with Vorbis or Opus sound", bad.join(" and ")));
    }
    if !p.codecs.iter().any(|c| c == "VP8" || c == "VP9") {
        return Some(format!("no video track was found at the start of this {} file", p.container));
    }
    None
}

/// Why a sound file (music, sound effect, voice) won't play, or `None` if it will.
/// `extension` is the file's, lowercase.
pub fn unplayable_sound(head: &[u8], extension: &str) -> Option<String> {
    const USE: &str = "use .ogg (Vorbis), .opus, .wav, .mp3 or .flac";
    match extension {
        "wav" | "mp3" | "flac" => None,
        "ogg" | "oga" | "opus" => {
            let p = probe(head);
            if p.container != "Ogg" {
                return Some(format!("it is {}, not an Ogg file; {USE}", p.container));
            }
            match p.codecs.iter().find(|c| *c != "Vorbis" && *c != "Opus") {
                Some(bad) => Some(format!("it holds {bad}; Ogg sound files must be Vorbis or Opus")),
                None if p.codecs.is_empty() => Some("no sound track was found at the start of this Ogg file".into()),
                None => None,
            }
        }
        "m4a" | "aac" => Some(format!("AAC isn't supported (it is patent-licensed); {USE}")),
        other => Some(format!(".{other} isn't a sound format games play; {USE}")),
    }
}

fn find(hay: &[u8], needle: &[u8]) -> Option<usize> {
    hay.windows(needle.len()).position(|w| w == needle)
}

/// Matroska keeps each track's codec as a `CodecID` element (ID 0x86, a size, then
/// text like `V_VP9`). Rather than walk the whole element tree, look for that shape.
fn matroska_codecs(head: &[u8]) -> Vec<String> {
    let mut codecs = Vec::new();
    let mut i = 0;
    while i + 2 < head.len() {
        let size = head[i + 1];
        if head[i] == 0x86 && size & 0x80 != 0 {
            let len = (size & 0x7f) as usize;
            if let Some(id) = head.get(i + 2..i + 2 + len) {
                let text = id.iter().all(|b| b.is_ascii_alphanumeric() || b"_/.-".contains(b));
                if text && len >= 3 && (id.starts_with(b"V_") || id.starts_with(b"A_")) {
                    codecs.push(matroska_name(std::str::from_utf8(id).expect("checked ASCII")));
                    i += 2 + len;
                    continue;
                }
            }
        }
        i += 1;
    }
    codecs
}

fn matroska_name(id: &str) -> String {
    match id {
        "V_VP8" => "VP8",
        "V_VP9" => "VP9",
        "V_THEORA" => "Theora",
        "A_VORBIS" => "Vorbis",
        "A_OPUS" => "Opus",
        "V_AV1" => "AV1",
        "V_MPEG4/ISO/AVC" => "H.264",
        "V_MPEGH/ISO/HEVC" => "H.265",
        "A_AAC" => "AAC",
        "A_MPEG/L3" => "MP3",
        "A_FLAC" => "FLAC",
        other => other,
    }
    .to_string()
}

/// Ogg starts each stream with a "beginning of stream" page whose first packet
/// names the codec (`\x80theora`, `\x01vorbis`, `OpusHead`...).
fn ogg_codecs(head: &[u8]) -> Vec<String> {
    let mut codecs = Vec::new();
    let mut at = 0;
    while let Some(page) = head.get(at..).filter(|p| p.starts_with(b"OggS") && p.len() >= 27) {
        let first_page_of_stream = page[5] & 0x02 != 0;
        if !first_page_of_stream {
            break; // every stream's first page comes before any other page
        }
        let segments = page[26] as usize;
        let Some(table) = page.get(27..27 + segments) else { break };
        let body: usize = table.iter().map(|&s| s as usize).sum();
        let Some(packet) = page.get(27 + segments..27 + segments + body) else { break }; // cut off
        let name = if packet.starts_with(b"\x80theora") {
            Some("Theora".to_string())
        } else if packet.starts_with(b"\x01vorbis") {
            Some("Vorbis".to_string())
        } else if packet.starts_with(b"OpusHead") {
            Some("Opus".to_string())
        } else if packet.starts_with(b"fishead\0") {
            None // Ogg Skeleton: an index, not a track
        } else if packet.starts_with(b"\x7fFLAC") {
            Some("FLAC".to_string())
        } else if packet.starts_with(b"Speex") {
            Some("Speex".to_string())
        } else if packet.starts_with(b"\x80daala") {
            Some("Daala".to_string())
        } else {
            Some("an unknown codec".to_string())
        };
        codecs.extend(name);
        at += 27 + segments + body;
    }
    codecs
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file(path: &str) -> Vec<u8> {
        let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..").join(path);
        let mut bytes = std::fs::read(&p).unwrap_or_else(|e| panic!("{}: {e}", p.display()));
        bytes.truncate(PROBE_BYTES);
        bytes
    }

    /// One Ogg page that starts a stream, holding one packet.
    fn ogg_page(packet: &[u8], first: bool) -> Vec<u8> {
        let mut p = b"OggS\0".to_vec();
        p.push(if first { 0x02 } else { 0 });
        p.extend([0; 20]); // granule, serial, sequence, checksum
        assert!(packet.len() < 255);
        p.push(1);
        p.push(packet.len() as u8);
        p.extend(packet);
        p
    }

    #[test]
    fn reads_the_codecs_of_real_webm_files() {
        let vp9 = probe(&file("player/tests/data/bars-vp9.webm"));
        assert_eq!(vp9, Probed { container: "WebM", codecs: vec!["VP9".into(), "Opus".into()] });
        assert_eq!(unplayable_video(&file("examples/assets/flashback.webm")), None);
        let av1 = unplayable_video(&file("player/tests/data/bars-av1.webm")).unwrap();
        assert!(av1.contains("uses AV1"), "{av1}");
        let calm = file("examples/assets/calm.ogg");
        assert_eq!(probe(&calm).codecs, ["Vorbis"]);
        assert_eq!(unplayable_sound(&calm, "ogg"), None);
        assert!(unplayable_video(&calm).unwrap().contains("it is Ogg"), "a sound file is not a video");
    }

    #[test]
    fn reads_ogg_streams_up_to_the_first_data_page() {
        let mut ogv = ogg_page(b"fishead\0rest", true);
        ogv.extend(ogg_page(b"\x80theora header", true));
        ogv.extend(ogg_page(b"\x01vorbis header", true));
        ogv.extend(ogg_page(b"\x7fFLAC not a stream start", false));
        assert_eq!(probe(&ogv).codecs, ["Theora", "Vorbis"]);
        assert!(unplayable_video(&ogv).unwrap().contains("games play WebM"), "Ogg video (Theora) isn't shipped");
        assert_eq!(probe(&ogv[..30]).codecs, Vec::<String>::new(), "a cut-off page is not a crash");

        let opus = ogg_page(b"OpusHead header", true);
        assert_eq!(unplayable_sound(&opus, "opus"), None);
        assert_eq!(unplayable_sound(&opus, "ogg"), None, "Opus in a .ogg file is fine too");
        let flac = ogg_page(b"\x7fFLAC header", true);
        assert!(unplayable_sound(&flac, "ogg").unwrap().contains("holds FLAC"));
    }

    #[test]
    fn other_containers_are_named() {
        let mp4 = [&[0, 0, 0, 0x20][..], b"ftypisom", &[0; 20]].concat();
        assert!(unplayable_video(&mp4).unwrap().contains("MP4/MOV"));
        assert!(unplayable_video(b"RIFF\0\0\0\0AVI LIST").unwrap().contains("AVI"));
        assert!(unplayable_video(b"hello").unwrap().contains("unknown"));
        assert!(unplayable_video(&[0x1a, 0x45, 0xdf, 0xa3, 0x80]).unwrap().contains("no video track"));
        assert!(unplayable_video(&[]).is_some());
    }

    #[test]
    fn sound_files_are_judged_by_extension_then_content() {
        for ok in ["wav", "mp3", "flac"] {
            assert_eq!(unplayable_sound(b"", ok), None, "{ok} is kira's");
        }
        assert!(unplayable_sound(b"", "m4a").unwrap().contains("AAC"));
        assert!(unplayable_sound(b"", "wma").unwrap().contains(".wma isn't"));
        let mp4 = [&[0, 0, 0, 0x20][..], b"ftypM4A ", &[0; 20]].concat();
        assert!(unplayable_sound(&mp4, "ogg").unwrap().contains("not an Ogg file"), "renamed files are caught");
    }
}
