//! Per-hand cancellation before filtered controls reach physical throw consumers.

use jc3vr_weapons::controllers::weapons::{c4, grenade};

pub(super) fn cancel_throws(unavailable: [bool; 2]) {
    grenade::input_unavailable(unavailable);
    c4::input_unavailable(unavailable);
}
