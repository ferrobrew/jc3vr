//! The VR menu's game side: the detours and the movie access they use. Everything here runs on
//! the UI capture thread, inside the root movie's advance or the pause screen's update.

mod hooks;
mod render;
mod root;
mod session;

pub(crate) use crate::pause_menu::runtime::{hooks::hook_library, session::SESSION};
