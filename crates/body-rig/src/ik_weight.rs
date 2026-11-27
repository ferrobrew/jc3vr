//! The IK body's weight per state, per channel, and its blend between them
//! (`docs/mod/body/ik-body.md`, "Per state").
//!
//! The IK pose and the game's clip share the body by channel: the pelvis's position, the pelvis's
//! orientation, the spine, the planted feet, and the airborne legs. Each state of the character
//! asks for a weight on each ([`IkState::weights`]); [`IkBlend`] carries the weights there at a
//! constant rate, so every hand-over between the IK and a clip takes the blend's time, whichever
//! way it goes and however far.
//!
//! A partial weight mixes rotations ([`blend_rotation`]), never model-space positions: mixing
//! positions bone by bone shortens the segments and moves the chain's ends mid-blend.

use glam::Quat;

use crate::math::rotation::shortest;

/// What the character is doing, as the IK body's table reads it.
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Copy, Clone, PartialEq, Eq, Default)]
pub enum IkState {
    /// Standing or crouching on foot, physical steps and turns included: the IK owns the body.
    Idle,
    /// The stick's walk, jog, or sprint, or a physical walk handed to the game's walk: the clip,
    /// with the pelvis's orientation and the spine drawn part of the way to the IK pose.
    Walk,
    /// A jump's takeoff, a landing, a hard landing: the clip.
    Clip,
    /// In the air from a jump or a fall: the IK pelvis under the head, the legs relaxed toward a
    /// light air pose.
    Airborne,
    /// Hanging from the hands: the torso stays the procedural body's on the clip's hips, and each
    /// foot braces on the face in front of it or hangs relaxed.
    Climb,
    /// Everything the table leaves to the game: seats, the parachute, the wingsuit, swimming,
    /// traversals, ragdolls, get-ups, the grapple's ledge climb, and the firm ragdoll.
    #[default]
    Unchanged,
}

impl IkState {
    pub fn label(self) -> &'static str {
        match self {
            IkState::Idle => "idle",
            IkState::Walk => "walk",
            IkState::Clip => "clip",
            IkState::Airborne => "airborne",
            IkState::Climb => "climb",
            IkState::Unchanged => "unchanged",
        }
    }

    /// The weights the state asks for, with `walk` the share of the IK pose on the walk.
    pub fn weights(self, walk: f32) -> IkWeights {
        let walk = if walk.is_finite() {
            walk.clamp(0.0, 1.0)
        } else {
            0.0
        };
        match self {
            IkState::Idle => IkWeights {
                pelvis_position: 1.0,
                pelvis_orientation: 1.0,
                spine: 1.0,
                feet: 1.0,
                air_legs: 0.0,
            },
            IkState::Walk => IkWeights {
                pelvis_orientation: walk,
                spine: walk,
                ..IkWeights::ZERO
            },
            IkState::Airborne => IkWeights {
                pelvis_position: 1.0,
                pelvis_orientation: 1.0,
                spine: 1.0,
                feet: 0.0,
                air_legs: 1.0,
            },
            IkState::Climb => IkWeights {
                feet: 1.0,
                air_legs: 1.0,
                ..IkWeights::ZERO
            },
            IkState::Clip | IkState::Unchanged => IkWeights::ZERO,
        }
    }
}

/// The IK pose's share of each channel against the clip, `0..=1`.
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Copy, Clone, PartialEq, Default)]
pub struct IkWeights {
    pub pelvis_position: f32,
    pub pelvis_orientation: f32,
    pub spine: f32,
    /// The planted feet's ankle and toe targets, and the braced feet's while climbing.
    pub feet: f32,
    /// The legs posed in the air.
    pub air_legs: f32,
}

impl IkWeights {
    pub const ZERO: Self = Self {
        pelvis_position: 0.0,
        pelvis_orientation: 0.0,
        spine: 0.0,
        feet: 0.0,
        air_legs: 0.0,
    };

    /// The largest of the channels: whether the IK body acts at all.
    pub fn max(&self) -> f32 {
        self.channels().into_iter().fold(0.0, f32::max)
    }

    /// Every channel scaled by `share`.
    pub fn scaled(&self, share: f32) -> Self {
        let share = if share.is_finite() {
            share.clamp(0.0, 1.0)
        } else {
            0.0
        };
        let [a, b, c, d, e] = self.channels().map(|w| w * share);
        Self::from_channels([a, b, c, d, e])
    }

    fn channels(&self) -> [f32; 5] {
        [
            self.pelvis_position,
            self.pelvis_orientation,
            self.spine,
            self.feet,
            self.air_legs,
        ]
    }

    fn from_channels(
        [pelvis_position, pelvis_orientation, spine, feet, air_legs]: [f32; 5],
    ) -> Self {
        Self {
            pelvis_position,
            pelvis_orientation,
            spine,
            feet,
            air_legs,
        }
    }
}

/// The weights on their way to the state's, each channel at a constant rate.
#[derive(Debug, Copy, Clone, PartialEq, Default)]
pub struct IkBlend {
    current: IkWeights,
}

impl IkBlend {
    pub const fn new() -> Self {
        Self {
            current: IkWeights::ZERO,
        }
    }

    pub fn weights(&self) -> IkWeights {
        self.current
    }

    /// Drop every channel to nothing at once.
    pub fn reset(&mut self) {
        self.current = IkWeights::ZERO;
    }

    /// Carry each channel toward `target` by `dt` over `blend_s`, the time a full hand-over takes,
    /// and return the weights. A blend time that is not positive goes at once.
    pub fn step(&mut self, target: IkWeights, dt: f32, blend_s: f32) -> IkWeights {
        let step = if blend_s.is_finite() && blend_s > 0.0 {
            if dt.is_finite() {
                dt.max(0.0) / blend_s
            } else {
                0.0
            }
        } else {
            1.0
        };
        let goal = target.channels();
        let moved = self.current.channels();
        let mut next = [0.0; 5];
        for i in 0..5 {
            let to = if goal[i].is_finite() {
                goal[i].clamp(0.0, 1.0)
            } else {
                0.0
            };
            next[i] = (moved[i] + (to - moved[i]).clamp(-step, step)).clamp(0.0, 1.0);
        }
        self.current = IkWeights::from_channels(next);
        self.current
    }
}

/// `animated` turned toward `ik` by `weight`, the short way round.
pub fn blend_rotation(animated: Quat, ik: Quat, weight: f32) -> Quat {
    let weight = if weight.is_finite() {
        weight.clamp(0.0, 1.0)
    } else {
        0.0
    };
    if !animated.is_finite() || !ik.is_finite() {
        return animated;
    }
    let animated = shortest(animated);
    let mut ik = ik.normalize();
    if animated.dot(ik) < 0.0 {
        ik = -ik;
    }
    animated.slerp(ik, weight).normalize()
}

#[cfg(test)]
mod tests {
    use super::*;

    const DT: f32 = 1.0 / 60.0;

    #[test]
    fn idle_gives_the_ik_everything_and_the_walk_only_the_pelvis_turn_and_the_spine() {
        let idle = IkState::Idle.weights(0.2);
        assert_eq!(idle.pelvis_position, 1.0);
        assert_eq!(idle.feet, 1.0);
        let walk = IkState::Walk.weights(0.2);
        assert_eq!(walk.pelvis_orientation, 0.2);
        assert_eq!(walk.spine, 0.2);
        // Never the feet, never the pelvis's position.
        assert_eq!(walk.feet, 0.0);
        assert_eq!(walk.pelvis_position, 0.0);
        assert_eq!(IkState::Clip.weights(0.2), IkWeights::ZERO);
        assert_eq!(IkState::Unchanged.weights(0.2), IkWeights::ZERO);
        let climb = IkState::Climb.weights(0.2);
        assert_eq!(
            (
                climb.pelvis_position,
                climb.spine,
                climb.feet,
                climb.air_legs
            ),
            (0.0, 0.0, 1.0, 1.0)
        );
        let air = IkState::Airborne.weights(0.2);
        assert_eq!(air.air_legs, 1.0);
        assert_eq!(air.feet, 0.0);
        assert_eq!(IkState::Walk.weights(f32::NAN).pelvis_orientation, 0.0);
    }

    #[test]
    fn a_hand_over_takes_the_blend_time_either_way() {
        let mut blend = IkBlend::new();
        let blend_s = 0.25;
        let mut frames = 0;
        while blend.weights().feet < 1.0 {
            blend.step(IkState::Idle.weights(0.2), DT, blend_s);
            frames += 1;
            assert!(frames < 100);
        }
        assert_eq!(frames, (blend_s / DT).ceil() as usize);
        // To the walk: the feet go out at the same rate and the pelvis's turn stops at the walk's.
        let mut previous = blend.weights();
        for _ in 0..30 {
            let w = blend.step(IkState::Walk.weights(0.2), DT, blend_s);
            assert!(previous.feet - w.feet <= DT / blend_s + 1e-6);
            previous = w;
        }
        assert_eq!(previous.feet, 0.0);
        assert!((previous.pelvis_orientation - 0.2).abs() < 1e-6);
        let mut instant = IkBlend::new();
        assert_eq!(instant.step(IkState::Idle.weights(0.2), DT, 0.0).feet, 1.0);
    }

    #[test]
    fn the_rotation_blend_goes_the_short_way() {
        let a = Quat::from_rotation_y(0.2);
        let b = -Quat::from_rotation_y(0.6);
        let mid = blend_rotation(a, b, 0.5);
        assert!(mid.angle_between(Quat::from_rotation_y(0.4)) < 1e-4);
        assert_eq!(blend_rotation(a, b, 0.0), a);
        assert!(blend_rotation(a, b, 1.0).angle_between(b) < 1e-4);
    }
}
