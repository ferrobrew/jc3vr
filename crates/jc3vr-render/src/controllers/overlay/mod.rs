//! The overlay's render half: the primitive vocabulary, the per-frame store the producers push
//! into, the resolution of primitives against the frame's hand poses, the per-eye view, the GPU
//! renderer, and the post-draw pass that draws them.

pub mod pass;
pub mod primitive;
pub mod renderer;
pub mod resolve;
pub mod store;
pub mod view;
