//! The little expression language used in `::if`, `{if ...}`, `if=` and assignments.
//!
//! Grammar (lowest to highest precedence):
//! `or` / `||`, `and` / `&&`, `not` / `!`, comparison (`== != < <= > >=`, and a lone
//! `=` also means equality) or `list has item` (the same level; the left side is
//! a list variable, the right a text, variable or parenthesised expression), then
//! `+ -`, `* / %`, unary `-`, and literals, variables, function calls
//! (`min max abs count random pick`), `seen(label)` and `( ... )`.
//! Literals: whole numbers, `true` / `false`, and "text" or 'text'.
//! Variable names may have dotted parts: `hero.love`, and parts named by a variable: `love.{who}`.
//! `seen(label)` is how many times the story has passed that label (0: never);
//! it reads the hidden variable `seen:label`, which the runtime counts up.

use crate::ops::{AssignOp, BinOp, Cmp, Effect, Expr, Func, Value, FUNCTIONS, SEEN};

#[derive(Debug, Clone, PartialEq)]
enum Tok {
    Ident(String),
    /// Not yet signed: `-9223372036854775808` is a minus and a number one past `i64::MAX`.
    Int(u64),
    Str(String),
    Cmp(Cmp),
    Plus,
    Minus,
    Star,
    Slash,
    Percent,
    Comma,
    And,
    Or,
    Not,
    LParen,
    RParen,
    /// `seen(label)`, with the label already normalised.
    Seen(String),
}

fn describe(t: &Tok) -> String {
    match t {
        Tok::Ident(s) => s.clone(),
        Tok::Int(n) => n.to_string(),
        Tok::Str(s) => format!("\"{s}\""),
        Tok::Cmp(c) => c.symbol().to_string(),
        Tok::And => "and".into(),
        Tok::Or => "or".into(),
        Tok::Not => "not".into(),
        Tok::Plus => "+".into(),
        Tok::Minus => "-".into(),
        Tok::Star => "*".into(),
        Tok::Slash => "/".into(),
        Tok::Percent => "%".into(),
        Tok::Comma => ",".into(),
        Tok::LParen => "(".into(),
        Tok::RParen => ")".into(),
        Tok::Seen(label) => format!("seen({label})"),
    }
}

/// A letters-digits-underscore name that doesn't start with a digit (character ids,
/// label parameters, one part of a dotted variable name).
pub fn is_plain_ident(s: &str) -> bool {
    let mut chars = s.chars();
    match chars.next() {
        Some(c) if c.is_alphabetic() || c == '_' => chars.all(|c| c.is_alphanumeric() || c == '_'),
        _ => false,
    }
}

/// A valid variable name: a plain name, then more parts joined by single dots (`trust`, `hero.love`,
/// `stats.hp.max`); no empty part, so no leading, trailing or doubled dot. Parts after the first may start
/// with a digit (`slot.0`, `grid.1.2`), so a name spelled from a number (`slot.{i}`) can be declared.
pub fn is_ident(s: &str) -> bool {
    let mut parts = s.split('.');
    parts.next().is_some_and(is_plain_ident) && parts.all(|p| !p.is_empty() && p.chars().all(|c| c.is_alphanumeric() || c == '_'))
}

/// Words expressions give a meaning of their own, so they can't name a variable.
pub fn is_keyword(s: &str) -> bool {
    matches!(s, "true" | "false" | "and" | "or" | "not")
}

/// Deepest nesting of parentheses, function calls, `-` and `not` chains: the parser
/// is recursive and expressions come from saves (untrusted).
pub const MAX_NESTING: usize = 64;
/// Most tokens in one expression (a long list literal is fine), and most operators (`+ - * / % and or not`,
/// comparisons): every operator can add a level to the tree, and a flat chain (`1+1+1+...`) must not build
/// one deep enough to overflow a stack in eval, clone or drop.
pub const MAX_TOKENS: usize = 8192;
pub const MAX_OPERATORS: usize = 256;

fn tokenize(s: &str) -> Result<Vec<Tok>, String> {
    let chars: Vec<char> = s.chars().collect();
    let at = |i: usize| chars.get(i).copied();
    let mut out = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        if out.len() > MAX_TOKENS {
            return Err(format!("expression is too long (at most {MAX_TOKENS} parts)"));
        }
        let c = chars[i];
        match c {
            c if c.is_whitespace() => i += 1,
            '(' => {
                out.push(Tok::LParen);
                i += 1;
            }
            ')' => {
                out.push(Tok::RParen);
                i += 1;
            }
            '+' => {
                out.push(Tok::Plus);
                i += 1;
            }
            '-' => {
                out.push(Tok::Minus);
                i += 1;
            }
            '*' => {
                out.push(Tok::Star);
                i += 1;
            }
            '/' => {
                out.push(Tok::Slash);
                i += 1;
            }
            '%' => {
                out.push(Tok::Percent);
                i += 1;
            }
            ',' => {
                out.push(Tok::Comma);
                i += 1;
            }
            '"' | '\'' => {
                let start = i + 1;
                let mut j = start;
                while j < chars.len() && chars[j] != c {
                    j += 1;
                }
                if j >= chars.len() {
                    return Err(format!("unterminated text in `{}`", s.trim()));
                }
                out.push(Tok::Str(chars[start..j].iter().collect()));
                i = j + 1;
            }
            '=' => {
                out.push(Tok::Cmp(Cmp::Eq));
                i += if at(i + 1) == Some('=') { 2 } else { 1 };
            }
            '!' => {
                if at(i + 1) == Some('=') {
                    out.push(Tok::Cmp(Cmp::Ne));
                    i += 2;
                } else {
                    out.push(Tok::Not);
                    i += 1;
                }
            }
            '<' => {
                if at(i + 1) == Some('=') {
                    out.push(Tok::Cmp(Cmp::Le));
                    i += 2;
                } else {
                    out.push(Tok::Cmp(Cmp::Lt));
                    i += 1;
                }
            }
            '>' => {
                if at(i + 1) == Some('=') {
                    out.push(Tok::Cmp(Cmp::Ge));
                    i += 2;
                } else {
                    out.push(Tok::Cmp(Cmp::Gt));
                    i += 1;
                }
            }
            '&' if at(i + 1) == Some('&') => {
                out.push(Tok::And);
                i += 2;
            }
            '|' if at(i + 1) == Some('|') => {
                out.push(Tok::Or);
                i += 2;
            }
            c if c.is_ascii_digit() => {
                let start = i;
                while i < chars.len() && chars[i].is_ascii_digit() {
                    i += 1;
                }
                if at(i) == Some('.') && at(i + 1).is_some_and(|d| d.is_ascii_digit()) {
                    let mut end = i + 1;
                    while end < chars.len() && chars[end].is_ascii_digit() {
                        end += 1;
                    }
                    let text: String = chars[start..end].iter().collect();
                    return Err(format!("`{text}` is not a whole number (numbers here have no decimals; use whole numbers, like 15)"));
                }
                let text: String = chars[start..i].iter().collect();
                let n = text.parse::<u64>().map_err(|_| format!("`{text}` is too large a number"))?;
                out.push(Tok::Int(n));
            }
            c if c.is_alphabetic() || c == '_' => {
                let start = i;
                loop {
                    while i < chars.len() && (chars[i].is_alphanumeric() || chars[i] == '_') {
                        i += 1;
                    }
                    // a dot continues the name when another part follows: `hero.love`
                    if at(i) != Some('.') {
                        break;
                    }
                    if at(i + 1).is_some_and(|d| d.is_alphanumeric() || d == '_') {
                        i += 1;
                    } else if at(i + 1) == Some('{') {
                        // a part named by a variable: `love.{who}`, checked as a whole below
                        let close = (i..chars.len()).find(|&k| chars[k] == '}').ok_or_else(|| format!("`{{` without a closing `}}` in `{}`", s.trim()))?;
                        i = close + 1;
                        if at(i).is_some_and(|d| d.is_alphanumeric() || d == '_') {
                            return Err(format!("a `{{}}` part must be followed by a dot or the end of the name, in `{}`", s.trim()));
                        }
                    } else {
                        let text: String = chars[start..=i].iter().collect();
                        return Err(format!("`{text}` is not a valid name (a dot goes between two parts, like `hero.love`)"));
                    }
                }
                let word: String = chars[start..i].iter().collect();
                if word.contains('{') {
                    crate::name_pieces(&word)?;
                }
                // `seen(label)`: the label is read as written (it may have dashes).
                let mut j = i;
                while at(j).is_some_and(char::is_whitespace) {
                    j += 1;
                }
                if word == "seen" && at(j) == Some('(') {
                    let close = (j..chars.len()).find(|&k| chars[k] == ')').ok_or("missing `)` after `seen(`")?;
                    let raw: String = chars[j + 1..close].iter().collect();
                    let label = crate::parser::slug(raw.trim().trim_start_matches('#'));
                    if label.is_empty() {
                        return Err("`seen()` needs a label, like `seen(rooftop)`".into());
                    }
                    out.push(Tok::Seen(label));
                    i = close + 1;
                    continue;
                }
                out.push(match word.as_str() {
                    "and" => Tok::And,
                    "or" => Tok::Or,
                    "not" => Tok::Not,
                    _ => Tok::Ident(word),
                });
            }
            other => return Err(format!("unexpected `{other}` in `{}`", s.trim())),
        }
    }
    Ok(out)
}

struct Parser {
    toks: Vec<Tok>,
    pos: usize,
    /// `random(lo, hi)` is accepted (the right side of an assignment).
    allow_random: bool,
    depth: usize,
}

impl Parser {
    /// Count one level of nesting; the caller undoes it with `self.depth -= 1`.
    fn nest(&mut self) -> Result<(), String> {
        if self.depth >= MAX_NESTING {
            return Err("expression is nested too deeply".into());
        }
        self.depth += 1;
        Ok(())
    }

    fn peek(&self) -> Option<&Tok> {
        self.toks.get(self.pos)
    }

    fn or(&mut self) -> Result<Expr, String> {
        let mut left = self.and()?;
        while self.peek() == Some(&Tok::Or) {
            self.pos += 1;
            left = Expr::Or(Box::new(left), Box::new(self.and()?));
        }
        Ok(left)
    }

    fn and(&mut self) -> Result<Expr, String> {
        let mut left = self.not()?;
        while self.peek() == Some(&Tok::And) {
            self.pos += 1;
            left = Expr::And(Box::new(left), Box::new(self.not()?));
        }
        Ok(left)
    }

    fn not(&mut self) -> Result<Expr, String> {
        if self.peek() == Some(&Tok::Not) {
            self.pos += 1;
            self.nest()?;
            let inner = self.not();
            self.depth -= 1;
            return Ok(Expr::Not(Box::new(inner?)));
        }
        self.comparison()
    }

    fn comparison(&mut self) -> Result<Expr, String> {
        let left = self.additive()?;
        if self.peek() == Some(&Tok::Ident("has".into())) {
            self.pos += 1;
            return match left {
                Expr::Var(name) if !name.starts_with(SEEN) => Ok(Expr::Has(name, Box::new(self.primary()?))),
                _ => Err("the left side of `has` must be a list variable, like `inventory has \"key\"`".into()),
            };
        }
        if let Some(Tok::Cmp(op)) = self.peek().cloned() {
            self.pos += 1;
            let right = self.additive()?;
            return Ok(Expr::Cmp(Box::new(left), op, Box::new(right)));
        }
        Ok(left)
    }

    fn additive(&mut self) -> Result<Expr, String> {
        let mut left = self.multiplicative()?;
        loop {
            let op = match self.peek() {
                Some(Tok::Plus) => BinOp::Add,
                Some(Tok::Minus) => BinOp::Sub,
                _ => return Ok(left),
            };
            self.pos += 1;
            left = Expr::Bin(Box::new(left), op, Box::new(self.multiplicative()?));
        }
    }

    fn multiplicative(&mut self) -> Result<Expr, String> {
        let mut left = self.unary()?;
        loop {
            let op = match self.peek() {
                Some(Tok::Star) => BinOp::Mul,
                Some(Tok::Slash) => BinOp::Div,
                Some(Tok::Percent) => BinOp::Rem,
                _ => return Ok(left),
            };
            self.pos += 1;
            let right = self.unary()?;
            if matches!(op, BinOp::Div | BinOp::Rem) && right == Expr::Lit(Value::Int(0)) {
                let what = if op == BinOp::Div { "divide" } else { "take the remainder" };
                return Err(format!("can't {what} by 0 (the divisor is the literal 0)"));
            }
            left = Expr::Bin(Box::new(left), op, Box::new(right));
        }
    }

    fn unary(&mut self) -> Result<Expr, String> {
        if self.peek() != Some(&Tok::Minus) {
            return self.primary();
        }
        self.pos += 1;
        // the one number that only exists negated
        if self.peek() == Some(&Tok::Int(1 << 63)) {
            self.pos += 1;
            return Ok(Expr::Lit(Value::Int(i64::MIN)));
        }
        self.nest()?;
        let inner = self.unary();
        self.depth -= 1;
        Ok(match inner? {
            Expr::Lit(Value::Int(n)) => Expr::Lit(Value::Int(n.saturating_neg())),
            other => Expr::Neg(Box::new(other)),
        })
    }

    /// `name(args)`, after the name; the `(` is next.
    fn function(&mut self, name: &str) -> Result<Expr, String> {
        self.nest()?;
        let r = self.function_inner(name);
        self.depth -= 1;
        r
    }

    fn function_inner(&mut self, name: &str) -> Result<Expr, String> {
        self.pos += 1;
        // no rolls inside a roll's own bounds
        let allowed = self.allow_random;
        if matches!(name, "random" | "pick") {
            self.allow_random = false;
        }
        let mut args = Vec::new();
        if self.peek() == Some(&Tok::RParen) {
            self.pos += 1;
        } else {
            loop {
                args.push(self.or()?);
                match self.toks.get(self.pos) {
                    Some(Tok::Comma) => self.pos += 1,
                    Some(Tok::RParen) => {
                        self.pos += 1;
                        break;
                    }
                    Some(t) => return Err(format!("unexpected `{}` in the arguments of `{name}` (expected `,` or `)`)", describe(t))),
                    None => return Err(format!("missing `)` after the arguments of `{name}`")),
                }
            }
        }
        self.allow_random = allowed;
        let arity = |ok: bool, want: &str| -> Result<(), String> {
            if ok {
                Ok(())
            } else {
                Err(format!("`{name}` takes {want}, but got {} (the functions are {FUNCTIONS})", args.len()))
            }
        };
        match name {
            "min" | "max" => {
                arity(args.len() >= 2, "at least 2 values")?;
                Ok(Expr::Func(if name == "min" { Func::Min } else { Func::Max }, args))
            }
            "abs" => {
                arity(args.len() == 1, "1 value")?;
                Ok(Expr::Func(Func::Abs, args))
            }
            "count" => {
                arity(args.len() == 1, "1 list variable")?;
                match args.pop() {
                    Some(Expr::Var(v)) if !v.starts_with(SEEN) => Ok(Expr::Count(v)),
                    _ => Err("`count` takes a list variable, like `count(inventory)`".into()),
                }
            }
            "random" => {
                arity(args.len() == 2, "2 values (the lowest and the highest)")?;
                if !allowed {
                    return Err("`random` can only be used on the right of an assignment (`::set{roll = random(1, 6)}`), \
                        not in a condition: assign it to a variable first, then test the variable"
                        .into());
                }
                let hi = args.pop().map(Box::new).expect("two arguments");
                let lo = args.pop().map(Box::new).expect("two arguments");
                if let (Expr::Lit(Value::Int(a)), Expr::Lit(Value::Int(b))) = (&*lo, &*hi) {
                    if a > b {
                        return Err(format!("`random({a}, {b})`: the lowest value comes first (try `random({b}, {a})`)"));
                    }
                }
                Ok(Expr::Random(lo, hi))
            }
            "pick" => {
                arity(!args.is_empty(), "at least 1 value (a list variable, or the choices)")?;
                if !allowed {
                    return Err("`pick` can only be used on the right of an assignment (`::set{who = pick(\"Mika\", \"Sora\")}`), \
                        not in a condition: assign it to a variable first, then test the variable"
                        .into());
                }
                Ok(Expr::Pick(args))
            }
            _ => Err(format!("unknown function `{name}` (the functions are {FUNCTIONS})")),
        }
    }

    fn primary(&mut self) -> Result<Expr, String> {
        let tok = self.toks.get(self.pos).cloned();
        self.pos += 1;
        match tok {
            Some(Tok::Int(n)) => match i64::try_from(n) {
                Ok(n) => Ok(Expr::Lit(Value::Int(n))),
                Err(_) => Err(format!("`{n}` is too large a number")),
            },
            Some(Tok::Str(s)) => Ok(Expr::Lit(Value::Str(s))),
            Some(Tok::Ident(name)) if self.peek() == Some(&Tok::LParen) && name != "has" => self.function(&name),
            Some(Tok::Ident(name)) => Ok(match name.as_str() {
                "true" => Expr::Lit(Value::Bool(true)),
                "false" => Expr::Lit(Value::Bool(false)),
                _ => Expr::Var(name),
            }),
            Some(Tok::Seen(label)) => Ok(Expr::Var(format!("{SEEN}{label}"))),
            Some(Tok::LParen) => {
                self.nest()?;
                let inner = self.or();
                self.depth -= 1;
                let inner = inner?;
                if self.peek() == Some(&Tok::RParen) {
                    self.pos += 1;
                    Ok(inner)
                } else {
                    Err("missing `)`".into())
                }
            }
            Some(t) => Err(format!("unexpected `{}`", describe(&t))),
            None => Err("the expression ends too early (is something missing after an operator?)".into()),
        }
    }
}

/// Parse an expression such as `trust > 1 and not met` or `price * 2 - discount`.
/// `random(lo, hi)` is refused: it only belongs on the right of an assignment.
pub fn parse_expr(s: &str) -> Result<Expr, String> {
    parse_expr_with(s, false)
}

fn parse_expr_with(s: &str, allow_random: bool) -> Result<Expr, String> {
    if s.trim().is_empty() {
        return Err("expected a condition, found nothing".into());
    }
    let toks = tokenize(s)?;
    let operators = toks.iter().filter(|t| matches!(t, Tok::Cmp(_) | Tok::Plus | Tok::Minus | Tok::Star | Tok::Slash | Tok::Percent | Tok::And | Tok::Or | Tok::Not));
    if operators.count() > MAX_OPERATORS {
        return Err(format!("expression is too long (at most {MAX_OPERATORS} operators)"));
    }
    let mut p = Parser { toks, pos: 0, allow_random, depth: 0 };
    let expr = p.or()?;
    match p.toks.get(p.pos) {
        None => Ok(expr),
        Some(t) => Err(format!("unexpected `{}` in `{}`", describe(t), s.trim())),
    }
}

/// Parse a plain literal: `3`, `true`, `"text"`.
pub fn parse_value(s: &str) -> Result<Value, String> {
    match parse_expr(s)? {
        Expr::Lit(v) => Ok(v),
        _ => Err(format!("`{}` must be a number, true/false, or \"text\" in quotes", s.trim())),
    }
}

/// Parse a list literal from the front matter: `[]`, `[map, torch]`, `["magic key", lamp]`.
/// Items are text, quoted or bare; repeats are dropped.
pub fn parse_list(s: &str) -> Result<Value, String> {
    let inner = s
        .trim()
        .strip_prefix('[')
        .and_then(|r| r.strip_suffix(']'))
        .ok_or_else(|| format!("`{}` is not a list (expected `[]` or `[map, torch]`)", s.trim()))?;
    let mut items: Vec<String> = Vec::new();
    for raw in split_top_level(inner, ',').into_iter().map(str::trim) {
        if raw.is_empty() && inner.trim().is_empty() {
            break;
        }
        let quoted = raw.len() >= 2 && (raw.starts_with('"') && raw.ends_with('"') || raw.starts_with('\'') && raw.ends_with('\''));
        let item = if quoted { &raw[1..raw.len() - 1] } else { raw };
        let stray: &[char] = if quoted { &[raw.as_bytes()[0] as char] } else { &['"', '\'', '[', ']'] };
        if item.is_empty() || item.contains(stray) {
            return Err(format!("bad list item `{raw}` in `{}` (items are text: `map` or `\"magic key\"`)", s.trim()));
        }
        if !items.iter().any(|i| i == item) {
            items.push(item.to_string());
        }
    }
    Ok(Value::List(items))
}

/// Parse an assignment: `trust+=1`, `trust-=1`, `met=true`, `name = "Alex"`, `gold -= price * 2`,
/// `bag = ["key"]`, `roll = random(1, 6)` (the only place `random` and `pick` are allowed).
pub fn parse_effect(s: &str) -> Result<Effect, String> {
    let s = s.trim();
    let bad = || format!("bad assignment `{s}` (expected `name+=1`, `name-=1` or `name=value`)");
    let var_len = s.find(|c: char| !(c.is_alphanumeric() || matches!(c, '_' | '.' | '{' | '}'))).unwrap_or(s.len());
    let var = &s[..var_len];
    if var.contains(['{', '}']) {
        crate::name_pieces(var)?;
    } else if !is_ident(var) {
        return Err(bad());
    }
    if is_keyword(var) {
        return Err(format!("`{var}` is a keyword and can't be a variable name"));
    }
    let rest = s[var_len..].trim_start();
    let (op, len) = if rest.starts_with("+=") {
        (AssignOp::Add, 2)
    } else if rest.starts_with("-=") {
        (AssignOp::Sub, 2)
    } else if rest.starts_with('=') && !rest.starts_with("==") {
        (AssignOp::Set, 1)
    } else {
        return Err(bad());
    };
    let rhs = &rest[len..];
    if rhs.trim().is_empty() {
        return Err(format!("missing value after the operator in `{s}`"));
    }
    // `bag = []` / `bag = ["key", "map"]` set a list (the parser checks that `bag` is one)
    let value = if op == AssignOp::Set && rhs.trim_start().starts_with('[') {
        Expr::Lit(parse_list(rhs)?)
    } else {
        parse_expr_with(rhs, true).map_err(|m| format!("{m} (in `{s}`)"))?
    };
    Ok(Effect { var: var.to_string(), op, value })
}

/// Split on `sep`, ignoring separators inside quotes or brackets.
pub fn split_top_level(s: &str, sep: char) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut depth = 0i32;
    let mut quote: Option<char> = None;
    let mut escaped = false;
    let mut start = 0;
    for (i, c) in s.char_indices() {
        match quote {
            // a backslash inside quotes keeps the next character (`\"`) from closing them
            Some(_) if escaped => escaped = false,
            Some(q) => {
                if c == '\\' {
                    escaped = true;
                } else if c == q {
                    quote = None;
                }
            }
            None => match c {
                '"' | '\'' => quote = Some(c),
                '(' | '{' | '[' => depth += 1,
                ')' | '}' | ']' => depth -= 1,
                c if c == sep && depth == 0 => {
                    parts.push(&s[start..i]);
                    start = i + c.len_utf8();
                }
                _ => {}
            },
        }
    }
    parts.push(&s[start..]);
    parts
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ops::Vars;

    fn eval(src: &str, vars: &[(&str, Value)]) -> bool {
        let v: Vars = vars.iter().map(|(k, v)| (k.to_string(), v.clone())).collect();
        parse_expr(src).unwrap().test(&v)
    }

    #[test]
    fn seen_reads_a_label_count() {
        assert_eq!(parse_expr("seen(rooftop)").unwrap(), Expr::Var("seen:rooftop".into()));
        let roof = Expr::Cmp(Box::new(Expr::Var("seen:the-roof".into())), Cmp::Ge, Box::new(Expr::Lit(Value::Int(2))));
        assert_eq!(parse_expr("seen (#The Roof) >= 2").unwrap(), roof, "spaces, `#` and heading text are fine");
        assert_eq!(parse_expr("seen").unwrap(), Expr::Var("seen".into()), "a plain variable named seen still works");
        assert!(parse_expr("seen()").unwrap_err().contains("needs a label"));
        assert!(parse_expr("seen(x").unwrap_err().contains("missing `)`"));
        let vars = [("seen:a".to_string(), Value::Int(2))].into_iter().collect();
        assert!(parse_expr("seen(a) > 1 and not seen(b)").unwrap().test(&vars));
    }

    #[test]
    fn literals_and_variables() {
        assert_eq!(parse_expr("42").unwrap(), Expr::Lit(Value::Int(42)));
        assert_eq!(parse_expr("-3").unwrap(), Expr::Lit(Value::Int(-3)));
        assert_eq!(parse_expr("true").unwrap(), Expr::Lit(Value::Bool(true)));
        assert_eq!(parse_expr("\"hi there\"").unwrap(), Expr::Lit(Value::Str("hi there".into())));
        assert_eq!(parse_expr("'x'").unwrap(), Expr::Lit(Value::Str("x".into())));
        assert_eq!(parse_expr("seen").unwrap(), Expr::Var("seen".into()));
    }

    #[test]
    fn comparisons() {
        let t = |s: &str| eval(s, &[("trust", Value::Int(2))]);
        assert!(t("trust > 1") && t("trust>=2") && t("trust == 2") && t("trust = 2"));
        assert!(t("trust != 3") && t("trust < 3") && t("trust <= 2"));
        assert!(!t("trust > 2") && !t("trust == 3"));
    }

    #[test]
    fn and_or_not_with_precedence_and_parentheses() {
        let vars = [("a", Value::Bool(true)), ("b", Value::Bool(false)), ("c", Value::Bool(true))];
        assert!(eval("a or b and b", &vars)); // and binds tighter: a or (b and b)
        assert!(!eval("(a or b) and b", &vars));
        assert!(eval("not b", &vars) && eval("!b", &vars));
        assert!(eval("a && c", &vars) && eval("b || c", &vars));
        assert!(eval("not (a and b)", &vars));
        assert!(eval("not not a", &vars));
    }

    #[test]
    fn text_comparison() {
        assert!(eval("name == \"Mika\"", &[("name", Value::Str("Mika".into()))]));
        assert!(!eval("name == \"Other\"", &[("name", Value::Str("Mika".into()))]));
    }

    #[test]
    fn helpful_errors() {
        assert!(parse_expr("").unwrap_err().contains("found nothing"));
        assert!(parse_expr("a >").unwrap_err().contains("ends too early"));
        assert!(parse_expr("(a").unwrap_err().contains("missing `)`"));
        assert!(parse_expr("a b").unwrap_err().contains("unexpected `b`"));
        assert!(parse_expr("a & b").unwrap_err().contains("unexpected `&`"));
        assert!(parse_expr("\"oops").unwrap_err().contains("unterminated"));
    }

    #[test]
    fn values_must_be_literals() {
        assert_eq!(parse_value("7").unwrap(), Value::Int(7));
        assert_eq!(parse_value("false").unwrap(), Value::Bool(false));
        assert_eq!(parse_value("\"Alex\"").unwrap(), Value::Str("Alex".into()));
        assert!(parse_value("Alex").unwrap_err().contains("must be a number"));
    }

    #[test]
    fn effects() {
        let e = parse_effect("trust+=1").unwrap();
        assert_eq!((e.var.as_str(), e.op, e.value), ("trust", AssignOp::Add, Expr::Lit(Value::Int(1))));
        assert_eq!(parse_effect("trust -= 2").unwrap().op, AssignOp::Sub);
        let e = parse_effect("name = \"Alex\"").unwrap();
        assert_eq!((e.op, e.value), (AssignOp::Set, Expr::Lit(Value::Str("Alex".into()))));
        let e = parse_effect("gold -= price").unwrap();
        assert_eq!(e.value, Expr::Var("price".into()));
        assert!(parse_effect("1x = 2").is_err());
        assert!(parse_effect("x == 2").is_err());
        assert!(parse_effect("x").is_err());
        assert!(parse_effect("x +=").unwrap_err().contains("missing value"));
        assert!(parse_effect("x += >").unwrap_err().contains("unexpected `>`"));
    }

    #[test]
    fn splitting_respects_quotes_and_brackets() {
        assert_eq!(split_top_level("a, b,c", ','), vec!["a", " b", "c"]);
        assert_eq!(split_top_level("a=\"x, y\", b", ','), vec!["a=\"x, y\"", " b"]);
        assert_eq!(split_top_level("f(1, 2), g", ','), vec!["f(1, 2)", " g"]);
        assert_eq!(split_top_level("", ','), vec![""]);
    }

    #[test]
    fn identifiers() {
        assert!(is_ident("trust") && is_ident("_x1") && is_ident("é"));
        assert!(!is_ident("") && !is_ident("1x") && !is_ident("a-b"));
    }

    // ---- list variables ----

    fn list(items: &[&str]) -> Value {
        Value::List(items.iter().map(|s| s.to_string()).collect())
    }

    #[test]
    fn has_asks_whether_a_list_holds_an_item() {
        let e = parse_expr("inventory has \"key\"").unwrap();
        assert_eq!(e, Expr::Has("inventory".into(), Box::new(Expr::Lit(Value::Str("key".into())))));
        let v = [("inventory", list(&["map", "key"])), ("item", Value::Str("map".into())), ("n", Value::Int(1))];
        assert!(eval("inventory has 'key'", &v) && eval("inventory has item", &v));
        assert!(!eval("inventory has 'lamp'", &v));
        assert!(!eval("inventory has n", &v), "only text can be in a list");
        assert!(!eval("n has 'key'", &v) && !eval("nothing has 'key'", &v), "a non-list never has anything");
        assert!(eval("inventory has (item)", &v));
    }

    #[test]
    fn has_binds_tighter_than_not_and_or() {
        let v = [("inv", list(&["a"])), ("b", Value::Bool(false))];
        assert!(eval("not inv has 'z'", &v));
        assert!(!eval("not inv has 'a'", &v));
        assert!(eval("inv has 'a' and not inv has 'z'", &v));
        assert!(eval("b or inv has 'a'", &v));
        assert!(!eval("b and inv has 'a' or inv has 'z'", &v));
        assert!(eval("not (inv has 'z' or b)", &v));
        assert!(eval("seen has 'x' or true", &[]), "`seen` and `has` stay usable as plain names");
        assert_eq!(parse_expr("has").unwrap(), Expr::Var("has".into()));
    }

    #[test]
    fn has_errors() {
        assert!(parse_expr("3 has 'a'").unwrap_err().contains("left side of `has`"));
        assert!(parse_expr("'a' has 'a'").unwrap_err().contains("left side of `has`"));
        assert!(parse_expr("seen(x) has 'a'").unwrap_err().contains("left side of `has`"));
        assert!(parse_expr("(a) has 'a'").is_ok(), "parentheses just group a variable");
        assert!(parse_expr("a has").unwrap_err().contains("ends too early"));
        assert!(parse_expr("a has 'x' 'y'").unwrap_err().contains("unexpected"));
    }

    #[test]
    fn has_counts_as_a_read_of_the_list() {
        let mut names = Vec::new();
        let e = parse_expr("inv has item").unwrap();
        e.vars_read(&mut names);
        assert_eq!(names, ["inv", "item"]);
    }

    #[test]
    fn list_literals() {
        assert_eq!(parse_list("[]").unwrap(), list(&[]));
        assert_eq!(parse_list("[ ]").unwrap(), list(&[]));
        assert_eq!(parse_list("[map, torch]").unwrap(), list(&["map", "torch"]));
        assert_eq!(parse_list("[\"magic key\", 'a, b', map, map]").unwrap(), list(&["magic key", "a, b", "map"]));
        for bad in ["map", "[a,,b]", "[a,]", "[\"\"]", "[a\"b]", "[[a]]"] {
            assert!(parse_list(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn effects_on_lists_parse_like_any_other() {
        let e = parse_effect("inventory += \"key\"").unwrap();
        assert_eq!((e.var.as_str(), e.op), ("inventory", AssignOp::Add));
        assert_eq!(parse_effect("inventory -= item").unwrap().value, Expr::Var("item".into()));
    }

    // ---- arithmetic, functions, dotted names, random ----

    fn num(src: &str, vars: &[(&str, Value)]) -> i64 {
        let v: Vars = vars.iter().map(|(k, v)| (k.to_string(), v.clone())).collect();
        parse_expr(src).unwrap().eval(&v).as_int().unwrap()
    }

    #[test]
    fn arithmetic_precedence_and_associativity() {
        assert_eq!(num("1 + 2 * 3", &[]), 7);
        assert_eq!(num("(1 + 2) * 3", &[]), 9);
        assert_eq!(num("10 - 4 - 3", &[]), 3, "left to right");
        assert_eq!(num("100 / 10 / 5", &[]), 2);
        assert_eq!(num("2 * 3 % 4", &[]), 2, "* / % share a level, left to right");
        assert_eq!(num("7 - 2 * 3 + 1", &[]), 2);
        let v = [("a", Value::Int(4)), ("b", Value::Int(3))];
        assert_eq!(num("a*b-a/b+a%b", &v), 12 - 1 + 1);
        assert!(eval("a + b > 6 and a - b == 1", &v), "arithmetic binds tighter than comparison and `and`");
        assert!(eval("not a + b == 7", &[("a", Value::Int(1)), ("b", Value::Int(2))]), "not is looser than comparison");
        assert!(eval("a * 2 >= b * 2 or false", &v));
    }

    #[test]
    fn unary_minus() {
        assert_eq!(parse_expr("-3").unwrap(), Expr::Lit(Value::Int(-3)));
        assert_eq!(num("-2 * 3", &[]), -6);
        assert_eq!(num("- -4", &[]), 4);
        assert_eq!(num("5 - -2", &[]), 7);
        assert_eq!(num("-x + 1", &[("x", Value::Int(3))]), -2);
        assert_eq!(num("-(2 + 3)", &[]), -5);
        assert_eq!(num("4-3", &[]), 1, "no space needed");
        assert_eq!(num("-9223372036854775808", &[]), i64::MIN);
        assert!(parse_expr("9223372036854775808").unwrap_err().contains("too large"));
        assert!(parse_expr("99999999999999999999").unwrap_err().contains("too large"));
    }

    #[test]
    fn arithmetic_saturates_instead_of_wrapping_or_panicking() {
        let big = [("m", Value::Int(i64::MAX)), ("n", Value::Int(i64::MIN)), ("one", Value::Int(1)), ("neg", Value::Int(-1))];
        assert_eq!(num("m + 1", &big), i64::MAX);
        assert_eq!(num("n - 1", &big), i64::MIN);
        assert_eq!(num("m * 2", &big), i64::MAX);
        assert_eq!(num("n * 2", &big), i64::MIN);
        assert_eq!(num("n * neg", &big), i64::MAX);
        assert_eq!(num("n / neg", &big), i64::MAX, "MIN / -1 saturates");
        assert_eq!(num("n % neg", &big), 0);
        assert_eq!(num("-n", &big), i64::MAX);
        assert_eq!(num("abs(n)", &big), i64::MAX);
        assert_eq!(num("max(m, n) - one", &big), i64::MAX - 1);
    }

    #[test]
    fn division_and_modulo_truncate_toward_zero_and_by_zero_is_zero() {
        let v = [("zero", Value::Int(0)), ("a", Value::Int(7)), ("b", Value::Int(-7)), ("two", Value::Int(2)), ("m2", Value::Int(-2))];
        assert_eq!(num("a / two", &v), 3);
        assert_eq!(num("b / two", &v), -3, "toward zero, not floor");
        assert_eq!(num("a / m2", &v), -3);
        assert_eq!(num("b / m2", &v), 3);
        assert_eq!(num("a % two", &v), 1);
        assert_eq!(num("b % two", &v), -1, "the remainder has the sign of the dividend");
        assert_eq!(num("a % m2", &v), 1);
        assert_eq!(num("b % m2", &v), -1);
        assert_eq!(num("a / zero", &v), 0);
        assert_eq!(num("a % zero", &v), 0);
        assert_eq!(num("a / (two - 2)", &v), 0);
        assert_eq!(num("nothing / nothing", &v), 0, "unset variables are 0");
        for bad in ["a / 0", "a % 0", "a / -0", "a / (0)", "1 + a / 0"] {
            assert!(parse_expr(bad).unwrap_err().contains("by 0"), "{bad}");
        }
        assert!(parse_expr("a / 00").is_err());
    }

    #[test]
    fn non_numbers_count_as_zero_in_arithmetic() {
        let v = [("t", Value::Str("hi".into())), ("l", list(&["a", "b"])), ("b", Value::Bool(true))];
        assert_eq!(num("t + 5", &v), 5);
        assert_eq!(num("l * 3 + 1", &v), 1);
        assert_eq!(num("b + b", &v), 2, "true is 1");
        assert_eq!(num("count(l) + 1", &v), 3);
    }

    #[test]
    fn min_max_abs_and_count() {
        let v = [("x", Value::Int(-4)), ("inv", list(&["a", "b", "c"])), ("n", Value::Int(2))];
        assert_eq!(num("min(3, 1, 2)", &v), 1);
        assert_eq!(num("max(3, 1, 2)", &v), 3);
        assert_eq!(num("min(x, 0) + max(n, 5)", &v), -4 + 5);
        assert_eq!(num("abs(x)", &v), 4);
        assert_eq!(num("abs(x * 2 - 1)", &v), 9);
        assert_eq!(num("count(inv)", &v), 3);
        assert_eq!(num("count(missing)", &v), 0);
        assert!(eval("count(inv) >= 3 and max(1, n) == 2", &v));
        assert_eq!(parse_expr("count(inv)").unwrap(), Expr::Count("inv".into()));
        assert_eq!(num("min( 1 , 2 )", &v), 1, "spaces are fine");
        assert_eq!(num("max(min(9, 4), 3)", &v), 4, "nesting");
    }

    #[test]
    fn function_errors_list_what_exists() {
        for (src, want) in [
            ("min(1)", "`min` takes at least 2 values, but got 1"),
            ("max()", "`max` takes at least 2 values, but got 0"),
            ("abs(1, 2)", "`abs` takes 1 value, but got 2"),
            ("abs()", "`abs` takes 1 value, but got 0"),
            ("count(a, b)", "`count` takes 1 list variable, but got 2"),
            ("count()", "`count` takes 1 list variable, but got 0"),
            ("count(3)", "`count` takes a list variable"),
            ("count(a + 1)", "`count` takes a list variable"),
            ("len(a)", "unknown function `len`"),
            ("sqrt(4)", "unknown function `sqrt`"),
            ("min(1, 2", "missing `)`"),
            ("min(1 2)", "unexpected `2`"),
            ("min(1,, 2)", "unexpected `,`"),
        ] {
            let e = parse_expr(src).unwrap_err();
            assert!(e.contains(want), "{src}: {e}");
        }
        for src in ["min(1)", "len(a)", "abs()"] {
            assert!(parse_expr(src).unwrap_err().contains("min(a, b, ...), max(a, b, ...), abs(x), count(list), random(lo, hi), pick(a, b, ...)"), "{src}");
        }
        assert_eq!(parse_expr("min").unwrap(), Expr::Var("min".into()), "a variable may share a function's name");
    }

    #[test]
    fn dotted_names() {
        for good in ["hero.love", "stats.hp.max", "a.b_c.d1", "é.ü", "_x._y", "slot.0", "grid.1.2", "a.1x"] {
            assert!(is_ident(good), "{good}");
        }
        for bad in ["", ".a", "a.", "a..b", "1.a", "a.-1", "a. b", "a-b.c", "."] {
            assert!(!is_ident(bad), "{bad}");
        }
        assert!(is_plain_ident("hero") && !is_plain_ident("hero.love"));
        assert_eq!(parse_expr("hero.love").unwrap(), Expr::Var("hero.love".into()));
        let v = [("hero.love", Value::Int(3)), ("stats.hp.max", Value::Int(10))];
        assert!(eval("hero.love > 2 and stats.hp.max == 10", &v));
        assert_eq!(num("stats.hp.max - hero.love * 2", &v), 4);
        assert_eq!(parse_expr("bag.items has 'key'").unwrap(), Expr::Has("bag.items".into(), Box::new(Expr::Lit(Value::Str("key".into())))));
        assert_eq!(parse_expr("count(bag.items)").unwrap(), Expr::Count("bag.items".into()));
        let e = parse_effect("hero.love += 2").unwrap();
        assert_eq!((e.var.as_str(), e.op), ("hero.love", AssignOp::Add));
        assert_eq!(parse_effect("hero.love=hero.love*2").unwrap().var, "hero.love");
        assert!(parse_effect("hero. love = 1").is_err());
        assert!(parse_effect("hero.=1").is_err());
        assert!(parse_effect(".hero=1").is_err());
        for src in ["a.", "a..b"] {
            assert!(parse_expr(src).unwrap_err().contains("not a valid name"), "{src}");
        }
        assert!(parse_expr("true.x").is_ok(), "only whole names are keywords");
        assert_eq!(parse_expr("and.x").unwrap(), Expr::Var("and.x".into()));
    }

    #[test]
    fn decimals_are_still_refused() {
        for src in ["1.5", "x + 0.5", "x > 2.25", "3.0"] {
            let e = parse_expr(src).unwrap_err();
            assert!(e.contains("not a whole number"), "{src}: {e}");
        }
        assert!(parse_effect("x = 1.5").unwrap_err().contains("not a whole number"));
        assert!(parse_expr(".5").unwrap_err().contains("unexpected `.`"));
        assert!(parse_value("1.5").is_err());
    }

    #[test]
    fn random_only_on_the_right_of_an_assignment() {
        let e = parse_effect("roll = random(1, 6)").unwrap();
        assert!(matches!(e.value, Expr::Random(..)));
        assert!(parse_effect("dmg = random(2, 4) + bonus").unwrap().value.has_random());
        assert!(parse_effect("x += min(random(1, 6), 4)").unwrap().value.has_random());
        assert!(parse_effect("x = random(lo, hi + 1)").is_ok(), "variables as bounds");
        for src in ["random(1, 6) > 3", "x == random(1, 2)", "not random(0, 1)"] {
            let e = parse_expr(src).unwrap_err();
            assert!(e.contains("assign it to a variable first"), "{src}: {e}");
        }
        assert!(parse_effect("x = random(1, random(1, 2))").unwrap_err().contains("assign it to a variable first"), "no rolls in bounds");
        assert!(parse_effect("x = random(1)").unwrap_err().contains("takes 2 values"));
        assert!(parse_effect("x = random(1, 2, 3)").unwrap_err().contains("takes 2 values"));
        assert!(parse_effect("x = random(6, 1)").unwrap_err().contains("lowest value comes first"));
        assert!(parse_effect("x = random(-3, -3)").is_ok(), "a single value is fine");
        assert!(parse_effect("x = random(a, 1)").is_ok(), "swapped by variables is clamped at run time");
    }

    #[test]
    fn random_draws_from_the_supplied_generator_and_stays_in_range() {
        let e = parse_effect("x = random(3, 5)").unwrap();
        let mut seen = std::collections::BTreeSet::new();
        let mut state = 7u64;
        let mut vars = Vars::new();
        for _ in 0..200 {
            e.apply_with(&mut vars, &mut |lo, hi| crate::roll(&mut state, lo, hi));
            seen.insert(vars["x"].as_int().unwrap());
        }
        assert_eq!(seen.into_iter().collect::<Vec<_>>(), [3, 4, 5]);
        // swapped bounds from variables are put in order before drawing
        let e = parse_effect("x = random(hi, lo)").unwrap();
        let mut vars: Vars = [("lo".to_string(), Value::Int(1)), ("hi".to_string(), Value::Int(4))].into_iter().collect();
        let mut asked = Vec::new();
        e.apply_with(&mut vars, &mut |lo, hi| {
            asked.push((lo, hi));
            lo
        });
        assert_eq!(asked, [(1, 4)]);
        let mut s = 1;
        for _ in 0..50 {
            assert!((i64::MIN..=i64::MAX).contains(&crate::roll(&mut s, i64::MIN, i64::MAX)));
            assert_eq!(crate::roll(&mut s, 5, 5), 5);
        }
    }

    #[test]
    fn pick_takes_one_value_or_list_item_with_the_same_generator() {
        let mut vars: Vars = [("bag".to_string(), Value::List(vec!["a".into(), "b".into(), "c".into()]))].into_iter().collect();
        let mut state = 5;
        let mut seen = std::collections::BTreeSet::new();
        for src in ["x = pick(bag)", "x = pick(\"a\", \"b\", \"c\")"] {
            let e = parse_effect(src).unwrap();
            assert!(e.value.has_random());
            for _ in 0..100 {
                e.apply_with(&mut vars, &mut |lo, hi| crate::roll(&mut state, lo, hi));
                seen.insert(vars["x"].to_string());
            }
        }
        assert_eq!(seen.into_iter().collect::<Vec<_>>(), ["a", "b", "c"]);
        // the draw is an index, 0 ..= n - 1; numbers keep their type; one value is still a draw
        // returns what was asked of the generator, and answers with the highest
        let take = |e: &str, vars: &mut Vars| {
            let mut asked = Vec::new();
            parse_effect(e).unwrap().apply_with(vars, &mut |lo, hi| {
                asked.push((lo, hi));
                hi
            });
            asked
        };
        assert_eq!(take("n = pick(4, 7, 9)", &mut vars), [(0, 2)]);
        assert_eq!(vars["n"], Value::Int(9));
        assert_eq!(take("m = pick(\"only\")", &mut vars), [(0, 0)]);
        // an empty list gives "" and does not draw
        vars.insert("bag".into(), Value::List(vec![]));
        assert!(take("x = pick(bag)", &mut vars).is_empty());
        assert_eq!(vars["x"], Value::Str(String::new()));
    }

    #[test]
    fn a_list_literal_sets_a_list_variable_and_only_that() {
        let mut vars: Vars = [("bag".to_string(), Value::List(vec!["a".into()])), ("n".to_string(), Value::Int(1))].into_iter().collect();
        parse_effect("bag = [\"key\", map, \"key\"]").unwrap().apply(&mut vars);
        assert_eq!(vars["bag"], Value::List(vec!["key".into(), "map".into()]));
        parse_effect("bag = []").unwrap().apply(&mut vars);
        assert_eq!(vars["bag"], Value::List(vec![]));
        parse_effect("n = [\"x\"]").unwrap().apply(&mut vars);
        assert_eq!(vars["n"], Value::Int(1), "a non-list is left alone (the parser refuses it)");
        let many = format!("bag = [{}]", (0..crate::ops::MAX_LIST_ITEMS + 1).map(|i| format!("i{i}")).collect::<Vec<_>>().join(","));
        parse_effect(&many).unwrap().apply(&mut vars);
        assert_eq!(vars["bag"].to_string().split(", ").count(), crate::ops::MAX_LIST_ITEMS, "capped like `+=`");
    }

    #[test]
    fn the_generator_is_splitmix64_and_repeats_for_a_seed() {
        let mut s = 0u64;
        assert_eq!(crate::splitmix64(&mut s), 0xE220_A839_7B1D_CDAF, "the reference first output for seed 0");
        let run = |seed: u64| {
            let mut s = seed;
            (0..8).map(|_| crate::roll(&mut s, 1, 6)).collect::<Vec<_>>()
        };
        assert_eq!(run(42), run(42));
        assert_ne!(run(42), run(43));
    }

    #[test]
    fn effects_take_any_expression() {
        let mut v: Vars = [("gold".to_string(), Value::Int(10)), ("price".to_string(), Value::Int(4))].into_iter().collect();
        parse_effect("gold -= price * 2 + 1").unwrap().apply(&mut v);
        assert_eq!(v["gold"], Value::Int(1));
        parse_effect("gold += min(price, 3)").unwrap().apply(&mut v);
        assert_eq!(v["gold"], Value::Int(4));
        parse_effect("cost = price * 2 - 1").unwrap().apply(&mut v);
        assert_eq!(v["cost"], Value::Int(7));
        parse_effect("name = \"A\"").unwrap().apply(&mut v);
        parse_effect("name += \"b\"").unwrap().apply(&mut v);
        parse_effect("name += price + 1").unwrap().apply(&mut v);
        assert_eq!(v["name"], Value::Str("Ab5".into()), "text += still appends");
    }

    #[test]
    fn arithmetic_reads_its_variables() {
        let mut names = Vec::new();
        let e = parse_expr("a + b * count(inv) - min(c, 2)").unwrap();
        e.vars_read(&mut names);
        assert_eq!(names, ["a", "b", "inv", "c"]);
    }

    // ---- depth and size limits (expressions come from saves) -----------------------

    /// Run on a thread with a small stack (a Windows main thread has 1 MB).
    fn small_stack<F: FnOnce() + Send + 'static>(f: F) {
        std::thread::Builder::new().stack_size(512 * 1024).spawn(f).unwrap().join().expect("overflowed or panicked");
    }

    #[test]
    fn hostile_nesting_is_an_error_not_a_stack_overflow() {
        small_stack(|| {
            let shapes = [
                format!("{}1{}", "(".repeat(5000), ")".repeat(5000)),
                format!("{}x", "-".repeat(5000)),
                format!("{}x", "not ".repeat(5000)),
                format!("{}1{}", "abs(".repeat(5000), ")".repeat(5000)),
                format!("{}1{}", "min(1,".repeat(5000), ")".repeat(5000)),
            ];
            for src in &shapes {
                let e = parse_expr(src).unwrap_err();
                assert!(e.contains("too deeply") || e.contains("too long"), "{e}");
            }
            // just inside the limit still works
            let ok = format!("{}1{}", "(".repeat(MAX_NESTING), ")".repeat(MAX_NESTING));
            assert!(parse_expr(&ok).is_ok());
            let over = format!("{}1{}", "(".repeat(MAX_NESTING + 1), ")".repeat(MAX_NESTING + 1));
            assert!(parse_expr(&over).unwrap_err().contains("nested too deeply"));
            let nots = format!("{}x", "not ".repeat(MAX_NESTING + 1));
            assert!(parse_expr(&nots).unwrap_err().contains("nested too deeply"));
            let neg = format!("{}x", "-".repeat(MAX_NESTING + 1));
            assert!(parse_expr(&neg).unwrap_err().contains("nested too deeply"));
        });
    }

    #[test]
    fn a_long_flat_chain_is_refused_and_the_longest_allowed_one_is_safe_to_use() {
        small_stack(|| {
            let long = vec!["1"; MAX_TOKENS].join("+");
            assert!(parse_expr(&long).unwrap_err().contains("too long"));
            // the longest chain that is accepted: evaluate, clone, walk and drop it
            assert!(parse_expr(&vec!["1"; MAX_OPERATORS + 2].join("+")).unwrap_err().contains("operators"));
            let n = MAX_OPERATORS + 1;
            let chain = vec!["x"; n].join("+");
            let e = parse_expr(&chain).unwrap();
            let vars: Vars = [("x".to_string(), Value::Int(2))].into_iter().collect();
            assert_eq!(e.eval(&vars), Value::Int(2 * n as i64));
            let copy = e.clone();
            assert!(!copy.has_random());
            let mut out = Vec::new();
            copy.vars_read(&mut out);
            assert_eq!(out.len(), n);
            drop(copy);
            // many tokens but no deep tree: a wide call is fine up to the token limit
            let wide = format!("min({})", vec!["x"; MAX_TOKENS / 2 - 2].join(","));
            assert_eq!(parse_expr(&wide).unwrap().eval(&vars), Value::Int(2));
            let wider = format!("min({})", vec!["x"; MAX_TOKENS / 2].join(","));
            assert!(parse_expr(&wider).unwrap_err().contains("too long"));
            let mixed = format!("{}x{}", "-(".repeat(MAX_NESTING / 2), ")".repeat(MAX_NESTING / 2));
            parse_expr(&mixed).unwrap().eval(&vars);
        });
    }
}
