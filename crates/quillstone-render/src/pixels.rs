//! Preparing decoded images (RGBA8, sRGB, straight alpha) for the GPU.
//!
//! Two problems are solved here, before upload:
//!
//! - **Dark fringes.** Art tools usually store fully transparent pixels as black.
//!   Linear filtering blends a sprite's edge with those neighbours, so edges get
//!   a dark halo. [`bleed_alpha`] gives transparent pixels the colour of their
//!   visible neighbours (their alpha stays 0, so nothing new becomes visible).
//! - **Shimmering when drawn small.** Without mipmaps, an image drawn at less than
//!   half its size skips texels and aliases. [`mip_chain`] builds the smaller
//!   levels, averaging in linear light and weighting colour by alpha.

/// One level of a mip chain.
#[derive(Debug, Clone, PartialEq)]
pub struct Level {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

/// Give every fully transparent pixel the average colour of its visible
/// neighbours (8-connected). Alpha is left untouched.
pub fn bleed_alpha(rgba: &mut [u8], width: u32, height: u32) {
    let (w, h) = (width as usize, height as usize);
    assert_eq!(rgba.len(), w * h * 4, "buffer does not match {width}x{height}");
    let source = rgba.to_vec();
    for y in 0..h {
        for x in 0..w {
            let i = (y * w + x) * 4;
            if source[i + 3] != 0 {
                continue;
            }
            let mut sum = [0u32; 3];
            let mut count = 0;
            for ny in y.saturating_sub(1)..(y + 2).min(h) {
                for nx in x.saturating_sub(1)..(x + 2).min(w) {
                    let j = (ny * w + nx) * 4;
                    if source[j + 3] != 0 {
                        for c in 0..3 {
                            sum[c] += source[j + c] as u32;
                        }
                        count += 1;
                    }
                }
            }
            for c in 0..3 {
                if let Some(avg) = (sum[c] + count / 2).checked_div(count) {
                    rgba[i + c] = avg as u8;
                }
            }
        }
    }
}

/// Side of the vignette picture, in pixels.
pub const VIGNETTE_SIZE: u32 = 256;
/// Distance from the centre (1 is the middle of an edge) where the vignette picture starts to darken; it is
/// fully opaque from 1 outwards, so a window larger than the picture can clamp at its edge.
const VIGNETTE_START: f32 = 0.5;

/// The vignette picture: white, its alpha 0 round the centre and rising smoothly to 1 at distance 1 from
/// it (an edge's middle) and beyond (a circle in the picture, an ellipse once stretched over the screen).
pub fn vignette_level() -> Level {
    let n = VIGNETTE_SIZE as usize;
    let mut rgba = vec![255u8; n * n * 4];
    for y in 0..n {
        for x in 0..n {
            let at = |i: usize| (i as f32 + 0.5) / n as f32 * 2.0 - 1.0;
            let t = ((at(x).hypot(at(y)) - VIGNETTE_START) / (1.0 - VIGNETTE_START)).clamp(0.0, 1.0);
            rgba[(y * n + x) * 4 + 3] = (t * t * (3.0 - 2.0 * t) * 255.0).round() as u8;
        }
    }
    Level { width: VIGNETTE_SIZE, height: VIGNETTE_SIZE, rgba }
}

/// How much of the vignette picture (its width and height, the centre staying put) the screen shows for a
/// `size` of 0..1: 0 darkens little but the corners, 1 reaches in from every edge. Corners are always opaque.
pub fn vignette_zoom(size: f32) -> f32 {
    std::f32::consts::FRAC_1_SQRT_2 + 0.5 * size.clamp(0.0, 1.0)
}

/// How soft the edge of a mask transition is, as a share of the mask's range (`shader.wgsl` has the same number).
pub const MASK_EDGE: f32 = 0.1;

/// The `reveal` threshold of a mask transition `progress` (0..1) of the way: 0 covers nothing and 1 everything,
/// the soft edge included at both ends.
pub fn mask_reveal(progress: f32) -> f32 {
    progress.clamp(0.0, 1.0) * (1.0 + MASK_EDGE)
}

/// How much of the new picture shows where the mask reads `mask` (0 dark .. 1 white): dark places go first.
/// `reveal` above 1 + [`MASK_EDGE`] shows everything.
pub fn mask_coverage(mask: f32, reveal: f32) -> f32 {
    let t = ((reveal - mask) / MASK_EDGE).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Number of levels in a full mip chain for this size (down to 1x1).
pub fn mip_level_count(width: u32, height: u32) -> u32 {
    32 - width.max(height).max(1).leading_zeros()
}

/// The full mip chain, starting with the image itself. Each level halves the
/// previous one (rounding down, never below 1) with a 2x2 box filter.
pub fn mip_chain(width: u32, height: u32, rgba: Vec<u8>) -> Vec<Level> {
    let to_linear: Vec<f32> = (0..=255u8).map(|v| srgb_to_linear(v as f32 / 255.0)).collect();
    let mut levels = vec![Level { width, height, rgba }];
    while let Some(prev) = levels.last().filter(|l| l.width > 1 || l.height > 1) {
        let next = downsample(prev, &to_linear);
        levels.push(next);
    }
    levels
}

fn downsample(src: &Level, to_linear: &[f32]) -> Level {
    let (sw, sh) = (src.width as usize, src.height as usize);
    let (w, h) = ((sw / 2).max(1), (sh / 2).max(1));
    let mut rgba = vec![0u8; w * h * 4];
    for y in 0..h {
        for x in 0..w {
            let mut weighted = [0.0f32; 3]; // colour * alpha, linear light
            let mut plain = [0.0f32; 3]; // colour alone, for all-transparent blocks
            let mut alpha = 0.0f32;
            let mut n = 0.0f32;
            for sy in (2 * y)..(2 * y + 2).min(sh) {
                for sx in (2 * x)..(2 * x + 2).min(sw) {
                    let i = (sy * sw + sx) * 4;
                    let a = src.rgba[i + 3] as f32 / 255.0;
                    for c in 0..3 {
                        let lin = to_linear[src.rgba[i + c] as usize];
                        weighted[c] += lin * a;
                        plain[c] += lin;
                    }
                    alpha += a;
                    n += 1.0;
                }
            }
            let o = (y * w + x) * 4;
            for c in 0..3 {
                let lin = if alpha > 0.0 { weighted[c] / alpha } else { plain[c] / n };
                rgba[o + c] = (linear_to_srgb(lin) * 255.0).round() as u8;
            }
            rgba[o + 3] = (alpha / n * 255.0).round() as u8;
        }
    }
    let mut level = Level { width: w as u32, height: h as u32, rgba };
    // Transparent texels next to visible ones should carry their colour too.
    bleed_alpha(&mut level.rgba, level.width, level.height);
    level
}

fn srgb_to_linear(c: f32) -> f32 {
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

fn linear_to_srgb(c: f32) -> f32 {
    let c = c.clamp(0.0, 1.0);
    if c <= 0.0031308 {
        c * 12.92
    } else {
        1.055 * c.powf(1.0 / 2.4) - 0.055
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn px(rgba: &[u8], width: u32, x: u32, y: u32) -> [u8; 4] {
        let i = ((y * width + x) * 4) as usize;
        rgba[i..i + 4].try_into().unwrap()
    }

    #[test]
    fn transparent_pixels_take_their_neighbours_colour_but_stay_invisible() {
        // A red pixel in the middle of a transparent black 3x3.
        let mut img = vec![0u8; 3 * 3 * 4];
        img[16..20].copy_from_slice(&[255, 0, 0, 255]);
        bleed_alpha(&mut img, 3, 3);
        for (x, y) in [(0, 0), (1, 0), (2, 2)] {
            assert_eq!(px(&img, 3, x, y), [255, 0, 0, 0]);
        }
        assert_eq!(px(&img, 3, 1, 1), [255, 0, 0, 255]);
    }

    #[test]
    fn bleeding_averages_and_leaves_far_and_visible_pixels_alone() {
        // 5x1: red, gap, half-visible blue, gap, and a pixel two away from anything.
        let mut img = vec![0u8; 5 * 4];
        img[0..4].copy_from_slice(&[200, 0, 0, 255]);
        img[8..12].copy_from_slice(&[0, 0, 100, 128]);
        bleed_alpha(&mut img, 5, 1);
        assert_eq!(px(&img, 5, 1, 0), [100, 0, 50, 0]); // between red and blue
        assert_eq!(px(&img, 5, 2, 0), [0, 0, 100, 128]); // visible: untouched
        assert_eq!(px(&img, 5, 3, 0), [0, 0, 100, 0]);
        assert_eq!(px(&img, 5, 4, 0), [0, 0, 0, 0]); // nothing visible nearby
    }

    #[test]
    fn level_counts() {
        assert_eq!(mip_level_count(1, 1), 1);
        assert_eq!(mip_level_count(2, 1), 2);
        assert_eq!(mip_level_count(1920, 1080), 11);
        assert_eq!(mip_level_count(560, 1000), 10);
    }

    #[test]
    fn the_chain_halves_down_to_one_pixel() {
        let chain = mip_chain(5, 3, vec![255; 5 * 3 * 4]);
        let sizes: Vec<(u32, u32)> = chain.iter().map(|l| (l.width, l.height)).collect();
        assert_eq!(sizes, [(5, 3), (2, 1), (1, 1)]);
        assert_eq!(chain.len() as u32, mip_level_count(5, 3));
        assert!(chain.iter().all(|l| l.rgba.len() == (l.width * l.height * 4) as usize));
        assert_eq!(px(&chain[2].rgba, 1, 0, 0), [255; 4]);
    }

    #[test]
    fn averaging_is_in_linear_light() {
        // Black and white average to linear 0.5, which is sRGB ~188, not 128.
        let img = [0, 0, 0, 255, 255, 255, 255, 255].to_vec();
        let chain = mip_chain(2, 1, img);
        let [r, g, b, a] = px(&chain[1].rgba, 1, 0, 0);
        assert_eq!((r, g, b, a), (188, 188, 188, 255));
    }

    #[test]
    fn transparent_texels_do_not_darken_the_colour() {
        // Opaque red next to transparent black: the smaller level stays pure red
        // at half coverage (the black that a plain average would mix in is ignored).
        let img = [255, 0, 0, 255, 0, 0, 0, 0].to_vec();
        let chain = mip_chain(2, 1, img);
        assert_eq!(px(&chain[1].rgba, 1, 0, 0), [255, 0, 0, 128]);
    }

    #[test]
    fn the_vignette_is_clear_in_the_middle_and_opaque_at_the_corners_and_never_dips() {
        let l = vignette_level();
        let n = VIGNETTE_SIZE as usize;
        let alpha = |x: usize, y: usize| l.rgba[(y * n + x) * 4 + 3];
        assert_eq!((alpha(n / 2, n / 2), alpha(n / 3, n / 2), alpha(0, 0), alpha(n - 1, n - 1)), (0, 0, 255, 255));
        assert!(l.rgba.chunks(4).all(|p| p[..3] == [255; 3]), "white, so the quad colour tints it");
        // Smooth along a row from the middle to the edge: rising, in steps of a few levels at most.
        let row: Vec<u8> = (n / 2..n).map(|x| alpha(x, n / 2)).collect();
        assert!(row.windows(2).all(|w| w[0] <= w[1] && w[1] - w[0] <= 8), "{row:?}");
        assert_eq!(alpha(n - 1, n / 2), 255, "an edge's middle is at distance 1");
        assert_eq!(alpha(0, 5), alpha(n - 1, 5), "symmetric");
    }

    #[test]
    fn the_zoom_grows_with_size_and_keeps_the_corners_opaque() {
        assert!(vignette_zoom(0.0) < vignette_zoom(0.5) && vignette_zoom(0.5) < vignette_zoom(1.0));
        // A screen corner is at (zoom, zoom) in picture units: at least distance 1.
        assert!(vignette_zoom(0.0) * 2f32.sqrt() >= 1.0 - 1e-6);
        assert_eq!(vignette_zoom(9.0), vignette_zoom(1.0), "clamped");
    }

    #[test]
    fn a_mask_shows_nothing_at_the_start_and_everything_at_the_end_whatever_the_picture() {
        for m in [0.0, 0.3, 1.0] {
            assert_eq!(mask_coverage(m, mask_reveal(0.0)), 0.0, "{m}");
            assert_eq!(mask_coverage(m, mask_reveal(1.0)), 1.0, "{m}");
        }
        // half way: the dark part is in, the white part is not, in between it is soft
        let half = mask_reveal(0.5);
        assert_eq!((mask_coverage(0.0, half), mask_coverage(1.0, half)), (1.0, 0.0));
        assert!(mask_coverage(0.5, half) > 0.0 && mask_coverage(0.5, half) < 1.0);
        assert!(mask_coverage(0.2, half) >= mask_coverage(0.4, half), "darker first");
    }
}
