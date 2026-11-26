//! What the weapon handling publishes for the HUD to draw: each hand's gun for the ammunition
//! counter ([`hand_ammo`]), each hand's grab hint ([`grab_hint`]), and the weapons lying in the
//! world ([`lost_weapons`]), with the weapon icons they name ([`icon`]).
//!
//! The input tick writes each sample on the game thread, and the HUD reads the latest one when it
//! draws. The HUD owns the drawing and everything it reads from the renderer; these modules hold
//! only the data and its slot.

pub mod grab_hint;
pub mod hand_ammo;
pub mod icon;
pub mod lost_weapons;
