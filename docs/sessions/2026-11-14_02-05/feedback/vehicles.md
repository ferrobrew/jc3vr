# Vehicles, mounted guns, doors, UI input, rendering, and capture: 14 November 02:05 take

Written on 15 November from the take and the code as they were before the fixes listed in the [README](../README.md#the-feedback-item-by-item); file and line references are to that code. Scripts, dumps, and frame sheets it names were working files of the analysis and are not kept.

Times are the Rerun `t`. Paths are relative to the repo root. Evidence came from the tracking dump (`tracking-synth dump --json`), video frames, and the annotated log; scratch tables and contact sheets are in the analysis scratch folder (not kept) (`bike.py`, `mg_sheet.jpg`, `mg2_sheet.jpg`, `boat_sheet.jpg`, `heli_sheet.jpg`, `fighter.jpg`, `shadow_sheet.jpg`).

## 1. The scooter (191 to 252 s)

### What happened

The scooter was ridden on the handlebars from t 186.7 to 251.8 with Index controllers. I tabulated, per frame, both hands' squeeze value and force, the bars' holds, the injected `ACCELERATE` and `REVERSE`, and each hand's wrist twist about the bars' line measured from its grab orientation (the same quantity `grip_twist` computes).

The throttle and the brake follow the wrists exactly, so they are not random in the code's terms. They are random from the player's point of view, for three reasons:

- The player twists the right wrist 50 to 120 degrees to open the throttle (t 191.3: -92; t 202.6: -116; t 231.2: -119), four times the 30 degree full twist. Any wrist roll past 30 degrees is full throttle, and the 4 degree deadzone is smaller than the wrist roll that steering, leaning, and the bike's bumps produce.
- The left wrist rolls along with the right (a sympathetic roll). That opens the brake together with the throttle: `ACCELERATE` 1.0 and `REVERSE` 1.0 together at t 191.4 to 191.7, `REVERSE` 0.7 to 0.8 with full throttle at t 233.9 to 234.6, and 0.3 to 0.5 at t 189.0 to 189.4, 222.4, and 223.1. Full brake against full throttle stalls the bike, which reads as "stops at a random time".
- Every regrab takes a new reference orientation, so the throttle closes at each regrab. The left hand's regrab at t 218.56 read as full brake (`REVERSE` 1.0) within 70 ms and held it for 2.4 s, because the wrist was already rolled 50 to 90 degrees from where it was taken.

Losing the steering is the grip value, not the mod letting go. All 11 releases during the ride have the reason `grip released`. In every one the Index squeeze value falls from 1.0 to 0 over about 150 ms (the fingers leaving the handle; the force had been at 0 for seconds before), and the hold ends when the value crosses the release threshold (`ClosedHand` in `controllers/bindings/controls.rs:241`). No release came from the hand drifting or from tracking. Six of the releases come right after a large throttle roll-back (t 189.79 after -68 to +8 degrees, 203.31 after -116, 228.41 after -97, 229.51, 212.54, 192.89). That matches what a real motorcycle teaches: the throttle springs shut when you let go, so riders open the hand to close it. Here, opening the hand also drops the bars.

### Root cause

- The twist throttle and brake: `payload/src/controllers/vehicles/physical/buttons.rs:153` (`grip_twist`, relative to the grab orientation), `:166` (`twist_throttle`: 4 degree deadzone, `CockpitButtonsTuning::throttle_twist_deg` 30 degrees at `tuning.rs:115`), and `:349` (right grip is the throttle, left is the brake). `bars_twist` is on by default (`controllers/vehicles/config.rs:160`). The tightness of the range, the bilateral roll, and the reference reset at each regrab are each enough on their own to make it feel random.
- Steering loss: the hold is "only releasing the grip lets go". On the Index the release is the capacitive value under 0.5. That is fine for a wheel, which the player never needs to let go of to work a pedal, but here the throttle invites the hand to open.

### The two steering implementations compared

The wheel (`steering.rs`, `physical/mod.rs::step_wheel`) gives each hand its own `WheelGrip`. A hand's turn is its travel around the hub, measured against the wheel as it was when that hand took hold. When both hands hold, the hand that moved furthest leads and the other only holds it back. The response is linear over a 135 degree lock with a 2 degree deadzone. It has a stop bump and a centre tick (`WheelCue`, `control_haptics.rs:34`). The hand is drawn where it took the rim, carried round with the wheel.

The bars (`physical/handlebars.rs:50`) take one latch for the pair: the turn of the line between the two grips in the horizontal plane, or with one hand, that grip mirrored through the bars' live centre (`bar_turn`, `:119`, called with `placement.bars.centre` at `:64`). The deadzone is 3 degrees, the lock 30 degrees, and the response exponent 1.6. They have no haptic cue at all: `step_bars` returns only the steering, and `Outputs` gets no cue for `Handlebars` (`physical/mod.rs`, the `ControlKind::Handlebars` arm). Two specific weaknesses:

- With one hand, the measure depends on the live bars' centre in the cockpit frame, which rides the animated head and sways with the riding animation. The wheel deliberately measures against the wheel as it was at the grab for exactly this reason (steering-wheel.md, "Turning the wheel"). This is a hypothesis for the one-hand weave. The ride had about 25 s of one-handed riding.
- The bars react to the line between both hands. A wrist that rolls to throttle also moves its grip a little fore and aft, so the throttle hand steers.

### Fix proposal (binding: triggers for throttle and brake, generous steering with haptic guidance)

1. Remove the bars' twist grips. Delete the `Throttle` and `Brake` arms of `TwistGrip` in `buttons.rs` and keep `Collective`. Drop `bars_twist` from `controllers/vehicles/config.rs`, `tuning.rs:114` and `:136`, `controllers/presets.rs:73`, and `ui/controllers/vehicles.rs:448`. Drop `bike_throttle` and `bike_brake` from the outputs. The triggers already reach `ACCELERATE` and `REVERSE` in that seat ("the larger of the two winning"), so nothing new is needed for the pedals. Update `docs/mod/input/cockpit-controls.md` (the button table, the handlebars paragraph, checklist item 11) and `steering-wheel.md` ("Handlebars").
2. Make the bars as generous as the wheel, modelled on it:
   - Per-hand grips measured against the bars as taken: each hand's angle about the bars' pivot at its grab, with the leader rule for two hands, as in `WheelGrip`. This replaces the line latch and the live-centre mirror.
   - A grace period for the grip on the bars only: hold through a squeeze value under the release threshold for up to about 250 ms while the hand stays within the bars' reach. Also let a closed hand that comes back within reach retake the bars without a fresh press, as the latch already does for a slow fist ("the closed hand came to a target and took it").
   - Haptic guidance: the wheel's stop and centre cues (`WheelCue::Stop`/`Centre`) at the bars' lock and centre. Add a "drifting" cue: a soft continuous pulse whose strength grows as a holding hand's grip moves past about 6 cm from its point on the bars, so the player feels where to bring the hand back before the drawn hand separates from the bars.
   - Consider a linear response (1.0) and a wider lock than 30 degrees, matching the wheel's "one to one with the hands" choice; tune in the headset.
3. Conflict to note: with the triggers as pedals on the bike, the seated reload's "double tap on the trigger drops the magazine" (binding) cannot use the trigger on the bike or in any driver's seat. See section 7.

## 2. Mounted guns

### 2a. Too close to the gun in the gunner seat (535 s)

What happened: after the seat lock (t 535.4), the frames at t 536 to 550 (`mg_sheet.jpg`) show the camera directly over the handles, looking down at them from a few centimetres. The player stepped back physically to reach them.

Cause: the gunner seat gets no view correction. The driving view (`controllers/vehicles/driving_view.rs`) moves the eyes to a standard offset from the wheel's hub in cars only. In the turret, the native animation's head is right over the handles.

Fix: a gunner-seat counterpart of the driving view. Measure the handles' midpoint from the eyes (the handle points `mounted_tick` already has, `MountedTick::handle_points`), average it over half a second after the seat lock, and ease the view so the midpoint sits at a standard offset: something like 0.40 to 0.45 m ahead and 0.30 m below the eyes, limited as `driving_view::MAX_M` is. Publish it through the same `roomscale::compose_cockpit` path. Files: `driving_view.rs` (generalise the measured anchor from the hub to "the seat's controls"), `mounted_tick.rs`, `begin.rs`.

### 2b. Grabbing the controls in the gunner seat does nothing (536 to 550, 1747 to 1754)

What happened: the log has both hands taking the handles repeatedly (`the hand took the mounted gun's handle`). `controller_state` shows `mounted_holds` true and `mounted_aim` changing with the hands. The tiers read `mounted gun` / `anchored` with an anchor weight of 1.0. Yet in the frames no hand is drawn anywhere, only the grab rings on the red grips, and the gun points straight along the jeep throughout. Shooting works.

Cause, partly established:

- `bb2bf2c2` (12 November) made the runtime skip a gun the player mans from a gunner seat (`controllers/vehicles/mounted/runtime.rs:178`, `if native && seated { ... continue; }`), leaving it to "the native mounted aim". The handles' aim only reaches the gun as the aim position of its per-weapon update (`controllers/aim/gun.rs:183` detour on `WeaponBase::UpdateFromUserPostSim_ParallelBase`, `hand_target`) and as the character-level aim point (`controllers/aim/mod.rs:194`). The frames show that neither turns a vehicle turret. `vehicle-weapons.md` lists "that the seat's native mounted aim turns the vehicle's turret toward the handles' aim point" as unverified, and this session refutes it.
- Hypothesis: a vehicle gun is turned by its mount parts, which the vehicle drives from the gunner's camera or aim control, not from the weapon's aim position. RE needed: find what writes the turret parts' (or the mount constraints') motor targets for a manned vehicle gun, and what it reads. I tried the xrefs of `GetMountedHorizontalAngularLimits` and `GetMountedVerticalAngularLimits`. They lead only to vtables and a debug draw, so the starting point is the writers of `WeaponBase::m_ConstraintHorizontal` and `m_ConstraintVertical`, and the vehicle's turret update.
- The hands not being drawn: not established. Even the free hands are invisible in the seat, which suggests the character's model is hidden or culled in the turret's native camera rather than an IK failure. A render trace with the hands in view, or a read of Rico's visibility flags while manned, would settle it.

### 2c. "Is a regression": the standalone tripod minigun (3395 s) and the git history

What happened: at t 3394.5 to 3401.7 the hands took the tripod minigun's handles (not the native use: the runtime logged `the hands took the gun hands=[true, false]` with both hands on). The frames (`mg2_sheet.jpg`) show the hands floating at the handles rather than on them, the gun barely moving, a shot at t 3399, and the use prompt still drawn by the gun. The on-foot recording has no `vehicle` block, so the aim itself was not recorded.

The history:

- 9 November session (838 s, before the handle work): the player grabbed the mounted MG's body as a physical articulated object, which swung on its joint: "I can grab this mounted machine gun with my two hands, which feels great". This is the "grip and swivel it" the reviewer remembers.
- `9763ea22` (9 Nov 12:59) "Fire and detach physically gripped mounted weapons": the runtime disables the mount constraints' motors while it holds the gun (`runtime.rs:278`, `motor.disable()`), so that a physical grip can swing it.
- `939f0210` and `af243a10` (9 Nov evening): the handles become virtual grab volumes, and the gun aims "from the hands' midpoint through the pivot" via `mounted_target`.
- 10 November session (1974 s): "I can shoot the machine gun from this position, but I can't turn it". At that point the grip had taken the turret part as a loose prop.
- `7dbe31a5` (12 Nov) "Rank a mounted gun's handles over the gun's own body": the handle now always beats the gun's body, so the physical articulated grab of 9 November no longer happens at all.
- `bb2bf2c2` (12 Nov): the gunner seat skip above.

Root cause (strong hypothesis): the handle path aims by the weapon's aim position, which the mounted aim turns into motor targets. The pyxis def says as much ("The mounted aim drives its motor", on `m_ConstraintHorizontal`), and so does the runtime's own comment on `Motor` ("One mount constraint whose motor the mounted aim drives"). The runtime disables those motors whenever it holds the gun (`runtime.rs:278`), which was right for the physical grip of `9763ea22` and is wrong for a handle grip. So the handle aim has nothing to drive it, and `7dbe31a5` removed the physical grip that used to work. The regression is the combination of `939f0210`/`af243a10` (aim through a motor) with `9763ea22`'s motor disable, made total by `7dbe31a5`.

Fix:
- For a handle grip (no physical body), keep the motors enabled and let the aim drive them. Disable them only while a hand physically holds the gun's body. In `runtime.rs` around `:276`, gate `motor.disable()` on `physical_body.is_some()` and `restore` otherwise.
- Alternatively, drive the gun directly: write the motor targets from the handles' direction each tick, which also serves the gunner seat if the RE above shows the seat overrides the aim position.
- Draw the holding hands on the handles off the seat too. Today the anchored tier exists only in the `mounted gun` scripted state, so on foot the handle hold leaves the hands `free` and floating (`tiers` read `free` throughout t 3394 to 3401).

### 2d. The "Y to enter machine gun mode" prompt still shows

`native::Prompt::suppress` runs only for a gun the runtime holds (`runtime.rs:284`), so the prompt is back whenever no hand is on it, including the approach. Fix: suppress the pickup context of every observed mounted gun while the controllers scheme is active (the `SEEN` set in `runtime.rs`), restoring it only for the gamepad scheme (gamepad degradation memory). That also removes the "Y enters" path into the hard-to-escape native state.

### 2e. Pulled off the mount but stuck; holding it ejected the other weapons; couldn't pick it up (3401 to 3442)

What happened: at t 3401.7 the log has `the pull tore the gun off from_vehicle=false`, the table refused (`carry: the weapon is not holstered on the player afterwards`), and the use-item fallback ran. That fallback took a dead soldier's rifle off the floor into the right hand and the right shoulder at t 3402.1 (`took a weapon into the carry set on the right shoulder`), which is the "ejected my existing weapons" (the game's pickup swaps the occupant of the slot it fills). The frame at t 3402.6 shows the minigun still on its tripod. At t 3441.7 the grip notes do not list the minigun as a candidate at all.

Causes:
- The use-item fallback (`runtime.rs`, `tear_off` at `:381`) fires the generic action, which picks the nearest pickup, not the torn gun.
- `DetachMountedGun` → `OnDrop` sets the dropped state and adds the bodies to the world, but nothing in the tear removes the mount constraints (`m_ConstraintHorizontal`/`m_ConstraintVertical`). The runtime even restores their motors just before the tear (`record.restore()` at `:330`). In the game's own tear the gun goes straight into the user's heavy slot, so the constraints never matter. In the mod's world drop they keep the gun pinned on the tripod. This is a hypothesis; RE `DetachMountedGun`'s callees for any constraint teardown.
- The snatch then refuses it forever: `snatch/target.rs:571` and `:792` exclude any weapon with `m_MountedCanBeDetached`, which a torn-off tripod minigun still has (`031b5628`). The hands refuse it as a weapon "not lying dropped" while it is still in the mounted state (grip note at t 3400.4: `state 5`).

Fix: after the detach, destroy or disable the mount constraints. Take the gun into the hand through a pickup aimed at that weapon (the weapon's own `PickupItem` for the local player), not the use-item action. Let the table hold a heavy gun in a hand, which the "pistols are separate / physical parity" direction wants anyway. Change the snatch exclusion from `m_MountedCanBeDetached` to "still in `E_WS_MOUNTED`".

### 2f. 1735 s: a mounted MG registers as a yank target but cannot be yanked

What happened: three snatches at t 1731.8, 1733.3, and 1735.3 armed on `OBJECT at 11 m, 25 kg`, body 14588, and each ended `the hook's attachment proxy selected_body=14588 proxy_body=14583 matched=Rejected` / `the hook did not fire`. Body 14588 is the armoured vehicle's turret part (`vehicle part (vftable 0x142462228), body 14588` at the handles at t 1745.8), not the weapon, so the mounted-gun exclusion (which only looks at weapons) does not catch it. The hook lands on the vehicle's body, and the selection rejects it.

Fix: either exclude vehicle parts that carry a mounted weapon (or are a mounted weapon's holder or mount) from `snatch/target.rs`'s object candidates, or make a yank on one the grapple version of the tear: release the vehicle's hold and detach, as `mounted::vehicle_hold` already does for the hand tear. The second fits the tear's design; the first is the quick consistency fix.

### 2g. 3382 to 3389: couldn't ragdoll-grab the tripod gunner out

What happened: four grabs on the tripod's gunner each logged `a hand's grip threw a character into their ragdoll` and `a hand threw a character ... yank=true`, then `no ragdoll came within the hand's reach after a yank`.

Cause: the gunner pull (`controllers/physical_hands/characters/engine.rs:130`) requires `IsAttachedToVehicle() && IsInMountedGunState()`, so a standalone tripod's gunner takes the standing path. The mounted-gun state apparently keeps him out of the ragdoll.

Fix: accept `IsInMountedGunState()` alone in `gunner_out`. Unverified: that the standalone mounted state accepts `ACT_FULL_RAGDOLL`. If it does not, stop his use first (`StopUsing` on his mounted weapon), then ragdoll.

### 2h. 4055 to 4070: the boat's MG separated from the boat

What happened: the gunner-seat entry timed out at t 4053.3 with the hands already on the handles, the outside tear fired at t 4055.1 (`from_vehicle=true`), the table refused, and use-item ran. The frames are dark. The gun stays where the bow was through t 4065, and the player was then thrown off at about t 4067.

Cause (hypothesis): the vehicle hold is released (`vehicle_hold`, so the boat stops placing the gun every update), but the gun is not freed as a dynamic body that falls or comes to the hand (as 2e: constraints or motion type). So it hangs where the mount was while the boat moves on. Same fix as 2e. Also, nothing should tear while the gunner-seat entry is still running: the entry and the tear raced here.

## 3. Doors

### 3a. Open from anywhere on the door, force at the grab point (459 s, 2702 s)

Today a closed door is taken only with the palm within `HANDLE_REACH_M` (0.30 m, `doors/mod.rs:77`) of its table handle, or, for a door with no table entry, at a handle share of at least 0.4. The gate is `doors/acquisition.rs:80-90`, which returns `PanelRefusal::HandleRequired`. An open door is taken anywhere within the box reach. At t 2702 the visual handles differ from the table's: front doors take the handle from where the entry animation's hand goes (grade `clip`), which is not where the drawn handle is.

Fix: in `panel`, accept a latched door wherever `door_box.reach(palm, radii.box_m)` finds its outer face, and make `Latch::handle_palm` hold the hand at that reached point rather than the table handle. Keep the unlatch rule (5 cm outward pull from where the hand took it, `UNLATCH_PULL_M`). After the unlatch, `TearGrip::take` already holds the door at the grab point, so the hinge modulates the hand's force by its lever arm, as the reviewer wants. Pulling outward near the hinge barely swings the door, which is the "modulated accordingly" part, and it needs no special case. Move the green marker to the reached point (as for an open door) so it no longer advertises the animation-derived handle. The handle table then serves only the scan's "door in reach" ranking and the hijack's reach. Update `docs/mod/input/vehicle-doors.md` ("Finding a door", "The latch").

### 3b. 1097.4 s: would that open a fighter's cockpit and let you drag the pilot out?

Partly. The fighter's canopy is a door the mod already knows: `fighterjet_door_f from the table (box and constraint)`. At t 1097.6 the grip note reads `front left door (shut): handle 661 mm away, past the 300 mm reach` (`fighter.jpg` shows the hand on the canopy glass), so the handle gate is the only refusal. With grab-anywhere, the hand would take the canopy where it touched it and unlatch it with an outward (upward) pull.

Dragging the pilot out is a different matter. A yank on a seated character through an open door clicks the vehicle-entry action (`character-grabs.md`, "Getting in"), so the game plays its own hijack. From the stunt position on top of the jet, the game already offers it ("Get in vehicle" on screen). So the result would be the game's fighter hijack animation, not a physical drag. Unverified: whether `Vehicle::OpenDoor` is honoured on a plane in flight, how a loosened canopy behaves at 75 m/s, and whether the reach rays hit the pilot through the opening.

## 4. Helicopter controls (3791 s, 3879.97 s)

Where they live: in code, not data.

- Measured cockpits are in `controllers/vehicles/physical/native.rs::cockpit_asset` (`:175`): the utility helicopter (`utility_body`), the fighter, the fighter-bomber, and the Akrobat. Two more are in `asset` (`:242`): the light assault helicopter's joystick and the floatplane's control wheel.
- Everything else uses the fixed specs in `controllers/vehicles/tuning.rs` (`cyclic` `:62`, `collective` `:68`), relative to the seated character's eyes in the cockpit frame. That frame's yaw is re-based at the seat lock by the head's direction, so the specs land somewhere different on each entry.

At t 3791 (the utility helicopter): the cyclic is the measured mesh grip (`stick_grip`), which is why it sat right. The utility entry has `lever: None`, so the collective comes from `CockpitFit::new`'s fallback (`native.rs:105-115`): `collective.rest_m` = (-0.28, -0.80, -0.10) from the eyes, clamped to the whole body part's bounds. For a left-seat pilot, 0.28 m further left is at or past the cabin wall, and the clamp to the airframe's outer box (skids and sponsons included) does not catch it. The frame at t 3791 shows the collective's bar by the door.

At t 3879.97 the second helicopter (the dark military one; type hash 0ee45be6, model not logged) has no entry in `cockpit_asset`, so both controls are the fallback specs, which ignore the cabin.

Fix: measure the utility helicopter's collective from its body mesh, as its cyclic was, and add the other helicopters' bodies to `cockpit_asset` (the tool that measured the utility's grip from connected components). Log the body part's name with the `driving position` line so the next session says which helicopter it was.

Easy in-game calibration: none exists. `PhysicalControlsTuning` is fixed, not configuration. The cheapest useful version is a debug UI "place controls" toggle: while on, a grip on a control moves the control itself to the hand. On release it stores the offset in the body part's own space, keyed by the part name and the control kind, in a `cockpit_overrides` table in settings.toml, which `cockpit_asset` and the fallback consult first, as `SEAT_OVERRIDES` does for the wheel. The values found that way can then be moved into `cockpit_asset`.

## 5. 730 to 734 s: freeze the UI cursor on a light pull, click at full press

Already done after the session: `f99b19a3` (15 November 06:51, not in the session's build) adds `hud/pointer/aim_hold.rs`. Its behaviour: the cursor pins at a trigger value of 0.10 and frees under 0.05. A click begins at 0.95 and ends under 0.80. During a held click the cursor follows the ray offset by the pin. It applies to the game UI, the map, and the debug panel. Nothing more to do but test it. One thing to check in the headset: on the Index the trigger's capacitive rest can read a few percent, which is below the 0.10 pin, but worth watching.

## 6. Shadows away from their objects (441.91 s)

What happened: at t 440 to 442.5 the player was wingsuiting at 30 to 38 m/s over a road with a car on it, perhaps 50 to 100 m away (`shadow_sheet.jpg`). The shadow appears displaced from the car, as on 10 November at t 2331 during a fast reel. The session never flipped `stereo.shadow_anchor_to_fit` (on by default) or `stereo.shadow_fit_unpadded`.

The previous hypothesis is in `docs/mod/stereo/lighting-shadow-vr-interactions.md`, "The shadow lookup's anchor": a gap between the cascade fit's camera and the render camera shifts every shadow by the gap, in proportion to speed. That fix was in this build and the symptom remains, so the gap is either not the cause or not the only one.

A better-fitting candidate is in the same document, section C, "Cascade update-pattern amortization": cascade level L re-fits and re-renders only every 2^L frames, and between refreshes its fit is copied forward. A distant car is in a far cascade. At 33 m/s and about 24 ms a frame, a cascade refreshed every 4 frames is about 3.2 m stale, a car length, which matches both reports, and it goes away when the player stops. The anchor correction applies one per-frame delta from the current fit position to all cascades, so it cannot correct a cascade whose fit is several frames old. Hypothesis: the lookup for an amortized cascade uses its stale transform against the current render camera, or its content shows a moving car where it was.

What a render trace should capture (Tools > Render capture with screenshots on, while moving fast past a car in sun, then the same standing still):
- Per frame: `ShadowManager::m_CascadeUpdateLevels` and which cascades actually re-fit or re-rendered that frame.
- Each cascade's transform translation, both as fitted and as staged in `m_ShadowCascades`.
- `StereoState::shadow_fit_position`, the render camera's position, and the anchor delta applied.
- The per-eye screenshots, to measure the shadow's offset against speed.

Then an A/B with `shadow_update_every_frame` on. If the offset vanishes with every cascade updating each frame, it is the amortization, and the fix is to correct each cascade's anchor by the camera's travel since that cascade's fit (store the fit position per cascade at its refresh), not to pay for every-frame updates.

## 7. Seated reloading, and the detonator and grenades seated (4147 s)

Gating found: the grenade and C4 pouches are placed only `if gameplay` (`controllers/weapons/begin/mod.rs:326`), and `gameplay()` (`:891`) is true only for `OnFoot(Ground)` and `Airborne(Falling | Parachute)`. So in any vehicle seat (and in the wingsuit and swimming) neither pouch is offered. At t 4143 to 4149 in the boat, every grip press offered only the shoulder and hip sockets (`targets=7`). That breaks the binding "Seated: belt and pouches available".

Fix: place the chest pouches from `Capabilities` rather than `gameplay` (add a `pouches` capability, true in `Mode::Vehicle`), keeping the existing rule that a hand holding something takes nothing from the chest. Also check that a grenade thrown or a charge placed from a seat leaves from the hand: unverified in game, and the moving-frame throw problem from 556.8 s (other area) will apply.

Reload seated: the magazine pouch already runs in vehicles (`weapons/begin/mod.rs:424` passes `capabilities.reload`, true for `Mode::Vehicle`), so the belt should already be offered. What is missing is a way to eject the magazine. The existing triggers are the reload button's tap and the gun hand's flick (`weapons/reload/eject.rs`), and the reload button is not on the vehicle action sets' layouts as on foot (I did not trace the binding). Binding: a double tap of the trigger drops the magazine. Add a `DoubleTap` detector beside `TapDetector` in `eject.rs` (two presses each shorter than about 0.2 s within about 0.4 s), active in `Mode::Vehicle` for the hand holding a gun, and queue the eject through `deferred::queue_eject` as the tap does.

Conflict: in the driver's seat the triggers are the pedals (and, with section 1, the bike's throttle and brake), and a driver's gun fires from the upper face button. So in the driver's seat the double tap must be on the button that fires the gun there, not the trigger. In passenger, gunner, and stunt seats the trigger works. The first tap of a double tap also fires a shot, which seems acceptable but should be stated.

## 8. Capture

### What causes the bleed-through

Today's capture records the desktop window (`docs/mod/capture.md`, gpu-screen-recorder on the host), so whatever composites under or over the mirror window at capture time gets in. That is a capture timing and compositing issue outside the game, and only in-process capture removes it.

### The old recorder (cd9be898)

From `render_engine_post_draw`, after the per-eye capture copies, a pixel shader (`i420_pack_ps.hlsl`) packed one eye into BT.709 4:2:0 in a single R8 texture, optionally scaled. A three-deep staging ring was mapped with `DO_NOT_WAIT`, two frames late, and a writer thread streamed y4m over loopback TCP to `scripts/capture_host.sh` (ffmpeg with a hardware H.264 encoder or libx264, ten-minute mkv segments, audio from the Pulse monitor). It was replaced by host-side capture in 3be0139e without a stated reason. The fix just before it (fd593048) shows the pain: full-resolution frames of about 19 MB stalling the socket while the encoder started. The GPU pack and the no-wait staging ring are still the right front end; what was missing was downscaling.

### (a) Encoders

- Media Foundation through the `windows` crate (sink writer, hardware MFT, `IMFDXGIDeviceManager`) is the best Windows path.
  - One API covers NVIDIA (`nvEncMFTH264x.dll` ships with the driver), AMD, and Intel, all with D3D11 textures in (`MF_SA_D3D11_AWARE`), so nothing is read back. AAC and MP4 muxing are built in. No licensing cost and no binary size. Use a second device on the same adapter with shared textures and a keyed mutex, not the game's immediate context.
  - Write fragmented MP4 or short segments, because the known WaveWorks crash would leave a plain MP4 unfinalised.
  - Under Wine it is unusable: winegstreamer's H.264 encoder MFT takes CPU samples only and stubs every `ICodecAPI` setter (`E_NOTIMPL`), Wine's sink writer does not encode (`/* FIXME: call the encoder */` in `mfreadwrite/writer.c`), and there is no AAC encoder.
- NVENC directly (nvidia-video-codec-sdk and similar): zero copy on Windows but NVIDIA only, and the crates lean towards CUDA. Under Proton there is no nvencodeapi bridge: wine-staging's patch was disabled for years and removed in February 2026, and Proton's nvcuda is a stub. A DXVK device is not a driver D3D11 device either. Not worth it.
- ffmpeg-sidecar (spawn ffmpeg.exe, pipe NV12): easy on Windows with h264_nvenc, AMF, or QSV, at the cost of a readback, a 64 to 140 MiB GPL executable shipped separately, and audio over a second pipe. Under Proton, ffmpeg.exe has no hardware encoders, libx264 at these rates next to the game is too expensive, and Win32 pipes into Linux children are unsupported (Wine bug 54099).
- ffmpeg-next: the most control, but cross-compiling with xwin, LGPL DLL obligations (GPL with x264), tens of MB, and still no hardware encoding under Wine. No.
- openh264: CPU only, Constrained Baseline, and Cisco's patent cover only for its downloaded binary. A last-resort fallback at most.
- Recommendation, layered:
  1. A shared GPU front end that downscales, places the eyes side by side, and converts to NV12 in one pass (the old pack shader with a scale).
  2. On Windows, the MF sink writer with a hardware MFT on shared textures, falling back to Microsoft's software H.264 with a readback.
  3. Under Wine (detect `ntdll!wine_get_version`), the old no-wait staging ring, then loopback TCP to a host-side ffmpeg with NVENC or VAAPI writing mkv segments. That is the existing relay, fed frames 3 to 12 MB instead of 19.

### (b) WASAPI loopback under Wine

- Real loopback for winepulse landed in Wine 9.12 (commit 0bb23c2c91, "winepulse.drv: Implement pulse_get_loopback_capture_device()", June 2024); the ALSA, OSS, and CoreAudio backends still FIXME. Proton 9 (Wine 9.0 base) likely lacks it unless backported, and Proton 10 should have it. Both are unverified.
- It records the default sink's monitor, so loopback brings the desktop's audio bleed back. Windows' per-process loopback (`AUDIOCLIENT_ACTIVATION_TYPE_PROCESS_LOOPBACK`) is not implemented in Wine's mmdevapi.
- On Windows the `wasapi` crate's loopback also picks up other applications.
- Recommendation: capture the game's own mix in process by hooking `IAudioRenderClient::GetBuffer`/`ReleaseBuffer` on the game's stream (format from `IAudioClient::Initialize`). That works on Windows and under Wine (FAudio and xaudio2 end in mmdevapi), carries game audio only, and runs on the game's clock. Check in the IDB whether JC3's Wwise reaches WASAPI directly. The host-side alternative under Linux is `pw-record` on the game's own sink input rather than the default monitor.

### (c) Eye resolution and the side-by-side size

The mod renders each eye at the runtime's recommended size × `vr.resolution_scale` (1.0 by default), with the width rounded up to 64 (`vr/eye_resolution.rs`). Both logged sessions created a 3584×3561 per-eye swapchain, which is the Bigscreen Beyond 2e (2560² panel; the runtime's recommendation is about 1.4×). It is not the Index: the session used Index controllers. Figures for the headsets asked about:

| Headset | Per-eye render (runtime recommendation at 100%) | Aspect w/h | Eye at 1920 wide | Side by side |
|---|---|---|---|---|
| Valve Index (SteamVR or Monado steamvr_lh) | 2016×2240 (1440×1600 panel ×1.4) | 0.90 | 1920×2133 | 3840×2133 |
| Quest 3 (WiVRn) | about 2064×2208 (panel; WiVRn's recommendation follows its scale setting) | 0.935 | 1920×2054 | 3840×2054 |
| Quest 2 (WiVRn) | about 1832×1920 | 0.95 | 1920×2012 | 3840×2012 |
| Beyond 2e (this session) | 3584×3561 | 1.006 | 1920×1908 | 3840×1908 |

The reviewer's choice ("1920 wide or 1080 tall, whichever gives more surface") comes out at 1920 wide for every headset here. 1080 tall would give an eye about 1000 px wide, which cannot hold today's 1920×1080 single-eye crop. Today's mirror scales the eye to the 1920 px window width and cuts the top and bottom, so a 1920-wide eye holds exactly today's crop plus the 830 to 1050 px of height the window cuts.

Sizes and bandwidth (NV12 is 1.5 bytes a pixel; the bitrate is scaled from today's 10 Mbps HEVC at 1080p60, about 0.08 bits per pixel; H.264 needs about 1.5 to 2× that):

| Stream | Pixels | NV12 per frame | Raw at 30 fps | Raw at 60 fps | HEVC 30 / 60 fps | H.264 30 / 60 fps |
|---|---|---|---|---|---|---|
| One eye, 1920×2133 (Index) | 4.10 M | 6.1 MB | 184 MB/s | 369 MB/s | ~10 / ~20 Mbps | ~16 / ~30 Mbps |
| Side by side, 3840×2133 (Index) | 8.19 M | 12.3 MB | 369 MB/s | 737 MB/s | ~20 / ~40 Mbps | ~30 / ~60 Mbps |
| Side by side, 3840×2054 (Quest 3) | 7.89 M | 11.8 MB | 355 MB/s | 710 MB/s | ~19 / ~38 Mbps | ~29 / ~57 Mbps |
| Side by side, 3840×1908 (Beyond) | 7.33 M | 11.0 MB | 330 MB/s | 660 MB/s | ~18 / ~35 Mbps | ~27 / ~53 Mbps |
| Today's single crop, 1920×1080 | 2.07 M | 3.1 MB | 93 MB/s | 187 MB/s | ~5 / ~10 Mbps | ~8 / ~16 Mbps |

Notes:
- 3840×2133 is 8.19 M luma samples, inside H.264 level 5.1 and 5.2's 8,912,896 frame limit, and 491 M samples a second at 60 fps, inside level 5.2. NVENC's H.264 width limit is 4096, so the side by side fits; HEVC has headroom.
- The game ran at about 41 fps, so encoding at the game's frame rate or a fixed 30 is the realistic choice. At 20 Mbps, an hour is about 9 GB.
- On the Wine readback path, 330 to 370 MB/s at 30 fps through a loopback socket is the cost to watch; it is the old recorder's failure mode at a quarter of the old frame size. If it is too much, a fallback is the full-width left eye plus the right eye at half size (2880×2133, 9.2 MB a frame).

## Related items from other areas, briefly

- 532.5 s, the yanked gunner falling through the jeep: moving reference frame and ragdoll (grabs area).
- 1742.6 s and the vehicle entry swings: covered in world.md's "the entry and exit animations still whip the view"; the binding's 0.5 s yaw steps apply.
- 1121 s, the clip release doing nothing: guns area. The seated eject above is separate.
