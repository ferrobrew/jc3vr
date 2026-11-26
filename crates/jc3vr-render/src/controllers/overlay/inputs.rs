//! What the world-space overlay's draw reads from the controllers, which they register at startup:
//! whether the overlay is on and how it draws, and the hands' render-time poses its hand-anchored
//! primitives ride. Until they do, the overlay draws nothing.

use std::sync::OnceLock;

use crate::controllers::overlay::resolve::HandPoses;

/// How the overlay draws, while it is on.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct DrawSettings {
    /// The thinnest a line is drawn, in pixels.
    pub min_line_px: f32,
    /// How far behind the scene's depth a primitive still draws, metres.
    pub depth_tolerance_m: f32,
    /// How far behind the scene's depth a primitive still draws, as a fraction of its distance.
    pub depth_tolerance_rel: f32,
}

/// What the overlay's draw reads from the controllers.
#[derive(Clone, Copy)]
pub struct OverlayInputs {
    /// How the overlay draws, or `None` while it is off.
    pub settings: fn() -> Option<DrawSettings>,
    /// Each hand's world poses as it is drawn this frame, by hand index.
    pub hand_poses: fn() -> [Option<HandPoses>; 2],
}

/// Register what the overlay's draw reads. The first registration stands.
pub fn register_overlay_inputs(inputs: OverlayInputs) {
    if INPUTS.set(inputs).is_err() {
        tracing::warn!("overlay: inputs are already registered; the new ones are ignored");
    }
}

/// The registered inputs, or `None` before the controllers register them.
pub fn registered() -> Option<&'static OverlayInputs> {
    INPUTS.get()
}

static INPUTS: OnceLock<OverlayInputs> = OnceLock::new();
