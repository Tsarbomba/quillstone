//! Where a running game gets its files: a folder (development, `--folder` packages) or a game archive
//! (see [`crate::archive`]) with an optional folder of loose files beside it that win over the archive's
//! copies, so a patch or a mod is just a file at the same path. Paths are the ones the script spells
//! (`/` separators, inside the game folder); anything else is refused, in both modes.

use std::collections::HashSet;
use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use quillstone_script::is_inside;

use crate::archive::{Archive, Slice};

/// An opened game file: a loose file, or a part of the archive.
#[derive(Debug)]
pub enum AssetFile {
    File(File),
    Slice(Slice),
}

impl AssetFile {
    /// The file's length in bytes.
    pub fn len(&self) -> io::Result<u64> {
        match self {
            AssetFile::File(f) => Ok(f.metadata()?.len()),
            AssetFile::Slice(s) => Ok(s.len()),
        }
    }

    pub fn is_empty(&self) -> io::Result<bool> {
        Ok(self.len()? == 0)
    }
}

impl Read for AssetFile {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        match self {
            AssetFile::File(f) => f.read(buf),
            AssetFile::Slice(s) => s.read(buf),
        }
    }
}

impl Seek for AssetFile {
    fn seek(&mut self, to: SeekFrom) -> io::Result<u64> {
        match self {
            AssetFile::File(f) => f.seek(to),
            AssetFile::Slice(s) => s.seek(to),
        }
    }
}

#[derive(Debug)]
enum Backend {
    Folder(PathBuf),
    /// Later patch archives would be more entries to look in, before this one.
    Archive { archive: Archive, overlay: Option<PathBuf> },
}

/// The game's files. Cheap to clone and share between threads.
#[derive(Debug, Clone)]
pub struct Assets(Arc<Backend>);

fn not_inside(path: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, format!("`{path}` is not a path inside the game folder"))
}

fn not_found(path: &str) -> io::Error {
    io::Error::new(io::ErrorKind::NotFound, format!("`{path}` is not in the game's files"))
}

impl Assets {
    /// Files in a folder (the script's own folder).
    pub fn folder(root: impl Into<PathBuf>) -> Assets {
        Assets(Arc::new(Backend::Folder(root.into())))
    }

    /// Files in `archive`, except those that also exist loose under `overlay`.
    pub fn archive(archive: Archive, overlay: Option<PathBuf>) -> Assets {
        Assets(Arc::new(Backend::Archive { archive, overlay }))
    }

    /// The archive's main script path, in archive mode.
    pub fn main_script(&self) -> Option<&str> {
        match &*self.0 {
            Backend::Folder(_) => None,
            Backend::Archive { archive, .. } => Some(archive.main()),
        }
    }

    /// The loose file for `path` (folder mode: the file in the folder; archive mode: only if the overlay has it).
    fn loose(&self, path: &str) -> io::Result<Option<PathBuf>> {
        if !is_inside(path) {
            return Err(not_inside(path));
        }
        Ok(match &*self.0 {
            Backend::Folder(root) => Some(root.join(path)),
            Backend::Archive { overlay, .. } => overlay.as_ref().map(|o| o.join(path)).filter(|p| p.is_file()),
        })
    }

    fn archived(&self, path: &str) -> Option<Slice> {
        match &*self.0 {
            Backend::Folder(_) => None,
            Backend::Archive { archive, .. } => archive.slice(path),
        }
    }

    /// Open a game file for reading and seeking.
    pub fn open(&self, path: &str) -> io::Result<AssetFile> {
        match self.loose(path)? {
            Some(file) => File::open(file).map(AssetFile::File),
            None => self.archived(path).map(AssetFile::Slice).ok_or_else(|| not_found(path)),
        }
    }

    /// A whole game file.
    pub fn read(&self, path: &str) -> io::Result<Vec<u8>> {
        let mut file = self.open(path)?;
        let mut bytes = Vec::with_capacity(file.len().unwrap_or(0).min(1 << 28) as usize);
        file.read_to_end(&mut bytes)?;
        Ok(bytes)
    }

    /// A whole game file as UTF-8 text.
    pub fn read_to_string(&self, path: &str) -> io::Result<String> {
        String::from_utf8(self.read(path)?).map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "the file is not UTF-8 text"))
    }

    pub fn exists(&self, path: &str) -> bool {
        match self.loose(path) {
            Ok(Some(file)) => file.is_file(),
            Ok(None) => self.archived(path).is_some(),
            Err(_) => false,
        }
    }

    /// The names of the files directly inside `folder` (`""` is the game folder itself): the loose ones first,
    /// then the archive's (sorted), none twice.
    pub fn list(&self, folder: &str) -> Vec<String> {
        if !folder.is_empty() && !is_inside(folder) {
            return Vec::new();
        }
        let mut names = Vec::new();
        let dir = |root: &Path| {
            let entries = std::fs::read_dir(if folder.is_empty() { root.to_path_buf() } else { root.join(folder) });
            entries
                .into_iter()
                .flatten()
                .filter_map(|e| e.ok())
                .filter(|e| e.path().is_file())
                .filter_map(|e| e.file_name().into_string().ok())
                .collect::<Vec<_>>()
        };
        match &*self.0 {
            Backend::Folder(root) => names = dir(root),
            Backend::Archive { archive, overlay } => {
                if let Some(o) = overlay {
                    names = dir(o);
                }
                let prefix = if folder.is_empty() { String::new() } else { format!("{folder}/") };
                let mut packed: Vec<&str> =
                    archive.paths().filter_map(|p| p.strip_prefix(&prefix)).filter(|n| !n.contains('/')).collect();
                packed.sort_unstable();
                let seen: HashSet<String> = names.iter().cloned().collect();
                names.extend(packed.into_iter().filter(|n| !seen.contains(*n)).map(String::from));
            }
        }
        names
    }

    /// What to hand FFmpeg to open `path`: the file's path, or for an archive entry FFmpeg's `subfile` protocol
    /// naming the archive and the entry's byte range. A path outside the game folder or not in the game is an
    /// error, never handed to FFmpeg as it is (FFmpeg would take `http:...` and the like as a URL).
    pub fn ffmpeg_url(&self, path: &str) -> io::Result<String> {
        if let Some(file) = self.loose(path)? {
            return Ok(file.to_string_lossy().into_owned());
        }
        match &*self.0 {
            Backend::Archive { archive, .. } => archive
                .range(path)
                .map(|(start, len)| subfile_url(&archive.path().to_string_lossy(), start, len))
                .ok_or_else(|| not_found(path)),
            Backend::Folder(_) => Err(not_found(path)),
        }
    }
}

/// FFmpeg's `subfile` URL for `len` bytes at `start` of the file `archive`. `end` is the byte after the range.
/// The options end at `,,` and the first `:` after that starts the path, which is taken as it is (so a
/// Windows `C:\...` path and commas in folder names are fine).
fn subfile_url(archive: &str, start: u64, len: u64) -> String {
    format!("subfile,,start,{start},end,{},,:{archive}", start + len)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("qs-assets-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn put(root: &Path, path: &str, text: &str) -> PathBuf {
        let file = root.join(path);
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(&file, text).unwrap();
        file
    }

    /// An archive of `files` and a loose folder beside it holding `loose`.
    fn game(name: &str, files: &[(&str, &str)], loose: &[(&str, &str)]) -> (Assets, PathBuf) {
        let d = dir(name);
        let list: Vec<_> = files.iter().map(|(p, t)| (p.to_string(), put(&d.join("src"), p, t))).collect();
        crate::archive::write(&d.join("game.qsa"), files[0].0, &list).unwrap();
        for (p, t) in loose {
            put(&d.join("game"), p, t);
        }
        (Assets::archive(Archive::open(&d.join("game.qsa")).unwrap(), Some(d.join("game"))), d)
    }

    #[test]
    fn a_folder_reads_its_files_and_refuses_paths_outside() {
        let d = dir("folder");
        put(&d, "a/b.txt", "hi");
        put(&d, "a/c.txt", "yo");
        let a = Assets::folder(&d);
        assert_eq!(a.read_to_string("a/b.txt").unwrap(), "hi");
        assert!(a.exists("a/b.txt") && !a.exists("a/x.txt") && !a.exists("../x"));
        assert_eq!(a.read("a/x.txt").unwrap_err().kind(), io::ErrorKind::NotFound);
        assert_eq!(a.read("../x").unwrap_err().kind(), io::ErrorKind::InvalidInput);
        let mut names = a.list("a");
        names.sort();
        assert_eq!(names, ["b.txt", "c.txt"]);
        assert!(a.list("nope").is_empty() && a.list("..").is_empty());
        assert_eq!(a.ffmpeg_url("a/b.txt").unwrap(), d.join("a/b.txt").to_string_lossy());
        assert_eq!(a.main_script(), None);
    }

    #[test]
    fn a_loose_file_wins_over_the_archive_including_the_script() {
        let (a, d) = game(
            "overlay",
            &[("main.quill.md", "old"), ("t.txt", "packed"), ("voice/a.ogg", "A"), ("voice/b.ogg", "B"), ("voice/sub/c.ogg", "C")],
            &[("main.quill.md", "patched"), ("voice/b.ogg", "loose B"), ("voice/z.ogg", "Z"), ("new.txt", "mod")],
        );
        assert_eq!(a.main_script(), Some("main.quill.md"));
        assert_eq!(a.read_to_string("main.quill.md").unwrap(), "patched");
        assert_eq!(a.read_to_string("t.txt").unwrap(), "packed");
        assert_eq!(a.read_to_string("voice/b.ogg").unwrap(), "loose B");
        assert_eq!(a.read_to_string("voice/a.ogg").unwrap(), "A");
        assert_eq!(a.read_to_string("new.txt").unwrap(), "mod");
        assert!(a.exists("voice/z.ogg") && a.exists("voice/sub/c.ogg") && !a.exists("voice/sub"));
        let mut loose: Vec<_> = a.list("voice");
        let packed = loose.split_off(2);
        loose.sort();
        assert_eq!(loose, ["b.ogg", "z.ogg"], "loose first");
        assert_eq!(packed, ["a.ogg"], "then the archive's, without repeats");
        assert!(a.list("voice/sub") == ["c.ogg"] && a.list("nope").is_empty());
        assert_eq!(a.list("").len(), 3, "root: main, t.txt, new.txt, and nothing from subfolders");
        for bad in ["../x", "/etc/passwd", "a\\b", "C:x"] {
            assert_eq!(a.read(bad).unwrap_err().kind(), io::ErrorKind::InvalidInput, "{bad}");
        }
        let _ = std::fs::remove_dir_all(d);
    }

    #[test]
    fn without_an_overlay_folder_only_the_archive_counts() {
        let (a, d) = game("no-overlay", &[("main.quill.md", "m")], &[]);
        let Backend::Archive { archive, .. } = &*a.0 else { unreachable!() };
        let a = Assets::archive(Archive::open(archive.path()).unwrap(), Some(d.join("missing")));
        assert_eq!(a.read_to_string("main.quill.md").unwrap(), "m");
        assert!(!a.exists("other.txt"));
        let _ = std::fs::remove_dir_all(d);
    }

    #[test]
    fn open_seeks_within_an_archived_file() {
        let (a, d) = game("seek", &[("main.quill.md", "m"), ("n.txt", "0123456789")], &[]);
        let mut f = a.open("n.txt").unwrap();
        assert_eq!(f.len().unwrap(), 10);
        f.seek(SeekFrom::Start(7)).unwrap();
        let mut s = String::new();
        f.read_to_string(&mut s).unwrap();
        assert_eq!(s, "789");
        let _ = std::fs::remove_dir_all(d);
    }

    #[test]
    fn ffmpeg_urls_for_both_modes() {
        let (a, d) = game("url", &[("main.quill.md", "m"), ("v.webm", "video")], &[("loose.webm", "x")]);
        let (start, len) = (8192, 5); // the script takes the first 4096-byte page
        let archive = d.join("game.qsa").to_string_lossy().into_owned();
        assert_eq!(a.ffmpeg_url("v.webm").unwrap(), format!("subfile,,start,{start},end,{},,:{archive}", start + len));
        assert_eq!(a.ffmpeg_url("loose.webm").unwrap(), d.join("game/loose.webm").to_string_lossy(), "a loose file is plain");
        for bad in ["http://example.com/v.webm", "../v.webm", "missing.webm"] {
            assert!(a.ffmpeg_url(bad).is_err(), "{bad} must never reach FFmpeg");
        }
        assert_eq!(
            subfile_url(r"C:\Games\My Game\game.qsa", 8192, 100),
            r"subfile,,start,8192,end,8292,,:C:\Games\My Game\game.qsa"
        );
        let _ = std::fs::remove_dir_all(d);
    }
}
