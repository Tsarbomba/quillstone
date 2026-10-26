//! Audio backend: plays the mixer's [`Action`]s with kira.
//!
//! Music, sound effects and voice each play on their own kira sub-track, so each
//! channel can get its own volume. Music and voice are streamed from disk (no
//! decode hitch on the main thread); sound effects are decoded up front so they
//! start instantly. Opus files are decoded by FFmpeg (see `ffaudio`), the rest
//! by kira. Without the `audio` feature, or without a sound device, the game
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

    use kira::sound::static_sound::StaticSoundData;
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
                    let data = match self.sfx.get(&src) {
                        Some(data) => data.clone(), // cheap: the samples are shared
                        None => match load(&self.root.join(&src)) {
                            Ok(data) => {
                                self.sfx.insert(src.clone(), data.clone());
                                data
                            }
                            Err(e) => return self.warn(&src, e),
                        },
                    };
                    if let Err(e) = self.sfx_track.play(data.volume(amplitude_to_db(volume))) {
                        self.warn(&src, e);
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
