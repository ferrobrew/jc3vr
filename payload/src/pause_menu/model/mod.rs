//! The VR menu's pure logic, testable without the game: the page model, row ids and row encoding,
//! slider mapping, flow steps, the live-value diff, the menu state machine, and command routing.

pub(crate) mod command;
pub(crate) mod flow;
pub(crate) mod ids;
pub(crate) mod live;
pub(crate) mod page;
pub(crate) mod rows;
pub(crate) mod slider;
pub(crate) mod state;
