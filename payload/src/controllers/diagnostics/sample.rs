//! The controller state event's construction from each subsystem's live readout.
//!
//! The event holds only what the tracking frames do not: the frames already carry the
//! controllers' poses, validity and tracking flags, focus, and action set, and the STAGE
//! information, so none of those are read here.

use std::time::Instant;

use jc3gi::character::character::Character;
use tracking_format::event::{
    AimState, CharacterMotion, ControllerState, GrappleAim, GroundHeight, HandAim, HandLate,
    HandTier, IkState, Label, LateState, RoomscaleState, RootTick, TierState, WeaponAim,
};

use crate::{
    controllers::{
        aim::{self, AimIdle, HandSource, raycast::RaycastError, sights::SightSource},
        body::rig::{
            collision::probe::ProbeResult,
            root::{
                SimStatus,
                trace::{RootTrace, STALE_AFTER as TRACE_STALE_AFTER},
            },
        },
        frame::{self, Hand},
        hands::{ik, late, tiers},
        mode::{self, AirborneVariant, HoldReason, Mode, OnFootVariant, Traversal},
        roomscale,
        scheme::{self, Scheme},
    },
    hooks::animation::body_rule_system,
    recorder::tracking::format::hand_to_file,
    vr::input as vr_input,
};

/// Read every subsystem's status, and the player `character`'s measured motion, into one event.
pub(super) fn snapshot(character: Option<&Character>) -> ControllerState {
    let detection = mode::current();
    let (frame_sequence, publication_age_us) = frame::with_latest(|frame| {
        (
            Some(frame.sequence),
            Some(u64::try_from(frame.age().as_micros()).unwrap_or(u64::MAX)),
        )
    })
    .unwrap_or((None, None));
    let ik_status = ik::status();
    let roomscale_status = roomscale::status();
    ControllerState {
        frame_sequence,
        publication_age_us,
        scheme: Label::Borrowed(scheme_label(scheme::current())),
        mode: Label::Borrowed(mode_label(detection.mode)),
        traversal: detection
            .traversal
            .map(traversal_label)
            .map(Label::Borrowed),
        held: detection.held.map(hold_label).map(Label::Borrowed),
        input_error: vr_input::status().error.map(Label::Owned),
        ik: Hand::BOTH.map(|hand| ik_state(ik_status[hand.index()])),
        tiers: tier_state(tiers::status()),
        late: late_state(late::status()),
        aim: aim_state(aim::status()),
        roomscale: roomscale_state(roomscale_status),
        root: fresh_trace(&roomscale_status.root.sim, Instant::now()).map(root_tick),
        character: character.map(character_motion),
    }
}

/// The character's measured world velocity, and the height over the ground its ground information
/// last wrote to its body rule system.
fn character_motion(character: &Character) -> CharacterMotion {
    // SAFETY: the caller's character is the live local player, read on the game thread.
    let rules = unsafe { body_rule_system(character) };
    CharacterMotion {
        velocity_mps: character.m_MeasuredWorldVelocity.into(),
        height: rules.map(|rules| GroundHeight {
            nearest_m: rules.m_HeightOverGround,
            ground_m: rules.m_HeightOverGroundExcludingWater,
        }),
    }
}

fn ik_state(status: ik::HandIkStatus) -> IkState {
    IkState {
        queued: status.queued,
        skip: status.skip.map(|skip| Label::Borrowed(skip.label())),
        weight: status.weight,
        reach_distance_m: status.reach_distance,
        rotation_deg: status.rotation_deg,
        secondary_wrist_off: status.secondary_wrist_off,
        secondary_grip_off: status.secondary_grip_off,
        overrode_live_target: status.overrode_live_target,
    }
}

fn tier_state(status: tiers::TierStatus) -> TierState {
    TierState {
        state: status.state.map(|state| Label::Borrowed(state.label())),
        hands: Hand::BOTH.map(|hand| {
            let index = hand.index();
            HandTier {
                assigned: Label::Borrowed(status.tiers[index].label()),
                effective: Label::Borrowed(status.effective[index].label()),
                controller_weight: status.weights[index],
                anchor_weight: status.anchor_weights[index],
            }
        }),
    }
}

fn late_state(status: late::LateStatus) -> LateState {
    LateState {
        hands: status.hands.map(|hand| HandLate {
            reason: Label::Borrowed(hand.reason.label()),
            physical_displacement_m: hand.physical_displacement_m,
            relative_rotation_deg: hand.relative_rotation_deg,
            requested_wrist: hand.requested_wrist_position,
            reference_wrist: hand.reference_wrist_position,
            latest_wrist: hand.latest_wrist_position,
            achieved_wrist_model: hand.achieved_wrist_position_model,
        }),
        weapons_updated: status.weapons_updated,
        palettes_edited: status.palettes_edited,
    }
}

fn aim_state(status: aim::AimStatus) -> AimState {
    AimState {
        idle: status.idle.map(idle_label).map(Label::Borrowed),
        hands: Hand::BOTH.map(|hand| status.hands[hand.index()].as_ref().map(hand_aim)),
        grapple: status.grapple.map(|grapple| GrappleAim {
            hit: grapple.hit,
            valid: grapple.valid,
            tetherable: grapple.tetherable,
            point: grapple.point,
        }),
        weapon: status.weapon.map(|weapon| WeaponAim {
            name_hash: weapon.hash,
            gun_hands: weapon.gun_hands.map(|hand| hand.map(hand_to_file)),
            sight_source: Label::Borrowed(sight_source_label(weapon.sight.source)),
            sight_offset_m: weapon.sight.sight.map(|sight| sight.offset_m),
            sight_angles_deg: weapon.sight.angles_deg(),
        }),
        raycast_error: status
            .raycast_error
            .map(raycast_error_label)
            .map(Label::Borrowed),
    }
}

fn hand_aim(aim: &aim::HandAim) -> HandAim {
    let (source, gun_index) = match aim.source {
        HandSource::AimPose => ("aim_pose", None),
        HandSource::Gun { gun } => ("gun_sight", u32::try_from(gun).ok()),
    };
    HandAim {
        source: Label::Borrowed(source),
        gun_index,
        hit_distance_m: aim.hit.map(|hit| hit.distance),
        hit_object: aim.hit.is_some_and(|hit| hit.object.is_some()),
        target: aim.target,
    }
}

fn roomscale_state(status: roomscale::RoomscaleStatus) -> RoomscaleState {
    let frame = status.frame;
    RoomscaleState {
        active: status.active,
        posture: Label::Borrowed(frame.posture.label()),
        eye_drop_m: frame.eye_drop_m,
        root_owner: status.root.sim.owner.map(|o| Label::Borrowed(o.label())),
        gap_m: status.root.frame.gap_m(),
        residual_m: status.root.frame.residual_m(),
        pushback_m: status.root.frame.debt_m,
        cockpit_shift_m: status.root.frame.shift,
        chase_velocity_mps: status.root.sim.chase_velocity,
        task_velocity_mps: status.root.sim.task_velocity,
        crouched: frame.crouched,
        seated_crouch: frame.seated_crouch,
        seat: Label::Borrowed(frame.seat.label()),
        probe: frame.probe.map(probe_label).map(Label::Borrowed),
        fade_level: status.fade.level,
    }
}

/// The last on-foot tick's trace, while the root's hook still runs.
fn fresh_trace(sim: &SimStatus, now: Instant) -> Option<RootTrace> {
    sim.trace.filter(|_| {
        sim.at
            .is_some_and(|at| now.saturating_duration_since(at) <= TRACE_STALE_AFTER)
    })
}

fn root_tick(trace: RootTrace) -> RootTick {
    RootTick {
        tick: trace.tick,
        dt_s: trace.dt,
        position: trace.position,
        moved_m: trace.moved,
        unexplained_m: trace.unexplained_m,
        task_mps: trace.split.task,
        stick_mps: trace.split.stick,
        palms_mps: trace.split.palms,
        chase_mps: trace.split.chase,
        wanted_mps: trace.split.wanted,
        proxy_mps: trace.proxy.velocity,
        surface_mps: trace.proxy.surface_velocity,
        support: Label::Borrowed(trace.proxy.support.label()),
        surface_dynamic: trace.proxy.surface_dynamic,
        ground_body: trace.proxy.ground_body,
        proxy_state: trace.proxy.state,
        pending_proxy_state: trace.proxy.pending_state,
        default_proxy_state: trace.proxy.default_state,
        authored: trace.authored,
        capsule_owned: trace.capsule_owned,
        capsule_blocked: trace.capsule_blocked,
        jumps: trace.jumps,
    }
}

fn scheme_label(scheme: Scheme) -> &'static str {
    match scheme {
        Scheme::Controllers => "controllers",
        Scheme::Gamepad => "gamepad",
    }
}

fn mode_label(mode: Mode) -> &'static str {
    match mode {
        Mode::OnFoot(OnFootVariant::Ground) => "on_foot",
        Mode::OnFoot(OnFootVariant::Swimming) => "swimming",
        Mode::Vehicle(_) => "vehicle",
        Mode::Airborne(AirborneVariant::Falling) => "falling",
        Mode::Airborne(AirborneVariant::Wingsuit) => "wingsuit",
        Mode::Airborne(AirborneVariant::Parachute) => "parachute",
        Mode::Ui => "ui",
    }
}

fn traversal_label(traversal: Traversal) -> &'static str {
    match traversal {
        Traversal::PreReel => "pre_reel",
        Traversal::ReelingIn => "reeling_in",
        Traversal::PreHang => "pre_hang",
        Traversal::ReeledIn => "reeled_in",
    }
}

fn hold_label(reason: HoldReason) -> &'static str {
    match reason {
        HoldReason::Teleporting => "teleporting",
        HoldReason::CharacterUnavailable => "character_unavailable",
        HoldReason::Debouncing => "debouncing",
    }
}

fn idle_label(idle: AimIdle) -> &'static str {
    match idle {
        AimIdle::Disabled => "disabled",
        AimIdle::GamepadScheme => "gamepad_scheme",
        AimIdle::NoHands => "no_hands",
    }
}

fn sight_source_label(source: SightSource) -> &'static str {
    match source {
        SightSource::Config => "config",
        SightSource::Table(grade) => grade.label(),
        SightSource::FirePoint => "fire_point",
    }
}

fn raycast_error_label(error: RaycastError) -> &'static str {
    match error {
        RaycastError::NoPhysicsSystem => "no_physics_system",
        RaycastError::InvalidDistance => "invalid_distance",
    }
}

fn probe_label(probe: ProbeResult) -> &'static str {
    match probe {
        ProbeResult::Clear => "clear",
        ProbeResult::Obstructed { .. } => "obstructed",
    }
}
