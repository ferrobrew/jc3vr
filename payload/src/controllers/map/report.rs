//! The native map's readout: what the latest map input dispatch was given, and what the map did
//! with it. The map hooks write it on the game thread; the debug panel reads a copy.

use std::time::Instant;

use glam::Vec2;
use parking_lot::Mutex;

use crate::{
    controllers::map::controller::{MapDelivery, ZoomStep},
    hud::pointer::RoutedPointer,
};

/// The map dispatches seen so far, and what the latest one did.
#[derive(Debug, Clone, Default)]
pub(crate) struct MapReport {
    /// How many times the map handled its input with the mod's hooks installed.
    pub(crate) dispatches: u64,
    /// Controller primary releases the map took as a selection.
    pub(crate) selections: u64,
    /// Controller primary releases that ended a drag instead.
    pub(crate) drags: u64,
    /// Confirms the shim answered with the context action at the cursor.
    pub(crate) context_actions: u64,
    pub(crate) last: Option<DispatchReport>,
}

/// One map input dispatch.
#[derive(Debug, Copy, Clone)]
pub(crate) struct DispatchReport {
    pub(crate) at: Instant,
    pub(crate) outcome: DispatchOutcome,
    /// The pointer route prepared for the dispatch; `None` while the mod does not route the mouse.
    pub(crate) route: Option<RoutedPointer>,
    pub(crate) delivery: Option<MapDelivery>,
    /// The map cursor in stage coordinates, while the controller drives it.
    pub(crate) cursor_stage: Option<Vec2>,
    /// Whether the map read a clicked confirm, which became the context action at the cursor.
    pub(crate) context_from_confirm: bool,
    /// The map's own state after the dispatch.
    pub(crate) map: Option<MapFields>,
}

/// Whether, and why not, the controller drove a map dispatch.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub(crate) enum DispatchOutcome {
    /// The controller drove the map's mouse branch.
    Driven,
    /// The dispatch released the controller's ownership.
    Cancelled,
    /// The pointer route was not the controller, so the map saw its own input.
    NotRouted,
    /// The map handled input outside the UI manager's prepared dispatch.
    NotPrepared,
    /// No mouse device was present to scope the map's mouse actions to.
    NoMouseDevice,
    /// The UI manager singleton was not available to switch the map to its mouse branch.
    NoUiManager,
}

impl DispatchOutcome {
    /// A short explanation for the readout.
    pub(crate) fn describe(self) -> &'static str {
        match self {
            Self::Driven => "the controller drives the map's mouse input",
            Self::Cancelled => "the controller's ownership ended (a neutral release)",
            Self::NotRouted => {
                "the pointer route is not the controller; the map reads its own input"
            }
            Self::NotPrepared => "the map ran outside the UI input dispatch; nothing was delivered",
            Self::NoMouseDevice => "no mouse device to scope the map's mouse actions to",
            Self::NoUiManager => "the UI manager is unavailable",
        }
    }
}

/// The map fields the readout shows, read after the dispatch.
#[derive(Debug, Copy, Clone, PartialEq)]
pub(crate) struct MapFields {
    pub(crate) dragging: bool,
    /// Whether the dispatch selected, at [`selection`](Self::selection).
    pub(crate) selected: bool,
    pub(crate) context_menu: bool,
    /// The map's selection point, in stage coordinates.
    pub(crate) selection: Vec2,
}

/// A copy of the readout.
pub(crate) fn report() -> MapReport {
    REPORT.lock().clone()
}

/// A label for a zoom step.
pub(crate) fn zoom_label(zoom: ZoomStep) -> &'static str {
    match zoom {
        ZoomStep::None => "none",
        ZoomStep::In => "in",
        ZoomStep::Out => "out",
    }
}

/// Record one dispatch. `dragging_before` is whether the map was dragging when the dispatch began,
/// which tells a release that selects from one that ends a drag.
pub(super) fn record(dispatch: DispatchReport, dragging_before: bool) {
    let mut report = REPORT.lock();
    report.dispatches += 1;
    if let Some(delivery) = dispatch
        .delivery
        .filter(|delivery| !delivery.cancelled && delivery.primary_released)
    {
        if dragging_before {
            report.drags += 1;
        } else if dispatch.map.is_some_and(|map| map.selected) {
            report.selections += 1;
        }
        tracing::debug!(
            target: "controllers",
            uv = ?delivery.uv,
            held_at_press = delivery.held_at_press,
            ended_drag = dragging_before,
            "map: controller primary released"
        );
    }
    if dispatch.context_from_confirm {
        report.context_actions += 1;
        tracing::debug!(
            target: "controllers",
            cursor = ?dispatch.cursor_stage,
            "map: confirm opens the context menu at the controller cursor"
        );
    }
    report.last = Some(dispatch);
}

static REPORT: Mutex<MapReport> = Mutex::new(MapReport {
    dispatches: 0,
    selections: 0,
    drags: 0,
    context_actions: 0,
    last: None,
});
