//! The grip model's tuning, read from the shipped `tuning/grab.toml` ([`GRAB`]): the fist's take
//! window and speed, the body slots' recall, the low reach, and the markers' sizes, opacities, and
//! colours.

use jc3vr_core::{
    config::engine::{Invalid, positive, range},
    tuning::Tuning,
};
use serde::Deserialize;

jc3vr_core::tuning_file! {
    /// The grip model's tuning file.
    pub static GRAB: GrabTuning = "grab.toml";
}

/// The grip model's tuning file's root.
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GrabTuning {
    pub reach: GrabReachTuning,
    pub markers: MarkersTuning,
}

impl Tuning for GrabTuning {
    fn validate(&self) -> Result<(), Invalid> {
        let r = &self.reach;
        positive("reach.fist_window_s", r.fist_window_s, 5.0)?;
        positive("reach.slow_hand_mps", r.slow_hand_mps, 20.0)?;
        positive("reach.recall_s", r.recall_s, 5.0)?;
        positive("reach.low_reach_m", r.low_reach_m, 1.0)?;
        let m = &self.markers;
        positive("markers.world_size_m", m.world_size_m, 1.0)?;
        positive("markers.min_size_m", m.min_size_m, 1.0)?;
        let shares = [
            ("markers.occluded_alpha", m.occluded_alpha),
            ("markers.idle_alpha", m.idle_alpha),
            ("markers.reach_alpha", m.reach_alpha),
            ("markers.chosen_alpha", m.chosen_alpha),
            ("markers.held_alpha", m.held_alpha),
            ("markers.ring_thickness", m.ring_thickness),
            ("markers.chosen_thickness", m.chosen_thickness),
            ("markers.handle_ring_thickness", m.handle_ring_thickness),
            ("markers.button_ring_thickness", m.button_ring_thickness),
            ("markers.handle_dot_share", m.handle_dot_share),
            ("markers.no_fit_share", m.no_fit_share),
        ];
        for (name, share) in shares {
            range(name, share, 0.0, 1.0)?;
        }
        range("markers.chosen_scale", m.chosen_scale, 0.5, 4.0)?;
        let c = &m.colours;
        let colours = [
            ("markers.colours.no_fit", c.no_fit),
            ("markers.colours.prop", c.prop),
            ("markers.colours.weapon", c.weapon),
            ("markers.colours.body", c.body),
            ("markers.colours.vehicle", c.vehicle),
            ("markers.colours.climb", c.climb),
            ("markers.colours.character", c.character),
            ("markers.colours.explosive", c.explosive),
        ];
        for (name, colour) in colours {
            for (i, channel) in colour.into_iter().enumerate() {
                range(&format!("{name}[{i}]"), channel, 0.0, 1.0)?;
            }
        }
        Ok(())
    }
}

/// What a hand reaches and takes ([`latch`](crate::controllers::grab::latch),
/// [`tick`](crate::controllers::grab::tick), and [`resolve`](crate::controllers::grab::resolve)).
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GrabReachTuning {
    /// How long after closing a fist still takes what the hand comes to however fast it moves,
    /// seconds: a hand that closes a moment before it reaches a handle still takes the handle.
    pub(crate) fist_window_s: f32,
    /// The fastest a hand may move for a closed fist to take what it comes to, metres per second at
    /// the player's scale: a hand laid on a handle is slower, and a punch is far faster.
    pub(crate) slow_hand_mps: f32,
    /// How long a hand that leaves a body slot's reach still takes from it, seconds.
    pub(crate) recall_s: f32,
    /// How far below the drawn hand the controller must be for the low reach to reach from it,
    /// metres: more than the drawn hand strays from its controller in ordinary reaches.
    pub(crate) low_reach_m: f32,
}

/// The grab markers ([`markers`](crate::controllers::grab::markers)). Opacities and ring
/// thicknesses are shares in `0..1`; a ring's thickness is a share of its radius.
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MarkersTuning {
    /// The size of the marker on something in the world, metres at the player's scale.
    pub world_size_m: f32,
    /// The smallest a marker is drawn, metres.
    pub(crate) min_size_m: f32,
    /// The opacity of a marker's part behind the world.
    pub occluded_alpha: f32,
    /// The opacity of a marker no hand is within reach of: faint, so that it shows where the target
    /// is without drawing the eye.
    pub(crate) idle_alpha: f32,
    /// The opacity of a marker a hand is within reach of.
    pub(crate) reach_alpha: f32,
    /// The opacity of the marker a squeeze would take.
    pub(crate) chosen_alpha: f32,
    /// The opacity of a marker a hand holds.
    pub(crate) held_alpha: f32,
    /// How much larger the marker a squeeze would take is drawn.
    pub(crate) chosen_scale: f32,
    /// A ring marker's thickness.
    pub ring_thickness: f32,
    /// A ring marker's thickness while chosen or held.
    pub(crate) chosen_thickness: f32,
    /// The thickness of the ring round a handle's dot while a hand reaches or holds it.
    pub handle_ring_thickness: f32,
    /// The thickness of a button's ring while a hand reaches it.
    pub(crate) button_ring_thickness: f32,
    /// The share of a handle's marker its centre dot spans.
    pub(crate) handle_dot_share: f32,
    /// The share of its size a no-fit marker's disc spans.
    pub(crate) no_fit_share: f32,
    pub colours: MarkerColours,
}

/// The markers' colours, RGB in `0..1`: one for each kind of target, and one for a place
/// that does not take what the hand holds.
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MarkerColours {
    pub no_fit: [f32; 3],
    pub prop: [f32; 3],
    pub weapon: [f32; 3],
    pub body: [f32; 3],
    pub vehicle: [f32; 3],
    pub climb: [f32; 3],
    pub character: [f32; 3],
    pub explosive: [f32; 3],
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_shipped_file_loads_and_validates() {
        assert_eq!(GRAB.get().validate(), Ok(()));
    }
}
