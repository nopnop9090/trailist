//! Windows-specific plumbing: finding the tray flyout, reading its icons,
//! grabbing their bitmaps, replaying clicks and touching the shell's registry
//! state.
//!
//! Everything in here is read-mostly and runs without elevation. The only writes
//! are the ones the user asks for by name: the `IsPromoted` flag, which is what
//! "always show this icon in the tray" means on Windows 11, and the autostart
//! entry behind a check box in the settings.

pub mod autostart;
pub mod capture;
pub mod focus;
pub mod forward;
pub mod glyph;
pub mod host;
pub mod island;
pub mod launch;
pub mod registry;
pub mod theme;
pub mod uia;