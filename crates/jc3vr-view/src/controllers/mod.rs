//! The view-side pieces of the controllers: the cockpit transform, the overlay's data model and the
//! inputs its pass reads, the scope's lens frame, and the rig's origin provenance. The controllers
//! themselves live in the layers above (the body, weapons, and the rest) and feed these, and the
//! passes that draw them live in the render layer.

pub mod cockpit;
pub mod lens_frame;
pub mod origin_provenance;
pub mod overlay;
