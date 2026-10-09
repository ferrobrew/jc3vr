//! The per-mode binding tables: from the tick's semantic actions to the game actions wanted this
//! tick. Pure, with the little state the tables need across ticks (the pause button's long press,
//! what each grip press is doing ([`GripLatch`]), the wingsuit's evade press, and the menu and pad
//! latches).
//!
//! Each mode reads the semantic actions of its action set ([`crate::vr::input::semantic`]), which
//! the player can rebind in the runtime; [`crate::vr::input::semantic::defaults`] suggests the
//! layout. The gestures, the grabs, the chords, and the pointers read the raw controls instead, and
//! what they claim of a hand is masked from the semantic actions bound there
//! ([`crate::controllers::bindings::intents`]).
//!
//! Where a semantic action stands for a gamepad button, it injects every gameplay action the
//! game's default gamepad keymap puts on that button, and the game's own context picks the one that
//! applies (a virtual gamepad button). This is what keeps the game's tap and hold overloads
//! intact: the reload button is also the holster (hold), `USE_VEHICLE_MOD` is nitrous (hold) and
//! the turbo jump (tap), `ENTER_VEHICLE` held enters the gunner seat, and the grapple tethers on a
//! hold, all decided by the game from the hold time the tracker ([`crate::controllers::bindings::hold`])
//! writes. The pause action is the one overload the mod resolves itself: a short press pauses and
//! a long press opens the commlink.
//!
//! One action drives the mod rather than the game: on foot, flicking `crouch_flick` (the dominant
//! stick's vertical axis by default) down toggles the seated crouch
//! ([`crate::controllers::roomscale::toggle_seated_crouch`]).
//!
//! Holding both stick clicks together recenters the VR view, in every mode ([`stick_chord`]). A lone
//! stick click waits a short window for the other before it reaches the actions bound to it, so
//! that the chord never also fires either click's own binding. The left click waits longer, for
//! the debug panel chord too ([`crate::controllers::bindings::panel_chord`]).
//!
//! The sight tuning chord drives the mod too, and only while the sight tuning tool's marker is placed
//! ([`Controls::sight_chord`]): on foot, holding both grips and pressing the dominant A captures the
//! held weapon's sight offset ([`crate::controllers::aim::sights::tuning`]). While it is armed, the
//! dominant grip withholds the grapple (squeeze the dominant grip first) and both grips withhold the
//! jump, so the chord neither grapples nor jumps.
//!
//! The tables themselves are declared once, in [`declare`], over the vocabulary of [`sink`].
//! [`Bindings::step`] evaluates them against the tick's intents, and [`describe`] reads the same
//! declarations to list what each input does, for the controls summary
//! ([`crate::controllers::summary`]).

#[cfg(test)]
pub(crate) mod coverage;
pub mod declare;
pub mod describe;
pub mod menu;
pub mod sink;
pub mod stick_chord;

use jc3gi::input::input_action_map::Action;

use crate::{
    controllers::{
        bindings::{
            controls::{ButtonControl, Controls},
            grapple_button::{GrappleActions, GrappleButton, GrappleTick},
            hold::{Desired, MapKind, Target, Want},
            intents::{Gestures, Intents, Mask, MaskScope, SemanticInput},
            tables::{
                declare::{TableCtx, declare},
                sink::{EvalSink, Latches, MenuLatch, PadLatch},
                stick_chord::StickChord,
            },
        },
        config::BindingsConfig,
        frame::Hand,
        grab::{
            GrabVolume,
            latch::{GripLatch, LatchTick},
        },
        mode::{AirborneVariant, Mode, VehicleClass},
        weapons::wheel::select::WheelPress,
    },
    vr::input::semantic::{
        SemanticSet,
        bound::Physical,
        vocabulary::{
            ActionId, GlobalAction, HelicopterAction, LandVehicleAction, MenuAction, OnFootAction,
            PlaneAction, WingsuitAction,
        },
    },
};

/// One tick's output of the tables.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Step {
    /// The wanted game actions.
    pub desired: Vec<Desired>,
    /// The body-turn input for the headpose's VR turn scheme, `-1..=1`, positive turning right.
    /// Zero in the modes whose tables use the turn stick themselves.
    pub body_turn: f32,
    /// The seated crouch toggle was flicked this tick.
    pub crouch_toggle: bool,
    /// The grab each hand's grip started this tick, indexed by [`Hand::index`]: what the hand was in
    /// reach of at the press, or what a fist came to while the grip was held ([`GripLatch`]). The
    /// grabbing subsystems act on it; the tables inject nothing for it.
    pub grabs: [Option<GrabVolume>; 2],
    /// The sight tuning capture chord was pressed this tick.
    pub sight_capture: bool,
    /// Both stick clicks were held together for the recenter hold time this tick
    /// ([`stick_chord`]).
    pub recenter: bool,
    /// Each hand's weapon wheel inputs, by [`Hand::index`]: the weapon-select trackpad press with
    /// the quadrant it latched to, and the `weapon_wheel` action's value. The wheel
    /// ([`crate::controllers::weapons::wheel`]) acts on them; the tables inject nothing for the
    /// `weapon_wheel` action.
    pub wheel: [WheelPress; 2],
}

/// The tables and their cross-tick state.
#[derive(Debug, Clone, Default)]
pub struct Bindings {
    system: SystemButton,
    /// What each hand's grip press is doing, by [`Hand::index`].
    grips: [GripLatch; 2],
    /// Whether the current wingsuit parachute press was latched as an evade.
    evade_press: Option<bool>,
    /// How long the move stick has been pushed out of its rest, seconds, or `None` while it rests.
    move_pushed_s: Option<f32>,
    /// Whether the crouch flick has fired and waits for the stick to come back.
    crouch_flicked: bool,
    /// Each hand's menu press latches, by [`Hand::index`].
    menu: [MenuLatch; 2],
    /// Each hand's pad press latch, by [`Hand::index`].
    pads: [PadLatch; 2],
    stick_chord: StickChord,
    /// The grapple button's gestures.
    grapple: GrappleButton,
}

impl Bindings {
    pub const fn new() -> Self {
        Self {
            system: SystemButton { held: None },
            grips: [GripLatch::Open; 2],
            evade_press: None,
            move_pushed_s: None,
            crouch_flicked: false,
            menu: [MenuLatch {
                confirm: None,
                back: None,
                stick: None,
            }; 2],
            pads: [PadLatch { press: None }; 2],
            stick_chord: StickChord::new(),
            grapple: GrappleButton::new(),
        }
    }

    /// What each hand's grip press is doing, by [`Hand::index`].
    pub fn grip_latches(&self) -> [GripLatch; 2] {
        self.grips
    }

    /// Forget the stick clicks' current presses, withholding both until both are released: the
    /// press belongs to the debug panel chord ([`crate::controllers::bindings::panel_chord`]).
    pub fn cancel_stick_clicks(&mut self) {
        self.stick_chord.cancel();
    }

    /// Evaluate the tables for `mode`: `controls` are the tick's raw controls, which the gestures,
    /// grabs, and chords read, and `input` its semantic actions, which the tables read.
    pub fn step(
        &mut self,
        mode: Mode,
        controls: &Controls,
        input: &SemanticInput,
        dt: f32,
        config: &BindingsConfig,
    ) -> Step {
        let arbitrated = self.stick_chord.step(
            controls.hands.map(|h| h.stick_click),
            dt,
            config.stick_click_windows(),
            config.recenter_hold_s,
        );
        let chord = match mode {
            Mode::OnFoot(_) => SightChord::of(controls),
            _ => SightChord::default(),
        };
        let off_hand = controls.dominant.other();
        let grabs = Hand::BOTH.map(|hand| {
            let volume = controls.hand(hand).grab_volume;
            // The chord's off-hand grip press is part of the chord, not a grab: it must not let go
            // of a foregrip the off-hand holds.
            let volume = volume.filter(|v| {
                !(chord.dominant_grip && hand == off_hand && *v == GrabVolume::Foregrip)
            });
            self.grips[hand.index()].step(LatchTick {
                grip: controls.hand(hand).grip,
                target: volume,
                bound: grip_bound(input, hand),
                slow: controls.hand(hand).grab_slow,
                dt,
            })
        });
        let ctx = TableCtx::of(controls);
        let mut masks = context_masks(mode, &ctx, controls.dominant);
        for hand in Hand::BOTH {
            let i = hand.index();
            // A grip press latched to a grab belongs to the grab for the whole press.
            if self.grips[i].grabbing() {
                masks.push(Mask {
                    hand,
                    scope: MaskScope::Input(Physical::Grip),
                    exempt: None,
                });
            }
            if controls.masked_hands[i] {
                masks.push(Mask {
                    hand,
                    scope: MaskScope::Hand,
                    exempt: None,
                });
            }
            for physical in MASKABLE {
                if controls.masked_inputs[i].contains(physical) {
                    masks.push(Mask {
                        hand,
                        scope: MaskScope::Input(physical),
                        exempt: None,
                    });
                }
            }
        }
        let intents = Intents::build(
            input,
            arbitrated.clicks,
            &masks,
            Gestures {
                yank: controls.yank,
            },
            off_hand,
        );
        for hand in Hand::BOTH {
            let i = hand.index();
            menu::step(&mut self.menu[i], &intents, hand, config.menu_stick_push);
            menu::step_pad(&mut self.pads[i], &intents, hand);
        }
        let move_stick =
            move_action(intents.mode_set()).map_or(0.0, |a| intents.vector(a).length());
        self.move_pushed_s = step_pushed(self.move_pushed_s, move_stick, config.evade_flick, dt);
        let evade = match mode {
            Mode::Airborne(variant) => latch_evade(
                &mut self.evade_press,
                variant,
                &intents,
                self.move_pushed_s,
                config,
            ),
            _ => false,
        };
        let latches = Latches {
            chord_grip: chord.dominant_grip,
            chord_armed: chord.armed,
            evade,
            menu: self.menu,
            pads: self.pads,
        };
        let map = match mode {
            Mode::Ui => MapKind::Ui,
            _ => MapKind::Player,
        };
        let mut out = EvalSink::new(&intents, latches, controls.move_rotation, map);
        declare(&mut out, mode, &ctx);
        if let Some((inputs, actions)) = grapple_press(mode) {
            let button = inputs
                .iter()
                .map(|&a| intents.button(a))
                .fold(ButtonControl::default(), merge_buttons);
            self.grapple.apply(
                GrappleTick {
                    button,
                    context: controls.grapple,
                    yank: controls.yank,
                    dt,
                },
                actions,
                &mut out.desired,
            );
        }
        let body_turn = body_turn_action(mode).map_or(0.0, |a| intents.raw(a));
        if !matches!(mode, Mode::Airborne(AirborneVariant::Wingsuit)) {
            self.evade_press = None;
        }
        self.system.step(
            &mut out.desired,
            intents.button(GlobalAction::Pause.id()),
            dt,
            config.long_press_s,
        );
        let crouch_toggle = matches!(mode, Mode::OnFoot(_))
            && flick_down(
                &mut self.crouch_flicked,
                intents.raw(OnFootAction::CrouchFlick.id()),
                config.crouch_flick,
            );
        let select = OnFootAction::WeaponSelectPress.id();
        let wheel = Hand::BOTH.map(|hand| WheelPress {
            pad: intents.hand(select, hand).button,
            zone: self.pads[hand.index()].zone(select),
            stick: intents.hand(OnFootAction::WeaponWheel.id(), hand).value,
        });
        Step {
            desired: out.desired,
            body_turn,
            crouch_toggle,
            grabs,
            sight_capture: chord.capture,
            recenter: arbitrated.recenter,
            wheel,
        }
    }
}

/// The masks the table context implies in `mode`: with a pistol in the off-hand, its trigger
/// belongs to the pistol's fire action; in a menu, the native UI pointer's trigger belongs to the
/// pointer.
pub fn context_masks(mode: Mode, ctx: &TableCtx, dominant: Hand) -> Vec<Mask> {
    let mut masks = Vec::new();
    let set = SemanticSet::of(mode.action_set());
    if ctx.dual_wield
        && triggers_fire_pistols(mode)
        && let Some(pistol) = fire_off_hand_pistol(set)
    {
        masks.push(Mask {
            hand: dominant.other(),
            scope: MaskScope::Input(Physical::Trigger),
            exempt: Some(pistol),
        });
    }
    if mode == Mode::Ui
        && let Some(hand) = ctx.pointer_hand
    {
        masks.push(Mask {
            hand,
            scope: MaskScope::Input(Physical::Trigger),
            exempt: Some(MenuAction::PointerSelect.id()),
        });
    }
    masks
}

/// Whether the hands' triggers fire the hand weapons in `mode`: everywhere but the wingsuit, a
/// driver's seat, and the menus.
const fn triggers_fire_pistols(mode: Mode) -> bool {
    match mode {
        Mode::OnFoot(_) => true,
        Mode::Airborne(variant) => !matches!(variant, AirborneVariant::Wingsuit),
        Mode::Vehicle(seat) => !seat.driver,
        Mode::Ui => false,
    }
}

/// The off-hand pistol's fire action of `set`, for the sets whose hands fire pistols.
const fn fire_off_hand_pistol(set: SemanticSet) -> Option<ActionId> {
    match set {
        SemanticSet::OnFoot => Some(OnFootAction::FireOffHandPistol.id()),
        SemanticSet::LandVehicle => Some(LandVehicleAction::FireOffHandPistol.id()),
        SemanticSet::Helicopter => Some(HelicopterAction::FireOffHandPistol.id()),
        SemanticSet::Plane => Some(PlaneAction::FireOffHandPistol.id()),
        SemanticSet::Global | SemanticSet::Wingsuit | SemanticSet::Menu => None,
    }
}

/// The inputs a mechanism outside the tables may claim ([`Controls::masked_inputs`]).
const MASKABLE: [Physical; 8] = [
    Physical::Trigger,
    Physical::Grip,
    Physical::Thumbstick,
    Physical::ThumbstickClick,
    Physical::Trackpad,
    Physical::LowerFace,
    Physical::UpperFace,
    Physical::System,
];

/// The grapple's semantic actions in `mode`, and the game actions its gestures drive there.
pub(crate) const fn grapple_press(mode: Mode) -> Option<(&'static [ActionId], GrappleActions)> {
    // On foot and in the wingsuit the release is the gamepad's B, which the grapple's button
    // took; in a vehicle it is the release alone.
    const ON_FOOT: GrappleActions = GrappleActions {
        fire: declare::GRAPPLE,
        release: declare::PAD_B,
    };
    const VEHICLE: GrappleActions = GrappleActions {
        fire: declare::GRAPPLE_VEHICLE,
        release: &[Action::RELEASE_GRAPPLE],
    };
    match mode {
        Mode::OnFoot(_) | Mode::Airborne(AirborneVariant::Falling | AirborneVariant::Parachute) => {
            Some((&ON_FOOT_GRAPPLE, ON_FOOT))
        }
        Mode::Airborne(AirborneVariant::Wingsuit) => Some((&WINGSUIT_GRAPPLE, ON_FOOT)),
        Mode::Vehicle(seat) => Some((
            match seat.class {
                VehicleClass::Helicopter => &HELICOPTER_GRAPPLE,
                VehicleClass::Plane => &PLANE_GRAPPLE,
                VehicleClass::Car
                | VehicleClass::Motorcycle
                | VehicleClass::Sea
                | VehicleClass::Unknown => &LAND_GRAPPLE,
            },
            VEHICLE,
        )),
        Mode::Ui => None,
    }
}

const ON_FOOT_GRAPPLE: [ActionId; 2] = [OnFootAction::Grapple.id(), OnFootAction::FireGrapple.id()];
const WINGSUIT_GRAPPLE: [ActionId; 2] = [
    WingsuitAction::Grapple.id(),
    WingsuitAction::FireGrapple.id(),
];
const LAND_GRAPPLE: [ActionId; 1] = [LandVehicleAction::Grapple.id()];
const HELICOPTER_GRAPPLE: [ActionId; 1] = [HelicopterAction::Grapple.id()];
const PLANE_GRAPPLE: [ActionId; 1] = [PlaneAction::Grapple.id()];

/// Two presses of one gesture as one: held while either is, with either's edges.
pub(crate) fn merge_buttons(a: ButtonControl, b: ButtonControl) -> ButtonControl {
    ButtonControl {
        down: a.down || b.down,
        pressed: a.pressed || b.pressed,
        released: a.released || b.released,
    }
}

/// The vector action of `set` that moves or steers, whose flick the wingsuit's evade reads.
const fn move_action(set: SemanticSet) -> Option<ActionId> {
    match set {
        SemanticSet::OnFoot => Some(OnFootAction::Move.id()),
        SemanticSet::Wingsuit => Some(WingsuitAction::Steer.id()),
        SemanticSet::LandVehicle => Some(LandVehicleAction::Steer.id()),
        SemanticSet::Helicopter => Some(HelicopterAction::Cyclic.id()),
        SemanticSet::Plane => Some(PlaneAction::PitchRoll.id()),
        SemanticSet::Global | SemanticSet::Menu => None,
    }
}

/// The action whose value turns the body in `mode`, or `None` in the modes whose tables use the
/// turn input themselves.
pub const fn body_turn_action(mode: Mode) -> Option<ActionId> {
    match mode {
        Mode::OnFoot(_) | Mode::Airborne(AirborneVariant::Falling | AirborneVariant::Parachute) => {
            Some(OnFootAction::Turn.id())
        }
        Mode::Airborne(AirborneVariant::Wingsuit) => Some(WingsuitAction::Turn.id()),
        Mode::Vehicle(seat) => match seat.class {
            VehicleClass::Helicopter | VehicleClass::Plane => None,
            VehicleClass::Car
            | VehicleClass::Motorcycle
            | VehicleClass::Sea
            | VehicleClass::Unknown => Some(LandVehicleAction::Turn.id()),
        },
        Mode::Ui => None,
    }
}

/// Whether a game action of the tick's sets reads `hand`'s grip: a press there with nothing in
/// reach drives that action rather than closing the hand into a fist.
fn grip_bound(input: &SemanticInput, hand: Hand) -> bool {
    [&input.global, &input.mode]
        .into_iter()
        .flat_map(|set| &set.actions)
        .any(|action| action.bound[hand.index()].contains(Physical::Grip))
}

/// A downward flick of a stick axis `y` (+Y forward) past `threshold`: fires once, then re-arms
/// once the stick is back above half the threshold.
fn flick_down(fired: &mut bool, y: f32, threshold: f32) -> bool {
    let threshold = threshold.clamp(0.05, 1.0);
    if !y.is_finite() {
        return false;
    }
    if *fired {
        if y > -threshold * 0.5 {
            *fired = false;
        }
        false
    } else if y <= -threshold {
        *fired = true;
        true
    } else {
        false
    }
}

/// The sight tuning capture chord on foot: both grips held and the dominant A pressed, armed only
/// while [`Controls::sight_chord`] is set.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Default)]
struct SightChord {
    /// The dominant grip is held with the chord armed: the off-hand's grapple is withheld.
    dominant_grip: bool,
    /// Both grips are held with the chord armed: the dominant A's jump is withheld.
    armed: bool,
    /// The dominant A was pressed with both grips held.
    capture: bool,
}

impl SightChord {
    fn of(c: &Controls) -> Self {
        if !c.sight_chord {
            return Self::default();
        }
        let (dom, off) = (c.dom(), c.off());
        let dominant_grip = dom.grip.held();
        let armed = dominant_grip && off.grip.held();
        Self {
            dominant_grip,
            armed,
            capture: armed && dom.a.pressed,
        }
    }
}

/// Advance how long the move stick has been out of its rest: it rests below half the evade flick,
/// and the time counts from when it last left the rest.
fn step_pushed(pushed_s: Option<f32>, magnitude: f32, evade_flick: f32, dt: f32) -> Option<f32> {
    let dt = if dt.is_finite() { dt.max(0.0) } else { 0.0 };
    if !magnitude.is_finite() || magnitude < evade_flick * 0.5 {
        return None;
    }
    Some(pushed_s.map_or(0.0, |s| s + dt))
}

/// Decide at the wingsuit parachute press's edge whether the press evades or acts as the virtual A.
/// It evades only on a flick: the steer stick is past the evade flick and left its rest within the
/// evade window. A stick held over to steer or dive is not a flick, so the press still opens the
/// parachute.
fn latch_evade(
    latch: &mut Option<bool>,
    variant: AirborneVariant,
    intents: &Intents,
    move_pushed_s: Option<f32>,
    config: &BindingsConfig,
) -> bool {
    let press = intents.button(WingsuitAction::OpenParachute.id());
    if variant != AirborneVariant::Wingsuit || !press.held() {
        *latch = None;
        return false;
    }
    *latch.get_or_insert_with(|| {
        intents.vector(WingsuitAction::Steer.id()).length() >= config.evade_flick
            && move_pushed_s.is_some_and(|s| s <= config.evade_window_s)
    })
}

/// The pause action: a press shorter than the long-press time clicks pause on release; holding it
/// that long clicks the commlink at once, and the release then does nothing. Both clicks go to the
/// player map through its blockers, which is where the game reads the toggles while a menu is up,
/// and to the UI map, which the open screen reads.
#[derive(Debug, Clone, Default)]
struct SystemButton {
    /// How long the current press has been held, and whether it already fired the long press.
    held: Option<(f32, bool)>,
}

impl SystemButton {
    fn step(&mut self, out: &mut Vec<Desired>, button: ButtonControl, dt: f32, long_press_s: f32) {
        let dt = if dt.is_finite() { dt.max(0.0) } else { 0.0 };
        if button.held() {
            let (seconds, fired) = match self.held {
                Some((seconds, fired)) => (seconds + dt, fired),
                None => (0.0, false),
            };
            let long = !fired && seconds >= long_press_s;
            if long {
                toggle(out, Action::GUI_PDA);
            }
            self.held = Some((seconds, fired || long));
        } else if let Some((_, fired)) = self.held.take()
            && !fired
        {
            toggle(out, Action::GUI_PAUSE);
        }
    }
}

fn toggle(out: &mut Vec<Desired>, action: Action) {
    for map in [MapKind::PlayerThroughBlockers, MapKind::Ui] {
        out.push(Desired {
            target: Target { map, action },
            want: Want::Click,
        });
    }
}

#[cfg(test)]
mod tests;
