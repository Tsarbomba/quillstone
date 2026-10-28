//! `quillstone package`: turn a script into a folder players can unzip and run.
//!
//! ```text
//! MyGame/
//!   MyGame.exe            the player, renamed
//!   avcodec-62.dll ...    FFmpeg's libraries (video), found next to the player or in --libs
//!   licenses/FFmpeg.txt   FFmpeg is LGPL: say so and where its source is
//!   game/
//!     story.quill.md      the script, under its own name (the only one, so the player finds it)
//!     chapters/...        files the script `::include`s, at the same paths
//!     theme.txt, assets/  only the files the script and theme use
//! ```
//!
//! The player finds `game/` by itself when double-clicked.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use quillstone_script::{is_inside, Program};

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
    // Included scripts keep their paths under game/, so the player reads them as it does from the project folder.
    files.extend(program.included_files().map(String::from));
    // The player starts `main.quill.md`, else the only `.quill.md` beside it: another script there makes it ambiguous.
    if script_name != "main.quill.md" {
        if let Some(beside) = program.included_files().find(|f| !f.contains('/') && f.to_ascii_lowercase().ends_with(".quill.md")) {
            return Err(format!(
                "can't package: `{beside}` is included from `{script_name}` but sits beside it, so the player couldn't tell which \
                 is the game's script. Name the main script `main.quill.md`, or move included files into a folder (`chapters/`)"
            ));
        }
    }
    for (_, _, asset) in program.assets() {
        if quillstone_script::is_pattern(asset) {
            // A picture named from variables: every file it can build ships.
            let matches = crate::pattern::expand(root, asset);
            if matches.is_empty() {
                return Err(format!("can't package `{asset}`: it matches no file"));
            }
            files.extend(matches);
        } else {
            files.insert(asset.to_string());
        }
    }
    files.extend(program.extra_assets().map(|(_, a)| a.to_string()));
    files.extend(program.item_icons().map(|(_, a)| a.to_string()));
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
    // Translations (a language without a file just shows the script's text, which `check` warns about).
    for lang in program.languages.iter().skip(1) {
        let file = quillstone_script::language_file(&lang.code);
        if root.join(&file).is_file() {
            files.insert(file);
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
    fn packages_every_animation_frame() {
        let base = std::env::temp_dir().join(format!("quillstone-package-anim-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let (src, out) = (base.join("src"), base.join("out"));
        std::fs::create_dir_all(src.join("assets")).unwrap();
        for f in ["o.png", "c.png", "rain1.png", "rain2.png", "unused.png"] {
            std::fs::write(src.join("assets").join(f), "png").unwrap();
        }
        let script = "::anim{id=blink frames=\"assets/o.png 3, assets/c.png 0.1\" loop}\n::anim{id=rain frames=\"assets/rain1.png 1, assets/rain2.png 1\" loop}\n::character{id=a sprites=assets/{pose}.png}\n::bg{anim=rain}\n::show{a anim=blink}\nHi.\n";
        std::fs::write(src.join("anim.quill.md"), script).unwrap();
        std::fs::write(base.join("player.exe"), "exe").unwrap();
        let program = quillstone_script::parse(script).unwrap();
        let opts = Options { script: &src.join("anim.quill.md"), player: &base.join("player.exe"), out: &out, name: None, libs: None };
        package(&opts, &program).unwrap();
        for f in ["o.png", "c.png", "rain1.png", "rain2.png"] {
            assert!(out.join("game/assets").join(f).is_file(), "{f}");
        }
        assert!(!out.join("game/assets/unused.png").exists());
        std::fs::remove_dir_all(&base).unwrap();
    }

    #[test]
    fn packages_a_particle_picture() {
        let base = std::env::temp_dir().join(format!("quillstone-package-particles-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let (src, out) = (base.join("src"), base.join("out"));
        std::fs::create_dir_all(src.join("assets")).unwrap();
        for f in ["petal.png", "unused.png"] {
            std::fs::write(src.join("assets").join(f), "png").unwrap();
        }
        let script = "::particles{snow}\n::particles{src=assets/petal.png}\nHi.\n";
        std::fs::write(src.join("p.quill.md"), script).unwrap();
        std::fs::write(base.join("player.exe"), "exe").unwrap();
        let program = quillstone_script::parse(script).unwrap();
        let opts = Options { script: &src.join("p.quill.md"), player: &base.join("player.exe"), out: &out, name: None, libs: None };
        package(&opts, &program).unwrap();
        assert!(out.join("game/assets/petal.png").is_file());
        assert!(!out.join("game/assets/unused.png").exists());
        std::fs::remove_dir_all(&base).unwrap();
    }

    #[test]
    fn packages_a_gallery_video() {
        let base = std::env::temp_dir().join(format!("quillstone-package-gallery-video-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let (src, out) = (base.join("src"), base.join("out"));
        std::fs::create_dir_all(src.join("assets")).unwrap();
        for f in ["clip.webm", "unused.webm"] {
            std::fs::write(src.join("assets").join(f), "webm").unwrap();
        }
        let script = "---\ngallery: [assets/clip.webm]\n---\n::bg{src=assets/clip.webm}\nHi.\n";
        std::fs::write(src.join("g.quill.md"), script).unwrap();
        std::fs::write(base.join("player.exe"), "exe").unwrap();
        let program = quillstone_script::parse(script).unwrap();
        let opts = Options { script: &src.join("g.quill.md"), player: &base.join("player.exe"), out: &out, name: None, libs: None };
        package(&opts, &program).unwrap();
        assert!(out.join("game/assets/clip.webm").is_file());
        assert!(!out.join("game/assets/unused.webm").exists());
        std::fs::remove_dir_all(&base).unwrap();
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
        std::fs::write(src.join("assets/icon.png"), "icon").unwrap();
        std::fs::write(src.join("assets/gem.png"), "gem").unwrap();
        std::fs::write(src.join("assets/panel.png"), "panel").unwrap();
        std::fs::write(src.join("assets/plate.png"), "plate").unwrap();
        std::fs::write(src.join("assets/rain.ogg"), "rain").unwrap();
        std::fs::write(src.join("assets/f.ttf"), [0, 1, 0, 0]).unwrap();
        std::fs::write(src.join("assets/fb.ttf"), [0, 1, 0, 0]).unwrap();
        std::fs::write(src.join("theme.txt"), "textbox_image = assets/box.png\nname_plate_image = assets/plate.png\ninventory_panel = assets/panel.png\nfont = assets/f.ttf\nfont_bold = assets/fb.ttf\n").unwrap();
        let script = "---\ntitle: Tiny Tale\ntheme: theme.txt\ninventory: bag\nvars: { bag: [] }\n---\n::item{id=gem icon=assets/gem.png price=1}\n# start\n::bg{src=assets/bg.png}\n::sfx{src=assets/rain.ogg loop}\nHello.\n::hotspots\n- [Again](#start){rect=0,0,0.1,0.1 image=assets/icon.png}\n::\n";
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
                  "game/assets/bg.png", "game/assets/box.png", "game/assets/icon.png", "game/assets/gem.png",
                  "game/assets/panel.png", "game/assets/plate.png", "game/assets/rain.ogg", "game/assets/f.ttf", "game/assets/fb.ttf"] {
            assert!(out.join(f).is_file(), "{f}");
        }
        assert!(!out.join("avfilter-11.dll").exists() && !out.join("game/assets/unused.png").exists());
        assert!(std::fs::read_to_string(out.join("licenses/FFmpeg.txt")).unwrap().contains("LGPL TEXT"));
        assert!(package(&opts, &program).unwrap_err().contains("isn't empty"), "never overwrites");
        std::fs::remove_dir_all(&base).unwrap();
    }

    #[test]
    fn packages_every_layer_file_and_what_a_layer_pattern_can_build() {
        let base = std::env::temp_dir().join(format!("quillstone-package-layers-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let (src, out) = (base.join("src"), base.join("out"));
        std::fs::create_dir_all(src.join("a")).unwrap();
        let files = ["body_casual", "body_formal", "eyes_open", "mouth_smile", "mouth_open", "open", "closed", "eyes_unused", "hat_x"];
        for f in files {
            std::fs::write(src.join("a").join(format!("{f}.png")), "png").unwrap();
        }
        let script = "---\nvars: { mood: \"smile\" }\n---\n::anim{id=blink frames=\"a/open.png 3, a/closed.png 0.1\" loop}\n\
            ::character{id=m layers=body,eyes,mouth layer_sprites=a/{layer}_{value}.png}\n\
            ::show{m body=casual eyes=anim:blink mouth={mood}}\nHi.\n::show{m body=formal eyes=open}\nBye.\n";
        std::fs::write(src.join("l.quill.md"), script).unwrap();
        std::fs::write(base.join("player.exe"), "exe").unwrap();
        let program = quillstone_script::parse(script).unwrap();
        let opts = Options { script: &src.join("l.quill.md"), player: &base.join("player.exe"), out: &out, name: None, libs: None };
        package(&opts, &program).unwrap();
        for f in ["body_casual", "body_formal", "eyes_open", "mouth_smile", "mouth_open", "open", "closed"] {
            assert!(out.join("game/a").join(format!("{f}.png")).is_file(), "{f} ships");
        }
        for f in ["eyes_unused", "hat_x"] {
            assert!(!out.join("game/a").join(format!("{f}.png")).exists(), "{f} does not");
        }
        std::fs::remove_dir_all(&base).unwrap();
    }

    #[test]
    fn packages_every_file_a_picture_pattern_can_build_and_nothing_else() {
        let base = std::env::temp_dir().join(format!("quillstone-package-pattern-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let (src, out) = (base.join("src"), base.join("out"));
        std::fs::create_dir_all(src.join("assets/places/sub")).unwrap();
        for f in ["places/rooftop.png", "places/night.png", "places/sub/deep.png", "places/readme.txt", "other.png", "mika_happy.png", "mika_sad.png", "ren_happy.png"] {
            std::fs::write(src.join("assets").join(f), "png").unwrap();
        }
        let script = "---\nvars: { place: \"rooftop\", mood: \"happy\" }\n---\n::character{id=mika sprites=assets/mika_{pose}.png}\n::bg{src=\"assets/places/{place}.png\"}\n::show{mika {mood}}\nHi.\n";
        std::fs::write(src.join("p.quill.md"), script).unwrap();
        std::fs::write(base.join("player.exe"), "exe").unwrap();
        let program = quillstone_script::parse(script).unwrap();
        let opts = Options { script: &src.join("p.quill.md"), player: &base.join("player.exe"), out: &out, name: None, libs: None };
        package(&opts, &program).unwrap();
        for f in ["places/rooftop.png", "places/night.png", "mika_happy.png", "mika_sad.png"] {
            assert!(out.join("game/assets").join(f).is_file(), "{f} ships");
        }
        for f in ["places/sub/deep.png", "places/readme.txt", "other.png", "ren_happy.png"] {
            assert!(!out.join("game/assets").join(f).exists(), "{f} does not");
        }
        // A pattern that matches nothing can't be packaged.
        let lonely = "::bg{src=\"assets/nowhere/{p}.png\"}\nHi.\n";
        std::fs::write(src.join("l.quill.md"), lonely).unwrap();
        let opts = Options { script: &src.join("l.quill.md"), player: &base.join("player.exe"), out: &base.join("out2"), name: None, libs: None };
        let err = package(&opts, &quillstone_script::parse(lonely).unwrap()).unwrap_err();
        assert!(err.contains("matches no file"), "{err}");
        std::fs::remove_dir_all(&base).unwrap();
    }

    #[test]
    fn packages_included_scripts_at_their_paths() {
        let base = std::env::temp_dir().join(format!("quillstone-package-include-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let (src, out) = (base.join("src"), base.join("out"));
        std::fs::create_dir_all(src.join("chapters/deep")).unwrap();
        std::fs::create_dir_all(src.join("assets")).unwrap();
        std::fs::write(src.join("assets/bg.png"), include_bytes!("../../../examples/assets/gui/key.png")).unwrap();
        std::fs::write(src.join("chapters/unused.quill.md"), "Not included.\n").unwrap();
        let main = "---\ntitle: Split\n---\n::include{chapters/one.quill.md}\n";
        std::fs::write(src.join("main.quill.md"), main).unwrap();
        std::fs::write(src.join("chapters/one.quill.md"), "# one\n::bg{src=assets/bg.png}\n::include{chapters/deep/two.quill.md}\n").unwrap();
        std::fs::write(src.join("chapters/deep/two.quill.md"), "Hi.\n").unwrap();
        std::fs::write(base.join("player.exe"), "exe").unwrap();
        let script = src.join("main.quill.md");
        let program = crate::load(&script).unwrap();
        let opts = Options { script: &script, player: &base.join("player.exe"), out: &out, name: None, libs: None };
        package(&opts, &program).unwrap();
        for f in ["main.quill.md", "chapters/one.quill.md", "chapters/deep/two.quill.md", "assets/bg.png"] {
            assert!(out.join("game").join(f).is_file(), "{f}");
        }
        assert!(!out.join("game/chapters/unused.quill.md").exists());
        // the packed game checks out on its own
        assert!(crate::check(&out.join("game/main.quill.md")).is_ok());
        std::fs::remove_dir_all(&base).unwrap();
    }

    #[test]
    fn an_included_script_beside_a_differently_named_main_is_refused() {
        let base = std::env::temp_dir().join(format!("quillstone-package-include-beside-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let (src, out) = (base.join("src"), base.join("out"));
        std::fs::create_dir_all(&src).unwrap();
        std::fs::write(src.join("story.quill.md"), "::include{two.quill.md}\n").unwrap();
        std::fs::write(src.join("two.quill.md"), "Hi.\n").unwrap();
        std::fs::write(base.join("player.exe"), "exe").unwrap();
        let script = src.join("story.quill.md");
        let program = crate::load(&script).unwrap();
        let opts = Options { script: &script, player: &base.join("player.exe"), out: &out, name: None, libs: None };
        assert!(package(&opts, &program).unwrap_err().contains("main.quill.md"));
        assert!(!out.exists());
        // as main.quill.md it is fine
        std::fs::rename(&script, src.join("main.quill.md")).unwrap();
        let script = src.join("main.quill.md");
        let program = crate::load(&script).unwrap();
        let opts = Options { script: &script, player: &base.join("player.exe"), out: &out, name: None, libs: None };
        package(&opts, &program).unwrap();
        std::fs::remove_dir_all(&base).unwrap();
    }
}
