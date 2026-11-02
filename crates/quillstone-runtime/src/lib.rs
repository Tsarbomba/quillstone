//! Quillstone runtime: a pure state machine over a compiled script.
//!
//! `step(&mut State, &Program, Option<Input>) -> Vec<Cmd>` is the whole engine.
//! The renderer and audio backends only consume [`Cmd`]s; they never touch the
//! script. Everything that is "on screen" (background, sprites, music, the line
//! being shown, the call stack, variables) lives in [`State`], which is `Clone`:
//! one snapshot per player action gives rollback and save slots (see [`Session`]),
//! and [`State::redraw`] rebuilds the whole scene from a snapshot.

use std::collections::VecDeque;

use quillstone_script::{roll, Area, AssignOp, Axis, ChoiceOption, Cond, Ease, Effect, Expr, Fit, HudAction, HudElem, Op, Particles, Program, ShopEntry, Tint, TimingBar, Hit, KeyPress, Value, Vars, Wipe};

mod explore;
#[cfg(test)]
mod hardening;
#[cfg(test)]
mod test_util;
#[cfg(test)]
mod review3_tests;
#[cfg(test)]
mod anim_tests;
#[cfg(test)]
mod dynpic_tests;
#[cfg(test)]
mod layers_tests;
#[cfg(test)]
mod hud_tests;
#[cfg(test)]
mod include_tests;
#[cfg(test)]
mod particles_tests;
#[cfg(test)]
mod tint_tests;
#[cfg(test)]
mod timed_tests;
#[cfg(test)]
mod press_tests;
#[cfg(test)]
mod timing_tests;
#[cfg(test)]
mod locked_tests;
#[cfg(test)]
mod use_tests;
#[cfg(test)]
mod hud_button_tests;
mod localize;
#[cfg(test)]
mod localize_tests;
#[cfg(test)]
mod name_tests;
mod dev;
#[cfg(test)]
mod dev_tests;
mod persist;
mod save;
pub use dev::{start_flags, StartAt};
pub use explore::{explore, Finding, Report};
pub use quillstone_script::DEFAULT_SEED;

/// A fresh game seed from a host's `seed` (which is advanced for the next game).
pub fn next_seed(seed: &mut u64) -> u64 {
    quillstone_script::splitmix64(seed)
}
pub use localize::localize_item;
pub use persist::{is_persistent_file, read_persistent, write_persistent, REPLAY, UNLOCKED};
pub use save::{line_id, q as quote, tokens, voice_name, MAX_LIST_ITEMS, MAX_SAVE_BYTES, SAVE_HISTORY};

/// Something the frontend should do.
#[derive(Debug, Clone, PartialEq)]
pub enum Cmd {
    /// Drop the whole scene (background, sprites, images) before a redraw.
    ClearScene,
    /// Show a line and wait for [`Input::Advance`]. `speaker: None` is narration.
    /// `color` is the speaker's name plate colour, if the character declared one.
    Say { speaker: Option<String>, color: Option<[f32; 3]>, text: String },
    /// An inline image shown with the next waiting line.
    ShowImage { src: String, alt: String },
    /// `src` may be a video file (a looping video background). With a `wipe`,
    /// `fade` is how long the wipe takes.
    SetBackground { src: String, fade: f32, wipe: Option<Wipe> },
    /// Show a sprite or change its pose. `x` is 0..1 across the screen; `y` is
    /// where its bottom edge is, as a fraction of the screen height (1 is the
    /// bottom of the screen); `z` orders sprites (higher is in front). `flip`
    /// mirrors the picture; `xscale` and `yscale` stretch it along one axis (on top of `scale`).
    /// A layered sprite has an empty `src` and every layer, back to front, as (layer id,
    /// picture): a file, `anim:<id>`, or empty for a hidden layer. Plain sprites have none.
    ShowSprite {
        id: String,
        src: String,
        layers: Vec<(String, String)>,
        x: f32,
        y: f32,
        z: i32,
        scale: f32,
        flip: bool,
        xscale: f32,
        yscale: f32,
        fade: f32,
    },
    HideSprite { id: String, fade: f32 },
    /// Animate a sprite to position `x`, `y` and sizes `scale`, `xscale`, `yscale`
    /// over `time` seconds. `flip` is what the sprite is from the start of the move.
    MoveSprite { id: String, x: f32, y: f32, scale: f32, flip: bool, xscale: f32, yscale: f32, time: f32, ease: Ease },
    /// A looping move: glide to these values over `time` seconds, then back to the
    /// values the sprite rests at (its last `ShowSprite`/`MoveSprite`) and forth, for
    /// ever. Any `MoveSprite` or a `ShowSprite` that changes where the sprite rests ends it.
    LoopSprite { id: String, x: f32, y: f32, scale: f32, flip: bool, xscale: f32, yscale: f32, time: f32, ease: Ease },
    /// Shake the stage for `time` seconds by up to `strength` pixels along `axis`.
    Shake { time: f32, strength: f32, axis: Axis },
    /// A full-screen flash of `color` (0..1 RGB) that fades out over `time` seconds.
    /// Like a shake it is a moment: not part of [`State`], never replayed.
    Flash { time: f32, color: [f32; 3] },
    /// A passing message in a corner of the screen for `time` seconds. `text` has the `{var}` values
    /// filled in; `source` is the script's own text, which a translation is looked up by.
    Notify { text: String, time: f32, source: String },
    /// Fade the stage to `color` over `time` seconds and keep it covered (0: at
    /// once; a redraw sends it that way). Part of the scene: see [`State::cover`].
    /// The story then waits (a [`Cmd::Wait`] of `time` follows), so a click ends the fade.
    FadeOut { time: f32, color: [f32; 3] },
    /// Fade the cover away over `time` seconds. Doesn't wait.
    FadeIn { time: f32 },
    /// Hide or show the dialogue box (textbox, name plate, line, quick menu),
    /// fading over `fade` seconds (0: at once). Part of the scene: see [`State::textbox_hidden`].
    Textbox { hidden: bool, fade: f32 },
    /// Create or replace a HUD element (same id: in place), fading in over `fade`
    /// seconds. Part of the scene: see [`State::hud`].
    HudShow { elem: Box<HudElem>, fade: f32 },
    /// Remove one HUD element, fading out over `fade` seconds (0: at once).
    HudHide { id: String, fade: f32 },
    /// Remove every HUD element.
    HudClear { fade: f32 },
    /// Start or change (`Some`) the ambient particles, or stop them (`None`), easing over
    /// `fade` seconds. Part of the scene: see [`State::particles`].
    Particles { set: Option<Particles>, fade: f32 },
    /// The colour wash over the stage (`::tint`), easing in over `fade` seconds; `None` removes it.
    Tint { set: Option<Tint>, fade: f32 },
    /// Pan/zoom the stage to `camera` over `time` seconds.
    Camera { camera: Camera, time: f32, ease: Ease },
    /// With an `intro`, that file plays once and `src` then loops.
    PlayMusic { src: String, intro: Option<String>, fade: f32, looped: bool, volume: f32 },
    StopMusic { fade: f32 },
    PlaySfx { src: String, volume: f32 },
    /// Start a looping sound effect (part of the scene: see [`State::sfx_loops`]).
    /// One that is already looping isn't restarted; only its volume follows.
    PlaySfxLoop { src: String, fade: f32, volume: f32 },
    /// Stop the loop of `src` and any one-shot of it still playing, or everything
    /// on the sound-effect channel when `src` is `None`.
    StopSfx { src: Option<String>, fade: f32 },
    /// Voice clip for the line being shown; stop it when the player advances.
    PlayVoice { src: String },
    /// Pause; send [`Input::Advance`] when the time is up (or the player skips).
    Wait { seconds: f32 },
    /// Stop until the player advances (`::wait{click}`): the screen stays as it is.
    WaitClick,
    /// Play a clip; send [`Input::Advance`] when it ends or the player skips it.
    PlayVideo { src: String, fit: Fit, looped: bool, skippable: bool },
    /// Show choices and wait for [`Input::Choose`]. Only the options currently
    /// available are listed, plus locked ones (`{show}`); the index refers to this list.
    /// `locked` runs alongside `options`: `Some(reason)` is a greyed option that can't be
    /// picked (the reason is empty when the author gave none), `None` a pickable one.
    ShowChoices { options: Vec<String>, locked: Vec<Option<String>> },
    /// Show clickable regions over the background and wait for [`Input::Choose`]
    /// (from a `::hotspots` block). Like choices: only the regions currently
    /// available are listed, and the index refers to this list.
    ShowHotspots { spots: Vec<Spot> },
    /// Show the inventory screen (from `::inventory`) and wait for [`Input::Advance`]
    /// when the player closes it. What it lists is [`inventory_items`].
    ShowInventory,
    /// Show the journal (from `::journal`) and wait for [`Input::Advance`] when the player
    /// closes it. What it lists is [`journal_items`].
    ShowJournal,
    /// Show a shop and wait. [`Input::Buy`] buys the item at that index of
    /// `items` (the list now on offer) and shows the shop again with the new
    /// money and what is owned; [`Input::Advance`] leaves.
    ShowShop { pay: String, money: i64, items: Vec<ShopItem> },
    /// Ask the player to type a text (from `::input`) and wait for [`Input::Text`].
    /// `prompt` is tagged text with its `{name}` values already put in (see
    /// [`interpolate`]); `max` counts characters. With a `default`, an empty answer
    /// stores it instead; the frontend may show it as a placeholder.
    AskInput { prompt: String, default: Option<String>, max: usize },
    /// Show a timing bar (from `::timing`) and wait for [`Input::Choose`]: 0 = miss, 1 = good, 2 = perfect
    /// (only with a perfect zone). The frontend works out where the marker was from the time since the
    /// bar appeared ([`TimingBar::hit`]) and sends `Choose(0)` when `bar.time` runs out. `prompt` is
    /// tagged text with its `{name}` values already put in; empty: none.
    ShowTiming { prompt: String, bar: TimingBar },
    /// Show a key prompt (from `::press`) and wait for [`Input::Choose`]: 0 = miss, 1 = hit. The frontend
    /// counts the presses of `press.key` and sends `Choose(1)` at the `press.count`-th within `press.time`
    /// seconds of the prompt appearing, `Choose(0)` when the time runs out. `prompt` as for `ShowTiming`.
    ShowPress { prompt: String, press: KeyPress },
    /// The story was stopped by a safety valve (a mistake in the script), and
    /// [`Cmd::Finished`] follows. Says what happened, for the author.
    Stopped { reason: String },
    /// Something in the story could not be done, but it goes on (a picture whose
    /// name couldn't be built from a variable). Says what, for the player and the
    /// author; the frontend shows it once.
    Warning { message: String },
    /// The story reached the start of a label listed in `replays:` (the frontend
    /// may ignore it; [`Session`] records it as unlocked).
    ReplayReached { label: String },
    /// `::unlock`: a gallery or music room entry is unlocked ([`Session`] records it; the frontend may ignore it).
    Unlock { src: String },
    /// `::end_replay` ran. A scene replay ends here; in normal play it means nothing.
    EndReplay,
    Finished,
}

impl Cmd {
    /// A screen the story stops at for the player to answer (not a line, a wait or a video).
    pub fn is_prompt(&self) -> bool {
        matches!(
            self,
            Cmd::ShowChoices { .. } | Cmd::ShowHotspots { .. } | Cmd::ShowShop { .. } | Cmd::AskInput { .. } | Cmd::ShowTiming { .. } | Cmd::ShowPress { .. }
        )
    }
}

/// One clickable region (see [`Cmd::ShowHotspots`]).
#[derive(Debug, Clone, PartialEq)]
pub struct Spot {
    /// Shown as a tooltip while hovered.
    pub label: String,
    /// `x, y, w, h` as fractions (0..1) of the screen, or of the sprite's drawn rect when `sprite` is set.
    pub rect: [f32; 4],
    /// The region follows this sprite; the view works out where it is, and it can't be clicked while the sprite is not shown.
    pub sprite: Option<String>,
    /// Drawn stretched into the region.
    pub image: Option<String>,
    /// No tint, outline or tooltip, and skipped by keyboard focus; still clickable.
    pub hidden: bool,
}

impl Spot {
    /// The timeout of a timed `::hotspots` screen (see [`Area::timeout`]), which has no text of its own.
    pub fn is_timeout(&self) -> bool {
        let area = Area { rect: self.rect, sprite: self.sprite.clone(), image: self.image.clone(), hidden: self.hidden };
        self.label.is_empty() && area.is_timeout()
    }
}

/// An item as the inventory and shop screens show it (see `::item`).
#[derive(Debug, Clone, PartialEq)]
pub struct ItemInfo {
    pub id: String,
    /// The item's name; the id for an item that was never declared.
    pub name: String,
    pub icon: Option<String>,
    pub text: String,
    /// The price the item was declared with, if any.
    pub price: Option<i64>,
    /// How many the player has, for an item declared with `count=` (see [`inventory_items`]).
    pub count: Option<i64>,
    /// The item has a Use action (`use=#label`).
    pub has_use: bool,
}

/// One item on offer in a shop (see [`Cmd::ShowShop`]).
#[derive(Debug, Clone, PartialEq)]
pub struct ShopItem {
    pub item: ItemInfo,
    /// What it costs here (the shop line's own price, or the item's).
    pub price: i64,
    /// Already in the player's list: it can't be bought again.
    pub owned: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Input {
    Advance,
    Choose(usize),
    /// Buy the item at this index of the shop's offered list.
    Buy(usize),
    /// Use the item with this id from the inventory: `::call` its `use=` label from the
    /// line the story waits at (see [`can_use`]).
    Use(String),
    /// Press the HUD button with this element id: `::call` its `call=` label from the line the
    /// story waits at (see [`can_press`]). The label is the shown element's, never the input's.
    Hud(String),
    /// The player's answer to a [`Cmd::AskInput`], as typed: it is cleaned up (see
    /// [`finish_input`]) and stored in the variable. An answer that is empty
    /// after that, with no default to use, is ignored and the story keeps waiting.
    Text(String),
}

/// Typed text without its control characters (newlines, tabs, escapes...).
pub fn filter_typed(s: &str) -> String {
    s.chars().filter(|c| !c.is_control()).collect()
}

/// What an `::input` answer becomes: control characters dropped, spaces at both
/// ends trimmed, cut to `max` characters (not bytes); when nothing is left, the
/// `default` (cleaned the same way); `None` when that is empty too.
pub fn finish_input(raw: &str, default: Option<&str>, max: usize) -> Option<String> {
    let clean = |s: &str| {
        let kept: String = filter_typed(s).trim().chars().take(max).collect();
        kept.trim_end().to_string()
    };
    let typed = clean(raw);
    if !typed.is_empty() {
        return Some(typed);
    }
    default.map(clean).filter(|d| !d.is_empty())
}

#[derive(Debug, Clone, PartialEq)]
pub struct SpriteState {
    pub id: String,
    pub src: String,
    /// Layers of a layered sprite (`src` is then empty): (layer id, picture) in the
    /// character's layer order; an empty picture is a hidden layer. Empty for a plain sprite.
    pub layers: Vec<(String, String)>,
    /// Position as `x=` / `y=` in `::show`: the point this fraction across the sprite is this fraction across the screen.
    pub x: f32,
    pub y: f32,
    /// Drawing order among sprites: higher is in front, ties keep show order.
    pub z: i32,
    pub scale: f32,
    /// Mirrored left to right.
    pub flip: bool,
    /// Stretch along one axis, multiplied with `scale`.
    pub xscale: f32,
    pub yscale: f32,
    /// A looping move: the sprite oscillates between the values above (where it
    /// starts) and these. The oscillation's phase is not state: it restarts on a redraw.
    pub looped: Option<SpriteLoop>,
}

/// The far end of a looping `::move`, with the time of one leg and its easing.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SpriteLoop {
    pub x: f32,
    pub y: f32,
    pub scale: f32,
    pub xscale: f32,
    pub yscale: f32,
    pub time: f32,
    pub ease: Ease,
}

impl SpriteState {
    fn loop_cmd(&self, l: &SpriteLoop) -> Cmd {
        Cmd::LoopSprite {
            id: self.id.clone(),
            x: l.x,
            y: l.y,
            scale: l.scale,
            flip: self.flip,
            xscale: l.xscale,
            yscale: l.yscale,
            time: l.time,
            ease: l.ease,
        }
    }
}

/// What part of the stage is in view: `zoom` 1 shows all of it; `x`, `y` (0..1) place the view over the
/// stage like a sprite's `x=` / `y=` (0 = the left/top edge in view, 1 = the right/bottom edge).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Camera {
    pub zoom: f32,
    pub x: f32,
    pub y: f32,
}

impl Default for Camera {
    fn default() -> Self {
        Camera { zoom: 1.0, x: 0.5, y: 0.5 }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct MusicState {
    pub src: String,
    /// Played once before `src` loops.
    pub intro: Option<String>,
    pub looped: bool,
    pub volume: f32,
}

/// A looping sound effect that is part of the scene.
#[derive(Debug, Clone, PartialEq)]
pub struct SfxLoopState {
    pub src: String,
    pub volume: f32,
}

/// The line currently (or most recently) on screen.
#[derive(Debug, Clone, PartialEq)]
pub struct Line {
    pub speaker: Option<String>,
    pub color: Option<[f32; 3]>,
    pub text: String,
}

/// One active call: where `::return` goes back to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Frame {
    pub ret: usize,
    /// A Use or HUD button made the call from a line, so `::return` shows that same line again
    /// (see [`call_from_line`]).
    pub line: bool,
}

/// Everything needed to resume the game exactly: cheap to clone, easy to serialize later.
#[derive(Debug, Clone, PartialEq)]
pub struct State {
    pub pc: usize,
    pub vars: Vars,
    /// The state of the generator behind `random(lo, hi)` (splitmix64). Every roll
    /// advances it. It is part of the scene, so rollback and roll forward replay
    /// the same rolls, and saves keep it.
    pub rng: u64,
    /// Active `::call`s, innermost last.
    pub stack: Vec<Frame>,
    pub background: Option<String>,
    pub sprites: Vec<SpriteState>,
    pub camera: Camera,
    /// The ambient particles (`::particles`): only their settings. Where each one is
    /// belongs to the view, which works it out from the time.
    pub particles: Option<Particles>,
    /// The colour wash over the stage (`::tint`). Its fade is the view's.
    pub tint: Option<Tint>,
    /// `::textbox{hide}` is in effect: the dialogue box stays hidden until
    /// `::textbox{show}` or the next line or choice.
    pub textbox_hidden: bool,
    /// The stage is covered with this colour (`::fade{out}`) until `::fade{in}`.
    /// Textbox, choices and menus are not covered.
    pub cover: Option<[f32; 3]>,
    /// The HUD elements (`::hud`), in the order they were created: that is the drawing
    /// order, and an id that is created again keeps its place.
    pub hud: Vec<HudElem>,
    pub music: Option<MusicState>,
    /// Looping sound effects, in the order they started.
    pub sfx_loops: Vec<SfxLoopState>,
    /// Inline images belonging to the line that is waiting.
    pub inline: Vec<(String, String)>,
    /// Voice clip belonging to the line that is waiting.
    pub voice: Option<String>,
    pub line: Option<Line>,
    /// The blocking command currently waiting for input, if any.
    pub pending: Option<Cmd>,
    pub finished: bool,
}

impl State {
    /// Add a HUD element, or replace the one with the same id where it stands.
    pub fn set_hud(&mut self, elem: HudElem) {
        match self.hud.iter_mut().find(|h| h.id == elem.id) {
            Some(slot) => *slot = elem,
            None => self.hud.push(elem),
        }
    }

    pub fn new(program: &Program) -> State {
        State::with_seed(program, DEFAULT_SEED)
    }

    /// A new game whose `random` rolls start from `seed` (see [`Session::start_seeded`]).
    pub fn with_seed(program: &Program, seed: u64) -> State {
        State {
            pc: 0,
            vars: program.vars.clone(),
            rng: seed,
            stack: Vec::new(),
            background: None,
            sprites: Vec::new(),
            camera: Camera::default(),
            particles: None,
            tint: None,
            textbox_hidden: false,
            cover: None,
            hud: Vec::new(),
            music: None,
            sfx_loops: Vec::new(),
            inline: Vec::new(),
            voice: None,
            line: None,
            pending: None,
            finished: false,
        }
    }

    /// Commands that rebuild the current screen from scratch (after rollback or load).
    /// Starts with [`Cmd::ClearScene`] so nothing from a later point lingers.
    pub fn redraw(&self) -> Vec<Cmd> {
        let mut out = vec![Cmd::ClearScene];
        if let Some(src) = &self.background {
            out.push(Cmd::SetBackground { src: src.clone(), fade: 0.0, wipe: None });
        }
        if self.camera != Camera::default() {
            out.push(Cmd::Camera { camera: self.camera, time: 0.0, ease: Ease::Linear });
        }
        if let Some(p) = &self.particles {
            out.push(Cmd::Particles { set: Some(p.clone()), fade: 0.0 });
        }
        if let Some(t) = self.tint {
            out.push(Cmd::Tint { set: Some(t), fade: 0.0 });
        }
        if let Some(color) = self.cover {
            out.push(Cmd::FadeOut { time: 0.0, color });
        }
        if self.textbox_hidden {
            out.push(Cmd::Textbox { hidden: true, fade: 0.0 });
        }
        for h in &self.hud {
            out.push(Cmd::HudShow { elem: Box::new(h.clone()), fade: 0.0 });
        }
        for s in &self.sprites {
            out.push(Cmd::ShowSprite {
                id: s.id.clone(),
                src: s.src.clone(),
                layers: s.layers.clone(),
                x: s.x,
                y: s.y,
                z: s.z,
                scale: s.scale,
                flip: s.flip,
                xscale: s.xscale,
                yscale: s.yscale,
                fade: 0.0,
            });
            if let Some(l) = &s.looped {
                out.push(s.loop_cmd(l));
            }
        }
        match &self.music {
            Some(m) => out.push(Cmd::PlayMusic {
                src: m.src.clone(),
                intro: m.intro.clone(),
                fade: 0.0,
                looped: m.looped,
                volume: m.volume,
            }),
            None => out.push(Cmd::StopMusic { fade: 0.0 }),
        }
        for l in &self.sfx_loops {
            out.push(Cmd::PlaySfxLoop { src: l.src.clone(), fade: 0.0, volume: l.volume });
        }
        for (src, alt) in &self.inline {
            out.push(Cmd::ShowImage { src: src.clone(), alt: alt.clone() });
        }
        if let Some(src) = &self.voice {
            out.push(Cmd::PlayVoice { src: src.clone() });
        }
        // A line stays on screen under choices and waits; a pending Say shows itself.
        if !matches!(self.pending, Some(Cmd::Say { .. })) {
            if let Some(line) = &self.line {
                out.push(Cmd::Say {
                    speaker: line.speaker.clone(),
                    color: line.color,
                    text: line.text.clone(),
                });
            }
        }
        out.extend(self.pending.clone());
        if self.finished {
            out.push(Cmd::Finished);
        }
        out
    }
}

/// Safety valve: a script that loops forever without waiting for the player is stopped.
const MAX_SILENT_OPS: usize = 100_000;
/// Safety valve: runaway recursion through `::call`.
const MAX_CALL_DEPTH: usize = 256;

/// Whether the option can be picked now (its condition, if any, is true).
fn pickable(option: &ChoiceOption, vars: &Vars) -> bool {
    option.cond.as_ref().is_none_or(|c| c.test(vars))
}

/// Indices of the options on screen: the pickable ones, and the greyed `{show}` ones.
fn visible_options(options: &[ChoiceOption], vars: &Vars) -> Vec<usize> {
    let shown = |o: &ChoiceOption| match &o.lock {
        Some(l) => l.shown_if.as_ref().is_none_or(|c| c.test(vars)),
        None => pickable(o, vars),
    };
    options.iter().enumerate().filter(|(_, o)| shown(o)).map(|(i, _)| i).collect()
}

/// Whether anything in `shown` can be picked. A choice with only locked options is treated as
/// one with no options at all (the story carries on past it), or nothing could move the game on.
fn any_pickable(options: &[ChoiceOption], shown: &[usize], vars: &Vars) -> bool {
    // The jump a timed hotspot screen adds for its timeout is not something to pick.
    shown.iter().any(|&i| pickable(&options[i], vars) && !is_timeout(&options[i]))
}

/// The invisible option of a timed `::hotspots` that carries `default=#label` (see `ChoiceOption::default`).
pub fn is_timeout(option: &ChoiceOption) -> bool {
    option.default && option.area.is_some()
}

/// The time limit of the timed `::choice` or `::hotspots` the state waits at. The runtime never counts the
/// time: the frontend does, and sends [`Input::Choose`] with `default` when it runs out.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ChoiceTimer {
    pub seconds: f32,
    /// Index into the visible options (the list of [`Cmd::ShowChoices`]); never a locked one.
    pub default: usize,
}

/// The timer of the choice the state waits at, if it is a timed one. It is worked out from
/// the script and the variables each time, never stored, so rollback, saves (which are
/// untrusted) and hot reload can't leave a wrong one behind. The default is the option marked
/// `{default}`, or the first pickable one when that is hidden or locked by its condition.
pub fn choice_timer(state: &State, program: &Program) -> Option<ChoiceTimer> {
    if !matches!(state.pending, Some(Cmd::ShowChoices { .. } | Cmd::ShowHotspots { .. })) {
        return None;
    }
    let Some(Op::Choice { options, time: Some(seconds) }) = program.ops.get(state.pc) else { return None };
    let visible = visible_options(options, &state.vars);
    let open = |&i: &usize| pickable(&options[i], &state.vars);
    let default = visible.iter().position(|i| options[*i].default && open(i)).or_else(|| visible.iter().position(open))?;
    Some(ChoiceTimer { seconds: *seconds, default })
}

/// The item `id` as the screens show it. An id that no `::item` declares still
/// shows, under its own name and without a picture, so nothing in a list is lost.
pub fn item_info(program: &Program, id: &str) -> ItemInfo {
    match program.items.get(id) {
        Some(i) => ItemInfo {
            id: id.to_string(),
            name: i.name.clone(),
            icon: i.icon.clone(),
            text: i.text.clone(),
            price: i.price,
            count: None,
            has_use: i.use_label.is_some(),
        },
        None => ItemInfo { id: id.to_string(), name: id.to_string(), icon: None, text: String::new(), price: None, count: None, has_use: false },
    }
}

/// What the inventory screen lists: the items in the game's inventory list, in the order they were
/// added, then the items declared with `count=` whose count is above 0 (in declaration order; the
/// list doesn't matter for them). Empty when the game has no inventory.
pub fn inventory_items(program: &Program, vars: &Vars) -> Vec<ItemInfo> {
    let Some(Value::List(held)) = program.inventory.as_ref().and_then(|name| vars.get(name)) else {
        return Vec::new();
    };
    let counted = |id: &str| program.items.get(id).is_some_and(|i| i.count.is_some());
    let mut out: Vec<ItemInfo> = held.iter().filter(|id| !counted(id)).map(|id| item_info(program, id)).collect();
    let mut stacks: Vec<_> = program.items.values().filter(|i| i.count.is_some()).collect();
    stacks.sort_by_key(|i| i.line);
    for item in stacks {
        let n = item.count.as_ref().and_then(|c| vars.get(c)).and_then(Value::as_int).unwrap_or(0);
        if n > 0 {
            out.push(ItemInfo { count: Some(n), ..item_info(program, &item.id) });
        }
    }
    out
}

/// What the journal lists: the entries of its list in the order they were added, then those of the
/// `journal_done` list (`true`), each with its `::item` text. Counts, prices and Use do not apply.
/// Empty when the game has no journal.
pub fn journal_items(program: &Program, vars: &Vars) -> Vec<(ItemInfo, bool)> {
    let held = |name: &Option<String>| match name.as_ref().and_then(|n| vars.get(n)) {
        Some(Value::List(l)) => l.clone(),
        _ => Vec::new(),
    };
    let open = held(&program.journal);
    let done = held(&program.journal_done);
    let entry = |id: &String, done| {
        let info = item_info(program, id);
        (ItemInfo { price: None, count: None, has_use: false, ..info }, done)
    };
    open.iter().filter(|id| !done.contains(id)).map(|id| entry(id, false)).chain(done.iter().map(|id| entry(id, true))).collect()
}

/// The story waits at a line the player advances (not a choice, `::wait`, `::input`, shop, `{nw}`
/// line or video): the only place a Use or HUD button works.
pub fn waits_at_line(state: &State) -> bool {
    matches!(&state.pending, Some(Cmd::Say { .. })) && !moves_on_by_itself(&state.pending) && !state.finished
}

/// A call that comes back to this same line, which is shown again. Passing the label's start
/// counts a visit for `seen(label)`; the return takes that one back ([`return_to`]), but only when
/// the label does come back (one that jumps away leaves the count as it was).
fn call_from_line(state: &mut State, program: &Program, label: &str) {
    state.stack.push(Frame { ret: state.pc, line: true });
    state.pc = program.labels[label];
}

/// Move the visit counters of the labels at `pc` (`seen(label)`) by `delta`; a counter not set yet starts at `default`.
fn count_visits(state: &mut State, program: &Program, pc: usize, default: i64, delta: i64) {
    for var in program.counted.get(&pc).into_iter().flatten() {
        let n = state.vars.get(var).and_then(Value::as_int).unwrap_or(default);
        state.vars.insert(var.clone(), Value::Int(n.saturating_add(delta)));
    }
}

/// `::return`: back to the frame's address. A line call shows its line again, which would count a second visit.
fn return_to(state: &mut State, program: &Program, frame: Frame) {
    if frame.line {
        count_visits(state, program, frame.ret, 1, -1);
    }
    state.pc = frame.ret;
}

/// The label of the shown HUD element `id`, when it is a `call=` button.
fn hud_call(program: &Program, state: &State, id: &str) -> Option<String> {
    let elem = state.hud.iter().find(|h| h.id == id)?;
    match &elem.action {
        Some(HudAction::Call(l)) if program.labels.contains_key(l) => Some(l.clone()),
        _ => None,
    }
}

/// Whether the HUD button `id` may be pressed now: it is shown (its `if=` holds), the story waits at a
/// line (see [`waits_at_line`]), and a `call=` button also has room on the call stack.
pub fn can_press(program: &Program, state: &State, id: &str) -> bool {
    let Some(elem) = state.hud.iter().find(|h| h.id == id) else { return false };
    waits_at_line(state)
        && elem.cond.as_ref().is_none_or(|c| c.expr.test(&state.vars))
        && match &elem.action {
            Some(HudAction::Call(_)) => state.stack.len() < MAX_CALL_DEPTH && hud_call(program, state, id).is_some(),
            Some(HudAction::Open(_)) => true,
            None => false,
        }
}

/// Whether the player may use the item `id` now: the inventory shows it, it has a `use=` label, and
/// the story waits at a line (see [`waits_at_line`]), with room on the call stack.
pub fn can_use(program: &Program, state: &State, id: &str) -> bool {
    let label = program.items.get(id).and_then(|i| i.use_label.as_ref()).and_then(|l| program.labels.get(l));
    waits_at_line(state)
        && state.stack.len() < MAX_CALL_DEPTH
        && label.is_some()
        && inventory_items(program, &state.vars).iter().any(|i| i.id == id)
}

/// Indices of the shop lines currently on offer (their condition, if any, is true).
fn offered_stock(stock: &[ShopEntry], vars: &Vars) -> Vec<usize> {
    let on = |c: &Option<Cond>| c.as_ref().is_none_or(|c| c.test(vars));
    stock.iter().enumerate().filter(|(_, s)| on(&s.cond)).map(|(i, _)| i).collect()
}

fn money(vars: &Vars, pay: &str) -> i64 {
    vars.get(pay).and_then(Value::as_int).unwrap_or(0)
}

fn shop_cmd(program: &Program, pay: &str, into: &str, stock: &[ShopEntry], vars: &Vars) -> Cmd {
    let held: &[String] = match vars.get(into) {
        Some(Value::List(items)) => items,
        _ => &[],
    };
    let items = offered_stock(stock, vars)
        .into_iter()
        .map(|i| {
            let s = &stock[i];
            // A counted item can be bought again: it is never "owned".
            let owned = held.contains(&s.item) && counted_var(program, &s.item).is_none();
            ShopItem { item: item_info(program, &s.item), price: s.price, owned }
        })
        .collect();
    Cmd::ShowShop { pay: pay.to_string(), money: money(vars, pay), items }
}

/// The variable counting the item `id` (`count=`), if it has one.
fn counted_var<'a>(program: &'a Program, id: &str) -> Option<&'a String> {
    program.items.get(id)?.count.as_ref()
}

/// Buy the `index`th offered item of the shop at `state.pc`: pay and add it to the
/// list (or add 1 to its count, for an item with `count=`). `false` (and nothing changes) if it is owned, costs more than the player
/// has, or isn't there, so money can never go below zero.
fn buy(state: &mut State, program: &Program, index: usize) -> bool {
    let Some(Op::Shop { pay, into, stock }) = program.ops.get(state.pc) else { return false };
    let offered = offered_stock(stock, &state.vars);
    let Some(entry) = offered.get(index).map(|&i| &stock[i]) else { return false };
    // `into` must be a list and `pay` a number: a damaged save can break either, and
    // then taking money without delivering (or the reverse) would be a cheat or a loss.
    let Some(Value::List(held)) = state.vars.get(into) else { return false };
    if !matches!(state.vars.get(pay), None | Some(Value::Int(_))) {
        return false;
    }
    let count = counted_var(program, &entry.item);
    if count.is_some_and(|c| !matches!(state.vars.get(c), Some(Value::Int(_)))) {
        return false;
    }
    let owned = count.is_none() && held.contains(&entry.item);
    if owned || entry.price < 0 || entry.price > money(&state.vars, pay) {
        return false;
    }
    let lit = |v| Expr::Lit(v);
    Effect { var: pay.clone(), op: AssignOp::Sub, value: lit(Value::Int(entry.price)) }.apply(&mut state.vars);
    match count {
        Some(c) => Effect { var: c.clone(), op: AssignOp::Add, value: lit(Value::Int(1)) }.apply(&mut state.vars),
        None => Effect { var: into.clone(), op: AssignOp::Add, value: lit(Value::Str(entry.item.clone())) }.apply(&mut state.vars),
    }
    true
}

/// Whether the state waits at a choice, hotspot or shop that has nothing on offer
/// (so no input could ever move it on).
pub(crate) fn waits_on_nothing(state: &State, program: &Program) -> bool {
    match (&state.pending, program.ops.get(state.pc)) {
        (Some(Cmd::ShowChoices { .. } | Cmd::ShowHotspots { .. }), Some(Op::Choice { options, .. })) => {
            !any_pickable(options, &visible_options(options, &state.vars), &state.vars)
        }
        (Some(Cmd::ShowShop { .. }), Some(Op::Shop { stock, .. })) => offered_stock(stock, &state.vars).is_empty(),
        _ => false,
    }
}

/// Rebuild what the state is waiting at from the script and the current variables,
/// after they changed behind the story's back (a persistent value, see `persist.rs`).
/// If that leaves a choice or shop with nothing on offer, the story goes on past it,
/// as it would have had the variables been so when it got there.
pub(crate) fn refresh_pending(state: &mut State, program: &Program) {
    let (Some(pending), Some(op)) = (&state.pending, program.ops.get(state.pc)) else { return };
    if !screen_matches(pending, op) {
        return;
    }
    if let Some(cmd) = rebuild_pending(program, op, &state.vars, &|s| s.to_string()) {
        remember_line(state, &cmd);
        state.pending = Some(cmd);
    }
    if waits_on_nothing(state, program) {
        state.pending = None;
        step(state, program, None);
    }
}

/// Whether `pending` is the screen the op `op` shows (the one table of which waiting command goes
/// with which op; a fade-out waits as a `Wait`).
pub(crate) fn screen_matches(pending: &Cmd, op: &Op) -> bool {
    matches!(
        (pending, op),
        (Cmd::Say { .. }, Op::Say { .. })
            | (Cmd::Wait { .. }, Op::Wait { .. } | Op::FadeOut { .. })
            | (Cmd::WaitClick, Op::WaitClick)
            | (Cmd::PlayVideo { .. }, Op::Video { .. })
            | (Cmd::ShowChoices { .. } | Cmd::ShowHotspots { .. }, Op::Choice { .. })
            | (Cmd::ShowInventory, Op::Inventory)
            | (Cmd::ShowJournal, Op::Journal)
            | (Cmd::ShowShop { .. }, Op::Shop { .. })
            | (Cmd::AskInput { .. }, Op::Input { .. })
            | (Cmd::ShowTiming { .. }, Op::Timing { .. })
            | (Cmd::ShowPress { .. }, Op::Press { .. })
    )
}

/// A line that is shown is also the current line (what the backlog and the textbox read).
fn remember_line(state: &mut State, cmd: &Cmd) {
    if let Cmd::Say { speaker, color, text } = cmd {
        state.line = Some(Line { speaker: speaker.clone(), color: *color, text: text.clone() });
    }
}

/// The command a waiting op shows, for ops whose screen is built from the script
/// and the variables (`text` maps the script's text to what is shown, the identity unless the
/// game is translated; the one place that builds them: `step` shows them, a save read or a
/// changed variable rebuilds them).
pub(crate) fn rebuild_pending(program: &Program, op: &Op, vars: &Vars, text: &dyn Fn(&str) -> String) -> Option<Cmd> {
    match op {
        Op::Say { speaker, color, text } => {
            Some(Cmd::Say { speaker: speaker_name(program, speaker, vars), color: *color, text: interpolate(text, vars) })
        }
        Op::Choice { options, .. } => Some(choice_cmd(options, &visible_options(options, vars), vars, text)),
        Op::Inventory => Some(Cmd::ShowInventory),
        Op::Journal => Some(Cmd::ShowJournal),
        Op::Shop { pay, into, stock } => Some(shop_cmd(program, pay, into, stock, vars)),
        Op::Input { prompt, default, max, .. } => {
            Some(Cmd::AskInput { prompt: interpolate(&text(prompt), vars), default: default.clone(), max: *max })
        }
        Op::Timing { prompt, bar, .. } => Some(Cmd::ShowTiming { prompt: interpolate(&text(prompt), vars), bar: bar.clone() }),
        Op::Press { prompt, press, .. } => Some(Cmd::ShowPress { prompt: interpolate(&text(prompt), vars), press: press.clone() }),
        _ => None,
    }
}

/// Show the screen the waiting op `op` stands for and wait at it.
fn show_screen(state: &mut State, program: &Program, op: &Op, out: &mut Vec<Cmd>) -> Vec<Cmd> {
    let cmd = rebuild_pending(program, op, &state.vars, &|s| s.to_string()).expect("a waiting op has a screen");
    remember_line(state, &cmd);
    block(state, out, cmd)
}

/// What a choice op shows for the options at `visible`: buttons, or regions for a `::hotspots` block.
fn choice_cmd(options: &[ChoiceOption], visible: &[usize], vars: &Vars, text: &dyn Fn(&str) -> String) -> Cmd {
    if options.iter().any(|o| o.area.is_some()) {
        let spots = visible
            .iter()
            .filter_map(|&i| {
                let o = &options[i];
                let area = o.area.as_ref()?;
                Some(Spot { label: interpolate(&text(&o.text), vars), rect: area.rect, sprite: area.sprite.clone(), image: area.image.clone(), hidden: area.hidden })
            })
            .collect();
        Cmd::ShowHotspots { spots }
    } else {
        let locked = visible
            .iter()
            .map(|&i| match &options[i].lock {
                Some(l) if !pickable(&options[i], vars) => Some(interpolate(&text(&l.reason), vars)),
                _ => None,
            })
            .collect();
        Cmd::ShowChoices { options: visible.iter().map(|&i| interpolate(&text(&options[i].text), vars)).collect(), locked }
    }
}

/// The name shown for a line's speaker: a character's display name is a template, so `{hero.name}`
/// becomes the variable's value now (typed text shows as typed, never as tags). Other speaker text
/// (an undeclared `**Name:**`) and plain names are left exactly as written.
pub fn speaker_name(program: &Program, speaker: &Option<String>, vars: &Vars) -> Option<String> {
    let name = speaker.as_deref()?;
    if !name.contains('{') || !program.characters.values().any(|c| c.name == name) {
        return Some(name.to_string());
    }
    Some(fill_name(name, vars))
}

/// A name template with its variables put in (see [`speaker_name`]).
pub(crate) fn fill_name(template: &str, vars: &Vars) -> String {
    if !template.contains('{') {
        return template.to_string();
    }
    quillstone_script::markup::plain(&interpolate(template, vars))
}

/// Replace `{name}` with the variable's value. `{name}` for an unknown variable
/// is left as written so mistakes are visible. The result is still tagged text
/// (see `quillstone_script::markup`): values are escaped so they show exactly as
/// they are, and `\{` stays for the markup to turn into a brace.
pub fn interpolate(text: &str, vars: &Vars) -> String {
    if !text.contains('{') {
        return text.to_string();
    }
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(pos) = rest.find(['{', '\\']) {
        out.push_str(&rest[..pos]);
        rest = &rest[pos..];
        if let Some(escaped) = rest.strip_prefix("\\{").or_else(|| rest.strip_prefix("\\\\")) {
            out.push_str(&rest[..2]); // an escape: left for the text tags to read
            rest = escaped;
        } else if rest.starts_with('{') {
            match rest.find('}') {
                Some(end) => {
                    let name = rest[1..end].trim();
                    match vars.get(name).filter(|_| !matches!(name, "nw" | "p")) {
                        Some(v) => out.push_str(&quillstone_script::markup::escape(&v.to_string())),
                        None => out.push_str(&rest[..=end]), // unknown, or the `{nw}` / `{p}` text tags
                    }
                    rest = &rest[end + 1..];
                }
                None => {
                    out.push_str(rest);
                    rest = "";
                }
            }
        } else {
            out.push('\\');
            rest = &rest[1..];
        }
    }
    out.push_str(rest);
    out
}

/// The file a picture name stands for right now: a plain path as it is, a pattern
/// (`assets/places/{place}.png`) with its variables put in. When that can't be done
/// (a variable holds no text, or text that isn't a safe file-name fragment) the
/// picture is [`MISSING_PICTURE`] (nothing is drawn, like a file that isn't there) and a
/// [`Cmd::Warning`] is added. The result is always inside the game folder.
fn picture_path(src: &str, vars: &Vars, out: &mut Vec<Cmd>) -> String {
    if !quillstone_script::is_pattern(src) {
        return src.to_string();
    }
    match quillstone_script::resolve(src, vars) {
        Ok(path) => path,
        Err(why) => {
            out.push(Cmd::Warning { message: format!("A picture is not shown: {why}.") });
            quillstone_script::MISSING_PICTURE.to_string()
        }
    }
}

/// The layers of a layered sprite after a `::show` that sets `given` of them (see [`Op::Show`]): every
/// layer of the character in its order, the given ones resolved, the rest as the sprite on stage has them.
/// A layer with no value (the sprite is new, or was shown as a plain pose) stays hidden, with a warning.
fn merge_layers(
    program: &Program,
    id: &str,
    given: &[(String, String)],
    before: Option<&SpriteState>,
    vars: &Vars,
    out: &mut Vec<Cmd>,
) -> Vec<(String, String)> {
    let Some(character) = program.characters.get(id) else {
        return given.iter().map(|(l, p)| (l.clone(), picture_path(p, vars, out))).collect();
    };
    let mut merged = Vec::with_capacity(character.layers.len());
    let mut missing = Vec::new();
    for layer in &character.layers {
        let picture = match given.iter().find(|(l, _)| l == layer) {
            Some((_, p)) => picture_path(p, vars, out),
            None => match before.and_then(|b| b.layers.iter().find(|(l, _)| l == layer)) {
                Some((_, p)) => p.clone(),
                None => {
                    missing.push(layer.as_str());
                    String::new()
                }
            },
        };
        merged.push((layer.clone(), picture));
    }
    if !missing.is_empty() {
        let message = format!("`{id}` was shown without a value for the layer(s) {}: they stay hidden.", missing.join(", "));
        out.push(Cmd::Warning { message });
    }
    merged
}

/// Advance the script. Call once with `None` to start, then with the player's input.
///
/// Input that doesn't match what the script is waiting for is ignored and returns
/// no commands. A step that does something always returns at least one command.
pub fn step(state: &mut State, program: &Program, input: Option<Input>) -> Vec<Cmd> {
    step_traced(state, program, input, &mut |_| {}, None)
}

/// Run one effect. Each `random(lo, hi)` in it is rolled from the state's generator
/// (the seed advances), or from `hook` when the story tester supplies the values.
fn run_effect(effect: &Effect, vars: &mut Vars, rng: &mut u64, hook: &mut Option<&mut dyn FnMut(i64, i64) -> i64>) {
    effect.apply_with(vars, &mut |lo, hi| match hook {
        Some(h) => h(lo, hi),
        None => roll(rng, lo, hi),
    });
}

/// [`step`], telling `ran` the index of every op it runs (for the story tester).
/// With a `draw` hook, `random(lo, hi)` asks it for its values instead of the
/// state's generator (which then doesn't move).
pub(crate) fn step_traced(
    state: &mut State,
    program: &Program,
    input: Option<Input>,
    ran: &mut dyn FnMut(usize),
    mut draw: Option<&mut dyn FnMut(i64, i64) -> i64>,
) -> Vec<Cmd> {
    if state.finished {
        return Vec::new();
    }

    if let Some(pending) = state.pending.clone() {
        match (pending, input) {
            (
                Cmd::Say { .. }
                | Cmd::PlayVideo { .. }
                | Cmd::Wait { .. }
                | Cmd::WaitClick
                | Cmd::ShowInventory
                | Cmd::ShowJournal
                | Cmd::ShowShop { .. },
                Some(Input::Advance),
            ) => {
                state.pc += 1;
            }
            (Cmd::ShowShop { .. }, Some(Input::Buy(i))) => {
                // Stays at the shop: a purchase is a state change, and the screen is shown afresh.
                if !buy(state, program, i) {
                    return Vec::new();
                }
                let Some(op @ Op::Shop { .. }) = program.ops.get(state.pc) else { return Vec::new() };
                return show_screen(state, program, op, &mut Vec::new());
            }
            (Cmd::Say { .. }, Some(Input::Use(id))) => {
                if !can_use(program, state, &id) {
                    return Vec::new();
                }
                call_from_line(state, program, program.items[&id].use_label.as_ref().expect("checked by can_use"));
            }
            (Cmd::Say { .. }, Some(Input::Hud(id))) => {
                let Some(label) = hud_call(program, state, &id).filter(|_| can_press(program, state, &id)) else { return Vec::new() };
                call_from_line(state, program, &label);
            }
            (Cmd::AskInput { .. }, Some(Input::Text(raw))) => {
                let Some(Op::Input { var, default, max, .. }) = program.ops.get(state.pc) else { return Vec::new() };
                let Some(answer) = finish_input(&raw, default.as_deref(), *max) else { return Vec::new() };
                state.vars.insert(var.clone(), Value::Str(answer));
                state.pc += 1;
            }
            (Cmd::ShowTiming { .. }, Some(Input::Choose(i))) => {
                let Some(Op::Timing { var, bar, .. }) = program.ops.get(state.pc) else { return Vec::new() };
                let Some(hit) = Hit::from_index(i).filter(|h| bar.hits().contains(h)) else { return Vec::new() };
                state.vars.insert(var.clone(), Value::Str(hit.text().into()));
                state.pc += 1;
            }
            (Cmd::ShowPress { .. }, Some(Input::Choose(i))) => {
                let Some(Op::Press { var, .. }) = program.ops.get(state.pc) else { return Vec::new() };
                let Some(text) = KeyPress::outcome_text(i) else { return Vec::new() };
                state.vars.insert(var.clone(), Value::Str(text.into()));
                state.pc += 1;
            }
            (Cmd::ShowChoices { .. } | Cmd::ShowHotspots { .. }, Some(Input::Choose(i))) => {
                let Some(Op::Choice { options: all, .. }) = program.ops.get(state.pc) else {
                    return Vec::new();
                };
                let visible = visible_options(all, &state.vars);
                let Some(&index) = visible.get(i).filter(|&&k| pickable(&all[k], &state.vars)) else { return Vec::new() };
                let chosen = &all[index];
                for effect in &chosen.effects {
                    run_effect(effect, &mut state.vars, &mut state.rng, &mut draw);
                }
                state.pc = chosen.target;
            }
            _ => return Vec::new(),
        }
        state.pending = None;
        // Images and voice belong to the line the player just left.
        state.inline.clear();
        state.voice = None;
    } else if input.is_some() {
        return Vec::new();
    }

    let mut out = Vec::new();
    for n in 0..=MAX_SILENT_OPS {
        if n == MAX_SILENT_OPS {
            let reason = format!("the story ran {MAX_SILENT_OPS} steps without showing anything (an endless loop?)");
            out.push(Cmd::Stopped { reason });
            break;
        }
        let Some(op) = program.ops.get(state.pc) else { break };
        ran(state.pc);
        if let Some(labels) = program.replay_at.get(&state.pc) {
            out.extend(labels.iter().map(|label| Cmd::ReplayReached { label: label.clone() }));
        }
        // Passing a label that some `seen(label)` asks about.
        count_visits(state, program, state.pc, 0, 1);
        match op {
            Op::Say { .. } => {
                show_textbox(state, &mut out);
                return show_screen(state, program, op, &mut out);
            }
            Op::Video { src, fit, looped, skippable } => {
                state.line = None;
                let cmd = Cmd::PlayVideo { src: src.clone(), fit: *fit, looped: *looped, skippable: *skippable };
                return block(state, &mut out, cmd);
            }
            Op::Wait { seconds } => {
                return block(state, &mut out, Cmd::Wait { seconds: *seconds });
            }
            Op::WaitClick => return block(state, &mut out, Cmd::WaitClick),
            Op::Choice { options, .. } => {
                let visible = visible_options(options, &state.vars);
                if !any_pickable(options, &visible, &state.vars) {
                    state.pc += 1; // nothing on offer: carry on past the block
                    continue;
                }
                // Choice buttons bring the box back; a hotspot map keeps it hidden (a clean map).
                if options.iter().all(|o| o.area.is_none()) {
                    show_textbox(state, &mut out);
                }
                return show_screen(state, program, op, &mut out);
            }
            Op::Shop { stock, .. } if offered_stock(stock, &state.vars).is_empty() => {
                state.pc += 1; // nothing on offer: carry on past the block
            }
            Op::Shop { .. } | Op::Inventory | Op::Journal | Op::Input { .. } | Op::Timing { .. } | Op::Press { .. } => {
                return show_screen(state, program, op, &mut out);
            }
            Op::Image { src, alt } => {
                let src = picture_path(src, &state.vars, &mut out);
                state.inline.push((src.clone(), alt.clone()));
                out.push(Cmd::ShowImage { src, alt: alt.clone() });
                state.pc += 1;
            }
            Op::Bg { src, fade, wipe } => {
                let src = &picture_path(src, &state.vars, &mut out);
                state.background = Some(src.clone());
                out.push(Cmd::SetBackground { src: src.clone(), fade: *fade, wipe: *wipe });
                state.pc += 1;
            }
            Op::Hud { elem, fade } => {
                state.set_hud((**elem).clone());
                out.push(Cmd::HudShow { elem: elem.clone(), fade: *fade });
                state.pc += 1;
            }
            Op::HudHide { id, fade } => {
                if state.hud.iter().any(|h| h.id == *id) {
                    state.hud.retain(|h| h.id != *id);
                    out.push(Cmd::HudHide { id: id.clone(), fade: *fade });
                }
                state.pc += 1;
            }
            Op::HudClear { fade } => {
                if !state.hud.is_empty() {
                    state.hud.clear();
                    out.push(Cmd::HudClear { fade: *fade });
                }
                state.pc += 1;
            }
            Op::Show { id, src, layers: layers_given, x: x_given, y: y_given, z, scale: scale_given, flip, xscale: xscale_given, yscale: yscale_given, fade } => {
                let (y, scale) = (*y_given, *scale_given);
                let src = &picture_path(src, &state.vars, &mut out);
                let existing = state.sprites.iter().position(|s| s.id == *id);
                let layers = if src.is_empty() { merge_layers(program, id, layers_given, existing.map(|i| &state.sprites[i]), &state.vars, &mut out) } else { Vec::new() };
                // A sprite already on stage keeps its place and size unless told otherwise.
                let x = x_given.or(existing.map(|i| state.sprites[i].x)).unwrap_or(0.5);
                let scale = scale.or(existing.map(|i| state.sprites[i].scale)).unwrap_or(1.0);
                let y = y.or(existing.map(|i| state.sprites[i].y)).unwrap_or(1.0);
                let z = z.or(existing.map(|i| state.sprites[i].z)).unwrap_or(0);
                let keep = |given: Option<f32>, of: fn(&SpriteState) -> f32| given.or(existing.map(|i| of(&state.sprites[i]))).unwrap_or(1.0);
                let (xscale, yscale) = (keep(*xscale_given, |s| s.xscale), keep(*yscale_given, |s| s.yscale));
                let flip = flip.or(existing.map(|i| state.sprites[i].flip)).unwrap_or(false);
                // A loop goes on through a new pose, unless the show sets position or size values.
                let sets_values = x_given.is_some() || y_given.is_some() || scale_given.is_some() || xscale_given.is_some() || yscale_given.is_some();
                let old_loop = existing.and_then(|i| state.sprites[i].looped);
                let looped = if sets_values { None } else { old_loop };
                let sprite = SpriteState { id: id.clone(), src: src.clone(), layers: layers.clone(), x, y, z, scale, flip, xscale, yscale, looped };
                match existing {
                    Some(i) => state.sprites[i] = sprite,
                    None => state.sprites.push(sprite),
                }
                out.push(Cmd::ShowSprite { id: id.clone(), src: src.clone(), layers, x, y, z, scale, flip, xscale, yscale, fade: *fade });
                if old_loop.is_some() && looped.is_none() {
                    // The view would keep a loop whose resting values did not change: end it.
                    out.push(Cmd::MoveSprite { id: id.clone(), x, y, scale, flip, xscale, yscale, time: 0.0, ease: Ease::Linear });
                }
                state.pc += 1;
            }
            Op::Hide { id, fade } => {
                match id {
                    Some(id) => {
                        if let Some(pos) = state.sprites.iter().position(|s| s.id == *id) {
                            state.sprites.remove(pos);
                            out.push(Cmd::HideSprite { id: id.clone(), fade: *fade });
                        }
                    }
                    None => {
                        for s in state.sprites.drain(..) {
                            out.push(Cmd::HideSprite { id: s.id, fade: *fade });
                        }
                    }
                }
                state.pc += 1;
            }
            Op::Move { id, x, y, scale, flip, xscale, yscale, time, ease, looped: true } => {
                if let Some(s) = state.sprites.iter_mut().find(|s| s.id == *id) {
                    // The sprite keeps the values it has (where each leg back ends); the far end is new.
                    s.flip = flip.unwrap_or(s.flip);
                    let far = SpriteLoop {
                        x: x.unwrap_or(s.x),
                        y: y.unwrap_or(s.y),
                        scale: scale.unwrap_or(s.scale),
                        xscale: xscale.unwrap_or(s.xscale),
                        yscale: yscale.unwrap_or(s.yscale),
                        time: *time,
                        ease: *ease,
                    };
                    s.looped = Some(far);
                    out.push(s.loop_cmd(&far));
                }
                state.pc += 1;
            }
            Op::Move { id, x, y, scale, flip, xscale, yscale, time, ease, looped: false } => {
                if let Some(s) = state.sprites.iter_mut().find(|s| s.id == *id) {
                    s.looped = None; // a plain move ends a loop (the view glides on from where the sprite is)
                    s.x = x.unwrap_or(s.x);
                    s.y = y.unwrap_or(s.y);
                    s.scale = scale.unwrap_or(s.scale);
                    s.flip = flip.unwrap_or(s.flip);
                    s.xscale = xscale.unwrap_or(s.xscale);
                    s.yscale = yscale.unwrap_or(s.yscale);
                    out.push(Cmd::MoveSprite {
                        id: id.clone(),
                        x: s.x,
                        y: s.y,
                        scale: s.scale,
                        flip: s.flip,
                        xscale: s.xscale,
                        yscale: s.yscale,
                        time: *time,
                        ease: *ease,
                    });
                }
                state.pc += 1;
            }
            Op::Shake { time, strength, axis } => {
                out.push(Cmd::Shake { time: *time, strength: *strength, axis: *axis });
                state.pc += 1;
            }
            Op::Flash { time, color } => {
                out.push(Cmd::Flash { time: *time, color: *color });
                state.pc += 1;
            }
            Op::FadeOut { time, color } => {
                // The fade is one command and the wait for it another, so a click, skip
                // mode and rollback treat it like any `::wait` (see `moves_on_by_itself`).
                state.cover = Some(*color);
                out.push(Cmd::FadeOut { time: *time, color: *color });
                return block(state, &mut out, Cmd::Wait { seconds: *time });
            }
            Op::FadeIn { time } => {
                if state.cover.take().is_some() {
                    out.push(Cmd::FadeIn { time: *time });
                }
                state.pc += 1;
            }
            Op::Notify { text, time } => {
                out.push(Cmd::Notify { text: interpolate(text, &state.vars), time: *time, source: text.clone() });
                state.pc += 1;
            }
            Op::Unlock { src } => {
                out.push(Cmd::Unlock { src: src.clone() });
                state.pc += 1;
            }
            Op::Textbox { hidden, fade } => {
                state.textbox_hidden = *hidden;
                out.push(Cmd::Textbox { hidden: *hidden, fade: *fade });
                state.pc += 1;
            }
            Op::Camera { zoom, x, y, time, ease } => {
                let c = &mut state.camera;
                *c = Camera { zoom: zoom.unwrap_or(c.zoom), x: x.unwrap_or(c.x), y: y.unwrap_or(c.y) };
                out.push(Cmd::Camera { camera: *c, time: *time, ease: *ease });
                state.pc += 1;
            }
            Op::Particles { set, fade } => {
                // Stopping what isn't running says nothing.
                if set.is_some() || state.particles.is_some() {
                    state.particles = set.clone();
                    out.push(Cmd::Particles { set: set.clone(), fade: *fade });
                }
                state.pc += 1;
            }
            Op::Tint { set, fade } => {
                // Removing what isn't there says nothing.
                if set.is_some() || state.tint.is_some() {
                    state.tint = *set;
                    out.push(Cmd::Tint { set: *set, fade: *fade });
                }
                state.pc += 1;
            }
            Op::Music { src, intro, fade, looped, volume } => {
                state.music = Some(MusicState { src: src.clone(), intro: intro.clone(), looped: *looped, volume: *volume });
                out.push(Cmd::PlayMusic { src: src.clone(), intro: intro.clone(), fade: *fade, looped: *looped, volume: *volume });
                state.pc += 1;
            }
            Op::StopMusic { fade } => {
                state.music = None;
                out.push(Cmd::StopMusic { fade: *fade });
                state.pc += 1;
            }
            Op::Sfx { src, volume } => {
                out.push(Cmd::PlaySfx { src: src.clone(), volume: *volume });
                state.pc += 1;
            }
            Op::SfxLoop { src, fade, volume } => {
                match state.sfx_loops.iter_mut().find(|l| l.src == *src) {
                    Some(l) => l.volume = *volume,
                    None => state.sfx_loops.push(SfxLoopState { src: src.clone(), volume: *volume }),
                }
                out.push(Cmd::PlaySfxLoop { src: src.clone(), fade: *fade, volume: *volume });
                state.pc += 1;
            }
            Op::StopSfx { src, fade } => {
                state.sfx_loops.retain(|l| src.as_ref().is_some_and(|s| *s != l.src));
                out.push(Cmd::StopSfx { src: src.clone(), fade: *fade });
                state.pc += 1;
            }
            Op::Voice { src } => {
                state.voice = Some(src.clone());
                out.push(Cmd::PlayVoice { src: src.clone() });
                state.pc += 1;
            }
            Op::Jump { target, cond } => {
                let taken = cond.as_ref().is_none_or(|c| c.test(&state.vars));
                state.pc = if taken { *target } else { state.pc + 1 };
            }
            Op::Call { target, args } => {
                if state.stack.len() >= MAX_CALL_DEPTH {
                    let reason = format!("`::call` nested more than {MAX_CALL_DEPTH} deep (a call that never returns?)");
                    out.push(Cmd::Stopped { reason });
                    break;
                }
                // The parameters are ordinary variables: every value is worked out
                // first (so `a=b b=a` swaps), then stored. Nothing undoes it at `::return`.
                let values: Vec<Value> = args.iter().map(|(_, e)| e.eval(&state.vars)).collect();
                for ((name, _), value) in args.iter().zip(values) {
                    if !matches!(value, Value::List(_)) {
                        state.vars.insert(name.clone(), value);
                    }
                }
                state.stack.push(Frame { ret: state.pc + 1, line: false });
                state.pc = *target;
            }
            Op::Return => match state.stack.pop() {
                Some(frame) => return_to(state, program, frame),
                None => break, // `::return` outside a `::call` ends the story
            },
            Op::Set(effect) => {
                run_effect(effect, &mut state.vars, &mut state.rng, &mut draw);
                state.pc += 1;
            }
            Op::EndReplay => {
                out.push(Cmd::EndReplay);
                state.pc += 1;
            }
            Op::End => break,
        }
    }

    state.finished = true;
    out.push(Cmd::Finished);
    out
}

/// Whether the story leaves this waiting step without the player: a `::wait`, a
/// video, or a line ending in `{nw}`. Rollback must not stop on one, it would run on again.
fn moves_on_by_itself(pending: &Option<Cmd>) -> bool {
    match pending {
        Some(Cmd::Wait { .. } | Cmd::PlayVideo { .. }) => true,
        Some(Cmd::Say { text, .. }) => quillstone_script::markup::rich(text).no_wait,
        _ => false,
    }
}

/// A line or choice arrives: a hidden dialogue box comes back (so the story never moves on unseen).
fn show_textbox(state: &mut State, out: &mut Vec<Cmd>) {
    if state.textbox_hidden {
        state.textbox_hidden = false;
        out.push(Cmd::Textbox { hidden: false, fade: 0.0 });
    }
}

fn block(state: &mut State, out: &mut Vec<Cmd>, cmd: Cmd) -> Vec<Cmd> {
    state.pending = Some(cmd.clone());
    out.push(cmd);
    std::mem::take(out)
}

/// One entry in the backlog: a line the player read, or a choice they made.
#[derive(Debug, Clone, PartialEq)]
pub enum Logged {
    Line(Line),
    Choice(String),
}

/// How many snapshots a [`Session`] keeps for rollback (roll forward holds at most as many).
/// Past it the oldest are dropped: rollback just stops there, and the backlog covers
/// the retained history only.
pub const MAX_HISTORY: usize = 500;

/// A running game: the state plus a snapshot per player action, for rollback.
pub struct Session<'p> {
    program: &'p Program,
    state: State,
    /// The state before each accepted input, and that input.
    history: VecDeque<(State, Input)>,
    /// What rollback popped, newest rollback on top: each entry is the state the player was
    /// rolled back from (the one after the input) and that input. Not saved.
    redo: Vec<(State, Input)>,
    /// Data kept across playthroughs (see `persist.rs`), and whether it changed.
    persistent: Vars,
    persistent_changed: bool,
}

impl<'p> Session<'p> {
    /// Start the script; returns the session and the commands to show first.
    pub fn start(program: &'p Program) -> (Session<'p>, Vec<Cmd>) {
        Session::start_with(program, Vars::new())
    }

    /// A new game: the script's starting variables, `persistent` applied, an empty scene and history.
    fn fresh(program: &'p Program, persistent: Vars, seed: u64) -> Session<'p> {
        let mut session = Session {
            program,
            state: State::with_seed(program, seed),
            history: VecDeque::new(),
            redo: Vec::new(),
            persistent,
            persistent_changed: false,
        };
        session.overlay();
        session
    }

    pub fn state(&self) -> &State {
        &self.state
    }

    pub fn can_rollback(&self) -> bool {
        !self.history.is_empty()
    }

    pub fn can_roll_forward(&self) -> bool {
        !self.redo.is_empty()
    }

    pub fn input(&mut self, input: Input) -> Vec<Cmd> {
        let before = self.state.clone();
        let cmds = step(&mut self.state, self.program, Some(input.clone()));
        if !cmds.is_empty() {
            self.push_history(before, input);
            self.redo.clear(); // a new path: what was rolled back is gone
        }
        self.note(&cmds);
        cmds
    }

    /// Go back one step. Returns the commands that redraw the earlier screen.
    ///
    /// A `::wait`, `::video` or `{nw}` line is not somewhere the player can stop: it
    /// would run out and carry the story forward again. So rollback goes past them to the
    /// screen before them (unless the very first screen is one).
    pub fn rollback(&mut self) -> Option<Vec<Cmd>> {
        let (first, input) = self.history.pop_back()?;
        self.redo.push((std::mem::replace(&mut self.state, first), input));
        while moves_on_by_itself(&self.state.pending) {
            match self.history.pop_back() {
                Some((earlier, input)) => self.redo.push((std::mem::replace(&mut self.state, earlier), input)),
                None => break,
            }
        }
        self.overlay(); // going back doesn't take back what was kept for good
        Some(self.state.redraw())
    }

    fn push_history(&mut self, state: State, input: Input) {
        self.history.push_back((state, input));
        if self.history.len() > MAX_HISTORY {
            self.history.pop_front();
        }
    }

    /// Undo a rollback: go on to the screen the player rolled back from. Waits and
    /// videos rollback went past are gone past again. `None` with nothing to redo.
    pub fn roll_forward(&mut self) -> Option<Vec<Cmd>> {
        let (after, input) = self.redo.pop()?;
        let before = std::mem::replace(&mut self.state, after);
        self.push_history(before, input);
        while moves_on_by_itself(&self.state.pending) {
            let Some((after, input)) = self.redo.pop() else { break };
            let before = std::mem::replace(&mut self.state, after);
            self.push_history(before, input);
        }
        self.overlay();
        Some(self.state.redraw())
    }

    /// Everything read and chosen on the way to the current screen, oldest first,
    /// including the line on screen now. Built from the rollback history, so a
    /// rollback also takes the lines after it out of the backlog.
    pub fn backlog(&self) -> Vec<Logged> {
        self.backlog_with(&quillstone_script::Catalog::default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use quillstone_script::{parse, Value};

    fn say(speaker: &str, text: &str) -> Cmd {
        Cmd::Say { speaker: Some(speaker.into()), color: None, text: text.into() }
    }

    fn int(n: i64) -> Value {
        Value::Int(n)
    }

    const STORY: &str = "\
---
vars: { trust: 0 }
---
# start
::bg{src=a.webp}
**A:** First.
::choice
- [Good](#good){trust+=1}
- [Bad](#bad){trust-=1}
::
# good
**A:** Nice.
::jump{to=end}
# bad
**A:** Ouch.
# end
::jump{to=happy if=\"trust>0\"}
**A:** Cold ending.
::end
# happy
**A:** Warm ending.
";

    // ---- the basics ----------------------------------------------------

    #[test]
    fn runs_to_the_first_blocking_op_and_collects_silent_commands() {
        let p = parse(STORY).unwrap();
        let mut s = State::new(&p);
        let cmds = step(&mut s, &p, None);
        assert_eq!(cmds, vec![Cmd::SetBackground { src: "a.webp".into(), fade: 0.0, wipe: None }, say("A", "First.")]);
        assert_eq!(s.background.as_deref(), Some("a.webp"));
    }

    #[test]
    fn choices_apply_effects_and_route() {
        let p = parse(STORY).unwrap();
        let (mut game, _) = Session::start(&p);
        game.input(Input::Advance); // past "First."
        assert_eq!(game.input(Input::Choose(0)), vec![say("A", "Nice.")]);
        assert_eq!(game.state().vars["trust"], int(1));
        assert_eq!(game.input(Input::Advance), vec![say("A", "Warm ending.")]);
        assert_eq!(game.input(Input::Advance), vec![Cmd::Finished]);
        assert!(game.state().finished);
    }

    #[test]
    fn the_other_branch_reaches_the_other_ending() {
        let p = parse(STORY).unwrap();
        let (mut game, _) = Session::start(&p);
        game.input(Input::Advance);
        game.input(Input::Choose(1));
        assert_eq!(game.input(Input::Advance), vec![say("A", "Cold ending.")]);
    }

    #[test]
    fn wrong_or_out_of_range_input_is_ignored() {
        let p = parse(STORY).unwrap();
        let (mut game, _) = Session::start(&p);
        assert!(game.input(Input::Choose(0)).is_empty()); // waiting for Advance
        game.input(Input::Advance);
        assert!(game.input(Input::Choose(9)).is_empty()); // out of range
        assert!(game.input(Input::Advance).is_empty()); // waiting for a choice
    }

    #[test]
    fn endless_silent_loop_is_stopped() {
        let p = parse("# a\n::set{n+=1}\n::jump{to=a}\n").unwrap();
        let mut s = State::new(&p);
        let cmds = step(&mut s, &p, None);
        assert!(matches!(&cmds[..], [Cmd::Stopped { reason }, Cmd::Finished] if reason.contains("endless loop")));
        assert!(s.finished);
    }

    // ---- rollback ----------------------------------------------------------

    #[test]
    fn rollback_restores_state_and_redraws_the_whole_screen() {
        let p = parse(STORY).unwrap();
        let (mut game, _) = Session::start(&p);
        game.input(Input::Advance);
        game.input(Input::Choose(0));
        assert_eq!(game.state().vars["trust"], int(1));

        let redraw = game.rollback().unwrap();
        assert_eq!(game.state().vars["trust"], int(0));
        assert_eq!(
            redraw,
            vec![
                Cmd::ClearScene,
                Cmd::SetBackground { src: "a.webp".into(), fade: 0.0, wipe: None },
                Cmd::StopMusic { fade: 0.0 },
                say("A", "First."), // the line stays under the choices
                Cmd::ShowChoices { options: vec!["Good".into(), "Bad".into()], locked: vec![None; 2] },
            ]
        );
        // and we can take the other branch from there
        assert_eq!(game.input(Input::Choose(1)), vec![say("A", "Ouch.")]);
    }

    #[test]
    fn rollback_goes_past_waits_instead_of_landing_on_them() {
        // Landing on the wait would let it run out and carry the story forward again.
        let p = parse("**A:** one\n::wait{time=1}\n::wait{time=1}\n**A:** two\n").unwrap();
        let (mut game, _) = Session::start(&p);
        game.input(Input::Advance); // first wait
        game.input(Input::Advance); // second wait
        game.input(Input::Advance); // "two"
        let redraw = game.rollback().unwrap();
        assert_eq!(redraw.last(), Some(&say("A", "one")));
        assert!(matches!(game.state().pending, Some(Cmd::Say { .. })));
        assert!(!game.can_rollback());
    }

    #[test]
    fn rollback_goes_past_videos_instead_of_landing_on_them() {
        let p = parse("**A:** one\n::video{src=c.webm}\n**A:** two\n").unwrap();
        let (mut game, _) = Session::start(&p);
        game.input(Input::Advance); // video
        assert!(matches!(game.state().pending, Some(Cmd::PlayVideo { .. })));
        game.input(Input::Advance); // "two"
        let redraw = game.rollback().unwrap();
        assert_eq!(redraw.last(), Some(&say("A", "one")));
        assert!(matches!(game.state().pending, Some(Cmd::Say { .. })));
        assert!(!game.can_rollback());
    }

    #[test]
    fn a_click_wait_stops_for_the_player_and_rollback_and_saves_land_on_it() {
        let p = parse("**A:** one\n::wait{click}\n**A:** two\n").unwrap();
        let (mut game, _) = Session::start(&p);
        assert_eq!(game.input(Input::Advance), vec![Cmd::WaitClick]);
        assert!(game.input(Input::Choose(0)).is_empty());
        let (mut loaded, cmds) = Session::load(&p, &game.save()).unwrap();
        assert_eq!(cmds.last(), Some(&Cmd::WaitClick), "a save made at it loads there");
        assert_eq!(lines(&loaded.input(Input::Advance)), vec!["two"]);
        // unlike a timed wait the story doesn't move on by itself, so rollback stops here
        assert_eq!(lines(&game.input(Input::Advance)), vec!["two"]);
        assert_eq!(game.rollback().unwrap().last(), Some(&Cmd::WaitClick));
        assert!(game.can_rollback());
    }

    #[test]
    fn a_story_that_starts_with_a_wait_can_still_roll_back_to_it() {
        let p = parse("::wait{time=1}\n**A:** one\n").unwrap();
        let (mut game, _) = Session::start(&p);
        game.input(Input::Advance);
        assert_eq!(game.rollback().unwrap().last(), Some(&Cmd::Wait { seconds: 1.0 }));
    }

    #[test]
    fn the_backlog_lists_lines_and_choices_and_follows_rollback() {
        let p = parse(STORY).unwrap();
        let (mut game, _) = Session::start(&p);
        let line = |t: &str| Logged::Line(Line { speaker: Some("A".into()), color: None, text: t.into() });
        assert_eq!(game.backlog(), vec![line("First.")]);
        game.input(Input::Advance);
        assert!(game.input(Input::Advance).is_empty()); // ignored input leaves no trace
        game.input(Input::Choose(1));
        assert_eq!(game.backlog(), vec![line("First."), Logged::Choice("Bad".into()), line("Ouch.")]);
        game.rollback();
        assert_eq!(game.backlog(), vec![line("First.")]);
    }

    #[test]
    fn roll_forward_restores_state_and_vars() {
        let p = parse(STORY).unwrap();
        let (mut game, _) = Session::start(&p);
        game.input(Input::Advance);
        game.input(Input::Choose(0));
        let at = game.state().clone();
        let redraw_at = at.redraw();
        game.rollback();
        assert_eq!(game.state().vars["trust"], int(0));
        assert!(game.can_roll_forward());
        assert_eq!(game.roll_forward().unwrap(), redraw_at);
        assert_eq!(game.state().vars["trust"], int(1));
        assert_eq!(game.state(), &at);
        assert!(!game.can_roll_forward());
        assert!(game.roll_forward().is_none());
        assert!(game.can_rollback());
    }

    #[test]
    fn a_new_input_clears_roll_forward() {
        let p = parse(STORY).unwrap();
        let (mut game, _) = Session::start(&p);
        game.input(Input::Advance);
        game.rollback();
        assert!(game.can_roll_forward());
        game.input(Input::Advance);
        assert!(!game.can_roll_forward());
        assert!(game.roll_forward().is_none());
    }

    #[test]
    fn roll_forward_comes_back_across_a_skipped_wait_and_video() {
        let p = parse("**A:** one\n::wait{time=1}\n::video{src=c.webm}\n**A:** two\n").unwrap();
        let (mut game, _) = Session::start(&p);
        game.input(Input::Advance);
        game.input(Input::Advance);
        game.input(Input::Advance);
        let at = game.state().clone();
        game.rollback();
        assert!(!game.can_rollback());
        let redraw = game.roll_forward().unwrap();
        assert_eq!(redraw.last(), Some(&say("A", "two")));
        assert_eq!(game.state(), &at);
        assert!(!game.can_roll_forward());
        // and it rolls back and forward again the same way
        game.rollback();
        game.roll_forward();
        assert_eq!(game.state(), &at);
    }

    #[test]
    fn several_rollbacks_roll_forward_one_screen_at_a_time() {
        let p = parse("**A:** one\n**A:** two\n**A:** three\n").unwrap();
        let (mut game, _) = Session::start(&p);
        game.input(Input::Advance);
        game.input(Input::Advance);
        game.rollback();
        game.rollback();
        assert_eq!(game.roll_forward().unwrap().last(), Some(&say("A", "two")));
        assert_eq!(game.roll_forward().unwrap().last(), Some(&say("A", "three")));
        assert!(game.roll_forward().is_none());
    }

    #[test]
    fn the_backlog_and_saves_follow_roll_forward() {
        let p = parse(STORY).unwrap();
        let (mut game, _) = Session::start(&p);
        let line = |t: &str| Logged::Line(Line { speaker: Some("A".into()), color: None, text: t.into() });
        game.input(Input::Advance);
        game.input(Input::Choose(1));
        let full = game.backlog();
        game.rollback();
        assert_eq!(game.backlog(), vec![line("First.")]);
        game.roll_forward();
        assert_eq!(game.backlog(), full);
        game.rollback();
        let (loaded, _) = Session::load(&p, &game.save()).unwrap();
        assert!(!loaded.can_roll_forward());
        assert_eq!(loaded.state(), game.state());
    }

    #[test]
    fn rollback_with_no_history_is_none() {
        let p = parse(STORY).unwrap();
        let (mut game, _) = Session::start(&p);
        assert!(!game.can_rollback());
        assert!(game.rollback().is_none());
    }

    // ---- conditions and blocks ---------------------------------------------

    fn run_to_end(src: &str, inputs: &[Input]) -> Vec<Cmd> {
        let p = parse(src).unwrap();
        let (mut game, mut all) = Session::start(&p);
        for input in inputs {
            all.extend(game.input(input.clone()));
        }
        all
    }

    fn lines(cmds: &[Cmd]) -> Vec<String> {
        cmds.iter()
            .filter_map(|c| match c {
                Cmd::Say { text, .. } => Some(text.clone()),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn if_elif_else_picks_exactly_one_branch() {
        let src = |trust: i64| {
            format!(
                "---\nvars: {{ t: {trust} }}\n---\n::if{{t > 1}}\n**X:** high\n::elif{{t > 0}}\n**X:** mid\n::else\n**X:** low\n::endif\n**X:** after\n"
            )
        };
        let adv = vec![Input::Advance; 2];
        assert_eq!(lines(&run_to_end(&src(2), &adv)), vec!["high", "after"]);
        assert_eq!(lines(&run_to_end(&src(1), &adv)), vec!["mid", "after"]);
        assert_eq!(lines(&run_to_end(&src(0), &adv)), vec!["low", "after"]);
    }

    #[test]
    fn conditions_can_combine_and_compare_text() {
        let src = "---\nvars: { name: \"Mika\", a: true, b: false }\n---\n::if{name == \"Mika\" and (a or b) and not b}\n**X:** yes\n::else\n**X:** no\n::endif\n";
        assert_eq!(lines(&run_to_end(src, &[])), vec!["yes"]);
    }

    #[test]
    fn choice_options_can_be_hidden_by_a_condition() {
        let src = "---\nvars: { met: false }\n---\n::choice\n- [One](#x)\n- [Secret](#x){if met}\n- [Two](#x)\n::\n# x\n**A:** done\n";
        let p = parse(src).unwrap();
        let (mut game, first) = Session::start(&p);
        assert_eq!(first, vec![Cmd::ShowChoices { options: vec!["One".into(), "Two".into()], locked: vec![None; 2] }]);
        // index 1 means the second *visible* option ("Two"), and it works
        assert_eq!(game.input(Input::Choose(1)), vec![say_none("done")]);

        fn say_none(t: &str) -> Cmd {
            Cmd::Say { speaker: Some("A".into()), color: None, text: t.into() }
        }
    }

    #[test]
    fn a_hidden_option_appears_once_its_condition_holds() {
        let src = "---\nvars: { met: true }\n---\n::choice\n- [One](#x)\n- [Secret](#x){if met, got+=1}\n::\n# x\n**A:** done\n";
        let p = parse(src).unwrap();
        let (mut game, first) = Session::start(&p);
        assert_eq!(first, vec![Cmd::ShowChoices { options: vec!["One".into(), "Secret".into()], locked: vec![None; 2] }]);
        game.input(Input::Choose(1));
        assert_eq!(game.state().vars["got"], int(1));
    }

    #[test]
    fn a_choice_with_nothing_on_offer_is_skipped() {
        let src = "::choice\n- [Secret](#x){if never}\n::\n# x\n**A:** carried on\n";
        assert_eq!(lines(&run_to_end(src, &[])), vec!["carried on"]);
    }

    // ---- call / return -----------------------------------------------------

    #[test]
    fn call_runs_a_subroutine_and_comes_back() {
        let src = "**A:** before\n::call{to=sub}\n**A:** after\n::end\n# sub\n**A:** inside\n::set{seen=true}\n::return\n";
        let p = parse(src).unwrap();
        let (mut game, first) = Session::start(&p);
        assert_eq!(lines(&first), vec!["before"]);
        assert_eq!(lines(&game.input(Input::Advance)), vec!["inside"]);
        assert_eq!(game.state().stack.len(), 1);
        assert_eq!(lines(&game.input(Input::Advance)), vec!["after"]);
        assert!(game.state().stack.is_empty());
        assert_eq!(game.state().vars["seen"], Value::Bool(true));
    }

    #[test]
    fn calls_nest_and_rollback_restores_the_stack() {
        let src = "::call{to=outer}\n**A:** done\n::end\n# outer\n::call{to=inner}\n**A:** outer end\n::return\n# inner\n**A:** inner line\n::return\n";
        let p = parse(src).unwrap();
        let (mut game, first) = Session::start(&p);
        assert_eq!(lines(&first), vec!["inner line"]);
        assert_eq!(game.state().stack.len(), 2);
        assert_eq!(lines(&game.input(Input::Advance)), vec!["outer end"]);
        assert_eq!(game.state().stack.len(), 1);
        game.rollback();
        assert_eq!(game.state().stack.len(), 2); // back inside `inner`
    }

    #[test]
    fn return_without_a_call_ends_the_story() {
        assert_eq!(run_to_end("::return\n**A:** unreachable\n", &[]), vec![Cmd::Finished]);
    }

    #[test]
    fn runaway_recursion_is_stopped() {
        let cmds = run_to_end("# a\n::call{to=a}\n", &[]);
        assert!(matches!(&cmds[..], [Cmd::Stopped { reason }, Cmd::Finished] if reason.contains("256 deep")));
    }

    // ---- scene: sprites, music, inline images, voice ------------------------

    const SCENE: &str = "\
::character{id=mika name=Mika color=#ff0000 sprites=m_{pose}.png}
::show{mika neutral x=left fade=0.3}
**Mika:** Hi
::show{mika happy}
::move{mika x=right time=1}
**Mika:** Again
::hide{mika fade=0.5}
**Mika:** Gone
";

    #[test]
    fn sprites_are_tracked_and_poses_keep_their_position() {
        let p = parse(SCENE).unwrap();
        let (mut game, first) = Session::start(&p);
        assert_eq!(
            first[0],
            Cmd::ShowSprite { id: "mika".into(), src: "m_neutral.png".into(), layers: vec![], x: 0.0, y: 1.0, z: 0, scale: 1.0, flip: false, xscale: 1.0, yscale: 1.0, fade: 0.3 }
        );
        // speaker colour comes from the character
        assert!(matches!(&first[1], Cmd::Say { color: Some(c), .. } if c[0] == 1.0));

        let cmds = game.input(Input::Advance);
        // the new pose keeps the old x; then it moves
        assert_eq!(
            cmds[0],
            Cmd::ShowSprite { id: "mika".into(), src: "m_happy.png".into(), layers: vec![], x: 0.0, y: 1.0, z: 0, scale: 1.0, flip: false, xscale: 1.0, yscale: 1.0, fade: 0.0 }
        );
        assert_eq!(cmds[1], Cmd::MoveSprite { id: "mika".into(), x: 1.0, y: 1.0, scale: 1.0, flip: false, xscale: 1.0, yscale: 1.0, time: 1.0, ease: Ease::InOut });
        assert_eq!(game.state().sprites.len(), 1);
        assert_eq!(game.state().sprites[0].x, 1.0);

        let cmds = game.input(Input::Advance);
        assert_eq!(cmds[0], Cmd::HideSprite { id: "mika".into(), fade: 0.5 });
        assert!(game.state().sprites.is_empty());
    }

    #[test]
    fn moves_keep_what_they_dont_change_and_the_camera_is_part_of_the_scene() {
        let src = "::show{src=s.png id=s x=left scale=0.8}\n::move{s scale=1.2}\n::camera{zoom=1.5}\n::camera{x=0.2 time=3}\n::shake\n**A:** one\n**A:** two\n";
        let p = parse(src).unwrap();
        let (mut game, first) = Session::start(&p);
        assert!(first.contains(&Cmd::MoveSprite { id: "s".into(), x: 0.0, y: 1.0, scale: 1.2, flip: false, xscale: 1.0, yscale: 1.0, time: 0.5, ease: Ease::InOut }));
        let camera = Camera { zoom: 1.5, x: 0.2, y: 0.5 };
        assert!(first.contains(&Cmd::Camera { camera, time: 3.0, ease: Ease::InOut }), "keeps the zoom it had");
        assert!(first.contains(&Cmd::Shake { time: 0.4, strength: 20.0, axis: Axis::Both }));
        assert_eq!(game.state().camera, camera);
        game.input(Input::Advance);
        let redraw = game.rollback().unwrap();
        assert!(redraw.contains(&Cmd::Camera { camera, time: 0.0, ease: Ease::Linear }), "rollback restores the view");
        assert!(!redraw.iter().any(|c| matches!(c, Cmd::Shake { .. })), "a shake is a moment, not a state");
        assert!(redraw.iter().any(|c| matches!(c, Cmd::ShowSprite { scale, .. } if *scale == 1.2)));
    }

    #[test]
    fn a_new_pose_keeps_the_size_too() {
        let src = "::show{src=a.png id=s}\n::move{s scale=1.4 time=0}\n::show{src=b.png id=s}\n**A:** big\n::show{src=c.png id=s scale=0.5}\n**A:** small\n";
        let p = parse(src).unwrap();
        let (mut game, first) = Session::start(&p);
        assert!(first.iter().any(|c| matches!(c, Cmd::ShowSprite { src, scale, .. } if src == "b.png" && *scale == 1.4)));
        let cmds = game.input(Input::Advance);
        assert!(matches!(&cmds[0], Cmd::ShowSprite { scale, .. } if *scale == 0.5), "unless told otherwise");
    }

    #[test]
    fn sprites_have_a_height_and_a_depth_that_shows_and_moves_keep() {
        let src = "::show{src=a.png id=a y=0.9 z=2}\n::show{src=b.png id=b}\n**A:** one\n::show{src=a2.png id=a}\n::move{a y=0.7 time=0.3 ease=out}\n**A:** two\n::move{a x=left time=0}\n::show{src=a3.png id=a y=1.1 z=-4}\n**A:** three\n";
        let p = parse(src).unwrap();
        let (mut game, first) = Session::start(&p);
        assert!(first.iter().any(|c| matches!(c, Cmd::ShowSprite { id, y, z, .. } if id == "a" && *y == 0.9 && *z == 2)));
        assert!(first.iter().any(|c| matches!(c, Cmd::ShowSprite { id, y, z, .. } if id == "b" && *y == 1.0 && *z == 0)), "new: bottom edge, z 0");
        let two = game.input(Input::Advance);
        assert!(matches!(&two[0], Cmd::ShowSprite { y, z, .. } if *y == 0.9 && *z == 2), "a new pose keeps y and z");
        assert_eq!(two[1], Cmd::MoveSprite { id: "a".into(), x: 0.5, y: 0.7, scale: 1.0, flip: false, xscale: 1.0, yscale: 1.0, time: 0.3, ease: Ease::Out });
        assert_eq!(game.state().sprites[0].y, 0.7);
        let three = game.input(Input::Advance);
        assert!(matches!(&three[0], Cmd::MoveSprite { x, y, .. } if *x == 0.0 && *y == 0.7), "an `at` move keeps y");
        assert!(matches!(&three[1], Cmd::ShowSprite { y, z, .. } if *y == 1.1 && *z == -4), "unless told otherwise");

        // rollback restores y and z, and redraw carries them
        game.rollback();
        let a = game.state().sprites.iter().find(|s| s.id == "a").unwrap().clone();
        assert_eq!((a.y, a.z), (0.7, 2));
        assert!(game.state().redraw().iter().any(|c| matches!(c, Cmd::ShowSprite { id, y, z, .. } if id == "a" && *y == 0.7 && *z == 2)));
        game.rollback();
        assert_eq!(game.state().sprites[0].y, 0.9);
    }

    #[test]
    fn saves_keep_sprite_height_and_depth_and_old_saves_load_with_defaults() {
        let p = parse("::show{src=s.png id=s y=0.85 z=7}\n::move{s scale=1.5 time=0}\n**A:** hi\n").unwrap();
        let (game, _) = Session::start(&p);
        let text = game.save();
        assert!(text.contains(" 0.85 7"), "{text}");
        let (loaded, cmds) = Session::load(&p, &text).unwrap();
        let s = &loaded.state().sprites[0];
        assert_eq!((s.y, s.z, s.scale), (0.85, 7, 1.5));
        assert!(cmds.iter().any(|c| matches!(c, Cmd::ShowSprite { y, z, .. } if *y == 0.85 && *z == 7)));
        // a save from before y and z: the sprite record stops after the scale
        let old: String = text
            .lines()
            .map(|l| if l.starts_with("sprite ") { l.rsplitn(6, ' ').last().unwrap().to_string() } else { l.to_string() })
            .collect::<Vec<_>>()
            .join("\n")
            + "\n";
        assert!(old.contains("sprite s \"s.png\" 0.5 1.5\n") || old.contains("sprite \"s\" \"s.png\" 0.5 1.5\n"), "{old}");
        let (loaded, _) = Session::load(&p, &old).unwrap();
        let s = &loaded.state().sprites[0];
        assert_eq!((s.y, s.z, s.scale), (1.0, 0, 1.5));
        // hand-edited nonsense is pulled into range
        let wild = text.replace(" 0.85 7", " 9 900");
        let (loaded, _) = Session::load(&p, &wild).unwrap();
        assert_eq!((loaded.state().sprites[0].y, loaded.state().sprites[0].z), (3.0, 100));
        let nan = text.replace(" 0.85 7", " NaN 7");
        assert_eq!(Session::load(&p, &nan).unwrap().0.state().sprites[0].y, 1.0);
    }

    #[test]
    fn a_sprite_slid_off_the_screen_is_saved_and_a_save_with_the_old_positions_is_refused() {
        let p = parse("::show{src=s.png id=s x=-150% y=250%}\n**A:** hi\n").unwrap();
        let (game, _) = Session::start(&p);
        let text = game.save();
        let (loaded, _) = Session::load(&p, &text).unwrap();
        assert_eq!((loaded.state().sprites[0].x, loaded.state().sprites[0].y), (-1.5, 2.5));
        let old = text.replacen("quillstone-session 2", "quillstone-session 1", 1);
        assert!(Session::load(&p, &old).is_err(), "positions changed meaning: an old save is not read");
    }

    #[test]
    fn rollback_goes_past_no_wait_lines_and_roll_forward_does_too() {
        let p = parse("**A:** one\n**A:** two{nw}\n**A:** three{nw}\n**A:** four\n").unwrap();
        let (mut game, _) = Session::start(&p);
        game.input(Input::Advance); // "two{nw}" would go on by itself
        game.input(Input::Advance); // "three{nw}"
        game.input(Input::Advance); // "four"
        assert_eq!(game.rollback().unwrap().last(), Some(&say("A", "one")), "not onto a line that moves on");
        assert!(!game.can_rollback());
        let fwd = game.roll_forward().unwrap();
        assert_eq!(fwd.last(), Some(&say("A", "four")));
        // the backlog still has the lines that went by on their own
        let texts: Vec<String> = game
            .backlog()
            .into_iter()
            .filter_map(|l| if let Logged::Line(l) = l { Some(l.text) } else { None })
            .collect();
        assert_eq!(texts, ["one", "two{nw}", "three{nw}", "four"]);
    }

    #[test]
    fn a_no_wait_line_before_a_choice_still_rolls_back_to_the_line_before() {
        let p = parse("**A:** one\n**A:** two{nw}\n::choice\n- [Go](#x)\n::\n# x\n**A:** end\n").unwrap();
        let (mut game, _) = Session::start(&p);
        game.input(Input::Advance); // "two{nw}"
        let cmds = game.input(Input::Advance); // the player (or the frontend) moves on
        assert!(cmds.iter().any(|c| matches!(c, Cmd::ShowChoices { .. })));
        assert_eq!(game.rollback().unwrap().last(), Some(&say("A", "one")));
    }

    #[test]
    fn hide_all_hides_every_sprite_and_unknown_hides_do_nothing() {
        let src = "::show{src=a.png id=a x=left}\n::show{src=b.png id=b x=right}\n::hide{ghost}\n::hide{all fade=1}\n**X:** hi\n";
        let cmds = run_to_end(src, &[]);
        let hidden: Vec<&Cmd> = cmds.iter().filter(|c| matches!(c, Cmd::HideSprite { .. })).collect();
        assert_eq!(hidden.len(), 2);
        assert!(hidden.iter().all(|c| matches!(c, Cmd::HideSprite { fade, .. } if *fade == 1.0)));
    }

    #[test]
    fn rollback_redraws_sprites_music_and_inline_images() {
        let src = "::bg{src=bg.png}\n::music{src=m.ogg volume=0.5}\n::show{src=s.png id=s x=center}\n**A:** one\n![alt](pic.png)\n::voice{src=v.ogg}\n**A:** two\n**A:** three\n";
        let p = parse(src).unwrap();
        let (mut game, _) = Session::start(&p);
        game.input(Input::Advance); // now on "two" with its image and voice
        assert_eq!(game.state().inline.len(), 1);
        assert_eq!(game.state().voice.as_deref(), Some("v.ogg"));
        game.input(Input::Advance); // on "three": image and voice are gone
        assert!(game.state().inline.is_empty() && game.state().voice.is_none());

        game.rollback(); // back to "two": everything about that moment returns
        let again = game.state().redraw();
        assert_eq!(again[0], Cmd::ClearScene);
        assert!(again.contains(&Cmd::SetBackground { src: "bg.png".into(), fade: 0.0, wipe: None }));
        assert!(again.iter().any(|c| matches!(c, Cmd::ShowSprite { id, fade, .. } if id == "s" && *fade == 0.0)));
        assert!(again.contains(&Cmd::PlayMusic { src: "m.ogg".into(), intro: None, fade: 0.0, looped: true, volume: 0.5 }));
        assert!(again.contains(&Cmd::ShowImage { src: "pic.png".into(), alt: "alt".into() }));
        assert!(again.contains(&Cmd::PlayVoice { src: "v.ogg".into() }));
        assert_eq!(again.last(), Some(&say("A", "two")));
    }

    #[test]
    fn music_state_survives_and_stop_clears_it() {
        let src = "::music{src=m.ogg}\n**A:** x\n::music{stop fade=1}\n**A:** y\n";
        let p = parse(src).unwrap();
        let (mut game, _) = Session::start(&p);
        assert!(game.state().music.is_some());
        game.input(Input::Advance);
        assert!(game.state().music.is_none());
    }

    #[test]
    fn sfx_is_fire_and_forget_and_not_replayed_on_redraw() {
        let p = parse("::sfx{src=door.ogg volume=0.5}\n**A:** x\n").unwrap();
        let (game, first) = Session::start(&p);
        assert_eq!(first[0], Cmd::PlaySfx { src: "door.ogg".into(), volume: 0.5 });
        assert!(!game.state().redraw().iter().any(|c| matches!(c, Cmd::PlaySfx { .. })));
    }

    #[test]
    fn wait_blocks_until_advanced() {
        let p = parse("**A:** one\n::wait{time=2}\n**A:** two\n").unwrap();
        let (mut game, _) = Session::start(&p);
        assert_eq!(game.input(Input::Advance), vec![Cmd::Wait { seconds: 2.0 }]);
        assert!(game.input(Input::Choose(0)).is_empty());
        assert_eq!(lines(&game.input(Input::Advance)), vec!["two"]);
        // the previous line stays on screen during a wait (redraw while waiting)
        let (mut game, _) = Session::start(&p);
        game.input(Input::Advance);
        let again = game.state().redraw();
        assert!(again.contains(&say("A", "one")) && again.contains(&Cmd::Wait { seconds: 2.0 }));
    }

    #[test]
    fn a_video_clears_the_line_and_backgrounds_may_be_video_files() {
        let p = parse("::bg{src=loop.webm}\n**A:** x\n::video{src=c.webm}\n**A:** y\n").unwrap();
        let (mut game, first) = Session::start(&p);
        assert!(first.contains(&Cmd::SetBackground { src: "loop.webm".into(), fade: 0.0, wipe: None }));
        game.input(Input::Advance);
        assert!(game.state().line.is_none());
    }

    // ---- variables and text -----------------------------------------------

    #[test]
    fn lines_and_choices_interpolate_variables() {
        let src = "---\nvars: { name: \"Alex\", n: 3, ok: true }\n---\n**A:** Hi {name}, {n} left, {ok}. {missing} \\{name} {name\n::choice\n- [Call {name}](#x)\n::\n# x\n**A:** bye\n";
        let p = parse(src).unwrap();
        let (_, first) = Session::start(&p);
        assert_eq!(first, vec![say("A", "Hi Alex, 3 left, true. {missing} \\{name} {name")]);
    }

    #[test]
    fn interpolation_helper_edge_cases() {
        let vars: Vars = [("a".to_string(), Value::Int(1))].into_iter().collect();
        assert_eq!(interpolate("no braces", &vars), "no braces");
        assert_eq!(interpolate("{a}{a}", &vars), "11");
        assert_eq!(interpolate("{ a }", &vars), "1");
        assert_eq!(interpolate("back\\slash", &vars), "back\\slash");
        assert_eq!(interpolate("\\\\{a}", &vars), "\\\\1", "an escaped backslash, then the value");
        assert_eq!(interpolate("\\{a}", &vars), "\\{a}", "the escape is the markup's to resolve");
        assert_eq!(quillstone_script::markup::plain(&interpolate("\\{a} {a}", &vars)), "{a} 1");
        assert_eq!(interpolate("é{a}é", &vars), "é1é");
    }

    #[test]
    fn text_variables_can_be_built_up() {
        let src = "::set{s=\"ab\"}\n::set{s+=\"cd\", n+=2}\n**A:** {s} {n}\n";
        assert_eq!(lines(&run_to_end(src, &[])), vec!["abcd 2"]);
    }

    // ---- visit counts and once-only options ---------------------------------

    #[test]
    fn seen_counts_each_pass_of_a_label() {
        let src = "# hub\n::choice\n- [Visit](#room)\n- [Leave](#out)\n::\n# room\n::if{seen(room) >= 2}\n**A:** Back again.\n::else\n**A:** First time.\n::endif\n::jump{to=hub}\n# out\n**A:** Bye.\n";
        let p = parse(src).unwrap();
        let (mut game, _) = Session::start(&p);
        let mut said = Vec::new();
        for _ in 0..2 {
            said.extend(lines(&game.input(Input::Choose(0))));
            said.extend(lines(&game.input(Input::Advance)));
        }
        assert_eq!(said, vec!["First time.", "Back again."]);
        assert_eq!(game.state().vars["seen:room"], int(2));
        assert!(!game.state().vars.contains_key("seen:hub"), "only labels a seen() asks about are counted");
        game.rollback();
        assert_eq!(game.state().vars["seen:room"], int(2), "rolled back to the second visit's line");
        let reloaded = Session::load(&p, &game.save()).unwrap().0;
        assert_eq!(reloaded.state().vars["seen:room"], int(2), "saves keep visit counts");
    }

    #[test]
    fn once_options_go_away_after_being_picked() {
        let src = "# hub\n::choice\n- [Ask](#ask){once}\n- [Leave](#out)\n::\n# ask\n**A:** Answer.\n::jump{to=hub}\n# out\n**A:** Bye.\n";
        let p = parse(src).unwrap();
        let (mut game, first) = Session::start(&p);
        let options = |cmds: &[Cmd]| cmds.iter().find_map(|c| match c {
            Cmd::ShowChoices { options, .. } => Some(options.clone()),
            _ => None,
        });
        assert_eq!(options(&first).unwrap(), ["Ask", "Leave"]);
        game.input(Input::Choose(0));
        assert_eq!(options(&game.input(Input::Advance)).unwrap(), ["Leave"]);
        game.rollback();
        game.rollback();
        assert_eq!(options(&game.state().redraw()).unwrap(), ["Ask", "Leave"], "rolling back before the pick brings it back");
    }

    // ---- hotspots ----------------------------------------------------------

    const MAP: &str = "---\nvars: { found: false }\n---\n# map\n::bg{src=map.png}\n::hotspots\n- [Library](#lib){rect=0.1,0.2,0.2,0.3, once}\n- [Key](#key){rect=0.6,0.7,0.05,0.08 image=key.png, found=true, if not found}\n- [Leave](#out){rect=0.8,0.8,0.1,0.1}\n::\n# lib\n**A:** Books.\n::jump{to=map}\n# key\n**A:** A key.\n::jump{to=map}\n# out\n**A:** Bye.\n";

    fn spots(cmds: &[Cmd]) -> Option<Vec<Spot>> {
        cmds.iter().find_map(|c| match c {
            Cmd::ShowHotspots { spots } => Some(spots.clone()),
            _ => None,
        })
    }

    fn labels(s: &[Spot]) -> Vec<&str> {
        s.iter().map(|s| s.label.as_str()).collect()
    }

    #[test]
    fn hotspots_are_shown_with_rects_and_visible_indices() {
        let p = parse(MAP).unwrap();
        let (mut game, first) = Session::start(&p);
        let all = spots(&first).unwrap();
        assert_eq!(labels(&all), ["Library", "Key", "Leave"]);
        assert_eq!(all[1].rect, [0.6, 0.7, 0.05, 0.08]);
        assert_eq!(all[1].image.as_deref(), Some("key.png"));
        assert_eq!(all[0].image, None);
        // the key: effects apply, and it is gone when the map comes back
        assert_eq!(lines(&game.input(Input::Choose(1))), vec!["A key."]);
        assert_eq!(game.state().vars["found"], Value::Bool(true));
        let back = game.input(Input::Advance);
        assert_eq!(labels(&spots(&back).unwrap()), ["Library", "Leave"]);
        // index 1 is now the second VISIBLE spot: Leave
        assert_eq!(lines(&game.input(Input::Choose(1))), vec!["Bye."]);
    }

    #[test]
    fn a_hotspot_screen_keeps_the_background_and_a_once_hotspot_goes_away() {
        let p = parse(MAP).unwrap();
        let (mut game, first) = Session::start(&p);
        assert!(first.contains(&Cmd::SetBackground { src: "map.png".into(), fade: 0.0, wipe: None }));
        game.input(Input::Choose(0));
        let back = game.input(Input::Advance);
        assert_eq!(labels(&spots(&back).unwrap()), ["Key", "Leave"]);
        assert!(game.backlog().iter().all(|l| !matches!(l, Logged::Choice(_))), "a click on the map isn't dialogue");
        game.rollback();
        game.rollback();
        assert_eq!(labels(&spots(&game.state().redraw()).unwrap()), ["Library", "Key", "Leave"]);
        assert!(matches!(game.state().pending, Some(Cmd::ShowHotspots { .. })));
    }

    #[test]
    fn hotspots_with_nothing_on_offer_are_skipped_and_bad_picks_ignored() {
        let src = "::hotspots\n- [Secret](#x){rect=0,0,1,1, if never}\n::\n# x\n**A:** carried on\n";
        assert_eq!(lines(&run_to_end(src, &[])), vec!["carried on"]);
        let p = parse(MAP).unwrap();
        let (mut game, _) = Session::start(&p);
        assert!(game.input(Input::Choose(9)).is_empty());
        assert!(game.input(Input::Advance).is_empty(), "advance doesn't pick a region");
    }

    #[test]
    fn hotspots_survive_a_save_and_load() {
        let p = parse(MAP).unwrap();
        let (mut game, _) = Session::start(&p);
        game.input(Input::Choose(1));
        game.input(Input::Advance); // back at the map, key gone
        let text = game.save();
        assert!(text.contains("pending hotspots"));
        let (loaded, cmds) = Session::load(&p, &text).unwrap();
        assert_eq!(loaded.state(), game.state());
        assert_eq!(labels(&spots(&cmds).unwrap()), ["Library", "Leave"]);
        assert_eq!(cmds, game.state().redraw());
    }

    #[test]
    fn the_hotspots_demo_plays_a_map_a_collectible_and_an_exit() {
        let p = parse(include_str!("../../../examples/hotspots.quill.md")).unwrap();
        let (mut game, first) = Session::start(&p);
        assert!(spots(&first).is_none(), "the intro line comes first");
        let map = game.input(Input::Advance);
        assert_eq!(labels(&spots(&map).unwrap()), ["The rooftop", "The old bridge", "The clock tower", "A scarf in the grass", "Leave town"]);
        game.input(Input::Choose(2)); // the tower (once)
        let map = game.input(Input::Advance);
        assert_eq!(labels(&spots(&map).unwrap()), ["The rooftop", "The old bridge", "A scarf in the grass", "Leave town"]);
        let scarf = game.input(Input::Choose(2));
        assert!(scarf.contains(&Cmd::ShowImage { src: "assets/scarf.png".into(), alt: "A scarf, left behind".into() }));
        let map = game.input(Input::Advance);
        assert_eq!(labels(&spots(&map).unwrap()), ["The rooftop", "The old bridge", "Leave town"]);
        game.input(Input::Choose(2));
        let end = game.input(Input::Advance);
        assert!(end.contains(&Cmd::Finished));
    }

    // ---- items, the inventory and shops ---------------------------------------

    const SHOP: &str = "---\ninventory: bag\nvars: { gold: 40, bag: [], met: false }\n---\n::item{id=key name=\"Brass key\" icon=key.png price=15 text=\"Opens it.\"}\n::item{id=lamp price=30}\n::item{id=map price=5}\n# shop\n**A:** Welcome.\n::shop{pay=gold into=bag}\n- key\n- lamp\n- map{price=2, if met}\n::\n**A:** Bye.\n::inventory\n**A:** Done.\n";

    fn shop_of(cmds: &[Cmd]) -> Option<(String, i64, Vec<ShopItem>)> {
        cmds.iter().find_map(|c| match c {
            Cmd::ShowShop { pay, money, items } => Some((pay.clone(), *money, items.clone())),
            _ => None,
        })
    }

    fn names(items: &[ShopItem]) -> Vec<(&str, i64, bool)> {
        items.iter().map(|i| (i.item.name.as_str(), i.price, i.owned)).collect()
    }

    fn at_shop() -> (Program, Vec<Cmd>) {
        let p = parse(SHOP).unwrap();
        let cmds = {
            let (mut game, _) = Session::start(&p);
            game.input(Input::Advance)
        };
        (p, cmds)
    }

    #[test]
    fn a_shop_waits_and_lists_what_is_on_offer_with_prices_and_ownership() {
        let (_, cmds) = at_shop();
        let (pay, money, items) = shop_of(&cmds).expect("a shop");
        assert_eq!((pay.as_str(), money), ("gold", 40));
        assert_eq!(names(&items), [("Brass key", 15, false), ("lamp", 30, false)], "the map needs `met`");
        assert_eq!(items[0].item.icon.as_deref(), Some("key.png"));
        assert_eq!(items[0].item.text, "Opens it.");
    }

    #[test]
    fn buying_pays_adds_the_item_and_shows_the_shop_again() {
        let p = parse(SHOP).unwrap();
        let (mut game, _) = Session::start(&p);
        game.input(Input::Advance);
        let again = game.input(Input::Buy(0));
        let (_, money, items) = shop_of(&again).expect("the shop is shown again");
        assert_eq!(money, 25);
        assert_eq!(names(&items), [("Brass key", 15, true), ("lamp", 30, false)]);
        assert_eq!(game.state().vars["gold"], int(25));
        assert_eq!(game.state().vars["bag"], Value::List(vec!["key".into()]));
        assert!(matches!(game.state().pending, Some(Cmd::ShowShop { .. })), "still waiting at the shop");
        assert_eq!(again.len(), 1, "only the shop is redrawn");
    }

    #[test]
    fn what_is_owned_or_unaffordable_cannot_be_bought_and_money_never_goes_negative() {
        let p = parse(SHOP).unwrap();
        let (mut game, _) = Session::start(&p);
        game.input(Input::Advance);
        assert!(game.input(Input::Buy(0)).len() == 1);
        assert!(game.input(Input::Buy(0)).is_empty(), "owned: a list is a set");
        assert!(game.input(Input::Buy(1)).is_empty(), "the lamp costs 30 and 25 are left");
        assert!(game.input(Input::Buy(7)).is_empty(), "no such item");
        assert_eq!(game.state().vars["gold"], int(25));
        assert_eq!(game.state().vars["bag"], Value::List(vec!["key".into()]));
        // exactly enough money is enough
        let (mut game, _) = Session::start(&p);
        game.input(Input::Advance);
        game.input(Input::Buy(0));
        game.state.vars.insert("gold".into(), int(30));
        assert!(!game.input(Input::Buy(1)).is_empty());
        assert_eq!(game.state().vars["gold"], int(0));
    }

    #[test]
    fn a_condition_that_becomes_true_adds_the_item_at_its_own_price_and_indices_follow_the_list() {
        let p = parse(SHOP).unwrap();
        let (mut game, _) = Session::start(&p);
        game.input(Input::Advance);
        game.state.vars.insert("met".into(), Value::Bool(true));
        let again = game.input(Input::Buy(2)); // the map, third in the offered list: 2 gold
        assert_eq!(game.state().vars["gold"], int(38));
        assert_eq!(names(&shop_of(&again).unwrap().2)[2], ("map", 2, true));
    }

    #[test]
    fn leaving_continues_after_the_block_and_the_inventory_waits_for_advance() {
        let p = parse(SHOP).unwrap();
        let (mut game, _) = Session::start(&p);
        game.input(Input::Advance);
        game.input(Input::Buy(0));
        assert_eq!(lines(&game.input(Input::Advance)), vec!["Bye."]);
        let inv = game.input(Input::Advance);
        assert_eq!(inv, vec![Cmd::ShowInventory]);
        assert!(game.input(Input::Choose(0)).is_empty() && game.input(Input::Buy(0)).is_empty());
        assert_eq!(lines(&game.input(Input::Advance)), vec!["Done."]);
    }

    #[test]
    fn a_shop_with_nothing_on_offer_is_skipped() {
        let src = "---\nvars: { gold: 1, bag: [], never: false }\n---\n::item{id=a price=1}\n::shop{pay=gold into=bag}\n- a{if never}\n::\n**A:** past it\n";
        assert_eq!(lines(&run_to_end(src, &[])), vec!["past it"]);
    }

    #[test]
    fn rolling_back_takes_a_purchase_back_and_roll_forward_redoes_it() {
        let p = parse(SHOP).unwrap();
        let (mut game, _) = Session::start(&p);
        game.input(Input::Advance);
        game.input(Input::Buy(0));
        game.input(Input::Buy(1)); // refused: too dear
        let back = game.rollback().expect("one step back");
        assert_eq!(game.state().vars["gold"], int(40), "the purchase is undone");
        assert_eq!(game.state().vars["bag"], Value::List(Vec::new()));
        assert_eq!(names(&shop_of(&back).unwrap().2), [("Brass key", 15, false), ("lamp", 30, false)]);
        let fwd = game.roll_forward().expect("and forward again");
        assert_eq!(game.state().vars["gold"], int(25));
        assert_eq!(names(&shop_of(&fwd).unwrap().2)[0], ("Brass key", 15, true));
        assert!(game.backlog().iter().all(|l| !matches!(l, Logged::Choice(_))), "a purchase isn't dialogue");
    }

    #[test]
    fn a_shop_and_the_inventory_survive_a_save_and_load() {
        let p = parse(SHOP).unwrap();
        let (mut game, _) = Session::start(&p);
        game.input(Input::Advance);
        game.input(Input::Buy(0));
        let text = game.save();
        assert!(text.contains("pending shop") && text.contains("input buy 0"));
        let (mut loaded, cmds) = Session::load(&p, &text).unwrap();
        assert_eq!(loaded.state(), game.state());
        assert_eq!(cmds, game.state().redraw());
        assert_eq!(names(&shop_of(&cmds).unwrap().2)[0], ("Brass key", 15, true));
        loaded.input(Input::Buy(1)); // too dear, nothing changes
        assert_eq!(loaded.state().vars["gold"], int(25));
        assert!(loaded.rollback().is_some(), "the purchase is still in the loaded history");
        assert_eq!(loaded.state().vars["gold"], int(40));
        // and the inventory
        loaded.input(Input::Advance);
        loaded.input(Input::Advance);
        let (back, cmds) = Session::load(&p, &loaded.save()).unwrap();
        assert_eq!(cmds.last(), Some(&Cmd::ShowInventory));
        assert!(loaded.save().contains("pending inventory") && back.state().pending == Some(Cmd::ShowInventory));
    }

    const QUESTS: &str = "---\njournal: quests\njournal_done: done\nvars: { quests: [cat, key, ghost], done: [], n: 2 }\n---\n\
        ::item{id=cat name=\"Find the cat\" text=\"Last seen on the roof.\" price=9 count=n use=#x}\n\
        ::item{id=key name=\"Get the key\" icon=key.png}\n# a\nHi.\n::journal\nBye.\n# x\nHi.\n::return\n";

    #[test]
    fn the_journal_lists_open_entries_then_done_ones_without_prices_counts_or_use() {
        let p = parse(QUESTS).unwrap();
        let mut vars = p.vars.clone();
        let shown = |vars: &Vars| journal_items(&p, vars);
        let plain = |(i, _): &(ItemInfo, bool)| i.price.is_none() && i.count.is_none() && !i.has_use;
        assert!(shown(&vars).iter().all(plain), "the cat has a price, a count and a use label, none of which show");
        assert_eq!(shown(&vars).into_iter().map(|(i, d)| (i.name, d)).collect::<Vec<_>>(), [("Find the cat".into(), false), ("Get the key".into(), false), ("ghost".into(), false)]);
        assert_eq!(journal_items(&p, &vars)[0].0.text, "Last seen on the roof.");
        // marking one done moves it to the end, even while it is still in the first list
        vars.insert("done".into(), Value::List(vec!["cat".into()]));
        let names: Vec<_> = shown(&vars).into_iter().map(|(i, d)| (i.name, d)).collect();
        assert_eq!(names, [("Get the key".into(), false), ("ghost".into(), false), ("Find the cat".into(), true)]);
        assert!(inventory_items(&p, &vars).is_empty(), "the inventory never shows the journal's list");
        let none = parse("---\nvars: { quests: [x] }\n---\n").unwrap();
        assert!(journal_items(&none, &none.vars).is_empty());
    }

    #[test]
    fn the_journal_command_waits_for_advance_and_survives_a_save_and_load() {
        let p = parse(QUESTS).unwrap();
        let (mut game, _) = Session::start(&p);
        assert_eq!(game.input(Input::Advance), vec![Cmd::ShowJournal]);
        let text = game.save();
        assert!(text.contains("pending journal"));
        let (mut back, cmds) = Session::load(&p, &text).unwrap();
        assert_eq!((cmds.last(), back.state().pending.clone()), (Some(&Cmd::ShowJournal), Some(Cmd::ShowJournal)));
        assert!(back.input(Input::Choose(0)).is_empty());
        assert_eq!(lines(&back.input(Input::Advance)), vec!["Bye."]);
    }

    #[test]
    fn a_shop_save_whose_script_changed_underneath_still_loads() {
        let p = parse(SHOP).unwrap();
        let (mut game, _) = Session::start(&p);
        game.input(Input::Advance);
        game.input(Input::Buy(0));
        let text = game.save();
        // the key got a new price and the lamp was dropped: the screen is rebuilt from the new script
        let edited = SHOP.replace("price=15", "price=10").replace("- lamp\n", "");
        let p2 = parse(&edited).unwrap();
        let (_, cmds) = Session::load(&p2, &text).unwrap();
        assert_eq!(names(&shop_of(&cmds).unwrap().2), [("Brass key", 10, true)]);
    }

    #[test]
    fn the_inventory_lists_what_the_list_holds_and_unknown_items_show_by_id() {
        let p = parse(SHOP).unwrap();
        let mut vars = p.vars.clone();
        assert!(inventory_items(&p, &vars).is_empty());
        vars.insert("bag".into(), Value::List(vec!["lamp".into(), "mystery".into(), "key".into()]));
        let shown: Vec<(String, Option<String>)> =
            inventory_items(&p, &vars).into_iter().map(|i| (i.name, i.icon)).collect();
        assert_eq!(
            shown,
            [("lamp".into(), None), ("mystery".into(), None), ("Brass key".into(), Some("key.png".into()))]
        );
        let no_inventory = parse("---\nvars: { bag: [x] }\n---\n").unwrap();
        assert!(inventory_items(&no_inventory, &no_inventory.vars).is_empty());
    }

    // ---- the demo ----------------------------------------------------------

    #[test]
    fn the_demo_plays_through_with_the_right_endings() {
        let program = parse(include_str!("../../../examples/demo.quill.md")).unwrap();

        let play = |picks: &[usize]| -> Vec<String> {
            let (mut game, first) = Session::start(&program);
            let mut said = lines(&first);
            let mut picks = picks.iter();
            for _ in 0..200 {
                if game.state().finished {
                    break;
                }
                let cmds = match game.state().pending {
                    Some(Cmd::ShowChoices { .. }) => match picks.next() {
                        Some(i) => game.input(Input::Choose(*i)),
                        None => break,
                    },
                    _ => game.input(Input::Advance),
                };
                said.extend(lines(&cmds));
            }
            said
        };

        // Tell the truth (+1): "honest" ending. The flashback set `met`, so there are 3 options.
        let truth = play(&[0]);
        assert!(truth.contains(&"Remember that day on the rooftop? Her voice is quieter now.".to_string()));
        assert!(truth.contains(&"Thank you.".to_string()));
        assert!(truth.contains(&"Thank you for being honest.".to_string()));
        // Lie (-1): cold ending
        assert!(play(&[1]).contains(&"I guess I expected that.".to_string()));
        // Tell her everything (+2): the best ending (only offered because `met`)
        let best = play(&[2]);
        assert!(best.contains(&"I'm glad it was you. I really am.".to_string()));
        let lights = "Below, the lights you watched together are still burning.".to_string();
        assert!(!best.contains(&lights), "only for those who looked at the city");
        // Look at the city first (`once`, then gone; `seen(city)` at the end), then tell the truth.
        let city = play(&[3, 0]);
        assert!(city.contains(&"The city hums below, a thousand windows still awake.".to_string()));
        assert!(city.contains(&"Thank you for being honest.".to_string()) && city.contains(&lights));
    }

    // ---- list variables ----

    fn list(items: &[&str]) -> Value {
        Value::List(items.iter().map(|s| s.to_string()).collect())
    }

    const BAG: &str = "\
---
vars: { inventory: [], item: \"lamp\" }
---
# start
Bag: [{inventory}]
::choice
- [Take map](#next){inventory += \"map\"}
- [Take key twice](#next){inventory += \"key\", inventory += 'key'}
::
# next
Now: [{inventory}]
::set{inventory += item}
::set{inventory += \"map\"}
Then: [{inventory}]
::set{inventory -= \"map\"}
::set{inventory -= \"never had it\"}
::if{inventory has \"lamp\" and not inventory has \"map\"}
Lamp, no map.
::endif
After: [{inventory}]
";

    #[test]
    fn a_list_variable_adds_removes_and_shows_in_order() {
        let p = parse(BAG).unwrap();
        let (mut s, first) = Session::start(&p);
        assert_eq!(lines(&first), ["Bag: []"], "an empty list shows nothing");
        s.input(Input::Advance);
        let now = s.input(Input::Choose(1));
        assert_eq!(lines(&now), ["Now: [key]"], "adding twice adds once");
        let then = s.input(Input::Advance);
        assert_eq!(lines(&then), ["Then: [key, lamp, map]"], "insertion order; a text variable works");
        let after = s.input(Input::Advance);
        assert_eq!(lines(&after), ["Lamp, no map."]);
        assert_eq!(lines(&s.input(Input::Advance)), ["After: [key, lamp]"]);
        assert_eq!(s.state().vars["inventory"], list(&["key", "lamp"]));
    }

    #[test]
    fn rollback_restores_a_list() {
        let p = parse(BAG).unwrap();
        let (mut s, _) = Session::start(&p);
        s.input(Input::Advance);
        s.input(Input::Choose(0));
        s.input(Input::Advance);
        assert_eq!(s.state().vars["inventory"], list(&["map", "lamp"]));
        s.rollback().unwrap();
        assert_eq!(s.state().vars["inventory"], list(&["map"]));
        s.rollback().unwrap();
        assert_eq!(s.state().vars["inventory"], list(&[]), "back at the choice");
        s.input(Input::Choose(1));
        assert_eq!(s.state().vars["inventory"], list(&["key"]), "a different branch starts from the restored list");
    }

    #[test]
    fn a_list_is_part_of_the_state_that_clones_independently() {
        let p = parse(BAG).unwrap();
        let (mut s, _) = Session::start(&p);
        s.input(Input::Advance);
        let mut a = s.state().clone();
        let b = s.state().clone();
        step(&mut a, &p, Some(Input::Choose(0)));
        assert_eq!(b.vars["inventory"], list(&[]));
        assert_eq!(a.vars["inventory"], list(&["map"]));
    }

    /// Press Advance until a menu is up; the lines passed on the way and the menu.
    fn to_menu(game: &mut Session, mut cmds: Vec<Cmd>) -> (Vec<String>, Vec<String>) {
        let mut said = Vec::new();
        for _ in 0..20 {
            said.extend(lines(&cmds));
            if let Some(options) = cmds.iter().find_map(|c| match c {
                Cmd::ShowChoices { options, .. } => Some(options.clone()),
                _ => None,
            }) {
                return (said, options);
            }
            cmds = game.input(Input::Advance);
        }
        panic!("no menu after {said:?}");
    }

    #[test]
    fn the_shop_demo_sells_each_item_once() {
        let p = parse(include_str!("../../../examples/shop.quill.md")).unwrap();
        let (mut game, first) = Session::start(&p);
        let (said, menu) = to_menu(&mut game, first);
        // the parameterized `price_line` subroutine is called twice, with different values
        assert_eq!(said[2..4], ["The brass key costs 15 gold.", "The lamp costs 10 gold."]);
        assert_eq!(said[4..], ["You have 30 gold.", "Your bag is empty."]);
        assert_eq!(menu, ["Buy the brass key (15 gold)", "Buy the lamp (10 gold)", "Buy the old map (10 gold)", "Leave the shop"]);
        let bought = game.input(Input::Choose(0)); // the key
        let (said, menu) = to_menu(&mut game, bought);
        assert_eq!(said, ["\"A fine choice,\" says the keeper.", "You have 15 gold.", "Your bag holds: key."]);
        assert_eq!(menu, ["Buy the lamp (10 gold)", "Buy the old map (10 gold)", "Leave the shop"], "no second key");
        let bought = game.input(Input::Choose(1)); // the map
        let (said, menu) = to_menu(&mut game, bought);
        assert_eq!(said[1..], ["You have 5 gold.", "Your bag holds: key, map."]);
        assert_eq!(menu, ["Sell the map back (5 gold)", "Leave the shop"], "too poor for the lamp");
        let sold = game.input(Input::Choose(0));
        let (said, menu) = to_menu(&mut game, sold);
        assert_eq!(said[1..], ["You have 10 gold.", "Your bag holds: key."]);
        assert_eq!(menu, ["Buy the lamp (10 gold)", "Buy the old map (10 gold)", "Leave the shop"]);
        let mut end = game.input(Input::Choose(2));
        let mut said = lines(&end);
        while !game.state().finished {
            end = game.input(Input::Advance);
            said.extend(lines(&end));
        }
        assert!(said[0].starts_with("Outside, the key turns"), "{said:?}");
        assert_eq!(said.len(), 1, "no lamp and map line");
    }

    #[test]
    fn leaving_the_shop_empty_handed_changes_the_text() {
        let p = parse(include_str!("../../../examples/shop.quill.md")).unwrap();
        let (mut game, first) = Session::start(&p);
        to_menu(&mut game, first);
        let end = game.input(Input::Choose(3));
        assert!(lines(&end)[0].starts_with("Outside, the old gate is locked"));
    }

    // ---- labels with parameters ---------------------------------------------

    const HEAL: &str = "\
---
vars: { amount: 99 }
---
# start
::call{to=heal amount=5 who=\"Mika\"}
**A:** back with {amount}
::call{to=heal amount=7 who=\"Ren\"}
**A:** and again
::end
# heal(amount, who)
**A:** {who} heals {amount}
::return
";

    #[test]
    fn call_arguments_become_ordinary_variables_before_the_jump() {
        let p = parse(HEAL).unwrap();
        let (mut game, first) = Session::start(&p);
        assert_eq!(first, vec![say("A", "Mika heals 5")], "the subroutine sees its values at once");
        assert_eq!(game.state().vars["amount"], int(5));
        assert_eq!(game.state().vars["who"], Value::Str("Mika".into()));
        // `::return` doesn't restore anything: the values stay, like any variable
        assert_eq!(game.input(Input::Advance), vec![say("A", "back with 5")]);
        assert_eq!(game.input(Input::Advance), vec![say("A", "Ren heals 7")]);
        game.input(Input::Advance);
        assert_eq!(game.state().vars["amount"], int(7));
        assert!(game.state().stack.is_empty());
    }

    #[test]
    fn arguments_read_variables_and_are_all_worked_out_before_any_is_stored() {
        let src = "---\nvars: { a: 1, b: 2 }\n---\n::call{to=f a=b b=a}\n::end\n# f(a, b)\n**X:** {a} {b}\n::return\n";
        let p = parse(src).unwrap();
        let (_, first) = Session::start(&p);
        assert_eq!(first, vec![say("X", "2 1")], "a swap, not a copy");
    }

    #[test]
    fn rollback_undoes_the_parameters_with_the_call() {
        let p = parse(HEAL).unwrap();
        let (mut game, _) = Session::start(&p);
        game.input(Input::Advance); // back with 5
        game.input(Input::Advance); // Ren heals 7
        assert_eq!(game.state().vars["amount"], int(7));
        let redraw = game.rollback().unwrap();
        assert_eq!(game.state().vars["amount"], int(5));
        assert_eq!(redraw.last(), Some(&say("A", "back with 5")));
        game.rollback();
        assert_eq!(game.state().vars["amount"], int(5), "inside the first call the parameter is 5");
        assert_eq!(game.state().stack.len(), 1);
        assert!(!game.can_rollback());
    }

    #[test]
    fn a_save_inside_a_parameterized_call_returns_with_its_values() {
        let p = parse(HEAL).unwrap();
        let (mut game, _) = Session::start(&p);
        game.input(Input::Advance);
        game.input(Input::Advance); // inside the second call
        let (mut loaded, cmds) = Session::load(&p, &game.save()).unwrap();
        assert_eq!(loaded.state(), game.state());
        assert_eq!(cmds.last(), Some(&say("A", "Ren heals 7")), "the interpolation still reads the variable");
        assert_eq!(loaded.state().stack.len(), 1);
        assert_eq!(loaded.input(Input::Advance), vec![say("A", "and again")]);
    }

    #[test]
    fn the_parenthesis_in_a_heading_changes_no_anchor_or_line_id() {
        let plain = parse("::call{to=heal}\n::end\n# heal\n**A:** hi\n::return\n").unwrap();
        let params = parse("::call{to=heal x=1}\n::end\n# heal(x)\n**A:** hi\n::return\n").unwrap();
        let at = plain.labels["heal"];
        assert_eq!(params.labels["heal"], at, "same op indices: a call adds no op");
        assert_eq!(line_id(&plain, at), line_id(&params, at), "the hash uses the label id, not the heading");
        assert_eq!(voice_name(&params, at).as_deref().map(|n| n.starts_with("heal-")), Some(true));
        // a save from before the parameters were added still lands in the same place
        let (mut game, _) = Session::start(&plain);
        game.input(Input::Advance);
        let (loaded, _) = Session::load(&params, &game.save()).unwrap();
        assert_eq!(loaded.state().pc, game.state().pc);
    }

    // ---- ::notify ------------------------------------------------------------

    #[test]
    fn a_notice_is_a_moment_with_its_values_filled_in() {
        let p = parse("---\nvars: { n: 2 }\n---\n::notify{text=\"Got {n}\" time=1}\n**A:** one\n**A:** two\n").unwrap();
        let (mut game, first) = Session::start(&p);
        assert_eq!(first[0], Cmd::Notify { text: "Got 2".into(), time: 1.0, source: "Got {n}".into() });
        assert!(!game.state().redraw().iter().any(|c| matches!(c, Cmd::Notify { .. })));
        game.input(Input::Advance);
        let back = game.rollback().unwrap();
        assert!(!back.iter().any(|c| matches!(c, Cmd::Notify { .. })), "rollback doesn't show it again");
    }

    // ---- ::flash -------------------------------------------------------------

    #[test]
    fn a_flash_is_a_moment_like_a_shake() {
        let p = parse("::flash{time=0.5 color=#ff0000}\n**A:** one\n**A:** two\n").unwrap();
        let (mut game, first) = Session::start(&p);
        assert_eq!(first[0], Cmd::Flash { time: 0.5, color: [1.0, 0.0, 0.0] });
        assert!(!game.state().redraw().iter().any(|c| matches!(c, Cmd::Flash { .. })));
        game.input(Input::Advance);
        let back = game.rollback().unwrap();
        assert!(!back.iter().any(|c| matches!(c, Cmd::Flash { .. })), "rollback doesn't flash again");
        let (loaded, cmds) = Session::load(&p, &game.save()).unwrap();
        assert!(!cmds.iter().any(|c| matches!(c, Cmd::Flash { .. })) && loaded.state().pc == game.state().pc);
    }

    // ---- looping sound effects ------------------------------------------------

    const RAIN: &str = "\
::sfx{src=rain.ogg loop volume=0.6 fade=1}
**A:** one
::sfx{src=rain.ogg loop volume=0.3}
::sfx{src=wind.ogg loop}
**A:** two
::sfx{stop=rain.ogg fade=2}
**A:** three
::sfx{stop}
**A:** four
";

    fn loops<'a>(game: &'a Session<'_>) -> Vec<(&'a str, f32)> {
        game.state().sfx_loops.iter().map(|l| (l.src.as_str(), l.volume)).collect()
    }

    #[test]
    fn a_looping_sfx_is_part_of_the_state_until_it_is_stopped() {
        let p = parse(RAIN).unwrap();
        let (mut game, first) = Session::start(&p);
        assert_eq!(first[0], Cmd::PlaySfxLoop { src: "rain.ogg".into(), fade: 1.0, volume: 0.6 });
        assert_eq!(loops(&game), [("rain.ogg", 0.6)]);
        game.input(Input::Advance);
        assert_eq!(loops(&game), [("rain.ogg", 0.3), ("wind.ogg", 1.0)], "again: same loop, new volume; order kept");
        let cmds = game.input(Input::Advance);
        assert_eq!(cmds[0], Cmd::StopSfx { src: Some("rain.ogg".into()), fade: 2.0 });
        assert_eq!(loops(&game), [("wind.ogg", 1.0)]);
        game.input(Input::Advance);
        assert!(loops(&game).is_empty(), "`stop` stops everything");
    }

    #[test]
    fn rollback_and_redraw_bring_the_loops_of_that_moment_back() {
        let p = parse(RAIN).unwrap();
        let (mut game, _) = Session::start(&p);
        game.input(Input::Advance);
        game.input(Input::Advance); // "three": rain stopped, wind going
        let redraw = game.rollback().unwrap(); // "two": rain 0.3 and wind
        assert_eq!(
            redraw.iter().filter(|c| matches!(c, Cmd::PlaySfxLoop { .. })).collect::<Vec<_>>(),
            [
                &Cmd::PlaySfxLoop { src: "rain.ogg".into(), fade: 0.0, volume: 0.3 },
                &Cmd::PlaySfxLoop { src: "wind.ogg".into(), fade: 0.0, volume: 1.0 },
            ]
        );
        // a one-shot is still never replayed
        let p = parse("::sfx{src=door.ogg}\n**A:** x\n").unwrap();
        let (game, _) = Session::start(&p);
        assert!(!game.state().redraw().iter().any(|c| matches!(c, Cmd::PlaySfx { .. } | Cmd::PlaySfxLoop { .. })));
    }

    #[test]
    fn loops_are_saved_and_an_old_save_loads_with_none() {
        let p = parse(RAIN).unwrap();
        let (mut game, _) = Session::start(&p);
        game.input(Input::Advance);
        let text = game.save();
        assert!(text.contains("sfxloop \"rain.ogg\" 0.3") && text.contains("sfxloop \"wind.ogg\" 1"), "{text}");
        let (loaded, cmds) = Session::load(&p, &text).unwrap();
        assert_eq!(loops(&loaded), loops(&game));
        assert_eq!(loaded.state(), game.state());
        assert!(cmds.contains(&Cmd::PlaySfxLoop { src: "wind.ogg".into(), fade: 0.0, volume: 1.0 }));
        // history entries keep theirs too: roll back into the first line and the rain is 0.6
        let (mut loaded, _) = Session::load(&p, &text).unwrap();
        loaded.rollback();
        assert_eq!(loops(&loaded), [("rain.ogg", 0.6)]);

        // a save from before loops existed has no such record
        let old: String = text.lines().filter(|l| !l.starts_with("sfxloop")).map(|l| format!("{l}\n")).collect();
        let (loaded, cmds) = Session::load(&p, &old).unwrap();
        assert!(loops(&loaded).is_empty());
        assert!(!cmds.iter().any(|c| matches!(c, Cmd::PlaySfxLoop { .. })));
        // an unsafe path in one is refused like any other
        let bad = text.replace("\"wind.ogg\"", "\"../wind.ogg\"");
        assert!(Session::load(&p, &bad).err().unwrap().contains("unsafe file path"));
    }

    // ---- ::textbox -----------------------------------------------------------------

    const HIDDEN: &str = "**A:** one\n::textbox{hide fade=0.5}\n::wait{time=1}\n**A:** two\n::textbox{hide}\n::choice\n- [Go](#end)\n::\n# end\n::textbox{hide}\n::hotspots\n- [Map](#back){rect=0,0,1,1}\n::\n# back\n**A:** back\n";

    #[test]
    fn textbox_hide_is_scene_state_and_redraw_emits_it_again() {
        let p = parse(HIDDEN).unwrap();
        let (mut game, _) = Session::start(&p);
        assert!(!game.state().textbox_hidden);
        let cmds = game.input(Input::Advance);
        assert!(cmds.contains(&Cmd::Textbox { hidden: true, fade: 0.5 }));
        assert!(game.state().textbox_hidden);
        let redraw = game.state().redraw();
        assert_eq!(redraw[0], Cmd::ClearScene);
        assert!(redraw.contains(&Cmd::Textbox { hidden: true, fade: 0.0 }));
        // rolling back to the first line restores the shown box
        let back = game.rollback().unwrap();
        assert!(!game.state().textbox_hidden);
        assert!(!back.iter().any(|c| matches!(c, Cmd::Textbox { .. })));
    }

    #[test]
    fn a_line_or_a_choice_shows_the_box_again_but_a_hotspot_map_does_not() {
        let p = parse(HIDDEN).unwrap();
        let (mut game, _) = Session::start(&p);
        game.input(Input::Advance); // hide, wait
        let cmds = game.input(Input::Advance); // wait ends: line "two" arrives
        let at = |c: &Cmd| cmds.iter().position(|x| x == c).unwrap();
        let say = cmds.iter().position(|c| matches!(c, Cmd::Say { .. })).unwrap();
        assert!(at(&Cmd::Textbox { hidden: false, fade: 0.0 }) < say, "shown before the line");
        assert!(!game.state().textbox_hidden);
        let cmds = game.input(Input::Advance); // hide, then choices
        assert!(cmds.contains(&Cmd::Textbox { hidden: true, fade: 0.0 }));
        assert!(cmds.contains(&Cmd::Textbox { hidden: false, fade: 0.0 }), "choices bring the box back");
        assert!(!game.state().textbox_hidden);
        let cmds = game.input(Input::Choose(0)); // hide, hotspots
        assert!(matches!(game.state().pending, Some(Cmd::ShowHotspots { .. })));
        assert!(game.state().textbox_hidden, "a map stays clean");
        assert!(!cmds.contains(&Cmd::Textbox { hidden: false, fade: 0.0 }));
        let cmds = game.input(Input::Choose(0));
        assert!(cmds.contains(&Cmd::Textbox { hidden: false, fade: 0.0 }), "the next line shows it");
    }

    #[test]
    fn textbox_show_command_shows_it() {
        let p = parse("::textbox{hide}\n::textbox{show fade=1}\n::wait{time=1}\n").unwrap();
        let (game, cmds) = Session::start(&p);
        assert!(cmds.contains(&Cmd::Textbox { hidden: false, fade: 1.0 }));
        assert!(!game.state().textbox_hidden);
    }

    // ---- ::fade (dip to colour) ------------------------------------------------

    const DIP: &str = "**A:** before\n::fade{out time=0.5 color=#ff0000}\n**A:** in the dark\n::fade{in time=0.25}\n**A:** after\n";

    #[test]
    fn fade_out_covers_the_stage_and_waits_like_a_wait() {
        let p = parse(DIP).unwrap();
        let (mut game, _) = Session::start(&p);
        assert_eq!(game.state().cover, None);
        let cmds = game.input(Input::Advance);
        assert_eq!(cmds[0], Cmd::FadeOut { time: 0.5, color: [1.0, 0.0, 0.0] });
        assert_eq!(cmds[1], Cmd::Wait { seconds: 0.5 }, "the fade waits");
        assert_eq!(game.state().cover, Some([1.0, 0.0, 0.0]));
        assert!(moves_on_by_itself(&game.state().pending));
        // the wait ends (or a click or skip ends it): the line over the black screen arrives
        let cmds = game.input(Input::Advance);
        assert!(matches!(cmds.last(), Some(Cmd::Say { text, .. }) if text == "in the dark"));
        assert_eq!(game.state().cover, Some([1.0, 0.0, 0.0]), "it stays covered");
        // fade in does not wait
        let cmds = game.input(Input::Advance);
        assert_eq!(cmds[0], Cmd::FadeIn { time: 0.25 });
        assert!(matches!(cmds.last(), Some(Cmd::Say { text, .. }) if text == "after"));
        assert_eq!(game.state().cover, None);
    }

    #[test]
    fn fade_in_with_nothing_covering_does_nothing() {
        let p = parse("::fade{in}\n**A:** hi\n").unwrap();
        let (game, first) = Session::start(&p);
        assert!(!first.iter().any(|c| matches!(c, Cmd::FadeIn { .. })));
        assert_eq!(game.state().cover, None);
    }

    #[test]
    fn the_cover_is_scene_state_redraw_emits_it_and_rollback_skips_the_fade_wait() {
        let p = parse(DIP).unwrap();
        let (mut game, _) = Session::start(&p);
        game.input(Input::Advance); // fade out: waiting
        game.input(Input::Advance); // "in the dark"
        let redraw = game.state().redraw();
        assert_eq!(redraw[0], Cmd::ClearScene);
        assert!(redraw.contains(&Cmd::FadeOut { time: 0.0, color: [1.0, 0.0, 0.0] }), "redraw covers at once");
        // back from the dark line goes past the fade's wait, to the line before it, uncovered
        let back = game.rollback().unwrap();
        assert!(matches!(game.state().pending, Some(Cmd::Say { .. })));
        assert_eq!(game.state().cover, None);
        assert!(!back.iter().any(|c| matches!(c, Cmd::FadeOut { .. })));
        // and forward again lands on the dark line, covered
        let fwd = game.roll_forward().unwrap();
        assert!(fwd.iter().any(|c| matches!(c, Cmd::FadeOut { time, .. } if *time == 0.0)));
        assert!(matches!(&game.state().pending, Some(Cmd::Say { text, .. }) if text == "in the dark"));
    }

    #[test]
    fn the_cover_is_saved_and_old_saves_load_uncovered() {
        let p = parse(DIP).unwrap();
        let (mut game, _) = Session::start(&p);
        game.input(Input::Advance);
        game.input(Input::Advance);
        let text = game.save();
        assert!(text.contains("cover 1,0,0"), "{text}");
        let (loaded, cmds) = Session::load(&p, &text).unwrap();
        assert_eq!(loaded.state().cover, Some([1.0, 0.0, 0.0]));
        assert!(cmds.contains(&Cmd::FadeOut { time: 0.0, color: [1.0, 0.0, 0.0] }));
        let old: String = text.lines().filter(|l| !l.starts_with("cover ")).collect::<Vec<_>>().join("\n") + "\n";
        assert_eq!(Session::load(&p, &old).unwrap().0.state().cover, None);
        // a save made while the fade-out itself was waiting loads onto the wait
        let (mut mid, _) = Session::start(&p);
        mid.input(Input::Advance);
        let (back, _) = Session::load(&p, &mid.save()).unwrap();
        assert_eq!(back.state().pending, Some(Cmd::Wait { seconds: 0.5 }));
        assert_eq!(back.state().cover, Some([1.0, 0.0, 0.0]));
        // nonsense colours are pulled into range
        let wild = text.replace("cover 1,0,0", "cover 9,NaN,-3");
        assert_eq!(Session::load(&p, &wild).unwrap().0.state().cover, Some([1.0, 0.0, 0.0]));
    }

    #[test]
    fn bg_through_dips_to_the_colour_changes_the_picture_and_comes_back() {
        let p = parse("::bg{src=a.png}\n**A:** one\n::bg{src=b.png fade=1 through=#000000}\n**A:** two\n").unwrap();
        let (mut game, _) = Session::start(&p);
        let cmds = game.input(Input::Advance);
        assert_eq!(cmds, vec![Cmd::FadeOut { time: 0.5, color: [0.0; 3] }, Cmd::Wait { seconds: 0.5 }]);
        assert_eq!(game.state().background.as_deref(), Some("a.png"), "the picture changes after the fade out");
        let cmds = game.input(Input::Advance);
        assert_eq!(cmds[0], Cmd::SetBackground { src: "b.png".into(), fade: 0.0, wipe: None });
        assert_eq!(cmds[1], Cmd::FadeIn { time: 0.5 });
        assert!(matches!(cmds[2], Cmd::Say { .. }));
        assert_eq!(game.state().cover, None);
    }

    // ---- sprite flip and per-axis scale ------------------------------------------

    #[test]
    fn flip_xscale_and_yscale_are_sprite_state_that_later_shows_keep() {
        let p = parse("::show{src=s.png id=s flip xscale=1.5 yscale=0.8}\n::show{src=s2.png id=s}\n::show{src=s2.png id=s flip=false xscale=2}\n**A:** hi\n").unwrap();
        let (game, cmds) = Session::start(&p);
        let shows: Vec<_> = cmds.iter().filter(|c| matches!(c, Cmd::ShowSprite { .. })).collect();
        assert!(matches!(shows[0], Cmd::ShowSprite { flip: true, xscale, yscale, .. } if *xscale == 1.5 && *yscale == 0.8));
        assert!(matches!(shows[1], Cmd::ShowSprite { flip: true, xscale, yscale, .. } if *xscale == 1.5 && *yscale == 0.8), "kept");
        let s = &game.state().sprites[0];
        assert_eq!((s.flip, s.xscale, s.yscale), (false, 2.0, 0.8), "given values change, the rest is kept");
        // a new sprite starts unflipped and unstretched
        let q = parse("::show{src=s.png id=s}\n**A:** hi\n").unwrap();
        let (g, _) = Session::start(&q);
        assert_eq!((g.state().sprites[0].flip, g.state().sprites[0].xscale, g.state().sprites[0].yscale), (false, 1.0, 1.0));
    }

    #[test]
    fn move_resolves_flip_and_the_scales_from_the_sprite() {
        let p = parse("::show{src=s.png id=s xscale=2}\n::move{s flip time=1}\n::move{s yscale=0.5 time=2 ease=out}\n::move{s x=left time=0}\n**A:** hi\n").unwrap();
        let (game, cmds) = Session::start(&p);
        let moves: Vec<_> = cmds.iter().filter_map(|c| if let Cmd::MoveSprite { flip, xscale, yscale, time, .. } = c { Some((*flip, *xscale, *yscale, *time)) } else { None }).collect();
        assert_eq!(moves, [(true, 2.0, 1.0, 1.0), (true, 2.0, 0.5, 2.0), (true, 2.0, 0.5, 0.0)]);
        let s = &game.state().sprites[0];
        assert_eq!((s.flip, s.xscale, s.yscale), (true, 2.0, 0.5));
        // redraw shows the sprite as it ended up
        assert!(game.state().redraw().iter().any(|c| matches!(c, Cmd::ShowSprite { flip: true, xscale, yscale, .. } if *xscale == 2.0 && *yscale == 0.5)));
    }

    #[test]
    fn flip_and_scales_are_saved_and_old_saves_load_unflipped() {
        let p = parse("::show{src=s.png id=s flip xscale=1.5 yscale=0.75}\n**A:** hi\n").unwrap();
        let (game, _) = Session::start(&p);
        let text = game.save();
        assert!(text.contains(" true 1.5 0.75"), "{text}");
        let (loaded, _) = Session::load(&p, &text).unwrap();
        let s = &loaded.state().sprites[0];
        assert_eq!((s.flip, s.xscale, s.yscale), (true, 1.5, 0.75));
        // a save from before these existed: the record stops after `z`
        let old: String = text
            .lines()
            .map(|l| if l.starts_with("sprite ") { l.rsplitn(4, ' ').last().unwrap().to_string() } else { l.to_string() })
            .collect::<Vec<_>>()
            .join("\n")
            + "\n";
        assert!(!old.contains("1.75") && !old.contains("true 1.5"), "{old}");
        let (loaded, _) = Session::load(&p, &old).unwrap();
        let s = &loaded.state().sprites[0];
        assert_eq!((s.flip, s.xscale, s.yscale), (false, 1.0, 1.0));
        // nonsense is pulled into range
        let wild = text.replace(" true 1.5 0.75", " true 99 NaN");
        let s = Session::load(&p, &wild).unwrap().0;
        assert_eq!((s.state().sprites[0].xscale, s.state().sprites[0].yscale), (10.0, 1.0));
    }

    // ---- music intro ---------------------------------------------------------------

    #[test]
    fn music_intro_is_kept_in_state_redraw_and_saves() {
        let p = parse("::music{src=loop.ogg intro=intro.ogg volume=0.5}\n**A:** hi\n").unwrap();
        let (game, first) = Session::start(&p);
        let play = Cmd::PlayMusic { src: "loop.ogg".into(), intro: Some("intro.ogg".into()), fade: 0.0, looped: true, volume: 0.5 };
        assert!(first.contains(&play));
        assert_eq!(game.state().music.as_ref().unwrap().intro.as_deref(), Some("intro.ogg"));
        assert!(game.state().redraw().contains(&play));
        let text = game.save();
        assert!(text.contains("music \"loop.ogg\" true 0.5 \"intro.ogg\""), "{text}");
        let (loaded, cmds) = Session::load(&p, &text).unwrap();
        assert_eq!(loaded.state().music, game.state().music);
        assert!(cmds.contains(&play));
        // a save from before intros has three values and loads without one
        let old = text.replace(" \"intro.ogg\"", "");
        assert_eq!(Session::load(&p, &old).unwrap().0.state().music.as_ref().unwrap().intro, None);
        // a save is untrusted: an intro path outside the game folder refuses the load
        let bad = text.replace("\"intro.ogg\"", "\"../x.ogg\"");
        assert!(Session::load(&p, &bad).is_err());
    }

    // ---- shake axis -----------------------------------------------------------------

    #[test]
    fn shake_carries_its_axis() {
        let p = parse("::shake{axis=y time=1}\n::shake{axis=x}\n**A:** hi\n").unwrap();
        let (_, cmds) = Session::start(&p);
        assert_eq!(cmds[0], Cmd::Shake { time: 1.0, strength: 20.0, axis: Axis::Y });
        assert_eq!(cmds[1], Cmd::Shake { time: 0.4, strength: 20.0, axis: Axis::X });
    }
}

#[cfg(test)]
mod dice;

#[cfg(test)]
mod text_input;
