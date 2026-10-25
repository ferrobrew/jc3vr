//! Current body reads and writes, with committed-generation observations before further access.

use glam::{Quat, Vec3};
use jc3gi::{game::GameObject, physics::PhysicsGameObject};

use crate::controllers::{
    physical_hands::{self, BodyError},
    weapons::reload::{
        config::LooseMagazineConfig,
        loose::{Loose, Operation, State, body_status, carry_velocities, upright_toward},
    },
};

pub(super) trait Bodies {
    fn pose(&mut self, magazine: Loose) -> Result<(Vec3, Quat), BodyError>;
    fn velocities(&mut self, magazine: Loose, linear: Vec3, angular: Vec3)
    -> Result<(), BodyError>;
    fn disable(&mut self, magazine: Loose);
}

pub(super) struct NativeBodies;

impl Bodies for NativeBodies {
    fn pose(&mut self, magazine: Loose) -> Result<(Vec3, Quat), BodyError> {
        unsafe { physical_hands::body_pose(magazine.body, magazine.owner) }
    }
    fn velocities(
        &mut self,
        magazine: Loose,
        linear: Vec3,
        angular: Vec3,
    ) -> Result<(), BodyError> {
        unsafe {
            physical_hands::set_body_velocities(magazine.body, magazine.owner, linear, angular)
        }
    }
    fn disable(&mut self, magazine: Loose) {
        let object = (magazine.object.get() + PhysicsGameObject::GAME_OBJECT_BASE_OFFSET as usize)
            as *mut GameObject;
        unsafe { (*object).Enable(false) };
    }
}

pub(super) struct Carry<'a> {
    pub top: Vec3,
    pub axis: Vec3,
    pub dt: f32,
    pub config: &'a LooseMagazineConfig,
}

impl State {
    /// Failed observations cannot restore a quarantined generation or affect its replacement.
    pub(super) fn observe<T>(
        &mut self,
        magazine: Loose,
        result: Result<T, BodyError>,
    ) -> Operation<T> {
        let status = body_status(result.as_ref().map(|_| ()).map_err(|error| *error));
        match self.records.observe(magazine, status) {
            Operation::Ready(()) => match result {
                Ok(value) => Operation::Ready(value),
                Err(_) => Operation::Deferred,
            },
            Operation::Deferred => Operation::Deferred,
            Operation::Gone => Operation::Gone,
        }
    }

    fn held_access(&mut self) -> Operation<Loose> {
        let Some(held) = self.held else {
            return Operation::Gone;
        };
        match self.records.observe(held.magazine, body_status(Ok(()))) {
            Operation::Ready(()) => Operation::Ready(held.magazine),
            Operation::Deferred => Operation::Deferred,
            Operation::Gone => {
                self.held = None;
                Operation::Gone
            }
        }
    }

    pub(super) fn carry_with(
        &mut self,
        carry: Carry<'_>,
        bodies: &mut impl Bodies,
    ) -> Operation<()> {
        let magazine = match self.held_access() {
            Operation::Ready(magazine) => magazine,
            Operation::Deferred => return Operation::Deferred,
            Operation::Gone => return Operation::Gone,
        };
        let result = bodies.pose(magazine);
        let (centre, orientation) = match self.observe(magazine, result) {
            Operation::Ready(pose) => pose,
            Operation::Deferred => return Operation::Deferred,
            Operation::Gone => return Operation::Gone,
        };
        let target = carry.top - carry.axis.normalize_or_zero() * carry.config.half_length_m;
        let (linear, angular) = carry_velocities(
            (centre, orientation),
            (target, upright_toward(orientation, carry.axis)),
            carry.dt,
            carry.config.max_carry_mps,
        );
        let result = bodies.velocities(magazine, linear, angular);
        self.observe(magazine, result)
    }

    /// Return ownership only after a current pose and exact-generation record removal succeed.
    pub(super) fn seat_with(&mut self, bodies: &mut impl Bodies) -> Operation<Loose> {
        let magazine = match self.held_access() {
            Operation::Ready(magazine) => magazine,
            Operation::Deferred => return Operation::Deferred,
            Operation::Gone => return Operation::Gone,
        };
        let result = bodies.pose(magazine);
        match self.observe(magazine, result) {
            Operation::Ready(_) => {}
            Operation::Deferred => return Operation::Deferred,
            Operation::Gone => return Operation::Gone,
        }
        if !self.records.remove(magazine) {
            return Operation::Deferred;
        }
        self.held = None;
        self.counts.in_world = self.records.magazines().count();
        Operation::Ready(magazine)
    }
}

#[cfg(test)]
mod tests;
