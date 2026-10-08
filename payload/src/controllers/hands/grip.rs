//! The rigid grip: a gun held in a controller-driven hand is fixed to that hand's wrist.
//!
//! The game places a hand-held gun at the world matrix of its attach bone, as it is
//! ([`WeaponData::UpdateAttachmentTransforms`]), and the attach bone is an animated bone whose pose
//! relative to the wrist comes from the clips playing. With the wrist driven by a controller, any
//! clip that keys the attach bone turns the gun in a hand the player is holding still (see
//! `docs/engine/character/hands-locomotion-and-state.md`, "The gun's pose in the hand is
//! animated"). So before each placement of the local player's weapons
//! ([`Inventory::UpdateAttachmentTransforms`], whose detour lives in
//! [`crate::controllers::weapons::hold`]), every gun on a hand attach bone of a hand
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
//! between them does not turn it either. A gun the hold puts in the left hand (any weapon but the
//! dual-wield pair under its own layout) can take the right hand's grip mirrored across the body
//! instead ([`LeftGrip`]): the left attach bone's bind pose is the one the game's left-hand pistol
//! uses, and a long gun on it may sit turned. The automatic choice keeps the left hand's own grip
//! unless it is turned far from the mirrored one, and the readout reports the angle between the two
//! ([`left_grip_read`]). A gun whose body the hand bodies hold
//! ([`crate::controllers::physical_hands::physics::guns`]) is pinned where that body is: the
//! canonical grip carried by how far the gun body strays from its seat on the holding hand's body
//! ([`seat_on_body`]), so the drawn gun, and everything that derives from its placement, follows the
//! gun body while the hand is drawn at its own. A gun on a holster or back bone, and a hand the
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
        physical_hands::physics::guns,
        weapons::{
            config::LeftGrip,
            hold::{
                self,
                act::{self, SnapStart, SnapView},
                second,
            },
        },
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

/// How the left hand's grip for a gun the hold put there was chosen, from its last placement.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct LeftGripRead {
    /// The angle between the left hand's own bind-pose grip and the right hand's grip mirrored onto
    /// the left wrist, degrees.
    pub angle_deg: f32,
    /// Whether the mirrored grip is the one used.
    pub mirrored: bool,
}

/// The left hand's grip read at its last placement of a gun the hold put there. A leaf lock, so any
/// thread may read it.
pub fn left_grip_read() -> Option<LeftGripRead> {
    *LEFT_GRIP.lock()
}

/// The model-space axis across the body: the one the two wrists' bind positions differ most
/// along.
pub fn lateral_axis(left: Vec3, right: Vec3) -> usize {
    let span = (left - right).abs();
    if span.x >= span.y && span.x >= span.z {
        0
    } else if span.y >= span.z {
        1
    } else {
        2
    }
}

/// `pose` mirrored across the plane through the origin across `axis`: its position reflected, and
/// its rotation the reflected frame's, turned back to a right-handed one by flipping the frame's
/// own `axis`, which is the mirror image of a body symmetric across its own `axis`.
pub fn mirror_pose(pose: Pose, axis: usize) -> Pose {
    let mut position = pose.position;
    position[axis] = -position[axis];
    let q = pose.orientation;
    let mut v = Vec3::new(q.x, q.y, q.z);
    let kept = v[axis];
    v = -v;
    v[axis] = kept;
    Pose {
        position,
        orientation: Quat::from_xyzw(v.x, v.y, v.z, q.w).normalize(),
    }
}

/// The right hand's grip mirrored onto the left wrist: the right attach bone's bind pose
/// `right_attach` mirrored across `axis`, relative to the left wrist's bind pose `left_wrist`.
pub fn mirrored_grip(left_wrist: Mat4, right_attach: Mat4, axis: usize) -> Option<Pose> {
    let attach = relative_pose(Mat4::IDENTITY, right_attach)?;
    let mirrored = mirror_pose(attach, axis);
    relative_pose(
        left_wrist,
        Mat4::from_rotation_translation(mirrored.orientation, mirrored.position),
    )
}

/// The grip the left hand takes for `choice`, from its own bind-pose grip `own` and the right's
/// mirrored, and whether it is the mirrored one.
pub fn pick_left_grip(choice: LeftGrip, own: Pose, mirrored: Pose) -> (Pose, bool) {
    let use_mirrored = match choice {
        LeftGrip::BindPose => false,
        LeftGrip::MirroredRight => true,
        LeftGrip::Auto => angle_deg(own.orientation, mirrored.orientation) > AUTO_TURN_DEG,
    };
    if use_mirrored {
        (mirrored, true)
    } else {
        (own, false)
    }
}

/// How far the left hand's own grip may turn from the mirrored one before the automatic choice
/// takes the mirrored one, degrees.
pub const AUTO_TURN_DEG: f32 = 45.0;

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
    let (enabled, left_grip) = Config::lock_query(|c| {
        (
            c.controllers.hands.rigid_grip,
            c.controllers.weapons.hold.left_grip,
        )
    });
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
            let address = weapon as *const WeaponBase as usize;
            let snap = act::snap_view(address, hand);
            // The hold's weapons in the left hand may take the mirrored grip; the dual-wield pair
            // under its own layout keeps the left hand's own, which suits its pistols.
            let held_by_hold =
                hold::owned_holder(address) == Some(Hand::Left) || second::is(address);
            // A sliding gun is not in the hand yet, so it has no body to follow.
            let deviation = snap
                .is_none()
                .then(|| guns::deviation(hand, address))
                .flatten();
            let grip = PinGrip {
                left: (hand == Hand::Left && held_by_hold).then_some(left_grip),
                weight,
                snap,
                deviation,
            };
            let read = unsafe { pin(character, controller, hand, attach, grip) };
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

/// The attach bone's pose `held` on the wrist carried by `deviation`, the gun body's stray from
/// its seat on the holding hand's body, in the gun's own frame: where the gun body is, drawn on the
/// hand.
pub fn seat_on_body(held: Pose, deviation: Pose) -> Pose {
    hand_pose::mul(held, deviation)
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

/// The attach bone's pose part of the way, `toward_hand`, from where the gun was (`start`: on its
/// dock, or where a hand took it in the world) to the hand's grip `held`, both in model space.
pub fn slide_grip(start: Pose, held: Pose, toward_hand: f32) -> Pose {
    let toward_hand = if toward_hand.is_finite() {
        toward_hand.clamp(0.0, 1.0)
    } else {
        1.0
    };
    hand_pose::interp(start, held, toward_hand)
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

/// The model-space pose of `character`'s skeleton bone `bone`, as its animation controller has it
/// now, or `None` when the bone does not resolve.
///
/// # Safety
///
/// `character` must be live, read on the thread updating it or outside the world update.
pub(crate) unsafe fn bone_model_pose(character: &Character, bone: SafeBoneIndex) -> Option<Pose> {
    let controller = unsafe { character.m_AnimatedModel.m_AnimationController.as_ref() }?;
    let index = unsafe { character.GetSafeIndex(bone) };
    if index == 0 {
        return None;
    }
    let pose = unsafe { joint_pose(controller, index) };
    (pose.position.is_finite() && pose.orientation.is_finite()).then_some(pose)
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

/// The local player's weapons in the hands: a slot weapon in a wielding state, or the second weapon
/// ([`second`]), whose user is `character`.
fn held_weapons<'a>(
    inventory: &'a Inventory,
    character: &'a Character,
) -> impl Iterator<Item = &'a WeaponBase> + 'a {
    let in_slots = inventory.m_Weapons.iter().filter_map(|slot| {
        let weapon = unsafe { slot.px.as_ref() }?;
        let held = matches!(
            weapon.m_State,
            WeaponState::E_WS_WIELDING | WeaponState::E_WS_DUAL_WIELDING
        ) || second::is(weapon as *const WeaponBase as usize);
        held.then_some(weapon)
    });
    // A second weapon the VR inventory carries is in no slot.
    // SAFETY: the placement runs inside the world update for the local player's inventory.
    let carried = unsafe { second::carried_weapon() };
    in_slots
        .chain(carried)
        .filter(move |weapon| std::ptr::eq(weapon.m_User[0].cast::<Character>(), character))
}

/// How [`pin`] places a gun's attach bone.
#[derive(Debug, Copy, Clone)]
struct PinGrip {
    /// The left hand's grip choice, for a gun the hold put in the left hand.
    left: Option<LeftGrip>,
    /// The hand's controller weight.
    weight: f32,
    /// A snap moving the gun between where it was and the hand.
    snap: Option<SnapView>,
    /// The gun body's stray from its seat on the holding hand's body, in the gun's own frame, when
    /// the hand bodies hold the gun.
    deviation: Option<Pose>,
}

/// Read `hand`'s wrist and the gun's attach bone `attach`, and with a weight above zero, write the
/// attach bone onto the wrist at the hand's canonical grip (or the left hand's chosen grip),
/// blended from the animated pose by the weight ([`blend_pin`]). While a snap moves the gun, the
/// grip is first carried from where the gun was toward the hand by the snap's progress. `None`
/// when a bone does not resolve or the skeleton gives no canonical grip.
unsafe fn pin(
    character: &Character,
    controller: &mut AnimationController,
    hand: Hand,
    attach: SafeBoneIndex,
    grip: PinGrip,
) -> Option<PinRead> {
    let PinGrip {
        left,
        weight,
        snap,
        deviation,
    } = grip;
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
    let Some(own) = (unsafe { canonical_grip(controller, hand, wrist_index, primary_index) })
    else {
        record_read(hand, GripRead::DegenerateBindPose);
        return None;
    };
    let offset = match left {
        Some(choice) if hand == Hand::Left => {
            unsafe { left_grip(character, controller, wrist_index, own, choice) }.unwrap_or(own)
        }
        _ => own,
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
    let mut held = pinned_attach(wrist, wrist_scale, offset);
    if let Some(deviation) = deviation {
        held = seat_on_body(held, deviation);
    }
    if let Some(snap) = snap
        && let Some(start) = unsafe { snap_start(character, snap.start) }
    {
        held = slide_grip(start, held, snap.toward_hand);
    }
    let pinned_pose = blend_pin(animated, held, weight);
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

/// Where a snap's far end is in `character`'s model space: the gun's pose on its dock bone as the
/// skeleton has it now, or a pose in the world carried into the model.
unsafe fn snap_start(character: &Character, start: SnapStart) -> Option<Pose> {
    match start {
        SnapStart::Dock { dock, offset } => {
            let bone = unsafe { bone_model_pose(character, dock) }?;
            Some(hand_pose::mul(bone, offset))
        }
        SnapStart::World(world) => relative_pose(
            Mat4::from(character.m_WorldMatrixT1),
            Mat4::from_rotation_translation(world.orientation, world.position),
        ),
    }
}

/// The left hand's grip for `choice`, from its own canonical grip `own` and the right hand's
/// mirrored onto the left wrist, which is cached per animation controller. Records the choice for
/// the readout. `None` when the right hand's bones do not resolve.
unsafe fn left_grip(
    character: &Character,
    controller: &AnimationController,
    left_wrist: u32,
    own: Pose,
    choice: LeftGrip,
) -> Option<Pose> {
    let key = GripKey {
        controller: controller as *const AnimationController as usize,
        wrist_index: left_wrist,
        primary_index: 0,
    };
    let cached = MIRRORED.lock().filter(|(k, _)| *k == key).map(|(_, m)| m);
    let mirrored = match cached {
        Some(mirrored) => mirrored,
        None => {
            let right_wrist = unsafe { character.GetSafeIndex(SafeBoneIndex::RIGHT_HAND) };
            let right_attach = unsafe { character.GetSafeIndex(SafeBoneIndex::ATTACH_HAND_RIGHT) };
            if right_wrist == 0 || right_attach == 0 {
                return None;
            }
            let bind = |index| {
                let mut matrix = Matrix4::default();
                unsafe { controller.GetBindPoseBoneMatrix(index, &mut matrix) };
                Mat4::from(matrix)
            };
            let left = bind(left_wrist);
            let axis = lateral_axis(left.w_axis.truncate(), bind(right_wrist).w_axis.truncate());
            let mirrored = mirrored_grip(left, bind(right_attach), axis)?;
            tracing::info!(
                target: "controllers",
                axis,
                offset_m = ?mirrored.position,
                own_offset_m = ?own.position,
                turn_from_own_deg = angle_deg(own.orientation, mirrored.orientation),
                "rigid grip: the right hand's grip mirrored onto the left wrist"
            );
            *MIRRORED.lock() = Some((key, mirrored));
            mirrored
        }
    };
    let (grip, used) = pick_left_grip(choice, own, mirrored);
    *LEFT_GRIP.lock() = Some(LeftGripRead {
        angle_deg: angle_deg(own.orientation, mirrored.orientation),
        mirrored: used,
    });
    Some(grip)
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
/// The right hand's grip mirrored onto the left wrist, and the controller it was read for.
static MIRRORED: Mutex<Option<(GripKey, Pose)>> = Mutex::new(None);
static LEFT_GRIP: Mutex<Option<LeftGripRead>> = Mutex::new(None);
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
    fn a_gun_on_a_body_is_drawn_where_its_body_strays() {
        let held = pose(Vec3::new(0.2, 1.1, -0.3), Quat::from_rotation_y(1.0));
        assert_eq!(seat_on_body(held, Pose::IDENTITY), held);
        // A gun body sagging 2 cm along its own down axis is drawn 2 cm down that axis.
        let sag = pose(Vec3::new(0.0, -0.02, 0.0), Quat::from_rotation_x(-0.05));
        let drawn = seat_on_body(held, sag);
        assert!(drawn.position.abs_diff_eq(
            held.position + held.orientation * Vec3::new(0.0, -0.02, 0.0),
            1e-6
        ));
        assert!(angle_deg(drawn.orientation, held.orientation) > 2.0);
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
    fn a_grip_slides_from_where_the_gun_was_to_the_hand() {
        let dock = pose(Vec3::new(0.2, 1.2, -0.1), Quat::from_rotation_z(0.6));
        let offset = pose(Vec3::new(0.0, 0.1, 0.0), Quat::from_rotation_x(0.3));
        let held = pose(Vec3::new(0.4, 1.0, 0.3), Quat::from_rotation_y(1.0));
        let on_dock = hand_pose::mul(dock, offset);
        let at = |t| slide_grip(on_dock, held, t);
        assert!(at(0.0).position.distance(on_dock.position) < 1e-5);
        assert!(angle_deg(at(0.0).orientation, on_dock.orientation) < 1e-2);
        assert!(at(1.0).position.distance(held.position) < 1e-5);
        let half = at(0.5).position;
        assert!(half.distance((on_dock.position + held.position) / 2.0) < 1e-5);
        // A bad progress puts the gun in the hand.
        assert!(at(f32::NAN).position.distance(held.position) < 1e-5);
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

    /// A rig symmetric across X: the right side's pose and the left's, its mirror.
    fn symmetric(right: Pose) -> (Mat4, Mat4) {
        let left = mirror_pose(right, 0);
        (
            Mat4::from_rotation_translation(right.orientation, right.position),
            Mat4::from_rotation_translation(left.orientation, left.position),
        )
    }

    #[test]
    fn a_symmetric_rig_mirrors_onto_its_own_left_grip() {
        let (right_wrist, left_wrist) = symmetric(pose(
            Vec3::new(0.35, 1.2, 0.1),
            Quat::from_euler(glam::EulerRot::XYZ, 0.3, -0.7, 0.2),
        ));
        let offset = pose(Vec3::new(0.08, -0.02, 0.03), Quat::from_rotation_z(0.4));
        let right_attach =
            right_wrist * Mat4::from_rotation_translation(offset.orientation, offset.position);
        let left_attach = {
            let a = relative_pose(Mat4::IDENTITY, right_attach).unwrap();
            let m = mirror_pose(a, 0);
            Mat4::from_rotation_translation(m.orientation, m.position)
        };
        let own = relative_pose(left_wrist, left_attach).unwrap();
        let axis = lateral_axis(left_wrist.w_axis.truncate(), right_wrist.w_axis.truncate());
        assert_eq!(axis, 0);
        let mirrored = mirrored_grip(left_wrist, right_attach, axis).unwrap();
        assert!(mirrored.position.distance(own.position) < 1e-4);
        assert!(angle_deg(mirrored.orientation, own.orientation) < 0.05);
        // The automatic choice keeps the left hand's own grip when the two agree.
        assert_eq!(pick_left_grip(LeftGrip::Auto, own, mirrored), (own, false));
    }

    #[test]
    fn mirroring_twice_gives_the_pose_back() {
        let p = pose(
            Vec3::new(0.1, -0.2, 0.3),
            Quat::from_euler(glam::EulerRot::YXZ, 0.5, 0.2, -1.1),
        );
        for axis in 0..3 {
            let back = mirror_pose(mirror_pose(p, axis), axis);
            assert!(back.position.distance(p.position) < 1e-6);
            assert!(angle_deg(back.orientation, p.orientation) < 1e-2);
        }
    }

    #[test]
    fn the_automatic_grip_turns_to_the_mirror_only_for_a_turned_left_grip() {
        let own = pose(Vec3::ZERO, Quat::IDENTITY);
        let near = pose(Vec3::X * 0.01, Quat::from_rotation_y(0.2));
        let flipped = pose(Vec3::ZERO, Quat::from_rotation_x(std::f32::consts::PI));
        assert_eq!(pick_left_grip(LeftGrip::Auto, own, near), (own, false));
        assert_eq!(
            pick_left_grip(LeftGrip::Auto, own, flipped),
            (flipped, true)
        );
        assert_eq!(
            pick_left_grip(LeftGrip::BindPose, own, flipped),
            (own, false)
        );
        assert_eq!(
            pick_left_grip(LeftGrip::MirroredRight, own, near),
            (near, true)
        );
    }

    #[test]
    fn the_lateral_axis_is_the_widest_span() {
        assert_eq!(
            lateral_axis(Vec3::new(-0.3, 1.0, 0.0), Vec3::new(0.3, 1.0, 0.0)),
            0
        );
        assert_eq!(
            lateral_axis(Vec3::new(0.0, 1.0, 0.4), Vec3::new(0.0, 1.1, -0.4)),
            2
        );
    }

    #[test]
    fn a_flip_measures_as_a_large_angle() {
        let held = Quat::from_rotation_y(0.3);
        assert!(angle_deg(held, -held) < 1e-3);
        assert!(angle_deg(held, Quat::from_rotation_x(std::f32::consts::PI) * held) > 179.0);
    }
}
