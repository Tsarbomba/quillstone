//! `blur=` on sprites and the background in the runtime: commands, state, redraw, rollback and saves.

use quillstone_script::parse;

use crate::test_util::next;
use crate::{Cmd, Session};

const SRC: &str = "\
::character{id=eve sprites=e_{pose}.png}
# start
::bg{src=street.png blur=12}
::show{eve blur=8}
One.
::show{eve happy}
Two.
::move{eve blur=24 time=1 then blur=0 time=0.5}
Three.
::bg{src=street.png}
::move{eve blur=30 loop}
Four.
";

fn blur_of(cmds: &[Cmd]) -> Vec<f32> {
    cmds.iter().filter_map(|c| if let Cmd::MoveSprite { to, .. } = c { Some(to.blur) } else { None }).collect()
}

#[test]
fn show_and_bg_set_blur_and_a_new_pose_keeps_the_sprites() {
    let p = parse(SRC).unwrap();
    let (mut game, first) = Session::start(&p);
    assert!(first.iter().any(|c| matches!(c, Cmd::SetBackground { blur, .. } if *blur == 12.0)));
    assert!(first.iter().any(|c| matches!(c, Cmd::ShowSprite { blur, .. } if *blur == 8.0)));
    assert_eq!((game.state().background_blur, game.state().sprites[0].blur), (12.0, 8.0));
    let cmds = next(&mut game);
    assert!(matches!(&cmds[0], Cmd::ShowSprite { blur, .. } if *blur == 8.0), "a new pose keeps the blur: {cmds:?}");
}

#[test]
fn moves_animate_blur_in_sequence_and_a_redraw_lands_on_the_end_state() {
    let p = parse(SRC).unwrap();
    let (mut game, _) = Session::start(&p);
    next(&mut game);
    let cmds = next(&mut game);
    assert_eq!(blur_of(&cmds), [24.0, 0.0]);
    assert_eq!(game.state().sprites[0].blur, 0.0);
    let redraw = game.state().redraw();
    assert!(blur_of(&redraw).is_empty() && redraw.iter().any(|c| matches!(c, Cmd::ShowSprite { blur, .. } if *blur == 0.0)));
    game.rollback().unwrap();
    assert_eq!(game.state().sprites[0].blur, 8.0, "back before the move");
}

#[test]
fn a_bg_without_blur_is_sharp_and_a_blur_loop_is_state() {
    let p = parse(SRC).unwrap();
    let (mut game, _) = Session::start(&p);
    for _ in 0..2 {
        next(&mut game);
    }
    let cmds = next(&mut game);
    assert!(cmds.iter().any(|c| matches!(c, Cmd::SetBackground { blur, .. } if *blur == 0.0)));
    assert_eq!(game.state().background_blur, 0.0);
    assert_eq!(game.state().sprites[0].looped.map(|l| l.blur), Some(30.0));
}

#[test]
fn blur_survives_a_save_and_hostile_or_missing_values_are_clamped() {
    let p = parse(SRC).unwrap();
    let (mut game, _) = Session::start(&p);
    let text = game.save();
    assert!(text.contains(" blur 12") && text.contains(" 8 1 1 1\n"), "{text}");
    let (loaded, cmds) = Session::load(&p, &text).unwrap();
    assert_eq!((loaded.state().background_blur, loaded.state().sprites[0].blur), (12.0, 8.0));
    assert!(cmds.iter().any(|c| matches!(c, Cmd::SetBackground { blur, .. } if *blur == 12.0)));
    let wild = text.replace(" blur 12", " blur 9999").replace(" 8 1 1 1\n", " -5 1 1 1\n");
    let (loaded, _) = Session::load(&p, &wild).unwrap();
    assert_eq!((loaded.state().background_blur, loaded.state().sprites[0].blur), (64.0, 0.0));
    // a save from before blur has neither word
    let old: String = text
        .lines()
        .map(|l| match l {
            l if l.starts_with("sprite ") => l.rsplitn(5, ' ').last().unwrap().to_string(),
            l if l.starts_with("bg ") => l.replace(" blur 12", ""),
            l => l.to_string(),
        })
        .collect::<Vec<_>>()
        .join("\n")
        + "\n";
    let (loaded, _) = Session::load(&p, &old).unwrap();
    assert_eq!((loaded.state().background_blur, loaded.state().sprites[0].blur), (0.0, 0.0));
    // a looping move and a looping sequence keep their blur too
    for _ in 0..3 {
        next(&mut game);
    }
    let (loaded, _) = Session::load(&p, &game.save()).unwrap();
    assert_eq!(loaded.state().sprites[0], game.state().sprites[0]);
}
