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

use std::collections::{BTreeSet, HashMap, HashSet, VecDeque};

use quillstone_script::{is_ident, Expr, Op, Program, Value, Vars, ONCE, SEEN};

use crate::{offered_stock, step_traced, visible_options, Cmd, Input, State};

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
        reported: HashSet::new(),
        endings: BTreeSet::new(),
        report: Report::default(),
        caps: seen_caps(program),
    };
    for (origin, kept) in playthroughs(program) {
        let mut state = State::new(program);
        state.vars.extend(kept);
        if let Some(root) = x.visit(state, None, None) {
            x.nodes[root].origin = origin;
        }
    }
    while let Some(id) = x.queue.pop_front() {
        let state = x.nodes[id].state.take().expect("queued once, with its state");
        let inputs: Vec<(Input, Option<String>)> = match &state.pending {
            Some(pending @ (Cmd::ShowChoices { .. } | Cmd::ShowHotspots { .. })) => {
                if let Some(Op::Choice { options: all }) = program.ops.get(state.pc) {
                    x.offered.extend(visible_options(all, &state.vars).into_iter().map(|i| (state.pc, i)));
                }
                let labels = match pending {
                    Cmd::ShowChoices { options } => options.clone(),
                    Cmd::ShowHotspots { spots } => spots.iter().map(|s| s.label.clone()).collect(),
                    _ => Vec::new(),
                };
                labels.into_iter().enumerate().map(|(i, text)| (Input::Choose(i), Some(text))).collect()
            }
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
            Some(_) => vec![(Input::Advance, None)],
            None => Vec::new(),
        };
        for (input, label) in inputs {
            if let Some(next) = x.visit(state.clone(), Some(input), Some((id, label))) {
                x.nodes[id].next.push(next);
            }
        }
    }
    x.finish()
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
    index: HashMap<(usize, Vec<usize>, Vars, bool), usize>,
    queue: VecDeque<usize>,
    /// Ops that ever ran.
    ran: Vec<bool>,
    /// (choice op, option index) pairs that were ever on offer.
    offered: HashSet<(usize, usize)>,
    /// (shop op, stock line) pairs that were ever on offer.
    shop_offered: HashSet<(usize, usize)>,
    /// (line, kind) already reported, to say each thing once.
    reported: HashSet<(usize, &'static str)>,
    endings: BTreeSet<usize>,
    report: Report,
    /// The most each `seen:` count can matter (see the module notes).
    caps: Vec<(String, i64)>,
}

impl Explorer<'_> {
    /// Step from `state` with `input` and file the situation it leads to.
    fn visit(&mut self, mut state: State, input: Option<Input>, parent: Option<(usize, Option<String>)>) -> Option<usize> {
        let mut trace = Vec::new();
        step_traced(&mut state, self.program, input, &mut |pc| trace.push(pc));
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
        if self.nodes.len() >= self.limit {
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
            let Op::Choice { options } = &self.program.ops[at] else { continue };
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
            self.never_offered();
        }
        self.unset_variables();
        self.unadded_items();
        let mut r = self.report;
        r.situations = self.nodes.len();
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
            let Op::Choice { options } = op else { continue };
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
            match op {
                Op::Set(effect) => {
                    set.insert(&effect.var);
                    read.extend(names(|out| effect.value.vars_read(out)).into_iter().map(|n| (at, n, false)));
                }
                Op::Jump { cond: Some(cond), .. } => {
                    read.extend(names(|out| cond.vars_read(out)).into_iter().map(|n| (at, n, false)));
                }
                Op::Say { text, .. } => read.extend(braces(text).into_iter().map(|n| (at, n, true))),
                Op::Shop { pay, into, stock } => {
                    set.insert(into);
                    read.push((at, pay, false));
                    for c in stock.iter().filter_map(|s| s.cond.as_ref()) {
                        read.extend(names(|out| c.vars_read(out)).into_iter().map(|n| (at, n, false)));
                    }
                }
                Op::Choice { options } => {
                    for o in options {
                        set.extend(o.effects.iter().map(|e| e.var.as_str()));
                        for e in &o.effects {
                            read.extend(names(|out| e.value.vars_read(out)).into_iter().map(|n| (at, n, false)));
                        }
                        if let Some(cond) = &o.cond {
                            read.extend(names(|out| cond.vars_read(out)).into_iter().map(|n| (at, n, false)));
                        }
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
            Expr::Lit(_) | Expr::Var(_) => {}
        }
    }
    let mut caps = HashMap::new();
    for op in &program.ops {
        match op {
            Op::Jump { cond: Some(c), .. } => tested(&mut caps, c),
            Op::Set(e) => uncapped(&mut caps, &e.value),
            Op::Choice { options } => {
                for o in options {
                    if let Some(c) = &o.cond {
                        tested(&mut caps, c);
                    }
                    for e in &o.effects {
                        uncapped(&mut caps, &e.value);
                    }
                }
            }
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
        Op::Choice { options } => options.iter().flat_map(|o| &o.effects).collect(),
        _ => Vec::new(),
    })
}

/// The starting points: a first playthrough, then each combination of values the
/// script assigns its persistent variables (as a later playthrough could have
/// left them), described for messages. At most 64 in all.
fn playthroughs(program: &Program) -> Vec<(Option<String>, Vars)> {
    let mut choices: Vec<(&str, Vec<Value>)> = Vec::new();
    for name in &program.persistent {
        let mut values: Vec<Value> = program.vars.get(name).cloned().into_iter().collect();
        let assigned = program.ops.iter().flat_map(|op| match op {
            Op::Set(e) => vec![e],
            Op::Choice { options } => options.iter().flat_map(|o| &o.effects).collect(),
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
        if let Some(Some(all)) = added_items(program).remove(name.as_str()) {
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
        } else if let Some(after) = rest.strip_prefix('{') {
            let Some(end) = after.find('}') else { break };
            let name = after[..end].trim();
            if is_ident(name) {
                out.push(name);
            }
            rest = &after[end + 1..];
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
}
