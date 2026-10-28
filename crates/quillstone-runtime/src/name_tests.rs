//! A character's display name may hold `{variable}`s, resolved when a line is shown.

use quillstone_script::{parse, Catalog, Program};

use crate::explore::explore;
use crate::{Cmd, Input, Logged, Session};

const SRC: &str = "\
---
vars: { hero.name: \"Alex\", hname: \"Zed\" }
---
::character{id=h name=\"{hero.name}\" color=#ffd98c}
::character{id=z name=\"{hname} \\{x}\"}
::character{id=mika name=Mika}
# start
**Mika:** Name?
::input{var=hero.name prompt=\"Name?\" max=12}
**h:** I am {hero.name}.
**{hname} \\{x}:** Second template.
**{hero.name}:** Nobody has this exact name text.
::set{hero.name = \"Bob\"}
**h:** Now Bob.
**{nope}:** Unknown speaker stays as written.
";

fn program() -> Program {
    parse(SRC).unwrap()
}

fn who(cmds: &[Cmd]) -> Option<String> {
    cmds.iter().rev().find_map(|c| match c {
        Cmd::Say { speaker, .. } => speaker.clone(),
        _ => None,
    })
}

fn typed<'p>(p: &'p Program, text: &str) -> (Session<'p>, Vec<Cmd>) {
    let (mut s, _) = Session::start(p);
    s.input(Input::Advance);
    let cmds = s.input(Input::Text(text.into()));
    (s, cmds)
}

#[test]
fn the_name_shows_the_current_value() {
    let p = program();
    let (mut s, cmds) = typed(&p, "Rin");
    assert_eq!(who(&cmds).as_deref(), Some("Rin"));
    assert_eq!(s.state().line.as_ref().unwrap().speaker.as_deref(), Some("Rin"));
    let cmds = s.input(Input::Advance);
    assert_eq!(who(&cmds).as_deref(), Some("Zed {x}"), "second template, escaped brace");
    let cmds = s.input(Input::Advance);
    assert_eq!(who(&cmds).as_deref(), Some("Rin"), "a prefix equal to the template finds the character");
    let cmds = s.input(Input::Advance);
    assert_eq!(who(&cmds).as_deref(), Some("Bob"));
    let cmds = s.input(Input::Advance);
    assert_eq!(who(&cmds).as_deref(), Some("{nope}"), "no such character: as written, like any unknown speaker");
}

#[test]
fn braces_and_stars_typed_by_the_player_stay_text() {
    let p = program();
    let (s, cmds) = typed(&p, "{b}*x*\\");
    assert_eq!(who(&cmds).as_deref(), Some("{b}*x*\\"));
    assert_eq!(s.state().line.as_ref().unwrap().speaker.as_deref(), Some("{b}*x*\\"));
}

#[test]
fn the_backlog_and_rollback_keep_the_name_of_the_time() {
    let p = program();
    let (mut s, _) = typed(&p, "Rin");
    for _ in 0..2 {
        s.input(Input::Advance); // up to the third line, spoken by the template text
    }
    let names: Vec<_> = s
        .backlog()
        .into_iter()
        .filter_map(|l| match l {
            Logged::Line(l) => l.speaker,
            _ => None,
        })
        .collect();
    assert_eq!(names, ["Mika", "Rin", "Zed {x}", "Rin"], "old lines keep the old name: {names:?}");
    // the line after the variable changed is Bob, the ones before still Rin
    s.input(Input::Advance);
    assert_eq!(s.backlog().iter().filter(|l| matches!(l, Logged::Line(l) if l.speaker.as_deref() == Some("Bob"))).count(), 1);
    let cmds = s.rollback().unwrap();
    assert_eq!(who(&cmds).as_deref(), Some("Rin"), "rolled back before the change");
}

#[test]
fn a_loaded_save_shows_the_same_names() {
    let p = program();
    let (mut s, _) = typed(&p, "Rin");
    s.input(Input::Advance);
    let (l, cmds) = Session::load(&p, &s.save()).unwrap();
    assert_eq!(who(&cmds).as_deref(), Some("Zed {x}"));
    assert_eq!(l.backlog().len(), s.backlog().len());
}

#[test]
fn the_catalog_is_keyed_on_the_raw_template_and_the_translated_name_is_filled() {
    let src = "---\nvars: { hero.name: \"Alex\" }\n---\n::character{id=h name=\"{hero.name}\"}\n**h:** Hi.\n";
    let p = parse(src).unwrap();
    let cat = Catalog::parse("> {hero.name}\n= Hjälten {hero.name}\n\n@ {hero.name}\n> Hi.\n= Hej.\n").unwrap();
    let (s, cmds) = Session::start(&p);
    assert_eq!(who(&cmds).as_deref(), Some("Alex"));
    let cmds = s.localize(cmds, &cat);
    match cmds.last() {
        Some(Cmd::Say { speaker, text, .. }) => assert_eq!((speaker.as_deref(), text.as_str()), (Some("Hjälten Alex"), "Hej.")),
        other => panic!("{other:?}"),
    }
    match s.backlog_with(&cat).last() {
        Some(Logged::Line(l)) => assert_eq!((l.speaker.as_deref(), l.text.as_str()), (Some("Hjälten Alex"), "Hej.")),
        other => panic!("{other:?}"),
    }
}

#[test]
fn check_counts_the_name_as_a_use() {
    let src = "---\nvars: { hero.name: \"A\" }\n---\n::character{id=h name=\"{hero.name}\"}\n**h:** Hi.\n";
    let r = explore(&parse(src).unwrap(), 1000);
    assert!(r.errors.is_empty(), "{:?}", r.errors.iter().map(|f| &f.message).collect::<Vec<_>>());
    // declared but set nowhere is the same as for a line: no warning for a declared variable
    let typo = parse("---\nvars: { hero.name: \"A\" }\n---\n::character{id=h name=\"{hero.nmae}\"}\n**h:** Hi.\n");
    assert!(typo.unwrap_err().iter().any(|e| e.line == 4 && e.message.contains("hero.nmae")));
}

#[test]
fn the_voice_name_does_not_depend_on_the_variable() {
    let p = program();
    let (s, _) = typed(&p, "Rin");
    let (t, _) = typed(&p, "Quentin");
    assert_eq!(crate::voice_name(&p, s.state().pc), crate::voice_name(&p, t.state().pc));
}
