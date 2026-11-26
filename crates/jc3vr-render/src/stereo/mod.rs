//! The render layer's stereo rendering: the single-pass path, its switches, the sun's shadow
//! cascades, the terrain preparation, and the COM detours the eject unpatches. The frame state they
//! read (the stereo state, the dispatch, the engine context, the layout, and the stereo settings)
//! lives in the view crate's `stereo` module.

pub mod register_unpatch;
pub mod single_pass;
pub mod sun_shadows;
pub mod switches;
pub(crate) mod terrain_prep;
