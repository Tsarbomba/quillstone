//! Audio backend: plays the mixer's [`Action`]s with kira.
//!
//! Music, sound effects and voice each play on their own kira sub-track, so each
//! channel can get its own volume. Music and voice are streamed from disk (no
//! decode hitch on the main thread); one-shot sound effects are decoded up front so
//! they start instantly. Looping sound effects are streamed like music (a long
//! ambience is never decoded whole) with a loop region, on the sound track; the
//! handle of each is kept per file so the mixer can stop or retune it. A loop that
//! cannot start, or ends without being stopped, is reported (see
//! `Audio::take_failed_loops`) so the mixer forgets it. Opus files are decoded by
//! FFmpeg (see `ffaudio`), the rest by kira. Without the `audio` feature, or without a sound device, the game
//! simply runs silent.

use std::path::Path;

use quillstone_audio::Action;

#[cfg(feature = "audio")]
pub use real::Audio;
#[cfg(not(feature = "audio"))]
pub use stub::Audio;

#[cfg(not(feature = "audio"))]
mod stub {
    use super::*;

    pub struct Audio;

    impl Audio {
        pub fn new<'a>(_root: &Path, _sfx: impl Iterator<Item = &'a str>) -> Option<Audio> {
            None
        }

        pub fn run(&mut self, _action: Action) {}

        pub fn update(&mut self) -> bool {
            false
        }

        pub fn take_failed_loops(&mut self) -> Vec<String> {
            Vec::new()
        }

        pub fn voice_playing(&self) -> bool {
            false
        }
    }
}

#[cfg(feature = "audio")]
mod real {
    use super::*;
    use std::collections::{HashMap, HashSet};
    use std::fmt::Display;
    use std::path::PathBuf;
    use std::time::Duration;

    use kira::sound::static_sound::{StaticSoundData, StaticSoundHandle};
    use kira::sound::streaming::{StreamingSoundData, StreamingSoundHandle};
    use kira::sound::{FromFileError, PlaybackState};
    use kira::track::{TrackBuilder, TrackHandle};
    use kira::{AudioManager, AudioManagerSettings, DefaultBackend, Tween};
    use quillstone_audio::{amplitude_to_db, channel_gain_db, Channel};

    /// A short fade instead of a hard cut, which would click.
    const DECLICK: f32 = 0.05;

    pub struct Audio {
        root: PathBuf,
        // Kept alive: dropping the manager stops all sound.
        _manager: AudioManager<DefaultBackend>,
        music_track: TrackHandle,
        sfx_track: TrackHandle,
        voice_track: TrackHandle,
        music: Option<StreamingSoundHandle<FromFileError>>,
        voice: Option<StreamingSoundHandle<FromFileError>>,
        sfx: HashMap<String, StaticSoundData>,
        /// The playing (streamed) loop of each looping sound effect, by file.
        sfx_loops: HashMap<String, StreamingSoundHandle<FromFileError>>,
        /// Loops that failed to start or died, not yet told to the mixer.
        failed_loops: Vec<String>,
        /// One-shot sound effects that may still be playing, so `stop=` can cut them.
        sfx_shots: Vec<(String, StaticSoundHandle)>,
        /// Files we already complained about, so a missing file warns once.
        warned: HashSet<String>,
    }

    impl Audio {
        /// Open the default sound device and preload the given sound effects
        /// (paths relative to `root`). `None` if there is no usable device.
        pub fn new<'a>(root: &Path, sfx: impl Iterator<Item = &'a str>) -> Option<Audio> {
            let mut manager = match AudioManager::<DefaultBackend>::new(AudioManagerSettings::default()) {
                Ok(m) => m,
                Err(e) => {
                    eprintln!("warning: no audio ({e}); playing silent");
                    return None;
                }
            };
            let mut track = || manager.add_sub_track(TrackBuilder::new()).ok();
            let (music_track, sfx_track, voice_track) = (track()?, track()?, track()?);

            let mut audio = Audio {
                root: root.to_path_buf(),
                _manager: manager,
                music_track,
                sfx_track,
                voice_track,
                music: None,
                voice: None,
                sfx: HashMap::new(),
                sfx_loops: HashMap::new(),
                failed_loops: Vec::new(),
                sfx_shots: Vec::new(),
                warned: HashSet::new(),
            };
            for src in sfx {
                if !audio.sfx.contains_key(src) {
                    match load(&audio.root.join(src)) {
                        Ok(data) => {
                            audio.sfx.insert(src.to_string(), data);
                        }
                        Err(e) => audio.warn(src, e),
                    }
                }
            }
            Some(audio)
        }

        /// Call every frame. `true` once when a one-shot music track has ended.
        pub fn update(&mut self) -> bool {
            self.forget_finished_sfx();
            if self.voice.as_ref().is_some_and(|v| v.state() == PlaybackState::Stopped) {
                self.voice = None;
            }
            let ended = self.music.as_ref().is_some_and(|m| m.state() == PlaybackState::Stopped);
            if ended {
                self.music = None;
            }
            ended
        }

        /// Whether a voice clip is still playing (auto-forward waits for it).
        pub fn voice_playing(&self) -> bool {
            self.voice.is_some()
        }

        pub fn run(&mut self, action: Action) {
            match action {
                Action::PlayMusic { src, fade, looped, volume } => {
                    let Some(data) = self.stream(&src) else { return };
                    let mut data = data.volume(amplitude_to_db(volume));
                    if looped {
                        data = data.loop_region(..);
                    }
                    if fade > 0.0 {
                        data = data.fade_in_tween(tween(fade));
                    }
                    match self.music_track.play(data) {
                        Ok(handle) => self.music = Some(handle),
                        Err(e) => self.warn(&src, e),
                    }
                }
                Action::StopMusic { fade } => {
                    if let Some(mut music) = self.music.take() {
                        music.stop(tween(fade.max(DECLICK)));
                    }
                }
                Action::SetMusicVolume { volume, fade } => {
                    if let Some(music) = &mut self.music {
                        music.set_volume(amplitude_to_db(volume), tween(fade));
                    }
                }
                Action::PlaySfx { src, volume } => {
                    let Some(data) = self.sfx_data(&src) else { return };
                    match self.sfx_track.play(data.volume(amplitude_to_db(volume))) {
                        Ok(handle) => {
                            self.forget_finished_sfx();
                            self.sfx_shots.push((src, handle));
                        }
                        Err(e) => self.warn(&src, e),
                    }
                }
                Action::PlaySfxLoop { src, fade, volume } => self.start_loop(src, fade, volume),
                Action::SetSfxLoopVolume { src, volume, fade } => match self.sfx_loops.get_mut(&src) {
                    Some(handle) => handle.set_volume(amplitude_to_db(volume), tween(fade)),
                    // The mixer thinks it plays but we have nothing: start it (or report why not).
                    None => self.start_loop(src, fade, volume),
                },
                Action::StopSfx { src, fade } => {
                    let out = tween(fade.max(DECLICK));
                    let wanted = |name: &String| src.as_ref().is_none_or(|s| s == name);
                    let loops: Vec<String> = self.sfx_loops.keys().filter(|k| wanted(k)).cloned().collect();
                    for name in loops {
                        if let Some(mut handle) = self.sfx_loops.remove(&name) {
                            handle.stop(out);
                        }
                    }
                    for (_, handle) in self.sfx_shots.iter_mut().filter(|(name, _)| wanted(name)) {
                        handle.stop(out);
                    }
                    self.sfx_shots.retain(|(name, _)| !wanted(name));
                }
                Action::StopSfxLoop { src, fade } => {
                    if let Some(mut handle) = self.sfx_loops.remove(&src) {
                        handle.stop(tween(fade.max(DECLICK)));
                    }
                }
                Action::PlayVoice { src } => {
                    let Some(data) = self.stream(&src) else { return };
                    match self.voice_track.play(data) {
                        Ok(handle) => self.voice = Some(handle),
                        Err(e) => self.warn(&src, e),
                    }
                }
                Action::StopVoice => {
                    if let Some(mut voice) = self.voice.take() {
                        voice.stop(tween(DECLICK));
                    }
                }
                Action::SetChannelVolume { channel, volume } => {
                    let track = match channel {
                        Channel::Music => &mut self.music_track,
                        Channel::Sfx => &mut self.sfx_track,
                        Channel::Voice => &mut self.voice_track,
                    };
                    track.set_volume(channel_gain_db(volume), tween(DECLICK));
                }
            }
        }

        /// Start streaming the loop of `src` on the sound track. On any failure the
        /// loop is reported so the mixer forgets it.
        fn start_loop(&mut self, src: String, fade: f32, volume: f32) {
            if let Some(mut old) = self.sfx_loops.remove(&src) {
                old.stop(tween(DECLICK)); // the mixer never asks twice; this is only a stale handle
            }
            let Some(data) = self.stream(&src) else { return self.failed_loops.push(src) };
            let mut data = data.volume(amplitude_to_db(volume)).loop_region(..);
            if fade > 0.0 {
                data = data.fade_in_tween(tween(fade));
            }
            match self.sfx_track.play(data) {
                Ok(handle) => {
                    self.sfx_loops.insert(src, handle);
                }
                Err(e) => {
                    self.warn(&src, e);
                    self.failed_loops.push(src);
                }
            }
        }

        /// The loops that could not start or died since the last call.
        pub fn take_failed_loops(&mut self) -> Vec<String> {
            std::mem::take(&mut self.failed_loops)
        }

        /// A sound effect's decoded samples, loaded on first use (or at startup).
        fn sfx_data(&mut self, src: &str) -> Option<StaticSoundData> {
            if let Some(data) = self.sfx.get(src) {
                return Some(data.clone()); // cheap: the samples are shared
            }
            match load(&self.root.join(src)) {
                Ok(data) => {
                    self.sfx.insert(src.to_string(), data.clone());
                    Some(data)
                }
                Err(e) => {
                    self.warn(src, e);
                    None
                }
            }
        }

        /// Drop the handles of sounds that have played to their end.
        fn forget_finished_sfx(&mut self) {
            self.sfx_shots.retain(|(_, h)| h.state() != PlaybackState::Stopped);
            // A loop is only ever stopped by us (and then forgotten): one that stopped
            // by itself (a decoder error, a lost device) is reported.
            let dead: Vec<String> =
                self.sfx_loops.iter().filter(|(_, h)| h.state() == PlaybackState::Stopped).map(|(k, _)| k.clone()).collect();
            for src in dead {
                self.sfx_loops.remove(&src);
                self.failed_loops.push(src);
            }
        }

        fn stream(&mut self, src: &str) -> Option<StreamingSoundData<FromFileError>> {
            let path = self.root.join(src);
            let data =
                if crate::ffaudio::wants_ffmpeg(&path) { crate::ffaudio::open(&path) } else { StreamingSoundData::from_file(path) };
            match data {
                Ok(data) => Some(data),
                Err(e) => {
                    self.warn(src, e);
                    None
                }
            }
        }

        fn warn(&mut self, src: &str, error: impl Display) {
            if self.warned.insert(src.to_string()) {
                eprintln!("warning: cannot play {src}: {error}");
            }
        }
    }

    /// A whole sound file, decoded (for sound effects).
    fn load(path: &Path) -> Result<StaticSoundData, FromFileError> {
        if crate::ffaudio::wants_ffmpeg(path) {
            crate::ffaudio::load(path)
        } else {
            StaticSoundData::from_file(path)
        }
    }

    fn tween(seconds: f32) -> Tween {
        if seconds <= 0.0 {
            return Tween::default(); // kira's default is a 10 ms ramp
        }
        Tween { duration: Duration::from_secs_f32(seconds), ..Default::default() }
    }
}
