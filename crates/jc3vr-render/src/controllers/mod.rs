//! The render-side pieces of the controllers: the overlay renderer and its pass, the scope's lens
//! pass and renderer, and the pipeline state both save and restore around their draws. The
//! post-draw (`crate::hooks::graphics_engine::engine`) runs the passes itself. The controllers
//! themselves live in the layers above (the overlay's placement in the body crate, the scope in the
//! weapons crate), and the data the passes draw lives in the view crate.

mod draw_state;
pub mod overlay;
pub mod scope;
