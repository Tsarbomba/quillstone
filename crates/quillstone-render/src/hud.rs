//! The HUD layer: always-on overlay elements (`::hud`). Their definitions come from
//! the runtime ([`Cmd::HudShow`] and friends); what they show (text with `{name}` values,
//! bar fractions, conditions) is worked out each time the draw list is built, from the
//! variables the game last handed over with [`HudLayer::set_vars`].

use quillstone_runtime::{interpolate, Cmd};
use quillstone_script::markup::rich;
use quillstone_script::{HudElem, HudKind, Vars};

use crate::fade::Fade;
use crate::layout::{is_wide, Rect, Size, VIRTUAL};
use crate::theme::Theme;
use crate::view::{faded, Align, Draw};

/// How buttons look this frame: where the mouse is, and whether they work now (else they are dimmed).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HudLook {
    pub pointer: Option<(f32, f32)>,
    pub active: bool,
}

impl Default for HudLook {
    fn default() -> Self {
        HudLook { pointer: None, active: true }
    }
}

/// How faint a button is while it can't be pressed (as a greyed cell in the inventory).
const INERT_ALPHA: f32 = 0.45;

/// One element on screen, with its fade.
#[derive(Debug, Clone, PartialEq)]
struct Item {
    elem: HudElem,
    /// Target 1 while shown or fading in, 0 while fading out (it is dropped at 0).
    fade: Fade,
}

#[derive(Debug, Clone, Default)]
pub struct HudLayer {
    items: Vec<Item>,
    vars: Vars,
}

impl HudLayer {
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// The ids of the elements, in drawing order (including ones that are fading out).
    pub fn ids(&self) -> Vec<&str> {
        self.items.iter().map(|i| i.elem.id.as_str()).collect()
    }

    pub fn set_vars(&mut self, vars: &Vars) {
        if self.vars != *vars {
            self.vars = vars.clone();
        }
    }

    pub fn clear_all(&mut self) {
        self.items.clear();
    }

    /// Whether a fade is running (so the frontend keeps drawing).
    pub fn fading(&self) -> bool {
        self.items.iter().any(|i| !i.fade.done())
    }

    /// Finish every fade at once (skipping doesn't wait for them).
    pub fn end_fades(&mut self) {
        self.items.retain(|i| i.fade.target > 0.0);
        for i in &mut self.items {
            i.fade.alpha = 1.0;
        }
    }

    pub fn apply(&mut self, cmd: &Cmd) {
        match cmd {
            Cmd::HudShow { elem, fade } => {
                let rate = Fade::rate_over(*fade);
                match self.items.iter_mut().find(|i| i.elem.id == elem.id) {
                    Some(item) => {
                        let was_hiding = item.fade.target == 0.0;
                        item.elem = (**elem).clone();
                        item.fade.target = 1.0;
                        item.fade.rate = rate;
                        // Replacing a shown element is seamless unless it is asked to fade in.
                        if rate > 0.0 && (was_hiding || *fade > 0.0) {
                            item.fade.alpha = if was_hiding { item.fade.alpha } else { 0.0 };
                        } else {
                            item.fade.alpha = 1.0;
                        }
                    }
                    None => self.items.push(Item { elem: (**elem).clone(), fade: Fade { alpha: if rate > 0.0 { 0.0 } else { 1.0 }, target: 1.0, rate } }),
                }
            }
            Cmd::HudHide { id, fade } => {
                if *fade > 0.0 {
                    if let Some(item) = self.items.iter_mut().find(|i| i.elem.id == *id) {
                        item.fade = Fade { target: 0.0, rate: Fade::rate_over(*fade), ..item.fade };
                    }
                } else {
                    self.items.retain(|i| i.elem.id != *id);
                }
            }
            Cmd::HudClear { fade } => {
                if *fade > 0.0 {
                    for item in &mut self.items {
                        item.fade = Fade { target: 0.0, rate: Fade::rate_over(*fade), ..item.fade };
                    }
                } else {
                    self.items.clear();
                }
            }
            _ => {}
        }
    }

    pub fn tick(&mut self, dt: f32) {
        for i in &mut self.items {
            i.fade.tick(dt);
        }
        self.items.retain(|i| i.fade.target > 0.0 || i.fade.alpha > 0.0);
    }

    /// Push the draws of every element that is on (its condition true, not faded out).
    pub fn draw(&self, theme: &Theme, image_size: &dyn Fn(&str) -> Option<Size>, look: HudLook, out: &mut Vec<Draw>) {
        for item in self.items.iter().filter(|i| i.fade.alpha > 0.0) {
            self.draw_item(item, theme, image_size, look, out);
        }
    }

    /// The pictures of the buttons (normal and hover), whose sizes hit-testing needs.
    pub fn button_pictures(&self) -> impl Iterator<Item = &str> {
        let buttons = self.items.iter().filter(|i| i.elem.action.is_some());
        buttons.flat_map(|i| i.elem.image().into_iter().chain(i.elem.hover.as_deref()))
    }

    /// The button under the point, if any: the topmost one, at the rect it is drawn at.
    pub fn button_at(&self, theme: &Theme, image_size: &dyn Fn(&str) -> Option<Size>, x: f32, y: f32) -> Option<&HudElem> {
        let look = HudLook::default();
        let mut live = self.items.iter().rev().filter(|i| i.fade.target > 0.0 && i.fade.alpha > 0.0 && i.elem.action.is_some());
        live.find(|i| self.draw_item(i, theme, image_size, look, &mut Vec::new()).is_some_and(|r| r.contains(x, y))).map(|i| &i.elem)
    }

    /// Push one element's draws and say where it is (none when it is off or its picture is unknown).
    fn draw_item(&self, item: &Item, theme: &Theme, image_size: &dyn Fn(&str) -> Option<Size>, look: HudLook, out: &mut Vec<Draw>) -> Option<Rect> {
        let e = &item.elem;
        if e.cond.as_ref().is_some_and(|c| !c.expr.test(&self.vars)) {
            return None;
        }
        let button = e.action.is_some();
        let alpha = if button && !look.active { item.fade.alpha * INERT_ALPHA } else { item.fade.alpha };
        let under_mouse = |r: Rect| button && look.active && look.pointer.is_some_and(|(x, y)| r.contains(x, y));
        match &e.kind {
            HudKind::Text { text, size, color, back, pad } => {
                let shown = interpolate(text, &self.vars);
                let size = size.unwrap_or(theme.text_size);
                let color = faded(color.unwrap_or(theme.text_color), alpha);
                let back = back.map(|b| faded(b, alpha));
                let r = draw_text(&mut Vec::new(), &shown, (e.x, e.y), 0.0, size, color, back, *pad);
                // A text button under the mouse gets the theme's hover colour as its panel.
                let back = if under_mouse(r) { Some(theme.button_hover_color) } else { back };
                Some(draw_text(out, &shown, (e.x, e.y), 0.0, size, color, back, *pad))
            }
            HudKind::Bar { value, max, size: [w, h], color, back, border } => {
                let int = |x: &quillstone_script::HudExpr| x.expr.eval(&self.vars).as_int().unwrap_or(0);
                let (v, m) = (int(value), int(max));
                let frac = if m <= 0 { 0.0 } else { (v as f64 / m as f64).clamp(0.0, 1.0) as f32 };
                let r = Rect::new(placed(e.x, *w, VIRTUAL.w), placed(e.y, *h, VIRTUAL.h), *w, *h);
                if let Some((c, t)) = border {
                    let c = faded(*c, item.fade.alpha);
                    for bar in [
                        Rect::new(r.x - t, r.y - t, r.w + 2.0 * t, *t),
                        Rect::new(r.x - t, r.y + r.h, r.w + 2.0 * t, *t),
                        Rect::new(r.x - t, r.y, *t, r.h),
                        Rect::new(r.x + r.w, r.y, *t, r.h),
                    ] {
                        out.push(Draw::Rect { rect: bar, color: c });
                    }
                }
                out.push(Draw::Rect { rect: r, color: faded(*back, item.fade.alpha) });
                if frac > 0.0 {
                    let fill = faded(color.unwrap_or(theme.accent_color), item.fade.alpha);
                    out.push(Draw::Rect { rect: Rect::new(r.x, r.y, r.w * frac, r.h), color: fill });
                }
                Some(r)
            }
            HudKind::Image { src, scale } => {
                let s = image_size(src).filter(|s| s.w > 0.0 && s.h > 0.0)?;
                let (w, h) = (s.w * scale, s.h * scale);
                let dest = Rect::new(placed(e.x, w, VIRTUAL.w), placed(e.y, h, VIRTUAL.h), w, h);
                // The hover picture is asked for every frame (so it is loaded before the first hover) and takes
                // the place of the normal one under the mouse once it is.
                let hover = e.hover.as_ref().filter(|h| image_size(h).is_some());
                let shown = hover.filter(|_| under_mouse(dest)).unwrap_or(src);
                out.push(Draw::Image { src: shown.clone(), dest, uv: Rect::new(0.0, 0.0, 1.0, 1.0), alpha });
                Some(dest)
            }
        }
    }
}

/// The start of something `size` long when the point `f` (0..1) of it is `f` of the way across `room`.
fn placed(f: f32, size: f32, room: f32) -> f32 {
    f * (room - size)
}

/// Line height the player's text engine uses (see `quillstone-render` `TextJob`s: 1.35 x size).
const LINE: f32 = 1.35;

/// A cautious width of the widest line of `text` at `size`: generous, so a panel built
/// from it never makes the text wrap (the real text engine is not visible from here).
/// `runs` are the `(start, end, size scale)` stretches of the text in order (bytes); one
/// pass walks the text and the runs together.
fn widest_line(text: &str, size: f32, runs: &[(usize, usize, f32)]) -> f32 {
    let mut cursor = 0;
    let mut widest = 0.0f32;
    let mut line = 0.0f32;
    for (at, c) in text.char_indices() {
        if c == '\n' {
            widest = widest.max(line * 1.06 + 2.0);
            line = 0.0;
            continue;
        }
        while runs.get(cursor).is_some_and(|r| r.1 <= at) {
            cursor += 1;
        }
        let scale = runs.get(cursor).filter(|r| r.0 <= at).map_or(1.0, |r| r.2);
        line += size * scale * if is_wide(c) {
            1.0
        } else if c.is_whitespace() {
            0.3
        } else if "il.,:;'!|jt".contains(c) {
            0.32
        } else if "mwMW@".contains(c) {
            0.9
        } else if c.is_uppercase() {
            0.7
        } else if c.is_ascii_digit() {
            0.62
        } else {
            0.55
        };
    }
    widest.max(line * 1.06 + 2.0)
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn draw_text(
    out: &mut Vec<Draw>,
    shown: &str,
    (fx, fy): (f32, f32),
    inset: f32,
    size: f32,
    color: [f32; 4],
    back: Option<[f32; 4]>,
    pad: f32,
) -> Rect {
    let r = rich(shown);
    // `{size=..}` runs make characters (and so lines) bigger than the base size.
    let run_size = |run: &quillstone_script::markup::TextRun| run.style.size.map_or(size, |s| s.resolve(size));
    let tallest = r.runs.iter().map(run_size).fold(size, f32::max);
    let scales: Vec<(usize, usize, f32)> = r.runs.iter().map(|run| (run.start, run.end, run_size(run) / size.max(1.0))).collect();
    let newlines = r.text.split('\n').count().max(1);
    // The box is as wide as the text is estimated to be, but never wider than the screen less the insets
    // (a long notice would run off the edge); then the text wraps in it and the height follows the estimate.
    // The text is aligned to the side the point leans to, so at the left, centre and right it is placed
    // exactly, whatever the font.
    let (wanted, room) = (widest_line(&r.text, size, &scales) + 2.0 * pad, VIRTUAL.w - 2.0 * inset);
    let width = wanted.min(room);
    let lines = if wanted > room {
        crate::layout::estimate_lines_styled(&r.text, &r.runs, size, (width - 2.0 * pad).max(1.0)).max(newlines)
    } else {
        newlines
    } as f32;
    let height = lines * tallest * LINE + 2.0 * pad;
    let (left, top) = (inset + placed(fx, width, VIRTUAL.w - 2.0 * inset), inset + placed(fy, height, VIRTUAL.h - 2.0 * inset));
    let align = if fx < 1.0 / 3.0 {
        Align::Left
    } else if fx <= 2.0 / 3.0 {
        Align::CenterX
    } else {
        Align::Right
    };
    let whole = Rect::new(left, top, width, height);
    if let Some(back) = back {
        out.push(Draw::Rect { rect: whole, color: back });
    }
    let rect = Rect::new(left + pad, top + pad, width - 2.0 * pad, height - 2.0 * pad);
    out.push(Draw::Text { text: r.text, runs: r.runs, visible_chars: usize::MAX, rect, size, color, align, outline: None });
    whole
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::view::View;
    use quillstone_runtime::{Input, Session};
    use quillstone_script::{parse, Value};

    fn no_images(_: &str) -> Option<Size> {
        None
    }

    fn view(src: &str) -> View {
        let p = parse(&src.replace("Hi.\n", "")).unwrap(); // no line: no textbox in the way
        
        let (session, cmds) = Session::start(&p);
        let mut v = View::new().with_speed(0.0);
        v.apply(&cmds);
        v.set_vars(&session.state().vars);
        v
    }

    fn vars(pairs: &[(&str, i64)]) -> Vars {
        pairs.iter().map(|(k, n)| (k.to_string(), Value::Int(*n))).collect()
    }

    fn rects(list: &[Draw]) -> Vec<(Rect, [f32; 4])> {
        list.iter().filter_map(|d| if let Draw::Rect { rect, color } = d { Some((*rect, *color)) } else { None }).collect()
    }

    fn texts(list: &[Draw]) -> Vec<&str> {
        list.iter().filter_map(|d| if let Draw::Text { text, .. } = d { Some(text.as_str()) } else { None }).collect()
    }

    const BAR: &str = "---\nvars: { hp: 5, max: 10 }\n---\n::hud{id=b bar=hp max=max x=0.1 y=0.2 size=200,20 color=#ff0000 back=#000000}\nHi.\n";

    fn fill_width(v: &View) -> Option<f32> {
        let list = v.draw_list(&no_images, None);
        rects(&list).into_iter().find(|(_, c)| c[0] == 1.0 && c[1] == 0.0).map(|(r, _)| r.w)
    }

    #[test]
    fn text_shows_the_variables_as_they_are_when_drawn() {
        let mut v = view("---\nvars: { hero.gold: 7, day: 1 }\n---\n::hud{id=c text=\"Day {day}, {hero.gold} gold\"}\nHi.\n");
        assert!(texts(&v.draw_list(&no_images, None)).contains(&"Day 1, 7 gold"));
        v.set_vars(&vars(&[("hero.gold", 9), ("day", 2)]));
        assert!(texts(&v.draw_list(&no_images, None)).contains(&"Day 2, 9 gold"), "no new command was needed");
    }

    #[test]
    fn text_takes_markup_but_a_value_cannot_inject_it() {
        let mut v = view("---\nvars: { who: \"x\" }\n---\n::hud{id=c text=\"**Hi** {who}\"}\nHi.\n");
        let list = v.draw_list(&no_images, None);
        let Some(Draw::Text { text, runs, .. }) = list.iter().find(|d| matches!(d, Draw::Text { text, .. } if text.contains("Hi"))) else { panic!() };
        assert_eq!(text, "Hi x");
        assert!(runs.iter().any(|r| r.style.bold));
        let mut hostile = Vars::new();
        hostile.insert("who".into(), Value::Str("{color=#f00}red{/color} **b**".into()));
        v.set_vars(&hostile);
        let list = v.draw_list(&no_images, None);
        assert!(texts(&list).contains(&"Hi {color=#f00}red{/color} **b**"));
    }

    #[test]
    fn size_and_colour_default_to_the_themes() {
        let v = view("::hud{id=a text=Hi}\n::hud{id=b text=Yo size=50 color=#ff0000}\nHi.\n");
        let list = v.draw_list(&no_images, None);
        let find = |t: &str| list.iter().find_map(|d| match d { Draw::Text { text, size, color, .. } if text == t => Some((*size, *color)), _ => None }).unwrap();
        assert_eq!(find("Hi"), (v.theme().text_size, v.theme().text_color));
        assert_eq!(find("Yo"), (50.0, [1.0, 0.0, 0.0, 1.0]));
    }

    #[test]
    fn a_bar_fills_by_the_fraction_of_its_expressions() {
        let mut v = view(BAR);
        assert_eq!(fill_width(&v), Some(100.0));
        for (hp, max, want) in [(0, 10, None), (10, 10, Some(200.0)), (15, 10, Some(200.0)), (-4, 10, None), (3, 0, None), (3, -5, None), (1, 4, Some(50.0))] {
            v.set_vars(&vars(&[("hp", hp), ("max", max)]));
            assert_eq!(fill_width(&v), want, "{hp}/{max}");
        }
    }

    #[test]
    fn a_bar_is_a_back_rect_then_the_fill_from_the_left_edge() {
        let v = view(BAR);
        let r = rects(&v.draw_list(&no_images, None));
        assert_eq!(r[0].0, Rect::new(172.0, 212.0, 200.0, 20.0));
        assert_eq!(r[1].0, Rect::new(172.0, 212.0, 100.0, 20.0));
        let v = view("---\nvars: { hp: 5 }\n---\n::hud{id=b bar=hp max=10 size=200,20 border=#ffffff border_size=3 x=0 y=0}\nHi.\n");
        let r = rects(&v.draw_list(&no_images, None));
        assert_eq!(r.len(), 6, "four border edges, the back, the fill");
        assert_eq!(r[0].0, Rect::new(-3.0, -3.0, 206.0, 3.0));
        assert_eq!(r[1].0, Rect::new(-3.0, 20.0, 206.0, 3.0));
    }

    #[test]
    fn x_and_y_put_the_point_that_far_across_the_element_that_far_across_the_screen() {
        let bar = |x: &str, y: &str| {
            let v = view(&format!("::hud{{id=b bar=1 max=2 x={x} y={y} size=200,20}}\nHi.\n"));
            rects(&v.draw_list(&no_images, None))[0].0
        };
        assert_eq!((bar("left", "top").x, bar("left", "top").y), (0.0, 0.0));
        assert_eq!(bar("center", "top").x, 860.0);
        assert_eq!(bar("right", "bottom").x, 1720.0);
        assert_eq!(bar("right", "bottom").y, 1060.0);
        assert_eq!(bar("25%", "center").x, 430.0, "a quarter of the way: a quarter of the free room");
        let text = |x: &str| {
            let v = view(&format!("::hud{{id=t text=Hello x={x} y=0.1}}\nHi.\n"));
            v.draw_list(&no_images, None).into_iter().find_map(|d| match d { Draw::Text { text, rect, align, .. } if text == "Hello" => Some((rect, align)), _ => None }).unwrap()
        };
        let (r, a) = text("left");
        assert_eq!((r.x, a), (0.0, Align::Left));
        let (r, a) = text("right");
        assert_eq!((r.x + r.w, a), (1920.0, Align::Right), "flush with the right edge whatever the font");
        let (r, a) = text("center");
        assert_eq!((r.x + r.w / 2.0, a), (960.0, Align::CenterX));
        assert!((r.y - 0.1 * (1080.0 - (r.h))).abs() < 0.01);
    }

    #[test]
    fn a_panel_sits_behind_the_text_with_the_padding_around_it() {
        let v = view("::hud{id=t text=Hello x=0.5 y=0.1 size=40 back=#00000099 pad=12}\nHi.\n");
        let list = v.draw_list(&no_images, None);
        let panel = rects(&list)[0].0;
        let Some(Draw::Text { rect, .. }) = list.iter().find(|d| matches!(d, Draw::Text { text, .. } if text == "Hello")) else { panic!() };
        assert!(matches!(list[list.len() - 2], Draw::Rect { .. }), "the panel is drawn just before its text");
        assert_eq!((rect.x - panel.x, rect.y - panel.y), (12.0, 12.0));
        assert!((panel.x + panel.w / 2.0 - 960.0).abs() < 0.01, "centred on the point");
        assert!((panel.w - rect.w - 24.0).abs() < 0.01 && (panel.h - rect.h - 24.0).abs() < 0.01);
        assert!(panel.w > 40.0 * 2.0, "wide enough for five letters");
        let right = view("::hud{id=t text=Hello x=right y=0.1 size=40 back=#000 pad=12}\nHi.\n");
        let p = rects(&right.draw_list(&no_images, None))[0].0;
        assert!((p.x + p.w - 1920.0).abs() < 0.01, "flush right");
    }

    #[test]
    fn big_runs_make_the_box_taller_and_wider() {
        let find = |src: &str| {
            let v = view(src);
            let list = v.draw_list(&no_images, None);
            let panel = rects(&list).first().map(|(r, _)| *r);
            let text = list.iter().find_map(|d| if let Draw::Text { rect, .. } = d { Some(*rect) } else { None }).unwrap();
            (panel, text)
        };
        let (_, plain) = find("::hud{id=t text=Hello size=40}\n");
        let (_, big) = find("::hud{id=t text=\"{size=100}Hello{/size}\" size=40}\n");
        assert!(plain.h >= 40.0 * 1.35 && big.h >= 100.0 * 1.35, "{plain:?} {big:?}");
        let (p1, _) = find("::hud{id=t text=Hello size=40 back=#000}\n");
        let (p2, _) = find("::hud{id=t text=\"{size=100}Hello{/size}\" size=40 back=#000}\n");
        assert!(p2.unwrap().w > p1.unwrap().w * 2.0);
    }

    #[test]
    fn a_long_notice_at_the_right_wraps_inside_the_screen_instead_of_running_off_the_left_edge() {
        let text = "A very long notice that goes on and on about nothing in particular. ".repeat(2);
        let mut out = Vec::new();
        draw_text(&mut out, &text, (1.0, 0.0), 40.0, 30.0, [1.0; 4], Some([0.0, 0.0, 0.0, 1.0]), 16.0);
        let Draw::Rect { rect, .. } = &out[0] else { panic!("a panel first") };
        assert!(rect.x >= 40.0 && rect.x + rect.w <= VIRTUAL.w - 40.0 + 0.01, "{rect:?}");
        assert!(rect.h > 30.0 * LINE + 32.0 + 1.0, "taller for the wrapped lines: {rect:?}");
    }

    #[test]
    fn a_panel_is_sized_in_one_pass_over_the_text_and_its_runs() {
        // a run's scale counts for exactly its own characters, in any line
        let plain = widest_line("Hello", 30.0, &[]);
        let doubled = widest_line("Hello", 30.0, &[(0, 5, 2.0)]);
        assert!((doubled - 2.0) / (plain - 2.0) > 1.99 && (doubled - 2.0) / (plain - 2.0) < 2.01);
        let half = widest_line("Hello", 30.0, &[(0, 2, 2.0)]);
        assert!(half > plain && half < doubled);
        let two_lines = widest_line("Hi\nHello", 30.0, &[(3, 8, 2.0)]);
        assert!((two_lines - doubled).abs() < 0.01, "the run is found past the first line: {two_lines} {doubled}");
        assert!((widest_line("Hello\nHi", 30.0, &[(0, 5, 2.0)]) - doubled).abs() < 0.01);
        // a long text of many runs, drawn as many elements: linear, not runs x characters
        let text = "*a*".repeat(666);
        let started = std::time::Instant::now();
        let mut out = Vec::new();
        for _ in 0..256 {
            out.clear();
            draw_text(&mut out, &text, (0.0, 0.0), 0.0, 30.0, [1.0; 4], Some([0.0, 0.0, 0.0, 1.0]), 4.0);
        }
        assert!(started.elapsed().as_millis() < 500, "{:?}", started.elapsed());
        assert_eq!(out.len(), 2, "a panel and the text");
    }

    #[test]
    fn an_if_hides_the_element_until_it_is_true() {
        let mut v = view("---\nvars: { hp: 9 }\n---\n::hud{id=w text=LOW if=\"hp <= 3\"}\n::hud{id=x text=ok}\nHi.\n");
        assert_eq!(texts(&v.draw_list(&no_images, None)), ["ok"]);
        v.set_vars(&vars(&[("hp", 2)]));
        assert_eq!(texts(&v.draw_list(&no_images, None)), ["LOW", "ok"]);
        v.set_vars(&vars(&[("hp", 4)]));
        assert_eq!(texts(&v.draw_list(&no_images, None)), ["ok"]);
    }

    #[test]
    fn an_image_is_drawn_at_its_own_size_times_scale_placed_by_x_and_y() {
        let v = view("::hud{id=h image=a.png x=0.5 y=0.5 scale=0.5}\nHi.\n");
        let size = |_: &str| Some(Size { w: 100.0, h: 60.0 });
        let list = v.draw_list(&size, None);
        let Some(Draw::Image { dest, alpha, uv, .. }) = list.first() else { panic!("{list:?}") };
        assert_eq!((*dest, *alpha, *uv), (Rect::new(935.0, 525.0, 50.0, 30.0), 1.0, Rect::new(0.0, 0.0, 1.0, 1.0)));
        assert!(v.draw_list(&no_images, None).is_empty(), "a picture that isn't loaded draws nothing");
    }

    #[test]
    fn elements_are_drawn_in_creation_order_and_a_replaced_one_keeps_its_place() {
        let mut v = view("::hud{id=a text=A}\n::hud{id=b text=B}\nHi.\n");
        assert_eq!(texts(&v.draw_list(&no_images, None)), ["A", "B"]);
        let p = parse("::hud{id=a text=A2}\n").unwrap();
        let (_, cmds) = Session::start(&p);
        v.apply(&cmds);
        assert_eq!(texts(&v.draw_list(&no_images, None)), ["A2", "B"]);
    }

    #[test]
    fn the_hud_is_above_the_stage_and_the_cover_and_under_the_textbox() {
        let src = "::bg{color=#112233}\n::fade{out time=1}\n::hud{id=a text=Title}\n**Mika:** Hello there.\n";
        let p = parse(src).unwrap();
        let (mut session, cmds) = Session::start(&p);
        let mut v = View::new().with_speed(0.0);
        v.apply(&cmds);
        let cmds = session.input(Input::Advance);
        v.apply(&cmds);
        v.set_vars(&session.state().vars);
        let list = v.draw_list(&no_images, None);
        let at = |f: &dyn Fn(&Draw) -> bool| list.iter().position(f);
        let cover = at(&|d| matches!(d, Draw::Rect { color, .. } if color[3] == 1.0 && color[0] == 0.0 && color[2] == 0.0)).unwrap();
        let hud = at(&|d| matches!(d, Draw::Text { text, .. } if text == "Title")).unwrap();
        let line = at(&|d| matches!(d, Draw::Text { text, .. } if text.starts_with("Hello"))).unwrap();
        assert!(cover < hud && hud < line, "{list:?}");
    }

    #[test]
    fn full_screen_overlays_and_the_hidden_ui_do_not_get_the_hud() {
        let mut v = view("::hud{id=a text=Title}\nHi.\n");
        assert_eq!(texts(&v.draw_scene(&no_images)), Vec::<&str>::new(), "the scene under an overlay has no HUD");
        v.set_hide_ui(true);
        assert!(!texts(&v.draw_list(&no_images, None)).contains(&"Title"));
        v.set_hide_ui(false);
        assert!(texts(&v.draw_list(&no_images, None)).contains(&"Title"));
    }

    #[test]
    fn the_textbox_hide_leaves_the_hud_alone() {
        let mut v = view("::hud{id=a text=Title}\n::textbox{hide}\nHi.\n");
        v.apply(&[Cmd::Textbox { hidden: true, fade: 0.0 }]);
        assert!(texts(&v.draw_list(&no_images, None)).contains(&"Title"));
    }

    fn alpha_of(v: &View, text: &str) -> Option<f32> {
        v.draw_list(&no_images, None).iter().find_map(|d| match d {
            Draw::Text { text: t, color, .. } if t == text => Some(color[3]),
            _ => None,
        })
    }

    #[test]
    fn fades_in_out_and_a_clear_with_a_fade() {
        let mut v = view("::hud{id=a text=A fade=1 color=#fff}\nHi.\n");
        assert!(v.busy());
        assert_eq!(alpha_of(&v, "A"), None, "not visible yet");
        v.tick(0.5);
        assert!((alpha_of(&v, "A").unwrap() - 0.5).abs() < 1e-4);
        v.tick(0.6);
        assert_eq!(alpha_of(&v, "A"), Some(1.0));
        assert!(!v.busy());
        v.apply(&[Cmd::HudHide { id: "a".into(), fade: 1.0 }]);
        assert!(v.busy());
        v.tick(0.5);
        assert!((alpha_of(&v, "A").unwrap() - 0.5).abs() < 1e-4);
        v.tick(0.6);
        assert!(v.hud().is_empty() && !v.busy());
        v.apply(&[Cmd::HudClear { fade: 0.0 }]);
        assert!(v.hud().ids().is_empty());
    }

    #[test]
    fn a_redraw_drops_elements_that_are_gone_and_skipping_ends_fades() {
        let mut v = view("::hud{id=a text=A fade=2}\nHi.\n");
        v.apply(&[Cmd::ClearScene]);
        assert!(v.hud().is_empty());
        let mut v = view("::hud{id=a text=A fade=2}\nHi.\n");
        v.end_cover_fade();
        assert_eq!(alpha_of(&v, "A"), Some(1.0));
        assert!(!v.busy());
    }

    #[test]
    fn replacing_a_visible_element_does_not_flicker() {
        let mut v = view("::hud{id=a text=A}\nHi.\n");
        let p = parse("::hud{id=a text=B}\nHi.\n").unwrap();
        v.apply(&Session::start(&p).1);
        assert_eq!(alpha_of(&v, "B"), Some(1.0));
        let p = parse("::hud{id=a text=C fade=1}\nHi.\n").unwrap();
        v.apply(&Session::start(&p).1);
        assert_eq!(alpha_of(&v, "C"), None, "asked to fade in again");
    }

    // ---- solid-colour backgrounds in the draw list ----

    fn bg_draws(cmds: &[Cmd], tick: f32) -> Vec<Draw> {
        let mut v = View::new();
        v.apply(cmds);
        v.tick(tick);
        v.draw_list(&no_images, None)
    }

    fn color_bg(c: &str, fade: f32) -> Cmd {
        Cmd::SetBackground { src: c.into(), fade, wipe: None }
    }

    #[test]
    fn a_colour_background_fills_the_screen() {
        let list = bg_draws(&[color_bg("color:#102030", 0.0)], 0.0);
        let [Draw::Rect { rect, color }] = &list[..] else { panic!("{list:?}") };
        assert_eq!(*rect, Rect::new(0.0, 0.0, 1920.0, 1080.0));
        assert_eq!(*color, [16.0 / 255.0, 32.0 / 255.0, 48.0 / 255.0, 1.0]);
    }

    #[test]
    fn colours_and_pictures_crossfade_both_ways() {
        let pic = |_: &str| Some(Size { w: 1920.0, h: 1080.0 });
        let mut v = View::new();
        v.apply(&[Cmd::SetBackground { src: "a.png".into(), fade: 0.0, wipe: None }, color_bg("color:#ff0000", 1.0)]);
        v.tick(0.5);
        let list = v.draw_list(&pic, None);
        assert!(matches!(&list[0], Draw::Image { src, alpha, .. } if src == "a.png" && *alpha == 1.0), "{list:?}");
        assert!(matches!(&list[1], Draw::Rect { color, .. } if (color[3] - 0.5).abs() < 1e-4 && color[0] == 1.0), "{list:?}");
        v.apply(&[Cmd::SetBackground { src: "a.png".into(), fade: 1.0, wipe: None }]);
        v.tick(0.25);
        let list = v.draw_list(&pic, None);
        assert!(matches!(&list[0], Draw::Rect { color, .. } if color[3] == 1.0), "{list:?}");
        assert!(matches!(&list[1], Draw::Image { alpha, .. } if (*alpha - 0.25).abs() < 1e-4), "{list:?}");
        // colour to colour
        v.apply(&[color_bg("color:#00ff00", 1.0), color_bg("color:#0000ff", 1.0)]);
        v.tick(0.5);
        let list = v.draw_list(&pic, None);
        assert_eq!(list.iter().filter(|d| matches!(d, Draw::Rect { .. })).count(), 2);
    }

    #[test]
    fn the_same_colour_again_does_not_restart_its_fade() {
        let mut v = View::new();
        v.apply(&[color_bg("color:#ff0000", 1.0)]);
        v.tick(1.0);
        v.apply(&[color_bg("color:#ff0000", 1.0)]);
        assert_eq!(v.background.as_ref().unwrap().alpha(), 1.0);
    }

    #[test]
    fn a_colour_wipes_in_like_a_picture_and_ignores_the_camera() {
        let mut v = View::new();
        v.apply(&[Cmd::SetBackground { src: "color:#ff0000".into(), fade: 1.0, wipe: Some(quillstone_script::Wipe::Right) }]);
        v.tick(0.25);
        let list = v.draw_list(&no_images, None);
        let [Draw::Rect { rect, color }] = &list[..] else { panic!("{list:?}") };
        assert_eq!((rect.x, rect.w, color[3]), (0.0, 480.0, 1.0));
        let mut v = View::new();
        v.apply(&[color_bg("color:#ff0000", 0.0), Cmd::Camera { camera: quillstone_runtime::Camera { zoom: 3.0, x: 0.1, y: 0.1 }, time: 0.0, ease: quillstone_script::Ease::Linear }]);
        v.tick(0.0);
        let list = v.draw_list(&no_images, None);
        assert!(matches!(&list[0], Draw::Rect { rect, .. } if *rect == Rect::new(0.0, 0.0, 1920.0, 1080.0)));
    }
}
