//! Text rasterization: [`TextJob`] -> RGBA pixels.
//!
//! With the `text` feature this uses cosmic-text. Without it, [`TextRenderer::rasterize`]
//! returns `None` and text simply isn't drawn.

use quillstone_render::TextJob;

pub struct Bitmap {
    pub width: u32,
    pub height: u32,
    /// RGBA8, straight (non-premultiplied) alpha, row-major.
    pub rgba: Vec<u8>,
}

#[cfg(feature = "text")]
pub use real::TextRenderer;
#[cfg(not(feature = "text"))]
pub use stub::TextRenderer;

#[cfg(not(feature = "text"))]
mod stub {
    use super::*;

    pub struct TextRenderer;

    impl TextRenderer {
        pub fn new(_fonts: Vec<Vec<u8>>) -> Self {
            TextRenderer
        }

        pub fn rasterize(&mut self, _job: &TextJob) -> Option<Bitmap> {
            None
        }

        pub fn recycle(&mut self, _bitmap: Bitmap) {}
    }
}

#[cfg(feature = "text")]
mod real {
    use super::*;
    use std::collections::HashMap;

    use cosmic_text::{Attrs, Buffer, Color, Family, FontSystem, Metrics, Shaping, Style, SwashCache, Weight};
    use quillstone_render::Align;

    /// Byte offset in `text` where each buffer line begins. Built from the buffer's own lines,
    /// because cosmic-text splits paragraphs at more than `\n` (`\r`, U+0085, U+2029, ...) and
    /// drops the separator: after a line comes one separator, `\r\n` counting as one.
    /// (Plain text records its own line ending; rich text always says `\n` whatever it was.)
    fn line_starts(buffer: &Buffer, text: &str, rich: bool) -> Vec<usize> {
        let mut starts = Vec::with_capacity(buffer.lines.len());
        let mut at = 0;
        for line in &buffer.lines {
            starts.push(at.min(text.len()));
            at += line.text().len();
            let rest = text.get(at..).unwrap_or("");
            at += if !rich {
                line.ending().as_str().len()
            } else if rest.starts_with("\r\n") {
                2
            } else {
                rest.chars().next().map_or(0, char::len_utf8)
            };
        }
        starts
    }

    /// The strongest stamp of every lit pixel's disc of radius `r` on each pixel; `edges_only` skips
    /// pixels that cannot change the result (see [`add_outline`]).
    pub(super) fn outline_ring(acc: &[[f32; 4]], w: usize, h: usize, r: i32, edges_only: bool) -> Vec<f32> {
        let disc: Vec<(i32, i32)> = (-r..=r)
            .flat_map(|dy| (-r..=r).map(move |dx| (dx, dy)))
            .filter(|(dx, dy)| dx * dx + dy * dy <= r * r + r) // a little over r, so small discs are round
            .collect();
        let mut ring = vec![0.0f32; w * h];
        let inside = |i: usize| {
            let (x, y, a) = (i % w, i / w, acc[i][3]);
            x > 0 && y > 0 && x + 1 < w && y + 1 < h && [i - 1, i + 1, i - w, i + w].iter().all(|&n| acc[n][3] >= a)
        };
        for (i, px) in acc.iter().enumerate().filter(|(_, px)| px[3] > 0.0) {
            if edges_only && inside(i) {
                ring[i] = ring[i].max(px[3]); // still its own cover, which it would have stamped on itself
                continue;
            }
            let (x, y) = ((i % w) as i32, (i / w) as i32);
            for (dx, dy) in &disc {
                let (nx, ny) = (x + dx, y + dy);
                if nx >= 0 && ny >= 0 && (nx as usize) < w && (ny as usize) < h {
                    let at = &mut ring[ny as usize * w + nx as usize];
                    *at = at.max(px[3]);
                }
            }
        }
        ring
    }

    /// Put `outline` under the letters already in `acc` (premultiplied): every lit pixel stamps its
    /// coverage on the pixels within the outline's radius, the strongest stamp wins, and the letters
    /// are laid over the result. A pixel whose four neighbours are all at least as strong is skipped:
    /// it only keeps its own coverage; the discs it would stamp lie inside its neighbours' discs, which are at
    /// least as strong, so the result is the same, and a letter's inside is most of its pixels. The radius is capped at 8 window px (a huge outline at 4K cost 100 ms a frame).
    fn add_outline(acc: &mut [[f32; 4]], w: usize, h: usize, outline: &quillstone_render::Outline) {
        let r = outline.size.round().clamp(1.0, 8.0) as i32;
        let ring = outline_ring(acc, w, h, r, true);
        let [r, g, b, a] = outline.color;
        for (px, cover) in acc.iter_mut().zip(ring) {
            let under = cover * a * (1.0 - px[3]); // what the letters leave uncovered
            *px = [px[0] + r * under, px[1] + g * under, px[2] + b * under, px[3] + under];
        }
    }

    pub struct TextRenderer {
        fonts: FontSystem,
        swash: SwashCache,
        /// The theme font's family name, if one was loaded; else system fonts.
        family: Option<String>,
        /// Shaped text by [`TextJob::layout_key`], so a line typing out is shaped
        /// once rather than once per character.
        layouts: HashMap<String, Buffer>,
        /// The coverage buffer of [`TextRenderer::draw`], kept so a line typing out does not allocate
        /// megabytes per character.
        acc: Vec<[f32; 4]>,
        /// A pixel buffer handed back by [`TextRenderer::recycle`], for the next bitmap.
        spare: Vec<u8>,
    }

    /// Layouts kept before the cache starts afresh (a screen shows a few dozen texts).
    const MAX_LAYOUTS: usize = 128;

    impl TextRenderer {
        /// `files`: `.ttf` / `.otf` contents to use for all text, regular first, then
        /// the same family's bold and italic faces (picked for `**bold**` / `*italic*`).
        /// System fonts stay available as a fallback for characters they lack.
        pub fn new(files: Vec<Vec<u8>>) -> Self {
            let mut fonts = FontSystem::new();
            let mut family = None;
            for (i, data) in files.into_iter().enumerate() {
                let before = fonts.db().faces().count();
                fonts.db_mut().load_font_data(data);
                let name = fonts.db().faces().nth(before).and_then(|f| f.families.first()).map(|(name, _)| name.clone());
                match name {
                    Some(name) if i == 0 => family = Some(name),
                    Some(_) => {}
                    None => eprintln!("warning: one of the theme's font files isn't a font cosmic-text can read"),
                }
            }
            TextRenderer { fonts, swash: SwashCache::new(), family, layouts: HashMap::new(), acc: Vec::new(), spare: Vec::new() }
        }

        pub fn rasterize(&mut self, job: &TextJob) -> Option<Bitmap> {
            let width = job.rect.w.max(1.0) as u32;
            let height = job.rect.h.max(1.0) as u32;
            // cosmic-text panics on a zero line height, so never go below 1px.
            let size_px = job.size_px.max(1.0);
            let layout_key = job.layout_key();
            let buffer = match self.layouts.remove(&layout_key) {
                Some(buffer) => buffer,
                None => self.shape_to_fit(job, width, height, size_px),
            };
            let bitmap = self.draw(job, &buffer, width, height);
            if self.layouts.len() >= MAX_LAYOUTS {
                self.layouts.clear();
            }
            self.layouts.insert(layout_key, buffer);
            Some(bitmap)
        }

        /// Give a bitmap's pixel buffer back once it is uploaded, to be drawn into again.
        pub fn recycle(&mut self, bitmap: Bitmap) {
            self.spare = bitmap.rgba;
        }

        /// Shape at `size_px`, or smaller if the text is too tall for its box
        /// (down to 60%; past that it is cut off at the bottom).
        fn shape_to_fit(&mut self, job: &TextJob, width: u32, height: u32, size_px: f32) -> Buffer {
            let mut size = size_px;
            loop {
                let line_height = quillstone_render::line_height(size);
                let buffer = self.shape(job, width, Metrics::new(size, line_height));
                let tall: f32 = buffer.layout_runs().map(|run| run.line_height).sum();
                if tall <= height as f32 || size <= size_px * 0.6 {
                    return buffer;
                }
                size = (size * 0.9).max(size_px * 0.6);
            }
        }

        /// Lay out the *whole* text so wrapping never changes while it types out.
        fn shape(&mut self, job: &TextJob, width: u32, metrics: Metrics) -> Buffer {
            let mut buffer = Buffer::new(&mut self.fonts, metrics);
            {
                let mut b = buffer.borrow_with(&mut self.fonts);
                b.set_size(Some(width as f32), None);
                let attrs = match &self.family {
                    Some(name) => Attrs::new().family(Family::Name(name)),
                    None => Attrs::new(),
                };
                if job.runs.is_empty() {
                    b.set_text(&job.text, &attrs, Shaping::Advanced, None);
                } else {
                    let byte = |c: f32| (c.clamp(0.0, 1.0) * 255.0).round() as u8;
                    // `{size=..}`: when any run has one, every run gets its own metrics, so a line is
                    // as tall as its largest run (a line of smaller text alone is shorter).
                    let sized = job.runs.iter().any(|run| run.style.size.is_some());
                    let shrink = metrics.font_size / job.size_px.max(1.0); // when the text was made to fit its box
                    let spans = job.runs.iter().map(|run| {
                        let mut a = attrs.clone();
                        if run.style.bold {
                            a = a.weight(Weight::BOLD);
                        }
                        if run.style.italic {
                            a = a.style(Style::Italic);
                        }
                        if let Some([r, g, b]) = run.style.color {
                            a = a.color(Color::rgb(byte(r), byte(g), byte(b)));
                        }
                        if sized {
                            let px = (job.run_size_px(run) * shrink).max(1.0);
                            a = a.metrics(Metrics::new(px, quillstone_render::line_height(px)));
                        }
                        (&job.text[run.start..run.end], a)
                    });
                    b.set_rich_text(spans, &attrs, Shaping::Advanced, None);
                }
            }
            // Without this, `layout_runs()` below is empty: set_text only marks the
            // text dirty, and shaping/layout happens here.
            buffer.shape_until_scroll(&mut self.fonts, false);
            buffer
        }

        /// The typed part of a shaped text, in its colours.
        fn draw(&mut self, job: &TextJob, buffer: &Buffer, width: u32, height: u32) -> Bitmap {
            // Glyphs that start at or after this byte offset are not typed yet.
            let byte_limit = job
                .text
                .char_indices()
                .nth(job.visible_chars)
                .map(|(i, _)| i)
                .unwrap_or(usize::MAX);

            // A glyph's `start` counts from the beginning of its paragraph (cosmic-text keeps one
            // buffer line per paragraph), so add where that paragraph begins in the text.
            let line_starts = line_starts(buffer, &job.text, !job.runs.is_empty());

            let [r, g, b, a] = job.color.map(|c| (c.clamp(0.0, 1.0) * 255.0).round() as u8);
            let base = Color::rgba(r, g, b, 255);

            let line_count = buffer.layout_runs().count();
            let block_h: f32 = buffer.layout_runs().map(|run| run.line_height).sum();
            let y_off = match job.align {
                Align::Center => ((height as f32 - block_h) / 2.0).max(0.0),
                Align::Left | Align::CenterX | Align::Right => 0.0,
            };

            if line_count == 0 && !job.text.trim().is_empty() {
                eprintln!(
                    "warning: text produced no layout lines ({:?}); fonts loaded: {}",
                    job.text,
                    self.fonts.db().len()
                );
            }

            // Premultiplied colour and coverage per pixel (coloured runs differ).
            let mut acc = std::mem::take(&mut self.acc);
            acc.clear();
            acc.resize((width * height) as usize, [0.0; 4]);
            for run in buffer.layout_runs() {
                let x_off = match job.align {
                    Align::Center | Align::CenterX => ((width as f32 - run.line_w) / 2.0).max(0.0),
                    Align::Left => 0.0,
                    Align::Right => (width as f32 - run.line_w).max(0.0),
                };
                let line_y = (run.line_y + y_off).round() as i32;
                for glyph in run.glyphs.iter() {
                    if line_starts.get(run.line_i).copied().unwrap_or(0) + glyph.start >= byte_limit {
                        continue;
                    }
                    let physical = glyph.physical((x_off, 0.0), 1.0);
                    let glyph_color = glyph.color_opt.unwrap_or(base);
                    self.swash.with_pixels(&mut self.fonts, physical.cache_key, glyph_color, |x, y, color| {
                        let px = physical.x + x;
                        let py = line_y + physical.y + y;
                        if px < 0 || py < 0 || px >= width as i32 || py >= height as i32 {
                            return;
                        }
                        let cover = color.a() as f32 / 255.0;
                        let slot = &mut acc[py as usize * width as usize + px as usize];
                        let src = [color.r(), color.g(), color.b()].map(|c| c as f32 / 255.0 * cover);
                        for k in 0..3 {
                            slot[k] = src[k] + slot[k] * (1.0 - cover); // source-over
                        }
                        slot[3] = cover + slot[3] * (1.0 - cover);
                    });
                }
            }

            if let Some(outline) = &job.outline {
                add_outline(&mut acc, width as usize, height as usize, outline);
            }

            // Back to straight alpha; `a` scales the whole text's opacity.
            let opacity = a as f32 / 255.0;
            let mut rgba = std::mem::take(&mut self.spare);
            rgba.clear();
            rgba.resize((width * height * 4) as usize, 0);
            for (i, [pr, pg, pb, cover]) in acc.iter().enumerate() {
                if *cover > 0.0 {
                    let straight = |c: f32| ((c / cover).clamp(0.0, 1.0) * 255.0).round() as u8;
                    rgba[i * 4] = straight(*pr);
                    rgba[i * 4 + 1] = straight(*pg);
                    rgba[i * 4 + 2] = straight(*pb);
                    rgba[i * 4 + 3] = (cover * opacity * 255.0).round() as u8;
                }
            }
            self.acc = acc;
            Bitmap { width, height, rgba }
        }
    }
}

#[cfg(all(test, feature = "text"))]
mod tests {
    use super::real::outline_ring;
    use super::*;
    use quillstone_render::{build_frame, Align, Draw, Rect, Size};
    use quillstone_script::markup::rich;

    fn job(tagged: &str) -> TextJob {
        let r = rich(tagged);
        let draw = Draw::Text {
            text: r.text,
            runs: r.runs,
            visible_chars: usize::MAX,
            rect: Rect::new(0.0, 0.0, 1200.0, 400.0),
            size: 40.0,
            color: [1.0; 4],
            align: Align::Left,
            outline: None,
        };
        build_frame(&[draw], Size::new(1920.0, 1080.0)).texts.remove(0)
    }

    /// Row of the lowest lit pixel: how far down the text reaches.
    fn bottom(bitmap: &Bitmap) -> usize {
        let rows = bitmap.rgba.chunks(bitmap.width as usize * 4);
        rows.enumerate().filter(|(_, row)| row.chunks(4).any(|p| p[3] > 0)).map(|(y, _)| y).max().unwrap_or(0)
    }

    #[test]
    fn a_larger_run_makes_its_line_taller_and_the_text_bigger() {
        let mut renderer = TextRenderer::new(Vec::new());
        let plain = renderer.rasterize(&job("Hello there friend")).unwrap();
        if bottom(&plain) == 0 {
            eprintln!("SKIPPED (no fonts on this machine): size test did not run");
            return;
        }
        let big = renderer.rasterize(&job("Hello {size=+40}there{/size} friend")).unwrap();
        let small = renderer.rasterize(&job("{size=12}Hello there friend{/size}")).unwrap();
        let normal = renderer.rasterize(&job("Hello there friend")).unwrap();
        let lit = |b: &Bitmap| b.rgba.chunks(4).filter(|p| p[3] > 0).count();
        assert!(lit(&big) > lit(&normal), "bigger letters cover more pixels");
        assert!(lit(&small) < lit(&normal));
        assert!(bottom(&big) > bottom(&normal), "the big word reaches lower");
    }

    #[test]
    fn a_paragraph_break_is_a_real_line_break_and_types_in_order() {
        let mut renderer = TextRenderer::new(Vec::new());
        let one = renderer.rasterize(&job("Hello there")).unwrap();
        if bottom(&one) == 0 {
            eprintln!("SKIPPED (no fonts on this machine): line break test did not run");
            return;
        }
        let two = renderer.rasterize(&job("Hello{p}there")).unwrap();
        assert!(bottom(&two) > bottom(&one) + 20, "the second paragraph sits on a new line");
        // with {size}: runs and the break still line up (the break belongs to the first run)
        let sized = renderer.rasterize(&job("{size=+10}Hello{p}{/size}there")).unwrap();
        assert!(bottom(&sized) > bottom(&one) + 20);
        // typewriter: only the first paragraph is shown until its characters are revealed
        let r = rich("Hello{p}there");
        let typed = |n: usize| {
            let draw = Draw::Text {
                text: r.text.clone(),
                runs: r.runs.clone(),
                visible_chars: n,
                rect: Rect::new(0.0, 0.0, 1200.0, 400.0),
                size: 40.0,
                color: [1.0; 4],
                align: Align::Left,
                outline: None,
            };
            build_frame(&[draw], Size::new(1920.0, 1080.0)).texts.remove(0)
        };
        let first_only = renderer.rasterize(&typed(6)).unwrap(); // "Hello" and the break
        let all = renderer.rasterize(&typed(11)).unwrap();
        assert!(bottom(&first_only) < bottom(&all), "the second line appears as it is typed");
        assert_eq!(bottom(&first_only), bottom(&one), "the break itself leaves nothing to see");
    }

    fn typed(tagged: &str, n: usize) -> TextJob {
        let r = rich(tagged);
        let draw = Draw::Text {
            text: r.text,
            runs: r.runs,
            visible_chars: n,
            rect: Rect::new(0.0, 0.0, 1200.0, 400.0),
            size: 40.0,
            color: [1.0; 4],
            align: Align::Left,
            outline: None,
        };
        build_frame(&[draw], Size::new(1920.0, 1080.0)).texts.remove(0)
    }

    #[test]
    fn the_typewriter_follows_paragraphs_that_cosmic_text_splits_at_more_than_newline() {
        let mut renderer = TextRenderer::new(Vec::new());
        if bottom(&renderer.rasterize(&job("Hello there")).unwrap()) == 0 {
            eprintln!("SKIPPED (no fonts on this machine): paragraph separator test did not run");
            return;
        }
        for sep in ["\u{2028}", "\u{2029}", "\u{85}", "\r", "\r\n"] {
            let tagged = format!("Aa{sep}Bb{{p}}Cc");
            let chars_before_c = rich(&tagged).text.chars().count() - 2;
            let shown = renderer.rasterize(&typed(&tagged, chars_before_c)).unwrap();
            let all = renderer.rasterize(&typed(&tagged, usize::MAX)).unwrap();
            assert!(bottom(&shown) < bottom(&all), "the last paragraph must stay hidden until typed ({sep:?})");
        }
    }

    #[test]
    fn a_blank_paragraph_in_sized_text_is_as_tall_as_its_neighbours() {
        // cosmic-text gives the empty line the run's metrics already; this guards against a regression
        let mut renderer = TextRenderer::new(Vec::new());
        if bottom(&renderer.rasterize(&job("Hello there")).unwrap()) == 0 {
            eprintln!("SKIPPED (no fonts on this machine): blank paragraph test did not run");
            return;
        }
        let blank = renderer.rasterize(&job("{size=+30}A{p}{p}B{/size}")).unwrap();
        let filled = renderer.rasterize(&job("{size=+30}A{p}x{p}B{/size}")).unwrap();
        assert_eq!(bottom(&blank), bottom(&filled), "the empty line has the sized height");
    }

    #[test]
    fn the_outline_skips_interior_pixels_without_changing_the_ring() {
        // a soft blob with a solid core and an irregular edge
        let (w, h) = (24, 20);
        let acc: Vec<[f32; 4]> = (0..w * h)
            .map(|i| {
                let (x, y) = ((i % w) as f32 - 11.0, (i / w) as f32 - 9.0);
                let d = (x * x + y * y).sqrt();
                let a: f32 = if d < 6.0 { 1.0 } else if d < 8.0 { 0.35 + 0.1 * ((i % 5) as f32) } else { 0.0 };
                [a, a, a, a]
            })
            .collect();
        for r in [1, 2, 5] {
            assert_eq!(outline_ring(&acc, w, h, r, true), outline_ring(&acc, w, h, r, false), "radius {r}");
        }
    }

    #[test]
    fn an_outline_adds_a_ring_in_its_colour_under_the_letters_and_is_part_of_the_cache_key() {
        let mut renderer = TextRenderer::new(Vec::new());
        let plain = job("Hello there");
        let plain_map = renderer.rasterize(&plain).unwrap();
        if bottom(&plain_map) == 0 {
            eprintln!("SKIPPED (no fonts on this machine): outline test did not run");
            return;
        }
        let mut outlined = plain.clone();
        outlined.outline = Some(quillstone_render::Outline { size: 2.0, color: [1.0, 0.0, 0.0, 1.0] });
        assert_ne!(plain.key(), outlined.key());
        let ringed = renderer.rasterize(&outlined).unwrap();
        let lit = |b: &Bitmap| b.rgba.chunks(4).filter(|p| p[3] > 0).count();
        assert!(lit(&ringed) > lit(&plain_map) * 11 / 10, "the outline widens the letters");
        // white letters stay white where they were solid; the new pixels around them are red
        let (mut solid, mut ring) = (0, 0);
        for (before, after) in plain_map.rgba.chunks(4).zip(ringed.rgba.chunks(4)) {
            if before[3] == 255 {
                assert_eq!(after[..3], before[..3], "solid letter pixels are untouched");
                solid += 1;
            } else if before[3] == 0 && after[3] == 255 {
                assert_eq!(after[..3], [255, 0, 0], "outside the letters it is the outline colour");
                ring += 1;
            }
        }
        assert!(solid > 0 && ring > 0, "{solid} solid, {ring} ring pixels");
    }
}
