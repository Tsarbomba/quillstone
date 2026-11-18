//! The Messages screen's input: the contact list, a thread, picking a reply, the enlarged picture or video.
//! The look is in `messages.rs`; the messages themselves are in the story's state (`State::threads`).

use quillstone_render::{Draw, Rect, Size};
use quillstone_runtime::{can_reply, reply_options, speaker_name, Input};
use quillstone_script::markup::plain;
use quillstone_script::Op;

use super::*;
use crate::messages as ui;

/// A picture or video opened in the centred panel.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct Viewing {
    /// The picture, or the video.
    pub src: String,
    pub thumb: Option<String>,
    pub video: bool,
}

#[derive(Debug, Clone, Default)]
pub(super) struct MsgUi {
    /// The thread open (a contact id); none: the contact list.
    pub thread: Option<String>,
    /// Bubbles skipped from the newest (a thread), or rows from the top (the list).
    pub scroll: usize,
    /// The reply option the keyboard is on.
    pub focus: Option<usize>,
    /// The bubble whose picture the keyboard is on.
    pub pic_focus: Option<usize>,
    pub viewing: Option<Viewing>,
}

impl Game<'_> {
    /// The contact the story waits to hear a reply for (a `::reply{to=..}` without `later`).
    fn reply_contact(&self) -> Option<&str> {
        match (&self.session.state().pending, self.program.ops.get(self.session.state().pc)) {
            (Some(Cmd::ShowChoices { .. }), Some(Op::Choice { reply: Some(c), .. })) => Some(c.as_str()),
            _ => None,
        }
    }

    /// The video enlarged from a message, if that is what the screen shows.
    pub(super) fn viewed_message_video(&self) -> Option<&str> {
        let v = self.msgs.viewing.as_ref().filter(|v| v.video)?;
        (self.screen == Screen::Messages).then_some(v.src.as_str())
    }

    /// The story arrived at a reply it waits for: the Messages screen opens on that thread.
    pub(super) fn messages_arrived(&mut self) {
        if self.in_story && self.screen == Screen::Story && self.reply_contact().is_some() {
            self.open_messages();
        }
    }

    pub(super) fn open_messages(&mut self) {
        self.stop_skip();
        self.cover_bag();
        self.msgs = MsgUi::default();
        self.screen = Screen::Messages;
        if let Some(id) = self.reply_contact().map(str::to_string) {
            self.open_thread(id);
        }
    }

    fn open_thread(&mut self, id: String) {
        self.session.mark_read(&id);
        self.msgs = MsgUi { thread: Some(id), ..MsgUi::default() };
    }

    fn contact_name(&self, id: &str) -> String {
        let name = self.program.characters.get(id).map_or(id, |c| c.name.as_str());
        speaker_name(self.program, &Some(name.to_string()), &self.session.state().vars).unwrap_or_else(|| id.to_string())
    }

    /// The contacts, most recent first.
    fn contact_rows(&self) -> Vec<(String, ui::ContactRow)> {
        let state = self.session.state();
        state
            .threads
            .iter()
            .rev()
            .map(|t| {
                let last = t.msgs.last();
                let preview = match last {
                    Some(m) => {
                        let body = m.text.as_deref().map(|t| plain(self.tr(t))).unwrap_or_else(|| {
                            self.ui().t(if m.video.is_some() { "Video" } else { "Picture" }).to_string()
                        });
                        if m.mine { format!("> {body}") } else { body }
                    }
                    None => String::new(),
                };
                let waiting = !reply_options(self.program, state, &t.id, &|s| s.to_string()).is_empty();
                let portrait = self.program.characters.get(&t.id).and_then(|c| c.portrait.as_ref()).map(|(_, p)| p.clone());
                (t.id.clone(), ui::ContactRow { name: self.contact_name(&t.id), portrait, unread: t.unread, preview, waiting })
            })
            .collect()
    }

    fn bubbles(&self, id: &str) -> Vec<ui::Bubble> {
        let Some(t) = self.session.state().threads.iter().find(|t| t.id == id) else { return Vec::new() };
        t.msgs
            .iter()
            .map(|m| ui::Bubble {
                mine: m.mine,
                text: m.text.as_deref().map(|t| self.tr(t).to_string()),
                pic: match (&m.image, &m.video) {
                    (Some(src), _) => Some(ui::Pic { src: Some(src.clone()), video: false }),
                    (None, Some((_, thumb))) => Some(ui::Pic { src: thumb.clone(), video: true }),
                    _ => None,
                },
            })
            .collect()
    }

    /// The reply options at the foot of the open thread: `(index to send, text)`.
    fn thread_options(&self, id: &str) -> Vec<(usize, String)> {
        if self.reply_contact() == Some(id) {
            return self.view.choices.iter().cloned().enumerate().collect();
        }
        reply_options(self.program, self.session.state(), id, &|s| self.tr(s).to_string())
    }

    fn rows(&self) -> usize {
        ui::Geo::new(self.view.theme()).rows()
    }

    fn max_list_scroll(&self) -> usize {
        self.contact_rows().len().saturating_sub(self.rows())
    }

    pub(super) fn draw_messages_screen(&self, out: &mut Vec<Draw>, image_size: &dyn Fn(&str) -> Option<Size>) {
        let known = |s: &str| {
            let size = image_size(s);
            if let Some(z) = size {
                self.msg_sizes.borrow_mut().insert(s.to_string(), z);
            }
            size
        };
        let image_size: &dyn Fn(&str) -> Option<Size> = &known;
        let (theme, tr) = (self.view.theme(), self.ui());
        let pointer = self.pointer();
        let back_hover = pointer.is_some_and(|(x, y)| ui::Geo::new(self.view.theme()).back_rect().contains(x, y));
        match &self.msgs.thread {
            None => {
                let rows: Vec<ui::ContactRow> = self.contact_rows().into_iter().skip(self.msgs.scroll).take(self.rows()).map(|(_, r)| r).collect();
                let hover = pointer.and_then(|(x, y)| ui::Geo::new(theme).contact_at(x, y, rows.len())).or(self.msgs.focus);
                ui::draw_contacts(out, theme, tr, &rows, hover, back_hover, image_size);
            }
            Some(id) => {
                let (bubbles, options) = (self.bubbles(id), self.thread_options(id));
                let placed = ui::layout_thread(&ui::Geo::new(theme), &bubbles, self.msgs.scroll, options.len(), image_size);
                let texts: Vec<String> = options.into_iter().map(|(_, t)| t).collect();
                let hover = pointer.and_then(|(x, y)| ui::Geo::new(self.view.theme()).option_at(x, y, texts.len())).or(self.msgs.focus);
                let portrait = self.program.characters.get(id.as_str()).and_then(|c| c.portrait.as_ref()).map(|(_, p)| p.as_str());
                ui::draw_thread(out, theme, tr, &self.contact_name(id), portrait, &bubbles, &placed, &texts, hover, back_hover, image_size);
                if let Some(p) = self.msgs.pic_focus.and_then(|i| placed.iter().find(|p| p.index == i)).and_then(|p| p.pic) {
                    for r in [
                        Rect::new(p.x - 4.0, p.y - 4.0, p.w + 8.0, 4.0),
                        Rect::new(p.x - 4.0, p.y + p.h, p.w + 8.0, 4.0),
                        Rect::new(p.x - 4.0, p.y, 4.0, p.h),
                        Rect::new(p.x + p.w, p.y, 4.0, p.h),
                    ] {
                        out.push(Draw::Rect { rect: r, color: theme.accent_color });
                    }
                }
            }
        }
        if let Some(v) = &self.msgs.viewing {
            let srcs: Vec<&str> = if v.video { v.thumb.iter().map(String::as_str).chain([v.src.as_str()]).collect() } else { vec![v.src.as_str()] };
            ui::draw_viewer(out, theme, tr, &srcs, image_size);
        }
    }

    pub(super) fn messages_event(&mut self, event: Event) {
        if self.msgs.viewing.is_some() {
            return self.viewer_event(event);
        }
        match self.msgs.thread.clone() {
            None => self.contacts_event(event),
            Some(id) => self.thread_event(event, &id),
        }
    }

    fn viewer_event(&mut self, event: Event) {
        let Some(v) = self.msgs.viewing.clone() else { return };
        match event {
            Event::Press(Button::Left) => {
                let inside = self.cursor.is_some_and(|(x, y)| self.viewer_panel(&v).contains(x, y));
                if inside && v.video {
                    self.video_generation += 1; // play it again
                } else if !inside {
                    self.msgs.viewing = None;
                }
            }
            Event::Press(Button::Right) | Event::Key(Key::Escape | Key::Backspace) => self.msgs.viewing = None,
            Event::Key(Key::Enter | Key::Space) if v.video => self.video_generation += 1,
            _ => {}
        }
    }

    /// Where the enlarged panel is: it follows the shape of the picture (or the clip's thumbnail, else 16:9) as drawn.
    fn viewer_panel(&self, v: &Viewing) -> Rect {
        let sizes = self.msg_sizes.borrow();
        let natural = std::iter::once(&v.src).chain(&v.thumb).find_map(|s| sizes.get(s.as_str()).copied());
        ui::viewer_rect(natural)
    }

    fn close_messages(&mut self) {
        self.screen = self.home();
        self.msgs = MsgUi::default();
    }

    fn contacts_event(&mut self, event: Event) {
        let rows = self.contact_rows();
        let visible = rows.len().saturating_sub(self.msgs.scroll).min(self.rows());
        let open = |g: &mut Self, i: usize| {
            if let Some((id, _)) = rows.get(g.msgs.scroll + i) {
                g.open_thread(id.clone());
            }
        };
        match event {
            Event::Press(Button::Left) => match self.cursor {
                Some((x, y)) if ui::Geo::new(self.view.theme()).back_rect().contains(x, y) => self.close_messages(),
                Some((x, y)) => {
                    if let Some(i) = ui::Geo::new(self.view.theme()).contact_at(x, y, visible) {
                        open(self, i);
                    }
                }
                None => {}
            },
            Event::Press(Button::Right) | Event::Key(Key::Escape | Key::Backspace) => self.close_messages(),
            Event::Wheel(dy) => {
                let max = self.max_list_scroll();
                self.msgs.scroll = if dy > 0.0 { self.msgs.scroll.saturating_sub(1) } else { (self.msgs.scroll + 1).min(max) };
            }
            Event::Key(Key::Down) if visible > 0 => {
                let f = self.msgs.focus.map_or(0, |f| f + 1);
                if f >= visible && self.msgs.scroll < self.max_list_scroll() {
                    self.msgs.scroll += 1;
                }
                self.msgs.focus = Some(f.min(visible - 1));
            }
            Event::Key(Key::Up) if visible > 0 => match self.msgs.focus {
                Some(0) if self.msgs.scroll > 0 => self.msgs.scroll -= 1,
                Some(f) => self.msgs.focus = Some(f.saturating_sub(1)),
                None => self.msgs.focus = Some(0),
            },
            Event::Key(Key::Enter | Key::Space) => {
                if let Some(f) = self.msgs.focus {
                    open(self, f);
                }
            }
            _ => {}
        }
    }

    fn thread_event(&mut self, event: Event, id: &str) {
        let (bubbles, options) = (self.bubbles(id), self.thread_options(id));
        let waiting = self.reply_contact() == Some(id);
        let max = bubbles.len().saturating_sub(1);
        let scroll = |g: &mut Self, d: isize| g.msgs.scroll = (g.msgs.scroll as isize + d).clamp(0, max as isize) as usize;
        match event {
            Event::Press(Button::Left) => {
                let Some((x, y)) = self.cursor else { return };
                if ui::Geo::new(self.view.theme()).back_rect().contains(x, y) {
                    self.leave_thread(waiting);
                } else if let Some(i) = ui::Geo::new(self.view.theme()).option_at(x, y, options.len()) {
                    self.pick_reply(id, options[i].0, waiting);
                } else if let Some(b) = self.picture_under(id, x, y) {
                    self.view_bubble(id, b);
                }
            }
            Event::Press(Button::Right) | Event::Key(Key::Escape) => self.leave_thread(waiting),
            Event::Key(Key::Backspace) if !waiting => self.leave_thread(false),
            Event::Wheel(dy) => scroll(self, if dy > 0.0 { 1 } else { -1 }),
            Event::Key(Key::PageUp) => scroll(self, 5),
            Event::Key(Key::PageDown) => scroll(self, -5),
            Event::Key(Key::Home) => self.msgs.scroll = max,
            Event::Key(Key::End) => self.msgs.scroll = 0,
            Event::Key(Key::Up) if !options.is_empty() => {
                self.msgs.pic_focus = None;
                self.msgs.focus = Some(self.msgs.focus.map_or(options.len() - 1, |f| f.saturating_sub(1)));
            }
            Event::Key(Key::Down) if !options.is_empty() => {
                self.msgs.pic_focus = None;
                self.msgs.focus = Some(self.msgs.focus.map_or(0, |f| (f + 1).min(options.len() - 1)));
            }
            Event::Key(Key::Up) => scroll(self, 1),
            Event::Key(Key::Down) => scroll(self, -1),
            Event::Key(Key::Left | Key::Right) => {
                // Pictures, by the keyboard: oldest to newest among the bubbles that have one.
                let with: Vec<usize> = bubbles.iter().enumerate().filter(|(_, b)| b.pic.is_some()).map(|(i, _)| i).collect();
                let at = self.msgs.pic_focus.and_then(|f| with.iter().position(|&i| i == f));
                let next = match (event, at) {
                    (Event::Key(Key::Left), Some(a)) => with.get(a.wrapping_sub(1)).or(with.last()),
                    (Event::Key(Key::Left), None) => with.last(),
                    (_, Some(a)) => with.get(a + 1).or(with.first()),
                    (_, None) => with.first(),
                };
                self.msgs.pic_focus = next.copied();
                self.msgs.focus = None;
                if let Some(i) = self.msgs.pic_focus {
                    let back = bubbles.len() - 1 - i;
                    if back < self.msgs.scroll {
                        self.msgs.scroll = back;
                    }
                }
            }
            Event::Key(Key::Enter | Key::Space) => match (self.msgs.focus, self.msgs.pic_focus) {
                (Some(f), _) if f < options.len() => self.pick_reply(id, options[f].0, waiting),
                (_, Some(b)) => self.view_bubble(id, b),
                _ => {}
            },
            _ => {}
        }
    }

    /// The bubble whose picture is at the point (needs the sizes the draw used, so it asks the view's pictures: unknown
    /// sizes lay out as the placeholder shape, which is what a click on an unloaded picture is tested against).
    fn picture_under(&self, id: &str, x: f32, y: f32) -> Option<usize> {
        let (bubbles, options) = (self.bubbles(id), self.thread_options(id));
        let placed = ui::layout_thread(&ui::Geo::new(self.view.theme()), &bubbles, self.msgs.scroll, options.len(), &|s| self.msg_sizes.borrow().get(s).copied());
        ui::picture_at(&placed, x, y)
    }

    fn view_bubble(&mut self, id: &str, bubble: usize) {
        let Some(m) = self.session.state().threads.iter().find(|t| t.id == id).and_then(|t| t.msgs.get(bubble)) else { return };
        let viewing = match (&m.image, &m.video) {
            (Some(src), _) => Viewing { src: src.clone(), thumb: None, video: false },
            (None, Some((src, thumb))) => Viewing { src: src.clone(), thumb: thumb.clone(), video: true },
            _ => return,
        };
        self.video_generation += 1;
        self.msgs.viewing = Some(viewing);
    }

    /// Back: to the contact list, or (at a reply the story waits for) the game menu, as Esc does at a choice.
    fn leave_thread(&mut self, waiting: bool) {
        if waiting {
            self.open_game_menu();
        } else {
            self.msgs = MsgUi::default();
        }
    }

    fn pick_reply(&mut self, id: &str, index: usize, waiting: bool) {
        if !waiting && !can_reply(self.program, self.session.state(), id, index) {
            return self.notify_ui("Can't use that now", &[]);
        }
        self.screen = Screen::Story;
        self.msgs = MsgUi::default();
        self.forward(if waiting { Input::Choose(index) } else { Input::Reply(id.to_string(), index) });
    }
}
