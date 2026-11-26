//! The payload's own detours: the game's frame entry points, which drive the per-frame update and
//! the render dispatch ([`game`]), and the window procedure, which takes the mod's hotkeys and the
//! desktop input for the debug UI ([`wndproc`]). Every other detour lives in the layer crate that
//! owns its engine system.

pub mod game;
pub mod wndproc;
