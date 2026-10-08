//! A hand's target shape for a tick, pure: its measured curls, or what it holds wrapped by the
//! grip pose, by the grip poses' settings.

use crate::controllers::{
    fingers::{
        config::FingersConfig,
        curl::{CurlSource, FingerCurls},
        grip::{
            Firmness, Grip, GripSource,
            blend::{self, BlendInputs},
            catalogue::GripPose,
        },
        pose::{ALL_FINGERS, HandShape, TRIGGER_FINGER_ONLY},
    },
    frame::HandInputs,
};

/// What a hand holds and how its fingers take it, for the readout.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct GripReport {
    pub source: GripSource,
    pub pose: GripPose,
    /// The pose's share, after its easing.
    pub share: f32,
    /// How many fingers the contact fit placed.
    pub fitted: usize,
    /// How the fingers take the hold.
    pub mode: GripMode,
}

/// How a hand's fingers take what it holds.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum GripMode {
    /// The grip pose wraps it.
    Posed,
    /// The hand has let go, and the fingers open back to the measured curls.
    Releasing,
    /// A weapon the game's grip wraps, with only the trigger finger driven (or every finger, with
    /// `drive_all_when_holding`).
    GameGrip,
    /// The grip poses are off: a sticky hold closes the hand into a fist, and any other keeps the
    /// measured curls.
    PosesOff,
}

impl GripMode {
    pub const fn label(self) -> &'static str {
        match self {
            GripMode::Posed => "posed",
            GripMode::Releasing => "releasing",
            GripMode::GameGrip => "game's grip",
            GripMode::PosesOff => "poses off",
        }
    }
}

/// What a hand's target reads.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct TargetInputs<'a> {
    pub config: &'a FingersConfig,
    /// The grip the hand's sources declared.
    pub declared: Option<Grip>,
    pub measured: &'a FingerCurls,
    pub source: CurlSource,
    pub inputs: &'a HandInputs,
    /// Seconds since the last tick.
    pub dt: f32,
}

/// A hand's grip across ticks: the pose's eased share and the grip it was last eased toward.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct GripMemory {
    pub share: f32,
    pub last: Option<Grip>,
}

impl GripMemory {
    pub const EMPTY: GripMemory = GripMemory {
        share: 0.0,
        last: None,
    };
}

/// A hand's target for a tick.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct HandTarget {
    pub shape: HandShape,
    pub driven: [bool; 5],
    /// Whether the hand holds a weapon the game's grip wraps.
    pub holding: bool,
    pub grip: Option<GripReport>,
}

/// The hand's target from its inputs, easing `memory` along.
pub fn hand_target(inputs: &TargetInputs, memory: &mut GripMemory) -> HandTarget {
    let config = inputs.config;
    let measured = HandShape::of_curls(inputs.measured, config);
    let report = |grip: Grip, share: f32, mode: GripMode| GripReport {
        source: grip.source,
        pose: grip.pose,
        share,
        fitted: 0,
        mode,
    };
    if let Some(grip) = inputs.declared
        && grip.source.is_weapon()
        && !(config.grip.enabled && config.grip.weapons)
    {
        *memory = GripMemory::EMPTY;
        return HandTarget {
            shape: measured,
            driven: if config.drive_all_when_holding {
                ALL_FINGERS
            } else {
                TRIGGER_FINGER_ONLY
            },
            holding: true,
            grip: Some(report(grip, 0.0, GripMode::GameGrip)),
        };
    }
    if !config.grip.enabled {
        *memory = GripMemory::EMPTY;
        let fist = inputs
            .declared
            .is_some_and(|g| g.firmness == Firmness::Held);
        return HandTarget {
            shape: if fist {
                HandShape {
                    curl: [1.0; 5],
                    splay: [0.0; 5],
                    ..measured
                }
            } else {
                measured
            },
            driven: ALL_FINGERS,
            holding: false,
            grip: inputs
                .declared
                .map(|g| report(g, f32::from(u8::from(fist)), GripMode::PosesOff)),
        };
    }

    let wanted = inputs.declared.map_or(0.0, |grip| {
        blend::firmness_share(grip.firmness, inputs.inputs.squeeze, &config.grip) * grip.share
    });
    if inputs.declared.is_some() {
        memory.last = inputs.declared;
    }
    memory.share = blend::follow_share(memory.share, wanted, inputs.dt, config.grip.blend_s);
    if inputs.declared.is_none() && memory.share <= RELEASED_SHARE {
        *memory = GripMemory::EMPTY;
    }
    let Some(grip) = memory.last else {
        return HandTarget {
            shape: measured,
            driven: ALL_FINGERS,
            holding: false,
            grip: None,
        };
    };
    let fit = if config.grip.contact_fit {
        grip.fit
    } else {
        [None; 5]
    };
    let pose = grip.pose.shape();
    let shape = blend::blend(
        &BlendInputs {
            measured: inputs.measured,
            source: inputs.source,
            trigger: inputs.inputs.trigger,
            trigger_touch: inputs.inputs.trigger_touch,
            pose: &pose,
            fit,
            share: memory.share,
        },
        config,
    );
    let mode = if inputs.declared.is_some() {
        GripMode::Posed
    } else {
        GripMode::Releasing
    };
    HandTarget {
        shape,
        driven: ALL_FINGERS,
        holding: false,
        grip: Some(GripReport {
            fitted: fit.iter().flatten().count(),
            ..report(grip, memory.share, mode)
        }),
    }
}

/// The share below which a released pose has opened fully.
const RELEASED_SHARE: f32 = 1e-3;

#[cfg(test)]
mod tests {
    use super::*;

    fn inputs<'a>(
        config: &'a FingersConfig,
        declared: Option<Grip>,
        measured: &'a FingerCurls,
        hand: &'a HandInputs,
    ) -> TargetInputs<'a> {
        TargetInputs {
            config,
            declared,
            measured,
            source: CurlSource::Estimate,
            inputs: hand,
            dt: 0.011,
        }
    }

    #[test]
    fn a_free_hand_keeps_its_measured_curls() {
        let config = FingersConfig::new();
        let measured = FingerCurls {
            curl: [0.3; 5],
            splay: [0.0; 5],
        };
        let hand = HandInputs::default();
        let mut memory = GripMemory::EMPTY;
        let target = hand_target(&inputs(&config, None, &measured, &hand), &mut memory);
        assert_eq!(target.shape, HandShape::of_curls(&measured, &config));
        assert_eq!(target.driven, ALL_FINGERS);
        assert_eq!(target.grip, None);
    }

    #[test]
    fn a_hold_eases_the_pose_in_and_a_release_eases_it_out() {
        let config = FingersConfig::new();
        let measured = FingerCurls::default();
        let hand = HandInputs::default();
        let wheel = Some(Grip::new(GripSource::Wheel));
        let mut memory = GripMemory::EMPTY;
        let mut last = 0.0;
        for _ in 0..60 {
            let target = hand_target(&inputs(&config, wheel, &measured, &hand), &mut memory);
            let middle = target.shape.curl[2];
            assert!(middle >= last, "the fingers close steadily");
            last = middle;
            assert_eq!(target.grip.unwrap().mode, GripMode::Posed);
        }
        let posed = GripSource::Wheel.pose().shape().curl[2];
        assert!((last - posed).abs() < 1e-3, "{last} {posed}");
        let released = hand_target(&inputs(&config, None, &measured, &hand), &mut memory);
        assert_eq!(released.grip.unwrap().mode, GripMode::Releasing);
        assert!(released.shape.curl[2] < last);
        for _ in 0..200 {
            hand_target(&inputs(&config, None, &measured, &hand), &mut memory);
        }
        let open = hand_target(&inputs(&config, None, &measured, &hand), &mut memory);
        assert_eq!(open.grip, None, "the released pose has opened");
        assert_eq!(memory, GripMemory::EMPTY);
    }

    #[test]
    fn a_light_grip_takes_part_of_the_pose() {
        let mut config = FingersConfig::new();
        config.grip.blend_s = 0.0;
        let measured = FingerCurls::default();
        let mut hand = HandInputs::default();
        let pickup = Some(Grip::new(GripSource::Pickup));
        let mut memory = GripMemory::EMPTY;
        hand.squeeze = 0.0;
        let light = hand_target(&inputs(&config, pickup, &measured, &hand), &mut memory);
        assert_eq!(light.grip.unwrap().share, config.grip.light_share);
        hand.squeeze = 1.0;
        let firm = hand_target(&inputs(&config, pickup, &measured, &hand), &mut memory);
        assert_eq!(firm.grip.unwrap().share, 1.0);
        assert!(firm.shape.curl[2] > light.shape.curl[2]);
    }

    #[test]
    fn a_weapon_left_to_the_game_drives_only_the_trigger_finger() {
        let mut config = FingersConfig::new();
        config.grip.weapons = false;
        let measured = FingerCurls::default();
        let hand = HandInputs::default();
        let gun = Some(Grip::new(GripSource::Gun));
        let mut memory = GripMemory::EMPTY;
        let target = hand_target(&inputs(&config, gun, &measured, &hand), &mut memory);
        assert_eq!(target.driven, TRIGGER_FINGER_ONLY);
        assert!(target.holding);
        assert_eq!(target.grip.unwrap().mode, GripMode::GameGrip);
        config.drive_all_when_holding = true;
        let all = hand_target(&inputs(&config, gun, &measured, &hand), &mut memory);
        assert_eq!(all.driven, ALL_FINGERS);
        config.grip.weapons = true;
        let posed = hand_target(&inputs(&config, gun, &measured, &hand), &mut memory);
        assert!(!posed.holding);
        assert_eq!(posed.grip.unwrap().mode, GripMode::Posed);
    }

    #[test]
    fn with_the_poses_off_a_sticky_hold_is_a_fist() {
        let mut config = FingersConfig::new();
        config.grip.enabled = false;
        let measured = FingerCurls {
            curl: [0.2; 5],
            splay: [0.1; 5],
        };
        let hand = HandInputs::default();
        let mut memory = GripMemory::EMPTY;
        let line = Some(Grip::new(GripSource::GrappleLine));
        let fist = hand_target(&inputs(&config, line, &measured, &hand), &mut memory);
        assert_eq!(fist.shape.curl, [1.0; 5]);
        assert_eq!(fist.shape.splay, [0.0; 5]);
        assert_eq!(fist.grip.unwrap().mode, GripMode::PosesOff);
        let pickup = Some(Grip::new(GripSource::Pickup));
        let kept = hand_target(&inputs(&config, pickup, &measured, &hand), &mut memory);
        assert_eq!(kept.shape.curl, measured.curl);
        let gun = Some(Grip::new(GripSource::Gun));
        let held = hand_target(&inputs(&config, gun, &measured, &hand), &mut memory);
        assert_eq!(held.driven, TRIGGER_FINGER_ONLY);
    }

    #[test]
    fn a_fit_is_used_only_with_the_contact_fit_on() {
        let mut config = FingersConfig::new();
        config.grip.blend_s = 0.0;
        let measured = FingerCurls::default();
        let hand = HandInputs {
            squeeze: 1.0,
            ..HandInputs::default()
        };
        let fit = [None, Some(0.2), Some(0.2), Some(0.2), None];
        let pickup = Some(Grip::new(GripSource::Pickup).fitted(Some(fit)));
        let mut memory = GripMemory::EMPTY;
        let fitted = hand_target(&inputs(&config, pickup, &measured, &hand), &mut memory);
        assert_eq!(fitted.grip.unwrap().fitted, 3);
        assert_eq!(fitted.shape.curl[1], 0.2);
        config.grip.contact_fit = false;
        let posed = hand_target(&inputs(&config, pickup, &measured, &hand), &mut memory);
        assert_eq!(posed.grip.unwrap().fitted, 0);
        assert_eq!(
            posed.shape.curl[1],
            GripSource::Pickup.pose().shape().curl[1]
        );
    }
}
