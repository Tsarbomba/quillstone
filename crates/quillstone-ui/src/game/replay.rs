//! Scene replays: the Extras "Scenes" tab starts a fresh story at a label; it runs
//! like the real one but changes nothing that lasts, and ends back at Extras.

use quillstone_script::Vars;

use super::*;
use crate::extras::Tab;

/// What a replay in progress remembers.
#[derive(Debug, Clone)]
pub(super) struct ReplayRun {
    pub label: String,
    /// The player's real persistent data from before the replay (the replay's own
    /// session works on a copy that is thrown away).
    pub persistent: Vars,
}

/// Shown when something that would save or load is tried during a replay.
pub(super) const NOT_IN_REPLAY: &str = "Not available during a replay";

impl Game<'_> {
    /// Whether a scene replay is running.
    pub fn in_replay(&self) -> bool {
        self.replay.is_some()
    }

    /// Replay the scene that starts at `label` (a `replays:` entry): a fresh game with the
    /// script's starting variables and the player's persistent data, an empty stage,
    /// no menu music. Nothing it does is saved. It ends at `::end_replay` or when the
    /// story ends, back at the Extras screen. `false` if there is no such label or a
    /// story is already running.
    pub fn start_replay(&mut self, label: &str) -> bool {
        if self.in_story || self.replay.is_some() || !self.program.replays.iter().any(|r| r.label == label) {
            return false;
        }
        let persistent = self.session.persistent().clone();
        let seed = quillstone_runtime::next_seed(&mut self.seed);
        let Some((session, first)) = Session::start_replay(self.program, persistent.clone(), seed, label) else { return false };
        self.replay = Some(ReplayRun { label: label.to_string(), persistent });
        self.session = session;
        self.extras.playing = None;
        self.extras.viewing = None;
        self.fresh_view();
        self.audio.extend(self.mixer.apply(&[Cmd::StopMusic { fade: 0.5 }])); // the menu's music
        self.in_story = true;
        self.under = None;
        self.screen = Screen::Story;
        self.show(&first);
        self.replay_check(&first);
        true
    }

    /// End the replay when these commands say so (`::end_replay`, or the story ending).
    pub(super) fn replay_check(&mut self, cmds: &[Cmd]) {
        if self.replay.is_some() && cmds.iter().any(|c| matches!(c, Cmd::EndReplay | Cmd::Finished)) {
            self.end_replay();
        }
    }

    /// Leave the replay for the Extras screen on the Scenes tab, with the menu music
    /// back and the player's own data as it was.
    pub(super) fn end_replay(&mut self) {
        let Some(run) = self.replay.take() else { return };
        self.confirm = None;
        let (mut session, _) = Session::start(self.program); // replaced by Start, Continue or Load
        session.set_persistent(run.persistent);
        self.session = session;
        self.go_to_menu();
        let at = self.program.replays.iter().position(|r| r.label == run.label).unwrap_or(0);
        let per = ex::per_page(Tab::Scenes);
        let tab_exists = self.extras_tabs().contains(&Tab::Scenes);
        if tab_exists {
            self.extras = Extras { tab: Tab::Scenes, page: at / per, focus: Some(at % per), viewing: None, playing: None };
            self.screen = Screen::Extras;
        }
    }
}
