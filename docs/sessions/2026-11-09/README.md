# Analysis of the 9 November 2026 session

Take: `sessions/2026-11-09_04-17-04_716/recordings/2026-11-09_04-17-12_573/`. The video lasts 38:34.087. The runtime reports build `2026-11-09 04:12:42`, after the collision-binding correction `b0275997` and the dual-wield counter correction `27ef150f`. Both fixes are exercised here. The comparison baseline is [7 November](../2026-11-07/README.md), with the intervening implementation history supplying the acceptance questions.

Recording zero is `2026-11-09T03:17:12.573Z`. Video seconds equal recording seconds minus 0.961668 s, using `stream.json`. Area reports use video mm:ss, with recording seconds where precise correlation matters. The video is a one-eye desktop mirror, not a stereo or tactile recording.

Files:

- [body.md](body.md): body and hand registration, crouch, stop-turn, seated reach, flight presentation, and performance.
- [guns.md](guns.md): guns, reloading, insertion guides, inventory, refused releases, C4, beacon coverage, and the HUD.
- [world.md](world.md): doors, carried objects, NPC grips, snatch, seated grapple, vehicle controls, and traversal.
- [timeline.txt](timeline.txt): whole-take activity timeline with both clocks.
- [log_annot.txt](log_annot.txt): timestamp-correlated session log with original line numbers.

The review combines whole-take contact sheets, denser video sequences around interventions and failures, detailed extracted frames, the session log, and decoded tracking inputs, haptics, body diagnostics, and timing. It is a sampled visual review, not continuous frame-by-frame observation. Counts identify their event denominator; consecutive telemetry samples are not independent user trials. A native return, haptic event, or absence of warnings is not sufficient evidence of a successful visible interaction.

## Overall result

The take contains more complete interaction sequences than the previous report: guns are fired and reloaded, weapons are caught from NPCs, detached doors are carried, barrels are carried and thrown before being shot, aircraft are flown, a motorcycle is ridden, and sustained wingsuit flights connect areas. The saved `video-pickup-and-boom.mp4` captures one of these barrel sequences around video 23:36-24:04. This supports the assessment that more of the interaction set is usable together. It does not establish that every intervention works or that all remaining problems are minor.

The clearest accepted results are the immediate door-crash and counter corrections, live inventory mutations, refused-release recovery, ordinary gun retention, representative rifle and belt reloads, and bounded stop-turn root behavior. The largest remaining interaction failures are seated grapple firing, completed Hydra reloading, and direct ragdoll lifting. Several implemented features have no relevant acceptance test in this take.

## Intervention outcomes

| Intervention | Result in this take | Evidence and limit |
| --- | --- | --- |
| Helicopter-door collision crash | Corrected in the exercised path | Two utility-helicopter doors open and detach at 01:04 and 32:37. Gameplay continues. The previous native collision-filter crash does not recur. This does not validate every collision lease or destruction path. |
| Dual-wield ammo placement | Supported | Opening views at 00:12-00:44 show a separate counter over each gun through different orientations, empty states, and reloads. |
| Live inventory edits | Explicit edits succeed; final inventory remains partial | Five of five mutations complete before the editor closes: two clears and three gives at 31:44-32:06. Pair restoration then repopulates the explicitly cleared chest after closing. |
| Refused holster resolution | Supported in both observed episodes | Fifteen refusal lines form two episodes. The rifle drops at 20:00.90; the Hydra docks after retry at 27:18.14. Neither remains indefinitely held in an open hand. |
| Closed-hand gun retention | No recurrence in measured drops | Zero of 24 logged gun drops occurs with the releasing hand's preceding squeeze above 0.5. Docks and handovers are not in this denominator. |
| Reload guides and rifle capture | Representative success, incomplete weapon coverage | Ammo-shaped outlines are visible for the U-39 and Hydra. Three U-39 seats lead to firing or ammo consumption. Outlines are the accepted interim presentation; usability and other ammo families remain incompletely tested. |
| Hydra insertion | Partial capture improvement; completion remains unsuccessful | Eleven takes produce ten distinct catches and no completed seat. Two windows end in death. The uninterrupted 21:59-22:40 window has nine takes and eight catches without a seat; 0/5 remains visible at 22:16. |
| Manual pistol reload | No demonstrated success | Three CS9 pouch takes at 24:52-24:58 produce no catch or seat. This is separate from successful belt reloads. |
| Detached-door carry | Supported for tested models and short carries | Five explicit detaches all visibly become carries. Four reach release without an immediate post-detach tear. The first helicopter panel remains held for about 20.8 s, then its body is freed. The heavier roadster from the previous take is not retested. |
| Loose-prop catch and carry | Supported for the barrel | A snatched barrel catches, transfers between hands, and stays held until release. Later two-barrel carry, throw, and shooting complete a multi-step sequence. |
| Player collision exclusion and transport | Partial | Carry and door-grab paths survive and props move with the user. The review does not measure player contact suppression, retained world contacts, or exclusion restoration after final release. |
| Direct NPC and corpse handling | Still incomplete | Four ragdoll hand acquisitions form three brief episodes. Two episodes conclusively tear while the hands remain closed. No sustained corpse lift or standing-to-ragdoll grip preservation is demonstrated. |
| NPC-gun snatch | Supported within narrower coverage | Five of five fired NPC-gun snatches visibly catch into the left hand. Right-hand receiving, late squeeze, and seated gun snatch are not retested. |
| Selected-body snatch | Exercised, not fully accepted | Body drags and a barrel arrival use attached native wires. Detached-door snatch, precise off-centre torque, and anatomical anchoring remain unverified. |
| Seated grapple replacement | Exercised input-to-fire failure | Sixteen locked-seat left-B cycles never establish a wire. The jet interval includes valid, tetherable aim hits. Tap commitment, held hand endpoint, and release transfer cannot be accepted without a firing line. |
| Vehicle controls | Supported as virtual controls | Thirty-six acquisitions have corresponding releases, with no reported drift-distance loss. Utility-helicopter, fighter-jet, buggy, and motorcycle control paths are exercised. The native-fit light-assault and floatplane assets are not. |
| Deep crouch and shoulder-root work | Partial success | Controlled crouch windows retain small drawn-wrist residuals, and the rig reaches about 80 cm of physical drop. Floor-pickup transitions retain large outliers; full anatomical shoulder correctness is not established. |
| Stop-turn root feedback | Supported in a bounded sample | Sixteen qualifying windows with no rig turn request have a median root-yaw change of 0 degrees and a maximum of 4.57 degrees over 1.2 s. Native stop-turn animations still occur. |
| Seated waist placement | Partial coverage | Aircraft draw/dock cycles and a motorcycle draw work. General seated wrist registration is good, but repeatable capture at the visible waist is not measured. |
| Full-arm flight and held endpoint | Partial or insufficient coverage | Sustained flight works, but most full arms are outside the mirror. No sustained held gun in wingsuit flight tests the shared held endpoint. |
| Pendular legs, arm pumping, and breaststroke | Unexercised | No active grapple lock or swimming mode is recorded. Ordinary grapple traversal does not test locked swing behavior. |
| Physical beacon | Unexercised or not evidenced | Generic grenades and C4 do not establish beacon acquisition, persistence, or release. |
| Baseline preservation and held-gun resume | Incomplete retest | There are three manual recenters and no automatic recenter. The debug panel opens with empty hands. Neither reproduces the prior lowered automatic baseline or a held-gun resume. |

The subject reports retain event times, methods, and narrower caveats behind these assessments. In particular, a catch is not counted as a completed reload, a pre-existing on-foot tether is not counted as seated firing, and different target classes are not combined into a misleading success rate.

## Remaining problems, ranked

1. Seated grapple does not fire despite repeated left-B input. The strongest interval is video 10:10-10:13 in the jet: five cycles, including a longer hold, with valid/tetherable aim hits and no line. Ten more taps at 32:59-33:01 in the utility helicopter also fail to establish a line. Investigate input routing and eligibility before tuning endpoint behavior. No grip modifier belongs in the intended contract. See [seated grapple](world.md#seated-grapple).
2. Hydra reloading advances to frequent capture but still does not complete. The safe follow-up is one uninterrupted pod take, catch, seat, counter refill, and verified shot. The final 41-second window supplies stronger failure evidence than the two death-interrupted attempts. Manual CS9 insertion also needs a focused check. See [reloads](guns.md#hydra-reloads-remain-incomplete).
3. Direct ragdoll grips remain too short for the intended lift. The two confirmed forced-loss episodes end after roughly 0.95 s and 0.5 s. Mounted and constrained objects also tear away, but must remain separate from freely carried barrels. See [NPC grips](world.md#npc-grips-and-articulated-objects).
4. Door lifecycle and initial grip stability remain incomplete. The bus door loses its hold about 25 ms after the latch opens at 35:13.9. The first helicopter panel is freed while held at 01:25. These are distinct from the corrected collision-function crash and from voluntary releases of detached panels. See [doors](world.md#doors-and-carry).
5. Pair restoration changes the inventory after otherwise successful live edits. A chest slot cleared at 31:46 is repopulated immediately after the editor closes at 32:08. Compare the requested final socket set with the state after pair restoration, rather than treating live application as complete inventory correctness. See [inventory](guns.md#live-edits-work-paired-restoration-needs-another-check).
6. Registration outliers remain during interaction transitions. The controlled crouch result is encouraging, but the floor-pickup window has a large tail, and one ragdoll-interaction wrist follows the requested target while being far from the raw controller. Inspect ownership and synchronized transforms before attributing every large raw-hand error to the arm solver. See [body registration](body.md#body-shoulders-and-crouch).
7. Long stalls remain despite higher mean cadence. Eight intervals exceed half a second, with matching timing-window maxima concentrated under `sim_steam`. This localizes the investigation; it does not prove which Steam operation causes the stall. Performance fixes were outside the previous implementation scope. See [performance](body.md#performance).

Other bounded failures include two locker-fixture snags, three body-snatch hook-fire refusals, and a C4 placement refusal that removes the held charge. They warrant follow-up without being generalized to all snatch targets or all throws.

## Comparison limits and acceptance gaps

The new tracking records the actually drawn arm palette. The 7 November body report largely measures earlier HumanIK output. The new 2.90 cm median stationary ordinary-on-foot wrist residual must not be presented as a direct reduction from the old approximately 13 cm figure. Likewise, the new 77 stop-turn state entries out of 382 stop-state entries do not share the previous walk-stop detector's denominator. The bounded root-yaw check is the evidence for the feedback fix.

Timed mean cadence is about 50.36 frames/s, versus approximately 40 in the prior report. The route and scene differ, so this is an observed improvement rather than attribution to the body changes. The 60 frames/s video encoding does not establish headset delivery at that rate. Tracking records 116,583 frames with zero recorder drops; that does not imply zero stalls or reprojection.

The next acceptance pass should deliberately cover detached-door snatch; a heavy roadster door; standing-person grip through toppling; a completed manual pistol and Hydra reload; both seated sockets and a seated reload; the supported native cockpit assets; a short-release long-A bail; a held gun during wingsuit flight; an active grapple lock with leg and arm-pump observation; breaststroke; the physical beacon; and pause/resume with a temporarily lowered head. Quiet rifle settling, walking rifle pumping, and drawn elbow discontinuities also remain open. The existing recording cannot close those cases by inference.

## Terminal exception

The recording stops at `03:55:47.557 UTC` and the capture host finalizes the video normally. At `03:55:59.458 UTC`, about 11.9 s later, `crash.log` records a first-chance access violation in `cudart64_55.dll` with WaveWorks frames. The session log ends shortly before it, with no clean shutdown or uninject record. This resembles the terminal water-library exception in earlier takes. It is not the previous collision-filter entry failure and does not occur during the recorded door-grab tests. The crash report alone does not establish whether the exception was handled or the ultimate exit mechanism.
