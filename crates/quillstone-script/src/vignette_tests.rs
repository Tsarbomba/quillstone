//! `::vignette` in the parser.

use crate::test_util::{bad, ok};
use crate::{Op, Program, Vignette};

fn set(p: &Program, at: usize) -> (Option<Vignette>, f32) {
    match &p.ops[at] {
        Op::Vignette { set, fade } => (*set, *fade),
        other => panic!("not a vignette: {other:?}"),
    }
}

#[test]
fn what_is_left_out_is_the_default() {
    let p = ok("::vignette{strength=0.6 fade=1}\n::vignette{color=#ff0000}\n::vignette{size=25%}\nHi.\n");
    assert_eq!(set(&p, 0), (Some(Vignette { color: [0.0; 3], strength: 0.6, size: 0.5 }), 1.0));
    assert_eq!(set(&p, 1), (Some(Vignette { color: [1.0, 0.0, 0.0], strength: 0.5, size: 0.5 }), 0.0));
    assert_eq!(set(&p, 2).0.unwrap().size, 0.25);
    assert_eq!(p.assets().count(), 0, "a vignette needs no file");
}

#[test]
fn none_and_off_remove_it_and_take_only_a_fade() {
    let p = ok("::vignette{none}\n::vignette{off fade=2}\nHi.\n");
    assert_eq!(set(&p, 0), (None, 0.0));
    assert_eq!(set(&p, 1), (None, 2.0));
}

#[test]
fn mistakes_have_clear_errors_with_line_numbers() {
    bad("Hi.\n::vignette{}\n", "2: `::vignette` needs");
    bad("::vignette{fade=1}\n", "needs `strength=`");
    bad("::vignette{strength=1.5}\n", "`strength=1.5` must be a number from 0 to 1");
    bad("::vignette{size=-0.1}\n", "`size=-0.1`");
    bad("::vignette{size=NaN}\n", "`size=NaN`");
    bad("::vignette{strength=inf}\n", "`strength=inf`");
    bad("::vignette{strength=dark}\n", "`strength=dark`");
    bad("::vignette{color=blue}\n", "bad colour `blue`");
    bad("::vignette{strength={s}}\n", "`strength={s}`");
    bad("::vignette{strength=0.5 fade=11}\n", "at most 10");
    bad("::vignette{strength=0.5 fade=-1}\n", "`fade=-1`");
    bad("::vignette{none size=0.2}\n", "takes only `fade=`, not `size=`");
    bad("::vignette{strength=0.5 shape=round}\n", "shape");
}
