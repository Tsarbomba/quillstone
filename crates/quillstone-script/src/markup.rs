//! Text tags inside lines and choice texts.
//!
//! - `**bold**` and `*italic*` (as in Markdown; `***both***`)
//! - `{color=#ff8080}...{/color}` (they nest)
//! - `{w=0.5}`: the typewriter pauses there for half a second
//! - `\*`, `\{` and `\\` write the character itself
//!
//! `{name}` is variable interpolation (done by the runtime before this); a brace
//! pair holding just a name is left as written. Lines are checked when the script
//! is parsed ([`check_markup`]), so displaying one ([`rich`]) never fails.

use crate::expr::is_ident;

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Style {
    pub bold: bool,
    pub italic: bool,
    /// sRGB, 0..1; `None` is the text's normal colour.
    pub color: Option<[f32; 3]>,
}

/// A stretch of [`Rich::text`] (byte range) in one style.
#[derive(Debug, Clone, PartialEq)]
pub struct TextRun {
    pub start: usize,
    pub end: usize,
    pub style: Style,
}

/// A line with its tags taken out.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Rich {
    /// What is shown, without tags.
    pub text: String,
    /// Styled stretches covering all of `text` in order; empty when it's all plain.
    pub runs: Vec<TextRun>,
    /// Typewriter pauses: (characters shown when it pauses, seconds).
    pub pauses: Vec<(usize, f32)>,
}

/// Longest `{w=...}` pause, in seconds.
pub const MAX_PAUSE: f32 = 10.0;

/// Parse tagged text, or say what is wrong with it.
pub fn parse_markup(src: &str) -> Result<Rich, String> {
    let chars: Vec<char> = src.chars().collect();
    let mut out = Rich::default();
    let mut style = Style::default();
    let mut colors: Vec<Option<[f32; 3]>> = Vec::new();
    let mut runs: Vec<TextRun> = Vec::new();
    let mut run_start = 0;
    let mut shown = 0; // characters in out.text
    // End the run so far (up to `end`, in `style`) because the style changes there.
    let mut close_run = |end: usize, style: Style| {
        if end > run_start {
            runs.push(TextRun { start: run_start, end, style });
        }
        run_start = end;
    };
    let mut i = 0;
    while i < chars.len() {
        match chars[i] {
            '\\' if i + 1 < chars.len() => {
                out.text.push(chars[i + 1]);
                shown += 1;
                i += 2;
            }
            '*' if chars.get(i + 1) == Some(&'*') => {
                close_run(out.text.len(), style);
                style.bold = !style.bold;
                i += 2;
            }
            '*' => {
                close_run(out.text.len(), style);
                style.italic = !style.italic;
                i += 1;
            }
            '{' if !chars[i + 1..].contains(&'}') => {
                out.text.push('{'); // a brace with no `}` after it can only be a brace
                shown += 1;
                i += 1;
            }
            '{' => {
                let close = (i + 1..chars.len()).find(|&k| chars[k] == '}').expect("checked just above");
                let inner: String = chars[i + 1..close].iter().collect();
                let tag = inner.trim();
                if tag == "/color" {
                    let previous = colors.pop().ok_or("`{/color}` without a `{color=...}` before it")?;
                    close_run(out.text.len(), style);
                    style.color = previous;
                } else if let Some(value) = tag.strip_prefix("color=") {
                    let color = crate::parser::parse_color(value.trim())?;
                    colors.push(style.color);
                    close_run(out.text.len(), style);
                    style.color = Some(color);
                } else if let Some(value) = tag.strip_prefix("w=") {
                    let seconds: f32 = value.trim().parse().map_err(|_| format!("`{{w={value}}}` needs a number of seconds"))?;
                    if !(seconds > 0.0 && seconds <= MAX_PAUSE) {
                        return Err(format!("`{{w={value}}}`: a pause is more than 0 and at most {MAX_PAUSE} seconds"));
                    }
                    out.pauses.push((shown, seconds));
                } else if is_ident(tag) {
                    // An unknown `{name}`, left as written by interpolation.
                    out.text.push_str(&format!("{{{inner}}}"));
                    shown += inner.chars().count() + 2;
                } else {
                    return Err(format!("unknown text tag `{{{inner}}}` (tags: `{{color=#rrggbb}}`, `{{/color}}`, `{{w=0.5}}`)"));
                }
                i = close + 1;
            }
            c => {
                out.text.push(c);
                shown += 1;
                i += 1;
            }
        }
    }
    if style.bold {
        return Err("`**` (bold) is never closed (write `\\*` for an asterisk)".into());
    }
    if style.italic {
        return Err("`*` (italic) is never closed (write `\\*` for an asterisk)".into());
    }
    if !colors.is_empty() {
        return Err("`{color=...}` is never closed with `{/color}`".into());
    }
    close_run(out.text.len(), style);
    if runs.iter().any(|r| r.style != Style::default()) {
        out.runs = runs;
    }
    Ok(out)
}

/// Whether tagged text is well formed (for the script parser).
pub fn check_markup(src: &str) -> Result<(), String> {
    parse_markup(src).map(|_| ())
}

/// Tagged text for display. Text that isn't well formed (it can't come from a
/// checked script) is shown as written rather than lost.
pub fn rich(src: &str) -> Rich {
    parse_markup(src).unwrap_or_else(|_| Rich { text: src.to_string(), ..Rich::default() })
}

/// Just the words, for places that show plain text (history, save previews).
pub fn plain(src: &str) -> String {
    rich(src).text
}

/// Text that shows exactly as written (a variable's value put into a line).
pub fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        if matches!(c, '\\' | '*' | '{') {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn styles(r: &Rich) -> Vec<(&str, bool, bool, Option<[f32; 3]>)> {
        r.runs.iter().map(|run| (&r.text[run.start..run.end], run.style.bold, run.style.italic, run.style.color)).collect()
    }

    #[test]
    fn plain_text_has_no_runs() {
        let r = parse_markup("Just words.").unwrap();
        assert_eq!((r.text.as_str(), r.runs.len(), r.pauses.len()), ("Just words.", 0, 0));
    }

    #[test]
    fn bold_italic_and_both() {
        let r = parse_markup("a **b** *c* ***d***").unwrap();
        assert_eq!(r.text, "a b c d");
        assert_eq!(
            styles(&r),
            [("a ", false, false, None), ("b", true, false, None), (" ", false, false, None), ("c", false, true, None),
             (" ", false, false, None), ("d", true, true, None)]
        );
    }

    #[test]
    fn colours_nest_and_restore() {
        let r = parse_markup("{color=#f00}red {color=#00f}blue{/color} red{/color} plain").unwrap();
        assert_eq!(r.text, "red blue red plain");
        let red = Some([1.0, 0.0, 0.0]);
        assert_eq!(styles(&r), [("red ", false, false, red), ("blue", false, false, Some([0.0, 0.0, 1.0])),
                                (" red", false, false, red), (" plain", false, false, None)]);
    }

    #[test]
    fn pauses_count_shown_characters() {
        let r = parse_markup("Well...{w=0.5} *no*.{w=1}").unwrap();
        assert_eq!(r.text, "Well... no.");
        assert_eq!(r.pauses, [(7, 0.5), (11, 1.0)]);
    }

    #[test]
    fn escapes_and_unknown_names_are_literal() {
        let r = parse_markup("2 \\* 3 = 6, \\{braces}, back\\\\slash, {nobody}").unwrap();
        assert_eq!(r.text, "2 * 3 = 6, {braces}, back\\slash, {nobody}");
        assert!(r.runs.is_empty());
        let value = "a*b{w=9}\\";
        assert_eq!(parse_markup(&escape(value)).unwrap().text, value, "escaped values show as written");
    }

    #[test]
    fn mistakes_are_explained() {
        for (src, needle) in [
            ("**open", "bold"),
            ("*open", "italic"),
            ("{color=#f00}open", "never closed"),
            ("{/color}", "without a `{color"),
            ("{color=red}x{/color}", "bad colour"),
            ("{w=fast}", "number of seconds"),
            ("{w=0}", "more than 0"),
            ("{size=40}big", "unknown text tag"),
        ] {
            let e = parse_markup(src).unwrap_err();
            assert!(e.contains(needle), "{src}: {e}");
        }
        assert_eq!(rich("**open").text, "**open", "shown as written, not lost");
        assert_eq!(parse_markup("a { b").unwrap().text, "a { b", "a lone brace is just a brace");
    }
}
