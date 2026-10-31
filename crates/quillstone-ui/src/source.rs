//! Reading a script from disk with its `::include`d files (the script crate itself does no I/O).

use std::io::Read;
use std::path::Path;

use std::collections::BTreeMap;

use quillstone_script::{check_translation, language_file, parse_files, Catalog, ParseError, Program, MAX_FILE_BYTES, MAX_INCLUDE_BYTES};

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

/// The text of `lang/<code>.txt` under the game folder `root`: `Ok(None)` when there is no such
/// file. At most [`MAX_FILE_BYTES`] + 1 are read, so a huge file can't fill memory (the parser then refuses it).
pub fn read_translation(root: &Path, code: &str) -> Result<Option<String>, String> {
    let file = match std::fs::File::open(root.join(language_file(code))) {
        Ok(f) => f,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e.to_string()),
    };
    let mut bytes = Vec::new();
    file.take(MAX_FILE_BYTES as u64 + 1).read_to_end(&mut bytes).map_err(|e| e.to_string())?;
    String::from_utf8(bytes).map(Some).map_err(|_| "the file is not UTF-8 text".to_string())
}

/// Every translation the game has (its `languages:` after the first, from the script's folder), and
/// one `lang/<code>.txt:line: message` per problem. A file that doesn't parse is left out, and a
/// translation whose tags don't match its original is blanked: that text shows the script's own.
pub fn load_translations(script: &Path, program: &Program) -> (BTreeMap<String, Catalog>, Vec<String>) {
    let root = script.parent().unwrap_or_else(|| Path::new("."));
    let (mut found, mut problems) = (BTreeMap::new(), Vec::new());
    for lang in program.languages.iter().skip(1) {
        let file = language_file(&lang.code);
        match read_translation(root, &lang.code) {
            Ok(None) => {}
            Ok(Some(text)) => match Catalog::parse(&text) {
                Ok(mut cat) => {
                    // A translation whose tags differ from the original would change how the line plays
                    // (a `{nw}` added makes it advance by itself): that entry shows the script's text.
                    // Obsolete entries are never shown, so they don't bother the player (`quillstone check` lists them).
                    for e in cat.entries.iter_mut().filter(|e| !e.obsolete) {
                        if let Err(m) = check_translation(&e.source, &e.target) {
                            problems.push(format!("{file}:{}: {m}", e.line));
                            e.target.clear();
                        }
                    }
                    found.insert(lang.code.clone(), cat);
                }
                Err(errors) => problems.extend(errors.iter().map(|e| match e.line {
                    0 => format!("{file}: {}", e.message),
                    n => format!("{file}:{n}: {}", e.message),
                })),
            },
            Err(e) => problems.push(format!("cannot read {file}: {e}")),
        }
    }
    (found, problems)
}

/// The translation files a running game depends on (they may not exist yet).
pub fn translation_files(script: &Path, program: &Program) -> Vec<std::path::PathBuf> {
    let root = script.parent().unwrap_or_else(|| Path::new("."));
    program.languages.iter().skip(1).map(|l| root.join(language_file(&l.code))).collect()
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

    #[test]
    fn translations_are_read_from_lang_and_problems_name_file_and_line() {
        let head = "---\nlanguages: en=English, sv=Svenska, es=Espanol\n---\nHi.\n";
        let dir = game(&[("main.quill.md", head), ("lang/sv.txt", "> Hi.\n= Hej.\n"), ("lang/es.txt", "> Hi.\noops\n")]);
        let script = dir.join("main.quill.md");
        let p = parse_script(&script, head).unwrap();
        let (found, problems) = load_translations(&script, &p);
        assert_eq!(found["sv"].get(None, "Hi."), Some("Hej."));
        assert!(!found.contains_key("es"));
        assert!(problems[0].starts_with("lang/es.txt:2: "), "{problems:?}");
        assert_eq!(translation_files(&script, &p), [dir.join("lang/sv.txt"), dir.join("lang/es.txt")]);
        assert_eq!(read_translation(&dir, "fr").unwrap(), None);
        std::fs::write(dir.join("lang/sv.txt"), vec![b'#'; MAX_FILE_BYTES + 50]).unwrap();
        assert_eq!(read_translation(&dir, "sv").unwrap().unwrap().len(), MAX_FILE_BYTES + 1);
        assert!(load_translations(&script, &p).1.iter().any(|m| m.contains("larger than")));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_translation_with_other_tags_than_its_original_is_blanked_and_reported() {
        let head = "---\nlanguages: en=English, sv=Svenska\n---\nHi.{nw}\n\nBye.\n\nSee {n}.\n";
        let lang = "> Hi.{nw}\n= Hej.\n\n> Bye.\n= Hejdå.{nw}\n\n> See {n}.\n= Se {n}.\n";
        let dir = game(&[("main.quill.md", head), ("lang/sv.txt", lang)]);
        let script = dir.join("main.quill.md");
        let p = parse_script(&script, head).unwrap();
        let (found, problems) = load_translations(&script, &p);
        assert_eq!(found["sv"].get(None, "Hi.{nw}"), None, "dropping {{nw}} would make the line wait");
        assert_eq!(found["sv"].get(None, "Bye."), None, "adding {{nw}} would make the line skip itself");
        assert_eq!(found["sv"].get(None, "See {n}."), Some("Se {n}."));
        assert_eq!(problems.len(), 2, "{problems:?}");
        assert!(problems[0].starts_with("lang/sv.txt:1: ") && problems[1].starts_with("lang/sv.txt:4: "), "{problems:?}");
        let _ = std::fs::remove_dir_all(dir);
    }
}
