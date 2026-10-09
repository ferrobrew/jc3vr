//! A draw's palette edit: the bones the player's body draw re-poses, and the model-space transform
//! each one's palette entry is pre-multiplied by.

use glam::Mat4;
use jc3gi::graphics_engine::render_block::Matrix3x4;

use crate::controllers::{
    hands::{
        late::{self, PaletteEdit},
        palette::{self, PaletteLayout},
    },
    presentation::{
        rig::{Group, GroupMap},
        solve::Edits,
    },
};

/// What a draw of the player's body re-poses: the whole upper body from the presentation layer, or
/// the hands and the forearms alone from the late update when the layer is off or has nothing to
/// pose from.
// The edit sits in a draw thread's cell for one draw and is copied there once; boxing the larger
// variant would allocate on every draw of the player's body instead.
#[allow(clippy::large_enum_variant)]
#[derive(Debug, Copy, Clone, PartialEq)]
pub enum DrawEdit {
    Hands(PaletteEdit),
    Body(BodyEdit),
}

/// The upper body's edit: every bone's group and each group's edit.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct BodyEdit {
    groups: GroupMap,
    edits: Edits,
    /// Which groups' edits move anything, by bit [`Group::index`].
    moved: u16,
}

impl BodyEdit {
    /// The edit of `edits` over the bones `groups` sorts, or `None` when no group moves.
    pub fn new(groups: GroupMap, edits: Edits) -> Option<Self> {
        let moved = Group::ALL
            .iter()
            .filter(|g| !edits[g.index()].abs_diff_eq(Mat4::IDENTITY, IDENTITY_EPSILON))
            .fold(0u16, |bits, g| bits | 1 << g.index());
        (moved != 0).then_some(Self {
            groups,
            edits,
            moved,
        })
    }

    fn edit_of(&self, bone: usize) -> Option<&Mat4> {
        let group = self.groups.of(bone)?;
        (self.moved & 1 << group.index() != 0).then(|| &self.edits[group.index()])
    }
}

impl DrawEdit {
    /// Whether the edit changes `bone`'s entry.
    pub fn touches(&self, bone: usize) -> bool {
        match self {
            Self::Hands(edit) => edit.touches(bone),
            Self::Body(edit) => edit.edit_of(bone).is_some(),
        }
    }

    /// Pre-multiply the edited entries of `entries`, which spans the batch's referenced bones.
    pub fn apply(&self, entries: &mut [Matrix3x4], layout: PaletteLayout) {
        match self {
            Self::Hands(edit) => edit.apply(entries, layout),
            Self::Body(edit) => {
                for (bone, entry) in entries.iter_mut().enumerate() {
                    if let Some(transform) = edit.edit_of(bone) {
                        palette::premultiply_entry(&mut entry.m, layout, transform);
                    }
                }
                late::count_palette_edit();
            }
        }
    }

    /// The neck's edit, which carries the point the hidden head collapses to.
    pub fn neck(&self) -> Mat4 {
        match self {
            Self::Hands(_) => Mat4::IDENTITY,
            Self::Body(edit) => edit.edits[Group::Neck.index()],
        }
    }
}

/// How close to the identity an edit is left out, in matrix elements: well under a tenth of a
/// millimetre or a hundredth of a degree.
const IDENTITY_EPSILON: f32 = 1e-6;

#[cfg(test)]
mod tests {
    use glam::Vec3;
    use smol_str::SmolStr;

    use super::*;
    use crate::controllers::presentation::rig::{PresentationRig, tests::skeleton};

    fn rig() -> (Vec<SmolStr>, PresentationRig) {
        let (names, parents) = skeleton();
        let rig = PresentationRig::build(&names, &parents).unwrap();
        (names, rig)
    }

    #[test]
    fn an_edit_that_moves_nothing_is_none() {
        let (_, rig) = rig();
        assert!(BodyEdit::new(rig.groups, [Mat4::IDENTITY; Group::COUNT]).is_none());
    }

    #[test]
    fn only_the_moved_groups_bones_are_touched_and_edited() {
        let (names, rig) = rig();
        let mut edits = [Mat4::IDENTITY; Group::COUNT];
        let shift = Mat4::from_translation(Vec3::new(0.0, 0.01, 0.0));
        edits[Group::RightHand.index()] = shift;
        let edit = DrawEdit::Body(BodyEdit::new(rig.groups, edits).unwrap());
        let index = |bone: &str| names.iter().position(|n| n == bone).unwrap();
        assert!(edit.touches(index("RightHand")));
        assert!(edit.touches(index("RightHandAttach")));
        assert!(!edit.touches(index("RightForeArm")));
        assert!(!edit.touches(index("Hips")));
        let mut entries = [Matrix3x4 {
            m: palette::matrix_to_entry(&Mat4::IDENTITY, PaletteLayout::Columns),
        }; 32];
        edit.apply(&mut entries, PaletteLayout::Columns);
        let moved =
            palette::entry_to_matrix(&entries[index("RightHandAttach")].m, PaletteLayout::Columns);
        assert!(moved.abs_diff_eq(shift, 1e-6));
        let kept = palette::entry_to_matrix(&entries[index("Spine")].m, PaletteLayout::Columns);
        assert!(kept.abs_diff_eq(Mat4::IDENTITY, 1e-6));
        assert_eq!(edit.neck(), Mat4::IDENTITY);
    }
}
