//! Deliberate roof-mantle entry and grip-open exit from an existing native hang. Pure.
//!
//! [`Tracker::step`] reads the game's stunt position, the held grips, and a completed roof
//! mantle, and returns one [`Decision`]. Physical grips do not request the animated hang.

/// A stunt position the game holds the player in.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum StuntKind {
    /// Standing in a vehicle's stunt area: a car's roof, a boat's deck, a plane's wing.
    Standing,
    /// Hanging from a helicopter.
    Hanging,
}

impl StuntKind {
    pub const fn label(self) -> &'static str {
        match self {
            StuntKind::Standing => "standing on a vehicle",
            StuntKind::Hanging => "hanging from a helicopter",
        }
    }
}

/// One input tick, as the tracker reads it.
#[derive(Debug, Copy, Clone, PartialEq, Default)]
pub struct Tick {
    /// The stunt position the game has the player in, if any.
    pub position: Option<StuntKind>,
    /// Whether each hand's grip is held, indexed by [`crate::controllers::frame::Hand::index`].
    pub held: [bool; 2],
    /// A mantle that landed on a vehicle since the last tick: how far the feet are from the
    /// vehicle's nearest stunt area, metres, or `None` for a vehicle without one.
    pub roof_landing: Option<Option<f32>>,
    /// The tick's length, seconds.
    pub dt: f32,
}

/// The tracker's tuning, from [`StuntConfig`](crate::controllers::physical_hands::stunt::config::StuntConfig).
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct Tuning {
    pub enter_roof: bool,
    pub roof_offer_m: f32,
    pub let_go: bool,
    pub release_grace_s: f32,
    pub retry_s: f32,
}

/// What to do on this tick.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum Decision {
    /// Nothing to change.
    Hold,
    /// Take the player into the stunt area the mantle landed by.
    EnterRoof,
    /// Let go of the helicopter: every grip opened in its hang.
    LetGo,
    /// The game kept the player in the hang after a let-go.
    LetGoIgnored,
}

/// The tracker's memory between ticks.
#[derive(Debug, Copy, Clone, PartialEq, Default)]
pub struct Tracker {
    /// The position on the last tick.
    position: Option<StuntKind>,
    /// Whether a grip has held in this hang, so that opening every grip lets go: a hang the
    /// grapple or the game entered waits for the player to take hold.
    armed: bool,
    /// How long every grip has been open in an armed hang, seconds.
    open_s: f32,
    /// How long ago the let-go was asked for in this hang, seconds, if it was.
    let_go_s: Option<f32>,
    /// Whether the game keeping the hang after a let-go was reported.
    ignored: bool,
    /// How long until an entry may be tried again, seconds.
    retry_in_s: f32,
}

impl Tracker {
    pub const fn new() -> Self {
        Self {
            position: None,
            armed: false,
            open_s: 0.0,
            let_go_s: None,
            ignored: false,
            retry_in_s: 0.0,
        }
    }

    /// Whether a grip has held in the current hang, so that letting go of every grip lets go.
    pub fn armed(&self) -> bool {
        self.armed
    }

    /// Read one tick and decide.
    pub fn step(&mut self, tick: &Tick, tuning: &Tuning) -> Decision {
        let dt = if tick.dt.is_finite() {
            tick.dt.max(0.0)
        } else {
            0.0
        };
        let any_held = tick.held.iter().any(|&h| h);
        if tick.position != self.position {
            self.position = tick.position;
            self.armed = tick.position == Some(StuntKind::Hanging) && any_held;
            self.open_s = 0.0;
            self.let_go_s = None;
            self.ignored = false;
        }
        match tick.position {
            Some(StuntKind::Hanging) => self.hanging(any_held, dt, tuning),
            Some(StuntKind::Standing) => Decision::Hold,
            None => self.free(tick, dt, tuning),
        }
    }

    /// The game refused an entry: wait before trying again.
    pub fn refused(&mut self, tuning: &Tuning) {
        self.retry_in_s = tuning.retry_s.max(0.0);
    }

    fn hanging(&mut self, any_held: bool, dt: f32, tuning: &Tuning) -> Decision {
        if let Some(since) = self.let_go_s.as_mut() {
            *since += dt;
            if *since >= LET_GO_TIMEOUT_S && !self.ignored {
                self.ignored = true;
                return Decision::LetGoIgnored;
            }
            return Decision::Hold;
        }
        if any_held {
            self.armed = true;
            self.open_s = 0.0;
            return Decision::Hold;
        }
        if !self.armed || !tuning.let_go {
            return Decision::Hold;
        }
        self.open_s += dt;
        if self.open_s >= tuning.release_grace_s.max(0.0) {
            self.let_go_s = Some(0.0);
            return Decision::LetGo;
        }
        Decision::Hold
    }

    fn free(&mut self, tick: &Tick, dt: f32, tuning: &Tuning) -> Decision {
        if self.retry_in_s > 0.0 {
            self.retry_in_s = (self.retry_in_s - dt).max(0.0);
            return Decision::Hold;
        }
        if tuning.enter_roof
            && let Some(Some(distance)) = tick.roof_landing
            && distance <= tuning.roof_offer_m
        {
            return Decision::EnterRoof;
        }
        Decision::Hold
    }
}

/// How long the hang may last after a let-go before the game is reported to have kept it,
/// seconds.
const LET_GO_TIMEOUT_S: f32 = 1.0;

#[cfg(test)]
mod tests {
    use super::*;

    const DT: f32 = 1.0 / 30.0;

    fn tuning() -> Tuning {
        Tuning {
            enter_roof: true,
            roof_offer_m: 0.6,
            let_go: true,
            release_grace_s: 0.2,
            retry_s: 1.0,
        }
    }

    fn run(tracker: &mut Tracker, tick: &Tick, ticks: usize) -> Vec<Decision> {
        (0..ticks).map(|_| tracker.step(tick, &tuning())).collect()
    }

    #[test]
    fn held_grips_without_a_completed_mantle_do_not_request_entry() {
        let mut tracker = Tracker::new();
        let tick = Tick {
            held: [true; 2],
            dt: DT,
            ..Tick::default()
        };
        assert!(
            run(&mut tracker, &tick, 100)
                .iter()
                .all(|d| *d == Decision::Hold)
        );
    }

    #[test]
    fn opening_every_grip_in_the_hang_lets_go_once() {
        let mut tracker = Tracker::new();
        let holding = Tick {
            position: Some(StuntKind::Hanging),
            held: [true, true],
            dt: DT,
            ..Tick::default()
        };
        run(&mut tracker, &holding, 10);
        assert!(tracker.armed());
        // One hand moving to another place on the skid keeps the hang.
        let one = Tick {
            held: [false, true],
            ..holding
        };
        assert!(
            run(&mut tracker, &one, 30)
                .iter()
                .all(|d| *d == Decision::Hold)
        );
        let open = Tick {
            held: [false, false],
            ..holding
        };
        let decisions = run(&mut tracker, &open, 60);
        let first = decisions
            .iter()
            .position(|d| *d == Decision::LetGo)
            .expect("the hang lets go");
        assert!(first as f32 * DT >= 0.2 - DT, "{first}");
        assert_eq!(
            decisions.iter().filter(|d| **d == Decision::LetGo).count(),
            1
        );
        assert_eq!(
            decisions
                .iter()
                .filter(|d| **d == Decision::LetGoIgnored)
                .count(),
            1
        );
    }

    #[test]
    fn a_hang_entered_without_a_grip_waits_for_the_player_to_take_hold() {
        let mut tracker = Tracker::new();
        let reeled = Tick {
            position: Some(StuntKind::Hanging),
            dt: DT,
            ..Tick::default()
        };
        assert!(
            run(&mut tracker, &reeled, 60)
                .iter()
                .all(|d| *d == Decision::Hold)
        );
        assert!(!tracker.armed());
        let held = Tick {
            held: [false, true],
            ..reeled
        };
        tracker.step(&held, &tuning());
        assert!(tracker.armed());
        assert!(run(&mut tracker, &reeled, 30).contains(&Decision::LetGo));
    }

    #[test]
    fn with_the_let_go_off_the_hang_holds_with_the_grips_open() {
        let mut tracker = Tracker::new();
        let holding = Tick {
            position: Some(StuntKind::Hanging),
            held: [true, false],
            dt: DT,
            ..Tick::default()
        };
        tracker.step(&holding, &tuning());
        let no_let_go = Tuning {
            let_go: false,
            ..tuning()
        };
        let open = Tick {
            held: [false; 2],
            ..holding
        };
        assert!(
            (0..60)
                .map(|_| tracker.step(&open, &no_let_go))
                .all(|d| d == Decision::Hold)
        );
    }

    #[test]
    fn standing_on_a_roof_needs_no_grip() {
        let mut tracker = Tracker::new();
        let standing = Tick {
            position: Some(StuntKind::Standing),
            dt: DT,
            ..Tick::default()
        };
        assert!(
            run(&mut tracker, &standing, 60)
                .iter()
                .all(|d| *d == Decision::Hold)
        );
    }

    #[test]
    fn a_mantle_onto_a_roof_enters_its_stunt_area_when_near_one() {
        let mut tracker = Tracker::new();
        let near = Tick {
            roof_landing: Some(Some(0.3)),
            dt: DT,
            ..Tick::default()
        };
        assert_eq!(tracker.step(&near, &tuning()), Decision::EnterRoof);
        let far = Tick {
            roof_landing: Some(Some(1.5)),
            ..near
        };
        assert_eq!(tracker.step(&far, &tuning()), Decision::Hold);
        let none = Tick {
            roof_landing: Some(None),
            ..near
        };
        assert_eq!(tracker.step(&none, &tuning()), Decision::Hold);
        let off = Tuning {
            enter_roof: false,
            ..tuning()
        };
        assert_eq!(tracker.step(&near, &off), Decision::Hold);
    }

    #[test]
    fn a_refused_roof_entry_waits_before_trying_again() {
        let mut tracker = Tracker::new();
        let near = Tick {
            roof_landing: Some(Some(0.3)),
            dt: DT,
            ..Tick::default()
        };
        assert_eq!(tracker.step(&near, &tuning()), Decision::EnterRoof);
        tracker.refused(&tuning());
        let after = run(&mut tracker, &near, 60);
        let next = after
            .iter()
            .position(|d| *d == Decision::EnterRoof)
            .expect("tried again");
        assert!(next as f32 * DT >= 1.0, "{next}");
    }

    #[test]
    fn a_bad_tick_length_does_not_advance_hang_release() {
        let mut tracker = Tracker::new();
        let holding = Tick {
            position: Some(StuntKind::Hanging),
            held: [true, false],
            dt: DT,
            ..Tick::default()
        };
        tracker.step(&holding, &tuning());
        let open = Tick {
            held: [false; 2],
            dt: f32::NAN,
            ..holding
        };
        assert!(
            run(&mut tracker, &open, 100)
                .iter()
                .all(|d| *d == Decision::Hold)
        );
    }
}
