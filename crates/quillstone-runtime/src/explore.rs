//! The story tester behind `quillstone test`: plays every branch of a script.
//!
//! [`step`](crate::step) is pure and [`State`] clones cheaply, so the tester forks the
//! story at every choice and follows each option, breadth first. A *situation* is
//! the script position, the call stack and the variables (the scene doesn't change
//! where the story goes); each one is explored once, so loops and hub menus end.
//! A story whose variables keep growing (a counter in a loop) would never run out
//! of new situations, so exploration stops at a limit and says so. Visit counts
//! (`seen(label)`) grow in every loop, so they are capped just past the largest
//! number the script compares them with: beyond that, more visits change nothing.
//! Persistent variables can hold what an earlier playthrough left, so the story
//! is also played from each combination of the values the script gives them.
//! The random seed is not part of a situation. A `random(lo, hi)` is a branch
//! point instead: one branch per value when the range has at most
//! [`MAX_ROLL_VALUES`] values, else `lo`, the middle and `hi` (and a warning).

use std::cell::Cell;
use std::collections::{BTreeSet, HashMap, HashSet, VecDeque};

use quillstone_script::{is_ident, Cmp, Expr, HudAction, KeyPress, Op, Program, Value, Vars, ONCE, SEEN};

use crate::{can_press, can_use, finish_input, offered_stock, pickable, step_traced, visible_options, Cmd, Input, Spot, State};

/// The widest `random(lo, hi)` the tester tries value by value; a wider one is
/// tried at its lowest, middle and highest value only.
pub const MAX_ROLL_VALUES: i64 = 12;

/// The most answers the tester tries for one `::input` (the default and the texts the script compares it to).
const MAX_ANSWERS: usize = 16;

/// Something the tester found, at a script line.
#[derive(Debug, Clone, PartialEq)]
pub struct Finding {
    pub line: usize,
    pub message: String,
    /// The choices that lead there from the start, when that helps to reproduce it.
    pub path: Option<Vec<String>>,
}

#[derive(Debug, Default)]
pub struct Report {
    /// Distinct situations explored.
    pub situations: usize,
    /// Script lines of the endings reached (`::end`, or the end of the script).
    pub endings: Vec<usize>,
    /// Ways the story breaks: it hangs, or can never end.
    pub errors: Vec<Finding>,
    /// Likely mistakes: unreachable parts, options never offered, unset variables.
    pub warnings: Vec<Finding>,
    /// Stopped at the limit, so some situations weren't explored (and the checks
    /// that need every branch were skipped).
    pub partial: bool,
    /// Steps re-run to follow a `random` fork: they count against the limit with the
    /// situations.
    pub reruns: usize,
}

/// Play every branch of `program`, exploring at most `limit` situations.
pub fn explore(program: &Program, limit: usize) -> Report {
    let mut x = Explorer {
        program,
        limit,
        nodes: Vec::new(),
        index: HashMap::new(),
        queue: VecDeque::new(),
        ran: vec![false; program.ops.len()],
        offered: HashSet::new(),
        shop_offered: HashSet::new(),
        used: HashSet::new(),
        reported: HashSet::new(),
        endings: BTreeSet::new(),
        report: Report::default(),
        caps: seen_caps(program),
        reruns: 0,
    };
    let compared = compared_texts(program);
    for (origin, kept) in playthroughs(program) {
        let mut state = State::new(program);
        state.vars.extend(kept);
        for (after, trace) in x.outcomes(&state, None) {
            if let Some(root) = x.visit(after, trace, None) {
                x.nodes[root].origin = origin.clone();
            }
        }
    }
    while let Some(id) = x.queue.pop_front() {
        let state = x.nodes[id].state.take().expect("queued once, with its state");
        let inputs: Vec<(Input, Option<String>)> = match &state.pending {
            Some(pending @ (Cmd::ShowChoices { .. } | Cmd::ShowHotspots { .. })) => {
                if let Some(Op::Choice { options: all, .. }) = program.ops.get(state.pc) {
                    // A greyed option can't be picked, so it doesn't count as offered.
                    let open = visible_options(all, &state.vars).into_iter().filter(|&i| pickable(&all[i], &state.vars));
                    x.offered.extend(open.map(|i| (state.pc, i)));
                }
                let (labels, locked) = match pending {
                    Cmd::ShowChoices { options, locked } => (options.clone(), locked.clone()),
                    Cmd::ShowHotspots { spots, .. } => {
                        // A region bound to a sprite that isn't shown can't be clicked (the explorer doesn't know
                        // where sprites are, so a shown one counts); the timeout is told apart by `Spot::is_timeout`.
                        let gone = |s: &Spot| s.sprite.as_ref().filter(|id| !state.sprites.iter().any(|p| &p.id == *id)).map(|_| String::new());
                        let label = |s: &Spot| if s.is_timeout() { "(time out)".to_string() } else { s.label.clone() };
                        (spots.iter().map(label).collect(), spots.iter().map(gone).collect())
                    }
                    _ => (Vec::new(), Vec::new()),
                };
                let open = |i: &usize| !locked.get(*i).is_some_and(Option::is_some);
                labels.into_iter().enumerate().filter(|(i, _)| open(i)).map(|(i, text)| (Input::Choose(i), Some(text))).collect()
            }
            // Each outcome the bar can have is a branch.
            Some(Cmd::ShowTiming { bar, .. }) => bar.hits().iter().map(|h| (Input::Choose(*h as usize), Some(h.text().to_string()))).collect(),
            Some(Cmd::ShowPress { .. }) => (0..).map_while(|i| Some((Input::Choose(i), Some(KeyPress::outcome_text(i)?.to_string())))).collect(),
            Some(Cmd::ShowShop { items, .. }) => {
                if let Some(Op::Shop { stock, .. }) = program.ops.get(state.pc) {
                    x.shop_offered.extend(offered_stock(stock, &state.vars).into_iter().map(|i| (state.pc, i)));
                }
                // Each purchase the player can make, and walking out. Money only goes down and
                // an item is bought once, so a shop that loops back to itself runs out.
                let mut inputs: Vec<(Input, Option<String>)> = items
                    .iter()
                    .enumerate()
                    .filter(|(_, s)| !s.owned && s.price <= money_of(&state))
                    .map(|(i, s)| (Input::Buy(i), Some(format!("buy {}", s.item.name))))
                    .collect();
                inputs.push((Input::Advance, Some("leave the shop".to_string())));
                inputs
            }
            // The default (or "Player"), plus every text the script compares the answer to, cleaned up as
            // typed text is: a branch like `::if{name == "Bob"}` can then be played.
            Some(Cmd::AskInput { default, max, .. }) => {
                let mut answers = vec![default.clone().unwrap_or_else(|| "Player".chars().take(*max).collect())];
                if let Some(Op::Input { var, default, max, .. }) = program.ops.get(state.pc) {
                    for text in compared.get(var.as_str()).into_iter().flatten() {
                        let typed = finish_input(text, default.as_deref(), *max);
                        if let Some(t) = typed.filter(|t| !answers.contains(t)) {
                            answers.push(t);
                        }
                    }
                }
                if answers.len() > MAX_ANSWERS {
                    x.report.partial = true;
                    answers.truncate(MAX_ANSWERS);
                }
                answers.into_iter().map(|a| (Input::Text(a), None)).collect()
            }
            Some(Cmd::Say { .. }) => {
                // Using an item runs its label and comes back to this line: each item is tried once,
                // the first time it can be used (every use at every line would multiply the situations).
                let mut inputs = vec![(Input::Advance, None)];
                for id in program.items.keys() {
                    if !x.used.contains(id) && can_use(program, &state, id) {
                        x.used.insert(id.clone());
                        inputs.push((Input::Use(id.clone()), Some(format!("use {}", program.items[id].name))));
                    }
                }
                // The same for each HUD button that calls a label (`open=` buttons need nothing).
                for h in state.hud.iter().filter(|h| matches!(h.action, Some(HudAction::Call(_)))) {
                    let key = format!("hud:{}", h.id);
                    if !x.used.contains(&key) && can_press(program, &state, &h.id) {
                        x.used.insert(key);
                        inputs.push((Input::Hud(h.id.clone()), Some(format!("press {}", h.id))));
                    }
                }
                inputs
            }
            Some(_) => vec![(Input::Advance, None)],
            None => Vec::new(),
        };
        // A `call` region comes back to its screen, and with the call stack full the click does nothing:
        // a label that jumps back to the screen without `::return` (reported like a `::call` that never returns).
        if state.stack.len() >= crate::MAX_CALL_DEPTH {
            if let Some(Op::Choice { options: all, .. }) = program.ops.get(state.pc) {
                let visible = visible_options(all, &state.vars);
                let full = inputs.iter().any(|(i, _)| matches!(i, Input::Choose(n) if visible.get(*n).is_some_and(|&k| all[k].area.as_ref().is_some_and(|a| a.call))));
                if full {
                    let msg = "a `call` region nests more than 256 deep here (its label jumps back to this screen without `::return`?)";
                    x.error(state.pc, "deep", msg, Some(id));
                }
            }
        }
        for (input, label) in inputs {
            for (after, trace) in x.outcomes(&state, Some(input)) {
                if let Some(next) = x.visit(after, trace, Some((id, label.clone()))) {
                    x.nodes[id].next.push(next);
                }
            }
        }
    }
    x.finish()
}

/// For each variable, the text literals the script compares it to with `==` or `!=`, in script order.
fn compared_texts(program: &Program) -> HashMap<&str, Vec<&str>> {
    fn walk<'a>(out: &mut HashMap<&'a str, Vec<&'a str>>, e: &'a Expr) {
        if let Expr::Cmp(a, Cmp::Eq | Cmp::Ne, b) = e {
            if let (Expr::Var(v), Expr::Lit(Value::Str(t))) | (Expr::Lit(Value::Str(t)), Expr::Var(v)) = (&**a, &**b) {
                let texts = out.entry(v.as_str()).or_default();
                if !texts.contains(&t.as_str()) {
                    texts.push(t);
                }
            }
        }
        e.children().into_iter().for_each(|c| walk(out, c));
    }
    let mut out = HashMap::new();
    program.ops.iter().flat_map(Op::exprs).for_each(|e| walk(&mut out, e));
    out
}

/// The money a shop at the state's position asks for.
fn money_of(state: &State) -> i64 {
    match state.pending {
        Some(Cmd::ShowShop { money, .. }) => money,
        _ => 0,
    }
}

struct Node {
    pc: usize,
    /// Kept until the situation has been explored.
    state: Option<State>,
    /// Where it was first reached from, and the choice made there (if it was one).
    parent: Option<(usize, Option<String>)>,
    next: Vec<usize>,
    /// The story stopped here (an ending, or a problem already reported).
    stopped: bool,
    /// For a starting point other than a first playthrough: what is different.
    origin: Option<String>,
}

struct Explorer<'p> {
    program: &'p Program,
    limit: usize,
    nodes: Vec<Node>,
    index: HashMap<(usize, Vec<crate::Frame>, Vars, bool), usize>,
    queue: VecDeque<usize>,
    /// Ops that ever ran.
    ran: Vec<bool>,
    /// (choice op, option index) pairs that were ever on offer.
    offered: HashSet<(usize, usize)>,
    /// (shop op, stock line) pairs that were ever on offer.
    shop_offered: HashSet<(usize, usize)>,
    /// Items whose Use action was already tried.
    used: HashSet<String>,
    /// (line, kind) already reported, to say each thing once.
    reported: HashSet<(usize, &'static str)>,
    endings: BTreeSet<usize>,
    report: Report,
    /// The most each `seen:` count can matter (see the module notes).
    caps: Vec<(String, i64)>,
    /// Steps re-run to follow a `random` fork, each charged against `limit` together
    /// with the situations found (so rolls can't cost more than the limit allows).
    reruns: usize,
}

impl Explorer<'_> {
    /// Every way stepping from `from` with `input` can end, with the ops that ran in
    /// each: one, unless a `random(lo, hi)` branches it (see [`MAX_ROLL_VALUES`]).
    /// Each branch is found by running the step again with the rolls fixed so far; a
    /// roll that is not fixed yet is where the next fork goes.
    fn outcomes(&mut self, from: &State, input: Option<Input>) -> Vec<(State, Vec<usize>)> {
        let mut done = Vec::new();
        let mut work: Vec<Vec<i64>> = vec![Vec::new()];
        while let Some(fixed) = work.pop() {
            // Every step run after the first is a re-simulation: it costs like a situation.
            if !fixed.is_empty() {
                if self.nodes.len() + self.reruns >= self.limit {
                    self.report.partial = true;
                    break;
                }
                self.reruns += 1;
            }
            let mut state = from.clone();
            let mut trace = Vec::new();
            let at = Cell::new(from.pc);
            let mut next_roll: Option<(i64, i64, usize)> = None;
            let mut asked = 0;
            let cmds;
            {
                let mut draw = |lo: i64, hi: i64| {
                    let k = asked;
                    asked += 1;
                    match fixed.get(k) {
                        Some(&v) => v,
                        None => {
                            next_roll.get_or_insert((lo, hi, at.get()));
                            lo
                        }
                    }
                };
                let mut ran = |pc: usize| {
                    trace.push(pc);
                    at.set(pc);
                };
                cmds = step_traced(&mut state, self.program, input.clone(), &mut ran, Some(&mut draw));
            }
            // A picture that can't be shown (a variable that isn't a safe name, a layer left empty)
            // is a warning the player would see as a notice.
            for cmd in &cmds {
                if let Cmd::Warning { message } = cmd {
                    self.warn(at.get(), "picture", message, None);
                }
            }
            let Some((lo, hi, pc)) = next_roll else {
                done.push((state, trace));
                continue;
            };
            let span = hi as i128 - lo as i128 + 1;
            let values: Vec<i64> = if span <= MAX_ROLL_VALUES as i128 {
                (lo..=hi).collect()
            } else {
                let msg = format!(
                    "`random({lo}, {hi})` has {span} values: the tester tries only {lo}, {} and {hi}, so what \
                     happens for the others is not checked",
                    lo + (hi - lo) / 2
                );
                self.warn(pc, "wide random", &msg, None);
                vec![lo, lo + (hi - lo) / 2, hi]
            };
            // never more branches than the situation limit (but always one way on)
            let spent = self.nodes.len() + self.reruns + done.len() + work.len();
            let room = self.limit.saturating_sub(spent).max(1);
            let mut values = values;
            if values.len() > room {
                self.report.partial = true;
                values.truncate(room);
            }
            for v in values.into_iter().rev() {
                let mut f = fixed.clone();
                f.push(v);
                work.push(f);
            }
        }
        done
    }

    /// File the situation `state` (reached by running `trace`) leads to.
    fn visit(&mut self, state: State, trace: Vec<usize>, parent: Option<(usize, Option<String>)>) -> Option<usize> {
        let mut state = state;
        for &pc in &trace {
            self.ran[pc] = true;
        }
        for (var, cap) in &self.caps {
            if let Some(n) = state.vars.get_mut(var) {
                if n.as_int().is_some_and(|n| n > *cap) {
                    *n = Value::Int(*cap);
                }
            }
        }
        // Lists are sets: the same items in another order are the same situation.
        let mut vars = state.vars.clone();
        for v in vars.values_mut() {
            if let Value::List(items) = v {
                items.sort();
            }
        }
        let key = (state.pc, state.stack.clone(), vars, state.finished);
        if let Some(&id) = self.index.get(&key) {
            return Some(id);
        }
        if self.nodes.len() + self.reruns >= self.limit {
            self.report.partial = true;
            return None;
        }
        let id = self.nodes.len();
        self.index.insert(key, id);
        let finished = state.finished;
        let (pc, waiting_at_choice) = (state.pc, matches!(state.pending, Some(Cmd::ShowChoices { .. } | Cmd::ShowHotspots { .. })));
        self.nodes.push(Node { pc, state: Some(state), parent, next: Vec::new(), stopped: finished, origin: None });

        // A choice that ran but didn't wait had nothing on offer. (A menu of only
        // `{once}` options is meant to run dry, so that one is left alone.)
        for (i, &at) in trace.iter().enumerate() {
            let waited = i + 1 == trace.len() && waiting_at_choice;
            let Op::Choice { options, .. } = &self.program.ops[at] else { continue };
            let all_once = options.iter().all(|o| o.effects.iter().any(|e| e.var.starts_with(ONCE)));
            if !waited && !all_once {
                let msg = "this choice can come up with no option on offer (the story then carries on past it)";
                self.warn(at, "empty choice", msg, Some(id));
            }
        }
        if finished {
            self.stopped_at(pc, id);
        } else {
            self.queue.push_back(id);
        }
        Some(id)
    }

    /// Why the story stopped at op `pc`.
    fn stopped_at(&mut self, pc: usize, id: usize) {
        match self.program.ops.get(pc) {
            Some(Op::End) | None => {
                self.endings.insert(self.line(pc));
            }
            Some(Op::Return) => {
                self.endings.insert(self.line(pc));
                let msg = "the story ends here: `::return` with no `::call` to go back to";
                self.warn(pc, "return", msg, Some(id));
            }
            Some(Op::Call { .. }) => {
                let msg = "`::call` nests more than 256 deep here (a label that calls itself without returning?)";
                self.error(pc, "deep", msg, Some(id));
            }
            Some(_) => {
                let msg = "the story hangs here: an endless loop that never shows a line or a choice";
                self.error(pc, "hang", msg, Some(id));
            }
        }
    }

    fn finish(mut self) -> Report {
        if !self.report.partial {
            self.stuck();
            self.unreached();
            self.locked_replays();
            self.never_offered();
        }
        self.unset_variables();
        self.unadded_items();
        let mut r = self.report;
        r.situations = self.nodes.len();
        r.reruns = self.reruns;
        r.endings = self.endings.into_iter().collect();
        r.errors.sort_by_key(|f| f.line);
        r.warnings.sort_by_key(|f| f.line);
        r
    }

    /// Situations from which no ending can be reached: the player can only go round.
    fn stuck(&mut self) {
        let mut before: Vec<Vec<usize>> = vec![Vec::new(); self.nodes.len()];
        for (id, node) in self.nodes.iter().enumerate() {
            for &next in &node.next {
                before[next].push(id);
            }
        }
        let mut can_end: Vec<bool> = self.nodes.iter().map(|n| n.stopped).collect();
        let mut todo: Vec<usize> = (0..self.nodes.len()).filter(|&i| can_end[i]).collect();
        while let Some(id) = todo.pop() {
            for &prev in &before[id] {
                if !can_end[prev] {
                    can_end[prev] = true;
                    todo.push(prev);
                }
            }
        }
        // Say it where the player walks into the trap, not at every step inside it.
        let mut reported = 0;
        for id in 0..self.nodes.len() {
            let entry = self.nodes[id].parent.as_ref().is_none_or(|(parent, _)| can_end[*parent]);
            if !can_end[id] && entry && reported < 5 {
                let msg = "from here the story can never reach an ending: every way on loops back";
                if self.error(self.nodes[id].pc, "stuck", msg, Some(id)) {
                    reported += 1;
                }
            }
        }
    }

    /// Replays whose label no branch reaches: the Scenes tab would never unlock them.
    fn locked_replays(&mut self) {
        let program = self.program;
        for replay in &program.replays {
            let at = program.labels[&replay.label];
            if at < self.ran.len() && !self.ran[at] {
                let msg = format!("replay `{}` can never be unlocked: no branch reaches its label", replay.label);
                self.report.warnings.push(Finding { line: self.line(at), message: msg, path: None });
            }
        }
    }

    /// Labels and lines no branch ever reaches.
    fn unreached(&mut self) {
        let program = self.program;
        let mut unreached_labels: Vec<usize> = Vec::new();
        for (name, &at) in &program.labels {
            if at < self.ran.len() && !self.ran[at] && !matches!(program.ops[at], Op::End) {
                unreached_labels.push(at);
                let msg = format!("the label `{name}` is never reached on any branch");
                self.report.warnings.push(Finding { line: self.line(at), message: msg, path: None });
            }
        }
        let mut lines = 0;
        for (at, op) in program.ops.iter().enumerate() {
            let Op::Say { text, .. } = op else { continue };
            if self.ran[at] {
                continue;
            }
            let label = program.labels.values().filter(|&&l| l <= at).max().copied();
            if label.is_some_and(|l| unreached_labels.contains(&l)) {
                continue; // covered by the label's warning
            }
            lines += 1;
            if lines <= 20 {
                let msg = format!("this line is never shown: \"{}\"", short(text));
                self.report.warnings.push(Finding { line: self.line(at), message: msg, path: None });
            }
        }
        if lines > 20 {
            let msg = format!("...and {} more lines that are never shown", lines - 20);
            self.report.warnings.push(Finding { line: 0, message: msg, path: None });
        }
    }

    /// Conditional options whose condition is false every time their choice comes up.
    fn never_offered(&mut self) {
        let program = self.program;
        for (at, op) in program.ops.iter().enumerate() {
            if let Op::Shop { stock, .. } = op {
                for (i, entry) in stock.iter().enumerate() {
                    if self.ran[at] && entry.cond.is_some() && !self.shop_offered.contains(&(at, i)) {
                        let name = program.items.get(&entry.item).map_or(entry.item.as_str(), |i| i.name.as_str());
                        let msg = format!("\"{}\" is never offered in this shop: its condition is never true here", short(name));
                        self.report.warnings.push(Finding { line: self.line(at), message: msg, path: None });
                    }
                }
            }
            let Op::Choice { options, .. } = op else { continue };
            if !self.ran[at] {
                continue;
            }
            for (i, option) in options.iter().enumerate() {
                if option.cond.is_some() && !self.offered.contains(&(at, i)) {
                    let msg = format!("the option \"{}\" is never offered: its condition is never true here", short(&option.text));
                    self.report.warnings.push(Finding { line: self.line(at), message: msg, path: None });
                }
            }
        }
    }

    /// Variables read somewhere but set nowhere: always 0, which is usually a typo.
    fn unset_variables(&mut self) {
        let program = self.program;
        let mut set: HashSet<&str> = program.vars.keys().map(String::as_str).collect();
        let mut read: Vec<(usize, &str, bool)> = Vec::new(); // (op, name, in text)
        for (at, op) in program.ops.iter().enumerate() {
            for e in op.exprs() {
                read.extend(names(|out| e.vars_read(out)).into_iter().map(|n| (at, n, false)));
            }
            match op {
                Op::Set(effect) => {
                    set.insert(&effect.var);
                    read.extend(quillstone_script::name_vars(&effect.var).into_iter().map(|n| (at, n, false)));
                }
                // A call sets its label's parameters, from the values it passes.
                Op::Call { args, .. } => set.extend(args.iter().map(|(param, _)| param.as_str())),
                Op::Say { speaker, text, .. } => {
                    read.extend(braces(text).into_iter().map(|n| (at, n, true)));
                    // a character's name may hold `{variable}`s too
                    read.extend(speaker.iter().flat_map(|s| braces(s)).map(|n| (at, n, true)));
                }
                // A HUD element reads its variables every time it is drawn.
                Op::Hud { elem, .. } => {
                    if let quillstone_script::HudKind::Text { text, .. } = &elem.kind {
                        read.extend(braces(text).into_iter().map(|n| (at, n, true)));
                    }
                }
                Op::Input { prompt, .. } | Op::Timing { prompt, .. } | Op::Press { prompt, .. } | Op::Notify { text: prompt, .. } => read.extend(braces(prompt).into_iter().map(|n| (at, n, true))),
                // A picture named from variables (`assets/{place}.png`).
                Op::Bg { src, .. } | Op::Image { src, .. } => {
                    read.extend(quillstone_script::pattern_vars(src).into_iter().map(|n| (at, n, false)));
                }
                // ... also in the pictures of a layered sprite's layers.
                Op::Show { src, layers, .. } => {
                    let names = std::iter::once(src).chain(layers.iter().map(|(_, p)| p));
                    read.extend(names.flat_map(|p| quillstone_script::pattern_vars(p)).map(|n| (at, n, false)));
                }
                Op::Shop { pay, into, .. } => {
                    set.insert(into);
                    read.push((at, pay, false));
                }
                Op::Choice { options, .. } => {
                    for o in options {
                        set.extend(o.effects.iter().map(|e| e.var.as_str()));
                        read.extend(o.effects.iter().flat_map(|e| quillstone_script::name_vars(&e.var)).map(|n| (at, n, false)));
                        read.extend(braces(&o.text).into_iter().map(|n| (at, n, true)));
                    }
                }
                _ => {}
            }
        }
        let mut told = HashSet::new();
        for (at, name, in_text) in read {
            if set.contains(name) || name.starts_with(SEEN) || !told.insert(name) {
                continue;
            }
            let message = if in_text {
                format!("`{{{name}}}` isn't a variable that is ever set, so it shows as written (a typo?)")
            } else {
                format!("`{name}` is never set anywhere, so it is always 0 (a typo?)")
            };
            self.report.warnings.push(Finding { line: self.line(at), message, path: None });
        }
    }

    /// `list has "item"` where no `+=` adds that literal text: usually a typo. Lists
    /// that something adds to with a computed value are skipped (not knowable).
    fn unadded_items(&mut self) {
        let program = self.program;
        let added = added_items(program);
        let mut told = HashSet::new();
        for (at, op) in program.ops.iter().enumerate() {
            let mut tests = Vec::new();
            for e in op.exprs() {
                e.has_tests(&mut tests);
            }
            for (list, item) in tests {
                let (Some(Some(known)), Expr::Lit(Value::Str(text))) = (added.get(list), item) else { continue };
                if !known.contains(text) && told.insert((list, text)) {
                    let message = format!("`{list} has \"{text}\"` can never be true: nothing adds \"{text}\" to `{list}` (a typo?)");
                    self.report.warnings.push(Finding { line: self.line(at), message, path: None });
                }
            }
        }
    }

    fn line(&self, at: usize) -> usize {
        self.program.lines.get(at).copied().unwrap_or(0)
    }

    /// The choices made on the way to situation `id`.
    fn path(&self, id: usize) -> Vec<String> {
        let mut path = Vec::new();
        let mut at = id;
        while let Some((parent, choice)) = &self.nodes[at].parent {
            path.extend(choice.clone());
            at = *parent;
        }
        path.extend(self.nodes[at].origin.clone());
        path.reverse();
        path
    }

    /// Report once per line and kind; `true` if this was new.
    fn error(&mut self, at: usize, kind: &'static str, msg: &str, id: Option<usize>) -> bool {
        let line = self.line(at);
        if !self.reported.insert((line, kind)) {
            return false;
        }
        let path = id.map(|id| self.path(id));
        self.report.errors.push(Finding { line, message: msg.into(), path });
        true
    }

    fn warn(&mut self, at: usize, kind: &'static str, msg: &str, id: Option<usize>) {
        let line = self.line(at);
        if self.reported.insert((line, kind)) {
            let path = id.map(|id| self.path(id));
            self.report.warnings.push(Finding { line, message: msg.into(), path });
        }
    }
}

/// How far each `seen:` count matters: one past the largest number it is compared
/// with, or 1 where it is only tested as yes/no. Counts that feed an assignment or
/// a comparison with another variable aren't capped.
fn seen_caps(program: &Program) -> Vec<(String, i64)> {
    fn raise(caps: &mut HashMap<String, Option<i64>>, var: &str, cap: Option<i64>) {
        let entry = caps.entry(var.to_string()).or_insert(Some(0));
        *entry = match (*entry, cap) {
            (Some(a), Some(b)) => Some(a.max(b)),
            _ => None,
        };
    }
    fn uncapped(caps: &mut HashMap<String, Option<i64>>, e: &Expr) {
        let mut vars = Vec::new();
        e.vars_read(&mut vars);
        for v in vars.into_iter().filter(|v| v.starts_with(SEEN)) {
            raise(caps, v, None);
        }
    }
    fn tested(caps: &mut HashMap<String, Option<i64>>, e: &Expr) {
        match e {
            Expr::Var(v) if v.starts_with(SEEN) => raise(caps, v, Some(1)),
            Expr::Not(a) => tested(caps, a),
            Expr::And(a, b) | Expr::Or(a, b) => {
                tested(caps, a);
                tested(caps, b);
            }
            Expr::Has(_, item) => uncapped(caps, item),
            Expr::Cmp(a, _, b) => match (&**a, &**b) {
                (Expr::Var(v), Expr::Lit(x)) | (Expr::Lit(x), Expr::Var(v)) if v.starts_with(SEEN) => {
                    raise(caps, v, Some(x.as_int().unwrap_or(0).max(0) + 1));
                }
                _ => {
                    uncapped(caps, a);
                    uncapped(caps, b);
                }
            },
            // arithmetic and calls: whatever `seen:` counts they read feed a computed value
            Expr::Bin(..) | Expr::Neg(_) | Expr::Func(..) | Expr::Random(..) | Expr::Pick(_) => uncapped(caps, e),
            Expr::Lit(_) | Expr::Var(_) | Expr::Count(_) => {}
        }
    }
    let mut caps = HashMap::new();
    for op in &program.ops {
        match op {
            Op::Jump { cond: Some(c), .. } => tested(&mut caps, c),
            Op::Set(e) => uncapped(&mut caps, &e.value),
            Op::Call { args, .. } => args.iter().for_each(|(_, e)| uncapped(&mut caps, e)),
            Op::Choice { options, .. } => {
                for o in options {
                    if let Some(c) = &o.cond {
                        tested(&mut caps, c);
                    }
                    for e in &o.effects {
                        uncapped(&mut caps, &e.value);
                    }
                }
            }
            Op::Shop { stock, .. } => stock.iter().filter_map(|s| s.cond.as_ref()).for_each(|c| tested(&mut caps, c)),
            // no other op holds an expression (see `Op::exprs`)
            _ => {}
        }
    }
    let mut caps: Vec<(String, i64)> = caps.into_iter().filter_map(|(v, cap)| Some((v, cap?))).collect();
    caps.sort();
    caps
}

/// For each declared list variable, the items that can ever be in it (its starting
/// items and every literal text added with `+=`), or `None` if some `+=` adds
/// something computed.
fn added_items(program: &Program) -> HashMap<&str, Option<Vec<String>>> {
    let mut out: HashMap<&str, Option<Vec<String>>> = HashMap::new();
    for (name, v) in &program.vars {
        if let Value::List(items) = v {
            out.insert(name, Some(items.clone()));
        }
    }
    // a shop adds what it sells to its list
    for op in &program.ops {
        let Op::Shop { into, stock, .. } = op else { continue };
        if let Some(Some(items)) = out.get_mut(into.as_str()) {
            for s in stock {
                if !items.contains(&s.item) {
                    items.push(s.item.clone());
                }
            }
        }
    }
    for e in all_effects(program) {
        // a name with `{}` parts could add to any list: nothing is known about them
        if quillstone_script::is_dynamic(&e.var) && e.op == quillstone_script::AssignOp::Add {
            out.values_mut().for_each(|slot| *slot = None);
        }
        let Some(slot) = out.get_mut(e.var.as_str()) else { continue };
        if e.op != quillstone_script::AssignOp::Add {
            continue;
        }
        match (&e.value, slot.as_mut()) {
            (Expr::Lit(Value::Str(s)), Some(items)) => {
                if !items.contains(s) {
                    items.push(s.clone());
                }
            }
            _ => *slot = None,
        }
    }
    out
}

fn all_effects(program: &Program) -> impl Iterator<Item = &quillstone_script::Effect> {
    program.ops.iter().flat_map(|op| match op {
        Op::Set(e) => vec![e],
        Op::Choice { options, .. } => options.iter().flat_map(|o| &o.effects).collect(),
        _ => Vec::new(),
    })
}

/// The starting points: a first playthrough, then each combination of values the
/// script assigns its persistent variables (as a later playthrough could have
/// left them), described for messages. At most 64 in all.
fn playthroughs(program: &Program) -> Vec<(Option<String>, Vars)> {
    let mut choices: Vec<(&str, Vec<Value>)> = Vec::new();
    let mut added = added_items(program);
    for name in &program.persistent {
        let mut values: Vec<Value> = program.vars.get(name).cloned().into_iter().collect();
        let assigned = program.ops.iter().flat_map(|op| match op {
            Op::Set(e) => vec![e],
            Op::Choice { options, .. } => options.iter().flat_map(|o| &o.effects).collect(),
            _ => Vec::new(),
        });
        for e in assigned.filter(|e| e.var == *name && e.op == quillstone_script::AssignOp::Set) {
            if let Expr::Lit(v) = &e.value {
                if !values.contains(v) {
                    values.push(v.clone());
                }
            }
        }
        // a later playthrough may have collected every item the script ever adds
        if let Some(Some(all)) = added.remove(name.as_str()) {
            let full = Value::List(all);
            if !values.contains(&full) {
                values.push(full);
            }
        }
        choices.push((name, values));
    }
    let mut out: Vec<Vars> = vec![Vars::new()];
    for (name, values) in &choices {
        let mut grown = Vec::new();
        for vars in &out {
            for v in values {
                let mut vars = vars.clone();
                vars.insert(name.to_string(), v.clone());
                grown.push(vars);
            }
        }
        if grown.len() > 64 {
            break; // enough combinations; the rest keep their starting values
        }
        out = grown;
    }
    out.into_iter()
        .enumerate()
        .map(|(i, vars)| {
            let changed: Vec<String> = vars
                .iter()
                .filter(|(k, v)| program.vars.get(*k) != Some(*v))
                .map(|(k, v)| format!("{k} = {v}"))
                .collect();
            let origin = (i > 0 && !changed.is_empty()).then(|| format!("(a later playthrough, with {})", changed.join(", ")));
            (origin, vars)
        })
        .collect()
}

fn names<'a>(collect: impl FnOnce(&mut Vec<&'a str>)) -> Vec<&'a str> {
    let mut out = Vec::new();
    collect(&mut out);
    out
}

/// The `{name}`s a text would fill in (as [`interpolate`](crate::interpolate) reads them).
fn braces(text: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut rest = text;
    while let Some(pos) = rest.find(['{', '\\']) {
        rest = &rest[pos..];
        if let Some(after) = rest.strip_prefix("\\{") {
            rest = after;
        } else if rest.starts_with('{') {
            let Some(end) = crate::brace_end(rest) else { break };
            let name = rest[1..end].trim();
            if is_ident(name) && !matches!(name, "nw" | "p" | "u") {
                out.push(name);
            } else if name.contains('{') {
                out.extend(quillstone_script::name_vars(name)); // `{love.{who}}` reads `who`
            }
            rest = &rest[end + 1..];
        } else {
            rest = &rest[1..];
        }
    }
    out
}

/// A text cut down for a message.
fn short(text: &str) -> String {
    let mut s: String = text.chars().take(50).collect();
    if s.len() < text.len() {
        s.push_str("...");
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use quillstone_script::parse;
    use std::time::Instant;

    fn test(src: &str) -> Report {
        explore(&parse(src).unwrap(), 10_000)
    }

    fn messages(findings: &[Finding]) -> Vec<String> {
        findings.iter().map(|f| format!("{}: {}", f.line, f.message)).collect()
    }

    #[test]
    fn the_demo_is_clean() {
        let src = include_str!("../../../examples/demo.quill.md");
        let r = test(src);
        assert!(r.errors.is_empty() && r.warnings.is_empty(), "{:#?}", (r.errors, r.warnings));
        assert!(!r.partial && r.situations > 5 && !r.endings.is_empty());
    }

    #[test]
    fn the_stats_demo_is_clean_and_every_roll_is_tried() {
        let r = test(include_str!("../../../examples/stats.quill.md"));
        assert!(r.errors.is_empty() && r.warnings.is_empty(), "{:#?}", (r.errors, r.warnings));
        assert!(!r.partial && r.situations > 100 && r.endings.len() == 1);
    }

    #[test]
    fn a_branch_on_typed_text_is_played() {
        let src = "---\nvars: { name: \"\" }\n---\n::input{var=name prompt=\"Name?\" default=\"Alex\"}\n\
                   ::if{name == \"Bob\"}\n::jump{to=secret}\n::endif\nNormal.\n::end\n# secret\nSecret ending.\n";
        let r = test(src);
        assert!(r.errors.is_empty() && r.warnings.is_empty(), "{:?}", messages(&r.warnings));
        assert_eq!(r.endings.len(), 2);
    }

    #[test]
    fn every_branch_is_played_and_endings_are_counted() {
        let r = test("# a\nHi.\n::choice\n- [Left](#l)\n- [Right](#r)\n::\n# l\nL.\n::end\n# r\nR.\n");
        assert_eq!(r.endings.len(), 2, "::end and the end of the script");
        assert!(r.errors.is_empty() && r.warnings.is_empty(), "{:?}", messages(&r.warnings));
    }

    #[test]
    fn a_loop_without_a_way_out_is_found_with_the_way_there() {
        let src = "# a\nStart.\n::choice\n- [Go round](#round)\n- [Leave](#out)\n::\n# round\nAround.\n::choice\n- [Again](#round)\n::\n# out\nBye.\n";
        let r = test(src);
        assert_eq!(r.errors.len(), 1, "{:?}", messages(&r.errors));
        assert!(r.errors[0].message.contains("never reach an ending"));
        assert_eq!(r.errors[0].path.as_deref(), Some(&["Go round".to_string()][..]));
    }

    #[test]
    fn a_hub_menu_with_an_exit_is_fine() {
        let src = "# hub\n::choice\n- [Talk](#talk)\n- [Leave](#bye)\n::\n# talk\nChat.\n::jump{to=hub}\n# bye\nBye.\n";
        let r = test(src);
        assert!(r.errors.is_empty(), "{:?}", messages(&r.errors));
    }

    #[test]
    fn silent_loops_hang_and_runaway_calls_are_errors() {
        let r = test("# a\nHi.\n# spin\n::jump{to=spin}\n");
        assert!(r.errors[0].message.contains("hangs"), "{:?}", messages(&r.errors));
        let r = test("# a\nHi.\n# deep\n::call{to=deep}\n");
        assert!(r.errors[0].message.contains("256 deep"), "{:?}", messages(&r.errors));
    }

    #[test]
    fn a_call_region_whose_label_jumps_back_to_its_screen_is_a_deep_error() {
        let src = "# hub\n::bg{src=map.png}\n::hotspots\n- [Door](#door){rect=0,0,0.3,0.3, call}\n- [Leave](#bye){rect=0.7,0,0.3,0.3}\n::\n# door\nA door.\n::jump{to=hub}\n# bye\nBye.\n";
        let r = test(src);
        assert!(r.errors.iter().any(|e| e.message.contains("256 deep")), "{:?}", messages(&r.errors));
    }

    #[test]
    fn a_replay_no_branch_reaches_can_never_be_unlocked() {
        let src = "---\nreplays: { seen: \"Seen\", hidden: \"Hidden\", lost: \"Lost\" }\n---\n# start\nHi.\n::choice\n- [Go](#seen)\n::\n# seen\nSeen.\n::end_replay\n::end\n# hidden\nH.\n::end\n# lost\n::jump{to=hidden}\n";
        let w = messages(&test(src).warnings);
        assert!(w.iter().any(|m| m.contains("replay `hidden` can never be unlocked")), "{w:?}");
        assert!(w.iter().any(|m| m.contains("replay `lost` can never be unlocked")), "{w:?}");
        assert!(!w.iter().any(|m| m.contains("replay `seen`")), "{w:?}");
        let fine = test(&src.replace("::choice\n- [Go](#seen)\n::\n", "::choice\n- [Go](#seen)\n- [Hid](#hidden)\n- [Lost](#lost)\n::\n"));
        assert!(!messages(&fine.warnings).iter().any(|m| m.contains("replay `")), "{:?}", messages(&fine.warnings));
        // `::end_replay` is a no-op for the tester: the story runs on to the end
        assert!(test(src).errors.is_empty());
    }

    #[test]
    fn unreachable_labels_lines_and_options_are_warned_about() {
        let src = "---\nvars: { met: false }\n---\n# a\nHi.\n::choice\n- [Secret](#b){if met}\n- [Plain](#c)\n::\n# b\nB.\n# c\nC.\n::end\nNever.\n# orphan\nAlone.\n";
        let w = messages(&test(src).warnings);
        assert!(w.iter().any(|m| m.contains("\"Secret\" is never offered")), "{w:?}");
        assert!(w.iter().any(|m| m.contains("`b` is never reached")), "{w:?}");
        assert!(w.iter().any(|m| m.contains("`orphan` is never reached")), "{w:?}");
        assert!(w.iter().any(|m| m.contains("never shown: \"Never.\"")), "{w:?}");
        assert!(!w.iter().any(|m| m.contains("Alone")), "covered by the label: {w:?}");
    }

    #[test]
    fn a_notice_text_is_checked_for_unknown_variables() {
        let w = messages(&test("---\nvars: { trust: 0 }\n---\n::notify{text=\"{trust} {nmae}\"}\nEnd.\n").warnings);
        assert!(w.iter().any(|m| m.contains("`{nmae}` isn't a variable")), "{w:?}");
        assert!(!w.iter().any(|m| m.contains("trust")), "{w:?}");
    }

    #[test]
    fn variables_read_but_never_set_are_typos() {
        let src = "---\nvars: { trust: 0 }\n---\n# a\n::set{trust+=1}\n::jump{to=a2 if=\"trsut > 0\"}\nHi {nmae}, {trust} \\{x}.\n# a2\nEnd.\n";
        let w = messages(&test(src).warnings);
        assert!(w.iter().any(|m| m.contains("`trsut` is never set")), "{w:?}");
        assert!(w.iter().any(|m| m.contains("`{nmae}` isn't a variable")), "{w:?}");
        assert!(!w.iter().any(|m| m.contains("trust`") || m.contains("{x}")), "{w:?}");
    }

    #[test]
    fn a_hotspot_hub_with_once_regions_terminates_and_reaches_its_endings() {
        let src = "---\nvars: { got: false }\n---\n# map\n::hotspots\n- [Room](#room){rect=0,0,0.3,0.3, once}\n- [Key](#key){rect=0.5,0.5,0.1,0.1, got=true, if not got}\n- [Exit](#out){rect=0.8,0.8,0.1,0.1}\n::\n# room\nA room.\n::jump{to=map}\n# key\nA key.\n::jump{to=map}\n# out\nBye.\n::end\n";
        let r = test(src);
        assert!(!r.partial && r.errors.is_empty(), "{r:?}");
        assert!(r.warnings.is_empty(), "{:?}", r.warnings);
        assert_eq!(r.endings.len(), 1);
        assert!(r.situations >= 4, "the hub is visited with and without each pick: {}", r.situations);
    }

    const SHOPPING: &str = "---\ninventory: bag\nvars: { gold: 40, bag: [], met: false }\n---\n::item{id=key price=15}\n::item{id=lamp price=30}\n::item{id=map price=5}\n# hub\n::shop{pay=gold into=bag}\n- key\n- lamp\n- map{if met}\n::\n::if{bag has \"key\"}\nKey.\n::endif\n::choice\n- [Back to the shop](#hub)\n- [Go](#out)\n::\n# out\nBye.\n::end\n";

    #[test]
    fn the_market_demo_is_clean() {
        let r = test(include_str!("../../../examples/market.quill.md"));
        assert!(r.errors.is_empty() && r.warnings.is_empty() && !r.partial, "{:#?}", (r.errors, r.warnings));
        assert_eq!(r.endings.len(), 2);
    }

    #[test]
    fn a_shop_hub_is_finite_and_every_purchase_and_leaving_are_branches() {
        let r = test(SHOPPING);
        assert!(!r.partial && r.errors.is_empty(), "{r:?}");
        // 40 gold buys the key, or the lamp, not both: three bags (none, key, lamp), each at the
        // shop and after it
        assert!(r.situations >= 6 && r.situations < 40, "{}", r.situations);
        assert_eq!(r.endings.len(), 1);
        let w = messages(&r.warnings);
        assert!(w.iter().any(|m| m.contains("\"map\" is never offered in this shop")), "{w:?}");
    }

    #[test]
    fn a_shop_that_can_be_bought_out_forever_still_terminates() {
        // 100 gold, three items of 10, and a way back: money only goes down, items are bought once
        let src = "---\nvars: { gold: 100, bag: [] }\n---\n::item{id=a price=10}\n::item{id=b price=10}\n::item{id=c price=10}\n# hub\n::shop{pay=gold into=bag}\n- a\n- b\n- c\n::\n::choice\n- [Again](#hub)\n- [Done](#out)\n::\n# out\nBye.\n";
        let r = explore(&parse(src).unwrap(), 10_000);
        assert!(!r.partial && r.errors.is_empty(), "{r:?}");
        assert!(r.situations <= 40, "{}", r.situations);
    }

    #[test]
    fn items_a_shop_sells_count_as_added_and_a_misspelt_one_is_still_caught() {
        let src = "---\nvars: { gold: 9, bag: [] }\n---\n::item{id=key price=1}\n::shop{pay=gold into=bag}\n- key\n::\n::if{bag has \"key\" and bag has \"kye\"}\nx\n::endif\n";
        let w = messages(&test(src).warnings);
        assert!(w.iter().any(|m| m.contains("`bag has \"kye\"` can never be true")), "{w:?}");
        assert!(!w.iter().any(|m| m.contains("\"key\"")), "{w:?}");
    }

    #[test]
    fn a_pay_variable_nothing_sets_is_flagged_and_an_inventory_screen_is_one_advance() {
        let src = "---\ninventory: bag\nvars: { bag: [] }\n---\n::item{id=a price=1}\n::shop{pay=goldd into=bag}\n- a\n::\n::inventory\nEnd.\n";
        let r = test(src);
        let w = messages(&r.warnings);
        assert!(w.iter().any(|m| m.contains("`goldd` is never set")), "{w:?}");
        assert!(r.errors.is_empty() && !r.partial && r.endings.len() == 1, "{r:?}");
    }

    #[test]
    fn a_journal_screen_is_one_advance_and_needs_no_coverage_of_its_own() {
        let src = "---\njournal: q\njournal_done: d\nvars: { q: [a], d: [] }\n---\n::item{id=a}\n::journal\n::set{d += \"a\"}\n::journal\n::if{d has \"a\"}\nDone.\n::endif\n";
        let r = test(src);
        assert!(r.errors.is_empty() && r.warnings.is_empty() && !r.partial && r.endings.len() == 1, "{r:?}");
    }

    #[test]
    fn empty_choices_and_stray_returns_are_warned_about() {
        let src = "# a\n::choice\n- [Only if](#a){if never}\n::\n::return\n";
        let w = messages(&test(src).warnings);
        assert!(w.iter().any(|m| m.contains("no option on offer")), "{w:?}");
        assert!(w.iter().any(|m| m.contains("`::return` with no `::call`")), "{w:?}");
    }

    #[test]
    fn visit_counts_in_a_hub_dont_explode_the_search() {
        let src = "# hub\n::choice\n- [Talk](#talk)\n- [Leave](#bye)\n::\n# talk\n::if{seen(talk) >= 3}\nNot again.\n::endif\nChat.\n::jump{to=hub}\n# bye\nBye.\n";
        let r = test(src);
        assert!(!r.partial && r.situations < 30, "{} situations", r.situations);
        assert!(r.errors.is_empty() && r.warnings.is_empty(), "{:?} {:?}", messages(&r.errors), messages(&r.warnings));
        // "Not again." needs the third visit, so the cap must not cut the count short.
        assert!(!messages(&r.warnings).iter().any(|m| m.contains("Not again")));
    }

    #[test]
    fn a_visit_count_read_only_by_a_shop_condition_stays_finite() {
        let src = "---\ninventory: bag\nvars: { gold: 100, bag: [] }\n---\n::item{id=a price=1}\n# hub\n::shop{pay=gold into=bag}\n- a{if seen(hub) >= 2}\n::\n::choice\n- [Again](#hub)\n- [Leave](#bye)\n::\n# bye\nBye.\n";
        let r = explore(&quillstone_script::parse(src).unwrap(), 200);
        assert!(!r.partial, "{} situations: the count must be capped", r.situations);
        assert!(r.situations < 40, "{}", r.situations);
    }

    #[test]
    fn once_only_options_disappear_until_none_are_left() {
        let src = "# hub\n::choice\n- [Ask about the scarf](#scarf){once}\n- [Ask about the city](#city){once}\n::\nNothing left to ask.\n::end\n# scarf\nIt was hers.\n::jump{to=hub}\n# city\nIt's big.\n::jump{to=hub}\n";
        let r = test(src);
        assert!(r.errors.is_empty() && r.warnings.is_empty(), "the hub runs dry by design: {:?}", messages(&r.warnings));
        assert_eq!(r.endings.len(), 1, "and then the story goes on to its end");
    }

    #[test]
    fn later_playthroughs_are_played_too() {
        let src = "---\npersistent: { done: false }\n---\n# a\n::if{done}\nAgain?\n::jump{to=spin}\n::endif\nHi.\n::set{done=true}\n::end\n# spin\n::jump{to=spin}\n";
        let r = test(src);
        assert!(!messages(&r.warnings).iter().any(|m| m.contains("Again?")), "reached in a later playthrough");
        assert_eq!(r.errors.len(), 1, "{:?}", messages(&r.errors));
        assert!(r.errors[0].message.contains("hangs"));
        assert_eq!(r.errors[0].path.as_deref(), Some(&["(a later playthrough, with done = true)".to_string()][..]));
    }

    #[test]
    fn growing_counters_stop_at_the_limit() {
        let src = "# a\n::set{n+=1}\nLine {n}.\n::jump{to=a}\n";
        let r = explore(&parse(src).unwrap(), 50);
        assert!(r.partial && r.situations == 50);
        assert!(r.errors.is_empty(), "no 'stuck' guesses from a partial run: {:?}", messages(&r.errors));
    }

    // ---- list variables ----

    const SHOP: &str = "---\nvars: { gold: 20, inv: [] }\n---\n# hub\n::choice\n- [Key](#hub){gold -= 10, inv += \"key\", if gold >= 10 and not inv has \"key\"}\n- [Lamp](#hub){gold -= 10, inv += \"lamp\", if gold >= 10 and not inv has \"lamp\"}\n- [Leave](#bye)\n::\n# bye\nBye.\n";

    #[test]
    fn a_shop_hub_with_buy_options_terminates() {
        let r = test(SHOP);
        assert!(r.errors.is_empty() && r.warnings.is_empty(), "{:?}", (messages(&r.errors), messages(&r.warnings)));
        assert!(!r.partial);
    }

    #[test]
    fn lists_with_the_same_items_in_another_order_are_one_situation() {
        // key-then-lamp and lamp-then-key hold the same set and the same gold
        let src = "---\nvars: { inv: [] }\n---\n# hub\n::choice\n- [A](#hub){inv += \"a\", if not inv has \"a\"}\n- [B](#hub){inv += \"b\", if not inv has \"b\"}\n- [Leave](#bye)\n::\n# bye\nBye.\n";
        let r = test(src);
        // {}, {a}, {b}, {a,b} at the hub, then the same four waiting at "Bye." and finished: 12
        // (without set-equality {a,b} and {b,a} would be two situations each time)
        assert_eq!(r.situations, 12, "{:?}", messages(&r.warnings));
        assert!(r.errors.is_empty());
    }

    #[test]
    fn more_items_mean_more_situations_but_the_limit_still_stops_the_search() {
        let mut src = String::from("---\nvars: { inv: [] }\n---\n# hub\n::choice\n");
        for i in 0..12 {
            src.push_str(&format!("- [Item {i}](#hub){{inv += \"i{i}\", if not inv has \"i{i}\"}}\n"));
        }
        src.push_str("- [Leave](#bye)\n::\n# bye\nBye.\n");
        let r = explore(&parse(&src).unwrap(), 500);
        assert!(r.partial && r.situations == 500, "2^12 subsets stop at the limit");
    }

    #[test]
    fn an_item_tested_but_never_added_is_a_likely_typo() {
        let src = "---\nvars: { inv: [start] }\n---\n# a\n::set{inv += \"key\"}\n::if{inv has \"kye\"}\nHm.\n::endif\n::if{inv has \"key\" and inv has \"start\"}\nOk.\n::endif\n::choice\n- [Go](#a){if inv has \"kye\"}\n- [Done](#z)\n::\n# z\nEnd.\n";
        let r = test(src);
        let w = messages(&r.warnings);
        assert_eq!(w.iter().filter(|m| m.contains("\"kye\"")).count(), 1, "once per item: {w:?}");
        assert!(w.iter().any(|m| m.starts_with("6: `inv has \"kye\"` can never be true")), "{w:?}");
        assert!(!w.iter().any(|m| m.contains("\"key\"") || m.contains("\"start\"")), "{w:?}");
    }

    #[test]
    fn a_computed_add_silences_the_typo_check() {
        let src = "---\nvars: { inv: [], pick: \"x\" }\n---\n# a\n::set{inv += pick}\n::if{inv has \"anything\"}\nHm.\n::endif\n";
        let r = test(src);
        assert!(!messages(&r.warnings).iter().any(|m| m.contains("never be true")), "{:?}", messages(&r.warnings));
    }

    #[test]
    fn list_variables_count_as_set_and_read() {
        // read by `has` and shown in text, set only by `+=`: no "never set" warnings
        let src = "---\nvars: { inv: [] }\n---\n# a\n::set{inv += \"a\"}\nBag {inv}.\n::if{inv has \"a\"}\nYes.\n::endif\n";
        let r = test(src);
        assert!(r.warnings.is_empty(), "{:?}", messages(&r.warnings));
    }

    #[test]
    fn persistent_lists_are_also_tried_full() {
        let src = "---\npersistent: { got: [] }\n---\n# a\n::set{got += \"x\"}\n::if{got has \"y\"}\nH.\n::endif\n";
        let p = parse(src).unwrap();
        let starts = playthroughs(&p);
        assert_eq!(starts.len(), 2);
        assert_eq!(starts[1].1["got"], Value::List(vec!["x".into()]));
        assert!(starts[1].0.as_deref().unwrap().contains("got = x"));
        let big = format!("---\npersistent: {{ {} }}\n---\nHi.\n", (0..10).map(|i| format!("l{i}: []")).collect::<Vec<_>>().join(", "));
        assert!(playthroughs(&parse(&big).unwrap()).len() <= 64);
    }

    #[test]
    fn the_shop_demo_is_clean() {
        let r = test(include_str!("../../../examples/shop.quill.md"));
        assert!(r.errors.is_empty() && r.warnings.is_empty(), "{:#?}", (r.errors, r.warnings));
        assert!(!r.partial && r.endings.len() == 1);
    }

    #[test]
    fn the_staging_demo_is_clean_and_nw_is_not_a_variable() {
        let r = test(include_str!("../../../examples/staging.quill.md"));
        assert!(r.errors.is_empty() && r.warnings.is_empty(), "{:#?}", (r.errors, r.warnings));
        assert!(!r.partial && r.endings.len() == 1);
    }

    #[test]
    fn the_tester_treats_the_default_of_a_timed_choice_as_an_ordinary_option() {
        let src = "# a\n::choice{time=5}\n- [Speak](#spoke)\n- [Stay quiet](#quiet){default}\n::\n# spoke\nSpoke.\n::end\n# quiet\nQuiet.\n::end\n";
        let r = test(src);
        assert!(!r.partial && r.errors.is_empty() && r.warnings.is_empty(), "{:?}", (messages(&r.errors), messages(&r.warnings)));
        assert_eq!(r.endings.len(), 2, "both options are played, whatever the timer");
    }

    #[test]
    fn the_tester_plays_the_timeout_of_a_timed_hotspot_screen_as_one_more_branch() {
        let src = "# a\n::hotspots{time=3 default=#slow}\n- [Door](#door){rect=0,0,0.1,0.1}\n::\n# door\nDoor.\n::end\n# slow\nToo slow.\n::end\n";
        let r = test(src);
        assert!(!r.partial && r.errors.is_empty() && r.warnings.is_empty(), "{:?}", (messages(&r.errors), messages(&r.warnings)));
        assert_eq!(r.endings.len(), 2, "the door and the timeout");
    }

    #[test]
    fn a_sprite_hotspot_counts_as_offered_only_while_its_sprite_is_shown() {
        let screen = "::hotspots\n- [Bug](#caught){sprite=bug}\n- [Door](#door){rect=0,0,0.1,0.1}\n::\n# caught\nCaught.\n::end\n# door\nDoor.\n::end\n";
        let shown = test(&format!("::show{{id=bug src=bug.png}}\n# a\n{screen}"));
        assert!(shown.errors.is_empty() && shown.warnings.is_empty(), "{:?}", (messages(&shown.errors), messages(&shown.warnings)));
        assert_eq!(shown.endings.len(), 2, "caught and door");
        let hidden = test(&format!("::show{{id=bug src=bug.png}}\n::hide{{bug}}\n# a\n{screen}"));
        assert_eq!(hidden.endings.len(), 1, "only the door can be clicked");
        // shown on one path only: both branches are explored, each with what is on stage
        let src = "---\nvars: { lit: false }\n---\n# a\n::choice\n- [Light](#on)\n- [Dark](#off)\n::\n# on\n::show{id=bug src=bug.png}\n::jump{to=map}\n# off\n::jump{to=map}\n# map\n::hotspots\n- [Bug](#caught){sprite=bug}\n- [Door](#door){rect=0,0,0.1,0.1}\n::\n# caught\nCaught.\n::end\n# door\nDoor.\n::end\n";
        let r = test(src);
        assert!(r.errors.is_empty(), "{:?}", messages(&r.errors));
        assert_eq!(r.endings.len(), 2, "caught (lit path) and door");
    }

    #[test]
    fn a_parameterized_hub_terminates_and_its_parameters_count_as_set() {
        let src = "---\nvars: { gold: 3, item: \"\" }\n---\n# hub\n::choice\n- [Look at the lamp](#lamp)\n- [Look at the map](#map)\n- [Leave](#out)\n::\n# lamp\n::call{to=price_line item=\"lamp\" cost=gold}\n::jump{to=hub}\n# map\n::call{to=price_line item=\"map\" cost=5}\n::jump{to=hub}\n# price_line(item, cost)\n**A:** The {item} costs {cost}.\n::return\n# out\nBye.\n::end\n";
        let r = test(src);
        assert!(!r.partial && r.errors.is_empty(), "{:?}", messages(&r.errors));
        assert!(r.warnings.is_empty(), "nothing new to warn about: {:?}", messages(&r.warnings));
        assert_eq!(r.endings.len(), 1);
        assert!(r.situations >= 4, "the hub is seen with each parameter set: {}", r.situations);
    }

    #[test]
    fn a_misspelt_variable_in_an_argument_is_still_a_typo_and_new_media_are_no_ops() {
        let src = "::sfx{src=r.ogg loop}\n::flash{time=1}\n::call{to=f x=glod}\n::sfx{stop}\n**A:** {x}\n::end\n# f(x)\n::return\n";
        let r = test(src);
        assert!(r.errors.is_empty(), "{:?}", messages(&r.errors));
        let w = messages(&r.warnings);
        assert!(w.iter().any(|m| m.contains("`glod` is never set")), "{w:?}");
        assert_eq!(w.len(), 1, "{w:?}");
    }

    // ---- random and expressions ----

    #[test]
    fn a_small_random_range_is_a_branch_per_value() {
        let src = "---\nvars: { roll: 0 }\n---\n::set{roll = random(1, 3)}\n::if{roll == 1}\nOne.\n::elif{roll == 2}\nTwo.\n::else\nThree.\n::endif\n::end\n";
        let r = test(src);
        assert!(r.errors.is_empty() && r.warnings.is_empty(), "{:?}", messages(&r.warnings));
        assert_eq!(r.situations, 3 * 2, "per value: the line shown, then the end");
        let p = parse(src).unwrap();
        assert_eq!(explore(&p, 10_000).endings.len(), 1);
    }

    #[test]
    fn every_value_of_a_roll_reaches_its_own_line() {
        // a line only the highest roll shows is not reported as unreachable
        let src = "---\nvars: { roll: 0 }\n---\n::set{roll = random(1, 12)}\n::if{roll == 12}\nCritical!\n::endif\nDone.\n::end\n";
        let r = test(src);
        assert!(r.warnings.is_empty(), "{:?}", messages(&r.warnings));
        assert_eq!(r.situations, 12 * 2 + 1, "per value a line and an end, and one more line for the 12");
    }

    #[test]
    fn two_rolls_in_one_step_multiply() {
        let src = "---\nvars: { a: 0, b: 0 }\n---\n::set{a = random(1, 3), b = random(1, 2)}\nHi {a} {b}.\n::end\n";
        assert_eq!(test(src).situations, 6 * 2, "3 x 2 combinations, each a line and an end");
    }

    #[test]
    fn a_choice_effect_rolls_branch_too() {
        let src = "---\nvars: { dmg: 0 }\n---\n# a\n::choice\n- [Hit](#b){dmg = random(2, 4)}\n::\n# b\n::if{dmg == 4}\nBig.\n::endif\n::end\n";
        let r = test(src);
        assert!(r.errors.is_empty() && r.warnings.is_empty(), "{:?}", messages(&r.warnings));
        assert_eq!(r.situations, 1 + 3 + 1, "the choice, one situation per value, and the end after the line that 4 shows");
    }

    #[test]
    fn a_wide_range_tries_three_values_and_warns() {
        let src = "---\nvars: { roll: 0 }\n---\n::set{roll = random(1, 100)}\nRolled {roll}.\n::end\n";
        let r = test(src);
        assert_eq!(r.situations, 3 * 2, "lo, mid, hi: each a line and an end");
        let w = messages(&r.warnings);
        assert!(w.len() == 1 && w[0].starts_with("4: ") && w[0].contains("random(1, 100)") && w[0].contains("only 1, 50 and 100"), "{w:?}");
        assert!(r.errors.is_empty() && !r.partial);
        // exactly at the limit of the small range, no warning
        let r = test("---\nvars: { roll: 0 }\n---\n::set{roll = random(1, 12)}\nRolled {roll}.\n::end\n");
        assert!(r.warnings.is_empty() && r.situations == 12 * 2);
        let r = test("---\nvars: { roll: 0 }\n---\n::set{roll = random(1, 13)}\nRolled {roll}.\n::end\n");
        assert_eq!((r.warnings.len(), r.situations), (1, 3 * 2));
    }

    #[test]
    fn a_pick_is_a_branch_per_value_and_an_omitted_default_is_followed() {
        let src = "---\nvars: { bag: [a, b, c], r: \"\" }\n---\n::set{r = pick(bag)}\n::if{r == \"c\"}\nOnly c.\n::endif\n::set{r = pick(\"x\", \"y\")}\n\
                   ::if{r == \"y\"}\nOnly y.\n::endif\nDone.\n::end\n";
        let r = test(src);
        assert!(r.errors.is_empty() && r.warnings.is_empty(), "{:?}", messages(&r.warnings));
        let src = "---\nvars: { p: 0 }\n---\n::call{to=hit}\n::call{to=hit power=5}\n::end\n# hit(power=2)\n::if{power == 2}\nDefault.\n::elif{power == 5}\n\
                   Given.\n::else\nNever.\n::endif\n::return\n";
        let r = test(src);
        assert!(r.errors.is_empty(), "{:?}", messages(&r.errors));
        assert!(r.warnings.iter().any(|w| w.message.contains("Never")), "only the impossible line is unreachable: {:?}", messages(&r.warnings));
        assert!(!r.warnings.iter().any(|w| w.message.contains("Default") || w.message.contains("Given")), "{:?}", messages(&r.warnings));
    }

    #[test]
    fn setting_a_list_changes_what_the_tester_follows() {
        let src = "---\nvars: { bag: [key] }\n---\n::set{bag = []}\n::if{bag has \"key\"}\nStill.\n::endif\n::set{bag = [\"key\"]}\n\
                   ::if{bag has \"key\"}\nBack.\n::endif\n::end\n";
        let r = test(src);
        let w = messages(&r.warnings);
        assert!(r.errors.is_empty() && w.len() == 1 && w[0].contains("Still"), "{w:?}");
    }

    #[test]
    fn the_seed_is_not_part_of_a_situation_so_a_rolling_loop_ends() {
        let src = "---\nvars: { r: 0 }\n---\n# a\n::set{r = random(1, 2)}\nRoll {r}.\n::choice\n- [Again](#a)\n- [Stop](#b)\n::\n# b\nBye.\n::end\n";
        let r = test(src);
        assert!(!r.partial && r.errors.is_empty(), "{:?}", messages(&r.errors));
        assert!(r.situations <= 12, "a handful of situations, not one per roll: {}", r.situations);
    }

    #[test]
    fn the_limit_still_stops_a_growing_counter_with_random_in_it() {
        let src = "---\nvars: { total: 0 }\n---\n# a\n::set{total += random(1, 3)}\nTotal {total}.\n::choice\n- [Again](#a)\n::\n";
        let r = explore(&parse(src).unwrap(), 50);
        assert!(r.partial, "total keeps growing");
        assert!(r.situations <= 50, "{}", r.situations);
    }

    #[test]
    fn branching_never_exceeds_the_limit() {
        let src = "---\nvars: { a: 0, b: 0, c: 0 }\n---\n::set{a = random(1, 12), b = random(1, 12), c = random(1, 12)}\nHi.\n::end\n";
        let r = explore(&parse(src).unwrap(), 100);
        assert!(r.partial && r.situations <= 100, "{} {}", r.partial, r.situations);
    }

    #[test]
    fn a_variable_in_a_picture_name_counts_as_read() {
        let src = "---\nvars: { place: \"a\" }\n---\n::character{id=mika sprites=m_{pose}.png}\n::bg{src=\"bg/{place}.png\"}\n::show{mika {mood}}\n![](i/{item}.png)\nHi.\n::end\n";
        let w = messages(&test(src).warnings);
        let said = |needle: &str| w.iter().filter(|m| m.contains(needle)).count();
        assert_eq!(said("`mood` is never set"), 1, "{w:?}");
        assert_eq!(said("`item` is never set"), 1, "{w:?}");
        assert_eq!(said("`place`"), 0, "{w:?}");
    }

    #[test]
    fn the_tester_walks_a_story_with_unsafe_picture_names_without_trouble() {
        let src = "---\nvars: { place: \"../x\" }\n---\n::bg{src=\"bg/{place}.png\"}\nHi.\n::end\n";
        let r = test(src);
        assert!(r.errors.is_empty() && !r.partial, "{:?}", r.errors);
    }

    #[test]
    fn variable_warnings_understand_expressions_and_dotted_names() {
        let src = "---\nvars: { hero.love: 0, gold: 5 }\n---\n::set{hero.love += gold * 2, bonus = max(hero.lvoe, 1)}\nLove {hero.love}, {hero.lvoe}.\n::if{hero.love + extra > 3}\nYes.\n::endif\n::end\n";
        let w = messages(&test(src).warnings);
        let said = |needle: &str| w.iter().filter(|m| m.contains(needle)).count();
        assert_eq!(said("`hero.lvoe` is never set"), 1, "{w:?}");
        assert_eq!(said("`extra` is never set"), 1, "{w:?}");
        assert_eq!(said("hero.love"), 0, "{w:?}");
        assert_eq!(said("gold"), 0, "{w:?}");
        assert_eq!(said("bonus"), 0, "set by the story");
    }

    #[test]
    fn count_of_an_undeclared_list_is_refused_before_the_tester_runs() {
        let src = "---\nvars: { bag: [] }\n---\n::set{n = count(bag) + count(bag.x)}\nHi.\n::end\n";
        // `count(bag.x)` isn't declared: the compiler already refuses it
        assert!(parse(src).is_err());
    }

    #[test]
    fn re_simulated_rolls_are_charged_to_the_limit() {
        // five d6 in one step: 7,776 outcomes, 9,331 step runs, every time round the loop
        let src = "---\nvars: { a: 0, b: 0, c: 0, d: 0, e: 0 }\n---\n# loop\n::set{a=random(1,6), b=random(1,6), c=random(1,6), d=random(1,6), e=random(1,6)}\nRoll {a} {b} {c} {d} {e}.\n::choice\n- [Again](#loop)\n- [Stop](#fin)\n::\n# fin\n::end\n";
        let started = Instant::now();
        let r = explore(&parse(src).unwrap(), 2_000);
        assert!(r.partial);
        assert!(r.situations + r.reruns <= 2_001, "{} situations + {} re-runs", r.situations, r.reruns);
        assert!(started.elapsed().as_secs() < 5);
        // without rolls nothing is re-run
        assert_eq!(test("Hi.\n::end\n").reruns, 0);
    }

    #[test]
    fn the_room_for_a_fork_is_what_is_left_of_the_budget() {
        let src = "---\nvars: { a: 0, b: 0, c: 0 }\n---\n# loop\n::set{a=random(1,12), b=random(1,12), c=random(1,12)}\nRoll {a} {b} {c}.\n::end\n";
        for limit in [5, 40, 300] {
            let r = explore(&parse(src).unwrap(), limit);
            assert!(r.partial && r.situations + r.reruns <= limit + 1, "limit {limit}: {} + {}", r.situations, r.reruns);
        }
    }

    #[test]
    fn a_click_wait_is_one_advance_like_a_timed_one() {
        let timed = test("Hi.\n::wait{time=1}\nBye.\n::end\n");
        let click = test("Hi.\n::wait{click}\nBye.\n::end\n");
        assert!(click.errors.is_empty() && !click.partial);
        assert_eq!((click.situations, click.endings.len()), (timed.situations, 1));
    }
}
