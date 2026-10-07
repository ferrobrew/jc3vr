//! Reading the character's skeleton: the bone names, the hierarchy, and the reference pose, from
//! the animation controller's skeleton resource (`AnimationController::m_Skeleton`, whose
//! `Skeleton::m_HavokSkeleton` is the Havok skeleton). See `docs/engine/character/skeleton.md`.
//!
//! [`dump_once`] logs the whole skeleton the first time the local player's pose is finalized, so a
//! session's log records the hierarchy the procedural body is built on.

use std::{
    ffi::CStr,
    fmt,
    sync::atomic::{AtomicBool, Ordering},
};

use glam::{Quat, Vec3};
use jc3gi::{
    animation::{pose_producer::QsTransform, skeleton::HkArray},
    character::character::{AnimationController, Character},
};
use smol_str::SmolStr;

/// A bone transform: translation, rotation, and scale, in the parent's frame or in model space
/// depending on where it came from.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct Transform {
    pub translation: Vec3,
    pub rotation: Quat,
    pub scale: Vec3,
}

impl Transform {
    /// `self` composed with `child`, a transform in `self`'s frame, the way Havok composes a
    /// `hkQsTransform` (`parent · child`).
    pub fn compose(&self, child: &Self) -> Self {
        Self {
            translation: self.translation + self.rotation * (self.scale * child.translation),
            rotation: self.rotation * child.rotation,
            scale: self.scale * child.scale,
        }
    }

    pub(crate) fn from_qs(qs: &QsTransform) -> Self {
        let [x, y, z, w] = qs.m_Rotation.data;
        Self {
            translation: Vec3::from_array(qs.m_Translation.data),
            rotation: Quat::from_xyzw(x, y, z, w),
            scale: Vec3::from_array(qs.m_Scale.data),
        }
    }

    fn is_finite(&self) -> bool {
        self.translation.is_finite() && self.rotation.is_finite() && self.scale.is_finite()
    }
}

/// One bone of a skeleton.
#[derive(Debug, Clone, PartialEq)]
pub struct Bone {
    pub name: SmolStr,
    /// The parent's index, which is always lower than the bone's own; `None` for a root.
    pub parent: Option<usize>,
    /// The reference pose's transform relative to the parent.
    pub reference: Transform,
}

/// A skeleton read from the game.
#[derive(Debug, Clone, PartialEq)]
pub struct SkeletonSnapshot {
    pub name: SmolStr,
    pub bones: Vec<Bone>,
}

impl SkeletonSnapshot {
    /// The reference pose in model space, one transform per bone.
    pub fn reference_model_space(&self) -> Vec<Transform> {
        let mut model: Vec<Transform> = Vec::with_capacity(self.bones.len());
        for bone in &self.bones {
            let transform = match bone.parent {
                Some(parent) => model[parent].compose(&bone.reference),
                None => bone.reference,
            };
            model.push(transform);
        }
        model
    }
}

/// Why a skeleton could not be read.
#[derive(Debug, Clone, PartialEq)]
pub enum SkeletonError {
    NoSkeleton,
    BoneCount(i32),
    ArrayMismatch {
        bones: i32,
        parents: i32,
        poses: i32,
    },
    BadParent {
        bone: usize,
        parent: i16,
    },
    BadName {
        bone: usize,
    },
    NonFinite {
        bone: usize,
    },
}

impl fmt::Display for SkeletonError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "skeleton: ")?;
        match self {
            Self::NoSkeleton => write!(f, "the animation controller has no skeleton"),
            Self::BoneCount(count) => write!(f, "implausible bone count {count}"),
            Self::ArrayMismatch {
                bones,
                parents,
                poses,
            } => write!(
                f,
                "the bone ({bones}), parent ({parents}), and reference pose ({poses}) counts differ"
            ),
            Self::BadParent { bone, parent } => {
                write!(
                    f,
                    "bone {bone} has parent {parent}, which does not precede it"
                )
            }
            Self::BadName { bone } => write!(f, "bone {bone} has no readable name"),
            Self::NonFinite { bone } => {
                write!(f, "bone {bone}'s reference transform is not finite")
            }
        }
    }
}

impl std::error::Error for SkeletonError {}

/// Which skeleton an animation controller holds: the controller, its skeleton resource, the Havok
/// skeleton under it, and its bone count. A rig built for one key is only valid while the
/// controller still holds that skeleton: a controller can be reused for another character, and a
/// skeleton can be swapped under a live controller.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct SkeletonKey {
    controller: usize,
    skeleton: usize,
    havok: usize,
    bones: usize,
}

impl SkeletonKey {
    /// The key of the skeleton `controller` holds now, or `None` without a plausible one.
    ///
    /// # Safety
    ///
    /// As [`read`].
    pub unsafe fn of(controller: &AnimationController) -> Option<Self> {
        let skeleton = unsafe { controller.m_Skeleton.as_ref() }?;
        let havok = unsafe { skeleton.m_HavokSkeleton.as_ref() }?;
        let count = havok.m_Bones.size;
        (1..=MAX_BONES).contains(&count).then_some(Self {
            controller: controller as *const AnimationController as usize,
            skeleton: skeleton as *const _ as usize,
            havok: havok as *const _ as usize,
            bones: count as usize,
        })
    }

    /// The skeleton's bone count: every joint index read or written must be below it, since
    /// `AnimationController::SetJoint` does not check its index.
    pub fn bones(&self) -> usize {
        self.bones
    }
}

/// Read the skeleton of an animation controller.
///
/// # Safety
///
/// `controller` must be a live animation controller, read on the thread that owns its character
/// for the duration of the call.
pub unsafe fn read(controller: &AnimationController) -> Result<SkeletonSnapshot, SkeletonError> {
    let skeleton = unsafe { controller.m_Skeleton.as_ref() }.ok_or(SkeletonError::NoSkeleton)?;
    let havok = unsafe { skeleton.m_HavokSkeleton.as_ref() }.ok_or(SkeletonError::NoSkeleton)?;
    let count = havok.m_Bones.size;
    if !(1..=MAX_BONES).contains(&count) {
        return Err(SkeletonError::BoneCount(count));
    }
    if havok.m_ParentIndices.size != count || havok.m_ReferencePose.size != count {
        return Err(SkeletonError::ArrayMismatch {
            bones: count,
            parents: havok.m_ParentIndices.size,
            poses: havok.m_ReferencePose.size,
        });
    }
    let (Some(bones), Some(parents), Some(poses)) = (
        unsafe { slice(&havok.m_Bones) },
        unsafe { slice(&havok.m_ParentIndices) },
        unsafe { slice(&havok.m_ReferencePose) },
    ) else {
        return Err(SkeletonError::NoSkeleton);
    };
    let mut out = Vec::with_capacity(bones.len());
    for (index, ((bone, &parent), pose)) in bones.iter().zip(parents).zip(poses).enumerate() {
        let parent = match parent {
            -1 => None,
            p if p >= 0 && (p as usize) < index => Some(p as usize),
            p => {
                return Err(SkeletonError::BadParent {
                    bone: index,
                    parent: p,
                });
            }
        };
        let name =
            unsafe { havok_string(bone.m_Name) }.ok_or(SkeletonError::BadName { bone: index })?;
        let reference = Transform::from_qs(pose);
        if !reference.is_finite() {
            return Err(SkeletonError::NonFinite { bone: index });
        }
        out.push(Bone {
            name,
            parent,
            reference,
        });
    }
    Ok(SkeletonSnapshot {
        name: unsafe { havok_string(havok.m_Name) }.unwrap_or_default(),
        bones: out,
    })
}

/// Log the local player's skeleton once per session: every bone's index, name, parent, and
/// reference transform, local and in model space.
///
/// # Safety
///
/// `character` must be the live local character, on the thread finalizing its pose.
pub unsafe fn dump_once(character: &Character) {
    if DUMPED.load(Ordering::Relaxed) {
        return;
    }
    let Some(controller) = (unsafe { character.m_AnimatedModel.m_AnimationController.as_ref() })
    else {
        return;
    };
    if DUMPED.swap(true, Ordering::Relaxed) {
        return;
    }
    let snapshot = match unsafe { read(controller) } {
        Ok(snapshot) => snapshot,
        Err(error) => {
            tracing::warn!(target: "body", "body: {error}");
            return;
        }
    };
    let model = snapshot.reference_model_space();
    tracing::info!(
        target: "body",
        skeleton = %snapshot.name,
        bones = snapshot.bones.len(),
        "body: the local player's skeleton",
    );
    for (index, (bone, model)) in snapshot.bones.iter().zip(&model).enumerate() {
        let parent = bone
            .parent
            .map_or(SmolStr::new_static("-"), |p| snapshot.bones[p].name.clone());
        tracing::info!(
            target: "body",
            index,
            name = %bone.name,
            parent = %parent,
            local_t = ?bone.reference.translation,
            local_r = ?bone.reference.rotation,
            model_t = ?model.translation,
            "body: skeleton bone",
        );
    }
}

/// Bone counts beyond this are garbage data, not a character skeleton.
const MAX_BONES: i32 = 1024;

static DUMPED: AtomicBool = AtomicBool::new(false);

unsafe fn slice<T>(array: &HkArray<T>) -> Option<&[T]> {
    (!array.data.is_null() && array.size > 0)
        .then(|| unsafe { std::slice::from_raw_parts(array.data, array.size as usize) })
}

/// A Havok `hkStringPtr`, whose lowest pointer bit is the string's ownership flag.
unsafe fn havok_string(pointer: *const u8) -> Option<SmolStr> {
    let pointer = (pointer as usize & !1) as *const std::ffi::c_char;
    if pointer.is_null() {
        return None;
    }
    let text = unsafe { CStr::from_ptr(pointer) }.to_str().ok()?;
    Some(SmolStr::new(text))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bone(name: &str, parent: Option<usize>, translation: Vec3, rotation: Quat) -> Bone {
        Bone {
            name: name.into(),
            parent,
            reference: Transform {
                translation,
                rotation,
                scale: Vec3::ONE,
            },
        }
    }

    #[test]
    fn model_space_composes_down_the_chain() {
        let snapshot = SkeletonSnapshot {
            name: "test".into(),
            bones: vec![
                bone("root", None, Vec3::new(0.0, 1.0, 0.0), Quat::IDENTITY),
                bone(
                    "child",
                    Some(0),
                    Vec3::new(0.0, 0.5, 0.0),
                    Quat::from_rotation_z(std::f32::consts::FRAC_PI_2),
                ),
                bone(
                    "grandchild",
                    Some(1),
                    Vec3::new(1.0, 0.0, 0.0),
                    Quat::IDENTITY,
                ),
            ],
        };
        let model = snapshot.reference_model_space();
        assert!(
            model[1]
                .translation
                .abs_diff_eq(Vec3::new(0.0, 1.5, 0.0), 1e-6)
        );
        // The child's quarter turn about Z carries the grandchild's +X onto +Y.
        assert!(
            model[2]
                .translation
                .abs_diff_eq(Vec3::new(0.0, 2.5, 0.0), 1e-5)
        );
    }
}
