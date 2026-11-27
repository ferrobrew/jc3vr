//! The weapon handling's contributions to the body and the character hooks below it
//! ([`BodyContributions`]): the consumer of the deferred magazine detachments at the character's
//! serial post-physics entry, the body mirror's sampler of the attachments on the body, and the
//! sink the physical hands hand a weapon their pickup grab reached to.

use jc3vr_body::{contributions::BodyContributions, controllers::held::pickup::PickupSink};

use crate::controllers::weapons::{
    hold::{button_pickup, record_pickup, source},
    mirror,
    reload::serial,
};

/// Fill the weapon handling's fields of `body`, beside the held-item source
/// ([`crate::controllers::weapons::register_held`]).
pub fn register(body: &mut BodyContributions) {
    body.serial_consumer = Some(serial::consume);
    body.attachment_sampler = Some(mirror::sample_attachments);
    body.pickup_sink = Some(PickupSink {
        take_ground: source::take_ground,
        record: record_pickup,
        grant: button_pickup::grant,
    });
}
