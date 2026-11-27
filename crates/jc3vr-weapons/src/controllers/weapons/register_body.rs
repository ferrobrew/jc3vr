//! The weapon handling's contributions to the body and the character hooks below it
//! ([`BodyContributions`]), filled in this one place: the held-item source
//! ([`crate::controllers::weapons::held_source`]), the held guns' support and weight targets, which
//! the hand bodies' drive applies ([`gun_bodies::apply`]), the consumer of the deferred magazine
//! detachments at the character's serial post-physics entry, the body mirror's sampler of the
//! attachments on the body, and the sink the physical hands hand a weapon their pickup grab reached
//! to.

use jc3vr_body::{contributions::BodyContributions, controllers::held::pickup::PickupSink};

use crate::controllers::weapons::{
    gun_bodies, held_source,
    hold::{button_pickup, record_pickup, source},
    mirror,
    reload::serial,
};

/// Fill the weapon handling's fields of `body`.
pub fn register(body: &mut BodyContributions) {
    body.held = Some(held_source::source());
    body.gun_targets = Some(gun_bodies::apply);
    body.serial_consumer = Some(serial::consume);
    body.attachment_sampler = Some(mirror::sample_attachments);
    body.pickup_sink = Some(PickupSink {
        take_ground: source::take_ground,
        record: record_pickup,
        grant: button_pickup::grant,
    });
}
