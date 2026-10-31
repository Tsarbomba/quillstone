//! Images are decoded on a background thread when the screen or the story
//! just ahead asks for them, and dropped from the GPU again once they haven't
//! been wanted for a while and too many are loaded. So a game with hundreds of
//! pictures starts at once and holds only the ones near where the player is.

use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::mpsc::{channel, Receiver};
use std::sync::{Arc, Condvar, Mutex};

use quillstone_render::pixels::Level;
use quillstone_render::Size;
use quillstone_ui::Assets;

use crate::gfx::{self, Gfx};

/// GPU memory images may take before ones not wanted lately are dropped
/// (about 45 full-HD pictures with their mipmaps).
const BUDGET: u64 = 512 << 20;

/// Decoder threads: a few, so a scene with several pictures doesn't wait for them one by one.
const WORKERS: usize = 3;

type Decoded = (String, Result<(u32, u32, Vec<Level>), String>);

/// Name of the decoder threads (the panic hook in `main` ignores their panics, which are caught).
pub const DECODER: &str = "image decoder";

/// Which images to drop to get under `budget`: the longest unwanted first, never one wanted in `frame`.
fn to_evict(loaded: &HashMap<String, (u64, u64)>, frame: u64, budget: u64) -> Vec<String> {
    let mut total: u64 = loaded.values().map(|(bytes, _)| bytes).sum();
    let mut unwanted: Vec<_> = loaded.iter().filter(|(_, (_, used))| *used < frame).collect();
    unwanted.sort_by_key(|(_, (_, used))| *used);
    let mut drop = Vec::new();
    for (src, (bytes, _)) in unwanted {
        if total <= budget {
            break;
        }
        total -= bytes;
        drop.push(src.clone());
    }
    drop
}

/// Decode a picture; the format is guessed from its content, not its name.
fn decode(file: impl std::io::Read + std::io::Seek) -> Result<(u32, u32, Vec<Level>), String> {
    let img = image::ImageReader::new(std::io::BufReader::new(file))
        .with_guessed_format()
        .map_err(|e| e.to_string())?
        .decode()
        .map_err(|e| e.to_string())?;
    let rgba = img.to_rgba8();
    let (w, h) = rgba.dimensions();
    Ok((w, h, gfx::prepare_image(w, h, rgba.into_raw())))
}

/// What the decoder threads still have to do: what the screen shows first, then the look-ahead.
#[derive(Default)]
struct Queue {
    now: VecDeque<String>,
    ahead: VecDeque<String>,
    closed: bool,
}

impl Queue {
    fn push(&mut self, src: &str, now: bool) {
        if now {
            self.now.push_back(src.to_string());
        } else {
            self.ahead.push_back(src.to_string());
        }
    }

    /// The screen wants `src` after all: it stops waiting behind the look-ahead.
    fn promote(&mut self, src: &str) {
        if let Some(i) = self.ahead.iter().position(|s| s == src) {
            self.ahead.remove(i);
            self.now.push_back(src.to_string());
        }
    }

    /// The story moved on: the look-ahead is asked for again if it is still wanted.
    fn drop_ahead(&mut self) -> Vec<String> {
        self.ahead.drain(..).collect()
    }

    fn pop(&mut self) -> Option<String> {
        self.now.pop_front().or_else(|| self.ahead.pop_front())
    }
}

pub struct Images {
    queue: Arc<(Mutex<Queue>, Condvar)>,
    results: Receiver<Decoded>,
    pending: HashSet<String>,
    failed: HashSet<String>,
    /// Loaded images: their bytes on the GPU and the frame they were last wanted in.
    loaded: HashMap<String, (u64, u64)>,
    frame: u64,
}

impl Images {
    /// Paths are relative to the game's files.
    pub fn new(assets: Assets) -> Images {
        let queue = Arc::new((Mutex::new(Queue::default()), Condvar::new()));
        let (done, results) = channel();
        for _ in 0..WORKERS {
            let (queue, done, assets) = (queue.clone(), done.clone(), assets.clone());
            let worker = move || loop {
                let src = {
                    let (lock, wake) = &*queue;
                    let mut q = lock.lock().unwrap();
                    loop {
                        if let Some(src) = q.pop() {
                            break src;
                        }
                        if q.closed {
                            return;
                        }
                        q = wake.wait(q).unwrap();
                    }
                };
                // A decoder bug must not leave the image pending forever or kill the thread.
                let decoded = std::panic::catch_unwind(|| assets.open(&src).map_err(|e| e.to_string()).and_then(decode))
                    .unwrap_or_else(|_| Err("the decoder crashed on this file".into()));
                if done.send((src, decoded)).is_err() {
                    return; // the player is closing
                }
            };
            std::thread::Builder::new().name(DECODER.into()).spawn(worker).expect("cannot start an image decoder");
        }
        Images { queue, results, pending: HashSet::new(), failed: HashSet::new(), loaded: HashMap::new(), frame: 0 }
    }

    /// The next frame begins: what it wants is marked with [`Images::want`].
    pub fn next_frame(&mut self) {
        self.frame += 1;
        // Jobs a worker already took stay pending (their result arrives); the rest are wanted again below.
        let stale = self.queue.0.lock().unwrap().drop_ahead();
        for src in stale {
            self.pending.remove(&src);
        }
    }

    /// This frame shows (`now`), or the story will soon show, `src`: load it if it isn't.
    pub fn want(&mut self, src: &str, now: bool) {
        let (lock, wake) = &*self.queue;
        if let Some((_, used)) = self.loaded.get_mut(src) {
            *used = self.frame;
        } else if self.pending.contains(src) {
            if now {
                lock.lock().unwrap().promote(src);
            }
        } else if !self.failed.contains(src) {
            self.pending.insert(src.to_string());
            lock.lock().unwrap().push(src, now);
            wake.notify_one();
        }
    }

    /// `src` is being decoded (a frame that shows it would be incomplete).
    pub fn is_pending(&self, src: &str) -> bool {
        self.pending.contains(src)
    }

    /// Try again the images that couldn't be loaded (after a hot reload).
    pub fn retry_failed(&mut self) {
        self.failed.clear();
    }

    /// Images are still being decoded (keep drawing, they'll show up).
    pub fn busy(&self) -> bool {
        !self.pending.is_empty()
    }

    /// Put decoded images on the GPU (their sizes into `sizes`), then drop
    /// the longest unwanted ones while over budget.
    pub fn update(&mut self, gfx: &mut Gfx, sizes: &mut HashMap<String, Size>) {
        for (src, decoded) in self.results.try_iter() {
            self.pending.remove(&src);
            match decoded {
                Ok((w, h, chain)) if gfx.add_image(src.clone(), &chain) => {
                    let bytes = chain.iter().map(|l| l.rgba.len() as u64).sum();
                    sizes.insert(src.clone(), Size::new(w as f32, h as f32));
                    self.loaded.insert(src, (bytes, self.frame));
                }
                Ok(_) => {
                    self.failed.insert(src);
                }
                Err(e) => {
                    eprintln!("warning: cannot load image {src}: {e}");
                    self.failed.insert(src);
                }
            }
        }
        for src in to_evict(&self.loaded, self.frame, BUDGET) {
            self.loaded.remove(&src);
            gfx.remove_image(&src);
            sizes.remove(&src);
        }
    }
}

impl Drop for Images {
    fn drop(&mut self) {
        self.queue.0.lock().unwrap().closed = true;
        self.queue.1.notify_all();
    }
}

#[cfg(test)]
mod tests {
    use super::{decode, to_evict, Queue};
    use std::collections::HashMap;

    #[test]
    fn screen_images_go_before_look_ahead() {
        let mut q = Queue::default();
        q.push("a", false);
        q.push("b", false);
        q.push("c", true);
        q.promote("b");
        let order: Vec<_> = std::iter::from_fn(|| q.pop()).collect();
        assert_eq!(order, ["c", "b", "a"]);
    }

    #[test]
    fn stale_look_ahead_is_dropped_but_screen_jobs_stay() {
        let mut q = Queue::default();
        q.push("a", false);
        q.push("b", true);
        assert_eq!(q.drop_ahead(), ["a"]);
        assert_eq!(q.pop().as_deref(), Some("b"));
        assert_eq!(q.pop(), None);
    }

    #[test]
    fn eviction_spares_wanted_and_drops_oldest_first() {
        let loaded: HashMap<String, (u64, u64)> = [
            ("new", (10, 5)),
            ("old", (10, 1)),
            ("mid", (10, 3)),
            ("now", (10, 9)),
        ]
        .into_iter()
        .map(|(k, v)| (k.to_string(), v))
        .collect();
        assert_eq!(to_evict(&loaded, 9, 25), ["old", "mid"]);
        assert_eq!(to_evict(&loaded, 9, 0), ["old", "mid", "new"]); // "now" is wanted: kept even over budget
        assert!(to_evict(&loaded, 9, 40).is_empty());
    }

    #[test]
    fn a_picture_decodes_from_bytes_whatever_its_name() {
        let mut png = std::io::Cursor::new(Vec::new());
        image::RgbaImage::from_pixel(3, 2, image::Rgba([10, 20, 30, 255])).write_to(&mut png, image::ImageFormat::Png).unwrap();
        let (w, h, levels) = decode(std::io::Cursor::new(png.into_inner())).unwrap();
        assert_eq!((w, h), (3, 2));
        assert!(!levels.is_empty());
        assert!(decode(std::io::Cursor::new(b"not a picture".to_vec())).is_err());
    }
}
