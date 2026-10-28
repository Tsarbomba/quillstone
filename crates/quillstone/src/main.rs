//! Quillstone command line: `check`, `test`, `voices`, `dump`, a text-mode `run`, and `package`.
//! The windowed player is its own crate, in `player/`.

use std::collections::{BTreeMap, HashSet};
use std::io::{self, BufRead, Write};
use std::path::Path;
use std::process::ExitCode;

use quillstone_runtime::{Cmd, Input, Session};
use quillstone_script::markup::plain;
use quillstone_script::{kind_from_path, AssetKind, Op, ParticleKind, Particles, Program, MAX_PARTICLES};

mod lang;
mod package;
mod pattern;
mod picture;

const USAGE: &str = "\
Quillstone

USAGE:
    quillstone check <script>   validate a script and that its assets exist
    quillstone test  <script> [--limit <n>] [--allow-partial]
                                play every branch: find endless loops, places the story
                                can't end from, unreachable parts, options never offered
                                and variables never set (explores up to n situations, 100000).
                                Exits 1 for a real problem, 2 if it stopped at the limit
                                (only part of the story explored) unless --allow-partial
    quillstone voices <script>  list every line's auto-voice file name, speaker and text
                                (tab-separated, for a spreadsheet: the recording script)
    quillstone lang export <script> <code>
                                create or update lang/<code>.txt for a language listed in the
                                script's `languages:`: new texts are added, texts no longer in
                                the script are flagged obsolete (translations are never deleted)
    quillstone dump  <script>   print the compiled ops and labels
    quillstone run   <script>   play the script in the terminal
    quillstone package <script> --player <exe> --out <folder> [--name <name>] [--libs <folder>]
                                build a folder players can unzip and run: the player
                                (renamed after the title, or --name), FFmpeg's libraries
                                (from beside the player, or --libs) and the files the game uses
";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    let result = match args.as_slice() {
        ["check", file] => check(Path::new(file)),
        ["test", file, flags @ ..] => {
            let run = test_flags(flags).and_then(|(limit, allow)| Ok((test(Path::new(file), limit)?, allow)));
            return match run {
                Ok((outcome, allow_partial)) => ExitCode::from(test_exit_code(outcome, allow_partial)),
                Err(msg) => {
                    eprintln!("{msg}");
                    ExitCode::FAILURE
                }
            };
        }
        ["dump", file] => dump(Path::new(file)),
        ["voices", file] => voices(Path::new(file)),
        ["lang", "export", file, code] => load(Path::new(file)).and_then(|p| lang::export(Path::new(file), &p, code)),
        ["run", file] => run(Path::new(file)),
        ["package", file, rest @ ..] => package_cmd(Path::new(file), rest),
        _ => {
            eprint!("{USAGE}");
            return ExitCode::from(2);
        }
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(msg) => {
            eprintln!("{msg}");
            ExitCode::FAILURE
        }
    }
}

/// `--limit <n>` and `--allow-partial`, in any order: (limit, allow partial).
fn test_flags(flags: &[&str]) -> Result<(usize, bool), String> {
    let (mut limit, mut allow) = (DEFAULT_LIMIT, false);
    let mut it = flags.iter();
    while let Some(&flag) = it.next() {
        match flag {
            "--allow-partial" => allow = true,
            "--limit" => {
                let n = it.next().ok_or("--limit needs a number")?;
                limit = n.parse().ok().filter(|&n| n > 0).ok_or(format!("--limit needs a positive number, not `{n}`"))?;
            }
            _ => return Err(format!("unknown option `{flag}`\n\n{USAGE}")),
        }
    }
    Ok((limit, allow))
}

/// How a `test` run that found no error ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Explored {
    Everything,
    /// Stopped at `--limit`: the rest of the story was not looked at.
    Part,
}

/// The process exit code of a `test` run that found no error: 0, or 2 for a partial
/// run (1 is for real problems) unless the caller accepts that.
fn test_exit_code(outcome: Explored, allow_partial: bool) -> u8 {
    if outcome == Explored::Part && !allow_partial { 2 } else { 0 }
}

fn load(path: &Path) -> Result<Program, String> {
    let src = std::fs::read_to_string(path)
        .map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    quillstone_ui::source::parse_script(path, &src).map_err(|errors| quillstone_ui::source::error_lines(path, &errors).join("\n"))
}

/// An included file (a game-folder path) as a path from where the command was run.
fn game_file(script: &Path, file: &str) -> std::path::PathBuf {
    script.parent().unwrap_or_else(|| Path::new("")).join(file)
}

/// `file:line` of a program line number: the script's own path, or the included file's.
fn at(program: &Program, script: &Path, line: usize) -> String {
    match program.locate(line) {
        (Some(file), n) => format!("{}:{n}", game_file(script, file).display()),
        (None, n) => format!("{}:{n}", script.display()),
    }
}

fn check(path: &Path) -> Result<(), String> {
    let program = load(path)?;
    let root = path.parent().unwrap_or_else(|| Path::new("."));

    let mut missing = 0;
    let hot = hotspot_images(&program);
    let mut checked: HashSet<String> = HashSet::new();
    for (line, kind, asset) in program.assets_with_lines() {
        if hot.contains(&(line, asset)) {
            continue; // below, as pictures
        }
        if quillstone_script::is_pattern(asset) {
            for (error, msg) in check_pattern_at(&at(&program, path, line), root, asset, &mut checked) {
                missing += usize::from(error);
                eprintln!("{msg}");
            }
            continue;
        }
        let file = root.join(asset);
        if !quillstone_script::is_inside(asset) {
            // Saves refuse such paths (a shared save must not reach outside the game), so the game would break.
            missing += 1;
            eprintln!("{}: error: `{asset}` is outside the script's folder (keep assets beside or below it)", at(&program, path, line));
        } else if !file.exists() {
            missing += 1;
            eprintln!("{}: error: missing asset `{asset}`", at(&program, path, line));
        } else if let Some(why) = unplayable(&file, kind) {
            missing += 1;
            eprintln!("{}: error: `{asset}` won't play in a shipped game: {why}", at(&program, path, line));
        }
    }
    for (error, msg) in picture_findings(&program, path, root) {
        missing += usize::from(error);
        eprintln!("{msg}");
    }
    for (error, msg) in layer_size_findings(&program, path, root) {
        missing += usize::from(error);
        eprintln!("{msg}");
    }
    for (pc, text) in program.unknown_items() {
        eprintln!(
            "{}: warning: \"{text}\" is not a declared `::item` (it will show in the inventory under that name, with no picture; a typo?)",
            at(&program, path, program.lines[pc])
        );
    }
    if let Some(theme_path) = &program.theme {
        for msg in theme_problems(&root.join(theme_path), root) {
            missing += 1;
            eprintln!("{msg}");
        }
        for msg in quick_menu_warnings(&root.join(theme_path), root) {
            eprintln!("{msg}");
        }
    }
    missing += check_extras(&program, path, root);
    for (pc, msg) in hidden_hotspots(&program) {
        eprintln!("{}: warning: {msg}", at(&program, path, program.lines[pc]));
    }
    for pc in all_hidden_hotspot_blocks(&program) {
        eprintln!(
            "{}: warning: every region of this `::hotspots` block is `hidden`: players using only keyboard or gamepad can't pick anything here; add a visible region such as an exit",
            at(&program, path, program.lines[pc])
        );
    }
    for (line, msg) in line_warnings(&program, root) {
        eprintln!("{}: warning: {msg}", at(&program, path, line));
    }
    for (line, msg) in replay_warnings(&program) {
        eprintln!("{}: warning: {msg}", at(&program, path, line));
    }
    missing += lang::check(path, &program);
    let voiced = check_voices(&program, path, root, &mut missing);
    if missing > 0 {
        return Err(format!("{missing} problem(s) (paths are relative to the script's folder)"));
    }
    println!("ok: {} ops, {} labels{voiced}", program.ops.len(), program.labels.len());
    Ok(())
}

/// Replays that no `::end_replay` follows: `(line, message)`. A static scan along every
/// jump, call and choice from the label, so approximate (it can't tell which branches
/// a player can really take), and only a warning.
fn replay_warnings(program: &Program) -> Vec<(usize, String)> {
    let mut out = Vec::new();
    for replay in &program.replays {
        let at = program.labels[&replay.label];
        let ends = program.ops_ahead(at, program.ops.len() + 1).into_iter().any(|i| matches!(program.ops[i], Op::EndReplay));
        if !ends {
            let msg = format!(
                "replay `{}` has no `::end_replay` after it, so it will run to the end of the game (or its `::return`)",
                replay.label
            );
            out.push((program.lines.get(at).copied().unwrap_or(0), msg));
        }
    }
    out
}

/// More files than this behind one picture name `{variable}` is probably a folder of unrelated art:
/// every one of them ships with the game.
const MANY_MATCHES: usize = 200;

/// A picture name with `{variable}` parts stands for every file it can build: there must
/// be at least one, and each must be a picture the player can show (`checked` skips files
/// another line already looked at).
#[cfg(test)]
fn check_pattern(script: &Path, root: &Path, line: usize, pattern: &str, checked: &mut HashSet<String>) -> Vec<Finding> {
    check_pattern_at(&format!("{}:{line}", script.display()), root, pattern, checked)
}

/// [`check_pattern`] for a line already written as `file:line`.
fn check_pattern_at(at: &str, root: &Path, pattern: &str, checked: &mut HashSet<String>) -> Vec<Finding> {
    let files = pattern::expand(root, pattern);
    if files.is_empty() {
        let msg = format!("{at}: error: `{pattern}` matches no file (each `{{variable}}` stands for a file-name part in that folder; keep the files beside or below the script)");
        return vec![(true, msg)];
    }
    let mut out = Vec::new();
    if files.len() > MANY_MATCHES {
        out.push((
            false,
            format!(
                "{at}: warning: `{pattern}` matches {} files, and all of them ship with the game: keep unrelated files out of that folder",
                files.len()
            ),
        ));
    }
    for file in files.iter().filter(|f| checked.insert((*f).clone())) {
        if let Some(why) = unplayable(&root.join(file), AssetKind::Image) {
            out.push((true, format!("{at}: error: `{file}` (matched by `{pattern}`) won't show in a shipped game: {why}")));
        }
    }
    out
}

/// Ops of `::hotspots` blocks whose regions are all `hidden`.
fn all_hidden_hotspot_blocks(program: &Program) -> Vec<usize> {
    let mut out = Vec::new();
    for (pc, op) in program.ops.iter().enumerate() {
        let Op::Choice { options, .. } = op else { continue };
        let mut areas = options.iter().filter_map(|o| o.area.as_ref()).peekable();
        if areas.peek().is_some() && areas.all(|a| a.hidden) {
            out.push(pc);
        }
    }
    out
}

/// Hotspots that can never be clicked: an unconditional region lying completely
/// under a later one (which is drawn and hit-tested on top). (op index, message)
fn hidden_hotspots(program: &Program) -> Vec<(usize, String)> {
    let mut out = Vec::new();
    for (pc, op) in program.ops.iter().enumerate() {
        let Op::Choice { options, .. } = op else { continue };
        for (i, a) in options.iter().enumerate() {
            let Some(ra) = a.area.as_ref().filter(|_| a.cond.is_none()).map(|a| a.rect) else { continue };
            for b in options[i + 1..].iter().filter(|b| b.cond.is_none()) {
                let Some(rb) = b.area.as_ref().map(|a| a.rect) else { continue };
                let eps = 1e-4;
                let covered = rb[0] <= ra[0] + eps
                    && rb[1] <= ra[1] + eps
                    && rb[0] + rb[2] >= ra[0] + ra[2] - eps
                    && rb[1] + rb[3] >= ra[1] + ra[3] - eps;
                if covered {
                    let msg = format!(
                        "the hotspot \"{}\" lies completely under \"{}\" (listed after it, so on top) and can never be clicked",
                        plain(&a.text),
                        plain(&b.text)
                    );
                    out.push((pc, msg));
                    break;
                }
            }
        }
    }
    out
}

/// Icons are drawn at about 100 px; bigger files only cost load time and memory.
const BIG_ICON_SIDE: u32 = 512;

/// A particle picture is drawn up to `MAX_PARTICLES` times at its own size.
const BIG_PARTICLE_SIDE: u32 = 256;

/// Frames of an animation are loaded together; past this many `check` warns.
const MANY_FRAMES: usize = 64;

/// A frame bigger than this on a side is more than any screen shows.
const BIG_FRAME_SIDE: u32 = 2048;

/// `(line, path)` of every hotspot region's image.
fn hotspot_images(program: &Program) -> HashSet<(usize, &str)> {
    let regions = program.ops.iter().filter_map(|op| match op {
        Op::Choice { options, .. } => Some(options),
        _ => None,
    });
    regions.flatten().filter_map(|o| Some((o.line, o.area.as_ref()?.image.as_deref()?))).collect()
}

/// A problem (`true`) or warning (`false`), already formatted with its file and line.
type Finding = (bool, String);

/// Hotspot images (script order, each at its region's own line), then item icons (declaration order):
/// pictures whatever their extension, at a sensible size.
fn picture_findings(program: &Program, script: &Path, root: &Path) -> Vec<Finding> {
    let hot = hotspot_images(program);
    let spots = program.assets_with_lines().filter(|(l, _, a)| hot.contains(&(*l, *a))).map(|(l, _, a)| (l, a, "hotspot image"));
    let icons = program.item_icons().map(|(l, a)| (l, a, "item icon"));
    let frames = program.anim_frames().map(|(l, _, a)| (l, a, "animation frame"));
    let mut out = Vec::new();
    // Animations with a great many frames cost load time and memory all at once.
    for anim in program.anims.values().filter(|a| a.frames.len() > MANY_FRAMES) {
        out.push((
            false,
            format!(
                "{}: warning: animation `{}` has {} frames (more than {MANY_FRAMES}): every frame is loaded when it is shown; fewer, or a shorter loop, loads faster",
                at(program, script, anim.line),
                anim.id,
                anim.frames.len()
            ),
        ));
    }
    // A particle picture is also checked as any asset (it exists, it is a picture): only its size is left.
    for (pc, op) in program.ops.iter().enumerate() {
        let Op::Particles { set: Some(Particles { kind: ParticleKind::Image(asset), .. }), .. } = op else { continue };
        if let Ok((w, h)) = picture::read_picture(&root.join(asset)) {
            if w.max(h) > BIG_PARTICLE_SIDE {
                let at = at(program, script, program.lines.get(pc).copied().unwrap_or(0));
                out.push((
                    false,
                    format!(
                        "{at}: warning: `{asset}` is {w}x{h}: a particle is drawn at its pixel size, up to {MAX_PARTICLES} at once; \
                         a picture over {BIG_PARTICLE_SIDE} px is far too big for one"
                    ),
                ));
            }
        }
    }
    for (line, asset, noun) in spots.chain(icons).chain(frames) {
        let at = at(program, script, line);
        let file = root.join(asset);
        if !quillstone_script::is_inside(asset) {
            out.push((true, format!("{at}: error: `{asset}` is outside the script's folder (keep assets beside or below it)")));
        } else if !file.exists() {
            out.push((true, format!("{at}: error: missing {noun} `{asset}`")));
        } else {
            match picture::read_picture(&file) {
                Err(why) => out.push((true, format!("{at}: error: `{asset}` won't show in a shipped game: {why}"))),
                Ok((w, h)) if noun == "animation frame" && w.max(h) > BIG_FRAME_SIDE => out.push((
                    false,
                    format!("{at}: warning: `{asset}` is {w}x{h}: frames beyond {BIG_FRAME_SIDE} px only cost load time and memory (the screen is 1920x1080)"),
                )),
                Ok((w, h)) if noun != "animation frame" && w.max(h) > BIG_ICON_SIDE => out.push((
                    false,
                    format!("{at}: warning: `{asset}` is {w}x{h}: icons are drawn at about 100 px; a smaller file loads faster and saves memory"),
                )),
                Ok(_) => {}
            }
        }
    }
    out
}

/// The pictures of a character's layers must all have the canvas size of the first one (they are drawn
/// stacked in the same place and size): one warning for each file that differs. Files that can't be read
/// are reported elsewhere. `(is an error, message)`, like [`picture_findings`].
fn layer_size_findings(program: &Program, script: &Path, root: &Path) -> Vec<Finding> {
    let mut out = Vec::new();
    for character in program.characters.values().filter(|c| !c.layers.is_empty()) {
        // (line, file) in script order, each file once.
        let mut files: Vec<(usize, String)> = Vec::new();
        for (pc, op) in program.ops.iter().enumerate() {
            let Op::Show { id, layers, .. } = op else { continue };
            if *id != character.id {
                continue;
            }
            for (_, picture) in layers {
                let named: Vec<String> = match quillstone_script::anim_id(picture).and_then(|a| program.anims.get(a)) {
                    Some(anim) => anim.frames.iter().map(|f| f.file.clone()).collect(),
                    None if picture.is_empty() || quillstone_script::anim_id(picture).is_some() => Vec::new(),
                    None if quillstone_script::is_pattern(picture) => pattern::expand(root, picture),
                    None => vec![picture.clone()],
                };
                for file in named {
                    if !files.iter().any(|(_, f)| *f == file) {
                        files.push((program.lines[pc], file));
                    }
                }
            }
        }
        let mut canvas: Option<(String, u32, u32)> = None;
        for (line, file) in files {
            let Ok((w, h)) = picture::read_picture(&root.join(&file)) else { continue };
            match &canvas {
                None => canvas = Some((file, w, h)),
                Some((first, fw, fh)) if (*fw, *fh) != (w, h) => out.push((
                    false,
                    format!(
                        "{}: warning: layer picture `{file}` is {w}x{h} but `{first}` is {fw}x{fh}: the layers of `{}` are drawn stacked in the same place and size, so they should be the same size",
                        at(program, script, line),
                        character.id
                    ),
                )),
                Some(_) => {}
            }
        }
    }
    out
}

/// A timed `::choice` shorter than this (seconds) gets a warning.
const MIN_READABLE_TIME: f32 = 2.0;

/// Text clearly too long for where it is drawn: `(line, message)`. The player shrinks text that
/// doesn't fit, so these are warnings; the estimate errs long, so only clear cases count.
/// Covers dialogue lines, item descriptions and (non-hotspot) choice options.
fn line_warnings(program: &Program, root: &Path) -> Vec<(usize, String)> {
    let theme = program
        .theme
        .as_ref()
        .and_then(|t| std::fs::read_to_string(root.join(t)).ok())
        .map(|text| quillstone_render::Theme::parse(&text).0)
        .unwrap_or_default();
    let mut out = Vec::new();
    for (pc, op) in program.ops.iter().enumerate() {
        match op {
            Op::Say { speaker, text, .. } => {
                let (lines, fit) = quillstone_render::view::dialogue_lines_tagged(&theme, text, speaker.is_some(), theme.text_size);
                if lines > fit + 1 {
                    out.push((
                        program.lines[pc],
                        format!(
                            "this line takes about {lines} lines and the textbox fits {fit}; it will be shown \
                             smaller, or cut off if very long (split it into two lines?)"
                        ),
                    ));
                }
            }
            Op::Choice { options, time } => {
                if let Some(t) = time.filter(|t| *t < MIN_READABLE_TIME) {
                    out.push((
                        program.lines[pc],
                        format!("this choice gives {t} s to answer; under {MIN_READABLE_TIME} s is too quick to read the options"),
                    ));
                }
                let buttons: Vec<_> = options.iter().filter(|o| o.area.is_none()).collect();
                let Some(rect) = quillstone_render::view::choice_rects(&theme, buttons.len()).first().copied() else { continue };
                let size = theme.text_size.min(rect.h * 0.5); // as the player draws it
                let fit = (rect.h / (size * 1.35).round().max(1.0)).floor() as usize;
                for o in buttons {
                    let lines = quillstone_render::estimate_lines(&plain(&o.text), size, rect.w);
                    if lines > fit + 1 {
                        out.push((
                            o.line,
                            format!(
                                "this choice takes about {lines} lines and its button fits {fit}; it will be shown \
                                 smaller, or cut off if very long (shorten it?)"
                            ),
                        ));
                    }
                }
            }
            _ => {}
        }
    }
    // The description box: name above, price below (when the item has one), text between (26 px).
    let d = quillstone_ui::bag::description_rect(&theme);
    let mut items: Vec<_> = program.items.values().filter(|i| !i.text.is_empty()).collect();
    items.sort_by_key(|i| i.line);
    for item in items {
        let below = if item.price.is_some() { 38.0 } else { 0.0 };
        let body_h = (d.h - 20.0 - 46.0 - below).max(20.0);
        let fit = (body_h / (26.0f32 * 1.35).round()).floor() as usize;
        let lines = quillstone_render::estimate_lines(&plain(&item.text), 26.0, d.w - 48.0);
        if lines > fit + 1 {
            out.push((
                item.line,
                format!(
                    "the description of `{}` takes about {lines} lines and the description box fits {fit}; it will be shown \
                     smaller, or cut off if very long (shorten it?)",
                    item.id
                ),
            ));
        }
    }
    out
}

/// Every line's auto-voice name, in script order: (op index, name).
fn voice_lines(program: &Program) -> Vec<(usize, String)> {
    (0..program.ops.len()).filter_map(|pc| Some((pc, quillstone_runtime::voice_name(program, pc)?))).collect()
}

/// The sound files in the voice folder: name without extension -> file name.
fn voice_folder_files(program: &Program, root: &Path) -> Option<BTreeMap<String, String>> {
    let folder = program.voice_folder.as_ref()?;
    let entries = std::fs::read_dir(root.join(folder)).ok()?;
    let files = entries
        .filter_map(|e| e.ok()?.file_name().into_string().ok())
        .filter(|name| kind_from_path(name) == AssetKind::Audio)
        .filter_map(|name| Some((name.rsplit_once('.')?.0.to_string(), name)))
        .collect();
    Some(files)
}

/// Auto-voice: recordings that play, and ones left over from reworded lines.
/// Returns a note for the summary (", 12 of 40 lines voiced").
fn check_voices(program: &Program, path: &Path, root: &Path, problems: &mut usize) -> String {
    let Some(folder) = &program.voice_folder else { return String::new() };
    let Some(files) = voice_folder_files(program, root) else {
        eprintln!("{}: warning: the voice folder `{folder}` doesn't exist yet", path.display());
        return String::new();
    };
    let lines = voice_lines(program);
    let names: std::collections::HashSet<&str> = lines.iter().map(|(_, n)| n.as_str()).collect();
    for (stem, file) in &files {
        if !names.contains(stem.as_str()) {
            eprintln!(
                "{}: warning: `{folder}/{file}` matches no line (was its line reworded or removed? `quillstone voices` lists the names)",
                path.display()
            );
        } else if let Some(why) = unplayable(&root.join(folder).join(file), AssetKind::Audio) {
            *problems += 1;
            eprintln!("{}: error: `{folder}/{file}` won't play: {why}", path.display());
        }
    }
    let voiced = lines.iter().filter(|(_, n)| files.contains_key(n)).count();
    format!(", {voiced} of {} lines voiced", lines.len())
}

/// The recording script: one row per line, tab-separated.
fn voices(path: &Path) -> Result<(), String> {
    let program = load(path)?;
    let root = path.parent().unwrap_or_else(|| Path::new("."));
    let folder = program.voice_folder.clone().unwrap_or_else(|| "voice".into());
    let files = voice_folder_files(&program, root);
    if program.voice_folder.is_none() {
        eprintln!("note: add `voice: voice` to the front matter to play these automatically");
    }
    let tidy = |s: &str| s.replace(['\t', '\n'], " ");
    println!("file\tspeaker\tline{}", if files.is_some() { "\trecorded" } else { "" });
    for (pc, name) in voice_lines(&program) {
        let Some(quillstone_script::Op::Say { speaker, text, .. }) = program.ops.get(pc) else { continue };
        let recorded = match &files {
            Some(f) => format!("\t{}", if f.contains_key(&name) { "yes" } else { "no" }),
            None => String::new(),
        };
        println!("{folder}/{name}\t{}\t{}{recorded}", tidy(speaker.as_deref().unwrap_or("")), tidy(&plain(text)));
    }
    Ok(())
}

/// The gallery and music room: files that exist and play, and that the story
/// really shows (or nothing would ever unlock them). Returns how many problems.
fn check_extras(program: &Program, path: &Path, root: &Path) -> usize {
    use quillstone_script::Op;
    let shown = |src: &str| {
        program.ops.iter().any(|op| match op {
            Op::Bg { src: s, .. } | Op::Image { src: s, .. } => {
                // an animation background shows each of its frames
                let frame = quillstone_script::anim_id(s).and_then(|id| program.anims.get(id)).is_some_and(|a| a.frames.iter().any(|f| f.file == src));
                s == src || quillstone_script::pattern_matches(s, src) || frame
            }
            Op::Video { src: s, .. } => s == src || quillstone_script::pattern_matches(s, src),
            _ => false,
        })
    };
    let played = |src: &str| program.ops.iter().any(|op| matches!(op, Op::Music { src: s, .. } if s == src));
    let mut count = 0;
    let mut problem = |msg: String| {
        count += 1;
        eprintln!("{}: error: {msg}", path.display());
    };
    for (kind, asset) in program.extra_assets() {
        let file = root.join(asset);
        let in_gallery = program.gallery.iter().any(|g| g == asset);
        let place = if in_gallery { "gallery" } else { "music room" };
        if !file.exists() {
            problem(format!("missing {place} file `{asset}`"));
        } else if in_gallery && kind == AssetKind::Audio {
            problem(format!("`{asset}` in the gallery isn't a picture or a video"));
        } else if let Some(why) = if kind == AssetKind::Image { picture::read_picture(&file).err() } else { unplayable(&file, kind) } {
            problem(format!("`{asset}` won't play in a shipped game: {why}"));
        } else if in_gallery && !shown(asset) {
            problem(format!("`{asset}` is in the gallery, but no `::bg`, `::video` or image shows it, so it can never unlock"));
        } else if !in_gallery && !played(asset) {
            problem(format!("`{asset}` is in the music room, but no `::music` plays it, so it can never unlock"));
        }
    }
    count
}

/// Check a theme file and the files it names: one message per problem.
fn theme_problems(file: &Path, root: &Path) -> Vec<String> {
    let mut found = Vec::new();
    let text = match std::fs::read_to_string(file) {
        Ok(t) => t,
        Err(e) => {
            return vec![format!("{}: error: cannot read the theme: {e}", file.display())];
        }
    };
    let (theme, problems) = quillstone_render::Theme::parse(&text);
    found.extend(problems.iter().map(|p| format!("{}: error: {p}", file.display())));
    // The line of the theme file that names `asset`, for the message.
    let at = |asset: &str| {
        let line = text.lines().position(|l| l.split_once('=').is_some_and(|(_, v)| v.trim() == asset));
        line.map_or_else(|| file.display().to_string(), |i| format!("{}:{}", file.display(), i + 1))
    };
    let fonts = theme.fonts();
    for asset in theme.assets() {
        let target = root.join(asset);
        let is_font = fonts.contains(&asset);
        let is_music = theme.menu_music.as_deref() == Some(asset);
        let kind = if is_music { "theme asset" } else if is_font { "theme font" } else { "theme picture" };
        let why = if !quillstone_script::is_inside(asset) {
            Err(format!("`{asset}` is outside the script's folder (keep assets beside or below it)"))
        } else if !target.exists() {
            Err(format!("missing {kind} `{asset}`"))
        } else if is_font {
            picture::font_problem(&target).map_or(Ok(()), |why| Err(format!("`{asset}` can't be used as a font: {why}")))
        } else {
            let why = if is_music { unplayable(&target, AssetKind::Audio) } else { picture::read_picture(&target).err() };
            why.map_or(Ok(()), |why| Err(format!("`{asset}` won't play in a shipped game: {why}")))
        };
        if let Err(msg) = why {
            found.push(format!("{}: error: {msg}", at(asset)));
        }
    }
    found
}

/// Warnings about the quick menu's picture buttons: ones that overlap, and ones placed partly off the
/// screen (the player moves those back in).
fn quick_menu_warnings(file: &Path, root: &Path) -> Vec<String> {
    use quillstone_render::theme::QUICK_IDS;
    use quillstone_ui::screens::{quick_button_size, quick_layout};
    let Ok(text) = std::fs::read_to_string(file) else { return Vec::new() };
    let theme = quillstone_render::Theme::parse(&text).0;
    if theme.quick_icons.iter().all(|q| q.pos.is_none() && q.icon.is_none() && q.size.is_none()) {
        return Vec::new();
    }
    let size_of = |s: &str| {
        let size = quillstone_script::is_inside(s).then(|| picture::read_picture(&root.join(s)).ok()).flatten();
        size.map(|(w, h)| quillstone_render::Size::new(w as f32, h as f32))
    };
    // `file:line` of the last `quick_<id>_<field>` line, else just the file.
    let at = |id: &str, fields: &[&str]| {
        let key_of = |l: &str| l.split_once('=').map(|(k, _)| k.trim().to_string());
        let line = text.lines().enumerate().filter(|(_, l)| fields.iter().any(|f| key_of(l) == Some(format!("quick_{id}_{f}")))).last();
        line.map_or_else(|| file.display().to_string(), |(i, _)| format!("{}:{}", file.display(), i + 1))
    };
    let mut found = Vec::new();
    for (k, spec) in theme.quick_icons.iter().enumerate() {
        let Some([x, y]) = spec.pos else { continue };
        let (w, h) = quick_button_size(spec, &size_of);
        if x + w > 1920.0 || y + h > 1080.0 {
            found.push(format!(
                "{}: warning: the {w}x{h} quick menu button `{}` at {x}, {y} reaches past the 1920x1080 screen; the player moves it back in",
                at(QUICK_IDS[k], &["pos", "size", "icon"]),
                QUICK_IDS[k]
            ));
        }
    }
    let layout = quick_layout(quillstone_render::textbox_rect(&theme), &theme, true, &size_of);
    let overlap = |a: &quillstone_render::Rect, b: &quillstone_render::Rect| {
        a.x < b.x + b.w && b.x < a.x + a.w && a.y < b.y + b.h && b.y < a.y + a.h
    };
    for (i, (a, ra)) in layout.buttons.iter().enumerate() {
        for (b, rb) in &layout.buttons[i + 1..] {
            if overlap(ra, rb) {
                let (ia, ib) = (a.id(), b.id());
                found.push(format!(
                    "{}: warning: the quick menu buttons `{ia}` and `{ib}` overlap; `{ib}` is on top and gets the click where they do",
                    at(ib, &["pos", "size", "icon"])
                ));
            }
        }
    }
    found
}

/// Why a video or sound file won't play in a shipped game (videos must suit the
/// slim FFmpeg it ships with). `used_as` is how the script uses the file; a file
/// used as a picture may still be a video (backgrounds, inline images), which
/// its extension tells.
fn unplayable(file: &Path, used_as: AssetKind) -> Option<String> {
    use quillstone_video::probe::{unplayable_sound, unplayable_video, PROBE_BYTES};
    let kind = match used_as {
        AssetKind::Image => kind_from_path(&file.to_string_lossy()),
        other => other,
    };
    let ext = file.extension().map(|e| e.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
    if kind == AssetKind::Image {
        // The whole file: JPEG metadata before the size can be long.
        return match std::fs::read(file) {
            Err(e) => Some(format!("cannot read it: {e}")),
            Ok(data) => quillstone_render::image_probe::unshowable_image(&data, &ext),
        };
    }
    let mut head = Vec::new();
    let read = std::fs::File::open(file)
        .and_then(|f| io::Read::read_to_end(&mut io::Read::take(f, PROBE_BYTES as u64), &mut head));
    if let Err(e) = read {
        return Some(format!("cannot read it: {e}"));
    }
    if kind == AssetKind::Video {
        unplayable_video(&head).map(|why| {
            format!("{why} (convert with e.g. `ffmpeg -i in.mp4 -c:v libvpx-vp9 -crf 32 -b:v 0 -c:a libopus out.webm`)")
        })
    } else {
        unplayable_sound(&head, &ext)
    }
}

const DEFAULT_LIMIT: usize = 100_000;

/// What `test` prints about how far it got. A partial run never claims the story has
/// no ending: it only says none was reached yet.
fn test_summary(report: &quillstone_runtime::Report, program: &Program, script: &Path) -> Vec<String> {
    // "12" in the main script, "chapters/ch1.quill.md:12" in an included file.
    let line = |line: usize| match program.locate(line) {
        (Some(file), n) => format!("{}:{n}", game_file(script, file).display()),
        (None, n) => n.to_string(),
    };
    let lines = |lines: &[usize]| lines.iter().map(|&l| line(l)).collect::<Vec<_>>().join(", ");
    let endings = match (report.endings.as_slice(), report.partial) {
        ([], false) => "no ending".to_string(),
        ([], true) => "no ending reached yet".to_string(),
        ([l], _) => format!("1 ending (line {})", line(*l)),
        (found, _) => format!("{} endings (lines {})", found.len(), lines(found)),
    };
    if report.partial {
        vec![
            format!(
                "stopped after {} situations and {} re-run steps (the limit is spent: variables keep changing, e.g. a counter that grows in a loop, a list with many items, or many `random` rolls in one step),",
                report.situations, report.reruns
            ),
            format!("so this is only PARTIAL: the rest of the story was not explored, and there may be more endings (or none). Raise --limit to explore more (or pass --allow-partial to accept that, exit code 2 otherwise). So far: {endings}."),
        ]
    } else {
        vec![format!("played every branch: {} situations, {endings}", report.situations)]
    }
}

fn test(path: &Path, limit: usize) -> Result<Explored, String> {
    let program = load(path)?;
    let report = quillstone_runtime::explore(&program, limit);
    for line in test_summary(&report, &program, path) {
        println!("{line}");
    }
    let show = |kind: &str, f: &quillstone_runtime::Finding| {
        let at = if f.line > 0 { format!("{}: ", at(&program, path, f.line)) } else { String::new() };
        eprintln!("{at}{kind}: {}", f.message);
        match f.path.as_deref() {
            Some([]) => eprintln!("    (straight from the start, no choices needed)"),
            Some(choices) => {
                // Choices are quoted; a "(a later playthrough, ...)" note isn't.
                let quoted: Vec<String> =
                    choices.iter().map(|c| if c.starts_with('(') { c.clone() } else { format!("\"{c}\"") }).collect();
                eprintln!("    reached by choosing {}", quoted.join(" > "));
            }
            None => {}
        }
    };
    for f in &report.errors {
        show("error", f);
    }
    for f in &report.warnings {
        show("warning", f);
    }
    let explored = if report.partial { Explored::Part } else { Explored::Everything };
    match (report.errors.len(), report.warnings.len()) {
        (0, 0) => {
            println!("ok");
            Ok(explored)
        }
        (0, w) => {
            println!("{w} warning(s)");
            Ok(explored)
        }
        (e, w) => Err(format!("{e} error(s), {w} warning(s)")),
    }
}

fn package_cmd(script: &Path, flags: &[&str]) -> Result<(), String> {
    let (mut player, mut out, mut name, mut libs) = (None, None, None, None);
    let mut it = flags.iter();
    while let Some(&flag) = it.next() {
        let slot = match flag {
            "--player" => &mut player,
            "--out" => &mut out,
            "--name" => &mut name,
            "--libs" => &mut libs,
            _ => return Err(format!("unknown option `{flag}`\n\n{USAGE}")),
        };
        *slot = Some(*it.next().ok_or(format!("`{flag}` needs a value"))?);
    }
    let (Some(player), Some(out)) = (player, out) else {
        return Err(format!("package needs --player and --out\n\n{USAGE}"));
    };
    check(script).map_err(|e| format!("{e}\nfix these before packaging"))?;
    let opts = package::Options {
        script,
        player: Path::new(player),
        out: Path::new(out),
        name,
        libs: libs.map(Path::new),
    };
    package::package(&opts, &load(script)?)
}

fn dump(path: &Path) -> Result<(), String> {
    let program = load(path)?;
    if let Some(title) = &program.title {
        println!("title: {title}");
    }
    if !program.languages.is_empty() {
        let list: Vec<String> = program.languages.iter().map(|l| format!("{}={}", l.code, l.name)).collect();
        println!("languages: {}", list.join(", "));
    }
    if !program.vars.is_empty() {
        println!("vars:  {:?}", program.vars);
    }
    let mut labels_at: BTreeMap<usize, Vec<&str>> = BTreeMap::new();
    for (name, at) in &program.labels {
        labels_at.entry(*at).or_default().push(name);
    }
    println!();
    for (i, op) in program.ops.iter().enumerate() {
        for name in labels_at.get(&i).into_iter().flatten() {
            println!("{name}:");
        }
        let (file, line) = program.locate(program.lines[i]);
        match file {
            None => println!("{i:>5}  (line {line:<3})  {op:?}"),
            Some(file) => println!("{i:>5}  ({file}:{line})  {op:?}"),
        }
    }
    Ok(())
}

fn run(path: &Path) -> Result<(), String> {
    let program = load(path)?;
    let (mut session, first) = Session::start(&program);
    print_cmds(&first);

    let stdin = io::stdin();
    let mut lines = stdin.lock().lines();
    while !session.state().finished {
        let choosing = matches!(session.state().pending, Some(Cmd::ShowChoices { .. } | Cmd::ShowHotspots { .. }));
        let shopping = matches!(session.state().pending, Some(Cmd::ShowShop { .. }));
        let asking = matches!(session.state().pending, Some(Cmd::AskInput { .. }));
        let prompt = if asking {
            "type your answer ([enter]=default, !back, !quit)> "
        } else if shopping {
            "buy (number), [enter]=leave, b=back, q=quit> "
        } else if choosing {
            "choose (number, b=back, q=quit)> "
        } else {
            "[enter]=next, b=back, q=quit> "
        };
        print!("{prompt}");
        io::stdout().flush().ok();

        let Some(Ok(line)) = lines.next() else { break };
        let line = line.trim();
        // At a text entry every line is an answer, so the commands are spelled differently.
        let line = match (asking, line) {
            (true, "!quit") => "q",
            (true, "!back") => "b",
            (true, answer) => {
                let cmds = session.input(Input::Text(answer.to_string()));
                if cmds.is_empty() {
                    println!("(type something)");
                }
                print_cmds(&cmds);
                continue;
            }
            (false, other) => other,
        };
        let cmds = match line {
            "q" => break,
            "b" => match session.rollback() {
                Some(cmds) => cmds,
                None => {
                    println!("(nothing to go back to)");
                    continue;
                }
            },
            "" if shopping => session.input(Input::Advance),
            _ if shopping => match line.parse::<usize>() {
                Ok(n) if n >= 1 => session.input(Input::Buy(n - 1)),
                _ => Vec::new(),
            },
            _ if choosing => match line.parse::<usize>() {
                Ok(n) if n >= 1 => session.input(Input::Choose(n - 1)),
                _ => Vec::new(),
            },
            _ => session.input(Input::Advance),
        };
        if cmds.is_empty() {
            println!("(not valid here)");
        }
        print_cmds(&cmds);
        if cmds.contains(&Cmd::ShowInventory) {
            println!("\n  Items:");
            for item in quillstone_runtime::inventory_items(&program, &session.state().vars) {
                println!("  - {}{}", item.name, if item.text.is_empty() { String::new() } else { format!(": {}", item.text) });
            }
        }
    }
    Ok(())
}

fn print_cmds(cmds: &[Cmd]) {
    for cmd in cmds {
        match cmd {
            Cmd::ClearScene => println!("[scene cleared]"),
            Cmd::Say { speaker: Some(who), text, .. } => println!("\n{who}: {}", plain(text)),
            Cmd::Say { speaker: None, text, .. } => println!("\n  {}", plain(text)),
            Cmd::ShowImage { src, alt } => println!("[image: {src}] {alt}"),
            Cmd::SetBackground { src, .. } => println!("[background: {src}]"),
            Cmd::ShowSprite { id, src, layers, x, y, .. } => {
                let what = if layers.is_empty() {
                    src.clone()
                } else {
                    layers.iter().map(|(l, p)| format!("{l}={}", if p.is_empty() { "none" } else { p })).collect::<Vec<_>>().join(" ")
                };
                println!("[show {id}: {what} at {:.0}%,{:.0}%]", x * 100.0, y * 100.0)
            }
            Cmd::HideSprite { id, .. } => println!("[hide {id}]"),
            Cmd::MoveSprite { id, x, y, scale, .. } => println!("[move {id} to {:.0}%,{:.0}% at {scale}x]", x * 100.0, y * 100.0),
            Cmd::LoopSprite { id, x, y, scale, .. } => println!("[loop {id} between here and {:.0}%,{:.0}% at {scale}x]", x * 100.0, y * 100.0),
            Cmd::Shake { .. } => println!("[the screen shakes]"),
            Cmd::Flash { .. } => println!("[the screen flashes]"),
            Cmd::HudShow { elem, .. } => println!("[hud: {}]", elem.id),
            Cmd::HudHide { id, .. } => println!("[hud hidden: {id}]"),
            Cmd::HudClear { .. } => println!("[hud cleared]"),
            Cmd::FadeOut { time, .. } if *time > 0.0 => println!("[the screen fades out]"),
            Cmd::FadeOut { .. } => println!("[the screen is covered]"),
            Cmd::FadeIn { .. } => println!("[the screen fades in]"),
            Cmd::Textbox { .. } => {} // nothing to show in text mode
            Cmd::Particles { set: Some(p), .. } => match &p.kind {
                ParticleKind::Snow => println!("[particles: snow]"),
                ParticleKind::Rain => println!("[particles: rain]"),
                ParticleKind::Image(src) => println!("[particles: {src}]"),
            },
            Cmd::Particles { set: None, .. } => println!("[particles stop]"),
            Cmd::Tint { set: Some(t), .. } => {
                let [r, g, b] = t.color.map(|c| (c * 255.0).round() as u8);
                println!("[tint: #{r:02x}{g:02x}{b:02x}]");
            }
            Cmd::Tint { set: None, .. } => println!("[tint off]"),
            Cmd::Camera { camera, .. } => println!("[camera: {}x on {:.0}%, {:.0}%]", camera.zoom, camera.x * 100.0, camera.y * 100.0),
            Cmd::PlayMusic { src, .. } => println!("[music: {src}]"),
            Cmd::StopMusic { .. } => println!("[music stops]"),
            Cmd::PlaySfx { src, .. } => println!("[sfx: {src}]"),
            Cmd::PlaySfxLoop { src, .. } => println!("[sfx loop: {src}]"),
            Cmd::StopSfx { src: Some(src), .. } => println!("[sfx stops: {src}]"),
            Cmd::StopSfx { src: None, .. } => println!("[sfx stops]"),
            Cmd::PlayVoice { src } => println!("[voice: {src}]"),
            Cmd::Wait { seconds } => println!("[wait {seconds}s]"),
            Cmd::PlayVideo { src, .. } => println!("[video: {src}]"),
            Cmd::ShowChoices { options } => {
                println!();
                for (i, text) in options.iter().enumerate() {
                    println!("  {}. {}", i + 1, plain(text));
                }
            }
            Cmd::ShowHotspots { spots } => {
                println!();
                for (i, s) in spots.iter().enumerate() {
                    let [x, y, w, h] = s.rect.map(|v| v * 100.0);
                    println!("  {}. {}  [region {x:.0}%,{y:.0}% {w:.0}%x{h:.0}%]", i + 1, plain(&s.label));
                }
            }
            Cmd::ShowInventory => println!("\n[inventory]"),
            Cmd::ShowShop { pay, money, items } => {
                println!("\n[shop] you have {money} {pay}");
                for (i, s) in items.iter().enumerate() {
                    let state = if s.owned { "  (owned)" } else if s.price > *money { "  (too dear)" } else { "" };
                    println!("  {}. {} - {} {pay}{state}", i + 1, s.item.name, s.price);
                }
            }
            Cmd::AskInput { prompt, default, max } => {
                let default = default.as_ref().map(|d| format!(", default \"{d}\"")).unwrap_or_default();
                println!("\n[input] {} (up to {max} characters{default})", plain(prompt));
            }
            Cmd::Finished => println!("\n— the end —"),
            Cmd::Stopped { reason } => eprintln!("warning: the story was stopped: {reason}"),
            Cmd::Warning { message } => eprintln!("warning: {message}"),
            Cmd::ReplayReached { .. } | Cmd::EndReplay => {} // scene replays are for the player
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use quillstone_script::parse;

    #[test]
    fn a_hotspots_block_with_only_hidden_regions_is_flagged() {
        let blocks = |s: &str| all_hidden_hotspot_blocks(&parse(s).unwrap()).len();
        assert_eq!(blocks("# a\n::hotspots\n- [x](#a){rect=0,0,0.1,0.1, hidden}\n- [y](#a){rect=0.5,0.5,0.1,0.1, hidden}\n::\n"), 1);
        assert_eq!(blocks("# a\n::hotspots\n- [x](#a){rect=0,0,0.1,0.1, hidden}\n- [y](#a){rect=0.5,0.5,0.1,0.1}\n::\n"), 0);
        assert_eq!(blocks("# a\n::choice\n- [x](#a)\n::\n"), 0);
    }

    #[test]
    fn hotspots_fully_under_a_later_unconditional_one_are_reported() {
        let src = "# a\n::hotspots\n- [Under](#a){rect=0.1,0.1,0.1,0.1}\n- [Over](#a){rect=0,0,0.5,0.5}\n- [Apart](#a){rect=0.6,0.6,0.1,0.1}\n::\n";
        let found = hidden_hotspots(&parse(src).unwrap());
        assert_eq!(found.len(), 1);
        assert!(found[0].1.contains("\"Under\"") && found[0].1.contains("\"Over\""));
        // a condition (or `once`) on either one means it may be fine
        for fine in [
            "- [Under](#a){rect=0.1,0.1,0.1,0.1, once}\n- [Over](#a){rect=0,0,0.5,0.5}",
            "- [Under](#a){rect=0.1,0.1,0.1,0.1}\n- [Over](#a){rect=0,0,0.5,0.5, if x}",
            "- [Over](#a){rect=0,0,0.5,0.5}\n- [Small](#a){rect=0.1,0.1,0.1,0.1}", // the small one is on top
        ] {
            let p = parse(&format!("# a\n::hotspots\n{fine}\n::\n")).unwrap();
            assert!(hidden_hotspots(&p).is_empty(), "{fine}");
        }
    }

    #[test]
    fn check_warns_when_a_replay_has_no_end_replay_after_it() {
        let front = "---\nreplays: { a: \"A\", b: \"B\", c: \"C\" }\n---\n";
        let p = parse(&format!("{front}# a\nHi.\n::end_replay\n# b\nBye.\n::jump{{to=a}}\n# c\n::if{{x}}\nMaybe.\n::endif\nEnd.\n")).unwrap();
        let w = replay_warnings(&p);
        assert_eq!(w.len(), 1, "{w:?}");
        assert!(w[0].1.contains("replay `c`") && w[0].1.contains("run to the end of the game"), "{w:?}");
        assert_eq!(w[0].0, p.lines[p.labels["c"]]);
        assert!(checked("---\nreplays: { a: \"A\" }\n---\n# a\nHi.\n", &[]).is_ok(), "only a warning");
    }

    /// A script in a fresh folder, with the files `(name, bytes)` beside it.
    fn checked(script: &str, files: &[(&str, &[u8])]) -> Result<(), String> {
        static COUNTER: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let n = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("quillstone-check-test-{}-{n}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("assets")).unwrap();
        for (name, bytes) in files {
            std::fs::create_dir_all(dir.join(name).parent().unwrap()).unwrap();
            std::fs::write(dir.join(name), bytes).unwrap();
        }
        std::fs::write(dir.join("s.quill.md"), script).unwrap();
        let result = check(&dir.join("s.quill.md"));
        std::fs::remove_dir_all(&dir).unwrap();
        result
    }

    #[test]
    fn check_looks_at_item_icons_and_the_inventory_theme_images() {
        const KEY: &[u8] = include_bytes!("../../../examples/assets/gui/key.png");
        let script = "---\ninventory: bag\nvars: { bag: [] }\n---\n::item{id=key icon=assets/key.png}\nHi.\n";
        assert!(checked(script, &[("assets/key.png", KEY)]).is_ok());
        let err = checked(script, &[]).unwrap_err();
        assert!(err.contains("1 problem"), "{err}");
        let err = checked(script, &[("assets/key.png", b"not a picture")]).unwrap_err();
        assert!(err.contains("1 problem"), "{err}");
        let themed = script.replace("inventory: bag", "inventory: bag\ntheme: t.txt");
        let theme = "inventory_panel = assets/panel.png\ninventory_cell = #112233\ninventory_cell_hover = assets/hover.png\ninventory_columns = 0\n";
        let err = checked(&themed, &[("assets/key.png", KEY), ("t.txt", theme.as_bytes())]).unwrap_err();
        assert!(err.contains("3 problem"), "two missing images and a bad line: {err}");
        let theme = "inventory_panel = assets/key.png\ninventory_cell = #112233\ninventory_cell_hover = assets/key.png\n";
        assert!(checked(&themed, &[("assets/key.png", KEY), ("t.txt", theme.as_bytes())]).is_ok());
    }

    #[test]
    fn check_validates_hud_images_as_pictures_and_accepts_the_hud_demo() {
        const KEY: &[u8] = include_bytes!("../../../examples/assets/gui/key.png");
        let script = "::hud{id=k image=assets/key.png at=0.1,0.1}\nHi.\n";
        assert!(checked(script, &[("assets/key.png", KEY)]).is_ok());
        let err = checked(script, &[]).unwrap_err();
        assert!(err.contains("1 problem"), "missing: {err}");
        let err = checked(script, &[("assets/key.png", b"not a picture")]).unwrap_err();
        assert!(err.contains("1 problem"), "not a picture: {err}");
        let err = checked("::hud{id=k image=../key.png}\nHi.\n", &[]).unwrap_err();
        assert!(err.contains("inside the game folder"), "{err}");
        // a solid background needs no file
        assert!(checked("::bg{color=#102030}\nHi.\n", &[]).is_ok());
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/hud.quill.md");
        assert!(check(&path).is_ok());
    }

    #[test]
    fn check_validates_looping_sound_effects_like_other_sounds() {
        const CALM: &[u8] = include_bytes!("../../../examples/assets/calm.ogg");
        let script = "::sfx{src=assets/rain.ogg loop volume=0.5}\nHi.\n::sfx{stop=assets/rain.ogg}\n";
        assert!(checked(script, &[("assets/rain.ogg", CALM)]).is_ok());
        let err = checked(script, &[]).unwrap_err();
        assert!(err.contains("1 problem"), "a missing loop file: {err}");
        let err = checked(script, &[("assets/rain.ogg", b"definitely not a sound")]).unwrap_err();
        assert!(err.contains("1 problem"), "an unplayable loop file: {err}");
        let m4a = script.replace("rain.ogg", "rain.m4a");
        let err = checked(&m4a, &[("assets/rain.m4a", CALM)]).unwrap_err();
        assert!(err.contains("1 problem"), "AAC isn't shipped: {err}");
        // a `stop` names a file but plays nothing, so it needs no file of its own
        assert!(checked("::sfx{src=assets/rain.ogg loop}\n::sfx{stop}\nHi.\n", &[("assets/rain.ogg", CALM)]).is_ok());
    }

    #[test]
    fn check_validates_the_intro_of_a_music_track_like_its_loop() {
        const CALM: &[u8] = include_bytes!("../../../examples/assets/calm.ogg");
        let script = "::music{src=assets/loop.ogg intro=assets/intro.ogg}\nHi.\n";
        let both = [("assets/loop.ogg", CALM), ("assets/intro.ogg", CALM)];
        assert!(checked(script, &both).is_ok());
        let err = checked(script, &[("assets/loop.ogg", CALM)]).unwrap_err();
        assert!(err.contains("1 problem"), "a missing intro file: {err}");
        let err = checked(script, &[("assets/loop.ogg", CALM), ("assets/intro.ogg", b"not a sound")]).unwrap_err();
        assert!(err.contains("1 problem"), "an unplayable intro file: {err}");
        let m4a = script.replace("intro.ogg", "intro.m4a");
        let err = checked(&m4a, &[("assets/loop.ogg", CALM), ("assets/intro.m4a", CALM)]).unwrap_err();
        assert!(err.contains("1 problem"), "AAC isn't shipped: {err}");
    }

    #[test]
    fn a_paragraph_break_counts_as_a_line_in_the_long_line_estimate() {
        let words = plain("one{p}two{p}three{p}four{p}five{p}six");
        let (lines, fit) = quillstone_render::view::dialogue_lines(&quillstone_render::Theme::default(), &words, true, 38.0);
        assert_eq!(lines, 6);
        assert!(lines > fit + 1, "six short paragraphs don't fit the textbox: {fit}");
    }

    #[test]
    fn the_market_demo_passes_check() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples");
        check(&root.join("market.quill.md")).unwrap();
    }

    /// Run `f` on a fresh folder holding `files` (and an `assets/` folder).
    fn in_dir<T>(files: &[(&str, &[u8])], f: impl FnOnce(&Path) -> T) -> T {
        static COUNTER: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let n = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("quillstone-check-dir-{}-{n}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("assets")).unwrap();
        for (name, bytes) in files {
            std::fs::create_dir_all(dir.join(name).parent().unwrap()).unwrap();
            std::fs::write(dir.join(name), bytes).unwrap();
        }
        let out = f(&dir);
        std::fs::remove_dir_all(&dir).unwrap();
        out
    }

    const FONT: &[u8] = &[0, 1, 0, 0, 0, 0, 0, 0];
    const KEY: &[u8] = include_bytes!("../../../examples/assets/gui/key.png");

    fn themed(theme: &str, files: &[(&str, &[u8])]) -> Vec<String> {
        let mut files = files.to_vec();
        files.push(("t.txt", theme.as_bytes()));
        in_dir(&files, |dir| theme_problems(&dir.join("t.txt"), dir))
    }

    #[test]
    fn a_theme_font_is_checked_as_a_font_not_as_a_picture() {
        let ok = themed("font = assets/f.ttf\nfont_bold = assets/f.ttf\nfont_italic = assets/g.otf\n", &[("assets/f.ttf", FONT), ("assets/g.otf", b"OTTO....")]);
        assert!(ok.is_empty(), "{ok:?}");
        let missing = themed("font = assets/f.ttf\n", &[]);
        assert!(missing.len() == 1 && missing[0].contains("t.txt:1:") && missing[0].contains("missing theme font"), "{missing:?}");
        let fake = themed("font = assets/f.ttf\n", &[("assets/f.ttf", b"not a font at all")]);
        assert!(fake.len() == 1 && fake[0].contains("can't be used as a font"), "{fake:?}");
        let wrong = themed("font = assets/f.png\n", &[("assets/f.png", FONT)]);
        assert!(wrong.len() == 1 && wrong[0].contains(".ttf or .otf"), "{wrong:?}");
        let script = "---\ntheme: t.txt\n---\nHi.\n";
        assert!(checked(script, &[("t.txt", b"font = assets/f.ttf"), ("assets/f.ttf", FONT)]).is_ok());
        assert!(checked(script, &[("t.txt", b"font = assets/f.ttf")]).is_err());
    }

    #[test]
    fn theme_paths_outside_the_game_folder_are_rejected_in_every_form() {
        let key: &[(&str, &[u8])] = &[("assets/key.png", KEY), ("assets/f.ttf", FONT)];
        for bad in ["../x.png", "/x.png", "assets\\key.png", "C:\\x.png", "C:/x.png", "\\\\host\\share\\x.png", "//host/share/x.png", "assets/../../x.png"] {
            for line in [format!("textbox_image = {bad}"), format!("inventory_panel = {bad}"), format!("font = {bad}"), format!("name_plate_image = {bad}")] {
                let found = themed(&format!("# a comment\n{line}\n"), key);
                assert!(
                    found.len() == 1 && found[0].contains("t.txt:2:") && found[0].contains("is outside the script's folder"),
                    "{line}: {found:?}"
                );
            }
        }
        let found = themed("menu_music = ../m.ogg\n", key);
        assert!(found[0].contains("outside the script's folder"), "{found:?}");
        assert!(themed("textbox_image = ./assets/key.png\n", key).is_empty());
    }

    #[test]
    fn gallery_and_music_room_paths_outside_the_game_folder_fail_check() {
        for bad in ["../x.png", "/x.png", "assets\\x.png", "C:\\x.png", "\\\\host\\x.png"] {
            let err = checked(&format!("---\ngallery: [\"{bad}\"]\n---\n::bg{{src=assets/key.png}}\n"), &[("assets/key.png", KEY)]).unwrap_err();
            assert!(err.contains("inside the game folder") && err.contains(":2:"), "{bad}: {err}");
            let err = checked(&format!("---\nmusic_room: [\"{bad}\"]\n---\nHi.\n"), &[]).unwrap_err();
            assert!(err.contains("inside the game folder"), "{bad}: {err}");
        }
    }

    #[test]
    fn icons_and_hotspot_images_must_be_pictures_whatever_their_extension() {
        for ext in ["webm", "mp4", "ogg"] {
            let script = format!(
                "---\ninventory: bag\nvars: {{ bag: [] }}\n---\n::item{{id=k icon=assets/k.{ext}}}\n# a\n::hotspots\n- [Go](#a){{rect=0,0.5,0.5,0.5 image=assets/h.{ext}}}\n::\n"
            );
            // the parser already refuses these names; `check` must not rely on that alone
            assert!(parse(&script).is_err());
            let mut program = parse(&script.replace(&format!(".{ext}"), ".png")).unwrap();
            program.items.get_mut("k").unwrap().icon = Some(format!("assets/k.{ext}"));
            for op in &mut program.ops {
                if let Op::Choice { options, .. } = op {
                    options[0].area.as_mut().unwrap().image = Some(format!("assets/h.{ext}"));
                }
            }
            let found = in_dir(&[(&format!("assets/k.{ext}"), KEY), (&format!("assets/h.{ext}"), KEY)], |dir| {
                picture_findings(&program, &dir.join("s.quill.md"), dir)
            });
            assert_eq!(found.len(), 2, "{found:?}");
            for (error, msg) in &found {
                assert!(*error && msg.contains("must be a PNG, JPEG or WebP picture"), "{msg}");
            }
            assert!(checked(&script, &[(&format!("assets/k.{ext}"), KEY), (&format!("assets/h.{ext}"), KEY)]).is_err());
        }
    }

    #[test]
    fn hotspot_images_report_their_own_line_and_icons_come_in_declaration_order() {
        let script = "---\ninventory: bag\nvars: { bag: [] }\n---\n::item{id=zed icon=assets/z.png}\n::item{id=alpha icon=assets/a.png}\n# m\n::hotspots\n- [One](#m){rect=0,0,.5,.5 image=assets/one.png}\n\n- [Two](#m){rect=0,0,.5,.5}\n- [Three](#m){rect=0,0,.5,.5 image=assets/three.png}\n::\n";
        let program = parse(script).unwrap();
        let found = in_dir(&[], |dir| picture_findings(&program, &dir.join("s.quill.md"), dir));
        let lines: Vec<(usize, &str)> = found
            .iter()
            .map(|(_, m)| {
                let line: usize = m.split(':').nth(1).unwrap().parse().unwrap();
                (line, m.split('`').nth(1).unwrap())
            })
            .collect();
        assert_eq!(lines, [(9, "assets/one.png"), (12, "assets/three.png"), (5, "assets/z.png"), (6, "assets/a.png")]);
        assert!(found[0].1.contains("hotspot image") && found[2].1.contains("item icon"));
    }

    #[test]
    fn oversized_icons_and_hotspot_images_get_a_warning_not_an_error() {
        let script = "---\ninventory: bag\nvars: { bag: [] }\n---\n::item{id=big icon=assets/big.png}\n::item{id=ok icon=assets/ok.png}\n# m\n::hotspots\n- [One](#m){rect=0,0,.5,.5 image=assets/wide.png}\n::\n";
        let program = parse(script).unwrap();
        let files: &[(&str, &[u8])] = &[
            ("assets/big.png", &picture::fake_png(1024, 1024)),
            ("assets/ok.png", &picture::fake_png(512, 512)),
            ("assets/wide.png", &picture::fake_png(900, 100)),
        ];
        let found = in_dir(files, |dir| picture_findings(&program, &dir.join("s.quill.md"), dir));
        assert_eq!(found.len(), 2, "{found:?}");
        assert!(found.iter().all(|(error, _)| !error));
        assert!(found[0].1.contains(":9:") && found[0].1.contains("900x100"), "{}", found[0].1);
        assert!(found[1].1.contains(":5:") && found[1].1.contains("1024x1024"));
        assert!(found[1].1.contains("icons are drawn at about 100 px; a smaller file loads faster and saves memory"));
        assert!(checked(script, files).is_ok(), "warnings don't fail check");
    }

    #[test]
    fn check_looks_at_the_particle_picture_and_warns_when_it_is_big() {
        let script = "::particles{snow}\n::particles{src=assets/petal.png}\nHi.\n";
        let small: &[(&str, &[u8])] = &[("assets/petal.png", &picture::fake_png(32, 24))];
        assert!(checked(script, small).is_ok());
        let found = in_dir(small, |dir| picture_findings(&parse(script).unwrap(), &dir.join("s.quill.md"), dir));
        assert!(found.is_empty(), "{found:?}");
        let err = checked(script, &[]).unwrap_err();
        assert!(err.contains("1 problem"), "{err}");
        assert!(checked(script, &[("assets/petal.png", b"not a picture")]).is_err(), "must be a real picture");
        let big: &[(&str, &[u8])] = &[("assets/petal.png", &picture::fake_png(300, 40))];
        let found = in_dir(big, |dir| picture_findings(&parse(script).unwrap(), &dir.join("s.quill.md"), dir));
        assert_eq!(found.len(), 1, "{found:?}");
        assert!(!found[0].0 && found[0].1.contains(":2:") && found[0].1.contains("300x40") && found[0].1.contains("256 px"), "{found:?}");
        assert!(checked(script, big).is_ok(), "only a warning");
    }

    #[test]
    fn check_reports_tint_mistakes_with_their_line() {
        assert!(checked("::tint{color=#203060 alpha=0.35 fade=1}\nHi.\n::tint{none fade=1}\n", &[]).is_ok());
        let err = checked("Hi.\n::tint{color=#203060 alpha=2}\n", &[]).unwrap_err();
        assert!(err.contains(":2:") && err.contains("alpha=2"), "{err}");
        let err = checked("::tint{none color=#fff}\n", &[]).unwrap_err();
        assert!(err.contains(":1:") && err.contains("takes only"), "{err}");
    }

    #[test]
    fn check_validates_every_animation_frame_as_a_picture() {
        let script = "::anim{id=blink frames=\"assets/o.png 3, assets/c.png 0.1\" loop}\n::show{a anim=blink}\nHi.\n";
        assert!(checked(script, &[("assets/o.png", KEY), ("assets/c.png", KEY)]).is_ok());
        let err = checked(script, &[("assets/o.png", KEY)]).unwrap_err();
        assert!(err.contains("1 problem"), "a frame is missing: {err}");
        let err = checked(script, &[("assets/o.png", KEY), ("assets/c.png", b"not a picture")]).unwrap_err();
        assert!(err.contains("1 problem"), "a frame is no picture: {err}");
        // Frames the story never shows are checked too: they are declared, so they ship.
        let unused = "::anim{id=blink frames=\"assets/o.png 3\"}\nHi.\n";
        assert!(checked(unused, &[]).unwrap_err().contains("1 problem"));
        // The findings name the declaration's line and the frame.
        let program = parse(&format!("Hi.\n{script}")).unwrap();
        let found = in_dir(&[("assets/o.png", KEY), ("assets/c.png", b"nope")], |dir| picture_findings(&program, &dir.join("s.quill.md"), dir));
        assert_eq!(found.len(), 1, "{found:?}");
        assert!(found[0].0 && found[0].1.contains(":2:") && found[0].1.contains("`assets/c.png` won't show in a shipped game"), "{}", found[0].1);
        let err = checked("::anim{id=a frames=\"assets/x.webm 1\"}\nHi.\n", &[]);
        assert!(err.is_err(), "the parser refuses non-pictures before check looks");
    }

    #[test]
    fn long_animations_and_huge_frames_get_warnings_not_errors() {
        let many: Vec<String> = (0..65).map(|_| "assets/s.png 0.1".to_string()).collect();
        let script = format!("::anim{{id=many frames=\"{}\"}}\n::anim{{id=fine frames=\"assets/s.png 1, assets/t.png 1\"}}\nHi.\n", many.join(", "));
        let program = parse(&script).unwrap();
        let files: &[(&str, &[u8])] = &[("assets/s.png", &picture::fake_png(100, 100)), ("assets/t.png", &picture::fake_png(2048, 1080))];
        let found = in_dir(files, |dir| picture_findings(&program, &dir.join("s.quill.md"), dir));
        assert_eq!(found.len(), 1, "{found:?}");
        assert!(!found[0].0 && found[0].1.contains(":1:") && found[0].1.contains("65 frames") && found[0].1.contains("`many`"), "{}", found[0].1);
        let exactly: Vec<String> = (0..64).map(|_| "assets/s.png 0.1".to_string()).collect();
        let ok = parse(&format!("::anim{{id=many frames=\"{}\"}}\nHi.\n", exactly.join(", "))).unwrap();
        assert!(in_dir(files, |dir| picture_findings(&ok, &dir.join("s.quill.md"), dir)).is_empty(), "64 is allowed");
        // A frame past 2048 px on a side warns, at the declaration's line.
        let huge = "Hi.\n::anim{id=h frames=\"assets/s.png 1, assets/h.png 1\"}\n";
        let program = parse(huge).unwrap();
        let files: &[(&str, &[u8])] = &[("assets/s.png", &picture::fake_png(100, 100)), ("assets/h.png", &picture::fake_png(2049, 100))];
        let found = in_dir(files, |dir| picture_findings(&program, &dir.join("s.quill.md"), dir));
        assert_eq!(found.len(), 1);
        assert!(!found[0].0 && found[0].1.contains(":2:") && found[0].1.contains("2049x100") && found[0].1.contains("assets/h.png"), "{}", found[0].1);
        assert!(checked(huge, files).is_ok(), "warnings don't fail check");
        assert!(checked(&script, &[("assets/s.png", &picture::fake_png(100, 100)), ("assets/t.png", &picture::fake_png(2048, 1080))]).is_ok());
    }

    #[test]
    fn the_animation_demo_passes_check() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/anim.quill.md");
        assert!(check(&path).is_ok());
    }

    #[test]
    fn a_partial_test_run_exits_2_unless_allowed() {
        assert_eq!(test_exit_code(Explored::Everything, false), 0);
        assert_eq!(test_exit_code(Explored::Part, false), 2);
        assert_eq!(test_exit_code(Explored::Part, true), 0);
        assert_eq!(test_flags(&[]), Ok((DEFAULT_LIMIT, false)));
        assert_eq!(test_flags(&["--allow-partial", "--limit", "50"]), Ok((50, true)));
        assert!(test_flags(&["--limit", "0"]).is_err() && test_flags(&["--limit"]).is_err() && test_flags(&["--bogus"]).is_err());
        // end to end through `test`: a counter that grows forever stops at the limit
        let outcome = in_dir(&[("s.quill.md", b"# a\n::set{n+=1}\nLine {n}.\n::jump{to=a}\n")], |dir| test(&dir.join("s.quill.md"), 50));
        assert_eq!(outcome, Ok(Explored::Part));
        let done = in_dir(&[("s.quill.md", b"Hi.\n")], |dir| test(&dir.join("s.quill.md"), 50));
        assert_eq!(done, Ok(Explored::Everything));
    }

    #[test]
    fn item_descriptions_and_choice_texts_too_long_for_their_boxes_are_warned_about() {
        let long = "word ".repeat(300);
        let script = format!(
            "---\ninventory: bag\nvars: {{ bag: [] }}\n---\n::item{{id=a text=\"short\"}}\n::item{{id=b text=\"{long}\"}}\n::choice\n- [Fine](#x)\n- [{long}](#x)\n::\n# x\nHi.\n"
        );
        let program = parse(&script).unwrap();
        let found = line_warnings(&program, Path::new("."));
        let at: Vec<usize> = found.iter().map(|(l, _)| *l).collect();
        assert_eq!(at, [9, 6], "the choice option's line, then the item's declaration line: {found:?}");
        assert!(found[0].1.contains("button") && found[1].1.contains("description of `b`"));
    }

    #[test]
    fn check_warns_when_a_timed_choice_is_too_quick_to_read_and_refuses_a_badly_marked_one() {
        let script = |attrs: &str, default: &str| format!("# a\n::choice{attrs}\n- [Go](#a)\n- [Stay](#a){default}\n::\n");
        let warns = |src: &str| line_warnings(&parse(src).unwrap(), Path::new("."));
        let quick = warns(&script("{time=1.5}", "{default}"));
        assert!(quick.len() == 1 && quick[0].0 == 2 && quick[0].1.contains("1.5 s"), "{quick:?}");
        assert!(warns(&script("{time=2}", "{default}")).is_empty() && warns(&script("", "")).is_empty());
        assert!(checked(&script("{time=1.5}", "{default}"), &[]).is_ok(), "a warning only");
        for bad in [script("{time=5}", ""), script("", "{default}"), script("{time=0}", "{default}")] {
            assert!(checked(&bad, &[]).is_err(), "{bad}");
        }
    }

    fn quick_warnings(theme: &str, files: &[(&str, &[u8])]) -> Vec<String> {
        let mut files = files.to_vec();
        files.push(("t.txt", theme.as_bytes()));
        in_dir(&files, |dir| quick_menu_warnings(&dir.join("t.txt"), dir))
    }

    #[test]
    fn check_errors_on_missing_or_unsupported_quick_menu_pictures() {
        let ok = themed("quick_back_icon = assets/key.png\nquick_back_hover = assets/key.png\nquick_skip_on = assets/key.png\n\
                         quick_skip_icon = assets/key.png\n", &[("assets/key.png", KEY)]);
        assert!(ok.is_empty(), "{ok:?}");
        let missing = themed("quick_back_icon = assets/key.png\nquick_back_hover = assets/nope.png\n", &[("assets/key.png", KEY)]);
        assert!(missing.len() == 1 && missing[0].contains("t.txt:2:") && missing[0].contains("missing theme picture `assets/nope.png`"), "{missing:?}");
        let gif = themed("quick_load_icon = assets/a.gif\n", &[("assets/a.gif", b"GIF89a")]);
        assert!(gif.len() == 1 && gif[0].contains("a.gif") && gif[0].contains("PNG, JPEG or WebP"), "{gif:?}");
        let fake = themed("quick_load_icon = assets/a.png\n", &[("assets/a.png", b"not a picture")]);
        assert!(fake.len() == 1 && fake[0].contains("a.png"), "{fake:?}");
        let outside = themed("quick_load_icon = ../a.png\n", &[]);
        assert!(outside.len() == 1 && outside[0].contains("outside"), "{outside:?}");
        let bad = themed("quick_nothing_icon = a.png\nquick_back_size = 3, 3\n", &[]);
        assert_eq!(bad.len(), 2, "{bad:?}");
        let script = "---\ntheme: t.txt\n---\nHi.\n";
        assert!(checked(script, &[("t.txt", b"quick_back_icon = assets/b.png")]).is_err());
        assert!(checked(script, &[("t.txt", b"quick_back_icon = assets/key.png"), ("assets/key.png", KEY)]).is_ok());
    }

    #[test]
    fn check_warns_about_overlapping_and_off_screen_quick_menu_buttons_but_not_about_a_clean_theme() {
        let key = [("assets/key.png", KEY)];
        let (w, h) = picture::read_picture(Path::new("../../examples/assets/gui/key.png")).unwrap();
        assert!(w >= 16 && h >= 16 && w < 400, "the fixture is a button-sized picture: {w}x{h}");
        let clean = quick_warnings("quick_back_icon = assets/key.png\nquick_back_pos = 20, 20\nquick_skip_icon = assets/key.png\n\
                                    quick_settings_icon = assets/key.png\nquick_settings_pos = 200, 20\n", &key);
        assert!(clean.is_empty(), "{clean:?}");
        assert!(quick_warnings("text_size = 40\n", &[]).is_empty() && quick_warnings("quick_menu = hover\n", &[]).is_empty());
        let both = quick_warnings("quick_back_icon = assets/key.png\nquick_back_pos = 20, 20\nquick_history_icon = assets/key.png\n\
                                   quick_history_pos = 25, 25\n", &key);
        assert!(both.len() == 1 && both[0].contains("t.txt:4:") && both[0].contains("`back` and `history` overlap"), "{both:?}");
        assert!(both[0].contains("`history` is on top"), "{both:?}");
        let off = quick_warnings("quick_back_icon = assets/key.png\nquick_back_pos = 1910, 1075\n", &key);
        assert!(off.len() == 1 && off[0].contains("t.txt:2:") && off[0].contains("past the 1920x1080 screen"), "{off:?}");
        // a text button placed over the row's buttons
        let row = quick_warnings("quick_back_pos = 1700, 1050\nquick_back_size = 100, 40\n", &[]);
        assert!(row.iter().any(|m| m.contains("`back` and")), "a text button placed on the row's buttons: {row:?}");
    }

    #[test]
    fn check_validates_the_name_plate_picture_like_the_textbox_picture() {
        let ok = themed("textbox_image = assets/key.png\nname_plate_image = assets/key.png\n", &[("assets/key.png", KEY)]);
        assert!(ok.is_empty(), "{ok:?}");
        let missing = themed("textbox_rect = 100, 500, 1200, 400\nname_plate_image = assets/plate.png\n", &[]);
        assert!(missing.len() == 1 && missing[0].contains("t.txt:2:") && missing[0].contains("missing theme picture `assets/plate.png`"), "{missing:?}");
        let fake = themed("name_plate_image = assets/plate.png\n", &[("assets/plate.png", b"not a picture")]);
        assert!(fake.len() == 1 && fake[0].contains("plate.png"), "{fake:?}");
        // a bad textbox line is reported with its line number like any theme line
        let bad = themed("text_size = 40\ntextbox_rect = 0, 0, 100, 100\n", &[]);
        assert!(bad.len() == 1 && bad[0].contains("line 2:") && bad[0].contains("textbox_rect"), "{bad:?}");
        let script = "---\ntheme: t.txt\n---\nHi.\n";
        assert!(checked(script, &[("t.txt", b"name_plate_image = assets/plate.png")]).is_err());
        assert!(checked(script, &[("t.txt", b"name_plate_image = assets/key.png"), ("assets/key.png", KEY)]).is_ok());
    }

    #[test]
    fn the_long_line_warning_uses_the_themes_textbox_and_padding() {
        let line = "word ".repeat(40); // 200 characters
        let script = format!("---\ntheme: t.txt\n---\n**A:** {line}\n");
        let warns = |theme: &str| {
            in_dir(&[("t.txt", theme.as_bytes()), ("s.quill.md", script.as_bytes())], |dir| {
                line_warnings(&parse(&script).unwrap(), dir)
            })
        };
        assert!(warns("").is_empty(), "fits the default textbox");
        assert!(warns("textbox_rect = 100, 700, 1200, 300\n").is_empty(), "a smaller but tall enough box still fits");
        let small = warns("textbox_rect = 700, 700, 500, 200\ntextbox_padding = 20\n");
        assert_eq!(small.len(), 1, "{small:?}");
        let padded = warns("textbox_rect = 160, 780, 800, 260\ntextbox_padding = 40, 36, 300, 20\n");
        assert_eq!(padded.len(), 1, "padding narrows the text area: {padded:?}");
    }

    #[test]
    fn check_looks_at_every_file_a_picture_pattern_can_build() {
        let png = picture::fake_png(10, 10);
        let script = "---\nvars: { place: \"a\" }\n---\n::bg{src=\"assets/places/{place}.png\"}\nHi.\n";
        assert!(checked(script, &[]).unwrap_err().contains("1 problem"), "no match is an error");
        assert!(checked(script, &[("assets/places/a.png", &png)]).is_ok());
        let found = in_dir(&[], |dir| check_pattern(&dir.join("s.quill.md"), dir, 4, "assets/places/{place}.png", &mut HashSet::new()));
        assert_eq!(found.len(), 1);
        assert!(found[0].0 && found[0].1.contains(":4:") && found[0].1.contains("`assets/places/{place}.png` matches no file"), "{}", found[0].1);
        let files: &[(&str, &[u8])] = &[("assets/places/a.png", &png), ("assets/places/b.png", &png)];
        let ok = in_dir(files, |dir| check_pattern(&dir.join("s.quill.md"), dir, 4, "assets/places/{place}.png", &mut HashSet::new()));
        assert!(ok.is_empty(), "{ok:?}");
        // Every match must be a picture: one bad file is one error naming it.
        let files: &[(&str, &[u8])] = &[("assets/places/a.png", &png), ("assets/places/b.png", b"not a picture")];
        let bad = in_dir(files, |dir| check_pattern(&dir.join("s.quill.md"), dir, 4, "assets/places/{place}.png", &mut HashSet::new()));
        assert_eq!(bad.len(), 1, "{bad:?}");
        assert!(bad[0].0 && bad[0].1.contains("assets/places/b.png") && bad[0].1.contains("won't show"), "{}", bad[0].1);
        // A file another pattern already checked isn't read twice.
        let mut seen = HashSet::new();
        in_dir(files, |dir| {
            check_pattern(&dir.join("s.quill.md"), dir, 4, "assets/places/{p}.png", &mut seen);
            assert!(check_pattern(&dir.join("s.quill.md"), dir, 5, "assets/places/{q}.png", &mut seen).is_empty());
        });
    }

    const LAYERS: &str = "---\nvars: { mood: \"smile\" }\n---\n::anim{id=blink frames=\"a/open.png 3, a/closed.png 0.1\" loop}\n\
        ::character{id=mika layers=body,eyes,mouth layer_sprites=a/{layer}_{value}.png}\n\
        ::show{mika body=casual eyes=anim:blink mouth={mood}}\nHi.\n::show{mika eyes=none}\nBye.\n";

    #[test]
    fn check_looks_at_every_layer_file_and_the_files_a_layer_pattern_can_build() {
        let png = picture::fake_png(10, 20);
        let all: &[(&str, &[u8])] =
            &[("a/body_casual.png", &png), ("a/open.png", &png), ("a/closed.png", &png), ("a/mouth_smile.png", &png), ("a/mouth_open.png", &png)];
        assert!(checked(LAYERS, all).is_ok());
        // a missing literal layer file, and a layer pattern that builds nothing
        let e = checked(LAYERS, &all[1..]).unwrap_err();
        assert!(e.contains("1 problem"), "{e}");
        let e = checked(LAYERS, &all[..3]).unwrap_err();
        assert!(e.contains("1 problem"), "{e}");
        // a file a pattern builds that is not a picture is an error too
        let mut bad = all.to_vec();
        bad[4] = ("a/mouth_open.png", b"not a picture");
        assert!(checked(LAYERS, &bad).is_err());
        // a layer file that is a video is not a picture: the character's template must end in a picture type
        assert!(parse("::character{id=m layers=a layer_sprites=x/{layer}_{value}.webm}\n").is_err());
    }

    #[test]
    fn layers_of_different_sizes_are_a_warning_not_an_error() {
        let (small, big) = (picture::fake_png(10, 20), picture::fake_png(40, 80));
        let found = |files: &[(&str, &[u8])]| in_dir(files, |dir| layer_size_findings(&parse(LAYERS).unwrap(), &dir.join("s.quill.md"), dir));
        let same: &[(&str, &[u8])] =
            &[("a/body_casual.png", &small), ("a/open.png", &small), ("a/closed.png", &small), ("a/mouth_smile.png", &small), ("a/mouth_open.png", &small)];
        assert!(found(same).is_empty());
        // one animation frame and one file a pattern builds differ from the first picture
        let mixed: &[(&str, &[u8])] =
            &[("a/body_casual.png", &small), ("a/open.png", &small), ("a/closed.png", &big), ("a/mouth_smile.png", &small), ("a/mouth_open.png", &big)];
        let w = found(mixed);
        assert_eq!(w.len(), 2, "{w:?}");
        assert!(w.iter().all(|(error, _)| !error), "warnings only");
        assert!(w[0].1.contains("a/closed.png") && w[0].1.contains("40x80") && w[0].1.contains("a/body_casual.png"), "{}", w[0].1);
        assert!(w.iter().any(|(_, m)| m.contains("a/mouth_open.png")), "{w:?}");
        // other characters' pictures are not compared with these
        let p = parse("::character{id=a layers=x layer_sprites=q/{layer}_{value}.png}\n::character{id=b layers=x layer_sprites=r/{layer}_{value}.png}\n::show{a x=1}\n::show{b x=1}\n").unwrap();
        let files: &[(&str, &[u8])] = &[("q/x_1.png", &small), ("r/x_1.png", &big)];
        assert!(in_dir(files, |dir| layer_size_findings(&p, &dir.join("s.quill.md"), dir)).is_empty());
    }

    #[test]
    fn a_pattern_matching_many_files_gets_a_warning_and_exactly_200_does_not() {
        let png = picture::fake_png(4, 4);
        let names: Vec<String> = (0..201).map(|i| format!("assets/p/f{i}.png")).collect();
        let files: Vec<(&str, &[u8])> = names.iter().map(|n| (n.as_str(), png.as_slice())).collect();
        let many = in_dir(&files, |dir| check_pattern(&dir.join("s.quill.md"), dir, 1, "assets/p/{x}.png", &mut HashSet::new()));
        assert_eq!(many.len(), 1, "{many:?}");
        assert!(!many[0].0 && many[0].1.contains("matches 201 files") && many[0].1.contains("all of them ship"), "{}", many[0].1);
        let exactly = in_dir(&files[..200], |dir| check_pattern(&dir.join("s.quill.md"), dir, 1, "assets/p/{x}.png", &mut HashSet::new()));
        assert!(exactly.is_empty(), "{exactly:?}");
    }

    #[test]
    fn a_gallery_picture_can_be_unlocked_by_a_pattern() {
        let png = picture::fake_png(10, 10);
        let script = "---\nvars: { place: \"a\" }\ngallery: [assets/places/a.png]\n---\n::bg{src=\"assets/places/{place}.png\"}\nHi.\n";
        let ok = in_dir(&[("assets/places/a.png", &png)], |dir| check_extras(&parse(script).unwrap(), &dir.join("s.quill.md"), dir));
        assert_eq!(ok, 0);
    }

    #[test]
    fn gallery_videos_are_probed_like_background_videos() {
        const VP9: &[u8] = include_bytes!("../../../player/tests/data/bars-vp9.webm");
        const AV1: &[u8] = include_bytes!("../../../player/tests/data/bars-av1.webm");
        let png = picture::fake_png(10, 10);
        let script = |entry: &str, story: &str| format!("---\ngallery: [assets/a.png, {entry}]\n---\n::bg{{src=assets/a.png}}\n{story}\nHi.\n");
        let run = |entry: &str, story: &str, files: &[(&str, &[u8])]| {
            let mut files = files.to_vec();
            files.push(("assets/a.png", &png));
            in_dir(&files, |dir| check_extras(&parse(&script(entry, story)).unwrap(), &dir.join("s.quill.md"), dir))
        };
        let ok: &[(&str, &[u8])] = &[("assets/c.webm", VP9)];
        assert_eq!(run("assets/c.webm", "::bg{src=assets/c.webm}", ok), 0, "shown as a looped background");
        assert_eq!(run("assets/c.webm", "::video{src=assets/c.webm}", ok), 0, "or as a cutscene");
        assert_eq!(run("assets/c.webm", "", ok), 1, "a clip the story never shows can never unlock");
        assert_eq!(run("assets/c.webm", "::bg{src=assets/c.webm}", &[]), 1, "missing");
        assert_eq!(run("assets/c.webm", "::bg{src=assets/c.webm}", &[("assets/c.webm", AV1)]), 1, "AV1 is not shipped");
        assert_eq!(run("assets/c.ogg", "", &[("assets/c.ogg", b"OggS")]), 1, "a sound is not a gallery entry");
    }

    #[test]
    fn a_gallery_picture_shown_only_as_a_background_animation_frame_can_unlock() {
        let png = picture::fake_png(10, 10);
        let files: &[(&str, &[u8])] = &[("assets/r1.png", &png), ("assets/r2.png", &png)];
        let script = |bg: &str| format!("---\ngallery: [assets/r1.png]\n---\n::anim{{id=rain frames=\"assets/r1.png 1, assets/r2.png 1\" loop}}\n{bg}\nHi.\n");
        let errors = |bg: &str| in_dir(files, |dir| check_extras(&parse(&script(bg)).unwrap(), &dir.join("s.quill.md"), dir));
        assert_eq!(errors("::bg{anim=rain}"), 0);
        assert_eq!(errors("::bg{color=#000000}"), 1, "nothing shows it");
    }

    #[test]
    fn the_places_demo_passes_check() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/places.quill.md");
        assert!(check(&path).is_ok());
    }

    #[test]
    fn a_partial_summary_never_claims_there_is_no_ending() {
        let src = "---\nvars: { a: 0, b: 0, c: 0, d: 0, e: 0 }\n---\n# loop\n::set{a=random(1,6), b=random(1,6), c=random(1,6), d=random(1,6), e=random(1,6)}\n::choice\n- [Again](#loop)\n- [Stop](#fin)\n::\n# fin\n::end\n";
        let report = quillstone_runtime::explore(&parse(src).unwrap(), 40);
        assert!(report.partial);
        let text = test_summary(&report, &parse(src).unwrap(), Path::new("s.quill.md")).join("\n");
        assert!(text.contains("PARTIAL") && !text.contains("Found no ending") && !text.ends_with("no ending."), "{text}");
        let done = quillstone_runtime::explore(&parse("Hi.\n").unwrap(), 40);
        assert!(test_summary(&done, &parse("Hi.\n").unwrap(), Path::new("s.quill.md"))[0].starts_with("played every branch"));
    }

    // ---- `::include` ---------------------------------------------------------

    const KEY_PNG: &[u8] = include_bytes!("../../../examples/assets/gui/key.png");

    fn text(s: &str) -> &[u8] {
        s.as_bytes()
    }

    #[test]
    fn check_finds_an_included_files_assets_and_errors_name_it() {
        let main = "::include{ch/a.quill.md}\n";
        let (ok, bad) = (
            in_dir(&[("g.quill.md", text(main)), ("ch/a.quill.md", text("::bg{src=assets/k.png}\nHi.\n")), ("assets/k.png", KEY_PNG)], |d| check(&d.join("g.quill.md"))),
            in_dir(&[("g.quill.md", text(main)), ("ch/a.quill.md", text("Hi.\n::bg{src=assets/k.png}\n"))], |d| check(&d.join("g.quill.md"))),
        );
        assert!(ok.is_ok(), "{ok:?}");
        assert!(bad.unwrap_err().starts_with("1 problem"));
        // where it is reported
        in_dir(&[("g.quill.md", text(main)), ("ch/a.quill.md", text("Hi.\n::bg{src=assets/k.png}\n"))], |d| {
            let script = d.join("g.quill.md");
            let program = load(&script).unwrap();
            let line = program.assets_with_lines().next().unwrap().0;
            assert_eq!(at(&program, &script, line), format!("{}:2", d.join("ch/a.quill.md").display()));
            assert_eq!(at(&program, &script, 1), format!("{}:1", script.display()));
        });
    }

    #[test]
    fn a_parse_error_in_an_included_file_names_the_file_and_line() {
        in_dir(&[("g.quill.md", text("::include{ch/a.quill.md}\n")), ("ch/a.quill.md", text("Hi.\n\n::bogus\n"))], |d| {
            let e = load(&d.join("g.quill.md")).unwrap_err();
            assert!(e.starts_with(&format!("{}:3: error: ", d.join("ch/a.quill.md").display())), "{e}");
            assert!(check(&d.join("g.quill.md")).unwrap_err().contains("ch/a.quill.md:3"));
        });
    }

    #[test]
    fn test_findings_and_endings_in_an_included_file_say_which_file() {
        let files = [("g.quill.md", text("# a\n::jump{to=z}\n::include{ch/a.quill.md}\n")), ("ch/a.quill.md", text("# lost\nNever.\n# z\nEnd.\n"))];
        in_dir(&files, |d| {
            let script = d.join("g.quill.md");
            let program = load(&script).unwrap();
            let report = quillstone_runtime::explore(&program, 1000);
            let lost = report.warnings.iter().find(|f| f.message.contains("lost") || f.message.contains("Never")).expect("unreached part");
            assert_eq!(at(&program, &script, lost.line), format!("{}:2", d.join("ch/a.quill.md").display()));
            let summary = test_summary(&report, &program, &script).join("\n");
            assert!(summary.contains(&format!("1 ending (line {}:", d.join("ch/a.quill.md").display())), "{summary}");
            assert!(test(&script, 1000).is_ok());
        });
    }

    #[test]
    fn dump_and_the_examples_work_with_includes() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/chapters.quill.md");
        assert!(check(&path).is_ok());
        assert!(test(&path, 1000).is_ok());
        assert!(dump(&path).is_ok());
    }
}
