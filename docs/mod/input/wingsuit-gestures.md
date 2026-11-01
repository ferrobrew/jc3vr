# Wingsuit pose controls

`controllers/vehicles/wingsuit_gestures/` recognizes deployment, braking, and closure from one coherent head-and-controller observation. It does not infer a pose by combining independently updated globals. `vr/input/flight_pose.rs` publishes a complete packet only when the head and controllers belong to the same input sampling operation. Replay, unavailable tracking, origin changes, recentering, and runtime replacement invalidate live packet use.

The recognizer requires fresh observations. A neutral followed by a T-pose can request deployment from falling. In the suit, a stable flight pose arms it, the suit's entry being the fresh start. Wide arms with both wrists rolled to face the wind request braking. Deliberate bilateral withdrawal can request closure only after the recognizer arms. One withdrawn arm, tracking loss, menu capture, or an interaction claim does not count as a closure gesture. Button deployment with lowered arms does not immediately close the suit.

The arm measures are taken against the body's facing, read from the line between the wrists, because a flier's head turns away from the arms. The palm is each wrist's roll about the fingers, the measure the arm steering's wrist tilt reads, so raising or sweeping the arms does not change it. [controllers.md, "Wingsuit gestures"](controllers.md) has the thresholds, the fallbacks, and the logging.

## Interpretation and finalization

The binding-owned `FlightState` retains the grip offset and body dimensions used to interpret observations. Any changed component resets arming, dwell, braking, and pitch reservation without rewinding the observation watermark. Repeating an old packet after calibration cannot contribute new dwell or reactivate held output. A fresh flight pose can arm the recognizer again in the suit, and a fresh neutral and flight pose while falling.

Finalization checks packet currency, availability, and current per-hand interaction claims again after other handlers run. A contextual canopy request suppresses a pending suit contribution. Newly acquired grip, trigger, mechanical, or grapple ownership also cancels the contribution. Braking reserves arm-pitch steering only while the recognizer has an active eligible brake command. Stick steering remains available.

## Native actions and validation

The converter emits the narrow suit deployment or closure action and the native held airbrake pair. Shared binding integration supplies these contributions alongside the fixed physical-button map; the recognizer does not fabricate raw trigger fire. Engine state and animation permissions still determine whether a native request succeeds.

Pure tests cover independent shoulder and palm geometry, scale, observation identity, dwell boundaries, calibration edits, interaction suspension, late canopy priority, and exact converter output, and poses sampled from a seated player's flights check the thresholds against arms that flew. These tests do not establish physical controller calibration, native acceptance during every transition, or headset comfort. Live flight validation remains required.
