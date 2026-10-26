//! Quillstone script format: parser and compiler.
//!
//! Turns markdown-flavoured script text into a flat [`Program`] of [`Op`]s with
//! every label and jump already resolved to an index. This crate has no
//! rendering or I/O dependencies, so tooling (`quillstone check`, editors, CI)
//! can use it on its own.

mod expr;
pub mod markup;
mod ops;
mod parser;

pub use expr::{is_ident, parse_effect, parse_expr, parse_value};
pub use ops::{
    kind_from_path, AssetKind, AssignOp, Character, ChoiceOption, Cmp, Cond, Ease, Effect, Expr,
    Fit, Op, Program, Value, Vars, Wipe, ONCE, SEEN,
};
pub use parser::{parse, ParseError};
