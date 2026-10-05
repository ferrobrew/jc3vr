//! The standing stance: keeping the character's stance square with a player who stands normally.
//!
//! On foot under the controllers, the body first goes weapon-agnostic ([`unarmed`]): whatever is
//! held, it plays what it plays unarmed. The two stages below predate it and act only while the
//! character stands still.
//!
//! On foot the character's base idle plays a clip chosen by the animation modifiers the engine
//! sets each tick ([`Character::SetCharacterModifiers`]): the unarmed relaxed idle with no weapon,
//! the wielded weapon's held stance, or its aimed stance while the trigger is pulled (see
//! `docs/engine/character/idle-stance.md`). The weapon stances carry the body bladed, one foot
//! ahead of the other, which leaves a standing player's body out of step with the character's.
//! Two stages answer it, each behind its own toggle in [`StanceConfig`]:
//!
//! 1. The relaxed idle: while standing still, the weapon's modifiers are kept off the body's rule
//!    system ([`crate::hooks::character::modifiers`]), so the idle plays the relaxed clip.
//! 2. The squared stance: while standing still, HumanIK targets at the pose-finalization entry put
//!    the ankles side by side across the body's forward and turn the pelvis and feet to face it,
//!    and the body IK's torso rotation turns the chest to face it ([`plan`] before the body IK,
//!    [`queue`] after the crouch).
//!
//! With the body alignment turning the chest toward the player's estimated torso, the squaring
//! also turns the pelvis and feet by a share of the same turn
//! ([`crate::controllers::body::alignment::stance_turn`]).
//!
//! The pure logic is in [`logic`]; this module reads the engine and queues the targets.

pub mod config;
pub mod logic;
pub mod unarmed;

use std::{
    sync::LazyLock,
    time::{Duration, Instant},
};

use jc3gi::{
    animation::ik::{Effector, HumanIK, Pass, SolveStep},
    character::character::{
        AimState, Character, CharacterLodFlags, Joint, get_Character_EnableHIK,
    },
    hash::hashlittle,
    types::math::{Matrix4, Vector3},
};
use parking_lot::Mutex;

pub use crate::controllers::roomscale::stance::config::StanceConfig;
use crate::{
    config::Config,
    controllers::{
        body::alignment,
        roomscale::{
            frame_shared,
            stance::logic::{
                BlendTiming, FeetLayout, IdleVariant, MAX_CORRECTION_RAD, StanceBlend, StillInputs,
                Verdict, classify, correction, feet_layout, forward_yaw, horizontal_speed,
                lateral_twist, square_feet,
            },
            visual_crouch_queued,
        },
        scheme,
    },
    headpose::{
        self,
        sim::{HeadMode, mode},
    },
    hooks::{self, animation::active_state_type_id, character::body_ik::bone_for_effector},
};

/// What the stance logic decided for the local player on its last tick, for the debug UI.
#[derive(Debug, Copy, Clone, PartialEq, Default)]
pub struct StanceStatus {
    /// When the pose-finalization half last ran for the local player.
    pub at: Option<Instant>,
    /// The body rule system's state, by name when it is one of [`STATE_NAMES`].
    pub state: StateName,
    pub verdict: Verdict,
    /// The character's horizontal speed, metres per second.
    pub speed_mps: Option<f32>,
    /// The squaring's blend weight, `0..=1`.
    pub weight: f32,
    /// The animated pose's turns and feet, before any squaring.
    pub measured: Measured,
    /// What the squaring queued.
    pub queued: Queued,
    /// The modifier half's last decision.
    pub modifiers: ModifierStatus,
}

/// The body rule system's current state.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Default)]
pub enum StateName {
    #[default]
    Unreadable,
    Named(&'static str),
    Hash(u32),
}

/// The animated pose's turns away from the body's forward, degrees, and the feet's layout.
#[derive(Debug, Copy, Clone, PartialEq, Default)]
pub struct Measured {
    pub pelvis_deg: Option<f32>,
    pub chest_deg: Option<f32>,
    pub feet: Option<FeetLayout>,
    /// Each foot's turn, indexed left then right.
    pub foot_deg: [Option<f32>; 2],
}

/// What the squaring queued on the last tick.
#[derive(Debug, Copy, Clone, PartialEq, Default)]
pub struct Queued {
    /// Whether the hips were held at their animated position (not while the visual crouch owns
    /// them).
    pub hips_position: bool,
    /// The pelvis turn queued, degrees.
    pub pelvis_deg: Option<f32>,
    /// The chest turn handed to the body IK's torso rotation, degrees.
    pub chest_deg: f32,
    /// How many ankles took a position target.
    pub ankles: u8,
    /// How many feet took a turn.
    pub feet_turned: u8,
    /// Why nothing was queued, when the stance stood still but the solver would not run.
    pub skipped: Option<&'static str>,
}

/// The modifier half's last decision for the local player.
#[derive(Debug, Copy, Clone, PartialEq, Default)]
pub struct ModifierStatus {
    pub at: Option<Instant>,
    /// Whether the relaxed idle kept the weapon's modifiers off the body's rule system.
    pub relaxed_applied: bool,
    /// Whether the unarmed body kept the weapon's modifiers off every rule system but the
    /// upper-body actions'.
    pub unarmed_applied: bool,
    /// Which idle variant the modifiers select.
    pub variant: IdleVariant,
}

/// The latest status.
pub fn status() -> StanceStatus {
    STATE.lock().status
}

/// The pose-finalization half's plan for one tick, from [`plan`] to [`queue`].
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct StancePlan {
    weight: f32,
    /// The turn about the model's up axis the body IK adds to its torso rotation, radians.
    pub chest_yaw: f32,
    pelvis_yaw: Option<f32>,
    /// The turn of the pelvis and feet toward the player's estimated torso, radians, already
    /// weighted by the blend ([`alignment::stance_turn`]).
    torso_turn: f32,
    joints: Joints,
}

/// Measure the local player's stance, decide whether it stands still, and advance the blend. Run
/// at the entry of the pose-finalization pass, before the body IK, which takes
/// [`StancePlan::chest_yaw`] into its torso rotation. `dt` is the tick's simulation step, seconds.
///
/// # Safety
///
/// `character` must be null or a live character at the entry of its pose-finalization pass.
pub unsafe fn plan(character: *mut Character, dt: f32) -> Option<StancePlan> {
    let character = unsafe { character.as_ref() }.filter(|c| c.m_IsLocalCharacter)?;
    let config = Config::lock_query(|c| c.controllers.roomscale.stance);
    let state_id = unsafe { active_state_type_id(character) };
    let world = |m: Matrix4| glam::Mat4::from(m).w_axis.truncate();
    let inputs = StillInputs {
        controllers: scheme::is_controllers(),
        vr: headpose::source() == headpose::Source::Vr,
        gameplay: hooks::in_gameplay(),
        on_foot: mode() == HeadMode::OnFoot,
        idle_state: state_id == Some(*S_IDLE_ID),
        aiming: character
            .m_AimFlags
            .intersects(AimState::m_AimingWeapon | AimState::m_AimingGrapple),
        tether: unsafe { tether(character) },
        crouched: frame_shared().is_some_and(|f| f.fresh(Instant::now()) && f.crouched),
        speed_mps: horizontal_speed(
            world(character.m_WorldMatrixT0),
            world(character.m_WorldMatrixT1),
            dt,
        ),
    };
    let verdict = classify(&inputs, config.still_speed_mps);
    let joints = unsafe { Joints::read(character) }.unwrap_or_default();
    let measured = joints.measure();

    let mut state = STATE.lock();
    let timing = BlendTiming {
        settle_s: config.settle_s,
        blend_in_s: config.blend_in_s,
        blend_out_s: config.blend_out_s,
    };
    let hard_stop = matches!(verdict, Verdict::StandDown(reason) if reason.is_hard());
    let weight = if config.square && !hard_stop {
        state
            .blend
            .step(verdict == Verdict::StandingStill, dt, timing)
    } else {
        state.blend.reset();
        0.0
    };
    let to_rad = |deg: Option<f32>| deg.map(f32::to_radians);
    let chest_yaw =
        to_rad(measured.chest_deg).map_or(0.0, |turn| correction(turn, config.chest_share, weight));
    // The body alignment's turn toward the player's torso joins the squaring, so the pelvis and
    // feet face the torso rather than the character's own facing. The chest's share is the body
    // IK's own ([`alignment::chest_turn`]).
    let rotation = glam::Mat4::from(character.m_WorldMatrixT1)
        .to_scale_rotation_translation()
        .1;
    let torso_turn = alignment::stance_turn(rotation) * weight;
    let pelvis_yaw = to_rad(measured.pelvis_deg)
        .map(|turn| combine(correction(turn, config.pelvis_share, weight), torso_turn));
    state.status = StanceStatus {
        at: Some(Instant::now()),
        state: state_name(state_id),
        verdict,
        speed_mps: inputs.speed_mps,
        weight,
        measured,
        queued: Queued::default(),
        modifiers: state.status.modifiers,
    };
    state.verdict_at = Some((verdict, Instant::now()));
    Some(StancePlan {
        weight,
        chest_yaw,
        pelvis_yaw,
        torso_turn,
        joints,
    })
}

/// Queue the squared stance's lower-body targets on the `MAIN` pass at the full-body step: the
/// hips held at their animated position (unless the visual crouch already placed them), the pelvis
/// turned by the plan's correction, and the ankles placed side by side and turned to face the
/// body's forward. Run after the visual crouch, so the squared ankles replace its pins.
///
/// # Safety
///
/// `character` must be null or the live local character at the entry of its pose-finalization
/// pass.
pub unsafe fn queue(character: *mut Character, plan: &StancePlan) {
    let Some(character) = (unsafe { character.as_mut() }).filter(|c| c.m_IsLocalCharacter) else {
        return;
    };
    if plan.weight <= MIN_WEIGHT {
        return;
    }
    let config = Config::lock_query(|c| c.controllers.roomscale.stance);
    let skipped = if !unsafe { *get_Character_EnableHIK() } {
        Some("HumanIK globally disabled")
    } else if character
        .m_LodFlags
        .contains(CharacterLodFlags::REDUCED_LOD)
    {
        Some("character in reduced LOD")
    } else {
        None
    };
    let mut queued = Queued {
        chest_deg: plan.chest_yaw.to_degrees(),
        skipped,
        ..Queued::default()
    };
    if skipped.is_none() {
        let hik = &mut character.m_HIK;
        let j = &plan.joints;
        if let Some(hips) = j.hips {
            if !visual_crouch_queued() {
                unsafe { queue_position(hik, Effector::HIPS as i32, hips) };
                queued.hips_position = true;
            }
            if let Some(turn) = plan.pelvis_yaw.filter(|t| t.abs() > MIN_TURN_RAD) {
                unsafe { queue_turn(hik, Effector::HIPS as i32, turn, 0.0) };
                queued.pelvis_deg = Some(turn.to_degrees());
            }
        }
        if let [Some(left), Some(right)] = j.ankles {
            let feet = square_feet(left, right, config.feet_width_m, plan.weight);
            for (effector, target) in [
                (Effector::LEFT_ANKLE as i32, feet.left),
                (Effector::RIGHT_ANKLE as i32, feet.right),
            ] {
                unsafe { queue_position(hik, effector, target) };
                queued.ankles += 1;
            }
            for (side, effector) in [Effector::LEFT_ANKLE, Effector::RIGHT_ANKLE]
                .into_iter()
                .enumerate()
            {
                let turn = j.ankles[side]
                    .zip(j.toes[side])
                    .and_then(|(ankle, toe)| forward_yaw(ankle, toe))
                    .map(|turn| {
                        combine(
                            correction(turn, config.feet_yaw_share, plan.weight),
                            plan.torso_turn,
                        )
                    });
                if let Some(turn) = turn.filter(|t| t.abs() > MIN_TURN_RAD) {
                    let resist = if config.ankle_resist.is_finite() {
                        config.ankle_resist.clamp(0.0, 1.0)
                    } else {
                        0.0
                    };
                    unsafe { queue_turn(hik, effector as i32, turn, resist) };
                    queued.feet_turned += 1;
                }
            }
        }
    }
    STATE.lock().status.queued = queued;
}

/// Whether the weapon's modifiers stay off the body's rule system on this tick: the relaxed idle
/// is on and the pose-finalization half found the local player standing still on its latest tick.
pub fn relaxed_idle_applies() -> bool {
    Config::lock_query(|c| c.controllers.roomscale.stance.relaxed_idle)
        && STATE.lock().verdict_at.is_some_and(|(verdict, at)| {
            verdict == Verdict::StandingStill && at.elapsed() < VERDICT_FRESH
        })
}

/// Record the modifier half's decision for the readout.
pub fn record_modifiers(relaxed_applied: bool, unarmed_applied: bool, variant: IdleVariant) {
    STATE.lock().status.modifiers = ModifierStatus {
        at: Some(Instant::now()),
        relaxed_applied,
        unarmed_applied,
        variant,
    };
}

/// Whether the character's grapple hook holds a live tether (the hook's aiming state), which the
/// engine marks with the grapple modifier.
///
/// # Safety
///
/// `character` must be a live character.
pub unsafe fn tether(character: &Character) -> bool {
    unsafe { character.m_Inventory.m_GrapplingHook.px.as_ref() }
        .is_some_and(|hook| unsafe { hook.IsHookAimingState() })
}

/// The state names the readout recognises: the on-foot idles and their neighbours.
pub const STATE_NAMES: [&str; 13] = [
    "S_IDLE",
    "S_IDLE_ONE_OFF",
    "S_IDLE_AIM_FOCUS",
    "S_IDLE_PRECISION",
    "S_IDLE_PICKUP",
    "S_IDLE_TRANS_TO_AIM",
    "S_IDLE_TRANS_TO_NO_AIM",
    "S_IDLE_TRANS_TO_NO_AIM_SLOW",
    "S_COMBAT_IDLE_TO_UNARMED_IDLE",
    "S_MOVE_AIM_FOCUS",
    "S_MOVE_AIM_STRAFE",
    "S_MOVE_NO_AIM",
    "S_MOVE_NON_COMBAT",
];

/// Below this blend weight nothing is queued, so a standing pose costs no full-body solve.
const MIN_WEIGHT: f32 = 0.01;

/// A turn smaller than this is not queued, radians.
const MIN_TURN_RAD: f32 = 1.0e-3;

/// How long the pose-finalization half's verdict holds for the modifier half, which runs earlier in
/// the next tick.
const VERDICT_FRESH: Duration = Duration::from_millis(250);

static S_IDLE_ID: LazyLock<u32> = LazyLock::new(|| hashlittle(b"S_IDLE") as u32);

static STATE_IDS: LazyLock<Vec<(u32, &'static str)>> = LazyLock::new(|| {
    STATE_NAMES
        .iter()
        .map(|name| (hashlittle(name.as_bytes()) as u32, *name))
        .collect()
});

static STATE: LazyLock<Mutex<StanceState>> = LazyLock::new(|| Mutex::new(StanceState::default()));

#[derive(Default)]
struct StanceState {
    blend: StanceBlend,
    verdict_at: Option<(Verdict, Instant)>,
    status: StanceStatus,
}

fn state_name(id: Option<u32>) -> StateName {
    match id {
        None => StateName::Unreadable,
        Some(id) => STATE_IDS
            .iter()
            .find(|(hash, _)| *hash == id)
            .map_or(StateName::Hash(id), |(_, name)| StateName::Named(name)),
    }
}

/// The animated (pre-solve) joints the stance reads, in character-model space, each `None` when the
/// rig has no node for its effector.
#[derive(Debug, Copy, Clone, PartialEq, Default)]
struct Joints {
    hips: Option<glam::Vec3>,
    /// The upper-leg joints, left then right.
    hips_lr: [Option<glam::Vec3>; 2],
    ankles: [Option<glam::Vec3>; 2],
    /// The foot effectors' joints, at the ball of each foot.
    toes: [Option<glam::Vec3>; 2],
    /// The upper-arm joints, left then right.
    shoulders: [Option<glam::Vec3>; 2],
}

impl Joints {
    /// Read the joints from the character's animation controller.
    ///
    /// # Safety
    ///
    /// `character` must be a live character at the entry of its pose-finalization pass.
    unsafe fn read(character: &Character) -> Option<Self> {
        let controller = unsafe { character.m_AnimatedModel.m_AnimationController.as_ref() }?;
        let hik = &character.m_HIK;
        let joint = |effector: Effector| {
            let bone = unsafe { bone_for_effector(hik, effector as i32) }?;
            let mut joint = Joint::default();
            unsafe { controller.GetJoint(bone, &mut joint) };
            let position = glam::Vec3::from_array(joint.m_Translation.data);
            position.is_finite().then_some(position)
        };
        Some(Self {
            hips: joint(Effector::HIPS),
            hips_lr: [joint(Effector::LEFT_HIP), joint(Effector::RIGHT_HIP)],
            ankles: [joint(Effector::LEFT_ANKLE), joint(Effector::RIGHT_ANKLE)],
            toes: [joint(Effector::LEFT_FOOT), joint(Effector::RIGHT_FOOT)],
            shoulders: [
                joint(Effector::LEFT_SHOULDER),
                joint(Effector::RIGHT_SHOULDER),
            ],
        })
    }

    fn measure(&self) -> Measured {
        let pair = |p: [Option<glam::Vec3>; 2]| p[0].zip(p[1]);
        let degrees = |turn: Option<f32>| turn.map(f32::to_degrees);
        Measured {
            pelvis_deg: degrees(pair(self.hips_lr).and_then(|(l, r)| lateral_twist(l, r))),
            chest_deg: degrees(pair(self.shoulders).and_then(|(l, r)| lateral_twist(l, r))),
            feet: pair(self.ankles).map(|(l, r)| feet_layout(l, r)),
            foot_deg: [0, 1].map(|side| {
                degrees(
                    self.ankles[side]
                        .zip(self.toes[side])
                        .and_then(|(ankle, toe)| forward_yaw(ankle, toe)),
                )
            }),
        }
    }
}

/// A squaring correction with the body alignment's turn added, bounded as the correction is.
fn combine(correction: f32, torso_turn: f32) -> f32 {
    let torso_turn = if torso_turn.is_finite() {
        torso_turn
    } else {
        0.0
    };
    (correction + torso_turn).clamp(-MAX_CORRECTION_RAD, MAX_CORRECTION_RAD)
}

/// Queue a full-reach positional target with no pull on `effector`, at the full-body step, without
/// interpolation or blend-out because the target is re-queued every tick from the current pose.
unsafe fn queue_position(hik: &mut HumanIK, effector: i32, position: glam::Vec3) {
    let Ok(index) = usize::try_from(effector) else {
        return;
    };
    let target = Vector3 {
        data: position.to_array(),
    };
    unsafe {
        hik.AddEffectorTargetPosition(
            effector,
            &target,
            SolveStep::FULL_BODY,
            Pass::MAIN,
            false,
            0.0,
            false,
            0.0,
        );
    }
    hik.m_TargetReachT[index] = 1.0;
    hik.m_TargetPull[index] = 0.0;
}

/// Queue a full-reach turn of `angle` radians about the model's up axis on `effector`, offset from
/// its animated orientation, with `resist`.
unsafe fn queue_turn(hik: &mut HumanIK, effector: i32, angle: f32, resist: f32) {
    let Ok(index) = usize::try_from(effector) else {
        return;
    };
    let axis = Vector3 {
        data: glam::Vec3::Y.to_array(),
    };
    unsafe {
        hik.AddEffectorTargetRotationVector(
            effector,
            angle,
            &axis,
            SolveStep::FULL_BODY,
            Pass::MAIN,
            false,
            0.0,
            false,
            0.0,
        );
    }
    hik.m_TargetReachR[index] = 1.0;
    hik.m_TargetResist[index] = resist;
}
