//! Tests for the third review's runtime findings: hostile expressions in saves
//! (stack depth) and the size of a save.

use quillstone_script::{parse, HudExpr, Program, Value};

use crate::test_util::replace_records;
use crate::{Input, Session, MAX_LIST_ITEMS, MAX_SAVE_BYTES};

fn program() -> Program {
    parse("---\nvars: { gold: 2 }\n---\n# start\nHello.\n::hud{id=g text=\"Gold {gold}\"}\nBye.\n").unwrap()
}

/// A thread with a 1 MB stack, like a Windows main thread (a debug build needs more than 512 KB to read a save with
/// an expression nested to the limit).
fn small_stack<F: FnOnce() + Send + 'static>(f: F) {
    std::thread::Builder::new().stack_size(1024 * 1024).spawn(f).unwrap().join().expect("overflowed or panicked");
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
                format!("hud text \"a\" 0 0 \"{src}\" \"x\" - - - 0"),
                format!("hud bar \"a\" 0 0 \"\" \"{src}\" \"2\" 10 10 - 0,0,0,1 - 0"),
            ] {
                assert!(Session::load(&p, &replace_records(&save, "hud ", &[rec])).is_err(), "{}", &src[..20]);
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
    let many: Vec<String> = (0..100).map(|i| format!("hud text \"e{i}\" 0 0 \"\" \"{text}\" - - - 0")).collect();
    let e = Session::load(&p, &replace_records(&save, "hud ", &many)).err().expect("refused");
    assert!(e.contains("bytes of text"), "{e}");
    // a normal handful is fine
    let few = &many[..10];
    assert!(Session::load(&p, &replace_records(&save, "hud ", few)).is_ok());
}

/// A story whose list holds `items` entries from the start, then says `lines` lines.
fn list_story(items: usize, lines: usize, body: &str) -> String {
    let bag: Vec<String> = (0..items).map(|i| format!("\"item-{i}\"")).collect();
    let says: String = (0..lines).map(|i| format!("Line {i}.\n\n")).collect();
    format!("---\nvars: {{ bag: [{}] }}\n---\n# a\n{body}{says}", bag.join(", "))
}

#[test]
fn a_big_list_and_many_lines_still_save_under_the_limit_and_load() {
    let p = parse(&list_story(9_000, 130, "")).unwrap();
    let (mut game, _) = Session::start(&p);
    for _ in 0..120 {
        game.input(Input::Advance);
    }
    let save = game.save();
    assert!(save.len() <= MAX_SAVE_BYTES - 64 * 1024, "room for a slot header: {} bytes", save.len());
    let (loaded, _) = Session::load(&p, &save).unwrap();
    assert_eq!(loaded.state().pc, game.state().pc, "the present state is the one that was saved");
    assert!(game.history.len() > 100 && loaded.history.len() < 100, "oldest history dropped: {}", loaded.history.len());
}

#[test]
fn a_list_stops_growing_at_the_limit_and_its_save_loads() {
    let p = parse(&list_story(MAX_LIST_ITEMS - 1, 1, "::set{bag += \"x\"}\n::set{bag += \"y\"}\n")).unwrap();
    let (game, _) = Session::start(&p);
    assert!(matches!(&game.state().vars["bag"], Value::List(l) if l.len() == MAX_LIST_ITEMS));
    let (loaded, _) = Session::load(&p, &game.save()).unwrap();
    assert_eq!(loaded.state().vars["bag"], game.state().vars["bag"]);
}

#[test]
fn a_story_that_makes_over_a_thousand_variables_saves_and_loads() {
    let flags: String = (0..1_100).map(|i| format!("::set{{flag{i}=true}}\n")).collect();
    let options: String = (0..1_100).map(|i| format!("- [Option {i}](#a){{once}}\n")).collect();
    let p = parse(&format!("# a\n{flags}::choice\n{options}- [Leave](#end)\n::\n# end\nDone.\n")).unwrap();
    let (mut game, _) = Session::start(&p);
    for _ in 0..1_100 {
        game.input(Input::Choose(0));
    }
    assert!(game.state().vars.len() >= 2_200, "{} variables", game.state().vars.len());
    let (loaded, _) = Session::load(&p, &game.save()).unwrap();
    assert_eq!(loaded.state().vars, game.state().vars);
}
