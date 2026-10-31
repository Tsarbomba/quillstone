//! Layered sprites in the parser: `::character{layers=... layer_sprites=...}` and `::show{mika eyes=open}`.

use crate::{parse, AssetKind, Op, Program};

const HEAD: &str = "---\nvars: { mood: \"smile\", n: 3 }\n---\n\
    ::anim{id=blink frames=\"e/open.png 3, e/closed.png 0.1\" loop}\n\
    ::character{id=mika name=Mika layers=body,eyes,mouth layer_sprites=art/{layer}_{value}.png}\n";

fn ok(body: &str) -> Program {
    parse(&format!("{HEAD}{body}")).unwrap_or_else(|e| panic!("unexpected errors: {e:?}"))
}

/// The messages of the errors with their lines (none when the script parses).
fn errors(src: &str) -> Vec<String> {
    parse(src).err().unwrap_or_default().into_iter().map(|e| format!("{}: {}", e.line, e.message)).collect()
}

fn bad(body: &str, needle: &str) {
    let e = errors(&format!("{HEAD}{body}"));
    assert!(e.iter().any(|m| m.contains(needle)), "wanted `{needle}` in {e:?}");
}

fn shows(p: &Program) -> Vec<(&str, &[(String, String)])> {
    p.ops.iter().filter_map(|o| if let Op::Show { id, layers, .. } = o { Some((id.as_str(), layers.as_slice())) } else { None }).collect()
}

fn pair(l: &str, p: &str) -> (String, String) {
    (l.to_string(), p.to_string())
}

#[test]
fn a_character_declares_layers_back_to_front() {
    let p = ok("::show{mika body=a eyes=b mouth=c}\n");
    let c = &p.characters["mika"];
    assert_eq!(c.layers, ["body", "eyes", "mouth"]);
    assert_eq!(c.layer_sprites.as_deref(), Some("art/{layer}_{value}.png"));
    assert_eq!(c.layer_path("eyes", "open").as_deref(), Some("art/eyes_open.png"));
    assert_eq!(c.sprites, None, "layers only");
    let both = parse("::character{id=a sprites=s_{pose}.png layers=\"body, eyes\" layer_sprites=l/{layer}/{value}.png}\n").unwrap();
    assert_eq!(both.characters["a"].layers, ["body", "eyes"], "spaces after a comma are fine in quotes");
    assert!(both.characters["a"].sprites.is_some());
}

#[test]
fn bad_layer_declarations_are_refused() {
    let line = |attrs: &str| errors(&format!("::character{{id=a {attrs}}}\n")).join("; ");
    assert!(line("layers=body").contains("needs `layer_sprites="));
    assert!(line("layer_sprites=a/{layer}_{value}.png").contains("needs `layers="));
    assert!(line("layers=body layer_sprites=a/{layer}.png").contains("needs `{value}`"));
    assert!(line("layers=body layer_sprites=a/{value}.png").contains("needs `{layer}`"));
    assert!(line("layers=body,body layer_sprites=a/{layer}_{value}.png").contains("listed twice"));
    assert!(line("layers=body,,eyes layer_sprites=a/{layer}_{value}.png").contains("empty entry"));
    assert!(line("layers=a/b layer_sprites=a/{layer}_{value}.png").contains("layer id"));
    assert!(line("layers=x layer_sprites=a/{layer}_{value}.png").contains("can't be a layer id"));
    assert!(line("layers=none layer_sprites=a/{layer}_{value}.png").contains("can't be a layer id"));
    assert!(line("layers=a,b,c,d,e,f,g,h,i layer_sprites=a/{layer}_{value}.png").contains("at most 8"));
    assert!(line("layers=a,b,c,d,e,f,g,h layer_sprites=a/{layer}_{value}.png").is_empty(), "eight are fine");
    assert!(line("layers=body layer_sprites=a/{layer}_{value}.webm").contains("picture"));
    assert!(line("layers=body layer_sprites=../a/{layer}_{value}.png").contains("inside the game folder"));
    assert!(line("layers=body layer_sprites=a/{layer}_{value}_{x}.png").contains("braces"), "only {{layer}} and {{value}}");
}

#[test]
fn a_show_sets_layers_in_the_characters_order() {
    let p = ok("::show{mika mouth=smile body=casual eyes=open x=left}\n::show{mika eyes=none}\n::show{mika eyes=anim:blink}\n");
    let s = shows(&p);
    assert_eq!(s[0], ("mika", &[pair("body", "art/body_casual.png"), pair("eyes", "art/eyes_open.png"), pair("mouth", "art/mouth_smile.png")][..]));
    assert_eq!(s[1].1, [pair("eyes", "")], "none hides the layer");
    assert_eq!(s[2].1, [pair("eyes", "anim:blink")]);
    assert!(matches!(&p.ops[0], Op::Show { src, x: Some(_), .. } if src.is_empty()), "a layered sprite has no src of its own");
}

#[test]
fn the_first_layered_show_names_every_layer() {
    bad("::show{mika body=casual}\n", "missing: eyes, mouth");
    bad("::show{mika x=left}\n", "missing: body, eyes, mouth");
    bad("::show{mika body=a eyes=none}\n", "missing: mouth");
    // after that, layers keep their values, so a show may name some or none
    let p = ok("::show{mika body=a eyes=b mouth=c}\n::show{mika mouth=d}\n::show{mika x=0.2}\n");
    assert_eq!(shows(&p).len(), 3);
    assert!(shows(&p)[2].1.is_empty(), "no layer named: only the position changes");
}

#[test]
fn layer_values_are_checked_like_pictures() {
    bad("::show{mika body=a eyes=b mouth=c}\n::show{mika eyes=anim:nope}\n", "unknown animation `nope`");
    bad("::show{mika body=a/../../../x eyes=b mouth=c}\n", "inside the game folder");
    bad("::show{mika body={n} eyes=b mouth=c}\n", "must be a text variable");
    bad("::show{mika body={1x} eyes=b mouth=c}\n", "isn't a variable name");
    let p = ok("::show{mika body=a eyes=b mouth={mood}}\n::show{mika mouth=m_{mood}}\n");
    assert_eq!(shows(&p)[0].1[2], pair("mouth", "art/mouth_{mood}.png"));
    assert_eq!(shows(&p)[1].1[0], pair("mouth", "art/mouth_m_{mood}.png"));
}

#[test]
fn poses_and_layers_do_not_mix() {
    bad("::show{mika happy}\n", "has layers (body, eyes, mouth), not poses");
    bad("::show{mika happy eyes=open}\n", "a pose and layers can't be set together");
    bad("::show{mika eyes=open src=x.png}\n", "can't mix layers");
    bad("::show{mika eyes=open anim=blink}\n", "can't mix layers");
    // an unknown layer is an ordinary unknown attribute
    bad("::show{mika nose=big}\n", "unknown attribute `nose`");
    bad("::show{ghost eyes=open}\n", "unknown attribute `eyes`");
}

#[test]
fn a_character_with_poses_and_layers_replaces_the_sprite_by_a_pose() {
    let src = "::character{id=ren sprites=r_{pose}.png layers=body,eyes layer_sprites=r/{layer}_{value}.png}\n\
               ::show{ren body=a eyes=b}\n::show{ren happy}\n::show{ren}\n::show{ren eyes=c}\n";
    let p = parse(src).unwrap();
    assert!(matches!(&p.ops[1], Op::Show { src, layers, .. } if src == "r_happy.png" && layers.is_empty()));
    assert!(matches!(&p.ops[2], Op::Show { src, .. } if src == "r_default.png"), "no layer named: the default pose");
    assert!(matches!(&p.ops[3], Op::Show { src, layers, .. } if src.is_empty() && layers.len() == 1));
    // a first layered show must still name all
    let e = errors("::character{id=ren sprites=r_{pose}.png layers=body,eyes layer_sprites=r/{layer}_{value}.png}\n::show{ren eyes=c}\n");
    assert!(e.iter().any(|m| m.contains("missing: body")), "{e:?}");
}

#[test]
fn layer_files_are_assets_and_their_animations_are_looked_ahead() {
    let p = ok("::show{mika body=a eyes=anim:blink mouth={mood}}\n::show{mika eyes=none mouth=x}\nHi.\n");
    let files: Vec<&str> = p.assets().filter(|(_, k, _)| *k == AssetKind::Image).map(|(_, _, f)| f).collect();
    assert!(files.contains(&"art/body_a.png") && files.contains(&"art/mouth_{mood}.png") && files.contains(&"art/mouth_x.png"));
    assert!(files.contains(&"e/open.png") && files.contains(&"e/closed.png"), "the animation's frames: {files:?}");
    assert!(!files.iter().any(|f| f.is_empty() || f.starts_with("anim:")));
    let ahead = p.images_ahead(0, 10);
    assert!(ahead.contains(&"art/body_a.png") && ahead.contains(&"e/open.png") && ahead.contains(&"art/mouth_x.png"));
    assert!(!ahead.iter().any(|f| f.contains('{')), "patterns load when shown");
}

#[test]
fn the_layers_demo_parses() {
    let p = parse(include_str!("../../../examples/layers.quill.md")).unwrap_or_else(|e| panic!("{e:?}"));
    assert_eq!(p.characters["mika"].layers.len(), 3);
    assert!(shows(&p).iter().any(|(_, l)| l.iter().any(|(_, p)| p == "anim:blink")));
}
