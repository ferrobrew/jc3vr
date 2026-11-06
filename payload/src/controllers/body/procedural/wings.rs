//! The wingsuit's arms at the solve gate: the player's tracked arms read before the gate takes its
//! state ([`read_player_arms`]), carried over onto Rico's flight pose ([`solver::wings`]) with a
//! weight that eases in and out with the wingsuit, and queued as wrist targets once the gate has let
//! go of its state ([`queue`]). The body stays the flight pose: the regions are the animation's.
//!
//! The player's shoulders are estimated about the torso's heading, and in flight there is no
//! grounded rig to estimate it, so the arms read it from themselves ([`WingHeading`]): spread wide,
//! the line between the wrists faces where the torso does, and otherwise a slow follower of the
//! head turns only for a sustained turn. The head's heading alone would turn the shoulders with
//! every look at an arm, and bend the arm being looked at.
//!
//! [`solver::wings`]: crate::controllers::body::procedural::solver::wings

use std::{
    sync::atomic::{AtomicU8, Ordering},
    time::{Duration, Instant},
};

use body_rig::{
    angle::{head_heading, line_yaw, wrap},
    rotation::level,
    yaw::{YawFollow, YawInput, YawTuning},
};
use glam::{Mat4, Quat, Vec3};
use jc3gi::character::character::Character;
use parking_lot::Mutex;

use crate::{
    config::Config,
    controllers::{
        body::{
            self,
            procedural::{
                authority::Case,
                config::ProceduralBodyConfig,
                gate_state::State,
                rig::{Rig, RigBone},
                solver::{
                    arms::Side,
                    clavicles::shoulder_joint,
                    pose::RigPose,
                    wings::{PlayerArms, RicoArms, WingTarget, flight_heading, wing_targets},
                },
            },
            rig::shoulder_heading,
        },
        frame::{self, Hand, Pose},
        hands::{ik::queue_model_wrists, pose::cockpit_hand},
        scheme,
    },
    headpose::{self, Source},
};

/// The wrist targets the gate placed for the wingsuit's arms this tick, to queue once it has let go
/// of its state.
#[derive(Debug, Copy, Clone, PartialEq)]
pub(super) struct WingQueue {
    pub(super) targets: [Option<WingTarget>; 2],
    pub(super) weight: f32,
}

/// Which hands the wingsuit's arms queued targets for at the last gate, by [`Hand::index`]: the
/// body IK does not pin those wrists to the animation.
pub fn wing_arms() -> [bool; 2] {
    let bits = WING_HANDS.load(Ordering::Relaxed);
    Hand::BOTH.map(|hand| bits & (1 << hand.index()) != 0)
}

/// The player's arms for the wingsuit, when the controller scheme drives the hands in the headset
/// and the mode detection has the player in the wingsuit. Takes the configuration lock, the
/// controller frame slot, and the rig's torso estimate, one after the other, so the gate reads it
/// before it takes its own state.
pub(super) fn read_player_arms() -> Option<PlayerArms> {
    let config = Config::lock_query(|c| c.controllers);
    if !config.enabled || !scheme::is_controllers() || headpose::source() != Source::Vr {
        return None;
    }
    let cockpit = headpose::xr::cockpit_pose()?;
    let head = Pose {
        position: cockpit.position,
        orientation: cockpit.orientation,
    };
    let wrists = frame::with_latest(|frame| {
        Hand::BOTH.map(|hand| {
            let h = frame.hand(hand);
            (frame.focused && h.active && h.grip.is_valid()).then(|| {
                let aim = if h.aim.is_valid() {
                    h.aim.pose
                } else {
                    h.grip.pose
                };
                // The player's own hand, not the remap's: the arm is scaled to Rico's below.
                cockpit_hand(hand, h.grip.pose, aim, &config.hands.grip, None).wrist
            })
        })
    })?;
    let torso = match shoulder_heading() {
        Some(torso) => level(torso),
        None => airborne_torso_heading(head.orientation, wrists.map(|w| w.map(|w| w.position))),
    };
    let player = body::player();
    Some(PlayerArms {
        torso,
        shoulders: body::shoulder_positions(head.position, torso, &player),
        wrists,
        arm_length: player.shoulder_to_wrist(),
    })
}

/// The torso's heading in flight ([`WingHeading`]) for the head's cockpit orientation `head` and
/// the player's tracked wrists `wrists` (cockpit frame, by [`Hand::index`]). Every in-flight
/// estimate of the player's shoulders reads this one follower, so the arms, the hands' shoulders,
/// the arm remap, and the ripcord agree.
pub(crate) fn airborne_torso_heading(head: Quat, wrists: [Option<Vec3>; 2]) -> Quat {
    WING_HEADING.lock().update(head, wrists, Instant::now())
}

impl State {
    /// Ease the wingsuit's arms in or out for `case`, and place their wrist targets from `player`
    /// on the animated pose `animated` of the root at `world`, for [`queue`] after the gate.
    pub(super) fn step_wings(
        &mut self,
        request: &WingRequest<'_>,
        config: &ProceduralBodyConfig,
    ) -> Option<WingQueue> {
        let WingRequest {
            case,
            player,
            rig,
            animated,
            world,
            dt,
        } = *request;
        let wanted = case == Case::Wingsuit && player.is_some();
        let weight = self.wings.step(
            if wanted { 1.0 } else { 0.0 },
            dt,
            config.blend_s,
            config.blend_out_s,
        );
        self.status.wing_weight = weight;
        if weight <= 0.0 {
            self.status.wing_hands = [false; 2];
            self.wing_logged = false;
            self.last_wings = None;
            return None;
        }
        // Easing out after the wingsuit, the last targets fade where the arms last were.
        let targets = match (player, self.last_wings) {
            (Some(player), _) if case == Case::Wingsuit => {
                let rico = RicoArms {
                    world: *world,
                    shoulders: Side::BOTH.map(|side| shoulder_joint(animated, rig, side)),
                    arm_lengths: Side::BOTH.map(|side| rig.arm(side).length),
                    heading: flight_heading(
                        world,
                        animated.position(RigBone::Hips),
                        animated.position(RigBone::Neck),
                    )?,
                };
                wing_targets(player, &rico)
            }
            (_, Some(last)) => last,
            _ => return None,
        };
        self.last_wings = Some(targets);
        self.status.wing_hands = targets.map(|t| t.is_some());
        if !self.wing_logged && targets.iter().any(Option::is_some) {
            self.wing_logged = true;
            tracing::info!(
                target: "body",
                left = targets[0].is_some(),
                right = targets[1].is_some(),
                "procedural body: Rico's arms follow the player's in the wingsuit",
            );
        }
        Some(WingQueue { targets, weight })
    }
}

/// What the wingsuit's arms are placed from at the gate.
#[derive(Clone, Copy)]
pub(super) struct WingRequest<'a> {
    pub(super) case: Case,
    pub(super) player: Option<&'a PlayerArms>,
    pub(super) rig: &'a Rig,
    pub(super) animated: &'a RigPose,
    /// The root's world transform.
    pub(super) world: &'a Mat4,
    pub(super) dt: f32,
}

/// Queue the wingsuit's wrist targets the gate placed, and publish which hands were queued.
///
/// # Safety
///
/// As [`queue_model_wrists`].
pub(super) unsafe fn queue(character: &mut Character, wings: Option<WingQueue>) -> [bool; 2] {
    let queued = match wings {
        Some(wings) => unsafe {
            queue_model_wrists(
                character,
                wings.targets.map(|t| t.map(|t| (t.position, t.rotation))),
                wings.weight,
            )
        },
        None => [false; 2],
    };
    let bits = Hand::BOTH
        .into_iter()
        .filter(|hand| queued[hand.index()])
        .fold(0u8, |bits, hand| bits | (1 << hand.index()));
    WING_HANDS.store(bits, Ordering::Relaxed);
    queued
}

/// The hands [`wing_arms`] returns, a bit per [`Hand::index`].
static WING_HANDS: AtomicU8 = AtomicU8::new(0);

/// The torso's heading in flight, read from the arms: the line between the wrists while they are
/// spread at least [`SPREAD_M`] apart, and otherwise a follower of the head that turns only for a
/// sustained turn ([`FOLLOW`]), slowly.
#[derive(Debug, Copy, Clone, PartialEq, Default)]
pub(crate) struct WingHeading {
    follow: YawFollow,
    last: Option<Instant>,
}

impl WingHeading {
    pub(crate) const fn new() -> Self {
        Self {
            follow: YawFollow::new(),
            last: None,
        }
    }

    /// The torso's heading for the head's cockpit orientation `head` and the tracked wrists
    /// `wrists` (cockpit frame, by [`Hand::index`]) at `now`. A gap longer than [`RESUME_AFTER`]
    /// starts again from the head's heading.
    pub(crate) fn update(&mut self, head: Quat, wrists: [Option<Vec3>; 2], now: Instant) -> Quat {
        let dt = match self.last {
            Some(last) if now.saturating_duration_since(last) <= RESUME_AFTER => {
                now.saturating_duration_since(last).as_secs_f32()
            }
            _ => {
                self.follow = YawFollow::new();
                0.0
            }
        };
        self.last = Some(now);
        let Some(head_yaw) = head_heading(head) else {
            return Quat::from_rotation_y(self.follow.yaw().unwrap_or(0.0));
        };
        if let Some(spread) = spread_heading(wrists, head_yaw) {
            self.follow.snap(spread);
            return Quat::from_rotation_y(spread);
        }
        let yaw = self.follow.update(
            &YawInput {
                head_yaw,
                head_pitch: 0.0,
                hands: None,
                moving: false,
                dt,
            },
            &FOLLOW,
        );
        Quat::from_rotation_y(yaw)
    }
}

/// The heading the line between the wrists faces, when both are tracked and spread at least
/// [`SPREAD_M`] apart across the ground, and it faces within a right angle of the head's heading
/// `head_yaw`: crossed arms face backward, and say nothing.
fn spread_heading(wrists: [Option<Vec3>; 2], head_yaw: f32) -> Option<f32> {
    let [Some(left), Some(right)] = wrists else {
        return None;
    };
    let across = (right - left) * Vec3::new(1.0, 0.0, 1.0);
    if !across.is_finite() || across.length() < SPREAD_M {
        return None;
    }
    line_yaw(left, right).filter(|yaw| wrap(yaw - head_yaw).abs() < std::f32::consts::FRAC_PI_2)
}

/// The wrists spread at least this far apart across the ground give the torso's heading, metres.
const SPREAD_M: f32 = 0.8;

/// The follower of the head in flight: no turn for a look alone, however far, a slow turn after a
/// look held to one side, and a slow settle under a steady head.
const FOLLOW: YawTuning = YawTuning {
    turn_deg: 180.0,
    sustain_deg: 35.0,
    sustain_s: 1.5,
    slew_deg_s: 90.0,
    ..YawTuning::new()
};

/// A gap between readings longer than this starts the follower again, a few ticks.
const RESUME_AFTER: Duration = Duration::from_millis(500);

static WING_HEADING: Mutex<WingHeading> = Mutex::new(WingHeading::new());

#[cfg(test)]
mod tests {
    use body_rig::angle::yaw_of;

    use super::*;

    const DT: Duration = Duration::from_millis(11);

    fn heading(rotation: Quat) -> f32 {
        yaw_of(rotation * Vec3::NEG_Z).unwrap()
    }

    fn run(
        follower: &mut WingHeading,
        head: Quat,
        wrists: [Option<Vec3>; 2],
        at: &mut Instant,
        s: f32,
    ) -> f32 {
        let mut yaw = 0.0;
        for _ in 0..(s / DT.as_secs_f32()) as usize {
            *at += DT;
            yaw = heading(follower.update(head, wrists, *at));
        }
        yaw
    }

    const SPREAD: [Option<Vec3>; 2] = [
        Some(Vec3::new(-0.7, 1.3, 0.1)),
        Some(Vec3::new(0.7, 1.3, 0.1)),
    ];

    #[test]
    fn spread_arms_hold_the_heading_through_a_look_at_one() {
        let mut follower = WingHeading::new();
        let mut at = Instant::now();
        run(&mut follower, Quat::IDENTITY, SPREAD, &mut at, 0.5);
        // Looking at the right hand, a right angle round, for seconds.
        let look = Quat::from_rotation_y(-1.4) * Quat::from_rotation_x(-0.3);
        let yaw = run(&mut follower, look, SPREAD, &mut at, 3.0);
        assert!(yaw.abs() < 1e-3, "{yaw}");
    }

    #[test]
    fn a_glance_with_the_arms_in_turns_nothing() {
        let mut follower = WingHeading::new();
        let mut at = Instant::now();
        let tucked = [
            Some(Vec3::new(-0.25, 1.1, 0.0)),
            Some(Vec3::new(0.25, 1.1, 0.0)),
        ];
        run(&mut follower, Quat::IDENTITY, tucked, &mut at, 0.5);
        let yaw = run(
            &mut follower,
            Quat::from_rotation_y(1.4),
            tucked,
            &mut at,
            1.0,
        );
        assert!(yaw.abs() < 1e-3, "{yaw}");
        // Held, the torso comes round, slowly.
        let yaw = run(
            &mut follower,
            Quat::from_rotation_y(1.4),
            tucked,
            &mut at,
            3.0,
        );
        assert!((yaw - 1.4).abs() < 0.15, "{yaw}");
    }

    #[test]
    fn crossed_or_close_wrists_say_nothing() {
        assert_eq!(spread_heading([SPREAD[1], SPREAD[0]], 0.0), None);
        assert_eq!(
            spread_heading(
                [
                    Some(Vec3::new(-0.3, 1.3, 0.0)),
                    Some(Vec3::new(0.3, 1.3, 0.0))
                ],
                0.0
            ),
            None
        );
        assert_eq!(spread_heading([SPREAD[0], None], 0.0), None);
        assert!(spread_heading(SPREAD, 0.0).unwrap().abs() < 1e-5);
    }
}
