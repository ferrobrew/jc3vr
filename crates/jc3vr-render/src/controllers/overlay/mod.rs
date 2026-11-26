//! The overlay's render half: the primitive vocabulary, the per-frame store the producers push
//! into, the resolution of primitives against the frame's hand poses, the per-eye view, the GPU
//! renderer, the post-draw pass that draws them, and the inputs that pass reads from the
//! controllers.

pub mod inputs;
pub mod pass;
pub mod primitive;
pub mod renderer;
pub mod resolve;
pub mod store;
pub mod view;
