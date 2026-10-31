//! The game data archive (`.qsa`): a shipped game's files in one file, stored as they are (no
//! compression, no scrambling: it consolidates, it doesn't hide). All numbers are little-endian.
//!
//! ```text
//! header (32 bytes)   "QSARC" + 3 zero bytes, version u32 (1), reserved u32,
//!                     index offset u64, index length u64
//! files               each as-is, starting on a 4096-byte boundary (zero padding before it)
//! index               main script path, entry count u32, then per entry:
//!                     path (u32 length + UTF-8, `/` separators, as the script spells it), offset u64, length u64
//!                     (the main script path is also length-prefixed)
//! ```
//!
//! An archive is read as untrusted input (players and modders may hand over odd files): [`Archive::open`]
//! checks every number and path before anything is read. A [`Slice`] then reads one entry with positioned
//! reads, so any number of threads and streams can read the same archive with no shared cursor.

use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use quillstone_script::is_inside;

const MAGIC: &[u8; 8] = b"QSARC\0\0\0";
const VERSION: u32 = 1;
const HEADER_LEN: u64 = 32;
/// Every file starts on a multiple of this (a disk page, so streaming reads line up).
pub const ALIGN: u64 = 4096;
/// The index is read whole into memory, so a damaged or hostile file can't ask for more than this.
pub const MAX_INDEX_BYTES: u64 = 64 << 20;
/// The most files one archive may hold.
pub const MAX_ENTRIES: usize = 1 << 20;
const MAX_PATH_BYTES: usize = 4096;

/// Write an archive of `files` (archive path, file on disk) to `out`, in the order given, with `main` (one of
/// the paths) as the game's script. Paths must be unique and inside the game folder.
/// It is written under a temporary name, opened again as the player will, and only then moved to `out`:
/// a failure leaves no partial file, and nothing is written that [`Archive::open`] refuses.
pub fn write(out: &Path, main: &str, files: &[(String, PathBuf)]) -> Result<(), String> {
    let mut tmp = out.as_os_str().to_owned();
    tmp.push(".part");
    let tmp = PathBuf::from(tmp);
    let done = write_file(&tmp, main, files)
        .and_then(|()| Archive::open(&tmp).map(drop))
        .and_then(|()| std::fs::rename(&tmp, out).map_err(|e| format!("cannot write {}: {e}", out.display())));
    if done.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    done
}

fn write_file(out: &Path, main: &str, files: &[(String, PathBuf)]) -> Result<(), String> {
    let mut names = std::collections::HashSet::new();
    for (path, _) in files {
        if !is_inside(path) {
            return Err(format!("can't archive `{path}`: it is not a path inside the game folder"));
        }
        if !names.insert(path.as_str()) {
            return Err(format!("can't archive `{path}` twice"));
        }
    }
    if !names.contains(main) {
        return Err(format!("the main script `{main}` is not among the files"));
    }
    let io = |what: &str, e: io::Error| format!("cannot {what} {}: {e}", out.display());
    let mut w = io::BufWriter::new(File::create(out).map_err(|e| io("create", e))?);
    w.write_all(&[0u8; HEADER_LEN as usize]).map_err(|e| io("write", e))?;
    let mut at = HEADER_LEN;
    let mut entries = Vec::with_capacity(files.len());
    for (path, from) in files {
        let start = at.div_ceil(ALIGN) * ALIGN;
        io::copy(&mut io::repeat(0).take(start - at), &mut w).map_err(|e| io("write", e))?;
        let mut src = File::open(from).map_err(|e| format!("cannot read {}: {e}", from.display()))?;
        let len = io::copy(&mut src, &mut w).map_err(|e| format!("cannot copy {}: {e}", from.display()))?;
        entries.push((path, start, len));
        at = start + len;
    }
    let mut index = Vec::new();
    put_str(&mut index, main);
    index.extend((entries.len() as u32).to_le_bytes());
    for (path, offset, len) in entries {
        put_str(&mut index, path);
        index.extend(offset.to_le_bytes());
        index.extend(len.to_le_bytes());
    }
    w.write_all(&index).map_err(|e| io("write", e))?;
    let mut file = w.into_inner().map_err(|e| io("write", e.into_error()))?;
    let mut header = Vec::with_capacity(HEADER_LEN as usize);
    header.extend(MAGIC);
    header.extend(VERSION.to_le_bytes());
    header.extend(0u32.to_le_bytes());
    header.extend(at.to_le_bytes());
    header.extend((index.len() as u64).to_le_bytes());
    file.seek(SeekFrom::Start(0)).and_then(|_| file.write_all(&header)).map_err(|e| io("write", e))?;
    file.sync_all().map_err(|e| io("write", e))
}

fn put_str(out: &mut Vec<u8>, s: &str) {
    out.extend((s.len() as u32).to_le_bytes());
    out.extend(s.as_bytes());
}

/// path -> (offset, length)
type Entries = HashMap<String, (u64, u64)>;

/// An opened archive: its index in memory, its data read on demand.
#[derive(Debug)]
pub struct Archive {
    path: PathBuf,
    file: Arc<File>,
    main: String,
    entries: Entries,
    /// Lower-cased path -> the entry's own path, for the names that are unique that way (see [`Archive::range`]).
    folded: HashMap<String, String>,
}

impl Archive {
    /// Open and validate `path`. Errors are plain sentences naming the problem.
    pub fn open(path: &Path) -> Result<Archive, String> {
        let name = path.display();
        let file = File::open(path).map_err(|e| format!("cannot open {name}: {e}"))?;
        let size = file.metadata().map_err(|e| format!("cannot read {name}: {e}"))?.len();
        let bad = |m: &str| format!("{name} is not a usable game archive: {m}");
        if size < HEADER_LEN {
            return Err(bad("it is too short to be one"));
        }
        let mut file = file;
        let mut head = [0u8; HEADER_LEN as usize];
        file.read_exact(&mut head).map_err(|e| format!("cannot read {name}: {e}"))?;
        if &head[..8] != MAGIC {
            return Err(bad("it does not start with the archive marker"));
        }
        let version = u32::from_le_bytes(head[8..12].try_into().unwrap());
        if version != VERSION {
            return Err(bad(&format!("it is version {version}, and this player reads version {VERSION}")));
        }
        let index_at = u64::from_le_bytes(head[16..24].try_into().unwrap());
        let index_len = u64::from_le_bytes(head[24..32].try_into().unwrap());
        if index_len > MAX_INDEX_BYTES {
            return Err(bad("its index is too large"));
        }
        if index_at < HEADER_LEN || index_at.checked_add(index_len).is_none_or(|end| end > size) {
            return Err(bad("its index lies outside the file (it was cut short?)"));
        }
        let mut index = vec![0u8; index_len as usize];
        file.seek(SeekFrom::Start(index_at))
            .and_then(|_| file.read_exact(&mut index))
            .map_err(|e| format!("cannot read {name}: {e}"))?;
        let (main, entries) = parse_index(&index, index_at).map_err(|m| bad(&m))?;
        let mut folded: HashMap<String, String> = HashMap::new();
        let mut twice = HashSet::new();
        for path in entries.keys() {
            let key = path.to_lowercase();
            if folded.insert(key.clone(), path.clone()).is_some() {
                twice.insert(key);
            }
        }
        folded.retain(|key, _| !twice.contains(key));
        Ok(Archive { path: path.to_path_buf(), file: Arc::new(file), main, entries, folded })
    }

    /// Where the archive file is on disk.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// The path of the game's script.
    pub fn main(&self) -> &str {
        &self.main
    }

    pub fn contains(&self, path: &str) -> bool {
        self.range(path).is_some()
    }

    /// The paths of every file inside (in no particular order).
    pub fn paths(&self) -> impl Iterator<Item = &str> {
        self.entries.keys().map(String::as_str)
    }

    /// `(offset, length)` of an entry within the archive file. A path that differs only in letter case
    /// finds the entry when that is unambiguous: the game folder on Windows ignores case, so a script
    /// (`{pose}.png` with `pose="happy"`) that works from the folder must work from the archive too.
    pub fn range(&self, path: &str) -> Option<(u64, u64)> {
        let exact = self.entries.get(path);
        exact.or_else(|| self.entries.get(self.folded.get(&path.to_lowercase())?)).copied()
    }

    /// A reader of one entry's bytes.
    pub fn slice(&self, path: &str) -> Option<Slice> {
        let (start, len) = self.range(path)?;
        Some(Slice { file: self.file.clone(), start, len, pos: 0 })
    }
}

fn parse_index(index: &[u8], data_end: u64) -> Result<(String, Entries), String> {
    let mut r = Cursor { bytes: index, at: 0 };
    let main = r.string()?;
    let count = r.u32()? as usize;
    if count > MAX_ENTRIES {
        return Err(format!("it claims {count} files, more than the {MAX_ENTRIES} allowed"));
    }
    // An entry takes at least 20 bytes of index: a tiny file claiming a million must not reserve for them.
    let mut entries = HashMap::with_capacity(count.min((index.len() - r.at) / 20));
    for _ in 0..count {
        let path = r.string()?;
        let (offset, len) = (r.u64()?, r.u64()?);
        if !is_inside(&path) {
            return Err(format!("`{path}` is not a path inside the game folder"));
        }
        if offset < HEADER_LEN || offset.checked_add(len).is_none_or(|end| end > data_end) {
            return Err(format!("`{path}` lies outside the file"));
        }
        if entries.insert(path.clone(), (offset, len)).is_some() {
            return Err(format!("`{path}` is listed twice"));
        }
    }
    if r.at != index.len() {
        return Err("its index has extra bytes at the end".into());
    }
    if !entries.contains_key(&main) {
        return Err(format!("the main script `{main}` is not inside"));
    }
    Ok((main, entries))
}

/// Reads the index's numbers and strings, failing (not panicking) when it ends early.
struct Cursor<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl Cursor<'_> {
    fn take(&mut self, n: usize) -> Result<&[u8], String> {
        let end = self.at.checked_add(n).filter(|&e| e <= self.bytes.len()).ok_or("its index is cut short")?;
        let out = &self.bytes[self.at..end];
        self.at = end;
        Ok(out)
    }

    fn u32(&mut self) -> Result<u32, String> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }

    fn u64(&mut self) -> Result<u64, String> {
        Ok(u64::from_le_bytes(self.take(8)?.try_into().unwrap()))
    }

    fn string(&mut self) -> Result<String, String> {
        let n = self.u32()? as usize;
        if n > MAX_PATH_BYTES {
            return Err("it has a path that is far too long".into());
        }
        String::from_utf8(self.take(n)?.to_vec()).map_err(|_| "it has a path that is not valid UTF-8".to_string())
    }
}

/// One entry of an archive as a `Read + Seek` stream. Every `Slice` has its own position and reads with
/// positioned reads, so slices of one archive never disturb each other (any thread, any number).
#[derive(Debug)]
pub struct Slice {
    file: Arc<File>,
    start: u64,
    len: u64,
    pos: u64,
}

impl Slice {
    /// The entry's length in bytes.
    pub fn len(&self) -> u64 {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }
}

#[cfg(unix)]
fn read_at(file: &File, buf: &mut [u8], offset: u64) -> io::Result<usize> {
    std::os::unix::fs::FileExt::read_at(file, buf, offset)
}

#[cfg(windows)]
fn read_at(file: &File, buf: &mut [u8], offset: u64) -> io::Result<usize> {
    std::os::windows::fs::FileExt::seek_read(file, buf, offset)
}

impl Read for Slice {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let n = buf.len().min(self.len.saturating_sub(self.pos).min(usize::MAX as u64) as usize);
        if n == 0 {
            return Ok(0);
        }
        let got = read_at(&self.file, &mut buf[..n], self.start + self.pos)?;
        self.pos += got as u64;
        Ok(got)
    }
}

impl Seek for Slice {
    /// Seeking past the end is allowed (reads there return nothing); before the start is an error.
    fn seek(&mut self, to: SeekFrom) -> io::Result<u64> {
        let target = match to {
            SeekFrom::Start(n) => i128::from(n),
            SeekFrom::End(d) => i128::from(self.len) + i128::from(d),
            SeekFrom::Current(d) => i128::from(self.pos) + i128::from(d),
        };
        self.pos = u64::try_from(target)
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "seek to before the start of the file"))?;
        Ok(self.pos)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_index_claiming_a_million_files_with_none_is_cut_short_without_reserving_for_them() {
        let mut index = 1u32.to_le_bytes().to_vec(); // main: one byte
        index.push(b'm');
        index.extend((MAX_ENTRIES as u32).to_le_bytes());
        let err = parse_index(&index, 1000).unwrap_err();
        assert!(err.contains("cut short"), "{err}");
    }

        fn dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("qs-archive-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// An archive of `files` on disk, `main` first.
    fn make(name: &str, files: &[(&str, &[u8])]) -> PathBuf {
        let d = dir(name);
        let mut list = Vec::new();
        for (p, bytes) in files {
            let from = d.join(p.replace('/', "_"));
            std::fs::write(&from, bytes).unwrap();
            list.push((p.to_string(), from));
        }
        let out = d.join("game.qsa");
        write(&out, files[0].0, &list).unwrap();
        out
    }

    fn read_all(mut s: Slice) -> Vec<u8> {
        let mut v = Vec::new();
        s.read_to_end(&mut v).unwrap();
        v
    }

    #[test]
    fn a_failed_write_leaves_no_file_and_nothing_the_reader_refuses_is_kept() {
        let d = dir("failed");
        std::fs::write(d.join("m"), "x").unwrap();
        let out = d.join("game.qsa");
        let long = "a".repeat(MAX_PATH_BYTES + 1); // written fine, refused when read back
        let err = write(&out, "m.txt", &[("m.txt".into(), d.join("m")), (long, d.join("m"))]).unwrap_err();
        assert!(err.contains("far too long"), "{err}");
        let err = write(&out, "m.txt", &[("m.txt".into(), d.join("gone"))]).unwrap_err();
        assert!(err.contains("cannot read"), "{err}");
        assert_eq!(std::fs::read_dir(&d).unwrap().count(), 1, "only the source file is left");
    }

    #[test]
    fn round_trip_with_aligned_entries() {
        let big = vec![7u8; 10_000];
        let path = make("round", &[("main.quill.md", b"Hi.\n"), ("assets/a.png", &big), ("empty.txt", b"")]);
        let a = Archive::open(&path).unwrap();
        assert_eq!(a.main(), "main.quill.md");
        assert_eq!(read_all(a.slice("main.quill.md").unwrap()), b"Hi.\n");
        assert_eq!(read_all(a.slice("assets/a.png").unwrap()), big);
        assert!(a.slice("empty.txt").unwrap().is_empty());
        assert!(a.slice("nope").is_none() && !a.contains("nope"));
        assert_eq!(a.paths().count(), 3);
        for p in a.paths() {
            assert_eq!(a.range(p).unwrap().0 % ALIGN, 0, "{p}");
        }
    }

    #[test]
    fn a_path_in_the_wrong_case_finds_its_entry_unless_two_entries_differ_only_in_case() {
        let path = make("case", &[("main.quill.md", b"Hi.\n"), ("assets/Happy.png", b"h"), ("a/X.png", b"1"), ("a/x.png", b"2")]);
        let a = Archive::open(&path).unwrap();
        assert_eq!(read_all(a.slice("assets/happy.png").unwrap()), b"h");
        assert!(a.contains("ASSETS/HAPPY.PNG"));
        assert_eq!(read_all(a.slice("a/x.png").unwrap()), b"2", "an exact name always wins");
        assert!(a.slice("a/X.PNG").is_none(), "ambiguous: no guess");
    }

    /// Open a copy of `path` after `edit` changed its bytes.
    fn broken(path: &Path, edit: impl FnOnce(&mut Vec<u8>)) -> String {
        let mut bytes = std::fs::read(path).unwrap();
        edit(&mut bytes);
        let bad = path.with_file_name("bad.qsa");
        std::fs::write(&bad, bytes).unwrap();
        Archive::open(&bad).unwrap_err()
    }

    #[test]
    fn damaged_archives_are_refused_with_a_reason() {
        let path = make("bad", &[("main.quill.md", b"Hi.\n"), ("a.png", b"png")]);
        assert!(broken(&path, |b| b[0] = b'X').contains("marker"));
        assert!(broken(&path, |b| b[8] = 9).contains("version 9"));
        assert!(broken(&path, |b| b.truncate(20)).contains("too short"));
        assert!(broken(&path, |b| b.truncate(b.len() - 3)).contains("outside the file"), "cut index");
        assert!(broken(&path, |b| b[24..32].copy_from_slice(&(MAX_INDEX_BYTES + 1).to_le_bytes())).contains("too large"));
        assert!(broken(&path, |b| b[16..24].copy_from_slice(&u64::MAX.to_le_bytes())).contains("outside the file"));
        // an index that ends inside a field (the header says it is shorter than it is)
        assert!(broken(&path, |b| {
            let n = u64::from_le_bytes(b[24..32].try_into().unwrap()) - 5;
            b[24..32].copy_from_slice(&n.to_le_bytes());
        })
        .contains("cut short"));
    }

    /// An archive whose index is `index` (after the 4096 bytes of "data").
    fn with_index(name: &str, index: &[u8]) -> String {
        let mut b = Vec::new();
        b.extend(MAGIC);
        b.extend(VERSION.to_le_bytes());
        b.extend(0u32.to_le_bytes());
        b.extend(4096u64.to_le_bytes());
        b.extend((index.len() as u64).to_le_bytes());
        b.resize(4096, 0);
        b.extend(index);
        let path = dir(name).join("x.qsa");
        std::fs::write(&path, b).unwrap();
        Archive::open(&path).unwrap_err()
    }

    fn index(main: &str, entries: &[(&str, u64, u64)]) -> Vec<u8> {
        let mut v = Vec::new();
        put_str(&mut v, main);
        v.extend((entries.len() as u32).to_le_bytes());
        for (p, o, l) in entries {
            put_str(&mut v, p);
            v.extend(o.to_le_bytes());
            v.extend(l.to_le_bytes());
        }
        v
    }

    #[test]
    fn bad_entries_are_refused() {
        let ok = ("main.quill.md", 40, 4);
        assert!(with_index("outside", &index("main.quill.md", &[("main.quill.md", 4000, 200)])).contains("outside the file"));
        assert!(with_index("wrap", &index("main.quill.md", &[("main.quill.md", 40, u64::MAX)])).contains("outside the file"));
        assert!(with_index("header", &index("main.quill.md", &[("main.quill.md", 4, 4)])).contains("outside the file"));
        assert!(with_index("dots", &index("main.quill.md", &[ok, ("../evil.txt", 40, 1)])).contains("not a path inside"));
        assert!(with_index("abs", &index("main.quill.md", &[ok, ("/etc/passwd", 40, 1)])).contains("not a path inside"));
        assert!(with_index("drive", &index("main.quill.md", &[ok, ("C:x.png", 40, 1)])).contains("not a path inside"));
        assert!(with_index("dup", &index("main.quill.md", &[ok, ok])).contains("listed twice"));
        assert!(with_index("nomain", &index("story.quill.md", &[ok])).contains("main script"));
        let mut utf = index("main.quill.md", &[ok]);
        let at = utf.windows(4).position(|w| w == b"main").unwrap();
        utf[at] = 0xFF;
        assert!(with_index("utf8", &utf).contains("UTF-8"));
        let mut many = index("main.quill.md", &[ok]);
        many[17..21].copy_from_slice(&u32::MAX.to_le_bytes());
        assert!(with_index("many", &many).contains("more than"));
        let mut trailing = index("main.quill.md", &[ok]);
        trailing.push(0);
        assert!(with_index("trail", &trailing).contains("extra bytes"));
    }

    #[test]
    fn the_writer_refuses_bad_input() {
        let d = dir("writer");
        let f = d.join("f");
        std::fs::write(&f, "x").unwrap();
        let out = d.join("o.qsa");
        let one = |p: &str| (p.to_string(), f.clone());
        assert!(write(&out, "a", &[one("a"), one("a")]).unwrap_err().contains("twice"));
        assert!(write(&out, "a", &[one("a"), one("../b")]).unwrap_err().contains("inside"));
        assert!(write(&out, "b", &[one("a")]).unwrap_err().contains("main script"));
    }

    #[test]
    fn a_slice_reads_and_seeks_like_a_file() {
        let data: Vec<u8> = (0..=255u8).cycle().take(5000).collect();
        let path = make("slice", &[("main.quill.md", b"m"), ("d.bin", &data)]);
        let a = Archive::open(&path).unwrap();
        let mut s = a.slice("d.bin").unwrap();
        let mut b = [0u8; 4];
        assert_eq!(s.seek(SeekFrom::Start(254)).unwrap(), 254);
        s.read_exact(&mut b).unwrap();
        assert_eq!(b, [254, 255, 0, 1]);
        assert_eq!(s.seek(SeekFrom::Current(-2)).unwrap(), 256);
        assert_eq!(s.seek(SeekFrom::End(-1)).unwrap(), 4999);
        assert_eq!(s.read(&mut b).unwrap(), 1, "stops at the entry's end, not the file's");
        assert_eq!(s.read(&mut b).unwrap(), 0, "at the end");
        assert_eq!(s.seek(SeekFrom::Start(9999)).unwrap(), 9999);
        assert_eq!(s.read(&mut b).unwrap(), 0, "past the end");
        assert!(s.seek(SeekFrom::Current(-10_000_000)).is_err());
        assert_eq!(s.seek(SeekFrom::Start(0)).unwrap(), 0, "a refused seek moves nothing");
    }

    #[test]
    fn slices_read_independently_on_many_threads() {
        let one: Vec<u8> = (0..30_000u32).map(|i| (i % 251) as u8).collect();
        let two: Vec<u8> = (0..30_000u32).map(|i| (i % 241) as u8).collect();
        let path = make("threads", &[("main.quill.md", b"m"), ("one", &one), ("two", &two)]);
        let a = Arc::new(Archive::open(&path).unwrap());
        let threads: Vec<_> = (0..8)
            .map(|i| {
                let a = a.clone();
                let (name, want) = if i % 2 == 0 { ("one", one.clone()) } else { ("two", two.clone()) };
                std::thread::spawn(move || {
                    let mut s = a.slice(name).unwrap();
                    let mut got: Vec<u8> = Vec::new();
                    let mut chunk = [0u8; 777];
                    loop {
                        let n = s.read(&mut chunk).unwrap();
                        if n == 0 {
                            break;
                        }
                        got.extend(&chunk[..n]);
                    }
                    assert!(got == want);
                })
            })
            .collect();
        for t in threads {
            t.join().unwrap();
        }
    }
}
