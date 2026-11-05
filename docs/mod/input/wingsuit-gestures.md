# Wingsuit pose controls

`controllers/vehicles/wingsuit_gestures/` recognizes deployment, braking, and closure from one coherent head-and-controller observation. It does not infer a pose by combining independently updated globals. `vr/input/flight_pose.rs` publishes a complete packet only when the head and controllers belong to the same input sampling operation. Replay, unavailable tracking, origin changes, recentering, and runtime replacement invalidate live packet use.

The recognizer requires fresh observations. A neutral followed by a T-pose can request deployment from falling. In the suit, a stable flight pose arms it, the suit's entry being the fresh start. Wide arms with both wrists rolled to face the wind request braking. Deliberate bilateral withdrawal can request closure only after the recognizer arms. One withdrawn arm, tracking loss, menu capture, or an interaction claim does not count as a closure gesture. Button deployment with lowered arms does not immediately close the suit.

The arm measures are taken against the body's facing, read from the line between the wrists, because a flier's head turns away from the arms. The palm is each wrist's roll about the fingers, the measure the arm steering's wrist tilt reads, so raising or sweeping the arms does not change it. [controllers.md, "Wingsuit gestures"](controllers.md) has the thresholds, the fallbacks, and the logging.

## Interpretation and finalization

The binding-owned `FlightState` retains the grip offset and body dimensions used to interpret observations. Any changed component resets arming, dwell, braking, and pitch reservation without rewinding the observation watermark. Repeating an old packet after calibration cannot contribute new dwell or reactivate held output. A fresh flight pose can arm the recognizer again in the suit, and a fresh neutral and flight pose while falling.

Finalization checks packet currency, availability, and current per-hand interaction claims again after other handlers run. A contextual canopy request suppresses a pending suit contribution. Newly acquired grip, trigger, mechanical, or grapple ownership also cancels the contribution. A hand has the grapple while the grapple button is held, while its line is attached, or during a traversal; tethers left in the world claim neither hand, so a fall after tethering a car still opens the suit. Braking reserves arm-pitch steering only while the recognizer has an active eligible brake command. Stick steering remains available.

## Landing

The game lands the wingsuit through its close act, which a held air brake queues on every tick ([engine: wingsuit-landing.md](../../engine/gameplay/wingsuit-landing.md)). Once the suit is slow enough and low enough, the act turns into a slide onto the feet; faster, it does nothing, and the glide that meets the ground ends in a ragdoll. So the brake is the landing: wide arms with both palms to the wind, held as the suit comes in low over level ground, slow it until the game closes the suit into the slide. The `air_brake` action holds the same pair of actions from any analog input a custom layout binds it to; it ships unbound, since it is a float and the free buttons in the suit are clicks.

The brake weakens while the suit turns: the game's brake gives up part of its drag and lift for the steering's sideways input, and the arm steering's bank is that input. Level wings brake hardest. The stall flip's removal ([controllers.md, "The wingsuit's stall flip"](controllers.md)) keeps the brake and the close act open as the suit slows with its nose up, where the game's own stall would close both until it recovered. Held higher up, the same brake closes the suit into a fall once it is slow, so the landing's brake belongs in the last few metres. Both the brake and the landing need the game's gear mods for them (`upgrade_wingsuit_air_brake` and `upgrade_wingsuit_cancel`); without the second, the close act never fires and the suit stays open until it meets the ground.

The arm-pitch reservation while braking keeps the brake's rolled wrists from also reading as a full climb. A flare (pitching up while braking) stays on the stick.

In the seated session the brake gestures never armed, so none of the three flights braked. The arms show what the player tried: in the first flight both palms rolled to the wind for about a second, two seconds before the ground, then rolled back past flat; in the third they rolled on past palm-forward toward palm-up for two seconds before the ground, past the brake's band and into the wrap of the arm steering's wrist tilt. The recording does not hold the suit's speed or height, so how far short of the landing's gates the flights fell is not measured.

## The ripcord

While falling or in the suit, a ripcord at the back of the hip on the dominant side opens the parachute: grip it and pull it out, the way a parachute handle is taken ([controllers.md, "Ripcord"](controllers.md)). The pull clicks the canopy action, the one the right lower face (A) drives, before the gestures commit, so a pull cancels a pending suit request as the button does.

## Native actions and validation

The converter emits the narrow suit deployment or closure action and the native held airbrake pair. Shared binding integration supplies these contributions alongside the fixed physical-button map; the recognizer does not fabricate raw trigger fire. Engine state and animation permissions still determine whether a native request succeeds.

Pure tests cover independent shoulder and palm geometry, scale, observation identity, dwell boundaries, calibration edits, interaction suspension, late canopy priority, and exact converter output, and poses sampled from a seated player's flights check the thresholds against arms that flew. These tests do not establish physical controller calibration, native acceptance during every transition, or headset comfort. Live flight validation remains required.

## Headset checklist

1. Tether two objects, walk off a ledge, and spread the arms: the suit opens, and the log does not say "the left hand has the grapple".
2. Hold the grapple button while falling: the gestures stand down until it is let go.
