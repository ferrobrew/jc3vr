//! The gameplay layer's detours, grouped by the engine system they hook: the input tick's look
//! detour, the simulation step, and the online managers the simulation tick runs. The hook
//! plumbing they install through lives in the core crate, the camera detours in the
//! view crate, the character, animation, and other input detours in the body crate, and the
//! equipment detours in the vehicles crate.

pub mod input;
pub mod online;
pub mod sim;
