# After the 14 November 02:05 session

The fixes for this session's feedback are listed in the [README](README.md#the-feedback-item-by-item). This is what the next session should check, and the plans for what was deferred.

## Checks in the headset

In the order of the feedback:

1. Walk and run forward and backward while looking down: the chest stays square over the hips rather than tipping up into the view.
2. Stand looking down at the feet, then turn on the spot and land from a fast fall on a slope: the feet stay planted in the idle, and the landing's feet are the clip's.
3. Grapple into the wingsuit from a reel and from a roof, and open the suit out of the canopy: the head angle holds throughout. Crash out of the suit: the view does not straighten in one step.
4. Get knocked down, die, and land hard: the view follows Rico's head, upright, and the body stands up in front of the eyes rather than below them, the view staying on the head until it is over the root. Get thrown off a moving car: the view keeps up with the body. Crash the wingsuit onto the ground: the view does not lift through the body. Jump off a roof: the view does not dip on landing. Hang from a ledge and sit in a car: the view is not pushed up.
5. Open the canopy out of a long reel: no roll, the log says `canopy: skipped the opening out of a reel`.
6. Crash the wingsuit along a road: the camera stays above the ground.
7. Stand on a car's roof while it drives and turns: the arms and the suit stay out of the face.
8. Enter and leave a car, the scooter, and a helicopter with the head held still: the view steps toward the vehicle's forward every half second instead of swinging with the clip; drive off turning straight away: the view turns with the car. Enter a helicopter from a jump (3788 read as on foot): note whether the view steps or swings.
9. Fast travel and die: the loading panel follows the face, and no grab circle shows on it.
10. Swing on the grapple lock in every direction, onto a level hook, a high hook, and diagonally: no jump in speed or direction at the lock; the line goes taut and swings and does not break. A fast rush at a level hook slows along the line and swings on a longer line. Let go mid-swing: Rico flies on with the swing's speed; let go into a wall or open the wingsuit at once: nothing pushes him on. Touch down while holding: the line holds and drags. Press jump: it lets go.
11. Hang under a ceiling: Rico lowers to the hang's length, upright, and swings; nothing freezes. Hang under a helicopter: the line takes in to 3.5 m and the swing settles.
12. Mash the grapple at a new target during a reel: it fires again. Grapple from the wingsuit after a gesture deploy: it fires.
13. Snatch a ragdoll: it comes to arm's reach, slowing near the hand, and holds there. Fire the pistol with the other hand while the line is out.
14. Under the canopy, push both handles up and forward: it dives. Pull both down: it slows. The reach ring shows around each handle while a hand is in it.
15. In the wingsuit, look at one hand and then the other with both arms out: neither arm bends.
16. With a charge in hand and charges placed, pull the trigger: they go off and the charge stays. With none placed, pull it: nothing goes off, and the hand buzzes empty. Throw a charge while falling: it is thrown. Throw one forward off a moving car: it lands ahead. Seated in a moving car, throw a grenade forward: it lands ahead too (the character's measured velocity is unverified while seated).
17. Seated in a car and a boat: take a grenade and a charge from the pouches; empty a pistol and double tap its fire press to drop the magazine, then reload from the belt.
18. Reload by the clip release and the belt with a split pistol and an SMG after a respawn: the magazine drops (and is no longer drawn in the gun), or the log says why not and the hand buzzes.
19. Knock a magazine out with a brisk strike that does not quite snap: it knocks at 0.75 m/s.
20. Press a grenade onto a full pouch: a long buzz.
21. Close the grapple hand on the line and swing past the hips: no holstered gun is drawn into it. Squeeze the free hand, then reach slowly for a holster: it draws.
22. Hold a soldier with one hand and with two, and a gas cylinder in a rack: they hang rather than thrash; walk with a held soldier and note whether the hold tears sooner. Grab an allied soldier holding her gun: she falls (the log says `retried the ragdoll` if the topple alone did not). Swing a held gun and a held panel into a soldier: they react, and the log says whether the native reaction acted too.
23. Yank a gunner out of a jeep's turret while riding it: the hands catch him on the deck. Yank a tripod's gunner off the gun.
24. Die holding a pistol in the left hand with one pistol: it holsters on the left hip, and the respawn fills the right hip with its twin.
25. Seated in a helicopter, lean far to look at a thigh and put an SMG away: it stays on the thigh. Enter a seat standing and sit down after it locks: the holsters are at the seated hips, not the standing ones.
26. Grip the tripod minigun's handles: the gun turns with the hands. Walk up to it: no use prompt shows.
27. Grab a closed car door at its handle and on its outer face toward the back, and a fighter's canopy glass: each opens with an outward pull. Brace on a car's hood and hang on to a riding car's side near the front door's hinge: the hand holds the car and no door opens.
28. Ride the scooter on the triggers, one hand and two: no weave from a still hand, the bars buzz at the lock and tick through centre, and fingers lifted off a moment keep the bars.
29. Climb a cliff that leans back: the body rises along the face, and the head need not lean forward.
30. Light-pull the trigger on the map and menus to pin the cursor, then pull fully to click.
31. Stand still and glance left and right without turning the body: the legs stay put and do not shuffle.
32. Walk up to a standalone mounted gun: no use prompt.
33. Drive a car at speed past buildings with the shadow cascades refreshed every frame and not (`stereo.shadow_update_every_frame`): note whether the shadows stay under their objects (441.91).
34. After a fast travel, note whether a dot cursor stays drawn in flight (774.24).
35. Fly with a rifle and a pistol holstered: note whether either pops in and out of view (388.682).

## Plans for what was deferred

### In-process capture of the eye views

The reviewer wants the eye views captured from the game rather than the desktop mirror, at a size that still allows today's 1920×1080 single-eye crop, both eyes side by side if bandwidth allows, with the processing on the GPU and the whole thing working on Windows for testers. The numbers and the options are in [feedback/vehicles.md](feedback/vehicles.md), section 8.

- Stage 1, the frames: restore the GPU pack and the staging ring the in-process recorder had before 3be0139e (`payload/src/recorder/convert.rs`, `convert/ring.rs`, `i420_pack_ps.hlsl`), and change the source from one eye to both, drawn side by side into one target scaled so each eye is 1920 wide (about 3840×2130 for the reviewer's headsets), with a setting for a smaller size. The pack and the readback stay as they were: a pixel-shader pass to planar 4:2:0, a three-deep staging ring, and a no-wait map.
- Stage 2, the transport: restore the writer thread from `recorder/stream.rs`, which streamed the frames as y4m over loopback to a host encoder, and extend the control protocol (version 2) so the host is told the frame size and rate.
- Stage 3, the encoder: the injector starts an ffmpeg it carries (`ffmpeg-sidecar`, with a fetch script for an LGPL build), choosing a hardware encoder where one is listed and x264 otherwise, and records `video.mkv` beside the take as today. On Linux the existing capture host can take the same stream to a native ffmpeg with NVENC instead. The gpu-screen-recorder relay goes once this works.
- Stage 4, audio: WASAPI loopback of the default output from the injector on Windows; under Wine, loopback works from Wine 9.12, unverified with Proton.
- Risks: 330 to 370 MB/s of readback at 30 fps through a loopback socket; the fallback is the full-width left eye with the right at half size.

### The IK-first on-foot body

The reviewer wants the solved IK, driven by the learned estimate, to own the body standing still, a share of it while walking, and the airborne phase, with authored poses only where the IK is ill-posed. The design, the per-state blend table, the feet's contact model, and what killed the procedural legs of 2 November are in [feedback/body.md](feedback/body.md), "The IK-first on-foot body: a design". It also replaces the lean, the drift, and the settle that slide the body at 132 to 140 s, plants the feet on a climbing face, and keeps the belt with the player's real pelvis while moving.

### Held ragdolls and articulated objects

A held ragdoll is damped within itself now, and both are pulled softly. Still to do from [feedback/hands.md](feedback/hands.md), "Fix proposals": a ball-and-socket joint for a second hand on a ragdoll and for any hand on an articulated object (bind `hkpBallAndSocketConstraintData` in pyxis, as `hkpFixedConstraintData` is), damping an articulated object against its assembly's velocity, keeping a held ragdoll out of the camera (with the body capsules below), and holding off the get-up while a hand holds the ragdoll. Check the hold's tear lag against the softer gains.

### The belt while moving (2044)

The sprint clip swings Rico's pelvis about 10 degrees each way every stride and bobs the hips 10 cm, and the belt's volumes ride the hip joints, so the pouch moves under a reaching hand ([feedback/body.md](feedback/body.md), 2044). Low-pass the belt frame over a stride (`controllers/weapons/volumes.rs`, `BodyFrame`), keep the estimate's pelvis in charge of its yaw while the stick walks, and consider a tighter moving deadzone. The IK-first body's belt replaces this once it lands.

### The body's yaw out of falls and get-ups (1806, 490)

Through a fall the body does not follow the torso, so the fast fall's clip leaves Rico facing up to 100 degrees from the view, and the landing swings the root through it; a get-up stands him up facing wherever he fell ([feedback/camera.md](feedback/camera.md), "Yaw on the get-up"). The follow's target has no deadband on landing, so the body should already turn to the torso; read the rig's body yaw and the follow's target across the 1806 landing and the 490 get-up in the recording to see what holds it. Keeping the body's heading on the view's through falls, or the IK-first body's airborne IK, would remove the swing at its source.

### Smaller items found in the review

- Frame time (new problem 4): an A/B with the estimator switched off.
- 1384, the scripted drop into freefall drawn as a ledge fall: the case the body reads for it.
- 2806, the canopy out of the wingsuit: whether the opening out of the suit belongs on the reel-opening skip list.
- The parachute handle's height (1153.93): the grip bar's offset on the riser, to move the handle to it.
- The canopy skip's order: whether the body machine's event handling runs before the local controller's update in the same tick.
- The character's measured velocity while seated, for seated throws.

### The firm ragdoll

One mode for grapple swings, hangs, and the canopy: the physics drives the body, muscle springs pull it toward the IK pose, the grapple arm keyframed to the line, firm enough to reach the holsters. The game ships `GRAPPLE_HANG` and `PARACHUTING` ragdoll presets with the upper body keyframed and the legs motor-driven. The design and its reverse-engineering blockers are in [feedback/grapple.md](feedback/grapple.md), "Firm ragdoll for swings, hangs, and the canopy".

### The helicopter's rail

Grappling a helicopter should stand Rico on its rail, with the full IK, doors that open, Y entering, and a step off falling with no jump. The proposal and the reverse-engineering it needs (how the reel's arrival chooses the door side, the helicopter's interaction-graph entries, whether an on-foot entry works from the skid) are in [feedback/grapple.md](feedback/grapple.md), "Helicopters".

### Held props against the body

A held prop should collide with Rico's drawn body but not the movement capsule. Rico has no physics bodies for his limbs while animated, so the mod needs keyframed capsules of its own on the drawn bones, on a layer only held props collide with ([feedback/hands.md](feedback/hands.md)). The same capsules would keep a held ragdoll out of the camera.

### Smaller items that need reverse-engineering

- The primed grenade's trail and sound: bind `Explosive::SetupEffects` and the grenade weapon's player trail effect and sound fields, and call the setup at the pin pull ([feedback/hands.md](feedback/hands.md), "A primed grenade in the hand has no trail or sound").
- The detonator the game shows during its act: place it in the pressing hand from the C4 weapon's attachment update.
- The gunner seat's turret: find what turns a manned vehicle gun, since the aim position does not.
- A torn mounted gun: take down its mount constraints and pick it up by its own pickup.
- The wingsuit's speed out of a reel: the reel's exit scale and the wingsuit's velocity, for a lerp.
- The canopy's yaw on opening (1459): the parachute's velocity alignment below a speed.
- Lockers: how the game refills the reserve there.

### Smaller items that do not

- A gunner-seat view correction, as the driving view's for the wheel.
- The helicopter controls: measure the utility helicopter's collective and the second helicopter's controls from their meshes, and a debug UI mode that places a control at the hand and saves it.
- The dual-wield twin shown stacked on the rack once it is ready.
- The debug panel pausing the weapons rather than standing them down, which dropped the SMG at 4380.
- The bars' twist grips removed outright if the triggers suit the reviewer.
- The mounted gun's hands drawn on its handles off the gunner seat (the anchored tier exists only in the scripted mounted state).
