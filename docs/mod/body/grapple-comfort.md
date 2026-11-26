# Grapple comfort

The grapple is JC3's most-used traversal mechanic, and unfiltered in VR it is the most uncomfortable one: firing at anything off-axis swings the view violently, with rotation the inner ear never feels. This doc describes the filter that fixes it (issue #36). The implementation lives in `crates/jc3vr-grapple/src/grapple/`; the engine-side grapple machinery it reads is documented in `docs/engine/gameplay/grapple-pipeline.md`.

## The problem: rotation applied twice

You aim the grapple by looking at the target, so the head pose already carries the rotation toward it. The engine then rotates the character's root toward the same target — first the fire act's directional alignment, a fast turn through a large angle before the wire even spawns, then `NReeledInController::RotateToGrappleTarget` during the reel. The alignment's angle and rate limits have no code constant and are not yet confirmed; they are most likely authored in the fire act's animation data. The headpose composes the view as `body × head`, so that rotation lands twice and the view sweeps *past* the thing you're looking at.

The fix filters the body-driven inputs to that composition while the grapple owns the character. The HMD's own tracking passes through untouched in both rotation and position, per the "no smoothing on the HMD→camera path" rule in `head-and-body.md`. Three pieces cooperate: the body-frame hold, the yaw handoff, and the landing-snap absorber.

## The body-frame hold

While the grapple is active, `filter_body_rotation` blends the body rotation the head composes onto toward a filtered frame, per `headpose.grapple.mode`:

- **Hold view** (default): the frame the previous render composed with, held from fire to landing. The view stays world-stable — what you're looking at stays looked-at — and only the HMD moves it. This cancels exactly the rotation the grapple *adds*, in both pitch and yaw, while preserving whatever frame you entered with (a banked wingsuit stays banked).
- **Level pitch**: the live frame flattened to its yaw. The view stays level but still turns with the body's heading. Kept for players who prefer the view to follow the reel's direction; it leaves the yaw half of the double-count in place.
- **Off**: no filtering.

### When the hold is active

The grapple's own state field is not a reliable activity signal, so the filter reads three hook fields together (`GrapplingHook` in the pyxis defs):

| Phase | Signal |
|---|---|
| Fire committed (act aligning the body) | `m_WaitingForGrappleFire` / `m_WaitingForTetherFire` |
| Hook in flight | `GHS_INACTIVE` with a live `m_ActiveWire` |
| Zipping | `GHS_REELING_IN` |
| Attached (wall perch, hang) | `GHS_REELED_*` with a live `m_ActiveWire` |

Two of these gates exist because the obvious signal lies. The fire flags matter because the body starts whipping toward the target before any wire exists — engaging on `GHS_REELING_IN` alone leaves that whole alignment in the view. And the `GHS_REELED_*` states need the wire check because the engine treats `m_State` as "last reel outcome" as much as "current state": telemetry caught it parked at `GHS_REELED_ATTACHED` for six minutes of ordinary play, which without the gate kept the filter engaged through normal locomotion.

### Engage and release

Engagement is instant and seamless by construction: on the engage edge, the filter holds the frame the view *last composed* — the previous advance's body under the current blend. The previous advance's body, because the fire snap lands in the same step the state flips (capturing the current body bakes 5–27° of it into the hold). Under the current blend, because a re-grapple during a release tail must hold the partially blended frame you're already looking through.

Release blends the accumulated rotation back over 0.4 s (`RELEASE_S` in `crate::grapple`), and the release tail snaps to fully disarmed below 0.1% blend — the decay is exponential and would otherwise linger near-zero for many seconds, leaving stale state for the next engage.

## The yaw handoff

An on-foot landing leaves the body heading 10–25° off the held view, and returning that through the release blend is 34–61° of rotation path at sustained vection. Under hold view, for a VR player on foot, the view never sweeps through it: at reel end the held heading is posted for the VR body-turn accumulator (`grapple::take_body_yaw_retarget`, consumed on its next on-foot tick), the *character* turns to face where you're looking via the game's own rate-limited turn machinery, and the hold stays engaged until the heading converges within 3° — leaving only a small pitch/roll residual for the blend. This is the design doc's body-follows-head scheme applied at the landing.

The handoff falls back to the plain blend release on a timeout (`HANDOFF_TIMEOUT_S`, since the body can be blocked from turning) and skips entirely for airborne releases: the body-yaw accumulator only steers on foot, so holding the view while flying away would pin it against a moving world.

Because the held frame's yaw stops tracking the real body, everything that steers the actual character reads the raw rotation (`headpose::xr::body_rotation_raw`); only the view composition reads the filtered one.

## The landing-snap absorber

The attach/landing animation moves the head anchor half a metre to a metre and a half against Rico's root in a single sim tick. `filter_anchor` absorbs that jump into an offset that decays out of the view over `ANCHOR_SNAP_EASE_S`, leaving the root's own motion, at any speed, untouched.

The detection watches the anchor's offset from the root (the anchor less the translation of the root it was captured with), which the frame's anchor pair carries, and absorbs a change of more than `ANCHOR_SNAP_M` between consecutive advances. The offset changes only when a new tick's anchors are published, so the advances between ticks see no step. A clip moves the head against the root by a few centimetres a tick, and a canopy's opening by up to about a quarter of a metre. The absorber is armed only from the landing onward: the zip passes through bit-exact, `GHS_REELING_IN` disarms the absorber, and the only snap the grapple produces is at the attach.

The detection used to compare each world-space anchor step against a smoothed velocity estimate. In a fast flight the sim's ticks are of uneven length and their changes were timed on the render cadence, so ordinary steps read as velocity changes: after each reel the absorber pulled the view 0.2 to 0.4 m behind the head for a second or more, putting the camera inside or behind Rico's back, which the player saw as flashes of the harness and backpack. The 10 November 17:07 take's camera matches a replay of that detection to the centimetre at t 1357.6 to 1358.2, and replayed with the root-relative detection the same flights never reach the threshold.

The filtered anchor is always `raw + offset`, so pose-pair deltas built from it keep exact tick spacing and the engine's sub-frame interpolation is undisturbed. The offset is hard-capped at 1 m (a mis-detection degrades into a bounded, decaying lag), and steps of the root-relative offset beyond 5 m are no animation — passed through with the absorber reset.

## Cadence and locking

The filter advances on the engine's input tick *and* on every rendered VR frame. The frame-cadence advance is load-bearing: the body starts rotating the instant the grapple acts, and a tick-only (~33 Hz) read leaves up to a tick of that rotation in the view before the filter can engage.

Both advances share one mutex, and it is the innermost lock in the headpose's SIM → BODY_YAW → FILTER order: the input-tick caller holds the sim lock when it calls `advance`, so nothing under the filter's lock may call back into a locking `headpose` accessor — `on_foot` arrives as a parameter for exactly this reason. Violating this deadlocked the game on injection once; see the lock-order note in `crates/jc3vr-grapple/src/grapple/mod.rs`.

## Telemetry

`crates/jc3vr-grapple/src/grapple/telemetry.rs` captures the filter's inputs and outputs — hook phase, blend, raw/filtered/held body frames, HMD cockpit pose, composed head pose, anchor — to `grapple-logs/<stamp>.csv` in the session folder ([sessions.md](../sessions.md)), one row per input tick and per rendered VR frame. Toggle it from the debug UI's Grapple feature, in its live status ("Log reel telemetry"; off by default); each enable starts a fresh file. Every behaviour above was diagnosed and verified against these captures, and they remain the tool of choice when a comfort report is hard to reproduce by feel.

## Configuration

`headpose.grapple.mode` (default `HoldView`) selects the body-frame filter mode; the pause menu's `VR: COMFORT` page offers it as `VIEW ON THE GRAPPLE` (`HOLD STEADY` for `HoldView`, `KEEP LEVEL` for `LevelPitch`, and `SWING WITH THE BODY` for `Off`). The timing is fixed in `crate::grapple`, and the debug UI's Grapple feature shows the live blend in its live status:

| Constant | Value | Meaning |
|---|---|---|
| `ENGAGE_S` | `0` | Blend-in time constant. Instant is seamless; a ramp only leaks the opening rotation. |
| `RELEASE_S` | `0.4` | Blend-out time constant for the accumulated rotation. |
| `HANDOFF_TIMEOUT_S` | `1.5` | Fallback to the blend release if the body cannot converge. |
| `ANCHOR_SNAP_M` | `0.4` | One tick's change in the anchor's offset from the root treated as a landing snap. |
| `ANCHOR_SNAP_EASE_S` | `0.15` | How long an absorbed snap takes to ease back out. |

## What needs confirming in the headset

- After a grapple pull under the canopy or a reel into a fall, at speed, the view stays at Rico's head: no flashes of the harness, the backpack, or the back of the body for the few seconds after the reel.
- A grapple's landing on a ledge or a wall still eases the attach's jump out of the view rather than snapping it.
