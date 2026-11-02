//! Line-based parser/compiler: markdown-flavoured script text -> [`Program`].
//!
//! Line kinds:
//! - `# heading`              label (any heading level)
//! - `**Name:** text`         dialogue; other prose is narration
//! - `![alt](path)`           inline image, or video if the extension says so
//! - `::name{key=value flag}` command, one line (see SCRIPT.md for the full list)
//! - `::choice` ... `::`      choice block with `- [text](#label){effects, if cond, default}` options (`::choice{time=8}` is timed)
//! - `::if{c}` `::elif{c}` `::else` `::endif`   conditional blocks, lowered to jumps
//! - `<!-- comment -->`       ignored (single line)
//!
//! Consecutive prose lines are joined into one paragraph. Errors are collected
//! (not just the first) and carry 1-based line numbers.

use std::collections::BTreeMap;
use std::fmt;
use std::ops::RangeInclusive;

use crate::dynpath::{is_device_name, is_dynamic};
use crate::expr::{is_ident, is_keyword, is_plain_ident, parse_effect, parse_expr, parse_list, parse_value, split_top_level};
use crate::ops::*;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseError {
    /// The line within `file` (or within the main script when `file` is `None`).
    pub line: usize,
    /// The included file the error is in (its game-folder path); `None` for the main script.
    pub file: Option<String>,
    pub message: String,
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.file {
            Some(file) => write!(f, "{file}:{}: {}", self.line, self.message),
            None => write!(f, "line {}: {}", self.line, self.message),
        }
    }
}

impl std::error::Error for ParseError {}

/// Compile script source into a [`Program`], or return every error found.
/// `::include` is an error here: a plain string has no game folder (see [`parse_files`]).
pub fn parse(src: &str) -> Result<Program, Vec<ParseError>> {
    parse_inner(src, "", None)
}

/// Reads an included file: gets its game-folder path, returns its text or why it can't be read.
pub type Loader<'a> = &'a dyn Fn(&str) -> Result<String, String>;

/// Like [`parse`], but `::include{path}` reads other script files through `load`, which gets
/// the path as written in the script (relative to the game folder, already checked to be
/// inside it) and returns the file's text or a reason it can't be read. `main` is the main
/// script's own path in the game folder: it names the file in messages and an include of
/// it is a cycle.
pub fn parse_files(src: &str, main: &str, load: Loader) -> Result<Program, Vec<ParseError>> {
    parse_inner(src, main, Some(load))
}

fn parse_inner(src: &str, main: &str, load: Option<Loader>) -> Result<Program, Vec<ParseError>> {
    let mut p = Parser { main: main.to_string(), ..Parser::default() };
    let src = src.strip_prefix('\u{feff}').unwrap_or(src);
    let mut files = Files { load, stack: Vec::new(), seen: std::collections::HashMap::new(), bytes: 0, next_base: src.lines().count() + 1 };
    p.run_file(src, &mut files);
    p.finish()
}

/// How deep includes may nest, how much text they may add up to in all, and how large one may be
/// (scripts can come from shared packages). There is no limit on the number of files: the total
/// size stops a runaway include, and a cycle is refused.
pub const MAX_INCLUDE_DEPTH: usize = 8;
pub const MAX_INCLUDE_BYTES: usize = 64 * 1024 * 1024;
pub const MAX_INCLUDE_FILE_BYTES: usize = 8 * 1024 * 1024;

/// What `::include` needs while one script is read: the loader and what it already read.
struct Files<'a> {
    load: Option<Loader<'a>>,
    /// The files being read right now, outermost first (for cycles and the depth); the main
    /// script is not in it.
    stack: Vec<String>,
    /// Every file included so far: lowercased name -> line of the `::include`, for the "twice" error.
    seen: std::collections::HashMap<String, usize>,
    bytes: usize,
    /// The program line number the next included file's line 0 gets.
    next_base: usize,
}

/// Allowed `scale=`, `xscale=` and `yscale=` of a sprite.
const SPRITE_SCALE: RangeInclusive<f32> = 0.05..=10.0;

/// Allowed `xoffset=` / `yoffset=` of a sprite, in pixels of the 1920x1080 stage (two stage widths either way).
pub const SPRITE_OFFSET: RangeInclusive<f32> = -3840.0..=3840.0;

/// Most steps (`then`) one `::move` may have.
pub const MAX_MOVE_STEPS: usize = 32;

const COMMANDS: &str = "character, anim, bg, show, hide, move, shake, flash, notify, unlock, fade, camera, particles, tint, textbox, hud, music, sfx, voice, wait, video, \
                        choice, hotspots, item, inventory, journal, shop, input, timing, press, include, jump, call, return, set, if, elif, else, endif, end, end_replay";

/// The attributes each command takes (for error messages).
fn known_attrs(command: &str) -> &'static [&'static str] {
    match command {
        "anim" => &["id", "frames", "loop"],
        "character" => &["id", "name", "color", "sprites", "layers", "layer_sprites"],
        "item" => &["id", "name", "icon", "price", "text", "count", "use"],
        "shop" => &["pay", "into"],
        "choice" => &["time"],
        "hotspots" => &["time", "default", "ground", "hover"],
        "bg" => &["src", "color", "anim", "fade", "wipe", "through"],
        "hud" => &["id", "text", "bar", "max", "image", "x", "y", "size", "color", "back", "pad", "border", "border_size", "scale", "if", "fade", "hide", "clear", "call", "open", "hover"],
        "show" => &["src", "anim", "id", "x", "y", "z", "scale", "flip", "xscale", "yscale", "alpha", "xoffset", "yoffset", "fade"],
        "hide" => &["all", "fade"],
        "move" => &["x", "y", "scale", "flip", "xscale", "yscale", "alpha", "xoffset", "yoffset", "time", "ease", "wait", "loop", "then"],
        "shake" => &["time", "strength", "axis"],
        "flash" => &["time", "color"],
        "notify" => &["text", "time"],
        "unlock" => &["src"],
        "fade" => &["out", "in", "time", "color"],
        "camera" => &["zoom", "x", "y", "time", "ease", "reset"],
        "particles" => &["snow", "rain", "none", "off", "src", "amount", "speed", "wind", "fade", "layer"],
        "tint" => &["none", "off", "color", "alpha", "fade"],
        "textbox" => &["hide", "show", "fade"],
        "music" => &["src", "intro", "fade", "loop", "volume", "stop"],
        "sfx" => &["src", "loop", "stop", "volume", "fade"],
        "voice" => &["src", "volume"],
        "wait" => &["time", "click"],
        "video" => &["src", "fit", "loop", "skippable"],
        "jump" => &["to", "if"],
        "input" => &["var", "prompt", "default", "max"],
        "timing" => &["result", "speed", "good", "perfect", "time", "prompt"],
        "press" => &["key", "time", "count", "result", "prompt"],
        _ => &[],
    }
}

/// The error for an attribute `::name` doesn't take, listing the ones it does.
fn unknown_attr(name: &str, extra: &str) -> String {
    let known = known_attrs(name);
    let hint = if known.is_empty() { String::new() } else { format!(" (it takes: {})", known.join(", ")) };
    format!("unknown attribute `{extra}` for `::{name}`{hint}")
}

/// How many unknown names get close-name suggestions: each costs a pass over every name, so a script with
/// thousands of typos would take quadratic time. The rest are just told how many exist.
const MAX_SUGGESTED: usize = 20;

/// [`suggest`] for the `nth` unknown label (counting from 0).
fn label_hint(program: &Program, name: &str, nth: usize) -> String {
    if nth < MAX_SUGGESTED {
        return suggest(name, program.labels.keys().map(String::as_str), "label");
    }
    let n = program.labels.len();
    format!("{n} label{} exist{}", if n == 1 { "" } else { "s" }, if n == 1 { "s" } else { "" })
}

/// Why `name` isn't one of `known`: at most 5 close names (a prefix or part of one, or a few typos
/// away), else just how many there are, so a big game doesn't get thousands of names in a message.
fn suggest<'a>(name: &str, known: impl Iterator<Item = &'a str>, what: &str) -> String {
    let (mut total, mut close) = (0, Vec::new());
    for k in known {
        total += 1;
        let (n, kn) = (name.chars().count(), k.chars().count());
        let limit = n.max(kn) / 3;
        // names whose lengths differ by more than the limit can't be within it: no need to measure
        let d = (n.abs_diff(kn) <= limit).then(|| edit_distance(name, k));
        if d.is_some_and(|d| d <= limit) || (name.len() >= 3 && (k.contains(name) || name.contains(k))) {
            close.push((d.unwrap_or_else(|| edit_distance(name, k)), k));
        }
    }
    close.sort();
    let names: Vec<String> = close.iter().take(5).map(|(_, k)| format!("`{k}`")).collect();
    let plural = if total == 1 { "" } else { "s" };
    match (total, names.is_empty()) {
        (0, _) => format!("there are no {what}s yet"),
        (_, true) => format!("{total} {what}{plural} exist{}, none close to it", if total == 1 { "s" } else { "" }),
        _ => format!("did you mean {}? {total} {what}{plural} in all", names.join(", ")),
    }
}

/// Number of single-character edits between two names (Levenshtein).
fn edit_distance(a: &str, b: &str) -> usize {
    let b: Vec<char> = b.chars().collect();
    let mut row: Vec<usize> = (0..=b.len()).collect();
    for (i, ca) in a.chars().enumerate() {
        let mut diag = std::mem::replace(&mut row[0], i + 1);
        for (j, cb) in b.iter().enumerate() {
            let up = row[j + 1];
            row[j + 1] = (diag + usize::from(ca != *cb)).min(up + 1).min(row[j] + 1);
            diag = up;
        }
    }
    row[b.len()]
}

#[derive(Default)]
struct Parser {
    program: Program,
    errors: Vec<ParseError>,
    fixups: Vec<Fixup>,
    blocks: Vec<IfBlock>,
    /// Labels with parameters and the line of their heading (for the fall-through check).
    param_labels: Vec<(String, usize)>,
    /// `use=#label` of each item, to check once all labels are known: (label, line).
    item_uses: Vec<(String, usize)>,
    /// `call=#label` of each HUD button, checked the same way: (label, line).
    hud_calls: Vec<(String, usize)>,
    /// `sprite=id` of each hotspot, checked against the sprites the script shows: (id, line).
    sprite_refs: Vec<(String, usize)>,
    /// `::sfx{stop=file}` targets, checked against the files the script plays.
    sfx_stops: Vec<(String, usize)>,
    /// `replays:` entries (line, label, title), checked once all labels are known.
    replay_decls: Vec<(usize, String, String)>,
    /// Ops a command adds right after its own (the fade back in of `::bg{through}`).
    trailing: Vec<Op>,
    /// Characters an earlier `::show` gave layers to (a later one may then leave layers out).
    layered_seen: std::collections::BTreeSet<String>,
    /// The main script's path as given to `parse_files` (empty for plain `parse`).
    main: String,
    /// Added to a line number of the file being read to make its program line number
    /// (0 for the main script).
    base: usize,
    /// The first line of every label, for the duplicate-label message.
    label_lines: BTreeMap<String, usize>,
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
        self.errors.push(ParseError { line, file: None, message: message.into() });
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

    /// Read one script (the main one, or an included file whose lines are numbered from `self.base`).
    fn run_file(&mut self, src: &str, files: &mut Files) {
        let lines: Vec<&str> = src.lines().collect();
        let mut i = 0;

        if self.base == 0 && lines.first().map(|l| l.trim()) == Some("---") {
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
            let line_no = self.base + i + 1;
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
                            self.err(self.base + end + 1, "nothing may follow `-->` on a comment's last line");
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
            } else if let Some(inner) = include_args(t) {
                self.flush(&mut para);
                self.include(line_no, inner, files);
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

        let fixups = std::mem::take(&mut self.fixups);
        let user_jumps: std::collections::HashSet<usize> = fixups.iter().map(|f| f.op).collect();
        self.program.block_jumps = (0..self.program.ops.len())
            .filter(|i| matches!(self.program.ops[*i], Op::Jump { .. }) && !user_jumps.contains(i))
            .collect();
        let mut typos = 0;
        for f in fixups {
            match self.program.labels.get(&f.label).copied() {
                Some(target) => {
                    let params = self.program.params.get(&f.label).cloned();
                    let defaults = self.program.param_defaults.get(&f.label).cloned().unwrap_or_default();
                    let mut problems = Vec::new();
                    match &mut self.program.ops[f.op] {
                        Op::Call { target: t, args } => {
                            *t = target;
                            problems = call_problems(&f.label, params.as_deref(), &defaults, args);
                            // a parameter left out takes its default: the call passes it like any other
                            for (k, v) in defaults {
                                if !args.iter().any(|(a, _)| *a == k) {
                                    args.push((k, Expr::Lit(v)));
                                }
                            }
                        }
                        Op::Jump { target: t, .. } => {
                            *t = target;
                            problems.extend(needs_call(&f.label, params.as_deref(), "`::jump`"));
                        }
                        Op::Choice { options, .. } => {
                            if let Some(k) = f.option {
                                options[k].target = target;
                            }
                            problems.extend(needs_call(&f.label, params.as_deref(), "a choice or hotspot link"));
                        }
                        _ => {}
                    }
                    for m in problems {
                        self.err(f.line, m);
                    }
                }
                None => {
                    let hint = label_hint(&self.program, &f.label, typos);
                    typos += 1;
                    self.err(f.line, format!("unknown label `{}` ({hint})", f.label));
                }
            }
        }

        for (label, line) in std::mem::take(&mut self.item_uses) {
            if !self.program.labels.contains_key(&label) {
                self.err(line, format!("`use=#{label}`: unknown label `{label}`"));
            } else if self.program.params.contains_key(&label) {
                self.err(line, format!("`use=#{label}`: `{label}` takes parameters, and the Use button can't give them"));
            }
        }

        let shown: std::collections::HashSet<String> =
            self.program.ops.iter().filter_map(|op| if let Op::Show { id, .. } = op { Some(id.clone()) } else { None }).collect();
        for (id, line) in std::mem::take(&mut self.sprite_refs) {
            if !shown.contains(&id) {
                self.err(line, format!("`sprite={id}`: no `::show` ever shows a sprite called `{id}`"));
            }
        }

        for (label, line) in std::mem::take(&mut self.hud_calls) {
            if !self.program.labels.contains_key(&label) {
                self.err(line, format!("`call=#{label}`: unknown label `{label}`"));
            } else if self.program.params.contains_key(&label) {
                self.err(line, format!("`call=#{label}`: `{label}` takes parameters, and a HUD button can't give them"));
            } else {
                self.program.hud_calls.insert(label);
            }
        }

        // A label with parameters is entered only by `::call`: running into it from
        // above (or the start of the script, or an `::if` that skips past it) would
        // leave its parameters unset.
        for (label, line) in std::mem::take(&mut self.param_labels) {
            let at = self.program.labels[&label];
            let falls_in = match at.checked_sub(1).and_then(|p| self.program.ops.get(p)) {
                None => true, // the story starts here
                Some(Op::Jump { cond: None, .. } | Op::Return | Op::End) => false,
                Some(_) => true,
            };
            let skipped_into = self.program.ops.iter().enumerate().any(|(i, op)| {
                matches!(op, Op::Jump { target, .. } if *target == at) && !user_jumps.contains(&i)
            });
            if falls_in || skipped_into {
                self.err(
                    line,
                    format!(
                        "the story can run into `{label}` without a `::call`, so its parameters would be unset: \
                         end the label above it with `::jump`, `::return` or `::end` {PARAM_HINT}"
                    ),
                );
            }
        }

        // `::sfx{stop=file}` must name a file some `::sfx` plays.
        for (src, line) in std::mem::take(&mut self.sfx_stops) {
            let played = self.program.ops.iter().any(|op| matches!(op, Op::Sfx { src: s, .. } | Op::SfxLoop { src: s, .. } if *s == src));
            if !played {
                self.err(line, format!("`::sfx{{stop={src}}}`: no `::sfx{{src={src}}}` in the script plays that file"));
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

        // `replays:`: each label must exist and take no parameters.
        let mut typos = 0;
        for (line, label, title) in std::mem::take(&mut self.replay_decls) {
            match self.program.labels.get(&label).copied() {
                None => {
                    let hint = label_hint(&self.program, &label, typos);
                    typos += 1;
                    self.err(line, format!("`replays`: there is no label `{label}` ({hint})"));
                }
                Some(_) if self.program.params.contains_key(&label) => self.err(
                    line,
                    format!("`replays`: label `{label}` has parameters, so it can't start a scene (a replay starts without a `::call`)"),
                ),
                Some(at) => {
                    self.program.replay_at.entry(at).or_default().push(label.clone());
                    self.program.replays.push(Replay { label, title });
                }
            }
        }
        for (line, label) in unknown {
            self.err(line, format!("`seen({label})`: there is no label `{label}`"));
        }

        if self.errors.is_empty() {
            Ok(self.program)
        } else {
            // Lines are program line numbers until now; give each error its file and own line.
            self.errors.sort_by_key(|e| e.line);
            for e in &mut self.errors {
                if let (Some(file), line) = self.program.locate(e.line) {
                    (e.file, e.line) = (Some(file.to_string()), line);
                }
            }
            Err(self.errors)
        }
    }

    /// `file:line` of a program line number, for messages that point at a second place.
    fn describe(&self, line: usize) -> String {
        match self.program.locate(line) {
            (Some(file), n) => format!("{file}:{n}"),
            (None, n) if self.main.is_empty() => format!("line {n} of the main script"),
            (None, n) => format!("{}:{n}", self.main),
        }
    }

    /// `::include{chapters/ch1.quill.md}`: read that file here, as if its text stood in its place.
    fn include(&mut self, line: usize, inner: &str, files: &mut Files) {
        let Some(path) = include_path(inner) else {
            return self.err(line, "`::include` needs a file: `::include{chapters/ch1.quill.md}`");
        };
        if path.contains(['{', '}']) {
            return self.err(line, format!("`::include{{{path}}}`: braces can't be part of a file name (a `::include` is read when the game is built, so it can't use a variable)"));
        }
        if !is_inside(&path) {
            return self.err(line, format!("`::include{{{path}}}`: a path must be relative and inside the game folder (no `..`, drive letters, backslashes or leading `/`)"));
        }
        let lower = path.to_ascii_lowercase();
        if !lower.ends_with(".quill.md") && !lower.ends_with(".md") {
            return self.err(line, format!("`::include{{{path}}}`: a script file ends in `.quill.md` (or `.md`)"));
        }
        let key = normal_path(&path);
        let same = |a: &String| a.eq_ignore_ascii_case(&key);
        if same(&normal_path(&self.main)) || files.stack.iter().any(same) {
            let mut chain: Vec<&str> = std::iter::once(self.main.as_str()).chain(files.stack.iter().map(String::as_str)).collect();
            chain.retain(|n| !n.is_empty());
            chain.push(&key);
            return self.err(line, format!("`::include{{{path}}}` includes a file that is already being read (a cycle: {})", chain.join(" -> ")));
        }
        if let Some(first) = files.seen.get(&key.to_ascii_lowercase()) {
            let first = self.describe(*first);
            return self.err(line, format!("`{path}` is included twice (first at {first}); each file may be included once, or its labels would be defined twice"));
        }
        if files.stack.len() >= MAX_INCLUDE_DEPTH {
            return self.err(line, format!("`::include` nests more than {MAX_INCLUDE_DEPTH} files deep"));
        }
        let Some(load) = files.load else {
            return self.err(line, "`::include` needs a game folder to read the file from: it can't be used in a script given as plain text");
        };
        files.seen.insert(key.to_ascii_lowercase(), line);
        let text = match load(&key) {
            Ok(t) => t,
            Err(why) => return self.err(line, format!("cannot include `{path}`: {why}")),
        };
        if text.len() > MAX_INCLUDE_FILE_BYTES {
            return self.err(line, format!("`{path}` is larger than {} MB: that is too much script for one file", MAX_INCLUDE_FILE_BYTES >> 20));
        }
        files.bytes = files.bytes.saturating_add(text.len());
        if files.bytes > MAX_INCLUDE_BYTES {
            return self.err(line, format!("the included files add up to more than {} MB: that is too much script", MAX_INCLUDE_BYTES >> 20));
        }
        let text = text.strip_prefix('\u{feff}').unwrap_or(&text);
        let len = text.lines().count();
        let base = files.next_base;
        files.next_base += len + 1;
        self.program.sources.push(SourceSpan { file: key.clone(), base, len });
        if text.lines().next().map(str::trim) == Some("---") {
            self.errors.push(ParseError {
                line: base + 1,
                file: None,
                message: "front matter (title, variables, theme, gallery, ...) belongs in the main script only: an included file has none".into(),
            });
            return;
        }
        let outer = std::mem::replace(&mut self.base, base);
        files.stack.push(key);
        self.run_file(text, files);
        files.stack.pop();
        self.base = outer;
    }

    // ---- front matter -------------------------------------------------

    fn front_matter(&mut self, lines: &[&str]) {
        let mut inventory: Option<(usize, String)> = None;
        let mut journal: Option<(usize, String)> = None;
        let mut journal_done: Option<(usize, String)> = None;
        let (mut pixels, mut art): (Option<usize>, Option<(usize, f32)>) = (None, None);
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
                "id" => match check_game_id(unquote(val)) {
                    Ok(()) => self.program.id = Some(unquote(val).to_string()),
                    Err(m) => self.err(line, m),
                },
                "theme" if !unquote(val).is_empty() => match check_inside("theme", unquote(val)) {
                    Ok(p) => self.program.theme = Some(p),
                    Err(m) => self.err(line, m),
                },
                "theme" => self.err(line, "`theme` needs a file name, like `theme: theme.txt`"),
                "vars" => self.front_vars(line, val, false),
                "persistent" => self.front_vars(line, val, true),
                "gallery" | "music_room" => self.front_list(line, key, val),
                "replays" => self.front_replays(line, val),
                "languages" => self.front_languages(line, val),
                "inventory" => match unquote(val) {
                    name if is_ident(name) => inventory = Some((line, name.to_string())),
                    _ => self.err(line, "`inventory` names a list variable, like `inventory: bag`"),
                },
                "journal" => match unquote(val) {
                    name if is_ident(name) => journal = Some((line, name.to_string())),
                    _ => self.err(line, "`journal` names a list variable, like `journal: quests`"),
                },
                "journal_done" => match unquote(val) {
                    name if is_ident(name) => journal_done = Some((line, name.to_string())),
                    _ => self.err(line, "`journal_done` names a list variable, like `journal_done: quests_done`"),
                },
                "voice" => match unquote(val).trim_end_matches(['/', '\\']) {
                    "" => self.err(line, "`voice` needs a folder, like `voice: voice`"),
                    folder => self.program.voice_folder = Some(folder.to_string()),
                },
                "sprite_size" => match unquote(val) {
                    "pixels" => pixels = Some(line),
                    "fixed" => pixels = None,
                    _ => self.err(line, "`sprite_size` is `fixed` (the default) or `pixels`"),
                },
                "art_scale" => match unquote(val).parse::<f32>() {
                    Ok(k) if (0.25..=8.0).contains(&k) => art = Some((line, k)),
                    _ => self.err(line, format!("`art_scale: {val}` must be a number from 0.25 to 8")),
                },
                other => self.err(line, format!("unknown front matter key `{other}`")),
            }
        }
        match (pixels, art) {
            (Some(_), art) => self.program.art_scale = Some(art.map_or(1.0, |a| a.1)),
            (None, Some((line, _))) => self.err(line, "`art_scale` only works with `sprite_size: pixels`"),
            (None, None) => {}
        }
        // `vars:` may come after it, so the list is checked once everything is read.
        if let Some((line, name)) = inventory {
            if self.is_list(&name) {
                self.program.inventory = Some(name);
            } else {
                self.err(line, format!("`inventory: {name}` must name a list declared in `vars:` (like `vars: {{ {name}: [] }}`)"));
            }
        }
        for (key, found) in [("journal", journal), ("journal_done", journal_done)] {
            let Some((line, name)) = found else { continue };
            if !self.is_list(&name) {
                self.err(line, format!("`{key}: {name}` must name a list declared in `vars:` (like `vars: {{ {name}: [] }}`)"));
            } else if key == "journal" {
                self.program.journal = Some(name);
            } else if self.program.journal.is_some() {
                self.program.journal_done = Some(name);
            } else {
                self.err(line, "`journal_done` needs a `journal:` list to go with");
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
                Some((name, _)) if is_keyword(name.trim()) => {
                    self.err(line, format!("`{}` is a keyword and can't be a variable name", name.trim()))
                }
                Some((name, v)) if is_ident(name.trim()) => match if v.trim().starts_with('[') { parse_list(v) } else { parse_value(v.trim()) } {
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

    /// `gallery: [a.png, b.png]` or `music_room: [...]`; the key may repeat. A gallery entry may name
    /// its own thumbnail, `clip.webm | thumbs/clip.png`.
    fn front_list(&mut self, line: usize, key: &str, val: &str) {
        let Some(inner) = val.strip_prefix('[').and_then(|v| v.strip_suffix(']')) else {
            self.err(line, format!("`{key}` must look like `[assets/one.png, assets/two.png]`"));
            return;
        };
        for item in split_top_level(inner, ',').into_iter().map(str::trim).filter(|p| !p.is_empty()) {
            // `|` inside quotes is part of a name
            let parts = split_top_level(item, '|');
            if parts.len() > 1 && key != "gallery" {
                self.err(line, format!("`{key}` entries have no thumbnail (`|` is only for `gallery`): `{}`", item.trim()));
                continue;
            }
            let (entry, thumb) = match parts[..] {
                [e, t] => (unquote(e.trim()), Some(unquote(t.trim()))),
                _ => (unquote(item), None),
            };
            if entry.is_empty() {
                continue;
            }
            let entry = match check_inside(key, entry) {
                Ok(e) => e,
                Err(m) => {
                    self.err(line, m);
                    continue;
                }
            };
            if let Some(t) = thumb {
                match check_inside("thumbnail", t) {
                    _ if t.is_empty() => self.err(line, format!("`{entry}` has a `|` but no thumbnail after it")),
                    Err(m) => self.err(line, m),
                    Ok(_) if kind_from_path(t) != AssetKind::Image => {
                        self.err(line, format!("the thumbnail `{t}` of `{entry}` must be a picture (PNG, JPEG or WebP)"))
                    }
                    Ok(t) => drop(self.program.gallery_thumbs.entry(entry.clone()).or_insert(t)),
                }
            }
            let list = if key == "gallery" { &mut self.program.gallery } else { &mut self.program.music_room };
            if !list.contains(&entry) {
                list.push(entry);
            }
        }
    }

    /// `languages: en=English, sv=Svenska`: code `=` name, the script's own language first.
    fn front_languages(&mut self, line: usize, val: &str) {
        if !self.program.languages.is_empty() {
            return self.err(line, "`languages` is given twice");
        }
        for part in val.split(',').map(str::trim).filter(|p| !p.is_empty()) {
            let Some((code, name)) = part.split_once('=') else {
                self.err(line, format!("bad language `{part}` (expected `code=Name`, like `languages: en=English, sv=Svenska`)"));
                continue;
            };
            let (code, name) = (code.trim(), unquote(name.trim()).trim());
            if let Some(why) = crate::translate::bad_code(code) {
                self.err(line, format!("bad language code `{code}`: {why}"));
            } else if name.is_empty() || name.chars().any(char::is_control) || name.chars().count() > 40 {
                self.err(line, format!("the name of language `{code}` must be 1 to 40 characters, no control characters"));
            } else if self.program.languages.iter().any(|l| l.code.eq_ignore_ascii_case(code)) {
                self.err(line, format!("language `{code}` is listed twice"));
            } else if self.program.languages.len() >= crate::ops::MAX_LANGUAGES {
                return self.err(line, format!("at most {} languages", crate::ops::MAX_LANGUAGES));
            } else {
                self.program.languages.push(crate::ops::Language { code: code.to_string(), name: name.to_string() });
            }
        }
        if self.program.languages.is_empty() {
            self.err(line, "`languages` needs entries like `languages: en=English, sv=Svenska`");
        }
    }

    /// `replays: { rooftop: "The rooftop", ending-a: "Ending A" }`: label -> title.
    fn front_replays(&mut self, line: usize, val: &str) {
        let Some(inner) = val.strip_prefix('{').and_then(|v| v.strip_suffix('}')) else {
            self.err(line, "`replays` must look like `{ rooftop: \"The rooftop\", ending-a: \"Ending A\" }`");
            return;
        };
        for part in split_top_level(inner, ',').into_iter().map(str::trim).filter(|p| !p.is_empty()) {
            let Some((key, title)) = part.split_once(':') else {
                self.err(line, format!("bad replay `{part}` (expected `label: \"Title\"`)"));
                continue;
            };
            let label = slug(unquote(key.trim()));
            let title = unescape_title(unquote(title.trim()).trim());
            if label.is_empty() {
                self.err(line, format!("bad replay `{part}`: the label is empty"));
            } else if title.is_empty() {
                self.err(line, format!("replay `{label}` needs a title, like `{label}: \"The rooftop\"`"));
            } else if title.chars().any(char::is_control) {
                self.err(line, format!("the title of replay `{label}` can't hold control characters"));
            } else if self.replay_decls.iter().any(|(_, l, _)| *l == label) {
                self.err(line, format!("replay `{label}` is listed twice"));
            } else if self.replay_decls.iter().any(|(_, _, t)| *t == title) {
                self.err(line, format!("two replays are titled \"{title}\"; titles must differ"));
            } else {
                self.replay_decls.push((line, label, title));
            }
        }
    }

    // ---- list variables ---------------------------------------------------

    fn is_list(&self, name: &str) -> bool {
        matches!(self.program.vars.get(name), Some(Value::List(_)))
    }

    /// `what` (like "`count(x)`") needs `name` to be a list variable declared in the front matter.
    fn need_list(&self, name: &str, what: &str) -> Result<(), String> {
        match self.program.vars.get(name) {
            Some(Value::List(_)) => Ok(()),
            Some(_) => Err(format!("{what}: `{name}` is not a list")),
            None => Err(format!("{what}: `{name}` is not a list (declare it in the front matter: `vars: {{ {name}: [] }}`)")),
        }
    }

    /// `has` and `count()` need a list variable declared in the front matter, and
    /// arithmetic needs numbers: a text or a list operand that is known from the
    /// declarations is an error.
    fn check_expr(&self, e: &Expr) -> Result<(), String> {
        let mut tests = Vec::new();
        e.has_tests(&mut tests);
        for (name, _) in tests {
            // a name with `{}` parts spells its variable only at run time, so it can't be checked
            if !is_dynamic(name) {
                self.need_list(name, &format!("`{name} has ...`"))?;
            }
        }
        let mut dynamic = Vec::new();
        e.dynamic_names(&mut dynamic);
        dynamic.into_iter().try_for_each(|n| self.check_parts(n))?;
        self.check_numbers(e)
    }

    /// The `{}` parts of a variable name (`love.{who}`) must hold text or a number: a declared yes/no or
    /// list variable can't. An undeclared one is allowed, like in picture names (a `::call` parameter or
    /// an `::input` answer is one); `quillstone test` warns when nothing sets it.
    fn check_parts(&self, name: &str) -> Result<(), String> {
        for v in crate::name_vars(name) {
            if let Some(Value::Bool(_) | Value::List(_)) = self.program.vars.get(v) {
                return Err(format!("`{name}`: `{v}` must be a text or number variable (its value is put into the name)"));
            }
        }
        Ok(())
    }

    fn check_numbers(&self, e: &Expr) -> Result<(), String> {
        let number = |operand: &Expr, what: &str| -> Result<(), String> {
            let kind = match operand {
                Expr::Lit(Value::Str(s)) => Some(format!("the text \"{s}\"")),
                Expr::Var(n) => match self.program.vars.get(n) {
                    Some(Value::Str(_)) => Some(format!("`{n}` (a text variable)")),
                    Some(Value::List(_)) => Some(format!("`{n}` (a list)")),
                    _ => None,
                },
                _ => None,
            };
            match kind {
                Some(k) => Err(format!(
                    "{k} is not a number, but {what} works on whole numbers (text is joined with `+=`; `count(list)` is a list's size)"
                )),
                None => Ok(()),
            }
        };
        match e {
            Expr::Bin(a, op, b) => {
                let what = format!("`{}`", op.symbol());
                number(a, &what)?;
                number(b, &what)?;
            }
            Expr::Neg(a) => number(a, "`-`")?,
            Expr::Func(_, args) => {
                for a in args {
                    number(a, "a function like `min`, `max` or `abs`")?;
                }
            }
            Expr::Random(a, b) => {
                number(a, "`random`")?;
                number(b, "`random`")?;
            }
            Expr::Count(name) if !is_dynamic(name) => self.need_list(name, &format!("`count({name})`"))?,
            _ => {}
        }
        e.children().into_iter().try_for_each(|c| self.check_numbers(c))
    }

    /// A bare list variable can't be used as a value (it would be silently ignored):
    /// `copy = inv`, `x += inv` or `::call{... a=inv}`. Use `inv has "item"` to ask about it.
    fn not_a_list_value(&self, e: &Expr, place: &str) -> Result<(), String> {
        match e {
            Expr::Var(name) if self.is_list(name) => Err(format!(
                "`{name}` is a list and can't be {place}: lists change by `+=`/`-=` of an item or `= [..]`, \
                 `{name} has \"item\"` asks about one, and `pick({name})` takes one"
            )),
            _ => Ok(()),
        }
    }

    /// A declared list changes by `+=` / `-=` of text, or `=` a list literal (`[]`, `["key", "map"]`).
    fn check_effect(&self, e: &Effect) -> Result<(), String> {
        self.check_expr(&e.value)?;
        self.not_a_list_value(&e.value, "the right side of an assignment")?;
        self.check_parts(&e.var)?;
        if matches!(e.value, Expr::Lit(Value::List(_))) && !self.is_list(&e.var) {
            return Err(format!("`{}` is not a list, so `= [..]` can't set it (declare it in the front matter: `vars: {{ {0}: [] }}`)", e.var));
        }
        if !self.is_list(&e.var) {
            return Ok(());
        }
        let var = &e.var;
        match (e.op, &e.value) {
            (AssignOp::Set, Expr::Lit(Value::List(_))) => Ok(()),
            (AssignOp::Set, _) => Err(format!(
                "`{var}` is a list: change it with `{var} += \"item\"`, `{var} -= \"item\"` or set it with `{var} = [\"a\", \"b\"]` (`[]` clears it), not `= value`"
            )),
            (_, Expr::Lit(Value::Str(s))) if !s.is_empty() => Ok(()),
            (_, Expr::Lit(_)) => Err(format!("`{var}` is a list: `+=` and `-=` take a non-empty text, like `{var} += \"key\"`")),
            (_, Expr::Bin(..) | Expr::Neg(_) | Expr::Func(..) | Expr::Count(_) | Expr::Random(..)) => {
                Err(format!("`{var}` is a list: `+=` and `-=` take a text item, like `{var} += \"key\"`, not a number"))
            }
            _ => Ok(()), // a variable or other expression: only text works, anything else is ignored
        }
    }

    // ---- labels, prose, images ------------------------------------------

    fn label(&mut self, line: usize, text: &str) {
        let (id_text, param_text) = split_params(text);
        let name = slug(id_text);
        if name.is_empty() {
            self.err(line, "heading has no usable label name");
            return;
        }
        let params = match param_text.map(|p| self.parameters(&name, p)) {
            Some(Ok(params)) => Some(params),
            Some(Err(m)) => {
                self.err(line, m);
                None
            }
            None => None,
        };
        let at = self.here();
        self.program.headings.push(Heading { label: name.clone(), text: text.to_string(), line });
        if self.program.labels.insert(name.clone(), at).is_some() {
            let first = self.label_lines.get(&name).map(|&l| format!(" (first defined at {})", self.describe(l))).unwrap_or_default();
            self.err(line, format!("duplicate label `{name}`{first}"));
        } else {
            self.label_lines.insert(name.clone(), line);
        }
        if let Some(params) = params {
            let defaults: BTreeMap<String, Value> = params.iter().filter_map(|(n, d)| Some((n.clone(), d.clone()?))).collect();
            if !defaults.is_empty() {
                self.program.param_defaults.insert(name.clone(), defaults);
            }
            self.program.params.insert(name.clone(), params.into_iter().map(|(n, _)| n).collect());
            self.param_labels.push((name, line));
        }
    }

    /// The parameters of a heading like `# heal(amount, who="you", times=1)`: names and defaults (literals).
    fn parameters(&self, label: &str, inner: &str) -> Result<Vec<(String, Option<Value>)>, String> {
        if inner.trim().is_empty() {
            return Err(format!("`{label}()` has no parameters: write the heading without parentheses"));
        }
        let mut names: Vec<(String, Option<Value>)> = Vec::new();
        for part in split_top_level(inner, ',').into_iter().map(str::trim) {
            let (part, default) = match part.split_once('=') {
                Some((n, d)) => (n.trim(), Some(d.trim())),
                None => (part, None),
            };
            let default = match default.map(parse_value) {
                Some(Ok(v)) => Some(v),
                Some(Err(_)) => {
                    return Err(format!("the default of `{part}` must be a number, true/false or \"text\" in quotes (a literal, not an expression)"))
                }
                None => None,
            };
            if !is_plain_ident(part) {
                return Err(format!("bad parameter `{part}` in the heading (names are letters, digits and `_`, separated by commas)"));
            }
            if is_keyword(part) || matches!(part, "to" | "p" | "nw") {
                let why = match part {
                    "to" => "`::call` uses `to=` for the label",
                    "p" | "nw" => "`{p}` and `{nw}` are text tags",
                    _ => "it is a keyword",
                };
                return Err(format!("`{part}` can't be a parameter name: {why}"));
            }
            if names.iter().any(|(n, _)| n == part) {
                return Err(format!("parameter `{part}` is listed twice"));
            }
            if self.is_list(part) {
                return Err(format!("`{part}` is a list variable and can't be a parameter"));
            }
            names.push((part.to_string(), default));
        }
        Ok(names)
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
        let path = match self.picture_path("path", path) {
            Ok(p) => p,
            Err(m) => {
                self.err(line, format!("image {m}"));
                return;
            }
        };
        let op = if kind_from_path(&path) == AssetKind::Video {
            Op::Video { src: path, fit: Fit::Contain, looped: false, skippable: false }
        } else {
            Op::Image { src: path, alt: alt.to_string() }
        };
        self.push(op, line);
    }

    // ---- commands ---------------------------------------------------------

    /// Parse the command at `lines[i]`. Returns the index of the last line consumed.
    fn command(&mut self, lines: &[&str], i: usize) -> usize {
        let line = self.base + i + 1;
        let body = &lines[i].trim()[2..];
        if body.trim().is_empty() {
            self.err(line, "stray `::` (it only closes a `::choice`, `::hotspots` or `::shop` block)");
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
            "choice" => return self.choice(lines, i, inner, false),
            "hotspots" => return self.choice(lines, i, inner, true),
            "shop" => return self.shop(lines, i, inner),
            "item" => {
                self.item(line, inner);
                return i;
            }
            "call" => {
                self.call(line, inner);
                return i;
            }
            "set" => {
                let parts: Vec<&str> =
                    split_top_level(inner, ',').into_iter().map(str::trim).filter(|p| !p.is_empty()).collect();
                if parts.is_empty() {
                    self.err(line, "`::set` needs an assignment like `{trust+=1}`");
                }
                for part in parts {
                    match parse_effect(part).and_then(|e| self.check_effect(&e).map(|()| e)) {
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
            "anim" => {
                self.anim(line, inner);
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
            "else" | "endif" | "return" | "inventory" | "journal" => {
                if !inner.is_empty() {
                    self.err(line, format!("`::{name}` takes no attributes"));
                }
                match name {
                    "else" => self.else_branch(line),
                    "endif" => self.endif(line),
                    "inventory" if self.program.inventory.is_none() => self.err(
                        line,
                        "`::inventory` needs a list to show: add `inventory: <list variable>` to the front matter",
                    ),
                    "inventory" => self.push(Op::Inventory, line),
                    "journal" if self.program.journal.is_none() => self.err(
                        line,
                        "`::journal` needs a list to show: add `journal: <list variable>` to the front matter",
                    ),
                    "journal" => self.push(Op::Journal, line),
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
        // An empty error means the attribute check below reports the mistake (see `hud`).
        if let Some(m) = built.as_ref().err().filter(|m| !m.is_empty()) {
            self.err(line, m);
        }
        // When building stopped at an error, attributes after it were never read:
        // only report those that are wrong, not merely unread.
        let known = known_attrs(name);
        if let Some(extra) = attrs.leftover(if built.is_ok() { &[] } else { known }) {
            let gone = match (extra, known.contains(&"x")) {
                ("at", true) => Some("use `x=` (left, center, right or a %) and `y=` (top, center, bottom or a %)"),
                ("anchor", true) => Some("`x=` and `y=` place the object by its own size (`x=right` puts its right edge at the screen's right edge)"),
                _ => None,
            };
            match gone {
                Some(g) => self.err(line, format!("`{extra}=` is gone: {g}")),
                None => self.err(line, unknown_attr(name, extra)),
            }
        }
        if let Ok((op, label)) = built {
            if let Some(label) = label {
                self.fixups.push(Fixup { op: self.here(), option: None, label, line });
            }
            self.push(op, line);
            for extra in std::mem::take(&mut self.trailing) {
                self.push(extra, line);
            }
        }
        i
    }

    /// `::call{to=heal amount=5 who="Mika"}`: a label and the values of its parameters.
    /// A value runs to the next space outside quotes and parentheses, and is an
    /// ordinary expression (so text is quoted).
    fn call(&mut self, line: usize, inner: &str) {
        let tokens = match call_tokens(inner) {
            Ok(t) => t,
            Err(m) => return self.err(line, m),
        };
        let mut to: Option<String> = None;
        let mut args: Vec<(String, Expr)> = Vec::new();
        for (key, value) in tokens {
            match value {
                None if to.is_none() => to = Some(key),
                None => self.err(line, format!("unexpected `{key}`: `::call` takes one label (arguments are written `name=value`)")),
                Some(v) if key == "to" => match (&to, unquote(&v)) {
                    (Some(_), _) => self.err(line, "`::call` names its label twice"),
                    (None, label) => to = Some(label.to_string()),
                },
                Some(v) => {
                    if !is_plain_ident(&key) || is_keyword(&key) {
                        self.err(line, format!("`{key}` can't be an argument name (arguments are the label's parameter names)"));
                    } else if args.iter().any(|(k, _)| *k == key) {
                        self.err(line, format!("`{key}` is given twice in this `::call`"));
                    } else {
                        let checked = parse_expr(&v).and_then(|e| {
                            self.check_expr(&e)?;
                            self.not_a_list_value(&e, "a `::call` argument")?;
                            Ok(e)
                        });
                        match checked {
                            Ok(e) => args.push((key, e)),
                            Err(m) => self.err(line, format!("{m} (in the argument `{key}={v}`)")),
                        }
                    }
                }
            }
        }
        let Some(to) = to.filter(|t| !slug(t).is_empty()) else {
            return self.err(line, "`::call` needs a label, like `::call{to=label}`");
        };
        let label = slug(&to);
        self.fixups.push(Fixup { op: self.here(), option: None, label, line });
        self.push(Op::Call { target: UNSET, args }, line);
    }

    /// Build an op from attributes. The `Option<String>` is a label to resolve later.
    fn build_op(&mut self, line: usize, name: &str, a: &mut Attrs) -> Result<(Op, Option<String>), String> {
        match name {
            "bg" => {
                let has = |a: &Attrs, k: &str| a.0.iter().any(|(key, _)| key == k);
                if has(a, "anim") && (has(a, "src") || has(a, "color")) {
                    return Err("`::bg` takes one of `src=`, `color=` or `anim=`".into());
                }
                let src = match (has(a, "src"), has(a, "color")) {
                    _ if has(a, "anim") => {
                        let id = a.value("anim").unwrap_or_default();
                        self.anim_ref(&id)?
                    }
                    (true, true) => return Err("`::bg` takes `src=` (a picture or video) or `color=` (a solid colour), not both".into()),
                    (false, true) => {
                        let c = color_attr(a, "color")?.ok_or("`::bg{color=...}` needs a colour like `color=#102030`")?;
                        bg_color_src(c)
                    }
                    (false, false) => return Err("`::bg` needs `src=file`, `color=#rrggbb` or `anim=id`".into()),
                    (true, false) => self.require(name, a, "src")?,
                };
                let sound = a.flag("sound");
                if sound && kind_from_path(&src) != AssetKind::Video {
                    return Err("`sound` is for a video background: `src=` must be a video file".into());
                }
                let fade = Self::fade(a)?;
                let wipe = match a.value("wipe") {
                    None => None,
                    Some(v) => match Wipe::parse(&v) {
                        Some(w) => Some(w),
                        None => return Err(format!("`wipe={v}` must be `left`, `right`, `up` or `down`")),
                    },
                };
                if wipe.is_some() && fade <= 0.0 {
                    return Err("a wipe needs a duration: add `fade=` (seconds)".into());
                }
                let through = match a.take("through") {
                    None => None,
                    Some(None) => Some([0.0; 3]),
                    Some(Some(c)) => Some(parse_color(&c)?),
                };
                if let Some(color) = through {
                    if fade <= 0.0 {
                        return Err("`through` needs a duration: add `fade=` (seconds; half to fade out, half to fade in)".into());
                    }
                    if wipe.is_some() {
                        return Err("`through` and `wipe` can't be combined: a wipe is a transition of its own".into());
                    }
                    // Fade out for half the time, change the picture (no crossfade), fade in.
                    let half = fade / 2.0;
                    self.push(Op::FadeOut { time: half, color }, line);
                    self.trailing.push(Op::FadeIn { time: half });
                    return Ok((Op::Bg { src, fade: 0.0, wipe: None, sound }, None));
                }
                Ok((Op::Bg { src, fade, wipe, sound }, None))
            }
            "show" => self.show(a),
            "hud" => self.hud(line, a),
            "hide" => {
                let all = a.flag("all");
                let bare = a.take_bare();
                Self::extra_words("hide", &bare, if all { 0 } else { 1 })?;
                let fade = Self::fade(a)?;
                let id = match (all, bare.first()) {
                    (true, _) => None,
                    (false, Some(id)) => Some(self.sprite_id(id)),
                    (false, None) => return Err("`::hide` needs a sprite (`::hide{mika}`) or `all`".into()),
                };
                Ok((Op::Hide { id, fade }, None))
            }
            "move" => {
                // `then` starts the next step: split before anything is taken from the first.
                let loop_attr = a.take("loop"); // anywhere in the command (the end reads best); before the bare words: a bare `loop` is no sprite name
                let mut parts: Vec<Vec<_>> = vec![Vec::new()];
                for (i, item) in std::mem::take(&mut a.0).into_iter().enumerate() {
                    if i > 0 && item.0 == "then" && item.1.is_none() {
                        parts.push(Vec::new());
                    } else if let Some(last) = parts.last_mut() {
                        last.push(item);
                    }
                }
                let mut groups = parts.into_iter().map(Attrs);
                a.0 = groups.next().map(|g| g.0).unwrap_or_default();
                let flip = opt_flag(a, "flip");
                let bare = a.take_bare();
                let looped = match loop_attr {
                    None => None,
                    Some(None) => Some(true),
                    Some(Some(v)) if v == "pingpong" => Some(true),
                    Some(Some(v)) => return Err(format!("`loop={v}` must be `pingpong` (or just `loop`: back and forth forever)")),
                };
                Self::extra_words("move", &bare, 1)?;
                let Some(id) = bare.first().map(|s| self.sprite_id(s)) else {
                    return Err("`::move` needs a sprite, like `::move{mika x=left time=0.5}`".into());
                };
                let mut steps = vec![Self::move_step(a)?];
                if steps[0].is_empty() && flip.is_none() {
                    return Err("`::move` needs `x=`, `y=`, `scale=`, `xscale=`, `yscale=`, `alpha=`, `xoffset=`, `yoffset=`, `wait=` and/or `flip`".into());
                }
                for mut g in groups {
                    steps.push(Self::move_step(&mut g)?);
                    if let Some((k, _)) = g.0.first() {
                        let hint = if k == "flip" { ": it belongs in the first step" } else { "" };
                        return Err(format!("unknown attribute `{k}` in a `then` step of `::move`{hint}"));
                    }
                    if steps[steps.len() - 1].is_empty() {
                        return Err("a `then` step needs `x=`, `y=`, `scale=`, `xscale=`, `yscale=`, `alpha=`, `xoffset=`, `yoffset=` and/or `wait=`".into());
                    }
                }
                if steps.len() > MAX_MOVE_STEPS {
                    return Err(format!("a `::move` takes at most {MAX_MOVE_STEPS} steps"));
                }
                let looped = looped.is_some();
                if looped {
                    if steps.len() == 1 {
                        let s = &steps[0];
                        if s.props().iter().all(Option::is_none) {
                            return Err("a `loop` move needs something to animate: `x=`, `y=`, `scale=`, `xscale=`, `yscale=`, `alpha=`, `xoffset=` or `yoffset=` (`flip` can't loop)".into());
                        }
                        if s.time <= 0.0 {
                            return Err("a `loop` move needs `time=` above 0 (the seconds one leg takes)".into());
                        }
                        if s.wait > 0.0 {
                            return Err("a one-step `loop` can't `wait=`: add `then` steps to repeat them all".into());
                        }
                    } else if steps.iter().map(|s| s.time + s.wait).sum::<f32>() <= 0.0 {
                        return Err("a `loop` move needs some `time=` or `wait=` above 0 (it would repeat without taking any time)".into());
                    }
                }
                Ok((Op::Move { id, flip, steps, looped }, None))
            }
            "shake" => {
                let time = Self::seconds(a, "time", 0.4)?;
                let strength = Self::number(a, "strength", 0.0..=200.0)?.unwrap_or(20.0);
                let axis = match a.value("axis") {
                    None => Axis::default(),
                    Some(v) => match Axis::parse(&v) {
                        Some(x) => x,
                        None => return Err(format!("`axis={v}` must be `x`, `y` or `both`")),
                    },
                };
                Ok((Op::Shake { time, strength, axis }, None))
            }
            "flash" => {
                let time = Self::short_seconds(a, "time", 0.4)?;
                let color = color_attr(a, "color")?.unwrap_or([1.0; 3]);
                Ok((Op::Flash { time, color }, None))
            }
            "notify" => {
                let text = a.value("text").unwrap_or_default();
                if text.trim().is_empty() {
                    return Err("`::notify` needs `text=\"...\"` (the message)".into());
                }
                crate::markup::check_inline_markup(&text, "a `::notify` text")?;
                let time = match a.value("time") {
                    None => 3.0,
                    Some(v) => match v.parse::<f32>() {
                        Ok(t) if t.is_finite() && (0.5..=30.0).contains(&t) => t,
                        _ => return Err(format!("`time={v}` must be a number of seconds from 0.5 to 30")),
                    },
                };
                Ok((Op::Notify { text, time }, None))
            }
            "unlock" => {
                let src = self.require(name, a, "src")?;
                let p = &self.program;
                if !p.gallery.contains(&src) && !p.music_room.contains(&src) {
                    return Err(format!("`::unlock{{src={src}}}`: `{src}` is not a `gallery:` or `music_room:` entry"));
                }
                Ok((Op::Unlock { src }, None))
            }
            "fade" => {
                let (out, into) = (a.flag("out"), a.flag("in"));
                let time = Self::short_seconds(a, "time", 0.5)?;
                match (out, into) {
                    (true, true) => Err("`::fade` takes `out` or `in`, not both".into()),
                    (false, false) => Err("`::fade` needs `out` (fade to a colour and stay covered) or `in` (uncover)".into()),
                    (false, true) => {
                        if a.0.iter().any(|(k, _)| k == "color") {
                            return Err("`::fade{in}` takes no `color`: it fades away the colour the screen is covered with".into());
                        }
                        Ok((Op::FadeIn { time }, None))
                    }
                    (true, false) => {
                        let color = color_attr(a, "color")?.unwrap_or([0.0; 3]);
                        Ok((Op::FadeOut { time, color }, None))
                    }
                }
            }
            "camera" => {
                let reset = a.flag("reset");
                let zoom = Self::number(a, "zoom", 1.0..=4.0)?;
                let x = Self::place(a, "x", 0.0..=1.0)?;
                let y = Self::place(a, "y", 0.0..=1.0)?;
                let time = Self::seconds(a, "time", 0.0)?;
                let ease = Self::ease(a)?;
                let op = if reset {
                    Op::Camera { zoom: Some(1.0), x: Some(0.5), y: Some(0.5), time, ease }
                } else if zoom.is_none() && x.is_none() && y.is_none() {
                    return Err("`::camera` needs `zoom=`, `x=` and/or `y=` (or `reset`)".into());
                } else {
                    Op::Camera { zoom, x, y, time, ease }
                };
                Ok((op, None))
            }
            "particles" => Self::particles(a).map(|op| (op, None)),
            "tint" => Self::tint(a).map(|op| (op, None)),
            "textbox" => {
                let (hide, show) = (a.flag("hide"), a.flag("show"));
                let fade = Self::short_seconds(a, "fade", 0.0)?;
                match (hide, show) {
                    (true, false) => Ok((Op::Textbox { hidden: true, fade }, None)),
                    (false, true) => Ok((Op::Textbox { hidden: false, fade }, None)),
                    (true, true) => Err("`::textbox` takes `hide` or `show`, not both".into()),
                    (false, false) => Err("`::textbox` needs `hide` or `show` (and optionally `fade=<seconds>`)".into()),
                }
            }
            "music" => {
                if a.flag("stop") {
                    let fade = Self::fade(a)?;
                    Ok((Op::StopMusic { fade }, None))
                } else {
                    let intro = a.take("intro");
                    if intro.is_some() && !a.0.iter().any(|(k, _)| k == "src") {
                        return Err("`intro=` needs `src=`: the file that loops after the intro".into());
                    }
                    let src = self.require(name, a, "src")?;
                    let intro = match intro {
                        None => None,
                        Some(Some(i)) if !i.is_empty() => match check_inside("intro", &i) {
                            Ok(i) if i == src => return Err("`intro` and `src` are the same file: `src` is the loop, `intro` plays once before it".into()),
                            Ok(i) => Some(i),
                            Err(m) => return Err(m),
                        },
                        Some(_) => return Err("`intro=` needs a file (`::music{src=loop.ogg intro=intro.ogg}`)".into()),
                    };
                    let fade = Self::fade(a)?;
                    let looped = a.flag_or("loop", true);
                    if intro.is_some() && !looped {
                        return Err("`intro` needs a looping track: it plays once and then `src` loops (drop `loop=false`)".into());
                    }
                    let volume = Self::volume(a)?;
                    Ok((Op::Music { src, intro, fade, looped, volume }, None))
                }
            }
            "sfx" => {
                if let Some(target) = a.take("stop") {
                    if let Some(extra) = ["src", "loop", "volume"].into_iter().find(|k| a.0.iter().any(|(key, _)| key == k)) {
                        return Err(format!("`::sfx{{stop}}` stops sounds, so `{extra}` doesn't go with it"));
                    }
                    let fade = Self::fade(a)?;
                    let src = match target {
                        None => None,
                        Some(v) if v.is_empty() => return Err("`stop=` needs a file (`::sfx{stop=rain.ogg}`), or write just `stop` for all sounds".into()),
                        Some(v) => Some(v),
                    };
                    if let Some(src) = &src {
                        self.sfx_stops.push((src.clone(), line));
                    }
                    return Ok((Op::StopSfx { src, fade }, None));
                }
                let src = self.require(name, a, "src")?;
                let looped = a.flag("loop");
                let faded = a.0.iter().any(|(k, _)| k == "fade");
                let volume = Self::volume(a)?;
                if looped {
                    let fade = Self::fade(a)?;
                    Ok((Op::SfxLoop { src, fade, volume }, None))
                } else if faded {
                    Err("`fade` on a sound effect needs `loop` (a one-shot sound is too short to fade) or `stop`".into())
                } else {
                    Ok((Op::Sfx { src, volume }, None))
                }
            }
            "voice" => {
                let src = self.require(name, a, "src")?;
                let volume = Self::volume(a)?;
                Ok((Op::Voice { src, volume }, None))
            }
            "wait" => {
                let time = a.value("time");
                let bare = if time.is_none() { a.take_bare().into_iter().next() } else { None };
                if bare.as_deref() == Some("click") {
                    return Ok((Op::WaitClick, None));
                }
                let value = time.or(bare);
                let seconds = match value.as_deref().map(str::parse::<f32>) {
                    Some(Ok(s)) if s.is_finite() && s > 0.0 => s,
                    _ => return Err("`::wait` needs a positive number of seconds, like `::wait{time=1.5}`, or `click`".into()),
                };
                Ok((Op::Wait { seconds }, None))
            }
            "video" => {
                let src = self.require(name, a, "src")?;
                let fit = match a.value("fit") {
                    None => Fit::default(),
                    Some(v) => match Fit::parse(&v) {
                        Some(f) => f,
                        None => return Err(format!("unknown fit `{v}` (use contain, cover or stretch)")),
                    },
                };
                let looped = a.flag("loop");
                // A looping video never ends by itself, so the player must be able to skip it.
                let skippable = a.flag("skippable") || looped;
                Ok((Op::Video { src, fit, looped, skippable }, None))
            }
            "jump" => {
                let to = match a.value("to").or_else(|| a.take_bare().into_iter().next()) {
                    Some(t) => t,
                    None => return Err(format!("`::{name}` needs a label, like `::{name}{{to=label}}`")),
                };
                let label = Some(slug(&to));
                let cond = match a.value("if") {
                    None => None,
                    Some(c) => Some(parse_expr(&c).and_then(|e| self.check_expr(&e).map(|()| e))?),
                };
                Ok((Op::Jump { target: UNSET, cond }, label))
            }
            "input" => {
                let var = match a.value("var") {
                    Some(v) if !v.is_empty() => v,
                    _ => return Err("`::input` needs `var=` (the text variable that receives the answer, like `var=hero.name`)".into()),
                };
                match self.program.vars.get(&var) {
                    Some(Value::Str(_)) => {}
                    Some(_) => return Err(format!("`var={var}`: `{var}` must be a text variable (declare it with a text default, like `vars: {{ {var}: \"Alex\" }}`)")),
                    None => return Err(format!("`var={var}`: no text variable `{var}` (declare it in the front matter, like `vars: {{ {var}: \"Alex\" }}`)")),
                }
                let prompt = match a.value("prompt") {
                    Some(p) if !p.trim().is_empty() => p.trim().to_string(),
                    _ => return Err("`::input` needs `prompt=\"...\"` (the question shown above the text field)".into()),
                };
                crate::markup::check_inline_markup(&prompt, "an `::input` prompt")?;
                let default = a.value("default").map(|d| d.trim().to_string()).filter(|d| !d.is_empty());
                if let Some(d) = &default {
                    if d.chars().any(char::is_control) {
                        return Err("`default=` can't hold control characters".into());
                    }
                }
                let max = match a.value("max") {
                    None => INPUT_MAX_DEFAULT,
                    Some(v) => match v.parse::<usize>() {
                        Ok(n) if (1..=INPUT_MAX_CAP).contains(&n) => n,
                        _ => return Err(format!("bad `max={v}` (a whole number from 1 to {INPUT_MAX_CAP})")),
                    },
                };
                if let Some(d) = &default {
                    if d.chars().count() > max {
                        return Err(format!("`default=` is longer than `max={max}`"));
                    }
                }
                Ok((Op::Input { var, prompt, default, max }, None))
            }
            "timing" => {
                let var = self.result_var(a, "timing")?;
                let prompt = Self::prompt_attr(a, "timing")?;
                let speed = match a.value("speed") {
                    None => 1.0,
                    Some(v) => match v.parse::<f32>() {
                        Ok(n) if (MIN_TIMING_SPEED..=MAX_TIMING_SPEED).contains(&n) => n,
                        _ => return Err(format!("bad `speed={v}` (sweeps of the bar per second, {MIN_TIMING_SPEED} to {MAX_TIMING_SPEED})")),
                    },
                };
                let Some(good) = a.value("good") else {
                    return Err("`::timing` needs `good=` (the zone to stop in, like `good=40%..60%`)".into());
                };
                let good = match parse_zone(&good) {
                    Ok(z) => z,
                    Err(m) => return Err(format!("bad `good={good}` ({m})")),
                };
                let perfect = match a.value("perfect") {
                    None => None,
                    Some(p) => match parse_zone(&p) {
                        Ok(z) if z[0] >= good[0] && z[1] <= good[1] => Some(z),
                        Ok(_) => return Err(format!("`perfect={p}` must lie inside `good`")),
                        Err(m) => return Err(format!("bad `perfect={p}` ({m})")),
                    },
                };
                let time = match a.value("time") {
                    None => None,
                    Some(t) => Some(parse_choice_time(&t)?),
                };
                Ok((Op::Timing { var, prompt, bar: TimingBar { speed, good, perfect, time } }, None))
            }
            "press" => {
                let var = self.result_var(a, "press")?;
                let prompt = Self::prompt_attr(a, "press")?;
                let key = a.value("key").unwrap_or_default().to_lowercase();
                if !PRESS_KEYS.contains(&key.as_str()) {
                    return Err(format!("`::press` needs `key=` (one of: {})", PRESS_KEYS.join(", ")));
                }
                let Some(t) = a.value("time") else {
                    return Err("`::press` needs `time=` (seconds to press it in, like `time=1.2`)".into());
                };
                let time = parse_choice_time(&t)?;
                let count = match a.value("count") {
                    None => 1,
                    Some(c) => match c.parse::<u32>() {
                        Ok(n) if (1..=MAX_PRESS_COUNT).contains(&n) => n,
                        _ => return Err(format!("bad `count={c}` (a whole number of presses from 1 to {MAX_PRESS_COUNT})")),
                    },
                };
                Ok((Op::Press { var, prompt, press: KeyPress { key, count, time } }, None))
            }
            "end" => Ok((Op::End, None)),
            "end_replay" => Ok((Op::EndReplay, None)),
            other => Err(format!("unknown command `::{other}` (known: {COMMANDS})")),
        }
    }

    fn show(&mut self, a: &mut Attrs) -> Result<(Op, Option<String>), String> {
        let flip = opt_flag(a, "flip");
        let bare = a.take_bare();
        let src_attr = a.value("src");
        let id_attr = a.value("id");
        let anim_attr = a.value("anim");
        let is_anim = anim_attr.is_some();
        // A character with layers: its layer attributes (`eyes=open`) are taken first, so
        // an error further down never reports them as unknown attributes.
        let layered_char = id_attr
            .as_ref()
            .or(bare.first())
            .and_then(|who| self.program.characters.get(&who.to_lowercase()))
            .filter(|c| !c.layers.is_empty())
            .cloned();
        let given: Vec<(String, String)> = match &layered_char {
            Some(c) => c.layers.iter().filter_map(|l| Some((l.clone(), a.value(l)?))).collect(),
            None => Vec::new(),
        };
        if anim_attr.is_some() && src_attr.is_some() {
            return Err("`::show` takes `src=` (a picture) or `anim=` (an animation), not both".into());
        }
        if let (Some(c), false) = (&layered_char, given.is_empty()) {
            if src_attr.is_some() || anim_attr.is_some() {
                return Err(format!("`::show` can't mix layers ({}) with `src=` or `anim=`", c.layers.join(", ")));
            }
            if bare.len() > 1 {
                let msg = format!("`::show{{{} {}}}`: a pose and layers can't be set together (a pose replaces the whole sprite)", c.id, bare[1]);
                return Err(msg);
            }
            return self.show_layers(a, c, given, flip);
        }

        Self::extra_words("show", &bare, if src_attr.is_some() || anim_attr.is_some() { 1 } else { 2 })?;
        let (id, mut src) = match (src_attr, anim_attr) {
            (_, Some(anim)) => {
                // Like a pose sprite, a character's name addresses the character's sprite.
                let Some(who) = id_attr.or_else(|| bare.first().cloned()) else {
                    return Err("`::show{anim=...}` needs a sprite name, like `::show{mika anim=blink}`".into());
                };
                (self.sprite_id(&who), self.anim_ref(&anim)?)
            }
            (Some(src), None) => {
                let named = id_attr.or_else(|| bare.first().cloned());
                if named.is_none() && crate::is_pattern(&src) {
                    return Err("a `src=` with a `{variable}` needs a sprite name, like `::show{mika src=\"assets/{place}.png\"}`".into());
                }
                let id = named.unwrap_or_else(|| src.clone());
                (id, src)
            }
            (None, None) => {
                let Some(who) = bare.first() else {
                    return Err("`::show` needs a character (`::show{mika happy}`) or `src=...`".into());
                };
                let pose = bare.get(1).cloned().unwrap_or_else(|| "default".to_string());
                let character = self.program.characters.get(&who.to_lowercase()).cloned();
                let Some(character) = character else {
                    return Err(format!("unknown character `{who}` (declare it first with `::character`)"));
                };
                if character.sprites.is_none() && !character.layers.is_empty() && bare.len() == 1 {
                    // Layers only: with no layer named, the show changes only where the sprite stands.
                    return self.show_layers(a, &character, Vec::new(), flip);
                }
                let Some(template) = character.sprites else {
                    let msg = if character.layers.is_empty() {
                        format!("character `{who}` has no `sprites=...` template")
                    } else {
                        format!(
                            "character `{who}` has layers ({}), not poses: set them like `::show{{{who} {}=...}}`",
                            character.layers.join(", "),
                            character.layers[0]
                        )
                    };
                    return Err(msg);
                };
                (character.id, template.replace("{pose}", &pose))
            }
        };

        if !is_anim {
            src = self.picture_path("src", &src)?;
        }
        self.show_rest(a, id, src, Vec::new(), flip)
    }

    /// The position, size and fade attributes of a `::show`, shared by pose and layered sprites.
    fn show_rest(&mut self, a: &mut Attrs, id: String, src: String, layers: Vec<(String, String)>, flip: Option<bool>) -> Result<(Op, Option<String>), String> {
        let x = Self::place(a, "x", POSITION)?;
        let y = Self::place(a, "y", POSITION)?;
        let z = match a.value("z") {
            None => None,
            Some(v) => match v.trim().parse::<i32>() {
                Ok(z) if (-100..=100).contains(&z) => Some(z),
                _ => return Err(format!("`z={v}` must be a whole number from -100 to 100")),
            },
        };
        let scale = Self::number(a, "scale", SPRITE_SCALE)?;
        let xscale = Self::number(a, "xscale", SPRITE_SCALE)?;
        let yscale = Self::number(a, "yscale", SPRITE_SCALE)?;
        let alpha = Self::number(a, "alpha", 0.0..=1.0)?;
        let dx = Self::number(a, "xoffset", SPRITE_OFFSET)?;
        let dy = Self::number(a, "yoffset", SPRITE_OFFSET)?;
        let fade = Self::fade(a)?;
        Ok((Op::Show { id, src, layers, x, y, z, scale, flip, xscale, yscale, alpha, dx, dy, fade }, None))
    }

    /// One step of a `::move`: the properties to animate, `time=`, `ease=` and `wait=`.
    fn move_step(a: &mut Attrs) -> Result<MoveStep, String> {
        let mut step = MoveStep {
            x: Self::place(a, "x", POSITION)?,
            y: Self::place(a, "y", POSITION)?,
            scale: Self::number(a, "scale", SPRITE_SCALE)?,
            xscale: Self::number(a, "xscale", SPRITE_SCALE)?,
            yscale: Self::number(a, "yscale", SPRITE_SCALE)?,
            alpha: Self::number(a, "alpha", 0.0..=1.0)?,
            dx: Self::number(a, "xoffset", SPRITE_OFFSET)?,
            dy: Self::number(a, "yoffset", SPRITE_OFFSET)?,
            time: 0.0,
            ease: Self::ease(a)?,
            wait: Self::seconds(a, "wait", 0.0)?,
        };
        // A step that animates nothing is only a pause: its `time` defaults to 0, so `then wait=2` lasts 2 s.
        let default = if step.props().iter().any(Option::is_some) { 0.5 } else { 0.0 };
        step.time = Self::seconds(a, "time", default)?;
        Ok(step)
    }

    /// `::show{mika body=casual eyes=anim:blink mouth=none x=left}`: set layers of a layered sprite.
    fn show_layers(&mut self, a: &mut Attrs, c: &Character, given: Vec<(String, String)>, flip: Option<bool>) -> Result<(Op, Option<String>), String> {
        let mut layers = Vec::new();
        for (layer, value) in &given {
            let picture = if value == LAYER_NONE {
                String::new()
            } else if let Some(id) = value.strip_prefix(ANIM_PREFIX) {
                self.anim_ref(id)?
            } else {
                let path = c.layer_path(layer, value).unwrap_or_default();
                self.picture_path(layer, &path)?
            };
            layers.push((layer.clone(), picture));
        }
        // The first layered show of a character has to name every layer: after that the
        // layers keep their values (a sprite hidden in between starts over at run time).
        if !self.layered_seen.contains(&c.id) {
            let missing: Vec<&str> = c.layers.iter().filter(|l| !given.iter().any(|(g, _)| g == *l)).map(String::as_str).collect();
            if !missing.is_empty() {
                let msg = format!(
                    "the first `::show` of `{}` with layers must set every layer; missing: {} (use `{}=none` to leave one empty)",
                    c.id,
                    missing.join(", "),
                    missing[0]
                );
                return Err(msg);
            }
        }
        self.layered_seen.insert(c.id.clone());
        self.show_rest(a, c.id.clone(), String::new(), layers, flip)
    }

    /// Report bare words beyond the first `allowed` (`::show{mika happy sad}`).
    fn extra_words(command: &str, bare: &[String], allowed: usize) -> Result<(), String> {
        match bare.get(allowed) {
            None => Ok(()),
            Some(extra) => {
                let what = match (command, allowed) {
                    ("show", 2) => "a character and a pose",
                    ("show", _) => "one sprite name (with `src=` or `anim=`, no pose)",
                    (_, 0) => "no sprite name with `all`",
                    _ => "one sprite name",
                };
                Err(format!("unexpected `{extra}`: `::{command}` takes {what}"))
            }
        }
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
        let (layers, layer_sprites) = (a.value("layers"), a.value("layer_sprites"));
        if let Some(extra) = a.leftover(&[]) {
            self.err(line, unknown_attr("character", extra));
        }
        let Some(id) = id else {
            self.err(line, "`::character` needs `id=...`");
            return;
        };
        if !is_plain_ident(&id) {
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
        if let Some(Err(m)) = sprites.as_deref().map(|t| check_inside("sprites", &t.replace("{pose}", "x"))) {
            self.err(line, m);
            return;
        }
        let sprites = sprites.map(|t| normal_path(&t));
        let layer_sprites = layer_sprites.map(|t| normal_path(&t));
        let layers = match self.character_layers(line, layers.as_deref(), layer_sprites.as_deref()) {
            Ok(l) => l,
            Err(m) => {
                self.err(line, m);
                return;
            }
        };
        let name = name.unwrap_or_else(|| id.clone());
        if let Err(m) = self.check_name_template(&name) {
            self.err(line, m);
            return;
        }
        self.program.characters.insert(key.clone(), Character { id: key, name, color, sprites, layers, layer_sprites });
    }

    /// A display name may hold `{variable}` of declared variables (shown with their value at the line);
    /// `\{` is a literal brace.
    fn check_name_template(&self, name: &str) -> Result<(), String> {
        let mut rest = name;
        while let Some(pos) = rest.find(['{', '\\']) {
            rest = &rest[pos..];
            if let Some(after) = rest.strip_prefix("\\{").or_else(|| rest.strip_prefix("\\\\")) {
                rest = after;
            } else if let Some(after) = rest.strip_prefix('{') {
                let Some(end) = after.find('}') else {
                    return Err(format!("character name `{name}` has a `{{` with no `}}` (write `\\{{` for a brace)"));
                };
                let var = after[..end].trim();
                if !self.program.vars.contains_key(var) {
                    return Err(format!(
                        "character name `{name}`: `{{{var}}}` is not a declared variable (declare it in the front matter `vars`)"
                    ));
                }
                rest = &after[end + 1..];
            } else {
                rest = &rest[1..];
            }
        }
        Ok(())
    }

    /// The layer ids of `layers=body,eyes,mouth`, checked together with the `layer_sprites=` template.
    fn character_layers(&self, _line: usize, layers: Option<&str>, template: Option<&str>) -> Result<Vec<String>, String> {
        let Some(list) = layers else {
            return match template {
                None => Ok(Vec::new()),
                Some(_) => Err("`layer_sprites=` needs `layers=body,eyes,...` (the layer ids, back to front)".into()),
            };
        };
        let Some(template) = template else {
            return Err("`layers=` needs `layer_sprites=assets/mika/{layer}_{value}.png` (where each layer's pictures are)".into());
        };
        let mut out: Vec<String> = Vec::new();
        for part in list.split(',') {
            let id = part.trim();
            if id.is_empty() {
                return Err("`layers=` has an empty entry (an extra comma?)".into());
            }
            if !is_plain_ident(id) || crate::unsafe_reason(id).is_some() {
                return Err(format!("layer id `{id}` must be letters, digits and `_`"));
            }
            if known_attrs("show").contains(&id) || id == LAYER_NONE {
                return Err(format!("`{id}` can't be a layer id: it is a `::show` attribute or keyword; pick another name"));
            }
            if out.iter().any(|o| o == id) {
                return Err(format!("layer `{id}` is listed twice"));
            }
            out.push(id.to_string());
        }
        if out.len() > MAX_LAYERS {
            return Err(format!("a character has at most {MAX_LAYERS} layers, not {}", out.len()));
        }
        for part in ["{layer}", "{value}"] {
            if !template.contains(part) {
                return Err(format!("`layer_sprites={template}` needs `{part}` in it"));
            }
        }
        let sample = template.replace("{layer}", "x").replace("{value}", "x");
        check_image_path(&sample, "layer_sprites", "a layer picture")?;
        Ok(out)
    }

    /// `::anim{id=blink frames="open.png 3, closed.png 0.12" loop}`: a named frame animation.
    fn anim(&mut self, line: usize, inner: &str) {
        let mut a = match Attrs::parse(inner) {
            Ok(a) => a,
            Err(m) => return self.err(line, m),
        };
        let (id, frames) = (a.value("id"), a.value("frames"));
        let looped = a.flag("loop");
        if let Some(extra) = a.leftover(&[]) {
            return self.err(line, unknown_attr("anim", extra));
        }
        let Some(id) = id else { return self.err(line, "`::anim` needs `id=...`") };
        if !is_plain_ident(&id) {
            return self.err(line, format!("animation id `{id}` must be letters, digits and `_`"));
        }
        let Some(frames) = frames else {
            return self.err(line, "`::anim` needs `frames=\"file.png seconds, file.png seconds\"` (in quotes)");
        };
        let mut list = Vec::new();
        for part in frames.split(',') {
            let words: Vec<&str> = part.split_whitespace().collect();
            let [file, seconds] = words[..] else {
                let shown = part.trim();
                let msg = if shown.is_empty() {
                    "`frames=` has an empty entry (an extra comma?): write `file.png seconds, file.png seconds`".to_string()
                } else {
                    format!("bad frame `{shown}`: each frame is a picture and how many seconds it stays, like `eyes_open.png 3`")
                };
                return self.err(line, msg);
            };
            let file = match check_image_path(file, "frames", "an animation frame") {
                Ok(f) => f,
                Err(m) => return self.err(line, m),
            };
            let seconds = match seconds.parse::<f32>() {
                Ok(s) if s.is_finite() && ANIM_FRAME_SECONDS.contains(&s) => s,
                _ => {
                    let (lo, hi) = (ANIM_FRAME_SECONDS.start(), ANIM_FRAME_SECONDS.end());
                    return self.err(line, format!("frame `{file} {seconds}`: the time must be a number of seconds from {lo} to {hi}"));
                }
            };
            list.push(AnimFrame { file, seconds });
        }
        if self.program.anims.contains_key(&id) {
            return self.err(line, format!("animation `{id}` is declared twice"));
        }
        self.program.anims.insert(id.clone(), Anim { id, frames: list, looped, line });
    }

    /// The picture `src` for `anim=id`: the animation must be declared before this line.
    fn anim_ref(&self, id: &str) -> Result<String, String> {
        if !self.program.anims.contains_key(id) {
            let hint = suggest(id, self.program.anims.keys().map(String::as_str), "animation");
            return Err(format!("unknown animation `{id}` ({hint}; declare it first with `::anim`)"));
        }
        Ok(anim_src(id))
    }

    /// `::item{id=key name="Brass key" icon=assets/key.png price=15 text="..." count=keys use=#unlock}`
    fn item(&mut self, line: usize, inner: &str) {
        let mut a = match Attrs::parse(inner) {
            Ok(a) => a,
            Err(m) => return self.err(line, m),
        };
        let (id, name, icon) = (a.value("id"), a.value("name"), a.value("icon"));
        let (price, text) = (a.value("price"), a.value("text"));
        let (count, use_label) = (a.value("count"), a.value("use"));
        if let Some(extra) = a.leftover(&[]) {
            return self.err(line, unknown_attr("item", extra));
        }
        let Some(id) = id else { return self.err(line, "`::item` needs `id=...`") };
        if id.is_empty() || !id.chars().all(|c| c.is_alphanumeric() || c == '_' || c == '-') {
            return self.err(line, format!("item id `{id}` must be letters, digits, `_` and `-`"));
        }
        if self.program.items.contains_key(&id) {
            return self.err(line, format!("item `{id}` is declared twice"));
        }
        let icon = match icon.map(|i| check_image_path(&i, "icon", "an item icon")).transpose() {
            Ok(i) => i,
            Err(m) => return self.err(line, m),
        };
        let price = match price.map(|p| parse_price(&p)).transpose() {
            Ok(p) => p,
            Err(m) => return self.err(line, m),
        };
        if let Some(c) = &count {
            if !matches!(self.program.vars.get(c), Some(Value::Int(_))) {
                return self.err(line, format!("`count={c}`: `{c}` must be a whole-number variable declared in `vars:`"));
            }
        }
        let use_label = match use_label.as_deref().map(|u| u.strip_prefix('#').filter(|l| !l.is_empty())) {
            Some(None) => return self.err(line, "`use=` names a label, like `use=#drink-potion`"),
            Some(Some(l)) => Some(l.to_string()),
            None => None,
        };
        if let Some(l) = &use_label {
            self.item_uses.push((l.clone(), line));
        }
        let name = name.filter(|n| !n.is_empty()).unwrap_or_else(|| id.clone());
        let item = Item { id: id.clone(), name, icon, price, text: text.unwrap_or_default(), count, use_label, line };
        self.program.items.insert(id, item);
    }

    /// The entry lines of the block `name` (`::choice`, `::hotspots` or `::shop`) that starts at `lines[start]`:
    /// `entry` gets each one that is not blank or a comment, with its line number. Reports a block that is never
    /// closed; returns the index of the closing `::` (or of the last line).
    fn block_lines(&mut self, lines: &[&str], start: usize, name: &str, mut entry: impl FnMut(&mut Self, &str, usize)) -> usize {
        let mut j = start + 1;
        let mut closed = false;
        while j < lines.len() {
            let t = lines[j].trim();
            if t == "::" {
                closed = true;
                break;
            }
            if t.starts_with("<!--") {
                if !t.ends_with("-->") {
                    self.err(self.base + j + 1, format!("a comment inside `{name}` must open and close on one line"));
                }
            } else if !t.is_empty() {
                entry(self, t, self.base + j + 1);
            }
            j += 1;
        }
        if !closed {
            self.err(self.base + start + 1, format!("`{name}` is never closed with a line containing only `::`"));
        }
        j
    }

    /// The `{...}` after an entry of a block: `after` names the entry and `goes` what its braces hold, for the message.
    fn entry_braces<'a>(tail: &'a str, after: &str, goes: &str) -> Result<&'a str, String> {
        tail.strip_prefix('{')
            .and_then(|x| x.strip_suffix('}'))
            .ok_or_else(|| format!("unexpected `{tail}` after the {after} ({goes} go in `{{...}}`)"))
    }

    /// `if <cond>` in an entry's braces (`None` for any other part; a variable merely starting with "if", like
    /// `iffy+=1`, is not a condition).
    fn entry_if(&self, part: &str) -> Option<Result<Expr, String>> {
        let text = part.strip_prefix("if").filter(|r| r.is_empty() || r.starts_with(char::is_whitespace))?;
        Some(parse_expr(text).and_then(|e| self.check_expr(&e).map(|()| e)))
    }

    /// `::shop{pay=gold into=inventory}` and its `- key{price=20, if met}` lines.
    fn shop(&mut self, lines: &[&str], start: usize, inner: &str) -> usize {
        let line = self.base + start + 1;
        let mut a = match Attrs::parse(inner) {
            Ok(a) => a,
            Err(m) => {
                self.err(line, m);
                Attrs(Vec::new())
            }
        };
        let (pay, into) = (a.value("pay"), a.value("into"));
        if let Some(extra) = a.leftover(&[]) {
            self.err(line, unknown_attr("shop", extra));
        }
        let pay = match pay {
            Some(p) if is_ident(&p) && !is_keyword(&p) => match self.program.vars.get(&p) {
                None | Some(Value::Int(_)) => Some(p),
                Some(_) => {
                    self.err(line, format!("`pay={p}`: `{p}` must be a whole-number variable"));
                    None
                }
            },
            Some(p) => {
                self.err(line, format!("`pay={p}` must name a variable"));
                None
            }
            None => {
                self.err(line, "`::shop` needs `pay=` (the number variable that pays, like `pay=gold`)");
                None
            }
        };
        let into = match into {
            Some(l) if self.is_list(&l) => Some(l),
            Some(l) => {
                let msg = format!("`into={l}`: `{l}` is not a list (declare it in the front matter: `vars: {{ {l}: [] }}`)");
                self.err(line, msg);
                None
            }
            None => {
                self.err(line, "`::shop` needs `into=` (the list that receives what is bought, like `into=inventory`)");
                None
            }
        };

        let mut stock: Vec<ShopEntry> = Vec::new();
        let mut lines_seen = 0;
        let j = self.block_lines(lines, start, "::shop", |p, t, ln| {
            lines_seen += 1;
            match p.shop_entry(t) {
                Ok(e) if stock.iter().any(|s| s.item == e.item) => p.err(ln, format!("`{}` is in this shop twice", e.item)),
                Ok(e) => stock.push(e),
                Err(m) => p.err(ln, m),
            }
        });
        if lines_seen == 0 {
            self.err(line, "`::shop` has no items");
        } else if stock.is_empty() {
            // every line was already reported
        } else if let (Some(pay), Some(into)) = (pay, into) {
            let (pay_kept, into_kept) = (self.program.persistent.contains(&pay), self.program.persistent.contains(&into));
            if pay_kept != into_kept {
                let (kept, plain) = if pay_kept { (&pay, &into) } else { (&into, &pay) };
                self.err(
                    line,
                    format!(
                        "`{kept}` is persistent but `{plain}` is not: rolling back a purchase would give the item twice or lose \
                         both the money and the item (make `{plain}` persistent too, or neither)"
                    ),
                );
            }
            // A bought counted item changes its count variable instead of the list.
            let counts: Vec<String> = stock.iter().filter_map(|e| self.program.items[&e.item].count.clone()).collect();
            for count in counts {
                if self.program.persistent.contains(&count) != pay_kept {
                    let msg = format!("`{count}` and `{pay}` must both be persistent or neither (rolling back a purchase would give the item twice or lose both)");
                    self.err(line, msg);
                }
            }
            self.push(Op::Shop { pay, into, stock }, line);
        }
        j
    }

    /// Parse `- key` or `- key{price=20, if met}`.
    fn shop_entry(&mut self, t: &str) -> Result<ShopEntry, String> {
        let t = t
            .strip_prefix("- ")
            .or_else(|| t.strip_prefix("* "))
            .ok_or("shop lines must start with `- `")?
            .trim();
        let end = t.find(|c: char| c == '{' || c.is_whitespace()).unwrap_or(t.len());
        let (key, tail) = (&t[..end], t[end..].trim());
        let Some(item) = self.program.items.get(key) else {
            return Err(format!("unknown item `{key}` (declare it first with `::item{{id={key}}}`)"));
        };
        let mut price = item.price;
        let mut cond = None;
        if !tail.is_empty() {
            let mut inner = Self::entry_braces(tail, "item", "settings")?.to_string();
            if let Some(p) = take_attr(&mut inner, "price") {
                price = Some(parse_price(&p)?);
            }
            for part in split_top_level(&inner, ',').into_iter().map(str::trim).filter(|p| !p.is_empty()) {
                match self.entry_if(part) {
                    Some(e) => cond = Some(e?),
                    None => return Err(format!("unexpected `{part}` (a shop line takes `price=` and `if condition`)")),
                }
            }
        }
        let price = price.ok_or_else(|| {
            format!("`{key}` has no price: give the item a `price=` or write `- {key}{{price=10}}`")
        })?;
        // A counted item can be bought again and again: a free one would never run out.
        if price == 0 && self.program.items[key].count.is_some() {
            return Err(format!("`{key}` has a `count=`, so a shop needs a price above 0 for it (a free one could be bought forever)"));
        }
        Ok(ShopEntry { item: key.to_string(), price, cond })
    }

    /// `::hud{...}`: create or replace an element, or `hide=id` / `clear`.
    fn hud(&mut self, line: usize, a: &mut Attrs) -> Result<(Op, Option<String>), String> {
        let has = |a: &Attrs, k: &str| a.0.iter().any(|(key, _)| key == k);
        let fade = Self::short_seconds(a, "fade", 0.0)?;
        let (clear, hide) = (a.flag("clear"), has(a, "hide"));
        if clear || hide {
            if clear && hide {
                return Err("`::hud` takes `clear` or `hide=id`, not both".into());
            }
            if let Some(extra) = a.0.iter().map(|(k, _)| k.as_str()).find(|k| *k != "hide") {
                let extra = extra.to_string();
                return Err(format!("`{extra}` can't be combined with `{}` (only `fade=` can)", if clear { "clear" } else { "hide" }));
            }
            if clear {
                return Ok((Op::HudClear { fade }, None));
            }
            return match a.value("hide") {
                Some(id) if is_plain_ident(&id) => Ok((Op::HudHide { id, fade }, None)),
                _ => Err("`::hud{hide=...}` needs the id of an element, like `::hud{hide=clock}`".into()),
            };
        }
        let id = match a.value("id") {
            Some(id) if is_plain_ident(&id) => id,
            Some(id) => return Err(format!("bad `id={id}` (an id is letters, digits and underscores, not starting with a digit)")),
            None => return Err("`::hud` needs `id=name` (the key that lets a later `::hud` replace or hide it)".into()),
        };
        let kinds: Vec<&str> = ["text", "bar", "image"].into_iter().filter(|k| has(a, k)).collect();
        if kinds.len() != 1 {
            let m = if kinds.is_empty() { "`::hud` needs one of `text=`, `bar=` or `image=`" } else { "`::hud` takes only one of `text=`, `bar=` and `image=`" };
            return Err(m.into());
        }
        let kind_name = kinds[0];
        // Attributes that belong to other kinds are mistakes, not silently ignored.
        let allowed: &[&str] = match kind_name {
            "text" => &["text", "size", "color", "back", "pad"],
            "bar" => &["bar", "max", "size", "color", "back", "border", "border_size"],
            _ => &["image", "scale"],
        };
        let common = ["id", "x", "y", "if", "call", "open", "hover"];
        let stray = a.0.iter().map(|(k, _)| k.as_str()).find(|k| !allowed.contains(k) && !common.contains(k)).map(str::to_string);
        if let Some(k) = stray {
            let known = known_attrs("hud");
            if known.contains(&k.as_str()) {
                return Err(format!("`{k}=` isn't used by a `{kind_name}` element (it takes: {}, {})", allowed.join(", "), common.join(", ")));
            }
            return Err(String::new()); // unknown: the caller reports it as an unknown attribute
        }
        let x = Self::place(a, "x", 0.0..=1.0)?.unwrap_or(0.0);
        let y = Self::place(a, "y", 0.0..=1.0)?.unwrap_or(0.0);
        let cond = match a.value("if") {
            None => None,
            Some(c) => Some(HudExpr::parse(&c).and_then(|e| self.check_expr(&e.expr).map(|()| e))?),
        };
        let action = self.hud_action(line, a, kind_name)?;
        let hover = match a.value("hover") {
            None => None,
            Some(_) if action.is_none() => return Err("`hover=` needs `call=#label` or `open=...` (it is the button's picture under the mouse)".into()),
            Some(_) if kind_name != "image" => return Err("`hover=` is a picture for an `image=` button (a text button lights up by itself)".into()),
            Some(h) => Some(check_image_path(&h, "hover", "a HUD hover image")?),
        };
        let colour = |a: &mut Attrs, key: &str| -> Result<Option<[f32; 4]>, String> {
            a.value(key).map(|v| parse_color_alpha(&v).map_err(|m| format!("`{key}`: {m}"))).transpose()
        };
        let kind = match kind_name {
            "text" => {
                let text = a.value("text").unwrap_or_default();
                if text.trim().is_empty() {
                    return Err("`::hud{text=...}` needs some text".into());
                }
                crate::markup::check_inline_markup(&text, "a `::hud` text")?;
                let size = Self::number(a, "size", 8.0..=300.0)?;
                let color = colour(a, "color")?;
                let back = colour(a, "back")?;
                let pad = Self::number(a, "pad", 0.0..=200.0)?.unwrap_or(0.0);
                HudKind::Text { text, size, color, back, pad }
            }
            "bar" => {
                let bar_src = a.value("bar").unwrap_or_default();
                let Some(max_src) = a.value("max") else {
                    return Err("`::hud{bar=...}` needs `max=` (the value that fills the bar: a number or an expression)".into());
                };
                let expr = |key: &str, v: &str| -> Result<HudExpr, String> {
                    HudExpr::parse(v).and_then(|e| self.check_expr(&e.expr).map(|()| e)).map_err(|m| format!("`{key}=`: {m}"))
                };
                let value = expr("bar", &bar_src)?;
                let max = expr("max", &max_src)?;
                let size = match a.value("size") {
                    None => return Err("`::hud{bar=...}` needs `size=width,height` (in pixels of the 1920x1080 screen, like `size=320,26`)".into()),
                    Some(v) => {
                        let parts: Vec<f32> = v.split(',').filter_map(|p| p.trim().parse::<f32>().ok().filter(|f| f.is_finite())).collect();
                        match parts.as_slice() {
                            [w, h] if (1.0..=1920.0).contains(w) && (1.0..=1080.0).contains(h) && v.split(',').count() == 2 => [*w, *h],
                            _ => return Err(format!("bad `size={v}` for a bar (width,height in pixels: `size=320,26`, at most 1920,1080)")),
                        }
                    }
                };
                let color = colour(a, "color")?;
                let back = colour(a, "back")?.unwrap_or([0.2, 0.2, 0.2, 1.0]);
                let border_color = colour(a, "border")?;
                let width = Self::number(a, "border_size", 1.0..=3.0)?;
                if width.is_some() && border_color.is_none() {
                    return Err("`border_size=` needs `border=` (the outline's colour)".into());
                }
                let border = border_color.map(|c| (c, width.unwrap_or(2.0)));
                HudKind::Bar { value, max, size, color, back, border }
            }
            _ => {
                let src = a.value("image").unwrap_or_default();
                if src.is_empty() {
                    return Err("`::hud{image=...}` needs a picture file".into());
                }
                let src = check_image_path(&src, "image", "a HUD image")?;
                let scale = Self::number(a, "scale", 0.05..=10.0)?.unwrap_or(1.0);
                HudKind::Image { src, scale }
            }
        };
        Ok((Op::Hud { elem: Box::new(HudElem { id, x, y, cond, kind, action, hover }), fade }, None))
    }

    /// `call=#label` or `open=journal`: what a press on the element does (none when neither is given).
    fn hud_action(&mut self, line: usize, a: &mut Attrs, kind_name: &str) -> Result<Option<HudAction>, String> {
        let (call, open) = (a.value("call"), a.value("open"));
        let action = match (call, open) {
            (None, None) => return Ok(None),
            (Some(_), Some(_)) => return Err("`::hud` takes `call=` or `open=`, not both".into()),
            _ if kind_name == "bar" => return Err("a bar can't be a button (`call=` and `open=` work on `text=` and `image=` elements)".into()),
            (Some(c), None) => match c.strip_prefix('#').filter(|l| !l.is_empty()) {
                Some(l) => {
                    self.hud_calls.push((l.to_string(), line));
                    HudAction::Call(l.to_string())
                }
                None => return Err("`call=` names a label, like `call=#phone`".into()),
            },
            (None, Some(o)) => {
                let Some(screen) = HudScreen::from_name(&o) else {
                    let names: Vec<&str> = HudScreen::NAMES.iter().map(|(n, _)| *n).collect();
                    return Err(format!("unknown `open={o}` (it can open: {})", names.join(", ")));
                };
                let missing = match screen {
                    HudScreen::Inventory if self.program.inventory.is_none() => Some(("an inventory", "inventory")),
                    HudScreen::Journal if self.program.journal.is_none() => Some(("a journal", "journal")),
                    _ => None,
                };
                if let Some((what, key)) = missing {
                    return Err(format!("`open={o}` needs {what} to open: add `{key}: <list variable>` to the front matter"));
                }
                HudAction::Open(screen)
            }
        };
        Ok(Some(action))
    }

    /// A picture name: a plain path inside the game folder, or a pattern whose
    /// `{variable}` parts name text variables (see `dynpath`). A variable the front
    /// matter doesn't declare is allowed, like in dialogue (a `::set` can create it;
    /// `quillstone test` warns when nothing sets it); a declared non-text one can never fit.
    fn picture_path(&self, key: &str, path: &str) -> Result<String, String> {
        if !crate::is_pattern(path) {
            return check_inside(key, path);
        }
        let parts = crate::pieces(path).map_err(|m| format!("`{key}`: {m}"))?;
        if kind_from_path(path) != AssetKind::Image {
            return Err(format!("`{key}={path}`: a name with a `{{variable}}` can only be a picture (.png, .jpg, .webp), not a video or sound"));
        }
        let mut sample = String::new();
        for part in parts {
            match part {
                crate::Piece::Lit(l) => sample.push_str(l),
                crate::Piece::Var(v) => {
                    match self.program.vars.get(v) {
                        Some(Value::Str(_)) | None => {}
                        Some(_) => {
                            return Err(format!(
                                "`{key}={path}`: `{v}` must be a text variable (the picture name is built from its text; \
                                 declare it with a text default, like `vars: {{ {v}: \"rooftop\" }}`)"
                            ))
                        }
                    }
                    sample.push('x');
                }
            }
        }
        if is_inside(&sample) {
            Ok(normal_path(path))
        } else {
            Err(format!("`{key}={path}`: a path must be relative and inside the game folder (no `..`, drive letters, backslashes or leading `/`)"))
        }
    }

    /// `::particles{snow}`, `{rain}`, `{src=file.png}`, with `amount=`, `speed=`, `wind=`, `layer=back`
    /// and `fade=`; `{none}` (or `off`) stops them.
    fn particles(a: &mut Attrs) -> Result<Op, String> {
        let kinds = ["snow", "rain", "none", "off"];
        let mut words = a.take_bare();
        if let Some(w) = words.iter().find(|w| !kinds.contains(&w.as_str())) {
            return Err(format!("`{w}` is not a kind of particle: use `snow`, `rain`, `src=<picture>` or `none`"));
        }
        let src = a.value("src");
        if src.is_some() {
            words.push("src".into());
        }
        if words.len() > 1 {
            return Err(format!("`::particles` takes one of `snow`, `rain`, `src=` or `none`, not {}", words.join(" and ")));
        }
        let fade = Self::seconds(a, "fade", 1.0)?;
        if fade > 10.0 {
            return Err(format!("`fade={fade}` must be at most 10 seconds"));
        }
        let kind = match (words.first().map(String::as_str), src) {
            (Some("none" | "off"), _) => {
                if let Some(extra) = ["amount", "speed", "wind", "layer"].into_iter().find(|k| a.0.iter().any(|(x, _)| x == k)) {
                    return Err(format!("`::particles{{none}}` takes only `fade=`, not `{extra}=`"));
                }
                return Ok(Op::Particles { set: None, fade });
            }
            (Some("snow"), _) => ParticleKind::Snow,
            (Some("rain"), _) => ParticleKind::Rain,
            (_, Some(src)) if src.is_empty() => return Err("`::particles` needs a picture in `src=...`".into()),
            (_, Some(src)) if src.contains(['{', '}']) => return Err(format!("`src={src}`: `::particles` can't use a `{{variable}}` in the picture name")),
            (_, Some(src)) => ParticleKind::Image(check_image_path(&src, "src", "a particle picture")?),
            _ => return Err("`::particles` needs `snow`, `rain`, `src=<picture>` or `none` (to stop them)".into()),
        };
        let amount = match a.value("amount") {
            None => DEFAULT_PARTICLES,
            Some(v) => match v.parse::<u32>() {
                Ok(n) if (1..=MAX_PARTICLES).contains(&n) => n,
                _ => return Err(format!("`amount={v}` must be a whole number from 1 to {MAX_PARTICLES}")),
            },
        };
        let speed = Self::number(a, "speed", PARTICLE_SPEED)?.unwrap_or(1.0);
        let wind = Self::number(a, "wind", PARTICLE_WIND)?.unwrap_or(0.0);
        let back = match a.value("layer").as_deref() {
            None | Some("front") => false,
            Some("back") => true,
            Some(v) => return Err(format!("`layer={v}` must be `front` (over the sprites) or `back` (behind them)")),
        };
        Ok(Op::Particles { set: Some(Particles { kind, amount, speed, wind, back }), fade })
    }

    /// `::tint{color=#203060 alpha=0.35 fade=1}` sets the wash; `::tint{none}` (or `off`) removes it.
    fn tint(a: &mut Attrs) -> Result<Op, String> {
        let (none, off) = (a.flag("none"), a.flag("off"));
        let fade = Self::seconds(a, "fade", 0.0)?;
        if fade > 10.0 {
            return Err(format!("`fade={fade}` must be at most 10 seconds"));
        }
        if none || off {
            if let Some(extra) = ["color", "alpha"].into_iter().find(|k| a.0.iter().any(|(x, _)| x == k)) {
                return Err(format!("`::tint{{none}}` takes only `fade=`, not `{extra}=`"));
            }
            return Ok(Op::Tint { set: None, fade });
        }
        let Some(c) = a.value("color") else {
            return Err("`::tint` needs `color=#rrggbb` (or `none` to remove the tint)".into());
        };
        let color = parse_color(&c)?;
        let alpha = Self::number(a, "alpha", TINT_ALPHA)?.unwrap_or(DEFAULT_TINT_ALPHA);
        Ok(Op::Tint { set: Some(Tint { color, alpha }), fade })
    }

    /// `result=` of `::timing` / `::press`: a text variable declared in the front matter.
    fn result_var(&self, a: &mut Attrs, cmd: &str) -> Result<String, String> {
        let var = a.value("result").unwrap_or_default();
        if !matches!(self.program.vars.get(&var), Some(Value::Str(_))) {
            return Err(format!("`::{cmd}` needs `result=` naming a text variable declared in the front matter (like `vars: {{ hit: \"\" }}`)"));
        }
        Ok(var)
    }

    /// The optional `prompt=` of `::timing` / `::press`: tagged text without `{nw}` / `{p}`.
    fn prompt_attr(a: &mut Attrs, cmd: &str) -> Result<String, String> {
        let prompt = a.value("prompt").map(|p| p.trim().to_string()).unwrap_or_default();
        crate::markup::check_inline_markup(&prompt, &format!("a `::{cmd}` prompt"))?;
        Ok(prompt)
    }

    fn require(&self, cmd: &str, a: &mut Attrs, key: &str) -> Result<String, String> {
        match a.value(key) {
            Some(v) if !v.is_empty() => if cmd == "bg" { self.picture_path(key, &v) } else { check_inside(key, &v) },
            _ => Err(format!("`::{cmd}` needs `{key}=...`")),
        }
    }

    fn fade(a: &mut Attrs) -> Result<f32, String> {
        Self::seconds(a, "fade", 0.0)
    }

    /// A non-negative number of seconds under `key`, or `default` when absent.
    fn seconds(a: &mut Attrs, key: &str, default: f32) -> Result<f32, String> {
        match a.value(key) {
            None => Ok(default),
            Some(v) => match v.parse::<f32>() {
                Ok(f) if f.is_finite() && f >= 0.0 => Ok(f),
                _ => Err(format!("`{key}={v}` must be a number of seconds, 0 or more")),
            },
        }
    }

    /// Like `seconds`, but above 0 and at most 10 (so an explicit `fade=0` is refused).
    fn short_seconds(a: &mut Attrs, key: &str, default: f32) -> Result<f32, String> {
        match a.value(key) {
            None => Ok(default),
            Some(v) => match v.parse::<f32>() {
                Ok(t) if t.is_finite() && t > 0.0 && t <= 10.0 => Ok(t),
                _ => Err(format!("`{key}={v}` must be a number of seconds above 0 and at most 10")),
            },
        }
    }

    /// An optional number under `key` within `range` (`50%` means 0.5); `None` means absent.
    fn number(a: &mut Attrs, key: &str, range: RangeInclusive<f32>) -> Result<Option<f32>, String> {
        let Some(v) = a.value(key) else { return Ok(None) };
        let parsed = match v.strip_suffix('%') {
            Some(pct) => pct.parse::<f32>().map(|p| p / 100.0),
            None => v.parse::<f32>(),
        };
        match parsed {
            Ok(f) if f.is_finite() && range.contains(&f) => Ok(Some(f)),
            _ => Err(format!("`{key}={v}` must be a number from {} to {}", range.start(), range.end())),
        }
    }

    /// `x=` / `y=`: `left` `center` `right` (`top` `center` `bottom`), a number (1 is 100%) or `N%`. The point that
    /// fraction across the object is that fraction across the screen (CSS `background-position`).
    fn place(a: &mut Attrs, key: &str, range: RangeInclusive<f32>) -> Result<Option<f32>, String> {
        let Some(v) = a.value(key) else { return Ok(None) };
        let words = if key == "x" { [("left", 0.0), ("center", 0.5), ("right", 1.0)] } else { [("top", 0.0), ("center", 0.5), ("bottom", 1.0)] };
        let named = words.iter().find(|w| w.0 == v || (v == "centre" && w.0 == "center")).map(|w| w.1);
        let number = || match v.strip_suffix('%') {
            Some(pct) => pct.parse::<f32>().ok().map(|p| p / 100.0),
            None => v.parse::<f32>().ok(),
        };
        match named.or_else(number).filter(|f| f.is_finite() && range.contains(f)) {
            Some(f) => Ok(Some(f)),
            None => {
                let (lo, hi) = (range.start() * 100.0, range.end() * 100.0);
                let gone = if v.starts_with("far-") { format!("; `far-left` and `far-right` are gone: try `{key}=12%` or `{key}=88%`") } else { String::new() };
                Err(format!("bad `{key}={v}` (use {}, {} or {}, a number or a percentage from {lo}% to {hi}%){gone}", words[0].0, words[1].0, words[2].0))
            }
        }
    }

    fn ease(a: &mut Attrs) -> Result<Ease, String> {
        match a.value("ease") {
            None => Ok(Ease::default()),
            Some(v) => Ease::parse(&v).ok_or_else(|| format!("`ease={v}` must be `linear`, `in`, `out` or `inout`")),
        }
    }

    fn volume(a: &mut Attrs) -> Result<f32, String> {
        match a.value("volume") {
            None => Ok(1.0),
            Some(v) => match v.parse::<f32>() {
                Ok(f) if (0.0..=1.0).contains(&f) => Ok(f),
                _ => Err(format!("`volume={v}` must be between 0 and 1")),
            },
        }
    }

    // ---- if / elif / else / endif ---------------------------------------

    fn condition(&mut self, line: usize, text: &str) -> Expr {
        match parse_expr(text).and_then(|e| self.check_expr(&e).map(|()| e)) {
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

    fn choice(&mut self, lines: &[&str], start: usize, inner: &str, hot: bool) -> usize {
        let line = self.base + start + 1;
        let name = if hot { "::hotspots" } else { "::choice" };
        let mut time = None;
        let mut default_to = None;
        let mut map = None;
        match Attrs::parse(inner) {
            Ok(mut a) => {
                let value = a.take("time");
                let jump = if hot { a.take("default") } else { None };
                let (ground, hover) = if hot { (a.take("ground"), a.take("hover")) } else { (None, None) };
                match (ground, hover) {
                    (None, None) => {}
                    (ground, Some(hover)) => {
                        let picture = |key: &str, v: Option<String>| {
                            let v = v.ok_or_else(|| format!("`{key}=` needs a picture, like `{key}=assets/map.png`"))?;
                            check_image_path(&v, key, "an imagemap picture")
                        };
                        match (ground.map(|g| picture("ground", g)).transpose(), picture("hover", hover)) {
                            (Ok(ground), Ok(hover)) => map = Some(Imagemap { ground, hover }),
                            (g, h) => [g.err(), h.err()].into_iter().flatten().for_each(|m| self.err(line, m)),
                        }
                    }
                    (Some(_), None) => self.err(line, "`ground=` needs `hover=` too: the picture shown over a region while it is hovered"),
                }
                if let Some(extra) = a.leftover(&[]) {
                    self.err(line, unknown_attr(name.trim_start_matches("::"), extra));
                }
                match value {
                    None => {}
                    Some(v) => match v.as_deref().map(parse_choice_time) {
                        Some(Ok(t)) => time = Some(t),
                        Some(Err(m)) => self.err(line, m),
                        None => self.err(line, format!("`time` needs a number of seconds, like `{name}{{time=8}}`")),
                    },
                }
                match jump.map(|j| j.as_deref().and_then(|j| j.strip_prefix('#')).map(slug)) {
                    None => {}
                    Some(Some(label)) if !label.is_empty() => default_to = Some(label),
                    Some(_) => self.err(line, "`default` needs a label, like `default=#too-slow`"),
                }
            }
            Err(m) => self.err(line, m),
        }
        let mut options = Vec::new();
        let mut labels = Vec::new();
        let j = self.block_lines(lines, start, name, |p, t, ln| match p.option(t, hot, ln) {
            Ok((opt, label)) => {
                options.push(opt);
                labels.push((label, ln));
            }
            Err(m) => p.err(ln, m),
        });
        if hot && time.is_some() != default_to.is_some() {
            let msg = if time.is_some() {
                "a timed `::hotspots` needs `default=#label`: where to go when time runs out"
            } else {
                "`default=#label` needs a timed block (`::hotspots{time=3 default=#too-slow}`)"
            };
            self.err(line, msg);
        }
        let defaults: Vec<usize> = options.iter().enumerate().filter(|(_, o)| o.default).map(|(k, _)| k).collect();
        if !hot && time.is_some() && defaults.is_empty() && !options.is_empty() {
            self.err(line, "a timed `::choice` needs one option marked `{default}`: the one picked when time runs out");
        }
        for &k in defaults.iter().skip(1) {
            self.err(options[k].line, "only one option of a `::choice` can be the `{default}`");
        }
        if time.is_none() {
            for &k in defaults.iter().take(1) {
                self.err(options[k].line, "`default` needs a timed choice (`::choice{time=8}`)");
            }
        }
        if options.is_empty() {
            self.err(line, if hot { "`::hotspots` has no regions" } else { "`::choice` has no options" });
        } else {
            let op = self.here();
            if let Some(label) = default_to.filter(|_| time.is_some()) {
                // The timeout is one more outcome, like a timed choice's default: a hidden option
                // nothing can click, so the explorer, graph, rollback and saves need no new case.
                options.push(ChoiceOption { text: String::new(), target: UNSET, effects: Vec::new(), cond: None, area: Some(Area::timeout()), default: true, lock: None, line });
                labels.push((label, line));
            }
            for (k, (label, ln)) in labels.into_iter().enumerate() {
                self.fixups.push(Fixup { op, option: Some(k), label, line: ln });
            }
            self.push(Op::Choice { options, time, map }, line);
        }
        j
    }

    /// Parse `- [text](#label){var+=1, if met}`.
    fn option(&mut self, t: &str, hot: bool, line: usize) -> Result<(ChoiceOption, String), String> {
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
        crate::markup::check_option_markup(text)?;
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
        let mut area = None;
        let mut hidden = false;
        let mut call = false;
        let mut default = false;
        let mut show: Option<String> = None;
        if hot && tail.is_empty() {
            return Err("a hotspot needs `{rect=x,y,w,h}` or `{sprite=id}` after its label".into());
        }
        if !tail.is_empty() {
            let mut inner = Self::entry_braces(tail, "option", "effects")?.to_string();
            if hot {
                let rect = take_attr(&mut inner, "rect");
                let image = take_attr(&mut inner, "image");
                let hover = take_attr(&mut inner, "hover");
                let sprite = take_attr(&mut inner, "sprite");
                if sprite.as_deref() == Some("") {
                    return Err("`sprite=` needs a sprite name, like `sprite=firefly`".into());
                }
                // Following a sprite, `rect=` is relative to it and the whole sprite when left out.
                let rect = match (&sprite, rect) {
                    (Some(_), None) => [0.0, 0.0, 1.0, 1.0],
                    (_, Some(r)) => parse_rect(&r)?,
                    (None, None) => return Err("a hotspot needs `rect=x,y,w,h` (each 0 to 1 or a percentage) or `sprite=id`".into()),
                };
                let sprite = sprite.map(|s| self.sprite_id(&s));
                if let Some(id) = &sprite {
                    self.sprite_refs.push((id.clone(), line));
                }
                let image = image.map(|i| check_hotspot_image(&i)).transpose()?;
                let hover = hover.map(|h| check_image_path(&h, "hover", "a hotspot hover picture")).transpose()?;
                area = Some(Area { rect, sprite, image, hover, call: false, hidden: false });
            } else if ["rect", "image", "sprite", "hover"].iter().any(|k| take_attr(&mut inner.clone(), k).is_some()) {
                return Err("`rect=`, `image=`, `hover=` and `sprite=` only belong in a `::hotspots` block".into());
            }
            let inner = inner.as_str();
            for item in split_top_level(inner, ',').into_iter().map(str::trim).filter(|p| !p.is_empty()) {
                if let Some(e) = self.entry_if(item) {
                    cond = Some(e?);
                } else if item == "once" {
                    once = true;
                } else if item == "default" {
                    if hot {
                        return Err("`default` only belongs in a timed `::choice`, not a `::hotspots` block".into());
                    }
                    default = true;
                } else if item == "show" || item.starts_with("show=") {
                    if hot {
                        return Err("`show` only belongs in a `::choice`, not a `::hotspots` block".into());
                    }
                    show = Some(match item.strip_prefix("show=") {
                        None => String::new(),
                        Some(q) => {
                            let reason = q.strip_prefix('"').and_then(|r| r.strip_suffix('"')).filter(|r| !r.contains('"'));
                            let reason = reason.ok_or("the reason of `show=` goes in double quotes, like `show=\"Needs STR 5\"`")?;
                            crate::markup::check_option_markup(reason)?;
                            reason.to_string()
                        }
                    });
                } else if item == "hidden" {
                    if !hot {
                        return Err("`hidden` only belongs in a `::hotspots` block".into());
                    }
                    hidden = true;
                } else if item == "call" {
                    // A `::choice` button would need its own way back to the line; only regions come back to a screen.
                    if !hot {
                        return Err("`call` only belongs in a `::hotspots` block".into());
                    }
                    call = true;
                } else {
                    let e = parse_effect(item)?;
                    self.check_effect(&e)?;
                    effects.push(e);
                }
            }
        }
        if let Some(a) = area.as_mut() {
            a.hidden = hidden;
            a.call = call;
        }
        if show.is_some() && cond.is_none() {
            return Err("`show` needs an `if`: it greys the option out while the condition is false".into());
        }
        let target = slug(target);
        let mut shown_if = None;
        if once {
            // Offered until picked: "not picked yet" joins the condition, and picking
            // it remembers that. Keyed by target and text, so it survives script edits.
            let not_picked = Expr::Not(Box::new(Expr::Var(format!("{ONCE}{target}:{text}"))));
            shown_if = Some(not_picked.clone());
            cond = Some(match cond {
                Some(c) => Expr::And(Box::new(c), Box::new(not_picked)),
                None => not_picked,
            });
            let var = format!("{ONCE}{target}:{text}");
            effects.push(Effect { var, op: AssignOp::Set, value: Expr::Lit(Value::Bool(true)) });
        }
        let lock = show.map(|reason| Lock { reason, shown_if });
        let opt = ChoiceOption { text: text.to_string(), target: UNSET, effects, cond, area, default, lock, line };
        Ok((opt, target))
    }
}

// ---- free helpers --------------------------------------------------------

/// Remove `key=value` (value up to whitespace or a comma, except that `rect`
/// takes its four comma-separated numbers) from `inner` and return the value.
fn take_attr(inner: &mut String, key: &str) -> Option<String> {
    let needle = format!("{key}=");
    let mut from = 0;
    while let Some(pos) = inner[from..].find(&needle).map(|p| p + from) {
        let boundary = inner[..pos].chars().next_back().is_none_or(|c| c.is_whitespace() || c == ',');
        if !boundary {
            from = pos + needle.len();
            continue;
        }
        let start = pos + needle.len();
        let len = inner[start..]
            .find(|c: char| c.is_whitespace() || (key != "rect" && c == ','))
            .unwrap_or(inner.len() - start);
        let value = inner[start..start + len].trim_end_matches(',').to_string();
        inner.replace_range(pos..start + len, "");
        return Some(value);
    }
    None
}

/// `time=` of a `::choice`: a plain number of seconds, `MIN_CHOICE_TIME` to `MAX_CHOICE_TIME_CAP` (above
/// `MAX_CHOICE_TIME`, `check` warns).
fn parse_choice_time(s: &str) -> Result<f32, String> {
    s.parse::<f32>()
        .ok()
        .filter(|t| t.is_finite() && (MIN_CHOICE_TIME..=MAX_CHOICE_TIME_CAP).contains(t))
        .ok_or_else(|| {
            format!("bad `time={s}` (a number of seconds from {MIN_CHOICE_TIME} to {MAX_CHOICE_TIME_CAP}, not a variable)")
        })
}

/// `40%..60%` (or `0.4..0.6`): a zone of the timing bar, `from` below `to`.
fn parse_zone(s: &str) -> Result<[f32; 2], &'static str> {
    let both = s.split_once("..").and_then(|(a, b)| Some([parse_fraction(a.trim())?, parse_fraction(b.trim())?]));
    both.filter(|z| z[0] < z[1]).ok_or("a zone like `40%..60%`, from 0% to 100%, the first end below the second")
}

/// `x,y,w,h`, each 0..1 or a percentage; the region must lie inside the screen.
fn parse_rect(s: &str) -> Result<[f32; 4], String> {
    let parts: Vec<&str> = s.split(',').collect();
    let bad = || format!("bad `rect={s}` (use four numbers x,y,w,h, each 0 to 1 or a percentage, no spaces)");
    if parts.len() != 4 {
        return Err(bad());
    }
    let mut r = [0.0; 4];
    for (slot, part) in r.iter_mut().zip(&parts) {
        *slot = parse_fraction(part).ok_or_else(bad)?;
    }
    let [x, y, w, h] = r;
    if w <= 0.0 || h <= 0.0 {
        return Err(format!("`rect={s}` has no size (width and height must be above 0)"));
    }
    if x + w > 1.0 + 1e-4 || y + h > 1.0 + 1e-4 {
        return Err(format!("`rect={s}` reaches outside the screen (x+w and y+h must be at most 1)"));
    }
    Ok(r)
}

fn check_hotspot_image(path: &str) -> Result<String, String> {
    check_image_path(path, "image", "a hotspot image")
}

/// `path` must name a .png, .jpg or .webp file (`key` and `what` are for the message).
/// The argument of an `::include{..}` line, or `None` for any other line.
fn include_args(t: &str) -> Option<&str> {
    let rest = t.strip_prefix("::include")?.trim();
    if rest.is_empty() {
        return Some("");
    }
    rest.strip_prefix('{')?.strip_suffix('}')
}

/// `chapters/ch1.quill.md`, `src=chapters/ch1.quill.md` or either in quotes.
fn include_path(inner: &str) -> Option<String> {
    let t = inner.trim();
    let t = t.strip_prefix("src=").unwrap_or(t).trim();
    let t = t.strip_prefix('"').and_then(|r| r.strip_suffix('"')).unwrap_or(t);
    (!t.is_empty()).then(|| t.to_string())
}

fn check_image_path(path: &str, key: &str, what: &str) -> Result<String, String> {
    let path = check_inside(key, path)?;
    let ext = path.rsplit('.').next().unwrap_or("").to_ascii_lowercase();
    if !path.contains('.') || !crate::dynpath::PICTURE_EXTENSIONS.contains(&ext.as_str()) {
        return Err(format!("bad `{key}={path}` ({what} must be a .png, .jpg or .webp file)"));
    }
    Ok(path)
}

/// A file named by the script must be relative and inside the game folder, or a save
/// that mentions it can't be loaded again (and the game can't be packaged). Returns the path
/// normalised, which is the spelling the program keeps.
fn check_inside(key: &str, path: &str) -> Result<String, String> {
    if path.contains(['{', '}']) {
        Err(format!(
            "`{key}={path}`: braces can't be part of a file name; a `{{variable}}` in a name works only for pictures \
             (`::show`, `::bg` and inline images)"
        ))
    } else if is_inside(path) {
        Ok(normal_path(path))
    } else {
        Err(format!(
            "`{key}={path}`: a path must be relative and inside the game folder (no `..`, drive letters, backslashes or leading `/`)"
        ))
    }
}

/// A price: a whole number, 0 or more.
fn parse_price(s: &str) -> Result<i64, String> {
    s.trim()
        .parse::<i64>()
        .ok()
        .filter(|p| *p >= 0)
        .ok_or_else(|| format!("bad `price={s}` (a price is a whole number, 0 or more)"))
}

/// `0.3` or `30%` as 0..1.
fn parse_fraction(s: &str) -> Option<f32> {
    let (num, scale) = match s.strip_suffix('%') {
        Some(n) => (n, 100.0),
        None => (s, 1.0),
    };
    num.trim().parse::<f32>().ok().map(|v| v / scale).filter(|v| (0.0..=1.0).contains(v))
}

/// `heal(amount, who)` -> (`heal`, Some(`amount, who`)). Parameters count only when
/// the `(` follows the name directly and the heading ends with `)`, so a heading
/// like `# Chapter 1 (the start)` is just a title, as it always was.
fn split_params(text: &str) -> (&str, Option<&str>) {
    if let Some(body) = text.strip_suffix(')') {
        if let Some(open) = body.find('(') {
            if body[..open].chars().next_back().is_some_and(|c| !c.is_whitespace()) {
                return (&body[..open], Some(&body[open + 1..]));
            }
        }
    }
    (text, None)
}

/// Problems with the arguments of a `::call` to `label`, whose parameters are `params`.
fn call_problems(label: &str, params: Option<&[String]>, defaults: &BTreeMap<String, Value>, args: &[(String, Expr)]) -> Vec<String> {
    let Some(params) = params else {
        return args
            .iter()
            .map(|(k, _)| format!("`{label}` takes no parameters, but this `::call` passes `{k}`"))
            .collect();
    };
    let takes = format!(
        "{label} takes: {}",
        params.iter().map(|p| if defaults.contains_key(p) { format!("{p} (optional)") } else { p.clone() }).collect::<Vec<_>>().join(", ")
    );
    let mut out = Vec::new();
    for (k, _) in args.iter().filter(|(k, _)| !params.contains(k)) {
        out.push(format!("`{label}` has no parameter `{k}` ({takes})"));
    }
    for p in params.iter().filter(|p| !defaults.contains_key(*p) && !args.iter().any(|(k, _)| k == *p)) {
        out.push(format!("this `::call` is missing `{p}={{...}}` ({takes})"));
    }
    out
}

/// Added to errors about reaching a label with parameters, for headings that were
/// meant as plain titles.
const PARAM_HINT: &str = "(`(a)` right after the name is read as a parameter list; put a space before `(` for a plain title)";

/// A jump or link can't give a parameterized label its values.
fn needs_call(label: &str, params: Option<&[String]>, what: &str) -> Option<String> {
    let params = params?;
    Some(format!(
        "`{label}` has parameters ({}), so only `::call{{to={label} ...}}` can go there; {what} can't supply them {PARAM_HINT}",
        params.join(", ")
    ))
}

/// Split the inside of `::call{...}` into `key=value` words and bare words, at
/// spaces and commas that are outside quotes and parentheses.
fn call_tokens(s: &str) -> Result<Vec<(String, Option<String>)>, String> {
    let mut words: Vec<String> = Vec::new();
    let (mut cur, mut quote, mut depth) = (String::new(), None::<char>, 0i32);
    for c in s.chars() {
        match (quote, c) {
            (Some(q), c) => {
                cur.push(c);
                if c == q {
                    quote = None;
                }
            }
            (None, '"' | '\'') => {
                quote = Some(c);
                cur.push(c);
            }
            (None, '(') => {
                depth += 1;
                cur.push(c);
            }
            (None, ')') => {
                depth -= 1;
                cur.push(c);
            }
            (None, c) if (c.is_whitespace() || c == ',') && depth <= 0 => {
                if !cur.is_empty() {
                    words.push(std::mem::take(&mut cur));
                }
            }
            (None, c) => cur.push(c),
        }
    }
    if quote.is_some() {
        return Err("unterminated quote in `::call`".into());
    }
    if !cur.is_empty() {
        words.push(cur);
    }
    words
        .into_iter()
        .map(|w| match w.split_once('=') {
            Some(("", _)) => Err("expected an argument name before `=`".to_string()),
            Some((k, v)) => Ok((k.to_string(), Some(v.to_string()))),
            None => Ok((w, None)),
        })
        .collect()
}

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
pub fn slug(s: &str) -> String {
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

/// `\"` and `\\` inside a quoted title are the character itself; any other backslash stays.
/// The `id:` front matter key becomes a folder name on every system, so it is plain and short.
fn check_game_id(id: &str) -> Result<(), String> {
    let plain = id.bytes().all(|b| matches!(b, b'a'..=b'z' | b'0'..=b'9' | b'-'));
    if !plain || id.is_empty() || id.len() > 64 || is_device_name(id) {
        return Err(format!(
            "`id` is the name of the save folder: 1 to 64 lowercase letters, digits and `-`, and not a name Windows keeps \
             for devices (like `id: my-game`); got `{id}`"
        ));
    }
    Ok(())
}

fn unescape_title(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        match (c, chars.peek()) {
            ('\\', Some(&n)) if matches!(n, '"' | '\'' | '\\') => {
                out.push(n);
                chars.next();
            }
            _ => out.push(c),
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

/// The colour under `key` (`#rrggbb`), if given.
fn color_attr(a: &mut Attrs, key: &str) -> Result<Option<[f32; 3]>, String> {
    a.value(key).map(|c| parse_color(&c)).transpose()
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

/// `#rgb`, `#rgba`, `#rrggbb` or `#rrggbbaa` -> 0..1 RGBA (opaque without an alpha part).
pub(crate) fn parse_color_alpha(s: &str) -> Result<[f32; 4], String> {
    let bad = || format!("bad colour `{s}` (use #rrggbb, #rrggbbaa, #rgb or #rgba)");
    let hex = s.strip_prefix('#').ok_or_else(bad)?;
    let d: Vec<u32> = hex.chars().map(|c| c.to_digit(16)).collect::<Option<_>>().ok_or_else(bad)?;
    let byte = |hi: u32, lo: u32| (hi * 16 + lo) as f32 / 255.0;
    match d.as_slice() {
        [r1, r2, g1, g2, b1, b2] => Ok([byte(*r1, *r2), byte(*g1, *g2), byte(*b1, *b2), 1.0]),
        [r1, r2, g1, g2, b1, b2, a1, a2] => Ok([byte(*r1, *r2), byte(*g1, *g2), byte(*b1, *b2), byte(*a1, *a2)]),
        [r, g, b] => Ok([byte(*r, *r), byte(*g, *g), byte(*b, *b), 1.0]),
        [r, g, b, a] => Ok([byte(*r, *r), byte(*g, *g), byte(*b, *b), byte(*a, *a)]),
        _ => Err(bad()),
    }
}

/// A flag that may be absent: `flip` or `flip=true` is `Some(true)`, `flip=false` is `Some(false)`.
fn opt_flag(a: &mut Attrs, key: &str) -> Option<bool> {
    a.take(key).map(|v| v.is_none_or(|v| !matches!(v.as_str(), "false" | "0" | "no")))
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

    /// The first attribute nobody took, other than those in `ignore`.
    fn leftover(&self, ignore: &[&str]) -> Option<&str> {
        self.0.iter().map(|(k, _)| k.as_str()).find(|k| !ignore.contains(k))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_util::ok;

    const DEMO: &str = include_str!("../../../examples/demo.quill.md");

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
        assert_eq!(p.ops[0], Op::Bg { src: "b.png".into(), fade: 1.0, wipe: Some(Wipe::Right), sound: false });
        assert_eq!(p.ops[1], Op::Move { id: "mika".into(), flip: None, steps: vec![MoveStep { x: None, y: None, scale: Some(1.2), xscale: None, yscale: None, alpha: None, dx: None, dy: None, time: 0.5, ease: Ease::Out, wait: 0.0 }], looped: false });
        assert_eq!(p.ops[2], Op::Shake { time: 0.4, strength: 20.0, axis: Axis::Both });
        assert_eq!(p.ops[3], Op::Shake { time: 1.0, strength: 40.0, axis: Axis::Both });
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
    fn textbox_commands() {
        let p = ok("::textbox{hide}\n::textbox{show fade=0.5}\n::textbox{hide fade=2}\n");
        assert_eq!(p.ops[0], Op::Textbox { hidden: true, fade: 0.0 });
        assert_eq!(p.ops[1], Op::Textbox { hidden: false, fade: 0.5 });
        assert_eq!(p.ops[2], Op::Textbox { hidden: true, fade: 2.0 });
    }

    #[test]
    fn textbox_errors_name_the_line() {
        for (src, needle) in [
            ("::textbox\n", "needs `hide` or `show`"),
            ("::textbox{hide show}\n", "not both"),
            ("::textbox{hide fade=0}\n", "fade=0"),
            ("::textbox{show fade=11}\n", "fade=11"),
            ("::textbox{hide fade=soon}\n", "fade=soon"),
            ("::textbox{hide speed=2}\n", "unknown attribute `speed`"),
        ] {
            let e = errs(&format!("Hi.\n{src}"));
            assert_eq!(e[0].line, 2, "{src}");
            assert!(e[0].message.contains(needle), "{src}: {}", e[0].message);
        }
    }

    #[test]
    fn animation_errors() {
        for (src, needle) in [
            ("::bg{src=b.png wipe=sideways fade=1}\n", "wipe=sideways"),
            ("::bg{src=b.png wipe=left}\n", "needs a duration"),
            ("::move{mika}\n", "`wait=` and/or `flip`"),
            ("::move{mika x=left ease=bouncy}\n", "ease=bouncy"),
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
    fn comments_inside_choices() {
        let p = ok("::choice\n<!-- the kind one -->\n- [Yes](#a)\n- [No](#a)\n::\n# a\n");
        assert!(matches!(&p.ops[0], Op::Choice { options, .. } if options.len() == 2));
        assert!(has_err("::choice\n<!-- open\n- [Yes](#a)\n::\n# a\n", "on one line"));
    }

    #[test]
    fn keywords_are_not_variable_names() {
        assert!(has_err("::set{true=1}\n", "keyword"));
        assert!(has_err("::set{and=1}\n", "keyword"));
        assert!(has_err("---\nvars: { not: 1 }\n---\n", "keyword"));
        ok("::set{seen=1, android=2}\n");
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
        assert_eq!(p.ops[0], Op::Bg { src: "a.webp".into(), fade: 0.5, wipe: None, sound: false });
        assert_eq!(
            p.ops[1],
            Op::Video { src: "my clip.webm".into(), fit: Fit::Cover, looped: true, skippable: true }
        );
        assert_eq!(p.ops[2], Op::StopMusic { fade: 2.0 });
    }

    #[test]
    fn music_loops_by_default_and_takes_a_volume() {
        let p = ok("::music{src=a.ogg}\n::music{src=b.ogg loop=false volume=0.4 fade=1}\n");
        assert_eq!(p.ops[0], Op::Music { src: "a.ogg".into(), intro: None, fade: 0.0, looped: true, volume: 1.0 });
        assert_eq!(p.ops[1], Op::Music { src: "b.ogg".into(), intro: None, fade: 1.0, looped: false, volume: 0.4 });
    }

    #[test]
    fn sfx_voice_and_wait() {
        let p = ok("::sfx{src=door.ogg volume=0.5}\n::voice{src=v1.ogg}\n::wait{time=1.5}\n::wait{2}\n");
        assert_eq!(p.ops[0], Op::Sfx { src: "door.ogg".into(), volume: 0.5 });
        assert_eq!(p.ops[1], Op::Voice { src: "v1.ogg".into(), volume: 1.0 });
        assert_eq!(p.ops[2], Op::Wait { seconds: 1.5 });
        assert_eq!(p.ops[3], Op::Wait { seconds: 2.0 });
    }

    #[test]
    fn wait_click_is_its_own_op() {
        assert_eq!(ok("::wait{click}\n").ops[0], Op::WaitClick);
        assert!(has_err("::wait{click=1}\n", "positive number"));
    }

    #[test]
    fn an_unknown_label_gets_a_few_close_names_not_all() {
        let many: String = (0..300).map(|i| format!("# scene_{i}\nHi.\n")).collect();
        let e = errs(&format!("{many}# bar\nHi.\n::jump{{to=scen-7}}\n"));
        let m = &e[0].message;
        assert!(m.contains("did you mean `scene-7`") && m.contains("301 labels in all") && m.len() < 200, "{m}");
        let e = errs("# alpha\nHi.\n::jump{to=zzzzzz}\n");
        assert!(e[0].message.contains("(1 label exists, none close to it)"), "{:?}", e[0].message);
        assert!(errs("::jump{to=x}\n")[0].message.contains("no labels yet"));
    }

    #[test]
    fn media_argument_errors() {
        assert!(has_err("::wait{time=0}\n", "positive number"));
        assert!(has_err("::wait\n", "positive number"));
        assert!(has_err("::sfx{src=a.ogg volume=3}\n", "between 0 and 1"));
        assert!(has_err("::music{src=a.ogg fade=-1}\n", "seconds"));
        assert!(has_err("::sfx{}\n", "needs `src=...`"));
    }

    #[test]
    fn a_voice_takes_a_volume_between_0_and_1() {
        assert!(matches!(ok("::voice{src=v.ogg volume=0.5}\n").ops[0], Op::Voice { volume, .. } if volume == 0.5));
        assert!(has_err("::voice{src=v.ogg volume=2}\n", "between 0 and 1"));
    }

    #[test]
    fn a_background_video_may_ask_for_its_sound_but_a_picture_may_not() {
        assert!(matches!(ok("::bg{src=a.webm sound}\n").ops[0], Op::Bg { sound: true, .. }));
        assert!(matches!(ok("::bg{src=a.webm}\n").ops[0], Op::Bg { sound: false, .. }));
        assert!(has_err("::bg{src=a.png sound}\n", "video"));
        assert!(has_err("::bg{color=#fff sound}\n", "video"));
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
        let p = ok(&format!("{MIKA}::show{{mika happy x=left fade=0.4 scale=0.9}}\n::show{{mika}}\n"));
        assert_eq!(
            p.ops[0],
            Op::Show { id: "mika".into(), src: "art/mika_happy.png".into(), layers: vec![], x: Some(0.0), y: None, z: None, scale: Some(0.9), flip: None, xscale: None, yscale: None, alpha: None, dx: None, dy: None, fade: 0.4 }
        );
        // default pose, keeps position
        assert_eq!(
            p.ops[1],
            Op::Show { id: "mika".into(), src: "art/mika_default.png".into(), layers: vec![], x: None, y: None, z: None, scale: None, flip: None, xscale: None, yscale: None, alpha: None, dx: None, dy: None, fade: 0.0 }
        );
    }

    #[test]
    fn show_with_an_explicit_file_and_numeric_positions() {
        let p = ok("::show{src=art/ghost.png id=ghost x=0.3}\n::show{src=art/x.png x=25%}\n");
        assert_eq!(
            p.ops[0],
            Op::Show { id: "ghost".into(), src: "art/ghost.png".into(), layers: vec![], x: Some(0.3), y: None, z: None, scale: None, flip: None, xscale: None, yscale: None, alpha: None, dx: None, dy: None, fade: 0.0 }
        );
        assert!(matches!(&p.ops[1], Op::Show { id, x: Some(a), .. } if id == "art/x.png" && (a - 0.25).abs() < 1e-6));
    }

    #[test]
    fn sprite_y_and_z() {
        let p = ok(&format!(
            "{MIKA}::show{{mika happy y=0.8 z=3}}\n::show{{mika y=90%  z=-100}}\n::show{{mika y=-0.5 z=100}}\n::show{{mika y=150%}}\n::move{{mika y=0.9 time=0.3 ease=out}}\n::move{{mika x=left y=1.5}}\n"
        ));
        let show = |i: usize| match &p.ops[i] {
            Op::Show { y, z, .. } => (*y, *z),
            other => panic!("{other:?}"),
        };
        assert_eq!(show(0), (Some(0.8), Some(3)));
        assert_eq!(show(1), (Some(0.9), Some(-100)));
        assert_eq!(show(2), (Some(-0.5), Some(100)));
        assert_eq!(show(3), (Some(1.5), None));
        assert_eq!(p.ops[4], Op::Move { id: "mika".into(), flip: None, steps: vec![MoveStep { x: None, y: Some(0.9), scale: None, xscale: None, yscale: None, alpha: None, dx: None, dy: None, time: 0.3, ease: Ease::Out, wait: 0.0 }], looped: false });
        assert!(matches!(&p.ops[5], Op::Move { steps, .. } if steps[0].x.is_some() && steps[0].y == Some(1.5)));
        // not given: `None`, so the sprite keeps its own
        let q = ok(&format!("{MIKA}::show{{mika}}\n"));
        assert!(matches!(&q.ops[0], Op::Show { y: None, z: None, .. }));
    }

    #[test]
    fn sprite_y_and_z_errors_name_the_line() {
        for (src, needle) in [
            ("::show{src=a.png y=4}\n", "y=4"),
            ("::show{src=a.png y=-2.1}\n", "y=-2.1"),
            ("::show{src=a.png y=low}\n", "y=low"),
            ("::show{src=a.png z=101}\n", "`z=101` must be a whole number from -100 to 100"),
            ("::show{src=a.png z=-101}\n", "z=-101"),
            ("::show{src=a.png z=1.5}\n", "z=1.5"),
            ("::show{src=a.png z=front}\n", "z=front"),
            ("::move{mika y=3.1}\n", "y=3.1"),
            ("::move{mika y=1 z=1}\n", "`z`"),
        ] {
            let e = errs(&format!("\n{src}"));
            assert_eq!(e[0].line, 2, "{src}");
            assert!(e[0].message.contains(needle), "{src}: {}", e[0].message);
        }
    }

    #[test]
    fn hide_and_move() {
        let p = ok(&format!("{MIKA}::hide{{mika fade=0.5}}\n::hide{{all}}\n::move{{mika x=right time=1}}\n::move{{mika x=0.1}}\n"));
        assert_eq!(p.ops[0], Op::Hide { id: Some("mika".into()), fade: 0.5 });
        assert_eq!(p.ops[1], Op::Hide { id: None, fade: 0.0 });
        assert_eq!(p.ops[2], Op::Move { id: "mika".into(), flip: None, steps: vec![MoveStep { x: Some(1.0), y: None, scale: None, xscale: None, yscale: None, alpha: None, dx: None, dy: None, time: 1.0, ease: Ease::InOut, wait: 0.0 }], looped: false });
        assert_eq!(p.ops[3], Op::Move { id: "mika".into(), flip: None, steps: vec![MoveStep { x: Some(0.1), y: None, scale: None, xscale: None, yscale: None, alpha: None, dx: None, dy: None, time: 0.5, ease: Ease::InOut, wait: 0.0 }], looped: false });
    }

    #[test]
    fn move_sequences_alpha_and_offsets_parse() {
        let p = ok("::show{src=a.png id=a alpha=0.4 xoffset=-30 yoffset=12.5}\n\
                    ::move{a x=30% time=1 then x=70% time=1 wait=0.5 ease=linear then alpha=0 xoffset=0 time=0.5 loop}\n\
                    ::move{a alpha=50% yoffset=8 loop}\n");
        assert!(matches!(&p.ops[0], Op::Show { alpha: Some(a), dx: Some(dx), dy: Some(dy), .. } if *a == 0.4 && *dx == -30.0 && *dy == 12.5));
        let Op::Move { steps, looped, flip: None, .. } = &p.ops[1] else { panic!("{:?}", p.ops[1]) };
        assert!(*looped && steps.len() == 3);
        assert_eq!((steps[0].x, steps[0].time, steps[0].wait), (Some(0.3), 1.0, 0.0));
        assert_eq!((steps[1].x, steps[1].wait, steps[1].ease), (Some(0.7), 0.5, Ease::Linear));
        assert_eq!((steps[2].alpha, steps[2].dx, steps[2].x, steps[2].time), (Some(0.0), Some(0.0), None, 0.5));
        assert!(matches!(&p.ops[2], Op::Move { steps, looped: true, .. } if steps.len() == 1 && steps[0].alpha == Some(0.5) && steps[0].dy == Some(8.0)));
        // a pause only is a step; flip belongs to the first
        let p = ok("::move{a flip then wait=1 then x=0}\n");
        assert!(matches!(&p.ops[0], Op::Move { flip: Some(true), steps, .. } if steps.len() == 3 && steps[1].wait == 1.0 && steps[1].props().iter().all(Option::is_none)));
        assert!(matches!(&p.ops[0], Op::Move { steps, .. } if steps[1].time == 0.0 && steps[0].time == 0.0 && steps[2].time == 0.5), "a pause is no longer than its wait");
    }

    #[test]
    fn move_sequence_alpha_and_offset_errors() {
        for (src, needle) in [
            ("::show{src=a.png alpha=1.5}\n", "`alpha=1.5` must be a number from 0 to 1"),
            ("::show{src=a.png xoffset=5000}\n", "`xoffset=5000` must be a number from -3840 to 3840"),
            ("::show{src=a.png yoffset=up}\n", "`yoffset=up`"),
            ("::move{a alpha=-1}\n", "`alpha=-1`"),
            ("::move{a x=1 then}\n", "a `then` step needs"),
            ("::move{a x=1 then time=1}\n", "a `then` step needs"),
            ("::move{a x=1 then flip}\n", "unknown attribute `flip` in a `then` step of `::move`: it belongs in the first step"),
            ("::move{a x=1 then speed=2 x=2}\n", "unknown attribute `speed` in a `then` step"),
            ("::move{a x=1 then x=2 wait=-1}\n", "`wait=-1`"),
            ("::move{a x=1 time=0 loop}\n", "needs `time=` above 0"),
            ("::move{a x=1 time=1 wait=1 loop}\n", "can't `wait=`"),
            ("::move{a x=1 time=0 then x=2 time=0 loop}\n", "would repeat without taking any time"),
            ("::move{a flip loop}\n", "needs something to animate"),
            ("::move{a}\n", "`wait=`"),
        ] {
            assert!(has_err(src, needle), "{src}: {:?}", errs(src));
        }
        let many = format!("::move{{a x=1{}}}\n", " then x=2".repeat(MAX_MOVE_STEPS));
        assert!(has_err(&many, "at most 32 steps"));
        assert_eq!(ok(&format!("::move{{a x=1{}}}\n", " then x=2".repeat(MAX_MOVE_STEPS - 1))).ops.len(), 2);
    }

    #[test]
    fn sprite_errors() {
        assert!(has_err("::show{nobody}\n", "unknown character `nobody`"));
        assert!(has_err("::character{id=a}\n::show{a}\n", "no `sprites="));
        assert!(has_err("::show\n", "needs a character"));
        assert!(has_err("::show{src=a.png x=sideways}\n", "bad `x=sideways`"));
        assert!(has_err("::show{src=a.png scale=0}\n", "from 0.05 to 10"));
        assert!(has_err("::show{src=a.png scale=100000}\n", "from 0.05 to 10"));
        assert!(has_err("::hide\n", "needs a sprite"));
        assert!(has_err("::move{a}\n", "needs `x="));
        assert!(has_err("::character{name=x}\n", "needs `id="));
        assert!(has_err("::character{id=a color=red}\n", "bad colour"));
        assert!(has_err("::character{id=a}\n::character{id=A}\n", "declared twice"));
        assert!(has_err("::character{id=\"a b\"}\n", "letters, digits"));
    }

    #[test]
    fn asset_paths_must_be_inside_the_game_folder_where_they_are_written() {
        for bad in ["assets\\calm.ogg", "../x.png", "/abs/x.png", "C:/x.png", "a/../../x.png"] {
            for (tail, line) in [
                (format!("Hi.\n::bg{{src={bad}}}\n"), 2),
                (format!("::music{{src={bad}}}\n"), 1),
                (format!("Hi.\n::sfx{{src={bad} loop}}\n"), 2),
                (format!("::voice{{src={bad}}}\n"), 1),
                (format!("::video{{src={bad}}}\n"), 1),
                (format!("Hi.\n::show{{x src={bad}}}\n"), 2),
                (format!("![alt]({bad})\n"), 1),
                (format!("Hi.\n::item{{id=k icon={bad}}}\n"), 2),
                (format!("::character{{id=m sprites={bad}}}\n"), 1),
                (format!("::hotspots\n- [a](#b){{rect=0,0,1,1 image={bad}}}\n::\n# b\nHi.\n"), 2),
            ] {
                let e = errs(&tail);
                let hit = e.iter().find(|e| e.message.contains("inside the game folder"));
                assert_eq!(hit.map(|e| e.line), Some(line), "{bad} in {tail:?}: {e:?}");
            }
            let front = format!("---\ngallery: [{bad}]\n---\nHi.\n");
            assert!(has_err(&front, "inside the game folder"), "{front}");
            assert!(has_err(&format!("---\ntheme: {bad}\n---\nHi.\n"), "inside the game folder"));
        }
        ok("::bg{src=./assets/a.png}\n::music{src=assets/m.ogg}\n");
    }

    #[test]
    fn options_know_their_line_and_item_icons_come_in_file_order() {
        let p = ok("::item{id=zed icon=z.png}\n::item{id=alpha icon=a.png}\n::item{id=mid}\n::item{id=beta icon=b.png}\n# m\n::hotspots\n- [One](#m){rect=0,0,.5,.5 image=one.png}\n\n- [Two](#m){rect=0,0,.5,.5}\n- [Three](#m){rect=0,0,.5,.5 image=three.png}\n::\n");
        let icons: Vec<_> = p.item_icons().collect();
        assert_eq!(icons, [(1, "z.png"), (2, "a.png"), (4, "b.png")], "declaration order, not id order");
        let Op::Choice { options, .. } = &p.ops[0] else { panic!() };
        assert_eq!(options.iter().map(|o| o.line).collect::<Vec<_>>(), [7, 9, 10]);
        let images: Vec<_> = p.assets_with_lines().collect();
        assert_eq!(images, [(7, AssetKind::Image, "one.png"), (10, AssetKind::Image, "three.png")]);
    }

    #[test]
    fn extra_words_are_errors() {
        let chars = "::character{id=mika sprites=m_{pose}.png}\n";
        assert!(has_err(&format!("{chars}::show{{mika happy sad}}\n"), "unexpected `sad`"));
        assert!(has_err("::show{a b src=x.png}\n", "unexpected `b`"));
        assert!(has_err("::hide{a b}\n", "unexpected `b`"));
        assert!(has_err("::hide{all a}\n", "no sprite name with `all`"));
        assert!(has_err("::move{a b x=left}\n", "unexpected `b`"));
        ok(&format!("{chars}::show{{mika happy}}\n::show{{a src=x.png}}\n::hide{{mika}}\n"));
    }

    #[test]
    fn one_mistake_gives_one_error() {
        let e = errs("::bg{src=a.png fade=abc wipe=left}\n");
        assert_eq!(e.len(), 1, "the unread `wipe` isn't reported: {e:?}");
        let e = errs("::bg{src=a.png fade=abc wipee=left}\n");
        assert!(e.iter().any(|e| e.message.contains("unknown attribute `wipee`")), "a real typo still is");
        assert!(has_err("::bg{src=a.png wipee=left}\n", "(it takes: src, color, anim, fade, wipe, through)"));
        assert!(has_err("::frobnicate\n", "shake, flash, notify, unlock, fade, camera"));
    }

    #[test]
    fn a_looping_video_can_always_be_skipped() {
        let p = ok("::video{src=a.webm loop}\n::video{src=b.webm}\n");
        assert!(matches!(p.ops[0], Op::Video { looped: true, skippable: true, .. }));
        assert!(matches!(p.ops[1], Op::Video { looped: false, skippable: false, .. }));
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
        assert_eq!(p.ops[0], Op::Call { target: p.labels["sub"], args: vec![] });
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
        let Op::Choice { options, .. } = &p.ops[0] else { panic!("not a choice") };
        assert_eq!(options.len(), 3);
        assert_eq!(options[0].target, p.labels["say-yes"]);
        assert_eq!(options[1].target, p.labels["no"]);
        assert_eq!(options[0].effects.len(), 2);
        assert_eq!(options[0].cond, None);
        assert_eq!(options[2].effects.len(), 1);
        assert!(options[2].cond.is_some());
    }

    #[test]
    fn timed_choices_carry_a_limit_and_one_default_that_combines_with_other_settings() {
        let p = ok("# a\n::choice{time=8}\n- [Speak](#a)\n- [Wait](#a){once, default, calm+=1, if calm < 9}\n::\n");
        let Op::Choice { options, time, .. } = &p.ops[0] else { panic!() };
        assert_eq!(*time, Some(8.0));
        assert_eq!(options.iter().map(|o| o.default).collect::<Vec<_>>(), [false, true]);
        assert!(options[1].cond.is_some() && options[1].effects.len() == 2, "if, once and effects all kept");
        for t in ["0.5", "2.5", "600", "601", "86400"] {
            ok(&format!("# a\n::choice{{time={t}}}\n- [x](#a){{default}}\n::\n"));
        }
        let plain = ok("# a\n::choice\n- [x](#a)\n::\n");
        assert!(matches!(&plain.ops[0], Op::Choice { time: None, .. }));
    }

    #[test]
    fn a_timed_hotspot_screen_adds_a_hidden_option_that_jumps_when_time_runs_out() {
        let p = ok("# a\n::hotspots{time=3 default=#too-slow}\n- [Go](#a){rect=0,0.5,0.5,0.5}\n::\n# too slow\n::end\n");
        let Op::Choice { options, time, .. } = &p.ops[0] else { panic!() };
        assert_eq!(*time, Some(3.0));
        assert_eq!(options.len(), 2);
        let late = &options[1];
        assert!(late.default && late.text.is_empty() && late.target == p.labels["too-slow"]);
        assert!(late.area.as_ref().is_some_and(|a| a.hidden && a.rect == [0.0; 4]), "nothing can click it");
        assert!(!options[0].default);
        let plain = ok("# a\n::hotspots\n- [Go](#a){rect=0,0.5,0.5,0.5}\n::\n");
        assert!(matches!(&plain.ops[0], Op::Choice { time: None, options, .. } if options.len() == 1));
    }

    #[test]
    fn timed_hotspot_errors_name_the_line() {
        let line = |src: &str, msg: &str| {
            let e = errs(src);
            e.iter().find(|e| e.message.contains(msg)).unwrap_or_else(|| panic!("{msg}: {e:?}")).line
        };
        let block = |attrs: &str| format!("# a\n# b\n::hotspots{attrs}\n- [x](#a){{rect=0,0,1,1}}\n::\n");
        for bad in ["0.4", "86401", "abc", "nan"] {
            assert_eq!(line(&block(&format!("{{time={bad} default=#b}}")), "bad `time="), 3, "{bad}");
        }
        assert_eq!(line(&block("{time=3}"), "needs `default=#label`"), 3);
        assert_eq!(line(&block("{default=#b}"), "needs a timed block"), 3);
        assert_eq!(line(&block("{time=3 default=#nowhere}"), "unknown label `nowhere`"), 3);
        assert_eq!(line(&block("{time=3 default=b}"), "`default` needs a label"), 3);
        assert_eq!(line(&block("{time=3 default}"), "`default` needs a label"), 3);
        assert_eq!(line(&block("{time=3 default=#b speed=2}"), "unknown attribute `speed`"), 3);
        ok(&block("{time=0.5 default=#b}"));
        ok(&block("{time=600 default=#b}"));
        // a choice still takes only `time`, and its default is an option
        assert!(has_err("# a\n::choice{time=3 default=#a}\n- [x](#a){default}\n::\n", "unknown attribute `default`"));
    }

    #[test]
    fn a_hotspot_can_follow_a_sprite_with_a_relative_rect() {
        let src = |tail: &str| format!("::show{{id=fly src=assets/fly.png}}\n# a\n::hotspots\n- [Fly](#a){tail}\n::\n");
        let area = |tail: &str| {
            let p = ok(&src(tail));
            let Op::Choice { options, .. } = p.ops.iter().find(|o| matches!(o, Op::Choice { .. })).unwrap() else { panic!() };
            options[0].area.clone().unwrap()
        };
        let whole = area("{sprite=fly}");
        assert_eq!((whole.sprite.as_deref(), whole.rect), (Some("fly"), [0.0, 0.0, 1.0, 1.0]));
        let part = area("{sprite=fly rect=25%,0,0.5,0.5 image=assets/gui/pin.png, hidden}");
        assert_eq!((part.rect, part.hidden, part.image.as_deref()), ([0.25, 0.0, 0.5, 0.5], true, Some("assets/gui/pin.png")));
        assert_eq!(area("{rect=0,0,1,1}").sprite, None);
        // a character's name finds the sprite its `::show` made
        ok("::character{id=mika name=Mika sprites=m_{pose}.png}\n::show{mika happy}\n# a\n::hotspots\n- [M](#a){sprite=Mika}\n::\n");
    }

    #[test]
    fn sprite_hotspot_errors_name_the_line() {
        let line = |src: &str, msg: &str| {
            let e = errs(src);
            e.iter().find(|e| e.message.contains(msg)).unwrap_or_else(|| panic!("{msg}: {e:?}")).line
        };
        let show = "::show{id=fly src=assets/fly.png}\n";
        let spot = |tail: &str| format!("{show}# a\n::hotspots\n- [Fly](#a){tail}\n::\n");
        assert_eq!(line(&spot("{sprite=bee}"), "no `::show` ever shows a sprite called `bee`"), 4);
        assert_eq!(line(&spot("{sprite=fly rect=0,0,2,1}"), "bad `rect="), 4);
        assert_eq!(line(&spot("{sprite=fly rect=0.5,0,0.8,1}"), "outside the screen"), 4);
        assert_eq!(line(&spot("{sprite=}"), "`sprite=` needs a sprite name"), 4);
        assert_eq!(line(&spot("{once}"), "needs `rect="), 4);
        assert!(has_err(&format!("{show}# a\n::choice\n- [x](#a){{sprite=fly}}\n::\n"), "only belong in a `::hotspots` block"));
    }

    #[test]
    fn timed_choice_errors_name_the_line() {
        let line = |src: &str, msg: &str| {
            let e = errs(src);
            e.iter().find(|e| e.message.contains(msg)).unwrap_or_else(|| panic!("{msg}: {e:?}")).line
        };
        assert_eq!(line("# a\n::choice{time=5}\n- [x](#a)\n::\n", "needs one option marked `{default}`"), 2);
        assert_eq!(line("# a\n::choice\n- [x](#a)\n- [y](#a){default}\n::\n", "`default` needs a timed choice"), 4);
        assert_eq!(line("# a\n::choice{time=5}\n- [x](#a){default}\n- [y](#a){default}\n::\n", "only one option"), 4);
        for bad in ["0", "0.4", "86401", "-3", "abc", "{secs}", "nan", "inf", "1e9"] {
            let src = format!("# a\n::choice{{time={bad}}}\n- [x](#a){{default}}\n::\n");
            assert_eq!(line(&src, "bad `time="), 2, "{bad}");
        }
        assert_eq!(line("# a\n::choice{time}\n- [x](#a){default}\n::\n", "`time` needs a number"), 2);
        assert_eq!(line("# a\n::choice{speed=2}\n- [x](#a)\n::\n", "unknown attribute `speed`"), 2);
        // not for hotspots (or anything else)
        assert!(has_err("# a\n::hotspots\n- [x](#a){rect=0,0,1,1, default}\n::\n", "only belongs in a timed `::choice`"));
        assert!(has_err("# a\n::shop{time=5 pay=g into=i}\n::\n", "unknown attribute"));
    }

    #[test]
    fn choice_errors() {
        assert!(has_err("::choice\n- [x](#y)\n", "never closed"));
        assert!(has_err("::choice\n::\n", "has no options"));
        assert!(has_err("# a\n::choice\n- [x](#a){if }\n::\n", "found nothing"));
        assert!(has_err("# a\n::choice\n- [x](#a){bogus}\n::\n", "bad assignment"));
        // a variable that merely starts with "if" is still an assignment
        let p = ok("# a\n::choice\n- [x](#a){iffy+=1}\n::\n");
        let Op::Choice { options, .. } = &p.ops[0] else { panic!() };
        assert_eq!((options[0].effects.len(), options[0].cond.is_none()), (1, true));
    }

    // ---- errors ------------------------------------------------------------

    #[test]
    fn hotspots_take_a_rect_an_image_and_the_usual_option_extras() {
        let p = ok("# map\n::hotspots\n- [Library](#map){rect=0.1,0.2,0.2,0.3}\n- [Old key](#map){rect=60%,70%,5%,8% image=assets/key.png, once, found=true, if not found}\n::\n");
        let Op::Choice { options, .. } = &p.ops[0] else { panic!("not a choice") };
        let a = options[0].area.as_ref().unwrap();
        assert_eq!((a.rect, a.image.as_deref()), ([0.1, 0.2, 0.2, 0.3], None));
        let b = options[1].area.as_ref().unwrap();
        assert!((b.rect[0] - 0.6).abs() < 1e-6 && (b.rect[3] - 0.08).abs() < 1e-6);
        assert_eq!(b.image.as_deref(), Some("assets/key.png"));
        assert!(options[1].cond.is_some());
        assert_eq!(options[1].effects.len(), 2, "found=true and the once marker");
        assert_eq!(options[1].target, 0);
        assert_eq!(p.assets().map(|(_, _, a)| a).collect::<Vec<_>>(), ["assets/key.png"]);
        assert_eq!(p.images_ahead(0, 10), ["assets/key.png"]);
    }

    const ITEMS: &str = "---\ntitle: T\ninventory: bag\nvars: { gold: 40, bag: [], met: false, name: \"x\" }\n---\n::item{id=key name=\"Brass key\" icon=assets/key.png price=15 text=\"Opens the clock tower.\"}\n::item{id=map}\n";

    #[test]
    fn items_have_a_name_an_icon_a_price_and_a_text() {
        let p = ok(&format!("{ITEMS}# a\nHi.\n"));
        let key = &p.items["key"];
        assert_eq!(
            (key.name.as_str(), key.icon.as_deref(), key.price, key.text.as_str(), key.line),
            ("Brass key", Some("assets/key.png"), Some(15), "Opens the clock tower.", 6)
        );
        let map = &p.items["map"];
        assert_eq!((map.name.as_str(), map.icon.as_deref(), map.price, map.text.as_str()), ("map", None, None, ""));
        assert_eq!(p.inventory.as_deref(), Some("bag"));
        assert_eq!(p.item_icons().collect::<Vec<_>>(), [(6, "assets/key.png")]);
    }

    #[test]
    fn an_item_may_name_an_int_to_count_it_and_a_label_to_use_it() {
        let head = "---\ninventory: bag\nvars: { bag: [], potions: 3, name: \"x\", gold: 9 }\n---\n";
        let p = ok(&format!("{head}::item{{id=potion count=potions use=#drink-potion}}\n# drink potion\nHi.\n# a\nHi.\n"));
        let potion = &p.items["potion"];
        assert_eq!((potion.count.as_deref(), potion.use_label.as_deref()), (Some("potions"), Some("drink-potion")));
        let bad = |item: &str, line: usize, msg: &str| {
            let errors = errs(&format!("{head}{item}\n# drink\nHi.\n"));
            assert!(errors.iter().any(|e| e.line == line && e.message.contains(msg)), "{item}: {errors:?}");
        };
        bad("::item{id=a count=name}", 5, "whole-number variable");
        bad("::item{id=a count=bag}", 5, "whole-number variable");
        bad("::item{id=a count=nothing}", 5, "whole-number variable");
        bad("::item{id=a use=#nowhere}", 5, "unknown label `nowhere`");
        bad("::item{id=a use=drink}", 5, "names a label");
        bad("::item{id=a use=#}", 5, "names a label");
        let params = errs(&format!("{head}::item{{id=a use=#heal}}\n# heal(n)\nHi.\n::return\n"));
        assert!(params.iter().any(|e| e.line == 5 && e.message.contains("takes parameters")), "{params:?}");
    }

    #[test]
    fn a_counted_item_in_a_shop_needs_a_price_and_a_matching_persistence() {
        let head = "---\nvars: { bag: [], potions: 3, gold: 9 }\n---\n";
        let shop = |item: &str| format!("{head}{item}\n::shop{{pay=gold into=bag}}\n- potion\n::\n");
        assert!(has_err(&shop("::item{id=potion count=potions price=0}"), "price above 0"));
        ok(&shop("::item{id=potion count=potions price=4}"));
        let kept = "---\nvars: { bag: [], gold: 9 }\npersistent: { potions: 3 }\n---\n::item{id=potion count=potions price=4}\n::shop{pay=gold into=bag}\n- potion\n::\n";
        assert!(has_err(kept, "both be persistent"));
    }

    #[test]
    fn the_staging_demo_uses_y_z_size_and_nw() {
        let p = ok(include_str!("../../../examples/staging.quill.md"));
        assert!(p.ops.iter().any(|op| matches!(op, Op::Move { steps, .. } if steps[0].y.is_some())));
        assert!(p.ops.iter().any(|op| matches!(op, Op::Show { z: Some(_), .. })));
        let says = |f: &str| p.ops.iter().any(|op| matches!(op, Op::Say { text, .. } if text.contains(f)));
        assert!(says("{size=+14}") && says("{size=24}") && says("{nw}") && says("{p}"));
        assert!(p.ops.iter().any(|op| matches!(op, Op::Flash { .. })));
        assert!(p.ops.iter().any(|op| matches!(op, Op::FadeOut { .. })) && p.ops.iter().any(|op| matches!(op, Op::FadeIn { .. })));
        assert!(p.ops.iter().any(|op| matches!(op, Op::Shake { axis: Axis::Y, .. })));
        assert!(p.ops.iter().any(|op| matches!(op, Op::Show { flip: Some(true), .. })));
        assert!(p.ops.iter().any(|op| matches!(op, Op::Move { steps, .. } if steps[0].xscale.is_some() && steps[0].yscale.is_some())));
        assert!(p.ops.contains(&Op::SfxLoop { src: "assets/calm.ogg".into(), fade: 1.0, volume: 0.4 }));
        assert!(p.ops.contains(&Op::StopSfx { src: Some("assets/calm.ogg".into()), fade: 1.5 }));
        assert_eq!(p.characters.len(), 2);
    }

    #[test]
    fn the_shop_demo_calls_a_parameterized_label_twice() {
        let p = ok(include_str!("../../../examples/shop.quill.md"));
        assert_eq!(p.params["price-line"], ["item", "cost"], "the label id is the slug of the heading");
        let calls = p.ops.iter().filter(|op| matches!(op, Op::Call { args, .. } if args.len() == 2)).count();
        assert_eq!(calls, 2);
    }

    #[test]
    fn the_stats_demo_uses_dotted_names_arithmetic_functions_and_random() {
        let p = ok(include_str!("../../../examples/stats.quill.md"));
        assert_eq!(p.vars["hero.hp"], Value::Int(12));
        let sets: Vec<&Effect> = p.ops.iter().filter_map(|op| if let Op::Set(e) = op { Some(e) } else { None }).collect();
        assert!(sets.iter().any(|e| e.var == "dmg" && matches!(e.value, Expr::Random(..))));
        assert!(sets.iter().any(|e| e.var == "cost" && matches!(e.value, Expr::Bin(_, BinOp::Sub, _))));
        assert!(sets.iter().any(|e| e.var == "gap" && matches!(&e.value, Expr::Func(Func::Abs, _))));
        assert!(sets.iter().any(|e| e.var == "items" && matches!(e.value, Expr::Count(_))));
        assert!(p.ops.iter().filter_map(|op| match op { Op::Jump { cond: Some(c), .. } => Some(c), _ => None }).any(|c| format!("{c:?}").contains("Count")));
    }

    #[test]
    fn the_market_demo_parses_with_items_a_shop_and_an_inventory_call() {
        let p = ok(include_str!("../../../examples/market.quill.md"));
        assert_eq!(p.items.len(), 7);
        assert_eq!((p.inventory.as_deref(), p.journal.as_deref(), p.journal_done.as_deref()), (Some("pack"), Some("quests"), Some("quests_done")));
        assert_eq!(p.items["tonic"].use_label.as_deref(), Some("drink-tonic"));
        assert!(p.ops.iter().any(|op| matches!(op, Op::Shop { stock, .. } if stock.len() == 5 && stock[3].cond.is_some() && stock[3].price == 18)));
        assert!(p.ops.contains(&Op::Inventory) && p.ops.contains(&Op::Journal));
        assert_eq!(p.item_icons().count(), 4);
        assert!(p.unknown_items().is_empty());
    }

    #[test]
    fn the_inventory_front_matter_may_come_before_vars_and_needs_a_list() {
        assert!(ok("---\ninventory: bag\nvars: { bag: [] }\n---\n").inventory.is_some());
        let e = |src: &str, line: usize, needle: &str| {
            let errors = errs(src);
            assert!(errors.iter().any(|e| e.line == line && e.message.contains(needle)), "{src:?}: {errors:?}");
        };
        e("---\ninventory: bag\n---\n", 2, "must name a list");
        e("---\ninventory: bag\nvars: { bag: 3 }\n---\n", 2, "must name a list");
        e("---\ninventory: two words\n---\n", 2, "names a list variable");
    }

    #[test]
    fn the_inventory_command_needs_an_inventory() {
        let p = ok("---\ninventory: bag\nvars: { bag: [] }\n---\n::inventory\n");
        assert_eq!(p.ops[0], Op::Inventory);
        let errors = errs("::inventory\n");
        assert!(errors.iter().any(|e| e.line == 1 && e.message.contains("`inventory: <list variable>`")), "{errors:?}");
        let errors = errs("---\ninventory: bag\nvars: { bag: [] }\n---\n::inventory{x}\n");
        assert!(errors.iter().any(|e| e.line == 5 && e.message.contains("takes no attributes")), "{errors:?}");
    }

    #[test]
    fn the_journal_front_matter_names_lists_and_done_needs_a_journal() {
        let p = ok("---\njournal_done: done\njournal: quests\nvars: { quests: [], done: [] }\n---\n");
        assert_eq!((p.journal.as_deref(), p.journal_done.as_deref(), p.inventory.as_deref()), (Some("quests"), Some("done"), None));
        let e = |src: &str, line: usize, needle: &str| {
            let errors = errs(src);
            assert!(errors.iter().any(|e| e.line == line && e.message.contains(needle)), "{src:?}: {errors:?}");
        };
        e("---\njournal: quests\n---\n", 2, "`journal: quests` must name a list");
        e("---\njournal: quests\nvars: { quests: 3 }\n---\n", 2, "must name a list");
        e("---\njournal: two words\n---\n", 2, "names a list variable");
        e("---\nvars: { d: [] }\njournal_done: d\n---\n", 3, "needs a `journal:`");
        e("---\njournal: q\njournal_done: d\nvars: { q: [] }\n---\n", 3, "`journal_done: d` must name a list");
    }

    #[test]
    fn the_journal_command_needs_a_journal() {
        let p = ok("---\njournal: q\nvars: { q: [] }\n---\n::journal\n");
        assert_eq!(p.ops[0], Op::Journal);
        let errors = errs("---\ninventory: bag\nvars: { bag: [] }\n---\n::journal\n");
        assert!(errors.iter().any(|e| e.line == 5 && e.message.contains("`journal: <list variable>`")), "{errors:?}");
        let errors = errs("---\njournal: q\nvars: { q: [] }\n---\n::journal{x}\n");
        assert!(errors.iter().any(|e| e.line == 5 && e.message.contains("takes no attributes")), "{errors:?}");
    }

    #[test]
    fn journal_entries_that_nothing_declares_are_found_like_items() {
        let head = "---\njournal: q\njournal_done: d\nvars: { q: [find-cat], d: [] }\n---\n::item{id=find-cat}\n";
        let p = ok(&format!("{head}# a\n::set{{q += \"find-cta\"}}\n::set{{d += \"find-cat\", d += \"lost\"}}\n::if{{q has \"nope\"}}\nx\n::endif\n"));
        let found: Vec<String> = p.unknown_items().into_iter().map(|(_, t)| t).collect();
        assert_eq!(found, ["find-cta", "lost", "nope"]);
    }

    #[test]
    fn item_errors_name_the_line() {
        let e = |src: &str, line: usize, needle: &str| {
            let errors = errs(src);
            assert!(errors.iter().any(|e| e.line == line && e.message.contains(needle)), "{src:?}: {errors:?}");
        };
        e("::item{name=x}\n", 1, "needs `id=");
        e("::item{id=\"a b\"}\n", 1, "must be letters");
        e("::item{id=a}\n::item{id=a}\n", 2, "declared twice");
        e("::item{id=a price=-3}\n", 1, "bad `price=-3`");
        e("::item{id=a price=1.5}\n", 1, "bad `price=1.5`");
        e("::item{id=a price=cheap}\n", 1, "bad `price=cheap`");
        e("::item{id=a icon=a.gif}\n", 1, "bad `icon=a.gif`");
        e("::item{id=a icon=a}\n", 1, "bad `icon=a`");
        e("::item{id=a colour=red}\n", 1, "unknown attribute `colour`");
    }

    const ASK: &str = "---\nvars: { hero.name: \"x\", n: 0 }\npersistent: { kept: \"k\" }\n---\n# a\n";

    #[test]
    fn an_input_asks_for_a_text_variable() {
        let p = ok(&format!("{ASK}::input{{var=hero.name prompt=\"What is your name?\" default=\"Alex\" max=16}}\nHi {{hero.name}}.\n"));
        assert_eq!(
            p.ops[0],
            Op::Input { var: "hero.name".into(), prompt: "What is your name?".into(), default: Some("Alex".into()), max: 16 }
        );
        assert_eq!(p.lines[0], 6);
        let p = ok(&format!("{ASK}::input{{var=kept prompt=\"Name, **please**?\"}}\n"));
        assert_eq!(p.ops[0], Op::Input { var: "kept".into(), prompt: "Name, **please**?".into(), default: None, max: 24 });
    }

    #[test]
    fn the_greeting_demo_parses_with_one_input() {
        let p = ok(include_str!("../../../examples/greeting.quill.md"));
        let inputs: Vec<&Op> = p.ops.iter().filter(|op| matches!(op, Op::Input { .. })).collect();
        assert_eq!(inputs.len(), 1);
        assert!(matches!(inputs[0], Op::Input { var, max: 16, default: Some(d), .. } if var == "hero.name" && d == "Alex"));
    }

    #[test]
    fn an_input_accepts_up_to_the_cap_and_a_long_one_is_only_a_check_warning() {
        for max in [INPUT_MAX_LIMIT, INPUT_MAX_LIMIT + 1, INPUT_MAX_CAP] {
            let p = ok(&format!("{ASK}::input{{var=hero.name prompt=\"Name?\" max={max}}}\n"));
            assert!(p.ops.iter().any(|op| matches!(op, Op::Input { max: m, .. } if *m == max)), "{max}");
        }
    }

    #[test]
    fn input_errors_name_the_line() {
        let e = |body: &str, needle: &str| {
            let errors = errs(&format!("{ASK}{body}"));
            assert!(errors.iter().any(|e| e.line == 6 && e.message.contains(needle)), "{body:?}: {errors:?}");
        };
        e("::input{prompt=\"Name?\"}\n", "needs `var=");
        e("::input{var=hero.name}\n", "needs `prompt=");
        e("::input{var=hero.name prompt=\"  \"}\n", "needs `prompt=");
        e("::input{var=nosuch prompt=\"Name?\"}\n", "no text variable `nosuch`");
        e("::input{var=n prompt=\"Name?\"}\n", "must be a text variable");
        e("::input{var=hero.name prompt=\"Name?\" bogus=1}\n", "unknown attribute `bogus`");
        e("::input{var=hero.name prompt=\"Name?\" max=0}\n", "bad `max=0`");
        e("::input{var=hero.name prompt=\"Name?\" max=1025}\n", "bad `max=1025`");
        e("::input{var=hero.name prompt=\"Name?\" max=x}\n", "bad `max=x`");
        e("::input{var=hero.name prompt=\"Name?\" default=\"toolong\" max=3}\n", "longer than `max=3`");
        e("::input{var=hero.name prompt=\"Name?{nw}\"}\n", "only work in dialogue");
        e("::input{var=hero.name prompt=\"**Name?\"}\n", "");
        e("::input\n", "needs `var=");
        ok(&format!("{ASK}::input{{var=hero.name prompt=\"ok\" max=64}}\n"));
    }

    const TIMED: &str = "---\nvars: { hit: \"\", n: 0 }\n---\n# a\n";

    #[test]
    fn a_timing_bar_has_zones_a_speed_a_limit_and_a_prompt() {
        let p = ok(&format!("{TIMED}::timing{{result=hit speed=1.5 good=40%..60% perfect=48%..52% time=5 prompt=\"Pick the **lock**!\"}}\nDone.\n"));
        let Op::Timing { var, prompt, bar } = &p.ops[0] else { panic!("{:?}", p.ops[0]) };
        assert_eq!((var.as_str(), prompt.as_str()), ("hit", "Pick the **lock**!"));
        assert_eq!((bar.speed, bar.good, bar.perfect, bar.time), (1.5, [0.4, 0.6], Some([0.48, 0.52]), Some(5.0)));
        assert_eq!(p.lines[0], 5);
        // only `result` and `good` are needed; fractions work like percentages
        let p = ok(&format!("{TIMED}::timing{{result=hit good=0.2..0.3}}\n"));
        assert_eq!(p.ops[0], Op::Timing { var: "hit".into(), prompt: String::new(), bar: TimingBar { speed: 1.0, good: [0.2, 0.3], perfect: None, time: None } });
    }

    #[test]
    fn a_key_prompt_has_a_key_a_time_a_count_and_a_prompt() {
        let p = ok(&format!("{TIMED}::press{{key=Space time=1.2 result=hit prompt=\"**Duck**!\"}}\nDone.\n"));
        assert_eq!(
            p.ops[0],
            Op::Press { var: "hit".into(), prompt: "**Duck**!".into(), press: KeyPress { key: "space".into(), count: 1, time: 1.2 } }
        );
        assert_eq!(p.lines[0], 5);
        let p = ok(&format!("{TIMED}::press{{key=click time=4 count=15 result=hit}}\n"));
        assert_eq!(p.ops[0], Op::Press { var: "hit".into(), prompt: String::new(), press: KeyPress { key: "click".into(), count: 15, time: 4.0 } });
        for key in PRESS_KEYS {
            ok(&format!("{TIMED}::press{{key={key} time=0.5 count=200 result=hit}}\n"));
        }
    }

    #[test]
    fn key_prompt_errors_name_the_line() {
        let e = |body: &str, needle: &str| {
            let errors = errs(&format!("{TIMED}{body}"));
            assert!(errors.iter().any(|e| e.line == 5 && e.message.contains(needle)), "{body:?}: {errors:?}");
        };
        e("::press{time=1 result=hit}\n", "needs `key=` (one of: space, enter, left");
        e("::press{key=f5 time=1 result=hit}\n", "needs `key=` (one of: space, enter, left");
        e("::press{key=ab time=1 result=hit}\n", "needs `key=`");
        e("::press{key=space result=hit}\n", "needs `time=");
        e("::press{key=space time=0.2 result=hit}\n", "bad `time=0.2`");
        e("::press{key=space time=86401 result=hit}\n", "bad `time=86401`");
        ok(&format!("{TIMED}::press{{key=space time=86400 result=hit}}\n"));
        ok(&format!("{TIMED}::press{{key=space time=601 result=hit}}\n"));
        e("::press{key=space time=n result=hit}\n", "bad `time=n`");
        e("::press{key=space time=1 count=0 result=hit}\n", "bad `count=0`");
        e("::press{key=space time=1 count=201 result=hit}\n", "bad `count=201`");
        e("::press{key=space time=1 count=2.5 result=hit}\n", "bad `count=2.5`");
        e("::press{key=space time=1}\n", "needs `result=");
        e("::press{key=space time=1 result=n}\n", "needs `result=");
        e("::press{key=space time=1 result=nosuch}\n", "needs `result=");
        e("::press{key=space time=1 result=hit prompt=\"Go{nw}\"}\n", "only work in dialogue");
        e("::press{key=space time=1 result=hit bogus=1}\n", "unknown attribute `bogus`");
    }

    #[test]
    fn timing_errors_name_the_line() {
        let e = |body: &str, needle: &str| {
            let errors = errs(&format!("{TIMED}{body}"));
            assert!(errors.iter().any(|e| e.line == 5 && e.message.contains(needle)), "{body:?}: {errors:?}");
        };
        e("::timing{good=40%..60%}\n", "needs `result=");
        e("::timing{result=nosuch good=40%..60%}\n", "needs `result=");
        e("::timing{result=n good=40%..60%}\n", "needs `result=");
        e("::timing{result=hit}\n", "needs `good=");
        e("::timing{result=hit good=40%}\n", "bad `good=40%`");
        e("::timing{result=hit good=60%..40%}\n", "bad `good=60%..40%`");
        e("::timing{result=hit good=50%..50%}\n", "bad `good=50%..50%`");
        e("::timing{result=hit good=40%..120%}\n", "bad `good=40%..120%`");
        e("::timing{result=hit good=40%..60% perfect=30%..50%}\n", "must lie inside `good`");
        e("::timing{result=hit good=40%..60% perfect=45%..65%}\n", "must lie inside `good`");
        e("::timing{result=hit good=40%..60% perfect=x}\n", "bad `perfect=x`");
        e("::timing{result=hit good=40%..60% speed=0.1}\n", "bad `speed=0.1`");
        e("::timing{result=hit good=40%..60% speed=9}\n", "bad `speed=9`");
        e("::timing{result=hit good=40%..60% time=0}\n", "bad `time=0`");
        e("::timing{result=hit good=40%..60% time=86401}\n", "bad `time=86401`");
        e("::timing{result=hit good=40%..60% bogus=1}\n", "unknown attribute `bogus`");
        e("::timing{result=hit good=40%..60% prompt=\"Go{nw}\"}\n", "only work in dialogue");
        ok(&format!("{TIMED}::timing{{result=hit good=40%..60% perfect=40%..60% speed=5 time=600}}\n"));
        ok(&format!("{TIMED}::timing{{result=hit good=40%..60% time=86400}}\n"));
    }

    const SHOP: &str = "---\nvars: { gold: 40, bag: [], met: false, name: \"x\" }\n---\n::item{id=key price=15}\n::item{id=lamp price=8}\n::item{id=free}\n# a\n";

    #[test]
    fn a_shop_lists_items_with_optional_price_and_condition() {
        let p = ok(&format!("{SHOP}::shop{{pay=gold into=bag}}\n- key\n- lamp{{price=20, if met}}\n* free{{price=0}}\n::\nDone.\n"));
        let Op::Shop { pay, into, stock } = &p.ops[0] else { panic!("not a shop: {:?}", p.ops[0]) };
        assert_eq!((pay.as_str(), into.as_str()), ("gold", "bag"));
        let seen: Vec<(&str, i64, bool)> = stock.iter().map(|s| (s.item.as_str(), s.price, s.cond.is_some())).collect();
        assert_eq!(seen, [("key", 15, false), ("lamp", 20, true), ("free", 0, false)]);
        assert_eq!(p.lines[0], 8);
        assert!(matches!(p.ops[1], Op::Say { .. }));
    }

    #[test]
    fn shop_errors_name_the_line() {
        let e = |body: &str, line: usize, needle: &str| {
            let src = format!("{SHOP}{body}");
            let errors = errs(&src);
            assert!(errors.iter().any(|e| e.line == line && e.message.contains(needle)), "{body:?}: {errors:?}");
        };
        e("::shop{pay=gold into=bag}\n- nope\n::\n", 9, "unknown item `nope`");
        e("::shop{pay=gold into=bag}\n- free\n::\n", 9, "`free` has no price");
        e("::shop{pay=gold into=bag}\n- key{price=-1}\n::\n", 9, "bad `price=-1`");
        e("::shop{pay=gold into=bag}\n- key{price=x}\n::\n", 9, "bad `price=x`");
        e("::shop{pay=gold into=bag}\n- key{bogus}\n::\n", 9, "unexpected `bogus`");
        e("::shop{pay=gold into=bag}\n- key{if nosuch has \"x\"}\n::\n", 9, "not a list");
        e("::shop{pay=gold into=bag}\n- key\n- key\n::\n", 10, "in this shop twice");
        e("::shop{pay=gold into=bag}\nkey\n::\n", 9, "must start with `- `");
        e("::shop{pay=gold into=bag}\n::\n", 8, "has no items");
        e("::shop{pay=gold into=bag}\n- key\n", 8, "never closed");
        e("::shop{pay=name into=bag}\n- key\n::\n", 8, "must be a whole-number variable");
        e("::shop{pay=gold into=gold}\n- key\n::\n", 8, "is not a list");
        e("::shop{pay=gold into=nolist}\n- key\n::\n", 8, "is not a list");
        e("::shop{pay=gold}\n- key\n::\n", 8, "needs `into=");
        e("::shop{into=bag}\n- key\n::\n", 8, "needs `pay=");
        e("::shop{pay=gold into=bag extra=1}\n- key\n::\n", 8, "unknown attribute `extra`");
    }

    #[test]
    fn items_that_nothing_declares_are_found_in_the_inventory_list_only() {
        let p = ok(&format!(
            "{ITEMS}# a\n::set{{bag += \"key\"}}\n::set{{bag += \"kye\"}}\n::set{{other += \"zzz\"}}\n::if{{bag has \"lamp\"}}\nx\n::endif\n"
        ));
        let found: Vec<String> = p.unknown_items().into_iter().map(|(_, t)| t).collect();
        assert_eq!(found, ["kye", "lamp"]);
    }

    #[test]
    fn hidden_is_a_bare_hotspot_option() {
        let p = ok("# a\n::hotspots\n- [Key](#a){rect=0,0,0.1,0.1, hidden, once, found=true}\n- [Door](#a){rect=0.5,0.5,0.1,0.1}\n::\n");
        let Op::Choice { options, .. } = &p.ops[0] else { panic!() };
        assert!(options[0].area.as_ref().unwrap().hidden);
        assert!(!options[1].area.as_ref().unwrap().hidden);
        assert_eq!(options[0].effects.len(), 2, "hidden is not an effect");
        let errors = errs("# a\n::choice\n- [x](#a)\n- [y](#a){hidden}\n::\n");
        assert!(errors.iter().any(|e| e.line == 4 && e.message.contains("`hidden` only belongs")), "{errors:?}");
    }

    #[test]
    fn a_shop_cannot_mix_persistent_and_ordinary_money_and_items() {
        let items = "::item{id=k price=5}\n::shop{pay=gold into=inv}\n- k\n::\n";
        let head = |gold: &str, inv: &str| format!("---\n{gold}\n{inv}\n---\n{items}");
        let (gold, inv) = ("vars: { gold: 9 }", "persistent: { inv: [] }");
        let e = errs(&head(gold, inv));
        assert!(e.iter().any(|e| e.line == 6 && e.message.contains("rolling back")), "{e:?}");
        let (gold, inv) = ("persistent: { gold: 9 }", "vars: { inv: [] }");
        assert!(has_err(&head(gold, inv), "rolling back"));
        ok(&head("vars: { gold: 9 }", "vars: { inv: [] }"));
        ok(&head("persistent: { gold: 9 }", "persistent: { inv: [] }"));
    }

    #[test]
    fn a_region_takes_a_hover_picture_and_call_and_a_block_takes_an_imagemap() {
        let p = ok("# a\n::hotspots{ground=assets/map.png hover=assets/map_hover.png time=3 default=#a}\n- [Door](#a){rect=0,0,0.5,0.5 image=assets/d.png hover=assets/d2.png, call, once}\n- [Hall](#a){rect=0.5,0.5,0.5,0.5 hover=assets/h2.png}\n::\n");
        let Op::Choice { options, map, time } = &p.ops[0] else { panic!() };
        assert_eq!(time, &Some(3.0));
        let map = map.as_ref().unwrap();
        assert_eq!((map.ground.as_deref(), map.hover.as_str()), (Some("assets/map.png"), "assets/map_hover.png"));
        let door = options[0].area.as_ref().unwrap();
        assert_eq!((door.image.as_deref(), door.hover.as_deref(), door.call), (Some("assets/d.png"), Some("assets/d2.png"), true));
        let hall = options[1].area.as_ref().unwrap();
        assert_eq!((hall.image.as_deref(), hall.hover.as_deref(), hall.call), (None, Some("assets/h2.png"), false));
        // `ground` is optional; every picture is an asset, each at its own line
        let p = ok("# a\n::hotspots{hover=map_hover.png}\n- [Hall](#a){rect=0,0,1,1 hover=h.png}\n::\n");
        assert!(matches!(&p.ops[0], Op::Choice { map: Some(m), .. } if m.ground.is_none()));
        let found: Vec<_> = p.assets_with_lines().map(|(l, _, a)| (l, a)).collect();
        assert_eq!(found, [(3, "h.png"), (2, "map_hover.png")]);
        assert!(matches!(&ok("# a\n::hotspots\n- [x](#a){rect=0,0,1,1}\n::\n").ops[0], Op::Choice { map: None, .. }));
    }

    #[test]
    fn imagemap_hover_and_call_errors_name_the_line() {
        let line = |src: &str, needle: &str| {
            let errors = errs(src);
            errors.iter().find(|e| e.message.contains(needle)).unwrap_or_else(|| panic!("{needle}: {errors:?}")).line
        };
        let region = |tail: &str| format!("# a\n::hotspots\n- [x](#a){tail}\n::\n");
        let block = |attrs: &str| format!("# a\n::hotspots{attrs}\n- [x](#a){{rect=0,0,1,1}}\n::\n");
        assert_eq!(line(&region("{rect=0,0,1,1 hover=k.gif}"), "bad `hover="), 3);
        assert_eq!(line(&region("{rect=0,0,1,1 hover=k}"), "bad `hover="), 3);
        assert_eq!(line(&region("{rect=0,0,1,1 hover=../k.png}"), "inside the game folder"), 3);
        assert_eq!(line(&block("{ground=map.png}"), "needs `hover=` too"), 2);
        assert_eq!(line(&block("{hover=map.bmp}"), "bad `hover="), 2);
        assert_eq!(line(&block("{ground=map hover=m.png}"), "bad `ground="), 2);
        assert_eq!(line(&block("{hover}"), "`hover=` needs a picture"), 2);
        // not for a choice
        let choice = |attrs: &str, tail: &str| format!("# a\n::choice{attrs}\n- [x](#a){tail}\n::\n");
        assert_eq!(line(&choice("{ground=g.png hover=h.png}", ""), "unknown attribute `ground` for `::choice`"), 2);
        assert_eq!(line(&choice("{hover=h.png}", ""), "unknown attribute `hover` for `::choice`"), 2);
        assert_eq!(line(&choice("", "{hover=h.png}"), "only belong in a `::hotspots` block"), 3);
        assert_eq!(line(&choice("", "{call}"), "`call` only belongs in a `::hotspots` block"), 3);
        // a call region can't pass the label's parameters
        let with_params = "# a\n::hotspots\n- [x](#h){rect=0,0,1,1, call}\n::\n# h(n)\n::return\n";
        assert_eq!(line(with_params, "has parameters"), 3);
    }

    #[test]
    fn hotspot_errors_name_the_line() {
        let e = |src: &str, needle: &str| {
            let errors = errs(src);
            assert!(errors.iter().any(|e| e.message.contains(needle)), "{src:?}: {errors:?}");
            errors
        };
        let line3 = |s: &str| format!("# a\n::hotspots\n{s}\n::\n");
        let errors = e(&line3("- [x](#a)"), "needs `{rect=");
        assert_eq!(errors.iter().find(|e| e.message.contains("needs `{rect=")).unwrap().line, 3);
        e(&line3("- [x](#a){once}"), "needs `rect=");
        e(&line3("- [x](#a){rect=0.1,0.2,0.3}"), "bad `rect=");
        e(&line3("- [x](#a){rect=0.1,0.2,0.3,wide}"), "bad `rect=");
        e(&line3("- [x](#a){rect=0.9,0.2,0.3,0.1}"), "outside the screen");
        e(&line3("- [x](#a){rect=0.1,0.2,0,0.1}"), "no size");
        e(&line3("- [x](#a){rect=1.5,0,0.1,0.1}"), "bad `rect=");
        e(&line3("- [x](#a){rect=0,0,1,1 image=k.gif}"), "bad `image=");
        e(&line3("- [x](#a){rect=0,0,1,1 image=k}"), "bad `image=");
        e(&line3("- [x](#a){rect=0,0,1,1, bogus}"), "bad assignment");
        e("# a\n::hotspots\n::\n", "has no regions");
        e("# a\n::choice\n- [x](#a){hidden}\n::\n", "`hidden` only belongs in a `::hotspots`");
        e("# a\n::hotspots\n- [x](#a){rect=0,0,1,1}\n", "never closed");
        e("# a\n::choice\n- [x](#a){rect=0,0,1,1}\n::\n", "only belong in a `::hotspots`");
        e("# a\n::hotspots{x}\n- [x](#a){rect=0,0,1,1}\n::\n", "unknown attribute `x` for `::hotspots` (it takes: time, default, ground, hover)");
        e(&line3("- [x](#nowhere){rect=0,0,1,1}"), "unknown label");
    }

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
    fn sprite_size_front_matter() {
        let art = |front: &str| parse(&format!("---\n{front}\n---\nHi.\n")).map(|p| p.art_scale);
        assert_eq!(art("title: T").unwrap(), None);
        assert_eq!(art("sprite_size: fixed").unwrap(), None);
        assert_eq!(art("sprite_size: pixels").unwrap(), Some(1.0));
        assert_eq!(art("art_scale: 1.5\nsprite_size: pixels").unwrap(), Some(1.5));
        for (bad, line) in [("sprite_size: big", 2), ("sprite_size: pixels\nart_scale: 9", 3), ("art_scale: 2", 2)] {
            let e = art(bad).unwrap_err();
            assert_eq!(e[0].line, line, "{bad}: {e:?}");
        }
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
    fn thousands_of_unknown_labels_are_checked_quickly_and_only_the_first_few_get_suggestions() {
        let n = 2000;
        let mut src = String::new();
        for i in 0..n {
            src += &format!("# label number {i}\n::jump{{to=missing{i}}}\n");
        }
        let started = std::time::Instant::now();
        let e = errs(&src);
        assert!(started.elapsed().as_secs() < 5, "took {:?}", started.elapsed());
        assert_eq!(e.len(), n, "{:?}", &e[..3]);
        assert!(e[MAX_SUGGESTED - 1].message.contains("in all") || e[MAX_SUGGESTED - 1].message.contains("none close"), "{:?}", e[MAX_SUGGESTED - 1]);
        assert!(e[MAX_SUGGESTED].message.ends_with("(2000 labels exist)"), "{:?}", e[MAX_SUGGESTED]);
    }

    #[test]
    fn once_options_are_offered_until_picked() {
        let p = ok("# a\n::choice\n- [Ask](#a){once, trust+=1, if trust < 5}\n::\n");
        let Op::Choice { options, .. } = &p.ops[0] else { panic!() };
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
    fn show_greys_an_option_out_with_or_without_a_reason() {
        let p = ok("---\nvars: { str: 1 }\n---\n# a\n::choice\n- [Plain](#a)\n- [Lift](#a){if str >= 5, show=\"Needs STR 5, you have {str}\"}\n- [Pick](#a){show, if str > 3}\n- [Gone](#a){if str > 9}\n::\n");
        let Op::Choice { options, .. } = &p.ops[0] else { panic!() };
        let reasons: Vec<Option<&str>> = options.iter().map(|o| o.lock.as_ref().map(|l| l.reason.as_str())).collect();
        assert_eq!(reasons, [None, Some("Needs STR 5, you have {str}"), Some(""), None]);
        assert!(options[1].cond.is_some(), "`if` is still what unlocks it");
        assert!(options[1].lock.as_ref().unwrap().shown_if.is_none(), "nothing else hides it");
        // `once` keeps hiding a picked option: it stays out of `shown_if`'s reach
        let p = ok("# a\n::choice\n- [Ask](#a){once, if 1 > 2, show=\"Later\"}\n::\n");
        let Op::Choice { options, .. } = &p.ops[0] else { panic!() };
        assert!(options[0].lock.as_ref().unwrap().shown_if.is_some());
    }

    #[test]
    fn show_needs_an_if_a_quoted_reason_and_a_choice_block() {
        let line = |src: &str, msg: &str| {
            let errors = errs(src);
            assert!(errors.iter().any(|e| e.line == 4 && e.message.contains(msg)), "{src}: {errors:?}");
        };
        line("# a\n::choice\n- [x](#a)\n- [y](#a){show}\n::\n", "`show` needs an `if`");
        line("# a\n::choice\n- [x](#a)\n- [y](#a){if 1 > 2, show=Needs STR}\n::\n", "double quotes");
        line("# a\n::choice\n- [x](#a)\n- [y](#a){if 1 > 2, show=\"a\"b\"}\n::\n", "double quotes");
        line("# a\n::choice\n- [x](#a)\n- [y](#a){if 1 > 2, show=\"a{p}b\"}\n::\n", "not in a choice");
        line("# a\n::hotspots\n- [x](#a){rect=0,0,0.1,0.1}\n- [y](#a){rect=0.5,0.5,0.1,0.1, if 1 > 2, show}\n::\n", "`show` only belongs");
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
    fn a_gallery_entry_can_name_its_own_thumbnail() {
        let p = ok("---\ngallery: [a.png, assets/c1.webm | assets/t/c1.png, \"my clip.webm\"|\"my thumb.jpg\"]\n---\nHi.\n");
        assert_eq!(p.gallery, ["a.png", "assets/c1.webm", "my clip.webm"]);
        assert_eq!(p.gallery_thumbs.len(), 2);
        assert_eq!(p.gallery_thumbs["assets/c1.webm"], "assets/t/c1.png");
        assert_eq!(p.gallery_thumbs["my clip.webm"], "my thumb.jpg");
        let files: Vec<_> = p.extra_assets().map(|(_, f)| f).collect();
        assert!(files.contains(&"assets/t/c1.png") && files.contains(&"my thumb.jpg"), "shipped with the game");
        assert!(has_err("---\ngallery: [a.webm | b.webm]\n---\nHi.\n", "must be a picture"));
        assert!(has_err("---\ngallery: [a.webm | ../b.png]\n---\nHi.\n", "inside the game folder"));
        assert!(has_err("---\ngallery: [../a.webm | b.png]\n---\nHi.\n", "inside the game folder"));
        assert!(ok("---\nmusic_room: [a.ogg]\n---\nHi.\n").gallery_thumbs.is_empty());
        // a `|` inside quotes belongs to the name; an empty thumbnail and a music room thumbnail say what is wrong
        assert_eq!(ok("---\ngallery: [\"a|b.png\"]\n---\nHi.\n").gallery, ["a|b.png"]);
        assert!(has_err("---\ngallery: [a.webm |]\n---\nHi.\n", "no thumbnail after it"));
        assert!(has_err("---\nmusic_room: [a.ogg | t.png]\n---\nHi.\n", "`music_room` entries have no thumbnail"));
    }

    #[test]
    fn unlock_names_a_gallery_or_music_room_entry() {
        let p = ok("---\ngallery: [a.webm | t.png]\nmusic_room: [m.ogg]\n---\n::unlock{src=a.webm}\n::unlock{src=m.ogg}\nHi.\n");
        assert_eq!(p.ops[0], Op::Unlock { src: "a.webm".into() });
        assert_eq!(p.ops[1], Op::Unlock { src: "m.ogg".into() });
        assert!(has_err("---\ngallery: [a.webm | t.png]\n---\n::unlock{src=t.png}\n", "not a `gallery:` or `music_room:` entry"));
        assert!(has_err("---\ngallery: [a.png]\n---\nHi.\n::unlock{src=b.png}\n", "b.png"));
        assert!(has_err("---\ngallery: [a.png]\n---\n::unlock\n", "src"));
        let errors = parse("---\ngallery: [a.png]\n---\nHi.\n::unlock{src=b.png}\n").unwrap_err();
        assert_eq!(errors[0].line, 5, "line numbered");
    }

    #[test]
    fn a_gallery_mixes_pictures_and_videos_and_keeps_the_path_rules() {
        let p = ok("---\ngallery: [a.png, assets/clip1.webm, \"my clip.WEBM\"]\ngallery: [assets/clip1.webm]\n---\nHi.\n");
        assert_eq!(p.gallery, ["a.png", "assets/clip1.webm", "my clip.WEBM"]);
        let kinds: Vec<_> = p.extra_assets().map(|(k, _)| k).collect();
        assert_eq!(kinds, [AssetKind::Image, AssetKind::Video, AssetKind::Video]);
        assert!(has_err("---\ngallery: [../x.webm]\n---\nHi.\n", "inside the game folder"));
        assert!(has_err("---\nvars: { n: \"a\" }\ngallery: [clips/{n}.webm]\n---\nHi.\n", "{"));
    }

    #[test]
    fn text_tags_are_checked_where_they_are_written() {
        ok("Some **bold**, *italic*, {color=#f80}colour{/color} and a pause{w=0.3}.\n::choice\n- [*Quietly*](#a)\n::\n# a\nX.\n");
        let e = errs("A line.\nAn **unclosed line.\n\nNext.\n");
        assert_eq!((e[0].line, e[0].message.contains("bold")), (1, true), "the paragraph's first line");
        assert!(has_err("::choice\n- [{bogus=9}x](#a)\n::\n# a\nX.\n", "unknown text tag"));
        ok("Big {size=+14}words{/size}, {size=24}small{/size}.{nw}\n::choice\n- [{size=+4}Go{/size}](#a)\n::\n# a\nX.\n");
        let e = errs("Fine.\n\nOops {size=300}x{/size}\n");
        assert_eq!((e[0].line, e[0].message.contains("8 to 200")), (3, true));
        let e = errs("A.\nB {nw} c\n");
        assert_eq!((e[0].line, e[0].message.contains("last thing")), (1, true));
        assert!(has_err("::choice\n- [Go{nw}](#a)\n::\n# a\nX.\n", "not in a choice"));
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
    }

    #[test]
    fn x_and_y_take_keywords_numbers_and_percentages() {
        let p = ok("::show{src=a.png id=a x=left y=top}\n::show{src=a.png id=b x=center y=center}\n::show{src=a.png id=c x=right y=bottom}\n::show{src=a.png id=d x=25% y=0.5}\n::show{src=a.png id=e x=-50% y=150%}\n");
        let xy = |i: usize| match &p.ops[i] {
            Op::Show { x, y, .. } => (x.unwrap(), y.unwrap()),
            _ => panic!(),
        };
        assert_eq!([xy(0), xy(1), xy(2), xy(3), xy(4)], [(0.0, 0.0), (0.5, 0.5), (1.0, 1.0), (0.25, 0.5), (-0.5, 1.5)]);
        // a sprite may leave the screen, but not for good; a keyword belongs to its own axis
        assert!(has_err("::show{src=a.png x=400%}\n", "bad `x=400%`"));
        assert!(has_err("::show{src=a.png x=top}\n", "bad `x=top`"));
        assert!(has_err("::show{src=a.png y=left}\n", "bad `y=left`"));
        let msg = |src: &str| parse(src).unwrap_err()[0].message.clone();
        assert!(msg("::show{src=a.png y=far-left}\n").contains("try `y=12%`"), "the hint names the key it is about");
        assert!(!msg("::show{src=a.png x=sideways}\n").ends_with("))") && msg("::show{src=a.png x=sideways}\n").ends_with("%)"), "no stray bracket");
        // the camera never leaves the stage
        assert!(has_err("::camera{x=150%}\n", "bad `x=150%`") && matches!(ok("::camera{zoom=2 x=right y=top}\n").ops[0], Op::Camera { x: Some(x), y: Some(y), .. } if x == 1.0 && y == 0.0));
    }

    #[test]
    fn the_old_position_forms_are_errors_that_name_the_new_one() {
        assert!(has_err("::show{src=a.png at=left}\n", "`at=` is gone: use `x=`"));
        assert!(has_err("::move{a at=left}\n", "`at=` is gone"));
        assert!(has_err("::hud{id=a text=x at=0.1,0.2}\n", "`at=` is gone: use `x=` (left, center, right or a %) and `y=`"));
        assert!(has_err("::hud{id=a text=x x=0.1 anchor=right}\n", "`anchor=` is gone"));
        assert!(has_err("::show{src=a.png x=far-left}\n", "`far-left` and `far-right` are gone: try `x=12%`"));
        assert!(has_err("::notify{text=x at=1}\n", "unknown attribute `at`"), "only where x= exists");
    }

    // ---- list variables ----

    fn list(items: &[&str]) -> Value {
        Value::List(items.iter().map(|s| s.to_string()).collect())
    }

    #[test]
    fn lists_are_declared_in_vars_and_persistent() {
        let p = ok("---\nvars: { gold: 30, inventory: [], bag: [map, \"magic key\", map], note: \"a, b\" }\npersistent: { seen_items: [] }\n---\nHi.\n");
        assert_eq!(p.vars["inventory"], list(&[]));
        assert_eq!(p.vars["bag"], list(&["map", "magic key"]));
        assert_eq!(p.vars["gold"], Value::Int(30));
        assert_eq!(p.vars["note"], Value::Str("a, b".into()));
        assert_eq!(p.vars["seen_items"], list(&[]));
        assert!(p.persistent.contains("seen_items"));
        assert!(has_err("---\nvars: { inv: [a,,b] }\n---\nHi.\n", "bad list item"));
    }

    #[test]
    fn has_and_list_effects_compile() {
        let src = "---\nvars: { inv: [] }\n---\n::set{inv += \"key\", inv -= 'old'}\n::if{inv has \"key\" and not inv has \"x\"}\nYes.\n::endif\n::jump{to=a if=\"inv has 'key'\"}\n# a\n::choice\n- [Buy](#a){inv += \"gem\", if not inv has \"gem\"}\n::\n";
        let p = ok(src);
        let Op::Set(e) = &p.ops[0] else { panic!() };
        assert_eq!((e.var.as_str(), e.op, &e.value), ("inv", AssignOp::Add, &Expr::Lit(Value::Str("key".into()))));
        let options = p.ops.iter().find_map(|op| match op { Op::Choice { options, .. } => Some(options), _ => None }).unwrap();
        assert_eq!(options[0].effects[0].op, AssignOp::Add);
        let has = |s: &str| Expr::Has("inv".into(), Box::new(Expr::Lit(Value::Str(s.into()))));
        assert_eq!(options[0].cond, Some(Expr::Not(Box::new(has("gem")))));
        assert!(p.ops.iter().any(|op| matches!(op, Op::Jump { cond: Some(c), .. } if *c == has("key"))));
    }

    #[test]
    fn a_declared_list_can_be_set_or_cleared_with_a_list_literal() {
        let head = "---\nvars: { inv: [a], gold: 5 }\n---\n";
        let p = ok(&format!("{head}::set{{inv = [], inv = [\"key\", \"magic map\", key]}}\n# a\n::choice\n- [x](#a){{inv = [\"k\"], gold += 1}}\n::\n"));
        let sets: Vec<_> = p.ops.iter().filter_map(|o| if let Op::Set(e) = o { Some(e) } else { None }).collect();
        assert_eq!(sets[0].value, Expr::Lit(Value::List(vec![])));
        assert_eq!(sets[1].value, Expr::Lit(Value::List(vec!["key".into(), "magic map".into()])), "repeats dropped");
        assert!(p.ops.iter().any(|o| matches!(o, Op::Choice { options, .. } if options[0].effects.iter().any(|e| e.var == "inv"))));
        // only a declared list; `+=` takes an item; a bad item is an error
        for (tail, needle) in [
            ("::set{gold = []}\n", "is not a list"),
            ("::set{nope = [\"a\"]}\n", "is not a list"),
            ("::set{inv += [\"a\"]}\n", "unexpected"),
            ("::set{inv = [\"\"]}\n", "bad list item"),
            ("::set{inv = [a, \"b\"}\n", "is not a list"),
        ] {
            assert!(has_err(&format!("{head}{tail}"), needle), "{tail}: {:?}", errs(&format!("{head}{tail}")));
        }
    }

    #[test]
    fn pick_is_for_assignments_like_random() {
        let head = "---\nvars: { inv: [a], who: \"\", n: 0 }\n---\n";
        let p = ok(&format!("{head}::set{{who = pick(inv), who = pick(\"A\", \"B\"), n = pick(1, 2, 3) + 1}}\n"));
        assert!(p.ops.iter().any(|o| matches!(o, Op::Set(e) if matches!(e.value, Expr::Pick(_)))));
        for (tail, needle) in [
            ("::if{pick(\"a\", \"b\") == \"a\"}\nHi.\n::endif\n", "only be used on the right of an assignment"),
            ("::set{who = pick()}\n", "takes at least 1"),
            ("::set{who = pick(random(1, 2))}\n", "assign it to a variable first"),
        ] {
            assert!(has_err(&format!("{head}{tail}"), needle), "{tail}: {:?}", errs(&format!("{head}{tail}")));
        }
    }

    #[test]
    fn list_mistakes_are_errors_with_line_numbers() {
        let head = "---\nvars: { inv: [], gold: 5 }\n---\n";
        let line = |tail: &str, needle: &str| {
            let e = errs(&format!("{head}{tail}"));
            let hit = e.iter().find(|e| e.message.contains(needle));
            assert!(hit.is_some(), "{tail:?}: {e:?}");
            hit.unwrap().line
        };
        assert_eq!(line("Hi.\n::set{inv = \"x\"}\n", "not `= value`"), 5);
        assert_eq!(line("Hi.\n::set{inv += 3}\n", "non-empty text"), 5);
        assert_eq!(line("::set{inv -= true}\n", "non-empty text"), 4);
        assert_eq!(line("::set{inv += \"\"}\n", "non-empty text"), 4);
        assert_eq!(line("# a\n::choice\n- [x](#a){inv += \"k\"}\n- [y](#a){inv = 1}\n::\n", "not `= value`"), 7);
        assert_eq!(line("::if{gold has \"x\"}\nHi.\n::endif\n", "not a list"), 4);
        assert_eq!(line("::if{nothing has \"x\"}\nHi.\n::endif\n", "declare it"), 4);
        assert_eq!(line("# a\n::jump{to=a if=\"gold has 'x'\"}\n", "not a list"), 5);
        assert_eq!(line("# a\n::choice\n- [x](#a){if gold has \"x\"}\n::\n", "not a list"), 6);
        // a bare list as a value is never meaningful
        assert_eq!(line("Hi.\n::set{gold = inv}\n", "can't be the right side"), 5);
        assert_eq!(line("::set{gold += inv}\n", "can't be the right side"), 4);
        assert_eq!(line("::set{inv += inv}\n", "can't be the right side"), 4);
        assert_eq!(line("# a\n::choice\n- [x](#a){gold = inv}\n::\n", "can't be the right side"), 6);
        assert_eq!(line("::call{to=h x=inv}\n::end\n# h(x)\nHi.\n::return\n", "`::call` argument"), 4);
        assert!(!ok(&format!("{head}::set{{gold = (inv has \"k\")}}\n")).ops.is_empty(), "asking about a list is fine");
    }

    // ---- arithmetic, functions, dotted names, random ----------------------

    #[test]
    fn expressions_work_wherever_a_value_or_condition_does() {
        let src = "---\nvars: { hero.hp: 10, gold: 20, bag: [], price: 3 }\n---\n\
            ::set{gold -= price * 2 + 1, hero.hp = min(hero.hp + 4, 12)}\n\
            ::if{hero.hp - gold > -20 and count(bag) == 0}\nYes.\n::elif{abs(gold) % 2 == 1}\nOdd.\n::endif\n\
            ::jump{to=a if=\"hero.hp * 2 > max(gold, 5)\"}\n\
            # a\n::call{to=h x=(price * 2) y=min(1,2)}\n::end\n\
            ::choice\n- [Rest](#a){hero.hp += 2 * price, gold = gold / 2, if gold >= price + 1}\n::\n::end\n\
            # h(x, y)\nHi {hero.hp}.\n::return\n";
        let p = ok(src);
        assert_eq!(p.vars["hero.hp"], Value::Int(10));
        let Op::Set(e) = &p.ops[0] else { panic!() };
        assert_eq!((e.var.as_str(), e.op), ("gold", AssignOp::Sub));
        assert!(matches!(&e.value, Expr::Bin(_, BinOp::Add, _)));
        let Op::Set(e) = &p.ops[1] else { panic!() };
        assert_eq!(e.var, "hero.hp");
        assert!(matches!(&e.value, Expr::Func(Func::Min, args) if args.len() == 2));
        let call = p.ops.iter().find_map(|op| match op { Op::Call { args, .. } => Some(args), _ => None }).unwrap();
        assert!(matches!(call[0].1, Expr::Bin(_, BinOp::Mul, _)) && matches!(call[1].1, Expr::Func(Func::Min, _)));
        let options = p.ops.iter().find_map(|op| match op { Op::Choice { options, .. } => Some(options), _ => None }).unwrap();
        assert_eq!(options[0].effects.len(), 2);
        assert!(matches!(&options[0].cond, Some(Expr::Cmp(_, Cmp::Ge, r)) if matches!(**r, Expr::Bin(_, BinOp::Add, _))));
    }

    #[test]
    fn dotted_names_in_declarations_effects_and_text() {
        let src = "---\nvars: { hero.love: 0, stats.hp.max: 10, bag.items: [], who.name: \"Mika\" }\npersistent: { seen.end: false }\ninventory: bag.items\n---\n\
            ::set{hero.love += 1, bag.items += \"key\", seen.end = true}\n\
            ::if{bag.items has \"key\" and hero.love >= 1}\nHi {who.name}, love {hero.love}/{stats.hp.max}.\n::endif\n";
        let p = ok(src);
        assert!(p.vars.contains_key("hero.love") && p.vars.contains_key("stats.hp.max"));
        assert!(p.persistent.contains("seen.end"));
        assert_eq!(p.inventory.as_deref(), Some("bag.items"));
        assert!(has_err("---\nvars: { a..b: 0 }\n---\nHi.\n", "bad variable"));
        assert!(has_err("---\nvars: { a.: 0 }\n---\nHi.\n", "bad variable"));
        assert!(has_err("---\nvars: { .a: 0 }\n---\nHi.\n", "bad variable"));
        assert!(has_err("---\nvars: { and: 0 }\n---\nHi.\n", "keyword"));
        assert!(has_err("::set{a. = 1}\n", "bad assignment"));
        assert!(has_err("---\nvars: { hero.hp: 1.5 }\n---\nHi.\n", "not a whole number"));
        assert!(has_err("::set{x = 1.5}\n", "not a whole number"));
        assert!(has_err("::if{x > 0.5}\nHi.\n::endif\n", "not a whole number"));
        // other uses of a dot are untouched
        let p = ok("::bg{src=assets/a.png}\n::show{x src=assets/b.v2.png}\n::wait{time=0.5}\nHi{w=0.5} there.\n");
        assert!(!p.ops.is_empty());
        assert!(has_err("::character{id=a.b name=A}\n", "character id"));
        assert!(has_err("::call{to=h a.b=1}\n::end\n# h(x)\nHi.\n::return\n", "argument name"));
        assert!(has_err("::end\n# h(a.b)\nHi.\n::return\n", "bad parameter"));
    }

    #[test]
    fn arithmetic_mistakes_are_errors_with_line_numbers() {
        let head = "---\nvars: { n: 1, t: \"x\", inv: [] }\n---\n";
        let line = |tail: &str, needle: &str| {
            let e = errs(&format!("{head}{tail}"));
            let hit = e.iter().find(|e| e.message.contains(needle));
            assert!(hit.is_some(), "{tail:?}: {e:?}");
            hit.unwrap().line
        };
        assert_eq!(line("Hi.\n::set{n = t + 1}\n", "is not a number"), 5);
        assert_eq!(line("::set{n = 1 + \"a\"}\n", "the text \"a\" is not a number"), 4);
        assert_eq!(line("::if{n * inv > 1}\nHi.\n::endif\n", "(a list)"), 4);
        assert_eq!(line("::set{n = -t}\n", "is not a number"), 4);
        assert_eq!(line("::set{n = max(t, 1)}\n", "is not a number"), 4);
        assert_eq!(line("::set{n = n / 0}\n", "by 0"), 4);
        assert_eq!(line("# a\n::choice\n- [x](#a){n = n % 0}\n::\n", "by 0"), 6);
        assert_eq!(line("::set{n = count(n)}\n", "is not a list"), 4);
        assert_eq!(line("::set{n = count(nothing)}\n", "declare it"), 4);
        assert_eq!(line("::set{n = count(1)}\n", "takes a list variable"), 4);
        assert_eq!(line("::set{n = len(inv)}\n", "unknown function `len`"), 4);
        assert_eq!(line("::set{n = min(1)}\n", "at least 2 values"), 4);
        assert_eq!(line("::set{n = abs(1, 2)}\n", "abs(x)"), 4);
        assert_eq!(line("::set{inv += n + 1}\n", "not a number"), 4);
        assert_eq!(line("::set{n = random(5, 1)}\n", "lowest value comes first"), 4);
        // text += still appends, and numbers add up; neither is an error
        assert!(!ok(&format!("{head}::set{{t += \"y\", t += n * 2, n += n}}\n")).ops.is_empty());
    }

    #[test]
    fn random_is_only_allowed_when_assigning() {
        let head = "---\nvars: { roll: 0, bonus: 1, bag: [] }\n---\n";
        let p = ok(&format!("{head}::set{{roll = random(1, 6), roll = random(2, 4) + bonus}}\n# a\n::choice\n- [x](#a){{roll = random(1, 3)}}\n::\n"));
        assert!(matches!(&p.ops[0], Op::Set(e) if matches!(e.value, Expr::Random(..))));
        assert!(matches!(&p.ops[1], Op::Set(e) if e.value.has_random()));
        let line = |tail: &str| {
            let e = errs(&format!("{head}{tail}"));
            let hit = e.iter().find(|e| e.message.contains("assign it to a variable first"));
            assert!(hit.is_some(), "{tail:?}: {e:?}");
            hit.unwrap().line
        };
        assert_eq!(line("::if{random(1, 6) > 3}\nHi.\n::endif\n"), 4);
        assert_eq!(line("Hi.\n::if{roll == 1}\nA.\n::elif{random(1, 2) == 1}\nB.\n::endif\n"), 7);
        assert_eq!(line("# a\n::jump{to=a if=\"random(1, 2) == 1\"}\n"), 5);
        assert_eq!(line("# a\n::choice\n- [x](#a){if random(0, 1)}\n::\n"), 6);
        assert_eq!(line("::call{to=h x=random(1, 2)}\n::end\n# h(x)\nHi.\n::return\n"), 4);
        assert_eq!(line("::item{id=k name=Key}\n::shop{pay=roll into=bag}\n- k{price=1, if random(0, 1)}\n::\n"), 6);
    }

    // ---- labels with parameters ------------------------------------------

    /// A story with a parameterized subroutine after the main flow.
    const SUB: &str = "::call{to=heal amount=5 who=\"Mika\"}\n**A:** back\n::end\n# heal(amount, who)\n**A:** {who} heals {amount}\n::return\n";

    fn line_of(src: &str, needle: &str) -> usize {
        let e = errs(src);
        e.iter().find(|e| e.message.contains(needle)).unwrap_or_else(|| panic!("{src:?}: {e:?}")).line
    }

    #[test]
    fn a_label_with_parameters_is_called_with_values() {
        let p = ok(SUB);
        assert_eq!(p.labels["heal"], 3, "the id is the heading text before the parenthesis");
        assert_eq!(p.params["heal"], ["amount", "who"]);
        let lit = |v| Expr::Lit(v);
        assert_eq!(
            p.ops[0],
            Op::Call {
                target: 3,
                args: vec![
                    ("amount".into(), lit(Value::Int(5))),
                    ("who".into(), lit(Value::Str("Mika".into()))),
                ]
            }
        );
    }

    #[test]
    fn a_parameter_may_have_a_literal_default_that_a_call_without_it_passes() {
        let src = "::call{to=hit}\n::call{to=hit power=9}\n::end\n# hit(power=1, who=\"Mi, (ka)\", loud = true, delta=-2, need)\n::return\n";
        let src = src.replace("::call{to=hit}", "::call{to=hit need=1}").replace("power=9", "power=9 need=2");
        let p = ok(&src);
        assert_eq!(p.params["hit"], ["power", "who", "loud", "delta", "need"]);
        let d = &p.param_defaults["hit"];
        assert_eq!(d.len(), 4, "`need` has none");
        assert_eq!((&d["power"], &d["who"]), (&Value::Int(1), &Value::Str("Mi, (ka)".into())));
        assert_eq!((&d["loud"], &d["delta"]), (&Value::Bool(true), &Value::Int(-2)));
        let arg = |op: usize, name: &str| match &p.ops[op] {
            Op::Call { args, .. } => args.iter().find(|(k, _)| k == name).map(|(_, e)| e.clone()),
            _ => panic!(),
        };
        assert_eq!(arg(0, "power"), Some(Expr::Lit(Value::Int(1))), "left out: the default");
        assert_eq!(arg(1, "power"), Some(Expr::Lit(Value::Int(9))), "given: kept, once");
        let Op::Call { args, .. } = &p.ops[1] else { panic!() };
        assert_eq!(args.len(), 5);
        assert!(ok("::call{to=f}\n::end\n# f(a=1)\n::return\n").param_defaults.contains_key("f"));
        assert!(ok("::end\n# f(a)\n::return\n").param_defaults.is_empty());
    }

    #[test]
    fn a_parameter_without_a_default_stays_required_and_names_stay_checked() {
        let tail = "::end\n# hit(power=1, need)\n::return\n";
        assert_eq!(line_of(&format!("::call{{to=hit}}\n{tail}"), "missing `need"), 1);
        ok(&format!("::call{{to=hit need=1}}\n{tail}"));
        assert!(has_err(&format!("::call{{to=hit need=1 powr=2}}\n{tail}"), "no parameter `powr`"));
        assert!(has_err(&format!("::call{{to=hit}}\n{tail}"), "power (optional)"), "the error says which are optional");
    }

    #[test]
    fn a_bad_default_is_an_error_on_the_heading() {
        for head in ["f(a=b)", "f(a=1 + 2)", "f(a=)", "f(a=[1])", "f(a=random(1, 2))", "f(a=1, a=2)", "f(1=2)", "f(a=\"x)"] {
            let src = format!("::end\n# {head}\n::return\n");
            assert!(!errs(&src).is_empty(), "{head}");
        }
        assert_eq!(line_of("::end\n# f(a=b)\n::return\n", "default of `a`"), 2);
    }

    #[test]
    fn arguments_are_expressions_with_quotes_and_parentheses_keeping_their_spaces() {
        let src = "---\nvars: { gold: 3 }\n---\n::call{to=f a=gold b=\"two words, here\" c=(gold >= 2 and true), d=-4}\n::end\n# f(a, b, c, d)\n::return\n";
        let p = ok(src);
        let Op::Call { args, .. } = &p.ops[0] else { panic!() };
        assert_eq!(args[0].1, Expr::Var("gold".into()));
        assert_eq!(args[1].1, Expr::Lit(Value::Str("two words, here".into())));
        assert!(matches!(args[2].1, Expr::And(..)));
        assert_eq!(args[3].1, Expr::Lit(Value::Int(-4)));
        // the label may also come first and bare, and `to` may be quoted
        ok("::call{f a=1}\n::end\n# f(a)\n::return\n");
        ok("::call{to=\"f\" a=1}\n::end\n# f(a)\n::return\n");
        ok("::call{to=f, a=1}\n::end\n# f(a)\n::return\n");
    }

    #[test]
    fn a_call_must_pass_exactly_the_parameters() {
        let tail = "::end\n# heal(amount, who)\n::return\n";
        assert_eq!(line_of(&format!("::call{{to=heal who=\"M\"}}\n{tail}"), "missing `amount"), 1);
        assert_eq!(line_of(&format!("Hi.\n::call{{to=heal}}\n{tail}"), "missing `amount"), 2);
        assert_eq!(line_of(&format!("::call{{to=heal amount=1 who=\"M\" extra=2}}\n{tail}"), "no parameter `extra`"), 1);
        assert_eq!(line_of(&format!("::call{{to=heal amont=1 who=\"M\"}}\n{tail}"), "no parameter `amont`"), 1);
        assert!(has_err(&format!("::call{{to=heal amont=1 who=\"M\"}}\n{tail}"), "missing `amount"), "the misspelling is both");
        assert!(has_err("::call{to=heal amount=1 amount=2 who=1}\n::end\n# heal(amount, who)\n::return\n", "given twice"));
        // a label without parameters takes no arguments
        assert_eq!(line_of("\n::call{to=plain x=1}\n::end\n# plain\n::return\n", "takes no parameters"), 2);
        // and a bad value is reported on its line
        assert_eq!(line_of("Hi.\n::call{to=heal amount=?? who=1}\n::end\n# heal(amount, who)\n::return\n", "amount"), 2);
        assert!(has_err("::call{to=heal amount=1 who=\"x}\n", "unterminated quote"));
        assert!(has_err("::call{amount=1}\n", "needs a label"));
    }

    #[test]
    fn jumps_and_links_cannot_reach_a_parameterized_label() {
        let sub = "# heal(amount)\n::return\n";
        assert_eq!(line_of(&format!("Hi.\n::jump{{to=heal}}\n::end\n{sub}"), "can't supply them"), 2);
        assert_eq!(line_of(&format!("::jump{{to=heal if=\"1\"}}\n::end\n{sub}"), "`::jump`"), 1);
        let choice = format!("::choice\n- [Go](#heal)\n::\n::end\n{sub}");
        assert_eq!(line_of(&choice, "a choice or hotspot link"), 2);
        let hot = format!("::hotspots\n- [Go](#heal){{rect=0,0,0.5,0.5}}\n::\n::end\n{sub}");
        assert_eq!(line_of(&hot, "a choice or hotspot link"), 2);
        // `seen()` only counts passes, so it is fine
        ok(&format!("::if{{seen(heal)}}\nHi.\n::endif\n::call{{to=heal amount=1}}\n::end\n{sub}"));
    }

    #[test]
    fn running_into_a_parameterized_label_is_an_error() {
        let heal = "# heal(amount)\n::return\n";
        // from the label above, from the start of the script, and past an `::if`
        assert_eq!(line_of(&format!("# main\nHi.\n{heal}"), "without a `::call`"), 3);
        assert_eq!(line_of(heal, "without a `::call`"), 1);
        // a title like `# Round 1(a)` is read as a parameter list: the error says so
        assert!(has_err("Hi.\n# Round 1(a)\nHo.\n", "put a space before `(` for a plain title"));
        assert!(has_err("::jump{to=round-1}\n# Round 1(a)\n::return\n", "put a space before `(` for a plain title"));
        assert_eq!(line_of(&format!("::if{{x}}\n::jump{{to=main}}\n::endif\n{heal}# main\n::end\n"), "without a `::call`"), 4);
        // a choice block with nothing on offer carries on past itself
        assert_eq!(line_of(&format!("# main\n::choice\n- [x](#main){{if 0}}\n::\n{heal}"), "without a `::call`"), 5);
        // ending the label above stops that
        for end in ["::jump{to=main}", "::return", "::end"] {
            ok(&format!("# main\n{end}\n{heal}"));
        }
        ok(&format!("::call{{to=heal amount=1}}\n::end\n{heal}"));
    }

    #[test]
    fn heading_parentheses_only_make_parameters_right_after_the_name() {
        // the old behaviour: spaces before the parenthesis (or none at the end) is plain title text
        let p = ok("# Chapter 1 (the start)\nHi.\n# Plain (\nHi.\n");
        assert!(p.labels.contains_key("chapter-1-the-start") && p.params.is_empty());
        assert!(p.labels.contains_key("plain"));
        // with a space the parentheses are part of the title, and the label is its slug
        let p = ok("# heal (amount)\nHi.\n");
        assert!(p.labels.contains_key("heal-amount") && p.params.is_empty());
        assert!(!ok("# Round 1 (a)\nHi.\n").labels.contains_key("round-1"));
        // the examples the manual gives for the touching form
        let p = ok("::call{to=round-1 a=1}\n::call{to=café night=2}\n::end\n# Round 1(a)\n::return\n# Café(night)\n::return\n");
        assert_eq!((p.params["round-1"].as_slice(), p.params["café"].as_slice()), (&["a".to_string()][..], &["night".to_string()][..]));
        let p = ok("::call{to=\"Price Line\" x=1}\n::end\n# Price line(x)\n::return\n");
        assert_eq!(p.params["price-line"], ["x"]);
        assert!(p.labels.contains_key("price-line"));
        for (src, needle) in [
            ("# f()\n", "no parameters"),
            ("# f(a,)\n", "bad parameter"),
            ("# f(a b)\n", "bad parameter"),
            ("# f(1)\n", "bad parameter"),
            ("# f(a, a)\n", "listed twice"),
            ("# f(not)\n", "keyword"),
            ("# f(to)\n", "`to=`"),
            ("# f(p)\n", "text tags"),
        ] {
            assert!(has_err(src, needle), "{src}");
        }
        assert!(has_err("---\nvars: { inv: [] }\n---\n# f(inv)\n", "list variable"));
        assert!(has_err("# f(a)\n::return\n# f(b)\n::return\n", "duplicate label"));
    }

    #[test]
    fn parameterized_labels_work_with_seen_and_old_scripts_are_unchanged() {
        let p = ok("::call{to=heal amount=1}\n::if{seen(heal) >= 1}\nHi.\n::endif\n::end\n# heal(amount)\n::return\n");
        assert!(p.counted.contains_key(&p.labels["heal"]));
        assert_eq!(ok(DEMO).params.len(), 0);
    }

    // ---- ::flash --------------------------------------------------------

    #[test]
    fn flash_has_defaults_and_validates() {
        let p = ok("::flash\n::flash{time=1.5 color=#f00}\n::flash{color=#00ff80}\n");
        assert_eq!(p.ops[0], Op::Flash { time: 0.4, color: [1.0, 1.0, 1.0] });
        assert_eq!(p.ops[1], Op::Flash { time: 1.5, color: [1.0, 0.0, 0.0] });
        assert_eq!(p.ops[2], Op::Flash { time: 0.4, color: [0.0, 1.0, 128.0 / 255.0] });
        for (src, needle) in [
            ("::flash{time=0}\n", "above 0"),
            ("::flash{time=-1}\n", "above 0"),
            ("::flash{time=10.5}\n", "at most 10"),
            ("::flash{time=soon}\n", "time=soon"),
            ("::flash{color=red}\n", "bad colour"),
            ("::flash{colour=#fff}\n", "unknown attribute `colour`"),
        ] {
            assert!(has_err(src, needle), "{src}");
        }
        assert_eq!(line_of("Hi.\n::flash{time=0}\n", "above 0"), 2);
        assert!(ok("::flash{time=10}\n").ops.contains(&Op::Flash { time: 10.0, color: [1.0; 3] }));
    }

    // ---- ::notify -------------------------------------------------------

    #[test]
    fn notify_has_a_default_time_and_validates() {
        let p = ok("::notify{text=\"Saved {x}\"}\n::notify{text=Hi time=0.5}\n");
        assert_eq!(p.ops[0], Op::Notify { text: "Saved {x}".into(), time: 3.0 });
        assert_eq!(p.ops[1], Op::Notify { text: "Hi".into(), time: 0.5 });
        for (src, needle) in [
            ("::notify\n", "needs `text="),
            ("::notify{text=\" \"}\n", "needs `text="),
            ("::notify{text=x time=0.2}\n", "from 0.5 to 30"),
            ("::notify{text=x time=31}\n", "from 0.5 to 30"),
            ("::notify{text=x time=soon}\n", "time=soon"),
            ("::notify{text=\"a{p}b\"}\n", "only work in dialogue"),
            ("::notify{text=x at=1}\n", "unknown attribute `at`"),
        ] {
            assert!(has_err(src, needle), "{src}");
        }
        assert_eq!(line_of("Hi.\n::notify{time=1}\n", "needs `text="), 2);
    }

    // ---- looping and stopping ::sfx ---------------------------------------

    #[test]
    fn sfx_loops_and_stops() {
        let p = ok("::sfx{src=rain.ogg loop volume=0.6 fade=1}\n::sfx{src=a.ogg loop}\n::sfx{src=door.ogg}\n::sfx{src=b.ogg loop=false}\n::sfx{stop=rain.ogg fade=2}\n::sfx{stop=door.ogg}\n::sfx{stop}\n::sfx{stop fade=0.5}\n");
        assert_eq!(p.ops[0], Op::SfxLoop { src: "rain.ogg".into(), fade: 1.0, volume: 0.6 });
        assert_eq!(p.ops[1], Op::SfxLoop { src: "a.ogg".into(), fade: 0.0, volume: 1.0 });
        assert_eq!(p.ops[2], Op::Sfx { src: "door.ogg".into(), volume: 1.0 }, "a one-shot is unchanged");
        assert_eq!(p.ops[3], Op::Sfx { src: "b.ogg".into(), volume: 1.0 });
        assert_eq!(p.ops[4], Op::StopSfx { src: Some("rain.ogg".into()), fade: 2.0 });
        assert_eq!(p.ops[5], Op::StopSfx { src: Some("door.ogg".into()), fade: 0.0 }, "a one-shot can be stopped too");
        assert_eq!(p.ops[6], Op::StopSfx { src: None, fade: 0.0 });
        assert_eq!(p.ops[7], Op::StopSfx { src: None, fade: 0.5 });
        assert_eq!(p.assets().filter(|(_, k, _)| *k == AssetKind::Audio).count(), 4, "loops are assets; stops are not");
    }

    #[test]
    fn sfx_mistakes_are_errors_with_lines() {
        for (src, needle) in [
            ("::sfx{src=a.ogg fade=1}\n", "needs `loop`"),
            ("::sfx{src=a.ogg loop volume=2}\n", "between 0 and 1"),
            ("::sfx{src=a.ogg loop fade=-1}\n", "seconds"),
            ("::sfx{stop=a.ogg src=a.ogg}\n", "doesn't go with"),
            ("::sfx{stop loop}\n", "doesn't go with"),
            ("::sfx{stop=}\n", "needs a file"),
            ("::sfx{loop}\n", "needs `src=...`"),
            ("::sfx{stop=never.ogg}\n", "no `::sfx{src=never.ogg}`"),
            ("::sfx{src=a.ogg loop speed=2}\n", "unknown attribute `speed`"),
        ] {
            assert!(has_err(src, needle), "{src}");
        }
        assert_eq!(line_of("Hi.\n::sfx{stop=never.ogg}\n", "no `::sfx"), 2);
        ok("::sfx{src=a.ogg loop}\n::sfx{stop=a.ogg}\n");
    }

    // ---- ::fade and through= -----------------------------------------------

    #[test]
    fn fade_out_and_in_have_defaults() {
        let p = ok("::fade{out}\n::fade{in}\n::fade{out time=2 color=#ff8000}\n::fade{in time=1.5}\n");
        assert_eq!(p.ops[0], Op::FadeOut { time: 0.5, color: [0.0; 3] });
        assert_eq!(p.ops[1], Op::FadeIn { time: 0.5 });
        assert_eq!(p.ops[2], Op::FadeOut { time: 2.0, color: [1.0, 128.0 / 255.0, 0.0] });
        assert_eq!(p.ops[3], Op::FadeIn { time: 1.5 });
        assert_eq!(ok("::fade{out color=#fff time=10}\n").ops[0], Op::FadeOut { time: 10.0, color: [1.0; 3] });
    }

    #[test]
    fn fade_errors_name_the_line() {
        for (src, needle) in [
            ("::fade\n", "needs `out`"),
            ("::fade{time=1}\n", "needs `out`"),
            ("::fade{out in}\n", "not both"),
            ("::fade{in color=#fff}\n", "takes no `color`"),
            ("::fade{out color=red}\n", "bad colour"),
            ("::fade{out color=#12}\n", "bad colour"),
            ("::fade{out time=0}\n", "time=0"),
            ("::fade{in time=-1}\n", "time=-1"),
            ("::fade{out time=10.5}\n", "time=10.5"),
            ("::fade{in time=soon}\n", "time=soon"),
            ("::fade{out speed=2}\n", "unknown attribute `speed`"),
            ("::fade{out fade=1}\n", "unknown attribute `fade`"),
        ] {
            let e = errs(&format!("Hi.\n{src}"));
            assert_eq!(e.len(), 1, "{src}: {e:?}");
            assert_eq!(e[0].line, 2, "{src}");
            assert!(e[0].message.contains(needle), "{src}: {}", e[0].message);
        }
    }

    #[test]
    fn bg_through_is_a_fade_out_a_cut_and_a_fade_in() {
        let p = ok("::bg{src=a.png fade=1 through=#ff0000}\n::bg{src=b.png fade=2 through}\n::bg{src=c.png fade=1}\n");
        assert_eq!(p.ops[0], Op::FadeOut { time: 0.5, color: [1.0, 0.0, 0.0] });
        assert_eq!(p.ops[1], Op::Bg { src: "a.png".into(), fade: 0.0, wipe: None, sound: false });
        assert_eq!(p.ops[2], Op::FadeIn { time: 0.5 });
        // bare `through` is black, and each half is fade/2
        assert_eq!(p.ops[3], Op::FadeOut { time: 1.0, color: [0.0; 3] });
        assert_eq!(p.ops[4], Op::Bg { src: "b.png".into(), fade: 0.0, wipe: None, sound: false });
        assert_eq!(p.ops[5], Op::FadeIn { time: 1.0 });
        // without `through` it is the old crossfade, one op
        assert_eq!(p.ops[6], Op::Bg { src: "c.png".into(), fade: 1.0, wipe: None, sound: false });
        assert!(matches!(p.ops[7], Op::End) && p.ops.len() == 8);
        // every op of the sugar reports the command's line
        assert_eq!(&p.lines[..3], &[1, 1, 1]);
    }

    #[test]
    fn bg_through_errors_name_the_line() {
        for (src, needle) in [
            ("::bg{src=a.png through}\n", "needs a duration"),
            ("::bg{src=a.png through=#fff fade=0}\n", "needs a duration"),
            ("::bg{src=a.png fade=1 through=#fff wipe=left}\n", "can't be combined"),
            ("::bg{src=a.png fade=1 through=blue}\n", "bad colour"),
        ] {
            let e = errs(&format!("Hi.\n{src}"));
            assert_eq!(e.len(), 1, "{src}: {e:?}");
            assert_eq!(e[0].line, 2, "{src}");
            assert!(e[0].message.contains(needle), "{src}: {}", e[0].message);
        }
    }

    // ---- shake axis -------------------------------------------------------

    #[test]
    fn shake_takes_an_axis() {
        let p = ok("::shake{axis=x}\n::shake{time=1 strength=5 axis=y}\n::shake{axis=both}\n::shake\n");
        let axes: Vec<Axis> = p.ops.iter().filter_map(|o| if let Op::Shake { axis, .. } = o { Some(*axis) } else { None }).collect();
        assert_eq!(axes, [Axis::X, Axis::Y, Axis::Both, Axis::Both]);
        let e = errs("Hi.\n::shake{axis=z}\n");
        assert_eq!(e[0].line, 2);
        assert!(e[0].message.contains("axis=z") && e[0].message.contains("`x`, `y` or `both`"), "{}", e[0].message);
    }

    // ---- music intro ------------------------------------------------------

    #[test]
    fn music_takes_an_intro() {
        let p = ok("::music{src=loop.ogg intro=intro.ogg fade=1 volume=0.5}\n::music{src=a.ogg}\n");
        assert_eq!(p.ops[0], Op::Music { src: "loop.ogg".into(), intro: Some("intro.ogg".into()), fade: 1.0, looped: true, volume: 0.5 });
        assert!(matches!(&p.ops[1], Op::Music { intro: None, .. }));
        // the intro is an asset of the op, so check, package and validation see it
        let assets: Vec<_> = p.assets().map(|(_, k, f)| (k, f.to_string())).collect();
        assert!(assets.contains(&(AssetKind::Audio, "intro.ogg".to_string())));
        assert!(assets.contains(&(AssetKind::Audio, "loop.ogg".to_string())));
    }

    #[test]
    fn music_intro_errors_name_the_line() {
        for (src, needle) in [
            ("::music{intro=i.ogg}\n", "`intro=` needs `src=`"),
            ("::music{src=a.ogg intro=i.ogg loop=false}\n", "needs a looping track"),
            ("::music{src=a.ogg intro=a.ogg}\n", "same file"),
            ("::music{src=a.ogg intro=}\n", "needs a file"),
            ("::music{src=a.ogg intro}\n", "needs a file"),
            ("::music{src=a.ogg intro=../x.ogg}\n", "inside"),
            ("::music{stop intro=i.ogg}\n", "unknown attribute `intro`"),
        ] {
            let e = errs(&format!("Hi.\n{src}"));
            assert_eq!(e[0].line, 2, "{src}");
            assert!(e[0].message.contains(needle), "{src}: {}", e[0].message);
        }
    }

    // ---- sprite flip and per-axis scale ---------------------------------------

    #[test]
    fn show_takes_flip_xscale_and_yscale() {
        let p = ok(&format!(
            "{MIKA}::show{{mika happy flip}}\n::show{{mika flip=false xscale=1.5 yscale=0.5}}\n::show{{mika}}\n::show{{src=a.png flip=true}}\n"
        ));
        let get = |i: usize| match &p.ops[i] {
            Op::Show { flip, xscale, yscale, .. } => (*flip, *xscale, *yscale),
            o => panic!("{o:?}"),
        };
        assert_eq!(get(0), (Some(true), None, None));
        assert_eq!(get(1), (Some(false), Some(1.5), Some(0.5)));
        assert_eq!(get(2), (None, None, None), "not given: the sprite keeps its own");
        assert_eq!(get(3), (Some(true), None, None));
    }

    #[test]
    fn move_takes_flip_xscale_and_yscale() {
        let p = ok(&format!("{MIKA}::move{{mika flip}}\n::move{{mika xscale=2 yscale=0.5 time=1}}\n::move{{mika flip=false x=left}}\n"));
        assert!(matches!(&p.ops[0], Op::Move { flip: Some(true), steps, .. } if steps[0].props().iter().all(Option::is_none)));
        assert!(matches!(&p.ops[1], Op::Move { flip: None, steps, .. } if steps[0].xscale == Some(2.0) && steps[0].yscale == Some(0.5) && steps[0].time == 1.0));
        assert!(matches!(&p.ops[2], Op::Move { flip: Some(false), steps, .. } if steps[0].x.is_some()));
    }

    #[test]
    fn sprite_scale_errors_name_the_line() {
        for (src, needle) in [
            ("::show{src=a.png xscale=0}\n", "xscale=0"),
            ("::show{src=a.png yscale=11}\n", "yscale=11"),
            ("::show{src=a.png xscale=wide}\n", "xscale=wide"),
            ("::move{mika yscale=0.01}\n", "yscale=0.01"),
            ("::move{mika xscale=-1}\n", "xscale=-1"),
        ] {
            let e = errs(&format!("Hi.\n{src}"));
            assert_eq!(e[0].line, 2, "{src}");
            assert!(e[0].message.contains(needle), "{src}: {}", e[0].message);
        }
    }

    // ---- ::hud and ::bg{color} ----

    fn hud_of(src: &str) -> (HudElem, f32) {
        match ok(src).ops.into_iter().next().unwrap() {
            Op::Hud { elem, fade } => (*elem, fade),
            other => panic!("not a hud op: {other:?}"),
        }
    }

    #[test]
    fn a_hud_text_takes_every_attribute() {
        let (e, fade) = hud_of("::hud{id=clock text=\"Day {day}, {hero.gold} gold\" x=0.92 y=0.04 size=32 color=#ffffff back=#00000099 pad=12 fade=0.3 if=\"day > 1\"}\n");
        assert_eq!(fade, 0.3);
        assert_eq!((e.id.as_str(), e.x, e.y), ("clock", 0.92, 0.04));
        assert_eq!(e.cond.as_ref().unwrap().src, "day > 1");
        let HudKind::Text { text, size, color, back, pad } = e.kind else { panic!() };
        assert_eq!(text, "Day {day}, {hero.gold} gold");
        assert_eq!((size, pad), (Some(32.0), 12.0));
        assert_eq!(color, Some([1.0, 1.0, 1.0, 1.0]));
        assert!((back.unwrap()[3] - 0.6).abs() < 1e-6);
    }

    #[test]
    fn hud_defaults_and_percentages() {
        let (e, fade) = hud_of("::hud{id=a text=Hi x=50% y=10%}\n");
        assert_eq!((fade, e.x, e.y, e.cond.is_none()), (0.0, 0.5, 0.1, true));
        assert_eq!(e.kind, HudKind::Text { text: "Hi".into(), size: None, color: None, back: None, pad: 0.0 });
        let (d, _) = hud_of("::hud{id=a text=Hi}\n");
        assert_eq!((d.x, d.y), (0.0, 0.0));
        let (k, _) = hud_of("::hud{id=a text=Hi x=right y=bottom}\n");
        assert_eq!((k.x, k.y), (1.0, 1.0));
    }

    #[test]
    fn a_hud_bar_takes_expressions_a_size_and_a_border() {
        let src = "::hud{id=hp bar=hero.hp max=hero.max_hp+2 x=0.02 y=0.04 size=320,26 color=#cc3333 back=#333333 border=#ffffff border_size=3}\n";
        let (e, _) = hud_of(&format!("---\nvars: {{ hero.hp: 3, hero.max_hp: 5 }}\n---\n{src}"));
        let HudKind::Bar { value, max, size, color, back, border } = e.kind else { panic!() };
        assert_eq!((value.src.as_str(), max.src.as_str(), size), ("hero.hp", "hero.max_hp+2", [320.0, 26.0]));
        assert_eq!(color.unwrap()[0], 0xcc as f32 / 255.0);
        assert_eq!(back, [0.2, 0.2, 0.2, 1.0]);
        assert_eq!(border.unwrap().1, 3.0);
        let (e, _) = hud_of("::hud{id=hp bar=3 max=10 size=100,10 border=#fff}\n");
        let HudKind::Bar { color, border, max, .. } = e.kind else { panic!() };
        assert_eq!((color, border.unwrap().1, max.expr), (None, 2.0, Expr::Lit(Value::Int(10))));
    }

    #[test]
    fn a_hud_image_is_checked_like_a_hotspot_image() {
        let p = ok("::hud{id=heart image=assets/heart.png x=0.02 y=0.9 scale=0.5}\n");
        assert!(matches!(&p.ops[0], Op::Hud { elem, .. } if elem.image() == Some("assets/heart.png")));
        assert_eq!(p.assets().map(|(_, _, a)| a).collect::<Vec<_>>(), ["assets/heart.png"]);
        assert!(has_err("::hud{id=h image=../x.png}\n", "inside the game folder"));
        assert!(has_err("::hud{id=h image=a.txt}\n", ".png, .jpg or .webp"));
        assert!(has_err("::hud{id=h image=a.png scale=11}\n", "scale=11"));
    }

    #[test]
    fn hud_hide_and_clear() {
        assert_eq!(ok("::hud{hide=clock fade=0.3}\n").ops[0], Op::HudHide { id: "clock".into(), fade: 0.3 });
        assert_eq!(ok("::hud{hide=clock}\n").ops[0], Op::HudHide { id: "clock".into(), fade: 0.0 });
        assert_eq!(ok("::hud{clear}\n").ops[0], Op::HudClear { fade: 0.0 });
        assert_eq!(ok("::hud{clear fade=2}\n").ops[0], Op::HudClear { fade: 2.0 });
        assert!(has_err("::hud{hide}\n", "needs the id"));
        assert!(has_err("::hud{clear hide=a}\n", "not both"));
        assert!(has_err("::hud{clear id=a}\n", "can't be combined"));
    }

    #[test]
    fn a_hud_needs_exactly_one_kind_and_an_id() {
        assert!(has_err("::hud{id=a}\n", "needs one of `text=`, `bar=` or `image=`"));
        assert!(has_err("::hud{id=a text=x bar=1 max=2 size=1,1}\n", "only one of"));
        assert!(has_err("::hud{id=a text=x image=a.png}\n", "only one of"));
        assert!(has_err("::hud{text=x}\n", "needs `id=name`"));
        assert!(has_err("::hud{id=1a text=x}\n", "bad `id=1a`"));
        assert!(has_err("::hud{id=a.b text=x}\n", "bad `id=a.b`"));
    }

    #[test]
    fn hud_errors_name_the_line_and_the_problem() {
        let e = errs("one\n\n::hud{id=a text=x fade=0}\n");
        assert_eq!(e.len(), 1);
        assert_eq!(e[0].line, 3);
        for (src, needle) in [
            ("::hud{id=a text=x fade=11}", "fade=11"),
            ("::hud{id=a text=x x=2}", "bad `x=2`"),
            ("::hud{id=a text=x y=left}", "bad `y=left`"),
            ("::hud{id=a text=x size=7}", "size=7"),
            ("::hud{id=a text=x size=301}", "size=301"),
            ("::hud{id=a text=x color=red}", "bad colour"),
            ("::hud{id=a text=x back=#12}", "bad colour"),
            ("::hud{id=a text=x pad=-1}", "pad=-1"),
            ("::hud{id=a text=\"x{nw}\"}", "only work in dialogue"),
            ("::hud{id=a text=\"x{p}y\"}", "only work in dialogue"),
            ("::hud{id=a text=\"{color=#zzz}x\"}", "colour"),
            ("::hud{id=a text=x if=\"1 +\"}", "too early"),
            ("::hud{id=a text=x if=\"random(1,2)\"}", "random"),
            ("::hud{id=a bar=1 size=10,10}", "needs `max=`"),
            ("::hud{id=a bar=1 max=2}", "needs `size="),
            ("::hud{id=a bar=1 max=2 size=0,10}", "bad `size=0,10`"),
            ("::hud{id=a bar=1 max=2 size=10}", "bad `size=10`"),
            ("::hud{id=a bar=1 max=2 size=10,10,10}", "bad `size="),
            ("::hud{id=a bar=\"1 +\" max=2 size=10,10}", "`bar=`"),
            ("::hud{id=a bar=1 max=random(1,2) size=10,10}", "`max=`"),
            ("::hud{id=a bar=1 max=2 size=10,10 border=#fff border_size=4}", "border_size=4"),
            ("::hud{id=a bar=1 max=2 size=10,10 border_size=2}", "needs `border=`"),
            ("::hud{id=a text=x scale=2}", "isn't used by a `text` element"),
            ("::hud{id=a image=a.png size=3}", "isn't used by a `image` element"),
            ("::hud{id=a bar=1 max=2 size=1,1 pad=3}", "isn't used by a `bar` element"),
            ("::hud{id=a text=x colour=#fff}", "unknown attribute `colour`"),
            ("::hud{id=a text=\"\"}", "needs some text"),
        ] {
            assert!(has_err(&format!("{src}\n"), needle), "{src} should say `{needle}`: {:?}", errs(&format!("{src}\n")));
        }
    }

    #[test]
    fn a_hud_text_or_image_can_be_a_button_that_calls_a_label_or_opens_a_screen() {
        let src = "---\ninventory: bag\njournal: quests\nvars: { bag: [], quests: [] }\n---\n\
::hud{id=phone image=assets/phone.png call=#phone hover=assets/phone_hover.png}\n\
::hud{id=log text=Log open=journal}\n::hud{id=menu text=Menu open=settings}\n::hud{id=plain text=Hi}\n# start\nHi.\n::end\n# phone\nRing.\n::return\n";
        let p = ok(src);
        let hud = |i: usize| match &p.ops[i] {
            Op::Hud { elem, .. } => (**elem).clone(),
            o => panic!("{o:?}"),
        };
        assert_eq!(hud(0).action, Some(HudAction::Call("phone".into())));
        assert_eq!(hud(0).hover.as_deref(), Some("assets/phone_hover.png"));
        assert_eq!(hud(1).action, Some(HudAction::Open(HudScreen::Journal)));
        assert_eq!(hud(2).action, Some(HudAction::Open(HudScreen::Settings)));
        assert_eq!((hud(3).action, hud(3).hover), (None, None));
        assert_eq!(p.hud_calls.iter().collect::<Vec<_>>(), ["phone"]);
        // the hover picture is an asset like the element's own
        let assets: Vec<_> = p.ops[0].all_assets().into_iter().map(|(_, a)| a).collect();
        assert_eq!(assets, ["assets/phone.png", "assets/phone_hover.png"]);
    }

    #[test]
    fn hud_button_errors_name_the_line_and_the_problem() {
        let story = "# start\nHi.\n::end\n# phone\n::return\n# ask(x)\n::return\n";
        for (head, src, needle) in [
            ("", "::hud{id=a text=x call=#start open=settings}", "not both"),
            ("", "::hud{id=a text=x call=start}", "names a label"),
            ("", "::hud{id=a text=x call=#}", "names a label"),
            ("", "::hud{id=a text=x call=#nowhere}", "unknown label `nowhere`"),
            ("", "::hud{id=a text=x call=#ask}", "takes parameters"),
            ("", "::hud{id=a bar=1 max=2 size=10,10 call=#phone}", "bar can't be a button"),
            ("", "::hud{id=a text=x open=map}", "unknown `open=map`"),
            ("", "::hud{id=a text=x open=journal}", "needs a journal"),
            ("", "::hud{id=a text=x open=inventory}", "needs an inventory"),
            ("", "::hud{id=a text=x hover=a.png}", "needs `call=#label` or `open=...`"),
            ("", "::hud{id=a text=x open=settings hover=a.png}", "picture for an `image=` button"),
            ("", "::hud{id=a image=a.png open=settings hover=../a.png}", "inside the game folder"),
            ("", "::hud{id=a image=a.png open=settings hover=a.txt}", "must be a .png"),
        ] {
            let text = format!("{head}{src}\n{story}");
            assert!(has_err(&text, needle), "{src} should say `{needle}`: {:?}", errs(&text));
        }
        let e = errs(&format!("one\n\n::hud{{id=a text=x call=#nowhere}}\n{story}"));
        assert_eq!(e[0].line, 3);
    }

    #[test]
    fn a_hud_bar_over_a_text_variable_is_an_error() {
        assert!(has_err("---\nvars: { name: \"Ann\" }\n---\n::hud{id=a bar=name+1 max=3 size=10,10}\n", "name"));
    }

    #[test]
    fn hud_expressions_count_as_reads() {
        let p = ok("---\nvars: { hp: 3 }\n---\n::hud{id=a bar=hp max=seen(go) size=10,10 if=\"hp > 0\"}\n# go\nHi.\n");
        let mut names = Vec::new();
        p.ops[0].exprs().iter().for_each(|e| e.vars_read(&mut names));
        assert!(names.contains(&"hp") && names.contains(&"seen:go"), "{names:?}");
        assert!(p.counted.contains_key(&p.labels["go"]), "seen() in a HUD expression is counted");
    }

    #[test]
    fn a_solid_colour_background() {
        assert_eq!(ok("::bg{color=#102030}\n").ops[0], Op::Bg { src: "color:#102030".into(), fade: 0.0, wipe: None, sound: false });
        assert_eq!(ok("::bg{color=#fff fade=0.5}\n").ops[0], Op::Bg { src: "color:#ffffff".into(), fade: 0.5, wipe: None, sound: false });
        assert_eq!(ok("::bg{color=#000 fade=1 wipe=left}\n").ops[0], Op::Bg { src: "color:#000000".into(), fade: 1.0, wipe: Some(Wipe::Left), sound: false });
        let p = ok("::bg{color=#fff fade=1 through=#000}\n");
        assert!(matches!(p.ops[0], Op::FadeOut { .. }) && matches!(p.ops[1], Op::Bg { fade, .. } if fade == 0.0) && matches!(p.ops[2], Op::FadeIn { .. }));
        assert_eq!(p.assets().count(), 0, "a colour is no file");
        assert_eq!(bg_color("color:#102030"), Some([16.0 / 255.0, 32.0 / 255.0, 48.0 / 255.0]));
        assert_eq!(bg_color("assets/a.png"), None);
        assert_eq!(bg_color("color:#12"), None);
        assert_eq!(bg_color(&bg_color_src([1.0, 0.5, 0.0])), Some([1.0, 128.0 / 255.0, 0.0]));
    }

    #[test]
    fn a_background_is_a_file_or_a_colour() {
        assert!(has_err("::bg{src=a.png color=#fff}\n", "not both"));
        assert!(has_err("::bg{fade=1}\n", "needs `src=file`, `color=#rrggbb` or `anim=id`"));
        assert!(has_err("::bg{color=blue}\n", "bad colour"));
        assert!(has_err("::bg{color=#fff wipe=left}\n", "needs a duration"));
        assert!(has_err("::bg{color}\n", "needs a colour"));
    }

    #[test]
    fn the_hud_demo_parses() {
        let p = ok(include_str!("../../../examples/hud.quill.md"));
        assert!(p.ops.iter().any(|o| matches!(o, Op::HudClear { .. })));
        assert_eq!(p.ops.iter().filter(|o| matches!(o, Op::Hud { .. })).count(), 7);
    }

    const ANIMS: &str = "::anim{id=blink frames=\"eyes_open.png 3, eyes_closed.png 0.12\" loop}\n";

    #[test]
    fn an_animation_lists_frames_and_may_loop() {
        let p = ok(ANIMS);
        let a = &p.anims["blink"];
        assert_eq!(a.id, "blink");
        assert_eq!(a.frames, [AnimFrame { file: "eyes_open.png".into(), seconds: 3.0 }, AnimFrame { file: "eyes_closed.png".into(), seconds: 0.12 }]);
        assert!(a.looped && a.line == 1 && a.is_animated());
        assert!((a.total() - 3.12).abs() < 1e-6);
        let once = ok("::anim{id=once frames=\"a.png 1, sub/b.webp 2.5,c.jpg 60 ,d.jpeg 0.02\"}\n");
        let a = &once.anims["once"];
        assert!(!a.looped);
        assert_eq!(a.frames.len(), 4, "spaces around commas don't matter");
        assert_eq!((a.frames[1].file.as_str(), a.frames[2].seconds, a.frames[3].seconds), ("sub/b.webp", 60.0, 0.02));
        let single = ok("::anim{id=still frames=\"a.png 1\"}\n");
        assert!(!single.anims["still"].is_animated(), "one frame: allowed, and static");
        assert!(!ok("::anim{id=l frames=\"a.png 1\" loop=false}\n").anims["l"].looped);
    }

    #[test]
    fn animation_errors_name_the_line() {
        for (src, needle) in [
            ("::anim{frames=\"a.png 1\"}\n", "needs `id="),
            ("::anim{id=x}\n", "needs `frames="),
            ("::anim{id=\"a b\" frames=\"a.png 1\"}\n", "must be letters, digits"),
            ("::anim{id=x frames=\"\"}\n", "empty entry"),
            ("::anim{id=x frames=\"a.png 1,\"}\n", "empty entry"),
            ("::anim{id=x frames=\"a.png\"}\n", "bad frame `a.png`"),
            ("::anim{id=x frames=\"a.png 1 2\"}\n", "bad frame"),
            ("::anim{id=x frames=\"a.png soon\"}\n", "number of seconds from 0.02 to 60"),
            ("::anim{id=x frames=\"a.png 0.01\"}\n", "from 0.02 to 60"),
            ("::anim{id=x frames=\"a.png 60.5\"}\n", "from 0.02 to 60"),
            ("::anim{id=x frames=\"a.png -1\"}\n", "from 0.02 to 60"),
            ("::anim{id=x frames=\"a.png NaN\"}\n", "from 0.02 to 60"),
            ("::anim{id=x frames=\"a.txt 1\"}\n", "must be a .png, .jpg or .webp"),
            ("::anim{id=x frames=\"a.webm 1\"}\n", "must be a .png, .jpg or .webp"),
            ("::anim{id=x frames=\"../a.png 1\"}\n", "inside the game folder"),
            ("::anim{id=x frames=\"/a.png 1\"}\n", "inside the game folder"),
            ("::anim{id=x frames=\"a.png 1\" speed=2}\n", "unknown attribute `speed`"),
            ("::anim{id=x frames=\"a.png 1\"\n", "expected `{...}`"),
            ("::anim{id=x frames=\"a.png 1}\n", "unterminated quote"),
        ] {
            let e = errs(&format!("Hi.\n{src}"));
            assert_eq!(e.len(), 1, "{src}: {e:?}");
            assert_eq!(e[0].line, 2, "{src}");
            assert!(e[0].message.contains(needle), "{src}: {}", e[0].message);
        }
        let e = errs(&format!("{ANIMS}Hi.\n{ANIMS}"));
        assert_eq!((e.len(), e[0].line), (1, 3));
        assert!(e[0].message.contains("declared twice"));
    }

    #[test]
    fn a_picture_that_is_an_animation_is_shown_with_anim() {
        let chars = "::character{id=mika sprites=m_{pose}.png}\n";
        let p = ok(&format!("{ANIMS}{chars}::show{{mika anim=blink x=left y=0.9 z=2 scale=0.8 flip fade=0.5}}\n::show{{ghost anim=blink}}\n::show{{Mika anim=blink}}\n::bg{{anim=blink fade=1 wipe=left}}\n::bg{{anim=blink fade=1 through=#000}}\n"));
        let shows: Vec<_> = p.ops.iter().filter_map(|o| if let Op::Show { id, src, x: at, y, z, scale, flip, fade, .. } = o { Some((id.as_str(), src.as_str(), *at, *y, *z, *scale, *flip, *fade)) } else { None }).collect();
        assert_eq!(shows[0], ("mika", "anim:blink", Some(0.0), Some(0.9), Some(2), Some(0.8), Some(true), 0.5));
        assert_eq!((shows[1].0, shows[1].1), ("ghost", "anim:blink"), "any name makes a sprite");
        assert_eq!(shows[2].0, "mika", "a character's name finds the character's sprite");
        assert!(p.ops.contains(&Op::Bg { src: "anim:blink".into(), fade: 1.0, wipe: Some(Wipe::Left), sound: false }));
        assert!(p.ops.iter().any(|o| matches!(o, Op::FadeOut { .. })));
        assert_eq!(anim_id("anim:blink"), Some("blink"));
        assert_eq!((anim_id("anim:"), anim_id("blink"), anim_id("anim:a b"), anim_id("color:#fff")), (None, None, None, None));
        assert!(!p.ops.iter().any(|o| o.asset().is_some_and(|(_, a)| a.starts_with("anim:"))), "an animation is no file");
        // `id=` names the sprite, like with `src=`.
        let q = ok(&format!("{ANIMS}::show{{id=hero anim=blink}}\n"));
        assert!(matches!(&q.ops[0], Op::Show { id, src, .. } if id == "hero" && src == "anim:blink"));
    }

    #[test]
    fn anim_errors_name_the_line() {
        let chars = "::character{id=mika sprites=m_{pose}.png}\n";
        for (src, needle) in [
            ("::show{mika anim=nope}\n", "unknown animation `nope`"),
            ("::bg{anim=nope}\n", "unknown animation `nope`"),
            ("::bg{anim}\n", "unknown animation ``"),
            ("::show{mika happy anim=blink}\n", "unexpected `happy`"),
            ("::show{mika src=x.png anim=blink}\n", "not both"),
            ("::show{anim=blink}\n", "needs a sprite name"),
            ("::bg{src=a.png anim=blink}\n", "one of `src=`, `color=` or `anim=`"),
            ("::bg{color=#fff anim=blink}\n", "one of `src=`, `color=` or `anim=`"),
            ("::move{mika anim=blink x=left}\n", "unknown attribute `anim`"),
            ("::hide{mika anim=blink}\n", "unknown attribute `anim`"),
            ("::camera{anim=blink zoom=2}\n", "unknown attribute `anim`"),
            ("::show{mika src=anim:blink}\n", "inside the game folder"),
        ] {
            let e = errs(&format!("{chars}{ANIMS}Hi.\n{src}"));
            // the failing command is the first line of `src`, after the 3 lines of set-up
            assert!(e.iter().any(|e| e.line == 4 && e.message.contains(needle)), "{src}: {e:?}");
            assert!(e.iter().all(|e| e.line >= 4), "{src}: {e:?}");
        }
        let e = errs(&format!("{chars}{ANIMS}Hi.\n::show{{mika anim=nope}}\n"));
        assert_eq!(e[0].line, 4);
        let e = errs(&format!("{chars}::show{{mika anim=blink}}\n{ANIMS}"));
        assert_eq!((e.len(), e[0].line), (1, 2), "an animation is declared before it is used");
        assert!(e[0].message.contains("unknown animation `blink` (there are no animations yet; declare it first with `::anim`)"));
    }

    #[test]
    fn animation_frames_are_assets_and_loaded_ahead() {
        let p = ok(&format!("{ANIMS}::show{{x anim=blink}}\nHi.\n::bg{{src=far.png}}\n"));
        let files: Vec<_> = p.assets().map(|(_, k, a)| (k, a)).collect();
        assert_eq!(files, [(AssetKind::Image, "far.png"), (AssetKind::Image, "eyes_open.png"), (AssetKind::Image, "eyes_closed.png")]);
        assert_eq!(p.images_ahead(0, 10), ["eyes_open.png", "eyes_closed.png", "far.png"]);
        assert_eq!(p.anim_frames().collect::<Vec<_>>(), [(1, "blink", "eyes_open.png"), (1, "blink", "eyes_closed.png")]);
        assert_eq!(p.assets_with_lines().count(), 1, "the check command reads the frames separately, as pictures");
    }

    #[test]
    fn frame_selection_holds_or_loops_and_changes_exactly_at_boundaries() {
        let p = ok("::anim{id=a frames=\"a.png 1, b.png 0.5, c.png 2\"}\n::anim{id=l frames=\"a.png 1, b.png 0.5\" loop}\n");
        let (once, looped) = (&p.anims["a"], &p.anims["l"]);
        for (t, want) in [(-1.0, "a.png"), (0.0, "a.png"), (0.99, "a.png"), (1.0, "b.png"), (1.49, "b.png"), (1.5, "c.png"), (3.49, "c.png"), (3.5, "c.png"), (1000.0, "c.png")] {
            assert_eq!(once.frame_at(t), want, "one-shot at {t}");
        }
        for (t, want) in [(0.0, "a.png"), (0.99, "a.png"), (1.0, "b.png"), (1.49, "b.png"), (1.5, "a.png"), (2.49, "a.png"), (2.5, "b.png"), (30.2, "a.png")] {
            assert_eq!(looped.frame_at(t), want, "loop at {t}");
        }
        assert!(once.running(3.49) && !once.running(3.5) && looped.running(1e6));
        assert_eq!(looped.frame_at(f32::NAN), "a.png");
        let still = ok("::anim{id=s frames=\"a.png 1\" loop}\n");
        assert!(!still.anims["s"].running(0.0) && still.anims["s"].frame_at(5.0) == "a.png");
    }

    #[test]
    fn a_move_may_loop_forever() {
        let p = ok("::character{id=mika sprites=m_{pose}.png}\n::move{mika y=0.97 time=1.2 ease=inout loop}\n::move{mika scale=1.1 loop=pingpong}\n::move{mika x=left y=0.9 loop time=2}\n::move{loop mika x=right flip}\n::move{mika y=0.9}\n");
        assert_eq!(p.ops[0], Op::Move { id: "mika".into(), flip: None, steps: vec![MoveStep { x: None, y: Some(0.97), scale: None, xscale: None, yscale: None, alpha: None, dx: None, dy: None, time: 1.2, ease: Ease::InOut, wait: 0.0 }], looped: true });
        assert!(matches!(&p.ops[1], Op::Move { looped: true, steps, .. } if steps[0].scale == Some(1.1) && steps[0].time == 0.5));
        assert!(matches!(&p.ops[2], Op::Move { looped: true, steps, .. } if steps[0].x.is_some() && steps[0].y.is_some() && steps[0].time == 2.0));
        assert!(matches!(&p.ops[3], Op::Move { id, flip: Some(true), looped: true, steps } if id == "mika" && steps[0].x.is_some()), "a flip may come along");
        assert!(matches!(&p.ops[4], Op::Move { looped: false, .. }));
        let all = ok("::move{a x=left y=0.9 scale=1.2 xscale=1.1 yscale=0.9 loop}\n");
        assert!(matches!(&all.ops[0], Op::Move { looped: true, steps, .. } if steps[0].xscale.is_some() && steps[0].yscale.is_some()));
    }

    #[test]
    fn loop_errors_name_the_line() {
        for (src, needle) in [
            ("::move{a y=0.9 loop=forever}\n", "`loop=forever` must be `pingpong`"),
            ("::move{a y=0.9 loop=repeat}\n", "must be `pingpong`"),
            ("::move{a y=0.9 loop time=0}\n", "needs `time=` above 0"),
            ("::move{a loop x=left}\n::move{a y=0.9 loop time=0}\n", "needs `time=` above 0"),
            ("::move{a flip loop}\n", "needs something to animate"),
            ("::move{a loop}\n", "needs `x=`, `y=`"),
            ("::show{a src=x.png loop=pingpong}\n", "unknown attribute `loop`"),
            ("::hide{a loop}\n", "unexpected `loop`"),
        ] {
            let e = errs(&format!("Hi.\n{src}"));
            assert!(e.iter().all(|e| e.line >= 2), "{src}: {e:?}");
            assert!(e.iter().any(|e| e.message.contains(needle)), "{src}: {e:?}");
        }
        let e = errs("Hi.\n::move{a y=0.9 loop=bounce}\n");
        assert_eq!((e.len(), e[0].line), (1, 2));
        ok("::move{a y=0.9 loop time=0.02}\n");
    }

    #[test]
    fn the_animation_demo_parses() {
        let p = ok(include_str!("../../../examples/anim.quill.md"));
        assert_eq!(p.anims.len(), 3);
        assert!(p.anims["talk"].looped && !p.anims["arrive"].looped);
        assert!(p.ops.iter().any(|o| matches!(o, Op::Bg { src, .. } if src == "anim:flicker")));
        assert!(p.ops.iter().filter(|o| matches!(o, Op::Move { looped: true, .. })).count() >= 3);
    }

    // ---- picture names from variables ------------------------------------------

    const VARS: &str = "---\nvars: { place: \"rooftop\", mood: \"happy\", hero.mood: \"sad\", n: 3, flag: true, bag: [] }\n---\n";

    fn with_vars(body: &str) -> String {
        format!("{VARS}{MIKA}{body}")
    }

    /// The (line, message) of the only error in `body`, which follows the front matter (3 lines) and MIKA (1).
    fn only_error(body: &str) -> (usize, String) {
        let e = errs(&with_vars(body));
        assert_eq!(e.len(), 1, "{e:?}");
        (e[0].line - 4, e[0].message.clone())
    }

    #[test]
    fn patterns_are_accepted_in_bg_show_pose_and_inline_images() {
        let p = ok(&with_vars(
            "::bg{src=\"assets/places/{place}.png\"}\n::show{mika {mood}}\n::show{mika src=\"x/{hero.mood}.png\"}\n::show{src=\"y/{mood}.jpg\" id=a}\n![alt](items/{place}.webp)\n::show{mika a{mood}b}\n",
        ));
        let srcs: Vec<&str> = p.ops.iter().filter_map(|o| o.asset().map(|(_, s)| s)).collect();
        assert_eq!(
            srcs,
            ["assets/places/{place}.png", "art/mika_{mood}.png", "x/{hero.mood}.png", "y/{mood}.jpg", "items/{place}.webp", "art/mika_a{mood}b.png"]
        );
        // The pose goes through the sprites template like a literal one.
        assert!(matches!(&p.ops[1], Op::Show { id, src, .. } if id == "mika" && src == "art/mika_{mood}.png"));
        // `bg` accepts the unquoted form too.
        assert!(parse(&with_vars("::bg{src=a/{place}.png}\n")).is_ok());
    }

    #[test]
    fn pattern_errors_name_the_line() {
        let cases: [(&str, &str); 14] = [
            ("::bg{src=\"a/{place.png\"}", "without a closing"),
            ("::bg{src=\"a/place}.png\"}", "without a `{`"),
            ("::bg{src=\"a/{}.png\"}", "empty `{}`"),
            ("::bg{src=\"a/{ place }.png\"}", "isn't a variable name"),
            ("::bg{src=\"a/{n}.png\"}", "must be a text variable"),
            ("::bg{src=\"a/{flag}.png\"}", "must be a text variable"),
            ("::bg{src=\"a/{bag}.png\"}", "must be a text variable"),
            ("::bg{src=\"a/{place}\"}", "extension"),
            ("::bg{src=\"a/{place}.{mood}\"}", "extension"),
            ("::bg{src=\"a/{place}.webm\"}", "must end in a picture extension"),
            ("![](a/{place}.webm)", "must end in a picture extension"),
            ("::bg{src=\"../{place}.png\"}", "inside the game folder"),
            ("::bg{src=\"a\\\\{place}.png\"}", "inside the game folder"),
            ("::bg{src=\"a/{place}/..\\{mood}.png\"}", "inside the game folder"),
        ];
        for (body, needle) in cases {
            let (line, msg) = only_error(&format!("One.\n{body}\n"));
            assert_eq!(line, 2, "{body}");
            assert!(msg.contains(needle), "{body}: {msg}");
        }
        let (line, msg) = only_error("::show{mika {mood\n");
        assert_eq!(line, 1);
        assert!(msg.contains("expected `{...}`") || msg.contains("{"), "{msg}");
        let (_, msg) = only_error("::show{mika a}b}\n");
        assert!(msg.contains("without a `{`"), "{msg}");
        let (_, msg) = only_error("::show{mika {}}\n");
        assert!(msg.contains("empty `{}`"), "{msg}");
        let (_, msg) = only_error("::show{mika {n}}\n");
        assert!(msg.contains("must be a text variable"), "{msg}");
        let (_, msg) = only_error("::show{src=\"a/{place}.png\"}\n");
        assert!(msg.contains("needs a sprite name"), "{msg}");
        let (_, msg) = only_error("::show{mika happy {mood}}\n");
        assert!(msg.contains("takes a character and a pose"), "{msg}");
    }

    #[test]
    fn an_undeclared_variable_is_allowed_like_in_dialogue() {
        // A `::set` can create it, and `quillstone test` warns when nothing does.
        let p = ok("::bg{src=\"a/{later}.png\"}\nHi.\n");
        assert_eq!(p.assets().next().map(|(_, _, a)| a), Some("a/{later}.png"));
    }

    #[test]
    fn patterns_mean_nothing_with_anim_color_or_for_other_commands() {
        for body in [
            "::bg{anim=\"{place}\"}",
            "::bg{color=\"{place}\"}",
            "::bg{src=\"a/{place}.png\" anim=x}",
            "::bg{src=\"a/{place}.png\" color=#fff}",
            "::show{mika anim=\"{mood}\"}",
            "::show{mika anim=blink {mood}}",
            "::show{mika src=\"a/{mood}.png\" anim=blink}",
            "::sfx{src=\"a/{place}.ogg\"}",
            "::music{src=\"a/{place}.ogg\"}",
            "::voice{src=\"a/{place}.ogg\"}",
            "::video{src=\"a/{place}.webm\"}",
            "::hud{id=h image=\"a/{place}.png\"}",
            "::character{id=bob sprites=a/{pose}/{place}.png}",
            "::bg{src=\"a{b.png\"}",
        ] {
            let e = errs(&with_vars(&format!("::anim{{id=blink frames=\"a.png 1\" loop}}\n{body}\n")));
            assert!(!e.is_empty(), "{body} should be refused");
            assert!(e.iter().all(|e| e.line == 6), "{body}: {e:?}");
        }
        let e = errs(&with_vars("::sfx{src=\"a/{place}.ogg\"}\n"));
        assert!(e[0].message.contains("only for pictures"), "{}", e[0].message);
    }

    #[test]
    fn a_pattern_is_listed_as_an_asset_with_its_line_and_never_loaded_ahead() {
        let p = ok(&with_vars("::bg{src=\"assets/places/{place}.png\"}\n::bg{src=plain.png}\n::show{mika {mood}}\n"));
        let found: Vec<_> = p.assets_with_lines().map(|(l, _, a)| (l, a)).collect();
        assert_eq!(found, [(5, "assets/places/{place}.png"), (6, "plain.png"), (7, "art/mika_{mood}.png")]);
        assert_eq!(p.assets().count(), 3);
        assert_eq!(p.images_ahead(0, 10), ["plain.png"], "nothing to load for a name that isn't known yet");
    }

    #[test]
    fn braces_cannot_be_part_of_any_other_file_name() {
        for body in ["::sfx{src=a{b}.ogg}", "::hud{id=h image=a{b}.png}", "::item{id=k icon=i{x}.png}"] {
            assert!(parse(body).is_err(), "{body}");
        }
        assert!(parse("::character{id=bob sprites=a{x}.png}\n").is_err(), "only `{{pose}}` may appear in a template");
        assert!(parse("::character{id=bob sprites=a_{pose}.png}\n").is_ok());
    }

    #[test]
    fn the_places_demo_parses() {
        let p = ok(include_str!("../../../examples/places.quill.md"));
        let pats: Vec<&str> = p.assets().map(|(_, _, a)| a).filter(|a| crate::is_pattern(a)).collect();
        assert!(pats.contains(&"assets/places/{place}.png") && pats.contains(&"assets/mika_{mood}.png"), "{pats:?}");
    }

    // ---- scene replays ----

    const REPLAYABLE: &str = "# Start\nHi.\n# The rooftop\nUp here.\n::end_replay\n# Night walk\nOut.\n";

    #[test]
    fn the_id_names_the_save_folder_and_is_checked() {
        let p = parse("---\ntitle: T\nid: my-game2\n---\n# a\nHi.\n").unwrap();
        assert_eq!(p.id.as_deref(), Some("my-game2"));
        assert_eq!(parse("---\ntitle: T\n---\n# a\nHi.\n").unwrap().id, None);
        for bad in ["", "My-Game", "a b", "a_b", "é", "../x", "con", "com1", &"a".repeat(65)] {
            let e = parse(&format!("---\ntitle: T\nid: {bad}\n---\n# a\nHi.\n")).unwrap_err();
            assert!(e[0].line == 3 && e[0].message.contains("save folder"), "{bad}: {e:?}");
        }
    }

    #[test]
    fn replays_name_labels_with_titles_in_declaration_order() {
        let p = ok(&format!("---\nreplays: {{ night-walk: \"A walk, at night\", the-rooftop: \"The rooftop\" }}\n---\n{REPLAYABLE}"));
        let got: Vec<(&str, &str)> = p.replays.iter().map(|r| (r.label.as_str(), r.title.as_str())).collect();
        assert_eq!(got, [("night-walk", "A walk, at night"), ("the-rooftop", "The rooftop")]);
        assert_eq!(p.replay_at[&p.labels["the-rooftop"]], ["the-rooftop"]);
        assert!(p.ops.contains(&Op::EndReplay));
        // keys are slugified like `::jump` targets, and a second line adds to the list
        let q = ok(&format!("---\nreplays: {{ The_Rooftop: Roof }}\nreplays: {{ \"night walk\": 'Walk' }}\n---\n{REPLAYABLE}"));
        assert_eq!(q.replays.len(), 2);
        assert_eq!(q.replays[0].label, "the-rooftop");
        assert!(ok("Hi.\n").replays.is_empty());
    }

    #[test]
    fn bad_replays_are_errors_with_the_front_matter_line() {
        let front = |r: &str| format!("---\ntitle: T\nreplays: {r}\n---\n{REPLAYABLE}");
        for (bad, needle) in [
            ("{ nowhere: \"X\" }", "there is no label `nowhere`"),
            ("{ the-rooftop: \"\" }", "needs a title"),
            ("{ the-rooftop: }", "needs a title"),
            ("{ the-rooftop: \"A\", the-rooftop: \"B\" }", "listed twice"),
            ("{ the-rooftop: \"A\", night-walk: \"A\" }", "titles must differ"),
            ("the-rooftop", "must look like"),
            ("{ the-rooftop }", "bad replay"),
            ("{ !!!: \"A\" }", "label is empty"),
        ] {
            let e = errs(&front(bad));
            let found = e.iter().find(|e| e.message.contains(needle)).unwrap_or_else(|| panic!("{bad}: {e:?}"));
            assert_eq!(found.line, 3, "{bad}");
        }
    }

    #[test]
    fn a_replay_label_may_not_take_parameters_and_the_number_of_replays_is_not_limited() {
        let src = "---\nreplays: { heal: \"Heal\" }\n---\n# Start\n::call{to=heal amount=1}\n::end\n# heal(amount)\nHi.\n::return\n";
        let e = errs(src);
        assert!(e.iter().any(|e| e.line == 2 && e.message.contains("has parameters")), "{e:?}");

        let many = |n: usize| {
            let list: Vec<String> = (0..n).map(|i| format!("s{i}: \"Scene {i}\"")).collect();
            let labels: String = (0..n).map(|i| format!("# s{i}\nHi.\n")).collect();
            format!("---\nreplays: {{ {} }}\n---\n{labels}", list.join(", "))
        };
        assert_eq!(ok(&many(500)).replays.len(), 500);
    }

    #[test]
    fn replay_titles_are_unescaped_and_any_length() {
        let src = |title: &str| format!("---\nreplays: {{ a: {title}, b: \"Two\" }}\n---\n# a\nHi.\n# b\nHo.\n");
        let p = parse(&src("\"Q\\\"uote, \\\\ ok\"")).unwrap();
        assert_eq!(p.replays[0].title, "Q\"uote, \\ ok");
        assert_eq!(p.replays[1].title, "Two");
        // the Scenes list shrinks or cuts a long title to fit its row, so there is no limit here
        let long = "x".repeat(5000);
        assert_eq!(parse(&src(&format!("\"{long}\""))).unwrap().replays[0].title, long);
        // an escaped quote doesn't end the title, so a comma after it doesn't split it
        let p = parse(&src("\"A\\\", B\"")).unwrap();
        assert_eq!(p.replays[0].title, "A\", B");
    }

    #[test]
    fn end_replay_takes_no_attributes() {
        assert!(has_err("# a\n::end_replay{now}\n", "end_replay"));
        assert!(has_err("# a\n::end_replay{x=1}\n", "unknown attribute `x`"));
        assert_eq!(ok("# a\n::end_replay\nHi.\n").ops[0], Op::EndReplay);
        assert!(has_err("# a\n::end_repaly\n", "end_replay"), "the command list names it");
    }

    #[test]
    fn the_replay_demo_parses() {
        let p = ok(include_str!("../../../examples/replay.quill.md"));
        assert_eq!(p.replays.len(), 2);
        assert!(p.ops.iter().filter(|o| matches!(o, Op::EndReplay)).count() == 2);
    }

    const NAMED: &str = "---\nvars: { hero.name: \"A\", hname: \"B\" }\n---\n";

    #[test]
    fn a_character_name_may_hold_declared_variables() {
        let p = ok(&format!("{NAMED}::character{{id=h name=\"{{hero.name}}\" color=#112233}}\n**h:** Hi.\n"));
        assert_eq!(p.characters["h"].name, "{hero.name}");
        assert!(matches!(&p.ops[0], Op::Say { speaker: Some(s), color: Some(_), .. } if s == "{hero.name}"));
        ok(&format!("{NAMED}::character{{id=h name=\"Dr. \\{{x}} {{hname}}\"}}\n"));
    }

    #[test]
    fn a_character_name_with_an_unknown_variable_or_open_brace_is_an_error() {
        for name in ["{hero.nmae}", "{nw}", "{}", "A {hname", "{hname} {zzz}"] {
            let e = errs(&format!("{NAMED}Hi.\n::character{{id=h name=\"{name}\"}}\n"));
            assert_eq!(e.len(), 1, "{name}: {e:?}");
            assert_eq!(e[0].line, 5, "{name}: {e:?}");
        }
    }

    #[test]
    fn a_speaker_prefix_equal_to_the_name_template_finds_the_character() {
        let src = format!(
            "{NAMED}::character{{id=h name=\"{{hero.name}}\" color=#112233}}\n::character{{id=b name=\"{{hname}}\" color=#445566}}\n\
             **{{hname}}:** One.\n\n**{{HERO.name}}:** Two.\n\n**{{hero.name}}:** Three.\n\n**{{other}}:** Four.\n"
        );
        let p = ok(&src);
        let says: Vec<_> = p.ops.iter().filter_map(|o| match o {
            Op::Say { speaker, color, text } => Some((speaker.clone().unwrap(), *color, text.clone())),
            _ => None,
        }).collect();
        assert_eq!(says[0].0, "{hname}");
        assert_eq!(says[0].1, p.characters["b"].color);
        assert_eq!(says[0].2, "One.");
        assert_eq!((says[1].0.as_str(), says[1].1), ("{hero.name}", p.characters["h"].color), "case-insensitive");
        assert_eq!((says[2].0.as_str(), says[2].1), ("{hero.name}", p.characters["h"].color));
        assert_eq!((says[3].0.as_str(), says[3].1), ("{other}", None), "unknown speaker: as written, no colour");
    }
}
