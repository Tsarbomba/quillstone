//! `::include`: other script files spliced in at compile time.

use std::collections::HashMap;

use crate::{parse, parse_files, Op, ParseError, Program, MAX_INCLUDES, MAX_INCLUDE_BYTES, MAX_INCLUDE_DEPTH};

fn load(main: &str, files: &[(&str, &str)]) -> Result<Program, Vec<ParseError>> {
    let files: HashMap<String, String> = files.iter().map(|(n, t)| (n.to_string(), t.to_string())).collect();
    parse_files(main, "main.quill.md", &|p| files.get(p).cloned().ok_or_else(|| "no such file".to_string()))
}

fn errors(main: &str, files: &[(&str, &str)]) -> Vec<String> {
    load(main, files).expect_err("expected errors").iter().map(|e| format!("{}:{}: {}", e.file.as_deref().unwrap_or("main.quill.md"), e.line, e.message)).collect()
}

fn says(p: &Program) -> Vec<&str> {
    p.ops.iter().filter_map(|op| if let Op::Say { text, .. } = op { Some(text.as_str()) } else { None }).collect()
}

#[test]
fn an_included_file_is_spliced_in_where_the_line_stands() {
    let p = load("# a\nOne.\n::include{ch/b.quill.md}\nThree.\n", &[("ch/b.quill.md", "# b\nTwo.\n")]).unwrap();
    assert_eq!(says(&p), ["One.", "Two.", "Three."]);
    assert_eq!(p.labels["b"], 1);
    assert_eq!(p.included_files().collect::<Vec<_>>(), ["ch/b.quill.md"]);
}

#[test]
fn both_spellings_and_quotes_work_and_dot_slash_is_ignored() {
    for form in ["::include{b.quill.md}", "::include{src=b.quill.md}", "::include{src=\"b.quill.md\"}", "::include{./b.quill.md}"] {
        let p = load(&format!("{form}\n"), &[("b.quill.md", "Hi.\n")]).unwrap();
        assert_eq!(says(&p), ["Hi."], "{form}");
        assert_eq!(p.sources[0].file, "b.quill.md");
    }
    assert!(load("::include{b.md}\n", &[("b.md", "Hi.\n")]).is_ok(), ".md is allowed");
}

#[test]
fn includes_nest_and_labels_jump_across_files() {
    let p = load(
        "::include{a.quill.md}\n# end\nBye.\n",
        &[
            ("a.quill.md", "# a\n::include{sub/b.quill.md}\n::jump{to=end}\n"),
            ("sub/b.quill.md", "# b\nIn b.\n::jump{to=a}\n"),
        ],
    )
    .unwrap();
    assert_eq!(p.sources.len(), 2);
    assert!(p.labels.contains_key("a") && p.labels.contains_key("b") && p.labels.contains_key("end"));
}

#[test]
fn characters_and_conditions_in_an_included_file_work_as_if_written_there() {
    let p = load(
        "---\nvars: { trust: 0 }\n---\n::include{c.quill.md}\n**Mika:** Hi.\n",
        &[("c.quill.md", "::character{id=mika name=Mika color=#ffd98c}\n::set{trust+=1}\n::if{trust > 0}\nSo.\n::endif\n")],
    )
    .unwrap();
    assert!(p.characters.contains_key("mika"));
    assert_eq!(says(&p), ["So.", "Hi."]);
}

#[test]
fn an_if_may_span_files() {
    let p = load("::if{1 == 1}\n::include{a.quill.md}\n::endif\n", &[("a.quill.md", "Inside.\n")]).unwrap();
    assert_eq!(says(&p), ["Inside."]);
}

#[test]
fn prose_does_not_join_across_an_include() {
    let p = load("Before\n::include{a.quill.md}\nAfter\n", &[("a.quill.md", "Middle\n")]).unwrap();
    assert_eq!(says(&p), ["Before", "Middle", "After"]);
}

#[test]
fn an_include_inside_a_comment_is_not_read() {
    let p = load("<!--\n::include{nope.quill.md}\n-->\nHi.\n", &[]).unwrap();
    assert!(p.sources.is_empty());
}

#[test]
fn lines_map_back_to_their_file() {
    let p = load("Main one.\n::include{a.quill.md}\nMain three.\n", &[("a.quill.md", "\n\nIn a, line 3.\nAnd 4.\n")]).unwrap();
    let at = |text: &str| {
        let pc = p.ops.iter().position(|op| matches!(op, Op::Say { text: t, .. } if t == text)).unwrap();
        match p.locate(p.lines[pc]) {
            (Some(file), n) => format!("{file}:{n}"),
            (None, n) => format!("main.quill.md:{n}"),
        }
    };
    assert_eq!(at("Main one."), "main.quill.md:1");
    assert_eq!(at("In a, line 3. And 4."), "a.quill.md:3");
    assert_eq!(at("Main three."), "main.quill.md:3");
    assert_eq!(p.locate(2), (None, 2));
}

#[test]
fn errors_name_the_file_and_line() {
    let e = errors("Fine.\n::include{a.quill.md}\n", &[("a.quill.md", "Fine.\nAlso.\n::bogus\n")]);
    assert_eq!(e.len(), 1, "{e:?}");
    assert!(e[0].starts_with("a.quill.md:3: "), "{e:?}");
    let errs = load("::include{a.quill.md}\n::bogus\n", &[("a.quill.md", "::bogus\n")]).unwrap_err();
    assert_eq!((errs[0].file.as_deref(), errs[0].line), (None, 2));
    assert_eq!((errs[1].file.as_deref(), errs[1].line), (Some("a.quill.md"), 1));
    assert_eq!(errs[1].to_string().split(':').next(), Some("a.quill.md"));
    assert!(errs[0].to_string().starts_with("line 2: "));
}

#[test]
fn an_unclosed_if_in_an_included_file_points_into_it() {
    let e = errors("::include{a.quill.md}\n", &[("a.quill.md", "Hi.\n::if{1 == 1}\nx\n")]);
    assert!(e.iter().any(|m| m.starts_with("a.quill.md:2: ") && m.contains("never closed")), "{e:?}");
}

#[test]
fn a_missing_file_is_an_error_on_the_include_line() {
    let e = errors("Hi.\n::include{gone.quill.md}\n", &[]);
    assert!(e[0].starts_with("main.quill.md:2: ") && e[0].contains("gone.quill.md") && e[0].contains("no such file"), "{e:?}");
}

#[test]
fn front_matter_in_an_included_file_is_refused() {
    let e = errors("::include{a.quill.md}\n", &[("a.quill.md", "---\ntitle: X\n---\nHi.\n")]);
    assert_eq!(e.len(), 1, "{e:?}");
    assert!(e[0].starts_with("a.quill.md:1: ") && e[0].contains("main script only"), "{e:?}");
}

#[test]
fn a_cycle_is_an_error() {
    let e = errors("::include{a.quill.md}\n", &[("a.quill.md", "::include{b.quill.md}\n"), ("b.quill.md", "::include{a.quill.md}\n")]);
    assert!(e.iter().any(|m| m.starts_with("b.quill.md:1: ") && m.contains("cycle") && m.contains("a.quill.md -> b.quill.md -> a.quill.md")), "{e:?}");
    // the main script included from a file
    let e = errors("::include{a.quill.md}\n", &[("a.quill.md", "::include{main.quill.md}\n")]);
    assert!(e.iter().any(|m| m.starts_with("a.quill.md:1: ") && m.contains("cycle")), "{e:?}");
    // a file including itself
    let e = errors("::include{a.quill.md}\n", &[("a.quill.md", "::include{./a.quill.md}\n")]);
    assert!(e.iter().any(|m| m.contains("cycle")), "{e:?}");
}

#[test]
fn the_same_file_twice_is_an_error_even_from_different_places() {
    let files = [("a.quill.md", "# a\nHi.\n"), ("b.quill.md", "::include{a.quill.md}\n")];
    let e = errors("::include{a.quill.md}\n::include{b.quill.md}\n", &files);
    assert!(e.iter().any(|m| m.starts_with("b.quill.md:1: ") && m.contains("included twice") && m.contains("main.quill.md:1")), "{e:?}");
    let e = errors("::include{a.quill.md}\n::include{A.QUILL.md}\n", &[("a.quill.md", "Hi.\n")]);
    assert!(e.iter().any(|m| m.starts_with("main.quill.md:2: ") && m.contains("twice")), "{e:?}");
}

#[test]
fn a_label_defined_in_two_files_names_both_places() {
    let e = errors("# start\n::include{a.quill.md}\n", &[("a.quill.md", "Hi.\n# start\nAgain.\n")]);
    assert_eq!(e.len(), 1, "{e:?}");
    assert!(e[0].starts_with("a.quill.md:2: ") && e[0].contains("duplicate label `start`") && e[0].contains("main.quill.md:1"), "{e:?}");
    let e = errors("::include{a.quill.md}\n::include{b.quill.md}\n", &[("a.quill.md", "# x\n"), ("b.quill.md", "\n# x\n")]);
    assert!(e[0].starts_with("b.quill.md:2: ") && e[0].contains("a.quill.md:1"), "{e:?}");
}

#[test]
fn paths_must_stay_inside_the_game_folder_and_look_like_scripts() {
    for path in ["../x.quill.md", "/etc/x.quill.md", "a\\b.quill.md", "C:x.quill.md", "a/../../x.quill.md"] {
        let e = errors(&format!("::include{{{path}}}\n"), &[]);
        assert!(e[0].contains("inside the game folder"), "{path}: {e:?}");
    }
    let e = errors("::include{notes.txt}\n", &[("notes.txt", "x")]);
    assert!(e[0].contains(".quill.md"), "{e:?}");
    let e = errors("::include{{x}.quill.md}\n", &[]);
    assert!(e[0].contains("braces"), "{e:?}");
    let e = errors("::include\n", &[]);
    assert!(e[0].contains("needs a file"), "{e:?}");
    let e = errors("::include{}\n", &[]);
    assert!(e[0].contains("needs a file"), "{e:?}");
}

#[test]
fn a_plain_string_has_no_game_folder() {
    let e = parse("::include{a.quill.md}\n").unwrap_err();
    assert_eq!(e.len(), 1);
    assert!(e[0].message.contains("game folder"), "{e:?}");
}

#[test]
fn nesting_depth_is_capped() {
    let chain = |n: usize| -> Vec<(String, String)> {
        (1..=n)
            .map(|i| (format!("f{i}.quill.md"), if i < n { format!("::include{{f{}.quill.md}}\n", i + 1) } else { "End.\n".to_string() }))
            .collect()
    };
    let run = |n: usize| {
        let files = chain(n);
        let refs: Vec<(&str, &str)> = files.iter().map(|(a, b)| (a.as_str(), b.as_str())).collect();
        load("::include{f1.quill.md}\n", &refs)
    };
    assert!(run(MAX_INCLUDE_DEPTH).is_ok());
    let e = run(MAX_INCLUDE_DEPTH + 1).unwrap_err();
    assert!(e[0].message.contains("deep") && e[0].file.as_deref() == Some("f8.quill.md"), "{e:?}");
}

#[test]
fn the_number_of_files_is_capped() {
    let main: String = (0..=MAX_INCLUDES).map(|i| format!("::include{{f{i}.quill.md}}\n")).collect();
    let parsed = parse_files(&main, "main.quill.md", &|_| Ok("Hi.\n".into()));
    let e = parsed.unwrap_err();
    assert_eq!(e.len(), 1, "{e:?}");
    assert!(e[0].message.contains("at most 200") && e[0].line == MAX_INCLUDES + 1, "{e:?}");
    let ok: String = (0..MAX_INCLUDES).map(|i| format!("::include{{f{i}.quill.md}}\n")).collect();
    assert_eq!(parse_files(&ok, "m.quill.md", &|_| Ok("Hi.\n".into())).unwrap().sources.len(), MAX_INCLUDES);
}

#[test]
fn the_total_size_is_capped() {
    let big = "a\n".repeat(MAX_INCLUDE_BYTES / 2 / 2 + 10);
    let main = "::include{a.quill.md}\n::include{b.quill.md}\n::include{c.quill.md}\n";
    let e = parse_files(main, "m.quill.md", &|_| Ok(big.clone())).unwrap_err();
    assert!(e.iter().any(|e| e.message.contains("too much script") && e.line == 3), "{e:?}");
}
