//! The driver's controls and the cockpit's buttons as the grip model names them: the kinds of
//! physical control a seat offers and the buttons a palm presses. The vehicle handling places and
//! works them (`vehicles::physical`). Pure.

use crate::controllers::mode::VehicleClass;

/// A physical control.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum ControlKind {
    Wheel,
    Handlebars,
    Cyclic,
    Collective,
    FlightStick,
    /// A plane's control wheel, in place of the flight stick where the cockpit models one.
    Yoke,
    Throttle,
}

impl ControlKind {
    pub const fn label(self) -> &'static str {
        match self {
            ControlKind::Wheel => "wheel",
            ControlKind::Handlebars => "handlebars",
            ControlKind::Cyclic => "cyclic",
            ControlKind::Collective => "collective",
            ControlKind::FlightStick => "flight stick",
            ControlKind::Yoke => "yoke",
            ControlKind::Throttle => "throttle",
        }
    }

    /// The controls a vehicle of `class` offers its driver. Cars and boats share their classes with
    /// the ATV, the jetski, and the duck, which steer by handlebars: a vehicle with a steering fork
    /// (`fork`) gets the handlebars in place of the wheel. A plane whose cockpit has a control wheel
    /// (`yoke`) gets the yoke in place of the flight stick.
    pub const fn for_seat(class: VehicleClass, fork: bool, yoke: bool) -> &'static [ControlKind] {
        match class {
            VehicleClass::Car | VehicleClass::Sea | VehicleClass::Unknown if fork => {
                &[ControlKind::Handlebars]
            }
            VehicleClass::Car | VehicleClass::Sea | VehicleClass::Unknown => &[ControlKind::Wheel],
            VehicleClass::Motorcycle => &[ControlKind::Handlebars],
            VehicleClass::Helicopter => &[ControlKind::Cyclic, ControlKind::Collective],
            VehicleClass::Plane if yoke => &[ControlKind::Yoke, ControlKind::Throttle],
            VehicleClass::Plane => &[ControlKind::FlightStick, ControlKind::Throttle],
            // The mech walks; it has no controls to hold.
            VehicleClass::Mech => &[],
        }
    }

    /// Whether both hands may hold the control at once.
    pub const fn two_handed(self) -> bool {
        matches!(
            self,
            ControlKind::Wheel | ControlKind::Handlebars | ControlKind::Yoke
        )
    }
}

/// A cockpit button.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum Button {
    /// The horn, on the wheel's hub.
    Horn,
    /// The nitrous button on the dash beside the wheel.
    Nitrous,
}

impl Button {
    pub const ALL: [Button; 2] = [Button::Horn, Button::Nitrous];

    pub const fn label(self) -> &'static str {
        match self {
            Button::Horn => "horn",
            Button::Nitrous => "nitrous button",
        }
    }

    pub const fn index(self) -> usize {
        match self {
            Button::Horn => 0,
            Button::Nitrous => 1,
        }
    }
}
