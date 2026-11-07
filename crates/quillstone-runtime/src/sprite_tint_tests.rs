//! `tint=` on sprites in the runtime: commands, state, redraw, rollback and saves.

use quillstone_script::parse;

use crate::test_util::next;
use crate::{Cmd, Session};

const SRC: &str = "\
::character{id=eve sprites=e_{pose}.png}
# start
::show{eve tint=#102030}
One.
::show{eve happy}
Two.
::move{eve tint=#000000 time=1 then tint=none time=0.5}
Three.
::move{eve tint=#808080 loop}
Four.
";

fn tints(cmds: &[Cmd]) -> Vec<[f32; 3]> {
    cmds.iter().filter_map(|c| if let Cmd::MoveSprite { to, .. } = c { Some(to.tint) } else { None }).collect()
}

#[test]
fn show_sets_the_tint_and_a_new_pose_keeps_it() {
    let p = parse(SRC).unwrap();
    let (mut game, first) = Session::start(&p);
    let want = [16.0 / 255.0, 32.0 / 255.0, 48.0 / 255.0];
    assert!(first.iter().any(|c| matches!(c, Cmd::ShowSprite { tint, .. } if *tint == want)));
    assert_eq!(game.state().sprites[0].tint, want);
    let cmds = next(&mut game);
    assert!(matches!(&cmds[0], Cmd::ShowSprite { tint, .. } if *tint == want), "a new pose keeps the tint: {cmds:?}");
}

#[test]
fn moves_animate_the_tint_and_rollback_and_redraw_restore_it() {
    let p = parse(SRC).unwrap();
    let (mut game, _) = Session::start(&p);
    next(&mut game);
    let cmds = next(&mut game);
    assert_eq!(tints(&cmds), [[0.0; 3], [1.0; 3]]);
    assert_eq!(game.state().sprites[0].tint, [1.0; 3]);
    let redraw = game.state().redraw();
    assert!(tints(&redraw).is_empty() && redraw.iter().any(|c| matches!(c, Cmd::ShowSprite { tint, .. } if *tint == [1.0; 3])));
    game.rollback().unwrap();
    assert_eq!(game.state().sprites[0].tint[0], 16.0 / 255.0, "back before the move");
    // a move without tint keeps the resting one
    let cmds = next(&mut game);
    assert_eq!(tints(&cmds).len(), 2);
    let cmds = next(&mut game);
    assert_eq!(game.state().sprites[0].looped.map(|l| l.tint), Some([128.0 / 255.0; 3]), "{cmds:?}");
}

#[test]
fn tint_survives_a_save_and_old_or_hostile_records_load() {
    let p = parse(SRC).unwrap();
    let (mut game, _) = Session::start(&p);
    let text = game.save();
    let (loaded, _) = Session::load(&p, &text).unwrap();
    assert_eq!(loaded.state().sprites[0], game.state().sprites[0]);
    let sprite = text.lines().find(|l| l.starts_with("sprite ")).unwrap().to_string();
    let hostile = |tail: &str| {
        let cut = sprite.rsplitn(4, ' ').last().unwrap();
        text.replace(&sprite, &format!("{cut} {tail}"))
    };
    let tint_of = |s: &str| Session::load(&p, s).unwrap().0.state().sprites[0].tint;
    assert_eq!(tint_of(&hostile("9 -4 NaN")), [1.0, 0.0, 1.0], "clamped; not a number is white");
    assert_eq!(tint_of(&hostile("inf 0.5 0.25")), [1.0, 0.5, 0.25]);
    // a save from before tint has no such numbers
    assert_eq!(tint_of(&text.replace(&sprite, sprite.rsplitn(4, ' ').last().unwrap())), [1.0; 3]);
    // a looping move and a looping sequence keep their tint too
    for _ in 0..3 {
        next(&mut game);
    }
    let (loaded, _) = Session::load(&p, &game.save()).unwrap();
    assert_eq!(loaded.state().sprites[0], game.state().sprites[0]);
}
