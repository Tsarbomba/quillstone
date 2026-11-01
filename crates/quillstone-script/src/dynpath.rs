//! Picture names with `{variable}` parts (`assets/places/{place}.png`).
//!
//! A path that contains `{name}` is a *pattern*: while the story runs, each
//! `{name}` is replaced by the text variable's value, but only when that value
//! is a safe file-name fragment (see [`unsafe_reason`]). Everything that has to
//! know the files up front (`check`, `package`) reads a pattern as a glob where
//! each `{name}` matches one safe fragment inside its folder ([`pattern_matches`]).
//!
//! Braces are not allowed in game file names; there is no escape for them.

use crate::{is_inside, is_ident, normal_path, Value, Vars};

/// What the game carries for a picture whose name could not be built. It is a
/// harmless path inside the game folder (so saves keep loading) that no real
/// file can have, because a fragment never starts with a dot: the picture
/// simply isn't there, like a missing file.
pub const MISSING_PICTURE: &str = ".missing.png";

/// The picture file types a name with `{variable}` parts may end in.
pub const PICTURE_EXTENSIONS: [&str; 4] = ["png", "jpg", "jpeg", "webp"];

/// The most characters a substituted value may have.
pub const MAX_FRAGMENT: usize = 64;

/// The most `{variable}` parts one name may have.
pub const MAX_PATTERN_VARS: usize = 4;

/// One part of a picture name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Piece<'a> {
    Lit(&'a str),
    Var(&'a str),
}

/// Whether a picture name has `{variable}` parts (or a stray brace, which
/// [`pieces`] reports).
pub fn is_pattern(path: &str) -> bool {
    path.contains(['{', '}'])
}

/// Split a name into literal text and `{variable}` parts, checking it:
/// braces must pair up, a variable name must be a plain or dotted name
/// (`place`, `hero.mood`, no spaces), at most [`MAX_PATTERN_VARS`] of them, and
/// the file extension must be written out (the kind of file can't depend on a variable).
pub fn pieces(path: &str) -> Result<Vec<Piece<'_>>, String> {
    let mut out = Vec::new();
    let mut rest = path;
    while let Some(pos) = rest.find(['{', '}']) {
        if rest[pos..].starts_with('}') {
            return Err(format!("`}}` without a `{{` in `{path}`"));
        }
        if pos > 0 {
            out.push(Piece::Lit(&rest[..pos]));
        }
        let after = &rest[pos + 1..];
        let Some(end) = after.find(['{', '}']).filter(|&e| after[e..].starts_with('}')) else {
            return Err(format!("`{{` without a closing `}}` in `{path}` (braces can't be part of a file name)"));
        };
        let name = &after[..end];
        if name.is_empty() {
            return Err(format!("empty `{{}}` in `{path}`: name a text variable, like `{{place}}`"));
        }
        if !is_ident(name) {
            return Err(format!("`{{{name}}}` in `{path}` isn't a variable name (letters, digits and `_`, with dots between parts; no spaces)"));
        }
        out.push(Piece::Var(name));
        rest = &after[end + 1..];
    }
    if !rest.is_empty() {
        out.push(Piece::Lit(rest));
    }
    let vars = out.iter().filter(|p| matches!(p, Piece::Var(_))).count();
    if vars > MAX_PATTERN_VARS {
        return Err(format!("`{path}` has {vars} `{{variable}}` parts; at most {MAX_PATTERN_VARS}"));
    }
    if vars > 0 {
        let name = path.rsplit('/').next().unwrap_or(path);
        let ext = name.rsplit_once('.').map(|(_, e)| e);
        if ext.is_none_or(|e| e.is_empty() || e.contains(['{', '}'])) {
            return Err(format!("`{path}`: a name with a `{{variable}}` still needs a written-out extension, like `.png`"));
        }
        let ext = ext.unwrap_or("").to_ascii_lowercase();
        if !PICTURE_EXTENSIONS.contains(&ext.as_str()) {
            return Err(format!("`{path}`: a name with a `{{variable}}` must end in a picture extension (.png, .jpg, .jpeg or .webp), not `.{ext}`"));
        }
    }
    Ok(out)
}

/// The variables a name reads (none for a plain path; a malformed name reads none).
pub fn pattern_vars(path: &str) -> Vec<&str> {
    match pieces(path) {
        Ok(p) => p
            .into_iter()
            .filter_map(|p| match p {
                Piece::Var(v) => Some(v),
                Piece::Lit(_) => None,
            })
            .collect(),
        Err(_) => Vec::new(),
    }
}

/// `Some(why)` when `value` can't be put into a file name. A safe fragment is 1 to
/// [`MAX_FRAGMENT`] characters: letters (any alphabet), digits, `_`, `-`, and
/// spaces and dots between other characters (no leading or trailing space or dot,
/// no `..`), and not one of the names Windows keeps for devices (`con`, `nul`, `com1`...).
/// So no `/`, `\`, `:` or control character can get in.
pub fn unsafe_reason(value: &str) -> Option<&'static str> {
    let n = value.chars().count();
    if n == 0 {
        return Some("empty");
    }
    if n > MAX_FRAGMENT {
        return Some("longer than 64 characters");
    }
    if value.chars().any(char::is_control) {
        return Some("control character");
    }
    if !value.chars().all(|c| c.is_alphanumeric() || matches!(c, '_' | '-' | ' ' | '.')) {
        return Some("not just letters, digits, _ - . and space");
    }
    if value.starts_with(['.', ' ']) || value.ends_with(['.', ' ']) {
        return Some("starts or ends with a dot or a space");
    }
    if value.contains("..") {
        return Some("has two dots in a row");
    }
    if is_device_name(value) {
        return Some("a name Windows keeps for devices");
    }
    None
}

/// Whether Windows treats `part` (one file or folder name) as a device: the stem (up
/// to the first dot, trailing spaces ignored) is `con`, `prn`, `aux`, `nul`, `com1-9` or `lpt1-9`.
pub(crate) fn is_device_name(part: &str) -> bool {
    let stem = part.split('.').next().unwrap_or("").trim_end();
    if !(stem.len() == 3 || stem.len() == 4) {
        return false;
    }
    let stem = stem.to_ascii_lowercase();
    matches!(stem.as_str(), "con" | "prn" | "aux" | "nul")
        || ["com", "lpt"].iter().any(|p| stem.strip_prefix(p).is_some_and(|d| matches!(d.as_bytes(), [b'1'..=b'9'])))
}

/// Build the file name a pattern stands for. `Err` says why not: a variable that
/// holds no text, or text that isn't a safe fragment (see [`unsafe_reason`]).
/// A plain path comes back unchanged. The result is always inside the game folder.
pub fn resolve(pattern: &str, vars: &Vars) -> Result<String, String> {
    let parts = pieces(pattern)?;
    let mut out = String::with_capacity(pattern.len() + 16);
    for part in parts {
        match part {
            Piece::Lit(s) => out.push_str(s),
            Piece::Var(name) => match vars.get(name) {
                Some(Value::Str(v)) => match unsafe_reason(v) {
                    None => out.push_str(v),
                    Some(why) => return Err(format!("{{{name}}} in {pattern} is not a safe file name ({why})")),
                },
                _ => return Err(format!("{{{name}}} in {pattern} is not set to a text")),
            },
        }
    }
    // The name as a whole, not only each value: `c{a}.png` with `a = on` is `con.png`.
    if out.split('/').any(is_device_name) {
        return Err(format!("{pattern} would make `{out}`, a name Windows keeps for devices"));
    }
    if is_inside(&out) {
        Ok(normal_path(&out))
    } else {
        debug_assert!(false, "`{out}` escaped the game folder");
        Err(format!("`{pattern}` doesn't make a path inside the game folder"))
    }
}

/// Whether `name` (one path part, no `/`) fits the pieces of one path part: each
/// variable stands for one safe fragment. A position that failed once is not tried
/// again, and a fragment is at most [`MAX_FRAGMENT`] characters, so a long name that
/// doesn't fit costs little however many variables the pattern has.
fn name_matches(pieces: &[Piece<'_>], name: &str) -> bool {
    let mut failed = std::collections::HashSet::new();
    matches_from(pieces, 0, name, 0, &mut failed)
}

fn matches_from(pieces: &[Piece<'_>], i: usize, name: &str, at: usize, failed: &mut std::collections::HashSet<(usize, usize)>) -> bool {
    if failed.contains(&(i, at)) {
        return false;
    }
    let rest = &name[at..];
    let ok = match pieces.get(i) {
        None => rest.is_empty(),
        Some(Piece::Lit(l)) => rest.starts_with(l) && matches_from(pieces, i + 1, name, at + l.len(), failed),
        Some(Piece::Var(_)) => {
            // The candidate fragments are the prefixes `unsafe_reason` accepts, found in
            // one pass: a character that is never allowed, a leading dot or space, or `..`
            // rules out every longer prefix too.
            let mut prev = None;
            let mut found = false;
            for (n, (k, c)) in rest.char_indices().enumerate().take(MAX_FRAGMENT) {
                let allowed = c.is_alphanumeric() || matches!(c, '_' | '-' | ' ' | '.');
                if !allowed || (n == 0 && matches!(c, '.' | ' ')) || (prev == Some('.') && c == '.') {
                    break;
                }
                prev = Some(c);
                let end = k + c.len_utf8();
                if matches!(c, '.' | ' ') || is_device_name(&rest[..end]) {
                    continue;
                }
                if matches_from(pieces, i + 1, name, at + end, failed) {
                    found = true;
                    break;
                }
            }
            found
        }
    };
    if !ok {
        failed.insert((i, at));
    }
    ok
}

/// Whether the game-folder path `candidate` is one a pattern can build.
pub fn pattern_matches(pattern: &str, candidate: &str) -> bool {
    let Ok(parts) = pieces(pattern) else { return false };
    // Split the pieces at each `/` in a literal, so folders and file name match one by one.
    let mut segments: Vec<Vec<Piece<'_>>> = vec![Vec::new()];
    for p in parts {
        match p {
            Piece::Var(_) => segments.last_mut().expect("never empty").push(p),
            Piece::Lit(l) => {
                for (i, chunk) in l.split('/').enumerate() {
                    if i > 0 {
                        segments.push(Vec::new());
                    }
                    if !chunk.is_empty() {
                        segments.last_mut().expect("never empty").push(Piece::Lit(chunk));
                    }
                }
            }
        }
    }
    let names: Vec<&str> = candidate.split('/').collect();
    names.len() == segments.len() && segments.iter().zip(names).all(|(s, n)| name_matches(s, n))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vars(pairs: &[(&str, Value)]) -> Vars {
        pairs.iter().map(|(k, v)| (k.to_string(), v.clone())).collect()
    }

    fn text(s: &str) -> Value {
        Value::Str(s.to_string())
    }

    #[test]
    fn splits_literals_and_variables() {
        assert_eq!(pieces("a/b.png").unwrap(), [Piece::Lit("a/b.png")]);
        assert_eq!(
            pieces("assets/{place}_{hero.mood}.png").unwrap(),
            [Piece::Lit("assets/"), Piece::Var("place"), Piece::Lit("_"), Piece::Var("hero.mood"), Piece::Lit(".png")]
        );
        assert!(!is_pattern("a/b.png") && is_pattern("a/{x}.png") && is_pattern("a/}.png"));
        assert_eq!(pattern_vars("a/{x}/{y.z}.png"), ["x", "y.z"]);
        assert!(pattern_vars("a/b.png").is_empty());
    }

    #[test]
    fn malformed_names_are_errors() {
        for bad in [
            "a/{x.png",
            "a/x}.png",
            "a/{}.png",
            "a/{ x }.png",
            "a/{x y}.png",
            "a/{x{y}}.png",
            "a/{1x}.png",
            "a/{x.}.png",
            "a/{x}",
            "a/{x}.{y}",
            "a/{x}.",
            "{a}{b}{c}{d}{e}.png",
        ] {
            assert!(pieces(bad).is_err(), "{bad} should be refused");
        }
        assert!(pieces("{a}{b}{c}{d}.png").is_ok());
        assert!(pieces("a/{x}.tar.png").is_ok());
        for ext in ["png", "jpg", "jpeg", "webp", "PNG", "Jpg", "WebP"] {
            assert!(pieces(&format!("a/{{x}}.{ext}")).is_ok(), "{ext}");
        }
        for ext in ["txt", "exe", "svg", "gif", "x", "bmp", "ogg"] {
            let e = pieces(&format!("a/{{x}}.{ext}")).unwrap_err();
            assert!(e.contains(".png, .jpg, .jpeg or .webp"), "{ext}: {e}");
        }
        assert!(pieces("a/readme.txt").unwrap().len() == 1, "no variable, no rule");
    }

    #[test]
    fn safe_fragments() {
        for ok in ["rooftop", "a", "Night 2", "a.b", "a-b_c", "héros", "夜空", "Привет", "x1", &"a".repeat(64)] {
            assert_eq!(unsafe_reason(ok), None, "{ok}");
        }
        for bad in [
            "", "..", ".", "../x", "a/b", "a\\b", "C:x", "a:b", ".hidden", "a.", " a", "a ", "a..b", "a\nb", "a\0b", "a\u{7f}b",
            "a\u{202e}b", "a*b", "a?b", "a{b}", "a\"b", "con", "NUL", "Com1", "lpt9", "aux.x", "con .x", &"a".repeat(65), &"é".repeat(65),
        ] {
            assert!(unsafe_reason(bad).is_some(), "{bad:?} should be unsafe");
        }
        assert_eq!(unsafe_reason(&"é".repeat(64)), None, "the limit counts characters");
        assert_eq!(unsafe_reason("com10"), None);
        assert_eq!(unsafe_reason("icon"), None);
    }

    #[test]
    fn resolves_text_variables_only() {
        let v = vars(&[("place", text("rooftop")), ("hero.mood", text("happy")), ("n", Value::Int(3)), ("bad", text("../x"))]);
        assert_eq!(resolve("a/b.png", &v).unwrap(), "a/b.png");
        assert_eq!(resolve("assets/{place}.png", &v).unwrap(), "assets/rooftop.png");
        assert_eq!(resolve("m_{place}_{hero.mood}.png", &v).unwrap(), "m_rooftop_happy.png");
        assert!(resolve("a/{n}.png", &v).unwrap_err().contains("not set to a text"));
        assert!(resolve("a/{unset}.png", &v).unwrap_err().contains("not set to a text"));
        let e = resolve("a/{bad}.png", &v).unwrap_err();
        assert!(e.contains("{bad}") && e.contains("a/{bad}.png") && !e.contains("../x"), "{e}");
        assert!(resolve("a/{x.png", &v).is_err());
    }

    #[test]
    fn every_resolved_name_is_inside_the_game_folder() {
        for value in ["a", "a.b", "a b", "é", "x-y_z", "1", "a.b.c"] {
            let v = vars(&[("p", text(value))]);
            for pattern in ["{p}.png", "a/{p}.png", "{p}/{p}.png", "./{p}.png", "x{p}y.png"] {
                let r = resolve(pattern, &v).unwrap();
                assert!(is_inside(&r), "{r}");
            }
        }
    }

    #[test]
    fn globs_match_safe_fragments_inside_one_folder() {
        let m = pattern_matches;
        assert!(m("assets/places/{place}.png", "assets/places/rooftop.png"));
        assert!(m("assets/places/{place}.png", "assets/places/Night 2.png"));
        assert!(!m("assets/places/{place}.png", "assets/places/.png"), "a variable is never empty");
        assert!(!m("assets/places/{place}.png", "assets/places/a/b.png"), "never across folders");
        assert!(!m("assets/places/{place}.png", "assets/other/a.png"));
        assert!(!m("assets/places/{place}.png", "assets/places/a.jpg"));
        assert!(!m("assets/places/{place}.png", "assets/places/a..b.png"), "only fragments a variable could hold");
        assert!(m("m_{pose}.png", "m_happy.png") && !m("m_{pose}.png", "x_happy.png"));
        assert!(m("{a}_{b}.png", "x_y.png") && m("{a}{b}.png", "xy.png"));
        assert!(m("art/{who}/face.png", "art/mika/face.png") && !m("art/{who}/face.png", "art/mika/x/face.png"));
        assert!(m("a/b.png", "a/b.png") && !m("a/b.png", "a/c.png"));
        assert!(m("a/{x}.png", "a/é.png"));
    }

    #[test]
    fn a_device_name_made_of_pieces_is_refused() {
        let v = vars(&[("a", text("on")), ("b", text("n")), ("c", text("co")), ("d", text("1")), ("ok", text("x"))]);
        for pattern in ["assets/c{a}.png", "assets/{c}{b}.png", "{c}{b}.png", "assets/lpt{d}.png", "com{d}/{ok}.png", "assets/{c}{b}.x.png"] {
            let e = resolve(pattern, &v).unwrap_err();
            assert!(e.contains("devices"), "{pattern}: {e}");
        }
        assert_eq!(resolve("assets/c{ok}.png", &v).unwrap(), "assets/cx.png");
        assert_eq!(resolve("assets/icon{ok}.png", &v).unwrap(), "assets/iconx.png");
        assert_eq!(resolve("assets/con_{ok}.png", &v).unwrap(), "assets/con_x.png", "a stem that merely starts with con");
        assert!(resolve("assets/con.png", &v).is_err(), "a plain device name is refused too");
    }

    #[test]
    fn matching_a_long_name_stays_fast() {
        let started = std::time::Instant::now();
        let miss = format!("assets/{}!.png", "a".repeat(150));
        let too_long = format!("assets/{}.png", "a".repeat(300));
        for pattern in ["assets/{a}{b}{c}{d}.png", "assets/{a}_{b}_{c}_{d}.png", "assets/{a}{b}{c}{d}.jpg"] {
            assert!(!pattern_matches(pattern, &miss) && !pattern_matches(pattern, &too_long));
        }
        // 150 characters do fit in four fragments of at most 64
        assert!(pattern_matches("assets/{a}{b}{c}{d}.png", &format!("assets/{}.png", "a".repeat(150))));
        assert!(started.elapsed().as_millis() < 50, "{:?}", started.elapsed());
        // a fragment is at most 64 characters, even for a single variable
        assert!(pattern_matches("assets/{a}.png", &format!("assets/{}.png", "a".repeat(64))));
        assert!(!pattern_matches("assets/{a}.png", &format!("assets/{}.png", "a".repeat(65))));
        assert!(pattern_matches("assets/{a}{b}.png", &format!("assets/{}.png", "a".repeat(128))));
        // the one-pass scan accepts exactly the fragments `unsafe_reason` accepts
        for v in ["rooftop", "a", "Night 2", "a.b", "é", "x1", ".h", "a.", " a", "a ", "a..b", "a\nb", "con", "NUL", "Com1", "aux.x", "com10", "icon", "a*b"] {
            assert_eq!(pattern_matches("{a}.png", &format!("{v}.png")), unsafe_reason(v).is_none(), "{v:?}");
        }
    }
}
