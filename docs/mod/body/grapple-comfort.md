# Grapple comfort

Unfiltered in VR, the grapple swings the view violently whenever the player fires at anything off-axis, with rotation the inner ear never feels. The player aims by looking at the target, so the head pose already carries the turn toward it; the engine then rotates Rico's root toward the same target (the fire act's directional alignment before the wire spawns, then `NReeledInController::RotateToGrappleTarget` during the reel), and since the view composes as `body × head`, the turn lands twice. The comfort filter removes the body-driven half. The code is `crates/jc3vr-grapple/src/grapple/`, registered with the head pose as its body frame filter (`headpose::BodyFrameFilter`) so the view crate names no grapple; the engine's grapple machinery is in [grapple-pipeline.md](../../engine/gameplay/grapple-pipeline.md). The headset's own tracking passes through untouched ([head-and-body.md](head-and-body.md#design)).

## The body-frame hold

While the grapple is active, `filter_body_rotation` blends the body rotation the head composes onto toward a filtered frame, per the `VIEW ON THE GRAPPLE` comfort setting (`headpose.grapple.mode`). `HoldView`, the default, holds the frame the previous render composed with from fire to landing, so the view stays world-stable and only the headset moves it; a banked wingsuit stays banked. `LevelPitch` flattens the live frame to its yaw, and `Off` does nothing.

Activity is read from three fields of [`GrapplingHook`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/equipment::grappling_hook::GrapplingHook) together, because the state field alone is unreliable: the fire flags (`m_WaitingForGrappleFire`, `m_WaitingForTetherFire`), since the body starts turning before any wire exists; `GHS_INACTIVE` with a live `m_ActiveWire` (the hook in flight); `GHS_REELING_IN`; and a `GHS_REELED_` state only with a live wire, because `m_State` keeps the last reel's outcome through minutes of ordinary play.

The engage is instant and holds the previous advance's body under the current blend: the fire's turn lands in the same step the state flips, and a second grapple during a release tail must hold the partly blended frame the player already sees. Release blends the rotation back exponentially and snaps to disarmed below a small blend so no stale state reaches the next engage.

## The yaw handoff

Under `HoldView`, for a VR player landing on foot, the release does not sweep the view through the body's heading. The held heading is posted for the VR body-turn accumulator (`grapple::take_body_yaw_retarget`), the character turns to face the view through the game's rate-limited turn machinery, and the hold stays engaged until the heading converges, leaving only a small pitch and roll residual for the blend. It falls back to the plain release on a timeout (the body can be blocked from turning), and ends when the character leaves its feet or a new reel starts. Airborne releases skip it, since the body-yaw accumulator steers only on foot.

Because the held frame's yaw stops tracking the real body, everything that steers the character reads the raw rotation (`headpose::xr::body_rotation_raw`); only the view composition reads the filtered one.

## The landing-snap absorber

The attach or landing animation moves the head anchor a large distance against Rico's root in one sim tick. `filter_anchor` watches the anchor's offset from the root (which changes only when a new tick's anchors are published) and absorbs a step beyond a threshold into an offset that decays out of the view. The world-space anchor's velocity is not usable for this: the sim's ticks are uneven and timed on the render cadence, so ordinary steps in a fast flight read as velocity changes. The absorber arms only from the landing onward (`GHS_REELING_IN` disarms it), the filtered anchor is always `raw + offset` so the engine's sub-frame interpolation is undisturbed, the offset is capped so a mis-detection is a bounded lag, and a step beyond a teleport threshold passes through and resets it.

## Cadence and locking

The filter advances on the engine's input tick and on every rendered VR frame, since a tick-only read would leave up to a tick of the opening rotation in the view. Both advances share one mutex, the innermost in the head pose's SIM, BODY_YAW, FILTER order: the input-tick caller holds the sim lock, so nothing under the filter's lock may call a locking `headpose` accessor (hence `on_foot` arrives as a parameter). A violation deadlocks the game; the lock-order note is in `grapple/mod.rs`.

The timings are the `comfort` section of `crates/jc3vr-grapple/tuning/grapple.toml`. The debug UI's Grapple feature shows the live blend and toggles a per-tick CSV capture of the filter's inputs and outputs (`grapple/telemetry.rs`, written to `grapple-logs/` in the session folder, [sessions.md](../sessions.md)), the tool for a comfort report that is hard to reproduce by feel.
