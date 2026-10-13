//! The diagnostic sample: a compact, serializable, pointer-free copy of each controller
//! subsystem's status, and its construction from the subsystems' live readouts.

use serde::Serialize;

use crate::{
    controllers::{
        aim::{self, AimIdle, HandSource, raycast::RaycastError, sights::SightSource},
        body::rig::collision::probe::ProbeResult,
        frame::{self, ActionSetKind, Hand},
        hands::{ik, late, tiers},
        mode::{self, AirborneVariant, HoldReason, Mode, OnFootVariant, Traversal},
        roomscale,
        scheme::{self, Scheme},
    },
    vr::input as vr_input,
};

/// A compact, pointer-free snapshot of the controller subsystems.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ControllerDiagnostic {
    /// Milliseconds since recording was enabled.
    pub elapsed_ms: u64,
    /// The published OpenXR frame sequence, when a frame is available.
    pub frame_sequence: Option<u64>,
    pub scheme: SchemeDiagnostic,
    pub mode: ModeDiagnostic,
    pub tracking: TrackingDiagnostic,
    pub ik: [IkDiagnostic; 2],
    pub tiers: TierDiagnostic,
    pub late: LateDiagnostic,
    pub aim: AimDiagnostic,
    pub roomscale: RoomscaleDiagnostic,
}

/// The effective and auto-selected controller schemes.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SchemeDiagnostic {
    pub effective: &'static str,
    pub selected: &'static str,
}

/// The mode detector's result and the action set selected for the next input frame.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ModeDiagnostic {
    pub mode: &'static str,
    pub action_set: &'static str,
    pub traversal: Option<&'static str>,
    pub held: Option<&'static str>,
}

/// Controller frame age, focus, action set, and per-hand tracking flags.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct TrackingDiagnostic {
    pub age_ms: Option<u64>,
    pub focused: bool,
    pub action_set: &'static str,
    pub input_error: Option<String>,
    pub left: HandTrackingDiagnostic,
    pub right: HandTrackingDiagnostic,
}

/// Validity and measured-tracking state for one hand's grip and aim poses.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Default)]
pub struct HandTrackingDiagnostic {
    pub active: bool,
    pub grip_position_valid: bool,
    pub grip_orientation_valid: bool,
    pub grip_position_tracked: bool,
    pub grip_orientation_tracked: bool,
    pub aim_position_valid: bool,
    pub aim_orientation_valid: bool,
    pub aim_position_tracked: bool,
    pub aim_orientation_tracked: bool,
}

/// One hand's HumanIK result.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct IkDiagnostic {
    pub queued: bool,
    pub skip: Option<&'static str>,
    pub weight: f32,
    pub reach_distance_m: f32,
    pub rotation_deg: f32,
    pub secondary_wrist_off: bool,
    pub secondary_grip_off: bool,
    pub overrode_live_target: bool,
}

/// The classified hand tier and its blend state.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct TierDiagnostic {
    pub state: Option<&'static str>,
    pub left: HandTierDiagnostic,
    pub right: HandTierDiagnostic,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct HandTierDiagnostic {
    pub assigned: &'static str,
    pub effective: &'static str,
    pub controller_weight: f32,
    pub anchor_weight: f32,
}

/// One hand's render-time late correction status and physical pose samples.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct HandLateDiagnostic {
    pub reason: &'static str,
    pub physical_displacement_m: Option<f32>,
    pub relative_rotation_deg: Option<f32>,
    pub requested_wrist_position: Option<[f32; 3]>,
    pub reference_wrist_position: Option<[f32; 3]>,
    pub latest_wrist_position: Option<[f32; 3]>,
    pub achieved_wrist_position_model: Option<[f32; 3]>,
}

/// Render-time late correction status and cumulative counters.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct LateDiagnostic {
    pub hands: [HandLateDiagnostic; 2],
    pub weapons_updated: u64,
    pub palettes_edited: u64,
}

/// Aim rays, hit results, weapon sight selection, and structured error information.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct AimDiagnostic {
    pub idle: Option<&'static str>,
    pub left: Option<HandAimDiagnostic>,
    pub right: Option<HandAimDiagnostic>,
    pub grapple: Option<GrappleDiagnostic>,
    pub weapon: Option<WeaponDiagnostic>,
    pub raycast_error: Option<&'static str>,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct HandAimDiagnostic {
    pub source: &'static str,
    pub gun_index: Option<usize>,
    pub hit: bool,
    pub hit_distance_m: Option<f32>,
    pub hit_object: bool,
    pub target: [f32; 3],
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct GrappleDiagnostic {
    pub hit: bool,
    pub valid: bool,
    pub tetherable: bool,
    pub point: [f32; 3],
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct WeaponDiagnostic {
    pub name_hash: u32,
    pub gun_hands: [Option<&'static str>; 4],
    pub sight_source: &'static str,
    pub sight_offset_m: Option<[f32; 3]>,
    pub sight_angles_deg: [f32; 2],
}

/// The posture, the stage, the rig's root and chase, the seat, the fade, and the requested velocities.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct RoomscaleDiagnostic {
    pub active: bool,
    pub posture: &'static str,
    pub stage_available: bool,
    pub stage_bounds_m: Option<[f32; 2]>,
    pub stage_head_height_m: Option<f32>,
    pub eye_drop_m: f32,
    /// Who owned the root on the last on-foot tick.
    pub root_owner: Option<&'static str>,
    /// How far the ground point under the upright eyes is from the root, and how far it is outside
    /// the lean, world metres.
    pub gap_m: f32,
    pub residual_m: f32,
    /// How far the push-back has slid the view in its current episode, metres.
    pub pushback_m: f32,
    pub cockpit_shift_m: [f32; 3],
    pub requested_chase_velocity_mps: [f32; 2],
    pub requested_task_velocity_mps: [f32; 2],
    pub crouched: bool,
    pub seated_crouch: bool,
    pub seat: &'static str,
    pub probe: Option<&'static str>,
    pub fade_level: f32,
}

/// Read every subsystem's status into one sample, with `elapsed_ms` left for the caller.
pub(super) fn snapshot() -> ControllerDiagnostic {
    let detection = mode::current();
    let input = tracking_snapshot();
    let ik_status = ik::status();
    let tier_status = tiers::status();
    let late_status = late::status();
    let aim_status = aim::status();
    let roomscale_status = roomscale::status();
    let scheme = SchemeDiagnostic {
        effective: scheme_label(scheme::current()),
        selected: scheme_label(scheme::selected()),
    };
    ControllerDiagnostic {
        elapsed_ms: 0,
        frame_sequence: input.sequence,
        scheme,
        mode: ModeDiagnostic {
            mode: mode_label(detection.mode),
            action_set: action_set_label(detection.mode.action_set()),
            traversal: detection.traversal.map(traversal_label),
            held: detection.held.map(hold_label),
        },
        tracking: input.tracking,
        ik: Hand::BOTH.map(|hand| ik_diagnostic(ik_status[hand.index()])),
        tiers: tier_diagnostic(tier_status),
        late: late_diagnostic(late_status),
        aim: aim_diagnostic(aim_status),
        roomscale: roomscale_diagnostic(roomscale_status),
    }
}

struct TrackingSnapshot {
    sequence: Option<u64>,
    tracking: TrackingDiagnostic,
}

fn tracking_snapshot() -> TrackingSnapshot {
    let input_error = vr_input::status().error;
    frame::with_latest(|frame| TrackingSnapshot {
        sequence: Some(frame.sequence),
        tracking: TrackingDiagnostic {
            age_ms: Some(frame.age().as_millis().try_into().unwrap_or(u64::MAX)),
            focused: frame.focused,
            action_set: action_set_label(frame.action_set),
            input_error: input_error.clone(),
            left: hand_tracking(frame, Hand::Left),
            right: hand_tracking(frame, Hand::Right),
        },
    })
    .unwrap_or_else(|| TrackingSnapshot {
        sequence: None,
        tracking: TrackingDiagnostic {
            age_ms: None,
            focused: false,
            action_set: action_set_label(vr_input::active_action_set()),
            input_error,
            left: HandTrackingDiagnostic::default(),
            right: HandTrackingDiagnostic::default(),
        },
    })
}

fn hand_tracking(frame: &frame::ControllerFrame, hand: Hand) -> HandTrackingDiagnostic {
    let hand = frame.hand(hand);
    HandTrackingDiagnostic {
        active: hand.active,
        grip_position_valid: hand.grip.position_valid,
        grip_orientation_valid: hand.grip.orientation_valid,
        grip_position_tracked: hand.grip.position_tracked,
        grip_orientation_tracked: hand.grip.orientation_tracked,
        aim_position_valid: hand.aim.position_valid,
        aim_orientation_valid: hand.aim.orientation_valid,
        aim_position_tracked: hand.aim.position_tracked,
        aim_orientation_tracked: hand.aim.orientation_tracked,
    }
}

fn ik_diagnostic(status: ik::HandIkStatus) -> IkDiagnostic {
    IkDiagnostic {
        queued: status.queued,
        skip: status.skip.map(|skip| skip.label()),
        weight: status.weight,
        reach_distance_m: status.reach_distance,
        rotation_deg: status.rotation_deg,
        secondary_wrist_off: status.secondary_wrist_off,
        secondary_grip_off: status.secondary_grip_off,
        overrode_live_target: status.overrode_live_target,
    }
}

fn tier_diagnostic(status: tiers::TierStatus) -> TierDiagnostic {
    TierDiagnostic {
        state: status.state.map(|state| state.label()),
        left: hand_tier_diagnostic(status, Hand::Left),
        right: hand_tier_diagnostic(status, Hand::Right),
    }
}

fn hand_tier_diagnostic(status: tiers::TierStatus, hand: Hand) -> HandTierDiagnostic {
    let index = hand.index();
    HandTierDiagnostic {
        assigned: status.tiers[index].label(),
        effective: status.effective[index].label(),
        controller_weight: status.weights[index],
        anchor_weight: status.anchor_weights[index],
    }
}

fn late_diagnostic(status: late::LateStatus) -> LateDiagnostic {
    let hands = status.hands.map(|hand| HandLateDiagnostic {
        reason: hand.reason.label(),
        physical_displacement_m: hand.physical_displacement_m,
        relative_rotation_deg: hand.relative_rotation_deg,
        requested_wrist_position: hand.requested_wrist_position.map(|p| p.to_array()),
        reference_wrist_position: hand.reference_wrist_position.map(|p| p.to_array()),
        latest_wrist_position: hand.latest_wrist_position.map(|p| p.to_array()),
        achieved_wrist_position_model: hand.achieved_wrist_position_model.map(|p| p.to_array()),
    });
    LateDiagnostic {
        hands,
        weapons_updated: status.weapons_updated,
        palettes_edited: status.palettes_edited,
    }
}

fn aim_diagnostic(status: aim::AimStatus) -> AimDiagnostic {
    AimDiagnostic {
        idle: status.idle.map(idle_label),
        left: status.hands[Hand::Left.index()]
            .as_ref()
            .map(hand_aim_diagnostic),
        right: status.hands[Hand::Right.index()]
            .as_ref()
            .map(hand_aim_diagnostic),
        grapple: status.grapple.map(|grapple| GrappleDiagnostic {
            hit: grapple.hit,
            valid: grapple.valid,
            tetherable: grapple.tetherable,
            point: grapple.point.to_array(),
        }),
        weapon: status.weapon.map(|weapon| WeaponDiagnostic {
            name_hash: weapon.hash,
            gun_hands: weapon.gun_hands.map(|hand| hand.map(Hand::label)),
            sight_source: sight_source_label(weapon.sight.source),
            sight_offset_m: weapon.sight.sight.map(|sight| sight.offset_m.to_array()),
            sight_angles_deg: weapon.sight.angles_deg().to_array(),
        }),
        raycast_error: status.raycast_error.map(raycast_error_label),
    }
}

fn hand_aim_diagnostic(aim: &aim::HandAim) -> HandAimDiagnostic {
    HandAimDiagnostic {
        source: match aim.source {
            HandSource::AimPose => "aim_pose",
            HandSource::Gun { .. } => "gun_sight",
        },
        gun_index: match aim.source {
            HandSource::AimPose => None,
            HandSource::Gun { gun } => Some(gun),
        },
        hit: aim.hit.is_some(),
        hit_distance_m: aim.hit.map(|hit| hit.distance),
        hit_object: aim.hit.is_some_and(|hit| hit.object.is_some()),
        target: aim.target.to_array(),
    }
}

fn roomscale_diagnostic(status: roomscale::RoomscaleStatus) -> RoomscaleDiagnostic {
    let frame = status.frame;
    RoomscaleDiagnostic {
        active: status.active,
        posture: frame.posture.label(),
        stage_available: frame.stage.available,
        stage_bounds_m: frame.stage.bounds.map(|bounds| bounds.to_array()),
        stage_head_height_m: frame.stage.head_height,
        eye_drop_m: frame.eye_drop_m,
        root_owner: status.root.sim.owner.map(|o| o.label()),
        gap_m: status.root.frame.gap_m(),
        residual_m: status.root.frame.residual_m(),
        pushback_m: status.root.frame.debt_m,
        cockpit_shift_m: status.root.frame.shift.to_array(),
        requested_chase_velocity_mps: status.root.sim.chase_velocity.to_array(),
        requested_task_velocity_mps: status.root.sim.task_velocity.to_array(),
        crouched: frame.crouched,
        seated_crouch: frame.seated_crouch,
        seat: frame.seat.label(),
        probe: frame.probe.map(probe_label),
        fade_level: status.fade.level,
    }
}

fn scheme_label(scheme: Scheme) -> &'static str {
    match scheme {
        Scheme::Controllers => "controllers",
        Scheme::Gamepad => "gamepad",
    }
}

fn action_set_label(action_set: ActionSetKind) -> &'static str {
    action_set.name()
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
