//! The finger curls' fixed tuning: the curl-to-bend mapping, the splay, the smoothing, and the
//! grip poses' blend.

/// The finger curls' tuning, fixed at [`FINGERS`].
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct FingerTuning {
    /// The bend each finger takes at its open and closed curl, as a share of its full joint
    /// bends, indexed by [`Finger::index`](crate::controllers::fingers::curl::Finger::index).
    pub curl_ranges: [CurlRange; 5],
    /// The full bend of a finger's proximal, middle, and distal joints from straight, degrees: the
    /// bend at a range share of one.
    pub finger_bend_deg: [f32; 3],
    /// The full bend of the thumb's base, middle, and distal joints across the palm, degrees.
    pub thumb_bend_deg: [f32; 3],
    /// Each joint's bend at a share of zero, degrees, indexed by finger and then from the root
    /// joint outward: a correction to the straight finger built from the animated hand, positive
    /// toward the palm.
    pub rest_offset_deg: [[f32; 3]; 5],
    /// The open thumb's turn from the fingers' direction toward the thumb side, in the palm's
    /// plane, degrees.
    pub thumb_spread_deg: f32,
    /// The share of the tracked finger splay applied (hand tracking only).
    pub splay_scale: f32,
    /// The largest splay applied either way, degrees.
    pub max_splay_deg: f32,
    /// The time constant the curls follow their source with, seconds. The estimate jumps when a
    /// touch sensor changes, and the smoothing turns the jump into a quick motion.
    pub smoothing_s: f32,
    /// The grip poses: the fingers wrap what the hand holds.
    pub grip: GripPoseTuning,
}

/// The grip poses' tuning, held in [`FingerTuning::grip`].
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct GripPoseTuning {
    /// The pose's share at the lightest grip that holds, `0..=1`; it rises to the full pose at
    /// [`firm_squeeze`](Self::firm_squeeze).
    pub light_share: f32,
    /// The grip's squeeze at which the pose is fully on, `0..=1`.
    pub firm_squeeze: f32,
    /// The time constant the pose's share follows the grip with, seconds, so that taking hold
    /// closes the fingers and letting go opens them over a short motion.
    pub blend_s: f32,
    /// The thumb's largest turn out of the palm's plane toward the palm side, at full opposition,
    /// degrees.
    pub max_opposition_deg: f32,
}

/// The bend a finger takes at its open (curl 0) and closed (curl 1) ends, as a share of its full
/// joint bends from straight. The bend replaces the animated one rather than adding to it.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct CurlRange {
    pub open: f32,
    pub closed: f32,
}

impl CurlRange {
    /// The bend share at `curl`.
    pub fn at(self, curl: f32) -> f32 {
        self.open + (self.closed - self.open) * curl.clamp(0.0, 1.0)
    }
}

/// The finger curls' tuning: every finger bends the whole way from straight to its full bend, on
/// every controller.
pub const FINGERS: FingerTuning = FingerTuning {
    curl_ranges: [CurlRange {
        open: 0.0,
        closed: 1.0,
    }; 5],
    finger_bend_deg: [85.0, 100.0, 70.0],
    thumb_bend_deg: [25.0, 45.0, 60.0],
    rest_offset_deg: [[0.0; 3]; 5],
    thumb_spread_deg: 40.0,
    splay_scale: 1.0,
    max_splay_deg: 20.0,
    smoothing_s: 0.04,
    grip: GripPoseTuning {
        light_share: 0.6,
        firm_squeeze: 0.85,
        blend_s: 0.06,
        max_opposition_deg: 55.0,
    },
};
