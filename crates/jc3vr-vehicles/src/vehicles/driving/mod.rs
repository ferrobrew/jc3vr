//! The driver's seat: the physical controls, the steering wheel and the handlebars fitted to the
//! vehicle, the driver's view, the rendered wheel, and the driver's hand-aimed weapons.
//!
//! - [`physical`]: the wheel, handlebars, cyclic, collective, flight stick, yoke, and throttle of
//!   the driver's seat and the cockpit's buttons, grabbed with the grip, which write the vehicle's
//!   analog axes and hold the hands;
//! - [`steering`]: how the held wheel turns and maps to the steering;
//! - [`wheel`] and [`fit`]: the wheel and the handlebars placed where the driven vehicle's own are;
//! - [`view`]: the driver's view moved per vehicle to a standard place behind the wheel;
//! - [`wheel_render`]: the vehicle's rendered steering wheel turned with the steering;
//! - [`haptics`]: the haptic cues the physical controls give the hands holding them;
//! - [`aim`]: the driver's vehicle weapons aimed by the dominant hand.

pub mod aim;
pub mod fit;
pub(super) mod haptics;
pub mod physical;
pub mod steering;
pub mod view;
pub mod wheel;
pub mod wheel_render;
