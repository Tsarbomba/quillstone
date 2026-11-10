//! Quillstone rendering, minus the GPU.
//!
//! Everything here is plain data and maths, so it is testable without a window:
//!
//! - [`layout`]: the fixed 1920x1080 virtual screen, letterboxing, and how an
//!   image or video frame is fitted into a rectangle (`contain` / `cover` / `stretch`).
//! - [`view`]: what is on screen. It consumes runtime [`Cmd`](quillstone_runtime::Cmd)s,
//!   animates backgrounds and the typewriter text, and produces a [`Draw`] list.
//! - [`batch`]: converts a draw list into clip-space [`Quad`]s for a window size.
//! - [`theme`]: colours, font, sizes and images of a game, from its theme file.
//! - [`filter`]: the `::filter` settings as a colour matrix for the GPU.
//! - [`particles`]: ambient snow, rain or a picture falling, from the time alone.
//! - [`pixels`]: prepares decoded images for upload (alpha bleeding, mipmaps).
//! - [`image_probe`]: whether an image file is one the player can show.
//!
//! The GPU backend (in `quillstone-player`: winit + wgpu) only has to upload
//! those quads and draw them, which keeps it small and keeps video frames just
//! another textured quad.

pub mod batch;
mod fade;
pub mod filter;
pub mod hud;
pub mod image_probe;
pub mod layout;
pub mod particles;
pub mod pixels;
pub mod theme;
pub mod view;

pub use batch::{build_frame, Frame, Quad, Tex, TextJob};
pub use filter::ColorMatrix;
pub use layout::{
    clip, estimate_lines, estimate_lines_styled, letterbox, line_height, one_row, place, to_ndc, to_virtual, to_window, Placement, Rect, Size, FULL_UV, VIRTUAL,
};
pub use theme::{CellStyle, QuickMenu, Theme};
pub use view::{
    Align, Background, Dialogue, Draw, InlineImage, Outline, SpriteView, TextRun, VideoView, View, SPRITE_HEIGHT,
    TEXTBOX,
};
pub use view::{choice_rects, dialogue_body, dialogue_lines_tagged, faded, name_plate_rect, portrait_rect, textbox_rect};
