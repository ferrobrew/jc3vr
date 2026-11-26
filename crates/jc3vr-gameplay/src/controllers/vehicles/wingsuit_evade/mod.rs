//! The boost wingsuit's barrel-roll evade, thrown with the arms.
//!
//! The game's evade ([`ACT_BOOST_WINGSUIT_BARREL_ROLL`]) starts when the `WINGSUIT_EVADE` action is
//! clicked with the move stick out of its rest, and the stick's direction picks which way the body
//! rolls. A player flying with the arms spread as wings has no free button for it, so the evade is
//! a gesture: a throw of the arms into a steep bank, much steeper and faster than any bank the arm
//! steering ([`crate::controllers::vehicles::arm_steering`]) turns with. A throw is the bank
//! reaching [`WingsuitEvadeConfig::throw_bank_deg`] either way after swinging toward that side by
//! [`WingsuitEvadeConfig::throw_sweep_deg`] within [`WingsuitEvadeConfig::throw_window_s`]. After a
//! throw the arms must come back within [`WingsuitEvadeConfig::rearm_bank_deg`], and the cooldown
//! must pass, before the next counts, so one throw is one roll.
//!
//! The throw clicks the action and, for [`DIRECTION_HOLD_S`], writes the move stick as the throw's
//! direction over the arms' own: full to the side the arms banked toward, the side the lower hand
//! is on, and, while the arms pitch past [`WingsuitEvadeConfig::diagonal_pitch`], as far up or down
//! the stick as the arms pitch, so the game picks the roll toward the throw (one of its side or
//! diagonal rolls) rather than one from whatever the arms steered with as the throw began.
//!
//! The recognizer reads the arm steering's pose, so it runs while the arm steering does: in the
//! wingsuit, with the arm steering on. It takes no throw while the air brake is held (by the brake
//! gesture or a button), while the grapple's aim holds the arms' steering, or without the boost
//! wingsuit,
//! which is the only suit that has the evade
//! ([`Character::HasBoostWingsuit`](jc3gi::character::character::Character::HasBoostWingsuit)).
//! The game still decides whether the roll plays: short of the boost's charge it shows the boost's
//! failure instead, unless an enemy missile is locked on.
//!
//! The view rolls with the body through the evade: the cockpit frame follows the character's
//! whole rotation in the wingsuit, the evade's roll included, so the head never parts from the
//! body. Switched off, no throw is taken and the player's barrel rolls are dropped
//! ([`jc3vr_body::hooks::input::wingsuit`]).
//!
//! [`ACT_BOOST_WINGSUIT_BARREL_ROLL`]: jc3gi::animation::symbol_table::EventIdSymbolTable::ACT_BOOST_WINGSUIT_BARREL_ROLL

use std::{collections::VecDeque, fmt};

use glam::Vec2;
use jc3gi::input::input_action_map::Action::{WINGSUIT_AIRBRAKE, WINGSUIT_EVADE};
use jc3vr_body::controllers::{
    intent::hold::{Desired, MapKind, Target, Want},
    locomotion::axis_output::write_move,
    physics::characters::local_player,
};
use parking_lot::Mutex;

use crate::controllers::vehicles::{config::WingsuitEvadeConfig, types::ArmReadout};

/// Which way a throw rolls the body: the side the arms banked toward, and the arms' pitch past the
/// diagonal threshold, if any.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct EvadeDirection {
    pub side: Side,
    pub pitch: Option<Pitch>,
}

impl EvadeDirection {
    /// The move stick the throw writes (+X right, +Y forward), of unit length.
    pub fn stick(self) -> Vec2 {
        let x = match self.side {
            Side::Left => -1.0,
            Side::Right => 1.0,
        };
        let y = match self.pitch {
            None => 0.0,
            Some(Pitch::Forward) => 1.0,
            Some(Pitch::Back) => -1.0,
        };
        Vec2::new(x, y).normalize()
    }
}

impl fmt::Display for EvadeDirection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let side = match self.side {
            Side::Left => "left",
            Side::Right => "right",
        };
        match self.pitch {
            None => f.write_str(side),
            Some(Pitch::Forward) => write!(f, "{side} with the stick forward"),
            Some(Pitch::Back) => write!(f, "{side} with the stick back"),
        }
    }
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum Side {
    Left,
    Right,
}

/// The stick's vertical half the arms pitch toward, in the move stick's convention.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum Pitch {
    Forward,
    Back,
}

/// What keeps a throw from being taken.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum Blocked {
    /// The last throw's cooldown has not passed.
    Cooldown,
    /// The player's suit is not the boost wingsuit, or the player is not flying it.
    NoBoostSuit,
    /// The air brake is held.
    Braking,
    /// The grapple's aim holds the arms' steering.
    Aiming,
}

impl fmt::Display for Blocked {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Cooldown => "the last evade's cooldown has not passed",
            Self::NoBoostSuit => "the evade needs the boost wingsuit in flight",
            Self::Braking => "the air brake is held",
            Self::Aiming => "the grapple's aim holds the arms",
        })
    }
}

/// What the recognizer made of one tick.
#[derive(Debug, Copy, Clone, PartialEq)]
pub enum Outcome {
    /// No throw, and no throw's direction held.
    Idle,
    /// A throw this tick: click the evade and write the direction.
    Throw(Throw),
    /// A throw's direction, still written after the throw.
    Holding(EvadeDirection),
    /// A throw this tick that a gate turned away.
    Refused(Throw, Blocked),
}

/// A throw as recognized.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct Throw {
    pub direction: EvadeDirection,
    /// The bank it reached, degrees, positive with the left hand above the right.
    pub bank_deg: f32,
    /// How far the bank swung toward the throw's side within the window, degrees.
    pub sweep_deg: f32,
}

/// One tick's observation.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct EvadeInput {
    /// The arms' bank, degrees, or `None` while the arms are not spread wide enough to read one.
    pub bank_deg: Option<f32>,
    /// The arms' pitch as the move stick's vertical (+Y forward), `-1..=1`.
    pub pitch: f32,
    /// A gate outside the recognizer that turns throws away, if any.
    pub gate: Option<Blocked>,
    pub dt: f32,
}

/// The throw recognizer. Pure: it reads only its inputs.
#[derive(Debug, Clone)]
pub struct EvadeRecognizer {
    clock: f32,
    /// Recent banks, oldest first, as (clock, bank in degrees), within the throw window.
    samples: VecDeque<(f32, f32)>,
    cooldown: f32,
    /// Whether the arms have come back within the re-arming bank since the last throw.
    armed: bool,
    holding: Option<(EvadeDirection, f32)>,
}

impl Default for EvadeRecognizer {
    fn default() -> Self {
        Self {
            clock: 0.0,
            samples: VecDeque::new(),
            cooldown: 0.0,
            armed: true,
            holding: None,
        }
    }
}

impl EvadeRecognizer {
    /// Forget the pose history and any held direction. The cooldown keeps running, so leaving and
    /// re-entering the arm steering does not reset it.
    pub fn reset(&mut self) {
        self.samples.clear();
        self.armed = true;
        self.holding = None;
    }

    /// Step the recognizer by one observation.
    pub fn step(&mut self, input: EvadeInput, config: &WingsuitEvadeConfig) -> Outcome {
        let dt = if input.dt.is_finite() {
            input.dt.max(0.0)
        } else {
            0.0
        };
        self.clock += dt;
        self.cooldown = (self.cooldown - dt).max(0.0);
        self.holding = self
            .holding
            .map(|(direction, left)| (direction, left - dt))
            .filter(|(_, left)| *left > 0.0);
        let Some(bank) = input.bank_deg.filter(|bank| bank.is_finite()) else {
            self.reset();
            return Outcome::Idle;
        };
        let window = config.throw_window_s;
        let clock = self.clock;
        self.samples.retain(|(at, _)| clock - at <= window);
        self.samples.push_back((clock, bank));
        if bank.abs() <= config.rearm_bank_deg {
            self.armed = true;
        }
        if let Some(throw) = self.throw(bank, input.pitch, config) {
            self.armed = false;
            self.samples.clear();
            let gate = input
                .gate
                .or((self.cooldown > 0.0).then_some(Blocked::Cooldown));
            if let Some(blocked) = gate {
                return Outcome::Refused(throw, blocked);
            }
            self.cooldown = config.cooldown_s;
            self.holding = Some((throw.direction, DIRECTION_HOLD_S));
            return Outcome::Throw(throw);
        }
        match self.holding {
            Some((direction, _)) => Outcome::Holding(direction),
            None => Outcome::Idle,
        }
    }

    fn throw(&self, bank: f32, pitch: f32, config: &WingsuitEvadeConfig) -> Option<Throw> {
        if !self.armed || bank.abs() < config.throw_bank_deg {
            return None;
        }
        let toward = bank.signum();
        let start = self
            .samples
            .iter()
            .map(|(_, sample)| sample * toward)
            .fold(f32::INFINITY, f32::min);
        let sweep_deg = bank * toward - start;
        (sweep_deg >= config.throw_sweep_deg).then(|| Throw {
            direction: EvadeDirection {
                side: if toward > 0.0 {
                    Side::Right
                } else {
                    Side::Left
                },
                pitch: pitch_of(pitch, config.diagonal_pitch),
            },
            bank_deg: bank,
            sweep_deg,
        })
    }
}

/// The arms' bank, degrees, positive with the left hand above the right, from the hands' height
/// difference and horizontal separation, or `None` while the hands are closer together than
/// [`MIN_SEPARATION`] of the arm span, where a height difference is no bank.
pub fn bank_deg(bank_m: f32, spread: f32, arm_span: f32) -> Option<f32> {
    let span = arm_span.max(0.1);
    let across = spread * span;
    let separation = across.hypot(bank_m) / span;
    (bank_m.is_finite() && spread.is_finite() && separation >= MIN_SEPARATION)
        .then(|| bank_m.atan2(across).to_degrees())
}

/// What the evade shows in the debug UI.
#[derive(Debug, Copy, Clone, Default, PartialEq)]
pub struct EvadeStatus {
    /// The arms' bank on the last tick the recognizer read one, degrees.
    pub bank_deg: Option<f32>,
    /// The last throw taken, and the last turned away with its reason.
    pub last_throw: Option<Throw>,
    pub last_refusal: Option<(Throw, Blocked)>,
    pub throws: u64,
    pub refusals: u64,
    /// Whether a throw's direction is written now.
    pub holding: bool,
}

pub fn status() -> EvadeStatus {
    STATE.lock().status
}

/// The arm-steered tick's evade: read the arms' pose, and on a throw click the evade and write its
/// direction over the move stick the arm steering wrote. `arms` is the arm steering's readout,
/// `None` while it does not act. `desired` holds the tick's inputs so far, the air brake's among them.
///
/// # Safety
///
/// On the game thread, before the world update: a throw reads the local player character.
pub(super) unsafe fn tick(
    arms: Option<&ArmReadout>,
    arm_span: f32,
    config: &WingsuitEvadeConfig,
    desired: &mut Vec<Desired>,
    dt: f32,
) {
    let mut state = STATE.lock();
    let state = &mut *state;
    let Some(arms) = arms.filter(|_| config.enabled && config.valid()) else {
        state.recognizer.reset();
        state.status.bank_deg = None;
        state.status.holding = false;
        return;
    };
    let braking = desired.iter().any(|input| {
        input.target.map == MapKind::Player && input.target.action == WINGSUIT_AIRBRAKE
    });
    let gate = if braking {
        Some(Blocked::Braking)
    } else if arms.held_for_grapple {
        Some(Blocked::Aiming)
    } else {
        None
    };
    let bank = bank_deg(arms.pose.bank_m, arms.pose.spread, arm_span);
    let input = EvadeInput {
        bank_deg: bank,
        pitch: arms.written.y,
        gate,
        dt,
    };
    let mut outcome = state.recognizer.step(input, config);
    if let Outcome::Throw(throw) = outcome
        && !unsafe { flying_boost_suit() }
    {
        state.recognizer.cancel_throw();
        outcome = Outcome::Refused(throw, Blocked::NoBoostSuit);
    }
    state.status.bank_deg = bank;
    state.status.holding = matches!(outcome, Outcome::Throw(_) | Outcome::Holding(_));
    match outcome {
        Outcome::Idle => {}
        Outcome::Holding(direction) => write_move(desired, direction.stick()),
        Outcome::Throw(throw) => {
            state.status.throws += 1;
            state.status.last_throw = Some(throw);
            write_move(desired, throw.direction.stick());
            desired.push(Desired {
                target: Target {
                    map: MapKind::Player,
                    action: WINGSUIT_EVADE,
                },
                want: Want::Click,
            });
            tracing::info!(
                target: "controllers",
                "wingsuit evade: the arms threw a {} evade, banked {:+.0}° after a {:.0}° swing",
                throw.direction,
                throw.bank_deg,
                throw.sweep_deg,
            );
        }
        Outcome::Refused(throw, blocked) => {
            state.status.refusals += 1;
            state.status.last_refusal = Some((throw, blocked));
            tracing::info!(
                target: "controllers",
                "wingsuit evade: the arms threw a {} evade, but {blocked} (banked {:+.0}° after a \
                 {:.0}° swing)",
                throw.direction,
                throw.bank_deg,
                throw.sweep_deg,
            );
        }
    }
}

/// How long a throw's direction is written over the move stick, seconds: long enough for the
/// animation graph to pick the roll from it on the ticks after the click.
pub const DIRECTION_HOLD_S: f32 = 0.3;

/// The hands' smallest separation, as a share of the arm span, at which their height difference
/// reads as a bank: the arms spread as wings, not drawn in.
pub const MIN_SEPARATION: f32 = 0.6;

impl EvadeRecognizer {
    /// Take back the throw this tick's [`Self::step`] returned, which a gate after the recognizer
    /// turned away: no cooldown and no held direction. The throw still needs the arms to re-arm.
    fn cancel_throw(&mut self) {
        self.cooldown = 0.0;
        self.holding = None;
    }
}

fn pitch_of(pitch: f32, threshold: f32) -> Option<Pitch> {
    if !pitch.is_finite() || pitch.abs() < threshold || pitch == 0.0 {
        None
    } else if pitch > 0.0 {
        Some(Pitch::Forward)
    } else {
        Some(Pitch::Back)
    }
}

/// Whether the local player flies the boost wingsuit.
///
/// # Safety
///
/// On the game thread, before the world update.
unsafe fn flying_boost_suit() -> bool {
    unsafe {
        local_player().is_some_and(|owner| owner.HasBoostWingsuit() && owner.IsUsingWingsuit())
    }
}

struct State {
    recognizer: EvadeRecognizer,
    status: EvadeStatus,
}

static STATE: Mutex<State> = Mutex::new(State {
    recognizer: EvadeRecognizer {
        clock: 0.0,
        samples: VecDeque::new(),
        cooldown: 0.0,
        armed: true,
        holding: None,
    },
    status: EvadeStatus {
        bank_deg: None,
        last_throw: None,
        last_refusal: None,
        throws: 0,
        refusals: 0,
        holding: false,
    },
});

#[cfg(test)]
mod tests;
