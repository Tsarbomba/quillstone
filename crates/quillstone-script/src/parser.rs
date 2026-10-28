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

use crate::expr::{is_ident, is_keyword, is_plain_ident, parse_effect, parse_expr, parse_list, parse_value, split_top_level};
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

/// Allowed `y=` of a sprite's bottom edge, as a fraction of the screen height.
const SPRITE_Y: RangeInclusive<f32> = -0.5..=1.5;

/// Allowed `scale=`, `xscale=` and `yscale=` of a sprite.
const SPRITE_SCALE: RangeInclusive<f32> = 0.05..=10.0;

const COMMANDS: &str = "character, bg, show, hide, move, shake, flash, fade, camera, textbox, music, sfx, voice, wait, video, \
                        choice, hotspots, item, inventory, shop, jump, call, return, set, if, elif, else, endif, end";

/// The attributes each command takes (for error messages).
fn known_attrs(command: &str) -> &'static [&'static str] {
    match command {
        "bg" => &["src", "fade", "wipe", "through"],
        "show" => &["src", "id", "at", "y", "z", "scale", "flip", "xscale", "yscale", "fade"],
        "hide" => &["all", "fade"],
        "move" => &["at", "y", "scale", "flip", "xscale", "yscale", "time", "ease"],
        "shake" => &["time", "strength", "axis"],
        "flash" => &["time", "color"],
        "fade" => &["out", "in", "time", "color"],
        "camera" => &["zoom", "x", "y", "time", "ease", "reset"],
        "textbox" => &["hide", "show", "fade"],
        "music" => &["src", "intro", "fade", "loop", "volume", "stop"],
        "sfx" => &["src", "loop", "stop", "volume", "fade"],
        "voice" => &["src"],
        "wait" => &["time"],
        "video" => &["src", "fit", "loop", "skippable"],
        "jump" => &["to", "if"],
        _ => &[],
    }
}

#[derive(Default)]
struct Parser {
    program: Program,
    errors: Vec<ParseError>,
    fixups: Vec<Fixup>,
    blocks: Vec<IfBlock>,
    /// Labels with parameters and the line of their heading (for the fall-through check).
    param_labels: Vec<(String, usize)>,
    /// `::sfx{stop=file}` targets, checked against the files the script plays.
    sfx_stops: Vec<(String, usize)>,
    /// Ops a command adds right after its own (the fade back in of `::bg{through}`).
    trailing: Vec<Op>,
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

        let fixups = std::mem::take(&mut self.fixups);
        let user_jumps: std::collections::HashSet<usize> = fixups.iter().map(|f| f.op).collect();
        for f in fixups {
            match self.program.labels.get(&f.label).copied() {
                Some(target) => {
                    let params = self.program.params.get(&f.label).cloned();
                    let mut problems = Vec::new();
                    match &mut self.program.ops[f.op] {
                        Op::Call { target: t, args } => {
                            *t = target;
                            problems = call_problems(&f.label, params.as_deref(), args);
                        }
                        Op::Jump { target: t, .. } => {
                            *t = target;
                            problems.extend(needs_call(&f.label, params.as_deref(), "`::jump`"));
                        }
                        Op::Choice { options } => {
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
        let mut inventory: Option<(usize, String)> = None;
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
                "theme" if !unquote(val).is_empty() => match check_inside("theme", unquote(val)) {
                    Ok(()) => self.program.theme = Some(unquote(val).to_string()),
                    Err(m) => self.err(line, m),
                },
                "theme" => self.err(line, "`theme` needs a file name, like `theme: theme.txt`"),
                "vars" => self.front_vars(line, val, false),
                "persistent" => self.front_vars(line, val, true),
                "gallery" | "music_room" => self.front_list(line, key, val),
                "inventory" => match unquote(val) {
                    name if is_ident(name) => inventory = Some((line, name.to_string())),
                    _ => self.err(line, "`inventory` names a list variable, like `inventory: bag`"),
                },
                "voice" => match unquote(val).trim_end_matches(['/', '\\']) {
                    "" => self.err(line, "`voice` needs a folder, like `voice: voice`"),
                    folder => self.program.voice_folder = Some(folder.to_string()),
                },
                other => self.err(line, format!("unknown front matter key `{other}`")),
            }
        }
        // `vars:` may come after it, so the list is checked once everything is read.
        if let Some((line, name)) = inventory {
            match self.program.vars.get(&name) {
                Some(Value::List(_)) => self.program.inventory = Some(name),
                _ => self.err(
                    line,
                    format!("`inventory: {name}` must name a list declared in `vars:` (like `vars: {{ {name}: [] }}`)"),
                ),
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

    /// `gallery: [a.png, b.png]` or `music_room: [...]`; the key may repeat.
    fn front_list(&mut self, line: usize, key: &str, val: &str) {
        let Some(inner) = val.strip_prefix('[').and_then(|v| v.strip_suffix(']')) else {
            self.err(line, format!("`{key}` must look like `[assets/one.png, assets/two.png]`"));
            return;
        };
        for item in split_top_level(inner, ',').into_iter().map(|p| unquote(p.trim())).filter(|p| !p.is_empty()) {
            if let Err(m) = check_inside(key, item) {
                self.err(line, m);
                continue;
            }
            let list = if key == "gallery" { &mut self.program.gallery } else { &mut self.program.music_room };
            if !list.iter().any(|p| p == item) {
                list.push(item.to_string());
            }
        }
    }

    // ---- list variables ---------------------------------------------------

    /// `has` and `count()` need a list variable declared in the front matter, and
    /// arithmetic needs numbers: a text or a list operand that is known from the
    /// declarations is an error.
    fn check_expr(&self, e: &Expr) -> Result<(), String> {
        let mut tests = Vec::new();
        e.has_tests(&mut tests);
        for (name, _) in tests {
            match self.program.vars.get(name) {
                Some(Value::List(_)) => {}
                Some(_) => return Err(format!("`{name} has ...`: `{name}` is not a list")),
                None => {
                    let msg = format!("`{name} has ...`: `{name}` is not a list (declare it in the front matter: `vars: {{ {name}: [] }}`)");
                    return Err(msg);
                }
            }
        }
        self.check_numbers(e)
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
            Expr::Count(name) => match self.program.vars.get(name) {
                Some(Value::List(_)) => {}
                Some(_) => return Err(format!("`count({name})`: `{name}` is not a list")),
                None => {
                    return Err(format!("`count({name})`: `{name}` is not a list (declare it in the front matter: `vars: {{ {name}: [] }}`)"));
                }
            },
            _ => {}
        }
        e.children().into_iter().try_for_each(|c| self.check_numbers(c))
    }

    /// A bare list variable can't be used as a value (it would be silently ignored):
    /// `copy = inv`, `x += inv` or `::call{... a=inv}`. Use `inv has "item"` to ask about it.
    fn not_a_list_value(&self, e: &Expr, place: &str) -> Result<(), String> {
        match e {
            Expr::Var(name) if matches!(self.program.vars.get(name), Some(Value::List(_))) => Err(format!(
                "`{name}` is a list and can't be {place}: lists change only by `+=`/`-=` of an item, and \
                 `{name} has \"item\"` asks about one"
            )),
            _ => Ok(()),
        }
    }

    /// A declared list changes only by `+=` / `-=` of text.
    fn check_effect(&self, e: &Effect) -> Result<(), String> {
        self.check_expr(&e.value)?;
        self.not_a_list_value(&e.value, "the right side of an assignment")?;
        if !matches!(self.program.vars.get(&e.var), Some(Value::List(_))) {
            return Ok(());
        }
        let var = &e.var;
        match (e.op, &e.value) {
            (AssignOp::Set, _) => Err(format!("`{var}` is a list: change it with `{var} += \"item\"` or `{var} -= \"item\"`, not `=`")),
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
        if self.program.labels.insert(name.clone(), at).is_some() {
            self.err(line, format!("duplicate label `{name}`"));
        }
        if let Some(params) = params {
            self.program.params.insert(name.clone(), params);
            self.param_labels.push((name, line));
        }
    }

    /// The parameter names of a heading like `# heal(amount, who)`.
    fn parameters(&self, label: &str, inner: &str) -> Result<Vec<String>, String> {
        if inner.trim().is_empty() {
            return Err(format!("`{label}()` has no parameters: write the heading without parentheses"));
        }
        let mut names: Vec<String> = Vec::new();
        for part in inner.split(',').map(str::trim) {
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
            if names.iter().any(|n| n == part) {
                return Err(format!("parameter `{part}` is listed twice"));
            }
            if matches!(self.program.vars.get(part), Some(Value::List(_))) {
                return Err(format!("`{part}` is a list variable and can't be a parameter"));
            }
            names.push(part.to_string());
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
        if let Err(m) = check_inside("path", path) {
            self.err(line, format!("image {m}"));
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
            "if" => {
                self.if_start(line, inner);
                return i;
            }
            "elif" => {
                self.elif(line, inner);
                return i;
            }
            "else" | "endif" | "return" | "inventory" => {
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
        // When building stopped at an error, attributes after it were never read:
        // only report those that are wrong, not merely unread.
        let known = known_attrs(name);
        if let Some(extra) = attrs.leftover(if built.is_some() { &[] } else { known }) {
            let hint = if known.is_empty() { String::new() } else { format!(" (it takes: {})", known.join(", ")) };
            self.err(line, format!("unknown attribute `{extra}` for `::{name}`{hint}"));
        }
        if let Some((op, label)) = built {
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
                let through = match a.take("through") {
                    None => None,
                    Some(None) => Some([0.0; 3]),
                    Some(Some(c)) => match parse_color(&c) {
                        Ok(c) => Some(c),
                        Err(m) => {
                            self.err(line, m);
                            return None;
                        }
                    },
                };
                if let Some(color) = through {
                    if fade <= 0.0 {
                        self.err(line, "`through` needs a duration: add `fade=` (seconds; half to fade out, half to fade in)");
                        return None;
                    }
                    if wipe.is_some() {
                        self.err(line, "`through` and `wipe` can't be combined: a wipe is a transition of its own");
                        return None;
                    }
                    // Fade out for half the time, change the picture (no crossfade), fade in.
                    let half = fade / 2.0;
                    self.push(Op::FadeOut { time: half, color }, line);
                    self.trailing.push(Op::FadeIn { time: half });
                    return Some((Op::Bg { src, fade: 0.0, wipe: None }, None));
                }
                Some((Op::Bg { src, fade, wipe }, None))
            }
            "show" => self.show(line, a),
            "hide" => {
                let all = a.flag("all");
                let bare = a.take_bare();
                self.extra_words(line, "hide", &bare, if all { 0 } else { 1 })?;
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
                let flip = opt_flag(a, "flip");
                let bare = a.take_bare();
                self.extra_words(line, "move", &bare, 1)?;
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
                let y = self.number(line, a, "y", SPRITE_Y)?;
                let scale = self.number(line, a, "scale", SPRITE_SCALE)?;
                let xscale = self.number(line, a, "xscale", SPRITE_SCALE)?;
                let yscale = self.number(line, a, "yscale", SPRITE_SCALE)?;
                if at.is_none() && y.is_none() && scale.is_none() && flip.is_none() && xscale.is_none() && yscale.is_none() {
                    self.err(line, "`::move` needs `at=`, `y=`, `scale=`, `xscale=`, `yscale=` and/or `flip`");
                    return None;
                }
                let time = self.seconds(line, a, "time", 0.5)?;
                let ease = self.ease(line, a)?;
                Some((Op::Move { id, at, y, scale, flip, xscale, yscale, time, ease }, None))
            }
            "shake" => {
                let time = self.seconds(line, a, "time", 0.4)?;
                let strength = self.number(line, a, "strength", 0.0..=200.0)?.unwrap_or(20.0);
                let axis = match a.value("axis") {
                    None => Axis::default(),
                    Some(v) => match Axis::parse(&v) {
                        Some(x) => x,
                        None => {
                            self.err(line, format!("`axis={v}` must be `x`, `y` or `both`"));
                            return None;
                        }
                    },
                };
                Some((Op::Shake { time, strength, axis }, None))
            }
            "flash" => {
                let time = match a.value("time") {
                    None => 0.4,
                    Some(v) => match v.parse::<f32>() {
                        Ok(t) if t.is_finite() && t > 0.0 && t <= 10.0 => t,
                        _ => {
                            self.err(line, format!("`time={v}` must be a number of seconds above 0 and at most 10"));
                            return None;
                        }
                    },
                };
                let color = match a.value("color") {
                    None => [1.0; 3],
                    Some(c) => match parse_color(&c) {
                        Ok(c) => c,
                        Err(m) => {
                            self.err(line, m);
                            return None;
                        }
                    },
                };
                Some((Op::Flash { time, color }, None))
            }
            "fade" => {
                let (out, into) = (a.flag("out"), a.flag("in"));
                let time = match a.value("time") {
                    None => 0.5,
                    Some(v) => match v.parse::<f32>() {
                        Ok(t) if t.is_finite() && t > 0.0 && t <= 10.0 => t,
                        _ => {
                            self.err(line, format!("`time={v}` must be a number of seconds above 0 and at most 10"));
                            return None;
                        }
                    },
                };
                match (out, into) {
                    (true, true) => {
                        self.err(line, "`::fade` takes `out` or `in`, not both");
                        None
                    }
                    (false, false) => {
                        self.err(line, "`::fade` needs `out` (fade to a colour and stay covered) or `in` (uncover)");
                        None
                    }
                    (false, true) => {
                        if a.0.iter().any(|(k, _)| k == "color") {
                            self.err(line, "`::fade{in}` takes no `color`: it fades away the colour the screen is covered with");
                            return None;
                        }
                        Some((Op::FadeIn { time }, None))
                    }
                    (true, false) => {
                        let color = match a.value("color") {
                            None => [0.0; 3],
                            Some(c) => match parse_color(&c) {
                                Ok(c) => c,
                                Err(m) => {
                                    self.err(line, m);
                                    return None;
                                }
                            },
                        };
                        Some((Op::FadeOut { time, color }, None))
                    }
                }
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
            "textbox" => {
                let (hide, show) = (a.flag("hide"), a.flag("show"));
                let fade = match a.value("fade") {
                    None => 0.0,
                    Some(v) => match v.parse::<f32>() {
                        Ok(t) if t.is_finite() && t > 0.0 && t <= 10.0 => t,
                        _ => {
                            self.err(line, format!("`fade={v}` must be a number of seconds above 0 and at most 10"));
                            return None;
                        }
                    },
                };
                match (hide, show) {
                    (true, false) => Some((Op::Textbox { hidden: true, fade }, None)),
                    (false, true) => Some((Op::Textbox { hidden: false, fade }, None)),
                    (true, true) => {
                        self.err(line, "`::textbox` takes `hide` or `show`, not both");
                        None
                    }
                    (false, false) => {
                        self.err(line, "`::textbox` needs `hide` or `show` (and optionally `fade=<seconds>`)");
                        None
                    }
                }
            }
            "music" => {
                if a.flag("stop") {
                    let fade = self.fade(line, a)?;
                    Some((Op::StopMusic { fade }, None))
                } else {
                    let intro = a.take("intro");
                    if intro.is_some() && !a.0.iter().any(|(k, _)| k == "src") {
                        self.err(line, "`intro=` needs `src=`: the file that loops after the intro");
                        return None;
                    }
                    let src = self.require(line, name, a, "src")?;
                    let intro = match intro {
                        None => None,
                        Some(Some(i)) if !i.is_empty() => match check_inside("intro", &i) {
                            Ok(()) if i == src => {
                                self.err(line, "`intro` and `src` are the same file: `src` is the loop, `intro` plays once before it");
                                return None;
                            }
                            Ok(()) => Some(i),
                            Err(m) => {
                                self.err(line, m);
                                return None;
                            }
                        },
                        Some(_) => {
                            self.err(line, "`intro=` needs a file (`::music{src=loop.ogg intro=intro.ogg}`)");
                            return None;
                        }
                    };
                    let fade = self.fade(line, a)?;
                    let looped = a.flag_or("loop", true);
                    if intro.is_some() && !looped {
                        self.err(line, "`intro` needs a looping track: it plays once and then `src` loops (drop `loop=false`)");
                        return None;
                    }
                    let volume = self.volume(line, a)?;
                    Some((Op::Music { src, intro, fade, looped, volume }, None))
                }
            }
            "sfx" => {
                if let Some(target) = a.take("stop") {
                    if let Some(extra) = ["src", "loop", "volume"].into_iter().find(|k| a.0.iter().any(|(key, _)| key == k)) {
                        self.err(line, format!("`::sfx{{stop}}` stops sounds, so `{extra}` doesn't go with it"));
                        return None;
                    }
                    let fade = self.fade(line, a)?;
                    let src = match target {
                        None => None,
                        Some(v) if v.is_empty() => {
                            self.err(line, "`stop=` needs a file (`::sfx{stop=rain.ogg}`), or write just `stop` for all sounds");
                            return None;
                        }
                        Some(v) => Some(v),
                    };
                    if let Some(src) = &src {
                        self.sfx_stops.push((src.clone(), line));
                    }
                    return Some((Op::StopSfx { src, fade }, None));
                }
                let src = self.require(line, name, a, "src")?;
                let looped = a.flag("loop");
                let faded = a.0.iter().any(|(k, _)| k == "fade");
                let volume = self.volume(line, a)?;
                if looped {
                    let fade = self.fade(line, a)?;
                    Some((Op::SfxLoop { src, fade, volume }, None))
                } else if faded {
                    self.err(line, "`fade` on a sound effect needs `loop` (a one-shot sound is too short to fade) or `stop`");
                    None
                } else {
                    Some((Op::Sfx { src, volume }, None))
                }
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
                // A looping video never ends by itself, so the player must be able to skip it.
                let skippable = a.flag("skippable") || looped;
                Some((Op::Video { src, fit, looped, skippable }, None))
            }
            "jump" => {
                let to = match a.value("to").or_else(|| a.take_bare().into_iter().next()) {
                    Some(t) => t,
                    None => {
                        self.err(line, format!("`::{name}` needs a label, like `::{name}{{to=label}}`"));
                        return None;
                    }
                };
                let label = Some(slug(&to));
                let cond = match a.value("if") {
                    None => None,
                    Some(c) => match parse_expr(&c).and_then(|e| self.check_expr(&e).map(|()| e)) {
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
        let flip = opt_flag(a, "flip");
        let bare = a.take_bare();
        let src_attr = a.value("src");
        let id_attr = a.value("id");

        self.extra_words(line, "show", &bare, if src_attr.is_some() { 1 } else { 2 })?;
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

        if let Err(m) = check_inside("src", &src) {
            self.err(line, m);
            return None;
        }
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
        let y = self.number(line, a, "y", SPRITE_Y)?;
        let z = match a.value("z") {
            None => None,
            Some(v) => match v.trim().parse::<i32>() {
                Ok(z) if (-100..=100).contains(&z) => Some(z),
                _ => {
                    self.err(line, format!("`z={v}` must be a whole number from -100 to 100"));
                    return None;
                }
            },
        };
        let scale = self.number(line, a, "scale", SPRITE_SCALE)?;
        let xscale = self.number(line, a, "xscale", SPRITE_SCALE)?;
        let yscale = self.number(line, a, "yscale", SPRITE_SCALE)?;
        let fade = self.fade(line, a)?;
        Some((Op::Show { id, src, at, y, z, scale, flip, xscale, yscale, fade }, None))
    }

    /// Report bare words beyond the first `allowed` (`::show{mika happy sad}`).
    fn extra_words(&mut self, line: usize, command: &str, bare: &[String], allowed: usize) -> Option<()> {
        match bare.get(allowed) {
            None => Some(()),
            Some(extra) => {
                let what = match (command, allowed) {
                    ("show", 2) => "a character and a pose",
                    ("show", _) => "one sprite name (with `src=`)",
                    (_, 0) => "no sprite name with `all`",
                    _ => "one sprite name",
                };
                self.err(line, format!("unexpected `{extra}`: `::{command}` takes {what}"));
                None
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
        if let Some(extra) = a.leftover(&[]) {
            self.err(line, format!("unknown attribute `{extra}` for `::character`"));
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
        if let Some(Err(m)) = sprites.as_deref().map(|t| check_inside("sprites", t)) {
            self.err(line, m);
            return;
        }
        let name = name.unwrap_or_else(|| id.clone());
        self.program.characters.insert(key.clone(), Character { id: key, name, color, sprites });
    }

    /// `::item{id=key name="Brass key" icon=assets/key.png price=15 text="..."}`
    fn item(&mut self, line: usize, inner: &str) {
        let mut a = match Attrs::parse(inner) {
            Ok(a) => a,
            Err(m) => return self.err(line, m),
        };
        let (id, name, icon) = (a.value("id"), a.value("name"), a.value("icon"));
        let (price, text) = (a.value("price"), a.value("text"));
        if let Some(extra) = a.leftover(&[]) {
            let msg = format!("unknown attribute `{extra}` for `::item` (it takes: id, name, icon, price, text)");
            return self.err(line, msg);
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
        let name = name.filter(|n| !n.is_empty()).unwrap_or_else(|| id.clone());
        let item = Item { id: id.clone(), name, icon, price, text: text.unwrap_or_default(), line };
        self.program.items.insert(id, item);
    }

    /// `::shop{pay=gold into=inventory}` and its `- key{price=20, if met}` lines.
    fn shop(&mut self, lines: &[&str], start: usize, inner: &str) -> usize {
        let line = start + 1;
        let mut a = match Attrs::parse(inner) {
            Ok(a) => a,
            Err(m) => {
                self.err(line, m);
                Attrs(Vec::new())
            }
        };
        let (pay, into) = (a.value("pay"), a.value("into"));
        if let Some(extra) = a.leftover(&[]) {
            self.err(line, format!("unknown attribute `{extra}` for `::shop` (it takes: pay, into)"));
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
            Some(l) if matches!(self.program.vars.get(&l), Some(Value::List(_))) => Some(l),
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
                    self.err(j + 1, "a comment inside `::shop` must open and close on one line");
                }
            } else if !t.is_empty() {
                lines_seen += 1;
                match self.shop_entry(t) {
                    Ok(e) if stock.iter().any(|s| s.item == e.item) => {
                        self.err(j + 1, format!("`{}` is in this shop twice", e.item));
                    }
                    Ok(e) => stock.push(e),
                    Err(m) => self.err(j + 1, m),
                }
            }
            j += 1;
        }
        if !closed {
            self.err(line, "`::shop` is never closed with a line containing only `::`");
        }
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
            let inner = tail
                .strip_prefix('{')
                .and_then(|x| x.strip_suffix('}'))
                .ok_or_else(|| format!("unexpected `{tail}` after the item (settings go in `{{...}}`)"))?;
            let mut inner = inner.to_string();
            if let Some(p) = take_attr(&mut inner, "price") {
                price = Some(parse_price(&p)?);
            }
            for part in split_top_level(&inner, ',').into_iter().map(str::trim).filter(|p| !p.is_empty()) {
                let cond_text =
                    part.strip_prefix("if").filter(|r| r.is_empty() || r.starts_with(char::is_whitespace));
                match cond_text {
                    Some(c) => {
                        let e = parse_expr(c)?;
                        self.check_expr(&e)?;
                        cond = Some(e);
                    }
                    None => return Err(format!("unexpected `{part}` (a shop line takes `price=` and `if condition`)")),
                }
            }
        }
        let price = price.ok_or_else(|| {
            format!("`{key}` has no price: give the item a `price=` or write `- {key}{{price=10}}`")
        })?;
        Ok(ShopEntry { item: key.to_string(), price, cond })
    }

    fn require(&mut self, line: usize, cmd: &str, a: &mut Attrs, key: &str) -> Option<String> {
        match a.value(key) {
            Some(v) if !v.is_empty() => match check_inside(key, &v) {
                Ok(()) => Some(v),
                Err(m) => {
                    self.err(line, m);
                    None
                }
            },
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
        let line = start + 1;
        let name = if hot { "::hotspots" } else { "::choice" };
        if !inner.is_empty() {
            self.err(line, format!("`{name}` takes no attributes"));
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
            if t.starts_with("<!--") {
                if !t.ends_with("-->") {
                    self.err(j + 1, format!("a comment inside `{name}` must open and close on one line"));
                }
            } else if !t.is_empty() {
                match self.option(t, hot, j + 1) {
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
            self.err(line, format!("`{name}` is never closed with a line containing only `::`"));
        }
        if options.is_empty() {
            self.err(line, if hot { "`::hotspots` has no regions" } else { "`::choice` has no options" });
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
        if hot && tail.is_empty() {
            return Err("a hotspot needs `{rect=x,y,w,h}` after its label".into());
        }
        if !tail.is_empty() {
            let inner = tail
                .strip_prefix('{')
                .and_then(|x| x.strip_suffix('}'))
                .ok_or_else(|| format!("unexpected `{tail}` after the option (effects go in `{{...}}`)"))?;
            let mut inner = inner.to_string();
            if hot {
                let rect = take_attr(&mut inner, "rect");
                let image = take_attr(&mut inner, "image");
                area = Some(Area {
                    rect: parse_rect(&rect.ok_or("a hotspot needs `rect=x,y,w,h` (each 0 to 1 or a percentage)")?)?,
                    image: image.map(|i| check_hotspot_image(&i)).transpose()?,
                    hidden: false,
                });
            } else if ["rect", "image"].iter().any(|k| take_attr(&mut inner.clone(), k).is_some()) {
                return Err("`rect=` and `image=` only belong in a `::hotspots` block".into());
            }
            let inner = inner.as_str();
            for item in split_top_level(inner, ',').into_iter().map(str::trim).filter(|p| !p.is_empty()) {
                // `if <cond>`; a variable merely starting with "if" (`iffy+=1`) is an effect
                let cond_text = item
                    .strip_prefix("if")
                    .filter(|r| r.is_empty() || r.starts_with(char::is_whitespace));
                if let Some(c) = cond_text {
                    let e = parse_expr(c)?;
                    self.check_expr(&e)?;
                    cond = Some(e);
                } else if item == "once" {
                    once = true;
                } else if item == "hidden" {
                    if !hot {
                        return Err("`hidden` only belongs in a `::hotspots` block".into());
                    }
                    hidden = true;
                } else {
                    let e = parse_effect(item)?;
                    self.check_effect(&e)?;
                    effects.push(e);
                }
            }
        }
        if let Some(a) = area.as_mut() {
            a.hidden = hidden;
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
        let opt = ChoiceOption { text: text.to_string(), target: UNSET, effects, cond, area, line };
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
fn check_image_path(path: &str, key: &str, what: &str) -> Result<String, String> {
    check_inside(key, path)?;
    let ext = path.rsplit('.').next().unwrap_or("").to_ascii_lowercase();
    if !path.contains('.') || !["png", "jpg", "jpeg", "webp"].contains(&ext.as_str()) {
        return Err(format!("bad `{key}={path}` ({what} must be a .png, .jpg or .webp file)"));
    }
    Ok(path.to_string())
}

/// A file named by the script must be relative and inside the game folder, or a save
/// that mentions it can't be loaded again (and the game can't be packaged).
fn check_inside(key: &str, path: &str) -> Result<(), String> {
    if is_inside(path) {
        Ok(())
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
fn call_problems(label: &str, params: Option<&[String]>, args: &[(String, Expr)]) -> Vec<String> {
    let Some(params) = params else {
        return args
            .iter()
            .map(|(k, _)| format!("`{label}` takes no parameters, but this `::call` passes `{k}`"))
            .collect();
    };
    let takes = format!("{label} takes: {}", params.join(", "));
    let mut out = Vec::new();
    for (k, _) in args.iter().filter(|(k, _)| !params.contains(k)) {
        out.push(format!("`{label}` has no parameter `{k}` ({takes})"));
    }
    for p in params.iter().filter(|p| !args.iter().any(|(k, _)| k == *p)) {
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
        assert_eq!(p.ops[1], Op::Move { id: "mika".into(), at: None, y: None, scale: Some(1.2), flip: None, xscale: None, yscale: None, time: 0.5, ease: Ease::Out });
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
            ("::move{mika}\n", "`xscale=`, `yscale=` and/or `flip`"),
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
        assert_eq!(p.ops[0], Op::Music { src: "a.ogg".into(), intro: None, fade: 0.0, looped: true, volume: 1.0 });
        assert_eq!(p.ops[1], Op::Music { src: "b.ogg".into(), intro: None, fade: 1.0, looped: false, volume: 0.4 });
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
            Op::Show { id: "mika".into(), src: "art/mika_happy.png".into(), at: Some(0.28), y: None, z: None, scale: Some(0.9), flip: None, xscale: None, yscale: None, fade: 0.4 }
        );
        // default pose, keeps position
        assert_eq!(
            p.ops[1],
            Op::Show { id: "mika".into(), src: "art/mika_default.png".into(), at: None, y: None, z: None, scale: None, flip: None, xscale: None, yscale: None, fade: 0.0 }
        );
    }

    #[test]
    fn show_with_an_explicit_file_and_numeric_positions() {
        let p = ok("::show{src=art/ghost.png id=ghost at=0.3}\n::show{src=art/x.png at=25%}\n");
        assert_eq!(
            p.ops[0],
            Op::Show { id: "ghost".into(), src: "art/ghost.png".into(), at: Some(0.3), y: None, z: None, scale: None, flip: None, xscale: None, yscale: None, fade: 0.0 }
        );
        assert!(matches!(&p.ops[1], Op::Show { id, at: Some(a), .. } if id == "art/x.png" && (a - 0.25).abs() < 1e-6));
    }

    #[test]
    fn sprite_y_and_z() {
        let p = ok(&format!(
            "{MIKA}::show{{mika happy y=0.8 z=3}}\n::show{{mika y=90%  z=-100}}\n::show{{mika y=-0.5 z=100}}\n::show{{mika y=150%}}\n::move{{mika y=0.9 time=0.3 ease=out}}\n::move{{mika at=left y=1.5}}\n"
        ));
        let show = |i: usize| match &p.ops[i] {
            Op::Show { y, z, .. } => (*y, *z),
            other => panic!("{other:?}"),
        };
        assert_eq!(show(0), (Some(0.8), Some(3)));
        assert_eq!(show(1), (Some(0.9), Some(-100)));
        assert_eq!(show(2), (Some(-0.5), Some(100)));
        assert_eq!(show(3), (Some(1.5), None));
        assert_eq!(p.ops[4], Op::Move { id: "mika".into(), at: None, y: Some(0.9), scale: None, flip: None, xscale: None, yscale: None, time: 0.3, ease: Ease::Out });
        assert!(matches!(&p.ops[5], Op::Move { at: Some(_), y: Some(y), .. } if *y == 1.5));
        // not given: `None`, so the sprite keeps its own
        let q = ok(&format!("{MIKA}::show{{mika}}\n"));
        assert!(matches!(&q.ops[0], Op::Show { y: None, z: None, .. }));
    }

    #[test]
    fn sprite_y_and_z_errors_name_the_line() {
        for (src, needle) in [
            ("::show{src=a.png y=2}\n", "y=2"),
            ("::show{src=a.png y=-0.6}\n", "y=-0.6"),
            ("::show{src=a.png y=low}\n", "y=low"),
            ("::show{src=a.png z=101}\n", "`z=101` must be a whole number from -100 to 100"),
            ("::show{src=a.png z=-101}\n", "z=-101"),
            ("::show{src=a.png z=1.5}\n", "z=1.5"),
            ("::show{src=a.png z=front}\n", "z=front"),
            ("::move{mika y=1.6}\n", "y=1.6"),
            ("::move{mika y=1 z=1}\n", "`z`"),
        ] {
            let e = errs(&format!("\n{src}"));
            assert_eq!(e[0].line, 2, "{src}");
            assert!(e[0].message.contains(needle), "{src}: {}", e[0].message);
        }
    }

    #[test]
    fn hide_and_move() {
        let p = ok(&format!("{MIKA}::hide{{mika fade=0.5}}\n::hide{{all}}\n::move{{mika at=right time=1}}\n::move{{mika at=0.1}}\n"));
        assert_eq!(p.ops[0], Op::Hide { id: Some("mika".into()), fade: 0.5 });
        assert_eq!(p.ops[1], Op::Hide { id: None, fade: 0.0 });
        assert_eq!(p.ops[2], Op::Move { id: "mika".into(), at: Some(0.72), y: None, scale: None, flip: None, xscale: None, yscale: None, time: 1.0, ease: Ease::InOut });
        assert_eq!(p.ops[3], Op::Move { id: "mika".into(), at: Some(0.1), y: None, scale: None, flip: None, xscale: None, yscale: None, time: 0.5, ease: Ease::InOut });
    }

    #[test]
    fn sprite_errors() {
        assert!(has_err("::show{nobody}\n", "unknown character `nobody`"));
        assert!(has_err("::character{id=a}\n::show{a}\n", "no `sprites="));
        assert!(has_err("::show\n", "needs a character"));
        assert!(has_err("::show{src=a.png at=sideways}\n", "bad position"));
        assert!(has_err("::show{src=a.png scale=0}\n", "from 0.05 to 10"));
        assert!(has_err("::show{src=a.png scale=100000}\n", "from 0.05 to 10"));
        assert!(has_err("::hide\n", "needs a sprite"));
        assert!(has_err("::move{a}\n", "needs `at="));
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
        let Op::Choice { options } = &p.ops[0] else { panic!() };
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
        assert!(has_err("::move{a b at=left}\n", "unexpected `b`"));
        ok(&format!("{chars}::show{{mika happy}}\n::show{{a src=x.png}}\n::hide{{mika}}\n"));
    }

    #[test]
    fn one_mistake_gives_one_error() {
        let e = errs("::bg{src=a.png fade=abc wipe=left}\n");
        assert_eq!(e.len(), 1, "the unread `wipe` isn't reported: {e:?}");
        let e = errs("::bg{src=a.png fade=abc wipee=left}\n");
        assert!(e.iter().any(|e| e.message.contains("unknown attribute `wipee`")), "a real typo still is");
        assert!(has_err("::bg{src=a.png wipee=left}\n", "(it takes: src, fade, wipe, through)"));
        assert!(has_err("::frobnicate\n", "shake, flash, fade, camera"));
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
    fn hotspots_take_a_rect_an_image_and_the_usual_option_extras() {
        let p = ok("# map\n::hotspots\n- [Library](#map){rect=0.1,0.2,0.2,0.3}\n- [Old key](#map){rect=60%,70%,5%,8% image=assets/key.png, once, found=true, if not found}\n::\n");
        let Op::Choice { options } = &p.ops[0] else { panic!("not a choice") };
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
    fn the_staging_demo_uses_y_z_size_and_nw() {
        let p = ok(include_str!("../../../examples/staging.quill.md"));
        assert!(p.ops.iter().any(|op| matches!(op, Op::Move { y: Some(_), .. })));
        assert!(p.ops.iter().any(|op| matches!(op, Op::Show { z: Some(_), .. })));
        let says = |f: &str| p.ops.iter().any(|op| matches!(op, Op::Say { text, .. } if text.contains(f)));
        assert!(says("{size=+14}") && says("{size=24}") && says("{nw}") && says("{p}"));
        assert!(p.ops.iter().any(|op| matches!(op, Op::Flash { .. })));
        assert!(p.ops.iter().any(|op| matches!(op, Op::FadeOut { .. })) && p.ops.iter().any(|op| matches!(op, Op::FadeIn { .. })));
        assert!(p.ops.iter().any(|op| matches!(op, Op::Shake { axis: Axis::Y, .. })));
        assert!(p.ops.iter().any(|op| matches!(op, Op::Show { flip: Some(true), .. })));
        assert!(p.ops.iter().any(|op| matches!(op, Op::Move { xscale: Some(_), yscale: Some(_), .. })));
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
        assert_eq!(p.items.len(), 4);
        assert_eq!(p.inventory.as_deref(), Some("pack"));
        assert!(p.ops.iter().any(|op| matches!(op, Op::Shop { stock, .. } if stock.len() == 4 && stock[3].cond.is_some() && stock[3].price == 18)));
        assert!(p.ops.contains(&Op::Inventory));
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
        let Op::Choice { options } = &p.ops[0] else { panic!() };
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
        e("# a\n::hotspots{x}\n- [x](#a){rect=0,0,1,1}\n::\n", "takes no attributes");
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
        assert_eq!(parse_position("center").unwrap(), 0.5);
        assert_eq!(parse_position("50%").unwrap(), 0.5);
        assert!(parse_position("150%").is_err() && parse_position("-1").is_err());
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
        let options = p.ops.iter().find_map(|op| match op { Op::Choice { options } => Some(options), _ => None }).unwrap();
        assert_eq!(options[0].effects[0].op, AssignOp::Add);
        let has = |s: &str| Expr::Has("inv".into(), Box::new(Expr::Lit(Value::Str(s.into()))));
        assert_eq!(options[0].cond, Some(Expr::Not(Box::new(has("gem")))));
        assert!(p.ops.iter().any(|op| matches!(op, Op::Jump { cond: Some(c), .. } if *c == has("key"))));
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
        assert_eq!(line("Hi.\n::set{inv = \"x\"}\n", "not `=`"), 5);
        assert_eq!(line("Hi.\n::set{inv += 3}\n", "non-empty text"), 5);
        assert_eq!(line("::set{inv -= true}\n", "non-empty text"), 4);
        assert_eq!(line("::set{inv += \"\"}\n", "non-empty text"), 4);
        assert_eq!(line("# a\n::choice\n- [x](#a){inv += \"k\"}\n- [y](#a){inv = 1}\n::\n", "not `=`"), 7);
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
        let options = p.ops.iter().find_map(|op| match op { Op::Choice { options } => Some(options), _ => None }).unwrap();
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
        assert_eq!(p.ops[1], Op::Bg { src: "a.png".into(), fade: 0.0, wipe: None });
        assert_eq!(p.ops[2], Op::FadeIn { time: 0.5 });
        // bare `through` is black, and each half is fade/2
        assert_eq!(p.ops[3], Op::FadeOut { time: 1.0, color: [0.0; 3] });
        assert_eq!(p.ops[4], Op::Bg { src: "b.png".into(), fade: 0.0, wipe: None });
        assert_eq!(p.ops[5], Op::FadeIn { time: 1.0 });
        // without `through` it is the old crossfade, one op
        assert_eq!(p.ops[6], Op::Bg { src: "c.png".into(), fade: 1.0, wipe: None });
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
        let p = ok(&format!("{MIKA}::move{{mika flip}}\n::move{{mika xscale=2 yscale=0.5 time=1}}\n::move{{mika flip=false at=left}}\n"));
        assert!(matches!(&p.ops[0], Op::Move { flip: Some(true), at: None, xscale: None, yscale: None, .. }));
        assert!(matches!(&p.ops[1], Op::Move { flip: None, xscale: Some(x), yscale: Some(y), time, .. } if *x == 2.0 && *y == 0.5 && *time == 1.0));
        assert!(matches!(&p.ops[2], Op::Move { flip: Some(false), at: Some(_), .. }));
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
}
