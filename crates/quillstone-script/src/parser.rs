//! Line-based parser/compiler: markdown-flavoured script text -> [`Program`].
//!
//! Line kinds:
//! - `# heading`              label (any heading level)
//! - `**Name:** text`         dialogue; other prose is narration
//! - `![alt](path)`           inline image, or video if the extension says so
//! - `::name{key=value flag}` command, one line (see SCRIPT.md for the full list)
//! - `::choice` ... `::`      choice block with `- [text](#label){effects, if cond}` options
//! - `::if{c}` `::elif{c}` `::else` `::endif`   conditional blocks, lowered to jumps
//! - `<!-- comment -->`       ignored (single line)
//!
//! Consecutive prose lines are joined into one paragraph. Errors are collected
//! (not just the first) and carry 1-based line numbers.

use std::collections::BTreeMap;
use std::fmt;
use std::ops::RangeInclusive;

use crate::expr::{is_ident, parse_effect, parse_expr, parse_value, split_top_level};
use crate::ops::*;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseError {
    pub line: usize,
    pub message: String,
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "line {}: {}", self.line, self.message)
    }
}

impl std::error::Error for ParseError {}

/// Compile script source into a [`Program`], or return every error found.
pub fn parse(src: &str) -> Result<Program, Vec<ParseError>> {
    let mut p = Parser::default();
    p.run(src);
    p.finish()
}

const COMMANDS: &str = "character, bg, show, hide, move, music, sfx, voice, wait, video, \
                        choice, jump, call, return, set, if, elif, else, endif, end";

#[derive(Default)]
struct Parser {
    program: Program,
    errors: Vec<ParseError>,
    fixups: Vec<Fixup>,
    blocks: Vec<IfBlock>,
}

/// A jump target that must be filled in once all labels are known.
struct Fixup {
    op: usize,
    option: Option<usize>,
    label: String,
    line: usize,
}

/// An open `::if` block.
struct IfBlock {
    /// The conditional jump over the current branch, patched at the next elif/else/endif.
    pending: Option<usize>,
    /// Unconditional jumps from the end of finished branches to after `::endif`.
    end_jumps: Vec<usize>,
    line: usize,
    seen_else: bool,
}

const UNSET: usize = usize::MAX;

impl Parser {
    fn err(&mut self, line: usize, message: impl Into<String>) {
        self.errors.push(ParseError { line, message: message.into() });
    }

    fn push(&mut self, op: Op, line: usize) {
        self.program.ops.push(op);
        self.program.lines.push(line);
    }

    fn here(&self) -> usize {
        self.program.ops.len()
    }

    fn patch(&mut self, jump: usize, target: usize) {
        if let Op::Jump { target: t, .. } = &mut self.program.ops[jump] {
            *t = target;
        }
    }

    fn run(&mut self, src: &str) {
        let src = src.strip_prefix('\u{feff}').unwrap_or(src);
        let lines: Vec<&str> = src.lines().collect();
        let mut i = 0;

        if lines.first().map(|l| l.trim()) == Some("---") {
            match lines.iter().skip(1).position(|l| l.trim() == "---") {
                Some(rel) => {
                    let end = rel + 1;
                    self.front_matter(&lines[1..end]);
                    i = end + 1;
                }
                None => {
                    self.err(1, "front matter opened with `---` is never closed");
                    return;
                }
            }
        }

        let mut para: Vec<(usize, &str)> = Vec::new();
        while i < lines.len() {
            let line_no = i + 1;
            let t = lines[i].trim();
            if t.is_empty() {
                self.flush(&mut para);
            } else if t.starts_with("<!--") {
                // A comment, on one line or several: ignored up to the line with `-->`.
                let mut end = i;
                loop {
                    let l = lines[end].trim();
                    if let Some(pos) = l.find("-->") {
                        if !l[pos + 3..].trim().is_empty() {
                            self.err(end + 1, "nothing may follow `-->` on a comment's last line");
                        }
                        break;
                    }
                    end += 1;
                    if end >= lines.len() {
                        self.err(line_no, "comment opened with `<!--` is never closed with `-->`");
                        end = lines.len() - 1;
                        break;
                    }
                }
                i = end;
            } else if let Some(text) = heading(t) {
                self.flush(&mut para);
                self.label(line_no, text);
            } else if t.starts_with("::") {
                self.flush(&mut para);
                i = self.command(&lines, i);
            } else if let Some((alt, path)) = standalone_image(t) {
                self.flush(&mut para);
                self.image(line_no, alt, path);
            } else {
                if split_say(t).is_some() {
                    self.flush(&mut para);
                }
                para.push((line_no, t));
            }
            i += 1;
        }
        self.flush(&mut para);
    }

    fn finish(mut self) -> Result<Program, Vec<ParseError>> {
        for block in std::mem::take(&mut self.blocks) {
            self.err(block.line, "`::if` is never closed with `::endif`");
        }

        let last_line = self.program.lines.last().copied().unwrap_or(1);
        self.push(Op::End, last_line);

        for f in std::mem::take(&mut self.fixups) {
            match self.program.labels.get(&f.label).copied() {
                Some(target) => match &mut self.program.ops[f.op] {
                    Op::Jump { target: t, .. } | Op::Call { target: t } => *t = target,
                    Op::Choice { options } => {
                        if let Some(k) = f.option {
                            options[k].target = target;
                        }
                    }
                    _ => {}
                },
                None => {
                    let known: Vec<&str> = self.program.labels.keys().map(String::as_str).collect();
                    let msg = if known.is_empty() {
                        format!("unknown label `{}` (the script has no labels)", f.label)
                    } else {
                        format!("unknown label `{}` (known: {})", f.label, known.join(", "))
                    };
                    self.err(f.line, msg);
                }
            }
        }

        // Tie dialogue speakers to declared characters (display name + colour).
        let characters = self.program.characters.clone();
        for op in &mut self.program.ops {
            if let Op::Say { speaker: Some(name), color, .. } = op {
                let wanted = name.to_lowercase();
                if let Some(c) = characters
                    .values()
                    .find(|c| c.id == wanted || c.name.to_lowercase() == wanted)
                {
                    *name = c.name.clone();
                    *color = c.color;
                }
            }
        }

        // `seen(label)`: the label must exist, and its visits are counted from now on.
        let mut counted: BTreeMap<usize, Vec<String>> = BTreeMap::new();
        let mut unknown = Vec::new();
        for (at, op) in self.program.ops.iter().enumerate() {
            let mut names = Vec::new();
            for e in op.exprs() {
                e.vars_read(&mut names);
            }
            for var in names {
                let Some(label) = var.strip_prefix(SEEN) else { continue };
                match self.program.labels.get(label) {
                    Some(&target) => {
                        let list = counted.entry(target).or_default();
                        if !list.iter().any(|v| v == var) {
                            list.push(var.to_string());
                        }
                    }
                    None => unknown.push((self.program.lines[at], label.to_string())),
                }
            }
        }
        self.program.counted = counted;
        for (line, label) in unknown {
            self.err(line, format!("`seen({label})`: there is no label `{label}`"));
        }

        if self.errors.is_empty() {
            Ok(self.program)
        } else {
            self.errors.sort_by_key(|e| e.line);
            Err(self.errors)
        }
    }

    // ---- front matter -------------------------------------------------

    fn front_matter(&mut self, lines: &[&str]) {
        for (idx, raw) in lines.iter().enumerate() {
            let line = idx + 2; // line 1 is the opening `---`
            let t = raw.trim();
            if t.is_empty() || t.starts_with('#') {
                continue;
            }
            let Some((key, val)) = t.split_once(':') else {
                self.err(line, "front matter lines look like `key: value`");
                continue;
            };
            let (key, val) = (key.trim(), val.trim());
            match key {
                "title" => self.program.title = Some(unquote(val).to_string()),
                "theme" if !unquote(val).is_empty() => self.program.theme = Some(unquote(val).to_string()),
                "theme" => self.err(line, "`theme` needs a file name, like `theme: theme.txt`"),
                "vars" => self.front_vars(line, val, false),
                "persistent" => self.front_vars(line, val, true),
                "gallery" | "music_room" => self.front_list(line, key, val),
                "voice" => match unquote(val).trim_end_matches(['/', '\\']) {
                    "" => self.err(line, "`voice` needs a folder, like `voice: voice`"),
                    folder => self.program.voice_folder = Some(folder.to_string()),
                },
                other => self.err(line, format!("unknown front matter key `{other}`")),
            }
        }
    }

    /// `vars: { ... }`, or `persistent: { ... }` for variables kept across playthroughs.
    fn front_vars(&mut self, line: usize, val: &str, persistent: bool) {
        let key = if persistent { "persistent" } else { "vars" };
        let Some(inner) = val.strip_prefix('{').and_then(|v| v.strip_suffix('}')) else {
            self.err(line, format!("`{key}` must look like `{{ trust: 0, met: false, name: \"Alex\" }}`"));
            return;
        };
        for part in split_top_level(inner, ',').into_iter().map(str::trim).filter(|p| !p.is_empty()) {
            match part.split_once(':') {
                Some((name, v)) if is_ident(name.trim()) => match parse_value(v.trim()) {
                    Ok(value) => {
                        let name = name.trim().to_string();
                        let both = self.program.persistent.contains(&name) != persistent;
                        if self.program.vars.insert(name.clone(), value).is_some() && both {
                            self.err(line, format!("`{name}` is in both `vars` and `persistent`; pick one"));
                        }
                        if persistent {
                            self.program.persistent.insert(name);
                        }
                    }
                    Err(m) => self.err(line, m),
                },
                _ => self.err(line, format!("bad variable `{part}` (expected `name: 0`)")),
            }
        }
    }

    /// `gallery: [a.png, b.png]` or `music_room: [...]`; the key may repeat.
    fn front_list(&mut self, line: usize, key: &str, val: &str) {
        let Some(inner) = val.strip_prefix('[').and_then(|v| v.strip_suffix(']')) else {
            self.err(line, format!("`{key}` must look like `[assets/one.png, assets/two.png]`"));
            return;
        };
        for item in split_top_level(inner, ',').into_iter().map(|p| unquote(p.trim())).filter(|p| !p.is_empty()) {
            let list = if key == "gallery" { &mut self.program.gallery } else { &mut self.program.music_room };
            if !list.iter().any(|p| p == item) {
                list.push(item.to_string());
            }
        }
    }

    // ---- labels, prose, images ------------------------------------------

    fn label(&mut self, line: usize, text: &str) {
        let name = slug(text);
        if name.is_empty() {
            self.err(line, "heading has no usable label name");
            return;
        }
        let at = self.here();
        if self.program.labels.insert(name.clone(), at).is_some() {
            self.err(line, format!("duplicate label `{name}`"));
        }
    }

    fn flush(&mut self, para: &mut Vec<(usize, &str)>) {
        if para.is_empty() {
            return;
        }
        let line = para[0].0;
        let joined = para.iter().map(|(_, t)| *t).collect::<Vec<_>>().join(" ");
        para.clear();

        let (speaker, text) = match split_say(&joined) {
            Some((name, text)) => (Some(name.to_string()), text.to_string()),
            None => (None, joined.clone()),
        };
        if text.is_empty() {
            self.err(line, "dialogue has no text after the speaker name");
            return;
        }
        if let Err(m) = crate::markup::check_markup(&text) {
            self.err(line, m);
        }
        self.push(Op::Say { speaker, color: None, text }, line);
    }

    fn image(&mut self, line: usize, alt: &str, path: &str) {
        if path.is_empty() {
            self.err(line, "image has no path");
            return;
        }
        let op = if kind_from_path(path) == AssetKind::Video {
            Op::Video { src: path.to_string(), fit: Fit::Contain, looped: false, skippable: false }
        } else {
            Op::Image { src: path.to_string(), alt: alt.to_string() }
        };
        self.push(op, line);
    }

    // ---- commands ---------------------------------------------------------

    /// Parse the command at `lines[i]`. Returns the index of the last line consumed.
    fn command(&mut self, lines: &[&str], i: usize) -> usize {
        let line = i + 1;
        let body = &lines[i].trim()[2..];
        if body.trim().is_empty() {
            self.err(line, "stray `::` (it only closes a `::choice` block)");
            return i;
        }
        let name_len = body
            .find(|c: char| !(c.is_alphanumeric() || c == '_' || c == '-'))
            .unwrap_or(body.len());
        let name = &body[..name_len];
        if name.is_empty() {
            self.err(line, "missing command name after `::`");
            return i;
        }
        let rest = body[name_len..].trim();
        let inner = if rest.is_empty() {
            ""
        } else if let Some(x) = rest.strip_prefix('{').and_then(|r| r.strip_suffix('}')) {
            x
        } else {
            self.err(line, format!("expected `{{...}}` after `::{name}`"));
            return i;
        };

        match name {
            "choice" => return self.choice(lines, i, inner),
            "set" => {
                let parts: Vec<&str> =
                    split_top_level(inner, ',').into_iter().map(str::trim).filter(|p| !p.is_empty()).collect();
                if parts.is_empty() {
                    self.err(line, "`::set` needs an assignment like `{trust+=1}`");
                }
                for part in parts {
                    match parse_effect(part) {
                        Ok(e) => self.push(Op::Set(e), line),
                        Err(m) => self.err(line, m),
                    }
                }
                return i;
            }
            "character" => {
                self.character(line, inner);
                return i;
            }
            "if" => {
                self.if_start(line, inner);
                return i;
            }
            "elif" => {
                self.elif(line, inner);
                return i;
            }
            "else" | "endif" | "return" => {
                if !inner.is_empty() {
                    self.err(line, format!("`::{name}` takes no attributes"));
                }
                match name {
                    "else" => self.else_branch(line),
                    "endif" => self.endif(line),
                    _ => self.push(Op::Return, line),
                }
                return i;
            }
            _ => {}
        }

        let mut attrs = match Attrs::parse(inner) {
            Ok(a) => a,
            Err(m) => {
                self.err(line, m);
                return i;
            }
        };
        let built = self.build_op(line, name, &mut attrs);
        if let Some(extra) = attrs.leftover() {
            self.err(line, format!("unknown attribute `{extra}` for `::{name}`"));
        }
        if let Some((op, label)) = built {
            if let Some(label) = label {
                self.fixups.push(Fixup { op: self.here(), option: None, label, line });
            }
            self.push(op, line);
        }
        i
    }

    /// Build an op from attributes. The `Option<String>` is a label to resolve later.
    fn build_op(&mut self, line: usize, name: &str, a: &mut Attrs) -> Option<(Op, Option<String>)> {
        match name {
            "bg" => {
                let src = self.require(line, name, a, "src")?;
                let fade = self.fade(line, a)?;
                let wipe = match a.value("wipe") {
                    None => None,
                    Some(v) => match Wipe::parse(&v) {
                        Some(w) => Some(w),
                        None => {
                            self.err(line, format!("`wipe={v}` must be `left`, `right`, `up` or `down`"));
                            return None;
                        }
                    },
                };
                if wipe.is_some() && fade <= 0.0 {
                    self.err(line, "a wipe needs a duration: add `fade=` (seconds)");
                    return None;
                }
                Some((Op::Bg { src, fade, wipe }, None))
            }
            "show" => self.show(line, a),
            "hide" => {
                let all = a.flag("all");
                let bare = a.take_bare();
                let fade = self.fade(line, a)?;
                let id = match (all, bare.first()) {
                    (true, _) => None,
                    (false, Some(id)) => Some(self.sprite_id(id)),
                    (false, None) => {
                        self.err(line, "`::hide` needs a sprite (`::hide{mika}`) or `all`");
                        return None;
                    }
                };
                Some((Op::Hide { id, fade }, None))
            }
            "move" => {
                let bare = a.take_bare();
                let Some(id) = bare.first().map(|s| self.sprite_id(s)) else {
                    self.err(line, "`::move` needs a sprite, like `::move{mika at=left time=0.5}`");
                    return None;
                };
                let at = match a.value("at") {
                    Some(v) => match parse_position(&v) {
                        Ok(x) => Some(x),
                        Err(m) => {
                            self.err(line, m);
                            return None;
                        }
                    },
                    None => None,
                };
                let scale = self.number(line, a, "scale", 0.05..=10.0)?;
                if at.is_none() && scale.is_none() {
                    self.err(line, "`::move` needs `at=...` and/or `scale=...`");
                    return None;
                }
                let time = self.seconds(line, a, "time", 0.5)?;
                let ease = self.ease(line, a)?;
                Some((Op::Move { id, at, scale, time, ease }, None))
            }
            "shake" => {
                let time = self.seconds(line, a, "time", 0.4)?;
                let strength = self.number(line, a, "strength", 0.0..=200.0)?.unwrap_or(20.0);
                Some((Op::Shake { time, strength }, None))
            }
            "camera" => {
                let reset = a.flag("reset");
                let zoom = self.number(line, a, "zoom", 1.0..=4.0)?;
                let x = self.number(line, a, "x", 0.0..=1.0)?;
                let y = self.number(line, a, "y", 0.0..=1.0)?;
                let time = self.seconds(line, a, "time", 0.0)?;
                let ease = self.ease(line, a)?;
                let op = if reset {
                    Op::Camera { zoom: Some(1.0), x: Some(0.5), y: Some(0.5), time, ease }
                } else if zoom.is_none() && x.is_none() && y.is_none() {
                    self.err(line, "`::camera` needs `zoom=`, `x=` and/or `y=` (or `reset`)");
                    return None;
                } else {
                    Op::Camera { zoom, x, y, time, ease }
                };
                Some((op, None))
            }
            "music" => {
                if a.flag("stop") {
                    let fade = self.fade(line, a)?;
                    Some((Op::StopMusic { fade }, None))
                } else {
                    let src = self.require(line, name, a, "src")?;
                    let fade = self.fade(line, a)?;
                    let looped = a.flag_or("loop", true);
                    let volume = self.volume(line, a)?;
                    Some((Op::Music { src, fade, looped, volume }, None))
                }
            }
            "sfx" => {
                let src = self.require(line, name, a, "src")?;
                let volume = self.volume(line, a)?;
                Some((Op::Sfx { src, volume }, None))
            }
            "voice" => {
                let src = self.require(line, name, a, "src")?;
                Some((Op::Voice { src }, None))
            }
            "wait" => {
                let value = a.value("time").or_else(|| a.take_bare().into_iter().next());
                let seconds = match value.as_deref().map(str::parse::<f32>) {
                    Some(Ok(s)) if s.is_finite() && s > 0.0 => s,
                    _ => {
                        self.err(line, "`::wait` needs a positive number of seconds, like `::wait{time=1.5}`");
                        return None;
                    }
                };
                Some((Op::Wait { seconds }, None))
            }
            "video" => {
                let src = self.require(line, name, a, "src")?;
                let fit = match a.value("fit") {
                    None => Fit::default(),
                    Some(v) => match Fit::parse(&v) {
                        Some(f) => f,
                        None => {
                            self.err(line, format!("unknown fit `{v}` (use contain, cover or stretch)"));
                            return None;
                        }
                    },
                };
                let looped = a.flag("loop");
                let skippable = a.flag("skippable");
                Some((Op::Video { src, fit, looped, skippable }, None))
            }
            "jump" | "call" => {
                let to = match a.value("to").or_else(|| a.take_bare().into_iter().next()) {
                    Some(t) => t,
                    None => {
                        self.err(line, format!("`::{name}` needs a label, like `::{name}{{to=label}}`"));
                        return None;
                    }
                };
                let label = Some(slug(&to));
                if name == "call" {
                    return Some((Op::Call { target: UNSET }, label));
                }
                let cond = match a.value("if") {
                    None => None,
                    Some(c) => match parse_expr(&c) {
                        Ok(c) => Some(c),
                        Err(m) => {
                            self.err(line, m);
                            return None;
                        }
                    },
                };
                Some((Op::Jump { target: UNSET, cond }, label))
            }
            "end" => Some((Op::End, None)),
            other => {
                self.err(line, format!("unknown command `::{other}` (known: {COMMANDS})"));
                None
            }
        }
    }

    fn show(&mut self, line: usize, a: &mut Attrs) -> Option<(Op, Option<String>)> {
        let bare = a.take_bare();
        let src_attr = a.value("src");
        let id_attr = a.value("id");

        let (id, src) = match src_attr {
            Some(src) => {
                let id = id_attr.or_else(|| bare.first().cloned()).unwrap_or_else(|| src.clone());
                (id, src)
            }
            None => {
                let Some(who) = bare.first() else {
                    self.err(line, "`::show` needs a character (`::show{mika happy}`) or `src=...`");
                    return None;
                };
                let pose = bare.get(1).cloned().unwrap_or_else(|| "default".to_string());
                let character = self.program.characters.get(&who.to_lowercase()).cloned();
                let Some(character) = character else {
                    self.err(line, format!("unknown character `{who}` (declare it first with `::character`)"));
                    return None;
                };
                let Some(template) = character.sprites else {
                    self.err(line, format!("character `{who}` has no `sprites=...` template"));
                    return None;
                };
                (character.id, template.replace("{pose}", &pose))
            }
        };

        let at = match a.value("at") {
            None => None,
            Some(v) => match parse_position(&v) {
                Ok(x) => Some(x),
                Err(m) => {
                    self.err(line, m);
                    return None;
                }
            },
        };
        let scale = match a.value("scale") {
            None => 1.0,
            Some(v) => match v.parse::<f32>() {
                Ok(s) if s.is_finite() && s > 0.0 => s,
                _ => {
                    self.err(line, format!("`scale={v}` must be a number above 0"));
                    return None;
                }
            },
        };
        let fade = self.fade(line, a)?;
        Some((Op::Show { id, src, at, scale, fade }, None))
    }

    /// Resolve a name used in `::hide` / `::move` to the sprite id (characters are lowercase).
    fn sprite_id(&self, name: &str) -> String {
        match self.program.characters.get(&name.to_lowercase()) {
            Some(c) => c.id.clone(),
            None => name.to_string(),
        }
    }

    fn character(&mut self, line: usize, inner: &str) {
        let mut a = match Attrs::parse(inner) {
            Ok(a) => a,
            Err(m) => {
                self.err(line, m);
                return;
            }
        };
        let id = a.value("id");
        let name = a.value("name");
        let color = a.value("color");
        let sprites = a.value("sprites");
        if let Some(extra) = a.leftover() {
            self.err(line, format!("unknown attribute `{extra}` for `::character`"));
        }
        let Some(id) = id else {
            self.err(line, "`::character` needs `id=...`");
            return;
        };
        if !is_ident(&id) {
            self.err(line, format!("character id `{id}` must be letters, digits and `_`"));
            return;
        }
        let color = match color {
            None => None,
            Some(c) => match parse_color(&c) {
                Ok(c) => Some(c),
                Err(m) => {
                    self.err(line, m);
                    None
                }
            },
        };
        let key = id.to_lowercase();
        if self.program.characters.contains_key(&key) {
            self.err(line, format!("character `{id}` is declared twice"));
            return;
        }
        let name = name.unwrap_or_else(|| id.clone());
        self.program.characters.insert(key.clone(), Character { id: key, name, color, sprites });
    }

    fn require(&mut self, line: usize, cmd: &str, a: &mut Attrs, key: &str) -> Option<String> {
        match a.value(key) {
            Some(v) if !v.is_empty() => Some(v),
            _ => {
                self.err(line, format!("`::{cmd}` needs `{key}=...`"));
                None
            }
        }
    }

    fn fade(&mut self, line: usize, a: &mut Attrs) -> Option<f32> {
        self.seconds(line, a, "fade", 0.0)
    }

    /// A non-negative number of seconds under `key`, or `default` when absent.
    fn seconds(&mut self, line: usize, a: &mut Attrs, key: &str, default: f32) -> Option<f32> {
        match a.value(key) {
            None => Some(default),
            Some(v) => match v.parse::<f32>() {
                Ok(f) if f.is_finite() && f >= 0.0 => Some(f),
                _ => {
                    self.err(line, format!("`{key}={v}` must be a number of seconds, 0 or more"));
                    None
                }
            },
        }
    }

    /// An optional number under `key` within `range` (`50%` means 0.5).
    /// `None` inside means absent; an outer `None` means it was given but wrong.
    fn number(&mut self, line: usize, a: &mut Attrs, key: &str, range: RangeInclusive<f32>) -> Option<Option<f32>> {
        let Some(v) = a.value(key) else { return Some(None) };
        let parsed = match v.strip_suffix('%') {
            Some(pct) => pct.parse::<f32>().map(|p| p / 100.0),
            None => v.parse::<f32>(),
        };
        match parsed {
            Ok(f) if f.is_finite() && range.contains(&f) => Some(Some(f)),
            _ => {
                self.err(line, format!("`{key}={v}` must be a number from {} to {}", range.start(), range.end()));
                None
            }
        }
    }

    fn ease(&mut self, line: usize, a: &mut Attrs) -> Option<Ease> {
        match a.value("ease") {
            None => Some(Ease::default()),
            Some(v) => match Ease::parse(&v) {
                Some(e) => Some(e),
                None => {
                    self.err(line, format!("`ease={v}` must be `linear`, `in`, `out` or `inout`"));
                    None
                }
            },
        }
    }

    fn volume(&mut self, line: usize, a: &mut Attrs) -> Option<f32> {
        match a.value("volume") {
            None => Some(1.0),
            Some(v) => match v.parse::<f32>() {
                Ok(f) if (0.0..=1.0).contains(&f) => Some(f),
                _ => {
                    self.err(line, format!("`volume={v}` must be between 0 and 1"));
                    None
                }
            },
        }
    }

    // ---- if / elif / else / endif ---------------------------------------

    fn condition(&mut self, line: usize, text: &str) -> Expr {
        match parse_expr(text) {
            Ok(e) => e,
            Err(m) => {
                self.err(line, m);
                // keep the block structure intact so later lines don't cascade errors
                Expr::Lit(Value::Bool(true))
            }
        }
    }

    /// Emit "jump past this branch unless `cond`" and return its op index.
    fn jump_unless(&mut self, line: usize, cond: Expr) -> usize {
        let idx = self.here();
        self.push(Op::Jump { target: UNSET, cond: Some(Expr::Not(Box::new(cond))) }, line);
        idx
    }

    fn if_start(&mut self, line: usize, inner: &str) {
        let cond = self.condition(line, inner);
        let pending = self.jump_unless(line, cond);
        self.blocks.push(IfBlock { pending: Some(pending), end_jumps: Vec::new(), line, seen_else: false });
    }

    fn elif(&mut self, line: usize, inner: &str) {
        let Some(mut block) = self.blocks.pop() else {
            self.err(line, "`::elif` without a matching `::if`");
            return;
        };
        if block.seen_else {
            self.err(line, "`::elif` after `::else`");
        }
        let end_jump = self.here();
        self.push(Op::Jump { target: UNSET, cond: None }, line);
        block.end_jumps.push(end_jump);
        if let Some(p) = block.pending.take() {
            let here = self.here();
            self.patch(p, here);
        }
        let cond = self.condition(line, inner);
        block.pending = Some(self.jump_unless(line, cond));
        self.blocks.push(block);
    }

    fn else_branch(&mut self, line: usize) {
        let Some(mut block) = self.blocks.pop() else {
            self.err(line, "`::else` without a matching `::if`");
            return;
        };
        if block.seen_else {
            self.err(line, "`::if` already has an `::else`");
        }
        let end_jump = self.here();
        self.push(Op::Jump { target: UNSET, cond: None }, line);
        block.end_jumps.push(end_jump);
        if let Some(p) = block.pending.take() {
            let here = self.here();
            self.patch(p, here);
        }
        block.seen_else = true;
        self.blocks.push(block);
    }

    fn endif(&mut self, line: usize) {
        let Some(block) = self.blocks.pop() else {
            self.err(line, "`::endif` without a matching `::if`");
            return;
        };
        let here = self.here();
        if let Some(p) = block.pending {
            self.patch(p, here);
        }
        for j in block.end_jumps {
            self.patch(j, here);
        }
    }

    // ---- choice blocks ---------------------------------------------------

    fn choice(&mut self, lines: &[&str], start: usize, inner: &str) -> usize {
        let line = start + 1;
        if !inner.is_empty() {
            self.err(line, "`::choice` takes no attributes");
        }
        let mut options = Vec::new();
        let mut labels = Vec::new();
        let mut j = start + 1;
        let mut closed = false;
        while j < lines.len() {
            let t = lines[j].trim();
            if t == "::" {
                closed = true;
                break;
            }
            if !t.is_empty() {
                match self.option(t) {
                    Ok((opt, label)) => {
                        options.push(opt);
                        labels.push((label, j + 1));
                    }
                    Err(m) => self.err(j + 1, m),
                }
            }
            j += 1;
        }
        if !closed {
            self.err(line, "`::choice` is never closed with a line containing only `::`");
        }
        if options.is_empty() {
            self.err(line, "`::choice` has no options");
        } else {
            let op = self.here();
            for (k, (label, ln)) in labels.into_iter().enumerate() {
                self.fixups.push(Fixup { op, option: Some(k), label, line: ln });
            }
            self.push(Op::Choice { options }, line);
        }
        j
    }

    /// Parse `- [text](#label){var+=1, if met}`.
    fn option(&mut self, t: &str) -> Result<(ChoiceOption, String), String> {
        let t = t
            .strip_prefix("- ")
            .or_else(|| t.strip_prefix("* "))
            .ok_or("choice options must start with `- `")?
            .trim();
        let rest = t.strip_prefix('[').ok_or("expected `[text](#label)`")?;
        let close = rest.find("](").ok_or("expected `](#label)` after the option text")?;
        let text = rest[..close].trim();
        if text.is_empty() {
            return Err("choice option has no text".into());
        }
        crate::markup::check_markup(text)?;
        let after = &rest[close + 2..];
        let paren = after.find(')').ok_or("missing `)` after the label")?;
        let target = after[..paren]
            .trim()
            .strip_prefix('#')
            .ok_or("choice target must look like `#label`")?;
        let tail = after[paren + 1..].trim();

        let mut effects = Vec::new();
        let mut cond = None;
        let mut once = false;
        if !tail.is_empty() {
            let inner = tail
                .strip_prefix('{')
                .and_then(|x| x.strip_suffix('}'))
                .ok_or_else(|| format!("unexpected `{tail}` after the option (effects go in `{{...}}`)"))?;
            for item in split_top_level(inner, ',').into_iter().map(str::trim).filter(|p| !p.is_empty()) {
                // `if <cond>`; a variable merely starting with "if" (`iffy+=1`) is an effect
                let cond_text = item
                    .strip_prefix("if")
                    .filter(|r| r.is_empty() || r.starts_with(char::is_whitespace));
                if let Some(c) = cond_text {
                    cond = Some(parse_expr(c)?);
                } else if item == "once" {
                    once = true;
                } else {
                    effects.push(parse_effect(item)?);
                }
            }
        }
        let target = slug(target);
        if once {
            // Offered until picked: "not picked yet" joins the condition, and picking
            // it remembers that. Keyed by target and text, so it survives script edits.
            let not_picked = Expr::Not(Box::new(Expr::Var(format!("{ONCE}{target}:{text}"))));
            cond = Some(match cond {
                Some(c) => Expr::And(Box::new(c), Box::new(not_picked)),
                None => not_picked,
            });
            let var = format!("{ONCE}{target}:{text}");
            effects.push(Effect { var, op: AssignOp::Set, value: Expr::Lit(Value::Bool(true)) });
        }
        let opt = ChoiceOption { text: text.to_string(), target: UNSET, effects, cond };
        Ok((opt, target))
    }
}

// ---- free helpers --------------------------------------------------------

fn heading(t: &str) -> Option<&str> {
    let hashes = t.bytes().take_while(|b| *b == b'#').count();
    if (1..=6).contains(&hashes) && t[hashes..].starts_with(' ') {
        Some(t[hashes..].trim())
    } else {
        None
    }
}

/// `**Name:** text` -> (`Name`, `text`)
fn split_say(s: &str) -> Option<(&str, &str)> {
    let rest = s.strip_prefix("**")?;
    let end = rest.find(":**")?;
    let name = rest[..end].trim();
    if name.is_empty() {
        return None;
    }
    Some((name, rest[end + 3..].trim()))
}

/// A line that is exactly `![alt](path)`.
fn standalone_image(t: &str) -> Option<(&str, &str)> {
    let rest = t.strip_prefix("![")?;
    let mid = rest.find("](")?;
    let tail = &rest[mid + 2..];
    let close = tail.find(')')?;
    if close != tail.len() - 1 {
        return None;
    }
    Some((rest[..mid].trim(), tail[..close].trim()))
}

/// Normalise a heading or link target to a label: `The Roof!` -> `the-roof`.
pub(crate) fn slug(s: &str) -> String {
    let mut out = String::new();
    let mut pending_dash = false;
    for c in s.chars() {
        if c.is_alphanumeric() {
            if pending_dash && !out.is_empty() {
                out.push('-');
            }
            pending_dash = false;
            out.extend(c.to_lowercase());
        } else if c.is_whitespace() || c == '-' || c == '_' {
            pending_dash = true;
        }
    }
    out
}

fn unquote(s: &str) -> &str {
    for q in ['"', '\''] {
        if let Some(inner) = s.strip_prefix(q).and_then(|x| x.strip_suffix(q)) {
            return inner;
        }
    }
    s
}

/// Screen position as a name or a number: `left`, `0.3`, `30%`. Result is 0..1 across the screen.
fn parse_position(s: &str) -> Result<f32, String> {
    let named = match s {
        "far-left" => Some(0.12),
        "left" => Some(0.28),
        "center" | "centre" => Some(0.5),
        "right" => Some(0.72),
        "far-right" => Some(0.88),
        _ => None,
    };
    if let Some(x) = named {
        return Ok(x);
    }
    let (num, scale) = match s.strip_suffix('%') {
        Some(n) => (n, 100.0),
        None => (s, 1.0),
    };
    match num.parse::<f32>() {
        Ok(v) if (0.0..=1.0).contains(&(v / scale)) => Ok(v / scale),
        _ => Err(format!(
            "bad position `{s}` (use far-left, left, center, right, far-right, a number 0 to 1, or a percentage)"
        )),
    }
}

/// `#rrggbb` or `#rgb` -> 0..1 RGB.
pub(crate) fn parse_color(s: &str) -> Result<[f32; 3], String> {
    let bad = || format!("bad colour `{s}` (use #rrggbb or #rgb)");
    let hex = s.strip_prefix('#').ok_or_else(bad)?;
    let digits: Vec<u32> = hex.chars().map(|c| c.to_digit(16)).collect::<Option<_>>().ok_or_else(bad)?;
    let byte = |hi: u32, lo: u32| (hi * 16 + lo) as f32 / 255.0;
    match digits.as_slice() {
        [r1, r2, g1, g2, b1, b2] => Ok([byte(*r1, *r2), byte(*g1, *g2), byte(*b1, *b2)]),
        [r, g, b] => Ok([byte(*r, *r), byte(*g, *g), byte(*b, *b)]),
        _ => Err(bad()),
    }
}

/// `key=value key="quoted value" flag`
struct Attrs(Vec<(String, Option<String>)>);

impl Attrs {
    fn parse(s: &str) -> Result<Attrs, String> {
        let chars: Vec<char> = s.chars().collect();
        let mut i = 0;
        let mut items = Vec::new();
        while i < chars.len() {
            if chars[i].is_whitespace() || chars[i] == ',' {
                i += 1;
                continue;
            }
            let start = i;
            while i < chars.len() && !chars[i].is_whitespace() && chars[i] != '=' && chars[i] != ',' {
                i += 1;
            }
            let key: String = chars[start..i].iter().collect();
            if key.is_empty() {
                return Err("expected an attribute name before `=`".into());
            }
            if i < chars.len() && chars[i] == '=' {
                i += 1;
                let value: String;
                if i < chars.len() && chars[i] == '"' {
                    i += 1;
                    let vs = i;
                    while i < chars.len() && chars[i] != '"' {
                        i += 1;
                    }
                    if i >= chars.len() {
                        return Err(format!("unterminated quote in the value of `{key}`"));
                    }
                    value = chars[vs..i].iter().collect();
                    i += 1;
                } else {
                    let vs = i;
                    while i < chars.len() && !chars[i].is_whitespace() {
                        i += 1;
                    }
                    value = chars[vs..i].iter().collect();
                }
                items.push((key, Some(value)));
            } else {
                items.push((key, None));
            }
        }
        Ok(Attrs(items))
    }

    fn take(&mut self, key: &str) -> Option<Option<String>> {
        let pos = self.0.iter().position(|(k, _)| k == key)?;
        Some(self.0.remove(pos).1)
    }

    fn value(&mut self, key: &str) -> Option<String> {
        self.take(key).flatten()
    }

    /// Present as a bare flag, or `key=true`; `key=false` / `0` / `no` turn it off.
    fn flag(&mut self, key: &str) -> bool {
        self.flag_or(key, false)
    }

    fn flag_or(&mut self, key: &str, default: bool) -> bool {
        match self.take(key) {
            None => default,
            Some(None) => true,
            Some(Some(v)) => !matches!(v.as_str(), "false" | "0" | "no"),
        }
    }

    /// Take all bare words (`::show{mika happy}` -> ["mika", "happy"]) in order.
    fn take_bare(&mut self) -> Vec<String> {
        let mut bare = Vec::new();
        self.0.retain(|(k, v)| {
            if v.is_none() {
                bare.push(k.clone());
                false
            } else {
                true
            }
        });
        bare
    }

    fn leftover(&self) -> Option<&str> {
        self.0.first().map(|(k, _)| k.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DEMO: &str = include_str!("../../../examples/demo.quill.md");

    fn ok(src: &str) -> Program {
        parse(src).unwrap_or_else(|e| panic!("unexpected errors: {e:?}"))
    }

    fn errs(src: &str) -> Vec<ParseError> {
        parse(src).expect_err("expected parse errors")
    }

    fn has_err(src: &str, needle: &str) -> bool {
        errs(src).iter().any(|e| e.message.contains(needle))
    }

    fn say(speaker: Option<&str>, text: &str) -> Op {
        Op::Say { speaker: speaker.map(str::to_string), color: None, text: text.into() }
    }

    fn int(n: i64) -> Expr {
        Expr::Lit(Value::Int(n))
    }

    const MIKA: &str = "::character{id=mika name=Mika color=#ffd98c sprites=art/mika_{pose}.png}\n";

    // ---- the demo ------------------------------------------------------

    #[test]
    fn animation_commands() {
        let p = ok("::bg{src=b.png fade=1 wipe=right}\n::move{mika scale=1.2 ease=out}\n::shake\n::shake{time=1 strength=40}\n::camera{zoom=1.5 x=30% time=2 ease=linear}\n::camera{reset time=1}\n");
        assert_eq!(p.ops[0], Op::Bg { src: "b.png".into(), fade: 1.0, wipe: Some(Wipe::Right) });
        assert_eq!(p.ops[1], Op::Move { id: "mika".into(), at: None, scale: Some(1.2), time: 0.5, ease: Ease::Out });
        assert_eq!(p.ops[2], Op::Shake { time: 0.4, strength: 20.0 });
        assert_eq!(p.ops[3], Op::Shake { time: 1.0, strength: 40.0 });
        assert_eq!(
            p.ops[4],
            Op::Camera { zoom: Some(1.5), x: Some(0.3), y: None, time: 2.0, ease: Ease::Linear }
        );
        assert_eq!(
            p.ops[5],
            Op::Camera { zoom: Some(1.0), x: Some(0.5), y: Some(0.5), time: 1.0, ease: Ease::InOut }
        );
    }

    #[test]
    fn animation_errors() {
        for (src, needle) in [
            ("::bg{src=b.png wipe=sideways fade=1}\n", "wipe=sideways"),
            ("::bg{src=b.png wipe=left}\n", "needs a duration"),
            ("::move{mika}\n", "`at=...` and/or `scale=...`"),
            ("::move{mika at=left ease=bouncy}\n", "ease=bouncy"),
            ("::move{mika scale=0}\n", "scale=0"),
            ("::camera{time=1}\n", "needs `zoom=`"),
            ("::camera{zoom=0.5}\n", "zoom=0.5"),
            ("::camera{x=2}\n", "x=2"),
            ("::shake{strength=-1}\n", "strength=-1"),
        ] {
            let e = errs(src);
            assert!(e[0].message.contains(needle), "{src}: {}", e[0].message);
        }
    }

    #[test]
    fn front_matter_can_name_a_theme() {
        assert_eq!(ok("---\ntheme: gui/theme.txt\n---\nHi.\n").theme.as_deref(), Some("gui/theme.txt"));
        assert_eq!(ok("---\ntheme: \"my theme.txt\"\n---\nHi.\n").theme.as_deref(), Some("my theme.txt"));
        assert_eq!(ok("Hi.\n").theme, None);
        assert_eq!(errs("---\ntheme:\n---\nHi.\n")[0].line, 2);
    }

    #[test]
    fn parses_the_demo_script() {
        let p = ok(DEMO);
        assert_eq!(p.title.as_deref(), Some("Quillstone Demo"));
        assert_eq!(p.vars.get("trust"), Some(&Value::Int(0)));
        assert_eq!(p.vars.get("met"), Some(&Value::Bool(false)));
        assert!(p.characters.contains_key("mika"));
        for label in ["rooftop", "remember", "truth", "lie", "everything", "ending"] {
            assert!(p.labels.contains_key(label), "missing label {label}");
        }
        assert_eq!(p.ops.len(), p.lines.len());
        assert_eq!(p.ops.last(), Some(&Op::End));
    }

    // ---- prose ---------------------------------------------------------

    #[test]
    fn dialogue_and_narration() {
        let p = ok("**Mika:** Hello\nthere.\n\nThe wind blows.\n");
        assert_eq!(p.ops[0], say(Some("Mika"), "Hello there."));
        assert_eq!(p.ops[1], say(None, "The wind blows."));
    }

    #[test]
    fn consecutive_speakers_split_without_blank_line() {
        let p = ok("**A:** one\n**B:** two\n");
        assert!(matches!(&p.ops[0], Op::Say { speaker: Some(s), .. } if s == "A"));
        assert!(matches!(&p.ops[1], Op::Say { speaker: Some(s), .. } if s == "B"));
    }

    #[test]
    fn comments_are_ignored() {
        assert_eq!(ok("<!-- note -->\n**A:** hi\n").ops.len(), 2);
    }

    #[test]
    fn multi_line_comments_are_ignored_too() {
        let p = ok("<!-- first line\n**A:** not dialogue\n  last line -->\n**A:** real\n");
        assert_eq!(p.ops, vec![say(Some("A"), "real"), Op::End]);
        assert_eq!(p.lines[0], 4); // line numbers still count the comment's lines
    }

    #[test]
    fn comment_mistakes_are_reported() {
        assert!(has_err("<!-- never closed\n**A:** hi\n", "never closed"));
        assert!(has_err("<!-- a -->**A:** hi\n", "nothing may follow"));
        assert!(has_err("<!-- a\nb --> trailing\n", "nothing may follow"));
    }

    #[test]
    fn markdown_image_syntax_picks_image_or_video() {
        let p = ok("![scarf](scarf.png)\n![](intro.webm)\n");
        assert!(matches!(&p.ops[0], Op::Image { src, alt } if src == "scarf.png" && alt == "scarf"));
        assert!(matches!(&p.ops[1], Op::Video { src, .. } if src == "intro.webm"));
    }

    // ---- media commands --------------------------------------------------

    #[test]
    fn background_music_video_attributes() {
        let p = ok("::bg{src=a.webp fade=0.5}\n::video{src=\"my clip.webm\" fit=cover loop skippable}\n::music{stop fade=2}\n");
        assert_eq!(p.ops[0], Op::Bg { src: "a.webp".into(), fade: 0.5, wipe: None });
        assert_eq!(
            p.ops[1],
            Op::Video { src: "my clip.webm".into(), fit: Fit::Cover, looped: true, skippable: true }
        );
        assert_eq!(p.ops[2], Op::StopMusic { fade: 2.0 });
    }

    #[test]
    fn music_loops_by_default_and_takes_a_volume() {
        let p = ok("::music{src=a.ogg}\n::music{src=b.ogg loop=false volume=0.4 fade=1}\n");
        assert_eq!(p.ops[0], Op::Music { src: "a.ogg".into(), fade: 0.0, looped: true, volume: 1.0 });
        assert_eq!(p.ops[1], Op::Music { src: "b.ogg".into(), fade: 1.0, looped: false, volume: 0.4 });
    }

    #[test]
    fn sfx_voice_and_wait() {
        let p = ok("::sfx{src=door.ogg volume=0.5}\n::voice{src=v1.ogg}\n::wait{time=1.5}\n::wait{2}\n");
        assert_eq!(p.ops[0], Op::Sfx { src: "door.ogg".into(), volume: 0.5 });
        assert_eq!(p.ops[1], Op::Voice { src: "v1.ogg".into() });
        assert_eq!(p.ops[2], Op::Wait { seconds: 1.5 });
        assert_eq!(p.ops[3], Op::Wait { seconds: 2.0 });
    }

    #[test]
    fn media_argument_errors() {
        assert!(has_err("::wait{time=0}\n", "positive number"));
        assert!(has_err("::wait\n", "positive number"));
        assert!(has_err("::sfx{src=a.ogg volume=3}\n", "between 0 and 1"));
        assert!(has_err("::music{src=a.ogg fade=-1}\n", "seconds"));
        assert!(has_err("::sfx{}\n", "needs `src=...`"));
    }

    // ---- characters and sprites ------------------------------------------

    #[test]
    fn character_declaration_and_speaker_resolution() {
        let p = ok(&format!("{MIKA}**mika:** hi\n**Someone:** yo\n"));
        let c = &p.characters["mika"];
        assert_eq!((c.name.as_str(), c.sprites.as_deref()), ("Mika", Some("art/mika_{pose}.png")));
        let col = c.color.unwrap();
        assert!((col[0] - 1.0).abs() < 1e-6 && (col[1] - 0xd9 as f32 / 255.0).abs() < 1e-6);
        // speaker names are matched case-insensitively and normalised to the display name
        assert_eq!(p.ops[0], Op::Say { speaker: Some("Mika".into()), color: c.color, text: "hi".into() });
        assert_eq!(p.ops[1], say(Some("Someone"), "yo"));
    }

    #[test]
    fn characters_may_be_matched_by_display_name_too() {
        let p = ok("::character{id=m name=\"Mika Sato\"}\n**Mika Sato:** hi\n");
        assert!(matches!(&p.ops[0], Op::Say { speaker: Some(s), .. } if s == "Mika Sato"));
    }

    #[test]
    fn show_with_character_pose_and_position() {
        let p = ok(&format!("{MIKA}::show{{mika happy at=left fade=0.4 scale=0.9}}\n::show{{mika}}\n"));
        assert_eq!(
            p.ops[0],
            Op::Show { id: "mika".into(), src: "art/mika_happy.png".into(), at: Some(0.28), scale: 0.9, fade: 0.4 }
        );
        // default pose, keeps position
        assert_eq!(
            p.ops[1],
            Op::Show { id: "mika".into(), src: "art/mika_default.png".into(), at: None, scale: 1.0, fade: 0.0 }
        );
    }

    #[test]
    fn show_with_an_explicit_file_and_numeric_positions() {
        let p = ok("::show{src=art/ghost.png id=ghost at=0.3}\n::show{src=art/x.png at=25%}\n");
        assert_eq!(
            p.ops[0],
            Op::Show { id: "ghost".into(), src: "art/ghost.png".into(), at: Some(0.3), scale: 1.0, fade: 0.0 }
        );
        assert!(matches!(&p.ops[1], Op::Show { id, at: Some(a), .. } if id == "art/x.png" && (a - 0.25).abs() < 1e-6));
    }

    #[test]
    fn hide_and_move() {
        let p = ok(&format!("{MIKA}::hide{{mika fade=0.5}}\n::hide{{all}}\n::move{{mika at=right time=1}}\n::move{{mika at=0.1}}\n"));
        assert_eq!(p.ops[0], Op::Hide { id: Some("mika".into()), fade: 0.5 });
        assert_eq!(p.ops[1], Op::Hide { id: None, fade: 0.0 });
        assert_eq!(p.ops[2], Op::Move { id: "mika".into(), at: Some(0.72), scale: None, time: 1.0, ease: Ease::InOut });
        assert_eq!(p.ops[3], Op::Move { id: "mika".into(), at: Some(0.1), scale: None, time: 0.5, ease: Ease::InOut });
    }

    #[test]
    fn sprite_errors() {
        assert!(has_err("::show{nobody}\n", "unknown character `nobody`"));
        assert!(has_err("::character{id=a}\n::show{a}\n", "no `sprites="));
        assert!(has_err("::show\n", "needs a character"));
        assert!(has_err("::show{src=a.png at=sideways}\n", "bad position"));
        assert!(has_err("::show{src=a.png scale=0}\n", "above 0"));
        assert!(has_err("::hide\n", "needs a sprite"));
        assert!(has_err("::move{a}\n", "needs `at="));
        assert!(has_err("::character{name=x}\n", "needs `id="));
        assert!(has_err("::character{id=a color=red}\n", "bad colour"));
        assert!(has_err("::character{id=a}\n::character{id=A}\n", "declared twice"));
        assert!(has_err("::character{id=\"a b\"}\n", "letters, digits"));
    }

    // ---- variables ---------------------------------------------------------

    #[test]
    fn front_matter_values_have_types() {
        let p = ok("---\nvars: { n: 3, ok: true, name: \"Alex, the 1st\", neg: -2 }\n---\n");
        assert_eq!(p.vars["n"], Value::Int(3));
        assert_eq!(p.vars["ok"], Value::Bool(true));
        assert_eq!(p.vars["name"], Value::Str("Alex, the 1st".into()));
        assert_eq!(p.vars["neg"], Value::Int(-2));
    }

    #[test]
    fn set_command_emits_one_op_per_assignment() {
        let p = ok("::set{a=1, b+=2, name=\"x, y\", ok=true}\n");
        assert_eq!(p.ops.len(), 5); // four sets + End
        let Op::Set(e) = &p.ops[2] else { panic!() };
        assert_eq!(e.value, Expr::Lit(Value::Str("x, y".into())));
        let Op::Set(e) = &p.ops[1] else { panic!() };
        assert_eq!((e.var.as_str(), e.op), ("b", AssignOp::Add));
    }

    // ---- control flow ------------------------------------------------------

    #[test]
    fn jump_conditions_use_full_expressions() {
        let p = ok("# a\n::jump{to=a if=\"trust >= 2 and not seen\"}\n::jump{to=a if=seen}\n::jump{a}\n");
        let Op::Jump { target, cond } = &p.ops[0] else { panic!() };
        assert_eq!(*target, 0);
        assert_eq!(
            cond,
            &Some(Expr::And(
                Box::new(Expr::Cmp(Box::new(Expr::Var("trust".into())), Cmp::Ge, Box::new(int(2)))),
                Box::new(Expr::Not(Box::new(Expr::Var("seen".into())))),
            ))
        );
        let Op::Jump { cond, .. } = &p.ops[1] else { panic!() };
        assert_eq!(cond, &Some(Expr::Var("seen".into())));
        assert!(matches!(&p.ops[2], Op::Jump { target: 0, cond: None }));
    }

    #[test]
    fn call_and_return() {
        let p = ok("::call{to=sub}\n**A:** back\n::end\n# sub\n**A:** inside\n::return\n");
        assert_eq!(p.ops[0], Op::Call { target: p.labels["sub"] });
        assert!(p.ops.contains(&Op::Return));
        assert!(has_err("::call{to=nowhere}\n", "unknown label `nowhere`"));
        assert!(has_err("::call\n", "needs a label"));
        assert!(has_err("::return{x}\n", "takes no attributes"));
    }

    #[test]
    fn if_else_lowers_to_jumps() {
        // ops: 0 jump-unless(c) -> 3 | 1 say A | 2 jump -> 4 | 3 say B | 4 say C | 5 End
        let p = ok("::if{c}\n**X:** A\n::else\n**X:** B\n::endif\n**X:** C\n");
        assert_eq!(
            p.ops[0],
            Op::Jump { target: 3, cond: Some(Expr::Not(Box::new(Expr::Var("c".into())))) }
        );
        assert!(matches!(&p.ops[2], Op::Jump { target: 4, cond: None }));
        assert_eq!(p.ops.len(), 6);
    }

    #[test]
    fn if_without_else_jumps_past_the_block() {
        let p = ok("::if{c}\n**X:** A\n::endif\n**X:** B\n");
        assert!(matches!(&p.ops[0], Op::Jump { target: 2, .. }));
    }

    #[test]
    fn elif_chains() {
        // 0 jmp-unless a ->3 | 1 say 1 | 2 jmp -> 7 | 3 jmp-unless b ->6 | 4 say 2 | 5 jmp -> 7 ... else
        let p = ok("::if{a}\n**X:** 1\n::elif{b}\n**X:** 2\n::else\n**X:** 3\n::endif\n**X:** 4\n");
        let jump_targets: Vec<usize> = p
            .ops
            .iter()
            .filter_map(|op| match op {
                Op::Jump { target, .. } => Some(*target),
                _ => None,
            })
            .collect();
        // every jump lands on a real op and the end jumps all go to the same place (the `4` line)
        assert!(jump_targets.iter().all(|t| *t < p.ops.len()));
        let after = p.ops.iter().position(|o| matches!(o, Op::Say { text, .. } if text == "4")).unwrap();
        assert_eq!(jump_targets.iter().filter(|t| **t == after).count(), 2);
    }

    #[test]
    fn nested_ifs() {
        let p = ok("::if{a}\n::if{b}\n**X:** in\n::endif\n::endif\n**X:** out\n");
        // both condition jumps skip to the line after their own ::endif
        let out = p.ops.iter().position(|o| matches!(o, Op::Say { text, .. } if text == "out")).unwrap();
        assert!(matches!(&p.ops[0], Op::Jump { target, .. } if *target == out));
        assert!(matches!(&p.ops[1], Op::Jump { target, .. } if *target == out));
    }

    #[test]
    fn block_structure_errors() {
        assert!(has_err("::if{a}\n**X:** hi\n", "never closed"));
        assert!(has_err("::endif\n", "without a matching `::if`"));
        assert!(has_err("::else\n", "without a matching `::if`"));
        assert!(has_err("::elif{a}\n", "without a matching `::if`"));
        assert!(has_err("::if{a}\n::else\n::else\n::endif\n", "already has an `::else`"));
        assert!(has_err("::if{a}\n::else\n::elif{b}\n::endif\n", "after `::else`"));
        assert!(has_err("::if{a >}\n::endif\n", "ends too early"));
        // a bad condition reports once and doesn't cascade into "never closed"
        assert_eq!(errs("::if{a >}\n::endif\n").len(), 1);
    }

    // ---- choices -----------------------------------------------------------

    #[test]
    fn choices_resolve_labels_effects_and_conditions() {
        let p = ok("::choice\n- [Yes](#Say Yes){trust+=2, met=1}\n- [No](#no)\n- [Secret](#no){if met and trust > 1, x=\"a, b\"}\n::\n# say yes\n**A:** y\n# no\n**A:** n\n");
        let Op::Choice { options } = &p.ops[0] else { panic!("not a choice") };
        assert_eq!(options.len(), 3);
        assert_eq!(options[0].target, p.labels["say-yes"]);
        assert_eq!(options[1].target, p.labels["no"]);
        assert_eq!(options[0].effects.len(), 2);
        assert_eq!(options[0].cond, None);
        assert_eq!(options[2].effects.len(), 1);
        assert!(options[2].cond.is_some());
    }

    #[test]
    fn choice_errors() {
        assert!(has_err("::choice\n- [x](#y)\n", "never closed"));
        assert!(has_err("::choice\n::\n", "has no options"));
        assert!(has_err("# a\n::choice\n- [x](#a){if }\n::\n", "found nothing"));
        assert!(has_err("# a\n::choice\n- [x](#a){bogus}\n::\n", "bad assignment"));
        // a variable that merely starts with "if" is still an assignment
        let p = ok("# a\n::choice\n- [x](#a){iffy+=1}\n::\n");
        let Op::Choice { options } = &p.ops[0] else { panic!() };
        assert_eq!((options[0].effects.len(), options[0].cond.is_none()), (1, true));
    }

    // ---- errors ------------------------------------------------------------

    #[test]
    fn reports_unknown_label_with_line_number() {
        let e = errs("# start\n**A:** hi\n::jump{to=nowhere}\n");
        assert_eq!(e.len(), 1);
        assert_eq!(e[0].line, 3);
        assert!(e[0].message.contains("unknown label `nowhere`"));
    }

    #[test]
    fn reports_many_errors_at_once() {
        let e = errs("::bogus\n::bg{fade=1}\n::video{src=a.webm fad=1}\n::choice\n- [x](#y)\n");
        let lines: Vec<usize> = e.iter().map(|e| e.line).collect();
        for expected in 1..=4 {
            assert!(lines.contains(&expected), "{e:?}");
        }
    }

    #[test]
    fn unknown_command_lists_the_known_ones() {
        assert!(has_err("::dance\n", "known: character"));
    }

    #[test]
    fn duplicate_labels_and_bad_front_matter() {
        let e = errs("---\nvars: { x: nope }\nwhat: 1\n---\n# a\n# a\n");
        assert!(e.iter().any(|e| e.message.contains("must be a number")));
        assert!(e.iter().any(|e| e.message.contains("unknown front matter key")));
        assert!(e.iter().any(|e| e.message.contains("duplicate label")));
    }

    #[test]
    fn unclosed_front_matter() {
        assert_eq!(errs("---\ntitle: x\n")[0].line, 1);
    }

    #[test]
    fn seen_needs_a_real_label_and_marks_it_for_counting() {
        let p = ok("# start\n::jump{to=start if=\"not seen(The End)\"}\n# the end\nBye.\n");
        assert_eq!(p.counted.get(&p.labels["the-end"]), Some(&vec!["seen:the-end".to_string()]));
        assert!(has_err("# a\n::if{seen(nowhere)}\nX.\n::endif\n", "no label `nowhere`"));
    }

    #[test]
    fn once_options_are_offered_until_picked() {
        let p = ok("# a\n::choice\n- [Ask](#a){once, trust+=1, if trust < 5}\n::\n");
        let Op::Choice { options } = &p.ops[0] else { panic!() };
        let (picked, mut vars) = ("once:a:Ask", Vars::new());
        assert_eq!(options[0].effects.len(), 2, "its own effect, then the remembering one");
        assert!(options[0].cond.as_ref().unwrap().test(&vars));
        for e in &options[0].effects {
            e.apply(&mut vars);
        }
        assert_eq!(vars[picked], Value::Bool(true));
        assert!(!options[0].cond.as_ref().unwrap().test(&vars), "gone once picked");
    }

    #[test]
    fn front_matter_declares_persistent_variables_and_extras() {
        let p = ok("---\nvars: { trust: 0 }\npersistent: { good_end: false, runs: 0 }\ngallery: [a.png, \"b c.png\"]\ngallery: [d.png, a.png]\nmusic_room: [calm.ogg]\n---\nHi.\n");
        assert_eq!(p.persistent.iter().collect::<Vec<_>>(), ["good_end", "runs"]);
        assert_eq!(p.vars["good_end"], Value::Bool(false), "starting values live with the others");
        assert_eq!(p.gallery, ["a.png", "b c.png", "d.png"]);
        assert_eq!(p.music_room, ["calm.ogg"]);
        assert_eq!(p.extra_assets().count(), 4);
        assert!(has_err("---\nvars: { x: 0 }\npersistent: { x: 1 }\n---\nHi.\n", "both `vars` and `persistent`"));
        assert!(has_err("---\ngallery: a.png\n---\nHi.\n", "must look like `["));
        assert_eq!(ok("---\nvoice: audio/voice/\n---\nHi.\n").voice_folder.as_deref(), Some("audio/voice"));
    }

    #[test]
    fn text_tags_are_checked_where_they_are_written() {
        ok("Some **bold**, *italic*, {color=#f80}colour{/color} and a pause{w=0.3}.\n::choice\n- [*Quietly*](#a)\n::\n# a\nX.\n");
        let e = errs("A line.\nAn **unclosed line.\n\nNext.\n");
        assert_eq!((e[0].line, e[0].message.contains("bold")), (1, true), "the paragraph's first line");
        assert!(has_err("::choice\n- [{size=9}x](#a)\n::\n# a\nX.\n", "unknown text tag"));
    }

    #[test]
    fn slug_normalises_headings() {
        assert_eq!(slug("The Roof!"), "the-roof");
        assert_eq!(slug("  Chapter  2 – Start "), "chapter-2-start");
    }

    #[test]
    fn colours_and_positions() {
        assert_eq!(parse_color("#fff").unwrap(), [1.0, 1.0, 1.0]);
        assert_eq!(parse_color("#000000").unwrap(), [0.0, 0.0, 0.0]);
        assert!(parse_color("fff").is_err() && parse_color("#ffff").is_err() && parse_color("#ggg").is_err());
        assert_eq!(parse_position("center").unwrap(), 0.5);
        assert_eq!(parse_position("50%").unwrap(), 0.5);
        assert!(parse_position("150%").is_err() && parse_position("-1").is_err());
    }
}
