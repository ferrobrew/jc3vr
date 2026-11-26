//! A draw's palette edit: the bones the player's body draw re-poses, and the model-space transform
//! each one's palette entry is pre-multiplied by.

use std::cell::RefCell;

use glam::{Mat4, Vec3};
use jc3gi::graphics_engine::render_block::Matrix3x4;
use jc3vr_view::hooks::graphics_engine::palette_edit::{
    BodyPaletteEdit, PaletteLayout, PlayerDrawEdits,
};

use crate::controllers::{
    hands::{
        late::{self, PaletteEdit},
        palette,
    },
    presentation::{
        draw_edit,
        rig::{Group, GroupMap, MAX_BONES},
        solve::Edits,
    },
    telemetry,
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
    sample: Option<DrawnArms>,
    registration: Option<PaletteRegistration>,
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
            sample: None,
            registration: None,
        })
    }

    pub(crate) fn registered(mut self, registration: PaletteRegistration) -> Self {
        self.registration = Some(registration);
        self
    }

    fn palette_edits(&self, entries: &[Matrix3x4], layout: PaletteLayout) -> Option<Edits> {
        let Some(registration) = self.registration else {
            return Some(self.edits);
        };
        let mut edits = self.edits;
        for group in [
            Group::LeftShoulder,
            Group::LeftArm,
            Group::LeftForeArm,
            Group::LeftHand,
            Group::RightShoulder,
            Group::RightArm,
            Group::RightForeArm,
            Group::RightHand,
        ] {
            let i = group.index();
            let entry = entries.get(registration.roots[i])?;
            let actual = palette::entry_to_matrix(&entry.m, layout) * registration.bind[i];
            edits[i] = self.edits[i] * registration.source[i] * actual.inverse();
        }
        for side in 0..2 {
            let [upper, fore, hand] = Group::arm(side).map(Group::index);
            let clavicle = if side == 0 {
                Group::LeftShoulder
            } else {
                Group::RightShoulder
            }
            .index();
            // Each segment samples its own distal bind point. The child's origin alone does not
            // describe where the parent's skinning matrix places that same point.
            let wanted = [upper, fore, hand]
                .map(|i| self.edits[i].transform_point3(registration.source[i].w_axis.truncate()));
            let clavicle_point = self.edits[clavicle]
                .transform_point3(registration.source[clavicle].w_axis.truncate());
            for (segment, end, start_point, end_point) in [
                (clavicle, upper, clavicle_point, wanted[0]),
                (upper, fore, wanted[0], wanted[1]),
                (fore, hand, wanted[1], wanted[2]),
            ] {
                let entry = entries.get(registration.roots[segment])?;
                let entry = palette::entry_to_matrix(&entry.m, layout);
                let source = [registration.bind[segment], registration.bind[end]]
                    .map(|bind| entry.transform_point3(bind.w_axis.truncate()));
                edits[segment] =
                    palette::fit_segment(edits[segment], source, [start_point, end_point])?;
            }
        }
        edits.iter().all(|edit| edit.is_finite()).then_some(edits)
    }

    pub(crate) fn with_arm_sample(mut self, sample: DrawnArms) -> Self {
        self.sample = Some(sample);
        self
    }

    pub(crate) fn hand_transform(&self, hand: usize) -> Mat4 {
        self.edits[Group::arm(hand)[2].index()]
    }

    pub(crate) fn forearm_transform(&self, hand: usize) -> Mat4 {
        self.edits[Group::arm(hand)[1].index()]
    }

    fn edit_of(&self, bone: usize) -> Option<&Mat4> {
        let group = self.groups.of(bone)?;
        (self.registration.is_some() || self.moved & 1 << group.index() != 0)
            .then(|| &self.edits[group.index()])
    }
}

#[derive(Debug, Copy, Clone, PartialEq)]
pub(crate) struct PaletteRegistration {
    pub(crate) roots: [usize; Group::COUNT],
    pub(crate) bind: [Mat4; Group::COUNT],
    pub(crate) source: [Mat4; Group::COUNT],
}

#[derive(Debug, Copy, Clone, PartialEq)]
pub(crate) struct DrawnArms {
    pub(crate) sequence: u64,
    pub(crate) dtf: f32,
    pub(crate) bones: [[usize; 3]; 2],
    pub(crate) bind_points: [[Vec3; 3]; 2],
}

impl DrawEdit {
    pub(crate) fn source_len(&self) -> usize {
        match self {
            Self::Hands(edit) => edit.source_len(),
            Self::Body(edit) => edit.registration.map_or(0, |registration| {
                registration
                    .roots
                    .into_iter()
                    .chain((0..MAX_BONES).filter(|&bone| edit.groups.roll(bone).is_some()))
                    .max()
                    .map_or(0, |bone| bone + 1)
            }),
        }
    }

    pub(crate) fn sample_drawn_arms(&self, entries: &[Matrix3x4], layout: PaletteLayout) {
        let Self::Body(body) = self else {
            return;
        };
        let Some(sample) = body.sample else {
            return;
        };
        if let Some(joints) =
            telemetry::drawn_arm_joints(entries, layout, sample.bones, sample.bind_points)
        {
            telemetry::sample_drawn_arms(sample.sequence, sample.dtf, joints);
        }
    }

    /// Whether the edit changes `bone`'s entry.
    pub fn touches(&self, bone: usize) -> bool {
        match self {
            Self::Hands(edit) => edit.touches(bone),
            Self::Body(edit) => edit.edit_of(bone).is_some(),
        }
    }

    /// Pre-multiply the edited entries of `entries`, which spans the batch's referenced bones.
    pub fn apply(&self, entries: &mut [Matrix3x4], source: &[Matrix3x4], layout: PaletteLayout) {
        match self {
            Self::Hands(edit) => edit.apply(entries, source, layout),
            Self::Body(edit) => {
                let Some(edits) = edit.palette_edits(source, layout) else {
                    return;
                };
                for (bone, entry) in entries.iter_mut().enumerate() {
                    if edit.edit_of(bone).is_some()
                        && let Some(group) = edit.groups.of(bone)
                    {
                        let mut transform = edits[group.index()];
                        if let (Some(along), Some(registration)) =
                            (edit.groups.roll(bone), edit.registration)
                        {
                            let side = usize::from(group == Group::RightForeArm);
                            let [_, fore, hand] = Group::arm(side).map(Group::index);
                            let elbow = edit.edits[fore]
                                .transform_point3(registration.source[fore].w_axis.truncate());
                            let wrist = edit.edits[hand]
                                .transform_point3(registration.source[hand].w_axis.truncate());
                            if let Some(axis) = (wrist - elbow).try_normalize() {
                                let frame = |bone: usize, bind: Mat4, correction: Mat4| {
                                    source.get(bone).map(|entry| {
                                        correction
                                            * palette::entry_to_matrix(&entry.m, layout)
                                            * bind
                                    })
                                };
                                if let (Some(forearm), Some(palm), Some(roll)) = (
                                    frame(
                                        registration.roots[fore],
                                        registration.bind[fore],
                                        edits[fore],
                                    ),
                                    frame(
                                        registration.roots[hand],
                                        registration.bind[hand],
                                        edits[hand],
                                    ),
                                    frame(
                                        bone,
                                        Mat4::from_quat(edit.groups.roll_bind(bone)),
                                        transform,
                                    ),
                                ) {
                                    let bind = [
                                        palette::bone_rotation(registration.bind[fore]),
                                        palette::bone_rotation(registration.bind[hand]),
                                    ];
                                    if let [Some(fore_bind), Some(hand_bind)] = bind {
                                        let angle = palette::roll_correction(
                                            [forearm, palm, roll],
                                            [fore_bind, hand_bind, edit.groups.roll_bind(bone)],
                                            axis,
                                            along,
                                        );
                                        transform =
                                            palette::roll_about(elbow, wrist, angle) * transform;
                                    }
                                }
                            }
                        }
                        palette::premultiply_entry(&mut entry.m, layout, &transform);
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

impl BodyPaletteEdit for DrawEdit {
    fn touches(&self, bone: usize) -> bool {
        DrawEdit::touches(self, bone)
    }

    fn source_len(&self) -> usize {
        DrawEdit::source_len(self)
    }

    fn neck(&self) -> Mat4 {
        DrawEdit::neck(self)
    }

    fn apply(&self, entries: &mut [Matrix3x4], source: &[Matrix3x4], layout: PaletteLayout) {
        DrawEdit::apply(self, entries, source, layout);
    }

    fn sample_drawn_arms(&self, entries: &[Matrix3x4], layout: PaletteLayout) {
        DrawEdit::sample_drawn_arms(self, entries, layout);
    }
}

/// The provider of the render blocks' edits of the player's body: each draw's [`DrawEdit`], held
/// for the drawing thread.
pub(crate) struct PlayerBodyEdits;

impl PlayerDrawEdits for PlayerBodyEdits {
    fn begin(&self, instance: Mat4, dtf: f32) -> bool {
        let edit = draw_edit(instance, dtf);
        let held = edit.is_some();
        DRAW_EDIT.with(|cell| *cell.borrow_mut() = edit);
        held
    }

    fn end(&self) {
        DRAW_EDIT.with(|cell| *cell.borrow_mut() = None);
    }

    fn with(&self, f: &mut dyn FnMut(Option<&dyn BodyPaletteEdit>)) {
        DRAW_EDIT.with(|cell| {
            let edit = cell.borrow();
            f(edit.as_ref().map(|edit| edit as &dyn BodyPaletteEdit));
        });
    }
}

thread_local! {
    /// The edit of this thread's draw of the player's body. Thread-local: character draws run on
    /// several render worker threads at once, and an edit leaked into a concurrent draw of another
    /// skeleton would re-pose its bones by index collision.
    static DRAW_EDIT: RefCell<Option<DrawEdit>> = const { RefCell::new(None) };
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
    fn actual_palette_segments_meet_at_shared_joints_in_both_layouts() {
        let (names, rig) = rig();
        let mut bind = [Mat4::IDENTITY; Group::COUNT];
        let mut source = bind;
        for (i, group) in Group::ALL.into_iter().enumerate() {
            let point = match group {
                Group::LeftShoulder | Group::RightShoulder => Vec3::new(0.08, 1.4, 0.0),
                Group::LeftArm | Group::RightArm => Vec3::new(0.2, 1.4, 0.0),
                Group::LeftForeArm | Group::RightForeArm => Vec3::new(0.2, 1.15, 0.0),
                Group::LeftHand | Group::RightHand => Vec3::new(0.2, 0.91, 0.0),
                _ => Vec3::new(0.0, 1.5, 0.0),
            };
            bind[i] = Mat4::from_rotation_translation(glam::Quat::from_rotation_z(0.4), point);
            source[i] = Mat4::from_translation(point);
        }
        let mut edits = [Mat4::IDENTITY; Group::COUNT];
        edits[Group::RightHand.index()] = Mat4::from_translation(Vec3::new(0.03, -0.08, -0.1));
        let body = BodyEdit::new(rig.groups, edits)
            .unwrap()
            .registered(PaletteRegistration {
                roots: rig.roots,
                bind,
                source,
            });
        for layout in [PaletteLayout::Columns, PaletteLayout::Rows] {
            let mut entries = vec![
                Matrix3x4 {
                    m: palette::matrix_to_entry(&Mat4::IDENTITY, layout)
                };
                names.len()
            ];
            for group in Group::ALL {
                let i = group.index();
                let actual = Mat4::from_rotation_translation(
                    glam::Quat::from_rotation_z(-0.8),
                    source[i].w_axis.truncate() + Vec3::new(0.07, 0.04, 0.02),
                );
                entries[rig.roots[i]].m =
                    palette::matrix_to_entry(&(actual * bind[i].inverse()), layout);
            }
            let source_entries = entries.clone();
            DrawEdit::Body(body).apply(&mut entries, &source_entries, layout);
            let at = |group: Group, point: Vec3| {
                palette::entry_to_matrix(&entries[rig.roots[group.index()]].m, layout)
                    .transform_point3(point)
            };
            for [parent, child] in [
                [Group::RightShoulder, Group::RightArm],
                [Group::RightArm, Group::RightForeArm],
                [Group::RightForeArm, Group::RightHand],
            ] {
                let joint = bind[child.index()].w_axis.truncate();
                let expected =
                    edits[child.index()].transform_point3(source[child.index()].w_axis.truncate());
                assert!(at(parent, joint).abs_diff_eq(expected, 1e-5));
                assert!(at(child, joint).abs_diff_eq(expected, 1e-5));
            }
        }
    }

    #[test]
    fn a_singular_source_palette_leaves_the_whole_upload_unchanged() {
        let (_, rig) = rig();
        let mut edits = [Mat4::IDENTITY; Group::COUNT];
        edits[Group::RightHand.index()] = Mat4::from_translation(Vec3::X);
        let edit = DrawEdit::Body(BodyEdit::new(rig.groups, edits).unwrap().registered(
            PaletteRegistration {
                roots: rig.roots,
                bind: [Mat4::IDENTITY; Group::COUNT],
                source: [Mat4::IDENTITY; Group::COUNT],
            },
        ));
        let mut entries = vec![Matrix3x4 { m: [0.0; 12] }; 32];
        let source = entries.clone();
        edit.apply(&mut entries, &source, PaletteLayout::Rows);
        assert!(entries.iter().all(|entry| entry.m == [0.0; 12]));
    }

    fn registered_arm(layout: PaletteLayout) -> (DrawEdit, Vec<Matrix3x4>, usize) {
        let (names, rig) = rig();
        let mut bind = vec![Mat4::IDENTITY; names.len()];
        for side in 0..2 {
            let shoulder = if side == 0 {
                Group::LeftShoulder
            } else {
                Group::RightShoulder
            };
            for (group, x, angle) in [
                (shoulder, 0.0, 0.0),
                (Group::arm(side)[0], 0.1, 0.0),
                (Group::arm(side)[1], 0.4, 0.2),
                (Group::arm(side)[2], 0.7, -0.1),
            ] {
                bind[rig.roots[group.index()]] = Mat4::from_rotation_translation(
                    glam::Quat::from_rotation_x(angle),
                    Vec3::X * x,
                );
            }
        }
        let roll = names
            .iter()
            .position(|name| name == "LeftForeArmRoll")
            .unwrap();
        bind[roll] =
            Mat4::from_rotation_translation(glam::Quat::from_rotation_x(0.4), Vec3::X * 0.55);
        let groups = rig.groups.with_rolls(&names, &bind);
        let group_bind = rig.roots.map(|bone| bind[bone]);
        let mut source = group_bind;
        source[Group::LeftForeArm.index()] =
            Mat4::from_rotation_translation(glam::Quat::from_rotation_x(0.3), Vec3::X * 0.4);
        source[Group::LeftHand.index()] =
            Mat4::from_rotation_translation(glam::Quat::from_rotation_x(1.1), Vec3::X * 0.7);
        let mut entries = vec![
            Matrix3x4 {
                m: palette::matrix_to_entry(&Mat4::IDENTITY, layout)
            };
            names.len()
        ];
        for (bone, angle) in [
            (rig.roots[Group::LeftForeArm.index()], 0.6),
            (rig.roots[Group::LeftHand.index()], -0.8),
            (roll, 1.7),
        ] {
            let actual = Mat4::from_rotation_translation(
                glam::Quat::from_rotation_x(angle),
                bind[bone].w_axis.truncate(),
            );
            entries[bone].m = palette::matrix_to_entry(&(actual * bind[bone].inverse()), layout);
        }
        let mut edits = [Mat4::IDENTITY; Group::COUNT];
        edits[Group::Neck.index()] = Mat4::from_translation(Vec3::Y * 0.01);
        let body = BodyEdit::new(groups, edits)
            .unwrap()
            .registered(PaletteRegistration {
                roots: rig.roots,
                bind: group_bind,
                source,
            });
        (DrawEdit::Body(body), entries, roll)
    }

    #[test]
    fn registered_roll_uses_bone_frames_and_replaces_native_roll() {
        for layout in [PaletteLayout::Columns, PaletteLayout::Rows] {
            let (edit, source, roll) = registered_arm(layout);
            let mut full = source.clone();
            edit.apply(&mut full, &source, layout);
            let bind =
                Mat4::from_rotation_translation(glam::Quat::from_rotation_x(0.4), Vec3::X * 0.55);
            let frame = palette::entry_to_matrix(&full[roll].m, layout) * bind;
            assert!(
                palette::bone_rotation(frame)
                    .unwrap()
                    .abs_diff_eq(glam::Quat::from_rotation_x(1.05), 1e-5)
            );
            let mut short = source[..=roll].to_vec();
            assert!(edit.source_len() > short.len());
            edit.apply(&mut short, &source, layout);
            for (short, full) in short.iter().zip(&full) {
                assert_eq!(short.m, full.m);
            }
        }
    }

    #[test]
    fn coincident_registered_segments_keep_every_uploaded_entry_finite() {
        for layout in [PaletteLayout::Columns, PaletteLayout::Rows] {
            for [start, end] in [
                [Group::LeftShoulder, Group::LeftArm],
                [Group::LeftArm, Group::LeftForeArm],
                [Group::LeftForeArm, Group::LeftHand],
            ] {
                let (DrawEdit::Body(mut body), source, _) = registered_arm(layout) else {
                    unreachable!()
                };
                let registration = body.registration.unwrap();
                let point = registration.source[start.index()].w_axis.truncate();
                body.edits[end.index()] = Mat4::from_translation(
                    point - registration.source[end.index()].w_axis.truncate(),
                );
                let mut entries = source.clone();
                DrawEdit::Body(body).apply(&mut entries, &source, layout);
                assert!(
                    entries
                        .iter()
                        .all(|entry| entry.m.iter().all(|value| value.is_finite()))
                );
                for group in [start, end] {
                    let frame = palette::entry_to_matrix(
                        &entries[registration.roots[group.index()]].m,
                        layout,
                    );
                    assert!(
                        frame
                            .transform_point3(registration.bind[end.index()].w_axis.truncate())
                            .abs_diff_eq(point, 1e-5)
                    );
                }
            }
        }
    }

    #[test]
    fn missing_registered_roots_leave_the_upload_unchanged() {
        for layout in [PaletteLayout::Columns, PaletteLayout::Rows] {
            let (edit, source, roll) = registered_arm(layout);
            let mut entries = source[..=roll].to_vec();
            let before = entries.clone();
            edit.apply(&mut entries, &source[..=roll], layout);
            assert!(
                entries
                    .iter()
                    .zip(before)
                    .all(|(after, before)| after.m == before.m)
            );
        }
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
        let source = entries;
        edit.apply(&mut entries, &source, PaletteLayout::Columns);
        let moved =
            palette::entry_to_matrix(&entries[index("RightHandAttach")].m, PaletteLayout::Columns);
        assert!(moved.abs_diff_eq(shift, 1e-6));
        let kept = palette::entry_to_matrix(&entries[index("Spine")].m, PaletteLayout::Columns);
        assert!(kept.abs_diff_eq(Mat4::IDENTITY, 1e-6));
        assert_eq!(edit.neck(), Mat4::IDENTITY);
    }
}
