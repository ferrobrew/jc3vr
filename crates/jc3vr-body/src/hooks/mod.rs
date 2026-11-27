//! The body layer's detours, grouped by the engine system they hook: the character and its pose,
//! the animation graph, and the input tick. The payload's composition enables the character and
//! animation libraries directly, and the input detours through the gameplay crate's input library,
//! which adds the look detour beside them.

pub mod animation;
pub mod character;
pub mod input;
