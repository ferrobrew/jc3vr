//! Pointer sources for the game's floating UI and the interactive egui panel.
//!
//! The game's UI is Scaleform, not egui. The controller path therefore produces the same panel UV
//! and primary-button state as the desktop mouse, and [`crate::hooks::ui`] feeds both through the
//! engine's `MovieImpl::NotifyMouseState` route. The desktop mouse remains the fallback when no
//! controller ray owns the game UI.

use std::{
    sync::atomic::{AtomicU32, Ordering},
    time::Instant,
};

use glam::{Vec3, Vec4};
use parking_lot::Mutex;

use crate::{
    controllers::{
        frame::{Button, ControllerFrame, Hand, Pose},
        hands::pose::compose_world,
    },
    headpose,
    hud::cursor,
};

/// The panel-texture UV of the desktop mouse this frame, or `None` when no mouse position or window
/// size is known yet. Shared by the egui event pump and the panel's own cursor dot so they agree.
pub fn window_uv() -> Option<(f32, f32)> {
    let (mx, my) = cursor::mouse_pos()?;
    let (nw, nh) = cursor::normalization_size()?;
    let u = (mx as f32 / nw as f32).clamp(0.0, 1.0);
    let v = (my as f32 / nh as f32).clamp(0.0, 1.0);
    Some((u, v))
}

/// Build the egui pointer events for the VR panel from the desktop mouse: a move to the current UV,
/// press/release for any button that changed since the last call, and any accumulated wheel motion.
/// Positions are `uv * panel_size` so they land in the panel texture's coordinate space (the same
/// space egui lays out in while [`crate::egui_impl::EguiState::set_panel_mode`] is active). Call once
/// per frame from the game thread before [`crate::egui_impl::EguiState::run`].
pub fn window_mouse_events(panel_size: (u32, u32)) -> Vec<egui::Event> {
    let mut events = Vec::new();
    let Some((u, v)) = window_uv() else {
        // Still drain the wheel so it does not burst on the next frame a position is known.
        let _ = cursor::take_wheel_notches();
        PREV_BUTTONS.store(cursor::buttons(), Ordering::Relaxed);
        return events;
    };
    let pos = egui::Pos2::new(u * panel_size.0 as f32, v * panel_size.1 as f32);
    events.push(egui::Event::PointerMoved(pos));

    let buttons = cursor::buttons();
    let prev = PREV_BUTTONS.swap(buttons, Ordering::Relaxed);
    let changed = buttons ^ prev;
    for (bit, button) in [
        (
            1u32 << (cursor::Button::Left as u32),
            egui::PointerButton::Primary,
        ),
        (
            1u32 << (cursor::Button::Right as u32),
            egui::PointerButton::Secondary,
        ),
    ] {
        if changed & bit != 0 {
            events.push(egui::Event::PointerButton {
                pos,
                button,
                pressed: buttons & bit != 0,
                modifiers: egui::Modifiers::default(),
            });
        }
    }

    let notches = cursor::take_wheel_notches();
    if notches != 0.0 {
        events.push(egui::Event::MouseWheel {
            unit: egui::MouseWheelUnit::Line,
            delta: egui::Vec2::new(0.0, notches),
            // Discrete wheel notches carry no gesture phase.
            phase: egui::TouchPhase::Move,
            modifiers: egui::Modifiers::default(),
        });
    }

    events
}

/// A ray in world space, used by the pure panel intersection helper.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct PanelRay {
    pub origin: Vec3,
    pub direction: Vec3,
}

/// A ray's intersection with the panel plane. `uv` is clamped for an active drag, while `inside`
/// records whether the original intersection was inside the displayed panel.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct PanelHit {
    pub uv: (f32, f32),
    pub inside: bool,
}

/// Intersect `ray` with the flat panel represented by top-left, top-right, bottom-left, and
/// bottom-right corners. The helper intentionally returns out-of-bounds UVs as a clamped hit: the
/// pointer state uses those values to keep a captured drag at the panel edge. A new hover only owns
/// the UI when [`PanelHit::inside`] is true.
pub fn intersect_panel(ray: PanelRay, corners: &[Vec4; 4]) -> Option<PanelHit> {
    if !ray.origin.is_finite() || !ray.direction.is_finite() {
        return None;
    }
    let top_left = corners[0].truncate();
    let top_right = corners[1].truncate();
    let bottom_left = corners[2].truncate();
    if !top_left.is_finite() || !top_right.is_finite() || !bottom_left.is_finite() {
        return None;
    }
    let across = top_right - top_left;
    let down = bottom_left - top_left;
    let normal = across.cross(down);
    let normal_length_squared = normal.length_squared();
    if !normal_length_squared.is_finite() || normal_length_squared <= f32::EPSILON {
        return None;
    }
    let denominator = normal.dot(ray.direction);
    if !denominator.is_finite() || denominator.abs() <= 1e-6 {
        return None;
    }
    let distance = normal.dot(top_left - ray.origin) / denominator;
    if !distance.is_finite() || distance < 0.0 {
        return None;
    }
    let point = ray.origin + ray.direction * distance;
    let across_length_squared = across.length_squared();
    let down_length_squared = down.length_squared();
    if across_length_squared <= f32::EPSILON || down_length_squared <= f32::EPSILON {
        return None;
    }
    let relative = point - top_left;
    let u = relative.dot(across) / across_length_squared;
    let v = relative.dot(down) / down_length_squared;
    if !u.is_finite() || !v.is_finite() {
        return None;
    }
    let inside = (-1e-5..=1.00001).contains(&u) && (-1e-5..=1.00001).contains(&v);
    Some(PanelHit {
        uv: (u.clamp(0.0, 1.0), v.clamp(0.0, 1.0)),
        inside,
    })
}

/// The center camera and tracking sample captured together before per-eye offsets.
#[derive(Clone, Copy)]
pub struct RenderedCamera {
    pub head: Pose,
    pub cockpit_head: Pose,
    pub scale: f32,
}

pub fn publish_camera(world: glam::Mat4, cockpit: Option<headpose::xr::CockpitPose>, scale: f32) {
    let (_, orientation, position) = world.to_scale_rotation_translation();
    *RENDERED_CAMERA.lock() = cockpit.map(|cockpit| {
        (
            RenderedCamera {
                head: Pose {
                    position,
                    orientation,
                },
                cockpit_head: Pose {
                    position: cockpit.position,
                    orientation: cockpit.orientation,
                },
                scale,
            },
            Instant::now(),
        )
    });
}

pub fn rendered_camera() -> Option<RenderedCamera> {
    RENDERED_CAMERA
        .lock()
        .filter(|(_, time)| time.elapsed() <= crate::controllers::frame::MAX_FRAME_AGE)
        .map(|(camera, _)| camera)
}

static RENDERED_CAMERA: Mutex<Option<(RenderedCamera, Instant)>> = Mutex::new(None);

/// The rendered panel and the tracking-to-world basis that placed its center camera.
#[derive(Clone, Copy)]
pub struct RenderedPanel {
    pub head: Pose,
    pub cockpit_head: Pose,
    pub scale: f32,
    pub corners: [Vec4; 4],
}

impl RenderedPanel {
    fn ray(self, aim: Pose) -> PanelRay {
        let rotation = self.head.orientation * self.cockpit_head.orientation.inverse();
        let world = compose_world(
            self.head.position,
            rotation,
            self.cockpit_head.position,
            aim,
            self.scale,
        );
        PanelRay {
            origin: world.position,
            direction: world.orientation * Vec3::NEG_Z,
        }
    }
}

pub fn publish_panel(panel: Option<RenderedPanel>) {
    *RENDERED_PANEL.lock() = panel.map(|panel| (panel, Instant::now()));
}

static RENDERED_PANEL: Mutex<Option<(RenderedPanel, Instant)>> = Mutex::new(None);

/// The controller-owned game-UI pointer for this frame.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct ControllerPointer {
    pub hand: Hand,
    pub uv: (f32, f32),
    /// Bit zero is the primary Scaleform button.
    pub buttons: u32,
    /// Whether the controller source should suppress the dominant hand's page action.
    pub owns_ui: bool,
}

/// The source selected for the engine's game-UI mouse route. `controller` distinguishes a
/// controller hover from the desktop fallback; the latter computes its UV from the UI manager's
/// own client-pixel position to preserve the pre-existing desktop path.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct RoutedPointer {
    pub controller: bool,
    /// A missing controller UV requests release at the last delivered controller position.
    /// A missing desktop UV uses the current desktop position.
    pub uv: Option<(f32, f32)>,
    pub buttons: u32,
}

/// Update the controller pointer state from the post-recovery controller frame. The caller supplies
/// the already-detected UI mode so a gameplay trigger can never become a game-menu click. The
/// dominant trigger is armed only after a released sample, which prevents a trigger held across a
/// mode transition, focus loss, or tracking recovery from creating a fresh click.
pub fn update_controller(
    frame: Option<&ControllerFrame>,
    ui_mode: bool,
    hand: Hand,
    recovery_ready: bool,
) -> Option<ControllerPointer> {
    let mut state = CONTROLLER_STATE.lock();
    state.expire(Instant::now());
    if !native_pointer_enabled() || !recovery_ready {
        state.captured = false;
        state.armed = false;
        state.output = None;
        state.updated = Some(Instant::now());
        return None;
    }
    let trigger_down = frame.is_some_and(|frame| {
        frame.focused && frame.hand(hand).buttons.contains(Button::TriggerPress)
    });
    let ui_frame = frame.filter(|frame| {
        frame.focused
            && frame.action_set == crate::controllers::frame::ActionSetKind::Ui
            && frame.is_fresh_at(Instant::now())
    });

    if ui_frame.is_none() || !ui_mode {
        state.armed = false;
        state.captured = false;
        state.output = None;
        state.updated = Some(Instant::now());
        return None;
    }

    let sample = ui_frame
        .filter(|frame| frame.focused)
        .filter(|_| {
            ui_mode
                && headpose::source() == headpose::Source::Vr
                && crate::controllers::scheme::is_controllers()
        })
        .and_then(|frame| {
            let aim = &frame.hand(hand).aim;
            if !frame.hand(hand).active || !frame.hand(hand).grip.is_valid() || !aim.is_valid() {
                return None;
            }
            let (panel, published) = (*RENDERED_PANEL.lock())?;
            if published.elapsed() > crate::controllers::frame::MAX_FRAME_AGE {
                return None;
            }
            Some((panel.ray(aim.pose), panel.corners))
        });

    let hit = sample.and_then(|(ray, corners)| intersect_panel(ray, &corners));
    let desktop_capture = {
        let router = ROUTER.lock();
        router.source == PointerSource::Desktop && router.pressed
    };
    state.suppress_desktop_press(desktop_capture, trigger_down);
    state.step(hit, trigger_down, hand, Instant::now())
}

impl ControllerState {
    fn suppress_desktop_press(&mut self, desktop_capture: bool, trigger_down: bool) {
        if desktop_capture && trigger_down {
            self.armed = false;
            self.captured = false;
        }
    }

    fn expire(&mut self, now: Instant) {
        if self.updated.is_some_and(|time| {
            now.saturating_duration_since(time) > crate::controllers::frame::MAX_FRAME_AGE
        }) {
            self.armed = false;
            self.captured = false;
            self.output = None;
            self.updated = None;
        }
    }

    fn step(
        &mut self,
        hit: Option<PanelHit>,
        trigger_down: bool,
        hand: Hand,
        now: Instant,
    ) -> Option<ControllerPointer> {
        self.expire(now);
        let state = self;
        let was_armed = state.armed;
        if trigger_down {
            state.armed = false;
        }
        let output = if let Some(hit) = hit {
            if !trigger_down {
                let owns_ui = hit.inside || state.captured;
                state.armed = true;
                state.captured = false;
                Some(ControllerPointer {
                    hand,
                    uv: hit.uv,
                    buttons: 0,
                    owns_ui,
                })
            } else if state.captured {
                // Continue a drag outside the panel while the ray still meets its plane.
                Some(ControllerPointer {
                    hand,
                    uv: hit.uv,
                    buttons: 1,
                    owns_ui: true,
                })
            } else if was_armed && hit.inside {
                state.armed = false;
                state.captured = true;
                Some(ControllerPointer {
                    hand,
                    uv: hit.uv,
                    buttons: 1,
                    owns_ui: true,
                })
            } else {
                // A held trigger that entered UI before the pointer was usable remains inert until a
                // complete release/press cycle. It can still provide hover feedback.
                Some(ControllerPointer {
                    hand,
                    uv: hit.uv,
                    buttons: 0,
                    owns_ui: hit.inside,
                })
            }
        } else {
            state.captured = false;
            state.armed = !trigger_down;
            None
        };

        state.output = output;
        state.updated = Some(now);
        output
    }
}

fn native_pointer_enabled() -> bool {
    crate::config::Config::lock_query(|c| c.hud.redirect && c.hud.quad && c.hud.cursor.enabled)
}

/// Return the most recently updated controller pointer, but never let a stalled input tick keep a
/// button held indefinitely.
pub fn controller_output() -> Option<ControllerPointer> {
    if !native_pointer_enabled() {
        return None;
    }
    let mut state = CONTROLLER_STATE.lock();
    state.expire(Instant::now());
    state.output
}

/// Clear controller ownership after focus, tracking, or input-set loss. The next controller press
/// must follow a real released sample before it can click.
pub fn clear_controller() {
    let mut state = CONTROLLER_STATE.lock();
    state.armed = false;
    state.captured = false;
    state.output = None;
    state.updated = None;
}

/// Whether the controller pointer owns the menu trigger while it is over the displayed panel. The
/// UI binding table uses that trigger for page navigation; suppressing that one route avoids a page
/// turn or gameplay action being delivered alongside the Scaleform click.
pub fn owns_menu_trigger() -> Option<Hand> {
    controller_output()
        .filter(|pointer| pointer.owns_ui)
        .map(|pointer| pointer.hand)
}

/// Route the game UI's button state between the controller pointer and the desktop mouse. A source
/// transition emits one neutral frame and, after controller ownership ends, waits for physical mouse
/// release before restoring the fallback. This prevents a held desktop or controller trigger from
/// becoming a new click merely because focus, tracking, or UI mode changed.
pub fn route_game_pointer(controller: Option<ControllerPointer>) -> RoutedPointer {
    ROUTER.lock().route(controller, cursor::buttons())
}

/// Cancel routing without transferring a held desktop button to a new route.
pub fn reset_route() {
    ROUTER.lock().reset();
}

impl RouterState {
    #[cfg(test)]
    pub(crate) fn new() -> Self {
        Self {
            source: PointerSource::None,
            pressed: false,
            wait_mouse_neutral: false,
        }
    }

    fn reset(&mut self) {
        self.source = PointerSource::None;
        self.pressed = false;
        self.wait_mouse_neutral = true;
    }

    pub(crate) fn route(
        &mut self,
        controller: Option<ControllerPointer>,
        mouse_buttons: u32,
    ) -> RoutedPointer {
        let state = self;
        if state.source == PointerSource::Desktop && state.pressed {
            state.pressed = mouse_buttons != 0;
            return RoutedPointer {
                controller: false,
                uv: None,
                buttons: mouse_buttons,
            };
        }
        state.pressed = false;
        if let Some(controller) = controller {
            if state.source != PointerSource::Controller {
                state.source = PointerSource::Controller;
                state.wait_mouse_neutral = false;
                return RoutedPointer {
                    controller: true,
                    uv: Some(controller.uv),
                    buttons: 0,
                };
            }
            return RoutedPointer {
                controller: true,
                uv: Some(controller.uv),
                buttons: controller.buttons,
            };
        }

        if state.source == PointerSource::Controller {
            state.source = PointerSource::None;
            state.wait_mouse_neutral = true;
            return RoutedPointer {
                controller: true,
                uv: None,
                buttons: 0,
            };
        }
        if state.wait_mouse_neutral {
            if mouse_buttons != 0 {
                return RoutedPointer {
                    controller: false,
                    uv: None,
                    buttons: 0,
                };
            }
            state.wait_mouse_neutral = false;
        }
        state.source = PointerSource::Desktop;
        state.pressed = mouse_buttons != 0;
        RoutedPointer {
            controller: false,
            uv: None,
            buttons: mouse_buttons,
        }
    }
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
enum PointerSource {
    None,
    Desktop,
    Controller,
}

struct ControllerState {
    armed: bool,
    captured: bool,
    output: Option<ControllerPointer>,
    updated: Option<Instant>,
}

static CONTROLLER_STATE: Mutex<ControllerState> = Mutex::new(ControllerState {
    armed: false,
    captured: false,
    output: None,
    updated: None,
});

pub(crate) struct RouterState {
    pressed: bool,
    source: PointerSource,
    wait_mouse_neutral: bool,
}

static ROUTER: Mutex<RouterState> = Mutex::new(RouterState {
    pressed: false,
    source: PointerSource::None,
    wait_mouse_neutral: false,
});

/// The previous frame's button bitmask (same layout as [`cursor::buttons`]), for edge detection.
static PREV_BUTTONS: AtomicU32 = AtomicU32::new(0);

#[cfg(test)]
mod tests {
    use super::*;

    fn corners() -> [Vec4; 4] {
        [
            Vec4::new(-1.0, 1.0, 0.0, 1.0),
            Vec4::new(1.0, 1.0, 0.0, 1.0),
            Vec4::new(-1.0, -1.0, 0.0, 1.0),
            Vec4::new(1.0, -1.0, 0.0, 1.0),
        ]
    }

    #[test]
    fn rendered_basis_handles_loading_base_scale_and_head_motion() {
        let base = glam::Quat::from_rotation_y(1.2);
        let translation = Vec3::new(80.0, -15.0, 30.0);
        let scale = 2.4;
        let panel_corners =
            corners().map(|p| (translation + base * (p.truncate() * scale)).extend(1.0));
        let aim = Pose {
            position: Vec3::new(0.0, 0.0, 2.0),
            orientation: glam::Quat::IDENTITY,
        };
        for position in [Vec3::ZERO, Vec3::new(0.3, 0.2, -0.4)] {
            let cockpit_head = Pose {
                position,
                orientation: glam::Quat::from_rotation_x(0.4),
            };
            let panel = RenderedPanel {
                head: Pose {
                    position: translation + base * (position * scale),
                    orientation: base * cockpit_head.orientation,
                },
                cockpit_head,
                scale,
                corners: panel_corners,
            };
            let hit = intersect_panel(panel.ray(aim), &panel.corners).unwrap();
            assert!(hit.inside);
            assert!((hit.uv.0 - 0.5).abs() < 1e-5);
            assert!((hit.uv.1 - 0.5).abs() < 1e-5);
        }
    }

    fn controller_state() -> ControllerState {
        ControllerState {
            armed: false,
            captured: false,
            output: None,
            updated: None,
        }
    }

    #[test]
    fn captured_release_outside_retains_ownership_and_current_uv() {
        let mut state = controller_state();
        let now = Instant::now();
        let inside = Some(PanelHit {
            uv: (0.2, 0.3),
            inside: true,
        });
        state.step(inside, false, Hand::Right, now);
        state.step(inside, true, Hand::Right, now);
        let outside = Some(PanelHit {
            uv: (1.0, 0.8),
            inside: false,
        });
        let release = state.step(outside, false, Hand::Right, now).unwrap();
        assert_eq!(
            (release.uv, release.buttons, release.owns_ui),
            ((1.0, 0.8), 0, true)
        );
        assert!(
            !state
                .step(outside, false, Hand::Right, now)
                .unwrap()
                .owns_ui
        );
    }

    #[test]
    fn press_during_desktop_capture_requires_real_release() {
        let mut state = controller_state();
        let now = Instant::now();
        let hit = Some(PanelHit {
            uv: (0.5, 0.5),
            inside: true,
        });
        state.step(hit, false, Hand::Right, now);
        state.suppress_desktop_press(true, true);
        assert_eq!(state.step(hit, true, Hand::Right, now).unwrap().buttons, 0);
        state.suppress_desktop_press(false, true);
        assert_eq!(state.step(hit, true, Hand::Right, now).unwrap().buttons, 0);
        state.step(hit, false, Hand::Right, now);
        assert_eq!(state.step(hit, true, Hand::Right, now).unwrap().buttons, 1);
    }

    #[test]
    fn inactive_reset_waits_for_desktop_neutral() {
        let mut router = RouterState {
            source: PointerSource::Desktop,
            pressed: true,
            wait_mouse_neutral: false,
        };
        router.reset();
        assert_eq!(router.route(None, 1).buttons, 0);
        assert_eq!(router.route(None, 0).buttons, 0);
        assert_eq!(router.route(None, 1).buttons, 1);
    }

    #[test]
    fn outside_press_is_consumed_before_entering_held() {
        let mut state = controller_state();
        let now = Instant::now();
        let outside = Some(PanelHit {
            uv: (1.0, 0.5),
            inside: false,
        });
        let inside = Some(PanelHit {
            uv: (0.5, 0.5),
            inside: true,
        });
        state.step(outside, false, Hand::Right, now);
        state.step(outside, true, Hand::Right, now);
        assert_eq!(
            state.step(inside, true, Hand::Right, now).unwrap().buttons,
            0
        );
        state.step(inside, false, Hand::Right, now);
        assert_eq!(
            state.step(inside, true, Hand::Right, now).unwrap().buttons,
            1
        );
    }

    #[test]
    fn stale_capture_requires_neutral_even_without_an_output_read() {
        for read_output in [false, true] {
            let mut state = controller_state();
            let now = Instant::now();
            let hit = Some(PanelHit {
                uv: (0.5, 0.5),
                inside: true,
            });
            state.step(hit, false, Hand::Right, now);
            assert_eq!(state.step(hit, true, Hand::Right, now).unwrap().buttons, 1);
            let later = now
                + crate::controllers::frame::MAX_FRAME_AGE
                + std::time::Duration::from_millis(1);
            if read_output {
                state.expire(later);
                assert!(state.output.is_none());
            }
            assert_eq!(
                state.step(hit, true, Hand::Right, later).unwrap().buttons,
                0
            );
            state.step(hit, false, Hand::Right, later);
            assert_eq!(
                state.step(hit, true, Hand::Right, later).unwrap().buttons,
                1
            );
        }
    }

    #[test]
    fn ray_maps_panel_corners_to_texture_uv() {
        let panel = corners();
        let hit = intersect_panel(
            PanelRay {
                origin: Vec3::new(0.0, 0.0, 2.0),
                direction: Vec3::NEG_Z,
            },
            &panel,
        )
        .expect("the ray should hit");
        assert_eq!(hit.uv, (0.5, 0.5));
        assert!(hit.inside);

        let hit = intersect_panel(
            PanelRay {
                origin: Vec3::new(-1.0, 1.0, 2.0),
                direction: Vec3::NEG_Z,
            },
            &panel,
        )
        .expect("the ray should hit");
        assert_eq!(hit.uv, (0.0, 0.0));
    }

    #[test]
    fn parallel_and_behind_rays_do_not_hit() {
        let panel = corners();
        assert!(
            intersect_panel(
                PanelRay {
                    origin: Vec3::ZERO,
                    direction: Vec3::X,
                },
                &panel,
            )
            .is_none()
        );
        assert!(
            intersect_panel(
                PanelRay {
                    origin: Vec3::new(0.0, 0.0, -1.0),
                    direction: Vec3::NEG_Z,
                },
                &panel,
            )
            .is_none()
        );
    }

    #[test]
    fn out_of_bounds_intersection_is_clamped_but_not_a_new_hover() {
        let panel = corners();
        let hit = intersect_panel(
            PanelRay {
                origin: Vec3::new(2.0, 0.0, 2.0),
                direction: Vec3::NEG_Z,
            },
            &panel,
        )
        .expect("the ray still meets the panel plane");
        assert_eq!(hit.uv, (1.0, 0.5));
        assert!(!hit.inside);
    }

    #[test]
    fn ownership_transition_neutralizes_and_waits_for_mouse_release() {
        let mut router = RouterState {
            pressed: false,
            source: PointerSource::None,
            wait_mouse_neutral: false,
        };
        let controller = ControllerPointer {
            hand: Hand::Right,
            uv: (0.5, 0.5),
            buttons: 1,
            owns_ui: true,
        };

        assert_eq!(router.route(None, 1).buttons, 1);
        assert!(!router.route(Some(controller), 1).controller);
        assert_eq!(router.route(Some(controller), 0).buttons, 0);
        assert_eq!(router.route(Some(controller), 0).buttons, 0);
        assert_eq!(router.route(Some(controller), 0).buttons, 1);
        assert!(router.route(Some(controller), 1).controller);
        assert_eq!(router.route(None, 1).buttons, 0);
        assert_eq!(router.route(None, 1).buttons, 0);
        assert_eq!(router.route(None, 0).buttons, 0);
    }
}
