//! Translation files: `lang/<code>.txt`, one per language besides the script's own.
//!
//! The format is plain lines, each starting with a marker, so any text (quotes, braces,
//! backslashes, markup) is written exactly as it is in the script, with no escaping:
//!
//! ```text
//! # a comment for the translator (lines starting `#:` say where the text is used)
//! @ Mika                    <- optional context: the speaker, for words that need two translations
//! > Hello, {name}!          <- the script's text, exactly (several `>` lines = several lines)
//! = Hej, {name}!            <- the translation (empty `=` = not translated yet)
//! ```
//!
//! This module does no I/O: callers read the file (at most [`MAX_FILE_BYTES`]) and pass the text.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fmt;

use crate::ops::{HudKind, Op, Program};

/// Larger files are refused (translation files are untrusted text).
pub const MAX_FILE_BYTES: usize = 8 << 20;
/// More entries than this are refused.
pub const MAX_ENTRIES: usize = 100_000;
/// The longest source or translation, in bytes.
pub const MAX_TEXT_BYTES: usize = 16 << 10;
const MAX_CONTEXT_BYTES: usize = 256;
const MAX_ERRORS: usize = 50;

/// `lang/<code>.txt`, the game-folder path of a language's translation file.
pub fn language_file(code: &str) -> String {
    format!("lang/{code}.txt")
}

/// Why `code` can't be a language code: a short ASCII name that is also a safe file name.
pub(crate) fn bad_code(code: &str) -> Option<&'static str> {
    if code.is_empty() || code.len() > 16 {
        Some("1 to 16 characters")
    } else if !code.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_')) {
        Some("letters, digits, `-` and `_` only")
    } else {
        crate::unsafe_reason(code).map(|_| "not usable as a file name")
    }
}

/// A problem in a translation file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TranslationError {
    /// 1-based; 0 for the whole file.
    pub line: usize,
    pub message: String,
}

impl fmt::Display for TranslationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.line {
            0 => write!(f, "{}", self.message),
            n => write!(f, "line {n}: {}", self.message),
        }
    }
}

/// One translated text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    /// Only for lines spoken by this character (a display name); `None` applies everywhere.
    pub context: Option<String>,
    pub source: String,
    /// Empty: not translated yet.
    pub target: String,
    /// The script no longer has this text (kept so the work isn't lost).
    pub obsolete: bool,
    /// Line of the `>` in the file (0 for an entry made by export).
    pub line: usize,
    /// The translator's own `#` comments (kept by export; `#:` places are rewritten).
    pub comments: Vec<String>,
}

/// A parsed translation file.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Catalog {
    pub entries: Vec<Entry>,
    index: HashMap<(Option<String>, String), usize>,
}

impl Catalog {
    /// Read a translation file, or list everything wrong with it (with line numbers).
    pub fn parse(text: &str) -> Result<Catalog, Vec<TranslationError>> {
        let mut errors = Vec::new();
        if text.len() > MAX_FILE_BYTES {
            let message = format!("the file is larger than {} MB", MAX_FILE_BYTES >> 20);
            return Err(vec![TranslationError { line: 0, message }]);
        }
        let text = text.strip_prefix('\u{feff}').unwrap_or(text);
        let mut cat = Catalog::default();
        let mut cur = Pending::default();
        let mut err = |line: usize, message: String| {
            if errors.len() < MAX_ERRORS {
                errors.push(TranslationError { line, message });
            }
        };
        for (i, raw) in text.split('\n').enumerate() {
            let n = i + 1;
            let line = raw.strip_suffix('\r').unwrap_or(raw);
            let (marker, rest) = match line.chars().next() {
                None => ('\0', ""),
                Some(c) => (c, &line[c.len_utf8()..]),
            };
            // `> text`: one space after the marker is part of the marker.
            let body = rest.strip_prefix(' ').unwrap_or(rest);
            if cur.in_target && !matches!(marker, '=') {
                cat.finish(&mut cur, &mut err);
            }
            match marker {
                '\0' => {
                    if cur.has_source() && !cur.in_target {
                        err(n, "a `>` text must be followed by its `=` translation".into());
                        cur = Pending::default();
                    } else if !cur.in_target {
                        cur.flush_header_blank();
                    }
                }
                '#' => {
                    if cur.has_source() {
                        err(n, "a `>` text must be followed by its `=` translation (a comment can't come between)".into());
                        cur = Pending::default();
                    }
                    if !rest.starts_with(':') {
                        cur.comments.push(rest.strip_prefix(' ').unwrap_or(rest).to_string());
                    }
                }
                '@' => {
                    if cur.has_source() || cur.context.is_some() {
                        err(n, "`@` (context) comes once, before the `>` text".into());
                    } else if body.is_empty() || body.len() > MAX_CONTEXT_BYTES {
                        err(n, format!("`@` needs a context of 1 to {MAX_CONTEXT_BYTES} bytes (the speaker's name)"));
                    } else {
                        cur.context = Some(body.to_string());
                    }
                }
                '!' => {
                    if cur.has_source() || cur.obsolete {
                        err(n, "`!` (a flag) comes once, before the `>` text".into());
                    } else if body.trim() == "obsolete" {
                        cur.obsolete = true;
                    } else {
                        err(n, format!("unknown flag `{}` (only `! obsolete`)", body.trim()));
                    }
                }
                '>' => {
                    if cur.source.is_empty() {
                        cur.line = n;
                    }
                    if cur.source.len() + body.len() > MAX_TEXT_BYTES {
                        err(n, format!("the text is longer than {MAX_TEXT_BYTES} bytes"));
                    }
                    cur.source.push(body.to_string());
                }
                '=' => {
                    if !cur.has_source() {
                        err(n, "`=` (a translation) must follow a `>` text".into());
                    } else {
                        cur.in_target = true;
                        if cur.target.iter().map(String::len).sum::<usize>() + body.len() > MAX_TEXT_BYTES {
                            err(n, format!("the translation is longer than {MAX_TEXT_BYTES} bytes"));
                        }
                        cur.target.push(body.to_string());
                    }
                }
                _ => err(n, "a line must start with `#`, `@`, `!`, `>` or `=` (or be empty)".into()),
            }
            if cat.entries.len() > MAX_ENTRIES {
                err(n, format!("more than {MAX_ENTRIES} entries"));
                break;
            }
        }
        if cur.has_source() && !cur.in_target {
            err(text.lines().count().max(1), "the last `>` text has no `=` translation".into());
        } else {
            cat.finish(&mut cur, &mut err);
        }
        if errors.is_empty() {
            Ok(cat)
        } else {
            Err(errors)
        }
    }

    fn finish(&mut self, cur: &mut Pending, err: &mut impl FnMut(usize, String)) {
        let p = std::mem::take(cur);
        if !p.has_source() {
            return;
        }
        let source = p.source.join("\n");
        if source.is_empty() {
            return err(p.line, "the `>` text is empty".into());
        }
        let key = (p.context.clone(), source.clone());
        if let Some(&first) = self.index.get(&key) {
            let first = self.entries[first].line;
            return err(p.line, format!("this text is already translated at line {first} (same context)"));
        }
        self.index.insert(key, self.entries.len());
        self.entries.push(Entry {
            context: p.context,
            source,
            target: p.target.join("\n"),
            obsolete: p.obsolete,
            line: p.line,
            comments: p.comments,
        });
    }

    /// The translation of `source` (said by `speaker`, if a character's line): the entry with that
    /// context first, else the one without. `None` when there is none or it is still empty.
    pub fn get(&self, speaker: Option<&str>, source: &str) -> Option<&str> {
        let find = |ctx: Option<&str>| {
            let key = (ctx.map(str::to_string), source.to_string());
            self.index.get(&key).map(|&i| self.entries[i].target.as_str()).filter(|t| !t.is_empty())
        };
        speaker.and_then(|s| find(Some(s))).or_else(|| find(None))
    }

    /// `get`, or the source itself.
    pub fn text<'a>(&'a self, speaker: Option<&str>, source: &'a str) -> &'a str {
        self.get(speaker, source).unwrap_or(source)
    }

    /// Bring the catalog up to date with the script's `strings` (see [`Program::translatable`]):
    /// new texts are added empty, texts the script still has stay (and lose an obsolete flag),
    /// the rest are flagged obsolete. Entries end up in script order, obsolete ones last.
    pub fn merged(&self, strings: &[Source]) -> Catalog {
        let mut out = Catalog::default();
        let mut used = BTreeSet::new();
        for s in strings {
            let key = (None, s.text.clone());
            let old = self.index.get(&key).map(|&i| &self.entries[i]);
            if let Some(i) = self.index.get(&key) {
                used.insert(*i);
            }
            let mut entry = old.cloned().unwrap_or(Entry {
                context: None,
                source: s.text.clone(),
                target: String::new(),
                obsolete: false,
                line: 0,
                comments: Vec::new(),
            });
            entry.obsolete = false;
            entry.line = 0;
            out.push_unique(entry);
            // The translator's context-specific versions of this text go right after it.
            for (i, e) in self.entries.iter().enumerate().filter(|(_, e)| e.context.is_some() && e.source == s.text) {
                used.insert(i);
                let mut e = e.clone();
                e.obsolete = !s.speakers.contains(e.context.as_deref().unwrap_or(""));
                e.line = 0;
                out.push_unique(e);
            }
        }
        for (i, e) in self.entries.iter().enumerate().filter(|(i, _)| !used.contains(i)) {
            let _ = i;
            let mut e = e.clone();
            e.obsolete = true;
            e.line = 0;
            out.push_unique(e);
        }
        out
    }

    fn push_unique(&mut self, e: Entry) {
        let key = (e.context.clone(), e.source.clone());
        if !self.index.contains_key(&key) {
            self.index.insert(key, self.entries.len());
            self.entries.push(e);
        }
    }

    /// The file text. `places` gives the `#:` comment of a source text (see [`Source::place`]).
    pub fn render(&self, language: &str, places: &BTreeMap<String, String>) -> String {
        let mut out = format!(
            "# Quillstone translation: {language}\n\
             # `>` is the game's text, `=` your translation (leave `=` empty to use the original).\n\
             # Keep {{names}} and {{tags}} as they are. Add `@ Speaker` above a `>` for a speaker-specific version.\n"
        );
        for e in &self.entries {
            out.push('\n');
            for c in &e.comments {
                out.push_str(&if c.is_empty() { "#\n".to_string() } else { format!("# {c}\n") });
            }
            if let (Some(place), None) = (places.get(&e.source), &e.context) {
                out.push_str(&format!("#: {place}\n"));
            }
            if e.obsolete {
                out.push_str("! obsolete\n");
            }
            if let Some(c) = &e.context {
                out.push_str(&format!("@ {c}\n"));
            }
            for l in e.source.split('\n') {
                out.push_str(&if l.is_empty() { ">\n".to_string() } else { format!("> {l}\n") });
            }
            for l in e.target.split('\n') {
                out.push_str(&if l.is_empty() { "=\n".to_string() } else { format!("= {l}\n") });
            }
        }
        out
    }
}

#[derive(Default)]
struct Pending {
    context: Option<String>,
    obsolete: bool,
    comments: Vec<String>,
    source: Vec<String>,
    target: Vec<String>,
    in_target: bool,
    line: usize,
}

impl Pending {
    fn has_source(&self) -> bool {
        !self.source.is_empty()
    }

    /// A blank line between entries ends the translator's comment block (a comment before a blank
    /// line belongs to nothing), unless it is the start of a header.
    fn flush_header_blank(&mut self) {
        if self.context.is_none() && !self.obsolete {
            self.comments.clear();
        }
    }
}

/// A text of the game that can be translated.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Source {
    pub text: String,
    /// Where the first use is: `label:line`, or what it is (`title`, `character`, `item`).
    pub place: String,
    /// The characters who say it (display names), for context entries.
    pub speakers: BTreeSet<String>,
}

impl Program {
    /// Every translatable text, once each, in script order: front matter and declarations first,
    /// then lines, choices, prompts and HUD texts as they come.
    pub fn translatable(&self) -> Vec<Source> {
        let mut out: Vec<Source> = Vec::new();
        let mut seen: HashMap<String, usize> = HashMap::new();
        let mut add = |text: &str, place: String, speaker: Option<&str>| {
            if text.trim().is_empty() {
                return;
            }
            let i = *seen.entry(text.to_string()).or_insert_with(|| {
                out.push(Source { text: text.to_string(), place, speakers: BTreeSet::new() });
                out.len() - 1
            });
            if let Some(s) = speaker {
                out[i].speakers.insert(s.to_string());
            }
        };
        if let Some(t) = &self.title {
            add(t, "title".into(), None);
        }
        for r in &self.replays {
            add(&r.title, format!("replay {}", r.label), None);
        }
        for g in &self.gallery {
            add(g, "gallery".into(), None);
        }
        for m in &self.music_room {
            add(m, "music room".into(), None);
        }
        for c in self.characters.values() {
            add(&c.name, format!("character {}", c.id), None);
        }
        for i in self.items.values() {
            add(&i.name, format!("item {}", i.id), None);
            add(&i.text, format!("item {}", i.id), None);
        }
        let mut labels: Vec<(usize, &str)> = self.labels.iter().map(|(n, &pc)| (pc, n.as_str())).collect();
        labels.sort();
        for (pc, op) in self.ops.iter().enumerate() {
            let label = labels.iter().rev().find(|(at, _)| *at <= pc).map_or("start", |(_, n)| *n);
            let place = format!("{label}:{}", self.locate(self.lines.get(pc).copied().unwrap_or(0)).1);
            match op {
                Op::Say { speaker, text, .. } => {
                    if let Some(s) = speaker {
                        add(s, place.clone(), None);
                    }
                    add(text, place, speaker.as_deref());
                }
                Op::Choice { options, .. } => {
                    for o in options {
                        let place = format!("{label}:{}", self.locate(o.line).1);
                        add(&o.text, place.clone(), None);
                        if let Some(l) = &o.lock {
                            add(&l.reason, place, None); // empty = no reason, skipped
                        }
                    }
                }
                Op::Input { prompt, .. } | Op::Notify { text: prompt, .. } => add(prompt, place, None),
                Op::Hud { elem, .. } => {
                    if let HudKind::Text { text, .. } = &elem.kind {
                        add(text, place, None);
                    }
                }
                _ => {}
            }
        }
        out
    }

    /// The `(speaker, text)` pairs of every dialogue line (to check a context entry is used).
    pub fn spoken(&self) -> BTreeSet<(String, String)> {
        let says = self.ops.iter().filter_map(|op| match op {
            Op::Say { speaker: Some(s), text, .. } => Some((s.clone(), text.clone())),
            _ => None,
        });
        says.collect()
    }
}

/// The `{...}` parts of a text, as written inside the braces (trimmed), skipping `\{` and `\\`
/// escapes; a brace with no `}` after it is a plain brace, as in the markup.
pub fn placeholders(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = text;
    while let Some(pos) = rest.find(['{', '\\']) {
        rest = &rest[pos..];
        if rest.starts_with("\\{") || rest.starts_with("\\\\") {
            rest = &rest[2..];
        } else if rest.starts_with('\\') {
            rest = &rest[1..];
        } else if let Some(end) = rest.find('}') {
            out.push(rest[1..end].trim().to_string());
            rest = &rest[end + 1..];
        } else {
            break;
        }
    }
    out
}

/// The name of a tag (`color` for `color=#fff`), or `None` for a `{variable}`.
fn tag_name(inner: &str) -> Option<&str> {
    let name = inner.split('=').next().unwrap_or(inner).trim();
    matches!(name, "nw" | "p" | "w" | "size" | "/size" | "color" | "/color").then_some(name)
}

/// What is wrong with `target` as a translation of `source`: a `{variable}` dropped, added or
/// misspelt, a text tag (`{nw}`, `{p}`, `{w=..}`, `{size=..}`, `{color=..}`) missing or extra, or
/// markup that does not parse. Tag arguments (a colour, a pause length) may differ.
pub fn check_translation(source: &str, target: &str) -> Result<(), String> {
    if target.is_empty() {
        return Ok(());
    }
    let key = |text: &str| {
        let mut vars: BTreeMap<String, i32> = BTreeMap::new();
        for p in placeholders(text) {
            let k = match tag_name(&p) {
                Some(t) => format!("{{{t}}}"),
                None => format!("{{{p}}}"),
            };
            *vars.entry(k).or_default() += 1;
        }
        vars
    };
    let (want, got) = (key(source), key(target));
    for (k, n) in &want {
        let have = got.get(k).copied().unwrap_or(0);
        if have < *n {
            return Err(format!("the translation is missing {k} (the original has it {n} time(s), the translation {have})"));
        }
    }
    for (k, n) in &got {
        if want.get(k).copied().unwrap_or(0) < *n {
            let hint = if k.len() > 2 && !matches!(&k[1..k.len() - 1], "nw" | "p" | "w" | "size" | "/size" | "color" | "/color") {
                " (a variable the original does not use: a typo?)"
            } else {
                ""
            };
            return Err(format!("the translation has {k} more often than the original{hint}"));
        }
    }
    crate::markup::check_markup(target).map_err(|m| format!("the translation's text tags are wrong: {m}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(t: &str) -> Catalog {
        Catalog::parse(t).unwrap()
    }

    fn errors(t: &str) -> Vec<String> {
        Catalog::parse(t).unwrap_err().iter().map(ToString::to_string).collect()
    }

    #[test]
    fn reads_entries_with_comments_context_and_multiple_lines() {
        let c = parse(
            "# header\n\n#: start:3\n> Hello, {name}!\n= Hej, {name}!\n\n@ Mika\n> Hello, {name}!\n= Tjena, {name}!\n\n\
             > One\n> Two\n= Ett\n= Två\n\n> \"Quoted\" \\{ {size=+4}big{/size}\n= \"Citerat\"\n",
        );
        assert_eq!(c.entries.len(), 4);
        assert_eq!(c.get(None, "Hello, {name}!"), Some("Hej, {name}!"));
        assert_eq!(c.get(Some("Mika"), "Hello, {name}!"), Some("Tjena, {name}!"));
        assert_eq!(c.get(Some("Ren"), "Hello, {name}!"), Some("Hej, {name}!"), "falls back to no context");
        assert_eq!(c.get(None, "One\nTwo"), Some("Ett\nTvå"));
        assert_eq!(c.get(None, "\"Quoted\" \\{ {size=+4}big{/size}"), Some("\"Citerat\""));
        assert_eq!(c.get(None, "nothing"), None);
        assert_eq!(c.text(None, "nothing"), "nothing");
    }

    #[test]
    fn an_empty_translation_is_untranslated_and_flags_are_read() {
        let c = parse("! obsolete\n> Old\n= Gammal\n\n> New\n=\n");
        assert!(c.entries[0].obsolete);
        assert_eq!(c.get(None, "Old"), Some("Gammal"), "obsolete entries still apply");
        assert_eq!(c.get(None, "New"), None);
        let c = parse("\u{feff}> A\r\n= B\r\n");
        assert_eq!(c.get(None, "A"), Some("B"));
    }

    #[test]
    fn errors_name_the_line() {
        assert!(errors("> a\n").iter().any(|e| e.contains("followed by its `=`")), "{:?}", errors("> a\n"));
        assert!(errors("= x\n")[0].starts_with("line 1: `=`"));
        assert!(errors("hello\n")[0].starts_with("line 1: a line must start"));
        assert!(errors("> a\n= b\n\n> a\n= c\n")[0].starts_with("line 4: this text is already translated at line 1"));
        assert!(errors("! maybe\n> a\n= b\n")[0].contains("unknown flag"));
        assert!(errors("@ \n> a\n= b\n")[0].starts_with("line 1"));
        assert!(errors("> a\n# no\n= b\n")[0].starts_with("line 2"));
        assert!(errors(">\n=\n")[0].contains("empty"));
        assert!(errors("@ A\n@ B\n> a\n= b\n")[0].contains("once"));
        assert_eq!(errors("x\ny\n").len(), 2, "every mistake is listed");
    }

    #[test]
    fn limits_hold() {
        let big = format!("> {}\n= x\n", "a".repeat(MAX_TEXT_BYTES + 1));
        assert!(errors(&big)[0].contains("longer than"));
        let many: String = (0..MAX_ENTRIES + 5).map(|i| format!("> {i}\n=\n")).collect();
        assert!(errors(&many).iter().any(|e| e.contains("more than")));
        let huge = "#".repeat(MAX_FILE_BYTES + 1);
        assert!(errors(&huge)[0].contains("larger than"));
    }

    #[test]
    fn hostile_input_never_panics() {
        let mut seed = 0x1234_5678_u64;
        let alphabet: Vec<char> = "#@!>= \n\r\t\u{0}é日{}\\abc:".chars().collect();
        for _ in 0..3000 {
            let n = (crate::splitmix64(&mut seed) % 60) as usize;
            let text: String = (0..n).map(|_| alphabet[(crate::splitmix64(&mut seed) % alphabet.len() as u64) as usize]).collect();
            if let Ok(c) = Catalog::parse(&text) {
                let again = Catalog::parse(&c.render("x", &BTreeMap::new())).expect("rendered files parse");
                assert_eq!(again.entries.len(), c.entries.len());
            }
            let _ = check_translation(&text, &text.chars().rev().collect::<String>());
        }
    }

    #[test]
    fn placeholders_ignore_escapes_and_open_braces() {
        assert_eq!(placeholders("a {x} \\{y} {nw} { open"), ["x", "nw"]);
        assert_eq!(placeholders("\\\\{x}"), ["x"]);
        assert_eq!(placeholders("{ size=+4 }"), ["size=+4"]);
    }

    #[test]
    fn translations_must_keep_variables_and_tags() {
        let ok = |s, t| check_translation(s, t);
        assert!(ok("Hi {name}{nw}", "Hej {name}{nw}").is_ok());
        assert!(ok("{color=#f00}x{/color}", "{color=#00f}y{/color}").is_ok(), "arguments may differ");
        assert!(ok("Hi {name}", "").is_ok());
        assert!(ok("Hi {name}", "Hej").unwrap_err().contains("missing {name}"));
        assert!(ok("Hi {name}", "Hej {nmae}").unwrap_err().contains("missing {name}"));
        assert!(ok("Hi", "Hej {name}").unwrap_err().contains("typo"));
        assert!(ok("Hi{nw}", "Hej").unwrap_err().contains("{nw}"));
        assert!(ok("Hi", "Hej{nw}").unwrap_err().contains("more often"));
        assert!(ok("Hi {x}", "Hej {x} {nw} {x}").is_err());
        assert!(ok("a {x} b", "a {x} {nw} b").is_err(), "extra tag");
        assert!(ok("a **b**", "a **b").unwrap_err().contains("never closed"));
        assert!(ok("{p}a", "{p}{p}a").is_err());
    }

    #[test]
    fn code_rules() {
        for good in ["en", "sv", "pt-BR", "es_419"] {
            assert_eq!(bad_code(good), None, "{good}");
        }
        for bad in ["", "a/b", "..", "x y", "aaaaaaaaaaaaaaaaa", "日本", "con", "a.b"] {
            assert!(bad_code(bad).is_some(), "{bad}");
        }
        assert_eq!(language_file("sv"), "lang/sv.txt");
    }

    fn prog(src: &str) -> Program {
        crate::parse(src).unwrap()
    }

    #[test]
    fn languages_are_declared_in_the_front_matter() {
        let p = prog("---\nlanguages: en=English, sv=Svenska, pt-BR = \"Português\"\n---\nHi.\n");
        let codes: Vec<(&str, &str)> = p.languages.iter().map(|l| (l.code.as_str(), l.name.as_str())).collect();
        assert_eq!(codes, [("en", "English"), ("sv", "Svenska"), ("pt-BR", "Português")]);
        assert!(prog("Hi.\n").languages.is_empty());
        let errs = |fm: &str| crate::parse(&format!("---\n{fm}\n---\nHi.\n")).unwrap_err().remove(0);
        for (fm, needle) in [
            ("languages: en", "code=Name"),
            ("languages: en=A, en=B", "twice"),
            ("languages: ../x=A", "bad language code"),
            ("languages: en=", "name"),
            ("languages:", "needs entries"),
            ("languages: a=A\nlanguages: b=B", "twice"),
        ] {
            let e = errs(fm);
            assert!(e.message.contains(needle), "{fm}: {}", e.message);
            assert!(e.line >= 2);
        }
        let many: Vec<String> = (0..17).map(|i| format!("l{i}=L")).collect();
        assert!(errs(&format!("languages: {}", many.join(", "))).message.contains("at most 16"));
    }

    #[test]
    fn translatable_lists_every_text_once_in_script_order() {
        let p = prog(
            "---\ntitle: Tale\nreplays: { a: \"Scene\" }\nvars: { n: 0, who: \"A\", bag: [] }\ninventory: bag\n---\n\
             ::character{id=mika name=Mika}\n::item{id=k name=Key text=\"Opens it.\"}\n# a\n\
             **Mika:** Hi {n}.\n\nHi {n}.\n\n**Ren:** Hi {n}.\n\n::hud{id=h text=\"HP {n}\"}\n::input{var=who prompt=\"Name?\"}\n\
             ::choice\n- [Yes](#a)\n- [Hi {n}.](#a)\n::\n",
        );
        let all = p.translatable();
        let texts: Vec<&str> = all.iter().map(|s| s.text.as_str()).collect();
        assert_eq!(texts, ["Tale", "Scene", "Mika", "Key", "Opens it.", "Hi {n}.", "Ren", "HP {n}", "Name?", "Yes"]);
        let hi = all.iter().find(|s| s.text == "Hi {n}.").unwrap();
        assert_eq!(hi.speakers.iter().map(String::as_str).collect::<Vec<_>>(), ["Mika", "Ren"]);
        assert!(hi.place.starts_with("a:"), "{}", hi.place);
        assert!(p.spoken().contains(&("Mika".to_string(), "Hi {n}.".to_string())));
    }

    #[test]
    fn merging_keeps_translations_adds_new_texts_and_flags_the_rest() {
        let old = parse("> A\n= a\n\n@ Bob\n> A\n= bob-a\n\n> Gone\n= borta\n\n! obsolete\n> B\n= b\n");
        let src = |t: &str, speakers: &[&str]| Source { text: t.into(), place: "x:1".into(), speakers: speakers.iter().map(|s| s.to_string()).collect() };
        let merged = old.merged(&[src("B", &[]), src("A", &["Bob"]), src("New", &[])]);
        let view: Vec<(Option<&str>, &str, &str, bool)> =
            merged.entries.iter().map(|e| (e.context.as_deref(), e.source.as_str(), e.target.as_str(), e.obsolete)).collect();
        assert_eq!(
            view,
            [
                (None, "B", "b", false),
                (None, "A", "a", false),
                (Some("Bob"), "A", "bob-a", false),
                (None, "New", "", false),
                (None, "Gone", "borta", true),
            ]
        );
        assert_eq!(merged.merged(&[src("B", &[]), src("A", &["Bob"]), src("New", &[])]), merged);
        // a context version nobody speaks any more is flagged
        let again = old.merged(&[src("A", &[])]);
        assert!(again.entries.iter().find(|e| e.context.is_some()).unwrap().obsolete);
    }
}
