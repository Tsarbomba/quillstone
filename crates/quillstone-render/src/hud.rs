//! The HUD layer: always-on overlay elements (`::hud`). Their definitions come from
//! the runtime ([`Cmd::HudShow`] and friends); what they show (text with `{name}` values,
//! bar fractions, conditions) is worked out each time the draw list is built, from the
//! variables the game last handed over with [`HudLayer::set_vars`].

use quillstone_runtime::{interpolate, Cmd};
use quillstone_script::markup::rich;
use quillstone_script::{HudAnchor, HudElem, HudKind, Vars};

use crate::layout::{is_wide, Rect, Size, VIRTUAL};
use crate::theme::Theme;
use crate::view::{faded, Align, Draw};

/// One element on screen, with its fade.
#[derive(Debug, Clone, PartialEq)]
struct Item {
    elem: HudElem,
    alpha: f32,
    /// 1 while shown or fading in, 0 while fading out (it is dropped at 0).
    target: f32,
    /// Opacity change per second (0: at once).
    rate: f32,
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
        self.items.iter().any(|i| i.alpha != i.target)
    }

    /// Finish every fade at once (skipping doesn't wait for them).
    pub fn end_fades(&mut self) {
        self.items.retain(|i| i.target > 0.0);
        for i in &mut self.items {
            i.alpha = 1.0;
        }
    }

    pub fn apply(&mut self, cmd: &Cmd) {
        match cmd {
            Cmd::HudShow { elem, fade } => {
                let rate = if *fade > 0.0 { 1.0 / fade } else { 0.0 };
                match self.items.iter_mut().find(|i| i.elem.id == elem.id) {
                    Some(item) => {
                        let was_hiding = item.target == 0.0;
                        item.elem = (**elem).clone();
                        item.target = 1.0;
                        item.rate = rate;
                        // Replacing a shown element is seamless unless it is asked to fade in.
                        if rate > 0.0 && (was_hiding || *fade > 0.0) {
                            item.alpha = if was_hiding { item.alpha } else { 0.0 };
                        } else {
                            item.alpha = 1.0;
                        }
                    }
                    None => self.items.push(Item { elem: (**elem).clone(), alpha: if rate > 0.0 { 0.0 } else { 1.0 }, target: 1.0, rate }),
                }
            }
            Cmd::HudHide { id, fade } => {
                if *fade > 0.0 {
                    if let Some(item) = self.items.iter_mut().find(|i| i.elem.id == *id) {
                        item.target = 0.0;
                        item.rate = 1.0 / fade;
                    }
                } else {
                    self.items.retain(|i| i.elem.id != *id);
                }
            }
            Cmd::HudClear { fade } => {
                if *fade > 0.0 {
                    for item in &mut self.items {
                        item.target = 0.0;
                        item.rate = 1.0 / fade;
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
            i.alpha = if i.rate > 0.0 {
                let step = i.rate * dt;
                if i.alpha < i.target { (i.alpha + step).min(i.target) } else { (i.alpha - step).max(i.target) }
            } else {
                i.target
            };
        }
        self.items.retain(|i| i.target > 0.0 || i.alpha > 0.0);
    }

    /// Push the draws of every element that is on (its condition true, not faded out).
    pub fn draw(&self, theme: &Theme, image_size: &dyn Fn(&str) -> Option<Size>, out: &mut Vec<Draw>) {
        for item in self.items.iter().filter(|i| i.alpha > 0.0) {
            let e = &item.elem;
            if e.cond.as_ref().is_some_and(|c| !c.expr.test(&self.vars)) {
                continue;
            }
            let (px, py) = (e.at[0] * VIRTUAL.w, e.at[1] * VIRTUAL.h);
            match &e.kind {
                HudKind::Text { text, size, color, back, pad } => {
                    let shown = interpolate(text, &self.vars);
                    let size = size.unwrap_or(theme.text_size);
                    let color = faded(color.unwrap_or(theme.text_color), item.alpha);
                    draw_text(out, &shown, (px, py), e.anchor, size, color, back.map(|b| faded(b, item.alpha)), *pad);
                }
                HudKind::Bar { value, max, size: [w, h], color, back, border } => {
                    let int = |x: &quillstone_script::HudExpr| x.expr.eval(&self.vars).as_int().unwrap_or(0);
                    let (v, m) = (int(value), int(max));
                    let frac = if m <= 0 { 0.0 } else { (v as f64 / m as f64).clamp(0.0, 1.0) as f32 };
                    let x = anchored(px, *w, e.anchor);
                    let r = Rect::new(x, py, *w, *h);
                    if let Some((c, t)) = border {
                        let c = faded(*c, item.alpha);
                        for bar in [
                            Rect::new(r.x - t, r.y - t, r.w + 2.0 * t, *t),
                            Rect::new(r.x - t, r.y + r.h, r.w + 2.0 * t, *t),
                            Rect::new(r.x - t, r.y, *t, r.h),
                            Rect::new(r.x + r.w, r.y, *t, r.h),
                        ] {
                            out.push(Draw::Rect { rect: bar, color: c });
                        }
                    }
                    out.push(Draw::Rect { rect: r, color: faded(*back, item.alpha) });
                    if frac > 0.0 {
                        let fill = faded(color.unwrap_or(theme.accent_color), item.alpha);
                        out.push(Draw::Rect { rect: Rect::new(r.x, r.y, r.w * frac, r.h), color: fill });
                    }
                }
                HudKind::Image { src, scale } => {
                    let Some(s) = image_size(src).filter(|s| s.w > 0.0 && s.h > 0.0) else { continue };
                    let (w, h) = (s.w * scale, s.h * scale);
                    let dest = Rect::new(anchored(px, w, e.anchor), py, w, h);
                    out.push(Draw::Image { src: src.clone(), dest, uv: Rect::new(0.0, 0.0, 1.0, 1.0), alpha: item.alpha });
                }
            }
        }
    }
}

/// The left edge of something `w` wide whose anchor point is at `x`.
fn anchored(x: f32, w: f32, anchor: HudAnchor) -> f32 {
    match anchor {
        HudAnchor::Left => x,
        HudAnchor::Center => x - w / 2.0,
        HudAnchor::Right => x - w,
    }
}

/// Line height the player's text engine uses (see `quillstone-render` `TextJob`s: 1.35 x size).
const LINE: f32 = 1.35;

/// A cautious width of one line of text at `size`: generous, so a panel built from it never
/// makes the text wrap (the real text engine is not visible from here).
fn line_width(line: &str, size: f32, scale_at: &dyn Fn(usize) -> f32, offset: usize) -> f32 {
    let w: f32 = line
        .char_indices()
        .map(|(at, c)| {
            size * scale_at(offset + at) * if is_wide(c) {
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
            }
        })
        .sum();
    w * 1.06 + 2.0
}

#[allow(clippy::too_many_arguments)]
fn draw_text(
    out: &mut Vec<Draw>,
    shown: &str,
    (px, py): (f32, f32),
    anchor: HudAnchor,
    size: f32,
    color: [f32; 4],
    back: Option<[f32; 4]>,
    pad: f32,
) {
    let r = rich(shown);
    // `{size=..}` runs make characters (and so lines) bigger than the base size.
    let run_size = |run: &quillstone_script::markup::TextRun| run.style.size.map_or(size, |s| s.resolve(size));
    let tallest = r.runs.iter().map(run_size).fold(size, f32::max);
    let scale_at = |byte: usize| r.runs.iter().find(|run| run.start <= byte && byte < run.end).map_or(1.0, |run| run_size(run) / size.max(1.0));
    let lines = r.text.split('\n').count().max(1) as f32;
    let height = lines * tallest * LINE + 2.0 * pad;
    let align = match anchor {
        HudAnchor::Left => Align::Left,
        HudAnchor::Center => Align::CenterX,
        HudAnchor::Right => Align::Right,
    };
    let rect = match back {
        // A panel is as wide as the text is estimated to be, so it can be drawn behind it.
        Some(back) => {
            let mut offset = 0;
            let mut widest = 0.0f32;
            for l in r.text.split('\n') {
                widest = widest.max(line_width(l, size, &scale_at, offset));
                offset += l.len() + 1;
            }
            let panel = Rect::new(anchored(px, widest + 2.0 * pad, anchor), py, widest + 2.0 * pad, height);
            out.push(Draw::Rect { rect: panel, color: back });
            Rect::new(panel.x + pad, panel.y + pad, panel.w - 2.0 * pad, panel.h - 2.0 * pad)
        }
        // No panel: the text gets all the room on its side of the anchor point and is aligned
        // against it, so it is placed exactly whatever the font.
        None => {
            match anchor {
                HudAnchor::Left => Rect::new(px + pad, py + pad, (VIRTUAL.w - px - pad).max(1.0), height - 2.0 * pad),
                HudAnchor::Right => Rect::new(0.0, py + pad, (px - pad).max(1.0), height - 2.0 * pad),
                HudAnchor::Center => {
                    let half = px.min(VIRTUAL.w - px).max(0.5);
                    Rect::new(px - half, py + pad, 2.0 * half, height - 2.0 * pad)
                }
            }
        }
    };
    out.push(Draw::Text { text: r.text, runs: r.runs, visible_chars: usize::MAX, rect, size, color, align });
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

    const BAR: &str = "---\nvars: { hp: 5, max: 10 }\n---\n::hud{id=b bar=hp max=max at=0.1,0.2 size=200,20 color=#ff0000 back=#000000}\nHi.\n";

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
        assert_eq!(r[0].0, Rect::new(192.0, 216.0, 200.0, 20.0));
        assert_eq!(r[1].0, Rect::new(192.0, 216.0, 100.0, 20.0));
        let v = view("---\nvars: { hp: 5 }\n---\n::hud{id=b bar=hp max=10 size=200,20 border=#ffffff border_size=3 at=0,0}\nHi.\n");
        let r = rects(&v.draw_list(&no_images, None));
        assert_eq!(r.len(), 6, "four border edges, the back, the fill");
        assert_eq!(r[0].0, Rect::new(-3.0, -3.0, 206.0, 3.0));
        assert_eq!(r[1].0, Rect::new(-3.0, 20.0, 206.0, 3.0));
    }

    #[test]
    fn the_anchor_decides_which_side_of_the_point_the_element_is_on() {
        let bar = |anchor: &str| {
            let v = view(&format!("::hud{{id=b bar=1 max=2 at=0.5,0 size=200,20 anchor={anchor}}}\nHi.\n"));
            rects(&v.draw_list(&no_images, None))[0].0
        };
        assert_eq!(bar("left").x, 960.0);
        assert_eq!(bar("center").x, 860.0);
        assert_eq!(bar("right").x, 760.0);
        let text = |anchor: &str, at: f32| {
            let v = view(&format!("::hud{{id=t text=Hello at={at},0.1 anchor={anchor}}}\nHi.\n"));
            v.draw_list(&no_images, None).into_iter().find_map(|d| match d { Draw::Text { text, rect, align, .. } if text == "Hello" => Some((rect, align)), _ => None }).unwrap()
        };
        let (r, a) = text("left", 0.25);
        assert_eq!((r.x, r.x + r.w, a), (480.0, 1920.0, Align::Left));
        let (r, a) = text("right", 0.75);
        assert_eq!((r.x, r.x + r.w, a), (0.0, 1440.0, Align::Right));
        let (r, a) = text("center", 0.25);
        assert_eq!((r.x, r.x + r.w, a), (0.0, 960.0, Align::CenterX), "centred on the point, as far as the screen allows");
        assert_eq!(r.y, 108.0);
    }

    #[test]
    fn a_panel_sits_behind_the_text_with_the_padding_around_it() {
        let v = view("::hud{id=t text=Hello at=0.5,0.1 anchor=center size=40 back=#00000099 pad=12}\nHi.\n");
        let list = v.draw_list(&no_images, None);
        let panel = rects(&list)[0].0;
        let Some(Draw::Text { rect, .. }) = list.iter().find(|d| matches!(d, Draw::Text { text, .. } if text == "Hello")) else { panic!() };
        assert!(matches!(list[list.len() - 2], Draw::Rect { .. }), "the panel is drawn just before its text");
        assert_eq!((rect.x - panel.x, rect.y - panel.y), (12.0, 12.0));
        assert!((panel.x + panel.w / 2.0 - 960.0).abs() < 0.01, "centred on the point");
        assert!((panel.w - rect.w - 24.0).abs() < 0.01 && (panel.h - rect.h - 24.0).abs() < 0.01);
        assert!(panel.w > 40.0 * 2.0, "wide enough for five letters");
        let right = view("::hud{id=t text=Hello at=0.5,0.1 anchor=right size=40 back=#000 pad=12}\nHi.\n");
        let p = rects(&right.draw_list(&no_images, None))[0].0;
        assert!((p.x + p.w - 960.0).abs() < 0.01);
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
    fn an_if_hides_the_element_until_it_is_true() {
        let mut v = view("---\nvars: { hp: 9 }\n---\n::hud{id=w text=LOW if=\"hp <= 3\"}\n::hud{id=x text=ok}\nHi.\n");
        assert_eq!(texts(&v.draw_list(&no_images, None)), ["ok"]);
        v.set_vars(&vars(&[("hp", 2)]));
        assert_eq!(texts(&v.draw_list(&no_images, None)), ["LOW", "ok"]);
        v.set_vars(&vars(&[("hp", 4)]));
        assert_eq!(texts(&v.draw_list(&no_images, None)), ["ok"]);
    }

    #[test]
    fn an_image_is_drawn_at_its_own_size_times_scale_by_its_anchor() {
        let v = view("::hud{id=h image=a.png at=0.5,0.5 anchor=center scale=0.5}\nHi.\n");
        let size = |_: &str| Some(Size { w: 100.0, h: 60.0 });
        let list = v.draw_list(&size, None);
        let Some(Draw::Image { dest, alpha, uv, .. }) = list.first() else { panic!("{list:?}") };
        assert_eq!((*dest, *alpha, *uv), (Rect::new(935.0, 540.0, 50.0, 30.0), 1.0, Rect::new(0.0, 0.0, 1.0, 1.0)));
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
