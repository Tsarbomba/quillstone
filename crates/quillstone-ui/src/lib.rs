//! Quillstone's game shell, minus the window: input routing, settings, the
//! backlog and the quick menu.
//!
//! - [`game`]: [`Game`] ties the runtime, the view and the audio mixer together
//!   and decides what each click or key does on the current screen.
//! - [`settings`]: player preferences and their file format.
//! - [`slots`]: save slots, the save file header, thumbnails, and the
//!   [`Storage`] the frontend provides for reading and writing files.
//! - [`screens`]: layout, hit-testing and drawing for the settings screen, the
//!   backlog and the quick menu; [`extras`]: the same for the gallery and music room.
//!
//! The player (`player/`) only translates window events into [`Event`]s and
//! carries out what comes back, so all of this is unit-tested here.

pub mod archive;
pub mod assets;
pub mod bag;
pub mod extras;
pub mod game;
pub mod input;
pub mod screens;
pub mod settings;
pub mod slots;
pub mod source;

pub use assets::Assets;
pub use game::{Button, Event, Game, Key, Request, Screen, AUTO_PER_CHAR};
pub use settings::{QuickMenuSetting, Settings};
pub use quillstone_runtime::MAX_SAVE_BYTES;
pub use slots::{MemoryStorage, Storage, Timestamp};
