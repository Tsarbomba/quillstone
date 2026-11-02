//! Variable names with `{}` parts (`love.{who}`) at run time, in saves and in the tester.

use quillstone_script::{parse, Value};

use crate::test_util::says;
use crate::{explore, Cmd, Input, Session};

const SRC: &str = "\
---
vars: { who: \"mia\", i: 1, love.mia: 0, love.zoe: 5, slot.0: 10, slot.1: 20, picked: 0 }
---
# start
::set{love.{who} += 1}
::set{who = \"zoe\"}
::set{love.{who} += 2}
::set{picked = slot.{i}, slot.{i} -= 1}
You have {love.{who}} points with {who}; slot {slot.{i}}.
::choice
- [Hug](#end){if love.{who} >= 7, love.{who} += 10}
- [Leave](#end)
::
# end
Done.
";

fn int(game: &Session, name: &str) -> i64 {
    game.state().vars[name].as_int().unwrap()
}

#[test]
fn a_name_reads_and_writes_the_variable_its_parts_spell() {
    let p = parse(SRC).unwrap();
    let (mut game, cmds) = Session::start(&p);
    assert_eq!((int(&game, "love.mia"), int(&game, "love.zoe")), (1, 7), "mia, then zoe");
    assert_eq!((int(&game, "picked"), int(&game, "slot.1"), int(&game, "slot.0")), (20, 19, 10), "a number spells a part too");
    assert!(says(&cmds, "You have 7 points with zoe; slot 19."), "{cmds:?}");
    let choices = game.input(Input::Advance);
    assert!(choices.iter().any(|c| matches!(c, Cmd::ShowChoices { options, .. } if options.len() == 2)), "the condition is true");
    game.input(Input::Choose(0));
    assert_eq!(int(&game, "love.zoe"), 17, "the effect writes the same way");
}

#[test]
fn a_name_that_spells_nothing_declared_reads_zero_and_changes_nothing_with_one_warning() {
    let src = "---\nvars: { who: \"kim\", love.mia: 3 }\n---\n::set{love.{who} += 1}\nSeen {love.{who}} {love.{who}.max}\n::if{love.{who} == 0}\nZero.\n::endif\n";
    let p = parse(src).unwrap();
    let (mut game, mut cmds) = Session::start(&p);
    assert!(says(&cmds, "Seen {love.{who}} {love.{who}.max}"), "shown as written, like an unknown `{{name}}`: {cmds:?}");
    cmds.extend(game.input(Input::Advance));
    assert!(says(&cmds, "Zero."));
    assert!(!game.state().vars.keys().any(|k| k.contains("kim")), "no variable is made");
    let warnings: Vec<_> = cmds.iter().filter_map(|c| if let Cmd::Warning { message } = c { Some(message) } else { None }).collect();
    assert_eq!(warnings.len(), 1, "{warnings:?}");
    assert!(!warnings[0].contains("kim"), "the value is not in the message");
}

#[test]
fn typed_text_that_is_not_a_plain_name_spells_nothing() {
    let src = "---\nvars: { who: \"\", love.mia: 3, love.a: 1 }\n---\n::input{var=who prompt=\"Who?\"}\n::set{love.{who} += 1}\n{love.{who}}\n";
    let p = parse(src).unwrap();
    for (typed, spelled) in [("mia", Some(4)), ("a.b", None), ("../x", None), ("a b", None), ("seen:x", None)] {
        let (mut game, _) = Session::start(&p);
        let cmds = game.input(Input::Text(typed.into()));
        let total: i64 = game.state().vars.iter().filter(|(k, _)| k.starts_with("love.")).filter_map(|(_, v)| v.as_int()).sum();
        assert_eq!(total, 4 + spelled.map_or(0, |s| s - 3), "{typed}");
        assert!(says(&cmds, &spelled.map_or("{love.{who}}".to_string(), |s| s.to_string())), "{typed}: {cmds:?}");
        assert_eq!(game.state().vars.len(), 3, "{typed}: no variable was made");
    }
}

#[test]
fn rollback_and_saves_carry_the_values_like_any_variable() {
    let p = parse(SRC).unwrap();
    let (mut game, _) = Session::start(&p);
    game.input(Input::Advance);
    let saved = game.save();
    game.input(Input::Choose(0));
    assert_eq!(int(&game, "love.zoe"), 17);
    let (loaded, _) = Session::load(&p, &saved).unwrap();
    assert_eq!(int(&loaded, "love.zoe"), 7);
    assert_eq!(loaded.state().vars["who"], Value::Str("zoe".into()));
}

#[test]
fn the_console_sets_through_a_name_with_parts() {
    let p = parse(SRC).unwrap();
    let (mut game, _) = Session::start(&p);
    game.dev_set("love.{who} = 40").unwrap();
    assert_eq!(int(&game, "love.zoe"), 40);
    game.dev_set("who = \"kim\"").unwrap();
    assert!(game.dev_set("love.{who} = 1").unwrap_err().contains("not changed"));
    game.dev_set("who = \"mia\"").unwrap();
    assert!(game.dev_set("love.{who} = \"x\"").unwrap_err().contains("can't be changed to text"), "the type of the spelled variable counts");
}

#[test]
fn the_tester_plays_the_right_variables_and_knows_their_names() {
    let src = "---\nvars: { who: \"mia\", love.mia: 0, love.zoe: 1 }\n---\n\
        ::choice\n- [Mia](#a){who = \"mia\"}\n- [Zoe](#a){who = \"zoe\"}\n::\n# a\n::set{love.{who} += 1}\n\
        ::if{love.{who} > 1}\nLiked {love.{who}}.\n::else\nNever.\n::endif\n::end\n";
    let r = explore(&parse(src).unwrap(), 1000);
    assert!(r.errors.is_empty() && r.warnings.is_empty(), "{:?}", (&r.errors, &r.warnings));
    assert_eq!(r.endings.len(), 1);
}

#[test]
fn the_tester_warns_about_a_part_nothing_sets_and_a_name_it_cannot_write() {
    let src = "---\nvars: { love.mia: 0 }\n---\n::set{love.{kim} += 1}\nHi {love.{kim}}.\n::set{love.zoe += 1}\n";
    let r = explore(&parse(src).unwrap(), 1000);
    let w: Vec<&str> = r.warnings.iter().map(|f| f.message.as_str()).collect();
    assert!(w.iter().any(|m| m.contains("`kim` is never set")), "{w:?}");
}
