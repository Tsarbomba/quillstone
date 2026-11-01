//! Helpers the test files share.

use crate::{Cmd, Input, Session};

/// `save` with its records starting with `prefix` (such as `"hud "`) dropped and `lines` put right after the `rng` line.
pub(crate) fn replace_records(save: &str, prefix: &str, lines: &[impl AsRef<str>]) -> String {
    let mut out = String::new();
    let mut done = false;
    for l in save.lines().filter(|l| !l.starts_with(prefix)) {
        out.push_str(l);
        out.push('\n');
        if l.starts_with("rng ") && !done {
            done = true;
            for r in lines {
                out.push_str(r.as_ref());
                out.push('\n');
            }
        }
    }
    out
}

/// Whether one of `cmds` says `text`.
pub(crate) fn says(cmds: &[Cmd], text: &str) -> bool {
    cmds.iter().any(|c| matches!(c, Cmd::Say { text: t, .. } if t == text))
}

/// Advance one step.
pub(crate) fn next(game: &mut Session) -> Vec<Cmd> {
    game.input(Input::Advance)
}
