# Guns, reloading, inventory, holds, C4, beacon, and HUD: 9 November take

## Evidence and scope

The take is `sessions/2026-11-09_04-17-04_716/recordings/2026-11-09_04-17-12_573/`. The runtime reports build `2026-11-09 04:12:42`, which includes the collision crash fix `b0275997` and the hidden-mesh ammo-counter fix `27ef150f`. The video lasts 38:34.087 at 1920x1080. Times below are video mm:ss. Recording time is video time plus 0.961668 s.

Evidence consists of whole-take contact sheets at 30-second intervals, denser sheets around the opening guns, rifle reloads, launcher reloads, C4, holster refusals, and inventory edits, and full-resolution reload frames. The session log supplies transition counts and failure reasons. A rebuilt `tracking-synth dump` reads the complete recording and supplies per-hand input and named haptic events. The older binary cannot read this recording correctly. Tracking extraction covers the end of the take as well as the main interval.

Haptic events establish that the mod reports a catch or seat, not that the user feels it or that the resulting presentation is correct. A well catch is not a completed reload. Explosions during combat are not sufficient evidence that the held launcher fired. Counts distinguish these cases. The desktop mirror shows one eye; stereo comfort, sound, and tactile quality remain outside the evidence.

The comparison baseline is [the 7 November report](../2026-11-07/guns.md). The intervening changes cover held-ammo insertion guides, release recovery after a refused holster, live inventory edits, seated sockets, beacon handling, and the opening dual-gun counter retest. This is an outcome review, not an implementation or reverse-engineering audit.

## Outcome summary

| Area | Outcome | Evidence and denominator |
|---|---|---|
| Separate ammo counters | Success in inspected views | The opening CS9 pair has one counter over each gun at 00:12-00:44, including different gun orientations and different ammo values. The previous stacked-counter symptom is absent in these views. |
| Dual-gun belt reload | Success in representative video | Seven belt-seat events occur at 00:18-00:53. The opening video shows depleted red counters becoming loaded counters while the guns remain held. There are 15 belt-seat events across the take. |
| U-39 physical reload | Success, with one unused take | Three magazine-seat events follow four pouch takes at 19:09-19:36. All three seats are followed by visible firing or ammo consumption without a separate cocking action. |
| Hydra physical reload | Partial capture; completed reload fails verification | Eleven pouch takes produce ten distinct well-catch events and zero magazine-seat events across three reload windows. The long final window still shows 0/5 at 22:16. |
| CS9 physical reload | Not demonstrated successful | Three pouch takes at 24:52, 24:54, and 24:58 produce no well catch or magazine seat. A belt reload succeeds earlier at 24:38. |
| Insertion guide | Partial success | The U-39 and Hydra show ammo-shaped outlines and rings, matching the accepted rendering scope. Guide usability and coverage of every ammo family remain only partially tested. |
| Live inventory editing | Success for the exercised mutations | Five of five explicit edits complete while the panel remains open: two clears and three gives. No close-time batch of duplicate give requests appears. A separate paired-gun restoration anomaly follows closing. |
| Closed-hand gun retention | Success on the measured release check | All 24 logged gun drops occur with the releasing hand's nearest preceding squeeze sample at or below 0.5. None occurs with squeeze at or above 0.9. Docking and handover are excluded from this denominator. |
| Refused holster resolution | Success for recovery, not uninterrupted docking | Two refusal episodes resolve: one drops the gun, and one docks after retry. Neither remains indefinitely in an open hand. |
| C4 recovery and detonation | Success for exercised paths | Seven of seven logged placed-charge recoveries use snatch. Two pouch returns complete. Six of seven logged wrist-detonator outcomes are detonation commands; the remaining press reports no placed charges. Representative video shows a charge, release, and explosions. |
| Direct in-reach C4 recovery | Unexercised or not evidenced | All seven recoveries are explicitly snatch paths. |
| Physical rebel-drop beacon | Unexercised or not evidenced | No beacon-specific lifecycle appears in the log or inspected video. A grenade or C4 charge is not counted as a beacon. |
| Held-gun stand-down/resume | Unexercised in the inspected panel stretch | The hands are empty when the debug editor opens near 31:32. Live editing does not test retaining a gun through resume. |

## Guns, reloads, and guidance

### Counters and belt reloads

The opening pair is CS9, not the Pozhar pair in the 7 November take. The log identifies the weapon, and the video shows the compact gun models. At 00:12-00:44, the two ammo counters remain separated above their corresponding guns. At 00:33, both counters are red and empty; at 00:36, one reads loaded while the other remains empty; by 00:39, both are loaded. The counters also remain distinct when the guns are raised vertically and brought closer together. This is direct visual support for the hidden-mesh counter fix in the tested configuration.

The opening belt-seat events occur at 00:18.26 right, 00:18.71 left, 00:28.22 left, 00:35.78 left, 00:39.84 right, 00:52.45 right, and 00:52.65 left. The complete take contains 15 belt-seat events and three magazine-seat events. The latter are all U-39 reloads. The event counts are not a complete attempt denominator: unsuccessful motions that never reach a catch or seat do not necessarily emit an event.

There are nine dry-fire haptic events on nine distinct per-hand trigger presses, spread over eight times because both hands click together once. No repeated dry-fire event occurs within those observed presses. This preserves the earlier one-click-per-pull result in the narrower set of dry pulls here. Only one explicit trigger diagnostic says that the magazine is out, at 21:13.85. The absence of another diagnostic does not prove that another dry pull is appropriate.

### U-39 reloads

At 19:09.58, the right hand takes ammo for the rifle in the left hand. A catch at 19:10.65 leads to a magazine seat at 19:10.99. The following frames show a loaded counter and firing. The rifle then transfers to the right hand. The next left-hand pouch take at 19:19.08 ends without a seat. A new take at 19:25.11 catches at 19:26.06 and seats at 19:26.68. The final take at 19:34.39 catches, briefly catches again, and seats at 19:36.30.

The two knock events are at 19:21.19 and 19:35.60. The first precedes a fresh pouch take; the second occurs during the final insertion sequence. They are two logged knocks, not four knocks inferred from feedback sent to both hands. The inspected frames support magazine replacement and subsequent firing, but not a detailed claim about each discarded magazine's trajectory.

The frame at 19:22 shows a cyan magazine-shaped outline between the held magazine and the well, together with gold rings. The guide is visible and describes more than a ring. The outline matches the accepted scope; a custom renderer for filled ghosts is not required. Its presence does not establish that it is easy to follow. The unused take and the need for a second take remain part of the observed interaction.

### Hydra reloads remain incomplete

The Hydra is drawn at 20:59, 21:31, and 21:53. The relevant reload windows are:

| Window | Pouch takes | Distinct catches | Completed magazine seats | Outcome |
|---|---:|---:|---:|---|
| 21:15-21:21 | 1 | 1 | 0 | The pod catches near 21:19, then a hit reaction and death interrupt the attempt. |
| 21:35-21:39 | 1 | 1 | 0 | The pod catches near 21:37, then another death interrupts the attempt. |
| 21:59-22:40 | 9 | 8 | 0 | Repeated pod handling and catches continue until the launcher is docked. No completed reload is evidenced. |
| Total | 11 | 10 | 0 | Capture is frequent; a usable seated pod is not demonstrated. |

The first two interrupted attempts should not be called ordinary insertion failures. The last window supplies the stronger remaining failure: nine takes and eight catches over about 41 seconds without a seat. In full-resolution video at 22:16, the counter still reads 0/5. Frames around 22:00, 22:14, 22:20, and 22:34 show the cyan pod outline and gold ring. Nearby rockets and explosions occur during enemy combat and do not establish successful player firing.

Compared with 7 November, the interaction progresses beyond repeated approaches without a catch. It still does not establish completed reloading. The earlier report records one seat in three broad Hydra attempts; this take records no completed seat in three windows, two of which end in death. Those window counts are not controlled success rates. The actionable follow-up is a safe-area check that follows one pod from take through catch, seating, counter refill, and a verified player shot. The evidence does not identify a native cause.

### Weapon coverage

The logs identify held CS9s, U-39s, Wraith 2000s, and the Hydra. The Wraith pair is taken from the rack at 25:33-25:34 and remains visibly separate in the 25:30-25:50 views. An Automat 12 is given to the right shoulder in the editor at 32:06, but no subsequent draw or shotgun reload is evidenced. Giving a weapon does not exercise its handling.

The Fire Leach and UPU-210 successes from 7 November are not repeated. The Negotiator, revolver cylinder, single-shell shotgun loading, Splitter, and bolt or pump actions remain outside this take's demonstrated reload coverage. The only manual magazine seats are the three U-39 seats. The three unsuccessful CS9 pouch takes at 24:52-24:58 warrant a focused manual pistol reload retest rather than a claim that all magazine weapons work.

## Inventory and holds

### Live edits work; paired restoration needs another check

The editor remains open from approximately 31:32 to 32:08. The back-outer socket clears at 31:44.01, and the chest clears at 31:46.44. CS9 gives complete on the left hip at 31:57.21 and the right hip at 32:00.28. The Automat 12 give completes on the right shoulder at 32:06.34. The corresponding spawn requests precede completion by only a small fraction of a second. All five mutations precede the panel-close event at 32:08.21. The contact sheet also shows a weapon on the ground while the editor is still open.

This resolves the specific 7 November failure in which edits wait until closing and duplicate requests spill unwanted weapons into the world. Each of the three explicit gives here has one request and one completion. The two clears each complete once.

The broader inventory result is partial. Immediately after closing, pair restoration creates another CS9 on the chest at 32:08.43, although the chest was explicitly cleared. At 32:10.35, a further second-half restoration is drawn into the left hand from the right-hip route. The resulting video shows two guns, so this is not a claim that a rendered gun is missing. The log does establish that the post-close inventory contains a newly restored chest weapon. The next check should compare the requested socket set with the persistent final socket set after paired-weapon restoration. Confidence is high in the recorded transitions and moderate in the user-visible consequence because the chest is not held in view throughout the transition.

### Closed-hand holds and refused releases

All 24 logged gun drops align with an open releasing hand in tracking. The highest sampled squeeze is about 0.492. This preserves the 7 November improvement from premature closed-hand release. There are also zero `held gun turned` warnings, as on 7 November. Neither measure establishes that every held pose is visually stable during every locomotion state.

The 15 animation-refusal lines form two episodes, not 15 failed user attempts:

- At 20:00.53-20:00.87, the U-39 encounters 11 refusals. It drops at 20:00.90. The video shows the rifle leaving the lower-right view while the other hand continues grappling. The releasing hand's squeeze is zero. The gun no longer remains held in an open hand, although docking does not succeed.
- At 27:17.78-27:17.89, the Hydra encounters four refusals while approaching the left shoulder. A retry proceeds, and docking completes at 27:18.14. The inspected sequence changes from a visible launcher to an empty hand without a manual re-grip.

Both episodes resolve, unlike the stranded open-hand weapon reported on 7 November. The animation restriction remains observable; the recovery is the improvement. No same-tick dock/redraw is identified in these inspected episodes, but there is no exhaustive grip-reopen latch test.

### Seated guns and other holds

Left-hip CS9 draws recur in an aircraft at 10:05, 10:19, and 10:25, followed by docks at 10:17, 10:24, and 10:47. The 10:00 contact-sheet view shows a held gun inside the cockpit. Later, a left-hip draw at 37:05 occurs during the motorcycle stretch and ends in a drop at 37:21. These are broader seated-use examples than the previous car-only take. They do not provide a controlled reachability measurement for the relocated socket or demonstrate a seated reload.

At 01:04-01:25, the user holds the helicopter door and then proceeds into the aircraft. The session continues for more than half an hour. This is a live exercise beyond the previous door-grab crash, not an independent proof of every collision-pair path. Carry stability, person handling, and seated grappling belong to the other session subjects.

## C4, grenades, and beacon

Placed C4 recovery remains a snatch interaction. The seven recoveries are at 06:15.22, 06:18.69, 06:24.33, 06:28.38, 06:51.49, 06:55.83, and 16:16.15. Each has an explicit snatch delivery record. Representative video supports charge handling, but not every individual arrival is resolved visually. None establishes a direct hand pickup. Pouch returns complete at 06:34.34 and 07:12.04. The video around 06:52-06:58 shows a charge in the hand and the return gesture with the pouch ring.

The wrist detonator reports six one-charge detonations, at 07:33.48, 07:47.96, 12:40.37, 16:20.86, 23:05.27, and 23:29.62. One further press at 12:43.12 reports no charges placed. The video shows the charge handling and explosions during the 07:30-07:50 sequence. These establish successful representative detonation use, not six visually audited attachment targets. The double-press-during-detonation guard is not explicitly exercised in the log, unlike 7 November. Multiple feedback pulses must not be counted as multiple detonations.

One C4 release at 27:00.63 reports that planting is not allowed. The charge then leaves the loadout as gone. This is a remaining failed placement, not a completed throw inferred from an open hand. The evidence establishes the refusal and loss of the held charge; it does not establish why the game rejects planting or where a charge subsequently appears.

A grenade is returned to its pouch at 06:44.62. Other grenade adoption lines occur later, but no inspected sequence establishes the complete rebel-drop beacon lifecycle. The intended physical beacon hold, release, and persistence therefore remain unverified. No beacon-specific success is inferred from generic grenade events.

## Remaining checks and comparison

| Subject | 7 November | 9 November | Remaining action and confidence |
|---|---|---|---|
| Hydra | One seat across three broad attempts; long no-catch stretches | Ten catches across eleven takes, but no seat; two windows interrupted by death | Verify catch-to-seat completion in a safe area. High confidence that this take does not demonstrate a completed reload. |
| Inventory edits | Deferred and duplicated on close | Five of five explicit mutations apply while open | Live editing succeeds. Check the extra post-close paired restoration against the requested sockets. High confidence in the transitions. |
| Refused holster | Two episodes leave a weapon held until further input | Two episodes resolve to one drop and one dock | Recovery succeeds. Retest intended docking under the same animation restrictions. High confidence in the recovery, not universal docking reliability. |
| Closed-hand gun drops | Zero of 22 measured releases | Zero of 24 measured drops | No recurrence on this metric. The denominators exclude docks and handovers. |
| Grip-rotation warning | Zero warnings | Zero warnings | No logged recurrence; this does not replace visual pose review. |
| C4 retrieval | Five snatch recoveries; no direct pickup evidence | Seven snatch recoveries; no direct pickup evidence | Direct in-reach pickup remains unverified. |
| Ammo guidance | Rings visible | Magazine and pod outlines plus rings visible | The outlines match the accepted scope. Guide usability and coverage of other ammo families remain partially tested. |
| Dual-gun ammo HUD | A counter over a gun was visible; two-counter placement was not established | Both opening counters visibly follow separate guns | The immediate counter regression is not reproduced in the inspected views. |
| Seated use | Car draws and firing; no reload | Aircraft draws/docks and a motorcycle draw; no controlled seated reload | Test natural socket reach and a completed reload while seated. |

The main demonstrated gains are live inventory mutations, separated dual-gun counters, continued open-hand release correctness, and recovery from animation-refused holsters. The main unresolved weapon interaction is Hydra seating, followed by manual CS9 reload coverage and post-close paired inventory restoration. Haptic feel, reload sound, a held-gun pause/resume test, golden-gun giving, direct C4 pickup, the beacon, and untested weapon families remain coverage gaps rather than successes.
