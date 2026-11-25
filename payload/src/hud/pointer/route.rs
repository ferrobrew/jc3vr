//! Arbitration of the game UI's mouse between the controller pointer and the desktop mouse.
//!
//! One router decides, per delivery, which source owns the Scaleform mouse. A source transition
//! never carries a held button across: taking over emits one neutral frame, and a held desktop
//! button is honoured only after it has been released. When the controller stops owning the UI the
//! pointer parks off the movie, which releases any held button without a click, and stays parked
//! until the desktop mouse shows activity, so the hover does not jump to wherever the desktop cursor
//! happens to rest.
//!
//! A caller that prepares a delivery ahead of time (the UI input step, which must show the map and
//! the Scaleform delivery the same sample) peeks the route with [`peek_route`] and commits it with
//! [`PendingRoute::commit`] only when the delivery happens, so a frame without a delivery cannot
//! consume a transition.

use parking_lot::Mutex;

use crate::hud::{cursor, pointer::controller::ControllerPointer};

/// The source selected for one game-UI mouse delivery.
#[derive(Debug, Copy, Clone, PartialEq)]
pub enum RoutedPointer {
    /// No source owns the UI: park the mouse off the movie with no button held. Parking releases a
    /// held button outside its target, which Scaleform does not treat as a click.
    Parked,
    /// The controller ray owns the UI at a panel UV.
    Controller { uv: (f32, f32), buttons: u32 },
    /// The desktop mouse owns the UI at the UI manager's own client-pixel position.
    Desktop { buttons: u32 },
}

impl RoutedPointer {
    /// Whether the controller owns this delivery.
    pub fn is_controller(self) -> bool {
        matches!(self, Self::Controller { .. })
    }
}

/// A route computed ahead of its delivery. Committing it applies the router transition it was
/// computed from; if another delivery advanced the router in between, the commit routes afresh.
#[derive(Debug, Copy, Clone)]
pub struct PendingRoute {
    pub routed: RoutedPointer,
    controller: Option<ControllerPointer>,
    before: RouterState,
    after: RouterState,
}

impl PendingRoute {
    /// Apply this route to the router for the delivery that is about to happen, and return the
    /// route to deliver.
    pub fn commit(self) -> RoutedPointer {
        self.apply(&mut ROUTER.lock(), MouseSample::current())
    }

    fn apply(self, router: &mut RouterState, mouse: MouseSample) -> RoutedPointer {
        if *router == self.before {
            *router = self.after;
            self.routed
        } else {
            router.route(self.controller, mouse)
        }
    }
}

/// Compute the route for `controller` without advancing the router.
pub fn peek_route(controller: Option<ControllerPointer>) -> PendingRoute {
    ROUTER.lock().peek(controller, MouseSample::current())
}

/// Route one delivery that happens now, advancing the router.
pub fn route_game_pointer(controller: Option<ControllerPointer>) -> RoutedPointer {
    ROUTER.lock().route(controller, MouseSample::current())
}

/// Cancel routing without transferring a held desktop button to a new route.
pub fn reset_route() {
    ROUTER.lock().reset();
}

/// Whether a desktop press currently owns the UI, so a controller press must not start mid-drag.
pub(crate) fn desktop_capture() -> bool {
    ROUTER.lock().desktop_capture()
}

/// The desktop mouse as the router sees it: the button bitmask and the client-pixel position.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub(crate) struct MouseSample {
    pub(crate) buttons: u32,
    pub(crate) position: Option<(i32, i32)>,
}

impl MouseSample {
    fn current() -> Self {
        Self {
            buttons: cursor::buttons(),
            position: cursor::mouse_pos(),
        }
    }
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
enum PointerSource {
    None,
    Desktop,
    Controller,
    /// The controller released the UI while the desktop mouse was at `at`.
    Parked {
        at: Option<(i32, i32)>,
    },
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub(crate) struct RouterState {
    source: PointerSource,
    /// Whether the desktop source holds a button, which keeps the desktop in charge until release.
    pressed: bool,
    /// Whether a held desktop button must be released before the desktop can press again.
    wait_mouse_neutral: bool,
}

impl RouterState {
    pub(crate) const fn new() -> Self {
        Self {
            source: PointerSource::None,
            pressed: false,
            wait_mouse_neutral: false,
        }
    }

    fn peek(&self, controller: Option<ControllerPointer>, mouse: MouseSample) -> PendingRoute {
        let mut after = *self;
        let routed = after.route(controller, mouse);
        PendingRoute {
            routed,
            controller,
            before: *self,
            after,
        }
    }

    fn reset(&mut self) {
        self.source = PointerSource::None;
        self.pressed = false;
        self.wait_mouse_neutral = true;
    }

    fn desktop_capture(&self) -> bool {
        self.source == PointerSource::Desktop && self.pressed
    }

    pub(crate) fn route(
        &mut self,
        controller: Option<ControllerPointer>,
        mouse: MouseSample,
    ) -> RoutedPointer {
        if self.desktop_capture() {
            self.pressed = mouse.buttons != 0;
            return RoutedPointer::Desktop {
                buttons: mouse.buttons,
            };
        }
        self.pressed = false;
        if let Some(controller) = controller {
            let buttons = if self.source == PointerSource::Controller {
                controller.buttons
            } else {
                self.source = PointerSource::Controller;
                self.wait_mouse_neutral = false;
                0
            };
            return RoutedPointer::Controller {
                uv: controller.uv,
                buttons,
            };
        }
        match self.source {
            PointerSource::Controller => {
                self.source = PointerSource::Parked { at: mouse.position };
                self.wait_mouse_neutral = true;
                return RoutedPointer::Parked;
            }
            PointerSource::Parked { at } if at == mouse.position && mouse.buttons == 0 => {
                return RoutedPointer::Parked;
            }
            PointerSource::Parked { .. } => self.source = PointerSource::None,
            PointerSource::None | PointerSource::Desktop => {}
        }
        if self.wait_mouse_neutral {
            if mouse.buttons != 0 {
                return RoutedPointer::Desktop { buttons: 0 };
            }
            self.wait_mouse_neutral = false;
        }
        self.source = PointerSource::Desktop;
        self.pressed = mouse.buttons != 0;
        RoutedPointer::Desktop {
            buttons: mouse.buttons,
        }
    }
}

static ROUTER: Mutex<RouterState> = Mutex::new(RouterState::new());

#[cfg(test)]
mod tests {
    use super::*;
    use crate::controllers::frame::Hand;

    const REST: Option<(i32, i32)> = Some((10, 20));

    fn mouse(buttons: u32) -> MouseSample {
        MouseSample {
            buttons,
            position: REST,
        }
    }

    fn controller(uv: (f32, f32), buttons: u32) -> Option<ControllerPointer> {
        Some(ControllerPointer {
            hand: Hand::Right,
            uv,
            buttons,
            owns_ui: true,
        })
    }

    #[test]
    fn inactive_reset_waits_for_desktop_neutral() {
        let mut router = RouterState {
            source: PointerSource::Desktop,
            pressed: true,
            wait_mouse_neutral: false,
        };
        router.reset();
        assert_eq!(
            router.route(None, mouse(1)),
            RoutedPointer::Desktop { buttons: 0 }
        );
        assert_eq!(
            router.route(None, mouse(0)),
            RoutedPointer::Desktop { buttons: 0 }
        );
        assert_eq!(
            router.route(None, mouse(1)),
            RoutedPointer::Desktop { buttons: 1 }
        );
    }

    #[test]
    fn a_desktop_drag_keeps_ownership_through_its_release() {
        let mut router = RouterState::new();
        let hover = controller((0.9, 0.8), 0);
        assert_eq!(
            router.route(None, mouse(1)),
            RoutedPointer::Desktop { buttons: 1 }
        );
        assert_eq!(
            router.route(hover, mouse(0)),
            RoutedPointer::Desktop { buttons: 0 }
        );
        assert_eq!(
            router.route(hover, mouse(0)),
            RoutedPointer::Controller {
                uv: (0.9, 0.8),
                buttons: 0
            }
        );
    }

    #[test]
    fn ownership_transition_neutralizes_and_waits_for_mouse_release() {
        let mut router = RouterState::new();
        let pressed = controller((0.5, 0.5), 1);
        assert_eq!(
            router.route(None, mouse(1)),
            RoutedPointer::Desktop { buttons: 1 }
        );
        assert!(!router.route(pressed, mouse(1)).is_controller());
        assert_eq!(
            router.route(pressed, mouse(0)),
            RoutedPointer::Desktop { buttons: 0 }
        );
        let takeover = router.route(pressed, mouse(0));
        assert_eq!(
            takeover,
            RoutedPointer::Controller {
                uv: (0.5, 0.5),
                buttons: 0
            }
        );
        assert_eq!(
            router.route(pressed, mouse(0)),
            RoutedPointer::Controller {
                uv: (0.5, 0.5),
                buttons: 1
            }
        );
        assert_eq!(router.route(None, mouse(1)), RoutedPointer::Parked);
        assert_eq!(
            router.route(None, mouse(1)),
            RoutedPointer::Desktop { buttons: 0 }
        );
        assert_eq!(
            router.route(None, mouse(0)),
            RoutedPointer::Desktop { buttons: 0 }
        );
    }

    #[test]
    fn controller_loss_mid_press_parks_instead_of_releasing_on_target() {
        let mut router = RouterState::new();
        router.route(controller((0.8, 0.9), 0), mouse(0));
        assert_eq!(
            router.route(controller((0.8, 0.9), 1), mouse(0)),
            RoutedPointer::Controller {
                uv: (0.8, 0.9),
                buttons: 1
            }
        );
        assert_eq!(router.route(None, mouse(0)), RoutedPointer::Parked);
    }

    #[test]
    fn a_parked_pointer_waits_for_desktop_activity() {
        let mut router = RouterState::new();
        router.route(controller((0.8, 0.9), 0), mouse(0));
        for _ in 0..3 {
            assert_eq!(router.route(None, mouse(0)), RoutedPointer::Parked);
        }
        let moved = MouseSample {
            buttons: 0,
            position: Some((11, 20)),
        };
        assert_eq!(
            router.route(None, moved),
            RoutedPointer::Desktop { buttons: 0 }
        );
    }

    #[test]
    fn a_click_while_parked_is_withheld_until_release() {
        let mut router = RouterState::new();
        router.route(controller((0.8, 0.9), 0), mouse(0));
        assert_eq!(router.route(None, mouse(0)), RoutedPointer::Parked);
        assert_eq!(
            router.route(None, mouse(1)),
            RoutedPointer::Desktop { buttons: 0 }
        );
        assert_eq!(
            router.route(None, mouse(0)),
            RoutedPointer::Desktop { buttons: 0 }
        );
        assert_eq!(
            router.route(None, mouse(1)),
            RoutedPointer::Desktop { buttons: 1 }
        );
    }

    #[test]
    fn the_controller_can_retake_a_parked_pointer() {
        let mut router = RouterState::new();
        router.route(controller((0.8, 0.9), 0), mouse(0));
        router.route(None, mouse(0));
        assert_eq!(
            router.route(controller((0.1, 0.2), 1), mouse(0)),
            RoutedPointer::Controller {
                uv: (0.1, 0.2),
                buttons: 0
            }
        );
    }

    #[test]
    fn a_peeked_route_advances_only_when_committed() {
        let mut router = RouterState::new();
        router.route(controller((0.8, 0.9), 1), mouse(0));
        // A frame that peeks the cancellation but makes no delivery leaves the router untouched,
        // so the next delivery still sees the cancellation.
        assert_eq!(router.peek(None, mouse(0)).routed, RoutedPointer::Parked);
        let pending = router.peek(None, mouse(0));
        assert_eq!(pending.routed, RoutedPointer::Parked);
        assert_eq!(pending.apply(&mut router, mouse(0)), RoutedPointer::Parked);
        assert_eq!(router.peek(None, mouse(0)).routed, RoutedPointer::Parked);
    }

    #[test]
    fn a_stale_peek_routes_afresh() {
        let mut router = RouterState::new();
        router.route(controller((0.8, 0.9), 0), mouse(0));
        let pending = router.peek(controller((0.8, 0.9), 0), mouse(0));
        router.route(None, mouse(0));
        assert_eq!(
            pending.apply(&mut router, mouse(0)),
            RoutedPointer::Controller {
                uv: (0.8, 0.9),
                buttons: 0
            }
        );
    }
}
