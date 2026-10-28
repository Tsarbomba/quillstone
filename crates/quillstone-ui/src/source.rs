//! Reading a script from disk with its `::include`d files (the script crate itself does no I/O).

use std::io::Read;
use std::path::Path;

use quillstone_script::{parse_files, ParseError, Program, MAX_INCLUDE_BYTES};

/// The text of an included file: `rel` (a game-folder path the parser has already checked) under `root`.
/// At most [`MAX_INCLUDE_BYTES`] are read, so a huge file can't fill memory before the parser counts it.
pub fn read_include(root: &Path, rel: &str) -> Result<String, String> {
    let file = std::fs::File::open(root.join(rel)).map_err(|e| e.to_string())?;
    let mut text = String::new();
    file.take(MAX_INCLUDE_BYTES as u64 + 1).read_to_string(&mut text).map_err(|e| e.to_string())?;
    Ok(text)
}

/// Compile `src`, the text of the script at `script`, reading its `::include`s from the script's
/// folder (the game folder). Errors name the included file they are in.
pub fn parse_script(script: &Path, src: &str) -> Result<Program, Vec<ParseError>> {
    let root = script.parent().unwrap_or_else(|| Path::new("."));
    let main = script.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    parse_files(src, &main, &|rel| read_include(root, rel))
}

/// One `path:line: error: message` per error, each with the path of the file it is in (an included
/// file's path is under the script's folder), as a compiler prints them.
pub fn error_lines(script: &Path, errors: &[ParseError]) -> Vec<String> {
    let root = script.parent().unwrap_or_else(|| Path::new(""));
    let place = |e: &ParseError| match &e.file {
        Some(file) => root.join(file).display().to_string(),
        None => script.display().to_string(),
    };
    errors.iter().map(|e| format!("{}:{}: error: {}", place(e), e.line, e.message)).collect()
}

/// The files a running game depends on besides the theme: the script and every file it includes.
pub fn script_files(script: &Path, program: &Program) -> Vec<std::path::PathBuf> {
    let root = script.parent().unwrap_or_else(|| Path::new("."));
    std::iter::once(script.to_path_buf()).chain(program.included_files().map(|f| root.join(f))).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn game(files: &[(&str, &str)]) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("qs-source-{}-{:?}", std::process::id(), std::thread::current().id()));
        let _ = std::fs::remove_dir_all(&dir);
        for (name, text) in files {
            let path = dir.join(name);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, text).unwrap();
        }
        dir
    }

    #[test]
    fn reads_includes_from_the_game_folder() {
        let dir = game(&[("main.quill.md", "# a\nHi.\n::include{ch/one.quill.md}\n"), ("ch/one.quill.md", "# b\nThere.\n")]);
        let script = dir.join("main.quill.md");
        let p = parse_script(&script, &std::fs::read_to_string(&script).unwrap()).unwrap();
        assert!(p.labels.contains_key("b"));
        assert_eq!(script_files(&script, &p), [script.clone(), dir.join("ch/one.quill.md")]);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn error_lines_name_the_file() {
        let dir = game(&[("main.quill.md", "::include{ch/a.quill.md}\n::bogus\n"), ("ch/a.quill.md", "Hi.\n::bogus\n")]);
        let script = dir.join("main.quill.md");
        let e = parse_script(&script, &std::fs::read_to_string(&script).unwrap()).unwrap_err();
        let lines = error_lines(&script, &e);
        assert!(lines[0].starts_with(&format!("{}:2: error: ", script.display())), "{lines:?}");
        assert!(lines[1].starts_with(&format!("{}:2: error: ", dir.join("ch/a.quill.md").display())), "{lines:?}");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_missing_file_is_an_error_naming_it() {
        let dir = game(&[("main.quill.md", "::include{nope.quill.md}\n")]);
        let script = dir.join("main.quill.md");
        let e = parse_script(&script, &std::fs::read_to_string(&script).unwrap()).unwrap_err();
        assert!(e[0].message.contains("nope.quill.md"), "{e:?}");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_file_over_the_cap_is_cut_and_refused_by_the_parser() {
        let big = "x".repeat(MAX_INCLUDE_BYTES + 10);
        let dir = game(&[("main.quill.md", "::include{big.quill.md}\n"), ("big.quill.md", &big)]);
        assert_eq!(read_include(&dir, "big.quill.md").unwrap().len(), MAX_INCLUDE_BYTES + 1);
        let script = dir.join("main.quill.md");
        let e = parse_script(&script, &std::fs::read_to_string(&script).unwrap()).unwrap_err();
        assert!(e[0].message.contains("too much"), "{e:?}");
        let _ = std::fs::remove_dir_all(dir);
    }
}
