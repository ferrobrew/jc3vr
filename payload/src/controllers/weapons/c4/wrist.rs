//! The wrist detonator: a button on the inside of one wrist that the other hand's index fingertip
//! presses to detonate the placed charges. The pressing hand is the hand the guns ride
//! ([`crate::controllers::weapons::volumes::gun_side`], named by [`TickInputs::presser`]), and the
//! button is on the other: a gun can ride the hand that is not the player's dominant one, and the
//! button belongs on the hand that the pouches leave free.
//!
//! On each input tick the button is placed on its hand where the hand is drawn (its grip in
//! [`hands::active_hands`], the poses the rendered hands follow), and the pressing fingertip is
//! read from the controller frame (its tracked skeleton, or an estimate from its controller) and
//! carried with its hand to where that hand is drawn ([`press::carry`]). The press
//! detector ([`press`]) and the drawn button therefore share one frame: the finger the player sees
//! touching the button is the one that presses it, whatever the body-scale remap or the two-handed
//! pose does to the hands.
//!
//! A press with charges placed asks the C4 weapon to detonate them ([`request_detonation`]) and
//! clicks both hands; a press with none buzzes the pressing hand and flashes the button grey. The
//! button is drawn on the wrist through the world-space overlay, lying on the wrist facing out of
//! it: dim with no charges placed, lit with a pip beside it for each placed charge, filling as a
//! slow touch dwells, and flashing on a press. A detonation the game starts, from the button or the
//! stick click, flashes it bright ([`note_detonation`]).

use std::time::{Duration, Instant};

use glam::Vec3;
use parking_lot::Mutex;

use crate::{
    controllers::{
        config::ControllersConfig,
        frame::{self, Hand},
        hands,
        haptics::{self, Event},
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
            config::WristDetonatorConfig,
        },
    },
    headpose,
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
    /// The hand whose fingertip presses the button, which sits on the other hand's wrist: the hand
    /// the guns ride ([`crate::controllers::weapons::volumes::gun_side`]).
    pub presser: Hand,
    pub dt: f32,
}

/// The input tick's half of the detonator: find the button and the fingertip, step the press, act
/// on a press, and draw the button.
pub(crate) fn input_tick(inputs: &TickInputs, config: &ControllersConfig) {
    if !inputs.acting {
        stand_down();
        return;
    }
    let Some(active) = hands::active_hands() else {
        stand_down();
        return;
    };
    let tuning = &config.weapons.c4.detonator;
    let presser = inputs.presser;
    let off = presser.other();
    let drawn = active.hands;
    let face = drawn[off.index()].and_then(|hand| press::button_face(off, hand.grip, tuning));
    let tip = frame::with_latest(|frame| {
        let dom = frame.hand(presser);
        let drawn = drawn[presser.index()]?;
        if !(dom.active && dom.aim.is_valid() && dom.grip.is_valid()) {
            return None;
        }
        let joints = dom.joints.as_ref();
        let source = FingertipSource {
            joints,
            aim: dom.aim.pose,
            trigger_touch: dom.inputs.trigger_touch,
        };
        let kind = match joints {
            Some(j) if j[press::INDEX_TIP_JOINT].valid => FingertipSourceKind::Skeleton,
            _ => FingertipSourceKind::Estimate,
        };
        let tip = press::fingertip(presser, source, tuning);
        Some((press::carry(tip, dom.grip.pose, drawn.grip), kind))
    })
    .flatten();
    let charges = c4::charges().unwrap_or(0).max(0);
    let lit = charges > 0;

    let now = Instant::now();
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
        state.flash = Some(Flash {
            kind: if lit {
                FlashKind::Pressed
            } else {
                FlashKind::Empty
            },
            until: now + FlashKind::Pressed.length(),
        });
    }
    state.flash = state.flash.filter(|f| now < f.until);
    let flash = state.flash.map(|f| f.kind);
    drop(state);

    if reading.pressed {
        if lit {
            tracing::info!(target: "controllers", charges, "wrist detonator: detonating");
            c4::request_detonation(DetonationSource::WristButton);
            for hand in Hand::BOTH {
                haptics::play(hand, Event::DetonatorPress);
            }
        } else {
            tracing::info!(target: "controllers", "wrist detonator: pressed with no charges placed");
            haptics::play(presser, Event::DetonatorEmpty);
        }
    }
    let Some(face) = face.filter(|_| tuning.overlay && config.overlay.enabled) else {
        store::clear(Source::WristDetonator);
        return;
    };
    let body = headpose::xr::body_rotation();
    let look = Look {
        off,
        normal: (body * face.normal).normalize_or(Vec3::Y),
        charges,
        progress: reading.phase.dwell_progress(tuning),
        flash,
    };
    let mut prims = button(&look, tuning);
    store::submit(Source::WristDetonator, &mut prims, TTL);
}

/// Note that the game started a detonation, from the wrist button or its own input, so that the
/// button flashes it.
pub(super) fn note_detonation() {
    STATE.lock().flash = Some(Flash {
        kind: FlashKind::Detonated,
        until: Instant::now() + FlashKind::Detonated.length(),
    });
}

/// Stop: forget the fingertip's approach and take the button off the wrist.
pub(crate) fn stand_down() {
    let mut state = STATE.lock();
    state.detector = PressDetector::new();
    state.status.acting = false;
    state.status.reading = PressReading::default();
    state.flash = None;
    drop(state);
    store::clear(Source::WristDetonator);
}

/// The overlay slot lives a little past one input tick.
const TTL: Duration = Duration::from_millis(150);

/// The most pips drawn beside the button; more charges than this show as this many.
const MAX_PIPS: i32 = 8;

/// A pip's diameter and the gap from one pip's centre to the next, metres.
const PIP_SIZE_M: f32 = 0.009;
const PIP_SPACING_M: f32 = 0.013;

const DIM: Rgba = [0.55, 0.55, 0.6, 0.35];
const DIM_RIM: Rgba = [0.8, 0.8, 0.85, 0.7];
const LIT: Rgba = [1.0, 0.2, 0.1, 0.9];
const LIT_RIM: Rgba = [1.0, 0.75, 0.6, 0.95];
const DWELL: Rgba = [1.0, 0.85, 0.2, 0.95];
const PRESSED: Rgba = [1.0, 0.95, 0.8, 1.0];
const EMPTY: Rgba = [0.5, 0.6, 0.8, 0.9];
const DETONATED: Rgba = [1.0, 0.6, 0.1, 1.0];
const PIP: Rgba = [1.0, 0.35, 0.15, 0.95];

/// A flash of the button after an event.
#[derive(Debug, Copy, Clone, PartialEq)]
struct Flash {
    kind: FlashKind,
    until: Instant,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
enum FlashKind {
    /// A press with charges placed.
    Pressed,
    /// A press with no charges placed.
    Empty,
    /// The game started a detonation.
    Detonated,
}

impl FlashKind {
    const fn length(self) -> Duration {
        match self {
            FlashKind::Pressed | FlashKind::Empty => Duration::from_millis(250),
            FlashKind::Detonated => Duration::from_millis(600),
        }
    }
}

struct State {
    detector: PressDetector,
    status: WristStatus,
    flash: Option<Flash>,
}

static STATE: Mutex<State> = Mutex::new(State {
    detector: PressDetector::new(),
    status: WristStatus {
        acting: false,
        reading: PressReading {
            pressed: false,
            distance_m: None,
            height_m: None,
            phase: PressPhase::Lost,
        },
        fingertip: None,
        lit: false,
        presses: 0,
        idle_presses: 0,
    },
    flash: None,
});

/// What the button looks like on a tick.
struct Look {
    off: Hand,
    /// The face's normal in the world.
    normal: Vec3,
    charges: i32,
    /// How far a slow touch is through its dwell, `0..=1`.
    progress: f32,
    flash: Option<FlashKind>,
}

/// The button's sprites on the off-hand's wrist, lying on the wrist facing out of it: a disc and
/// its rim, and a pip for each placed charge in a row along the forearm toward the elbow.
fn button(look: &Look, tuning: &WristDetonatorConfig) -> Vec<Primitive> {
    let centre = mirrored(look.off, tuning.offset_m);
    let radius = tuning.radius_m.max(0.005);
    let lit = look.charges > 0;
    let (fill, rim, scale) = match look.flash {
        Some(FlashKind::Detonated) => (DETONATED, DETONATED, 1.8),
        Some(FlashKind::Pressed) => (PRESSED, PRESSED, 1.2),
        Some(FlashKind::Empty) => (EMPTY, EMPTY, 1.0),
        None if lit => (mix(LIT, DWELL, look.progress), LIT_RIM, 1.0),
        None => (mix(DIM, DWELL, look.progress), DIM_RIM, 1.0),
    };
    let sprite = |offset: Vec3, size: f32, shape: Shape, color: Rgba| Primitive::Sprite {
        at: Anchor::Hand {
            hand: look.off,
            pose: HandPose::Grip,
            offset,
        },
        size: SpriteSize::World(size),
        facing: Facing::Surface {
            normal: look.normal,
        },
        shape,
        color,
        depth: DepthMode::XRay {
            occluded_alpha: 0.5,
        },
    };
    let diameter = radius * 2.0 * scale;
    let mut prims = vec![
        sprite(centre, diameter, Shape::Disc, fill),
        sprite(centre, diameter, Shape::Ring { thickness: 0.25 }, rim),
    ];
    // +Z of the grip frame runs back from the fist toward the elbow.
    for i in 0..look.charges.min(MAX_PIPS) {
        let along = radius + PIP_SPACING_M * (i as f32 + 1.0);
        prims.push(sprite(
            centre + Vec3::Z * along,
            PIP_SIZE_M,
            Shape::Disc,
            PIP,
        ));
    }
    prims
}

/// A vector given for the left hand, mirrored for `hand`.
fn mirrored(hand: Hand, v: Vec3) -> Vec3 {
    match hand {
        Hand::Left => v,
        Hand::Right => Vec3::new(-v.x, v.y, v.z),
    }
}

fn mix(a: Rgba, b: Rgba, t: f32) -> Rgba {
    let t = t.clamp(0.0, 1.0);
    std::array::from_fn(|i| a[i] + (b[i] - a[i]) * t)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn look(charges: i32, flash: Option<FlashKind>) -> Look {
        Look {
            off: Hand::Left,
            normal: Vec3::X,
            charges,
            progress: 0.0,
            flash,
        }
    }

    fn colour(prim: &Primitive) -> Rgba {
        match prim {
            Primitive::Sprite { color, .. } => *color,
            Primitive::Line { color, .. } => *color,
        }
    }

    #[test]
    fn a_pip_shows_each_placed_charge() {
        let t = WristDetonatorConfig::new();
        // The disc and its rim, then one pip per charge, up to the most drawn.
        assert_eq!(button(&look(0, None), &t).len(), 2);
        assert_eq!(button(&look(3, None), &t).len(), 5);
        assert_eq!(button(&look(40, None), &t).len(), 2 + MAX_PIPS as usize);
    }

    #[test]
    fn the_button_lights_with_charges_and_flashes_on_events() {
        let t = WristDetonatorConfig::new();
        assert_eq!(colour(&button(&look(0, None), &t)[0]), DIM);
        assert_eq!(colour(&button(&look(2, None), &t)[0]), LIT);
        assert_eq!(
            colour(&button(&look(0, Some(FlashKind::Empty)), &t)[0]),
            EMPTY
        );
        assert_eq!(
            colour(&button(&look(2, Some(FlashKind::Detonated)), &t)[0]),
            DETONATED
        );
    }

    #[test]
    fn the_button_lies_on_the_wrist_facing_out() {
        let t = WristDetonatorConfig::new();
        for prim in button(&look(2, None), &t) {
            let Primitive::Sprite { facing, .. } = prim else {
                panic!("{prim:?}");
            };
            assert_eq!(facing, Facing::Surface { normal: Vec3::X });
        }
    }
}
