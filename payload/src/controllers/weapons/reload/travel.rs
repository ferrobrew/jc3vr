//! The physical action: a slide, a charging handle, a bolt, or a pump as a part that follows the
//! hand through its travel. Pure.
//!
//! The part has a place along its travel ([`ActionTravel::position`], a share from home to the
//! rear) and, for a bolt, a place along its lift ([`ActionTravel::lift`], a share of the turn up
//! that unlocks it). A hand that takes the handle carries the part with it along the gun's axis,
//! held between home and the rear, from wherever along the part it took hold, so the part never
//! jumps to the hand:
//!
//! - a slide or a charging handle let go runs home on its spring, unless the slide catches at the
//!   rear: pulled back over a magazine that is in and empty, or after the shot that emptied it
//!   ([`ActionTravel::lock_back`]), a slide stays back until it is racked (taken and let go);
//! - a pump stays where it is let go, and follows its held foregrip even with a ready chamber;
//! - a bolt is turned up about its axis before it can be pulled back, cannot be turned down until
//!   it is home again, and stays where it is let go.
//!
//! The part reaching the rear of its travel is the rack; coming home after one (and, for a bolt,
//! turning down) cycles the action, which loads the chamber from the magazine. Slides, bolts, and
//! pumps can be worked while ready; a charging handle is offered only for an empty chamber or an
//! action out of battery.

use glam::Vec3;

use crate::controllers::weapons::reload::{chamber::ActionKind, config::ActionConfig};

/// One gun's action as a part on its travel.
#[derive(Debug, Copy, Clone, PartialEq, Default)]
pub struct ActionTravel {
    /// How far back the part is, as a share of its travel, `0..=1`.
    pub position: f32,
    /// How far a bolt is turned up, as a share of its lift, `0..=1`.
    pub lift: f32,
    /// The hand's hold on the handle, while it holds it.
    pub grip: Option<TravelGrip>,
    /// The slide is held at the rear by its catch.
    pub locked_back: bool,
    /// The part reached the rear since it last cycled.
    pub pulled: bool,
}

/// Where along the part the hand took hold: the hand's offset from the part along the gun's axis,
/// and, for a bolt, its turn about the bolt's axis ahead of the bolt's.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct TravelGrip {
    pub along_m: f32,
    pub angle_rad: f32,
}

/// What a tick of the travel reports.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Default)]
pub struct TravelEvents {
    /// The hand took the handle.
    pub grabbed: bool,
    /// The hand let go of the handle.
    pub let_go: bool,
    /// A bolt turned all the way up.
    pub lifted: bool,
    /// The part reached the rear of its travel.
    pub rear: bool,
    /// The part came home: forward of the return share, or a bolt turned back down.
    pub home: bool,
    /// The action cycled: home after a rack. The chamber loads from the magazine.
    pub cycled: bool,
    /// The slide caught at the rear.
    pub locked_back: bool,
    /// A hand freed the slide from its catch.
    pub unlocked: bool,
}

/// One tick's inputs, in the cockpit frame.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct TravelInputs {
    pub kind: ActionKind,
    /// The handle's place with the part at home.
    pub handle: Vec3,
    /// The gun's rearward axis (from the muzzle toward the stock), unit length.
    pub rearward: Vec3,
    /// The direction from a bolt's axis toward its handle at rest, across the gun's axis, unit
    /// length.
    pub side: Vec3,
    /// The gun's up, toward which a bolt's handle turns, unit length.
    pub up: Vec3,
    /// The part's travel, cockpit metres.
    pub travel_m: f32,
    /// The working hand's grip position; `None` without a hand free to work it.
    pub hand: Option<Vec3>,
    /// The working hand's grip was pressed onto the handle's volume this tick.
    pub grip_pressed: bool,
    pub grip_held: bool,
    /// For the pump: the working hand holds the foregrip.
    pub on_foregrip: bool,
    /// The chamber waits for the action.
    pub needed: bool,
    /// The magazine is in.
    pub magazine_in: bool,
    /// The magazine is in and holds a round to feed.
    pub feeds: bool,
    pub dt: f32,
}

impl ActionTravel {
    pub const fn new() -> Self {
        Self {
            position: 0.0,
            lift: 0.0,
            grip: None,
            locked_back: false,
            pulled: false,
        }
    }

    /// Whether the action is closed: the part home, and a bolt turned down. A gun fires only from
    /// battery.
    pub fn in_battery(&self, config: &ActionConfig) -> bool {
        self.position <= config.return_fraction && self.lift <= 0.0
    }

    /// Whether a held action delays ordinary gun release. A pump foregrip at home is support.
    pub fn holds_release(&self, kind: ActionKind) -> bool {
        self.grip.is_some()
            && (kind != ActionKind::Pump
                || self.position > 0.0
                || self.lift > 0.0
                || self.pulled)
    }

    /// The handle's place now, with the part where it is along its travel.
    pub fn handle_point(&self, inputs: &TravelInputs) -> Vec3 {
        inputs.handle + inputs.rearward * (self.position * inputs.travel_m)
    }

    /// Hold the slide at the rear on its catch: the shot that emptied the magazine leaves it there.
    pub fn lock_back(&mut self) {
        self.position = 1.0;
        self.locked_back = true;
        self.pulled = true;
    }

    /// Close the action at once, letting go of it: a belt reload works it by itself.
    pub fn close(&mut self) {
        *self = Self {
            grip: None,
            ..Self::new()
        };
    }

    /// Let go of the handle without moving the part: the weapon or the working hand changed.
    pub fn release(&mut self) {
        self.grip = None;
    }

    /// Fold in one tick.
    pub fn step(&mut self, inputs: &TravelInputs, config: &ActionConfig) -> TravelEvents {
        let mut events = TravelEvents::default();
        let valid = inputs.kind != ActionKind::None
            && inputs.handle.is_finite()
            && inputs.rearward.is_finite()
            && inputs.side.is_finite()
            && inputs.up.is_finite()
            && inputs.travel_m.is_finite()
            && inputs.travel_m > 0.0;
        if !valid {
            self.grip = None;
            return events;
        }
        let hand = inputs.hand.filter(|h| h.is_finite());
        let before = *self;
        let lift_rad = config.bolt_lift_deg.to_radians().max(1e-3);

        if self.grip.is_none()
            && let Some(hand) = hand
            && self.takes(inputs, hand, config)
        {
            self.grip = Some(TravelGrip {
                along_m: along(inputs, hand) - self.position * inputs.travel_m,
                angle_rad: angle(inputs, hand, config) - self.lift * lift_rad,
            });
            events.grabbed = true;
            if self.locked_back {
                self.locked_back = false;
                events.unlocked = true;
            }
        }

        match (self.grip, hand) {
            (Some(grip), Some(hand)) if self.holding(inputs, hand, config) => {
                let target =
                    ((along(inputs, hand) - grip.along_m) / inputs.travel_m).clamp(0.0, 1.0);
                if inputs.kind == ActionKind::Bolt {
                    let turn =
                        ((angle(inputs, hand, config) - grip.angle_rad) / lift_rad).clamp(0.0, 1.0);
                    self.bolt(target, turn, config);
                } else {
                    self.position = target;
                }
            }
            (Some(_), _) => {
                self.grip = None;
                events.let_go = true;
            }
            (None, _) => {}
        }

        if self.grip.is_none() {
            self.settle(inputs, config, &mut events);
        }

        if !self.pulled && self.position >= config.pull_fraction {
            self.pulled = true;
            events.rear = true;
        }
        if inputs.kind == ActionKind::Bolt {
            events.lifted = before.lift < 1.0 && self.lift >= 1.0;
            events.home = before.lift > 0.0 && self.lift <= 0.0;
        } else {
            events.home =
                before.position > config.return_fraction && self.position <= config.return_fraction;
        }
        if events.home && self.pulled {
            self.pulled = false;
            events.cycled = true;
        }
        events
    }

    /// Whether a hand at `hand` takes the handle this tick.
    fn takes(&self, inputs: &TravelInputs, hand: Vec3, config: &ActionConfig) -> bool {
        let wanted = inputs.kind.ready_rack()
            || inputs.needed
            || !self.in_battery(config)
            || self.locked_back;
        if !wanted {
            return false;
        }
        if inputs.kind == ActionKind::Pump {
            return inputs.on_foregrip;
        }
        inputs.grip_pressed && hand.distance(self.handle_point(inputs)) <= config.grab_radius_m
    }

    /// Whether the hand still holds the handle: its grip held and not strayed from the gun's axis,
    /// or for the pump, the foregrip held, which has its own release.
    fn holding(&self, inputs: &TravelInputs, hand: Vec3, config: &ActionConfig) -> bool {
        if inputs.kind == ActionKind::Pump {
            return inputs.on_foregrip;
        }
        let offset = hand - self.handle_point(inputs);
        let off_axis = (offset - inputs.rearward * offset.dot(inputs.rearward)).length();
        inputs.grip_held && off_axis <= config.release_distance_m
    }

    /// A held bolt toward the hand's pull `target` and turn `turn`: turned up before it is pulled
    /// back, and turned down only once it is home.
    fn bolt(&mut self, target: f32, turn: f32, config: &ActionConfig) {
        let unlock = config.lift_fraction.clamp(0.5, 1.0);
        if self.lift >= 1.0 && (self.position > config.return_fraction || target > 0.0) {
            self.position = target;
            if self.position > config.return_fraction {
                return;
            }
        }
        if self.position > config.return_fraction {
            return;
        }
        self.lift = if turn >= unlock {
            1.0
        } else if turn <= 1.0 - unlock {
            0.0
        } else {
            turn
        };
        if self.lift < 1.0 {
            self.position = 0.0;
        }
    }

    /// The part with no hand on it: a slide caught at the rear by an empty magazine, a slide or a
    /// charging handle run home by its spring, and a pump or a bolt left where it is, settling
    /// home once forward of the return share.
    fn settle(&mut self, inputs: &TravelInputs, config: &ActionConfig, events: &mut TravelEvents) {
        match inputs.kind {
            ActionKind::Slide | ActionKind::ChargingHandle => {
                let catches = inputs.kind == ActionKind::Slide
                    && config.lock_back
                    && inputs.magazine_in
                    && !inputs.feeds
                    && self.position >= config.pull_fraction;
                if self.locked_back {
                    self.position = 1.0;
                } else if catches {
                    self.lock_back();
                    events.locked_back = true;
                } else {
                    let rate = 1.0 / config.spring_s.max(1e-3);
                    self.position = (self.position - rate * inputs.dt.max(0.0)).max(0.0);
                }
            }
            ActionKind::Pump => {
                if self.position <= config.return_fraction {
                    self.position = 0.0;
                }
            }
            ActionKind::Bolt => {
                if self.lift <= 0.0 {
                    self.position = 0.0;
                }
            }
            ActionKind::None => {}
        }
    }
}

/// The hand's place along the gun's rearward axis from the handle's rest.
fn along(inputs: &TravelInputs, hand: Vec3) -> f32 {
    (hand - inputs.handle).dot(inputs.rearward)
}

/// The hand's turn about a bolt's axis from the handle's rest toward the gun's top, radians. The
/// axis lies `bolt_radius_m` from the handle's rest, opposite its side.
fn angle(inputs: &TravelInputs, hand: Vec3, config: &ActionConfig) -> f32 {
    let pivot = inputs.handle - inputs.side * config.bolt_radius_m;
    let offset = hand - pivot;
    let across = offset - inputs.rearward * offset.dot(inputs.rearward);
    across.dot(inputs.up).atan2(across.dot(inputs.side))
}

#[cfg(test)]
mod tests {
    use super::*;

    const CONFIG: ActionConfig = ActionConfig::new();
    const HANDLE: Vec3 = Vec3::new(0.04, 0.1, -0.05);
    const TRAVEL: f32 = 0.05;

    fn inputs(kind: ActionKind, hand: Vec3, pressed: bool, held: bool) -> TravelInputs {
        TravelInputs {
            kind,
            handle: HANDLE,
            rearward: Vec3::Z,
            side: Vec3::X,
            up: Vec3::Y,
            travel_m: TRAVEL,
            hand: Some(hand),
            grip_pressed: pressed,
            grip_held: held,
            on_foregrip: held,
            needed: true,
            magazine_in: true,
            feeds: true,
            dt: 1.0 / 90.0,
        }
    }

    fn back(share: f32) -> Vec3 {
        HANDLE + Vec3::Z * (share * TRAVEL)
    }

    #[test]
    fn a_slide_follows_the_hand_within_its_travel() {
        let mut t = ActionTravel::new();
        let k = ActionKind::Slide;
        let grab_point = back(0.0) + Vec3::Z * 0.01;
        let e = t.step(&inputs(k, grab_point, true, true), &CONFIG);
        assert!(e.grabbed);
        assert_eq!(t.position, 0.0, "the part does not jump to the hand");
        t.step(
            &inputs(k, grab_point + Vec3::Z * 0.02, false, true),
            &CONFIG,
        );
        assert!((t.position - 0.4).abs() < 1e-4);
        t.step(&inputs(k, grab_point + Vec3::Z * 0.2, false, true), &CONFIG);
        assert_eq!(t.position, 1.0, "held at the rear of its travel");
        t.step(&inputs(k, grab_point - Vec3::Z * 0.2, false, true), &CONFIG);
        assert_eq!(t.position, 0.0, "held at home");
    }

    #[test]
    fn a_racked_slide_springs_home_and_cycles() {
        let mut t = ActionTravel::new();
        let k = ActionKind::Slide;
        t.step(&inputs(k, back(0.0), true, true), &CONFIG);
        let e = t.step(&inputs(k, back(1.0), false, true), &CONFIG);
        assert!(e.rear && t.pulled);
        let e = t.step(&inputs(k, back(1.0), false, false), &CONFIG);
        assert!(e.let_go);
        let mut cycled = 0;
        for _ in 0..30 {
            let e = t.step(&inputs(k, back(1.0), false, false), &CONFIG);
            cycled += u32::from(e.cycled);
        }
        assert_eq!(cycled, 1);
        assert_eq!(t.position, 0.0);
        assert!(t.in_battery(&CONFIG));
    }

    #[test]
    fn a_short_pull_springs_home_without_cycling() {
        let mut t = ActionTravel::new();
        let k = ActionKind::ChargingHandle;
        t.step(&inputs(k, back(0.0), true, true), &CONFIG);
        t.step(&inputs(k, back(0.5), false, true), &CONFIG);
        for _ in 0..30 {
            let e = t.step(&inputs(k, back(0.5), false, false), &CONFIG);
            assert!(!e.cycled && !e.rear);
        }
        assert_eq!(t.position, 0.0);
    }

    #[test]
    fn a_distant_handle_refuses_but_a_ready_slide_remains_grabbable() {
        let mut t = ActionTravel::new();
        let k = ActionKind::Slide;
        let e = t.step(&inputs(k, back(0.0) + Vec3::Y * 0.2, true, true), &CONFIG);
        assert!(!e.grabbed);
        let not_needed = TravelInputs {
            needed: false,
            ..inputs(k, back(0.0), true, true)
        };
        assert!(t.step(&not_needed, &CONFIG).grabbed);
    }

    #[test]
    fn a_hand_straying_off_the_axis_lets_go() {
        let mut t = ActionTravel::new();
        let k = ActionKind::Slide;
        t.step(&inputs(k, back(0.0), true, true), &CONFIG);
        let away = back(0.5) + Vec3::X * (CONFIG.release_distance_m + 0.01);
        assert!(t.step(&inputs(k, away, false, true), &CONFIG).let_go);
        assert!(t.grip.is_none());
    }

    #[test]
    fn an_empty_magazine_catches_the_slide_until_it_is_racked() {
        let mut t = ActionTravel::new();
        let k = ActionKind::Slide;
        let empty = |hand, pressed, held| TravelInputs {
            feeds: false,
            ..inputs(k, hand, pressed, held)
        };
        t.step(&empty(back(0.0), true, true), &CONFIG);
        t.step(&empty(back(1.0), false, true), &CONFIG);
        let e = t.step(&empty(back(1.0), false, false), &CONFIG);
        assert!(e.locked_back && t.locked_back);
        for _ in 0..30 {
            t.step(&empty(back(1.0), false, false), &CONFIG);
        }
        assert_eq!(t.position, 1.0, "held back by the catch");
        assert!(!t.in_battery(&CONFIG));
        // A full magazine goes in; the slide is taken and let go: it runs home and chambers.
        let e = t.step(&inputs(k, back(1.0), true, true), &CONFIG);
        assert!(e.grabbed && e.unlocked && !t.locked_back);
        t.step(&inputs(k, back(1.0), false, false), &CONFIG);
        let mut cycled = false;
        for _ in 0..30 {
            cycled |= t.step(&inputs(k, back(1.0), false, false), &CONFIG).cycled;
        }
        assert!(cycled);
        assert_eq!(t.position, 0.0);
    }

    #[test]
    fn a_slide_locked_back_by_the_last_shot_stays_back() {
        let mut t = ActionTravel::new();
        t.lock_back();
        let free = TravelInputs {
            hand: None,
            ..inputs(ActionKind::Slide, back(0.0), false, false)
        };
        for _ in 0..30 {
            t.step(&free, &CONFIG);
        }
        assert_eq!(t.position, 1.0);
        t.close();
        assert!(t.in_battery(&CONFIG) && !t.locked_back);
    }

    #[test]
    fn without_a_magazine_the_slide_does_not_catch() {
        let mut t = ActionTravel::new();
        let k = ActionKind::Slide;
        let out = |hand, pressed, held| TravelInputs {
            feeds: false,
            magazine_in: false,
            ..inputs(k, hand, pressed, held)
        };
        t.step(&out(back(0.0), true, true), &CONFIG);
        t.step(&out(back(1.0), false, true), &CONFIG);
        let e = t.step(&out(back(1.0), false, false), &CONFIG);
        assert!(!e.locked_back);
        for _ in 0..30 {
            t.step(&out(back(1.0), false, false), &CONFIG);
        }
        assert_eq!(t.position, 0.0);
    }

    #[test]
    fn a_pump_follows_the_foregrip_hand_and_stays_where_it_is_left() {
        let mut t = ActionTravel::new();
        let k = ActionKind::Pump;
        let start = HANDLE + Vec3::new(0.0, -0.01, 0.005);
        let not_needed = TravelInputs {
            needed: false,
            ..inputs(k, start, false, true)
        };
        assert!(
            t.step(&not_needed, &CONFIG).grabbed,
            "a ready pump can be worked"
        );
        let e = t.step(&inputs(k, start + Vec3::Z * TRAVEL, false, true), &CONFIG);
        assert!(e.rear);
        let e = t.step(
            &inputs(k, start + Vec3::Z * 0.6 * TRAVEL, false, false),
            &CONFIG,
        );
        assert!(e.let_go);
        for _ in 0..10 {
            t.step(&inputs(k, start, false, false), &CONFIG);
        }
        assert_eq!(t.position, 1.0, "a pump has no spring");
        // The hand takes the foregrip again further forward and pushes the pump home.
        let regrip = start + Vec3::Z * 0.5 * TRAVEL;
        assert!(t.step(&inputs(k, regrip, false, true), &CONFIG).grabbed);
        let e = t.step(&inputs(k, regrip - Vec3::Z * TRAVEL, false, true), &CONFIG);
        assert!(e.home && e.cycled);
    }

    /// The hand at the bolt's handle turned `lift` of the way up and pulled `share` back.
    fn bolt_hand(lift: f32, share: f32) -> Vec3 {
        let radius = CONFIG.bolt_radius_m;
        let pivot = HANDLE - Vec3::X * radius;
        let turn = lift * CONFIG.bolt_lift_deg.to_radians();
        pivot + Vec3::new(turn.cos(), turn.sin(), 0.0) * radius + Vec3::Z * (share * TRAVEL)
    }

    #[test]
    fn a_bolt_turns_up_back_forward_and_down() {
        let mut t = ActionTravel::new();
        let k = ActionKind::Bolt;
        let step = |t: &mut ActionTravel, lift, share| {
            t.step(&inputs(k, bolt_hand(lift, share), false, true), &CONFIG)
        };
        assert!(
            t.step(&inputs(k, bolt_hand(0.0, 0.0), true, true), &CONFIG)
                .grabbed
        );
        step(&mut t, 0.5, 0.5);
        assert_eq!(t.position, 0.0, "a bolt turned halfway does not slide");
        assert!((t.lift - 0.5).abs() < 0.02);
        assert!(step(&mut t, 1.0, 0.0).lifted);
        assert!(step(&mut t, 1.0, 1.0).rear);
        step(&mut t, 0.0, 0.6);
        assert_eq!(t.lift, 1.0, "a bolt cannot turn down while back");
        assert!((t.position - 0.6).abs() < 1e-3);
        let e = step(&mut t, 1.0, 0.0);
        assert!(!e.cycled, "home but still up");
        assert!(!t.in_battery(&CONFIG));
        let e = step(&mut t, 0.0, 0.0);
        assert!(e.home && e.cycled);
        assert!(t.in_battery(&CONFIG));
    }

    #[test]
    fn a_bolt_let_go_open_stays_open_and_can_be_closed() {
        let mut t = ActionTravel::new();
        let k = ActionKind::Bolt;
        t.step(&inputs(k, bolt_hand(0.0, 0.0), true, true), &CONFIG);
        t.step(&inputs(k, bolt_hand(1.0, 0.0), false, true), &CONFIG);
        t.step(&inputs(k, bolt_hand(1.0, 1.0), false, true), &CONFIG);
        t.step(&inputs(k, bolt_hand(1.0, 1.0), false, false), &CONFIG);
        for _ in 0..10 {
            t.step(&inputs(k, bolt_hand(0.0, 0.0), false, false), &CONFIG);
        }
        assert_eq!((t.position, t.lift), (1.0, 1.0));
        let not_needed = TravelInputs {
            needed: false,
            ..inputs(k, bolt_hand(1.0, 1.0), true, true)
        };
        assert!(
            t.step(&not_needed, &CONFIG).grabbed,
            "an open bolt can be taken"
        );
        t.step(&inputs(k, bolt_hand(1.0, 0.0), false, true), &CONFIG);
        let e = t.step(&inputs(k, bolt_hand(0.0, 0.0), false, true), &CONFIG);
        assert!(e.cycled);
    }

    #[test]
    fn invalid_inputs_let_go() {
        let mut t = ActionTravel::new();
        t.step(&inputs(ActionKind::Slide, back(0.0), true, true), &CONFIG);
        let bad = TravelInputs {
            travel_m: 0.0,
            ..inputs(ActionKind::Slide, back(0.0), false, true)
        };
        t.step(&bad, &CONFIG);
        assert!(t.grip.is_none());
    }
}
