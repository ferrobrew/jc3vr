# Grapple lock

While the reel carries Rico toward the hook, the player can lock the line's length and swing from it. Holding the grapple button during the reel locks the line; pulling the grapple hand back shortens it; a yank resumes the reel; letting go of the button lets go of the line, and Rico flies on with his momentum. The line is a spring the player can work with, measured against the hook's point's own motion, so a lock onto a moving vehicle or a helicopter keeps Rico at a mostly fixed offset from it. The target feels the line's pull back.

The implementation is `payload/src/grapple/physical/lock/`, with the rope shared with the snatch in `payload/src/grapple/rope.rs` and the button's gestures in `payload/src/controllers/bindings/grapple_button.rs`. The engine side is in [grapple-pipeline.md](../../engine/gameplay/grapple-pipeline.md), "The reel's motion and endings". The rest of the physical grapple is in [physical-grapple.md](physical-grapple.md).

## The button

The lock owns the grapple button while the reel task carries Rico (the mode detector's reeling-in traversal, not the wind-up before it) along a line whose far end is attached to anything but a character, on the controller scheme with the lock switched on. The button's gestures, read before the game sees the button:

| Press | Effect |
|---|---|
| Press | Locks the line at once, at the distance Rico is at, so the tap window does not cost several metres at reel speed |
| Release within 0.25 s | Unlocks and resumes the reel, and passes the tap on to the game, which fires again from the reel when its upgrade is bought |
| Release after a longer hold | Lets go of the line: Rico flies on |
| A yank while held | Unlocks and resumes the reel, and the yank boosts it as during any reel |

The yank is the reel's boost yank ([physical-grapple.md](physical-grapple.md#the-reel)), so it resumes the reel only while pull to reel is on; without it, the button's release is the way out. The lock hears the yank directly from the physical grapple, whatever the button's gesture is doing, and the button only spends its press so that the release afterwards does nothing. The game never sees a held fire input during the reel, so the hold cannot start a dual tether. The game's re-fire reads the fire input's press edge, so the passed tap still fires; it fires at the release rather than the press, and aims where the grapple's aim points then. A quick tap whose reel ended under it (a landing or a break) passes nothing on, so it cannot fire a fresh line. A double tap still releases tethers and does nothing to the reel's line. With the far end on a character, or the lock switched off, the button reaches the game as before.

## The swing

The lock stays inside the game's reel. The reel task keeps running, so the reel's animation (the arm thrust along the line, which reads as holding the rope), its turn of Rico toward the line, the [grapple comfort](../body/grapple-comfort.md) hold, and the anchored grapple hand all carry on. The detour on the reel's velocity update hands the reel the lock's velocity instead of the game's (`lock::drive`), and the reel's arrival and stuck check are held off while the lock holds (`lock::hooks`): the reel's arrival would end a swing near the hook, and its stuck check would drop or reattach Rico the moment he brushed a wall, since a held line never closes on the target.

Each reel tick, `lock::model::swing`:

1. adds gravity to Rico's velocity, which the reel's velocity carries none of;
2. while the brake runs, removes Rico's speed toward the hook's point over `lock.brake_s`, keeping the speed across the line, which becomes the swing, and lets the held length follow Rico in;
3. steps the shared rope between the hook's point and Rico.

The rope (`grapple::rope::Rope`) is tension only: a spring-damper on the stretch past the held length, with the damping on the ends parting along the line, normalised by the ends' reduced mass so that the same stretch asks the same acceleration whatever the masses, and capped at a tension. The hook's end moves with the attach point's own velocity, as the attachment proxy reports it, so the relative motion is measured against the target: on a static wall it is a pendulum, and under a helicopter the line holds Rico at its distance from the point in the helicopter's frame.

| Setting | Default | Meaning |
|---|---|---|
| `lock.stiffness_per_s2` | 65 | A 15 cm stretch at rest under one gravity; about 1.3 Hz, soft enough to feel and to pump |
| `lock.damping_per_s` | 11 | About 0.7 of critical |
| `lock.max_accel_g` | 4 | The line's strength, in multiples of gravity on Rico's mass: no catapult from a fast relative motion |
| `lock.break_stretch_m` | 3 | How far past the held length the line stretches before it breaks |
| `lock.reel_in_mps` | 3 | The fastest the hand takes line in |
| `lock.min_length_m` | 1.5 | The shortest the line is held at |
| `lock.brake_s` | 0.25 | How long the brake takes to stop the rush toward the hook |
| `lock.reaction` | 1 | The share of the line's pull the target feels |

Rico's mass is his proxy body's, read from its motion when the lock takes hold; a lock whose mass cannot be read is refused, and the button's tap still goes to the game.

## Shortening the line

Pulling the grapple hand back along the aim it had when the lock took hold takes line in, as far as the hand comes back and no faster than `lock.reel_in_mps`, down to `lock.min_length_m` (`lock::model::TakeIn`). The least length is never below the floor every held line shares with the snatch (`grapple::rope::held_floor`), and taking line in never pays it out (`Rope::shorten`), so raising `lock.min_length_m` while the line is held leaves a shorter line as it is. Reaching out pays nothing out, so hand over hand keeps taking line in. An untracked hand takes nothing in: the lock holds on the button alone.

## The target

The rope shares its tension with the body behind the hook's point by inverse mass, and the impulse it gave that body over the reel's ticks is applied to it at the hook's point on the next input tick, outside the physics step. The body is the one the attachment proxy names for a wire constraint, and its mass is its motion's. A parked car's tyres hold more than the line's capped tension, so it barely moves; a helicopter sways; a light object is dragged. A static target, or a body that is not dynamic, takes nothing and is a fixed end. `lock.reaction` scales the share, for tuning rather than as a rule for any kind of target. Characters are never locked onto, so a ragdoll is never flung.

The active wire never holds a native constraint ([grapple-pipeline.md](../../engine/gameplay/grapple-pipeline.md#the-far-ends-proxy)), so the target never takes two rope forces.

## How it ends

| Ending | What follows |
|---|---|
| The button released after a hold | The active wire is retracted; the reel task cancels itself into the fall on its next update, and Rico keeps his velocity, from which the parachute and the wingsuit open as usual |
| A quick release | The reel resumes, and the tap goes to the game |
| A yank | The reel resumes toward the hook and its own arrival (a perch, the helicopter's hang, a vehicle's stunt position) |
| Landing: Rico's proxy stands on a surface after he has left the ground since the lock took hold | The line is let go of, so Rico lands on foot rather than being dragged |
| The line breaks: Rico past the break stretch, the hook's target gone or no longer live, or a sample the swing needs not finite | The line is let go of, with the snap |
| The reel ends by itself, the lock is switched off, or the bindings stand down | The lock lets go of the reel, and the game's reel carries on if it can |

## The shared rope

The snatch's held line ([grapple-snatch.md](grapple-snatch.md)) is the same rope with the ends' roles swapped: there the grapple hand is the kinematic end, and the target it swings is the dynamic one; here the hook's point and Rico are the ends. The snatch keeps its own reel toward its swing length, its strength scaled by the target's mass, and its brace, and hands the rope its length each tick. The two never run at once: the snatch's line needs Rico on foot with its own scripted wire, and the lock needs the reel, which the scripted wire's state never enters.

## Haptics

The lock taking hold plays `Event::LineLock` on the grapple hand, a release or a landing `Event::LineRelease`, and a break `Event::LineSnap` ([haptics.md](haptics.md)). While the lock holds, `Sustain::GrappleTension` follows the line's tension as a share of its strength, and the reel's hum stops.

## Threading

The lock's tick runs in the bindings' game stage, after the tables step (the game thread, before the world update): it applies the target's reaction, takes the lock, ends it, and takes line in. The reel's velocity detour, the arrival detour, and the stuck-check detour run on the game thread's character and hook updates. The lock's state is behind one leaf lock that the input tick and the reel task take in turn, never held across a game call, and an atomic flag tells the detours whether the lock holds ([threading.md](../threading.md)).

## Settings and readouts

The switch and the tuning are in the debug UI's Grapple feature, under Movement/vehicles: lock the reel's line (`controllers.grapple.lock.enabled`, on by default) and the sliders for the settings above.

The Status page's `GRAPPLE` section has a `LINE LOCK` row: while locked, whether the brake still runs, the target's kind and mass, the held length, the stretch, the tension and the line's strength, and the hook's speed; otherwise the counts of locks and breaks and how the last lock ended.

The log has, at info level under the `grapple` target, `grapple lock: the line locks` with the length, the target's kind and mass, and Rico's mass, `grapple lock: refused` with why, and `grapple lock: the lock ends` with why, the length, and the stretch.

The tracking recorder's `grapple` record carries the lock while it holds ([recorder.md](../testing/recorder.md)): the brake, the held length, the stretch, the tension and the strength, the target's mass, and the hook's point and Rico's position. The Rerun export plots the length, stretch, and tension under `plots/grapple/`.

## Design notes

The lock follows the prescan's first design, inside the reel task, after the engine checks it needed:

- The reel task survives a velocity that does not close the distance: it has no timeout and no progress check of its own. The reel's arrival test and the hook's stuck check are the only endings a held line meets, and the lock holds both off rather than avoiding them by distance.
- The reel's velocity is the character's whole motion, so the lock adds gravity itself. The reel's inherited velocity is the attach point's own, so the lock measures the rope against the same point velocity.
- Letting go is a retract alone: the game's reel task answers a detached far end with its own cancel, the same path the game's cancel click takes during a reel.
- The lock waits for the reel task rather than taking hold in the wind-up, where nothing yet drives Rico and a lock would pin him to the ground.
- The game's hang was not taken as the base. It is the game's one rope swing with a body, a free capsule joined to the far end by the wire's native spring, but it is entered only through the reel's arrival, at the hook, with the short line its animations expect, and shortening it would mean rebuilding the native constraint as the tether squeeze does.
- No in-world overlay: the game draws the wire itself, and the tension is in the hand.

## Headset checklist

With the lock on (the default):

1. Zip toward a high wall and hold the grapple button mid-reel: Rico stops rushing in within a quarter of a second and swings below the hook; `the line locks` is logged with a plausible length, the lock pulse is felt, and `LINE LOCK` shows `BRAKING` briefly and then the held length.
2. Let the swing settle: Rico hangs at the held length plus a small stretch, and the grapple hand's hum follows the tension through the swing.
3. Still holding, pull the grapple hand back repeatedly, hand over hand: the line shortens, no faster than about 3 m/s, and stops at 1.5 m. Reach out: nothing pays out.
4. Swing, and let go at the bottom of the arc going forward: Rico flies on with his speed into the fall, the release pulse is felt, `the lock ends` with `released`, and the parachute opens from the flight.
5. Lock, then tap-release within a quarter of a second, with the re-reel upgrade bought: the reel resumes and fires again from the reel at the aim. Without the upgrade, the reel simply resumes.
6. Lock, then yank the grapple hand: the reel resumes to the hook and its arrival (a perch or a hang), boosted if the upgrade is bought; `the lock ends` with `yanked`.
7. Lock low over the ground and let Rico swing down onto it: the line is let go of on touching down, with `landed`, and Rico lands on foot.
8. Lock onto a parked car from above: the car barely moves while Rico swings. Lock onto a light object hung or lying on a ledge: it is dragged toward Rico. Lock onto a flying helicopter: Rico keeps his offset under it as it flies, the helicopter sways, and `LINE LOCK` shows its speed.
9. Swing hard into a long arc on a short line: if Rico is flung past the break stretch the line snaps with the snap pulse and `broke: overstretched`. Shoot down a helicopter while locked under it: the line breaks with `broke: target gone`.
10. Grapple a character and hold the button during the reel: nothing locks, and the game's own reel and grab behave as before.
11. Brush a wall mid-swing: the swing carries on rather than the reel dropping or reattaching.
12. Switch the lock off: holding the button mid-reel does what it did before.

## Unverified in the headset

- That the character proxy moves at the wanted velocity as it stands while the reel's game-controlled flag is set, with no gravity of its own (from the symbol dump's character states). A double gravity would show as a swing sagging past the stretch.
- That Rico keeps his velocity through the reel's cancel into the fall, and how the fall's own air control treats it.
- That the proxy's support reading is live during the reel, for the landing.
- That the impulse on a deactivated body wakes it, and how a helicopter's flight model and pilot answer a steady pull.
- How the defaults feel: the stiffness, the damping, the strength, and the brake.
- The vehicle proxy's body for a part: whether the impulse reaches the vehicle through the part.

## Not yet done

- A gamepad mapping (holding the grapple bumper mid-reel).
- Paying line out by reaching out.
- Listing the lock among the wrist's essential controls during the reel.
