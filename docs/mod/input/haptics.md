# Haptics

Every vibration the mod sends to the controllers is one of a few named primitives, played at a strength within that primitive's range, and every source of one goes through the same mixer. A tick always feels like a tick and a thud like a thud, whatever raised it, and a hand that several sources reach at once feels the strongest of them clearly rather than a saturated buzz. The code is `payload/src/controllers/haptics/`: `vocabulary.rs` (the primitives), `cues.rs` (every source and its cue), `mixer.rs` (what each hand plays), `rumble.rs` (the game's own rumble), and `mod.rs` (the entry points and the readout).

## The vocabulary

Seven primitives. Five play once (one-off cues) and two follow a level that their source sends every tick it lasts (sustained cues).

| Primitive | Kind | Soft | Full | Length | Frequency hint | Priority | Feels like |
| --- | --- | --- | --- | --- | --- | --- | --- |
| tick | one-off | 0.06 | 0.25 | 8 ms | 320 Hz | 0 | a hand crossing into a volume, a detent, a selection moving |
| click | one-off | 0.15 | 0.55 | 20 ms | 250 Hz | 1 | a button, a catch, a round, a release |
| bump | one-off | 0.12 | 0.75 | 40 ms | 160 Hz | 2 | a grip taken, a hand meeting a surface, a stop reached |
| buzz | one-off | 0.2 | 0.6 | 140 ms | 200 Hz | 3 | no: a refused grab, a press that does nothing |
| thud | one-off | 0.4 | 1.0 | 70 ms | 90 Hz | 4 | a shot, a hook taking hold, a body caught, a detonation |
| hum | sustained | 0 | 0.7 | 100 ms hold | 70 Hz | - | the game's rumble, the reel's speed |
| tension | sustained | 0 | 0.6 | 60 ms hold | 130 Hz | - | a pull, a press, a hang |

A cue's strength places it between the primitive's soft and full amplitudes; a source whose cue follows how hard something happened (a palm's speed, a climb's fling, a recoil kick) scales that strength by its share. A sustained cue's amplitude is its strength times the level its source sends, so a level of zero is still. The length is how long a one-off cue plays, and for a sustained one how long a level holds without being sent again. One cue, the detonation, plays a long thud: three and a half times the thud's length. The shot plays for the length the game's fire rumble sets, capped at `max_shot_s`.

The frequency is a hint (OpenXR's `XrHapticVibration::frequency`); a runtime may ignore it. With `frequencies` off, every vibration goes at the runtime's default frequency, as before the vocabulary.

The shapes are settings (`controllers.haptics.vocabulary`), tuned in the debug UI's Haptics section, so a shape changes every cue built on it together.

## The mixer

OpenXR plays one vibration per controller at a time, and a new one replaces whatever is playing. The sources queue their cues from any thread, and the VR frame loop asks the mixer once per rendered frame what each hand plays (`haptics::mix`, from the input update under the runtime lock). The mixer holds per hand the one-off cues queued since the last flush, the cue playing and when it ends, and each sustained source's level and when it expires.

- Sustained sources sum. The sum passes a soft limiter: unchanged up to `mix.sustain_knee` (0.45), then bent smoothly toward `mix.sustain_ceiling` (0.7). However many sustained sources stack (a held load, the game's rumble, the grapple's tension), the hand never reaches full strength from them, and a one-off cue stands out above them. With no cue playing, the hand plays the limited sum for `mix.refresh_s` (100 ms), refreshed at every flush. A source that sends a level of zero ends at once; one that stops sending ends when its hold runs out.
- One-off cues play at the next flush. The cue of the highest priority (then the strongest) among the frame's arrivals leads and sets the length and the frequency. Each other arrival, and a cue still playing, adds `mix.stack_share` (0.35) of its amplitude, and the mod's sustained level adds `mix.duck_share` (0.5) of itself beneath the lead. The total passes the soft limiter at full strength with its knee at `mix.knee` (0.85), so a single cue below the knee plays exactly as shaped and a pile of them never clips. The cue plays for the longer of its own length and what remained of the one it cut into.
- The game's rumble is left out of a one-off cue's onset entirely (it ducks under the mod's cues), and every sustained source is held off a hand while a one-off cue plays there, resuming at the first flush after it ends.
- An arrival weaker than the cue playing and of a lower priority is masked: dropped rather than cutting a heavier cue short (a holster tick during a long burst of fire).

The global strength (`intensity`) scales the mixer's output as it is sent, so it scales every cue the same way, and it never changes the mix.

## Categories and switches

Each source belongs to one category, and each category has its own switch beside the master switch and the strength. A source whose category is off sends nothing, so a switched-off category also stops contributing to the mix.

| Category | Sources |
| --- | --- |
| Contact | the palms and the hand bodies meeting, pressing, and striking things, taking hold and letting go, a door's latch |
| Weapons | shots and kicks, dry fire, holsters and pouches, the foregrip, the true reload's steps, grenades, C4, the weapon wheel |
| Vehicles | the physical controls' stops, detents, and gate, the cockpit buttons, the mounted gun's handles and recoil |
| Traversal | climbing, the stunt positions, the grapple (fire, attach, yank, tether retract, line tension, reel) |
| Interface | the controller chords (the recenter and the debug panel) |
| Game | the game's own rumble, behind `game_rumble` |

The shot and the recoil kick are the weapons category's, and also need `game_rumble`, as they always have: the shot is the game's own fire rumble routed to the firing hand.

The pause menu's `VR: CONTROLS` page has the settings under HAPTICS: `HAPTICS` (`enabled`), `HAPTIC STRENGTH` (`intensity`), `CONTACT HAPTICS`, `WEAPON HAPTICS`, `VEHICLE HAPTICS`, `TRAVERSAL HAPTICS`, and `CHORD HAPTICS` (`categories.*`), `GAME RUMBLE`, `ALSO RUMBLE GAMEPAD` (`gamepad_rumble`), and `PER-CUE FREQUENCIES` (`frequencies`). Everything is on by default except `ALSO RUMBLE GAMEPAD`. The settings live in `controllers.haptics` ([settings.md](../settings.md)); they moved there from `controllers.weapons.haptics`, so a settings file from before the move loads them as defaults once. The per-source switches that came before the categories (the holster and pouch cues, the line tension, the hand bodies' pressure and hit pulses, the palms' contact pulses, the grapple's pulses) and every per-source amplitude and duration are gone: the category switches and the vocabulary's shapes replace them.

## Every source

The one-off sources are `Event` and the sustained ones `Sustain` (`cues.rs`), and the table there is the one below. Strength is the cue's place within its primitive's range at full scale; a source marked scaled multiplies it by its share.

| Source | Raised when | Primitive | Strength | Category | Raised by |
| --- | --- | --- | --- | --- | --- |
| `Event::PalmTouch` | a palm meets a surface, scaled by how fast (a quarter at a touch) | bump | 0.4 | contact | `physical_hands::touch` |
| `Event::HandStrike` | a hand body's contact, or a blow on a character, scaled by the arm's momentum; a held gun's contact by the arm's alone, at most once every `physical_hands.bodies.guns.contact_pulse_interval_s` per hand | bump | 0.4 | contact | `physical_hands::physics::blows` |
| `Event::Take` | a hand takes hold of an object, a character, a door, or a weapon off the ground | bump | 0.6 | contact | `physical_hands`, `physical_hands::pickup` |
| `Event::LetGo` | a hand lets go of what it held | click | 0.25 | contact | `physical_hands` |
| `Event::Unlatch` | a door's latch gives under the hand | bump | 0.9 | contact | `physical_hands` |
| `Event::Shot` | a weapon's fire rumble, on the firing hand; the game sets its strength (raised by the kick) and length | thud | game | weapons | `haptics::rumble` |
| `Event::Kick` | a shot of a weapon with no fire rumble, scaled by its kick | bump | 1.0 | weapons | `haptics::rumble`, from `weapons::fire` |
| `Event::DryFire` | a trigger pull of an empty gun that cannot fire | click | 0.4 | weapons | `weapons::empty`, `weapons::reload::steps` |
| `Event::VolumeEnter` | a hand enters a holster or a pouch | tick | 0.3 | weapons | `weapons` |
| `Event::VolumeGrab` | a draw, a holster, or a foregrip grab or release | click | 0.5 | weapons | `weapons` |
| `Event::Eject` | the magazine or the rounds are ejected | click | 0.6 | weapons | `weapons::reload::steps` |
| `Event::PouchTake` | a round or a magazine comes out of the pouch | click | 0.25 | weapons | `weapons::reload::steps` |
| `Event::PouchEmpty` | a grab at an empty ammunition pouch | tick | 0.75 | weapons | `weapons::reload::steps` |
| `Event::WellCatch` | the well catches a round or a magazine, on both hands | click | 0.4 | weapons | `weapons::reload::steps` |
| `Event::MagazineSeat` | a magazine seats, on both hands | bump | 0.9 | weapons | `weapons::reload::steps` |
| `Event::BeltSeat` | a belt seats | thud | 0.75 | weapons | `weapons::reload::steps` |
| `Event::Rack` | a full pull of the action, on the working hand | click | 0.9 | weapons | `weapons::reload::steps` |
| `Event::ActionDetent` | the action's part meeting a stop of its travel (home, a bolt turned all the way up or down, a slide caught at the rear), on the working hand | click | 0.35 | weapons | `weapons::reload::steps` |
| `Event::Chambered` | a round is chambered, on the gun hand | click | 0.5 | weapons | `weapons::reload::steps` |
| `Event::GrenadePrime` | a grenade is primed in the hand | click | 0.4 | weapons | `weapons::grenade` |
| `Event::GrenadePin` | a grenade's pin is pulled, on both hands | bump | 0.4 | weapons | `weapons::grenade` |
| `Event::GrenadeWarning` | a cooking grenade's fuse warning, faster toward the end | click | 0.75 | weapons | `weapons::grenade` |
| `Event::GrenadeCatch` | a live grenade is caught | bump | 0.75 | weapons | `weapons::grenade` |
| `Event::PouchRefused` | the game refuses a grab at the grenade or C4 pouch | buzz | 0.75 | weapons | `weapons::grenade`, `weapons::c4` |
| `Event::C4Prime` | a charge is primed in the hand | click | 0.4 | weapons | `weapons::c4` |
| `Event::C4Throw` | a charge leaves the hand | click | 0.6 | weapons | `weapons::c4` |
| `Event::C4Stick` | a charge pressed onto a surface sticks | bump | 0.6 | weapons | `weapons::c4` |
| `Event::C4Detonate` | the placed charges detonate, on both hands | thud, long | 1.0 | weapons | `weapons::c4` |
| `Event::DetonatorPress` | the wrist detonator is pressed with charges placed, on both hands | click | 1.0 | weapons | `weapons::c4::wrist` |
| `Event::DetonatorEmpty` | the wrist detonator is pressed with nothing placed | buzz | 0.1 | weapons | `weapons::c4::wrist` |
| `Event::DetonatorNear` | the pressing fingertip comes within the wrist detonator's approach radius, on the pressing hand | tick | 0.4 | weapons | `weapons::c4::wrist` |
| `Event::WheelTick` | the weapon wheel opens or its choice changes | tick | 0.75 | weapons | `weapons::wheel` |
| `Event::WheelCommit` | the weapon wheel's release commits a weapon | click | 0.75 | weapons | `weapons::wheel` |
| `Event::SteeringStop` | the held wheel reaches its lock | bump | 0.6 | vehicles | `vehicles::control_haptics` |
| `Event::SteeringCentre` | the held wheel passes centre | tick | 0.5 | vehicles | `vehicles::control_haptics` |
| `Event::ControlStop` | a held stick or lever reaches a stop | bump | 0.6 | vehicles | `vehicles::control_haptics` |
| `Event::ControlNeutral` | a held lever passes neutral | tick | 0.5 | vehicles | `vehicles::control_haptics` |
| `Event::ThrottleGate` | the held throttle reaches its afterburner gate | bump | 0.9 | vehicles | `vehicles::control_haptics` |
| `Event::ButtonPress` | a cockpit button presses, or a twist grip twists far enough to act | click | 0.75 | vehicles | `vehicles::control_haptics` |
| `Event::ButtonRelease` | a cockpit button or a twist grip is let go | tick | 0.5 | vehicles | `vehicles::control_haptics` |
| `Event::HandleGrab` | a hand takes a mounted gun's handle | click | 0.5 | vehicles | `vehicles` |
| `Event::MountedKick` | a shot of the mounted gun, on each holding hand | bump | 0.6 | vehicles | `vehicles::mounted_recoil` |
| `Event::GrappleFire` | the grapple fires | bump | 0.4 | traversal | `hands::anchor` |
| `Event::GrappleAttach` | the hook takes hold | thud | 0.5 | traversal | `hands::anchor` |
| `Event::Yank` | a yank of the grapple hand, or a yank that boosts the reel | bump | 0.6 | traversal | `bindings`, `grapple::physical` |
| `Event::TetherRetract` | the tethers start to retract under the hand's pull | bump | 0.75 | traversal | `grapple::physical` |
| `Event::ClimbGrip` | a hand grips the world to climb | bump | 0.75 | traversal | `physical_hands::climb` |
| `Event::ClimbRelease` | a climbing hand lets go | click | 0.25 | traversal | `physical_hands::climb` |
| `Event::ClimbFling` | the body is flung off a climbing grip, scaled by the throw | thud | 0.85 | traversal | `physical_hands::climb` |
| `Event::StuntEnter` | a hand takes a stunt position | thud | 0.65 | traversal | `physical_hands::stunt` |
| `Event::StuntRelease` | the hands let go of a stunt position | click | 0.5 | traversal | `physical_hands::stunt` |
| `Event::Chord` | a controller chord the mod acts on (the recenter or the debug panel) | bump | 0.6 | interface | `bindings` |
| `Sustain::PalmPressure` | a palm presses into an object, by its shove's load | tension | 0.5 | contact | `physical_hands::touch` |
| `Sustain::HeldLoad` | a hand pulls or twists what it holds, by the load | tension | 0.5 | contact | `physical_hands::touch` |
| `Sustain::BodyPressure` | a hand body is held back by what it presses, by its lag | tension | 0.5 | contact | `physical_hands::physics` |
| `Sustain::ClimbStrain` | the climbing hands move the body, by its speed | tension | 0.5 | traversal | `physical_hands::climb` |
| `Sustain::StuntHang` | a hand hangs from a stunt position, by the vehicle's speed | tension | 0.35 | traversal | `physical_hands::stunt` |
| `Sustain::GrappleTension` | the grapple hand while the tethers are squeezed, by their tension | tension | 0.75 | traversal | `grapple::physical` |
| `Sustain::GrappleReel` | the grapple hand while the reel carries the character, by its speed | hum | 0.35 | traversal | `grapple::physical` |
| `Sustain::GameRumble` | the game's per-frame motor strength: the low motor on the left hand, the high one on the right | hum | 1.0 | game | `haptics::rumble` |

Each default is rounded onto the primitive its source belongs to. The pressure and tension sources are sustained levels that sum, not short pulses resent every tick, and the softest end of the kick and the palm contact is the bump's soft amplitude.

## The game's rumble

The game's rumble reaches the controllers through two detours ([controllers.md, "Recoil, haptics, dual-wield, and the grapple hand"](controllers.md#recoil-haptics-dual-wield-and-the-grapple-hand)): the weapon's own fire rumble inside a shot of a gun in a hand plays as `Event::Shot` on that hand, and the rumble manager's per-frame motor strength plays as `Sustain::GameRumble`, the low motor on the left hand and the high motor on the right. The gamepad itself is silenced unless `gamepad_rumble` is on. The forward sends every motor value, zero included, so the game's rumble stops on the controllers the frame it stops on the pad.

## Threading

`haptics::play`, `play_scaled`, and `sustain` are called from the input tick, the hand bodies' callbacks and frame tick, the weapon hooks, and the rumble detours. Each reads the settings under the config lock, then takes the mixer's lock, a leaf held only to queue the cue ([threading.md](../threading.md)). The frame loop takes the same lock once per frame to mix, under the runtime lock.

## Readout and log

The debug UI's Controllers tab has a Haptics section: the cues sent per category, the mixer's counts (played, stacked, masked, limited), the amplitude each hand was last sent, and the last dozen one-off cues with their primitive, hand, amplitude, and length. Its settings hold the switches, the vocabulary's shapes, the mixer's rules, and the cue table; the live controller frame has a test button per primitive for each hand, which plays it at full strength whatever the category switches say.

The pause menu's `VR: STATUS` page has `HAPTIC CUES` under FEEDBACK: the cues sent, masked, and limited. The cues themselves are not logged (they are too frequent); the events that raise them are, by their own subsystems.

## In the headset

1. Open `VR: STATUS`: `HAPTIC CUES` reads `WAITING` with nothing sent. Use the debug UI's test buttons on each hand: tick, click, bump, buzz, and thud should feel distinct in weight and length, and hum and tension should feel like a held vibration. If the five one-off primitives feel the same, the runtime is probably ignoring the frequency; try `PER-CUE FREQUENCIES` off and compare.
2. Reach into a holster and out again: a faint tick on entering. Draw: a click. `HAPTIC CUES` counts up.
3. Fire a pistol: a thud on the gun hand only. Fire a long burst from an automatic weapon while reaching into a pouch with the other hand: the burst stays clean on the gun hand, and the pouch tick plays on the other.
4. Hold a gun on its foregrip and fire a burst while the reel or a vehicle's rumble runs: the shots stand out above the rumble rather than merging into it, and the rumble resumes between shots.
5. Press a palm into a crate and push: a tension that rises with the push. Grip a wall to climb and pull yourself up: a bump on the grip, a tension while moving, and a click on letting go. Do both hands at once while the game rumbles (stand near an explosion): the hands buzz together but never at full strength, and `LIMITED` counts up.
6. Slap a wall at speed and then gently: a firm bump, then a faint one.
7. Take the wheel of a car, turn to the lock and back through centre: a bump at the lock and a tick at centre. Press the horn: a click, and a tick on letting go.
8. Grapple to a wall: a bump as it fires and a thud as it attaches. Tether two objects and squeeze: a tension on the grapple hand.
9. Detonate C4: a long thud on both hands.
10. Switch each category off in `VR: CONTROLS` and repeat one of its steps: nothing from that category, while the others still play. Turn `GAME RUMBLE` off: landings and vehicles no longer rumble, and shots no longer pulse. Lower `HAPTIC STRENGTH`: everything weakens together.

## Open questions

- Whether the runtimes in use (SteamVR's Index driver through Monado, and WiVRn) honour the frequency hint, and whether the low hint of the thud and the hum feels weaker than the runtime's default on the Index's actuators.
- Whether the sustained ceiling leaves the game's strongest rumbles (a landing from height, an explosion close by) strong enough, since they sum under it.
