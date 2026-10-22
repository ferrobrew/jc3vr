# Stunt positions by grip

The game's stunt positions, standing on a vehicle's roof and hanging from a helicopter ([engine side](../../engine/gameplay/stunt-positions.md)), are held by the player's own grip. A hand that grips a helicopter with the body hanging below it takes the player into the helicopter's hang, and in the hang, letting go of every grip lets go of the helicopter. A mantle onto a vehicle's roof takes the player into the roof's stunt area, where the hands are free. Clinging to a vehicle's side is the moving climb ([hand-climbing.md](hand-climbing.md#moving-bodies)), which carries the player until the game takes over. The code is `payload/src/controllers/physical_hands/stunt/`. It is on by default (`physical_hands.stunt.enabled`, in the debug UI's Controllers tab under "Stunt positions") and needs the hand climbing and its moving grips. It has not yet been seen working in the headset.

## How the game holds a stunt position

A stunt position is an attachment through the vehicle's interaction graph: the vehicle carries the character rigidly, and the rule system plays the stunt idles. So once the game holds the player there, the mod has nothing to carry; it needs only to enter the position at the right moment and to leave it when the grips let go. Both are the game's own: the entry is [`Character::AutoAttachToClosestStuntAreaOnVehicle`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/character::character::Character##func-AutoAttachToClosestStuntAreaOnVehicle), or for a roof first [`Character::AutoAttachToStuntAreaOnVehicleBelow`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/character::character::Character##func-AutoAttachToStuntAreaOnVehicleBelow), which the game's own on-foot movement calls for a character that lands on a vehicle; and the exit is the game's jump.

Each input tick, after the climb's, the stunt positions read which position the game holds the player in ([`Character::IsInStuntPosition`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/character::character::Character##func-IsInStuntPosition), then [`IsStuntHanging`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/character::character::Character##func-IsStuntHanging) for the hang, or [`IsInStuntArea`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/character::character::Character##func-IsInStuntArea) outside a stunt traversal for standing), and decide one thing to do (`stunt::solve`, pure and unit-tested). The input tick runs on the game thread outside the world update, where the predicates and the entries are safe to call ([threading.md](../threading.md)).

## Hanging from a helicopter

A climbing grip on a moving body ([hand-climbing.md](hand-climbing.md#moving-bodies)) whose owner is a [`Helicopter`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/vehicle::vehicle::Helicopter) is a grip on a stunt handhold: a skid, or the airframe's edge. While its grip is held, the body's centre is at least `stunt.hang_below_m` below the gripped point (the player hangs from it rather than climbing over it), and that has lasted `stunt.hang_hold_s`, the hand takes the player into the helicopter's hang. Until then the climb carries the player with the helicopter as with any moving body, so a brief grab while climbing past does not hang. The entry plays `Event::StuntEnter` ([haptics.md](haptics.md)) on each gripping hand and logs `stunt positions: the grip took the player into the helicopter's hang`. The game puts the hang where its interaction places it, which is likely a fixed point of the airframe rather than where the hand gripped; the player may move to it.

In the hang, a grip held by either hand holds on. Both hands are offered the stunt hold as a fallback grip target (`GrabVolume::StuntHold`, [grip.md](grip.md)), so a grip pressed there takes the hold. Once a grip has held in the hang, opening every grip for `stunt.release_grace_s` lets go (`stunt.let_go`): the hang asks the game for its jump, which leaves the hang, plays `Event::StuntRelease` on both hands, and logs `stunt positions: every grip opened, so the hang lets go`. A hand moved from one grip to the other inside the grace keeps the hang. Which input leaves the hang is not established ([engine side](../../engine/gameplay/stunt-positions.md#leaving)); if the game keeps the player there a second after the jump, the log warns `stunt positions: the game kept the player in the hang after the let-go's jump`, and the game's own controls still leave it.

A hang the grip did not enter, such as one the grapple reeled the player into, waits for the player to take hold: until a grip has held in it, open hands do not let go, so the player is never dropped for not having held on yet.

While hanging, each hand that holds its grip sends `Sustain::StuntHang` every tick in proportion to the helicopter's speed, full at `stunt.hold_full_speed_mps`: the wind and the airframe's pull.

The hand tiers treat the hang as a state of its own, `stunt hang`, yielded to the animation ([controllers-and-roomscale.md](controllers-and-roomscale.md)): the hang's animation holds the airframe with Rico's hands, which is where the player's grip is. The procedural body leaves the hang to the animation, as every vehicle state.

## Standing on a roof

A mantle that lands on a moving body ([hand-climbing.md](hand-climbing.md#mantling)) is reported to the stunt positions with the body and where the feet landed. When the body is a vehicle's and the feet are within `stunt.roof_offer_m` of its nearest stunt area ([`GetNearestStuntArea_World`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/module/vehicle::stunt_areas##func-GetNearestStuntArea_World)), the mantle takes the player into the stunt area (`stunt.enter_roof`), plays `Event::StuntEnter` on both hands, and logs `stunt positions: the mantle took the player into the vehicle's stunt area`. The entry first asks for the area below the player, which keeps the heading; when the game does not see a vehicle below, it asks for the nearest area to the feet, which also turns the player to the vehicle's heading, a turn of the view. If the game's own on-foot movement attaches the player first, the stunt positions find the player already standing there and do nothing.

Standing on a roof needs no grip: the game holds the player, and the stick moves the player over the roof through the game's stunt locomotion. The hands are free and act as on foot: a hand may grip a roof rail or the door frame, which the climb holds as below, or take a gun.

With the procedural body in the full mode, standing in a stunt area is the case `standing on a vehicle`: the torso is procedural on the stunt idle's hips, posed as in a seat ([procedural-states.md](../body/procedural-states.md#standing-on-a-roof)), and the legs keep the idle's balance on the moving roof, which is safer than stepping the feet on it (`stunt_torso`, on by default; `TORSO ON VEHICLE ROOFS` on the `VR: BODY` page). A stunt traversal (a step onto the next area, a step up or down) and the hang stay the animation's.

## The climb while the game holds the player

While the game holds the player in a stunt position, the climb moves nothing: the hand bodies' pre-simulation step reads that the game holds the player and gives the proxy neither a velocity nor a support. A climbing grip then only holds its hand where it took hold, carried with the vehicle, so a hand on a roof rail or a skid stays there, and letting go keeps no fling. When the game takes the player into a stunt position, every grip taken before it lets go (`hand climbing: a grip was let go` with `the game took the player into a stunt position`): those grips were taken for a body the game has since moved. The climbing case of the procedural body does not apply while the game holds the player.

## Finger poses and haptics

A hand that holds its grip in the hang, and a climbing grip on a helicopter, declare the stunt handhold grip ([grip-poses.md](grip-poses.md)): the fingers round a bar of `stunt.skid_diameter_m`, a skid. A grip on a roof's edge, a rail, or a door frame is the climb's, and hooks the fingers over it. The cues are the entry's, the let-go's, and the hang's level above, and the climb's own grip and release cues for the grips it takes, all of the traversal category.

## Settings and readouts

The settings are `physical_hands.stunt`, under "Stunt positions" in the debug UI's Controllers tab, with the readout beside the climb's: the position the game holds the player in, whether a grip has held in this hang, the hangs and roofs entered, the hangs let go, the entries refused, the last entry's kind, and for each hand whether it holds the hang or grips a helicopter.

The `STUNT POSITIONS` status row, under `HANDS IN THE WORLD`, shows the position (`HANGING, HOLDING ON`, `HANGING, TAKE HOLD` before a grip has held, `HANGING, LETTING GO`, or `ON A ROOF`) with the counts, and a note naming the last refused entry. The `ROOF TORSO` row, under `PROCEDURAL BODY`, shows the torso's weight on a roof. The recorder's signals are `stunt.hanging`, `stunt.standing`, `stunt.hang_entries`, `stunt.roof_entries`, `stunt.releases`, `stunt.refusals`, and `procedural.roof`, and the template is `testing/scenarios/stunt_positions.toml`.

The log has, at info level, `stunt positions: the game holds the player in a stunt position` with the position, `the player left the stunt position`, the two entries above with the grip's point and how far the body hangs or the feet's distance from the area, `every grip opened, so the hang lets go`, and `the game refused the entry` with the reason; the warning above when the game keeps the hang.

## Headset checklist

With `physical_hands.stunt.enabled`, `physical_hands.climb.enabled`, and `physical_hands.climb.moving` on, empty-handed:

1. Walk up to a parked helicopter and grip a skid with one hand, then let the body hang below it (crouch, or lift the feet off the ground by pulling): after a moment, `the grip took the player into the helicopter's hang`, a strong pulse, and `STUNT POSITIONS` says `HANGING, HOLDING ON`. Report where the hang puts the player against where the hand gripped, and whether the view jumps.
2. In the hang, the rendered hands are the hang animation's on the airframe, and the fingers of a held grip close round a bar (the `LEFT GRIP POSE` and `RIGHT GRIP POSE` rows say `stunt handhold`). Have the helicopter lift off and fly: the hands feel a hum that grows with its speed.
3. Swap hands on the grip: let one go and take it again with the other inside a fifth of a second; the hang stays.
4. Open both grips over water: `every grip opened, so the hang lets go`, a pulse, and the player drops. Report if `the game kept the player in the hang` appears instead, and which game input then leaves it.
5. Grapple onto a helicopter so the reel ends in the hang, with the grips open: `HANGING, TAKE HOLD`, and the player stays. Grip, then open both: the player drops.
6. Grip a parked car's side, climb it, and mantle onto the roof: `the mantle took the player into the vehicle's stunt area` (or the game's own attach, with `the game holds the player in a stunt position` and no entry line), and `STUNT POSITIONS` says `ON A ROOF`. Report whether the view turned on the entry.
7. On the roof with the hands free and the car driven off: the player stays on it, the stick moves over the roof, and with `BODY FROM YOUR HEAD` on `FULL` the case is `standing on a vehicle`, `ROOF TORSO` reads `OK`, and leaning moves Rico's back while his legs keep the idle's balance. Grip the roof's edge: the hand stays on it and the body does not move.
8. Jump off the roof with the game's jump: `the player left the stunt position`, and the climb grips nothing it held.
9. With `physical_hands.stunt.enabled` off, gripping a helicopter's skid is the moving climb only, and the hang's open grips drop nothing.

## Unverified in the headset

- Everything above: that the grip reads the helicopter's owner through its body, that the game accepts the hang's entry from a grip, and where the hang puts the player.
- That the game's jump leaves the hang ([engine side](../../engine/gameplay/stunt-positions.md#leaving)).
- That the game sees a vehicle below a player whose climb wrote the vehicle's body as the proxy's support, so that the roof's entry keeps the heading; otherwise the fallback turns the view.
- Whether the hang reads as on foot to the mode detection, so that the hand bodies act there; the climb's grips hold only where they do.
- How the torso on a roof looks against the stunt idle's balance at speed.
