//! Save files: a [`Session`] (the scene plus recent rollback history) as text.
//!
//! The format is line-based: one record per line, words and `"quoted strings"`.
//! Positions in the script are stored as *label + offset* rather than raw op
//! indices, so a save still lands in the right place after the script is
//! edited above that label. If the label was renamed or removed, the save finds
//! its line by its words, or else uses the raw position. If the spot no longer
//! matches what the save was waiting for, the story re-runs from there instead
//! of getting stuck.

use std::collections::HashSet;

use quillstone_script::{bg_color, is_inside, Fit, HudAnchor, HudElem, HudExpr, HudKind, Op, Program, Value, Vars};

use quillstone_script::INPUT_MAX_LIMIT;
use quillstone_script::{Ease, ANIM_PREFIX};
use crate::{filter_typed, rebuild_pending, step, Camera, Cmd, Input, Line, MusicState, Session, SfxLoopState, SpriteLoop, SpriteState, State};

/// The longest leg of a looping move a save may hold (the script's own limit is far lower in practice).
const MAX_LOOP_LEG: f32 = 3600.0;

/// How many rollback steps a save keeps (so rollback and the backlog work after loading).
pub const SAVE_HISTORY: usize = 100;

/// Most items a list variable may hold when read from a save or the persistent
/// file. A list past it makes the record unreadable (a save is refused, a persistent
/// entry is skipped): real games hold dozens, and the limit keeps a hostile file
/// from costing memory or time.
pub const MAX_LIST_ITEMS: usize = 10_000;

/// Most states (history plus the present) a save may hold; [`SAVE_HISTORY`] is what
/// one writes. A save with more is refused.
const MAX_SAVE_STATES: usize = 1_000;

/// Most return addresses (`stack` records) in one state: the runtime's call depth limit.
const MAX_STACK_RECORDS: usize = crate::MAX_CALL_DEPTH;

/// Most `hud` records in one state (a save with more is refused), and the longest
/// text or expression one may hold.
const MAX_HUD_RECORDS: usize = 256;
const MAX_HUD_TEXT: usize = 2_000;

const MAGIC: &str = "quillstone-session 1";

/// Longest `pending wait` read from a save, in seconds (the language has no limit,
/// but a wait this long is a hand-edited save, and the player can skip it anyway).
const MAX_WAIT: f32 = 3600.0;

/// A number read from a save, made safe: not finite gives `default`, else it is
/// clamped to `lo..=hi` (the range the script language itself allows).
fn sane(value: f32, lo: f32, hi: f32, default: f32) -> f32 {
    if value.is_finite() {
        value.clamp(lo, hi)
    } else {
        default
    }
}

impl Session<'_> {
    /// The session as save-file text.
    pub fn save(&self) -> String {
        let mut out = format!("{MAGIC}\n");
        let start = self.history.len().saturating_sub(SAVE_HISTORY);
        for (state, input) in &self.history[start..] {
            write_state(&mut out, state, self.program);
            match input {
                Input::Advance => out.push_str("input advance\n"),
                Input::Choose(i) => out.push_str(&format!("input choose {i}\n")),
                Input::Buy(i) => out.push_str(&format!("input buy {i}\n")),
                Input::Text(t) => out.push_str(&format!("input text {}\n", q(t))),
            }
        }
        write_state(&mut out, &self.state, self.program);
        out
    }
}

impl<'p> Session<'p> {
    /// Restore a session from [`Session::save`] text. Returns the session and the
    /// commands that draw its screen (as after a rollback).
    pub fn load(program: &'p Program, text: &str) -> Result<(Session<'p>, Vec<Cmd>), String> {
        Session::load_impl(program, text, false)
    }

    /// [`Session::load`] for a script that was edited under a running game (hot reload): a
    /// picture that is an animation the script no longer declares is dropped (a background
    /// goes, a sprite leaves the stage) instead of making the whole load fail.
    pub fn load_edited(program: &'p Program, text: &str) -> Result<(Session<'p>, Vec<Cmd>), String> {
        Session::load_impl(program, text, true)
    }

    fn load_impl(program: &'p Program, text: &str, edited: bool) -> Result<(Session<'p>, Vec<Cmd>), String> {
        let mut lines = text.lines().enumerate().map(|(n, l)| (n + 1, l)).filter(|(_, l)| !l.trim().is_empty());
        match lines.next() {
            Some((_, l)) if l.trim() == MAGIC => {}
            _ => return Err("not a Quillstone save (or from a newer version)".into()),
        }
        let mut states: Vec<(State, Option<Input>)> = Vec::new();
        while let Some((n, line)) = lines.next() {
            let words = tokens(line).map_err(|e| format!("line {n}: {e}"))?;
            match words.first().map(String::as_str) {
                Some("state") => {
                    if states.len() >= MAX_SAVE_STATES {
                        return Err(format!("line {n}: the save holds more than {MAX_SAVE_STATES} states"));
                    }
                    states.push((read_state(&mut lines, program, edited)?, None));
                }
                Some("input") => {
                    let last = states.last_mut().ok_or(format!("line {n}: input before any state"))?;
                    last.1 = Some(match (words.get(1).map(String::as_str), words.get(2)) {
                        (Some("advance"), _) => Input::Advance,
                        (Some("choose"), Some(i)) => Input::Choose(num(i, n)?),
                        (Some("buy"), Some(i)) => Input::Buy(num(i, n)?),
                        // Only kept so the history lines up: the state before it is what rollback uses.
                        (Some("text"), Some(t)) => Input::Text(filter_typed(t).chars().take(INPUT_MAX_LIMIT).collect()),
                        _ => return Err(format!("line {n}: bad input record")),
                    });
                }
                _ => return Err(format!("line {n}: unexpected `{line}`")),
            }
        }
        let (mut state, _) = states.pop().ok_or("the save has no state")?;
        let mut history = Vec::with_capacity(states.len());
        for (s, input) in states {
            history.push((s, input.ok_or("a history entry is missing its input")?));
        }

        // If the script changed so much that a saved spot is no longer what the
        // save was waiting at, the old history can't be trusted: keep only the
        // present, re-run from its position.
        let mut cmds = None;
        if !consistent(&state, program) || history.iter().any(|(s, _)| !consistent(s, program)) {
            history.clear();
            if !consistent(&state, program) {
                // Keep the scene, drop what belonged to the line being waited on.
                state.pending = None;
                state.line = None;
                state.finished = false;
                state.inline.clear();
                state.voice = None;
                let mut redraw = state.redraw();
                redraw.extend(step(&mut state, program, None));
                cmds = Some(redraw);
            }
        }
        let cmds = cmds.unwrap_or_else(|| state.redraw());
        let session = Session { program, state, history, redo: Vec::new(), redo_tip: None, persistent: Vars::new(), persistent_changed: false };
        Ok((session, cmds))
    }
}

/// The auto-voice file name (without folder or extension) for the line at `pc`:
/// its label and a short form of its [`line_id`], like `rooftop-1a2b3c4d`, so a
/// recording stays with its line when other lines are edited. Lines before the
/// first label use `start`.
pub fn voice_name(program: &Program, pc: usize) -> Option<String> {
    let id = line_id(program, pc)?;
    let (label, _) = anchor(program, pc);
    let label = if label.is_empty() { "start" } else { label };
    Some(format!("{label}-{:08x}", id as u32))
}

/// Whether the op at the state's position is the one it is waiting at (and, at a
/// choice or shop, that something is on offer, or nothing could move the story on).
fn consistent(state: &State, program: &Program) -> bool {
    let op = program.ops.get(state.pc);
    if crate::waits_on_nothing(state, program) {
        return false;
    }
    match &state.pending {
        None => state.finished,
        Some(Cmd::Say { .. }) => matches!(op, Some(Op::Say { .. })),
        Some(Cmd::Wait { .. }) => matches!(op, Some(Op::Wait { .. } | Op::FadeOut { .. })),
        Some(Cmd::PlayVideo { .. }) => matches!(op, Some(Op::Video { .. })),
        Some(Cmd::ShowChoices { .. } | Cmd::ShowHotspots { .. }) => matches!(op, Some(Op::Choice { .. })),
        Some(Cmd::ShowInventory) => matches!(op, Some(Op::Inventory)),
        Some(Cmd::ShowShop { .. }) => matches!(op, Some(Op::Shop { .. })),
        Some(Cmd::AskInput { .. }) => matches!(op, Some(Op::Input { .. })),
        Some(_) => false,
    }
}

// ---- positions as label + offset ---------------------------------------------------

/// A stable identity for the line at `pc` (`None` if it isn't a line): a hash
/// of its label, speaker and script text. Editing elsewhere in the script, or
/// moving the line within its label, doesn't change it; editing the line does.
/// Used to remember which lines the player has read (for skip mode).
pub fn line_id(program: &Program, pc: usize) -> Option<u64> {
    let Some(Op::Say { speaker, text, .. }) = program.ops.get(pc) else { return None };
    let (label, _) = anchor(program, pc);
    Some(fnv(&[label, "\0", speaker.as_deref().unwrap_or(""), "\0", text]))
}

/// Like [`line_id`] but without the label: finds a line again after its label was renamed.
fn words_id(program: &Program, pc: usize) -> Option<u64> {
    let Some(Op::Say { speaker, text, .. }) = program.ops.get(pc) else { return None };
    Some(fnv(&[speaker.as_deref().unwrap_or(""), "\0", text]))
}

/// FNV-1a: tiny, and stable across Rust versions (unlike `DefaultHasher`).
fn fnv(parts: &[&str]) -> u64 {
    let mut hash: u64 = 0xcbf29ce484222325;
    for byte in parts.iter().flat_map(|p| p.bytes()) {
        hash ^= byte as u64;
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}

/// The label at or before `pc` (`""` = the start of the script) and the distance from it.
fn anchor(program: &Program, pc: usize) -> (&str, usize) {
    program
        .labels
        .iter()
        .filter(|(_, &at)| at <= pc)
        .max_by_key(|(_, &at)| at)
        .map(|(name, &at)| (name.as_str(), pc - at))
        .unwrap_or(("", pc))
}

/// A saved position: `"label" offset [pc [words]]`. The raw `pc` and the line's
/// [`words_id`] are fallbacks for when the label is gone (older saves lack them).
fn resolve(program: &Program, args: &[String], line: usize) -> Result<usize, String> {
    let arg = |i: usize| args.get(i).ok_or(format!("line {line}: a position needs a label and an offset"));
    let label: &str = arg(0)?;
    let offset: usize = num(arg(1)?, line)?;
    let base = if label.is_empty() { Some(0) } else { program.labels.get(label).copied() };
    if let Some(base) = base {
        return Ok(base.saturating_add(offset).min(program.ops.len()));
    }
    // The label was renamed or removed: look for the line itself, then fall back
    // to where it was (right if nothing above it changed).
    let words = args.get(3).and_then(|w| u64::from_str_radix(w, 16).ok());
    let mut matches = (0..program.ops.len()).filter(|&pc| words.is_some() && words_id(program, pc) == words);
    if let (Some(pc), None) = (matches.next(), matches.next()) {
        return Ok(pc);
    }
    match args.get(2) {
        Some(pc) => Ok(num::<usize>(pc, line)?.min(program.ops.len())),
        None => Err(format!("the script no longer has the label `{label}`")),
    }
}

fn position(program: &Program, pc: usize) -> String {
    let (label, offset) = anchor(program, pc);
    match words_id(program, pc) {
        Some(words) => format!("{} {offset} {pc} {words:016x}", q(label)),
        None => format!("{} {offset} {pc}", q(label)),
    }
}

// ---- writing ---------------------------------------------------------------------------

/// `var "name" int 3` (also used by the persistent data file).
pub(crate) fn var_record(name: &str, value: &Value) -> String {
    match value {
        Value::Int(n) => format!("var {} int {n}", q(name)),
        Value::Bool(b) => format!("var {} bool {b}", q(name)),
        Value::Str(t) => format!("var {} text {}", q(name), q(t)),
        // `var "inventory" list "map" "magic key"`: one quoted word per item, in order
        Value::List(items) => {
            let words: String = items.iter().map(|i| format!(" {}", q(i))).collect();
            format!("var {} list{words}", q(name))
        }
    }
}

/// A `"quoted string"` with backslash escapes, as [`tokens`] reads it back.
pub fn q(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

fn color(c: Option<[f32; 3]>) -> String {
    match c {
        Some([r, g, b]) => format!("{r},{g},{b}"),
        None => "-".into(),
    }
}

fn rgba(c: Option<[f32; 4]>) -> String {
    match c {
        Some([r, g, b, a]) => format!("{r},{g},{b},{a}"),
        None => "-".into(),
    }
}

/// `hud <kind> "id" x y anchor "if" ...kind fields`: an empty `if` is no condition, `-` is "not set".
fn hud_record(h: &HudElem) -> String {
    let head = format!(
        "hud {} {} {} {} {} {}",
        match &h.kind {
            HudKind::Text { .. } => "text",
            HudKind::Bar { .. } => "bar",
            HudKind::Image { .. } => "image",
        },
        q(&h.id),
        h.at[0],
        h.at[1],
        h.anchor.name(),
        q(h.cond.as_ref().map_or("", |c| c.src.as_str()))
    );
    match &h.kind {
        HudKind::Text { text, size, color, back, pad } => {
            format!("{head} {} {} {} {} {pad}", q(text), size.map_or("-".into(), |s| s.to_string()), rgba(*color), rgba(*back))
        }
        HudKind::Bar { value, max, size, color, back, border } => {
            let (bc, bw) = border.map_or(("-".to_string(), 0.0), |(c, w)| (rgba(Some(c)), w));
            format!("{head} {} {} {} {} {} {} {bc} {bw}", q(&value.src), q(&max.src), size[0], size[1], rgba(*color), rgba(Some(*back)))
        }
        HudKind::Image { src, scale } => format!("{head} {} {scale}", q(src)),
    }
}

fn parse_rgba(s: &str, n: usize) -> Result<[f32; 4], String> {
    let parts: Vec<f32> = s.split(',').map(|p| num(p, n)).collect::<Result<_, _>>()?;
    let [r, g, b, a] = parts[..] else { return Err(format!("line {n}: bad colour `{s}`")) };
    Ok([r, g, b, a].map(|c| sane(c, 0.0, 1.0, 0.0)))
}

fn read_hud(t: &[String], n: usize) -> Result<HudElem, String> {
    let arg = |i: usize| t.get(i).map(String::as_str).ok_or(format!("line {n}: a `hud` record is too short"));
    let f = |i: usize| -> Result<f32, String> { num(arg(i)?, n) };
    let opt_rgba = |i: usize| -> Result<Option<[f32; 4]>, String> {
        match arg(i)? {
            "-" => Ok(None),
            c => parse_rgba(c, n).map(Some),
        }
    };
    let expr = |i: usize| -> Result<HudExpr, String> {
        let src = arg(i)?;
        if src.len() > MAX_HUD_TEXT {
            return Err(format!("line {n}: a `hud` expression is too long"));
        }
        HudExpr::parse(src).map_err(|e| format!("line {n}: bad `hud` expression `{src}`: {e}"))
    };
    let id = arg(2)?.to_string();
    if !quillstone_script::is_plain_ident(&id) {
        return Err(format!("line {n}: bad `hud` id `{id}`"));
    }
    let at = [sane(f(3)?, 0.0, 1.0, 0.0), sane(f(4)?, 0.0, 1.0, 0.0)];
    let anchor = HudAnchor::parse(arg(5)?).ok_or(format!("line {n}: bad `hud` anchor `{}`", arg(5)?))?;
    let cond = match arg(6)? {
        "" => None,
        _ => Some(expr(6)?),
    };
    let kind = match arg(1)? {
        "text" => {
            let text = arg(7)?.to_string();
            if text.len() > MAX_HUD_TEXT {
                return Err(format!("line {n}: a `hud` text is too long"));
            }
            let size = match arg(8)? {
                "-" => None,
                _ => Some(sane(f(8)?, 8.0, 300.0, 32.0)),
            };
            HudKind::Text { text, size, color: opt_rgba(9)?, back: opt_rgba(10)?, pad: sane(f(11)?, 0.0, 200.0, 0.0) }
        }
        "bar" => HudKind::Bar {
            value: expr(7)?,
            max: expr(8)?,
            size: [sane(f(9)?, 1.0, 1920.0, 100.0), sane(f(10)?, 1.0, 1080.0, 20.0)],
            color: opt_rgba(11)?,
            back: opt_rgba(12)?.unwrap_or([0.2, 0.2, 0.2, 1.0]),
            border: opt_rgba(13)?.map(|c| (c, sane(f(14).unwrap_or(2.0), 1.0, 3.0, 2.0))),
        },
        "image" => {
            let src = arg(7)?;
            if !is_inside(src) {
                return Err(format!("line {n}: unsafe file path `{src}`"));
            }
            HudKind::Image { src: src.to_string(), scale: sane(f(8)?, 0.05, 10.0, 1.0) }
        }
        other => return Err(format!("line {n}: unknown `hud` kind `{other}`")),
    };
    Ok(HudElem { id, at, anchor, cond, kind })
}

fn write_state(out: &mut String, s: &State, program: &Program) {
    let mut w = |line: String| {
        out.push_str(&line);
        out.push('\n');
    };
    w("state".into());
    w(format!("at {}", position(program, s.pc)));
    for &ret in &s.stack {
        w(format!("stack {}", position(program, ret)));
    }
    for (name, value) in &s.vars {
        w(var_record(name, value));
    }
    // Saves from before `random` existed have no such record: they load with the default seed.
    w(format!("rng {}", s.rng));
    if let Some(bg) = &s.background {
        w(format!("bg {}", q(bg)));
    }
    for sp in &s.sprites {
        // New fields are appended: `y` and `z` came after the first four, then `flip`,
        // `xscale` and `yscale`; an older save has fewer and loads with the defaults.
        w(format!(
            "sprite {} {} {} {} {} {} {} {} {}",
            q(&sp.id),
            q(&sp.src),
            sp.x,
            sp.scale,
            sp.y,
            sp.z,
            sp.flip,
            sp.xscale,
            sp.yscale
        ));
        // A looping move is a record of its own, so older saves and readers are unaffected.
        if let Some(l) = &sp.looped {
            w(format!("loop {} {} {} {} {} {} {} {}", q(&sp.id), l.x, l.y, l.scale, l.xscale, l.yscale, l.time, l.ease.name()));
        }
    }
    if s.camera != Camera::default() {
        w(format!("camera {} {} {}", s.camera.zoom, s.camera.x, s.camera.y));
    }
    // Saves from before `::textbox` existed have no such record: they load with the box shown.
    if s.textbox_hidden {
        w("textbox hidden".to_string());
    }
    // Saves from before `::fade` existed have no such record: they load uncovered.
    if s.cover.is_some() {
        w(format!("cover {}", color(s.cover)));
    }
    // Saves from before `::hud` existed have no such records: they load with none.
    for h in &s.hud {
        w(hud_record(h));
    }
    // The intro, if any, is a fourth value; saves from before it existed load without one.
    if let Some(m) = &s.music {
        match &m.intro {
            Some(intro) => w(format!("music {} {} {} {}", q(&m.src), m.looped, m.volume, q(intro))),
            None => w(format!("music {} {} {}", q(&m.src), m.looped, m.volume)),
        }
    }
    // Saves from before looping sounds existed have no such record: they load with none.
    for l in &s.sfx_loops {
        w(format!("sfxloop {} {}", q(&l.src), l.volume));
    }
    for (src, alt) in &s.inline {
        w(format!("inline {} {}", q(src), q(alt)));
    }
    if let Some(v) = &s.voice {
        w(format!("voice {}", q(v)));
    }
    if let Some(l) = &s.line {
        let speaker = l.speaker.as_deref().map_or("-".into(), q);
        w(format!("line {speaker} {} {}", color(l.color), q(&l.text)));
    }
    match &s.pending {
        Some(Cmd::Say { .. }) => w("pending say".into()),
        Some(Cmd::Wait { seconds }) => w(format!("pending wait {seconds}")),
        Some(Cmd::PlayVideo { src, fit, looped, skippable }) => {
            let fit = match fit {
                Fit::Contain => "contain",
                Fit::Cover => "cover",
                Fit::Stretch => "stretch",
            };
            w(format!("pending video {} {fit} {looped} {skippable}", q(src)));
        }
        Some(Cmd::ShowChoices { options }) => {
            let opts: Vec<String> = options.iter().map(|o| q(o)).collect();
            w(format!("pending choices {}", opts.join(" ")));
        }
        // The regions themselves come from the script when the save is read.
        Some(Cmd::ShowHotspots { .. }) => w("pending hotspots".into()),
        // Likewise the inventory and the shop (prices, what is owned) are read from the script.
        Some(Cmd::ShowInventory) => w("pending inventory".into()),
        Some(Cmd::ShowShop { .. }) => w("pending shop".into()),
        // The prompt comes from the script (with the variables as they are); what was typed so far is not saved.
        Some(Cmd::AskInput { .. }) => w("pending input".into()),
        _ => {}
    }
    if s.finished {
        w("finished".into());
    }
    w("end".into());
}

// ---- reading -----------------------------------------------------------------------------

/// The name and value of a `var` record's words (see [`var_record`]).
pub(crate) fn read_var(t: &[String]) -> Result<(String, Value), String> {
    let (Some(name), Some(kind)) = (t.get(1), t.get(2)) else {
        return Err("a `var` record is too short".into());
    };
    if kind == "list" {
        if t.len() - 3 > MAX_LIST_ITEMS {
            return Err(format!("the list `{name}` has more than {MAX_LIST_ITEMS} items"));
        }
        let mut seen: HashSet<&str> = HashSet::with_capacity(t.len() - 3);
        let items: Vec<String> = t[3..].iter().filter(|i| !i.is_empty() && seen.insert(i.as_str())).cloned().collect();
        return Ok((name.clone(), Value::List(items)));
    }
    let Some(v) = t.get(3) else {
        return Err("a `var` record is too short".into());
    };
    let bad = || format!("`{v}` is not a {kind}");
    let value = match kind.as_str() {
        "int" => Value::Int(v.parse().map_err(|_| bad())?),
        "bool" => Value::Bool(v.parse().map_err(|_| bad())?),
        "text" => Value::Str(v.clone()),
        other => return Err(format!("unknown value type `{other}`")),
    };
    Ok((name.clone(), value))
}

/// Split a record into words and unquoted strings.
pub fn tokens(line: &str) -> Result<Vec<String>, String> {
    let mut out = Vec::new();
    let mut chars = line.chars().peekable();
    while let Some(&c) = chars.peek() {
        if c.is_whitespace() {
            chars.next();
        } else if c == '"' {
            chars.next();
            let mut s = String::new();
            loop {
                match chars.next() {
                    Some('"') => break,
                    Some('\\') => s.push(match chars.next() {
                        Some('n') => '\n',
                        Some('r') => '\r',
                        Some('t') => '\t',
                        Some(other) => other,
                        None => return Err("unfinished escape".into()),
                    }),
                    Some(c) => s.push(c),
                    None => return Err("unclosed quote".into()),
                }
            }
            out.push(s);
        } else {
            let mut s = String::new();
            while let Some(&c) = chars.peek() {
                if c.is_whitespace() {
                    break;
                }
                s.push(c);
                chars.next();
            }
            out.push(s);
        }
    }
    Ok(out)
}

fn num<T: std::str::FromStr>(s: &str, line: usize) -> Result<T, String> {
    s.parse().map_err(|_| format!("line {line}: `{s}` is not a number"))
}

fn read_state<'a>(lines: &mut impl Iterator<Item = (usize, &'a str)>, program: &Program, edited: bool) -> Result<State, String> {
    let mut s = State::new(program); // starts from the script's defaults, so new variables exist
    let mut saw_at = false;
    let mut dropped: Vec<String> = Vec::new(); // sprites an edit took the picture of
    for (n, line) in lines.by_ref() {
        let t = tokens(line).map_err(|e| format!("line {n}: {e}"))?;
        let arg = |i: usize| t.get(i).map(String::as_str).ok_or(format!("line {n}: `{line}` is too short"));
        let f = |i: usize| -> Result<f32, String> { num(arg(i)?, n) };
        let b = |i: usize| -> Result<bool, String> { num(arg(i)?, n) };
        // Saves are shared between players, so a path in one must not reach outside the game folder.
        let path = |i: usize| -> Result<String, String> {
            let p = arg(i)?;
            if is_inside(p) { Ok(p.to_string()) } else { Err(format!("line {n}: unsafe file path `{p}`")) }
        };
        // A picture: a file, or an animation the script declares.
        let pic = |i: usize| -> Result<String, String> {
            let p = arg(i)?;
            match p.strip_prefix(ANIM_PREFIX) {
                Some(id) if program.anims.contains_key(id) => Ok(p.to_string()),
                Some(id) => Err(format!("line {n}: unknown animation `{id}` (the script no longer declares it)")),
                None => path(i),
            }
        };
        // After a script edit, a picture that is an animation no longer declared is dropped.
        let gone = |i: usize| edited && t.get(i).and_then(|p| p.strip_prefix(ANIM_PREFIX)).is_some_and(|id| !program.anims.contains_key(id));
        match arg(0)? {
            "bg" if gone(1) => {}
            "sprite" if gone(2) => dropped.push(arg(1)?.to_string()),
            "end" if saw_at => {
                // A save whose script changed so the op is gone is dealt with by the caller
                // (`consistent`): the placeholder stays and the story re-runs from the spot.
                // Choices and hotspots are rebuilt from the script too, never from saved text.
                let rebuilt = match (&s.pending, program.ops.get(s.pc)) {
                    (Some(Cmd::ShowChoices { .. } | Cmd::ShowHotspots { .. }), Some(op @ Op::Choice { .. })) => rebuild_pending(program, op, &s.vars),
                    (Some(Cmd::ShowShop { .. }), Some(op @ Op::Shop { .. })) => rebuild_pending(program, op, &s.vars),
                    (Some(Cmd::AskInput { .. }), Some(op @ Op::Input { .. })) => rebuild_pending(program, op, &s.vars),
                    _ => None,
                };
                if rebuilt.is_some() {
                    s.pending = rebuilt;
                }
                return Ok(s);
            }
            "end" => return Err(format!("line {n}: state without a position")),
            "at" => {
                s.pc = resolve(program, &t[1..], n)?;
                saw_at = true;
            }
            "stack" => {
                if s.stack.len() >= MAX_STACK_RECORDS {
                    return Err(format!("line {n}: more than {MAX_STACK_RECORDS} `stack` records in a state"));
                }
                s.stack.push(resolve(program, &t[1..], n)?);
            }
            "var" => {
                let (name, value) = read_var(&t).map_err(|e| format!("line {n}: {e}"))?;
                // A list is a list and a number is not: a record of the other kind than the
                // script declares is ignored (the declared default stays). Numbers, text and
                // booleans may change into each other as the story runs, so those pass.
                let is_list = |v: &Value| matches!(v, Value::List(_));
                if program.vars.get(&name).is_none_or(|d| is_list(d) == is_list(&value)) {
                    s.vars.insert(name, value);
                }
            }
            "rng" => s.rng = num(arg(1)?, n)?,
            // A solid colour is `color:#rrggbb`; anything else is a file and must be inside the game folder.
            "bg" if bg_color(arg(1)?).is_some() => s.background = Some(arg(1)?.to_string()),
            "bg" => s.background = Some(pic(1)?),
            "hud" => {
                if s.hud.len() >= MAX_HUD_RECORDS {
                    return Err(format!("line {n}: more than {MAX_HUD_RECORDS} `hud` records in a state"));
                }
                s.set_hud(read_hud(&t, n)?);
            }
            "sprite" => s.sprites.push(SpriteState {
                id: arg(1)?.to_string(),
                src: pic(2)?,
                x: sane(f(3)?, 0.0, 1.0, 0.5),
                scale: sane(f(4)?, 0.05, 10.0, 1.0),
                y: if t.len() > 5 { sane(f(5)?, -0.5, 1.5, 1.0) } else { 1.0 },
                z: if t.len() > 6 { num::<i32>(arg(6)?, n)?.clamp(-100, 100) } else { 0 },
                // lenient: a damaged flag is just "not flipped"
                flip: t.get(7).is_some_and(|v| v == "true"),
                xscale: if t.len() > 8 { sane(f(8)?, 0.05, 10.0, 1.0) } else { 1.0 },
                yscale: if t.len() > 9 { sane(f(9)?, 0.05, 10.0, 1.0) } else { 1.0 },
                looped: None,
            }),
            "loop" => {
                let id = arg(1)?;
                let Some(sprite) = s.sprites.iter_mut().find(|sp| sp.id == id) else {
                    if dropped.iter().any(|d| d == id) {
                        continue;
                    }
                    return Err(format!("line {n}: a `loop` for the sprite `{id}`, which is not on stage"));
                };
                sprite.looped = Some(SpriteLoop {
                    x: sane(f(2)?, 0.0, 1.0, 0.5),
                    y: sane(f(3)?, -0.5, 1.5, 1.0),
                    scale: sane(f(4)?, 0.05, 10.0, 1.0),
                    xscale: sane(f(5)?, 0.05, 10.0, 1.0),
                    yscale: sane(f(6)?, 0.05, 10.0, 1.0),
                    time: sane(f(7)?, 0.02, MAX_LOOP_LEG, 1.0),
                    ease: Ease::parse(arg(8)?).unwrap_or_default(),
                });
            }
            "camera" => s.camera = Camera { zoom: sane(f(1)?, 1.0, 4.0, 1.0), x: sane(f(2)?, 0.0, 1.0, 0.5), y: sane(f(3)?, 0.0, 1.0, 0.5) },
            "textbox" => s.textbox_hidden = arg(1)? == "hidden",
            "music" => {
                let intro = if t.len() > 4 { Some(path(4)?) } else { None };
                s.music = Some(MusicState { src: path(1)?, intro, looped: b(2)?, volume: sane(f(3)?, 0.0, 1.0, 1.0) });
            }
            "cover" => {
                let parts: Vec<f32> = arg(1)?.split(',').map(|p| num(p, n)).collect::<Result<_, _>>()?;
                let [r, g, b] = parts[..] else { return Err(format!("line {n}: bad colour `{}`", arg(1)?)) };
                s.cover = Some([sane(r, 0.0, 1.0, 0.0), sane(g, 0.0, 1.0, 0.0), sane(b, 0.0, 1.0, 0.0)]);
            }
            "sfxloop" => {
                let volume = f(2)?;
                let src = path(1)?;
                if !s.sfx_loops.iter().any(|l| l.src == src) {
                    s.sfx_loops.push(SfxLoopState { src, volume: sane(volume, 0.0, 1.0, 1.0) });
                }
            }
            "inline" => s.inline.push((path(1)?, arg(2)?.to_string())),
            "voice" => s.voice = Some(path(1)?),
            "line" => {
                let speaker = match (arg(1)?, line.split_whitespace().nth(1)) {
                    ("-", Some("-")) => None, // an unquoted dash is "no speaker"
                    (name, _) => Some(name.to_string()),
                };
                let color = match arg(2)? {
                    "-" => None,
                    c => {
                        let parts: Vec<f32> = c.split(',').map(|p| num(p, n)).collect::<Result<_, _>>()?;
                        let [r, g, b] = parts[..] else { return Err(format!("line {n}: bad colour `{c}`")) };
                        Some([r, g, b])
                    }
                };
                s.line = Some(Line { speaker, color, text: arg(3)?.to_string() });
            }
            "pending" => {
                s.pending = Some(match arg(1)? {
                    "say" => {
                        let l = s.line.as_ref().ok_or(format!("line {n}: pending say without a line"))?;
                        Cmd::Say { speaker: l.speaker.clone(), color: l.color, text: l.text.clone() }
                    }
                    "wait" => Cmd::Wait { seconds: sane(f(2)?, 0.01, MAX_WAIT, 1.0) },
                    "video" => Cmd::PlayVideo {
                        src: path(2)?,
                        fit: match arg(3)? {
                            "cover" => Fit::Cover,
                            "stretch" => Fit::Stretch,
                            _ => Fit::Contain,
                        },
                        looped: b(4)?,
                        skippable: b(5)?,
                    },
                    "choices" => Cmd::ShowChoices { options: t[2..].to_vec() },
                    // Rebuilt from the script at `end`, once the variables are known.
                    "hotspots" => Cmd::ShowHotspots { spots: Vec::new() },
                    "shop" => Cmd::ShowShop { pay: String::new(), money: 0, items: Vec::new() },
                    "inventory" => Cmd::ShowInventory,
                    "input" => Cmd::AskInput { prompt: String::new(), default: None, max: 1 },
                    other => return Err(format!("line {n}: unknown pending `{other}`")),
                });
            }
            "finished" => s.finished = true,
            other => return Err(format!("line {n}: unknown record `{other}`")),
        }
    }
    Err("the save ends in the middle of a state".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use quillstone_script::parse;

    const SCENE: &str = "\
---
vars: { name: \"Mi ka\", n: 0, ok: false }
---
::character{id=mika name=Mika color=#ffd98c sprites=m_{pose}.png}
# start
::bg{src=bg.png}
::music{src=calm.ogg volume=0.5}
::show{mika happy at=left}
**Mika:** Hello {name}.
![a scarf](scarf.png)
::voice{src=v.ogg}
Narration with \"quotes\" and a \\\\ backslash.
::call{to=sub}
::end
# sub
::set{n+=2, ok=true}
::choice
- [Yes](#yes)
- [No](#no)
::
# yes
**Mika:** Yes.
::end
# no
**Mika:** No.
";

    fn play<'p>(p: &'p Program, inputs: &[Input]) -> Session<'p> {
        let (mut game, _) = Session::start(p);
        for i in inputs {
            game.input(i.clone());
        }
        game
    }

    #[test]
    fn a_session_round_trips_exactly() {
        let p = parse(SCENE).unwrap();
        for inputs in [&[][..], &[Input::Advance][..], &[Input::Advance, Input::Advance][..]] {
            let game = play(&p, inputs);
            let (loaded, cmds) = Session::load(&p, &game.save()).unwrap();
            assert_eq!(loaded.state(), game.state(), "after {} inputs", inputs.len());
            assert_eq!(loaded.history, game.history);
            assert_eq!(cmds, game.state().redraw());
        }
    }

    #[test]
    fn the_saved_state_has_everything_in_it() {
        let p = parse(SCENE).unwrap();
        let game = play(&p, &[Input::Advance, Input::Advance]); // at the choice, inside the call
        let s = game.state();
        assert!(matches!(s.pending, Some(Cmd::ShowChoices { .. })));
        assert_eq!(s.stack.len(), 1);
        let (loaded, _) = Session::load(&p, &game.save()).unwrap();
        let l = loaded.state();
        assert_eq!(l.vars["n"], Value::Int(2));
        assert_eq!(l.vars["name"], Value::Str("Mi ka".into()));
        assert_eq!(l.sprites, s.sprites);
        assert_eq!(l.music, s.music);
        assert_eq!(l.line.as_ref().unwrap().text, "Narration with \"quotes\" and a \\\\ backslash.");
    }

    #[test]
    fn a_loaded_session_plays_on_and_rolls_back() {
        let p = parse(SCENE).unwrap();
        let game = play(&p, &[Input::Advance, Input::Advance]);
        let (mut loaded, _) = Session::load(&p, &game.save()).unwrap();
        let cmds = loaded.input(Input::Choose(0));
        assert!(cmds.iter().any(|c| matches!(c, Cmd::Say { text, .. } if text == "Yes.")));
        loaded.rollback();
        loaded.rollback();
        assert!(loaded.state().inline.len() == 1, "rolled back into the saved history");
        assert_eq!(loaded.backlog().len(), 2);
    }

    #[test]
    fn only_recent_history_is_kept() {
        let src: String = (0..150).map(|i| format!("**A:** line {i}\n")).collect();
        let p = parse(&src).unwrap();
        let game = play(&p, &vec![Input::Advance; 140]);
        let (loaded, _) = Session::load(&p, &game.save()).unwrap();
        assert_eq!(loaded.history.len(), SAVE_HISTORY);
        assert_eq!(loaded.state(), game.state());
    }

    #[test]
    fn a_finished_story_round_trips() {
        let p = parse("**A:** only\n").unwrap();
        let game = play(&p, &[Input::Advance]);
        assert!(game.state().finished);
        let (loaded, cmds) = Session::load(&p, &game.save()).unwrap();
        assert!(loaded.state().finished);
        assert_eq!(cmds.last(), Some(&Cmd::Finished));
    }

    #[test]
    fn edits_above_the_label_keep_the_save_in_place() {
        let before = parse("# a\n**A:** one\n# b\n**A:** two\n**A:** three\n").unwrap();
        let game = play(&before, &[Input::Advance, Input::Advance]); // on "three"
        let after = parse("# a\n**A:** new line\n**A:** one\n# b\n**A:** two\n**A:** three\n").unwrap();
        let (loaded, _) = Session::load(&after, &game.save()).unwrap();
        assert_eq!(loaded.state().line.as_ref().unwrap().text, "three");
        assert_eq!(loaded.state().pc, 3);
        assert_eq!(loaded.history.len(), 2, "history still matches, so it is kept");
    }

    #[test]
    fn a_save_that_no_longer_matches_restarts_from_its_spot() {
        let before = parse("# a\n**A:** one\n::choice\n- [x](#a)\n::\n").unwrap();
        let game = play(&before, &[Input::Advance]); // at the choice (offset 1 from `a`)
        let after = parse("# a\n**A:** one\n**A:** inserted\n::choice\n- [x](#a)\n::\n").unwrap();
        let (loaded, cmds) = Session::load(&after, &game.save()).unwrap();
        assert!(loaded.history.is_empty());
        assert!(cmds.iter().any(|c| matches!(c, Cmd::Say { text, .. } if text == "inserted")));
        assert!(matches!(loaded.state().pending, Some(Cmd::Say { .. })), "waiting again, not stuck");
    }

    #[test]
    fn new_variables_get_their_defaults() {
        let before = parse("**A:** hi\n").unwrap();
        let game = play(&before, &[]);
        let after = parse("---\nvars: { added: 7 }\n---\n**A:** hi\n").unwrap();
        let (loaded, _) = Session::load(&after, &game.save()).unwrap();
        assert_eq!(loaded.state().vars["added"], Value::Int(7));
    }

    #[test]
    fn broken_saves_are_errors_not_panics() {
        let p = parse(SCENE).unwrap();
        let good = play(&p, &[Input::Advance]).save();
        assert!(Session::load(&p, "").is_err());
        assert!(Session::load(&p, "hello\n").is_err());
        assert!(Session::load(&p, &good[..good.len() / 2]).is_err());
        assert!(Session::load(&p, &good.replace("\"bg.png\"", "\"bg.png")).is_err());
        assert!(Session::load(&p, "quillstone-session 1\nstate\nat \"gone\" 1\nend\n").is_err(), "old save, label gone");
    }

    #[test]
    fn a_save_cannot_point_outside_the_game_folder() {
        let p = parse(SCENE).unwrap();
        let good = play(&p, &[Input::Advance]).save();
        let vp = parse("::video{src=c.webm}\n**A:** y\n").unwrap();
        let video = play(&vp, &[]).save();
        assert!(good.contains("\"calm.ogg\"") && video.contains("pending video \"c.webm\""));
        for bad in ["\\\\host\\share\\x.png", "../../x.png", "/etc/passwd", "C:/x.png"] {
            let bad = q(bad);
            for (prog, save, from) in [(&p, &good, "\"bg.png\""), (&p, &good, "\"calm.ogg\""), (&vp, &video, "\"c.webm\"")] {
                let e = Session::load(prog, &save.replace(from, &bad)).err().expect(&bad);
                assert!(e.contains("unsafe file path"), "{e}");
            }
        }
        let fine = good.replace("\"bg.png\"", "\"assets/bg.png\"");
        assert_eq!(Session::load(&p, &fine).unwrap().0.state().background.as_deref(), Some("assets/bg.png"));
    }

    #[test]
    fn a_renamed_label_finds_the_line_by_its_words() {
        let before = parse("# intro\n**A:** one\n**A:** two\n# next\n**A:** three\n").unwrap();
        let game = play(&before, &[Input::Advance]); // on "two"
        // Renamed, and a line added above it: the raw position is now wrong, the words aren't.
        let after = parse("# prologue\n**A:** zero\n**A:** one\n**A:** two\n# next\n**A:** three\n").unwrap();
        let (loaded, _) = Session::load(&after, &game.save()).unwrap();
        assert_eq!(loaded.state().line.as_ref().unwrap().text, "two");
        assert_eq!(loaded.history.len(), 1, "the history still fits too");
    }

    #[test]
    fn a_removed_label_falls_back_to_the_old_position() {
        let before = parse("# intro\n**A:** one\n**A:** two\n").unwrap();
        let game = play(&before, &[Input::Advance]); // on "two"
        // Label gone and the line reworded: only the raw position still points at it.
        let after = parse("**A:** one\n**A:** two, reworded\n").unwrap();
        let (loaded, _) = Session::load(&after, &game.save()).unwrap();
        assert_eq!(loaded.state().pc, 1);
        assert!(matches!(loaded.state().pending, Some(Cmd::Say { .. })), "loads, waiting at that line");
    }

    #[test]
    fn call_stacks_survive_a_renamed_label() {
        let src = |l: &str| format!("# {l}\n::call{{to=sub}}\n**A:** back\n::end\n# sub\n**A:** inside\n::return\n");
        let before = parse(&src("main")).unwrap();
        let game = play(&before, &[]); // inside the call
        let after = parse(&src("story")).unwrap();
        let (mut loaded, _) = Session::load(&after, &game.save()).unwrap();
        let cmds = loaded.input(Input::Advance);
        assert!(cmds.iter().any(|c| matches!(c, Cmd::Say { text, .. } if text == "back")), "returns to the right spot");
    }

    #[test]
    fn the_camera_and_sprite_sizes_are_saved() {
        let p = parse("::show{src=s.png id=s}\n::move{s scale=1.5 time=0}\n::camera{zoom=2 x=0.25 time=0}\n**A:** hi\n").unwrap();
        let (game, _) = Session::start(&p);
        let (loaded, cmds) = Session::load(&p, &game.save()).unwrap();
        assert_eq!(loaded.state().camera, Camera { zoom: 2.0, x: 0.25, y: 0.5 });
        assert_eq!(loaded.state().sprites[0].scale, 1.5);
        assert!(cmds.iter().any(|c| matches!(c, Cmd::Camera { time, .. } if *time == 0.0)));
    }

    #[test]
    fn line_ids_follow_the_line_not_its_position() {
        let before = parse("# a\n**A:** one\n**A:** two\n# b\n**A:** one\n::wait{time=1}\n").unwrap();
        let ids: Vec<Option<u64>> = (0..4).map(|pc| line_id(&before, pc)).collect();
        assert!(ids[0].is_some() && ids[0] != ids[1]);
        assert_ne!(ids[0], ids[2], "the same text under another label is another line");
        assert_eq!(ids[3], None, "a wait is not a line");
        let after = parse("# a\n**A:** new\n**A:** one\n**A:** two\n# b\n**A:** one\n").unwrap();
        assert_eq!(line_id(&after, 1), ids[0]);
        assert_eq!(line_id(&after, 3), ids[2]);
        let reworded = parse("# a\n**A:** one!\n").unwrap();
        assert_ne!(line_id(&reworded, 0), ids[0]);
    }

    #[test]
    fn the_demo_saves_and_loads_at_every_step() {
        let program = parse(include_str!("../../../examples/demo.quill.md")).unwrap();
        let (mut game, _) = Session::start(&program);
        for _ in 0..40 {
            let (loaded, _) = Session::load(&program, &game.save()).unwrap();
            assert_eq!(loaded.state(), game.state());
            if game.state().finished {
                break;
            }
            match game.state().pending {
                Some(Cmd::ShowChoices { .. }) => game.input(Input::Choose(0)),
                _ => game.input(Input::Advance),
            };
        }
        assert!(game.state().finished);
    }

    // ---- list variables ----

    fn awkward() -> Vec<String> {
        ["a b", "say \"hi\"", "x, y", "back\\slash", "two\nlines\ttab", "ünï ✓", "  padded  ", "{inventory}"]
            .iter()
            .map(|s| s.to_string())
            .collect()
    }

    #[test]
    fn a_list_round_trips_through_a_var_record() {
        for items in [vec![], vec!["one".to_string()], awkward()] {
            let rec = var_record("inv", &Value::List(items.clone()));
            assert!(!rec.contains('\n'), "one line per record");
            let t = tokens(&rec).unwrap();
            assert_eq!(read_var(&t).unwrap(), ("inv".to_string(), Value::List(items)));
        }
        assert_eq!(var_record("inv", &Value::List(vec![])), "var \"inv\" list");
        // a hand-edited record with repeats or empty words still reads as a set
        let t = tokens("var \"inv\" list \"a\" \"a\" \"\" \"b\"").unwrap();
        assert_eq!(read_var(&t).unwrap().1, Value::List(vec!["a".into(), "b".into()]));
    }

    #[test]
    fn a_session_keeps_its_list_in_a_save() {
        let src = "---\nvars: { inv: [], n: 1 }\n---\n# a\n**A:** one\n::set{inv += \"map\"}\n::choice\n- [Take](#b){inv += \"key\"}\n::\n# b\n**A:** two\n";
        let p = parse(src).unwrap();
        let mut game = play(&p, &[Input::Advance]); // at the choice, holding the map
        game.state.vars.insert("inv".into(), Value::List(awkward()));
        let (loaded, _) = Session::load(&p, &game.save()).unwrap();
        assert_eq!(loaded.state().vars["inv"], Value::List(awkward()));
        assert_eq!(loaded.state(), game.state());
        // history keeps earlier lists, so rolling back after loading restores them too
        let mut game = play(&p, &[Input::Advance, Input::Choose(0)]);
        assert_eq!(game.state().vars["inv"], Value::List(vec!["map".into(), "key".into()]));
        let (mut loaded, _) = Session::load(&p, &game.save()).unwrap();
        assert_eq!(loaded.state().vars["inv"], game.state().vars["inv"]);
        loaded.rollback().unwrap();
        game.rollback().unwrap();
        assert_eq!(loaded.state().vars["inv"], Value::List(vec!["map".into()]));
        assert_eq!(loaded.state(), game.state());
    }

    #[test]
    fn a_hidden_textbox_is_saved_and_an_old_save_loads_shown() {
        let p = parse("**A:** one\n::textbox{hide}\n::wait{time=1}\n**A:** two\n").unwrap();
        let (mut game, _) = Session::start(&p);
        game.input(Input::Advance);
        assert!(game.state().textbox_hidden);
        let text = game.save();
        assert!(text.contains("textbox hidden"), "{text}");
        let (loaded, cmds) = Session::load(&p, &text).unwrap();
        assert!(loaded.state().textbox_hidden);
        assert_eq!(loaded.state(), game.state());
        assert!(cmds.contains(&Cmd::Textbox { hidden: true, fade: 0.0 }));
        // history keeps its own copy: back at the first line the box is shown
        let (mut loaded, _) = Session::load(&p, &text).unwrap();
        loaded.rollback();
        assert!(!loaded.state().textbox_hidden);
        // a save from before the command existed has no such record
        let old: String = text.lines().filter(|l| !l.starts_with("textbox")).map(|l| format!("{l}\n")).collect();
        let (loaded, cmds) = Session::load(&p, &old).unwrap();
        assert!(!loaded.state().textbox_hidden);
        assert!(!cmds.iter().any(|c| matches!(c, Cmd::Textbox { .. })));
    }
}
