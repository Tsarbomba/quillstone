//! Turn a [`Draw`] list into GPU-ready quads for a given window size.
//!
//! This is the whole contract with the GPU backend: each [`Quad`] is one textured
//! rectangle in clip space, and each [`TextJob`] is a piece of text the backend
//! rasterizes into a texture (text quads point at their job). A video frame will
//! just be another quad whose texture is updated every frame.

use crate::layout::{letterbox, to_ndc, to_window, Rect, Size, VIRTUAL};
use crate::view::{Align, Draw, Outline, TextRun};

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
    /// Blur radius in window pixels (0 is sharp): the backend averages the picture over a disc this wide.
    pub blur: f32,
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
    /// Window pixels per virtual pixel, for sizes that are given in virtual ones.
    pub scale: f32,
    pub color: [f32; 4],
    pub align: Align,
    /// Border round the letters; `size` in window pixels.
    pub outline: Option<Outline>,
}

impl TextJob {
    /// Identifies the rasterized result, so a backend can cache it: equal keys
    /// mean identical pixels.
    pub fn key(&self) -> String {
        let [r, g, b, a] = self.color.map(|c| (c.clamp(0.0, 1.0) * 255.0).round() as u8);
        let outline = self.outline.map_or(String::new(), |o| format!("|{:.2}:{:?}", o.size, o.color));
        format!("{}|{}|{r},{g},{b},{a}{outline}", self.layout_key(), self.visible_chars)
    }

    /// The size in window pixels of one of this job's runs: the text size, or the
    /// run's `{size=...}` applied to it (within 8 to 200 virtual pixels).
    pub fn run_size_px(&self, run: &TextRun) -> f32 {
        match run.style.size {
            Some(size) if self.scale > 0.0 => size.resolve(self.size_px / self.scale) * self.scale,
            _ => self.size_px,
        }
    }

    /// Identifies the layout (where every glyph goes), which doesn't depend on
    /// how much is typed or on the colour: a backend can shape once per line.
    pub fn layout_key(&self) -> String {
        let mut key = format!("{}|{}x{}|{:.2}@{:.4}|{:?}", self.text, self.rect.w, self.rect.h, self.size_px, self.scale, self.align);
        for run in &self.runs {
            key.push_str(&format!("|{}-{}:{:?}:{:?}", run.start, run.end, run.style, run.ruby));
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
            Draw::Image { src, dest, uv, alpha, shade, blur } => frame.quads.push(Quad {
                texture: Tex::Image(src.clone()),
                rect: to_ndc(to_window(*dest, lb), window),
                uv: [uv.x, uv.y, uv.x + uv.w, uv.y + uv.h],
                color: [*shade, *shade, *shade, *alpha],
                blur: blur.max(0.0) * scale,
            }),
            Draw::Rect { rect, color } => frame.quads.push(Quad {
                texture: Tex::Solid,
                rect: to_ndc(to_window(*rect, lb), window),
                uv: [0.0, 0.0, 1.0, 1.0],
                color: *color,
                blur: 0.0,
            }),
            Draw::Text { text, runs, visible_chars, rect, size, color, align, outline } => {
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
                    blur: 0.0,
                });
                frame.texts.push(TextJob {
                    text: text.clone(),
                    runs: runs.clone(),
                    visible_chars: visible,
                    rect,
                    size_px: size * scale,
                    scale,
                    color: *color,
                    align: *align,
                    outline: outline.map(|o| Outline { size: o.size * scale, ..o }),
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
        Draw::Image { src: src.into(), dest: screen(), uv, alpha, shade: 1.0, blur: 0.0 }
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
            outline: None,
        }
    }

    const UNIT: Rect = Rect::new(0.0, 0.0, 1.0, 1.0);

    #[test]
    fn blur_is_scaled_to_window_pixels() {
        let blurred = Draw::Image { src: "a.png".into(), dest: screen(), uv: UNIT, alpha: 1.0, shade: 1.0, blur: 10.0 };
        assert_eq!(build_frame(std::slice::from_ref(&blurred), Size::new(1920.0, 1080.0)).quads[0].blur, 10.0);
        assert_eq!(build_frame(std::slice::from_ref(&blurred), Size::new(960.0, 540.0)).quads[0].blur, 5.0);
        let f = build_frame(&[image("a.png", UNIT, 1.0), Draw::Rect { rect: screen(), color: [0.0; 4] }], Size::new(960.0, 540.0));
        assert!(f.quads.iter().all(|q| q.blur == 0.0), "sharp unless asked");
    }

    #[test]
    fn a_ruby_or_underline_changes_the_layout_key() {
        let job = |tagged: &str| {
            let r = quillstone_script::markup::rich(tagged);
            let d = Draw::Text { text: r.text, runs: r.runs, visible_chars: usize::MAX, rect: screen(), size: 38.0, color: [1.0; 4], align: Align::Left, outline: None };
            build_frame(&[d], Size::new(1920.0, 1080.0)).texts.remove(0)
        };
        let keys = ["ab", "{ruby=x}ab{/ruby}", "{ruby=y}ab{/ruby}", "{u}ab{/u}"].map(|t| job(t).layout_key());
        for i in 0..keys.len() {
            for j in 0..i {
                assert_ne!(keys[i], keys[j], "{i} vs {j}");
            }
        }
    }

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
    fn size_runs_reach_the_job_and_its_cache_key() {
        use quillstone_script::markup::{rich, TextSize};
        let r = rich("a {size=+14}b{/size} {size=24}c{/size}");
        let draw = |runs: Vec<TextRun>| Draw::Text {
            text: r.text.clone(),
            runs,
            visible_chars: usize::MAX,
            rect: screen(),
            size: 38.0,
            color: [1.0; 4],
            align: Align::Left,
            outline: None,
        };
        let big = build_frame(&[draw(r.runs.clone())], Size::new(1920.0, 1080.0));
        let job = &big.texts[0];
        let sizes: Vec<f32> = job.runs.iter().map(|run| job.run_size_px(run)).collect();
        assert_eq!(sizes, [38.0, 52.0, 38.0, 24.0]);
        assert_eq!(job.runs[1].style.size, Some(TextSize::Relative(14)));
        // half-size window: everything is halved, sizes follow
        let half = build_frame(&[draw(r.runs.clone())], Size::new(960.0, 540.0));
        let sizes: Vec<f32> = half.texts[0].runs.iter().map(|run| half.texts[0].run_size_px(run)).collect();
        assert_eq!(sizes, [19.0, 26.0, 19.0, 12.0]);
        // a different size is a different picture, so a different key
        let mut other = r.runs.clone();
        other[1].style.size = Some(TextSize::Relative(15));
        let b = build_frame(&[draw(other)], Size::new(1920.0, 1080.0));
        assert_ne!(job.key(), b.texts[0].key());
        assert_ne!(job.layout_key(), b.texts[0].layout_key());
        let plain = build_frame(&[draw(Vec::new())], Size::new(1920.0, 1080.0));
        assert_ne!(job.layout_key(), plain.texts[0].layout_key());
    }

    #[test]
    fn an_outline_is_scaled_to_the_window_and_is_part_of_the_cache_key() {
        let draw = |outline| match text("hi", 2, screen()) {
            Draw::Text { text, runs, visible_chars, rect, size, color, align, .. } => {
                Draw::Text { text, runs, visible_chars, rect, size, color, align, outline }
            }
            other => other,
        };
        let black = Outline { size: 2.0, color: [0.0, 0.0, 0.0, 1.0] };
        let at = |o| build_frame(&[draw(o)], Size::new(960.0, 540.0)).texts.remove(0);
        let (none, thin, red) = (at(None), at(Some(black)), at(Some(Outline { color: [1.0, 0.0, 0.0, 1.0], ..black })));
        assert_eq!(thin.outline.map(|o| o.size), Some(1.0), "half-size window, half-width outline");
        assert_ne!(none.key(), thin.key());
        assert_ne!(thin.key(), red.key());
        assert_eq!(none.layout_key(), thin.layout_key(), "the outline moves no glyph");
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
