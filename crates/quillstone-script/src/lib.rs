//! Quillstone script format: parser and compiler.
//!
//! Turns markdown-flavoured script text into a flat [`Program`] of [`Op`]s with
//! every label and jump already resolved to an index. This crate has no
//! rendering or I/O dependencies, so tooling (`quillstone check`, editors, CI)
//! can use it on its own.

mod dynpath;
mod expr;
pub mod markup;
mod ops;
mod parser;

pub use dynpath::{is_pattern, pattern_matches, pattern_vars, pieces, resolve, unsafe_reason, Piece, MAX_FRAGMENT, MAX_PATTERN_VARS, MISSING_PICTURE};
pub use expr::{is_ident, is_plain_ident, parse_effect, parse_expr, parse_list, parse_value};
pub use ops::{
    is_inside, kind_from_path, anim_id, anim_src, bg_color, bg_color_src, Anim, AnimFrame, AssetKind, ANIM_FRAME_SECONDS, ANIM_PREFIX, AssignOp, Area, BinOp, Character, ChoiceOption, Cmp, Cond, Ease, Effect, Expr,
    Axis, Fit, Func, HudAnchor, HudElem, HudExpr, HudKind, Item, Op, Program, Replay, ShopEntry, Value, Vars, Wipe, BG_COLOR_PREFIX, MAX_CHOICE_TIME, MIN_CHOICE_TIME, DEFAULT_SEED, FUNCTIONS, INPUT_MAX_DEFAULT, INPUT_MAX_LIMIT, MAX_REPLAYS, ONCE, SEEN,
    roll, splitmix64,
};
pub use parser::{parse, ParseError};
