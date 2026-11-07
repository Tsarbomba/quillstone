//! Save slots: where saves live, what a save file looks like, and thumbnails.
//!
//! Files are reached through [`Storage`], so the game logic never touches the
//! disk itself (the player stores them in its per-user folder; tests use
//! [`MemoryStorage`]). A save file is a short header followed by the runtime's
//! session text:
//!
//! ```text
//! quillstone-save 1
//! seq 1761430200000
//! time "2026-10-25 22:10"
//! preview "Mika: You came after all."
//! quillstone-session 2
//! ...
//! ```

use std::collections::HashMap;

use quillstone_render::{clip, Draw, Rect, Size, View, VIRTUAL};
use quillstone_runtime::{quote, tokens, Session, State, MAX_SAVE_BYTES};
use quillstone_script::Program;

/// The current time, as the frontend knows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Timestamp {
    /// Milliseconds since 1970; orders saves (newest wins).
    pub millis: u64,
    /// Local date and time to show on the slot, e.g. "2026-10-25 22:10".
    pub text: String,
}

/// Where save files are kept.
pub trait Storage {
    /// A file's contents: `Ok(None)` if it doesn't exist, `Err` if it does but can't be read
    /// (locked, an IO error, over the size limit, not UTF-8).
    fn load(&self, name: &str) -> Result<Option<String>, String>;
    fn write(&mut self, name: &str, contents: &str) -> Result<(), String>;
    fn now(&self) -> Timestamp;

    /// A file's contents if it exists and can be read (missing and unreadable look the same).
    fn read(&self, name: &str) -> Option<String> {
        self.load(name).ok().flatten()
    }
}

/// Keeps files in memory. For tests, and for running without a disk.
#[derive(Debug, Clone, Default)]
pub struct MemoryStorage {
    pub files: HashMap<String, String>,
    /// Files that exist but can't be read (for tests): `load` fails for them.
    pub unreadable: std::collections::HashSet<String>,
    /// What [`Storage::now`] returns; each call moves it on by a second.
    pub clock: std::cell::Cell<u64>,
}

impl Storage for MemoryStorage {
    fn load(&self, name: &str) -> Result<Option<String>, String> {
        if self.unreadable.contains(name) {
            return Err("unreadable".to_string());
        }
        Ok(self.files.get(name).cloned())
    }

    fn write(&mut self, name: &str, contents: &str) -> Result<(), String> {
        self.files.insert(name.to_string(), contents.to_string());
        Ok(())
    }

    fn now(&self) -> Timestamp {
        let t = self.clock.get() + 1000;
        self.clock.set(t);
        Timestamp { millis: t, text: format!("t{}", t / 1000) }
    }
}

/// A page of slots on the save/load screens.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Page {
    /// Written by the game at checkpoints (choices, quitting); load-only.
    Auto,
    /// Written by quick save; load-only on the slot screens.
    Quick,
    /// Pages the player saves to, numbered from 1.
    Numbered(u8),
}

pub const SLOTS_PER_PAGE: usize = 6;
pub const NUMBERED_PAGES: u8 = 9;

/// Every page, in tab order.
pub fn pages() -> Vec<Page> {
    let mut out = vec![Page::Auto, Page::Quick];
    out.extend((1..=NUMBERED_PAGES).map(Page::Numbered));
    out
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Slot {
    pub page: Page,
    /// 0-based position on the page.
    pub index: usize,
}

impl Slot {
    pub fn file_name(&self) -> String {
        let n = self.index + 1;
        match self.page {
            Page::Auto => format!("auto-{n}.save"),
            Page::Quick => format!("quick-{n}.save"),
            Page::Numbered(p) => format!("{p}-{n}.save"),
        }
    }
}

const MAGIC: &str = "quillstone-save 1";

/// A save file's header plus its session text.
#[derive(Debug, Clone, PartialEq)]
pub struct SaveFile {
    pub seq: u64,
    pub time: String,
    pub preview: String,
    pub session: String,
}

impl SaveFile {
    pub fn new(session: &Session, now: Timestamp) -> SaveFile {
        SaveFile { seq: now.millis, time: now.text, preview: preview(session.state()), session: session.save() }
    }

    pub fn to_text(&self) -> String {
        format!(
            "{MAGIC}\nseq {}\ntime {}\npreview {}\n{}",
            self.seq,
            quote(&self.time),
            quote(&self.preview),
            self.session
        )
    }

    /// `None` for a file that isn't a save, is damaged, or is larger than
    /// [`MAX_SAVE_BYTES`] (never parsed: a hostile file costs a multiple of its size).
    pub fn parse(text: &str) -> Option<SaveFile> {
        if text.len() > MAX_SAVE_BYTES {
            return None;
        }
        let mut lines = text.lines();
        if lines.next()?.trim() != MAGIC {
            return None;
        }
        let mut file = SaveFile { seq: 0, time: String::new(), preview: String::new(), session: String::new() };
        let mut rest = text;
        for line in lines {
            let (key, value) = line.split_once(' ').unwrap_or((line, ""));
            match key {
                "seq" => file.seq = value.trim().parse().ok()?,
                "time" => file.time = unquote(value)?,
                "preview" => file.preview = unquote(value)?,
                _ => {
                    // The session starts here: everything from this line on.
                    let at = text.find(line)?;
                    rest = &text[at..];
                    break;
                }
            }
        }
        file.session = rest.to_string();
        Some(file)
    }
}

/// What the slot shows as a reminder: who was talking and what they said.
pub fn preview(state: &State) -> String {
    let Some(line) = &state.line else { return String::new() };
    let words = quillstone_script::markup::plain(&line.text).replace('\n', " "); // one line: `{p}` is a space
    let text = match &line.speaker {
        Some(name) => format!("{name}: {words}"),
        None => words,
    };
    const MAX: usize = 70;
    if text.chars().count() <= MAX {
        text
    } else {
        format!("{}…", text.chars().take(MAX - 1).collect::<String>().trim_end())
    }
}

/// One quoted string, as [`quote`] writes it.
fn unquote(s: &str) -> Option<String> {
    let mut words = tokens(s).ok()?;
    (words.len() == 1).then(|| words.remove(0))
}

/// A filled slot as the save/load screens show it.
#[derive(Debug, Clone)]
pub struct SlotInfo {
    pub seq: u64,
    pub time: String,
    pub preview: String,
    /// The saved scene, for the thumbnail; `None` if the save can't be read
    /// (damaged, or made for a different script).
    pub scene: Option<View>,
}

/// Read a slot for display. `None` if it is empty.
pub fn read_slot(storage: &dyn Storage, program: &Program, slot: Slot) -> Option<SlotInfo> {
    let text = storage.read(&slot.file_name())?;
    let Some(file) = SaveFile::parse(&text) else {
        return Some(SlotInfo { seq: 0, time: String::new(), preview: "Unreadable save".into(), scene: None });
    };
    let scene = Session::load(program, &file.session).ok().map(|(_, cmds)| {
        let mut view = View::new().with_speed(0.0).with_anims(&program.anims).with_art_scale(program.art_scale);
        view.apply(&cmds);
        view
    });
    let preview = if scene.is_some() { file.preview } else { "This save doesn't fit the current script".into() };
    Some(SlotInfo { seq: file.seq, time: file.time, preview, scene })
}

/// The slot on `page` to write next: an empty one, else the oldest. (Auto and
/// quick saves rotate through their page.)
pub fn next_rotating_slot(storage: &dyn Storage, page: Page) -> Slot {
    let seq = |i: usize| {
        let slot = Slot { page, index: i };
        storage.read(&slot.file_name()).map(|t| SaveFile::parse(&t).map_or(0, |f| f.seq))
    };
    let index = (0..SLOTS_PER_PAGE)
        .min_by_key(|&i| match seq(i) {
            None => (0, 0), // empty slots first
            Some(s) => (1, s),
        })
        .unwrap_or(0);
    Slot { page, index }
}

/// The newest save on `page`, if any.
pub fn newest_slot(storage: &dyn Storage, page: Page) -> Option<Slot> {
    (0..SLOTS_PER_PAGE)
        .filter_map(|i| {
            let slot = Slot { page, index: i };
            let file = SaveFile::parse(&storage.read(&slot.file_name())?)?;
            Some((file.seq, slot))
        })
        .max_by_key(|(seq, _)| *seq)
        .map(|(_, slot)| slot)
}

/// The newest save on any page and its `seq` (for the main menu's Continue).
pub fn newest_save(storage: &dyn Storage) -> Option<(u64, Slot)> {
    pages()
        .into_iter()
        .flat_map(|page| (0..SLOTS_PER_PAGE).map(move |index| Slot { page, index }))
        .filter_map(|slot| Some((SaveFile::parse(&storage.read(&slot.file_name())?)?.seq, slot)))
        .max_by_key(|(seq, _)| *seq)
}

/// The pictures of a saved scene (background, sprites, inline images), scaled
/// into `into` and clipped to it. Text is left out: it would be unreadable.
pub fn thumbnail(scene: &View, image_size: &dyn Fn(&str) -> Option<Size>, into: Rect) -> Vec<Draw> {
    let scale = into.w / VIRTUAL.w;
    let mut out = vec![Draw::Rect { rect: into, color: [0.0, 0.0, 0.0, 1.0] }];
    for draw in scene.draw_list(image_size, None) {
        if let Draw::Image { src, dest, uv, alpha, shade, blur, additive } = draw {
            let dest = Rect::new(into.x + dest.x * scale, into.y + dest.y * scale, dest.w * scale, dest.h * scale);
            if let Some((dest, uv)) = clip(dest, uv, into) {
                out.push(Draw::Image { src, dest, uv, alpha, shade, blur: blur * scale, additive });
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use quillstone_runtime::Input;
    use quillstone_script::parse;

    const STORY: &str = "::bg{src=bg.png}\n**Mika:** Hello \"there\".\n**Mika:** Second.\n";

    #[test]
    fn file_names() {
        assert_eq!(Slot { page: Page::Auto, index: 0 }.file_name(), "auto-1.save");
        assert_eq!(Slot { page: Page::Quick, index: 5 }.file_name(), "quick-6.save");
        assert_eq!(Slot { page: Page::Numbered(3), index: 1 }.file_name(), "3-2.save");
        assert_eq!(pages().len(), 2 + NUMBERED_PAGES as usize);
    }

    #[test]
    fn save_files_round_trip() {
        let p = parse(STORY).unwrap();
        let (session, _) = Session::start(&p);
        let file = SaveFile::new(&session, Timestamp { millis: 42, text: "today \"noon\"".into() });
        assert_eq!(file.preview, "Mika: Hello \"there\".");
        let back = SaveFile::parse(&file.to_text()).unwrap();
        assert_eq!(back, file);
        assert!(Session::load(&p, &back.session).is_ok());
        assert!(SaveFile::parse("nonsense").is_none());
        assert!(SaveFile::parse("quillstone-save 1\nseq x\n").is_none());
    }

    #[test]
    fn hostile_slot_files_show_as_unreadable_without_crashing() {
        let p = parse("---\nvars: { gold: 2 }\n---\n# a\nHi.\n::hud{id=g text=\"Gold {gold}\"}\nBye.\n").unwrap();
        let (session, _) = Session::start(&p);
        let good = SaveFile::new(&session, Timestamp { millis: 42, text: "now".into() });
        let slot = Slot { page: Page::Numbered(1), index: 0 };
        let mut storage = MemoryStorage::default();
        // a file over the size limit: unreadable, and never parsed
        let big = format!("{}{}", good.to_text(), "\n".repeat(MAX_SAVE_BYTES));
        storage.write(&slot.file_name(), &big).unwrap();
        assert!(SaveFile::parse(&big).is_none());
        let info = read_slot(&storage, &p, slot).unwrap();
        assert_eq!((info.preview.as_str(), info.seq, info.scene.is_none()), ("Unreadable save", 0, true));
        // a save whose hud expression nests 990 deep, read on a small stack (the Load screen
        // reads every slot of a page)
        let deep = format!("{}1{}", "(".repeat(990), ")".repeat(990));
        let evil = good.to_text().replace("rng ", &format!("hud text \"a\" 0 0 left \"{deep}\" \"x\" - - - 0\nrng "));
        // and one whose hud records add up to more text than a state may hold
        let text = "x".repeat(1500);
        let lots: String = (0..100).map(|i| format!("hud text \"e{i}\" 0 0 left \"\" \"{text}\" - - - 0\n")).collect();
        let heavy = good.to_text().replace("rng ", &format!("{lots}rng "));
        for (n, file) in [evil, heavy].into_iter().enumerate() {
            let mut storage = MemoryStorage::default();
            storage.write(&slot.file_name(), &file).unwrap();
            let info = std::thread::Builder::new()
                .stack_size(512 * 1024)
                .spawn(move || {
                    let p = parse("---\nvars: { gold: 2 }\n---\n# a\nHi.\n::hud{id=g text=\"Gold {gold}\"}\nBye.\n").unwrap();
                    read_slot(&storage, &p, slot).unwrap().preview
                })
                .unwrap()
                .join()
                .expect("no stack overflow");
            assert_eq!(info, "This save doesn't fit the current script", "file {n}");
        }
    }

    #[test]
    fn a_paragraph_break_is_a_space_in_a_one_line_preview() {
        let p = parse("**Mika:** One.{p}Two.\n").unwrap();
        let (session, _) = Session::start(&p);
        assert_eq!(preview(session.state()), "Mika: One. Two.");
    }

    #[test]
    fn long_previews_are_shortened() {
        let src = format!("**A:** {}\n", "word ".repeat(40));
        let p = parse(&src).unwrap();
        let (session, _) = Session::start(&p);
        let text = preview(session.state());
        assert!(text.chars().count() <= 70 && text.ends_with('…'));
    }

    #[test]
    fn reading_slots_for_display() {
        let p = parse(STORY).unwrap();
        let mut storage = MemoryStorage::default();
        let slot = Slot { page: Page::Numbered(1), index: 0 };
        assert!(read_slot(&storage, &p, slot).is_none());

        let (mut session, _) = Session::start(&p);
        session.input(Input::Advance);
        storage.write(&slot.file_name(), &SaveFile::new(&session, storage.now()).to_text()).unwrap();
        let info = read_slot(&storage, &p, slot).unwrap();
        assert_eq!(info.preview, "Mika: Second.");
        assert_eq!(info.scene.unwrap().background.unwrap().src, "bg.png");

        storage.write(&slot.file_name(), "garbage").unwrap();
        assert!(read_slot(&storage, &p, slot).unwrap().scene.is_none());
        // An old save (no fallback position) whose label the script no longer has can't be placed.
        let labelled = parse("# chapter-two\n**Mika:** Hi.\n").unwrap();
        let (session, _) = Session::start(&labelled);
        let text = SaveFile::new(&session, storage.now()).to_text();
        let old: Vec<String> = text
            .lines()
            .map(|l| if l.starts_with("at ") { l.split(' ').take(3).collect::<Vec<_>>().join(" ") } else { l.to_string() })
            .collect();
        storage.write(&slot.file_name(), &old.join("\n")).unwrap();
        let info = read_slot(&storage, &p, slot).unwrap();
        assert!(info.scene.is_none());
        assert_eq!(info.preview, "This save doesn't fit the current script");
    }

    #[test]
    fn a_slot_thumbnail_shows_the_first_frame_of_an_animation() {
        let p = parse("::anim{id=sky frames=\"day.png 1, night.png 1\" loop}\n::bg{anim=sky}\n**A:** hi\n").unwrap();
        let mut storage = MemoryStorage::default();
        let slot = Slot { page: Page::Numbered(1), index: 0 };
        let (session, _) = Session::start(&p);
        storage.write(&slot.file_name(), &SaveFile::new(&session, storage.now()).to_text()).unwrap();
        let scene = read_slot(&storage, &p, slot).unwrap().scene.unwrap();
        let draws = thumbnail(&scene, &|_| Some(Size::new(1920.0, 1080.0)), Rect::new(0.0, 0.0, 192.0, 108.0));
        let files: Vec<&str> = draws.iter().filter_map(|d| if let Draw::Image { src, .. } = d { Some(src.as_str()) } else { None }).collect();
        assert_eq!(files, ["day.png"]);
    }

    #[test]
    fn rotating_slots_fill_up_then_replace_the_oldest() {
        let p = parse(STORY).unwrap();
        let (session, _) = Session::start(&p);
        let mut storage = MemoryStorage::default();
        let mut used = Vec::new();
        for _ in 0..SLOTS_PER_PAGE + 2 {
            let slot = next_rotating_slot(&storage, Page::Quick);
            used.push(slot.index);
            storage.write(&slot.file_name(), &SaveFile::new(&session, storage.now()).to_text()).unwrap();
            assert_eq!(newest_slot(&storage, Page::Quick), Some(slot));
        }
        assert_eq!(used, vec![0, 1, 2, 3, 4, 5, 0, 1]);
        assert_eq!(newest_slot(&storage, Page::Auto), None);
    }

    #[test]
    fn thumbnails_are_scaled_pictures_without_text() {
        let p = parse("::bg{src=bg.png}\n::show{src=s.png id=s x=12%}\n::vignette{strength=0.9}\n**A:** hi\n").unwrap();
        let (_, cmds) = Session::start(&p);
        let mut view = View::new().with_speed(0.0);
        view.apply(&cmds);
        let into = Rect::new(100.0, 100.0, 192.0, 108.0);
        let draws = thumbnail(&view, &|_| Some(Size::new(1000.0, 1000.0)), into);
        assert!(draws.iter().all(|d| !matches!(d, Draw::Text { .. })));
        assert_eq!(draws.len(), 3); // backdrop, background, sprite: no vignette, as with a tint
        for d in &draws {
            if let Draw::Image { dest, .. } = d {
                assert!(dest.x >= into.x && dest.x + dest.w <= into.x + into.w + 1e-3);
                assert!(dest.y >= into.y && dest.y + dest.h <= into.y + into.h + 1e-3);
            }
        }
    }

    #[test]
    fn a_thumbnail_blurs_in_proportion_to_its_size() {
        let p = parse("::bg{src=bg.png blur=20}\n**A:** hi\n").unwrap();
        let (_, cmds) = Session::start(&p);
        let mut view = View::new().with_speed(0.0);
        view.apply(&cmds);
        let draws = thumbnail(&view, &|_| Some(Size::new(1920.0, 1080.0)), Rect::new(0.0, 0.0, 192.0, 108.0));
        let blur = draws.iter().find_map(|d| if let Draw::Image { blur, .. } = d { Some(*blur) } else { None }).unwrap();
        assert!((blur - 2.0).abs() < 1e-3, "{blur}");
    }
}
