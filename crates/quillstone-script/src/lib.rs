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
mod translate;

#[cfg(test)]
mod test_util;
#[cfg(test)]
mod dynname_tests;
#[cfg(test)]
mod include_tests;
#[cfg(test)]
mod layers_tests;
#[cfg(test)]
mod particles_tests;
#[cfg(test)]
mod tint_tests;

pub use dynpath::{is_dynamic, is_name_piece, is_pattern, name_piece, name_pieces, name_vars, pattern_matches, resolve_name, pattern_vars, pieces, resolve, unsafe_reason, Piece, MISSING_PICTURE, PICTURE_EXTENSIONS};
pub use expr::{is_ident, is_plain_ident, parse_effect, parse_expr};
pub use ops::{
    get_var, is_inside, normal_path, kind_from_path, anim_id, bg_color, Anim, AnimFrame, AssetKind, ANIM_PREFIX, AssignOp, Area, BinOp, Imagemap, Character, ChoiceOption, Lock, Cmp, Cond, Ease, Effect, Expr,
    Axis, Fit, Func, Heading, HudAction, HudScreen, HudElem, HudExpr, HudKind, Item, MoveStep, Op, Program, Language, Replay, SourceSpan, ShopEntry, Value, Vars, Wipe, Hit, KeyPress, TimingBar, DEFAULT_SEED, INPUT_MAX_CAP, INPUT_MAX_LIMIT, MAX_CHOICE_TIME, MAX_CHOICE_TIME_CAP, MAX_LIST_ITEMS, ONCE, SEEN, POSITION,
    roll, splitmix64, ParticleKind, Particles, MAX_PARTICLES, PARTICLE_SPEED, PARTICLE_WIND, Tint, TINT_ALPHA,
};
pub use parser::{parse, parse_files, slug, Loader, MAX_MOVE_STEPS, SPRITE_OFFSET, ParseError, MAX_INCLUDE_BYTES, MAX_INCLUDE_DEPTH, MAX_INCLUDE_FILE_BYTES};
pub use translate::{check_translation, language_file, Catalog, Entry, Source, TranslationError, MAX_ENTRIES, MAX_FILE_BYTES};
