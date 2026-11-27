# The grapple

The game's grapple is one button and a reel: fire at a point and zip to it, or tether two things together. The mod keeps all of that and lets the hand work the line: it aims from the hand, pulls the reel, locks the line and swings from it, hangs below ceilings and helicopters, takes the line with the other hand, fires from a vehicle seat, and pulls things into the hand. This page is the one home for the grapple's behaviour in the mod. The snatch's detail is [snatch.md](snatch.md), and the game's own machinery is [grapple-pipeline.md](../../engine/gameplay/grapple-pipeline.md).

The grapple button is the off-hand's upper face button (Y on a left Touch, B on a left Index) on foot, falling, and under the parachute, the left upper face button in the wingsuit, and the off-hand stick click in a vehicle seat ([bindings.md](../input/bindings.md#default-layout)). The grapple hand is the off-hand. The device stays on Rico's left forearm and is drawn on the tracked forearm every frame (`grapple::device`).

## What the player can do

Firing and pulling:

- Pressing the grapple button fires along the grapple hand's aim. A tap zips Rico to what the hook hits, and a hold released at a second target tethers the two, as in the game.
- With a line attached, a yank (a sharp pull of the grapple hand back along its aim) while the button is held zips, as a tap does.
- During the reel, pulling the grapple hand back speeds it up, and a yank boosts it once the game's retract boost is unlocked.

Tethers:

- A double tap of the grapple button releases every tether.
- Holding the button with nothing new under the aim tightens the tethers, and pulling the hand back while holding tightens them harder. The off-hand trigger retracts them as in the game, or the off-hand stick click while that hand holds a gun.

Locking and swinging:

- Pressing the grapple button during the reel locks the line's length, and Rico swings from it as from a rope, on the world, an object, or a vehicle, which feels the pull.
- Pulling the grapple hand back takes line in, and moving it across the line pumps the swing.
- A quick release resumes the reel, firing at a new target if the aim is on one. A longer hold's release or a jump lets go, and Rico flies on with the swing's speed. A yank resumes and boosts the reel.

Hanging:

- A reel onto a ceiling or onto a helicopter's hang point leaves Rico hanging upright below it on a short line, with the other hand free. The grapple hand pumps the swing and takes line in; a release, a yank, or a jump lets go.
- Below a helicopter, the use button pulls Rico up onto its skid.
- In the game's own grapple hang, moving the grapple hand swings the body.

Holding the line:

- While the line is locked, or the snatch's line swings something, the other hand's grip takes the line ahead of the grapple hand. Pulling it back takes line in, or draws the swung thing in.

Snatching:

- With the grapple hand empty and nothing in reach, aiming it at a weapon, a loose object, a grenade, a placed charge, or a car door, squeezing its grip, and yanking back pulls the thing to Rico. Whichever free hand is nearest its path catches it by closing its grip, and a gun arrives ready to shoot.
- A heavy object or a ragdoll swings on the line instead, and holding the grapple button after the grip swings a light one too.
- A tap of the grapple button at a gun tosses the gun to Rico's feet.

Vehicles and flight:

- In a vehicle seat, the grapple button fires along the grapple hand's aim and tethers what the hook holds to the vehicle. Riding in a car seat, the snatch works too.
- A reel that ends in the air carries its speed into the fall, the wingsuit, and the parachute. Holding the grapple button in the wingsuit holds the arms' steering where it was.

Gamepad play:

- Under the gamepad scheme the grapple is the game's own, aimed along the gaze, and nothing above that needs the controllers runs: no hand aim, lock, hang, line grab, snatch, or seat grapple. The comfort hold still applies.

Comfort:

- From the fire to the landing the view is held level and still, so the game's turns of Rico toward the hook never swing it. The pause menu's `VIEW ON THE GRAPPLE` picks the hold, level pitch only, or off.

## States

| State | Entered by | The grapple button | Left by |
| --- | --- | --- | --- |
| Idle | | The game's: a tap fires, a hold tethers | A fire, or a grip on a snatch target |
| Hook in flight | A fire | Passed to the game | The bite, or a miss |
| Reel | The bite of a tap | A press locks | The arrival, the lock, or the hang |
| Locked swing | A press during the reel | Held, it keeps the lock | A quick release or a yank (back to the reel); a long release, a jump, or a break (free flight); the reel ending |
| Hang | The reel's arrival at a ceiling or a helicopter's hang entry | Any release lets go | A release, a yank, a jump, or a landing (free flight); the use button below a helicopter (the skid) |
| Game attachment | The reel's arrival anywhere else: a perch, the game's hang, a wall | The game's | The game's |
| Tethered | A hold released at a second target | Double tap releases, a hold tightens, a tap fires | The release |
| Seat hook | A press in a vehicle seat | Held, the wire's car end follows the hand | The bite (a tether), or the call back |
| Snatch | A grip on a remote target | The snatch's | The catch, a miss, or a cancel ([snatch.md](snatch.md#the-interaction)) |

## The button

While nothing is attached the game reads the grapple button as the gamepad's. With tethers or a line attached, `controllers::grapple_button` reads the gesture first: a tap fires once no second tap follows within `button.double_tap_s`; a double tap releases every tether (on foot and in the wingsuit it is also the gamepad's B, which cancels and steps off a ledge); a hold on a new target is the game's tether; a hold with nothing new under the aim tightens the tethers from `button.tighten_start` to `tighten_held` over `tighten_ramp_s`; and a yank while held zips.

While the hook reels in on a line to anything but a character, from the wind-up on, the button is the lock's, and the game never sees a held fire: one held past the game's hold time would arm the dual tether, whose release ends the reel, and a fire onto the attached line can release it. The hand's pulls act only while the button is held, so a hand moved back in play never zips or tightens. A press the snatch claimed or another traversal spent (the wingsuit's gesture deploy) is ignored until the button reads up. The controls summary shows the release, the tightening, and, during a lockable reel, the lock in the tightening's place ([bindings.md](../input/bindings.md#the-controls-summary)).

## The aim

The grapple slot ([`TargetType::TARGET_GRAPPLE`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/aim::target::TargetType##variant-TARGET_GRAPPLE)) follows a ray from the grapple hand under the controller scheme (`jc3vr_body::controllers::aim::grapple`). Everything that fills it reads [`PlayerAimControl::GetAdjustedCameraMatrix`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/aim::aim::PlayerAimControl##func-GetAdjustedCameraMatrix) or [`GameCameraManager::GetCameraMatrix`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/camera::game_camera_manager::GameCameraManager##func-GetCameraMatrix), so while a thread-local override is set both return the grapple ray and the engine targets with its own filter, range, and scoring. [`PlayerAimControl::UpdateDirectAim`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/aim::aim::PlayerAimControl##func-UpdateDirectAim) runs twice, once under the override keeping only the grapple slot, because one raycast in it serves the weapon and grapple slots together; the fitness and last-valid checks run under the override too. The visibility check ([`PlayerAimControl::IsTargetVisibleCast`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/aim::aim::PlayerAimControl##func-IsTargetVisibleCast)) is a synchronous cast from the hand, because the engine's one scheduled raycast per target would leak the hand into the camera-based types.

The assist (`aim::assist::grapple_assist`, `assist.grapple_cone_deg` in `crates/jc3vr-body/tuning/aim.toml`) turns the ray onto the attachable object nearest it within a cone when the ray hits no object; surfaces are left to the last-valid cache. The readout is current only while [`GrapplingHook::GetFireInputEnabled`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/equipment::grappling_hook::GrapplingHook##func-GetFireInputEnabled) holds. The aim also sees weapons, for the snatch ([snatch.md](snatch.md#targeting)), and an overlay ring replaces the game's reticle ([hands-and-aiming.md](../input/hands-and-aiming.md#the-games-reticles)). The engine side is [grapple-pipeline.md, "Targeting"](../../engine/gameplay/grapple-pipeline.md#targeting).

## The hand on the line

`grapple::physical` reads the grapple hand on top of the buttons, so a reel the hand does not drive runs on the game's own tuning. Unless noted, the keys below are in `crates/jc3vr-grapple/tuning/grapple.toml` ([tuning.md](../tuning.md)).

The zip's yank (`controllers::yank::LineYank`) is a drop of `grapple_yank.min_distance_m` in reach along the aim the grapple hand had when it armed, at `grapple_yank.min_speed_mps`, in the cockpit frame. Measured along the aim rather than toward the head, a seated player's hand, which pulls back past its side, still yanks. A pull carrying the hand out from the head across the ground by more than `yank.outward_share` is not a yank, since spreading the arms for the wingsuit drops reach along an aim across the body as fast. Every armed gesture on the line latches its line this way (`controllers::yank::ArmedLine`).

The pull (`physical::pull::LinePull`) arms when the reel starts or the tether retract arms, takes the aim of its first tracked tick as its line, and reads the pull's speed, peak-held over `physical.pull.speed_hold_s` so a sharp pull counts through a reel longer than the arm, and its depth, from a reference that jumps out with the hand and relaxes at `physical.pull.relax_mps`.

The reel's detour on [`NStateTask_ReelIn::UpdateReelVelocity`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/equipment::grappling_hook::NStateTask_ReelIn##func-UpdateReelVelocity) hands the local player's update a copy of the [`GrappleTuning`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/equipment::grappling_hook::GrappleTuning) scaled by the pull's speed from `physical.reel.min_scale` to `max_scale` (`physical::reel`); the shared resource is never written. A drive older than `physical.reel.drive_max_age_ms` is ignored. A yank during the reel boosts it for `physical.reel.boost_s` of game time with the retract's top speed and acceleration, as holding the retract does in the game, only once [`GrapplingHook::GetRetractAcceleratesReelIn`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/equipment::grappling_hook::GrapplingHook##func-GetRetractAcceleratesReelIn) says the boost is unlocked, and without touching the retract input, so it never squeezes tethers.

The tether retract (`physical::tether`) arms while the button is held with dual tethers attached, no line of the grapple's own, nothing carrying Rico, and Rico on the ground or falling, since the retract input also pulls a light object on the line and boosts the wingsuit and parachute. Past `physical.tether.deadzone_m`, the pull's depth becomes the cube root of its fraction of `full_m`, because the game squeezes by the cube of the value ([grapple-pipeline.md, "The tether squeeze"](../../engine/gameplay/grapple-pipeline.md#the-tether-squeeze)). It joins the analog `RETRACT_GRAPPLE` with the trigger and the button's tightening, the largest winning.

## The lock

The lock (`grapple::physical::lock`, keys in the `lock` section) holds the line's length while the reel carries Rico. A press in the reel's wind-up only arms it, since a lock before the reel carries him would pin him to the ground. A release within `button.tap_s` resumes the reel, and with the aim on a target farther than `bindings.grapple_retarget_m` (`crates/jc3vr-gameplay/tuning/gameplay.toml`) from the hook also fires at it, which the game takes as a retarget.

The lock stays inside the game's reel, so the reel's arm animation, its turn of Rico toward the line, the comfort hold, and the anchored grapple hand carry on. The reel's velocity detour hands the reel the lock's velocity (`lock::drive`), and the reel's arrival and stuck check are held off (`lock::hooks`), since the first would end a swing near the hook and the second drop Rico when he brushed a wall. The reel's motion turns him toward the line's level part, so a hook overhead hangs him upright.

Each reel tick, `lock::model::swing` adds gravity, which the reel's velocity lacks, and steps the rope (`grapple::rope::Rope`) between the hook's point and Rico: a tension-only spring-damper on the stretch past the held length (`stiffness_per_s2`, `damping_per_s`), normalised by the ends' reduced mass and capped at `max_accel_g` of Rico's weight. The hook's end moves with the attach point, so on a moving vehicle Rico keeps his offset from it. The snatch's swinging line is the same rope with the hand as its fixed end; the two never run at once.

Taking hold changes nothing about Rico's motion (`lock::model::Held::take`): while he still closes on the hook he coasts and the held length follows him in, so the line goes taut where his approach ends and nothing across the line is turned. A short line cannot swing a fast rider, so the coast brakes along the line alone to stop at the shortest length that bears his swing with `coast_strength_share` of the line's strength. A lock whose rider mass cannot be read is refused.

Pulling the grapple hand back along its aim at the take takes line in, at most `reel_in_mps`, down to `min_length_m` and never below `rope.min_held_length_m`; reaching out pays nothing out, so hand over hand keeps taking in. The hand's speed across a taut line pumps the swing against the hand's motion (`lock::model::Pump`), past `physical.swing.deadzone_mps` and up to `lock.pump.accel_g` at `physical.swing.full_mps`, with tracking jumps (`lock.pump.max_step_m`, `max_speed_mps`) ignored.

The rope shares its tension with the body behind the hook by inverse mass, and the impulse is applied on the next input tick, scaled by `reaction`: a parked car barely moves, a helicopter sways, a light object is dragged, and a static body is a fixed end. A character is never locked onto. The active wire holds no native constraint ([grapple-pipeline.md, "The far end's proxy"](../../engine/gameplay/grapple-pipeline.md#the-far-ends-proxy)), so the target never takes two rope forces.

A held swing lasts as long as the button does, dragging Rico along the ground if he touches down. A release after `button.tap_s` or a jump lets go; overstretch past `break_stretch_m`, a lost target, or a non-finite sample breaks the line; the reel ending or the bindings standing down hand the reel back to the game. Letting go retracts the wire, and the reel's cancel scales Rico's velocity down a world update later ([grapple-pipeline.md, "The reel's endings"](../../engine/gameplay/grapple-pipeline.md#the-reels-endings)), so the swing's last velocity is handed back within `handback_for_s` while he carries less than `handback_carried` of it. A reel that ends in the air carries its speed into the fall's and the wingsuit's speed cap, which `jc3vr_body::hooks::input::air` eases back over `air.carry_s` (`crates/jc3vr-body/tuning/input.toml`).

## The hang

A reel onto a ceiling would end in the game's upside-down cling ([`GrapplingHook::StartReelUpsideDown`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/equipment::grappling_hook::GrapplingHook##func-StartReelUpsideDown) from [`NStateTask_ReelIn::FinishReel`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/equipment::grappling_hook::NStateTask_ReelIn##func-FinishReel)), a pose the player's body cannot match. A detour holds it off for the local player, and the lock takes the line with no press (`lock::Hold::Hang`), paying out at `pay_out_mps` to `hang_length_m`. Rico hangs upright on the reel's animation with the other hand free. Near the hook the reel's arrival test passes every tick and skips the reel's motion ([`NStateTask_ReelIn::UpdateMotion`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/equipment::grappling_hook::NStateTask_ReelIn##func-UpdateMotion)), so the arrival detour runs the motion itself, or Rico would freeze under the hook.

A reel onto a helicopter whose chosen entry is the stunt hang ([`VehicleGrapplePointProxy::IsCurrentEntryHangNode`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/equipment::grapple_point_proxy::VehicleGrapplePointProxy##func-IsCurrentEntryHangNode)) hangs the same way, measured against the airframe, with the swing across the line damped toward its motion over `hang_swing_damping_s` while it moves faster than `moving_hook_mps`. A door-side arrival is the game's hijack and is left to it.

The use button below a helicopter pulls Rico up onto the skid (`jc3vr_gameplay::controllers::bindings::heli_hang`). It drops the use button's wingsuit, which would end the reel, keeps the press from the game until it is released, and re-points the reel at the door side: each entry tagged [`INTERACTION_USE_TAG_REEL_IN_DOOR_SIDE`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/interaction::INTERACTION_USE_TAG_REEL_IN_DOOR_SIDE) goes through [`VehicleGrapplePointProxy::UpdateVehicleReelInEntrySelection`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/equipment::grapple_point_proxy::VehicleGrapplePointProxy##func-UpdateVehicleReelInEntrySelection) with the stored goal cleared, and the one starting nearest Rico is kept. Helicopters have no door-side idle, so the game would drop him into the stunt hang at the clip's end ([`ValidateStuntPositionCondition::Evaluate`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/animation::rule_system::ValidateStuntPositionCondition##func-Evaluate)); that transition is refused, and the skid rail (`vehicles::rail` in `jc3vr-vehicles`) stands him on the skid until he steps off, jumps, lowers himself, or enters ([stunt-positions.md](../input/stunt-positions.md)).

The hold-off applies only while the lock's tick runs and has not refused a hang within `hang_refused_ms` (`lock::hang_takes_over`), since otherwise nothing would end the reel; that keeps the gamepad scheme on the game's own cling and hang. Every way out of a hang lets go of the line, because resuming the reel would only arrive at the ceiling again, except the use button below a helicopter.

## The game's grapple hang

Where the reel's arrival enters the game's own hang, the hang task's detour (`jc3vr_body::controllers::hands::anchor`) pushes the body against the grapple hand's displacement from where it took the line, and `grapple::physical::swing` adds a push against the hand's smoothed velocity relative to the head, scaled by `physical.swing.scale` to the game's full-stick swing ([`GrapplingHangTask_SmallImpulseSwingMagnitude`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/equipment::grappling_hook::GrapplingHangTask_SmallImpulseSwingMagnitude)) and capped together at `physical.swing.max_scale` of it per tick. On the reel and the hang the grapple hand is anchored, keeping its controller position and pointing at the wire, unless it holds something ([hands-and-aiming.md](../input/hands-and-aiming.md#hand-tiers)).

## The free hand on the line

While the lock holds or the snatch's line swings, `grapple::line_grab` offers the other hand one grab target, `GrabVolume::GrappleLine`, when that hand is free and empty and its palm is near the line ahead of the grapple hand's wrist (the `line` section; the near limit leaves the wrist to the snatch's brace). It ranks like any grab target ([grip.md](../input/grip.md)), a squeeze latches the grip on the line until it opens, and it is the one thing the snatch's reserved catch grip may still take. On the lock's line that hand's pull takes line in as the grapple hand's does (`lock::model::FreeHand`), the two adding, and only the grapple hand pumps. On the snatch's line the holding hand becomes the anchor ([snatch.md](snatch.md#swinging-on-the-line)).

## The seat grapple

The game takes no grapple fire in a seat, and ships a vehicle grapple that tethers what the hook hits to the vehicle but never calls it ([grapple-pipeline.md, "The grapple in a vehicle seat"](../../engine/gameplay/grapple-pipeline.md#the-grapple-in-a-vehicle-seat)). `grapple::seat` runs its steps from the input tick (keys in the `seat` section). One test, `jc3vr_body::controllers::facts::seat::stands_in` (riding per [`Character::IsInRidingInVehicleState`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/character::character::Character##func-IsInRidingInVehicleState) where the game refuses the fire), decides the button, the aim, and whether a flying hook is still seated. A seat stops the game refreshing the grapple slot, so there the aim casts the hand's ray itself, out to [`GrappleTuning::m_GrappleDistance`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/equipment::grappling_hook::GrappleTuning##field-m_GrappleDistance).

A press with nothing attached fires [`GrapplingHook::FireHookCustom`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/equipment::grappling_hook::GrapplingHook##func-FireHookCustom); with tethers attached the button behaves as on foot. The wire's near end moves to the vehicle at the grapple hand's grip and follows the hand while the button is held. Once the hook holds and the button is up, the wire becomes one of the hook's tethers ([`GrapplingHookWire::PushGrapplingHookWire`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/equipment::grappling_hook::GrapplingHookWire##func-PushGrapplingHookWire)) and is the game's from there; a long hold released at a second target ties the two instead. A hook still flying after `seat.flight_s`, or out when Rico leaves the seat or the controllers, is called back ([`GrapplingHook::CancelActiveWire`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/equipment::grappling_hook::GrapplingHook##func-CancelActiveWire)).

## Comfort

The player aims by looking, so the head pose already carries the turn toward the target; the engine then turns Rico's root toward it too (the fire act's alignment, then `NReeledInController::RotateToGrappleTarget`), and since the view composes the head onto the body, the turn lands twice, a rotation the inner ear never feels. The filter (`grapple/mod.rs`, the head pose's `BodyFrameFilter`, keys in the `comfort` section) removes the body's half and leaves the headset's tracking untouched ([head-and-body.md](../body/head-and-body.md)).

While the grapple is active, `filter_body_rotation` blends the body frame toward a filtered one per `headpose.grapple.mode`: `HoldView` (the default) holds the frame the previous render composed with, `LevelPitch` flattens the live frame to its yaw, and `Off` does nothing. Activity reads [`GrapplingHook`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/equipment::grappling_hook::GrapplingHook)'s fire flags (the body turns before any wire exists), `GHS_REELING_IN`, and the inactive or reeled states only with a live wire, since the state keeps the last reel's outcome long after. The engage is instant and holds the frame the player last saw, partly blended included; the release blends back over `release_s`.

Landing on foot under `HoldView`, the held heading is posted for the body-turn accumulator (`grapple::take_body_yaw_retarget`) and the hold stays until Rico's game turn converges on it (`handoff_converged_deg`, or `handoff_timeout_s`), so the view never sweeps through the yaw; everything that steers Rico reads the raw rotation (`headpose::xr::body_rotation_raw`). `filter_anchor` absorbs the landing animation's jump of the head anchor against the root (past `anchor_snap_m`, easing out over `anchor_snap_ease_s`, capped at `anchor_offset_max_m`, a step past `anchor_teleport_m` passing through), always as the raw anchor plus an offset so the engine's interpolation is undisturbed.

The filter advances on the input tick and every rendered frame. Its mutex is innermost in the head pose's SIM, BODY_YAW, FILTER order, so nothing under it may call a locking `headpose` accessor, or the game deadlocks. The debug UI's Grapple feature shows the blend and toggles a CSV capture of the filter into the session folder (`grapple/telemetry.rs`, [sessions.md](../sessions.md)).

## The body and the other systems

- While the lock holds, the firm ragdoll hangs Rico's hips and legs from the physics and the torso reads the case `grapple line locked` ([procedural-states.md](../body/procedural-states.md#the-firm-ragdoll)); a reel that leaves him slumped on a wall is posed by the torso ([procedural-states.md](../body/procedural-states.md#the-grapples-wall-attach)).
- The physical hands stand down on the grapple ([physical-hands.md](../input/physical-hands.md)), and the grapple hand's fist on a line never takes a socket ([grip.md](../input/grip.md)).
- While the lock holds or the snatch's line is out, the shooting permission is waived, so the other hand can fire ([physical-weapons.md](../input/physical-weapons.md#the-shooting-permission)).
- Under the parachute the look steer turns the canopy toward a reel's line, and an opening out of a reel skips its roll ([parachute.md](../body/parachute.md)); a stable flight pose on a yielding line deploys the wingsuit ([flight.md](../input/flight.md#opening-and-closing)).
- The lock's release and handback run before the world update, ahead of the movement task ([physics-substrate.md](../body/physics-substrate.md)).

## Feedback, readouts, and threading

The grapple hand plays `Event::GrappleFire`, `GrappleAttach`, `Yank`, `TetherRetract`, `LineLock`, `LineRelease`, and `LineSnap`, with `Sustain::GrappleTension` from the game's wire tension or the lock's, and `GrappleReel` while the hand drives the reel; the free hand on the lock's line feels the tension too ([haptics.md](../input/haptics.md)). The Status page's `Grapple` section shows the phase, target, reel scale, tethers, `Line lock`, `Seat grapple`, the hang swing, `Snatch`, and the line haptics, and the log's `grapple` target notes attaches, reels, boosts, retracts, the lock's take, refusal, and end, and the seat grapple ([recorder.md](../testing/recorder.md) records it each frame).

The bindings' game stage ([bindings.md](../input/bindings.md#injection-timing)) runs the button, the pull, the lock's tick, and the seat grapple; the reel, arrival, stuck-check, and hang detours run on the game thread's updates. Shared state sits behind leaf locks never held across a game call, and `jc3vr_body::controllers::facts::lock` tells other crates whether the lock holds ([threading.md](../threading.md)).

## Unverified

- That the proxy moves at the wanted velocity with no gravity of its own while the reel's game-controlled flag is set, and that its support reading is live during the reel.
- That the lock's impulse wakes a deactivated body, and how a helicopter's flight model answers a steady pull.
- That the retract's analog value drives the squeeze as the trigger's does.
- Whether the on-foot entry finds the door from a helicopter's skid.
