//! The developer console (`--dev`): a one-line entry at the top of the screen and what it answers.
//! Here is the pure part: the commands, how values are shown, and the drawing. Running a command
//! on the game is in `game/console.rs`.

use quillstone_render::{Align, Draw, Rect, Theme, VIRTUAL};
use quillstone_script::{is_ident, Value, Vars};

use crate::input::{caret_visible, line, text, visible_tail};

/// The most characters one command may have.
pub const MAX_LEN: usize = 200;
/// The most variables `vars` lists at once.
pub const MAX_VARS: usize = 10;

const SIZE: f32 = 30.0;
const ROW: f32 = 40.0;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    Get(String),
    /// Prefix filter (may be empty).
    Vars(String),
    /// An assignment as `::set` writes it: `gold = 5`, `gold += 1`.
    Set(String),
    Jump(String),
    Help,
}

pub const HELP: [&str; 5] = [
    "get <var>   (or just <var>)   show a value",
    "vars [prefix]                 list variables",
    "set <var> = <expr>            change a variable (also += and -=)",
    "jump <label>                  go there now, keeping the variables",
    "help; Esc or F12 closes",
];

pub fn parse(input: &str) -> Result<Command, String> {
    let input = input.trim();
    let (word, rest) = input.split_once(char::is_whitespace).map_or((input, ""), |(w, r)| (w, r.trim()));
    let need = |what: &str| if rest.is_empty() { Err(format!("`{word}` needs {what}")) } else { Ok(rest.to_string()) };
    match word {
        "help" | "?" => Ok(Command::Help),
        "vars" => Ok(Command::Vars(rest.to_string())),
        "get" => need("a variable name").map(Command::Get),
        "set" => need("an assignment like `gold = 5`").map(Command::Set),
        "jump" => need("a label").map(Command::Jump),
        _ if is_ident(input) => Ok(Command::Get(input.to_string())),
        _ => Err(format!("unknown command `{word}` (try `help`)")),
    }
}

/// A value as a script writes it: text quoted, a list in brackets.
pub fn show_value(v: &Value) -> String {
    match v {
        Value::Str(s) => format!("\"{s}\""),
        Value::List(items) => format!("[{}]", items.join(", ")),
        other => other.to_string(),
    }
}

/// `name = value`, or that the variable isn't set.
pub fn show_var(vars: &Vars, name: &str) -> String {
    vars.get(name).map_or_else(|| format!("{name} is not set"), |v| format!("{name} = {}", show_value(v)))
}

/// The lines `vars [prefix]` answers: at most [`MAX_VARS`], sorted. The engine's own counters
/// (`seen:...`, `once:...`) only show when the prefix asks for them.
pub fn list_vars(vars: &Vars, prefix: &str) -> Vec<String> {
    let mut shown: Vec<String> = vars
        .iter()
        .filter(|(k, _)| k.starts_with(prefix) && (!prefix.is_empty() || is_ident(k)))
        .map(|(k, v)| format!("{k} = {}", show_value(v)))
        .collect();
    if shown.len() > MAX_VARS {
        let more = shown.len() - MAX_VARS;
        shown.truncate(MAX_VARS);
        shown.push(format!("... {more} more (give a prefix: `vars hero.`)"));
    }
    if shown.is_empty() {
        shown.push("no such variables".into());
    }
    shown
}

/// What the console holds while it is open.
#[derive(Debug, Clone, Default)]
pub struct Console {
    pub text: String,
    /// The last command's answer, one line each.
    pub lines: Vec<String>,
    /// Seconds since the last keystroke, for the caret.
    pub since_key: f32,
}

pub fn draw_console(out: &mut Vec<Draw>, theme: &Theme, c: &Console) {
    let h = ROW * (c.lines.len() + 1) as f32 + 20.0;
    line(out, Rect::new(0.0, 0.0, VIRTUAL.w, h), [0.0, 0.0, 0.0, 0.92]);
    let mut y = 10.0;
    for l in &c.lines {
        text(out, visible_tail(l, SIZE, VIRTUAL.w - 60.0), Rect::new(30.0, y, VIRTUAL.w - 60.0, ROW), SIZE, theme.dim_color, Align::Left);
        y += ROW;
    }
    let caret = if caret_visible(c.since_key) { "|" } else { "" };
    let typed = format!("> {}{caret}", visible_tail(&c.text, SIZE, VIRTUAL.w - 120.0));
    text(out, typed, Rect::new(30.0, y, VIRTUAL.w - 60.0, ROW), SIZE, theme.text_color, Align::Left);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vars(list: &[(&str, Value)]) -> Vars {
        list.iter().map(|(k, v)| (k.to_string(), v.clone())).collect()
    }

    #[test]
    fn commands_parse() {
        assert_eq!(parse("get gold"), Ok(Command::Get("gold".into())));
        assert_eq!(parse("  hero.love "), Ok(Command::Get("hero.love".into())));
        assert_eq!(parse("vars"), Ok(Command::Vars(String::new())));
        assert_eq!(parse("vars hero."), Ok(Command::Vars("hero.".into())));
        assert_eq!(parse("set gold = 5"), Ok(Command::Set("gold = 5".into())));
        assert_eq!(parse("jump The Roof"), Ok(Command::Jump("The Roof".into())));
        assert_eq!(parse("help"), Ok(Command::Help));
    }

    #[test]
    fn bad_commands_say_why() {
        assert!(parse("get").unwrap_err().contains("needs a variable"));
        assert!(parse("set").unwrap_err().contains("needs an assignment"));
        assert!(parse("jump").unwrap_err().contains("needs a label"));
        assert!(parse("fly away").unwrap_err().contains("unknown command `fly`"));
        assert!(parse("").unwrap_err().contains("unknown command"));
    }

    #[test]
    fn values_are_shown_as_a_script_writes_them() {
        let v = vars(&[("n", Value::Int(3)), ("s", Value::Str("a b".into())), ("l", Value::List(vec!["x".into(), "y".into()]))]);
        assert_eq!(show_var(&v, "n"), "n = 3");
        assert_eq!(show_var(&v, "s"), "s = \"a b\"");
        assert_eq!(show_var(&v, "l"), "l = [x, y]");
        assert_eq!(show_var(&v, "z"), "z is not set");
    }

    #[test]
    fn the_list_is_capped_filtered_and_hides_engine_counters() {
        let mut v = Vars::new();
        for i in 0..15 {
            v.insert(format!("v{i:02}"), Value::Int(i));
        }
        v.insert("seen:start".into(), Value::Int(1));
        let all = list_vars(&v, "");
        assert_eq!(all.len(), MAX_VARS + 1);
        assert!(all[MAX_VARS].starts_with("... 5 more"));
        assert!(all.iter().all(|l| !l.starts_with("seen:")));
        assert_eq!(list_vars(&v, "v1").len(), 5);
        assert_eq!(list_vars(&v, "seen:"), ["seen:start = 1"]);
        assert_eq!(list_vars(&v, "zzz"), ["no such variables"]);
    }
}
