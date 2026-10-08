//! The blend from the measured curls to a grip pose, pure.
//!
//! A hand that holds something takes a share of its grip pose that follows how firmly it grips:
//! a sticky hold (a gun, a vehicle's control, the grapple line) has the whole pose, and a hold the
//! grip keeps (a loose object, a ledge, a door) has [`GripPosesConfig::light_share`] at the
//! lightest squeeze and the whole pose from [`GripPosesConfig::firm_squeeze`]. The share then
//! follows its target with [`GripPosesConfig::blend_s`], so taking hold closes the fingers onto
//! the pose and letting go opens them back to the measured curls over a short motion.
//!
//! The fingers a pose leaves live keep the measured curl at any share: the trigger finger off the
//! trigger, and a finger hand tracking measures where the pose leaves it free
//! ([`FingerRole::LiveWhenTracked`]).

use crate::controllers::fingers::{
    config::{FingersConfig, GripPosesConfig},
    curl::{CurlSource, FingerCurls},
    grip::{
        Firmness,
        catalogue::{FingerRole, PoseShape},
    },
    pose::HandShape,
};

/// What a hand's blend toward its grip pose reads.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct BlendInputs<'a> {
    /// The hand's measured curls, and where they came from.
    pub measured: &'a FingerCurls,
    pub source: CurlSource,
    /// The trigger's pull, `0..=1`, and whether the trigger finger touches it.
    pub trigger: f32,
    pub trigger_touch: bool,
    pub pose: &'a PoseShape,
    /// The contact fit's curl for each finger that found the held thing.
    pub fit: [Option<f32>; 5],
    /// The pose's share, `0..=1`.
    pub share: f32,
}

/// The hand's shape: the measured curls blended toward the grip pose by the share.
pub fn blend(inputs: &BlendInputs, config: &FingersConfig) -> HandShape {
    let measured = HandShape::of_curls(inputs.measured, config);
    let pose = inputs.pose;
    let mut target = HandShape {
        curl: [0.0; 5],
        splay: pose.splay,
        hook: pose.hook,
        opposition: pose.opposition,
        thumb_spread: pose
            .thumb_spread_deg
            .map_or(measured.thumb_spread, f32::to_radians),
    };
    for i in 0..5 {
        let posed = inputs.fit[i].unwrap_or(pose.curl[i]);
        let curl = match pose.roles[i] {
            FingerRole::Posed => Some(posed),
            FingerRole::Trigger { rest, pulled } => (inputs.trigger_touch || inputs.trigger > 0.0)
                .then(|| rest + (pulled - rest) * inputs.trigger.clamp(0.0, 1.0)),
            FingerRole::LiveWhenTracked => (inputs.source != CurlSource::Joints).then_some(posed),
        };
        match curl {
            Some(curl) => target.curl[i] = curl.clamp(0.0, 1.0),
            None => {
                target.curl[i] = measured.curl[i];
                target.splay[i] = measured.splay[i];
                target.hook[i] = 0.0;
                if i == 0 {
                    target.opposition = measured.opposition;
                    target.thumb_spread = measured.thumb_spread;
                }
            }
        }
    }
    let share = if inputs.share.is_finite() {
        inputs.share.clamp(0.0, 1.0)
    } else {
        0.0
    };
    measured.lerp(target, share)
}

/// The pose's share a hold of `firmness` asks for at the grip's `squeeze`, `0..=1`.
pub fn firmness_share(firmness: Firmness, squeeze: f32, config: &GripPosesConfig) -> f32 {
    match firmness {
        Firmness::Held => 1.0,
        Firmness::Grip => {
            let light = config.light_share.clamp(0.0, 1.0);
            let firm = config.firm_squeeze.clamp(0.01, 1.0);
            let squeeze = if squeeze.is_finite() { squeeze } else { 0.0 };
            light + (1.0 - light) * (squeeze / firm).clamp(0.0, 1.0)
        }
    }
}

/// Follow the share `target` from `previous` over `dt` seconds with the time constant
/// `time_constant_s`.
pub fn follow_share(previous: f32, target: f32, dt: f32, time_constant_s: f32) -> f32 {
    if time_constant_s <= 0.0 || !dt.is_finite() {
        return target;
    }
    let t = 1.0 - (-dt.max(0.0) / time_constant_s).exp();
    previous + (target - previous) * t
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::controllers::fingers::{curl::Finger, grip::catalogue::GripPose};

    fn open() -> FingerCurls {
        FingerCurls::default()
    }

    fn inputs<'a>(measured: &'a FingerCurls, pose: &'a PoseShape, share: f32) -> BlendInputs<'a> {
        BlendInputs {
            measured,
            source: CurlSource::Estimate,
            trigger: 0.0,
            trigger_touch: false,
            pose,
            fit: [None; 5],
            share,
        }
    }

    #[test]
    fn no_share_keeps_the_measured_curls_and_the_whole_share_takes_the_pose() {
        let config = FingersConfig::new();
        let measured = FingerCurls {
            curl: [0.1, 0.2, 0.3, 0.4, 0.5],
            splay: [0.0, 0.1, 0.0, -0.1, -0.2],
        };
        let pose = GripPose::Foregrip.shape();
        let none = blend(&inputs(&measured, &pose, 0.0), &config);
        assert_eq!(none, HandShape::of_curls(&measured, &config));
        let whole = blend(&inputs(&measured, &pose, 1.0), &config);
        assert_eq!(whole.curl, pose.curl);
        assert_eq!(whole.hook, pose.hook);
        assert_eq!(whole.opposition, pose.opposition);
        let half = blend(&inputs(&measured, &pose, 0.5), &config);
        let middle = Finger::Middle.index();
        let expected = (measured.curl[middle] + pose.curl[middle]) * 0.5;
        assert!((half.curl[middle] - expected).abs() < 1e-6);
        assert_eq!(blend(&inputs(&measured, &pose, f32::NAN), &config), none);
    }

    #[test]
    fn the_trigger_finger_rests_on_the_trigger_follows_it_and_points_off_it() {
        let config = FingersConfig::new();
        let pose = GripPose::PistolGrip.shape();
        let FingerRole::Trigger { rest, pulled } = pose.roles[1] else {
            panic!("the pistol grip has a trigger finger");
        };
        let measured = open();
        let mut i = inputs(&measured, &pose, 1.0);
        assert_eq!(blend(&i, &config).curl[1], 0.0, "off the trigger it points");
        i.trigger_touch = true;
        assert_eq!(blend(&i, &config).curl[1], rest);
        i.trigger = 1.0;
        assert_eq!(blend(&i, &config).curl[1], pulled);
        i.trigger = 0.5;
        assert!((blend(&i, &config).curl[1] - (rest + pulled) * 0.5).abs() < 1e-6);
        assert_eq!(blend(&i, &config).curl[2], pose.curl[2], "the grip stays");
    }

    #[test]
    fn a_tracked_finger_the_pose_leaves_free_stays_live() {
        let config = FingersConfig::new();
        let pose = GripPose::Cylinder { diameter_m: 0.035 }.shape();
        assert_eq!(pose.roles[0], FingerRole::LiveWhenTracked);
        let measured = FingerCurls {
            curl: [0.2, 0.3, 0.3, 0.3, 0.3],
            splay: [0.0; 5],
        };
        let mut i = inputs(&measured, &pose, 1.0);
        let estimated = blend(&i, &config);
        assert_eq!(
            estimated.curl[0], pose.curl[0],
            "an estimate takes the pose"
        );
        i.source = CurlSource::Joints;
        let tracked = blend(&i, &config);
        assert_eq!(tracked.curl[0], 0.2, "a tracked thumb stays live");
        assert_eq!(tracked.opposition, 0.0);
        assert_eq!(tracked.thumb_spread, config.thumb_spread_deg.to_radians());
        assert_eq!(tracked.curl[1], pose.curl[1], "the wrap is posed");
    }

    #[test]
    fn a_contact_fit_replaces_the_posed_curl() {
        let config = FingersConfig::new();
        let pose = GripPose::Wrap.shape();
        let measured = open();
        let mut i = inputs(&measured, &pose, 1.0);
        i.fit = [None, Some(0.3), Some(0.9), None, Some(1.4)];
        let fitted = blend(&i, &config);
        assert_eq!(fitted.curl[0], pose.curl[0]);
        assert_eq!(fitted.curl[1], 0.3);
        assert_eq!(fitted.curl[2], 0.9);
        assert_eq!(fitted.curl[4], 1.0, "a fit is clamped to a fist");
    }

    #[test]
    fn the_share_follows_the_grip() {
        let config = GripPosesConfig::new();
        assert_eq!(firmness_share(Firmness::Held, 0.0, &config), 1.0);
        let light = firmness_share(Firmness::Grip, 0.0, &config);
        assert_eq!(light, config.light_share);
        assert_eq!(
            firmness_share(Firmness::Grip, config.firm_squeeze, &config),
            1.0
        );
        assert_eq!(firmness_share(Firmness::Grip, 1.0, &config), 1.0);
        let middle = firmness_share(Firmness::Grip, config.firm_squeeze * 0.5, &config);
        assert!(middle > light && middle < 1.0);
        assert_eq!(firmness_share(Firmness::Grip, f32::NAN, &config), light);
    }

    #[test]
    fn the_share_eases_in_and_out() {
        let step = follow_share(0.0, 1.0, 0.06, 0.06);
        assert!((step - (1.0 - (-1.0f32).exp())).abs() < 1e-6);
        let back = follow_share(1.0, 0.0, 0.06, 0.06);
        assert!((back - (-1.0f32).exp()).abs() < 1e-6);
        assert_eq!(follow_share(0.2, 1.0, 0.01, 0.0), 1.0);
        assert_eq!(follow_share(0.2, 1.0, f32::INFINITY, 0.06), 1.0);
    }
}
