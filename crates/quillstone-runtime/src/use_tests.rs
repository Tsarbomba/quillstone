//! Counted items (`count=`) and the Use action (`use=#label`): what the inventory lists, the shop, and
//! the call into the story.

use quillstone_script::{parse, Value};

use crate::explore::explore;
use crate::{can_use, inventory_items, Cmd, Input, Session};

const SRC: &str = "\
---
inventory: bag
vars: { gold: 12, potions: 2, hp: 5, bag: [key], who: \"\" }
---
::item{id=potion name=\"Potion\" price=5 count=potions use=#drink}
::item{id=key name=\"Key\"}
::item{id=gem name=\"Gem\" price=1}
# start
**A:** one
**A:** two
::choice
- [Go](#shop)
::
# shop
::shop{pay=gold into=bag}
- potion
- gem
::
::input{var=who prompt=\"Name?\"}
::wait{time=1}
**A:** end
::end
# drink
::set{potions -= 1}
::set{hp += 3}
**A:** Glug.
::return
";

fn int(s: &Session, var: &str) -> i64 {
    s.state().vars[var].as_int().unwrap()
}

fn shown(s: &Session) -> Vec<(String, Option<i64>)> {
    inventory_items(&parse(SRC).unwrap(), &s.state().vars).into_iter().map(|i| (i.name, i.count)).collect()
}

#[test]
fn a_counted_item_is_listed_while_its_count_is_above_zero_whatever_the_list_holds() {
    let p = parse(SRC).unwrap();
    let mut vars = p.vars.clone();
    let list = |v: &quillstone_script::Vars| inventory_items(&p, v).into_iter().map(|i| (i.name, i.count)).collect::<Vec<_>>();
    assert_eq!(list(&vars), [("Key".into(), None), ("Potion".into(), Some(2))], "not in the list, still shown");
    vars.insert("bag".into(), Value::List(vec!["potion".into(), "key".into()]));
    assert_eq!(list(&vars).len(), 2, "in the list as well: shown once");
    vars.insert("potions".into(), Value::Int(0));
    assert_eq!(list(&vars), [("Key".into(), None)], "listed but at 0: hidden");
    vars.insert("potions".into(), Value::Int(-1));
    assert_eq!(list(&vars), [("Key".into(), None)]);
}

#[test]
fn use_calls_the_label_from_the_line_and_comes_back_to_it() {
    let p = parse(SRC).unwrap();
    let (mut s, _) = Session::start(&p);
    assert!(can_use(&p, s.state(), "potion"));
    assert!(!can_use(&p, s.state(), "key"), "no use= label");
    let cmds = s.input(Input::Use("potion".into()));
    assert!(matches!(&cmds[..], [.., Cmd::Say { text, .. }] if text == "Glug."), "{cmds:?}");
    assert_eq!((int(&s, "potions"), int(&s, "hp")), (1, 8), "the label did the changes");
    assert_eq!(s.state().stack.len(), 1);
    let back = s.input(Input::Advance);
    assert!(matches!(&back[..], [.., Cmd::Say { text, .. }] if text == "one"), "back at the line: {back:?}");
    assert!(s.state().stack.is_empty());
    s.input(Input::Advance);
    assert_eq!(shown(&s)[1], ("Potion".into(), Some(1)));
}

#[test]
fn use_is_refused_unless_the_story_waits_at_a_line() {
    let p = parse(SRC).unwrap();
    let (mut s, _) = Session::start(&p);
    s.input(Input::Advance);
    s.input(Input::Advance);
    assert!(matches!(s.state().pending, Some(Cmd::ShowChoices { .. })));
    assert!(!can_use(&p, s.state(), "potion") && s.input(Input::Use("potion".into())).is_empty(), "over a choice");
    s.input(Input::Choose(0));
    assert!(matches!(s.state().pending, Some(Cmd::ShowShop { .. })));
    assert!(!can_use(&p, s.state(), "potion"), "over a shop");
    s.input(Input::Advance);
    assert!(matches!(s.state().pending, Some(Cmd::AskInput { .. })));
    assert!(!can_use(&p, s.state(), "potion") && s.input(Input::Use("potion".into())).is_empty(), "over an input");
    s.input(Input::Text("Bo".into()));
    assert!(matches!(s.state().pending, Some(Cmd::Wait { .. })));
    assert!(!can_use(&p, s.state(), "potion"), "over a wait");
    assert_eq!(int(&s, "potions"), 2);
    // and not when the count is 0, or for an item that isn't there
    let (mut s, _) = Session::start(&p);
    assert!(s.input(Input::Use("nothing".into())).is_empty());
    s.input(Input::Use("potion".into()));
    s.input(Input::Advance);
    s.input(Input::Use("potion".into()));
    s.input(Input::Advance);
    assert_eq!(int(&s, "potions"), 0);
    assert!(!can_use(&p, s.state(), "potion") && s.input(Input::Use("potion".into())).is_empty());
}

#[test]
fn rolling_back_a_use_returns_to_before_it() {
    let p = parse(SRC).unwrap();
    let (mut s, _) = Session::start(&p);
    s.input(Input::Use("potion".into()));
    assert_eq!(int(&s, "potions"), 1);
    s.rollback().expect("back");
    assert_eq!((int(&s, "potions"), int(&s, "hp"), s.state().stack.len()), (2, 5, 0));
    assert!(matches!(&s.state().pending, Some(Cmd::Say { text, .. }) if text == "one"));
    s.roll_forward().expect("and forward");
    assert_eq!(int(&s, "potions"), 1);
}

#[test]
fn a_save_made_inside_the_label_loads_and_finishes_the_use() {
    let p = parse(SRC).unwrap();
    let (mut s, _) = Session::start(&p);
    s.input(Input::Use("potion".into()));
    let text = s.save();
    assert!(text.contains("input use "));
    let (mut loaded, _) = Session::load(&p, &text).unwrap();
    assert_eq!(loaded.state(), s.state());
    assert_eq!(loaded.state().stack.len(), 1);
    loaded.input(Input::Advance);
    assert!(matches!(&loaded.state().pending, Some(Cmd::Say { text, .. }) if text == "one"));
    assert!(loaded.rollback().is_some() && loaded.rollback().is_some(), "the use is in the loaded history");
    assert_eq!(int(&loaded, "potions"), 2);
}

#[test]
fn using_an_item_does_not_count_as_another_visit_to_the_label_it_was_used_in() {
    let src = "---\ninventory: bag\nvars: { potions: 1, bag: [] }\n---\n::item{id=p count=potions use=#drink}\n# cave\n**A:** hello\n::if{seen(cave) > 5}\n**A:** often\n::endif\n::end\n# drink\n::set{potions -= 1}\n::return\n";
    let p = parse(src).unwrap();
    let seen = p.counted.values().flatten().next().expect("`seen(cave)` is counted").clone();
    let (mut s, _) = Session::start(&p);
    assert_eq!(int(&s, &seen), 1);
    s.input(Input::Use("p".into()));
    assert!(matches!(&s.state().pending, Some(Cmd::Say { text, .. }) if text == "hello"), "back at the line");
    assert_eq!((int(&s, &seen), int(&s, "potions")), (1, 0));
}

#[test]
fn a_counted_item_is_bought_by_adding_one_and_can_be_bought_again() {
    let p = parse(SRC).unwrap();
    let (mut s, _) = Session::start(&p);
    s.input(Input::Advance);
    s.input(Input::Advance);
    s.input(Input::Choose(0));
    let Some(Cmd::ShowShop { items, .. }) = s.state().pending.clone() else { panic!("not at the shop") };
    assert!(!items[0].owned && !items[1].owned);
    assert!(!s.input(Input::Buy(0)).is_empty());
    assert!(!s.input(Input::Buy(0)).is_empty(), "again: it is a count, not a set");
    assert_eq!((int(&s, "potions"), int(&s, "gold")), (4, 2));
    assert!(s.input(Input::Buy(0)).is_empty(), "too dear now");
    assert_eq!(s.state().vars["bag"], Value::List(vec!["key".into()]), "the list is untouched");
    assert!(!s.input(Input::Buy(1)).is_empty() && s.input(Input::Buy(1)).is_empty(), "a plain item is still owned once");
}

#[test]
fn the_tester_plays_a_use_label_once_and_does_not_call_it_unreached() {
    let p = parse(SRC).unwrap();
    let report = explore(&p, 5000);
    assert!(report.errors.is_empty() && !report.partial, "{report:?}");
    assert!(report.warnings.iter().all(|w| !w.message.contains("drink") && !w.message.contains("Glug")), "{:?}", report.warnings);
}

/// The name of the hidden counter behind `seen(label)`.
fn counter(p: &quillstone_script::Program) -> String {
    p.counted.values().flatten().next().expect("a `seen` counter").clone()
}

#[test]
fn a_use_label_that_jumps_away_does_not_cost_seen_a_visit() {
    let src = "---\ninventory: bag\nvars: { potions: 1, bag: [] }\n---\n::item{id=p count=potions use=#drink}\n# start\n**A:** hello\n::end\n\
# drink\n::jump{to=away}\n# away\n::if{seen(start) > 5}\n**A:** often\n::endif\n**A:** away\n::end\n";
    let p = parse(src).unwrap();
    let seen = counter(&p);
    let (mut s, _) = Session::start(&p);
    s.input(Input::Use("p".into()));
    assert!(matches!(&s.state().pending, Some(Cmd::Say { text, .. }) if text == "away"));
    assert_eq!(int(&s, &seen), 1, "start was visited once and never left through a return");
    // the line call is saved as such; a save from before (plain `stack`) still loads
    let saved = s.save();
    assert!(saved.contains("linecall "), "{saved}");
    let (loaded, _) = Session::load(&p, &saved).unwrap();
    assert_eq!(loaded.state(), s.state());
    assert!(Session::load(&p, &saved.replace("linecall ", "stack ")).is_ok());
}

#[test]
fn a_hand_edited_seen_counter_at_the_limits_does_not_overflow() {
    let src = "---\ninventory: bag\nvars: { potions: 1, bag: [] }\n---\n::item{id=p count=potions use=#drink}\n# start\n**A:** hello\n\
::jump{to=start}\n# drink\n::return\n::if{seen(start) > 1}\n**A:** x\n::endif\n";
    let p = parse(src).unwrap();
    let seen = counter(&p);
    let line = |n: i64| format!("var {} int {n}", crate::save::q(&seen));
    // A game loaded with the counter hand-edited to `edit`.
    let loaded = |edit: i64| {
        let (s, _) = Session::start(&p);
        let saved = s.save().replace(&line(1), &line(edit));
        assert!(saved.contains(&line(edit)), "{saved}");
        Session::load(&p, &saved).unwrap().0
    };
    // MAX: the jump back passes `start` again (+1).
    let mut s = loaded(i64::MAX);
    s.input(Input::Advance);
    assert_eq!(int(&s, &seen), i64::MAX);
    // MIN: Use, then `::return` shows the line again (-1).
    let mut s = loaded(i64::MIN);
    s.input(Input::Use("p".into()));
    assert_eq!(int(&s, &seen), i64::MIN + 1, "the line's own pass adds one back to the saturated floor");
}

#[test]
fn the_line_after_a_use_is_not_logged_again_and_keeps_its_picture() {
    let src = "---\ninventory: bag\nvars: { potions: 2, bag: [] }\n---\n::item{id=potion name=\"Potion\" count=potions use=#drink}\n\
               ![cat](cat.png)\n**A:** one\n**A:** two\n# drink\n**A:** Glug.\n::return\n";
    let p = parse(src).unwrap();
    let (mut s, first) = Session::start(&p);
    assert!(first.iter().any(|c| matches!(c, Cmd::ShowImage { .. })));
    let log = |s: &Session| s.backlog().into_iter().filter_map(|l| if let crate::Logged::Line(l) = l { Some(l.text) } else { None }).collect::<Vec<_>>();
    s.input(Input::Use("potion".into()));
    assert_eq!(log(&s), ["one", "Glug."]);
    let back = s.input(Input::Advance);
    assert!(matches!(&back[..], [Cmd::ShowImage { src, .. }, Cmd::Say { text, .. }] if src == "cat.png" && text == "one"), "{back:?}");
    assert!(!back.iter().any(|c| matches!(c, Cmd::PlayVoice { .. })));
    assert_eq!(log(&s), ["one", "Glug."], "the line is in the log once");
    assert_eq!(s.state().inline.len(), 1);
    s.input(Input::Advance);
    assert_eq!(log(&s), ["one", "Glug.", "two"]);
}
