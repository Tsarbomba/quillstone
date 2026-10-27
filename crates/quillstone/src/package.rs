//! `quillstone package`: turn a script into a folder players can unzip and run.
//!
//! ```text
//! MyGame/
//!   MyGame.exe            the player, renamed
//!   avcodec-62.dll ...    FFmpeg's libraries (video), found next to the player or in --libs
//!   licenses/FFmpeg.txt   FFmpeg is LGPL: say so and where its source is
//!   game/
//!     story.quill.md      the script, under its own name (the only one, so the player finds it)
//!     theme.txt, assets/  only the files the script and theme use
//! ```
//!
//! The player finds `game/` by itself when double-clicked.

use std::collections::BTreeSet;
use std::path::{Component, Path, PathBuf};

use quillstone_script::Program;

/// FFmpeg's libraries the player loads (avfilter and avdevice are never needed).
const FFMPEG_LIBS: [&str; 5] = ["avcodec", "avformat", "avutil", "swscale", "swresample"];

pub struct Options<'a> {
    pub script: &'a Path,
    pub player: &'a Path,
    pub out: &'a Path,
    pub name: Option<&'a str>,
    pub libs: Option<&'a Path>,
}

pub fn package(opts: &Options, program: &Program) -> Result<(), String> {
    let root = opts.script.parent().unwrap_or_else(|| Path::new("."));
    if !opts.player.is_file() {
        return Err(format!("the player `{}` doesn't exist (build it with `cargo build --release` in player/)", opts.player.display()));
    }
    if opts.out.exists() && std::fs::read_dir(opts.out).map_or(true, |mut d| d.next().is_some()) {
        return Err(format!("`{}` already exists and isn't empty; pick a new folder", opts.out.display()));
    }

    // What the game reads: the script, its theme, and every file either names.
    let script_name = opts.script.file_name().and_then(|n| n.to_str()).ok_or("the script needs a plain file name")?;
    let mut files: BTreeSet<String> = BTreeSet::new();
    files.insert(script_name.to_string());
    files.extend(program.assets().map(|(_, _, a)| a.to_string()));
    files.extend(program.extra_assets().map(|(_, a)| a.to_string()));
    if let Some(theme_file) = &program.theme {
        files.insert(theme_file.clone());
        let text = std::fs::read_to_string(root.join(theme_file)).map_err(|e| format!("cannot read the theme: {e}"))?;
        files.extend(quillstone_render::Theme::parse(&text).0.assets().into_iter().map(String::from));
    }
    // Auto-voice recordings that belong to a line (leftovers stay behind).
    if let Some(folder) = &program.voice_folder {
        let names: BTreeSet<String> =
            (0..program.ops.len()).filter_map(|pc| quillstone_runtime::voice_name(program, pc)).collect();
        if let Ok(entries) = std::fs::read_dir(root.join(folder)) {
            for name in entries.filter_map(|e| e.ok()?.file_name().into_string().ok()) {
                if name.rsplit_once('.').is_some_and(|(stem, _)| names.contains(stem)) {
                    files.insert(format!("{folder}/{name}"));
                }
            }
        }
    }
    for f in &files {
        if f.contains('\\') {
            return Err(format!("can't package `{f}`: write `/` between folders, which works on every system"));
        }
        if !is_inside(f) {
            return Err(format!("can't package `{f}`: it is outside the script's folder (keep assets beside or below the script)"));
        }
    }

    let game = opts.out.join("game");
    let mut bytes = 0;
    for f in &files {
        bytes += copy(&root.join(f), &game.join(f))?;
    }

    let name = exe_name(opts.name.or(program.title.as_deref()));
    let ext = opts.player.extension().map(|e| format!(".{}", e.to_string_lossy())).unwrap_or_default();
    bytes += copy(opts.player, &opts.out.join(format!("{name}{ext}")))?;

    let lib_dir = opts.libs.or(opts.player.parent()).unwrap_or_else(|| Path::new("."));
    let libs = ffmpeg_libs(lib_dir);
    for lib in &libs {
        bytes += copy(lib, &opts.out.join(lib.file_name().expect("listed from a folder")))?;
    }
    if libs.is_empty() {
        eprintln!(
            "warning: no FFmpeg libraries in `{}`; videos won't play (pass --libs with FFmpeg's bin folder, \
             unless the player was built without video)",
            lib_dir.display()
        );
    } else {
        write_ffmpeg_notice(lib_dir, &opts.out.join("licenses"))?;
    }

    println!(
        "packaged {} game file(s), the player and {} FFmpeg librar{} into {} ({:.1} MB)",
        files.len(),
        libs.len(),
        if libs.len() == 1 { "y" } else { "ies" },
        opts.out.display(),
        bytes as f64 / 1e6
    );
    Ok(())
}

/// A relative path that stays inside the folder it's relative to.
/// `\` is refused on every system: on Windows it is a separator, so
/// `assets\..\..\x.png` would climb out there even though it is one name elsewhere.
fn is_inside(path: &str) -> bool {
    let p = Path::new(path);
    !path.is_empty()
        && !path.contains('\\')
        && p.components().all(|c| matches!(c, Component::Normal(_) | Component::CurDir))
}

/// The executable's name: the game's title without characters file systems dislike.
fn exe_name(title: Option<&str>) -> String {
    let clean: String = title
        .unwrap_or("")
        .chars()
        .filter(|c| c.is_alphanumeric() || matches!(c, ' ' | '-' | '_' | '.' | '\''))
        .collect();
    let clean = clean.trim().trim_matches('.').trim();
    if clean.is_empty() { "game".into() } else { clean.into() }
}

/// Whether a file is one of FFmpeg's shared libraries we need: `avcodec-62.dll`
/// (Windows), `libavcodec.so.62` (Linux), `libavcodec.62.dylib` (macOS).
fn is_ffmpeg_lib(file: &str) -> bool {
    let base = file.strip_prefix("lib").unwrap_or(file);
    let shared = file.ends_with(".dll") || file.ends_with(".dylib") || file.contains(".so");
    shared
        && FFMPEG_LIBS.iter().any(|lib| {
            base.strip_prefix(lib).is_some_and(|rest| rest.starts_with(['-', '.']))
        })
}

fn ffmpeg_libs(dir: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(dir) else { return Vec::new() };
    let mut libs: Vec<PathBuf> = entries
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.is_file() && p.file_name().and_then(|n| n.to_str()).is_some_and(is_ffmpeg_lib))
        .collect();
    libs.sort();
    libs
}

/// The LGPL asks that players are told FFmpeg is inside and can get its source.
/// FFmpeg builds usually carry their licence one folder up from `bin/` (or beside it).
fn write_ffmpeg_notice(lib_dir: &Path, licenses: &Path) -> Result<(), String> {
    let mut notice = String::from(
        "This game uses libraries from the FFmpeg project (https://ffmpeg.org), licensed under the\n\
         GNU Lesser General Public License (LGPL) version 2.1 or later. They are the separate\n\
         avcodec / avformat / avutil / swscale / swresample library files next to the game, and\n\
         may be replaced with other builds of the same FFmpeg version.\n\
         FFmpeg's source code: https://ffmpeg.org/download.html\n",
    );
    let found = [lib_dir.join("../LICENSE.txt"), lib_dir.join("LICENSE.txt"), lib_dir.join("../LICENSE")]
        .into_iter()
        .find_map(|p| std::fs::read_to_string(p).ok());
    match found {
        Some(text) => {
            notice.push_str("\nThe licence text that came with these libraries follows.\n\n");
            notice.push_str(&text);
        }
        None => eprintln!(
            "warning: FFmpeg's LICENSE.txt wasn't found near `{}`; add the LGPL text to licenses/FFmpeg.txt by hand",
            lib_dir.display()
        ),
    }
    std::fs::create_dir_all(licenses).map_err(|e| format!("cannot create {}: {e}", licenses.display()))?;
    std::fs::write(licenses.join("FFmpeg.txt"), notice).map_err(|e| format!("cannot write the FFmpeg notice: {e}"))
}

fn copy(from: &Path, to: &Path) -> Result<u64, String> {
    if let Some(dir) = to.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("cannot create {}: {e}", dir.display()))?;
    }
    std::fs::copy(from, to).map_err(|e| format!("cannot copy {}: {e}", from.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_paths_inside_the_game_folder_are_packaged() {
        assert!(is_inside("assets/bg.png") && is_inside("./theme.txt") && is_inside("story.quill.md"));
        assert!(!is_inside("../shared/bg.png") && !is_inside("assets/../../x.png"));
        assert!(!is_inside("/home/me/bg.png") && !is_inside(""));
        assert!(!is_inside("assets\\..\\..\\x.png") && !is_inside("assets\\bg.png"), "backslashes, on any system");
        #[cfg(windows)]
        assert!(!is_inside("C:\\art\\bg.png"));
    }

    #[test]
    fn the_exe_is_named_after_the_title() {
        assert_eq!(exe_name(Some("Rooftop Promise")), "Rooftop Promise");
        assert_eq!(exe_name(Some("What? A/B: test*")), "What AB test");
        assert_eq!(exe_name(Some("...")), "game");
        assert_eq!(exe_name(None), "game");
        assert_eq!(exe_name(Some("../../evil")), "evil", "--name can't leave the output folder");
    }

    #[test]
    fn ffmpeg_libraries_are_recognised_on_every_platform() {
        for yes in ["avcodec-62.dll", "swscale-9.dll", "libavformat.so.62", "libavutil.so.60.8.100", "libswresample.6.dylib"] {
            assert!(is_ffmpeg_lib(yes), "{yes}");
        }
        for no in ["avfilter-11.dll", "avdevice-62.dll", "avcodec.lib", "ffmpeg.exe", "libavcodec.a", "avcodecx.dll"] {
            assert!(!is_ffmpeg_lib(no), "{no}");
        }
    }

    #[test]
    fn packages_the_demo_with_only_what_it_uses() {
        let base = std::env::temp_dir().join(format!("quillstone-package-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let (src, out, libs) = (base.join("src"), base.join("out"), base.join("ffmpeg/bin"));
        std::fs::create_dir_all(src.join("assets")).unwrap();
        std::fs::create_dir_all(&libs).unwrap();
        std::fs::write(src.join("assets/bg.png"), "png").unwrap();
        std::fs::write(src.join("assets/unused.png"), "nope").unwrap();
        std::fs::write(src.join("assets/box.png"), "box").unwrap();
        std::fs::write(src.join("theme.txt"), "textbox_image = assets/box.png\n").unwrap();
        let script = "---\ntitle: Tiny Tale\ntheme: theme.txt\n---\n# start\n::bg{src=assets/bg.png}\nHello.\n";
        std::fs::write(src.join("tiny.quill.md"), script).unwrap();
        std::fs::write(base.join("player.exe"), "exe").unwrap();
        for lib in ["avcodec-62.dll", "avfilter-11.dll", "avutil-60.dll"] {
            std::fs::write(libs.join(lib), "dll").unwrap();
        }
        std::fs::write(base.join("ffmpeg/LICENSE.txt"), "LGPL TEXT").unwrap();

        let program = quillstone_script::parse(script).unwrap();
        let opts = Options {
            script: &src.join("tiny.quill.md"),
            player: &base.join("player.exe"),
            out: &out,
            name: None,
            libs: Some(&libs),
        };
        package(&opts, &program).unwrap();

        for f in ["Tiny Tale.exe", "avcodec-62.dll", "avutil-60.dll", "game/tiny.quill.md", "game/theme.txt",
                  "game/assets/bg.png", "game/assets/box.png"] {
            assert!(out.join(f).is_file(), "{f}");
        }
        assert!(!out.join("avfilter-11.dll").exists() && !out.join("game/assets/unused.png").exists());
        assert!(std::fs::read_to_string(out.join("licenses/FFmpeg.txt")).unwrap().contains("LGPL TEXT"));
        assert!(package(&opts, &program).unwrap_err().contains("isn't empty"), "never overwrites");
        std::fs::remove_dir_all(&base).unwrap();
    }
}
