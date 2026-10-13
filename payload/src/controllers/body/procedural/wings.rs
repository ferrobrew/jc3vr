//! The wingsuit's arms at the solve gate: the player's tracked arms read before the gate takes its
//! state ([`read_player_arms`]), carried over onto Rico's flight pose ([`solver::wings`]) with a
//! weight that eases in and out with the wingsuit, and queued as wrist targets once the gate has let
//! go of its state ([`queue`]). The body stays the flight pose: the regions are the animation's.
//!
//! [`solver::wings`]: crate::controllers::body::procedural::solver::wings

use std::sync::atomic::{AtomicU8, Ordering};

use glam::Mat4;
use jc3gi::character::character::Character;

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
        hands::{
            ik::queue_model_wrists,
            pose::cockpit_hand,
            reach::{torso_heading, yaw_only},
        },
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
    if !config.enabled
        || !config.body.procedural.wingsuit_arms
        || !scheme::is_controllers()
        || headpose::source() != Source::Vr
    {
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
    let torso = yaw_only(torso_heading(head, shoulder_heading()));
    let player = body::player();
    Some(PlayerArms {
        torso,
        shoulders: body::shoulder_positions(head.position, torso, &player),
        wrists,
        arm_length: player.upper_arm + player.forearm,
    })
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
        let wanted = case == Case::Wingsuit && config.wingsuit_arms && player.is_some();
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
