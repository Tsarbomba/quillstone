//! The little expression language used in `::if`, `{if ...}`, `if=` and assignments.
//!
//! Grammar (lowest to highest precedence):
//! `or` / `||`, `and` / `&&`, `not` / `!`, comparison (`== != < <= > >=`, and a lone
//! `=` also means equality), then literals, variables, `seen(label)` and `( ... )`.
//! Literals: whole numbers, `true` / `false`, and "text" or 'text'.
//! `seen(label)` is how many times the story has passed that label (0: never);
//! it reads the hidden variable `seen:label`, which the runtime counts up.

use crate::ops::{AssignOp, Cmp, Effect, Expr, Value, SEEN};

#[derive(Debug, Clone, PartialEq)]
enum Tok {
    Ident(String),
    Int(i64),
    Str(String),
    Cmp(Cmp),
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
        Tok::Cmp(c) => match c {
            Cmp::Eq => "==",
            Cmp::Ne => "!=",
            Cmp::Lt => "<",
            Cmp::Le => "<=",
            Cmp::Gt => ">",
            Cmp::Ge => ">=",
        }
        .to_string(),
        Tok::And => "and".into(),
        Tok::Or => "or".into(),
        Tok::Not => "not".into(),
        Tok::LParen => "(".into(),
        Tok::RParen => ")".into(),
        Tok::Seen(label) => format!("seen({label})"),
    }
}

/// A valid variable / character name: letters, digits and `_`, not starting with a digit.
pub fn is_ident(s: &str) -> bool {
    let mut chars = s.chars();
    match chars.next() {
        Some(c) if c.is_alphabetic() || c == '_' => chars.all(|c| c.is_alphanumeric() || c == '_'),
        _ => false,
    }
}

fn tokenize(s: &str) -> Result<Vec<Tok>, String> {
    let chars: Vec<char> = s.chars().collect();
    let at = |i: usize| chars.get(i).copied();
    let mut out = Vec::new();
    let mut i = 0;
    while i < chars.len() {
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
            c if c.is_ascii_digit() || (c == '-' && at(i + 1).is_some_and(|d| d.is_ascii_digit())) => {
                let start = i;
                i += 1;
                while i < chars.len() && chars[i].is_ascii_digit() {
                    i += 1;
                }
                let text: String = chars[start..i].iter().collect();
                let n = text.parse::<i64>().map_err(|_| format!("`{text}` is too large a number"))?;
                out.push(Tok::Int(n));
            }
            c if c.is_alphabetic() || c == '_' => {
                let start = i;
                while i < chars.len() && (chars[i].is_alphanumeric() || chars[i] == '_') {
                    i += 1;
                }
                let word: String = chars[start..i].iter().collect();
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
}

impl Parser {
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
            return Ok(Expr::Not(Box::new(self.not()?)));
        }
        self.comparison()
    }

    fn comparison(&mut self) -> Result<Expr, String> {
        let left = self.primary()?;
        if let Some(Tok::Cmp(op)) = self.peek().cloned() {
            self.pos += 1;
            let right = self.primary()?;
            return Ok(Expr::Cmp(Box::new(left), op, Box::new(right)));
        }
        Ok(left)
    }

    fn primary(&mut self) -> Result<Expr, String> {
        let tok = self.toks.get(self.pos).cloned();
        self.pos += 1;
        match tok {
            Some(Tok::Int(n)) => Ok(Expr::Lit(Value::Int(n))),
            Some(Tok::Str(s)) => Ok(Expr::Lit(Value::Str(s))),
            Some(Tok::Ident(name)) => Ok(match name.as_str() {
                "true" => Expr::Lit(Value::Bool(true)),
                "false" => Expr::Lit(Value::Bool(false)),
                _ => Expr::Var(name),
            }),
            Some(Tok::Seen(label)) => Ok(Expr::Var(format!("{SEEN}{label}"))),
            Some(Tok::LParen) => {
                let inner = self.or()?;
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

/// Parse an expression such as `trust > 1 and not met`.
pub fn parse_expr(s: &str) -> Result<Expr, String> {
    if s.trim().is_empty() {
        return Err("expected a condition, found nothing".into());
    }
    let mut p = Parser { toks: tokenize(s)?, pos: 0 };
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

/// Parse an assignment: `trust+=1`, `trust-=1`, `met=true`, `name = "Alex"`, `gold -= price`.
pub fn parse_effect(s: &str) -> Result<Effect, String> {
    let s = s.trim();
    let bad = || format!("bad assignment `{s}` (expected `name+=1`, `name-=1` or `name=value`)");
    let var_len = s.find(|c: char| !(c.is_alphanumeric() || c == '_')).unwrap_or(s.len());
    let var = &s[..var_len];
    if !is_ident(var) {
        return Err(bad());
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
    let value = parse_expr(rhs).map_err(|m| format!("{m} (in `{s}`)"))?;
    Ok(Effect { var: var.to_string(), op, value })
}

/// Split on `sep`, ignoring separators inside quotes or brackets.
pub fn split_top_level(s: &str, sep: char) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut depth = 0i32;
    let mut quote: Option<char> = None;
    let mut start = 0;
    for (i, c) in s.char_indices() {
        match quote {
            Some(q) => {
                if c == q {
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
}
