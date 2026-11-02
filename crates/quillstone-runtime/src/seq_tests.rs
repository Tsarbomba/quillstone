//! Move sequences (`then`), `alpha=` and `xoffset=` / `yoffset=` in the runtime: commands, state,
//! redraw, rollback and saves.

use quillstone_script::{parse, Ease};

use crate::test_util::next;
use crate::{Cmd, Session};

const SRC: &str = "\
::character{id=eve sprites=e_{pose}.png}
# start
::show{eve x=10% alpha=0.5 xoffset=40 yoffset=-20}
One.
::move{eve x=30% time=1 then x=70% time=1 wait=0.5 then alpha=0 xoffset=0 time=0.5}
Two.
::show{eve happy}
Three.
::move{eve x=20% time=1 then alpha=1 time=0.5 wait=1 loop}
Four.
::show{eve alpha=0.25}
Five.
";

fn steps(cmds: &[Cmd]) -> Vec<(bool, f32, f32, f32, f32)> {
    cmds.iter().filter_map(|c| if let Cmd::MoveSprite { to, queued, .. } = c { Some((*queued, to.x, to.alpha, to.time, to.wait)) } else { None }).collect()
}

#[test]
fn show_sets_alpha_and_pixel_offsets_and_keeps_them() {
    let p = parse(SRC).unwrap();
    let (mut game, first) = Session::start(&p);
    assert!(first.iter().any(|c| matches!(c, Cmd::ShowSprite { alpha, dx, dy, .. } if *alpha == 0.5 && *dx == 40.0 && *dy == -20.0)));
    let s = &game.state().sprites[0];
    assert_eq!((s.alpha, s.dx, s.dy), (0.5, 40.0, -20.0));
    next(&mut game);
    let cmds = next(&mut game); // a new pose keeps what the move left
    assert!(matches!(&cmds[0], Cmd::ShowSprite { alpha, dx, .. } if *alpha == 0.0 && *dx == 0.0), "{cmds:?}");
}

#[test]
fn a_sequence_is_queued_moves_and_the_sprite_ends_at_the_last_step() {
    let p = parse(SRC).unwrap();
    let (mut game, _) = Session::start(&p);
    let cmds = next(&mut game);
    assert_eq!(steps(&cmds), [(false, 0.3, 0.5, 1.0, 0.0), (true, 0.7, 0.5, 1.0, 0.5), (true, 0.7, 0.0, 0.5, 0.0)]);
    assert!(matches!(&cmds[0], Cmd::MoveSprite { to, .. } if to.ease == Ease::InOut && to.dx == 40.0), "unnamed values stay");
    let s = &game.state().sprites[0];
    assert_eq!((s.x, s.alpha, s.dx, s.dy), (0.7, 0.0, 0.0, -20.0));
    assert!(matches!(game.state().pending, Some(Cmd::Say { .. })), "the story does not wait for it");
    let redraw = game.state().redraw();
    assert!(matches!(redraw.iter().find(|c| matches!(c, Cmd::ShowSprite { .. })), Some(Cmd::ShowSprite { x, alpha, .. }) if *x == 0.7 && *alpha == 0.0));
    assert!(steps(&redraw).is_empty(), "a redraw and a rollback land at the end state");
    next(&mut game);
    game.rollback().unwrap();
    assert_eq!(game.state().sprites[0].x, 0.7);
    game.rollback().unwrap();
    assert_eq!(game.state().sprites[0].x, 0.1, "back before the move");
}

#[test]
fn a_looping_sequence_repeats_from_where_its_last_step_ends() {
    let p = parse(SRC).unwrap();
    let (mut game, _) = Session::start(&p);
    for _ in 0..2 {
        next(&mut game);
    }
    let cmds = next(&mut game);
    assert_eq!(steps(&cmds), [(false, 0.2, 0.0, 1.0, 0.0), (true, 0.2, 1.0, 0.5, 1.0)]);
    assert!(cmds.iter().any(|c| matches!(c, Cmd::CycleSprite { .. })));
    let s = game.state().sprites[0].clone();
    assert_eq!((s.x, s.alpha, s.cycle.len()), (0.2, 1.0, 2));
    // A redraw shows the sprite at the end values, then runs the legs again and repeats them.
    let redraw = game.state().redraw();
    let show = redraw.iter().position(|c| matches!(c, Cmd::ShowSprite { .. })).unwrap();
    assert!(matches!(&redraw[show + 1], Cmd::MoveSprite { queued: false, .. }) && matches!(&redraw[show + 3], Cmd::CycleSprite { .. }));
    // A save keeps it.
    let (loaded, again) = Session::load(&p, &game.save()).unwrap();
    assert_eq!(loaded.state().sprites[0], s);
    assert!(again.iter().any(|c| matches!(c, Cmd::CycleSprite { .. })), "a load starts the repeat again");
    // A show that sets values ends it; one that only changes the pose does not.
    let cmds = next(&mut game);
    assert!(game.state().sprites[0].cycle.is_empty() && cmds.iter().any(|c| matches!(c, Cmd::MoveSprite { queued: false, to, .. } if to.time == 0.0 && to.alpha == 0.25)), "{cmds:?}");
}

#[test]
fn alpha_and_offsets_survive_a_save_and_hostile_values_are_clamped() {
    let p = parse(SRC).unwrap();
    let (game, _) = Session::start(&p);
    let text = game.save();
    assert!(text.contains(" 0.5 40 -20"), "{text}");
    let (loaded, _) = Session::load(&p, &text).unwrap();
    let s = &loaded.state().sprites[0];
    assert_eq!((s.alpha, s.dx, s.dy), (0.5, 40.0, -20.0));
    let wild = text.replace(" 0.5 40 -20", " 7 99999 -99999");
    let (loaded, _) = Session::load(&p, &wild).unwrap();
    let s = &loaded.state().sprites[0];
    assert_eq!((s.alpha, s.dx, s.dy), (1.0, 3840.0, -3840.0));
    // an older save stops after `yscale`
    let old: String = text.lines().map(|l| if l.starts_with("sprite ") { l.rsplitn(5, ' ').last().unwrap().to_string() } else { l.to_string() }).collect::<Vec<_>>().join("\n") + "\n";
    let (loaded, _) = Session::load(&p, &old).unwrap();
    let s = &loaded.state().sprites[0];
    assert_eq!((s.alpha, s.dx, s.dy), (1.0, 0.0, 0.0));
}

#[test]
fn a_cycle_record_for_a_sprite_that_is_not_there_is_refused_and_too_many_legs_too() {
    let p = parse(SRC).unwrap();
    let (game, _) = Session::start(&p);
    let text = game.save();
    let bad = text.replace("sprite ", "cycle \"ghost\" 0 0 1 1 1 1 0 0 1 linear 0\nsprite ");
    assert!(Session::load(&p, &bad).is_err());
    let leg = "cycle \"eve\" 0 0 1 1 1 1 0 0 1 linear 0\n".repeat(33);
    let bad = text.lines().map(|l| if l.starts_with("sprite ") { format!("{l}\n{leg}") } else { format!("{l}\n") }).collect::<String>();
    assert!(Session::load(&p, &bad).is_err());
}
