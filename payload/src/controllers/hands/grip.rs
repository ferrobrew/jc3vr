//! The rigid grip: a gun held in a controller-driven hand is fixed to that hand's wrist.
//!
//! The game places a hand-held gun at the world matrix of its attach bone, as it is
//! ([`WeaponData::UpdateAttachmentTransforms`]), and the attach bone is an animated bone whose pose
//! relative to the wrist comes from the clips playing. With the wrist driven by a controller, any
//! clip that keys the attach bone turns the gun in a hand the player is holding still (see
//! `docs/engine/character/hands-locomotion-and-state.md`, "The gun's pose in the hand is
//! animated"). So before each placement of the local player's weapons
//! ([`Inventory::UpdateAttachmentTransforms`], whose detour lives in
//! [`crate::controllers::weapons::dual_wield::attach`]), every gun on a hand attach bone of a hand
//! with a controller weight above zero ([`tiers::weights`]) has that bone written
//! ([`AnimationController::SetJoint`]) to the hand's current wrist composed with the hand's
//! canonical grip: the primary attach bone's pose relative to the wrist in the skeleton's bind
//! pose ([`AnimationController::GetBindPoseBoneMatrix`]). The written pose is blended from the
//! animated one by the hand's weight ([`blend_pin`]), so the gun settles into the hand as the hand
//! blends onto its controller and back out as it yields. The placements run after the state tasks,
//! after the `MAIN` solve, and after the `SECONDARY` solve, so the last one reads the solved wrist
//! and the gun's cached transform, which the muzzle effects, the gun's model parts, its physics
//! proxy, the shot origin, and the render-time late update all derive from, is rigid in the hand.
//!
//! The canonical grip is the same for both of a hand's attach bones, so a state task moving the gun
//! between them does not turn it either. A gun on a holster or back bone, and a hand the
//! controllers do not drive, are left to the game. Each placement also records how the hand's
//! animated attach bone sat relative to the canonical grip before the pin, and how the gun sits on
//! the wrist after the placement, for [`crate::controllers::weapons::grip_watch`].

use std::{
    sync::atomic::{AtomicU8, AtomicU64, Ordering},
    time::Instant,
};

use glam::{Mat4, Quat, Vec3};
use jc3gi::{
    character::{
        character::{AnimationController, Character, Joint, SafeBoneIndex},
        inventory::Inventory,
    },
    equipment::weapon::{WeaponBase, WeaponData, WeaponState},
    types::math::Matrix4,
};
use parking_lot::Mutex;

use crate::{
    config::Config,
    controllers::{
        aim::gun,
        frame::{Hand, Pose},
        hands::{ik, pose as hand_pose, tiers},
    },
};

/// One hand's gun as its last placement left it.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct HandGrip {
    /// When the placement ran.
    pub at: Instant,
    /// The weapon's address and the gun's index in it.
    pub weapon: usize,
    pub gun: usize,
    /// The attach bone the gun rides (an `NBone::ESafeBoneIndex`).
    pub bone: u32,
    /// The weapon's state at the placement.
    pub state: WeaponState,
    /// Whether the attach bone was pinned to the canonical grip.
    pub pinned: bool,
    /// The angle between the animated attach bone's pose on the wrist and the canonical grip, before
    /// the pin, degrees.
    pub animated_deviation_deg: f32,
    /// The animated attach bone's rotation relative to the wrist, before the pin.
    pub animated_in_wrist: Quat,
    /// The gun's rotation relative to the solved wrist bone, after the placement.
    pub gun_in_wrist: Quat,
    /// The solved wrist bone's world rotation relative to the tick's wrist target, when the hand had
    /// one. Its change over a grab is how far the solve drifted from the target.
    pub wrist_in_target: Option<Quat>,
    /// The distance from the tick's wrist target to the solved wrist bone, metres.
    pub wrist_miss_m: Option<f32>,
    /// The tick's IK weight for the hand, when it had a target.
    pub weight: Option<f32>,
}

/// The rigid grip's readout.
#[derive(Debug, Copy, Clone, PartialEq, Default)]
pub struct GripStatus {
    /// Each hand's gun at its last placement, indexed by [`Hand::index`].
    pub hands: [Option<HandGrip>; 2],
    /// Attach bones pinned since the payload loaded.
    pub pins: u64,
}

pub fn status() -> GripStatus {
    GripStatus {
        hands: *LAST.lock(),
        pins: PINS.load(Ordering::Relaxed),
    }
}

/// How a hand's last placement fared at reading its canonical grip.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
#[repr(u8)]
pub(crate) enum GripRead {
    /// The canonical grip was read, or found cached.
    Read = 1,
    /// The wrist or an attach bone did not resolve on the skeleton.
    BonesUnresolved,
    /// The bind pose gave no usable wrist-to-attach pose.
    DegenerateBindPose,
}

/// Each hand's last canonical grip read, indexed by [`Hand::index`], or `None` before the hand
/// placed a gun. Atomics only, so any thread may read it.
pub(crate) fn grip_reads() -> [Option<GripRead>; 2] {
    READS
        .each_ref()
        .map(|read| match read.load(Ordering::Relaxed) {
            1 => Some(GripRead::Read),
            2 => Some(GripRead::BonesUnresolved),
            3 => Some(GripRead::DegenerateBindPose),
            _ => None,
        })
}

/// Pin the attach bone of each gun the local player holds in a controller-driven hand, then let the
/// caller run the placement; returns what [`after_placement`] needs. Called from the inventory's
/// placement detour, before the original.
///
/// # Safety
///
/// `inventory` and `character` must be null or live, on the thread running `character`'s
/// pose-finalization pass.
pub(crate) unsafe fn before_placement(
    inventory: *mut Inventory,
    character: *const Character,
) -> Option<Placement> {
    let local = ik::local_character();
    if local == 0 || character as usize != local {
        return None;
    }
    let character = unsafe { character.as_ref() }?;
    let inventory = unsafe { inventory.as_ref() }?;
    if !character.m_IsLocalCharacter {
        return None;
    }
    let controller = unsafe { character.m_AnimatedModel.m_AnimationController.as_mut() }?;
    let enabled = Config::lock_query(|c| c.controllers.hands.rigid_grip);
    let targets = ik::tick_records().current;
    let weights = tiers::weights();
    let mut placement = Placement {
        guns: [None; 2],
        targets: targets.map(|t| t.map(|t| (t.wrist, t.weight))),
    };
    for weapon in held_weapons(inventory, character) {
        let state = weapon.m_State;
        for (index, data) in unsafe { gun::guns(weapon) }.enumerate() {
            let Some(data) = data else {
                continue;
            };
            let Some(&bone) = data.m_lBoneAttachementPerWeaponState.get(state as usize) else {
                continue;
            };
            let Some((hand, attach)) = hand_attach(bone) else {
                continue;
            };
            if placement.guns[hand.index()].is_some() {
                continue;
            }
            // The tier weight, not whether a target was queued: a hand blending back to the
            // animation has no target on the tick it reaches zero, and a hand blending in has one
            // at a small weight, so keying the pin on the target switched the grip at a stroke.
            let weight = if enabled { weights[hand.index()] } else { 0.0 };
            let read = unsafe { pin(character, controller, hand, attach, weight) };
            let Some(read) = read else {
                continue;
            };
            placement.guns[hand.index()] = Some(PlacedGun {
                weapon: weapon as *const WeaponBase as usize,
                data: data as *const WeaponData,
                gun: index,
                bone,
                state,
                read,
            });
        }
    }
    Some(placement)
}

/// Record how each pinned or watched gun sits on its wrist after the placement.
///
/// # Safety
///
/// As [`before_placement`], with the placement's guns still live.
pub(crate) unsafe fn after_placement(character: *const Character, placement: Placement) {
    let Some(character) = (unsafe { character.as_ref() }) else {
        return;
    };
    let Some(controller) = (unsafe { character.m_AnimatedModel.m_AnimationController.as_ref() })
    else {
        return;
    };
    let to_world = Mat4::from(character.m_WorldMatrixT1);
    let (_, body, _) = to_world.to_scale_rotation_translation();
    let now = Instant::now();
    let mut last = LAST.lock();
    for hand in Hand::BOTH {
        let Some(placed) = placement.guns[hand.index()] else {
            continue;
        };
        let data = unsafe { &*placed.data };
        let (_, gun, _) = Mat4::from(data.m_WorldMatrixT1).to_scale_rotation_translation();
        let wrist = unsafe { joint_pose(controller, placed.read.wrist_index) };
        let wrist_world_rotation = (body * wrist.orientation).normalize();
        let wrist_world_position = to_world.transform_point3(wrist.position);
        let target = placement.targets[hand.index()];
        last[hand.index()] = Some(HandGrip {
            at: now,
            weapon: placed.weapon,
            gun: placed.gun,
            bone: placed.bone,
            state: placed.state,
            pinned: placed.read.pinned,
            animated_deviation_deg: placed.read.animated_deviation_deg,
            animated_in_wrist: placed.read.animated_in_wrist,
            gun_in_wrist: (wrist_world_rotation.inverse() * gun).normalize(),
            wrist_in_target: target
                .map(|(t, _)| (t.orientation.inverse() * wrist_world_rotation).normalize()),
            wrist_miss_m: target.map(|(t, _)| t.position.distance(wrist_world_position)),
            weight: target.map(|(_, w)| w),
        });
    }
}

/// The guns one placement covers, from [`before_placement`] to [`after_placement`].
#[derive(Debug, Copy, Clone)]
pub(crate) struct Placement {
    guns: [Option<PlacedGun>; 2],
    targets: [Option<(Pose, f32)>; 2],
}

/// The hand and the hand's primary attach bone for a hand attach bone, or `None` for any other
/// bone.
pub fn hand_attach(bone: u32) -> Option<(Hand, SafeBoneIndex)> {
    [
        SafeBoneIndex::ATTACH_HAND_LEFT,
        SafeBoneIndex::ATTACH_HAND_LEFT2,
        SafeBoneIndex::ATTACH_HAND_RIGHT,
        SafeBoneIndex::ATTACH_HAND_RIGHT2,
    ]
    .into_iter()
    .find(|&safe| safe as u32 == bone)
    .map(|safe| match safe {
        SafeBoneIndex::ATTACH_HAND_LEFT | SafeBoneIndex::ATTACH_HAND_LEFT2 => (Hand::Left, safe),
        _ => (Hand::Right, safe),
    })
}

/// A readable name for an attach bone, for the logs.
pub fn bone_label(bone: u32) -> &'static str {
    [
        (SafeBoneIndex::ATTACH_HAND_LEFT, "AttachHandLeft"),
        (SafeBoneIndex::ATTACH_HAND_LEFT2, "AttachHandLeft2"),
        (SafeBoneIndex::ATTACH_HAND_RIGHT, "AttachHandRight"),
        (SafeBoneIndex::ATTACH_HAND_RIGHT2, "AttachHandRight2"),
        (SafeBoneIndex::LEFT_HOLSTER, "LeftHolster"),
        (SafeBoneIndex::RIGHT_HOLSTER, "RightHolster"),
        (SafeBoneIndex::BACK_ATTACH, "BackAttach"),
        (SafeBoneIndex::BACK_ATTACH_2, "BackAttach2"),
    ]
    .into_iter()
    .find(|&(safe, _)| safe as u32 == bone)
    .map_or("other", |(_, label)| label)
}

/// The attach bone's pose `offset` carried onto a model-space `wrist` joint of scale
/// `wrist_scale`, as the pose producer composes a child onto its parent.
pub fn pinned_attach(wrist: Pose, wrist_scale: Vec3, offset: Pose) -> Pose {
    Pose {
        position: wrist.position + wrist.orientation * (wrist_scale * offset.position),
        orientation: (wrist.orientation * offset.orientation).normalize(),
    }
}

/// The attach bone's pose for a hand at `weight` between the animation (`0`) and the controller
/// (`1`): the `animated` pose carried toward the `pinned` one by the weight, so the gun settles
/// into the hand as the hand blends onto its controller and back out as it yields, rather than
/// switching between the two at a stroke.
pub fn blend_pin(animated: Pose, pinned: Pose, weight: f32) -> Pose {
    let weight = if weight.is_finite() {
        weight.clamp(0.0, 1.0)
    } else {
        0.0
    };
    hand_pose::interp(animated, pinned, weight)
}

/// The rigid part of `attach` relative to `wrist`, both model-space matrices, or `None` for a
/// degenerate matrix.
pub fn relative_pose(wrist: Mat4, attach: Mat4) -> Option<Pose> {
    let (scale, orientation, position) = (wrist.inverse() * attach).to_scale_rotation_translation();
    (scale.is_finite() && orientation.is_finite() && position.is_finite()).then(|| Pose {
        position,
        orientation: orientation.normalize(),
    })
}

/// The angle between two rotations, degrees.
pub fn angle_deg(a: Quat, b: Quat) -> f32 {
    let delta = (a.inverse() * b).normalize();
    let delta = if delta.w < 0.0 { -delta } else { delta };
    delta.to_axis_angle().1.to_degrees()
}

/// One gun a placement covers.
#[derive(Debug, Copy, Clone)]
struct PlacedGun {
    weapon: usize,
    data: *const WeaponData,
    gun: usize,
    bone: u32,
    state: WeaponState,
    read: PinRead,
}

/// What [`pin`] read and did.
#[derive(Debug, Copy, Clone)]
struct PinRead {
    wrist_index: u32,
    pinned: bool,
    animated_deviation_deg: f32,
    animated_in_wrist: Quat,
}

/// The local player's weapons wielded in the hands: a slot weapon in a wielding state whose user is
/// `character`.
fn held_weapons<'a>(
    inventory: &'a Inventory,
    character: &'a Character,
) -> impl Iterator<Item = &'a WeaponBase> + 'a {
    inventory.m_Weapons.iter().filter_map(move |slot| {
        let weapon = unsafe { slot.px.as_ref() }?;
        (matches!(
            weapon.m_State,
            WeaponState::E_WS_WIELDING | WeaponState::E_WS_DUAL_WIELDING
        ) && std::ptr::eq(weapon.m_User[0].cast::<Character>(), character))
        .then_some(weapon)
    })
}

/// Read `hand`'s wrist and the gun's attach bone `attach`, and with a `weight` above zero, write
/// the attach bone onto the wrist at the hand's canonical grip, blended from the animated pose by
/// the weight ([`blend_pin`]). `None` when a bone does not resolve or the skeleton gives no
/// canonical grip.
unsafe fn pin(
    character: &Character,
    controller: &mut AnimationController,
    hand: Hand,
    attach: SafeBoneIndex,
    weight: f32,
) -> Option<PinRead> {
    let (wrist_safe, primary_safe) = match hand {
        Hand::Left => (SafeBoneIndex::LEFT_HAND, SafeBoneIndex::ATTACH_HAND_LEFT),
        Hand::Right => (SafeBoneIndex::RIGHT_HAND, SafeBoneIndex::ATTACH_HAND_RIGHT),
    };
    let wrist_index = unsafe { character.GetSafeIndex(wrist_safe) };
    let attach_index = unsafe { character.GetSafeIndex(attach) };
    let primary_index = unsafe { character.GetSafeIndex(primary_safe) };
    if wrist_index == 0 || attach_index == 0 || primary_index == 0 || attach_index == wrist_index {
        record_read(hand, GripRead::BonesUnresolved);
        return None;
    }
    let Some(offset) = (unsafe { canonical_grip(controller, hand, wrist_index, primary_index) })
    else {
        record_read(hand, GripRead::DegenerateBindPose);
        return None;
    };
    record_read(hand, GripRead::Read);
    let mut wrist_joint = Joint::default();
    unsafe { controller.GetJoint(wrist_index, &mut wrist_joint) };
    let mut attach_joint = Joint::default();
    unsafe { controller.GetJoint(attach_index, &mut attach_joint) };
    let wrist = joint_to_pose(&wrist_joint);
    let animated = joint_to_pose(&attach_joint);
    let animated_in_wrist = (wrist.orientation.inverse() * animated.orientation).normalize();
    let animated_deviation_deg = angle_deg(animated_in_wrist, offset.orientation);
    let wrist_scale = Vec3::from_array(wrist_joint.m_Scale.data);
    let pinned_pose = blend_pin(animated, pinned_attach(wrist, wrist_scale, offset), weight);
    let pinned =
        weight > 0.0 && pinned_pose.position.is_finite() && pinned_pose.orientation.is_finite();
    if pinned {
        attach_joint.m_Translation.data = pinned_pose.position.to_array();
        attach_joint.m_Orientation.data = pinned_pose.orientation.to_array();
        unsafe { controller.SetJoint(attach_index, &mut attach_joint) };
        PINS.fetch_add(1, Ordering::Relaxed);
    }
    Some(PinRead {
        wrist_index,
        pinned,
        animated_deviation_deg,
        animated_in_wrist,
    })
}

/// `hand`'s canonical grip: its primary attach bone's pose relative to its wrist in the skeleton's
/// bind pose, cached per animation controller.
unsafe fn canonical_grip(
    controller: &AnimationController,
    hand: Hand,
    wrist_index: u32,
    primary_index: u32,
) -> Option<Pose> {
    let key = GripKey {
        controller: controller as *const AnimationController as usize,
        wrist_index,
        primary_index,
    };
    if let Some(cached) = GRIPS.lock()[hand.index()].filter(|(k, _)| *k == key) {
        return Some(cached.1);
    }
    let bind = |index| {
        let mut matrix = Matrix4::default();
        unsafe { controller.GetBindPoseBoneMatrix(index, &mut matrix) };
        Mat4::from(matrix)
    };
    let grip = relative_pose(bind(wrist_index), bind(primary_index))?;
    tracing::info!(
        target: "controllers",
        ?hand,
        offset_m = ?grip.position,
        rotation_deg = angle_deg(Quat::IDENTITY, grip.orientation),
        "rigid grip: the canonical grip from the bind pose"
    );
    GRIPS.lock()[hand.index()] = Some((key, grip));
    Some(grip)
}

fn record_read(hand: Hand, read: GripRead) {
    READS[hand.index()].store(read as u8, Ordering::Relaxed);
}

fn joint_to_pose(joint: &Joint) -> Pose {
    let [x, y, z] = joint.m_Translation.data;
    let [qx, qy, qz, qw] = joint.m_Orientation.data;
    Pose {
        position: Vec3::new(x, y, z),
        orientation: Quat::from_xyzw(qx, qy, qz, qw).normalize(),
    }
}

unsafe fn joint_pose(controller: &AnimationController, index: u32) -> Pose {
    let mut joint = Joint::default();
    unsafe { controller.GetJoint(index, &mut joint) };
    joint_to_pose(&joint)
}

/// What a cached canonical grip was read for.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
struct GripKey {
    controller: usize,
    wrist_index: u32,
    primary_index: u32,
}

static GRIPS: Mutex<[Option<(GripKey, Pose)>; 2]> = Mutex::new([None; 2]);
static LAST: Mutex<[Option<HandGrip>; 2]> = Mutex::new([None; 2]);
static PINS: AtomicU64 = AtomicU64::new(0);
/// Each hand's last [`GripRead`] as its discriminant, `0` before the hand placed a gun.
static READS: [AtomicU8; 2] = [const { AtomicU8::new(0) }; 2];

#[cfg(test)]
mod tests {
    use super::*;

    fn pose(position: Vec3, orientation: Quat) -> Pose {
        Pose {
            position,
            orientation,
        }
    }

    #[test]
    fn the_pinned_attach_rides_the_wrist_at_the_bind_offset() {
        let bind_wrist = Mat4::from_rotation_translation(Quat::from_rotation_y(0.4), Vec3::Y);
        let offset = pose(Vec3::new(0.1, 0.0, 0.02), Quat::from_rotation_x(0.3));
        let bind_attach =
            bind_wrist * Mat4::from_rotation_translation(offset.orientation, offset.position);
        let grip = relative_pose(bind_wrist, bind_attach).unwrap();
        assert!(grip.position.distance(offset.position) < 1e-5);
        assert!(angle_deg(grip.orientation, offset.orientation) < 1e-2);

        let wrist = pose(Vec3::new(3.0, 1.0, -2.0), Quat::from_rotation_z(1.2));
        let attach = pinned_attach(wrist, Vec3::ONE, grip);
        let back = hand_pose::mul(hand_pose::inverse(wrist), attach);
        assert!(back.position.distance(offset.position) < 1e-5);
        assert!(angle_deg(back.orientation, offset.orientation) < 1e-2);
    }

    #[test]
    fn the_pin_blends_from_the_animation_by_the_hand_weight() {
        let animated = pose(Vec3::new(0.1, 0.0, 0.0), Quat::from_rotation_y(1.2));
        let pinned = pose(Vec3::new(0.0, 0.05, 0.0), Quat::from_rotation_x(0.4));
        let at = |w| blend_pin(animated, pinned, w);
        assert!(at(0.0).position.distance(animated.position) < 1e-6);
        assert!(angle_deg(at(0.0).orientation, animated.orientation) < 1e-2);
        assert!(at(1.0).position.distance(pinned.position) < 1e-6);
        assert!(angle_deg(at(1.0).orientation, pinned.orientation) < 1e-2);
        // Part way, the gun is part way between, and the turn grows steadily with the weight.
        let whole = angle_deg(animated.orientation, pinned.orientation);
        let mut last = 0.0;
        for step in 1..=10 {
            let w = step as f32 / 10.0;
            let turned = angle_deg(animated.orientation, at(w).orientation);
            assert!((turned - whole * w).abs() < 0.1, "{w}: {turned}");
            assert!(turned > last);
            last = turned;
        }
        // Out-of-range and non-finite weights stay within the two poses.
        assert!(at(2.0).position.distance(pinned.position) < 1e-6);
        assert!(at(f32::NAN).position.distance(animated.position) < 1e-6);
    }

    #[test]
    fn a_turned_wrist_carries_the_attach_bone_rigidly() {
        let grip = pose(Vec3::new(0.1, 0.0, 0.0), Quat::from_rotation_x(0.2));
        let wrist = pose(Vec3::ZERO, Quat::from_rotation_y(0.7));
        let a = pinned_attach(wrist, Vec3::ONE, grip);
        let turned = pose(Vec3::ZERO, Quat::from_rotation_x(1.0) * wrist.orientation);
        let c = pinned_attach(turned, Vec3::ONE, grip);
        let in_wrist = |w: Pose, p: Pose| hand_pose::mul(hand_pose::inverse(w), p);
        assert!(
            angle_deg(
                in_wrist(wrist, a).orientation,
                in_wrist(turned, c).orientation
            ) < 1e-2
        );
    }

    #[test]
    fn a_scaled_wrist_scales_the_offset() {
        let grip = pose(Vec3::new(0.1, 0.0, 0.0), Quat::IDENTITY);
        let attach = pinned_attach(Pose::IDENTITY, Vec3::splat(2.0), grip);
        assert!(attach.position.distance(Vec3::new(0.2, 0.0, 0.0)) < 1e-6);
    }

    #[test]
    fn hand_attach_bones_map_to_their_hands() {
        assert_eq!(
            hand_attach(SafeBoneIndex::ATTACH_HAND_RIGHT2 as u32),
            Some((Hand::Right, SafeBoneIndex::ATTACH_HAND_RIGHT2))
        );
        assert_eq!(
            hand_attach(SafeBoneIndex::ATTACH_HAND_LEFT as u32),
            Some((Hand::Left, SafeBoneIndex::ATTACH_HAND_LEFT))
        );
        assert_eq!(hand_attach(SafeBoneIndex::RIGHT_HOLSTER as u32), None);
        assert_eq!(
            bone_label(SafeBoneIndex::RIGHT_HOLSTER as u32),
            "RightHolster"
        );
    }

    #[test]
    fn a_flip_measures_as_a_large_angle() {
        let held = Quat::from_rotation_y(0.3);
        assert!(angle_deg(held, -held) < 1e-3);
        assert!(angle_deg(held, Quat::from_rotation_x(std::f32::consts::PI) * held) > 179.0);
    }
}
