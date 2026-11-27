//! The physical hands' contributions to the body, the grip model, and the character hooks below
//! them ([`BodyContributions`]): the climb's readers ([`jc3vr_body::controllers::facts::climb`]),
//! the palms' push in the rig's simulation half, the observer of a body destruction, and the palm
//! summary logged with each grip press. The climb's refusal of the automatic entry into a vehicle's
//! stunt area below the player ([`blocks_auto_attach`]) rides in the gameplay crate's refusal
//! table, which declares the refusals' order.

use jc3gi::{character::character::Character, physics::hknpWorld};
use jc3vr_body::{contributions::BodyContributions, controllers::facts::climb::ClimbReaders};

use crate::controllers::physical_hands::{
    carry_bodies_destroying, climb, press_summary, push_body,
};

/// Fill the physical hands' fields of `body`.
pub fn register(body: &mut BodyContributions) {
    body.climb = Some(ClimbReaders {
        climbing: climb::climbing,
        body_frame: climb::engine::body_frame,
        body_identity: climb::engine::body_identity,
    });
    body.palm_push = Some(push_body);
    body.bodies_destroying = Some(bodies_destroying);
    body.palm_summary = Some(press_summary);
}

/// Whether a physical handhold the player climbs by refuses the automatic entry of `character`
/// into a vehicle's stunt area below him.
///
/// # Safety
///
/// The native caller keeps `character` and its proxy member alive for this call; only members of
/// that character are read.
pub unsafe fn blocks_auto_attach(character: *const Character) -> bool {
    unsafe { climb::native::blocks(character) }
}

/// Before the world destroys bodies: the climb forgets each body's lifetime stamp, then the carried
/// bodies' collision lets go of them.
///
/// # Safety
///
/// Called from the world's destruction call with its arguments: `ids` points at `count` ids, or is
/// null or `count` is not positive.
unsafe fn bodies_destroying(world: *mut hknpWorld, ids: *const u32, count: i32) {
    if !ids.is_null() && count > 0 {
        for i in 0..count as usize {
            let body = unsafe { *ids.add(i) };
            climb::lifetime::destroyed(world as usize, body);
        }
    }
    unsafe { carry_bodies_destroying(world, ids, count) };
}
