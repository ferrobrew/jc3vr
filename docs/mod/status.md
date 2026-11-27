# Status rows

The status rows say whether each of the mod's systems is live, for confirming a headset session at a glance. The code is `crates/jc3vr-ui/src/status/`; the debug UI's Status feature (`crates/jc3vr-ui/src/ui/overview/mod.rs`) shows every row and logs them as a summary on request. Every row's line is a state word, `Ok` (live and doing what it should), `Off` (switched off, or not used in the current setup), `Waiting` (on, but waiting for a session, the player, or a measurement), or `Failed` (on and broken), followed after a comma by a short reason or the key numbers. A row has one spelling, which the Status feature, the feature pages, and the log all show. The rows are grouped in sections:

| Section | Row | Reads |
|---|---|---|
| `Rendering` | `VR session` | whether a VR frame is rendering, and the session state the runtime last reported; `Failed` while bring-up is retrying |
| | `Direct eye blit` | how the last frame left the direct route (`vr::direct_status`): both eyes direct, the back buffer unreadable, a failed acquire, the route given up for the session, or not armed (the single-pass collapse, or a mono frame) |
| | `Render size` | the engine's render size, and whether it has reached the native target |
| | `Render-rate body` | how many of the player's body draws the presentation layer re-posed in its last active frame, and its smoothed cost per frame in milliseconds ([rendering/presentation-layer.md](rendering/presentation-layer.md)); `Waiting` with why before it has run, `Failed, No rig` for a skeleton without its bones |
| `Input` | `Controller actions` | whether the actions are attached, created or reused from a reinject, and whether a restart is needed |
| | `Layout fingerprint` | the first eight hex digits of the action layout's fingerprint for the hand the bindings were suggested for (the whole one is logged) |
| | `Self-check` | whether the input self-check has run, been skipped for a runtime and controllers checked before, and how many findings it logged |
| | `Session focus` | whether the session has focus, and whether any controller is active, from the latest controller frame |
| | `Debug panel` | the debug panel's mode (hidden, editing, or read-only), waiting while it holds back gameplay input the player asked for; a note names what blocks it |
| `Capture and tools` | `Video recording`, `Tracking capture`, `Profiler trace` | each recorder's state: off when idle, failed with its last error, and otherwise recording with its progress |
| | `World overlay` | the primitives the overlay drew per eye and the frames it has drawn in; failed with the pipeline's last error |
| `HUD` | `Atlas parts` | which parts the HUD atlas moved into cells on the last frame, or the count of failed moves and restores |
| | `World markers` | how many markers were placed in the world on the last frame, and how many were left on the panel; a note counts how many took their distance from the scene depth, the fallback ray, or their own point |
| `Gun in hand` | `Rigid grip` | which hands' guns were pinned to the canonical grip at their last placement, and the pins since load |
| | `Canonical grip` | whether each hand's canonical grip was read, or why not (bones missing, an unusable bind pose) |
| | `Flip watch` | the grip watch's snaps and sweeps since load; a snap fails the row |
| | `Animated grip offset` | how far the clips hold each weight class's gun from the bind pose's grip, in degrees, last seen with the hand on its controller at full weight; a diagnostic for the unverified assumption that the two agree, so it shows numbers rather than a verdict |
| `Weapons in hand` | `Left hand`, `Right hand` | the weapon's slot in each hand and how it came there: docked, picked up, stolen, off a rack, the game's draw, or beside the wielded weapon ([input/physical-weapons.md](input/physical-weapons.md)) |
| | `Left hand on gun`, `Right hand on gun` | the part of the wielded gun each hand holds and whether it fires it from the grip or braces it, with the braces and the guns passed to the brace since load ([input/physical-guns.md](input/physical-guns.md#grabbing-a-gun-anywhere)) |
| | `Take from hands` | the guns in other characters' hands within reach of a grab, and the takes since load |
| | `Take from racks` | the weapons on racks within reach of a grab, the weapons racks handed over, and the copies asked of racks with unlimited stock |
| | `Armed beside` | each weapon armed in a hand beside the wielded one, its slot and hand, and its magazine ([input/physical-weapons.md](input/physical-weapons.md#a-weapon-in-each-hand)) |
| | `Right hand grip` | which grip the right hand took for a gun (its own, or the left hand's mirrored), and the angle between the two ([input/physical-weapons.md](input/physical-weapons.md#the-right-hands-grip)) |
| `Inventory` | `Tracked weapons` | how many weapons the loadout table has on the body and in the world, how many the player let go of another character holds, and the spawn system resets the inventory saw ([input/two-handed-inventory.md](input/two-handed-inventory.md#the-weapons-in-the-world)) |
| | `Carried` | each weapon on the body beyond the slots and its socket (and whether they are stashed for a mission), and the weapons pickups would have replaced that kept their place on the body ([input/two-handed-inventory.md](input/two-handed-inventory.md#the-weapons-beyond-the-slots)) |
| | `Save sidecar` | how many times the save sidecar has been written and how many carried weapons the last one held, or that the last write failed (the error is logged) |
| | `Last load` | what the last load made of the sidecar (matched with the weapons to restore, none, unreadable, or another save's), and how many carried weapons the last respawn put back, from the load or the reset |
| | `Kept in the world` | how many of the weapons lying in the world are kept from the game's despawn, the pins since load, and the weapons left to the game |
| `Hand bodies` | `Callback` | whether the pre-simulation callback is registered, has fired, and fires on the game thread (else the thread's id), and how many times it has fired |
| | `Left body`, `Right body` | each body's phase (in the world, parked, or none) and its id |
| | `Body errors` | the bodies made and destroyed, or the last body error |
| | `Contact events` | which hands' bodies listen for contacts, the contacts seen, and any dropped |
| `Hands in the world` | `Palms` | whether the palms' tick acts, and with how many casts, or why it stands down; failed when a tick ran off the game thread, which takes away every door, pickup, and climb ([input/physical-hands.md](input/physical-hands.md#threading)) |
| | `Vehicle doors` | the grips taken on latched doors, the latches undone, the last door's slot, and whether a hand is on a latch ([input/vehicle-doors.md](input/vehicle-doors.md)) |
| | `Characters` | the grips taken on characters, the characters thrown down by a grip's topple or a yank, the knockdowns of the yanks and shoves, the ragdolls dragged, and the occupants pulled out of a seat ([input/character-grabs.md](input/character-grabs.md)) |
| | `Moving climbs` | the grips taken on moving bodies, and whether a hand holds on to one ([input/hand-climbing.md](input/hand-climbing.md#moving-bodies)) |
| | `Stunt positions` | the stunt position the game holds the player in (hanging from a helicopter, with whether a grip holds it, or on a roof), and the hangs and roofs entered and the hangs let go; a note names the last refused entry ([input/stunt-positions.md](input/stunt-positions.md)) |
| | `Push up` | the pushes up with the hands pressed down, the lift while they lift, and the last push's rise ([input/physical-hands.md](input/physical-hands.md#pushing-up)) |
| | `Hinged things` | the articulated objects the hands took hold of, and whether one is in reach ([input/physical-hands.md](input/physical-hands.md#articulated-objects)) |
| `Feedback` | `Haptic cues` | the haptic cues sent, and those the mixer masked under a heavier cue or bent under its ceiling ([input/haptics.md](input/haptics.md)) |
| | `Impact sounds` | the impact sounds played, the impacts the cooldowns held back, and the impacts whose material has no melee impact sound ([input/impact-sounds.md](input/impact-sounds.md)) |
| `Fingers` | `Left grip pose`, `Right grip pose` | the pose each hand's fingers take and the source that declared it, its share, the fingers the contact fit placed, and whether a released pose is opening ([input/grip-poses.md](input/grip-poses.md)) |
| `Grapple` | `Grapple state` | where the grapple is: inactive, firing, in flight, attached, reeling in, hanging, reeled in, or a tether line; waiting while the controllers' input tick does not run |
| | `Attached target` | an object or a surface, and the distance from the head to the attach point |
| | `Reel from hand` | the reel's scale from the grapple hand's pull while it drives a reel, whether a yank boosts it, and the reels and boosts since load |
| | `Tethers` | the tethers attached, the retract the pull drives, the tension while they are squeezed, and the pulls since load |
| | `Line lock` | while the lock holds the reel's line, the brake, the target's kind and mass, the held length, the stretch, the tension of the line's strength, and the hook's speed; otherwise the locks and breaks since load and how the last lock ended ([grapple-lock.md](input/grapple-lock.md)) |
| | `Hang swing` | the hang ticks the hand swung, and how many of them its motion pumped |
| | `Snatch` | what the snatch is doing (idle, armed, freeing, hook out, pulling), the target offered to the grapple hand and its distance or why none is offered, and the snatches taken of those fired; a note counts the failed snatches and the tosses, gives the mass of a loose object under the aim, and names the last refusal ([input/grapple-snatch.md](input/grapple-snatch.md)) |
| | `Line haptics` | the grapple hand's line level, the stronger of the tension's and the reel's |
| `Locomotion` | `Stick velocity` | whether the stick drives the body's velocity this tick, and its speed |
| | `Act filter` | acts dropped and swapped (and refused swaps) since load |
| | `Act watchdog` | the watchdog's releases, and whether one is in progress |
| | `Steady view` | the root-fixed view anchor: engaged, blending, measuring, waiting for the player to stand still, or idle |
| | `Neutral head` | the measured neutral eye height in model space |
| `Procedural body` | `Mode` | the procedural torso's mode, and whether its gate has run in it (the log adds how long ago); a rig error fails it |
| | `Hand-over case` | in the full mode, the hand-over table's current case; `Ok` on foot, `Waiting` while the animation has the torso |
| | `Twist bones` | in the full mode, how many twist bones the limbs' twist is shared over, on how many segments, and the largest twist on the last pass; `Waiting` while the body has no weight, `None found` fails it |
| | `Wingsuit arms` | in the full mode, the wingsuit's arms' weight and the hands queued; `Ok` with both at full weight, `Not in the wingsuit` otherwise |
| | `Parachute body` | in the full mode, the torso's weight under the parachute and the neck's distance from the point under the player's head; `Ok` at full weight, `Not under the canopy` otherwise |
| | `Roof torso` | in the full mode, the torso's weight standing in a vehicle's stunt area and the neck's distance from the point under the player's head; `Ok` at full weight, `Not on a roof` otherwise |
| | `Swim torso` | in the full mode, the torso's weight in the water and the neck's distance from the point under the player's head; `Ok` at full weight, `Not swimming` otherwise |
| | `Body mirror` | the body mirror, a diagnostic switched on in the debug UI: how many of its stick figure's bones it found; `No bones found` fails it |
| | `Seated torso` | in the full mode, the seated torso's weight and the neck's distance from the point under the player's head; `Ok` at full weight, `Not seated` outside a seat |

Each section module owns its rows as `status::Row` constants (`status::feedback::HAPTIC_CUES`, for example), and a row's formatter is the one place its status struct is turned into text. A feature page that has a row shows it as its headline through `ui::status_row::headlines`, the same state word, detail, and note as the Status feature, and keeps only the lines that add detail the row has no room for, so the two never disagree.

The Status feature reads every row each frame it is drawn, and lays each out as its section and label, its state word, its detail, and the row's longer note, where it has one, with the rows that need attention first. Its `Log the summary` button logs every row once at info, as one line starting `status:`, each row as `label: value` and sections separated by `|`; where a value is cut short for its row (the full fingerprint, an error's text, the hand-over case's full name), the log line adds the whole of it in parentheses.

A row may be read on any thread, including in the middle of a frame while the game thread holds the VR runtime lock ([threading.md](threading.md)), so each reads only what its module publishes for any thread: atomics, or a leaf lock nothing holds while it waits, and one at a time. Some rows read the configuration, so a caller holds no configuration lock while it reads them. The modules publish these for the rows: the direct route's last frame (`vr::blit`), the session state and bring-up result (`vr::state`), the render sizes (`vr::resolution`), each hand's canonical grip read (`controllers::hands::grip`), the finger curls' last tick (`controllers::fingers::status`, a copy published apart from the curls' own state), the hand bodies' callback and contact listeners (`physical_hands::physics::callback_status`), the physical grapple's readout (`grapple::physical::status`), and a copy of the procedural torso's status taken after its gate lets go of its own state (`body::procedural::published_status`), so a reader never waits on a gate in progress, and the twist distribution's last pass (`body::procedural::twist::status`).
