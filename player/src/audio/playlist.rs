//! The order and timing of a chain of music streams (pure, so it is tested without a sound card).

/// The music clock ticks this many times a second (a stream's length is counted in them).
pub const CLOCK_HZ: f64 = 1000.0;

/// The clock tick at which the stream after one that begins at `begin` and lasts `seconds`
/// must start: exactly where that one ends, so the join has no gap.
pub fn next_start<T: std::ops::Add<f64, Output = T>>(begin: T, seconds: f64) -> T {
    begin + seconds * CLOCK_HZ
}

/// The order music plays in: an optional intro once, then the files, round again when looped.
pub struct Playlist {
    pub tracks: Vec<String>,
    /// Where the cycle starts again (after the intro, when there is one).
    restart: usize,
    pos: usize,
    looped: bool,
}

impl Playlist {
    pub fn new(intro: Option<&str>, src: &str, looped: bool) -> Playlist {
        let tracks: Vec<String> = intro.into_iter().chain(quillstone_script::music_tracks(src)).map(str::to_string).collect();
        Playlist { restart: usize::from(intro.is_some()), tracks, pos: 0, looped }
    }

    /// The next file, and whether it is the only one left and plays over and over (so the
    /// stream loops itself, with nothing to queue after it).
    pub fn take_next(&mut self) -> Option<(String, bool)> {
        if self.pos >= self.tracks.len() {
            if !self.looped {
                return None;
            }
            self.pos = self.restart;
        }
        let name = self.tracks[self.pos].clone();
        self.pos += 1;
        let repeats = self.looped && self.pos == self.tracks.len() && self.restart + 1 == self.tracks.len();
        if repeats {
            self.looped = false; // the stream loops itself from here on
        }
        Some((name, repeats))
    }

    /// Nothing more to take.
    pub fn finished(&self) -> bool {
        self.pos >= self.tracks.len() && !self.looped
    }
}

#[cfg(test)]
mod tests {
    use super::*;

        fn order(mut p: Playlist, n: usize) -> Vec<(String, bool)> {
            std::iter::from_fn(|| p.take_next()).take(n).collect()
        }

        fn names(v: &[(String, bool)]) -> Vec<&str> {
            v.iter().map(|(n, _)| n.as_str()).collect()
        }

        #[test]
        fn a_playlist_loops_as_a_whole() {
            let got = order(Playlist::new(None, "a.ogg,b.ogg,c.ogg", true), 7);
            assert_eq!(names(&got), ["a.ogg", "b.ogg", "c.ogg", "a.ogg", "b.ogg", "c.ogg", "a.ogg"]);
            assert!(got.iter().all(|(_, repeats)| !repeats));
        }

        #[test]
        fn loop_false_plays_the_list_once() {
            let mut p = Playlist::new(None, "a.ogg,b.ogg", false);
            assert!(!p.finished());
            assert_eq!(names(&order(Playlist::new(None, "a.ogg,b.ogg", false), 9)), ["a.ogg", "b.ogg"]);
            p.take_next();
            p.take_next();
            assert!(p.finished() && p.take_next().is_none());
        }

        #[test]
        fn an_intro_plays_once_and_the_list_loops() {
            let got = order(Playlist::new(Some("i.ogg"), "a.ogg,b.ogg", true), 6);
            assert_eq!(names(&got), ["i.ogg", "a.ogg", "b.ogg", "a.ogg", "b.ogg", "a.ogg"]);
        }

        #[test]
        fn one_file_loops_itself_and_leaves_nothing_to_queue() {
            let mut p = Playlist::new(None, "a.ogg", true);
            assert_eq!(p.take_next(), Some(("a.ogg".to_string(), true)));
            assert!(p.finished() && p.take_next().is_none());
            let mut p = Playlist::new(Some("i.ogg"), "a.ogg", true);
            assert_eq!(p.take_next(), Some(("i.ogg".to_string(), false)));
            assert!(!p.finished());
            assert_eq!(p.take_next(), Some(("a.ogg".to_string(), true)));
            assert!(p.finished());
            let mut once = Playlist::new(None, "a.ogg", false);
            assert_eq!(once.take_next(), Some(("a.ogg".to_string(), false)));
            assert!(once.finished());
        }

        #[test]
        fn each_stream_starts_where_the_last_ends() {
            let mut at = 60.0;
            let mut starts = Vec::new();
            for seconds in [2.5, 0.75, 10.0] {
                starts.push(at);
                at = next_start(at, seconds);
            }
            assert_eq!(starts, [60.0, 2560.0, 3310.0]);
            assert_eq!(at, 13310.0);
        }
}
