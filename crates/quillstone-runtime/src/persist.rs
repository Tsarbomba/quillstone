//! Persistent data: what is kept across playthroughs, unlike saves.
//!
//! Two kinds of entries share one [`Vars`] map, kept by the [`Session`] and
//! written to disk by the frontend whenever it changes:
//! - the script's `persistent:` variables (endings seen, routes unlocked). They are
//!   ordinary variables in the story; after every step their values are copied
//!   here, and every new game, loaded save and rollback gets them back, so going
//!   back never takes an unlock away. (Replaying a line that adds
//!   to one adds again: yes/no flags are the safe kind.)
//! - `unlocked:<file>` for gallery pictures shown and music room tracks played.

use quillstone_script::{Program, Value, Vars};

use crate::save::{read_var, tokens, var_record};
use crate::{step, Cmd, Session, State};

/// Persistent entries `unlocked:<file>` mark gallery pictures and music room
/// tracks the player has seen or heard.
pub const UNLOCKED: &str = "unlocked:";

const MAGIC: &str = "quillstone-persistent 1";

impl<'p> Session<'p> {
    /// Start the script with what earlier playthroughs kept (see [`Session::persistent`]).
    pub fn start_with(program: &'p Program, persistent: Vars) -> (Session<'p>, Vec<Cmd>) {
        let mut session = Session {
            program,
            state: State::new(program),
            history: Vec::new(),
            persistent,
            persistent_changed: false,
        };
        session.overlay();
        let cmds = step(&mut session.state, program, None);
        session.note(&cmds);
        (session, cmds)
    }

    /// [`Session::load`], keeping `persistent` (the save's own copies of persistent
    /// variables may be older).
    pub fn load_with(program: &'p Program, text: &str, persistent: Vars) -> Result<(Session<'p>, Vec<Cmd>), String> {
        let (mut session, cmds) = Session::load(program, text)?;
        session.persistent = persistent;
        session.overlay();
        session.note(&cmds);
        Ok((session, cmds))
    }

    /// Replace what is kept across playthroughs (as read from disk), putting its
    /// variables into the current state. Doesn't count as a change to write back.
    pub fn set_persistent(&mut self, persistent: Vars) {
        self.persistent = persistent;
        self.persistent_changed = false;
        self.overlay();
    }

    /// What is kept across playthroughs: the script's persistent variables and the
    /// extras unlocked. Hand it to the next [`Session::start_with`] / [`Session::load_with`].
    pub fn persistent(&self) -> &Vars {
        &self.persistent
    }

    /// Whether [`Session::persistent`] changed since the last call (time to write it).
    pub fn take_persistent_changed(&mut self) -> bool {
        std::mem::take(&mut self.persistent_changed)
    }

    /// Whether a gallery picture or music room track has been unlocked.
    pub fn unlocked(&self, file: &str) -> bool {
        self.persistent.get(&format!("{UNLOCKED}{file}")).is_some_and(Value::truthy)
    }

    /// Keep what the story just did that lasts: persistent variables, extras shown.
    pub(crate) fn note(&mut self, cmds: &[Cmd]) {
        for name in &self.program.persistent {
            if let Some(value) = self.state.vars.get(name) {
                if self.persistent.get(name) != Some(value) {
                    self.persistent.insert(name.clone(), value.clone());
                    self.persistent_changed = true;
                }
            }
        }
        for cmd in cmds {
            let (src, list) = match cmd {
                Cmd::SetBackground { src, .. } | Cmd::ShowImage { src, .. } => (src, &self.program.gallery),
                Cmd::PlayMusic { src, .. } => (src, &self.program.music_room),
                _ => continue,
            };
            if list.contains(src) {
                let key = format!("{UNLOCKED}{src}");
                if self.persistent.insert(key, Value::Bool(true)).is_none() {
                    self.persistent_changed = true;
                }
            }
        }
    }

    /// Put the kept values of persistent variables into the current state.
    pub(crate) fn overlay(&mut self) {
        for name in &self.program.persistent {
            if let Some(value) = self.persistent.get(name) {
                self.state.vars.insert(name.clone(), value.clone());
            }
        }
    }
}

/// The persistent data file's text.
pub fn write_persistent(vars: &Vars) -> String {
    let mut out = format!("{MAGIC}\n");
    for (name, value) in vars {
        out.push_str(&var_record(name, value));
        out.push('\n');
    }
    out
}

/// Read [`write_persistent`] text. Unreadable lines are skipped, so a damaged
/// file loses only what it lost, never everything.
pub fn read_persistent(text: &str) -> Vars {
    let mut lines = text.lines();
    if lines.next().map(str::trim) != Some(MAGIC) {
        return Vars::new();
    }
    lines.filter_map(|l| tokens(l).ok()).filter_map(|t| read_var(&t).ok()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Input;
    use quillstone_script::parse;

    const STORY: &str = "---\npersistent: { good_end: false }\ngallery: [cg.png]\nmusic_room: [song.ogg]\n---\n# start\n::if{good_end}\nWelcome back.\n::endif\n::choice\n- [Be kind](#kind)\n- [Leave](#leave)\n::\n# kind\n::bg{src=cg.png}\n::music{src=song.ogg}\n::set{good_end=true}\nA warm ending.\n::end\n# leave\nYou leave.\n";

    fn says(cmds: &[Cmd], text: &str) -> bool {
        cmds.iter().any(|c| matches!(c, Cmd::Say { text: t, .. } if t == text))
    }

    #[test]
    fn persistent_values_carry_into_the_next_playthrough() {
        let p = parse(STORY).unwrap();
        let (mut s, first) = Session::start(&p);
        assert!(!says(&first, "Welcome back."));
        s.input(Input::Choose(0));
        assert!(s.take_persistent_changed());
        assert!(s.unlocked("cg.png") && s.unlocked("song.ogg"));
        assert_eq!(s.persistent()["good_end"], Value::Bool(true));

        let kept = read_persistent(&write_persistent(s.persistent()));
        assert_eq!(&kept, s.persistent(), "the file round-trips");
        let (_, again) = Session::start_with(&p, kept);
        assert!(says(&again, "Welcome back."), "a new game knows about the earlier ending");
    }

    #[test]
    fn rollback_and_old_saves_dont_take_unlocks_away() {
        let p = parse(STORY).unwrap();
        let (mut s, _) = Session::start(&p);
        let early_save = s.save();
        s.input(Input::Choose(0));
        s.rollback();
        assert_eq!(s.state().vars["good_end"], Value::Bool(true), "kept, even before the line that set it");
        assert!(s.unlocked("cg.png"));
        let (loaded, _) = Session::load_with(&p, &early_save, s.persistent().clone()).unwrap();
        assert_eq!(loaded.state().vars["good_end"], Value::Bool(true), "a save from before the ending still knows");
    }

    #[test]
    fn only_listed_files_unlock_and_damaged_files_lose_little() {
        let p = parse(STORY).unwrap();
        let (mut s, _) = Session::start(&p);
        s.input(Input::Choose(1));
        assert!(!s.unlocked("cg.png"));
        let text = format!("{MAGIC}\nvar \"a\" int 1\ngarbage \"\nvar \"b\" bool maybe\nvar \"c\" text \"x\"\n");
        let vars = read_persistent(&text);
        assert_eq!(vars.keys().collect::<Vec<_>>(), ["a", "c"]);
        assert!(read_persistent("something else\nvar \"a\" int 1\n").is_empty());
    }
}
