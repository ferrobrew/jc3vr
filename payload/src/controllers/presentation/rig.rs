//! The bones the presentation layer re-poses, found by name in the character's skeleton, and the
//! group every other bone rides with. Pure.
//!
//! The upper body is re-posed as a few rigid groups, each rooted at one bone: the four torso joints
//! the spine bends at (`Spine1`, `Spine2`, `Sternum`, and `Neck`), the head, and per arm the upper
//! arm, the forearm, and the hand. Every bone takes the edit of the nearest group root at or above
//! it, so the clavicles ride the sternum, the twist bones their limb segment, the fingers and the
//! attach bones their hand, and the facial bones the head; a bone under no group root (the hips,
//! the legs, the lower spine, the targets under the root) is left as the simulation posed it.

use std::fmt;

use smol_str::SmolStr;

/// A rigidly re-posed group of bones, named by its root bone.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum Group {
    Spine1,
    Spine2,
    Sternum,
    Neck,
    Head,
    LeftArm,
    LeftForeArm,
    LeftHand,
    RightArm,
    RightForeArm,
    RightHand,
}

impl Group {
    /// Every group, parents before children.
    pub const ALL: [Self; 11] = [
        Self::Spine1,
        Self::Spine2,
        Self::Sternum,
        Self::Neck,
        Self::Head,
        Self::LeftArm,
        Self::LeftForeArm,
        Self::LeftHand,
        Self::RightArm,
        Self::RightForeArm,
        Self::RightHand,
    ];

    pub const COUNT: usize = Self::ALL.len();

    /// The torso joints the spine bends at, base first.
    pub const SPINE: [Self; 4] = [Self::Spine1, Self::Spine2, Self::Sternum, Self::Neck];

    pub const fn index(self) -> usize {
        self as usize
    }

    /// The group's root bone's name in the skeleton.
    pub const fn bone(self) -> &'static str {
        match self {
            Self::Spine1 => "Spine1",
            Self::Spine2 => "Spine2",
            Self::Sternum => "Sternum",
            Self::Neck => "Neck",
            Self::Head => "Head",
            Self::LeftArm => "LeftArm",
            Self::LeftForeArm => "LeftForeArm",
            Self::LeftHand => "LeftHand",
            Self::RightArm => "RightArm",
            Self::RightForeArm => "RightForeArm",
            Self::RightHand => "RightHand",
        }
    }

    /// The group whose root must be an ancestor of this one's for the edits to compose as the
    /// solve assumes: each spine joint on the one below, the head on the neck, each arm's upper arm
    /// on the sternum (through its clavicle), and each forearm and hand on the segment above.
    pub const fn above(self) -> Option<Self> {
        match self {
            Self::Spine1 => None,
            Self::Spine2 => Some(Self::Spine1),
            Self::Sternum => Some(Self::Spine2),
            Self::Neck => Some(Self::Sternum),
            Self::Head => Some(Self::Neck),
            Self::LeftArm | Self::RightArm => Some(Self::Sternum),
            Self::LeftForeArm => Some(Self::LeftArm),
            Self::LeftHand => Some(Self::LeftForeArm),
            Self::RightForeArm => Some(Self::RightArm),
            Self::RightHand => Some(Self::RightForeArm),
        }
    }

    /// The arm groups of one side: the upper arm, the forearm, and the hand. `side` is
    /// [`Hand::index`](crate::controllers::frame::Hand::index), left then right.
    pub const fn arm(side: usize) -> [Self; 3] {
        if side == 0 {
            [Self::LeftArm, Self::LeftForeArm, Self::LeftHand]
        } else {
            [Self::RightArm, Self::RightForeArm, Self::RightHand]
        }
    }
}

/// The largest skeleton the group map covers: the palette the draws upload spans no more bones.
pub const MAX_BONES: usize = 256;

/// Each bone's group, by skeleton index; bones past [`MAX_BONES`] have none.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct GroupMap([u8; MAX_BONES]);

impl GroupMap {
    /// The group bone `bone` rides with, or `None` for a bone the presentation leaves alone.
    pub fn of(&self, bone: usize) -> Option<Group> {
        let group = *self.0.get(bone)?;
        Group::ALL.get(group as usize).copied()
    }
}

const NO_GROUP: u8 = u8::MAX;

/// The presentation layer's rig of one skeleton.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct PresentationRig {
    /// Each group's root bone's skeleton index, by [`Group::index`].
    pub roots: [usize; Group::COUNT],
    pub groups: GroupMap,
}

/// Why a skeleton has no presentation rig.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RigError {
    Missing(&'static str),
    NotUnder {
        bone: &'static str,
        ancestor: &'static str,
    },
}

impl fmt::Display for RigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "presentation rig: ")?;
        match self {
            Self::Missing(bone) => write!(f, "the skeleton has no bone named {bone}"),
            Self::NotUnder { bone, ancestor } => write!(f, "{bone} does not hang under {ancestor}"),
        }
    }
}

impl std::error::Error for RigError {}

impl PresentationRig {
    /// The rig of a skeleton whose bones are named `names` and whose parents are `parents`, by
    /// index, each parent before its child.
    pub fn build(names: &[SmolStr], parents: &[Option<usize>]) -> Result<Self, RigError> {
        let find = |name: &'static str| {
            names
                .iter()
                .position(|n| n == name)
                .ok_or(RigError::Missing(name))
        };
        let mut roots = [0; Group::COUNT];
        for group in Group::ALL {
            roots[group.index()] = find(group.bone())?;
        }
        let descends = |mut bone: usize, ancestor: usize| loop {
            if bone == ancestor {
                return true;
            }
            match parents.get(bone).copied().flatten() {
                Some(parent) if parent < bone => bone = parent,
                _ => return false,
            }
        };
        for group in Group::ALL {
            if let Some(above) = group.above()
                && !descends(roots[group.index()], roots[above.index()])
            {
                return Err(RigError::NotUnder {
                    bone: group.bone(),
                    ancestor: above.bone(),
                });
            }
        }
        let mut map = [NO_GROUP; MAX_BONES];
        for bone in 0..names.len().min(MAX_BONES) {
            map[bone] = match Group::ALL.iter().position(|g| roots[g.index()] == bone) {
                Some(group) => group as u8,
                None => parents
                    .get(bone)
                    .copied()
                    .flatten()
                    .filter(|&parent| parent < bone)
                    .map_or(NO_GROUP, |parent| map[parent]),
            };
        }
        Ok(Self {
            roots,
            groups: GroupMap(map),
        })
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// A skeleton shaped like the shipped one's upper body: the root, the hips with the legs on a
    /// sibling of the spine, the spine to the head with a facial bone, and both arms with a twist
    /// bone and fingers.
    pub(crate) fn skeleton() -> (Vec<SmolStr>, Vec<Option<usize>>) {
        let bones: &[(&str, Option<&str>)] = &[
            ("Reference", None),
            ("Hips", Some("Reference")),
            ("Pelvis", Some("Hips")),
            ("LeftUpLeg", Some("Pelvis")),
            ("Spine", Some("Hips")),
            ("Spine1", Some("Spine")),
            ("Spine2", Some("Spine1")),
            ("BackAttach", Some("Spine2")),
            ("Sternum", Some("Spine2")),
            ("Neck", Some("Sternum")),
            ("Head", Some("Neck")),
            ("fJaw", Some("Head")),
            ("LeftShoulder", Some("Sternum")),
            ("LeftArm", Some("LeftShoulder")),
            ("df_LeftArmRollA01", Some("LeftArm")),
            ("LeftForeArm", Some("LeftArm")),
            ("LeftForeArmRoll", Some("LeftForeArm")),
            ("LeftHand", Some("LeftForeArm")),
            ("LeftHandIndex1", Some("LeftHand")),
            ("RightShoulder", Some("Sternum")),
            ("RightArm", Some("RightShoulder")),
            ("RightForeArm", Some("RightArm")),
            ("RightHand", Some("RightForeArm")),
            ("RightHandAttach", Some("RightHand")),
            ("LeftHandIKTarget", Some("Reference")),
        ];
        let names: Vec<SmolStr> = bones.iter().map(|(n, _)| SmolStr::new(n)).collect();
        let parents = bones
            .iter()
            .map(|(_, parent)| parent.map(|p| names.iter().position(|n| n == p).unwrap()))
            .collect();
        (names, parents)
    }

    fn group_of(rig: &PresentationRig, names: &[SmolStr], bone: &str) -> Option<Group> {
        rig.groups.of(names.iter().position(|n| n == bone).unwrap())
    }

    #[test]
    fn every_bone_rides_the_nearest_group_root_above_it() {
        let (names, parents) = skeleton();
        let rig = PresentationRig::build(&names, &parents).unwrap();
        let cases = [
            ("Reference", None),
            ("Hips", None),
            ("LeftUpLeg", None),
            ("Spine", None),
            ("LeftHandIKTarget", None),
            ("Spine1", Some(Group::Spine1)),
            ("BackAttach", Some(Group::Spine2)),
            ("LeftShoulder", Some(Group::Sternum)),
            ("Neck", Some(Group::Neck)),
            ("fJaw", Some(Group::Head)),
            ("df_LeftArmRollA01", Some(Group::LeftArm)),
            ("LeftForeArmRoll", Some(Group::LeftForeArm)),
            ("LeftHandIndex1", Some(Group::LeftHand)),
            ("RightHandAttach", Some(Group::RightHand)),
        ];
        for (bone, group) in cases {
            assert_eq!(group_of(&rig, &names, bone), group, "{bone}");
        }
        assert_eq!(rig.groups.of(MAX_BONES + 3), None);
        assert_eq!(rig.roots[Group::Head.index()], 10);
    }

    #[test]
    fn a_skeleton_without_a_group_root_or_with_it_elsewhere_is_refused() {
        let (mut names, parents) = skeleton();
        names[10] = SmolStr::new("Skull");
        assert_eq!(
            PresentationRig::build(&names, &parents),
            Err(RigError::Missing("Head"))
        );
        let (names, mut parents) = skeleton();
        // The left arm hung from the spine's root rather than from the sternum.
        parents[12] = Some(5);
        assert_eq!(
            PresentationRig::build(&names, &parents),
            Err(RigError::NotUnder {
                bone: "LeftArm",
                ancestor: "Sternum"
            })
        );
    }
}
