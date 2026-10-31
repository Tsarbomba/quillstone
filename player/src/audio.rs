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
//! FFmpeg (see `ffaudio`), the rest by kira. A music track with an intro is two streams:
//! the intro starts at a time on a kira clock and the loop at that time plus the
//! intro's length, so the join is sample-accurate to within one kira buffer (about 3 ms). Without the `audio` feature, or without a sound device, the game
//! simply runs silent.

use quillstone_audio::Action;
use quillstone_ui::Assets;

#[cfg(feature = "audio")]
pub use real::Audio;
#[cfg(not(feature = "audio"))]
pub use stub::Audio;

#[cfg(not(feature = "audio"))]
mod stub {
    use super::*;

    pub struct Audio;

    impl Audio {
        pub fn new<'a>(_assets: &Assets, _sfx: impl Iterator<Item = &'a str>) -> Option<Audio> {
            None
        }

        pub fn run(&mut self, _action: Action) {}

        pub fn update(&mut self) -> bool {
            false
        }

        pub fn set_paused(&mut self, _paused: bool) {}

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
    use std::io::{Read, Seek, SeekFrom};
    use quillstone_ui::assets::AssetFile;
    use std::time::Duration;

    use kira::sound::static_sound::{StaticSoundData, StaticSoundHandle};
    use kira::sound::streaming::{StreamingSoundData, StreamingSoundHandle};
    use kira::sound::{FromFileError, PlaybackState};
    use kira::clock::{ClockHandle, ClockSpeed};
    use kira::track::{TrackBuilder, TrackHandle};
    use kira::{AudioManager, AudioManagerSettings, DefaultBackend, Tween};
    use quillstone_audio::{amplitude_to_db, channel_gain_db, Channel};

    /// A short fade instead of a hard cut, which would click.
    const DECLICK: f32 = 0.05;

    /// The music clock ticks this many times a second (the intro's length is counted in them).
    const CLOCK_HZ: f64 = 1000.0;
    /// How far ahead of the clock an intro is scheduled, in ticks: both streams must
    /// be queued before their start time passes, whatever the main thread's lag.
    const LEAD_TICKS: f64 = 60.0;

    pub struct Audio {
        assets: Assets,
        // Kept alive: dropping the manager stops all sound.
        _manager: AudioManager<DefaultBackend>,
        music_track: TrackHandle,
        sfx_track: TrackHandle,
        voice_track: TrackHandle,
        /// Times the start of a music intro and the loop that follows it.
        clock: ClockHandle,
        /// The track that is playing (the loop part, when there is an intro).
        music: Option<StreamingSoundHandle<FromFileError>>,
        /// The intro, while it may still be playing.
        music_intro: Option<StreamingSoundHandle<FromFileError>>,
        voice: Option<StreamingSoundHandle<FromFileError>>,
        sfx: HashMap<String, StaticSoundData>,
        /// The playing (streamed) loop of each looping sound effect, by file.
        sfx_loops: HashMap<String, StreamingSoundHandle<FromFileError>>,
        /// Loops that failed to start or died, not yet told to the mixer.
        failed_loops: Vec<String>,
        /// The last music request could not be played (a file that won't open, a
        /// device error, the loop after a started intro): `update` reports it like a
        /// track that ended, so the mixer forgets the music and a later `::music` restarts it.
        music_failed: bool,
        /// One-shot sound effects that may still be playing, so `stop=` can cut them.
        sfx_shots: Vec<(String, StaticSoundHandle)>,
        /// Files we already complained about, so a missing file warns once.
        warned: HashSet<String>,
    }

    impl Audio {
        /// Open the default sound device and preload the given sound effects
        /// (paths relative to the game's files). `None` if there is no usable device.
        pub fn new<'a>(assets: &Assets, sfx: impl Iterator<Item = &'a str>) -> Option<Audio> {
            let mut manager = match AudioManager::<DefaultBackend>::new(AudioManagerSettings::default()) {
                Ok(m) => m,
                Err(e) => {
                    eprintln!("warning: no audio ({e}); playing silent");
                    return None;
                }
            };
            let mut track = || manager.add_sub_track(TrackBuilder::new()).ok();
            let (music_track, sfx_track, voice_track) = (track()?, track()?, track()?);
            let mut clock = manager.add_clock(ClockSpeed::TicksPerSecond(CLOCK_HZ)).ok()?;
            clock.start();

            let mut audio = Audio {
                assets: assets.clone(),
                _manager: manager,
                music_track,
                sfx_track,
                voice_track,
                clock,
                music: None,
                music_intro: None,
                voice: None,
                sfx: HashMap::new(),
                sfx_loops: HashMap::new(),
                failed_loops: Vec::new(),
                music_failed: false,
                sfx_shots: Vec::new(),
                warned: HashSet::new(),
            };
            for src in sfx {
                if !audio.sfx.contains_key(src) {
                    match load(&audio.assets, src) {
                        Ok(data) => {
                            audio.sfx.insert(src.to_string(), data);
                        }
                        Err(e) => audio.warn(src, e),
                    }
                }
            }
            Some(audio)
        }

        /// Call every frame. `true` once when a one-shot music track has ended, or when
        /// the music could not be started (the mixer then forgets it).
        pub fn update(&mut self) -> bool {
            self.forget_finished_sfx();
            if self.voice.as_ref().is_some_and(|v| v.state() == PlaybackState::Stopped) {
                self.voice = None;
            }
            if self.music_intro.as_ref().is_some_and(|m| m.state() == PlaybackState::Stopped) {
                self.music_intro = None;
            }
            let ended = self.music.as_ref().is_some_and(|m| m.state() == PlaybackState::Stopped);
            if ended {
                self.music = None;
            }
            ended | std::mem::take(&mut self.music_failed)
        }

        /// Pause (or resume) everything that sounds, exactly where it is: the three tracks keep
        /// their sounds' positions, and the clock that times a music intro stops with them.
        pub fn set_paused(&mut self, paused: bool) {
            let t = tween(DECLICK);
            for track in [&mut self.music_track, &mut self.sfx_track, &mut self.voice_track] {
                if paused { track.pause(t) } else { track.resume(t) }
            }
            if paused { self.clock.pause() } else { self.clock.start() }
        }

        /// Whether a voice clip is still playing (auto-forward waits for it).
        pub fn voice_playing(&self) -> bool {
            self.voice.is_some()
        }

        pub fn run(&mut self, action: Action) {
            match action {
                Action::PlayMusic { src, intro, fade, looped, volume } => self.play_music(&src, intro.as_deref(), fade, looped, volume),
                Action::StopMusic { fade } => {
                    for mut music in [self.music.take(), self.music_intro.take()].into_iter().flatten() {
                        music.stop(tween(fade.max(DECLICK)));
                    }
                }
                Action::SetMusicVolume { volume, fade } => {
                    for music in [self.music.as_mut(), self.music_intro.as_mut()].into_iter().flatten() {
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

        /// Start a music track. With an `intro` that opens, the intro and the loop are
        /// both queued on the music clock: the intro at a moment just ahead, the loop
        /// exactly one intro-length after it, so the loop begins as the intro ends.
        /// An intro that cannot be opened is skipped (warned once): the loop plays alone.
        fn play_music(&mut self, src: &str, intro: Option<&str>, fade: f32, looped: bool, volume: f32) {
            self.music_failed = false;
            let Some(main) = self.stream(src) else {
                self.music_failed = true;
                return;
            };
            let gain = amplitude_to_db(volume);
            let mut main = main.volume(gain);
            if looped {
                main = main.loop_region(..);
            }
            let intro = intro.and_then(|name| self.stream(name).map(|data| (name, data)));
            let Some((intro_name, intro)) = intro else {
                if fade > 0.0 {
                    main = main.fade_in_tween(tween(fade));
                }
                match self.music_track.play(main) {
                    Ok(handle) => self.music = Some(handle),
                    Err(e) => {
                        self.warn(src, e);
                        self.music_failed = true;
                    }
                }
                return;
            };
            let begin = self.clock.time() + LEAD_TICKS;
            let join = begin + intro.duration().as_secs_f64() * CLOCK_HZ;
            let mut intro = intro.volume(gain).start_time(begin);
            if fade > 0.0 {
                intro = intro.fade_in_tween(tween(fade)); // only the intro fades in; the loop joins at full level
            }
            let main = main.start_time(join);
            match self.music_track.play(intro) {
                Ok(handle) => self.music_intro = Some(handle),
                Err(e) => {
                    self.warn(intro_name, e);
                    self.music_failed = true;
                    return;
                }
            }
            match self.music_track.play(main) {
                Ok(handle) => self.music = Some(handle),
                Err(e) => {
                    self.warn(src, e);
                    // the intro alone would end in silence the mixer doesn't know about
                    if let Some(mut intro) = self.music_intro.take() {
                        intro.stop(tween(DECLICK));
                    }
                    self.music_failed = true;
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
            match load(&self.assets, src) {
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
            let data = if crate::ffaudio::wants_ffmpeg(&self.assets, src) {
                crate::ffaudio::open(&self.assets, src)
            } else {
                self.assets.open(src).map_err(FromFileError::from).and_then(|f| StreamingSoundData::from_media_source(Source(f)))
            };
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
    fn load(assets: &Assets, src: &str) -> Result<StaticSoundData, FromFileError> {
        if crate::ffaudio::wants_ffmpeg(assets, src) {
            crate::ffaudio::load(assets, src)
        } else {
            StaticSoundData::from_media_source(Source(assets.open(src)?))
        }
    }

    /// A game file as symphonia (kira's decoder) wants it: it reads and seeks the file or archive entry
    /// directly, so music streams from the archive without being copied into memory.
    pub struct Source(pub AssetFile);

    impl Read for Source {
        fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
            self.0.read(buf)
        }
    }

    impl Seek for Source {
        fn seek(&mut self, to: SeekFrom) -> std::io::Result<u64> {
            self.0.seek(to)
        }
    }

    impl symphonia::core::io::MediaSource for Source {
        fn is_seekable(&self) -> bool {
            true
        }

        fn byte_len(&self) -> Option<u64> {
            self.0.len().ok()
        }
    }

    fn tween(seconds: f32) -> Tween {
        if seconds <= 0.0 {
            return Tween::default(); // kira's default is a 10 ms ramp
        }
        Tween { duration: Duration::from_secs_f32(seconds), ..Default::default() }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        /// A 16-bit mono WAV of `n` silent samples.
        fn wav(n: u32) -> Vec<u8> {
            let data = n * 2;
            let mut w = Vec::new();
            w.extend(b"RIFF");
            w.extend((36 + data).to_le_bytes());
            w.extend(b"WAVEfmt ");
            w.extend(16u32.to_le_bytes());
            w.extend([1, 0, 1, 0]); // PCM, mono
            w.extend(8000u32.to_le_bytes());
            w.extend(16000u32.to_le_bytes());
            w.extend([2, 0, 16, 0]);
            w.extend(b"data");
            w.extend(data.to_le_bytes());
            w.extend(vec![0u8; data as usize]);
            w
        }

        #[test]
        fn sounds_decode_from_an_archive_entry_through_a_media_source() {
            let dir = std::env::temp_dir().join(format!("qs-audio-archive-{}", std::process::id()));
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(dir.join("main.quill.md"), "Hi.\n").unwrap();
            std::fs::write(dir.join("a.wav"), wav(800)).unwrap();
            let list = [("main.quill.md".to_string(), dir.join("main.quill.md")), ("snd/a.wav".to_string(), dir.join("a.wav"))];
            quillstone_ui::archive::write(&dir.join("game.qsa"), "main.quill.md", &list).unwrap();
            let assets = Assets::archive(quillstone_ui::archive::Archive::open(&dir.join("game.qsa")).unwrap(), None);
            let data = load(&assets, "snd/a.wav").unwrap();
            assert_eq!((data.sample_rate, data.frames.len()), (8000, 800));
            assert!(StreamingSoundData::from_media_source(Source(assets.open("snd/a.wav").unwrap())).is_ok());
            assert!(load(&assets, "snd/missing.wav").is_err());
            let _ = std::fs::remove_dir_all(dir);
        }
    }
}
