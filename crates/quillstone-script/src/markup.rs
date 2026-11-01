//! Text tags inside lines and choice texts.
//!
//! - `**bold**` and `*italic*` (as in Markdown; `***both***`)
//! - `{color=#ff8080}...{/color}` (they nest)
//! - `{w=0.5}`: the typewriter pauses there for half a second
//! - `{size=+10}`, `{size=-8}`, `{size=24}`...`{/size}`: relative or absolute text size (they nest)
//! - `{p}`: a line break inside the line (a paragraph break); it doesn't wait
//! - `{nw}` at the very end: the story goes on by itself once the line is typed
//! - `\*`, `\{` and `\\` write the character itself; any other `\` is shown as is
//!
//! `{name}` is variable interpolation (done by the runtime before this); a brace
//! pair holding just a name is left as written. Lines are checked when the script
//! is parsed ([`check_markup`]), so displaying one ([`rich`]) never fails.

use crate::expr::is_ident;

/// A `{size=...}` tag: pixels on the 1920x1080 screen, or a change from the surrounding size.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum TextSize {
    Absolute(i32),
    Relative(i32),
}

/// Smallest and largest text size, in pixels (also the limit of a relative one).
pub const MIN_SIZE: i32 = 8;
pub const MAX_SIZE: i32 = 200;

impl TextSize {
    /// The size in pixels when the surrounding text is `base` pixels.
    pub fn resolve(self, base: f32) -> f32 {
        let px = match self {
            TextSize::Absolute(n) => n as f32,
            TextSize::Relative(d) => base + d as f32,
        };
        px.clamp(MIN_SIZE as f32, MAX_SIZE as f32)
    }

    /// `self` written inside text already sized `outer`.
    fn within(self, outer: Option<TextSize>) -> TextSize {
        match (self, outer) {
            (TextSize::Relative(d), Some(TextSize::Relative(c))) => TextSize::Relative(c.saturating_add(d).clamp(-MAX_SIZE, MAX_SIZE)),
            (TextSize::Relative(d), Some(TextSize::Absolute(c))) => {
                TextSize::Absolute(c.saturating_add(d).clamp(MIN_SIZE, MAX_SIZE))
            }
            (size, _) => size,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Style {
    pub bold: bool,
    pub italic: bool,
    /// sRGB, 0..1; `None` is the text's normal colour.
    pub color: Option<[f32; 3]>,
    /// `None` is the text's normal size.
    pub size: Option<TextSize>,
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
    /// The line ended in `{nw}`: the story goes on once it is typed.
    pub no_wait: bool,
}

/// Deepest nesting of `{color}` tags and of `{size}` tags (each counted alone).
pub const MAX_TAG_NESTING: usize = 64;

/// Longest `{w=...}` pause, in seconds.
pub const MAX_PAUSE: f32 = 10.0;

/// Parse tagged text, or say what is wrong with it.
pub fn parse_markup(src: &str) -> Result<Rich, String> {
    let chars: Vec<char> = src.chars().collect();
    let mut out = Rich::default();
    let mut style = Style::default();
    let mut colors: Vec<Option<[f32; 3]>> = Vec::new();
    let mut sizes: Vec<Option<TextSize>> = Vec::new();
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
            // Only `\\`, `\*` and `\{` are escapes; any other backslash is shown (`C:\temp`).
            '\\' if matches!(chars.get(i + 1), Some('\\' | '*' | '{')) => {
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
                    if colors.len() >= MAX_TAG_NESTING {
                        return Err("`{color=...}` tags are nested too deeply".into());
                    }
                    colors.push(style.color);
                    close_run(out.text.len(), style);
                    style.color = Some(color);
                } else if tag == "/size" {
                    let previous = sizes.pop().ok_or("`{/size}` without a `{size=...}` before it")?;
                    close_run(out.text.len(), style);
                    style.size = previous;
                } else if let Some(value) = tag.strip_prefix("size=") {
                    let size = parse_size(value.trim())?;
                    if sizes.len() >= MAX_TAG_NESTING {
                        return Err("`{size=...}` tags are nested too deeply".into());
                    }
                    sizes.push(style.size);
                    close_run(out.text.len(), style);
                    style.size = Some(size.within(style.size));
                } else if tag == "nw" {
                    if chars[close + 1..].iter().any(|c| !c.is_whitespace()) {
                        return Err("`{nw}` must be the last thing in the line".into());
                    }
                    out.no_wait = true;
                } else if let Some(value) = tag.strip_prefix("w=") {
                    let seconds: f32 = value.trim().parse().map_err(|_| format!("`{{w={value}}}` needs a number of seconds"))?;
                    if !(seconds > 0.0 && seconds <= MAX_PAUSE) {
                        return Err(format!("`{{w={value}}}`: a pause is more than 0 and at most {MAX_PAUSE} seconds"));
                    }
                    out.pauses.push((shown, seconds));
                } else if tag == "p" {
                    // A break at the very start would only leave an empty first line.
                    if shown > 0 {
                        out.text.push('\n');
                        shown += 1;
                    }
                } else if is_ident(tag) {
                    // An unknown `{name}`, left as written by interpolation.
                    out.text.push_str(&format!("{{{inner}}}"));
                    shown += inner.chars().count() + 2;
                } else {
                    return Err(format!("unknown text tag `{{{inner}}}` (tags: `{{color=#rrggbb}}`, `{{/color}}`, `{{size=+10}}`, `{{/size}}`, `{{w=0.5}}`, `{{p}}`, `{{nw}}`)"));
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
    if !sizes.is_empty() {
        return Err("`{size=...}` is never closed with `{/size}`".into());
    }
    close_run(out.text.len(), style);
    // A break at the very end would only leave an empty last line (and a pause or
    // style that reached past the shorter text is clamped to it).
    if out.text.ends_with('\n') {
        out.text.truncate(out.text.trim_end_matches('\n').len());
        let (len, chars) = (out.text.len(), out.text.chars().count());
        runs.iter_mut().for_each(|r| r.end = r.end.min(len));
        runs.retain(|r| r.start < r.end);
        out.pauses.iter_mut().for_each(|p| p.0 = p.0.min(chars));
    }
    if runs.iter().any(|r| r.style != Style::default()) {
        out.runs = runs;
    }
    Ok(out)
}

/// `+10`, `-8` (relative) or `24` (absolute pixels).
fn parse_size(v: &str) -> Result<TextSize, String> {
    let n: i32 = v.parse().map_err(|_| format!("`{{size={v}}}` needs a whole number: `+10`, `-8` or `24`"))?;
    if v.starts_with('+') || v.starts_with('-') {
        if n.unsigned_abs() > MAX_SIZE as u32 {
            return Err(format!("`{{size={v}}}`: a size change is at most {MAX_SIZE} either way"));
        }
        Ok(TextSize::Relative(n))
    } else if (MIN_SIZE..=MAX_SIZE).contains(&n) {
        Ok(TextSize::Absolute(n))
    } else {
        Err(format!("`{{size={v}}}`: a text size is from {MIN_SIZE} to {MAX_SIZE} pixels"))
    }
}

/// Whether tagged text is well formed (for the script parser).
pub fn check_markup(src: &str) -> Result<(), String> {
    parse_markup(src).map(|_| ())
}

/// Like [`check_markup`] for a choice option, where `{nw}` and `{p}` make no sense.
pub fn check_option_markup(src: &str) -> Result<(), String> {
    match parse_markup(src)? {
        Rich { no_wait: true, .. } => Err("`{nw}` only works at the end of a dialogue line, not in a choice".into()),
        Rich { text, .. } if text.contains('\n') => Err("`{p}` only works in a dialogue line, not in a choice".into()),
        _ => Ok(()),
    }
}

/// Like [`check_markup`] for tagged text shown in one piece (`place` names it for the message), where `{nw}` and `{p}`
/// make no sense.
pub fn check_inline_markup(src: &str, place: &str) -> Result<(), String> {
    match parse_markup(src)? {
        Rich { no_wait: true, .. } => Err(format!("`{{nw}}` and `{{p}}` only work in dialogue lines, not in {place}")),
        Rich { text, .. } if text.contains('\n') => Err(format!("`{{nw}}` and `{{p}}` only work in dialogue lines, not in {place}")),
        _ => Ok(()),
    }
}

/// Tagged text for display. Text that isn't well formed (it can't come from a
/// checked script) is shown as written rather than lost.
pub fn rich(src: &str) -> Rich {
    parse_markup(src).unwrap_or_else(|_| Rich { text: src.to_string(), ..Rich::default() })
}

/// Just the words, for places that show plain text (history, save previews). A
/// `{p}` stays a line break (the history shows it as one); a one-line preview turns
/// it into a space itself.
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

    fn sizes(r: &Rich) -> Vec<(&str, Option<TextSize>)> {
        r.runs.iter().map(|run| (&r.text[run.start..run.end], run.style.size)).collect()
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
    fn nesting_sizes_cannot_overflow() {
        let big = TextSize::Relative(i32::MAX);
        assert_eq!(big.within(Some(TextSize::Relative(i32::MAX))), TextSize::Relative(MAX_SIZE));
        assert_eq!(TextSize::Relative(i32::MIN).within(Some(TextSize::Relative(i32::MIN))), TextSize::Relative(-MAX_SIZE));
        assert_eq!(big.within(Some(TextSize::Absolute(i32::MAX))), TextSize::Absolute(MAX_SIZE));
    }

    #[test]
    fn sizes_are_relative_or_absolute_and_nest() {
        let r = parse_markup("a {size=+10}b {size=+4}c{/size} d{/size} e {size=24}f {size=-6}g{/size}{/size}").unwrap();
        assert_eq!(r.text, "a b c d e f g");
        use TextSize::*;
        assert_eq!(
            sizes(&r),
            [("a ", None), ("b ", Some(Relative(10))), ("c", Some(Relative(14))), (" d", Some(Relative(10))),
             (" e ", None), ("f ", Some(Absolute(24))), ("g", Some(Absolute(18)))]
        );
        assert_eq!(Relative(14).resolve(38.0), 52.0);
        assert_eq!(Relative(-100).resolve(38.0), 8.0, "never below the minimum");
        assert_eq!(Absolute(24).resolve(38.0), 24.0);
        assert_eq!(Relative(900).resolve(38.0), 200.0);
        // sizes mix with the other tags
        let r = parse_markup("{size=+10}**{color=#f00}x{/color}**{/size}").unwrap();
        assert_eq!(r.runs.len(), 1);
        let s = r.runs[0].style;
        assert!(s.bold && s.color.is_some() && s.size == Some(Relative(10)));
        assert_eq!(plain("a{size=+10}b{/size}"), "ab");
    }

    #[test]
    fn nw_must_end_the_line() {
        let r = parse_markup("Hold on.{nw}").unwrap();
        assert_eq!((r.text.as_str(), r.no_wait), ("Hold on.", true));
        assert!(parse_markup("Trailing space ok {nw} ").unwrap().no_wait);
        assert!(!parse_markup("Plain.").unwrap().no_wait);
        assert!(parse_markup("Well...{w=1}{nw}").unwrap().no_wait);
        assert_eq!(parse_markup("a {nw} b").unwrap_err(), "`{nw}` must be the last thing in the line");
        assert_eq!(plain("Gone.{nw}"), "Gone.");
        assert!(check_option_markup("Pick{nw}").is_err());
        assert!(check_option_markup("Pick {size=+4}me{/size}").is_ok());
        assert!(!parse_markup(&escape("{nw}")).unwrap().no_wait, "a value can't smuggle one in");
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
            ("{size=big}x{/size}", "whole number"),
            ("{size=7}x{/size}", "from 8 to 200"),
            ("{size=201}x{/size}", "from 8 to 200"),
            ("{size=+201}x{/size}", "at most 200"),
            ("{size=-2147483648}x{/size}", "at most 200"),
            ("{size=+2147483647}x{/size}", "at most 200"),
            ("{size=24}open", "never closed"),
            ("{/size}", "without a `{size"),
            ("{nw}more", "last thing"),
            ("{nw}{/size}", "last thing"),
            ("{size=}x{/size}", "whole number"),
            ("{huge=40}big", "unknown text tag"),
        ] {
            let e = parse_markup(src).unwrap_err();
            assert!(e.contains(needle), "{src}: {e}");
        }
        assert_eq!(rich("**open").text, "**open", "shown as written, not lost");
        assert_eq!(parse_markup("a { b").unwrap().text, "a { b", "a lone brace is just a brace");
        assert_eq!(parse_markup("Path is C:\\temp\\new.").unwrap().text, "Path is C:\\temp\\new.", "other backslashes stay");
        assert_eq!(parse_markup("ends with \\").unwrap().text, "ends with \\");
    }

    #[test]
    fn p_is_a_line_break_that_counts_as_one_character() {
        let r = parse_markup("One.{p}Two.").unwrap();
        assert_eq!(r.text, "One.\nTwo.");
        assert!(r.runs.is_empty() && r.pauses.is_empty() && !r.no_wait);
        assert_eq!(plain("One.{p}Two."), "One.\nTwo.", "the history keeps the break");
        assert_eq!(rich("a{p}{p}b").text, "a\n\nb", "two make an empty line");
        // an escaped one is the literal text, and a value can't smuggle a break in
        assert_eq!(parse_markup("\\{p}").unwrap().text, "{p}");
        assert_eq!(parse_markup(&escape("{p}")).unwrap().text, "{p}");
    }

    #[test]
    fn p_at_the_start_or_end_is_harmless() {
        assert_eq!(parse_markup("{p}Hi.").unwrap().text, "Hi.");
        assert_eq!(parse_markup("Hi.{p}").unwrap().text, "Hi.");
        assert_eq!(parse_markup("{p}{p}").unwrap().text, "");
        let r = parse_markup("**Hi.{p}**{w=1}{nw}").unwrap();
        assert_eq!((r.text.as_str(), r.no_wait), ("Hi.", true));
        assert_eq!(r.pauses, [(3, 1.0)], "a pause past the trimmed break is clamped");
        assert_eq!(styles(&r), [("Hi.", true, false, None)]);
    }

    #[test]
    fn p_mixes_with_pauses_sizes_colours_and_typed_counts() {
        let r = parse_markup("Well.{w=0.5}{p}{color=#f00}No{size=+10}!{/size}{/color}{nw}").unwrap();
        assert_eq!(r.text, "Well.\nNo!");
        assert_eq!(r.pauses, [(5, 0.5)], "a pause counts characters up to the break");
        assert!(r.no_wait);
        assert_eq!(r.text.chars().count(), 9, "the break is one typed character");
        let red = Some([1.0, 0.0, 0.0]);
        assert_eq!(styles(&r)[1], ("No", false, false, red));
        assert_eq!(sizes(&r).last(), Some(&("!", Some(TextSize::Relative(10)))));
        // after the break the pause position moves on by one
        assert_eq!(parse_markup("a{p}b{w=1}").unwrap().pauses, [(3, 1.0)]);
    }

    #[test]
    fn p_is_for_dialogue_not_choices() {
        assert!(check_markup("a{p}b").is_ok());
        assert!(check_option_markup("a{p}b").unwrap_err().contains("not in a choice"));
        assert!(check_option_markup("{p}a").is_ok(), "a leading one is ignored, so harmless");
    }

    #[test]
    fn tags_nested_too_deeply_are_refused() {
        let open = "{color=#fff}".repeat(MAX_TAG_NESTING + 1);
        assert!(parse_markup(&format!("{open}a{}", "{/color}".repeat(MAX_TAG_NESTING + 1))).unwrap_err().contains("too deeply"));
        let open = "{size=+1}".repeat(MAX_TAG_NESTING + 1);
        assert!(parse_markup(&format!("{open}a{}", "{/size}".repeat(MAX_TAG_NESTING + 1))).unwrap_err().contains("too deeply"));
        let ok = "{color=#fff}".repeat(MAX_TAG_NESTING);
        assert!(parse_markup(&format!("{ok}a{}", "{/color}".repeat(MAX_TAG_NESTING))).is_ok());
    }
}
