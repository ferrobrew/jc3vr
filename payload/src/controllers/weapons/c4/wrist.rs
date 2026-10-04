//! The wrist detonator: a button on the inside of the off-hand's wrist that the dominant hand's
//! index fingertip presses to detonate the placed charges.
//!
//! On each input tick the button's face is placed from the off-hand controller's grip pose and the
//! fingertip from the dominant hand (its tracked skeleton, or an estimate from its controller), both
//! from the controller frame, and the press detector ([`press`]) decides whether this tick pressed
//! it. A press with charges placed asks the C4 weapon to detonate them
//! ([`request_detonation`]); any press clicks both hands. The button is drawn on the wrist through
//! the world-space overlay: dim with no charges placed, lit while there are, and brighter while the
//! fingertip pushes into it.

use std::time::{Duration, Instant};

use glam::Vec3;
use parking_lot::Mutex;

use crate::controllers::{
    config::ControllersConfig,
    frame::{self, Hand},
    overlay::{
        primitive::{
            Anchor, DepthMode, Facing, HandPose, Primitive, Rgba, Shape, Source, SpriteSize,
        },
        store,
    },
    weapons::{
        c4::{
            self, DetonationSource,
            press::{self, FingertipSource, PressDetector, PressPhase, PressReading},
        },
        haptics::{self, PulseSource},
    },
};

/// Where the fingertip was read from.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum FingertipSourceKind {
    /// The tracked hand skeleton's index tip.
    Skeleton,
    /// The estimate from the controller's aim pose.
    Estimate,
}

impl FingertipSourceKind {
    pub const fn label(self) -> &'static str {
        match self {
            FingertipSourceKind::Skeleton => "hand skeleton",
            FingertipSourceKind::Estimate => "controller estimate",
        }
    }
}

/// The wrist detonator's readout.
#[derive(Debug, Copy, Clone, PartialEq, Default)]
pub struct WristStatus {
    /// Whether the last input tick ran the detonator.
    pub acting: bool,
    /// The press detector's last reading.
    pub reading: PressReading,
    pub fingertip: Option<FingertipSourceKind>,
    /// Whether the button was lit: charges were placed.
    pub lit: bool,
    pub presses: u64,
    /// Presses with no charges placed, which detonate nothing.
    pub idle_presses: u64,
}

pub fn status() -> WristStatus {
    STATE.lock().status
}

/// What the input tick hands the detonator.
pub(crate) struct TickInputs {
    /// Whether the detonator may act this tick: its toggle is on and the hands are active.
    pub acting: bool,
    pub dt: f32,
}

/// The input tick's half of the detonator: find the button and the fingertip, step the press, act
/// on a press, and draw the button.
pub(crate) fn input_tick(inputs: &TickInputs, config: &ControllersConfig) {
    if !inputs.acting {
        stand_down();
        return;
    }
    let tuning = &config.weapons.c4.detonator;
    let dominant = config.dominant_hand;
    let off = dominant.other();
    let placed = frame::with_latest(|frame| {
        let off_grip = frame.hand(off).grip;
        let face = (frame.hand(off).active && off_grip.is_valid())
            .then(|| press::button_face(off, off_grip.pose, tuning))
            .flatten();
        let dom = frame.hand(dominant);
        let joints = dom.joints.as_ref();
        let tip = (dom.active && dom.aim.is_valid()).then(|| {
            let source = FingertipSource {
                joints,
                aim: dom.aim.pose,
                trigger_touch: dom.inputs.trigger_touch,
            };
            let kind = match joints {
                Some(j) if j[press::INDEX_TIP_JOINT].valid => FingertipSourceKind::Skeleton,
                _ => FingertipSourceKind::Estimate,
            };
            (press::fingertip(dominant, source, tuning), kind)
        });
        (face, tip)
    });
    let (face, tip) = placed.unwrap_or((None, None));
    let charges = c4::charges().unwrap_or(0);
    let lit = charges > 0;

    let mut state = STATE.lock();
    let reading = state
        .detector
        .step(tip.map(|(t, _)| t), face, inputs.dt, tuning);
    state.status = WristStatus {
        acting: true,
        reading,
        fingertip: tip.map(|(_, kind)| kind),
        lit,
        presses: state.status.presses + u64::from(reading.pressed),
        idle_presses: state.status.idle_presses + u64::from(reading.pressed && !lit),
    };
    if reading.pressed {
        state.flash_until = Some(Instant::now() + FLASH);
    }
    let flashing = state.flash_until.is_some_and(|t| Instant::now() < t);
    drop(state);

    if reading.pressed {
        tracing::debug!(target: "controllers", charges, "wrist detonator pressed");
        if lit {
            c4::request_detonation(DetonationSource::WristButton);
        }
        for hand in Hand::BOTH {
            haptics::pulse(hand, tuning.pulse, PulseSource::C4);
        }
    }
    if tuning.overlay && config.overlay.enabled {
        let offset = mirrored(off, tuning.offset_m);
        let mut prims = vec![button(off, offset, tuning.radius_m, lit, reading, flashing)];
        store::submit(Source::WristDetonator, &mut prims, TTL);
    } else {
        store::clear(Source::WristDetonator);
    }
}

/// Stop: forget the fingertip's approach and take the button off the wrist.
pub(crate) fn stand_down() {
    let mut state = STATE.lock();
    state.detector = PressDetector::new();
    state.status.acting = false;
    state.status.reading = PressReading::default();
    state.flash_until = None;
    drop(state);
    store::clear(Source::WristDetonator);
}

/// The overlay slot lives a little past one input tick.
const TTL: Duration = Duration::from_millis(150);

/// How long the button flashes after a press.
const FLASH: Duration = Duration::from_millis(250);

const DIM: Rgba = [0.6, 0.6, 0.6, 0.5];
const LIT: Rgba = [1.0, 0.2, 0.1, 0.9];
const FLASHING: Rgba = [1.0, 0.95, 0.8, 1.0];

struct State {
    detector: PressDetector,
    status: WristStatus,
    flash_until: Option<Instant>,
}

static STATE: Mutex<State> = Mutex::new(State {
    detector: PressDetector::new(),
    status: WristStatus {
        acting: false,
        reading: PressReading {
            pressed: false,
            depth_m: None,
            lateral_m: None,
            phase: PressPhase::Lost,
        },
        fingertip: None,
        lit: false,
        presses: 0,
        idle_presses: 0,
    },
    flash_until: None,
});

/// A vector given for the left hand, mirrored for `hand`.
fn mirrored(hand: Hand, v: Vec3) -> Vec3 {
    match hand {
        Hand::Left => v,
        Hand::Right => Vec3::new(-v.x, v.y, v.z),
    }
}

/// The button's sprite on the `off` wrist: a ring while no charges are placed, a disc while they
/// are, growing brighter as the fingertip pushes in, and a flash after a press.
fn button(
    off: Hand,
    offset: Vec3,
    radius: f32,
    lit: bool,
    reading: PressReading,
    flashing: bool,
) -> Primitive {
    let pushing = matches!(reading.phase, PressPhase::Pushing { .. });
    let color = if flashing {
        FLASHING
    } else if lit {
        let mut c = LIT;
        if pushing {
            c[1] = 0.5;
        }
        c
    } else {
        DIM
    };
    Primitive::Sprite {
        at: Anchor::Hand {
            hand: off,
            pose: HandPose::Grip,
            offset,
        },
        size: SpriteSize::World(radius.max(0.005) * 2.0),
        facing: Facing::Billboard,
        shape: if lit || flashing {
            Shape::Disc
        } else {
            Shape::Ring { thickness: 0.3 }
        },
        color,
        depth: DepthMode::XRay {
            occluded_alpha: 0.5,
        },
    }
}
