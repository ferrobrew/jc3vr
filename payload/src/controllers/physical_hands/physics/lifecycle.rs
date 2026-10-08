//! The hand bodies' own state, owned by the pre-simulation step: each hand's body, whether it is in
//! the simulation, where it was last sent, and its joint, and the transitions between them: made,
//! added, driven, held in place, parked, and destroyed.

use std::time::Instant;

use glam::Vec3;
use parking_lot::Mutex;

use crate::controllers::{
    frame::{Hand, Pose},
    physical_hands::physics::{
        BodyPhase, HandBodyStatus, HoldTarget,
        body::{BodyError, BodySpec, HandBody},
        contact,
        drive::{DriveTuning, Goal, drive},
        joint::Joint,
        render::RenderOffset,
    },
};

/// One hand's drive request.
pub(super) struct ActRequest<'a> {
    pub hand: Hand,
    pub spec: BodySpec,
    /// Where the body is driven: the controller's palm at the end of the step, or a grip's pin.
    pub goal: Goal,
    /// Where the controller's palm will be at the end of the step, which the hand is drawn
    /// against.
    pub palm: Goal,
    pub render: &'a RenderTuning,
    pub break_distance_m: f32,
    pub tuning: &'a DriveTuning,
    pub step_s: f32,
}

/// How a hand is drawn at its body.
pub(super) struct RenderTuning {
    pub max_shift_m: f32,
    pub deadband_rad: f32,
    pub max_turn_rad: f32,
}

/// What driving one hand did.
pub(super) struct Acted {
    pub status: HandBodyStatus,
    /// How far the body was short of where it was last sent, world space.
    pub lag: Vec3,
    /// The offset from the controller's palm, as the last step was sent it, to where the body is
    /// now: what draws the hand at its body.
    pub render: RenderOffset,
    pub created: bool,
    pub destroyed: bool,
    /// Whether the body's joint ended against the grip's wish.
    pub joint_lost: bool,
}

/// The bodies, owned by the step.
pub(super) struct Hands {
    pub bodies: [Option<HandBody>; 2],
    /// Each body's joint to what its grip holds.
    pub joints: [Option<Joint>; 2],
    /// Whether each body is in the simulation.
    added: [bool; 2],
    /// Where each body was last sent, world space.
    goals: [Option<Goal>; 2],
    /// Where each controller's palm was predicted for the end of the last step, world space.
    palms: [Option<Goal>; 2],
    /// When each hand stopped acting.
    pub missing_since: [Option<Instant>; 2],
    resets: [u64; 2],
    pub steps: u64,
    pub last_log: Option<Instant>,
}

pub(super) static HANDS: Mutex<Hands> = Mutex::new(Hands {
    bodies: [None, None],
    joints: [None, None],
    added: [false; 2],
    goals: [None; 2],
    palms: [None; 2],
    missing_since: [None; 2],
    resets: [0; 2],
    steps: 0,
    last_log: None,
});

impl Hands {
    pub(super) fn phase(&self, hand: Hand) -> BodyPhase {
        let i = hand.index();
        match (&self.bodies[i], self.added[i]) {
            (None, _) => BodyPhase::Absent,
            (Some(_), false) => BodyPhase::Parked,
            (Some(_), true) => BodyPhase::Acting,
        }
    }

    /// Make, add, and drive one hand's body toward `request.goal`.
    pub(super) unsafe fn act(&mut self, request: ActRequest<'_>) -> Result<Acted, BodyError> {
        let ActRequest {
            hand,
            spec,
            goal,
            palm,
            render,
            break_distance_m,
            tuning,
            step_s,
        } = request;
        let i = hand.index();
        let mut destroyed = false;
        if let Some(existing) = &self.bodies[i]
            && let Some(reason) = incompatible(&existing.spec(), &spec)
        {
            // SAFETY: forwarded from the caller.
            destroyed = unsafe { self.destroy(hand, reason) };
        }
        let mut created = false;
        let body = match &mut self.bodies[i] {
            Some(body) => body,
            empty @ None => {
                // SAFETY: forwarded from the caller.
                let body =
                    unsafe { HandBody::create(hand, spec, goal.position, goal.orientation) }?;
                let filter = unsafe { body.filter_info() }.ok();
                let contact_events = unsafe { body.raises_contact_events() }.ok();
                tracing::info!(
                    hand = hand.label(),
                    body_id = body.body_id(),
                    filter_info = ?filter.map(|f| format!("{f:#010x}")),
                    system_group = spec.system_group,
                    mass_kg = spec.mass_kg,
                    radius_m = spec.capsule.radius_m,
                    half_length_m = spec.capsule.half_length_m,
                    listens = spec.contact_events,
                    contact_events = ?contact_events,
                    "hand bodies: created a hand body"
                );
                self.added[i] = false;
                created = true;
                empty.insert(body)
            }
        };
        if !self.added[i] {
            // Added, then placed, as the engine activates its own free bodies: a parked body comes
            // back at its hand, at rest.
            // SAFETY: forwarded from the caller.
            unsafe { body.add() };
            self.added[i] = true;
            self.goals[i] = None;
            self.palms[i] = None;
            unsafe { body.place(goal.position, goal.orientation) }?;
            tracing::info!(
                hand = hand.label(),
                body_id = body.body_id(),
                in_world = unsafe { body.is_added() },
                "hand bodies: added a hand body to the world"
            );
        }
        let motion = match unsafe { body.motion() } {
            Ok(motion) => motion,
            Err(BodyError::Lost) => {
                tracing::warn!(
                    hand = hand.label(),
                    body_id = body.body_id(),
                    "hand bodies: a hand body's slot no longer holds it; forgetting it"
                );
                // SAFETY: forwarded from the caller.
                unsafe { self.unjoin(hand, "the hand body was lost") };
                if let Some(body) = self.bodies[i].take() {
                    body.abandon(hand);
                }
                self.added[i] = false;
                self.goals[i] = None;
                self.palms[i] = None;
                return Err(BodyError::Lost);
            }
            Err(e) => return Err(e),
        };
        let lag = self.goals[i].map_or(Vec3::ZERO, |sent| sent.position - motion.position);
        let drawn = self.palms[i].map_or(RenderOffset::NONE, |sent| {
            RenderOffset::between(
                Pose {
                    position: sent.position,
                    orientation: sent.orientation,
                },
                Pose {
                    position: motion.position,
                    orientation: motion.orientation,
                },
                render.max_shift_m,
                render.deadband_rad,
                render.max_turn_rad,
            )
        });
        self.palms[i] = Some(palm);
        let mut status = HandBodyStatus {
            phase: BodyPhase::Acting,
            body_id: Some(body.body_id()),
            filter_info: unsafe { body.filter_info() }.ok(),
            lag_m: lag.length(),
            speed_mps: motion.linear.length(),
            force_share: 0.0,
            torque_share: 0.0,
            resets: self.resets[i],
            joint: None,
        };
        if (goal.position - motion.position).length() > break_distance_m {
            // A body put back on its hand would drag what it is joined to with it.
            // SAFETY: forwarded from the caller.
            let joint_lost = unsafe {
                end_joint(
                    &mut self.joints[i],
                    hand,
                    "the hand body fell too far behind",
                )
            };
            unsafe { body.place(goal.position, goal.orientation) }?;
            contact::record_motion(hand, goal.position, Vec3::ZERO);
            self.resets[i] += 1;
            status.resets = self.resets[i];
            self.goals[i] = Some(goal);
            tracing::debug!(
                hand = hand.label(),
                "hand bodies: a hand body fell too far behind and was put back on its hand"
            );
            return Ok(Acted {
                status,
                lag: Vec3::ZERO,
                render: RenderOffset::NONE,
                created,
                destroyed,
                joint_lost,
            });
        }
        let driven = drive(&motion, &goal, tuning, step_s);
        unsafe { body.set_velocities(driven.linear, driven.angular) }?;
        contact::record_motion(hand, motion.position, driven.linear);
        self.goals[i] = Some(goal);
        status.force_share = driven.force_share;
        status.torque_share = driven.torque_share;
        Ok(Acted {
            status,
            lag,
            render: drawn,
            created,
            destroyed,
            joint_lost: false,
        })
    }

    /// Make, keep, or end `hand`'s joint so that it holds `wanted`. Returns whether a hold the grip
    /// wanted was lost: its joint ended or could not be made.
    pub(super) unsafe fn join(&mut self, hand: Hand, wanted: Option<HoldTarget>) -> bool {
        let i = hand.index();
        if let Some(joint) = &self.joints[i] {
            let same = wanted.is_some_and(|w| joint.target() == (w.object, w.body));
            // SAFETY: forwarded from the caller.
            if same && unsafe { joint.intact() } {
                return false;
            }
            let reason = if same {
                "the constraint or the object's body went away"
            } else {
                "the grip let go"
            };
            // SAFETY: forwarded from the caller.
            unsafe { self.unjoin(hand, reason) };
            if same {
                return true;
            }
        }
        let Some(wanted) = wanted else {
            return false;
        };
        let Some(body) = self.bodies[i].as_ref().filter(|_| self.added[i]) else {
            return true;
        };
        // SAFETY: forwarded from the caller; the body is in the simulation.
        match unsafe { Joint::create(body.body_id(), wanted.body, wanted.object) } {
            Ok(joint) => {
                tracing::info!(
                    hand = hand.label(),
                    constraint = joint.id(),
                    hand_body = body.body_id(),
                    object_body = wanted.body,
                    "hand bodies: joined a hand to what it holds"
                );
                self.joints[i] = Some(joint);
                false
            }
            Err(e) => {
                tracing::warn!(
                    hand = hand.label(),
                    object_body = wanted.body,
                    error = %e,
                    "hand bodies: could not join a hand to what it holds"
                );
                true
            }
        }
    }

    /// End `hand`'s joint, if it has one. Returns whether it had one.
    pub(super) unsafe fn unjoin(&mut self, hand: Hand, reason: &str) -> bool {
        // SAFETY: forwarded from the caller.
        unsafe { end_joint(&mut self.joints[hand.index()], hand, reason) }
    }

    /// Keep a hand's body where it is for a moment without driving it toward its hand.
    pub(super) unsafe fn hold(&mut self, hand: Hand) {
        let i = hand.index();
        if let Some(body) = self.bodies[i].as_mut()
            && self.added[i]
        {
            let _ = unsafe { body.set_velocities(Vec3::ZERO, Vec3::ZERO) };
        }
        self.goals[i] = None;
        self.palms[i] = None;
    }

    /// Take a hand's body out of the simulation.
    pub(super) unsafe fn park(&mut self, hand: Hand, reason: &str) {
        let i = hand.index();
        // SAFETY: forwarded from the caller.
        unsafe { self.unjoin(hand, reason) };
        self.goals[i] = None;
        self.palms[i] = None;
        let Some(body) = self.bodies[i].as_mut() else {
            return;
        };
        if !self.added[i] {
            return;
        }
        unsafe { body.remove() };
        self.added[i] = false;
        tracing::info!(
            hand = hand.label(),
            body_id = body.body_id(),
            reason,
            "hand bodies: removed a hand body from the world"
        );
    }

    /// Destroy a hand's body, after its joint. Returns whether there was one.
    unsafe fn destroy(&mut self, hand: Hand, reason: &str) -> bool {
        let i = hand.index();
        // SAFETY: forwarded from the caller.
        unsafe { self.unjoin(hand, reason) };
        self.added[i] = false;
        self.goals[i] = None;
        self.palms[i] = None;
        self.missing_since[i] = None;
        let Some(body) = self.bodies[i].take() else {
            return false;
        };
        let body_id = body.body_id();
        // SAFETY: forwarded from the caller.
        if unsafe { body.destroy(hand) } {
            tracing::info!(
                hand = hand.label(),
                body_id,
                reason,
                "hand bodies: destroyed a hand body"
            );
        } else {
            tracing::warn!(
                hand = hand.label(),
                body_id,
                reason,
                "hand bodies: a hand body's slot no longer holds it; forgot it without destroying it"
            );
        }
        true
    }

    /// Destroy both hands' bodies. Returns how many there were.
    pub(super) unsafe fn destroy_all(&mut self, reason: &str) -> u64 {
        Hand::BOTH
            .into_iter()
            .map(|hand| u64::from(unsafe { self.destroy(hand, reason) }))
            .sum()
    }
}

/// End the joint in `slot`, `hand`'s, if there is one. Returns whether there was one.
///
/// # Safety
///
/// The thread that steps the physics world, outside the step.
unsafe fn end_joint(slot: &mut Option<Joint>, hand: Hand, reason: &str) -> bool {
    let Some(joint) = slot.take() else {
        return false;
    };
    let constraint = joint.id();
    // SAFETY: forwarded from the caller.
    let destroyed = unsafe { joint.destroy() };
    tracing::info!(
        hand = hand.label(),
        constraint,
        destroyed,
        reason,
        "hand bodies: ended a joint"
    );
    true
}

/// Why a body made to `existing` cannot serve as one made to `wanted`, if it cannot.
fn incompatible(existing: &BodySpec, wanted: &BodySpec) -> Option<&'static str> {
    if existing.system_group != wanted.system_group {
        return Some("the player's collision group changed");
    }
    if existing.contact_events != wanted.contact_events {
        return Some("the hits were switched on or off");
    }
    let close = |a: f32, b: f32| (a - b).abs() <= SPEC_TOLERANCE * a.abs().max(b.abs());
    if !(close(existing.mass_kg, wanted.mass_kg)
        && close(existing.capsule.radius_m, wanted.capsule.radius_m)
        && close(existing.capsule.half_length_m, wanted.capsule.half_length_m))
    {
        return Some("the hand's size or mass changed");
    }
    None
}

/// The relative change in a body's size or mass that remakes it.
const SPEC_TOLERANCE: f32 = 0.02;
