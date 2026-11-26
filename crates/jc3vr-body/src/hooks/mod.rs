//! The body layer's detours, grouped by the engine system they hook: the character and its pose,
//! the animation graph, and the input tick. The gameplay crate composes their hook libraries into
//! the payload's, with the input tick's look detour among the input ones.

pub mod animation;
pub mod character;
pub mod input;
