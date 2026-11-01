//! The root's per-tick trace: where the root went against what the previous tick asked of it, and
//! the proxy it moved through, for the recorder's controller state and the `rig: the root jumped`
//! log. The accounting is pure; [`Tracer::step`] returns what to log and the caller logs it.
//!
//! The on-foot task's post-call writes the wanted velocity in stages (the task's own, the stick's
//! in its place, the palms' push added, the chase added), and the physics step after the tick moves
//! the proxy by the last of them plus the velocity of the ground it stands on. So the root's move
//! between two ticks is explained by the earlier tick's wanted velocity; a move beyond it came from
//! something else (a collision, a depenetration, a proxy shape change, a moving surface the
//! surface velocity misses), and the trace's split names what the tick did ask for.

use std::time::{Duration, Instant};

use glam::{Vec2, Vec3};

/// The wanted horizontal velocity at each stage of a tick, world `(x, z)` metres per second, and
/// the full wanted velocity the tick left.
#[derive(Debug, Copy, Clone, PartialEq, Default)]
pub struct Split {
    /// As the movement task wrote it: the clip's root speed.
    pub task: Vec2,
    /// After the stick locomotion: the stick's velocity, or the task's while the animation moves
    /// the body.
    pub stick: Vec2,
    /// The palms' push the tick added.
    pub palms: Vec2,
    /// The chase the tick added.
    pub chase: Vec2,
    /// The wanted velocity the tick left, world metres per second.
    pub wanted: Vec3,
}

/// The proxy's support.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Default)]
pub enum Support {
    #[default]
    Unsupported,
    Sliding,
    Supported,
    /// A support value the game does not define.
    Unknown,
}

impl Support {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Unsupported => "unsupported",
            Self::Sliding => "sliding",
            Self::Supported => "supported",
            Self::Unknown => "unknown",
        }
    }
}

/// What the tick read off the character and its proxy.
#[derive(Debug, Copy, Clone, PartialEq, Default)]
pub struct ProxyRead {
    /// The proxy's own velocity, world metres per second.
    pub velocity: Vec3,
    /// The velocity of the surface the proxy stands on, world metres per second.
    pub surface_velocity: Vec3,
    pub support: Support,
    pub surface_dynamic: bool,
    pub ground_body: Option<u32>,
    /// The proxy's current, pending, and default states, as the game numbers them.
    pub state: i32,
    pub pending_state: i32,
    pub default_state: i32,
}

/// One tick's trace.
#[derive(Debug, Copy, Clone, PartialEq, Default)]
pub struct RootTrace {
    /// The on-foot ticks traced since the payload loaded.
    pub tick: u64,
    pub dt: f32,
    /// The root's world position as the tick found it.
    pub position: Vec3,
    /// How far the root moved since the previous traced tick, world metres; zero after a gap.
    pub moved: Vec3,
    /// How far the horizontal move is from what the previous tick's wanted velocity and its
    /// ground's velocity explain, metres.
    pub unexplained_m: f32,
    pub split: Split,
    pub proxy: ProxyRead,
    /// Whether the animation carried the body this tick.
    pub authored: bool,
    pub capsule_owned: bool,
    pub capsule_blocked: bool,
    /// The ticks whose move went further than their wanted velocity explains, since the payload
    /// loaded.
    pub jumps: u64,
}

/// A tick whose root went further than the previous tick's wanted velocity explains: what to log.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct Jump {
    /// The horizontal move, world `(x, z)` metres.
    pub moved: Vec2,
    /// What the previous tick's wanted velocity and its ground's velocity explain, world `(x, z)`
    /// metres.
    pub expected: Vec2,
    /// The previous tick's split, which asked for the move.
    pub asked: Split,
    /// Its proxy, as it read it.
    pub proxy: ProxyRead,
    /// The jumps the rate limit held back since the last one logged.
    pub suppressed: u64,
}

/// The tracer's history between ticks.
#[derive(Debug, Copy, Clone, PartialEq, Default)]
pub struct Tracer {
    previous: Option<RootTrace>,
    ticks: u64,
    jumps: u64,
    logged_at: Option<Instant>,
    suppressed: u64,
}

impl Tracer {
    pub const fn new() -> Self {
        Self {
            previous: None,
            ticks: 0,
            jumps: 0,
            logged_at: None,
            suppressed: 0,
        }
    }

    /// Trace a tick from what it read and asked for. Returns the trace and, at most once per
    /// [`LOG_INTERVAL`], a jump to log.
    pub fn step(&mut self, input: TickInput, now: Instant) -> (RootTrace, Option<Jump>) {
        self.ticks += 1;
        let dt = if input.dt.is_finite() {
            input.dt.max(0.0)
        } else {
            0.0
        };
        let (moved, accounting) = match self.previous {
            Some(previous) => {
                let moved = input.position - previous.position;
                let accounting = account(moved, &previous, dt);
                (moved, Some((accounting, previous)))
            }
            None => (Vec3::ZERO, None),
        };
        let mut jump = None;
        if let Some((accounting, previous)) = accounting
            && accounting.jumped()
        {
            self.jumps += 1;
            let due = self
                .logged_at
                .is_none_or(|at| now.saturating_duration_since(at) >= LOG_INTERVAL);
            if due {
                jump = Some(Jump {
                    moved: accounting.moved,
                    expected: accounting.expected,
                    asked: previous.split,
                    proxy: previous.proxy,
                    suppressed: self.suppressed,
                });
                self.logged_at = Some(now);
                self.suppressed = 0;
            } else {
                self.suppressed += 1;
            }
        }
        let trace = RootTrace {
            tick: self.ticks,
            dt,
            position: input.position,
            moved,
            unexplained_m: accounting.map_or(0.0, |(a, _)| a.unexplained_m()),
            split: input.split,
            proxy: input.proxy,
            authored: input.authored,
            capsule_owned: input.capsule_owned,
            capsule_blocked: input.capsule_blocked,
            jumps: self.jumps,
        };
        self.previous = Some(trace);
        (trace, jump)
    }

    /// Forget the previous tick: the task stopped, or the root was teleported, so the next move is
    /// not one tick's.
    pub fn forget(&mut self) {
        self.previous = None;
    }
}

/// What a tick hands the tracer.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct TickInput {
    pub position: Vec3,
    pub dt: f32,
    pub split: Split,
    pub proxy: ProxyRead,
    pub authored: bool,
    pub capsule_owned: bool,
    pub capsule_blocked: bool,
}

/// How far the root may move beyond what the wanted velocity explains in a tick before the move
/// counts as a jump, metres: above a stair's pop and the proxy's own settling.
pub const JUMP_M: f32 = 0.04;

/// How often a jump is logged at most.
pub const LOG_INTERVAL: Duration = Duration::from_millis(500);

/// How old the latest tick's trace may be for the recorder to sample it.
pub const STALE_AFTER: Duration = Duration::from_millis(250);

/// A move against what explains it.
#[derive(Debug, Copy, Clone, PartialEq)]
struct Accounting {
    moved: Vec2,
    expected: Vec2,
}

impl Accounting {
    fn unexplained_m(&self) -> f32 {
        (self.moved - self.expected).length()
    }

    /// Whether the root went further than the wanted velocity explains. A move that falls short
    /// (a wall, a slope the proxy will not climb) is the proxy doing its job, not a jump.
    fn jumped(&self) -> bool {
        self.moved.length() - self.expected.length() > JUMP_M && self.unexplained_m() > JUMP_M
    }
}

/// The horizontal move `moved` against what `previous` asked for over `dt` seconds: its wanted
/// velocity and its ground's.
fn account(moved: Vec3, previous: &RootTrace, dt: f32) -> Accounting {
    let velocity = previous.split.wanted + previous.proxy.surface_velocity;
    Accounting {
        moved: Vec2::new(moved.x, moved.z),
        expected: Vec2::new(velocity.x, velocity.z) * dt,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DT: f32 = 1.0 / 30.0;

    fn input(position: Vec3, wanted: Vec3) -> TickInput {
        TickInput {
            position,
            dt: DT,
            split: Split {
                wanted,
                ..Split::default()
            },
            proxy: ProxyRead::default(),
            authored: false,
            capsule_owned: false,
            capsule_blocked: false,
        }
    }

    #[test]
    fn a_move_the_wanted_velocity_explains_is_no_jump() {
        let mut tracer = Tracer::new();
        let now = Instant::now();
        let wanted = Vec3::new(0.0, 0.0, -3.0);
        let mut position = Vec3::ZERO;
        for _ in 0..10 {
            let (trace, jump) = tracer.step(input(position, wanted), now);
            assert_eq!(jump, None);
            assert!(trace.unexplained_m < 1e-4);
            position += wanted * DT;
        }
        assert_eq!(tracer.step(input(position, wanted), now).0.jumps, 0);
    }

    #[test]
    fn a_move_the_wanted_velocity_does_not_explain_is_a_jump_with_the_asking_split() {
        let mut tracer = Tracer::new();
        let now = Instant::now();
        let asked = TickInput {
            split: Split {
                palms: Vec2::new(0.0, 0.5),
                ..Split::default()
            },
            ..input(Vec3::ZERO, Vec3::ZERO)
        };
        tracer.step(asked, now);
        // Standing still, the root moved eleven centimetres in one tick.
        let (trace, jump) = tracer.step(input(Vec3::new(0.0, 0.0, -0.11), Vec3::ZERO), now);
        let jump = jump.expect("a jump");
        assert!((trace.unexplained_m - 0.11).abs() < 1e-5);
        assert_eq!(trace.jumps, 1);
        assert_eq!(jump.asked.palms, Vec2::new(0.0, 0.5));
        assert_eq!(jump.expected, Vec2::ZERO);
    }

    #[test]
    fn falling_short_of_the_wanted_velocity_is_no_jump() {
        let mut tracer = Tracer::new();
        let now = Instant::now();
        // Walking into a wall: asked for five metres a second, went nowhere.
        let wanted = Vec3::new(5.0, 0.0, 0.0);
        tracer.step(input(Vec3::ZERO, wanted), now);
        let (trace, jump) = tracer.step(input(Vec3::ZERO, wanted), now);
        assert_eq!(jump, None);
        assert!(trace.unexplained_m > JUMP_M);
        assert_eq!(trace.jumps, 0);
    }

    #[test]
    fn the_ground_velocity_explains_a_moving_platform() {
        let mut tracer = Tracer::new();
        let now = Instant::now();
        let platform = Vec3::new(10.0, 0.0, 0.0);
        let on_deck = |position| TickInput {
            proxy: ProxyRead {
                surface_velocity: platform,
                ..ProxyRead::default()
            },
            ..input(position, Vec3::ZERO)
        };
        tracer.step(on_deck(Vec3::ZERO), now);
        let (_, jump) = tracer.step(on_deck(platform * DT), now);
        assert_eq!(jump, None);
    }

    #[test]
    fn jumps_are_logged_at_most_once_per_interval_and_count_what_was_held_back() {
        let mut tracer = Tracer::new();
        let start = Instant::now();
        let mut logged = Vec::new();
        for i in 0..12u32 {
            let z = if i % 2 == 0 { 0.0 } else { -0.11 };
            let now = start + Duration::from_millis(u64::from(i) * 100);
            if let (_, Some(jump)) = tracer.step(input(Vec3::new(0.0, 0.0, z), Vec3::ZERO), now) {
                logged.push((i, jump.suppressed));
            }
        }
        // Every tick after the first jumps; one is logged per half second.
        assert_eq!(logged, vec![(1, 0), (6, 4), (11, 4)]);
        assert_eq!(tracer.jumps, 11);
    }

    #[test]
    fn a_gap_forgets_the_previous_tick() {
        let mut tracer = Tracer::new();
        let now = Instant::now();
        tracer.step(input(Vec3::ZERO, Vec3::ZERO), now);
        tracer.forget();
        let (trace, jump) = tracer.step(input(Vec3::new(50.0, 0.0, 0.0), Vec3::ZERO), now);
        assert_eq!(jump, None);
        assert_eq!(trace.moved, Vec3::ZERO);
    }
}
