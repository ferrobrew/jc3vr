//! The choice between the plain exit and the parachute bail for a vehicle seat's long exit press
//! ([`crate::controllers::bindings::tables::declare`]'s vehicle table).
//!
//! The choice is made once per press, on the tick the press becomes long, and kept until the press
//! ends: the plain exit is a one-tick tap of the gamepad's Y at that moment and the bail a hold of
//! `OPEN_PARACHUTE` from then on, so a choice that changed mid-press would drop the held action
//! without the tap (neither exit nor bail), or hold the bail after the tap (both). Between presses
//! the vehicle's speed is read through a hysteresis band that lies wholly above the game's bail
//! speed, so a vehicle hovering near that speed neither flickers between the two nor bails where
//! the game would refuse it.

use crate::controllers::mode::{VehicleClass, VehicleSeat};

/// The vehicle speed, metres per second, from which the game's driver, rider, and spinning mounted
/// gun states take a long `OPEN_PARACHUTE` hold as the parachute bail: the minimum of the animation
/// graph's `C_VEHICLE_IS_FAST_ENOUGH_FOR_PARA_TRANSITION` condition (a
/// `CGraphParentVehicleSpeedCondition`), which gates the `INPUTHOLD` of `OPEN_PARACHUTE` into
/// `ACT_TRIGGER_PARACHUTE` and on to `S_EXIT_VEHICLE_PARA`. The condition's maximum is a hundred
/// thousand, beyond any vehicle, so only the minimum matters. The values are the graph asset's,
/// not code's, so no binding holds them. The condition compares the squared length of the
/// vehicle's ground-relative velocity at the seated character
/// ([`crate::controllers::vehicles::seat_vehicle_speed_mps`]) against its squared bounds; the unit
/// is unverified, and taken to be metres per second.
pub const PARACHUTE_BAIL_MIN_SPEED_MPS: f32 = 6.0;

/// The speed above [`PARACHUTE_BAIL_MIN_SPEED_MPS`] at which the vehicle starts to count as fast
/// enough to bail. The game checks the speed only once its own hold time has passed after the
/// press became long, so the margin leaves room to slow a little in between.
pub const BAIL_ENTER_MARGIN_MPS: f32 = 1.0;

/// The speed above [`PARACHUTE_BAIL_MIN_SPEED_MPS`] below which a fast vehicle stops counting as
/// fast enough to bail. Smaller than [`BAIL_ENTER_MARGIN_MPS`], for the hysteresis, and not below
/// the game's minimum, where the bail would be refused.
pub const BAIL_LEAVE_MARGIN_MPS: f32 = 0.5;

/// Whether the game's states for `seat` bail out by parachute at speed: the car's driver, the
/// motorcycle's rider, and a spinning mounted gun's gunner (`mounted_gun`). A passenger seat, the
/// boats, the aircraft, and the mech have no bail.
pub const fn bails_by_parachute(seat: VehicleSeat, mounted_gun: bool) -> bool {
    match seat.class {
        VehicleClass::Car | VehicleClass::Motorcycle | VehicleClass::Unknown => {
            seat.driver || mounted_gun
        }
        VehicleClass::Sea | VehicleClass::Helicopter | VehicleClass::Plane | VehicleClass::Mech => {
            false
        }
    }
}

/// The per-press choice of the bail over the plain exit.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Default)]
pub struct BailLatch {
    /// Whether the vehicle counts as fast enough to bail, through the hysteresis band.
    fast: bool,
    /// The choice handed out on the last tick.
    chosen: bool,
}

/// What [`BailLatch::step`] reads each tick in a vehicle seat.
#[derive(Debug, Copy, Clone, PartialEq, Default)]
pub struct BailTick {
    /// Whether the seat's states have the bail ([`bails_by_parachute`]).
    pub seat_bails: bool,
    /// The vehicle's speed, metres per second, while it can be read.
    pub speed_mps: Option<f32>,
    /// Whether the exit press had already become long by the last tick.
    pub press_long: bool,
}

impl BailLatch {
    pub const fn new() -> Self {
        Self {
            fast: false,
            chosen: false,
        }
    }

    /// Advance by one tick in a vehicle seat, returning whether a long exit press bails out by
    /// parachute this tick. While the press that became long goes on, the choice made on the tick
    /// it became long stands.
    pub fn step(&mut self, tick: BailTick) -> bool {
        let threshold = PARACHUTE_BAIL_MIN_SPEED_MPS
            + if self.fast {
                BAIL_LEAVE_MARGIN_MPS
            } else {
                BAIL_ENTER_MARGIN_MPS
            };
        self.fast = tick
            .speed_mps
            .is_some_and(|speed| speed.is_finite() && speed >= threshold);
        if !tick.press_long {
            self.chosen = tick.seat_bails && self.fast;
        }
        self.chosen
    }

    /// Forget the speed and the choice: out of a vehicle seat.
    pub fn reset(&mut self) {
        *self = Self::new();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tick(speed: f32, press_long: bool) -> BailTick {
        BailTick {
            seat_bails: true,
            speed_mps: Some(speed),
            press_long,
        }
    }

    #[test]
    fn the_speed_counts_through_a_band_above_the_games_minimum() {
        let enter = PARACHUTE_BAIL_MIN_SPEED_MPS + BAIL_ENTER_MARGIN_MPS;
        let leave = PARACHUTE_BAIL_MIN_SPEED_MPS + BAIL_LEAVE_MARGIN_MPS;
        assert!(leave >= PARACHUTE_BAIL_MIN_SPEED_MPS);
        let mut latch = BailLatch::new();
        assert!(!latch.step(tick(enter - 0.01, false)));
        assert!(latch.step(tick(enter, false)));
        // Inside the band a fast vehicle stays fast, and a slow one stays slow.
        assert!(latch.step(tick(leave + 0.01, false)));
        assert!(!latch.step(tick(leave - 0.01, false)));
        assert!(!latch.step(tick(leave + 0.01, false)));
    }

    #[test]
    fn the_choice_stands_until_the_long_press_ends() {
        let fast = PARACHUTE_BAIL_MIN_SPEED_MPS * 2.0;
        let slow = 0.0;
        let mut latch = BailLatch::new();
        assert!(latch.step(tick(fast, false)));
        // Braking to a stop through the press keeps the bail it became long with.
        assert!(latch.step(tick(slow, true)));
        assert!(latch.step(tick(slow, true)));
        // Let go: the next press reads the speed afresh.
        assert!(!latch.step(tick(slow, false)));
        // Speeding up through a press that became long slow keeps the plain exit.
        assert!(!latch.step(tick(fast, true)));
        assert!(latch.step(tick(fast, false)));
    }

    #[test]
    fn no_bail_where_the_seat_has_none_or_the_speed_is_unknown() {
        let fast = PARACHUTE_BAIL_MIN_SPEED_MPS * 2.0;
        let mut latch = BailLatch::new();
        assert!(!latch.step(BailTick {
            seat_bails: false,
            ..tick(fast, false)
        }));
        assert!(!latch.step(BailTick {
            speed_mps: None,
            ..tick(fast, false)
        }));
        assert!(!latch.step(tick(f32::NAN, false)));
    }
}
