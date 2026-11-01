//! Translations in the command line: `quillstone lang export`, and the checks `quillstone check` makes.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use quillstone_script::{check_translation, language_file, Catalog, Program};
use quillstone_ui::source::read_translation;

/// `quillstone lang export <script> <code>`: create or update `lang/<code>.txt`.
pub fn export(script: &Path, program: &Program, code: &str) -> Result<(), String> {
    let known = program.languages.iter().map(|l| l.code.as_str()).collect::<Vec<_>>().join(", ");
    let Some(first) = program.languages.first() else {
        return Err("the script declares no languages: add `languages: en=English, sv=Svenska` to its front matter".into());
    };
    if first.code == code {
        return Err(format!("`{code}` is the language the script is written in; translate into one of the others"));
    }
    if !program.languages.iter().any(|l| l.code == code) {
        return Err(format!("`{code}` is not in the script's `languages:` (they are: {known})"));
    }
    let root = quillstone_ui::source::script_dir(script);
    let file = language_file(code);
    let old = match read_translation(root, code)? {
        None => Catalog::default(),
        Some(text) => Catalog::parse(&text).map_err(|errors| {
            let lines: Vec<String> = errors.iter().map(|e| format!("{}:{e}", root.join(&file).display())).collect();
            format!("{}\n(fix {file} first; nothing was written)", lines.join("\n"))
        })?,
    };
    let strings = program.translatable();
    let places: BTreeMap<String, String> = strings.iter().map(|s| (s.text.clone(), s.place.clone())).collect();
    let merged = old.merged(&strings);
    let name = program.languages.iter().find(|l| l.code == code).map_or(code, |l| l.name.as_str());
    let text = merged.render(name, &places);
    let path = root.join(&file);
    std::fs::create_dir_all(path.parent().expect("lang/ has a parent")).map_err(|e| format!("cannot create lang/: {e}"))?;
    write_file(&path, &text).map_err(|e| format!("cannot write {}: {e}", path.display()))?;
    let have: BTreeSet<&str> = old.entries.iter().map(|e| e.source.as_str()).collect();
    let fresh = strings.iter().filter(|s| !have.contains(s.text.as_str())).count();
    let obsolete = merged.entries.iter().filter(|e| e.obsolete).count();
    let todo = merged.entries.iter().filter(|e| !e.obsolete && e.context.is_none() && e.target.is_empty()).count();
    println!("{}: {} text(s), {fresh} new, {obsolete} obsolete, {todo} not translated yet", path.display(), strings.len());
    Ok(())
}

/// Write `text` to a temp file next to `path`, then rename it over, so a full disk or a kill midway
/// leaves the translator's file as it was.
fn write_file(path: &Path, text: &str) -> std::io::Result<()> {
    use std::io::Write;
    let tmp = path.with_extension("tmp");
    let mut file = std::fs::File::create(&tmp)?;
    file.write_all(text.as_bytes())?;
    file.sync_all()?; // on disk before the rename, so a power cut can't leave an empty file in place
    drop(file);
    std::fs::rename(&tmp, path).inspect_err(|_| {
        let _ = std::fs::remove_file(&tmp);
    })
}

/// What `check` finds in the translation files: errors count as problems (printed here), the
/// rest are warnings. Returns the number of errors.
pub fn check(script: &Path, program: &Program) -> usize {
    let root = quillstone_ui::source::script_dir(script);
    let mut errors = 0;
    let strings: BTreeSet<String> = program.translatable().into_iter().map(|s| s.text).collect();
    let spoken = program.spoken();
    for lang in program.languages.iter().skip(1) {
        let file = language_file(&lang.code);
        let shown = root.join(&file).display().to_string();
        let text = match read_translation(root, &lang.code) {
            Ok(Some(t)) => t,
            Ok(None) => {
                eprintln!("{}: warning: no translation file `{file}` for {} (create it with `quillstone lang export <script> {}`)", script.display(), lang.name, lang.code);
                continue;
            }
            Err(e) => {
                errors += 1;
                eprintln!("{shown}: error: {e}");
                continue;
            }
        };
        let cat = match Catalog::parse(&text) {
            Ok(c) => c,
            Err(list) => {
                for e in list {
                    errors += 1;
                    eprintln!("{shown}:{}: error: {}", e.line, e.message);
                }
                continue;
            }
        };
        let mut obsolete = 0;
        for e in &cat.entries {
            let line = e.line;
            if let Err(m) = check_translation(&e.source, &e.target) {
                errors += 1;
                eprintln!("{shown}:{line}: error: {m}");
            }
            let used = strings.contains(&e.source)
                && e.context.as_ref().is_none_or(|c| spoken.contains(&(c.clone(), e.source.clone())));
            match (used, e.obsolete) {
                (true, _) => {}
                (false, true) => obsolete += 1,
                (false, false) => {
                    let what = if strings.contains(&e.source) { "no line spoken by that character has this text" } else { "the script has no such text (changed or removed?)" };
                    eprintln!("{shown}:{line}: warning: {what}; run `quillstone lang export` to flag it as obsolete");
                }
            }
        }
        if obsolete > 0 {
            eprintln!("{shown}: warning: {obsolete} obsolete entr{} kept (not in the script any more)", if obsolete == 1 { "y" } else { "ies" });
        }
        let missing = strings.iter().filter(|s| cat.get(None, s).is_none()).count();
        if missing > 0 {
            eprintln!("{shown}: warning: {} is {missing} of {} text(s) short: those show in the script's language", lang.name, strings.len());
        }
    }
    errors
}

#[cfg(test)]
mod tests {
    use super::*;

    const SCRIPT: &str = "---\ntitle: T\nlanguages: en=English, sv=Svenska\nvars: { n: 1 }\n---\n# a\n**A:** Hello {n}.\n\nBye.\n";

    fn game(name: &str, files: &[(&str, &str)]) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("qs-lang-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        for (f, text) in files {
            let path = dir.join(f);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, text).unwrap();
        }
        dir
    }

    fn export_to(dir: &Path, script: &str) -> Result<String, String> {
        let path = dir.join("main.quill.md");
        std::fs::write(&path, script).unwrap();
        let program = crate::load(&path)?;
        export(&path, &program, "sv")?;
        Ok(std::fs::read_to_string(dir.join("lang/sv.txt")).unwrap())
    }

    #[test]
    fn export_adds_new_texts_in_script_order_with_places_and_is_idempotent() {
        let dir = game("new", &[]);
        let text = export_to(&dir, SCRIPT).unwrap();
        let order: Vec<&str> = text.lines().filter(|l| l.starts_with("> ")).collect();
        assert_eq!(order, ["> T", "> A", "> Hello {n}.", "> Bye."]);
        assert!(text.contains("#: a:7\n> Hello {n}."), "{text}");
        assert_eq!(export_to(&dir, SCRIPT).unwrap(), text, "a second export changes nothing");
        // a translation and a translator's comment survive an export
        let filled = text.replace("> Bye.\n=", "# keep it short\n> Bye.\n= Hejdå.");
        std::fs::write(dir.join("lang/sv.txt"), &filled).unwrap();
        let again = export_to(&dir, SCRIPT).unwrap();
        assert!(again.contains("# keep it short\n#: a:9\n> Bye.\n= Hejdå."), "{again}");
        assert_eq!(export_to(&dir, SCRIPT).unwrap(), again);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_failed_export_leaves_the_translators_file_as_it_was() {
        let dir = game("atomic", &[]);
        let old = export_to(&dir, SCRIPT).unwrap();
        // a directory where the temp file goes makes the write fail
        std::fs::create_dir(dir.join("lang/sv.tmp")).unwrap();
        assert!(export_to(&dir, &SCRIPT.replace("Bye.", "Farewell.")).is_err());
        assert_eq!(std::fs::read_to_string(dir.join("lang/sv.txt")).unwrap(), old);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn texts_that_left_the_script_are_flagged_obsolete_not_deleted_and_come_back() {
        let dir = game("obsolete", &[]);
        export_to(&dir, SCRIPT).unwrap();
        let filled = std::fs::read_to_string(dir.join("lang/sv.txt")).unwrap().replace("> Bye.\n=", "> Bye.\n= Hejdå.");
        std::fs::write(dir.join("lang/sv.txt"), filled).unwrap();
        let without = export_to(&dir, &SCRIPT.replace("Bye.", "Farewell.")).unwrap();
        let obsolete = without.find("! obsolete").expect("flagged");
        assert!(without[obsolete..].contains("> Bye.\n= Hejdå."), "kept with its translation, at the end:\n{without}");
        assert!(without.contains("> Farewell.\n=\n"));
        let back = export_to(&dir, SCRIPT).unwrap();
        assert!(!back.contains("! obsolete") || !back[back.find("! obsolete").unwrap()..].contains("Hejdå"), "{back}");
        assert!(back.contains("> Bye.\n= Hejdå."));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn export_refuses_bad_requests_and_a_broken_file_is_left_alone() {
        let dir = game("refuse", &[("lang/sv.txt", "> a\nbroken\n")]);
        let err = export_to(&dir, SCRIPT).unwrap_err();
        assert!(err.contains("sv.txt:line 2") && err.contains("nothing was written"), "{err}");
        assert_eq!(std::fs::read_to_string(dir.join("lang/sv.txt")).unwrap(), "> a\nbroken\n");
        let program = quillstone_script::parse(SCRIPT).unwrap();
        let script = dir.join("main.quill.md");
        assert!(export(&script, &program, "en").unwrap_err().contains("written in"));
        assert!(export(&script, &program, "fr").unwrap_err().contains("not in the script"));
        let plain = quillstone_script::parse("Hi.\n").unwrap();
        assert!(export(&script, &plain, "sv").unwrap_err().contains("no languages"));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn check_counts_parse_errors_and_placeholder_mismatches_but_only_warns_about_the_rest() {
        let ok = "> Hello {n}.\n= Hej {n}.\n\n> Gone\n= Borta\n";
        let dir = game("check", &[("main.quill.md", SCRIPT), ("lang/sv.txt", ok)]);
        let path = dir.join("main.quill.md");
        let program = crate::load(&path).unwrap();
        assert_eq!(check(&path, &program), 0, "unknown text and untranslated texts are warnings");
        for (bad, why) in [
            ("> Hello {n}.\n= Hej {m}.\n", "placeholder"),
            ("> Hello {n}.\n= Hej.\n", "placeholder"),
            ("> Hello {n}.\n= Hej {n}{nw}.\n", "placeholder"),
            ("> Hello {n}.\nx\n", "parse"),
        ] {
            std::fs::write(dir.join("lang/sv.txt"), bad).unwrap();
            assert!(check(&path, &program) >= 1, "{why}: {bad:?}");
        }
        std::fs::remove_file(dir.join("lang/sv.txt")).unwrap();
        assert_eq!(check(&path, &program), 0, "no file: a warning");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn the_example_is_complete_and_checks_and_packages() {
        let examples = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples");
        let script = examples.join("lang.quill.md");
        let program = crate::load(&script).unwrap();
        assert_eq!(check(&script, &program), 0);
        crate::check(&script).unwrap();
        // its translation is up to date: exporting changes nothing and leaves nothing untranslated
        let dir = game("example", &[]);
        std::fs::copy(&script, dir.join("main.quill.md")).unwrap();
        std::fs::create_dir_all(dir.join("lang")).unwrap();
        let shipped = std::fs::read_to_string(examples.join("lang/sv.txt")).unwrap();
        std::fs::write(dir.join("lang/sv.txt"), &shipped).unwrap();
        let p = crate::load(&dir.join("main.quill.md")).unwrap();
        export(&dir.join("main.quill.md"), &p, "sv").unwrap();
        assert_eq!(std::fs::read_to_string(dir.join("lang/sv.txt")).unwrap(), shipped);
        assert!(!shipped.contains("=\n\n"), "everything is translated");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn package_ships_the_translation_files_and_check_passes_on_the_result() {
        let src = game("pack", &[("main.quill.md", SCRIPT), ("lang/sv.txt", "> Hello {n}.\n= Hej {n}.\n"), ("lang/fr.txt", "> x\n=\n")]);
        let out = src.join("out");
        std::fs::write(src.join("player.exe"), "exe").unwrap();
        let program = crate::load(&src.join("main.quill.md")).unwrap();
        let opts = crate::package::Options { script: &src.join("main.quill.md"), player: &src.join("player.exe"), out: &out, name: None, libs: None, folder: true };
        crate::package::package(&opts, &program).unwrap();
        assert!(out.join("game/lang/sv.txt").is_file());
        assert!(!out.join("game/lang/fr.txt").exists(), "only languages the script lists");
        crate::check(&out.join("game/main.quill.md")).unwrap();
        std::fs::remove_dir_all(src).unwrap();
    }
}
