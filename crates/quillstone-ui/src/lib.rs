//! Quillstone's game shell, minus the window: input routing, settings, the
//! backlog and the quick menu.
//!
//! - [`game`]: [`Game`] ties the runtime, the view and the audio mixer together
//!   and decides what each click or key does on the current screen.
//! - [`settings`]: player preferences and their file format.
//! - [`slots`]: save slots, the save file header, thumbnails, and the
//!   [`Storage`] the frontend provides for reading and writing files.
//! - [`screens`]: layout, hit-testing and drawing for the settings screen, the
//!   backlog and the quick menu; [`extras`]: the same for the gallery and music room.
//!
//! The player (`player/`) only translates window events into [`Event`]s and
//! carries out what comes back, so all of this is unit-tested here.

pub mod archive;
pub mod assets;
pub mod bag;
pub mod console;
pub mod extras;
pub mod game;
pub mod input;
pub mod screens;
pub mod settings;
pub mod slots;
pub mod source;
pub mod texts;

pub use assets::Assets;
pub use game::{Button, Event, Game, Key, Request, Screen, AUTO_PER_CHAR};
pub use settings::{QuickMenuSetting, Settings};
pub use quillstone_runtime::{start_flags, StartAt, MAX_SAVE_BYTES};
pub use slots::{MemoryStorage, Storage, Timestamp};

/// Write a file so that a crash or power cut midway can't leave it half written: write a temporary file next
/// to it (flushed to disk), then rename it into place. Missing folders are created; a failure leaves no temp file.
pub fn write_atomic(path: &std::path::Path, text: &str) -> std::io::Result<()> {
    use std::io::Write;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension("tmp");
    let mut file = std::fs::File::create(&tmp)?;
    file.write_all(text.as_bytes())?;
    file.sync_all()?; // on disk before the rename, so a power cut can't leave an empty file in place
    drop(file);
    std::fs::rename(&tmp, path).inspect_err(|_| {
        let _ = std::fs::remove_file(&tmp);
    })
}

#[cfg(test)]
mod tests {
    #[test]
    fn write_atomic_replaces_the_file_and_leaves_no_temp_file_behind() {
        let dir = std::env::temp_dir().join(format!("quillstone-atomic-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let file = dir.join("deep/f.txt");
        super::write_atomic(&file, "one").unwrap();
        super::write_atomic(&file, "two").unwrap();
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "two");
        assert!(!file.with_extension("tmp").exists());
        // a directory in the way of the rename fails, and the temp file does not stay
        std::fs::create_dir(dir.join("deep/d.txt")).unwrap();
        assert!(super::write_atomic(&dir.join("deep/d.txt"), "x").is_err());
        assert!(!dir.join("deep/d.tmp").exists());
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
