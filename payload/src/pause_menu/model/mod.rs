//! The VR menu's pure logic, testable without the game: the page model, row ids and row encoding,
//! slider mapping, the live-value diff, the menu state machine, command routing, and finding the
//! chosen VR entry.

pub(crate) mod command;
pub(crate) mod entries;
pub(crate) mod ids;
pub(crate) mod live;
pub(crate) mod page;
pub(crate) mod rows;
pub(crate) mod slider;
pub(crate) mod state;
