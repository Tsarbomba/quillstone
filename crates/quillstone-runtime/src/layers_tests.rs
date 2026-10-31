//! Layered sprites in the runtime: what a show keeps, replaces and hides, the commands and
//! the state, rollback, saves (old, round trip, hostile) and the tester.

use quillstone_script::{parse, Program};

use crate::{explore, Cmd, Input, Session, SpriteState};

const SRC: &str = "\
---
vars: { look: \"casual\", mood: \"smile\", bad: \"../x\" }
---
::anim{id=blink frames=\"e/open.png 3, e/closed.png 0.1\" loop}
::character{id=mika layers=body,eyes,mouth layer_sprites=art/{layer}_{value}.png}
::character{id=ren sprites=ren_{pose}.png layers=body,eyes layer_sprites=ren/{layer}_{value}.png}
# start
::show{mika body={look} eyes=open mouth={mood} x=left scale=0.8}
One.
::show{mika mouth=open}
Two.
::show{mika eyes=anim:blink body=formal x=right}
Three.
::show{mika eyes=none}
Four.
::hide{mika}
::show{mika eyes=open}
Five.
::show{ren body=a eyes=b}
Six.
::show{ren happy}
Seven.
::show{ren eyes=c}
Eight.
::set{look=\"../x\"}
::show{mika body={look} eyes=open mouth=open}
Nine.
";

fn program() -> Program {
    parse(SRC).unwrap()
}

fn next(game: &mut Session) -> Vec<Cmd> {
    game.input(Input::Advance)
}

fn layers(game: &Session, id: &str) -> Vec<(String, String)> {
    game.state().sprites.iter().find(|s| s.id == id).map(|s| s.layers.clone()).unwrap_or_default()
}

fn l(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
    pairs.iter().map(|(a, b)| (a.to_string(), b.to_string())).collect()
}

fn warnings(cmds: &[Cmd]) -> Vec<&str> {
    cmds.iter().filter_map(|c| if let Cmd::Warning { message } = c { Some(message.as_str()) } else { None }).collect()
}

fn mika<'a>(game: &'a Session) -> &'a SpriteState {
    game.state().sprites.iter().find(|s| s.id == "mika").unwrap()
}

#[test]
fn a_layered_show_composes_keeps_and_hides_layers() {
    let p = program();
    let (mut game, first) = Session::start(&p);
    let all = l(&[("body", "art/body_casual.png"), ("eyes", "art/eyes_open.png"), ("mouth", "art/mouth_smile.png")]);
    assert_eq!(layers(&game, "mika"), all, "{{var}} values are resolved; the character's layer order");
    assert_eq!(mika(&game).src, "", "a layered sprite has no picture of its own");
    assert!(first.iter().any(|c| matches!(c, Cmd::ShowSprite { id, src, layers, x, scale, .. }
        if id == "mika" && src.is_empty() && *layers == all && *x == 0.0 && *scale == 0.8)));
    assert!(warnings(&first).is_empty());
    // only the mouth changes; the others keep their value, and so do position and size
    let cmds = next(&mut game);
    assert_eq!(layers(&game, "mika")[2].1, "art/mouth_open.png");
    assert_eq!(layers(&game, "mika")[..2], all[..2]);
    assert!(cmds.iter().any(|c| matches!(c, Cmd::ShowSprite { layers, x, scale, .. } if layers.len() == 3 && *x == 0.0 && *scale == 0.8)));
    // an animation as a layer, a new outfit and a new place
    next(&mut game);
    let now = layers(&game, "mika");
    assert_eq!((now[0].1.as_str(), now[1].1.as_str(), now[2].1.as_str()), ("art/body_formal.png", "anim:blink", "art/mouth_open.png"));
    assert_eq!(mika(&game).x, 1.0);
    // none hides one layer and keeps its place in the list
    next(&mut game);
    assert_eq!(layers(&game, "mika")[1], ("eyes".to_string(), String::new()));
    assert_eq!(layers(&game, "mika").len(), 3);
}

#[test]
fn a_sprite_hidden_and_shown_again_starts_over_with_a_warning() {
    let p = program();
    let (mut game, _) = Session::start(&p);
    for _ in 0..3 {
        next(&mut game);
    }
    // ::hide then ::show{mika eyes=open}: body and mouth were never given
    let cmds = next(&mut game);
    assert_eq!(layers(&game, "mika"), l(&[("body", ""), ("eyes", "art/eyes_open.png"), ("mouth", "")]));
    let w = warnings(&cmds);
    assert_eq!(w.len(), 1, "{w:?}");
    assert!(w[0].contains("body, mouth") && w[0].contains("mika"), "{}", w[0]);
}

#[test]
fn a_pose_replaces_the_layers_and_layers_replace_a_pose() {
    let p = program();
    let (mut game, _) = Session::start(&p);
    for _ in 0..5 {
        next(&mut game);
    }
    assert_eq!(layers(&game, "ren").len(), 2, "Six");
    let cmds = next(&mut game); // ::show{ren happy}
    let ren = game.state().sprites.iter().find(|s| s.id == "ren").unwrap();
    assert_eq!((ren.src.as_str(), ren.layers.len()), ("ren_happy.png", 0), "a pose drops the layers");
    assert!(cmds.iter().any(|c| matches!(c, Cmd::ShowSprite { id, src, layers, .. } if id == "ren" && src == "ren_happy.png" && layers.is_empty())));
    // layers on a pose sprite: the pose goes, the unnamed layer is missing
    let cmds = next(&mut game);
    let ren = game.state().sprites.iter().find(|s| s.id == "ren").unwrap();
    assert_eq!(ren.src, "");
    assert_eq!(ren.layers, l(&[("body", ""), ("eyes", "ren/eyes_c.png")]));
    assert!(warnings(&cmds)[0].contains("body"));
}

#[test]
fn an_unsafe_layer_value_shows_nothing_and_warns() {
    let p = program();
    let (mut game, _) = Session::start(&p);
    for _ in 0..7 {
        next(&mut game);
    }
    let cmds = next(&mut game); // Eight -> Nine, through the ::set and the bad ::show
    let w = warnings(&cmds);
    assert_eq!(w.len(), 1, "{w:?}");
    assert!(w[0].contains("not a safe file name") && !w[0].contains("../x\""), "{}", w[0]);
    assert_eq!(layers(&game, "mika")[0].1, quillstone_script::MISSING_PICTURE);
    assert_eq!(layers(&game, "mika")[1].1, "art/eyes_open.png", "the other layers are fine");
}

#[test]
fn rollback_redraw_and_save_restore_the_layers() {
    let p = program();
    let (mut game, _) = Session::start(&p);
    next(&mut game);
    next(&mut game);
    let at_three = layers(&game, "mika");
    next(&mut game);
    let back = game.rollback().unwrap();
    assert_eq!(layers(&game, "mika"), at_three);
    assert!(back.iter().any(|c| matches!(c, Cmd::ShowSprite { layers, .. } if *layers == at_three)));
    // a redraw carries them with no fade
    assert!(game.state().redraw().iter().any(|c| matches!(c, Cmd::ShowSprite { layers, fade, .. } if *layers == at_three && *fade == 0.0)));
    // save and load
    let text = game.save();
    assert!(text.contains("sprite \"mika\" \"\" "), "{text}");
    assert!(text.contains("layer \"mika\" \"eyes\" \"anim:blink\""), "{text}");
    let (loaded, cmds) = Session::load(&p, &text).unwrap();
    assert_eq!(loaded.state().sprites, game.state().sprites);
    assert!(cmds.iter().any(|c| matches!(c, Cmd::ShowSprite { layers, .. } if *layers == at_three)));
    assert_eq!(loaded.save(), text, "stable");
}

#[test]
fn a_save_from_before_layers_loads_unchanged() {
    let p = parse("::show{x src=x.png}\nHi.\n").unwrap();
    let (game, _) = Session::start(&p);
    let text = game.save();
    assert!(!text.contains("layer "), "a plain sprite writes no layer record");
    let (loaded, _) = Session::load(&p, &text).unwrap();
    assert_eq!((loaded.state().sprites[0].src.as_str(), loaded.state().sprites[0].layers.len()), ("x.png", 0));
    // the same text loads under a script that has since got layers
    let q = parse("::character{id=mika layers=body layer_sprites=a/{layer}_{value}.png}\n::show{x src=x.png}\nHi.\n").unwrap();
    assert_eq!(Session::load(&q, &text).unwrap().0.state().sprites[0].src, "x.png");
}

#[test]
fn hostile_layer_records_are_refused_or_made_harmless() {
    let p = program();
    let (mut game, _) = Session::start(&p);
    next(&mut game);
    let text = game.save();
    let layer = "layer \"mika\" \"eyes\" \"art/eyes_open.png\"";
    assert!(text.contains(layer), "{text}");
    let with = |new: &str| text.replace(layer, new);
    for (what, bad) in [
        ("a path outside the game", "layer \"mika\" \"eyes\" \"../../etc/passwd\""),
        ("an absolute path", "layer \"mika\" \"eyes\" \"/etc/passwd\""),
        ("a drive", "layer \"mika\" \"eyes\" \"C:/x.png\""),
        ("a backslash", "layer \"mika\" \"eyes\" \"a\\\\b.png\""),
        ("an unknown animation", "layer \"mika\" \"eyes\" \"anim:snow\""),
        ("an undeclared layer", "layer \"mika\" \"nose\" \"art/nose_a.png\""),
        ("a layer of a sprite that isn't on stage", "layer \"ghost\" \"eyes\" \"art/eyes_open.png\""),
        ("a layer of a character without layers", "layer \"ren\" \"hat\" \"art/eyes_open.png\""),
        ("a short record", "layer \"mika\" \"eyes\""),
    ] {
        assert!(Session::load(&p, &with(bad)).is_err(), "{what} should be refused");
    }
    // the same layer twice: the later value wins, there is still one entry
    let twice = with(&format!("{layer}\nlayer \"mika\" \"eyes\" \"art/eyes_closed.png\""));
    let (loaded, _) = Session::load(&p, &twice).unwrap();
    assert_eq!(layers(&loaded, "mika").len(), 3);
    assert_eq!(layers(&loaded, "mika")[1].1, "art/eyes_closed.png");
    // records in any order end up in the character's order
    let lines: Vec<&str> = text.lines().collect();
    let first = lines.iter().position(|l| l.starts_with("layer ")).unwrap();
    let mut shuffled = lines.clone();
    shuffled[first..first + 3].reverse();
    let (loaded, _) = Session::load(&p, &(shuffled.join("\n") + "\n")).unwrap();
    assert_eq!(layers(&loaded, "mika"), layers(&game, "mika"));
    // a sprite record whose picture is empty but no layers follow is harmless: nothing is drawn
    let stripped: String = text.lines().filter(|l| !l.starts_with("layer ")).map(|l| format!("{l}\n")).collect();
    let (loaded, cmds) = Session::load(&p, &stripped).unwrap();
    assert!(layers(&loaded, "mika").is_empty());
    assert!(cmds.iter().any(|c| matches!(c, Cmd::ShowSprite { src, layers, .. } if src.is_empty() && layers.is_empty())));
}

#[test]
fn a_script_edit_that_drops_a_layer_or_animation_drops_it_from_the_load() {
    let script = |layers: &str, eyes: &str, anim: &str| {
        let eyes = if layers.contains("eyes") { format!("eyes={eyes} ") } else { String::new() };
        parse(&format!(
            "{anim}::character{{id=mika layers={layers} layer_sprites=art/{{layer}}_{{value}}.png}}\n::show{{mika body=a {eyes}mouth=c}}\nHi.\n"
        ))
        .unwrap()
    };
    let blink = "::anim{id=blink frames=\"e/open.png 3, e/closed.png 0.1\" loop}\n";
    let p = script("body,eyes,mouth", "anim:blink", blink);
    let (game, _) = Session::start(&p);
    let text = game.save();
    assert_eq!(layers(&game, "mika")[1].1, "anim:blink");
    // the layer is gone from the character
    let q = script("body,mouth", "", "");
    assert!(Session::load(&q, &text).is_err(), "a plain load refuses it");
    let (loaded, _) = Session::load_edited(&q, &text).unwrap();
    assert_eq!(layers(&loaded, "mika").iter().map(|(n, _)| n.as_str()).collect::<Vec<_>>(), ["body", "mouth"]);
    // the animation is gone: that layer is hidden, the rest stay
    let r = script("body,eyes,mouth", "x", "");
    assert!(Session::load(&r, &text).is_err());
    let (loaded, _) = Session::load_edited(&r, &text).unwrap();
    assert_eq!(layers(&loaded, "mika")[1], ("eyes".to_string(), String::new()));
    assert_eq!(layers(&loaded, "mika")[0].1, "art/body_a.png");
}

#[test]
fn the_tester_reports_layers_left_empty_and_unset_variables() {
    let p = program();
    let report = explore(&p, 1000);
    let all: Vec<&str> = report.warnings.iter().map(|w| w.message.as_str()).collect();
    assert!(all.iter().any(|m| m.contains("without a value for the layer(s) body, mouth")), "{all:?}");
    assert!(all.iter().any(|m| m.contains("not a safe file name")), "{all:?}");
    // a variable nobody sets
    let q = parse("::character{id=m layers=a layer_sprites=x/{layer}_{value}.png}\n::show{m a={never}}\nHi.\n").unwrap();
    let report = explore(&q, 100);
    assert!(report.warnings.iter().any(|w| w.message.contains("`never` is never set")), "{:?}", report.warnings);
}
