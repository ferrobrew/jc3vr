//! The fade of the HumanIK targets on the bones the procedural body owns. Pure.
//!
//! The gate scales each owned effector's target reach and pull by what is left of its region's
//! weight, so the solve after the gate moves the written pose only as much as the animation still
//! owns it. A target the game or the mod queued this tick arrives at full strength; one nobody
//! queued again still holds what the last gate wrote, and scaling that again would compound the
//! fade tick on tick, taking a half-weight target to nothing. [`TargetFade`] remembers what it
//! wrote and the value it scaled, and scales the value a slot held before the fade, so the result
//! is the queued value times the keep, whatever the history.
//!
//! The live values the engine eases toward the targets are held at most at the faded target, so a
//! fade takes effect at once rather than at the engine's easing pace, and the pull, which the
//! engine copies from its target unchanged, is the faded target.

use jc3gi::animation::ik::HumanIK;

/// One effector slot's targets and live values, as the solver holds them.
#[derive(Debug, Copy, Clone, PartialEq, Default)]
pub struct Slot {
    pub target_reach_t: f32,
    pub target_reach_r: f32,
    pub target_pull: f32,
    pub reach_t: f32,
    pub reach_r: f32,
    pub pull: f32,
}

/// What to fade on a slot.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum Fade {
    /// The reaches and the pull: a bone the procedural body owns.
    All,
    /// The pull alone: a target that may still reach its bone but not drag the owned bones after it
    /// (the head's).
    Pull,
}

/// The fade's memory of what it wrote, per effector slot.
#[derive(Debug, Clone, PartialEq)]
pub struct TargetFade {
    records: [[Option<Record>; CHANNELS]; SLOTS],
}

impl TargetFade {
    pub const fn new() -> Self {
        Self {
            records: [[None; CHANNELS]; SLOTS],
        }
    }

    /// Forget what was written: the next fade takes every slot as freshly queued.
    pub fn forget(&mut self) {
        *self = Self::new();
    }

    /// `slot` (effector `index`) with its fadeable values scaled by `keep`, `0..=1`.
    pub fn fade(&mut self, index: usize, slot: Slot, keep: f32, fade: Fade) -> Slot {
        let Some(records) = self.records.get_mut(index) else {
            return slot;
        };
        let keep = if keep.is_finite() {
            keep.clamp(0.0, 1.0)
        } else {
            0.0
        };
        let [reach_t, reach_r, pull] = records;
        let mut out = slot;
        if fade == Fade::All {
            out.target_reach_t = scale(reach_t, slot.target_reach_t, keep);
            out.target_reach_r = scale(reach_r, slot.target_reach_r, keep);
            out.reach_t = slot.reach_t.min(out.target_reach_t);
            out.reach_r = slot.reach_r.min(out.target_reach_r);
        }
        out.target_pull = scale(pull, slot.target_pull, keep);
        out.pull = out.target_pull;
        out
    }
}

impl Default for TargetFade {
    fn default() -> Self {
        Self::new()
    }
}

/// The number of effector slots the solver has.
const SLOTS: usize = HumanIK::EFFECTOR_SLOTS as usize;

/// The faded target channels: the translation reach, the rotation reach, and the pull.
const CHANNELS: usize = 3;

/// What a fade wrote to a channel, and the value it scaled to get it.
#[derive(Debug, Copy, Clone, PartialEq)]
struct Record {
    original: f32,
    written: f32,
}

/// `current` scaled by `keep`, from the value before the last fade when `current` is still what
/// that fade wrote.
fn scale(record: &mut Option<Record>, current: f32, keep: f32) -> f32 {
    let original = match *record {
        Some(r) if r.written.to_bits() == current.to_bits() => r.original,
        _ => current,
    };
    let written = original * keep;
    *record = Some(Record { original, written });
    written
}

#[cfg(test)]
mod tests {
    use super::*;

    fn queued(value: f32) -> Slot {
        Slot {
            target_reach_t: value,
            target_reach_r: value,
            target_pull: value,
            reach_t: value,
            reach_r: value,
            pull: value,
        }
    }

    #[test]
    fn a_target_nobody_queues_again_fades_in_proportion_rather_than_compounding() {
        let mut fade = TargetFade::new();
        let mut slot = queued(0.8);
        for _ in 0..10 {
            slot = fade.fade(3, slot, 0.5, Fade::All);
            assert!((slot.target_reach_t - 0.4).abs() < 1e-6, "{slot:?}");
            assert!((slot.pull - 0.4).abs() < 1e-6);
            assert!(slot.reach_t <= slot.target_reach_t);
        }
        // Queued afresh at a new value, the fade starts from it.
        let slot = fade.fade(3, queued(0.6), 0.5, Fade::All);
        assert!((slot.target_reach_r - 0.3).abs() < 1e-6);
    }

    #[test]
    fn a_pull_fade_leaves_the_reach() {
        let mut fade = TargetFade::new();
        let slot = fade.fade(15, queued(1.0), 0.0, Fade::Pull);
        assert_eq!(slot.target_reach_t, 1.0);
        assert_eq!(slot.reach_t, 1.0);
        assert_eq!(slot.target_pull, 0.0);
        assert_eq!(slot.pull, 0.0);
    }

    #[test]
    fn full_ownership_leaves_nothing_and_none_leaves_everything() {
        let mut fade = TargetFade::new();
        assert_eq!(fade.fade(0, queued(0.7), 0.0, Fade::All), queued(0.0));
        let mut fade = TargetFade::new();
        assert_eq!(fade.fade(0, queued(0.7), 1.0, Fade::All), queued(0.7));
        assert_eq!(fade.fade(99, queued(0.7), 0.0, Fade::All), queued(0.7));
    }
}
