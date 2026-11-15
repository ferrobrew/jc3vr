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
36. Record a take with the in-process capture (the default): the video has both eyes side by side, the left on the left, with the HUD, at the colours and levels the headset shows, and the audio in sync with a gunshot. Compare the frame time with and without a take, and note the dropped and repeated frame counts in the debug UI ([capture.md](../../mod/capture.md)).
37. Get knocked down and stand up: the view keeps its heading through the get-up, and the body turns to face the player's torso after it.
38. Sprint and reach for the magazine pouch: the belt holds still enough to take it.
39. Open the debug panel holding an SMG in each hand: both stay in the hands; close it with a grip open: that gun goes back on the body.
40. Tear a tripod minigun off its mount by the handles: it comes away in the hands as a heavy weapon (the log counts the constraints freed). Snatch a heavy weapon lying on the ground. Drop it and take it again.
41. Pull a grenade's pin and hold it: it fizzes and trails (the log says `resourced=true`); put it back in the pouch: the sound stops.
42. With a charge in hand, pull the trigger: the game's detonator shows upright in that hand, and goes.
43. In a jeep's gunner seat, take the handles and swing them: the turret turns (the log says whether the gun turned from the stick, and the debug UI counts the hand-aimed ticks). The view sits back from the handles rather than over them.
44. Open the canopy out of a reel: no swing to the side (1459). Reel into the air and open the wingsuit: it keeps the reel's speed and slows smoothly (the log says `air: carrying a reel's speed`).
45. Grapple a helicopter from below its door: Rico arrives at the door side and Y enters; from below its belly: the line hang.
46. Under the canopy, the handles sit at the toggles' grips, not at the risers' tops; reach both.
47. Fall from very high, and take a scripted drop into a skydive: the body is prone with the view on its head, not standing.
48. Swing on the grapple lock and hang under the canopy: the legs swing with the physics, the holsters stay reachable, and nothing in the log repeats `firm ragdoll: in` and `out`.
49. Hold a soldier with both hands: no ring between the hands (the log names the point joints). Swing a long prop through your own chest: it stops against the torso; reach for every holster and pouch with a prop in the other hand: nothing blocks the reach.
50. At a weapon locker with a gun below its reserve's cap, walk up and stand there for a few seconds without taking anything: `ammo: the game refilled every reserve to its cap` means the locker's refill ran (then check the belt's magazines). Its absence means the locker's event never reached the player, and what the mod suppresses near a locker is the next suspect.
51. Look at a rack's pistol pair: both pistols show side by side; take one and then the other.
52. In the heavy troop helicopter and the utility helicopter, if a control is out of reach: in the debug UI's Air feature, press "Set the cockpit controls from the hands", close the panel, and hold the left hand where the collective should be and the right on the stick through the three ticks. Both hands buzz, the controls move there at once, and `cockpit_calibration.toml` (path in the readout) holds the numbers; keep it for the table.
53. Open the canopy out of the wingsuit (2806): the view rises with Rico's head.
54. Stand still in each idle the game gives (with a rifle, with a pistol, crouched): the log names any standing state the stance's idle list misses.
55. The IK body, in the order of [ik-body.md](../../mod/body/ik-body.md)'s checks: stand still with the body mirror on (the readout reads `idle`, both feet planted; Rico's pelvis under the player's, the feet still, the torso not pulled; if it fights, lower the hips' resist and turn reach first); bend and crouch (no steps, the knees forward, no floating or sunk feet); glance and turn (a glance does not step, a held turn takes two turn steps); step aside, forward, and back (one or two square steps, no slide, a few steps a standing minute); walk physically and on the stick (it hands over to the game's walk and comes back without a snap); step up and down a kerb and over a ledge; jump and fall (the takeoff and landing are the clip's, the pelvis under the head in the air); and the other modes and the gamepad unchanged.
56. Frame time (new problem 4): play a minute with the estimator on and a minute with it off (`controllers.body.estimator.enabled`), and compare the frame time and the `body_estimator` timing slot.
57. In a jeep's gunner seat and at a scripted mounted gun, Rico's hands and body are drawn; the log says `mounted gun: faded the gunner back in` once per use.
58. Fly the heavy troop helicopter and the utility helicopter: each control sits under the hand that reaches for it, and the utility helicopter's collective is to the left of and below the cyclic. If one is off, calibrate it as in check 52.
59. Reel onto a helicopter's door side and wait past the arrival: `rail: standing on the helicopter's skid`, and Rico stands on foot on the skid, riding with it as it flies. Walk along it physically and on the stick; step slowly outward (the pull holds you) and then briskly (you fall with the helicopter's speed into a skydive, `rail: let go of Rico` with `stepped off the skid`). Again: grip the skid with both hands and lower yourself to hang under it (`went off the skid's edge`, the climb carries you); jump off; press Y by the door and note whether the game enters (`the game took him into the helicopter`). Reach for the door handle. Note in the debug UI whether the update carried the proxy. With `vehicles.helicopter_rail` off, he drops into the hang as before.
60. With a full grenade pouch, put a grenade to it: a click with the buzz.
61. Grip a gas cylinder in a rack and hold still for five seconds: the log's `damping a held articulated object's assembly` shows more than one body, and the lag settles rather than ringing at `load 1.00`.
62. Hold a toppled soldier up for ten seconds, then lay them down in the hand for ten more: `holding a held character's ragdolling timers off its get-up`, and no `the game's get-up ended a hand's hold of a ragdoll`. If the get-up still fires, note `held_s` and `timers_held`. Let go: they get up a few seconds later.
63. Swing the far end of a plank into the chest and keep pulling: `a held object rests against Rico's torso capsules`, no push, no strike buzz or sound, and no tear. Wedge the plank behind a post: it still tears.
64. At a tripod minigun, close each grip on a handle without pressing use: the log says `manned=false`, and the drawn hands slide onto the handles and turn with the wrists. Open one hand: it blends back to the controller.
65. The IK body's new checks in [ik-body.md](../../mod/body/ik-body.md): the step rate against the replay's prediction of about four a standing minute; standing on a truck bed or a boat deck while it is driven (the readout says `a moving body`, no steps standing still, `pelvis_speed_mps` near zero); hand climbing a face (`takes the climbing legs`, soles flat on the face, knees toward it, one brace at a time); and swinging on the line and under the canopy with the muscle target live (the legs swing toward the IK air pose; if they jitter or fold, compare with `ragdoll_target` off).
66. Hang on the line under a helicopter and press Y: no wingsuit, `grapple lock: the lock ends` with `entering the helicopter`, `the reel pulls up to the door` with `DoorSide` (note `candidates` and `distance_m`), and Rico is pulled up onto the skid by the nearer door and stands there (`arrived at the door side`, then `rail: standing on the helicopter's skid`). Press Y again: he enters. He never goes into the upside-down hang: `arrived at the hang; back on the line` means the pull failed and he is back on the line. Watch whether the pull up passes through the airframe or catches on the skid.
67. Put a grenade back by bringing it down onto the pouch from above, stopping a hand's width over it: it goes back (`grenade: put the grenade back into the pouch`) rather than falling (2560).
68. Break car glass ([breaking-glass.md](../../mod/input/breaking-glass.md)): by a parked car, punch a side window lightly (a crack, a haptic strike, `glass: a strike cracked a pane`), punch the same spot until the pane comes down (`glass: a strike brought a pane down`), and punch another as hard as you can (down at once, or note the speed); rest a hand on glass and push it through slowly (nothing); jab a window with a rifle and with a pistol; then from inside a car, parked and driving, punch the windscreen and a side window. Note any `would shatter but stays` and its reason, and, if nothing breaks from inside, the readout's sweep and glass counts. Then shoot through a pane you brought down at someone inside: the shot lands with no new crack and no hit on the car, and the readout counts the pass. Note the logged collide factor for intact glass.

## What was built for the deferred work

Everything the earlier plans deferred was built on 15 November, untested in the headset; the checks above cover it. The commits are listed in the [README](README.md#the-deferred-work-built-on-15-november).

## Still open

- The bars' twist grips: kept behind their switch until the reviewer has tried the triggers, then removed outright if the triggers suit.
- The seated magazine drop stays for dry guns only, as the reviewer chose (a loaded gun's double tap is two shots).
