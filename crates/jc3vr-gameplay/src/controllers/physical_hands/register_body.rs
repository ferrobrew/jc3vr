//! Registers the physical hands with the body, the grip model, the weapon handling, and the
//! character hooks below them: the climb's readers ([`crate::controllers::facts::climb`]), the
//! palms' push in the rig's simulation half, the climb's refusal of the automatic entry into a
//! vehicle's stunt area below the player, the observer of a body destruction, the palm summary
//! logged with each grip press, the grip view's flash for a refused take of a weapon, and the props
//! the hands carry, which the loadout table records.

use jc3gi::physics::hknpWorld;
use jc3vr_input::frame::Hand;

use crate::{
    controllers::{
        body::rig::root::register_palm_push,
        facts::climb::{self as climb_facts, ClimbReaders},
        grab::tick::register_palm_summary,
        physical_hands::{
            carry_bodies_destroying, climb, grab_view, held_props, press_summary, push_body,
        },
        weapons::{hold::take::register_refusal_sink, inventory::register_held_props},
    },
    hooks::character::{
        AutoAttachBlocker, register_auto_attach_blocker, register_bodies_destroying,
    },
};

pub(crate) fn register() {
    climb_facts::register(ClimbReaders {
        climbing: climb::climbing,
        body_frame: climb::engine::body_frame,
        body_identity: climb::engine::body_identity,
    });
    register_palm_push(push_body);
    register_auto_attach_blocker(AutoAttachBlocker::PhysicalClimb, climb::native::blocks);
    register_bodies_destroying(bodies_destroying);
    register_palm_summary(press_summary);
    register_refusal_sink(take_refused);
    register_held_props(held_props);
}

/// A refused take of a weapon, flashed in the grip view at the target the hand's grip chose.
fn take_refused(hand: Hand, why: &str) {
    grab_view::refused(hand, None, why);
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
