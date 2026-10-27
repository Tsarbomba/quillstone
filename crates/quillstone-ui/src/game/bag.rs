//! The inventory and shop screens' input: what a click, a key or the wheel does
//! on the grid (see `crate::bag` for the layout). The inventory is view-only; in a
//! shop a click on an item buys it.

use quillstone_runtime::{inventory_items, ItemInfo};

use super::*;
use crate::bag::{self, BagCell, BagScreen, CellState, Detail, Hit};

/// Where the inventory or shop screen is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(super) struct Bag {
    /// The inventory was opened by the player (quick menu, `I`) over the story, as
    /// opposed to by `::inventory`, which leaves the story waiting for it.
    pub overlay: bool,
    pub page: usize,
    /// The cell the keyboard (or a click, in the inventory) is on: an index into all cells.
    pub focus: Option<usize>,
}

/// What the grid lists right now.
struct Shelf {
    cells: Vec<BagCell>,
    details: Vec<Detail>,
    /// The shop's money line and label for what it is paid in.
    shop: Option<(String, String)>,
}

fn capitalise(s: &str) -> String {
    let mut chars = s.chars();
    chars.next().map(|c| c.to_uppercase().chain(chars).collect()).unwrap_or_default()
}

impl Game<'_> {
    /// Whether the game has an inventory (an `inventory:` list in the front matter).
    pub fn has_inventory(&self) -> bool {
        self.program.inventory.is_some()
    }

    /// Open the inventory over the story (the quick menu's Items, the `I` key).
    pub(super) fn open_inventory_overlay(&mut self) {
        if self.has_inventory() && self.in_story && !self.view.finished {
            self.open_bag(Screen::Inventory, true);
        }
    }

    fn open_bag(&mut self, screen: Screen, overlay: bool) {
        self.stop_skip();
        self.screen = screen;
        self.bag = Bag { overlay, page: 0, focus: None };
    }

    /// The story arrived at (or was redrawn at) a screen of its own.
    pub(super) fn bag_arrived(&mut self, cmds: &[Cmd]) {
        if !self.in_story {
            return;
        }
        for cmd in cmds {
            match cmd {
                Cmd::ShowInventory => self.open_bag(Screen::Inventory, false),
                // A purchase shows the shop again: stay on the page and item.
                Cmd::ShowShop { .. } if self.screen == Screen::Shop => self.clamp_bag(),
                Cmd::ShowShop { .. } => self.open_bag(Screen::Shop, false),
                _ => {}
            }
        }
    }

    fn clamp_bag(&mut self) {
        let n = self.shelf().cells.len();
        let theme = self.view.theme();
        self.bag.page = self.bag.page.min(bag::pages(theme, n) - 1);
        self.bag.focus = self.bag.focus.filter(|&f| f < n);
    }

    /// After the script changed under a screen that belongs to it: back to the story
    /// when what that screen was for is no longer what the story waits at.
    pub(super) fn bag_resync(&mut self) {
        let waiting = &self.session.state().pending;
        let stale = match self.screen {
            Screen::Shop => !matches!(waiting, Some(Cmd::ShowShop { .. })),
            Screen::Inventory if self.bag.overlay => !self.has_inventory(),
            Screen::Inventory => !matches!(waiting, Some(Cmd::ShowInventory)),
            _ => false,
        };
        if stale {
            self.screen = Screen::Story;
        } else if matches!(self.screen, Screen::Inventory | Screen::Shop) {
            self.clamp_bag();
        }
    }

    fn shelf(&self) -> Shelf {
        let info_detail = |i: &ItemInfo, price: Option<String>| Detail { name: i.name.clone(), text: i.text.clone(), price };
        let price_line = |p: i64| format!("Price: {p}");
        match (self.screen, &self.session.state().pending) {
            (Screen::Shop, Some(Cmd::ShowShop { pay, money, items })) => {
                let label = self.theme().shop_currency.clone().unwrap_or_else(|| pay.clone());
                let cells = items
                    .iter()
                    .map(|s| BagCell {
                        name: s.item.name.clone(),
                        icon: s.item.icon.clone(),
                        price: Some(format!("{} {label}", s.price)),
                        state: if s.owned {
                            CellState::Owned
                        } else if s.price > *money {
                            CellState::Disabled
                        } else {
                            CellState::Normal
                        },
                    })
                    .collect();
                let details = items.iter().map(|s| info_detail(&s.item, Some(format!("{} {label}", price_line(s.price))))).collect();
                Shelf { cells, details, shop: Some((format!("{}: {money}", capitalise(&label)), label)) }
            }
            (Screen::Shop, _) => Shelf { cells: Vec::new(), details: Vec::new(), shop: None },
            _ => {
                let items = inventory_items(self.program, &self.session.state().vars);
                let cells = items
                    .iter()
                    .map(|i| BagCell { name: i.name.clone(), icon: i.icon.clone(), price: None, state: CellState::Normal })
                    .collect();
                let details = items.iter().map(|i| info_detail(i, i.price.map(price_line))).collect();
                Shelf { cells, details, shop: None }
            }
        }
    }

    /// The screen as drawn.
    pub(super) fn draw_bag_screen(&self, out: &mut Vec<Draw>, image_size: &dyn Fn(&str) -> Option<Size>) {
        let theme = self.view.theme();
        let shelf = self.shelf();
        let shop = self.screen == Screen::Shop;
        let per = bag::per_page(theme);
        let pages = bag::pages(theme, shelf.cells.len());
        let page = self.bag.page.min(pages - 1);
        let start = page * per;
        let shown = &shelf.cells[start.min(shelf.cells.len())..(start + per).min(shelf.cells.len())];
        let hover = self.cursor.and_then(|(x, y)| bag::hit(theme, x, y, shown.len(), pages));
        let focus = self.bag.focus.and_then(|f| f.checked_sub(start)).filter(|&f| f < shown.len());
        // The description follows the mouse, else the keyboard.
        let detail_at = match hover {
            Some(Hit::Cell(i)) => Some(start + i),
            _ => focus.map(|f| start + f),
        };
        let money = shelf.shop.as_ref().map(|(line, _)| line.as_str());
        let background = if shop { theme.shop_background.as_deref().or(theme.inventory_background.as_deref()) } else { theme.inventory_background.as_deref() };
        let screen = BagScreen {
            title: if shop { &theme.shop_title } else { &theme.inventory_title },
            background,
            money,
            cells: shown,
            page,
            pages,
            focus,
            hover,
            detail: detail_at.and_then(|i| shelf.details.get(i)),
            close_label: if shop { "Leave" } else { "Close" },
            empty: "Nothing here yet.",
        };
        bag::draw_bag(out, theme, &screen, image_size);
    }

    pub(super) fn bag_event(&mut self, event: Event) {
        let shelf_len = self.shelf().cells.len();
        let theme = self.view.theme();
        let (per, cols) = (bag::per_page(theme), bag::columns(theme));
        let pages = bag::pages(theme, shelf_len);
        let on_page = shelf_len.saturating_sub(self.bag.page * per).min(per);
        match event {
            Event::Press(Button::Left) => {
                match self.cursor.and_then(|(x, y)| bag::hit(theme, x, y, on_page, pages)) {
                    Some(Hit::Cell(i)) => self.bag_activate(self.bag.page * per + i),
                    Some(Hit::Prev) => self.turn_bag_page(-1, per, pages),
                    Some(Hit::Next) => self.turn_bag_page(1, per, pages),
                    Some(Hit::Close) => self.close_bag(),
                    None => {}
                }
            }
            Event::Press(Button::Right) | Event::Key(Key::Escape) => self.close_bag(),
            Event::Key(Key::Char('i' | 'I')) if self.screen == Screen::Inventory => self.close_bag(),
            Event::Wheel(dy) => self.turn_bag_page(if dy > 0.0 { -1 } else { 1 }, per, pages),
            Event::Key(Key::PageUp) => self.turn_bag_page(-1, per, pages),
            Event::Key(Key::PageDown) => self.turn_bag_page(1, per, pages),
            Event::Key(Key::Enter | Key::Space) => {
                if let Some(f) = self.bag.focus {
                    self.bag_activate(f);
                }
            }
            Event::Key(key @ (Key::Up | Key::Down | Key::Left | Key::Right)) if shelf_len > 0 => {
                // Arrows walk the whole list, so the page follows the focus.
                let at = match self.bag.focus {
                    None => self.bag.page * per,
                    Some(f) => match key {
                        Key::Left => f.saturating_sub(1),
                        Key::Right => (f + 1).min(shelf_len - 1),
                        Key::Up if f >= cols => f - cols,
                        Key::Up => f,
                        _ => (f + cols).min(shelf_len - 1),
                    },
                };
                self.bag.focus = Some(at.min(shelf_len - 1));
                self.bag.page = at.min(shelf_len - 1) / per;
            }
            _ => {}
        }
    }

    fn turn_bag_page(&mut self, by: isize, per: usize, pages: usize) {
        let page = (self.bag.page as isize + by).clamp(0, pages as isize - 1) as usize;
        if page == self.bag.page {
            return;
        }
        self.bag.page = page;
        // Keep the keyboard on the same place of the new page.
        let n = self.shelf().cells.len();
        self.bag.focus = self.bag.focus.map(|f| (page * per + f % per).min(n.saturating_sub(1)));
    }

    /// Click or Enter on the cell `i`: look at it in the inventory, buy it in a shop.
    fn bag_activate(&mut self, i: usize) {
        self.bag.focus = Some(i);
        if self.screen != Screen::Shop {
            return;
        }
        let shelf = self.shelf();
        let Some(cell) = shelf.cells.get(i) else { return };
        match cell.state {
            CellState::Owned => self.notify("Already owned"),
            CellState::Disabled => {
                let label = shelf.shop.map(|(_, label)| label).unwrap_or_default();
                self.notify(format!("Not enough {label}"));
            }
            CellState::Normal => {
                let name = cell.name.clone();
                self.forward(Input::Buy(i));
                if matches!(self.session.state().pending, Some(Cmd::ShowShop { .. })) {
                    self.notify(format!("Bought {name}"));
                }
            }
        }
    }

    /// Esc, right click, `I` or the button. Over the story that just closes it;
    /// where the script opened it, the story carries on.
    fn close_bag(&mut self) {
        let leaving_script_screen = self.screen == Screen::Shop || !self.bag.overlay;
        self.screen = Screen::Story;
        if leaving_script_screen {
            self.forward(Input::Advance);
        }
    }
}
