//! Frame animations (`anim:<id>` pictures) and looping moves in the runtime: state,
//! redraw, rollback, saves (old, new and hostile) and hot reload.

use quillstone_script::{parse, Ease, Program};

use crate::test_util::next;
use crate::{Cmd, Input, Session, SpriteStep};

const SRC: &str = "\
::anim{id=blink frames=\"a.png 1, b.png 0.5\" loop}
::anim{id=rain frames=\"r1.png 1, r2.png 1\" loop}
::character{id=mika sprites=m_{pose}.png}
# start
::bg{anim=rain fade=0.5}
::show{mika anim=blink x=left scale=0.8}
One.
::show{mika happy}
Two.
::show{mika anim=blink}
::move{mika y=0.9 time=1.2 ease=out loop}
Three.
::show{mika neutral}
Four.
::show{mika neutral x=right}
Five.
::move{mika y=0.9 time=1 loop}
Six.
::move{mika x=left y=1 time=0.5}
Seven.
::move{mika scale=1.2 xscale=1.1 yscale=0.9 time=2 loop=pingpong}
Eight.
::hide{mika}
Nine.
";

fn program() -> Program {
    parse(SRC).unwrap()
}

fn sprite<'a>(game: &'a Session) -> &'a crate::SpriteState {
    &game.state().sprites[0]
}

fn loop_of(game: &Session) -> Option<SpriteStep> {
    game.state().sprites.first().and_then(|s| s.looped)
}

// ---- animations as pictures --------------------------------------------------------------------

#[test]
fn a_sprite_and_a_background_can_be_animations() {
    let p = program();
    let (game, first) = Session::start(&p);
    assert_eq!(game.state().background.as_deref(), Some("anim:rain"));
    assert!(first.contains(&Cmd::SetBackground { src: "anim:rain".into(), fade: 0.5, wipe: None, sound: false, blur: 0.0, mask: None }));
    assert_eq!(sprite(&game).src, "anim:blink");
    assert!(first.iter().any(|c| matches!(c, Cmd::ShowSprite { id, src, x, scale, .. } if id == "mika" && src == "anim:blink" && *x == 0.0 && *scale == 0.8)));
    // The scene is rebuilt from state alone.
    let redraw = game.state().redraw();
    assert!(redraw.contains(&Cmd::SetBackground { src: "anim:rain".into(), fade: 0.0, wipe: None, sound: false, blur: 0.0, mask: None }));
    assert!(redraw.iter().any(|c| matches!(c, Cmd::ShowSprite { src, .. } if src == "anim:blink")));
}

#[test]
fn a_sprite_keeps_everything_else_when_its_picture_changes_between_animation_and_pose() {
    let p = program();
    let (mut game, _) = Session::start(&p);
    let cmds = next(&mut game);
    assert!(matches!(&cmds[0], Cmd::ShowSprite { src, x, scale, .. } if src == "m_happy.png" && *x == 0.0 && *scale == 0.8));
    let cmds = next(&mut game);
    assert!(matches!(&cmds[0], Cmd::ShowSprite { src, x, scale, .. } if src == "anim:blink" && *x == 0.0 && *scale == 0.8));
}

#[test]
fn rollback_returns_to_the_pictures_of_the_earlier_screen() {
    let p = program();
    let (mut game, _) = Session::start(&p);
    next(&mut game);
    assert_eq!(sprite(&game).src, "m_happy.png");
    let back = game.rollback().unwrap();
    assert_eq!(back[0], Cmd::ClearScene);
    assert_eq!(sprite(&game).src, "anim:blink");
    assert_eq!(game.state().background.as_deref(), Some("anim:rain"));
    assert!(back.iter().any(|c| matches!(c, Cmd::ShowSprite { src, .. } if src == "anim:blink")));
}

#[test]
fn saves_keep_the_animation_ids_and_an_old_save_loads_as_before() {
    let p = program();
    let (game, _) = Session::start(&p);
    let text = game.save();
    assert!(text.contains("bg \"anim:rain\"") && text.contains("sprite \"mika\" \"anim:blink\""), "{text}");
    let (loaded, cmds) = Session::load(&p, &text).unwrap();
    assert_eq!(loaded.state().background.as_deref(), Some("anim:rain"));
    assert_eq!(sprite(&loaded).src, "anim:blink");
    assert!(cmds.iter().any(|c| matches!(c, Cmd::ShowSprite { src, .. } if src == "anim:blink")));
    // A save from before animations (plain paths, no loop record) is untouched by any of this.
    let q = parse("::bg{src=a.png}\n::show{x src=x.png}\nHi.\n").unwrap();
    let (old, _) = Session::start(&q);
    let plain = old.save();
    assert!(!plain.contains("anim:") && !plain.contains("\nloop "));
    assert_eq!(Session::load(&q, &plain).unwrap().0.state().sprites[0].src, "x.png");
}

#[test]
fn an_unknown_animation_in_a_save_is_refused() {
    let p = program();
    let (game, _) = Session::start(&p);
    let text = game.save();
    for (from, to) in [
        ("anim:rain", "anim:snow"),
        ("anim:blink", "anim:snow"),
        ("anim:blink", "anim:"),
        ("anim:blink", "anim:a b"),
        ("anim:blink", "anim:../x"),
        ("anim:blink", "ANIM:blink"),
        ("anim:blink", "anim:blink.png"),
    ] {
        let e = Session::load(&p, &text.replace(from, to));
        assert!(e.is_err(), "{to}");
    }
    let e = Session::load(&p, &text.replace("anim:blink", "anim:snow")).err().unwrap();
    assert!(e.contains("unknown animation `snow`"), "{e}");
    // A game that declares no animations refuses any.
    let none = parse("::character{id=mika sprites=m_{pose}.png}\nHi.\n").unwrap();
    assert!(Session::load(&none, &text).is_err());
}

#[test]
fn a_hot_reload_that_removed_an_animation_drops_its_picture_but_a_load_refuses() {
    let p = program();
    let (mut game, _) = Session::start(&p);
    next(&mut game);
    next(&mut game); // sprite is anim:blink again, with history that holds both
    let text = game.save();
    let edited = parse(&SRC.replace("::anim{id=rain frames=\"r1.png 1, r2.png 1\" loop}\n", "").replace("::bg{anim=rain fade=0.5}", "::bg{src=plain.png}")).unwrap();
    assert!(Session::load(&edited, &text).is_err(), "a save is refused");
    let (reloaded, cmds) = Session::load_edited(&edited, &text).unwrap();
    assert_eq!(reloaded.state().background, None, "the background picture is gone");
    assert!(!cmds.iter().any(|c| matches!(c, Cmd::SetBackground { .. })));
    assert_eq!(sprite(&reloaded).src, "anim:blink", "other animations stay");
    // Removing the sprite's animation takes the sprite off the stage, and its loop with it.
    let mut g = Session::start(&p).0;
    for _ in 0..2 {
        next(&mut g);
    }
    assert!(loop_of(&g).is_some());
    let text = g.save();
    let no_blink = parse(&SRC.replace("::anim{id=blink frames=\"a.png 1, b.png 0.5\" loop}\n", "").replace("anim=blink", "src=gone.png")).unwrap();
    assert!(Session::load(&no_blink, &text).is_err());
    let (r, cmds) = Session::load_edited(&no_blink, &text).unwrap();
    assert!(r.state().sprites.is_empty() && !cmds.iter().any(|c| matches!(c, Cmd::LoopSprite { .. })));
    // The same script reloads unchanged.
    let (same, _) = Session::load_edited(&p, &text).unwrap();
    assert_eq!(same.state().sprites, g.state().sprites);
    // And a hand-edited save that loops a sprite that is not there is refused even when edited.
    assert!(Session::load_edited(&p, &text.replace("loop \"mika\"", "loop \"nobody\"")).is_err());
}

// ---- looping moves ----------------------------------------------------------------------------

#[test]
fn a_looping_move_is_sprite_state_and_a_command() {
    let p = program();
    let (mut game, _) = Session::start(&p);
    next(&mut game);
    let cmds = next(&mut game); // Three: ::show anim, ::move loop
    assert_eq!(
        cmds.iter().find(|c| matches!(c, Cmd::LoopSprite { .. })).unwrap(),
        &Cmd::LoopSprite { id: "mika".into(), flip: false, to: SpriteStep { x: 0.0, y: 0.9, scale: 0.8, xscale: 1.0, yscale: 1.0, alpha: 1.0, dx: 0.0, dy: 0.0, blur: 0.0, tint: [1.0; 3], time: 1.2, ease: Ease::Out, wait: 0.0 } }
    );
    assert!(!cmds.iter().any(|c| matches!(c, Cmd::MoveSprite { .. })), "the view glides to the far values by itself");
    // The sprite's own values are where it starts (what each leg back ends at).
    let s = sprite(&game);
    assert_eq!((s.x, s.y, s.scale), (0.0, 1.0, 0.8));
    assert_eq!(loop_of(&game), Some(SpriteStep { x: 0.0, y: 0.9, scale: 0.8, xscale: 1.0, yscale: 1.0, alpha: 1.0, dx: 0.0, dy: 0.0, blur: 0.0, tint: [1.0; 3], time: 1.2, ease: Ease::Out, wait: 0.0 }));
}

#[test]
fn a_redraw_sends_the_loop_after_the_sprite() {
    let p = program();
    let (mut game, _) = Session::start(&p);
    next(&mut game);
    next(&mut game);
    let redraw = game.state().redraw();
    let show = redraw.iter().position(|c| matches!(c, Cmd::ShowSprite { .. })).unwrap();
    assert!(matches!(&redraw[show + 1], Cmd::LoopSprite { to, .. } if to.y == 0.9));
    assert!(matches!(&redraw[show], Cmd::ShowSprite { y, .. } if *y == 1.0), "shown at the starting values: the phase restarts there");
}

#[test]
fn a_new_pose_keeps_the_loop_but_new_position_or_size_values_end_it() {
    let p = program();
    let (mut game, _) = Session::start(&p);
    for _ in 0..2 {
        next(&mut game);
    }
    let cmds = next(&mut game); // Four: ::show{mika neutral}
    assert!(loop_of(&game).is_some(), "a pose change alone leaves the loop");
    assert!(matches!(&cmds[0], Cmd::ShowSprite { src, y, .. } if src == "m_neutral.png" && *y == 1.0));
    assert!(!cmds.iter().any(|c| matches!(c, Cmd::MoveSprite { .. } | Cmd::LoopSprite { .. })));
    let cmds = next(&mut game); // Five: ::show{mika neutral x=right}
    assert!(loop_of(&game).is_none());
    assert!(matches!(&cmds[0], Cmd::ShowSprite { x, .. } if *x == 1.0));
    assert!(
        matches!(&cmds[1], Cmd::MoveSprite { to, .. } if to.time == 0.0 && to.y == 1.0),
        "an instant move tells the view to end its loop: {cmds:?}"
    );
}

#[test]
fn showing_the_values_it_already_has_still_ends_the_loop() {
    let p = parse("::show{a src=a.png x=0.4}\n::move{a y=0.9 loop}\nOne.\n::show{a src=b.png x=0.4}\nTwo.\n").unwrap();
    let (mut game, _) = Session::start(&p);
    assert!(loop_of(&game).is_some());
    let cmds = next(&mut game);
    assert!(loop_of(&game).is_none());
    assert!(cmds.iter().any(|c| matches!(c, Cmd::MoveSprite { to, .. } if to.time == 0.0)));
}

#[test]
fn a_plain_move_cancels_the_loop_and_a_new_loop_replaces_it() {
    let p = program();
    let (mut game, _) = Session::start(&p);
    for _ in 0..5 {
        next(&mut game);
    }
    assert_eq!(sprite(&game).src, "m_neutral.png");
    assert!(loop_of(&game).is_some(), "Six comes with `::move{{mika y=0.9 time=1 loop}}`");
    let cmds = next(&mut game); // Seven: plain move
    assert!(loop_of(&game).is_none());
    assert!(matches!(&cmds[0], Cmd::MoveSprite { to, .. } if to.x == 0.0 && to.y == 1.0 && to.time == 0.5));
    let cmds = next(&mut game); // Eight: a loop on size only
    let l = loop_of(&game).unwrap();
    assert_eq!((l.scale, l.xscale, l.yscale, l.y, l.time), (1.2, 1.1, 0.9, 1.0, 2.0));
    assert!(matches!(&cmds[0], Cmd::LoopSprite { to, .. } if to.scale == 1.2 && to.xscale == 1.1));
    next(&mut game); // Nine: hidden
    assert!(game.state().sprites.is_empty() && game.state().redraw().iter().all(|c| !matches!(c, Cmd::LoopSprite { .. })));
}

#[test]
fn a_loop_does_not_hold_the_story_or_rollback() {
    let p = program();
    let (mut game, _) = Session::start(&p);
    for _ in 0..2 {
        next(&mut game);
    }
    assert!(matches!(game.state().pending, Some(Cmd::Say { .. })), "the loop does not wait");
    assert!(game.can_rollback());
    game.rollback().unwrap();
    assert!(loop_of(&game).is_none(), "back before the loop");
    assert!(matches!(game.state().pending, Some(Cmd::Say { .. })));
    let forward = game.roll_forward().unwrap();
    assert!(loop_of(&game).is_some());
    assert!(forward.iter().any(|c| matches!(c, Cmd::LoopSprite { .. })), "roll forward restores it");
    // One step back from inside the loop lands on a screen with the loop.
    next(&mut game);
    let back = game.rollback().unwrap();
    assert!(loop_of(&game).is_some() && back.iter().any(|c| matches!(c, Cmd::LoopSprite { .. })));
}

#[test]
fn saves_restore_the_loop_and_old_saves_have_none() {
    let p = program();
    let (mut game, _) = Session::start(&p);
    for _ in 0..2 {
        next(&mut game);
    }
    let text = game.save();
    assert!(text.contains("loop \"mika\" 0 0.9 0.8 1 1 1.2 out"), "{text}");
    let (loaded, cmds) = Session::load(&p, &text).unwrap();
    assert_eq!(loaded.state().sprites, game.state().sprites);
    assert!(cmds.iter().any(|c| matches!(c, Cmd::LoopSprite { to, .. } if to.y == 0.9 && to.time == 1.2)));
    // The history carries it too, so rollback after a load works.
    let mut loaded = loaded;
    assert!(loaded.rollback().is_some() && loop_of(&loaded).is_none());
    // A save without the record loads with no loop.
    let old: String = text.lines().filter(|l| !l.starts_with("loop ")).map(|l| format!("{l}\n")).collect();
    let (plain, _) = Session::load(&p, &old).unwrap();
    assert!(loop_of(&plain).is_none() && sprite(&plain).y == 1.0);
}

#[test]
fn hostile_loop_records_are_clamped_or_refused() {
    let p = program();
    let (mut game, _) = Session::start(&p);
    for _ in 0..2 {
        next(&mut game);
    }
    let text = game.save();
    let with = |rec: &str| text.lines().map(|l| if l.starts_with("loop ") { rec } else { l }).collect::<Vec<_>>().join("\n") + "\n";
    let (l, _) = Session::load(&p, &with("loop \"mika\" 9 -9 99 0 0 0 sideways")).unwrap();
    let r = loop_of(&l).unwrap();
    assert_eq!((r.x, r.y, r.scale, r.xscale, r.yscale), (3.0, -2.0, 10.0, 0.05, 0.05));
    assert_eq!(r.time, 0.02, "a zero leg would spin: clamped up");
    assert_eq!(r.ease, Ease::InOut, "an unknown ease is the default");
    let (l, _) = Session::load(&p, &with("loop \"mika\" nan inf 1 1 1 1e30 linear")).unwrap();
    let r = loop_of(&l).unwrap();
    assert!(r.x.is_finite() && r.y.is_finite() && r.time.is_finite() && r.time <= 3600.0, "{r:?}");
    for bad in ["loop \"mika\" 0.5", "loop \"mika\" x 0.9 1 1 1 1 out", "loop \"ghost\" 0.5 0.9 1 1 1 1 out", "loop"] {
        assert!(Session::load(&p, &with(bad)).is_err(), "{bad}");
    }
}

#[test]
fn the_tester_walks_through_animations_and_loops() {
    let p = parse(include_str!("../../../examples/anim.quill.md")).unwrap();
    let r = crate::explore(&p, 100_000);
    assert!(r.warnings.is_empty() && r.errors.is_empty(), "{:?} {:?}", r.warnings, r.errors);
    assert!(!r.partial);
}

/// The demo, played from its first line to its last, checking the scene as it goes.
#[test]
fn the_animation_demo_plays_through_with_the_right_scene_at_every_step() {
    let p = parse(include_str!("../../../examples/anim.quill.md")).unwrap();
    let (mut game, first) = Session::start(&p);
    assert!(first.iter().any(|c| matches!(c, Cmd::SetBackground { src, .. } if src == "assets/rooftop.png")));
    let mut seen_anim_sprite = false;
    let mut seen_anim_bg = false;
    let mut loops = 0;
    let mut lines = 0;
    while !game.state().finished {
        let cmds = next(&mut game);
        lines += 1;
        seen_anim_sprite |= game.state().sprites.iter().any(|s| s.src.starts_with("anim:"));
        seen_anim_bg |= game.state().background.as_deref().is_some_and(|b| b.starts_with("anim:"));
        loops += cmds.iter().filter(|c| matches!(c, Cmd::LoopSprite { .. })).count();
        // A rollback and a save/load at every step show the same scene.
        let (loaded, _) = Session::load(&p, &game.save()).unwrap();
        assert_eq!(loaded.state().sprites, game.state().sprites, "line {lines}");
        assert_eq!(loaded.state().background, game.state().background);
        assert!(lines < 100, "the demo must end");
    }
    assert!(seen_anim_sprite && seen_anim_bg);
    assert_eq!(loops, 3);
    assert!(game.state().sprites.iter().all(|s| s.looped.is_none()), "the last move is a plain one");
}

#[test]
fn an_animation_background_unlocks_the_gallery_pictures_that_are_its_frames() {
    let src = "---\ngallery: [r1.png, r2.png, other.png, a.png]\n---\n::anim{id=rain frames=\"r1.png 1, r2.png 1\" loop}\n::anim{id=blink frames=\"a.png 1, b.png 1\"}\n::character{id=mika sprites=m_{pose}.png}\n::show{mika anim=blink}\nOne.\n::bg{anim=rain}\nTwo.\n";
    let p = parse(src).unwrap();
    let (mut game, _) = Session::start(&p);
    let kept = |g: &Session| {
        let mut k: Vec<String> = g.persistent().keys().filter_map(|k| k.strip_prefix(crate::UNLOCKED).map(String::from)).collect();
        k.sort();
        k
    };
    assert!(kept(&game).is_empty(), "a sprite animation unlocks nothing: {:?}", kept(&game));
    game.input(Input::Advance);
    assert_eq!(kept(&game), ["r1.png", "r2.png"], "every listed frame, and not the other pictures");
}
