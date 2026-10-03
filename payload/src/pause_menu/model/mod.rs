//! The VR menu's pure logic, testable without the game: the page model, row ids and row encoding,
//! slider mapping, the live-value diff, the menu state machine, command routing, finding the chosen
//! VR entry, the VR tabs' memory, and the section headers' highlight and style bookkeeping.

pub(crate) mod command;
pub(crate) mod entries;
pub(crate) mod headers;
pub(crate) mod ids;
pub(crate) mod live;
pub(crate) mod page;
pub(crate) mod rows;
pub(crate) mod slider;
pub(crate) mod state;
pub(crate) mod tabs;
