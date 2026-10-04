//! Arbitration of the egui debug panel's pointer between the desktop mouse and the controller
//! pointer ([`crate::hud::pointer::egui_controller`]), and the egui events it produces.
//!
//! Whichever source was used last owns the pointer. The desktop mouse is used when it moves,
//! clicks, or scrolls. The controller is used when its ray lands on the panel, moves on it beyond
//! [`CONTROLLER_MOVE_UV`] (so tracking jitter of a controller at rest never takes the pointer from
//! the mouse), clicks, or scrolls. Ownership changes only while the owner holds no button, so a drag
//! always ends where it began. A source taking over with a button already held has that button
//! withheld until it is released, so the takeover itself never clicks.
//!
//! Positions are `uv * panel_size`, the panel texture's space, which egui lays out in while
//! [`crate::egui_impl::EguiState::set_panel_mode`] is active.

use glam::Vec2;
use parking_lot::Mutex;

use crate::{
    config::Config,
    hud::{
        cursor,
        pointer::{
            egui_controller::{ControllerUpdate, reset_egui_controller, take_updates},
            window_uv,
        },
    },
};

/// Build this frame's egui pointer events for the VR panel of `panel_size` pixels from the desktop
/// mouse and the controller updates queued since the last call. Call once per frame from the game
/// thread before [`crate::egui_impl::EguiState::run`].
pub fn egui_panel_events(panel_size: (u32, u32)) -> Vec<egui::Event> {
    let desktop = DesktopSample {
        uv: window_uv(),
        position: cursor::mouse_pos(),
        buttons: cursor::buttons(),
        wheel: cursor::take_wheel_notches(),
    };
    let updates = take_updates();
    let mut events = Vec::new();
    let cursor_uv = {
        let mut router = ROUTER.lock();
        router.step(desktop, &updates, panel_size, &mut events);
        router.cursor_uv
    };
    *CURSOR_UV.lock() = cursor_uv;
    events
}

/// Reset the panel pointer while the panel is not shown, so that the next time it is, the desktop
/// owns the pointer and a button held across the gap does not click.
pub fn reset_egui_pointer() {
    *ROUTER.lock() = EguiRouter::new();
    *CURSOR_UV.lock() = None;
    reset_egui_controller();
}

/// The panel UV of the pointer's owner, for the panel's cursor dot, or `None` while the owner is
/// off the panel.
pub fn egui_cursor_uv() -> Option<(f32, f32)> {
    *CURSOR_UV.lock()
}

/// Set egui's click options for the panel: while the panel is shown, a press held up to
/// [`EguiPanelConfig::click_duration_s`](crate::hud::config::EguiPanelConfig::click_duration_s)
/// still clicks; otherwise egui's own default applies. Call from the egui frame on the game thread.
pub fn apply_panel_input_options(ctx: &egui::Context, panel: bool) {
    let default = egui::InputOptions::default().max_click_duration;
    let duration = if panel {
        let configured = Config::lock_query(|c| c.hud.egui_panel.click_duration_s);
        if configured.is_finite() && configured > 0.0 {
            f64::from(configured)
        } else {
            default
        }
    } else {
        default
    };
    ctx.options_mut(|options| options.input_options.max_click_duration = duration);
}

/// How far, in panel UV, the controller's ray must move from where it rested while the desktop
/// owned the pointer before the controller takes the pointer over.
pub(crate) const CONTROLLER_MOVE_UV: f32 = 0.02;

/// The desktop mouse on one frame.
#[derive(Debug, Copy, Clone, PartialEq)]
struct DesktopSample {
    /// The window position normalized to panel UV.
    uv: Option<(f32, f32)>,
    /// The window-client pixel position, for change detection.
    position: Option<(i32, i32)>,
    /// The button bitmask, left in bit zero and right in bit one.
    buttons: u32,
    /// Wheel notches since the last frame.
    wheel: f32,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
enum Owner {
    Desktop,
    Controller,
}

#[derive(Debug, Copy, Clone, PartialEq)]
struct EguiRouter {
    owner: Owner,
    /// The buttons egui was last told are held.
    sent: u32,
    /// The owner's buttons are withheld until it holds none.
    wait_neutral: bool,
    /// The last position egui was given, for a release after the controller leaves the panel.
    pos: Option<egui::Pos2>,
    /// The desktop's previous position and buttons, for change detection.
    last_desktop: Option<(Option<(i32, i32)>, u32)>,
    /// The controller's previous update.
    controller: ControllerUpdate,
    /// Where the controller's ray rested when the desktop was last used.
    anchor: Option<(f32, f32)>,
    /// The owner's panel UV.
    cursor_uv: Option<(f32, f32)>,
}

static ROUTER: Mutex<EguiRouter> = Mutex::new(EguiRouter::new());

static CURSOR_UV: Mutex<Option<(f32, f32)>> = Mutex::new(None);

impl EguiRouter {
    const fn new() -> Self {
        Self {
            owner: Owner::Desktop,
            sent: 0,
            wait_neutral: true,
            pos: None,
            last_desktop: None,
            controller: ControllerUpdate::GONE,
            anchor: None,
            cursor_uv: None,
        }
    }

    fn step(
        &mut self,
        desktop: DesktopSample,
        updates: &[ControllerUpdate],
        size: (u32, u32),
        out: &mut Vec<egui::Event>,
    ) {
        let desktop_active = self.last_desktop.is_some_and(|(position, buttons)| {
            position != desktop.position || buttons != desktop.buttons
        }) || desktop.wheel != 0.0;
        self.last_desktop = Some((desktop.position, desktop.buttons));
        let mut controller_active = false;
        for update in updates {
            controller_active |= self.controller_used(update);
        }

        let mut took_over = false;
        if self.sent == 0 {
            match self.owner {
                Owner::Desktop if controller_active && !desktop_active => {
                    self.take_over(Owner::Controller, self.controller.buttons);
                    took_over = true;
                }
                Owner::Controller if desktop_active => {
                    self.take_over(Owner::Desktop, desktop.buttons);
                }
                Owner::Desktop | Owner::Controller => {}
            }
        }
        if self.owner == Owner::Controller || desktop_active {
            self.anchor = self.controller.uv;
        }

        match self.owner {
            Owner::Desktop => {
                self.emit_desktop(desktop, size, out);
                self.cursor_uv = desktop.uv;
            }
            Owner::Controller => {
                // Updates that arrived before the takeover in the same frame were activity, not
                // input for egui; only the latest state is delivered on a takeover frame.
                let deliver = match updates.split_last() {
                    Some((last, _)) if took_over => std::slice::from_ref(last),
                    _ => updates,
                };
                for update in deliver {
                    self.emit_controller(*update, size, out);
                }
                self.cursor_uv = self.controller.uv;
            }
        }
    }

    /// Fold one controller update into the activity tracking, and report whether it is a use of
    /// the controller.
    fn controller_used(&mut self, update: &ControllerUpdate) -> bool {
        let entered = update.uv.is_some() && self.controller.uv.is_none();
        let pressed = update.buttons & !self.controller.buttons != 0;
        let moved = match (update.uv, self.anchor) {
            (Some((u, v)), Some((au, av))) => {
                Vec2::new(u - au, v - av).length() > CONTROLLER_MOVE_UV
            }
            _ => false,
        };
        if update.uv.is_none() {
            self.anchor = None;
        } else if self.anchor.is_none() {
            self.anchor = update.uv;
        }
        self.controller = *update;
        entered || pressed || moved || update.scroll != Vec2::ZERO
    }

    fn take_over(&mut self, owner: Owner, held: u32) {
        self.owner = owner;
        self.wait_neutral = held != 0;
    }

    fn gate(&mut self, buttons: u32) -> u32 {
        if self.wait_neutral {
            self.wait_neutral = buttons != 0;
            return 0;
        }
        buttons
    }

    fn emit_desktop(
        &mut self,
        desktop: DesktopSample,
        size: (u32, u32),
        out: &mut Vec<egui::Event>,
    ) {
        let Some(uv) = desktop.uv else {
            return;
        };
        let pos = to_pos(uv, size);
        out.push(egui::Event::PointerMoved(pos));
        self.pos = Some(pos);
        let buttons = self.gate(desktop.buttons);
        self.send_buttons(buttons, pos, out);
        if desktop.wheel != 0.0 {
            out.push(egui::Event::MouseWheel {
                unit: egui::MouseWheelUnit::Line,
                delta: egui::Vec2::new(0.0, desktop.wheel),
                // Discrete wheel notches carry no gesture phase.
                phase: egui::TouchPhase::Move,
                modifiers: egui::Modifiers::default(),
            });
        }
    }

    fn emit_controller(
        &mut self,
        update: ControllerUpdate,
        size: (u32, u32),
        out: &mut Vec<egui::Event>,
    ) {
        let Some(uv) = update.uv else {
            if let Some(pos) = self.pos.take() {
                self.send_buttons(0, pos, out);
                out.push(egui::Event::PointerGone);
            }
            self.sent = 0;
            return;
        };
        let pos = to_pos(uv, size);
        out.push(egui::Event::PointerMoved(pos));
        self.pos = Some(pos);
        let buttons = self.gate(update.buttons);
        self.send_buttons(buttons, pos, out);
        if update.scroll != Vec2::ZERO {
            out.push(egui::Event::MouseWheel {
                unit: egui::MouseWheelUnit::Point,
                delta: egui::Vec2::new(update.scroll.x, update.scroll.y),
                // A held stick scrolls continuously, like a touchpad gesture in progress.
                phase: egui::TouchPhase::Move,
                modifiers: egui::Modifiers::default(),
            });
        }
    }

    fn send_buttons(&mut self, buttons: u32, pos: egui::Pos2, out: &mut Vec<egui::Event>) {
        let changed = buttons ^ self.sent;
        for (bit, button) in BUTTONS {
            if changed & bit != 0 {
                out.push(egui::Event::PointerButton {
                    pos,
                    button,
                    pressed: buttons & bit != 0,
                    modifiers: egui::Modifiers::default(),
                });
            }
        }
        self.sent = buttons;
    }
}

/// The pointer buttons by bit: primary (the left mouse button, the trigger) and secondary (the
/// right mouse button, the upper face button).
const BUTTONS: [(u32, egui::PointerButton); 2] = [
    (1, egui::PointerButton::Primary),
    (2, egui::PointerButton::Secondary),
];

fn to_pos((u, v): (f32, f32), (width, height): (u32, u32)) -> egui::Pos2 {
    egui::Pos2::new(u * width as f32, v * height as f32)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SIZE: (u32, u32) = (100, 100);

    fn mouse(position: (i32, i32), buttons: u32) -> DesktopSample {
        DesktopSample {
            uv: Some((position.0 as f32 / 100.0, position.1 as f32 / 100.0)),
            position: Some(position),
            buttons,
            wheel: 0.0,
        }
    }

    fn pointing(uv: (f32, f32), buttons: u32) -> ControllerUpdate {
        ControllerUpdate {
            uv: Some(uv),
            buttons,
            scroll: Vec2::ZERO,
        }
    }

    fn step(
        router: &mut EguiRouter,
        desktop: DesktopSample,
        updates: &[ControllerUpdate],
    ) -> Vec<egui::Event> {
        let mut out = Vec::new();
        router.step(desktop, updates, SIZE, &mut out);
        out
    }

    fn presses(events: &[egui::Event]) -> Vec<(egui::PointerButton, bool)> {
        events
            .iter()
            .filter_map(|event| match event {
                egui::Event::PointerButton {
                    button, pressed, ..
                } => Some((*button, *pressed)),
                _ => None,
            })
            .collect()
    }

    fn moved_to(events: &[egui::Event]) -> Option<egui::Pos2> {
        events.iter().rev().find_map(|event| match event {
            egui::Event::PointerMoved(pos) => Some(*pos),
            _ => None,
        })
    }

    #[test]
    fn the_desktop_owns_until_the_controller_is_used() {
        let mut router = EguiRouter::new();
        let events = step(&mut router, mouse((10, 20), 0), &[]);
        assert_eq!(moved_to(&events), Some(egui::pos2(10.0, 20.0)));
        let events = step(&mut router, mouse((10, 20), 0), &[pointing((0.5, 0.5), 0)]);
        assert_eq!(moved_to(&events), Some(egui::pos2(50.0, 50.0)));
        assert_eq!(router.owner, Owner::Controller);
        assert_eq!(router.cursor_uv, Some((0.5, 0.5)));
    }

    #[test]
    fn a_resting_controller_does_not_steal_the_pointer() {
        let mut router = EguiRouter::new();
        step(&mut router, mouse((10, 20), 0), &[]);
        step(&mut router, mouse((10, 20), 0), &[pointing((0.5, 0.5), 0)]);
        // The mouse takes it back, and the controller jitters in place.
        step(&mut router, mouse((11, 20), 0), &[pointing((0.5, 0.5), 0)]);
        assert_eq!(router.owner, Owner::Desktop);
        for jitter in [0.505, 0.495, 0.51] {
            let events = step(
                &mut router,
                mouse((11, 20), 0),
                &[pointing((jitter, 0.5), 0)],
            );
            assert_eq!(moved_to(&events), Some(egui::pos2(11.0, 20.0)));
        }
        step(&mut router, mouse((11, 20), 0), &[pointing((0.6, 0.5), 0)]);
        assert_eq!(router.owner, Owner::Controller);
    }

    #[test]
    fn the_mouse_takes_over_when_it_moves() {
        let mut router = EguiRouter::new();
        step(&mut router, mouse((10, 20), 0), &[]);
        step(&mut router, mouse((10, 20), 0), &[pointing((0.5, 0.5), 0)]);
        let events = step(&mut router, mouse((25, 75), 0), &[pointing((0.5, 0.5), 0)]);
        assert_eq!(moved_to(&events), Some(egui::pos2(25.0, 75.0)));
        assert_eq!(router.cursor_uv, Some((0.25, 0.75)));
    }

    #[test]
    fn a_drag_keeps_its_source_until_release() {
        let mut router = EguiRouter::new();
        step(&mut router, mouse((10, 20), 0), &[]);
        let events = step(&mut router, mouse((10, 20), 1), &[]);
        assert_eq!(presses(&events), [(egui::PointerButton::Primary, true)]);
        // The controller moves onto the panel mid-drag: the mouse keeps the pointer.
        let events = step(&mut router, mouse((12, 20), 1), &[pointing((0.5, 0.5), 0)]);
        assert_eq!(moved_to(&events), Some(egui::pos2(12.0, 20.0)));
        let events = step(&mut router, mouse((12, 20), 0), &[pointing((0.7, 0.5), 0)]);
        assert_eq!(presses(&events), [(egui::PointerButton::Primary, false)]);
        step(&mut router, mouse((12, 20), 0), &[pointing((0.9, 0.5), 0)]);
        assert_eq!(router.owner, Owner::Controller);
    }

    #[test]
    fn a_button_held_across_a_takeover_waits_for_its_release() {
        let mut router = EguiRouter::new();
        step(&mut router, mouse((10, 20), 0), &[]);
        step(&mut router, mouse((10, 20), 0), &[pointing((0.5, 0.5), 0)]);
        // The mouse button goes down as the mouse takes over: no press reaches egui.
        let events = step(&mut router, mouse((10, 20), 1), &[]);
        assert_eq!(router.owner, Owner::Desktop);
        assert!(presses(&events).is_empty());
        step(&mut router, mouse((10, 20), 0), &[]);
        let events = step(&mut router, mouse((10, 20), 1), &[]);
        assert_eq!(presses(&events), [(egui::PointerButton::Primary, true)]);
    }

    #[test]
    fn a_controller_click_presses_and_releases_at_the_hit() {
        let mut router = EguiRouter::new();
        step(&mut router, mouse((10, 20), 0), &[pointing((0.5, 0.5), 0)]);
        let events = step(&mut router, mouse((10, 20), 0), &[pointing((0.5, 0.5), 1)]);
        assert_eq!(presses(&events), [(egui::PointerButton::Primary, true)]);
        let events = step(&mut router, mouse((10, 20), 0), &[pointing((0.5, 0.5), 0)]);
        assert_eq!(presses(&events), [(egui::PointerButton::Primary, false)]);
    }

    #[test]
    fn a_press_and_release_within_one_frame_both_reach_egui() {
        let mut router = EguiRouter::new();
        step(&mut router, mouse((10, 20), 0), &[pointing((0.5, 0.5), 0)]);
        let events = step(
            &mut router,
            mouse((10, 20), 0),
            &[pointing((0.5, 0.5), 1), pointing((0.5, 0.5), 0)],
        );
        assert_eq!(
            presses(&events),
            [
                (egui::PointerButton::Primary, true),
                (egui::PointerButton::Primary, false)
            ]
        );
    }

    #[test]
    fn leaving_the_panel_releases_and_clears_the_hover() {
        let mut router = EguiRouter::new();
        step(&mut router, mouse((10, 20), 0), &[pointing((0.5, 0.5), 0)]);
        step(&mut router, mouse((10, 20), 0), &[pointing((0.5, 0.5), 1)]);
        let events = step(&mut router, mouse((10, 20), 0), &[ControllerUpdate::GONE]);
        assert_eq!(presses(&events), [(egui::PointerButton::Primary, false)]);
        assert!(matches!(events.last(), Some(egui::Event::PointerGone)));
        assert_eq!(router.cursor_uv, None);
        // The mouse resting where it was does not take the pointer back.
        let events = step(&mut router, mouse((10, 20), 0), &[ControllerUpdate::GONE]);
        assert!(events.is_empty());
    }

    #[test]
    fn the_stick_scrolls_in_points() {
        let mut router = EguiRouter::new();
        let scroll = ControllerUpdate {
            uv: Some((0.5, 0.5)),
            buttons: 0,
            scroll: Vec2::new(0.0, 12.0),
        };
        step(&mut router, mouse((10, 20), 0), &[pointing((0.5, 0.5), 0)]);
        let events = step(&mut router, mouse((10, 20), 0), &[scroll]);
        assert!(events.iter().any(|event| matches!(
            event,
            egui::Event::MouseWheel {
                unit: egui::MouseWheelUnit::Point,
                delta,
                ..
            } if delta.y == 12.0
        )));
    }

    #[test]
    fn the_desktop_wheel_is_delivered_in_lines() {
        let mut router = EguiRouter::new();
        let mut wheel = mouse((10, 20), 0);
        wheel.wheel = 1.0;
        let events = step(&mut router, wheel, &[]);
        assert!(events.iter().any(|event| matches!(
            event,
            egui::Event::MouseWheel {
                unit: egui::MouseWheelUnit::Line,
                ..
            }
        )));
    }
}
