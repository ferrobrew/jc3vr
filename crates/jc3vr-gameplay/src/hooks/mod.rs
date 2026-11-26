//! The gameplay layer's detours, grouped by the engine system they hook: the input tick's look
//! detour, the equipment, and the simulation step, with the stance height the camera adjust
//! applies. The hook plumbing they install through lives in the core crate, the camera detours
//! themselves in the view crate, and the character, animation, and other input detours in the body
//! crate.

pub mod camera;
pub mod equipment;
pub mod input;
pub mod sim;
