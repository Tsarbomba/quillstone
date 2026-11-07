//! The colour filter (`::filter`) as a 3x4 matrix the GPU applies to each stage quad.

use quillstone_script::Filter;

/// `rgb' = M * rgb + offset`: three rows of `[r, g, b, offset]`.
pub type ColorMatrix = [[f32; 4]; 3];

pub const IDENTITY: ColorMatrix = [[1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0]];

/// Rec. 709 luma weights.
const LUMA: [f32; 3] = [0.2126, 0.7152, 0.0722];
/// The CSS `sepia()` matrix at full strength.
const SEPIA: [[f32; 3]; 3] = [[0.393, 0.769, 0.189], [0.349, 0.686, 0.168], [0.272, 0.534, 0.131]];

/// `a` after `b` (apply `b` first).
fn then(a: &ColorMatrix, b: &ColorMatrix) -> ColorMatrix {
    let mut out = [[0.0; 4]; 3];
    for (row, o) in a.iter().zip(&mut out) {
        for c in 0..4 {
            o[c] = (0..3).map(|k| row[k] * b[k][c]).sum::<f32>() + if c == 3 { row[3] } else { 0.0 };
        }
    }
    out
}

fn scale(k: f32) -> ColorMatrix {
    [[k, 0.0, 0.0, 0.0], [0.0, k, 0.0, 0.0], [0.0, 0.0, k, 0.0]]
}

/// Blend from the identity (`t` = 0) to `target` (`t` = 1).
fn toward(target: [[f32; 3]; 3], t: f32) -> ColorMatrix {
    let mut m = IDENTITY;
    for (r, row) in m.iter_mut().enumerate() {
        for c in 0..3 {
            row[c] += (target[r][c] - row[c]) * t;
        }
    }
    m
}

/// The matrix for the five settings, applied in the order gray, sepia, saturation, brightness, contrast (the
/// CSS definitions; they are not clamped in between).
pub fn matrix(f: &Filter) -> ColorMatrix {
    let luma = [LUMA; 3];
    // Contrast pivots on mid-grey: c * x + 0.5 * (1 - c).
    let mut contrast = scale(f.contrast);
    for row in &mut contrast {
        row[3] = 0.5 * (1.0 - f.contrast);
    }
    [toward(luma, f.gray), toward(SEPIA, f.sepia), toward(luma, 1.0 - f.saturation), scale(f.brightness), contrast]
        .iter()
        .fold(IDENTITY, |acc, step| then(step, &acc))
}

/// `matrix`, or `None` when the filter changes nothing.
pub fn matrix_unless_neutral(f: &Filter) -> Option<ColorMatrix> {
    Some(matrix(f)).filter(|m| *m != IDENTITY)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn apply(m: &ColorMatrix, rgb: [f32; 3]) -> [f32; 3] {
        m.map(|r| (r[0] * rgb[0] + r[1] * rgb[1] + r[2] * rgb[2] + r[3]).clamp(0.0, 1.0))
    }

    fn near(a: [f32; 3], b: [f32; 3]) {
        assert!(a.iter().zip(b).all(|(x, y)| (x - y).abs() < 1e-4), "{a:?} vs {b:?}");
    }

    #[test]
    fn the_defaults_change_nothing() {
        assert_eq!(matrix(&Filter::default()), IDENTITY);
        assert_eq!(matrix_unless_neutral(&Filter::default()), None);
        assert!(matrix_unless_neutral(&Filter { gray: 0.1, ..Filter::default() }).is_some());
    }

    #[test]
    fn full_gray_is_the_luma_and_keeps_grey_and_white() {
        let m = matrix(&Filter { gray: 1.0, ..Filter::default() });
        let y = 0.2126 * 0.8 + 0.7152 * 0.4 + 0.0722 * 0.2;
        near(apply(&m, [0.8, 0.4, 0.2]), [y; 3]);
        near(apply(&m, [1.0; 3]), [1.0; 3]);
        near(apply(&m, [0.3; 3]), [0.3; 3]);
        // Half gray is halfway; saturation 0 is the same as gray 1.
        near(apply(&matrix(&Filter { gray: 0.5, ..Filter::default() }), [0.8, 0.4, 0.2]), [(0.8 + y) / 2.0, (0.4 + y) / 2.0, (0.2 + y) / 2.0]);
        near(apply(&matrix(&Filter { saturation: 0.0, ..Filter::default() }), [0.8, 0.4, 0.2]), [y; 3]);
    }

    #[test]
    fn sepia_follows_the_css_matrix() {
        let m = matrix(&Filter { sepia: 1.0, ..Filter::default() });
        near(apply(&m, [0.5, 0.5, 0.5]), [0.5 * 1.351, 0.5 * 1.203, 0.5 * 0.937]);
    }

    #[test]
    fn brightness_scales_and_contrast_pivots_on_half() {
        near(apply(&matrix(&Filter { brightness: 0.5, ..Filter::default() }), [0.8, 0.4, 0.2]), [0.4, 0.2, 0.1]);
        let c = matrix(&Filter { contrast: 2.0, ..Filter::default() });
        near(apply(&c, [0.5; 3]), [0.5; 3]);
        near(apply(&c, [0.6, 0.4, 0.5]), [0.7, 0.3, 0.5]);
        near(apply(&matrix(&Filter { contrast: 0.0, ..Filter::default() }), [0.9, 0.1, 0.3]), [0.5; 3]);
    }

    #[test]
    fn the_order_is_gray_sepia_saturation_brightness_contrast() {
        let f = Filter { gray: 1.0, sepia: 0.5, brightness: 0.8, contrast: 1.5, saturation: 1.5 };
        let step = |m: ColorMatrix, rgb| apply(&m, rgb);
        let mut rgb = [0.8, 0.4, 0.2];
        rgb = step(matrix(&Filter { gray: 1.0, ..Filter::default() }), rgb);
        rgb = step(matrix(&Filter { sepia: 0.5, ..Filter::default() }), rgb);
        rgb = step(matrix(&Filter { saturation: 1.5, ..Filter::default() }), rgb);
        rgb = step(matrix(&Filter { brightness: 0.8, ..Filter::default() }), rgb);
        rgb = step(matrix(&Filter { contrast: 1.5, ..Filter::default() }), rgb);
        // The one matrix equals the steps (no clamping in between happens for these values).
        near(apply(&matrix(&f), [0.8, 0.4, 0.2]), rgb);
        // Brightness comes before contrast: 0.6 * 0.5 = 0.3, then 0.3 * 2 - 0.5 = 0.1 (the other way gives 0.35).
        near(apply(&matrix(&Filter { brightness: 0.5, contrast: 2.0, ..Filter::default() }), [0.6; 3]), [0.1; 3]);
    }
}
