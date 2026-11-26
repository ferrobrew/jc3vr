//! The held grenade's pin, pulled by the other hand or by the teeth. Pure.
//!
//! A grenade in one hand with its pin in offers the pin to the other hand as a grip target
//! ([`GrabVolume::GrenadePin`]) at the top of the fuse head, marked always so that the player
//! knows that a squeeze there pulls the pin, while a squeeze anywhere else on the grenade takes it
//! into that hand. The pin ranks over the grenade itself ([`Tier::Handle`]), so a hand within the
//! pin's reach takes the pin however near the grenade's body is. When the other hand is occupied,
//! the grenade brought up to the mouth and held there for a moment pulls the pin by the teeth
//! ([`TeethPull`]). A trigger press on the holding hand pulls it as well, whatever the other hand
//! does. The design is in `docs/mod/input/grenades.md`.
//!
//! [`Tier::Handle`]: crate::controllers::grab::Tier::Handle

use glam::Vec3;

use crate::controllers::{
    frame::{Hand, Pose},
    grab::{Candidate, GrabVolume},
    weapons::grenade::tuning::GrenadeTuning,
};

/// How a pin came out, for the log.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub(crate) enum PinSource {
    /// The holding hand's trigger.
    Trigger,
    /// The other hand's squeeze on the pin.
    OtherHand,
    /// The teeth, with the grenade held at the mouth.
    Teeth,
}

impl PinSource {
    pub(crate) const fn label(self) -> &'static str {
        match self {
            PinSource::Trigger => "the trigger",
            PinSource::OtherHand => "the other hand",
            PinSource::Teeth => "the teeth",
        }
    }
}

/// The pin's grab point in the cockpit frame, for a grenade held in `hand` whose grip is `grip`
/// (cockpit frame) at the VR world scale `world_scale`: the top of the fuse head, where the ring
/// is. `None` for a degenerate scale or pose.
pub(crate) fn pin_point(
    grip: Pose,
    hand: Hand,
    world_scale: f32,
    tuning: &GrenadeTuning,
) -> Option<Vec3> {
    if !world_scale.is_finite() || world_scale <= 0.0 {
        return None;
    }
    let hold = tuning.hold.in_hand(hand);
    // The grenade's model is drawn at the game's size, which the cockpit frame divides by the scale.
    let in_grip = (hold.position + hold.orientation * PIN_IN_MODEL_M) / world_scale;
    let point = grip.position + grip.orientation * in_grip;
    point.is_finite().then_some(point)
}

/// The pin target for the hand other than `holder`, the grenade's pin at `pin` (cockpit frame).
pub(crate) fn candidate(holder: Hand, pin: Vec3) -> Candidate {
    Candidate {
        volume: GrabVolume::GrenadePin,
        centre: pin,
        radius: PIN_RADIUS_M,
        hand: Some(holder.other()),
    }
}

/// Where the mouth is for the head `head` (the eye midpoint, cockpit frame): below the eyes and a
/// little in front of them.
pub(crate) fn mouth_point(head: Pose) -> Vec3 {
    head.position + head.orientation * MOUTH_IN_HEAD_M
}

/// Whether the pin at `pin` is at the mouth at `mouth`, both in the cockpit frame.
pub(crate) fn at_mouth(pin: Vec3, mouth: Vec3) -> bool {
    pin.is_finite() && mouth.is_finite() && pin.distance(mouth) <= MOUTH_RADIUS_M
}

/// The teeth pull for one hand's grenade: the grenade's pin held at the mouth for
/// [`TEETH_DWELL_S`] while the other hand is occupied pulls the pin, once per visit to the mouth.
/// A grenade only passing the face, or one brought there while the other hand is free to pull the
/// pin itself, pulls nothing.
#[derive(Debug, Copy, Clone, PartialEq)]
pub(crate) struct TeethPull {
    /// How long the pin has been at the mouth with the other hand occupied, seconds.
    held_s: f32,
    /// Whether this visit to the mouth already pulled, so a pin left there does not pull again.
    pulled: bool,
}

/// One tick of [`TeethPull::step`].
#[derive(Debug, Copy, Clone, PartialEq)]
pub(crate) struct TeethTick {
    /// Whether the hand holds a grenade whose pin is in.
    pub pin_in: bool,
    /// Whether the pin is at the mouth ([`at_mouth`]).
    pub at_mouth: bool,
    /// Whether the other hand is occupied: it holds something, its grip is closed on something, or
    /// it is not tracked.
    pub other_occupied: bool,
    /// The tick's length, seconds.
    pub dt: f32,
}

impl TeethPull {
    /// Nothing at the mouth yet.
    pub(crate) const NEW: Self = Self {
        held_s: 0.0,
        pulled: false,
    };

    /// Advance one tick. `true` on the tick the teeth pull the pin.
    pub(crate) fn step(&mut self, tick: TeethTick) -> bool {
        if !tick.at_mouth {
            *self = TeethPull::NEW;
            return false;
        }
        if !tick.pin_in || !tick.other_occupied || self.pulled {
            self.held_s = 0.0;
            return false;
        }
        if tick.dt.is_finite() && tick.dt > 0.0 {
            self.held_s += tick.dt;
        }
        if self.held_s >= TEETH_DWELL_S {
            self.pulled = true;
            self.held_s = 0.0;
            return true;
        }
        false
    }
}

/// The pin's reach, metres at the player's scale: larger than the ring, so that a hand reaching for
/// the top of the grenade takes the pin without aiming for it, and smaller than the grenade's own
/// reach ([`crate::controllers::weapons::hold::HANDLE_RADIUS_M`]), so that a hand closing on the
/// grenade's body still takes the grenade.
pub(crate) const PIN_RADIUS_M: f32 = 0.05;
/// How long the pin stays at the mouth before the teeth pull it, seconds: longer than a grenade
/// passing the face takes.
pub(crate) const TEETH_DWELL_S: f32 = 0.15;

/// The pin in the grenade model's frame, metres: on the model's +Y axis, at the top of the fuse
/// head (see [`crate::controllers::weapons::grenade::tuning::HOLD`] for the model's axes).
const PIN_IN_MODEL_M: Vec3 = Vec3::new(0.0, 0.065, 0.0);
/// The mouth in the head's frame (the eye midpoint, -Z forward, +Y up), metres.
const MOUTH_IN_HEAD_M: Vec3 = Vec3::new(0.0, -0.08, -0.07);
/// How near the mouth the pin must come for the teeth to take it, metres.
const MOUTH_RADIUS_M: f32 = 0.08;

#[cfg(test)]
mod tests {
    use glam::Quat;

    use super::*;
    use crate::controllers::{
        grab::grab_volume,
        weapons::{grenade::tuning::GRENADE, hold::HANDLE_RADIUS_M},
    };

    fn upright() -> Pose {
        Pose {
            position: Vec3::new(0.2, -0.4, -0.3),
            orientation: Quat::IDENTITY,
        }
    }

    #[test]
    fn the_pin_sits_above_the_fist() {
        for hand in Hand::BOTH {
            let grip = upright();
            let pin = pin_point(grip, hand, 1.0, &GRENADE).unwrap();
            // Up the finger tube, the grip's -Z, out of the top of the fist.
            let up = (pin - grip.position).dot(Vec3::NEG_Z);
            assert!(up > 0.05, "{hand:?}: {up}");
            // Within the grenade's own reach, so the pin is on the grenade the hand holds.
            assert!(pin.distance(grip.position) < HANDLE_RADIUS_M);
        }
    }

    #[test]
    fn the_pin_shrinks_with_the_world_scale() {
        let grip = upright();
        let at_one = pin_point(grip, Hand::Right, 1.0, &GRENADE).unwrap();
        let at_two = pin_point(grip, Hand::Right, 2.0, &GRENADE).unwrap();
        let offset_one = at_one - grip.position;
        let offset_two = at_two - grip.position;
        assert!((offset_one * 0.5).abs_diff_eq(offset_two, 1e-6));
    }

    #[test]
    fn a_degenerate_scale_places_no_pin() {
        for scale in [0.0, -1.0, f32::NAN, f32::INFINITY] {
            assert_eq!(pin_point(upright(), Hand::Left, scale, &GRENADE), None);
        }
    }

    #[test]
    fn the_pin_outranks_the_grenade_it_is_on() {
        let holder = Hand::Right;
        let taker = holder.other();
        let grip = upright();
        let pin = pin_point(grip, holder, 1.0, &GRENADE).unwrap();
        let candidates = [
            Candidate {
                volume: GrabVolume::Handle,
                centre: grip.position,
                radius: HANDLE_RADIUS_M,
                hand: Some(taker),
            },
            candidate(holder, pin),
        ];
        // At the pin, and even nearer the grenade's grip than the pin, within the pin's reach.
        let toward_grip = pin + (grip.position - pin).normalize() * (PIN_RADIUS_M * 0.9);
        assert!(toward_grip.distance(grip.position) < toward_grip.distance(pin));
        for point in [pin, toward_grip] {
            assert_eq!(
                grab_volume(point, taker, &candidates),
                Some(GrabVolume::GrenadePin)
            );
        }
        // Beside the grenade's body, past the pin's reach: the grenade changes hands.
        let beside = grip.position + Vec3::X * 0.06;
        assert!(beside.distance(pin) > PIN_RADIUS_M);
        assert_eq!(
            grab_volume(beside, taker, &candidates),
            Some(GrabVolume::Handle)
        );
        // The holding hand never takes its own grenade's pin.
        assert_eq!(grab_volume(pin, holder, &candidates), None);
    }

    #[test]
    fn the_mouth_is_below_and_in_front_of_the_eyes() {
        let head = Pose {
            position: Vec3::new(0.0, 1.7, 0.0),
            orientation: Quat::from_rotation_y(std::f32::consts::FRAC_PI_2),
        };
        let mouth = mouth_point(head);
        assert!(mouth.y < head.position.y);
        // Turned to the left, forward is -X.
        assert!(mouth.x < head.position.x);
        assert!(at_mouth(mouth, mouth));
        assert!(!at_mouth(mouth + Vec3::Y * 0.2, mouth));
        assert!(!at_mouth(Vec3::NAN, mouth));
    }

    fn tick(at_mouth: bool, other_occupied: bool, dt: f32) -> TeethTick {
        TeethTick {
            pin_in: true,
            at_mouth,
            other_occupied,
            dt,
        }
    }

    #[test]
    fn the_teeth_pull_after_a_moment_at_the_mouth() {
        let mut teeth = TeethPull::NEW;
        assert!(!teeth.step(tick(true, true, TEETH_DWELL_S * 0.5)));
        assert!(teeth.step(tick(true, true, TEETH_DWELL_S * 0.6)));
        // Left there, the pin is not pulled again.
        assert!(!teeth.step(tick(true, true, 1.0)));
        // Away and back, it can be.
        assert!(!teeth.step(tick(false, true, 0.1)));
        assert!(teeth.step(tick(true, true, TEETH_DWELL_S)));
    }

    #[test]
    fn a_grenade_passing_the_face_pulls_nothing() {
        let mut teeth = TeethPull::NEW;
        assert!(!teeth.step(tick(true, true, TEETH_DWELL_S * 0.6)));
        assert!(!teeth.step(tick(false, true, 0.01)));
        assert!(!teeth.step(tick(true, true, TEETH_DWELL_S * 0.6)));
    }

    #[test]
    fn the_teeth_wait_while_the_other_hand_is_free() {
        let mut teeth = TeethPull::NEW;
        assert!(!teeth.step(tick(true, false, 1.0)));
        // Its dwell starts once the other hand is occupied.
        assert!(!teeth.step(tick(true, true, TEETH_DWELL_S * 0.5)));
        assert!(teeth.step(tick(true, true, TEETH_DWELL_S * 0.6)));
    }

    #[test]
    fn the_teeth_take_no_pin_that_is_out_or_a_tick_without_time() {
        let mut teeth = TeethPull::NEW;
        let out = TeethTick {
            pin_in: false,
            ..tick(true, true, 1.0)
        };
        assert!(!teeth.step(out));
        for dt in [f32::NAN, -1.0, 0.0] {
            assert!(!teeth.step(tick(true, true, dt)));
        }
    }
}
