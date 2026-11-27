//! The render-side pieces of the controllers: the overlay renderer and its pass, the scope's lens
//! pass and renderer, and the pipeline state both save and restore around their draws. The
//! post-draw (`crate::hooks::graphics_engine::engine`) runs the passes itself. The controllers
//! themselves live in the gameplay layer, and the data the passes draw lives in the view layer.

// The pipeline state the overlay and scope lens passes save and restore around their draws.
mod draw_state;
pub mod overlay;
pub mod weapons;
