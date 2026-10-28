//! Presenting the story in another language.
//!
//! The runtime itself is language-free: [`State`], saves and rollback hold the script's own text.
//! A frontend that has a translation ([`Catalog`]) passes what it is about to show through here:
//! the text is looked up by its script source (before `{name}` values go in), then interpolated
//! with the same variables, so `{nw}`, `{p}` and every other tag keep working.

use quillstone_script::{HudKind, Op, Catalog, Vars};

use crate::{fill_name, interpolate, speaker_name, visible_options, Cmd, ItemInfo, Logged, Session};

/// The item with its name and description translated.
pub fn localize_item(info: &mut ItemInfo, cat: &Catalog) {
    info.name = cat.text(None, &info.name).to_string();
    info.text = cat.text(None, &info.text).to_string();
}

impl Session<'_> {
    /// Where the line on screen came from: the op and the variables as they were when it was said.
    /// Under choices the line is an earlier one, found in the rollback history (a loaded save may
    /// have none left; then it stays in the script's language).
    fn line_source(&self) -> Option<(usize, &Vars)> {
        if matches!(self.state.pending, Some(Cmd::Say { .. })) {
            return Some((self.state.pc, &self.state.vars));
        }
        let line = self.state.line.as_ref()?;
        self.history
            .iter()
            .rev()
            .map(|(s, _)| s)
            .find(|s| matches!(s.pending, Some(Cmd::Say { .. })) && s.line.as_ref() == Some(line))
            .map(|s| (s.pc, &s.vars))
    }

    fn say_in(&self, pc: usize, vars: &Vars, speaker: &Option<String>, cat: &Catalog) -> Option<(Option<String>, String)> {
        let Some(Op::Say { speaker: s, text, .. }) = self.program.ops.get(pc) else { return None };
        if &speaker_name(self.program, s, vars) != speaker {
            return None; // not the line this was made from (the script changed): leave it
        }
        // The catalog is keyed on the raw name template; the translated template is then filled in.
        let name = s.as_deref().map(|n| fill_name(cat.text(None, n), vars));
        Some((name, interpolate(cat.text(s.as_deref(), text), vars)))
    }

    /// `cmds` (from `step`, `input`, a rollback or [`Session::redraw`]-style lists) with the texts
    /// the player reads in `cat`'s language. Commands without text pass through untouched, and a
    /// text with no translation stays as it is.
    pub fn localize(&self, cmds: Vec<Cmd>, cat: &Catalog) -> Vec<Cmd> {
        let last = cmds.len().saturating_sub(1);
        let vars = &self.state.vars;
        cmds.into_iter()
            .enumerate()
            .map(|(i, cmd)| match cmd {
                Cmd::Say { speaker, color, text } => {
                    let from = if i == last && matches!(self.state.pending, Some(Cmd::Say { .. })) {
                        Some((self.state.pc, vars))
                    } else {
                        self.line_source()
                    };
                    match from.and_then(|(pc, v)| self.say_in(pc, v, &speaker, cat)) {
                        Some((speaker, text)) => Cmd::Say { speaker, color, text },
                        None => Cmd::Say { speaker, color, text },
                    }
                }
                Cmd::ShowChoices { options } if i == last => match self.program.ops.get(self.state.pc) {
                    Some(Op::Choice { options: src, .. }) => {
                        let visible = visible_options(src, vars);
                        if visible.len() == options.len() && src.iter().all(|o| o.area.is_none()) {
                            let texts = visible.iter().map(|&k| interpolate(cat.text(None, &src[k].text), vars));
                            Cmd::ShowChoices { options: texts.collect() }
                        } else {
                            Cmd::ShowChoices { options }
                        }
                    }
                    _ => Cmd::ShowChoices { options },
                },
                Cmd::ShowHotspots { mut spots } if i == last => {
                    if let Some(Op::Choice { options: src, .. }) = self.program.ops.get(self.state.pc) {
                        let visible = visible_options(src, vars);
                        if visible.len() == spots.len() {
                            for (spot, &k) in spots.iter_mut().zip(&visible) {
                                spot.label = interpolate(cat.text(None, &src[k].text), vars);
                            }
                        }
                    }
                    Cmd::ShowHotspots { spots }
                }
                Cmd::ShowShop { pay, money, mut items } => {
                    items.iter_mut().for_each(|s| localize_item(&mut s.item, cat));
                    Cmd::ShowShop { pay, money, items }
                }
                Cmd::AskInput { prompt, default, max } => match self.program.ops.get(self.state.pc) {
                    Some(Op::Input { prompt: src, .. }) if i == last => {
                        Cmd::AskInput { prompt: interpolate(cat.text(None, src), vars), default, max }
                    }
                    _ => Cmd::AskInput { prompt, default, max },
                },
                Cmd::HudShow { mut elem, fade } => {
                    if let HudKind::Text { text, .. } = &mut elem.kind {
                        *text = cat.text(None, text).to_string();
                    }
                    Cmd::HudShow { elem, fade }
                }
                other => other,
            })
            .collect()
    }

    /// What to show again after the language changed: the HUD, the line, and the screen the story
    /// waits at, all in `cat`'s language. Nothing else on the stage is touched.
    pub fn relocalized(&self, cat: &Catalog) -> Vec<Cmd> {
        let mut cmds: Vec<Cmd> = self.state.hud.iter().map(|h| Cmd::HudShow { elem: Box::new(h.clone()), fade: 0.0 }).collect();
        if !matches!(self.state.pending, Some(Cmd::Say { .. })) {
            if let Some(line) = &self.state.line {
                cmds.push(Cmd::Say { speaker: line.speaker.clone(), color: line.color, text: line.text.clone() });
            }
        }
        cmds.extend(self.state.pending.clone());
        self.localize(cmds, cat)
    }

    /// Like [`Session::backlog`], in `cat`'s language: each line and choice is looked up again from
    /// the script text it came from, so the whole log follows a language change.
    pub fn backlog_with(&self, cat: &Catalog) -> Vec<Logged> {
        let mut out = Vec::new();
        let mut log = |state: &crate::State, input: Option<&crate::Input>| match (&state.pending, input) {
            (Some(Cmd::Say { .. }), _) => {
                let Some(line) = state.line.clone() else { return };
                let said = self.say_in(state.pc, &state.vars, &line.speaker, cat);
                out.push(Logged::Line(match said {
                    Some((speaker, text)) => crate::Line { speaker, color: line.color, text },
                    None => line,
                }));
            }
            (Some(Cmd::ShowChoices { options }), Some(crate::Input::Choose(i))) => {
                let Some(shown) = options.get(*i) else { return };
                let src = match self.program.ops.get(state.pc) {
                    Some(Op::Choice { options: src, .. }) => {
                        let visible = visible_options(src, &state.vars);
                        visible.get(*i).map(|&k| interpolate(cat.text(None, &src[k].text), &state.vars))
                    }
                    _ => None,
                };
                out.push(Logged::Choice(src.unwrap_or_else(|| shown.clone())));
            }
            _ => {}
        };
        for (state, input) in &self.history {
            log(state, Some(input));
        }
        log(&self.state, None);
        out
    }
}
