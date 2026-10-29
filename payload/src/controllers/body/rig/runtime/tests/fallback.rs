use super::*;

const STEP: Duration = Duration::from_millis(33);

/// The standing packet, optionally with the right grip far beyond any arm's reach, which no fit
/// can meet within the hand tolerance.
fn packet(now: Instant, sequence: u64, overreach: bool) -> Tracking {
    let mut packet = tracking(now, sequence);
    if overreach {
        let mut grip = packet.head;
        grip.pose.position += Vec3::new(0.25, -0.3, -3.0);
        packet.grips = [None, Some(grip)];
    }
    packet
}

struct Stream<'a> {
    rig: &'a Rig,
    runtime: Runtime,
    start: Instant,
    sequence: u64,
}

impl<'a> Stream<'a> {
    fn new(rig: &'a Rig) -> Self {
        Self {
            rig,
            runtime: Runtime::default(),
            start: Instant::now(),
            sequence: 0,
        }
    }

    fn now(&self) -> Instant {
        self.start + STEP * self.sequence as u32
    }

    fn gate(&mut self, overreach: bool) -> Option<GateFit> {
        self.sequence += 1;
        let now = self.now();
        let mut gate = input(self.rig, now);
        gate.dt = STEP.as_secs_f32();
        self.runtime
            .commit(Some(packet(now, self.sequence, overreach)), gate)
    }

    /// The estimator's own latest commit, which the next gate continues.
    fn committed(&self) -> Snapshot {
        *self.runtime.estimator.snapshot().expect("a committed fit")
    }

    /// Release the overreach and gate until a fit is accepted again, checking that every gate on
    /// the way continues the estimator's stream.
    fn recover(&mut self) -> GateFit {
        for _ in 0..60 {
            let fit = self.gate(false);
            assert_eq!(self.committed().provenance.transition, Transition::Continue);
            if self.runtime.status.presented == Some(Phase::Accepted) {
                return fit.expect("an accepted fit is presented");
            }
        }
        panic!("the fit does not recover once the reach is released");
    }
}

#[test]
fn a_single_rejected_fit_keeps_the_accepted_fit_and_continues_the_stream() {
    let rig = rig();
    let mut stream = Stream::new(&rig);
    let accepted = stream.gate(false).expect("the standing fit is accepted");
    assert_eq!(stream.runtime.status.presented, Some(Phase::Accepted));
    stream
        .runtime
        .hold
        .held_mut_for_test()
        .unwrap()
        .snapshot
        .root
        .displacement = Vec3::new(0.08, 0.0, 0.0);

    let held = stream.gate(true).expect("the accepted fit stands in");
    let rejected = stream.committed();
    assert!(!rejected.quality.feasible, "{rejected:?}");
    assert_eq!(rejected.provenance.current.step, 2);
    assert_eq!(stream.runtime.status.fallback, Some(Fallback::Residual));
    assert_eq!(stream.runtime.status.presented, Some(Phase::Grace));
    assert_eq!(held.weight, 1.0);
    assert_eq!(held.root, accepted.root);
    assert_eq!(held.snapshot.provenance, accepted.snapshot.provenance);
    assert_eq!(held.snapshot.body, accepted.snapshot.body);
    assert_eq!(held.snapshot.feet, accepted.snapshot.feet);

    // The hands' shoulders and the root keep following the held fit through the grace.
    let now = stream.now();
    assert!(stream.runtime.torso(now).is_some());
    let chase = stream
        .runtime
        .take_root(now, STEP.as_secs_f32(), Mat4::IDENTITY, true);
    assert!(chase.x > 0.0, "{chase:?}");
    assert_eq!(
        stream
            .runtime
            .take_root(now, STEP.as_secs_f32(), Mat4::IDENTITY, true),
        Vec2::ZERO
    );

    let recovered = stream.recover();
    assert_eq!(recovered.weight, 1.0);
    assert_eq!(
        recovered.snapshot.provenance.transition,
        Transition::Continue
    );
}

#[test]
fn sustained_rejection_fades_out_and_recovery_fades_back_in_without_a_reseed() {
    let rig = rig();
    let mut stream = Stream::new(&rig);
    let accepted = stream.gate(false).expect("the standing fit is accepted");
    let mut last = 1.0;
    let mut faded = false;
    for _ in 0..40 {
        let Some(held) = stream.gate(true) else {
            faded = true;
            break;
        };
        assert_eq!(held.snapshot.body, accepted.snapshot.body);
        assert!(held.weight <= last && held.weight > 0.0);
        last = held.weight;
        let phase = stream.runtime.status.presented.unwrap();
        assert_eq!(held.weight < 1.0, phase == Phase::FadingOut);
        // Past the grace the hands return to their own shoulders while the body fades.
        assert_eq!(
            stream.runtime.torso(stream.now()).is_some(),
            phase == Phase::Grace
        );
    }
    assert!(faded, "sustained rejection reaches the animation");
    assert!(last < 0.5);
    assert_eq!(stream.runtime.status.presented, None);
    assert_eq!(stream.runtime.status.weight, 0.0);

    // The pose writer reports the empty gate; a rejected fit keeps the estimator's stream.
    stream.runtime.gate_failed();
    assert!(stream.runtime.previous.is_some());
    stream.gate(true);
    assert_eq!(
        stream.committed().provenance.transition,
        Transition::Continue
    );

    let recovered = stream.recover();
    assert!(recovered.weight > 0.0 && recovered.weight < 1.0);
    let mut last = recovered.weight;
    while last < 1.0 {
        let fit = stream
            .gate(false)
            .expect("the recovered fit stays accepted");
        assert!(fit.weight > last);
        last = fit.weight;
    }
}

#[test]
fn a_failed_update_still_withdraws_the_held_fit() {
    let rig = rig();
    let mut stream = Stream::new(&rig);
    stream.gate(false).expect("the standing fit is accepted");
    stream.gate(true).expect("the accepted fit stands in");
    assert!(
        stream
            .runtime
            .commit(None, input(&rig, stream.now()))
            .is_none()
    );
    assert_eq!(
        stream.runtime.status.fallback,
        Some(Fallback::TrackingUnavailable)
    );
    assert!(stream.runtime.hold.presented().is_none());
    assert!(stream.runtime.previous.is_none());
}
