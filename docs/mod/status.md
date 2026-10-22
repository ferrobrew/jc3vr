# Status rows

The status rows say whether each of the mod's systems is live, for confirming a headset session at a glance. The code is `payload/src/status/`; the debug UI's Status tab (`payload/src/ui/status.rs`) shows every row, and the scenario runner's status-row probes ([testing/recorder.md](testing/recorder.md)) read a row by its label. Every row's line is a state word, `OK` (live and doing what it should), `OFF` (switched off, or not used in the current setup), `WAITING` (on, but waiting for a session, the player, or a measurement), or `FAILED` (on and broken), followed after a comma by a short reason or the key numbers. The rows are grouped in sections:

| Section | Row | Reads |
|---|---|---|
| `SAVED SETTINGS` | `SETTINGS FILE` | the settings file's save state ([settings.md](settings.md)) |
| `RENDERING` | `VR SESSION` | whether a VR frame is rendering, and the session state the runtime last reported; `FAILED` while bring-up is retrying |
| | `DIRECT EYE BLIT` | how the last frame left the direct route (`vr::direct_status`): both eyes direct, the back buffer unreadable, a failed acquire, the route given up for the session, or not armed (the single-pass collapse, or a mono frame) |
| | `FRAME TIME` | the median and 95th percentile of the last few seconds of game frame intervals, from the always-on frame timing |
| | `RENDER SIZE` | the engine's render size, and whether it has reached the native target |
| | `RENDER-RATE BODY` | how many of the player's body draws the presentation layer re-posed in its last active frame, and its smoothed cost per frame in milliseconds ([rendering/presentation-layer.md](rendering/presentation-layer.md)); `WAITING` with why before it has run, `FAILED, NO RIG` for a skeleton without its bones |
| `INPUT` | `CONTROLLER ACTIONS` | whether the actions are attached, created or reused from a reinject, and whether a restart is needed |
| | `LAYOUT FINGERPRINT` | the first eight hex digits of the action layout's fingerprint for the hand the bindings were suggested for (the whole one is logged) |
| | `SELF-CHECK` | whether the input self-check has run, and how many findings it logged |
| `HUD` | `ATLAS PARTS` | which parts the HUD atlas moved into cells on the last frame, or the count of failed moves and restores |
| | `WORLD MARKERS` | how many markers were placed in the world on the last frame, and how many were left on the panel |
| | `MARKER DEPTH` | how many of them took their distance from the scene depth, the fallback ray, or their own point |
| `GUN IN HAND` | `RIGID GRIP` | which hands' guns were pinned to the canonical grip at their last placement, and the pins since load |
| | `CANONICAL GRIP` | whether each hand's canonical grip was read, or why not (bones missing, an unusable bind pose) |
| | `FLIP WATCH` | the grip watch's snaps and sweeps since load; a snap fails the row |
| | `ANIMATED GRIP OFFSET` | how far the clips hold each weight class's gun from the bind pose's grip, in degrees, last seen with the hand on its controller at full weight; a diagnostic for the unverified assumption that the two agree, so it shows numbers rather than a verdict |
| | `ATTACH BONE CHANGES` | the grip watch's attach-bone changes per hand |
| `WEAPONS IN HAND` | `LEFT HAND`, `RIGHT HAND` | the weapon's slot in each hand and how it came there: docked, picked up, stolen, off a rack, the game's draw, or the second weapon ([input/physical-weapons.md](input/physical-weapons.md)) |
| | `LEFT HAND ON GUN`, `RIGHT HAND ON GUN` | with `controllers.weapons.regions.enabled` on, the part of the wielded gun each hand holds and whether it fires it from the grip, braces it, or carries it unable to fire, with the carries, braces, and grips taken since load ([input/physical-guns.md](input/physical-guns.md#grabbing-a-gun-anywhere)) |
| | `TAKE FROM HANDS` | the guns in other characters' hands within reach of a grab, and the takes since load |
| | `TAKE FROM RACKS` | the weapons on racks within reach of a grab, the weapons racks handed over, and the copies asked of racks with unlimited stock |
| | `LEFT ACTION`, `RIGHT ACTION` | with the physical action on, each held gun's action on its travel: how far its part is back (and a bolt's turn up), whether a hand holds it or the slide is locked back, the chamber, and the actions worked since load ([input/reload.md](input/reload.md#the-physical-action)) |
| | `SECOND WEAPON` | the second weapon's slot and hand, its magazine, and whether the true reload is on it or on the wielded weapon |
| | `RIGHT HAND GRIP` | which grip the right hand took for a gun (its own, or the left hand's mirrored), and the angle between the two ([input/physical-weapons.md](input/physical-weapons.md#the-right-hands-grip)) |
| | `HOLSTERS` | how many holsters were moved onto the guns docked in them on the last tick |
| | `LAST REFUSAL` | why the last pickup, take, draw, holster, drop, or second weapon was refused |
| `INVENTORY` | `TRACKED WEAPONS` | how many weapons the VR inventory tracks as the player's, in the world, and with other characters, and the spawn system resets it saw ([input/vr-inventory.md](input/vr-inventory.md)) |
| | `CARRIED` | each weapon carried beyond the slots and its socket (and whether they are stashed for a mission), the carried weapons drawn and the held ones put away since load, and the weapons pickups would have dropped that went onto a socket instead |
| | `SAVE SIDECAR` | how many times the save sidecar has been written and how many carried weapons the last one held, or that the last write failed (the error is logged) |
| | `LAST LOAD` | what the last load made of the sidecar (matched with the weapons to restore, none, unreadable, or another save's), and how many carried weapons the last respawn put back, from the load or the reset |
| | `KEPT IN THE WORLD` | how many of the weapons lying in the world are kept from the game's despawn, the pins since load, and the weapons left to the game |
| `HAND BODIES` | `CALLBACK` | whether the pre-simulation callback is registered, has fired, and fires on the game thread (else the thread's id) |
| | `CALLBACK FIRES` | how many times it has fired |
| | `LEFT BODY`, `RIGHT BODY` | each body's phase (in the world, parked, or none) and its id |
| | `BODY ERRORS` | the bodies made and destroyed, or the last body error |
| | `CONTACT EVENTS` | which hands' bodies listen for contacts, the contacts seen, and any dropped |
| | `BLOWS DEALT` | blows dealt to characters, and the last one's impulse |
| | `GRAB JOINTS` | how many hands hold what they grip through a joint |
| | `COLLIDER VIEW` | whether the collider view is on (off, it names the key that shows it), and the bodies it draws out of those in range, its primitives, and the bodies its budget left out ([input/collider-view.md](input/collider-view.md)) |
| `HANDS IN THE WORLD` | `PALMS` | whether the palms' tick acts, and with how many casts, or why it stands down; failed when a tick ran off the main thread, which takes away every door, pickup, and climb ([input/physical-hands.md](input/physical-hands.md#threading)) |
| | `VEHICLE DOORS` | the grips taken on latched doors, the latches undone, the last door's slot, and whether a hand is on a latch ([input/vehicle-doors.md](input/vehicle-doors.md)) |
| | `CHARACTERS` | the grips taken on characters, the yanks, the knockdowns of the yanks and shoves, the ragdolls dragged, and the occupants pulled out of a seat ([input/character-grabs.md](input/character-grabs.md)) |
| | `MOVING CLIMBS` | the grips taken on moving bodies, and whether a hand holds on to one ([input/hand-climbing.md](input/hand-climbing.md#moving-bodies)) |
| | `STUNT POSITIONS` | the stunt position the game holds the player in (hanging from a helicopter, with whether a grip holds it, or on a roof), and the hangs and roofs entered and the hangs let go; a note names the last refused entry ([input/stunt-positions.md](input/stunt-positions.md)) |
| | `PUSH UP` | the pushes up with the hands pressed down, the lift while they lift, and the last push's rise ([input/physical-hands.md](input/physical-hands.md#pushing-up)) |
| | `HINGED THINGS` | the articulated objects the hands took hold of, and whether one is in reach ([input/physical-hands.md](input/physical-hands.md#articulated-objects)) |
| `FEEDBACK` | `HAPTIC CUES` | the haptic cues sent, and those the mixer masked under a heavier cue or bent under its ceiling ([input/haptics.md](input/haptics.md)) |
| | `IMPACT SOUNDS` | the impact sounds played, the impacts the cooldowns held back, and the impacts whose material has no melee impact sound ([input/impact-sounds.md](input/impact-sounds.md)) |
| `FINGERS` | `LEFT GRIP POSE`, `RIGHT GRIP POSE` | the pose each hand's fingers take and the source that declared it, its share, the fingers the contact fit placed, and whether a released pose is opening; `GAME'S GRIP` for a gun left to the game's grip ([input/grip-poses.md](input/grip-poses.md)) |
| `GRAPPLE` | `GRAPPLE STATE` | where the grapple is: inactive, firing, in flight, attached, reeling in, hanging, reeled in, or a tether line; waiting while the controllers' input tick does not run |
| | `ATTACHED TARGET` | an object or a surface, and the distance from the head to the attach point |
| | `REEL FROM HAND` | the reel's scale from the grapple hand's pull while it drives a reel, whether a yank boosts it, and the reels and boosts since load |
| | `TETHERS` | the tethers attached, the retract the pull drives, the tension while they are squeezed, and the pulls since load |
| | `HANG SWING` | the hang ticks the hand swung, and how many of them its motion pumped |
| | `LINE HAPTICS` | the grapple hand's line level, the stronger of the tension's and the reel's |
| `LOCOMOTION` | `STICK VELOCITY` | whether the stick drives the body's velocity this tick, and its speed |
| | `ACT FILTER` | acts dropped and swapped (and refused swaps) since load |
| | `ACT WATCHDOG` | the watchdog's releases, and whether one is in progress |
| | `STEADY VIEW` | the root-fixed view anchor: engaged, blending, measuring, waiting for the player to stand still, or idle |
| | `NEUTRAL HEAD` | the measured neutral eye height in model space |
| `PROCEDURAL BODY` | `MODE` | the procedural body's mode, and whether its gate has run in it (the log adds how long ago); a rig error fails it |
| | `PASS-THROUGH ERROR` | in the test mode, the largest read-back error over the driven bones, failed past a millimetre or a tenth of a degree |
| | `HAND-OVER CASE` | in the full mode, the hand-over table's current case; `OK` on foot, `WAITING` while the animation has the body |
| | `GROUND CASTS` | in the full mode, the last gate's ground casts that hit, and any errors |
| | `STANCE` | whether the neutral stance has been measured |
| | `LOWER BODY` | in the full mode, the lower body's pose on foot: `STAND`, `CROUCH`, `KNEEL` with the knee that is down and the kneel's weight, or `SIT` with how the legs lie (`CROSSED` or `KNEES UP`) and the sit's weight; `WAITING` off the ground |
| | `POSE INPUTS` | in the full mode, what the pose is picked from: the hips' height the head asks for against the squat's floor, the crouch's depth, how long the head has been low and still enough to sit, `MOVING` while the body is told to move, `SEATED` in the seated posture (which does not sit on the ground), and `RISE TO SIT` after the stick stood the body up, until the head rises; the log adds the head's speed and the sit's weight |
| | `TWIST BONES` | in the full mode, how many twist bones the limbs' twist is shared over, on how many segments, and the largest twist on the last pass; `WAITING` while the body has no weight, `NONE FOUND` fails it |
| | `WINGSUIT ARMS` | in the full mode with `ARMS AS WINGS` on, the wingsuit's arms' weight and the hands queued; `OK` with both at full weight, `NOT IN THE WINGSUIT` otherwise |
| | `PARACHUTE BODY` | in the full mode with `BODY UNDER THE PARACHUTE` on, the torso's weight under the parachute and how far the legs flare for the handles' brake; `OK` at full weight, `NOT UNDER THE CANOPY` otherwise |
| | `SWIM TORSO` | in the full mode with `TORSO AND ARMS SWIMMING` on, the torso's weight in the water and the neck's distance from the point under the player's head; `OK` at full weight, `NOT SWIMMING` otherwise |
| | `ROOF TORSO` | in the full mode with `TORSO ON VEHICLE ROOFS` on, the torso's weight standing in a vehicle's stunt area and the neck's distance from the point under the player's head; `OK` at full weight, `NOT ON A ROOF` otherwise |
| | `BODY MIRROR` | the body mirror, a diagnostic switched on in the debug UI: how many of its stick figure's bones it found; `NO BONES FOUND` fails it |
| | `SEATED TORSO` | in the full mode with `TORSO IN VEHICLES` on, the seated torso's weight and the neck's distance from the point under the player's head; `OK` at full weight, `NOT SEATED` outside a seat |

The Status tab reads every row each frame it is drawn, and lays each out as its label, its state word, and its detail, with the row's longer note, where it has one, on hover. Its `Log the summary` button logs every row once at info, as one line starting `status:`, each row as `label: value` and sections separated by `|`; where a value is cut short for its row (the full fingerprint, an error's text, the hand-over case's full name), the log line adds the whole of it in parentheses.

A row may be read on any thread, including in the middle of a frame while the game thread holds the VR runtime lock ([threading.md](threading.md)), so each reads only what its module publishes for any thread: atomics, or a leaf lock nothing holds while it waits, and one at a time. Some rows read the configuration, so a caller holds no configuration lock while it reads them. The modules publish these for the rows: the direct route's last frame (`vr::blit`), the session state and bring-up result (`vr::state`), the render sizes (`vr::resolution`), the frame-interval percentiles (`frame_timing`), each hand's canonical grip read (`controllers::hands::grip`), the finger curls' last tick (`controllers::fingers::status`, a copy published apart from the curls' own state), the hand bodies' callback and contact listeners (`physical_hands::physics::callback_status`), the physical grapple's readout (`grapple::physical::status`), and a copy of the procedural body's status taken after its gate lets go of its own state (`body::procedural::published_status`), since the gate holds that state through its ground casts, and the twist distribution's last pass (`body::procedural::twist::status`).
