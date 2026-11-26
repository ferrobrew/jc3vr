//! The render-side pieces of the controllers: the overlay renderer and its pass, the scope's lens
//! pass and renderer, and the registration of those passes in the post-draw. The controllers
//! themselves live in the gameplay layer, and the data the passes draw lives in the view layer.

pub mod overlay;
pub mod register_passes;
pub mod weapons;
