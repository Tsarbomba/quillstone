//! The engine's own texts (buttons, labels, hints, messages) and how they are translated.
//!
//! Every text the player sees that the story didn't write goes through [`Tr`], which gives the
//! translation of the language in use (from the game's `lang/<code>.txt`, under the same `>` text
//! as here) or the English original. [`STRINGS`] is the one list of them: `quillstone lang export`
//! adds them to a translation file, and in debug builds a text not in the list fails at once.
//! A text with a changing part has a `{name}` in it, filled in after the lookup ([`Tr::f`]).

use std::fmt::Display;

use quillstone_script::{Catalog, Source};

/// `(place, English text)`: the place says where the translator sees it. A text is listed once.
pub const STRINGS: &[(&str, &str)] = &[
    ("game menu", "Save"),
    ("game menu", "Load"),
    ("game menu", "Settings"),
    ("game menu", "History"),
    ("game menu", "Main menu"),
    ("game menu", "Quit"),
    ("game menu", "Back"),
    ("main menu", "Start"),
    ("main menu", "Continue"),
    ("main menu", "Extras"),
    ("quick menu", "Skip"),
    ("quick menu", "Auto"),
    ("quick menu", "Q.Save"),
    ("quick menu", "Q.Load"),
    ("quick menu", "Items"),
    ("quick menu", "Journal"),
    ("quick menu", "AUTO"),
    ("quick menu", "SKIP »"),
    ("settings screen", "Text speed"),
    ("settings screen", "Text size"),
    ("settings screen", "Auto-forward delay"),
    ("settings screen", "Skip unread text"),
    ("settings screen", "Music volume"),
    ("settings screen", "Sound volume"),
    ("settings screen", "Voice volume"),
    ("settings screen", "Fullscreen"),
    ("settings screen", "Quick menu"),
    ("settings screen", "Language"),
    ("settings screen", "Instant"),
    ("settings screen", "{n} chars/s"),
    ("settings screen", "On"),
    ("settings screen", "Off"),
    ("settings screen", "Game default"),
    ("settings screen", "Always"),
    ("settings screen", "On hover"),
    ("settings screen", "Hidden"),
    ("settings screen", "Esc to close  ·  Up/Down to pick  ·  Left/Right to adjust"),
    ("history screen", "Wheel / Page Up / Page Down to scroll  ·  Esc to close"),
    ("history screen", "Nothing yet."),
    ("save screen", "Quick"),
    ("save screen", "Empty"),
    ("save screen", "Unreadable save"),
    ("save screen", "This save doesn't fit the current script"),
    ("question", "Yes"),
    ("question", "No"),
    ("question", "Overwrite this save?"),
    ("question", "Load this save?\nUnsaved progress will be lost."),
    ("question", "Load the quick save?\nUnsaved progress will be lost."),
    ("question", "Return to the main menu?\nUnsaved progress will be lost."),
    ("question", "Leave this replay and go back to Extras?"),
    ("question", "Quit the game?"),
    ("extras screen", "Gallery"),
    ("extras screen", "Music room"),
    ("extras screen", "Scenes"),
    ("extras screen", "Page {n} of {pages}"),
    ("extras screen", "Click or Esc to go back  ·  Left/Right for the others"),
    ("inventory and shop", "Shop"),
    ("inventory and shop", "Owned"),
    ("inventory and shop", "Use"),
    ("inventory and shop", "Close"),
    ("inventory and shop", "Leave"),
    ("inventory and shop", "Done"),
    ("inventory and shop", "Nothing here yet."),
    ("inventory and shop", "Price: {n}"),
    ("inventory and shop", "{price} {label} (owned: {n})"),
    ("text input", "OK"),
    ("message", "Loaded"),
    ("message", "Quick saved"),
    ("message", "No quick save yet"),
    ("message", "Nothing here has been read yet"),
    ("message", "Not unlocked yet"),
    ("message", "Not available during a replay"),
    ("message", "Can't use that now"),
    ("message", "Already owned"),
    ("message", "Not enough {label}"),
    ("message", "Bought {name}"),
    ("message", "Could not save: {error}"),
    ("message", "Could not load: {error}"),
    ("message", "Could not save which lines were read: {error}"),
    ("message", "Could not save what was unlocked: {error}"),
];

/// The list as translatable texts, for `quillstone lang export`.
pub fn sources() -> Vec<Source> {
    let source = |&(place, text): &(&str, &str)| Source { text: text.into(), place: place.into(), speakers: Default::default() };
    STRINGS.iter().map(source).collect()
}

/// Looks the engine's texts up in a translation; `Tr::default()` is English.
#[derive(Debug, Clone, Copy, Default)]
pub struct Tr<'a>(Option<&'a Catalog>);

impl<'a> Tr<'a> {
    pub fn new(catalog: Option<&'a Catalog>) -> Self {
        Tr(catalog)
    }

    /// The translation of the listed English text `s`, or `s` itself (also while it is not translated yet).
    pub fn t<'s>(self, s: &'s str) -> &'s str
    where
        'a: 's,
    {
        debug_assert!(STRINGS.iter().any(|&(_, t)| t == s), "`{s}` is not in texts::STRINGS");
        self.0.map_or(s, |c| c.text(None, s))
    }

    /// [`Tr::t`] with each `{name}` in the result replaced by its value.
    pub fn f(self, s: &str, args: &[(&str, &dyn Display)]) -> String {
        args.iter().fold(self.t(s).to_string(), |out, (name, value)| out.replace(&format!("{{{name}}}"), &value.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn swedish() -> Catalog {
        Catalog::parse("> Save\n= Spara\n\n> Load\n=\n\n> Page {n} of {pages}\n= Sida {n} av {pages}\n").unwrap()
    }

    #[test]
    fn a_text_is_listed_once() {
        let mut seen = std::collections::HashSet::new();
        for &(_, text) in STRINGS {
            assert!(seen.insert(text), "`{text}` is listed twice");
        }
        assert_eq!(sources().len(), STRINGS.len());
    }

    #[test]
    fn every_listed_text_keeps_its_placeholders_in_a_translation_that_copies_it() {
        for &(_, text) in STRINGS {
            assert_eq!(quillstone_script::check_translation(text, text), Ok(()), "{text}");
        }
    }

    #[test]
    fn the_translation_is_used_and_the_original_when_there_is_none_or_it_is_empty() {
        let cat = swedish();
        let tr = Tr::new(Some(&cat));
        assert_eq!(tr.t("Save"), "Spara");
        assert_eq!(tr.t("Load"), "Load", "an empty translation means the original");
        assert_eq!(tr.t("Quit"), "Quit", "not in the file");
        assert_eq!(Tr::default().t("Save"), "Save");
    }

    #[test]
    fn placeholders_are_filled_in_after_the_lookup() {
        let cat = swedish();
        let args: &[(&str, &dyn Display)] = &[("n", &2), ("pages", &5)];
        assert_eq!(Tr::new(Some(&cat)).f("Page {n} of {pages}", args), "Sida 2 av 5");
        assert_eq!(Tr::default().f("Page {n} of {pages}", args), "Page 2 of 5");
    }

    #[test]
    #[cfg(debug_assertions)]
    #[should_panic(expected = "is not in texts::STRINGS")]
    fn a_text_missing_from_the_list_fails_in_debug_builds() {
        Tr::default().t("Not a listed text");
    }
}
