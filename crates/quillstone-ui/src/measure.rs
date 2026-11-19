//! Measuring text for layouts that size themselves to it (chat bubbles, journal rows). The frontend's text engine
//! knows the real wrapping; it hands a measuring function to [`crate::game::Game::set_text_measure`]. Without one
//! (tests, a frontend that has none) the cautious estimate of `quillstone_render` is used.

pub use quillstone_render::{fit_row, lines_of, Measure, MeasureFn};
