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
//! - [`palette`]: the pure palette-entry math of the late update.
//!
//! Everything here is inert unless the controller scheme is in effect and the hand's controller is
//! tracked ([`active_hands`]), and each part has its toggle in [`HandsConfig`].

use glam::{Quat, Vec3};
use re_utilities::hook_library::HookLibrary;

use crate::{
    config::Config,
    controllers::{
        body::{self, BodyDimensions, alignment},
        config::{ControllersConfig, HandsConfig},
        frame::{self, ControllerFrame, Hand, Pose},
        hands::reach::ArmRemap,
        physical_hands, scheme, vehicles,
        weapons::{ads, foregrip, heft, recoil},
    },
    headpose, hooks,
};

pub mod anchor;
pub mod grip;
pub mod ik;
pub mod late;
pub mod palette;
pub mod parts;
pub mod pose;
pub mod reach;
pub mod rig;
pub mod secondary;
pub mod tiers;

pub use pose::CockpitHand;

/// The hands' hooks, aggregated by [`crate::controllers::hook_library`].
pub(crate) fn hook_library() -> HookLibrary {
    late::hook_library()
        .with_hook_library(parts::hook_library())
        .with_hook_library(anchor::hook_library())
}

/// The inputs every hand subsystem starts from: the controllers' configuration, the head's cockpit
/// pose, and each tracked hand's cockpit-frame poses after the grip offset and the remap.
#[derive(Clone)]
pub struct ActiveHands {
    pub config: ControllersConfig,
    /// The head (the eye midpoint) in the cockpit frame.
    pub head: Pose,
    /// Indexed by [`Hand::index`]; `None` for a hand whose controller is absent or not tracked.
    pub hands: [Option<CockpitHand>; 2],
    /// Each tracked hand's grip position as the player holds it, before the body-scale remap:
    /// where the player's own hand is relative to the player's own body, which is where the
    /// body's pouches and holsters are reached. Indexed and `None` as [`hands`](Self::hands).
    pub player_grips: [Option<Vec3>; 2],
    /// The VR world scale the head pose composes with.
    pub world_scale: f32,
}

/// The active hands, or `None` while the hands are inert: the controllers are disabled, the gamepad
/// scheme is in effect, no VR session publishes a head pose, or no controller frame exists. While
/// the off-hand holds a two-handed weapon's foregrip, the hands are in the two-handed pose
/// ([`foregrip::apply`]); a long gun held in one hand sags, trails, and sways ([`heft::apply`]); a
/// gun hand carries its recoil offset ([`recoil::apply`]); a palm pressed into a surface is held
/// at it ([`physical_hands::apply`]); an anchored
/// grapple hand holds the line ([`anchor::apply`]); and a hand on a physical vehicle control or a
/// mounted gun's handle holds it ([`vehicles::apply`]).
pub fn active_hands() -> Option<ActiveHands> {
    let mut active = active_hands_raw()?;
    vehicles::apply(&mut active.hands);
    foregrip::apply(&mut active.hands, &active.config.weapons.foregrip);
    heft::apply(
        &mut active.hands,
        active.head.position,
        &active.config.weapons.heft,
    );
    let torso = reach::torso_heading(active.head, alignment::shoulder_heading());
    let shoulders = shoulder_joints(&active.config, active.head, active.world_scale, torso);
    ads::apply(
        &mut active.hands,
        active.head,
        shoulders,
        torso,
        &active.config.weapons.ads,
    );
    recoil::apply(&mut active.hands, &active.config.weapons.recoil);
    physical_hands::apply(&mut active.hands);
    anchor::apply(&mut active.hands, active.head, active.world_scale);
    Some(active)
}

/// The active hands as the controllers hold them, before any two-handed pose: where the grab
/// volumes are tested.
pub fn active_hands_raw() -> Option<ActiveHands> {
    let (config, world_scale) = Config::lock_query(|c| (c.controllers, c.vr.world_scale));
    if !config.enabled || !scheme::is_controllers() || headpose::source() != headpose::Source::Vr {
        return None;
    }
    let cockpit = headpose::xr::cockpit_pose()?;
    let head = Pose {
        position: cockpit.position,
        orientation: cockpit.orientation,
    };
    let remaps = arm_remaps(&config, head, world_scale);
    let (hands, player_grips) = frame::with_latest(|frame| {
        let hands = Hand::BOTH.map(|hand| {
            frame
                .focused
                .then(|| cockpit_hand(frame, hand, &config.hands, remaps[hand.index()]))
                .flatten()
        });
        // The remap shifts the grip without turning it, so the grip before the shift is the
        // controller's own.
        let player_grips =
            Hand::BOTH.map(|hand| hands[hand.index()].map(|_| frame.hand(hand).grip.pose.position));
        (hands, player_grips)
    })?;
    Some(ActiveHands {
        config,
        head,
        hands,
        player_grips,
        world_scale,
    })
}

/// Hand IK ownership without composing poses or acquiring weapon and vehicle state locks.
pub(crate) fn ik_owned() -> [bool; 2] {
    let enabled = Config::lock_query(|c| c.controllers.enabled && c.controllers.hands.ik);
    if !enabled
        || !scheme::is_controllers()
        || headpose::source() != headpose::Source::Vr
        || !headpose::is_active()
        || !hooks::in_gameplay()
        || headpose::xr::cockpit_pose().is_none()
    {
        return [false; 2];
    }
    let weights = tiers::weights();
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
/// shoulders.
fn shoulder_joints(
    config: &ControllersConfig,
    head: Pose,
    world_scale: f32,
    torso: Quat,
) -> [Option<Vec3>; 2] {
    let remaps = arm_remaps(config, head, world_scale);
    let estimated = body::shoulder_positions(head.position, torso, &body::player());
    Hand::BOTH.map(|hand| {
        Some(remaps[hand.index()].map_or(estimated[hand.index()], |r| r.character_shoulder))
    })
}

/// Each hand's remap: about the character's measured shoulder when that is on and the arm is
/// measured, else about the estimated shoulders when `body.remap_hands` is on, else none.
fn arm_remaps(config: &ControllersConfig, head: Pose, world_scale: f32) -> [Option<ArmRemap>; 2] {
    let measured = config
        .hands
        .reach
        .measured_remap
        .then(rig::arms)
        .unwrap_or([None; 2]);
    if measured.iter().all(Option::is_none) && !config.body.remap_hands {
        return [None; 2];
    }
    let player = body::player();
    let torso = reach::torso_heading(head, alignment::shoulder_heading());
    Hand::BOTH.map(|hand| {
        measured[hand.index()]
            .and_then(|arm| ArmRemap::measured(hand, head, torso, &player, &arm, world_scale))
            .or_else(|| {
                config.body.remap_hands.then(|| {
                    ArmRemap::estimated(
                        hand,
                        head,
                        torso,
                        &player,
                        &BodyDimensions::CHARACTER_ESTIMATE,
                    )
                })
            })
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
