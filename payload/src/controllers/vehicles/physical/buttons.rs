//! The cockpit's buttons, worked by hand: the horn on the wheel's hub and a nitrous button on the
//! dash beside the wheel, pressed by pushing a free hand into them, and the twist grips, which
//! burn the nitrous while the hand holding the handlebars' right grip or the collective twists it.
//! Pure.
//!
//! A push button is a disc facing the driver. A hand arms it by coming to it from the front, within
//! its radius, and presses it by pushing on past its face by the press depth; it stays pressed,
//! with some give, until the hand comes back out or slides off. A hand that reaches the disc from
//! behind or from the side, as one passing through the dash or sweeping across the wheel does,
//! presses nothing.
//!
//! The vehicle's own inputs are held while a button is pressed: `SOUND_HORN_SIREN` for the horn
//! and `USE_VEHICLE_MOD` for the nitrous, which the game reads as nitrous while held and as the
//! turbo jump when tapped, as it does the gamepad's button.

use glam::{Quat, Vec2, Vec3};

use crate::controllers::{
    frame::Hand,
    vehicles::{
        config::CockpitButtonsConfig,
        physical::{ControlKind, Hold, bar_ends, sticks::twist_about},
        wheel::{BarsFrame, WheelFrame},
    },
};

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

    const fn index(self) -> usize {
        match self {
            Button::Horn => 0,
            Button::Nitrous => 1,
        }
    }
}

/// A push button in the cockpit frame: a disc facing the driver.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct PushButton {
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
    pub fn horn(wheel: &WheelFrame, config: &CockpitButtonsConfig) -> Self {
        Self {
            centre: wheel.centre,
            normal: wheel.normal(),
            radius: wheel.radius * config.hub_share.clamp(0.1, 0.6),
            depth: config.press_depth_m,
        }
    }

    /// The nitrous button on the dash, beside `wheel` to its right and a little up, facing the
    /// driver as the wheel does.
    pub fn nitrous(wheel: &WheelFrame, config: &CockpitButtonsConfig) -> Self {
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
pub fn push_step(phase: PushPhase, button: &PushButton, point: Option<Vec3>) -> PushPhase {
    let Some(point) = point.filter(|p| p.is_finite()) else {
        return PushPhase::Out;
    };
    let d = point - button.centre;
    let along = d.dot(button.normal);
    let lateral = d.reject_from_normalized(button.normal).length();
    let depth = button.depth.max(0.005);
    let in_front = (0.0..=ARM_REACH_M).contains(&along) && lateral <= button.radius;
    let pushed = along <= -depth && along >= -MAX_PUSH_M && lateral <= button.radius;
    // A pressed button gives a little before it lets go, so a hand resting on it does not chatter.
    let held = along <= -depth * 0.5 && along >= -MAX_PUSH_M && lateral <= button.radius * 1.5;
    match phase {
        PushPhase::Armed if pushed => PushPhase::Pressed,
        PushPhase::Pressed if held => PushPhase::Pressed,
        _ if in_front => PushPhase::Armed,
        _ => PushPhase::Out,
    }
}

/// The twist grips' place: the handlebars' right grip and the collective.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum TwistGrip {
    /// The handlebars' right grip, the throttle, twisted back as a motorcycle's throttle is.
    Bars,
    /// The collective's grip, twisted either way for the nitrous.
    Collective,
}

impl TwistGrip {
    pub const fn label(self) -> &'static str {
        match self {
            TwistGrip::Bars => "handlebars' throttle grip",
            TwistGrip::Collective => "collective's twist grip",
        }
    }
}

/// How far a hand that took a control at orientation `grab` and holds it at `now` has twisted
/// it, radians: for the bars, about the line from the left grip to the right (`across`), positive
/// rolling the grip's top back toward the driver; for the collective, about the fore-aft axis,
/// either way.
pub fn grip_twist(grip: TwistGrip, grab: Quat, now: Quat, across: Vec3) -> f32 {
    let delta = now * grab.inverse();
    match grip {
        TwistGrip::Bars => twist_about(delta, across.try_normalize().unwrap_or(Vec3::X)),
        TwistGrip::Collective => twist_about(delta, Vec3::Z).abs(),
    }
}

/// The throttle a handlebars' right grip twisted back by `twist` radians opens, `0..=1`: nothing
/// within [`THROTTLE_DEADZONE_DEG`], so a hand that settles on the grip does not creep, and full at
/// [`CockpitButtonsConfig::throttle_twist_deg`]. A grip rolled forward is closed.
pub fn twist_throttle(twist: f32, config: &CockpitButtonsConfig) -> f32 {
    let deadzone = THROTTLE_DEADZONE_DEG.to_radians();
    let full = config
        .throttle_twist_deg
        .max(THROTTLE_DEADZONE_DEG + 1.0)
        .to_radians();
    if !twist.is_finite() || twist <= deadzone {
        return 0.0;
    }
    ((twist - deadzone) / (full - deadzone)).min(1.0)
}

/// Whether a twist grip at `twist` radians burns the nitrous, given whether it did on the last
/// tick: past the twist to engage, and until it comes back by a margin.
pub fn twist_engaged(twist: f32, was: bool, config: &CockpitButtonsConfig) -> bool {
    let engage = config.twist_deg.to_radians();
    let release = (config.twist_deg - TWIST_HYSTERESIS_DEG)
        .max(0.0)
        .to_radians();
    if was {
        twist >= release
    } else {
        twist >= engage
    }
}

/// The buttons' state: each hand's phase with each push button, and the twist grip each hand works:
/// the collective's burning the nitrous, or the bars' throttle open.
#[derive(Debug, Copy, Clone, PartialEq, Default)]
pub struct ButtonState {
    /// Indexed by [`Button`], then by [`Hand::index`].
    pub phases: [[PushPhase; 2]; 2],
    /// The twist grip each hand works: the collective's past its nitrous twist, or the bars'
    /// throttle open.
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
pub struct ButtonPlacement {
    pub horn: Option<PushButton>,
    pub nitrous: Option<PushButton>,
    pub bars_throttle: bool,
    pub collective_twist: bool,
}

impl ButtonPlacement {
    /// The buttons a seat offering `kinds` has with `config`: the horn and the nitrous button with
    /// the wheel, the bars' throttle with the handlebars, and the collective's twist with the
    /// collective.
    pub fn for_seat(
        kinds: &[ControlKind],
        config: &CockpitButtonsConfig,
        wheel: &WheelFrame,
    ) -> Self {
        let wheel_seat = kinds.contains(&ControlKind::Wheel);
        Self {
            horn: (wheel_seat && config.horn).then(|| PushButton::horn(wheel, config)),
            nitrous: (wheel_seat && config.nitrous_button)
                .then(|| PushButton::nitrous(wheel, config)),
            bars_throttle: config.twist_throttle && kinds.contains(&ControlKind::Handlebars),
            collective_twist: config.twist_nitrous && kinds.contains(&ControlKind::Collective),
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
pub struct ButtonOutputs {
    pub horn: bool,
    pub nitrous: bool,
    /// The throttle the bars' right grip opens, `0..=1`, while the right hand holds it.
    pub throttle: Option<f32>,
}

/// One tick's inputs for the buttons.
pub struct ButtonInputs<'a> {
    pub placement: &'a ButtonPlacement,
    pub config: &'a CockpitButtonsConfig,
    pub bars: &'a BarsFrame,
    /// Each hand's grip position, `None` while untracked.
    pub grips: [Option<Vec3>; 2],
    /// Each hand's grip orientation, `None` while untracked.
    pub orientations: [Option<Quat>; 2],
    /// What each hand holds after this tick's grabs and releases.
    pub holds: [Option<Hold>; 2],
    /// The handlebars' turn and roll, radians, for the line across them.
    pub bars_angles: Vec2,
}

/// Advance the buttons by one tick: the free hands press the push buttons, the hand holding the
/// bars' right grip twists the throttle, and a hand holding the collective twists for the
/// nitrous.
pub fn step_buttons(
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
                Some(b) => push_step(was, &b, inputs.grips[i]),
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
    let [left, right] = bar_ends(inputs.bars.centre, 1.0, inputs.bars_angles);
    let across = right - left;
    let mut throttle = None;
    for hand in Hand::BOTH {
        let i = hand.index();
        let grip = inputs.holds[i].and_then(|hold| match hold.kind {
            ControlKind::Handlebars if inputs.placement.bars_throttle && hand == Hand::Right => {
                Some((TwistGrip::Bars, hold))
            }
            ControlKind::Collective if inputs.placement.collective_twist => {
                Some((TwistGrip::Collective, hold))
            }
            _ => None,
        });
        let was = state.twists[i];
        let now = grip.and_then(|(grip, hold)| {
            let twist = grip_twist(grip, hold.orientation?, inputs.orientations[i]?, across);
            match grip {
                TwistGrip::Bars => {
                    let open = twist_throttle(twist, inputs.config);
                    throttle = Some(open);
                    (open > 0.0).then_some(grip)
                }
                TwistGrip::Collective => {
                    twist_engaged(twist, was == Some(grip), inputs.config).then_some(grip)
                }
            }
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
        throttle,
    }
}

/// How far in front of a push button's face a hand may be to arm it, metres.
const ARM_REACH_M: f32 = 0.12;

/// How far past a push button's face a hand may be and still press it, metres: further is a hand
/// through the dash.
const MAX_PUSH_M: f32 = 0.12;

/// The twist of the handlebars' right grip that opens no throttle, degrees.
pub const THROTTLE_DEADZONE_DEG: f32 = 4.0;

/// How far a twist grip comes back from its engaging twist before it lets go, degrees.
const TWIST_HYSTERESIS_DEG: f32 = 8.0;
