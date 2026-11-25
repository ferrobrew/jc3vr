//! The action's part on a gun's own skeleton: the pump-action shotgun's pump bone
//! ([`WeaponData::PUMP_BONE_HASH`]) and the bolt-action rifles' bolt bone
//! ([`WeaponData::SLIDER_BONE_HASH`]), moved where the action's travel has them.
//!
//! A gun with an animated model computes its pose in its pre-simulation update
//! ([`WeaponData::UpdatePreSim`]), and the render phase skins that pose. A post-call detour on the
//! update writes the bone's model-space joint over the pose the gun's clips gave it, through the
//! gun's animation controller, so the part is drawn where the hand has it. The clips' pose is
//! computed afresh every tick, so the write never accumulates, and a part at home is left to the
//! clips.
//!
//! The update runs on the simulation's workers for every character's guns; the published weapon is
//! checked before anything else ([`render::bone_part`]).

use std::ffi::c_void;

use detours_macro::detour;
use glam::{Quat, Vec3};
use jc3gi::{
    character::character::{AnimationController, Joint},
    equipment::weapon::WeaponData,
};
use re_utilities::hook_library::HookLibrary;

use crate::controllers::{
    hands::late,
    weapons::reload::{chamber::ActionKind, render},
};

pub(super) fn hook_library() -> HookLibrary {
    HookLibrary::new().with_static_binder(&GUN_UPDATE_PRE_SIM_BINDER)
}

/// The gun's pre-simulation update, then its action bone moved when the gun's pose was updated.
#[detour(address = jc3gi::equipment::weapon::WeaponData::UpdatePreSim_ADDRESS)]
fn gun_update_pre_sim(gun: *mut WeaponData, context: *mut c_void) -> bool {
    let updated = GUN_UPDATE_PRE_SIM.get().unwrap().call(gun, context);
    if updated {
        // SAFETY: the gun was just updated by the game on this thread, and its pose is its own.
        unsafe { drive(gun) };
    }
    updated
}

/// Move `gun`'s action bone when the published weapon's gun has its action out.
///
/// # Safety
///
/// `gun` must be a live gun whose pose this thread may write.
unsafe fn drive(gun: *mut WeaponData) {
    let Some(gun_ref) = (unsafe { gun.as_ref() }) else {
        return;
    };
    if let Some(cylinder) = render::cylinder_part(gun_ref.m_Owner as usize, gun as usize) {
        // SAFETY: as this function's.
        unsafe { drive_cylinder(gun_ref, cylinder) };
        return;
    }
    let Some(part) = render::bone_part(gun_ref.m_Owner as usize, gun as usize) else {
        return;
    };
    let hash = match part.kind {
        ActionKind::Pump => WeaponData::PUMP_BONE_HASH,
        ActionKind::Bolt => WeaponData::SLIDER_BONE_HASH,
        _ => return,
    };
    if part.share <= 0.0 && part.lift <= 0.0 {
        return;
    }
    let Some(model) = (unsafe { gun_ref.m_AnimatedModel.as_mut() }) else {
        return;
    };
    let Some(controller) = (unsafe { model.m_AnimationController.as_mut() }) else {
        return;
    };
    let Some(bone) = (unsafe { bone_index(controller, hash) }) else {
        return;
    };
    let mut joint = Joint::default();
    unsafe { controller.GetJoint(bone, &mut joint) };
    let [x, y, z] = joint.m_Translation.data;
    let [qx, qy, qz, qw] = joint.m_Orientation.data;
    let (position, orientation) =
        render::bone_joint(Vec3::new(x, y, z), Quat::from_xyzw(qx, qy, qz, qw), part);
    if !(position.is_finite() && orientation.is_finite()) {
        return;
    }
    joint.m_Translation.data = position.to_array();
    joint.m_Orientation.data = orientation.to_array();
    unsafe { controller.SetJoint(bone, &mut joint) };
}

/// Turn a revolver's crane and spin its cylinder where `cylinder` has them, over the pose the gun's
/// clips gave them: the crane about the model's Z at its own pivot, and the cylinder carried round
/// with it and spun about its own axis, which lies along Z too.
///
/// # Safety
///
/// As [`drive`].
unsafe fn drive_cylinder(gun: &WeaponData, cylinder: render::CylinderPart) {
    if cylinder.crane_rad == 0.0 && cylinder.spin_rad == 0.0 {
        return;
    }
    let Some(model) = (unsafe { gun.m_AnimatedModel.as_mut() }) else {
        return;
    };
    let Some(controller) = (unsafe { model.m_AnimationController.as_mut() }) else {
        return;
    };
    let (Some(crane), Some(spin)) = (unsafe {
        (
            bone_index(controller, WeaponData::CHAMBER_BONE_HASH),
            bone_index(controller, WeaponData::CHAMBER_SPIN_BONE_HASH),
        )
    }) else {
        return;
    };
    let mut crane_joint = Joint::default();
    let mut spin_joint = Joint::default();
    unsafe { controller.GetJoint(crane, &mut crane_joint) };
    unsafe { controller.GetJoint(spin, &mut spin_joint) };
    let (pivot, crane_turn) = joint_pose(&crane_joint);
    let (centre, spin_turn) = joint_pose(&spin_joint);
    let turn = Quat::from_rotation_z(cylinder.crane_rad);
    let shift = Vec3::X * cylinder.crane_shift_m;
    let crane_pose = (pivot + shift, turn * crane_turn);
    let spin_pose = (
        pivot + shift + turn * (centre - pivot),
        turn * Quat::from_rotation_z(cylinder.spin_rad) * spin_turn,
    );
    for (bone, joint, (position, orientation)) in [
        (crane, &mut crane_joint, crane_pose),
        (spin, &mut spin_joint, spin_pose),
    ] {
        if !(position.is_finite() && orientation.is_finite()) {
            return;
        }
        joint.m_Translation.data = position.to_array();
        joint.m_Orientation.data = orientation.normalize().to_array();
        unsafe { controller.SetJoint(bone, joint) };
    }
}

/// A joint's model-space position and orientation.
fn joint_pose(joint: &Joint) -> (Vec3, Quat) {
    let [x, y, z] = joint.m_Translation.data;
    let [qx, qy, qz, qw] = joint.m_Orientation.data;
    (Vec3::new(x, y, z), Quat::from_xyzw(qx, qy, qz, qw))
}

/// The bone named by `hash` in the gun's skeleton, or `None` when the skeleton has no such bone
/// (the lookup resolves to the root) or the index is implausible.
pub(super) unsafe fn bone_index(controller: &AnimationController, hash: u32) -> Option<u32> {
    let index = unsafe { controller.GetBoneIndex(hash) };
    (index != 0 && index <= late::MAX_BONE_INDEX).then_some(index)
}
