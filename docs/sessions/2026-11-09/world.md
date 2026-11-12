# World, vehicles, grapple, and flight: 9 November take

The take is `sessions/2026-11-09_04-17-04_716/recordings/2026-11-09_04-17-12_573/`. The tracking header identifies the build as `2026-11-09 04:12:42`. It includes the collision-binding crash fix and the ammo-counter fix. Times below use video mm:ss and recording seconds (rec). Recording zero is `2026-11-09T03:17:12.573Z`; video seconds = rec seconds - 0.961668. The video lasts 38:34.087. The [annotated log](log_annot.txt) retains original `session.log` line numbers; the [timeline](timeline.txt) covers the whole session.

The main improvements are usable detached-door carries, successful barrel play, and reliable observed NPC-gun catches. The main failures are seated grapple firing and sustained direct ragdoll grips. Native selected-body attachment is exercised on props and bodies, but detached-door snatch is not retested. Aircraft flight uses the utility helicopter and a fighter jet; it does not validate the native fitted controls for other aircraft.

## Timeline

| Video | Rec | Interaction and observed result |
| --- | --- | --- |
| 01:01-01:25 | 62-86 | Yellow utility helicopter door opens, tears off, and remains held while the user moves around the helicopter. The game eventually frees the held body. No crash follows. |
| 01:33-03:18 | 94-199 | Helicopter flight with a left collective and a right cyclic. A late left-B tap produces no visible grapple. |
| 04:19-06:01 | 260-362 | Grapple traversal and a sustained wingsuit flight along the mountain road. |
| 06:15-06:56 | 376-417 | Six placed-C4 retrievals. A seventh occurs at 16:16 (rec 977). |
| 08:16-08:58 | 497-539 | Both buggy doors open and detach. Each detached door is carried briefly and released. Wheel grips then steer the buggy. |
| 09:13-10:57 | 554-658 | Fighter-jet cockpit and flight. Repeated throttle and flight-stick grips work as virtual controls. Five left-B cycles during flight produce no observed wire. |
| 11:27-11:52 | 688-713 | Vintage sports-car hood opens twice, tears off, and is carried and transferred between hands. |
| 12:50-13:40 | 771-820 | Close NPC interaction and strikes. Standing-NPC lifting is not demonstrated. |
| 13:57-14:41 | 838-882 | Repeated grips on a mounted gun as an articulated object. The fixed assembly does not become a portable carry. |
| 15:07-15:16 | 909-917 | Three brief direct ragdoll-grip episodes, including a two-hand attempt. Two episodes conclusively tear while the grips remain closed. |
| 15:32-15:55 | 934-956 | On-foot tethers near a large prop, with two failed two-hand articulated-object holds. |
| 16:44-17:00 | 1005-1020 | One barrel pull is cancelled; another barrel reaches the left hand, transfers to the right, and remains held until release. |
| 18:19-18:29 | 1100-1110 | Two snatch attempts at a locker fixture snag. Neither becomes a carry. |
| 19:02, 24:05, 26:18, 26:39, 35:01 | 1143, 1446, 1579, 1601, 2102 | Five NPC-gun catches, all into the left hand. |
| 19:55-20:28 | 1196-1229 | Two successful held-line body drags, separated by three hook-fire refusals. |
| 23:22-23:47 | 1403-1428 | Barrel pickup, carry, release, and explosion. The later sequence carries one barrel in each hand, throws them, and shoots them. |
| 27:19-31:16 | 1640-1876 | Short and sustained wingsuit segments, grapple traversal, and brief jet-seat entries. The sustained departure from the runway crosses the valley. |
| 32:34-33:12 | 1955-1993 | Blue utility helicopter door opens and detaches. Ten rapid left-B cycles in the locked seat produce no observed wire. |
| 35:13-35:19 | 2115-2120 | A bus door opens, but the new hold tears away immediately. The door later closes and relatches. |
| 35:57-36:37 | 2158-2198 | An on-foot tether persists through the approach to a motorcycle. Its existence does not establish seated grapple firing. |
| 36:24-37:45 | 2185-2266 | Motorcycle grips, riding, regrips, and one-handed control. |
| 37:49-38:07 | 2271-2288 | Four parachute-handle acquisitions, including a two-hand interval. |

## Doors and carry

Five explicit detach requests return `Detached`: the first helicopter door at rec 65.20, the two buggy doors at 499.43 and 514.43, the sports-car hood at 706.84, and the second helicopter door at 1957.94. Frames after all five show the detached panel at the hands. This is stronger evidence than a successful native return alone. It also exercises the interaction that previously crashed before the collision-binding fix.

The first helicopter panel remains in the left hold for about 20.8 s after detachment. The right hand is free again by rec 72.83 while the left continues carrying. At rec 85.97, the log reports that the held body has been freed (`session.log:439`). This is a remaining lifetime failure, not a grip release or the earlier collision-binding crash. The video shows the panel retained near the user before that loss.

The two buggy panels remain carried for approximately 3.3 s and 3.1 s after detachment. The hood remains in the left hold for approximately 4.4 s, transfers to the right, and is released about 0.9 s later. The second helicopter panel remains in the left hold for approximately 2.1 s. These four detached carries end with release records rather than the immediate tear losses seen on the previous take. Short retention does not establish that every door is now easy one-handed or that long-distance moving carries are stable.

Attached parts still lose grips. The second buggy door loses its right supporting hand at 08:30 (rec 511.14), before the left hand detaches it. The first hood opening loses its hold at 11:29 (rec 690.93); the hood relatches before the next attempt. The bus door is worse: the latch releases at rec 2114.87, and the hold tears away about 0.025 s later (`session.log:6040-6041`). The inspected 35:13.95 frame shows the hand at the open door, without a sustained carry. A jet-related door hold also tears away at rec 657.92; this is not a detached-panel carry success.

Barrel play works across several operations. At 16:53 (rec 1014.48), a snatched barrel reaches the left hold. It transfers to the right at rec 1016.96 and remains held until rec 1020.36. The barrel is visible close to the user in the arrival and post-arrival frames. At 23:22-23:28 (rec 1402.91-1408.51), a barrel is carried and released before an explosion. At 23:38-23:47 (rec 1419.10-1427.10), one barrel is held in each hand while the user moves toward the fence. Both are released, and shooting produces the visible explosion around 23:49-23:50. The saved `video-pickup-and-boom.mp4` covers this later pickup-and-explosion sequence, beginning around video 23:36; its exact subsecond offset is not independently matched.

These sequences show functional multi-step play. They do not prove that held props never contact Rico, that every constrained body receives the intended collision exclusion, or that world collision remains unchanged. Contact-pair instrumentation is not inspected at that level. Survival with the collision-exclusion path enabled is a regression result for the crash, not a complete collision-policy result.

## NPC grips and articulated objects

Direct ragdoll grips acquire, but do not sustain a lift. There are four hand acquisitions in three episodes:

| Video and rec start | Hands | End | Outcome |
| --- | --- | --- | --- |
| 15:07.81, rec 908.77 | Left | Rec 910.35 | The physical hand joint ends with a lag reason about 1.6 s after acquisition. The squeeze is already open by then, so this is not a confirmed forced carry loss. |
| 15:10.89, rec 911.86 | Left | Rec 912.81 | The hold tears away after about 0.95 s. |
| 15:15.75-15:15.79, rec 916.71-916.75 | Both | Rec 917.24 | Both holds tear away after about 0.5 s. |

The second and third episodes are confirmed forced losses: the squeeze-force tracking remains well above release force around rec 912.81, and both hands remain closed at rec 917.24. The first episode is ambiguous. The left squeeze falls from full force to open between rec 909.86 and 910.19, before the joint teardown at rec 910.35. Its lag reason does not override that input evidence. Thus two of three episodes, comprising three of four hand acquisitions, conclusively tear while held. The frames show a body still on the ground and brief local movement, not a stable one-hand or two-hand carry. No sustained corpse lift is demonstrated in any of the three episodes.

Standing NPCs are approached, touched, and struck. No recorded standing-character acquisition demonstrates preservation of an anatomical grip through toppling. The later ragdoll acquisitions demonstrate selection of individual ragdoll bodies, but not the full standing-to-ragdoll transition requested in the earlier feedback.

Mounted-gun grips must remain a separate category. Ten hand acquisitions target the same roughly 25 kg articulated gun between rec 837.96 and 880.75. The video shows manipulation at the mount, not a freely carried prop. A larger articulated prop is gripped with both hands twice at rec 941.41 and 953.39; all four hand holds tear away within about 0.4-0.6 s. These constrained-object failures do not invalidate the barrel carries, and the barrel carries do not validate constrained-assembly transport.

## Snatch and selected-body attachment

The log contains 21 fired snatch attempts. Two additional object selections are armed and cancelled without a yank, at rec 757.67 and 1654.66; they are excluded from the fired denominator.

| Target category | Fired | Result | Evidence limit |
| --- | --- | --- | --- |
| Placed C4 | 7 | Seven retrieval records, all to the left hand. | The recording shows repeated retrieval gestures and subsequent handling. Individual charge arrivals are not all resolved in detailed frames, so 7/7 is the logged delivery result. |
| NPC-held gun | 5 | Five visible left-hand catches, matching five delivery records. | No right-hand catch or seated gun snatch is exercised. |
| Body | 5 | Two attached held-line drags; three hook-fire refusals. | The two bodies move on the ground. Neither becomes a hand catch or a sustained lifted carry. |
| Object | 4 | One barrel catch, one pull cancelled by grip release, and two locker-fixture snags. | Different target classes; this is not a four-attempt barrel success rate. |

The gun catches occur at rec 1143.25, 1446.28, 1578.74, 1600.55, and 2101.97. Inspected before/after frames show the arriving weapon at the left hand. At 26:39, the right hand retains its existing long gun while the left catches another. This confirms the observed NPC-disarm-and-catch loop, including preservation of the other hand's weapon. It does not retest the previous take's right-hand catch rate or late-squeeze timing.

The first body drag runs from rec 1196.38 to 1206.44, about 10.1 s. The second runs from rec 1226.90 to 1228.99, about 2.1 s. The video shows a line and substantial movement of the selected body over the ground. Tracking records an attached custom wire and an active sling during the first drag. This is actual native attachment and held-line use, not a conclusion from the existence of selected-body code. The intervening attempts at rec 1220.06, 1222.98, and 1225.22 all return `the hook did not fire` without reaching a held-line state.

The barrel arrival at rec 1014.48 also follows an attached custom wire in tracking. Together, the barrel and body sequences exercise attachment, pull, hold, and release on these target classes. The images and telemetry do not expose the selected rigid-body anchor precisely enough to certify off-centre torque or every anatomical attachment point. No fired snatch targets a detached vehicle door. The previous take's nine failed torn-door hooks therefore remain an open retest, even though direct detached-door carry now works.

The two locker attempts at rec 1100.66 and 1109.51 reach an attachment and then report `the target snagged` after roughly 0.6 s. Frames show the line aimed into the locker fixture. They should be investigated as fixture eligibility or constrained-target handling, rather than counted as ordinary loose-prop arrival failures.

## Seated grapple

The intended controls are a short left-B tap for a persistent vehicle-to-target tether and a long left-B hold for a target-to-tracked-left-hand line. Releasing the long hold while aimed transfers the hand endpoint to the aimed target; releasing into empty space discards it. There is no grip modifier. Committed tethers retain normal tension breakage and explicit retract-all release. This take does not reach those downstream outcomes.

Tracking records 16 complete left-B press/release cycles while the seat is locked:

- One in the first utility helicopter, rec 192.34-192.50 (03:11.38-03:11.54). The nearby aim sample is empty, so this alone is not a valid-target failure.
- Five in the fighter jet, rec 611.01-613.96 (10:10.05-10:13.00). Four last approximately 0.17-0.33 s; one lasts 0.67 s. Aim telemetry includes valid, tetherable surface hits during this interval.
- Ten in the second utility helicopter, rec 1979.58-1981.78 (32:58.62-33:00.81). These are short taps of approximately 0.08-0.19 s. Aim telemetry shows valid ground hits, although they are not marked tetherable by the ordinary aim readout.

No `seat grapple:` fire, refusal, miss, cancellation, or handoff record appears anywhere in the session log. No inspected locked-seat tracking sample contains an active grapple, and detailed frames show the aimed left gauntlet without a line during the repeated helicopter taps. The jet presses also have no visible wire. The absence of logs is corroboration here, not the sole failure criterion. The result is an exercised input-to-fire failure, strongest in the jet interval with valid/tetherable hits. The exact routing or eligibility condition responsible is unverified.

The jet's left hand is squeezed during its presses. The helicopter taps vary between open and squeezed hands. Neither should require a grip modifier. Adding a squeeze requirement would contradict the intended contract, and would not explain the jet failure.

Persistent vehicle-to-target commitment, tracked hand-endpoint motion, aimed-release transfer, empty-release discard, seated retargeting, seated NPC-gun snatch, and seated retract-all remain unverified because no seated wire is established. The motorcycle's pre-existing tether begins on foot at rec 2158.84 and disappears at 2198.21. It is not evidence of successful motorcycle grapple firing. No left-B press occurs during the motorcycle riding intervals. The buggy's B presses are right-hand inputs, not left-B grapple tests.

## Vehicles and flight controls

The observed control grips are stable in the narrow sense established by the logs and inspected frames. There are 36 control acquisitions and 36 corresponding releases. Thirty-four releases say `grip released`; the final two motorcycle holds end with `gone` when the user leaves the motorcycle for the parachute. There is no reported drift-distance release.

| Control | Acquisitions | Total held time | Observed coverage |
| --- | --- | --- | --- |
| Buggy wheel | 4 | 18.8 hand-seconds | Two-hand grips, release, regrip, and visible steering along the road. |
| Motorcycle handlebars | 19 | 72.5 hand-seconds | Both hands, repeated regrips, and right-hand-only intervals during riding. |
| Helicopter collective | 1 | 75.9 s | Left hand; sustained utility-helicopter flight. |
| Helicopter cyclic | 2 | 65.1 s | Right hand; aircraft attitude changes during flight. |
| Jet flight stick | 3 | 80.7 s | Right hand; take-off, sustained banked flight, and eventual exit. |
| Jet throttle | 7 | 11.6 s | Left hand; repeated short holds. |

Hand-seconds sum both hands where both can grip a control; they are not elapsed driving time. A helicopter cyclic is not a jet flight stick, so the absence of a helicopter `flight stick` acquisition is expected in these records.

The utility helicopter and fighter jet use virtual control targets in this take. The detailed 09:45.90 jet frame shows the target near the base of the visible cockpit stick, below its native handle. Successful flight therefore does not demonstrate native-handle registration. Neither the light-assault aircraft nor the small floatplane, the two models identified in the earlier implementation feedback as native-fit cases, receives a cockpit-control retest. The take does not justify a claim that native aircraft controls are fixed generally.

Motorcycle grips are newly exercised relative to the previous report. Visible riding and handlebar registration work well enough for the user to regrip and continue. The final change to parachute occurs around rec 2266.02. It demonstrates a vehicle-to-canopy transition, but is not an isolated long-A-at-speed regression test with the same release timing as the previous failed bail. The earlier bail failure should remain open until that contract is reproduced deliberately. Nitrous and vehicle-mod button behavior are not isolated either.

## Wingsuit, parachute, and grapple traversal

Sustained wingsuit flight runs at approximately 04:31-06:01 (rec 272-362), 30:19-31:16 (rec 1820-1876), and 33:57-34:55 (rec 2037-2096). Detailed frames inspect the first interval through 05:58; the mode transition records its end at rec 362.3. The runway departure enters the wingsuit at rec 1819.873 (30:18.9). The earlier interval traverses the mountain road; the later intervals cross terrain rather than ending immediately on activation. Shorter suit openings and repeated grapples appear around the airbase. These observations establish traversal, not a measured improvement in take-off alignment, momentum retention, arm-driven swing, or flight-wrist convergence.

The four use-button wind-ups at rec 842.39, 846.45, 871.51, and 876.30 occur while the user manipulates the mounted gun. The sampled frames do not show a resulting launch. They must not be counted as successful gaze-aligned take-offs merely because a heading record exists. The later ground-use record at rec 1779.09 is beside or under the jet; it also does not establish a clean take-off test. No comparable heading-error measurement is made against the previous report's take-off results.

Ordinary grapple lines and reel traversal work repeatedly. Four on-foot tether commitments around rec 934.97, 940.09, 952.17, and 1016.39 are logged before the late town tether. None should be relabelled as a seated operation. Tracking contains no active lock state in the reviewed recording, so the previous take's held grapple-lock results have no comparable new denominator. Relaxed swing legs, incoming-momentum preservation, and lateral arm pumping remain unverified.

At 37:49-38:07, the left parachute handle is acquired three times and the right once. All four releases are explicit grip releases. Frames show the canopy descent and a two-hand interval. This validates handle acquisition and retention during that descent, but does not isolate body-yaw decoupling from travel direction. Breaststroke swimming, canopy-cut-to-suit latency, and the prior early-jump retry failure are not retested with a controlled comparable sequence.

## Comparison with 7 November and remaining work

| Subject | Previous report | Current result |
| --- | --- | --- |
| Detached-door carry | One of three torn doors retained usefully; two heavy roadster doors lost rapidly. | Five of five explicit detaches visibly become carries. Four reach release without an immediate post-detach tear. The first helicopter body is eventually freed while held. Different models and short carries prevent a direct heavy-door strength comparison. |
| Detached-door snatch | Nine fired attempts at one torn door fail. | No detached-door snatch attempt. The previous failure is not closed. |
| Loose-prop arrival | One light prop is lost almost immediately. | A barrel reaches the hand, transfers between hands, and remains held for about 5.9 s before release. Two-hand barrel play also works. |
| Direct NPC carry | Standing/toppling and corpse handling require validation. | No sustained lift in three brief corpse-grip episodes. Two episodes conclusively tear while the grips remain closed; the first ends after the user opens the squeeze. Standing-to-ragdoll grip preservation is not demonstrated. |
| Weapon snatch | Seventeen catches, including eight right-hand catches; four misses. | Five of five NPC-gun attempts visibly catch into the left hand. This is narrower coverage, not proof that the right-hand or late-catch cases improve. |
| Seat grapple | Four old fire-and-tether operations reported as working. | The replacement is attempted in helicopter and jet seats but never establishes a wire. The old result does not validate the new tap/hold contract. |
| Vehicle grip drift | Zero drift losses in eleven releases. | Zero reported drift losses in thirty-six control releases; motorcycle, helicopter, and jet controls now have observed coverage. |
| Native aircraft controls | Asset/control fitting remains unresolved. | Utility and fighter-jet virtual controls support flight. Native-fit light-assault and floatplane controls remain unexercised. |
| Grapple lock and flight timing | Four sustained locks, measured canopy-cut opening, and take-off headings. | Traversal is visible, but lock, retry, momentum, and heading regression criteria are not measured. |

The next world-interaction investigation should prioritize the seated input-to-fire failure, the rapid direct ragdoll-grip losses, the bus-door acquisition loss, and the held helicopter body's destruction. Detached-door selected-body snatch still needs its own ground-level test. Collision exclusions need explicit player-contact, world-contact, multi-body, and final-release checks. Aircraft-native control fitting, sustained heavy-door transport, standing-NPC toppling, and a matched long-A bail test remain coverage gaps rather than successful fixes.

## Evidence and limits

The review uses 30-second contact sheets across the whole video, denser timestamped sheets around doors, NPC grips, snatches, seated inputs, aircraft controls, and flight, and full-resolution extracted frames for selected failures. It also inspects the barrel highlight and the corresponding main-video interval. Original `session.log` outcomes are aligned with tracking controller inputs, seat phases, aim results, native wire state, sling state, and vehicle-control records. Tracking is decoded with the existing `tracking-synth dump --json` tool. The final recording interval is checked separately after the main telemetry extraction.

Counts refer to recorded acquisitions, explicit fired snatches, or raw button cycles as specified, not to every possible user intention. Sparse video samples cannot establish continuous absence of clipping, exact attachment torque, or precise control feel. A returned call or logged catch is not treated as a visible gameplay success without frame corroboration. The terminal WaveWorks crash is handled in the session overview; it is not attributed to a door grip or a collision-filter call here.
