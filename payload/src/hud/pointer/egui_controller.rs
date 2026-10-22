//! The egui debug panel's controller pointer: either hand's aim ray against the panel, its trigger
//! as the primary button, its upper face button as the secondary, and its thumbstick as the wheel.
//!
//! The input tick steps it once per tick ([`update_egui_controller`]) from the controller frame
//! with the debug panel chord's presses already masked out, and queues one [`ControllerUpdate`] per
//! tick. The egui pump drains the queue on the same thread before the next egui frame and decides
//! whether the controller or the desktop mouse owns egui's pointer
//! ([`crate::hud::pointer::egui_route`]). The queue's lock is a leaf.
//!
//! The pointing hand starts as the dominant hand. It passes to the other hand when that hand pulls
//! its trigger with its ray on the panel, or when only the other hand's ray is on the panel, but
//! never while the pointing hand holds a press. The press that hands the pointer over does not
//! click: the new hand's trigger must be released first, since the player has not seen where that
//! hand points until its cursor appears.
//!
//! A trigger press is the derived [`Button::TriggerPress`], whose press and release thresholds
//! differ, so a partial pull resting near one threshold does not chatter. The press logic itself is
//! the game-UI pointer's ([`ControllerState`]): a press clicks only after a released sample and only
//! when it begins with the ray on the panel, and a press held across the ray leaving the panel
//! drags along the panel's edge.
//!
//! A hand grabs the panel by pressing its grip with its ray on the panel ([`GrabTracker`]): the panel
//! then follows the hand rigidly, and the hand's thumbstick pushes and pulls it along the aim ray
//! instead of scrolling, until the grip is released. The grip is not a pointer button, so a grab
//! never clicks.
//!
//! A held press stays a click however long it is held, until the hit moves past the click slop
//! ([`ClickSlop`]): until then the pointer stays where the press began, so a hand's tremor never
//! turns a press into a drag. Once the hit leaves the slop, the pointer jumps to it and follows it
//! for the rest of the press, which drags a slider or a scroll area. egui also stops counting a
//! press as a click once it has been held for its click duration, which the panel raises
//! ([`crate::hud::pointer::apply_panel_input_options`]).

use std::{
    cmp::Ordering,
    collections::VecDeque,
    time::{Duration, Instant},
};

use glam::{Vec2, Vec3};
use parking_lot::Mutex;

use crate::{
    config::Config,
    controllers::{
        frame::{Button, ControllerFrame, Edges, Hand, Pose},
        overlay::{
            primitive::{Anchor, DepthMode, HandPose, Primitive, Rgba, Source},
            store,
        },
    },
    hud::{
        egui_panel,
        pointer::{
            controller::{ControllerState, TriggerSample},
            egui_grab::{GrabSample, GrabStep, GrabTracker},
            panel::{PanelHit, aim_at_egui_panel, intersect_panel},
        },
    },
};

/// One input tick of the controller pointer, as the egui pump consumes it.
#[derive(Debug, Copy, Clone, PartialEq)]
pub(crate) struct ControllerUpdate {
    /// The panel UV the pointer is at, or `None` while it is off the panel.
    pub(crate) uv: Option<(f32, f32)>,
    /// Bit zero is the primary button, bit one the secondary.
    pub(crate) buttons: u32,
    /// The wheel motion this tick, in egui points, in egui's wheel convention.
    pub(crate) scroll: Vec2,
}

impl ControllerUpdate {
    /// The controller has stopped pointing: off the panel, nothing held.
    pub(crate) const GONE: Self = Self {
        uv: None,
        buttons: 0,
        scroll: Vec2::ZERO,
    };
}

/// Step the controller pointer from the latest frame and the edges since the last tick, both with
/// the debug panel chord masked, or `None` when no focused frame is available. Runs on the input
/// tick, which owns the controller edges.
pub(crate) fn update_egui_controller(input: Option<(&ControllerFrame, &Edges)>, dominant: Hand) {
    let now = Instant::now();
    let (enabled, slop) = Config::lock_query(|c| {
        let panel = &c.hud.egui_panel;
        (
            panel.controller_pointer,
            ClickSlop {
                radius: panel.click_slop,
                aspect: panel.aspect,
            },
        )
    });
    let samples = input
        .filter(|(frame, _)| enabled && frame.is_fresh_at(now) && egui_panel::is_active())
        .map(|(frame, edges)| Hand::BOTH.map(|hand| sample(frame, edges, hand)));
    let placed = egui_panel::is_placed();
    let mut state = STATE.lock();
    let Some(mut samples) = samples else {
        let grabbed = state.grab.hand().is_some();
        if state.hand.is_some() || grabbed {
            *state = EguiControllerState::new();
            push(ControllerUpdate::GONE);
        }
        drop(state);
        if grabbed {
            egui_panel::end_grab();
        }
        store::clear(Source::PanelPointer);
        return;
    };
    let dt = state.last_tick.map_or(0.0, |last| {
        now.saturating_duration_since(last).as_secs_f32()
    });
    state.last_tick = Some(now);
    let preferred = state.hand.unwrap_or(dominant);
    let grab = state
        .grab
        .step(samples.map(|sample| sample.grab), preferred, placed);
    // The holding hand's stick moves the panel, so it does not scroll.
    let push_rate = grab.holding().map_or(0.0, |hand| {
        let stick = &mut samples[hand.index()].stick;
        let rate = axis_rate(stick.y);
        *stick = Vec2::ZERO;
        rate
    });
    let (hand, update, laser) = state.step(&samples, dominant, slop, dt, now);
    drop(state);
    apply_grab(grab, &samples, push_rate, dt);
    push(update);
    draw_laser(hand, laser);
}

/// Drain the updates queued since the last call, oldest first.
pub(crate) fn take_updates() -> Vec<ControllerUpdate> {
    UPDATES.lock().drain(..).collect()
}

/// Forget the queued updates, for a panel that is not shown. The pointer's own state resets on the
/// next input tick, which finds the panel hidden.
pub(crate) fn reset_egui_controller() {
    UPDATES.lock().clear();
}

/// The deadzone of each thumbstick axis before it scrolls, as a fraction of full deflection.
pub(crate) const SCROLL_DEADZONE: f32 = 0.2;

/// The scroll speed at full deflection, egui points per second.
pub(crate) const SCROLL_SPEED: f32 = 1200.0;

/// The longest interval one tick's scroll covers, seconds, so a hitch does not jump the view.
pub(crate) const MAX_SCROLL_DT: f32 = 0.1;

/// The wheel motion for a thumbstick deflection held for `dt` seconds: each axis scrolls past
/// [`SCROLL_DEADZONE`] at a speed proportional to its deflection beyond it, up to [`SCROLL_SPEED`].
/// Pushing the stick up or right reveals what lies above or to the right, so the stick's Y keeps
/// its sign and its X flips in egui's content-motion convention.
pub(crate) fn scroll_delta(stick: Vec2, dt: f32) -> Vec2 {
    if !dt.is_finite() || dt <= 0.0 {
        return Vec2::ZERO;
    }
    let dt = dt.min(MAX_SCROLL_DT);
    Vec2::new(-axis_rate(stick.x), axis_rate(stick.y)) * SCROLL_SPEED * dt
}

/// How far a held press's hit may wander from where the press began and still click.
#[derive(Debug, Copy, Clone, PartialEq)]
pub(crate) struct ClickSlop {
    /// The radius, as a fraction of the panel's height.
    pub(crate) radius: f32,
    /// The panel's aspect ratio (width / height), which turns a UV distance into panel heights.
    pub(crate) aspect: f32,
}

impl ClickSlop {
    /// Whether `uv` lies outside the slop around `origin`.
    fn exceeded(self, origin: (f32, f32), uv: (f32, f32)) -> bool {
        let aspect = if self.aspect.is_finite() && self.aspect > 0.0 {
            self.aspect
        } else {
            1.0
        };
        let offset = Vec2::new((uv.0 - origin.0) * aspect, uv.1 - origin.1);
        // A slop that is not a positive number never holds the pointer.
        !matches!(
            offset.length().partial_cmp(&self.radius),
            Some(Ordering::Less | Ordering::Equal)
        )
    }
}

/// What one hand offers the pointer on one tick.
#[derive(Debug, Copy, Clone, PartialEq, Default)]
struct HandSample {
    hit: Option<PanelHit>,
    trigger: TriggerSample,
    secondary: TriggerSample,
    stick: Vec2,
    /// The hand's cockpit-frame aim pose, while its poses are usable.
    aim: Option<Pose>,
    grab: GrabSample,
}

impl HandSample {
    fn on_panel(&self) -> bool {
        self.hit.is_some_and(|hit| hit.inside)
    }
}

/// The queued updates beyond which the oldest are dropped, so a pump that stops draining (the
/// panel hidden between the gate and the drain) cannot grow the queue.
const MAX_QUEUED: usize = 16;

static UPDATES: Mutex<VecDeque<ControllerUpdate>> = Mutex::new(VecDeque::new());

static STATE: Mutex<EguiControllerState> = Mutex::new(EguiControllerState::new());

/// Carry the tick's grab to the panel's placement: take hold with the hand's aim pose, move the
/// held panel with the hand and push it along the ray at `push_rate` for `dt` seconds, or let go.
fn apply_grab(grab: GrabStep, samples: &[HandSample; 2], push_rate: f32, dt: f32) {
    match grab {
        GrabStep::Idle => {}
        GrabStep::Begin(hand) => {
            if let Some(aim) = samples[hand.index()].aim {
                egui_panel::begin_grab(hand, aim);
            }
        }
        GrabStep::Hold(hand) => {
            egui_panel::hold_grab(hand, samples[hand.index()].aim, push_rate, dt);
        }
        GrabStep::Release(_) => egui_panel::end_grab(),
    }
}

fn push(update: ControllerUpdate) {
    let mut updates = UPDATES.lock();
    if updates.len() >= MAX_QUEUED {
        updates.pop_front();
    }
    updates.push_back(update);
}

fn sample(frame: &ControllerFrame, edges: &Edges, hand: Hand) -> HandSample {
    let side = frame.hand(hand);
    let hit = usable(frame, hand)
        .then(|| aim_at_egui_panel(side.aim.pose))
        .flatten()
        .and_then(|(ray, corners)| intersect_panel(ray, &corners));
    read_hand(frame, edges, hand, hit)
}

fn usable(frame: &ControllerFrame, hand: Hand) -> bool {
    let side = frame.hand(hand);
    side.active && side.grip.is_valid() && side.aim.is_valid()
}

/// A hand's buttons and stick from `frame` and `edges`, with its ray's `hit` on the panel.
fn read_hand(
    frame: &ControllerFrame,
    edges: &Edges,
    hand: Hand,
    hit: Option<PanelHit>,
) -> HandSample {
    let side = frame.hand(hand);
    let button = |button: Button| TriggerSample {
        level: side.buttons.contains(button),
        pressed: edges.pressed(hand, button),
        released: edges.released(hand, button),
    };
    let usable = usable(frame, hand);
    let hit_inside = hit.is_some_and(|hit| hit.inside);
    HandSample {
        hit,
        trigger: button(Button::TriggerPress),
        secondary: button(Button::BClick),
        stick: if usable {
            side.inputs.thumbstick
        } else {
            Vec2::ZERO
        },
        aim: usable.then_some(side.aim.pose),
        grab: GrabSample {
            grip: button(Button::SqueezePress),
            on_panel: hit_inside,
            usable,
        },
    }
}

fn axis_rate(value: f32) -> f32 {
    if !value.is_finite() {
        return 0.0;
    }
    let magnitude = value.abs().min(1.0);
    if magnitude <= SCROLL_DEADZONE {
        0.0
    } else {
        value.signum() * (magnitude - SCROLL_DEADZONE) / (1.0 - SCROLL_DEADZONE)
    }
}

/// Draw the pointing hand's laser from its aim pose to the panel, `distance` along its ray, or
/// clear it. Both ends are anchored to the hand, so the laser follows the controller at frame rate
/// between input ticks.
fn draw_laser(hand: Hand, distance: Option<f32>) {
    let Some(distance) = distance.filter(|d| d.is_finite() && *d > 0.0) else {
        store::clear(Source::PanelPointer);
        return;
    };
    let mut prims = vec![Primitive::Line {
        from: Anchor::hand(hand, HandPose::Aim),
        to: Anchor::Hand {
            hand,
            pose: HandPose::Aim,
            offset: Vec3::new(0.0, 0.0, -distance),
        },
        width: LASER_WIDTH_M,
        color: LASER_COLOR,
        depth: DepthMode::XRay {
            occluded_alpha: 0.25,
        },
    }];
    store::submit(Source::PanelPointer, &mut prims, LASER_TTL);
}

const LASER_WIDTH_M: f32 = 0.004;

const LASER_COLOR: Rgba = [1.0, 1.0, 1.0, 0.6];

/// The laser's time to live: a few input ticks, so it disappears on its own when they stop.
const LASER_TTL: Duration = Duration::from_millis(150);

/// The pointing hand and its press state.
struct EguiControllerState {
    /// The pointing hand, `None` until the pointer has run since it was last reset.
    hand: Option<Hand>,
    press: ControllerState,
    secondary: SecondaryButton,
    /// Where the held primary press began, and whether its hit has left the slop since.
    press_origin: Option<PressOrigin>,
    /// Each hand's trigger as the pointer saw it on the previous tick, in [`Hand::index`] order.
    triggers: [bool; 2],
    last_tick: Option<Instant>,
    grab: GrabTracker,
}

/// Where a held primary press began.
#[derive(Debug, Copy, Clone, PartialEq)]
struct PressOrigin {
    uv: (f32, f32),
    /// The hit has left the slop: the pointer follows it.
    dragging: bool,
}

impl EguiControllerState {
    const fn new() -> Self {
        Self {
            hand: None,
            press: ControllerState::new(),
            secondary: SecondaryButton::after(false),
            press_origin: None,
            triggers: [false; 2],
            last_tick: None,
            grab: GrabTracker::new(),
        }
    }

    /// Advance by one tick. Returns the pointing hand, the update for egui, and the laser's length
    /// while the hand's ray is on the panel or drags a press along it.
    fn step(
        &mut self,
        samples: &[HandSample; 2],
        dominant: Hand,
        slop: ClickSlop,
        dt: f32,
        now: Instant,
    ) -> (Hand, ControllerUpdate, Option<f32>) {
        let downs = Hand::BOTH.map(|h| samples[h.index()].trigger.down(self.triggers[h.index()]));
        let pulled = Hand::BOTH.map(|h| downs[h.index()] && !self.triggers[h.index()]);
        let current = self.hand.unwrap_or(dominant);
        let on_panel = samples.each_ref().map(HandSample::on_panel);
        let hand = choose_hand(current, on_panel, pulled, self.press.captured());
        if self.hand != Some(hand) {
            self.press = ControllerState::after(self.triggers[hand.index()]);
            self.secondary = SecondaryButton::after(samples[hand.index()].secondary.level);
            self.press_origin = None;
            self.hand = Some(hand);
        }
        self.triggers = downs;

        let sample = samples[hand.index()];
        let pointer = self
            .press
            .step(sample.hit, downs[hand.index()], hand, now)
            .filter(|pointer| pointer.owns_ui);
        let primary = pointer.map_or(0, |pointer| pointer.buttons & 1);
        let secondary = self
            .secondary
            .step(sample.secondary, sample.on_panel() && primary == 0);
        let uv = match pointer {
            Some(pointer) => Some(self.hold_click(pointer.uv, primary != 0, slop)),
            None => {
                self.press_origin = None;
                None
            }
        };
        let update = ControllerUpdate {
            uv,
            buttons: primary | (u32::from(secondary && uv.is_some()) << 1),
            scroll: if uv.is_some() {
                scroll_delta(sample.stick, dt)
            } else {
                Vec2::ZERO
            },
        };
        let laser = sample.hit.filter(|_| uv.is_some()).map(|hit| hit.distance);
        (hand, update, laser)
    }
}

impl EguiControllerState {
    /// The pointer position for a hit at `uv` with the primary button `held`: where the press
    /// began, until the hit leaves the slop around it, and the hit itself otherwise. The release
    /// is still reported where the press began, so egui sees no motion between press and release.
    fn hold_click(&mut self, uv: (f32, f32), held: bool, slop: ClickSlop) -> (f32, f32) {
        if !held {
            return match self.press_origin.take() {
                Some(origin) if !origin.dragging => origin.uv,
                _ => uv,
            };
        }
        let origin = self.press_origin.get_or_insert(PressOrigin {
            uv,
            dragging: false,
        });
        origin.dragging |= slop.exceeded(origin.uv, uv);
        if origin.dragging { uv } else { origin.uv }
    }
}

/// The pointing hand after a tick. `on_panel` is whether each hand's ray is on the panel, `pulled`
/// whether each hand's trigger was pulled this tick, and `holding` whether the current hand holds a
/// press, which keeps the pointer with it.
fn choose_hand(current: Hand, on_panel: [bool; 2], pulled: [bool; 2], holding: bool) -> Hand {
    if holding {
        return current;
    }
    let (cur, other) = (current.index(), current.other().index());
    let claims = |i: usize| pulled[i] && on_panel[i];
    if claims(other) && !claims(cur) {
        return current.other();
    }
    if on_panel[other] && !on_panel[cur] {
        return current.other();
    }
    current
}

/// The secondary button: down while the button is held, if its press began with the ray on the
/// panel and no primary press held.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
struct SecondaryButton {
    /// The button as the pointer saw it on the previous tick.
    raw: bool,
    held: bool,
}

impl SecondaryButton {
    const fn after(raw: bool) -> Self {
        Self { raw, held: false }
    }

    fn step(&mut self, sample: TriggerSample, can_press: bool) -> bool {
        let down = sample.down(self.raw);
        let began = down && !self.raw;
        self.held = down && (self.held || (began && can_press));
        self.raw = down;
        self.held
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::controllers::{
        bindings::panel_chord::{self, ChordInput, PanelChord},
        frame::ActionSetKind,
    };

    const INSIDE: Option<PanelHit> = Some(PanelHit {
        uv: (0.25, 0.75),
        inside: true,
        distance: 2.0,
    });

    const SLOP: ClickSlop = ClickSlop {
        radius: 0.03,
        aspect: 4.0 / 3.0,
    };

    const OTHER_INSIDE: Option<PanelHit> = Some(PanelHit {
        uv: (0.8, 0.1),
        inside: true,
        distance: 2.5,
    });

    fn level(down: bool) -> TriggerSample {
        TriggerSample {
            level: down,
            pressed: false,
            released: false,
        }
    }

    fn hand(hit: Option<PanelHit>, trigger: bool) -> HandSample {
        HandSample {
            hit,
            trigger: level(trigger),
            ..Default::default()
        }
    }

    fn step(
        state: &mut EguiControllerState,
        left: HandSample,
        right: HandSample,
    ) -> (Hand, ControllerUpdate) {
        let (hand, update, _) = state.step(&[left, right], Hand::Right, SLOP, 0.0, Instant::now());
        (hand, update)
    }

    #[test]
    fn the_dominant_hand_points_first_and_clicks_after_a_release() {
        let mut state = EguiControllerState::new();
        let (h, u) = step(&mut state, hand(None, false), hand(INSIDE, false));
        assert_eq!((h, u.uv, u.buttons), (Hand::Right, Some((0.25, 0.75)), 0));
        let (_, u) = step(&mut state, hand(None, false), hand(INSIDE, true));
        assert_eq!(u.buttons, 1);
        let (_, u) = step(&mut state, hand(None, false), hand(INSIDE, false));
        assert_eq!(u.buttons, 0);
    }

    #[test]
    fn a_trigger_held_as_the_pointer_starts_does_not_click() {
        let mut state = EguiControllerState::new();
        let (_, u) = step(&mut state, hand(None, false), hand(INSIDE, true));
        assert_eq!(u.buttons, 0);
        let (_, u) = step(&mut state, hand(None, false), hand(INSIDE, true));
        assert_eq!(u.buttons, 0);
        step(&mut state, hand(None, false), hand(INSIDE, false));
        let (_, u) = step(&mut state, hand(None, false), hand(INSIDE, true));
        assert_eq!(u.buttons, 1);
    }

    #[test]
    fn the_other_hands_pull_takes_the_pointer_without_clicking() {
        let mut state = EguiControllerState::new();
        step(&mut state, hand(OTHER_INSIDE, false), hand(INSIDE, false));
        let (h, u) = step(&mut state, hand(OTHER_INSIDE, true), hand(INSIDE, false));
        assert_eq!((h, u.uv, u.buttons), (Hand::Left, Some((0.8, 0.1)), 0));
        step(&mut state, hand(OTHER_INSIDE, false), hand(INSIDE, false));
        let (h, u) = step(&mut state, hand(OTHER_INSIDE, true), hand(INSIDE, false));
        assert_eq!((h, u.buttons), (Hand::Left, 1));
    }

    #[test]
    fn the_pointer_follows_the_only_hand_on_the_panel() {
        let mut state = EguiControllerState::new();
        step(&mut state, hand(None, false), hand(INSIDE, false));
        let (h, _) = step(&mut state, hand(OTHER_INSIDE, false), hand(None, false));
        assert_eq!(h, Hand::Left);
        let (h, _) = step(&mut state, hand(OTHER_INSIDE, false), hand(INSIDE, false));
        assert_eq!(h, Hand::Left, "with both on the panel the pointer stays");
    }

    #[test]
    fn a_held_press_keeps_the_pointer() {
        let mut state = EguiControllerState::new();
        step(&mut state, hand(None, false), hand(INSIDE, false));
        step(&mut state, hand(None, false), hand(INSIDE, true));
        let (h, u) = step(&mut state, hand(OTHER_INSIDE, true), hand(INSIDE, true));
        assert_eq!((h, u.buttons), (Hand::Right, 1));
    }

    fn at(u: f32, v: f32, trigger: bool) -> HandSample {
        hand(
            Some(PanelHit {
                uv: (u, v),
                inside: true,
                distance: 2.0,
            }),
            trigger,
        )
    }

    #[test]
    fn a_held_press_stays_at_its_origin_within_the_slop() {
        let mut state = EguiControllerState::new();
        step(&mut state, hand(None, false), at(0.5, 0.5, false));
        let (_, u) = step(&mut state, hand(None, false), at(0.5, 0.5, true));
        assert_eq!((u.uv, u.buttons), (Some((0.5, 0.5)), 1));
        // A tremor within the slop: the pointer stays put, however long the press is held.
        for (du, dv) in [(0.01, 0.0), (-0.015, 0.02), (0.02, -0.01), (0.0, 0.025)] {
            let (_, u) = step(&mut state, hand(None, false), at(0.5 + du, 0.5 + dv, true));
            assert_eq!((u.uv, u.buttons), (Some((0.5, 0.5)), 1));
        }
        // The release lands where the press began, so egui sees a click, and the pointer follows
        // the hit again from the next tick.
        let (_, u) = step(&mut state, hand(None, false), at(0.51, 0.5, false));
        assert_eq!((u.uv, u.buttons), (Some((0.5, 0.5)), 0));
        let (_, u) = step(&mut state, hand(None, false), at(0.51, 0.5, false));
        assert_eq!(u.uv, Some((0.51, 0.5)));
    }

    #[test]
    fn a_press_that_leaves_the_slop_drags_from_the_live_hit() {
        let mut state = EguiControllerState::new();
        step(&mut state, hand(None, false), at(0.5, 0.5, false));
        step(&mut state, hand(None, false), at(0.5, 0.5, true));
        let (_, u) = step(&mut state, hand(None, false), at(0.5, 0.52, true));
        assert_eq!(u.uv, Some((0.5, 0.5)));
        // Horizontal UV counts by the panel's aspect: 0.025 of the width is 0.033 heights.
        let (_, u) = step(&mut state, hand(None, false), at(0.525, 0.5, true));
        assert_eq!(u.uv, Some((0.525, 0.5)));
        // Having left the slop, the pointer follows every small move, back toward the origin too.
        let (_, u) = step(&mut state, hand(None, false), at(0.51, 0.5, true));
        assert_eq!((u.uv, u.buttons), (Some((0.51, 0.5)), 1));
        step(&mut state, hand(None, false), at(0.51, 0.5, false));
        // The next press starts a fresh slop.
        step(&mut state, hand(None, false), at(0.2, 0.2, true));
        let (_, u) = step(&mut state, hand(None, false), at(0.21, 0.2, true));
        assert_eq!(u.uv, Some((0.2, 0.2)));
    }

    #[test]
    fn a_bad_slop_never_holds_the_pointer() {
        for radius in [0.0, -1.0, f32::NAN] {
            let slop = ClickSlop {
                radius,
                aspect: f32::NAN,
            };
            assert!(slop.exceeded((0.5, 0.5), (0.501, 0.5)), "{radius}");
        }
    }

    #[test]
    fn a_tap_between_ticks_clicks_once() {
        let mut state = EguiControllerState::new();
        step(&mut state, hand(None, false), hand(INSIDE, false));
        let tap = HandSample {
            hit: INSIDE,
            trigger: TriggerSample {
                level: false,
                pressed: true,
                released: true,
            },
            ..Default::default()
        };
        let (_, u) = step(&mut state, hand(None, false), tap);
        assert_eq!(u.buttons, 1);
        let (_, u) = step(&mut state, hand(None, false), hand(INSIDE, false));
        assert_eq!(u.buttons, 0);
    }

    #[test]
    fn the_secondary_button_presses_only_on_the_panel() {
        let mut state = EguiControllerState::new();
        let with_b = |hit, b| HandSample {
            hit,
            secondary: level(b),
            ..Default::default()
        };
        step(&mut state, hand(None, false), with_b(INSIDE, false));
        let (_, u) = step(&mut state, hand(None, false), with_b(INSIDE, true));
        assert_eq!(u.buttons, 2);
        step(&mut state, hand(None, false), with_b(INSIDE, false));
        let outside = Some(PanelHit {
            uv: (1.0, 0.5),
            inside: false,
            distance: 2.0,
        });
        step(&mut state, hand(None, false), with_b(outside, true));
        let (_, u) = step(&mut state, hand(None, false), with_b(INSIDE, true));
        assert_eq!(u.buttons, 0, "a press begun off the panel is not a click");
    }

    #[test]
    fn the_stick_scrolls_only_while_pointing() {
        let mut state = EguiControllerState::new();
        let stick = |hit| HandSample {
            hit,
            stick: Vec2::new(0.0, 1.0),
            ..Default::default()
        };
        let (_, _, _) = state.step(
            &[hand(None, false), stick(INSIDE)],
            Hand::Right,
            SLOP,
            0.05,
            Instant::now(),
        );
        let (_, u, _) = state.step(
            &[hand(None, false), stick(INSIDE)],
            Hand::Right,
            SLOP,
            0.05,
            Instant::now(),
        );
        assert!(u.scroll.y > 0.0);
        let (_, u, _) = state.step(
            &[hand(None, false), stick(None)],
            Hand::Right,
            SLOP,
            0.05,
            Instant::now(),
        );
        assert_eq!(u.scroll, Vec2::ZERO);
    }

    #[test]
    fn the_laser_reaches_the_panel_only_while_pointing() {
        let mut state = EguiControllerState::new();
        let (_, _, laser) = state.step(
            &[hand(None, false), hand(INSIDE, false)],
            Hand::Right,
            SLOP,
            0.0,
            Instant::now(),
        );
        assert_eq!(laser, Some(2.0));
        let (_, _, laser) = state.step(
            &[hand(None, false), hand(None, false)],
            Hand::Right,
            SLOP,
            0.0,
            Instant::now(),
        );
        assert_eq!(laser, None);
    }

    #[test]
    fn scroll_has_a_deadzone_and_grows_with_deflection() {
        assert_eq!(scroll_delta(Vec2::new(0.15, -0.2), 0.1), Vec2::ZERO);
        let half = scroll_delta(Vec2::new(0.0, 0.6), 0.1);
        let full = scroll_delta(Vec2::new(0.0, 1.0), 0.1);
        assert!(half.y > 0.0 && full.y > half.y);
        assert!((full.y - SCROLL_SPEED * 0.1).abs() < 1e-3);
        assert!((half.y - full.y * 0.5).abs() < 1e-3);
        assert!(scroll_delta(Vec2::new(1.0, 0.0), 0.1).x < 0.0);
    }

    #[test]
    fn scroll_ignores_bad_input_and_caps_a_hitch() {
        assert_eq!(scroll_delta(Vec2::new(f32::NAN, 1.0), 0.0), Vec2::ZERO);
        assert_eq!(scroll_delta(Vec2::new(0.0, 1.0), f32::NAN), Vec2::ZERO);
        assert_eq!(scroll_delta(Vec2::new(f32::NAN, 0.0), 0.1), Vec2::ZERO);
        assert_eq!(
            scroll_delta(Vec2::new(0.0, 1.0), 5.0),
            scroll_delta(Vec2::new(0.0, 1.0), MAX_SCROLL_DT)
        );
    }

    /// The left stick click held and the right B pressed, held, and let go, with both rays on the
    /// panel throughout, and then a plain press of the right B.
    fn chord_then_right_click(click_held: bool) -> Vec<u32> {
        let mut chord = PanelChord::new();
        let mut state = EguiControllerState::new();
        let mut previous = (false, false);
        let ticks = [
            (click_held, false),
            (click_held, true),
            (click_held, true),
            (click_held, false),
            (false, false),
            (false, true),
            (false, false),
        ];
        let mut buttons = Vec::new();
        for (click, b) in ticks {
            let mut frame = ControllerFrame {
                sequence: 0,
                display_time_ns: 0,
                published_at: Instant::now(),
                focused: true,
                action_set: ActionSetKind::Ui,
                hands: Default::default(),
            };
            let mut edges = Edges::default();
            let (left, right) = (panel_chord::CLICK_HAND.index(), panel_chord::B_HAND.index());
            frame.hands[left]
                .buttons
                .set(Button::ThumbstickClick, click);
            frame.hands[right].buttons.set(Button::BClick, b);
            edges.pressed[left].set(Button::ThumbstickClick, click && !previous.0);
            edges.released[left].set(Button::ThumbstickClick, !click && previous.0);
            edges.pressed[right].set(Button::BClick, b && !previous.1);
            edges.released[right].set(Button::BClick, !b && previous.1);
            previous = (click, b);

            let tick = chord.step(ChordInput::of(&frame, &edges), 0.05, 0.35);
            panel_chord::mask(&mut frame, &mut edges, tick);
            let samples = Hand::BOTH.map(|hand| read_hand(&frame, &edges, hand, INSIDE));
            let (_, update, _) = state.step(&samples, Hand::Right, SLOP, 0.0, Instant::now());
            buttons.push(update.buttons);
        }
        buttons
    }

    #[test]
    fn the_chord_b_never_right_clicks_on_the_panel() {
        assert_eq!(chord_then_right_click(true), [0, 0, 0, 0, 0, 2, 0]);
        // Without the stick click the same press right-clicks, so the mask is what withholds it.
        assert_eq!(chord_then_right_click(false), [0, 2, 2, 0, 0, 2, 0]);
    }
}
