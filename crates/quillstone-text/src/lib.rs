//! Text rasterization: [`TextJob`] -> RGBA pixels.
//!
//! With the `cosmic` feature this uses cosmic-text. Without it, [`TextRenderer::rasterize`]
//! returns `None` and text simply isn't drawn.

use quillstone_render::TextJob;

pub struct Bitmap {
    pub width: u32,
    pub height: u32,
    /// RGBA8, straight (non-premultiplied) alpha, row-major.
    pub rgba: Vec<u8>,
}

/// The moving letters of a line (`{shake}`, `{wave}`, `{glitch}`): where they are, and a picture for each of
/// `letters.pics` (same order).
pub struct LetterSet {
    pub letters: std::rc::Rc<quillstone_render::Letters>,
    pub bitmaps: Vec<Bitmap>,
}

/// How the player draws a text that has to fit its box: at `scale` of its size (1.0: whole; text too tall for its
/// box is shrunk in 10% steps down to 0.6), `cut` when even that is too tall and the end is cut off.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Fit {
    pub scale: f32,
    pub cut: bool,
}

#[cfg(feature = "cosmic")]
pub use real::{Measurer, TextRenderer};
#[cfg(not(feature = "cosmic"))]
pub use stub::{Measurer, TextRenderer};

#[cfg(not(feature = "cosmic"))]
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

        pub fn letters(&mut self, _job: &TextJob) -> Option<std::rc::Rc<LetterSet>> {
            None
        }
    }

    /// Without text there is nothing to measure: layouts fall back to their estimate.
    pub struct Measurer;

    impl Measurer {
        pub fn new(_fonts: Vec<Vec<u8>>) -> Self {
            Measurer
        }

        pub fn measure(&self, _text: &str, _size: f32, _width: f32) -> Option<(usize, f32)> {
            None
        }

        pub fn fit(&self, _job: &TextJob) -> Option<Fit> {
            None
        }

        /// The measure as a function, as `Game::set_text_measure` takes it.
        pub fn into_measure(self) -> quillstone_render::MeasureFn {
            Box::new(|_, _, _| None)
        }
    }
}

#[cfg(feature = "cosmic")]
mod real {
    use super::*;
    use std::collections::HashMap;

    use cosmic_text::{Attrs, Buffer, CacheKey, Color, Family, FontSystem, Metrics, Shaping, Style, SwashCache, UnderlineStyle, Weight};
    use quillstone_render::{Align, Anim, Letter, Letters, Pic};
        use std::rc::Rc;

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

    /// Lay `color` over the premultiplied pixel at (`px`, `py`), if it is inside the `width` x `height` bitmap.
    fn plot(acc: &mut [[f32; 4]], (width, height): (i32, i32), (px, py): (i32, i32), color: Color) {
        if px < 0 || py < 0 || px >= width || py >= height {
            return;
        }
        let cover = color.a() as f32 / 255.0;
        let slot = &mut acc[(py * width + px) as usize];
        let src = [color.r(), color.g(), color.b()].map(|c| c as f32 / 255.0 * cover);
        for k in 0..3 {
            slot[k] = src[k] + slot[k] * (1.0 - cover); // source-over
        }
        slot[3] = cover + slot[3] * (1.0 - cover);
    }

    /// `color` with its alpha scaled by `k`.
    fn faded(color: Color, k: f32) -> Color {
        Color::rgba(color.r(), color.g(), color.b(), (color.a() as f32 * k.clamp(0.0, 1.0)).round() as u8)
    }

    /// A colour channel in 0..1 as a byte.
    fn byte(c: f32) -> u8 {
        (c.clamp(0.0, 1.0) * 255.0).round() as u8
    }

    /// The attributes every text starts from: the theme font's family, if one was loaded.
    fn base_attrs(family: &Option<String>) -> Attrs<'_> {
        match family {
            Some(name) => Attrs::new().family(Family::Name(name)),
            None => Attrs::new(),
        }
    }

    /// Plain text of `px` size shaped with `attrs`, wrapped at `width` (`None`: one line).
    fn plain_buffer(fonts: &mut FontSystem, text: &str, attrs: &Attrs, px: f32, width: Option<f32>) -> Buffer {
        let mut buffer = Buffer::new(fonts, Metrics::new(px, quillstone_render::line_height(px)));
        buffer.borrow_with(fonts).set_size(width, None);
        buffer.borrow_with(fonts).set_text(text, attrs, Shaping::Advanced, None);
        buffer.shape_until_scroll(fonts, false);
        buffer
    }

    /// How far down a text block is pushed in a box `height` tall: `Align::Center` puts it in the middle.
    /// The drawn line and its moving letters both use this, so letters sit in their gaps.
    fn block_top(buffer: &Buffer, align: Align, height: u32) -> f32 {
        let block_h: f32 = buffer.layout_runs().map(|run| run.line_height).sum();
        match align {
            Align::Center => ((height as f32 - block_h) / 2.0).max(0.0),
            Align::Left | Align::CenterX | Align::Right => 0.0,
        }
    }

    /// Where a line `line_w` wide starts across a text `width` wide.
    fn line_x(align: Align, width: f32, line_w: f32) -> f32 {
        match align {
            Align::Center | Align::CenterX => ((width - line_w) / 2.0).max(0.0),
            Align::Left => 0.0,
            Align::Right => (width - line_w).max(0.0),
        }
    }

    /// Premultiplied coverage back to straight alpha RGBA8 (`rgba` is reused), scaled by `opacity`.
    fn to_rgba(acc: &[[f32; 4]], opacity: f32, rgba: &mut Vec<u8>) {
        rgba.clear();
        rgba.resize(acc.len() * 4, 0);
        for (i, [pr, pg, pb, cover]) in acc.iter().enumerate() {
            if *cover > 0.0 {
                let straight = |c: f32| ((c / cover).clamp(0.0, 1.0) * 255.0).round() as u8;
                rgba[i * 4] = straight(*pr);
                rgba[i * 4 + 1] = straight(*pg);
                rgba[i * 4 + 2] = straight(*pb);
                rgba[i * 4 + 3] = (cover * opacity * 255.0).round() as u8;
            }
        }
    }

    /// A ruby reading is this fraction of the size of the text it sits on.
    const RUBY_SCALE: f32 = 0.5;
    /// Extra line height for a line with ruby, in text sizes (cosmic-text puts half of it above the text).
    const RUBY_ROOM: f32 = 0.85;
    /// How far above the baseline the reading's own baseline is, in text sizes (just above the tallest glyphs).
    const RUBY_LIFT: f32 = 0.95;

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
        /// The moving letters of lines, by [`TextJob::letters_key`].
        sets: HashMap<String, Rc<LetterSet>>,
    }

    /// Measures text for layouts that fit themselves to it (see `quillstone_render::Measure`). It has a text engine of its
    /// own, made when first asked, and remembers its answers (a screen asks the same ones every frame).
    type MeasureKey = (String, u32, u32);

    pub struct Measurer {
        files: Vec<Vec<u8>>,
        engine: std::cell::RefCell<Option<TextRenderer>>,
        cache: std::cell::RefCell<HashMap<MeasureKey, (usize, f32)>>,
        fits: std::cell::RefCell<HashMap<String, Option<Fit>>>,
    }

    impl Measurer {
        pub fn new(files: Vec<Vec<u8>>) -> Self {
            Measurer { files, engine: Default::default(), cache: Default::default(), fits: Default::default() }
        }

        pub fn measure(&self, text: &str, size: f32, width: f32) -> Option<(usize, f32)> {
            let key = (text.to_string(), size.to_bits(), width.to_bits());
            if let Some(found) = self.cache.borrow().get(&key) {
                return Some(*found);
            }
            let mut engine = self.engine.borrow_mut();
            let found = engine.get_or_insert_with(|| TextRenderer::new(self.files.clone())).measure(text, size, width);
            let mut cache = self.cache.borrow_mut();
            if cache.len() >= 8192 {
                cache.clear();
            }
            cache.insert(key, found);
            Some(found)
        }

        /// The measure as a function, as `Game::set_text_measure` takes it.
        pub fn into_measure(self) -> quillstone_render::MeasureFn {
            Box::new(move |text, size, width| self.measure(text, size, width))
        }

        /// How the player draws `job`: whole, shrunk to fit its box, or cut off (the same shaping and fit test as
        /// `TextRenderer::rasterize`). `None` when there is no font to lay the text out with. Answers are kept.
        pub fn fit(&self, job: &TextJob) -> Option<Fit> {
            let key = job.layout_key();
            if let Some(found) = self.fits.borrow().get(&key) {
                return *found;
            }
            let mut engine = self.engine.borrow_mut();
            let found = engine.get_or_insert_with(|| TextRenderer::new(self.files.clone())).fit(job);
            self.fits.borrow_mut().insert(key, found);
            found
        }
    }

    /// How far (px) the laid-out text is taller than its `height`; at most 0 when it fits. Off the 1920x1080 size the
    /// lines' heights are rounded to whole pixels, so N lines that exactly fill a box measured in virtual pixels can be
    /// up to half a pixel per line taller in the window; that much is allowed, or such a box would shrink its text.
    fn too_tall(buffer: &Buffer, job: &TextJob, height: u32) -> f32 {
        let (mut tall, mut lines) = (0.0f32, 0);
        for run in buffer.layout_runs() {
            tall += run.line_height;
            lines += 1;
        }
        let slack = if (job.scale - 1.0).abs() < 1e-3 { 0.0 } else { 0.5 * lines as f32 };
        tall - slack - height as f32
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
            TextRenderer { fonts, swash: SwashCache::new(), family, layouts: HashMap::new(), acc: Vec::new(), spare: Vec::new(), sets: HashMap::new() }
        }

        pub fn rasterize(&mut self, job: &TextJob) -> Option<Bitmap> {
            let (width, height) = (job.rect.w.max(1.0) as u32, job.rect.h.max(1.0) as u32);
            let (layout_key, buffer) = self.take_layout(job);
            let bitmap = self.draw(job, &buffer, width, height);
            self.keep_layout(layout_key, buffer);
            Some(bitmap)
        }

        /// The shaped text of `job` (from the cache, or shaped now) and its key; give it back with
        /// [`TextRenderer::keep_layout`].
        fn take_layout(&mut self, job: &TextJob) -> (String, Buffer) {
            let (width, height) = (job.rect.w.max(1.0) as u32, job.rect.h.max(1.0) as u32);
            let layout_key = job.layout_key();
            let buffer = match self.layouts.remove(&layout_key) {
                Some(buffer) => buffer,
                // cosmic-text panics on a zero line height, so never go below 1px.
                None => self.shape_to_fit(job, width, height, job.size_px.max(1.0)),
            };
            (layout_key, buffer)
        }

        fn keep_layout(&mut self, key: String, buffer: Buffer) {
            if self.layouts.len() >= MAX_LAYOUTS {
                self.layouts.clear();
            }
            self.layouts.insert(key, buffer);
        }

        /// The letters of `job` that move, each as a picture of its own (see `quillstone_render::letters`), or
        /// `None` when nothing in it moves. Kept until a few other lines have been asked for.
        pub fn letters(&mut self, job: &TextJob) -> Option<Rc<LetterSet>> {
            self.letters_keyed(job, &job.letters_key())
        }

        /// [`TextRenderer::letters`] for a caller that has the job's `key` ([`TextJob::letters_key`]) already.
        pub fn letters_keyed(&mut self, job: &TextJob, key: &str) -> Option<Rc<LetterSet>> {
            if !job.animated() {
                return None;
            }
            if let Some(set) = self.sets.get(key) {
                return Some(set.clone());
            }
            let (layout_key, buffer) = self.take_layout(job);
            let set = Rc::new(self.make_letters(job, &buffer));
            self.keep_layout(layout_key, buffer);
            if self.sets.len() >= MAX_LAYOUTS {
                self.sets.clear();
            }
            self.sets.insert(key.to_string(), set.clone());
            Some(set)
        }

        /// How `text` wraps at `size` px in `width` px: the lines it makes and the widest one's width, as
        /// [`TextRenderer::rasterize`] will lay it out.
        fn measure(&mut self, text: &str, size: f32, width: f32) -> (usize, f32) {
            let buffer = plain_buffer(&mut self.fonts, text, &base_attrs(&self.family), size.max(1.0), Some(width.max(1.0)));
            let (mut lines, mut widest) = (0, 0.0f32);
            for run in buffer.layout_runs() {
                lines += 1;
                widest = widest.max(run.line_w);
            }
            (lines.max(1), widest)
        }

        /// How `job` is drawn: see [`Measurer::fit`]. `None` when no font could be found (nothing was laid out).
        fn fit(&mut self, job: &TextJob) -> Option<Fit> {
            let (width, height) = (job.rect.w.max(1.0) as u32, job.rect.h.max(1.0) as u32);
            let size_px = job.size_px.max(1.0);
            let buffer = self.shape_to_fit(job, width, height, size_px);
            let laid_out = buffer.layout_runs().next().is_some();
            laid_out.then(|| Fit { scale: buffer.metrics().font_size / size_px, cut: too_tall(&buffer, job, height) > 0.0 })
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
                if too_tall(&buffer, job, height) <= 0.0 || size <= size_px * 0.6 {
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
                let attrs = base_attrs(&self.family);
                if job.runs.is_empty() {
                    b.set_text(&job.text, &attrs, Shaping::Advanced, None);
                } else {
                    // `{size=..}`: when any run has one, every run gets its own metrics, so a line is
                    // as tall as its largest run (a line of smaller text alone is shorter).
                    // A run with ruby also gets its own, taller line (room for the reading above).
                    let sized = job.runs.iter().any(|run| run.style.size.is_some() || run.style.whisper || run.ruby.is_some());
                    let shrink = metrics.font_size / job.size_px.max(1.0); // when the text was made to fit its box
                    let spans = job.runs.iter().enumerate().map(|(i, run)| {
                        let mut a = attrs.clone().metadata(i); // `draw` finds the run's opacity by it
                        if run.style.bold {
                            a = a.weight(Weight::BOLD);
                        }
                        if run.style.italic {
                            a = a.style(Style::Italic);
                        }
                        if let Some([r, g, b]) = run.style.color {
                            a = a.color(Color::rgb(byte(r), byte(g), byte(b)));
                        }
                        if run.style.underline {
                            a = a.underline(UnderlineStyle::Single);
                        }
                        if run.style.strike {
                            a = a.strikethrough();
                        }
                        if sized {
                            let px = (job.run_size_px(run) * shrink).max(1.0);
                            let room = if run.ruby.is_some() { (px * RUBY_ROOM).round() } else { 0.0 };
                            a = a.metrics(Metrics::new(px, quillstone_render::line_height(px) + room));
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

        /// A ruby reading of `px` size, centred on `centre` with its baseline at `baseline`.
        fn draw_ruby(&mut self, acc: &mut [[f32; 4]], size: (i32, i32), text: &str, color: Color, (centre, baseline): (f32, i32), px: f32) {
            let buffer = plain_buffer(&mut self.fonts, text, &base_attrs(&self.family), px.max(1.0), None);
            let wide = buffer.layout_runs().map(|run| run.line_w).fold(0.0, f32::max);
            let left = (centre - wide / 2.0).min(size.0 as f32 - wide).max(0.0);
            for run in buffer.layout_runs() {
                for glyph in run.glyphs.iter() {
                    let physical = glyph.physical((left, 0.0), 1.0);
                    let color = glyph.color_opt.unwrap_or(color);
                    self.swash.with_pixels(&mut self.fonts, physical.cache_key, color, |x, y, c| {
                        plot(acc, size, (physical.x + x, baseline + physical.y + y), c);
                    });
                }
            }
        }

        /// One glyph as a picture of its own, with the job's outline round it: where its top left is from the glyph's pen
        /// position, and the pixels. `None` when it draws nothing (a space).
        fn picture(&mut self, key: CacheKey, color: Color, opacity: f32, job: &TextJob) -> Option<(Pic, Bitmap)> {
            let mut pixels: Vec<(i32, i32, Color)> = Vec::new();
            self.swash.with_pixels(&mut self.fonts, key, color, |x, y, c| pixels.push((x, y, faded(c, opacity))));
            let lit = pixels.iter().filter(|p| p.2.a() > 0);
            let (x0, x1) = (lit.clone().map(|p| p.0).min()?, lit.clone().map(|p| p.0).max()?);
            let (y0, y1) = (lit.clone().map(|p| p.1).min()?, lit.map(|p| p.1).max()?);
            // room for the outline, which draws outside the letter
            let pad = job.outline.as_ref().map_or(0, |o| o.size.round().clamp(1.0, 8.0) as i32);
            let (w, h) = ((x1 - x0 + 1 + 2 * pad) as usize, (y1 - y0 + 1 + 2 * pad) as usize);
            let mut acc = vec![[0.0f32; 4]; w * h];
            for (x, y, c) in pixels {
                plot(&mut acc, (w as i32, h as i32), (x - x0 + pad, y - y0 + pad), c);
            }
            if let Some(outline) = &job.outline {
                add_outline(&mut acc, w, h, outline);
            }
            let mut rgba = Vec::new();
            to_rgba(&acc, job.color[3].clamp(0.0, 1.0), &mut rgba);
            Some((Pic { dx: x0 - pad, dy: y0 - pad, w: w as u32, h: h as u32 }, Bitmap { width: w as u32, height: h as u32, rgba }))
        }

        /// The symbols a glitching letter of `style` swaps to, as pictures in the text's colour.
        fn glitch_pictures(&mut self, glyph: &cosmic_text::LayoutGlyph, color: Color, opacity: f32, job: &TextJob) -> Vec<(Pic, Bitmap)> {
            let run = job.runs.get(glyph.metadata);
            let family = self.family.clone(); // `picture` borrows `self` whole
            let mut attrs = base_attrs(&family);
            if run.is_some_and(|r| r.style.bold) {
                attrs = attrs.weight(Weight::BOLD);
            }
            if run.is_some_and(|r| r.style.italic) {
                attrs = attrs.style(Style::Italic);
            }
            let mut out = Vec::new();
            for symbol in quillstone_render::letters::GLITCH_CHARS.chars() {
                let buffer = plain_buffer(&mut self.fonts, &symbol.to_string(), &attrs, glyph.font_size.max(1.0), None);
                let key = buffer.layout_runs().next().and_then(|run| run.glyphs.first().map(|g| g.physical((0.0, 0.0), 1.0).cache_key));
                // a symbol the fonts can't draw is a blank one: the letter just doesn't change
                out.extend(key.and_then(|key| self.picture(key, color, opacity, job)));
            }
            // narrowest first: a letter can pick among the first few that fit it
            out.sort_by_key(|(pic, _)| pic.w);
            out
        }

        /// Every moving letter of a shaped text as a picture of its own, at its place in the line.
        fn make_letters(&mut self, job: &TextJob, buffer: &Buffer) -> LetterSet {
            let (width, height) = (job.rect.w.max(1.0) as u32, job.rect.h.max(1.0) as u32);
            let line_starts = line_starts(buffer, &job.text, !job.runs.is_empty());
            let [r, g, b, _] = job.color.map(byte);
            let base = Color::rgba(r, g, b, 255);
            let y_off = block_top(buffer, job.align, height);
            let mut letters = Letters::default();
            let mut bitmaps = Vec::new();
            let mut symbols: HashMap<usize, std::ops::Range<usize>> = HashMap::new(); // by run
            for run in buffer.layout_runs() {
                let x_off = line_x(job.align, width as f32, run.line_w);
                let line_y = (run.line_y + y_off).round() as i32;
                let start = line_starts.get(run.line_i).copied().unwrap_or(0);
                for glyph in run.glyphs.iter() {
                    let byte = start + glyph.start;
                    let Some(anim) = job.runs.get(glyph.metadata).and_then(|r| r.style.anim) else { continue };
                    let physical = glyph.physical((x_off, 0.0), 1.0);
                    let color = glyph.color_opt.unwrap_or(base);
                    let opacity = job.runs[glyph.metadata].style.opacity();
                    let Some((pic, bitmap)) = self.picture(physical.cache_key, color, opacity, job) else { continue };
                    let pic_at = letters.pics.len();
                    letters.pics.push(pic);
                    bitmaps.push(bitmap);
                    let mut letter_symbols = 0..0;
                    if anim == Anim::Glitch {
                        // a run's symbols are drawn once, in the first of its letters' style
                        letter_symbols = match symbols.get(&glyph.metadata) {
                            Some(r) => r.clone(),
                            None => {
                                let drawn = self.glitch_pictures(glyph, color, opacity, job);
                                let r = letters.pics.len()..letters.pics.len() + drawn.len();
                                for (pic, bitmap) in drawn {
                                    letters.pics.push(pic);
                                    bitmaps.push(bitmap);
                                }
                                symbols.insert(glyph.metadata, r.clone());
                                r
                            }
                        };
                    }
                    letters.letters.push(Letter {
                        index: job.text[..byte].chars().count(),
                        anim,
                        amount: job.runs[glyph.metadata].style.anim_value,
                        width: glyph.w.round() as i32,
                        x: physical.x,
                        y: line_y + physical.y,
                        pic: pic_at,
                        fit: letters.pics[letter_symbols.clone()].iter().filter(|p| p.w as i32 <= glyph.w.round() as i32).count().max(1),
                        symbols: letter_symbols,
                    });
                }
            }
            LetterSet { letters: Rc::new(letters), bitmaps }
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

            let [r, g, b, a] = job.color.map(byte);
            let base = Color::rgba(r, g, b, 255);
            // Letters that move are not in the line: they are pictures of their own (see `letters`).
            let moving = |glyph: &cosmic_text::LayoutGlyph| job.runs.get(glyph.metadata).is_some_and(|run| run.style.anim.is_some());
            // How opaque the run a glyph is in is (`{whisper}`, a colour's alpha); `shape` put the run's number in it.
            let opacity_of = |glyph: &cosmic_text::LayoutGlyph| job.runs.get(glyph.metadata).map_or(1.0, |run| run.style.opacity());

            let line_count = buffer.layout_runs().count();
            let y_off = block_top(buffer, job.align, height);

            if line_count == 0 && !job.text.trim().is_empty() {
                eprintln!(
                    "warning: text produced no layout lines ({:?}); fonts loaded: {}",
                    job.text,
                    self.fonts.db().len()
                );
            }

            // Premultiplied colour and coverage per pixel (coloured runs differ).
            let size = (width as i32, height as i32);
            let mut acc = std::mem::take(&mut self.acc);
            acc.clear();
            acc.resize((width * height) as usize, [0.0; 4]);
            for run in buffer.layout_runs() {
                let x_off = line_x(job.align, width as f32, run.line_w);
                let line_y = (run.line_y + y_off).round() as i32;
                let start = line_starts.get(run.line_i).copied().unwrap_or(0);
                for glyph in run.glyphs.iter() {
                    if start + glyph.start >= byte_limit || moving(glyph) {
                        continue;
                    }
                    let physical = glyph.physical((x_off, 0.0), 1.0);
                    let glyph_color = glyph.color_opt.unwrap_or(base);
                    let opacity = opacity_of(glyph);
                    self.swash.with_pixels(&mut self.fonts, physical.cache_key, glyph_color, |x, y, color| {
                        plot(&mut acc, size, (physical.x + x, line_y + physical.y + y), faded(color, opacity));
                    });
                }
                // Underlines and strikethroughs run along the typed part of each stretch, in the stretch's colour.
                for span in run.decorations.iter() {
                    let td = &span.data.text_decoration;
                    let typed = run.glyphs[span.glyph_range.clone()].iter().filter(|g| start + g.start < byte_limit);
                    let (left, right) = typed.fold((f32::MAX, f32::MIN), |(l, r), g| (l.min(g.x), r.max(g.x + g.w)));
                    if left >= right {
                        continue;
                    }
                    let opacity = run.glyphs.get(span.glyph_range.start).map_or(1.0, opacity_of);
                    let lines = [(td.underline != UnderlineStyle::None, span.data.underline_metrics), (td.strikethrough, span.data.strikethrough_metrics)];
                    for m in lines.iter().filter(|(on, _)| *on).map(|(_, m)| m) {
                        let thickness = (m.thickness * span.font_size).max(1.0).ceil() as i32;
                        let (x0, y0) = ((left + x_off) as i32, (line_y as f32 - m.offset * span.font_size) as i32);
                        let color = faded(span.color_opt.unwrap_or(base), opacity);
                        for y in y0..y0 + thickness {
                            for x in x0..(right + x_off) as i32 {
                                plot(&mut acc, size, (x, y), color);
                            }
                        }
                    }
                }
            }

            // Ruby: each reading is centred over its text on the first line that holds any of it, and
            // appears with the first typed character of that text.
            for tr in job.runs.iter().filter(|r| r.ruby.is_some()) {
                let found = buffer.layout_runs().find_map(|run| {
                    let start = line_starts.get(run.line_i).copied().unwrap_or(0);
                    let base: Vec<_> = run.glyphs.iter().filter(|g| (tr.start..tr.end).contains(&(start + g.start))).collect();
                    let first = base.iter().map(|g| start + g.start).min()?;
                    let (left, right) = base.iter().fold((f32::MAX, f32::MIN), |(l, r), g| (l.min(g.x), r.max(g.x + g.w)));
                    let x_off = line_x(job.align, width as f32, run.line_w);
                    let px = base[0].font_size;
                    Some((first, x_off + (left + right) / 2.0, (run.line_y + y_off - RUBY_LIFT * px).round() as i32, px * RUBY_SCALE))
                });
                if let (Some((first, centre, baseline, px)), Some(reading)) = (found, &tr.ruby) {
                    let [r, g, b] = tr.style.color.map_or([r, g, b], |c| c.map(byte));
                    if first < byte_limit {
                        self.draw_ruby(&mut acc, size, reading, Color::rgba(r, g, b, 255), (centre, baseline), px);
                    }
                }
            }

            if let Some(outline) = &job.outline {
                add_outline(&mut acc, width as usize, height as usize, outline);
            }

            // Back to straight alpha; `a` scales the whole text's opacity.
            let mut rgba = std::mem::take(&mut self.spare);
            to_rgba(&acc, a as f32 / 255.0, &mut rgba);
            self.acc = acc;
            Bitmap { width, height, rgba }
        }
    }
}

#[cfg(all(test, feature = "cosmic"))]
mod tests {
    use super::real::outline_ring;
    use super::*;
    use quillstone_render::{build_frame, Align, Draw, Rect, Size};
    use quillstone_script::markup::rich;
    use std::rc::Rc;

    #[test]
    fn a_box_sized_to_its_measured_lines_keeps_its_text_whole_at_every_window_size() {
        let m = Measurer::new(Vec::new());
        let text = "The quick brown fox jumps over the lazy dog while the committee deliberates at length. ".repeat(6);
        let (width, size) = (1400.0, 38.0);
        let Some((lines, _)) = m.measure(&text, size, width) else { return };
        let height = lines as f32 * quillstone_render::line_height(size);
        let at = |w: f32, h: f32| {
            let draw = Draw::Text { text: text.clone(), runs: Vec::new(), visible_chars: usize::MAX, rect: Rect::new(0.0, 0.0, width, height), size, color: [1.0; 4], align: Align::Left, outline: None };
            m.fit(&build_frame(&[draw], Size::new(w, h)).texts.remove(0))
        };
        for (w, h) in [(1920.0, 1080.0), (1280.0, 720.0), (960.0, 540.0), (800.0, 450.0), (1366.0, 768.0)] {
            let Some(fit) = at(w, h) else { return };
            assert!(fit.scale == 1.0 && !fit.cut, "{w}x{h}: {fit:?}");
        }
        // one line short is still too tall at any size
        let draw = Draw::Text { text: text.clone(), runs: Vec::new(), visible_chars: usize::MAX, rect: Rect::new(0.0, 0.0, width, height - 2.0 * quillstone_render::line_height(size)), size, color: [1.0; 4], align: Align::Left, outline: None };
        assert!(m.fit(&build_frame(&[draw], Size::new(960.0, 540.0)).texts.remove(0)).unwrap().scale < 1.0);
    }

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

    /// A renderer with DejaVu Sans, the font the pixel tolerances of some tests are tuned to
    /// (another system's default font draws other edges); None where it isn't installed.
    fn dejavu() -> Option<TextRenderer> {
        use cosmic_text::fontdb::{Database, Family, Query};
        let mut db = Database::new();
        db.load_system_fonts();
        let id = db.query(&Query { families: &[Family::Name("DejaVu Sans")], ..Query::default() })?;
        Some(TextRenderer::new(vec![db.with_face_data(id, |data, _| data.to_vec())?]))
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

    /// Row of the highest lit pixel.
    fn top(bitmap: &Bitmap) -> usize {
        let rows = bitmap.rgba.chunks(bitmap.width as usize * 4);
        rows.enumerate().find(|(_, row)| row.chunks(4).any(|p| p[3] > 0)).map_or(usize::MAX, |(y, _)| y)
    }

    fn lit(b: &Bitmap) -> usize {
        b.rgba.chunks(4).filter(|p| p[3] > 0).count()
    }

    #[test]
    fn an_underline_is_a_line_under_the_typed_part_of_its_text_only() {
        let mut renderer = TextRenderer::new(Vec::new());
        if bottom(&renderer.rasterize(&job("Hello there")).unwrap()) == 0 {
            eprintln!("SKIPPED (no fonts on this machine): underline test did not run");
            return;
        }
        let plain = renderer.rasterize(&job("Hello there")).unwrap();
        let under = renderer.rasterize(&job("Hello {u}there{/u}")).unwrap();
        assert_ne!(plain.rgba, under.rgba);
        assert!(lit(&under) > lit(&plain) + 100, "a line about the width of the word: {} vs {}", lit(&under), lit(&plain));
        // the longest unbroken stretch of lit pixels in one row: letters leave gaps, a line does not
        let run = |b: &Bitmap| {
            let row_runs = b.rgba.chunks(b.width as usize * 4).map(|row| {
                row.chunks(4).fold((0, 0), |(best, now), p| if p[3] > 0 { (best.max(now + 1), now + 1) } else { (best, 0) }).0
            });
            row_runs.max().unwrap_or(0)
        };
        assert!(run(&plain) < 40 && run(&under) > 80, "the underline is an unbroken row: {} vs {}", run(&under), run(&plain));
        // not typed yet: no line (the pixels are those of the same text without the tag)
        let before = renderer.rasterize(&typed("Hello {u}there{/u}", 6)).unwrap();
        assert_eq!(before.rgba, renderer.rasterize(&typed("Hello there", 6)).unwrap().rgba);
        let half = renderer.rasterize(&typed("Hello {u}there{/u}", 8)).unwrap();
        assert!(run(&half) > run(&before) && run(&half) < run(&under), "two letters typed: a short line");
        // the underline takes the stretch's colour
        let red = renderer.rasterize(&job("{color=#ff0000}{u}____{/u}{/color}")).unwrap();
        assert!(red.rgba.chunks(4).filter(|p| p[3] > 0).all(|p| p[..3] == [255, 0, 0]));
    }

    /// The longest unbroken stretch of lit pixels in one row: letters leave gaps, a line does not.
    fn longest_run(b: &Bitmap) -> usize {
        let row_runs = b.rgba.chunks(b.width as usize * 4).map(|row| {
            row.chunks(4).fold((0, 0), |(best, now), p| if p[3] > 0 { (best.max(now + 1), now + 1) } else { (best, 0) }).0
        });
        row_runs.max().unwrap_or(0)
    }

    #[test]
    fn a_strikethrough_is_a_line_through_the_typed_part_in_the_text_colour() {
        let Some(mut renderer) = dejavu() else {
            eprintln!("SKIPPED (no DejaVu Sans on this machine): strikethrough test did not run");
            return;
        };
        let plain = renderer.rasterize(&job("Hello there")).unwrap();
        let struck = renderer.rasterize(&job("Hello {s}there{/s}")).unwrap();
        let under = renderer.rasterize(&job("Hello {u}there{/u}")).unwrap();
        assert!(lit(&struck) > lit(&plain) + 100 && longest_run(&struck) > 80 && longest_run(&plain) < 40, "{} {}", longest_run(&struck), longest_run(&plain));
        // through the middle of the letters: between the top and the bottom of the text, unlike an underline
        let line_row = |b: &Bitmap| {
            let rows = b.rgba.chunks(b.width as usize * 4);
            rows.enumerate().find(|(_, row)| row.chunks(4).fold((0, 0), |(best, now), p| if p[3] > 0 { (best.max(now + 1), now + 1) } else { (best, 0) }).0 > 80).map(|(y, _)| y)
        };
        let (middle, below) = (line_row(&struck).unwrap(), line_row(&under).unwrap());
        assert!(middle + 5 < below && middle > top(&plain) && middle < bottom(&plain), "{middle} {below}");
        // only the typed part, like the underline
        let before = renderer.rasterize(&typed("Hello {s}there{/s}", 6)).unwrap();
        assert_eq!(before.rgba, renderer.rasterize(&typed("Hello there", 6)).unwrap().rgba);
        let half = renderer.rasterize(&typed("Hello {s}there{/s}", 8)).unwrap();
        assert!(longest_run(&half) > longest_run(&before) && longest_run(&half) < longest_run(&struck));
        // in the stretch's colour, and it mixes with the underline
        let red = renderer.rasterize(&job("{color=#ff0000}{s}____{/s}{/color}")).unwrap();
        assert!(red.rgba.chunks(4).filter(|p| p[3] > 0).all(|p| p[..3] == [255, 0, 0]));
        let both = renderer.rasterize(&job("Hello {u}{s}there{/s}{/u}")).unwrap();
        assert!(lit(&both) > lit(&struck) && lit(&both) > lit(&under));
    }

    #[test]
    fn a_colour_with_alpha_is_see_through_and_a_whisper_is_smaller_and_half_so() {
        let mut renderer = TextRenderer::new(Vec::new());
        if bottom(&renderer.rasterize(&job("Hello there")).unwrap()) == 0 {
            eprintln!("SKIPPED (no fonts on this machine): alpha test did not run");
            return;
        }
        // the most opaque pixel of the picture, and the colour there
        let solid = |b: &Bitmap| b.rgba.chunks(4).max_by_key(|p| p[3]).map(|p| [p[0], p[1], p[2], p[3]]).unwrap();
        assert_eq!(solid(&renderer.rasterize(&job("{color=#ff0000}HHHH{/color}")).unwrap()), [255, 0, 0, 255]);
        for (tag, want) in [("{color=#ff000080}", 128), ("{color=#f008}", 136)] {
            let a = renderer.rasterize(&job(&format!("{tag}HHHH{{/color}}"))).unwrap();
            let [r, g, b, alpha] = solid(&a);
            assert!(r == 255 && g == 0 && b == 0 && alpha.abs_diff(want) <= 1, "{tag}: {alpha}");
        }
        // an alpha colour inside the text's own colour (white): white at that opacity
        let part = renderer.rasterize(&job("{color=#ffffff80}HHHH{/color}")).unwrap();
        assert_eq!(solid(&part)[..3], [255, 255, 255]);
        // the line under a see-through strike is see-through too
        let struck = renderer.rasterize(&job("{color=#00ff0080}{s}    {/s}{/color}")).unwrap();
        assert!(struck.rgba.chunks(4).filter(|p| p[3] > 0).all(|p| p[3] <= 129 && p[..3] == [0, 255, 0]));
        // the text colour's own alpha still scales it all
        let mut dim = job("{color=#ff000080}HHHH{/color}");
        dim.color[3] = 0.5;
        assert!(solid(&renderer.rasterize(&dim).unwrap())[3].abs_diff(64) <= 1);

        let normal = renderer.rasterize(&job("HHHH")).unwrap();
        let whisper = renderer.rasterize(&job("{whisper}HHHH{/whisper}")).unwrap();
        let [_, _, _, alpha] = solid(&whisper);
        assert!(alpha.abs_diff((0.55f32 * 255.0).round() as u8) <= 1, "{alpha}");
        assert_eq!(solid(&whisper)[..3], [255, 255, 255], "the text's own colour");
        // about 75% as tall and wide
        let (h0, h1) = (bottom(&normal) - top(&normal), bottom(&whisper) - top(&whisper));
        assert!((h1 as f32 / h0 as f32 - 0.75).abs() < 0.08, "{h1} / {h0}");
        // it nests with colour and size
        let red = renderer.rasterize(&job("{color=#ff0000}{whisper}HHHH{/whisper}{/color}")).unwrap();
        assert_eq!(solid(&red)[..3], [255, 0, 0]);
        let big = renderer.rasterize(&job("{size=+40}{whisper}HHHH{/whisper}{/size}")).unwrap();
        let sized = renderer.rasterize(&job("{size=+40}HHHH{/size}")).unwrap();
        let ratio = (bottom(&big) - top(&big)) as f32 / (bottom(&sized) - top(&sized)) as f32;
        assert!((ratio - 0.75).abs() < 0.08, "{ratio}");
        let both = renderer.rasterize(&job("{color=#ff000080}{whisper}HHHH{/whisper}{/color}")).unwrap();
        assert!(solid(&both)[3].abs_diff((0.55f32 * 128.0).round() as u8) <= 1, "alpha and whisper multiply");
    }

    /// Lay a letter picture over `line` at `x`, `y` (source-over, straight alpha).
    fn lay(line: &mut Bitmap, pic: &Bitmap, (x, y): (i32, i32)) {
        for py in 0..pic.height as i32 {
            for px in 0..pic.width as i32 {
                let (tx, ty) = (x + px, y + py);
                if tx < 0 || ty < 0 || tx >= line.width as i32 || ty >= line.height as i32 {
                    continue;
                }
                let s = &pic.rgba[((py as u32 * pic.width + px as u32) * 4) as usize..][..4];
                let d = ((ty as u32 * line.width + tx as u32) * 4) as usize;
                let (sa, da) = (s[3] as f32 / 255.0, line.rgba[d + 3] as f32 / 255.0);
                let out = sa + da * (1.0 - sa);
                if out == 0.0 {
                    continue;
                }
                for (k, c) in s[..3].iter().enumerate() {
                    line.rgba[d + k] = ((*c as f32 * sa + line.rgba[d + k] as f32 * da * (1.0 - sa)) / out).round() as u8;
                }
                line.rgba[d + 3] = (out * 255.0).round() as u8;
            }
        }
    }

    fn animated_job(tagged: &str, n: usize, outline: bool) -> TextJob {
        let mut j = typed(tagged, n);
        if outline {
            j.outline = Some(quillstone_render::Outline { size: 2.0, color: [0.0, 0.0, 1.0, 1.0] });
        }
        j
    }

    #[test]
    fn moving_letters_leave_gaps_in_the_line_and_are_pictures_that_put_it_back() {
        let Some(mut renderer) = dejavu() else {
            eprintln!("SKIPPED (no DejaVu Sans on this machine): moving letters test did not run");
            return;
        };
        for (tag, outline) in [("shake", false), ("wave", false), ("glitch", false), ("shake", true)] {
            let tagged = format!("Say {{{tag}}}boo{{/{tag}}} now");
            let moving = animated_job(&tagged, usize::MAX, outline);
            let plain = animated_job("Say boo now", usize::MAX, outline);
            let line = renderer.rasterize(&moving).unwrap();
            let whole = renderer.rasterize(&plain).unwrap();
            assert!(lit(&line) < lit(&whole) - 100, "{tag}: the three letters are left out ({} vs {})", lit(&line), lit(&whole));
            let set = renderer.letters(&moving).expect("it has moving letters");
            let letters = &set.letters.letters;
            assert_eq!(letters.iter().map(|l| l.index).collect::<Vec<_>>(), [4, 5, 6], "{tag}");
            assert_eq!(set.bitmaps.len(), set.letters.pics.len());
            assert!(letters.iter().all(|l| (tag == "glitch") == !l.symbols.is_empty()));
            for l in letters.iter().filter(|l| !l.symbols.is_empty()) {
                let fitting = &set.letters.pics[l.symbols.start..l.symbols.start + l.fit];
                assert!(l.fit == 1 || fitting.iter().all(|p| p.w as i32 <= l.width), "only symbols that fit the letter");
                assert!(set.letters.pics[l.symbols.clone()].windows(2).all(|w| w[0].w <= w[1].w), "narrowest first");
            }
            assert_eq!(set.letters.pics.len(), if tag == "glitch" { 3 + quillstone_render::GLITCH_SYMBOLS } else { 3 });
            // each letter at rest, drawn at its pen position and picture offset, completes the line
            let mut put = Bitmap { width: line.width, height: line.height, rgba: line.rgba.clone() };
            for l in letters {
                let p = &set.letters.pics[l.pic];
                assert_eq!((set.bitmaps[l.pic].width, set.bitmaps[l.pic].height), (p.w, p.h));
                lay(&mut put, &set.bitmaps[l.pic], (l.x + p.dx, l.y + p.dy));
            }
            // an outline's ring is drawn under the letters of the whole line; as pictures a letter's ring may sit over
            // its neighbour's edge, so a few edge pixels differ a little there
            let off = put.rgba.chunks(4).zip(whole.rgba.chunks(4)).filter(|(a, b)| a.iter().zip(*b).any(|(x, y)| x.abs_diff(*y) > 3)).count();
            let worst = put.rgba.iter().zip(&whole.rgba).map(|(a, b)| a.abs_diff(*b)).max().unwrap();
            assert!(if outline { off * 200 < lit(&whole) && worst <= 16 } else { worst <= 3 }, "{tag} outline {outline}: the letters sit where they were ({off} pixels off, by up to {worst})");
        }
        // the outline is part of the letter pictures: they are bigger
        let (flat, ringed) = (renderer.letters(&animated_job("a{shake}b{/shake}", 9, false)).unwrap(), renderer.letters(&animated_job("a{shake}b{/shake}", 9, true)).unwrap());
        assert!(ringed.letters.pics[0].w >= flat.letters.pics[0].w + 4);
        assert_ne!(animated_job("a{shake}b{/shake}", 9, false).letters_key(), animated_job("a{shake}b{/shake}", 9, true).letters_key());
    }

    #[test]
    fn moving_letters_appear_with_the_typing_and_a_line_without_any_asks_for_none() {
        let mut renderer = TextRenderer::new(Vec::new());
        if bottom(&renderer.rasterize(&job("Hello there")).unwrap()) == 0 {
            eprintln!("SKIPPED (no fonts on this machine): moving letters typing test did not run");
            return;
        }
        let tagged = "Say {wave}boo{/wave}";
        // the line only holds what is typed; the pictures are the same however much is
        let a = renderer.letters(&animated_job(tagged, 5, false)).unwrap();
        let b = renderer.letters(&animated_job(tagged, usize::MAX, false)).unwrap();
        assert!(Rc::ptr_eq(&a, &b), "one set for the whole line");
        assert_eq!(animated_job(tagged, 5, false).letters_key(), animated_job(tagged, 9, false).letters_key());
        assert_eq!(lit(&renderer.rasterize(&animated_job(tagged, 5, false)).unwrap()), lit(&renderer.rasterize(&animated_job("Say b", 5, false)).unwrap()) - lit_of_letter(&mut renderer, "Say b", 4));
        assert!(renderer.letters(&animated_job("Say boo", 9, false)).is_none());
        assert!(renderer.letters(&animated_job("Say {u}boo{/u}", 9, false)).is_none());
    }

    /// The pixels the letter at `index` of `tagged` lights in the line.
    fn lit_of_letter(renderer: &mut TextRenderer, tagged: &str, index: usize) -> usize {
        let with = lit(&renderer.rasterize(&typed(tagged, index + 1)).unwrap());
        let without = lit(&renderer.rasterize(&typed(tagged, index)).unwrap());
        with - without
    }

    #[test]
    fn with_animation_off_the_text_is_drawn_whole_and_still() {
        let mut renderer = TextRenderer::new(Vec::new());
        if bottom(&renderer.rasterize(&job("Hello there")).unwrap()) == 0 {
            eprintln!("SKIPPED (no fonts on this machine): still text test did not run");
            return;
        }
        let r = rich("Say {shake}boo{/shake} {glitch}now{/glitch}");
        let draw = Draw::Text { text: r.text, runs: r.runs, visible_chars: usize::MAX, rect: Rect::new(0.0, 0.0, 1200.0, 400.0), size: 40.0, color: [1.0; 4], align: Align::Left, outline: None };
        let size = Size::new(1920.0, 1080.0);
        let on = quillstone_render::build_frame_with(std::slice::from_ref(&draw), size, true);
        let off = quillstone_render::build_frame_with(std::slice::from_ref(&draw), size, false);
        assert!(!off.animated() && renderer.letters(&off.texts[0]).is_none());
        let whole = renderer.rasterize(&typed("Say boo now", usize::MAX)).unwrap();
        assert_eq!(renderer.rasterize(&off.texts[0]).unwrap().rgba, whole.rgba, "just plain text");
        assert!(lit(&renderer.rasterize(&on.texts[0]).unwrap()) < lit(&whole) - 100, "the same text with animation has gaps");
        // and no quads for letters
        let mut f = off;
        f.add_letters(size, 1.0, &mut |j, _| renderer.letters(j).map(|s| s.letters.clone()));
        assert!(!f.quads.iter().any(|q| matches!(q.texture, quillstone_render::Tex::Letter(..))));
        let mut f = on;
        f.add_letters(size, 1.0, &mut |j, _| renderer.letters(j).map(|s| s.letters.clone()));
        assert_eq!(f.quads.iter().filter(|q| matches!(q.texture, quillstone_render::Tex::Letter(..))).count(), 6);
        // the letters of a shake are in other places at other times
        let mut at = |t: f64| {
            let mut f = quillstone_render::build_frame(std::slice::from_ref(&draw), size);
            f.add_letters(size, t, &mut |j, _| renderer.letters(j).map(|s| s.letters.clone()));
            f.quads.iter().filter(|q| matches!(q.texture, quillstone_render::Tex::Letter(..))).map(|q| q.rect).collect::<Vec<_>>()
        };
        let (first, again) = (at(0.5), at(0.5));
        assert_eq!(first, again);
        assert_ne!(at(0.0), at(0.3));
    }

    #[test]
    fn ruby_sits_above_its_text_makes_only_its_own_line_taller_and_types_with_its_base() {
        let mut renderer = TextRenderer::new(Vec::new());
        if bottom(&renderer.rasterize(&job("Hello there")).unwrap()) == 0 {
            eprintln!("SKIPPED (no fonts on this machine): ruby test did not run");
            return;
        }
        // The same layout with a reading that draws nothing (a zero-width space), to tell the reading's pixels apart.
        let (real, blank) = ("ab {ruby=xy}cd{/ruby}", "ab {ruby=\u{200B}}cd{/ruby}");
        let with = renderer.rasterize(&job(real)).unwrap();
        let without = renderer.rasterize(&job(blank)).unwrap();
        assert_eq!(bottom(&with), bottom(&without), "the same layout");
        assert!(top(&with) + 10 < top(&without), "the reading is above everything else: {} vs {}", top(&with), top(&without));
        assert!(lit(&with) > lit(&without));
        let left_edge = |b: &Bitmap| (0..b.width as usize).find(|&x| (0..b.height as usize).any(|y| b.rgba[(y * b.width as usize + x) * 4 + 3] > 0));
        assert_eq!(left_edge(&with), left_edge(&without), "the reading is within the text, over `cd`");
        // a line without ruby keeps its normal height
        let no_ruby = renderer.rasterize(&job("ab cd{p}ef")).unwrap();
        let ruby_second = renderer.rasterize(&job("ab cd{p}{ruby=xy}ef{/ruby}")).unwrap();
        assert!(bottom(&ruby_second) > bottom(&no_ruby), "a line with ruby is taller");
        let ruby_first = renderer.rasterize(&job("{ruby=xy}ab{/ruby} cd{p}ef")).unwrap();
        assert!(bottom(&ruby_first) > bottom(&no_ruby));
        // typing: the reading shows up with the first character of its text, not before
        let mut at = |tagged: &str, n| renderer.rasterize(&typed(tagged, n)).unwrap().rgba;
        assert_eq!(at(real, 3), at(blank, 3), "only `ab ` is typed");
        assert_ne!(at(real, 4), at(blank, 4), "`c` is typed");
    }

    #[test]
    fn a_reading_wider_than_its_text_at_the_right_edge_stays_inside() {
        let mut renderer = TextRenderer::new(Vec::new());
        if bottom(&renderer.rasterize(&job("Hello there")).unwrap()) == 0 {
            eprintln!("SKIPPED (no fonts on this machine): ruby edge test did not run");
            return;
        }
        // the reading's own pixels: the same line with a reading that draws nothing, subtracted
        let mut reading = |tagged: &str, align| {
            let mut ink = |tagged: &str| {
                let r = rich(tagged);
                let draw = Draw::Text { text: r.text, runs: r.runs, visible_chars: usize::MAX, rect: Rect::new(0.0, 0.0, 1200.0, 400.0), size: 40.0, color: [1.0; 4], align, outline: None };
                lit(&renderer.rasterize(&build_frame(&[draw], Size::new(1920.0, 1080.0)).texts.remove(0)).unwrap())
            };
            ink(tagged) - ink(&tagged.replace("xyxyxyxyxy", "\u{200B}"))
        };
        let inside = reading("ab {ruby=xyxyxyxyxy}i{/ruby} cd", Align::Left);
        let at_edge = reading("{ruby=xyxyxyxyxy}i{/ruby}", Align::Right);
        assert!(at_edge * 100 >= inside * 95, "the whole reading is drawn: {at_edge} vs {inside}");
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
