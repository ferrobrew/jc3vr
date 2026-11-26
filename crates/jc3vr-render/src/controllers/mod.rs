//! The render-side pieces of the controllers: the cockpit transform, the overlay renderer and its
//! primitive store, the scope's lens pass and renderer, and the rig's origin provenance. The
//! controllers themselves live in the gameplay layer and feed these.

pub mod body;
pub mod cockpit;
pub mod overlay;
pub mod weapons;
