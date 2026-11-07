//! `blend=add` on sprites in the runtime: commands, state, redraw, rollback and saves.

use quillstone_script::parse;

use crate::test_util::next;
use crate::{Cmd, Session};

const SRC: &str = "\
::character{id=eve sprites=e_{pose}.png}
# start
::show{eve blend=add}
One.
::show{eve happy}
Two.
::show{eve blend=normal}
Three.
";

fn additive(cmds: &[Cmd]) -> Vec<bool> {
    cmds.iter().filter_map(|c| if let Cmd::ShowSprite { additive, .. } = c { Some(*additive) } else { None }).collect()
}

#[test]
fn show_sets_the_blend_and_a_new_pose_keeps_it() {
    let p = parse(SRC).unwrap();
    let (mut game, first) = Session::start(&p);
    assert_eq!(additive(&first), [true]);
    assert!(game.state().sprites[0].additive);
    assert_eq!(additive(&next(&mut game)), [true], "a new pose keeps the blend");
    assert_eq!(additive(&next(&mut game)), [false], "blend=normal switches back");
}

#[test]
fn rollback_and_redraw_restore_the_blend() {
    let p = parse(SRC).unwrap();
    let (mut game, _) = Session::start(&p);
    next(&mut game);
    next(&mut game);
    assert_eq!(additive(&game.state().redraw()), [false]);
    game.rollback().unwrap();
    assert_eq!(additive(&game.state().redraw()), [true]);
}

#[test]
fn the_blend_survives_a_save_and_old_or_hostile_records_load() {
    let p = parse(SRC).unwrap();
    let (game, _) = Session::start(&p);
    let text = game.save();
    assert!(text.lines().any(|l| l == "additive \"eve\""), "{text}");
    let (loaded, _) = Session::load(&p, &text).unwrap();
    assert!(loaded.state().sprites[0].additive);
    // a save from before the blend has no record: normal
    let old = text.replace("additive \"eve\"\n", "");
    assert!(!Session::load(&p, &old).unwrap().0.state().sprites[0].additive);
    // a record for a sprite that is not on stage is refused, a doubled one is harmless
    assert!(Session::load(&p, &text.replace("additive \"eve\"", "additive \"ghost\"")).is_err());
    let twice = text.replace("additive \"eve\"\n", "additive \"eve\"\nadditive \"eve\"\n");
    assert!(Session::load(&p, &twice).unwrap().0.state().sprites[0].additive);
    assert!(Session::load(&p, &text.replace("additive \"eve\"", "additive")).is_err(), "no id");
}
