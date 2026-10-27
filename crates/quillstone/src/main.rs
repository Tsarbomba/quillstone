//! Quillstone command line: `check`, `test`, `voices`, `dump`, a text-mode `run`, and `package`.
//! The windowed player is its own crate, in `player/`.

use std::collections::BTreeMap;
use std::io::{self, BufRead, Write};
use std::path::Path;
use std::process::ExitCode;

use quillstone_runtime::{Cmd, Input, Session};
use quillstone_script::markup::plain;
use quillstone_script::{kind_from_path, parse, AssetKind, Op, Program};

mod package;

const USAGE: &str = "\
Quillstone

USAGE:
    quillstone check <script>   validate a script and that its assets exist
    quillstone test  <script> [--limit <n>]
                                play every branch: find endless loops, places the story
                                can't end from, unreachable parts, options never offered
                                and variables never set (explores up to n situations, 100000)
    quillstone voices <script>  list every line's auto-voice file name, speaker and text
                                (tab-separated, for a spreadsheet: the recording script)
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
        ["test", file] => test(Path::new(file), DEFAULT_LIMIT),
        ["test", file, "--limit", n] => match n.parse() {
            Ok(n) if n > 0 => test(Path::new(file), n),
            _ => Err(format!("--limit needs a positive number, not `{n}`")),
        },
        ["dump", file] => dump(Path::new(file)),
        ["voices", file] => voices(Path::new(file)),
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

fn load(path: &Path) -> Result<Program, String> {
    let src = std::fs::read_to_string(path)
        .map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    parse(&src).map_err(|errors| {
        errors
            .iter()
            .map(|e| format!("{}:{}: error: {}", path.display(), e.line, e.message))
            .collect::<Vec<_>>()
            .join("\n")
    })
}

fn check(path: &Path) -> Result<(), String> {
    let program = load(path)?;
    let root = path.parent().unwrap_or_else(|| Path::new("."));

    let mut missing = 0;
    for (idx, kind, asset) in program.assets() {
        let file = root.join(asset);
        if !quillstone_script::is_inside(asset) {
            // Saves refuse such paths (a shared save must not reach outside the game), so the game would break.
            missing += 1;
            let at = program.lines[idx];
            eprintln!("{}:{at}: error: `{asset}` is outside the script's folder (keep assets beside or below it)", path.display());
        } else if !file.exists() {
            missing += 1;
            eprintln!("{}:{}: error: missing asset `{asset}`", path.display(), program.lines[idx]);
        } else if let Some(why) = unplayable(&file, kind) {
            missing += 1;
            eprintln!("{}:{}: error: `{asset}` won't play in a shipped game: {why}", path.display(), program.lines[idx]);
        }
    }
    for (line, icon) in program.item_icons() {
        let file = root.join(icon);
        if !quillstone_script::is_inside(icon) {
            missing += 1;
            eprintln!("{}:{line}: error: `{icon}` is outside the script's folder (keep assets beside or below it)", path.display());
        } else if !file.exists() {
            missing += 1;
            eprintln!("{}:{line}: error: missing item icon `{icon}`", path.display());
        } else if let Some(why) = unplayable(&file, AssetKind::Image) {
            missing += 1;
            eprintln!("{}:{line}: error: `{icon}` won't show in a shipped game: {why}", path.display());
        }
    }
    for (pc, text) in program.unknown_items() {
        eprintln!(
            "{}:{}: warning: \"{text}\" is not a declared `::item` (it will show in the inventory under that name, with no picture; a typo?)",
            path.display(),
            program.lines[pc]
        );
    }
    if let Some(theme_path) = &program.theme {
        missing += check_theme(&root.join(theme_path), root);
    }
    missing += check_extras(&program, path, root);
    for (pc, msg) in hidden_hotspots(&program) {
        eprintln!("{}:{}: warning: {msg}", path.display(), program.lines[pc]);
    }
    for pc in all_hidden_hotspot_blocks(&program) {
        eprintln!(
            "{}:{}: warning: every region of this `::hotspots` block is `hidden`: players using only keyboard or gamepad can't pick anything here; add a visible region such as an exit",
            path.display(),
            program.lines[pc]
        );
    }
    check_line_lengths(&program, path, root);
    let voiced = check_voices(&program, path, root, &mut missing);
    if missing > 0 {
        return Err(format!("{missing} problem(s) (paths are relative to the script's folder)"));
    }
    println!("ok: {} ops, {} labels{voiced}", program.ops.len(), program.labels.len());
    Ok(())
}

/// Ops of `::hotspots` blocks whose regions are all `hidden`.
fn all_hidden_hotspot_blocks(program: &Program) -> Vec<usize> {
    let mut out = Vec::new();
    for (pc, op) in program.ops.iter().enumerate() {
        let Op::Choice { options } = op else { continue };
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
        let Op::Choice { options } = op else { continue };
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

/// Warn about lines clearly too long for the textbox. The player shrinks text that
/// doesn't fit, so this is a warning; the estimate errs long, so only clear cases count.
fn check_line_lengths(program: &Program, path: &Path, root: &Path) {
    let theme = program
        .theme
        .as_ref()
        .and_then(|t| std::fs::read_to_string(root.join(t)).ok())
        .map(|text| quillstone_render::Theme::parse(&text).0)
        .unwrap_or_default();
    for (pc, op) in program.ops.iter().enumerate() {
        let Op::Say { speaker, text, .. } = op else { continue };
        let words = plain(text);
        let (lines, fit) = quillstone_render::view::dialogue_lines(&words, speaker.is_some(), theme.text_size);
        if lines > fit + 1 {
            eprintln!(
                "{}:{}: warning: this line takes about {lines} lines and the textbox fits {fit}; it will be shown \
                 smaller, or cut off if very long (split it into two lines?)",
                path.display(),
                program.lines[pc]
            );
        }
    }
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
    let shown = |src: &str| program.ops.iter().any(|op| matches!(op, Op::Bg { src: s, .. } | Op::Image { src: s, .. } if s == src));
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
        } else if in_gallery && kind != AssetKind::Image {
            problem(format!("`{asset}` in the gallery isn't a picture"));
        } else if let Some(why) = unplayable(&file, kind) {
            problem(format!("`{asset}` won't play in a shipped game: {why}"));
        } else if in_gallery && !shown(asset) {
            problem(format!("`{asset}` is in the gallery, but no `::bg` or image shows it, so it can never unlock"));
        } else if !in_gallery && !played(asset) {
            problem(format!("`{asset}` is in the music room, but no `::music` plays it, so it can never unlock"));
        }
    }
    count
}

/// Check a theme file and the files it names; returns how many problems were found.
fn check_theme(file: &Path, root: &Path) -> usize {
    let text = match std::fs::read_to_string(file) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("{}: error: cannot read the theme: {e}", file.display());
            return 1;
        }
    };
    let (theme, problems) = quillstone_render::Theme::parse(&text);
    for p in &problems {
        eprintln!("{}: error: {p}", file.display());
    }
    let mut count = problems.len();
    for asset in theme.assets() {
        let kind = if theme.menu_music.as_deref() == Some(asset) { AssetKind::Audio } else { AssetKind::Image };
        if !root.join(asset).exists() {
            count += 1;
            eprintln!("{}: error: missing theme asset `{asset}`", file.display());
        } else if let Some(why) = unplayable(&root.join(asset), kind) {
            count += 1;
            eprintln!("{}: error: `{asset}` won't play in a shipped game: {why}", file.display());
        }
    }
    count
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

fn test(path: &Path, limit: usize) -> Result<(), String> {
    let program = load(path)?;
    let report = quillstone_runtime::explore(&program, limit);
    let endings = match report.endings.as_slice() {
        [] => "no ending".to_string(),
        [line] => format!("1 ending (line {line})"),
        lines => format!(
            "{} endings (lines {})",
            lines.len(),
            lines.iter().map(usize::to_string).collect::<Vec<_>>().join(", ")
        ),
    };
    if report.partial {
        println!("stopped after {} situations (variables keep changing, e.g. a counter that grows in a loop, or a list with many items),", report.situations);
        println!("so this is only part of the story; raise --limit to explore more. Found {endings}.");
    } else {
        println!("played every branch: {} situations, {endings}", report.situations);
    }
    let show = |kind: &str, f: &quillstone_runtime::Finding| {
        let at = if f.line > 0 { format!("{}:{}: ", path.display(), f.line) } else { String::new() };
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
    match (report.errors.len(), report.warnings.len()) {
        (0, 0) => {
            println!("ok");
            Ok(())
        }
        (0, w) => {
            println!("{w} warning(s)");
            Ok(())
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
        println!("{i:>5}  (line {:<3})  {op:?}", program.lines[i]);
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
        let prompt = if shopping {
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
            Cmd::ShowSprite { id, src, x, .. } => println!("[show {id}: {src} at {:.0}%]", x * 100.0),
            Cmd::HideSprite { id, .. } => println!("[hide {id}]"),
            Cmd::MoveSprite { id, x, scale, .. } => println!("[move {id} to {:.0}% at {scale}x]", x * 100.0),
            Cmd::Shake { .. } => println!("[the screen shakes]"),
            Cmd::Camera { camera, .. } => println!("[camera: {}x on {:.0}%, {:.0}%]", camera.zoom, camera.x * 100.0, camera.y * 100.0),
            Cmd::PlayMusic { src, .. } => println!("[music: {src}]"),
            Cmd::StopMusic { .. } => println!("[music stops]"),
            Cmd::PlaySfx { src, .. } => println!("[sfx: {src}]"),
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
            Cmd::Finished => println!("\n— the end —"),
            Cmd::Stopped { reason } => eprintln!("warning: the story was stopped: {reason}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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

    /// A script in a fresh folder, with the files `(name, bytes)` beside it.
    fn checked(script: &str, files: &[(&str, &[u8])]) -> Result<(), String> {
        static COUNTER: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let n = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("quillstone-check-test-{}-{n}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("assets")).unwrap();
        for (name, bytes) in files {
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
    fn the_market_demo_passes_check() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples");
        check(&root.join("market.quill.md")).unwrap();
    }
}
