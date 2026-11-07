//! Developer tools on a [`Session`]: change a variable or jump to a label by hand (the player's
//! `--dev` console) and start a new game at a label with chosen variables (`--start`, `--set`).
//! Nothing here is reachable by a player of a released game unless the host turns it on.

use quillstone_script::{parse_effect, slug, Expr, Program, Value};

use crate::{refresh_pending, refused_write, run_effect, step, Cmd, Input, Session, State};

/// What the command line asks for: where a new game starts and which variables it starts with.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StartAt {
    pub label: Option<String>,
    /// `(name, value as typed)`, in the order given.
    pub sets: Vec<(String, String)>,
}

/// Take `--start <label>` and `--set var=value` (repeatable) out of `args`, wherever they stand.
/// The value is text as typed: [`Session::start_at`] reads it by the variable's declared type.
pub fn start_flags(args: &mut Vec<String>) -> Result<StartAt, String> {
    let mut out = StartAt::default();
    let mut rest = Vec::new();
    let mut it = std::mem::take(args).into_iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--start" => out.label = Some(it.next().ok_or("--start needs a label")?),
            "--set" => {
                let pair = it.next().ok_or("--set needs var=value")?;
                let (name, value) = pair.split_once('=').ok_or(format!("--set needs var=value, not `{pair}`"))?;
                out.sets.push((name.trim().to_string(), value.trim().to_string()));
            }
            _ => rest.push(a),
        }
    }
    *args = rest;
    Ok(out)
}

fn kind(v: &Value) -> &'static str {
    match v {
        Value::Int(_) => "a number",
        Value::Bool(_) => "true/false",
        Value::Str(_) => "text",
        Value::List(_) => "a list",
    }
}

fn label_pc(program: &Program, label: &str) -> Result<usize, String> {
    let name = slug(label.trim().trim_start_matches('#'));
    program.labels.get(&name).copied().ok_or_else(|| {
        let known: Vec<&str> = program.labels.keys().map(String::as_str).take(12).collect();
        format!("no label `{name}` (labels: {})", known.join(", "))
    })
}

/// The value of `--set` text, by the variable's declared type.
fn typed(declared: &Value, raw: &str) -> Result<Value, String> {
    let unquoted = raw.strip_prefix('"').and_then(|r| r.strip_suffix('"')).unwrap_or(raw);
    Ok(match declared {
        Value::Str(_) => Value::Str(unquoted.to_string()),
        Value::Int(_) => Value::Int(raw.parse().map_err(|_| format!("`{raw}` is not a whole number"))?),
        Value::Bool(_) => Value::Bool(raw.parse().map_err(|_| format!("`{raw}` is not true or false"))?),
        Value::List(_) => return Err("a list can't be given here: change it in the console with `set name += \"item\"` or `set name = [\"a\", \"b\"]`".into()),
    })
}

/// Run the assignment `src` (`n = 3`, `n += 1`, `name = "Ann"`) on the state's variables,
/// refusing to change the type of a variable the script declares (or already holds).
fn set_checked(program: &Program, state: &mut State, src: &str) -> Result<(), String> {
    let effect = parse_effect(src)?;
    if let Some(message) = refused_write(&effect, &state.vars) {
        return Err(message);
    }
    // the variable a name with `{}` parts spells now
    let var = &effect.target(&state.vars).map_or_else(|| effect.var.clone(), |t| t.into_owned());
    let was = state.vars.get(var).or_else(|| program.vars.get(var)).cloned();
    let (mut vars, mut rng) = (state.vars.clone(), state.rng);
    run_effect(&effect, &mut vars, &mut rng, &mut None, &mut Vec::new());
    let now = vars.get(var);
    match (&was, now) {
        (Some(Value::List(_)), _) if matches!(effect.op, quillstone_script::AssignOp::Set) && !matches!(effect.value, Expr::Lit(Value::List(_))) => {
            return Err(format!("`{var}` is a list: change it with `{var} += \"item\"`, `{var} -= \"item\"` or `{var} = [\"a\", \"b\"]`"));
        }
        (Some(w), _) if !matches!(w, Value::List(_)) && matches!(effect.value, Expr::Lit(Value::List(_))) => {
            return Err(format!("`{var}` holds {}, not a list, so `= [..]` can't set it", kind(w)));
        }
        (Some(w), Some(n)) if std::mem::discriminant(w) != std::mem::discriminant(n) => {
            return Err(format!("`{var}` holds {}; it can't be changed to {}", kind(w), kind(n)));
        }
        _ => {}
    }
    state.vars = vars;
    state.rng = rng;
    Ok(())
}

impl<'p> Session<'p> {
    /// A new game for `--start` / `--set`: the script's variables with `at.sets` put in, an empty
    /// scene, starting at `at.label` (the top without one). `Err` for an unknown label, a
    /// bad value, or a variable the script doesn't declare (a typo would otherwise do nothing).
    pub fn start_at(program: &'p Program, persistent: quillstone_script::Vars, seed: u64, at: &StartAt) -> Result<(Session<'p>, Vec<Cmd>), String> {
        let pc = at.label.as_deref().map_or(Ok(0), |l| label_pc(program, l))?;
        let mut session = Session::fresh(program, persistent, seed);
        for (name, raw) in &at.sets {
            let declared = program.vars.get(name).ok_or(format!("--set {name}: the script declares no variable `{name}`"))?;
            let value = typed(declared, raw).map_err(|m| format!("--set {name}: {m}"))?;
            session.state.vars.insert(name.clone(), value);
        }
        session.state.pc = pc;
        let mut cmds = vec![Cmd::ClearScene];
        cmds.extend(step(&mut session.state, program, None));
        session.note(&cmds);
        Ok((session, cmds))
    }

    /// Developer console: run an assignment now. It is not a rollback step: going back to an
    /// earlier screen brings that screen's variables back.
    pub fn dev_set(&mut self, src: &str) -> Result<(), String> {
        set_checked(self.program, &mut self.state, src)?;
        refresh_pending(&mut self.state, self.program); // a shown line, choice or price uses variables
        self.note(&[]);
        Ok(())
    }

    /// Developer console: go to `label` now, keeping the variables, the call stack and the scene
    /// (as a `::jump` does). It is a normal step: rollback comes back to where the jump left.
    /// Returns the commands that redraw the screen.
    pub fn dev_jump(&mut self, label: &str) -> Result<Vec<Cmd>, String> {
        let pc = label_pc(self.program, label)?;
        let before = self.state.clone();
        self.state.pc = pc;
        self.state.pending = None;
        self.state.finished = false;
        self.state.inline.clear();
        self.state.voice = None;
        let cmds = step(&mut self.state, self.program, None);
        self.push_history(before, Input::Advance);
        self.redo.clear();
        self.note(&cmds);
        Ok(self.state.redraw())
    }
}
