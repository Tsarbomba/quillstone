//! Variable names with `{}` parts: `love.{who}`.

use crate::test_util::{bad, ok};
use crate::{get_var, is_name_piece, name_pieces, parse_expr, resolve_name, Expr, Op, Value, Vars};

fn vars(pairs: &[(&str, Value)]) -> Vars {
    pairs.iter().map(|(k, v)| (k.to_string(), v.clone())).collect()
}

fn text(s: &str) -> Value {
    Value::Str(s.to_string())
}

const HEAD: &str = "---\nvars: { who: \"mia\", n: 1, flag: false, bag: [], love.mia: 0, love.zoe: 0, grid.1.2: 7 }\n---\n";

#[test]
fn a_name_is_split_into_text_and_variables() {
    assert_eq!(name_pieces("love.{who}").unwrap().len(), 2);
    assert_eq!(name_pieces("grid.{x}.{y}.max").unwrap().len(), 5);
    assert_eq!(name_pieces("plain.name").unwrap().len(), 1);
    for wrong in ["love.{who", "love.who}", "love.{}", "love.{a b}", "love{who}", "love.{who}x", "{who}.love", "love..{who}", "love.{a.{b}}"] {
        assert!(name_pieces(wrong).is_err(), "{wrong}");
    }
    let nine = format!("a{}", ".{x}".repeat(9));
    assert!(name_pieces(&nine).unwrap_err().contains("at most 8"));
}

#[test]
fn only_a_safe_text_or_a_whole_number_spells_a_name() {
    let name = |v: Value| resolve_name("love.{who}", &vars(&[("who", v)])).map(|n| n.into_owned());
    assert_eq!(name(text("mia")), Some("love.mia".into()));
    assert_eq!(name(text("Zoë_2")), Some("love.Zoë_2".into()));
    assert_eq!(name(Value::Int(3)), Some("love.3".into()));
    for unsafe_text in ["", "a.b", "a/b", "a\\b", "a b", "seen:x", "..", &"x".repeat(65)] {
        assert_eq!(name(text(unsafe_text)), None, "{unsafe_text:?}");
    }
    assert_eq!(name(Value::Int(-1)), None);
    assert_eq!(name(Value::Bool(true)), None);
    assert_eq!(name(Value::List(vec![])), None);
    assert_eq!(resolve_name("love.{who}", &Vars::new()), None, "an unset part");
    assert!(is_name_piece(&"x".repeat(64)));
}

#[test]
fn a_name_is_read_through_its_parts() {
    let v = vars(&[("who", text("zoe")), ("love.zoe", Value::Int(4)), ("x", Value::Int(1)), ("y", Value::Int(2)), ("grid.1.2", Value::Int(7))]);
    assert_eq!(get_var(&v, "love.{who}"), Some(&Value::Int(4)));
    assert_eq!(get_var(&v, "grid.{x}.{y}"), Some(&Value::Int(7)));
    assert_eq!(get_var(&v, "love.{nobody}"), None);
    assert!(parse_expr("love.{who} > 3").unwrap().test(&v));
    assert_eq!(parse_expr("love.{who} + grid.{x}.{y}").unwrap().eval(&v), Value::Int(11));
    let mut read = Vec::new();
    let e = parse_expr("love.{who} > n and grid.{x}.{y} == 1").unwrap();
    e.vars_read(&mut read);
    assert_eq!(read, ["who", "n", "x", "y"], "a dynamic name reads the variables in its parts");
}

#[test]
fn an_unset_or_unsafe_name_reads_as_an_unset_variable() {
    let e = parse_expr("love.{who} == 0").unwrap();
    assert!(e.test(&vars(&[("who", text("a.b"))])) && e.test(&Vars::new()));
    assert!(e.test(&vars(&[("who", text("kim"))])), "an undeclared name is unset: 0");
}

#[test]
fn reads_and_targets_parse_everywhere_a_name_goes() {
    let p = ok(&format!("{HEAD}::set{{love.{{who}} += 1}}\n::if{{love.{{who}} > 3}}\nHi.\n::endif\n::choice\n- [Hug](#a){{if love.{{who}} >= 2, love.{{who}} += 1}}\n::\n# a\nBye.\n"));
    assert!(matches!(&p.ops[0], Op::Set(e) if e.var == "love.{who}"));
    let Some(Op::Choice { options, .. }) = p.ops.iter().find(|o| matches!(o, Op::Choice { .. })) else { panic!() };
    assert_eq!(options[0].effects[0].var, "love.{who}");
    assert!(matches!(options[0].cond, Some(Expr::Cmp(..))));
    ok(&format!("{HEAD}::set{{grid.{{n}}.{{n}} = 1, bag.{{who}} = 2}}\n::if{{n > grid.{{n}}.2}}\nHi.\n::endif\nDone.\n"));
}

#[test]
fn text_may_show_a_name_with_parts() {
    ok(&format!("{HEAD}You have {{love.{{who}}}} points with {{who}}, {{grid.{{n}}.2}}.\n"));
}

#[test]
fn bad_names_are_errors_with_the_line() {
    bad(&format!("{HEAD}::set{{love.{{flag}} += 1}}\n"), "`flag` must be a text or number variable");
    bad(&format!("{HEAD}::if{{love.{{bag}} > 1}}\nHi.\n::endif\n"), "`bag` must be a text or number variable");
    bad(&format!("{HEAD}::set{{love.{{who += 1}}\n"), "braces must pair up");
    bad(&format!("{HEAD}::set{{love.{{}} += 1}}\n"), "must name a variable");
    bad(&format!("{HEAD}::set{{love.{{who}}x += 1}}\n"), "after a dot");
    bad(&format!("{HEAD}::if{{love.{{who}}x > 1}}\nHi.\n::endif\n"), "dot or the end");
    bad(&format!("{HEAD}::set{{{{who}}.love += 1}}\n"), "isn't a variable name");
}

#[test]
fn an_undeclared_part_variable_is_allowed_like_a_call_parameter() {
    ok(&format!("{HEAD}::call{{to=talk who=\"zoe\"}}\n::end\n# talk(who)\n::set{{love.{{who}} += 1}}\n::return\n"));
}

#[test]
fn undeclared_combinations_are_found_only_when_the_values_are_known() {
    let found = |body: &str| ok(&format!("{HEAD}{body}")).undeclared_names().into_iter().map(|(_, n, m)| (n, m)).collect::<Vec<_>>();
    assert!(found("::set{love.{who} += 1}\nHi.\n").is_empty(), "who is only mia");
    assert_eq!(found("::set{who = \"kim\"}\n::set{love.{who} += 1}\nHi.\n"), [("love.{who}".to_string(), vec!["love.kim".to_string()])]);
    assert!(found("::input{var=who prompt=\"Who?\"}\n::set{love.{who} += 1}\nHi.\n").is_empty(), "typed: unknown");
    assert!(found("::set{who += \"x\"}\n::set{love.{who} += 1}\nHi.\n").is_empty(), "computed: unknown");
    assert!(found("::set{who = \"a.b\"}\n::if{love.{who} > 1}\nHi.\n::endif\n").is_empty(), "can't be spelled at all");
    let two = found("::set{who = \"zoe\"}\n::set{who = \"kim\"}\n::set{who = \"li\"}\n::if{love.{who} > 1}\nHi.\n::endif\n");
    assert_eq!(two[0].1, ["love.kim", "love.li"]);
    let grid = found("::set{n = 2}\n::set{grid.{n}.{n} = 1}\nHi.\n");
    assert_eq!(grid.len(), 1, "n is 1 or 2, the same in both places");
    assert_eq!(grid[0].1, ["grid.1.1", "grid.2.2"], "grid.1.2 is declared but never spelled");
}
