# Wingsuit pose controls

`controllers/vehicles/wingsuit_gestures/` recognizes deployment, braking, and closure from one coherent head-and-controller observation. It does not infer a pose by combining independently updated globals. `vr/input/flight_pose.rs` publishes a complete packet only when the head and controllers belong to the same input sampling operation. Replay, unavailable tracking, origin changes, recentering, and runtime replacement invalidate live packet use.

The recognizer requires fresh observations and a neutral flight sequence before arming. A T-pose can request deployment from falling. Wide arms with palms facing forward request braking. Deliberate bilateral withdrawal can request closure only after the recognizer arms. One withdrawn arm, tracking loss, menu capture, or an interaction claim does not count as a closure gesture. Button deployment with lowered arms does not immediately close the suit.

## Interpretation and finalization

The binding-owned `FlightState` retains the grip offset and body dimensions used to interpret observations. Any changed component resets arming, dwell, braking, and pitch reservation without rewinding the observation watermark. Repeating an old packet after calibration cannot contribute new dwell or reactivate held output. Fresh neutral and flight observations can arm the recognizer again.

Finalization checks packet currency, availability, and current per-hand interaction claims again after other handlers run. A contextual canopy request suppresses a pending suit contribution. Newly acquired grip, trigger, mechanical, or grapple ownership also cancels the contribution. Braking reserves arm-pitch steering only while the recognizer has an active eligible brake command. Stick steering remains available.

## Native actions and validation

The converter emits the narrow suit deployment or closure action and the native held airbrake pair. Shared binding integration supplies these contributions alongside the fixed physical-button map; the recognizer does not fabricate raw trigger fire. Engine state and animation permissions still determine whether a native request succeeds.

Pure tests cover independent shoulder and palm geometry, scale, observation identity, dwell boundaries, calibration edits, interaction suspension, late canopy priority, and exact converter output. These tests do not establish physical controller calibration, native acceptance during every transition, or headset comfort. Live flight validation remains required.
