//! `crop=` and `brightness=` on sprites in the runtime: commands, state, redraw, rollback and saves.

use quillstone_script::parse;

use crate::test_util::next;
use crate::{Cmd, Session};

const WHOLE: [f32; 4] = [0.0, 0.0, 1.0, 1.0];

const SRC: &str = "\
::character{id=eve sprites=e_{pose}.png}
# start
::show{eve crop=0.25,0,50%,0.5 brightness=0.1}
One.
::show{eve happy}
Two.
::move{eve crop=none brightness=-0.5 time=1 then brightness=0 time=0.5}
Three.
::move{eve crop=0,0,0.5,0.5 brightness=0.3 loop}
Four.
::show{eve crop=none brightness=0}
Five.
";

fn legs(cmds: &[Cmd]) -> Vec<(f32, [f32; 4])> {
    cmds.iter().filter_map(|c| if let Cmd::MoveSprite { to, .. } = c { Some((to.brightness, to.crop)) } else { None }).collect()
}

#[test]
fn show_sets_crop_and_brightness_and_a_new_pose_keeps_them() {
    let p = parse(SRC).unwrap();
    let (mut game, first) = Session::start(&p);
    assert!(first.iter().any(|c| matches!(c, Cmd::ShowSprite { crop, brightness, .. } if *crop == [0.25, 0.0, 0.5, 0.5] && *brightness == 0.1)));
    assert_eq!((game.state().sprites[0].crop, game.state().sprites[0].brightness), ([0.25, 0.0, 0.5, 0.5], 0.1));
    let cmds = next(&mut game);
    assert!(matches!(&cmds[0], Cmd::ShowSprite { crop, brightness, .. } if *crop == [0.25, 0.0, 0.5, 0.5] && *brightness == 0.1), "{cmds:?}");
}

#[test]
fn a_new_sprite_is_whole_and_as_drawn_and_none_goes_back() {
    let p = parse("::character{id=eve sprites=e_{pose}.png}\n# start\n::show{eve}\nOne.\n::show{eve crop=0,0,0.5,0.5}\nTwo.\n::show{eve crop=none}\nThree.\n").unwrap();
    let (mut game, _) = Session::start(&p);
    assert_eq!((game.state().sprites[0].crop, game.state().sprites[0].brightness), (WHOLE, 0.0));
    next(&mut game);
    assert_eq!(game.state().sprites[0].crop, [0.0, 0.0, 0.5, 0.5]);
    next(&mut game);
    assert_eq!(game.state().sprites[0].crop, WHOLE);
}

#[test]
fn moves_animate_both_and_rollback_and_redraw_restore_them() {
    let p = parse(SRC).unwrap();
    let (mut game, _) = Session::start(&p);
    next(&mut game);
    let cmds = next(&mut game);
    assert_eq!(legs(&cmds), [(-0.5, WHOLE), (0.0, WHOLE)]);
    assert_eq!((game.state().sprites[0].crop, game.state().sprites[0].brightness), (WHOLE, 0.0));
    let redraw = game.state().redraw();
    assert!(legs(&redraw).is_empty() && redraw.iter().any(|c| matches!(c, Cmd::ShowSprite { crop, brightness, .. } if *crop == WHOLE && *brightness == 0.0)));
    game.rollback().unwrap();
    assert_eq!((game.state().sprites[0].crop, game.state().sprites[0].brightness), ([0.25, 0.0, 0.5, 0.5], 0.1), "back before the move");
    // a loop's far end carries them, and a show that sets them ends the loop
    next(&mut game);
    let cmds = next(&mut game);
    assert!(cmds.iter().any(|c| matches!(c, Cmd::LoopSprite { to, .. } if to.brightness == 0.3 && to.crop == [0.0, 0.0, 0.5, 0.5])), "{cmds:?}");
    assert_eq!(game.state().sprites[0].looped.map(|l| (l.brightness, l.crop)), Some((0.3, [0.0, 0.0, 0.5, 0.5])));
    next(&mut game);
    assert!(game.state().sprites[0].looped.is_none());
}

#[test]
fn both_survive_a_save_and_old_or_hostile_records_load() {
    let p = parse(SRC).unwrap();
    let (mut game, _) = Session::start(&p);
    for _ in 0..3 {
        next(&mut game);
    }
    let text = game.save();
    assert!(text.lines().any(|l| l.starts_with("loop ")), "{text}");
    let (loaded, _) = Session::load(&p, &text).unwrap();
    assert_eq!(loaded.state().sprites[0], game.state().sprites[0]);
    let (first, _) = Session::start(&p);
    let text = first.save();
    let sprite = text.lines().find(|l| l.starts_with("sprite ")).unwrap().to_string();
    let load = |s: &str| Session::load(&p, s).unwrap().0.state().sprites[0].clone();
    assert_eq!((load(&text).crop, load(&text).brightness), ([0.25, 0.0, 0.5, 0.5], 0.1));
    // a save from before has neither: as drawn, the whole picture
    let old = text.replace(&sprite, sprite.rsplitn(6, ' ').last().unwrap());
    assert_eq!((load(&old).crop, load(&old).brightness), (WHOLE, 0.0));
    // nonsense is pulled into range: brightness clamps, a crop outside the picture is the whole picture
    let cut = sprite.rsplitn(6, ' ').last().unwrap();
    let wild = text.replace(&sprite, &format!("{cut} 9 0.5 0 0.9 1"));
    assert_eq!((load(&wild).crop, load(&wild).brightness), (WHOLE, 1.0));
    let nan = text.replace(&sprite, &format!("{cut} NaN 0 0 0 0"));
    assert_eq!((load(&nan).crop, load(&nan).brightness), (WHOLE, 0.0));
}
