//! The input layer's pure core: the published controller frame and its motion history, the control
//! scheme, the haptic data and mixer, the semantic action vocabulary, and the debug UI's capture
//! policy and published capture state.
//!
//! Nothing here names the game, Win32, or OpenXR: the producers (the VR runtime's input sampling,
//! the native desktop sampler, and the debug UI's renderer) live above and feed it, so the module
//! builds and tests on any host.

pub(crate) mod capture;
pub(crate) mod frame;
pub(crate) mod haptics;
pub(crate) mod motion;
pub(crate) mod scheme;
pub(crate) mod semantic;
