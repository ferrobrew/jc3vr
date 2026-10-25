# Trajectory failure diagnosis

## Verification baseline

The first centralized diagnostic run compiles 36 estimator tests and reports 32 passes and four failures. Its output is `call_6d2e6db4ea644f168ba4f48c1434ee76.stdout` and `.stderr` in session `0ed2pv-oval`. The next central run, `call_a324421e75e7432bbf084afdca848388`, reports 38 passes out of 41 tests. All four original failures pass. Two new recovery fixtures incorrectly assume that a scalar-viable lateral rate prevents every normal fit. The third failure is the transformed nonzero-rate stream. The current follow-up changes have not run. No Rust, Cargo, formatting, lint, or test command runs in this delegate. The coordinator temporarily removes test-only registration during edits and owns restoration and centralized verification.

## Held heading

The first weak-evidence sample publishes stance and chest yaw `-0.15499997`, zero hand-reach residuals, neck residual approximately `0.00847 m`, yaw ambiguity `1.0`, and low confidence. At 72 Hz, a stationary squat later publishes stance yaw `-0.58597064`. The static cost has no stance-heading anchor, while continuity only penalizes departure from the immediately predicted state. Small comfort gains can therefore accumulate heading drift without new yaw evidence.

`fit::Context` now carries completed root yaw. The secondary cost penalizes stance departure from both completed root facing and carried stance heading, normalized by the existing leg-twist limit. These terms use the same coefficient as retained stance translation. They apply on initialization and continuation, remain below tracking fit, and do not use head, wrist, hand-separation, or activity labels. Chest twist and tracking-driven turns remain available. Confidence remains low. The existing weak-hand trajectory is unchanged, and the bend/squat trajectory now also checks the same small stance-heading envelope throughout.

## Swing fixture

The original helper starts at pelvis-coordinate height `0.8955 m`, repeatedly uses the initial candidate for transitions, and fits a lower candidate without carrying that result into subsequent transitions. Its initial endpoint at forward displacement `0.08 m` is already invalid against the nearly extended leg. At step 31, swing progress reaches one and transition validation produces `Released(InvalidTouchdown)` before final fitting. The frozen endpoint has moved to approximately `(-0.12577, 0.03, 0.04000)`. Final fitting retains the release. The solver correctly refuses a plant; the old success oracle is invalid for that fixture.

The success fixture now starts with an additional `0.04 m` reach reserve and asserts endpoint feasibility before lifting. It carries each fitted body and parameter state into the next transition. Its turn re-realizes the complete candidate rather than changing yaw parameters while leaving body geometry stale. Stop, reversal, turn, final ankle/foot agreement, and failed scheduling assertions remain. A new negative regression preserves the original invalid endpoint and requires explicit invalid-touchdown release. The production touchdown validator is unchanged.

## Coupled recovery

The low-head fixture succeeds initially and fails at step 315, not at initialization. The prior state is finite and every coordinate passes scalar stopping-distance viability. Its rates include stance X `-1.1220 m/s`, stance Z `-0.6637 m/s`, stance yaw `-1.3925 rad/s`, and lateral pelvis compensation `1.1028 m/s`. All four constrained seeds pass temporal acceptance but fail coupled geometric evaluation. The last neck residual is approximately `0.02494 m`; one foot is released and the other swings. Independent coordinate viability does not prove feasibility of the next coupled body geometry.

If either normal topology pass exhausts during a continuing update, `recover_constraints` evaluates only the carried previous parameters against four immutable contact-release alternatives. Support carry still validates identity, patch, and geometry, but swing progress does not advance in this degraded attempt. A valid result commits `TemporalStatus::ConstraintRecovery`, zero correction rates, and reduced authority. Current tracker residuals remain explicit. Normal search resumes on the next update. If the previous pose is invalid under all release alternatives, the original structured `SearchExhausted` remains and the committed state is untouched. This recovery explicitly waives acceleration continuity; it does not weaken anatomy or claim dynamic feasibility. No low-head timer or knee/pelvis support type is added.

The bound increases by four fixed-pose evaluations, from 6,957 to 6,961 candidate evaluations and from 13,924 to at most 13,932 analytic leg solves per accepted update. Preliminary recovery skips transitions and the final search. There is no unbounded retry or additional search pass.

## Bend and squat oracle

At 72 Hz, the forward/down endpoint has hip flexion `0.6656473 rad`, mean knee flexion approximately `1.01269 rad`, pelvis height `0.808205 m`, pelvis rearward compensation `0.210871 m`, and neck residual `0.025030 m`. Vertical lowering has hip flexion `0.674885 rad`, mean knee flexion approximately `1.53270 rad`, pelvis height `0.691842 m`, rearward compensation `0.228963 m`, and neck residual `0.025072 m`. Both fits report admissible tracking, normal temporal status, and small active-contact residuals. Different inferred support histories and substantial yaw drift also affect the endpoints. These values disprove the former requirement that bend hip flexion exceed squat hip flexion by at least `0.05 rad`.

The proposal permits hip and torso inclination in a squat and supplies no universal cross-trajectory hip-angle ordering. The replacement checks increased knee flexion and lower pelvis height for vertical lowering, positive hip articulation in both trajectories, and rearward pelvis compensation closer to the existing coupled comfort preference than zero compensation. The last comparison has no new numerical threshold: it compares the candidate against the model's zero-compensation alternative. Existing finite/fixed-length, speed-continuity, tracking residual, zero gait credit, stance/root provenance, and multi-rate endpoint checks remain. This is a selected-prior regression, not recovered physical waist truth.

## Follow-up recovery and frame invariance

The lateral-rate fixture at pelvis height `0.82 m` admits a constrained seed with pelvis X `0.010000003 m`. The full update also finds a normal admissible solution with neck residual `0.024700297 m` and both feet planted. Normal success must not invoke degraded recovery. That state now supplies a negative control requiring `TemporalStatus::Normal`.

Both positive recovery fixtures now reconstruct the diagnosed step-315 state: parameters, incoming rates, released left foot, and right swing origin, endpoint, and progress. The test requires actual exhaustion of the normally advanced topology search before checking held-pose recovery. A test-only candidate setter installs the checked synthetic state for complete-update coverage; production update logic is not forced into fallback. The repeated swinging test carries the reconstructed request at zero clock advance, publishes realized ankles, and repeats on static and translated/yawed support. It requires frozen progress and support-local endpoint, full-update `ConstraintRecovery`, and residuals against the reconstructed arc rather than the prior published ankle. The nonzero residual in this captured state is numerical realization error, approximately `1e-7 m`, rather than a deliberately large unreachable target.

The transformed stream exposes a world-axis search lattice: trials on world X and Z can occur in a different order and sign after a rigid yaw, while temporal clamping also uses world-axis boxes. Planar trial directions, speed/acceleration intervals, acceptance, and continuity costs now use completed-root axes. Saved planar rates remain world vectors and still rotate through completed root transport before prediction. Planar `temporal::interval` values are root-local displacements; articulation intervals remain absolute coordinates. This removes world-axis orientation from the deterministic search family and temporal envelope. Search budget and hard anatomical limits are unchanged.

The stream's orthogonal assertion retains its `0.002 m` tolerance but compares against the transformed control stream, rather than assuming that a control stream can never select an orthogonal correction. The full posture/contact/rate equivalence checks remain. Failure messages include both complete snapshots and numerical state. An additional direct envelope test uses an arbitrary yaw and translation.

The unused `Confidence::Constrained` variant is removed because finite yaw probes cannot justify it and lower-body confidence is always low. No `Moderate` variant exists. `Transition::Reseed` remains because calibration changes require it; a new calibration-epoch test exercises its state/rate reset instead of suppressing a warning.

## Unrun regression inventory

The changed existing tests are `selected_bend_and_squat_prior_allocates_continuously_at_multiple_rates`, `interior_hand_motion_and_head_glance_keep_weak_heading_evidence_weak`, `low_head_dwell_never_adds_a_pelvis_or_knee_support`, and `full_swing_stop_reverse_turn_and_failed_schedule_use_final_realized_targets`.

`tests/recovery.rs` adds:

- `weak_heading_prior_does_not_override_tracking_driven_turn`.
- `near_extension_swing_rejects_invalid_touchdown_instead_of_inventing_a_plant`.
- `coupled_geometry_failure_can_hold_previous_pose_with_explicit_rate_reset`.
- `coupled_recovery_never_publishes_an_invalid_previous_pose`.
- `full_update_coupled_recovery_commits_degraded_then_resumes`.
- `valid_normal_fit_is_not_replaced_by_constraint_recovery`.
- `swinging_constraint_recovery_reconstructs_arc_after_publication_and_support_transport`.

`tests/final_invariants.rs` adds `planar_envelope_is_equivalent_under_arbitrary_world_yaw_and_translation`. `tests/transitions.rs` adds `explicit_reseed_accepts_new_calibration_and_resets_unobserved_rates`. The expected estimator test count is 45.

Central verification should run the full estimator test filter with at most four test threads and aggregate Cargo jobs at most 16. Source review and the static bound do not establish passing assertions, compilation, frame time, or headset behavior. Runtime activation remains outside this revision.
