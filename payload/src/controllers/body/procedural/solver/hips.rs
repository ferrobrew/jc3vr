//! The hips: where they sit over the stance, how high the head puts them, and how far they drop
//! for the legs to reach the feet. Pure, in Rico's model space.

use glam::{Quat, Vec2, Vec3};

use crate::controllers::body::procedural::{
    config::ProceduralBodyConfig,
    rig::{Rig, RigBone},
    solver::{LEGS, SolveInput, line_yaw, pose::RigPose, wrap},
};

/// Where the hips want to sit horizontally, model space `(x, z)`: over the middle of the stance the
/// root carries, moved back by `counterbalance` of the head's lean ahead of where a neutral head
/// would be over it, the lean measured against the root and taken at most [`MAX_LEAN_M`], and
/// drawn [`PLANTED_BIAS`] of the way toward the one foot on the ground while the other swings.
pub fn hips_horizontal(rig: &Rig, input: &SolveInput, config: &ProceduralBodyConfig) -> Vec2 {
    let reference_hips = rig.reference(RigBone::Hips).translation;
    let head_over_hips = rig.reference(RigBone::Head).translation - reference_hips;
    let support = Vec2::new(input.support.x, input.support.z);
    let yaw = hips_yaw(input, config);
    let neutral = Quat::from_rotation_y(yaw) * Vec3::new(head_over_hips.x, 0.0, head_over_hips.z);
    let lean = Vec2::new(input.head.x, input.head.z) - support - Vec2::new(neutral.x, neutral.z);
    let lean = lean.clamp_length_max(MAX_LEAN_M);
    let counterbalance = config.counterbalance.clamp(0.0, 1.0);
    let bias = match input.planted {
        [true, false] => Some(0),
        [false, true] => Some(1),
        _ => None,
    }
    .map_or(Vec2::ZERO, |i| {
        let foot = input.feet[i];
        Vec2::new(foot.x - support.x, 0.0) * PLANTED_BIAS
    });
    // A squat sends the hips back over the heels as they go down, as a person's do to balance.
    let squat = (reference_hips.y - hips_height(rig, input.hips_head(), config)).max(0.0);
    let back = Quat::from_rotation_y(yaw) * Vec3::Z * (squat * SQUAT_BACK);
    let place = support - lean * counterbalance + bias + Vec2::new(back.x, back.z);
    if place.is_finite() { place } else { support }
}

/// How far the stance moves to step under the player's head, model space `(x, z)`, as an offset
/// from the stance the root carries (`support`, its middle): the head's lean past `allowance`
/// metres from where a neutral head would stand over the stance, its hips turned `yaw`. A lean
/// within the allowance is the hips' to take, as a hinge, and the feet stay; a head further out
/// (a player who walked off across the room, or leaned far over one foot) has the feet follow it to
/// within the allowance. Under the roomscale chase the root follows the head too, which leaves less
/// of the lean each tick, so the offset goes back to nothing as the root arrives.
pub fn step_offset(rig: &Rig, head: Vec3, support: Vec3, yaw: f32, allowance: f32) -> Vec2 {
    let reference_hips = rig.reference(RigBone::Hips).translation;
    let head_over_hips = rig.reference(RigBone::Head).translation - reference_hips;
    let neutral = Quat::from_rotation_y(yaw) * Vec3::new(head_over_hips.x, 0.0, head_over_hips.z);
    let lean = Vec2::new(
        head.x - support.x - neutral.x,
        head.z - support.z - neutral.z,
    );
    let length = lean.length();
    let allowance = if allowance.is_finite() {
        allowance.max(0.0)
    } else {
        0.0
    };
    if !length.is_finite() || length <= allowance {
        return Vec2::ZERO;
    }
    lean * (1.0 - allowance / length)
}

/// Follows the hips' horizontal place with a time constant, so a quick lean bends the spine before
/// the hips go with it. It follows the stance's step under the head the same way.
#[derive(Debug, Copy, Clone, PartialEq, Default)]
pub struct HipsFollower {
    current: Option<Vec2>,
}

impl HipsFollower {
    pub const fn new() -> Self {
        Self { current: None }
    }

    pub fn reset(&mut self) {
        self.current = None;
    }

    /// Start from `place`: the hips start where something else left them.
    pub fn seed(&mut self, place: Vec2) {
        self.current = place.is_finite().then_some(place);
    }

    /// Move toward `wanted` over `dt` seconds with the time constant `time_s`, and return the
    /// place. The first step takes `wanted` as it is.
    pub fn step(&mut self, wanted: Vec2, dt: f32, time_s: f32) -> Vec2 {
        if !wanted.is_finite() {
            return self.current.unwrap_or(Vec2::ZERO);
        }
        let next = match self.current {
            Some(current) if time_s > 0.0 => {
                let k = 1.0 - (-dt.max(0.0) / time_s).exp();
                current + (wanted - current) * k
            }
            _ => wanted,
        };
        self.current = Some(next);
        next
    }
}

/// How far the hips must drop below where the head puts them for both legs to reach their feet,
/// metres: zero when they reach, and never more than the hips have left above the lowest they go.
pub fn reach_drop(rig: &Rig, input: &SolveInput, config: &ProceduralBodyConfig) -> f32 {
    let mut pose = input.animated;
    place_hips(rig, &mut pose, &hips_input(rig, input, config, 0.0), config);
    let mut need: f32 = 0.0;
    for (side, (up, leg, foot)) in LEGS.into_iter().enumerate() {
        let hip = pose.position(up);
        let length = (pose.position(leg) - hip).length()
            + (pose.position(foot) - pose.position(leg)).length();
        let rest = (rig.reference(foot).translation - rig.reference(up).translation).length();
        let reach = (length * LEG_REACH).max(rest.min(length));
        let offset = hip - input.feet[side];
        let across = Vec2::new(offset.x, offset.z).length();
        let below = if across < reach {
            offset.y - (reach * reach - across * across).sqrt()
        } else {
            offset.y
        };
        need = need.max(below);
    }
    let room = hips_room(rig, input.hips_head(), config);
    if need.is_finite() {
        need.clamp(0.0, room)
    } else {
        0.0
    }
}

/// Follows the hips' reach drop: down quickly, so a foot on a lower stair reaches it as it lands,
/// and back up at a third of the pace, so the body does not bob on every stride.
#[derive(Debug, Copy, Clone, PartialEq, Default)]
pub struct DropFollower {
    current: f32,
}

impl DropFollower {
    pub const fn new() -> Self {
        Self { current: 0.0 }
    }

    pub fn reset(&mut self) {
        self.current = 0.0;
    }

    /// Start from `drop`: the hips start where something else left them and rise from there at the
    /// follower's pace.
    pub fn seed(&mut self, drop: f32) {
        self.current = if drop.is_finite() { drop.max(0.0) } else { 0.0 };
    }

    /// Move toward `need` over `dt` seconds with the time constant `time_s`, and return the drop.
    pub fn step(&mut self, need: f32, dt: f32, time_s: f32) -> f32 {
        let need = if need.is_finite() { need.max(0.0) } else { 0.0 };
        let time = if need > self.current {
            time_s
        } else {
            time_s * RISE_SLOWDOWN
        };
        self.current = if time <= 0.0 {
            need
        } else {
            let k = 1.0 - (-dt.max(0.0) / time).exp();
            self.current + (need - self.current) * k
        };
        self.current
    }
}

/// The hips' height for a head at `head_height`, before any drop for the legs' reach: the head's
/// less the reference pose's head-over-hips height, at most the reference standing height and at
/// least `min_hips_height` of it.
pub fn hips_height(rig: &Rig, head_height: f32, config: &ProceduralBodyConfig) -> f32 {
    let reference_hips = rig.reference(RigBone::Hips).translation;
    let head_over_hips = rig.reference(RigBone::Head).translation.y - reference_hips.y;
    let standing = reference_hips.y;
    let lowest = standing * config.min_hips_height.clamp(0.1, 1.0);
    (head_height - head_over_hips).clamp(lowest, standing)
}

/// How far a head at `head_height` has taken the hips toward their lowest, `0..=1`: zero standing,
/// one at the floor of a squat ([`hips_height`]).
pub fn crouch_depth(rig: &Rig, head_height: f32, config: &ProceduralBodyConfig) -> f32 {
    let standing = rig.reference(RigBone::Hips).translation.y;
    let lowest = standing * config.min_hips_height.clamp(0.1, 1.0);
    let span = standing - lowest;
    if span <= 1e-4 {
        return 0.0;
    }
    let depth = (standing - hips_height(rig, head_height, config)) / span;
    if depth.is_finite() {
        depth.clamp(0.0, 1.0)
    } else {
        0.0
    }
}

/// The most the hips may drop below where a head at `head_height` puts them: down to their lowest,
/// counted from where the head has already taken them, so a crouch's drop and a stride's do not
/// add past the floor of a squat.
pub fn hips_room(rig: &Rig, head_height: f32, config: &ProceduralBodyConfig) -> f32 {
    let lowest =
        rig.reference(RigBone::Hips).translation.y * config.min_hips_height.clamp(0.1, 1.0);
    (hips_height(rig, head_height, config) - lowest).max(0.0)
}

/// The share of a leg's length it reaches to before the hips drop: a leg never quite straight.
const LEG_REACH: f32 = 0.97;

/// How much slower the hips rise back than they drop.
const RISE_SLOWDOWN: f32 = 3.0;

/// The most the head's lean counts for the hips, metres: a head further out is a reach, not a lean
/// the hips should hinge back from.
pub const MAX_LEAN_M: f32 = 0.5;

/// How far back the hips go per metre they drop below standing.
pub const SQUAT_BACK: f32 = 0.3;

/// How far toward the one foot on the ground the hips are drawn while the other swings, as a share
/// of that foot's sideways offset from the stance's middle: a walk's small sway over the stance
/// foot.
pub const PLANTED_BIAS: f32 = 0.15;

/// What the hips are placed from.
pub(super) struct HipsInput {
    /// The head's height the hips follow ([`SolveInput::hips_head`]).
    pub(super) head_height: f32,
    pub(super) horizontal: Vec2,
    pub(super) yaw: f32,
    pub(super) drop: f32,
    /// Where a held pose puts the hips, and how much of it to take.
    pub(super) held: Option<(Vec3, f32)>,
}

pub(super) fn hips_input(
    rig: &Rig,
    input: &SolveInput,
    config: &ProceduralBodyConfig,
    drop: f32,
) -> HipsInput {
    HipsInput {
        head_height: input.hips_head(),
        horizontal: input
            .hips_xz
            .filter(|h| h.is_finite())
            .unwrap_or_else(|| hips_horizontal(rig, input, config)),
        yaw: hips_yaw(input, config),
        drop: if drop.is_finite() { drop.max(0.0) } else { 0.0 },
        held: input
            .lower
            .map(|held| (held.hips, held.weight.clamp(0.0, 1.0)))
            .filter(|(hips, _)| hips.is_finite()),
    }
}

/// The hips' turn toward the player's torso, within its limit.
pub fn hips_yaw(input: &SolveInput, config: &ProceduralBodyConfig) -> f32 {
    let torso_yaw = input.torso_yaw.filter(|y| y.is_finite()).unwrap_or(0.0);
    let pelvis_limit = config.pelvis_turn_limit_deg.max(0.0).to_radians();
    torso_yaw.clamp(-pelvis_limit, pelvis_limit)
}

/// Place and turn the hips. See the module docs.
pub(super) fn place_hips(
    rig: &Rig,
    pose: &mut RigPose,
    input: &HipsInput,
    config: &ProceduralBodyConfig,
) {
    let HipsInput {
        head_height,
        horizontal,
        yaw,
        drop,
        held,
    } = *input;
    let height = hips_height(rig, head_height, config) - drop;
    let own = Vec3::new(horizontal.x, height, horizontal.y);
    let target = held.map_or(own, |(hips, weight)| own.lerp(hips, weight));

    let hips = pose.position(RigBone::Hips);
    pose.translate(RigBone::Hips, target - hips);

    if let Some(current) = line_yaw(
        pose.position(RigBone::LeftUpLeg),
        pose.position(RigBone::RightUpLeg),
    ) {
        let delta = wrap(yaw - current);
        pose.rotate_about(RigBone::Hips, Quat::from_rotation_y(delta), target);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::controllers::body::procedural::rig::tests::skeleton;

    #[test]
    fn the_stance_steps_under_a_head_that_has_walked_off_but_not_under_a_lean() {
        let rig = Rig::from_skeleton(&skeleton()).unwrap();
        let head = rig.reference(RigBone::Head).translation;
        let support = Vec3::ZERO;
        // Standing over the stance, and a lean within the allowance, leave the feet.
        assert_eq!(step_offset(&rig, head, support, 0.0, 0.2), Vec2::ZERO);
        let lean = head + Vec3::new(0.0, -0.1, -0.18);
        assert_eq!(step_offset(&rig, lean, support, 0.0, 0.2), Vec2::ZERO);
        // A metre off to the right: the stance follows to within the allowance of the head.
        let walked = head + Vec3::new(1.0, 0.0, 0.0);
        let offset = step_offset(&rig, walked, support, 0.0, 0.2);
        assert!(offset.abs_diff_eq(Vec2::new(0.8, 0.0), 1e-5), "{offset}");
        // Once the stance is there, nothing more.
        let moved = Vec3::new(offset.x, 0.0, offset.y);
        assert!(step_offset(&rig, walked, moved, 0.0, 0.2).length() < 1e-5);
        // Without an allowance the feet go right under the head; a head that is not a number
        // leaves them.
        let under = step_offset(&rig, walked, support, 0.0, 0.0);
        assert!(under.abs_diff_eq(Vec2::new(1.0, 0.0), 1e-5));
        assert_eq!(step_offset(&rig, Vec3::NAN, support, 0.0, 0.2), Vec2::ZERO);
    }

    #[test]
    fn the_crouch_depth_runs_from_standing_to_the_squats_floor() {
        let rig = Rig::from_skeleton(&skeleton()).unwrap();
        let config = ProceduralBodyConfig::new();
        let head = rig.reference(RigBone::Head).translation.y;
        assert_eq!(crouch_depth(&rig, head, &config), 0.0);
        assert_eq!(crouch_depth(&rig, head + 0.3, &config), 0.0);
        // The test hips stand at 1.0 and go no lower than 0.5.
        assert!((crouch_depth(&rig, head - 0.25, &config) - 0.5).abs() < 1e-4);
        assert_eq!(crouch_depth(&rig, head - 0.5, &config), 1.0);
        assert_eq!(crouch_depth(&rig, head - 2.0, &config), 1.0);
        assert_eq!(crouch_depth(&rig, f32::NAN, &config), 0.0);
    }

    #[test]
    fn the_hips_follower_eases_toward_its_place() {
        let mut follower = HipsFollower::new();
        assert_eq!(follower.step(Vec2::X, 0.05, 0.2), Vec2::X);
        let next = follower.step(Vec2::ZERO, 0.05, 0.2);
        assert!(next.x > 0.5 && next.x < 1.0);
        assert_eq!(follower.step(Vec2::NAN, 0.05, 0.2), next);
    }

    #[test]
    fn the_drop_follower_drops_fast_and_rises_slowly() {
        let mut follower = DropFollower::new();
        let down = follower.step(0.1, 0.05, 0.05);
        let mut up = DropFollower { current: 0.1 };
        let rise = 0.1 - up.step(0.0, 0.05, 0.05);
        assert!(down > rise * 2.0);
        assert!(follower.step(f32::NAN, 0.05, 0.05) <= down);
    }
}
