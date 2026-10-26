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
}
