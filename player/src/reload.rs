//! Hot reload: save the script (or a file it includes, or its theme) and the running game picks the
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
    /// Watch the script, the files it includes, the theme file it names and the translation files.
    pub fn new(script: &Path, program: &Program, root: &Path) -> Watch {
        let mut watch = Watch { files: Vec::new(), last: Instant::now() };
        watch.add(quillstone_ui::source::script_files(script, program));
        watch.add(program.theme.as_ref().map(|t| root.join(t)));
        watch.add(quillstone_ui::source::translation_files(script, program));
        watch
    }

    /// Also watch these files (those already watched stay as they are).
    pub fn add(&mut self, paths: impl IntoIterator<Item = PathBuf>) {
        for path in paths {
            if !self.files.iter().any(|(p, _)| *p == path) {
                let seen = modified(&path);
                self.files.push((path, seen));
            }
        }
    }

    /// The watched files, in the order they were added (the script first).
    #[cfg(test)]
    fn paths(&self) -> Vec<&Path> {
        self.files.iter().map(|(p, _)| p.as_path()).collect()
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

#[cfg(test)]
mod tests {
    use super::*;

    fn dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("qs-reload-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("ch")).unwrap();
        dir
    }

    #[test]
    fn included_files_are_watched_with_the_script_and_theme() {
        let root = dir("watch");
        let main = root.join("main.quill.md");
        let text = "---\ntheme: t.txt\n---\n::include{ch/a.quill.md}\n";
        std::fs::write(&main, text).unwrap();
        std::fs::write(root.join("ch/a.quill.md"), "::include{ch/b.quill.md}\n").unwrap();
        std::fs::write(root.join("ch/b.quill.md"), "Hi.\n").unwrap();
        let program = quillstone_ui::source::parse_script(&main, text).unwrap();
        let mut watch = Watch::new(&main, &program, &root);
        assert_eq!(watch.paths(), [main.as_path(), &root.join("ch/a.quill.md"), &root.join("ch/b.quill.md"), &root.join("t.txt")]);
        // saving an included file is a change, once
        watch.last = Instant::now() - EVERY;
        assert!(!watch.changed());
        std::thread::sleep(Duration::from_millis(20));
        std::fs::write(root.join("ch/b.quill.md"), "Changed.\n").unwrap();
        let later = SystemTime::now() + Duration::from_secs(5);
        std::fs::File::options().write(true).open(root.join("ch/b.quill.md")).unwrap().set_modified(later).unwrap();
        watch.last = Instant::now() - EVERY;
        assert!(watch.changed());
        watch.last = Instant::now() - EVERY;
        assert!(!watch.changed());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn a_file_with_an_error_can_be_added_and_nothing_is_watched_twice() {
        let root = dir("add");
        let main = root.join("main.quill.md");
        std::fs::write(&main, "Hi.\n").unwrap();
        let program = quillstone_ui::source::parse_script(&main, "Hi.\n").unwrap();
        let mut watch = Watch::new(&main, &program, &root);
        watch.add([root.join("ch/new.quill.md"), main.clone(), root.join("ch/new.quill.md")]);
        assert_eq!(watch.paths(), [main.as_path(), &root.join("ch/new.quill.md")]);
        let _ = std::fs::remove_dir_all(root);
    }
}
