//! Quillstone audio, minus the sound card.
//!
//! [`Mixer`] turns the runtime's [`Cmd`]s into [`Action`]s for an audio backend
//! (`quillstone-player` uses kira). It owns the rules, so the backend stays a thin
//! executor and the rules are tested here:
//!
//! - Music keeps playing across a rollback redraw (or any `::music` of the track
//!   already playing); only its volume follows. A different track crossfades.
//! - A voice clip belongs to its line: it stops as soon as the player acts.
//! - Sound effects are fire-and-forget (the runtime never replays them on redraw).
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
    PlayMusic { src: String, fade: f32, looped: bool, volume: f32 },
    /// Fade out and drop the current music track.
    StopMusic { fade: f32 },
    /// Change the current music track's volume without restarting it.
    SetMusicVolume { volume: f32, fade: f32 },
    PlaySfx { src: String, volume: f32 },
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
    looped: bool,
    volume: f32,
}

/// What is playing, as far as the story is concerned.
#[derive(Debug, Clone, Default)]
pub struct Mixer {
    music: Option<Track>,
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

    /// Apply one batch of commands from the runtime (the same batch the view gets).
    pub fn apply(&mut self, cmds: &[Cmd]) -> Vec<Action> {
        let mut out = Vec::new();
        if cmds.is_empty() {
            return out;
        }
        // A new batch means the player acted, so the old line's voice is over.
        // (A rollback redraw re-sends the voice of the line it returns to.)
        self.stop_voice(&mut out);

        for cmd in cmds {
            match cmd {
                Cmd::PlayMusic { src, fade, looped, volume } => self.play_music(&mut out, src, *fade, *looped, *volume),
                Cmd::StopMusic { fade } => {
                    if self.music.take().is_some() {
                        out.push(Action::StopMusic { fade: *fade });
                    }
                }
                Cmd::PlaySfx { src, volume } => out.push(Action::PlaySfx { src: src.clone(), volume: *volume }),
                Cmd::PlayVoice { src } => {
                    self.stop_voice(&mut out);
                    out.push(Action::PlayVoice { src: src.clone() });
                    self.voice = true;
                }
                _ => {}
            }
        }
        out
    }

    /// The backend reports that a non-looping music track has played to its end,
    /// so asking for it again starts it over.
    pub fn music_ended(&mut self) {
        self.music = None;
    }

    fn play_music(&mut self, out: &mut Vec<Action>, src: &str, fade: f32, looped: bool, volume: f32) {
        if let Some(track) = &mut self.music {
            if track.src == src && track.looped == looped {
                if track.volume != volume {
                    track.volume = volume;
                    out.push(Action::SetMusicVolume { volume, fade });
                }
                return;
            }
            out.push(Action::StopMusic { fade });
        }
        self.music = Some(Track { src: src.to_string(), looped, volume });
        out.push(Action::PlayMusic { src: src.to_string(), fade, looped, volume });
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
        Action::PlayMusic { src: src.into(), fade, looped: true, volume }
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
        let play = |src: &str, fade: f32, volume: f32| Cmd::PlayMusic { src: src.into(), fade, looped: true, volume };
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
        let once = Cmd::PlayMusic { src: "jingle.ogg".into(), fade: 0.0, looped: false, volume: 1.0 };
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
}
