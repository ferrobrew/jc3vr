# Analysis of the 14 November 2026 02:05 session

Take: `sessions/2026-11-14_02-05-36_243/recordings/2026-11-14_02-09-08_957/`. The session started at 01:05:36.248 UTC and ended at 02:22:32.8 in the known WaveWorks crash (CONTRIBUTING.md), 16 s after the recording stopped. Tracking started at 01:09:08.957, the video at 01:09:10.821, and the recording stopped at 02:22:16.534, so the take is 4387.6 s of tracking. It has the video, `tracking.jc3trk`, `frames.jsonl`, `take.rrd` (exported by the capture relay), and `view-scan.md` (copied here). The build string is `2026-11-14 02:04:28`, which includes everything up to 6947b3ad: the learned body estimator, the skeleton shoulders and forward-reach arm remap, and Rico's head on the camera's neck pivot all ran in a headset for the first time. The estimator ran on the game's device with the 16-bit weights.

Times are the Rerun `recording` timeline's, which starts at the tracking start: a time `t` is the UTC log time minus 01:09:08.957, and the video time is `t` minus 1.864 s.

This is the initial harvest, written before the reviewer's feedback. The verdicts come from the tracking data and video frames, not only from the log. Each area has its full report beside this one:

- [body.md](body.md): the body, the camera, the learned estimate, the arm remap, and the head pivot.
- [guns.md](guns.md): guns, holsters, reloading, grenades, C4, grabs of objects and characters, and the snatch.
- [world.md](world.md): the grapple, flight, vehicles and mounted guns, doors and ragdolls, the hull, the debug UI, and frame timing.
- [view-scan.md](view-scan.md): 304 flashes, 74 with the camera more than 0.12 m behind Rico's head.
- `timeline.txt` and `log_annot.txt` (not committed): what the reviewer did, and the session log with `t` and the video time on each line.

## The previous round's checks

From [the 10 November 17:07 test plan](../2026-11-10_17-07/next-session.md), in its order.

Fixed:

- The Steam stalls: no `online:` stall lines, `sim_steam` peaks at 9.2 ms, and the 61 s beat is gone.
- Head bob: 0.1 to 0.2 mm RMS vertically while jogging and sprinting, against 20 to 23 mm, and no longer correlated with the animated head. A 7.7 mm lateral sway remains.
- Get-ups on the ground hold the view upright to within 1 degree.
- The narrowed hull steps up 0.23 to 0.42 m kerbs and steps (14 step-ups, no stalls).
- Ragdoll grabs mostly hang: of 36 ragdoll holds, 21 ended at the grip's release (12 of them throws) and 11 were torn after 0.23 to 6.97 s, against every hold torn within 0.35 s before.
- Grenade snatches (6 of 6), car doors by the handle, and the door coming to the hand.
- C4: the right hand's first press takes a charge 86 percent of the time (50 percent before); the detonator replaces a held charge and shows on the pouch with none left.
- The launcher's fresh pod shows at once.
- The fast magazine knock-out works (9 times), though 5 of the 12 pushes that were too slow came at 0.81 to 0.99 m/s, just under the 1.0 m/s threshold.
- The swing entry keeps 61 to 126 percent of the incoming speed; tap and hold as the hook lands took 11 locks within 0.12 s.
- Wingsuit gestures: every deploy worked, none was refused for a grip, and no superspeed. The suit opened from a helicopter hang (t 1800.5).
- The seat lock's yaw re-base is spread over about a second (at most 1.8 degrees a frame), and nothing blacked out near cockpit glass.
- The fighter's throttle and stick haptics all fire (78 throttle steps, 13 zero stops, 10 reverse-gate crossings).
- Yanking a turret gunner out, and entering the gunner seat.

Partly fixed:

- Holsters: hip draw misses fell from 30 to 17 percent, but docks still succeed only 75 percent of the time. Eleven drops follow "the animation forbids it", where the attraction path has no keep fallback; six guns fell mid-travel with no refusal logged; a two-handed hip holster lost the right gun 3 times in 4.
- Swaps onto a full socket: 2 of 3 worked, and the hip swap's hint reads old to new. The failed shoulder swap at t 2657 put both guns on the ground.
- Grenade priming: the pin bump, the quickening clicks, and the green-to-yellow ring work, but the trail and its sound never start in the hand (`effect=false sound=false` on all 14 primes). None went off early.
- Dropping from a grapple: 21 of 24 reel endings on the ground sink the view about 1 m and spring back within 0.3 s (for example t 1974.4).
- Helicopter hangs ride upright but on the reel's own length (5 to 12 m), and two ended at once.
- Mounted guns: the gun never comes to the hand. The gunboat's gun and the tripod minigun stay mounted although the tear is logged, and the pickup took a dead soldier's rifle instead (t 3401.7, 4055.1); tears from the gunner seat are often given up with the player still seated.

Not fixed:

- The swing exit: all 18 releases drop the speed to about zero within some 70 ms (37 to 4.8 m/s at t 928.75), on the frame the grapple button comes up, and Rico falls straight down.
- Ceiling hangs: the line never pays out to 3.5 m and stays at 1.5 to 1.9 m. In three of four (t 889.9, 945, 2538) Rico hangs frozen 0.6 m under the hook with the line slack and the camera at the ceiling, inside Rico's body.
- The canopy opening out of a reel: the reel-originated deploys (t 261.5, 814.9, 1814.0) end the reel with a 20 to 50 ms jump first, so the roll skip never fires; all three roll the view 150 to 170 degrees through the harness.
- The launcher's burst is still two rockets, then the pod is thrown; the game's own reload start is never logged.

Not exercised: swim stroke yaw (3.4 s of swimming), the Akrobat yoke, the shadow A/B, a tethered barrel swing, the helicopter seat's stunt press, the revolver, two rifles to one shoulder, and a fence carry.

## Tonight's body changes

- The learned estimate turns the legs (d305e9c4, 3fcd73c8). After a head turn held more than 3 s the legs have followed 65 percent of it, against 20 percent under the old rules. But they also follow every glance (median 23 degrees), turn about 55 times a standing minute, and move in 57 percent of standing frames (2.8 percent before). The small turns start turn-in-place shuffles: 16.4 animated foot pivots a minute against 10.1, 83 percent of them following a rig turn. The estimate swings 64 to 84 percent of the way with a head-only glance, more than the 60 percent the tuning assumed, so the 9 degree band is too narrow for it.
- The arm remap (2b54ac38, 6947b3ad): the drawn elbow is near full extension in 15 percent (left) and 19 percent (right) of on-foot frames, against 36 and 47 percent before and the 8 and 14 percent the commit predicted. The shoulders sit about 1 cm ahead of the sternum (9 cm before). A hand at rest now hangs on its hip, so the "Left hip" hint shows while walking.
- Rico's head on the neck pivot (c48ab372): standing, the head bone lands 11 mm from the pivot and wide head turns err 3 to 6 mm sideways (70 to 84 mm before). Jogging and sprinting, the head runs 10 to 12 cm off the pivot, lower and further forward, and the camera comes within 6 cm of the head bone in 1.1 percent of on-foot frames, mostly running while looking down.

## New problems

1. The estimate-driven legs turn on glances and shuffle the feet (above).
2. Vehicle entry and exit animations swing the view 33 to 100 degrees in 0.1 to 0.5 s with the sticks idle, up to 18 degrees a frame (helicopters at t 3788.3 and 3872.8, the scooter at t 182.8 and 251.8). The seat-lock ease does not cover them.
3. A crash out of the wingsuit straightens the tilted view in one step (77 degrees in 0.17 s at t 593.1, and six other cases).
4. Frame time: the median rose from 20.5 to 24.5 ms against the previous take, and `engine_update_render` doubled. The estimator runs on the game's device and is the obvious suspect, but the scenes differ and its own GPU time measured about 0.12 ms offline, so the cause is not established; an A/B with the estimator switched off would settle it.
5. A refused C4 planting loses the charge (t 906, 1939, 2100), and at t 575 to 578 ten presses logged detonations with no charge going off.
6. Belt pushes with a magazine still in do nothing and block the trigger (9 pushes, t 1110 to 1122).
7. About 40 percent of the 64 character grabs left the hand empty, and allied soldiers do not go limp although the log says they did (t 3256 to 3261).
8. Opening the debug panel at t 4380 dropped the left hand's SMG.

Before the recording (t −166 to −49), opening the debug panel with the controller chord while the move stick was tilted left Rico walking under the panel, and the panel could be neither used nor closed for two minutes until F8. The walking is fixed by 304ae765; 1daf4598 logs what holds such a wait, since the second half is not yet explained.

Suspected, not confirmed: bursts of lock-then-tap pairs on one reel that may swallow re-fire taps; step-ups that lift the player and drop them back; the seat grapple's "fire refused", which does not say which check failed.

## Frame hitches

None in play. One 546 ms frame at t 0.91 came with the screen recorder starting; the other 12 gaps of 81 to 145 ms fall on loading screens and fast travel.

## The feedback, item by item

Worked on 15 November, against [the reviewer's feedback](philpax_feedback.md). The analysis behind each item, with the evidence from the take and the code, is in [feedback/](feedback/): [camera.md](feedback/camera.md), [grapple.md](feedback/grapple.md), [hands.md](feedback/hands.md), [vehicles.md](feedback/vehicles.md), and [body.md](feedback/body.md). Items marked deferred have a plan in [next-session.md](next-session.md). None of the changes has been seen in the headset.

Capture and the session recording:

- Capturing the eye views in-process, both eyes, for testers on Windows: deferred, planned. The desktop bleed-through is the screen recorder catching the mirror window mid-present.

The body:

- More IK when idle, a lighter blend while walking, IK in the air: deferred, planned (the IK-first on-foot body).
- The chest coming forward while walking (278, 280): b2687bad takes the gait clip's 20 to 27 degree forward lean out of the spine before the torso follows the head.
- Sliding and a slow catch-up (132 to 140): deferred to the IK-first body, which replaces the lean and the slides with planted feet and steps.
- Hiding the body yaw arrows: c76e1fd6.
- The legs off the ground after a landing (899.7): 908cbdea squares the stance only in a standing idle; the fast fall's landing had dragged the feet across the slope.
- Legs while hanging under the canopy (1166) and swinging (322 to 344): deferred to the firm ragdoll, planned.
- Climbing (1313): 794c5626 lifts the body along a face that leans back, so the player no longer has to keep leaning forward. Feet on the face are deferred to the IK-first body.
- Body yaw strongly aligned with movement (2044): not done. The three misses were while sprinting at 5.4 m/s, with Rico facing only 5 to 20 degrees from the head; the sprint clip's pelvis swing and hip bob carried the belt's volumes, which ride the hip joints, by up to 10 cm a stride, and the one grab standing still took. Aligning the body to the movement conflicts with backpedalling, strafing, and the idle IK, so the plan is a belt frame low-passed over a stride and driven by the estimate's pelvis while walking (next-session.md). The seated hips were the cause at 2362 and 3894 (0231e4ec, 30bb9a94).

The camera and comfort:

- The head pitching down entering the wingsuit (295, 592.69, 757.5, 1176): 0934ffbb stops the view following the clip's head through flight and vehicle changes.
- The speed loss from grapple to wingsuit (295): the reel's ending scales speed to about 0.43, not the wingsuit. The lerp needs the reel's exit path and the wingsuit's velocity reverse-engineered: deferred.
- Camera-body-head detachment on get-ups and landings (297, 301, 490, 597.3, 755.4, 824.9, 870.2, 1382.8, 2993, 3824.8): d0402249 has the held view follow Rico's head through ragdolls, get-ups, and deaths, and escape to it when a landing carries the head away for longer than a dip, which keeps the roof-jump fix. f64c1e2b follows the head's offset from the root so a body moving fast is not left behind, and 7efd7a49 keeps the view on the head past a get-up until it is over the root. The body standing up yawed away from the view (490) is not addressed: the follow's target already has no deadband, so why the body stays turned after a get-up needs the recording's rig yaw; planned in next-session.md.
- The parachute roll (262): d4b66fd6. The skip had never run, because the act transition that opens the canopy does not go through the hooked function.
- The camera underground and inside the body on landings (445.8 to 446.6): 03644c9c keeps the view's head above the ground, and 76c5fbfa holds that floor off seats, hangs, climbs, and swims. The lift through the torso at 445.9 was the handover from the wingsuit's animated head to the root-held view blending through the body; f64c1e2b starts such a handover on the head and returns to the root once the head is over it, and counts a head lying low toward the escape. The body seen at 446.576 is Rico's pack in the crash recovery's crouch under a correctly placed view; that is the clip's crouch under a standing view, which the IK-first body addresses.
- Camera-body clipping on a vehicle roof (565 to 572): c06e434b holds the eyes out from the face there.
- The camera inside the body on a landing (1806): the reviewer's guess is right. Through the fast fall Rico faced about 100 degrees from the view, since the body follows the torso only on foot, and the landing turned the root 75 degrees in 0.2 s and put the head 0.9 m in front of the root-held view. Not done: the follow's target already has no deadband on landing, so the fix (keeping the body's heading on the view's through falls, or the airborne IK of the IK-first body) is planned in next-session.md.
- The death animation (576): d0402249 follows the dying head upright, and 03644c9c keeps it above the ground.
- The camera not following the canopy's yank out of the wingsuit (2806): the cause is not established; deferred, planned: whether the opening out of the suit belongs on the reel-opening skip list (next-session.md).
- The view on vehicle entry and exit, and the stepped turn (the note on vehicle entry, 1742.59, and new problem 2 above): 8908e260 steps the view toward the body every half second by up to 30 degrees, and 5b6cd60f rides the vehicle's own turn once the clip ends and has the seat lock's yaw re-base wait for the steps. Entries the mode reads as on foot (the helicopter at 3788 read as a jump) take no steps; the next session checks them. The seat tables it relies on named states that are in none of Rico's graphs; they are replaced.
- Loading screens following the face (582): a413c461.
- Grab circles in a loading screen (740.35): 44110ad3 draws only the panel pointer out of gameplay.
- The dot cursor through the flight (774.24): not established; likely the game's own map cursor left visible after the fast travel. Deferred.

The grapple and flight:

- Grappling a ragdoll (83): 267b4b24 yanks the body in to arm's reach and holds it there for the swing, and 291d5d2f eases it in over the last of the line so it does not fly on through the view.
- The hint laser while grappled: 92bd73eb hides the beam while a line is out and keeps the target ring.
- Shooting while grappled (92): 0545c866 lets the free hand's gun fire on the mod's own lines, on foot or in the air (0fe6198b).
- Tangential jumps and vertical discontinuities at swing engagement (800, 807.9, 833.8, 885.89, 918.54, 927.5): deab9675 replaces the take with a ballistic coast; the line goes taut where the approach ends. As committed, a fast rush coasted onto a line too short to bear its swing and broke it; bc32891f stops the coast at the shortest length the line can swing on, braking along the line, so a level rush loses some speed rather than snapping the line.
- The ground breaking the grapple (802): 59621522 keeps a held swing across the ground.
- The release killing all velocity (906.45): 68d0e245 hands the velocity back after the reel's cancel scales it, and edef4acc stops that on touching down, a new reel, the wingsuit, or the parachute.
- The upside-down hang that jump would not release (844.69 to 865): fb1d4b46 keeps the rider upright on a held line, and 59621522 lets jump release.
- Trapped at the ceiling (950, 2539): fb1d4b46 runs the reel's motion at the arrival, which had frozen Rico, and 5d0305f7 forgets that motion's reel state once the reel or the lock ends; deab9675 lets the hang pay out to its length.
- Regrappling the car by mashing (465 to 468): 7e961110 fires again on a tap aimed away from the locked line.
- Grappling from the wingsuit (1223 to 1244): e23f7afb ends a spent grapple press in any mode.
- The grapple cursor bouncing on a moving vehicle (2926): b3d9d852 places it between its last two ticks' points.
- Grappling onto a helicopter (1794) and the rail (743 to 754, 2634): e4b15544 takes the hang in to its length and damps its swing. The rail and entering from it need reverse-engineering: deferred, planned. 743 to 754 was the fast travel's scripted sequence on the skid, not a grapple; standing on the rail and stepping off into a skydive applies there too, and the rail plan covers it.
- Freefalling but standing (1384): a scripted drop into freefall shown as a ledge fall. Deferred, planned: the body's case for the scripted drop (next-session.md).
- The body swinging left as the parachute opened (1459.4): not a collision; probably the parachute's velocity alignment turning toward a near-zero velocity. Deferred, needs reverse-engineering.
- The parachute handle out of reach (1153.93, 1400): e0ab1a26 rings the reach that takes presses, in place of the column the reviewer asked for, which is not drawn. The handle sits at the top of the riser; moving it to the grip bar needs the bar's offset reverse-engineered. At 1400 the left controller was mostly untracked.
- Pitching the parachute with the handles (1153.93, 2806): e0ab1a26 dives with both handles pushed up or forward; both pulled down already braked.
- The wingsuit arms bent in (370.2): 4447aae7 hangs the estimated shoulders from the neck pivot, so a glance no longer bends an arm.
- Grapple pickups from the ground (3442, 3518): 37bc474b gives a placed charge longer before it counts as snagged. What 3442 was could not be found in the log.

Hands, guns, and inventory:

- A primed grenade lacking the live grenade's effects (115): the game sets the trail effect and sound only at the throw. The fix is to call the same setup at the pin pull, which needs two pyxis bindings: deferred, planned.
- The C4 trigger (478) and the second detonator (489.5): 93a34bc5 detonates from the held charge's trigger, keeps the charge in the hand, and refuses presses the game would not act on; f5b97fdc detonates only with charges on the placed list, since whether the game's count includes the primed charge is unverified. The upside-down detonator was the game's own, from its detonate act; placing it in the hand needs some reverse-engineering, deferred.
- Hands snapping back and forth on the car (510 to 519): 73cd26e4 leads the carrier's motion by simulation steps.
- The yanked gunner falling through the vehicle (532.5): 8fca10c9 keeps the vehicle's bodies out of the player's carry collision set.
- Thrown C4 ignoring the moving frame (556.8, 560.57): 7b94ffec carries the throw with the vehicle's speed, read once a tick, and no longer caps it; 6f895458 bounds the carried part. Whether the measured velocity follows a vehicle while seated is unverified, and the next session checks a seated throw.
- Holsters across respawn (646): becdd9cd puts the game's own holster on the hand's hip and replicates a lone pistol on respawn. The pistol was lost at 291, not at the respawn.
- The clip release doing nothing (1121.42): f4f3006e drops the magazine unretained when retention fails and reports refusals at the push, and 14070339 hides a spareless drop's magazine part and reports refusals raised when the drop runs. Which refusal fired on the take is a hypothesis, so this is not known to cover every case; the records-leak fix in the analysis is not done.
- Knock-out reloads (1886 to 1891): f4f3006e lowers the threshold to 0.75 m/s.
- Reloading in a vehicle: c602149b drops a dry gun's magazine with a double tap of its fire press. A loaded gun's double tap is two shots, so it ejects nothing; the belt push reloads it.
- C4 and grenades while seated (4147): 774ee5b2.
- Rejected grenades falling unnoticed (1629): fa2fdd67 gives a full pouch a long, full buzz.
- Lockers refilling spare ammo: deferred; how the game refills at a locker is not established.
- The dual-wield duplicate appearing on the first grab (1653): the twin is already spawned and hidden. Showing both stacked is deferred.
- Where the grab circles are placed (2362.54): on foot, on Rico's drawn leg holster bones, pelvis, and chest. Seated, the hips hung below the live head; 0231e4ec latches the neck when the player sits, and 30bb9a94 latches it only once the posture reads seated and afresh with every change of the cockpit frame.
- Grenades back into the pouch (2560 to 2580): the hand was released at shoulder height, above the pouch; not changed.
- The weapon flying off after a swing (2526) and the drops while upside down (844 to 865): 0c8dcdff keeps a slow fist past its window from taking anything on the body, which had drawn holstered guns into the hand holding the line; 1ff3fbe4 limits that to the grapple's hand while its line is out, so the free hand's slow fist still reaches the holsters.
- Ragdoll and prop grips spasming (2659, 2674, 3270, the general note): 9f5d1e74 pulls held ragdolls and articulated objects softly and topples a character once, and fc5d5c64 damps a held ragdoll's bodies toward its whole velocity. The point joint for a second hand or an articulated object (it needs a pyxis binding), damping an articulated object, keeping a held ragdoll out of the camera, and holding off the get-up are not done; planned in next-session.md. The softer pull leaves more lag before a hold tears, which the next session checks.
- Hitting people with props and swung weapons (3238, 3265): d46e0002. The calibrated strike now applies even when the native hit reaction acts too; a64524b3 logs both, so the next take shows any blow taken twice.
- Ragdoll grabbing not working (3257): the ally's hit reaction refused the topple; why is not established. 404c405d retries the ragdoll act once for a character still standing 0.3 s into the wait.
- The tripod gunner (3382 to 3389): d9547d62.
- The gun stuttering in the hand (3715.6): probably the same drive and timing as 510; 73cd26e4 may cover it.
- The SMG put away seated (3894): 0231e4ec.
- The popping object in flight (388.682): the holstered rifle and pistol; the mechanism is a hypothesis. Deferred.
- Held props passing through the body (1438.8): deferred, planned; Rico has no physics bodies for his limbs while animated.

Vehicles and mounted guns:

- The scooter (191 to 252): 5409e58f turns the twist grips off for the triggers, and the bars take the wheel's centre latch, cues, and a grip grace.
- Opening doors from anywhere (459, 2702): 84fe4916, narrowed by f6777b14 to the outer face of a door other than a hood or trunk, away from the hinge, so that bracing on a car to climb or ride still holds the car.
- The fighter's cockpit (1097.4): with 84fe4916 the canopy is taken where the hand touches it; pulling the pilot out plays the game's hijack, not a drag.
- Too close to the gun in the gunner seat (535): deferred, planned (a gunner-seat view correction).
- Aiming mounted guns by the handles (536 to 550, 1747 to 1754, 3395): 9c822cc2 keeps the mount's motors running for a handle grip, which is the regression on standalone guns. In a vehicle's gunner seat the game's aim does not turn the turret, and what does needs reverse-engineering: deferred. The hands not drawn on the handles off the seat (the anchored tier exists only in the scripted mounted state) are deferred with it.
- The Y prompt still showing (3395): 7c9d2f06 hides a standalone mounted gun's prompt under the controllers from the moment the gun is seen.
- A torn gun stuck on its mount, the weapons ejected, and the boat's gun (3401 to 3442, 4055): a9035421 removes the use-item fallback, which took the nearest weapon and ejected the player's own. The mount constraints surviving the tear need reverse-engineering: deferred, planned. The snatch still refuses a torn mounted gun, since the hands' table cannot take a heavy gun from the world.
- A mounted gun as a yank target (1735): 0ad0cf9b stops offering a part still on its vehicle, which removes the false target; the yank that tears the gun off as the hand's tear does is not implemented, and waits on the torn gun's constraints.
- The helicopter controls (3791, 3879.97): the utility helicopter's collective and the second helicopter's controls fall back to fixed offsets. Measuring them and an in-game calibration are deferred.

The interface and rendering:

- Freezing the UI cursor on a light pull (730 to 734): f99b19a3.
- Shadows away from their objects (441.91): the shadow cascades' refresh is the leading hypothesis; a render trace is needed. Deferred.

The new problems listed above:

- 1, the estimate-driven legs on glances: 7e9a9e77 widens the estimate's band to 20 degrees and raises the turn gate to 20 degrees a second for now; the IK-first body replaces them (planned).
- 2, the vehicle entry swings: 8908e260 and 5b6cd60f (above).
- 3, the wingsuit crash straightening the view: 0934ffbb (above, next-session check 3).
- 4, the frame time: not investigated; the next session runs the A/B with the estimator off (next-session.md).
- 5, the refused C4 planting and the detonations with nothing going off: 93a34bc5 keeps a refused charge in the hand, and f5b97fdc detonates only with charges placed.
- 6, the belt pushes with a magazine in: f4f3006e and 14070339 (the clip release, above).
- 7, the empty-handed character grabs and the allies that did not go limp: 404c405d retries the ragdoll act; the rest of the grabs that ended empty are not analysed further.
- 8, the debug panel dropping the SMG: deferred, planned (the panel pausing the weapons rather than standing them down).

## The deferred work, built on 15 November

After the review, the reviewer asked for every deferred item to be prepared before the next session, with hypotheses and instrumentation where the data runs out. Three reverse-engineering passes against the release IDB supplied the bindings (4391cc47, f7d9ae53, bfd1a947). None of this has been seen in the headset; [next-session.md](next-session.md) lists the checks.

- Capture (the reviewer's first item): the eye views are captured in the game, both eyes side by side at each eye's aspect, packed to 4:2:0 on the GPU, streamed over loopback to an ffmpeg the injector carries (or the relay's), with WASAPI loopback audio; the gpu-screen-recorder relay stays as the fallback until this is seen working (60514510 to bf3532b9, [capture.md](../../mod/capture.md)).
- Doors win over climbing wherever the hand reaches them, without a speed guard, as the reviewer asked (1ee0a6a0).
- The view keeps its heading through a get-up's turn of the root (490), which turned it 170 degrees in one step (085c13d6). For 1806, the recording shows the body held 72 degrees off the view through the fall, the follow being off foot, and turned onto the head in 0.2 s at the landing; the drawn body's turn in the air belongs to the IK-first body's airborne IK.
- The belt holds steady through a stride (2044, 1f30d3ae); the debug panel pauses the weapons (4380, d07ac886).
- Heavy weapons: the table holsters a heavy weapon the pickup leaves dropped, so the hands take a minigun from the world and the snatch takes one (dea4e18f); a torn gun is freed from its mount's hinges, which the game's drop leaves enabled, and offers its pickup (ec47605c).
- The primed grenade gets its trail resources at the pin (39e1bde8); the detonate act's detonator is held in the pressing hand (709cc26e).
- A manned vehicle gun turns toward the hands rather than the look input (f682b974), and the gunner seat's view sits back from the handles (1b74bd18).
- The canopy's turn toward the velocity is held off at its opening (1459, f9b57d67); a reel's speed is carried into the fall's and the wingsuit's speed cap (295, 51f02d5a); a helicopter reel's door-side arrival plays as the game's (8441523e); a free canopy handle sits at its attach bone (1153.93, 8a433477); the game's skydive is presented as the wingsuit is (1384, bc45775c); the canopy skip's ordering is confirmed (1f21ebc0).
- The firm ragdoll hangs the legs from the physics on the grapple line and under the canopy (a857485a); two hands on one ragdoll hold it by points (590b92b0); Rico's drawn torso has capsules held objects meet (1438.8, 1d571ce7).
- Lockers: the take no longer empties a locker weapon's stored rounds, the likeliest cause, and the reserve's rises and the auto-credits are logged (de1dae90).
- A rack pair's two pistols are drawn apart (1653, 5f52671d).
- Instrumentation: unlisted standing idles (a46cb528), the cockpit part and the pilot's eyes in the driving position line (8c9499a0), and a calibration log of the hands in a cockpit part's space (a18fa1ef).
- The IK-first on-foot body (the reviewer's idle IK): standing on foot, Rico's pelvis is placed by forward kinematics from the learned estimate, the feet are planted contacts that step only by rule (reach, a held twist, balance, the root carried, a tidy at rest), the grounded rig's root follows the floor under that pelvis, the walk keeps 0.2 of it on the pelvis and the spine, and the air is IK between an animated takeoff and landing ([ik-body.md](../../mod/body/ik-body.md)). Whether HumanIK holds the seeded pelvis with both feet planted without the torso fighting is the first thing to see; moving decks are treated as static.

## The open items, built on 15 November

The reviewer then asked for the open items to be addressed, and chose to keep the scooter's twist grips until the triggers are tried, to keep the seated magazine drop for dry guns only, and to add a sound to the full pouch. Further reverse-engineering supplied the bindings (ba25f69a, a0cb71a9, eb1b7e3c). None of this has been seen in the headset either.

- A full grenade pouch clicks as well as buzzes when it refuses a grenade (1fdff148).
- The body estimator's GPU pass has a frame-timing slot of its own (eea547fb).
- The gunner's hands: a mounted gun's use fades the local player out through the character's opacity fade, so the seat now fades them back in (e041f66b).
- The heavy troop helicopter's controls are measured from its model. The utility helicopter has no collective mesh, so its collective sits relative to the cyclic, and so does any other cockpit's missing collective (e0837e02).
- The rail: a reel onto a helicopter's door side keeps Rico on the skid. The drop into the stunt hang at the arrival clip's end is refused, so the door-side state holds him, Y still enters, and the jump steps him off into a fall with the airframe's velocity (b2b25ee2, `vehicles.helicopter_rail`).
- The hands: a held articulated object is damped against its assembly's velocity (e8571c18). A prop resting on the torso capsules neither pushes nor tears the hold (ab7afba8). The hands are drawn onto a standalone mounted gun's handles off the seat (f5fb1380). A held ragdoll's get-up is held off by writing its ragdolling timers back each tick (9b5ae399, then 6f03be73 once the timers were bound).
- The IK body: planted feet are carried on a moving support, and the pelvis speed and the root drift are measured against it (c206b2be). The feet brace on the face while the hands climb (a3835ff2). The firm ragdoll's muscle target is composed from the drawn body above the hips and the IK legs below (576395f8), and a detour on the ragdoll's pre-physics step hands it over (24eb63f5).
- The offline replay of the IK feet over this take (rig-eval `--ik`) found the reach rule stepping 156 times a standing minute, because a tall player's FK hips sit just past the straight leg. With a reach allowance for the pelvis drop and retuned thresholds, it predicts 3.7 steps a standing minute and 0.17 per glance, at the cost of the pelvis turning up to 65 degrees over the feet for most of a second before they step (0da7f375, [ik-body.md](../../mod/body/ik-body.md)).

## The rail and the helicopter hang, built on 15 November

The reviewer asked for the last open items too. The rail now stands Rico on foot on the skid at the door-side arrival's end, out of the helicopter's graph, carried with the airframe's motion like the moving climb, free to walk along the skid's line under a sticky pull, falling into the skydive past the step-off distance, and handing over to a hand climb to hang under the skid by choice. Y in the line's hang below a helicopter lets the reel arrive into the game's hang, which enters, instead of opening the wingsuit (1800). The twist grips and the seated magazine drop stay as the reviewer chose until they are tried.
