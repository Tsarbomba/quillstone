//! `::particles` in the parser.

use crate::test_util::{bad, ok};
use crate::{AssetKind, Op, ParticleKind, Particles, Program};

fn set(p: &Program, at: usize) -> (Option<Particles>, f32) {
    match &p.ops[at] {
        Op::Particles { set, fade } => (set.clone(), *fade),
        other => panic!("not particles: {other:?}"),
    }
}

#[test]
fn snow_and_rain_have_defaults() {
    let p = ok("::particles{snow}\n::particles{rain}\nHi.\n");
    let (snow, fade) = set(&p, 0);
    assert_eq!(snow, Some(Particles { kind: ParticleKind::Snow, amount: 60, speed: 1.0, wind: 0.0, back: false }));
    assert_eq!(fade, 1.0);
    assert_eq!(set(&p, 1).0.unwrap().kind, ParticleKind::Rain);
}

#[test]
fn every_attribute_is_read() {
    let p = ok("::particles{rain amount=200 speed=2.5 wind=-1.5 fade=0 layer=back}\n::particles{snow layer=front}\nHi.\n");
    let (rain, fade) = set(&p, 0);
    assert_eq!(rain, Some(Particles { kind: ParticleKind::Rain, amount: 200, speed: 2.5, wind: -1.5, back: true }));
    assert_eq!(fade, 0.0);
    assert!(!set(&p, 1).0.unwrap().back);
}

#[test]
fn a_picture_is_a_particle_and_an_asset() {
    let p = ok("::particles{src=assets/petal.png amount=10}\nHi.\n");
    assert_eq!(set(&p, 0).0.unwrap().kind, ParticleKind::Image("assets/petal.png".into()));
    assert_eq!(p.assets().collect::<Vec<_>>(), [(0, AssetKind::Image, "assets/petal.png")]);
    assert_eq!(p.images_ahead(0, 10), ["assets/petal.png"]);
    assert_eq!(p.assets_with_lines().next().map(|(l, _, _)| l), Some(1));
}

#[test]
fn none_and_off_stop_them() {
    let p = ok("::particles{none fade=2}\n::particles{off}\nHi.\n");
    assert_eq!(set(&p, 0), (None, 2.0));
    assert_eq!(set(&p, 1), (None, 1.0));
    assert_eq!(p.assets().count(), 0);
}

#[test]
fn mistakes_name_the_line() {
    bad("Hi.\n::particles\n", "2: `::particles` needs");
    bad("Hi.\n::particles{hail}\n", "2: `hail` is not a kind of particle");
    bad("::particles{snow rain}\n", "not snow and rain");
    bad("::particles{snow src=a.png}\n", "not snow and src");
    bad("::particles{none src=a.png}\n", "not none and src");
    bad("::particles{none amount=3}\n", "takes only `fade=`");
    bad("::particles{snow amount=0}\n", "amount=0");
    bad("::particles{snow amount=201}\n", "amount=201");
    bad("::particles{snow amount=1.5}\n", "amount=1.5");
    bad("::particles{snow speed=0.1}\n", "speed=0.1");
    bad("::particles{snow speed=6}\n", "speed=6");
    bad("::particles{snow wind=3}\n", "wind=3");
    bad("::particles{snow wind=nan}\n", "wind=nan");
    bad("::particles{snow fade=-1}\n", "fade=-1");
    bad("::particles{snow fade=11}\n", "at most 10");
    bad("::particles{snow layer=middle}\n", "layer=middle");
    bad("::particles{snow colour=red}\n", "unknown attribute `colour`");
}

#[test]
fn the_picture_must_be_a_safe_picture_without_variables() {
    bad("::particles{src=../p.png}\n", "inside the game folder");
    bad("::particles{src=/p.png}\n", "inside the game folder");
    bad("::particles{src=p.ogg}\n", "must be a .png, .jpg or .webp");
    bad("---\nvars: { k: \"a\" }\n---\n::particles{src=\"p_{k}.png\"}\n", "can't use a `{variable}`");
    bad("::particles{src=}\n", "needs");
}
