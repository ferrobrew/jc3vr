//! The native map's controller-side state machine: which hand owns the map, the button edges the
//! map sees, where the map's cursor sits during a press, and the zoom steps from the off-hand
//! stick. Pure; the hooks in the parent module feed it one sample per map input dispatch.

use std::time::{Duration, Instant};

use glam::Vec2;

use crate::{
    controllers::frame::{Hand, MAX_FRAME_AGE},
    hud::pointer::RoutedPointer,
};

/// A controller sample that is eligible for native map input.
#[derive(Debug, Copy, Clone, PartialEq)]
pub(crate) struct ControllerSample {
    /// The controller that owns this map interaction.
    pub(crate) hand: Hand,
    /// Absolute panel UV, `(0, 0)` at the top left.
    pub(crate) uv: Vec2,
    /// The map's primary button, the pointer's trigger.
    pub(crate) primary: bool,
    /// The map's secondary button. The controller pointer has no secondary binding yet.
    pub(crate) secondary: bool,
    /// The off-hand stick's vertical axis, positive to zoom in. Raw: the zoom stepper applies its
    /// own thresholds.
    pub(crate) zoom_axis: f32,
}

/// One zoom step for a dispatch.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub(crate) enum ZoomStep {
    None,
    In,
    Out,
}

/// The values to deliver to the native map for one dispatch.
#[derive(Debug, Copy, Clone, PartialEq)]
pub(crate) struct MapDelivery {
    /// Absolute panel UV for the map cursor. On cancellation this is the last delivered UV. While a
    /// press is held at its press point (see [`held_at_press`](Self::held_at_press)), this is the
    /// press point rather than the ray.
    pub(crate) uv: Vec2,
    /// Whether the cursor is held at the primary press point: the press has not wandered past
    /// [`CLICK_SLOP`], or this is the release of such a press.
    pub(crate) held_at_press: bool,
    /// The primary button level for this dispatch.
    pub(crate) primary: bool,
    /// The secondary button level for this dispatch.
    pub(crate) secondary: bool,
    /// The zoom step for this dispatch; none during source transitions and cancellation.
    pub(crate) zoom: ZoomStep,
    /// Whether this dispatch ends the controller's ownership rather than continuing it.
    pub(crate) cancelled: bool,
    /// Whether the primary button began in this dispatch.
    pub(crate) primary_pressed: bool,
    /// Whether the primary button ended in this dispatch.
    pub(crate) primary_released: bool,
    /// Whether the secondary button began in this dispatch.
    pub(crate) secondary_pressed: bool,
    /// Whether the secondary button ended in this dispatch.
    pub(crate) secondary_released: bool,
}

impl MapDelivery {
    /// Fit a cancellation to the map's own state. The map treats a released primary button outside
    /// a drag as a selection and a released secondary button as the context action, so a cancelled
    /// press releases the primary button only to end a drag in progress, and never releases the
    /// secondary one.
    pub(crate) fn for_map(self, dragging: bool) -> Self {
        if !self.cancelled {
            return self;
        }
        Self {
            primary_released: self.primary_released && dragging,
            secondary_released: false,
            ..self
        }
    }

    const fn neutral(
        uv: Vec2,
        cancelled: bool,
        primary_released: bool,
        secondary_released: bool,
    ) -> Self {
        Self {
            uv,
            held_at_press: false,
            primary: false,
            secondary: false,
            zoom: ZoomStep::None,
            cancelled,
            primary_pressed: false,
            primary_released,
            secondary_pressed: false,
            secondary_released,
        }
    }
}

/// The native map's controller-side state machine.
#[derive(Debug, Copy, Clone, PartialEq)]
pub(crate) struct MapController {
    owner: Option<Hand>,
    /// A controller press is accepted only after a released sample owned the map. This prevents a
    /// held trigger from becoming a click when tracking or the owning hand changes.
    armed: bool,
    /// Whether the previous delivery held the primary button.
    primary_held: bool,
    /// Whether the previous delivery held the secondary button.
    secondary_held: bool,
    /// The last delivered cursor, once there has been one.
    last_uv: Option<Vec2>,
    /// Where the held primary press began, while the ray has stayed within [`CLICK_SLOP`] of it.
    press_anchor: Option<Vec2>,
    zoom: ZoomRepeat,
    /// When the map last received a delivery or a cancellation.
    updated: Option<Instant>,
}

impl MapController {
    /// Create an inactive controller state.
    pub(crate) const fn new() -> Self {
        Self {
            owner: None,
            armed: false,
            primary_held: false,
            secondary_held: false,
            last_uv: None,
            press_anchor: None,
            zoom: ZoomRepeat::new(),
            updated: None,
        }
    }

    /// Deliver one controller sample, or a neutral cancellation when the sample is absent or
    /// invalid. A cancellation is emitted exactly once for an active owner; the next absent sample
    /// returns `None`. State older than [`MAX_FRAME_AGE`] belongs to a map dispatch that stopped
    /// (the map closed), and is dropped silently.
    pub(crate) fn step(
        &mut self,
        sample: Option<ControllerSample>,
        now: Instant,
    ) -> Option<MapDelivery> {
        self.expire(now);
        self.updated = Some(now);
        let Some(sample) = sample.filter(|sample| sample.uv.is_finite()) else {
            return self.cancel();
        };
        let uv = sample.uv.clamp(Vec2::ZERO, Vec2::ONE);
        if self.owner != Some(sample.hand) {
            let previous = self.owner.and(self.last_uv);
            let primary_released = self.primary_held;
            let secondary_released = self.secondary_held;
            *self = Self {
                owner: Some(sample.hand),
                armed: !sample.primary && !sample.secondary,
                last_uv: Some(uv),
                updated: Some(now),
                ..Self::new()
            };
            return Some(MapDelivery::neutral(
                previous.unwrap_or(uv),
                previous.is_some(),
                primary_released,
                secondary_released,
            ));
        }

        if !sample.primary && !sample.secondary {
            self.armed = true;
        }
        let accepted = self.armed;
        let primary = accepted && sample.primary;
        let secondary = accepted && sample.secondary;
        let primary_pressed = primary && !self.primary_held;
        let primary_released = !primary && self.primary_held;
        let anchor = press_anchor(
            self.press_anchor,
            uv,
            primary,
            primary_pressed,
            primary_released,
        );
        self.press_anchor = anchor.filter(|_| primary);
        let uv = anchor.unwrap_or(uv);
        self.last_uv = Some(uv);
        let delivery = MapDelivery {
            uv,
            held_at_press: anchor.is_some(),
            primary,
            secondary,
            zoom: if accepted {
                self.zoom.step(sample.zoom_axis, now)
            } else {
                self.zoom = ZoomRepeat::new();
                ZoomStep::None
            },
            cancelled: false,
            primary_pressed,
            primary_released,
            secondary_pressed: secondary && !self.secondary_held,
            secondary_released: !secondary && self.secondary_held,
        };
        self.primary_held = primary;
        self.secondary_held = secondary;
        Some(delivery)
    }

    /// Cancel the active controller owner and return the one neutral delivery that tells the map
    /// its buttons were released. A later cancellation produces no delivery.
    pub(crate) fn cancel(&mut self) -> Option<MapDelivery> {
        self.owner?;
        let delivery = MapDelivery::neutral(
            self.last_uv.unwrap_or(Vec2::ZERO),
            true,
            self.primary_held,
            self.secondary_held,
        );
        *self = Self {
            last_uv: self.last_uv,
            updated: self.updated,
            ..Self::new()
        };
        Some(delivery)
    }

    /// Where the map cursor rests while no source owns the pointer: the last controller cursor.
    pub(crate) fn resting_uv(&self) -> Option<Vec2> {
        self.last_uv
    }

    fn expire(&mut self, now: Instant) {
        if self
            .updated
            .is_some_and(|time| now.saturating_duration_since(time) > MAX_FRAME_AGE)
        {
            *self = Self::new();
        }
    }
}

/// Advance map state after the pointer router's decision. A controller route steps the owner; any
/// other route, including the mod not routing the mouse at all, cancels it.
pub(crate) fn prepare_delivery(
    controller: &mut MapController,
    sample: Option<ControllerSample>,
    routed: Option<RoutedPointer>,
    now: Instant,
) -> Option<MapDelivery> {
    if routed.is_some_and(RoutedPointer::is_controller) {
        controller.step(sample, now)
    } else {
        controller.expire(now);
        controller.cancel()
    }
}

/// How far, in panel UV, the ray may wander from a primary press before the map sees the cursor
/// move. The map turns a held press into a drag once its cursor leaves the press point by a few
/// stage pixels, and a hand-held ray moves further than that from tremor and from the trigger pull
/// itself, so without the slop nearly every trigger pull would start a drag, and its release would
/// end the drag instead of selecting.
pub(crate) const CLICK_SLOP: f32 = 0.025;

/// The press point to hold the cursor at for one delivery, if any. A press anchors at its own UV;
/// a held press keeps the anchor while the ray stays within [`CLICK_SLOP`] of it and loses it for
/// good once the ray leaves, which hands the map the live ray and lets it start a drag; a release
/// is delivered at the anchor it still has, so a click selects where it was pressed even when
/// letting go of the trigger jerks the ray.
fn press_anchor(
    anchor: Option<Vec2>,
    uv: Vec2,
    primary: bool,
    primary_pressed: bool,
    primary_released: bool,
) -> Option<Vec2> {
    if primary_pressed {
        Some(uv)
    } else if primary {
        anchor.filter(|anchor| anchor.distance(uv) <= CLICK_SLOP)
    } else if primary_released {
        anchor
    } else {
        None
    }
}

/// How far the stick must be pushed to start zooming.
const ZOOM_ENGAGE: f32 = 0.6;
/// How far the stick must return before a new push counts; the gap to [`ZOOM_ENGAGE`] is the
/// hysteresis that keeps a stick resting near the threshold from chattering.
const ZOOM_RELEASE: f32 = 0.4;
/// How long a held push waits before it repeats.
const ZOOM_REPEAT_DELAY: Duration = Duration::from_millis(450);
/// The interval between repeated steps while the push is held.
const ZOOM_REPEAT_INTERVAL: Duration = Duration::from_millis(200);

/// Turns a stick axis into discrete zoom steps: one per push, then repeating at a fixed rate while
/// the push is held. The map moves one snapped zoom level per clicked zoom action, so an axis fed
/// through every dispatch would run through all the levels in a few frames, and stick drift would
/// pin the map at a limit.
#[derive(Debug, Copy, Clone, PartialEq)]
struct ZoomRepeat {
    held: ZoomStep,
    next: Option<Instant>,
}

impl ZoomRepeat {
    const fn new() -> Self {
        Self {
            held: ZoomStep::None,
            next: None,
        }
    }

    fn step(&mut self, axis: f32, now: Instant) -> ZoomStep {
        let axis = if axis.is_finite() { axis } else { 0.0 };
        let direction = match self.held {
            ZoomStep::In if axis > ZOOM_RELEASE => ZoomStep::In,
            ZoomStep::Out if axis < -ZOOM_RELEASE => ZoomStep::Out,
            _ if axis >= ZOOM_ENGAGE => ZoomStep::In,
            _ if axis <= -ZOOM_ENGAGE => ZoomStep::Out,
            _ => ZoomStep::None,
        };
        if direction == ZoomStep::None {
            *self = Self::new();
            return ZoomStep::None;
        }
        if direction != self.held {
            self.held = direction;
            self.next = Some(now + ZOOM_REPEAT_DELAY);
            return direction;
        }
        match self.next {
            Some(next) if now >= next => {
                self.next = Some(now + ZOOM_REPEAT_INTERVAL);
                direction
            }
            _ => ZoomStep::None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const UV: Vec2 = Vec2::new(0.25, 0.5);

    fn sample(hand: Hand, uv: Vec2, primary: bool, zoom_axis: f32) -> Option<ControllerSample> {
        Some(ControllerSample {
            hand,
            uv,
            primary,
            secondary: false,
            zoom_axis,
        })
    }

    const CONTROLLER_ROUTE: Option<RoutedPointer> = Some(RoutedPointer::Controller {
        uv: (0.25, 0.5),
        buttons: 0,
    });

    #[test]
    fn first_owner_sample_is_neutral_and_requires_release() {
        let now = Instant::now();
        let mut map = MapController::new();
        assert_eq!(
            map.step(sample(Hand::Right, UV, true, 1.0), now),
            Some(MapDelivery::neutral(UV, false, false, false))
        );
        let held = map.step(sample(Hand::Right, UV, true, 1.0), now).unwrap();
        assert!(!held.primary);
        assert_eq!(held.zoom, ZoomStep::None);
        let released = map.step(sample(Hand::Right, UV, false, 0.0), now).unwrap();
        assert!(!released.primary);
        let pressed = map.step(sample(Hand::Right, UV, true, 0.0), now).unwrap();
        assert!(pressed.primary);
        assert!(pressed.primary_pressed);
    }

    #[test]
    fn hand_change_cancels_before_accepting_the_new_owner() {
        let now = Instant::now();
        let mut map = MapController::new();
        map.step(sample(Hand::Right, UV, false, 0.0), now);
        assert!(
            map.step(sample(Hand::Right, UV, true, 0.0), now)
                .unwrap()
                .primary
        );
        let other = Vec2::new(0.8, 0.2);
        let cancel = map.step(sample(Hand::Left, other, true, 1.0), now).unwrap();
        assert!(cancel.cancelled);
        assert!(cancel.primary_released);
        assert!(!cancel.secondary_released);
        assert_eq!(cancel.uv, UV);
        assert!(!cancel.primary);
        assert!(
            !map.step(sample(Hand::Left, other, true, 1.0), now)
                .unwrap()
                .primary
        );
    }

    #[test]
    fn cancellation_releases_once_at_last_uv() {
        let now = Instant::now();
        let mut map = MapController::new();
        map.step(sample(Hand::Right, UV, false, 0.0), now);
        map.step(sample(Hand::Right, UV, true, 0.0), now);
        let cancel = map.cancel().unwrap();
        assert!(cancel.cancelled);
        assert!(cancel.primary_released);
        assert_eq!(cancel.uv, UV);
        assert_eq!(map.cancel(), None);
        assert_eq!(map.step(None, now), None);
        assert_eq!(map.resting_uv(), Some(UV));
    }

    #[test]
    fn a_cancelled_press_selects_nothing_and_only_ends_a_drag() {
        let now = Instant::now();
        let mut map = MapController::new();
        map.step(sample(Hand::Right, UV, false, 0.0), now);
        map.step(sample(Hand::Right, UV, true, 0.0), now);
        let cancel = map.cancel().unwrap();
        assert!(!cancel.for_map(false).primary_released);
        assert!(cancel.for_map(true).primary_released);
        let cancel = MapDelivery::neutral(UV, true, false, true);
        assert!(!cancel.for_map(true).secondary_released);

        let release = map.step(sample(Hand::Right, UV, false, 0.0), now).unwrap();
        let genuine = MapDelivery {
            primary_released: true,
            ..release
        };
        assert!(genuine.for_map(false).primary_released);
    }

    #[test]
    fn desktop_arbitration_cancels_native_delivery_once() {
        let now = Instant::now();
        let mut map = MapController::new();
        let desktop = Some(RoutedPointer::Desktop { buttons: 1 });
        let controller = sample(Hand::Right, UV, false, 0.0);
        assert_eq!(prepare_delivery(&mut map, controller, desktop, now), None);

        assert_eq!(
            prepare_delivery(&mut map, controller, CONTROLLER_ROUTE, now),
            Some(MapDelivery::neutral(UV, false, false, false))
        );
        let pressed = sample(Hand::Right, UV, true, 0.0);
        let held = prepare_delivery(&mut map, pressed, CONTROLLER_ROUTE, now).unwrap();
        assert!(held.primary);
        for route in [desktop, Some(RoutedPointer::Parked), None] {
            let mut map = map;
            let cancellation = prepare_delivery(&mut map, pressed, route, now).unwrap();
            assert!(cancellation.cancelled);
            assert!(cancellation.primary_released);
            assert_eq!(cancellation.uv, UV);
            assert_eq!(prepare_delivery(&mut map, pressed, route, now), None);
        }
    }

    #[test]
    fn a_closed_map_forgets_its_state_silently() {
        let now = Instant::now();
        let mut map = MapController::new();
        map.step(sample(Hand::Right, UV, false, 0.0), now);
        map.step(sample(Hand::Right, UV, true, 0.0), now);
        let later = now + MAX_FRAME_AGE + Duration::from_millis(1);
        let reopened = map.step(sample(Hand::Right, UV, true, 0.0), later).unwrap();
        assert!(!reopened.cancelled);
        assert!(!reopened.primary_released);
        assert!(!reopened.primary);
        let mut map2 = MapController::new();
        map2.step(sample(Hand::Right, UV, false, 0.0), now);
        map2.step(sample(Hand::Right, UV, true, 0.0), now);
        assert_eq!(prepare_delivery(&mut map2, None, None, later), None);
    }

    /// An armed owner with the ray at `UV` and nothing held.
    fn armed() -> (MapController, Instant) {
        let now = Instant::now();
        let mut map = MapController::new();
        map.step(sample(Hand::Right, UV, false, 0.0), now);
        (map, now)
    }

    #[test]
    fn a_jittery_click_selects_at_its_press_point() {
        let (mut map, now) = armed();
        let pressed = map.step(sample(Hand::Right, UV, true, 0.0), now).unwrap();
        assert!(pressed.primary_pressed);
        assert!(pressed.held_at_press);
        assert_eq!(pressed.uv, UV);
        for jitter in [
            Vec2::new(0.01, 0.0),
            Vec2::new(-0.012, 0.015),
            Vec2::new(0.0, -0.02),
        ] {
            let held = map
                .step(sample(Hand::Right, UV + jitter, true, 0.0), now)
                .unwrap();
            assert_eq!(held.uv, UV, "a press within the slop does not move");
            assert!(held.held_at_press);
        }
        let jerked = UV + Vec2::new(0.2, 0.1);
        let released = map
            .step(sample(Hand::Right, jerked, false, 0.0), now)
            .unwrap();
        assert!(released.primary_released);
        assert_eq!(released.uv, UV, "the release selects where the press began");
        let after = map
            .step(sample(Hand::Right, jerked, false, 0.0), now)
            .unwrap();
        assert_eq!(after.uv, jerked);
        assert!(!after.held_at_press);
    }

    #[test]
    fn a_press_that_leaves_the_slop_drags_with_the_live_ray() {
        let (mut map, now) = armed();
        map.step(sample(Hand::Right, UV, true, 0.0), now);
        let far = UV + Vec2::new(CLICK_SLOP * 2.0, 0.0);
        let dragging = map.step(sample(Hand::Right, far, true, 0.0), now).unwrap();
        assert_eq!(dragging.uv, far);
        assert!(!dragging.held_at_press);
        let back = map.step(sample(Hand::Right, UV, true, 0.0), now).unwrap();
        assert_eq!(back.uv, UV);
        assert!(!back.held_at_press, "the anchor does not return once lost");
        let end = UV + Vec2::new(0.0, 0.3);
        let released = map.step(sample(Hand::Right, end, false, 0.0), now).unwrap();
        assert!(released.primary_released);
        assert_eq!(released.uv, end, "a drag ends at the live ray");
    }

    #[test]
    fn the_slop_holds_inside_and_lets_go_outside() {
        assert_eq!(
            press_anchor(
                Some(UV),
                UV + Vec2::new(CLICK_SLOP * 0.99, 0.0),
                true,
                false,
                false
            ),
            Some(UV)
        );
        assert_eq!(
            press_anchor(
                Some(UV),
                UV + Vec2::new(CLICK_SLOP * 1.01, 0.0),
                true,
                false,
                false
            ),
            None
        );
        assert_eq!(press_anchor(Some(UV), UV, false, false, false), None);
    }

    #[test]
    fn a_cancelled_press_releases_at_its_press_point() {
        let (mut map, now) = armed();
        map.step(sample(Hand::Right, UV, true, 0.0), now);
        map.step(sample(Hand::Right, UV + Vec2::splat(0.01), true, 0.0), now);
        let cancel = map.cancel().unwrap();
        assert_eq!(cancel.uv, UV);
        assert!(!cancel.held_at_press);
    }

    #[test]
    fn invalid_uv_values_cancel_without_panicking() {
        let mut map = MapController::new();
        assert!(
            map.step(sample(Hand::Right, Vec2::NAN, false, 0.0), Instant::now())
                .is_none()
        );
    }

    #[test]
    fn zoom_steps_once_per_push_then_repeats() {
        let start = Instant::now();
        let mut zoom = ZoomRepeat::new();
        assert_eq!(zoom.step(0.9, start), ZoomStep::In);
        assert_eq!(
            zoom.step(0.9, start + Duration::from_millis(16)),
            ZoomStep::None
        );
        assert_eq!(zoom.step(0.9, start + ZOOM_REPEAT_DELAY), ZoomStep::In);
        let repeat = start + ZOOM_REPEAT_DELAY + ZOOM_REPEAT_INTERVAL;
        assert_eq!(
            zoom.step(0.9, repeat - Duration::from_millis(1)),
            ZoomStep::None
        );
        assert_eq!(zoom.step(0.9, repeat), ZoomStep::In);
        assert_eq!(zoom.step(0.0, repeat), ZoomStep::None);
        assert_eq!(zoom.step(-0.9, repeat), ZoomStep::Out);
    }

    #[test]
    fn stick_drift_never_zooms() {
        let start = Instant::now();
        let mut zoom = ZoomRepeat::new();
        for frame in 0..600 {
            let at = start + Duration::from_millis(16 * frame);
            let drift = 0.3 * ((frame % 7) as f32 / 7.0) - 0.1;
            assert_eq!(zoom.step(drift, at), ZoomStep::None);
        }
        assert_eq!(zoom.step(f32::NAN, start), ZoomStep::None);
    }

    #[test]
    fn a_push_resting_between_the_thresholds_does_not_chatter() {
        let start = Instant::now();
        let mut zoom = ZoomRepeat::new();
        assert_eq!(zoom.step(0.65, start), ZoomStep::In);
        assert_eq!(zoom.step(0.55, start), ZoomStep::None);
        assert_eq!(zoom.step(0.65, start), ZoomStep::None);
        assert_eq!(zoom.step(0.3, start), ZoomStep::None);
        assert_eq!(zoom.step(0.65, start), ZoomStep::In);
    }

    #[test]
    fn zoom_waits_for_an_armed_owner() {
        let now = Instant::now();
        let mut map = MapController::new();
        assert_eq!(
            map.step(sample(Hand::Right, UV, true, 1.0), now)
                .unwrap()
                .zoom,
            ZoomStep::None
        );
        assert_eq!(
            map.step(sample(Hand::Right, UV, true, 1.0), now)
                .unwrap()
                .zoom,
            ZoomStep::None
        );
        map.step(sample(Hand::Right, UV, false, 0.0), now);
        assert_eq!(
            map.step(sample(Hand::Right, UV, false, 1.0), now)
                .unwrap()
                .zoom,
            ZoomStep::In
        );
    }
}
