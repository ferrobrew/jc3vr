//! The overlay's data model: the primitive vocabulary, the per-frame store the producers push into,
//! the resolution of primitives against the frame's hand poses, and the inputs the render layer's
//! pass reads from the controllers.

pub mod inputs;
pub mod primitive;
pub mod resolve;
pub mod store;
