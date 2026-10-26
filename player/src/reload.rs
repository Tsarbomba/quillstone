//! Hot reload: save the script (or its theme) and the running game picks the
//! change up at the same spot (see `quillstone_ui::Game::reload`).
//!
//! The files are polled (twice a second), which needs no file-watching library
//! and works the same on every system and editor.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};

use quillstone_script::Program;

const EVERY: Duration = Duration::from_millis(500);

pub struct Watch {
    files: Vec<(PathBuf, Option<SystemTime>)>,
    last: Instant,
}

fn modified(path: &Path) -> Option<SystemTime> {
    std::fs::metadata(path).and_then(|m| m.modified()).ok()
}

impl Watch {
    /// Watch the script and the theme file it names.
    pub fn new(script: &Path, program: &Program, root: &Path) -> Watch {
        let mut paths = vec![script.to_path_buf()];
        paths.extend(program.theme.as_ref().map(|t| root.join(t)));
        let files = paths
            .into_iter()
            .map(|path| {
                let seen = modified(&path);
                (path, seen)
            })
            .collect();
        Watch { files, last: Instant::now() }
    }

    /// Whether a watched file was saved since the last call (looks at most twice a second).
    pub fn changed(&mut self) -> bool {
        if self.last.elapsed() < EVERY {
            return false;
        }
        self.last = Instant::now();
        let mut changed = false;
        for (path, seen) in &mut self.files {
            let now = modified(path);
            if now.is_some() && now != *seen {
                *seen = now;
                changed = true;
            }
        }
        changed
    }
}
