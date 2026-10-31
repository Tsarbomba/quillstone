//! Video decoding with FFmpeg (`ffmpeg-next`), behind the `video` feature.
//!
//! [`opener`] gives `quillstone_video::Videos` a way to open files; each clip
//! gets an [`FfmpegDecoder`] on its own thread (see `quillstone_video::Clip`).
//! Frames are decoded in software with FFmpeg's frame threads and converted
//! to RGBA for upload. Hardware decode and the clip's audio track come later.
//! Without the feature, every video reports that no decoder is built in.

use quillstone_ui::Assets;
use quillstone_video::{Decoder, OpenFn, TestPattern};

/// How the player opens video files (paths are relative to the game's files).
/// `QUILLSTONE_TEST_VIDEO=1` plays a 3-second test pattern for every video instead.
pub fn opener(assets: &Assets) -> OpenFn {
    if std::env::var_os("QUILLSTONE_TEST_VIDEO").is_some_and(|v| v == "1") {
        return Box::new(|_src: &str| Ok(Box::new(TestPattern::new(640, 360, 30, 90)) as Box<dyn Decoder>));
    }
    let assets = assets.clone();
    Box::new(move |src: &str| open(&assets.ffmpeg_url(src).map_err(|e| e.to_string())?))
}

#[cfg(not(feature = "video"))]
fn open(_url: &str) -> Result<Box<dyn Decoder>, String> {
    Err("this player was built without the `video` feature (no FFmpeg)".into())
}

/// Set FFmpeg up once (also used for Opus sound, see `ffaudio`).
#[cfg(feature = "video")]
pub fn init_ffmpeg() {
    static INIT: std::sync::Once = std::sync::Once::new();
    INIT.call_once(|| {
        let _ = ffmpeg_next::init();
        ffmpeg_next::util::log::set_level(ffmpeg_next::util::log::Level::Error);
    });
}

#[cfg(feature = "video")]
fn open(url: &str) -> Result<Box<dyn Decoder>, String> {
    Ok(Box::new(real::FfmpegDecoder::open(url)?))
}

#[cfg(feature = "video")]
mod real {
    use std::time::Duration;

    use ffmpeg_next as ff;
    use ff::format::Pixel;
    use ff::software::scaling;
    use ff::util::error::EAGAIN;
    use quillstone_video::{Decoder, Picture, TimedFrame};

    /// The RGBA converter. `ffmpeg-next` doesn't mark it `Send` because it holds
    /// a raw pointer, but a swscale context has no tie to the thread that made it.
    struct Scaler(scaling::Context);

    // SAFETY: the context is owned by one decoder and only used through
    // `&mut self` by whichever single thread owns that decoder.
    unsafe impl Send for Scaler {}

    fn err(what: &str, e: ff::Error) -> String {
        format!("{what}: {e}")
    }

    /// One video file being decoded, frame by frame, in display order.
    pub struct FfmpegDecoder {
        input: ff::format::context::Input,
        stream: usize,
        decoder: ff::decoder::Video,
        /// Converts the decoder's pixel format to RGBA; made for the first frame
        /// (and again if the size or format ever changes mid-stream).
        scaler: Option<(Scaler, Pixel, u32, u32)>,
        time_base: f64,
        /// Frames returned so far (a fallback clock for frames without a timestamp).
        count: u64,
        frame_rate: f64,
        eof_sent: bool,
    }

    impl FfmpegDecoder {
        /// `url` is a path or an FFmpeg `subfile` URL (see `Assets::ffmpeg_url`).
        pub fn open(url: &str) -> Result<FfmpegDecoder, String> {
            super::init_ffmpeg();
            let input = ff::format::input(url).map_err(|e| err("cannot open", e))?;
            let stream = input.streams().best(ff::media::Type::Video).ok_or("the file has no video stream")?;
            let index = stream.index();
            let tb = stream.time_base();
            let time_base = tb.numerator() as f64 / tb.denominator().max(1) as f64;
            let rate = stream.avg_frame_rate();
            let frame_rate = if rate.numerator() > 0 && rate.denominator() > 0 {
                rate.numerator() as f64 / rate.denominator() as f64
            } else {
                30.0
            };
            let mut context =
                ff::codec::context::Context::from_parameters(stream.parameters()).map_err(|e| err("bad stream", e))?;
            // Decode on several cores (count 0 = as many as FFmpeg thinks useful).
            context.set_threading(ff::codec::threading::Config {
                kind: ff::codec::threading::Type::Frame,
                count: 0,
            });
            let decoder = context.decoder().video().map_err(|e| err("no decoder for this video format", e))?;
            Ok(FfmpegDecoder {
                input,
                stream: index,
                decoder,
                scaler: None,
                time_base,
                count: 0,
                frame_rate,
                eof_sent: false,
            })
        }

        /// Give the decoder the next packet of our stream (or tell it the file ended).
        fn feed(&mut self) -> Result<(), String> {
            let mut packet = ff::Packet::empty();
            loop {
                match packet.read(&mut self.input) {
                    Ok(()) if packet.stream() == self.stream => {
                        return self.decoder.send_packet(&packet).map_err(|e| err("decode", e));
                    }
                    Ok(()) => continue, // audio or another stream
                    Err(ff::Error::Eof) => {
                        self.eof_sent = true;
                        return self.decoder.send_eof().map_err(|e| err("decode", e));
                    }
                    Err(e) => return Err(err("read", e)),
                }
            }
        }

        fn convert(&mut self, frame: &ff::frame::Video) -> Result<Picture, String> {
            let (format, w, h) = (frame.format(), frame.width(), frame.height());
            let stale = !matches!(&self.scaler, Some((_, f, sw, sh)) if *f == format && *sw == w && *sh == h);
            if stale {
                let ctx = scaling::Context::get(format, w, h, Pixel::RGBA, w, h, scaling::Flags::BILINEAR)
                    .map_err(|e| err("cannot convert the picture", e))?;
                self.scaler = Some((Scaler(ctx), format, w, h));
            }
            let Some((scaler, ..)) = &mut self.scaler else { unreachable!("made just above") };
            let mut rgba = ff::frame::Video::empty();
            scaler.0.run(frame, &mut rgba).map_err(|e| err("convert", e))?;
            // Rows may be padded: copy them out tightly.
            let (row, stride) = (w as usize * 4, rgba.stride(0));
            let data = rgba.data(0);
            let mut out = Vec::with_capacity(row * h as usize);
            for y in 0..h as usize {
                out.extend_from_slice(&data[y * stride..y * stride + row]);
            }
            Ok(Picture { width: w, height: h, rgba: out })
        }
    }

    impl Decoder for FfmpegDecoder {
        fn next_frame(&mut self) -> Result<Option<TimedFrame<Picture>>, String> {
            let mut frame = ff::frame::Video::empty();
            loop {
                match self.decoder.receive_frame(&mut frame) {
                    Ok(()) => {
                        let seconds = match frame.timestamp() {
                            Some(ts) => ts as f64 * self.time_base,
                            None => self.count as f64 / self.frame_rate,
                        };
                        self.count += 1;
                        let picture = self.convert(&frame)?;
                        let pts = Duration::from_secs_f64(seconds.max(0.0));
                        return Ok(Some(TimedFrame { pts, frame: picture }));
                    }
                    Err(ff::Error::Eof) => return Ok(None),
                    Err(ff::Error::Other { errno }) if errno == EAGAIN => {
                        if self.eof_sent {
                            return Ok(None);
                        }
                        self.feed()?;
                    }
                    Err(e) => return Err(err("decode", e)),
                }
            }
        }

        fn rewind(&mut self) -> Result<(), String> {
            self.input.seek(0, ..).map_err(|e| err("cannot go back to the start", e))?;
            self.decoder.flush();
            self.eof_sent = false;
            self.count = 0;
            Ok(())
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use std::path::Path;

        fn clip(name: &str) -> String {
            Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data").join(name).to_string_lossy().into_owned()
        }

        fn all_frames(d: &mut FfmpegDecoder) -> Vec<TimedFrame<Picture>> {
            std::iter::from_fn(|| d.next_frame().unwrap()).collect()
        }

        // (bars-av1.webm is there to be refused by `quillstone check`: the slim FFmpeg
        // games ship with has no AV1, so it isn't decoded here.)
        #[test]
        fn decodes_a_vp9_clip_in_order() {
            let mut d = FfmpegDecoder::open(&clip("bars-vp9.webm")).unwrap();
            let frames = all_frames(&mut d);
            assert_eq!(frames.len(), 12, "1.2 s at 10 fps");
            let f = &frames[0].frame;
            assert_eq!((f.width, f.height, f.rgba.len()), (64, 36, 64 * 36 * 4));
            assert!(frames.windows(2).all(|w| w[0].pts < w[1].pts), "rising timestamps");
            assert!((frames[11].pts.as_secs_f64() - 1.1).abs() < 0.01);
            assert_eq!(d.next_frame().unwrap(), None, "stays at the end");
        }

        #[test]
        fn rewinding_starts_over() {
            let mut d = FfmpegDecoder::open(&clip("bars-vp9.webm")).unwrap();
            let first = all_frames(&mut d);
            d.rewind().unwrap();
            let again = all_frames(&mut d);
            assert_eq!(again.len(), first.len());
            assert_eq!(again[0], first[0]);
        }

        #[test]
        fn missing_files_are_errors_not_panics() {
            assert!(FfmpegDecoder::open(&clip("nope.webm")).is_err());
            assert!(FfmpegDecoder::open(&Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml").to_string_lossy()).is_err());
        }

        /// A clip inside a game archive is read through FFmpeg's `subfile` protocol (see `Assets::ffmpeg_url`).
        #[test]
        fn a_clip_inside_an_archive_decodes_like_the_loose_file() {
            let dir = std::env::temp_dir().join(format!("qs-video-subfile-{}", std::process::id()));
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(dir.join("main.quill.md"), "Hi.\n").unwrap();
            let list = [
                ("main.quill.md".to_string(), dir.join("main.quill.md")),
                ("bars.webm".to_string(), clip("bars-vp9.webm").into()),
            ];
            quillstone_ui::archive::write(&dir.join("game.qsa"), "main.quill.md", &list).unwrap();
            let archive = quillstone_ui::archive::Archive::open(&dir.join("game.qsa")).unwrap();
            let assets = Assets::archive(archive, None);
            let mut d = FfmpegDecoder::open(&assets.ffmpeg_url("bars.webm").unwrap()).unwrap();
            assert_eq!(all_frames(&mut d).len(), 12);
            d.rewind().unwrap();
            assert_eq!(all_frames(&mut d).len(), 12, "seeking works inside the range");
            let _ = std::fs::remove_dir_all(dir);
        }

        #[test]
        fn the_demo_flashback_plays_through_the_pipeline() {
            let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../examples/assets/flashback.webm");
            let d = FfmpegDecoder::open(&path.to_string_lossy()).unwrap();
            let mut c = quillstone_video::Clip::start(Box::new(d), false);
            assert!(c.wait_buffered(3, Duration::from_secs(5)));
            assert!(c.advance(Duration::ZERO));
            assert_eq!(c.frame().map(|p| (p.width, p.height)), Some((1280, 720)));
        }
    }
}
