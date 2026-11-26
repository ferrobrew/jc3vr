//! The gameplay layer's detours, grouped by the engine system they hook: the character and its
//! pose, the animation graph, the input tick, the equipment, and the simulation step, with the
//! stance height the camera adjust applies. The hook plumbing they install through lives in the
//! core crate, and the camera detours themselves in the view crate.

pub mod animation;
pub mod camera;
pub mod character;
pub mod equipment;
pub mod input;
pub mod sim;
