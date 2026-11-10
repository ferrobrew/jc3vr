# Analysis of the 10 November 2026 11:32 session

Take: `sessions/2026-11-10_11-32-19_615/recordings/2026-11-10_11-33-15_573/`. The session ran from 10:32:19.620 to the crash at 10:37:08.397 UTC. Tracking started at 10:33:15.588, the video at 10:33:16.546, and the video is 232.167 s long. The take has video, `tracking.jc3trk`, `frames.jsonl`, and `take.rrd`. The build string is `2026-11-09 23:44:44`.

The reviewer's times are the rerun timeline's, which starts with the take's wall clock at the tracking start. The session log times below are UTC; a rerun time `t` is about `10:33:16.5 + t`. The review is log- and frame-based; a log event is evidence that the mod requested or observed an action, not that it looked correct.

Files:

- [philpax_feedback.md](philpax_feedback.md): the session's feedback verbatim.

## Crash

The session ended in an access violation on execute at `JustCause3.exe` RVA `0x28DCCE8`. The address is in `.rdata`, not code. The faulting stack was the mounted-gun runtime, and the registers named a `CGameObject` and the value eight bytes into it: `WeaponBase::m_Holder` names its holder's `CGameObject`, but `doors::engine::vehicle_of` expects the physics game object around it. Passing the holder added `GAME_OBJECT_BASE_OFFSET` twice, landing on the holder's `CEventHandler` subobject; its two-entry vtable has no `IsType` slot, so the dispatch read the RTTI column pointer after the table and jumped into `.rdata`. The player was near a car; the holder was a boat with a mounted gun. The fix resolves the holder with `physics_object_of`, which tests `PhysicsGameObject::TYPE_ID` before converting, and keeps the mount comparison in the physics object space (`ca67f620`).

## Feedback and changes

### The debug UI started visible and could not be used until the window was refocused

The panel's visibility (`hud.egui_panel.enabled`) was an ordinary saved setting, and the panel's activation opened it, so a previous session's panel came back in the headset. It is now session state (`#[serde(skip)]`) and starts hidden. The input side was the desktop tracker not knowing the game window's handle until a window message arrived: a focused, idle window sends none, so every sample was `WindowNotObserved`, `focused` was unknown, and the editing owner never became active. The tracker now falls back to the graphics params' handle, so the first sample reads the real foreground. Desktop focus now gates only the sources the window carries, the keyboard and text; the pointer, grip drag, and scroll are the virtual cursor's, so the in-world panel stays editable while the window is in the background. A focus loss while a key is held still clears the remembered key and cancels the pointer drag.

### Holsters dropped weapons that should have gone in

At rerun 39.5 s (10:33:55.191) the left pistol was released into the world while the right hip drew the right pistol in. The release catch consults the socket's own reach and then its attraction; the left hand was outside both. The socket radii are now 0.18 m at the hips and 0.255 m at the shoulders, and the attraction reaches 0.375 m and 0.675 m, each 1.5 times the old value.

### Grapple-snatch still refused weapons on the ground

The take has no `WEAPON ON THE GROUND` offer: the only snatch offers are `OBJECT` and `CAR DOOR`, while hand pickup takes the same ground weapons at 0.10 to 0.16 m throughout. `b9f3f369` added a surface cast that must meet the offered body exactly. A weapon body casts without a game object, so every ground weapon offer was rejected. Ground and world-grenade offers now skip that surface gate and take their anchor from the body hit, as placed charges already do.

### The camera bobbed with the walking animation

The view hangs from the animated head, and the stable view anchor was switched off entirely for the headset. The stable anchor is now computed in the headset too, but only for the camera: the pose and body IK keep the animated head, so the head bone stays on the animation while the view hangs from the neutral head over the smoothed root. The camera's eye point uses the matching neck delta and eye arm. Vehicles and airborne states are excluded by the same eligibility as before, and the neutral needs 1.5 s of standing still before the 0.25 s handover.

### The left hand could not draw from the back

`volumes::reserve_long_guns` dropped every shoulder offer not marked for the dominant hand while both hands were empty, so the off-hand had no draw candidate at either back socket. Unmarked offers still go to the dominant hand, but an offer a socket names stays with it. Drawing with the off-hand moves the layout with the gun, which the trigger, tether, pouch, and wrist-detomator tables follow.

### A weapon snapped to the hand at 138.3 s

The grab volume on a gun derived its radius from the hand's own touch distance, so any touch the palm ray could find was in reach, and a handle handover moved the gun to the other hand at once. The handle and foregrip volumes are now capped at `HANDOVER_MAX_M` (0.20 m); beyond it the grab is refused. A handle handover within the cap takes the gun's pose before the move and slides it into the hand over `SNAP_S`, as a pass by the action already does.

### No feedback when inserting into a loaded well

The well marker is gold while a round is in hand. It now turns red (`MarkStyle::NoFitRing`) while the well still holds a round. A round pushed at an occupied well reports `InsertEvent::Refused` once per approach, which plays the gun's dry-fire click and the pouch-refusal pulse; the wrist summary keeps asking for the old magazine to be cleared.

### The head was detached in the plane stunt position at 176 s

Since `ac274945` the stunt body keeps the character's height, and the torso's bounded lean could not close the gap to a player standing taller than the reference. The reference now shifts toward the tracked head's height by up to 0.20 m before the solve, so the feet stay on what they stand on and the head comes into reach. The case still changes to the on-foot body when the player leaves the plane, which is why leaving the plane fully realigned the body.

### The door grab target was too small at 193 s

`HANDLE_REACH_M` is now 0.30 m, twice the old 0.15 m. It is the handle-grab radius in both the whole-vehicle handle scan and the palm's door reach.

### The C4 detonate button and hint showed with nothing placed

The wrist detonator's 3D button was drawn on the wrist whatever was placed, dim with no charges, and the wrist summary listed it as a mode essential on foot. Both now depend on the C4 weapon's own placed count: with no charge out in the world the button and its overlay approach feedback are cleared (`c4/wrist.rs`), the summary's detonate hints (the stick-click action and the wrist detonator) are withheld (`bindings/summary.rs`, `summary/eligibility.rs`, and `summary/essentials.rs`, all reading `c4::charges()`). With a charge placed they return; the button still dims while a detonation is in flight.

## Not session feedback

The request that the rerun scenes default to tracking Rico's head with the eye is a tooling change, not feedback about this session. The exporter's pinned Rerun SDK (0.40) writes the views' blueprint but exposes no eye-controls API, and the viewer's eye control is a blueprint property the SDK does not let a caller set. The head is logged at `/world/cockpit/head`; the request needs either an SDK patch or a blueprint splice in `tools/tracking-synth/src/rerun_export/blueprint.rs`, and is not included in this pass.

## Unverified

None of the changes has been run in the headset. The camera handover, the stunt height shift, the refusal feedback, and the grab cap need a live pass; the holster, back-bank, snatch, and focus changes are verified by unit tests and the existing in-game evidence only.
