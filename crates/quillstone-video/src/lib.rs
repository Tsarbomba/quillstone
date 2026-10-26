//! Quillstone video pipeline, minus the codec.
//!
//! - [`Decoder`]: what a codec backend implements (frames in display order).
//! - [`Clip`]: one playing video. A decoder thread fills a bounded
//!   [`FrameQueue`]; the render loop advances the clip's clock and shows the
//!   frame that is due, dropping late ones. Looping keeps timestamps rising.
//! - [`Videos`]: starts and stops clips to match what the story wants on screen.
//! - [`TestPattern`]: a synthetic decoder for tests and for trying the pipeline.
//!
//! The clock is advanced by the caller (frame time for now; the audio clock
//! once video audio plays), so everything here is testable without a codec.

pub mod probe;

use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

/// A decoded frame (`F` is whatever the renderer wants, e.g. a wgpu texture handle)
/// with its presentation timestamp relative to the start of the clip.
#[derive(Debug, Clone, PartialEq)]
pub struct TimedFrame<F> {
    pub pts: Duration,
    pub frame: F,
}

/// Bounded queue between a decoder thread and the render loop.
///
/// The decoder calls [`push`](Self::push) and backs off when it returns the frame
/// (queue full). The renderer calls [`take_due`](Self::take_due) once per tick.
#[derive(Debug)]
pub struct FrameQueue<F> {
    capacity: usize,
    frames: VecDeque<TimedFrame<F>>,
    dropped: u64,
}

impl<F> FrameQueue<F> {
    pub fn new(capacity: usize) -> Self {
        assert!(capacity > 0, "capacity must be at least 1");
        FrameQueue { capacity, frames: VecDeque::with_capacity(capacity), dropped: 0 }
    }

    pub fn len(&self) -> usize {
        self.frames.len()
    }

    pub fn is_empty(&self) -> bool {
        self.frames.is_empty()
    }

    pub fn is_full(&self) -> bool {
        self.frames.len() >= self.capacity
    }

    /// Frames discarded because the renderer fell behind the clock.
    pub fn dropped(&self) -> u64 {
        self.dropped
    }

    /// Queue a frame. Gives it back if the queue is full so the decoder can wait.
    pub fn push(&mut self, frame: TimedFrame<F>) -> Result<(), TimedFrame<F>> {
        if self.is_full() {
            Err(frame)
        } else {
            self.frames.push_back(frame);
            Ok(())
        }
    }

    /// The frame to show at `clock`: the newest one whose timestamp has been
    /// reached. Older frames that were never shown are dropped. `None` means the
    /// next frame isn't due yet (keep showing the previous one).
    pub fn take_due(&mut self, clock: Duration) -> Option<TimedFrame<F>> {
        let mut due = None;
        while self.frames.front().is_some_and(|f| f.pts <= clock) {
            let next = self.frames.pop_front();
            if due.replace(next?).is_some() {
                self.dropped += 1;
            }
        }
        due
    }

    /// Discard everything (seek, skip, or clip change).
    pub fn clear(&mut self) {
        self.frames.clear();
    }
}

// ---- decoding ---------------------------------------------------------------------

/// A decoded picture: RGBA8, sRGB, row-major (`width * height * 4` bytes).
#[derive(Debug, Clone, PartialEq)]
pub struct Picture {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

/// A codec backend: produces a clip's frames in display order.
pub trait Decoder: Send {
    /// The next frame, with its timestamp from the start of the clip; `None` at the end.
    fn next_frame(&mut self) -> Result<Option<TimedFrame<Picture>>, String>;
    /// Start again from the first frame (for looping).
    fn rewind(&mut self) -> Result<(), String>;
}

/// Opens a decoder for a video file (path as written in the script).
pub type OpenFn = Box<dyn Fn(&str) -> Result<Box<dyn Decoder>, String>>;

/// Frames buffered ahead of the clock.
const QUEUE_FRAMES: usize = 6;

/// Frame length assumed until two frames tell the real one.
const DEFAULT_FRAME: Duration = Duration::from_millis(33);

struct Shared {
    queue: Mutex<FrameQueue<Picture>>,
    /// Signalled when the renderer takes frames (room for the decoder).
    space: Condvar,
    stop: AtomicBool,
    /// The decoder has no more frames (end of a non-looping clip, or an error).
    ended: AtomicBool,
    error: Mutex<Option<String>>,
}

/// One playing video.
pub struct Clip {
    shared: Arc<Shared>,
    thread: Option<JoinHandle<()>>,
    clock: Duration,
    current: Option<Picture>,
    /// Counts frames shown, so the renderer knows when to upload a new one.
    frame_id: u64,
}

impl Clip {
    /// Start decoding on a thread of its own. The clock starts at zero.
    pub fn start(decoder: Box<dyn Decoder>, looped: bool) -> Clip {
        let shared = Arc::new(Shared {
            queue: Mutex::new(FrameQueue::new(QUEUE_FRAMES)),
            space: Condvar::new(),
            stop: AtomicBool::new(false),
            ended: AtomicBool::new(false),
            error: Mutex::new(None),
        });
        let worker = shared.clone();
        let thread = std::thread::Builder::new()
            .name("quillstone-video".into())
            .spawn(move || decode_loop(decoder, looped, &worker))
            .ok();
        if thread.is_none() {
            *shared.error.lock().unwrap() = Some("could not start a decoder thread".into());
            shared.ended.store(true, Ordering::SeqCst);
        }
        Clip { shared, thread, clock: Duration::ZERO, current: None, frame_id: 0 }
    }

    /// Move the clock on by `dt` and pick up the frame that is now due.
    /// Returns `true` if a new frame is to be shown.
    pub fn advance(&mut self, dt: Duration) -> bool {
        self.clock += dt;
        let due = self.shared.queue.lock().unwrap().take_due(self.clock);
        self.shared.space.notify_one();
        match due {
            Some(f) => {
                self.current = Some(f.frame);
                self.frame_id += 1;
                true
            }
            None => false,
        }
    }

    pub fn clock(&self) -> Duration {
        self.clock
    }

    /// The frame on screen now (`None` until the first one is due).
    pub fn frame(&self) -> Option<&Picture> {
        self.current.as_ref()
    }

    /// Changes whenever [`Clip::frame`] does.
    pub fn frame_id(&self) -> u64 {
        self.frame_id
    }

    /// Frames that were never shown because playback fell behind.
    pub fn dropped(&self) -> u64 {
        self.shared.queue.lock().unwrap().dropped()
    }

    /// All frames have been shown (never true for a looping clip that works).
    pub fn finished(&self) -> bool {
        self.shared.ended.load(Ordering::SeqCst) && self.shared.queue.lock().unwrap().is_empty()
    }

    pub fn error(&self) -> Option<String> {
        self.shared.error.lock().unwrap().clone()
    }

    /// Block until `n` frames are buffered or the clip has ended (for tests and
    /// for starting a clip without a blank first frame). `false` on timeout.
    pub fn wait_buffered(&self, n: usize, timeout: Duration) -> bool {
        let deadline = std::time::Instant::now() + timeout;
        loop {
            if self.shared.queue.lock().unwrap().len() >= n || self.shared.ended.load(Ordering::SeqCst) {
                return true;
            }
            if std::time::Instant::now() >= deadline {
                return false;
            }
            std::thread::sleep(Duration::from_millis(1));
        }
    }
}

impl Drop for Clip {
    fn drop(&mut self) {
        self.shared.stop.store(true, Ordering::SeqCst);
        self.shared.space.notify_all();
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

/// The decoder thread: decode ahead until the queue is full, wait for room.
fn decode_loop(mut decoder: Box<dyn Decoder>, looped: bool, shared: &Shared) {
    let stopped = || shared.stop.load(Ordering::SeqCst);
    // Timestamps of later loops continue after the earlier ones, so the queue
    // (and the clock) only ever see rising times.
    let mut offset = Duration::ZERO;
    let mut first: Option<Duration> = None; // the clip's own first timestamp
    let mut last = Duration::ZERO; // last timestamp in this loop, from `first`
    let mut frame_len = DEFAULT_FRAME;
    let mut frames_this_loop = 0u64;
    let fail = |e: String| {
        *shared.error.lock().unwrap() = Some(e);
        shared.ended.store(true, Ordering::SeqCst);
    };
    while !stopped() {
        let frame = match decoder.next_frame() {
            Ok(Some(f)) => f,
            Ok(None) if looped && frames_this_loop > 0 => {
                if let Err(e) = decoder.rewind() {
                    return fail(e);
                }
                offset += last + frame_len;
                (first, last, frames_this_loop) = (None, Duration::ZERO, 0);
                continue;
            }
            Ok(None) => {
                shared.ended.store(true, Ordering::SeqCst);
                return;
            }
            Err(e) => return fail(e),
        };
        let base = *first.get_or_insert(frame.pts);
        let pts = frame.pts.saturating_sub(base);
        if frames_this_loop > 0 && pts > last {
            frame_len = pts - last;
        }
        last = pts;
        frames_this_loop += 1;

        let mut item = TimedFrame { pts: offset + pts, frame: frame.frame };
        let mut queue = shared.queue.lock().unwrap();
        loop {
            match queue.push(item) {
                Ok(()) => break,
                Err(back) => {
                    item = back;
                    if stopped() {
                        return;
                    }
                    queue = shared.space.wait_timeout(queue, Duration::from_millis(50)).unwrap().0;
                }
            }
        }
    }
}

// ---- which clips are playing ------------------------------------------------------------

/// A video the story wants playing.
#[derive(Debug, Clone, PartialEq)]
pub struct Wanted {
    pub src: String,
    pub looped: bool,
    /// Changes when the same file should start over (e.g. rollback to a cutscene).
    pub generation: u64,
}

/// The clips playing right now, kept in line with what the story wants.
pub struct Videos {
    open: OpenFn,
    clips: HashMap<String, (u64, Clip)>,
    /// Files that couldn't be opened, and why (reported once, not retried).
    failed: HashMap<String, String>,
    new_errors: Vec<(String, String)>,
}

impl Videos {
    pub fn new(open: OpenFn) -> Videos {
        Videos { open, clips: HashMap::new(), failed: HashMap::new(), new_errors: Vec::new() }
    }

    /// Start what is wanted and not playing; stop what is no longer wanted.
    pub fn sync(&mut self, wanted: &[Wanted]) {
        self.clips.retain(|src, (generation, _)| wanted.iter().any(|w| w.src == *src && w.generation == *generation));
        for w in wanted {
            if self.clips.contains_key(&w.src) || self.failed.contains_key(&w.src) {
                continue;
            }
            match (self.open)(&w.src) {
                Ok(decoder) => {
                    let clip = Clip::start(decoder, w.looped);
                    clip.wait_buffered(1, Duration::from_millis(200)); // avoid a blank first frame
                    self.clips.insert(w.src.clone(), (w.generation, clip));
                }
                Err(e) => {
                    self.failed.insert(w.src.clone(), e.clone());
                    self.new_errors.push((w.src.clone(), e));
                }
            }
        }
    }

    /// Advance every clip's clock.
    pub fn advance(&mut self, dt: Duration) {
        for (_, clip) in self.clips.values_mut() {
            clip.advance(dt);
        }
        let broken: Vec<(String, String)> = self
            .clips
            .iter()
            .filter_map(|(src, (_, c))| c.error().map(|e| (src.clone(), e)))
            .filter(|(src, _)| !self.failed.contains_key(src))
            .collect();
        for (src, e) in broken {
            self.failed.insert(src.clone(), e.clone());
            self.new_errors.push((src, e));
        }
    }

    /// The frame to show for `src` and its id (changes with every new frame).
    pub fn frame(&self, src: &str) -> Option<(&Picture, u64)> {
        let (_, clip) = self.clips.get(src)?;
        clip.frame().map(|f| (f, clip.frame_id()))
    }

    /// Whether `src` is actually playing (opened, no error).
    pub fn playing(&self, src: &str) -> bool {
        self.clips.get(src).is_some_and(|(_, c)| c.error().is_none())
    }

    pub fn finished(&self, src: &str) -> bool {
        self.clips.get(src).is_some_and(|(_, c)| c.finished())
    }

    /// The sources of all clips playing now.
    pub fn sources(&self) -> Vec<String> {
        self.clips.keys().cloned().collect()
    }

    /// Problems since the last call: `(file, message)`.
    pub fn take_errors(&mut self) -> Vec<(String, String)> {
        std::mem::take(&mut self.new_errors)
    }
}

// ---- a synthetic decoder -------------------------------------------------------------------

/// Moving colour bars, `frames` long at `fps`. Each frame's top-left pixel
/// encodes its number (red = n % 256, green = n / 256 % 256), which tests read back.
pub struct TestPattern {
    pub width: u32,
    pub height: u32,
    pub fps: u32,
    pub frames: u64,
    next: u64,
}

impl TestPattern {
    pub fn new(width: u32, height: u32, fps: u32, frames: u64) -> TestPattern {
        TestPattern { width, height, fps: fps.max(1), frames, next: 0 }
    }

    /// The frame number a picture from this decoder shows.
    pub fn number(picture: &Picture) -> u64 {
        picture.rgba[0] as u64 + picture.rgba[1] as u64 * 256
    }
}

impl Decoder for TestPattern {
    fn next_frame(&mut self) -> Result<Option<TimedFrame<Picture>>, String> {
        if self.next >= self.frames {
            return Ok(None);
        }
        let n = self.next;
        self.next += 1;
        let (w, h) = (self.width as usize, self.height as usize);
        let mut rgba = vec![0u8; w * h * 4];
        const BARS: [[u8; 3]; 7] =
            [[255, 255, 255], [255, 255, 0], [0, 255, 255], [0, 255, 0], [255, 0, 255], [255, 0, 0], [0, 0, 255]];
        for y in 0..h {
            for x in 0..w {
                let bar = ((x * 7 / w.max(1)) + n as usize) % 7;
                let i = (y * w + x) * 4;
                rgba[i..i + 3].copy_from_slice(&BARS[bar]);
                rgba[i + 3] = 255;
            }
        }
        rgba[0] = (n % 256) as u8;
        rgba[1] = (n / 256 % 256) as u8;
        let pts = Duration::from_secs_f64(n as f64 / self.fps as f64);
        Ok(Some(TimedFrame { pts, frame: Picture { width: self.width, height: self.height, rgba } }))
    }

    fn rewind(&mut self) -> Result<(), String> {
        self.next = 0;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ms(n: u64) -> Duration {
        Duration::from_millis(n)
    }

    fn frame(pts_ms: u64, id: u32) -> TimedFrame<u32> {
        TimedFrame { pts: ms(pts_ms), frame: id }
    }

    // ---- clips (these run a real decoder thread) ---------------------------------

    const SEC: Duration = Duration::from_secs(1);

    fn pattern(frames: u64) -> Box<dyn Decoder> {
        Box::new(TestPattern::new(8, 4, 10, frames)) // 10 fps: a frame every 100 ms
    }

    fn shown(clip: &Clip) -> Option<u64> {
        clip.frame().map(TestPattern::number)
    }

    #[test]
    fn a_clip_shows_its_frames_on_time() {
        let mut clip = Clip::start(pattern(5), false);
        assert!(clip.wait_buffered(5, SEC));
        assert!(clip.advance(Duration::ZERO));
        assert_eq!(shown(&clip), Some(0));
        assert!(!clip.advance(ms(50)), "frame 1 isn't due until 100 ms");
        assert!(clip.advance(ms(60)));
        assert_eq!(shown(&clip), Some(1));
        assert_eq!(clip.frame_id(), 2);
        assert!(!clip.finished());
    }

    #[test]
    fn a_late_clip_skips_ahead_and_then_finishes() {
        let mut clip = Clip::start(pattern(5), false);
        assert!(clip.wait_buffered(5, SEC));
        clip.advance(ms(350));
        assert_eq!(shown(&clip), Some(3));
        assert_eq!(clip.dropped(), 3);
        clip.advance(ms(100));
        assert_eq!(shown(&clip), Some(4));
        assert!(clip.wait_buffered(usize::MAX, SEC)); // until the decoder says it ended
        assert!(clip.finished());
    }

    #[test]
    fn a_looping_clip_starts_over_with_rising_timestamps() {
        let mut clip = Clip::start(pattern(3), true); // 0.3 s per loop
        let mut seen = Vec::new();
        for _ in 0..8 {
            clip.wait_buffered(1, SEC);
            if clip.advance(ms(100)) {
                seen.push(shown(&clip).unwrap());
            }
        }
        assert_eq!(seen, vec![1, 2, 0, 1, 2, 0, 1, 2]);
        assert!(!clip.finished());
    }

    #[test]
    fn a_long_clip_only_buffers_a_few_frames_ahead() {
        let clip = Clip::start(pattern(10_000), false);
        assert!(clip.wait_buffered(QUEUE_FRAMES, SEC));
        std::thread::sleep(ms(20));
        assert_eq!(clip.shared.queue.lock().unwrap().len(), QUEUE_FRAMES);
        drop(clip); // stopping a blocked decoder thread doesn't hang
    }

    struct Broken;
    impl Decoder for Broken {
        fn next_frame(&mut self) -> Result<Option<TimedFrame<Picture>>, String> {
            Err("bad data".into())
        }
        fn rewind(&mut self) -> Result<(), String> {
            Ok(())
        }
    }

    #[test]
    fn decoder_errors_end_the_clip_and_are_reported() {
        let mut clip = Clip::start(Box::new(Broken), true);
        clip.wait_buffered(1, SEC);
        clip.advance(SEC);
        assert!(clip.finished());
        assert_eq!(clip.error().as_deref(), Some("bad data"));
    }

    // ---- the set of clips ----------------------------------------------------------

    fn videos() -> Videos {
        Videos::new(Box::new(|src: &str| match src {
            "missing.webm" => Err("no such file".into()),
            "broken.webm" => Ok(Box::new(Broken) as Box<dyn Decoder>),
            "long.webm" => Ok(pattern(1000)),
            _ => Ok(pattern(3)),
        }))
    }

    fn want(src: &str, generation: u64) -> Wanted {
        Wanted { src: src.into(), looped: false, generation }
    }

    #[test]
    fn videos_start_and_stop_with_what_is_wanted() {
        let mut v = videos();
        v.sync(&[want("a.webm", 0), want("bg.webm", 0)]);
        v.advance(Duration::ZERO);
        assert!(v.frame("a.webm").is_some() && v.playing("bg.webm"));
        v.sync(&[want("bg.webm", 0)]);
        assert!(v.frame("a.webm").is_none());
        assert_eq!(v.sources(), vec!["bg.webm".to_string()]);
    }

    #[test]
    fn a_new_generation_restarts_the_clip() {
        let mut v = videos();
        v.sync(&[want("long.webm", 1)]);
        v.advance(ms(250));
        assert_eq!(v.frame("long.webm").map(|(p, _)| TestPattern::number(p)), Some(2));
        v.sync(&[want("long.webm", 2)]); // e.g. rolled back to the same cutscene
        v.advance(Duration::ZERO);
        assert_eq!(v.frame("long.webm").map(|(p, _)| TestPattern::number(p)), Some(0));
    }

    #[test]
    fn files_that_fail_are_reported_once_and_not_retried() {
        let mut v = videos();
        v.sync(&[want("missing.webm", 0)]);
        v.sync(&[want("missing.webm", 0)]);
        assert_eq!(v.take_errors(), vec![("missing.webm".to_string(), "no such file".to_string())]);
        assert!(!v.playing("missing.webm"));
        v.sync(&[want("broken.webm", 0)]);
        v.advance(SEC);
        v.advance(SEC);
        assert_eq!(v.take_errors().len(), 1);
        assert!(!v.playing("broken.webm"));
    }

    #[test]
    fn nothing_is_due_before_the_first_timestamp() {
        let mut q = FrameQueue::new(4);
        q.push(frame(40, 1)).unwrap();
        assert!(q.take_due(ms(10)).is_none());
        assert_eq!(q.len(), 1);
    }

    #[test]
    fn on_time_playback_shows_every_frame() {
        let mut q = FrameQueue::new(4);
        q.push(frame(0, 1)).unwrap();
        q.push(frame(40, 2)).unwrap();
        assert_eq!(q.take_due(ms(0)).unwrap().frame, 1);
        assert!(q.take_due(ms(20)).is_none());
        assert_eq!(q.take_due(ms(41)).unwrap().frame, 2);
        assert_eq!(q.dropped(), 0);
    }

    #[test]
    fn late_renderer_skips_to_the_newest_due_frame() {
        let mut q = FrameQueue::new(8);
        for (i, t) in [0, 40, 80, 120, 160].into_iter().enumerate() {
            q.push(frame(t, i as u32)).unwrap();
        }
        let shown = q.take_due(ms(130)).unwrap();
        assert_eq!(shown.frame, 3); // the 120ms frame
        assert_eq!(q.dropped(), 3); // 0, 40, 80 were never shown
        assert_eq!(q.len(), 1); // the 160ms frame is still waiting
    }

    #[test]
    fn full_queue_hands_the_frame_back() {
        let mut q = FrameQueue::new(2);
        q.push(frame(0, 1)).unwrap();
        q.push(frame(40, 2)).unwrap();
        let back = q.push(frame(80, 3)).unwrap_err();
        assert_eq!(back.frame, 3);
    }

    #[test]
    fn clear_empties_the_queue() {
        let mut q = FrameQueue::new(2);
        q.push(frame(0, 1)).unwrap();
        q.clear();
        assert!(q.is_empty());
    }
}
