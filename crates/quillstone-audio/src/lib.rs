//! Quillstone audio, minus the sound card.
//!
//! [`Mixer`] turns the runtime's [`Cmd`]s into [`Action`]s for an audio backend
//! (`quillstone-player` uses kira). It owns the rules, so the backend stays a thin
//! executor and the rules are tested here:
//!
//! - Music keeps playing across a rollback redraw (or any `::music` of the track
//!   already playing); only its volume follows. A different track crossfades.
//! - A track with an `intro` plays the intro once and then loops. The same
//!   (src, intro) already playing is never restarted, whether the batch is a plain
//!   `::music`, a rollback redraw or a load; a different intro, or a different `src`,
//!   crossfades. Where the track was when the player rolled back or loaded is not
//!   kept: whatever starts afresh (another track, or nothing was playing) starts from
//!   the intro again.
//! - A voice clip belongs to its line: it stops as soon as the player acts.
//! - One-shot sound effects are fire-and-forget (the runtime never replays them on redraw).
//! - A looping sound effect belongs to the scene, like music: starting one that is
//!   already looping only changes its volume, and a redraw (rollback, load, hot
//!   reload) keeps the loops the earlier moment had and stops the others. Skipping
//!   mutes one-shots but not loops, so what plays always agrees with the story.
//!   A loop the backend failed to start (or lost) is reported with [`Mixer::loop_failed`].
//!
//! Every sound plays on one of three [`Channel`]s, each with its own volume, so
//! the player can offer music / sound / voice sliders.

use quillstone_runtime::Cmd;

/// The mixer channels a sound can play on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Channel {
    Music,
    Sfx,
    Voice,
}

/// Something the audio backend should do. Volumes are linear amplitude (0..1),
/// times in seconds; convert with [`amplitude_to_db`] where decibels are needed.
#[derive(Debug, Clone, PartialEq)]
pub enum Action {
    /// Start a music track, fading in over `fade`. Music that was playing before
    /// has already been sent a [`Action::StopMusic`], so the two crossfade.
    /// With an `intro`, the backend plays that file once and then loops `src`
    /// with no audible gap.
    PlayMusic { src: String, intro: Option<String>, fade: f32, looped: bool, volume: f32 },
    /// Fade out and drop the current music track.
    StopMusic { fade: f32 },
    /// Change the current music track's volume without restarting it.
    SetMusicVolume { volume: f32, fade: f32 },
    PlaySfx { src: String, volume: f32 },
    /// Start a looping sound effect on the sound channel, fading in over `fade`.
    PlaySfxLoop { src: String, fade: f32, volume: f32 },
    /// Change a playing loop's volume without restarting it.
    SetSfxLoopVolume { src: String, volume: f32, fade: f32 },
    /// Fade out the loop of `src` and any one-shot of it still playing; with
    /// `None`, everything on the sound channel.
    StopSfx { src: Option<String>, fade: f32 },
    /// Fade out only the loop of `src` (a redraw dropped it); one-shots of the same
    /// file play on.
    StopSfxLoop { src: String, fade: f32 },
    PlayVoice { src: String },
    /// Cut the voice clip of the line the player has left.
    StopVoice,
    /// The player moved a volume slider: `volume` is the slider position (0..1);
    /// turn it into a gain with [`channel_gain_db`].
    SetChannelVolume { channel: Channel, volume: f32 },
}

#[derive(Debug, Clone, PartialEq)]
struct Track {
    src: String,
    intro: Option<String>,
    looped: bool,
    volume: f32,
}

/// A looping sound effect that is playing.
#[derive(Debug, Clone, PartialEq)]
struct SfxLoop {
    src: String,
    volume: f32,
}

/// What is playing, as far as the story is concerned.
#[derive(Debug, Clone, Default)]
pub struct Mixer {
    music: Option<Track>,
    loops: Vec<SfxLoop>,
    voice: bool,
}

impl Mixer {
    pub fn new() -> Mixer {
        Mixer::default()
    }

    /// The music track that is playing, if any.
    pub fn music(&self) -> Option<&str> {
        self.music.as_ref().map(|t| t.src.as_str())
    }

    /// Whether any looping sound effect is playing.
    pub fn has_sfx_loops(&self) -> bool {
        !self.loops.is_empty()
    }

    /// Apply one batch of commands from the runtime (the same batch the view gets).
    pub fn apply(&mut self, cmds: &[Cmd]) -> Vec<Action> {
        let mut out = Vec::new();
        if cmds.is_empty() {
            return out;
        }
        // A new batch means the player acted, so the old line's voice is over.
        // (A rollback redraw re-sends the voice of the line it returns to.)
        self.stop_voice(&mut out);

        // A redraw says what the whole scene is: loops it doesn't list are over.
        let redraw = cmds.first() == Some(&Cmd::ClearScene);
        let mut listed: Vec<&str> = Vec::new();

        for cmd in cmds {
            match cmd {
                Cmd::PlayMusic { src, intro, fade, looped, volume } => {
                    self.play_music(&mut out, src, intro.as_deref(), *fade, *looped, *volume)
                }
                Cmd::StopMusic { fade } => {
                    if self.music.take().is_some() {
                        out.push(Action::StopMusic { fade: *fade });
                    }
                }
                Cmd::PlaySfx { src, volume } => out.push(Action::PlaySfx { src: src.clone(), volume: *volume }),
                Cmd::PlaySfxLoop { src, fade, volume } => {
                    listed.push(src);
                    self.play_loop(&mut out, src, *fade, *volume);
                }
                Cmd::StopSfx { src, fade } => {
                    match src {
                        Some(src) => self.loops.retain(|l| l.src != *src),
                        None => self.loops.clear(),
                    }
                    // One-shots aren't tracked (they end by themselves), so always tell the backend.
                    out.push(Action::StopSfx { src: src.clone(), fade: *fade });
                }
                Cmd::PlayVoice { src } => {
                    self.stop_voice(&mut out);
                    out.push(Action::PlayVoice { src: src.clone() });
                    self.voice = true;
                }
                _ => {}
            }
        }
        if redraw {
            let gone: Vec<String> = self.loops.iter().filter(|l| !listed.contains(&l.src.as_str())).map(|l| l.src.clone()).collect();
            for src in gone {
                self.loops.retain(|l| l.src != src);
                out.push(Action::StopSfxLoop { src, fade: 0.0 });
            }
        }
        out
    }

    /// The backend reports that a non-looping music track has played to its end,
    /// so asking for it again starts it over.
    pub fn music_ended(&mut self) {
        self.music = None;
    }

    /// The backend could not start the loop of `src` (a missing file, a decoder or
    /// device error) or it ended without being stopped: forget it, so a later
    /// `::sfx{loop}` or a redraw (rollback, load) starts it again.
    pub fn loop_failed(&mut self, src: &str) {
        self.loops.retain(|l| l.src != src);
    }

    fn play_music(&mut self, out: &mut Vec<Action>, src: &str, intro: Option<&str>, fade: f32, looped: bool, volume: f32) {
        if let Some(track) = &mut self.music {
            // The same loop with another intro (or none) is another piece: it crossfades.
            if track.src == src && track.looped == looped && track.intro.as_deref() == intro {
                if track.volume != volume {
                    track.volume = volume;
                    out.push(Action::SetMusicVolume { volume, fade });
                }
                return;
            }
            out.push(Action::StopMusic { fade });
        }
        let intro = intro.map(str::to_string);
        self.music = Some(Track { src: src.to_string(), intro: intro.clone(), looped, volume });
        out.push(Action::PlayMusic { src: src.to_string(), intro, fade, looped, volume });
    }

    fn play_loop(&mut self, out: &mut Vec<Action>, src: &str, fade: f32, volume: f32) {
        match self.loops.iter_mut().find(|l| l.src == src) {
            Some(l) => {
                if l.volume != volume {
                    l.volume = volume;
                    out.push(Action::SetSfxLoopVolume { src: src.to_string(), volume, fade });
                }
            }
            None => {
                self.loops.push(SfxLoop { src: src.to_string(), volume });
                out.push(Action::PlaySfxLoop { src: src.to_string(), fade, volume });
            }
        }
    }

    fn stop_voice(&mut self, out: &mut Vec<Action>) {
        if std::mem::take(&mut self.voice) {
            out.push(Action::StopVoice);
        }
    }
}

/// Quietest level in decibels; anything at or below it is silence.
pub const SILENCE_DB: f32 = -60.0;

/// Linear amplitude (1 = unchanged, 0 = silent) to decibels.
pub fn amplitude_to_db(amplitude: f32) -> f32 {
    if amplitude <= 0.0 {
        SILENCE_DB
    } else {
        (20.0 * amplitude.log10()).max(SILENCE_DB)
    }
}

/// Gain in decibels for a volume slider at `position` (0..1).
///
/// Loudness is not linear in amplitude: a slider that scaled amplitude directly
/// would barely change for most of its travel. Squaring the position makes the
/// middle of the slider sound about half as loud, which is what players expect.
pub fn channel_gain_db(position: f32) -> f32 {
    let p = position.clamp(0.0, 1.0);
    amplitude_to_db(p * p)
}

#[cfg(test)]
mod tests {
    use super::*;
    use quillstone_runtime::{Input, Session};
    use quillstone_script::parse;

    fn music(src: &str, fade: f32, volume: f32) -> Action {
        Action::PlayMusic { src: src.into(), intro: None, fade, looped: true, volume }
    }

    #[test]
    fn the_demo_starts_its_music() {
        let program = parse(include_str!("../../../examples/demo.quill.md")).unwrap();
        let (_, first) = Session::start(&program);
        let mut mixer = Mixer::new();
        assert_eq!(mixer.apply(&first), vec![music("assets/calm.ogg", 1.0, 0.8)]);
    }

    #[test]
    fn rollback_keeps_the_music_playing_instead_of_restarting_it() {
        let p = parse("::music{src=a.ogg}\n**A:** one\n**A:** two\n").unwrap();
        let (mut game, first) = Session::start(&p);
        let mut mixer = Mixer::new();
        mixer.apply(&first);
        assert!(mixer.apply(&game.input(Input::Advance)).is_empty());
        let redraw = game.rollback().unwrap();
        assert!(redraw.iter().any(|c| matches!(c, Cmd::PlayMusic { .. })));
        assert!(mixer.apply(&redraw).is_empty(), "same track: nothing to do");
    }

    #[test]
    fn rolling_back_to_before_the_music_started_stops_it() {
        let p = parse("**A:** quiet\n::music{src=a.ogg}\n**A:** loud\n").unwrap();
        let (mut game, first) = Session::start(&p);
        let mut mixer = Mixer::new();
        mixer.apply(&first);
        assert_eq!(mixer.apply(&game.input(Input::Advance)), vec![music("a.ogg", 0.0, 1.0)]);
        assert_eq!(mixer.apply(&game.rollback().unwrap()), vec![Action::StopMusic { fade: 0.0 }]);
        assert_eq!(mixer.music(), None);
    }

    #[test]
    fn a_new_track_crossfades_and_the_same_track_only_changes_volume() {
        let mut mixer = Mixer::new();
        let play = |src: &str, fade: f32, volume: f32| Cmd::PlayMusic { src: src.into(), intro: None, fade, looped: true, volume };
        mixer.apply(&[play("a.ogg", 0.0, 1.0)]);
        assert_eq!(
            mixer.apply(&[play("b.ogg", 2.0, 1.0)]),
            vec![Action::StopMusic { fade: 2.0 }, music("b.ogg", 2.0, 1.0)]
        );
        assert_eq!(mixer.apply(&[play("b.ogg", 1.0, 0.5)]), vec![Action::SetMusicVolume { volume: 0.5, fade: 1.0 }]);
        assert!(mixer.apply(&[play("b.ogg", 1.0, 0.5)]).is_empty());
    }

    #[test]
    fn stopping_when_nothing_plays_does_nothing() {
        let mut mixer = Mixer::new();
        assert!(mixer.apply(&[Cmd::StopMusic { fade: 1.0 }]).is_empty());
    }

    #[test]
    fn a_finished_one_shot_track_can_be_played_again() {
        let mut mixer = Mixer::new();
        let once = Cmd::PlayMusic { src: "jingle.ogg".into(), intro: None, fade: 0.0, looped: false, volume: 1.0 };
        mixer.apply(std::slice::from_ref(&once));
        mixer.music_ended();
        assert_eq!(mixer.apply(&[once]).len(), 1);
    }

    #[test]
    fn voice_stops_when_the_player_moves_on() {
        let p = parse("::voice{src=v1.ogg}\n**A:** one\n**A:** two\n::voice{src=v3.ogg}\n**A:** three\n").unwrap();
        let (mut game, first) = Session::start(&p);
        let mut mixer = Mixer::new();
        assert_eq!(mixer.apply(&first), vec![Action::PlayVoice { src: "v1.ogg".into() }]);
        assert_eq!(mixer.apply(&game.input(Input::Advance)), vec![Action::StopVoice]);
        assert_eq!(mixer.apply(&game.input(Input::Advance)), vec![Action::PlayVoice { src: "v3.ogg".into() }]);
        // rolling back to line two cuts line three's voice; line two has none
        assert_eq!(mixer.apply(&game.rollback().unwrap()), vec![Action::StopVoice]);
    }

    #[test]
    fn rolling_back_to_a_voiced_line_plays_its_voice_again() {
        let p = parse("::voice{src=v1.ogg}\n**A:** one\n**A:** two\n").unwrap();
        let (mut game, first) = Session::start(&p);
        let mut mixer = Mixer::new();
        mixer.apply(&first);
        mixer.apply(&game.input(Input::Advance));
        assert_eq!(mixer.apply(&game.rollback().unwrap()), vec![Action::PlayVoice { src: "v1.ogg".into() }]);
    }

    #[test]
    fn sound_effects_pass_through_and_are_not_replayed_by_rollback() {
        let p = parse("**A:** one\n::sfx{src=door.ogg volume=0.5}\n**A:** two\n").unwrap();
        let (mut game, first) = Session::start(&p);
        let mut mixer = Mixer::new();
        mixer.apply(&first);
        assert_eq!(
            mixer.apply(&game.input(Input::Advance)),
            vec![Action::PlaySfx { src: "door.ogg".into(), volume: 0.5 }]
        );
        game.input(Input::Advance);
        assert!(mixer.apply(&game.rollback().unwrap()).is_empty());
    }

    #[test]
    fn an_empty_batch_changes_nothing() {
        let mut mixer = Mixer::new();
        mixer.apply(&[Cmd::PlayVoice { src: "v.ogg".into() }]);
        assert!(mixer.apply(&[]).is_empty()); // ignored input: the voice keeps going
    }

    #[test]
    fn slider_gain_curve() {
        assert_eq!(channel_gain_db(1.0), 0.0);
        assert_eq!(channel_gain_db(0.0), SILENCE_DB);
        assert!((channel_gain_db(0.5) + 12.04).abs() < 0.01); // about half as loud
        assert_eq!(channel_gain_db(7.0), 0.0); // clamped
    }

    #[test]
    fn decibel_conversion() {
        assert_eq!(amplitude_to_db(1.0), 0.0);
        assert!((amplitude_to_db(0.5) + 6.0206).abs() < 1e-3);
        assert_eq!(amplitude_to_db(0.0), SILENCE_DB);
        assert_eq!(amplitude_to_db(0.0001), SILENCE_DB);
    }

    // ---- looping sound effects ------------------------------------------------

    fn playing(mixer: &Mixer) -> Vec<&str> {
        mixer.loops.iter().map(|l| l.src.as_str()).collect()
    }

    fn rain(fade: f32, volume: f32) -> Action {
        Action::PlaySfxLoop { src: "rain.ogg".into(), fade, volume }
    }

    fn stop(src: Option<&str>, fade: f32) -> Action {
        Action::StopSfx { src: src.map(String::from), fade }
    }

    fn stop_loop(src: &str) -> Action {
        Action::StopSfxLoop { src: src.into(), fade: 0.0 }
    }

    const LOOPS: &str = "::sfx{src=rain.ogg loop volume=0.6 fade=1}\n**A:** one\n::sfx{src=rain.ogg loop volume=0.6}\n::sfx{src=wind.ogg loop}\n**A:** two\n::sfx{stop=rain.ogg fade=2}\n**A:** three\n::sfx{stop}\n**A:** four\n";

    #[test]
    fn a_loop_starts_once_and_starting_it_again_only_changes_its_volume() {
        let mut mixer = Mixer::new();
        let start = |fade, volume| Cmd::PlaySfxLoop { src: "rain.ogg".into(), fade, volume };
        assert_eq!(mixer.apply(&[start(1.0, 0.6)]), vec![rain(1.0, 0.6)]);
        assert!(mixer.apply(&[start(1.0, 0.6)]).is_empty(), "idempotent");
        assert_eq!(
            mixer.apply(&[start(2.0, 0.3)]),
            vec![Action::SetSfxLoopVolume { src: "rain.ogg".into(), volume: 0.3, fade: 2.0 }]
        );
        assert_eq!(playing(&mixer), ["rain.ogg"]);
    }

    #[test]
    fn stopping_one_loop_or_all_of_them() {
        let mut mixer = Mixer::new();
        let start = |src: &str| Cmd::PlaySfxLoop { src: src.into(), fade: 0.0, volume: 1.0 };
        mixer.apply(&[start("rain.ogg"), start("wind.ogg")]);
        let out = mixer.apply(&[Cmd::StopSfx { src: Some("rain.ogg".into()), fade: 1.0 }]);
        assert_eq!(out, vec![stop(Some("rain.ogg"), 1.0)]);
        assert_eq!(playing(&mixer), ["wind.ogg"]);
        assert_eq!(mixer.apply(&[Cmd::StopSfx { src: None, fade: 0.5 }]), vec![stop(None, 0.5)]);
        assert!(!mixer.has_sfx_loops());
        // a stop is passed on even when no loop plays: a one-shot of that file may be
        assert_eq!(mixer.apply(&[Cmd::StopSfx { src: None, fade: 0.0 }]), vec![stop(None, 0.0)]);
        // and a stopped loop can start again
        assert_eq!(mixer.apply(&[start("rain.ogg")]).len(), 1);
    }

    #[test]
    fn rollback_keeps_loops_that_were_playing_and_stops_the_others() {
        let p = parse(LOOPS).unwrap();
        let (mut game, first) = Session::start(&p);
        let mut mixer = Mixer::new();
        assert_eq!(mixer.apply(&first), vec![rain(1.0, 0.6)]);
        assert_eq!(
            mixer.apply(&game.input(Input::Advance)),
            vec![Action::PlaySfxLoop { src: "wind.ogg".into(), fade: 0.0, volume: 1.0 }]
        );
        assert_eq!(mixer.apply(&game.input(Input::Advance)), vec![stop(Some("rain.ogg"), 2.0)]);
        // back on "two": the rain comes back (it had stopped), the wind plays on untouched
        let back = mixer.apply(&game.rollback().unwrap());
        assert_eq!(back, vec![rain(0.0, 0.6)]);
        // back on "one": the wind hadn't started yet, so it stops; the rain is untouched
        assert_eq!(mixer.apply(&game.rollback().unwrap()), vec![stop_loop("wind.ogg")]);
        assert_eq!(playing(&mixer), ["rain.ogg"]);
        // forward again to the end: `stop` clears the mixer too
        for _ in 0..3 {
            mixer.apply(&game.input(Input::Advance));
        }
        assert!(!mixer.has_sfx_loops());
    }

    #[test]
    fn a_redraw_with_nothing_looping_stops_every_loop_and_loading_a_save_agrees() {
        let p = parse(LOOPS).unwrap();
        let (mut game, first) = Session::start(&p);
        let mut mixer = Mixer::new();
        mixer.apply(&first);
        mixer.apply(&game.input(Input::Advance));
        let save = game.save();
        // load into a game whose mixer already plays what the save has: nothing to do
        let (loaded, cmds) = Session::load(&p, &save).unwrap();
        assert!(mixer.apply(&cmds).is_empty());
        assert_eq!(loaded.state().sfx_loops.len(), 2);
        // load a save from before any loop into the same mixer: both stop
        let plain = parse("**A:** x\n").unwrap();
        let (quiet, quiet_cmds) = Session::load(&plain, "quillstone-session 1\nstate\nat \"\" 0 0\nend\n").unwrap();
        assert!(quiet.state().sfx_loops.is_empty());
        assert_eq!(mixer.apply(&quiet_cmds), vec![stop_loop("rain.ogg"), stop_loop("wind.ogg")]);
        // a fresh mixer (hot reload after a restart) starts them from the save
        let mut fresh = Mixer::new();
        let (_, cmds) = Session::load(&p, &save).unwrap();
        assert_eq!(fresh.apply(&cmds).len(), 2);
    }

    #[test]
    fn a_failed_loop_is_forgotten_so_it_starts_again() {
        let mut mixer = Mixer::new();
        let start = Cmd::PlaySfxLoop { src: "rain.ogg".into(), fade: 0.5, volume: 0.6 };
        assert_eq!(mixer.apply(std::slice::from_ref(&start)), vec![rain(0.5, 0.6)]);
        assert!(mixer.apply(std::slice::from_ref(&start)).is_empty(), "believed to be playing");
        mixer.loop_failed("rain.ogg");
        assert!(!mixer.has_sfx_loops());
        assert_eq!(mixer.apply(&[start]), vec![rain(0.5, 0.6)], "the next `::sfx{{loop}}` tries again");
        mixer.loop_failed("never-started.ogg"); // unknown files are ignored
        assert_eq!(playing(&mixer), ["rain.ogg"]);
    }

    #[test]
    fn a_redraw_restarts_a_loop_the_backend_lost() {
        let p = parse(LOOPS).unwrap();
        let (game, first) = Session::start(&p);
        let mut mixer = Mixer::new();
        mixer.apply(&first);
        mixer.loop_failed("rain.ogg");
        let redraw = game.state().redraw();
        assert_eq!(mixer.apply(&redraw), vec![rain(0.0, 0.6)]);
    }

    #[test]
    fn a_redraw_drops_only_the_loop_not_a_one_shot_of_the_same_file() {
        let mut mixer = Mixer::new();
        mixer.apply(&[Cmd::PlaySfxLoop { src: "rain.ogg".into(), fade: 0.0, volume: 1.0 }]);
        let out = mixer.apply(&[Cmd::ClearScene]);
        assert_eq!(out, vec![stop_loop("rain.ogg")]);
        assert!(!out.iter().any(|a| matches!(a, Action::StopSfx { .. })));
        // a script's `::sfx{stop=rain.ogg}` still cuts both
        mixer.apply(&[Cmd::PlaySfxLoop { src: "rain.ogg".into(), fade: 0.0, volume: 1.0 }]);
        assert_eq!(mixer.apply(&[Cmd::StopSfx { src: Some("rain.ogg".into()), fade: 1.0 }]), vec![stop(Some("rain.ogg"), 1.0)]);
    }

    #[test]
    fn one_shot_sound_effects_never_touch_the_loops() {
        let mut mixer = Mixer::new();
        mixer.apply(&[Cmd::PlaySfxLoop { src: "rain.ogg".into(), fade: 0.0, volume: 1.0 }]);
        let out = mixer.apply(&[Cmd::PlaySfx { src: "rain.ogg".into(), volume: 1.0 }]);
        assert_eq!(out, vec![Action::PlaySfx { src: "rain.ogg".into(), volume: 1.0 }]);
        assert!(mixer.has_sfx_loops());
    }

    // ---- music intro -------------------------------------------------------------

    fn intro_play(src: &str, intro: Option<&str>, fade: f32) -> Cmd {
        Cmd::PlayMusic { src: src.into(), intro: intro.map(Into::into), fade, looped: true, volume: 1.0 }
    }

    fn intro_action(src: &str, intro: Option<&str>, fade: f32) -> Action {
        Action::PlayMusic { src: src.into(), intro: intro.map(Into::into), fade, looped: true, volume: 1.0 }
    }

    #[test]
    fn a_track_with_an_intro_starts_once_and_the_same_one_never_restarts() {
        let mut mixer = Mixer::new();
        let play = intro_play("loop.ogg", Some("intro.ogg"), 1.0);
        assert_eq!(mixer.apply(std::slice::from_ref(&play)), vec![intro_action("loop.ogg", Some("intro.ogg"), 1.0)]);
        assert_eq!(mixer.music(), Some("loop.ogg"));
        assert!(mixer.apply(&[play]).is_empty(), "the same (src, intro) keeps playing");
        // only the volume follows
        let quieter = Cmd::PlayMusic { src: "loop.ogg".into(), intro: Some("intro.ogg".into()), fade: 2.0, looped: true, volume: 0.5 };
        assert_eq!(mixer.apply(&[quieter]), vec![Action::SetMusicVolume { volume: 0.5, fade: 2.0 }]);
    }

    #[test]
    fn a_redraw_or_load_of_the_same_intro_track_keeps_it_and_starts_afresh_otherwise() {
        let p = parse("::music{src=loop.ogg intro=intro.ogg}\n**A:** one\n**A:** two\n").unwrap();
        let (mut game, first) = Session::start(&p);
        let mut mixer = Mixer::new();
        assert_eq!(mixer.apply(&first), vec![intro_action("loop.ogg", Some("intro.ogg"), 0.0)]);
        mixer.apply(&game.input(Input::Advance));
        assert!(mixer.apply(&game.rollback().unwrap()).is_empty(), "rollback while it plays: untouched");
        let text = game.save();
        let (_, loaded) = Session::load(&p, &text).unwrap();
        assert!(mixer.apply(&loaded).is_empty(), "so is a load of the same moment");
        // a fresh mixer (nothing playing, e.g. after the menu) starts from the intro
        let mut fresh = Mixer::new();
        assert_eq!(fresh.apply(&loaded), vec![intro_action("loop.ogg", Some("intro.ogg"), 0.0)]);
    }

    #[test]
    fn another_intro_or_no_intro_crossfades() {
        let mut mixer = Mixer::new();
        mixer.apply(&[intro_play("loop.ogg", Some("a.ogg"), 0.0)]);
        assert_eq!(
            mixer.apply(&[intro_play("loop.ogg", Some("b.ogg"), 2.0)]),
            vec![Action::StopMusic { fade: 2.0 }, intro_action("loop.ogg", Some("b.ogg"), 2.0)]
        );
        assert_eq!(
            mixer.apply(&[intro_play("loop.ogg", None, 1.0)]),
            vec![Action::StopMusic { fade: 1.0 }, intro_action("loop.ogg", None, 1.0)]
        );
        assert_eq!(
            mixer.apply(&[intro_play("other.ogg", Some("b.ogg"), 1.0)]),
            vec![Action::StopMusic { fade: 1.0 }, intro_action("other.ogg", Some("b.ogg"), 1.0)]
        );
    }

    #[test]
    fn a_music_room_entry_unlocks_for_the_loop_file_not_the_intro() {
        let p = parse("---\npersistent: { done: false }\nmusic_room: [loop.ogg]\n---\n::music{src=loop.ogg intro=intro.ogg}\n**A:** hi\n").unwrap();
        assert!(p.assets().any(|(_, _, f)| f == "intro.ogg"), "the intro is still a game file");
        let (_, cmds) = Session::start(&p);
        assert!(cmds.iter().any(|c| matches!(c, Cmd::PlayMusic { src, .. } if src == "loop.ogg")));
    }
}
