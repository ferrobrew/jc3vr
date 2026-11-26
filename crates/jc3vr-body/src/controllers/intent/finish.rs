//! What the bindings hand the second half of each subsystem's input tick: the weapon handling's,
//! the pickups', and the mounted guns'.

use crate::controllers::{grab::GrabVolume, intent::controls::ButtonControl};

/// What the bindings hand the second half of a subsystem's input tick.
pub struct FinishInputs {
    /// The grab volume each hand grabbed this tick, indexed by
    /// [`Hand::index`](jc3vr_input::frame::Hand::index).
    pub grabs: [Option<GrabVolume>; 2],
    /// Each hand's grip over the tick, as a hold reads it
    /// ([`HandControls::hold`](crate::controllers::intent::controls::HandControls::hold)).
    pub grips: [ButtonControl; 2],
    /// Each hand's trigger press over the tick.
    pub triggers: [ButtonControl; 2],
    /// Each hand's fire press for the gun it holds, over the tick: its trigger where the triggers
    /// fire the hand weapons, and in a vehicle its vehicle weapon action (the bindings' gun fire).
    pub fire_presses: [ButtonControl; 2],
    /// The input owner's per-hand recovery mask, before filtered controls can appear neutral.
    pub unavailable: [bool; 2],
    /// The reload action's press over the tick.
    pub reload_button: ButtonControl,
    /// The engine's tick delta, game seconds.
    pub dt: f32,
    /// The tick's length, real seconds: what times the player's body.
    pub real_dt: f32,
}
