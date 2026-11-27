# Grapple lock

While the reel carries Rico toward the hook, the player can lock the line's length and swing from it. Holding the grapple button during the reel locks the line, pulling the grapple hand back shortens it, a yank resumes the reel, and letting go of the button lets go of the line, so Rico flies on with his momentum. The line is a spring measured against the hook point's own motion, so a lock onto a moving vehicle or a helicopter keeps Rico at a mostly fixed offset from it, and the target feels the pull back.

The code is `grapple::physical::lock` in `jc3vr-grapple`, with the rope in `grapple::rope` and the button's gestures in `controllers::grapple_button`. The tuning is the `lock` section of `crates/jc3vr-grapple/tuning/grapple.toml` (`LockTuning` documents each key, [tuning.md](../tuning.md)). The engine side is [grapple-pipeline.md, "The reel"](../../engine/gameplay/grapple-pipeline.md#the-reel). The lock acts only under the controller scheme and has no switch.

## The button

The lock owns the grapple button while the hook reels in on a line attached to anything but a character, from the hook's reeling-in state on, which includes the reel's wind-up. It takes hold only once the reel task carries Rico: a lock in the wind-up, where nothing drives him yet, would pin him to the ground. A press in the wind-up arms it, and a hold that follows the fire's tap is the lock's from the tick the reel starts.

A press locks at the distance Rico is at. A release within `button.tap_s` resumes the reel; a longer hold's release lets go of the line. A yank while held resumes the reel and boosts it as during any reel ([physical-grapple.md](physical-grapple.md#the-reel)).

The game never sees a held fire during the wind-up or the reel: a fire held past the game's hold time on the reel's line would arm the dual tether, whose release ends the reel. A quick release fires nothing onto the attached line, which could release it. With the far end on a character, the button reaches the game as before.

## The swing

The lock stays inside the game's reel, so the reel's arm animation (which reads as holding the rope), its turn of Rico toward the line, the [grapple comfort](../body/grapple-comfort.md) hold, and the anchored grapple hand all carry on. The detour on the reel's velocity update hands the reel the lock's velocity (`lock::drive`), and the reel's arrival and stuck check are held off while the lock holds (`lock::hooks`): the arrival would end a swing near the hook, and the stuck check would drop Rico the moment he brushed a wall. The reel task has no timeout or progress check of its own, so those two are the only endings a held line meets.

Each reel tick, `lock::model::swing` adds gravity (the reel's velocity is the character's whole motion and carries none), and steps the rope between the hook's point and Rico. The rope (`grapple::rope::Rope`) is tension only: a spring-damper on the stretch past the held length, normalised by the ends' reduced mass, capped at `max_accel_g` of Rico's weight so a fast relative motion never catapults him. The hook's end moves with the attach point's velocity as the attachment proxy reports it, which is also the velocity the reel inherits. The snatch's swinging line uses the same rope with the grapple hand as the kinematic end; the two never run at once, since the snatch needs Rico on foot and the lock needs the reel.

### The take

Taking hold changes nothing about Rico's motion (`lock::model::Held::take`). While he still closes on the hook's point he coasts on his momentum and the held length follows him in, so the line goes taut when his approach ends, with no jerk. His speed across the line is never turned: a zip up toward a high hook carries on up, and a rush at a level hook falls under it.

A line cannot swing a fast rider on a short length, since it turns his speed at speed squared over length and would stretch past its strength and break. So the coast works out the shortest length the line can bear his swing on with `coast_strength_share` of its strength, and brakes along the line alone to stop there. The speed kept is also capped so the swing reaches its bottom with the line at its strength. Rico's mass is his proxy body's; a lock whose mass cannot be read is refused, and the tap goes to the game.

## The hang below a ceiling or a helicopter

A reel onto a ceiling ends in the game's upside-down cling ([`NStateTask_ReelIn::FinishReel`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/equipment::grappling_hook::NStateTask_ReelIn##func-FinishReel) starting [`GrapplingHook::StartReelUpsideDown`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/equipment::grappling_hook::GrapplingHook##func-StartReelUpsideDown)), a pose the player's body cannot match. A detour holds that start off for the local player, and the lock takes the line itself with no press (`lock::Hold::Hang`), paying out at `pay_out_mps` to `hang_length_m` with Rico at rest, as the game's cling clears his velocities. He hangs upright on the reel's animation, the grapple hand pumps and takes line in, and the other hand is free. Near the hook the reel's arrival test passes every tick and skips the reel's motion ([`NStateTask_ReelIn::UpdateMotion`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/equipment::grappling_hook::NStateTask_ReelIn##func-UpdateMotion)), so the arrival detour runs that motion itself; otherwise Rico would freeze under the hook.

A reel onto a [`Helicopter`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/vehicle::vehicle::Helicopter) whose chosen entry is the stunt hang ([`VehicleGrapplePointProxy::IsCurrentEntryHangNode`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/equipment::grapple_point_proxy::VehicleGrapplePointProxy##func-IsCurrentEntryHangNode), [stunt-positions.md](../../engine/gameplay/stunt-positions.md#leaving)) is hung the same way, measured against the helicopter's motion so he rides under it. A door-side arrival is the game's hijack and is left to it. Under a moving hook the swing across the line is damped toward the hook's motion over `hang_swing_damping_s`, since the airframe's turns keep pumping the pendulum.

Y in a hang below a helicopter pulls Rico up onto the skid (`bindings::heli_hang` in `jc3vr-gameplay`). It drops Y's wingsuit, which would end the reel, and re-points the reel at the door side: each entry with [`INTERACTION_USE_TAG_REEL_IN_DOOR_SIDE`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/interaction::INTERACTION_USE_TAG_REEL_IN_DOOR_SIDE) is put through [`VehicleGrapplePointProxy::UpdateVehicleReelInEntrySelection`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/equipment::grapple_point_proxy::VehicleGrapplePointProxy##func-UpdateVehicleReelInEntrySelection) with the stored goal cleared (its side check compares against it), and the one starting nearest Rico is kept. Helicopters have no door-side idle, so at the arrival clip's end the game would drop him into the stunt hang through [`ValidateStuntPositionCondition::Evaluate`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/animation::rule_system::ValidateStuntPositionCondition##func-Evaluate); that transition is refused, and the skid rail (`vehicles::rail` in `jc3vr-vehicles`, whose module docs describe it) stands him on the skid, carried with the airframe, until he steps off, jumps, lowers himself, or enters. Whether the on-foot entry finds the door from the skid is unverified.

The hold-off applies only while the lock's tick runs and has not just refused a hang (`lock::hang_takes_over`): with the bindings idle (the gamepad scheme, no controllers, or the debug UI holding input) or after a refusal, nothing would end the reel, so the game keeps its cling or hang. Every exit from a hang lets go of the line, since resuming the reel would only arrive at the ceiling again, except Y below a helicopter.

## Shortening the line

Pulling the grapple hand back along the aim it had when the lock took hold takes line in, no faster than `reel_in_mps`, down to `min_length_m` and never below the floor every held line shares (`grapple::rope::held_floor`). Reaching out pays nothing out, so hand over hand keeps taking line in. An untracked hand takes nothing in.

## The free hand on the line

While the lock holds or the snatch's line swings ([grapple-snatch.md](grapple-snatch.md#swinging-on-the-line)), the other hand can take hold of the line ahead of the grapple hand (`grapple::line_grab`). `line_grab::targets` places one target, `GrabVolume::GrappleLine`, when that hand is free and empty and its palm is near the line from the grapple hand's wrist to the hook's point; the `line` section of the tuning sets the distances, and the near limit leaves the wrist to the snatch's brace. It is ranked like any grab target, and a squeeze latches the grip on the line (`GripLatch::Grab(GrappleLine)`) until it opens. It is the one thing the snatch's reserved catch grip may still take.

On the lock's line, the hand's pull along the line takes line in as the grapple hand's does (`lock::model::FreeHand`), each new hold measuring from where it took hold; both hands' pulls add. Only the grapple hand pumps the swing.

On the snatch's line, the holding hand's palm becomes the anchor (`Brace::step_line`), the line shortened by how far ahead it took hold, so pulling back draws the target in and swinging the hand pulls the target the new way. Letting go hands the line back to the grapple hand without paying anything out (`line_grab::handed_back_m`), and neither change of anchor is read as hand motion (`Sling::regrip`). Two hands on the line are as strong as a brace, and the brace and the hold never run together. The line is held at the palm, so a hand drawn off the line still anchors it.

## The target

The rope shares its tension with the body behind the hook's point by inverse mass, and its impulse over the reel's ticks is applied there on the next input tick, outside the physics step. A parked car barely moves, a helicopter sways, and a light object is dragged; a static or non-dynamic body is a fixed end. `reaction` scales the share. Characters are never locked onto. The active wire never holds a native constraint ([grapple-pipeline.md](../../engine/gameplay/grapple-pipeline.md#the-far-ends-proxy)), so the target never takes two rope forces.

## How it ends

A hold's release or the jump lets go with the swing's velocity. A quick release resumes the reel, and with the aim on a valid target farther from the hook than `bindings.grapple_retarget_m` it also fires at it, which the game takes as a retarget. A yank resumes the reel to its own arrival. Landing ends a hang, but a held swing holds for as long as the button does and drags Rico along the ground. Overstretch past `break_stretch_m`, a lost target, or a non-finite sample breaks the line. The reel ending by itself or the bindings standing down release the reel to the game.

Letting go retracts the active wire, and the reel task cancels itself into the fall. The reel's cancel scales Rico's velocity to a small fraction a world update later ([grapple-pipeline.md](../../engine/gameplay/grapple-pipeline.md#the-reels-endings)), so the swing's last velocity is handed back on input ticks within `handback_for_s` while he carries less than `handback_carried` of it, and stops once he touches down or anything else takes his motion over. The fall's air physics ([`NAirMovement_UpdateAirPhysics`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/input::parachute::NAirMovement_UpdateAirPhysics)) caps speed far below a swing's, so `jc3vr_body::hooks::input::air` raises the cap to the carried speed and eases it back over `air.carry_s`.

## Feedback and threading

`Event::LineLock`, `Event::LineRelease`, and `Event::LineSnap` play on the grapple hand, and `Sustain::GrappleTension` follows the line's tension ([haptics.md](haptics.md)). There is no in-world overlay: the game draws the wire, and the tension is in the hand. The Status page's `Line lock` row shows the lock, the log has `grapple lock: the line locks`, `refused`, and `the lock ends` under the `grapple` target, and the recorder's `grapple` record carries it ([recorder.md](../testing/recorder.md)).

The lock's tick runs in the bindings' game stage; the reel, arrival, and stuck-check detours run on the game thread's character and hook updates. Its state is behind one leaf lock never held across a game call, with an atomic flag telling the detours whether it holds ([threading.md](../threading.md)).

## Unverified in the headset

- That the proxy moves at the wanted velocity with no gravity of its own while the reel's game-controlled flag is set.
- That the proxy's support reading is live during the reel, for the landing.
- That the impulse wakes a deactivated body, and how a helicopter's flight model answers a steady pull.
- How the defaults feel, the coast's brake near the hook in particular.
