# Grip poses

A hand that holds something wraps its fingers round it instead of taking a generic curl: a pistol grip with the trigger finger on the trigger, a foregrip, a magazine pinched between the thumb and the first fingers, a grenade in the palm, fingers hooked over a ledge, a palm flat on a crate, a wrap sized to a wheel's rim or a rope, a door handle, a fistful of collar. The code is in `payload/src/controllers/fingers/grip/` and `payload/src/controllers/fingers/target.rs`; the finger curls it builds on are in [controllers.md, "Finger curls"](controllers.md#finger-curls).

## Declaring what a hand holds

Each system that puts something in a hand declares it on each of its ticks while the hand holds it, with `fingers::grip::declare(hand, Grip::new(source))`. The source (`GripSource`) names the default pose and how firmly the hold holds. A declaration can name another pose (`Grip::pose`), scale the pose's share while its hold blends in (`Grip::share`), and carry the contact fit's curls (`Grip::fitted`). The finger curls read the declarations once per sim tick, at the end of the pose-finalization pass. A declaration lasts two and a half of those ticks (between 30 and 150 ms), so a source ticking once a frame is never lost between two of them, and a hold that ends, or a source that stands down, lets the hand go without a release call. When two sources declare for one hand, the one earlier in `GripSource::ALL` wins: what the fingers hold over what the hand holds on to, and a palm on a surface last.

| Source | Declared from | Pose | Firmness |
|---|---|---|---|
| Magazine | the weapon handling's input tick, a round in hand for the reload | magazine pinch | held |
| Grenade | the same, a grenade in hand or caught | grenade palm | held |
| C4 | the same, a charge in hand | charge pinch | held |
| Gun | the same, each hand with a gun of the wielded weapon and the second weapon's hand | pistol grip, or the pose of the part a hand carries a long gun by | held |
| Foregrip | the same, the off-hand on the foregrip, at the two-handed grip's weight | the pose of the part it braces the gun at: foregrip on the handguard | held |
| Character | the palms' tick: a character held, or its ragdoll | collar grab | grip |
| Door | the palms' tick: a latched door's handle area, or the door once open | door handle | grip |
| Hinged object | the palms' tick: an articulated object | 40 mm bar | grip |
| Loose object | the palms' tick: a loose object, or a flat palm on one the hand cannot lift | wrap (flat palm) | grip |
| Stunt handhold | the stunt positions' input tick, a hand holding its grip in a helicopter's hang or a climbing grip on a helicopter ([stunt-positions.md](stunt-positions.md)) | 50 mm bar | grip |
| Climb | the climb's input tick, a hand gripping the world or a moving body | ledge hook | grip |
| Wheel, handlebars, stick, lever | the vehicles' input tick, a hand on a physical control | 35, 32, 38, and 40 mm bars | held |
| Parachute handle | the vehicles' input tick | 25 mm bar | held |
| Mounted gun | the vehicles' input tick, each hand the handles anchor, at the anchor weight | 35 mm bar | held |
| Grapple line | the grapple anchor's input tick, the grapple hand anchored to the line, at the anchor weight | 15 mm bar | held |
| Surface | the palms' tick, an empty palm pressing into a surface | flat palm | held |

A held hold (the weapons, the vehicles' controls, the grapple line) stays in the hand after the grip lets go, or the source ends it itself, so it takes the whole pose. A grip hold has the pose's `light_share` at the lightest squeeze and the whole pose from `firm_squeeze`, so squeezing harder closes the hand onto the thing.

## The poses

Each pose (`catalogue.rs`) gives every finger a curl, a splay, and a hook, and the thumb an opposition and a spread. The hook moves a finger's bend along it: a positive hook straightens the knuckle and bends the outer joints further, as over a ledge, and a negative one bends the knuckle and keeps the outer joints straight, as in a flat pinch. The opposition turns the open thumb out of the palm's plane toward the palm side, up to `max_opposition_deg`, so the thumb meets the fingers round a grip. A hand on a long gun takes the pose of the part it holds ([physical-guns.md, "Grabbing a gun anywhere"](physical-guns.md#grabbing-a-gun-anywhere)): the pistol grip on the grip, the foregrip on the handguard, a 30 mm bar on the barrel, a 45 mm bar on the stock, the magazine cup (the palm under the magazine, the fingers cupped round it) on the magazine, and the slide pinch (the thumb against the bent index and middle fingers, the others tucked) on the action. A bar's pose follows its diameter: the fingers' curl is interpolated between judged points, from a fist round a rope to an open hand round a beam, and never closes further on a thicker bar.

Every finger plays one of three parts in a pose:

- Posed: it takes the pose's curl, or the contact fit's where the fit found the held body.
- Trigger: the pistol grip's index finger. On the trigger it rests on it and closes with the trigger's pull over the trigger's short travel; lifted off the trigger, it keeps the measured curl, so the player can point it along the slide.
- Live when tracked: the thumb on a bar or a flat palm. With hand tracking (Index finger tracking) it keeps the measured curl, since the thumb moves freely on a wheel's rim; with the estimate from the inputs it takes the pose.

The pose values are judged by eye, not measured, and are the first thing to tune in the headset.

## The blend

The hand's shape is its measured curls blended toward the pose by the pose's share (`blend.rs`). The share follows its target with `blend_s`, so taking hold closes the fingers onto the pose and letting go opens them back to the measured curls; the shape is then smoothed with the curls' own `smoothing_s` and bent as the curls are, by the hand's tier weight.

With the grip poses off (`grip.enabled`), the earlier behaviour holds: a held hold closes the hand into a fist, and every other hold keeps the measured curls. With `grip.weapons` off, a gun or the foregrip keeps the game's grip animation, and only the trigger finger is driven (every finger with `drive_all_when_holding`), because the game's grip clips per weapon already wrap the fingers round the gun.

## Contact fit

A hand holding a body (a loose object, a hinged object, an open door, or a ragdoll's body) closes each finger until it touches the body (`contact.rs`). The finger curls publish the hand's proportions each tick they write the fingers: each finger's root and bone lengths in the palm's frame, from the animated bones (the distal bone's length is taken as 0.8 of the middle bone's). From the palms' tick, each finger is swept in the world through the same bend the curls give it, from straight toward a fist in five steps, casting the chords its fingertip and middle joint move along. The first chord that meets the held body ends the sweep at the curl where it met it; a cast that meets something else (the hand's own body, the world) is passed over, and a finger that meets nothing keeps the pose's curl. The wrist the sweep starts from is the tracked wrist moved as the rendered hand is to the grab point.

The casts are bounded: two fingers per hand per tick (at most twenty casts), so a fresh hold is fitted over three ticks, and a fitted hold is swept again only every half second, since the hand holds the body still in its own frame. A new body or pose starts the fit over. The casts run on the game thread in the palms' tick, outside the physics step, with the player aim's ray filter.

## Settings and readouts

The settings are `controllers.fingers.grip`: `enabled`, `weapons`, and `contact_fit` (all on), `light_share`, `firm_squeeze`, `blend_s`, and `max_opposition_deg`. The pause menu's `VR: CONTROLS` page has `FINGERS WRAP WHAT THE HAND HOLDS` and `FINGERS WRAP GUNS`; the debug UI's finger section has the rest, and shows per hand the source, the pose, the mode, the share, and the fitted fingers, and each finger's hook.

`VR: STATUS` has a `FINGERS` section with `LEFT GRIP POSE` and `RIGHT GRIP POSE`: the pose and its source, the share, the fitted fingers, and `LETTING GO` while a released pose opens; `GAME'S GRIP` for a gun left to the game; `HOLDS NOTHING`; `WAITING` while the controllers do not drive the hand; `FAILED` when the finger bones do not resolve. A change of a hand's source is logged at info (`fingers: the hand's grip source changed`, with the hand, the old and new source, and the pose), at most twice a second per hand; the source a hand settles on within that interval is logged when it passes, with the count of the changes in between.

## Unverified in the headset

- The pose values: every curl, hook, and opposition is a judgement.
- That the tracked wrist (`CockpitHand::wrist`) is where the rendered hand's wrist bone sits, closely enough for the contact fit's sweep to start at the rendered fingers.
- That a cast from the fingers does not meet the hand's own physics body before the held body, which would leave those fingers at the pose's curl.
- That the pistol grip's fingers sit on the gun's grip without clipping, given the rigid grip that places the gun in the hand.

## Headset checklist

1. Draw a pistol. The fingers wrap its grip and the thumb crosses over them; touching the trigger rests the index finger on it, pulling closes it a little, and lifting off points it. `LEFT GRIP POSE` or `RIGHT GRIP POSE` reads `PISTOL GRIP FROM GUN, 100%`, and the log has one `grip source changed` line for the draw.
2. Turn `FINGERS WRAP GUNS` off: the gun takes the game's grip, the row reads `GAME'S GRIP`, and only the trigger finger moves. Turn it back on.
3. Take a rifle's foregrip with the off-hand: the off-hand wraps the handguard with the thumb along its side, blending in with the two-handed grip.
   With `GRAB GUNS ANYWHERE`, brace the rifle on its barrel, magazine, action, and stock in turn: a tight wrap round the barrel, a cup under the magazine, a pinch on the action, and a looser wrap round the stock; `LEFT GRIP POSE` names each pose from `FOREGRIP`. Carry the rifle by its barrel alone: that hand's row names the bar from `GUN`.
4. Take a round or a magazine from the pouch: a pinch between the thumb and the first fingers. A grenade sits in a spread palm; a C4 charge between the thumb and straight fingers.
5. Pick up a small loose object and squeeze lightly, then hard: the fingers close onto it as the squeeze grows, and the row counts the fitted fingers within a few ticks. Fingers that sit in the air rather than on the object point at a wrong wrist or proportions.
6. Grab a heavy crate one hand cannot lift: the palm lies flat on it. Press a palm into a wall with the hand empty: the palm flattens.
7. Climb a ledge: the fingers hook over it with the knuckles straight.
8. Grip a car door's handle, then hold the open door: the door-handle pose. Take hold of a character: a fistful of collar.
9. Drive a car, a bike, a helicopter, and a plane with the physical controls: a wrap round the rim, the bars, the stick, and the lever. With hand tracking, the thumb on the rim follows the real thumb.
10. Hold the parachute handles, a mounted gun's handles, and the grapple line while reeling: each wraps its bar, the line closest to a fist.
11. Let go of each: the fingers open back to the controller's curls over a short motion, and the row reads `LETTING GO` briefly, then `HOLDS NOTHING`.
12. Turn `FINGERS WRAP WHAT THE HAND HOLDS` off: holds on vehicles, mounted guns, and the line close into a fist, and everything else keeps the controller's curls.
