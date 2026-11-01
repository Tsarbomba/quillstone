//! Skip mode, and the record of which lines the player has read.
//!
//! A line counts as read once the player moves on from it. The record covers
//! every playthrough (it is kept in `read.txt` next to the saves) and is keyed
//! by [`line_id`], so it survives edits elsewhere in the script.
//!
//! Skipping races through lines already read (one every [`SKIP_DELAY`]),
//! finishing text and fades at once and muting sound effects and voice. It
//! stops at an unread line (unless the "skip unread" setting is on), at a
//! choice, at the end, and at anything the player does.

use std::collections::HashSet;

use super::*;
use quillstone_runtime::line_id;

/// Seconds between lines while skipping.
pub const SKIP_DELAY: f32 = 0.05;

/// Long enough to finish any fade, slide or typing in one step.
const SETTLE: f32 = 60.0;

const READ_FILE: &str = "read.txt";
const READ_MAGIC: &str = "quillstone-read 1";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SkipMode {
    /// Tab or the quick menu's Skip: on until toggled off or something stops it.
    Toggled,
    /// While Ctrl is held down.
    Held,
}

/// The lines read, and whether the file exists but is unreadable or damaged (then it is never overwritten).
pub(super) fn load_read(storage: &dyn Storage) -> (HashSet<u64>, bool) {
    let text = match storage.load(READ_FILE) {
        Ok(None) => return (HashSet::new(), false),
        Ok(Some(text)) if text.trim().is_empty() => return (HashSet::new(), false), // nothing to lose
        Ok(Some(text)) => text,
        Err(_) => return (HashSet::new(), true),
    };
    let mut lines = text.lines();
    if lines.next().map(str::trim) != Some(READ_MAGIC) {
        return (HashSet::new(), true);
    }
    (lines.filter_map(|l| u64::from_str_radix(l.trim(), 16).ok()).collect(), false)
}

impl Game<'_> {
    pub fn skipping(&self) -> bool {
        self.skip.is_some()
    }

    /// Whether the line on screen has been read before, in any playthrough.
    pub fn current_line_read(&self) -> bool {
        line_id(self.program, self.session.state().pc).is_some_and(|id| self.read.contains(&id))
    }

    fn waiting_on_a_line(&self) -> bool {
        matches!(self.session.state().pending, Some(Cmd::Say { .. }))
    }

    /// The player is leaving the line on screen: it has been read.
    pub(super) fn mark_current_read(&mut self) {
        if self.replay.is_some() || !self.waiting_on_a_line() {
            return; // (a replay only reads the record)
        }
        if let Some(id) = line_id(self.program, self.session.state().pc) {
            self.read_dirty |= self.read.insert(id);
        }
    }

    /// Write the read-lines record if it changed.
    pub(super) fn save_read(&mut self) {
        if !self.read_dirty || self.read_locked {
            return;
        }
        let mut ids: Vec<u64> = self.read.iter().copied().collect();
        ids.sort_unstable();
        let mut text = format!("{READ_MAGIC}\n");
        for id in ids {
            text.push_str(&format!("{id:016x}\n"));
        }
        match self.storage.write(READ_FILE, &text) {
            Ok(()) => self.read_dirty = false,
            Err(e) => self.notify(format!("Could not save which lines were read: {e}")),
        }
    }

    pub(super) fn start_skip(&mut self, mode: SkipMode) {
        if self.skip.is_some() {
            if mode == SkipMode::Toggled {
                self.stop_skip(); // Tab / Skip again turns it off
            }
            return;
        }
        if !self.settings.skip_unread && self.waiting_on_a_line() && !self.current_line_read() {
            self.notify("Nothing here has been read yet");
            return;
        }
        self.skip = Some(mode);
        self.skip_elapsed = SKIP_DELAY; // take the first step right away
    }

    pub(super) fn stop_skip(&mut self) {
        self.skip = None;
    }

    pub(super) fn release_skip(&mut self) {
        if self.skip == Some(SkipMode::Held) {
            self.stop_skip();
        }
    }

    /// One frame of skipping.
    pub(super) fn skip_tick(&mut self, dt: f32) {
        self.skip_elapsed += dt;
        if self.skip_elapsed < SKIP_DELAY {
            return;
        }
        self.skip_elapsed = 0.0;
        match &self.session.state().pending {
            Some(Cmd::Say { .. } | Cmd::Wait { .. } | Cmd::PlayVideo { skippable: true, .. }) => {
                self.forward(Input::Advance);
            }
            Some(Cmd::PlayVideo { .. }) => {} // can't be skipped: let it play out
            _ => self.stop_skip(),            // a choice, or the end
        }
        if self.skip.is_some() {
            self.view.settle(SETTLE); // fades and slides finish; animations keep their own pace
        }
    }

    /// The story moved on while skipping: stop at what mustn't be skipped.
    pub(super) fn skip_arrived(&mut self) {
        let stop = match &self.session.state().pending {
            Some(Cmd::Say { .. }) => !self.settings.skip_unread && !self.current_line_read(),
            Some(Cmd::ShowChoices { .. } | Cmd::ShowHotspots { .. } | Cmd::ShowShop { .. } | Cmd::ShowInventory | Cmd::ShowJournal | Cmd::AskInput { .. } | Cmd::ShowTiming { .. } | Cmd::ShowPress { .. })
            | None => true,
            Some(_) => false,
        };
        if stop {
            self.stop_skip();
        }
    }
}
