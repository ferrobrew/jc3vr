//! The input layer's pure core: the published controller frame and its motion history, the control
//! scheme, the haptic data and mixer, the semantic action vocabulary, and the debug UI's capture
//! policy and published capture state.
//!
//! Nothing here names the game, Win32, or OpenXR: the producers (the VR runtime's input sampling,
//! the native desktop sampler, and the debug UI's renderer) live above and feed it, so the crate
//! builds and tests on any host. It sits directly above the core crate in the layering but depends
//! on no layer crate, which keeps it out of the Windows-only build graph.
//!
//! The public surface is its six modules, which the layers above reach by path:
//!
//! - [`frame`]: the published [`frame::ControllerFrame`] slot, the press and release edge latch,
//!   and the controller vocabulary (hands, buttons, interaction profiles, press thresholds).
//! - [`motion`]: each grip's recent pose history and the release-velocity and strike-speed
//!   estimates built on it.
//! - [`scheme`]: the controllers-versus-gamepad [`scheme::Scheme`] and the rule that picks it.
//! - [`semantic`]: the semantic action sets and each set's actions.
//! - [`haptics`]: the haptic primitives, every cue the mod plays, the per-hand mixer, and the
//!   mixed output slot the VR runtime drains.
//! - [`capture`]: the debug panel's input-ownership policy, the egui event filter, and the
//!   published capture state the input hooks consult.

pub mod capture;
pub mod frame;
pub mod haptics;
pub mod motion;
pub mod scheme;
pub mod semantic;
