//! Virtual screen and fitting maths. All game coordinates are in a fixed
//! 1920x1080 space; the window is just a scaled, letterboxed view of it.

use quillstone_script::Fit;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Size {
    pub w: f32,
    pub h: f32,
}

impl Size {
    pub const fn new(w: f32, h: f32) -> Size {
        Size { w, h }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl Rect {
    pub const fn new(x: f32, y: f32, w: f32, h: f32) -> Rect {
        Rect { x, y, w, h }
    }

    pub fn contains(&self, px: f32, py: f32) -> bool {
        px >= self.x && px < self.x + self.w && py >= self.y && py < self.y + self.h
    }
}

/// The coordinate space scripts and UI are laid out in.
pub const VIRTUAL: Size = Size::new(1920.0, 1080.0);

/// The largest 16:9 rectangle that fits in the window, centred (black bars elsewhere).
pub fn letterbox(window: Size) -> Rect {
    if window.w <= 0.0 || window.h <= 0.0 {
        return Rect::new(0.0, 0.0, 0.0, 0.0);
    }
    let scale = (window.w / VIRTUAL.w).min(window.h / VIRTUAL.h);
    let (w, h) = (VIRTUAL.w * scale, VIRTUAL.h * scale);
    Rect::new((window.w - w) / 2.0, (window.h - h) / 2.0, w, h)
}

/// Map a rectangle in virtual coordinates to window pixels.
pub fn to_window(r: Rect, lb: Rect) -> Rect {
    let s = lb.w / VIRTUAL.w;
    Rect::new(lb.x + r.x * s, lb.y + r.y * s, r.w * s, r.h * s)
}

/// Map a window pixel (e.g. the mouse) to virtual coordinates.
/// `None` when it lies on the black bars.
pub fn to_virtual(px: f32, py: f32, lb: Rect) -> Option<(f32, f32)> {
    if lb.w <= 0.0 || !lb.contains(px, py) {
        return None;
    }
    let s = VIRTUAL.w / lb.w;
    Some(((px - lb.x) * s, (py - lb.y) * s))
}

/// Convert a rectangle in window pixels to clip space: `[left, top, right, bottom]`
/// with x and y both in -1..1 and y pointing up.
pub fn to_ndc(r: Rect, window: Size) -> [f32; 4] {
    let x = |px: f32| px / window.w * 2.0 - 1.0;
    let y = |py: f32| 1.0 - py / window.h * 2.0;
    [x(r.x), y(r.y), x(r.x + r.w), y(r.y + r.h)]
}

/// Where to draw a picture and which part of it to sample.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Placement {
    pub dest: Rect,
    /// Texture coordinates (0..1) of the visible region.
    pub uv: Rect,
}

const FULL_UV: Rect = Rect::new(0.0, 0.0, 1.0, 1.0);

/// Fit `content` (an image or video frame size) into `container`.
///
/// - `Contain`: whole picture visible, bars where the aspect differs.
/// - `Cover`: container filled, the picture is cropped (via `uv`, so no scissor needed).
/// - `Stretch`: container filled, aspect ignored.
pub fn place(content: Size, container: Rect, fit: Fit) -> Placement {
    if content.w <= 0.0 || content.h <= 0.0 {
        return Placement { dest: container, uv: FULL_UV };
    }
    match fit {
        Fit::Stretch => Placement { dest: container, uv: FULL_UV },
        Fit::Contain => {
            let s = (container.w / content.w).min(container.h / content.h);
            let (w, h) = (content.w * s, content.h * s);
            Placement {
                dest: Rect::new(
                    container.x + (container.w - w) / 2.0,
                    container.y + (container.h - h) / 2.0,
                    w,
                    h,
                ),
                uv: FULL_UV,
            }
        }
        Fit::Cover => {
            let s = (container.w / content.w).max(container.h / content.h);
            let vis_w = container.w / (content.w * s);
            let vis_h = container.h / (content.h * s);
            Placement {
                dest: container,
                uv: Rect::new((1.0 - vis_w) / 2.0, (1.0 - vis_h) / 2.0, vis_w, vis_h),
            }
        }
    }
}

/// Cut `dest` down to `to`, cropping the texture coordinates `uv` to match.
/// `None` if nothing of it is left.
pub fn clip(dest: Rect, uv: Rect, to: Rect) -> Option<(Rect, Rect)> {
    let (x0, x1) = (dest.x.max(to.x), (dest.x + dest.w).min(to.x + to.w));
    let (y0, y1) = (dest.y.max(to.y), (dest.y + dest.h).min(to.y + to.h));
    if x1 <= x0 || y1 <= y0 || dest.w <= 0.0 || dest.h <= 0.0 {
        return None;
    }
    if (x0, y0, x1, y1) == (dest.x, dest.y, dest.x + dest.w, dest.y + dest.h) {
        return Some((dest, uv)); // already inside: keep the exact values
    }
    let u = |x: f32| uv.x + (x - dest.x) / dest.w * uv.w;
    let v = |y: f32| uv.y + (y - dest.y) / dest.h * uv.h;
    Some((Rect::new(x0, y0, x1 - x0, y1 - y0), Rect::new(u(x0), v(y0), u(x1) - u(x0), v(y1) - v(y0))))
}

/// A cautious guess at how many lines `text` wraps to at `size` px in `width` px.
///
/// The real wrapping happens in the backend's text engine, which this crate can't
/// see; over-guessing only leaves a little extra space between entries.
pub fn estimate_lines(text: &str, size: f32, width: f32) -> usize {
    let char_w = |c: char| if is_wide(c) { size } else { size * 0.55 };
    let space = size * 0.3;
    let mut lines = 0;
    for paragraph in text.split('\n') {
        let mut line = 0.0f32;
        let mut count = 1;
        for word in paragraph.split_whitespace() {
            let w: f32 = word.chars().map(char_w).sum();
            if line > 0.0 && line + space + w > width {
                count += 1;
                line = 0.0;
            }
            if line > 0.0 {
                line += space;
            }
            line += w;
            while line > width {
                // a word longer than a line (or CJK text without spaces) breaks anywhere
                count += 1;
                line -= width;
            }
        }
        lines += count;
    }
    lines.max(1)
}

fn is_wide(c: char) -> bool {
    matches!(c as u32,
        0x1100..=0x115F | 0x2E80..=0xA4CF | 0xAC00..=0xD7A3 | 0xF900..=0xFAFF
        | 0xFE30..=0xFE4F | 0xFF00..=0xFF60 | 0xFFE0..=0xFFE6 | 0x20000..=0x3FFFD)
}

#[cfg(test)]
mod tests {
    #[test]
    fn clipping_crops_the_texture_too() {
        let to = Rect::new(0.0, 0.0, 100.0, 100.0);
        let unit = Rect::new(0.0, 0.0, 1.0, 1.0);
        let (d, uv) = clip(Rect::new(-50.0, 0.0, 100.0, 100.0), unit, to).unwrap();
        assert_eq!(d, Rect::new(0.0, 0.0, 50.0, 100.0));
        assert_eq!(uv, Rect::new(0.5, 0.0, 0.5, 1.0));
        assert!(clip(Rect::new(200.0, 0.0, 10.0, 10.0), unit, to).is_none());
        assert_eq!(clip(Rect::new(10.0, 10.0, 5.0, 5.0), unit, to), Some((Rect::new(10.0, 10.0, 5.0, 5.0), unit)));
    }

    use super::*;

    fn close(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-3
    }

    fn rect_close(a: Rect, b: Rect) -> bool {
        close(a.x, b.x) && close(a.y, b.y) && close(a.w, b.w) && close(a.h, b.h)
    }

    #[test]
    fn letterbox_exact_16_9_fills_the_window() {
        let lb = letterbox(Size::new(1280.0, 720.0));
        assert!(rect_close(lb, Rect::new(0.0, 0.0, 1280.0, 720.0)));
    }

    #[test]
    fn letterbox_adds_bars_top_and_bottom_for_4_3() {
        let lb = letterbox(Size::new(800.0, 600.0));
        assert!(rect_close(lb, Rect::new(0.0, 75.0, 800.0, 450.0)));
    }

    #[test]
    fn letterbox_adds_bars_left_and_right_for_ultrawide() {
        let lb = letterbox(Size::new(2560.0, 1080.0));
        assert!(rect_close(lb, Rect::new(320.0, 0.0, 1920.0, 1080.0)));
    }

    #[test]
    fn letterbox_of_a_zero_window_is_empty() {
        assert_eq!(letterbox(Size::new(0.0, 100.0)).w, 0.0);
    }

    #[test]
    fn mouse_mapping_round_trips_and_rejects_the_bars() {
        let lb = letterbox(Size::new(800.0, 600.0));
        let (vx, vy) = to_virtual(400.0, 300.0, lb).unwrap();
        assert!(close(vx, 960.0) && close(vy, 540.0));

        let back = to_window(Rect::new(960.0, 540.0, 0.0, 0.0), lb);
        assert!(close(back.x, 400.0) && close(back.y, 300.0));

        assert!(to_virtual(400.0, 10.0, lb).is_none()); // in the top bar
    }

    #[test]
    fn ndc_of_the_whole_window_is_the_whole_clip_space() {
        let ndc = to_ndc(Rect::new(0.0, 0.0, 800.0, 600.0), Size::new(800.0, 600.0));
        assert_eq!(ndc, [-1.0, 1.0, 1.0, -1.0]);
    }

    #[test]
    fn ndc_of_the_centre_quarter() {
        let ndc = to_ndc(Rect::new(200.0, 150.0, 400.0, 300.0), Size::new(800.0, 600.0));
        assert_eq!(ndc, [-0.5, 0.5, 0.5, -0.5]);
    }

    #[test]
    fn contain_letterboxes_a_wide_picture() {
        // 2:1 picture into a 1920x1080 box: full width, 960 tall, centred.
        let p = place(Size::new(200.0, 100.0), Rect::new(0.0, 0.0, 1920.0, 1080.0), Fit::Contain);
        assert!(rect_close(p.dest, Rect::new(0.0, 60.0, 1920.0, 960.0)));
        assert_eq!(p.uv, FULL_UV);
    }

    #[test]
    fn cover_crops_through_uv_and_fills_the_container() {
        // 2:1 picture into a 16:9 box: full height, sides cropped.
        let container = Rect::new(0.0, 0.0, 1920.0, 1080.0);
        let p = place(Size::new(2000.0, 1000.0), container, Fit::Cover);
        assert_eq!(p.dest, container);
        assert!(close(p.uv.h, 1.0));
        assert!(close(p.uv.w, (16.0 / 9.0) / 2.0));
        assert!(close(p.uv.x, (1.0 - p.uv.w) / 2.0));
    }

    #[test]
    fn stretch_ignores_aspect() {
        let container = Rect::new(10.0, 20.0, 300.0, 100.0);
        let p = place(Size::new(50.0, 500.0), container, Fit::Stretch);
        assert_eq!(p.dest, container);
        assert_eq!(p.uv, FULL_UV);
    }

    #[test]
    fn degenerate_content_does_not_divide_by_zero() {
        let container = Rect::new(0.0, 0.0, 100.0, 100.0);
        let p = place(Size::new(0.0, 0.0), container, Fit::Cover);
        assert_eq!(p.dest, container);
    }

    #[test]
    fn line_estimates() {
        assert_eq!(estimate_lines("", 32.0, 1000.0), 1);
        assert_eq!(estimate_lines("short", 32.0, 1000.0), 1);
        assert_eq!(estimate_lines("a\nb\nc", 32.0, 1000.0), 3);
        let long = "word ".repeat(200);
        let n = estimate_lines(&long, 32.0, 1260.0);
        assert!((10..=16).contains(&n), "{n}");
        // CJK has no spaces and wide glyphs: about width/size characters per line
        assert_eq!(estimate_lines(&"字".repeat(80), 32.0, 1280.0), 2);
    }
}
