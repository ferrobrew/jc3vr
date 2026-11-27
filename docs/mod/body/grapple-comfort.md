# Grapple comfort

The grapple is JC3's most-used traversal mechanic, and unfiltered in VR it is the most uncomfortable one: firing at anything off-axis swings the view violently, with rotation the inner ear never feels. The grapple's comfort filter removes that rotation. The implementation is in `crates/jc3vr-grapple/src/grapple/`; the engine-side grapple machinery it reads is in [grapple-pipeline.md](../../engine/gameplay/grapple-pipeline.md).

## Rotation applied twice

The player aims the grapple by looking at the target, so the head pose already carries the rotation toward it. The engine then rotates the character's root toward the same target: first the fire act's directional alignment, a fast turn through a large angle before the wire spawns, then `NReeledInController::RotateToGrappleTarget` during the reel. The alignment's angle and rate limits have no code constant and are unverified; they are most likely authored in the fire act's animation data. The head pose composes the view as `body × head`, so that rotation lands twice and the view sweeps past the thing the player is looking at.

The filter acts on the body-driven inputs to that composition while the grapple owns the character. The headset's own tracking passes through untouched in both rotation and position, per the rule against smoothing the headset-to-camera path in [head-and-body.md](head-and-body.md). Three pieces cooperate: the body-frame hold, the yaw handoff, and the landing-snap absorber. The grapple registers them with the head pose as its body frame filter (`headpose::BodyFrameFilter`, from the grapple crate's `register`), so the head pose names no grapple.

## The body-frame hold

While the grapple is active, `filter_body_rotation` blends the body rotation the head composes onto toward a filtered frame, per the setting `headpose.grapple.mode`:

- `HoldView` (the default): the frame the previous render composed with, held from fire to landing. The view stays world-stable, so what the player looks at stays looked at, and only the headset moves it. This cancels exactly the rotation the grapple adds, in both pitch and yaw, while keeping whatever frame the player entered with (a banked wingsuit stays banked).
- `LevelPitch`: the live frame flattened to its yaw. The view stays level but still turns with the body's heading, for players who prefer the view to follow the reel's direction; it leaves the yaw half of the double count in place.
- `Off`: no filtering.

The pause menu's `VR: COMFORT` page offers the setting as `VIEW ON THE GRAPPLE` (`HOLD STEADY`, `KEEP LEVEL`, and `SWING WITH THE BODY`).

### When the hold is active

The grapple's own state field is not a reliable activity signal, so the filter reads three fields of [`GrapplingHook`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/equipment::grappling_hook::GrapplingHook) together:

| Phase | Signal |
|---|---|
| Fire committed (the act aligning the body) | `m_WaitingForGrappleFire` or `m_WaitingForTetherFire` |
| Hook in flight | `GHS_INACTIVE` with a live `m_ActiveWire` |
| Zipping | `GHS_REELING_IN` |
| Attached (a wall perch, a hang) | a `GHS_REELED_` state with a live `m_ActiveWire` |

The fire flags matter because the body starts turning toward the target before any wire exists, so engaging on `GHS_REELING_IN` alone would leave the whole alignment in the view. The reeled states need the wire check because the engine treats `m_State` as the last reel's outcome as much as the current state: it can sit at `GHS_REELED_ATTACHED` through minutes of ordinary play, which without the check would keep the filter engaged through normal locomotion.

### Engage and release

On the engage edge the filter holds the frame the view last composed: the previous advance's body under the current blend. It takes the previous advance's body because the fire's turn lands in the same step the state flips, so the current body already carries some of it. It applies the current blend because a second grapple during a release tail must hold the partially blended frame the player is already looking through. The engage's time constant, `comfort.engage_s`, is zero: an instant engage is seamless by construction, and a ramp would only leak the opening rotation.

Release blends the accumulated rotation back with the time constant `comfort.release_s`, and the release tail snaps to fully disarmed below the blend `comfort.disarm_factor`, since the exponential decay would otherwise linger near zero and leave stale state for the next engage.

## The yaw handoff

An on-foot landing leaves the body heading well off the held view, and returning that through the release blend would sweep the view through it. Under `HoldView`, for a VR player on foot, the view does not sweep: at the reel's end the held heading is posted for the VR body-turn accumulator (`grapple::take_body_yaw_retarget`, taken on its next on-foot tick within `comfort.retarget_fresh_s`), the character turns to face where the player looks through the game's own rate-limited turn machinery, and the hold stays engaged until the body's heading converges within `comfort.handoff_converged_deg`, leaving only a small pitch and roll residual for the blend. This is the body-follows-head scheme of [head-and-body.md](head-and-body.md) applied at the landing.

The handoff falls back to the plain release on a timeout (`comfort.handoff_timeout_s`, since the body can be blocked from turning), and ends when the character leaves its feet or a new reel starts. It is skipped for airborne releases: the body-yaw accumulator only steers on foot, so holding the view while flying away would pin it against a moving world.

Because the held frame's yaw stops tracking the real body, everything that steers the actual character reads the raw rotation (`headpose::xr::body_rotation_raw`); only the view composition reads the filtered one.

## The landing-snap absorber

The attach or landing animation moves the head anchor a large distance against Rico's root in a single sim tick. `filter_anchor` absorbs that jump into an offset that decays out of the view over `comfort.anchor_snap_ease_s`, leaving the root's own motion, at any speed, untouched.

The detection watches the anchor's offset from the root (the anchor less the translation of the root it was captured with), which the frame's anchor pair carries, and absorbs a change of more than `comfort.anchor_snap_m` between consecutive advances. The offset changes only when a new tick's anchors are published, so the advances between ticks see no step. A clip moves the head against the root by a few centimetres a tick, and a canopy's opening by more but still under the threshold. Watching the world-space anchor's velocity instead fails in a fast flight: the sim's ticks are of uneven length and are timed on the render cadence, so ordinary steps read as velocity changes and the absorber pulls the camera behind Rico's head. The absorber is armed only from the landing onward: `GHS_REELING_IN` disarms it, so the zip passes through unchanged, and the only snap the grapple produces is at the attach.

The filtered anchor is always `raw + offset`, so pose-pair deltas built from it keep exact tick spacing and the engine's sub-frame interpolation is undisturbed. The offset is capped at `comfort.anchor_offset_max_m`, so a mis-detection degrades into a bounded, decaying lag, and a step of the root-relative offset beyond `comfort.anchor_teleport_m` is no animation: it passes through and resets the absorber.

## Cadence and locking

The filter advances on the engine's input tick and on every rendered VR frame. The frame-cadence advance is needed because the body starts rotating the instant the grapple acts, and a tick-only read would leave up to a tick of that rotation in the view before the filter could engage.

Both advances share one mutex, and it is the innermost lock in the head pose's SIM, BODY_YAW, FILTER order: the input-tick caller holds the sim lock when it calls `advance`, so nothing under the filter's lock may call back into a locking `headpose` accessor, which is why `on_foot` arrives as a parameter. A violation deadlocks the game; the lock-order note is in `crates/jc3vr-grapple/src/grapple/mod.rs`.

## Tuning

The timing is the `comfort` section of the grapple's tuning file (`crates/jc3vr-grapple/tuning/grapple.toml`, [tuning.md](../tuning.md)), whose keys this page names. The debug UI's Grapple feature shows the live blend in its live status.

## Telemetry

`crates/jc3vr-grapple/src/grapple/telemetry.rs` captures the filter's inputs and outputs (the hook's phase, the blend, the raw, filtered, and held body frames, the headset's cockpit pose, the composed head pose, and the anchor) to `grapple-logs/<stamp>.csv` in the session folder ([sessions.md](../sessions.md)), one row per input tick and per rendered VR frame. The debug UI's Grapple feature toggles it in its live status (`Log reel telemetry`, off by default); each enable starts a fresh file. These captures are the tool for a comfort report that is hard to reproduce by feel.

## What needs confirming in the headset

- After a grapple pull under the canopy or a reel into a fall, at speed, the view stays at Rico's head: no flashes of the harness, the backpack, or the back of the body for the few seconds after the reel.
- A grapple's landing on a ledge or a wall eases the attach's jump out of the view rather than snapping it.
