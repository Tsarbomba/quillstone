//! Persistent data: what is kept across playthroughs, unlike saves.
//!
//! Two kinds of entries share one [`Vars`] map, kept by the [`Session`] and
//! written to disk by the frontend whenever it changes:
//! - the script's `persistent:` variables (endings seen, routes unlocked). They are
//!   ordinary variables in the story; after every step their values are copied
//!   here, and every new game, loaded save and rollback gets them back, so going
//!   back never takes an unlock away. (Replaying a line that adds
//!   to one adds again: yes/no flags are the safe kind.)
//! - `unlocked:<file>` for gallery pictures shown and music room tracks played, and
//!   `replay:<label>` for scenes reached (the Extras "Scenes" tab).

use quillstone_script::{Program, Value, Vars};

use crate::save::{read_var, tokens, var_record};
use crate::{step, Cmd, Session, State};

/// Persistent entries `unlocked:<file>` mark gallery pictures and music room
/// tracks the player has seen or heard.
pub const UNLOCKED: &str = "unlocked:";

/// Persistent entries `replay:<label>` mark scenes the story has reached, which
/// the Extras menu then lets the player replay.
pub const REPLAY: &str = "replay:";

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

    /// Begin a scene replay: a fresh game (the script's starting variables, `persistent`
    /// applied, an empty scene, an empty call stack) that starts at `label`. `None` if
    /// there is no such label. The host decides what to keep of what happens in it.
    pub fn start_replay(program: &'p Program, persistent: Vars, seed: u64, label: &str) -> Option<(Session<'p>, Vec<Cmd>)> {
        let pc = *program.labels.get(label)?;
        let mut session = Session {
            program,
            state: State::with_seed(program, seed),
            history: Vec::new(),
            redo: Vec::new(),
            redo_tip: None,
            persistent,
            persistent_changed: false,
        };
        session.state.pc = pc;
        session.overlay();
        let mut cmds = vec![Cmd::ClearScene];
        cmds.extend(step(&mut session.state, program, None));
        session.note(&cmds);
        Some((session, cmds))
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

    /// Whether the story has reached the label of the replay `label`.
    pub fn replay_unlocked(&self, label: &str) -> bool {
        self.persistent.get(&format!("{REPLAY}{label}")).is_some_and(Value::truthy)
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
            if let Cmd::ReplayReached { label } = cmd {
                if self.persistent.insert(format!("{REPLAY}{label}"), Value::Bool(true)).is_none() {
                    self.persistent_changed = true;
                }
                continue;
            }
            // An animation as the background shows each of its frames: the ones the gallery
            // lists unlock (a sprite animation doesn't count, only backgrounds).
            if let Cmd::SetBackground { src, .. } = cmd {
                if let Some(anim) = quillstone_script::anim_id(src).and_then(|id| self.program.anims.get(id)) {
                    for frame in anim.frames.iter().filter(|f| self.program.gallery.contains(&f.file)) {
                        if self.persistent.insert(format!("{UNLOCKED}{}", frame.file), Value::Bool(true)).is_none() {
                            self.persistent_changed = true;
                        }
                    }
                    continue;
                }
            }
            let (src, list) = match cmd {
                // a video entry unlocks as a looped background (or as a `::video`)
                Cmd::SetBackground { src, .. } | Cmd::ShowImage { src, .. } | Cmd::PlayVideo { src, .. } => {
                    (src, &self.program.gallery)
                }
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

/// Whether `text` starts like a [`write_persistent`] file (a damaged one is better left alone than overwritten).
pub fn is_persistent_file(text: &str) -> bool {
    text.lines().next().map(str::trim) == Some(MAGIC)
}

/// Read [`write_persistent`] text. Unreadable lines are skipped, so a damaged
/// file loses only what it lost, never everything.
pub fn read_persistent(text: &str) -> Vars {
    if !is_persistent_file(text) {
        return Vars::new();
    }
    text.lines().skip(1).filter_map(|l| tokens(l).ok()).filter_map(|t| read_var(&t).ok()).collect()
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

    // ---- video entries ----

    const CLIPS: &str = "---\ngallery: [cg.png, loop.webm, cut.webm, other.webm]\n---\n# start\nHi.\n::bg{src=loop.webm}\nOne.\n::video{src=cut.webm}\nTwo.\n";

    #[test]
    fn a_gallery_video_unlocks_as_a_background_or_a_cutscene_and_survives_the_file() {
        let p = parse(CLIPS).unwrap();
        let (mut s, _) = Session::start(&p);
        assert!(!s.unlocked("loop.webm"));
        s.input(Input::Advance);
        assert!(s.unlocked("loop.webm") && !s.unlocked("cut.webm") && !s.unlocked("cg.png"));
        s.input(Input::Advance);
        s.input(Input::Advance);
        assert!(s.unlocked("cut.webm") && !s.unlocked("other.webm"));
        let kept = read_persistent(&write_persistent(s.persistent()));
        assert_eq!(&kept, s.persistent(), "same records as for pictures");
        s.rollback();
        assert!(s.unlocked("loop.webm"), "going back takes nothing away");
    }

    #[test]
    fn an_unlock_file_cannot_unlock_what_the_script_does_not_list() {
        let p = parse(CLIPS).unwrap();
        let text = format!("{MAGIC}\nvar \"unlocked:evil.webm\" bool true\nvar \"unlocked:loop.webm\" bool true\n");
        let (s, _) = Session::start_with(&p, read_persistent(&text));
        assert!(s.unlocked("loop.webm"));
        // the gallery is the script's list: an entry nobody declared is never offered
        assert!(!p.gallery.iter().any(|g| g == "evil.webm") && !s.unlocked("other.webm"));
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

    // ---- scene replays ----

    const SCENES: &str = "---\nvars: { n: 0 }\npersistent: { flag: false }\nreplays: { first: \"First\", second: \"Second\", third: \"Third\", sub: \"Sub\" }\n---\n# first\n::set{n += 1}\n**A:** in first\n::end_replay\n**A:** after first\n::choice\n- [Go](#second)\n- [Jump](#elsewhere)\n::\n# second\n**A:** in second\n::call{to=sub}\n::jump{to=third}\n# sub\n**A:** in sub\n::return\n# third\n**A:** in third\n::end\n# elsewhere\n::end\n";

    #[test]
    fn a_scene_unlocks_when_the_story_reaches_its_label() {
        let p = parse(SCENES).unwrap();
        let (mut s, first) = Session::start(&p);
        // the script starts at `first`: reaching a label by running into it counts
        assert!(first.contains(&Cmd::ReplayReached { label: "first".into() }));
        assert!(s.replay_unlocked("first") && !s.replay_unlocked("second") && !s.replay_unlocked("third"));
        assert!(s.take_persistent_changed());
        assert_eq!(s.persistent()["replay:first"], Value::Bool(true));
        for _ in 0..2 {
            s.input(Input::Advance); // ::end_replay does nothing in a real game
        }
        let cmds = s.input(Input::Choose(0)); // `::choice` link to #second
        assert!(cmds.contains(&Cmd::ReplayReached { label: "second".into() }));
        assert!(s.replay_unlocked("second") && !s.replay_unlocked("sub"));
        s.input(Input::Advance); // ::call into `sub`
        assert!(s.replay_unlocked("sub"), "::call reaches it");
        assert!(!s.replay_unlocked("third"));
        s.input(Input::Advance); // ::return, then ::jump to `third`
        assert!(s.replay_unlocked("third"), "::jump reaches it");
        // the record is written with the other persistent data and read back
        let kept = read_persistent(&write_persistent(s.persistent()));
        let (again, _) = Session::start_with(&p, kept);
        assert!(again.replay_unlocked("third"));
    }

    #[test]
    fn rollback_and_old_saves_dont_take_a_scene_away() {
        let p = parse(SCENES).unwrap();
        let (mut s, _) = Session::start(&p);
        let early = s.save();
        s.input(Input::Advance);
        s.input(Input::Advance);
        s.input(Input::Choose(0));
        s.rollback();
        s.rollback();
        s.rollback();
        assert!(s.replay_unlocked("second"), "still unlocked after going back");
        let (loaded, _) = Session::load_with(&p, &early, s.persistent().clone()).unwrap();
        assert!(loaded.replay_unlocked("second"));
    }

    #[test]
    fn a_replay_session_starts_fresh_at_the_label() {
        let p = parse(SCENES).unwrap();
        let mut kept = Vars::new();
        kept.insert("flag".into(), Value::Bool(true));
        kept.insert("n".into(), Value::Int(9)); // not a persistent variable: ignored
        let (s, cmds) = Session::start_replay(&p, kept, 7, "second").unwrap();
        assert_eq!(cmds[0], Cmd::ClearScene, "an empty stage");
        assert!(says(&cmds, "in second"));
        assert_eq!(s.state().vars["n"], Value::Int(0), "the script's starting variables");
        assert_eq!(s.state().vars["flag"], Value::Bool(true), "the player's persistent data");
        assert!(s.state().stack.is_empty() && s.state().background.is_none() && s.state().sprites.is_empty());
        assert!(Session::start_replay(&p, Vars::new(), 7, "nowhere").is_none());
        // `::return` with nothing to return to ends it, like `::end_replay` says so
        let (mut s, _) = Session::start_replay(&p, Vars::new(), 7, "sub").unwrap();
        assert_eq!(s.input(Input::Advance), vec![Cmd::Finished]);
        let (mut s, _) = Session::start_replay(&p, Vars::new(), 7, "first").unwrap();
        assert_eq!(s.state().vars["n"], Value::Int(1));
        assert!(s.input(Input::Advance).contains(&Cmd::EndReplay));
    }

    #[test]
    fn end_replay_in_a_real_game_lets_the_story_go_on() {
        let p = parse("**A:** one\n::end_replay\n**A:** two\n").unwrap();
        let (mut s, _) = Session::start(&p);
        let cmds = s.input(Input::Advance);
        assert!(says(&cmds, "two"), "{cmds:?}");
    }
}
