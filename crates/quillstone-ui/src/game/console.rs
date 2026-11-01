//! The developer console's behaviour (see `console.rs` for the commands): opened with F12, only
//! when the host turned it on with [`Game::set_dev`]. While it is open it takes every key and the
//! game stands still. What it changes is real state: saves include it.

use quillstone_runtime::filter_typed;

use super::*;
use crate::console::{self, Command, Console};

impl Game<'_> {
    /// Turn the developer console on (the player's `--dev`). Off by default, so the key does nothing.
    pub fn set_dev(&mut self, on: bool) {
        self.dev = on;
        if !on {
            self.console = None;
        }
    }

    /// Start a new game at `at.label` with `at.sets` (the player's `--start` / `--set`).
    pub fn start_dev(&mut self, at: &quillstone_runtime::StartAt) -> Result<(), String> {
        let seed = quillstone_runtime::next_seed(&mut self.seed);
        let (session, first) = Session::start_at(self.program, self.session.persistent().clone(), seed, at)?;
        self.enter_story(session, &first);
        Ok(())
    }

    /// F12 while the story is showing (not behind a dialog, an overlay screen or a replay).
    pub(super) fn open_console(&mut self) {
        if self.dev && self.in_story && self.screen == Screen::Story && self.confirm.is_none() && self.replay.is_none() {
            self.stop_skip();
            self.console = Some(Console { lines: vec!["developer console: help lists the commands".into()], ..Console::default() });
        }
    }

    pub(super) fn console_event(&mut self, event: Event) {
        let Some(c) = &mut self.console else { return };
        match event {
            Event::Text(s) => {
                let room = console::MAX_LEN.saturating_sub(c.text.chars().count());
                c.text.extend(filter_typed(&s).chars().take(room));
                c.since_key = 0.0;
            }
            Event::Key(Key::Backspace) => {
                c.text.pop();
                c.since_key = 0.0;
            }
            Event::Key(Key::Enter) => {
                let typed = std::mem::take(&mut c.text);
                if !typed.trim().is_empty() {
                    self.run_console(&typed);
                }
            }
            Event::Key(Key::Escape | Key::F12) => self.console = None,
            _ => {} // every other key, click and wheel is ignored while it is open
        }
    }

    fn run_console(&mut self, typed: &str) {
        let lines = match console::parse(typed).and_then(|cmd| self.exec_console(cmd)) {
            Ok(lines) => lines,
            Err(message) => vec![format!("error: {message}")],
        };
        if let Some(c) = &mut self.console {
            c.lines = lines;
        }
    }

    /// Show the screen again from the state (after variables or the position changed under it).
    fn redraw_from_state(&mut self) {
        self.view.dialogue = None;
        let cmds = self.session.state().redraw();
        self.show(&cmds);
    }

    fn exec_console(&mut self, cmd: Command) -> Result<Vec<String>, String> {
        let vars = &self.session.state().vars;
        Ok(match cmd {
            Command::Help => console::HELP.iter().map(|l| l.to_string()).collect(),
            Command::Get(name) => vec![console::show_var(vars, &name)],
            Command::Vars(prefix) => console::list_vars(vars, &prefix),
            Command::Set(src) => {
                self.session.dev_set(&src)?;
                self.redraw_from_state();
                let name = src.split(|c: char| !(c.is_alphanumeric() || c == '_' || c == '.')).next().unwrap_or("");
                vec![console::show_var(&self.session.state().vars, name)]
            }
            Command::Jump(label) => {
                let cmds = self.session.dev_jump(&label)?;
                self.auto = false;
                self.view.dialogue = None;
                self.show(&cmds);
                vec![format!("jumped to `{}` (rollback goes back)", quillstone_script::slug(label.trim().trim_start_matches('#')))]
            }
        })
    }
}
