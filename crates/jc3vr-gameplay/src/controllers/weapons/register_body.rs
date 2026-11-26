//! Registers the weapon handling with the body and the character hooks below it: the consumer of
//! the deferred magazine detachments at the character's serial post-physics entry, and the body
//! mirror's sampler of the attachments on the body.

use jc3vr_body::{
    controllers::body::mirror::register_attachment_sampler,
    hooks::character::register_serial_consumer,
};

use crate::controllers::weapons::{mirror, reload::serial};

pub(crate) fn register() {
    register_serial_consumer(serial::consume);
    register_attachment_sampler(mirror::sample_attachments);
}
