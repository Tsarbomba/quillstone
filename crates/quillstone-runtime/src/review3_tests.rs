//! Tests for the third review's runtime findings: hostile expressions in saves
//! (stack depth) and the size of a save.

use quillstone_script::{parse, HudExpr, Program};

use crate::{Session, MAX_SAVE_BYTES};

fn program() -> Program {
    parse("---\nvars: { gold: 2 }\n---\n# start\nHello.\n::hud{id=g text=\"Gold {gold}\"}\nBye.\n").unwrap()
}

/// The first state's `hud` records replaced by `lines` (as in `hud_tests`).
fn with_hud_records(save: &str, lines: &[String]) -> String {
    let mut out = String::new();
    let mut done = false;
    for l in save.lines().filter(|l| !l.starts_with("hud ")) {
        out.push_str(l);
        out.push('\n');
        if l.starts_with("rng ") && !done {
            done = true;
            for h in lines {
                out.push_str(h);
                out.push('\n');
            }
        }
    }
    out
}

/// A thread with a 512 KB stack: smaller than a Windows main thread (1 MB).
fn small_stack<F: FnOnce() + Send + 'static>(f: F) {
    std::thread::Builder::new().stack_size(512 * 1024).spawn(f).unwrap().join().expect("overflowed or panicked");
}

#[test]
fn deeply_nested_hud_expressions_are_errors_not_stack_overflows() {
    small_stack(|| {
        let shapes = [
            format!("{}1{}", "(".repeat(5000), ")".repeat(5000)),
            format!("{}x", "-".repeat(5000)),
            format!("{}x", "not ".repeat(5000)),
        ];
        for src in &shapes {
            assert!(HudExpr::parse(src).is_err());
        }
        let p = program();
        let (game, _) = Session::start(&p);
        let save = game.save();
        // within the 2,000 byte limit of a hud expression
        let shapes = [
            format!("{}1{}", "(".repeat(990), ")".repeat(990)),
            format!("{}x", "-".repeat(1900)),
            format!("{}x", "not ".repeat(480)),
            format!("{}1{}", "abs(".repeat(480), ")".repeat(480)),
            vec!["1"; 990].join("+"),
        ];
        for src in &shapes {
            for rec in [
                format!("hud text \"a\" 0 0 left \"{src}\" \"x\" - - - 0"),
                format!("hud bar \"a\" 0 0 left \"\" \"{src}\" \"2\" 10 10 - 0,0,0,1 - 0"),
            ] {
                assert!(Session::load(&p, &with_hud_records(&save, &[rec])).is_err(), "{}", &src[..20]);
            }
        }
    });
}

#[test]
fn a_save_over_the_size_limit_is_refused_before_it_is_parsed() {
    let p = program();
    let (game, _) = Session::start(&p);
    let mut save = game.save();
    assert!(Session::load(&p, &save).is_ok());
    // valid records, but too many bytes: the cap is on the file, not the content
    while save.len() <= MAX_SAVE_BYTES {
        save.push_str("\n\n\n\n\n\n\n\n\n\n\n\n\n\n\n\n\n\n\n\n\n\n\n\n\n\n\n\n\n\n\n\n\n\n\n\n\n\n\n\n\n\n\n\n\n\n\n\n\n\n");
        save.push_str(&" ".repeat(60_000));
    }
    let e = Session::load(&p, &save).err().expect("refused");
    assert!(e.contains("larger than"), "{e}");
}

#[test]
fn hud_text_in_one_state_is_capped_in_total() {
    let p = program();
    let (game, _) = Session::start(&p);
    let save = game.save();
    // 100 records of 1,500 bytes each: every record is fine alone and under the record count
    let text = "x".repeat(1500);
    let many: Vec<String> = (0..100).map(|i| format!("hud text \"e{i}\" 0 0 left \"\" \"{text}\" - - - 0")).collect();
    let e = Session::load(&p, &with_hud_records(&save, &many)).err().expect("refused");
    assert!(e.contains("bytes of text"), "{e}");
    // a normal handful is fine
    let few = &many[..10];
    assert!(Session::load(&p, &with_hud_records(&save, few)).is_ok());
}
