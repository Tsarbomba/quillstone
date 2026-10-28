//! `::include` is resolved when the script is compiled: the runtime sees one program.

use crate::{Cmd, Input, Session};
use quillstone_script::{parse, parse_files, Program};

const WHOLE: &str = "---\nvars: { n: 0 }\n---\n# a\n**A:** one\n::set{n+=1}\n::choice\n- [Go](#b)\n- [Stay](#a)\n::\n# b\n**A:** two {n}\n**A:** three\n";
const MAIN: &str = "---\nvars: { n: 0 }\n---\n# a\n**A:** one\n::set{n+=1}\n::choice\n- [Go](#b)\n- [Stay](#a)\n::\n::include{part.quill.md}\n";
const PART: &str = "# b\n**A:** two {n}\n**A:** three\n";

fn split() -> Program {
    parse_files(MAIN, "main.quill.md", &|p| if p == "part.quill.md" { Ok(PART.into()) } else { Err("no such file".into()) }).unwrap()
}

fn play<'p>(p: &'p Program, inputs: &[Input]) -> (Session<'p>, Vec<Cmd>) {
    let (mut s, mut cmds) = Session::start(p);
    for i in inputs {
        cmds.extend(s.input(i.clone()));
    }
    (s, cmds)
}

const WALK: [Input; 4] = [Input::Advance, Input::Choose(0), Input::Advance, Input::Advance];

#[test]
fn a_split_story_plays_exactly_like_the_whole_one() {
    let (whole, split) = (parse(WHOLE).unwrap(), split());
    assert_eq!(whole.ops, split.ops);
    assert_eq!(whole.labels, split.labels);
    let (a, a_cmds) = play(&whole, &WALK);
    let (b, b_cmds) = play(&split, &WALK);
    assert_eq!(a_cmds, b_cmds);
    assert!(a.state().finished && b.state().finished);
    assert_eq!(a.state().vars, b.state().vars);
}

#[test]
fn a_save_made_before_the_story_was_split_still_loads() {
    let (whole, split) = (parse(WHOLE).unwrap(), split());
    for steps in 0..WALK.len() {
        let (game, _) = play(&whole, &WALK[..steps]);
        let text = game.save();
        let (loaded, _) = Session::load(&split, &text).unwrap();
        assert_eq!(loaded.state().pc, game.state().pc, "{steps} steps in");
        assert_eq!(loaded.history.len(), game.history.len(), "history kept: the ops did not change");
        // and the other way round
        let (back, _) = Session::load(&whole, &play(&split, &WALK[..steps]).0.save()).unwrap();
        assert_eq!(back.state().pc, game.state().pc);
    }
}

#[test]
fn a_save_before_an_include_was_added_ahead_of_its_spot_is_re_found_by_label() {
    let before = parse("# a\n**A:** one\n# b\n**A:** two\n**A:** three\n").unwrap();
    let (game, _) = play(&before, &[Input::Advance, Input::Advance]); // on "three"
    let after = parse_files("# a\n**A:** one\n::include{x.quill.md}\n# b\n**A:** two\n**A:** three\n", "main.quill.md", &|_| Ok("**A:** extra\n**A:** more\n".into())).unwrap();
    let (loaded, _) = Session::load(&after, &game.save()).unwrap();
    assert_eq!(loaded.state().line.as_ref().unwrap().text, "three");
    assert_eq!(loaded.state().pc, 4, "two lines were added above it");
}
