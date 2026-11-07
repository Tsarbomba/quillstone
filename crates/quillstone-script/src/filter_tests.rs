//! `::filter` in the parser.

use crate::test_util::{bad, ok};
use crate::{Filter, Op, Program};

fn set(p: &Program, at: usize) -> (Option<Filter>, f32) {
    match &p.ops[at] {
        Op::Filter { set, fade } => (*set, *fade),
        other => panic!("not a filter: {other:?}"),
    }
}

#[test]
fn what_is_left_out_is_neutral() {
    let p = ok("::filter{gray=1 fade=2}\nHi.\n");
    let (f, fade) = set(&p, 0);
    assert_eq!(f, Some(Filter { gray: 1.0, ..Filter::default() }));
    assert_eq!(fade, 2.0);
    assert_eq!(set(&ok("::filter{gray=0}\nHi.\n"), 0).0, Some(Filter::default()), "given, so still a filter");
}

#[test]
fn every_attribute_is_read() {
    let p = ok("::filter{gray=0.5 sepia=0.8 brightness=0.8 contrast=1.1 saturation=3 fade=10}\n::filter{saturation=50%}\nHi.\n");
    assert_eq!(set(&p, 0), (Some(Filter { gray: 0.5, sepia: 0.8, brightness: 0.8, contrast: 1.1, saturation: 3.0 }), 10.0));
    assert_eq!(set(&p, 1).0.unwrap().saturation, 0.5);
}

#[test]
fn none_and_off_remove_it_and_take_only_a_fade() {
    let p = ok("::filter{none}\n::filter{off fade=2}\nHi.\n");
    assert_eq!(set(&p, 0), (None, 0.0));
    assert_eq!(set(&p, 1), (None, 2.0));
    assert_eq!(p.assets().count(), 0, "a filter needs no file");
}

#[test]
fn mistakes_have_clear_errors_with_line_numbers() {
    bad("Hi.\n::filter{gray=2}\n", "2: `gray=2` must be a number from 0 to 1");
    bad("::filter{}\n", "needs `gray`");
    bad("::filter{fade=1}\n", "needs `gray`");
    bad("::filter{sepia=-0.1}\n", "`sepia=-0.1`");
    bad("::filter{brightness=3.5}\n", "`brightness=3.5` must be a number from 0 to 3");
    bad("::filter{contrast=-1}\n", "`contrast=-1`");
    bad("::filter{saturation=NaN}\n", "`saturation=NaN`");
    bad("::filter{gray=inf}\n", "`gray=inf`");
    bad("::filter{gray=dark}\n", "`gray=dark`");
    bad("::filter{gray={g}}\n", "`gray={g}`");
    bad("::filter{gray=1 fade=11}\n", "at most 10");
    bad("::filter{gray=1 fade=-1}\n", "`fade=-1`");
    bad("::filter{none fade=11}\n", "at most 10");
    bad("::filter{none gray=1}\n", "takes only `fade=`, not `gray=`");
    bad("::filter{off contrast=1}\n", "takes only `fade=`, not `contrast=`");
    bad("::filter{gray=1 mode=multiply}\n", "mode");
    bad("::filter{gray=1 fade={t}}\n", "`fade={t}`");
}
