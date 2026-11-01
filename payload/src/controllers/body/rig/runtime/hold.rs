//! The last accepted fit's survival across fits that miss the feasibility tolerance.
//!
//! A rejected fit keeps presenting the last accepted one for a short grace, so a single deep reach
//! or fast duck neither snaps the drawn body to the animation nor drops root following. Rejection
//! past the grace fades the presentation out over [`BLEND_S`], and an accepted fit fades it back in
//! over the same time. Time is the gate's own step, never the wall clock.

/// How long rejected fits keep presenting the last accepted fit at its full weight, in seconds.
pub(crate) const GRACE_S: f32 = 0.15;
/// How long the presentation takes to fade between the fit and the animation, in seconds.
pub(crate) const BLEND_S: f32 = 0.3;

/// The fit to present after one gate, and how much of it.
#[derive(Debug, Copy, Clone, PartialEq)]
pub(crate) struct Presentation<T> {
    pub fit: T,
    /// The fit's share of the drawn pose, `0..=1`; the animation has the rest.
    pub weight: f32,
    pub phase: Phase,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub(crate) enum Phase {
    /// The presented fit is this gate's own.
    Accepted,
    /// This gate's fit was rejected; the last accepted fit stands in at full weight.
    Grace,
    /// Rejection outlasted the grace; the last accepted fit fades toward the animation.
    FadingOut,
}

/// The last accepted fit, how long fits have been rejected since, and the presentation weight.
#[derive(Debug, Copy, Clone, PartialEq)]
pub(crate) struct Hold<T> {
    held: Option<T>,
    rejected_s: f32,
    weight: f32,
    phase: Phase,
}

impl<T: Copy> Hold<T> {
    pub(crate) const fn new() -> Self {
        Self {
            held: None,
            rejected_s: 0.0,
            weight: 1.0,
            phase: Phase::Accepted,
        }
    }

    /// Hold this gate's accepted fit, fading back in from wherever a rejection left the weight.
    pub(crate) fn accept(&mut self, fit: T, dt: f32) -> Presentation<T> {
        self.held = Some(fit);
        self.rejected_s = 0.0;
        self.weight = (self.weight + step(dt) / BLEND_S).min(1.0);
        self.phase = Phase::Accepted;
        Presentation {
            fit,
            weight: self.weight,
            phase: self.phase,
        }
    }

    /// Present the held fit in place of this gate's rejected one. Nothing is presented without a
    /// held fit, or once the fade reaches the animation, which also lets the held fit go.
    pub(crate) fn reject(&mut self, dt: f32) -> Option<Presentation<T>> {
        let fit = self.held?;
        self.rejected_s += step(dt);
        self.phase = if self.rejected_s <= GRACE_S {
            Phase::Grace
        } else {
            self.weight = (self.weight - step(dt) / BLEND_S).max(0.0);
            Phase::FadingOut
        };
        if self.weight <= 0.0 {
            self.held = None;
            return None;
        }
        Some(Presentation {
            fit,
            weight: self.weight,
            phase: self.phase,
        })
    }

    /// Let the held fit go when its owner or tracking ends. The weight stays, so a fit accepted
    /// after a fade-out still fades in.
    pub(crate) fn forget(&mut self) {
        self.held = None;
        self.rejected_s = 0.0;
    }

    /// Re-express the held fit, keeping its weight and phase.
    pub(crate) fn map_held(&mut self, f: impl FnOnce(T) -> T) {
        self.held = self.held.map(f);
    }

    /// The fit standing after the latest gate, at its weight.
    pub(crate) fn presented(&self) -> Option<Presentation<T>> {
        Some(Presentation {
            fit: self.held?,
            weight: self.weight,
            phase: self.phase,
        })
    }

    #[cfg(test)]
    pub(crate) fn held_mut_for_test(&mut self) -> Option<&mut T> {
        self.held.as_mut()
    }
}

impl<T: Copy> Default for Hold<T> {
    fn default() -> Self {
        Self::new()
    }
}

/// A gate step that cannot run the clock backward or stall it on a non-finite value.
fn step(dt: f32) -> f32 {
    if dt.is_finite() { dt.max(0.0) } else { 0.0 }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DT: f32 = 1.0 / 60.0;

    #[test]
    fn a_rejected_tick_presents_the_last_accepted_fit_at_full_weight() {
        let mut hold = Hold::new();
        hold.accept(1, DT);
        let presented = hold.reject(DT).expect("the accepted fit stands in");
        assert_eq!(presented.fit, 1);
        assert_eq!(presented.weight, 1.0);
        assert_eq!(presented.phase, Phase::Grace);
        let recovered = hold.accept(2, DT);
        assert_eq!(recovered.fit, 2);
        assert_eq!(recovered.weight, 1.0);
        assert_eq!(recovered.phase, Phase::Accepted);
    }

    #[test]
    fn nothing_is_presented_for_a_rejection_without_an_accepted_fit() {
        let mut hold = Hold::<u32>::new();
        assert_eq!(hold.reject(DT), None);
        assert_eq!(hold.presented(), None);
    }

    #[test]
    fn sustained_rejection_fades_out_monotonically_after_the_grace() {
        let mut hold = Hold::new();
        hold.accept(7, DT);
        let mut elapsed = 0.0;
        let mut last = 1.0;
        let mut faded_at = None;
        for _ in 0..120 {
            elapsed += DT;
            match hold.reject(DT) {
                Some(presented) => {
                    assert_eq!(presented.fit, 7);
                    assert!(presented.weight <= last);
                    if elapsed <= GRACE_S - 1.0e-4 {
                        assert_eq!(presented.weight, 1.0);
                        assert_eq!(presented.phase, Phase::Grace);
                    } else if elapsed > GRACE_S + 1.0e-4 {
                        assert_eq!(presented.phase, Phase::FadingOut);
                        // The fade is gradual: no single step drops more than one step's share.
                        assert!(last - presented.weight <= DT / BLEND_S + 1.0e-5);
                    }
                    last = presented.weight;
                }
                None => {
                    faded_at = Some(elapsed);
                    break;
                }
            }
        }
        let faded_at = faded_at.expect("sustained rejection reaches the animation");
        assert!(
            (faded_at - (GRACE_S + BLEND_S)).abs() <= 2.0 * DT,
            "faded at {faded_at}"
        );
        assert_eq!(hold.presented(), None);
    }

    #[test]
    fn recovery_fades_back_in_from_the_animation() {
        let mut hold = Hold::new();
        hold.accept(1, DT);
        while hold.reject(DT).is_some() {}
        let mut last = 0.0;
        let mut steps = 0;
        loop {
            let presented = hold.accept(2, DT);
            assert!(presented.weight > last);
            assert!(presented.weight - last <= DT / BLEND_S + 1.0e-5);
            last = presented.weight;
            steps += 1;
            if presented.weight >= 1.0 {
                break;
            }
        }
        assert!((steps as f32 * DT - BLEND_S).abs() <= 2.0 * DT, "{steps}");
    }

    #[test]
    fn recovery_mid_fade_resumes_from_the_reached_weight() {
        let mut hold = Hold::new();
        hold.accept(1, DT);
        let mut weight = 1.0;
        while weight > 0.5 {
            weight = hold.reject(DT).expect("still fading").weight;
        }
        let recovered = hold.accept(2, DT);
        assert!((recovered.weight - (weight + DT / BLEND_S)).abs() <= 1.0e-6);
        // A single later miss starts a fresh grace at the reached weight rather than resuming the fade.
        let missed = hold.reject(DT).expect("the new fit stands in");
        assert_eq!(missed.fit, 2);
        assert_eq!(missed.phase, Phase::Grace);
        assert_eq!(missed.weight, recovered.weight);
    }

    #[test]
    fn forgetting_drops_the_fit_but_keeps_the_fade() {
        let mut hold = Hold::new();
        hold.accept(1, DT);
        while hold.reject(DT).is_some() {}
        hold.forget();
        assert_eq!(hold.presented(), None);
        assert!(hold.accept(3, DT).weight < 1.0);
    }

    #[test]
    fn a_non_finite_step_does_not_advance_the_grace() {
        let mut hold = Hold::new();
        hold.accept(1, DT);
        for _ in 0..100 {
            assert_eq!(hold.reject(f32::NAN).unwrap().phase, Phase::Grace);
        }
    }
}
