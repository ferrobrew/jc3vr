//! The cockpit's buttons, worked by hand: the horn on the wheel's hub and a nitrous button on the
//! dash beside the wheel, pressed by pushing a free hand into them, and the collective's twist
//! grip, twisted to burn the nitrous. Pure.
//!
//! A push button is a disc facing the driver. A hand arms it by coming to it from the front, within
//! its radius, and presses it by pushing on past its face by the press depth; it stays pressed,
//! with some give, until the hand comes back out or slides off. A hand that reaches the disc from
//! behind or from the side, as one passing through the dash or sweeping across the wheel does,
//! presses nothing.
//!
//! The vehicle's own inputs are held while a button is pressed: `SOUND_HORN_SIREN` for the horn
//! and `USE_VEHICLE_MOD` for the nitrous, which the game reads as nitrous while held and as the
//! turbo jump when tapped, as it does the gamepad's button. On the handlebars the triggers are the
//! throttle and the brake.

use body_rig::math::rotation::twist_angle;
use glam::{Quat, Vec3};
use jc3vr_body::controllers::grab::cockpit::{Button, ControlKind};
use jc3vr_input::frame::Hand;

use crate::controllers::vehicles::{
    physical::Hold, tuning::CockpitButtonsTuning, wheel::WheelFrame,
};

/// A push button in the cockpit frame: a disc facing the driver.
#[derive(Debug, Copy, Clone, PartialEq)]
pub(crate) struct PushButton {
    /// The middle of its face.
    pub centre: Vec3,
    /// The face's normal, toward the driver, a unit vector.
    pub normal: Vec3,
    pub radius: f32,
    /// How far past the face a hand pushes to press it, metres.
    pub depth: f32,
}

impl PushButton {
    /// The horn on `wheel`'s hub.
    pub fn horn(wheel: &WheelFrame, config: &CockpitButtonsTuning) -> Self {
        Self {
            centre: wheel.centre,
            normal: wheel.normal(),
            radius: wheel.radius * config.hub_share.clamp(0.1, 0.6),
            depth: config.press_depth_m,
        }
    }

    /// The nitrous button on the dash, beside `wheel` to its right and a little up, facing the
    /// driver as the wheel does.
    pub fn nitrous(wheel: &WheelFrame, config: &CockpitButtonsTuning) -> Self {
        let from = config.nitrous_from_rim_m;
        Self {
            centre: wheel.centre
                + wheel.right * (wheel.radius + from.x)
                + wheel.up * from.y
                + wheel.normal() * from.z,
            normal: wheel.normal(),
            radius: config.nitrous_radius_m,
            depth: config.press_depth_m,
        }
    }
}

/// Where a hand is with a push button.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Default)]
pub enum PushPhase {
    /// Away from it, or reached from behind or the side.
    #[default]
    Out,
    /// In front of it within its radius: a push from here presses it.
    Armed,
    Pressed,
}

/// `phase` for a hand whose grip is now at `point`, `None` while untracked.
pub(crate) fn push_step(
    phase: PushPhase,
    button: &PushButton,
    point: Option<Vec3>,
    config: &CockpitButtonsTuning,
) -> PushPhase {
    let Some(point) = point.filter(|p| p.is_finite()) else {
        return PushPhase::Out;
    };
    let d = point - button.centre;
    let along = d.dot(button.normal);
    let lateral = d.reject_from_normalized(button.normal).length();
    let depth = button.depth.max(0.005);
    let in_front = (0.0..=config.arm_reach_m).contains(&along) && lateral <= button.radius;
    let pushed = along <= -depth && along >= -config.max_push_m && lateral <= button.radius;
    let held = along <= -depth * config.release_depth
        && along >= -config.max_push_m
        && lateral <= button.radius * config.held_radius;
    match phase {
        PushPhase::Armed if pushed => PushPhase::Pressed,
        PushPhase::Pressed if held => PushPhase::Pressed,
        _ if in_front => PushPhase::Armed,
        _ => PushPhase::Out,
    }
}

/// The twist grips' place: the collective.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum TwistGrip {
    /// The collective's grip, twisted either way for the nitrous.
    Collective,
}

impl TwistGrip {
    pub const fn label(self) -> &'static str {
        match self {
            TwistGrip::Collective => "collective's twist grip",
        }
    }
}

/// How far a hand that took the collective at orientation `grab` and holds it at `now` has twisted
/// it, radians, about the fore-aft axis, either way.
pub(crate) fn grip_twist(grab: Quat, now: Quat) -> f32 {
    twist_angle(now * grab.inverse(), Vec3::Z).abs()
}

/// Whether a twist grip at `twist` radians burns the nitrous, given whether it did on the last
/// tick: past the twist to engage, and until it comes back by a margin.
pub(crate) fn twist_engaged(twist: f32, was: bool, config: &CockpitButtonsTuning) -> bool {
    let engage = config.twist_deg.to_radians();
    let release = (config.twist_deg - config.twist_hysteresis_deg)
        .max(0.0)
        .to_radians();
    if was {
        twist >= release
    } else {
        twist >= engage
    }
}

/// The buttons' state: each hand's phase with each push button, and the twist grip each hand works:
/// the collective's burning the nitrous, or a bars' grip open.
#[derive(Debug, Copy, Clone, PartialEq, Default)]
pub struct ButtonState {
    /// Indexed by [`Button`], then by [`Hand::index`].
    pub phases: [[PushPhase; 2]; 2],
    /// The twist grip each hand works: the collective's past its nitrous twist, or a bars' grip
    /// open.
    pub twists: [Option<TwistGrip>; 2],
}

impl ButtonState {
    pub const fn new() -> Self {
        Self {
            phases: [[PushPhase::Out; 2]; 2],
            twists: [None; 2],
        }
    }

    pub fn pressed(&self, button: Button) -> bool {
        self.phases[button.index()].contains(&PushPhase::Pressed)
    }

    pub fn phase(&self, button: Button, hand: Hand) -> PushPhase {
        self.phases[button.index()][hand.index()]
    }
}

/// A press or a release of a button or a twist grip, for the haptics and the log.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum ButtonEvent {
    Pressed(Hand, Button),
    Released(Hand, Button),
    Twisted(Hand, TwistGrip),
    Untwisted(Hand, TwistGrip),
}

impl ButtonEvent {
    pub const fn hand(self) -> Hand {
        match self {
            ButtonEvent::Pressed(hand, _)
            | ButtonEvent::Released(hand, _)
            | ButtonEvent::Twisted(hand, _)
            | ButtonEvent::Untwisted(hand, _) => hand,
        }
    }

    /// Whether the event starts a press or a twist.
    pub const fn engages(self) -> bool {
        matches!(self, ButtonEvent::Pressed(..) | ButtonEvent::Twisted(..))
    }

    /// What was worked: the button's or the twist grip's name.
    pub const fn what(self) -> &'static str {
        match self {
            ButtonEvent::Pressed(_, button) | ButtonEvent::Released(_, button) => button.label(),
            ButtonEvent::Twisted(_, grip) | ButtonEvent::Untwisted(_, grip) => grip.label(),
        }
    }
}

/// The buttons a seat has.
#[derive(Debug, Copy, Clone, PartialEq, Default)]
pub(crate) struct ButtonPlacement {
    pub horn: Option<PushButton>,
    pub nitrous: Option<PushButton>,
    pub(crate) collective_twist: bool,
}

impl ButtonPlacement {
    /// The buttons a seat offering `kinds` has with `config`: the horn with the wheel, the nitrous
    /// button with the wheel of a vehicle with nitrous (`nitrous`), and the collective's twist with
    /// the collective.
    pub fn for_seat(
        kinds: &[ControlKind],
        config: &CockpitButtonsTuning,
        wheel: &WheelFrame,
        nitrous: bool,
    ) -> Self {
        let wheel_seat = kinds.contains(&ControlKind::Wheel);
        Self {
            horn: wheel_seat.then(|| PushButton::horn(wheel, config)),
            nitrous: (wheel_seat && nitrous).then(|| PushButton::nitrous(wheel, config)),
            collective_twist: kinds.contains(&ControlKind::Collective),
        }
    }

    pub fn button(&self, button: Button) -> Option<PushButton> {
        match button {
            Button::Horn => self.horn,
            Button::Nitrous => self.nitrous,
        }
    }
}

/// What the buttons ask of the vehicle this tick.
#[derive(Debug, Copy, Clone, PartialEq, Default)]
pub(crate) struct ButtonOutputs {
    pub horn: bool,
    pub nitrous: bool,
}

/// One tick's inputs for the buttons.
pub(crate) struct ButtonInputs<'a> {
    pub placement: &'a ButtonPlacement,
    pub config: &'a CockpitButtonsTuning,
    /// Each hand's grip position, `None` while untracked.
    pub grips: [Option<Vec3>; 2],
    /// Each hand's grip orientation, `None` while untracked.
    pub orientations: [Option<Quat>; 2],
    /// What each hand holds after this tick's grabs and releases.
    pub holds: [Option<Hold>; 2],
}

/// Advance the buttons by one tick: the free hands press the push buttons, and a hand holding the
/// collective twists for the nitrous.
pub(crate) fn step_buttons(
    state: &mut ButtonState,
    inputs: &ButtonInputs,
    events: &mut Vec<ButtonEvent>,
) -> ButtonOutputs {
    for button in Button::ALL {
        let placed = inputs.placement.button(button);
        for hand in Hand::BOTH {
            let i = hand.index();
            let was = state.phases[button.index()][i];
            let free = inputs.holds[i].is_none();
            let now = match placed.filter(|_| free) {
                Some(b) => push_step(was, &b, inputs.grips[i], inputs.config),
                None => PushPhase::Out,
            };
            state.phases[button.index()][i] = now;
            match (was == PushPhase::Pressed, now == PushPhase::Pressed) {
                (false, true) => events.push(ButtonEvent::Pressed(hand, button)),
                (true, false) => events.push(ButtonEvent::Released(hand, button)),
                _ => {}
            }
        }
    }
    for hand in Hand::BOTH {
        let i = hand.index();
        let grip = inputs.holds[i].and_then(|hold| match hold.kind {
            ControlKind::Collective if inputs.placement.collective_twist => {
                Some((TwistGrip::Collective, hold))
            }
            _ => None,
        });
        let was = state.twists[i];
        let now = grip.and_then(|(grip, hold)| {
            let twist = grip_twist(hold.orientation?, inputs.orientations[i]?);
            twist_engaged(twist, was == Some(grip), inputs.config).then_some(grip)
        });
        state.twists[i] = now;
        if was != now {
            if let Some(grip) = was {
                events.push(ButtonEvent::Untwisted(hand, grip));
            }
            if let Some(grip) = now {
                events.push(ButtonEvent::Twisted(hand, grip));
            }
        }
    }
    ButtonOutputs {
        horn: state.pressed(Button::Horn),
        nitrous: state.pressed(Button::Nitrous)
            || state.twists.contains(&Some(TwistGrip::Collective)),
    }
}
