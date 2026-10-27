//! Quillstone runtime: a pure state machine over a compiled script.
//!
//! `step(&mut State, &Program, Option<Input>) -> Vec<Cmd>` is the whole engine.
//! The renderer and audio backends only consume [`Cmd`]s; they never touch the
//! script. Everything that is "on screen" (background, sprites, music, the line
//! being shown, the call stack, variables) lives in [`State`], which is `Clone`:
//! one snapshot per player action gives rollback and save slots (see [`Session`]),
//! and [`State::redraw`] rebuilds the whole scene from a snapshot.

use quillstone_script::{AssignOp, ChoiceOption, Cond, Ease, Effect, Expr, Fit, Op, Program, ShopEntry, Value, Vars, Wipe};

mod explore;
mod persist;
mod save;
pub use explore::{explore, Finding, Report};
pub use persist::{read_persistent, write_persistent, UNLOCKED};
pub use save::{line_id, q as quote, tokens, voice_name, SAVE_HISTORY};

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
    /// bottom of the screen); `z` orders sprites (higher is in front).
    ShowSprite { id: String, src: String, x: f32, y: f32, z: i32, scale: f32, fade: f32 },
    HideSprite { id: String, fade: f32 },
    /// Animate a sprite to position `x`, `y` and size `scale` over `time` seconds.
    MoveSprite { id: String, x: f32, y: f32, scale: f32, time: f32, ease: Ease },
    /// Shake the stage for `time` seconds by up to `strength` pixels.
    Shake { time: f32, strength: f32 },
    /// Pan/zoom the stage to `camera` over `time` seconds.
    Camera { camera: Camera, time: f32, ease: Ease },
    PlayMusic { src: String, fade: f32, looped: bool, volume: f32 },
    StopMusic { fade: f32 },
    PlaySfx { src: String, volume: f32 },
    /// Voice clip for the line being shown; stop it when the player advances.
    PlayVoice { src: String },
    /// Pause; send [`Input::Advance`] when the time is up (or the player skips).
    Wait { seconds: f32 },
    /// Play a clip; send [`Input::Advance`] when it ends or the player skips it.
    PlayVideo { src: String, fit: Fit, looped: bool, skippable: bool },
    /// Show choices and wait for [`Input::Choose`]. Only the options currently
    /// available are listed; the index refers to this list.
    ShowChoices { options: Vec<String> },
    /// Show clickable regions over the background and wait for [`Input::Choose`]
    /// (from a `::hotspots` block). Like choices: only the regions currently
    /// available are listed, and the index refers to this list.
    ShowHotspots { spots: Vec<Spot> },
    /// Show the inventory screen (from `::inventory`) and wait for [`Input::Advance`]
    /// when the player closes it. What it lists is [`inventory_items`].
    ShowInventory,
    /// Show a shop and wait. [`Input::Buy`] buys the item at that index of
    /// `items` (the list now on offer) and shows the shop again with the new
    /// money and what is owned; [`Input::Advance`] leaves.
    ShowShop { pay: String, money: i64, items: Vec<ShopItem> },
    /// The story was stopped by a safety valve (a mistake in the script), and
    /// [`Cmd::Finished`] follows. Says what happened, for the author.
    Stopped { reason: String },
    Finished,
}

/// One clickable region (see [`Cmd::ShowHotspots`]).
#[derive(Debug, Clone, PartialEq)]
pub struct Spot {
    /// Shown as a tooltip while hovered.
    pub label: String,
    /// `x, y, w, h` as fractions (0..1) of the screen.
    pub rect: [f32; 4],
    /// Drawn stretched into the region.
    pub image: Option<String>,
    /// No tint, outline or tooltip, and skipped by keyboard focus; still clickable.
    pub hidden: bool,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Input {
    Advance,
    Choose(usize),
    /// Buy the item at this index of the shop's offered list.
    Buy(usize),
}

#[derive(Debug, Clone, PartialEq)]
pub struct SpriteState {
    pub id: String,
    pub src: String,
    pub x: f32,
    /// Where the bottom edge is, as a fraction of the screen height.
    pub y: f32,
    /// Drawing order among sprites: higher is in front, ties keep show order.
    pub z: i32,
    pub scale: f32,
}

/// What part of the stage is in view: `zoom` 1 shows all of it; the point
/// `x`, `y` (0..1 across and down) is kept in the middle where possible.
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
    pub looped: bool,
    pub volume: f32,
}

/// The line currently (or most recently) on screen.
#[derive(Debug, Clone, PartialEq)]
pub struct Line {
    pub speaker: Option<String>,
    pub color: Option<[f32; 3]>,
    pub text: String,
}

/// Everything needed to resume the game exactly: cheap to clone, easy to serialize later.
#[derive(Debug, Clone, PartialEq)]
pub struct State {
    pub pc: usize,
    pub vars: Vars,
    /// Return addresses of active `::call`s.
    pub stack: Vec<usize>,
    pub background: Option<String>,
    pub sprites: Vec<SpriteState>,
    pub camera: Camera,
    pub music: Option<MusicState>,
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
    pub fn new(program: &Program) -> State {
        State {
            pc: 0,
            vars: program.vars.clone(),
            stack: Vec::new(),
            background: None,
            sprites: Vec::new(),
            camera: Camera::default(),
            music: None,
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
        for s in &self.sprites {
            out.push(Cmd::ShowSprite {
                id: s.id.clone(),
                src: s.src.clone(),
                x: s.x,
                y: s.y,
                z: s.z,
                scale: s.scale,
                fade: 0.0,
            });
        }
        match &self.music {
            Some(m) => out.push(Cmd::PlayMusic {
                src: m.src.clone(),
                fade: 0.0,
                looped: m.looped,
                volume: m.volume,
            }),
            None => out.push(Cmd::StopMusic { fade: 0.0 }),
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

/// Indices of the options currently on offer (their condition, if any, is true).
fn visible_options(options: &[ChoiceOption], vars: &Vars) -> Vec<usize> {
    options
        .iter()
        .enumerate()
        .filter(|(_, o)| o.cond.as_ref().is_none_or(|c| c.test(vars)))
        .map(|(i, _)| i)
        .collect()
}

/// The item `id` as the screens show it. An id that no `::item` declares still
/// shows, under its own name and without a picture, so nothing in a list is lost.
pub fn item_info(program: &Program, id: &str) -> ItemInfo {
    match program.items.get(id) {
        Some(i) => ItemInfo { id: id.to_string(), name: i.name.clone(), icon: i.icon.clone(), text: i.text.clone(), price: i.price },
        None => ItemInfo { id: id.to_string(), name: id.to_string(), icon: None, text: String::new(), price: None },
    }
}

/// What the inventory screen lists: the items in the game's inventory list, in
/// the order they were added. Empty when the game has no inventory.
pub fn inventory_items(program: &Program, vars: &Vars) -> Vec<ItemInfo> {
    let Some(Value::List(held)) = program.inventory.as_ref().and_then(|name| vars.get(name)) else {
        return Vec::new();
    };
    held.iter().map(|id| item_info(program, id)).collect()
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
            ShopItem { item: item_info(program, &s.item), price: s.price, owned: held.contains(&s.item) }
        })
        .collect();
    Cmd::ShowShop { pay: pay.to_string(), money: money(vars, pay), items }
}

/// Buy the `index`th offered item of the shop at `state.pc`: pay and add it to the
/// list. `false` (and nothing changes) if it is owned, costs more than the player
/// has, or isn't there, so money can never go below zero.
fn buy(state: &mut State, program: &Program, index: usize) -> bool {
    let Some(Op::Shop { pay, into, stock }) = program.ops.get(state.pc) else { return false };
    let offered = offered_stock(stock, &state.vars);
    let Some(entry) = offered.get(index).map(|&i| &stock[i]) else { return false };
    let owned = matches!(state.vars.get(into), Some(Value::List(items)) if items.contains(&entry.item));
    if owned || entry.price < 0 || entry.price > money(&state.vars, pay) {
        return false;
    }
    let lit = |v| Expr::Lit(v);
    Effect { var: pay.clone(), op: AssignOp::Sub, value: lit(Value::Int(entry.price)) }.apply(&mut state.vars);
    Effect { var: into.clone(), op: AssignOp::Add, value: lit(Value::Str(entry.item.clone())) }.apply(&mut state.vars);
    true
}

/// The command a waiting op shows, for ops whose screen is built from the script
/// and the variables (used to rebuild it when a save is read).
pub(crate) fn rebuild_pending(program: &Program, op: &Op, vars: &Vars) -> Option<Cmd> {
    match op {
        Op::Choice { options } => Some(choice_cmd(options, &visible_options(options, vars), vars)),
        Op::Inventory => Some(Cmd::ShowInventory),
        Op::Shop { pay, into, stock } => Some(shop_cmd(program, pay, into, stock, vars)),
        _ => None,
    }
}

/// What a choice op shows for the options at `visible`: buttons, or regions for a `::hotspots` block.
fn choice_cmd(options: &[ChoiceOption], visible: &[usize], vars: &Vars) -> Cmd {
    if options.iter().any(|o| o.area.is_some()) {
        let spots = visible
            .iter()
            .filter_map(|&i| {
                let o = &options[i];
                let area = o.area.as_ref()?;
                Some(Spot { label: interpolate(&o.text, vars), rect: area.rect, image: area.image.clone(), hidden: area.hidden })
            })
            .collect();
        Cmd::ShowHotspots { spots }
    } else {
        Cmd::ShowChoices { options: visible.iter().map(|&i| interpolate(&options[i].text, vars)).collect() }
    }
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
                    match vars.get(name).filter(|_| name != "nw") {
                        Some(v) => out.push_str(&quillstone_script::markup::escape(&v.to_string())),
                        None => out.push_str(&rest[..=end]), // unknown, or the `{nw}` text tag
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

/// Advance the script. Call once with `None` to start, then with the player's input.
///
/// Input that doesn't match what the script is waiting for is ignored and returns
/// no commands. A step that does something always returns at least one command.
pub fn step(state: &mut State, program: &Program, input: Option<Input>) -> Vec<Cmd> {
    step_traced(state, program, input, &mut |_| {})
}

/// [`step`], telling `ran` the index of every op it runs (for the story tester).
pub(crate) fn step_traced(state: &mut State, program: &Program, input: Option<Input>, ran: &mut dyn FnMut(usize)) -> Vec<Cmd> {
    if state.finished {
        return Vec::new();
    }

    if let Some(pending) = state.pending.clone() {
        match (pending, input) {
            (
                Cmd::Say { .. }
                | Cmd::PlayVideo { .. }
                | Cmd::Wait { .. }
                | Cmd::ShowInventory
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
                let Some(Op::Shop { pay, into, stock }) = program.ops.get(state.pc) else { return Vec::new() };
                let cmd = shop_cmd(program, pay, into, stock, &state.vars);
                return block(state, &mut Vec::new(), cmd);
            }
            (Cmd::ShowChoices { .. } | Cmd::ShowHotspots { .. }, Some(Input::Choose(i))) => {
                let Some(Op::Choice { options: all }) = program.ops.get(state.pc) else {
                    return Vec::new();
                };
                let visible = visible_options(all, &state.vars);
                let Some(&index) = visible.get(i) else { return Vec::new() };
                let chosen = &all[index];
                for effect in &chosen.effects {
                    effect.apply(&mut state.vars);
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
        // Passing a label that some `seen(label)` asks about.
        for var in program.counted.get(&state.pc).into_iter().flatten() {
            let n = state.vars.get(var).and_then(Value::as_int).unwrap_or(0);
            state.vars.insert(var.clone(), Value::Int(n + 1));
        }
        match op {
            Op::Say { speaker, color, text } => {
                let text = interpolate(text, &state.vars);
                state.line = Some(Line { speaker: speaker.clone(), color: *color, text: text.clone() });
                return block(state, &mut out, Cmd::Say { speaker: speaker.clone(), color: *color, text });
            }
            Op::Video { src, fit, looped, skippable } => {
                state.line = None;
                let cmd = Cmd::PlayVideo { src: src.clone(), fit: *fit, looped: *looped, skippable: *skippable };
                return block(state, &mut out, cmd);
            }
            Op::Wait { seconds } => {
                return block(state, &mut out, Cmd::Wait { seconds: *seconds });
            }
            Op::Choice { options } => {
                let visible = visible_options(options, &state.vars);
                if visible.is_empty() {
                    state.pc += 1; // nothing on offer: carry on past the block
                    continue;
                }
                return block(state, &mut out, choice_cmd(options, &visible, &state.vars));
            }
            Op::Inventory => return block(state, &mut out, Cmd::ShowInventory),
            Op::Shop { pay, into, stock } => {
                if offered_stock(stock, &state.vars).is_empty() {
                    state.pc += 1; // nothing on offer: carry on past the block
                    continue;
                }
                let cmd = shop_cmd(program, pay, into, stock, &state.vars);
                return block(state, &mut out, cmd);
            }
            Op::Image { src, alt } => {
                state.inline.push((src.clone(), alt.clone()));
                out.push(Cmd::ShowImage { src: src.clone(), alt: alt.clone() });
                state.pc += 1;
            }
            Op::Bg { src, fade, wipe } => {
                state.background = Some(src.clone());
                out.push(Cmd::SetBackground { src: src.clone(), fade: *fade, wipe: *wipe });
                state.pc += 1;
            }
            Op::Show { id, src, at, y, z, scale, fade } => {
                let existing = state.sprites.iter().position(|s| s.id == *id);
                // A sprite already on stage keeps its place and size unless told otherwise.
                let x = at.or(existing.map(|i| state.sprites[i].x)).unwrap_or(0.5);
                let scale = scale.or(existing.map(|i| state.sprites[i].scale)).unwrap_or(1.0);
                let y = y.or(existing.map(|i| state.sprites[i].y)).unwrap_or(1.0);
                let z = z.or(existing.map(|i| state.sprites[i].z)).unwrap_or(0);
                let sprite = SpriteState { id: id.clone(), src: src.clone(), x, y, z, scale };
                match existing {
                    Some(i) => state.sprites[i] = sprite,
                    None => state.sprites.push(sprite),
                }
                out.push(Cmd::ShowSprite { id: id.clone(), src: src.clone(), x, y, z, scale, fade: *fade });
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
            Op::Move { id, at, y, scale, time, ease } => {
                if let Some(s) = state.sprites.iter_mut().find(|s| s.id == *id) {
                    s.x = at.unwrap_or(s.x);
                    s.y = y.unwrap_or(s.y);
                    s.scale = scale.unwrap_or(s.scale);
                    out.push(Cmd::MoveSprite { id: id.clone(), x: s.x, y: s.y, scale: s.scale, time: *time, ease: *ease });
                }
                state.pc += 1;
            }
            Op::Shake { time, strength } => {
                out.push(Cmd::Shake { time: *time, strength: *strength });
                state.pc += 1;
            }
            Op::Camera { zoom, x, y, time, ease } => {
                let c = &mut state.camera;
                *c = Camera { zoom: zoom.unwrap_or(c.zoom), x: x.unwrap_or(c.x), y: y.unwrap_or(c.y) };
                out.push(Cmd::Camera { camera: *c, time: *time, ease: *ease });
                state.pc += 1;
            }
            Op::Music { src, fade, looped, volume } => {
                state.music = Some(MusicState { src: src.clone(), looped: *looped, volume: *volume });
                out.push(Cmd::PlayMusic { src: src.clone(), fade: *fade, looped: *looped, volume: *volume });
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
            Op::Voice { src } => {
                state.voice = Some(src.clone());
                out.push(Cmd::PlayVoice { src: src.clone() });
                state.pc += 1;
            }
            Op::Jump { target, cond } => {
                let taken = cond.as_ref().is_none_or(|c| c.test(&state.vars));
                state.pc = if taken { *target } else { state.pc + 1 };
            }
            Op::Call { target } => {
                if state.stack.len() >= MAX_CALL_DEPTH {
                    let reason = format!("`::call` nested more than {MAX_CALL_DEPTH} deep (a call that never returns?)");
                    out.push(Cmd::Stopped { reason });
                    break;
                }
                state.stack.push(state.pc + 1);
                state.pc = *target;
            }
            Op::Return => match state.stack.pop() {
                Some(back) => state.pc = back,
                None => break, // `::return` outside a `::call` ends the story
            },
            Op::Set(effect) => {
                effect.apply(&mut state.vars);
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

/// A running game: the state plus a snapshot per player action, for rollback.
pub struct Session<'p> {
    program: &'p Program,
    state: State,
    /// The state before each accepted input, and that input.
    history: Vec<(State, Input)>,
    /// What rollback popped, newest rollback on top: each entry is a state the
    /// player was rolled back to and the input that left it. Not saved.
    redo: Vec<(State, Input)>,
    /// The state the first rollback of this run started from (where redo ends).
    redo_tip: Option<State>,
    /// Data kept across playthroughs (see `persist.rs`), and whether it changed.
    persistent: Vars,
    persistent_changed: bool,
}

impl<'p> Session<'p> {
    /// Start the script; returns the session and the commands to show first.
    pub fn start(program: &'p Program) -> (Session<'p>, Vec<Cmd>) {
        Session::start_with(program, Vars::new())
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
        let cmds = step(&mut self.state, self.program, Some(input));
        if !cmds.is_empty() {
            self.history.push((before, input));
            self.redo.clear(); // a new path: what was rolled back is gone
            self.redo_tip = None;
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
        let (first, input) = self.history.pop()?;
        if self.redo_tip.is_none() {
            self.redo_tip = Some(self.state.clone());
        }
        self.redo.push((first.clone(), input));
        self.state = first;
        while moves_on_by_itself(&self.state.pending) {
            match self.history.pop() {
                Some((earlier, input)) => {
                    self.redo.push((earlier.clone(), input));
                    self.state = earlier;
                }
                None => break,
            }
        }
        self.overlay(); // going back doesn't take back what was kept for good
        Some(self.state.redraw())
    }

    /// Undo a rollback: go on to the screen the player rolled back from. Waits and
    /// videos rollback went past are gone past again. `None` with nothing to redo.
    pub fn roll_forward(&mut self) -> Option<Vec<Cmd>> {
        let (state, input) = self.redo.pop()?;
        self.history.push((state, input));
        loop {
            match self.redo.last() {
                Some((next, _)) => self.state = next.clone(),
                None => {
                    self.state = self.redo_tip.take()?;
                    break;
                }
            }
            if !moves_on_by_itself(&self.state.pending) {
                break;
            }
            let (state, input) = self.redo.pop()?;
            self.history.push((state, input));
        }
        if self.redo.is_empty() {
            self.redo_tip = None;
        }
        self.overlay();
        Some(self.state.redraw())
    }

    /// Everything read and chosen on the way to the current screen, oldest first,
    /// including the line on screen now. Built from the rollback history, so a
    /// rollback also takes the lines after it out of the backlog.
    pub fn backlog(&self) -> Vec<Logged> {
        let mut out = Vec::new();
        let mut log = |state: &State, input: Option<&Input>| match (&state.pending, input) {
            (Some(Cmd::Say { .. }), _) => out.extend(state.line.clone().map(Logged::Line)),
            (Some(Cmd::ShowChoices { options }), Some(Input::Choose(i))) => {
                out.extend(options.get(*i).cloned().map(Logged::Choice));
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
                Cmd::ShowChoices { options: vec!["Good".into(), "Bad".into()] },
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
            all.extend(game.input(*input));
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
        let adv = [Input::Advance; 2];
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
        assert_eq!(first, vec![Cmd::ShowChoices { options: vec!["One".into(), "Two".into()] }]);
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
        assert_eq!(first, vec![Cmd::ShowChoices { options: vec!["One".into(), "Secret".into()] }]);
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
::show{mika neutral at=left fade=0.3}
**Mika:** Hi
::show{mika happy}
::move{mika at=right time=1}
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
            Cmd::ShowSprite { id: "mika".into(), src: "m_neutral.png".into(), x: 0.28, y: 1.0, z: 0, scale: 1.0, fade: 0.3 }
        );
        // speaker colour comes from the character
        assert!(matches!(&first[1], Cmd::Say { color: Some(c), .. } if c[0] == 1.0));

        let cmds = game.input(Input::Advance);
        // the new pose keeps the old x; then it moves
        assert_eq!(
            cmds[0],
            Cmd::ShowSprite { id: "mika".into(), src: "m_happy.png".into(), x: 0.28, y: 1.0, z: 0, scale: 1.0, fade: 0.0 }
        );
        assert_eq!(cmds[1], Cmd::MoveSprite { id: "mika".into(), x: 0.72, y: 1.0, scale: 1.0, time: 1.0, ease: Ease::InOut });
        assert_eq!(game.state().sprites.len(), 1);
        assert_eq!(game.state().sprites[0].x, 0.72);

        let cmds = game.input(Input::Advance);
        assert_eq!(cmds[0], Cmd::HideSprite { id: "mika".into(), fade: 0.5 });
        assert!(game.state().sprites.is_empty());
    }

    #[test]
    fn moves_keep_what_they_dont_change_and_the_camera_is_part_of_the_scene() {
        let src = "::show{src=s.png id=s at=left scale=0.8}\n::move{s scale=1.2}\n::camera{zoom=1.5}\n::camera{x=0.2 time=3}\n::shake\n**A:** one\n**A:** two\n";
        let p = parse(src).unwrap();
        let (mut game, first) = Session::start(&p);
        assert!(first.contains(&Cmd::MoveSprite { id: "s".into(), x: 0.28, y: 1.0, scale: 1.2, time: 0.5, ease: Ease::InOut }));
        let camera = Camera { zoom: 1.5, x: 0.2, y: 0.5 };
        assert!(first.contains(&Cmd::Camera { camera, time: 3.0, ease: Ease::InOut }), "keeps the zoom it had");
        assert!(first.contains(&Cmd::Shake { time: 0.4, strength: 20.0 }));
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
        let src = "::show{src=a.png id=a y=0.9 z=2}\n::show{src=b.png id=b}\n**A:** one\n::show{src=a2.png id=a}\n::move{a y=0.7 time=0.3 ease=out}\n**A:** two\n::move{a at=left time=0}\n::show{src=a3.png id=a y=1.1 z=-4}\n**A:** three\n";
        let p = parse(src).unwrap();
        let (mut game, first) = Session::start(&p);
        assert!(first.iter().any(|c| matches!(c, Cmd::ShowSprite { id, y, z, .. } if id == "a" && *y == 0.9 && *z == 2)));
        assert!(first.iter().any(|c| matches!(c, Cmd::ShowSprite { id, y, z, .. } if id == "b" && *y == 1.0 && *z == 0)), "new: bottom edge, z 0");
        let two = game.input(Input::Advance);
        assert!(matches!(&two[0], Cmd::ShowSprite { y, z, .. } if *y == 0.9 && *z == 2), "a new pose keeps y and z");
        assert_eq!(two[1], Cmd::MoveSprite { id: "a".into(), x: 0.5, y: 0.7, scale: 1.0, time: 0.3, ease: Ease::Out });
        assert_eq!(game.state().sprites[0].y, 0.7);
        let three = game.input(Input::Advance);
        assert!(matches!(&three[0], Cmd::MoveSprite { x, y, .. } if *x == 0.28 && *y == 0.7), "an `at` move keeps y");
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
            .map(|l| if l.starts_with("sprite ") { l.rsplitn(3, ' ').last().unwrap().to_string() } else { l.to_string() })
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
        assert_eq!((loaded.state().sprites[0].y, loaded.state().sprites[0].z), (1.5, 100));
        let nan = text.replace(" 0.85 7", " NaN 7");
        assert_eq!(Session::load(&p, &nan).unwrap().0.state().sprites[0].y, 1.0);
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
        let src = "::show{src=a.png id=a at=left}\n::show{src=b.png id=b at=right}\n::hide{ghost}\n::hide{all fade=1}\n**X:** hi\n";
        let cmds = run_to_end(src, &[]);
        let hidden: Vec<&Cmd> = cmds.iter().filter(|c| matches!(c, Cmd::HideSprite { .. })).collect();
        assert_eq!(hidden.len(), 2);
        assert!(hidden.iter().all(|c| matches!(c, Cmd::HideSprite { fade, .. } if *fade == 1.0)));
    }

    #[test]
    fn rollback_redraws_sprites_music_and_inline_images() {
        let src = "::bg{src=bg.png}\n::music{src=m.ogg volume=0.5}\n::show{src=s.png id=s at=center}\n**A:** one\n![alt](pic.png)\n::voice{src=v.ogg}\n**A:** two\n**A:** three\n";
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
        assert!(again.contains(&Cmd::PlayMusic { src: "m.ogg".into(), fade: 0.0, looped: true, volume: 0.5 }));
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
            Cmd::ShowChoices { options } => Some(options.clone()),
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
                Cmd::ShowChoices { options } => Some(options.clone()),
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
        assert_eq!(said[1..], ["You have 30 gold.", "Your bag is empty."]);
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
}
