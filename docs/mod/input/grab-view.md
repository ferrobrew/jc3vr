# The grab view

The grab view draws, around each hand, what the grip and the palms' reach saw on the last tick: the reach's rays and what they hit, a line from the hand to every grip target near it, the palms' state on the wrist, a flash where a grab was refused, and the boxes of the things near the hands that a hand might take, whether or not the reach finds them. It is a diagnostic for telling, in the headset, why a weapon on the ground, a loose object, or a car door is not taken: whether it is a candidate at all, whether a ray reaches it, whether the reach takes what the ray hit, whether the resolver offers it, and whether the subsystem that acts on the grab refused it. The `VR: STATUS` page's `GRIP` rows and the grip press log ([grip.md](grip.md#readouts-and-logs)) say the same in text; the view puts it where the hand is. It is on by default. The code is `payload/src/controllers/physical_hands/grab_view/`.

## Using it

The button at the top of "Grab view" in the debug UI's Controllers tab shows and hides it; the tab is reachable in the headset on the floating panel. No key toggles it. The section sits beside "Collider view" ([collider-view.md](collider-view.md)), and the two can be on together. Its settings are `controllers.physical_hands.grab_view`, under "Grab view" in the tab's settings: the switch (`enabled`, on), switches for each part (`rays`, `targets`, `wrist`, `refusals`, `candidates`), how far from a hand a target out of reach still gets a line (`radius_m`, 2 m), how far from a hand a candidate is drawn (`candidate_radius_m`, 3 m), and the most candidates drawn (`max_candidates`, 24). It is a diagnostic, so it has no pause menu entry ([settings.md](../settings.md)); switching `enabled` off is how a build for players hides it. The view draws through the overlay, so it needs `controllers.overlay.enabled`, and it is taken down while a menu is up.

The readout under the button names each hand's palm state, counts what the last tick drew and what the budget left out, the candidates by kind with the time the last read of the world took, and the refused grabs with the last one's hand and reason.

## What it draws

The overlay has no text, so every state is a colour or a shape.

The reach rays. Each acting palm casts five short rays from the back of the hand through the palm ([physical-hands.md](physical-hands.md)); a hand its body drives casts them from its body. Each is drawn from where it started to where it hit, or to its end when it hit nothing, with a dot at the hit:

| Ray | Colour | What it found |
| --- | --- | --- |
| Missed | faint grey | nothing within the reach |
| Not taken | orange | something a hand does not take: the static world, a body that is not dynamic, too heavy, a weapon not lying dropped, or what the player stands on |
| In reach | green | something a hand takes, offered to the grip |

The door handles. Each door handle within a metre of a free palm is a ring on the handle: bright green when it is within the handle reach and offered, faint green when it is past the reach, and red when it is within the reach but no body was found at it, so it is not offered ([vehicle-doors.md](vehicle-doors.md)).

The lines to the targets. From each hand's grip a line runs to every grip target the resolver saw that tick, of the ones that hand may take, to where its marker is ([grip.md, "Markers"](grip.md#markers)). A target out of reach gets a line only within `radius_m`, and a socket or pouch on the body gets one only once it is in reach, since its own marker already shows it. A target held by the other hand gets none. At most 24 lines per hand are drawn, the nearest first:

| State | Line and marker | Meaning |
| --- | --- | --- |
| Out of reach | dim blue-grey, ring | the hand is not within the target's reach; brighter as the hand nears |
| In reach | pale yellow, ring | the hand is within reach, but another target wins |
| Chosen | magenta, disc | the target a squeeze would take |
| Held | cyan, disc | the hand holds it |

The marker at the target's end grows from 15 mm to 50 mm as the hand comes from `radius_m` away to the target's reach, so an approach is visible before the hand gets there.

The palm marker. A disc over the back of each wrist shows the palms' state for that hand, ringed while the palm acts on the world. Its inputs are those of the grip press log's `palms` field (`physical_hands::palm_readout`):

| State | Colour | Meaning |
| --- | --- | --- |
| Off | grey | the physical hands are off, or the controller scheme is not in effect |
| Not on foot | blue | the palms act only on foot on the ground or falling |
| No tracked hands | dark grey | no hands to act for |
| Off the main thread | red | the aim update ran on another thread than the input tick |
| Not ticking | faint white | the palms have not ticked lately |
| Not tracked | dark grey | this hand is not tracked |
| Not free | yellow | the hand holds a gun, the foregrip, or a grenade or charge, or the animation owns it |
| Holding | cyan, ringed | the hand holds what its grip took from the world |
| Driven | violet, ringed | the hand's body drives it and reaches from where the body is |
| Acting | green, ringed | the palm probes, pushes, and reaches |

The refusals. When a grab chose a target and the subsystem acting on it refused (the direct pickup of a weapon in the world, the take of a weapon from a character's hands, a rack, or a locker, and the physical hands' grip on an object, a door, or a character), a red ring flashes at the target for half a second, growing as it fades. A refusal the physical hands make flashes where the reach met the object; one the weapon handling makes flashes at the marker of the target the hand chose on that input tick.

The candidates. The weapons lying dropped in the world (gold), the loose objects light enough for a hand (white) and those too heavy (red), and the vehicles and the parts loosened from them (green) whose boxes come within `candidate_radius_m` of a hand are drawn as their boxes, with a faint line from each box's middle to the nearest hand. One box is drawn per object, the nearest first. A candidate the reach never finds, or a weapon with no gold box, is the place to look when a grab does nothing.

## Where the data comes from

The palms' simulation tick (`physical_hands::tick::after_aim`) records each hand's reach rays and the door handles near it while the view is on, as the reach casts them (`pickup::reach` and `doors::scan::handle_reach` take a place to record into, and record nothing when it is not given). It publishes them after it has let go of its own state, with the head position it placed them against.

The candidates are read from the physics world on the same tick, outside the physics step, a few times a second rather than every tick: the read goes through the collider view's ([collider-view.md, "Reading the world"](collider-view.md#reading-the-world)) with the static bodies left out, around the middle of the hands, and keeps at most 64 bodies before they are told apart. Each moving body near a hand is named by its owner, the object whose engine instance holds the body, and told apart the way the reach tells what it hits apart: a vehicle or a part of one, a weapon lying dropped, or a loose object with a mass, light enough for a hand or not. A body without an owner, a weapon that is not lying dropped, a body without a mass, and the body the player stands on are not candidates.

The refusals are noted where they are made: the direct pickup's refusal in `pickup::finish_tick`, a take's refusal in `weapons::hold::take::finish_grab`, and the pickup tick's refused grips, published after the tick. The palm state is read on the input tick from the palms' readout.

The input tick draws it all after the grip model's own finish (`bindings::input_tick`), with the grip model's targets and resolution of the same tick, and submits it to the overlay's own slot, which lives a little past one tick.

## Anchoring

Everything is anchored at render time ([render-time-anchoring.md](../rendering/render-time-anchoring.md)). The palm markers and the hands' ends of the lines follow the hands as they are drawn. A target's end of its line is where its grip marker is, anchored as the marker is. The reach rays and their hits ride with the player, so they are anchored to the render-time head at their offset from the head of the tick that cast them, as the palm boxes are. The door handles, the candidates' boxes, and a refusal the physical hands made lie in the world and are world anchors; the candidates are read a few times a second, so a box on something moving lags it by up to one read.

## Cost and caps

The reach records five rays and a few handles per hand per tick, only while the view is on. The read for candidates is one pass over every body slot up to the world's peak id, as the collider view's read is, a few times a second, with at most 64 bodies kept and named; the readout shows its time. The drawing is capped at 1024 primitives a tick: the palm markers, the refusals, the rays, and the handles first, then the target lines, then the candidates, each candidate's box whole or not at all, with what does not fit counted as over budget. The view's slot comes before the collider view's in the overlay, so when the overlay is full the collider view gives way first.

## Threading

Everything runs on the main thread. The view's own state is a leaf lock, taken by the palms' tick after it has let go of its own state, by the refusals where they are made, and by the input tick; the overlay store's and the readout's leaf locks are taken briefly under it. The palm readout, which takes the palms' own state, is read before the view's lock. The debug UI reads the readout's lock on its own thread.

## Headset checklist

1. With the view on (the default), look at a wrist: the palm marker is green and ringed on foot with an empty hand, yellow with a gun in the hand, cyan holding a crate, and blue in a vehicle.
2. Hold an empty hand over the ground: five grey rays fan out of the palm. Lower it onto the ground: the rays that reach it turn orange, with dots where they hit.
3. Hold the hand over a crate: the rays on the crate turn green, a magenta line runs from the hand to the crate's marker, and a squeeze takes it. Try one too heavy: its box is red, the rays orange, and no line.
4. Drop a weapon and look at it: it has a gold box with a faint line to the nearer hand. Reach for it: the rays on it turn green and the line to its marker magenta. If the take is refused, a red ring flashes on it, and the readout names why.
5. Stand beside a parked car: its box is green, and as a hand comes within a metre of a door's handle, a faint green ring sits on the handle, bright once the hand is within reach. A red ring there means no body was found at the handle.
6. Move a hand toward the holster with a gun drawn in the other hand: the line to the holster appears as the hand comes into reach and turns magenta when it would be taken.
7. Turn the view off with the button: everything it drew goes, and the grip markers stay.
