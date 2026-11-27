# Grapple lock

While the reel carries Rico toward the hook, the player can lock the line's length and swing from it. Holding the grapple button during the reel locks the line; pulling the grapple hand back shortens it; a yank resumes the reel; letting go of the button lets go of the line, and Rico flies on with his momentum. The line is a spring measured against the hook's point's own motion, so a lock onto a moving vehicle or a helicopter keeps Rico at a mostly fixed offset from it, and the target feels the line's pull back.

The implementation is `grapple::physical::lock` in `jc3vr-grapple`, with the rope shared with the snatch in `grapple::rope` and the button's gestures in `controllers::grapple_button`. The engine side is in [grapple-pipeline.md](../../engine/gameplay/grapple-pipeline.md), "The reel's motion and endings". The rest of the physical grapple is in [physical-grapple.md](physical-grapple.md). The lock has no switch, and acts only under the controller scheme.

## The button

The lock owns the grapple button while the hook reels in along a line whose far end is attached to anything but a character. It owns it from the hook's reeling-in state on, which takes in the reel's wind-up (the mode detector's pre-reel traversal, and in the parachute a stretch before it) as well as the reel itself. The lock takes hold only once the reel task carries Rico (the reeling-in traversal); a press in the wind-up arms it, and it takes hold as the reel starts if the button is still held. A press that reached the game a tick before the hook entered the reel (the hold of a quick tap and hold, landing as the hook attaches) is the lock's from the tick the reel starts, while it is still held: the game sees it come up, as for a tap.

| Press | Effect |
|---|---|
| Press | Locks the line at once, at the distance Rico is at; in the wind-up, as soon as the reel carries Rico |
| Release within `button.tap_s` | Unlocks and resumes the reel |
| Release after a longer hold | Lets go of the line: Rico flies on |
| A yank while held | Unlocks and resumes the reel, and the yank boosts it as during any reel |

The yank is the reel's boost yank ([physical-grapple.md](physical-grapple.md#the-reel)); the lock hears it directly from the physical grapple, whatever the button's gesture is doing, and the button only spends its press so that the release afterwards does nothing. The game never sees a held fire input during the wind-up or the reel: a fire held past the game's hold time with the reel's line attached would arm the dual tether, whose release fires the tether's second end and ends the reel. A quick release fires nothing onto the attached line, since a fire onto it can release it, and neither does a quick tap whose reel ended under it (a landing or a break). A double tap still releases tethers and does nothing to the reel's line. With the far end on a character, the button reaches the game as before.

## The swing

The lock stays inside the game's reel. The reel task keeps running, so the reel's animation (the arm thrust along the line, which reads as holding the rope), its turn of Rico toward the line, the [grapple comfort](../body/grapple-comfort.md) hold, and the anchored grapple hand all carry on. The detour on the reel's velocity update hands the reel the lock's velocity instead of the game's (`lock::drive`), and the reel's arrival and stuck check are held off while the lock holds (`lock::hooks`): the arrival would end a swing near the hook, and the stuck check would drop or reattach Rico the moment he brushed a wall, since a held line never closes on the target.

Each reel tick, `lock::model::swing`:

1. adds gravity to Rico's velocity, which the reel's velocity carries none of;
2. while the take runs, leaves Rico's velocity alone and lets the held length follow him in, so the line goes taut where his approach to the hook's point ends;
3. steps the shared rope between the hook's point and Rico.

The rope (`grapple::rope::Rope`) is tension only: a spring-damper on the stretch past the held length, with the damping on the ends parting along the line, normalised by the ends' reduced mass so that the same stretch asks the same acceleration whatever the masses, and capped at a tension. The hook's end moves with the attach point's own velocity, as the attachment proxy reports it, so on a static wall it is a pendulum, and under a helicopter the line holds Rico at his distance from the point in the helicopter's frame.

The tuning is the `lock` section of the grapple's tuning file (`crates/jc3vr-grapple/tuning/grapple.toml`, [tuning.md](../tuning.md)); `LockTuning` in `grapple::physical::config` says what each key does. The main ones are `stiffness_per_s2` and `damping_per_s` (the spring), `max_accel_g` (the line's strength, in multiples of gravity on Rico's mass, so a fast relative motion never catapults him), `break_stretch_m`, `reel_in_mps` and `min_length_m` (the hand's take-in), `reaction` (the share of the pull the target feels), and `hang_length_m`; the rest tune the coast, the hang's pay-out and damping, the release's hand-back, the hang gate's windows, and the pump.

### The take

Taking hold changes nothing about Rico's motion (`lock::model::Held::take`). While he still closes on the hook's point, measured against the point's own motion, he coasts on his own momentum under gravity, and the held length follows him in. The line therefore goes taut the moment his approach ends: his speed along the line is zero then, so the rope takes over with no jerk. His speed across the line is never turned, so the swing goes where he was going: a zip up toward a high hook carries on up toward it, and a rush at a level hook falls under it, the line catching on the length that is left.

A line cannot swing a fast rider on a short length: it turns the speed across it at that speed squared over the length, plus gravity at the swing's bottom, and past `max_accel_g` it stretches and breaks. Rico's speed across the line grows as he closes in, keeping his momentum about the hook, so the coast works out the shortest length the line bears his swing on with `coast_strength_share` of its strength, and stops there: it is braked, along the line alone, from the point where stopping at that length (or at `min_length_m`, whichever is longer) takes more than `coast_brake_g`, and a rider already inside it ends the coast at once. A level rush therefore loses some of its speed along the line and swings on a longer line rather than snapping it, and a rush straight up under the hook stops there and hangs.

The speed kept is capped by what the line holds at the bottom of the swing it starts, where a pendulum pulls hardest: the cap is the speed whose swing reaches the bottom with the line at its strength. Without it a fast entry would stretch the line to its break.

The reel's own velocity carries the velocity Rico brought into it, blending from it to the attach point's over the reel's first moments ([the reel's velocity](../../engine/gameplay/grapple-pipeline.md#the-reels-velocity)), so a lock taken as the reel starts carries a fall or a run into the swing as well; how much of it is left by the time the reel task carries Rico is unverified.

Rico's mass is his proxy body's, read from its motion when the lock takes hold; a lock whose mass cannot be read is refused, and the button's tap still goes to the game.

## The hang below a ceiling or a helicopter

A reel onto a ceiling ends, in the game, in the upside-down attach: the arrival ([`NStateTask_ReelIn::FinishReel`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/equipment::grappling_hook::NStateTask_ReelIn##func-FinishReel)) chooses it from the attachment setup and starts it ([`GrapplingHook::StartReelUpsideDown`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/equipment::grappling_hook::GrapplingHook##func-StartReelUpsideDown)), and Rico clings to the surface inverted, a pose the player's body cannot match. A detour holds that start off for the local player, the reel keeps running, and the lock's next tick takes the line itself, with no press (`lock::Hold::Hang`). The line is held from Rico's distance and pays out at `pay_out_mps` to `hang_length_m` from the hook to Rico's root, and he starts at rest against the hook, as the game's own ceiling attach clears his velocities, rather than his rush turning into a swing.

He hangs below the hook, upright, on the reel's animation as in any lock: while the lock holds, the reel's motion ([`NStateTask_ReelIn::UpdateMotion`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/equipment::grappling_hook::NStateTask_ReelIn##func-UpdateMotion)) turns him toward the line's level part, or keeps his own facing for a line near vertical, rather than up the line. Near the hook the reel's arrival test passes on every tick and the arrival skips that motion, so the lock's arrival detour runs the motion itself, with the lock's velocity and the reel task's instance from its last motion; otherwise the proxy would never move and Rico would freeze under the hook with the line slack. The grapple hand pumps the swing and takes line in by pulling back, and the other hand is free.

A reel onto a helicopter is hung the same way. Its arrival would take Rico into the game's stunt hang under the airframe, whose states are the grapple's upside-down ones ([stunt-positions.md](../../engine/gameplay/stunt-positions.md#leaving)); the lock's arrival detour holds it off when the far end is attached to a [`Helicopter`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/vehicle::vehicle::Helicopter) and the reel's chosen entry is the hang ([`VehicleGrapplePointProxy::IsCurrentEntryHangNode`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/equipment::grapple_point_proxy::VehicleGrapplePointProxy##func-IsCurrentEntryHangNode)). The reel prefers a valid entry on the helicopter's door side and falls back to the hang. A door-side arrival is the game's hijack (Rico at the door, which opens, and a Use press enters), which the detour leaves to the game.

On a hang entry, the line holds Rico at the hang's length below the hook's point, measured against the helicopter's motion, so he rides under it as it flies. A hang that takes hold further out than its length, as one at the reel's distance under a helicopter does, is taken in to it at the pay-out's rate. Under a moving hook the swing across the line is damped toward the hook's own motion over `hang_swing_damping_s`, since the rope damps only along itself and the airframe's turns keep pumping the pendulum; a hang from a still hook swings freely, for the arm to pump.

### Up onto the skid

Y in a hang below a helicopter pulls Rico up onto the skid (`bindings::heli_hang` in `jc3vr-gameplay`). The press drops Y's wingsuit, which would open and end the reel, and ends the hang without letting go of the line (`entering the helicopter`). It points the reel at the door side: each enabled entry of the helicopter's interaction graph with the door-side use tag ([`INTERACTION_USE_TAG_REEL_IN_DOOR_SIDE`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/interaction::INTERACTION_USE_TAG_REEL_IN_DOOR_SIDE)) is chosen in turn and put through the reel's own update of its choice ([`VehicleGrapplePointProxy::UpdateVehicleReelInEntrySelection`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/equipment::grapple_point_proxy::VehicleGrapplePointProxy##func-UpdateVehicleReelInEntrySelection)), which checks the entry's act from Rico's state and works out where it starts, and the one starting nearest him is kept. The update's side check compares an entry with the goal it finds stored, so each is tried with the goal cleared. With no door-side entry standing, the choice is cleared for the reel to select afresh. The resumed reel pulls him to the door side, and the press that started the pull is kept from the game until it is released, so he stops there, and a fresh Y enters from the door-side state. A reel arriving at the hang entry is held off whatever the choice, so Y never ends in the game's inverted hang: at worst Rico is back on the line (`Y below a helicopter arrived at the hang; back on the line`).

Helicopters have no idle on the door side: once the arrival's clip ends, the game drops Rico into the stunt hang through [`ValidateStuntPositionCondition::Evaluate`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/animation::rule_system::ValidateStuntPositionCondition##func-Evaluate). Under the controllers that one transition is refused for the local player, and at the clip's end the skid rail (`vehicles::rail` in `jc3vr-vehicles`) takes him out of the helicopter's interaction graph and stands him on foot on the skid, carried with the airframe and free to walk along the skid's line until he steps off, jumps, lowers himself by the hands, or enters. The rail's module documentation describes its carry, its line, and its exits; its tuning is the `rail` section of the vehicles' tuning. While the rail holds, the game's automatic entry into a stunt area on the vehicle below is refused for him, and during the arrival's clip the jump steps him off without a jump. Standing down vehicle input clears the rail, its remembered airframe, the pending entry, and the automatic-attach block, and the physics callback carries the proxy only while the feature is enabled and the rail belongs to the current local character, so a debug-panel interruption releases the rail. Whether the game's on-foot entry finds the door from the skid, and whether the door-handle grabs open a helicopter's doors, are unverified.

### When the game keeps its own hang

The hold-off applies only while the lock's input tick runs and has not just refused a hang (`lock::hang_takes_over`). The tick does not run while the controller bindings stand idle (the gamepad scheme, no tracked controllers, or the debug UI holding the input), and a refused hang (the line's far end lost or the character's mass unreadable) would be refused again on every tick. In either case nothing would end the reel, so the arrival is left to the game: the ceiling cling or the helicopter's hang, for `hang_refused_ms` after a refusal.

Every way out of a hang lets go of the line (a quick release, a longer one, and a yank), since resuming the reel would only arrive at the ceiling again; the one exception is Y below a helicopter. A landing lets go as for any lock. Perches on walls, the ledge hang, and the stunt positions on cars, boats, and planes are left to the game.

## Shortening the line

Pulling the grapple hand back along the aim it had when the lock took hold takes line in, as far as the hand comes back and no faster than `reel_in_mps`, down to `min_length_m` (`lock::model::TakeIn`). The least length is never below the floor every held line shares with the snatch (`grapple::rope::held_floor`), and taking line in never pays it out (`Rope::shorten`), so raising `min_length_m` while the line is held leaves a shorter line as it is. Reaching out pays nothing out, so hand over hand keeps taking line in. An untracked hand takes nothing in: the lock holds on the button alone. The free hand can take hold of the line ahead of the grapple hand and take line in the same way ([grapple-line-grab.md](grapple-line-grab.md)).

## The target

The rope shares its tension with the body behind the hook's point by inverse mass, and the impulse it gave that body over the reel's ticks is applied to it at the hook's point on the next input tick, outside the physics step. The body is the one the attachment proxy names for a wire constraint, and its mass is its motion's. A parked car's tyres hold more than the line's capped tension, so it barely moves; a helicopter sways; a light object is dragged. A static target, or a body that is not dynamic, takes nothing and is a fixed end. `reaction` scales the share, for tuning rather than as a rule for any kind of target. Characters are never locked onto, so a ragdoll is never flung.

The active wire never holds a native constraint ([grapple-pipeline.md](../../engine/gameplay/grapple-pipeline.md#the-far-ends-proxy)), so the target never takes two rope forces.

## How it ends

| Ending | What follows |
|---|---|
| The button released after a hold, or the jump pressed | The line is let go of with the swing's velocity ([The release](#the-release)) |
| A quick release | The reel resumes. With the grapple's aim on a valid target farther from the line's hook than the gameplay tuning's `bindings.grapple_retarget_m`, the release also fires the hook at it, which the game takes from the reel as a retarget; with the aim on the attached line it fires nothing |
| A yank | The reel resumes toward the hook and its own arrival (a perch, the helicopter's hang, a vehicle's stunt position) |
| Landing from a hang: Rico's proxy stands on a surface after he has left the ground since the hang took hold | The line is let go of, so Rico lands on foot. A held swing does not end on landing: it holds for as long as the button does, and the line drags Rico along the ground when it is taut |
| The line breaks: Rico past `break_stretch_m`, the hook's target gone or no longer live, or a sample the swing needs not finite | The line is let go of, with the snap |
| The reel ends by itself, or the bindings stand down | The lock lets go of the reel, and the game's reel carries on if it can |

### The release

Letting go retracts the active wire, and the reel task cancels itself into the fall on its next update. The reel's cancel scales Rico's velocity to a small fraction a world update after the retract ([grapple-pipeline.md](../../engine/gameplay/grapple-pipeline.md#the-reels-endings)), so the swing's last velocity, the one the lock handed the reel, is given to Rico before the retract and then, carried on under gravity, on every input tick within `handback_for_s` that finds him carrying less than `handback_carried` of it along its direction; the hand-back lands after the cancel whichever tick that falls on. Once he has carried it for `handback_settle_s`, the hand-back stops, and it stops at once when anything else takes his motion over: touching down (so a wall or the ground is not pushed into), a new reel, the wingsuit, or the parachute. The wanted velocity read back from the character reads near zero while the lock drives the reel, so it is only the fallback.

The fall that follows, and a wingsuit opened in it, move him through the air physics ([`NAirMovement_UpdateAirPhysics`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/input::parachute::NAirMovement_UpdateAirPhysics)), which caps the speed at its settings' maximum by rescaling the whole velocity, far below a swing's or a reel's. A release, and any reel that ends with him in the air, therefore carries its speed into that cap (`jc3vr_body::hooks::input::air`): the cap is raised to the carried speed and eased back to the game's over the input tuning's `air.carry_s`, so the fall and the suit keep the speed and lose it as smoothly as the drag allows, and the parachute and the wingsuit open from the swing's exit velocity as usual. The log has `air: carrying a reel's speed into the air physics' cap`.

## The shared rope

The snatch's held line ([grapple-snatch.md](grapple-snatch-swing.md)) is the same rope with the ends' roles swapped: there the grapple hand is the kinematic end and the target it swings is the dynamic one; here the hook's point and Rico are the ends. The snatch keeps its own reel toward its swing length, its strength scaled by the target's mass, and its brace, and hands the rope its length each tick. The two never run at once: the snatch's line needs Rico on foot with its own scripted wire, and the lock needs the reel, which the scripted wire's state never enters.

## Haptics

The lock taking hold plays `Event::LineLock` on the grapple hand, a release or a landing `Event::LineRelease`, and a break `Event::LineSnap` ([haptics.md](haptics.md)). While the lock holds, `Sustain::GrappleTension` follows the line's tension as a share of its strength, and the reel's hum stops.

## Threading

The lock's tick runs in the bindings' game stage, after the tables step (the game thread, before the world update): it applies the target's reaction, takes the lock, ends it, and takes line in. The reel's velocity detour, the arrival detour, and the stuck-check detour run on the game thread's character and hook updates. The lock's state is behind one leaf lock that the input tick and the reel task take in turn, never held across a game call, and an atomic flag tells the detours whether the lock holds ([threading.md](../threading.md)).

## Readouts

The Status page's `Grapple` section has a `Line lock` row: while locked, whether it hangs, whether the take still coasts in, the target's kind and mass, the held length, the stretch, the tension of the line's strength, and the hook's speed; otherwise the counts of locks and breaks and how the last lock ended.

The log has, at info level under the `grapple` target, `grapple lock: the line locks` with the length, the target's kind and mass, Rico's mass, and whether it hangs, `grapple lock: refused` with why, and `grapple lock: the lock ends` with why, the length, and the stretch.

The tracking recorder's `grapple` record carries the lock while it holds ([recorder.md](../testing/recorder.md)): the coast, the held length, the stretch, the tension and the strength, the target's mass, and the hook's point and Rico's position. The Rerun export plots the length, stretch, and tension under `plots/grapple/`.

## Design notes

- The reel task survives a velocity that does not close the distance: it has no timeout and no progress check of its own. The reel's arrival test and the hook's stuck check are the only endings a held line meets, and the lock holds both off rather than avoiding them by distance.
- The reel's velocity is the character's whole motion, so the lock adds gravity itself. The reel's inherited velocity is the attach point's own, so the lock measures the rope against the same point velocity.
- Letting go is a retract alone: the game's reel task answers a detached far end with its own cancel, the same path the game's cancel click takes during a reel.
- The lock waits for the reel task rather than taking hold in the wind-up, where nothing yet drives Rico and a lock would pin him to the ground. It owns the button through the wind-up all the same: a hold that follows the fire's tap lands there, and were the button the game's, the hold would never lock and the game would read it as a dual tether hold, whose release cuts the reel.
- The game's hang is not the base. It is the game's one rope swing with a body, a free capsule joined to the far end by the wire's native spring, but it is entered only through the reel's arrival, at the hook, with the short line its animations expect, and shortening it would mean rebuilding the native constraint as the tether squeeze does.
- No in-world overlay: the game draws the wire itself, and the tension is in the hand.

## Headset checklist

1. Zip toward a high wall, tapping the grapple button, then press it again at once and hold it, before Rico starts moving: nothing locks in the wind-up, and the line locks as the reel carries Rico. Release: Rico flies on (`released`), and no dual tether fires.
2. Zip toward a high wall and hold the grapple button mid-reel: Rico carries on along his path with no jump in speed or direction, and the line goes taut and swings him once he stops closing on the hook; `the line locks` is logged with a plausible length, the lock pulse is felt, and `Line lock` shows coasting in and then the held length. Zip along a level line and lock: Rico falls under the hook and swings on at speed. Zip at a diagonal and lock: nothing drives him into the ground.
3. Let the swing settle: Rico hangs at the held length plus a small stretch, and the grapple hand's hum follows the tension through the swing.
4. Still holding, pull the grapple hand back repeatedly, hand over hand: the line shortens, no faster than `reel_in_mps`, and stops at `min_length_m`. Reach out: nothing pays out.
5. Swing, and let go at the bottom of the arc going forward: Rico flies on with his speed into the fall, carrying on forward rather than dropping straight down, the release pulse is felt, `the lock ends` with `released`, and the parachute opens from the flight.
6. Lock, then tap-release, with the re-reel upgrade bought: the reel resumes and fires again from the reel at the aim. Without the upgrade, the reel simply resumes.
7. Lock, then yank the grapple hand: the reel resumes to the hook and its arrival, boosted if the upgrade is bought; `the lock ends` with `yanked`.
8. Lock low over the ground and let Rico swing down onto it: the line holds while the button does, dragging him along the ground when taut; let go and he lands on foot. From a ceiling hang, drop onto a floor below: the hang ends with `landed`. Press jump while locked: the line is let go of with `released`.
9. Lock onto a parked car from above: the car barely moves. Lock onto a light object on a ledge: it is dragged toward Rico. Lock onto a flying helicopter: Rico keeps his offset under it, the helicopter sways, and `Line lock` shows its speed.
10. Swing hard into a long arc on a short line: if Rico is flung past the break stretch the line snaps with the snap pulse and `broke: overstretched`. Shoot down a helicopter while locked under it: `broke: target gone`.
11. Grapple a character and hold the button during the reel: nothing locks, and the game's own reel and grab behave as before.
12. Brush a wall mid-swing: the swing carries on rather than the reel dropping or reattaching.
13. Zip to a ceiling without touching the button: Rico does not flip into the inverted cling; he drops a couple of metres and hangs upright below the hook, `Line lock` reading hanging. Pump the swing, use the other hand freely, and pull the grapple hand back to climb the line. Tap the button: he drops, and the reel does not carry him back to the ceiling.
14. Zip to a hovering helicopter: Rico hangs upright below the hook and rides with it. Tap the button: he drops. Hang again and press Y: no wingsuit, the lock ends with `entering the helicopter`, the reel pulls Rico up to the door side, and he stands on the skid; press Y again to enter.
15. Switch to the gamepad scheme and zip to a ceiling and to a helicopter: the game's own ceiling cling and stunt hang play.

## Unverified in the headset

- That the character proxy moves at the wanted velocity as it stands while the reel's game-controlled flag is set, with no gravity of its own. A double gravity would show as a swing sagging past the stretch.
- That Rico keeps his velocity through the reel's cancel into the fall, and how the fall's own air control treats it.
- That the proxy's support reading is live during the reel, for the landing.
- That the impulse on a deactivated body wakes it, how a helicopter's flight model and pilot answer a steady pull, and whether the impulse on a vehicle's part reaches the vehicle.
- How the defaults feel: the stiffness, the damping, the strength, and the coast; whether the coast's brake near the hook stops a rush comfortably.
