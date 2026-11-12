# Analysis of the 9 November 2026 14:18 session

Take: `sessions/2026-11-09_14-18-04_907/recordings/2026-11-09_14-19-29_248/`. The session ran from 13:18:04 to 14:03:53 UTC, and the tracking recording ran from 13:19:29.249 to 14:03:41.940 UTC, 44:12.691, with 120,880 frames and zero recorder drops. There is no video. The take has only `tracking.jc3trk`; `take.rrd` was exported from it without video (683 MB, 120,880 frames, 172,237 events, 62,006 rig rows, `wall_clock=false`).

The build string is `2026-11-09 14:14:02`. It includes the fixes through `b2e989a4`, including the C4 physical snatch `8747f167`, the rider reaction `05723133`, the mounted grip path `9763ea22`, and the held-object removal pause `b2e989a4`. It does not include the camera transition gate `6ac8a4c5` (committed 14:14:39) or the recording unification `665d295e` and `d8ab6819` (committed after the take ended). The take is therefore the pre-gate camera behavior and the old recording UI.

The comparison baseline is the [9 November 04:17 session](../2026-11-09/README.md) and the fixes committed between the two takes. Recording seconds below are relative to the tracking start at 13:19:29.249 UTC.

Files:

- [timeline.txt](timeline.txt): key events with session log line numbers and recording seconds.
- [log_annot.txt](log_annot.txt): the full session log with original line numbers and recording seconds.

The review is log-based. There is no video, so visual registration, camera comfort, and the visible outcome of a catch, throw, or tear are not established. A log event is evidence that the mod requested or observed an action, not that the action looked correct.

## Overall result

The take exercises a broad interaction set: parachute and wingsuit flight, vehicle controls, mounted gun handles, ragdoll grabs and throws, a door tear, C4 throws and detonations, and placed-charge snatches. It is the first take with the C4 physical snatch, the rider reaction, the mounted grip path, and the removal pause in the build.

The C4 physical snatch works at the log level: three placed charges are caught at 911.1, 936.4, and 968.3 s. The door carry failure from 7 November persists: the front-left door is torn at 225.8 s and its hold is lost at 229.2 s, 3.4 s later, with the object torn from the hand. Ragdoll holds remain short: 29 character holds produce 29 throws, and 12 of the hold losses are ragdolls, all reported as torn from the hand. The mounted gun path is not exercised through its intended grip: the hands take the mounted gun's handles six times, but there are no `mounted gun:` runtime logs and no evidence of grip-driven firing or tear-off. The rider reaction is not exercised: every blow logs `rider_reaction_queued=false`.

The camera gate and the recording unification cannot be evaluated from this take. The camera complaint that prompted `6ac8a4c5` was made against this build. The take is tracking-only because the old Tools page tracking-only control was used; the video path was never started. That failure is what `665d295e` and `d8ab6819` address.

## Fixes between the 04:17 and 14:18 sessions

| Fix | Commit | In the build | Evidence in this take |
| --- | --- | --- | --- |
| C4 physical snatch | `8747f167` | Yes | Three placed-charge catches, 911.1, 936.4, and 968.3 s. |
| Rider reaction | `05723133` | Yes | No `rider_reaction_queued=true`; no motorcycle rider punch. |
| Mounted grip firing and tear-off | `9763ea22` | Yes | Six mounted-handle grips, no `mounted gun:` runtime logs, no tear or shot. |
| Held-object removal pause | `b2e989a4` | Yes | No direct evidence; the one detached door is torn from the hand before any removal countdown is visible. |
| Camera transition gate | `6ac8a4c5` | No | The take is the pre-gate behavior. |
| Session recording as the only capture path | `665d295e`, `d8ab6819` | No | The take is tracking-only, recorded through the old Tools page control. |

## Intervention outcomes

| Intervention | Result in this take | Evidence and limit |
| --- | --- | --- |
| C4 physical snatch | Supported at the log level | Three catches, each followed by the pending take. No video, so the line, travel, and catch are not visually confirmed. |
| C4 throw and detonation | Exercised | Fourteen throws and twelve detonations. The detonator and throw paths are active; visual timing is not available. |
| Rider reaction | Not exercised | No blow queues the rider fallback. The take has no motorcycle rider punch. |
| Mounted grip firing | Not exercised | The hands take the handles, not the gun body. The runtime path never logs. |
| Mounted tear-off | Not exercised | No detached mounted gun and no portable pickup. |
| Held-object removal pause | Not demonstrated | The door is torn from the hand 3.1 s after detach, which is a hold failure, not a removal-queue expiry. The pause targets delayed removal, so this take cannot confirm or refute it. |
| Door carry | Still failing | Front-left door detached at 225.8 s, hold lost at 229.2 s. This matches the 7 November roadster pattern. |
| Ragdoll grip | Still short | 29 character holds, 29 throws, 12 ragdoll holds torn. The direct-lift problem remains. |
| Vehicle controls | Exercised | 73 control takes and 73 releases, including flight stick and throttle. No video for the cockpit fits. |
| Camera transition gate | Not in the build | The take records the unconditional pre-gate behavior. |
| Recording path | Failed for video | The take is tracking-only. The unified session recording was not in the build. |

## Remaining problems, ranked

1. The camera transition gate is untested. The take predates `6ac8a4c5`, and the camera complaint came from this build. The next take must run the gate build and deliberately cover vehicle entry and exit, parachute deployment and landing, and wingsuit deployment and landing.
2. The door carry still tears 3.4 s after detach. The removal pause does not address the physical hold loss. The hold path needs the same attention as the removal countdown; this take supplies the reproduction.
3. Ragdoll holds still tear. The take has 29 character holds and 12 ragdoll holds torn. The direct ragdoll lift remains the largest physical-interaction failure.
4. The recording path is still the old one in this build. The take is tracking-only, and the video was never started. The unified path must be tested both with the capture host and without it, where it should degrade to tracking-only and say so.
5. The mounted grip and rider reaction paths have no coverage. Both need deliberate tests: two hands on the gun body, trigger fire, prompt suppression, tear-off, and a punch on an exposed motorcycle rider.
6. The terminal WaveWorks exception remains. The tracking recording stops at 14:03:41.940, and the first-chance access violation in `cudart64_55.dll` is recorded at 14:03:53.675, with WaveWorks frames. This is the same terminal water-library exception as earlier takes and is outside the interaction fixes.

## Comparison limits and next acceptance pass

The take has no video, so it cannot support visual acceptance of camera comfort, arm seams, door carry, C4 catches, mounted firing, or rider reactions. The tracking data and the log can confirm inputs, state changes, catches, and tears, but not how they looked.

The next pass should use the current build and cover, in order: vehicle entry and exit with the camera gate; parachute deployment and landing; wingsuit deployment and landing; a mounted gun gripped by its body, fired, and torn off; a motorcycle rider punch; a door carried for more than 3 s after detach; a placed C4 snatch with the line and catch visible; and a session recording started with F1, both with the capture host running and with it stopped, to confirm the tracking-only degradation and its message.

## Feedback from the 14:18 session

The verbatim list is in [philpax_feedback.md](philpax_feedback.md). This section combines it with the take's log and the code paths. Confidence is high where the code path is direct, medium where the log is consistent but does not isolate the cause, and low where the take has no coverage.

### Ground snatch

Evidence: the take has nine `OBJECT` snatch attempts. A 79.9 kg object at 19.9 m and another at 14.4 m fail with `snatch: the line came away` at 13:22:15 and 13:22:18. A 99.9 kg object at 5.7 m fails with `snatch: the hand missed the catch` at 13:24:50. A 25 kg object at 7.3 m is caught at 13:24:11. Manual pickup works for the same objects.

Theory: the failures split into two paths. `the line came away` is the native wire going invalid after the selected-body attachment. `b9f3f369` made a failed native attachment cancel instead of binding to the owner or centre, so the regression is likely in the selected body hit or its proxy verification. `the hand missed the catch` is the swept catch not meeting a heavy object. The pull path was rewritten by `8747f167`, and the 99.9 kg object is the heaviest caught attempt.

Fix: verify the body hit and owner across the pull, and fall back to the object's own body when the proxy check fails. For heavy objects, widen the catch or lower the line speed so the object reaches the hand. Add a diagnostic for the selected body, proxy, and catch distance. Reproduce with a close, heavy ground object before and after `8747f167`.

### Virtual controls and throttle

Evidence: the take's helicopter log shows the cyclic and collective being grabbed. The collective is 607 mm and 493 mm out of reach at 13:54:07 and 13:54:13, then 92 mm at 13:54:54. The utility asset has no measured collective (`native.rs:119`), so the fallback is placed from the cyclic.

Theory: `visual.rs:92-94` returns no overlay geometry for a native stick or a cockpit fit, so the virtual yoke and throttle disappear exactly when the measured cockpit is used. The full travel rail is already built for levers (`visual.rs:57-68`), so it is hidden with the overlay. The utility collective is a cyclic-relative fallback, and its offset places it outside the cockpit.

Fix: draw the overlay even when a cockpit fit or native stick exists, using the fit's measured handle and the existing rail. Add a measured collective for `utility_body`, or clamp the fallback to the part bounds and bring it closer. The rail then shows the full travel in planes and helicopters.

### Mounted gun and minigun

Evidence: the take grips the mounted gun's handles six times at 13:50:05 to 13:50:14, but the hands hold the handles, not the weapon body. There are no `mounted gun:` runtime logs, no shots, and no tear. The helicopter at 13:54 is a pilot seat with cyclic and collective; no minigun control appears in the log. The user reports the minigun use mode as a hard-to-escape native state.

Theory: `mounted/runtime.rs:92` skips a weapon with a native user, and `:97` matches the held body only against the weapon's own game object. The handles are a vehicle part, so the new runtime never takes control. Entering the native use prompt sets `m_User` and enters the mounted state, which the physical hands and camera do not support.

Fix: match the held body against the weapon's holder or handle part, as the original design intended, and resolve the weapon from that holder. Suppress the use prompt while the gun or its handles are gripped, and prevent the native `StartUsingDeferred` from running. If the native state is already entered, take it over by stopping the native use before driving the gun. Fire along the barrel when the native aim cannot be driven, or feed the aim from the hands; the current runtime already uses the barrel direction.

### Rudder twist

Evidence: the take grips the helicopter cyclic at 13:54:05. `sticks.rs:115` applies `twist_rudder` only when the kind is `FlightStick`; the helicopter cyclic is `ControlKind::Cyclic`.

Theory: the condition excludes the cyclic, so the twist is never read in helicopters. Planes use `FlightStick` and should already work.

Fix: allow `Cyclic` as well as `FlightStick`, with the measured cockpit axis when a fit exists. Add a regression for both kinds.

### Two-handed pickup

Evidence: the log has 15 `single pistols: a pistol is one pistol` lines. The user sees the left holster pistol in the left hand after a two-handed pickup. The take log does not print the left hand's socket draw next to the two-handed take, so the exact event is not isolated.

Theory: `take_by_table` calls `inventory::reconcile::apply` after the take (`hold/pickup.rs:273`), and the projection selects the primary hand's weapon. A two-handed weapon in the right hand may not yet be the selected slot, so the left hand's socket pistol is drawn as the primary. The single-pistol designation may also make it the one pistol.

Fix: after a two-handed take, keep the other hand's socket weapon docked and select the two-handed slot. Exclude the two-handed take from the single-pistol designation. Add a loadout regression for a right-hand two-handed take with a left-hip pistol.

### Ragdoll grabbing

Evidence: the take has 29 character holds and 29 throws, with 12 ragdoll hold losses. The user says live people cannot be grabbed and successful ragdoll holds tear in under a second.

Theory: `characters/acquisition.rs:81` rejects a target whose `ragdoll` flag is set, so a character already in a ragdoll cannot enter the live-character path. A target that becomes a ragdoll after the take is handled by the separate fallen-ragdoll path. That path uses a force-limited soft solve for anatomical gaps (`physics/joint.rs:93`), and the drive's force budget cannot follow a ragdoll's mass, so the tear threshold is reached.

Fix: allow the ragdoll target in the character path, or transition to the fallen-ragdoll path when the target becomes one, instead of rejecting it. Keep a persistent character hold across the transition. Use a stronger ragdoll carry constraint, or a native joint, with the mass accounted for, and preserve the release velocity for a throw. This is the same problem the earlier analysis recorded; the take confirms it.

### C4 trigger

Evidence: the detector requires the palm normal to oppose the button face (`press.rs:227`, `dot <= -min_palm_alignment`). The user says the working approach is from the top with the palm facing away from the player.

Theory: `wrist.rs:280` computes the palm normal from `wrist.orientation * NEG_Y` or the grip's mirrored X. That axis is likely the back of the hand, so the dot is inverted relative to the user's gesture. The button face normal itself may also be flipped.

Fix: negate the palm normal in `wrist.rs` or flip the sign in `press.rs`, then verify both hands with the palm away and the fingertip from above. Add a regression with the current and flipped normals.

### Grapple swing

Evidence: the take has nine grapple locks. The user reports a slow conversion to the fixed rope, easy detachment, and lost exit velocity. `grapple_button.rs:162` sets `TAP_S = 0.25`; the lock phase waits for that hold (`:273-290`) before sending `LockButton::Pressed`. The release path sends `LetGo` without a velocity capture.

Theory: the 0.25 s lock threshold plus the native lock animation is the perceived delay. A tap under the threshold fires the game's hook again (`:292-302`), which can release the line. The native release likely clears the character's velocity, and the mod does not reapply the swing velocity.

Fix: use a separate, shorter lock threshold, or lock on the press. Do not refire on a tap while the line is attached. Capture the character's world velocity before releasing, then restore it for the release. Test a chained swing and a mid-swing release.

### Stunt position

Evidence: the take logs `tracked_stunt=true` and the game holding the player in a stunt position. The user sees a crouched pose and lagging arms. `stunt.rs:14` replaces the waist and torso with the reference pose; `authority.rs:134-135` marks both stunt cases tracked; `mod.rs:493` applies it.

Theory: the native stunt idle is crouched, and `neutral_frame` keeps the native leg transforms relative to the new waist, so the legs stay crouched. The arm targets are solved from the tracked head and hand remaps, but the stunt animation and the procedural arm solve are not coherent, so the arms lag.

Fix: disable the tracked stunt neutralization for now, restoring the native stunt animation, and revisit with a leg-aware neutral frame. If it stays, apply it only to the helicopter hang and keep the native hips and legs for roof and wing standing.

### Wingsuit from grapple and parachute

Evidence: `flight.rs:475-484` allows deployment only for `PreReel` and `ReelingIn`; `PreHang` and `ReeledIn` are false. `wingsuit_gestures/mod.rs:107-113` maps only `Falling` and `Wingsuit`; `Parachute` is `Other`, which reports `the player is neither falling nor in the suit`.

Theory: an attached grapple hang is excluded by the traversal match, and the parachute mode never reaches the deploy stage. The user's two cases are exactly those two exclusions.

Fix: allow `PreHang` and `ReeledIn` to release the line and deploy, preserving the line release and the suit request order. Add a parachute stage that can deploy the suit, cutting or releasing the canopy first if the game requires it. Test both transitions.

## Fixes in response

The following commits address the feedback. They are local to the `controllers` branch and are not in the `2026-11-09 14:14:02` build, so this take cannot evaluate any of them. Unless a row says otherwise, the change is verified by compilation and unit tests of its pure logic, not by a live game run.

| Feedback | Commit | Change |
| --- | --- | --- |
| C4 trigger facing | `b9c93142` | The press detector admits a palm facing the same way as the button and blocks one facing into it, which is the above-the-wrist press the player makes. Tests cover the path for either hand. |
| Virtual controls and throttle | `e5521bb8` | The virtual stick and lever are drawn over measured cockpits and native sticks at the measured handle, with the lever's full travel rail. A lever the mesh does not model is placed from the tuning relative to the pilot's eyes and clamped into the part's bounds, so a helicopter's collective comes inside the airframe. |
| Minigun and mounted gun | `939f0210` | A hand that takes the gun's body, its holder, or a handle on the holder's vehicle drives the gun. A native mounted use is stopped and taken over with the native handles kept, the start-using hook refuses a new native use while the runtime holds the gun, the prompt is re-hidden when the game re-enables it, and the tear works from the handle's own lag. The takeover, firing, and tear have no live coverage. |
| Rudder twist | `e5521bb8` | Both the flight stick and the cyclic work the yaw by their twist. A helicopter's twist writes its own yaw actions instead of the plane's rudder. |
| Two-handed pickup | `9a0456d0` | A two-handed take into the primary hand selects the two-handed slot and keeps the weapon the game still wields from a socket docked until the game stops wielding it there. Only dual-wield weapons are made single pistols. Loadout and live tests cover the docked weapon. |
| Ragdoll grabbing and carry | `d48f2174` | A ragdolling character is admitted by ownership alone and held by the bone the reach found through the hand body's joint, with the rest of the ragdoll hanging from it. The joint uses the scoped ragdoll strength and the last hand throws the ragdoll even when joined. Tests cover admission, regrip, tuning, and throw selection; live carry and throw are unverified. |
| Ground snatch | `33cc2421` | The fire checks the attachment proxy against the selected body and point, follows the object's own body when the proxy holds another of its bodies, and fails when the line holds something else. The anchor is re-verified every tick and the native rope is excluded from the fire. A heavy target's catch zone widens and the line asks less speed. The take's `line came away` and `hand missed the catch` paths now log the selected body, proxy, offset, mass, and catch distance. The regression's cause is not confirmed without a live reproduction. |
| Grapple swing | `e4b4c797` | The press locks the line at once rather than after the tap window, a quick release resumes the reel without firing onto the attached line, and the lock brakes the reel's rush into the swing while letting the held length follow the character in. A release or break captures the wanted velocity before the retract and hands it back after the reel's cancel. Tests cover a chained swing and a mid-swing release. |
| Wingsuit from grapple and parachute | `d21c930d` | The recognizer runs under a canopy and on a yieldable line, retracts a hang's line before requesting the suit, and retries while the pose is held. A canopy the player opened starts unarmed so the opening pose does not reopen the suit. Tests cover both transitions. |
| Stunt position | `ac274945` | The stunt reference body is placed at the character's own height instead of the animation hips, keeping the standing and hanging stunt cases at character scale. The take predates the commit, so its result is unverified. |
| Camera transition gate | `6ac8a4c5` | The animated head frame follows only vehicle and airborne transitions, with a slow filter. The take predates the commit. |
| Recording path | `665d295e`, `d8ab6819` | Session recording is the only recording control and degrades to tracking-only after a 1.5 s host grace. The take predates both commits and used the removed tracking-only control. |

The next take should be built after these commits and should cover the items the log could not: a mounted gun gripped by its body, fired, and torn off; a two-handed take with a pistol on the other hip; a ragdoll carried and thrown; a heavy ground object snatched with the line visible; a chained grapple swing and a mid-swing release; a wingsuit deployment from a hang and from a canopy; and a helicopter collective reached from the pilot seat.

## Adversarial review of the fixes

A second pass traced each commit end-to-end against the feedback rather than against its own tests. The verdicts are harsher than the table above.

| Feedback | Verdict | Main gap |
| --- | --- | --- |
| C4 trigger | Partial | The sign flip is plausibly right, but the tests hand the detector the assumed palm normal and do not measure the over-the-wrist pose. A fingertip that comes over the back of the wrist is still blocked by `max_behind_m`. `docs/mod/input/controllers.md` still says the palm must face into the button. |
| Virtual yoke and throttle | Addressed (hiding), addressed (travel), partial (placement) | The overlay draws over measured cockpits and native sticks, and the lever rail shows the full travel. The fallback collective is only used for a cockpit fit with no measured lever; native-stick and no-fit helicopters keep the old unclamped placement. The clamp is the whole-body mesh AABB, not the cabin interior. |
| Helicopter throttle inside | Partial | Only `utility_body`-style cockpit fits get the new fallback; the take's 607/493 mm misses were the old placement. The new point is inside the AABB but not proven inside the cabin. |
| Stick twist yaw | Addressed in code, partial in docs | The cyclic twists and a helicopter writes `HELI_TURN_*`, but the setting, UI, and `docs/mod/input/cockpit-controls.md` still describe a plane-only rudder. An `Unknown`-class helicopter would still write the plane's rudder. |
| Minigun aim | Not addressed | `StopUsing` removes the heavy slot, so `mounted::read` fails and the hand-derived aim is cleared. The runtime fires along the barrel's last direction; there is no hand aim and no forward lock. This regresses a native use that had a working aim. |
| Minigun external grip | Not addressed | The mounted-handle grip is a virtual `GrabVolume`, not a `HeldBody`; the runtime only reads physical pickup holds. A mounted gun's own body cannot be grabbed because `bodies::classify` rejects non-dropped weapons. Only a held vehicle part near the gun matches. |
| Minigun native trap | Partial | The takeover is gated on the module's own handle holds, and the prompt can still start the native state when the hands are not gripping. The trap remains reachable. |
| Minigun tear and delivery | Partial | The native handle tear measures hand-to-handle distance with `load = 1.0`, so aiming can tear the gun. A detached heavy gun stays `DROPPED`, so the table's `carry_loose` refuses it and the gun falls rather than entering the hand. |
| Two-handed take | Partial / not demonstrated | Every two-handed take in the take log has the reconcile equip the rifle in the same call, so `gun::held_weapon` is the rifle and `displaced_by_take` never marks the hip pistol. The single-pistol guard was already effectively true. The direct `OnUnequip`/`SetSelectedWeaponSlot` bypasses the reconcile's clip abort and armed-target handling. |
| Ragdoll grab | Partial | Only an already-ragdolled target is newly admitted; the live-character path is unchanged. The carry joins only the grabbed bone, the game's get-up can still end the hold, and the hand body is not excluded from colliding with the ragdoll it holds. A throw moves the bone, not the whole ragdoll. |
| Ground snatch | Partial, cause not identified | The only change aimed at `the line came away` moves `exclude_rope` from the bite to the fire; the 04:12 take caught a 149.6 kg object at 19.8 m with the old placement, so the fire-to-bite window is not proven. `check_anchor`'s "selected body no longer owned but proxy holds it" branch is unreachable. Native wire loss still returns before `verified_hit` and logs nothing. The heavy speed scale is only a request (`max_speed_mps` stays 18). |
| Grapple swing responsiveness | Not addressed | The lock still starts only when the reel carries (`GHS_REELING_IN`); the press no longer waits an extra 0.25 s, but the take's 0.5 s pre-reel wind-up remains. The commit message's "on the tick the button goes down" holds only after the reel already carries. |
| Grapple swing attachment | Partial | The brake targets the take's dominant slack (6/10 locks ended 7-43 m slack), but it does not address the outward overstretch breaks. It also edits velocity directly, bypassing the line's `max_accel_g` and `max_tension_n`. |
| Grapple swing exit velocity | Partial / unverified | The release captures the wanted velocity, not the proxy body's actual velocity. The engine docs say the retract-cancel keeps the velocity, so the premise is unbacked. The reapply is on the next input tick, after the cancel's world update. |
| Wingsuit from a grapple hang | Partial | The retract precedes the request, but the first request is emitted on the same tick while still in the hang, and the retry is 0.5 s within 1.5 s. The retry is not pose-gated, so the suit can open after the player lowers their arms, and it can retract a new grapple fired during the window. |
| Wingsuit from a canopy | Not addressed | No canopy cut is emitted. The commit assumes the parachute state takes `OPEN_WINGSUIT` directly, but the engine doc places the act in the falls out of the parachute, and the mod's own checklist says to cut the canopy first. |
| Stunt position | Partial | The reference body at the character's height plausibly fixes the crouch for roof and wing standing. The arm lag is untouched; the hand IK, late correction, and transport are unchanged. The game's leg IK is not neutralized over the reference legs, the helicopter hang is reverted to the animation, and the head frame now differs from the hook's animation frame. |

The batch is not an acceptance pass. The mounted minigun item is the most wrong: after takeover the gun cannot be aimed, and the external grip the user asked for is still unreachable. The grapple responsiveness and canopy transition are also not addressed. The next take should be treated as a diagnostic run, not a confirmation, and the mounted, snatch, and wingsuit paths need the fixes above before the feedback can be called addressed.

## Remediation after the adversarial review

The critical gaps are fixed in the working tree; they are not committed yet.

- **Mounted gun.** The input tick now selects a mounted gun by the hands' proximity to its handles or pivot when the player is not in the native mounted state, so the handles are offered and can be taken without the use prompt. The runtime treats the virtual handle holds as a grip, keeps a native use's hands when it stops that use, and no longer requires the grip button for a takeover. The aim is derived from the hands through the same proximity path after the native use stops, so the gun does not fire along a frozen barrel. The start-using hook refuses the native use for the selected gun, and a detached gun the table refuses is picked up through the game's use-item action. The handle tear measures the hand's distance from the gun's pivot beyond the handle's, so aiming does not tear the gun.
- **Wingsuit.** Under a canopy the deploy now clicks the game's cancel action to cut the canopy; the retry opens the suit once the fall out of it begins. The retry is pose-gated: a lowered pose no longer opens the suit, and the pending intent expires. A retry no longer retracts a line the player fired during the deploy window.
- **Ground snatch.** `check_anchor` no longer has an unreachable branch, the heavy speed scale also caps `max_speed_mps`, and the line-loss paths log the wire state, selected body, proxy body, and ownership. The heavy catch widens the zone and slows the line; the diagnostics will identify the next take's `line came away` cause.
- **Ragdoll.** A ragdoll's bones no longer need the character object's ownership once the target is a ragdoll, and the game's cancel input is held while a hand holds a ragdoll, so the character cannot get up out of the grip. The joint carry and the throw remain as before.
- **Docs.** The C4 palm rule, the cyclic twist and helicopter yaw, the grapple tap and release velocity, the canopy cut, the ragdoll carry and get-up, the snatch heavy catch and fire-time exclusion, the stunt reference body, and the mounted external grip and takeover are updated in `docs/mod/`.

Still open: the ragdoll hand body can still collide with the ragdoll it holds; the throw moves the grabbed bone and the rest follows by constraints; the two-handed take's logged cases still reconcile the rifle in the same call, so the displaced-pistol guard is not exercised; the C4 pose is still not measured against a real hand; and none of the above is live-verified. The grapple's remaining pre-reel wind-up is the game's; the mod's extra tap threshold is gone, and the docs now state that the lock waits for the reel task.
