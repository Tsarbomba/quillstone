//! Opus sound files (`.opus`, or Opus inside `.ogg`), decoded by FFmpeg for kira.
//!
//! kira's own decoder (symphonia) has no Opus, but the game already ships FFmpeg
//! for video, and its Opus decoder is as good as any. [`open`] streams (music and
//! voice), [`load`] decodes a whole file up front (sound effects). Everything else
//! still goes through kira. Needs the `video` feature, which brings FFmpeg.

use kira::sound::FromFileError;
use quillstone_ui::Assets;

/// Whether a file needs FFmpeg rather than kira: Opus, by extension or content.
pub fn wants_ffmpeg(assets: &Assets, src: &str) -> bool {
    if src.rsplit_once('.').is_some_and(|(_, e)| e.eq_ignore_ascii_case("opus")) {
        return true;
    }
    let mut head = [0u8; 512];
    let n = assets.open(src).and_then(|mut f| std::io::Read::read(&mut f, &mut head)).unwrap_or(0);
    quillstone_video::probe::probe(&head[..n]).codecs.iter().any(|c| c == "Opus")
}

fn error(message: impl std::fmt::Display) -> FromFileError {
    FromFileError::IoError(std::io::Error::other(message.to_string()))
}

#[cfg(not(feature = "video"))]
mod imp {
    use super::*;
    use kira::sound::static_sound::StaticSoundData;
    use kira::sound::streaming::StreamingSoundData;

    const NO_FFMPEG: &str = "Opus sound needs FFmpeg, and this player was built without the `video` feature";

    pub fn open(_assets: &Assets, _src: &str) -> Result<StreamingSoundData<FromFileError>, FromFileError> {
        Err(error(NO_FFMPEG))
    }

    pub fn load(_assets: &Assets, _src: &str) -> Result<StaticSoundData, FromFileError> {
        Err(error(NO_FFMPEG))
    }
}

#[cfg(feature = "video")]
mod imp {
    use super::*;
    use std::sync::Arc;

    use ffmpeg_next as ff;
    use ff::format::sample::Type as Layout;
    use ff::format::Sample;
    use ff::util::error::EAGAIN;
    use kira::sound::static_sound::{StaticSoundData, StaticSoundSettings};
    use kira::sound::streaming::{Decoder, StreamingSoundData};
    use kira::Frame;

    pub fn open(assets: &Assets, src: &str) -> Result<StreamingSoundData<FromFileError>, FromFileError> {
        Ok(StreamingSoundData::from_decoder(FfmpegAudio::open(&assets.ffmpeg_url(src).map_err(error)?)?))
    }

    pub fn load(assets: &Assets, src: &str) -> Result<StaticSoundData, FromFileError> {
        let mut d = FfmpegAudio::open(&assets.ffmpeg_url(src).map_err(error)?)?;
        let mut frames = Vec::with_capacity(d.length);
        while let Some(chunk) = d.next_chunk()? {
            frames.extend(chunk);
        }
        Ok(StaticSoundData {
            sample_rate: d.rate,
            frames: Arc::from(frames),
            settings: StaticSoundSettings::default(),
            slice: None,
        })
    }

    /// One sound file being decoded in order. kira asks for chunks and, to loop,
    /// seeks back.
    struct FfmpegAudio {
        input: ff::format::context::Input,
        stream: usize,
        decoder: ff::decoder::Audio,
        rate: u32,
        /// The length the file states, in frames. kira trusts it: asking for a frame
        /// before it that never comes would spin forever, so a short file is padded.
        length: usize,
        /// Frames handed out so far.
        position: usize,
        /// Frames still to drop after a seek (seeking goes to the start, then forward).
        skip: usize,
        eof_sent: bool,
    }

    impl FfmpegAudio {
        /// `url` is a path or an FFmpeg `subfile` URL (see `Assets::ffmpeg_url`).
        fn open(url: &str) -> Result<FfmpegAudio, FromFileError> {
            crate::video::init_ffmpeg();
            let input = ff::format::input(url).map_err(|e| error(format!("cannot open: {e}")))?;
            let stream = input.streams().best(ff::media::Type::Audio).ok_or(FromFileError::NoDefaultTrack)?;
            let index = stream.index();
            let context = ff::codec::context::Context::from_parameters(stream.parameters())
                .map_err(|e| error(format!("bad stream: {e}")))?;
            let decoder = context.decoder().audio().map_err(|e| error(format!("no decoder for this sound: {e}")))?;
            let rate = decoder.rate();
            if rate == 0 {
                return Err(FromFileError::UnknownSampleRate);
            }
            let tb = stream.time_base();
            let seconds = if stream.duration() > 0 && tb.denominator() > 0 {
                stream.duration() as f64 * tb.numerator() as f64 / tb.denominator() as f64
            } else if input.duration() > 0 {
                input.duration() as f64 / f64::from(ff::ffi::AV_TIME_BASE)
            } else {
                return Err(FromFileError::UnknownDuration);
            };
            let length = (seconds * f64::from(rate)).round() as usize;
            Ok(FfmpegAudio { input, stream: index, decoder, rate, length, position: 0, skip: 0, eof_sent: false })
        }

        /// The next decoded frames, or `None` at the end.
        fn next_chunk(&mut self) -> Result<Option<Vec<Frame>>, FromFileError> {
            let mut frame = ff::frame::Audio::empty();
            loop {
                match self.decoder.receive_frame(&mut frame) {
                    Ok(()) => {
                        let mut out = to_frames(&frame)?;
                        let dropped = self.skip.min(out.len());
                        out.drain(..dropped);
                        self.skip -= dropped;
                        if !out.is_empty() {
                            return Ok(Some(out));
                        }
                    }
                    Err(ff::Error::Eof) => return Ok(None),
                    Err(ff::Error::Other { errno }) if errno == EAGAIN => {
                        if self.eof_sent {
                            return Ok(None);
                        }
                        self.feed()?;
                    }
                    Err(e) => return Err(error(format!("decode: {e}"))),
                }
            }
        }

        fn feed(&mut self) -> Result<(), FromFileError> {
            let mut packet = ff::Packet::empty();
            loop {
                match packet.read(&mut self.input) {
                    Ok(()) if packet.stream() == self.stream => {
                        return self.decoder.send_packet(&packet).map_err(|e| error(format!("decode: {e}")));
                    }
                    Ok(()) => continue,
                    Err(ff::Error::Eof) => {
                        self.eof_sent = true;
                        return self.decoder.send_eof().map_err(|e| error(format!("decode: {e}")));
                    }
                    Err(e) => return Err(error(format!("read: {e}"))),
                }
            }
        }
    }

    impl Decoder for FfmpegAudio {
        type Error = FromFileError;

        fn sample_rate(&self) -> u32 {
            self.rate
        }

        fn num_frames(&self) -> usize {
            self.length
        }

        fn decode(&mut self) -> Result<Vec<Frame>, FromFileError> {
            let chunk = match self.next_chunk()? {
                Some(chunk) => chunk,
                // Shorter than it said: fill up with silence (see `length`).
                None => vec![Frame::ZERO; self.length.saturating_sub(self.position).min(4096)],
            };
            self.position += chunk.len();
            Ok(chunk)
        }

        fn seek(&mut self, index: usize) -> Result<usize, FromFileError> {
            // Back to the start and forward from there: exact, and loops (the one
            // seek a game makes) start at 0 anyway.
            self.input.seek(0, ..).map_err(|e| error(format!("cannot go back to the start: {e}")))?;
            self.decoder.flush();
            self.eof_sent = false;
            self.skip = index;
            self.position = index;
            Ok(index)
        }
    }

    /// FFmpeg's samples (any common format, planar or packed) as kira's stereo frames.
    /// Mono plays on both sides; beyond two channels, the front left and right are used.
    fn to_frames(frame: &ff::frame::Audio) -> Result<Vec<Frame>, FromFileError> {
        let (n, channels) = (frame.samples(), usize::from(frame.channels().max(1)));
        let (size, read): (usize, fn(&[u8]) -> f32) = match frame.format() {
            Sample::F32(_) => (4, |b| f32::from_ne_bytes(b.try_into().expect("4 bytes"))),
            Sample::F64(_) => (8, |b| f64::from_ne_bytes(b.try_into().expect("8 bytes")) as f32),
            Sample::I16(_) => (2, |b| f32::from(i16::from_ne_bytes(b.try_into().expect("2 bytes"))) / 32768.0),
            Sample::I32(_) => (4, |b| i32::from_ne_bytes(b.try_into().expect("4 bytes")) as f32 / 2_147_483_648.0),
            Sample::U8(_) => (1, |b| (f32::from(b[0]) - 128.0) / 128.0),
            other => return Err(error(format!("unsupported sample format {other:?}"))),
        };
        let planar = matches!(frame.format(), Sample::F32(Layout::Planar) | Sample::F64(Layout::Planar)
            | Sample::I16(Layout::Planar) | Sample::I32(Layout::Planar) | Sample::U8(Layout::Planar));
        let sample = |channel: usize, i: usize| -> f32 {
            let (data, at) = if planar {
                (frame.data(channel), i * size)
            } else {
                (frame.data(0), (i * channels + channel) * size)
            };
            data.get(at..at + size).map_or(0.0, read)
        };
        let right = usize::from(channels > 1);
        Ok((0..n).map(|i| Frame { left: sample(0, i), right: sample(right, i) }).collect())
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        /// The test clip's sound track is Opus; FFmpeg reads it straight from the WebM.
        fn assets() -> Assets {
            Assets::folder(concat!(env!("CARGO_MANIFEST_DIR"), "/.."))
        }

        const CLIP: &str = "player/tests/data/bars-vp9.webm";

        #[test]
        fn opus_sound_decodes_whole() {
            let data = load(&assets(), CLIP).unwrap();
            assert_eq!(data.sample_rate, 48_000, "Opus always decodes at 48 kHz");
            let seconds = data.frames.len() as f64 / 48_000.0;
            assert!((seconds - 1.2).abs() < 0.05, "about as long as the clip: {seconds} s");
            assert!(data.frames.iter().all(|f| f.left.is_finite() && f.left.abs() <= 1.5));
        }

        #[test]
        fn streaming_gives_the_stated_length_and_loops_back_exactly() {
            let mut d = FfmpegAudio::open(&assets().ffmpeg_url(CLIP).unwrap()).unwrap();
            let length = d.num_frames();
            let mut first = Vec::new();
            while first.len() < length {
                let chunk = d.decode().unwrap();
                assert!(!chunk.is_empty(), "never runs dry before the stated length");
                first.extend(chunk);
            }
            assert_eq!(d.seek(0).unwrap(), 0);
            let again = d.decode().unwrap();
            assert_eq!(again[..], first[..again.len()], "a loop starts with the same samples");
            assert_eq!(d.seek(1000).unwrap(), 1000);
            assert_eq!(d.decode().unwrap()[0], first[1000], "seeking lands on the exact frame");
        }

        #[test]
        fn opus_is_recognised_by_content_and_name() {
            assert!(wants_ffmpeg(&assets(), CLIP), "Opus inside WebM");
            assert!(wants_ffmpeg(&assets(), "voice/line1.opus"), "by name, even before it exists");
            assert!(!wants_ffmpeg(&assets(), "examples/assets/calm.ogg"), "Vorbis stays with kira");
        }
    }
}

pub use imp::{load, open};
