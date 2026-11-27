//! Controller-owned hands: the plan of record's "Hands", "Hand tiers", and "Body scale" sections
//! (`docs/mod/input/controllers-and-roomscale.md`), with the engine side in
//! `docs/engine/character/humanik.md` and `docs/engine/character/hands-locomotion-and-state.md`.
//!
//! - [`pose`]: the pure composition from a controller's grip pose to the character's wrist frame,
//!   through the grip offset, the body-scale remap, and the head's world transform;
//! - [`reach`]: the pure arm-reach logic: the remap about the character's measured shoulder, the
//!   pull ramp near full extension, and the capped detach of the rendered hand;
//! - [`rig`]: the character's arms measured from his animated skeleton at the pre-solve seam;
//! - [`tiers`]: the per-state tier table and the per-hand blend between controller and animation;
//! - [`anchor`]: the anchored tier of the grapple states: the grapple hand holding the line, and
//!   the hang it swings;
//! - [`ik`]: the wrist targets queued on HumanIK's `MAIN` pass at the pre-solve seam, and the
//!   record of each tick's targets that the late update corrects from;
//! - [`secondary`]: the per-hand switch for the game's `SECONDARY` hand IK pass;
//! - [`grip`]: the rigid grip: a gun held in a driven hand pinned to the wrist at the hand's
//!   canonical grip before each placement of the weapons;
//! - [`late`]: the render-time late update of the held weapon's model and of the hand and forearm
//!   bones in the skinning palette, and the render-time hand poses for the overlay;
//! - [`parts`]: the render-time late update of a held gun's separate model parts (its magazine,
//!   slide, or spinning barrel);
//! - [`palette`]: the pure palette-entry math of the late update;
//! - [`tuning`]: the tuning, read from the shipped `tuning/hands.toml`.
//!
//! Everything here is inert unless the controller scheme is in effect and the hand's controller is
//! tracked ([`active_hands`]).

use glam::{Quat, Vec3};
use jc3vr_input::frame::{self, ControllerFrame, Hand, Pose};
use jc3vr_view::{headpose, vr::VR_CONFIG};
use re_utilities::hook_library::HookLibrary;

use crate::controllers::{
    body,
    body::{BodyDimensions, config::BODY_REMAP_CONFIG, rig as body_rig},
    body_frame,
    hands::{
        config::{HANDS_CONFIG, HandsConfig},
        reach::{ArmRemap, ReachLimit, level_eyes},
        stages::HandComposition,
    },
    scheme,
};

pub mod anchor;
pub mod config;
pub mod grip;
pub mod ik;
pub mod late;
pub mod palette;
pub mod parts;
pub mod pose;
pub mod reach;
pub mod rig;
pub mod secondary;
pub mod stages;
pub mod tiers;
pub mod tuning;

pub use pose::CockpitHand;
pub use stages::DrawTime;

/// The hands' hooks, aggregated by `controllers::hook_library`.
pub fn hook_library() -> HookLibrary {
    late::hook_library()
        .with_hook_library(parts::hook_library())
        .with_hook_library(anchor::hook_library())
}

/// The inputs every hand subsystem starts from: the hands' settings, the head's cockpit pose, and
/// each tracked hand's cockpit-frame poses after the grip offset and the remap.
#[derive(Clone)]
pub struct ActiveHands {
    pub config: HandSettings,
    /// The head (the eye midpoint) in the cockpit frame.
    pub head: Pose,
    /// Indexed by [`Hand::index`]; `None` for a hand whose controller is absent or not tracked.
    pub hands: [Option<CockpitHand>; 2],
    /// The VR world scale the head pose composes with.
    pub world_scale: f32,
    /// The torso's heading the player's shoulders are estimated at ([`body_rig::torso_heading`]),
    /// in the cockpit frame.
    pub torso: Quat,
}

/// The settings the hands are composed with: the hands' own group, and the body remap's switch
/// for the arms.
#[derive(Copy, Clone)]
pub struct HandSettings {
    pub hands: HandsConfig,
    /// Scale the hands' offsets from the shoulders by the ratio of the arm spans
    /// ([`BodyRemapConfig::remap_hands`](crate::controllers::body::config::BodyRemapConfig::remap_hands)).
    pub remap_hands: bool,
}

impl HandSettings {
    /// The settings as they are now, read one group at a time.
    pub fn read() -> Self {
        let hands = *HANDS_CONFIG.lock();
        let remap_hands = BODY_REMAP_CONFIG.query(|c| c.remap_hands);
        Self { hands, remap_hands }
    }
}

/// The active hands, or `None` while the hands are inert: the controllers are disabled, the gamepad
/// scheme is in effect, no VR session publishes a head pose, or no controller frame exists. The
/// controllers' poses are composed with every stage of the hand composition
/// ([`stages`]): a hand on a physical vehicle control or a mounted gun's handle holds
/// it; while the off-hand holds a two-handed weapon's foregrip, the hands are in the two-handed
/// pose through the shared support and weight targets, and a long gun held in one hand sags,
/// trails, and sways. Physical guns use the same weighted target before their residual body
/// correction, without adding synthetic recoil or wall-probe displacement. A kinematic gun hand
/// carries its recoil offset; a palm pressed into a surface is held at it, and a hand body's hand
/// is drawn at its body; and an anchored grapple hand holds the line ([`anchor::apply`]).
pub fn active_hands() -> Option<ActiveHands> {
    compose_active_hands(DrawTime::Tick)
}

/// Compose hands from this exact controller snapshot, without reading the controller slot again.
pub(crate) fn active_hands_from_frame(
    frame: &ControllerFrame,
    at: DrawTime,
) -> Option<ActiveHands> {
    let mut active = active_hands_raw_from_frame(frame)?;
    let mut composition = HandComposition::new(
        active.hands,
        active.head,
        active.world_scale,
        active.torso,
        at,
    );
    let shoulders = shoulder_joints(
        &active.config,
        active.head,
        active.world_scale,
        active.torso,
    );
    stages::run(&mut composition, shoulders);
    active.hands = composition.hands;
    Some(active)
}

/// The active hands as the controllers hold them, before any two-handed pose: where the grab
/// volumes are tested.
pub fn active_hands_raw() -> Option<ActiveHands> {
    active_hands_raw_from_frame(&frame::latest()?)
}

fn compose_active_hands(at: DrawTime) -> Option<ActiveHands> {
    active_hands_from_frame(&frame::latest()?, at)
}

fn active_hands_raw_from_frame(frame: &ControllerFrame) -> Option<ActiveHands> {
    let config = HandSettings::read();
    let world_scale = VR_CONFIG.lock().world_scale;
    if !scheme::enabled() || !scheme::is_controllers() || headpose::source() != headpose::Source::Vr
    {
        return None;
    }
    let cockpit = headpose::xr::cockpit_pose()?;
    let head = Pose {
        position: cockpit.position,
        orientation: cockpit.orientation,
    };
    // The player's own wrists, before the remap that the torso's heading places.
    let wrists = Hand::BOTH.map(|hand| {
        frame
            .focused
            .then(|| cockpit_hand(frame, hand, &config.hands, None))
            .flatten()
            .map(|h| h.wrist.position)
    });
    let torso = body_rig::torso_heading(head, wrists);
    let remaps = arm_remaps(&config, head, torso, world_scale);
    let limits = reach_limits(&remaps, world_scale);
    let hands = Hand::BOTH.map(|hand| {
        let i = hand.index();
        let cockpit = frame
            .focused
            .then(|| cockpit_hand(frame, hand, &config.hands, remaps[i]))
            .flatten()?;
        Some(match &limits[i] {
            Some(limit) => pose::limit_reach(cockpit, limit),
            None => cockpit,
        })
    });
    Some(ActiveHands {
        config,
        head,
        hands,
        world_scale,
        torso,
    })
}

/// Each hand's grip where its controller holds it in the cockpit frame of `active`, before the
/// remap onto the character's arms and every pose after it, by [`Hand::index`]: where the player's
/// own hand is, which the drawn hand cannot follow where the character's body does not go as low
/// as the player's (a deep crouch to the floor).
pub fn controller_grips(active: &ActiveHands) -> [Option<Vec3>; 2] {
    let Some(frame) = frame::latest() else {
        return [None; 2];
    };
    Hand::BOTH.map(|hand| {
        frame
            .focused
            .then(|| cockpit_hand(&frame, hand, &active.config.hands, None))
            .flatten()
            .map(|h| h.grip.position)
    })
}

/// Hand IK ownership without composing poses or acquiring weapon and vehicle state locks.
pub(crate) fn ik_owned() -> [bool; 2] {
    let enabled = scheme::enabled();
    if !enabled
        || !scheme::is_controllers()
        || headpose::source() != headpose::Source::Vr
        || !headpose::is_active()
        || !jc3vr_core::hooks::in_gameplay()
        || headpose::xr::cockpit_pose().is_none()
    {
        return [false; 2];
    }
    let weights = body_frame::tiers().weights;
    frame::with_latest(|frame| {
        Hand::BOTH.map(|hand| {
            let h = frame.hand(hand);
            frame.focused && h.active && h.grip.is_valid() && weights[hand.index()] > 0.0
        })
    })
    .unwrap_or([false; 2])
}

/// Each shoulder joint of the character the hands are drawn on, in the cockpit frame, by
/// [`Hand::index`]: the remap's shoulder where the hands are remapped, else the player's estimated
/// shoulders, hung from the levelled eyes as the reach's are ([`level_eyes`]).
fn shoulder_joints(
    config: &HandSettings,
    head: Pose,
    world_scale: f32,
    torso: Quat,
) -> [Option<Vec3>; 2] {
    let remaps = arm_remaps(config, head, torso, world_scale);
    let estimated = body::shoulder_positions(level_eyes(head, torso), torso, &body::player());
    Hand::BOTH.map(|hand| {
        Some(remaps[hand.index()].map_or(estimated[hand.index()], |r| r.character_shoulder))
    })
}

pub fn arm_remaps(
    config: &HandSettings,
    head: Pose,
    torso: Quat,
    world_scale: f32,
) -> [Option<ArmRemap>; 2] {
    let measured = rig::arms();
    if measured.iter().all(Option::is_none) && !config.remap_hands {
        return [None; 2];
    }
    let player = body::player();
    let shoulders = body_rig::grounded::torso().map(|rig| rig.shoulders);
    Hand::BOTH.map(|hand| {
        let remap = measured[hand.index()]
            .and_then(|arm| ArmRemap::measured(hand, head, torso, &player, &arm, world_scale))
            .or_else(|| {
                config.remap_hands.then(|| {
                    ArmRemap::estimated(
                        hand,
                        head,
                        torso,
                        &player,
                        &BodyDimensions::CHARACTER_ESTIMATE,
                    )
                })
            });
        remap.map(|remap| {
            shoulders.map_or(remap, |shoulders| {
                remap.with_player_shoulder(shoulders[hand.index()])
            })
        })
    })
}

fn reach_limits(remaps: &[Option<ArmRemap>; 2], world_scale: f32) -> [Option<ReachLimit>; 2] {
    let arms = rig::arms();
    if arms.iter().all(Option::is_none) {
        return [None; 2];
    }
    Hand::BOTH.map(|hand| {
        let arm = arms[hand.index()]?;
        let remap = remaps[hand.index()]?;
        ReachLimit::new(remap.character_shoulder, &arm, world_scale)
    })
}

fn cockpit_hand(
    frame: &ControllerFrame,
    hand: Hand,
    config: &HandsConfig,
    remap: Option<ArmRemap>,
) -> Option<CockpitHand> {
    let h = frame.hand(hand);
    if !h.active || !h.grip.is_valid() {
        return None;
    }
    // The aim pose rides with the grip when the runtime reports it invalid on its own, so that the
    // overlay's ray still leaves the hand.
    let aim = if h.aim.is_valid() {
        h.aim.pose
    } else {
        h.grip.pose
    };
    Some(pose::cockpit_hand(
        hand,
        h.grip.pose,
        aim,
        &config.grip,
        remap,
    ))
}
