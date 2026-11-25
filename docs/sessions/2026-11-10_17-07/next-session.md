# Next headset session

What to try in the session after 10 November 17:07, in a sensible play order. Each item's full check is in the doc named; the [README](README.md#feedback-and-changes) maps each to its feedback item and commits. None of it has been seen in the headset.

## Before playing

- Unplug the USB Wi-Fi adapter (or keep the Steam Frame on and in range). The log should then show no `online: the Steam callbacks blocked the game thread` lines; with it plugged in, they should fall on Steam's retry seconds.
- Restart `scripts/capture_host.sh`: each take now also gets a `view-scan.md` (flashes and camera-in-body moments) beside `take.rrd`.

## On foot

- Jog and sprint: no head bob ([locomotion.md](../../mod/body/locomotion.md)).
- Jump, fall, land hard, crash, get ragdolled, get up: the view stays upright at standing height over Rico. Judge how a get-up looks with the body below you, and whether the view ends up inside walls or the ground.
- Walk up kerbs and steps up to knee height: the narrow player steps up smoothly; walking into a wall, a car's side, or a tall ledge lifts nothing ([human-hull-restoration.md](../../mod/body/human-hull-restoration.md)).
- Lean over a crate with a long gun on the shoulder: is the dark shape at the view's lower edge that gun?
- Shadows: grapple fast toward a parked car and watch its shadow; flip `stereo.shadow_anchor_to_fit` off and on for an A/B, and compare sharpness with `stereo.shadow_fit_unpadded` ([lighting-shadow-vr-interactions.md](../../mod/stereo/lighting-shadow-vr-interactions.md)).

## Hands and the world

- Grab a soldier: they hang limp from the hand for several seconds. Drag one on the grapple: it follows promptly ([character-grabs.md](../../mod/input/character-grabs.md)).
- Judge the two-thirds hand strength.
- Carry a grenade while walking: it stays in the fist. Snatch a grenade a dead soldier dropped.
- Open a car door by the handle and pull it off: the door comes to the hand ([physical-hands.md](../../mod/input/physical-hands.md)).
- Carry a fence or other long object and walk: if it blocks you, the log names whether it was kept off your collision.

## Guns

- Draw and holster both pistols repeatedly, including slow openings of the hand: no drops, no missed hips ([physical-weapons.md](../../mod/input/physical-weapons.md)).
- Drop a gun onto a full holster (hip, shoulder, back): it swaps, the old one falls, and the hint reads old to new. Toss one over the shoulder fast. Stow two rifles to one shoulder at once.
- Grab C4 from the chest repeatedly. Trigger while holding a charge: the detonator replaces it; trigger again to detonate. With no charges left, the detonator shows on the pouch. Snatch a placed charge back ([grenades.md](../../mod/input/grenades.md), [pouch-models.md](../../mod/input/pouch-models.md)).
- Pull a grenade's pin: trail, fuse sound, and quickening haptic tick. If one goes off early, the log says whether it was in the hand or on contact ([grenades.md](../../mod/input/grenades.md)).
- Launcher: a short pull fires all five rockets, and a reload shows the fresh pod at once ([reload.md](../../mod/input/reload.md)).
- Revolver: roll the wrist to swing the cylinder out, tip the muzzle up to dump, touch the speedloader from the belt to the cylinder, flick to close. Judge the swing side and flick directions. The belt push still works with the cylinder closed.
- Knock a magazine out with a fast push of a fresh one (1 m/s).

## Grapple and flight

- Swing: enter at speed and keep the momentum; release and keep the swing's speed ([grapple-lock.md](../../mod/input/grapple-lock.md)). If short lines still stop dead, raise `lock.max_accel_g`.
- Tap and hold right as the hook lands: the lock takes it.
- Tether a barrel and swing it without pulling it in.
- After boosted reels, no re-acceleration on landing.
- Reel to a ceiling, and onto a helicopter: Rico hangs upright on the short line, left arm steering. Judge the 3.5 m hang length. On the gamepad scheme the game's own ending plays. A helicopter seat's stunt press leaves the seat.
- The grapple device stays on the forearm through fast arm motion ([presentation-layer.md](../../mod/rendering/presentation-layer.md)).
- Open the canopy out of a reel: no roll, straight to hanging ([canopy-opening.md](../../mod/body/canopy-opening.md)). Fire a line to the side under the canopy: the canopy turns toward it. Judge the canopy pull's speed (left as the game's).
- Cut the canopy with the arms spread, and spread the arms during a grapple pull: the wingsuit opens, with no superspeed ([wingsuit-gestures.md](../../mod/input/wingsuit-gestures.md)).

## Vehicles

- Enter a few vehicles: the view eases about 20 degrees over a second at the seat lock instead of snapping. Judge whether that is acceptable. No blackout near cockpit glass.
- The Urga U-17 Akrobat, entered from both doors: the yoke sits on the modelled yokes; turn to roll, push and pull to pitch ([cockpit-controls.md](../../mod/input/cockpit-controls.md)).
- Throttle: ticks every 10 percent, a hard stop at zero, reverse only past the gate in the back third. Stick and yoke tick at centre and bump at the limits.
- A jeep's turret: yank the gunner out as a ragdoll; stand at the handles to enter the seat; aim with the handles from the seat and from outside; tear the gun off from outside and from the seat. Then try entering the gutted seat, and see whether an NPC tries to man it ([vehicle-weapons.md](../../mod/input/vehicle-weapons.md)).

## Swimming

- Stroke diagonally: the swimmer turns toward the stroke ([swim-strokes.md](../../mod/body/swim-strokes.md)).
