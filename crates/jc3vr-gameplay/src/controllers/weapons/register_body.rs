//! Registers the weapon handling with the body and the character hooks below it: the consumer of
//! the deferred magazine detachments at the character's serial post-physics entry, the body
//! mirror's sampler of the attachments on the body, and the sink the physical hands hand a weapon
//! their pickup grab reached to.

use jc3vr_body::{
    controllers::{
        body::mirror::register_attachment_sampler,
        held::pickup::{PickupSink, register_pickup_sink},
    },
    hooks::character::register_serial_consumer,
};

use crate::controllers::weapons::{
    hold::{button_pickup, record_pickup, source},
    mirror,
    reload::serial,
};

pub(crate) fn register() {
    register_serial_consumer(serial::consume);
    register_attachment_sampler(mirror::sample_attachments);
    register_pickup_sink(PickupSink {
        take_ground: source::take_ground,
        record: record_pickup,
        grant: button_pickup::grant,
    });
}
