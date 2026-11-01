//! Helpers the test files share.

use crate::{parse, Program};

/// Parse `src`, which must be valid.
pub(crate) fn ok(src: &str) -> Program {
    parse(src).unwrap_or_else(|e| panic!("unexpected errors: {e:?}"))
}

/// The messages of the errors with their lines (none when the script parses).
pub(crate) fn errors(src: &str) -> Vec<String> {
    parse(src).err().unwrap_or_default().into_iter().map(|e| format!("{}: {}", e.line, e.message)).collect()
}

/// `src` must fail with an error containing `needle`.
pub(crate) fn bad(src: &str, needle: &str) {
    let e = errors(src);
    assert!(e.iter().any(|m| m.contains(needle)), "wanted `{needle}` in {e:?}");
}
