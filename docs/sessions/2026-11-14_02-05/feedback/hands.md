# Hands, guns, inventory, grabs, and props: 14 November 02:05 feedback analysis

Written on 15 November from the take and the code as they were before the fixes listed in the [README](../README.md#the-feedback-item-by-item); file and line references are to that code. Scripts, dumps, and frame sheets it names were working files of the analysis and are not kept.

Times are the Rerun `recording` timeline (t); video time is t − 1.864 s. Each part below was verified against the session log, tracking dumps, and video frames, with IDA checks where named; per-part scratch files (frames, dumps) are in the `hands/` scratch subfolders. The parts were written separately and are joined here unchanged except for heading levels.

## Verdicts at a glance

- 115 s, primed grenade has no trail or sound: confirmed. The game's trail start runs, but the trail effect and sound are only assigned by the weapon's throw, never at priming. Fix: assign them at the pin pull from the weapon's player trail effect and sound (`start_trail`, grenade/native.rs:226). Needs `Explosive::SetupEffects` and four weapon trail fields bound in pyxis. No early projectile spawn needed.
- 478 s, C4 trigger should detonate at once: the trigger swaps the charge for the detonator (c4/mod.rs:422-437) and only the next press detonates (c4/detonator.rs:79). Fix: call `press_detonator` straight from the held charge's trigger and keep the charge in hand. The empty-pouch case (no charge to hold) is open.
- 489.5 s, upside-down detonator: the event is at t 479.0-479.7. The detonator that flashes upside down is the game's authored one, from its detonate animation. The mod's copy is the small cap that stays in the fist. So the reviewer has the two swapped, but his conclusion holds: drop the mod's copy and place the game's in the hand (attachment hook, c4/mod.rs:1003, some RE).
- Refused plantings lose the charge: the mod refuses whenever the game's animation-driven planting permission is off (falls, ragdolls, landings, a live wire), and any refusal at release despawns the charge (c4/stage.rs:239). Fix: ignore that permission for hand throws and never despawn silently.
- 574-578 s, ten phantom detonations: the player was already dead (574.83). `press_detonator` (c4/mod.rs:649) does not check the detonate permission it already reads. Fix: gate on permission and death, give refusals their own buzz, log at info, put the detonator away on death.
- 4147 s, no pouches seated: the pouches, grenades, and C4 are gated on a gameplay-mode check that excludes vehicles (weapons/begin/mod.rs:326/348/389/402/891, finish.rs:294/335). This conflicts with the binding answer. Fix: add a pouch capability that is true in vehicles.
- 510-519 s, hands snapping: not the stunt position (no stunt or IK transitions in the window). On the moving car, frames without a sim tick lead the hand targets by one extra frame of the car's travel. The force-capped rifle hand lags and is drawn at its body, so it jumps. Cause: physics/step.rs:146-150, :255, `Target::led` (physics/state.rs:203). Fix: lead the platform part by ticks elapsed, not by wall time.
- 388.682 s, popping object: the holstered weapons (the right-shoulder rifle and the left-hip pistol). In wingsuit flight at ~47 m/s they draw up to a tick ahead on no-tick frames. The object is identified; the mechanism is a hypothesis that needs a diagnostic and RE of the game's wingsuit weapon placement.
- 1121.42 s, clip release does nothing: every drop request (the 21 button taps and the 9 belt pushes) was refused inside `queue_eject`, and `eject()` ignores the refusal (reload/steps.rs:523). The belt path latches the gun as "in the belt pouch" anyway (steps.rs:845-856), which blocks the trigger. Which refusal fired is a hypothesis (no spare magazine prop, or stale records). Fix: unlatch on refusal, log and buzz every refusal, and allow a drop without a prop.
- 1886-1891 s, knock-out: mostly tuning. Speed is sampled on one tick at contact, which reads 55-70% of the hand's peak. Three strikes that peaked at 1.2-1.8 m/s logged 0.84-0.99. Fix: use the peak over ~100 ms with a 0.8 m/s threshold (reload config.rs:129). The 0.2-0.5 m/s pushes were seat attempts with the old magazine still in, which is technique.
- Seated reloading: allowed, but there is no seated eject input, and the one attempt hit the 1121 bug. A raw double tap can't be told apart from two semi-auto shots. The proposal restricts it to dry guns or a raised muzzle, which narrows the binding answer; the reviewer should confirm.
- 1629 s, full grenade pouch: the refusal buzz did fire, but it is the generic one and went unnoticed. Fix: a red pouch ring before release, a distinct double buzz, and a sound.
- Lockers: spare magazines are the game's ammo reserve. No reserve rise shows at any locker visit. How the game refills at a locker is unverified, and the suspect is the mod zeroing rack weapons' stored rounds. A log or RE check is needed first.
- 1653 s, dual-wield duplicate: the second gun is already spawned but kept hidden until the first take (hold/rack/paired/finite.rs:176, 490-503). Fix: show both stacked once the copy is ready.
- 646 s, left pistol: it was not lost at respawn. The right pistol fell at 291.22 (hip release too fast). At death the game holstered the remaining left pistol on the right hip, because loadout/live/hands.rs:61 ignores the hand's side. Holsters do carry across respawn. Lone-pistol replication fits in dual_wield/single/grant.rs:249-288.
- Grab circle placement (direct question): on foot, the hips follow the thigh holster bones (children of the drawn `LeftUpLeg`/`RightUpLeg`), and the pouches follow the drawn pelvis and chest. They are never placed from the capsule or the head. Seated, the hips are derived from the live head (weapons/docks.rs:398-447), which explains 2362.54 and 3894. At 2044 body yaw was 5-21° off while standing still; at 2560 the hand was released too high.
- Body yaw aligned with movement (the reviewer's proposal): it doesn't fit the observed cases (stationary or seated) and conflicts with the idle-IK binding. Fix the seated hip derivation and the idle reach yaw instead.
- 3894 s, SMG put away: it docked on the right hip, which seated was ~0.5 m off the thigh, so it vanished from view.
- 2526 s and 844-865 s, weapons flying off: the fist holding the grapple line counted as empty (grab/latch.rs:79-99, bindings/tables/mod.rs:350-358). It drew a holstered gun mid-swing and threw it when the line was released. The Hydra at 860 fell to the animation-refusal drop. Nothing left a socket on its own.
- Debug panel dropped the SMG (4380): opening the panel runs a full `weapons::stand_down` (bindings/mod.rs:1221, weapons/state.rs:46-87) instead of pausing.
- Harvest problems 1-2 (stows dropping): the release retry (finish.rs:374-398, hold/dock.rs:55-57, act.rs:316-321) drops a weapon the animation refuses and cancels attractions already in flight. That covers 17 of the 18 failed stows. What forbids holstering needs RE.
- Ragdoll and prop grips spasm or are lost: the joined drive (physics/drive.rs:184) closes the whole error in one step, sized for the ~7 kg gripped bone, while ~93 kg of undamped ragdoll hangs off it. Two hands add two fixed joints that fight the joint limits. The held bone orbits the palm at 2-4 Hz even one-handed while standing still. The propane tank fails the same way. Topples fail silently on allies and on tripod gunners, and the game's get-up ends a hold after ~6 s.
- 3238 / 3265 s, hitting people does nothing: a swung gun is classed as trying to take the target's gun and never strikes (impacts hit.rs:85-92; one gun blow in the whole take). This is a code bug. For props, the supplemental strike is skipped whenever the game's `DoReactToHit` fires (impacts/mod.rs:492), which is likely but unconfirmed (debug-level logging).
- 532.5 s, gunner falls through the vehicle: a collision exclusion, not the moving frame. On the vehicle stunt position the player shares the jeep's collision group, so carry_collision.rs:104-118 counts the jeep's bodies as the player's and disables ragdoll-versus-jeep collision.
- 556.8 / 560.57 s, C4 lands behind: the vehicle velocity is added, but it is sampled from the camera every render frame while the camera moves only on ticks. It reads 0 or about 2x the real speed (hold/act.rs:847, release_diagnostics.rs:66-101). The 15 m/s throw cap also clamps the vehicle's share. Grenades share the path.
- 1438.8-1442 s, barrel through the body: as far as the defs and IDA show, Rico has no physics bodies for his limbs while animated (only the character proxy). The binding needs mod-owned keyframed capsules on the drawn bones, on a layer held props collide with, in the player's group so the capsule and hands ignore them. Some IDA work remains.
- 3715.6-3718.2 s, gun stutter: the hand-and-gun body oscillates while moving, then settles. Likely the same drive problem plus the once-per-tick render offset reading stale between ticks.

## Cross-cutting findings

One defect class shows up in four items: values sampled per rendered frame against state that advances only on sim ticks. It covers the hand lead on a moving car (510-519), the platform velocity added to throws (556.8), the holstered guns drawn ahead in flight (388.682), and probably the gun stutter (3715). One shared "platform motion per tick" source, used by the physics lead, the throw velocity, and render-time placement of attachments, would fix all four together.

A second class is refusals that act silently or destructively: refused plantings despawn the charge, refused magazine drops latch the belt and block the trigger, refused stows drop the gun, a full pouch drops the grenade with a generic buzz, detonations are logged when the game refused them, and topples fail on allies while logged as thrown. Each needs a "keep in hand" fallback where one makes sense, a distinct haptic, and an info-level log line naming the refusal.

A third is the line-holding fist being treated as an empty hand (2526, 844-865). This interacts with the firm-ragdoll swing binding: the other hand must still reach holsters mid-swing, so block only the line hand.

## Conflicts with the binding answers or between items

- Seated pouches (binding) are blocked by the vehicle exclusion in `begin::gameplay`. This one root cause covers 4147 and the seated-reload item. The refused-planting fix is a prerequisite for C4 while seated.
- The seated double-tap drop (binding) collides with semi-auto fire. The proposed narrowing (dry gun or raised muzzle) needs the reviewer's sign-off.
- The C4 binding doesn't say what the trigger does with the pouch empty and no charge in hand. The proposal: a grab at the pouch detonates on the spot.
- Keeping body yaw aligned with movement conflicts with the idle-IK binding and wouldn't fix the observed misses. The body area should own the idle reach yaw.
- The seated hips derived from the head contradict "align Rico, don't decouple" and the seated-pouch binding.
- The held-prop body collision (binding) needs body proxies that don't exist yet. The same proxies would keep held ragdolls out of the camera, so coordinate with the body area.
- Docs drift: docs/mod/input/held-prop-impacts.md still says the observer deals no reaction (`be62f8fa` added the supplemental strike). character-grabs.md's "unverified in the headset" can now be settled (get-up after ~6 s, silent topple failures).

Related items in other areas, briefly: the gunner-seat grips not responding (536-550, vehicles) may share the same-collision-group effect found at 532.5. The reviewer's 1735/3395/4055 mounted-gun tear issues are in world.md's scope.


## Grenades and C4: 14 November 02:05 feedback analysis

Times are t (Rerun recording timeline); video time is t − 1.864. Frame strips are in the analysis scratch folder (not kept). RE was done against the release IDB with the symbol dump as a locator; nothing was written to the pyxis defs.

### 1. A primed grenade in the hand has no trail or sound (115 s)

What happened. Confirmed from the log: all 14 `grenade: the fuse's trail is lit` lines report `effect=false sound=false sound_follows_fuse=false`. The harvest's frames and spectrograms (guns.md) show no spark or smoke on the held grenade and no fuse beeping until 0.25 to 0.3 s after the throw, when the launch's trail starts.

Root cause (verified in the IDB). `Explosive::PlayTrailEffect` plays whatever is in `Explosive::m_TrailEffect` and `Explosive::m_TrailSound`, and skips each half whose resource is null. For a hand grenade those two shared pointers are filled only by the weapon's launch. `ThrownGrenadeWeapon`'s `Fire_Projectile` override (IDA `CThrownGrenadeWeapon::Fire_Projectile`) does, in order: set the initial target, the glass and homing upgrade trails (`CGrenade::SetupGlassEffects` and `SetupHomingEffects`, only with those upgrades), `Explosive::SetPhysicsTransform`, then `CPhysicalProjectileWeapon::GetTrailSound` and `GetTrailEffect` into `CExplosive::SetupEffects` (two shared-pointer assignments into `m_TrailEffect` and `m_TrailSound`), then the virtual `PropelToward`, whose base calls `PlayTrailEffect`. `ThrownGrenadeWeapon::PrimeGrenade` never calls `SetupEffects`; the xrefs to `SetupEffects` are the grenade launch, `PlantedExplosiveWeapon::PrimeExplosive` (which is why the C4 has its trail from the prime), and the balloon and Namaste weapons. So when `grenade::native::start_trail` (payload/src/controllers/weapons/grenade/native.rs:226) calls `PlayTrailEffect` at the pin (native.rs:210), both resources are still null and nothing plays. The doc comment on `start_trail` and the design doc (docs/mod/input/grenades.md, "A grenade whose pin is out looks and sounds live in the hand") assume otherwise.

`GetTrailEffect` and `GetTrailSound` pick between a player pair and an AI pair of shared pointers on the weapon by `WeaponBase::m_IsUserPlayer`; the player pair sits right after `ThrownGrenadeWeapon::m_PhysicalProjectileModel` in the physical-projectile part of the weapon (trail effect, then trail sound, then the AI effect and sound). None of these are in the defs yet.

Cheapest fix. Fill the two resources from the weapon before the pin's `PlayTrailEffect`, exactly as the launch does, and keep everything else as is:

- pyxis-defs, equipment/explosive.pyxis: add `Explosive::SetupEffects(&mut self, trail_effect: *const SharedPtr<void>, trail_sound: *const SharedPtr<void>)` (shared-pointer assignment of both; takes references, so no temporaries to release).
- pyxis-defs, equipment/weapon.pyxis on `ThrownGrenadeWeapon` (or a `PhysicalProjectileWeapon` base region if one is wanted): the four shared-pointer fields `m_TrailEffectPlayer`, `m_TrailSoundPlayer`, `m_TrailEffectAi`, `m_TrailSoundAi` (dump names `m_TrailEffectPlayer` etc.), and optionally `GetTrailEffect`/`GetTrailSound`. Passing the field addresses straight to `SetupEffects` avoids handling the returned shared pointers' reference counts in Rust.
- payload, grenade/native.rs `start_trail`: take the weapon (the caller `pull_pin` needs `&ThrownGrenadeWeapon`, which `safe.rs` already has at its call site), call `explosive.SetupEffects(&weapon.m_TrailEffectPlayer, &weapon.m_TrailSoundPlayer)` (choosing the pair by `m_IsUserPlayer`, which is always the player here), then `PlayTrailEffect`. The launch later re-assigns the same resources and restarts the trail, which the engine allows.

Side benefits: an inert release of a live grenade (the fallback when `launch_live` is refused, e.g. without `Inventory::m_DoesAnimationAllowGrenades`, native.rs:559) keeps a trail, which today it never gets. Risks to check in a session: the trail and its sound are moved to `Explosive::m_WorldMatrixT1` only in the post-simulation update (so they follow the tick's grip, not the render-time hand; a small lag is likely visible on fast hand moves), and the trail effect's speed/velocity parameters will read a stationary body. Spawning the real projectile early is not needed and would not help: the item already is a real pooled `Grenade`; it simply never got its resources. The upgrade trails (glass, homing) could be added the same way later via `CGrenade::SetupGlassEffects`/`SetupHomingEffects` (in the IDB, not in the defs), but they are upgrade cosmetics. docs/engine/gameplay/aim-pipeline.md ("The burning fuse") should say the launch is what sets the resources.

### 2. C4: the trigger on a held charge should detonate at once (478 s)

What happened. 478.079 `c4: the trigger swapped the charge for the detonator`, the charge leaves (`the object is gone`, it went back to the pouch), and the detonation is only on the second press at 478.868. Frames at t 478.36 to 478.86 show the mod's detonator in the hand in place of the charge.

Current flow, with code:

- payload/src/controllers/weapons/c4/mod.rs:422-437: in `input_tick`, a trigger press on `Stage::Held { release: None }` folds `InputEvent::Return` (the charge is cancelled into the pouch) and calls `Detonator::take`.
- c4/detonator.rs:79-84: `take` arms the detonator only if the trigger is up, so the press that swapped cannot detonate; `Detonator::step` (detonator.rs:97-118) needs a release and a new press.
- c4/mod.rs:406-407 then `State::press_detonator` (mod.rs:649-669) queues `PendingDetonation`, which the serial update raises as `WeaponInput::E_INPUT_DETONATE_EXPLOSIVES` (mod.rs:906-910).
- An empty pouch hands out the detonator (mod.rs:413-420), and the pouch is offered while the detonator is not held (`pouch_offered`, mod.rs:219).

What must change for the binding answer ("with a charge in hand, the trigger detonates all placed charges at once, detonator shown briefly then hidden, the charge stays in hand"):

- mod.rs:422-437: replace the swap with a direct call of `press_detonator(hand)` (with the gates from item 5), leaving `state.stage` as `Held` so the charge stays in the hand. Edge-trigger it on `pressed` so a held trigger does not re-queue; the existing in-flight and pending logic already prevents a second request during the 1.2 s chain. The log line becomes "the trigger detonates the placed charges with a charge in hand".
- The "detonator shown briefly" part comes for free from the game: the detonate act shows the weapon's own trigger model (see item 3). The mod's owned held detonator (`detonator::place_model`, `render_frame`, `OwnedModel`) is then not needed for this path.
- The empty-pouch path needs a decision the binding does not cover: with every charge placed there is no charge to hold. Options: keep the held detonator for that case only (drawn with the authored model rather than the owned copy), or make a grab at the empty pouch detonate on the spot. I would take the second; it removes `Detonator` and the owned model entirely and matches "no intermediate step". Flag for the user.
- Update `C4Status::swapped`, the debug readout, the module docs (mod.rs:38-49, detonator.rs header), docs/mod/input (the C4 doc), and the tests in detonator.rs and c4/input_tests.rs.
- The extra presses during the chain (41 in the take, each an "empty" buzz) want a distinct "already going off" haptic rather than `DetonatorEmpty`; that is item 5's feedback too.

### 3. The second detonator: upside down, appears and vanishes (489.5 s)

What happened. Nothing C4-related happens at 489.5: the log has no C4 line between 486.76 and 494, and frames at t 488.9 to 490.4 show the player ragdolling after `ACT_HITREACT_FLY` (487.79) with a primed charge (#13, taken 486.57) in the right hand, and no detonator. The description matches t 479.0 to 479.7 exactly, so I believe the reviewer meant 479.5 (hypothesis, but strong). Frames at 0.2 s steps from t 478.46:

- t 478.46 to 478.86: a small blue cap sticks out of the pinky side of the right fist, the rest hidden in the hand. This persists from the swap (478.08) to the return at 486.24 (also visible at t 479.86 to 481.9), so it is the mod's owned detonator at `DETONATOR_HOLD` (detonator.rs:124).
- t 479.06 to 479.66: a second, large blue cylinder hangs below the fist with its red end at the bottom, then it is gone at 479.86 while the small cap remains. It appears about 0.2 s after the detonation press (478.868) and lasts about 0.7 s, which is the detonate act: the game shows the C4 weapon's trigger model (`WeaponData::m_BaseModelInstance`) in Rico's hand while the act plays, placed by `WeaponData::UpdateAttachmentTransforms` at the attach bone the act's weapon state names. Rico's hand bone is put on the controller, but the attach bone's pose relative to the wrist is the animation's, which grips the detonator thumb-on-button; with the VR hand's orientation that renders upside down.

So the reviewer has the two the wrong way round: the one that "appears and disappears, upside down" is the authored (game's) detonator, and the one "in the wrong place" is the mod's owned copy. The conclusion still holds and fits item 2: drop the mod's owned detonator and keep the game's, which already shows briefly and hides by itself.

Positioning the authored one in the hand. The C4 weapon's attachment update is already detoured (`update_attachment_transforms`, c4/mod.rs:1003-1044, used for the held charge). Extend it: after the original, while the detonate act runs (`Character::IsDetonatingExplosives` on the user, or equivalently while the trigger model is shown), overwrite the first gun's `WeaponData::m_WorldMatrixT1` (and `m_WorldMatrixT0` on the first frame, to avoid a swoop) with the pressing hand's grip composed with a hold like `DETONATOR_HOLD`, and push the render-time pose in the render frame as `detonator::render_frame` does today for the owned model (retarget it at `m_BaseModelInstance`, or set `WeaponData::m_SkipRenderInterpolation`). If the charge is in the right hand, decide whether the detonator goes in the same hand (offset so the two do not overlap) or the free hand; the binding says nothing, I would use the free hand when it is empty and the pressing hand otherwise. RE needed: which `WeaponState` the act uses and how the act shows and hides the base model (track message or state change), so the override is keyed on the real condition rather than on `IsDetonatingExplosives` alone; verify also that `DETONATOR_HOLD`'s axes match the base model's (the owned copy sits mostly inside the fist, so the derived hold is itself off).

### 4. Refused plantings lose the charge (906.38, 1939.30, 2100.35; pouch refusals 1075.58, 1532.36, 2933.90)

What happened. Each of the three charge losses is a release (906.38 thrown, 1939.29 thrown, 2100.35 pressed onto a surface) logged `c4 pouch outcome=refused: planting is not allowed now`, then `the object is gone` 20 ms later. The body state at each: 906.18 the grapple lock released and the player is in "a fall past the jump's grace"; 1939.08 to 1939.16 ragdoll then climbing; 2100.27 ragdoll then jump. The three pouch refusals are all in a fall off a vehicle roof: 1074.57 "standing on a vehicle" to "a fall", press at 1075.58 (landing at 1075.49, so in the landing); 1531.82 fall, press 1532.36 in the air; 2933.03 fall, press 2933.90 in the air. The "stunt clip modifiers restored" line is just the mod noticing the player left the roof; it is not the cause.

Root cause. `read_weapon` (c4/mod.rs:1076-1104) turns `Inventory::GetAnimationAllowsPlantingExplosives` into `Refusal::NotAllowed` (mod.rs:1085). That permission is `Inventory::m_DoesAnimationAllowPlantingExplosives` gated by the `SB_DISABLE_PLANT_EXPLOSIVES` state bit, which the animation graph's `NSetInventoryPermissions` tasks set per frame in falls, landings, ragdolls, hit reactions, and similar states, and `Inventory::UpdateInventoryPermissions` also clears every permission while the grappling hook has a live wire. It exists to keep the game's planting animation out of those states; the hand-held throw needs no animation. Then:

- At a release, `stage::before_update` (c4/stage.rs:239-243) maps any refusal on a released, physics-ready charge to `WeaponAction::Cancel`, i.e. `PlantedExplosiveWeapon::DespawnPrimedExplosive`. The charge vanishes from the open hand. Priming consumes nothing, so the stock is not lost, but the throw is, silently (a `PouchRefused` buzz only), and the next detonator press finds nothing placed (1946.6, 2106.8).
- At a pouch press, `Stage::Wanted` (stage.rs:219-231) refuses with the same reading.

Fix.

- Stop gating the hand on the animation half. In `read_weapon`, refuse `NotAllowed` only for the persistent reasons: the ability not unlocked (the player-without-ability case already folds into `m_DoesAnimationAllowPlantingExplosives`, so this needs a separate check; RE: find the ability test `UpdateInventoryPermissions` uses, or the profile flag) and scripted disables. Since the disable bits are per-frame requests from both the graph and scripts, they cannot be told apart cheaply; the pragmatic option is to ignore the permission entirely for the hand path and keep only `NoRoom` and `Busy`, as the grenade's draw already does (the grenade gates only its launch on `m_DoesAnimationAllowGrenades`, with an inert fallback). Mission-scripted no-explosive zones would then need their own check if they matter; flag.
- Never cancel a released charge for a permission. In stage.rs:239, keep the cancel only for `NoRoom` (a charge beyond `PlantedExplosiveWeapon::MaxActiveExplosives` would be out of the live list), and even then prefer a visible outcome: leave the charge in the hand (`Stage::Held { release: None }`) with a refusal buzz, so the player sees it was not thrown. For anything else, throw.
- `PlantedExplosiveWeapon::Fire_Projectile` itself has no permission check (it only removes a charge whose parent has an active bavarium shield), and the parachute path already proves it works off the planting animation, so the throw is safe in a fall.
- The 1939.3 case (hand inside a parked jet's fuselage) would then throw the charge from inside the jet's collision; check that `stick` and the release do not tunnel it, but that is a separate concern.

### 5. Ten detonations that did nothing (574 to 578)

What happened. 574.295 the empty pouch gave the detonator. 574.833 the body went to "dead". Every press from 575.015 to 578.000 (ten, 0.3 s apart) logged `c4: the detonator detonates charges=3`; none was followed by the "going off" gate, frames 575.4 to 577.2 show the jeep and its charges intact, and the respawn starts at 579.65. The player was already dead at the first press.

Root cause. `press_detonator` (c4/mod.rs:649-669) checks only that charges are placed and none are in flight; it does not consult the game's permission, which it already reads into `C4Status::may_detonate` (mod.rs:1084, 838). The game's serial update (`PlantedExplosiveWeapon::UpdateFromUserPostSim_Serial`) starts the detonate act only while `Inventory::GetAnimationAllowsTriggeringExplosives` holds, the character is not already detonating (`Character::IsDetonatingExplosives`), and `m_AlreadyDetonatedOnThisInput` is clear, and only "when the character can take" the upper-body act. Dead (and in ragdoll, hit reactions, and with a live grapple wire, per `UpdateInventoryPermissions`), the raised input does nothing. The request then expires after `DETONATION_TIMEOUT_S` (mod.rs:558) and `settle_detonation` logs the failure at debug level only (mod.rs:992), so the info log claims a detonation every time, and because nothing started, `in_flight` never gates the next press. Hypothesis: whether the inventory still ran the weapon's serial update while dead is not visible in the info log; either way nothing could start.

Fix.

- Yes, the mod should check the gate. In `press_detonator`, refuse when the last weapon update read `may_detonate == false` or the player is dead, with a distinct buzz and an info line ("the game does not allow detonating now"). Make the reading's freshness explicit (store the tick it was read in) so a stale `true` from before death does not pass.
- Optionally keep the request alive while the permission is off but transient (e.g. a hit reaction), up to the existing timeout, so a press during a brief stagger still fires once it ends; a dead player should cancel outright.
- Log the request's outcome at info: started, or expired with the reason (permission off, act refused). Today's debug-only line hid the failure.
- Stand the C4 down on death (the detonator stayed out until 578.32), e.g. by feeding a dead/respawning flag into `acting`.
- Related: the game's gate also refuses during ragdolls and falls (item 4's states), so with item 2's direct detonation a press in a fall will be refused by the game; the user should feel that refusal rather than a click.

### 6. No C4 detonator or grenades while seated (4147 s)

What happened. Seated in the boat from 4140.98 ("seated in a vehicle", controls summary BOAT) to 4147.64. Four right-hand grip presses (4141.80, 4142.79, 4145.49, 4146.76) all logged `chosen="nothing in reach" others="none"` with only a shoulder or hip socket as the nearest miss: the pouches were not candidates at all. The C4 and grenade pouches were offered again as soon as the player stood on the boat (4149.16, detonator from the empty pouch; detonation at 4152.52).

Root cause. Pouch access is gated on `gameplay(mode)` (payload/src/controllers/weapons/begin/mod.rs:891-897), which is true only for `Mode::OnFoot(Ground)` and `Mode::Airborne(Falling | Parachute)`, so `Mode::Vehicle(_)` is out:

- begin/mod.rs:326: the grenade and C4 pouch targets are only built under `if gameplay`.
- begin/mod.rs:348: pouch returns likewise.
- begin/mod.rs:389 and 402: grenade and placed-charge `begin_tick` `acting: gameplay`.
- payload/src/controllers/weapons/finish.rs:294 and 335: `grenade::input_tick` and `c4::input_tick` `acting` also include `gameplay`; with `acting` false, `c4::input_tick` stands the throw down and releases the detonator (c4/mod.rs:398-402).

`Capabilities::of` (payload/src/controllers/weapons/capabilities.rs:28-52) already enables fire, transfer, dock, and reload for `Mode::Vehicle(_)`, so guns work seated; only the explosives use the narrower `gameplay`.

Fix.

- Add a `pouches` (or `explosives`) capability to `Capabilities`, true for the vehicle modes per the binding answer ("seated: belt and pouches available"), and use it at begin/mod.rs:326, 348, 389, 402 and finish.rs:294, 335 instead of `gameplay`. Keep `gameplay` for what genuinely needs feet (it is also stored in `state.gameplay`, begin/mod.rs:647; check its other readers). Decide whether wingsuit and swimming get pouches too; today they do not.
- The pouch volumes need a body frame while seated. `skeleton_in_cockpit` already gave one in the seat (the socket misses prove it), so the volumes should place; verify the pouch positions against the seated skeleton in the next session.
- Game-side gates seated: the grenade's launch requires `Inventory::m_DoesAnimationAllowGrenades` (grenade/native.rs:559) and otherwise releases inert with a running fuse, which works; the C4 prime would be refused by item 4's `NotAllowed` if seated animations clear the planting permission, so item 4's fix is a prerequisite. Detonation goes through the game's own detonate gate (item 5); RE or test whether `GetAnimationAllowsTriggeringExplosives` holds in a vehicle seat (the stick's `DETONATE_EXPLOSIVE` works in vehicles in the base game, which suggests yes, unverified). Also confirm that the inventory runs `PlantedExplosiveWeapon::UpdateFromUserPostSim_Serial` while seated (the defs say every frame whatever is wielded; not verified for vehicles).
- Releases from a moving vehicle need the vehicle's velocity in the throw, which is the separate 556.8/560.57 item (projectiles ignore the moving frame).

### Conflicts and cross-area notes

- Items 2 and 3 resolve together: detonate on the held charge's trigger, drop the owned detonator, place the authored one during the act. The empty-pouch case is the one open design question.
- Item 4's permission change is needed for item 6 (C4 seated) and changes how item 5 feels in falls: throwing is allowed, but the game will still refuse detonation in those states.
- The reviewer's reading of item 3 swaps the two detonators; his conclusion stands.
- The 1629 s "full grenade pouch rejects grenades silently" item belongs to the same feedback problem as item 4's silent cancel.

## Reloading, grenade pouch, lockers, and dual-wield racks: 14 November 02:05 take

Times are Rerun t. Sources: the session log (`log_annot.txt`), tracking-synth dumps of the take (controller frames, injected actions, haptics), video frames, and the code at the session's build. One IDA check was made against the release IDB. Scratch files are in `scratchpad/analysis/hands/reload/`.

### 1. 1121.42: the clip release does nothing, and the gun will not fire

#### What happened (verified)

The right pistol (#4, pistols slot, a single pistol of the dual-wield pair) was drawn from the right hip at 1107.67. The player was standing on a moving vehicle the whole time (`body_state: ... (standing on a vehicle)`, character velocity about 97 m/s, mode `on_foot`). From 1110.24 to 1118.80 five belt pushes each logged `the Right gun's belt push could not drop its magazine`, and each one was followed by `what holds the gun from firing changed ... now="the gun is in the belt pouch"`.

The clip release was pressed, and pressed a lot. The tracking shows 21 taps of the left lower face button between 1109.73 and 1122.57, each 0.08 to 0.19 s long, which is well inside `RELOAD.tap_s` (0.3 s). The tap at the reviewer's 1121.42 runs 1121.462 to 1121.593. The take's injected-action events confirm that the semantic `RELOAD` was clicked and applied (for example at 1109.78). None of these taps logged anything: no eject, no `magazine request refused`, and no `the reload button found no magazine to eject`.

The three trigger pulls at 1121.18, 1122.24, and 1122.46 logged `the trigger fired nothing: the gun is in the belt pouch`. The hold cleared at 1123.35, when the gun left the pouch sphere, and the pistol was docked at 1124.47.

The same failure recurs at 2161.57 and 2167.32 (the right pistol #4 again: at 2167.32 the left gun's eject and belt reload worked in the same push while the right gun's failed), at 3918.57 (SMG #101, just after leaving a helicopter), and at 4114.28 (SMG #101, armed in the right hand while seated in a boat).

#### Why the trigger was blocked (certain)

`BeltPush::step` (`reload/belt.rs`, `reach`) moves the push to `PushStage::Reloaded` and returns true *before* the caller tries the drop. In `steps.rs:845-856` (`State::belt`), a gun whose magazine is in calls `queue_eject(..., DetachKind::Belt, ...)`. When that returns false, it logs the line and `continue`s, so the stage stays `Reloaded`. `BeltPush::holds_fire` is true for `Reloaded` until the push point leaves the sphere, so `FireHold::BeltPouch` holds the trigger for as long as the gun stays near the belt. A failed drop therefore reads to the player as a dead gun.

#### Why the clip release did nothing (certain as to the mechanism)

The tap goes `steps.rs:476 ejects` → `eject::tap_target` → `steps.rs:523 eject` → `queue_eject`. The "found no magazine" line was never logged, so `ejectable_rounds` found a target: the gun's magazine identity was captured, the magazine was in, and no eject was pending. `eject()` discards `queue_eject`'s bool, so a refused queue is silent, with no log, haptic, or sound. The belt push and the button failed the same way at the same moments.

#### Root cause of the refused drop (hypothesis, ranked)

`queue_eject` (`reload/deferred.rs:27-66`) can only fail here at `serial::submit_with_retention` (`serial.rs:186-207`), because its other early exits are the same conditions `ejectable_rounds` had just passed. That function refuses in two ways, both logged only at debug level (`serial.rs:558`), and the session log has no debug lines:

- `Refusal::Admission` (the hand's mailbox `can_submit` fails, `serial.rs:444`): a slot left `Pending`, `Executing`, or `Complete` and never polled or cancelled.
- `Refusal::Retention` (`Retention::reserve`, `loose.rs:179`, which calls `Records::reserve_unheld`, `loose/records.rs:57-86`): this returns `None` when the gun's spare is null (`key.spare == 0`, line 63), when another record already carries the same spare pointer (lines 64-67), or when all 32 slots hold records with rounds.

The most likely of these is a gun with no spare magazine prop. `WeaponData::m_MagazineObject` is null after every throw until the spawn system's callback stores the next spare (the defs say so: `WeaponData::ThrowAwayMagazineObject` releases the reference, and `WeaponData::SpawnMagazineObject` fills it in asynchronously). The guns that failed are weapons the mod re-creates or moves around (a split single pistol, and an SMG respawned by the spawn reset at 3749.9), so a spare request that never completes, or completes into a gun the spawn callback no longer reaches, fits. The callback-side identity check also refuses a spareless gun (`serial.rs:256-260`, `has_spare`), so even the unretained path cannot drop it.

The second candidate is a leak in the records store. `Records::maintain` (`loose/records.rs:130-160`) only expires records whose body reads `Live`, with zero rounds, unleased and unheld. A record whose prop the game despawned stays `Unavailable` forever, keeping its spare pointer. A new spare allocated at that recycled address is then refused as a duplicate. Records with rounds never expire either.

A stuck mailbox slot is possible but less likely, because the same hand ejected normally before and after each failure run.

To settle which one it is, log the refusal at info, with the gun's spare pointer and whether a spawn is pending.

#### Fix

1. The belt (certain): add a `BeltPush::refuse()` that sets the stage to `Brushed` (the gun fires, and nothing happens until it leaves the sphere), and call it in `steps.rs` where `queue_eject` returns false. Add a test to `steps/belt_tests.rs` that a refused drop does not hold fire.
2. Make every refusal visible: give `queue_eject` a typed result (`Busy`, `NoIdentity`, `NoSpare`, `Duplicate`, `StoreFull`, `Admission`) instead of a bool, carrying the serial `Refusal`. Log it at info from both the belt and the button paths. Play `Event::PouchRefused` and `steps::refuse_sound` (the gun's dry-fire usage) on the gun hand. `eject()` at `steps.rs:523` must stop discarding the result.
3. The spareless gun: when `m_MagazineObject` is null, still drop the magazine as an unretained detach (set `m_WeaponIsWithoutMagazine` and leave the rounds in the native count, the `DroppedUnretained` outcome that already exists). That way the player can always reload, and only the visible prop is missing. Relax `has_spare` for `retain == false`. If no request is outstanding, re-request the spare with `WeaponData::SpawnMagazineObject`, which needs a pyxis-backed way to tell whether a spawn is pending. That is RE work: the spawn callback that stores the prop, and whether it is skipped for a holstered, armed, or carried weapon.
4. The records leak: in `Records::maintain`, drop records whose body is `Unavailable { identity_lost: true }` once they are older than `keep_s`, and age out records with rounds too (a magazine the game despawned is gone).

### 2. 1886-1891: struggling to knock the magazine out

#### What happened (verified)

Five meetings logged `too slowly to knock it out`, with logged closing speeds of 0.34, 0.25, 0.84, 0.84, and 0.29 m/s against `min_speed_mps=1.0`. The run ended with the magazine dropped by the button at 1893.58. The second struggle, 2113.89 to 2122.01, has seven: 0.03, 0.99, 0.81, 0.05, 0.97, 0.21, and 0.89.

I measured the left controller's peak speed relative to the right (gun) controller over the 250 ms before each meeting, from the tracking:

| t | logged closing | peak hand-relative speed |
|---|---|---|
| 1886.24 | 0.34 | 0.53 |
| 1886.63 | 0.25 | 0.22 |
| 1887.97 | 0.84 | 1.21 |
| 1888.16 | 0.84 | 1.21 |
| 1890.33 | 0.29 | 0.42 |
| 2114.51 | 0.99 | 1.82 |
| 2114.88 | 0.81 | 0.75 |
| 2119.73 | 0.97 | 1.57 |
| 2122.01 | 0.89 | 0.69 |
| successful knocks 2003.7, 2016.8, 2035.1, 4331.4, 4350.1 | 1.04 to 1.83 | 1.79 to 2.71 |

#### Verdict: mostly tuning, partly skill

The slow ones (0.2 to 0.5 m/s, peak at most 0.53) are the hand bringing the magazine to the well as if to seat it, with the old one still in. That is technique, and the guide could say so: the well ring is already red while occupied. The 0.8 to 1.0 ones are genuine strikes. Their hand peaked at 1.2 to 1.8 m/s, overlapping the successful knocks (1.8 to 2.7), but the logged speed is one input tick's travel at the moment of entry, projected on the capsule's normal (`knock.rs:312-356`, `strike`). That reads 55 to 70 percent of the hand's peak, because the hand decelerates into the contact. So a fixed 1.0 m/s on that measure turns away strikes that look and feel like strikes.

#### Fix

The threshold is `KnockConfig::min_speed_mps` = 1.0 at `payload/src/controllers/weapons/reload/config.rs:129`, used at `knocking.rs:28`, with the push variant `push_min_speed_mps` at `config.rs:130`. Two options:

- Measure the closing speed as the peak of the last three input ticks (about 100 ms), kept by `Knocks` beside `previous`, rather than the entry tick alone. Then keep a threshold around 0.9 m/s.
- Simpler: lower `min_speed_mps` to 0.7 m/s. Seating pushes measured 0.3 to 0.5 m/s at the mouth on 10 November, and 0.2 to 0.5 here, so 0.7 still separates them. Of this take's twelve refusals, 0.7 would have let through the seven at 0.81 to 0.99 and no slow seat attempt.

I recommend both: the peak window at 0.8 m/s. Update the doc comment at `config.rs:115-122` with this take's numbers.

### 3. Seated reloading (binding: belt and pouches available; a double tap on the trigger drops the magazine)

#### What blocks it now (verified from code and log)

- The reload itself runs in vehicles. `Capabilities::of(Mode::Vehicle(_)).reload` is true (`weapons/capabilities.rs:28-41`), and `reload::begin_tick` is passed `capabilities.reload` (`weapons/begin/mod.rs:424`), so the magazine pouch and the belt push are placed while seated.
- The drop failed. The one seated attempt, at 4114.28 in the boat (SMG #101 armed in the right hand, `reconcile: armed the weapon in the right hand`), hit item 1's refused drop. So the first blocker is item 1, not the vehicle.
- There is no eject input. The reload button is `OnFootAction::Reload` (`bindings/buttons.rs:12-25`), which is not in the vehicle action sets, so a seated tap ejects nothing. With the other hand on the wheel, there is no free hand to knock or pull either, so the belt push is the only route, and it needs the old magazine dropped first.
- The explosive pouches are off while seated: `begin::gameplay` (`weapons/begin/mod.rs:891-897`) admits only ground and falling or parachute, so the grenade and C4 pouches, their returns, and the grenade and C4 ticks (`acting: gameplay`) are all off in a vehicle. This is also why the reviewer could not draw C4 or grenades at 4147 (another area), and it conflicts with the binding answer "pouches available seated".

#### Implementation

1. Fix item 1, so that the seated belt push drops and loads.
2. Add a `DoubleTapDetector` beside `TapDetector` in `reload/eject.rs`, fed per hand with that hand's trigger `ButtonControl`. That needs the triggers added to `reload::FinishInputs` (`reload/mod.rs:531`) from `weapons/finish.rs:72` and `bindings/mod.rs:815`. Gate it on `Mode::Vehicle(_)` in `steps.rs:476 ejects`, and send the target into the existing `self.eject` for that hand.
3. Avoiding the conflict with semi-automatic fire: two fast trigger presses are exactly how a semi-automatic is fired, and the first press has already fired before the second arrives, so no detector can tell "eject" from "two shots" in time. My recommendation:
   - A dry double tap (both presses refused by the reload's fire hold, or the second press after the magazine ran empty) always ejects. This covers the real need, an empty gun, with no conflict, because a dry press fires nothing.
   - For a loaded gun, require the gun pointed well up (the muzzle more than about 60 degrees above the horizontal, the usual "gun up, magazine down" posture) for the double tap to count. Aimed fire never looks like that. The other route for a tactical reload is the belt push, which the binding keeps.
   - The windows: presses of 0.2 s or less, at most 0.35 s apart, with the eject on the second press's release.
   This narrows the binding ("a double tap drops the magazine") to dry guns and a raised muzzle. Flag this for the reviewer: an unconditional double tap would eject on every quick pair of shots from a seat.
4. Let `begin::gameplay` admit `Mode::Vehicle(_)` for the grenade and C4 pouches and their returns, or split the predicate so that the pouches use `Capabilities`. Check that a pouch draw cannot fire while a hand holds a vehicle control. The seat's body frame is already used for the magazine pouch.

### 4. 1629: grenades pressed into a full pouch fall without the player noticing

#### What happened (verified)

The grenades came from a locker shelf (`chosen="locker grenade (explosive)"`, from 1626.90). The first went back at 1628.42 (`put the grenade back into the pouch`). After that the pouch was full. Six presses at 1632.30, 1633.73, 1635.86, 1637.69, 1638.68, and so on logged `grenade: the pouch refused the grenade ... the pouch is full`, and each grenade then fell when the grip opened (1634.42, 1636.43, 1639.06). The take's events show the haptic did play every time: `haptic: right pouch refused (buzz) 0.50 for 140 ms at 200 Hz` at 1632.22 and 1633.65. That is one 140 ms buzz at 0.5 amplitude, the same buzz as a refused grab, which the player did not notice.

#### The rejection site

`return_to_pouch` at `payload/src/controllers/weapons/grenade/mod.rs:660-668` refuses with `stock::may_give_back` (`grenade/stock.rs:163`), increments `returns_refused`, and plays `Event::PouchRefused` (`haptics/cues.rs:357`, buzz at 0.75 strength). The grenade stays in the hand, and letting go then drops it as any held grenade is dropped. The press detector is `weapons/pouch_return.rs`.

#### Proposed feedback (reuses what exists)

- The haptic: play a distinct double cue for "full", rather than the generic refusal. Two `Event::PouchRefused` buzzes about 0.18 s apart, the way `ReadyToFire` is played twice by the reload's ready cue (`cues.rs:350`), or add an `Event::PouchFull` with `Primitive::Buzz` at full strength. Document it in `docs/mod/input/haptics.md`'s table.
- The sound: play the player's melee-impact sound for the default material at the pouch (`physical_hands::sounds`, the `AttachedEffectContainer::MELEE_IMPACT_USAGE` path already used for hand impacts). That gives a dull knock at the chest. A grenade-specific "denied" sound would need RE: whether the grenade weapon's attached effects carry `WeaponData::OUT_OF_AMMO_FIRE_USAGE`, as `reload::engine::play_refuse_sound` uses for guns.
- The visual (best of the three): while a hand holds a returnable grenade and the stock is full, draw the grenade pouch's ring red as the hand approaches, as the occupied magazine well's ring turns red (`begin/mod.rs`, the `MagazineWell` target). The player then knows before pressing.
- Optionally, show the wrist hint "Grenade pouch full" on refusal.

The grenade should still fall when let go (physical parity), so the fix is the warning, not keeping it in the hand.

### 5. Lockers should refill carried spare magazines

#### How the mod tracks spare magazines

The mod has no separate count of spare magazines. A pouch take draws on the game's reserve for the gun's ammunition type, `Inventory::m_Ammunition` (read through `WeaponTick::reserve`, with `has_ammo` at `reload/mod.rs:980`). The magazines lying in the world, the loose records (`reload/loose.rs`, `loose/records.rs`), are not carried. The loaded magazine's rounds are partitioned by `reload/ammo.rs` (magazine, chamber, detached) and never touch the reserve except through `WeaponBase::Reload`. So anything the game adds to the reserve is the player's carried spares, and it leaves loaded magazines alone, which is what the reviewer asks for.

#### What the game does at a locker (partly verified)

The defs show one automatic ammunition path: the weapon pickup interaction's credit, `WeaponBase::ShouldAutoPickup`. Coming within a weapon's pickup interaction gives its stored rounds (`WeaponBase::m_StoredNonMagazineAmmo`) to the reserve through `Inventory::AddAmmo`, up to the cap, when `Inventory::HasWeaponWithAmmoType` holds. I decompiled it to confirm: it also skips a weapon whose ammunition type is one excluded kind, plus the unlimited cases. `Inventory::GiveMaxAmmo` is called only from `Inventory::Init` and a scripted player event in `CPlayer::HandleEvent`, and there is no locker-specific refill function by name (`CResupplyPoint` handles only flares and beacons). So unless an unmapped script event fires at lockers, the base game's "stand at a locker and refill" is the auto-credit from the weapons displayed in the locker. This is unverified as the locker's mechanism: the next step is to put a breakpoint or detour log on `ShouldAutoPickup` and `Inventory::AddAmmo` for the local player at a locker.

#### What the log shows

During the locker visit at 1626 to 1660, no reserve change was logged. The two reserve rises in the whole take (2067.96: 90 to 160, and 3991.98: 325 to 360) both come from taking a same-type rifle into the hand. Locker grenade grabs also happened around 2180, 3180, and 4220 to 4260, with no reserve rise logged near any of them. Caveat: the reserve is only logged for a gun held in a hand.

#### Which mod state likely stops it (hypotheses)

- The mod zeroes `m_StoredNonMagazineAmmo` on the rack weapons it handles: a finite pair's staged copy (`hold/rack/paired/finite.rs:174, 276, 484`), use-item takes (`hold/use_item.rs:204`), and `weapons/engine.rs:397`. A locker weapon the mod has staged or split has no stored rounds left to credit.
- `HasWeaponWithAmmoType` is answered by the detour in `weapons/inventory/ammo.rs` only for weapons the table has on the body. That should be correct, but a single pistol or a carried weapon outside the eight slots depends on `publish` having run that tick.
- A locker weapon in `E_WS_WEAPON_RACK` may not activate its pickup interaction at all while the mod's rack targets own it. That needs a runtime check.

#### Fix

1. Verify first: add an info log in the `HasWeaponWithAmmoType` detour path, or a light detour on `Inventory::AddAmmo` for the local player, giving the type, the amount, and the source weapon. Then stand at a locker with a matching gun below its cap.
2. If the game credits but the mod emptied the stored rounds, keep `m_StoredNonMagazineAmmo` on the locker's original weapons and zero only the staged copy's.
3. If the base game really refills to the cap at lockers through a script event (the event `CPlayer::HandleEvent` routes to `Inventory::GiveMaxAmmo`), nothing in the mod blocks it: `GiveMaxAmmo` writes the reserve, and its refill-from-empty `StartReloadingWieldingWeapon` is refused by `reload/gate.rs` as intended, leaving loaded magazines alone. Then the gap is elsewhere. Either way, mapping that event's name is RE work for the defs.

### 6. 1653: the dual-wield rack's second weapon appears on the first grab

#### What happened (verified)

At 1649.86 the mod logged `rack: requested the finite pair's only initialized copy`, before any grab. The copy is spawned ahead of time, so it is not created lazily. At 1653.35 the right hand took member 0, the copy (`original=false`), and at 1655.40 the left hand took member 1, the original actor. In the frames, the shelf shows one SMG at 1652.5, the SMG is in the hand at 1653.5, and a second SMG lies on the shelf at 1654.0. To the player, the second gun appears out of nowhere on the first grab.

#### Why it looks lazy

`finite::staged` (`hold/rack/paired/finite.rs:165-190`) puts the pre-spawned copy into the rack state and hides it (`SetVisible(false, ...)`, line 176). The original pair actor keeps being drawn the game's way, which shows the pair as one gun in its rack pose. Only on the first accepted take does `show_remaining` (lines 490-503) move gun 0 onto gun 1's matrix, show the original, and hide its extra gun. The two weapons exist from 1649.86, but only one is ever visible on the shelf.

#### Fix

Once the copy is `Stage::Ready`, show both, stacked:

- In `finite::targets` (the `ready` branch, around line 81), or in `staged`, make the copy visible in the rack state and place its guns at member 0's matrix, offset by a gun's thickness along the shelf's up (or by the pair's own spacing). On the original, show gun 1 only (`single::hide_extra` hides index 1 and up, so this needs a "hide gun 0" counterpart, `engine::hide_gun(weapon, 0)`).
- The grab targets already come from each member's own points (`member_points`), so the hand takes whichever gun it reaches, and `show_remaining` becomes a no-op for the visuals.
- Before the copy arrives (`Stage::Waiting`), keep the current look. The copy normally arrives within a few seconds of the source being seen.
- `settle`'s refusal path (`ClaimKind::FiniteFirst` not accepted, lines 284-288) must re-hide the copy and re-show the original whole, as it does now.

The game internals involved are the rack placement of a single weapon's guns. Check that `SetVisible` plus a matrix write on a rack-state weapon holds against the placement rule each frame (the same question `show_remaining` already answers for the original).

### Conflicts and cross-area notes

- Item 3's binding (pouches available seated) conflicts with `begin::gameplay` excluding vehicles. That is the same cause as the 4147 report (no C4 detonator or grenades while seated).
- Item 3's unconditional trigger double tap conflicts with semi-automatic fire. The proposal limits it to dry guns or a raised muzzle, and the reviewer should confirm that.
- Item 1's refused drops also defeat the seated belt reload (item 3) and any knock (item 2's successful strikes go through the same `queue_eject`).

## Holsters, pouches, and dropped weapons: 14 November 02:05 take

Times are Rerun t. Evidence is the session log (via `docs/sessions/2026-11-14_02-05/log_annot.txt`), the take's tracking (`tracking-synth dump`, text and `--json`), and video frames (strips in `scratchpad/analysis/hands/holsters/`). Code references are to the working tree at a4405484; the session build is 02:04:28, and none of the code cited here changed in between except where noted.

The short version: three code faults explain most of the drops in my items. The release path's retry drops a weapon whose dock the animation refuses and cancels an attraction already in flight (harvest problems 1 and 2). A grip held on the grapple line counts as an empty fist that keeps grabbing, so it pulls guns off the hips mid-swing and throws them when the line is let go (items 4 and 5). Opening the debug panel tears the weapon handling down instead of pausing it (item 6). Separately, seated hip sockets are placed from the live head rather than the drawn body, which explains 2362 and 3894.

### Item 1: left pistol lost on respawn (646 s)

What happened (verified from the log and frames). The pistol was not lost at the respawn. The player had only one pistol long before the death:

- At t 291.22 the right pistol (#1) fell from a two-handed hip holster, released at 4.5 m/s, over the hips' 3.5 m/s attraction cap (the harvest's "too fast" case). It left the table at 307.63 ("left to the game"). From then on #4, the former left-hip pistol, was the only one.
- At 573.87 the left hand drew #4 off the left hip. At 574.83, the tick the body case changed to "dead", the table logged `#4 moved from the left hand to the right hip: docked` with no `hold:` line. The game holstered the wielded weapon at death, and the departure went to the right hip. Frames: at 574.9 the left hand is empty.
- The respawn at 579.65 to 582.74 kept it: `#4 ... is bound to object ...: the spawn reset (respawn)`, `#3` (the special slot) bound likewise, and `single pistols: a pistol is one pistol why="the respawn of one pistol"`. The carried set was empty (`respawning the carried weapons ... requested=0`). #2, the rifle dropped at 573.82 before the death, left the table at the reset, as a weapon lying in the world should.
- At 646.88 the right hand drew #4 off the right hip (frames 646.6 and 647.3 show the "Right hip" hint, then the pistol in the right hand with the left empty). So the left hip was empty because the only pistol had moved to the right hip at the death.

The same death-time move happened again at 675.35 (`moved from the right hand to the right hip: docked` at the "dead" tick).

Holsters do carry across a respawn. The restore path (`weapons/inventory/restore.rs`, `on_pre_reset` records, then `input_tick` at restore.rs:308-318 respawns and adopts) and the slot rebinding both behaved correctly here.

Root causes:

1. The death-time dock ignores the hand's side. `loadout/live/hands.rs:61` sends a `Whereabouts::Slot` departure to `free_socket(table, kind, None)`, which takes the body's preference order (right hip first for a pistol) rather than the hip on the hand's side. A pistol the left hand held lands on the right hip.
2. The real loss was at 291.22: a hip release too fast for the attraction, in a two-handed stow (harvest).

Fix:

- `loadout/live/hands.rs:59-62`: pass `Some(Socket::Hip(hand))` as the preferred socket for a weapon that fits a hip, so the game's own holster (death, cutscene, vehicle entry) puts the weapon on the hand's own side.
- The binding "lone pistol replicated to the empty holster on respawn only" is mostly built already. The pair-splitting path asks the spawn system for a twin and carries it on the free hip (`weapons/dual_wield/single/mod.rs` `act_on` `Grant::Request`, then `take_twin`, which deals the magazine). On a respawn, `Grants::arrive` (`weapons/dual_wield/single/grant.rs:249-288`) turns a respawn cover of a single pistol into `Grant::One` (grant.rs:262), so no twin is asked for. Proposal:
  - Extend the respawn cover (`Grants::cover_respawn`, grant.rs:163, set from `restore.rs:315` through `single::cover_respawn`) with whether the reset's record carried another pistol of the same tag on a hip. Keep `slot_pistol_single` and add a `lone` flag computed in `on_pre_reset` from the carried records.
  - In `arrive`, a respawn cover that is single and lone returns `Grant::Request { object }` instead of `Grant::One`, provided the other hip is free (`tick.socket_free`). The game hands the slot back as its pair after a reset, so the existing split makes the two pistols and deals the magazine between them.
  - "Respawn only": set `lone` only for `Origin::Reset`, never for `Origin::Load` or a mission give-back (`cover_mission` stays as is).
  - The case where the survivor is the carried twin and the slot is empty needs a second path. After adoption on a reset, request a twin of the carried pistol's tag (`restore::request_twin`) and carry it on the free hip. Treat that as a follow-up, since the slot-holds-the-pistol case covers the observed one.
  - Unit tests belong in `grant.rs`'s tests: single and lone gives Request, single with a carried twin gives One, and a load gives One.

### Item 2: reaching fails at the waist, the left-leg holster, and the grenade pouch (2044-2050, 2362.54, 2560-2580)

#### Where the grab circles are placed (the reviewer's question)

On foot, every body volume rides Rico's drawn skeleton, as the previous frame left it (read on the input tick before the world update), carried into the cockpit frame. The hand side of the test is the drawn grip (the arm-remapped hand as drawn), not the controller. Nothing is placed relative to the movement capsule. Nothing on foot is placed relative to the head.

- Hip sockets ride the leg, not the waist. `weapons/inventory/sockets.rs:31-38` maps each hip to `SafeBoneIndex::LEFT_HOLSTER` / `RIGHT_HOLSTER`. In the skeleton log these are `LeftHolsterAttach` (a child of `sm_LeftHolsterClip`, a child of `LeftUpLeg`) and `RightHolsterAttach` (a child of `RightUpLeg`), so the hip sockets follow each thigh. `weapons/docks.rs:136-141` reads the bone (`bone_world`, docks.rs:526, from `grip::bone_model_pose`, `hands/grip.rs:473`).
  - A holstered weapon's draw circle sits on the weapon's own grip point (docks.rs:169, `Placement::Visible`, `docks/attachments.rs:141-152`).
  - An empty hip's dock circle sits at the grip last seen on that bone, or else at the bone origin plus the socket offset in the character frame (attachments.rs:154-185).
  - Reach radii are 0.22 m for the hips and 0.255 m for the shoulders (`weapons/volumes.rs:41-44`). The attraction extends a hip to 0.3 m for a release (`docks/attract.rs`).
- Shoulder sockets ride `BackAttach` / `BackAttach02` under the upper-back harness (sockets.rs:33-34).
- The pouches ride the drawn torso (`weapons/volumes.rs:1-26`). The magazine pouch is at the belt front, oriented by the pelvis girdle (the line through the hip joints, `centre_magazine_pouch` / `magazine_pouch`, volumes.rs:323-362), radius 0.075 m. The grenade and C4 pouches are on the chest front under each shoulder joint, oriented by the chest girdle (radius 0.09 m), and kept upright against a look down.
- Conversion: `weapons/begin/socket_candidates.rs:110` (`frames.cockpit.point_to_cockpit`), with the cockpit frame anchored on the live head (`docks.rs:487-494`).
- Seated is the exception, and it matters for 2362 and 3894. While the posture is seated or the seat is locked (`seated_waist`, docks.rs:398-447), the hip sockets are not on the legs. They are at a "waist" computed from the live head: `neck_pivot(head)` minus the standing spine length straight down (docks.rs:409-410), plus the bind-pose holster offset turned by the torso yaw, plus the arm remap (docks.rs:433-444). `place_waist` (docks.rs:353-396) then moves the holstered weapons there each frame. So seated hip circles and holstered guns are relative to the head, and they follow every lean.

#### 2044-2050: magazine pouch at the waist

Verified. Three left-hand presses missed the reload pouch by 103, 51, and 22 mm (2043.65, 2044.47, 2045.83, `grip: pressed ... nearest_miss="reload pouch is ... out of reach"`). The take came at 2050.82 at 42 mm.

From the rig events (`event rig`, JSON): the player was standing still, not walking. `moving` went false from 2045.5, the pelvis floor point was fixed, and the phase was idle or drifting throughout.

- Body (legs and pelvis) yaw sat latched at 42.4 degrees from 2042.7 to 2046.0, while the head's yaw target fell from 37 to 23 degrees. That is a misalignment of 5 to 19 degrees, and 12 to 40 degrees against the raw head quaternion yaw as the player looked down and left.
- From 2047.2 an estimate-driven turn (`turning (estimate)`) carried the body to 74 degrees while the head stayed at 50 to 53 degrees, overshooting by about 21 degrees until 2050.6.
- The spine bend read 14 to 21 degrees (the head leaning forward to look at the belt).

A pelvis yaw error of 15 to 20 degrees moves a belt-front point about 0.15 m ahead of the hip centre by about 4 to 5 cm, which is the size of two of the three misses. So the reviewer's diagnosis is plausible but only partly verified. The miss sizes fit a yaw error, but the successful take at 2050.82 also had the body 20 degrees off the head. Hand placement varied too.

#### 2362.54: the left-leg holster while seated in the plane

Verified: the player was seated in a plane (flight stick and throttle grabs; `controller_state` mode "vehicle", seat "locked"). The left hip was empty at that moment, because the SMG #66 was in the left hand from its draw at 2349.64 until it docked on the left hip at 2365.74. So the circle the player wanted was the left hip's dock circle for the SMG in hand. Because the seat was locked, that circle and the hip's holstered position came from `seated_waist`, under the live head, not on the drawn left thigh.

The head was 0.16 m low and 0.18 m forward of the cockpit origin at 2362.4 (`view.head_cockpit` = (0.10, -0.16, 0.18)), leaning toward the knees. The head-derived waist moves with it, so the circle sits ahead of and inside where the drawn holster is. In the frames (2361.5 to 2362.54) Rico's left thigh holster area shows no circle, and a circle appears further right, near the knees. The dock did work 3 s later, from wherever the hand found it.

#### 2560-2580: grenades back into the pouch

Verified as a reach miss, not a body yaw issue. Two snatched grenades came to the left hand (2562.85, 2567.02). The player put one back successfully at 2578.34 (`grenade: put the grenade back into the pouch hand="right"`). The failures are two right-hand releases that the tracking logs as `throw: right grenade 0.18 m/s ... not armed` at 2560.26 and `0.36 m/s` at 2564.90. These are silent drops outside the pouch, with no refusal haptic or log line.

- In Rico's model space (the `drawn arm joints` diagnostic), the right wrist at those releases was 0 to 4 cm below the right shoulder joint and about 9 to 10 cm forward of it. At the successful return it was 7 to 9 cm below.
- The chest girdle that carries the grenade pouch followed the head: chest yaw was within about 1 to 8 degrees of the head's yaw target at the failures (-17.5 vs -21.7, -58.9 vs -60.3). The body (legs) yaw was 13 to 24 degrees off, but that also held at the success (-100 vs -113).

So the hand was released too high, at shoulder level, above a pouch that sits below and inside the shoulder joint. The pouch's chest yaw was not the problem.

#### The reviewer's proposal (body yaw strongly aligned with movement)

It would not have helped these cases: at 2044-2050 and 2560-2580 the player was standing still, and 2362 was seated.

It also conflicts with the binding answer for idle. Idle on foot, the legs and body follow the solved IK driven by the learned EgoPoser estimate, and authored poses apply only where IK is ill-posed. It also conflicts with the standing rule against reorienting the player without a movement action. While walking, the authored walk already faces the movement direction and IK adds about 20 percent on top, so the proposal adds little there.

What the data supports instead is aligning Rico's body to the player (the "align Rico, don't decouple" principle) during an idle reach:

- Tighten the idle body yaw's dead band, or turn the body toward the head's yaw target (or the estimate's pelvis yaw) faster while a hand reaches toward a belt or hip volume, below the chest and within about 0.35 m of the pelvis.
- Find why the estimate-driven turn at 2047-2050 overshot the head by about 21 degrees and held there. That belongs to the body area (`crates/body-rig/src/rig.rs`, the yaw and estimate stage).
- Optionally, orient the magazine pouch by the chest girdle, or a blend of chest and pelvis, instead of the pelvis alone. The chest tracked the head within a few degrees in both windows.

#### Fixes for item 2

- Seated hips (2362, 3894): place seated hip sockets on the drawn body's thigh holster bones as on foot, or at least compute the seated waist from the head pose captured at the seat lock rather than the live head, so a lean or a look down does not move the holsters (`weapons/docks.rs:398-447`, `place_waist` docks.rs:353-396). Prefer the drawn body: then the circle, the gun, and the thigh agree, and Rico's seated pose is what the player sees.
- Idle reach yaw: as above, in the body rig.
- A pouch release that misses: give it the "pouch refused" haptic and a log line (this also covers the 1629 s feedback), and consider widening the grenade pouch upward by a few centimetres.
- A diagnostic gap: the recording has no volume centres. Recording each hand's nearest body-volume centre and distance in `controller_state` would let the next review measure these misses directly instead of reconstructing them from bones.

### Item 3: the SMG put away at 3894 (where did it go?)

Verified from the log and frames. The player was seated in a helicopter (seat locked). At 3883.14 the right hand drew SMG #101 off the right hip at 48 mm. At 3895.45 the right hip's attraction drew in the SMG the right hand let go of (0.27 m, 0.18 s travel), and at 3895.64 the table logged it `docked` on the right hip. It never left the loadout.

It vanished from view because of the seated placement above. At 3895.3 the head was 0.51 m to the right of the cockpit origin and 0.18 m low (`view.head_cockpit` = (0.51, -0.18, -0.03), eye drop 0.21 m): the player was leaning far right to look at the right thigh. The seated waist sits straight under the live head (docks.rs:409-410), so the right hip socket, and `place_waist`'s holstered SMG with it, was about half a metre to the right of Rico's drawn right thigh, likely outside the cabin wall. Frames: at 3895.2 and 3895.5 the SMG ("12/420") is in the right hand on the right thigh. At 3895.8, just after the dock, it is gone, and it does not show on the thigh afterwards.

Root cause: `weapons/docks.rs:398-447` (seated hips from the live head) together with `place_waist` (docks.rs:353-396). Fix: as in item 2.

### Item 4: weapon flies off after a bad grapple swing at 2526 (no grip release)

Verified from the log and tracking:

- 2525.25: the left grip closed on the grapple line (`left line lock` haptic, squeeze 1.0) and the line locked at 2525.37. That press logged `grip: pressed hand="left" grip="fist" chosen="nothing in reach"`. To the grab latch, the hand was an empty fist.
- 2525.81 to 2525.93: the left controller barely moved (cockpit position about (-0.02, -0.51, +0.03) throughout). But the swinging body brought the drawn left hip under the drawn hand, and the latch took it: `grip: the closed hand came to a target and took it ... chosen="left hip socket (body) at 217 mm"`. SMG #66 was drawn into the left hand mid-swing (haptic `left draw, holster, or foregrip`).
- 2525.98 to 2526.20: the line released (landed at 2526.09), and the player opened the left grip to let go of the line (squeeze 1.0, then 0.92, 0.67, 0.39, 0.14, 0.0). The table read that as releasing the SMG: `#66 moved from the left hand to the world (dropped)` at 2526.26, `speed_mps=7.77`, which is the body's landing motion carried into the throw.

So the player did let go, but of the grapple line, not knowing that the fist holding the line had taken the SMG.

Root cause:

- `grab/latch.rs:79-99`: a press with nothing in reach and no game action bound becomes `GripLatch::Fist`, which takes the next target the hand comes to after the 0.2 s window for as long as the hand is "slow". "Slow" is measured on the controller, which is still while the body swings.
- `bindings/tables/mod.rs:350-358` already reserves a grip for the snatch's catch (`catch_reserved`) but not for a grip that holds the grapple line. The line-holding press counts as `bound = false` and becomes a hunting fist.

Fix:

- Reserve a hand whose grip holds the physical grapple line (attached, locked, or reeling) the way `catch_reserved` is reserved. Its latch should be `Default`, or a new `Line` state, never `Fist`, and it should keep that until the grip opens (`bindings/tables/mod.rs:350-358`, with the reservation published by the grapple module).
- Belt and braces: never let a fist take a body socket or pouch (only a fresh press draws). The latch's own doc says "a fist swung past the hip draws nothing", but that holds only for a fast fist. Drop body volumes from the fist path in `GripLatch::step`, or measure "slow" on the drawn hand relative to the body rather than on the controller.

### Item 5: back weapon and right pistol dropped while upside down on a grapple (844-865)

Verified. Neither weapon left its socket on its own, and the body orientation did not knock them off. Both drops are in-hand releases:

- The right pistol (#4, the only pistol, on the right hip since the death at 675) left the hip by the item 4 mechanism. The left grip closed on the line lock at 833.51 (`grip: pressed hand="left" grip="fist" chosen="nothing in reach"`). When the lock ended (landed at 836.23), the closed left fist swept through the right hip socket and took the pistol: `the closed hand came to a target and took it ... chosen="right hip socket (body) at 133 mm"` at 836.26. The pistol rode in the left hand through the next grapple, and when the left grip opened at 845.90 during the inverted hang (line locked at 1.5 m, `hang=true`), it fell (`let go ... speed_mps=0.50`). The harvest classed this drop as deliberate; it wasn't.
- The back weapon (the Hydra, #3, special slot, left shoulder) was drawn by the right hand at 851.95. Released over the left shoulder at 860.08, the shoulder's attraction started, and `Inventory::GetAnimationAllowsHolstering` refused on every tick for 0.35 s (11 refusals) during the grapple hang. The gun fell at 860.44: harvest problem 1, item 7 below.

Fix: items 4 and 7. Separately, find out what forbids holstering in the grapple hang (item 7).

### Item 6: the debug panel dropped the left hand's SMG (4380)

Verified from the log. At 4378.33 the left hand drew SMG #143 off the left hip (it became the wielded weapon). At 4380.451 `controller input: toggled the debug panel enabled=true`, and in the same tick `held guns: ended a gun's joint hand="left" ... reason="the gun left the hand"` and the gun body was destroyed. The "device poll is disabled" stand-down followed at 4380.484.

The left controller's squeeze was 1.0 before and after the toggle (tracking 4380.32 to 4380.56), so the grip did not open. The table logged no let-go or dock for #143 before the recording stopped at 4387.57; the panel closed at 4389.96, after the recording. The frames do not show the left hand clearly enough to say where the SMG was drawn while the panel was up.

Root cause: the panel's capture change stands the whole weapon handling down instead of pausing it.

- The toggle tick calls `cancel_capture_routes` (`bindings/mod.rs:1202-1227`), which calls `weapons::stand_down()` (bindings/mod.rs:1221). Every idle tick with the panel up does the same (bindings/mod.rs:365-379).
- `weapons::stand_down` (`weapons/state.rs:46-87`) does several things: it publishes empty gun holds (state.rs:48), which destroys the physical gun body; it clears the hold so that the mod no longer owns the wielded weapon's attach bones (state.rs:65, `hold::clear` at `weapons/hold/mod.rs:331-335`), and the game re-attaches the wielded SMG by its own rule; it clears the release state (state.rs:66); and it disarms any weapon armed in a hand (state.rs:71, `armed::stand_down`, `hold/armed.rs:315-327`).
- The loadout table is deliberately kept through a pause ("a pause of the controllers (the debug panel, a lost frame) keeps it", bindings/mod.rs:385-386), but the presentation and physics of the held weapon are not. To the player the SMG leaves the hand.
- On resume, `release_tick` (`weapons/hold/act.rs:340`) reads the physical grip afresh. If the player is not squeezing when the panel closes, it stows the gun, or lets it fall when no socket fits (`StowFallback::Fall`, act.rs:478). This is the likely source of an actual drop after the panel closes.
- 304ae765 and 1daf4598 do not touch this. They changed the poll the bindings stand down for, and the logging of a stuck panel, not what the weapons do.

Fix:

- Split `weapons::stand_down` into a full stand-down (bindings disabled, gamepad scheme, no controllers) and a pause for `Idle::ModPanel` and `Idle::PollDisabled` caused by the panel.
- The pause keeps `hold` enabled, keeps armed weapons armed, and republishes the last gun holds frozen to the hands, while suppressing the fire and release inputs and the grab latches.
- On resume, keep the table's holds whatever the grip reads, until the grip next opens. The pause should not count as a release.
- Files: `weapons/state.rs`, `bindings/mod.rs` (the ModPanel branch at 365-379 and `cancel_capture_routes`), `weapons/hold/act.rs` (`release_tick`'s stand-down rule, act.rs:328-339).

### Item 7: harvest new problems 1 and 2 (root causes confirmed in code)

The release flow: `release` (`weapons/finish.rs:783-907`) records a release intent with its socket (`intend_release`, finish.rs:827 and 864), then attracts or docks. Every later tick, `retry_release` (finish.rs:374-398, `weapons/hold/act.rs:305-323`) steps the intent (`ReleaseIntent::step`, `weapons/hold/dock.rs:42-59`). If that returns true, `release` runs again.

Problem 1 (stows refused by the animation drop the gun):

- `hold::act::attract` and `dock` call `start_dock`, which refuses with `DockRefusal::AnimationForbids` while the weapon is wielded and `Inventory::GetAnimationAllowsHolstering` is false (act.rs:1029; again at the finish, act.rs:1052). The refusal is only logged at debug (finish.rs:887-889).
- The intent keeps its socket, so the next tick retries the attraction, which is the burst of `refused` lines (for example 11 at 860.08 to 860.42 and at 2517.82).
- After 0.35 s (`ReleaseIntent::new`, dock.rs:34-40), or as soon as the hand is no longer "near", `step` sets the intent's socket to `None` (dock.rs:55-57). The next `release` then finds no socket: `retry_socket` filters out `over` (finish.rs:933-938), and `attracted` is filtered by `pending.socket` (finish.rs:818-826). `grab::release` returns `LetGo`, and `let_go` drops the gun (finish.rs:903).
- The `StowFallback::Keep` that the stand-down stow path has (act.rs:385-393, 466-481) exists nowhere on this path.

Problem 2 (the attraction starts and the gun still falls):

- After `attract` succeeds, a holster snap (`slide_onto` or `slide_back`, `Finish::Dock`) carries the gun to the socket over `travel_s` (0.16 to 0.34 s). During the travel, `retry_release` keeps re-evaluating `near` as "the hand is in the socket's reach, or `attraction_for(hand)` still picks this socket" (finish.rs:375-381).
- `attraction_for` reads the hand's current drawn grip and its peak speed over the last 0.1 s (finish.rs:989-1012), so a hand that moves on after letting go, or speeds past the 3.5 m/s hip cap, is no longer "near". At 2169.82 the release was logged at 6.5 m/s 45 ms after the attraction began.
- `step` then clears the socket (dock.rs:55-57). `retry_release` cancels the in-flight holster snap because the intent has no socket (act.rs:316-321) and returns true, and `release` drops the gun as in problem 1.
- The 0.35 s intent timer also counts from the first release (`intend_release` keeps an existing intent for the same weapon, act.rs:298-303), so a long travel can expire it mid-flight.
- Log timing matches: in 2169.82 to 2169.86 and 3218.60 to 3218.64 the let-go came 45 ms (two ticks) after `drew in the weapon`, well inside the travel.

Fix:

- Commit an attraction or dock once its snap has started. Add a `committed` flag to `ReleaseIntent`, set by `dock_started` and `attract`. `step` should then skip the `near` and timer checks, and `retry_release` should not clear a committed holster snap. Only a fresh grip press (act.rs:540-545, already handled) or the weapon leaving the hand calls it off.
- Defer instead of dropping on `AnimationForbids`, as the game itself defers a holster (the pyxis doc on `Inventory::m_DoesAnimationAllowHolstering`).
  - Let the attraction or dock start, and gate only `finish_dock`.
  - In `act::tick` (act.rs:532-563), when `finish_dock` returns `AnimationForbids`, keep the snap parked at the socket and retry each tick for up to about 1.5 s.
  - If the animation still forbids it after that, keep the gun in the hand (the grip re-latches when it closes) rather than letting it fall.
  - This makes `start_dock`'s check at act.rs:1029 unnecessary on the attract and dock paths.
- RE needed: which half of `Inventory::GetAnimationAllowsHolstering` refuses in these states. The candidates are `Inventory::m_DoesAnimationAllowHolstering`, recomputed by `Inventory::UpdateInventoryPermissions` from the animation, and the owner's `StateBitFlag::SB_DISABLE_HOLSTER_WEAPON` state bit. Find which animation states (grapple hang, the firing recovery 1 to 4 s after a shot, the equip right after a draw) set them.
  - A cheap first step without RE: log the field and the state bit separately on each `AnimationForbids`, at info and rate-limited.
  - If the bit is the grapple hang's own, a mod-driven stow of a weapon the player holds by hand may be safe to complete regardless. That needs verifying in the IDB before relying on it.

These two faults account for 17 of the take's 18 failed stows (11 and 6), including the Hydra at 860.44 (item 5) and the rifle at 2517.84.

### Conflicts and cross-area notes

- The reviewer's "align body yaw with movement" conflicts with the binding for idle (EgoPoser-driven IK) and does not cover the observed cases, which were stationary or seated. The body area should own the idle reach-yaw and estimate-overshoot fixes.
- The seated hip placement from the live head conflicts with "align Rico, don't decouple", and with the binding that seated play keeps the belt and pouches usable: a lean moves the holsters off the body.
- Item 4's reservation of the line hand interacts with the firm-ragdoll grapple-swing binding. Holsters must stay reachable during a swing by a deliberate press of the other hand, so the fix should block only the line-holding fist, not draws in general.
- The death-time dock to the wrong hip (item 1) would also show after cutscenes and vehicle entries, wherever the game holsters for the player.

## Hands, props, ragdolls, and throws: 14 November 02:05 take

Times are Rerun t (log UTC minus 01:09:08.957). Evidence comes from the session log (`log_annot.txt`), `tracking-synth dump` of `tracking.jc3trk` (the `held:`, `bodies:`, `throw:`, `force:` events and the per-frame controller state), and video frame sheets in `scratchpad/analysis/hands/props/` (`g2659.jpg`, `g2672.jpg`, `g2716.jpg`, `g3268.jpg`, `g531.jpg`, `g3236.jpg`, `g3263.jpg`, `g3715.jpg`; the dumps are the `d*.txt` beside them). Paths are relative to `payload/src/controllers/`.

### 1. Ragdoll grips: lost with movement, two-point grips spasm

#### What happened

- 2659.40 to 2665 (two-handed NPC grab): both hands gripped a standing soldier on the same tick. Each grip fired its own topple (`a hand threw a character` twice at 2659.42, 1.1 of a knockdown each, so the hit reaction accumulated 2.2), and each hand then joined a different bone (16836 and 12950, about 19 cm apart, both at head height). For five seconds the `held:` reports show both bones circling the palms in a 0.3 to 0.5 m loop at roughly 2 to 4 Hz: bone speed swinging 2 to 6.5 m/s, lag 0.1 to 0.43 m, and the hand's lag at full (`load 1.00`) on most samples. The player walked at 2.3 to 3 m/s throughout. Frames show the soldier's torso thrashing a few centimetres from the camera (2660 to 2664, `g2659.jpg`); the carry-collision lease keeps every ragdoll body off the player, so nothing stops the body from filling the view.
- 3268.97 to 3270.37 (one hand, ally): a single joined hold on bone 11938 while the player stood still (character velocity 0). The same loop appears: bone speed 1.2 to 5.7 m/s, lag oscillating 0.11 to 0.55 m with a period of about 0.25 s, pegged load, then the lag ran out to 0.95 m and the hold tore (`torn from the hand`). So the oscillation does not need player motion; movement only makes the tear likelier.
- 3273.58 to 3275.05 (two hands): smaller (lag up to 0.22 m) but the same 3 Hz ring. Both joints then ended together (`the constraint or the object's body went away`) and at 3275.07 the hand struck the same character standing: she got up out of the grip. This answers the open question in character-grabs.md: the game's get-up does fire while a hand holds the ragdoll, about 6 s after the topple here.
- 2674 to 2768 "propane tank" (`g2672.jpg`): the gas cylinder is body 7325, 25 kg, classed as an articulated object (it has an enabled constraint to another body, probably its neighbours in the rack), held joined in the right hand 2672.33 to 2677.74. For 5.4 s every report is `load 1.00`, lag oscillating 0.06 to 0.45 m about every 0.2 s, spin pinned at 9.98 rad/s, with the player standing still. That is the spasm in the frames. The later red crate (body 8187, 100 kg, also articulated through its lid hinge, 2716 to 2744) tore twice.
- 3257 (and 3256.06, 3259.95): three grips on an allied blue-beret soldier each logged a topple, then `no ragdoll came within the hand's reach after a yank`. She stayed standing (frames). The same character did topple at 3268.92, after her gun had been taken from her at 3261.90.
- 3382 to 3389 (tripod machine gunner, body 15743): five topples logged (`seated=false`), all followed by `no ragdoll came within the hand's reach`. The gunner never left the gun.
- Over the take: 36 ragdoll holds, 11 torn, 4 ended by a joint (two of which are the get-up above).

#### Root causes

1. The joined drive is a dead-beat velocity servo on the hand body alone, sized for the bone it holds, while the real load is the whole ragdoll on soft constraints (hypothesis, strongly supported by the data). `physics/drive.rs:184-194` (`drive_joined`) asks the hand to close `linear_gain` (1.0, `physics/config.rs:169`) of the error in one step against `tuning.mass_kg`, which for a ragdoll is the hand plus `joined_mass_kg` (`pickup/tick.rs:1088-1100`), the bone alone (about 7 kg). It reads and overwrites only the hand body's velocity each step. The other ~93 kg hangs off the bone through Havok's ragdoll constraints and stores momentum the drive never sees or damps. The commit that sized the drive for the bone fixed the runaway ring (the test at `physics/drive.rs:397-431`), but what is left is an undamped mode of the rest of the body against a stiff, force-capped servo: the hand saturates, overshoots, and the body orbits the palm. A 3 kg hand body joined to a 7 kg bone hanging 93 kg is also a mass ratio the iterative solver converges badly on.
2. Two hands on two bones over-constrain the chain. Each hand has its own fixed joint (`physics/joint.rs`), its own goal, and its own angular drive (twist cut to `two_hand_angular_share` 0.25, `physics/state.rs:419-435`), and neither knows about the other. The palms' separation and relative rotation rarely match what the ragdoll's joint limits allow between those two bones, so the hands fight the ragdoll constraint through the solver; each drive also assumes it alone moves its bone, so where both pull the same chain the effective gain doubles. This is the "two constraints fighting" case, and it matches the larger loop with two hands at 2659.
3. Articulated objects (the cylinder) have the same structure: a fixed joint and a full-strength dead-beat drive fighting the object's own constraint. `HeldKind::Articulated` is joined the same way as a prop (`pickup/tick.rs:731-760`); the hold's lag never closes, so the drive stays saturated. The 9.98 rad/s spin ceiling seen on both the cylinder and the swung panel at 3238 is not a mod constant (pickup caps at 20, hand bodies at 30); it is probably the Havok motion's own maximum angular speed (unverified).
4. Grabbing with both hands topples twice. Each hand's `topples` path (`characters/mod.rs:153-162`, dealt in `physics/blows.rs`) queues its own 1.1 knockdown blow on the same character in the same tick, so the reaction sums to 2.2 and the body is flung, which starts the two-hand hold already ringing.
5. Topples that do nothing are logged as throws. `characters::topples` only checks `SB_SKIP_HITREACTION_RAGDOLL` and death. Cases where `Character::HitReaction` returns without ragdolling (the damage-state flag `m_DamageFlags`, an ally in a state the reaction ignores, a character in a mounted-gun state) leave the hand waiting a second for a ragdoll that never comes. The tripod gunner is not attached to a vehicle, so `characters/engine.rs:130` (`gunner_out` requires `IsAttachedToVehicle() && IsInMountedGunState()`) never applies to it, and the reach classifies him as on foot. Why the ally refused at 3256 to 3260 but not at 3268 is not established (her gun being in her hands is one difference).
6. The get-up ends the hold after a few seconds (above), so long carries end on their own.

#### Fix proposals

- Damp the ragdoll as a body, not the bone. In the pre-simulation step, for the held character's ragdoll bodies (the hold already knows them through `Held::ragdoll`), pull each body's linear and angular velocity toward the gripped bone's (or the ragdoll's centre-of-mass velocity) by a small factor per step, `v_i += (1 - exp(-c·dt))·(v_ref - v_i)` with c around 3 to 6 /s, relative to the carrier. This removes the orbit and the thrash without stiffening the limbs. It is a velocity write through `hknpWorld::setBodyVelocity`, which the hold already uses. New code in `physical_hands/characters/ragdoll.rs`, called from the joint step.
- Make the joined ragdoll drive a soft spring-damper: `linear_gain` around 0.3 to 0.5 for `HeldKind::Ragdoll` and `HeldKind::Articulated`, and damp against the joined assembly's velocity (hand and bone together, mass-weighted) rather than the hand's alone (`physics/drive.rs:136`, `physics/state.rs:417`). Keep the dead-beat drive for props, where it is correct.
- Two hands on one ragdoll: join the second hand with a ball-and-socket (point) constraint instead of a fixed one, or with no angular drive, so only positions are imposed; and divide each hand's gain by the number of hands on the assembly (`assembly_hands` already counts them, `pickup/tick.rs:1150`). Apply the same point joint to articulated objects, whose orientation their own constraint should own. A point-constraint data type needs binding in pyxis (`hkpBallAndSocketConstraintData`, the analogue of the bound `hkpFixedConstraintData`).
- Raise the hand body's effective mass while it is joined to a constrained body (keep the hand-to-bone ratio within about 1:3), or keep the hand light but drive the bone directly as well.
- Topple once per character per tick: when both hands seize the same character, deal one topple (`pickup/tick.rs:1047-1068`).
- After a topple, check `Character::IsRagdolling` a few ticks later; if not, try `ACT_HITREACT_RAGDOLL` then `ACT_FULL_RAGDOLL` through `AnimatedModel::TryAct`/`QueueAct`, as `react_rider` already does (`physics/blows.rs:676-693`). Widen `gunner_out` to any character in `IsInMountedGunState`, attached to a vehicle or not, and log the refusal reason. RE needed: what in `Character::HitReaction` refuses an allied soldier (read the early-out on `m_DamageFlags` and any state checks).
- Keep the held ragdoll out of the camera: collide it with Rico's drawn-body proxies from item 5 instead of excluding it from all player bodies.
- Hold the get-up off while a hand holds the ragdoll. RE needed: the ragdolling-timer condition (`CRagdollingTimerCondition`) and how the graph reads it; the mod's held cancel input evidently does not stop it.

### 2. Hitting people with held props or a swung weapon does nothing (3238, 3265)

#### What happened

- 3236 to 3239.6 (`g3236.jpg`): both hands held a 50 kg roof panel (body 6714, a prop, carried) and swung it at 7 to 11 m/s around 3238.0 to 3238.4. Every few hundred milliseconds a both-hands strike haptic of 0.33 to 0.37 fired, which is the held-prop observer's feedback, so contacts were being reported. No character reacted. Whether the panel actually touched the soldier at 3238 cannot be confirmed from the frames (he stands a few metres off at 3238.0; at 3239.3 he is close); the supplemental strike only logs at debug level (`held impacts: a prop struck a character`, `physics/blows.rs:339-344`), so the log cannot say.
- 3261.9 to 3267.6 (`g3263.jpg`): the right hand took the ally's rifle out of her hands and swung it through her head and chest at 3264.9, 3266.4, and 3266.7. No reaction and no `a hand struck a character` line. In the whole take, only one blow was ever logged `with_gun=true` (t 2163.66), against 32 bare-hand blows.

#### Root causes

- Swung guns: an intent bug (verified in code, consistent with the counts). `physics/hit.rs:85-92` returns `Intent::Take` for `GripLatch::Grab(GrabVolume::WorldWeapon)` "whatever was offered after", and `blow_impulse` returns `None` for `Take` (`physics/hit.rs:121`). The latch stays `Grab(WorldWeapon)` for as long as the grip that took the weapon stays closed (`grab/latch.rs:22-34`), and under hold-to-hold that is the whole time the gun is held. So a gun taken from the world or from a character never strikes anyone. A gun drawn from a holster (latch `Grab` of a socket volume) is also `Take` whenever an armed character's gun is offered to that hand (`(Grab(_), true) => Take`), which is most melee range. `intent` is computed in `physics/blows.rs:558-566`.
- Held props: the supplemental strike exists (`impacts/mod.rs:469-512`, `blows::held_strike`), but it is skipped whenever `native[1]` is set (`impacts/mod.rs:492`). `native` is set by the detour on `PhysicsGameObject::DoReactToHit` (`impacts/hooks.rs:46-49`, `impacts/mod.rs:390-421`), which records "suppress conservatively by recipient" for any retained pair whose character got a `DoReactToHit` call. The character's capsule listener turns every started contact into a hit that is reacted to after the step (havok-bodies.md, "Contact events"), so a prop touching a character produces a native `DoReactToHit` call nearly every time, whether or not the native reaction does anything. The supplement therefore defers to a native reaction that, for a hand-held prop, evidently does nothing visible. This is a hypothesis from the code path; the debug-level log would confirm it.
- Also limiting: `damage_impulse` (`impacts/policy.rs:30-59`) clamps the momentum at 20 kg times 12 m/s and at the solver impulse, which is fine, but the 9.98 rad/s spin ceiling above limits the tip speed of anything swung.

#### What the game offers to reuse

The player's melee is the model (melee-and-damage.md): with a linked character it sends a melee `DamageMsg` with no impulse after queuing a stumble; with none it box-queries, sends each hit object a melee damage message, and pushes each body with `hknpWorld::applyBodyImpulseAt`. The mod's own `blows::strike` already does the calibrated version of this: `Character::HitReaction` scaled against the victim's `m_HitReaction_ImpulseStumbleThreshold`/`m_HitReaction_ImpulseRagdollThreshold`, or a melee `DamageMsg` through `PhysicsGameObject::OnDamage`. No new game call is needed; the problem is gating. For impact damage proper (a thrown barrel), RE `Character::DoReactToHit`/`CPfxCharacterInstance::ProcessHitInfos` to learn the native impact thresholds (relative speed and mass) before deciding whether the native path should ever be trusted for held bodies.

#### Fix proposals

- `physics/hit.rs:85-92`: a hand that already holds a weapon never takes another, so its intent is `Swing`. Compute intent from what the hand holds (the loadout table's hand slot) before the latch: holding a gun means Swing, and `Take` only applies to an empty or closing hand. Add a test case for a hand holding a gun next to an armed character.
- `impacts/mod.rs:492`: replace the "native happened" suppression with "native had an effect". Snapshot the victim's health, `IsRagdolling`, and whether a hit-reaction act is queued before the native `DoReactToHit` call in the detour, and compare after; suppress the supplement only when the native reaction changed one of them. Simpler alternative: always deliver the calibrated strike for held-body contacts with characters, since the native path does not treat a hand-held prop as a weapon.
- Raise `held impacts: a prop struck a character` to info, with the reason a strike was skipped (below threshold, native suppressed, cooldown), so the next session can tell.
- RE: the Havok motion's maximum angular speed for props (the motion properties the body uses) and, if it is the 10 rad/s cap, raise it while held.

### 3. The yanked gunner falls through the vehicle (532.5)

#### What happened

The player stood on the moving turreted jeep (`stunt positions: ... standing on a vehicle`; character velocity about 10.7 m/s, downhill). At 532.13 the right hand gripped the seated gunner, `gunner_out` dropped him into his full ragdoll, and at 532.18 the hand took bone 2021 (100 kg ragdoll), joined at 532.21. The bone kept pace with the vehicle (10.2 to 10.6 m/s), but its lag grew from 0.06 to 0.86 m in 0.25 s and the hold tore at 532.46. The frames (`g531.jpg`) show the gunner sinking down through the hatch floor between 532.4 and 532.8 and gone by 533.0.

#### Root cause

It is a collision exclusion, not the moving frame. The carry-collision lease at 532.18 paired the ragdoll's bodies with `player=[650, 651, 834, 835, 836, 856, 857, 858, 873, 874, 875, 949, 5997, 5998, ...]`. Bodies 857 and 875 are the jeep's parts (the grip notes call them `vehicle part`), and 949 is the gunner's own capsule. `carry_collision.rs:104-118` counts as the player's every body in the player proxy's collision system group, and while Rico stands in the vehicle stunt position his proxy shares the vehicle's system group (that is what puts the jeep's parts in the list). So the held ragdoll was told never to collide with the jeep and fell straight through its floor; with the hand holding only the bone's weight (`joined_mass_kg`), the rest of the body dragged the bone down until the lag passed the tear distance (`pickup/tuning.rs:95`, 1.0 m). The carrier feed-forward did its job: the hand body kept up with the vehicle.

#### Fix proposal

In `carry_collision.rs`, define the player's bodies as those the player character owns plus the hand bodies and gun bodies by id, and drop the system-group criterion, or keep it only for bodies with no owner. A body owned by a vehicle (or anything not the player) must never be in the player set. The same function also seeds the carried set from the held bone's system group (`Subject::includes`, `carry_collision.rs:36-40`), which for a character still attached to a vehicle can be the vehicle's group; restrict it to bodies owned by the held character. With collision restored, the jeep's deck carries the ragdoll and the hand can pull it up, as the reviewer expected. Item 1's damping keeps it from ringing while it is pulled.

### 4. Thrown C4 ignores the moving frame (556.8, 560.57)

#### What happened

Both charges were thrown from the jeep's stunt position at about 10.6 m/s. The `throw:` events: at 556.57 the charge left at exactly 15.00 m/s (10.98, 10.02, 2.01); at 559.92 at 11.39 m/s (7.86, 6.94, 4.46). The vehicle was moving at (8.35, −1.45, 6.38) and (9.03, −1.31, 5.37). Along the vehicle's heading the second charge had 9.0 m/s against the vehicle's 10.5, so it fell behind by about 2.5 m over its 1.7 s flight, landing behind at 561.5 as the reviewer saw. The controller history at that release shows the hand moving about 6.6 m/s horizontally and only about 1 m/s up in the cockpit, while the release has 6.9 m/s up in the world, more than 8 m/s above the vehicle's vertical: the inherited part of the release was wrong, not missing.

#### Root causes

- The inherited motion is a single-tick finite difference sampled at the wrong rate (verified). `weapons/hold/act.rs:847-866` samples the rig origin from `hooks::camera::eye_point()` on every input tick (`weapons/hold/mod.rs:235-252`, render rate, wall-clock dt), and `ReleaseTransport::update` differences consecutive samples (`weapons/hold/release_diagnostics.rs:66-101`). The camera's pose only advances on simulation ticks. In the dump around 559.3 to 560.0 the head's world velocity per frame reads 0.0 on frames without a sim tick and 12 to 22 m/s on the frames that follow, against a true 10.6 m/s. `estimate_release` adds whichever value the latest sample has (`release_diagnostics.rs:216-223`, 289-292), so the inherited velocity is anything from zero to about twice the vehicle's, and its vertical component carries the same aliasing.
- The throw cap clamps the world velocity, inherited motion included (verified). `release_diagnostics.rs:185-202` (`WorldMotion::capped`) applies `MAX_THROW_MPS` (15 m/s, `weapons/hold/mod.rs:118`) to the sum, so on any vehicle faster than about 15 m/s a throw cannot even keep the vehicle's speed, and the first charge here was clamped (15.00 exactly) with the vehicle's share shrunk along with the hand's.
- The same estimator serves grenades (`weapons/grenade/mod.rs:133-159` calls `act::release_at` then caps) and gun throws. Props and ragdolls thrown from the physical hands use the palm's smoothed world velocity (`physical_hands/grasp.rs:240-245`), which is sampled at sim rate and smoothed, so it does not alias, but it also caps the world velocity at 25 m/s, inherited motion included.

#### Fix proposal

- Take the inherited velocity from the carrier, not from differencing the camera: the player character's `m_MeasuredWorldVelocity` (what `physical_hands/tick.rs:527` already feeds the hands as `carrier_velocity`), or the ground or vehicle body's velocity at the release point (linear plus angular cross offset) when the player stands on or sits in a vehicle. If differencing is kept, difference only across simulation ticks with the simulation's dt, and smooth over two or three ticks.
- Cap only the hand's relative velocity: `world = inherited + clamp(relative, MAX_THROW_MPS)` in `capped` (and the same in `grasp::throw_velocity`, which needs the carrier passed in).
- The same change fixes grenades. Worth a test in `release_diagnostics` tests: a rig moving at 20 m/s with a 5 m/s throw leaves at 25 m/s; alternating zero and double rig samples give the same release.

### 5. The held barrel swings through Rico (1438.8 to 1442), and how held props could hit the drawn body

#### What happens now

`carry_collision.rs` disables collisions between every body of the held object (its component, or a whole ragdoll) and every player body through the pair filter (`PhysicsSystem::m_PairCollisionFilter`, `disableCollisionsBetween`), so a held prop passes through Rico; only the keep-out (`hold.rs`, `PlayerKeepOut`) moves the target, and only the object's centre, so a long barrel's end swings through the torso.

#### Does Rico have bodies while animated?

ragdoll.md says a character's ragdoll exists whether it ragdolls or not and its bodies follow the animated pose while the owner controls them, and character-grabs.md notes that a standing NPC's ragdoll bodies are hit by the reach rays. For the player the evidence says otherwise: every carry-collision log line in the take lists the player's bodies as only 650 (the proxy capsule, layer `CHARACTER_COLLISION`, filter `0x00060006`), 651 (unidentified), and the two hand bodies, and the tracking's nearby-body listing never shows bodies at Rico's bones. A keyframed ragdoll owned by the player in the player's group would appear in both. So either Rico's ragdoll bodies are only in the world while he ragdolls, or they are owned by something other than the character and outside his group. The layer list has `CHARACTER_RAGDOLL_KEYFRAMED` and `CHARACTER_RAGDOLL_PROXIES` (`PfxLayer`), which suggests animated ragdolls and hit proxies have their own layers. Even if Rico's keyframed bodies exist, they would follow the animated pose, not the IK'd body the mod draws. RE needed (IDA): when `RagdollInstance` adds its bodies to the world and with which layer and motion type; whether the player's are added while animated; which layers the physics system's constructor pairs with `CHARACTER_RAGDOLL_KEYFRAMED` (does `DYNAMIC` collide with it?); and at which pose stage the keyframed bodies read their targets (before or after the mod's IK).

#### Concrete design

Make the drawn body's own collision proxies, using the hand-body recipe the mod already ships:

- A handful of capsules from the drawn skeleton: chest, belly, pelvis, upper legs, upper arms (no head, so the camera never meets one). The procedural torso already builds chest and belly capsules in model space (`body::procedural::torso_keep_out`), which can seed them.
- Made like the hand bodies (havok-bodies.md, "Making a free dynamic body"), then switched to keyframed with `PfxRigidBody::SetMotionType` (`PfxMotionType::KEYFRAMED`), so collisions never push them and they never push the player. Each pre-simulation step sets their velocities to reach the drawn pose's capsules at the end of the step (`hknpWorld::setBodyVelocity`), as the hand drive does.
- Layer `WEAPONS`, in the player's system group with a zero subsystem id, exactly like the hand bodies. In the player's group they never touch the player's capsule or the hand bodies (same group, zero subsystem ids). On `WEAPONS` they meet `DYNAMIC` props, `CHARACTER_RAGDOLL` (so a held NPC ragdoll stays out of Rico, fixing part of item 1), destruction, other characters' capsules, and vehicles, and the aim ray (`RAYCAST_EVERYTHING_PHYSICAL`) and the trigger and explosion queries pass through them, so bullets and the aim never hit Rico's proxies. `CHARACTER_ATTACHED_DYNAMIC_RB` would also avoid the player capsule but does not collide with ragdolls.
- `carry_collision.rs` keeps disabling the held bodies against the proxy capsule (650) and the hand bodies, and leaves the drawn-body proxies out of the player set (by id), so held props hit them.
- Consequences to handle: a held prop pushed into the body stops there, so the hand lags; that lag must not push the player (`impacts::obstruction` and the lag push in `physics/step.rs:333-342`) and must not count toward the tear's stuck timer while the obstruction is the player's own proxy. Contacts with the proxies must be excluded from strike haptics and the impact sounds (`body::in_player_group` already excludes the group). Hip and leg proxies should be shorter than the drawn limbs so holstering and pouch reaches stay unobstructed, and suppressed while a hand is docking into a socket. With the proxies in place the keep-out can shrink.
- Walking into a carried prop: because the proxies are keyframed and the prop is carried by the hand, walking forward pushes the prop ahead rather than stopping the player, which is the behaviour the current suppression was protecting.

This matches the binding answer (held props collide with the drawn body, not the movement capsule) and conflicts with nothing else in this area.

### 6. Right hand and gun stutter while moving (3715.6 to 3718.2)

#### What happened

The right hand held a 6 kg launcher, joined to the hand body (gun body 4725, `held guns: made a gun body and joined it`, 3713.37), while the player jogged at 4.6 to 4.8 m/s. The late update's `physical_displacement_m` for the right hand (how far the drawn hand is moved off the controller to the body) jumped frame to frame between 1 and 11 cm: median 4.8 cm on frames without a sim tick and 2.9 cm on frames with one. The player stopped at 3717.7 (`S_RUN_TO_STOP_TURN_0`); the offset peaked at 23 cm during the stop and then settled to 2 to 4 mm. When the player ran again from 3718.2 at 5.4 m/s (faster than before), the offset stayed at 0.2 to 1.2 cm (median 0.2 to 0.3 cm). The hand body's own speed during the first run swung 4.2 to 6.2 m/s around the player's 4.7, so the body itself was oscillating, not only its drawing. Frames (`g3715.jpg`) show the launcher shifting between frames until the stop.

#### Hypothesis

Two parts. First, the joined hand-and-gun drive was in a self-sustaining oscillation, the same family as item 1 (a dead-beat drive on a 3 kg hand body carrying a 6 kg gun at the end of a lever, excited by the carrier's acceleration as the player started running); stopping let it decay, and the second run started from rest and did not excite it. The data do not say what first excited it (the gun was handed between hands and re-jointed at 3713.08 to 3713.37 and met a wall at 3713.99). Second, the offset is sampled once per simulation step and applied at render rate (render-time-anchoring.md, "Moving-frame sampling"), so whatever the body's error is, it reads stale on frames without a sim tick, which is why the non-tick frames read about 2 cm worse. Movement alone is not the cause: the second run at a higher speed was clean.

#### Fix proposal

- Apply item 1's soft, damped joined drive to guns too (`physics/guns/step.rs`, `gun_tuning` in `physics/state.rs`): damp against the hand-plus-gun velocity, not the hand's alone.
- Record the hand body's velocity relative to the carrier and the offset sample's age in the tracking, so the next session can separate a body oscillation from a stale sample.
- Consider drawing the hand from the body's pose interpolated between the last two steps, rather than the latest controller pose plus an offset frozen at the step.

### Cross-area and conflicts

- Item 5's proxies also keep held NPC ragdolls (item 1) out of the camera, and need the procedural body's capsules (body area).
- Item 3's fix changes what the carry collision treats as the player when standing on a vehicle; hand bodies in the vehicle's group already never touch that vehicle (they share the group), which may matter for the gunner-seat grips (reviewer's 536 to 550 item, vehicles area).
- character-grabs.md's "Unverified in the headset" can now be settled: the get-up fires in the grip after about 6 s, and topples can fail silently on allies and on tripod gunners. held-prop-impacts.md is stale: it says the observer deals no reaction, but `be62f8fa` added the supplemental strike.

## Hands on a moving roof (510-519 s) and the popping object (388.682 s)

Scratch evidence: tracking dumps `dump505.jsonl` (t 505-522) and `dump387.jsonl` (t 387-390), contact sheets under `stunt/a`, `stunt/b`, `stunt/p`, all in this folder.

### 1. 510-519 s: the hands snap back and forth

#### What happened

From t 503.35 to 534.55 the game holds the player in the "standing on a vehicle" stunt position (log lines 1781 and 1861). There is no stunt enter/exit and no other state change between 509 and 520: the log is silent apart from health lines, and every `controller_state` in the window reports mode `on_foot`, tier "on foot", both hands `free`, IK weight 1 and queued, late update `applied` (419 of 419 frames). The car carrying the player drives at 10.2-10.4 m/s (body 650's velocity, the vehicle `step_m` of 0.336 m per tick). The right hand holds the drawn rifle (drawn at 506.3, its gun body 10387 joined to hand body 5998 at 506.45); the left hand is empty.

The right hand's late-update `physical_displacement_m` (the distance between the tick's IK reference wrist and the latest tracked wrist) swings every frame between about 0.005 and 0.19 m, while the left hand stays at 0.001-0.01 m. The video agrees: the rifle and right forearm jitter while the scene is otherwise smooth (the 12 fps sheets in `stunt/b/d_01.jpg`; individual 60 fps frames are hard to read because of muzzle flashes and the moving car).

The physics step data shows where it comes from. Splitting the hand-body steps between 509 and 520 by the gap since the previous step:

| Step | n | Left (free) body lead along travel | Gun body lead |
|---|---|---|---|
| after a frame that ran no sim tick (gap > 40 ms) | 89 | 0.368 m | -0.442 m |
| regular | 241 | 0.098 m | -0.510 m |

On every step that follows a render frame without a tick (the 42 fps frame rate against the 30 Hz tick gives a `ticks` pattern of 1, 1, 0), both hand bodies are driven about 0.27 m further forward along the car's travel, which is 10.2 m/s times one extra frame (about 26 ms). The free left body reaches its goal, so its offset against the sent palm stays near zero and the drawn left hand does not move. The right hand's gun assembly is force-capped (`gun_tuning`) and only covers about 0.07 m of the jump, so its body falls 0.2 m short of the sent palm for one step and catches up on the next. The drawn right hand follows its body (`render_on_gun`, sampled by `sample_render_offsets` against the sent palm), so it snaps back and forth along the direction of travel several times a second.

#### Root cause

The stunt position is not involved: `physical_hands/stunt` drives nothing while standing (`solve.rs:113` holds, and the module doc says the game moves the player), and neither the arm IK, the tier, nor a vehicle grip changes in the window. The alternation is between two kinds of hand-body step, caused by the lead in the pre-simulation drive:

- `payload/src/controllers/physical_hands/physics/step.rs:146-150`: `age_s` is the real time since the palms were published, and `lead_s = age_s * time_scale + step_s`.
- `payload/src/controllers/physical_hands/physics/step.rs:255`: `target.led(lead_s)`, which (`physics/state.rs:203`) extrapolates the palm by its whole world velocity, `self.linear * lead_s`, and that velocity includes the carrier (the player riding the car).
- The palms are published once per tick (`physics/observe.rs:102`, run from `tick.rs:107` only when a tick was noted), so after a frame with no tick the next step finds a published palm one frame older. The hand's own motion really does age in real time, but the carrier's travel does not: the character moves exactly one tick per step whatever the wall clock did. The extra frame of age is multiplied by the car's speed and lands on the goal as a 0.26-0.3 m forward jump on those steps.

The comment at `step.rs:149` ("The palms' target ages in real time while its velocity is the world's") states the assumption that breaks on a moving carrier. On foot at walking speed the error is a few centimetres and invisible, which is why it shows only on the roof of a fast car (and would on any moving platform, in a seat, or falling).

#### Fix

Split the lead into the carrier's part and the hand's own part in `step.rs`:

- carrier travel: `target.carrier * steps_ahead * step_s`, with `steps_ahead = step_number - published_step` (normally 1, since the palms are published after each tick and the step runs at the start of the next), independent of wall-clock age;
- the hand's own motion: `(target.linear - target.carrier) * (age_s * time_scale + step_s)`, as now.

That needs `Target::led` (`physics/state.rs:203`) to take both leads, and `sample_render_offsets` (`physics/observe.rs`, it measures against `sent_palm`) keeps working unchanged because it already compares the body with the palm that was actually sent. Add a unit test next to the drive tests: a palm carried at 10 m/s, published once per tick, stepped after a skipped frame, must give the same goal as a step without the skip. With the goal no longer jumping, the gun hand's force cap stops showing as a 0.2 m oscillation; optionally also feed the carrier forward in the render offset so a gun hand on a moving carrier is not drawn with a steady lag (`gun_tuning` makes the gun body trail its palm by ~0.5 m here even on regular steps, which the late update hides only because it measures against the sent palm).

Not a conflict with any binding answer. It also likely contributes to item 556.8/560.57 s (throws on a moving frame) and to holding anything heavy on a vehicle, so the vehicle-frame analysts should know.

### 2. 388.682 s: an object on the body pops in and out

#### What happened

The player is in wingsuit flight from t 360 to about 396, at about 47 m/s (`character.velocity_mps` (-20.4, -17.9, 38.2)), so the body moves 1.57 m per tick. In the frames around v 386.8-388.5 (t 388.7-390.4) a dark object appears for a single rendered frame and is gone in the next, about every 0.1-0.17 s (`stunt/p/s1.jpg`, and the full-resolution `stunt/p/s2.jpg`). At full resolution there are two objects: a pistol, floating a metre or two ahead of the camera (frames 1, 7, and 8 of `s2.jpg`), and the rifle's receiver and ribbed handguard close to the camera (frames 0 and 6). The loadout at that moment (log lines 1163-1501) has the long gun #2 docked on the right shoulder (its slot's own dock, `loadout/socket.rs:68`) and the lone pistol #4 docked on the left hip (off its slot's dock, so placed by `inventory::carry::place` with `place_again` and `shift`); the carry set is empty (`carried=0` at 396.6). No log line in 360-396 touches weapons, visibility, or the loadout.

So the object is the holstered weapons themselves, both of them: the shoulder rifle and the hip pistol. Lying prone in the wingsuit with the camera at the head, both sockets are behind the camera and out of view when drawn where they belong; on the popping frames they are drawn ahead of it.

The camera is not the cause: the camera's rendered position minus the character's position interpolated by the same fraction stays constant to the centimetre on every frame, ticked or not (computed from `controller_state.view.camera` and the per-tick `pose` events).

Mapping the video to rendered frames (`frames.jsonl` `present` = `mirror_frame` in the tracking), each large detection (dark-pixel count above 1000) falls on a rendered frame that ran no simulation tick, or one present after such a frame within the capture's latency uncertainty: mirror frames 27619/27620, 27622/27623, 27626/27627, 27632, 27645, 27652, 27659, 27663, and 27668. About 4 of the 13 tickless frames in that stretch show nothing. There is no correlation with the interpolation fraction itself (visible frames average 0.43-0.58, hidden 0.40-0.50 for every latency tried).

#### Root cause (hypothesis, not verified)

Which code moves the guns on a tickless frame is not established. What is established:

- `WeaponBase::UpdateRender` (checked in the IDB) does nothing but lerp each gun's `WeaponData::m_WorldMatrixT0` toward `m_WorldMatrixT1` by the render context's fraction and push the result, so any pop comes from the pair holding the wrong values at render time on those frames (or from the render update not running for the weapon on them).
- The engine documents that `WeaponData::UpdateAttachmentTransforms` builds a gun "from the holder's reference and head bones and its flight direction" while wingsuiting, a path the mod's placement code never considers.
- The game-docked rifle pops as well as the mod-placed pistol, so the cause is not specific to `carry.rs`'s `shift` or `place_again`.

The leading hypothesis is that some placement runs outside the tick on tickless frames during wingsuit flight (an engine render-phase or wingsuit-specific placement, or a non-advancing placement), leaving `T0 == T1` or `T1` a tick ahead, so the gun is drawn up to 1.57 m ahead of the body. That is the "tick ahead" pop the carry set's `end_pass` was written to prevent (`docs/mod/rendering/render-time-anchoring.md`, "the rifle pops at speed"), here reappearing for slot weapons in the wingsuit. A second possibility is that the weapon render update skips tickless frames while the hip and back bones' skinning does not, leaving the model matrix one frame stale. At 47 m/s a stale frame is 1.2 m of travel, though the sign is wrong for that alone to bring a gun into view ahead.

#### Fix proposal

1. Confirm with a short diagnostic before changing behaviour: log, per rendered frame during wingsuit flight, each docked gun's `m_WorldMatrixT0`/`T1` translations relative to the character's `m_WorldMatrixT0`/`T1`, its `m_SkipRenderInterpolation`, the weapon state, the frame's `ticks`, and whether `WeaponBase::UpdateRender` ran that frame (counter in a pass-through detour beside `hands/late/weapon.rs`). The docked-guns overlay (`controllers/weapons/inventory/docked_view.rs`) shows T1 and the visible flag and can be turned on for a quick headset check.
2. RE the wingsuit branch of `WeaponData::UpdateAttachmentTransforms` and find whether and how the character's update calls it outside the sim tick (render phase or a second pass); bind what is found in `equipment/weapon.pyxis`.
3. Then fix along the lines `pouch_models` already uses for its own instances: for holstered guns on the local player, re-push the model matrix at render time as the gun's placement in the character's model space between its last two ticks, carried by the character's `m_WorldMatrixT0`/`T1` at the frame's fraction (a post-call on `WeaponBase::UpdateRender` for holstered guns, in `controllers/weapons/inventory/` next to `carry.rs`). That makes docked guns ride the drawn body whatever wrote their matrices, fixing native and mod-placed sockets alike.

Related: the docs' headset checklist item "fall with a rifle and a pistol carried on the body and watch them stay on their sockets" should include wingsuit flight and slot weapons, not only carry-set ones.
