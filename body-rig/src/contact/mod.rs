//! The foot contact model: each foot planted on a support, swinging to a planned landing, or
//! unsupported, and the rules that release a planted foot into a step (`docs/mod/body/ik-body.md`,
//! "Feet").
//!
//! A planted foot is a pose in its support's frame, with the support's identity, so it stays put
//! however the pelvis moves, and a support that moves carries it. A foot is released into a step
//! only by an explicit rule ([`rules`]): the hip carried out of the leg's reach, the pelvis twisted
//! over the feet past a threshold for a while, the centre of mass out of the support polygon by a
//! margin, the root carried past a tolerance by something other than the player, or a foot left off
//! its stance spot for a while once the body has come to rest. One foot swings at a time,
//! alternating, to where the pelvis puts its hip plus the stance's width and toe-out, probed for the
//! ground there ([`swing`]). A missed probe leaves the foot unsupported, never planted.
//!
//! When both feet would have to move at once, or a step would be longer than a step, or the body
//! outruns a swinging foot, the movement is a walk rather than a step: the model drops its contacts
//! and signals a hand-over ([`Handover`]) for the caller to give the legs to the game's walk.
//!
//! The model works in any frame with +Y up in which the supports' frames are given: the payload uses
//! the game world, in Rico's metres.

mod rate;
mod rules;
mod swing;
#[cfg(test)]
mod tests;

use glam::{Quat, Vec2, Vec3};

pub use crate::contact::{
    rate::StepRate,
    rules::{outside_support, support_points},
    swing::Swing,
};
use crate::{
    angle::{right, wrap},
    contact::{
        rules::{Need, needs},
        swing::{Landing, plan},
    },
};

/// A foot.
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum Side {
    Left,
    Right,
}

impl Side {
    pub const BOTH: [Side; 2] = [Side::Left, Side::Right];

    pub const fn index(self) -> usize {
        match self {
            Side::Left => 0,
            Side::Right => 1,
        }
    }

    pub const fn other(self) -> Side {
        match self {
            Side::Left => Side::Right,
            Side::Right => Side::Left,
        }
    }

    /// Which way along the body's right the foot stands from the centre: -1 left, 1 right.
    pub const fn across(self) -> f32 {
        match self {
            Side::Left => -1.0,
            Side::Right => 1.0,
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Side::Left => "left",
            Side::Right => "right",
        }
    }
}

/// What a foot stands on: an identity the caller can find the support's frame by.
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash, Default)]
pub struct SupportId(pub u64);

impl SupportId {
    /// The static world: the terrain and anything that never moves.
    pub const WORLD: Self = Self(0);
}

/// A support's frame: its pose in the model's frame.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct SupportFrame {
    pub translation: Vec3,
    pub rotation: Quat,
}

impl SupportFrame {
    pub const IDENTITY: Self = Self {
        translation: Vec3::ZERO,
        rotation: Quat::IDENTITY,
    };

    /// `pose`, held in this support's frame, in the model's frame.
    pub fn to_model(&self, pose: FootPose) -> FootPose {
        FootPose {
            ankle: self.translation + self.rotation * pose.ankle,
            yaw: wrap(pose.yaw + yaw_turn(self.rotation)),
        }
    }

    /// `pose`, in the model's frame, held in this support's frame.
    pub fn to_local(&self, pose: FootPose) -> FootPose {
        FootPose {
            ankle: self.rotation.inverse() * (pose.ankle - self.translation),
            yaw: wrap(pose.yaw - yaw_turn(self.rotation)),
        }
    }

    fn is_finite(&self) -> bool {
        self.translation.is_finite() && self.rotation.is_finite()
    }
}

/// A foot's pose: the ankle, and the heading the foot points along.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct FootPose {
    pub ankle: Vec3,
    pub yaw: f32,
}

impl FootPose {
    /// The ankle's floor point, `(x, z)`.
    pub fn flat(&self) -> Vec2 {
        Vec2::new(self.ankle.x, self.ankle.z)
    }

    fn is_finite(&self) -> bool {
        self.ankle.is_finite() && self.yaw.is_finite()
    }
}

/// The ground a probe found.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct Ground {
    /// The ground's height under the probed point.
    pub height: f32,
    pub support: SupportId,
}

/// A ground probe's answer.
#[derive(Debug, Copy, Clone, PartialEq)]
pub enum Probe {
    Hit(Ground),
    /// No ground within reach under the point.
    Miss,
    /// Asked for; the answer comes on a later frame.
    Pending,
}

/// What the model asks of the world around the feet.
pub trait Surroundings {
    /// The frame of `support` now, or `None` once it is gone.
    fn frame(&self, support: SupportId) -> Option<SupportFrame>;

    /// The ground under `at` (`(x, z)`) for the foot `side`, near the height `near`.
    fn probe(&mut self, side: Side, at: Vec2, near: f32) -> Probe;
}

/// Why a foot stepped.
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum Release {
    /// The pelvis carried the hip out of the leg's reach.
    Reach,
    /// The pelvis twisted over the feet past the threshold for the hold.
    Twist,
    /// The centre of mass left the support polygon by the margin.
    CentreOfMass,
    /// Something other than the player carried the root past the tolerance.
    RootMoved,
    /// The foot stood off its stance spot for a while.
    Tidy,
    /// The other foot just stepped and left this one off its stance spot.
    FollowUp,
}

impl Release {
    pub fn label(self) -> &'static str {
        match self {
            Release::Reach => "out of reach",
            Release::Twist => "twist",
            Release::CentreOfMass => "centre of mass",
            Release::RootMoved => "root moved",
            Release::Tidy => "off its stance",
            Release::FollowUp => "follow-up",
        }
    }
}

/// Why the model handed the legs to the game's walk.
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum Handover {
    /// Both feet had to move at once while the pelvis moved at a walk.
    BothFeet,
    /// The step needed was longer than the longest step.
    StepTooLong,
    /// The pelvis carried the planted foot's hip past its reach while the other foot swung.
    Outrun,
}

impl Handover {
    pub fn label(self) -> &'static str {
        match self {
            Handover::BothFeet => "both feet at once",
            Handover::StepTooLong => "step too long",
            Handover::Outrun => "outrun",
        }
    }
}

/// What happened to a foot this frame.
#[derive(Debug, Copy, Clone, PartialEq)]
pub enum ContactEvent {
    /// Planted where it stood, from unsupported.
    Acquired { side: Side },
    /// Released into a step.
    Released {
        side: Side,
        reason: Release,
        /// How far the step goes, metres.
        length_m: f32,
    },
    /// Planted at the end of a step.
    Landed { side: Side },
    /// The step's landing has no ground: the foot is unsupported.
    Missed { side: Side },
    /// The foot's support is gone: the foot is unsupported.
    Lost { side: Side },
}

/// The stance a step lands in: the square stance's settings.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct Stance {
    /// How far apart the ankles stand, metres.
    pub width_m: f32,
    /// How far each foot turns out from the pelvis's heading, degrees.
    pub toe_out_deg: f32,
}

/// The contact model's tuning.
#[cfg_attr(
    feature = "serde",
    derive(serde::Serialize, serde::Deserialize),
    serde(default)
)]
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct ContactTuning {
    /// How long a step's swing takes, seconds.
    pub step_s: f32,
    /// How high the swing lifts the foot at its middle, metres.
    pub arc_m: f32,
    /// The share of the leg's straight length past which the hip is out of the leg's reach: a
    /// little over one, since a player standing tall has the hips at the legs' full length over
    /// the ankles, and the caller lowers the drawn pelvis to keep a reaching leg on its foot.
    pub reach_share: f32,
    /// How far the pelvis twists over the feet before the twist counts, degrees.
    pub twist_deg: f32,
    /// How long the twist must hold before a foot steps, seconds.
    pub twist_hold_s: f32,
    /// How far outside the support polygon the centre of mass goes before a foot steps, metres.
    pub com_margin_m: f32,
    /// The share of the way from the pelvis to the neck the centre of mass stands, horizontally.
    pub com_neck_share: f32,
    /// How far something other than the player may carry the root before a foot steps, metres.
    pub root_tolerance_m: f32,
    /// How far a foot may stand off its stance spot before it tidies, metres...
    pub tidy_m: f32,
    /// ...or how far it may turn off the stance's heading, degrees...
    pub tidy_deg: f32,
    /// ...for this long, seconds.
    pub tidy_s: f32,
    /// How far ahead of the stance spot a step lands, as a share of the pelvis's travel over the
    /// step's swing.
    pub lead: f32,
    /// The longest step; a longer one hands over to the walk, metres.
    pub max_step_m: f32,
    /// The pelvis's speed past which two feet moving at once are a walk, metres per second.
    pub walk_speed_mps: f32,
    /// How far past its reach a planted foot's hip may be carried while the other swings before
    /// the body has outrun the step, metres.
    pub outrun_m: f32,
    /// The foot's length from the ankle to the toe, for the support polygon, metres.
    pub foot_m: f32,
    /// How long a step waits at its landing for the probe's answer before the foot counts as
    /// unsupported, seconds.
    pub landing_wait_s: f32,
}

impl ContactTuning {
    pub const fn new() -> Self {
        Self {
            step_s: 0.3,
            arc_m: 0.07,
            reach_share: 1.02,
            twist_deg: 40.0,
            twist_hold_s: 0.3,
            com_margin_m: 0.05,
            com_neck_share: 0.3,
            root_tolerance_m: 0.2,
            tidy_m: 0.15,
            tidy_deg: 25.0,
            tidy_s: 1.0,
            lead: 0.5,
            max_step_m: 0.6,
            walk_speed_mps: 0.8,
            outrun_m: 0.1,
            foot_m: 0.18,
            landing_wait_s: 0.2,
        }
    }
}

impl Default for ContactTuning {
    fn default() -> Self {
        Self::new()
    }
}

/// One frame's inputs, in the model's frame.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct ContactFrame {
    /// The pelvis.
    pub pelvis: Vec3,
    /// The pelvis's heading.
    pub pelvis_yaw: f32,
    /// The neck pivot, for the centre of mass.
    pub neck: Vec3,
    /// The hip joints, left then right.
    pub hips: [Vec3; 2],
    /// How far from the hip the ankle reaches with the knee straight, metres.
    pub leg_m: f32,
    /// How far the ankle stands over the ground, metres.
    pub ankle_height_m: f32,
    /// The pelvis's horizontal velocity, `(x, z)`, metres per second.
    pub pelvis_velocity: Vec2,
    /// How far something other than the player moved the root this frame, `(x, z)`.
    pub external_shift: Vec2,
    pub stance: Stance,
    /// Where the animation has each foot now, left then right: an unsupported foot is planted
    /// where it stands.
    pub animated: [FootPose; 2],
    pub dt: f32,
}

/// Where a foot goes this frame, in the model's frame.
#[derive(Debug, Copy, Clone, PartialEq)]
pub enum FootTarget {
    Planted(FootPose),
    /// On its arc, `progress` of the way, `0..=1`.
    Swinging {
        pose: FootPose,
        progress: f32,
    },
    Unsupported,
}

impl FootTarget {
    pub fn pose(&self) -> Option<FootPose> {
        match *self {
            FootTarget::Planted(pose) | FootTarget::Swinging { pose, .. } => Some(pose),
            FootTarget::Unsupported => None,
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            FootTarget::Planted(_) => "planted",
            FootTarget::Swinging { .. } => "swinging",
            FootTarget::Unsupported => "unsupported",
        }
    }
}

/// One frame's result.
#[derive(Debug, Clone, PartialEq)]
pub struct ContactOutput {
    pub feet: [FootTarget; 2],
    /// The walk took over this frame: the contacts are dropped.
    pub handover: Option<Handover>,
    pub events: Vec<ContactEvent>,
}

impl ContactOutput {
    /// How many feet are planted.
    pub fn planted(&self) -> usize {
        self.feet
            .iter()
            .filter(|f| matches!(f, FootTarget::Planted(_)))
            .count()
    }
}

/// A foot's state.
#[derive(Debug, Copy, Clone, PartialEq)]
pub enum FootState {
    Planted {
        /// The pose in the support's frame.
        local: FootPose,
        support: SupportId,
    },
    Swinging(Swing),
    Unsupported,
}

/// The two feet and the timers of the rules that release them.
#[derive(Debug, Clone, PartialEq)]
pub struct FootContacts {
    feet: [FootState; 2],
    /// How long the twist has held past its threshold, seconds.
    twist_held: f32,
    /// How long each foot has stood off its stance spot, seconds.
    tidy_held: [f32; 2],
    /// How far something other than the player has carried the root since the last landing.
    drift: Vec2,
    /// The foot that stepped last.
    last_stepped: Option<Side>,
    /// The foot to step at once after the other landed, if it is off its stance spot.
    follow_up: Option<Side>,
    steps: u64,
}

impl FootContacts {
    pub const fn new() -> Self {
        Self {
            feet: [FootState::Unsupported, FootState::Unsupported],
            twist_held: 0.0,
            tidy_held: [0.0; 2],
            drift: Vec2::ZERO,
            last_stepped: None,
            follow_up: None,
            steps: 0,
        }
    }

    /// Drop every contact: a teleport, a recenter, a change of mode, or a hand-over. The feet
    /// are acquired afresh where they stand on the next frame the model runs.
    pub fn reset(&mut self) {
        *self = Self {
            steps: self.steps,
            ..Self::new()
        };
    }

    /// Steps taken since the model was made.
    pub fn steps(&self) -> u64 {
        self.steps
    }

    pub fn state(&self, side: Side) -> &FootState {
        &self.feet[side.index()]
    }

    /// Advance one frame.
    pub fn update(
        &mut self,
        frame: &ContactFrame,
        world: &mut impl Surroundings,
        tuning: &ContactTuning,
    ) -> ContactOutput {
        let dt = if frame.dt.is_finite() {
            frame.dt.max(0.0)
        } else {
            0.0
        };
        let mut events = Vec::new();
        if frame.external_shift.is_finite() {
            self.drift += frame.external_shift;
        }
        let mut poses = self.resolve(world, &mut events);
        self.advance_swings(frame, world, tuning, dt, &mut poses, &mut events);
        self.acquire(frame, world, &mut poses, &mut events);
        if let Some(handover) = self.release(frame, world, tuning, dt, &mut poses, &mut events) {
            self.reset();
            return ContactOutput {
                feet: [FootTarget::Unsupported; 2],
                handover: Some(handover),
                events,
            };
        }
        let feet = Side::BOTH.map(
            |side| match (&self.feet[side.index()], poses[side.index()]) {
                (FootState::Planted { .. }, Some(pose)) => FootTarget::Planted(pose),
                (FootState::Swinging(swing), Some(pose)) => FootTarget::Swinging {
                    pose,
                    progress: swing.progress(),
                },
                _ => FootTarget::Unsupported,
            },
        );
        ContactOutput {
            feet,
            handover: None,
            events,
        }
    }

    /// Each planted foot in the model's frame, through its support's frame; a foot whose support
    /// is gone is unsupported.
    fn resolve(
        &mut self,
        world: &impl Surroundings,
        events: &mut Vec<ContactEvent>,
    ) -> [Option<FootPose>; 2] {
        Side::BOTH.map(|side| {
            let foot = &mut self.feet[side.index()];
            match *foot {
                FootState::Planted { local, support } => {
                    match world.frame(support).filter(SupportFrame::is_finite) {
                        Some(frame) => Some(frame.to_model(local)),
                        None => {
                            *foot = FootState::Unsupported;
                            events.push(ContactEvent::Lost { side });
                            None
                        }
                    }
                }
                FootState::Swinging(_) | FootState::Unsupported => None,
            }
        })
    }

    /// Carry each swing along its arc, and plant it at its landing, or leave it unsupported where
    /// the landing has no ground.
    fn advance_swings(
        &mut self,
        frame: &ContactFrame,
        world: &mut impl Surroundings,
        tuning: &ContactTuning,
        dt: f32,
        poses: &mut [Option<FootPose>; 2],
        events: &mut Vec<ContactEvent>,
    ) {
        for side in Side::BOTH {
            let FootState::Swinging(mut swing) = self.feet[side.index()] else {
                continue;
            };
            if swing.landing == Landing::Pending {
                swing.landing = match world.probe(side, swing.to.flat(), swing.to.ankle.y) {
                    Probe::Hit(ground) => Landing::Known(ground),
                    Probe::Miss => Landing::Missed,
                    Probe::Pending => Landing::Pending,
                };
            }
            swing.advance(dt);
            if !swing.arrived() {
                poses[side.index()] = Some(swing.pose(frame.ankle_height_m));
                self.feet[side.index()] = FootState::Swinging(swing);
                continue;
            }
            match swing.landing {
                Landing::Known(ground) => match world.frame(ground.support) {
                    Some(support) if support.is_finite() => {
                        let pose = swing.landed(ground, frame.ankle_height_m);
                        self.feet[side.index()] = FootState::Planted {
                            local: support.to_local(pose),
                            support: ground.support,
                        };
                        poses[side.index()] = Some(pose);
                        self.drift = Vec2::ZERO;
                        self.twist_held = 0.0;
                        self.tidy_held = [0.0; 2];
                        events.push(ContactEvent::Landed { side });
                        if swing.reason != Release::Tidy && swing.reason != Release::FollowUp {
                            self.follow_up = Some(side.other());
                        }
                    }
                    _ => {
                        self.feet[side.index()] = FootState::Unsupported;
                        events.push(ContactEvent::Lost { side });
                    }
                },
                Landing::Pending if swing.waited < tuning.landing_wait_s => {
                    poses[side.index()] = Some(swing.pose(frame.ankle_height_m));
                    self.feet[side.index()] = FootState::Swinging(swing);
                }
                Landing::Pending | Landing::Missed => {
                    self.feet[side.index()] = FootState::Unsupported;
                    events.push(ContactEvent::Missed { side });
                }
            }
        }
    }

    /// Plant each unsupported foot where the animation has it, on the ground under it.
    fn acquire(
        &mut self,
        frame: &ContactFrame,
        world: &mut impl Surroundings,
        poses: &mut [Option<FootPose>; 2],
        events: &mut Vec<ContactEvent>,
    ) {
        for side in Side::BOTH {
            if self.feet[side.index()] != FootState::Unsupported {
                continue;
            }
            let animated = frame.animated[side.index()];
            if !animated.is_finite() {
                continue;
            }
            let Probe::Hit(ground) = world.probe(side, animated.flat(), animated.ankle.y) else {
                continue;
            };
            let Some(support) = world.frame(ground.support).filter(SupportFrame::is_finite) else {
                continue;
            };
            let pose = FootPose {
                ankle: Vec3::new(
                    animated.ankle.x,
                    ground.height + frame.ankle_height_m,
                    animated.ankle.z,
                ),
                yaw: wrap(animated.yaw),
            };
            self.feet[side.index()] = FootState::Planted {
                local: support.to_local(pose),
                support: ground.support,
            };
            poses[side.index()] = Some(pose);
            events.push(ContactEvent::Acquired { side });
        }
    }

    /// Release at most one planted foot into a step, by the rules, or name the hand-over when the
    /// movement is a walk.
    fn release(
        &mut self,
        frame: &ContactFrame,
        world: &mut impl Surroundings,
        tuning: &ContactTuning,
        dt: f32,
        poses: &mut [Option<FootPose>; 2],
        events: &mut Vec<ContactEvent>,
    ) -> Option<Handover> {
        let planted = Side::BOTH.map(|side| match self.feet[side.index()] {
            FootState::Planted { .. } => poses[side.index()],
            _ => None,
        });
        let need = needs(
            frame,
            planted,
            self.drift,
            &mut self.twist_held,
            &mut self.tidy_held,
            dt,
            tuning,
        );
        let swinging = Side::BOTH
            .into_iter()
            .find(|side| matches!(self.feet[side.index()], FootState::Swinging(_)));
        if let Some(swinging) = swinging {
            let other = swinging.other();
            return (need.reach_excess[other.index()] > tuning.outrun_m)
                .then_some(Handover::Outrun);
        }
        let speed = frame.pelvis_velocity.length();
        if need.urgent[0] && need.urgent[1] && speed > tuning.walk_speed_mps {
            return Some(Handover::BothFeet);
        }
        let follow_up = self.follow_up.take().filter(|side| {
            planted[side.index()].is_some()
                && off_stance(frame, *side, planted[side.index()], tuning, 0.5)
        });
        let (side, reason) = match follow_up {
            Some(side) => (side, Release::FollowUp),
            None => self.choose(&need, planted)?,
        };
        let from = planted[side.index()]?;
        let swing = plan(frame, side, from, world, tuning, reason);
        let length_m = (swing.to.flat() - from.flat()).length();
        if length_m > tuning.max_step_m {
            return Some(Handover::StepTooLong);
        }
        self.feet[side.index()] = FootState::Swinging(swing);
        poses[side.index()] = Some(from);
        self.steps += 1;
        self.last_stepped = Some(side);
        self.twist_held = 0.0;
        self.tidy_held[side.index()] = 0.0;
        events.push(ContactEvent::Released {
            side,
            reason,
            length_m,
        });
        None
    }

    /// The foot to release and why, from what the rules found, alternating with the foot that
    /// stepped last when both want to.
    fn choose(&self, need: &Need, planted: [Option<FootPose>; 2]) -> Option<(Side, Release)> {
        let wanted = Side::BOTH
            .map(|side| need.reasons[side.index()].filter(|_| planted[side.index()].is_some()));
        let both = wanted[0].is_some() && wanted[1].is_some();
        let pick = match (need.first, self.last_stepped) {
            (Some(first), Some(last)) if both && first == last => first.other(),
            (Some(first), _) if wanted[first.index()].is_some() => first,
            _ => Side::BOTH
                .into_iter()
                .find(|side| wanted[side.index()].is_some())?,
        };
        wanted[pick.index()].map(|reason| (pick, reason))
    }
}

impl Default for FootContacts {
    fn default() -> Self {
        Self::new()
    }
}

/// The spot `side`'s ankle stands on in the stance under the pelvis, `(x, z)`, and the foot's
/// heading there.
pub fn stance_spot(frame: &ContactFrame, side: Side) -> (Vec2, f32) {
    let flat = Vec2::new(frame.pelvis.x, frame.pelvis.z);
    let half = frame.stance.width_m.max(0.0) * 0.5;
    let toe_out = frame.stance.toe_out_deg.to_radians();
    let spot = flat + right(frame.pelvis_yaw) * half * side.across();
    let yaw = wrap(frame.pelvis_yaw - toe_out * side.across());
    (spot, yaw)
}

/// Whether the foot at `pose` stands further from its stance spot than `share` of the tidy's
/// distance or angle.
fn off_stance(
    frame: &ContactFrame,
    side: Side,
    pose: Option<FootPose>,
    tuning: &ContactTuning,
    share: f32,
) -> bool {
    let Some(pose) = pose else {
        return false;
    };
    let (spot, yaw) = stance_spot(frame, side);
    let off = (pose.flat() - spot).length();
    let turned = wrap(pose.yaw - yaw).abs();
    off > tuning.tidy_m * share || turned > tuning.tidy_deg.to_radians() * share
}

/// The turn about +Y a rotation makes of a heading.
fn yaw_turn(rotation: Quat) -> f32 {
    crate::angle::yaw_of(rotation * Vec3::NEG_Z).unwrap_or(0.0)
}
