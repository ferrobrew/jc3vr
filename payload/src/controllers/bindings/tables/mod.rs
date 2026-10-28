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
//! The shared input arbiter resolves the system chords before these tables. They consume only
//! accepted semantic values and never recognize or cancel a second chord.
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

use jc3gi::input::input_action_map::Action;

use crate::{
    controllers::{
        bindings::{
            controls::{ButtonControl, Controls},
            grapple_button::{
                GrappleActions, GrappleButton, GrappleContext, GrapplePermissions, GrappleTick,
            },
            hold::{Desired, MapKind, Target, Want},
            intents::{Gestures, Intents, Mask, MaskScope, SemanticInput},
            tables::{
                declare::{TableCtx, declare},
                sink::{EvalSink, Latches, MenuLatch, PadLatch},
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
        bound::{Physical, PhysicalSet},
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
    /// Each hand's weapon wheel inputs, by [`Hand::index`]: the weapon-select trackpad press with
    /// the quadrant it latched to, and the `weapon_wheel` action's value. The wheel
    /// ([`crate::controllers::weapons::wheel`]) acts on them; the tables inject nothing for the
    /// `weapon_wheel` action.
    pub wheel: [WheelPress; 2],
    /// The grapple's button is held this tick, in a mode with a grapple: the hand is aiming it.
    pub grapple_held: bool,
}

/// The tables and their cross-tick state.
#[derive(Debug, Clone, Default)]
pub struct Bindings {
    system: SystemButton,
    /// What each hand's grip press is doing, by [`Hand::index`].
    grips: [GripLatch; 2],
    /// Whether the crouch flick has fired and waits for the stick to come back.
    crouch_flicked: bool,
    /// Each hand's menu press latches, by [`Hand::index`].
    menu: [MenuLatch; 2],
    /// Each hand's pad press latch, by [`Hand::index`].
    pads: [PadLatch; 2],
    /// The grapple button's gestures.
    grapple: GrappleButton,
    /// Trigger sources accepted into the current grapple episode, including inseparable routes.
    grapple_triggers: [bool; 2],
}

impl Bindings {
    pub const fn new() -> Self {
        Self {
            system: SystemButton { held: None },
            grips: [GripLatch::Open; 2],
            crouch_flicked: false,
            menu: [MenuLatch {
                confirm: None,
                back: None,
                stick: None,
                page_prev: None,
                page_next: None,
            }; 2],
            pads: [PadLatch { press: None }; 2],
            grapple: GrappleButton::new(),
            grapple_triggers: [false; 2],
        }
    }

    /// What each hand's grip press is doing, by [`Hand::index`].
    pub fn grip_latches(&self) -> [GripLatch; 2] {
        self.grips
    }

    /// Claim the snatch's B episode before ordinary grapple evaluation.
    pub(crate) fn claim_snatch(
        &mut self,
        button: ButtonControl,
        eligible: bool,
        usable: bool,
        all_sources_neutral: bool,
    ) -> bool {
        self.grapple
            .observe_snatch_neutral(button, usable && all_sources_neutral);
        self.grapple.claim_snatch(button, eligible && usable)
    }

    /// Reset the tables while retaining ownership of a consumed snatch press.
    pub(crate) fn reset_preserving_snatch(&mut self) {
        let mut grapple = self.grapple;
        grapple.reset_preserving_snatch();
        *self = Self::new();
        self.grapple = grapple;
    }

    pub(crate) fn grapple_gate_open(mode: Mode, controls: &Controls) -> bool {
        !matches!(mode, Mode::OnFoot(_)) || !SightChord::of(controls).dominant_grip
    }

    pub(crate) fn semantic_masks(&self, mode: Mode, controls: &Controls) -> Vec<Mask> {
        let mut masks = context_masks(mode, &TableCtx::of(controls), controls.dominant);
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
        masks
    }

    /// Read the current grapple gesture permissions without advancing the tables.
    pub fn grapple_permissions(&self, context: GrappleContext) -> GrapplePermissions {
        self.grapple.current_permissions(context)
    }

    /// Cancel only a grapple episode attributable to a trigger whose ownership or usability changed.
    pub(crate) fn cancel_trigger_gestures(&mut self, changed: [bool; 2]) {
        if Hand::BOTH
            .into_iter()
            .any(|hand| changed[hand.index()] && self.grapple_triggers[hand.index()])
        {
            self.grapple.cancel();
            self.grapple_triggers = [false; 2];
        }
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
        let masks = self.semantic_masks(mode, controls);
        let intents = Intents::build(
            input,
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
        let latches = Latches {
            chord_grip: chord.dominant_grip,
            chord_armed: chord.armed,
            ragdolling: controls.ragdolling,
            menu: self.menu,
            pads: self.pads,
        };
        let map = match mode {
            Mode::Ui => MapKind::Ui,
            _ => MapKind::Player,
        };
        let mut out = EvalSink::new(&intents, latches, controls.move_rotation, map);
        declare(&mut out, mode, &ctx);
        let mut grapple_held = false;
        if let Some((inputs, actions)) = grapple_press(mode) {
            let button = inputs
                .iter()
                .map(|&a| intents.button(a))
                .fold(ButtonControl::default(), merge_buttons);
            grapple_held = button.down;
            if !self.grapple.pending() {
                self.grapple_triggers = [false; 2];
            }
            for hand in Hand::BOTH {
                for &id in inputs {
                    if intents.hand(id, hand).button.held()
                        && let Some(action) = input.get(id)
                    {
                        let bound = action.bound[hand.index()];
                        self.grapple_triggers[hand.index()] |= bound.contains(Physical::Trigger)
                            || bound == bound.with_unknown()
                            || bound == PhysicalSet::EMPTY;
                    }
                }
            }
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
            if !self.grapple.pending() {
                self.grapple_triggers = [false; 2];
            }
        }
        let body_turn = body_turn_action(mode).map_or(0.0, |a| intents.raw(a));
        self.system.step(
            &mut out.desired,
            intents.button(GlobalAction::Pause.id()),
            dt,
            config.long_press_s,
        );
        if mode != Mode::Ui {
            skip_cutscene(
                &mut out.desired,
                intents.button(GlobalAction::SkipCutscene.id()),
            );
        }
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
            wheel,
            grapple_held,
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
        && triggers_fire_hand_weapons(mode)
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
pub(crate) const fn triggers_fire_hand_weapons(mode: Mode) -> bool {
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
    // took. In a vehicle it is the vehicle's own release, which the vehicle reads for a tether
    // fired from it, with the character's for one fired from a passenger's seat.
    const ON_FOOT: GrappleActions = GrappleActions {
        fire: declare::GRAPPLE,
        release: declare::PAD_B,
    };
    const VEHICLE: GrappleActions = GrappleActions {
        fire: declare::GRAPPLE_VEHICLE,
        release: &[Action::VEHICLE_RELEASE_GRAPPLE, Action::RELEASE_GRAPPLE],
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
                | VehicleClass::Mech
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
            | VehicleClass::Mech
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

/// The cutscene skip: held while the action is. A cutscene reads its skip from the UI action map
/// whenever no game screen takes input, so it goes there, from every mode outside the menus.
fn skip_cutscene(out: &mut Vec<Desired>, button: ButtonControl) {
    if button.held() {
        out.push(Desired {
            target: Target {
                map: MapKind::Ui,
                action: Action::SKIP_CUTSCENE,
            },
            want: Want::Hold(1.0),
        });
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

#[cfg(test)]
mod grapple_permission_tests {
    use super::*;

    #[test]
    fn grapple_permissions_forward_the_current_phase_without_advancing() {
        let mut bindings = Bindings::new();
        let context = GrappleContext {
            tethers: true,
            line: false,
            aim_target: false,
        };
        for button in [
            ButtonControl {
                down: true,
                pressed: true,
                released: false,
            },
            ButtonControl {
                down: false,
                pressed: false,
                released: true,
            },
        ] {
            bindings.grapple.step(GrappleTick {
                button,
                context,
                yank: false,
                dt: 0.0,
            });
        }
        let before = bindings.grapple;
        let expected = GrapplePermissions {
            release: true,
            tighten: true,
        };
        assert_eq!(bindings.grapple_permissions(context), expected);
        assert_eq!(bindings.grapple_permissions(context), expected);
        assert_eq!(bindings.grapple, before);
    }
}
