//! The inventory, journal and shop screens' input: what a click, a key or the wheel does
//! on the grid (see `crate::bag` for the layout). The inventory is view-only; in a
//! shop a click on an item buys it. An item with a `use=` label has a Use button
//! (or Enter) while the story waits at a line.

use quillstone_runtime::{can_use, inventory_items, journal_items, ItemInfo};

use super::*;
use crate::bag::{self, BagCell, BagScreen, CellState, Detail, Hit};

/// Where the inventory or shop screen is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(super) struct Bag {
    /// The inventory was opened by the player (quick menu, `I`) over the story, as
    /// opposed to by `::inventory`, which leaves the story waiting for it.
    pub overlay: bool,
    /// The screen shows the journal's list (no Use, prices or counts) instead of the inventory's.
    pub journal: bool,
    pub page: usize,
    /// The cell the keyboard (or a click, in the inventory) is on: an index into all cells.
    pub focus: Option<usize>,
}

/// What the grid lists right now.
struct Shelf {
    cells: Vec<BagCell>,
    details: Vec<Detail>,
    /// The id of each inventory cell's item when it has a Use action (empty in a shop).
    uses: Vec<Option<String>>,
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

    /// Whether the game has a journal (a `journal:` list in the front matter).
    pub fn has_journal(&self) -> bool {
        self.program.journal.is_some()
    }

    /// Open the inventory over the story (the quick menu's Items, the `I` key).
    pub(super) fn open_inventory_overlay(&mut self) {
        if self.has_inventory() && self.in_story && !self.view.finished {
            self.open_bag(Screen::Inventory, true, false);
        }
    }

    /// Open the journal over the story (the quick menu's Journal, the `J` key).
    pub(super) fn open_journal_overlay(&mut self) {
        if self.has_journal() && self.in_story && !self.view.finished {
            self.open_bag(Screen::Inventory, true, true);
        }
    }

    fn open_bag(&mut self, screen: Screen, overlay: bool, journal: bool) {
        self.stop_skip();
        self.screen = screen;
        self.bag = Bag { overlay, journal, page: 0, focus: None };
    }

    /// The shop or inventory this game is at: the screen showing, or the one
    /// Settings, the history or a save screen is open over.
    fn bag_screen(&self) -> Option<Screen> {
        match self.screen {
            Screen::Inventory | Screen::Shop => Some(self.screen),
            _ => self.under.filter(|s| matches!(s, Screen::Inventory | Screen::Shop)),
        }
    }

    /// Another screen is about to open over the shop, inventory or text entry: remember it,
    /// so closing that screen comes back (page, focus and purchases kept).
    pub(super) fn cover_bag(&mut self) {
        if matches!(self.screen, Screen::Inventory | Screen::Shop | Screen::Input) {
            self.under = Some(self.screen);
        }
    }

    /// The story arrived at (or was redrawn at) a screen of its own.
    pub(super) fn bag_arrived(&mut self, cmds: &[Cmd]) {
        if !self.in_story {
            return;
        }
        for cmd in cmds {
            let (want, journal) = match cmd {
                Cmd::ShowInventory => (Screen::Inventory, false),
                Cmd::ShowJournal => (Screen::Inventory, true),
                Cmd::ShowShop { .. } => (Screen::Shop, false),
                _ => continue,
            };
            match self.screen {
                // A purchase shows the shop again: stay on the page and item.
                Screen::Shop if want == Screen::Shop => self.clamp_bag(),
                Screen::Story | Screen::Inventory | Screen::Shop => self.open_bag(want, false, journal),
                // Settings, the history or a save screen is open (a reload, say):
                // leave it be, and show the screen when it closes.
                _ if self.under == Some(Screen::Shop) && want == Screen::Shop => self.clamp_bag(),
                _ => {
                    self.bag = Bag { overlay: false, journal, page: 0, focus: None };
                    self.under = Some(want);
                }
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
        let Some(kind) = self.bag_screen() else { return };
        let waiting = &self.session.state().pending;
        let stale = match kind {
            Screen::Shop => !matches!(waiting, Some(Cmd::ShowShop { .. })),
            _ if self.bag.overlay => !if self.bag.journal { self.has_journal() } else { self.has_inventory() },
            _ if self.bag.journal => !matches!(waiting, Some(Cmd::ShowJournal)),
            _ => !matches!(waiting, Some(Cmd::ShowInventory)),
        };
        if !stale {
            self.clamp_bag();
        } else if self.screen == kind {
            self.screen = Screen::Story;
        } else {
            self.under = None;
        }
    }

    fn shelf(&self) -> Shelf {
        let info_detail = |i: &ItemInfo, price: Option<String>| Detail { name: i.name.clone(), text: i.text.clone(), price };
        let price_line = |p: i64| format!("Price: {p}");
        let shown = self.pending_shown();
        let localize = |mut i: ItemInfo| {
            if let Some(cat) = self.catalog() {
                quillstone_runtime::localize_item(&mut i, cat);
            }
            i
        };
        match (self.bag_screen().unwrap_or(self.screen), &shown) {
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
                Shelf { cells, details, uses: Vec::new(), shop: Some((format!("{}: {money}", capitalise(&label)), label)) }
            }
            (Screen::Shop, _) => Shelf { cells: Vec::new(), details: Vec::new(), uses: Vec::new(), shop: None },
            _ if self.bag.journal => {
                let entries: Vec<(ItemInfo, bool)> =
                    journal_items(self.program, &self.session.state().vars).into_iter().map(|(i, done)| (localize(i), done)).collect();
                // A finished entry is greyed and says Done; the screen is view-only.
                let cells = entries
                    .iter()
                    .map(|(i, done)| BagCell {
                        name: i.name.clone(),
                        icon: i.icon.clone(),
                        price: done.then(|| "Done".to_string()),
                        state: if *done { CellState::Disabled } else { CellState::Normal },
                    })
                    .collect();
                let details = entries.iter().map(|(i, _)| info_detail(i, None)).collect();
                Shelf { cells, details, uses: Vec::new(), shop: None }
            }
            _ => {
                let items: Vec<ItemInfo> = inventory_items(self.program, &self.session.state().vars).into_iter().map(localize).collect();
                let cells = items
                    .iter()
                    .map(|i| BagCell { name: i.name.clone(), icon: i.icon.clone(), price: i.count.map(|n| format!("x{n}")), state: CellState::Normal })
                    .collect();
                let details = items.iter().map(|i| info_detail(i, i.price.map(price_line))).collect();
                let uses = items.iter().map(|i| i.has_use.then(|| i.id.clone())).collect();
                Shelf { cells, details, uses, shop: None }
            }
        }
    }

    /// Whether the Use button works for the item `id`: the inventory was opened over the story
    /// (not by `::inventory`), no replay is running, and the story waits at a line (see `can_use`).
    fn can_use_now(&self, id: &str) -> bool {
        self.bag.overlay && self.screen == Screen::Inventory && self.replay.is_none() && can_use(self.program, self.session.state(), id)
    }

    /// The Use button for the selected item, if it has one: whether it works.
    fn use_button(&self, shelf: &Shelf) -> Option<bool> {
        let id = shelf.uses.get(self.bag.focus?)?.as_deref()?;
        Some(self.can_use_now(id))
    }

    /// The Use button or Enter: close the inventory and call the item's label.
    fn use_selected(&mut self, shelf: &Shelf) {
        let Some(id) = self.bag.focus.and_then(|f| shelf.uses.get(f)).cloned().flatten() else { return };
        if self.can_use_now(&id) {
            self.screen = Screen::Story;
            self.forward(Input::Use(id));
        } else {
            self.notify("Can't use that now");
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
        let use_button = self.use_button(&shelf);
        let hover = self.pointer().and_then(|(x, y)| match use_button {
            Some(_) if bag::use_rect(theme).contains(x, y) => Some(Hit::Use),
            _ => bag::hit(theme, x, y, shown.len(), pages),
        });
        let focus = self.bag.focus.and_then(|f| f.checked_sub(start)).filter(|&f| f < shown.len());
        // The description follows the mouse, else the keyboard.
        let detail_at = match hover {
            Some(Hit::Cell(i)) => Some(start + i),
            _ => focus.map(|f| start + f),
        };
        let money = shelf.shop.as_ref().map(|(line, _)| line.as_str());
        let background = if shop { theme.shop_background.as_deref().or(theme.inventory_background.as_deref()) } else { theme.inventory_background.as_deref() };
        let screen = BagScreen {
            title: match (shop, self.bag.journal) {
                (true, _) => &theme.shop_title,
                (_, true) => &theme.journal_title,
                _ => &theme.inventory_title,
            },
            background,
            money,
            cells: shown,
            page,
            pages,
            focus,
            hover,
            use_button,
            detail: detail_at.and_then(|i| shelf.details.get(i)),
            close_label: if shop { "Leave" } else { "Close" },
            empty: "Nothing here yet.",
        };
        bag::draw_bag(out, theme, &screen, image_size);
    }

    pub(super) fn bag_event(&mut self, event: Event) {
        let shelf = self.shelf(); // built once for the whole event
        let shelf_len = shelf.cells.len();
        let theme = self.view.theme();
        let (per, cols) = (bag::per_page(theme), bag::columns(theme));
        let pages = bag::pages(theme, shelf_len);
        let on_page = shelf_len.saturating_sub(self.bag.page * per).min(per);
        match event {
            Event::Press(Button::Left) => {
                let use_clicked = self.use_button(&shelf).is_some() && self.cursor.is_some_and(|(x, y)| bag::use_rect(theme).contains(x, y));
                if use_clicked {
                    return self.use_selected(&shelf);
                }
                match self.cursor.and_then(|(x, y)| bag::hit(theme, x, y, on_page, pages)) {
                    Some(Hit::Cell(i)) => self.bag_activate(self.bag.page * per + i, false, shelf),
                    Some(Hit::Prev) => self.turn_bag_page(-1, per, pages, shelf_len),
                    Some(Hit::Next) => self.turn_bag_page(1, per, pages, shelf_len),
                    Some(Hit::Close) => self.close_bag(),
                    Some(Hit::Use) | None => {}
                }
            }
            // The inventory over the story is an overlay like the history: Esc closes
            // it. The shop and a script's inventory are screens of the story, so
            // Esc and a right click open Settings, as everywhere else.
            Event::Press(Button::Right) | Event::Key(Key::Escape) if self.bag.overlay && self.screen == Screen::Inventory => {
                self.close_bag()
            }
            Event::Press(Button::Right) | Event::Key(Key::Escape) => self.open_settings(),
            Event::Key(Key::Char('s' | 'S')) => self.open_slots(Screen::Save),
            Event::Key(Key::Char('l' | 'L')) => self.open_slots(Screen::Load),
            Event::Key(Key::Char('h' | 'H')) => self.open_backlog(),
            Event::Key(Key::F5) => self.quick_save(),
            Event::Key(Key::F9) => self.quick_load(),
            Event::Key(Key::Char('i' | 'I')) if self.screen == Screen::Inventory && !self.bag.journal => self.close_bag(),
            Event::Key(Key::Char('j' | 'J')) if self.screen == Screen::Inventory && self.bag.journal => self.close_bag(),
            // Backspace (the gamepad's X) leaves too: a pad has no other way to press Leave.
            Event::Key(Key::Backspace) => self.close_bag(),
            Event::Wheel(dy) => self.turn_bag_page(if dy > 0.0 { -1 } else { 1 }, per, pages, shelf_len),
            Event::Key(Key::PageUp) => self.turn_bag_page(-1, per, pages, shelf_len),
            Event::Key(Key::PageDown) => self.turn_bag_page(1, per, pages, shelf_len),
            Event::Key(Key::Enter | Key::Space) => {
                if let Some(f) = self.bag.focus {
                    self.bag_activate(f, true, shelf);
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
                        _ if f + cols < shelf_len => f + cols,
                        _ => f, // nothing below in this column: stay put
                    },
                };
                self.bag.focus = Some(at.min(shelf_len - 1));
                self.bag.page = at.min(shelf_len - 1) / per;
            }
            _ => {}
        }
    }

    fn turn_bag_page(&mut self, by: isize, per: usize, pages: usize, n: usize) {
        let page = (self.bag.page as isize + by).clamp(0, pages as isize - 1) as usize;
        if page == self.bag.page {
            return;
        }
        self.bag.page = page;
        // Keep the keyboard on the same place of the new page.
        self.bag.focus = self.bag.focus.map(|f| (page * per + f % per).min(n.saturating_sub(1)));
    }

    /// Click or Enter on the cell `i`: look at it in the inventory (Enter also uses it), buy it in a shop.
    fn bag_activate(&mut self, i: usize, by_key: bool, shelf: Shelf) {
        self.bag.focus = Some(i);
        if self.screen != Screen::Shop {
            // Enter on an item with a Use action uses it; a click only selects it (the Use button is for that).
            if by_key && self.use_button(&shelf).is_some() {
                self.use_selected(&shelf);
            }
            return;
        }
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

    /// The Leave / Close button, or `I` in the inventory (`J` in the journal) (and Esc over the story: the
    /// overlay). Over the story that just closes it;
    /// where the script opened it, the story carries on.
    fn close_bag(&mut self) {
        let leaving_script_screen = self.screen == Screen::Shop || !self.bag.overlay;
        self.screen = Screen::Story;
        if leaving_script_screen {
            self.forward(Input::Advance);
        }
    }
}
