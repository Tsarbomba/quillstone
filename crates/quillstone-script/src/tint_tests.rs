//! `::tint` in the parser.

use crate::test_util::{bad, ok};
use crate::{Op, Program, Tint};

fn set(p: &Program, at: usize) -> (Option<Tint>, f32) {
    match &p.ops[at] {
        Op::Tint { set, fade } => (*set, *fade),
        other => panic!("not a tint: {other:?}"),
    }
}

#[test]
fn a_tint_has_a_colour_a_default_alpha_and_no_fade() {
    let p = ok("::tint{color=#203060}\nHi.\n");
    let (t, fade) = set(&p, 0);
    let t = t.unwrap();
    assert!((t.color[0] - 32.0 / 255.0).abs() < 1e-6 && (t.color[2] - 96.0 / 255.0).abs() < 1e-6);
    assert_eq!((t.alpha, fade), (0.4, 0.0));
}

#[test]
fn every_attribute_is_read() {
    let p = ok("::tint{color=#fff alpha=0.9 fade=10}\n::tint{color=#000 alpha=0 fade=0.5}\n::tint{color=#000 alpha=50%}\nHi.\n");
    assert_eq!(set(&p, 0), (Some(Tint { color: [1.0; 3], alpha: 0.9 }), 10.0));
    assert_eq!(set(&p, 1), (Some(Tint { color: [0.0; 3], alpha: 0.0 }), 0.5));
    assert_eq!(set(&p, 2).0.unwrap().alpha, 0.5);
}

#[test]
fn none_and_off_remove_it_and_take_only_a_fade() {
    let p = ok("::tint{none}\n::tint{off fade=2}\nHi.\n");
    assert_eq!(set(&p, 0), (None, 0.0));
    assert_eq!(set(&p, 1), (None, 2.0));
    assert_eq!(p.assets().count(), 0, "a tint needs no file");
}

#[test]
fn mistakes_have_clear_errors_with_line_numbers() {
    bad("Hi.\n::tint{color=#12}\n", "2: bad colour `#12`");
    bad("::tint{color=blue}\n", "bad colour `blue`");
    bad("::tint{color={c}}\n", "bad colour");
    bad("::tint{alpha=0.4}\n", "needs `color=");
    bad("::tint{}\n", "needs `color=");
    bad("::tint{color=#fff alpha=0.95}\n", "`alpha=0.95` must be a number from 0 to 0.9");
    bad("::tint{color=#fff alpha=-0.1}\n", "`alpha=-0.1`");
    bad("::tint{color=#fff alpha=dark}\n", "`alpha=dark`");
    bad("::tint{color=#fff alpha=NaN}\n", "`alpha=NaN`");
    bad("::tint{color=#fff fade=11}\n", "at most 10");
    bad("::tint{color=#fff fade=-1}\n", "`fade=-1`");
    bad("::tint{none fade=11}\n", "at most 10");
    bad("::tint{none color=#fff}\n", "takes only `fade=`, not `color=`");
    bad("::tint{off alpha=0.2}\n", "takes only `fade=`, not `alpha=`");
    bad("::tint{color=#fff mode=multiply}\n", "mode");
    bad("::tint{color=#fff alpha=0.3 fade={t}}\n", "`fade={t}`");
}
