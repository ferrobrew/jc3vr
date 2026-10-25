//! Player proportion priors and measured character reference geometry for one calibration version.

use glam::Vec3;

use crate::controllers::body::{
    BodyDimensions,
    procedural::rig::{Rig, RigBone},
    rig::{
        estimate::EstimateError,
        torso::{EYES_TO_NECK_PIVOT, HIP_SPACING_FRACTION},
    },
};

/// Player leg and torso splits remain priors. Character chain lengths come from `Rig::reference`.
#[derive(Debug, Copy, Clone, PartialEq)]
pub(crate) struct Anatomy {
    pub calibration: u64,
    pub player: BodyDimensions,
    pub character: Segments,
    pub player_segments: Segments,
    /// Model-space offset from the observed root to the neutral stance centre.
    pub root_to_stance: Vec3,
    pub ankle_height: f32,
}

#[derive(Debug, Copy, Clone, PartialEq)]
pub(crate) struct Segments {
    pub thighs: [f32; 2],
    pub shins: [f32; 2],
    pub lower_torso: f32,
    pub upper_torso: f32,
    pub shoulder_width: f32,
    pub hip_width: f32,
    pub arms: [f32; 2],
    pub neutral_hip_height: f32,
}

impl Anatomy {
    pub(crate) fn from_rig(
        player: BodyDimensions,
        rig: &Rig,
        calibration: u64,
    ) -> Result<Self, EstimateError> {
        player
            .validate()
            .map_err(|_| EstimateError::InvalidAnatomy)?;
        if rig.reference.iter().any(|t| {
            !t.translation.is_finite()
                || !t.rotation.is_finite()
                || (t.rotation.length_squared() - 1.0).abs() > 1.0e-4
                || !t.scale.abs_diff_eq(Vec3::ONE, 1.0e-4)
        }) {
            return Err(EstimateError::InvalidAnatomy);
        }
        let point = |bone: RigBone| rig.reference[bone.index()].translation;
        let hips = [point(RigBone::LeftUpLeg), point(RigBone::RightUpLeg)];
        let knees = [point(RigBone::LeftLeg), point(RigBone::RightLeg)];
        let ankles = [point(RigBone::LeftFoot), point(RigBone::RightFoot)];
        let pelvis = (hips[0] + hips[1]) * 0.5;
        let chest = (point(RigBone::LeftShoulder) + point(RigBone::RightShoulder)) * 0.5;
        let neck = point(RigBone::Neck);
        let stance = (ankles[0] + ankles[1]) * 0.5;
        let character = Segments {
            thighs: [0, 1].map(|i| hips[i].distance(knees[i])),
            shins: [0, 1].map(|i| knees[i].distance(ankles[i])),
            lower_torso: pelvis.distance(chest),
            upper_torso: chest.distance(neck),
            shoulder_width: point(RigBone::LeftShoulder).distance(point(RigBone::RightShoulder)),
            hip_width: hips[0].distance(hips[1]),
            arms: rig.arms.map(|arm| arm.length),
            neutral_hip_height: pelvis.y - stance.y,
        };
        let player_hips = player.height * 0.53;
        let player_neck = player.eye_height + EYES_TO_NECK_PIVOT.y;
        let player_segments = Segments {
            thighs: [player_hips * 0.51; 2],
            shins: [player_hips * 0.49; 2],
            lower_torso: player.shoulder_height - player_hips,
            upper_torso: player_neck - player.shoulder_height,
            shoulder_width: player.shoulder_width,
            hip_width: player.shoulder_width * HIP_SPACING_FRACTION,
            arms: [player.upper_arm + player.forearm + player.hand * 0.25; 2],
            neutral_hip_height: player_hips,
        };
        let anatomy = Self {
            calibration,
            player,
            character,
            player_segments,
            root_to_stance: Vec3::new(stance.x, 0.0, stance.z),
            ankle_height: stance.y,
        };
        anatomy.validate()?;
        Ok(anatomy)
    }

    pub(crate) fn validate(&self) -> Result<(), EstimateError> {
        self.player
            .validate()
            .map_err(|_| EstimateError::InvalidAnatomy)?;
        if !self.root_to_stance.is_finite()
            || !self.ankle_height.is_finite()
            || self.ankle_height < 0.0
            || !self.character.valid()
            || !self.player_segments.valid()
            || (0..2).any(|side| {
                let upper = self.character.thighs[side];
                let lower = self.character.shins[side];
                let minimum =
                    (upper * upper + lower * lower + 2.0 * upper * lower * 2.65_f32.cos())
                        .max(0.0)
                        .sqrt();
                self.character.neutral_hip_height * 0.995 < minimum
            })
        {
            return Err(EstimateError::InvalidAnatomy);
        }
        Ok(())
    }
}

impl Segments {
    pub(crate) fn torso(self) -> f32 {
        self.lower_torso + self.upper_torso
    }

    pub(crate) fn leg(self, side: usize) -> f32 {
        self.thighs[side] + self.shins[side]
    }

    fn valid(self) -> bool {
        [
            self.thighs[0],
            self.thighs[1],
            self.shins[0],
            self.shins[1],
            self.lower_torso,
            self.upper_torso,
            self.shoulder_width,
            self.hip_width,
            self.arms[0],
            self.arms[1],
            self.neutral_hip_height,
        ]
        .into_iter()
        .all(|length| length.is_finite() && (0.015..=3.0).contains(&length))
            && (0..2).all(|i| self.neutral_hip_height <= self.leg(i) * 1.01)
    }
}
