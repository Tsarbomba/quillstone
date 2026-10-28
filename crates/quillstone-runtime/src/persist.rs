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
        Session::start_seeded(program, persistent, crate::DEFAULT_SEED)
    }

    /// [`Session::start_with`] for a game whose `random(lo, hi)` rolls start from
    /// `seed`: the same seed and the same choices give the same game. The host
    /// picks it (the player uses the clock; tests and the command line use a fixed one).
    pub fn start_seeded(program: &'p Program, persistent: Vars, seed: u64) -> (Session<'p>, Vec<Cmd>) {
        let mut session = Session {
            program,
            state: State::with_seed(program, seed),
            history: Vec::new(),
            redo: Vec::new(),
            redo_tip: None,
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
        Session::load_with_edit(program, text, persistent, false)
    }

    /// [`Session::load_with`] for a hot reload: see [`Session::load_edited`].
    pub fn reload_with(program: &'p Program, text: &str, persistent: Vars) -> Result<(Session<'p>, Vec<Cmd>), String> {
        Session::load_with_edit(program, text, persistent, true)
    }

    fn load_with_edit(program: &'p Program, text: &str, persistent: Vars, edited: bool) -> Result<(Session<'p>, Vec<Cmd>), String> {
        let (mut session, mut cmds) = if edited { Session::load_edited(program, text)? } else { Session::load(program, text)? };
        session.persistent = persistent;
        if session.overlay() {
            // a kept value differs from the save's: the screen was rebuilt with it
            cmds = session.state.redraw();
        }
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

    /// Put the kept values of persistent variables into the current state. When one
    /// changed, what the story is waiting at is rebuilt from the script and the new
    /// values (choice texts and which options are on offer, hotspots, shop stock and
    /// prices, an interpolated line), so the screen and the state never disagree.
    /// Returns whether anything changed (the caller's screen may need redrawing).
    pub(crate) fn overlay(&mut self) -> bool {
        let mut changed = false;
        for name in &self.program.persistent {
            if let Some(value) = self.persistent.get(name) {
                // a kept value of another type (the script changed) is dropped
                let list = |v: Option<&Value>| matches!(v, Some(Value::List(_)));
                if list(Some(value)) != list(self.program.vars.get(name)) {
                    continue;
                }
                if self.state.vars.get(name) != Some(value) {
                    self.state.vars.insert(name.clone(), value.clone());
                    changed = true;
                }
            }
        }
        if changed {
            crate::refresh_pending(&mut self.state, self.program);
        }
        changed
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

    // ---- list variables ----

    const COLLECT: &str = "---\npersistent: { seen_items: [], other: 0 }\nvars: { bag: [] }\n---\n# a\n::set{seen_items += \"a b\", seen_items += 'say \"hi\"', seen_items += \"x, y\"}\n::set{bag += \"k\"}\nHi.\n::end\n";

    #[test]
    fn a_persistent_list_is_kept_across_playthroughs() {
        let p = parse(COLLECT).unwrap();
        let (s, _) = Session::start(&p);
        let want = Value::List(vec!["a b".into(), "say \"hi\"".into(), "x, y".into()]);
        assert_eq!(s.persistent()["seen_items"], want);
        assert!(!s.persistent().contains_key("bag"), "ordinary lists are not kept");
        let text = write_persistent(s.persistent());
        let kept = read_persistent(&text);
        assert_eq!(kept["seen_items"], want);
        let (next, _) = Session::start_with(&p, kept);
        assert_eq!(next.state().vars["seen_items"], want, "the next playthrough starts with them");
        assert_eq!(next.state().vars["bag"], Value::List(vec!["k".into()]));
    }

    #[test]
    fn an_empty_persistent_list_and_a_changed_type_are_safe() {
        let p = parse(COLLECT).unwrap();
        let kept = read_persistent(&write_persistent(&[("seen_items".to_string(), Value::List(vec![]))].into_iter().collect()));
        assert_eq!(kept["seen_items"], Value::List(vec![]));
        // the script now declares a number where the file kept a list: the number stays
        let mut odd = Vars::new();
        odd.insert("other".into(), Value::List(vec!["x".into()]));
        let (s, _) = Session::start_with(&p, odd);
        assert_eq!(s.state().vars["other"], Value::Int(0));
    }
}
