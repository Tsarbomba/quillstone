//! Tests for the developer tools (see `dev.rs`).

use super::*;
use quillstone_script::{parse, Value};

const STORY: &str = "\
---
vars: { gold: 5, name: \"Ann\", met: false, bag: [] }
---
# start
**A:** Hello {name}.
::jump{to=end}
# shop
**A:** Gold is {gold}.
# end
**A:** The end.
";

fn say(c: &[Cmd]) -> String {
    c.iter().find_map(|c| if let Cmd::Say { text, .. } = c { Some(text.clone()) } else { None }).unwrap_or_default()
}

fn flags(args: &[&str]) -> (Result<StartAt, String>, Vec<String>) {
    let mut v: Vec<String> = args.iter().map(|s| s.to_string()).collect();
    (start_flags(&mut v), v)
}

#[test]
fn start_flags_are_taken_out_wherever_they_stand() {
    let (at, rest) = flags(&["a.quill.md", "--set", "gold=7", "--start", "shop", "--set", "name=Bo=b"]);
    let at = at.unwrap();
    assert_eq!(at.label.as_deref(), Some("shop"));
    assert_eq!(at.sets, [("gold".into(), "7".into()), ("name".into(), "Bo=b".into())]);
    assert_eq!(rest, ["a.quill.md"]);
    assert_eq!(flags(&["x"]).0.unwrap(), StartAt::default());
}

#[test]
fn bad_start_flags_are_errors() {
    assert!(flags(&["--start"]).0.unwrap_err().contains("needs a label"));
    assert!(flags(&["--set"]).0.unwrap_err().contains("var=value"));
    assert!(flags(&["--set", "gold"]).0.unwrap_err().contains("var=value"));
}

#[test]
fn start_at_begins_at_the_label_with_the_variables() {
    let p = parse(STORY).unwrap();
    let at = StartAt { label: Some("shop".into()), sets: vec![("gold".into(), "42".into()), ("name".into(), "\"Bo\"".into())] };
    let (s, cmds) = Session::start_at(&p, Default::default(), 1, &at).unwrap();
    assert_eq!(say(&cmds), "Gold is 42.");
    assert_eq!(s.state().vars["name"], Value::Str("Bo".into()));
    assert!(!s.can_rollback());
}

#[test]
fn start_at_refuses_unknown_labels_variables_and_values() {
    let p = parse(STORY).unwrap();
    let go = |label: Option<&str>, name: &str, v: &str| {
        let at = StartAt { label: label.map(Into::into), sets: vec![(name.into(), v.into())] };
        Session::start_at(&p, Default::default(), 1, &at).err()
    };
    assert!(go(Some("nope"), "gold", "1").unwrap().contains("no label `nope`"));
    assert!(go(None, "golf", "1").unwrap().contains("declares no variable"));
    assert!(go(None, "gold", "lots").unwrap().contains("not a whole number"));
    assert!(go(None, "met", "maybe").unwrap().contains("true or false"));
    assert!(go(None, "bag", "x").unwrap().contains("list"));
    assert!(go(None, "gold", "1").is_none());
}

#[test]
fn dev_set_changes_variables_and_refuses_type_changes() {
    let p = parse(STORY).unwrap();
    let (mut s, _) = Session::start(&p);
    s.dev_set("gold = gold * 2 + 1").unwrap();
    assert_eq!(s.state().vars["gold"], Value::Int(11));
    s.dev_set("name = \"Bo\"").unwrap();
    s.dev_set("extra = 3").unwrap(); // undeclared: as `::set`
    assert!(s.dev_set("gold = \"rich\"").unwrap_err().contains("can't be changed to text"));
    assert!(s.dev_set("name = 4").unwrap_err().contains("holds text"));
    assert!(s.dev_set("bag = 1").unwrap_err().contains("is a list"));
    s.dev_set("bag = [\"k\", \"m\"]").unwrap();
    assert_eq!(s.state().vars["bag"], Value::List(vec!["k".into(), "m".into()]));
    s.dev_set("bag = []").unwrap();
    assert_eq!(s.state().vars["bag"], Value::List(vec![]), "a list literal sets or clears a list");
    assert!(s.dev_set("gold = [\"k\"]").is_ok() && s.state().vars["gold"] == Value::Int(11), "a number is left alone");
    assert!(s.dev_set("gold ==").is_err());
    assert_eq!(s.state().vars["gold"], Value::Int(11), "a refused change leaves the value");
}

#[test]
fn dev_set_updates_the_line_on_screen() {
    let p = parse(STORY).unwrap();
    let at = StartAt { label: Some("shop".into()), sets: vec![] };
    let (mut s, _) = Session::start_at(&p, Default::default(), 1, &at).unwrap();
    s.dev_set("gold = 9").unwrap();
    assert_eq!(say(&s.state().redraw()), "Gold is 9.");
}

#[test]
fn dev_jump_keeps_variables_and_rollback_undoes_it() {
    let p = parse(STORY).unwrap();
    let (mut s, _) = Session::start(&p);
    s.dev_set("gold = 77").unwrap();
    let cmds = s.dev_jump("Shop").unwrap(); // a heading's own spelling works
    assert_eq!(say(&cmds), "Gold is 77.");
    assert_eq!(s.state().vars["gold"], Value::Int(77));
    assert!(s.can_rollback());
    assert_eq!(say(&s.rollback().unwrap()), "Hello Ann.");
    assert_eq!(say(&s.roll_forward().unwrap()), "Gold is 77.");
    assert!(s.dev_jump("nowhere").unwrap_err().contains("no label `nowhere`"));
}

#[test]
fn dev_jump_revives_a_finished_game() {
    let p = parse(STORY).unwrap();
    let (mut s, _) = Session::start(&p);
    s.input(Input::Advance);
    s.input(Input::Advance);
    assert!(s.state().finished);
    assert_eq!(say(&s.dev_jump("start").unwrap()), "Hello Ann.");
    assert!(!s.state().finished);
}
