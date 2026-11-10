//! The full-screen page (`::page`): the paragraphs of its lines, and which of them are on the screen page that
//! holds the newest line. Pure functions of the lines, the theme and the player's text scale, so the same lines
//! always make the same picture (rollback, a load and a redraw included) and the earlier lines of a screen page
//! stay where they are while the page grows.

use quillstone_script::markup::{rich, Style, TextRun};

use crate::layout::{estimate_lines_styled, line_height, Rect};
use crate::theme::Theme;

/// A page line as the view keeps it: its words with the tags taken out (see [`quillstone_script::markup`]).
#[derive(Debug, Clone, PartialEq)]
pub struct PageLine {
    pub speaker: Option<String>,
    pub color: Option<[f32; 3]>,
    pub text: String,
    pub runs: Vec<TextRun>,
}

impl PageLine {
    pub fn new(speaker: Option<String>, color: Option<[f32; 3]>, tagged: &str) -> PageLine {
        let r = rich(tagged);
        PageLine { speaker, color, text: r.text, runs: r.runs }
    }

    /// The paragraph drawn for it: a speaker's name in their colour in front of the words, as one run of text
    /// (`Mika: Hello.`), which wraps and is measured like any other text.
    pub fn paragraph(&self, theme: &Theme) -> Paragraph {
        let Some(name) = &self.speaker else {
            return Paragraph { text: self.text.clone(), runs: self.runs.clone(), name_chars: 0 };
        };
        let prefix = format!("{name}: ");
        let [r, g, b, _] = theme.name_color;
        let named = Style { color: Some(self.color.unwrap_or([r, g, b])), ..Style::default() };
        let mut runs = vec![TextRun { start: 0, end: prefix.len(), style: named, ruby: None }];
        if self.runs.is_empty() {
            runs.push(TextRun { start: prefix.len(), end: prefix.len() + self.text.len(), style: Style::default(), ruby: None });
        }
        runs.extend(self.runs.iter().map(|run| TextRun { start: run.start + prefix.len(), end: run.end + prefix.len(), ..run.clone() }));
        Paragraph { name_chars: prefix.chars().count(), text: prefix + &self.text, runs }
    }
}

/// One paragraph of the page, ready to measure and draw.
#[derive(Debug, Clone, PartialEq)]
pub struct Paragraph {
    pub text: String,
    pub runs: Vec<TextRun>,
    /// How many characters of `text` are the name (a typing line shows those at once).
    pub name_chars: usize,
}

/// A paragraph of the screen page: its index among the paragraphs, where it goes and the size it is drawn at.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Placed {
    pub index: usize,
    pub rect: Rect,
    pub size: f32,
}

/// The paragraphs on the screen page that holds the last of `paras`. Paragraphs fill `theme.page_rect` from the
/// top, in order, with half a line between them, and the one that no longer fits starts the next screen page: the
/// pages are cut from the first paragraph on, so they don't depend on which line is the newest. A paragraph is
/// drawn at the player's text size, or the largest size below it that fits the page by itself (see
/// [`crate::view::fitted_size`]), so it can be taller than the page only at the theme's own size.
pub fn screen_page(theme: &Theme, text_scale: f32, paras: &[Paragraph]) -> Vec<Placed> {
    let [x, y, w, h] = theme.page_rect;
    let gap = line_height(theme.text_size * text_scale) / 2.0;
    let mut placed: Vec<Placed> = Vec::new();
    let mut used = 0.0;
    for (index, p) in paras.iter().enumerate() {
        let height = |size: f32| estimate_lines_styled(&p.text, &p.runs, size, w) as f32 * line_height(size);
        let size = crate::view::fitted_size(theme, text_scale, |size| height(size) <= h);
        let tall = height(size);
        if !placed.is_empty() && used + gap + tall > h {
            placed.clear();
            used = 0.0;
        }
        let top = if placed.is_empty() { 0.0 } else { used + gap };
        placed.push(Placed { index, rect: Rect::new(x, y + top, w, tall), size });
        used = top + tall;
    }
    placed
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layout::{Size, VIRTUAL};
    use crate::view::{textbox_rect, Draw, View};
    use quillstone_runtime::{Cmd, Input, Line, Session};
    use quillstone_script::parse;

    fn no_images(_: &str) -> Option<Size> {
        None
    }

    fn line(speaker: Option<&str>, text: &str) -> Line {
        Line { speaker: speaker.map(str::to_string), color: None, text: text.into() }
    }

    /// What the runtime sends for the newest of `lines` being shown on a page.
    fn shown(lines: &[Line]) -> Vec<Cmd> {
        let last = lines.last().unwrap();
        vec![Cmd::Page { lines: lines.to_vec() }, Cmd::Say { speaker: last.speaker.clone(), color: last.color, text: last.text.clone() }]
    }

    fn numbered(n: usize) -> Vec<Line> {
        (0..n).map(|i| line(None, &format!("Paragraph {i} has some words in it, enough to take a line or two on the page."))).collect()
    }

    /// The text draws of the page: (text, visible_chars, rect, size).
    fn texts(v: &View) -> Vec<(String, usize, Rect, f32)> {
        v.draw_list(&no_images, None)
            .into_iter()
            .filter_map(|d| match d {
                Draw::Text { text, visible_chars, rect, size, .. } => Some((text, visible_chars, rect, size)),
                _ => None,
            })
            .filter(|(t, ..)| t.starts_with("Paragraph") || t.contains("Mika: "))
            .collect()
    }

    #[test]
    fn a_page_line_draws_no_textbox_name_plate_or_portrait_but_a_dim_and_the_text_in_the_page_area() {
        let theme = Theme::default();
        let mut v = View::new().with_speed(0.0);
        v.apply(&shown(&[line(Some("Mika"), "Hello there.")]));
        let list = v.draw_list(&no_images, None);
        let screen = Rect::new(0.0, 0.0, VIRTUAL.w, VIRTUAL.h);
        assert!(list.iter().any(|d| matches!(d, Draw::Rect { rect, color } if *rect == screen && *color == theme.page_color)));
        assert!(!list.iter().any(|d| matches!(d, Draw::Rect { rect, .. } if *rect == textbox_rect(&theme))), "no textbox");
        assert!(!list.iter().any(|d| matches!(d, Draw::Text { text, .. } if text == "Mika")), "no name plate text");
        let [px, py, pw, ph] = theme.page_rect;
        for d in &list {
            if let Draw::Text { text, rect, .. } = d {
                assert!(text.contains("Hello"));
                assert!(rect.x >= px && rect.y >= py && rect.x + rect.w <= px + pw && rect.y + rect.h <= py + ph, "{rect:?}");
            }
        }
        // the name is a coloured run in front of the words
        let Some(Draw::Text { text, runs, .. }) = list.iter().find(|d| matches!(d, Draw::Text { .. })) else { panic!("no text") };
        assert_eq!(text, "Mika: Hello there.");
        assert_eq!((runs[0].start, runs[0].end, runs[0].style.color), (0, 6, Some([theme.name_color[0], theme.name_color[1], theme.name_color[2]])));
        assert_eq!((runs[1].start, runs[1].end), (6, text.len()));
    }

    #[test]
    fn earlier_lines_are_whole_and_only_the_newest_types_out() {
        let mut v = View::new().with_speed(10.0);
        let all = numbered(3);
        v.apply(&shown(&all));
        v.tick(0.5); // 5 characters of the newest
        let drawn = texts(&v);
        assert_eq!(drawn.len(), 3);
        assert_eq!((drawn[0].1, drawn[1].1), (usize::MAX, usize::MAX));
        assert_eq!(drawn[2].1, 5);
        assert!(drawn[0].2.y < drawn[1].2.y && drawn[1].2.y < drawn[2].2.y);
        // a speaker's name shows at once: it counts before the typed characters
        v.apply(&shown(&[line(Some("Mika"), "Hello there.")]));
        v.tick(0.5);
        let Some(Draw::Text { visible_chars, .. }) = v.draw_list(&no_images, None).into_iter().find(|d| matches!(d, Draw::Text { .. })) else { panic!() };
        assert_eq!(visible_chars, "Mika: ".len() + 5);
    }

    #[test]
    fn paragraphs_are_a_half_line_apart_and_a_full_page_turns_with_the_newest_on_it() {
        let theme = Theme::default();
        let gap = line_height(theme.text_size) / 2.0;
        let mut v = View::new().with_speed(0.0);
        v.apply(&shown(&numbered(2)));
        let t = texts(&v);
        assert_eq!(t[0].2.y, theme.page_rect[1]);
        assert!((t[1].2.y - (t[0].2.y + t[0].2.h + gap)).abs() < 1e-3);
        // grow the page one line at a time: the lines of the screen page only ever lose old ones
        let all = numbered(40);
        let mut previous: Vec<String> = Vec::new();
        let mut turned = 0;
        for n in 1..=all.len() {
            v.apply(&shown(&all[..n]));
            let now: Vec<String> = texts(&v).into_iter().map(|t| t.0).collect();
            assert_eq!(now.last().unwrap(), &all[n - 1].text, "the newest is on the screen page");
            let [_, y, _, h] = theme.page_rect;
            assert!(texts(&v).iter().all(|t| t.2.y + t.2.h <= y + h + 1e-3), "inside the page area");
            if now.len() == 1 && !previous.is_empty() {
                turned += 1; // a fresh screen page starts with the line that didn't fit
            } else {
                assert_eq!(now[..now.len() - 1], previous[..], "same earlier lines as one step before");
            }
            previous = now;
        }
        assert!(turned >= 2, "forty paragraphs fill the page more than twice: {turned}");
    }

    #[test]
    fn the_text_scale_applies_and_a_paragraph_taller_than_the_page_is_drawn_at_the_largest_size_that_fits() {
        let size = Theme::default().text_size;
        let mut v = View::new().with_speed(0.0).with_text_scale(1.5);
        v.apply(&shown(&numbered(1)));
        assert!((texts(&v)[0].3 - size * 1.5).abs() < 1e-3);
        let mut huge = String::from("Paragraph");
        while estimate_lines_styled(&huge, &[], size * 1.5, 1400.0) as f32 * line_height(size * 1.5) <= 840.0 {
            huge.push_str(" word");
        }
        v.apply(&shown(&[line(None, &huge)]));
        let drawn = texts(&v)[0].3;
        assert!(drawn >= size && drawn < size * 1.5, "{drawn}");
        assert!(estimate_lines_styled(&huge, &[], drawn, 1400.0) as f32 * line_height(drawn) <= 840.0 || drawn == size);
    }

    #[test]
    fn hiding_the_textbox_hides_the_page_and_showing_it_brings_it_back() {
        let mut v = View::new().with_speed(0.0);
        v.apply(&shown(&numbered(3)));
        v.apply(&[Cmd::Textbox { hidden: true, fade: 0.0 }]);
        let hidden = v.draw_list(&no_images, None);
        assert!(texts(&v).is_empty() && !hidden.iter().any(|d| matches!(d, Draw::Rect { rect, .. } if rect.w >= VIRTUAL.w)));
        v.apply(&[Cmd::Textbox { hidden: false, fade: 0.0 }]);
        assert_eq!(texts(&v).len(), 3);
        v.set_hide_ui(true);
        assert!(texts(&v).is_empty());
    }

    #[test]
    fn clearing_the_page_returns_to_the_textbox_and_a_page_op_leaves_no_line_behind() {
        let mut v = View::new().with_speed(0.0);
        v.apply(&shown(&numbered(2)));
        v.apply(&[Cmd::Page { lines: Vec::new() }]);
        assert!(v.dialogue.is_none() && v.page.is_empty());
        v.apply(&[Cmd::Page { lines: Vec::new() }, Cmd::Say { speaker: None, color: None, text: "Plain.".into() }]);
        let list = v.draw_list(&no_images, None);
        assert!(list.iter().any(|d| matches!(d, Draw::Rect { rect, .. } if *rect == textbox_rect(&Theme::default()))));
    }

    #[test]
    fn a_view_rebuilt_from_the_state_draws_what_the_live_view_drew() {
        let src = "::character{id=mika name=Mika color=#ffd98c}\nBefore.\n::page\nOne {u}under{/u}.\n\n**Mika:** Two.\n\nThree.\n\n::choice\n- [A](#a)\n::\n# a\nFour.\n";
        let program = parse(src).unwrap();
        let (mut game, first) = Session::start(&program);
        let mut live = View::new().with_speed(0.0);
        live.apply(&first);
        for _ in 0..5 {
            let cmds = match game.state().pending {
                Some(Cmd::ShowChoices { .. }) => game.input(Input::Choose(0)),
                _ => game.input(Input::Advance),
            };
            live.apply(&cmds);
            let mut rebuilt = View::new().with_speed(0.0);
            rebuilt.apply(&game.state().redraw());
            assert_eq!(live.draw_list(&no_images, None), rebuilt.draw_list(&no_images, None), "at {:?}", game.state().line);
        }
        // and back one step: the page is the earlier one again
        let cmds = game.rollback().unwrap();
        live.apply(&cmds);
        let mut rebuilt = View::new().with_speed(0.0);
        rebuilt.apply(&game.state().redraw());
        assert_eq!(live.draw_list(&no_images, None), rebuilt.draw_list(&no_images, None));
    }

    #[test]
    fn a_textbox_game_draws_the_same_list_as_before() {
        let mut v = View::new().with_speed(0.0);
        v.apply(&[Cmd::Say { speaker: Some("Mika".into()), color: None, text: "Hi.".into() }]);
        let list = v.draw_list(&no_images, None);
        let theme = Theme::default();
        assert!(list.iter().any(|d| matches!(d, Draw::Rect { rect, .. } if *rect == textbox_rect(&theme))));
        assert!(!list.iter().any(|d| matches!(d, Draw::Rect { rect, .. } if rect.w >= VIRTUAL.w)), "no page dim");
    }
}
