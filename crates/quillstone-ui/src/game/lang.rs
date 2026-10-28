//! Languages: the Settings row, and presenting the story's texts through a translation.
//!
//! The story, saves and rollback never hold translated text. The texts are looked up as they
//! are shown ([`Session::localize`]), so changing the language takes effect at once and nothing
//! has to be rewound.

use std::collections::BTreeMap;

use quillstone_runtime::{Cmd, Logged};
use quillstone_script::{Catalog, Language};

use super::Game;

impl<'p> Game<'p> {
    /// The translations by language code (`lang/<code>.txt`, read by the frontend). A language
    /// with no entry here shows the script's own text.
    pub fn set_translations(&mut self, translations: BTreeMap<String, Catalog>) {
        self.translations = translations;
        self.language_changed();
    }

    /// The languages the game offers: the script's own first. Empty for a one-language game.
    pub fn languages(&self) -> &[Language] {
        &self.program.languages
    }

    /// The language in use, as a code from [`Game::languages`]; `None` is the script's own.
    pub fn language(&self) -> Option<&str> {
        let chosen = self.settings.language.as_deref()?;
        self.program.languages.iter().skip(1).find(|l| l.code == chosen).map(|l| l.code.as_str())
    }

    /// The name of the language in use, as the Settings row shows it.
    pub(super) fn language_name(&self) -> &str {
        let code = self.language();
        let current = self.program.languages.iter().find(|l| Some(l.code.as_str()) == code);
        current.or(self.program.languages.first()).map_or("", |l| l.name.as_str())
    }

    /// The window title in the language in use.
    pub fn title(&self) -> String {
        let title = self.program.title.as_deref().unwrap_or("Quillstone");
        plain(self.tr(title))
    }

    pub(super) fn has_language_row(&self) -> bool {
        self.program.languages.len() > 1
    }

    /// The translation of the language in use, if there is one.
    pub(super) fn catalog(&self) -> Option<&Catalog> {
        self.translations.get(self.language()?)
    }

    /// `text` as the screens show it: translated when the language in use has it.
    pub(super) fn tr<'a>(&'a self, text: &'a str) -> &'a str {
        self.catalog().map_or(text, |c| c.text(None, text))
    }

    /// `cmds` with their texts in the language in use (`None`: nothing to translate).
    pub(super) fn localized(&self, cmds: &[Cmd]) -> Option<Vec<Cmd>> {
        let cat = self.catalog()?;
        let texty = |c: &Cmd| {
            matches!(
                c,
                Cmd::Say { .. } | Cmd::ShowChoices { .. } | Cmd::ShowHotspots { .. } | Cmd::ShowShop { .. } | Cmd::AskInput { .. } | Cmd::HudShow { .. }
            )
        };
        cmds.iter().any(texty).then(|| self.session.localize(cmds.to_vec(), cat))
    }

    /// The command the story waits at, in the language in use (for the screens drawn from it).
    pub(super) fn pending_shown(&self) -> Option<Cmd> {
        let pending = self.session.state().pending.clone()?;
        match self.catalog() {
            Some(cat) => self.session.localize(vec![pending], cat).pop(),
            None => Some(pending),
        }
    }

    /// The backlog in the language in use.
    pub(super) fn backlog_now(&self) -> Vec<Logged> {
        match self.catalog() {
            Some(cat) => self.session.backlog_with(cat),
            None => self.session.backlog(),
        }
    }

    /// The Language row: next language (wrapping), or the previous with `back`. Applies at once.
    pub(super) fn cycle_language(&mut self, back: bool) {
        let n = self.program.languages.len();
        if n < 2 {
            return;
        }
        let code = self.language();
        let at = self.program.languages.iter().position(|l| Some(l.code.as_str()) == code).unwrap_or(0);
        let next = (at + if back { n - 1 } else { 1 }) % n;
        self.settings.language = (next > 0).then(|| self.program.languages[next].code.clone());
        self.language_changed();
    }

    /// Show what is on screen again in the language now in use: the line, the choices and the
    /// HUD (the typed-out text appears whole). The stage, sounds and timers are not touched.
    fn language_changed(&mut self) {
        if !self.in_story {
            return;
        }
        let empty = Catalog::default();
        let cat = self.catalog().unwrap_or(&empty);
        let mut cmds = self.session.relocalized(cat);
        cmds.retain(|c| matches!(c, Cmd::Say { .. } | Cmd::ShowChoices { .. } | Cmd::ShowHotspots { .. } | Cmd::HudShow { .. }));
        self.view.apply(&cmds);
        if let Some(d) = &mut self.view.dialogue {
            d.reveal_all();
        }
        if !self.view.hud().is_empty() {
            self.view.set_vars(&self.session.state().vars);
        }
    }
}

fn plain(text: &str) -> String {
    quillstone_script::markup::plain(text)
}
