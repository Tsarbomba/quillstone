//! Turn a [`Draw`] list into GPU-ready quads for a given window size.
//!
//! This is the whole contract with the GPU backend: each [`Quad`] is one textured
//! rectangle in clip space, and each [`TextJob`] is a piece of text the backend
//! rasterizes into a texture (text quads point at their job). A video frame will
//! just be another quad whose texture is updated every frame.

use crate::layout::{letterbox, to_ndc, to_window, Rect, Size, VIRTUAL};
use crate::view::{Align, Draw, TextRun};

/// What a quad samples from.
#[derive(Debug, Clone, PartialEq)]
pub enum Tex {
    /// A 1x1 white texture; the quad's colour is the fill.
    Solid,
    /// A loaded image, by the path used in the script.
    Image(String),
    /// The rasterized text of `Frame::texts[i]`.
    Text(usize),
}

/// One textured rectangle, ready to upload.
#[derive(Debug, Clone, PartialEq)]
pub struct Quad {
    pub texture: Tex,
    /// `[left, top, right, bottom]` in clip space (y up).
    pub rect: [f32; 4],
    /// `[u0, v0, u1, v1]`, v0 at the top of the picture.
    pub uv: [f32; 4],
    /// Multiplied with the texture; alpha is straight (not premultiplied).
    pub color: [f32; 4],
}

/// Text to rasterize, in window pixels (already scaled to the letterboxed screen).
#[derive(Debug, Clone, PartialEq)]
pub struct TextJob {
    pub text: String,
    /// Bold / italic / coloured stretches of `text` (empty: all in `color`).
    pub runs: Vec<TextRun>,
    /// Characters to show (typewriter). Never more than the text has.
    pub visible_chars: usize,
    /// Pixel-aligned destination in the window; also the texture size.
    pub rect: Rect,
    pub size_px: f32,
    pub color: [f32; 4],
    pub align: Align,
}

impl TextJob {
    /// Identifies the rasterized result, so a backend can cache it: equal keys
    /// mean identical pixels.
    pub fn key(&self) -> String {
        let [r, g, b, a] = self.color.map(|c| (c.clamp(0.0, 1.0) * 255.0).round() as u8);
        let mut key = format!(
            "{}|{}|{}x{}|{:.2}|{r},{g},{b},{a}|{:?}",
            self.text, self.visible_chars, self.rect.w, self.rect.h, self.size_px, self.align
        );
        for run in &self.runs {
            key.push_str(&format!("|{}-{}:{:?}", run.start, run.end, run.style));
        }
        key
    }
}

/// Text smaller than this many pixels is not drawn at all.
pub const MIN_TEXT_PX: f32 = 4.0;

/// Everything the backend needs for one frame.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Frame {
    pub quads: Vec<Quad>,
    pub texts: Vec<TextJob>,
}

/// Convert draws (virtual 1920x1080 space) into quads and text jobs for a window
/// of `window` pixels. Order is preserved (back to front).
pub fn build_frame(draws: &[Draw], window: Size) -> Frame {
    let lb = letterbox(window);
    let scale = lb.w / VIRTUAL.w;
    let mut frame = Frame { quads: Vec::with_capacity(draws.len()), texts: Vec::new() };

    for draw in draws {
        match draw {
            Draw::Image { src, dest, uv, alpha } => frame.quads.push(Quad {
                texture: Tex::Image(src.clone()),
                rect: to_ndc(to_window(*dest, lb), window),
                uv: [uv.x, uv.y, uv.x + uv.w, uv.y + uv.h],
                color: [1.0, 1.0, 1.0, *alpha],
            }),
            Draw::Rect { rect, color } => frame.quads.push(Quad {
                texture: Tex::Solid,
                rect: to_ndc(to_window(*rect, lb), window),
                uv: [0.0, 0.0, 1.0, 1.0],
                color: *color,
            }),
            Draw::Text { text, runs, visible_chars, rect, size, color, align } => {
                let total = text.chars().count();
                let visible = (*visible_chars).min(total);
                // Nothing to show yet, or the window is so small the text would be
                // sub-pixel (e.g. a minimized window): a text engine can't lay that out.
                if visible == 0 || lb.w <= 0.0 || size * scale < MIN_TEXT_PX {
                    continue;
                }
                let px = to_window(*rect, lb);
                let rect = Rect::new(px.x.round(), px.y.round(), px.w.ceil().max(1.0), px.h.ceil().max(1.0));
                frame.quads.push(Quad {
                    texture: Tex::Text(frame.texts.len()),
                    rect: to_ndc(rect, window),
                    uv: [0.0, 0.0, 1.0, 1.0],
                    color: [1.0, 1.0, 1.0, 1.0],
                });
                frame.texts.push(TextJob {
                    text: text.clone(),
                    runs: runs.clone(),
                    visible_chars: visible,
                    rect,
                    size_px: size * scale,
                    color: *color,
                    align: *align,
                });
            }
        }
    }
    frame
}

#[cfg(test)]
mod tests {
    use super::*;

    fn screen() -> Rect {
        Rect::new(0.0, 0.0, VIRTUAL.w, VIRTUAL.h)
    }

    fn image(src: &str, uv: Rect, alpha: f32) -> Draw {
        Draw::Image { src: src.into(), dest: screen(), uv, alpha }
    }

    fn text(s: &str, visible: usize, rect: Rect) -> Draw {
        Draw::Text {
            text: s.into(),
            runs: Vec::new(),
            visible_chars: visible,
            rect,
            size: 38.0,
            color: [1.0, 0.5, 0.0, 1.0],
            align: Align::Left,
        }
    }

    const UNIT: Rect = Rect::new(0.0, 0.0, 1.0, 1.0);

    #[test]
    fn full_screen_image_in_a_16_9_window_covers_clip_space() {
        let f = build_frame(&[image("bg.png", UNIT, 0.5)], Size::new(1280.0, 720.0));
        let q = &f.quads[0];
        assert_eq!(q.texture, Tex::Image("bg.png".into()));
        assert_eq!(q.rect, [-1.0, 1.0, 1.0, -1.0]);
        assert_eq!(q.uv, [0.0, 0.0, 1.0, 1.0]);
        assert_eq!(q.color, [1.0, 1.0, 1.0, 0.5]);
    }

    #[test]
    fn a_4_3_window_gets_bars_top_and_bottom() {
        let f = build_frame(&[image("bg.png", UNIT, 1.0)], Size::new(800.0, 600.0));
        // The picture spans y = 75..525 of 600 pixels.
        let [l, t, r, b] = f.quads[0].rect;
        assert_eq!((l, r), (-1.0, 1.0));
        assert!((t - 0.75).abs() < 1e-5 && (b + 0.75).abs() < 1e-5);
    }

    #[test]
    fn solid_rects_keep_their_colour() {
        let draws = [Draw::Rect { rect: screen(), color: [0.0, 0.0, 0.0, 0.7] }];
        let f = build_frame(&draws, Size::new(1920.0, 1080.0));
        assert_eq!(f.quads[0].texture, Tex::Solid);
        assert_eq!(f.quads[0].color, [0.0, 0.0, 0.0, 0.7]);
    }

    #[test]
    fn cropped_uv_rects_become_corner_coordinates() {
        let f = build_frame(&[image("w.png", Rect::new(0.25, 0.0, 0.5, 1.0), 1.0)], Size::new(1920.0, 1080.0));
        assert_eq!(f.quads[0].uv, [0.25, 0.0, 0.75, 1.0]);
    }

    #[test]
    fn text_becomes_a_job_plus_a_quad_pointing_at_it() {
        let rect = Rect::new(100.0, 200.0, 600.0, 100.0);
        let f = build_frame(&[text("hello", 3, rect)], Size::new(1920.0, 1080.0));
        assert_eq!(f.texts.len(), 1);
        assert_eq!(f.texts[0].rect, rect);
        assert_eq!(f.texts[0].size_px, 38.0);
        assert_eq!(f.texts[0].visible_chars, 3);
        assert_eq!(f.quads[0].texture, Tex::Text(0));
        assert_eq!(f.quads[0].color, [1.0; 4]);
        let expected = to_ndc(rect, Size::new(1920.0, 1080.0));
        assert_eq!(f.quads[0].rect, expected);
    }

    #[test]
    fn text_scales_with_the_window() {
        let f = build_frame(&[text("hello", 5, Rect::new(100.0, 200.0, 600.0, 100.0))], Size::new(960.0, 540.0));
        assert_eq!(f.texts[0].rect, Rect::new(50.0, 100.0, 300.0, 50.0));
        assert_eq!(f.texts[0].size_px, 19.0);
    }

    #[test]
    fn text_in_a_letterboxed_window_is_offset_by_the_bars() {
        // 800x600: the 16:9 area is 800x450 starting at y = 75.
        let f = build_frame(&[text("x", 1, Rect::new(0.0, 0.0, 192.0, 108.0))], Size::new(800.0, 600.0));
        let r = f.texts[0].rect;
        assert_eq!((r.x, r.y), (0.0, 75.0));
        assert_eq!((r.w, r.h), (80.0, 45.0));
    }

    #[test]
    fn text_is_dropped_when_the_window_is_too_small_for_it() {
        // A minimized window reports 0x0 (clamped to 1x1 by the backend): 38px text
        // becomes ~0.02px, which crashed cosmic-text with "line height cannot be 0".
        for window in [Size::new(1.0, 1.0), Size::new(0.0, 0.0), Size::new(100.0, 56.0)] {
            let draws = [text("hello", 5, screen()), Draw::Rect { rect: screen(), color: [0.0; 4] }];
            let f = build_frame(&draws, window);
            assert!(f.texts.is_empty(), "text kept in {window:?}");
        }
        // 38px * (320/1920) = 6.3px: small but still drawn.
        let f = build_frame(&[text("hello", 5, screen())], Size::new(320.0, 180.0));
        assert_eq!(f.texts.len(), 1);
    }

    #[test]
    fn visible_chars_are_clamped_to_the_text_length_in_characters() {
        let f = build_frame(&[text("héllo", usize::MAX, screen())], Size::new(1920.0, 1080.0));
        assert_eq!(f.texts[0].visible_chars, 5);
    }

    #[test]
    fn nothing_is_emitted_for_text_that_is_not_visible_yet() {
        let f = build_frame(&[text("hello", 0, screen()), text("", usize::MAX, screen())], Size::new(1920.0, 1080.0));
        assert!(f.quads.is_empty() && f.texts.is_empty());
    }

    #[test]
    fn cache_key_changes_only_when_pixels_would() {
        let rect = Rect::new(0.0, 0.0, 600.0, 100.0);
        let a = build_frame(&[text("hello", 2, rect)], Size::new(1920.0, 1080.0)).texts[0].key();
        let b = build_frame(&[text("hello", 2, rect)], Size::new(1920.0, 1080.0)).texts[0].key();
        let c = build_frame(&[text("hello", 3, rect)], Size::new(1920.0, 1080.0)).texts[0].key();
        let d = build_frame(&[text("hello", 2, rect)], Size::new(960.0, 540.0)).texts[0].key();
        assert_eq!(a, b);
        assert_ne!(a, c);
        assert_ne!(a, d);
    }

    #[test]
    fn order_is_preserved_across_kinds() {
        let draws = [
            Draw::Rect { rect: screen(), color: [1.0; 4] },
            text("hi", 2, screen()),
            image("a.png", UNIT, 1.0),
        ];
        let f = build_frame(&draws, Size::new(1920.0, 1080.0));
        let kinds: Vec<&Tex> = f.quads.iter().map(|q| &q.texture).collect();
        assert_eq!(kinds, [&Tex::Solid, &Tex::Text(0), &Tex::Image("a.png".into())]);
    }

    #[test]
    fn text_indices_stay_correct_when_some_text_is_skipped() {
        let draws = [text("a", 0, screen()), text("b", 1, screen()), text("c", 1, screen())];
        let f = build_frame(&draws, Size::new(1920.0, 1080.0));
        assert_eq!(f.texts.len(), 2);
        assert_eq!(f.texts[0].text, "b");
        assert_eq!(f.quads[1].texture, Tex::Text(1));
    }
}
