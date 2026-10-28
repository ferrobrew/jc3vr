# Gameplay review follow-up

The feedback on `recording-20261024-021026.mp4` (2026-10-24) produced a large batch of commits on the `controllers` branch. A read-only appraisal at `b1e36445` compared the code against the original feedback. No item has game or headset acceptance. The appraisal treats the earlier tracking documents as claims, and several of them overstated completion: they counted items as closed where the code fixes a neighbouring cause, follows a different signal, or adds only a diagnostic. This document replaces those tracking documents.

Verdicts use four states. Implemented means a connected production path exists and is enabled by default. Needs playtest means the path is connected but its effect on the recorded symptom is unproven. Partial means part of the request is unaddressed or the change targets a different cause. Not met means the request is unaddressed.

## Ranked corrective work

1. Body fit fallback. When a fit misses its feasibility tolerance (neck or either hand beyond the tolerance in `payload/src/controllers/body/rig/estimate/state.rs`), the runtime withdraws the fit: the drawn body snaps to the animation pose without a blend, the next tick reseeds from a neutral pose with the feet at rest, root following stops for that tick, and the hand-remap shoulders fall back to head yaw. Deep reaches, fast ducks, and a slightly short arm calibration trigger it. This plausibly accounts for much of the roomscale separation, the bend-into-crouch behaviour, and the leg pops. The correction keeps the previous fit across a rejected tick, blends out instead of snapping, and keeps root following on the last accepted stance.
2. Floor-weapon grapple snatch. A dropped weapon is a snatch candidate only when the hand ray hits its collision body exactly (`payload/src/controllers/aim/grapple_weapons.rs`). The aim cone used for held weapons does not apply. A gun lying flat at range subtends a fraction of a degree, so hand jitter makes it effectively untargetable. This path is unchanged since the recording.
3. Support-hand brace churn. The brace joint is rebuilt whenever the palm-in-gun frame changes by more than `brace_changed`'s thresholds (`payload/src/controllers/physical_hands/physics/guns/math.rs`), which ordinary two-handed aim exceeds almost continuously. Each rebuild teleports the palm body, mutes its contacts, and logs at info level twice per physics step.
4. Detonator fingertip. The press point is the runtime's index-tip joint carried rigidly onto the drawn grip (`payload/src/controllers/weapons/c4/press.rs`), not Rico's posed index-tip bone. A difference between the runtime hand model and Rico's hand appears directly as the reported offset. The correction derives the point from the drawn finger.
5. Authored camera coverage. The authored camera follows the `CharacterCam1` marker, not the head, and only the upside-down grapple clip is verified to animate that marker. The wingsuit-to-landing state seen at 01:06 is refused by the classifier in `payload/src/hooks/camera/authored/native.rs`. Handback takes a fixed time regardless of the correction size, so an interrupted inversion returns up to a half-turn of roll in that time. The correction covers the recorded states, considers deriving the correction from the body's own rotation where the marker does not move, and scales handback time with the correction.
6. Fire permission outside on-foot play. A held gun reserves its hand's trigger in every state, but firing still requires the game's animation shoot permission (`payload/src/controllers/weapons/capabilities.rs`). Whether that permission holds while driving and in the wingsuit is unverified. If it does not, the trigger is reserved and the gun does not fire.
7. Wingsuit gestures while holding. Holding anything disables every wingsuit gesture, and `Use` is unbound in the wingsuit, so a player holding a gun cannot brake or close the suit.
8. Hand bodies on moving platforms. Physical hand bodies are driven in world space with a speed cap (`max_speed_mps` in `payload/src/controllers/physical_hands/physics/config.rs`). Above that speed a hand body cannot keep up with the vehicle, and with `render_at_body` on the drawn hand lags up to the break distance, snaps back, and repeats. The lag also feeds the push on the player. The correction drives hand bodies relative to the carrying body.
9. Gun body against render mesh. The gun body may stray from its seat by `stray_distance_m` and `stray_turn_deg` before it is reset, while the drawn gun follows the stray only by `drawn_max_shift_m` and `drawn_max_turn_deg`. The difference is hidden from the render by design. Recoil pitches the body upward while the render shows little of it, which matches the upward-pointing case at 24:56. Only a readout was added.

## Body

Roomscale following (B1) needs playtest. The root chases the fitted stance, so small head motion becomes lean and larger motion becomes a step. The fallback in item 1 interrupts following when it matters most. The drawn pelvis is not tied to the collision root, so a blocked root lets the drawn body walk ahead, and the only correction is the older pushback.

Bending (B2) needs playtest. Kneel and sit inference are removed, and a low neck spreads across hip, knee, and ankle flexion. The coupling prior penalises hip flexion without knee flexion, which biases toward a squat. The only bend test is shallow and accepts a neck residual far above the live tolerance. A head drop beyond the crouch threshold still requests the game's crouch state through the roomscale capsule path.

Leg IK (B3) is partial. The new path has no future-against-present frame mismatch. Swing feet deliberately target a velocity-led position, and a planted foot holds until a large leg twist before stepping, which reads as exaggerated during turns. Fallback reseeds also put the feet back at rest.

One-hand torso yaw (B4) is partial. A punch within reach no longer turns the body. A fully extended punch twists the chest up to its limit, because the hand reach cost outranks every heading prior. The proposed hands-and-head rule and passive realignment are not implemented. A real body turn with the head under the neck limit and relaxed arms is never detected, so the chest keeps the old heading.

The external study of VR body rigs is thin. The sources are VRChat IK documentation, Meta's Movement SDK, the AvatarPoser paper, and one squat study. No shipped game's rig was examined.

The half-width human hull (P1) needs playtest. It covers every human proxy state, restores transactionally, and is on by default. NPCs receive the same reduction as the player, where the feedback asked for a smaller one. Ledge probes and the AI radius keep the full width. The toggle is not persisted.

Mirror slot markers (U5) need playtest. The mirror is off by default. Pouch markers use the render-time mirror plane while the skeleton uses the simulation plane, so they drift apart in motion and can be mistaken for the A2 misplacement.

## Camera and controls

Chord restriction (I1) needs playtest. One arbiter handles every chord on the stick clicks and the right upper button. The recenter join window is short, so a right-stick click that precedes the left by more than the window commits detonate, and holding for the recenter completes the game's hold-to-detonate. A failed panel chord produces no action and no feedback. Left-stick presses wait for the panel chord window in every context, which delays horn and weapon cycle. The arbiter withholds any route whose bound-source lookup fails, so a binding enumeration fault mutes input without a warning.

Wingsuit controls (I2) need playtest. The mapping matches the request. Item 7 applies. Withdrawal closure may trigger while an empty-handed player reaches for a belt or holster item.

Held guns in every state (I4) need playtest. Gun hands reserve their trigger and release in every mode. Item 6 applies. A gun in the dominant hand removes accelerate in cars and collective in helicopters.

The controls hint (U1) needs playtest. Unbound and irrelevant rows are dropped, and the layout is a two-column grid. On Index controllers on foot the content likely needs two or three pages that cycle automatically, with no manual paging, and a context change resets to the first page.

## Grabs

Grapple snatch (G1) is partial. The pre-arm cache keeps the target through a brief aim miss, and the armed phase holds it through the pull, which meets the target-lock request. Item 2 applies to floor weapons. The yank speed threshold rejects a deliberate moderate pull, and the snatch loses arbitration to any target within reach, including the enlarged world-weapon volumes.

Grab volumes (G2) are implemented and need playtest. Take radius is three times larger, hand-to-hand transfer is twice as large, and floor pickup reach is somewhat less than twice as large. Overlapping take volumes in a cluttered locker let the wrong weapon win, and no feedback shows the selected target before the grab completes. The saved settings file previously masked these values. The file was reduced to the body calibration on 2026-10-29.

Floor reach-and-grab (G3) needs playtest. A dropped-weapon scan no longer depends on wrist direction, and table takes skip the animation pickup gate, which is a plausible cause of the 04:18 failure. Both casts can pass a thin gun and hit the ground.

Doors (G4) are close to implemented. Handle points come from the handle table, the opening pull carries through to the hinged body, and an open door can be held anywhere. Door tearing on a hard yank is on by default and was not requested.

Barrel lift (P2) is partial. The two-hand carry uses one shared solve with extra strength for separated grips. The 24:06 failure was never diagnosed, and objects above the assisted mass limit receive no assist.

Locker grenades (A4) need playtest. Static grenade models in the Guardia Massos model collections register as sources, and the recorded location is a Massos base. The collection names are matched against an allowlist whose runtime spelling is unverified, and a mismatch logs only at debug level.

Dual-wield pairs (A5) need playtest. Racks give two independent guns for any qualifying dual-wield weapon. Floor and NPC dual weapons still give one.

## Guns

Magazine extraction (W1) needs playtest. A gun without a spare-magazine model offers no target. A fresh grip on the magazine well suppresses the support targets and starts an extraction claim that blocks fire, so a magwell support hold leaves the player without a brace or fire.

Thrown guns (W4) need playtest. Peak hand velocity over a short window is converted to world space and written after the game's drop. There is no minimum speed, so a gun set down just after a quick motion can launch. Nothing reads the velocity back after the next physics step.

Heavy droop (W5) needs playtest. One weight target drives physics and rendering. The gun at 23:16 has a mod gun body, so it is not in the heavy slot and probably receives the mild rifle tuning. The droop is a rotation about the grip with no arm sag.

Racking (W6) needs playtest. Reload and action targets are open in every mode, and a ready action can be racked. Rifle charging handles do not move visually, so the only signals are haptics and the hint text. Whether a right hand on a vehicle control counts as free is unclear.

Collider lag display (T3) is a diagnostic-only change. The view now draws the newest physics pose, so moving colliders lead their interpolated meshes.

Gun part separation (T4) needs playtest. Parts follow the mod's own hold record, which covers vehicles where the game marks a held gun as holstered.

Collider shape (P3) needs no change. The gun body shares the game's physics shape. The viewer draws compound shapes as boxes and ignores the convex radius, which accounts for part of the coarse appearance.

## Attachments and transforms

Moving platforms (T1) need playtest. The one-step-behind hand blend is gone, and late references interpolate in the character's moving frame. Item 8 applies. The render offset compares a body stepped toward an extrapolated goal against a palm published later, which gives a speed-dependent error that no test covers.

Body-attached targets (A2) are partial. Targets follow the drawn skeleton, so the symptom mostly comes from the body fit. Grab tests use the controller hand rather than the drawn hand. The skeleton plausibility gate removes every body target without feedback when it fails.

Belt magazine (W2) is partial. The pouch side no longer flips and the model survives source gaps, but a one-tick offer loss still recreates the model. The reach position is unchanged. The magazine placement offset defaults to zero, so the shared display origin does not move the target circle. There is still no visual seated, chambered, or needs-rack state.

Parachute handles (A3) need playtest. Free targets come from the game's own handle links, so ungrabbed alignment holds by construction. Without a fresh native sample no target is offered. The held position is unchanged, as agreed.

## Debug UI

The read-only overlay (U2) needs playtest. It cannot scroll or change page while read-only, and the desktop capture key only enters read-only. Status casing (U4) is implemented. The reorganisation (U3) is not met: it kept every setting under a parity rule. The pruning plan is in [debug-ui-prune.md](debug-ui-prune.md).

## Cross-cutting observations

Many paths refuse without feedback: snatch arbitration, missing parachute samples, missing spare-magazine models, the grenade allowlist, the skeleton plausibility gate, and fire permission. In the headset each failure looks like no response. A selected-target highlight and a short refusal cue would make playtests diagnosable.

The configuration saves the whole structure on every change, so a saved file freezes every default at the time of the last save. Retuned defaults do not reach a player with an existing file. Baking settled values into constants, as the pruning plan does, removes this for those values.
