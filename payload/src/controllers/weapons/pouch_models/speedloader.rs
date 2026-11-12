//! The speedloader on the belt pouch a revolver is pushed onto: the game has no model of a
//! revolver's rounds or of a speedloader, so it is drawn as the hand draws a cylinder of rounds,
//! with the casing the gun's ejection effect throws out as a stand-in, one instance per round
//! standing in a ring round the group's axis, their mouths up, where the gun's cylinder comes
//! down onto them ([`crate::controllers::weapons::reload::belt_speedloaders`]).

use glam::{Mat4, Quat, Vec3};

use crate::controllers::{
    body::rig::frames::Frames,
    frame::Hand,
    weapons::{
        pouch_models::{MODEL_PLACEMENT, on_pouch, owned::OwnedModel, render_matrix},
        reload::{BeltSpeedloader, plan::MAX_HELD_ROUNDS},
    },
};

/// Each gun hand's speedloader on its belt pouch, by [`Hand::index`].
#[derive(Debug, Copy, Clone)]
pub(super) struct Speedloaders {
    groups: [Group; 2],
}

impl Speedloaders {
    pub(super) const EMPTY: Speedloaders = Speedloaders {
        groups: [Group::EMPTY; 2],
    };

    /// Show each hand's speedloader in `wanted` on its pouch, carried into the character's model
    /// space by `frames`, and hide the rest. `character` is the character's latest world transform.
    /// Returns how many instances it made.
    ///
    /// # Safety
    ///
    /// The game thread, before the world update; each wanted weapon is a live weapon of the local
    /// player.
    pub(super) unsafe fn input_tick(
        &mut self,
        wanted: [Option<BeltSpeedloader>; 2],
        frames: &Frames,
        character: Mat4,
    ) -> u64 {
        let mut created = 0;
        for (group, wanted) in self.groups.iter_mut().zip(wanted) {
            let local = wanted.and_then(|w| group_local(&w, frames).map(|local| (w, local)));
            created += match local {
                Some((wanted, local)) => unsafe { group.show(&wanted, local, character) },
                None => {
                    unsafe { group.hide() };
                    0
                }
            };
        }
        created
    }

    /// Push each shown speedloader's rounds for the frame at `dtf`, between the character's last
    /// two world transforms `character`. Returns whether any was drawn.
    ///
    /// # Safety
    ///
    /// The game thread, outside the world update.
    pub(super) unsafe fn render_frame(&self, character: [Mat4; 2], dtf: f32) -> bool {
        let mut drew = false;
        for group in &self.groups {
            drew |= unsafe { group.render(character, dtf) };
        }
        drew
    }

    /// How many rounds each hand's speedloader shows, by [`Hand::index`].
    pub(super) fn shown(&self) -> [usize; 2] {
        self.groups.map(|g| if g.visible() { g.count } else { 0 })
    }

    /// Release every instance.
    ///
    /// # Safety
    ///
    /// The game thread, before the world update.
    pub(super) unsafe fn release(&mut self) {
        for group in &mut self.groups {
            unsafe { group.release() };
        }
    }
}

/// The group's frame in the character's model space: on its pouch, turned so the rounds' axis
/// points up the girdle, with the rounds' mouths at the pouch's centre.
fn group_local(wanted: &BeltSpeedloader, frames: &Frames) -> Option<Mat4> {
    if !frames.is_usable() {
        return None;
    }
    let position = frames.cockpit_to_model(wanted.pouch.position);
    let orientation = frames.cockpit_rotation_to_model(wanted.pouch.orientation);
    let stand = Quat::from_rotation_arc(wanted.round.axis, Vec3::Y);
    let local = Mat4::from_rotation_translation(orientation, position)
        * on_pouch(&MODEL_PLACEMENT, wanted.side)
        * Mat4::from_quat(stand)
        * Mat4::from_translation(-wanted.round.top);
    local.is_finite().then_some(local)
}

/// One speedloader: an instance per round and its placement on the last two ticks.
#[derive(Debug, Copy, Clone)]
struct Group {
    rounds: [OwnedModel; MAX_HELD_ROUNDS],
    offsets: [Vec3; MAX_HELD_ROUNDS],
    count: usize,
    /// The group's frame in the character's model space on the last two ticks, older first.
    local: Option<[Mat4; 2]>,
}

impl Group {
    const EMPTY: Group = Group {
        rounds: [OwnedModel::EMPTY; MAX_HELD_ROUNDS],
        offsets: [Vec3::ZERO; MAX_HELD_ROUNDS],
        count: 0,
        local: None,
    };

    fn visible(&self) -> bool {
        self.rounds[0].visible()
    }

    /// Show `wanted`'s rounds at `local`: the first from the game's model cache, the rest from its
    /// reference. Returns how many instances it made; a model not in the cache shows nothing.
    ///
    /// # Safety
    ///
    /// As [`Speedloaders::input_tick`].
    unsafe fn show(&mut self, wanted: &BeltSpeedloader, local: Mat4, character: Mat4) -> u64 {
        let was_visible = self.visible();
        for round in &mut self.rounds {
            unsafe { round.retarget(Some(wanted.weapon)) };
        }
        let world = character * local;
        let Some(first) = (unsafe { self.rounds[0].show_cached(wanted.round.name_hash, world) })
        else {
            unsafe { self.hide() };
            return 0;
        };
        let Some(model) = self.rounds[0].kept_for(Some(wanted.weapon)) else {
            unsafe { self.hide() };
            return 0;
        };
        let count = wanted.round.count.min(MAX_HELD_ROUNDS);
        let mut created = u64::from(first);
        for (i, round) in self.rounds.iter_mut().enumerate().skip(1) {
            if i < count {
                created += u64::from(unsafe { round.show(model, world) } == Some(true));
            } else {
                unsafe { round.hide() };
            }
        }
        self.offsets = [Vec3::ZERO; MAX_HELD_ROUNDS];
        for (offset, place) in self.offsets.iter_mut().zip(wanted.round.offsets()) {
            *offset = place;
        }
        self.count = count;
        // A group shown afresh starts at its new place rather than sweeping from its last.
        self.local = Some(match self.local.filter(|_| was_visible && created == 0) {
            Some([_, last]) => [last, local],
            None => [local, local],
        });
        created
    }

    /// # Safety
    ///
    /// As [`Speedloaders::render_frame`].
    unsafe fn render(&self, character: [Mat4; 2], dtf: f32) -> bool {
        let Some(local) = self.local.filter(|_| self.visible()) else {
            return false;
        };
        let Some(matrix) = render_matrix(character, local, dtf) else {
            return false;
        };
        let mut drew = false;
        for (round, offset) in self.rounds.iter().zip(self.offsets).take(self.count) {
            // SAFETY: the game thread, outside the world update.
            drew |= unsafe { round.push(matrix * Mat4::from_translation(offset)) };
        }
        drew
    }

    /// # Safety
    ///
    /// The game thread, before the world update.
    unsafe fn hide(&mut self) {
        for round in &mut self.rounds {
            unsafe { round.hide() };
        }
        self.local = None;
    }

    /// # Safety
    ///
    /// The game thread, before the world update.
    unsafe fn release(&mut self) {
        for round in &mut self.rounds {
            unsafe { round.release() };
        }
        *self = Group::EMPTY;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::controllers::{cockpit::CockpitTransform, frame::Pose, weapons::reload::plan};

    #[test]
    fn the_rounds_stand_mouth_up_on_the_pouch() {
        let round = plan::RoundModel {
            name_hash: 1,
            top: Vec3::new(0.0, 0.0, -0.016),
            axis: Vec3::NEG_Z,
            radius_m: 0.0065,
            count: 6,
            ring_m: 0.013,
        };
        let pouch = Pose {
            position: Vec3::new(0.1, 1.0, -0.2),
            orientation: Quat::IDENTITY,
        };
        let frames = Frames {
            cockpit: CockpitTransform {
                head_world: Vec3::new(20.0, 4.0, -8.0),
                head_cockpit: Vec3::Y * 1.7,
                body: Quat::from_rotation_y(0.8),
                world_scale: 1.0,
            },
            character: Mat4::from_rotation_translation(
                Quat::from_rotation_y(-0.3),
                Vec3::new(19.0, 2.0, -7.0),
            ),
        };
        let wanted = BeltSpeedloader {
            pouch,
            side: Hand::Right,
            weapon: 7,
            round,
        };
        let local = group_local(&wanted, &frames).expect("a usable frame places the group");
        // The rounds' mouths sit at the pouch's centre and their axis points up.
        let mouth = local.transform_point3(round.top);
        assert!(mouth.distance(frames.cockpit_to_model(pouch.position)) < 1e-4);
        let up = local.transform_vector3(round.axis).normalize();
        assert!(up.distance(frames.cockpit_rotation_to_model(Quat::IDENTITY) * Vec3::Y) < 1e-4);
    }
}
