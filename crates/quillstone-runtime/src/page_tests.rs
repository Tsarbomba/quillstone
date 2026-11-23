//! Full-screen pages in the runtime: what joins and clears the page, the cap, rollback, saves
//! (old, round trip, hostile), a Use that comes back to a page line, languages and the tester.

use quillstone_script::{parse, Catalog, Place, Program, Value};

use crate::test_util::{next, replace_records};
use crate::{explore, Cmd, Input, Session, MAX_PAGE_LINES};

const SRC: &str = "\
---
vars: { n: 0 }
---
::character{id=mika name=Mika color=#ffd98c}
# start
Before.
::page
One.

**Mika:** Two.

Three.
::choice
- [Left](#left)
- [Right](#right)
::
# left
Four.
::page
Five.
::page{off}
Back in the box.
::end
# right
::page{off}
Plain.
";

fn program() -> Program {
    parse(SRC).unwrap()
}

/// The page as the player reads it: the lines before the current one, then the current one when it is a page line.
fn texts<'a>(game: &'a Session) -> Vec<&'a str> {
    let state = game.state();
    let line = state.line.as_ref().filter(|l| l.place == Place::Page);
    state.page.iter().chain(line).map(|l| l.text.as_str()).collect()
}

/// The page the commands show: the lines of the last `Page` and the page line a `Say` shows after them.
fn page_of(cmds: &[Cmd]) -> Option<Vec<String>> {
    let before = cmds.iter().rev().find_map(|c| if let Cmd::Page { lines } = c { Some(lines.iter().map(|l| l.text.clone()).collect::<Vec<_>>()) } else { None });
    let line = cmds.iter().rev().find_map(|c| if let Cmd::Say { text, place: Place::Page, .. } = c { Some(text.clone()) } else { None });
    (before.is_some() || line.is_some()).then(|| before.into_iter().flatten().chain(line).collect())
}

#[test]
fn page_lines_accumulate_and_each_waits_like_a_textbox_line() {
    let p = program();
    let (mut game, first) = Session::start(&p);
    assert!(page_of(&first).is_none(), "the textbox line sends no page");
    let cmds = next(&mut game);
    assert_eq!(page_of(&cmds).unwrap(), ["One."]);
    assert!(matches!(game.state().pending, Some(Cmd::Say { .. })));
    let cmds = next(&mut game);
    assert_eq!(page_of(&cmds).unwrap(), ["One.", "Two."]);
    let line = game.state().line.as_ref().unwrap();
    assert_eq!(line.speaker.as_deref(), Some("Mika"));
    assert!(line.color.is_some());
    assert_eq!(game.state().page.len(), 1, "the page holds the lines before the one waiting");
    next(&mut game);
    assert_eq!(texts(&game), ["One.", "Two.", "Three."]);
    assert_eq!(game.state().line.as_ref().map(|l| l.text.as_str()), Some("Three."));
}

#[test]
fn a_choice_does_not_clear_the_page_and_the_redraw_brings_it_back() {
    let p = program();
    let (mut game, _) = Session::start(&p);
    for _ in 0..4 {
        next(&mut game);
    }
    assert!(matches!(game.state().pending, Some(Cmd::ShowChoices { .. })));
    assert_eq!(texts(&game), ["One.", "Two.", "Three."]);
    let redraw = game.state().redraw();
    let at = redraw.iter().position(|c| matches!(c, Cmd::Page { .. })).unwrap();
    assert!(matches!(&redraw[at + 1], Cmd::Say { text, .. } if text == "Three."), "the page, then the newest line");
    assert_eq!(page_of(&redraw).unwrap().len(), 3);
}

#[test]
fn the_page_op_a_textbox_line_and_a_video_clear_it() {
    let p = program();
    let (mut game, _) = Session::start(&p);
    for _ in 0..4 {
        next(&mut game);
    }
    let cmds = game.input(Input::Choose(0)); // Four. (still a page line), then `::page`, Five.
    assert_eq!(page_of(&cmds).unwrap(), ["One.", "Two.", "Three.", "Four."]);
    let cmds = next(&mut game);
    assert_eq!(cmds.iter().filter_map(|c| if let Cmd::Page { lines } = c { Some(lines.len()) } else { None }).collect::<Vec<_>>(), [0]);
    assert_eq!(texts(&game), ["Five."]);
    let cmds = next(&mut game); // `::page{off}`: the textbox line takes the page away
    assert_eq!(page_of(&cmds).unwrap().len(), 0);
    assert!(game.state().page.is_empty());

    let v = parse("::page\nOne.\n::video{src=c.webm}\nTwo.\n").unwrap();
    let (mut game, _) = Session::start(&v);
    let cmds = next(&mut game);
    assert!(game.state().page.is_empty() && game.state().line.is_none());
    assert_eq!(page_of(&cmds).unwrap().len(), 0);
}

#[test]
fn a_page_op_with_a_choice_after_it_leaves_no_line_behind() {
    let p = parse("::page\nOne.\n::page\n::choice\n- [A](#a)\n::\n# a\nEnd.\n").unwrap();
    let (mut game, _) = Session::start(&p);
    next(&mut game);
    assert!(matches!(game.state().pending, Some(Cmd::ShowChoices { .. })));
    assert!(game.state().page.is_empty() && game.state().line.is_none());
    assert!(!game.state().redraw().iter().any(|c| matches!(c, Cmd::Page { .. } | Cmd::Say { .. })));
}

#[test]
fn the_sixty_fifth_page_line_starts_a_fresh_page() {
    let mut src = String::from("::page\n");
    for i in 0..MAX_PAGE_LINES + 2 {
        src.push_str(&format!("Line {i}.\n\n"));
    }
    let p = parse(&src).unwrap();
    let (mut game, _) = Session::start(&p);
    for _ in 0..MAX_PAGE_LINES - 1 {
        next(&mut game);
    }
    assert_eq!(game.state().page.len(), MAX_PAGE_LINES - 1);
    assert_eq!(game.state().page[0].text, "Line 0.");
    let cmds = next(&mut game);
    assert_eq!(page_of(&cmds).unwrap(), [format!("Line {MAX_PAGE_LINES}.")]);
    next(&mut game);
    assert_eq!(texts(&game), [format!("Line {MAX_PAGE_LINES}."), format!("Line {}.", MAX_PAGE_LINES + 1)]);
}

#[test]
fn rollback_and_roll_forward_restore_the_page_exactly() {
    let p = program();
    let (mut game, _) = Session::start(&p);
    let mut seen = vec![(game.state().page.clone(), texts(&game).len())];
    for _ in 0..4 {
        next(&mut game);
        seen.push((game.state().page.clone(), texts(&game).len()));
    }
    for (want, shown) in seen.iter().rev().skip(1) {
        let cmds = game.rollback().unwrap();
        assert_eq!(&game.state().page, want);
        assert_eq!(page_of(&cmds).unwrap_or_default().len(), *shown);
    }
    for (want, _) in seen.iter().skip(1) {
        game.roll_forward().unwrap();
        assert_eq!(&game.state().page, want);
    }
}

#[test]
fn a_page_script_saves_and_loads_at_every_step() {
    let p = program();
    let (mut game, _) = Session::start(&p);
    for _ in 0..12 {
        let (loaded, cmds) = Session::load(&p, &game.save()).unwrap();
        assert_eq!(loaded.state(), game.state());
        assert_eq!(loaded.state().page, game.state().page);
        assert_eq!(page_of(&cmds).unwrap_or_default().len(), texts(&game).len());
        if game.state().finished {
            break;
        }
        match game.state().pending {
            Some(Cmd::ShowChoices { .. }) => game.input(Input::Choose(0)),
            _ => game.input(Input::Advance),
        };
    }
    assert!(game.state().finished);
}

#[test]
fn awkward_page_text_round_trips() {
    let p = parse("::page\n**Mika \"M\":** Tab\\there, \"quotes\" and a \\\\ slash.\n\nPlain \u{fc}\u{ef} \u{2713}.\n").unwrap();
    let (mut game, _) = Session::start(&p);
    next(&mut game);
    let (loaded, _) = Session::load(&p, &game.save()).unwrap();
    assert_eq!(loaded.state().page, game.state().page);
    assert_eq!(loaded.state().line, game.state().line);
    assert_eq!(loaded.state().page.len(), 1);
}

#[test]
fn a_save_from_before_pages_loads_with_an_empty_page_and_a_clean_game_writes_no_page_records() {
    let p = program();
    let (mut game, _) = Session::start(&p);
    assert!(!game.save().contains("\npage "), "a textbox game writes nothing new");
    next(&mut game);
    next(&mut game);
    let save = game.save();
    assert!(save.contains("\npage "));
    let old = replace_records(&save, "page ", &[] as &[&str]);
    let (loaded, _) = Session::load(&p, &old).unwrap();
    assert!(loaded.state().page.is_empty());
    assert_eq!(loaded.state().line, game.state().line);
}

#[test]
fn broken_page_data_in_a_save_is_an_error_not_a_panic() {
    let p = program();
    let (mut game, _) = Session::start(&p);
    next(&mut game);
    let save = game.save();
    let bad = |records: &[String]| Session::load(&p, &replace_records(&save, "page ", records)).is_err();
    assert!(bad(&["page".to_string()]), "too short");
    assert!(bad(&["page - - ".to_string()]), "no text");
    assert!(bad(&["page - nonsense \"x\"".to_string()]), "bad colour");
    assert!(bad(&["page - - \"unclosed".to_string()]), "unclosed quote");
    let too_many = vec!["page - - \"x\"".to_string(); MAX_PAGE_LINES + 1];
    assert!(bad(&too_many), "more than the cap");
    let full = vec!["page - - \"x\"".to_string(); MAX_PAGE_LINES];
    assert!(!bad(&full), "exactly the cap is fine");
}

#[test]
fn using_an_item_from_a_page_line_shows_the_line_again_once() {
    let src = "---\ninventory: bag\nvars: { bag: [], n: 0 }\n---\n::item{id=pill name=Pill use=#eat}\n::set{bag+=\"pill\"}\n::page\nOne.\n\nTwo.\n\nThree.\n# eat\n::set{n+=1}\n::return\n";
    let p = parse(src).unwrap();
    let (mut game, _) = Session::start(&p);
    next(&mut game);
    assert_eq!(texts(&game), ["One.", "Two."]);
    let cmds = game.input(Input::Use("pill".into()));
    assert!(!cmds.is_empty());
    assert_eq!(texts(&game), ["One.", "Two."], "the line is not on the page twice");
    assert_eq!(page_of(&cmds).unwrap(), ["Two."], "the page above it is still on the screen: only the line is shown again");
}

#[test]
fn a_page_line_that_a_persistent_value_changed_joins_the_page_once_after_a_use() {
    let src = "---\npersistent: { k: 0 }\ninventory: bag\nvars: { bag: [] }\n---\n::item{id=pill name=Pill use=#eat}\n::set{bag+=\"pill\"}\n::page\nOne.\n\nSeen {k}.\n\nThree.\n# eat\n::return\n";
    let p = parse(src).unwrap();
    let (mut game, _) = Session::start(&p);
    next(&mut game);
    assert_eq!(texts(&game), ["One.", "Seen 0."]);
    game.set_persistent([("k".to_string(), Value::Int(5))].into_iter().collect());
    assert_eq!(texts(&game), ["One.", "Seen 5."], "the page shows the refreshed line");
    let cmds = game.input(Input::Use("pill".into()));
    assert_eq!(page_of(&cmds).unwrap(), ["Seen 5."]);
    assert_eq!(texts(&game), ["One.", "Seen 5."]);
}

#[test]
fn page_lines_are_translated_the_newest_and_the_earlier_ones_alike() {
    let p = parse("::page\nOne.\n\nTwo.\n\nThree.\n").unwrap();
    let cat = Catalog::parse("> One.\n= Ett.\n\n> Two.\n= Tva.\n\n> Three.\n= Tre.\n").unwrap();
    let (mut game, _) = Session::start(&p);
    next(&mut game);
    next(&mut game);
    let cmds = game.localize(game.state().redraw(), &cat);
    assert_eq!(page_of(&cmds).unwrap(), ["Ett.", "Tva.", "Tre."]);
    let cmds = game.relocalized(&cat);
    assert_eq!(page_of(&cmds).unwrap(), ["Ett.", "Tva.", "Tre."]);
}

#[test]
fn the_tester_finds_as_many_situations_with_pages_as_without() {
    let body = |page: &str| {
        let mut s = format!("{page}\n");
        for i in 0..80 {
            s.push_str(&format!("Line {i}.\n\n"));
        }
        s + "::choice\n- [A](#a)\n- [B](#b)\n::\n# a\nA.\n::end\n# b\n::page{off}\nB.\n"
    };
    let (with, without) = (explore(&parse(&body("::page")).unwrap(), 100_000), explore(&parse(&body("")).unwrap(), 100_000));
    assert!(!with.partial && with.errors.is_empty());
    assert_eq!((with.situations, with.endings.len()), (without.situations, without.endings.len()));
}

#[test]
fn a_centred_line_stays_centred_under_choices_after_a_load_and_a_language_change() {
    let p = parse("::center Later.\n::choice\n- [Go](#go)\n::\n# go\n**A:** end\n").unwrap();
    let centred = |cmds: &[Cmd]| cmds.iter().rev().find_map(|c| if let Cmd::Say { place, .. } = c { Some(*place == Place::Centre) } else { None });
    let (mut game, first) = Session::start(&p);
    assert_eq!(centred(&first), Some(true));
    let (loaded, cmds) = Session::load(&p, &game.save()).unwrap();
    assert_eq!(centred(&cmds), Some(true), "loaded while waiting at it");
    assert_eq!(centred(&loaded.relocalized(&Catalog::parse("").unwrap())), Some(true), "a language change while waiting at it");
    next(&mut game);
    assert!(matches!(game.state().pending, Some(Cmd::ShowChoices { .. })));
    assert_eq!(centred(&game.state().redraw()), Some(true), "redraw under choices");
    let (loaded, cmds) = Session::load(&p, &game.save()).unwrap();
    assert_eq!(centred(&cmds), Some(true), "loaded under choices");
    assert_eq!(centred(&loaded.relocalized(&Catalog::parse("").unwrap())), Some(true), "a language change under choices");
}

#[test]
fn a_label_called_from_a_page_line_with_page_lines_of_its_own_leaves_the_line_on_the_page_once() {
    let src = "---\ninventory: bag\nvars: { bag: [] }\n---\n::item{id=pill name=Pill use=#eat}\n::set{bag+=\"pill\"}\n::page\nOne.\n\nTwo.\n\nThree.\n# eat\nNope.\n::return\n";
    let p = parse(src).unwrap();
    let (mut game, _) = Session::start(&p);
    next(&mut game);
    game.input(Input::Use("pill".into()));
    assert_eq!(texts(&game), ["One.", "Nope."]);
    next(&mut game);
    assert_eq!(texts(&game), ["One.", "Nope.", "Two."]);
}

#[test]
fn a_save_that_wrote_the_page_line_among_the_page_records_loads_it_as_the_line() {
    let p = program();
    let (mut game, _) = Session::start(&p);
    next(&mut game);
    next(&mut game);
    let save = game.save();
    assert!(save.contains("\"Two.\" page\n"), "the line says where it is shown");
    let mut old = String::new();
    for l in save.lines() {
        match l.strip_prefix("line ").and_then(|r| r.strip_suffix(" page")) {
            Some(rest) => old.push_str(&format!("page {rest}\nline {rest}\n")),
            None => old.push_str(&format!("{l}\n")),
        }
    }
    let (loaded, cmds) = Session::load(&p, &old).unwrap();
    assert_eq!(loaded.state().page.len(), 1);
    assert_eq!(loaded.state().line.as_ref().map(|l| (l.text.as_str(), l.place)), Some(("Two.", Place::Page)));
    assert!(matches!(&loaded.state().pending, Some(Cmd::Say { place: Place::Page, .. })));
    assert_eq!(page_of(&cmds).unwrap(), ["One.", "Two."]);
}
