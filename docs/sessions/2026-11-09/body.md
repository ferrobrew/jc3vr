# Body, arms, camera, and performance: 9 November take

## Recording and method

The take is `sessions/2026-11-09_04-17-04_716/recordings/2026-11-09_04-17-12_573/`. The recording header identifies build `2026-11-09 04:12:42`, which includes the door-collision crash fix and the dual-wield ammo-anchor fix. The video is 1920x1080 and lasts 2314.087 s. Recording time starts at `2026-11-09T03:17:12.573Z`. Video time is recording time minus 0.961668 s. Times below name both clocks where a narrow event matters.

The comparison reference is [the 7 November body analysis](../2026-11-07/body.md). The session-feedback follow-up records the intended interventions: a deep-crouch root correction, actual seated waist transforms and targets, full-arm flight correction with a shared held-object endpoint, relaxed pendular legs, arm-pump locomotion, and removal of animated-hip feedback into the root. It also explicitly leaves residual rifle idle settling and performance fixes outside that implementation scope. Implementation history supplies the test questions, not the outcome.

Visual inspection covers 30-second contact sheets across the take, denser sequences at video 03:50-04:08, 12:54-13:07.5, 08:55-09:22, 02:58-03:14, and 33:56-34:50, and individual detailed frames at 03:58, 13:04, 09:16, and 34:20. These are samples, not a continuous frame-by-frame review or a headset comfort assessment.

The tracking file contains 116,583 frames and 572,270 events, with no truncation or skipped records reported by the reader. It includes 114,768 `drawn arm joints` diagnostics. These diagnostics sample the post-presentation palette in model space, rather than the earlier HumanIK result. The old report's HumanIK-derived hand errors and elbow-flip estimates are not a compatible numerical baseline. No percentage improvement is calculated between those measurements and the new palette measurements.

For the wrist checks, each palette sample is joined to the nearest controller state, final character transform, and tracked frame, with a maximum time separation of 35 ms. The model-space wrist is placed using that final character transform. The comparison wrist uses the tracked controller grip plus the configured mirrored grip-to-wrist translation, placed relative to the recorded eye through the recorded view rotation. Character speed must be at most 0.1 m/s. This limits translation interpolation error, but does not remove fast rotation, tracking publication skew, remapping, or anatomical calibration error. Counts are hand samples, combining both sides; consecutive samples are correlated, and are not independent trials. The simulation target comparison uses `requested_wrist`, not an independently recorded final presentation target.

Numerical extraction used the existing `tracking-synth dump --json` command and temporary analysis files under `/tmp/jc3vr-body09/`. No payload, tool, or binding changes are part of the analysis.

## Intervention coverage

| Intervention or previous finding | Status in this take | Evidence and limit |
| --- | --- | --- |
| Eye-relative hand registration | Supported in stationary samples | Ordinary on-foot samples have a 2.90 cm median drawn-to-controller wrist error. Fast motion and outliers remain. |
| Shoulder registration and natural reach | Partial | The hands reach forward, down, across the body, and toward objects. The detailed floor-reach frame shows a continuous arm. There is no external shoulder reference or calibrated reach sweep. |
| Deep crouch and pelvis correction | Partial success | The rig reaches a 79.8 cm crouch drop. Controlled crouch windows show small drawn-wrist residuals, but pickup transitions still contain larger errors. |
| Seated waist socket placement | Partial coverage, not accepted | Hands and weapons move beside the thighs in the helicopter. General seated hand registration is measurable. Repeated capture at the visible holster is not established. |
| Stop-turn root feedback | Supported in a bounded sample | Sixteen qualifying native stop-turn entries have no large uncommanded root rotation over the following 1.2 s. Native stop-turn animations still occur. |
| Square hips, square feet, look-down suppression, and idle settling | Insufficient retest | Look-down and locomotion are present, but no matched quiet-spell or foot-yaw analysis is performed. The prior rifle idle-settling issue is not closed. |
| Rifle pumping | Insufficient retest | Long guns, walking, aiming, and manipulation are present. No matched controller-versus-drawn acceleration analysis establishes the remaining pumping amplitude. |
| Drawn elbow stability | Recording coverage succeeds; motion result remains open | Actual palette elbows are now recorded. No compatible flip-rate comparison or exhaustive discontinuity review is performed. |
| Full-arm flight correction | Partial visual coverage | Wingsuit and parachute flight are exercised. Most extended arms are outside the first-person image, and the ordinary controller-owned late-target diagnostics do not describe sustained wingsuit ownership. |
| Shared flight wrist and held-object endpoint | Insufficient retest | No sustained held gun is identified during the wingsuit samples. Passing empty-handed flight does not validate a held endpoint. |
| Pendular legs and arm-pump locomotion | Unexercised | No controller-state sample contains an active grapple lock. Ordinary grapples and flight do not test this intervention. |
| Breaststroke recognition | Unexercised | No swimming mode or swim rule state is recorded. The camera sees water at a distance, not a swimming test. |
| Baseline preservation on resume | Unexercised | Three manual recenters occur, but no automatic recenter is logged. The two menu intervals do not reproduce the previous low automatic baseline. |
| Performance | Mean cadence improves; long stalls remain | Timed mean cadence is 50.36 frames/s. Eight intervals exceed 500 ms, with their matching timing-window maxima concentrated under `sim_steam`. |

## Body, shoulders, and crouch

Video 03:50-04:08 (rec 230.962-248.962) includes repeated looks at the waist, lowered hands, dual guns, and a deep crouch. The detailed frame at video 03:58 (rec 238.962) places the camera near the grass and the visible waist gear. The tracking rig crosses 35 cm of crouch drop at rec 238.98 and reaches about 74 cm in this spell. This is a physical height excursion, rather than merely a crouch-button animation.

The deepest rig sample is at rec 785.050 (video 13:04.088), with a 79.8 cm drop. The detailed video frame at 13:04 shows the left hand contacting the rifle near the ground and a continuous forearm reaching toward it. The surrounding sequence shows the user lower the hand toward the rifle and then return to an upright view. The close view does not expose the full pelvis, both knees, or both shoulder roots. It therefore supports usable floor reach, not a complete anatomical pose assessment.

| Stationary sample set | Hand samples | Drawn-to-controller wrist median / p95 | Drawn-to-simulation-target median / p95 |
| --- | ---: | ---: | ---: |
| On foot, eye drop below 15 cm | 66,684 | 2.90 / 17.38 cm | 0.61 / 5.81 cm, 65,826 samples with a target |
| On foot, eye drop at least 35 cm | 1,408 | 2.65 / 10.99 cm | 1.18 / 5.18 cm |
| Rec 238-240.7, video 03:57.038-03:59.738 | 192 | 2.28 / 3.88 cm | 1.25 / 4.90 cm |
| Rec 889-892, video 14:48.038-14:51.038 | 126 | 2.64 / 4.94 cm | 0.66 / 4.17 cm |
| Rec 784-786, video 13:03.038-13:05.038 | 144 | 2.81 / 33.29 cm | 1.69 / 25.84 cm |

The controlled crouch windows do not show a persistent large wrist displacement. The floor-pickup window has substantial transient error, despite a good median. Another outlier occurs near rec 909.71 (video 15:08.74): the left drawn wrist is about 56 cm from the eye-placed tracked wrist, but only about 5 cm from the simulation target. The latter distinguishes target placement or interaction ownership from simple failure to reach the requested target. These outliers are not assigned to one solver without a synchronized ownership and interpolation investigation.

The shoulder result remains less certain than the wrist result. Small wrist error can coexist with an incorrect shoulder origin, compensating clavicle motion, or an unnatural elbow. The detailed frames do not show an obvious detached forearm at the inspected reaches, but the player has no recorded external shoulder tracker. A claim that shoulder anatomy is now correct would exceed the evidence.

The camera-to-head registration also behaves well when stationary on foot: 32,968 controller-state samples with exactly zero recorded character velocity have a median positional difference of zero and a p95 of 0.30 cm between `head_world` and the rendered camera. This does not measure visual comfort during traversal, or bound every camera snap. Moving-frame differences include render interpolation and must not be interpreted as static registration error.

## Stop-turn and lower-body yaw

There are 382 nonperiodic stop-state entries: 77 name a run/walk-to-stop-turn state, and 305 name a plain run/walk stop. The 77/382 proportion is 20.2%. These are recorded state entries, not independently detected walk stops. The 7 November report used 228 detected walk stops as its denominator. Its 35% figure is therefore not directly comparable.

A narrower test examines the actual root after a native stop-turn entry. The following 1.2 s must remain on foot, every sampled character position must remain within 20 cm of the first, and no sampled rig output may request a turn. Sixteen entries qualify. Their absolute root-yaw changes have a median of 0 degrees and a maximum of 4.57 degrees. Some qualifying windows overlap, so this is not sixteen independent controlled trials. Examples include rec 43.762, 238.981, 908.231, 1068.098, 1216.694, and 1599.041.

This supports the intended separation between the native stop-turn animation and root steering. It does not show that the animation itself is gone. It also does not retest all quiet rifle settling, low-gaze suppression, or moving leg reversals. An initial looser test found larger yaw changes when it admitted windows with a later explicit rig turn; those changes are not counted as uncommanded feedback.

## Seated waist and flight body

Video 02:58-03:14 (rec 178.962-194.962) shows the helicopter lap, hands beside the thighs, a gun held across the right thigh, and hand movements toward the visible waist area. Video 08:55-09:22 (rec 535.962-562.962) shows the truck controls, exit, and a military aircraft cockpit. The detailed aircraft frame at video 09:16 (rec 556.962) shows the right hand near the thigh and the separate flight-stick hint between the knees. A control hint is not evidence that a holster socket matches the physical waist.

Across 7,612 stationary, vehicle-locked hand samples, drawn-to-controller wrist error is median 2.46 cm and p95 12.09 cm. Drawn-to-simulation-target error is median 0.37 cm and p95 1.71 cm. This supports general seated hand registration. It does not locate the holster target, measure its separation from the visible gun, or establish reliable draw/dock cycles. Seated socket placement remains an explicit acceptance gap rather than a confirmed failure or success.

Flight coverage includes rec 272.1-362.3 (video about 04:31-06:01), 1819.9-1875.8 (30:19-31:15), and 2036.9-2095.4 (33:56-34:54) in the wingsuit, plus parachute intervals and transitions. The late flight sequence shows sustained travel, banks, and occasional visible arm or wrist edges. Most sampled frames omit the arms or show only a partial limb. The detailed video 34:20 frame contains no arm with which to judge the complete shoulder-elbow-wrist chain.

The recording contains 10,775 drawn-arm diagnostics while the most recent controller state is wingsuit. However, 10,714 of 10,777 wingsuit controller states label both hands `yielded` with zero ordinary controller weight. Ordinary late-update controller targets are consequently absent in sustained flight. This is an ownership and measurement limitation, not evidence that the separate flight presentation correction is disabled. A controller-target residual cannot be fabricated from these missing fields.

No sustained held gun is identified in those wingsuit controller states. Brief `debouncing` held-state labels occur at transitions. They do not establish the shared solved wrist/held-object endpoint. Full-arm flight closure and held-object alignment therefore remain open acceptance tests despite substantial flight exposure.

## Swimming, pendular movement, and baseline resume

The mode census contains on-foot, vehicle, wingsuit, UI, parachute, and falling samples. It contains no swimming mode, and the body-state census contains no swim rule state. The breaststroke change has no live acceptance evidence in this take.

No sampled grapple record contains a non-null lock. The take therefore does not test relaxed pendular legs or arm-pump acceleration while locked to the wire. Traversal without the lock does not provide a substitute test.

Manual yaw-and-height recenters occur at rec 215.575 (video 03:34.613), 722.735 (12:01.773), and 899.537 (14:58.575). UI mode spans rec 1893.4-1929.1 and 2004.0-2023.4. No automatic recenter is logged at their end. The prior defect concerns accepting a low automatic height baseline; absence of that operation is not a successful retest.

## Performance

The 2,287 frame-timing windows cover 116,579 frames over 2315.104 reported window-seconds, for 50.356 timed frames/s. This agrees with the separate view-publication calculation of about 50.36 frames/s. The video file's 60 frames/s is a recording format, not the engine's render rate or the headset's delivered presentation cadence.

The mean is higher than the approximately 40 frames/s reported on 7 November. The scene, route, object population, and interaction mix differ, so the difference does not isolate the body changes. No headset present/reprojection histogram is reconstructed here. The old report's every-second-refresh percentage cannot be compared with publication timing.

| Timing slot | Frame-weighted mean self time |
| --- | ---: |
| `hand_bodies` | 0.009 ms/frame |
| `presentation` | 0.041 ms/frame |
| `body_gate` | 0.022 ms/frame |
| `human_hull` | 0.086 ms/frame |
| `physical_hands` | 0.030 ms/frame |
| `bindings` | 0.268 ms/frame |
| `engine_update_render` | 3.059 ms/frame |
| `drain` | 4.337 ms/frame |
| `tail_drain` | 2.544 ms/frame |

The calculation multiplies each window's per-frame self time by its frame count, sums those products, and divides by the total frame count. Slots have nested scopes and different call counts; their means must not be added indiscriminately. The previously reported 0.35 ms/frame `hand_bodies` cost is not reproduced. This is an observational improvement, not controlled attribution to one commit.

The controller-state frame intervals contain 27 intervals at least 100 ms out of 116,582 samples, including 24 at least 250 ms. Eight exceed 500 ms. Their rec times are 95.28, 156.20, 278.39, 522.32, 888.38, 949.21, 1559.33, and 2230.32. The worst is 627.55 ms at rec 95.28 (video 01:34.319), while in a vehicle. The slow intervals span vehicles, on-foot play, and wingsuit flight.

In all eight matching frame-timing windows, the dominant large maximum is nested under `sim_steam`, `sim_tail`, `sim_tick`, and `engine_update`. The `sim_steam` maxima range from about 561 to 607 ms. For example, the worst window reports about 607 ms in `sim_steam`, versus about 30 ms in `update_render_hook`. The aggregation localizes these large stalls to that instrumented scope. It does not identify the specific Steam operation, prove synchronous causation from window maxima alone, or explain every shorter hitch.

## Remaining acceptance work

1. Inspect the floor-pickup and rec 909.71 registration outliers with synchronized interaction ownership and the exact render transform. The small drawn-to-requested-target residual in one large raw-hand outlier argues against treating every outlier as an arm solve failure.
2. Test seated draw and dock repeatedly at the visible waist, including both sides and both seated and vehicle-locked standing posture. General wrist registration is insufficient for this acceptance criterion.
3. Test wingsuit and parachute poses with the full shoulder-elbow-wrist chain visible, including a held gun or object. Record an explicit flight presentation target if endpoint error is to be measured.
4. Exercise a grapple lock while inspecting leg relaxation and arm-pump acceleration. Exercise breaststroke at the surface and underwater. Neither intervention is covered here.
5. Reproduce pause/resume with a temporarily lowered head to test baseline preservation. Manual recenter counts do not answer that question.
6. Retest quiet rifle idle settling, walking rifle pumping, look-down yaw suppression, and drawn elbow discontinuities with compatible palette-based measures. Their previous findings remain open.
7. Investigate the long `sim_steam` stalls separately from average body cost. The higher mean cadence does not remove the observed half-second interruptions.
