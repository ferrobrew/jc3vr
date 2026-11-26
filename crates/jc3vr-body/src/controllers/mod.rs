//! The body-bound controller subsystems, built on the input crate's controller snapshot
//! ([`ControllerFrame`](jc3vr_input::frame::ControllerFrame)) that the view crate's
//! [`jc3vr_view::vr::input`] publishes once per rendered frame, following the plan of record in
//! `docs/mod/input/controllers-and-roomscale.md`:
//!
//! - [`scheme`]: controller versus gamepad scheme selection, the master switch, and the acting
//!   hand;
//! - [`mode`]: the per-tick mode detector, which selects the OpenXR action set;
//! - [`clock`]: the game's clock, which its slow motion slows, against the player's real one;
//! - [`grab`]: the grip model: what each hand's grip would take among the targets every subsystem
//!   provides, when it takes it, and the markers that show it;
//! - [`body`]: the player's body dimensions, their calibration in the headset, and the remap onto
//!   the character's proportions;
//! - [`gamepad_setting`]: the game's active-input-device setting held at the gamepad under the
//!   controller scheme;
//! - [`aim_state`]: the aim-state pin, the mental-state cuts, and skipping the player's aim IK;
//! - [`hands`]: controller-owned hands: the wrist IK, the hand tiers, and the render-time late
//!   update;
//! - [`presentation`]: the render-rate presentation layer: the upper body re-posed every rendered
//!   frame from the latest tracking, on top of the pose the tick wrote;
//! - [`haptics`]: the controllers' vibration: one vocabulary of cues for every source, which the
//!   input crate's mixer folds into what each hand plays, and the game's rumble forwarded;
//! - [`fingers`]: finger curls on the rendered hands, from hand tracking or estimated from the
//!   inputs;
//! - [`aim`]: per-hand aiming: the guns along the hands' sight lines and the grapple along the
//!   off-hand's ray;
//! - [`overlay`]: the world-space overlay layer: sprites and lines drawn into the scene per eye;
//! - [`spawn`]: the spawn system's requests and the shipped spawn definition table, for the
//!   inventory's gives and the director's spawner;
//! - [`locomotion`] and [`roomscale`]: stick locomotion, and posture, the head-height remap, the
//!   cockpit composition, the crouch capsule, the head-through-geometry fade, and seat transitions.
//!   Roomscale needs the headset rather than the controllers, so it is gated on the VR source and
//!   its own toggles instead of the scheme;
//! - [`telemetry`]: the notes the subsystems write to the tracking recording;
//! - [`intent`], [`held`], [`physics`], and [`facts`]: the vocabulary, the held-item facts, the
//!   physics substrate, and the published facts that the subsystems above share with these.
//!
//! The weapon handling, the physical hands, the vehicles, the grapple, the bindings, and the
//! summary live in the gameplay crate above, which registers what they contribute here and
//! composes these subsystems' hook libraries with its own.

pub mod aim;
pub mod aim_state;
pub mod body;
pub mod clock;
pub mod facts;
pub mod fingers;
pub mod gamepad_setting;
pub mod grab;
pub mod hands;
pub mod haptics;
pub mod held;
pub mod intent;
pub mod locomotion;
pub mod mode;
pub mod overlay;
pub mod physics;
pub mod presentation;
pub mod roomscale;
pub mod scheme;
pub mod spawn;
pub mod telemetry;
