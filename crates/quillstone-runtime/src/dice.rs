//! `random(lo, hi)`: determinism, rollback, saves; dotted variable names through the runtime.

use quillstone_script::{parse, Value, DEFAULT_SEED};

use crate::{read_persistent, write_persistent, Cmd, Input, Session};

/// Five rolls in a row, each shown in a line: "R<n>: <value>".
const FIVE: &str = "\
---
vars: { r: 0 }
---
::set{r = random(1, 1000)}
R1: {r}
::set{r = random(1, 1000)}
R2: {r}
::set{r = random(1, 1000)}
R3: {r}
::set{r = random(1, 1000)}
R4: {r}
::set{r = random(1, 1000)}
R5: {r}
";

fn said(cmds: &[Cmd]) -> Option<String> {
    cmds.iter().find_map(|c| match c {
        Cmd::Say { text, .. } => Some(text.clone()),
        _ => None,
    })
}

/// Play `FIVE` from `seed`, returning the five lines.
fn play(seed: u64) -> Vec<String> {
    let p = parse(FIVE).unwrap();
    let (mut s, first) = Session::start_seeded(&p, Default::default(), seed);
    let mut lines = vec![said(&first).unwrap()];
    while lines.len() < 5 {
        lines.push(said(&s.input(Input::Advance)).unwrap());
    }
    lines
}

#[test]
fn the_same_seed_gives_the_same_game_and_another_seed_another() {
    assert_eq!(play(7), play(7));
    assert_ne!(play(7), play(8));
    let rolls = play(7);
    assert!(rolls.iter().enumerate().all(|(i, l)| l.starts_with(&format!("R{}: ", i + 1))));
    let distinct: std::collections::BTreeSet<_> = rolls.iter().map(|l| l.split(": ").nth(1).unwrap().to_string()).collect();
    assert!(distinct.len() >= 4, "the rolls move on: {rolls:?}");
}

#[test]
fn a_plain_start_uses_the_default_seed() {
    let p = parse(FIVE).unwrap();
    let (a, first) = Session::start(&p);
    let (b, again) = Session::start_seeded(&p, Default::default(), DEFAULT_SEED);
    assert_eq!(said(&first), said(&again));
    assert_eq!(a.state().rng, b.state().rng);
    assert_ne!(a.state().rng, DEFAULT_SEED, "the first roll moved it on");
}

#[test]
fn a_story_without_random_never_touches_the_seed() {
    let p = parse("::set{a = 1 + 2}\nHi.\n::end\n").unwrap();
    let (s, _) = Session::start_seeded(&p, Default::default(), 99);
    assert_eq!(s.state().rng, 99);
}

#[test]
fn rollback_and_roll_forward_replay_the_same_rolls() {
    let p = parse(FIVE).unwrap();
    let (mut s, first) = Session::start_seeded(&p, Default::default(), 1234);
    let r1 = said(&first).unwrap();
    let r2 = said(&s.input(Input::Advance)).unwrap();
    let r3 = said(&s.input(Input::Advance)).unwrap();
    // back to the second line, then forward again: the third roll is the one it was
    assert_eq!(said(&s.rollback().unwrap()), Some(r2.clone()));
    assert_eq!(said(&s.input(Input::Advance)), Some(r3.clone()), "going back doesn't buy a re-roll");
    // back twice, then redo with roll forward
    s.rollback();
    assert_eq!(said(&s.rollback().unwrap()), Some(r1.clone()));
    assert_eq!(said(&s.roll_forward().unwrap()), Some(r2));
    assert_eq!(said(&s.roll_forward().unwrap()), Some(r3.clone()));
    // and the game goes on from there exactly as an uninterrupted one
    let r4 = said(&s.input(Input::Advance)).unwrap();
    assert_eq!(r4, play(1234)[3]);
    s.rollback();
    assert_eq!(said(&s.input(Input::Advance)), Some(r4));
}

#[test]
fn choice_effects_roll_too_and_repeat_after_rollback() {
    let src = "---\nvars: { dmg: 0 }\n---\n# a\n::choice\n- [Strike](#b){dmg = random(1, 1000)}\n::\n# b\nDamage {dmg}.\n::end\n";
    let p = parse(src).unwrap();
    let (mut s, _) = Session::start_seeded(&p, Default::default(), 5);
    let hit = said(&s.input(Input::Choose(0))).unwrap();
    assert!(hit.starts_with("Damage ") && hit != "Damage 0.");
    s.rollback();
    assert_eq!(said(&s.input(Input::Choose(0))), Some(hit));
}

#[test]
fn a_save_keeps_the_seed_and_the_game_goes_on_the_same() {
    let p = parse(FIVE).unwrap();
    let (mut s, _) = Session::start_seeded(&p, Default::default(), 2024);
    s.input(Input::Advance);
    let text = s.save();
    assert!(text.lines().any(|l| l.starts_with("rng ")), "{text}");
    let (mut loaded, _) = Session::load(&p, &text).unwrap();
    assert_eq!(loaded.state().rng, s.state().rng);
    assert_eq!(loaded.state(), s.state());
    for _ in 0..3 {
        assert_eq!(said(&loaded.input(Input::Advance)), said(&s.input(Input::Advance)));
    }
    // the history's own seeds are kept: rolling back after loading and advancing repeats the roll
    let (mut again, _) = Session::load(&p, &text).unwrap();
    let before = again.state().clone();
    let next = said(&again.input(Input::Advance));
    again.rollback();
    assert_eq!(again.state(), &before);
    assert_eq!(said(&again.input(Input::Advance)), next);
}

#[test]
fn an_old_save_without_a_seed_loads_with_the_default_one() {
    let p = parse(FIVE).unwrap();
    let (mut s, _) = Session::start_seeded(&p, Default::default(), 77);
    s.input(Input::Advance);
    let old: String = s.save().lines().filter(|l| !l.starts_with("rng ")).map(|l| format!("{l}\n")).collect();
    let (loaded, _) = Session::load(&p, &old).unwrap();
    assert_eq!(loaded.state().rng, DEFAULT_SEED);
    assert!(Session::load(&p, &old.replace("state\n", "state\nrng nope\n")).is_err(), "a damaged record is refused like any other");
}

// ---- dotted names through the runtime ----

const DOTTED: &str = "\
---
vars: { hero.love: 2, n: 0, bag.items: [], who.name: \"Mi ka\" }
persistent: { seen.end: false }
inventory: bag.items
---
::set{hero.love += 3, bag.items += \"key\", seen.end = true, n = count(bag.items)}
{who.name} loves you {hero.love} ({n} item).
::if{hero.love * 2 > 9 and bag.items has \"key\"}
Yes.
::endif
::end
";

#[test]
fn dotted_names_work_in_text_conditions_saves_and_persistent_files() {
    let p = parse(DOTTED).unwrap();
    let (mut s, first) = Session::start(&p);
    assert_eq!(said(&first).as_deref(), Some("Mi ka loves you 5 (1 item)."));
    assert_eq!(said(&s.input(Input::Advance)).as_deref(), Some("Yes."));
    let text = s.save();
    assert!(text.contains("var \"hero.love\" int 5"), "{text}");
    let (loaded, _) = Session::load(&p, &text).unwrap();
    assert_eq!(loaded.state().vars["hero.love"], Value::Int(5));
    assert_eq!(loaded.state().vars["bag.items"], Value::List(vec!["key".into()]));
    // the persistent file
    let kept = s.persistent().clone();
    assert_eq!(kept["seen.end"], Value::Bool(true));
    let file = write_persistent(&kept);
    assert_eq!(read_persistent(&file), kept);
    let (next, _) = Session::start_with(&p, read_persistent(&file));
    assert_eq!(next.state().vars["seen.end"], Value::Bool(true));
}

// ---- the stats demo, played with a fixed seed ----

/// Advance until a menu is up: the lines passed on the way and the options.
fn to_menu(s: &mut Session, mut cmds: Vec<Cmd>, said: &mut Vec<String>) -> Vec<String> {
    for _ in 0..30 {
        said.extend(cmds.iter().filter_map(|c| match c {
            Cmd::Say { text, .. } => Some(text.clone()),
            _ => None,
        }));
        if let Some(options) = cmds.iter().find_map(|c| match c {
            Cmd::ShowChoices { options, .. } => Some(options.clone()),
            _ => None,
        }) {
            return options;
        }
        if s.state().finished {
            return Vec::new();
        }
        cmds = s.input(Input::Advance);
    }
    panic!("no menu after {said:?}");
}

/// Play the stats demo by picking the options that start with each of `picks`;
/// the whole transcript (lines and menus) and the final session.
fn stats_game<'p>(p: &'p quillstone_script::Program, seed: u64, picks: &[&str]) -> (Vec<String>, Session<'p>) {
    let (mut s, first) = Session::start_seeded(p, Default::default(), seed);
    let mut said = Vec::new();
    let mut menu = to_menu(&mut s, first, &mut said);
    for pick in picks {
        let i = menu.iter().position(|o| o.starts_with(pick)).unwrap_or_else(|| panic!("no `{pick}` in {menu:?}"));
        said.push(format!("> {}", menu[i]));
        let cmds = s.input(Input::Choose(i));
        menu = to_menu(&mut s, cmds, &mut said);
    }
    (said, s)
}

fn int(s: &Session, name: &str) -> i64 {
    s.state().vars[name].as_int().unwrap()
}

#[test]
fn the_stats_demo_plays_through_with_a_fixed_seed() {
    let p = parse(include_str!("../../../examples/stats.quill.md")).unwrap();
    let picks = ["Go into", "Rest", "Pick the old", "Visit", "Buy the lantern", "Buy the rope", "Back to camp", "Head home"];
    let (said, s) = stats_game(&p, 2024, &picks);

    // the bat: damage 2..=5, never below 1 hit point, shown through the stored variable
    let dmg = int(&s, "dmg");
    assert!((2..=5).contains(&dmg), "{dmg}");
    let hurt = 12 - dmg;
    assert!(said.contains(&format!("It takes {dmg} hit points. You have {hurt} left.")), "{said:#?}");
    assert_eq!(said.iter().any(|l| l == "You are hurt and should rest."), hurt <= 8);
    // resting heals 4, clamped to the maximum
    assert_eq!(int(&s, "hero.hp"), (hurt + 4).min(12) - if int(&s, "roll") <= 2 { 1 } else { 0 });
    // the dice: 1..=6, and `abs` measures the distance from 4
    let roll = int(&s, "roll");
    assert!((1..=6).contains(&roll), "{roll}");
    assert_eq!(int(&s, "gap"), (roll - 4).abs());
    assert!(said.contains(&format!("You rolled {roll}, which is {} away from the sweet spot.", (roll - 4).abs())));
    let lock_gold = if roll >= 5 { 10 } else { 0 };
    // the trader: cost = base * 2 - discount = 14, `count` of the bag
    assert_eq!(int(&s, "cost"), 14);
    assert!(said.contains(&"\"The lantern is 14 gold, the rope 8.\" You carry 0 things.".to_string()), "{said:#?}");
    assert!(said.iter().any(|l| l.ends_with("You carry 1 things.")), "{said:#?}");
    assert_eq!(int(&s, "hero.gold"), 20 + 5 + lock_gold - 14 - 8);
    assert_eq!(s.state().vars["inventory"], Value::List(vec!["lantern".into(), "rope".into()]));
    assert!(said.last().unwrap().starts_with("With a lantern and a rope you are ready"), "{said:#?}");
    assert!(s.state().finished);

    // the same seed and choices replay exactly; another seed rolls differently
    let (again, _) = stats_game(&p, 2024, &picks);
    assert_eq!(said, again);
    let rolls = |seed: u64| {
        let (_, s) = stats_game(&p, seed, &["Go into", "Pick the old"]);
        (int(&s, "dmg"), int(&s, "roll"))
    };
    let seen: std::collections::BTreeSet<_> = (0..40).map(rolls).collect();
    assert!(seen.len() > 8, "different seeds give different rolls: {seen:?}");
    // pinned: splitmix64 is part of the saved-game contract
    assert_eq!(rolls(2024), (dmg, roll));
    assert_eq!(rolls(1), PINNED_SEED_1);
}

const PINNED_SEED_1: (i64, i64) = (3, 2);

#[test]
fn a_roll_in_the_stats_demo_cannot_be_rerolled_by_going_back() {
    let p = parse(include_str!("../../../examples/stats.quill.md")).unwrap();
    let (_, mut s) = stats_game(&p, 9, &["Pick the old"]);
    let roll = int(&s, "roll");
    // back to the menu before the pick, and pick again: the same roll
    while s.rollback().is_some() && !matches!(s.state().pending, Some(Cmd::ShowChoices { .. })) {}
    let i = match &s.state().pending {
        Some(Cmd::ShowChoices { options, .. }) => options.iter().position(|o| o.starts_with("Pick the old")).unwrap(),
        other => panic!("{other:?}"),
    };
    s.input(Input::Choose(i));
    s.input(Input::Advance); // the dice are thrown after the first line
    assert_eq!(int(&s, "roll"), roll);
}
