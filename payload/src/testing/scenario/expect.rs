//! Evaluating an expectation over the frames of a replay: each frame's value is held to the
//! expectation's condition, and the check decides from the frames' answers whether it passed.

use std::fmt;

use serde::Deserialize;

use crate::testing::signals::Value;

/// Over which frames a condition must hold.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Check {
    /// On at least one frame.
    Eventually,
    /// On every frame the value was present, and on at least one.
    Always,
    /// On no frame the value was present.
    Never,
    /// On the last frame.
    AtEnd,
}

/// What a frame's value must be.
#[derive(Debug, Copy, Clone, PartialEq)]
pub(crate) enum Condition {
    Is(bool),
    Below(f64),
    Above(f64),
    AtLeast(f64),
    AtMost(f64),
    Equals(f64),
}

impl Condition {
    /// Whether `value` meets the condition. A missing value meets none, and a value of the other
    /// kind (a number for `Is`) meets none either.
    pub(crate) fn holds(self, value: &Value) -> bool {
        match (self, value) {
            (Condition::Is(want), Value::Bool(b)) => *b == want,
            (Condition::Below(limit), Value::Number(n)) => *n < limit,
            (Condition::Above(limit), Value::Number(n)) => *n > limit,
            (Condition::AtLeast(limit), Value::Number(n)) => *n >= limit,
            (Condition::AtMost(limit), Value::Number(n)) => *n <= limit,
            (Condition::Equals(want), Value::Number(n)) => (*n - want).abs() <= EQUALS_TOLERANCE,
            _ => false,
        }
    }

    /// The condition in words, for a default expectation name.
    pub(crate) fn describe(self) -> String {
        match self {
            Condition::Is(true) => "is true".to_owned(),
            Condition::Is(false) => "is false".to_owned(),
            Condition::Below(n) => format!("below {n}"),
            Condition::Above(n) => format!("above {n}"),
            Condition::AtLeast(n) => format!("at least {n}"),
            Condition::AtMost(n) => format!("at most {n}"),
            Condition::Equals(n) => format!("equals {n}"),
        }
    }
}

/// The verdict on one expectation.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Verdict {
    pub(crate) passed: bool,
    /// What decided it.
    pub(crate) detail: String,
}

impl fmt::Display for Verdict {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let word = if self.passed { "PASS" } else { "FAIL" };
        write!(f, "{word}: {}", self.detail)
    }
}

/// One expectation's record over the frames observed so far.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Tracker {
    check: Check,
    condition: Condition,
    /// Frames observed, present or not.
    frames: u32,
    /// Frames with a value present.
    present: u32,
    /// The first frame the condition held on.
    first_held: Option<(u32, Value)>,
    /// The first frame with a present value the condition did not hold on.
    first_broken: Option<(u32, Value)>,
    last: Option<Value>,
}

impl Tracker {
    pub(crate) fn new(check: Check, condition: Condition) -> Self {
        Self {
            check,
            condition,
            frames: 0,
            present: 0,
            first_held: None,
            first_broken: None,
            last: None,
        }
    }

    /// Fold in the next frame's value.
    pub(crate) fn observe(&mut self, value: Value) {
        let frame = self.frames;
        self.frames += 1;
        if value != Value::Missing {
            self.present += 1;
            if self.condition.holds(&value) {
                self.first_held.get_or_insert((frame, value.clone()));
            } else {
                self.first_broken.get_or_insert((frame, value.clone()));
            }
        }
        self.last = Some(value);
    }

    /// The verdict on the frames observed.
    pub(crate) fn verdict(&self) -> Verdict {
        let condition = self.condition.describe();
        let pass = |detail: String| Verdict {
            passed: true,
            detail,
        };
        let fail = |detail: String| Verdict {
            passed: false,
            detail,
        };
        if self.frames == 0 {
            return fail("no frame was observed".to_owned());
        }
        match self.check {
            Check::Eventually => match &self.first_held {
                Some((frame, value)) => pass(format!("{condition} from frame {frame} ({value})")),
                None => fail(format!(
                    "never {condition} over {} frames; the last value was {}",
                    self.frames,
                    self.last_value()
                )),
            },
            Check::Always => match &self.first_broken {
                Some((frame, value)) => fail(format!("not {condition} on frame {frame} ({value})")),
                None if self.present == 0 => {
                    fail(format!("never present over {} frames", self.frames))
                }
                None => pass(format!("{condition} on all {} frames", self.present)),
            },
            Check::Never => match &self.first_held {
                Some((frame, value)) => fail(format!("{condition} on frame {frame} ({value})")),
                None => pass(format!("never {condition} over {} frames", self.frames)),
            },
            Check::AtEnd => {
                let last = self.last.clone().unwrap_or(Value::Missing);
                if self.condition.holds(&last) {
                    pass(format!("{condition} at the end ({last})"))
                } else {
                    fail(format!("not {condition} at the end ({last})"))
                }
            }
        }
    }

    fn last_value(&self) -> Value {
        self.last.clone().unwrap_or(Value::Missing)
    }
}

/// How close a number must be to an `equals` condition's value.
const EQUALS_TOLERANCE: f64 = 1e-9;

#[cfg(test)]
mod tests {
    use super::*;

    fn run(check: Check, condition: Condition, values: &[Value]) -> Verdict {
        let mut tracker = Tracker::new(check, condition);
        for value in values {
            tracker.observe(value.clone());
        }
        tracker.verdict()
    }

    const T: Value = Value::Bool(true);
    const F: Value = Value::Bool(false);
    const M: Value = Value::Missing;

    fn n(value: f64) -> Value {
        Value::Number(value)
    }

    #[test]
    fn eventually_passes_once_the_condition_holds() {
        let verdict = run(Check::Eventually, Condition::Is(true), &[F, M, T, F]);
        assert!(verdict.passed);
        assert_eq!(verdict.detail, "is true from frame 2 (true)");
        assert!(!run(Check::Eventually, Condition::Is(true), &[F, F]).passed);
    }

    #[test]
    fn always_skips_missing_frames_but_needs_one_present() {
        assert!(
            run(
                Check::Always,
                Condition::Below(0.001),
                &[M, n(0.0002), M, n(0.0009)]
            )
            .passed
        );
        let broken = run(
            Check::Always,
            Condition::Below(0.001),
            &[n(0.0), n(0.02), n(0.0)],
        );
        assert!(!broken.passed);
        assert_eq!(broken.detail, "not below 0.001 on frame 1 (0.02)");
        let never = run(Check::Always, Condition::Below(0.001), &[M, M]);
        assert!(!never.passed);
        assert_eq!(never.detail, "never present over 2 frames");
    }

    #[test]
    fn never_fails_on_the_first_frame_it_holds() {
        assert!(run(Check::Never, Condition::Is(true), &[F, M, F]).passed);
        let verdict = run(Check::Never, Condition::Is(true), &[F, T]);
        assert!(!verdict.passed);
        assert_eq!(verdict.detail, "is true on frame 1 (true)");
    }

    #[test]
    fn at_end_reads_the_last_frame_only() {
        assert!(run(Check::AtEnd, Condition::AtLeast(1.0), &[n(0.0), n(3.0)]).passed);
        assert!(!run(Check::AtEnd, Condition::AtMost(0.0), &[n(0.0), n(1.0)]).passed);
        assert!(!run(Check::AtEnd, Condition::AtLeast(0.0), &[n(1.0), M]).passed);
    }

    #[test]
    fn nothing_observed_fails_every_check() {
        for check in [Check::Eventually, Check::Always, Check::Never, Check::AtEnd] {
            assert!(!run(check, Condition::Is(true), &[]).passed, "{check:?}");
        }
    }

    #[test]
    fn a_value_of_the_other_kind_meets_no_condition() {
        assert!(!Condition::Is(true).holds(&n(1.0)));
        assert!(!Condition::AtLeast(0.0).holds(&T));
        assert!(Condition::Equals(0.3).holds(&n(0.1 + 0.2)));
    }
}
