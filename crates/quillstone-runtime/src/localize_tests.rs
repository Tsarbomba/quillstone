//! Presenting the story in another language: the runtime keeps the script's text, and
//! [`Session::localize`] and friends translate at the edge.

use quillstone_script::{parse, Catalog, Program};

use crate::{Cmd, Input, Logged, Session};

const SRC: &str = "\
---
vars: { gold: 5 }
---
::character{id=mika name=Mika}
# start
**Mika:** You have {gold} coins.

Plain narration.

**Mika:** Yes.

::set{gold += 1}
**Ren:** Yes.

::choice
- [Go left](#left)
- [Go right {gold}](#left)
::
# left
Done.
";

const SV: &str = "\
> Mika\n= Mikaela\n
> You have {gold} coins.\n= Du har {gold} mynt.\n
> Plain narration.\n= Vanlig berättelse.\n
> Yes.\n= Ja.\n
@ Mika\n> Yes.\n= Jo.\n
> Go left\n= Gå vänster\n
> Go right {gold}\n= Gå höger {gold}\n
> Ren\n=\n
";

fn program() -> Program {
    parse(SRC).unwrap()
}

fn sv() -> Catalog {
    Catalog::parse(SV).unwrap()
}

fn say(cmds: &[Cmd]) -> (Option<String>, String) {
    match cmds.last() {
        Some(Cmd::Say { speaker, text, .. }) => (speaker.clone(), text.clone()),
        other => panic!("not a line: {other:?}"),
    }
}

#[test]
fn a_line_is_looked_up_before_the_values_go_in() {
    let p = program();
    let (s, cmds) = Session::start(&p);
    assert_eq!(say(&cmds), (Some("Mika".into()), "You have 5 coins.".into()));
    let t = s.localize(cmds, &sv());
    assert_eq!(say(&t), (Some("Mikaela".into()), "Du har 5 mynt.".into()));
    assert_eq!(s.state().line.as_ref().unwrap().text, "You have 5 coins.", "the state stays in the script's language");
}

#[test]
fn an_empty_catalog_changes_nothing() {
    let p = program();
    let (mut s, mut cmds) = Session::start(&p);
    let empty = Catalog::default();
    loop {
        assert_eq!(s.localize(cmds.clone(), &empty), cmds);
        assert_eq!(s.relocalized(&empty), s.state().redraw()[s.state().redraw().len() - s.relocalized(&empty).len()..]);
        if matches!(s.state().pending, Some(Cmd::ShowChoices { .. })) {
            break;
        }
        cmds = s.input(Input::Advance);
    }
}

#[test]
fn a_speaker_context_beats_the_general_entry_and_a_missing_entry_falls_back() {
    let p = program();
    let (mut s, _) = Session::start(&p);
    s.input(Input::Advance);
    let cmds = s.input(Input::Advance); // Mika: Yes.
    assert_eq!(say(&s.localize(cmds, &sv())).1, "Jo.");
    let cmds = s.input(Input::Advance); // Ren: Yes. (gold set in between)
    let (name, text) = say(&s.localize(cmds, &sv()));
    assert_eq!((name.as_deref(), text.as_str()), (Some("Ren"), "Ja."), "an empty `=` is untranslated");
}

#[test]
fn choices_are_translated_and_the_line_under_them_keeps_its_own_values() {
    let p = program();
    let (mut s, _) = Session::start(&p);
    for _ in 0..4 {
        s.input(Input::Advance);
    }
    assert!(matches!(s.state().pending, Some(Cmd::ShowChoices { .. })));
    let cmds = s.relocalized(&sv());
    let n = cmds.len();
    assert_eq!(cmds[n - 1], Cmd::ShowChoices { options: vec!["Gå vänster".into(), "Gå höger 6".into()] });
    assert!(matches!(&cmds[n - 2], Cmd::Say { text, .. } if text == "Ja."));
}

#[test]
fn the_backlog_follows_the_language_and_rollback_and_saves_stay_in_the_script_language() {
    let p = program();
    let (mut s, _) = Session::start(&p);
    for _ in 0..4 {
        s.input(Input::Advance);
    }
    s.input(Input::Choose(1));
    let log = s.backlog_with(&sv());
    let texts: Vec<String> = log
        .iter()
        .map(|l| match l {
            Logged::Line(l) => l.text.clone(),
            Logged::Choice(c) => format!("> {c}"),
        })
        .collect();
    assert_eq!(texts, ["Du har 5 mynt.", "Vanlig berättelse.", "Jo.", "Ja.", "> Gå höger 6", "Done."]);
    assert_eq!(s.backlog().len(), log.len());
    // rollback and save/load after "switching": the data is the script's own
    s.rollback().unwrap();
    assert!(s.save().contains("You have") || !s.save().contains("Du har"));
    assert!(!s.save().contains("mynt") && !s.save().contains("Mikaela"));
    let (loaded, cmds) = Session::load(&p, &s.save()).unwrap();
    assert_eq!(loaded.backlog_with(&sv()).len(), s.backlog().len());
    let cmds = loaded.localize(cmds, &sv());
    assert!(cmds.iter().any(|c| matches!(c, Cmd::ShowChoices { options } if options[0] == "Gå vänster")));
}

#[test]
fn items_inputs_and_hud_text_are_translated() {
    let p = parse(
        "---\nvars: { name: \"A\", bag: [lamp] }\ninventory: bag\n---\n::item{id=lamp name=Lamp text=\"A small lamp.\"}\n\
         ::hud{id=h text=\"Gold {name}\"}\n::input{var=name prompt=\"Your {name}?\"}\n",
    )
    .unwrap();
    let cat = Catalog::parse("> Lamp\n= Lampa\n\n> A small lamp.\n= En liten lampa.\n\n> Gold {name}\n= Guld {name}\n\n> Your {name}?\n= Ditt {name}?\n").unwrap();
    let (s, cmds) = Session::start(&p);
    let t = s.localize(cmds, &cat);
    assert!(t.iter().any(|c| matches!(c, Cmd::HudShow { elem, .. } if format!("{:?}", elem.kind).contains("Guld {name}"))));
    assert!(t.iter().any(|c| matches!(c, Cmd::AskInput { prompt, .. } if prompt == "Ditt A?")));
    let mut info = crate::item_info(&p, "lamp");
    crate::localize_item(&mut info, &cat);
    assert_eq!((info.name.as_str(), info.text.as_str()), ("Lampa", "En liten lampa."));
}

#[test]
fn a_notice_is_looked_up_before_the_values_go_in_and_is_in_the_export() {
    let p = parse("---\nvars: { n: 2 }\n---\n::notify{text=\"Got {n}\" time=1}\n**A:** one\n").unwrap();
    assert!(p.translatable().iter().any(|s| s.text == "Got {n}"), "lang export lists it");
    let cat = Catalog::parse("> Got {n}\n= Fick {n}\n").unwrap();
    let (s, cmds) = Session::start(&p);
    assert!(matches!(&cmds[0], Cmd::Notify { text, .. } if text == "Got 2"));
    assert!(matches!(&s.localize(cmds, &cat)[0], Cmd::Notify { text, .. } if text == "Fick 2"));
}
